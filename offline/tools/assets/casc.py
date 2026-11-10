"""Read-only access to the beta's local CASC storage (data/casc): indices, BLTE, encoding, root.

  python casc.py root                 # the root file (the list of manifests)
  python casc.py cat <ckey> [out]     # a file by content key
  python casc.py ecat <ekey> [out]    # a file by encoding key
The build configuration names the root and encoding files.
"""
import glob
import hashlib
import json
import os
import struct
import sys
import zlib
from owpaths import GAME_INSTALL

CASC = os.path.join(GAME_INSTALL, "data", "casc")


def build_config():
    cfg = {}
    for path in glob.glob(os.path.join(CASC, "config", "*", "*", "*")):
        text = open(path, "rb").read().decode("utf-8", "replace")
        if text.startswith("# Build Configuration"):
            for line in text.splitlines():
                if " = " in line:
                    k, v = line.split(" = ", 1)
                    cfg[k.strip()] = v.strip().split()
    return cfg


class Storage:
    def __init__(self):
        self.index = {}   # first 9 bytes of the encoding key -> (archive, offset, size)
        self._files = {}
        self._load_indices()
        self.config = build_config()
        self._encoding = None

    def _load_indices(self):
        latest = {}
        for path in glob.glob(os.path.join(CASC, "data", "*.idx")):
            name = os.path.basename(path)
            bucket, version = int(name[:2], 16), int(name[2:10], 16)
            if bucket not in latest or version > latest[bucket][0]:
                latest[bucket] = (version, path)
        for _, path in latest.values():
            data = open(path, "rb").read()
            header_size = struct.unpack_from("<I", data, 0)[0]
            _, _, _, len_size, loc_size, key_size, _ = struct.unpack_from("<HBBBBBB", data, 8)
            pos = (8 + header_size + 15) & ~15
            entries_size = struct.unpack_from("<I", data, pos)[0]
            pos += 8
            entry = key_size + loc_size + len_size
            for i in range(entries_size // entry):
                e = data[pos + i * entry:pos + (i + 1) * entry]
                key = e[:key_size]
                loc = int.from_bytes(e[key_size:key_size + loc_size], "big")
                size = int.from_bytes(e[key_size + loc_size:], "little")
                archive, offset = loc >> 30, loc & 0x3FFFFFFF
                self.index.setdefault(key, (archive, offset, size))

    def _archive(self, n):
        if n not in self._files:
            self._files[n] = open(os.path.join(CASC, "data", "data.%03d" % n), "rb")
        return self._files[n]

    def raw(self, ekey):
        loc = self.index.get(ekey[:9])
        if not loc:
            return None
        archive, offset, size = loc
        f = self._archive(archive)
        f.seek(offset)
        blob = f.read(size)
        return blob[30:]    # 16-byte key, size, flags, checksums

    def ecat(self, ekey):
        blob = self.raw(ekey)
        return blte(blob) if blob is not None else None

    def encoding(self):
        if self._encoding is None:
            ekey = bytes.fromhex(self.config["encoding"][1])
            self._encoding = parse_encoding(self.ecat(ekey))
        return self._encoding

    def ekey_of(self, ckey):
        return self.encoding().get(ckey)

    def cat(self, ckey):
        ekey = self.ekey_of(ckey)
        return self.ecat(ekey) if ekey else None

    def root(self):
        return self.cat(bytes.fromhex(self.config["root"][0]))


# TACT keys by key name: local inputs, kept out of the source (tools/lobby/vendor/data/README.md).
KEYS_FILE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "lobby", "vendor", "data", "tact_keys_174.json")


def load_keys(path=KEYS_FILE):
    if not os.path.isfile(path):
        return {}
    with open(path, encoding="utf-8") as handle:
        return {int(name, 16): bytes.fromhex(key) for name, key in json.load(handle).items()}


KEYS = load_keys()


def _rotl(v, n):
    return ((v << n) | (v >> (32 - n))) & 0xFFFFFFFF


