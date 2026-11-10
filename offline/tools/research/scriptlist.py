"""list live statescript instances (vtable img+262E740): graph GUID (+0x1E8), states (+0xA0/+0xA8),
active state indexes (state +0x40 bit0). Read-only. It can miss an instance that guidhits.py finds.

Args: pid [graph-index-hex ...]  (only those graphs when given)
"""
import struct
import sys

from debug_assert import PROCESS_VM_READ, _kernel32, _read_remote, find_target_module, validate_pid
from volley_probe import PROCESS_QUERY_INFORMATION, scan_for_qword

pid = int(sys.argv[1])
wanted = {int(a, 16) for a in sys.argv[2:]}
validate_pid(pid)
k32 = _kernel32()
base, _ = find_target_module(k32, pid)
process = k32.OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, False, pid)
q = lambda a: struct.unpack('<Q', _read_remote(k32, process, a, 8))[0]
d = lambda a: struct.unpack('<I', _read_remote(k32, process, a, 4))[0]
for inst in scan_for_qword(k32, process, base + 0x262E740):
    try:
        graph = q(inst + 0x1E8)
        if wanted and graph & 0xFFFFFFFF not in wanted:
            continue
        array, count = q(inst + 0xA0), d(inst + 0xA8)
        if not 0 < count < 1024:
            continue
        states = struct.unpack('<%dQ' % count, _read_remote(k32, process, array, 8 * count))
        active = [i for i, s in enumerate(states) if s and d(s + 0x40) & 1]
        print('%X graph %X states %d active %s' % (inst, graph, count, active))
    except (OSError, ValueError, struct.error):
        continue
