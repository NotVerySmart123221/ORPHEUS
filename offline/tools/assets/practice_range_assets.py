"""Read-only build-104319 CMF indexer for OW retail CASC.

CMF layout and cryptography follow TACTLib's ContentManifestFile and
ProCMF_104319 implementations (MIT); see research/practice-range-assets.md.
"""
from __future__ import annotations

import os
import argparse
import csv
import functools
import hashlib
import importlib.util
import io
import struct
import sys
import types
from pathlib import Path

KEYTABLE_FILE = Path(__file__).resolve().parents[1] / "lobby" / "vendor" / "data" / "cmf_keytable_104319.bin"
BUILD = 104319
ROOT_MANIFEST = "Manifest/Win_SPWin_RDEV_EExt.cmf"
HEADER_SIZE = 48
HASH_SIZE = 29
MAPHEADER_TYPE = 0x9F


@functools.cache
def keytable() -> bytes:
    """Build 104319's 512-byte manifest key table: a local input (tools/lobby/vendor/data/README.md)."""
    table = KEYTABLE_FILE.read_bytes()
    if len(table) != 512:
        raise ValueError("cmf_keytable_104319.bin must hold 512 bytes")
    return table


def signed_mod(value: int, divisor: int) -> int:
    value &= 0xFFFFFFFF
    if value >= 0x80000000:
        value -= 0x100000000
    return value % divisor


def cmf_key(build: int, length: int = 32) -> bytes:
    kidx = keytable()[length + 256]
    out = bytearray()
    for _ in range(length):
        out.append(keytable()[signed_mod(kidx, 512)])
        mode = signed_mod(kidx, 3)
        if mode == 0:
            kidx = (kidx + 103) & 0xFFFFFFFF
        elif mode == 1:
            kidx = signed_mod((kidx * 4) & 0xFFFFFFFF, build)
        else:
            kidx = (kidx - 1) & 0xFFFFFFFF
    return bytes(out)


def cmf_iv(filename: str, length: int = 16) -> bytes:
    digest = hashlib.sha1(filename.encode("ascii")).digest()
    origin = kidx = 2 * digest[5]
    out = bytearray()
    for i in range(length):
        value = keytable()[signed_mod(kidx, 512)]
        kidx += 37 if digest[6] & 1 else origin % 61
        out.append(value ^ digest[signed_mod(kidx - i, len(digest))])
    return bytes(out)


def resource_type(guid: int) -> int:
    bits = (guid >> 48) & 0xFFF
    return int(f"{bits:012b}"[::-1], 2) + 1


def parse_cmf(blob: bytes, filename: str) -> tuple[tuple[int, ...], list[tuple[int, int, bytes]]]:
    if len(blob) < HEADER_SIZE:
        raise ValueError("CMF shorter than v26 header")
    fields = struct.unpack_from("<12I", blob, 0)
    build, data_count, entry_count, magic = fields[0], fields[8], fields[10], fields[11]
    if build != BUILD:
        raise ValueError(f"CMF build {build}, expected {BUILD}")
    if magic >> 8 != 0x636D66:
        raise ValueError(f"unexpected CMF magic 0x{magic:08x}")
    encrypted = blob[HEADER_SIZE:]
    if len(encrypted) % 16:
        raise ValueError("encrypted CMF payload is not AES block-aligned")
    from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
    decryptor = Cipher(algorithms.AES(cmf_key(build)), modes.CBC(cmf_iv(filename))).decryptor()
    plain = decryptor.update(encrypted) + decryptor.finalize()
    entries_size = entry_count * 20
    hashes_size = data_count * HASH_SIZE
    if entries_size + hashes_size > len(plain):
        raise ValueError("CMF counts exceed decrypted payload")
    records = []
    offset = entries_size
    for i in range(data_count):
        pos = offset + i * HASH_SIZE
        guid, size = struct.unpack_from("<QI", plain, pos)
        records.append((guid, size, plain[pos + 13:pos + 29]))
    return fields, records


def load_storage(game_install: str):
    sys.dont_write_bytecode = True
    paths = types.ModuleType("owpaths")
    paths.GAME_INSTALL = str(Path(game_install))
    sys.modules["owpaths"] = paths
    casc_path = Path(__file__).with_name("casc.py")
    spec = importlib.util.spec_from_file_location("ow_beta_casc_readonly", casc_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load read-only CASC helper: {casc_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    storage = module.Storage()
    storage.keys = module.KEYS  # callers extracting event content can supply the existing login keyring
    if storage.config.get("build-name", [None])[0] != "prometheus-1_74_0_0-104319":
        for handle in storage._files.values():
            handle.close()
        raise RuntimeError("retail install is not build 104319")
    return storage


def root_manifest_key(storage, manifest_path: str) -> tuple[bytes, str]:
    root = storage.root()
    if root is None:
        raise RuntimeError("CASC root unavailable")
    rows = csv.reader(io.StringIO(root.decode("utf-8")), delimiter="|")
    for row in rows:
        if row and row[0].lower() == manifest_path.lower():
            return bytes.fromhex(row[1]), Path(row[0]).name
    raise FileNotFoundError(manifest_path)


def map_headers(storage) -> list[tuple[int, int, bytes]]:
    root_ckey, name = root_manifest_key(storage, ROOT_MANIFEST)
    blob = storage.cat(root_ckey)
    if blob is None or hashlib.md5(blob).digest() != root_ckey:
        raise RuntimeError("CMF missing or CASC content-key check failed")
    _, records = parse_cmf(blob, name)
    return [record for record in records if resource_type(record[0]) == MAPHEADER_TYPE]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--game-install", default=os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
    parser.add_argument("--map-index", type=lambda value: int(value, 0), help="also show map type 0x002 with this low 32-bit index")
    args = parser.parse_args()
    storage = load_storage(args.game_install)
    try:
        root_ckey, name = root_manifest_key(storage, ROOT_MANIFEST)
        blob = storage.cat(root_ckey)
        if blob is None or hashlib.md5(blob).digest() != root_ckey:
            raise RuntimeError("CMF missing or CASC content-key check failed")
        _, records = parse_cmf(blob, name)
        headers = [row for row in records if resource_type(row[0]) == MAPHEADER_TYPE]
        rows = headers if args.map_index is None else [
            row for row in records
            if (row[0] & 0xFFFFFFFF) == args.map_index
            and resource_type(row[0]) in (0x002, MAPHEADER_TYPE)
        ]
        encoding = storage.encoding()
        print(f"build={BUILD} mapheader_count={len(headers)} manifest={ROOT_MANIFEST}")
        for guid, size, ckey in rows:
            ekey = encoding.get(ckey)
            data = storage.cat(ckey) if ekey else None
            verified = data is not None and hashlib.md5(data).digest() == ckey
            print(f"guid=0x{guid:016X} type=0x{resource_type(guid):03X} map_index=0x{guid & 0xFFFFFFFF:08X} size={size} ckey={ckey.hex().upper()} loose_encoding={bool(ekey)} loose_md5={verified}")
    finally:
        for handle in storage._files.values():
            handle.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
