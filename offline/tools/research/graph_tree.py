"""A catalog graph's nodes as a tree of what starts what: each Entry, then every node its plugs lead to, with the
state index and owner slot of a state (`st5/o5`; `(local)`: client-only) and a MovementMod's mods spelt out.

    python tools/research/graph_tree.py GRAPH [GRAPH ...]    (hex indexes: 1983 is Baptiste's Exo Boots)
"""
import json, logging, sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lobby'))
logging.disable(logging.CRITICAL)
import practice  # noqa
from ow174.game.script import graph as G
from ow174.game import mover as M

MODS = {getattr(M, n): n for n in ('SPEED', 'ACCEL_GROUND', 'ACCEL_AIR', 'DECEL_GROUND', 'DECEL_AIR', 'JUMP_SPEED_MOD', 'GRAVITY_MOD',
        'TERMINAL', 'TERMINAL_UP', 'GROUND_ACCEL', 'GROUND_DECEL', 'JUMP_INTENT', 'CROUCH_INTENT', 'COLLISION_MOD',
        'LAND_LIMIT_MOD', 'STEP_DOWN_MOD', 'DECEL_AIR_BASE', 'DECEL_AIR_SPEED') if hasattr(M, n)}

def short(v, depth=0):
    if isinstance(v, list): return '[' + ', '.join(short(i, depth + 1) for i in v[:6]) + (', ...' if len(v) > 6 else '') + ']'
    if not isinstance(v, dict): return repr(v)
    cls = v.get('$', '').replace('STUConfigVar', '').replace('STUStatescript', '')
    rest = {k: x for k, x in v.items() if k not in ('$', 'm_EE729DCB', 'links')}
    if set(rest) == {'m_value'}: return f'{cls}({rest["m_value"]})'
    if cls == 'Dynamic': return f'var{rest.get("m_identifier")}' + ('e' if rest.get('m_60DB8F99') else '')
    return cls + '(' + ', '.join(f'{k[2:]}={short(x, depth + 1)}' for k, x in rest.items() if x not in (None, 0, [], False)) + ')'

def describe(node):
    f = node.fields
    cls = node.cls.replace('STUStatescript', '')
    bits = []
    if node.cls == 'STU_316CFEF2':
        for m in f.get('m_BC5E91CF') or ():
            op = M.MOD_OPS.get(m.get('$'))
            bits.append(f"{MODS.get(m.get('m_A9561CA0'), m.get('m_A9561CA0'))} {op[0] if op else m.get('$')} {short(m.get('m_9B7A63EA'))}" + (f" p{short(m.get('m_priority'))}" if m.get('m_priority') else ''))
    for key in ('m_condition', 'm_out_Var', 'm_value', 'm_timeout', 'm_logicalButton', 'm_button', 'm_gameMessage', 'm_abilityButton', 'm_cooldownTime', 'm_duration', 'm_target', 'm_effect', 'm_animation', 'm_11E0A658'):
        if f.get(key) not in (None, 0, [], False):
            bits.append(f'{key[2:]}={short(f[key])[:90]}')
    return cls + (' {' + '; '.join(bits) + '}' if bits else '')

for arg in sys.argv[1:]:
    g = G.graph(int(arg, 16))
    owner = {id(n): i for i, n in enumerate(g.owner_states())}
    by_pos = {n.pos: n for n in g.nodes if n is not None} if hasattr(g, 'nodes') else {}
    print(f'=== {arg} owner states {len(owner)}')
    children, has_parent = {}, set()
    for n in by_pos.values():
        for path, plug in n.plugs().items():
            for target, tplug in n.links(path):
                children.setdefault(n.pos, []).append((path.split('.')[0][2:], target))
                has_parent.add(target)
    def show(pos, depth, via, seen):
        n = by_pos.get(pos)
        if n is None: return
        tag = f'st{n.state}/o{owner[id(n)]}' if id(n) in owner else (f'st{n.state}(local)' if n.is_state else 'act')
        print('  ' * depth + f'{via + " -> " if via else ""}[{pos}] {tag} {describe(n)}'[:230])
        if pos in seen: return
        for via2, target in children.get(pos, ()):
            show(target, depth + 1, via2, seen | {pos})
    for pos in sorted(by_pos):
        if pos not in has_parent:
            show(pos, 0, '', frozenset())
