"""The Practice Range's 15 retail bots, placed from the 1.74 map data (research/range-bot-roster.md)."""
from collections import deque
from dataclasses import dataclass
import math
import random

import hitbox
import nav
from combat import aim_direction
from ow174.game.content import heroes
from ow174.game.mover import mover_data
from ow174.game.pools import body_pools
from ow174.game.statescript import Entity, Graph, Instance, pulser_state

BOT_HERO = 0x02E000000000016B
BOT_BODY = 0x0400000000001403
# A Range bot's reassembly after a death (research/bot-death.md): its body graph 0BA8's StateAnim, remote slot 1,
# plays alias 0x0C80000000000451, which the bot's blend-tree set resolves to a random one of clips 0x39D7 and 0x39D8
# (2.667 s at 60 fps: the parts fly in and lock into the idle pose). The client plays the death ragdoll by itself.
REBUILD = Graph(0x0BA8, 17, 4, 11)
REBUILD_SLOT, REBUILD_SECONDS = 1, 2.667
# A dead body's kill feed line (research/lizrollin-runtime.md section 16). A hero body's HUD graph 01CF takes the death
# (003E.025), keeps the killer in #392 and pulses state 108, a ClientOnlyPulser; per pulse the client posts 0765.025
# to the game mode entity's feed 0DEA. A Range bot's 0C18, a child of its body graph 0BE2, does the same with state 8.
# Each: (graph, the pulser's remote slot, #392's presence bit).
KILL_FEED = (Graph(0x0C18, 9, 7, 3), 1, 0)
HERO_KILL_FEED = (Graph(0x01CF, 41, 22, 20), 15, 15)
BODIES ={0x02E000000000016B: 0x0400000000001403, 0x02E000000000016C: 0x040000000000143E,
    0x02E000000000016D: 0x040000000000146E, 0x02E000000000016E: 0x04000000000014ED}
TARGET_HEALTH = 200
ATTACK_DAMAGE = 20
ATTACK_INTERVAL = (1.75, 2.5)  # graph B92's two random min/max config vars
ATTACK_RANGE = 30.0
# The shot: BA3's Volley spawns projectile entity 0x1476 at 35 m/s (config var 299); its child 0x148D is the
# effect the client draws. The child has no mover, so the server re-creates it along the path (bot-fire.md).
PROJECTILE = 0x040000000000148D
PROJECTILE_SPEED = 35.0
PROJECTILE_RADIUS = 0.1  # shortcut: the shot's own radius is not decoded; upgrade from graph BA3's Volley data
HERO_SPEED = 5.5  # the mover's RUN_SPEED
RIFLE_RANGE = 50.0  # how far hero bots look for enemies
INFINITE = 9999  # rounds of a weapon with no ammo (kits.py sends it no ammo vars)
REACTION_SECONDS = 0.5  # shortcut: a guessed delay before the first shot at a new sighting; no 1.74 AI data read
# Difficulty (wiki): the AI deals 25/50/100% damage and aims better when harder.
# shortcut: the aim error (degrees, one standard deviation) is a guess; upgrade if the AI's aim data is found.
DIFFICULTY = {'easy': (0.25, 2.5), 'medium': (0.5, 1.75), 'hard': (1.0, 1.0)}  # (damage scale, aim error)
# D.Va's Boosters hitting an enemy (wiki, OW1): 10 damage, knocked along the flight at 17 m/s.
# shortcut: contact is the mech's feet within BOOST_REACH of the bot's (horizontal, 2 m tall); the knock lasts
# KNOCK_SECONDS (not in the wiki), stopped by walls.
BOOST_DAMAGE, BOOST_KNOCK, BOOST_REACH, KNOCK_SECONDS = 10, 17.0, 1.5, 0.25
# Reinhardt's Charge: what a bot he runs into takes (the wiki's 50 for a bump; shortcut: no pin and its 300), and
# the speed under which a charge that has had time to get going counts as stopped by a wall.
CHARGE_BUMP, CHARGE_STOP_SPEED, CHARGE_RUN_UP = 50, 3.0, 0.3
STRIKE_DAMAGE = 50  # what a bot Genji's Swift Strike passes through takes (the wiki's number)
# D.Va's mech and pilot (wiki, OW1): Eject! leaves the pilot invulnerable for its 1.5 s cast; Self-Destruct's mech
# blows up 3 s after the press for 1000 damage at its centre down to 100 at 20 m, and cover blocks it; a called mech
# lands for 250 damage within 2.5 m.
EJECT_SECONDS, SELF_DESTRUCT_FUSE, SELF_DESTRUCT_RADIUS, SELF_DESTRUCT_DAMAGE = 1.5, 3.0, 20.0, (1000, 100)
# Eject: the pilot starts EJECT_BACK behind and EJECT_RISE above the mech's feet (inside it, her camera sat in the mech)
# and flies back at EJECT_LAUNCH (back, up) m/s: about 0.75 s in the air, landing some 5 m behind it, as in the Hero
# Info clip. shortcut: a ballistic arc from the body's create, not the pilot graph's own launch states.
EJECT_BACK, EJECT_RISE, EJECT_LAUNCH = 1.0, 1.0, (6.0, 5.0)
# Call Mech: her mech falls from CALL_MECH_DROP m up onto a spot CALL_MECH_AHEAD in front of her over the cast, then she
# boards it there (Hero Info clip: it lands right in front of her). shortcut: a plain body entity moved by the server,
# without the pilot graph's beams or cockpit-entry states.
CALL_MECH_DROP, CALL_MECH_AHEAD = 20.0, 1.5
CALL_MECH_DAMAGE, CALL_MECH_RADIUS = 250, 2.5
CALL_MECH_CAST = 1.5  # s from Q to her new mech: the Hero Info clip's call, landing and cockpit (about 1.5-2 s)
PATROL_SPEED = 2.5  # shortcut: a walking pace for Range shooters walking their route; patrols take their body's own
# How the B8E patrols walk, as LizRollin measured retail's Range bots (their bots.py): FIRST_WAIT before the first walk
# (B8E's two waits, DATA), keys held from stop to stop, WALK_GAP from letting go to the next walk, a TURN_RATE turn
# toward the way it walks until TURN_AFTER after letting go, no keys within ARRIVED of a stop.
FIRST_WAIT, WALK_GAP, TURN_RATE, TURN_AFTER, ARRIVED = 3.0, 1.2, 60.0, 0.5, 0.05
PATROL_TICK = 0.016  # s a patrol's step: the movers' tick
# Each body's first-person camera height above its feet, (standing, crouched): its STUFirstPersonComponent
# (tools/assets/eye_heights.py). Hitscan shots leave from it. shortcut: the stance switches at once, while the
# client's camera eases between the two.
EYE_HEIGHTS = {
    0x0400000000001424: (1.25, 0.875),  # Ana
    0x040000000000251C: (1.445, 1.0115),  # Ashe
    0x04000000000029A6: (1.35, 0.945),  # Baptiste
    0x04000000000001FD: (1.82, 1.35),  # Bastion
    0x0400000000001B1B: (1.58, 1.2),  # Brigitte
    0x04000000000002A3: (1.425, 0.9975),  # Cassidy
    0x0400000000000407: (1.74, 1.74),  # D.Va
    0x040000000000040A: (1.32, 0.8),  # D.Va pilot
    0x0400000000001235: (1.5, 1.2),  # Doomfist
    0x0400000000002593: (1.7, 1.1),  # Echo
    0x0400000000000225: (1.2, 0.84),  # Genji
    0x0400000000000141: (1.35, 0.945),  # Hanzo
    0x0400000000000380: (1.25, 0.875),  # Junkrat
    0x04000000000003FC: (1.175, 0.8225),  # Lúcio
    0x04000000000008BA: (1.3, 0.91),  # Mei
    0x04000000000000E4: (1.45, 1.015),  # Mercy
    0x0400000000001C46: (1.7, 1.1),  # Moira
    0x0400000000001498: (1.78, 1.246),  # Orisa
    0x0400000000000032: (1.35, 0.945),  # Pharah
    0x0400000000000022: (1.55, 1.085),  # Reaper
    0x0400000000000024: (1.8, 1.26),  # Reinhardt
    0x040000000000028F: (1.6, 1.12),  # Roadhog
    0x0400000000002AB1: (2.1, 1.75),  # Sigma
    0x04000000000003CF: (1.55, 1.085),  # Soldier: 76
    0x040000000000121C: (1.3, 0.91),  # Sombra
    0x04000000000026F0: (1.3, 0.91),  # Symmetra
    0x04000000000028B6: (1.0, 0.7),  # Torbjörn
    0x0400000000000001: (1.25, 0.875),  # Tracer
    0x040000000000001C: (1.5, 1.05),  # Widowmaker
    0x04000000000001C0: (1.5, 1.05),  # Winston
    0x040000000000200B: (1.83, 1.5),  # Wrecking Ball
    0x04000000000003BF: (1.6, 1.12),  # Zarya
    0x0400000000000216: (1.55, 1.15),  # Zenyatta
}
EYE_DEFAULT = (1.55, 1.15)  # the component's own defaults (OWLib), for a body not listed
POSE_HISTORY = 64  # bot poses kept for shots at the shooter's view time: 64 match ticks, about 1 s


