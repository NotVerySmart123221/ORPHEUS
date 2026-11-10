"""Every hero's body on the vendored statescript VM, offline: does it run, and does the client read its frames?

    python tools/research/vm_heroes.py [--hero NAME ...] [--out FILE] [--volleys]

Per hero: a BodyScript (vendor ow174/game/script/driver.py) is started, left idle, then given each button in turn
(primary, secondary, ability 1 and 2, ultimate with a full charge, reload, quick melee). Every frame it sends is
read by the client model of the lobby tests (test_vm_driver.ClientModel). The report lists, per hero: an error or
client crash; whether the client's states match the server's at the end; the Volleys that fired and the button that first fired each,
split into the ones the server plays (body_vm.effect) and the ones it does not; whether an ultimate ran; the
movement mods seen; and the VM's own warnings (things it does not model). Results: research/lizrollin-runtime.md section 18.
--volleys prints instead the table tools/lobby/body_vm.py keeps: each Volley and the button that fires it.
"""
import argparse
import logging
import sys
from pathlib import Path

LOBBY = Path(__file__).resolve().parents[1] / 'lobby'
sys.path.insert(0, str(LOBBY))
import practice  # noqa: E402,F401  (puts vendor/ on the path)
from ow174.game.content import heroes  # noqa: E402
from ow174.game.mover import statescript_mods  # noqa: E402
from ow174.game.script import decode, expr, nodes  # noqa: E402
from ow174.game.script.driver import BodyScript  # noqa: E402

import body_vm  # noqa: E402
from test_vm_driver import ClientModel, server_on  # noqa: E402

# buttons as the commands carry them (test_body_vm.py): name -> mask
BUTTONS = {'primary': 0x01, 'secondary': 0x02, 'ability1': 0x08, 'ability2': 0x10, 'ultimate': 0x20,
           'reload': 0x400, 'melee': 0x800}
ENTITY, START = 0xA0000102, 1000


class _Create:
    delivered = True


def run(hero):
    """One hero's findings as a dict."""
    warnings = []
    warn = expr.warn_once
    expr.warn_once = lambda key, text, *args: warnings.append(text % args if args else text)
    found = {'name': hero.name, 'error': None, 'match': None, 'volleys': {}, 'ults': 0, 'mods': set(), 'frames': 0}
    pressed = 'idle'
    try:
        script = BodyScript(hero.guid, ENTITY, START, health=None, ult_seconds=0)
        client = ClientModel(hero.guid)
        frame = START

        def step(buttons=0, frames=1, forward=0):
            nonlocal frame
            for _ in range(frames):
                frame += 1
                script.command(frame, buttons, 0, forward, 0)
                for state, _, _ in script.component.shots:  # each Volley, under the button that first fired it
                    found['volleys'].setdefault((state.instance.graph.index, state.index), pressed)
                script.component.shots[:] = []
                found['mods'] |= {mod[:2] for mod in (statescript_mods(script.component) or {}).values()}
                if any(state.flags & nodes.Ability.RUNNING for state in script.ultimates):
                    found['ults'] += 1
                for update in script.frames():
                    client.apply(update)
                    update.stream.arrived(update.chunk_last)
                    found['frames'] += 1

        create = script.created(_Create())
        create.stream.arrived(0)  # the client has the body: the full frame goes out
        step(0, 30)
        for name, mask in BUTTONS.items():
            pressed = name
            if name == 'ultimate':
                cost = expr.to_float(script.component.entity_var(body_vm.ULT_COST).value())
                script.set_ult_charge(cost)
            step(mask, 40, forward=127 if name == 'ability1' else 0)
            step(0, 400)
        found['match'] = client.on() == server_on(script)
    except decode.ClientCrash as error:
        found['error'] = f'the client would crash: {error.where}: {error.what}'
    except Exception as error:  # the VM or the encoder stopped on this hero
        found['error'] = f'{type(error).__name__}: {error}'
    finally:
        expr.warn_once = warn
    found['warnings'] = sorted(set(warnings))
    return found


def report(rows):
    out = ['| Hero | Runs | Client matches | Volleys the server plays | Volleys it does not | Ultimate ran | Move mods |',
           '| --- | --- | --- | --- | --- | --- | --- |']
    for row in rows:
        def volleys(played):
            return ', '.join(f'{graph:04X}/{state} ({button})' for (graph, state), button in sorted(row['volleys'].items())
                             if (body_vm.effect(row['name'], (graph, state)) is not None) == played) or '-'
        out.append(f"| {row['name']} | {'no: ' + row['error'] if row['error'] else 'yes'} | "
                   f"{ {True: 'yes', False: 'NO', None: '-'}[row['match']] } | {volleys(True)} | {volleys(False)} | "
                   f"{'yes' if row['ults'] else 'no'} | {len(row['mods'])} |")
    kinds = {}
    for row in rows:
        for text in row['warnings']:
            kinds.setdefault(text, []).append(row['name'])
    out += ['', 'Warnings (what the VM does not model), by how many heroes hit them:', '']
    out += [f'- {len(names)}: {text}' for text, names in sorted(kinds.items(), key=lambda item: -len(item[1]))]
    return '\n'.join(out) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--hero', action='append', help='only these heroes (name prefix, any case)')
    parser.add_argument('--out', type=Path)
    parser.add_argument('--volleys', action='store_true', help='print body_vm.VOLLEYS')
    args = parser.parse_args()
    logging.disable(logging.CRITICAL)
    chosen = [hero for hero in sorted(heroes().values(), key=lambda hero: hero.name)
              if not args.hero or any(hero.name.lower().startswith(name.lower()) for name in args.hero)]
    rows = [run(hero) for hero in chosen]
    if args.volleys:
        lines = []
        for row in rows:
            fired = sorted((key, BUTTONS[button]) for key, button in row['volleys'].items() if button in BUTTONS)
            if fired:
                lines.append('    ' + ' '.join(f'({graph:#06x}, {state}): {mask:#x},' for (graph, state), mask in fired)
                             + f'  # {row["name"]}')
        text = '\n'.join(['VOLLEYS = {', *lines, '}', ''])
    else:
        text = report(rows)
    if args.out:
        args.out.write_bytes(text.encode('utf-8'))
    else:
        sys.stdout.buffer.write(text.encode('utf-8'))


if __name__ == '__main__':
    main()
