"""Run client functions in unicorn over lazily copied live memory; read-only, no debugger.

Pages are copied from the verified client on first touch; writes stay in the emulator. Each call gets a real
call-site return address so the client's caller checks pass. TLS reads use a real thread's TEB.

  python live_emu.py PID VOLLEY [--thread N] [--anim ADDR --ms N]

Evaluates the rifle fire-animation gates for a Volley state (address from volley_probe.py) and, with --anim,
lists the writes the play call 0x7FF6D17EFB90 would make. Addresses use image base 0x7FF6D1220000 (the IDA
dump); call() rebases them onto wherever the client is loaded.
"""
import argparse
import ctypes
from ctypes import wintypes
import struct

import unicorn
from unicorn import x86_const as X

from debug_assert import PROCESS_VM_READ, _kernel32, _read_remote, find_target_module

PAGE = 0x1000
STACK, STACK_SIZE = 0x10000000, 0x100000
IMAGE_BASE = 0x7FF6D1220000
THREAD_QUERY_LIMITED_INFORMATION = 0x0800


class THREADENTRY32(ctypes.Structure):
    _fields_ = [('dwSize', wintypes.DWORD), ('cntUsage', wintypes.DWORD), ('th32ThreadID', wintypes.DWORD),
                ('th32OwnerProcessID', wintypes.DWORD), ('tpBasePri', wintypes.LONG),
                ('tpDeltaPri', wintypes.LONG), ('dwFlags', wintypes.DWORD)]


def thread_tebs(pid):
    k32 = ctypes.WinDLL('kernel32', use_last_error=True)
    k32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    k32.OpenThread.restype = wintypes.HANDLE
    ntdll = ctypes.WinDLL('ntdll')
    snapshot = k32.CreateToolhelp32Snapshot(4, 0)  # TH32CS_SNAPTHREAD
    entry, tebs = THREADENTRY32(), []
    entry.dwSize = ctypes.sizeof(entry)
    ok = k32.Thread32First(wintypes.HANDLE(snapshot), ctypes.byref(entry))
    while ok:
        if entry.th32OwnerProcessID == pid:
            thread = k32.OpenThread(THREAD_QUERY_LIMITED_INFORMATION, False, entry.th32ThreadID)
            info = (ctypes.c_byte * 48)()  # THREAD_BASIC_INFORMATION; TebBaseAddress at +8
            if thread and ntdll.NtQueryInformationThread(wintypes.HANDLE(thread), 0, info, 48, None) == 0:
                tebs.append(struct.unpack_from('<Q', bytes(info), 8)[0])
            if thread:
                k32.CloseHandle(wintypes.HANDLE(thread))
        ok = k32.Thread32Next(wintypes.HANDLE(snapshot), ctypes.byref(entry))
    k32.CloseHandle(wintypes.HANDLE(snapshot))
    return tebs


