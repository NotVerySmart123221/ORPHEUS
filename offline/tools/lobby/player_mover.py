"""The player's mover: LizRollin's port of the client's own (vendor ow174/game/mover.py), plus what our kits drive from
outside it, since the server does not run the body's statescript (research/lizrollin-runtime.md section 6).

- Sprint (`speed_scale`, set by match.py): the movement mod the client's statescript puts on, +0.5 to the speed
  scalar, acting from the frame after it starts until the frame it stops in.
- D.Va's Boosters (`flight`, truthy while they run, set by match.py after the frame's step): the client's own
  walking mover with the MovementMod state of graph 0x047E (instance 79), from the frame after the press through
  the frame they end in, flying along the aim (see BOOST_MODS). Her eject (`launch`) throws the body into the air.
- The body frame's MovementMod states (`ability_mods`, set by match.py when it builds the frame): the client puts a
  mod on for every such state the server turns on (Bastion's no-move state, an ability's speed), from the frame's
  command frame + 1, so the server's copy takes the same mods from its next step (`frame_mods`).
- A dash (`dash`, called by match.py on the press): the body is carried a distance along the input, as Tracer's
  Blink does (the action STU_A807207A of graph 0x0024: 7.5 m in 0.1 s). The client does not run that action on a
  kits body, so it follows through the owner's records (owner_correction). The carry is a move on top of each
  step, with the velocity left as it was: a record that carried the dash's speed made the client run on past the
  end (2.4 m, live) before the next record pulled it back.
- The launches, read off each graph's STU_FF3471E9 action and checked against the client's own prediction of them
  on a VM body (research/lizrollin-runtime.md section 25): `jet` (Jump Jet), `leap` (Jump Pack) and `roll` (Combat
  Roll) change the velocity on the press, and the client takes it from the owner's next record. `strike` (Swift
  Strike) is a dash along the aim with the body's own movement held still.
- `lift` (Pharah's Hover Jets, set by match.py with the body frame that has the thrust state on): an upward push
  each step that fades as she rises.
- A flight along the aim (`aimed`, with the mover values the state swaps in, `swapped`; both read off the body frame
  by `frame_flight`): Echo's Flight. The client runs that state itself; the launch that starts it is `fly`.
- `client_sync` moves the feet from outside; the physics engine's body follows them before each tick, and without
  the map's collision the stand-in floor follows grounded feet to their new height.
"""
import math
from dataclasses import fields, replace

from ow174.game import contacts
from ow174.game.collision import MOVER, TEAM0, TEAM1, flat_world
from ow174.game.commands import CROUCH, JUMP
from ow174.game.mover import (ADD, COLLISION_MOD, CROUCH_INTENT, DECEL_AIR, FELL, FLT_MAX, GRAVITY_MOD, IN_AIR,
                              JUMP_INTENT, LAND_LIMIT_MOD, MOD_OPS, MOVEMENT_MOD, ROUNDS, SET, SPEED, TINY, TOUCHING,
                              TURN, Mod, Mover, MoverData, f32, intent, mover_data, quantise)
from ow174.game.script import expr
from ow174.game.script import graph as catalog

SPRINT = ('sprint',)  # the key of Sprint's speed mod
# D.Va's mech (body 0x0400000000000407) is missing from game_movers_174.json. Its STUCharacterMoverComponent
# (tools/assets/stu_graph.py 0x0400000000000407 --class 6EFDBD73) is Soldier: 76's values with a 0.7 m capsule.
MECH = 0x0400000000000407
LOCAL_MOVERS = {MECH: replace(mover_data(None), stand_radius=0.7, crouch_radius=0.7)}
# Boosters' MovementMod (graph 0x047E, instance 79; tools/assets/stu_graph.py 0x058000000000047E):
# - instance 88 (STU_ED2B22BD) swaps in the mover values of instance 89: the mech's, with air acceleration 35 for 3.5;
# - SPEED ADD 1.18 (instance 102, no index field: 0): 5.5 x 2.18 = 11.99 m/s;
# - DECEL_AIR SET 1 at priority 1, character collision SET 0;
# - throttle mods 17 (forward part) SET 1 and 18 (backward part) SET 0, read at 0x7FF6D1E53960: forward is forced.
# The state's flag m_485109CB (read at 0x7FF6D1F59590, behind obfuscated code) flies along the aim: the walking
# turn toward the wish, in 3D (PlayerMover._accelerate). Fitted to ten recorded flights, level, aimed up and down,
# into walls and up slopes (research/lizrollin-runtime.md section 8): within 0.85 m of the client.
# shortcut: no gravity, no landing until the flight ends and the touching acceleration of 35 are fitted, not read;
# read the flag's code to make them exact.
BOOST_VALUES = replace(LOCAL_MOVERS[MECH], accel_air=35.0, accel_air_touching=35.0)
BOOST_MODS = {
    ('boost', 'speed'): (SPEED, ADD, 1.18, 0.0),
    ('boost', 'decel'): (DECEL_AIR, SET, 1.0, 1.0),
    ('boost', 'collide'): (COLLISION_MOD, SET, 0.0, 0.0),
    ('boost', 'gravity'): (GRAVITY_MOD, SET, 0.0, 0.0),
    ('boost', 'land'): (LAND_LIMIT_MOD, SET, -FLT_MAX, 0.0),
}


