"""Send one selected Soldier body statescript frame through the owned loopback launcher."""
import argparse
import logging
import math
from pathlib import Path
import sys

LOBBY_TOOLS = Path(__file__).resolve().parents[1] / "lobby"
if str(LOBBY_TOOLS) not in sys.path:
    sys.path.insert(0, str(LOBBY_TOOLS))

import practice
from ow174.game.statescript import (Asset, Bool, Float, Graph, Instance, Int, Entity, ScriptRef, Stream,
    CONTROLLER, HUD, StateBound, subscript, owner_full_frame, remote_full_frame)

SEQUENCE = ("empty", "graph", "int", "draw", "float", "root", "stack", "full")
PHASES = (*SEQUENCE, "rifle-bot-remote-init", "root-only", "root-bool", "ready", "combined", "ammo", "volley",
    "volley2", "volley-trailer1", "hud-bool", "hud-seeded", "hud-entity", "hud-local", "hud-root", "hud-root-body",
    "hud-gate-bool", "hud-gate-entity", "hud-mode", "hud-native", "hud-bootstrap", "hud-bootstrap-entity", "hud-bootstrap-ref", "hud-reset",
    "draw-zero", "draw-one", "body-roots", "manager-root", "rifle-input", "rifle-hud",
    "rifle-stack-top", "rifle-stack-under", "rifle-stack-both", "rifle-effect", "rifle-client", "rifle-dataflow", "rifle-entry", "rifle-body", "rifle-manager", "rifle-binding", "rifle-feedback", "rifle-reload", "rifle-root-body", "rifle-root-flags", "rifle-root-hud", "rifle-hud-mode", "rifle-shared-hud", "rifle-shared-parent", "rifle-projectile", "rifle-bot-graph", "rifle-bot-volley", "rifle-bot-feedback", "rifle-bot-remote", "rifle-muzzle-body", "rifle-muzzle-entity", "rifle-hud-body", "rifle-attack", "sequence", "control")
RIFLE = Graph(596, 81, 23)
CONTROL_FILE = Path(__file__).resolve().parents[2] / '.scratchpad' / 'body-phase.txt'


def rifle_states(phase):
    if phase in ('rifle-attack', 'rifle-bot-feedback', 'rifle-bot-remote', 'rifle-bot-remote-init', 'rifle-shared-parent'):
        phase = 'rifle-shared-hud'
    if phase == 'rifle-shared-hud':
        return {**rifle_states('rifle-reload'), **rifle_states('rifle-hud')}
    if phase.startswith('rifle-bot-') or phase in ('rifle-root-body', 'rifle-root-flags', 'rifle-root-hud', 'rifle-hud-mode', 'rifle-projectile'):
        phase = 'rifle-reload'
    if phase == 'rifle-reload':
        return {**rifle_states('rifle-feedback'), 39: None}
    if phase == 'rifle-feedback':
        return {**rifle_states('rifle-binding'), 23: None, 68: None}
    if phase == 'rifle-binding':
        phase = 'rifle-manager'
    from ow174.game.bits import BitWriter
    selected, hidden, button = BitWriter(), BitWriter(), BitWriter()
    selected.bit(1)
    hidden.bit(0)
    button.bit(1)  # LogicalButton explicit Begin, then counter4; owner prelude fields are filtered.
    button.bits(0, 4)
    active = {1: hidden, 2: hidden, 44: hidden, 72: selected, 8: button}
    if phase in ('rifle-client', 'rifle-dataflow', 'rifle-entry', 'rifle-body', 'rifle-manager'):
        active[79] = None  # State83 hosts the asset's client-only states57..59.
    if phase in ('rifle-dataflow', 'rifle-entry', 'rifle-body', 'rifle-manager'):
        active.update({state: None for state in (6, 17, 18, 22, 33, 34)})
    if phase in ('rifle-entry', 'rifle-manager'):
        startup = BitWriter()
        startup.bits(1, 2)
        active[60] = startup  # Entry0's Stack64, with the recovered two-bit Stack DSR.
    if phase == 'rifle-hud':
        active.update({9: None, 63: hidden, 65: None, 66: None})
    elif phase.startswith('rifle-stack-') or phase in ('rifle-effect', 'rifle-client', 'rifle-dataflow', 'rifle-entry', 'rifle-body', 'rifle-manager', 'rifle-muzzle-body', 'rifle-muzzle-entity'):
        flags = BitWriter()
        flags.bit(phase != 'rifle-stack-under')
        flags.bit(phase in ('rifle-stack-under', 'rifle-stack-both'))
        active.update({13: hidden, 29: flags})
        if phase in ('rifle-effect', 'rifle-client', 'rifle-dataflow', 'rifle-entry', 'rifle-body', 'rifle-manager', 'rifle-muzzle-body', 'rifle-muzzle-entity'):
            from soldier import muzzle_payload
            active.update({21: muzzle_payload(True, 0), 57: None})
    return active


