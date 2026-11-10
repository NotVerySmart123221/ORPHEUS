"""A statescript graph's network layout from the retail install: what a server needs to create it.

    python graph_layout.py GUID [GUID ...] [--states]

Prints Graph(index, owner states, instance sync vars, remote slots[, silent slots]) as tools/lobby's
statescript.Graph takes it, the entity-scope variables, and the SubScript states with their child graphs.
--states lists every state: index, remote slot, class, client-only (C) / server-only (S). Silent slots are
remote entries the client reads no presence bit for: nodes that are not states (actions, effects) and
client-/server-only states (reader 0x7FF6D1C2BE30 skips them before reading).

Layout (STU v2, build 104319; checked against the pinned graph catalog for 0F4A, 0C90 and 0C8F):
- header u32[9]; [7] is the dynamic area. An array field holds an offset into it; there: u64 count, u64
  offset (from the dynamic area) of count u64 instance indexes.
- graph fields: m_nodes 9B2111C5, m_states E6E7A042, m_remoteSyncNodes 41A83970, m_syncVars 434A5521
  (inline: u32 count, then per slot a bag index, -1 for none, with identifier 0D09D2D9 and scope 56341592).
- a state's E1B0C07F = 1 marks it client-only, 9A861B79 = 1 server-only. Owner states are the others.
"""
import os
import argparse
import struct

from retail_bundle_lookup import _decode_asset_ref, find_asset_payload, load_storage, stu_header_crc64
from stu_graph import instances

NODES, STATES, REMOTE, SYNC = 0x9B2111C5, 0xE6E7A042, 0x41A83970, 0x434A5521
VAR_ID, VAR_SCOPE = 0x0D09D2D9, 0x56341592
CLIENT_ONLY, SERVER_ONLY = 0xE1B0C07F, 0x9A861B79
SUBSCRIPT = 0x37D754C8


def _array(blob, dyn, field):
    count, start = struct.unpack_from('<QQ', blob, dyn + struct.unpack('<I', field)[0])
    return list(struct.unpack_from('<%dQ' % count, blob, dyn + start)) if count else []


def sync_vars(blob):
    """[(identifier, scope)] per sync slot, None for an empty slot."""
    header = struct.unpack_from('<9I', blob)
    bag_count, bag_table, cursor = header[4], header[5], header[8]
    root = struct.unpack_from('<i', blob, cursor)[0]
    nfields, table = struct.unpack_from('<II', blob, bag_table + 8 * root)
    pos, raw = cursor + 4, None
    for i in range(nfields):
        field, length = struct.unpack_from('<II', blob, table + 8 * i)
        if not length:
            length = struct.unpack_from('<I', blob, pos)[0]
            pos += 4
        if field == SYNC:
            raw = blob[pos:pos + length]
            break
        pos += length
    if raw is None:
        return []
    crc, out, cur = stu_header_crc64(blob), [], 4
    for _ in range(struct.unpack_from('<I', raw)[0]):
        bag = struct.unpack_from('<i', raw, cur)[0]
        cur += 4
        if bag < 0:
            out.append(None)
            continue
        if bag >= bag_count:
            raise ValueError('sync slot names a missing bag')
        values = {}
        nf, ft = struct.unpack_from('<II', blob, bag_table + 8 * bag)
        for j in range(nf):
            field, length = struct.unpack_from('<II', blob, ft + 8 * j)
            if not length:
                length = struct.unpack_from('<I', raw, cur)[0]
                cur += 4
            values[field] = raw[cur:cur + length]
            cur += length
        ident = values.get(VAR_ID)
        ident = _decode_asset_ref(int.from_bytes(ident, 'little'), VAR_ID, crc) if ident and len(ident) == 8 else None
        out.append(None if ident is None else (ident & 0xFFFFFFFF, int.from_bytes(values.get(VAR_SCOPE, b''), 'little')))
    if cur != len(raw):
        raise ValueError('sync array length mismatch')
    return out


def layout(blob):
    inst = instances(blob)
    dyn = struct.unpack_from('<9I', blob)[7]
    graph = inst[0][3]
    states = _array(blob, dyn, graph[STATES]) if STATES in graph else []
    remote_nodes = _array(blob, dyn, graph[REMOTE]) if REMOTE in graph else []
    remote = {node: slot for slot, node in enumerate(remote_nodes)}
    rows = []
    for index, node in enumerate(states):
        cls, start, size, fields = inst[node]
        rows.append({'state': index, 'remote': remote.get(node), 'class': cls, 'instance': node,
                     'client_only': fields.get(CLIENT_ONLY) == b'\1', 'server_only': fields.get(SERVER_ONLY) == b'\1',
                     'bytes': blob[start:start + size]})
    variables = [v for v in sync_vars(blob) if v]
    silent = [slot for slot, node in enumerate(remote_nodes)
              if node not in states or inst[node][3].get(CLIENT_ONLY) == b'\1' or inst[node][3].get(SERVER_ONLY) == b'\1']
    return {'states': rows, 'remote_slots': len(remote), 'silent': silent,
            'owner_states': sum(not r['client_only'] and not r['server_only'] for r in rows),
            'instance_vars': sum(1 for _, scope in variables if scope == 0),
            'entity_vars': [ident for ident, scope in variables if scope == 1]}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('guids', nargs='+', type=lambda value: int(value, 16))
    parser.add_argument('--states', action='store_true')
    parser.add_argument('--game-install', default=os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
    args = parser.parse_args()
    storage = load_storage(args.game_install)
    for guid in args.guids:
        guid |= 0x0580000000000000
        blob = find_asset_payload(storage, guid).payload
        info = layout(blob)
        silent = f", frozenset({set(info['silent'])})" if info['silent'] else ''
        print(f"Graph(0x{guid & 0xFFFF:04X}, {info['owner_states']}, {info['instance_vars']}, {info['remote_slots']}{silent})"
              f"  states={len(info['states'])} entity vars={info['entity_vars']}")
        for row in info['states']:
            flag = 'C' if row['client_only'] else 'S' if row['server_only'] else ' '
            if args.states or row['class'] == SUBSCRIPT:
                print(f"  st={row['state']:3} rem={row['remote'] if row['remote'] is not None else '-':>3} {flag} {row['class']:08X}")


if __name__ == '__main__':
    main()