def body_values(body: int | None):
    """A hero body's mover values: game_movers_174.json, or LOCAL_MOVERS for the bodies it misses."""
    return LOCAL_MOVERS.get(body) or mover_data(body)


# The throttle mods (Mover applies none of them): what the forward, backward and sideways input comes to. All three
# at 0 is the no-move state of Bastion's Sentry and Mei's Cryo-Freeze; Reinhardt's Charge sets forward to 1.
THROTTLE_FORWARD, THROTTLE_BACKWARD, THROTTLE_SIDE = 17, 18, 19
# Mod 32 (Mover applies none): the most the speed may be. Read off Echo's Flight, whose state 24 puts a MINIMUM on it
# with a variable that falls from 30 to 8 in 0.1 s: the client's own flight lost speed along that line while it was
# under the speed she had (research/lizrollin-runtime.md section 26).
SPEED_LIMIT = 32
NO_LANDING = {('aimed', 'land'): (LAND_LIMIT_MOD, SET, -FLT_MAX, 0.0)}
# A MovementMod state's mover swap (the mod STU_ED2B22BD): its STUCharacterMoverComponent's fields, in MoverData's
# order (the values of Echo's Flight against her body's own; the component's last nine fields are not known).
SWAP = 'STU_ED2B22BD'
SWAP_FIELDS = (
    'm_1972C687', 'm_7A70E3E3', 'm_4A576BC2', 'm_03E04168', 'm_915FF368', 'm_D97B3735', 'm_D4AE9D05', 'm_5C6B4602',
    'm_6B7FDB90', 'm_E54438F7', 'm_AEBAD1B0', 'm_08FFAA0A', 'm_CFD3E635', 'm_61D6B4E9', 'm_19452F15', 'm_B470B8DA',
    'm_AA6F4CE1', 'm_0DDD18C3', 'm_DAA0618B', 'm_E631BA84', 'm_D24BF4E6', 'm_E0A0FE25', 'm_6F25AA23', 'm_4E03E892',
    'm_142DA8D4', 'm_031FE7EE', 'm_8895A9C8', 'm_EC289987', 'm_C2C40969', 'm_B74245E8', 'm_975F21A5', 'm_B99EC983',
    'm_CEF4898C')
# Jump Pack's direction, as the client's own leaps came out at five pitches: the aim with 0.1 added upward, its
# elevation kept between 20 and 70 degrees (STU_0762B33A, whose code is not read).
LEAP_LIFT, LEAP_LEAST = 0.1, math.radians(20.0)


def _authored(graph, var: int):
    """The one constant the graph's own SetVar actions give an instance variable; None when they give none or more
    than one. Only for a variable the graph does not sync: the client then sets it itself."""
    found = set()
    for node in graph.nodes:
        out = node.fields.get('m_out_Var') if node is not None and node.cls == 'STUStatescriptActionSetVar' else None
        if isinstance(out, dict) and out.get('m_identifier') == var and not out.get('m_60DB8F99'):
            value = node.fields.get('m_value')
            constant = value.get('m_value') if isinstance(value, dict) else None
            found.add(float(constant) if isinstance(constant, (bool, int, float)) else None)
    return found.pop() if len(found) == 1 else None


