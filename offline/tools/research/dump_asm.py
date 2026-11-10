"""Disassemble a stretch of the client's runtime image (ida/Overwatch-runtime.dump: file offset = RVA) with capstone.

    python tools/research/dump_asm.py START END [--dump PATH]

START, END: addresses in the image's own numbering (base 0x7FF6D1220000). A linear sweep, so it also shows code IDA
left out of a function (the obfuscated lookups break its analysis); where the bytes are junk the sweep goes wrong
until the next real instruction. A rip-relative SSE operand gets the float it names, a call its target.
"""
import argparse
import struct
from pathlib import Path

import capstone
from capstone import x86_const as x86

BASE = 0x7FF6D1220000
DUMP = Path(__file__).resolve().parents[2] / 'ida' / 'Overwatch-runtime.dump'
FLOATS = ('ss', 'ps')


def lines(image: bytes, start: int, end: int):
    """(address, text) of each instruction from start to end."""
    engine = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    engine.detail = True
    engine.skipdata = True
    for insn in engine.disasm(image[start - BASE:end - BASE], start):
        note = ''
        if insn.id:  # not skipped data
            for operand in insn.operands:
                if operand.type == x86.X86_OP_MEM and operand.mem.base == x86.X86_REG_RIP and insn.mnemonic[-2:] in FLOATS:
                    at = insn.address + insn.size + operand.mem.disp - BASE
                    count = 4 if insn.mnemonic.endswith('ps') else 1
                    note = '   ; ' + ' '.join(f'{v:g}' for v in struct.unpack_from(f'<{count}f', image, at))
        yield insn.address, f'{insn.mnemonic} {insn.op_str}{note}'


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('start', type=lambda v: int(v, 16))
    parser.add_argument('end', type=lambda v: int(v, 16))
    parser.add_argument('--dump', type=Path, default=DUMP)
    args = parser.parse_args()
    image = args.dump.read_bytes()
    for address, text in lines(image, args.start, args.end):
        print(f'{address:X}  {text}')


if __name__ == '__main__':
    main()
