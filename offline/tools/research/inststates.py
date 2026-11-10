"""active states of given statescript instances (address hex). Read-only.

Args: pid instance-address ...
"""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from debug_assert import PROCESS_VM_READ, _kernel32, _read_remote, validate_pid

pid = int(sys.argv[1])
validate_pid(pid)
k32 = _kernel32()
process = k32.OpenProcess(PROCESS_VM_READ, False, pid)
q = lambda a: struct.unpack('<Q', _read_remote(k32, process, a, 8))[0]
d = lambda a: struct.unpack('<I', _read_remote(k32, process, a, 4))[0]
for arg in sys.argv[2:]:
    inst = int(arg, 16)
    array, count = q(inst + 0xA0), d(inst + 0xA8)
    raw = b''.join(_read_remote(k32, process, array + o, min(512, count * 8 - o)) for o in range(0, count * 8, 512))
    states = struct.unpack('<%dQ' % count, raw)
    flags = {i: d(s + 0x40) for i, s in enumerate(states) if s}
    print('%X graph %X states %d active %s' % (inst, q(inst + 0x1E8), count, [i for i, f in flags.items() if f & 1]))
