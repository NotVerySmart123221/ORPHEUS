"""Soldier's build-104319 weapon initialization; graph evidence is in research/."""
from collections import deque
from dataclasses import dataclass
import math

from ow174.game.statescript import (Asset, Bool, Float, Graph, Instance, Int, Entity, StateBound, owner_full_frame)
from ow174.game.bits import BitWriter
from target import INFINITE, SOLDIER_RIFLE

# Owner states and instance-presence order from the pinned graph596 extraction.
RIFLE = Graph(596, 81, 23, 49)
CAPACITY = 30
RELOAD = 0x400
MAX_UPDATES = 128
# shortcut: falloff and headshot scale from the Overwatch Wiki (Sept 2022 revision): 20 damage to 30 m, 6 at
# 50 m, headshots x2; graph596's damage expression terms are not traced.
RIFLE_DAMAGE, RIFLE_MIN_DAMAGE, RIFLE_FALLOFF = 20, 6, (30.0, 50.0)
CRITICAL_SCALE = 2


# hero -> ((stimulus, key) of its HrSlct-EntrMtch line (Genji has none: his one 0x2B line, FlvrLns), (stimulus, key) of
# its first Rspwn line), from tools/assets/voice_lines.py (build 104319) on each hero's spawn body's VoiceSet. Mech and
# pilot share D.Va's. Soldier's and D.Va's were heard in game.
SPAWN_VOICES = {
    0x02E000000000013B: ((0x2B, 0x858C), (0x2C, 0x8546)),  # Ana
    0x02E0000000000200: ((0x2B, 0xBC66), (0x2C, 0xBA1D)),  # Ashe
    0x02E0000000000221: ((0x2B, 0xC96C), (0x2C, 0xC56E)),  # Baptiste
    0x02E0000000000015: ((0x2B, 0x0696), (0x2C, 0x62B0)),  # Bastion
    0x02E0000000000195: ((0x2B, 0xA881), (0x2C, 0xA870)),  # Brigitte
    0x02E0000000000042: ((0x2B, 0x664F), (0x2C, 0x666C)),  # Cassidy
    0x02E000000000007A: ((0x2B, 0x776D), (0x2C, 0x7765)),  # D.Va
    0x02E000000000012F: ((0x2B, 0x9F68), (0x2C, 0x9E17)),  # Doomfist
    0x02E0000000000206: ((0x2B, 0xD9AD), (0x2C, 0xD989)),  # Echo
    0x02E0000000000029: ((0x2B, 0x7953), (0x2C, 0x7833)),  # Genji
    0x02E0000000000005: ((0x2B, 0x5DB6), (0x2C, 0x5D27)),  # Hanzo
    0x02E0000000000065: ((0x2B, 0x75C4), (0x2C, 0x757E)),  # Junkrat
    0x02E0000000000079: ((0x2B, 0x713B), (0x2C, 0x7102)),  # Lucio
    0x02E00000000000DD: ((0x2B, 0x7883), (0x2C, 0x7802)),  # Mei
    0x02E0000000000004: ((0x2B, 0x5C97), (0x2C, 0x5C09)),  # Mercy
    0x02E00000000001A2: ((0x2B, 0xA7D4), (0x2C, 0xA2AC)),  # Moira
    0x02E000000000013E: ((0x2B, 0x8F80), (0x2C, 0x8FA9)),  # Orisa
    0x02E0000000000008: ((0x2B, 0x5B78), (0x2C, 0x5AE9)),  # Pharah
    0x02E0000000000002: ((0x2B, 0x581B), (0x2C, 0x578C)),  # Reaper
    0x02E0000000000007: ((0x2B, 0x05B2), (0x2C, 0x5F65)),  # Reinhardt
    0x02E0000000000040: ((0x2B, 0x7626), (0x2C, 0x753D)),  # Roadhog
    0x02E000000000023B: ((0x2B, 0xCE17), (0x2C, 0xCE13)),  # Sigma
    0x02E000000000006E: ((0x2B, 0x6F9C), (0x2C, 0x7036)),  # Soldier
    0x02E000000000012E: ((0x2B, 0x8743), (0x2C, 0x87C5)),  # Sombra
    0x02E0000000000016: ((0x2B, 0x6478), (0x2C, 0x63CF)),  # Symmetra
    0x02E0000000000006: ((0x2B, 0x5ED5), (0x2C, 0x067B)),  # Torbjorn
    0x02E0000000000003: ((0x2B, 0x5A59), (0x2C, 0x59CA)),  # Tracer
    0x02E000000000000A: ((0x2B, 0x593A), (0x2C, 0x58AB)),  # Widowmaker
    0x02E0000000000009: ((0x2B, 0x6113), (0x2C, 0x6084)),  # Winston
    0x02E00000000001CA: ((0x2B, 0xB69E), (0x2C, 0xB6EA)),  # WreckingBall
    0x02E0000000000068: ((0x2B, 0x6560), (0x2C, 0x656C)),  # Zarya
    0x02E0000000000020: ((0x2B, 0x6240), (0x2C, 0x619F)),  # Zenyatta
}


