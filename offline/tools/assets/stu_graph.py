"""Dump the instances of an STU v2 asset (statescript graphs and others) from the retail install.

Each instance: index, class hash, and each serialized field (hash, size, raw hex), with any
mangled asset references that fall inside the instance decoded by stu_refs.

  python stu_graph.py GUID [--classes] [--class 75001A77] [--width 48]
"""
import os
import argparse
import struct
from collections import Counter

import stu_names

from retail_bundle_lookup import find_asset_payload, load_storage
from stu_refs import decode_refs


def instances(blob):
    """[(class_hash, start, size, {field_hash: bytes})] in instance order."""
    count, table, _, _, bags, bag_table, _, _, cursor = struct.unpack_from("<9I", blob)
    out = []
    for i in range(count):
        cls, _, _, size = struct.unpack_from("<4I", blob, table + 16 * i)
        start, cursor = cursor, cursor + size
        fields = {}
        bag = struct.unpack_from("<i", blob, start)[0]
        if 0 <= bag < bags:
            nf, ft = struct.unpack_from("<II", blob, bag_table + 8 * bag)
            pos = start + 4
            for j in range(nf):
                key, n = struct.unpack_from("<II", blob, ft + 8 * j)
                if n == 0:
                    n = struct.unpack_from("<I", blob, pos)[0]
                    pos += 4
                fields[key] = blob[pos:pos + n]
                pos += n
        out.append((cls, start, size, fields))
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("guid", type=lambda v: int(v, 0))
    parser.add_argument("--classes", action="store_true", help="only print the class histogram")
    parser.add_argument("--class", dest="only", type=lambda v: int(v, 16), help="only instances of this class")
    parser.add_argument("--width", type=int, default=48, help="max hex bytes per field")
    parser.add_argument("--game-install", default=os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
    args = parser.parse_args()
    storage = load_storage(args.game_install)
    try:
        blob = find_asset_payload(storage, args.guid).payload
    finally:
        for handle in storage._files.values():
            handle.close()
    rows = instances(blob)
    print(f"{args.guid:#018x}: {len(blob)} bytes, {len(rows)} instances")
    for cls, n in Counter(r[0] for r in rows).most_common():
        print(f"  class {cls:08X} x{n} {stu_names.load().get(f'{cls:08X}', {}).get('name', '')}")
    if args.classes:
        return
    refs = decode_refs(blob)
    names = stu_names.load()
    for i, (cls, start, size, fields) in enumerate(rows):
        if args.only is not None and cls != args.only:
            continue
        name = names.get(f"{cls:08X}", {}).get("name", "")
        print(f"[{i}] {cls:08X} {name} @{start:#x} size {size}")
        for key, value in fields.items():
            tail = "..." if len(value) > args.width else ""
            field = stu_names.field_name(names, f"{cls:08X}", f"{key:08X}") or ""
            print(f"    {key:08X} {field} ({len(value)}) {value[:args.width].hex()}{tail}")
        for offset, field, guid in refs:
            if start <= offset < start + size:
                print(f"    ref +{offset - start:#x} {field:08X} -> {guid:#018x}")


if __name__ == "__main__":
    main()
