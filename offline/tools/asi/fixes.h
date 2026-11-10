// Shared by the modules of ow_fixes.asi (the list is in ow_fixes.c). Static, so a module's self-check that includes
// its .c file builds without the rest of the plugin.
#pragma once
#include <windows.h>
#include <stdio.h>

// This plugin's own path with ".asi" replaced by `ext` (".log", ".ini", ".go"); 0 when it does not fit.
static int fix_path(char path[MAX_PATH], const char *ext) {
    HMODULE self;
    if (!GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                            (LPCSTR)fix_path, &self)) return 0;
    DWORD n = GetModuleFileNameA(self, path, MAX_PATH);
    if (n < 4 || n >= MAX_PATH || n - 4 + lstrlenA(ext) >= MAX_PATH) return 0;
    lstrcpyA(path + n - 4, ext);
    return 1;
}

// One line in <plugin>.log: "HH:MM:SS.mmm module text". A single write on an append-only handle, so the modules'
// threads never overwrite each other's lines.
static void fix_log(const char *module, const char *text) {
    char path[MAX_PATH], line[512];
    if (!fix_path(path, ".log")) return;
    SYSTEMTIME now;
    GetLocalTime(&now);
    int n = _snprintf_s(line, sizeof line - 2, _TRUNCATE, "%02d:%02d:%02d.%03d %s %s", now.wHour, now.wMinute,
                        now.wSecond, now.wMilliseconds, module, text);
    if (n < 0) n = lstrlenA(line);  // truncated
    line[n++] = '\r';
    line[n++] = '\n';
    HANDLE file = CreateFileA(path, FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, NULL,
                              OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) return;
    DWORD written;
    WriteFile(file, line, n, &written, NULL);
    CloseHandle(file);
}
