"""Read-only lookup of a build-104319 asset stored loose or inside a CMF bundle."""
from __future__ import annotations

import os
import argparse
import functools
import hashlib
import struct
from dataclasses import dataclass

from practice_range_assets import (
    ROOT_MANIFEST,
    load_storage,
    parse_cmf,
    resource_type,
    root_manifest_key,
)

BUILD = 104319
BUNDLE_TYPE = 0x09C
STU_HERO = 0x7C3457DC
STU_HERO_GAMEPLAY_ENTITY = 0x950BBA06


@dataclass(frozen=True)
class AssetPayload:
    asset_guid: int
    size: int
    ckey: bytes
    bundle_guid: int
    offset: int
    payload: bytes


def parse_bundle_index(blob: bytes) -> dict[int, int]:
    """Read the 1.48+ bundle GUID→offset table described by OWLib Bundle.cs."""
    if len(blob) < 5:
        raise ValueError("bundle shorter than header")
    count, offset_size = struct.unpack_from("<iB", blob)
    if count < 0 or offset_size not in (1, 2, 4):
        raise ValueError("invalid bundle header")
    stride = 8 + offset_size
    table_end = 5 + count * stride
    if table_end > len(blob):
        raise ValueError("bundle index exceeds file")
    entries: dict[int, int] = {}
    for i in range(count):
        pos = 5 + i * stride
        guid = struct.unpack_from("<Q", blob, pos)[0]
        offset = int.from_bytes(blob[pos + 8:pos + stride], "little")
        if guid in entries or offset < table_end or offset > len(blob):
            raise ValueError("invalid bundle entry")
        entries[guid] = offset
    return entries


def stu_header_crc64(header: bytes) -> int:
    """OWLib's reflected CRC64 over the first 36 STU v2 header bytes."""
    if len(header) < 36:
        raise ValueError("truncated STU v2 header")
    crc = 0
    polynomial = 0x95AC9329AC4BC9B5
    for value in header[:36]:
        crc ^= value
        for _ in range(8):
            crc = (crc >> 1) ^ (polynomial if crc & 1 else 0)
    return (~crc) & 0xFFFFFFFFFFFFFFFF


def _decode_asset_ref(raw: int, field_hash: int, header_crc: int) -> int:
    field_hash64 = field_hash | (field_hash << 32)
    value = raw ^ field_hash64 ^ header_crc
    raw_bytes = bytearray(value.to_bytes(8, "little"))
    for left, right in ((0, 3), (7, 1), (2, 6), (4, 5)):
        raw_bytes[left], raw_bytes[right] = raw_bytes[right], raw_bytes[left]
    return int.from_bytes(raw_bytes, "little")


def hero_gameplay_entity_guid(blob: bytes) -> int:
    """Read the root STUHero.m_gameplayEntity reference using OWLib's v2 layout."""
    if len(blob) < 36:
        raise ValueError("truncated STU header")
    instance_count, instance_offset = struct.unpack_from("<II", blob, 0)
    field_bag_count, field_bags_offset = struct.unpack_from("<II", blob, 16)
    data_offset = struct.unpack_from("<I", blob, 32)[0]
    if instance_count < 1 or instance_offset + 16 > len(blob):
        raise ValueError("invalid STU instance table")
    root_hash = struct.unpack_from("<I", blob, instance_offset)[0]
    if root_hash != STU_HERO:
        raise ValueError(f"root STU 0x{root_hash:08X} is not STUHero")
    if field_bag_count < 1 or field_bags_offset + field_bag_count * 8 > len(blob):
        raise ValueError("invalid STU field-bag table")
    if data_offset + 4 > len(blob):
        raise ValueError("invalid STU data offset")
    bag_index = struct.unpack_from("<i", blob, data_offset)[0]
    if bag_index < 0 or bag_index >= field_bag_count:
        raise ValueError("invalid STU field-bag index")
    field_count, fields_offset = struct.unpack_from("<II", blob, field_bags_offset + bag_index * 8)
    if field_count < 0 or fields_offset + field_count * 8 > len(blob):
        raise ValueError("invalid STU field list")
    data_pos = data_offset + 4
    header_crc = stu_header_crc64(blob)
    for i in range(field_count):
        field_hash, field_size = struct.unpack_from("<II", blob, fields_offset + i * 8)
        if field_size == 0:
            if data_pos + 4 > len(blob):
                raise ValueError("truncated variable STU field")
            field_size = struct.unpack_from("<I", blob, data_pos)[0]
            data_pos += 4
        if data_pos + field_size > len(blob):
            raise ValueError("STU field exceeds asset")
        if field_hash == STU_HERO_GAMEPLAY_ENTITY:
            if field_size != 8:
                raise ValueError("unexpected gameplay-entity reference size")
            raw = struct.unpack_from("<Q", blob, data_pos)[0]
            return _decode_asset_ref(raw, field_hash, header_crc)
        data_pos += field_size
    raise LookupError("STUHero field bag does not contain m_gameplayEntity")


