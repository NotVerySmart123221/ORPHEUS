"""Read-only: the live client's UX binding objects (vtable img+0x27B5B90, 0xC0 bytes each) and the identifier each
property name resolved to. Names come from the widget templates (0x0BA); the 0x0D8 ids are only known at run time.

  python ux_bindings.py PID [NAME ...]   (no names: every binding)

Prints: object address, +0x18 name key, +0x28 data context, +0x80 resolved id, name (research/hero-menu.md)."""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from debug_assert import PROCESS_VM_READ, _kernel32, _read_remote, find_target_module, validate_pid
from volley_probe import PROCESS_QUERY_INFORMATION, scan_for_qword

DUMP_BASE = 0x7FF6D1220000
VTABLE = 0x7FF6D39D5B90 - DUMP_BASE

pid = validate_pid(int(sys.argv[1]))
wanted = set(sys.argv[2:])
k32 = _kernel32()
base, _ = find_target_module(k32, pid)
process = k32.OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, False, pid)
rd = lambda a, n: _read_remote(k32, process, a, n)
q = lambda a: struct.unpack('<Q', rd(a, 8) or bytes(8))[0]


def name_at(tagged):
    """Interned string object: length u32 at +0x50, capacity at +0x54; text inline at +0x40 when it fits, else a
    pointer at +0x40."""
    p = tagged & 0x0000FFFFFFFFFFF0
    head = rd(p + 0x40, 0x18)
    if len(head) < 0x18:
        return '', 0
    n, cap = struct.unpack_from('<II', head, 0x10)
    if not 0 < n < 256:
        return '', 0
    if n < cap:
        return head[:n].decode('latin1'), p + 0x40
    ptr = struct.unpack_from('<Q', head)[0]
    return (rd(ptr, n) or b'').decode('latin1'), ptr


hits = scan_for_qword(k32, process, base + VTABLE)
print(f'base {base:#x}; {len(hits)} binding objects')
shown = 0
for obj in hits:
    raw18, ctx, ident = q(obj + 0x18), q(obj + 0x28), q(obj + 0x80)
    name, where = name_at(raw18)
    if wanted and name not in wanted:
        continue
    print(f'{obj:#x} +0x18={raw18:#x} ctx={ctx:#x} id={ident:#018x} name={name!r} (at {where:#x})')
    shown += 1
    if False:
        break
