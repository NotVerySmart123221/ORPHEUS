"""Extract heroes' Hero Info ability clips (STULoadout m_infoMovie) and decode them to MP4.

  python hero_movies.py OUTDIR HERO... [--game-install ROOT]

HERO is a name from lobby/vendor/data/game_heroes_174.json ("Soldier: 76", case-insensitive) or a hero GUID.
Writes OUTDIR/<hero>/<n>_<loadout GUID>_<movie GUID>.mp4, n = the hero's loadout order (STUHero m_loadouts),
and OUTDIR/<hero>/index.json. Movies are Bink 2 (KB2j), which ffmpeg cannot decode, so frames come from the
game's own _retail_/bink2w64.dll (read-only use of an install file) and are piped to ffmpeg as raw BGRx.
"""
import argparse
import ctypes
import json
import os
import subprocess
import struct
from pathlib import Path

from practice_range_assets import load_storage
from retail_bundle_lookup import _decode_asset_ref, find_asset_payload, stu_header_crc64
from stu_graph import instances
from stu_refs import decode_refs

HEROES = Path(__file__).resolve().parents[1] / "lobby" / "vendor" / "data" / "game_heroes_174.json"
LOADOUTS = 0x77FED604  # STUHero field holding the hero's loadout refs (0x0B9 GUIDs), in Hero Info order
INFO_MOVIE = 0xC8D38D7B  # STULoadout m_infoMovie (0x0AD GUID)
MOVI_HEADER = 128  # OWLib ExtractDebugMovies: the Bink file starts after this
BINKSURFACE32 = 3  # BGRx
BINKCOPYALL = 0x80000000


def hero_guid(text):
    if text.lower().startswith("0x") or len(text) == 16:
        return int(text, 16)
    table = json.loads(HEROES.read_text())
    for guid, row in table.items():
        if row["name"].lower() == text.lower():
            return int(guid, 16)
    raise SystemExit(f"unknown hero {text!r}; names: {', '.join(r['name'] for r in table.values())}")


def movies(storage, hero):
    """[(loadout GUID, movie GUID)] in the hero's loadout order."""
    blob = find_asset_payload(storage, hero).payload
    refs = [guid for _, field, guid in decode_refs(blob, {0xB90}) if field == LOADOUTS]
    out = []
    for loadout in refs:
        payload = find_asset_payload(storage, loadout).payload
        raw = instances(payload)[0][3].get(INFO_MOVIE)
        if raw:
            out.append((loadout, _decode_asset_ref(int.from_bytes(raw[:8], "little"), INFO_MOVIE,
                                                   stu_header_crc64(payload))))
    return out


def bink_dll(game_install):
    dll = ctypes.CDLL(os.path.join(game_install, "_retail_", "bink2w64.dll"))
    dll.BinkOpen.restype, dll.BinkOpen.argtypes = ctypes.c_void_p, [ctypes.c_char_p, ctypes.c_uint32]
    dll.BinkGetError.restype = ctypes.c_char_p
    for name in ("BinkDoFrame", "BinkNextFrame", "BinkClose"):
        getattr(dll, name).argtypes = [ctypes.c_void_p]
    dll.BinkCopyToBuffer.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int] + [ctypes.c_uint32] * 4
    return dll


def decode(dll, bk2, mp4):
    """Every frame of a .bk2 into an H.264 MP4; returns (width, height, frames, fps)."""
    handle = dll.BinkOpen(str(bk2).encode(), 0)
    if not handle:
        raise RuntimeError(f"BinkOpen {bk2}: {dll.BinkGetError().decode(errors='replace')}")
    try:
        width, height, frames, _, _, rate, rate_div = (ctypes.c_uint32 * 7).from_address(handle)
        buf = ctypes.create_string_buffer(width * height * 4)
        ffmpeg = subprocess.Popen(
            ["ffmpeg", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "bgr0", "-s", f"{width}x{height}",
             "-r", f"{rate}/{rate_div}", "-i", "-", "-c:v", "libx264", "-crf", "16", "-pix_fmt", "yuv420p", str(mp4)],
            stdin=subprocess.PIPE)
        for i in range(frames):
            dll.BinkDoFrame(handle)
            dll.BinkCopyToBuffer(handle, buf, width * 4, height, 0, 0, BINKSURFACE32 | BINKCOPYALL)
            if i == 0:
                assert any(buf.raw[::4096]), f"{bk2}: first frame decoded all black"
            ffmpeg.stdin.write(buf.raw)
            dll.BinkNextFrame(handle)
        ffmpeg.stdin.close()
        if ffmpeg.wait():
            raise RuntimeError(f"ffmpeg failed on {mp4}")
        return width, height, frames, rate / rate_div
    finally:
        dll.BinkClose(handle)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("out", type=Path)
    parser.add_argument("heroes", nargs="+")
    parser.add_argument("--game-install", default=os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
    args = parser.parse_args()
    names = {int(g, 16): row["name"] for g, row in json.loads(HEROES.read_text()).items()}
    storage, dll = load_storage(args.game_install), bink_dll(args.game_install)
    for hero in map(hero_guid, args.heroes):
        folder = args.out / "".join(c for c in names.get(hero, f"{hero:016X}") if c.isalnum())
        folder.mkdir(parents=True, exist_ok=True)
        index = []
        for n, (loadout, movie) in enumerate(movies(storage, hero)):
            blob = find_asset_payload(storage, movie).payload
            assert struct.unpack_from("<I", blob)[0] == 0x4D4F5649 and blob[MOVI_HEADER:MOVI_HEADER + 3] == b"KB2", \
                f"{movie:016X} is not a MOVI-wrapped Bink 2 file"
            bk2 = folder / f"{movie:016X}.bk2"
            mp4 = folder / f"{n}_{loadout:016X}_{movie:016X}.mp4"
            bk2.write_bytes(blob[MOVI_HEADER:])
            try:
                width, height, frames, fps = decode(dll, bk2, mp4)
            finally:
                bk2.unlink()
            index.append({"order": n, "loadout": f"{loadout:016X}", "movie": f"{movie:016X}", "file": mp4.name,
                          "width": width, "height": height, "frames": frames, "fps": round(fps, 3)})
            print(f"{mp4} {width}x{height} {frames} frames @ {fps:.2f} fps")
        (folder / "index.json").write_text(json.dumps(index, indent=1) + "\n")


if __name__ == "__main__":
    main()