def find_asset_payload(storage, asset_guid: int, manifest: str = ROOT_MANIFEST) -> AssetPayload:
    if type(asset_guid) is not int or not 0 <= asset_guid <= 0xFFFFFFFFFFFFFFFF:
        raise ValueError("asset GUID must be an unsigned 64-bit integer")
    root_ckey, name = root_manifest_key(storage, manifest)
    root_blob = storage.cat(root_ckey)
    if root_blob is None or hashlib.md5(root_blob).digest() != root_ckey:
        raise RuntimeError("CMF missing or root CKey check failed")
    _, records = parse_cmf(root_blob, name)
    target = next((row for row in records if row[0] == asset_guid), None)
    if target is None:
        raise KeyError(f"asset 0x{asset_guid:016X} not in {name}")
    _, size, ckey = target
    try:
        payload = storage.cat(ckey)
    except (OSError, ValueError):
        payload = None
    if payload is not None and len(payload) == size and hashlib.md5(payload).digest() == ckey:
        return AssetPayload(asset_guid, size, ckey, 0, 0, payload)
    for bundle_guid, _, bundle_ckey in records:
        if resource_type(bundle_guid) != BUNDLE_TYPE:
            continue
        try:
            bundle = storage.cat(bundle_ckey)
            if bundle is None:
                continue
            offset = parse_bundle_index(bundle).get(asset_guid)
            if offset is None or offset + size > len(bundle):
                continue
            payload = bundle[offset:offset + size]
        except (OSError, ValueError, struct.error):
            # Some local bundle CKeys are encrypted with a key absent from casc.py.
            continue
        if hashlib.md5(payload).digest() == ckey:
            return AssetPayload(asset_guid, size, ckey, bundle_guid, offset, payload)
    raise LookupError(f"no readable bundle contains asset 0x{asset_guid:016X}")


def iter_payloads(storage, keep, manifest: str = ROOT_MANIFEST):
    """Yield (GUID, payload) for every asset of the CMF whose GUID passes keep(GUID), loose or from its bundle
    (MD5-checked); unreadable ones are skipped. The bundle indexes are parsed once."""
    ckey, name = root_manifest_key(storage, manifest)
    _, records = parse_cmf(storage.cat(ckey), name)
    bundles = {}
    for guid, _, bundle_ckey in records:
        if resource_type(guid) != BUNDLE_TYPE:
            continue
        try:
            for asset, offset in parse_bundle_index(storage.cat(bundle_ckey)).items():
                bundles.setdefault(asset, (bundle_ckey, offset))
        except (OSError, ValueError, struct.error, TypeError):
            continue
    bundle = functools.lru_cache(maxsize=16)(storage.cat)  # bundles are large and shared by many assets
    for guid, size, asset_ckey in sorted(records):
        if not keep(guid):
            continue
        try:
            blob = storage.cat(asset_ckey)
        except (OSError, ValueError):
            blob = None
        if (blob is None or len(blob) != size) and guid in bundles:
            bundle_ckey, offset = bundles[guid]
            blob = bundle(bundle_ckey)[offset:offset + size]
            if hashlib.md5(blob).digest() != asset_ckey:
                blob = None
        if blob is not None:
            yield guid, blob


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("guid", type=lambda value: int(value, 0))
    parser.add_argument("--game-install", default=os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
    parser.add_argument("--hero-gameplay-entity", action="store_true")
    args = parser.parse_args()
    storage = load_storage(args.game_install)
    try:
        result = find_asset_payload(storage, args.guid)
        suffix = ""
        if args.hero_gameplay_entity:
            body_guid = hero_gameplay_entity_guid(result.payload)
            suffix = f" gameplay_entity=0x{body_guid:016X}"
        print(
            f"build={BUILD} asset=0x{result.asset_guid:016X} size={result.size} "
            f"ckey={result.ckey.hex().upper()} md5={hashlib.md5(result.payload).hexdigest().upper()} "
            f"bundle=0x{result.bundle_guid:016X} offset=0x{result.offset:X} verified=true{suffix}"
        )
    finally:
        for handle in storage._files.values():
            handle.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