class _Sent(expr.Context):
    """What an expression of a frame's instance reads: a variable is what the frame sends for it. Anything else (an
    unsent variable, a world query) has no answer here."""

    def __init__(self, instance, graph) -> None:
        super().__init__()
        self.instance, self.graph = instance, graph

    def var(self, scope: int, var: int):
        bit = self.graph.presence_bit(var) if scope == expr.INSTANCE else None
        sent = self.instance.presence.get(bit) if bit is not None else None
        sent = sent() if callable(sent) else sent
        if not isinstance(getattr(sent, 'value', None), (bool, int, float)):
            raise LookupError(var)
        return sent.value

    def native(self, cfg: dict):
        raise LookupError(cfg.get('$'))


def _number(config, instance, graph):
    """A mod's config var as a number: its constant, or the instance variable it names. A synced variable is what
    the frame sends for it (unsent, the client has none: Stealth's speed, var7779, stayed 0 until the kit sent it);
    an unsynced one is the constant the graph's own SetVar gives it. An expression of sent variables is worked out
    (Exo Boots' jump); None for anything else."""
    if not isinstance(config, dict):
        return None
    if config.get('$') in expr.EXPRESSIONS:
        try:
            value = expr.evaluate(config, _Sent(instance, graph))
        except LookupError:
            return None
        return float(value) if isinstance(value, (bool, int, float)) else None
    if 'm_value' in config and isinstance(config['m_value'], (bool, int, float)):
        return float(config['m_value'])
    if config.get('$') == 'STUConfigVarDynamic' and not config.get('m_60DB8F99'):
        bit = graph.presence_bit(config.get('m_identifier'))
        if bit is None:
            return _authored(graph, config.get('m_identifier'))
        sent = instance.presence.get(bit)
        sent = sent() if callable(sent) else sent
        if isinstance(getattr(sent, 'value', None), (bool, int, float)):
            return float(sent.value)
    return None


def frame_mods(instances) -> dict:
    """The movement mods an owner frame turns on: {('frame', instance, owner slot, n): (index, op, value, priority)} for
    each mod of each active MovementMod state (STU_316CFEF2), from the graph catalog (statescript_mods does this for a VM
    body). A graph the catalog lacks (D.Va's) gives none."""
    found = {}
    for instance in instances:
        graph = catalog.graph(instance.graph.index)
        if graph is None:
            continue
        states = graph.owner_states()
        for slot in instance.active:
            node = states[slot] if slot < len(states) else None
            if node is None or node.cls != MOVEMENT_MOD:
                continue
            for n, mod in enumerate(node.fields.get('m_BC5E91CF') or ()):
                op, empty = MOD_OPS.get(mod.get('$') if isinstance(mod, dict) else None, (None, None))
                if op is None or mod.get('m_A9561CA0') is None:
                    continue
                value = empty if mod.get('m_9B7A63EA') is None else _number(mod['m_9B7A63EA'], instance, graph)
                priority = 0.0 if mod.get('m_priority') is None else _number(mod['m_priority'], instance, graph)
                if value is not None and priority is not None:
                    found[('frame', instance.index, slot, n)] = (int(mod['m_A9561CA0']), op, f32(value), priority)
    return found


def frame_flight(instances) -> tuple:
    """(mover values, aimed) of an owner frame's MovementMod states: the values one of them swaps in (None without a
    swap) and whether one flies along the aim (m_485109CB, the flag of D.Va's Boosters too)."""
    swapped, aimed = None, False
    for instance in instances:
        graph = catalog.graph(instance.graph.index)
        states = graph.owner_states() if graph else ()
        for slot in instance.active:
            node = states[slot] if slot < len(states) else None
            if node is None or node.cls != MOVEMENT_MOD:
                continue
            flag = node.fields.get('m_485109CB')
            aimed = aimed or bool(isinstance(flag, dict) and flag.get('m_value'))
            for mod in node.fields.get('m_BC5E91CF') or ():
                values = mod.get('m_9B7A63EA') if isinstance(mod, dict) and mod.get('$') == SWAP else None
                if isinstance(values, dict):
                    names = [field.name for field in fields(MoverData)]
                    swapped = MoverData(**{name: values[key] for name, key in zip(names, SWAP_FIELDS) if key in values})
    return swapped, aimed


