"""The graphs instanced on a hero's VM body: the buttons each waits on and its node classes; with GRAPH (hex), every
state and action node of that graph with its fields (expressions decompiled).

    python tools/research/body_graphs.py HERO [GRAPH...]
"""
import collections
import logging
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lobby'))
logging.disable(logging.CRITICAL)
import ability_states as A  # noqa: E402
from ow174.game.content import heroes  # noqa: E402
from ow174.game.script import expr  # noqa: E402


def show(v):
    if isinstance(v, dict):
        try:
            return expr.source(v)
        except Exception as error:
            return f'<{type(error).__name__}>'
    if isinstance(v, list):
        return [show(x) if not isinstance(x, dict) or '$' not in x else {k: show(w) for k, w in x.items() if w not in (None, [], 0)} for x in v]
    return v


name, wanted = sys.argv[1], {int(x, 16) for x in sys.argv[2:]}
hero = next(h for h in heroes().values() if h.name == name)
script = A.BodyScript(hero.guid, 0xA0000102, 1000, quanta=A.QUANTA, health=None, ult_seconds=0)
script.created(A._Create()).stream.arrived(0)
for instance in script.component.instances.values():
    graph = instance.graph
    nodes = [n for n in graph.nodes if n is not None]
    buttons = sorted({n.fields['m_A5F1A77B'].get('m_logicalButton') for n in nodes
                      if isinstance(n.fields.get('m_A5F1A77B'), dict)} - {None})
    if not wanted:
        classes = collections.Counter(n.cls.replace('STUStatescript', '') for n in nodes)
        print(f'{graph.index:04X} buttons {buttons} nodes {len(nodes)}: ' + ', '.join(f'{c}x{k}' for c, k in classes.most_common(14)))
    elif graph.index in wanted:
        owner = {id(node): slot for slot, node in enumerate(graph.owner_states())}
        print(f'== {graph.index:04X} buttons {buttons} sync {[e.var for e in graph.presence_vars()]}')
        for n in nodes:
            fields = {k: show(v) for k, v in n.fields.items() if k.startswith('m_') and 'lug' not in k and v not in (None, [], 0)}
            print(f'  {n.pos:3} o{owner.get(id(n), "-")} {n.cls.replace("STUStatescript", "")}: {str(fields)[:330]}')
