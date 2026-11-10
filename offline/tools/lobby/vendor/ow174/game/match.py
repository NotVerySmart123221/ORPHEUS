"""A match: its players, their entities, and what each player's client is told.

How a player gets into the world, as ProCore got it working in the Practice Range (their live tests;
the create path, 20302 and the loading gate re-read in IDA):
- When the map is loaded the client sends 21601 (and later 21602). 0.3 s later the server sends
  20302 {the player entity}: "this is your entity" (0x7FF7896E6C10 writes world +0x298). It has to
  come before the player's create, or the client does not add its command components (59, 63).
- 0.6 s after loading, one frame creates the map's server-owned placeables (the loading screen only
  ends once they all exist), the player entity (000E.003, flags 0x3C) and the game mode entity
  (16B2.003).
- Then the player's entity gets its controller's statescript (heroselect.py): the HUD and the hero
  select screen. That frame puts the entity's statescript in owner mode, so small owner frames keep
  telling the client which of its commands the server has (CmFD), else it replays old key presses.
- The hero's body comes with the pick, as in retail (OW2 traffic): the body with its movement state
  and an update of the player entity whose component 44 possesses it. A switch replaces the body
  under a new id in the same frame (the client ignores a destroyed id for 30 s).
- The client then moves the body itself; it never tells the server where it is. The server moves
  a copy from the commands, and that copy is what the other players see.
- In the PvP modes the first spawn starts "assemble your team": 30 s shown on the hero select
  screen, which stays open after a pick and closes when the time is up.
"""

import logging
import math
import random
import secrets
import time
from collections import deque
from dataclasses import dataclass, replace
from functools import partial

from ow174.game import heroselect, world
from ow174.game.bits import BitWriter
from ow174.game.commands import CROUCH, FIRE, JUMP, SECONDARY, Command, CommandQueue
from ow174.game.content import PRACTICE_RANGE, SOLDIER, SPAWN_BODY, GameMap, Hero, heroes, pilot, spawn_point
from ow174.game.mover import GRAVITY_SCALE, JUMPED, TURN, quantise
from ow174.game.pools import body_pools

DEAD = 0x2000  # movement-state flag: dead, the 3P animation's dead pose (LizRollin's combat.py)
KILLED_BY = 2  # 25004 +0x78: "YOU WERE KILLED BY <name>"; 0 "YOU DIED" (LizRollin's combat.py)
from ow174.game.statescript import (ANNOUNCER, ANNOUNCER_STATE, OBJECTIVE, OBJECTIVE_MODE, ROUND_COMPLETE,
    ROUND_COMPLETE_STATE, Asset, Bool, Entity, Float, Graph, Instance, Int, StateBound, Stream, message_state, owner_ack, remote_full_frame,
    variables_frame)
from ow174.game.world import (
    OP_CREATE,
    OP_DESTROY,
    OP_UPDATE,
    PLAYER_PATH,
    WITH_PARTICIPANT,
    WITH_SCALE,
    WITH_TRANSFORM,
    NO_INPUT,
    EntityUpdate,
    Movement,
)
from soldier import (FIELD_ENTITY, FIELD_HEAL, FIELD_PERIOD, FIELD_RADIUS, FIELD_RELEASE, FIELD_SECONDS, HELIX_IMPACT, HELIX_PROJECTILE, HELIX_RANGE, HELIX_DEFINITIONS,
    HELIX_SPEED, SPRINT_BONUS, ULT_CHARGE, ULT_POINTS, VISOR_BUTTON, VISOR_CAST, VISOR_CONE, VISOR_SECONDS, Rifle, RifleUpdate,
    anim_payload, body_frame, field_visual_frame, health_variables, helix_splash, helix_states, rifle_damage, spawn_voice, ult_meter,
    voice, DVA_CALL_MECH_VOICE, DVA_EJECT_VOICE, DVA_SELF_DESTRUCT_VOICE)
from kits import (KITS as WEAPON_KITS, AIR_JUMP_AFTER, AIR_JUMPS, Aimed, BASTION, Crouched, BASTION_REPAIR_BLEND, BASTION_SENTRY, DVA, DVA_ULTS, Held, dva_call_mech,
                  dva_self_destruct, MERCY, MERCY_BLASTER, MOIRA,
                  MOIRA_GRASP_STATES, SCOPE_HEROES, SOMBRA, WIDOWMAKER, ZENYATTA, SOMBRA_HACK_STATES, alt_states, bastion_body,
                  MERCY_BOOST, MERCY_HEAL, MERCY_STAFF_STATES, frame_builder, mercy_staff_states, moira_grasp_states,
                  sombra_hack_states, Timed, phased)
from assault import ROUND_SECONDS, Assault, on_point
from combat import aim_direction
import board
import body_vm
import onfire
import client_sync
import shadow_step
import owner_correction
from player_mover import PlayerMover, body_values, frame_flight, frame_mods
import hitbox
import hitbox_feed
import nameplates
import nav
import viewer
from nav import nav_points
from target import (ABILITY_HITS, ABILITY_VOLLEYS, AUTOMATIC, WEAPON_EFFECTS, WEAPON_MODELS, WEAPON_SHOTS, ATTACK_DAMAGE, BASTION_SENTRY_WEAPON, BODIES, BOT_HERO, DIFFICULTY, GRASP_CONE,
    GRASP_DAMAGE, GRASP_HEAL, GRASP_REACH, GRASP_TICK, HEAL_PERIOD, STAFF_HEAL, STAFF_REACH, STAFF_TICK,
    HERO_KIT, MATRIX_CASTS, MATRIX_IGNORES, MELEE, MERCY_BLASTER_WEAPON, ORB_TICK, ORBS, PAYLOAD, PLAYER_KIT, WEAPON_GRAPHS, RIFLE_RANGE, Projectile,
    REBUILD, REBUILD_SLOT, TrainingTarget, BOOST_DAMAGE, BOOST_KNOCK, BOOST_REACH, KNOCK_SECONDS, CALL_MECH_DAMAGE,
    CHARGE_BUMP, CHARGE_RUN_UP, CHARGE_STOP_SPEED, STRIKE_DAMAGE,
    CALL_MECH_AHEAD, CALL_MECH_CAST, CALL_MECH_DROP, CALL_MECH_RADIUS, EJECT_BACK, EJECT_LAUNCH, EJECT_RISE, EJECT_SECONDS, SELF_DESTRUCT_DAMAGE, SELF_DESTRUCT_FUSE, SELF_DESTRUCT_RADIUS,
    ability_splash, armour_cut, chest, eye_height, hero_bots, hero_name, matrix_intersects, omnic_bots, patrol_points, payload_target,
    kill_feed, range_bots)

log = logging.getLogger("ow174.game")

PLAYER_DEFINITION = 0x040000000000000E  # 000E.003, the player (controller) entity
GAME_MODE_DEFINITION = 0x04000000000016B2
GAME_MODE_ENTITY = 0xD0000010
BASTION_RECONFIGURE = 0x08
BASTION_REPAIR_RATE = 90.0  # Graph 339, ModifyHealth state 0.
BASTION_REPAIR_CAPACITY = 300.0
BASTION_REPAIR_DELAY = .5  # Graph 339 entry, var1357.
BASTION_REPAIR_RECHARGE = BASTION_REPAIR_CAPACITY / 7.0
PLAYER_FLAGS = WITH_PARTICIPANT | PLAYER_PATH | WITH_TRANSFORM | WITH_SCALE
BODY_FLAGS = WITH_TRANSFORM | WITH_SCALE
DROP_SLOT = 0xFF  # the player's entity id for D.Va's called mech while it falls (Match.change_form)

# Component 26, the filter bits: 0x800000 << team picks the team's physics proxy (team 0 or 1). The
# client's own session builder gives its player 0x10040001 | that bit (0x7FF78973950E), but
# 0x10000000 is the spectator bit (GetEntityIsSpectator, 0x7FF789B2F000): ProCore's player with it
# got the spectator's HUD panel, and their bodies with only it had no collision and no enemy
# outline. So the player gets 0x00040001 | the team bit, a body only the team bit.
TEAM_BIT = 0x800000
PLAYER_FILTER = 0x00040001
SHOT_LINGER = 0.5  # s a landed bot shot projectile stays stopped before its destroy; shortcut: not tuned
UNDER_MAP = (0.0, -1000.0, 0.0)  # where a hitscan shot's projectile is made: only its record draws it (LizRollin's CREATE_AT)
HEALTH = 200.0  # a hero missing from target.PLAYER_KIT

RETAIL_QUANTA = 16000  # microseconds per command frame; 20300 must name the client's own value
TOURNAMENT_QUANTA = 7000  # tank_TournamentModeTickRate, which tournament mode turns on

# Seconds after the client reports its map loaded.
SEND_OWN_ENTITY = 0.3
SEND_GAME = 0.4
SEND_WORLD = 0.6
SEND_CONTROLLER = 0.7  # a frame after the entity exists
OWNER_ACK_EVERY = 0.1  # seconds between owner frames that only move CmFD on
SOLDIER_DRAW_DELAY = 2.0
KITS = (SOLDIER, *WEAPON_KITS)  # heroes with a served weapon kit; other picks play Soldier
WEAPON_PULSE_SECONDS = 1 / 30
# A cast's states stay on this long after the Volley's stop frame: the client applies owner frames at its own pace
# and skipped the 33 ms start/stop pulses outright (no state ever activated); held, the cast animation plays.
# shortcut: one length for all; the authored animation lengths are not read.
CAST_HOLD = 1.0
COOLDOWN_REFRESH = 0.5  # shortcut: the sweep steps every 0.5 s; what refreshes var61 natively is not traced
# Practice vs AI bot steering. shortcut: all four values are guesses; no 1.74 AI data has been read.
CHASE_SECONDS = 6.0  # how long the bots hunt where an enemy was last seen
LEASH = 30.0  # defenders hunt only sightings this close to the point
SPREAD = 4.0  # radius of the ring the bots spread on round their goal
STEER_SECONDS = 0.5
REPATH = 3.0  # metres a goal moves before a bot re-paths
RETARGET_SECONDS = 0.2  # how often a bot re-picks its target and re-checks the sight line (each check ~0.05 ms)
ALLIES = 5  # hero bots on the player's team: six against six with the player (wiki)
STRAFE = 2.0  # shortcut: metres an engaged bot sidesteps per steer, a guess; the 1.74 AI's dodging is not read

# "Assemble your team" in the PvP modes: their mode graphs (0F4A, 0EC2) set the game mode entity's
# v1303 to 30 and count it down; 288B shows it on the hero select screen as long as it is open.
ASSEMBLE_SECONDS = 30
COUNTDOWN = 1303
OBJECTIVE_MODES = {0x0230000000000014, 0x0230000000000015, 0x0230000000000016}  # Assault, Escort, Hybrid
# Scripted-loading modes (content.SCRIPTED_LOADING_MODES): their mode graphs each have one remote-synced marker state
# (class 9FC136D7, no fields) that the normal mode graphs lack. The client's loading screen (loader state 9) ends
# only once an active state of that class exists, so the countdown frames keep it on from the first frame after the
# create. Mode -> (graph, the marker's remote slot).
UPRISING_MODE = Graph(0x0D28, 78, 7, 42, frozenset({10, 21, 22, 23, 24, 31, 38}))
LOADING_GATES = {
    # shortcut: the other two graphs' counts are graph_layout.py's and unverified in game
    0x023000000000000F: (UPRISING_MODE, 14),  # Uprising
    0x023000000000001A: (UPRISING_MODE, 14),  # Uprising (the other card)
    0x0230000000000025: (Graph(0x100B, 114, 7, 61, frozenset({19, 53, 54, 55, 56, 57})), 12),  # Retribution
    0x0230000000000008: (Graph(0x0CE4, 93, 5, 58, frozenset({37, 47, 48, 50, 51, 52, 53, 54})), 35),  # Mei's Snowball Offensive
}
# The mission's HUD (research/uprising-mission.md): a UXPresenter attaches its widget to the presenter whose handle
# sits in the variable it names; the objective widgets name var 19450, published by the frame graph 106B, which the
# client creates under the HUD bootstrap graph 0CB6. Assault's widget graph starts 0CB6 itself (client-only); the
# PvE mode graphs start it from a server-owned SubScript state, so the gate frame carries it as a child, with the
# objective widget and the mission director whose remote slot 0 posts the message (74E.025) that opens the widget.
HUD_BOOTSTRAP = Graph(0x0CB6, 1, 0, 1)
PRACTICE_MODE = Graph(0x13C0, 20, 3, 10)
# The Tab board (research/lizrollin-runtime.md section 22): the client's HUD graph 0CC9 turns var 6699 on while it has
# game message 06DF.025, and a Stack keeps the board (01BE) shut until then. Only a mode graph's remote 38EE1100 state
# posts that message, so each mode frame turns one on. The slots are tools/assets/board_modes.py's: 13C0's here,
# the mode graphs of the modes that have no mode frame of ours otherwise (sent for this alone), and the PvE gates' by
# graph. Each mode here was looked at in the game: the board opens and nothing else shows.
# shortcut: left out are 0x67 (its graph 29A6 also puts an "UNSPECIFIED 0.0" objective header up) and the modes not
# run (Lucioball 0x20, Yeti Hunt 0x29, Junkenstein 0x03 and 0x2A); board_modes.py prints their lines.
PRACTICE_BOARD = 4
BOARD_MODES = {
    0x0230000000000007: (Graph(0x0CE8, 118, 16, 80, frozenset({65, 67, 72, 73, 74, 75, 76, 77, 25, 58, 59, 60, 61, 30})), 1),
    0x0230000000000009: (Graph(0x0D0B, 143, 14, 85, frozenset({34, 66, 68, 37, 69, 57})), 6),
    0x0230000000000010: (Graph(0x0D76, 48, 14, 28), 2),
    0x0230000000000017: (Graph(0x0EC2, 147, 26, 98, frozenset({32, 71, 73, 74, 46, 47, 48, 82, 83, 84, 85, 86, 23, 61, 62})), 1),
    0x023000000000001D: (Graph(0x0F60, 72, 3, 42, frozenset({32, 33, 35, 36, 37, 38, 39, 18, 21})), 0),
    0x023000000000001E: (Graph(0x0F73, 75, 6, 46, frozenset({34, 35, 39, 40, 41, 42, 43, 17, 20})), 0),
    0x0230000000000046: (Graph(0x1A59, 143, 13, 81, frozenset({32, 66, 35, 54, 62})), 6),
    0x023000000000005A: (PRACTICE_MODE, 4),
    0x0230000000000070: (Graph(0x0F73, 75, 6, 46, frozenset({34, 35, 39, 40, 41, 42, 43, 17, 20})), 0),
}
GATE_BOARD = {0x0D28: 0, 0x100B: 0, 0x0CE4: 5}
# Seconds a dead hero waits: what the mode graph sends every body script (0C8E) as game message 06FD.025: the
# objective modes' 0F4A its var20680 (one SetVar: 10), the Practice Range's 13C0 a 3 (LizRollin's combat.RESPAWN).
# shortcut: other modes (PvE, Skirmish) keep the Range's 3; 0EC2 (a 10) is not one of ours.
RESPAWN_SECONDS, OBJECTIVE_RESPAWN_SECONDS = 3.0, 10.0
UPRISING_WIDGET = Graph(0x0E95, 1, 0, 1)
UPRISING_DIRECTOR = Graph(0x0D4A, 136, 1, 10, frozenset({4, 5, 6, 7, 8}))  # DEFEND THE PAYLOAD: title + v1303 timer
UPRISING_ESCORT = Graph(0x0D4B, 33, 3, 7, frozenset({2, 4}))  # ESCORT THE PAYLOAD: title + progress bar
# 0E95's entity var: the widget shows that entity's health as PAYLOAD HEALTH; its "REMAINING" timer is v1303.
DEFENDED = 2190
DEFEND_SECONDS = 240.0  # the real mission's DEFEND THE PAYLOAD phase (research/uprising-mission.md)


def pve_instances(mode_guid, defended=False, escort=False):
    """The gate frame's instances beside the mode graph (instance 1): mode -> its HUD children and the phase's
    director, which draws the objective title (and the defend timer) itself; its slot 0 opens the payload health
    panel once there is something to defend (its health text is formatted only when it opens). The escort
    director takes a new instance index so the client destroys the defend one with its widgets.
    shortcut: the escort bar stays at 0 (the stand-in does not move); Retribution's 100B and Snowball's 0CE4
    SubScript 0CB6 states not yet looked up."""
    if mode_guid not in (0x023000000000000F, 0x023000000000001A):
        return []
    index, director = (5, UPRISING_ESCORT) if escort else (4, UPRISING_DIRECTOR)
    return [Instance(2, HUD_BOOTSTRAP, parent=(1, 12)), Instance(3, UPRISING_WIDGET, parent=(1, 31)),
            Instance(index, director, active={0: message_state()} if defended else {})]


# Graph 0F4A's remote states that post the objective widget's phases (game messages 692, 701, 694), with v1303
# as the seconds left. Team 0 attacks.
PHASE_ASSEMBLE, PHASE_SETUP, PHASE_ATTACK, PHASE_ROUND_COMPLETE = 5, 8, 11, 24
# Each phase's own state that posts 06DF.025 (the Tab board, above): its neighbour in 0F4A's phase subgraph.
PHASE_BOARD = {PHASE_ASSEMBLE: 4, PHASE_SETUP: 7, PHASE_ATTACK: 10, PHASE_ROUND_COMPLETE: 26}
OBJECTIVE_INDEX, OVERTIME = 10548, 3392  # mode vars: the point under attack (1 reads "Attack objective B"); 1 plays the overtime banner
SCORE_LEFT, SCORE_RIGHT = 6, 7  # 0C35's presence bits for vars 7899 and 7901: the round-complete screen's blue | red score
SHOW_PROGRESS, PROGRESS = 4, 3  # 0C35 bits for vars 2198 (not 0: a "CAPTURE PROGRESS" row) and 8018 (its value, 0..1)
ASSAULT = 0x0230000000000014
LEAVE_GAME = 20304  # reliable, {+0x78: 0} = leave for good: the client goes back to the menu
RESULT_SECONDS = 8  # shortcut: how long the round-complete screen shows before the menu; no victory/defeat screen yet
SETUP_SECONDS = 45  # shortcut: not from the mode data or the wiki; check against an Assault wiki revision.
COUNTDOWN_EVERY = 1.0  # the screen shows whole seconds
BOARD_EVERY = 0.5  # seconds between updates of the Tab board's numbers (LizRollin's STATS_EVERY)
HEAL_EVERY = 0.2  # seconds between the pulses of a Timed ability's heal (kits.Ability.heals)
# The chat line "<name> switched to <hero> (was <hero>)." (LizRollin's notices.py @ 9f8f3bc, MIT). The PvP controller
# graph 0C90 posts it when three of its variables arrive in one controller frame: the old hero (v4513), the new one
# (v4514) and the switching player's entity (v4515), whose component 29 gives the name. Sent again, or straight after
# other values, the client posts it again, so they go in one frame only, to a client whose last controller frame had
# none and has arrived. Only the modes whose controller is 0C90 have the line.
SWITCH_MODES = {0x0230000000000010, 0x0230000000000014, 0x0230000000000015, 0x0230000000000016, 0x0230000000000017,
                0x023000000000005A, 0x0230000000000067}
OLD_HERO_BIT, NEW_HERO_BIT, SWITCHER_BIT = 4, 6, 7  # 0C90's presence bits of the three
NOTICE_GAP = 0.1  # seconds after a controller frame before the next: the line's events have to run first


def _yaw_units(degrees: float) -> int:
    return round(degrees * 65536 / 360)


def _rotation(yaw_units: int) -> tuple[float, float, float, float]:
    half = yaw_units * math.pi / 65536
    return (0.0, math.sin(half), 0.0, math.cos(half))


