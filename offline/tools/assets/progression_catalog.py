"""Extract typed OW1 reward pools and frame awards from the verified retail install."""
import argparse
import hashlib
import json
from pathlib import Path
import struct

from hero_content import DEFAULTS, PROGRESSION, SLOTS, UNLOCK, entries
from practice_range_assets import ROOT_MANIFEST, parse_cmf, resource_type, root_manifest_key
from retail_bundle_lookup import BUNDLE_TYPE, _decode_asset_ref, load_storage, parse_bundle_index, stu_header_crc64
from stu_graph import instances

PLAYER = 0x0CA0000000000018
POOLS, BOX_TYPE, RARITY = 0x473494FF, 0x7AB4E3F8, 0xBB99FCD3


def assets(storage, wanted):
    """Batch lookup: each bundle is read once, each returned asset is content-hash verified."""
    key, name = root_manifest_key(storage, ROOT_MANIFEST)
    root = storage.cat(key)
    if root is None or hashlib.md5(root).digest() != key:
        raise ValueError('root manifest content hash mismatch')
    _, rows = parse_cmf(root, name)
    records = {g: (size, key) for g, size, key in rows}
    pending, out = set(wanted), {}
    if pending - records.keys():
        raise ValueError('progression asset is absent from the root manifest')

    def accept(guid, blob):
        size, key = records[guid]
        if blob is not None and len(blob) == size and hashlib.md5(blob).digest() == key:
            out[guid] = blob
            pending.remove(guid)

    for guid in tuple(pending):
        try:
            accept(guid, storage.cat(records[guid][1]))
        except (OSError, ValueError):
            pass  # A readable bundle may contain the same content.
    for guid, _, key in rows:
        if not pending:
            break
        if resource_type(guid) != BUNDLE_TYPE:
            continue
        try:
            bundle = storage.cat(key)
        except (OSError, ValueError):
            continue  # Unrelated event bundles can require keys absent from the read-only extractor.
        if bundle is None or hashlib.md5(bundle).digest() != key:
            continue
        index = parse_bundle_index(bundle)
        for target in pending.intersection(index):
            start, size = index[target], records[target][0]
            accept(target, bundle[start:start + size])
    if pending:
        raise ValueError(f'{len(pending)} progression assets failed content verification: '
                         + ', '.join(f'{g:016X}' for g in sorted(pending)[:10]))
    return out


def unlock_refs(blob, raw):
    """STUUnlocks or an unlock-reference array's serialized dynamic-array pointer."""
    if len(raw) not in (4, 8):
        raise ValueError('invalid unlock-list reference')
    data = struct.unpack_from('<9I', blob)[7]
    header = data + int.from_bytes(raw[-4:], 'little')
    count, first = struct.unpack_from('<QQ', blob, header)
    if count > 10000 or data + first + count * 16 > len(blob):
        raise ValueError('unlock list exceeds its asset')
    crc = stu_header_crc64(blob)
    result = [_decode_asset_ref(struct.unpack_from('<Q', blob, data + first + i * 16 + 8)[0], UNLOCK, crc)
              for i in range(count)]
    if any(g >> 52 != 0x025 for g in result):
        raise ValueError('unlock list contains a non-unlock GUID')
    return result


