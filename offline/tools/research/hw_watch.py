"""Count which instructions access up to four addresses in the verified 1.74 client, using hardware
data breakpoints. Read-only: no process memory is written. The tool attaches as a debugger, arms
DR0-DR3 on each thread as the attach reports it, and disarms every thread before detaching.

  python hw_watch.py PID SECONDS ADDRESS [ADDRESS...] [--write]

  python hw_watch.py PID SECONDS RVA [RVA...] --rva --execute [--deref rsi+0xF8 ...]

Output: JSON with, per hit RIP, the count, the watched slot and the first return-address RVAs.
Each RIP is the instruction after the access (data breakpoints trap after the instruction).
--execute arms instruction breakpoints instead (they trap before the instruction; the resume sets
RF so it runs once). Each sample also records rax, rcx, rdx, rsi, r8 and every --deref REG+OFF qword.
"""
import argparse
import ctypes
from ctypes import wintypes
import json
import struct
import sys
import time

from debug_assert import (
    CREATE_PROCESS_DEBUG_EVENT, CREATE_THREAD_DEBUG_EVENT, DBG_CONTINUE, DBG_EXCEPTION_NOT_HANDLED,
    DEBUG_EVENT, EXCEPTION_BREAKPOINT, EXCEPTION_DEBUG_EVENT, EXIT_PROCESS_DEBUG_EVENT,
    LOAD_DLL_DEBUG_EVENT, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ, _kernel32, _read_remote,
    _win_error, filtered_return_rvas, find_target_module, validate_pid,
)

EXCEPTION_SINGLE_STEP = 0x80000004
CONTEXT_FULL_DEBUG = 0x00100000 | 0x1 | 0x2 | 0x10  # AMD64 control, integer, debug registers
CONTEXT_SIZE = 0x4D0
OFF_FLAGS, OFF_DR0, OFF_DR6, OFF_DR7, OFF_RSP, OFF_RIP = 0x30, 0x48, 0x68, 0x70, 0x98, 0xF8
OFF_EFLAGS, RF = 0x44, 1 << 16
REGS = {"rax": 0x78, "rcx": 0x80, "rdx": 0x88, "rbx": 0x90, "rsp": 0x98, "rbp": 0xA0, "rsi": 0xA8, "rdi": 0xB0,
        "r8": 0xB8, "r9": 0xC0}
SAMPLE_REGS = ("rax", "rcx", "rdx", "rsi", "r8")
MAX_HITS = 50000
MAX_SAMPLES = 4


def dr7_value(count, write_only=False, length=4, execute=False):
    """Local-enable DR0..DR(count-1); RW 01 write or 11 read/write; LEN 11 = 4 bytes, 10 = 8 bytes.
    execute: RW 00 and LEN 00 (instruction breakpoints)."""
    if not 1 <= count <= 4 or length not in (4, 8):
        raise ValueError("1-4 watches of 4 or 8 bytes")
    rw = 0b01 if write_only else 0b11
    ln = 0b11 if length == 4 else 0b10
    if execute:
        rw = ln = 0
    value = 0
    for i in range(count):
        value |= 1 << (2 * i)
        value |= (rw | ln << 2) << (16 + 4 * i)
    return value


class Context:
    def __init__(self):
        self.raw = ctypes.create_string_buffer(CONTEXT_SIZE + 15)
        self.address = (ctypes.addressof(self.raw) + 15) & ~15

    def view(self):
        return (ctypes.c_ubyte * CONTEXT_SIZE).from_address(self.address)

    def get(self, k32, thread):
        ctypes.c_uint32.from_address(self.address + OFF_FLAGS).value = CONTEXT_FULL_DEBUG
        return bool(k32.GetThreadContext(thread, ctypes.c_void_p(self.address)))

    def q(self, offset):
        return ctypes.c_uint64.from_address(self.address + offset).value

    def set_q(self, offset, value):
        ctypes.c_uint64.from_address(self.address + offset).value = value


def arm(k32, thread, addresses, dr7):
    ctx = Context()
    if not ctx.get(k32, thread):
        return False
    for i in range(4):
        ctx.set_q(OFF_DR0 + 8 * i, addresses[i] if i < len(addresses) else 0)
    ctx.set_q(OFF_DR6, 0)
    ctx.set_q(OFF_DR7, dr7)
    return bool(k32.SetThreadContext(thread, ctypes.c_void_p(ctx.address)))


def disarm(k32, thread):
    if k32.SuspendThread(thread) == 0xFFFFFFFF:
        return False
    try:
        return arm(k32, thread, [], 0)
    finally:
        k32.ResumeThread(thread)


def parse_deref(text):
    """'rsi+0xF8' -> ('rsi', 0xF8)."""
    reg, _, off = text.partition("+")
    if reg not in REGS:
        raise ValueError("unknown register %r" % reg)
    return reg, int(off or "0", 0)


