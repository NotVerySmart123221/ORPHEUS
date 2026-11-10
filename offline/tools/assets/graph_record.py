"""Graph catalog records for graphs the pinned catalog lacks, read from the retail install with our STU reader, for
the statescript oracle (tools/lobby/oracle.py).

    python graph_record.py OUT.json GUID [GUID ...] [--game-install DIR]

The output is {"graphs": [record, ...]} in the format of the vendored catalog (tools/lobby/vendor/ow174/game/script/
graph.py), with only the parts decode.py reads:
- per node: its class (the codec table's name for it, so decode's StEC test matches), its m_states index, its
  client-only and server-only flags, and whether it is a state;
- the graph's m_states, m_remoteSyncNodes and m_syncVars;
- the header's bit counts. In all 373 catalog graphs these are max(1, (n - 1).bit_length()) of their list's length.
Node positions are our reader's instance indexes. A remote node is a state exactly when it is in m_states, which
also holds in all 373 catalog graphs.
"""
import os
import argparse
import json
import struct
from pathlib import Path

from graph_layout import CLIENT_ONLY, NODES, REMOTE, SERVER_ONLY, STATES, _array, sync_vars
from retail_bundle_lookup import find_asset_payload, load_storage
from stu_graph import instances

CODECS = Path(__file__).resolve().parents[1] / 'lobby' / 'vendor' / 'data' / 'statescript_codecs_174.json'
IS_STATE = 'm_2BBEEAB8'


def bit_count(length):
    return max(1, (length - 1).bit_length())


def record(index, blob, names):
    """The catalog record of graph `index` (its 16-bit index) from its STU blob; `names`: codec hash -> class name."""
    inst = instances(blob)
    dyn = struct.unpack_from('<9I', blob)[7]  # the dynamic area
    fields = inst[0][3]
    nodes, states, remote = (_array(blob, dyn, fields[key]) if key in fields else [] for key in (NODES, STATES, REMOTE))
    rid = {position: number for number, position in enumerate(nodes)}
    slot = {position: number for number, position in enumerate(remote)}
    state = {position: number for number, position in enumerate(states)}
    out = []
    for position in sorted(set(states) | set(remote)):
        cls, _, _, values = inst[position]
        out.append({'pos': position, 'class': names.get(f'{cls:08X}', f'STU_{cls:08X}'), 'uid': 0,
                    'rid': rid.get(position, -1), 'remote': slot.get(position, -1), 'state': state.get(position),
                    'client_only': values.get(CLIENT_ONLY) == b'\1', 'server_only': values.get(SERVER_ONLY) == b'\1',
                    'fields': {IS_STATE: 1} if position in state else {}})
    sync = [[None, 0, 0] if entry is None else [entry[0], entry[1], 0] for entry in sync_vars(blob)]
    header = {'m_nodesBitCount': bit_count(len(nodes)), 'm_statesBitCount': bit_count(len(states)),
              'm_remoteSyncNodesBitCount': bit_count(len(remote)), 'm_syncVarsBitCount': bit_count(len(sync))}
    return {'index': index, 'header': header, 'fields': {}, 'items': len(inst), 'containers': {}, 'nodes': out,
            'unknown_nodes': [], 'states': states, 'entries': [], 'remote': remote, 'sync_vars': sync}


def codec_names():
    return {key: entry['name'] for key, entry in json.loads(CODECS.read_text(encoding='utf-8'))['classes'].items()}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('out', type=Path)
    parser.add_argument('guids', nargs='+', type=lambda value: int(value, 16))
    parser.add_argument('--game-install', default=os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
    args = parser.parse_args()
    storage, names = load_storage(args.game_install), codec_names()
    graphs = [record(guid & 0xFFFF, find_asset_payload(storage, guid | 0x0580000000000000).payload, names)
              for guid in args.guids]
    args.out.write_text(json.dumps({'graphs': graphs}, separators=(',', ':')) + '\n', encoding='utf-8')
    print(f'{len(graphs)} graphs -> {args.out}')


if __name__ == '__main__':
    main()
