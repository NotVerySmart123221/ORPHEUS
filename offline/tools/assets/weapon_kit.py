"""Derive a hero weapon graph's owner-state set from the pinned graph catalog, the way Cassidy's was mapped from
Soldier's (research/non-soldier-hero-scope.md): the switches on the path to the fire button, to the ammo HUD and to
the weapon model, plus the data-flow mappings under the root.

    python weapon_kit.py CATALOG.json.gz GRAPH_INDEX [VOLLEY_STATE]

Prints owner-state numbers as tools/lobby/kits.py takes them: {state: 1 | 0 | 'button' | None} (BooleanSwitch on its true or
false branch, a LogicalButton, any other state on), and the weapon model (CosmeticEntity) state.
Graph catalog: upstream's data/game_graphs_174.json.gz (pinned commit, see tools/lobby/vendor/README.md).
"""
import argparse
import gzip
import json

SWITCH, BUTTON = 'STUStatescriptStateBooleanSwitch', 'STUStatescriptStateLogicalButton'
STACK, TARGETS = 'STUStatescriptStateStack', 'STUStatescriptStateTrackTargets'
COSMETIC, MAPPING, UX = 'STUStatescriptStateCosmeticEntity', 'STUStatescriptStateDataFlowMapping', 'STUStatescriptStateUXPresenter'
AMMO, CAPACITY = 53, 198  # instance vars the ammo presenter reads
# states with no payload (research/abilities.md; the STU_ ones and TrackRay: research/statescript-payloads.md)
BARE = {COSMETIC, MAPPING, UX, 'STUStatescriptStateWait', 'STUStatescriptState', 'STUStatescriptStateTrackRay', 'STU_DEBD057B',
        'STU_9D7BF987', 'STU_54FEB21C'}
# tools/lobby/kits.py PAYLOADS; 'targets': no targets; 'ability': StateAbility with no cooldown (soldier.ability_payload)
KINDS = {BUTTON: 'button', STACK: 'stack', TARGETS: 'targets', 'STUStatescriptStateAbility': 'ability'}


def plugs(value, path=''):
    """(plug path, [linked node positions]) for every plug in a node's fields."""
    if isinstance(value, dict):
        if value.get('links'):
            yield path, [link[0] for link in value['links']]
        for key, inner in value.items():
            if key != 'links':
                yield from plugs(inner, f'{path}.{key}' if path else key)
    elif isinstance(value, list):
        for i, inner in enumerate(value):
            yield from plugs(inner, f'{path}[{i}]')


def variables(node):
    text = json.dumps(node['fields'])
    return {int(v) for part in text.split('"m_identifier": ')[1:] if (v := part.split(',')[0]) != 'null'}


