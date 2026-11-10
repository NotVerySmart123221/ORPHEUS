"""Observe exceptions from the verified Overwatch 1.74 process without changing it."""
import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import struct
import sys
import time

LOBBY_TOOLS = Path(__file__).resolve().parents[1] / "lobby"
if str(LOBBY_TOOLS) not in sys.path:
    sys.path.insert(0, str(LOBBY_TOOLS))
from run import CLIENT_SHA256

EXPECTED_EXE = Path(os.environ.get('OW174_GAME', 'C:/Games/Overwatch')) / '_retail_' / 'Overwatch.exe'
CONTEXT_SIZE = 0x4D0
CONTEXT_FLAGS_OFFSET = 0x30
# Microsoft ABI: https://learn.microsoft.com/windows/win32/api/winnt/ns-winnt-context
# Debug events: https://learn.microsoft.com/windows/win32/api/minwinbase/ns-minwinbase-debug_event
CONTEXT_RSP_OFFSET = 0x98
CONTEXT_RIP_OFFSET = 0xF8
CONTEXT_AMD64 = 0x00100000
CONTEXT_CONTROL = 0x1
CONTEXT_INTEGER = 0x2
MAX_READ = 512
MAX_EXCEPTIONS = 256

CREATE_THREAD_DEBUG_EVENT = 2
CREATE_PROCESS_DEBUG_EVENT = 3
EXIT_PROCESS_DEBUG_EVENT = 5
LOAD_DLL_DEBUG_EVENT = 6
EXCEPTION_DEBUG_EVENT = 1
EXCEPTION_BREAKPOINT = 0x80000003
DBG_CONTINUE = 0x00010002
DBG_EXCEPTION_NOT_HANDLED = 0x80010001
TH32CS_SNAPMODULE = 0x00000008
TH32CS_SNAPMODULE32 = 0x00000010
PROCESS_VM_READ = 0x0010
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
THREAD_GET_CONTEXT = 0x0008
INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value


class EXCEPTION_RECORD(ctypes.Structure):
    # Pointer padding and ULONG_PTR fields follow the x64 EXCEPTION_RECORD layout.
    _fields_ = [
        ("ExceptionCode", wintypes.DWORD),
        ("ExceptionFlags", wintypes.DWORD),
        ("ExceptionRecord", ctypes.c_void_p),
        ("ExceptionAddress", ctypes.c_void_p),
        ("NumberParameters", wintypes.DWORD),
        ("ExceptionInformation", ctypes.c_ulonglong * 15),
    ]


class EXCEPTION_DEBUG_INFO(ctypes.Structure):
    _fields_ = [("ExceptionRecord", EXCEPTION_RECORD), ("dwFirstChance", wintypes.DWORD)]


class CREATE_THREAD_DEBUG_INFO(ctypes.Structure):
    _fields_ = [
        ("hThread", ctypes.c_void_p),
        ("lpThreadLocalBase", ctypes.c_void_p),
        ("lpStartAddress", ctypes.c_void_p),
    ]


class CREATE_PROCESS_DEBUG_INFO(ctypes.Structure):
    _fields_ = [
        ("hFile", ctypes.c_void_p),
        ("hProcess", ctypes.c_void_p),
        ("hThread", ctypes.c_void_p),
        ("lpBaseOfImage", ctypes.c_void_p),
        ("dwDebugInfoFileOffset", wintypes.DWORD),
        ("nDebugInfoSize", wintypes.DWORD),
        ("lpThreadLocalBase", ctypes.c_void_p),
        ("lpStartAddress", ctypes.c_void_p),
        ("lpImageName", ctypes.c_void_p),
        ("fUnicode", wintypes.WORD),
    ]


class LOAD_DLL_DEBUG_INFO(ctypes.Structure):
    _fields_ = [
        ("hFile", ctypes.c_void_p),
        ("lpBaseOfDll", ctypes.c_void_p),
        ("dwDebugInfoFileOffset", wintypes.DWORD),
        ("nDebugInfoSize", wintypes.DWORD),
        ("lpImageName", ctypes.c_void_p),
        ("fUnicode", wintypes.WORD),
    ]