def volley_payload(count, trailer=0):
    from ow174.game.bits import BitWriter
    if not 1 <= count <= 65535:
        raise ValueError('count must fit the native compact counter')
    out = BitWriter()
    out.bit(1)  # custom-state Begin
    out.bits(0, 10)  # signed time delta zero: sign0, width0, 8-bit magnitude
    out.bit(1)  # count update present
    out.bits(0, 10)  # signed update-time delta zero
    out.w_u16(count)
    out.bit(0)  # optional multiplier defaults to1
    out.bits(trailer, 6)
    return out


def hud_frame(phase, cmfd, player):
    if phase == 'hud-reset':
        return owner_full_frame(cmfd, [Instance(1, CONTROLLER, presence={5: Bool(True)})],
                                {2438: Bool(False), 31435: Bool(False)})
    extra = {} if phase in ('hud-bool', 'hud-gate-bool', 'hud-gate-entity', 'hud-mode', 'hud-native', 'hud-bootstrap', 'hud-bootstrap-entity', 'hud-bootstrap-ref', 'hud-root', 'hud-root-body') else {
        3179: Entity(player.entity), 17937: Entity(player.body), 23604: Entity(player.entity)}
    variables = {2438: Bool(False), 31435: Bool(False)}
    if phase in ('hud-bool', 'hud-seeded', 'hud-gate-bool', 'hud-gate-entity', 'hud-mode'):
        variables.update({17906: Bool(True), 18405: Bool(True)})
    elif phase == 'hud-entity':
        variables.update({17906: Entity(player.entity), 18405: Entity(player.body)})
    if phase == 'hud-gate-bool':
        variables[23604] = Bool(True)
    elif phase == 'hud-gate-entity':
        variables[23604] = Entity(player.body)
    return owner_full_frame(cmfd, [
        Instance(1, CONTROLLER, presence={5: Bool(True)}, active={36: subscript(4)}),
        Instance(4, HUD, parent=(1, 42), extra=extra, active={0: None})], variables)