def kit(graph, volley=None):
    nodes = {n['pos']: n for n in graph['nodes']}
    parents = {}
    for pos, node in nodes.items():
        for plug, children in plugs(node['fields']):
            for child in children:
                parents.setdefault(child, []).append((pos, plug))

    def chain(pos):
        """The states above `pos` up to an entry, each with how it is entered (first parent wins)."""
        out, seen = {}, set()
        while pos in parents and pos not in seen:
            seen.add(pos)
            parent, plug = parents[pos][0]
            node = nodes[parent]
            if node['state'] is not None and not (node['client_only'] or node['server_only']):
                if node['class'] == SWITCH:
                    lower = plug.lower()
                    # a switch always carries its bit; under m_subgraphPlug (both branches) either value works
                    # shortcut: 1 picked for m_subgraphPlug without checking which branch the client then runs
                    out.setdefault(node['state'], 0 if 'false' in lower else 1)
                elif node['class'] == BUTTON:
                    out.setdefault(node['state'], 'button')
                elif node['class'] in KINDS:
                    out.setdefault(node['state'], KINDS[node['class']])
                elif node['class'] in BARE:
                    out.setdefault(node['state'], None)
                # shortcut: states with an undecoded payload (StateAbility, ...) are left out
                # (a wrong width misaligns the frame); Sombra's weapon shows without them.
            pos = parent
        return out

    states = {}
    by_state = {n['state']: pos for pos, n in nodes.items() if n['state'] is not None}
    ammo = [pos for pos, n in nodes.items() if n['class'] == UX and {AMMO, CAPACITY} <= variables(n)]
    models = [pos for pos, n in nodes.items() if n['class'] == COSMETIC]
    for target in ammo + models:
        states.update(chain(target))
        states[nodes[target]['state']] = None
    if volley is not None:
        fire = chain(by_state[volley])
        buttons = [s for s, how in fire.items() if how == 'button']
        if buttons:  # the top button (the last one walked) and the switches above it
            top = buttons[-1]
            states.update(chain(by_state[top]))
            states[top] = 'button'
    grown = True
    while grown:  # mappings whose whole chain is entered (a mapping can sit under another mapping)
        grown = False
        for pos, n in nodes.items():
            if n['class'] == MAPPING and n['state'] is not None and n['state'] not in states:
                up = chain(pos)
                if up and all(s in states and states[s] == how for s, how in up.items()):
                    states[n['state']] = None
                    grown = True
    # Owner frames number only the states that are neither client- nor server-only.
    owner = {}
    for n in sorted((n for n in nodes.values() if n['state'] is not None), key=lambda n: n['state']):
        if not (n['client_only'] or n['server_only']):
            owner[n['state']] = len(owner)
    fire = {}
    if volley is not None:  # Stacks between the button and the volley: on while a shot shows
        fire = {owner[s]: 'stack' for s, how in chain(by_state[volley]).items()
                if s not in states and s in owner and nodes[by_state[s]]['class'] == STACK}
    return ({owner[s]: how for s, how in sorted(states.items()) if s in owner},
            [owner[nodes[p]['state']] for p in models if nodes[p]['state'] in owner],
            owner.get(volley) if volley is not None else None, fire)


RELOAD_ALIAS, RELOAD_BUTTON, ALT_BUTTON = '0x0C80000000000004', 14, 29
VOLLEY, ANIM, BUTTON_VAR = 'STUStatescriptStateWeaponVolley', 'STUStatescriptStateAnim', 'm_logicalButton'


def _owner(graph):
    owner = {}
    for n in sorted((n for n in graph['nodes'] if n['state'] is not None), key=lambda n: n['state']):
        if not (n['client_only'] or n['server_only']):
            owner[n['state']] = len(owner)
    return owner


def _button(node):
    value = node['fields'].get(BUTTON_VAR)
    return value.get(BUTTON_VAR) if isinstance(value, dict) else None


def reload(graph):
    """The reload animation: ({owner state: how} from the graph's root down to it: the "reloading" switch's true
    branch and its Stack, the StateAnim's owner state). The Anim plays the shared reload alias (its length is var581)
    and sits where var51 (reload requested, set by the button-14 route) sends the graph; tools/lobby/kits.py holds
    only the path entries that differ from the kit's normal states. None: the weapon has none."""
    nodes = {n['pos']: n for n in graph['nodes']}
    owner = _owner(graph)
    parents = {}
    for pos, node in nodes.items():
        for plug, children in plugs(node['fields']):
            for child in children:
                parents.setdefault(child, []).append((pos, plug))
    for pos, n in nodes.items():
        if n['class'] == ANIM and RELOAD_ALIAS in json.dumps(n['fields'].get('m_11E0A658')) and n['state'] in owner:
            path, seen = {}, set()
            while pos in parents and pos not in seen:
                seen.add(pos)
                pos, plug = parents[pos][0]
                node = nodes[pos]
                if node['state'] in owner:
                    path[owner[node['state']]] = ((0 if 'false' in plug.lower() else 1) if node['class'] == SWITCH
                        else KINDS.get(node['class'], None if node['class'] in BARE else '?'))
            return path, owner[n['state']]
    return None