class DEBUG_EVENT_UNION(ctypes.Union):
    _fields_ = [
        ("Exception", EXCEPTION_DEBUG_INFO),
        ("CreateThread", CREATE_THREAD_DEBUG_INFO),
        ("CreateProcessInfo", CREATE_PROCESS_DEBUG_INFO),
        ("LoadDll", LOAD_DLL_DEBUG_INFO),
    ]


class DEBUG_EVENT(ctypes.Structure):
    _fields_ = [
        ("dwDebugEventCode", wintypes.DWORD),
        ("dwProcessId", wintypes.DWORD),
        ("dwThreadId", wintypes.DWORD),
        ("u", DEBUG_EVENT_UNION),
    ]


class MODULEENTRY32W(ctypes.Structure):
    _fields_ = [
        ("dwSize", wintypes.DWORD),
        ("th32ModuleID", wintypes.DWORD),
        ("th32ProcessID", wintypes.DWORD),
        ("GlblcntUsage", wintypes.DWORD),
        ("ProccntUsage", wintypes.DWORD),
        ("modBaseAddr", ctypes.c_void_p),
        ("modBaseSize", wintypes.DWORD),
        ("hModule", ctypes.c_void_p),
        ("szModule", wintypes.WCHAR * 256),
        ("szExePath", wintypes.WCHAR * 260),
    ]


assert ctypes.sizeof(ctypes.c_void_p) == 8, "debug_assert requires 64-bit Python"
assert ctypes.sizeof(EXCEPTION_RECORD) == 152
assert ctypes.sizeof(DEBUG_EVENT) == 176
assert CONTEXT_RSP_OFFSET == 0x98 and CONTEXT_RIP_OFFSET == 0xF8


def validate_pid(pid: int) -> int:
    if isinstance(pid, bool) or not isinstance(pid, int) or not 1 <= pid <= 0xFFFFFFFF:
        raise ValueError("pid must be an integer in 1..4294967295")
    return pid


def parse_context_buffer(buffer) -> tuple[int, int]:
    if len(buffer) < CONTEXT_SIZE:
        raise ValueError("short x64 CONTEXT buffer")
    rip = struct.unpack_from("<Q", buffer, CONTEXT_RIP_OFFSET)[0]
    rsp = struct.unpack_from("<Q", buffer, CONTEXT_RSP_OFFSET)[0]
    return rip, rsp


def module_rva(address: int, base: int, size: int):
    return address - base if base <= address < base + size else None


def filtered_return_rvas(stack_bytes: bytes, base: int, size: int) -> list[int]:
    if len(stack_bytes) > MAX_READ or base < 0 or size <= 0:
        raise ValueError("invalid bounded stack or module range")
    return [
        value - base
        for (value,) in struct.iter_unpack("<Q", stack_bytes[: len(stack_bytes) & ~7])
        if base <= value < base + size
    ]


def _win_error():
    return ctypes.WinError(ctypes.get_last_error())


def _canonical(path: str | Path) -> str:
    value = os.fspath(path)
    if value.startswith("\\\\?\\"):
        value = value[4:]
    return os.path.normcase(os.path.abspath(value))


def _hash_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _kernel32():
    if os.name != "nt":
        raise OSError("Windows debugger APIs are required")
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    handle = wintypes.HANDLE
    boolean = wintypes.BOOL
    dword = wintypes.DWORD
    k32.CreateToolhelp32Snapshot.argtypes = [dword, dword]
    k32.CreateToolhelp32Snapshot.restype = handle
    k32.Module32FirstW.argtypes = [handle, ctypes.POINTER(MODULEENTRY32W)]
    k32.Module32FirstW.restype = boolean
    k32.Module32NextW.argtypes = [handle, ctypes.POINTER(MODULEENTRY32W)]
    k32.Module32NextW.restype = boolean
    k32.OpenProcess.argtypes = [dword, boolean, dword]
    k32.OpenProcess.restype = handle
    k32.OpenThread.argtypes = [dword, boolean, dword]
    k32.OpenThread.restype = handle
    k32.CloseHandle.argtypes = [handle]
    k32.CloseHandle.restype = boolean
    k32.DebugActiveProcess.argtypes = [dword]
    k32.DebugActiveProcess.restype = boolean
    k32.DebugActiveProcessStop.argtypes = [dword]
    k32.DebugActiveProcessStop.restype = boolean
    k32.DebugSetProcessKillOnExit.argtypes = [boolean]
    k32.DebugSetProcessKillOnExit.restype = boolean
    k32.WaitForDebugEvent.argtypes = [ctypes.POINTER(DEBUG_EVENT), dword]
    k32.WaitForDebugEvent.restype = boolean
    k32.ContinueDebugEvent.argtypes = [dword, dword, dword]
    k32.ContinueDebugEvent.restype = boolean
    k32.GetThreadContext.argtypes = [handle, ctypes.c_void_p]
    k32.GetThreadContext.restype = boolean
    k32.ReadProcessMemory.argtypes = [
        handle, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t,
        ctypes.POINTER(ctypes.c_size_t),
    ]
    k32.ReadProcessMemory.restype = boolean
    return k32


