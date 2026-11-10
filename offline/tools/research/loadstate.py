"""The live client's map-loading systems and their pending game. Read-only.

A 20300 (join instance) makes the client copy a 336-byte pending game {+0 map GUID, +8 mode GUID, +0x10 third GUID,
+0x18 32-byte hotfix blob, u64 arrays at +0x38, +0x50, +0x68, +0xF8} and attach it to two loading systems (vtable
`0x7FF6D387CD88`; +0x18 state byte, +0x20 the pending game, +0x41 network-attached, +0x42 ready to leave the loading
screen). States: 2 waits for the map header asset, 7 sends 21601 (map loaded), 9 waits for the loading screen's
predicate (a scripted-loading mode needs an active statescript state of class 9FC136D7), 10 is in the world.
The second system stays at 7. See research/play-menu.md.

  python loadstate.py PID MAPGUID MODEGUID

Lists every pending-game copy for that map and mode with the arrays the state-9 compare reads, and each loading
system holding one with its state and flags.
"""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from debug_assert import PROCESS_VM_READ, _kernel32, _read_remote, find_target_module, validate_pid
from volley_probe import PROCESS_QUERY_INFORMATION, scan_for_qword


def main(argv):
    pid = validate_pid(int(argv[1]))
    map_guid, mode_guid = int(argv[2], 16), int(argv[3], 16)
    k32 = _kernel32()
    find_target_module(k32, pid)
    process = k32.OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, False, pid)
    rd = lambda a, n: _read_remote(k32, process, a, n)
    q = lambda a: struct.unpack('<Q', rd(a, 8) or bytes(8))[0]
    d = lambda a: struct.unpack('<i', rd(a, 4) or bytes(4))[0]

    def array(addr, ptr_off, count_off):
        ptr, count = q(addr + ptr_off), d(addr + count_off)
        if not ptr or not 0 <= count <= 64:
            return f'ptr {ptr:#x} count {count}'
        raw = rd(ptr, 8 * count) or b''
        return f'[{count}] ' + ' '.join(f'{v:016X}' for v in struct.unpack(f'<{len(raw) // 8}Q', raw))

    games = [a for a in scan_for_qword(k32, process, map_guid) if q(a + 8) == mode_guid]
    print(f'pending-game copies for {map_guid:#x} in {mode_guid:#x}: {len(games)}')
    for game in games:
        systems = [o - 32 for o in scan_for_qword(k32, process, game)]  # system+0x20 holds the game
        if not systems:
            continue
        print(f'game {game:#x} held by ' + ' '.join(
            f'{s:#x} (state {rd(s + 0x18, 1)[0]}, attached {rd(s + 0x41, 1)[0]}, ready {rd(s + 0x42, 1)[0]})'
            for s in systems))
        print(f'  +0x10 {q(game + 0x10):016X}  blob {(rd(game + 0x18, 32) or b"").hex()}')
        for off in (0x38, 0x50, 0x68, 0xF8):
            print(f'  +{off:#x} {array(game, off, off + 8)}')


if __name__ == '__main__':
    main(sys.argv)