def alt(graph):
    """The alternate fire: ({owner state: how} on while the weapon shows: the LogicalButton on button 29 and the
    switches above it; {owner state: how} pulsed with a shot: the states from the button down to its WeaponVolley;
    the Volley's owner state), as Soldier's Helix route is hand-mapped (soldier.helix_states); None: no such route
    (scopes and cooldown abilities on the right mouse are not weapon volleys)."""
    nodes = {n['pos']: n for n in graph['nodes']}
    owner = _owner(graph)
    parents = {}
    for pos, node in nodes.items():
        for plug, children in plugs(node['fields']):
            for child in children:
                parents.setdefault(child, []).append((pos, plug))

    def how(pos, plug):
        node = nodes[pos]
        if node['class'] == SWITCH:
            return 0 if 'false' in plug.lower() else 1
        return KINDS.get(node['class'], None if node['class'] in BARE else '?')

    for lb in nodes.values():
        if lb['class'] != BUTTON or _button(lb) != ALT_BUTTON or lb['state'] not in owner:
            continue
        came, queue = {lb['pos']: None}, [lb['pos']]
        while queue:
            pos = queue.pop(0)
            if nodes[pos]['class'] == VOLLEY:
                pulsed, volley = {}, owner[nodes[pos]['state']]
                while came[pos] is not None:
                    above = came[pos]
                    pos, plug = above, next(plug for p, plug in parents[pos] if p == above)
                    if nodes[pos]['state'] in owner and pos != lb['pos']:
                        pulsed[owner[nodes[pos]['state']]] = how(pos, plug)
                if '?' in pulsed.values():
                    return None  # a state with an undecoded payload on the way
                shown, pos, seen = {owner[lb['state']]: 'button'}, lb['pos'], set()
                while pos in parents and pos not in seen:
                    seen.add(pos)
                    pos, plug = parents[pos][0]
                    if nodes[pos]['state'] in owner and not (nodes[pos]['client_only'] or nodes[pos]['server_only']):
                        shown.setdefault(owner[nodes[pos]['state']], how(pos, plug))
                return shown, pulsed, volley
            for _, children in plugs(nodes[pos]['fields']):
                for child in children:
                    if child not in came:
                        came[child] = pos
                        queue.append(child)
    return None


def layout(graph):
    """Owner states, instance sync vars, remote slots: tools/lobby statescript.Graph's counts."""
    owner = sum(1 for n in graph['nodes'] if n['state'] is not None and not (n['client_only'] or n['server_only']))
    return f"{owner}, {sum(1 for v in graph['sync_vars'] if v[0] is not None and v[1] == 0)}, {len(graph['remote'])}"


def instance_vars(graph):
    """{var: presence bit} for the instance-scoped sync vars (catalog sync_vars: [identifier, scope, ...])."""
    bits = [v[0] for v in graph['sync_vars'] if v[0] is not None and v[1] == 0]
    return {var: bit for bit, var in enumerate(bits)}


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('catalog')
    parser.add_argument('graph', type=lambda v: int(v, 0))
    parser.add_argument('volley', nargs='?', type=int, help="the volley's catalog state (target.WEAPON_GRAPHS)")
    parser.add_argument('--hero', help='hero name in the catalog bodies: prints its HUD texture (graph75 var1807)')
    args = parser.parse_args()
    catalog = json.load(gzip.open(args.catalog))
    graphs = {g['index']: g for g in catalog['graphs']}
    graph = graphs[args.graph]
    states, models, volley, fire = kit(graph, args.volley)
    print(f'graph=Graph(0x{graph["index"]:04X}, {layout(graph)})')
    print(f'states={states}')
    print(f'models={models} volley={volley} fire={fire}'
          f' state_count={sum(1 for n in graph["nodes"] if n["state"] is not None)}')
    print(f'reload={reload(graph)} alt={alt(graph)}')
    if args.hero:
        body = next(b for b in catalog['bodies'].values() if b['name'] == args.hero)
        texture = [e['m_value'].get('m_texture') for g in body['graphs'] if g['graph'] == 75
                   for e in g.get('m_1EB5A024') or [] if e['m_0D09D2D9'] == '0x0D8000000000070F']
        print(f'hero={body["hero"]} hud_texture={texture}')
    print(f'instance_vars={instance_vars(graph)}')


if __name__ == '__main__':
    main()