def find_target_module(k32, pid: int) -> tuple[int, int]:
    snapshot = k32.CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)
    if snapshot == INVALID_HANDLE_VALUE:
        raise _win_error()
    try:
        entry = MODULEENTRY32W()
        entry.dwSize = ctypes.sizeof(entry)
        if not k32.Module32FirstW(snapshot, ctypes.byref(entry)):
            raise _win_error()
        expected_path = _canonical(EXPECTED_EXE)
        found = None
        while True:
            if _canonical(entry.szExePath) == expected_path:
                found = (int(entry.modBaseAddr or 0), int(entry.modBaseSize), Path(entry.szExePath))
                break
            if not k32.Module32NextW(snapshot, ctypes.byref(entry)):
                break
        if found is None:
            raise ValueError("PID does not contain the expected retail executable")
        base, size, path = found
        if not base or not size or _hash_file(path) != CLIENT_SHA256:
            raise ValueError("retail executable hash or loaded module metadata did not match")
        return base, size
    finally:
        k32.CloseHandle(snapshot)


def _open_aligned_context():
    raw = ctypes.create_string_buffer(CONTEXT_SIZE + 15)
    address = (ctypes.addressof(raw) + 15) & ~15
    ctypes.c_uint32.from_address(address + CONTEXT_FLAGS_OFFSET).value = (
        CONTEXT_AMD64 | CONTEXT_CONTROL | CONTEXT_INTEGER
    )
    return raw, address


def _get_context(k32, tid: int):
    thread = k32.OpenThread(THREAD_GET_CONTEXT, False, tid)
    if not thread:
        return None
    try:
        raw, address = _open_aligned_context()
        if not k32.GetThreadContext(thread, ctypes.c_void_p(address)):
            return None
        return parse_context_buffer((ctypes.c_ubyte * CONTEXT_SIZE).from_address(address))
    finally:
        k32.CloseHandle(thread)


def _read_remote(k32, process, address: int, size: int) -> bytes:
    if not 0 <= size <= MAX_READ or not address:
        raise ValueError("remote read exceeds the bounded observer policy")
    buffer = ctypes.create_string_buffer(size)
    read = ctypes.c_size_t()
    if not k32.ReadProcessMemory(process, ctypes.c_void_p(address), buffer, size, ctypes.byref(read)):
        return b""
    return buffer.raw[: min(read.value, size)]


