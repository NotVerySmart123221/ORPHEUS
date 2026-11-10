"""Walk the live client's loaded-asset table and report asset load states. Read-only.

The table (buckets `qword_7FF6D443F360`, count `dword_7FF6D443F358`, both image-relative) holds one record per
asset the client has requested: {+0 next, +8 self, +0x10 hash, +0x28 data, +0x80 GUID, +0xAC status}. Status 3 is
loaded; below 2 is still loading. The map loader waits at its state 2 for the map header (`0x0790...`) to be 3,
and only the login message 20505's header list makes the client request it (research/play-menu.md).

  python assettab.py PID [GUID ...]

Prints the record count and status histogram, the given GUIDs' states, then every map header (0x0790), game mode
(0x0230 / 0x0A30) and map (0x0800) record. Addresses use the IDA dump's base 0x7FF6D1220000.
"""
import os
import collections
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from debug_assert import PROCESS_VM_READ, _kernel32, _read_remote, find_target_module, validate_pid
from volley_probe import PROCESS_QUERY_INFORMATION

DUMP_BASE = 0x7FF6D1220000
BUCKETS, COUNT = 0x7FF6D443F360 - DUMP_BASE, 0x7FF6D443F358 - DUMP_BASE


def records(read, base):
    """GUID -> (record address, status byte) for every record in the table."""
    q = lambda a: struct.unpack('<Q', read(a, 8) or bytes(8))[0]
    buckets = q(base + BUCKETS)
    count = struct.unpack('<I', read(base + COUNT, 4) or bytes(4))[0]
    out = {}
    for i in range(count):
        node, steps = q(buckets + 8 * i), 0
        while node and steps < 500:
            out[q(node + 0x80)] = (node, (read(node, 0xB0) or bytes(0xB0))[0xAC])
            node, steps = q(node), steps + 1
    return out


def main(argv):
    pid = validate_pid(int(argv[1]))
    k32 = _kernel32()
    base, _ = find_target_module(k32, pid)
    process = k32.OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, False, pid)
    recs = records(lambda a, n: _read_remote(k32, process, a, n), base)
    print(f'{len(recs)} records; status histogram {dict(collections.Counter(s for _, s in recs.values()))}')
    for arg in argv[2:]:
        g = int(arg, 16)
        print(f'{g:#018x}: ' + (f'record {recs[g][0]:#x} status {recs[g][1]}' if g in recs else 'absent'))
    for label, types in (('map headers', (0x0790,)), ('modes', (0x0230, 0x0A30)), ('maps', (0x0800,))):
        print(f'{label}:', ' '.join(f'{g:#x}:{s}' for g, (_, s) in sorted(recs.items()) if g >> 48 in types))


if __name__ == '__main__':
    main(sys.argv)