# D.Va's ability lines (tools/assets/voice_lines.py labels Ejct-Actvt, GmOvr-Actvt-Frndly, CllMch-Actvt)
DVA_EJECT_VOICE, DVA_SELF_DESTRUCT_VOICE, DVA_CALL_MECH_VOICE = (0x497, 0x78E6), (0x4D1, 0x8015), (0x479, 0x77C5)


def voice(body, stimulus, line):
    """The ClientInVoice 27500 that plays a hero voice line, with the body as speaker."""
    return {'+0x78': {'+0x0': body}, '+0x7C': {'+0x0': body}, '+0x80': {'+0x0': body},
            '+0x88': 0x0EE0000000000000 | stimulus, '+0x90': 0x0760000000000000 | line,
            '+0x98': 0, '+0xA0': 0}


def spawn_voice(hero, body, respawn=False):
    """The ClientInVoice 27500 a spawn plays, with the body as speaker; None for a hero without one."""
    return voice(body, *SPAWN_VOICES[hero][respawn]) if hero in SPAWN_VOICES else None


def rifle_damage(distance, critical=False, visor=False):
    """One pulse's damage at distance; Tactical Visor turns off both falloff and headshots."""
    if visor:
        return RIFLE_DAMAGE
    near, far = RIFLE_FALLOFF
    f = min(1.0, max(0.0, (distance - near) / (far - near)))
    return round(RIFLE_DAMAGE + (RIFLE_MIN_DAMAGE - RIFLE_DAMAGE) * f) * (CRITICAL_SCALE if critical else 1)


@dataclass(frozen=True)
class RifleUpdate:
    ammo: int
    serial: int
    active: bool
    count_update: bool
    reloading: bool = False
    reload_phase: str | None = None