class LiveEmulator:
    def __init__(self, pid, thread=0):
        self.k32 = _kernel32()
        base, _ = find_target_module(self.k32, pid)
        self.delta = base - IMAGE_BASE  # dump address + delta = live address
        self.process = self.k32.OpenProcess(PROCESS_VM_READ, False, pid)
        if not self.process:
            raise OSError('cannot open verified client read-only')
        self.uc = unicorn.Uc(unicorn.UC_ARCH_X86, unicorn.UC_MODE_64)
        self.uc.mem_map(STACK, STACK_SIZE)
        teb = thread_tebs(pid)[thread]
        self.uc.reg_write(X.UC_X86_REG_GS_BASE, teb)
        self.uc.hook_add(unicorn.UC_HOOK_MEM_UNMAPPED, self._copy_page)
        # the TEB's stack range (+0x08 base, +0x10 limit) must be the emulator's: __chkstk probes down to the limit
        if self._copy_page(self.uc, 0, teb, 0, 0, None):
            self.uc.mem_write(teb + 0x08, struct.pack('<QQ', STACK + STACK_SIZE, STACK))
        self.writes = []
        self.uc.hook_add(unicorn.UC_HOOK_MEM_WRITE, self._record_write)
        self.fault, self.stopped_at = None, None

    def _copy_page(self, uc, access, address, size, value, data):
        page = address & ~(PAGE - 1)
        raw = b''.join(_read_remote(self.k32, self.process, page + o, 512) for o in range(0, PAGE, 512))
        if len(raw) != PAGE:
            return False
        uc.mem_map(page, PAGE)
        uc.mem_write(page, raw)
        return True

    def _record_write(self, uc, access, address, size, value, data):
        if not STACK <= address < STACK + STACK_SIZE:
            self.writes.append((uc.reg_read(X.UC_X86_REG_RIP), address, size, value & ((1 << 8 * size) - 1)))

    def copy(self, address):
        """Copy the live page holding address now, unless the emulator already has it."""
        try:
            self.uc.mem_read(address, 1)
        except unicorn.UcError:
            self._copy_page(self.uc, 0, address, 0, 0, None)

    def live(self, dump_address):
        return dump_address + self.delta

    def stop_at(self, *dump_addresses):
        """Halt emulation when execution reaches any of these (dump) addresses; recorded in stopped_at."""
        def hit(uc, address, size, data):
            self.stopped_at = address - self.delta
            uc.emu_stop()
        for address in dump_addresses:
            self.uc.hook_add(unicorn.UC_HOOK_CODE, hit, begin=self.live(address), end=self.live(address))

    def call(self, function, return_address, *args):
        """function(*args) returning to a real call site; None if it faults, stops early or does not get there.
        Code addresses are dump addresses; fault records (dump RIP, error) when unicorn fails."""
        function, return_address = self.live(function), self.live(return_address)
        rsp = STACK + STACK_SIZE - 0x1000
        self.uc.mem_write(rsp, struct.pack('<Q', return_address))
        self.uc.reg_write(X.UC_X86_REG_RSP, rsp)
        for register, value in zip((X.UC_X86_REG_RCX, X.UC_X86_REG_RDX, X.UC_X86_REG_R8, X.UC_X86_REG_R9), args):
            self.uc.reg_write(register, value)
        self.writes.clear()
        self.fault, self.stopped_at = None, None
        try:
            self.uc.emu_start(function, return_address, count=5_000_000)
        except unicorn.UcError as error:
            self.fault = (self.uc.reg_read(X.UC_X86_REG_RIP) - self.delta, str(error))
            return None
        if self.uc.reg_read(X.UC_X86_REG_RIP) != return_address:
            return None
        return self.uc.reg_read(X.UC_X86_REG_RAX)


def fire_gates(emulator, volley):
    """The spawner 0x7FF6D1D064F0's gates for the play call at 0x7FF6D1D06DA4, called as the spawner does."""
    entity = emulator.call(0x7FF6D1EE6EF0, 0x7FF6D1D0652E, volley)
    context = emulator.call(0x7FF6D2D91C50, 0x7FF6D1D0653D, volley)
    component = emulator.call(0x7FF6D1B99210, 0x7FF6D1D06545, context)
    predicate = emulator.call(0x7FF6D17A74F0, 0x7FF6D1D06557, component, entity)
    target = emulator.call(0x7FF6D17A1330, 0x7FF6D1D06D7C, component)
    return {'entity': entity, 'component': component, 'predicate': None if predicate is None else predicate & 0xFF,
            'target': target}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('pid', type=int)
    parser.add_argument('volley', type=lambda v: int(v, 16))
    parser.add_argument('--thread', type=int, default=0, help='index of the thread whose TLS to use')
    parser.add_argument('--anim', type=lambda v: int(v, 16), help='[info+0x40] from volley_probe.py')
    parser.add_argument('--ms', type=int, default=0)
    args = parser.parse_args(argv)
    emulator = LiveEmulator(args.pid, args.thread)
    gates = fire_gates(emulator, args.volley)
    print(' '.join('%s=%s' % (k, v if v is None or k == 'predicate' else '%X' % v) for k, v in gates.items()))
    if args.anim and gates['target']:
        emulator.call(0x7FF6D17EFB90, 0x7FF6D1D06DA9, gates['target'], args.anim, args.ms)
        for rip, address, size, value in emulator.writes:
            print('  rip %X  [%X] (+%X) <- %X (%d bytes)' % (rip, address, address - gates['target'], value, size))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