def body_startup(phase):
    if phase == 'rifle-shared-parent':
        instances, variables = body_startup('rifle-shared-hud')
        instances[1].active[3] = subscript(463)
        instances[-1].parent = (75, 3)
        return instances, variables
    if phase in ('rifle-attack', 'rifle-bot-feedback', 'rifle-bot-remote', 'rifle-bot-remote-init'):
        phase = 'rifle-shared-hud'
    if phase == 'rifle-shared-hud':
        instances, variables = body_startup('rifle-root-flags')
        # State139 hosts the portrait/health presenter;140 hosts the remaining client graph.
        instances.append(Instance(463, Graph(463, 41, 22),
            presence={0: Asset(0x0C00000000007DA7)}, active={39: None, 40: None}))
        return instances, variables
    if phase == 'rifle-root-hud':
        instances, variables = body_startup('rifle-root-flags')
        instances.append(Instance(4418, Graph(4418, 1, 0), active={0: None}))
        return instances, variables
    if phase in ('rifle-projectile', 'rifle-root-flags', 'rifle-hud-mode'):
        phase = 'rifle-root-body'
    if phase == 'rifle-root-body':
        instances, variables = body_startup('rifle-reload')
        instances.append(Instance(4429, Graph(4429, 1, 0), active={0: None}))
        return instances, variables
    if phase.startswith('rifle-bot-'):
        phase = 'rifle-reload'
    if phase in ('rifle-binding', 'rifle-feedback', 'rifle-reload'):
        phase = 'rifle-manager'
    if phase not in ('rifle-body', 'rifle-manager'):
        return [], {}
    # The native client retains an existing instance's graph when its ID is reused.
    instances = [Instance(75, Graph(75, 39, 4), presence={2: Int(1)}, active={19: None})]
    variables = {338: Int(1650), 982: Int(1)}
    if phase == 'rifle-manager':
        from ow174.game.bits import BitWriter
        branch = BitWriter()
        branch.bit(0)
        visible = BitWriter()
        visible.bits(1, 2)
        instances.insert(0, Instance(21, Graph(21, 35, 1),
            active={11: branch, 23: visible, 29: None, 32: None, 33: None}))
        variables[3772] = Bool(True)  # Owner Stack23's catalog value, required by presenter38.
    return instances, variables


def frame_for_phase(phase: str, cmfd: int, weapon_entity=None):
    if phase == 'rifle-hud-body':
        from soldier import weapon_instance
        return owner_full_frame(cmfd, [weapon_instance(extra_active=rifle_states(phase))],
            {31: Int(1), 34: Bool(False), 478: Bool(False), 7184: Int(30), 7185: Int(30),
                23604: Bool(True)})
    if phase.startswith('rifle-'):
        from soldier import body_frame
        instances, variables = body_startup(phase)
        return body_frame(cmfd, extra_active=rifle_states(phase), weapon_entity=weapon_entity,
                          extra_instances=instances, extra_variables=variables)
    if phase in ('body-roots', 'manager-root'):
        from soldier import weapon_instance
        from ow174.game.bits import BitWriter
        active = {}
        if phase == 'manager-root':
            branch = BitWriter()
            branch.bit(0)
            active = {11: branch, 29: None}
        return owner_full_frame(cmfd, [weapon_instance(),
            Instance(2, Graph(21, 35, 1), active=active), Instance(3, Graph(51, 12, 4))],
            {31: Int(1), 34: Bool(False), 478: Bool(False), 7184: Int(30), 7185: Int(30)})
    if phase in ('root-bool', 'ready', 'combined', 'ammo', 'volley', 'volley2', 'volley-trailer1'):
        from ow174.game.bits import BitWriter
        selected, hidden = BitWriter(), BitWriter()
        selected.bit(1)
        hidden.bit(0)
        active = {0: selected, 12: hidden}
        if phase in ('combined', 'ammo', 'volley', 'volley2', 'volley-trailer1'):
            active[11] = None
        if phase == 'ready':
            draw = BitWriter()
            draw.bit(1)
            active[11] = draw
        presence = {4: Int(1)}
        variables = {31: Int(1), 34: Bool(False)}
        if phase in ('ammo', 'volley', 'volley2', 'volley-trailer1'):
            count = 2 if phase == 'volley2' else int(phase.startswith('volley'))
            presence.update({8: Int(30-count), 12: Float(1.5), 13: Int(30), 15: Float(20),
                17: Int(0), 18: Float(0), 20: Int(100), 21: Int(100)})
            variables.update({478: Bool(False), 7184: Int(30-count), 7185: Int(30)})
            if count:
                active[10] = volley_payload(count, int(phase == 'volley-trailer1'))
        return owner_full_frame(cmfd, [Instance(1, RIFLE, presence=presence,
            active=active)], variables)
    if phase == 'root-only':
        return owner_full_frame(cmfd, [Instance(1, RIFLE, presence={4: Int(1)},
            active={0: None, 12: None})], {31: Int(1), 34: Bool(False)})
    if phase in ('draw-zero', 'draw-one'):
        from ow174.game.bits import BitWriter
        payload = BitWriter()
        payload.bit(phase == 'draw-one')
        return owner_full_frame(cmfd, [Instance(1, RIFLE, presence={4: Int(1)},
            active={11: payload, 0: None, 12: None})], {31: Int(1), 34: Bool(False)})
    if phase == "empty":
        return owner_full_frame(cmfd, [], {})
    if phase == "graph":
        return owner_full_frame(cmfd, [Instance(1, RIFLE)], {})
    if phase == "int":
        return owner_full_frame(cmfd, [Instance(1, RIFLE, presence={4: Int(1)})], {})
    if phase == "draw":
        instance = Instance(1, RIFLE, presence={4: Int(1)}, active={11: None})
        return owner_full_frame(cmfd, [instance], {31: Int(1), 34: Bool(False)})
    if phase in ('float', 'root', 'stack'):
        presence = {4: Int(1)}
        active = {11: None}
        if phase == 'float':
            presence[15] = Float(20)
        elif phase == 'root':
            active.update({0: None, 12: None})
        else:
            active[60] = None
        return owner_full_frame(cmfd, [Instance(1, RIFLE, presence=presence, active=active)],
                                {31: Int(1), 34: Bool(False)})
    if phase == 'full':
        instance = Instance(1, RIFLE, presence={
            4: Int(1), 8: Int(30), 12: Float(1.5), 13: Int(30), 15: Float(20),
            17: Int(0), 18: Float(0), 20: Int(100), 21: Int(100)},
            extra={6884: Float(0.511), 6885: Float(0.1)},
            active={0: None, 11: None, 12: None, 60: None})
        return owner_full_frame(cmfd, [instance], {
            31: Int(1), 34: Bool(False), 478: Bool(False), 7184: Int(30), 7185: Int(30)})
    raise ValueError(f"unknown body probe phase: {phase}")


