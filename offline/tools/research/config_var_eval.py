"""A config-var class hash -> its runtime record, vtable and evaluator (vtable slot 4), from a runtime snapshot of
the client (text/rdata/data .bin + snapshot.json); prints the evaluator's first instructions. Read-only.

    python config_var_eval.py HASH [HASH...] [-n COUNT]   (env OW_SNAPSHOT: the snapshot folder)

The STU registration loads the record (lea rax, record) right before `mov r8d, hash`; the runtime registration
stores the same record beside the class's create function (test rcx; je; lea vtable; mov [rcx], vtable).
research/play-menu.md (PLAY cards) has the method."""
import os
import json, os, struct, sys
from pathlib import Path
import capstone

BASE = 0x7FF6D1220000
snap = Path(os.environ.get('OW_SNAPSHOT', Path(__file__).resolve().parents[2] / '.scratchpad' / 'runtime-weapon'))
meta = json.loads((snap / 'snapshot.json').read_text())
sec = {s['name']: s for s in meta['sections']}
blobs = {n: (snap / f'{n[1:]}.bin').read_bytes() for n in ('.text', '.rdata', '.data')}
md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
md.detail = True


def q(addr):
    for n, b in blobs.items():
        off = addr - BASE - sec[n]['rva']
        if 0 <= off <= len(b) - 8:
            return struct.unpack_from('<Q', b, off)[0]


def code(addr, n):
    off = addr - BASE - sec['.text']['rva']
    return list(md.disasm(blobs['.text'][off:off + n * 15], addr))[:n]


def rip(i, k=1):
    o = i.operands[k]
    if o.type == capstone.x86.X86_OP_MEM and o.mem.base == capstone.x86.X86_REG_RIP:
        return i.address + i.size + o.mem.disp


def xrefs(target):
    import re
    text, start, out = blobs['.text'], BASE + sec['.text']['rva'], []
    for m in re.finditer(bytes([0x5B, 5, 0x0D, 0x15, 0x1D, 0x25, 0x2D, 0x35, 0x3D, 0x5D]), text):
        p = m.start()
        if p + 5 <= len(text) and start + p + 5 + struct.unpack_from('<i', text, p + 1)[0] == target:
            out.append(start + p + 5)  # the next instruction's address
    return out


def runtime_create(record):
    """The runtime registration stores the record next to the class's create function (test rcx; je; lea vt;
    mov [rcx], vt): return that function."""
    for end in xrefs(record):
        for back in range(8, 80):
            for i in code(end - back, 14):
                t = rip(i) if i.mnemonic == 'lea' and i.op_str.startswith('rax') else None
                if t and BASE + sec['.text']['rva'] <= t < BASE + sec['.text']['rva'] + len(blobs['.text']):
                    head = code(t, 4)
                    if len(head) == 4 and head[0].mnemonic == 'test' and head[2].mnemonic == 'lea':
                        return t
    return None


def resolve(h):
    text = blobs['.text']
    at = text.find(b'\x41\xb8' + struct.pack('<I', h))  # mov r8d, hash
    if at < 0:
        return None
    va = BASE + sec['.text']['rva'] + at
    ins = code(va - 24, 40)
    start = [i for i in ins if i.address >= va - 24]
    prev = code(va - 7, 1)  # the record pointer is loaded right before the hash: lea rax, [rip + record]
    record = rip(prev[0]) if prev and prev[0].mnemonic == 'lea' and prev[0].address + prev[0].size == va else None
    if record is None:
        return va, None, None, None
    create, vt = runtime_create(record), None
    if create:
        target = create
        for i in code(create, 4):
            if i.mnemonic == 'jmp' and i.operands[0].type == capstone.x86.X86_OP_IMM:
                target = i.operands[0].imm
        regs = {}
        for i in code(target, 80):
            if i.mnemonic == 'lea' and rip(i):
                regs[i.reg_name(i.operands[0].reg)] = rip(i)
            if (i.mnemonic == 'mov' and i.operands[0].type == capstone.x86.X86_OP_MEM and i.operands[0].mem.disp == 0
                    and i.operands[1].type == capstone.x86.X86_OP_REG and i.reg_name(i.operands[1].reg) in regs):
                vt = regs[i.reg_name(i.operands[1].reg)]
            if i.mnemonic == 'ret':
                break
    return va, record, create, vt


def main():
  n = 30
  args = sys.argv[1:]
  if '-n' in args:
      n = int(args[args.index('-n') + 1]); args = args[:args.index('-n')]
  for h in args:
      va, record, create, vt = resolve(int(h, 16)) or (None,) * 4
      ev = q(vt + 0x20) if vt else None
      print(f'== {h}: reg {va and hex(va)} record {record and hex(record)} create {create and hex(create)} '
            f'vtable {vt and hex(vt)} eval {ev and hex(ev)}')
      if ev:
          for i in code(ev, n):
              print(f'   {i.address:x} {i.mnemonic} {i.op_str}')


if __name__ == '__main__':
    main()
