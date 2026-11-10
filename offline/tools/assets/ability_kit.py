"""Derive a hero body's ability HUD from the pinned graph catalog, the way Soldier's Biotic Field HUD was mapped
(research/abilities.md): per ability graph, its StateAbility, the button it waits on, the UX presenters that
draw its icon, and the cooldown (var472, authored on the Stack that writes it) with the seconds-left var61.

    python ability_kit.py CATALOG.json.gz HERO_NAME...

Prints, per hero, the tuples tools/lobby/kits.py's ABILITIES takes:
(graph index, owner states, instance vars, remote slots, ability state, command bit, UX states,
 var472 bit, var61 bit, cooldown seconds, state bits, var61's bound state).
var61 binds the StateAbility by its full state index, not its owner index: they differ when client-only states
come first (Zarya's E: owner 6, state 9), and the owner index showed no countdown.
Then, per ability with a projectile, its cast: the owner states from the StateAbility down to the first WeaponVolley
(all payload-free, else none) and the Volley's owner state, which kits.py pulses on a press like Biotic Field's.
Graph catalog: upstream's data/game_graphs_174.json.gz (pinned commit, see tools/lobby/vendor/README.md).
"""
import argparse
import gzip
import json

from weapon_kit import BARE, instance_vars, plugs

ABILITY, UX, VOLLEY = 'STUStatescriptStateAbility', 'STUStatescriptStateUXPresenter', 'STUStatescriptStateWeaponVolley'
ANIM = 'STUStatescriptStateAnim'
# logical button -> command-record bit (research/abilities.md: read from the lobby's input log)
COMMAND = {29: 0x02, 35: 0x08, 36: 0x10}
SHARED = {67}  # graph 67 (every body, logical button 31): not a hero ability


def owner_states(graph):
    owner = {}
    for n in sorted((n for n in graph['nodes'] if n['state'] is not None), key=lambda n: n['state']):
        if not (n['client_only'] or n['server_only']):
            owner[n['state']] = len(owner)
    return owner


def cooldown(graph):
    """Seconds of the cooldown struct the graph writes to var472 (STU_7197C080 m_01AFADB6), or None."""
    for n in graph['nodes']:
        out, value = n['fields'].get('m_out_Var'), n['fields'].get('m_value')
        if isinstance(out, dict) and out.get('m_identifier') == 472 and isinstance(value, dict):
            return (value.get('m_01AFADB6') or {}).get('m_value')
    return None


def abilities(graph):
    """The graph's first StateAbility on a COMMAND button, as an ABILITIES tuple, or None."""
    nodes = {n['pos']: n for n in graph['nodes']}
    owner = owner_states(graph)
    found = sorted((owner[n['state']], COMMAND[n['fields']['m_A5F1A77B']['m_logicalButton']], n['state']) for n in nodes.values()
                   if n['class'] == ABILITY and n['state'] in owner and isinstance(n['fields'].get('m_A5F1A77B'), dict)
                   and n['fields']['m_A5F1A77B'].get('m_logicalButton') in COMMAND)
    if not found:
        return None
    parents = {}
    for pos, node in nodes.items():
        for _, children in plugs(node['fields']):
            for child in children:
                parents.setdefault(child, []).append(pos)

    def under_ability(pos):
        seen = set()
        while pos in parents and pos not in seen:
            seen.add(pos)
            pos = parents[pos][0]
            if nodes[pos]['class'] == ABILITY:
                return True
        return False

    # the icon presenters: not client-only and not under the StateAbility (those run while it is used)
    ux = sorted(owner[n['state']] for pos, n in nodes.items()
                if n['class'] == UX and n['state'] in owner and not under_ability(pos))
    variables = instance_vars(graph)
    total = sum(1 for n in graph['nodes'] if n['state'] is not None)
    return (graph['index'], len(owner), len(variables), len(graph['remote']), found[0][0], found[0][1], ux,
            variables.get(472), variables.get(61), cooldown(graph), (total - 1).bit_length(), found[0][2])


def cast(graph, state):
    """(path owner states, Volley owner state) from the StateAbility at full state index `state` down to the first
    WeaponVolley (breadth first), or None: no Volley, or a state on the way with an undecoded payload."""
    nodes = {n['pos']: n for n in graph['nodes']}
    owner = owner_states(graph)
    start = next(pos for pos, n in nodes.items() if n['state'] == state and n['class'] == ABILITY)
    came, queue = {start: None}, [start]
    while queue:
        pos = queue.pop(0)
        if nodes[pos]['class'] == VOLLEY:
            volley, path = owner[nodes[pos]['state']], []
            while came[pos] != start:
                pos = came[pos]
                if nodes[pos]['state'] in owner:
                    if nodes[pos]['class'] not in BARE:
                        return None
                    path.append(owner[nodes[pos]['state']])
            return tuple(reversed(path)), volley
        for _, children in plugs(nodes[pos]['fields']):
            for child in children:
                if child not in came:
                    came[child] = pos
                    queue.append(child)
    return None


def anim(graph, state):
    """(path owner states, StateAnim owner state) from the StateAbility down to its first StateAnim, as cast() finds
    the Volley: the cast animation, pulsed with the cast (research/statescript-payloads.md, StateAnim)."""
    nodes = {n['pos']: n for n in graph['nodes']}
    owner = owner_states(graph)
    start = next(pos for pos, n in nodes.items() if n['state'] == state and n['class'] == ABILITY)
    came, queue = {start: None}, [start]
    while queue:
        pos = queue.pop(0)
        if nodes[pos]['class'] == ANIM:
            target, path = owner[nodes[pos]['state']], []
            while came[pos] != start:
                pos = came[pos]
                if nodes[pos]['state'] in owner:
                    if nodes[pos]['class'] not in BARE:
                        return None
                    path.append(owner[nodes[pos]['state']])
            return tuple(reversed(path)), target
        for _, children in plugs(nodes[pos]['fields']):
            for child in children:
                if child not in came:
                    came[child] = pos
                    queue.append(child)
    return None


def hero(catalog, name):
    graphs = {g['index']: g for g in catalog['graphs']}
    body = next(b for b in catalog['bodies'].values() if b['name'] == name)
    weapons = {w for w in body['weapons'] if w is not None}
    out = []
    for entry in body['graphs']:
        index = entry['graph']
        if index in graphs and index not in SHARED and index not in weapons:
            found = abilities(graphs[index])
            if found:
                out.append(found)
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('catalog')
    parser.add_argument('heroes', nargs='+')
    args = parser.parse_args()
    catalog = json.load(gzip.open(args.catalog))
    graphs = {g['index']: g for g in catalog['graphs']}
    for name in args.heroes:
        found = hero(catalog, name)
        print(name, found)
        for ability in found:
            print('  cast', ability[0], cast(graphs[ability[0]], ability[-1]), 'anim', anim(graphs[ability[0]], ability[-1]))


if __name__ == '__main__':
    main()
