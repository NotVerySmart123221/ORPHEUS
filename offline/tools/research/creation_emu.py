"""Emulate the client's projectile creation 0x7FF6D1D41790 for a live shot config and report its status code.

  python creation_emu.py PID [--state 16] [--state 10] [--watch SECONDS]

Finds the Soldier Volleys (volley_probe.inspect), takes each requested state's shot config from the entity's
info map (key byte 1 = state index), builds the request the shot builder 0x7FF6D1D043F0 passes (entity +0x08,
config +0x20, projectile asset +0x30 from [config+0xC0]+0x20) and runs creation in unicorn over live memory
(live_emu.py), once with the caller's flag (r9b) 0 and once with 1. Emulation stops at the spawn call
0x7FF6D1D4227D, so no entity is created even in the emulator; reaching it means every refusal passed.
Status codes: research/soldier-runtime/helix-visual-findings.md (projectile creation trace). Read-only.
"""
import argparse
import struct
import time

from live_emu import LiveEmulator
from debug_assert import _read_remote
import volley_probe

CREATE = 0x7FF6D1D41790
RETURN_SITE = 0x7FF6D1D04609  # the shot builder's call returns here
SPAWN_CALL = 0x7FF6D1D4227D   # sub_7FF6D2A50A50: the spawn itself
REQUEST, REQUEST_SIZE = 0x20000000, 0x1000
STATUS = REQUEST + 0xF8       # the builder passes request+0xF8
SRET = REQUEST + 0x800
STATUSES = {
    1: 'refused: component 47 or config byte +0x107, with the caller flag',
    2: 'refused: no asset', 3: 'refused: definition load failed', 4: 'refused: no ProjectileVisual (50)',
    5: 'refused: config +0xB0 set and no component 49', 6: 'refused: config +0x11C == 1 without an entity',
    9: 'refused: component 22', 10: 'refused: speed > 0 and an Effect component (9)',
    7: 'spawn returned no entity',
    8: 'type-1 config (+0x11C == 1): no spawner, or the spawner made no entity (no reserved predicted ID)',
    11: 'initial value (no refusal written)',
}


def request_bytes(entity, config, asset, tick=0):
    raw = bytearray(REQUEST_SIZE)
    struct.pack_into('<I', raw, 0x00, tick)  # the executor stores sub_7FF6D2D91860(volley), the game tick
    struct.pack_into('<Q', raw, 0x08, entity)
    struct.pack_into('<Q', raw, 0x20, config)
    struct.pack_into('<Q', raw, 0x30, asset)
    return bytes(raw)


def snapshot(reader, config, projectile):
    """The pages holding the projectile info and the config, read now: a config lives about 30 ms in the info
    map and shares its slot with other states' configs. None if the copy is not complete."""
    pages = {}
    for address in (projectile, projectile + 0x3F, config, config + 0x1FF):
        page = address & ~0xFFF
        if page not in pages:
            pages[page] = b''.join(_read_remote(reader.k32, reader.process, page + o, 512) for o in range(0, 0x1000, 512))
    read = lambda a: struct.unpack_from('<Q', pages[a & ~0xFFF], a & 0xFFF)[0]
    if read(config + 0xC0) != projectile or not read(projectile + 0x20):
        return None
    return pages


def prepare(pid, pages):
    """An emulator whose copies of these pages are the snapshot."""
    emulator = LiveEmulator(pid)
    for page, raw in pages.items():
        emulator.uc.mem_map(page, 0x1000)
        emulator.uc.mem_write(page, raw)
    return emulator


def emulate(emulator, entity, config, asset, flag, tick=0):
    emulator.uc.mem_map(REQUEST, REQUEST_SIZE)
    emulator.uc.mem_write(REQUEST, request_bytes(entity, config, asset, tick))
    emulator.stop_at(SPAWN_CALL)
    result = emulator.call(CREATE, RETURN_SITE, SRET, REQUEST, STATUS, flag)
    status = struct.unpack('<I', emulator.uc.mem_read(STATUS, 4))[0]
    return {'flag': flag, 'status': status, 'meaning': STATUSES.get(status, '?'),
            'reached_spawn': emulator.stopped_at == SPAWN_CALL, 'returned': result is not None,
            'fault': None if emulator.fault is None else '%X %s' % emulator.fault}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('pid', type=int)
    parser.add_argument('--state', type=int, action='append', help='Volley state index (default 16 and 10)')
    parser.add_argument('--watch', type=float, default=0.0, help='wait this long for a live key (fire meanwhile)')
    args = parser.parse_args(argv)
    states = args.state or [16, 10]
    probe = volley_probe.inspect(args.pid)
    volley = next(v for v in probe['volleys'] if 'entity' in v)
    entity = int(volley['entity'], 16)
    reader = LiveEmulator(args.pid)  # reads, and the tick
    tick = reader.call(0x7FF6D2D91860, 0x7FF6D1D071AA, int(volley['address'], 16)) & 0xFFFFFFFF
    qword = lambda a: struct.unpack('<Q', _read_remote(reader.k32, reader.process, a, 8))[0]
    dword = lambda a: struct.unpack('<I', _read_remote(reader.k32, reader.process, a, 4))[0]
    base = reader.live(0x7FF6D1220000)  # the client's live image base
    # the info map only holds a config while its key is live: poll it, and copy the config the moment it shows
    done, deadline = set(), time.monotonic() + args.watch
    while True:
        for key, info in volley_probe.info_map(qword, dword, base, entity).items():
            state = key >> 8 & 0xFF
            if state not in states or state in done or not info:
                continue
            projectile = qword(info + 0xC0)
            if not projectile or not qword(projectile + 0x20):
                continue  # not filled yet (or already released): keep polling
            pages = snapshot(reader, info, projectile)
            if pages is None:
                continue
            done.add(state)
            emulators = [prepare(args.pid, pages) for _ in (0, 1)]
            asset = struct.unpack('<Q', emulators[0].uc.mem_read(projectile + 0x20, 8))[0]
            print('state %d key %X config %X entity %X asset %X config+4C %r config+B0 %X config+11C %d' % (
                state, key, info, entity, asset, *struct.unpack('<fQI', bytes(emulators[0].uc.mem_read(info + 0x4C, 4)) +
                bytes(emulators[0].uc.mem_read(info + 0xB0, 8)) + bytes(emulators[0].uc.mem_read(info + 0x11C, 4)))), flush=True)
            for flag, emulator in zip((0, 1), emulators):
                print('  ', emulate(emulator, entity, info, asset, flag, tick), flush=True)
        if set(states) <= done or time.monotonic() > deadline:
            break
        time.sleep(0.002)
    missing = set(states) - done
    if missing:
        print('no live shot config for states', sorted(missing), '(fire once so the key exists)')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