def salsa20(key, nonce, data):
    """Salsa20/20 with a 16-byte key ("expand 16-byte k") and an 8-byte nonce."""
    c = struct.unpack("<4I", b"expand 16-byte k")
    k = struct.unpack("<4I", key)
    n = struct.unpack("<2I", nonce)
    out = bytearray(len(data))
    for block in range((len(data) + 63) // 64):
        st = [c[0], k[0], k[1], k[2], k[3], c[1], n[0], n[1],
              block & 0xFFFFFFFF, block >> 32, c[2], k[0], k[1], k[2], k[3], c[3]]
        x = st[:]
        for _ in range(10):
            for a, b, cc, d in ((0, 4, 8, 12), (5, 9, 13, 1), (10, 14, 2, 6), (15, 3, 7, 11)):
                x[b] ^= _rotl((x[a] + x[d]) & 0xFFFFFFFF, 7)
                x[cc] ^= _rotl((x[b] + x[a]) & 0xFFFFFFFF, 9)
                x[d] ^= _rotl((x[cc] + x[b]) & 0xFFFFFFFF, 13)
                x[a] ^= _rotl((x[d] + x[cc]) & 0xFFFFFFFF, 18)
            for a, b, cc, d in ((0, 1, 2, 3), (5, 6, 7, 4), (10, 11, 8, 9), (15, 12, 13, 14)):
                x[b] ^= _rotl((x[a] + x[d]) & 0xFFFFFFFF, 7)
                x[cc] ^= _rotl((x[b] + x[a]) & 0xFFFFFFFF, 9)
                x[d] ^= _rotl((x[cc] + x[b]) & 0xFFFFFFFF, 13)
                x[a] ^= _rotl((x[d] + x[cc]) & 0xFFFFFFFF, 18)
        ks = struct.pack("<16I", *[(x[i] + st[i]) & 0xFFFFFFFF for i in range(16)])
        seg = data[block * 64:block * 64 + 64]
        for i, bch in enumerate(seg):
            out[block * 64 + i] = bch ^ ks[i]
    return bytes(out)


def blte(blob):
    if blob[:4] != b"BLTE":
        raise ValueError("not BLTE")
    header_size = struct.unpack_from(">I", blob, 4)[0]
    if header_size == 0:
        return chunk(blob[8:], 0)
    count = int.from_bytes(blob[9:12], "big")
    pos, data, out = 12, header_size, []
    for i in range(count):
        comp, _decomp = struct.unpack_from(">II", blob, pos)
        pos += 24
        out.append(chunk(blob[data:data + comp], i))
        data += comp
    return b"".join(out)


def chunk(c, index):
    mode = c[:1]
    if mode == b"N":
        return c[1:]
    if mode == b"Z":
        return zlib.decompress(c[1:])
    if mode == b"E":
        # key name size, key name, iv size, iv, type ('S' Salsa20), encrypted chunk
        kn = c[1]
        name = int.from_bytes(c[2:2 + kn], "little")
        ivn = c[2 + kn]
        iv = bytearray(c[3 + kn:3 + kn + ivn]) + bytearray(8 - ivn)
        kind = c[3 + kn + ivn:4 + kn + ivn]
        key = KEYS.get(name)
        if key is None or kind != b"S":
            raise ValueError("encrypted chunk: key %016x type %r" % (name, kind))
        for j in range(4):
            iv[j] ^= (index >> (8 * j)) & 0xFF
        return chunk(salsa20(key, bytes(iv), c[4 + kn + ivn:]), index)
    raise ValueError("chunk mode %r" % mode)


def parse_encoding(data):
    if data[:2] != b"EN":
        raise ValueError("not an encoding file")
    _, ckey_size, ekey_size, cpage_kb, _epage_kb, ccount, _ecount, _, espec_size = struct.unpack_from(">BBBHHIIBI", data, 2)
    pos = 22 + espec_size + ccount * 32
    table = {}
    page = cpage_kb * 1024
    for p in range(ccount):
        base = pos + p * page
        i = base
        while i < base + page:
            keys = data[i]
            if keys == 0:
                break
            ckey = data[i + 6:i + 6 + ckey_size]
            ekey = data[i + 6 + ckey_size:i + 6 + ckey_size + ekey_size]
            table[ckey] = ekey
            i += 6 + ckey_size + ekey_size * keys
    return table


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    s = Storage()
    cmd = sys.argv[1] if len(sys.argv) > 1 else "root"
    if cmd == "root":
        data = s.root()
        print(data.decode("utf-8", "replace"))
    elif cmd in ("cat", "ecat"):
        key = bytes.fromhex(sys.argv[2])
        data = s.cat(key) if cmd == "cat" else s.ecat(key)
        if data is None:
            print("not found")
            return 1
        if len(sys.argv) > 3:
            open(sys.argv[3], "wb").write(data)
            print("wrote", len(data), "bytes, md5", hashlib.md5(data).hexdigest())
        else:
            sys.stdout.buffer.write(data[:4096])
    return 0


if __name__ == "__main__":
    sys.exit(main())
