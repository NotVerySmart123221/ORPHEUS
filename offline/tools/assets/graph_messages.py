"""A statescript graph's game messages from the retail install: which states post or wait for which message, and
which graphs in the whole install use a message.

  python graph_messages.py GUID [GUID ...]      per graph: state index, remote slot, class, message GUID; then the
                                                entries/actions that send or wait for a message
  python graph_messages.py --find 74E [717 ...]  every graph whose nodes reference these 0x0240 messages

A message-carrying node names a ConfigVarGameMessage instance (field m_gameMessage 92A85396, a 4-byte instance
index); that instance holds the mangled asset reference (the same field hash, 8 bytes), decoded with the STU
header CRC as retail_bundle_lookup does. Classes: 38EE1100 posts its message while the state is on (the mode
graphs' phase states), 9D6CF8AC is a state entered when the message arrives, 8A7E9DA8 an entry that starts on it,
BD6F5F83 an action that sends it. --find parses the CMF and the bundle indexes once, then every 0x0580 asset.
"""
import os
import argparse
import struct
import sys

import graph_layout
import stu_graph
import stu_names
from retail_bundle_lookup import _decode_asset_ref, find_asset_payload, iter_payloads, load_storage, stu_header_crc64

MESSAGE = 0x92A85396
MESSAGE_VAR = 0xA5447DAD  # STUConfigVarGameMessage
SENDERS = {0x38EE1100: 'posts', 0x9D6CF8AC: 'state on', 0x8A7E9DA8: 'entry on', 0xBD6F5F83: 'sends'}


def messages(blob):
    """{instance index: message GUID} for every node of the graph that names a game message."""
    rows = stu_graph.instances(blob)
    crc = stu_header_crc64(blob)
    guids = {}
    for i, (cls, _, _, fields) in enumerate(rows):
        if cls == MESSAGE_VAR and len(fields.get(MESSAGE, b'')) == 8:
            guids[i] = _decode_asset_ref(struct.unpack('<Q', fields[MESSAGE])[0], MESSAGE, crc)
    out = {}
    for i, (cls, _, _, fields) in enumerate(rows):
        if cls != MESSAGE_VAR and len(fields.get(MESSAGE, b'')) == 4:
            index = struct.unpack('<i', fields[MESSAGE])[0]
            if index in guids:
                out[i] = guids[index]
    return out


def report(guid, blob):
    rows = stu_graph.instances(blob)
    found = messages(blob)
    by_instance = {row['instance']: row for row in graph_layout.layout(blob)['states']}
    print(f'{guid:#018x}: {len(found)} message nodes')
    order = lambda item: (by_instance[item[0]]['state'] if item[0] in by_instance else 10 ** 6, item[0])
    for i, message in sorted(found.items(), key=order):
        cls = rows[i][0]
        role = SENDERS.get(cls, '?')
        name = stu_names.load().get(f'{cls:08X}', {}).get('name', '')
        if i in by_instance:
            row = by_instance[i]
            slot = row['remote'] if row['remote'] is not None else '-'
            flag = 'C' if row['client_only'] else 'S' if row['server_only'] else ' '
            print(f"  st {row['state']:3} rem {slot:>3} {flag} {cls:08X} {role:9} {message:#x} {name}")
        else:
            print(f'  node {i:4}         {cls:08X} {role:9} {message:#x} {name}')


def find(storage, wanted):
    """Yield (graph GUID, instance, class, message) over every graph in the root CMF."""
    for g, blob in iter_payloads(storage, lambda guid: guid >> 48 == 0x0580):
        try:
            found = messages(blob)
        except (ValueError, struct.error, IndexError):
            continue
        rows = None
        for i, message in found.items():
            if message in wanted:
                rows = rows or stu_graph.instances(blob)
                yield g, i, rows[i][0], message


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('guids', nargs='*', type=lambda value: int(value, 16))
    parser.add_argument('--find', nargs='+', type=lambda value: 0x0240000000000000 | int(value, 16))
    parser.add_argument('--game-install', default=os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
    args = parser.parse_args()
    storage = load_storage(args.game_install)
    for guid in args.guids:
        guid |= 0x0580000000000000
        report(guid, find_asset_payload(storage, guid).payload)
    if args.find:
        for g, i, cls, message in find(storage, set(args.find)):
            print(f'{g:#x} node {i} {cls:08X} {SENDERS.get(cls, "?"):9} {message:#x}', flush=True)


if __name__ == '__main__':
    main()