class PlayerMover(Mover):
    def __init__(self, position, yaw: int = 0, world=None, body: int | None = None, team: int | None = None):
        """world: the map's collision (collision.movement_world; None walks on a flat floor at the start height);
        body: the hero body's GUID, for its mover values; team 0 or 1 collides with that team's proxies."""
        super().__init__(position, yaw, world, body_values(body), (TEAM0, TEAM1)[team] if team in (0, 1) else MOVER)
        self.speed_scale = 1.0  # Sprint: 1 + SPRINT_BONUS while it runs
        self.flight = 0.0  # Boosters run while truthy (kits.Timed's flight speed)
        self.walking = self.data  # the hero's own values, while Boosters swap in theirs
        self.scale_then = 1.0  # speed_scale when the last step began: the statescript frame the mods follow
        self.script_mods = None  # a body_vm body: the VM's movement mods after the last frame, in place of Sprint's
        self.ability_mods = {}  # a kits body: the mods of the last body frame's MovementMod states (frame_mods)
        self.dashing = None  # (x, y, z, m/s, metres left, alone) of a dash under way
        self.capping = None  # (seconds left, m/s): the ground speed is cut to no more than this when they are over (roll)
        self.throws = []  # [seconds from now, level part kept, vertical part kept, (x, y, z) m/s or its function] (_throw)
        self.thrown = None  # (level part kept, x, the vertical speed, z) this step's move is thrown with
        self.lift = None  # (m/s2, m/s) Hover Jets push up with: all of it while not rising, none at that rise
        self.swapped = None  # the mover values the last body frame's MovementMod state swaps in (frame_flight)
        self.aimed = False  # that frame has a flight along the aim on (Echo's Flight)
        self.rise = 0.0  # jump less crouch, as an aimed flight's mods leave them: its input up and down
        self.limit = FLT_MAX  # the most the speed may be this step (the mods on SPEED_LIMIT)

    @property
    def grounded(self) -> bool:
        return not self.airborne

    @property
    def driven(self) -> bool:
        """A frame's mod sets the forward throttle to 1 (Reinhardt's Charge): the body runs on whatever the input."""
        return any(index == THROTTLE_FORWARD and op == SET and value == 1.0 for index, op, value, _ in self.ability_mods.values())

    @property
    def striking(self) -> bool:
        """A dash that is the body's whole movement is under way (Swift Strike; not a flight to a point)."""
        return bool(self.dashing and self.dashing[5] is True)

    def use(self, data=None, world=None) -> None:
        if data is not None:
            self.walking = data
        super().use(None if self.flight else data, world)

    @staticmethod
    def _heading(command) -> tuple:
        """The level direction (x, z) of the command's input, or straight ahead without one."""
        angle = command.yaw * TURN
        ahead = (math.sin(angle), math.cos(angle))  # the right-hand axis is (-cos, sin)
        x = command.forward * ahead[0] - command.right * ahead[1]
        z = command.forward * ahead[1] + command.right * ahead[0]
        length = math.hypot(x, z)
        return (x / length, z / length) if length else ahead

    def dash(self, command, distance: float, seconds: float) -> None:
        """Carry the body `distance` metres in `seconds` from the next step on, on top of its own movement: along the
        command's input, or straight ahead without one. Walls stop it."""
        x, z = self._heading(command)
        self.dashing = (x, 0.0, z, distance / seconds, distance, False)

    def strike(self, command, distance: float, speed: float) -> None:
        """Swift Strike (graph 01DB's StateMoveTo: 15 m in 0.4 s): a dash along the aim, pitch and all, that is the
        body's whole movement while it lasts."""
        yaw, pitch = command.yaw * TURN, command.pitch * TURN  # positive pitch looks down
        level = math.cos(pitch)
        self.dashing = (math.sin(yaw) * level, -math.sin(pitch), math.cos(yaw) * level, speed, distance, True)

    def fly_to(self, point, speed: float) -> bool:
        """Guardian Angel (graph 006E's StateMoveTo): `speed` m/s straight at `point` is the body's whole movement
        from the next step on. Called every step, as the point moves with the ally; True once the body is there."""
        way = [p - s for p, s in zip(point, self.position)]
        left = math.hypot(*way)
        there = left < 0.01  # the feet's grid is 1 mm an axis
        self.dashing = None if there else (*(w / left for w in way), speed, left, 'to')
        return there

    def jet(self, command, ahead: float, up: float) -> None:
        """Jump Jet (graph 0027): the vertical speed becomes `up` m/s and `ahead` m/s is added along the facing."""
        angle = command.yaw * TURN
        self._throw((ahead * math.sin(angle), up, ahead * math.cos(angle)))

    @staticmethod
    def _lifted(command, speed: float, steepest: float, back: bool = False) -> tuple:
        """`speed` m/s along the command's aim (or straight away from it) lifted by LEAP_LIFT, no flatter than
        LEAP_LEAST and no steeper than `steepest`: the graphs' STU_0762B33A, a direction kept within a cone."""
        way = -1.0 if back else 1.0
        yaw, pitch = command.yaw * TURN, command.pitch * TURN
        rise = math.atan2(-math.sin(pitch) * way + LEAP_LIFT, math.cos(pitch))
        rise = min(max(rise, LEAP_LEAST), steepest)
        level = speed * math.cos(rise) * way
        return level * math.sin(yaw), speed * math.sin(rise), level * math.cos(yaw)

    def leap(self, command, speed: float) -> None:
        """Jump Pack (graph 010A): the vertical speed is dropped and `speed` m/s added along the lifted aim, no
        steeper than 70 degrees."""
        self._throw(self._lifted(command, speed, math.pi / 2 - LEAP_LEAST))

    def kick(self, command, speed: float, delay: float) -> None:
        """Coach Gun (graph 1513): `delay` seconds after the press the velocity becomes `speed` m/s straight away from
        where she aims then, lifted."""
        self._throw(lambda aim: self._lifted(aim, speed, math.pi / 2, back=True), delay, level=0.0)

    def fly(self, command, speed: float) -> None:
        """Flight (graph 1BB6): the vertical speed is dropped and `speed` m/s added along the input in the aim's own
        frame, pitch and all (along the aim without one); with jump held as much again upward, the direction's
        vertical part kept to 1."""
        yaw, pitch = command.yaw * TURN, command.pitch * TURN  # positive pitch looks down
        level = math.cos(pitch)
        aim = (math.sin(yaw) * level, -math.sin(pitch), math.cos(yaw) * level)
        side = (-math.cos(yaw), 0.0, math.sin(yaw))
        way = [command.forward * a + command.right * s for a, s in zip(aim, side)]
        length = math.hypot(*way)
        way = [w / length for w in way] if length else list(aim)
        if command.buttons & JUMP:
            way[1] = min(1.0, way[1] + 1.0)
        self._throw([w * speed for w in way])

    def blast(self, origin, level: float, up: float) -> None:
        """Thrown by the hero's own Concussion Mine going off at `origin` (graph 096A): `level` m/s straight away
        from it on the level and `up` m/s upward, added to the velocity."""
        dx, dz = self.position[0] - origin[0], self.position[2] - origin[2]
        length = math.hypot(dx, dz)
        x, z = (dx / length, dz / length) if length else (0.0, 0.0)
        self._throw((x * level, up, z * level), rise=1.0)

    def hop(self, command, scale: float) -> None:
        """Genji's second jump (graph 0157: STU_9AF0FA80 on axis 3 with 1.5): the vertical speed becomes `scale` jumps.
        shortcut: axis 3 is taken to be a jump times the value (Sound Barrier's hop has 1, Lucio's wall jump 1.15);
        the client predicts none of them on a VM body, so it is not checked."""
        self._throw((0.0, self.data.jump_speed * scale, 0.0))

    def lunge(self, command, speed: float, lift: float) -> None:
        """Hanzo's Lunge (graph 010C): the velocity becomes `speed` m/s along the input (straight ahead without one)
        with `lift` of it upward."""
        x, z = self._heading(command)
        self._throw((x * speed, lift * speed, z * speed), level=0.0)

    def uppercut(self, command, lunge: float, up: float) -> None:
        """Rising Uppercut (graph 0DD8), three launches: the velocity becomes `lunge` m/s along the facing lifted 0.3
        (16.7 degrees); 0.15 s on a fifth of the level speed is kept and 1 m/s ahead and `up` m/s upward added; 0.1 s
        after that a fifth of the rise is kept. The facing is each launch's own."""
        def facing(ahead, rise):
            return lambda aim: (ahead * math.sin(aim.yaw * TURN), rise, ahead * math.cos(aim.yaw * TURN))
        length = math.hypot(1.0, 0.3)
        self._throw(facing(lunge / length, lunge * 0.3 / length), level=0.0)
        self._throw(facing(1.0, up), 0.15, level=0.2)
        self._throw((0.0, 0.0, 0.0), 0.25, rise=0.2)

    def roll(self, command, speed: float, seconds: float, after: float) -> None:
        """Combat Roll (graph 01F0): the ground speed becomes `speed` m/s along the input, or straight ahead without
        one, and when `seconds` are over it is cut to no more than `after`. The roll's own MovementMod states (no
        deceleration, no input) keep the speed meanwhile."""
        x, z = self._heading(command)
        self.velocity = (quantise(x * speed), self.velocity[1], quantise(z * speed))
        self.capping = (seconds, after)

    def _throw(self, velocity, after: float = 0.0, level: float = 1.0, rise: float = 0.0) -> None:
        """Off the ground with the step `after` seconds from now: `level` of its level speed and `rise` of its vertical
        speed are kept and `velocity` (x, y, z; or what it gives for that step's command) is added. The client's
        launch is an action of that tick, after the tick's friction and gravity and before its move (its first frame
        in the air has the launch's speed to the grid), so `_move` plays it."""
        self.throws.append([after, level, rise, velocity])

    def _thrown(self, command, seconds: float) -> None:
        """The throws due this step, made ready for its move."""
        waiting = []
        for after, level, rise, velocity in self.throws:
            if after >= seconds:  # a Wait of 0.165 s ends in the client's 24th tick of 7 ms
                waiting.append([after - seconds, level, rise, velocity])
                continue
            ax, ay, az = velocity(command) if callable(velocity) else velocity
            vx, vy, vz = self.velocity
            self.thrown = (level, ax, quantise(vy * rise + ay), az)
            self.velocity = (vx, self.thrown[2], vz)  # rising already: the step's ground check lets it go
            self.flags_word |= FELL
        self.throws = waiting

    def _move(self, dt: float, normal):
        if self.lift:
            (push, spent), (vx, vy, vz) = self.lift, self.velocity
            self.velocity = (vx, vy + push * min(1.0, max(0.0, 1.0 - vy / spent)) * dt, vz)
        if self.thrown:
            (level, ax, ay, az), (vx, _, vz), self.thrown = self.thrown, self.velocity, None
            self.velocity = (quantise(vx * level + ax), ay, quantise(vz * level + az))
        speed = math.hypot(*self.velocity)
        if speed > self.limit:
            self.velocity = tuple(v / speed * self.limit for v in self.velocity)
        return super()._move(dt, normal)

    def _friction(self, dt: float, horizontal, vertical: float, grounded: bool) -> None:
        """A flight along the aim: the air slows the whole velocity as it slows a walk's level part (Echo's own
        flights lose speed along their line, whatever its pitch)."""
        if not self.aimed:
            return super()._friction(dt, horizontal, vertical, grounded)
        data, speed = self.values, math.hypot(*self.velocity)
        rate = (data.decel_air_base * data.run_forward
                + max(0.0, speed - data.decel_air_base_speed) * data.decel_air_per_speed * self.data.run_forward)
        slow = min(speed, dt * rate)
        if speed > TINY and slow > 0.0:
            self.velocity = tuple(v - v / speed * slow for v in self.velocity)

    def launch(self, velocity) -> None:
        """Thrown off the ground at velocity (x, y, z) m/s (D.Va's eject): in the air until it lands."""
        self.velocity = tuple(quantise(v) for v in velocity)
        self.flags_word |= FELL

    def step(self, command, seconds: float, tick: int = 0) -> None:
        if not self.on_map and self.grounded and self.position[1] != self.world.lowest:
            self.world, self.body = flat_world(self.position[1]), None  # no map: the floor follows a teleport
        # The mods of the statescript's last frame act from this one: Sprint as it was when the last step began
        # (match.py sets it before the step), Boosters as they are now (set after the last step).
        if self.script_mods is not None:
            mods = dict(self.script_mods)
        else:
            mods = {SPRINT: (SPEED, ADD, self.scale_then - 1.0, 0.0)} if self.scale_then != 1.0 else {}
            mods.update(self.ability_mods)
        if self.flight:
            mods.update(BOOST_MODS)
        elif self.aimed:
            mods.update(NO_LANDING)
        self.track_mods(mods, command.frame - 1)
        self.scale_then = self.speed_scale
        super().use(BOOST_VALUES if self.flight else self.swapped or self.walking)
        if (self.flight or self.aimed) and not self.flags_word & IN_AIR:
            self.flags_word |= FELL  # lifts off
        if self.flight:
            command = replace(command, forward=127)
        active = [mod for mod in sorted(self.mods.values(), key=Mod.order) if mod.acts(command.frame)]
        self.limit = intent(active, SPEED_LIMIT, FLT_MAX)
        self.rise = (intent(active, JUMP_INTENT, float(bool(command.buttons & JUMP)))
                     - intent(active, CROUCH_INTENT, float(bool(command.buttons & CROUCH)))) if self.aimed else 0.0
        throttles = [mod for mod in active if mod.index in (THROTTLE_FORWARD, THROTTLE_BACKWARD, THROTTLE_SIDE)]
        if throttles:  # the input the client's mover is left with
            forward = intent(throttles, THROTTLE_FORWARD, max(command.forward, 0) / 127)
            backward = intent(throttles, THROTTLE_BACKWARD, max(-command.forward, 0) / 127)
            side = intent(throttles, THROTTLE_SIDE, abs(command.right) / 127)
            command = replace(command, forward=round((forward - backward) * 127),
                              right=round(math.copysign(side, command.right) * 127))
        if self.dashing and self.dashing[5]:
            command, self.velocity = replace(command, forward=0, right=0), (0.0, 0.0, 0.0)
        self._thrown(command, seconds)
        if self.body is not None:
            x, y, z = self.position
            self.body.teleport((x, f32(y + self._lift()), z))
        super().step(command, seconds, tick)
        if self.dashing:
            self._carry(command.frame, seconds)
        if self.capping:
            left, top = self.capping[0] - seconds, self.capping[1]
            self.capping = (left, top) if left >= seconds / 2 else None  # whole frames, as kits.Timed counts them
            vx, vy, vz = self.velocity
            fast = math.hypot(vx, vz)
            if self.capping is None and fast > top:
                self.velocity = (quantise(vx / fast * top), vy, quantise(vz / fast * top))

    def _carry(self, frame: int, seconds: float) -> None:
        """This step's part of the dash: the capsule moved through the map as Mover._move moves it, the feet and the
        step's snapshot put where it ends."""
        x, y, z, speed, left, alone = self.dashing
        go = min(speed * seconds, left)
        body, lift = self._engine_body(), self._lift()
        final, _, _ = body.move(contacts.v3(x * go, y * go, z * go), f32(seconds), ROUNDS)
        self.position = [quantise(final[0]), quantise(f32(final[1] - lift)), quantise(final[2])]
        px, py, pz = self.position
        body.teleport((px, f32(py + lift), pz))
        self.history[-1] = self.snapshot(frame)
        self.dashing = (x, y, z, speed, left - go, alone) if left - go > 1e-6 else None
        if alone and y > 0.0:
            self.flags_word |= FELL  # struck upward: in the air when it ends

    def _accelerate(self, dt: float, right: float, forward: float, axes, grounded: bool) -> None:
        """A flight along the aim (Boosters, Echo's Flight) turns the whole velocity toward a wish along the aim, pitch
        included: Mover._accelerate in 3D. Echo's also takes jump and crouch as up and down."""
        if not (self.flight or self.aimed):
            return super()._accelerate(dt, right, forward, axes, grounded)
        data = self.values
        pitch = self.pitch * TURN  # positive looks down
        (fx, fz), level = axes, math.cos(pitch)
        ahead, strafe = forward * data.run_forward, right * data.run_strafe
        wish = (strafe * -fz + ahead * fx * level, self.rise * data.run_forward - ahead * math.sin(pitch),
                strafe * fx + ahead * fz * level)
        norm = math.hypot(*wish)
        if norm <= 0.0:
            return
        wish = [w / norm for w in wish]
        top = min(norm, self._cap_of(wish[0], wish[2], math.hypot(wish[0], wish[2]), axes))
        rate = data.accel_air * data.run_forward * dt  # touching or not: Echo's own slide along a wall at full speed
        velocity = self.velocity
        speed = math.hypot(*velocity)
        target = max(speed, top)
        turn = [target * w - v for w, v in zip(wish, velocity)]
        delta = math.hypot(*turn)
        if delta <= 0.0:
            return
        k = min(1.0, rate / delta)
        new = [v + k * t for v, t in zip(velocity, turn)]
        if speed > TINY and sum(w * v for w, v in zip(wish, velocity)) > 0.0:
            scale = (speed + min(rate, target - speed)) / math.hypot(*new)
            new = [v * scale for v in new]
        self.velocity = tuple(new)