def eye_height(body, crouched=False):
    return EYE_HEIGHTS.get(body, EYE_DEFAULT)[bool(crouched)]


@dataclass(frozen=True)
class Weapon:
    """A hero bot's primary fire: `damage` per pellet up to falloff[0] metres, falling linearly to `far_damage`
    at falloff[1] (None: no falloff); a shot every `shot_seconds` from a `rounds` magazine, `reload_seconds` to
    refill; bots fire within `reach`, and each of the `pellets` leaves within `spread` degrees of the aim. A
    projectile weapon's shots fly at `speed` m/s (None: hitscan)."""
    damage: float
    far_damage: float
    falloff: tuple | None
    shot_seconds: float
    rounds: int
    reload_seconds: float
    reach: float = RIFLE_RANGE
    pellets: int = 1
    spread: float = 0.0
    speed: float | None = None
    refill: float = 1.0  # the player's magazine refills this far into the reload (the weapon graph's var6884)
    reload_phases: tuple[float, float, float] | None = None  # open, insert one round, close
    recharge: float | None = None  # the player's passive refill: a round every this many seconds, no reload
    crit: float = 2.0  # the headshot multiplier; 1 where the weapon cannot headshot (the wiki's flag, and the 1.74
    # data's m_A872C70E where a Volley sets it: research/damage-data)

    def damage_at(self, distance):
        if self.falloff is None:
            return self.damage
        near, far = self.falloff
        f = min(1.0, max(0.0, (distance - near) / (far - near)))
        return self.damage * (1 - f) + self.far_damage * f


SOLDIER_RIFLE = Weapon(20, 6, (30, 50), 1 / 9, 30, 1.5, refill=0.489)  # the player's rate (soldier.Rifle); the 2022 wiki says 10/s
MERCY_BLASTER_WEAPON = Weapon(20, 20, None, .2, 20, 1.4, speed=50)
BASTION_SENTRY_WEAPON = Weapon(15, 4.5, (30, 50), 1 / 30, 300, 2.0, refill=.773, crit=1)  # Volley 013A.11: 30/s
# Practice vs AI heroes (wiki, 2020-07 list): name -> (health, primary fire), from each hero's wiki page as of
# 2022-09-30, the last Overwatch 1 revisions (research/wiki/roster/). Health counts armor and shields.
# Projectiles fly straight at their wiki speed (pspeed), with no drop and no size.
# shortcut: arcs and beams land at once like hitscan; Lucio's burst is spread evenly; spread is read
# as the cone's full angle; Zarya's charge, healing, slows and abilities are left out, and armor and shields are
# plain health. Upgrade a hero from its weapon graph.
# The weapon graphs (pinned catalog) give the player's reload: var581 (seconds) and var6884 (the refill point,
# `refill`); automatic fire rates follow the WeaponVolley's m_numShotsPerSecond (Mei 20, Wrecking Ball 25, D.Va 7).
HERO_KIT = {
    'Roadhog': (600, Weapon(6.6, 1.98, (15, 30), 0.848, 5, 2.0, pellets=25, spread=10.05, speed=80, refill=0.517)),
    'Zarya': (400, Weapon(3.75, 3.75, None, 0.05, 100, 1.5, reach=15, refill=0.711, crit=1)),
    'Bastion': (300, Weapon(20, 6, (25, 45), 0.125, 35, 1.5, refill=0.489)),
    'Cassidy': (225, Weapon(70, 21, (20, 40), 0.496, 6, 1.5, refill=0.53333)),
    'Mei': (250, Weapon(2.75, 2.75, None, 0.05, 120, 1.5, reach=10, speed=40, refill=0.6, crit=1)),  # 05E6.13: 40 m/s
    'Reaper': (250, Weapon(6, 1.8, (10, 20), 0.496, 8, 1.5, pellets=20, spread=3.0, refill=0.15)),
    'Soldier: 76': (200, SOLDIER_RIFLE),
    'Sombra': (200, Weapon(8, 2.4, (15, 35), 0.05, 60, 1.4, refill=0.4)),  # Volley 0B1B.6: 20/s
    'Torbjörn': (250, Weapon(70, 70, None, 0.608, 18, 2.0, speed=70, refill=0.5)),
    'Ana': (200, Weapon(70, 70, None, 0.8, 12, 1.5, speed=125, refill=0.4, crit=1)),
    'Lúcio': (200, Weapon(20, 20, None, 0.23, 20, 1.5, speed=50, refill=0.733)),
    'Zenyatta': (200, Weapon(48, 48, None, 0.4, 20, 1.5, speed=90, refill=0.8)),
}
# Heroes the player can play but no bot does: same sources and shortcuts (research/wiki/roster/, revisions
# before 2022-10-01). Splash damage counts as a direct hit; Baptiste's 3-round burst is three pellets; Ashe's and
# Widowmaker's hip fire; Mercy's Caduceus Staff.
PLAYER_KIT = {**HERO_KIT,
    'Tracer': (150, Weapon(6, 1.8, (12, 20), 0.05, 40, 1.0, pellets=2, spread=3.6, refill=0.7)),
    'Pharah': (200, Weapon(120, 120, None, 0.848, 6, 1.5, speed=35, refill=0.4, crit=1)),
    'Widowmaker': (175, Weapon(13, 3.9, (20, 40), 0.1, 35, 1.5, refill=0.55)),
    'Junkrat': (200, Weapon(120, 120, None, 0.64, 5, 1.5, speed=25, refill=0.578, crit=1)),
    'Orisa': (450, Weapon(11, 11, None, 1 / 12, 150, 2.5, spread=1.2, speed=120, refill=0.711)),
    'Wrecking Ball': (600, Weapon(5, 1.5, (15, 25), 0.04, 80, 2.0, spread=2.0, refill=0.5)),
    'Ashe': (200, Weapon(40, 12, (20, 40), 0.256, 12, 3.55, spread=1.85, reload_phases=(.5, .25, .3))),
    'Echo': (200, Weapon(17, 17, None, 0.3, 12, 1.5, pellets=3, speed=75, refill=0.711)),
    'Baptiste': (200, Weapon(24, 7.2, (25, 45), 0.58, 45, 1.5, pellets=3, refill=0.6)),
    'Mercy': (200, Weapon(0, 0, None, 0.1, INFINITE, 1.5, reach=15)),  # her staff (kits.MERCY) heals, hurts nothing
    # shortcut: Hanzo's arrows are all full charge; Genji's 3-shuriken burst is spread evenly over its 0.88 s;
    # Sigma's two orbs are two pellets; Symmetra's beam stays at its first damage tier; Winston's Tesla hits one
    # target; Moira's primary heals and so hurts nothing; Doomfist bots reload in one go (the
    # player recharges a round per graph 0x0D0A's 0.65 s Wait).
    'Hanzo': (200, Weapon(125, 125, None, 0.5, INFINITE, 1.5, speed=110)),
    # her mech (kits.DVA): 300 health + 300 armor (body 0x0407's schema), Fusion Cannons; out of it, the pilot
    # (content.pilot, kits.DVA_PILOT): 150 health, Light Gun.
    'D.Va': (600, Weapon(2, 0.6, (10, 20), 0.144, INFINITE, 1.5, pellets=11, spread=4.15)),
    'D.Va pilot': (150, Weapon(14, 14, None, 1 / 7, 20, 1.4, speed=50, refill=0.65)),
    'Sigma': (400, Weapon(55, 55, None, 1.48, INFINITE, 1.3, reach=22, pellets=2, speed=50, crit=1)),
    'Symmetra': (225, Weapon(6, 6, None, 0.1, 70, 1.5, reach=12, refill=0.73, crit=1)),
    'Moira': (200, Weapon(0, 0, None, 0.192, INFINITE, 1.5, reach=15, crit=1)),
    'Genji': (200, Weapon(29, 29, None, 0.88 / 3, 30, 1.5, speed=60, refill=0.6)),
    'Doomfist': (250, Weapon(6, 1.8, (15, 30), 0.32, 4, 0.32 + 4 * 0.656, pellets=11, spread=2.15, speed=80,
                             recharge=.65)),
    'Reinhardt': (500, Weapon(85, 85, None, 0.96, INFINITE, 1.5, reach=5, crit=1)),
    'Winston': (500, Weapon(5.76, 5.76, None, 0.096, 100, 1.5, reach=8, refill=0.711, crit=1)),
    'Brigitte': (200, Weapon(35, 35, None, 0.6, INFINITE, 1.5, reach=6, crit=1)),
}