class ProbeSchedule:
    def __init__(self, phase: str):
        if phase not in PHASES:
            raise ValueError(f"unknown body probe phase: {phase}")
        self.phase = phase
        self.started = {}
        self.next_index = {}
        self.last_queued = {}
        self.sent = set()

    def due(self, matches, now: float):
        due = []
        pending = set()
        for match in matches:
            for player in match.players:
                if (player.hero.guid != practice.SOLDIER or not player.has_body
                        or not player.spawned or player.client is None):
                    continue
                body = player.body
                if body in pending:
                    continue
                if body not in self.started:
                    self.started[body] = now
                    pending.add(body)
                    continue
                if self.phase == "sequence":
                    index = self.next_index.get(body, 0)
                    if index == len(SEQUENCE):
                        continue
                    if now - self.started[body] < 2:
                        continue
                    if index:
                        stream = player.body_script
                        if stream is None or stream.entity != body or stream.data_in_flight:
                            continue
                        if now - self.last_queued[body] < 5:
                            continue
                    due.append((player, SEQUENCE[index]))
                elif body not in self.sent and now - self.started[body] >= 2:
                    due.append((player, self.phase))
                pending.add(body)
        return due

    def queued(self, player, phase: str, now: float) -> None:
        body = player.body
        if self.phase == "sequence":
            index = self.next_index.get(body, 0)
            if index >= len(SEQUENCE) or SEQUENCE[index] != phase:
                raise ValueError("body probe phase queued out of sequence")
            self.next_index[body] = index + 1
            self.last_queued[body] = now
        else:
            if phase != self.phase or body in self.sent:
                raise ValueError("body probe phase queued more than once")
            self.sent.add(body)