class Player:
    def __init__(
        self, match: "Match", slot: int, account_lo: int, name: str, hero: Hero, team: int, quanta: int
    ):
        self.match = match
        self.slot = slot
        self.account_lo = account_lo
        self.name = name
        self.hero = hero
        self.team = team
        self.quanta = quanta
        self.entity = 0xA0000000 | (slot + 1) << 8
        self.body = self.entity | 1
        self.has_body = False  # not before the first pick
        self.health = self.max_health
        self.discorded = (0.0, 0.0)  # (until, extra damage taken) from an enemy Zenyatta bot
        self.respawn_at = None
        self.skin = (0, False)  # the body's (skin theme, golden weapon)
        self.client = None  # the connected client (server.Client), None before it joins or after it left
        self.loaded_at: float | None = None
        self.steps_done = 0
        self.spawned = False
        teammates = sum(1 for other in match.players if other.team == team)
        self.spawn = spawn_point(match.game_map, team, teammates)  # (position, yaw in degrees)
        self.mover = PlayerMover(self.spawn[0], _yaw_units(self.spawn[1]), match.world, hero.body, team)
        self.commands = CommandQueue()
        self.last_input = (0, 0, 0)
        self.last_command = None
        # Other players' bodies this player's client has: body -> False in the frame of its create.
        self.seen: dict[int, bool] = {}
        self.script = Stream(self.entity)  # the player entity's statescript: the controller
        self.body_script = None
        self.body_vm = None  # body_vm.BodyVM: the VM runs the body's statescript (then body_script stays None)
        self.body_create = None  # the own body's create record: owner_correction waits for it to arrive
        self.body_picked_at: float | None = None
        self.rifle = Rifle(quanta, PLAYER_KIT.get(hero.name, (0, None))[1])
        self.mercy_blaster_rifle = None
        self.bastion_other_rifle = None
        self.bastion_phase, self.bastion_switch_at = 0, 0.0
        self.bastion_repair_phase, self.bastion_repair_at = 0, 0.0
        self.bastion_repair_charge, self.bastion_repair_tick = BASTION_REPAIR_CAPACITY, None
        self.bastion_heal_at = 0.0
        self.widow_charge_from, self.widow_charge_serial = None, 0
        self.widow_charge, self.widow_charge_next = 1.0, 0.0
        self.next_rifle_update = 0.0
        self.weapon_extra_states = {}
        self.hack_target = None
        self.hack_completes_at = self.hack_ready_at = 0.0
        self.grasp_target, self.grasp_tick = None, 0.0  # Moira's Biotic Grasp: the bot it drains, its next tick
        # Mercy's staff: the ally bot its beam is on, its next heal tick, and whether it boosts instead of heals
        self.staff_target, self.staff_tick, self.staff_boost = None, 0.0, False
        self.guardian = None  # (the Timed ability, the ally bot) of a Guardian Angel under way
        self.weapon_entity = None
        self.kit_frame = body_frame  # the body frame builder: soldier's, or the hero kit's (body_setup)
        self.body_extra_instances = []
        self.body_extra_variables = {}
        self.sprint = None  # soldier.Sprint once the body is set up
        self.helix = None  # soldier.Volley (Helix Rockets) once the body is set up
        self.field = None  # soldier.Volley (Biotic Field) once the body is set up
        self.alt = None  # kits.AltFire (the weapon's right-mouse volley) once a kit with one is set up
        self.melee = None  # soldier.Volley (quick melee, V) once the body is set up; also in cooldowns
        self.cooldowns = []  # soldier.Volley per kits.ABILITIES entry: a press runs the icon's cooldown sweep
        self.body_dirty = False  # ability states changed: resend the body frame
        self.refresh_abilities = lambda pulse=None: None  # the kit's (kits.py, soldier.py setup); none on a VM body
        self.ult_charge = 0  # kept through deaths, lost on a hero switch
        self.visor_until = 0.0  # server time Tactical Visor ends; 0 when off
        self.visor_cast_until = 0.0  # server time its cast ends and the lock-on starts; 0 once it has
        self.visor_uses = 0  # visors started on this body: the tint effect's counter (soldier.visor_instance)
        self.field_release_at = 0.0  # server time the thrown Biotic Field lands; 0 when none is in the hand
        self.next_cooldown_frame = 0.0  # server time of the next body frame for the HUD's cooldown sweeps
        self.cast_hold = 0.0  # server time a cast's states (path, Volley stop, animation) are dropped; 0: none
        self.last_rifle_update = None
        self.pending_spawn_voice = None
        self.boost_hits: set[int] = set()  # bots this Boosters flight has already hit
        self.invulnerable_until = 0.0  # server time D.Va's Eject! cast ends: no damage before it
        self.barrels = (0.0, 0.0, False)  # D.Va's cannon spin (var261) at a server time, and whether fire is held
        self.burst_end = None  # the cmfd of the body frame that ended D.Va's pilot's last burst (kits.DVA_PILOT var350)
        self.primary_firing = False  # the last body frame was firing (Soldier: soldier.py setup; D.Va's pilot)
        self.form_change = None  # D.Va's 'eject', 'self_destruct' or 'call_mech', run by Match.change_form
        self.mech_call_at = 0.0  # server time her called mech arrives (Match.land_mech)
        self.ult_instance = None  # D.Va's ultimate graph instance (kits.initialize_practice_kit)
        self.mech_drop = None  # D.Va's called mech: (feet it lands on, yaw), falling until mech_call_at
        self.bots_seen: set[int] = set()  # Range bot entities this player's client has
        self.bot_controllers: set[int] = set()
        self.bot_nameplates: dict[int, tuple] = {}  # controller -> (stream, body create, stage 0/1/2)
        self.mode_script = Stream(GAME_MODE_ENTITY)  # the game mode entity's variables: the countdown
        self.objective_scripts: dict[int, Stream] = {}  # objective entity -> its statescript
        self.bot_weapons: dict[int, tuple] = {}  # bot body -> (its statescript, what it last showed)
        self.next_countdown = 0.0
        self.select_open = True
        self.career = {}  # this match's career counters: hero GUID -> {counter: amount} (tools/lobby/career.py)
        self.board_sent, self.next_board = None, 0.0  # the Tab board's numbers his client has (board.py)
        self.fire = onfire.Fire()  # his On Fire score in this match
        self.switch_notice = None  # (old hero, new hero) of a switch whose chat line is still to send
        self.phased = False  # his last body frame had a kits.PHASED state on: no damage in, no shots out
        self.charged_at = 0  # the command frame Reinhardt's Charge began to run him (Player.charge)
        self.moving = None  # (kits.Ability.moves, the press's command) the next body frame plays on the mover
        self.trail = deque()  # (command frame, feet, health) of the last seconds, for Recall (kits.Ability.rewinds)
        self.rewinding = None  # the trail as it was when the Recall under way began
        self.air_since = None  # the command frame the body left the ground at (kits.AIR_JUMPS)
        self.air_jump_at = 0  # the command frame from which a jump in the air works again; None: used this time up
        self.notice_frame, self.controller_sent = False, 0.0  # his last controller frame: carried a line; when
        self.streak = 0  # eliminations since the last death

    def count(self, name: str, amount: float = 1) -> None:
        """Add to a career counter of the hero played; 'kill_streak' keeps the highest instead."""
        counted = self.career.setdefault(self.hero.guid, {})
        counted[name] = max(counted.get(name, 0), amount) if name == 'kill_streak' else counted.get(name, 0) + amount

    @property
    def max_health(self) -> float:
        return float(PLAYER_KIT.get(self.hero.name, (HEALTH,))[0])

    def frame_at(self, t: float) -> int:
        """This client's command frame at match time t (server.Client._frame, FIRST_TICK 1000)."""
        return 1000 + round((t - self.match.started) * 1_000_000 / self.quanta)

    @property
    def pools(self) -> tuple:
        """(armour, shields) of the hero's body, within max_health (0033's vars, pools.py). shortcut: D.Va's mech
        has none (its pools are in her graph 0109)."""
        return body_pools(self.hero.guid)[1:]

    @property
    def ult_most(self) -> float:
        """Points that fill the ultimate: D.Va's by form (kits.DVA_ULTS), else Tactical Visor's."""
        return DVA_ULTS[self.hero.body][1] if self.hero.body in DVA_ULTS else ULT_POINTS

    @property
    def team_bit(self) -> int:
        return TEAM_BIT << self.team

    def eye(self) -> tuple:
        """The first-person camera, where hitscan shots leave from (target.EYE_HEIGHTS)."""
        x, y, z = self.mover.position
        return x, y + eye_height(self.hero.body, self.mover.crouched), z

    def movement(self) -> Movement:
        mover = self.mover
        flags = (0x2 if mover.crouched else 0) | (DEAD if self.health <= 0 else 0)
        return Movement(mover.position, mover.yaw, mover.pitch, mover.velocity, flags)

    def take_commands(self, commands: list[Command]) -> None:
        """Move the server's copy of the body. Without a body the client moves nothing either."""
        seconds = self.quanta / 1e6
        fresh = self.commands.new(commands)
        if self.match.round_finished:
            return  # keep acknowledging inputs while the result screen is up
        for command in fresh:
            self.last_command = command  # hitbox_feed draws the bots at its view time
            state = (command.forward, command.right, command.buttons)
            pressed = command.buttons & ~self.last_input[2]
            if state != self.last_input:
                log.info('[game] input frame=%d forward=%d right=%d buttons=%06X yaw=%d at %.2f %.2f %.2f',
                    command.frame, *state, command.yaw, *self.mover.position)
                if (self.hero.guid in SCOPE_HEROES and self.body_script is not None and self.health > 0
                        and (command.buttons ^ self.last_input[2]) & SECONDARY):
                    self.body_dirty = True
                if self.hero.body == SPAWN_BODY.get(DVA) and (command.buttons ^ self.last_input[2]) & FIRE:
                    self.barrels = (self.barrel_spin(self.match.now), self.match.now, bool(command.buttons & FIRE))
                self.last_input = state
            if self.has_body and self.health > 0:
                if self.sprint is not None and self.body_script is not None and self.sprint.command(command):
                    log.info('[game] sprint %s frame=%d', 'start' if self.sprint.active else 'stop', command.frame)
                    self.mover.speed_scale = 1 + SPRINT_BONUS if self.sprint.active else 1.0
                    self.refresh_abilities()
                    self.body_dirty = True
                if (self.hero.guid == BASTION and self.body_script is not None and self.match.gameplay_enabled
                        and self.match.body_setup is not None
                        and pressed & BASTION_RECONFIGURE and not self.bastion_switch_at):
                    self.switch_bastion()
                move = command
                if self.bastion_phase:
                    move = replace(command, forward=0, right=0, buttons=command.buttons & ~(JUMP | CROUCH))
                if self.hero.guid in AIR_JUMPS and self.match.gameplay_enabled:
                    self.air_jump(command, pressed)
                if self.guardian is not None:
                    self.match.guard(self)
                jumped = self.mover.flags_word & JUMPED
                self.mover.step(move, seconds)
                if self.mover.flags_word & JUMPED and not jumped:  # a jump spends Exo Boots' charge
                    for charge in self.cooldowns:
                        if isinstance(charge, Crouched) and charge.meter:
                            charge.spend()
                            self.refresh_abilities()
                            self.body_dirty = True
                for step in self.cooldowns:  # Shadow Step: the body is put where the click aimed
                    if isinstance(step, Aimed) and step.active and not step.moved and step.elapsed >= step.spec.steps[1]:
                        self.mover.position, self.mover.velocity, step.moved = list(step.target), (0.0, 0.0, 0.0), True
                recall = next((c for c in self.cooldowns if getattr(getattr(c, 'spec', None), 'rewinds', None)), None)
                if recall is not None and self.match.gameplay_enabled:
                    self.rewind(recall, command)
                client_sync.stepped(self.mover, command.frame)
                if self.mover.flight and self.match.gameplay_enabled:
                    self.match.boost_collide(self, command)
                for timed in self.cooldowns:
                    if isinstance(timed, Timed) and timed.landing and self.mover.grounded:  # Jump Pack
                        timed.stop()
                        self.refresh_abilities()
                        self.body_dirty = True
                if self.mover.striking and self.match.gameplay_enabled:
                    self.match.boost_collide(self, command, STRIKE_DAMAGE, knock=False)
                if self.mover.driven and self.match.gameplay_enabled:
                    self.charge(command)
                else:
                    self.charged_at = 0
                if self.body_script is not None and self.hero.guid == MERCY and command.action in (1, 2):
                    self.switch_mercy_weapon(command.action == 2)
                matrix = next((c for c in self.cooldowns if isinstance(c, Held)), None) if self.hero.guid == DVA else None
                matrix_changed = matrix.command(command) if matrix is not None and self.body_script is not None else False
                # no shot leaves; a reload still runs
                held = (matrix is not None and matrix.active or self.phased
                        or any(getattr(c, 'holstered', False) for c in self.cooldowns))  # Shadow Step's aim
                rifle_command = replace(command, buttons=command.buttons & ~FIRE) if held else command
                if (self.match.gameplay_enabled and self.hero.guid in KITS and self.body_script is not None
                        and not self.bastion_switch_at and not any(cooldown.choosing for cooldown in self.cooldowns)
                        and self.rifle.command(rifle_command)):  # a pick's click (kits.Choice) is not a shot
                    log.info('[game] rifle shot frame=%d serial=%d ammo=%d yaw=%d pitch=%d',
                        command.frame, self.rifle.serial, self.rifle.ammo, command.yaw, command.pitch)
                    if self.rifle is self.mercy_blaster_rifle:
                        self.match.fire_at_target(self, command, MERCY_BLASTER_WEAPON)
                    elif self.bastion_phase == 2:
                        self.match.fire_at_target(self, command, BASTION_SENTRY_WEAPON)
                    else:
                        self.match.fire_at_target(self, command)
                if self.alt is not None and self.body_script is not None and not self.phased and self.alt.command(command):
                    log.info('[game] alt fire serial=%d frame=%d ammo=%d', self.alt.serial, command.frame, self.rifle.ammo)
                    hit = ABILITY_HITS.get((self.hero.name, SECONDARY))  # a flown alt (Symmetra's orb)
                    if hit is not None:
                        self.match.fire_ability(self, command, hit)
                    else:
                        self.match.fire_at_target(self, command, self.alt.weapon)
                if self.match.gameplay_enabled and self.hero.guid == SOMBRA and self.body_script is not None:
                    self.match.update_hack(self, command)
                if self.match.gameplay_enabled and self.hero.guid == MOIRA and self.body_script is not None:
                    self.match.update_grasp(self, command)
                if (self.match.gameplay_enabled and self.hero.guid == MERCY and self.body_script is not None
                        and self.rifle is not self.mercy_blaster_rifle):
                    self.match.update_staff(self, command)
                if self.helix is not None and self.body_script is not None and self.helix.command(command):
                    log.info('[game] helix serial=%d frame=%d', self.helix.serial, command.frame)
                    self.match.fire_helix(self, command)
                if self.field is not None and self.body_script is not None and self.field.command(command):
                    log.info('[game] biotic field serial=%d frame=%d', self.field.serial, command.frame)
                    self.field_release_at = self.match.now + FIELD_RELEASE
                for cooldown in self.cooldowns if self.body_script is not None else ():
                    press = command
                    air = getattr(getattr(cooldown, 'spec', None), 'falling', None)
                    if air is not None and not self.match.falling(self, air):
                        press = replace(command, buttons=command.buttons & ~cooldown.button)  # Glide: not here
                    follows, ally = getattr(getattr(cooldown, 'spec', None), 'follows', None), None
                    if follows is not None and not cooldown.active and command.buttons & cooldown.button:
                        ally = self.match.ally_ahead(self, command, follows[1])  # Guardian Angel: only at an ally
                        if ally is None:
                            press = replace(command, buttons=command.buttons & ~cooldown.button)
                    if getattr(cooldown, 'aiming', False) and pressed & FIRE:  # Shadow Step's click: where to
                        cooldown.target = self.match.step_point(self, command, cooldown.spec.steps[0])
                    if (matrix_changed if cooldown is matrix else cooldown.command(press)):
                        log.info('[game] ability %#04x frame=%d', cooldown.button, command.frame)
                        if ally is not None and cooldown.active:
                            self.guardian = (cooldown, ally)
                        hit = ABILITY_HITS.get((self.hero.name, cooldown.button))
                        if hit is not None and getattr(cooldown, 'active', True):  # not at a Timed ability's end
                            self.match.fire_ability(self, command, hit)
                        orb = ORBS.get((self.hero.name, cooldown.button, cooldown.pick))
                        if orb is not None:
                            self.match.launch_orb(self, command, orb)
                        if cooldown is self.melee:
                            self.match.fire_at_target(self, command, MELEE)
                        spec = getattr(cooldown, 'spec', None)
                        if spec is not None and spec.reloads and cooldown.active and self.match.gameplay_enabled:
                            self.rifle.refill()
                        moves = getattr(spec, 'moves', None)
                        if moves and self.match.gameplay_enabled and getattr(cooldown, 'active', True):  # its start
                            self.moving = (moves, command)  # played with the body frame (_body_weapon_frame)
                            self.boost_hits = set()  # Swift Strike hits each bot once
                        self.refresh_abilities()
                        self.body_dirty = True
                        flight = next((c.flight for c in self.cooldowns if isinstance(c, Timed) and c.active), 0.0)
                        if flight and not self.mover.flight:
                            self.boost_hits = set()
                        self.mover.flight = flight
                if (self.helix is not None and self.body_script is not None and command.buttons & VISOR_BUTTON
                        and not self.visor_until and self.ult_charge >= ULT_POINTS):
                    log.info('[game] tactical visor frame=%d', command.frame)
                    self.match.start_visor(self)
                if (self.hero.body in DVA_ULTS and self.body_script is not None and command.buttons & VISOR_BUTTON
                        and self.health > 0 and self.form_change is None and self.ult_charge >= self.ult_most):
                    self.form_change = 'self_destruct' if self.hero.body == SPAWN_BODY[DVA] else 'call_mech'
            if self.body_vm is not None and self.has_body:  # after the mover's step, dead or alive (its owner_dead)
                self.match.body_vm_command(self, command)

    def rewind(self, recall, command):
        """Recall: while it runs the body goes back along the path of its last seconds; when it ends it has the most
        health it had on that path and a full magazine. Otherwise this step joins the path."""
        if recall.active:
            if self.rewinding is None:
                self.rewinding = list(self.trail)
            if self.rewinding:
                left = max(0.0, 1.0 - recall.elapsed / (recall.length * self.quanta / 1e6))
                self.mover.position = list(self.rewinding[round(left * (len(self.rewinding) - 1))][1])
                self.mover.velocity = (0.0, 0.0, 0.0)
            return
        if self.rewinding is not None:  # it has just ended
            path, self.rewinding = self.rewinding, None
            if path:
                self.mover.position = list(path[0][1])
                self.health = max(self.health, max(health for _, _, health in path))
                self.match._send_player_health(self)
            self.rifle.refill()
            self.trail.clear()
        self.trail.append((command.frame, tuple(self.mover.position), self.health))
        while self.trail[0][0] < command.frame - round(recall.spec.rewinds * 1e6 / self.quanta):
            self.trail.popleft()

    def air_jump(self, command, pressed):
        """Genji's second jump and Hanzo's Lunge: a press of jump while in the air, once each time off the ground."""
        (move, *numbers), cooldown = AIR_JUMPS[self.hero.guid]
        if self.mover.grounded:
            self.air_since = None
            if self.air_jump_at is None:
                self.air_jump_at = self.air_used + round(cooldown * 1e6 / self.quanta)
            return
        if self.air_since is None:
            self.air_since = command.frame
        armed = (command.frame - self.air_since) * self.quanta / 1e6 >= AIR_JUMP_AFTER
        if pressed & JUMP and armed and self.air_jump_at is not None and command.frame >= self.air_jump_at:
            log.info('[game] air jump %s frame=%d', move, command.frame)
            getattr(self.mover, move)(command, *numbers)
            self.air_used, self.air_jump_at = command.frame, None

    def charge(self, command):
        """A step of Reinhardt's Charge: the bots he runs into are bumped, and a wall that stops him ends it."""
        timed = next((c for c in self.cooldowns if isinstance(c, Timed) and c.active), None)
        if timed is None:
            return  # stopped already: the frame that takes the mods off has not gone yet
        if not self.charged_at:
            self.charged_at, self.boost_hits = command.frame, set()
        self.match.boost_collide(self, command, CHARGE_BUMP)
        ran = (command.frame - self.charged_at) * self.quanta / 1e6
        if ran >= CHARGE_RUN_UP and math.hypot(self.mover.velocity[0], self.mover.velocity[2]) < CHARGE_STOP_SPEED:
            log.info('[game] charge stopped by a wall frame=%d', command.frame)
            timed.stop()
            self.charged_at = 0
            self.refresh_abilities()
            self.body_dirty = True

    def switch_mercy_weapon(self, blaster):
        if blaster == (self.rifle is self.mercy_blaster_rifle):
            return
        if blaster:
            self.match.update_staff(self, Command(0))
            kit, weapon = MERCY_BLASTER, MERCY_BLASTER_WEAPON
            if self.mercy_blaster_rifle is None:
                self.mercy_blaster_rifle = Rifle(self.quanta, weapon)
            self.rifle = self.mercy_blaster_rifle
        else:
            kit, weapon = WEAPON_KITS[MERCY], PLAYER_KIT['Mercy'][1]
            self.rifle.updates.clear()
            self.rifle.firing = False
            self.rifle.reload_until = self.rifle.refill_at = None
            self.rifle = Rifle(self.quanta, weapon)
        self.kit_frame = frame_builder(kit, weapon.reload_seconds)
        self.last_rifle_update = RifleUpdate(self.rifle.ammo, self.rifle.serial, False, True)
        self.body_dirty = True

    def switch_bastion(self):
        sentry = self.bastion_phase == 0
        kit, weapon = (BASTION_SENTRY, BASTION_SENTRY_WEAPON) if sentry else (
            WEAPON_KITS[BASTION], PLAYER_KIT['Bastion'][1])
        self.rifle.updates.clear()
        self.rifle.firing = False
        self.rifle.reload_until = self.rifle.refill_at = None
        next_rifle = self.bastion_other_rifle or Rifle(self.quanta, weapon)
        self.rifle, self.bastion_other_rifle = next_rifle, self.rifle
        self.kit_frame = frame_builder(kit, weapon.reload_seconds)
        self.last_rifle_update = RifleUpdate(self.rifle.ammo, self.rifle.serial, False, True)
        self.bastion_phase = 1 if sentry else 3
        self.bastion_switch_at = self.match.now + (1.0 if sentry else .5)
        self.body_dirty = True
        log.info('[game] Bastion entering %s', 'Sentry' if sentry else 'Recon')

    def update_bastion_repair(self, now):
        phase = self.bastion_repair_phase
        buttons = self.last_input[2]
        previous = self.bastion_repair_tick
        self.bastion_repair_tick = now
        charge = self.bastion_repair_charge
        if previous is not None and phase == 0:
            charge = min(BASTION_REPAIR_CAPACITY, charge + BASTION_REPAIR_RECHARGE * (now - previous))
        if self.bastion_switch_at or self.rifle.reload_until is not None or not self.health:
            next_phase = 0
        elif (buttons & SECONDARY and not buttons & FIRE and self.health < self.max_health
                and (phase != 3 or now >= self.bastion_repair_at)
                and (charge > 0 if phase in (1, 2) else charge >= .2 * BASTION_REPAIR_CAPACITY)):
            if phase in (0, 3):
                next_phase = 1
                self.bastion_heal_at = now + BASTION_REPAIR_DELAY
            else:
                next_phase = 2 if now >= self.bastion_repair_at else phase
                if next_phase == 2 and previous is not None:
                    active = max(0.0, now - max(previous, self.bastion_heal_at))
                    amount = min(charge, self.max_health - self.health, BASTION_REPAIR_RATE * active)
                    if amount > 0:
                        charge -= amount
                        self.match._heal(self, amount, self)
        elif phase in (1, 2):
            next_phase = 3
        else:
            next_phase = 0 if now >= self.bastion_repair_at else phase
        if next_phase != phase:
            self.bastion_repair_phase = next_phase
            self.bastion_repair_at = now + BASTION_REPAIR_BLEND
            self.body_dirty = True
        self.bastion_repair_charge = charge

    def barrel_spin(self, now):
        """D.Va's cannons' spin, var261 (DataFlowMapping 21 turns the barrels): held fire runs ChaseVar 22 up to 1
        at 2/s, the release ChaseVar 21 down to 0 at 2/3 per second. The client does not run them: sent."""
        spin, at, held = self.barrels
        return min(1.0, spin + 2.0 * (now - at)) if held else max(0.0, spin - (now - at) * 2 / 3)

    def update_widow_charge(self, now):
        if not self.health or not self.last_input[2] & SECONDARY or self.rifle.reload_until is not None:
            self.widow_charge_from = None
        elif self.rifle.serial != self.widow_charge_serial:
            self.widow_charge_from = now + .5
        elif self.widow_charge_from is None:
            self.widow_charge_from = now
        self.widow_charge_serial = self.rifle.serial
        # Graph 27 charges from 1 to 10 at ten units per second.
        charge = 1.0 if self.widow_charge_from is None else min(10.0, 1 + max(0.0, now - self.widow_charge_from) * 10)
        if charge != self.widow_charge and (now >= self.widow_charge_next or charge == 1):
            self.widow_charge = charge
            self.widow_charge_next = now + .05
            self.body_dirty = True

    def new_body(self) -> None:
        """The next body id, at the spawn point, where the client puts a new body too. Ids cycle through
        the player's 254; DROP_SLOT is D.Va's falling mech."""
        self.body = self.entity | ((self.body & 0xFF) % 254 + 1)
        self.mover = PlayerMover(self.spawn[0], _yaw_units(self.spawn[1]), self.match.world, self.hero.body, self.team)

    def describe(self) -> str:
        return f"{self.name} ({self.hero.name}, team {self.team + 1})"