# Alternate fires that are weapon volleys (kits.Kit.alt): name -> (weapon, rounds a shot), wiki as above
# (research/hero-weapons-wiki.md). shortcut: land at once like the primaries; Roadhog's flak is the primary's
# pellets (no 50 pre-detonation hit), Zarya's grenade is at 0 % charge, Baptiste's grenade only heals (not served)
# and uses no ammo here (its own 10-round magazine is not modelled).
# Quick melee (every hero, graph 67): wiki Melee page, 30 damage within 2.5 m, 1 s cooldown.
# shortcut: Reinhardt's hammer, Brigitte's flail and Torbjorn's hammer swing the same 30 (no 85 / 35 / 55).
MELEE = Weapon(30, 30, None, 1.0, INFINITE, 0, reach=2.5, crit=1)
ALT_KIT = {
    'Mei': (Weapon(75, 75, None, .8, 120, 1.5, speed=115), 10),
    'Cassidy': (Weapon(50, 15, (20, 40), 0.125, 6, 1.5, spread=6.5, crit=1), 1),  # Fan the Hammer: Volley 01D2.15, 8/s
    'Roadhog': (Weapon(6.6, 1.98, (15, 30), 0.848, 5, 2.0, pellets=25, spread=10.05, speed=80), 1),
    'Torbjörn': (Weapon(12.5, 3.75, (10, 20), 0.8, 18, 2.0, pellets=10, spread=4.15, speed=120), 3),
    'Zarya': (Weapon(47, 47, None, 1.0, 100, 1.5, speed=25, crit=1), 25),
    'Baptiste': (Weapon(0, 0, None, 0.9, 45, 1.5, speed=60), 0),
    # Photon Projector's orb: held 1 s (kits alt_windup) it fires at full charge for 10 rounds; it flies and hurts
    # as ABILITY_HITS[('Symmetra', 0x02)]. shortcut: always full charge (fires once charged, not on release).
    'Symmetra': (Weapon(0, 0, None, 0.25, 70, 1.5, speed=25), 10),
    # Orb Volley: 48 an orb, one round each, loaded every 0.524 s up to 5 (kits.ChargedAlt, `pellets` = orbs loaded),
    # 0.6 s recovery. Not flown: its Volley's drawn child 0x02CD showed as a growing glass bubble on the crosshair.
    'Zenyatta': (Weapon(48, 48, None, 0.6, 20, 1.5, speed=90), 1),
}
# Moira's Biotic Grasp (right mouse, Match.update_grasp), wiki: 9.6 damage and 4.61 self-heal per 0.192 s to one
# enemy in sight within 20 m. shortcut: lock-on cone guessed (15 degrees), no biotic energy refill.
GRASP_DAMAGE, GRASP_HEAL, GRASP_TICK, GRASP_REACH = 9.6, 4.61, 0.192, 20
GRASP_CONE = math.cos(math.radians(15))
# Mercy's Caduceus Staff heal beam: 60 HP/s within 15 m (1.74); shortcut: the lock-on cone is Moira's, upgrade when
# the staff's own TrackTargets cone is decoded.
STAFF_HEAL, STAFF_TICK, STAFF_REACH = 60.0, 0.1, 15


@dataclass(frozen=True)
class AbilityHit:
    """A thrown or fired ability (kits.ABILITIES cast), landed by the server like a Helix rocket."""
    speed: float  # m/s; 0 is instant
    range: float  # m: it lands at the first wall or bot, else here
    direct: float = 0  # to the bot it hits
    splash: float = 0  # within radius of where it lands
    radius: float = 0
    rings: tuple = ()  # (radius, scale) of its explosion (the 1.74 Volley's rings: research/damage-data); () Helix's shape
    # flown to where it lands: the child (field 436BEFE6, tools/assets/stu_refs.py) of the Volley's
    # m_projectileEntity, the part the client draws (research/bot-fire.md); Steel Trap's has none
    entity: int = None
    self_harm: bool = False  # the thrower takes half its splash, as from a Helix rocket
    # (m/s away from it on the level, m/s up) added to its thrower's velocity when it goes off within `radius` of
    # him: Concussion Mine's own launch (graph 096A, node 26)
    kick: tuple = None
    pierce: bool = False  # flies on through enemies to a wall: each takes `direct` when it gets there (Fire Strike)
    # a salvo (Micro Missiles): `shots` hits, `rate` a second, each from the next launcher in turn (body space, m:
    # right, up, forward), flown at what the crosshair is on
    shots: int = 1
    rate: float = 0
    launchers: tuple = ()


# (hero, command bit) -> hit, from the last OW1 wiki revisions (research/wiki/roster/).
# shortcut: damage only: no stun, sleep, snare, anti-heal, burn, knockback or healing; Biotic Grenade only hurts;
# throws fly straight (no arc) and mines, traps and bombs go off where they land; ranges of throws (and Fire
# Strike's 40 m) are guesses; no cast time (Fire Strike leaves at once, not after the wiki's 0.5 s);
# self-damage (Dynamite, Sticky Bombs) is half the splash like Helix's, not the wiki's own figure
# (the authored lifetimes are expressions); Echo's six bombs are one; D.Va's missiles have no 0.256 s cast time,
# no spread and leave the bind-pose hardpoints; Coach Gun's 15 pellets all hit.
# Entities: each ability graph's first WeaponVolley (kits.Ability.cast); Concussive Blast, Halt!, the Immortality
# Field and Sentry Turret fly only (no damage; their effects are not served).
ABILITY_HITS = {
    ('Cassidy', 0x10): AbilityHit(30, 6.5, splash=25, radius=3, rings=((3, 1),), entity=0x0400000000000453),  # Flashbang
    ('Ana', 0x10): AbilityHit(30, 20, splash=50, radius=4, rings=((4, 1),), entity=0x04000000000015BC),  # Biotic Grenade: 0B97.0
    ('Ana', 0x08): AbilityHit(60, 60, direct=5, entity=0x0400000000001614),  # Sleep Dart
    ('Pharah', 0x10): AbilityHit(60, 30, entity=0x0400000000000160),  # Concussive Blast
    ('Widowmaker', 0x10): AbilityHit(20, 20, splash=75, radius=3, entity=0x0400000000000161),  # Venom Mine (5 s at once)
    ('Junkrat', 0x08): AbilityHit(25, 25, splash=120, radius=3, rings=((3, 1),), entity=0x040000000000109F, kick=(15.0, 15.0)),  # Concussion Mine
    ('Junkrat', 0x10): AbilityHit(10, 10, splash=80, radius=1, entity=0x04000000000011A9),  # Steel Trap
    ('Orisa', 0x02): AbilityHit(25, 25, entity=0x040000000000197E),  # Halt!
    ('Ashe', 0x10): AbilityHit(25, 25, splash=50, radius=5, entity=0x0400000000002A5C, self_harm=True),  # Dynamite: no burn
    ('Ashe', 0x08): AbilityHit(0, 9.4, direct=90),  # Coach Gun
    ('Echo', 0x02): AbilityHit(50, 30, direct=30, splash=150, radius=2, rings=((2, 1),), entity=0x0400000000002CBB,
                               self_harm=True),  # Sticky Bombs: six of 1C36.0's 5 + 25
    ('Baptiste', 0x10): AbilityHit(60, 20, entity=0x0400000000002E86),  # Immortality Field
    # Accretion: 1BBB.3's SetVars give 30 on impact and 40 in the explosion (the wiki's 70 is both)
    ('Sigma', 0x10): AbilityHit(37.5, 50, direct=30, splash=40, radius=2.5, rings=((1.5, 1), (2.5, 0.3)),
                                entity=0x0400000000003330),
    ('Symmetra', 0x08): AbilityHit(15, 15, entity=0x04000000000028BA),  # Sentry Turret
    ('Reinhardt', 0x10): AbilityHit(25, 40, direct=100, entity=0x0400000000000211, pierce=True),  # Fire Strike
    # Micro Missiles: 18 missiles at 11/s (7 direct, 4 splash in 1.5 m), from graph 0x1128 Volley's two hardpoints
    # (0x0DC...75 / 77 on the mech model 0x0D00...11CE, bind pose), drawn as the projectile 0x0400...1E8A's child
    # 0x0400...1E99 (1E8A itself draws nothing when created, as research/bot-fire.md found for other Volleys).
    ('D.Va', 0x10): AbilityHit(40, 40, direct=7, splash=4, radius=1.5, entity=0x0400000000001E99, self_harm=True,
                               shots=18, rate=11, launchers=((0.74, 1.83, 0.35), (-0.74, 1.83, 0.36))),
    # Symmetra's full-charge orb (right mouse, kits.AltFire): 60 + 60 splash = the wiki's 120 direct; 40 m guessed
    ('Symmetra', 0x02): AbilityHit(25, 40, direct=60, splash=60, radius=2, entity=0x04000000000002EF),
}


