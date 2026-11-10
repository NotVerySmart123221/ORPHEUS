"""Read the client's own body position from the running game (read-only) and report the server's drift.

The client never sends its position; the server moves a copy from the commands (mover.py), and its hitscan test
shoots from that copy. The client's own mover object holds its simulated feet together with the command frame they
belong to (research/hitbox-drift.md, "The client's mover"):

    +0x00  vtable img+0x260E7C8; a second vtable img+0x260E880 at +0x20
    +0x70  f32 x, y, z  the feet after that frame's command, on the 1/1024 m grid
    +0xA0  u32          the command frame: the server's Command.frame

The client keeps these in an array, 0x1610 bytes apart; the free ones hold no position. A scan for the vtable finds
them at any time (about 8 s on a loaded map), not only right after a spawn. A new body (respawn, hero switch) takes
another slot, sometimes just outside the ones found, so the neighbours are checked too before scanning again.

    python client_position.py [--port 47123] [--pid PID] [--seconds N]   drift against the hitbox feed (OW_HITBOX_VIEW=PORT)

The lobby's resync (OW_CLIENT_SYNC) is fed from inside the client by ow_fixes.asi (tools/asi/client_feed.c), the
same reader in C.

It prints `drift dx dy dz |horizontal|` (client minus server, metres) twice a second, both taken at the
same command frame, so it is exact while moving too.
"""
import argparse
import json
import math
import struct
import subprocess
import sys
import time

from debug_assert import PROCESS_VM_READ, _kernel32, _read_remote, find_target_module, validate_pid
from hitbox_view import PORT, listen
from volley_probe import PROCESS_QUERY_INFORMATION, scan_for_bytes

EYE = 1.6  # hitbox_feed.EYE
VTABLE_RVA, SECOND_VTABLE_RVA = 0x260E7C8, 0x260E880
POSITION, FRAME = 0x70, 0xA0
STRIDE, NEAR = 0x1610, 16  # bytes between slots; neighbours checked on each side of every known slot
STALE = 2.0  # seconds without a new frame before the pool is checked again (respawn, hero switch, new map)
RESCAN = 10.0  # seconds between full scans while no pool slot holds a body (menus, a new map's pool)
POLL = 0.0005  # seconds between reads: a command frame is 7 ms, and the client simulates some in pairs


def game_pid():
    out = subprocess.run(['powershell', '-NoProfile', '-Command', '(Get-Process Overwatch).Id'],
                         capture_output=True, text=True).stdout.split()
    if not out:
        sys.exit('game not running')
    return validate_pid(int(out[0]))


def feet(state):
    x, y, z = state['eye']
    return x, y - EYE, z


class ClientMover:
    """The client's own mover: (command frame, feet) at any time."""

    def __init__(self, pid):
        self.k32 = _kernel32()
        self.base, _ = find_target_module(self.k32, pid)  # refuses anything but the hash-checked retail client
        self.process = self.k32.OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, False, pid)
        if not self.process:
            raise OSError('OpenProcess failed')
        self.pool, self.slot, self.next_scan = [], None, 0.0
        self.last_frame, self.last_change = None, time.monotonic()

    def _read(self, address, size):
        return _read_remote(self.k32, self.process, address, size)

    def sample(self, slot):
        """(frame, feet) of a slot holding a body, else None. Two equal reads: the position and its frame are written
        separately, and one read could catch them from different frames; a few tries get past the client's write."""
        for _ in range(4):
            raw = self._read(slot, FRAME + 4)
            if len(raw) != FRAME + 4:
                return None
            if raw == self._read(slot, FRAME + 4):
                break
        else:
            return None
        if struct.unpack_from('<Q', raw, 0)[0] != self.base + VTABLE_RVA or \
                struct.unpack_from('<Q', raw, 0x20)[0] != self.base + SECOND_VTABLE_RVA:
            return None
        position = struct.unpack_from('<3f', raw, POSITION)
        if not all(math.isfinite(v) for v in position) or position == (0.0, 0.0, 0.0):
            return None  # a free slot
        return struct.unpack_from('<I', raw, FRAME)[0], position

    def find(self):
        """The slot holding the body: the known slots and their neighbours first, then (at most every RESCAN seconds)
        a scan of the whole game for the vtable. shortcut: with several bodies (never seen in the Range) the newest
        frame wins; upgrade if a mode gives remote bodies a client mover."""
        for scan in (False, True):
            if scan:
                if time.monotonic() < self.next_scan:
                    break
                self.next_scan = time.monotonic() + RESCAN
                self.pool = scan_for_bytes(self.k32, self.process, struct.pack('<Q', self.base + VTABLE_RVA), 8, 64 << 20)
            near = {slot + k * STRIDE for slot in self.pool for k in range(-NEAR, NEAR + 1)}
            samples = [(s[0], slot) for slot in near if (s := self.sample(slot))]
            if samples:
                self.slot = max(samples)[1]
                if self.slot not in self.pool:
                    self.pool.append(self.slot)
                self.last_change = time.monotonic()
                return self.slot
        self.slot = None
        return None

    def read(self):
        """(frame, feet) now, or None (no body). Looks for the body again when its slot is freed or its frame has not
        moved for STALE seconds."""
        sample = self.sample(self.slot) if self.slot is not None else None
        if sample is None or time.monotonic() - self.last_change > STALE:
            if self.find() is None:
                return None
            sample = self.sample(self.slot)
        if sample is not None and sample[0] != self.last_frame:
            self.last_frame, self.last_change = sample[0], time.monotonic()
            self.next_scan = self.last_change + RESCAN  # a lost body is looked for near the old slots first
        return sample


def wait_for_body(client, end):
    while time.monotonic() < end and client.read() is None:
        time.sleep(1)


def drift(client, port, seconds):
    sock = listen(port)
    sock.setblocking(False)
    seen, server, end, report = {}, None, time.monotonic() + seconds, 0.0
    while time.monotonic() < end:
        sample = client.read()
        if sample is not None:
            seen[sample[0]] = sample[1]
        try:
            while True:
                state = json.loads(sock.recv(65536))
                server = state['frame'], feet(state)
        except BlockingIOError:
            pass
        if time.monotonic() >= report and server is not None and server[0] in seen:
            d = [c - s for c, s in zip(seen[server[0]], server[1])]
            print(f'frame {server[0]} drift {d[0]:+.3f} {d[1]:+.3f} {d[2]:+.3f} |{math.hypot(d[0], d[2]):.3f}|', flush=True)
            report = time.monotonic() + 0.5
        if len(seen) > 4096:
            seen = {f: p for f, p in seen.items() if f > max(seen) - 1024}
        time.sleep(POLL)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--port', type=int, default=PORT, help='hitbox feed port')
    parser.add_argument('--pid', type=int, help='game process (default: the running Overwatch)')
    parser.add_argument('--seconds', type=float, default=600)
    args = parser.parse_args(argv)
    client = ClientMover(validate_pid(args.pid) if args.pid else game_pid())
    wait_for_body(client, time.monotonic() + args.seconds)
    print(f'client mover at {client.slot:#x}', flush=True)
    drift(client, args.port, args.seconds)


if __name__ == '__main__':
    main()
