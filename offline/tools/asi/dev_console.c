// dev_console.asi: opens the client's built-in developer console (an ImGui window). Research only.
// The console toggle (0x7FF6D2A393E0, bound to engine key 0x60 GRAVE + Alt in 0x7FF6D2C7B900) needs cvar byte 0x7FF6D404F640
// (tank_Console; tournament mode already sets it) and, in tournament mode (byte 0x7FF6D404F448), also
// 0x7FF6D404F4B0 (tank_TournamentModeAdmin) and 0x7FF6D404F5C8 (dev gate, cleared by the tournament preset).
// This plugin keeps those three bytes at 1 and logs the console object (0x7FF6D445BC58) and its open flag (+21504).
// When the console first opens it dumps the command table to the log. Besides those three bytes it writes only the
// debug cvars named in <plugin>.set (table `cvars` below), each time that file changes.
// Log: <plugin>.log. Build: build.bat. Open the console with Alt+` (foreground key; posted keys are ignored).
#include <windows.h>
#include <stdio.h>

// RVAs in Overwatch.exe 1.74.0.0.104319 (image base 0x7FF6D1220000)
#define CONSOLE_FLAG_RVA 0x2E2F640  // tank_Console
#define ADMIN_FLAG_RVA 0x2E2F4B0    // tank_TournamentModeAdmin
#define DEV_FLAG_RVA 0x2E2F5C8      // unnamed dev gate
#define CONSOLE_RVA 0x323BC58       // console object pointer
#define OPEN_OFFSET 21504
#define COMMANDS_RVA 0x323BC60      // 256 hash buckets of command entries (registrar 0x7FF6D2A3C2D0)
#define COMMAND_COUNT_RVA 0x323C468
#define RUN_SECONDS 3600

static void note(const char *text) {
    char path[MAX_PATH];
    HMODULE self;
    GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                       (LPCSTR)note, &self);
    DWORD n = GetModuleFileNameA(self, path, MAX_PATH);
    if (n < 4) return;
    lstrcpyA(path + n - 4, ".log");
    FILE *log = fopen(path, "a");
    if (!log) return;
    SYSTEMTIME now;
    GetLocalTime(&now);
    fprintf(log, "%02d:%02d:%02d %s\n", now.wHour, now.wMinute, now.wSecond, text);
    fclose(log);
}

// Named debug cvars whose globals still have a reader (research/dev-console.md). size 4 = float or int
// (is_float), size 1 = bool byte. Offsets are metres, angles degrees.
static const struct cvar { const char *name; UINT64 rva; int size, is_float; } cvars[] = {
    {"firstPerson.DetachModelFlag", 0x2F5E418, 4, 0},
    {"firstPerson.OffsetX", 0x2F5E41C, 4, 1}, {"firstPerson.OffsetY", 0x2F5E420, 4, 1},
    {"firstPerson.OffsetZ", 0x2F5E424, 4, 1}, {"firstPerson.OffsetYaw", 0x2F5E428, 4, 1},
    {"firstPerson.OffsetPitch", 0x2F5E42C, 4, 1}, {"firstPerson.OffsetRoll", 0x2F5E430, 4, 1},
    {"camera.EnableUpdate", 0x2D032B4, 1, 0},
    {"anim.ShowBindPose", 0x3225C7C, 1, 0},
};