class Rifle:
    """The player's primary fire. `weapon` (a `target.HERO_KIT` weapon: shot_seconds, rounds, reload_seconds,
    refill) sets the cadence, magazine and reload; None is Soldier: 76's rifle. As in the weapon graphs, the
    magazine refills `refill` of the way into the reload (Wait var581 * var6884, then var53 = var198) and the
    reload animation runs on to the end (the second Wait); a shot after the refill cuts it short. The refill
    itself sends nothing (an update mid-reload restarts the client's reload Anim)."""
    def __init__(self, quanta, weapon=None):
        if type(quanta) is not int or not 1000 <= quanta <= 1000000:
            raise ValueError('invalid command tick duration')
        weapon = weapon or SOLDIER_RIFLE
        shot, self.capacity, reload = weapon.shot_seconds, weapon.rounds, weapon.reload_seconds
        self.interval = math.ceil(shot * 1000000 / quanta)
        self.reload_duration = math.ceil(reload * 1000000 / quanta)
        self.refill_after = math.ceil(reload * weapon.refill * 1000000 / quanta)
        self.reload_phases = (tuple(math.ceil(seconds * 1000000 / quanta) for seconds in weapon.reload_phases)
                              if weapon.reload_phases else None)
        self.reload_phase = None
        self.recharge = math.ceil(weapon.recharge * 1000000 / quanta) if weapon.recharge else None
        self.recharge_at = None
        self.ammo = self.capacity
        self.serial = 0
        self.next_fire = 0
        self.reload_until = None
        self.refill_at = None
        self.updates = deque()
        self.last_frame = None
        self.firing = False  # the Volley stays on through a held burst, like the native automatic fire

    def refill(self):
        """Full magazine now, ending any reload (Tactical Visor's game message 0x7D sets var53 = var198)."""
        if len(self.updates) >= MAX_UPDATES:
            return
        self.ammo, self.reload_until, self.refill_at = self.capacity, None, None
        self.reload_phase = None
        self.updates.append(RifleUpdate(self.ammo, self.serial, self.firing, True))

    def _start_reload(self, frame):
        if self.recharge:  # a recharging weapon has no reload
            return
        self.reload_until, self.refill_at = frame + self.reload_duration, frame + self.refill_after
        if self.reload_phases:
            opening, insertion, closing = self.reload_phases
            self.reload_phase = 'open'
            self.refill_at = frame + opening
            self.reload_until = self.refill_at + (self.capacity - self.ammo - 1) * insertion + closing
        self._reload_update()

    def _reload_update(self):
        self.updates.append(RifleUpdate(self.ammo, self.serial, False, True, True, self.reload_phase))

    def _advance_round_reload(self, frame, buttons):
        _, insertion, closing = self.reload_phases
        while self.refill_at is not None and frame >= self.refill_at and len(self.updates) < MAX_UPDATES:
            self.ammo += 1
            if self.ammo == self.capacity:
                self.reload_phase, self.refill_at = 'close', None
            else:
                self.reload_phase = 'insert'
                self.refill_at += insertion
            self._reload_update()
        if (self.reload_phase == 'insert' and buttons & 1 and self.ammo and frame >= self.next_fire
                and len(self.updates) < MAX_UPDATES):
            self.reload_phase, self.refill_at, self.reload_until = 'close', None, frame + closing
            self._reload_update()

    def _advance_recharge(self, frame, buttons):
        """The graph's passive refill (Doomfist 0x0D0A switch 17): once the shot timer (var173) runs out, Wait 19
        adds a round every `recharge` and replays Anim 20 each time; a shot aborts the Wait."""
        if (self.recharge_at is None and self.ammo < self.capacity and frame >= self.next_fire
                and not (buttons & 1 and self.ammo) and len(self.updates) < MAX_UPDATES):
            # shortcut: the shot interval stands in for var173's 1/var9805 = 0.333 s timer (0.32 s, 1/80 s early)
            self.recharge_at = frame + self.recharge
            self.updates.append(RifleUpdate(self.ammo, self.serial, False, True, True, 'recharge'))
        while self.recharge_at is not None and frame >= self.recharge_at and len(self.updates) < MAX_UPDATES:
            self.ammo += 1
            full = self.ammo == self.capacity
            self.recharge_at = None if full else self.recharge_at + self.recharge
            self.updates.append(RifleUpdate(self.ammo, self.serial, False, True, not full, None if full else 'recharge'))

    def can_queue_shot(self):
        # The last shot needs a second update to start the reload.
        return len(self.updates) <= MAX_UPDATES - 2

    def spend(self, frame, cost):
        """The alternate fire takes `cost` rounds; an emptied magazine reloads as after the last shot."""
        if not self.can_queue_shot():
            return
        self.reload_until = None  # after the refill (kits.AltFire waits for it) a shot cuts the reload animation
        self.ammo = max(0, self.ammo - cost)
        self.updates.append(RifleUpdate(self.ammo, self.serial, self.firing, True))
        if not self.ammo:
            self.firing = False
            self._start_reload(frame)

    def command(self, command):
        frame = command.frame
        if self.last_frame is not None and frame <= self.last_frame:
            if self.last_frame - frame <= 1024:
                return False
            shift = frame - self.last_frame
            self.next_fire += shift
            if self.reload_until is not None:
                self.reload_until += shift
            if self.refill_at is not None:
                self.refill_at += shift
            if self.recharge_at is not None:
                self.recharge_at += shift
        self.last_frame = frame
        if self.recharge:
            self._advance_recharge(frame, command.buttons)
        if self.reload_phase is not None:
            self._advance_round_reload(frame, command.buttons)
        elif self.refill_at is not None and frame >= self.refill_at and len(self.updates) < MAX_UPDATES:
            # No update here: any instance update mid-reload restarted the reload Anim in game (Winston, Wrecking
            # Ball, Soldier). The HUD shows the refill with the next shot or the reload's end.
            self.ammo, self.refill_at = self.capacity, None
        if (self.reload_until is not None and self.refill_at is None and frame >= self.reload_until
                and len(self.updates) < MAX_UPDATES):
            self.reload_until, self.reload_phase = None, None
            self.updates.append(RifleUpdate(self.ammo, self.serial, False, True))
        if (command.buttons & RELOAD and self.ammo < self.capacity and self.reload_until is None
                and len(self.updates) < MAX_UPDATES):
            self._start_reload(frame)
            self.firing = False
        if (not command.buttons & 1 or self.refill_at is not None or not self.ammo
                or self.reload_phase is not None
                or frame < self.next_fire or not self.can_queue_shot() or self.serial == 65535):
            if self.firing and not command.buttons & 1 and len(self.updates) < MAX_UPDATES:
                self.firing = False
                self.updates.append(RifleUpdate(self.ammo, self.serial, False, True))
            return False
        self.reload_until = self.recharge_at = None  # a shot cuts the reload animation and aborts a recharge Wait
        if self.capacity != INFINITE:
            self.ammo -= 1
        self.serial += 1
        self.next_fire = frame + self.interval
        self.updates.append(RifleUpdate(self.ammo, self.serial, True, True))
        self.firing = bool(self.ammo)
        if not self.ammo:
            self._start_reload(frame)
        return True