def _image_excerpt_range(rip: int, base: int, size: int) -> tuple[int, int] | None:
    if module_rva(rip, base, size) is None:
        return None
    start = max(base, rip - MAX_READ // 2)
    end = min(base + size, start + MAX_READ)
    return start, end - start


def _capture_exception(k32, process, event: DEBUG_EVENT, base: int, size: int) -> dict:
    exception = event.u.Exception
    code = int(exception.ExceptionRecord.ExceptionCode)
    exception_address = int(exception.ExceptionRecord.ExceptionAddress or 0)
    registers = _get_context(k32, event.dwThreadId)
    rip, rsp = registers if registers is not None else (0, 0)
    stack = _read_remote(k32, process, rsp, MAX_READ) if rsp else b""
    excerpt = _image_excerpt_range(rip, base, size)
    image_bytes = _read_remote(k32, process, *excerpt) if excerpt else b""
    return {
        "code": f"0x{code:08X}",
        "first_chance": bool(exception.dwFirstChance),
        "exception_rva": module_rva(exception_address, base, size),
        "rip_rva": module_rva(rip, base, size),
        "image_start_rva": excerpt[0] - base if excerpt else None,
        "image_bytes_hex": image_bytes.hex() if image_bytes else "",
        "stack_return_rvas": filtered_return_rvas(stack, base, size),
    }


def _close_event_handles(k32, event: DEBUG_EVENT):
    handles = []
    if event.dwDebugEventCode == CREATE_PROCESS_DEBUG_EVENT:
        info = event.u.CreateProcessInfo
        handles.extend((info.hFile, info.hThread, info.hProcess))
    elif event.dwDebugEventCode == CREATE_THREAD_DEBUG_EVENT:
        handles.append(event.u.CreateThread.hThread)
    elif event.dwDebugEventCode == LOAD_DLL_DEBUG_EVENT:
        handles.append(event.u.LoadDll.hFile)
    for handle in handles:
        if handle:
            k32.CloseHandle(handle)


def observe(pid: int, timeout: int) -> dict:
    validate_pid(pid)
    if not 1 <= timeout <= 600:
        raise ValueError("timeout must be in 1..600 seconds")
    k32 = _kernel32()
    base, size = find_target_module(k32, pid)
    process = k32.OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if not process:
        raise _win_error()
    attached = False
    exceptions = []
    dropped = 0
    initial_breakpoint_seen = False
    exited = False
    started = time.monotonic()
    try:
        if not k32.DebugActiveProcess(pid):
            raise _win_error()
        attached = True
        if not k32.DebugSetProcessKillOnExit(False):
            k32.DebugActiveProcessStop(pid)
            attached = False
            raise _win_error()
        deadline = started + timeout
        event = DEBUG_EVENT()
        while time.monotonic() < deadline:
            remaining_ms = max(1, min(250, int((deadline - time.monotonic()) * 1000)))
            if not k32.WaitForDebugEvent(ctypes.byref(event), remaining_ms):
                error = ctypes.get_last_error()
                if error in (0, 121, 258):  # no event before timeout
                    continue
                raise ctypes.WinError(error)
            code = int(event.dwDebugEventCode)
            tid = int(event.dwThreadId)
            continue_status = DBG_CONTINUE
            try:
                if code == EXCEPTION_DEBUG_EVENT:
                    record = event.u.Exception.ExceptionRecord
                    exception_code = int(record.ExceptionCode)
                    first_chance = bool(event.u.Exception.dwFirstChance)
                    if exception_code == EXCEPTION_BREAKPOINT and first_chance and not initial_breakpoint_seen:
                        initial_breakpoint_seen = True
                        continue_status = DBG_CONTINUE
                    else:
                        continue_status = DBG_EXCEPTION_NOT_HANDLED
                    if len(exceptions) < MAX_EXCEPTIONS:
                        captured = _capture_exception(k32, process, event, base, size)
                        captured["thread_id"] = tid
                        captured["elapsed_seconds"] = round(time.monotonic() - started, 3)
                        exceptions.append(captured)
                    else:
                        dropped += 1
                elif code == EXIT_PROCESS_DEBUG_EVENT:
                    exited = True
            finally:
                _close_event_handles(k32, event)
                if not k32.ContinueDebugEvent(event.dwProcessId, tid, continue_status):
                    raise _win_error()
            if exited:
                break
    finally:
        if attached:
            k32.DebugActiveProcessStop(pid)
        k32.CloseHandle(process)
    return {
        "status": "process-exited" if exited else "timeout",
        "pid": pid,
        "module_base": base,
        "module_size": size,
        "elapsed_seconds": round(time.monotonic() - started, 3),
        "exceptions": exceptions,
        "dropped_exceptions": dropped,
    }


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pid", type=int)
    parser.add_argument("output", type=Path)
    parser.add_argument("--timeout", type=int, default=240)
    args = parser.parse_args(argv)
    try:
        validate_pid(args.pid)
        if not 1 <= args.timeout <= 600:
            raise ValueError("timeout must be in 1..600 seconds")
        with args.output.open("x", encoding="utf-8") as output:
            result = observe(args.pid, args.timeout)
            json.dump(result, output, indent=2)
            output.write("\n")
        return 0
    except (OSError, ValueError) as exc:
        parser.exit(2, f"debug_assert: {exc}\n")


if __name__ == "__main__":
    raise SystemExit(main())