@dataclass(frozen=True)
class Orb:
    """Moira's Biotic Orb (wiki): flies `speed` m/s for `seconds`; every ORB_TICK it gives `rate` * ORB_TICK to each
    target within `radius` (hurt allies when `heal`, else enemy bots) until `total` is spent."""
    rate: float
    total: float
    radius: float
    heal: bool
    entity: int  # drawn as the picked Volley's projectile child, like ABILITY_HITS
    speed: float = 20
    seconds: float = 7


ORB_TICK = 0.192
# (hero, command bit, pick bit) -> orb: E, then left mouse (0x01) = graph 4324's Volley 10, the healing orb; right
# mouse (0x02) = Volley 11, the damage orb (kits.Choice).
# shortcut: straight flight to the first wall (no bounces), no slow-down while tethered, no line-of-sight check.
ORBS = {
    ('Moira', 0x10, 0x01): Orb(65, 300, 5, True, 0x0400000000001B42),
    ('Moira', 0x10, 0x02): Orb(50, 200, 4, False, 0x0400000000001E62),
}


def ability_splash(hit, distance):
    """Splash at distance from where it lands, by its rings: the first ring's scale inside it, a straight line from
    ring to ring, none past the last (LizRollin's HitDef.splash). shortcut: an ability without data rings takes
    Helix's shape (full to a third of the radius, half at the radius)."""
    if not hit.splash:
        return 0
    rings = hit.rings or ((hit.radius / 3, 1.0), (hit.radius, 0.5))
    inner, scale = rings[0]
    if distance <= inner:
        return round(hit.splash * scale)
    for radius, edge in rings[1:]:
        if distance <= radius:
            return round(hit.splash * (scale + (edge - scale) * (distance - inner) / (radius - inner)))
        inner, scale = radius, edge
    return 0


@dataclass(frozen=True)
class Heal:
    """A healer bot's healing: `rate` health per second to each ally in `reach` and in sight (only the most hurt
    one if `one`), `own` per second to itself, and only while it has no enemy to shoot if `idle`."""
    rate: float
    reach: float
    one: bool = False
    own: float = 0.0
    idle: bool = False


HEAL_PERIOD = 0.192  # the wiki's healing tick
# Practice vs AI healers, from the same wiki pages: Biotic Rifle (70 per dart), Orb of Harmony (30/s, 40 m),
# Healing Boost (16/s to allies in 12 m, 12/s to Lucio).
# shortcut: Ana's darts heal as a steady 70 per 0.8 s shot while she has no enemy to shoot, at once and unseen; the
# orb and the song are not drawn, and barriers do not block them. Upgrade with each ability's state.
# Uprising's Null Sector units: the hero assets the mission director graph 0D4A names (research/uprising-mission.md),
# hero -> (name, body from the hero's m_gameplayEntity). shortcut: the names go by how often 0D4A uses each (the hero
# assets' name strings are not decoded); their hit volumes are Soldier's (hitbox.HEROES): the units' bodies are
# outside the root CMF.
OMNICS = {
    0x02E0000000000179: ('Nulltrooper', 0x040000000000184A),
    0x02E0000000000178: ('Slicer', 0x0400000000001842),
    0x02E0000000000173: ('Eradicator', 0x0400000000001775),
    0x02E000000000017C: ('Detonator', 0x040000000000187E),
}
# Apart from HERO_KIT: no weapon graph is shown for them. shortcut: guessed stats (the wiki is unreachable, the units'
# weapon data unread): a slow light rifle, a short beam, a heavy rifle; six of the first wave focused at medium
# difficulty take about 3 s to kill 200 health, one Nulltrooper about 25 s.
NULLTROOPER_GUN = Weapon(6, 6, None, 0.4, 20, 1.5)
SLICER_BEAM = Weapon(3, 3, None, 0.1, INFINITE, 0, reach=10.0)
OMNIC_KIT = {'Nulltrooper': (200, NULLTROOPER_GUN), 'Slicer': (150, SLICER_BEAM),
             'Eradicator': (500, Weapon(15, 15, None, 0.25, 60, 2.0)), 'Detonator': (150, NULLTROOPER_GUN)}
# The first wave at the spawn markers nearest the players' spawn. shortcut: one wave, placed once; the mission's
# waves, routes and counts are 0D4A's and not read.
UPRISING_WAVE = (0x02E0000000000179,) * 4 + (0x02E0000000000178,) * 2
HEALS = {'Ana': Heal(70 / 0.8, RIFLE_RANGE, one=True, idle=True), 'Zenyatta': Heal(30, 40.0, one=True),
         'Lúcio': Heal(16, 12.0, own=12)}


@dataclass(frozen=True)
class Ability:
    """A hero bot's signature ability, damage and health only. 'mend': below half health, heal `amount` per second
    for `seconds`, not firing and taking `taken` of all damage meanwhile; 'guard': hurt in a fight, soak `amount`
    damage for `seconds`; 'burst': `amount` damage to its target within `reach`; 'leech': heal `amount` of the damage
    it deals; 'discord': its target within `reach` takes `amount` more damage. `cooldown` counts from the effect's end."""
    kind: str
    amount: float
    seconds: float = 0.0
    cooldown: float = 0.0
    reach: float = 0.0
    taken: float = 1.0