def volley_payload(update):
    if not 0 <= update.serial <= 65535:
        raise ValueError('volley serial exceeds native compact integer')
    out = BitWriter()
    out.bit(update.active)
    out.bits(0, 10)  # native signed time delta zero
    out.bit(update.count_update)
    if update.count_update:
        out.bits(0, 10)
        out.w_u16(update.serial)
    out.bit(0)  # optional multiplier defaults to1
    out.bits(update.serial & 63, 6)  # native serializer sends the state's event counter
    return out


def muzzle_payload(active, serial):
    out = BitWriter()
    out.bit(active)
    out.bits(serial & 3, 2)  # native effect DSR wraps its event counter to two bits
    return out


def weapon_instance(ammo=CAPACITY, update=None, extra_active=None, weapon_entity=None, *, fire_elapsed=None):
    if type(ammo) is not int or not 0 <= ammo <= CAPACITY:
        raise ValueError('invalid rifle ammo')
    selected, hidden = BitWriter(), BitWriter()
    selected.bit(1)
    hidden.bit(0)
    active = {0: selected, 11: None, 12: hidden}
    if extra_active:
        active.update(extra_active)
    if 8 in active:
        # LogicalButton helper: keep its presence and explicitly stop input while firing is blocked.
        active[8] = _bits(ammo > 0 and (update is None or not update.reloading), 5)
    if update is not None:
        active[10] = volley_payload(update)
        if 21 in active:
            active[21] = muzzle_payload(update.active, update.serial)
    presence = {4: Int(1), 8: Int(ammo), 12: Float(1.5),
        13: Int(CAPACITY), 15: Float(20), 17: Int(0), 18: Float(0),
        20: Float(100), 21: Float(100)}
    if fire_elapsed is not None:
        # Volley.onBegin starts Chase19; Mapping18 feeds its live output to first-person parameter F7.
        presence[20] = StateBound(Float(100), 1, 19, 7, per_frame=True)
        active[19] = chase_since(fire_elapsed)
    if extra_active and 31 in extra_active:  # Helix's StateAbility: var61 (bit 10) and var472 (bit 14)
        presence.update({10: cooldown_left(1, 31, 7), 14: Float(HELIX_COOLDOWN)})
    if extra_active and 23 in extra_active:
        firing = update is not None and update.active
        presence[6] = Bool(firing)  # instance689; the nested Stack72 writes entity689.
        for bit in (23, 68):
            if firing:
                stack = BitWriter()
                stack.bits(1, 2)
                active[bit] = stack
            else:
                active.pop(bit, None)
    extra = {}
    if extra_active and 39 in extra_active:
        reloading = update is not None and update.reloading
        extra[51] = Bool(reloading)
        if reloading:
            animation = BitWriter()
            animation.bit(0)  # native linked-state reference absent
            animation.bits(update.serial & 7, 3)
            animation.bit(0)  # optional time override absent
            active[39] = animation
        else:
            active.pop(39, None)
    if weapon_entity is not None:
        if isinstance(weapon_entity, StateBound) and isinstance(weapon_entity.value, Entity):
            presence[7] = weapon_entity
        elif type(weapon_entity) is not int or not 0 < weapon_entity <= 0xFFFFFFFF:
            raise ValueError('invalid weapon entity')
        else:
            presence[7] = Entity(weapon_entity)
    return Instance(1, RIFLE, presence=presence, extra=extra, active=active)


def body_frame(cmfd, update=None, extra_active=None, weapon_entity=None,
               *, extra_instances=(), extra_variables=None, serialize=None, fire_elapsed=None):
    ammo = update.ammo if update is not None else CAPACITY
    variables = {31: Int(1), 34: Bool(False), 478: Bool(False), 7184: Int(ammo), 7185: Int(CAPACITY)}
    if extra_variables:
        variables.update(extra_variables)
    if extra_active and 23 in extra_active:
        variables[689] = Bool(update is not None and update.active)
    return (serialize or owner_full_frame)(cmfd,
        [weapon_instance(ammo, update, extra_active, weapon_entity, fire_elapsed=fire_elapsed), *extra_instances], variables)


def ability_payload(flags=0, cooldown=None):
    """StateAbility: the owner prelude first reflects its mirror's one unskipped field (u8 flags at
    state+0x270; bit0 in use, bit1 ended, bit2 one-shot), then DSR 0x7FF6D1EA4AC0 reads a linked-state
    bit and a timing-block bit. The block (cooldown = (seconds, elapsed ms)) is an f32 duration, a rate
    bit (absent: 1.0) and the start's offset back from now; output 0 (0x7FF6D1EA4D00) is then the
    seconds left, which var61 is bound to for the HUD's cooldown sweep."""
    out = BitWriter()
    out.bits(flags, 8)
    out.bit(0)  # no linked state
    out.bit(cooldown is not None)
    if cooldown is not None:
        out.f32(cooldown[0])
        out.bit(0)
        out.w_var(cooldown[1])
    return out


