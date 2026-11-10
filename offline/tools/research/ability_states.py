"""What an ability's own graph turns on when its button is pressed: the hero's body run offline on the vendored
statescript VM (the client's own graphs), one press, and each state of one graph with the time it was on.

    python tools/research/ability_states.py HERO BUTTON [--graph INDEX] [--hold SECONDS] [--seconds S] [--forward]
                                            [--kit]

BUTTON: shift, e, rmb, lmb, q, jump, crouch (the command bits). --graph: the graph to list (hex index; default: the
kits' ability on that button). --hold: how long the button is held (default 0.1). --forward: W held throughout.
Per state that changed: its owner slot (what tools/lobby/kits.py's Ability.running names), class, the codec the client
reads its data by (vendor/data/statescript_codecs_174.json), on and off times after the press, and what it does (a
MovementMod's mods, a Stack's variable, a Wait's timeout). The times are the graph's own: the VM gets no landing or
target from a mover, so a state that waits for one stays on (research/lizrollin-runtime.md sections 21 and 23).
--kit prints the states as tools/lobby/kits.py's Ability.running takes them: (owner slot, how, on, off) for each
state whose data the kits can write (HOW: by its codec), with the times it is on after the press; the rest are
listed as left out.
STU_54FEB21C is taken to end at once: the VM has no class for it, so it never ended and nothing after it ran. It is
the first state of 29 abilities' activation, with the ability's own Wait on its end plug (GATE below).
"""
import argparse
import logging
import sys
from pathlib import Path

LOBBY = Path(__file__).resolve().parents[1] / 'lobby'
sys.path.insert(0, str(LOBBY))
import practice  # noqa: E402,F401  (puts vendor/ on the path)
from ow174.game import mover  # noqa: E402
from ow174.game.content import heroes  # noqa: E402
from ow174.game.script import codecs, runtime  # noqa: E402
from ow174.game.script.driver import BodyScript  # noqa: E402

BUTTONS = {'lmb': 0x01, 'rmb': 0x02, 'shift': 0x08, 'e': 0x10, 'q': 0x20, 'jump': 0x40, 'crouch': 0x80}
QUANTA = 7000  # microseconds a command frame
MODS = {getattr(mover, name): name for name in (
    'SPEED', 'ACCEL_GROUND', 'ACCEL_AIR', 'DECEL_GROUND', 'DECEL_AIR', 'JUMP_SPEED_MOD', 'GRAVITY_MOD', 'TERMINAL',
    'TERMINAL_UP', 'GROUND_ACCEL', 'GROUND_DECEL', 'JUMP_INTENT', 'CROUCH_INTENT', 'COLLISION_MOD', 'LAND_LIMIT_MOD',
    'STEP_DOWN_MOD')}
OPS = ('ADD', 'MULTIPLY', 'SET', 'MINIMUM', 'MAXIMUM', 'CLAMP')
# codec -> kits.PAYLOADS' name for a state's data; a switch takes its value. Only for the classes in SENT: a state
# that makes an entity or changes health (StatePet, StateModifyHealth) is the server's to play, not a bit to send.
HOW = {'none': None, 'link': 'unlinked', 'stack': 'stack', 'anim': 'anim', 'counter2': 'effect', 'button': 'button'}
SENT = {'STUStatescriptState', 'STUStatescriptStateWait', 'STUStatescriptStateStack', 'STUStatescriptStateAnim',
        'STUStatescriptStateBooleanSwitch', 'STUStatescriptStateUXPresenter', 'STUStatescriptStateLogicalButton',
        'STUStatescriptStatePhysicsExplosion', 'STU_316CFEF2', 'STU_9028E035', 'STU_BD02E168', 'STU_691BFA55',
        'STU_33A46033'}


class _Create:
    delivered = True


@runtime.state_class('STU_54FEB21C')
class GATE(runtime.State):
    """In this process only: the gate before an ability's main Wait finishes as it begins."""

    def on_enter(self) -> None:
        self.request_finish()


def _config(value) -> str:
    if not isinstance(value, dict):
        return repr(value)
    if 'm_value' in value and not isinstance(value['m_value'], dict):
        return repr(value['m_value'])
    if value.get('$') == 'STUConfigVarDynamic':
        return f"var{value.get('m_identifier')}" + ('e' if value.get('m_60DB8F99') else '')
    return value.get('$', '?').replace('STUConfigVar', '')


def what(node, instance) -> str:
    """A state's own data, in a few words."""
    fields = node.fields
    if node.cls == mover.MOVEMENT_MOD:
        parts = []
        for mod in fields.get('m_BC5E91CF') or ():
            op = mover.MOD_OPS.get(mod.get('$'))
            config = mod.get('m_9B7A63EA')
            value = _config(config)
            if isinstance(config, dict) and 'm_value' not in config:  # a variable or expression: what the VM gets
                try:
                    value += f' = {instance.evaluate(config)!r}'
                except Exception as error:  # the VM cannot evaluate it offline
                    value += f' ({type(error).__name__})'
            parts.append(f"{MODS.get(mod.get('m_A9561CA0'), mod.get('m_A9561CA0'))} {OPS[op[0]] if op else mod.get('$')} {value}")
        return '; '.join(parts)
    for key, label in (('m_out_Var', 'sets'), ('m_timeout', 'timeout'), ('m_condition', 'if'), ('m_11E0A658', 'anim'),
                       ('m_effect', 'effect')):
        if isinstance(fields.get(key), dict):
            text = f'{label} {_config(fields[key])}'
            if key == 'm_out_Var' and isinstance(fields.get('m_value'), dict):
                text += f" = {_config(fields['m_value'])}"
            if key == 'm_timeout' and 'm_value' not in fields[key]:
                try:
                    text += f' = {instance.evaluate(fields[key])!r}'
                except Exception as error:
                    text += f' ({type(error).__name__})'
            return text
    return ''


