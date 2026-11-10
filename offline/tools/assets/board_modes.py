"""Each game mode's mode graph and its remote states that post game message 06DF.025, from the retail install.

The client's Tab board (graph 01BE) stays shut until its HUD graph 0CC9 has that message, which only a mode graph's
remote 38EE1100 state posts, so the server has to turn one on (research/lizrollin-runtime.md section 22).

    python board_modes.py [MODE ...] [--game-install DIR]

MODE: a game mode's GUID or its index (10 = 0x0230000000000010); none: every mode of the lobby's map data
(tools/lobby/vendor/data/game_maps_174.json). One line per mode: the mode, its graph as tools/lobby's Graph(...)
(graph_layout.py) and the remote slots that post the message, in slot order; a mode without one has no board.
"""
import os
import argparse
import json
from pathlib import Path

from graph_layout import layout
from graph_messages import messages
from retail_bundle_lookup import find_asset_payload, load_storage
from stu_refs import decode_refs

MODE_GRAPH = 0xCF63B633  # the mode asset's field that names its mode graph
BOARD_ON = 0x02400000000006DF
POSTS = 0x38EE1100  # the state class that posts its message while on
MAPS = Path(__file__).resolve().parents[1] / 'lobby' / 'vendor' / 'data' / 'game_maps_174.json'


def board(storage, mode: int):
    """(the mode graph's GUID, its layout, the remote slots that post 06DF.025), or None for a mode without a graph."""
    blob = find_asset_payload(storage, mode).payload
    graph = next((guid for _, field, guid in decode_refs(blob, {0x580}) if field == MODE_GRAPH), None)
    if graph is None:
        return None
    blob = find_asset_payload(storage, graph).payload
    info, found = layout(blob), messages(blob)
    slots = sorted(row['remote'] for row in info['states'] if row['class'] == POSTS and row['remote'] is not None
                   and found.get(row['instance']) == BOARD_ON)
    return graph, info, slots


def modes() -> list[int]:
    """Every game mode a map of the lobby's data can be played in."""
    maps = json.loads(MAPS.read_text(encoding='utf-8'))
    return sorted({int(mode, 16) for entry in maps.values() for mode in entry.get('modes', ())})


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('modes', nargs='*', type=lambda value: int(value, 16) | 0x0230000000000000)
    parser.add_argument('--game-install', default=os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
    args = parser.parse_args()
    storage = load_storage(args.game_install)
    for mode in args.modes or modes():
        try:
            found = board(storage, mode)
        except KeyError as error:  # a mode the install lacks
            print(f'0x{mode:016X}: not in the install ({error})')
            continue
        if found is None:
            print(f'0x{mode:016X}: no mode graph')
            continue
        graph, info, slots = found
        silent = f", frozenset({set(info['silent'])})" if info['silent'] else ''
        print(f"0x{mode:016X}: Graph(0x{graph & 0xFFFF:04X}, {info['owner_states']}, {info['instance_vars']}, "
              f"{info['remote_slots']}{silent}), slots {slots}")


if __name__ == '__main__':
    main()
