"""Compare suspension, unchanged DR writes and one dormant watch without debugger attachment.

  python hw_detector_probe.py PID MODE OUTPUT.jsonl [--seconds 1] [--thread TID]

Only a verified game thread is changed (window thread by default). Watch target is its read-only PE header.
Original breakpoint settings are restored and read back; Windows may sanitize raw status/reserved bits.
No process memory writes.
Modes execute and replace-execute put four dormant instruction breakpoints on the PE header.
replace-execute intentionally displaces existing slots and can crash the owned test game.
"""
import argparse
from contextlib import contextmanager
import ctypes
from ctypes import wintypes
import json
from pathlib import Path
import time

from debug_assert import _kernel32, _read_remote, _win_error, find_target_module, validate_pid
from hw_watch import Context, OFF_FLAGS, dr7_value
from volley_probe import MEMORY_BASIC_INFORMATION

DR_OFFSETS = (0x48, 0x50, 0x58, 0x60, 0x68, 0x70)
DEBUG_FLAGS = 0x100010
WAIT_TIMEOUT = 258
MODES = ("read", "same", "arm", "execute", "replace-execute")


def same_debug(left, right):
    # Windows sanitizes reserved/OS-controlled bits; retain raw values in evidence.
    return (left[:4] == right[:4] and (left[4] ^ right[4]) & 0xE00F == 0
            and (left[5] ^ right[5]) & 0xFFFF23FF == 0)


def read_debug(kernel, thread):
    context = Context()
    ctypes.c_uint32.from_address(context.address + OFF_FLAGS).value = DEBUG_FLAGS
    if not kernel.GetThreadContext(thread, ctypes.c_void_p(context.address)):
        raise _win_error()
    return [context.q(offset) for offset in DR_OFFSETS]


def write_debug(kernel, thread, state):
    context = Context()
    ctypes.c_uint32.from_address(context.address + OFF_FLAGS).value = DEBUG_FLAGS
    for offset, value in zip(DR_OFFSETS, state):
        context.set_q(offset, value)
    if not kernel.SetThreadContext(thread, ctypes.c_void_p(context.address)):
        raise _win_error()


@contextmanager
def suspended(kernel, thread):
    previous = kernel.SuspendThread(thread)
    if previous == 0xFFFFFFFF:
        raise _win_error()
    try:
        if previous:
            raise ValueError("thread already suspended; refusing to alter it")
        yield
    finally:
        if kernel.ResumeThread(thread) == 0xFFFFFFFF:
            raise _win_error()


def exercise(kernel, process, thread, address, mode, seconds, emit):
    original, changed, restored, resumed = None, False, False, False
    stopped_at = time.perf_counter()
    try:
        with suspended(kernel, thread):
            try:
                original = read_debug(kernel, thread)
                emit("original", registers=original)
                if mode in ("arm", "execute") and (original[5] & 0xFF or any(original[:4])):
                    raise ValueError("existing hardware breakpoint state; refusing to overwrite it")
                desired = original[:]
                if mode == "arm":
                    desired[0], desired[5] = address, original[5] | dr7_value(1, write_only=True)
                elif mode in ("execute", "replace-execute"):
                    desired[:4] = [address] * 4
                    desired[4], desired[5] = 0, (original[5] & ~0xFFFF00FF) | 0x55
                if mode != "read":
                    changed = True
                    write_debug(kernel, thread, desired)
                    emit("write", registers=desired)
                immediate = read_debug(kernel, thread)
                emit("immediate_readback", registers=immediate, matches=same_debug(immediate, desired),
                     raw_matches=immediate == desired)
                if not same_debug(immediate, desired):
                    raise RuntimeError("debug state write did not read back as requested")
            except BaseException:
                if changed:
                    write_debug(kernel, thread, original)
                    restored = same_debug(read_debug(kernel, thread), original)
                    emit("error_restore", matches=restored)
                    changed = not restored
                raise
        resumed = True
        emit("resumed", suspension_elapsed_ms=round((time.perf_counter() - stopped_at) * 1000, 3))
        wait = kernel.WaitForSingleObject(process, round(seconds * 1000))
        if wait not in (0, WAIT_TIMEOUT):
            raise _win_error()
        survived = wait == WAIT_TIMEOUT
        emit("interval_end", survived=survived)
    finally:
        if (changed or resumed) and kernel.WaitForSingleObject(process, 0) != 0:
            with suspended(kernel, thread):
                if changed:
                    try:
                        before = read_debug(kernel, thread)
                        emit("before_restore", registers=before)
                    finally:
                        write_debug(kernel, thread, original)
                        restored = same_debug(read_debug(kernel, thread), original)
                        emit("restored", matches=restored)
                        if not restored:
                            raise RuntimeError("original debug state restoration did not read back")
                else:
                    emit("final_readback", registers=read_debug(kernel, thread))
    return {"survived": survived, "restored": restored if changed or restored else None}