def watch(pid, seconds, addresses, write_only=False, execute=False, rva=False, derefs=()):
    validate_pid(pid)
    if not 1 <= seconds <= 300 or not 1 <= len(addresses) <= 4:
        raise ValueError("1-300 seconds and 1-4 addresses")
    k32 = _kernel32()
    k32.SetThreadContext.argtypes = [wintypes.HANDLE, ctypes.c_void_p]
    k32.SetThreadContext.restype = wintypes.BOOL
    k32.SuspendThread.argtypes = k32.ResumeThread.argtypes = [wintypes.HANDLE]
    k32.SuspendThread.restype = k32.ResumeThread.restype = wintypes.DWORD
    base, size = find_target_module(k32, pid)
    if rva:
        addresses = [base + a for a in addresses]
    process = k32.OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if not process:
        raise _win_error()
    dr7 = dr7_value(len(addresses), write_only, execute=execute)
    threads, hits, total, armed = {}, {}, 0, 0
    exited, attached = False, False
    started = time.monotonic()
    try:
        if not k32.DebugActiveProcess(pid):
            raise _win_error()
        attached = True
        k32.DebugSetProcessKillOnExit(False)
        event, deadline = DEBUG_EVENT(), started + seconds
        while time.monotonic() < deadline and total < MAX_HITS:
            if not k32.WaitForDebugEvent(ctypes.byref(event), 100):
                continue
            code, tid, status = int(event.dwDebugEventCode), int(event.dwThreadId), DBG_CONTINUE
            if code == CREATE_PROCESS_DEBUG_EVENT:
                info = event.u.CreateProcessInfo
                for handle in (info.hFile, info.hProcess):
                    if handle:
                        k32.CloseHandle(handle)
                threads[tid] = info.hThread
                armed += arm(k32, info.hThread, addresses, dr7)
            elif code == CREATE_THREAD_DEBUG_EVENT:
                threads[tid] = event.u.CreateThread.hThread
                armed += arm(k32, event.u.CreateThread.hThread, addresses, dr7)
            elif code == LOAD_DLL_DEBUG_EVENT and event.u.LoadDll.hFile:
                k32.CloseHandle(event.u.LoadDll.hFile)
            elif code == EXIT_PROCESS_DEBUG_EVENT:
                exited = True
            elif code == EXCEPTION_DEBUG_EVENT:
                record = event.u.Exception.ExceptionRecord
                if int(record.ExceptionCode) == EXCEPTION_SINGLE_STEP and tid in threads:
                    ctx = Context()
                    if ctx.get(k32, threads[tid]):
                        dr6, rip = ctx.q(OFF_DR6), ctx.q(OFF_RIP)
                        slot = next((i for i in range(4) if dr6 >> i & 1), -1)
                        key = (rip - base if base <= rip < base + size else rip, slot)
                        entry = hits.setdefault(key, {"count": 0, "callers": []})
                        entry["count"] += 1
                        total += 1
                        if len(entry["callers"]) < MAX_SAMPLES:
                            stack = _read_remote(k32, process, ctx.q(OFF_RSP), 256)
                            entry["callers"].append(filtered_return_rvas(stack, base, size)[:6])
                            sample = {r: hex(ctx.q(REGS[r])) for r in SAMPLE_REGS}
                            for reg, off in derefs:
                                try:
                                    sample["[%s+%#x]" % (reg, off)] = hex(struct.unpack(
                                        "<Q", _read_remote(k32, process, ctx.q(REGS[reg]) + off, 8))[0])
                                except (OSError, ValueError):
                                    sample["[%s+%#x]" % (reg, off)] = None
                            entry.setdefault("samples", []).append(sample)
                        ctx.set_q(OFF_DR6, 0)
                        if execute:  # resume flag: run the trapped instruction once
                            flags = ctypes.c_uint32.from_address(ctx.address + OFF_EFLAGS)
                            flags.value |= RF
                        k32.SetThreadContext(threads[tid], ctypes.c_void_p(ctx.address))
                elif int(record.ExceptionCode) != EXCEPTION_BREAKPOINT:
                    status = DBG_EXCEPTION_NOT_HANDLED
            k32.ContinueDebugEvent(event.dwProcessId, tid, status)
            if exited:
                break
    finally:
        disarmed = 0
        if not exited:
            for handle in threads.values():
                disarmed += disarm(k32, handle)
        for handle in threads.values():
            k32.CloseHandle(handle)
        if attached:
            k32.DebugActiveProcessStop(pid)
        k32.CloseHandle(process)
    return {
        "pid": pid, "module_base": base, "addresses": [hex(a) for a in addresses],
        "threads": len(threads), "armed": armed, "disarmed": disarmed, "exited": exited,
        "total_hits": total, "elapsed": round(time.monotonic() - started, 2),
        "hits": [{"rip_rva": hex(k[0]), "slot": k[1], **v}
                 for k, v in sorted(hits.items(), key=lambda kv: -kv[1]["count"])],
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("pid", type=int)
    parser.add_argument("seconds", type=int)
    parser.add_argument("addresses", nargs="+", type=lambda s: int(s, 0))
    parser.add_argument("--write", action="store_true", help="trap writes only")
    parser.add_argument("--execute", action="store_true", help="instruction breakpoints")
    parser.add_argument("--rva", action="store_true", help="addresses are RVAs in the verified client")
    parser.add_argument("--deref", action="append", default=[], type=parse_deref, help="REG+OFF qword per sample")
    args = parser.parse_args(argv)
    result = watch(args.pid, args.seconds, args.addresses, args.write, args.execute, args.rva, args.deref)
    for hit in result["hits"]:
        hit["callers"] = [[hex(r) for r in c] for c in hit["callers"]]
    print(json.dumps(result, indent=1))


if __name__ == "__main__":
    sys.exit(main())