# One signature ability per hero, from the same wiki pages.
# shortcut: nothing is drawn; bursts land at once on the target alone (no splash, stun, knockback or Biotic Grenade
# heal) and Ana's throw reach is a guess; Take a Breather heals its 300 over the 1 s inside its 2 s animation;
# Overload's armor is a plain soak; Self-Repair has no resource meter; Sombra's Hack has no damage or health effect,
# so she has none. Upgrade each from its ability state.
ABILITIES = {
    'Roadhog': Ability('mend', 300, 1.0, 8, taken=0.5),  # Take a Breather
    'Mei': Ability('mend', 50, 3.0, 12, taken=0.0),  # Cryo-Freeze
    'Bastion': Ability('mend', 90, 3.33, 1),  # Self-Repair
    'Zarya': Ability('guard', 200, 2.0, 10),  # Particle Barrier
    'Torbjörn': Ability('guard', 100, 5.0, 10),  # Overload's +100 armor
    'Cassidy': Ability('burst', 25, cooldown=10, reach=7),  # Flashbang
    'Soldier: 76': Ability('burst', 120, cooldown=6, reach=RIFLE_RANGE),  # Helix Rockets, a direct hit
    'Ana': Ability('burst', 60, cooldown=10, reach=20),  # Biotic Grenade
    'Lúcio': Ability('burst', 25, cooldown=4, reach=8),  # Soundwave
    'Reaper': Ability('leech', 0.35),  # The Reaping
    'Zenyatta': Ability('discord', 0.25, reach=40),  # Orb of Discord
}
# Abilities shown as a second Volley of the weapon graph: owner state, remote slot (research/bot-fire.md).
ABILITY_VOLLEYS = {'Soldier: 76': (16, 6)}  # Helix Rockets
# Each roster body's weapon graph (its STUWeaponComponent) as (index, owner states, sync vars, remote slots, the remote
# slots the client reads no bit for: graph_layout.py's silent set; without it every later slot is read one bit off), its
# first Volley state (taken as the primary fire) as owner state and remote slot, and the presence bit of var53, the
# ammo, which the remote Volley needs before it draws (tools/assets/graph_layout.py; research/bot-fire.md).
# shortcut: Zarya is left out, her bots fire unseen: her one Volley (graph 0x0241, owner state 19) is the alternate
# fire's energy ball, and the primary beam is not a Volley; upgrade when her beam state is found.
WEAPON_GRAPHS = {
    'Roadhog': ((0x01C6, 56, 14, 37, frozenset({11, 12})), 9, 4, 13),
    'Bastion': ((0x012E, 46, 16, 28), 14, 7, 5), 'Cassidy': ((0x01D2, 63, 18, 38), 11, 5, 6),
    'Mei': ((0x05E6, 65, 20, 45, frozenset({2})), 13, 7, 7),
    'Reaper': ((0x0019, 44, 11, 29, frozenset({10, 11, 15})), 6, 2, 9),
    'Soldier: 76': ((0x0254, 81, 23, 49, frozenset({11, 39})), 10, 4, 8),
    'Sombra': ((0x0B1B, 92, 42, 61, frozenset({11})), 6, 1, 8),
    'Torbjörn': ((0x1683, 49, 17, 27, frozenset({15})), 10, 5, 16),
    'Ana': ((0x0B94, 84, 22, 51, frozenset({23, 24, 25, 31})), 5, 0, 10),
    'Lúcio': ((0x0776, 61, 19, 39, frozenset({4, 8})), 14, 7, 9),
    'Zenyatta': ((0x0164, 104, 22, 73, frozenset({19, 20, 21, 22, 23, 31, 45})), 14, 7, 7),
    'D.Va': ((0x050A, 33, 10, 25), 3, 0, 8),  # her mech's Fusion Cannons (content.SPAWN_BODY)
}
# Heroes whose primary Volley is automatic (shot config mode 3, read live on the mannequins 2026-11-09): the client
# fires it itself up to the ammo var per start and ignores shot records, so it has to end at each reload
# (research/bot-fire.md). The others (modes 1 and 2) fire one shot per shot record.
AUTOMATIC = frozenset({'Soldier: 76', 'Mei', 'Bastion', 'Lúcio'})
# The gun model of a hero's weapon graph: its CosmeticEntity's remote slot, the graph's m_remoteSyncNodesBitCount (an H=0
# binding names a remote slot, 0x7FF6D1C2D726) and var49's presence bit, None where var49 is an entity variable. The
# Volley's muzzle is a hard point on var49's entity, the CosmeticEntity's output, made while var34 || var478 is false;
# unbound, the client starts the shots and muzzle effects at the body's feet (research/bot-fire.md).
# shortcut: Torbjörn's gun also gates on var842 and Reaper's and Zenyatta's var49 is not synced; Bastion and D.Va have no
# gun state. Their fire still starts at the feet; upgrade when those gates are read.
WEAPON_MODELS = {'Mei': (0, 6, None), 'Lúcio': (0, 6, None), 'Sombra': (0, 6, None), 'Cassidy': (16, 6, None),
                 'Ana': (34, 6, None), 'Roadhog': (9, 6, 12), 'Soldier: 76': (5, 6, 7)}
# The effect states (STU_691BFA55) in a hero's primary Volley's subgraph, by remote slot: the fire you see, played on
# var49's entity. A remote Volley does not begin its subgraph, so the bot frame begins them while the bot fires.
# Needs the hero in WEAPON_MODELS: with var49 unbound the client crashes reading [0 + 0x1F8] (research/bot-fire.md).
WEAPON_EFFECTS = {'Mei': (5, 6)}
# The m_projectileEntity of each mode-1 primary Volley ([info+0x11C] == 1, read live on the mannequins; Roadhog, Reaper
# and Sombra are 2, AUTOMATIC 3). The shot-record handler (0x7FF6D1D07570) of a remote mode-1 Volley makes no projectile,
# only the per-shot muzzle effect, so each shot names a server-made one, as a Helix rocket's does (research/bot-fire.md).
# A flying one is sent flying at the weapon's speed; Cassidy's hitscan tracer is drawn from the record alone.
# shortcut: straight flights, no drop (Torbjörn's Volley has projectile motions).
WEAPON_SHOTS = {'Cassidy': 0x04000000000002A7, 'Ana': 0x04000000000015B7, 'Torbjörn': 0x0400000000002A66,
                'Zenyatta': 0x0400000000000236}

# (map record, hero, team, position, yaw degrees, behaviour, waypoints): spawner entity 0x04000000000013F7.
# Behaviour: graph B92 shoots, B8E patrols hint points in order, none stands still.
ROSTER = (
    (3, 0x02E000000000016C, 0, (74.34, -2.0, 6.2), 144.7, None, ()),
    (19, 0x02E000000000016B, 1, (82.36, -2.0, -12.02), 324.7, 'shoot', ()),
    (69, 0x02E000000000016B, 1, (43.22, 2.0, 7.86), -35.5, None, ()),
    (70, 0x02E000000000016E, 1, (93.65, -4.0, 43.8), -125.3, 'patrol', ((93.65, -4.0, 43.8), (76.93, -4.0, 31.94))),
    (75, 0x02E000000000016C, 0, (43.58, 11.0, 0.01), -215.3, 'patrol', ((40.32, 11.0, 4.49), (50.09, 11.0, 11.33),
        (53.86, 11.0, 7.0), (62.83, 11.0, 13.36), (53.86, 11.0, 7.0), (55.78, 11.0, 2.29), (46.7, 11.0, -4.52))),
    (276, 0x02E000000000016B, 1, (46.48, 2.0, 10.19), -35.5, None, ()),
    (287, 0x02E000000000016D, 0, (61.7, -2.0, -2.77), 144.7, 'shoot', ()),
    (366, 0x02E000000000016B, 1, (92.66, -2.0, -7.46), -35.5, None, ()),
    (403, 0x02E000000000016B, 1, (73.27, -2.0, -19.08), 324.7, None, ()),
    (408, 0x02E000000000016B, 1, (51.1, 5.0, 6.92), 144.7, 'patrol', ((71.63, 5.0, 21.22), (50.37, 5.0, 6.36))),
    (429, 0x02E000000000016B, 1, (85.62, -2.0, -9.71), 324.7, 'shoot', ()),
    (501, 0x02E000000000016E, 1, (100.59, -4.0, 34.01), -125.3, 'patrol', ((100.59, -4.0, 34.01),
        (84.28, -4.0, 22.44), (100.59, -4.0, 34.01), (92.23, -4.0, 28.08))),
    (506, 0x02E000000000016E, 1, (104.06, -4.0, 29.12), -125.3, 'patrol', ((104.06, -4.0, 29.12),
        (95.91, -4.0, 23.33), (85.3, -1.74, 15.81))),
    (522, 0x02E000000000016E, 1, (97.12, -4.0, 38.9), -125.3, 'patrol', ((97.12, -4.0, 38.9), (80.81, -4.0, 27.33))),
    (523, 0x02E000000000016C, 0, (71.08, -2.0, 3.89), 144.7, None, ()),
)


def chest(position, height=1.4):
    return (position[0], position[1] + height, position[2])


# Primary attacks without Matrix-blockable bullets (the bot simulation otherwise treats beams as hitscan).
MATRIX_IGNORES = frozenset(hero.guid for hero in heroes().values()
                         if hero.name in {'Winston', 'Zarya', 'Symmetra', 'Moira', 'Mei', 'Reinhardt', 'Brigitte'})
MATRIX_CASTS = frozenset(hero.guid for hero in heroes().values() if hero.name in {'Soldier: 76', 'Ana', 'Cassidy'})


