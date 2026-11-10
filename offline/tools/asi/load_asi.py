"""Load ASI plugins (DLLs) into a running Overwatch process: LoadLibraryW on a remote thread.

    python load_asi.py PID PLUGIN.asi [PLUGIN.asi ...]

The game restricts its DLL search to system32, so a proxy version.dll in the game folder is never loaded;
the launcher (`tools/lobby/run.py --asi`) calls `load` instead. Opt-in only.
"""
import ctypes
import sys
from ctypes import wintypes
from pathlib import Path

PROCESS_ACCESS = 0x0002 | 0x0008 | 0x0010 | 0x0020 | 0x0400  # create thread, vm op/read/write, query info
MEM_COMMIT_RESERVE, MEM_RELEASE, PAGE_READWRITE, INFINITE = 0x3000, 0x8000, 0x04, 0xFFFFFFFF

kernel32 = ctypes.WinDLL('kernel32', use_last_error=True)
kernel32.OpenProcess.restype = wintypes.HANDLE
kernel32.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
kernel32.VirtualAllocEx.restype = ctypes.c_void_p
kernel32.VirtualAllocEx.argtypes = (wintypes.HANDLE, ctypes.c_void_p, ctypes.c_size_t, wintypes.DWORD, wintypes.DWORD)
kernel32.VirtualFreeEx.argtypes = (wintypes.HANDLE, ctypes.c_void_p, ctypes.c_size_t, wintypes.DWORD)
kernel32.WriteProcessMemory.argtypes = (wintypes.HANDLE, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t,
                                        ctypes.POINTER(ctypes.c_size_t))
kernel32.GetModuleHandleW.restype = wintypes.HMODULE
kernel32.GetProcAddress.restype = ctypes.c_void_p
kernel32.GetProcAddress.argtypes = (wintypes.HMODULE, ctypes.c_char_p)
kernel32.CreateRemoteThread.restype = wintypes.HANDLE
kernel32.CreateRemoteThread.argtypes = (wintypes.HANDLE, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p,
                                        ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p)
kernel32.GetExitCodeThread.argtypes = (wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD))
kernel32.CloseHandle.argtypes = (wintypes.HANDLE,)


def load(pid, plugin):
    """LoadLibraryW(plugin) inside process pid; returns the low 32 bits of the module handle (0 = failed)."""
    path = str(Path(plugin).resolve())
    if not Path(path).is_file():
        raise FileNotFoundError(path)
    data = ctypes.create_unicode_buffer(path)
    size = ctypes.sizeof(data)
    process = kernel32.OpenProcess(PROCESS_ACCESS, False, pid)
    if not process:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        remote = kernel32.VirtualAllocEx(process, None, size, MEM_COMMIT_RESERVE, PAGE_READWRITE)
        if not remote:
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            if not kernel32.WriteProcessMemory(process, remote, data, size, None):
                raise ctypes.WinError(ctypes.get_last_error())
            # kernel32 sits at the same address in every process of a boot
            start = kernel32.GetProcAddress(kernel32.GetModuleHandleW('kernel32.dll'), b'LoadLibraryW')
            thread = kernel32.CreateRemoteThread(process, None, 0, start, remote, 0, None)
            if not thread:
                raise ctypes.WinError(ctypes.get_last_error())
            kernel32.WaitForSingleObject(thread, INFINITE)
            code = wintypes.DWORD()
            kernel32.GetExitCodeThread(thread, ctypes.byref(code))
            kernel32.CloseHandle(thread)
            return code.value
        finally:
            kernel32.VirtualFreeEx(process, remote, 0, MEM_RELEASE)
    finally:
        kernel32.CloseHandle(process)


if __name__ == '__main__':
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    for name in sys.argv[2:]:
        print(name, 'loaded' if load(int(sys.argv[1]), name) else 'FAILED')
