"""Generate the lobby's owner delta layouts from the pinned 1.74 graph catalog; graphs the catalog lacks (D.Va's)
are read from the retail install (`--game-install`)."""
import os
import argparse
import gzip
import json
import sys
from pathlib import Path

from ability_kit import owner_states
from graph_layout import layout, sync_vars
from retail_bundle_lookup import find_asset_payload, load_storage

LOBBY = Path(__file__).resolve().parents[1] / 'lobby'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('catalog', type=Path)
    parser.add_argument('--game-install', default=os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
    args = parser.parse_args()

    sys.path.insert(0, str(LOBBY))
    import practice  # installs the lobby's pinned vendor path
    import kits

    with gzip.open(args.catalog) as stream:
        graphs = {graph['index']: graph for graph in json.load(stream)['graphs']}
    used = {kit.graph.index for kit in kits.KITS.values()}
    used.update(ability.graph.index for abilities in kits.ABILITIES.values() for ability in abilities)
    used.update(kit.graph.index for kit in (kits.MERCY_BLASTER, kits.BASTION_SENTRY))
    # quick melee, Soldier's body graphs, the common animation graph, Widowmaker's HUD, Bastion's body, the HUD
    # texture host; 4429 is not in the catalog (lobby falls back to a full frame if it ever changes)
    used.update((67, 596, 597, 599, 601, 75, 118, 303, 463))
    # D.Va: the mech HUD, the Matrix meter child, the pilot's Light Gun, both ultimates' graphs
    used.update((kits.DVA_HUD.index, 0x214E, kits.DVA_PILOT.graph.index,
                 *(instance.graph.index for instance, _ in kits.DVA_ULTS.values())))
    storage = None
    rows = []
    for index in sorted(used):
        graph = graphs.get(index)
        if graph is not None:
            count = max(node['state'] for node in graph['nodes'] if node['state'] is not None) + 1
            excluded = sorted(set(range(count)) - owner_states(graph).keys())
            variables = graph['sync_vars']
            sync = [slot for slot, var in enumerate(variables) if var[0] is not None and var[1] == 0]
        else:  # same rows as the catalog's (checked on 75, 463 and 596)
            storage = storage or load_storage(args.game_install)
            blob = find_asset_payload(storage, 0x0580000000000000 | index).payload
            states, variables = layout(blob)['states'], sync_vars(blob)
            count = len(states)
            excluded = [row['state'] for row in states if row['client_only'] or row['server_only']]
            sync = [slot for slot, var in enumerate(variables) if var is not None and var[1] == 0]
        rows.append(f'  "{index}": {json.dumps([count, excluded, len(variables), sync])}')
    output = LOBBY / 'vendor' / 'data' / 'owner_delta_layouts_174.json'
    output.write_text('{\n' + ',\n'.join(rows) + '\n}\n', encoding='utf-8')


if __name__ == '__main__':
    main()