def run(pid, mode, seconds, emit, thread_id=None):
    validate_pid(pid)
    if mode not in MODES or not 0 < seconds <= 5:
        raise ValueError("unsupported mode; interval greater than 0 and at most 5 seconds")
    kernel = _kernel32()
    for name in ("SuspendThread", "ResumeThread"):
        function = getattr(kernel, name)
        function.argtypes, function.restype = [wintypes.HANDLE], wintypes.DWORD
    kernel.SetThreadContext.argtypes = [wintypes.HANDLE, ctypes.c_void_p]
    kernel.SetThreadContext.restype = wintypes.BOOL
    kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    kernel.WaitForSingleObject.restype = wintypes.DWORD
    kernel.GetExitCodeProcess.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
    kernel.GetExitCodeProcess.restype = wintypes.BOOL
    kernel.VirtualQueryEx.argtypes = [wintypes.HANDLE, ctypes.c_void_p,
                                    ctypes.POINTER(MEMORY_BASIC_INFORMATION), ctypes.c_size_t]
    kernel.VirtualQueryEx.restype = ctypes.c_size_t
    kernel.GetProcessIdOfThread.argtypes, kernel.GetProcessIdOfThread.restype = [wintypes.HANDLE], wintypes.DWORD
    tid = thread_id
    if tid is None:
        user = ctypes.WinDLL("user32", use_last_error=True)
        user.FindWindowW.argtypes, user.FindWindowW.restype = [wintypes.LPCWSTR, wintypes.LPCWSTR], wintypes.HWND
        user.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
        user.GetWindowThreadProcessId.restype = wintypes.DWORD
        owner = wintypes.DWORD()
        tid = user.GetWindowThreadProcessId(user.FindWindowW("tankWindowClass", None), ctypes.byref(owner))
        if not tid or owner.value != pid:
            raise ValueError("verified PID does not own the game window")
    validate_pid(tid)
    base, size = find_target_module(kernel, pid)
    process = kernel.OpenProcess(0x100000 | 0x0400 | 0x0010, False, pid)
    if not process:
        raise _win_error()
    try:
        region = MEMORY_BASIC_INFORMATION()
        if not kernel.VirtualQueryEx(process, ctypes.c_void_p(base), ctypes.byref(region), ctypes.sizeof(region)):
            raise _win_error()
        if region.State != 0x1000 or region.Protect not in (0x02, 0x20) or _read_remote(kernel, process, base, 2) != b"MZ":
            raise ValueError("watch target is not a committed read-only PE header")
        thread = kernel.OpenThread(0x81A, False, tid)
        if not thread:
            raise _win_error()
        try:
            if kernel.GetProcessIdOfThread(thread) != pid:
                raise ValueError("selected thread is not owned by the verified game PID")
            emit("begin", pid=pid, thread_id=tid, mode=mode, seconds=seconds,
                 module_base=base, module_size=size, header_protection=region.Protect,
                 debugger_attached=False)
            result = exercise(kernel, process, thread, base, mode, seconds, emit)
            code = wintypes.DWORD()
            if not kernel.GetExitCodeProcess(process, ctypes.byref(code)):
                raise _win_error()
            result["exit_code"] = code.value
            emit("result", **result)
            return result
        finally:
            kernel.CloseHandle(thread)
    finally:
        kernel.CloseHandle(process)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pid", type=int)
    parser.add_argument("mode", choices=MODES)
    parser.add_argument("output", type=Path)
    parser.add_argument("--seconds", type=float, default=1)
    parser.add_argument("--thread", type=int)
    args = parser.parse_args()
    started = time.monotonic()
    with args.output.open("x", encoding="utf-8") as log:
        def emit(phase, **fields):
            row = {"phase": phase, "elapsed": round(time.monotonic() - started, 6), **fields}
            line = json.dumps(row)
            print(line, flush=True)
            log.write(line + "\n")
            log.flush()
        try:
            run(args.pid, args.mode, args.seconds, emit, args.thread)
        except Exception as error:
            emit("error", type=type(error).__name__, message=str(error), winerror=getattr(error, "winerror", None))
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
