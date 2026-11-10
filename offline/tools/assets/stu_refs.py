"""List asset references in an STU v2 payload by trying each field hash at each offset.

STU v2 stores asset references mangled with the field hash and the header CRC64
(see retail_bundle_lookup._decode_asset_ref). A decoded value is kept when it looks like a
real GUID: reserved bits clear, a known GUID prefix and a small index. Prefixes are the GUID's
leading 12 bits as printed (0x0580... graph -> 580, 0x0400... entity -> 400), not the bit-reversed
resource type that practice_range_assets.resource_type returns.

  python stu_refs.py GUID [--prefixes 580,400,...]
"""
import os
import argparse
import struct

from retail_bundle_lookup import _decode_asset_ref, find_asset_payload, load_storage, stu_header_crc64

PREFIXES = {0x120, 0x2E0, 0x300, 0x400, 0x580, 0x7C0, 0x980, 0x9A0, 0xA88, 0xBA0, 0xC80, 0xD00, 0xD80}
MAX_INDEX = 1 << 24


def field_hashes(blob):
    """Field hashes from the v2 field-bag table (header words 4 and 5: bag count, table offset)."""
    if len(blob) < 36:
        raise ValueError("truncated STU v2 header")
    nbags, btable = struct.unpack_from("<II", blob, 16)
    hashes = set()
    if nbags <= 4096 and btable + nbags * 8 <= len(blob):
        for bag in range(nbags):
            count, table = struct.unpack_from("<II", blob, btable + bag * 8)
            if count <= 512 and table + count * 8 <= len(blob):
                hashes.update(struct.unpack_from("<I", blob, table + j * 8)[0] for j in range(count))
    hashes.add(0xC71EA6BC)  # STUStatescriptGraphWithOverrides.m_graph (inline nested field)
    return hashes


def plausible(guid, prefixes=PREFIXES):
    return (guid >> 48) in prefixes and guid & 0x0000FFFF00000000 == 0 and 0 < guid & 0xFFFFFFFF < MAX_INDEX


def decode_refs(blob, prefixes=PREFIXES):
    """Sorted {(offset, field_hash, guid)} for every plausible decoded reference."""
    crc = stu_header_crc64(blob)
    found = set()
    for field in field_hashes(blob):
        for offset in range(36, len(blob) - 7):
            guid = _decode_asset_ref(struct.unpack_from("<Q", blob, offset)[0], field, crc)
            if plausible(guid, prefixes):
                found.add((offset, field, guid))
    return sorted(found)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("guid", type=lambda v: int(v, 0))
    parser.add_argument("--prefixes", type=lambda v: {int(t, 16) for t in v.split(",")}, default=PREFIXES)
    parser.add_argument("--game-install", default=os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
    args = parser.parse_args()
    storage = load_storage(args.game_install)
    try:
        blob = find_asset_payload(storage, args.guid).payload
        for offset, field, guid in decode_refs(blob, args.prefixes):
            print(f"+{offset:#06x} field {field:08X} -> {guid:#018x}")
    finally:
        for handle in storage._files.values():
            handle.close()


if __name__ == "__main__":
    main()