def matrix_intersects(origin, yaw, pitch, start, end):
    """A segment against graph 02CC's four boxes, whose union is [-1.25,-1,.8]..[1.25,1,10.25] around the eye `origin`."""
    # shortcut: scale 1 and the eye/aim frame; replace once the native box transform/factor is verified.
    forward = aim_direction(yaw, pitch)
    angle, tilt = yaw * math.pi / 32768, pitch * math.pi / 32768
    right = (-math.cos(angle), 0.0, math.sin(angle))
    up = (math.sin(angle) * math.sin(tilt), math.cos(tilt), math.cos(angle) * math.sin(tilt))
    local = [tuple(sum((p - o) * v for p, o, v in zip(point, origin, axis))
                   for axis in (right, up, forward)) for point in (start, end)]
    near, far = 0.0, 1.0
    for a, b, low, high in zip(*local, (-1.25, -1.0, .8), (1.25, 1.0, 10.25)):
        delta = b - a
        if abs(delta) < 1e-12:
            if not low <= a <= high:
                return False
            continue
        first, last = sorted(((low - a) / delta, (high - a) / delta))
        near, far = max(near, first), min(far, last)
        if near > far:
            return False
    return True


class Projectile:
    """A shot in flight, straight from origin to target: a bot's at the player's chest, or a player's ability
    cast (`entity`: its Volley's projectile)."""

    def __init__(self, origin, target, now, speed=PROJECTILE_SPEED, entity=PROJECTILE):
        self.origin, self.target, self.entity = origin, target, entity
        self.start, self.end = now, now + math.dist(origin, target) / speed
        self.checked_at = now
        self.index = 0  # the match numbers its projectiles for their entity ids
        self.landed = False
        self.by = None  # the bot that fired it: the death screen names it
        self.shown = {}  # viewer -> the entity id its client has now
        self.native_owner = None  # this viewer receives the native projectile instead of the moving effect
        self.components = {}  # world.create components of its entity (a player's cast: filter bits 26, its team)

    def position(self, now):
        if now >= self.end:
            return self.target
        f = (now - self.start) / (self.end - self.start)
        return tuple(o + (t - o) * f for o, t in zip(self.origin, self.target))

    def rotation(self):
        """Quaternion (x, y, z, w) turning +z (an entity's forward) along the flight: yaw about y, then pitch."""
        dx, dy, dz = (t - o for o, t in zip(self.origin, self.target))
        yaw, pitch = math.atan2(dx, dz) / 2, math.atan2(-dy, math.hypot(dx, dz)) / 2
        return (math.cos(yaw) * math.sin(pitch), math.sin(yaw) * math.cos(pitch), -math.sin(yaw) * math.sin(pitch),
                math.cos(yaw) * math.cos(pitch))

    def hits(self, position, yaw=0.0, volumes=hitbox.SOLDIER):
        """Whether a player standing at position, turned yaw radians, when it lands takes the shot: the
        aimed point is within the shot's radius of one of its hit volumes."""
        return hitbox.touches(hitbox.posed(volumes, position, yaw), self.target, PROJECTILE_RADIUS)


def range_bots(grid=None, mesh=None):
    """With the map's nav grid and collision mesh, patrols go round the walls their hint points have between them."""
    return [TrainingTarget(position, yaw, BODIES[hero], team, behaviour,
                           nav.loop(grid, mesh, position, waypoints) if grid and waypoints else waypoints,
                           0xB0000001 + (n << 8), stops=waypoints)
        for n, (_, hero, team, position, yaw, behaviour, waypoints) in enumerate(ROSTER)]


def _turned(yaw, toward, most):
    """Degrees yaw turned toward `toward` by at most `most`, the short way, within +-180."""
    left = (toward - yaw + 180) % 360 - 180
    return (yaw + max(-most, min(most, left)) + 180) % 360 - 180


def _throttles(yaw, direction):
    """(right, forward) throttle bytes of a walk along direction (x, _, z) by a body facing yaw degrees: forward is
    (sin, cos) of the yaw, right (-cos, sin), as in the mover (LizRollin's bots.throttles)."""
    dx, dz = direction[0], direction[2]
    norm = math.hypot(dx, dz)
    if not norm:
        return 0, 0
    angle = math.radians(yaw)
    forward = (dx * math.sin(angle) + dz * math.cos(angle)) / norm
    right = (-dx * math.cos(angle) + dz * math.sin(angle)) / norm
    return round(right * 127), round(forward * 127)


def _slide_length(pace, brake, dt):
    """How far friction slides a body on from pace, tick by tick (LizRollin's bots.Bot._slide_length)."""
    slow, length = brake * dt, 0.0
    if slow <= 0:
        return 0.0
    pace -= slow
    while pace > 0:
        length += pace * dt
        pace -= slow
    return length


ARMOUR_BLOCK = 3  # damage armour stops of each hit, at most half (OW1 since the January 24, 2019 patch: was 5)


def armour_cut(amount, total, most, armour, shields, hits=1):
    """What `hits` equal hits dealing `amount` in all take off a body at `total` of `most` with these pools (world.health's
    order): shields first, then armour, which stops ARMOUR_BLOCK of each hit that reaches it (at most half), then health."""
    if not armour or amount <= 0:
        return amount
    floor, lost = most - armour - shields, 0.0
    for _ in range(hits):
        left = amount / hits
        taken = min(left, max(0.0, total - lost - floor - armour))  # shields
        lost, left = lost + taken, left - taken
        armour_now = min(armour, max(0.0, total - lost - floor))
        if left > 0 and armour_now > 0:
            left = max(left - ARMOUR_BLOCK, left / 2)
            taken = min(left, armour_now)
            lost, left = lost + taken, left - taken
        lost += left
    return lost


def patrol_points():
    """Where the Range bots stand and patrol: what the Range's nav grid has to cover."""
    return [point for _, _, _, position, _, _, waypoints in ROSTER for point in (position, *waypoints)]


def hero_name(hero):
    """A bot hero's name: a playable hero's from the catalog, else an omnic's."""
    return heroes()[hero].name if hero in heroes() else OMNICS[hero][0]


def omnic_bots(spawns, team=1):
    """Uprising's first wave: UPRISING_WAVE at that team's spawn points in turn (the map's markers, nearest first)."""
    return hero_bots(spawns, team, [(hero, OMNICS[hero][1], OMNICS[hero][0]) for hero in UPRISING_WAVE], kits=OMNIC_KIT)


PAYLOAD = 'payload'  # a TrainingTarget behaviour: stands still, is shot at by the omnics, shows as PAYLOAD HEALTH
PAYLOAD_HEALTH = 1500  # shortcut: guessed; the real payload entity, model and health are unread


def payload_target(position):
    """Uprising's payload, stood in by a Range training bot on the players' team at their point (entity 0xB000FF01)."""
    return TrainingTarget(position, 0, team=0, behaviour=PAYLOAD, entity=0xB0000001 + (0xFF << 8), max_health=PAYLOAD_HEALTH)


def kill_feed(index, hero, killer, fresh, count=1):
    """Instance `index` of a dead bot's frame: its kill feed graph naming `killer` (a body, or None), its pulser at
    `count` pulses. The client posts a line per pulse it has not seen; for a pulser it first reads, only with `fresh`."""
    graph, slot, bit = HERO_KILL_FEED if hero else KILL_FEED
    return Instance(index, graph, presence={bit: Entity(killer)} if killer else {},
                    active={slot: pulser_state(fresh, count)})


def hero_bots(spawns, team, picks, first=0, kits=HERO_KIT):
    """Practice vs AI: a shooting bot per (hero GUID, body, HERO_KIT name) pick at that team's spawn points, in
    turn, at running pace, with the hero's health, primary fire and hit volumes; the match steers them. Entity ids
    count from bot number `first`."""
    points = [spawn for spawn in spawns if spawn.team == team]
    bots = []
    for n, (hero, body, name) in enumerate(picks if points else ()):
        health, weapon = kits[name]
        spawn = points[n % len(points)]
        bots.append(TrainingTarget(spawn.position, spawn.yaw, body, team, 'shoot', (), 0xB0000001 + ((first + n) << 8),
            hero=hero, volumes=hitbox.HEROES[body], speed=HERO_SPEED, weapon=weapon, max_health=health,
            heal=HEALS.get(name), ability=ABILITIES.get(name)))
    return bots