def no_skin(account_lo: int, hero: int) -> tuple[int, bool]:
    return 0, False


def _remote_volley(on, events):
    volley = BitWriter()
    volley.bit(on)
    volley.bit(0)  # optional multiplier defaults to 1
    volley.bits(events & 63, 6)
    return volley


def _effect_begun():
    effect = BitWriter()  # STU_691BFA55: begun, 2-bit counter
    effect.bit(1)
    effect.bits(1, 2)
    return effect


# Mercy's heal status (graph 0x0079, from status asset 0x04A0000000000019 that her staff's STU_431F3FCC names): on the
# healed entity; Entry starts a client-only STU_9932D728 beam from var27's var49 (her staff) to it. var27 is presence
# bit 3, found live with a marker entity per bit (graph_layout's slot order puts it at 5).
# shortcut: why this graph's presence order differs from the slot order is unknown; graph_layout.py is wrong for it.
HEAL_STATUS = Graph(0x0079, 20, 6, 10, frozenset({8, 1, 2, 7}))
# Her damage boost status (graph 0x006C, status asset 0x04A0000000000016): its client Entry starts STU_9932D728 4 / 5
# from var27's var49 too, but var27 is no sync var here; it falls back to the
# instance's source_entity (extra variables did not reach it). Remote slot 0
# is effect state 0 (the boost's glow on the target).
BOOST_STATUS = Graph(0x006C, 12, 3, 7)


def remote_volley_frame(hero, ammo, firing, shots, uses=0, healer=None, killed=None):
    """A hero bot's weapon graph as the other clients see it (H0): its primary Volley on while it fires with the
    event counter at the shots fired, and the ammo it needs to draw. The remote Volley DSR is the owner one
    without the time and count block (research/remote-bot-statescript.md). An ability Volley (ABILITY_VOLLEYS)
    is on once used, its counter at the uses. A dead bot's (`killed`: its killer's body or None, in a tuple) carries
    its kill feed line (target.kill_feed)."""
    instances, entity_vars = [], {31: Int(1), 34: Bool(False)}
    if hero in WEAPON_GRAPHS:  # a hero without one still carries the heal status below
        layout, _, slot, ammo_bit = WEAPON_GRAPHS[hero]
        active = {slot: _remote_volley(firing, shots)}
        if firing:
            active.update(dict.fromkeys(WEAPON_EFFECTS.get(hero, ()), _effect_begun()))
        if uses and hero in ABILITY_VOLLEYS:
            active[ABILITY_VOLLEYS[hero][1]] = _remote_volley(True, uses)
        presence = {ammo_bit: Int(ammo)}
        if hero in WEAPON_MODELS:  # the gun: CosmeticEntity (no payload) makes it when var34 || var478 is false
            model_slot, slot_bits, model_bit = WEAPON_MODELS[hero]
            active[model_slot] = None
            gun = StateBound(Entity(0), 1, model_slot, slot_bits)
            entity_vars[478] = Bool(False)
            if model_bit is None:
                entity_vars[49] = gun
            else:
                presence[model_bit] = gun
        instances.append(Instance(1, Graph(*layout), presence=presence, active=active))
    if healer is not None:  # (body, boost) of the Mercy whose staff is on this bot
        body, boost = healer
        instances.append(Instance(2, BOOST_STATUS, active={0: _effect_begun()}, source_entity=body) if boost
                         else Instance(2, HEAL_STATUS, presence={3: Entity(body)}))
    if killed is not None:
        instances.append(kill_feed(3, True, killed[0], True))  # a hero body dies once: its respawn is a new body
    return remote_full_frame(instances, entity_vars)


@dataclass
class HelixRocket:
    player: Player
    body: int
    entity: int
    shot: Projectile
    serial: int
    begin_frame: int = 0
    records: tuple = ()


