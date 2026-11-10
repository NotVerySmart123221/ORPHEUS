"""Every config-var class of the client, from the runtime snapshot: hash, record, vtable, evaluator, and what the
evaluator returns when it is a plain global byte (a cvar: default value from .data). Read-only, static.

    python config_var_scan.py [-n MAX]        (env OW_SNAPSHOT: the snapshot folder, as config_var_eval.py)

Config vars are STU classes registered through the same call as every other class (`sub_7FF6D2E09EA0`: record,
"Unknown", hash, ..., size, align, ...): the registration loads the record (lea rax, record) right before
`mov r8d, hash`. This lists the registrations whose evaluator (runtime vtable slot 4) starts by loading a byte
from a global, the shape of the PLAY-card gates retail_menu.asi forces (research/play-menu.md), so leftover
switches can be found and forced the same way. Names are stripped from the build ("Unknown")."""
import re
import struct
import sys

import config_var_eval as cv

BASE, TEXT = cv.BASE, cv.BASE + cv.sec['.text']['rva']
RECORD_MIN = BASE + cv.sec['.rdata']['rva']
MODRM = (0x5B, 0x05, 0x0D, 0x15, 0x1D, 0x25, 0x2D, 0x35, 0x3D, 0x5D)  # rip-relative lea/mov forms, as config_var_eval.xrefs


def index_xrefs():
    """rip-relative target -> next-instruction addresses, one pass (config_var_eval.xrefs rescans .text per call)."""
    text, index = cv.blobs['.text'], {}
    for m in re.finditer(b'[' + b''.join(re.escape(bytes([c])) for c in MODRM) + b']', text):
        p = m.start()
        if p + 5 <= len(text):
            index.setdefault(TEXT + p + 5 + struct.unpack_from('<i', text, p + 1)[0], []).append(TEXT + p + 5)
    cv.xrefs = lambda target: index.get(target, [])


def registrations():
    """(hash, record) for each `lea rax, [rip+record]; mov r8d, imm32` pair in .text."""
    text = cv.blobs['.text']
    for m in re.finditer(rb'\x48\x8d\x05(....)\x41\xb8(....)', text, re.DOTALL):
        va = TEXT + m.start()
        record = va + 7 + struct.unpack('<i', m.group(1))[0]
        if record >= RECORD_MIN:
            yield struct.unpack('<I', m.group(2))[0], record


def is_evaluator(fn):
    """Config-var evaluators start by asking the context (rdx) for its vtable slot 0xB0 (the six known ones do)."""
    return any(i.mnemonic == 'call' and i.op_str == 'qword ptr [rax + 0xb0]' for i in cv.code(fn, 10))


def global_byte(evaluator):
    """The first global byte the evaluator touches ([rip+X], byte-sized operand) within its first 40 instructions."""
    for i in cv.code(evaluator, 40):
        for k in range(len(i.operands)):
            t = cv.rip(i, k)
            if t and i.operands[k].size == 1:
                return t
        if i.mnemonic == 'ret':
            break
    return None


def main():
    limit = int(sys.argv[sys.argv.index('-n') + 1]) if '-n' in sys.argv else None
    index_xrefs()
    seen, rows = set(), []
    for h, record in registrations():
        if h in seen:
            continue
        seen.add(h)
        _, _, _, vt = cv.resolve(h) or (None,) * 4
        ev = cv.q(vt + 0x20) if vt else None
        if not ev or not is_evaluator(ev):
            continue
        byte = global_byte(ev)
        data = cv.sec['.data']
        default = None
        if byte and 0 <= byte - BASE - data['rva'] < len(cv.blobs['.data']):
            default = cv.blobs['.data'][byte - BASE - data['rva']]
        rows.append((h, record, vt, ev, byte, default))
        if limit and len(rows) >= limit:
            break
    print(f'{len(rows)} config-var classes (16-byte STU class whose slot 4 reads the context)')
    print('hash      record         vtable         evaluator      global byte    default')
    for h, record, vt, ev, byte, default in rows:
        print(f'{h:08X}  {record:x}  {vt:x}  {ev:x}  {byte and hex(byte) or "-":14} {default if default is not None else ""}')


if __name__ == '__main__':
    main()