class TrainingTarget:
    def __init__(self, position, yaw, body=BOT_BODY, team=1, behaviour=None, waypoints=(), entity=0xB0000001,
                 hero=None, volumes=hitbox.TRAINING_BOT, speed=PATROL_SPEED, weapon=SOLDIER_RIFLE,
                 max_health=TARGET_HEALTH, heal=None, ability=None, stops=()):
        self.heal, self.ability = heal, ability  # a healer's Heal, a hero bot's Ability
        self.ability_ready = self.effect_until = self.soak = 0.0  # its cooldown's end, its effect's end, guard left
        self.hacked_until = 0.0
        self.discorded = (0.0, 0.0)  # (until, extra damage taken) from an enemy Zenyatta
        self.hero, self.volumes, self.speed = hero, volumes, speed  # hero: a hero body's component 123; Range bot bodies have none
        self.weapon, self.max_health = weapon, max_health
        self.position = tuple(position)
        self.velocity = (0.0, 0.0, 0.0)
        self.knocked = None  # (velocity, until, moved at): a knockback carries it instead of its legs
        self.home = self.position
        self.yaw = self.home_yaw = yaw
        self.pitch = 0.0  # aim, degrees, positive down: at its target's chest while it fights
        self.body, self.team, self.behaviour, self.waypoints = body, team, behaviour, tuple(waypoints)
        self.stops = frozenset(stops or waypoints)  # the hint points a patrol waits at; its waypoints add nav corners
        if behaviour == 'patrol':  # its body's own run speed, ground acceleration and friction (LizRollin's bots.py)
            values = mover_data(body)
            self.speed = values.run_forward
            self.accel, self.brake = values.accel_ground * self.speed, values.decel_ground_base * self.speed
        self.pace, self.wait, self.keys, self.turning, self.heading = 0.0, FIRST_WAIT, False, 0.0, yaw  # a patrol's walk
        self.held = self.last_held = (0, 0)  # its throttles now, and when it last held its keys: its 3P walk
        self.held_at = None  # match time it last held its keys
        self.entity = entity
        self.controller = entity & ~0xFF
        self.slot = None
        self.name = hero_name(hero) if hero in heroes() or hero in OMNICS else 'Training Bot'
        self.health = max_health
        self.damage_by = {}
        self.dead_at = None
        self.deaths, self.killer = 0, None  # this body's deaths, and the body that dealt the last one
        self.respawn_seconds = 3.0  # the match sets its mode's for hero bots (Match.respawn_seconds)
        self.revived_at = None
        self.next_attack_at = None
        self.rounds = weapon.rounds
        self.shots = 0  # every shot fired, for the remote Volley's event counter
        self.ability_uses = 0  # for the ability Volley's event counter (ABILITY_VOLLEYS)
        self.in_flight = []  # [lands at, origin, direction, fired at, distance checked], at the current target
        self.leg = 0
        self.moved_at = None
        self.goal = None  # where the match last routed a hero bot
        self.target, self.retarget_at = None, 0.0  # the enemy a hero bot shoots, and when it picks again
        self.poses = deque(maxlen=POSE_HISTORY)  # (match time, position, yaw) after each match tick

    @property
    def pools(self):
        """(armour, shields) of a hero bot's body (0033's vars, pools.py); Range bots have health only."""
        return body_pools(self.hero)[1:] if self.hero in heroes() else (0.0, 0.0)

    def remember(self, now):
        self.poses.append((now, self.position, self.yaw))

    def pose_at(self, when):
        """(position, yaw) as a client drew this bot at match time `when`: between the two kept poses around it, else
        the oldest or newest; the current pose when none is kept."""
        before = after = None
        for pose in self.poses:
            if pose[0] <= when:
                before = pose
            else:
                after = pose
                break
        if before is None and after is None:
            return self.position, self.yaw
        if before is None or after is None:
            return (before or after)[1:]
        share = (when - before[0]) / (after[0] - before[0])
        position = tuple(a + (b - a) * share for a, b in zip(before[1], after[1]))
        return position, (before if share < 0.5 else after)[2]

    def ray(self, origin, direction, max_distance=100, at=None):
        """(distance, critical) along the shot to this bot's original hit volumes, or None when it misses; `at`: the
        match time of the shooter's view (pose_at), else the current pose."""
        if not self.health:
            return None
        position, yaw = (self.position, self.yaw) if at is None else self.pose_at(at)
        return hitbox.ray(hitbox.posed(self.volumes, position, math.radians(yaw)), origin, direction, max_distance)

    def hit(self, origin, direction, now):
        hit = self.ray(origin, direction)
        if hit is None:
            return None
        self.damage(20, now)
        return hit[0]

    def center(self):
        return (self.position[0], self.position[1] + 1.0, self.position[2])

    def damage(self, amount, now, hits=1, by=None):
        """`amount` in all from `hits` equal hits (a shotgun's pellets: armour cuts each); `by`: the body that deals it."""
        if not self.health:
            return
        until, extra = self.discorded
        amount *= 1 + extra if now < until else 1
        if now < self.effect_until:
            if self.ability.kind == 'mend':
                amount *= self.ability.taken
            soak = min(self.soak, amount)
            self.soak -= soak
            amount -= soak
        amount = armour_cut(amount, self.health, self.max_health, *self.pools, hits)
        self.health = max(0, self.health - amount)
        if not self.health:
            self.dead_at, self.killer = now, by
            self.deaths += 1

    def attack_player(self, position, now, seen=True):
        """The Projectile this bot fires at an enemy at position, or None: shooters turn to it in range and
        in sight (seen: nothing solid between chests) and fire every 1.75-2.5 s, the first shot one
        interval after it is first in reach."""
        # shortcut: aim tracks the player directly, with no lead.
        if not self._face(position, seen, ATTACK_RANGE):
            return None
        if self.next_attack_at is None:
            self.next_attack_at = now + random.uniform(*ATTACK_INTERVAL)
            return None
        if now < self.next_attack_at:
            return None
        self.next_attack_at = now + random.uniform(*ATTACK_INTERVAL)
        return Projectile(chest(self.position, 1.2), chest(position), now)

    def fire(self, position, yaw, now, seen=True, aim_error=DIFFICULTY['medium'][1], volumes=hitbox.SOLDIER, blocked=None):
        """Distances of this hero bot's pellet hits, since the last call, on an enemy with hit volumes `volumes`
        at position, turned yaw radians: in sight and its weapon's reach it fires REACTION_SECONDS after the
        sighting, then at the weapon's cadence, each shot aimed at the chest with a random error of aim_error
        degrees. Bots never headshot (wiki). A projectile pellet lands when it has flown the distance to the aimed
        point and hits only if the enemy is still in its path then. `blocked` can consume a shot's traveled segment
        before it damages the target."""
        # shortcut: no tracer is drawn (the bot body's remote Volley shows nothing, bot-fire.md), and a scattered
        # shot is not stopped by walls the chest-to-chest sight line misses.
        weapon = self.weapon
        volumes = hitbox.posed(volumes, position, yaw)
        hits = self._land(now, volumes, blocked)
        if not self._face(position, seen, weapon.reach):
            return hits
        if self.next_attack_at is None:
            self.next_attack_at = now + REACTION_SECONDS
        # no catch-up burst for time the bot was not asked to shoot (the enemy was dead or the round over)
        self.next_attack_at = max(self.next_attack_at, now - weapon.shot_seconds)
        origin, aimed = chest(self.position, 1.2), chest(position)
        flight = math.dist(origin, aimed) / weapon.speed if weapon.speed else None
        dx, dy, dz = (a - o for a, o in zip(aimed, origin))
        for _ in range(weapon.rounds):  # bounded: at most one magazine per call
            if now < self.next_attack_at:
                break
            if not self.rounds:
                self.rounds = weapon.rounds  # the reload ended
            self.rounds -= 1
            self.shots += 1
            self.next_attack_at += weapon.shot_seconds if self.rounds else weapon.reload_seconds
            heading = math.atan2(dx, dz) + math.radians(random.gauss(0, aim_error))
            pitch = math.atan2(dy, math.hypot(dx, dz)) + math.radians(random.gauss(0, aim_error))
            for _ in range(weapon.pellets):
                off, turn = math.radians(weapon.spread / 2) * math.sqrt(random.random()), random.uniform(0, math.tau)
                h, p = heading + off * math.cos(turn), pitch + off * math.sin(turn)
                direction = (math.cos(p) * math.sin(h), math.sin(p), math.cos(p) * math.cos(h))
                if flight is not None:
                    self.in_flight.append([now + flight, origin, direction, now, 0.0])
                    continue
                hit = hitbox.ray(volumes, origin, direction, weapon.reach)
                if hit is not None and (blocked is None or not blocked(origin,
                        tuple(o + d * hit[0] for o, d in zip(origin, direction)))):
                    hits.append(hit[0])
        return hits

    def _land(self, now, volumes, blocked=None):
        """Sweep pending pellets through blockers, then return hits for those that reach their target by now."""
        hits = []
        for shot in list(self.in_flight):
            at, origin, direction, start, checked = shot
            distance = max(0.0, min(now, at) - start) * self.weapon.speed
            if blocked is not None and blocked(
                    tuple(o + d * checked for o, d in zip(origin, direction)),
                    tuple(o + d * distance for o, d in zip(origin, direction))):
                self.in_flight.remove(shot)
                continue
            shot[4] = distance
            if now < at:
                continue
            self.in_flight.remove(shot)
            hit = hitbox.ray(volumes, shot[1], shot[2], self.weapon.reach)
            if hit is not None:
                hits.append(hit[0])
        return hits

    def _face(self, position, seen, reach):
        """Turn a living shooter to an enemy at position in sight and reach; otherwise drop its cadence."""
        if self.behaviour != 'shoot' or not self.health:
            return False
        dx, dz = position[0] - self.position[0], position[2] - self.position[2]
        if not seen or not 0 < math.hypot(dx, dz) <= reach:
            self.next_attack_at = None
            self.pitch = 0.0
            return False
        self.yaw = math.degrees(math.atan2(dx, dz))
        eye = self.position[1] + eye_height(self.body)
        self.pitch = math.degrees(math.atan2(eye - chest(position)[1], math.hypot(dx, dz)))
        return True

    def patrol(self, now):
        """Walk the waypoints: patrols loop their hint points as retail's did (_stride); shooters walk their route once at a
        constant pace, facing the way they go (a hero bot in a fight keeps facing its target), Range shooters holding
        while an enemy is in reach (the match steers hero bots, strafing them in a fight)."""
        self.held = (0, 0)
        if self.knocked is not None:
            velocity, until, moved_at = self.knocked
            seconds = max(0.0, min(now, until) - moved_at)
            self.position = tuple(p + v * seconds for p, v in zip(self.position, velocity))
            self.velocity = velocity if now < until else (0.0, 0.0, 0.0)
            self.knocked = (velocity, until, now) if now < until else None
            self.moved_at = None
            return
        looping = self.behaviour == 'patrol'
        walking = looping or (self.behaviour == 'shoot' and self.waypoints
                              and (self.next_attack_at is None or self.hero))
        if not walking or not self.health:
            self.velocity, self.moved_at = (0.0, 0.0, 0.0), None
            return
        dt = 0.0 if self.moved_at is None else now - self.moved_at
        self.moved_at = now
        if not looping:
            self._walk(dt, False, now)
            return
        steps = min(max(1, math.ceil(dt / PATROL_TICK - 1e-9)), 625)  # in 16 ms ticks, as retail's bots moved (10 s at most)
        for _ in range(steps):
            self._walk(dt / steps, True, now)

    def _walk(self, dt, looping, now):
        self.held = (0, 0)
        budget = self._stride(dt) if looping else dt * self.speed
        for _ in range(len(self.waypoints)):  # bounded: legs can have zero length
            target = self.waypoints[self.leg]
            remaining = math.dist(target, self.position)
            if remaining > budget + 1e-9:  # float: a stride onto a stop lands on it
                break
            budget = max(0.0, budget - remaining)
            self.position = target
            if not looping and self.leg == len(self.waypoints) - 1:
                break
            self.leg = (self.leg + 1) % len(self.waypoints)
        target = self.waypoints[self.leg]
        remaining = math.dist(target, self.position)
        if not remaining:
            self.velocity = (0.0, 0.0, 0.0)
            return
        direction = [(t - p) / remaining for t, p in zip(target, self.position)]
        step = min(budget, remaining)
        self.position = tuple(p + d * step for p, d in zip(self.position, direction))
        self.velocity = tuple(d * (self.pace if looping else self.speed) for d in direction)
        if looping:
            if self.keys:
                self.heading = math.degrees(math.atan2(direction[0], direction[2]))
            if self.keys or self.turning > 0:
                self.yaw = _turned(self.yaw, self.heading, TURN_RATE * dt)
                self.turning -= 0 if self.keys else dt
        elif not (self.hero and self.next_attack_at is not None):  # a fighting hero bot keeps facing its target
            self.yaw = math.degrees(math.atan2(direction[0], direction[2]))
        if self.keys if looping else step:
            self.held = self.last_held = _throttles(self.yaw, direction)
            self.held_at = now

    def _stride(self, dt):
        """How far a patrol walks in dt: it stands FIRST_WAIT, then walks stop to stop with its keys held, up to its
        speed at its acceleration, lets go where its friction stops it on the stop, and walks on WALK_GAP after."""
        if not self.keys:
            self.wait -= dt
            if self.pace <= 0 and self.wait <= 0:
                for _ in self.waypoints:  # stops it stands on need no walk
                    if self._to_stop() > ARRIVED:
                        break
                    self._pass_stop()
                self.keys, self.turning = True, 0.0
        remaining = self._to_stop()
        if not self.keys:
            if self.pace <= 0:
                return 0.0
            self.pace = max(0.0, self.pace - self.brake * dt)
            stride = self.pace * dt
            if stride >= remaining:  # onto the stop: patrol's loop passes it
                self.pace, stride = 0.0, remaining
            elif not self.pace:  # its friction stopped it short (retail's stopped 0.05-0.45 m short)
                self._pass_stop()
            return stride
        self.pace = min(self.speed, self.pace + self.accel * dt)
        stride = self.pace * dt
        slide = _slide_length(self.pace, self.brake, dt)
        if remaining - stride <= slide:  # its last tick with keys: its friction slides it onto the stop
            stride = max(0.0, remaining - slide)
            self.keys, self.wait, self.turning = False, WALK_GAP, TURN_AFTER
            if stride >= remaining:
                self.pace = 0.0
        return stride

    def _to_stop(self):
        """Metres along the route to the next stop."""
        at, total = self.position, 0.0
        for k in range(len(self.waypoints)):
            point = self.waypoints[(self.leg + k) % len(self.waypoints)]
            total += math.dist(at, point)
            if point in self.stops:
                break
            at = point
        return total

    def _pass_stop(self):
        """Route on from the next stop: the next walk goes to the one after."""
        for k in range(len(self.waypoints)):
            if self.waypoints[(self.leg + k) % len(self.waypoints)] in self.stops:
                self.leg = (self.leg + k + 1) % len(self.waypoints)
                return

    def rebuilding(self, now):
        """A revived Range bot, alive, still within its reassembly clip."""
        return self.revived_at is not None and self.health > 0 and now < self.revived_at + REBUILD_SECONDS

    def tick(self, now):
        """respawn_seconds after a death the bot is back at full health. The dead body stays until then: the client
        plays the death ragdoll itself on a body at 0 health, and only on a body it still has. A Range bot comes back as the
        same body where it fell, ('revive', id), which the client stands back up; a hero bot as a new body at
        home, ('respawn', old id)."""
        if self.dead_at is None or now < self.dead_at + self.respawn_seconds:
            return None
        self.health = self.max_health
        self.damage_by.clear()
        self.dead_at = None
        self.next_attack_at, self.rounds, self.in_flight, self.pitch = None, self.weapon.rounds, [], 0.0
        self.ability_ready = self.effect_until = self.soak = 0.0
        self.hacked_until = 0.0
        self.velocity = (0.0, 0.0, 0.0)
        self.moved_at = self.knocked = None
        self.pace, self.wait, self.keys, self.turning, self.held = 0.0, FIRST_WAIT, False, 0.0, (0, 0)
        if not self.hero:
            self.revived_at = now
            return 'revive', self.entity
        old = self.entity
        self.entity = (self.entity & ~0xFF) | (self.entity & 0xFF) % 255 + 1
        self.position, self.yaw = self.home, self.home_yaw
        self.poses.clear()  # a new body: no view of it is older than now
        self.leg, self.waypoints, self.goal = 0, (), None  # the match re-routes it
        return 'respawn', old