def queue_probe_frame(player, phase: str, newest_frame: int):
    if phase.startswith('hud-'):
        stream = player.script
        cmfd = stream.next_cmfd(newest_frame)
        update = stream.full_frame(hud_frame(phase, cmfd, player))
        updates = [update]
        if phase in ('hud-mode', 'hud-native', 'hud-bootstrap', 'hud-bootstrap-entity', 'hud-bootstrap-ref', 'hud-root', 'hud-root-body'):
            from ow174.game.statescript import variables_frame
            if phase in ('hud-root', 'hud-root-body'):
                mode_cmfd = player.mode_script.next_cmfd(newest_frame)
                mode_frame = owner_full_frame(mode_cmfd, [
                    Instance(3, Graph(4429, 1, 0), active={0: None},
                        source_entity=player.body if phase == 'hud-root-body' else None)],
                    {23604: ScriptRef(3, 0)})
            elif phase.startswith('hud-bootstrap'):
                mode_cmfd = player.mode_script.next_cmfd(newest_frame)
                parent = (ScriptRef(2, 5) if phase.endswith('-ref') else
                    Entity(player.mode_script.entity) if phase.endswith('-entity') else Bool(True))
                mode_frame = owner_full_frame(mode_cmfd, [
                    Instance(1, Graph(5056, 20, 3), active={19: subscript(2)}),
                    Instance(2, Graph(3254, 1, 0), parent=(1, 21))],
                    {23604: parent})
            else:
                mode_frame = variables_frame({23604: Bool(True)})
            mode = player.mode_script.full_frame(mode_frame)
            if phase != 'hud-mode':
                updates.insert(0, mode)
            else:
                updates.append(mode)
        player.client.queue_entities(updates)
        return update
    if phase == 'rifle-hud-mode':
        queue_probe_frame(player, 'hud-bootstrap-ref', newest_frame)
    body = player.body
    from ow174.game import world
    updates = []
    flags = phase in ('rifle-root-flags', 'rifle-root-hud', 'rifle-hud-mode', 'rifle-shared-hud', 'rifle-shared-parent', 'rifle-attack', 'rifle-bot-feedback', 'rifle-bot-remote', 'rifle-bot-remote-init')
    if flags != getattr(player, '_probe_ui_flags', False):
        updates.append(world.EntityUpdate(player.entity, world.OP_UPDATE,
            lambda origin, flags=flags: world.update(origin, {85: [int(flags)]})))
        player._probe_ui_flags = flags
    old_weapon = getattr(player, '_probe_weapon', None)
    if old_weapon is not None:
        updates.append(world.EntityUpdate(old_weapon, world.OP_DESTROY))
        player._probe_weapon = None
    player.weapon_entity = body if phase == 'rifle-muzzle-body' else None
    if phase in ('rifle-binding', 'rifle-feedback', 'rifle-reload', 'rifle-root-body', 'rifle-root-flags', 'rifle-root-hud', 'rifle-hud-mode', 'rifle-shared-hud', 'rifle-shared-parent', 'rifle-attack', 'rifle-projectile') or phase.startswith('rifle-bot-'):
        player.weapon_entity = StateBound(Entity(0), 1, 11, 7)
    if phase in ('rifle-muzzle-entity', 'rifle-projectile'):
        # shortcut: fixed eye-level fixture isolates entity binding; attachment remains unverified.
        angle = player.mover.yaw * math.pi / 32768
        x, y, z = player.mover.position
        position = (x + .5*math.sin(angle), y + 1.6, z + .5*math.cos(angle))
        serial = getattr(player, '_probe_weapon_serial', 0) + 1
        if serial >= 0x1000000:
            raise ValueError('diagnostic weapon entity IDs exhausted')
        player._probe_weapon_serial = serial
        fixture = player._probe_weapon = 0xB1000000 + serial
        if phase == 'rifle-muzzle-entity':
            player.weapon_entity = fixture
        definition = 0x0400000000001476 if phase == 'rifle-projectile' else 0x04000000000004BF
        updates.append(world.EntityUpdate(fixture, world.OP_CREATE,
            lambda origin: world.create(origin, definition, {}, position=position)))
    player.weapon_extra_states = rifle_states(phase) if phase.startswith('rifle-') else {}
    player.body_extra_instances, player.body_extra_variables = body_startup(phase)
    stream = player.body_script
    if stream is None or stream.entity != body:
        stream = player.body_script = Stream(body)
    cmfd = stream.next_cmfd(newest_frame)
    update = stream.full_frame(frame_for_phase(phase, cmfd, player.weapon_entity))
    updates.append(update)
    if phase.startswith('rifle-bot-'):
        updates.append(bot_probe_update(player, phase, newest_frame))
    player.client.queue_entities(updates)
    return update