def anim_payload(serial=0):
    """StateAnim (DSR 0x7FF6D1EDD370): no mirror; the prelude's linked-state bit (0: none), 3 bits (state+0x44; the
    client sets 1 on its own anims, meaning not traced; the rifle's reload sends its shot serial), then a bit for
    an optional value (0: none)."""
    out = BitWriter()
    out.bit(0)
    out.bits(serial & 7, 3)
    out.bit(0)
    return out


MELEE_BUTTON = 0x800  # V in the command record (logical button 31)
MELEE = Graph(67, 27, 7, 14)  # every body's quick melee graph (research/melee.md)
MELEE_DAMAGE, MELEE_COOLDOWN = 30, 1.0  # graph 67's Entry sets var476 = 30; the wiki's 1 s cooldown


def melee_instance(melee, update=None):
    """Graph 67's instance: var476 (damage), var61 bound to the StateAbility (state 6, owner 5, logical button 31)
    with the cooldown's timing; with a press's update the swing: switch 6 false, Wait 7 and the Anims 1 and 11.
    shortcut: TrackTargets 4 and state 3 on the way (undecoded payloads) are left off."""
    off = BitWriter()
    off.bit(0)
    active = {5: melee.ability}
    if update is not None:
        active.update({6: off, 7: None, 1: anim_payload(update.serial), 11: anim_payload(update.serial)})
    return Instance(67, MELEE, presence={5: Int(MELEE_DAMAGE), 6: cooldown_left(67, 6, 5)}, active=active)


def cooldown_left(instance, state, state_bits):
    """var61: bound to the StateAbility's output 0 (seconds left); 0 while the state is off."""
    return StateBound(Float(0.0), instance, state, state_bits, per_frame=True)


SPRINT_BUTTON = 0x08  # Shift in the command record (logical button 35)
SPRINT_BONUS = 0.5  # graph597 node3 sets var1002, the speed modifier's amount
SPRINT_CANCEL = 0x01 | 0x02 | 0x80  # fire, secondary fire, crouch
# Stack 5 (node 13) writes entity var1273, the first-person animation category the animator reads
ANIM_CATEGORY, SPRINT_CATEGORY = 1273, 0x028000000000017D


class Sprint:
    """Soldier's Sprint, run by the server: the client does not start graph597 itself.
    shortcut: held Shift + forward input only; graph597's toggle option (button 31) and its
    0.3 s re-sprint delay (var636) are not modelled."""

    def __init__(self, quanta=7000):
        self.active = False
        self.count = 0  # activations, for the effect's event counter
        self.quanta = quanta / 1e6
        self.animation = None
        self.animation_left = 0.0
        self.last_frame = None

    def command(self, command):
        """Update sprint and the authored enter/exit animations from command time."""
        changed = False
        if self.animation is not None and self.last_frame is not None:
            self.animation_left -= max(0, command.frame - self.last_frame) * self.quanta
            if self.animation_left <= 0 or command.buttons & SPRINT_CANCEL:
                self.animation = None
                changed = True
        self.last_frame = command.frame
        active = bool(command.buttons & SPRINT_BUTTON) and command.forward > 0 \
            and not command.buttons & SPRINT_CANCEL
        if active == self.active:
            return changed
        self.active = active
        self.count += active
        if not command.buttons & SPRINT_CANCEL:
            self.animation = 15 if active else 14
            # Graph597 aliases 0306/0307: retail first-person clips 1E77/1E78.
            self.animation_left = 16 / 30 if active else 20 / 30
        return True


def _bits(value, width):
    out = BitWriter()
    out.bits(value, width)
    return out


def sprint_instance(sprint=None):
    """Graph597 (Sprint): the HUD presenter, switch 35 on its false branch and the StateAbility. While
    sprinting, the ability's activation subgraph: speed modifier (owner bit 0, var1002), anim category
    Stack 5, effect 9, Stacks 13/19/27/29 and the second modifier 20."""
    states = {28: None, 18: _bits(0, 1), 1: ability_payload(1 if sprint and sprint.active else 0)}
    presence = {5: Float(SPRINT_BONUS if sprint and sprint.active else 0)}
    if sprint and sprint.active:
        states.update({0: None, 20: None, 9: muzzle_payload(True, sprint.count),
            **{bit: _bits(1, 2) for bit in (5, 13, 19, 27, 29)}})
    if sprint and sprint.animation is not None:
        states[sprint.animation] = anim_payload(sprint.count)
    return Instance(597, Graph(597, 31, 9), presence=presence, active=states)


