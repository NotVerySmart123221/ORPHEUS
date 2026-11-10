"""A map's placeable markers of one entity definition, from the retail install, as the lobby's spawn list.

  python map_markers.py MAPINDEX MODEGUID ENTITYGUID [--game-install DIR]
  python map_markers.py 6FE 0x023000000000000F 0x04000000000016DC > tools/lobby/data/pve_markers.json

Reads the map's entity placeable chunk 0x0DD0000B0000IIII (header: u32 count, instance-data offset, placeable
offset, 0; then per placeable a 16-byte id, u32 kind, u32 size, u64 entity definition, u64 identifier, position
x y z at +0x30, scale at +0x3C, quaternion x y z w at +0x48) and keeps the placeables the mode uses (the map
catalog's placeables_by_mode, catalog index - first_entity = record index) whose entity is the one given. Prints
JSON: {"MAPGUID": [[x, y, z, yaw], ...]} in record order, yaw in degrees from the quaternion's turn about Y.
shortcut: the yaw's sign and zero are not checked against the catalog's spawns; the bots turn to their targets.
"""
import os
import argparse
import json
import math
import struct
import sys
from pathlib import Path

from retail_bundle_lookup import find_asset_payload, load_storage

CHUNK = 0x0DD0000B00000000
MAPS = Path(__file__).resolve().parents[1] / 'lobby' / 'vendor' / 'data' / 'game_maps_174.json'


def records(blob):
    """[(entity definition, (x, y, z), yaw degrees)] for every placeable record."""
    count, _, pos, _ = struct.unpack_from('<4I', blob, 0)
    out = []
    for _ in range(count):
        size, entity = struct.unpack_from('<IQ', blob, pos + 20)
        x, y, z = struct.unpack_from('<3f', blob, pos + 0x30)
        _, qy, _, qw = struct.unpack_from('<4f', blob, pos + 0x48)
        out.append((entity, (x, y, z), math.degrees(2 * math.atan2(qy, qw))))
        pos += size
    return out


def markers(blob, entry, mode_guid, entity):
    indexes = entry.get('placeables_by_mode', {}).get(f'0x{mode_guid:016X}', entry.get('placeables', []))
    rows = records(blob)
    first = entry['first_entity']
    return [[*map(lambda v: round(v, 3), position), round(yaw, 1)]
            for index in indexes
            for definition, position, yaw in [rows[index - first]] if definition == entity]


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('map_index', type=lambda value: int(value, 16))
    parser.add_argument('mode_guid', type=lambda value: int(value, 16))
    parser.add_argument('entity', type=lambda value: int(value, 16))
    parser.add_argument('--game-install', default=os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
    parser.add_argument('--maps', default=str(MAPS))
    args = parser.parse_args()
    map_guid = 0x0800000000000000 | args.map_index
    entry = json.loads(Path(args.maps).read_text(encoding='utf-8'))[f'0x{map_guid:016X}']
    blob = find_asset_payload(load_storage(args.game_install), CHUNK | args.map_index).payload
    found = markers(blob, entry, args.mode_guid, args.entity)
    print(f'{len(found)} markers of {args.entity:#x} in mode {args.mode_guid:#x}', file=sys.stderr)
    json.dump({f'0x{map_guid:016X}': found}, sys.stdout, separators=(',', ':'))
    print()


if __name__ == '__main__':
    main()
