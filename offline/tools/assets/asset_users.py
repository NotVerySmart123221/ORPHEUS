"""Which assets of the retail install reference these GUIDs: the reverse of stu_refs.py.

    python asset_users.py GUID [GUID ...] [--types 580,400,...] [--game-install DIR]

STU v2 stores a reference as swap(GUID) ^ (field hash | field hash << 32) ^ header CRC64
(retail_bundle_lookup._decode_asset_ref; the byte swap is its own inverse). So each asset's field hashes and header
CRC give the exact 8-byte values to look for, and the search is one bytes.find per value. --types: the GUID's leading
16 bits as printed (580 graphs, 400 entities, 230 modes); every type by default. Prints one line per hit:
asset GUID, offset, field hash, the referenced GUID.
"""
import os
import argparse
import struct
import sys

from retail_bundle_lookup import iter_payloads, load_storage, stu_header_crc64
from stu_refs import field_hashes

SWAP = ((0, 3), (7, 1), (2, 6), (4, 5))


def swap(guid: int) -> int:
    raw = bytearray(guid.to_bytes(8, 'little'))
    for left, right in SWAP:
        raw[left], raw[right] = raw[right], raw[left]
    return int.from_bytes(raw, 'little')


def encoded(guid: int, field: int, crc: int) -> bytes:
    """The 8 bytes an asset with this header CRC stores for a reference to guid in this field."""
    return struct.pack('<Q', swap(guid) ^ (field | field << 32) ^ crc)


def users(blob: bytes, targets) -> list:
    """[(offset, field hash, target)] for every reference to a target in one STU v2 payload."""
    if len(blob) < 36:
        return []
    crc = stu_header_crc64(blob)
    try:
        fields = field_hashes(blob)
    except (ValueError, struct.error):
        return []
    found = []
    for field in fields:
        for target in targets:
            needle = encoded(target, field, crc)
            at = blob.find(needle, 36)
            while at >= 0:
                found.append((at, field, target))
                at = blob.find(needle, at + 1)
    return sorted(found)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('guids', nargs='+', type=lambda value: int(value, 16))
    parser.add_argument('--types', type=lambda value: {int(t, 16) for t in value.split(',')})
    parser.add_argument('--game-install', default=os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
    args = parser.parse_args()
    storage = load_storage(args.game_install)
    keep = (lambda guid: guid >> 48 in args.types) if args.types else (lambda guid: True)
    scanned = 0
    for guid, blob in iter_payloads(storage, keep):
        scanned += 1
        for offset, field, target in users(blob, args.guids):
            print(f'{guid:#018x} +{offset:#06x} field {field:08X} -> {target:#018x}', flush=True)
    print(f'{scanned} assets scanned', file=sys.stderr)


if __name__ == '__main__':
    main()