def bot_probe_update(player, phase, newest_frame):
    """The bot's BA3 statescript frame for a rifle-bot-* phase, on the nearest still bot."""
    bots = [bot for bot in player.match.bots if bot.behaviour is None]
    if not bots:
        raise ValueError('bot probe requires an existing target')
    # The nearest still bot, so its shots are in view from the spawn room.
    target = min(bots, key=lambda bot: math.dist(bot.position, player.mover.position))
    bot_stream = getattr(player, '_probe_bot_script', None)
    if bot_stream is None or bot_stream.entity != target.entity:
        bot_stream = player._probe_bot_script = Stream(target.entity)
    active = {13: volley_payload(1, 1)} if phase in ('rifle-bot-volley', 'rifle-bot-feedback') else {}
    presence = {12: Asset(0x0DC0000000000005)} if active else {}
    variables = {}
    if phase == 'rifle-bot-feedback':
        from ow174.game.bits import BitWriter
        stack = BitWriter()
        stack.bits(1, 2)
        active.update({7: None, 10: stack, 26: stack})
        presence[8] = Bool(True)  # BA3's instance233, mirrored to data flow6C by state7.
        variables[233] = Bool(True)
    bot_cmfd = bot_stream.next_cmfd(newest_frame)
    if phase in ('rifle-bot-remote', 'rifle-bot-remote-init'):
        from ow174.game.bits import BitWriter
        # Repeats (BodyProbeServer) raise the counter: the first frame may only set the baseline.
        counter = getattr(player, '_probe_volley_counter', 1)
        payload = BitWriter()
        payload.bit(1)  # native custom-state active request
        payload.bit(0)  # remote Volley optional integer defaults to1
        payload.bits(counter & 63, 6)
        bot = Instance(1, Graph(0xBA3, 32, 13, 21),
            active={7: payload}, presence={12: Asset(0x0DC0000000000005)})
        if phase == 'rifle-bot-remote-init':
            # BA3 Entry constants; remote3 is authored owner7's firing data-flow mapping.
            # Presence order is BA3's instance sync vars: 581, 2316, 350, 351, 229, 230, 231,
            # 1769, 233, 6889, 173, 50, 6973. Volley reads 231; ChaseVars drive 231 (rate 2316).
            bot.presence.update({0: Float(1.5), 1: Float(0), 2: Float(100), 3: Float(100), 4: Int(0),
                5: Float(2.75), 6: Float(0), 7: Float(100), 8: Bool(True)})
            bot.extra = {198: Int(20), 53: Int(20), 6884: Float(.489)}
            bot.active[3] = None
        bot_frame = remote_full_frame([bot], {})
    else:
        bot_frame = owner_full_frame(bot_cmfd,
            [Instance(1, Graph(0xBA3, 32, 13), active=active, presence=presence)], variables)
    return bot_stream.full_frame(bot_frame)


