"""find qwords equal to graph GUIDs and print the qword at hit-0x1E8 (instance vtable?) as an RVA.

Args: pid guid-hex ...
"""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from debug_assert import PROCESS_VM_READ, _kernel32, _read_remote, find_target_module, validate_pid
from volley_probe import PROCESS_QUERY_INFORMATION, scan_for_qword

pid = int(sys.argv[1])
validate_pid(pid)
k32 = _kernel32()
base, size = find_target_module(k32, pid)
process = k32.OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, False, pid)
q = lambda a: struct.unpack('<Q', _read_remote(k32, process, a, 8))[0]
for arg in sys.argv[2:]:
    guid = int(arg, 16)
    hits = list(scan_for_qword(k32, process, guid))
    print('%X: %d hits' % (guid, len(hits)))
    for hit in hits[:12]:
        try:
            v = q(hit - 0x1E8)
            print('  %X  [-1E8]=%s' % (hit, ('img+%X' % (v - base)) if base <= v < base + size else '%X' % v))
        except OSError:
            print('  %X  unreadable' % hit)
