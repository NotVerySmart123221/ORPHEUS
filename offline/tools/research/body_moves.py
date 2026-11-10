"""Every graph instanced on a hero's VM body that has a movement node: MovementMod states and their mods, launch
actions and their operations, MoveTo, the mover swap, Blink's dash, the thrust state.

    python tools/research/body_moves.py HERO...
"""
import logging
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lobby'))
logging.disable(logging.CRITICAL)
import ability_states as A  # noqa: E402
from ow174.game.content import heroes  # noqa: E402
from ow174.game.script import expr  # noqa: E402

KINDS = {'STU_316CFEF2': 'mod', 'STU_FF3471E9': 'launch', 'STUStatescriptStateMoveTo': 'moveto', 'STU_ED2B22BD': 'swap',
         'STU_A807207A': 'blink', 'STU_A9C4FF83': 'thrust'}


def show(v):
    if isinstance(v, dict):
        try:
            return expr.source(v)
        except Exception as error:
            return f'<{type(error).__name__}>'
    return v


for name in sys.argv[1:]:
    hero = next(h for h in heroes().values() if h.name == name)
    script = A.BodyScript(hero.guid, 0xA0000102, 1000, quanta=A.QUANTA, health=None, ult_seconds=0)
    script.created(A._Create()).stream.arrived(0)
    print(f'== {name}')
    for instance in script.component.instances.values():
        graph = instance.graph
        found = [n for n in graph.nodes if n is not None and n.cls in KINDS]
        if not found:
            continue
        buttons = sorted({n.fields['m_A5F1A77B'].get('m_logicalButton') for n in graph.nodes
                          if n is not None and isinstance(n.fields.get('m_A5F1A77B'), dict)} - {None})
        print(f' graph {graph.index:04X} buttons {buttons}')
        for n in found:
            if n.cls in ('STU_316CFEF2', 'STU_FF3471E9'):
                ops = '; '.join(f"{m.get('$')}({m.get('m_A9561CA0', m.get('m_CFEC5F76'))}, "
                                f"{show(m.get('m_9B7A63EA')) if m.get('m_9B7A63EA') is not None else ''}"
                                f"{' ' + str(show(m.get('m_8AD739CE'))) + '..' + str(show(m.get('m_8213EB22'))) if m.get('m_8213EB22') is not None else ''})"
                                for m in n.fields.get('m_BC5E91CF') or ())
                print(f'   {KINDS[n.cls]:6} node {n.pos}: {ops[:420]}')
            else:
                print(f'   {KINDS[n.cls]:6} node {n.pos}: ' + ', '.join(f'{k}={show(v)}' for k, v in n.fields.items() if k.startswith('m_') and 'lug' not in k and v not in (None, [], 0))[:420])