// Applies <plugin>.set ("name value" per line) whenever the file changes; logs each write.
static void apply_settings(BYTE *base) {
    static FILETIME seen;
    char path[MAX_PATH], line[200], name[100], text[200];
    HMODULE self;
    GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                       (LPCSTR)apply_settings, &self);
    DWORD n = GetModuleFileNameA(self, path, MAX_PATH);
    if (n < 4) return;
    lstrcpyA(path + n - 4, ".set");
    WIN32_FILE_ATTRIBUTE_DATA info;
    if (!GetFileAttributesExA(path, GetFileExInfoStandard, &info) || !CompareFileTime(&info.ftLastWriteTime, &seen))
        return;
    seen = info.ftLastWriteTime;
    FILE *file = fopen(path, "r");
    if (!file) return;
    while (fgets(line, sizeof line, file)) {
        double value;
        if (sscanf_s(line, "%99s %lf", name, (unsigned)sizeof name, &value) != 2) continue;
        int known = 0;
        for (int i = 0; i < sizeof cvars / sizeof *cvars; i++) {
            const struct cvar *c = &cvars[i];
            if (lstrcmpA(name, c->name)) continue;
            known = 1;
            BYTE *at = base + c->rva;
            if (c->size == 1) *(volatile BYTE *)at = (BYTE)value;
            else if (c->is_float) *(volatile float *)at = (float)value;
            else *(volatile INT32 *)at = (INT32)value;
            _snprintf_s(text, sizeof text, _TRUNCATE, "set %s = %g", name, value);
            note(text);
        }
        if (!known) { _snprintf_s(text, sizeof text, _TRUNCATE, "unknown cvar %s", name); note(text); }
    }
    fclose(file);
}

// Engine string at p: inline when capacity (+20) <= 16, else a pointer; length at +16.
static const char *text_of(BYTE *p) {
    return *(DWORD *)(p + 20) > 16 ? *(const char **)p : (const char *)p;
}

// Entry: +0 handler, +8 full name, +40 name after the first '.', +72 / +104 more strings, +136 hash,
// +144 context, +152 / +153 min / max args (0xFF = any), +160 next in bucket.
static void dump_commands(BYTE *base) {
    char line[600];
    BYTE **buckets = (BYTE **)(base + COMMANDS_RVA);
    wsprintfA(line, "command table: %d entries", *(int *)(base + COMMAND_COUNT_RVA));
    note(line);
    for (int b = 0; b < 256; b++)
        for (BYTE *e = buckets[b]; e; e = *(BYTE **)(e + 160)) {
            _snprintf_s(line, sizeof line, _TRUNCATE, "cmd %s | args %d..%d | handler +%llX | %s | %s",
                        text_of(e + 8), e[152], e[153], (UINT64)(*(BYTE **)e - base), text_of(e + 72),
                        text_of(e + 104));
            note(line);
        }
}

static DWORD WINAPI run(LPVOID unused) {
    BYTE *base = (BYTE *)GetModuleHandleW(NULL);
    volatile BYTE *flags[] = {base + CONSOLE_FLAG_RVA, base + ADMIN_FLAG_RVA, base + DEV_FLAG_RVA};
    UINT64 *console = (UINT64 *)(base + CONSOLE_RVA);
    UINT64 seen_console = ~0ULL;
    int seen_open = -1, seen_flags = -1, dumped = 0;
    char text[160];
    note("started");
    for (int tick = 0; tick < RUN_SECONDS * 4; tick++) {
        int now = flags[0][0] | flags[1][0] << 1 | flags[2][0] << 2;
        if (now != seen_flags) {
            wsprintfA(text, "flags before forcing: console %d admin %d dev %d", now & 1, now >> 1 & 1, now >> 2 & 1);
            note(text);
            seen_flags = now;
        }
        for (int i = 0; i < 3; i++) flags[i][0] = 1;
        UINT64 c = *console;
        if (c != seen_console) {
            wsprintfA(text, "console object %08X%08X", (DWORD)(c >> 32), (DWORD)c);
            note(text);
            seen_console = c;
        }
        int open = c ? *(volatile BYTE *)(c + OPEN_OFFSET) : -1;
        if (open != seen_open) {
            wsprintfA(text, "console open %d", open);
            note(text);
            if (open == 1 && !dumped) { dump_commands(base); dumped = 1; }  // the table is complete by then
            seen_open = open;
        }
        apply_settings(base);
        Sleep(250);
    }
    note("stopped");
    return 0;
}

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID reserved) {
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(instance);
        HANDLE thread = CreateThread(NULL, 0, run, NULL, 0, NULL);
        if (thread) {
            SetThreadPriority(thread, THREAD_PRIORITY_LOWEST);
            CloseHandle(thread);
        }
    }
    return TRUE;
}