class BodyProbeServer(practice.GameServer):
    gameplay_enabled = True
    body_setup = None  # Preserve isolated probes instead of applying the normal Practice loadout.
    phase = "full"

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._probe_schedule = ProbeSchedule('draw' if self.phase == 'control' else self.phase)
        self._control_phase = None
        self._next_control_read = 0
        self._control_valid = False

    def _tick(self, now):
        if self.phase == 'control' and now >= self._next_control_read:
            self._next_control_read = now + 1
            try:
                with CONTROL_FILE.open(encoding='ascii') as source:
                    phase = source.read(65).strip()
                if phase not in PHASES or phase in ('control', 'sequence'):
                    raise ValueError('control file must contain one concrete phase name')
            except (OSError, UnicodeError, ValueError) as exc:
                self._control_valid = False
                print(f'[BODY-PROBE] control held: {exc}', file=sys.stderr, flush=True)
            else:
                self._control_valid = True
                if phase != self._control_phase:
                    self._control_phase = phase
                    self._probe_schedule = ProbeSchedule(phase)
        enabled = self.phase != 'control' or self._control_valid
        for match in self.matches:
            match.bot_attacks_enabled = enabled and self._probe_schedule.phase == 'rifle-attack'
        super()._tick(now)
        if self.phase == 'control' and not self._control_valid:
            return
        for player, phase in self._probe_schedule.due(self.matches, now):
            stream = player.script if phase.startswith('hud-') else player.body_script
            if stream is not None and stream.data_in_flight:
                continue
            if phase == 'rifle-hud-mode' and (player.script.data_in_flight or player.mode_script.data_in_flight):
                continue
            if phase in ('hud-mode', 'hud-native', 'hud-bootstrap', 'hud-bootstrap-entity', 'hud-bootstrap-ref', 'hud-root', 'hud-root-body') and player.mode_script.data_in_flight:
                continue
            newest = self.tick if player.commands.last_frame is None else player.commands.last_frame
            queue_probe_frame(player, phase, newest)
            self._probe_schedule.queued(player, phase, now)
            player._probe_volley_next = now + 1
            print(f"[BODY-PROBE] phase={phase} owner frame queued", file=sys.stderr, flush=True)
        if self._probe_schedule.phase == 'rifle-bot-remote-init':
            self._repeat_bot_volley(now)

    def _repeat_bot_volley(self, now):
        """Once a second after the first frame, send the bot's Volley again with the next counter."""
        for match in self.matches:
            for player in match.players:
                bot_stream = getattr(player, '_probe_bot_script', None)
                if (player.client is None or bot_stream is None or bot_stream.data_in_flight
                        or now < getattr(player, '_probe_volley_next', math.inf)):
                    continue
                player._probe_volley_counter = getattr(player, '_probe_volley_counter', 1) + 1
                player._probe_volley_next = now + 1
                newest = self.tick if player.commands.last_frame is None else player.commands.last_frame
                update = bot_probe_update(player, 'rifle-bot-remote-init', newest)
                player.client.queue_entities([update])
                print(f"[BODY-PROBE] bot volley counter={player._probe_volley_counter}", file=sys.stderr, flush=True)


def serve(phase: str) -> int:
    import lobby

    BodyProbeServer.phase = phase
    practice.GameServer = BodyProbeServer
    logging.basicConfig(level=logging.INFO, format='%(asctime)s %(message)s')
    lobby.serve()


def launch(game_root: Path, phase: str, timeout: int) -> int:
    import run

    server_command = [sys.executable, "-u", str(Path(__file__).resolve()), "--serve", "--phase", phase]
    return run.launch(game_root, timeout, server_command=server_command)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("game_root", nargs="?", type=Path)
    parser.add_argument("--phase", choices=PHASES, default="full")
    parser.add_argument("--timeout", type=int, default=180)
    parser.add_argument("--serve", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    if args.serve:
        if args.game_root is not None:
            parser.error("--serve does not take game_root")
        return serve(args.phase)
    if args.game_root is None:
        parser.error("game_root is required")
    if not 10 <= args.timeout <= 3600:
        parser.error("timeout must be 10..3600 seconds")
    return launch(args.game_root, args.phase, args.timeout)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        raise SystemExit(130)