HELIX_BUTTON = 0x02  # right mouse in the command record (logical button 29)
HELIX_COOLDOWN = 6.0  # graph596 Entry node 123: var472's cooldown struct
HELIX_SPEED = 50.0  # graph596 Volley node 38 projectileSpeed; lifetime 5 s
HELIX_RANGE = HELIX_SPEED * 5
HELIX_IMPACT = 40  # Volley 38's direct-hit modifyHealth amount
HELIX_SPLASH = 80  # its explosion amount: full within 1 m, half at 3 m, nothing beyond
HELIX_RINGS = ((1.0, 1.0), (3.0, 0.5))
# Native0x072C has model0x1883; other viewers retain the child0x072E fallback.
HELIX_PROJECTILE = 0x040000000000072E
# Component 90 records (world "reservation"): the client only creates a predicted Helix rocket under an
# ID block the server reserved for that definition (research/soldier-runtime/helix-visual-findings.md).
HELIX_DEFINITIONS = (0x040000000000072C, 0x0400000000001C86)  # var842 picks one
HELIX_ID_STEP = 0x07000000  # 0x1C86's IDs sit this far above 0x072C's
HELIX_ROCKET_SECONDS = 5.0  # Volley 38's projectile lifetime
HELIX_RESERVE_DELAY = 0.5  # the client creates its rocket 0-6 frames after the press: re-reserve after that


def helix_reservations(rocket):
    """Component 90 records giving the next rocket of each definition the ID 0xA0000000 | rocket (| + step).
    IDs per tick 0: the allocator then issues the same ID on every tick, so the server knows it and confirms
    it with a create; the client's second prediction (the next tick) is refused, that ID existing already.
    shortcut: rocket IDs count up from 0x01000000 and must stay below 0x08000000 (then 0x1C86's below
    0x10000000, the bots' and shots' IDs): fine for 117M shots."""
    return [(definition, 0, 0xFFFFFFFF, rocket + k * HELIX_ID_STEP, 0) for k, definition in enumerate(HELIX_DEFINITIONS)]


def helix_splash(distance):
    """Explosion damage at distance from the impact, interpolating the authored rings.
    shortcut: linear between rings; the native falloff curve between them is not traced."""
    (r0, s0), (r1, s1) = HELIX_RINGS
    if distance <= r0:
        return HELIX_SPLASH
    if distance > r1:
        return 0
    return round(HELIX_SPLASH * (s0 + (s1 - s0) * (distance - r0) / (r1 - r0)))


