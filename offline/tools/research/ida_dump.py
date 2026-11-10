"""IDAPython, headless: dump functions' disassembly (with the float constants their SSE operands name) to a text file.

    idat.exe -A -S"ida_dump.py OUT ADDR [ADDR ...]" database.i64

Leaves the database as it was (DBFL_KILL: nothing is saved).
"""
import struct

import ida_bytes
import ida_funcs
import idaapi
import idautils
import idc

out = open(idc.ARGV[1], 'w', encoding='utf-8')
for start in (int(a, 16) for a in idc.ARGV[2:]):
    func = ida_funcs.get_func(start)
    if func is None:
        out.write(f'=== {start:X}: not a function\n')
        continue
    out.write(f'=== {func.start_ea:X} size {func.size():X}\n')
    for ea in idautils.FuncItems(func.start_ea):
        note = ''
        mnem = idc.print_insn_mnem(ea)
        for i in range(3):
            if idc.get_operand_type(ea, i) == idc.o_mem and (mnem[-2:] in ('ss', 'ps', 'sd') or mnem in ('movd', 'movq')):
                raw = ida_bytes.get_bytes(idc.get_operand_value(ea, i), 16) or b''
                if len(raw) >= 4:
                    note = '   ; f32 ' + ' '.join(f'{v:g}' for v in struct.unpack('<4f', raw.ljust(16, b'\0'))[:4 if mnem[-2:] == 'ps' else 1])
        out.write(f'{ea:X}  {idc.generate_disasm_line(ea, 0)}{note}\n')
    try:
        out.write('--- decompiled\n' + str(idaapi.decompile(func.start_ea)) + '\n')
    except Exception as error:  # Hex-Rays gives up on the obfuscated lookups
        out.write(f'--- decompile failed: {error}\n')
out.close()
idaapi.set_database_flag(idaapi.DBFL_KILL)
idc.qexit(0)
