"""A statescript graph's links from the retail install: which node starts which.

    python graph_edges.py GUID [GUID ...]

One line per STUGraphLink between two graph nodes: source instance, class and state index (or node), the
output plug's field hash, then the destination's. Uses the layout in graph_layout.py; a link joins an output
plug (498B0009) to an input plug (EA1269DF), and each plug names its node (m_parentNode 3868F518).
"""
import os
import argparse
import struct

from graph_layout import NODES, STATES, _array
from retail_bundle_lookup import find_asset_payload, load_storage
from stu_graph import instances

LINK, OUT_PLUG, IN_PLUG, PARENT = 0x6E63F8E1, 0x498B0009, 0xEA1269DF, 0x3868F518


def edges(blob):
    """[(source instance, output plug field hash or None, destination instance)] in link order."""
    inst = instances(blob)
    dyn = struct.unpack_from('<9I', blob)[7]
    nodes = set(_array(blob, dyn, inst[0][3][NODES]))
    owner = {i: struct.unpack('<I', fields[PARENT])[0] for i, (_, _, _, fields) in enumerate(inst) if PARENT in fields}
    plug_field = {}  # plug instance -> the field of its node that holds it
    for node in nodes:
        for key, raw in inst[node][3].items():
            if len(raw) == 4 and owner.get(struct.unpack('<I', raw)[0]) == node:  # not an int that looks like one
                plug_field[struct.unpack('<I', raw)[0]] = key
    out = []
    for cls, _, _, fields in inst:
        if cls == LINK and OUT_PLUG in fields and IN_PLUG in fields:
            plug, dest = (struct.unpack('<I', fields[key])[0] for key in (OUT_PLUG, IN_PLUG))
            if owner.get(plug) in nodes and owner.get(dest) in nodes:
                out.append((owner[plug], plug_field.get(plug), owner[dest]))
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('guids', nargs='+', type=lambda value: int(value, 16))
    parser.add_argument('--game-install', default=os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
    args = parser.parse_args()
    storage = load_storage(args.game_install)
    for guid in args.guids:
        blob = find_asset_payload(storage, guid | 0x0580000000000000).payload
        inst = instances(blob)
        state = {node: i for i, node in enumerate(_array(blob, struct.unpack_from('<9I', blob)[7], inst[0][3][STATES]))}
        name = lambda node: f"{node} {inst[node][0]:08X} {'state%d' % state[node] if node in state else 'node'}"
        for source, plug, dest in edges(blob):
            print(f"{name(source)} --{plug or 0:08X}--> {name(dest)}")


if __name__ == '__main__':
    main()