class Match:
    def __init__(self, game_map: GameMap, on_leave=None, skin_of=no_skin, gameplay=False, body_setup=None,
                 mesh=None, difficulty='medium', on_finish=None, world=None) -> None:
        self.id = (secrets.randbits(63), secrets.randbits(63))
        self.game_map = game_map
        self.on_finish = on_finish
        # (account, hero GUID) -> (skin theme, golden weapon) the player has equipped, from the lobby.
        # The client's hero select equips a skin through the lobby (24500), even in a match.
        self.skin_of = skin_of
        self.controller = heroselect.controller_of(game_map.mode_guid)
        self.tick = 0  # the server's frame number, set every tick
        self.assemble_ends: float | None = None  # PvP: when "assemble your team" ends
        self.assembled = False
        self.players: list[Player] = []
        self.started = time.time()
        self.on_leave = on_leave  # called with each player who leaves
        self.ended = False
        self.gameplay_enabled = gameplay
        self.body_setup = body_setup
        self.bots = []  # the Practice Range's roster bots, created with the first player's body
        self.projectiles = []  # (target player, target.Projectile) in flight or not yet destroyed everywhere
        # Helix rockets and ability casts in flight: (land time, impact point, bot hit directly, damageable bots,
        # shooter, target.AbilityHit or None for Helix)
        self.rockets = []
        self.shots = []  # players' projectile shots in flight (_tick_shots)
        self.salvos = []  # [start time, shots launched, shooter, target.AbilityHit] (Micro Missiles)
        self.fields = []  # Biotic Fields on the ground: [entity, position, team, next heal, end, viewers]
        self.orbs = []  # Biotic Orbs: [launch time, origin, end point, target.Orb, owner, amount left, next tick]
        self.field_count = 0
        self.helix_rockets = []  # owner-native Helix startup, replication and lifetime
        self.bot_shots = []  # (when, viewer, entity, impact): mode-1 shot projectiles to stop at impact, or destroy (None)
        self.mech_bombs = []  # D.Va's Self-Destructing mechs: [owner, body, feet, time it blows up, time it is removed]
        self.rocket_ids = 0x00FFFFFF  # native Helix visuals use B3000000 onward, separate from B1 shots and B2 fields
        self.projectile_count = 0
        self.now = 0.0  # the server time of the last update
        self.bot_attacks_enabled = gameplay
        self.difficulty = difficulty  # Practice vs AI: a target.DIFFICULTY key
        self.mesh = mesh  # the map's collision.Mesh: line of sight, bots, nav; None sees all
        self.world = world  # the player mover's collision (collision.movement_world); None walks flat
        self.assault = None  # assault.Assault once an Assault round starts
        self.send_home_at = None  # when the finished match sends its players back to the menu
        self.sightings = {}  # team -> (position, time) where one of its hero bots last saw an enemy
        self.next_steer = 0.0
        self.next_heal = 0.0
        self.mannequin, self.next_viewer = None, 0.0  # viewer.py: the Range's animation-viewer bot, its next check
        self.retired = None  # the mannequin a swap took out of play, destroyed at the next check
        self.nav_region = None  # the nav nodes the hero bots walk, from their spawn

    @property
    def target(self):
        """The first roster bot: the research probes drive one bot's statescript."""
        return self.bots[0] if self.bots else None

    def _tick_viewer(self, now):
        """Practice Range with OW_VIEWER_FILE set: keep the mannequin of the hero the file names (viewer.py), swapping
        it when the name changes; once the Range bots exist, so the first player's _target_updates creates it."""
        if self.game_map.map_guid != PRACTICE_RANGE.map_guid or not self.bots or now < self.next_viewer:
            return
        self.next_viewer = now + viewer.CHECK_SECONDS
        retired, self.retired = self.retired, None
        for player in self.players if retired is not None else ():  # last swap's mannequin, its fire off by now
            if player.client is not None and retired.entity in player.bots_seen:
                player.client.queue_entities([EntityUpdate(retired.entity, OP_DESTROY)])
                player.bots_seen.discard(retired.entity)
                player.seen.pop(retired.entity, None)
                player.bot_weapons.pop(retired.entity, None)
        name = viewer.wanted()
        old = self.mannequin
        if name is None:
            return
        ally = name.lower().startswith('ally ')  # 'ally <hero>': on the player's team, for healing beams
        name, team = (name[5:].strip(), self.players[0].team) if ally and self.players else (name, 1)
        if old is not None and hero_name(old.hero).lower() == name.lower() and old.team == team:
            return
        serial = getattr(old or retired, 'serial', -1) + 1
        new = viewer.mannequin(name, spawn_point(self.game_map, 0, 0), serial, team) if name else None
        if new is None and (name or old is None):
            return  # an unknown or half-typed name keeps the current mannequin; an empty file removes it
        if old is not None:
            # Out of play with its fire off now; destroyed, and the new one made, at the next check.
            self.bots.remove(old)
            self.retired, self.mannequin = old, None
            for player in self.players:
                stream, sent = player.bot_weapons.get(old.entity, (None, None))
                if player.client is not None and old.entity in player.bots_seen and sent is not None and sent[2]:
                    player.client.queue_entities([stream.full_frame(remote_volley_frame(*sent[:2], False, *sent[3:]))])
            return
        if new is not None:
            new.serial = serial
            self.bots.append(new)
        self.mannequin = new
        log.info('[game] viewer mannequin: %s', hero_name(new.hero) if new else 'none')

    def _steer_bots(self, now):
        """Practice vs AI: each hero bot not in a fight walks to its own slot on a ring round the point; while
        its team's enemy sighting near the point is fresh, round where it was seen instead, or else round a living
        teammate player within LEASH of the point. A bot in a fight strafes. Attacking bots wait in their spawn
        room until setup ends."""
        if not self.bots or not self.bots[0].hero or not self.game_map.objectives or now < self.next_steer:
            return
        self.next_steer = now + STEER_SECONDS
        grid = nav.for_map(self.game_map.map_guid, nav_points(self.game_map))
        if grid is None:
            return
        if self.nav_region is None:
            self.nav_region = grid.region(self.bots[0].home)
        objective = self._objective()
        setup = self.game_map.mode_guid in OBJECTIVE_MODES and (
            self.assemble_ends is None or now < self.assemble_ends + SETUP_SECONDS)
        for team in {bot.team for bot in self.bots} - ({0} if setup else set()):  # team 0 attacks
            sighting = self.sightings.get(team)
            hunting = (sighting is not None and now - sighting[1] <= CHASE_SECONDS
                       and math.dist(sighting[0], objective) <= LEASH)
            near = [p.mover.position for p in self.players if p.team == team and p.client is not None
                    and p.has_body and p.health > 0 and math.dist(p.mover.position, objective) <= LEASH]
            centre = sighting[0] if hunting else min(near, key=lambda at: math.dist(at, objective), default=objective)
            mates = [bot for bot in self.bots if bot.team == team and bot.hero]
            for n, bot in enumerate(mates):
                if not bot.health:
                    continue
                if bot.next_attack_at is not None:
                    self._strafe(bot, grid)
                    continue
                angle = n * math.tau / len(mates)
                goal = (centre[0] + SPREAD * math.sin(angle), centre[1], centre[2] + SPREAD * math.cos(angle))
                if bot.goal is None or math.dist(bot.goal, goal) > REPATH:
                    bot.goal = goal
                    route = grid.path(bot.position, goal, self.nav_region)
                    bot.waypoints = tuple(nav.smooth(self.mesh, route) if self.mesh else route)
                    bot.leg, bot.moved_at = 0, None

    def _strafe(self, bot, grid):
        """Sidestep up to STRAFE metres to a random side of the bot's target, where the floor goes and no wall
        stands in the way; else hold."""
        if bot.target is None:
            return
        aim = self._stance(bot.target)[0]
        dx, dz = aim[0] - bot.position[0], aim[2] - bot.position[2]
        side = random.choice((-1, 1)) * STRAFE / (math.hypot(dx, dz) or 1.0)
        goal = (bot.position[0] - dz * side, bot.position[1], bot.position[2] + dx * side)
        node = grid.nodes[grid.nearest(goal, self.nav_region)]
        bot.goal = None  # re-path to the ring when the fight ends
        clear = self.mesh is None or nav.passable(self.mesh, bot.position, node)
        bot.waypoints = (node,) if clear and math.dist(node, goal) <= nav.SIZE else ()
        bot.leg, bot.moved_at = 0, None

    def _objective(self):
        index = self.assault.index if self.assault else 0
        return self.game_map.objectives[min(index, len(self.game_map.objectives) - 1)]

    @property
    def round_finished(self) -> bool:
        return self.assault is not None and self.assault.winner is not None

    def _tick_assault(self, now):
        """Assault: the round starts when setup ends; then the points fall to attackers standing on them. A
        capture sends the bots to the next point and refreshes every widget; RESULT_SECONDS after the round
        ends the players go back to the menu."""
        if self.game_map.mode_guid != ASSAULT or not self.assembled or not self.game_map.objectives:
            return
        if self.send_home_at is not None and now >= self.send_home_at:
            self.send_home_at = None
            for player in self.players:
                if player.client is not None:
                    player.client.queue_reliable(LEAVE_GAME, {"+0x78": 0, "+0x79": False})
            log.info('[game] %s: match over, players sent to the menu', self.label())
        if self.round_finished:
            return
        if self.assault is None:
            if now < self.assemble_ends + SETUP_SECONDS:
                return
            self.assault = Assault(self.game_map.objectives, self.assemble_ends + SETUP_SECONDS)
        fighters = ([(p.team, p.mover.position) for p in self.players
                     if p.client is not None and p.has_body and p.health > 0]
                    + [(b.team, b.position) for b in self.bots if b.health])
        point = self._objective()
        on = [sum(1 for team, position in fighters if team == side and on_point(position, point)) for side in (0, 1)]
        event = self.assault.tick(now, *on)
        if event is None:
            return
        log.info('[game] %s: assault %s, point %d', self.label(), event, self.assault.index)
        if event != 'captured':
            if self.on_finish is not None:
                self.on_finish(self, now)
            self.bot_attacks_enabled = False
            self.send_home_at = now + RESULT_SECONDS
            self.rockets.clear()
            self.shots.clear()
            self.salvos.clear()
            self.orbs.clear()
            for _, shot in self.projectiles:
                shot.landed = True  # the next replication destroys the visible effects
            for field in self.fields:
                field[3] = field[4] = now
            self._tick_fields(now)
            self._tick_rockets(now)
            for bot in self.bots:
                bot.next_attack_at = None
        for player in self.players:
            player.next_countdown = now

    def respawn_seconds(self) -> float:
        return OBJECTIVE_RESPAWN_SECONDS if self.game_map.mode_guid in OBJECTIVE_MODES else RESPAWN_SECONDS

    def _ensure_bots(self, player):
        if not self.bots and self.gameplay_enabled:
            if self.game_map.map_guid == PRACTICE_RANGE.map_guid:
                grid = self.mesh and nav.for_map(PRACTICE_RANGE.map_guid, patrol_points())
                self.bots = range_bots(grid, self.mesh)
            elif self.game_map.mode_guid in LOADING_GATES:  # PvE: the mission's omnics, no hero bots on either side
                self.bots = omnic_bots(self.game_map.spawns) + [payload_target(self.game_map.objectives[0])]
            else:
                # shortcut: each team is a random draw of different roster heroes; the 1.74 AI's picks are not read.
                kits = [hero for hero in heroes().values() if hero.name in HERO_KIT]
                pick = lambda count: [(hero.guid, hero.body, hero.name) for hero in random.sample(kits, count)]
                self.bots = (hero_bots(self.game_map.spawns, 1 - player.team, pick(6))
                             + hero_bots(self.game_map.spawns, player.team, pick(ALLIES), first=6))
                for bot in self.bots:  # their bodies get the mode's respawn time too
                    bot.respawn_seconds = self.respawn_seconds()
            if self.bots and self.bots[0].hero:
                log.info('[game] %s bots: %s', self.label(),
                    ', '.join(f'{hero_name(bot.hero)} (team {bot.team})' for bot in self.bots if bot.hero))
        slot = max([p.slot for p in self.players] + [b.slot for b in self.bots if b.slot is not None] + [-1]) + 1
        added = False
        for bot in self.bots:
            if bot.slot is None and bot.behaviour != PAYLOAD:
                bot.slot, slot = slot, slot + 1
                added = True
        return added

    def _target_updates(self, player):
        if self._ensure_bots(player):
            for viewer in self.players:
                if viewer.client is not None:
                    viewer.client.queue_reliable(20301, {'+0x78': self.game_struct(viewer)})
        updates = []
        for bot in self.bots:
            if not bot.health or bot.entity in player.bots_seen:
                continue
            player.bots_seen.add(bot.entity)
            player.seen[bot.entity] = False
            if bot.slot is not None:
                hero = bot.hero or next((guid for guid, body in BODIES.items() if body == bot.body), BOT_HERO)
                identity = {74: [None, None, bot.slot, None, None, None, 1, 1, None],
                    44: [bot.entity] * 3, 75: [hero, None, None, hero]}
                if bot.controller not in player.bot_controllers:
                    player.bot_controllers.add(bot.controller)
                    identity[26] = [PLAYER_FILTER | TEAM_BIT << bot.team]
                    updates.append(EntityUpdate(bot.controller, OP_CREATE, lambda origin, identity=identity:
                        world.create(origin, PLAYER_DEFINITION, identity, flags=PLAYER_FLAGS & ~PLAYER_PATH)))
                else:
                    updates.append(EntityUpdate(bot.controller, OP_UPDATE, lambda origin, identity=identity:
                        world.update(origin, identity)))
            components = world.health(bot.health, bot.max_health, *bot.pools)
            components[26] = [TEAM_BIT << bot.team]
            if bot.hero:
                components[123] = [bot.hero]
            position, rotation = bot.position, _rotation(_yaw_units(bot.yaw))
            creation = EntityUpdate(bot.entity, OP_CREATE, lambda origin, body=bot.body, components=components,
                position=position, rotation=rotation: world.create(origin, body, components,
                    position=position, rotation=rotation))
            updates.append(creation)
            if bot.slot is not None:
                previous = player.bot_nameplates.get(bot.controller)
                stream = previous[0] if previous else Stream(bot.controller)
                player.bot_nameplates[bot.controller] = stream, creation, 0
        return updates

    def _nameplate_updates(self, player):
        updates = []
        bodies = {bot.entity: bot for bot in self.bots if bot.health}
        for controller, (stream, creation, stage) in list(player.bot_nameplates.items()):
            if creation is None:
                continue
            if creation.entity not in bodies:
                updates.append(stream.full_frame(variables_frame({})))
                player.bot_nameplates[controller] = stream, None, 0
            elif not creation.delivered:
                continue
            elif stage == 0:
                updates.append(stream.full_frame(nameplates.create(bodies[creation.entity])))
                player.bot_nameplates[controller] = stream, creation, 1
            elif stage == 1 and not stream.data_in_flight:
                cmfd = stream.next_cmfd(self._newest_frame(player))
                updates.append(stream.delta_frame(nameplates.show(cmfd)))
                player.bot_nameplates[controller] = stream, creation, 2
        return updates

    def view_time(self, player, command):
        """The match time at which the shooter's client drew the bots it aimed at: the command frame's time (world
        frames are FIRST_TICK + elapsed / quanta, server.Client._payload) less the view delay the command carries."""
        return self.started + (command.frame - 1000) * player.quanta / 1e6 - command.view_delay(player.quanta)

    def fire_at_target(self, player, command, weapon=None):
        origin, direction = player.eye(), aim_direction(*command.aim())
        seen = self.view_time(player, command)
        if command.click_yaw is not None:
            log.info('[game] click frame=%d subframe=%d extra=%d aim=(%d, %d) click=(%d, %d) view=%.3f s ago',
                command.frame, command.subframe, command.extra, command.yaw, command.pitch,
                command.click_yaw, command.click_pitch, self.now - seen)
        fired = weapon or (None if player.hero.guid == SOLDIER else PLAYER_KIT[player.hero.name][1])
        primary = weapon is None
        if primary:
            player.count('shots')  # with 'shots_hit': the Tab board's Weapon Accuracy (board.py)
        if fired is not None and fired.speed:  # a projectile flies (_tick_shots); only hitscan lands at once
            wall = self.mesh and self.mesh.ray(origin, direction, fired.reach)
            self.shots.append([player, origin, direction, fired, self.now, 0.0, wall[0] if wall else fired.reach])
            return
        hits = [(bot.ray(origin, direction, at=seen), bot) for bot in self._enemy_bots(player)]
        wall = self.mesh and self.mesh.ray(origin, direction, 100)
        hits = [(*hit, bot) for hit, bot in hits if hit is not None and (not wall or hit[0] < wall[0])]
        visor = bool(player.visor_until and not player.visor_cast_until)
        if visor and weapon is None:
            hits = self._visor_target(player, origin, direction)
        if weapon is not None or player.hero.guid != SOLDIER:  # melee and beams stop at their reach
            hits = [hit for hit in hits if hit[0] <= (weapon or PLAYER_KIT[player.hero.name][1]).reach]
        if not hits:
            return
        distance, critical, target = min(hits, key=lambda hit: hit[0])
        if player.hero.guid == SOLDIER and weapon is None:
            amount = rifle_damage(distance, critical, visor)
        else:  # shortcut: one hitscan ray, every pellet hits; projectiles not traced
            weapon = weapon or PLAYER_KIT[player.hero.name][1]
            critical = critical and weapon.crit > 1  # weapons that cannot headshot (target.Weapon.crit)
            amount = round(weapon.damage_at(distance)) * weapon.pellets * (weapon.crit if critical else 1)
        if primary:
            player.count('shots_hit')
        self._damage_bot(player, target, amount, time.time(), critical, 1 if weapon is None else weapon.pellets)
        log.info('[game] training target %08X hit frame=%d distance=%.2f critical=%s health=%d',
            target.entity, command.frame, distance, critical, target.health)

    def _tick_shots(self, now):
        """A player's projectile shot flies at its weapon's speed along the aim it was fired on, to the first wall or
        its reach. Each tick it hits the first enemy bot on the stretch flown since the last (as LizRollin's
        combat._fly), where the bot is now: a moving target has to be led.
        shortcut: a straight line with no arc, one ray for all its pellets and no splash; upgrade from the Volleys'
        data when a hero needs it."""
        flying = []
        for shot in self.shots:
            player, origin, direction, weapon, fired, flown, reach = shot
            end = min((now - fired) * weapon.speed, reach)
            start = tuple(o + d * flown for o, d in zip(origin, direction))
            hits = [(bot.ray(start, direction, end - flown), bot) for bot in self._enemy_bots(player)]
            hits = [(*hit, bot) for hit, bot in hits if hit is not None]
            if hits:
                distance, critical, target = min(hits, key=lambda hit: hit[0])
                critical = critical and weapon.crit > 1
                amount = round(weapon.damage_at(flown + distance)) * weapon.pellets * (weapon.crit if critical else 1)
                if weapon is PLAYER_KIT.get(player.hero.name, (None, None))[1]:  # the primary's own shot
                    player.count('shots_hit')
                self._damage_bot(player, target, amount, now, critical, weapon.pellets)
                log.info('[game] training target %08X hit by a projectile distance=%.2f critical=%s health=%d',
                    target.entity, flown + distance, critical, target.health)
            elif end < reach:
                shot[5] = end
                flying.append(shot)
        self.shots = flying

    def _visor_target(self, player, origin, direction, cone=VISOR_CONE, reach=math.inf, bots=None):
        """Tactical Visor: the living enemy (or one of `bots`) in clear view nearest the crosshair, hit wherever the
        aim is."""
        best = None
        for bot in self._enemy_bots(player) if bots is None else bots:
            center = bot.center()
            offset = [c - o for c, o in zip(center, origin)]
            distance = math.hypot(*offset)
            facing = sum(d * o for d, o in zip(direction, offset)) / distance if distance else 1
            if (bot.health and facing >= cone and distance <= reach
                    and (self.mesh is None or self.mesh.clear(origin, center))
                    and (best is None or facing > best[0])):
                best = (facing, distance, bot)
        return [(best[1], False, best[2])] if best else []

    def update_hack(self, player, command):
        if self.now < player.hack_ready_at:
            return
        # shortcut: acquire an aimed bot; the native lock-on cone and tether are still unserved.
        target = None
        if command.buttons & (FIRE | SECONDARY) == SECONDARY and player.rifle.reload_until is None:
            reach = 15
            origin, direction = player.eye(), aim_direction(*command.aim())
            wall = self.mesh and self.mesh.ray(origin, direction, reach)
            hits = [(bot.ray(origin, direction, reach), bot) for bot in self._enemy_bots(player)]
            _, target = min(((hit[0], bot) for hit, bot in hits if hit and (not wall or hit[0] < wall)),
                            key=lambda hit: hit[0], default=(0, None))
        if player.hack_target is not None:
            if target is None or target.entity != player.hack_target:
                self._finish_hack(player, self.now)
            elif self.now >= player.hack_completes_at:
                target.hacked_until = self.now + 5.0
                self._finish_hack(player, self.now, completed=True)
            return
        if target is None:
            return
        player.hack_target = target.entity
        player.hack_completes_at = self.now + .65
        player.weapon_extra_states.update(sombra_hack_states(target.entity, command.frame))
        player.body_dirty = True
        log.info('[game] Hack start frame=%d target=%08X', command.frame, target.entity)

    def update_grasp(self, player, command):
        """Moira's Biotic Grasp: while the right mouse is held alone, the enemy bot nearest the crosshair within the
        lock-on cone and reach takes GRASP_DAMAGE and heals her GRASP_HEAL every GRASP_TICK; the beam's states follow
        the target (kits.moira_grasp_states)."""
        target = None
        if command.buttons & (FIRE | SECONDARY) == SECONDARY:
            found = self._visor_target(player, player.eye(), aim_direction(*command.aim()), GRASP_CONE, GRASP_REACH)
            target = found[0][2] if found else None
        if (target and target.entity) != player.grasp_target:
            for state in MOIRA_GRASP_STATES:
                player.weapon_extra_states.pop(state, None)
            if target is not None:
                player.weapon_extra_states.update(moira_grasp_states(target.entity))
            player.grasp_target, player.grasp_tick, player.body_dirty = target and target.entity, self.now + GRASP_TICK, True
            log.info('[game] grasp target=%s', target and f'{target.entity:08X}')
        while target is not None and target.health and self.now >= player.grasp_tick:
            self._damage_bot(player, target, GRASP_DAMAGE, self.now)
            if 0 < player.health < player.max_health:
                self._heal(player, GRASP_HEAL, player)
            log.info('[game] grasp %08X health=%.1f self=%.1f', target.entity, target.health, player.health)
            player.grasp_tick += GRASP_TICK

    def ally_ahead(self, player, command, reach):
        """The ally bot Guardian Angel flies to: the one nearest the crosshair in the staff's cone, within `reach`."""
        if not self.gameplay_enabled:
            return None
        allies = [bot for bot in self.bots if bot.team == player.team and bot.entity in player.bots_seen]
        found = self._visor_target(player, player.eye(), aim_direction(*command.aim()), GRASP_CONE, reach, allies)
        return found[0][2] if found else None

    def guard(self, player):
        """A step of Guardian Angel (kits.Ability.follows): the mover is aimed at the point short of the ally and over
        his feet, wherever he is now. There, or with the ally dead or the ability over (its time, the button again),
        it ends and its cooldown starts."""
        timed, ally = player.guardian
        speed, _, short, over = timed.spec.follows
        there = True
        if timed.active and ally.health:
            (x, y, z), own = ally.center(), player.mover.position
            away = math.hypot(own[0] - x, own[2] - z)
            part = min(short / away, 1.0) if away else 0.0
            there = player.mover.fly_to((x + (own[0] - x) * part, y - 1.0 + over, z + (own[2] - z) * part), speed)
        if there:
            log.info('[game] guardian angel ends at %.2f %.2f %.2f', *player.mover.position)
            player.mover.dashing, player.guardian = None, None
            timed.stop()
            player.refresh_abilities()
            player.body_dirty = True

    def update_staff(self, player, command):
        """Mercy's Caduceus Staff: while the primary fire (heal) or the secondary (damage boost) is held alone, the beam
        locks on the ally bot nearest the crosshair within the cone and STAFF_REACH. Healing restores STAFF_HEAL a
        second. The staff's own states follow (kits.mercy_staff_states: glow, poses) and the bot's remote frame carries
        the heal or boost status graph, whose client-only beam runs from her staff to it (_send_bot_weapons)."""
        target, held = None, command.buttons & (FIRE | SECONDARY)
        boost = held == SECONDARY
        if held in (FIRE, SECONDARY):
            allies = [bot for bot in self.bots if bot.team == player.team and bot.entity in player.bots_seen]
            found = self._visor_target(player, player.eye(), aim_direction(*command.aim()), GRASP_CONE,
                                       STAFF_REACH, allies)
            target = found[0][2] if found else None
        if (target and target.entity) != player.staff_target or (target and boost != player.staff_boost):
            for state in MERCY_STAFF_STATES:
                player.weapon_extra_states.pop(state, None)
            if target is not None:
                player.weapon_extra_states.update(mercy_staff_states(boost))
            player.staff_target, player.staff_tick = target and target.entity, self.now + STAFF_TICK
            player.staff_boost, player.body_dirty = boost, True
            log.info('[game] staff %s target=%s', 'boost' if boost else 'heal', target and f'{target.entity:08X}')
        while target is not None and not boost and target.health and self.now >= player.staff_tick:
            if target.health < target.max_health:
                self._heal(target, STAFF_HEAL * STAFF_TICK, player)
            player.staff_tick += STAFF_TICK

    def _finish_hack(self, player, now, *, completed=False):
        log.info('[game] Hack %s target=%08X', 'complete' if completed else 'cancel', player.hack_target)
        player.hack_target = None
        player.hack_completes_at = 0.0
        player.hack_ready_at = now + (8.0 if completed else 2.0)
        for state in SOMBRA_HACK_STATES:
            player.weapon_extra_states.pop(state, None)
        player.body_dirty = True

    def start_visor(self, player):
        player.visor_cast_until = self.now + VISOR_CAST
        player.visor_until = player.visor_cast_until + VISOR_SECONDS
        player.visor_uses += 1
        player.rifle.refill()
        player.ult_charge = 0
        player.body_extra_variables[ULT_CHARGE] = Int(0)
        player.refresh_abilities()
        player.body_dirty = True

    def _send_bot_health(self, target):
        health, most = target.health, target.max_health
        for viewer in self.players:
            if viewer.client is not None and target.entity in viewer.bots_seen:
                viewer.client.queue_entities([EntityUpdate(target.entity, OP_UPDATE,
                    lambda offset, health=health: world.update(offset, world.health(health, most, *target.pools)))])

    def _revive_bot(self, bot):
        """A Range bot's body gets its health back (the client stands its ragdoll up) and its overhead presenter,
        which its death removed, starts again once that update arrives."""
        for viewer in self.players:
            if viewer.client is None or bot.entity not in viewer.bots_seen:
                continue
            update = EntityUpdate(bot.entity, OP_UPDATE, lambda offset, most=bot.max_health:
                world.update(offset, world.health(most, most, *bot.pools)))
            viewer.client.queue_entities([update])
            stream, creation, _ = viewer.bot_nameplates.get(bot.controller, (None, None, 0))
            if stream is not None and creation is None:
                viewer.bot_nameplates[bot.controller] = stream, update, 0

    def _damage_bot(self, player, target, amount, now, critical=False, hits=1):
        before = target.health
        target.damage(amount, now, hits, player.body)
        done = before - target.health
        self._charge_ult(player, done)
        self._send_bot_health(target)
        if done <= 0:
            return
        if player.client is not None:
            frame = max(0, player.frame_at(now))
            player.client.queue_reliable(25001, {'+0x78': {'+0x0': {'+0x0': target.entity},
                '+0x4': round(done), '+0x8': int(critical) | (0 if target.health else 0x2)}, '+0x84': frame})  # bit 1: the kill
        target.damage_by[player.entity] = target.damage_by.get(player.entity, 0) + done
        player.count('damage', done)
        if target.hero:
            player.count('hero_damage', done)
        if not target.health:  # shortcut: the player's hit is always the final blow; split them when assists come
            player.streak += 1
            for name, amount in (('eliminations', 1), ('final_blows', 1), ('kill_streak', player.streak)):
                player.count(name, amount)
        if not target.health:
            # the share of its health he took: the number beside "ELIMINATED", and his On Fire points (onfire.py)
            percent = min(100, round(100 * target.damage_by[player.entity] / target.max_health))
            player.fire.add(percent)
        if not target.health and player.client is not None:
            # shortcut: notify the finisher; credit other contributors when assists are implemented.
            player.client.queue_reliable(25005, {'+0x78': 0, '+0x80': {'+0x0': player.body},
                '+0x84': {'+0x0': target.entity}, '+0x88': percent, '+0x89': False})

    def _charge_ult(self, player, amount):
        """Damage dealt and healing done fill the ultimate 1:1 in points up to ULT_POINTS; the body frame carries it.
        shortcut: no passive charge over time."""
        if (amount <= 0 or ULT_CHARGE not in player.body_extra_variables and player.body_vm is None
                or player.ult_charge >= player.ult_most or player.visor_until):
            return
        player.ult_charge = min(player.ult_most, player.ult_charge + int(amount))  # healer bots heal fractions
        if player.body_vm is not None:
            player.body_vm.sync(player)
            return
        player.body_extra_variables[ULT_CHARGE] = ult_meter(player.ult_charge, player.ult_most)
        player.body_dirty = True

    def change_form(self, player, now):
        """D.Va's Eject! (the mech at 1 health), Self-Destruct and Call Mech: the pilot's or a new mech's body where
        she stands (Call Mech's falls in first). shortcut: no wreck; the Self-Destruct mech stands still until it blows."""
        change, player.form_change = player.form_change, None
        at, base = (tuple(player.mover.position), player.mover.yaw), heroes()[player.hero.guid]
        if change == 'call_mech':  # her call first; Match.land_mech brings the mech CALL_MECH_CAST later
            player.ult_instance = dva_call_mech()
            player.refresh_abilities()
            player.mech_call_at = now + CALL_MECH_CAST
            player.mech_drop = (self._along(at[0], player.mover.yaw, 1, CALL_MECH_AHEAD, 1.0), at[1])
            if player.client is not None:
                player.seen[player.entity | DROP_SLOT] = False
                player.client.queue_entities([EntityUpdate(player.entity | DROP_SLOT, OP_CREATE,
                    lambda origin, drop=player.mech_drop: self._drop_create(origin, player, drop))])
            player.pending_spawn_voice = voice(player.body, *DVA_CALL_MECH_VOICE)
            player.body_dirty = True
            return
        self._eject(player, change, at, base, now)

    def land_mech(self, player, now):
        """Call Mech's mech lands (Match.change_form) and she is in it: CALL_MECH_DAMAGE to enemy bots within
        CALL_MECH_RADIUS."""
        player.mech_call_at = 0.0
        at = player.mech_drop or (tuple(player.mover.position), player.mover.yaw)
        self.end_mech_drop(player)
        self.switch_hero(player, heroes()[player.hero.guid], at=at)
        for bot in self._enemy_bots(player):
            if bot.health and math.dist(bot.position, at[0]) <= CALL_MECH_RADIUS:
                self._damage_bot(player, bot, CALL_MECH_DAMAGE, now)
        log.info('[game] D.Va calls her mech body=%08X', player.body)

    def end_mech_drop(self, player):
        """Remove the falling mech (Match.change_form) from her client."""
        if player.mech_drop is None:
            return
        player.mech_drop = None
        player.seen.pop(player.entity | DROP_SLOT, None)
        if player.client is not None:
            player.client.queue_entities([EntityUpdate(player.entity | DROP_SLOT, OP_DESTROY)])

    def _drop_create(self, origin, player, drop):
        """Her called mech's body at the top of its fall, facing her way; no movement block, as a bot's: the movement
        records place it."""
        (x, y, z), yaw = drop
        most = float(PLAYER_KIT['D.Va'][0])
        components = world.health(most, most)
        components[26] = [player.team_bit]
        components[123] = [DVA]
        position = (x, y + CALL_MECH_DROP, z)
        return world.create(origin, SPAWN_BODY[DVA], components, flags=BODY_FLAGS, position=position,
                            rotation=_rotation(yaw), skin=player.skin)

    def _along(self, feet, yaw, sign, reach, margin):
        """feet moved `reach` m ahead (sign 1) or behind (-1) along the yaw, kept `margin` off a wall."""
        angle = yaw * TURN
        way = (sign * math.sin(angle), 0.0, sign * math.cos(angle))
        wall = self.mesh and self.mesh.ray((feet[0], feet[1] + 1.0, feet[2]), way, reach + margin)
        if wall:
            reach = max(0.0, wall[0] - margin)
        return tuple(f + w * reach for f, w in zip(feet, way))

    def _eject(self, player, change, at, base, now):
        """Eject! or Self-Destruct: the pilot thrown back from EJECT_BACK behind the mech (target.EJECT_LAUNCH)."""
        if change == 'self_destruct' and player.body_script is not None and player.client is not None:
            # the mech's last owner frame starts its own countdown; the client runs the fuse and the blast effect
            player.ult_instance = dva_self_destruct(SELF_DESTRUCT_FUSE)
            player.refresh_abilities()
            cmfd = player.body_script.next_cmfd(self._newest_frame(player))
            player.client.queue_entities([self._body_weapon_frame(player, cmfd, player.last_rifle_update, refresh=True)])
        mech, feet, angle = player.body, at[0], player.mover.yaw * TURN
        back = (-math.sin(angle), 0.0, -math.cos(angle))
        at = (self._along(feet, player.mover.yaw, -1, EJECT_BACK, 0.4), at[1])
        if self.mesh:  # without one the mover walks flat at its start height
            at = ((at[0][0], at[0][1] + EJECT_RISE, at[0][2]), at[1])
        speed, lift = EJECT_LAUNCH
        self.switch_hero(player, pilot(base), at=at, keep_old=change == 'self_destruct',
                         launch=(back[0] * speed, lift, back[2] * speed))
        player.invulnerable_until = now + EJECT_SECONDS
        player.pending_spawn_voice = voice(player.body, *(DVA_SELF_DESTRUCT_VOICE if change == 'self_destruct'
                                                          else DVA_EJECT_VOICE))
        if change == 'self_destruct':
            # removed after the blast so its effect plays out (shortcut: 1.5 s, not the effect's length)
            self.mech_bombs.append([player, mech, feet, now + SELF_DESTRUCT_FUSE, now + SELF_DESTRUCT_FUSE + 1.5])
        log.info('[game] D.Va %s body=%08X', change.replace('_', '-'), player.body)

    def _tick_mech_bombs(self, now):
        """A Self-Destruct mech blows up: SELF_DESTRUCT_DAMAGE falling off over its radius to enemy bots in the open
        (cover blocks it), and its owner's Call Mech fills."""
        for bomb in list(self.mech_bombs):
            player, mech, feet, blow_at, gone_at = bomb
            if now >= gone_at:
                self.mech_bombs.remove(bomb)
                if player.client is not None:
                    player.client.queue_entities([EntityUpdate(mech, OP_DESTROY)])
                self.destroy_for_others(player, mech)
            if blow_at is None or now < blow_at:
                continue
            bomb[3] = None
            core = (feet[0], feet[1] + 1.0, feet[2])  # shortcut: the mech's middle taken 1 m above its feet
            most, least = SELF_DESTRUCT_DAMAGE
            for bot in self._enemy_bots(player):
                centre = bot.center()
                distance = math.dist(centre, core)
                if not bot.health or distance > SELF_DESTRUCT_RADIUS:
                    continue
                way = tuple((b - c) / distance for b, c in zip(centre, core)) if distance else (0.0, 1.0, 0.0)
                if distance and self.mesh and self.mesh.ray(core, way, distance):
                    continue  # cover
                self._damage_bot(player, bot, most + (least - most) * distance / SELF_DESTRUCT_RADIUS, now)
            if player.hero.body in DVA_ULTS and player.hero.body != SPAWN_BODY[DVA]:
                player.ult_charge = player.ult_most
                player.body_extra_variables[ULT_CHARGE] = ult_meter(player.ult_charge, player.ult_most)
                player.body_dirty = True
            log.info('[game] D.Va mech blows up body=%08X', mech)

    def boost_collide(self, player, command, damage=BOOST_DAMAGE, knock=True):
        """Boosters, and Reinhardt's Charge and Genji's Swift Strike with their own damage: an enemy bot the body
        touches takes `damage` once per flight and, with `knock`, is knocked along the flight's heading at BOOST_KNOCK
        for KNOCK_SECONDS, or until a wall."""
        feet, angle = player.mover.position, command.yaw * TURN
        heading = (math.sin(angle), 0.0, math.cos(angle))
        for bot in self._enemy_bots(player):
            if (not bot.health or bot.entity in player.boost_hits or abs(bot.position[1] - feet[1]) > 2.0
                    or math.hypot(bot.position[0] - feet[0], bot.position[2] - feet[2]) > BOOST_REACH):
                continue
            player.boost_hits.add(bot.entity)
            self._damage_bot(player, bot, damage, self.now)
            if not knock:
                continue
            reach = BOOST_KNOCK * KNOCK_SECONDS
            wall = self.mesh and self.mesh.ray(bot.center(), heading, reach)
            if wall:
                reach = max(0.0, wall[0] - 0.4)  # stop short of the wall by about the bot's radius
            bot.knocked = (tuple(h * BOOST_KNOCK for h in heading), self.now + reach / BOOST_KNOCK, self.now)
            log.info('[game] boosters hit entity=%08X knock=%.2f m', bot.entity, reach)

    def step_point(self, player, command, reach):
        """Where a Shadow Step aimed by `command` puts the feet, or None where the aim finds no place: the client's
        own marker with its feed on (client_sync.marker), else the server's port of the client's targeting query
        (shadow_step.place) on the mover's collision."""
        drawn = client_sync.marker(command.frame)
        if drawn is not None:  # the client's own marker (client_feed.c): there, or nowhere when it shows no place
            return tuple(map(quantise, drawn[0])) if drawn[1] else None
        found = shadow_step.place(player.mover.world, player.eye(), aim_direction(command.yaw, command.pitch), reach)
        return found and tuple(map(quantise, found))

    def falling(self, player, air):
        """The player falls with `air` metres of nothing under the feet (no map: any fall counts)."""
        mover = player.mover
        if not mover.airborne or mover.velocity[1] >= 0.0:
            return False
        x, y, z = mover.position
        return not (self.mesh and self.mesh.ray((x, y + 0.25, z), (0.0, -1.0, 0.0), air + 0.25))

    def _enemy_bots(self, player):
        """Bots the player can damage: seen by its client and on the other team (no friendly fire)."""
        return [bot for bot in self.bots if bot.entity in player.bots_seen and bot.team != player.team]

    def fire_helix(self, player, command):
        """Fly a Helix rocket from the eye along the aim to the first wall or bot (lifetime range)."""
        shot = self._fly(player, command, HELIX_RANGE, HELIX_SPEED, None)
        self._confirm_rocket(player, shot)

    def _confirm_rocket(self, player, shot):
        """Link a fresh native visual after the owner Volley arrives; predicted objects may already have collided."""
        if shot is None or player.client is None or player.body_script is None or player.helix is None:
            return
        self.rocket_ids += 1
        entity = 0xB2000000 + self.rocket_ids
        shot.native_owner = player
        self.helix_rockets.append(HelixRocket(player, player.body, entity, shot, player.helix.serial))

    def _tick_rockets(self, now):
        alive = []
        for rocket in self.helix_rockets:
            player, shot, entity = rocket.player, rocket.shot, rocket.entity
            if player.client is None:
                continue
            valid = player.has_body and player.body == rocket.body
            ended = not valid or shot.landed or now >= shot.end
            if ended:
                player.client.queue_entities([EntityUpdate(entity, OP_DESTROY)])
                continue
            if not rocket.records and rocket.begin_frame and player.body_script.delivered >= rocket.begin_frame:
                tick, position = self._newest_frame(player), shot.position(now)
                distance = math.dist(shot.origin, shot.target)
                direction = tuple((t - o) / distance for o, t in zip(shot.origin, shot.target))
                velocity = tuple(value * HELIX_SPEED for value in direction)
                rocket.records = (
                    EntityUpdate(entity, OP_CREATE, lambda origin, position=position: world.create(
                        origin, HELIX_DEFINITIONS[0], {}, position=position)),
                    EntityUpdate(rocket.body, shot=world.shot_record(0, 1, 16, rocket.serial & 63,
                        shot.origin, direction, entities=(entity,)), shot_time=tick),
                    EntityUpdate(entity, projectile=world.projectile_state(tick, position, velocity,
                        event_count=0, source_entities=(rocket.body, rocket.body))),
                )
                player.client.queue_entities(list(rocket.records))
                log.info('[game] helix native entity=%08X serial=%d tick=%d', entity, rocket.serial, tick)
            alive.append(rocket)
        self.helix_rockets = alive

    def fire_ability(self, player, command, hit):
        """A kits.ABILITIES cast: like a Helix rocket, but it lands at its range when it meets nothing."""
        if hit.shots > 1:
            self.salvos.append([time.time(), 0, player, hit])
        else:
            self._fly(player, command, hit.range, hit.speed, hit)

    def _tick_salvos(self, now):
        """Launch each salvo's due shots from its launchers, aimed where the shooter's crosshair is now."""
        for salvo in self.salvos:
            start, done, player, hit = salvo
            while done < hit.shots and now >= start + done / hit.rate and player.has_body and player.health > 0:
                mover = player.mover
                angle = mover.yaw * TURN
                right, forward = (-math.cos(angle), 0.0, math.sin(angle)), (math.sin(angle), 0.0, math.cos(angle))
                x, y, z = hit.launchers[done % len(hit.launchers)]
                origin = tuple(p + r * x + f * z for p, r, f in zip(mover.position, right, forward))
                aim = Command(0, yaw=mover.yaw, pitch=mover.pitch)  # where the crosshair is now
                self._fly(player, aim, hit.range, hit.speed, hit, (origin[0], origin[1] + y, origin[2]))
                done += 1
            salvo[1] = done
        self.salvos = [s for s in self.salvos if s[1] < s[3].shots and s[2].has_body and s[2].health > 0]

    def _fly(self, player, command, reach, speed, hit, launcher=None):
        """From the eye along the aim; from `launcher`, toward the point the aim ray meets."""
        origin, direction = player.eye(), aim_direction(*command.aim())
        if launcher is not None:
            wall = self.mesh and self.mesh.ray(origin, direction, reach)
            aim = [o + d * (wall[0] if wall else reach) for o, d in zip(origin, direction)]
            offset = [a - l for a, l in zip(aim, launcher)]
            length = math.hypot(*offset) or 1.0
            origin, direction = launcher, tuple(o / length for o in offset)
        distance, direct = reach, None
        wall = self.mesh and self.mesh.ray(origin, direction, reach)
        if wall:
            distance = wall[0]
        pierce = hit is not None and hit.pierce
        for bot in self._enemy_bots(player):  # friendly bots neither stop it nor take damage
            found = bot.ray(origin, direction, distance)
            if found is not None and pierce:  # Fire Strike: each enemy on the line is hit as the strike passes
                point = tuple(o + d * found[0] for o, d in zip(origin, direction))
                self.rockets.append((time.time() + found[0] / speed, point, bot, [bot], player, hit))
            elif found is not None and found[0] < distance:
                distance, direct = found[0], bot
        impact = tuple(o + d * distance for o, d in zip(origin, direction))
        flight = distance / speed if speed else 0.0
        entity = HELIX_PROJECTILE if hit is None else hit.entity
        # Other viewers retain the stepped effect; the Helix owner's native startup is queued separately.
        shot = None
        if entity is not None and flight:
            shot = Projectile(origin, impact, self.now, speed, entity)
            shot.components = {26: [player.team_bit]}  # the shooter's team: without it the client draws enemy colours
            self.projectile_count += 1
            shot.index = self.projectile_count & 0xFFFF
            self.projectiles.append((None, shot))  # no target player: it is drawn, the rocket list lands it
        if pierce or (hit is None and direct is None and not wall):
            return shot  # a rocket flies out of its lifetime without exploding; a piercing strike has no landing
        self.rockets.append((time.time() + flight, impact, direct, self._enemy_bots(player), player, hit))
        return shot

    def launch_orb(self, player, command, orb):
        """A Biotic Orb from the eye along the aim, to the first wall or the end of its lifetime; _tick_orbs heals or
        hurts around it as it goes."""
        origin, direction = player.eye(), aim_direction(*command.aim())
        reach = orb.speed * orb.seconds
        wall = self.mesh and self.mesh.ray(origin, direction, reach)
        distance = wall[0] if wall else reach
        end = tuple(o + d * distance for o, d in zip(origin, direction))
        if distance:
            shot = Projectile(origin, end, self.now, orb.speed, orb.entity)
            shot.components = {26: [player.team_bit]}
            self.projectile_count += 1
            shot.index = self.projectile_count & 0xFFFF
            self.projectiles.append((None, shot))
        self.orbs.append([self.now, origin, end, orb, player, orb.total, self.now + ORB_TICK])
        log.info('[game] %s orb out to %.1f m', 'healing' if orb.heal else 'damage', distance)

    def _orb_targets(self, owner, orb, at):
        """Who an orb at `at` touches: hurt allies (players and bots of the owner's team) or living enemy bots."""
        if not orb.heal:
            return [bot for bot in self._enemy_bots(owner) if bot.health
                    and math.dist(bot.center(), at) <= orb.radius]
        players = [p for p in self.players if p.team == owner.team and p.has_body and 0 < p.health < p.max_health
                   and math.dist(chest(p.mover.position), at) <= orb.radius]
        bots = [b for b in self.bots if b.team == owner.team and b.health
                and b.health < b.max_health and math.dist(b.center(), at) <= orb.radius]
        return players + bots

    def _tick_orbs(self, now):
        """Every ORB_TICK an orb gives its rate's share to each target it touches, until its amount or lifetime
        runs out (target.Orb)."""
        for state in self.orbs:
            start, origin, end, orb, owner, left, tick = state
            length = math.dist(origin, end)
            while left > 0 and tick <= min(now, start + orb.seconds):
                along = min(orb.speed * (tick - start), length) / length if length else 0
                at = tuple(o + (e - o) * along for o, e in zip(origin, end))
                for target in self._orb_targets(owner, orb, at):
                    amount = min(orb.rate * ORB_TICK, left)
                    before = target.health
                    if orb.heal:
                        self._heal(target, amount, owner)
                    else:
                        self._damage_bot(owner, target, amount, now)
                    done = abs(target.health - before)
                    left -= done
                    if orb.heal:
                        self._charge_ult(owner, done)
                    log.info('[game] orb %s %08X %.1f health=%.1f left=%.1f', 'heal' if orb.heal else 'damage',
                             getattr(target, 'entity', None) or target.body, done, target.health, left)
                    if left <= 0:
                        break
                tick += ORB_TICK
            state[5], state[6] = left, tick
        self.orbs = [state for state in self.orbs if state[5] > 0 and state[6] <= state[0] + state[3].seconds]

    def _land_rockets(self, now):
        landed = [rocket for rocket in self.rockets if now >= rocket[0]]
        self.rockets = [rocket for rocket in self.rockets if now < rocket[0]]
        for _, impact, direct, enemies, player, hit in landed:
            for bot in enemies:
                if hit is None:
                    amount = helix_splash(math.dist(impact, bot.center())) + (HELIX_IMPACT if bot is direct else 0)
                else:
                    amount = ability_splash(hit, math.dist(impact, bot.center())) + (hit.direct if bot is direct else 0)
                if amount and bot.health:
                    self._damage_bot(player, bot, amount, now)
                    log.info('[game] %s hit %08X damage=%d health=%d', 'helix' if hit is None else 'ability',
                             bot.entity, amount, bot.health)
            pos = player.mover.position
            if (hit is not None and hit.kick and player.has_body and player.health > 0
                    and math.dist(impact, (pos[0], pos[1] + 1.0, pos[2])) <= hit.radius):
                player.mover.blast(impact, *hit.kick)  # the mine jump
            if hit is not None and not hit.self_harm:
                continue
            # The shooter takes half the splash (wiki, Sept 2022: Helix self 20-40). shortcut: no rocket-jump knockback.
            pos = player.mover.position
            own_distance = math.dist(impact, (pos[0], pos[1] + 1.0, pos[2]))
            own = (helix_splash(own_distance) if hit is None else ability_splash(hit, own_distance)) // 2
            if own and player.has_body and player.health > 0:
                self._damage_player(player, own, now)

    def _can_be_shot(self, player):
        return (self.gameplay_enabled and self.bot_attacks_enabled and player.client is not None
            and player.has_body and player.spawned and not player.select_open and player.health > 0
            and player.body_script is not None
            and not any(isinstance(c, Timed) and c.hidden for c in player.cooldowns))  # Stealth

    def _bot_attack(self, player, now):
        if not self.bots or not self._can_be_shot(player):
            return
        for bot in self.bots:
            if not bot.hero and bot.team != player.team and bot.entity in player.bots_seen:
                seen = self.mesh is None or self.mesh.clear(chest(bot.position, 1.2), chest(player.mover.position))
                shot = bot.attack_player(player.mover.position, now, seen)
                if shot is not None:
                    self.projectile_count += 1
                    shot.index = self.projectile_count & 0xFFFF
                    shot.by = bot.entity
                    self.projectiles.append((player, shot))

    def _bots_fight(self, now):
        """Practice vs AI: each hero bot fires Soldier's rifle at the nearest enemy it sees, player or bot,
        re-picked every RETARGET_SECONDS; where it sees one is its team's sighting."""
        if not self.bot_attacks_enabled:
            return
        scale, aim_error = DIFFICULTY[self.difficulty]
        for bot in self.bots:
            if not bot.hero or not bot.health:
                continue
            if self._signature(bot, now, scale):
                bot.next_attack_at = None  # mending: no fire
                continue
            if now >= bot.retarget_at or not self._shootable(bot.target):
                target, bot.retarget_at = self._pick_target(bot), now + RETARGET_SECONDS
                if target is not bot.target:
                    bot.target, bot.in_flight = target, []  # shots in flight were aimed at the old target
            if bot.target is None:
                bot.next_attack_at = None
                continue
            position, yaw = self._stance(bot.target)
            self.sightings[bot.team] = (tuple(position), now)
            volumes = bot.target.volumes if isinstance(bot.target, TrainingTarget) else hitbox.SOLDIER
            blocked = partial(self._matrix_blocks, bot.team) if bot.hero not in MATRIX_IGNORES else None
            hits = bot.fire(position, yaw, now, True, aim_error, volumes, blocked)
            if hits:
                before = bot.target.health
                self._hurt(bot.target, sum(bot.weapon.damage_at(distance) for distance in hits) * scale, now, bot.entity,
                           len(hits))
                if bot.ability is not None and bot.ability.kind == 'leech' and bot.health < bot.max_health:
                    self._heal(bot, (before - bot.target.health) * bot.ability.amount)

    def _signature(self, bot, now, scale):
        """Uses a hero bot's Ability when it applies; True while a mend keeps it from firing."""
        ability, target = bot.ability, bot.target
        if ability is None or now < bot.hacked_until:
            return False
        if now < bot.ability_ready and ability.kind != 'discord':
            return now < bot.effect_until and ability.kind == 'mend'
        if ability.kind == 'mend' and bot.health < bot.max_health / 2:
            bot.effect_until = now + ability.seconds
            bot.ability_ready = bot.effect_until + ability.cooldown
            return True
        if target is None or not self._shootable(target):
            return False
        if ability.kind == 'guard' and bot.health < bot.max_health:
            bot.effect_until, bot.soak = now + ability.seconds, ability.amount
            bot.ability_ready = bot.effect_until + ability.cooldown
        elif ability.kind in ('burst', 'discord') and math.dist(bot.position, self._stance(target)[0]) <= ability.reach:
            if ability.kind == 'discord':
                target.discorded = (now + 2 * RETARGET_SECONDS, ability.amount)  # while Zenyatta keeps it targeted
            else:
                bot.ability_ready = now + ability.cooldown
                bot.ability_uses += 1
                if (bot.hero not in MATRIX_CASTS
                        or not self._matrix_blocks(bot.team, chest(bot.position, 1.2), chest(self._stance(target)[0]))):
                    self._hurt(target, ability.amount * scale, now, bot.entity)
        return False

    def _hurt(self, target, amount, now, by=None, hits=1):
        if isinstance(target, TrainingTarget):
            target.damage(amount, now, hits, by)
            self._send_bot_health(target)
        else:
            self._damage_player(target, amount, now, by, hits)

    def _bots_heal(self, now):
        """Practice vs AI: every HEAL_PERIOD each healer bot heals its hurt allies, players and bots, in reach
        and in sight, as its Heal says."""
        if now < self.next_heal:
            return
        self.next_heal = now + HEAL_PERIOD
        for bot in self.bots:
            kit = bot.heal
            if (bot.ability is not None and bot.ability.kind == 'mend'
                    and bot.hacked_until <= now < bot.effect_until and bot.health):
                self._heal(bot, bot.ability.amount * HEAL_PERIOD)
            if kit is None or not bot.health or (kit.idle and bot.target is not None):
                continue
            hurt = ([p for p in self.players if p.team == bot.team and self._shootable(p) and p.health < p.max_health]
                    + [b for b in self.bots if b is not bot and b.hero and b.team == bot.team
                       and self._shootable(b) and b.health < b.max_health])
            hurt = [a for a in hurt if math.dist(bot.position, self._stance(a)[0]) <= kit.reach and (
                self.mesh is None or self.mesh.clear(chest(bot.position, 1.2), chest(self._stance(a)[0])))]
            if kit.one:
                hurt = sorted(hurt, key=lambda a: a.health / a.max_health)[:1]
            for ally in hurt:
                self._heal(ally, kit.rate * HEAL_PERIOD)
            if kit.own and bot.health < bot.max_health:
                self._heal(bot, kit.own * HEAL_PERIOD)

    def _heal(self, ally, amount, healer=None):
        """healer: the player whose healing it is, for his Healing Done (board.py) and his fire (onfire.py)."""
        before, ally.health = ally.health, min(ally.max_health, ally.health + amount)
        if isinstance(ally, TrainingTarget):
            self._send_bot_health(ally)
        else:
            self._send_player_health(ally)
        if healer is not None and ally.health > before:
            healer.count('healing', ally.health - before)
            healer.fire.add((ally.health - before) / onfire.HEALTH_A_POINT)

    def _send_bot_weapons(self):
        """Hero bots show their primary fire to each client that has them: the Volley on while they shoot with
        a Volley event per shot, and each shot as a ch2 event, which is what plays a remote shot of any Volley
        but an automatic one. An automatic Volley (AUTOMATIC) fires itself, up to the ammo var per start, so it is
        off while the bot reloads and starts again with each magazine. A dead bot's fire goes off too."""
        due = [shot for shot in self.bot_shots if shot[0] <= self.now]
        self.bot_shots = [shot for shot in self.bot_shots if shot[0] > self.now]
        for _, viewer, entity, impact in due:
            if viewer.client is None:
                continue
            if impact is None:
                viewer.client.queue_entities([EntityUpdate(entity, OP_DESTROY)])
                continue
            # Landed: stopped where it hit, then destroyed.
            viewer.client.queue_entities([EntityUpdate(entity, projectile=world.projectile_state(
                self._newest_frame(viewer), impact, (0.0, 0.0, 0.0), stopped=True))])
            self.bot_shots.append((self.now + SHOT_LINGER, viewer, entity, None))
        for bot in self.bots:
            if not bot.hero:
                continue
            armed = hero_name(bot.hero) in WEAPON_GRAPHS
            firing = (bool(bot.health) and bot.next_attack_at is not None and bot.target is not None
                      and (bot.rounds > 0 or hero_name(bot.hero) not in AUTOMATIC))
            state = (hero_name(bot.hero), bot.weapon.rounds, firing, bot.shots, bot.ability_uses,
                     next(((p.body, p.staff_boost) for p in self.players if p.staff_target == bot.entity), None),
                     None if bot.health else (bot.killer,))
            for viewer in self.players:
                if viewer.client is None or bot.entity not in viewer.bots_seen:
                    continue
                stream, sent = viewer.bot_weapons.get(bot.entity, (None, None))
                if sent == state or (not armed and sent is None and state[5:] == (None, None)):
                    continue  # unchanged, or an unarmed bot never healed or killed: nothing to show
                stream = stream or Stream(bot.entity)
                viewer.bot_weapons[bot.entity] = (stream, state)
                updates = [stream.full_frame(remote_volley_frame(*state))]
                if armed and sent is not None and bot.target is not None:
                    for count, changed, owner_state, drawn in (
                            (bot.shots, bot.shots != sent[3], WEAPON_GRAPHS[state[0]][1], WEAPON_SHOTS.get(state[0])),
                            (bot.ability_uses, bot.ability_uses != sent[4], ABILITY_VOLLEYS.get(state[0], (None,))[0], None)):
                        if changed and owner_state is not None:
                            updates += self._bot_shot(viewer, bot, owner_state, count, drawn)
                viewer.client.queue_entities(updates)

    def _send_range_bot_frames(self, now):
        """A Range bot's own statescript, sent whenever it changes, from its first death on. Revived, its body runs its
        reassembly Anim (target.REBUILD) for the clip's length, then stops it so the next revival starts it again.
        Its kill feed graph (target.KILL_FEED) holds its killer and pulses once per death: the client posts a feed
        line for each pulse it has not seen, and for a pulser it first reads only while the bot lies dead."""
        for bot in self.bots:
            if bot.hero or not bot.deaths:
                continue
            state = bot.rebuilding(now), bot.deaths
            for viewer in self.players:
                if viewer.client is None or bot.entity not in viewer.bots_seen:
                    continue
                stream, sent = viewer.bot_weapons.get(bot.entity, (None, (False, 0)))
                if sent == state:
                    continue
                stream = stream or Stream(bot.entity)
                viewer.bot_weapons[bot.entity] = stream, state
                rebuild = Instance(1, REBUILD, active={REBUILD_SLOT: anim_payload()} if state[0] else {})
                feed = kill_feed(2, False, bot.killer, not bot.health and state[1] != sent[1], bot.deaths)
                viewer.client.queue_entities([stream.full_frame(remote_full_frame([rebuild, feed], {}))])

    def _bot_shot(self, viewer, bot, state, count, kind=None):
        """The ch2 event of the bot's latest shot of the Volley at owner `state` (`count`: its shot and event counter),
        from its muzzle height toward its target's chest. A mode-1 Volley's shot (`kind`: its WEAPON_SHOTS entity) also
        names a projectile made for it, as a Helix rocket's does. A flying one also gets a ProjectileVisual snapshot, the
        part that draws it, and is destroyed when it has flown to the chest. A hitscan one (Cassidy) is drawn by the
        record alone, so it is made under the map, as LizRollin's _show_others does, and destroyed SHOT_LINGER later
        (research/bot-fire.md, run 31)."""
        origin = chest(bot.position, 1.2)
        offset = tuple(a - o for a, o in zip(chest(self._stance(bot.target)[0]), origin))
        if kind is None:
            shot = EntityUpdate(bot.entity, shot=world.shot_record(count, 1, state, count, origin, offset))
            shot.resend = False
            return [shot]
        self.rocket_ids += 1
        entity, tick, speed = 0xB2000000 + self.rocket_ids, self._newest_frame(viewer), bot.weapon.speed
        record = EntityUpdate(bot.entity, shot=world.shot_record(count, 1, state, count, origin, offset, entities=(entity,)),
                              shot_time=tick)
        if not speed:
            self.bot_shots.append((self.now + SHOT_LINGER, viewer, entity, None))
            return [EntityUpdate(entity, OP_CREATE, lambda o: world.create(o, kind, {}, position=UNDER_MAP)), record]
        distance = math.hypot(*offset) or 1.0
        self.bot_shots.append((self.now + distance / speed, viewer, entity, tuple(o + d for o, d in zip(origin, offset))))
        return [EntityUpdate(entity, OP_CREATE, lambda o, position=origin: world.create(o, kind, {}, position=position)),
                record,
                EntityUpdate(entity, projectile=world.projectile_state(tick, origin, tuple(d / distance * speed for d in offset),
                                                                       source_entities=(bot.entity, bot.entity)))]

    def _pick_target(self, bot):
        """The nearest living enemy, player or hero bot, within rifle range and in sight of the bot."""
        enemies = ([p for p in self.players if p.team != bot.team and self._shootable(p)]
                   + [b for b in self.bots if (b.hero or b.behaviour == PAYLOAD) and b.team != bot.team and self._shootable(b)])
        reach = sorted((d, n) for n, e in enumerate(enemies)
                       if (d := math.dist(bot.position, self._stance(e)[0])) <= RIFLE_RANGE)
        for _, n in reach:
            if self.mesh is None or self.mesh.clear(chest(bot.position, 1.2), chest(self._stance(enemies[n])[0])):
                return enemies[n]
        return None

    def _shootable(self, target):
        if isinstance(target, TrainingTarget):
            return bool(target.health)
        return target is not None and self._can_be_shot(target)

    @staticmethod
    def _stance(target):
        """(feet position, yaw radians) of a player or bot."""
        if isinstance(target, TrainingTarget):
            return target.position, math.radians(target.yaw)
        return target.mover.position, target.mover.yaw * math.pi / 32768

    def _matrix_blocks(self, team, start, end):
        return any(player.team != team and player.hero.guid == DVA and self._can_be_shot(player)
                   and any(isinstance(c, Held) and c.active for c in player.cooldowns)
                   and matrix_intersects(player.eye(), player.mover.yaw, player.mover.pitch, start, end)
                   for player in self.players)

    def _land_projectiles(self, now):
        for player, shot in self.projectiles:
            if (not shot.landed and player is not None and self._matrix_blocks(1 - player.team,
                    shot.position(shot.checked_at), shot.position(now))):
                shot.landed = True
            shot.checked_at = now
            if not shot.landed and now >= shot.end:
                shot.landed = True
                if player is not None and self._can_be_shot(player) and shot.hits(player.mover.position, player.mover.yaw * math.pi / 32768):
                    self._damage_player(player, ATTACK_DAMAGE, now, shot.by)

    def _projectile_updates(self, viewer):
        """Every frame, each projectile's effect entity is destroyed and re-created a step further: it has
        no mover for movement records. Its id's low byte counts the generations."""
        updates = []
        for _, shot in self.projectiles:
            if shot.native_owner is viewer:
                continue
            old = shot.shown.pop(viewer, None)
            if old is not None:
                updates.append(EntityUpdate(old, OP_DESTROY))
            if not shot.landed:
                entity = 0xB1000000 | shot.index << 8 | ((old or 0) & 0xFF) % 255 + 1
                position = shot.position(self.now)
                create = EntityUpdate(entity, OP_CREATE,
                    lambda origin, position=position, kind=shot.entity, turn=shot.rotation(), shot=shot: world.create(
                        origin, kind, dict(shot.components), position=position, rotation=turn))
                create.resend = False  # shortcut: a lost step is skipped, not replayed after its destroy
                updates.append(create)
                shot.shown[viewer] = entity
        self.projectiles = [(p, shot) for p, shot in self.projectiles if not shot.landed or shot.shown]
        return updates

    def throw_field(self, player):
        """Biotic Field: its entity on the ground at the player's feet, healing the player's team. Called when Volley 4
        throws (FIELD_RELEASE after the press). shortcut: the canister (entity 0x072D) is not flown; the field
        appears at the feet at release (the Hero Info clip shows it landing ~1-2 m ahead)."""
        self.field_count += 1
        entity = 0xB2000001 | (self.field_count & 0xFFFF) << 8
        position, team = tuple(player.mover.position), player.team
        viewers = [viewer for viewer in self.players if viewer.client is not None and viewer.spawned]
        for viewer in viewers:
            stream = Stream(entity)
            cmfd = stream.next_cmfd(self._newest_frame(viewer))
            viewer.client.queue_entities([EntityUpdate(entity, OP_CREATE, lambda origin: world.create(
                origin, FIELD_ENTITY, {26: [TEAM_BIT << team]}, position=position,
                movement=Movement(position))), stream.full_frame(field_visual_frame(cmfd))])
        self.fields.append([entity, position, team, self.now, self.now + FIELD_SECONDS, viewers, player])

    def _tick_fields(self, now):
        """Every FIELD_PERIOD a field heals its team's living players and hero bots within its radius; then it
        goes away."""
        for field in self.fields:
            entity, position, team, _, end, viewers, owner = field
            while field[3] < end and now >= field[3]:
                field[3] += FIELD_PERIOD
                for bot in self.bots:
                    if (bot.hero and bot.team == team and bot.health and bot.health < bot.max_health
                            and math.dist(bot.position, position) <= FIELD_RADIUS):
                        before = bot.health
                        self._heal(bot, FIELD_HEAL, owner)
                        self._charge_ult(owner, bot.health - before)
                        log.info('[game] biotic field heal bot=%08X health=%d', bot.entity, bot.health)
                for player in self.players:
                    if (player.team == team and player.has_body and 0 < player.health < player.max_health
                            and math.dist(player.mover.position, position) <= FIELD_RADIUS):
                        before = player.health
                        self._heal(player, FIELD_HEAL, owner)
                        self._charge_ult(owner, player.health - before)
                        log.info('[game] biotic field heal body=%08X health=%d', player.body, player.health)
            if now >= end:
                for viewer in viewers:
                    if viewer.client is not None:
                        viewer.client.queue_entities([EntityUpdate(entity, OP_DESTROY)])
        self.fields = [field for field in self.fields if now < field[4]]

    def _send_player_health(self, player):
        health, body = player.health, player.body
        if player.body_vm is not None:  # the HUD's number and the graphs' dead check: the VM's next owner frame
            player.body_vm.sync(player)
        if player.body_script is not None:  # the HUD's number: graph463's synced vars, re-sent with the body frame
            player.body_extra_variables.update(health_variables(health, player.max_health))
            player.body_dirty = True
        for viewer in self.players:
            if viewer.client is not None and (viewer is player or body in viewer.seen):
                viewer.client.queue_entities([EntityUpdate(body, OP_UPDATE,
                    lambda offset, health=health, most=player.max_health: world.update(offset, world.health(health, most, *player.pools)))])

    def _damage_player(self, player, damage, now, by=None, hits=1):
        """`by`: the entity that dealt it, which the death screen names (None: the player's own); `hits` equal hits
        (armour cuts each, target.armour_cut)."""
        if now < player.invulnerable_until or player.health <= 0 or player.phased:
            return
        until, extra = player.discorded
        damage *= 1 + extra if now < until else 1
        damage = armour_cut(damage, player.health, player.max_health, *player.pools, hits)
        if damage > 0 and player.hack_target is not None:
            self._finish_hack(player, now)
        mech = player.hero.body == SPAWN_BODY.get(player.hero.guid)
        player.health = max(1 if mech else 0, player.health - damage)  # D.Va's mech ejects her at 1 health
        self._send_player_health(player)
        if mech and player.health == 1:
            player.form_change = player.form_change or 'eject'
        elif not player.health:
            player.count('deaths')
            player.streak = 0
            self.update_staff(player, Command(0))
            self.update_grasp(player, Command(0))
            player.respawn_at = now + self.respawn_seconds()
            killer = player.body if by is None else by
            if player.client is not None:  # the death screen
                player.client.queue_reliable(25004, {'+0x78': KILLED_BY if killer != player.body else 0,
                                                     '+0x7C': {'+0x0': killer}})
            if player.body_vm is not None:  # the kill feed line
                try:
                    player.body_vm.died(killer)
                except Exception:
                    log.exception('[game] %s: %s\'s body VM failed', self.label(), player.name)
                    self.stop_body_vm(player)
        log.info('[game] player damage body=%08X amount=%d health=%d', player.body, damage, player.health)

    def add_player(self, account_lo: int, name: str, hero_guid: int, team: int, tournament: bool) -> Player:
        hero = heroes().get(hero_guid) or heroes()[SOLDIER]
        quanta = TOURNAMENT_QUANTA if tournament else RETAIL_QUANTA
        slot = max([p.slot for p in self.players] + [b.slot for b in self.bots if b.slot is not None] + [-1]) + 1
        player = Player(self, slot, account_lo, name, hero, team, quanta)
        self.players.append(player)
        return player

    def label(self) -> str:
        return f"match {self.id[0] & 0xFFFF:04X}"

    # --- messages ------------------------------------------------------------------------------

    def game_struct(self, viewer: Player) -> dict:
        """The 20300/20301 game struct: mode, participant names and teams. Component 74 indexes these names."""
        self._ensure_bots(viewer)
        participants = [(p.slot, p.entity, p.name, p.team) for p in self.players]
        participants += [(b.slot, b.controller, b.name, b.team) for b in self.bots if b.slot is not None]
        records = [{} for _ in range(max((slot for slot, _, _, _ in participants), default=-1) + 1)]
        for slot, _, name, _ in participants:
            records[slot] = {'+0x50': name}
        teams = []
        for team in (0, 1):
            slots = [{'+0x0': entity} for _, entity, _, member_team in participants if member_team == team]
            teams.append({"+0x18": slots, "+0x60": team, "+0x61": team})
        return {
            "+0x0": {
                "+0x0": {
                    "+0x0": {"+0x0": records},
                    "+0xB0": self.game_map.mode_guid,
                    "+0xC0": teams,
                    "+0xD8": [{"+0x0": viewer.entity}],
                },
                "+0x274": 16,
            }
        }

    def instance_message(self, viewer: Player) -> dict:
        """20300: load this map with this mode. +0x500 must equal the client's command frame length
        (0x7FF7896E94B0), else error 262445; +0x519 must match a byte that is 0 (error 262455)."""
        return {
            "+0x78": 1,
            "+0x80": self.game_struct(viewer),
            "+0x4A8": self.game_map.map_guid,
            "+0x500": viewer.quanta,
            "+0x508": {"+0x0": 1, "+0x8": 1},
        }

    # --- entities ------------------------------------------------------------------------------

    def placeable_ids(self) -> list[int]:
        return [0x80000000 | index for index in self.game_map.placeables]

    def spawn_updates(self, viewer: Player) -> list[EntityUpdate]:
        """The frame that ends the loading screen: the placeables, the viewer's player (and body, if it
        has one), the game mode entity, and the bodies of the players already in the world."""
        updates = [
            EntityUpdate(entity, OP_CREATE, lambda origin, index=index: world.create_placeable(origin, index))
            for entity, index in zip(self.placeable_ids(), self.game_map.placeables, strict=True)
        ]
        if viewer.has_body:
            updates += self.own_body_updates(viewer)
        updates.append(
            EntityUpdate(viewer.entity, OP_CREATE, lambda origin: self._player_create(origin, viewer))
        )
        updates.append(EntityUpdate(GAME_MODE_ENTITY, OP_CREATE, self._game_mode_create))
        for other in self.players:
            if other is not viewer and self._shown(other):
                updates.append(self.remote_body_create(viewer, other))
        return updates

    @staticmethod
    def _shown(player: Player) -> bool:
        """Whether other players' clients get this player's body."""
        return player.spawned and player.has_body and player.client is not None

    def own_body_updates(self, viewer: Player) -> list[EntityUpdate]:
        """The viewer's own body: its movement state rides in the create (at frame 0xFFFFFFFE) and in a
        movement record at the packet frame, as in ProCore's working spawn. shortcut: 0xFFFFFFFE is newer than any later
        record, so a body she leaves (D.Va's Self-Destruct mech) shows where it was made; stamped at the command frame
        instead, the Range dummy took no cannon hits (2026-11-08). Fix before showing left bodies elsewhere."""
        state = viewer.movement()
        viewer.body_create = EntityUpdate(
            viewer.body, OP_CREATE, lambda origin: self._body_create(origin, viewer, state), state
        )
        self.start_body_vm(viewer)
        return [viewer.body_create]

    def start_body_vm(self, player: Player) -> None:
        """The VM runs the new body's statescript (body_vm.wanted); its full frame waits for the create."""
        self.stop_body_vm(player)
        if not body_vm.wanted(player.hero.guid):
            return
        try:
            player.body_vm = body_vm.BodyVM(self, player, player.body_create)
        except Exception:
            log.exception('[game] %s: no body VM for %s; kits.py frames instead', self.label(), player.describe())

    @staticmethod
    def stop_body_vm(player: Player) -> None:
        if player.body_vm is not None:
            player.body_vm.retire()
            player.body_vm = None
            player.mover.script_mods = None

    def body_vm_command(self, player: Player, command: Command) -> None:
        """A failure turns the VM off for this body (logged); kits.py's frames take over."""
        try:
            player.body_vm.command(self, player, command)
        except Exception:
            log.exception('[game] %s: %s\'s body VM failed', self.label(), player.name)
            self.stop_body_vm(player)

    def _body_vm_frames(self, player: Player) -> None:
        try:
            player.client.queue_entities(player.body_vm.frames())
        except Exception:
            log.exception('[game] %s: %s\'s body VM failed', self.label(), player.name)
            self.stop_body_vm(player)

    def _body_create(self, origin: int, owner: Player, state: Movement | None) -> world.RecordWriter:
        components = world.health(owner.health, owner.max_health, *owner.pools)
        components[26] = [owner.team_bit]
        components[123] = [owner.hero.guid]
        # Authoritative Helix visuals use fresh IDs, so no client projectile reservation is needed.
        return world.create(
            origin,
            owner.hero.body,
            components,
            flags=BODY_FLAGS,
            position=owner.mover.position,
            rotation=_rotation(owner.mover.yaw),
            movement=state,
            skin=owner.skin,
        )

    def _player_create(self, origin: int, player: Player) -> world.RecordWriter:
        components = {
            26: [PLAYER_FILTER | player.team_bit],
            74: [None, None, player.slot, None, None, None, 1, 1, None],
        }
        if player.has_body:
            components.update(self._possession(player))
        components[29] = [None, player.name]  # m_battleTag: his name in the chat's hero switch line
        player.board_sent = components[board.GMP_STATS] = self._board(player)
        return world.create(origin, PLAYER_DEFINITION, components, flags=PLAYER_FLAGS)

    def _board(self, player: Player) -> list:
        return board.component(player.career, self.game_map.mode_guid, player.hero.guid if player.has_body else None,
                               player.fire)

    def _send_board(self, player: Player, now: float) -> None:
        """The Tab board's numbers again when they changed, at most every BOARD_EVERY seconds; none before his
        entity's create carried the first."""
        if now < player.next_board or player.board_sent is None:
            return
        values = self._board(player)
        if values != player.board_sent:
            player.board_sent, player.next_board = values, now + BOARD_EVERY
            player.client.queue_entities([EntityUpdate(
                player.entity, OP_UPDATE, lambda origin: world.update(origin, {board.GMP_STATS: values}))])

    @staticmethod
    def _possession(player: Player) -> dict:
        """Components 44 (the body the player possesses, views and controls) and 75 (its hero, skin
        theme and the last valid hero)."""
        theme = player.skin[0] or None
        return {44: [player.body] * 3, 75: [player.hero.guid, theme, None, player.hero.guid]}

    def _gate_frame(self, variables, player, escort=False) -> BitWriter:
        """The mode entity's frame for a scripted-loading mode: its graph with the loader's marker state on, and
        the mission HUD once the client has the payload stand-in (the widget reads its health when it opens)."""
        graph, slot = LOADING_GATES[self.game_map.mode_guid]
        defended = self._defended(player)
        if defended is not None:
            variables = {**variables, DEFENDED: Entity(defended.entity)}
        instances = [Instance(1, graph, active={slot: None, GATE_BOARD[graph.index]: message_state()}),  # the marker takes no data
                     *pve_instances(self.game_map.mode_guid, defended is not None, escort)]
        return remote_full_frame(instances, variables)

    def _defended(self, player):
        """The PvE payload stand-in the client already has, if it lives: what the objective widget reads health from."""
        return next((bot for bot in self.bots if bot.behaviour == PAYLOAD and bot.health
                     and bot.entity in player.bots_seen), None)

    def _game_mode_create(self, origin: int) -> world.RecordWriter:
        return world.create(origin, GAME_MODE_DEFINITION, {114: [None, self.game_map.mode_guid]})

    def remote_body_create(self, viewer: Player, other: Player) -> EntityUpdate:
        """Another player's body: placed by its transform, then moved by movement records from the next
        frame on (a movement state in the create would sit at frame 0xFFFFFFFE, and the client refuses
        every later record that is not newer)."""
        viewer.seen[other.body] = False
        return EntityUpdate(other.body, OP_CREATE, lambda origin: self._body_create(origin, other, None))

    def _bot_movement(self, bot, viewer, frame):
        """A bot's record. Its 3P walk takes the throttles against its yaw and its speed from the velocity; with no keys
        held, +36 names the frame its keys were last held in and +34/+35 their throttles (LizRollin's bots.Bot.movement:
        that frame goes as the record's own when it is not before it, or the client reads the rest out of step)."""
        if bot.held != (0, 0) or (bot.held_at is not None and (frame is None or viewer.frame_at(bot.held_at) >= frame)):
            input_frame = None
        else:
            input_frame = NO_INPUT if bot.held_at is None else viewer.frame_at(bot.held_at)
        return Movement(bot.position, _yaw_units(bot.yaw), _yaw_units(bot.pitch), bot.velocity, throttles=bot.held,
                        input_frame=input_frame, input_throttles=bot.last_held, gravity=GRAVITY_SCALE)

    def remote_movements(self, viewer: Player, frame: int | None = None) -> list[EntityUpdate]:
        """A movement record for every other player's body in every frame, standing or not, as the
        retail server sent them (OW2's live traffic); the first in the frame after the body's create. With the
        packet frame, also the viewer's own body's state, which its client checks its prediction against
        (owner_correction)."""
        updates = []
        for other in self.players:
            if other is viewer or other.body not in viewer.seen:
                continue
            if not viewer.seen[other.body]:
                viewer.seen[other.body] = True
                continue
            updates.append(EntityUpdate(other.body, movement=other.movement()))
        for bot in self.bots:
            if bot.health and bot.entity in viewer.bots_seen:
                if viewer.seen.get(bot.entity):
                    updates.append(EntityUpdate(bot.entity, movement=self._bot_movement(bot, viewer, frame)))
                else:
                    viewer.seen[bot.entity] = True
        drop = viewer.entity | DROP_SLOT
        if viewer.mech_drop is not None and drop in viewer.seen:  # her called mech, falling faster as it comes
            if viewer.seen[drop]:
                (x, y, z), yaw = viewer.mech_drop
                gone = 1.0 - min(1.0, max(0.0, viewer.mech_call_at - self.now) / CALL_MECH_CAST)
                updates.append(EntityUpdate(drop, movement=Movement((x, y + CALL_MECH_DROP * (1 - gone * gone), z), yaw,
                    velocity=(0.0, -2 * CALL_MECH_DROP * gone / CALL_MECH_CAST, 0.0))))
            viewer.seen[drop] = True
        own = owner_correction.own_record(viewer, frame) if frame is not None else None
        return updates + self._projectile_updates(viewer) + ([own] if own is not None else [])

    def destroy_for_others(self, player: Player, body: int) -> None:
        for other in self.players:
            if other is not player and body in other.seen:
                del other.seen[body]
                if other.client is not None:
                    other.client.queue_entities([EntityUpdate(body, OP_DESTROY)])

    # --- lifetime ------------------------------------------------------------------------------

    def _body_weapon_frame(self, player, cmfd, update, *, refresh=False):
        instances, variables, weapon_states = (player.body_extra_instances, player.body_extra_variables,
                                                player.weapon_extra_states)
        if self.body_setup is not None and player.hero.guid == BASTION:
            instances, variables, weapon_states = bastion_body(player.bastion_phase, player.bastion_repair_phase,
                                                               instances, variables, weapon_states)
        args = (cmfd, update, weapon_states, player.weapon_entity)
        body = dict(extra_instances=instances, extra_variables=variables)
        # The client takes the frame's MovementMod states from this frame on; so does our copy. Soldier: 76's Sprint
        # keeps its own path (mover.speed_scale, which the recordings replay).
        player.mover.ability_mods = {} if player.sprint is not None else frame_mods(instances)
        player.mover.swapped, player.mover.aimed = frame_flight(instances)
        player.mover.lift = next((c.spec.lifts for c in player.cooldowns
                                  if getattr(c, 'active', False) and getattr(getattr(c, 'spec', None), 'lifts', None)), None)
        if player.moving:
            # An ability's movement starts with the frame that carries its states: a roll begun a step before its
            # mods lost 1.2 m/s to the ground on the way (live), and the client's copy of the mods starts here too.
            (move, *numbers), command = player.moving
            player.moving = None
            getattr(player.mover, move)(command, *numbers)
        player.phased = phased(instances)
        if player.sprint is not None:
            firing = update is not None and update.active
            if firing and not player.primary_firing:
                player.primary_started = cmfd
            player.primary_firing = firing
            if player.primary_started is not None:
                body['fire_elapsed'] = min(100000, max(0, cmfd - player.primary_started) * player.quanta // 1000)
        if self.body_setup is not None and player.hero.guid in SCOPE_HEROES:
            body['scoped'] = bool(player.last_input[2] & SECONDARY)
            if player.hero.guid == WIDOWMAKER:
                body['scope_charge'] = player.widow_charge
        if player.grasp_target is not None:
            # Moira's locked beam: switch 22 needs var38 and var2318 (the entity TrackRay 21 hits, var14517, through
            # filters) and var375 (TrackTargets' target); the client finds none of them itself (research/asi-tracer.md)
            body['weapon_values'] = {v: Entity(player.grasp_target) for v in (375, 38, 2318, 14517)}
        if player.hero.guid == MERCY and player.rifle is not player.mercy_blaster_rifle:
            # instance sync vars: heal 689 / boost 690 (Stacks 71 / 72; DataFlowMappings 22 / 23 feed the staff's head)
            beam = player.staff_target is not None
            body['weapon_values'] = {689: Bool(beam and not player.staff_boost), 690: Bool(beam and player.staff_boost)}
            if beam:  # kits.mercy_staff_states: the mode and the target
                body['weapon_values'].update({465: Asset(MERCY_BOOST if player.staff_boost else MERCY_HEAL),
                                              468: Entity(player.staff_target)})
        if player.hero.body == SPAWN_BODY.get(DVA):
            body['weapon_values'] = {261: Float(player.barrel_spin(self.now))}
        elif player.hero.guid == DVA:  # her pilot's release timer runs from the frame that ends a burst
            firing = update is not None and update.active
            if player.primary_firing and not firing:
                player.burst_end = cmfd
            player.primary_firing = firing
            if player.burst_end is not None:
                body['fire_elapsed'] = min(100000, max(0, cmfd - player.burst_end) * player.quanta // 1000)
        if player.hack_target is not None:
            body['weapon_values'] = {8909: Entity(player.hack_target)}  # the tether beams' far end (kits.SOMBRA_HACK_STATES)
        if player.hero.guid == ZENYATTA and player.alt is not None:
            # Volley 18 fires var1945 orbs (the release chain computes it from var1392 client-side, after the volley
            # the server starts has begun); unsent, it ended at once and no orb drew
            body['weapon_values'] = {1392: Int(player.alt.weapon.pellets), 1945: Int(player.alt.weapon.pellets)}
        if player.sprint is not None or player.hero.guid == DVA or (update is not None and update.reloading):
            # Soldier's (and D.Va's) unchanged animation states must survive weapon and HUD updates: D.Va's Boosters
            # ring needs var215 resent through the flight, which full frames did by replaying the flight's anims.
            steady = player.sprint is not None or player.hero.guid == DVA
            serialize = partial(player.body_script.reload_frame, refresh=refresh or steady)
            return player.kit_frame(*args, **body, serialize=serialize)
        return player.body_script.full_frame(player.kit_frame(*args, **body))

    def update(self, now: float, tick: int) -> None:
        """Run the spawn steps of players who loaded, keep their statescript acked, and show new bodies
        to the others."""
        if self.now and not self.round_finished:
            for player in self.players:
                if player.client is not None and player.has_body:
                    player.count('time', now - self.now)  # Time Played on the hero, dead or alive
        self.tick, self.now = tick, now
        if not self.round_finished:
            self._land_projectiles(now)
            self._land_rockets(now)
            self._tick_shots(now)
            self._tick_fields(now)
            self._tick_orbs(now)
            self._tick_rockets(now)
            self._tick_salvos(now)
            self._tick_mech_bombs(now)
            for bot in self.bots:
                bot.patrol(now)
                event = bot.tick(now)
                if event is not None:
                    kind, entity = event
                    if kind == 'revive':
                        self._revive_bot(bot)
                    for viewer in self.players if kind == 'respawn' else ():
                        if viewer.client is not None and entity in viewer.bots_seen:
                            viewer.client.queue_entities([EntityUpdate(entity, OP_DESTROY)])
                            viewer.bots_seen.discard(entity)
                            viewer.seen.pop(entity, None)
                    log.info('[game] training target %s entity=%08X', kind, entity)
        if self.assemble_ends is not None and not self.assembled and now >= self.assemble_ends:
            self._end_assemble(now)
        self._tick_assault(now)
        if not self.round_finished:
            self._tick_viewer(now)
            self._steer_bots(now)
            self._bots_fight(now)
            self._bots_heal(now)
            for bot in self.bots:  # the poses shots are tested against at the shooter's view time
                bot.remember(now)
        self._send_bot_weapons()
        self._send_range_bot_frames(now)
        for player in self.players:
            client = player.client
            if client is None:
                continue
            player.fire.tick(now)
            self._send_board(player, now)
            self._send_switch_notice(player, now)
            if not self.round_finished and player.form_change is not None and player.has_body:
                self.change_form(player, now)
            if (player.ult_instance is not None and player.ult_instance.active.get(27, 0) is None
                    and now >= player.invulnerable_until and player.body_script is not None):
                player.ult_instance = DVA_ULTS[player.hero.body][0]  # the eject animation is over
                player.refresh_abilities()
                player.body_dirty = True
            if player.mech_call_at and now >= player.mech_call_at:
                if not self.round_finished and player.has_body and player.health > 0:
                    self.land_mech(player, now)
                player.mech_call_at = 0.0  # a pilot killed while calling stays on foot
                self.end_mech_drop(player)
            if not self.round_finished and player.respawn_at is not None and now >= player.respawn_at:
                self.switch_hero(player, heroes().get(player.hero.guid, player.hero))  # D.Va's pilot: back in a mech
                log.info('[game] player respawn body=%08X', player.body)
            self._bot_attack(player, now)
            if (player.bastion_switch_at and now >= player.bastion_switch_at and player.health > 0 and not self.round_finished
                    and not player.body_dirty and not player.body_script.data_in_flight):
                player.bastion_phase = 2 if player.bastion_phase == 1 else 0
                player.bastion_switch_at = 0.0
                player.body_dirty = True
                log.info('[game] Bastion settled in %s', 'Sentry' if player.bastion_phase else 'Recon')
            if (player.hero.guid == BASTION and player.body_script is not None and self.body_setup is not None
                    and self.gameplay_enabled and not self.round_finished):
                player.update_bastion_repair(now)
            if (player.hero.guid == WIDOWMAKER and player.body_script is not None and self.body_setup is not None
                    and self.gameplay_enabled and not self.round_finished):
                player.update_widow_charge(now)
            for timed in (c for c in player.cooldowns if isinstance(c, Timed) and c.active) if player.body_script is not None else ():
                if timed.healing and now >= timed.heal_at and 0 < player.health < player.max_health:
                    timed.heal_at = now + HEAL_EVERY  # Cryo-Freeze
                    player.health = min(player.max_health, player.health + timed.healing * HEAL_EVERY)
                    self._send_player_health(player)
                if timed.live:
                    player.body_dirty = True  # the client does not run var215 itself: Boosters' ring reads each resend
                elif timed.due():  # one of its timed states came on or went off (kits.Ability.running)
                    player.refresh_abilities()
                    player.body_dirty = True
            if player.body_script is not None and 0.0 < player.barrel_spin(now) < 1.0:
                player.body_dirty = True  # the cannons spin up or down
            # The HUD reads var61 (seconds left) only when a body frame arrives: resend while a cooldown runs.
            if now >= player.next_cooldown_frame and (
                    (player.helix is not None and (player.helix.showing_cooldown() or player.field.showing_cooldown()))
                    or any(cooldown.showing_cooldown() for cooldown in player.cooldowns)):
                player.next_cooldown_frame = now + COOLDOWN_REFRESH
                if player.cooldowns and not player.cast_hold and not any(cooldown.updates for cooldown in player.cooldowns):
                    player.refresh_abilities()  # charges that came back; a cast pulse keeps its states
                player.body_dirty = True
            if player.cast_hold and now >= player.cast_hold:
                player.cast_hold = 0.0
                player.refresh_abilities()
                player.body_dirty = True
            if player.field_release_at and now >= player.field_release_at:
                player.field_release_at = 0.0
                if player.health > 0:
                    self.throw_field(player)
            if player.visor_cast_until and now >= player.visor_cast_until:
                player.visor_cast_until = 0.0
                player.refresh_abilities()
                player.body_dirty = True
            if player.visor_until and now >= player.visor_until:
                player.visor_until = 0.0
                player.refresh_abilities()
                player.body_dirty = True
                log.info('[game] tactical visor ends')
            if player.loaded_at is not None:
                self._spawn_steps(player, client, now)
            if (player.pending_spawn_voice is not None and player.health > 0
                    and (player.body_script is not None and not player.body_script.data_in_flight
                         or player.body_vm is not None and player.body_vm.script.stream.base is not None)):
                client.queue_reliable(27500, player.pending_spawn_voice)
                player.pending_spawn_voice = None
            if player.body_vm is not None and player.spawned:
                self._body_vm_frames(player)
            elif (player.body_script is None and player.has_body and player.spawned
                    and player.hero.guid in KITS and player.body_picked_at is not None
                    and now - player.body_picked_at >= SOLDIER_DRAW_DELAY):
                stream = Stream(player.body)
                cmfd = stream.next_cmfd(self._newest_frame(player))
                player.body_script = stream
                updates = []
                if self.body_setup is not None:
                    self.body_setup(player)
                    if ULT_CHARGE in player.body_extra_variables:
                        player.body_extra_variables[ULT_CHARGE] = ult_meter(player.ult_charge, player.ult_most)
                    updates.append(EntityUpdate(player.entity, OP_UPDATE,
                        lambda origin: world.update(origin, {85: [1]})))
                updates.append(self._body_weapon_frame(player, cmfd, None))
                client.queue_entities([*updates, *self._target_updates(player)])
            elif (player.body_script is not None and player.rifle.updates
                    and not player.body_script.data_in_flight and now >= player.next_rifle_update):
                update = player.rifle.updates.popleft()
                player.last_rifle_update, player.body_dirty = update, False
                cmfd = player.body_script.next_cmfd(self._newest_frame(player))
                client.queue_entities([self._body_weapon_frame(player, cmfd, update)])
                # Diagnostic hold: allow a client update before replacing Start with Stop.
                player.next_rifle_update = now + WEAPON_PULSE_SECONDS if update.active else now
            elif (player.body_script is not None
                    and ((player.helix is not None and (player.helix.updates or player.field.updates))
                         or (player.alt is not None and player.alt.updates)
                         or any(cooldown.updates for cooldown in player.cooldowns))
                    and not player.body_script.data_in_flight and now >= player.next_rifle_update
                    and not any(r.player is player and r.begin_frame and
                        (not r.records or not all(record.delivered for record in r.records)) for r in self.helix_rockets)):
                cast = False  # Biotic Field's throw, held like kits.ABILITIES casts
                helix_begin = False
                if player.helix is not None and player.helix.updates:
                    update = player.helix.updates.popleft()
                    player.weapon_extra_states.update(helix_states(update, player.helix))
                    helix_begin = update.active
                elif player.alt is not None and player.alt.updates:
                    update = player.alt.updates.popleft()
                    player.weapon_extra_states.update(alt_states(WEAPON_KITS[player.hero.guid], update))
                elif player.helix is not None and player.field.updates:
                    update = player.field.updates.popleft()
                    player.refresh_abilities(update)
                    cast = True  # shortcut: a Sprint change inside the 1 s hold drops the throw states early
                else:  # kits.ABILITIES: the cast's start, then its stop (abilities with no cast resend the HUD)
                    cooldown = next(cooldown for cooldown in player.cooldowns if cooldown.updates)
                    update = cooldown.updates.popleft()
                    player.refresh_abilities((cooldown, update))
                player.body_dirty = False
                cmfd = player.body_script.next_cmfd(self._newest_frame(player))
                client.queue_entities([self._body_weapon_frame(player, cmfd, player.last_rifle_update, refresh=True)])
                if helix_begin:
                    for rocket in self.helix_rockets:
                        if rocket.player is player and rocket.serial == update.serial and not rocket.begin_frame:
                            rocket.begin_frame = player.body_script.data_last
                player.next_rifle_update = now + WEAPON_PULSE_SECONDS if update.active else now
                if (cast or player.helix is None) and not update.active:  # the cast is out: its states stay on for CAST_HOLD
                    player.cast_hold = now + CAST_HOLD  # (dropped at once, Symmetra stayed in turret placement)
            elif (player.body_script is not None and player.body_dirty and not player.rifle.updates
                    and not player.body_script.data_in_flight):
                player.body_dirty = False
                cmfd = player.body_script.next_cmfd(self._newest_frame(player))
                client.queue_entities([self._body_weapon_frame(player, cmfd, player.last_rifle_update, refresh=True)])
            if player.script.data_last and now >= player.script.next_ack:
                self._owner_ack(player, player.script, now)
            if player.body_script is not None and now >= player.body_script.next_ack:
                self._owner_ack(player, player.body_script, now)
            if ((self.assembling() or self.game_map.mode_guid in LOADING_GATES  # PvE: the gate frame carries the HUD
                    or self.game_map.mode_guid in OBJECTIVE_MODES and self.assembled)
                    and player.steps_done >= 4 and now >= player.next_countdown):
                self._send_countdown(player, now)
            if player.body_script is not None or player.body_vm is not None:
                updates = self._target_updates(player) + self._nameplate_updates(player)
                if updates:
                    client.queue_entities(updates)
        for viewer in self.players:
            if not viewer.spawned or viewer.client is None:
                continue
            for other in self.players:
                if other is not viewer and self._shown(other) and other.body not in viewer.seen:
                    viewer.client.queue_entities([self.remote_body_create(viewer, other)])
        hitbox_feed.publish(self)

    def _spawn_steps(self, player: Player, client, now: float) -> None:
        waited = now - player.loaded_at
        if player.steps_done == 0 and waited >= SEND_OWN_ENTITY:
            client.queue_reliable(20302, {"+0x78": {"+0x0": player.entity}, "+0x7C": False, "+0x80": 0})
            player.steps_done = 1
        if player.steps_done == 1 and waited >= SEND_GAME:
            client.queue_reliable(20301, {"+0x78": self.game_struct(player)})
            player.steps_done = 2
        if player.steps_done == 2 and waited >= SEND_WORLD:
            client.queue_entities(self.spawn_updates(player))
            player.steps_done = 3
            player.spawned = True
            log.info("[game] %s: %s spawned", self.label(), player.describe())
            if self.controller is heroselect.PVP and self.assemble_ends is None:
                self.assemble_ends = now + ASSEMBLE_SECONDS
                log.info("[game] %s: assemble your team, %d s", self.label(), ASSEMBLE_SECONDS)
        if player.steps_done == 3 and waited >= SEND_CONTROLLER:
            player.steps_done = 4
            self.send_controller(player)

    # --- assembling heroes ---------------------------------------------------------------------

    def assembling(self) -> bool:
        return self.assemble_ends is not None and not self.assembled

    def _send_countdown(self, player: Player, now: float) -> None:
        """The seconds left, as the game mode entity's v1303. The client shows them rounded up. In the
        objective modes graph 0F4A runs the phases after assembly: setup, attacking a point, round complete."""
        player.next_countdown = now + COUNTDOWN_EVERY
        if self.game_map.mode_guid in LOADING_GATES:  # PvE: after assembly v1303 is the DEFEND timer, then escort
            ends = self.assemble_ends + (0.0 if self.assembling() else DEFEND_SECONDS)
            frame = self._gate_frame({COUNTDOWN: Float(max(0.0, ends - now))}, player, escort=now >= ends)
        elif self.game_map.mode_guid not in OBJECTIVE_MODES:
            variables = {COUNTDOWN: Float(max(0.0, self.assemble_ends - now))}
            graph, slot = BOARD_MODES.get(self.game_map.mode_guid, (None, None))
            frame = (variables_frame(variables) if graph is None else
                     remote_full_frame([Instance(1, graph, active={slot: message_state()})], variables))
        else:
            setup_ends, game = self.assemble_ends + SETUP_SECONDS, self.assault
            instances, variables = [], {}
            if self.assembling():
                phase, left = PHASE_ASSEMBLE, self.assemble_ends - now
            elif game is None:  # setup; Escort and Hybrid have no rules yet and stay on point A
                phase, left = (PHASE_SETUP, setup_ends - now) if now < setup_ends else (PHASE_ATTACK, setup_ends + ROUND_SECONDS - now)
            elif game.winner is None:
                phase, left = PHASE_ATTACK, game.ends - now
                variables = {OBJECTIVE_INDEX: Int(game.index), OVERTIME: Int(int(game.overtime(now)))}
            else:
                phase, left = PHASE_ROUND_COMPLETE, game.ends - game.finished
                # shortcut: one round, so the defenders' score stays 0; the round number and time remaining
                # stay "0" and "0.0" (research/assault-mode-graph.md), until their vars are found.
                presence = {SCORE_LEFT: Int(game.index), SCORE_RIGHT: Int(0)}
                if game.winner:  # lost: how far the attackers got on the last point
                    presence.update({SHOW_PROGRESS: Int(1), PROGRESS: Float(game.progress)})
                instances = [Instance(3, ROUND_COMPLETE, parent=(1, ROUND_COMPLETE_STATE), presence=presence)]
            frame = remote_full_frame([Instance(1, OBJECTIVE_MODE, active={phase: message_state(),
                                                                           PHASE_BOARD[phase]: message_state()}),
                                       Instance(2, ANNOUNCER, parent=(1, ANNOUNCER_STATE)), *instances],
                                      {COUNTDOWN: Float(max(0.0, left)), **variables})
        updates = [player.mode_script.full_frame(frame)]
        if self.game_map.mode_guid in OBJECTIVE_MODES:
            for index in self.game_map.objective_placeables:
                stream = player.objective_scripts.setdefault(index, Stream(0x80000000 | index))
                updates.append(stream.full_frame(remote_full_frame([Instance(1, OBJECTIVE)], {})))
        player.client.queue_entities(updates)

    def _end_assemble(self, now: float) -> None:
        """The countdown is over: the screens of the players who have a hero close, and the skin
        selector goes."""
        self.assembled = True
        log.info("[game] %s: heroes assembled", self.label())
        for player in self.players:
            if player.client is None or player.steps_done < 4:
                continue
            self._send_countdown(player, now)
            if player.has_body:
                player.select_open = False
            self.send_controller(player)

    # --- the player's statescript --------------------------------------------------------------

    def _newest_frame(self, player: Player) -> int:
        """The newest command frame we have from the client, or our tick before the first."""
        return self.tick if player.commands.last_frame is None else player.commands.last_frame

    def _send_switch_notice(self, player: Player, now: float) -> None:
        """The waiting hero switch line, in one controller frame, once the client has every controller frame sent
        before. After a frame that carried one, a plain frame goes first: it clears the three variables (the old
        hero goes too, so that posts nothing)."""
        if player.switch_notice is None or player.script.data_in_flight or now < player.controller_sent + NOTICE_GAP:
            return
        if player.notice_frame:
            self.send_controller(player)
            return
        (old, new), player.switch_notice = player.switch_notice, None
        self.send_controller(player, {OLD_HERO_BIT: Asset(old), NEW_HERO_BIT: Asset(new),
                                      SWITCHER_BIT: Entity(player.entity)})

    def send_controller(self, player: Player, notice: dict | None = None) -> None:
        """A full frame of the controller's statescript, with hero select open or closed. The skin
        selector shows in the PvP modes while heroes are assembled, in the Practice Range whenever
        the screen is open. notice: the hero switch line's presence bits (_send_switch_notice)."""
        if player.client is None:
            return
        player.notice_frame, player.controller_sent = notice is not None, self.now
        cmfd = player.script.next_cmfd(self._newest_frame(player))
        practice = self.controller is heroselect.PRACTICE
        skins = player.select_open and (practice or self.assembling())
        frame = heroselect.controller_frame(self.controller, cmfd, player.select_open, skins, practice, notice)
        player.client.queue_entities([player.script.full_frame(frame)])
        if practice and not player.mode_script.data_last:
            player.client.queue_entities([player.mode_script.full_frame(remote_full_frame([
                Instance(1, PRACTICE_MODE, active={PRACTICE_BOARD: message_state()}),
                Instance(2, HUD_BOOTSTRAP, parent=(1, 21))], {}))])
        state = "open" if player.select_open else "closed"
        log.info("[game] %s: %s's controller sent (hero select %s)", self.label(), player.name, state)

    def _owner_ack(self, player: Player, stream: Stream, now: float) -> None:
        """A delta owner frame that only moves CmFD on, so the client replays fewer of its own commands.
        Only once the frames with data have arrived: a newer chunk whose range covers a queued one makes
        the client drop that one."""
        stream.next_ack = now + OWNER_ACK_EVERY
        if stream.data_in_flight or (player.commands.last_frame or 0) <= stream.cmfd:
            return
        cmfd = stream.next_cmfd(self._newest_frame(player))
        player.client.queue_entities([stream.ack_delta(owner_ack(cmfd))])

    def game_message(self, player: Player, value: dict, now: float) -> None:
        """21616: a game message a statescript action sends to the server, such as a hero pick."""
        if self.round_finished:
            return
        message = value.get("+0xA8", 0)
        parameters = bytes(value.get("+0x80") or b"")
        log.info(
            "[game] %s: %s sent game message %X (instance %s) %s",
            self.label(),
            player.name,
            message & 0xFFFFFFFF,
            value.get("+0xB0"),
            parameters.hex(" "),
        )
        if message == heroselect.PICK_HERO:
            hero = heroes().get(heroselect.picked_hero(parameters) or 0)
            if hero is None:
                log.warning("[game] %s: no hero found in the pick", self.label())
                return
            if self.gameplay_enabled and hero.guid not in KITS:
                # shortcut: only the KITS heroes have a served weapon; any other pick plays Soldier. Add a kit
                # per playable hero (research/non-soldier-hero-scope.md).
                log.info("[game] %s: %s is not playable yet; Soldier instead", self.label(), hero.name)
                hero = heroes()[SOLDIER]
            skin = self.skin_of(player.account_lo, hero.guid)
            old = player.hero.guid if player.has_body else None  # None: the first pick is not a switch
            if hero is not player.hero or skin != player.skin or not player.has_body:
                self.switch_hero(player, hero)
            if (old not in (None, hero.guid) and self.controller is heroselect.PVP
                    and self.game_map.mode_guid in SWITCH_MODES):
                player.switch_notice = (old, hero.guid)
            if self.assembling():
                return  # the screen stays open until the heroes are assembled, as in retail
            player.select_open = False
            self.send_controller(player)
        elif message == heroselect.OPEN_OR_CLOSE:
            wanted = heroselect.wanted_open(parameters)
            player.select_open = not player.select_open if wanted is None else wanted
            if not player.has_body:
                player.select_open = True  # no hero yet: nothing to go back to
            self.send_controller(player)

    def map_loaded(self, player: Player, now: float) -> None:
        if player.loaded_at is None:
            player.loaded_at = now
            log.info("[game] %s: %s loaded the map", self.label(), player.name)

    def switch_hero(self, player: Player, hero: Hero, at=None, keep_old=False, launch=None) -> None:
        """The body of a picked hero. The first one just comes. A switch creates the new body under a
        new id at the spawn point, moves the player's components 44 and 75 to it and destroys the old
        body in the same frame: retail drops the old one at once too (OW2 traffic), while ProCore's
        .sethero kept it 0.5 s, and it showed when the hero select screen closed.
        at: (feet, yaw units) for the new body instead of the spawn point; keep_old: leave the old body (D.Va's
        Self-Destruct mech) to the caller; launch: the new body's velocity (x, y, z) m/s, sent in its create."""
        old = player.body if player.has_body else None
        if old is not None:
            player.new_body()
            if at is not None:
                player.mover = PlayerMover(at[0], at[1], self.world, hero.body, player.team)
            if launch is not None:
                player.mover.launch(launch)
        if hero is not player.hero:
            player.ult_charge = 0
        player.hero = hero
        player.mover.use(body_values(hero.body))  # its mover values (capsule, speeds)
        player.skin = self.skin_of(player.account_lo, hero.guid)
        player.has_body = True
        player.health = player.max_health
        player.pending_spawn_voice = spawn_voice(hero.guid, player.body, player.respawn_at is not None)
        player.respawn_at = None
        player.body_script = None
        self.stop_body_vm(player)
        player.body_picked_at = time.time()
        player.rifle = Rifle(player.quanta, PLAYER_KIT.get(player.hero.name, (0, None))[1])
        player.mercy_blaster_rifle = None
        player.bastion_other_rifle = None
        player.bastion_phase, player.bastion_switch_at = 0, 0.0
        player.bastion_repair_phase, player.bastion_repair_at = 0, 0.0
        player.bastion_repair_charge, player.bastion_repair_tick = BASTION_REPAIR_CAPACITY, None
        player.bastion_heal_at = 0.0
        player.widow_charge_from, player.widow_charge_serial = None, 0
        player.widow_charge, player.widow_charge_next = 1.0, 0.0
        player.next_rifle_update = 0.0
        player.weapon_extra_states = {}
        player.hack_target = None
        player.hack_completes_at = player.hack_ready_at = 0.0
        player.staff_target = player.grasp_target = player.guardian = None
        player.staff_tick = player.grasp_tick = 0.0
        player.staff_boost = False
        player.weapon_entity = None
        player.body_extra_instances = []
        player.body_extra_variables = {}
        player.sprint, player.helix, player.field = None, None, None
        player.body_dirty, player.last_rifle_update, player.visor_until, player.visor_cast_until = False, None, 0.0, 0.0
        player.field_release_at = 0.0
        if player.client is not None and player.spawned:
            components = self._possession(player)
            updates = self.own_body_updates(player)
            updates.append(
                EntityUpdate(player.entity, OP_UPDATE, lambda origin: world.update(origin, components))
            )
            if old is not None and not keep_old:
                updates.append(EntityUpdate(old, OP_DESTROY))
            player.client.queue_entities(updates)
        if old is not None and not keep_old:
            self.destroy_for_others(player, old)
        theme, golden = player.skin
        look = f", skin theme {theme:X}{' + golden weapon' if golden else ''}" if theme else ""
        log.info("[game] %s: %s plays %s%s", self.label(), player.name, hero.name, look)

    def leave(self, player: Player) -> None:
        if player.client is None and not player.spawned:
            return
        self.update_staff(player, Command(0))
        self.update_grasp(player, Command(0))
        player.client = None
        player.spawned = False
        self.stop_body_vm(player)
        self.destroy_for_others(player, player.body)
        log.info("[game] %s: %s left", self.label(), player.name)
        if self.on_leave is not None:
            self.on_leave(player)
        if all(other.client is None for other in self.players):
            self.ended = True

    def snapshot(self) -> dict:
        return {
            "id": self.label(),
            "map": self.game_map.name,
            "started": self.started,
            "players": [
                {
                    "name": player.name,
                    "hero": player.hero.name,
                    "team": player.team + 1,
                    "state": "playing" if player.spawned else ("loading" if player.client else "waiting"),
                }
                for player in self.players
            ],
        }


def pong(ping: dict, received_ns: int) -> dict:
    """20306 for a 21610 ping: its index and stamps, plus when we got it and when we answer."""
    stamps = [*ping.get("+0x80", []), received_ns, time.time_ns()]
    return {"+0x78": ping.get("+0x78", 0), "+0x80": stamps}