def build(storage, heroes):
    first = assets(storage, {int(h, 16) for h in heroes} | {PLAYER})
    progressions = {}
    for hero in heroes:
        blob = first[int(hero, 16)]
        raw = instances(blob)[0][3][PROGRESSION]
        progressions[hero] = _decode_asset_ref(int.from_bytes(raw, 'little'), PROGRESSION, stu_header_crc64(blob))
    blobs = assets(storage, progressions.values())
    player = first[PLAYER]
    pools, defaults, memberships, frames, currency_rarities = {}, set(), {}, [], {}
    for hero, entry in heroes.items():
        for guid in entry['unlocks']:
            memberships.setdefault(int(guid, 16), set()).add(int(hero, 16))
    for hero, blob in [(h, blobs[g]) for h, g in progressions.items()] + [(None, player)]:
        fields = instances(blob)[0][3]
        for row in entries(blob, fields.get(POOLS, b'\0' * 4)):
            kind = int.from_bytes(row.get(BOX_TYPE, b'\0' * 4), 'little', signed=True)
            for guid in unlock_refs(blob, row[UNLOCK]) if UNLOCK in row else []:
                pools.setdefault(guid, set()).add(kind)
        for row in entries(blob, fields.get(DEFAULTS, b'\0' * 4)) if hero else []:
            defaults.update(unlock_refs(blob, row[UNLOCK]))
    fields = instances(player)[0][3]
    for row in entries(player, fields[0x9A4245F2]):
        level = max(1, int.from_bytes(row.get(0x2C01908B, b'\0' * 4), 'little'))
        frames.extend((level, guid) for guid in unlock_refs(player, row[UNLOCK]))
    for row in entries(player, fields[0x03F27C01]):
        kind = int.from_bytes(row.get(BOX_TYPE, b'\0' * 4), 'little', signed=True)
        for currency in entries(player, row[UNLOCK]):
            guid = _decode_asset_ref(int.from_bytes(currency[0xF5087894], 'little'), 0xF5087894, stu_header_crc64(player))
            pools.setdefault(guid, set()).add(kind)
            currency_rarities[guid] = int.from_bytes(currency.get(RARITY, b'\0' * 4), 'little')
    wanted = set(memberships) | set(pools) | defaults | {g for _, g in frames}
    items, frame_guids = {}, set()
    initial = {guid for level, guid in frames if level == 1}
    for guid, blob in assets(storage, wanted).items():
        cls, _, _, values = instances(blob)[0]
        rarity = currency_rarities.get(guid, int.from_bytes(values.get(RARITY, b'\0' * 4), 'little', signed=True))
        members = sorted(memberships.get(guid, []))
        item = {'hero': members[0] if len(members) == 1 else 0, 'heroes': members,
                'rarity': rarity, 'slot': 'Icon' if cls == 0x54BC2188 else SLOTS.get(cls), 'boxes': sorted(pools.get(guid, [])),
                'default': guid in defaults or guid in initial}
        if cls == 0x6FE91269:
            frame_guids.add(guid)
        if cls == 0x614BC677:
            item['currency'] = int.from_bytes(values[0x4CE56BC0], 'little', signed=True)
        if 0 in item['boxes'] and item['slot'] in SLOTS.values() and 0 <= rarity <= 3 and not item['default']:
            item['price'] = (25, 75, 250, 1000)[rarity]
        items[f'{guid:016X}'] = item
    return {'version': 2, 'items': items,
            'frames': sorted([[level, f'{guid:016X}'] for level, guid in frames if guid in frame_guids])}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('out', type=Path)
    parser.add_argument('--game-install', required=True)
    parser.add_argument('--heroes', type=Path, required=True, help='existing hero_content.json')
    args = parser.parse_args()
    storage = load_storage(args.game_install)
    try:
        keys = json.loads((Path(__file__).resolve().parents[1] / 'lobby' / 'vendor' / 'data'
                           / 'resource_keys_174.json').read_text(encoding='utf-8'))
        storage.keys.update({int(entry['name'], 16): bytes.fromhex(entry['key'])
                             for entry in keys.values() if entry.get('name') and entry.get('key')})
        catalog = build(storage, json.loads(args.heroes.read_text(encoding='utf-8')))
    finally:
        for handle in storage._files.values():
            handle.close()
    pending = args.out.with_suffix('.tmp')
    try:
        pending.write_text(json.dumps(catalog, sort_keys=True), encoding='utf-8')
        pending.replace(args.out)
    finally:
        pending.unlink(missing_ok=True)


if __name__ == '__main__':
    main()