def run(hero, button: int, graph: int, hold: float, seconds: float, forward: bool):
    """[(owner slot or None, state index, class, on s, off s or None, text)] of the graph's states that were on at
    some time after the press and not before it, in the order they came on."""
    start, rest = 1000, 60
    script = BodyScript(hero.guid, 0xA0000102, start, quanta=QUANTA, health=None, ult_seconds=0)
    script.created(_Create()).stream.arrived(0)
    press = start + rest
    seen, before, current = {}, set(), {}
    instance = None
    for frame in range(start + 1, press + round(seconds * 1e6 / QUANTA)):
        held = press <= frame < press + max(1, round(hold * 1e6 / QUANTA))
        script.command(frame, button if held else 0, 0, 127 if forward else 0, 0)
        script.component.shots[:] = []
        for update in script.frames():
            update.stream.arrived(update.chunk_last)
        instance = next((i for i in script.component.instances.values() if i.graph.index == graph), None)
        if instance is None:
            continue
        on = {index for index, state in instance.states.items() if state.active}
        if frame < press:
            before = on
            continue
        now = (frame - press) * QUANTA / 1e6
        for index in on - before:
            seen.setdefault(index, [now, None])
            if seen[index][1] is not None:  # came on again
                seen[index][1] = None
            current.setdefault(index, getattr(instance.states[index], 'current', None))
        for index, times in seen.items():
            if index not in on and times[1] is None:
                times[1] = now
    if instance is None:
        return []
    owner = {id(node): slot for slot, node in enumerate(instance.graph.owner_states())}
    rows = []
    for index, (on, off) in sorted(seen.items(), key=lambda item: (item[1][0], item[0])):
        node = instance.graph.state(index)
        rows.append((owner.get(id(node)), index, node.cls, on, off, what(node, instance), current.get(index)))
    return rows


def kit(rows) -> tuple[list, list]:
    """(Ability.running entries with their times to 1/100 s, the states left out as (owner slot, class, why))."""
    running, left = [], []
    for slot, _, cls, on, off, _, current in rows:
        on, off = round(on, 2), None if off is None else round(off, 2)
        item = codecs.codec(cls)
        codec = item.codec if item else None
        if slot is None:
            continue  # client-only or server-only: no bit in an owner frame
        if cls not in SENT:
            left.append((slot, cls, 'not a class the kits send'))
        elif codec == 'switch':
            running.append((slot, int(bool(current)), on, off))
        elif codec in HOW:
            running.append((slot, HOW[codec], on, off))
        else:
            left.append((slot, cls, f'codec {codec}'))
    return sorted(running, key=lambda entry: (entry[2], entry[0])), left


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('hero', help='hero name (a prefix, any case)')
    parser.add_argument('button', choices=sorted(BUTTONS))
    parser.add_argument('--graph', type=lambda value: int(value, 16))
    parser.add_argument('--hold', type=float, default=0.1)
    parser.add_argument('--seconds', type=float, default=6.0)
    parser.add_argument('--forward', action='store_true')
    parser.add_argument('--kit', action='store_true', help="print the states as kits.py's Ability.running")
    args = parser.parse_args()
    logging.disable(logging.CRITICAL)
    hero = next(h for h in sorted(heroes().values(), key=lambda h: h.name) if h.name.lower().startswith(args.hero.lower()))
    graph = args.graph
    if graph is None:
        import kits
        graph = next(a.graph.index for a in kits.ABILITIES.get(hero.guid, ()) if a.button == BUTTONS[args.button])
    rows = run(hero, BUTTONS[args.button], graph, args.hold, args.seconds, args.forward)
    print(f'{hero.name}, {args.button}: graph {graph:04X}, {len(rows)} states came on')
    print('| Owner slot | State | Class | Codec | On (s) | Off (s) | What |')
    print('|---|---|---|---|---|---|---|')
    for slot, index, cls, on, off, text, _ in rows:
        item = codecs.codec(cls)
        print(f"| {'-' if slot is None else slot} | {index} | {cls.replace('STUStatescript', '')} | "
              f"{item.codec if item else '?'} | {on:.2f} | {'-' if off is None else f'{off:.2f}'} | {text} |")
    if args.kit:
        running, left = kit(rows)
        print('running=(' + ', '.join(f'({slot}, {how!r}, {on:.2f}, {off})' for slot, how, on, off in running) + ')')
        for slot, cls, why in left:
            print(f'left out: owner slot {slot} {cls} ({why})')


if __name__ == '__main__':
    main()