class Volley:
    """An ability volley on a cooldown, run by the server like the rifle: a press queues the volley's
    start and stop updates (Helix Rockets, Biotic Field)."""

    def __init__(self, quanta, button=HELIX_BUTTON, seconds=HELIX_COOLDOWN):
        self.button, self.quanta, self.seconds = button, quanta, seconds
        self.cooldown = math.ceil(seconds * 1000000 / quanta)
        self.ready_at = 0
        self.frame = 0  # the newest command frame
        self.serial = 0
        self.updates = deque()
        self.choosing, self.pick = False, None  # kits.Choice: armed, waiting for its pick; the pick that fired it

    def ability(self):
        """Its StateAbility payload now: with the cooldown's timing block while it runs."""
        left = self.ready_at - self.frame
        if left <= 0 or left > self.cooldown:
            return ability_payload()
        return ability_payload(0, (self.seconds, (self.cooldown - left) * self.quanta // 1000))

    def showing_cooldown(self):
        """Its cooldown runs, or ended within the last second (the HUD needs one frame after it)."""
        return 0 < self.ready_at and self.frame <= self.ready_at + math.ceil(1000000 / self.quanta)

    def command(self, command):
        self.frame = command.frame
        if not command.buttons & self.button or command.frame < self.ready_at or self.serial == 65535:
            return False
        self.ready_at = command.frame + self.cooldown
        self.serial += 1
        self.updates.append(RifleUpdate(0, self.serial, True, True))
        self.updates.append(RifleUpdate(0, self.serial, False, True))
        return True


def helix_states(update=None, volley=None):
    """Graph596's Helix route: switch 70 (bit 35) false -> LogicalButton 36 (bit 14, button 29) and the
    cooldown StateAbility 65 (bit 31); when firing, switch 37 (bit 15) false -> Stack 64 (bit 30) ->
    Volley 38 (bit 16)."""
    button = _bits(1, 1)  # LogicalButton: Begin 1, then a four-bit counter 0
    button.bits(0, 4)
    states = {35: _bits(0, 1), 14: button, 31: volley.ability if volley else ability_payload()}
    if update is not None:
        states.update({15: _bits(0, 1), 30: _bits(1, 2), 16: volley_payload(update)})
    return states


ULT_COST_VAR, ULT_COST = 338, 1650  # graph75 copies its var338 to the entity: Tactical Visor's cost
# shortcut: the wiki (Sept 2022) gives 2310 damage/healing points to fill it; var342 counts in var338's units.
ULT_POINTS = 2310


def ult_meter(points, most=ULT_POINTS):
    """Entity var342 for points of ultimate charge, of `most` to fill it (sent in var338's units)."""
    return Int(int(points * ULT_COST // most))
ULT_CHARGE = 342  # entity Int; graph601's presenter and ready switch compare it with var338


def health_variables(health, most):
    """Graph463's player card binds its CurrentHP text to the expression `478 or 33688 ? 0 : 1737 == 1732 ?
    1737 : min(int(1732), 1737 - 1)` over its remote-sync vars, so 1732 is the current health and 1737 its
    maximum (the bar itself is native, from the health component)."""
    return {1732: Float(health), 1737: Float(most)}

FIELD_BUTTON = 0x10  # E in the command record (logical button 36)
FIELD_COOLDOWN = 15.0  # graph599: var472's cooldown struct
# Impact03DC's graph600 creates canister068A and the native floor-ring effect0B06.
FIELD_ENTITY = 0x04000000000003DC
# shortcut: the wiki's (Sept 2022) 35 health per second within 4.5 m for 5 s, as 7 every 0.2 s; graph600 has no
# heal action, and its Ints (5, 5, 20) and 0.5 s Wait are not traced to these roles.
FIELD_RADIUS, FIELD_SECONDS, FIELD_HEAL, FIELD_PERIOD = 4.5, 5.0, 7, 0.2
FIELD_RELEASE = 0.4  # graph599 Wait 8: Volley 4 throws the canister this long after the press (Hero Info clip: 0.4 s)


def field_visual_frame(cmfd):
    # State9's material3398 renders a checker texture here;2/3 preserve the authored ring and canister.
    return owner_full_frame(cmfd, [Instance(1, Graph(600, 11, 1, 5),
        presence={0: StateBound(Entity(0), 1, 3, 4)},
        active={2: muzzle_payload(True, 1), 3: None})], {})


def field_instance(update=None, volley=None):
    """Graph599 (Biotic Field): the HUD presenter and the StateAbility; when thrown, its Waits 6 and 8
    (bits 3, 4), the throw Anim 5 (bit 2) and Volley 4 (bit 1). Presence bits 3 and 5 are var472 (the cooldown) and var61."""
    states = {16: None, 0: volley.ability if volley else ability_payload()}
    if update is not None:  # Wait 6's subgraph holds the throw's StateAnim 5 (bit 2)
        states.update({3: None, 2: anim_payload(update.serial), 4: None, 1: volley_payload(update)})
    return Instance(599, Graph(599, 21, 6), presence={3: Float(FIELD_COOLDOWN), 5: cooldown_left(599, 0, 5)},
        active=states)


VISOR_BUTTON = 0x20  # Q in the command record (logical button 37)
# Graph601 Wait 91's timeout: the cast before ChaseVar 8 starts the timer; Anim 3 (the hand-to-visor gesture,
# alias 0x0C80000000000029 -> Soldier 1P clip 0x0A00000000001C07) lasts as long (research/hero-info/soldier76.md).
VISOR_CAST = 1.2
# shortcut: OW1's published 6 s; the start value of graph601's timer (var215) is not traced.
VISOR_SECONDS = 6.0
VISOR_CONE = math.cos(math.radians(45))  # shortcut: lock-on cone estimated, not read from TrackTargets 11/36


def chase_payload(value):
    """ChaseVar (DSR 0x7FF6D2DB8400, no mirror prelude): vector, rate and third flag bits, the f32 value
    (three when a vector), the 16/32-bit rate when flagged, then a timing bit (tick offset when set)."""
    out = BitWriter()
    out.bits(0, 3)
    out.f32(value)
    out.bit(0)
    return out


def chase_since(elapsed):
    """A ChaseVar from 0 begun `elapsed` ms ago (the timing bit set): the client runs its output on from there."""
    if type(elapsed) is not int or not 0 <= elapsed < 1 << 31:
        raise ValueError('invalid ChaseVar elapsed time')
    out = BitWriter()
    out.bits(0, 3)
    out.f32(0)
    out.bit(1)
    out.w_var(elapsed)
    return out


def visor_instance(phase=None, uses=0):
    """Graph601 (Tactical Visor), by owner state: the HUD presenter 32; while on, the ability 0, Stacks 9/34
    (var842, var699), B5C87F3D 14, switch 33 true with effect 3 and BD02E168 42 (parent of the client-only
    presenter), and switch 40 false (var1956). phase 'cast' adds Wait 41 with its Stack 2 (var33, the fire
    button's action priority) and the hand-to-visor Anim 1; 'active' replaces them with ChaseVar 6 (timer var215).
    uses: visors started on this body, effect 3's counter. Effect 3 plays 0x0300000000000ADE, the screen tint.
    shortcut: its TrackTargets, DataFlowMappings, looping Anim 5 and effect 10 are not sent."""
    states = {32: None}
    if phase:
        states.update({0: ability_payload(1), 9: _bits(1, 2), 34: _bits(1, 2), 14: None, 33: _bits(1, 1),
            3: muzzle_payload(True, uses), 42: None, 40: _bits(0, 1)})
        if phase == 'cast':
            states.update({41: None, 2: _bits(1, 2), 1: anim_payload(1)})
        else:
            states[6] = chase_payload(VISOR_SECONDS)
    elif uses:  # an effect left out keeps playing (kits.ability_instance): stop the tint with its Begin bit cleared
        states[3] = muzzle_payload(False, uses)
    return Instance(601, Graph(601, 44, 7), active=states)


def ability_instances(sprint=None, field=None, visor=None, field_volley=None, visor_uses=0):
    """Sprint (597), Biotic Field (599) and Tactical Visor (601)."""
    return [sprint_instance(sprint), field_instance(field, field_volley), visor_instance(visor, visor_uses)]


def initialize_practice_soldier(player):
    """Install the native-verified rifle, body hosts and shared health/ammo HUD."""
    selected, hidden, button, stack = BitWriter(), BitWriter(), BitWriter(), BitWriter()
    selected.bit(1)
    hidden.bit(0)
    button.bit(1)
    button.bits(0, 4)
    stack.bits(1, 2)
    states = {state: None for state in (6, 9, 17, 18, 22, 23, 33, 34, 39, 57, 65, 66, 68, 79)}
    states.update({1: hidden, 2: hidden, 3: button, 8: button, 13: hidden, 21: muzzle_payload(False, 0),
        29: stack, 44: hidden, 60: stack, 63: hidden, 72: selected})
    player.melee = Volley(player.quanta, MELEE_BUTTON, MELEE_COOLDOWN)
    player.cooldowns = [player.melee]  # a previous pick's kits.ABILITIES are gone
    player.alt = None
    player.helix = Volley(player.quanta)
    player.field = Volley(player.quanta, FIELD_BUTTON, FIELD_COOLDOWN)
    states.update(helix_states(volley=player.helix))
    player.weapon_extra_states = states
    player.weapon_entity = StateBound(Entity(0), 1, 11, 7)
    player.kit_frame = body_frame
    player.sprint = Sprint(player.quanta)
    player.primary_started = None
    player.primary_firing = False
    base = [
        Instance(21, Graph(21, 35, 1), active={11: hidden, 23: stack, 29: None, 32: None, 33: None}),
        # state 23: the ForCodeValue notification that makes the animator reread var1273 (kits.notify_animation_categories)
        Instance(75, Graph(75, 39, 4), presence={2: Int(1)}, active={19: None, 23: None}),
        Instance(4429, Graph(4429, 1, 0), active={0: None}),
        Instance(463, Graph(463, 41, 22), presence={0: Asset(0x0C00000000007DA7)},
            active={39: None, 40: None}),
    ]
    player.visor_until = player.visor_cast_until = 0.0
    player.visor_uses = 0
    player.body_extra_variables = {ULT_COST_VAR: Int(ULT_COST), ULT_CHARGE: Int(0), 982: Int(1), 3772: Bool(True),
        **health_variables(player.health, player.max_health)}

    def refresh(pulse=None):
        """pulse: Biotic Field's volley update, or (the melee's cooldown, its update) while the swing plays."""
        player.body_extra_instances = [*base, melee_instance(player.melee, pulse[1] if isinstance(pulse, tuple) else None),
            *ability_instances(player.sprint, None if isinstance(pulse, tuple) else pulse,
                               'cast' if player.visor_cast_until else 'active' if player.visor_until else None,
                               player.field, player.visor_uses)]
        # the server-run Stack 5 does not write its output on the client
        player.body_extra_variables[ANIM_CATEGORY] = Asset(SPRINT_CATEGORY if player.sprint and player.sprint.active else 0)
    player.refresh_abilities = refresh
    refresh()
