// retail_menu (ow_fixes.asi): makes a few Bool config vars report fixed values so the client shows the retail menus.
// - STU_2B33EE38 returns the --tank_TournamentMode flag; the PLAY screen (graph 0x0B7C state 10) and the main
//   menu's TRAINING button read it. Forced false: retail cards, TRAINING enabled. Everything else keeps
//   reading the real flag (research/play-menu.md).
// - TRAINING's enable expression (graph 0x1214, UX var 25424) is A && !B && !C && !tournament. A is
//   STU_0CFBAF2D (a player-state check that returns false offline); B (STU_C407C4BA) and C (STU_C3D71CB5) are
//   queue checks, already false at the menu, so they are left alone.
// - The Training screen's PRACTICE VS. AI card (graph 0x0B7F, presenter 146) shows when STU_27FF1349 is true;
//   it returns a cvar (byte 0x7FF6D4052888) that is on by default and off in tournament mode.
// The plugin logs the game's own value of each when it changes.
//
// Startup graph 0194 already has a route around Pod and Name when entity var 2275 is true. Its evaluator
// starts the normal native lobby connection, then selects that route on the graph's own thread.
//
// It also lifts the tournament route's input lock (research/tournament-input-lock.md): the input setup
// (0x7FF6D2B07670) installs a WH_KEYBOARD_LL hook (0x7FF6D2B0B410) in tournament and show mode that swallows
// Alt+Tab, Alt+Esc, Ctrl+Esc, Alt+F4, Alt+Space and both Windows keys; the handle sits in a global. The plugin
// calls UnhookWindowsHookEx on it once. Shutdown's own unhook then fails harmlessly (it ignores the result).
//
// The image cannot be made writable (its section refuses protection changes), so the vtables stay as they
// are: the plugin gives each runtime instance (heap objects whose first qword is the vtable) a copy of the
// vtable with the evaluator swapped. New instances are picked up by a rescan every few seconds.
#define _CRT_RAND_S
#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include "fixes.h"

// RVAs in Overwatch.exe 1.74.0.0.104319 (IDA dump, image base 0x7FF6D1220000)
#define SET_BOOL_RVA 0x0F66AF0  // 0x7FF6D2186AF0: variant setter (variant, bool); Bool variant: +0 value, +12 type 1
#define SLOTS 24                // copied; the classes have fewer, the rest are the next vtable's and unused
#define EVALUATE_SLOT 4
#define RESCAN_MS 3000

#define LOGIN_NAME "Player"      // the lobby's 0x5014 sends the name the menu shows
#define LOGIN_SCAN_MS 10
#define COMMAND_ARGS_RVA (0x7FF6D2A86670ULL - 0x7FF6D1220000ULL)
#define ARGUMENT_RVA (0x7FF6D2A59760ULL - 0x7FF6D1220000ULL)
#define KEY_HOOK_RVA 0x3285B00   // 0x7FF6D44A5B00: HHOOK of the WH_KEYBOARD_LL hook, 0 until the input setup

typedef char (__fastcall *evaluate_fn)(INT64, INT64, INT64, INT64, INT64);
typedef void (__fastcall *set_bool_fn)(INT64, unsigned char);

static struct override {
    const char *name;
    UINT64 vtable_rva, evaluate_rva;  // runtime class vtable and its slot 4
    int value;                        // forced Bool, or -1 to only log
    UINT64 *vtable, *copy;
    evaluate_fn evaluate;
    int seen;                         // last logged game value, -1 before the first
} overrides[] = {
    {"STUConfigVarDynamic (startup route)", 0x27ACDD8, 0x1B76650, -1},
    {"STU_2B33EE38 (tournament flag)", 0x26640D8, 0x0CEF750, 0},
    {"STU_0CFBAF2D (TRAINING enabled: A)", 0x262BAD0, 0x0953570, 1},
    {"STU_27FF1349 (Training: vs-AI card shown)", 0x262B3D0, 0x094FF20, 1},
    {"STU_DEA278FD (PLAY card shown, cvar 0x7FF6D4052680)", 0x262A720, 0x0949030, 1},
    {"STU_74BA7BFC (PLAY card shown, cvar 0x7FF6D40528B0)", 0x26250E0, 0x0918120, 1},
    {"STU_ECD42997 (PLAY card shown, cvar 0x7FF6D4053790)", 0x2626B00, 0x0928A80, 1},
};
#define COUNT (sizeof overrides / sizeof *overrides)
static set_bool_fn set_bool;

typedef INT64 (__fastcall *args_fn)(void);
typedef const char *(__fastcall *arg_fn)(INT64, const char *, const char *, BYTE *);
typedef INT64 (__fastcall *login_fn)(INT64, const char *);
typedef void (__fastcall *set_var_fn)(INT64, const void *, const void *);
typedef INT64 (__fastcall *hero_record_fn)(INT64, const UINT64 *);
typedef UINT64 (__fastcall *unlock_theme_fn)(UINT64);
typedef INT64 (__fastcall *asset_fn)(UINT64);
static INT64 *app;
static BYTE *tournament;
static args_fn command_args;
static arg_fn argument;
static login_fn set_address, start_login;
static set_var_fn set_variable;
static UINT64 *table_key;
static hero_record_fn hero_record;
static unlock_theme_fn unlock_theme;
static asset_fn loaded_asset;
static UINT64 menu_hero, menu_skin;
static volatile INT64 login_instance;
static BYTE recovery_field[24];
static int login_failed;

static void note(const char *text) { fix_log("retail_menu", text); }

static UINT64 random_menu_skin(UINT64 hero) {
    if (hero == menu_hero && menu_skin) return menu_skin;
    if (!*app) return 0;
    UINT64 table = (0x4E3494C633194824ULL - *table_key) ^ *(UINT64 *)(*app + 320);
    if (!table) return 0;
    INT64 catalog = *(INT64 *)(table + 70 * 8);
    INT64 record = catalog ? hero_record(catalog + 0x58, &hero) : 0;
    if (!record || *(UINT64 *)(record + 0x30) != hero) return 0;
    UINT64 *unlocks = *(UINT64 **)record, count = *(UINT64 *)(record + 8);
    if (!unlocks || count > 65536) return 0;
    UINT64 chosen = 0, chosen_unlock = 0;
    unsigned candidates = 0;
    for (UINT64 i = 0; i < count; i++) {
        UINT64 theme = unlock_theme(unlocks[2 * i]);
        if (theme >> 52 != 0x0A5 || !loaded_asset(theme)) continue;
        unsigned draw;
        if (rand_s(&draw)) return 0;
        if (draw % ++candidates == 0) {
            chosen = theme;
            chosen_unlock = unlocks[2 * i];
        }
    }
    if (chosen) {
        char text[160];
        menu_hero = hero;
        menu_skin = chosen;
        sprintf_s(text, sizeof text, "menu: hero=%016llX owned skin=%016llX theme=%016llX (%u candidates)",
                  hero, chosen_unlock, chosen, candidates);
        note(text);
    }
    return chosen;
}

static void seed_menu_skin(INT64 self, INT64 context, INT64 item, INT64 field, INT64 result) {
    if (*(DWORD *)(result + 12) != 4 || *(UINT64 *)result >> 52 != 0x02E) return;
    BYTE input_field[24];
    DWORD input[4];
    memcpy(input_field, (void *)field, sizeof input_field);
    // Explicit Showcase inputs belong to previews and authored scenes.
    for (USHORT id = 13241; id <= 13242; id++) {
        *(USHORT *)(input_field + 16) = id;
        memset(input, 0, sizeof input);
        input[3] = 2;
        if (!overrides[0].evaluate(self, context, item, (INT64)input_field, (INT64)input) ||
            (input[3] != 255 && (input[3] != 4 || *(UINT64 *)input))) return;
    }
    UINT64 skin = random_menu_skin(*(UINT64 *)result);
    if (!skin) return;
    *(UINT64 *)input = skin;
    input[2] = 0;
    input[3] = 4;
    set_variable(*(INT64 *)(context + 152), input_field, input);
}

static char __fastcall dynamic_value(INT64 self, INT64 context, INT64 item, INT64 field, INT64 result) {
    char ok = overrides[0].evaluate(self, context, item, field, result);
    if (!item || !field || !context) return ok;
    INT64 inst = *(INT64 *)(item + 120);
    if (!inst) return ok;
    UINT64 graph = *(UINT64 *)(inst + 0x1E8);
    if (*tournament && graph == 0x0580000000000134ULL &&
        *(USHORT *)(field + 16) == 8646 && *(BYTE *)(field + 18) == 1) {
        // Enable the native 062C publisher; local29323 still controls its screen lifetime.
        set_bool(result, 1);
        return 1;
    }
    if (ok && *tournament && graph == 0x0580000000000B4AULL &&
        *(USHORT *)(field + 16) == 940 && !*(BYTE *)(field + 18)) {
        seed_menu_skin(self, context, item, field, result);
        return ok;
    }
    if (!ok || login_failed || !*tournament || graph != 0x0580000000000194ULL || !*app) return ok;
    INT64 lobby = *(INT64 *)(*app + 632);
    if (!lobby || (login_instance && login_instance != inst)) return ok;
    DWORD state = *(DWORD *)(lobby + 112);
    if (login_instance && (state == 0 || state == 2)) {
        DWORD zero[4] = {0, 0, 0, 2};  // native Int variant
        login_failed = 1;
        set_variable(*(INT64 *)(context + 152), recovery_field, zero);
        note("login: connection failed; restored the native Pod recovery route");
        return ok;
    }
    if (*(USHORT *)(field + 16) != 2275 || !*(BYTE *)(field + 18)) return ok;
    if (!login_instance) {
        BYTE found = 0;
        const char *address = argument(command_args(), "lobbyServer", "", &found);
        if (!found || !address || !*address || *(DWORD *)(lobby + 112) != 1) return ok;
        memcpy(recovery_field, (void *)field, sizeof recovery_field);
        *(USHORT *)(recovery_field + 16) = 1693;
        recovery_field[18] = 0;  // startup graph's local selector
        login_instance = inst;  // native login can re-enter graph evaluation
        set_address(lobby, address);
        start_login(lobby, LOGIN_NAME);
        note("login: native connection requested; bypassing Pod and Name");
    }
    state = *(DWORD *)(lobby + 112);
    if (state >= 3 && state <= 7) set_bool(result, 1);
    return ok;
}

// Slot 4 of every copied vtable; a1 is the config var instance, whose vtable names the override.
static char __fastcall forced(INT64 a1, INT64 a2, INT64 a3, INT64 a4, INT64 result) {
    UINT64 *vtable = *(UINT64 **)a1;
    for (int i = 0; i < COUNT; i++) {
        struct override *o = &overrides[i];
        if (vtable != o->copy) continue;
        if (i == 0) return dynamic_value(a1, a2, a3, a4, result);
        char ok = o->evaluate(a1, a2, a3, a4, result);
        int game = *(DWORD *)(result + 12) == 1 ? *(BYTE *)result : -2;
        if (game != o->seen) {
            char text[128];
            o->seen = game;
            wsprintfA(text, "%s: game value %d", o->name, game);
            note(text);
        }
        if (o->value >= 0) set_bool(result, (unsigned char)o->value);
        return ok;
    }
    return 0;  // unreachable: only the copies point here
}

// Point every private read-write qword equal to `from` at `to`; returns how many.
static int swap_pointers(UINT64 from, UINT64 to) {
    int swapped = 0;
    BYTE *at = (BYTE *)0x10000;
    MEMORY_BASIC_INFORMATION info;
    while (VirtualQuery(at, &info, sizeof info) == sizeof info) {
        BYTE *end = (BYTE *)info.BaseAddress + info.RegionSize;
        if (info.State == MEM_COMMIT && info.Type == MEM_PRIVATE && info.Protect == PAGE_READWRITE) {
            __try {
                for (UINT64 *q = (UINT64 *)info.BaseAddress; (BYTE *)(q + 1) <= end; q++)
                    if (*q == from) { *q = to; swapped++; }
            } __except (EXCEPTION_EXECUTE_HANDLER) {}  // a region freed while we read it
        }
        if (end <= at) break;
        at = end;
    }
    return swapped;
}

DWORD WINAPI retail_menu_run(LPVOID unused) {
    BYTE *base = (BYTE *)GetModuleHandleW(NULL);
    set_bool = (set_bool_fn)(base + SET_BOOL_RVA);
    app = (INT64 *)(base + 0x2F586E0);
    tournament = base + 0x2E2F448;
    command_args = (args_fn)(base + COMMAND_ARGS_RVA);
    argument = (arg_fn)(base + ARGUMENT_RVA);
    set_address = (login_fn)(base + 0x0E38610);
    start_login = (login_fn)(base + 0x0E38770);
    set_variable = (set_var_fn)(base + (0x7FF6D2DC5650ULL - 0x7FF6D1220000ULL));
    table_key = (UINT64 *)(base + (0x7FF6D4104BD0ULL - 0x7FF6D1220000ULL));
    hero_record = (hero_record_fn)(base + (0x7FF6D2F2E900ULL - 0x7FF6D1220000ULL));
    unlock_theme = (unlock_theme_fn)(base + (0x7FF6D1A14540ULL - 0x7FF6D1220000ULL));
    loaded_asset = (asset_fn)(base + (0x7FF6D295BA30ULL - 0x7FF6D1220000ULL));
    for (int i = 0; i < COUNT; i++) {
        struct override *o = &overrides[i];
        o->vtable = (UINT64 *)(base + o->vtable_rva);
        o->evaluate = (evaluate_fn)(base + o->evaluate_rva);
        o->seen = -1;
        for (int t = 0; o->vtable[EVALUATE_SLOT] != (UINT64)o->evaluate; t++) {  // the packed image fills in after start
            if (t == 1200) { note("evaluator not found in a vtable: wrong build? nothing changed"); return 0; }
            Sleep(100);
        }
        // made read-only below, so the scan (read-write regions only) skips it
        o->copy = VirtualAlloc(NULL, SLOTS * sizeof *o->copy, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        if (!o->copy) { note("no memory for a vtable copy"); return 0; }
        memcpy(o->copy, o->vtable, SLOTS * sizeof *o->copy);
        o->copy[EVALUATE_SLOT] = (UINT64)forced;
        DWORD old;
        VirtualProtect(o->copy, SLOTS * sizeof *o->copy, PAGE_READONLY, &old);
    }
    // The login screens read the tournament flag: false there takes the retail (Battle.net) route and login
    // never ends. So wait for <plugin>.go, which tools/lobby/run.py creates once the lobby has logged us in.
    char go[MAX_PATH];
    if (!fix_path(go, ".go")) return 0;
    note("waiting for the login");
    HHOOK *key_hook = (HHOOK *)(base + KEY_HOOK_RVA);
    while (GetFileAttributesA(go) == INVALID_FILE_ATTRIBUTES) {
        if (key_hook && *key_hook) {  // the input setup has run: lift the Alt+Tab / Windows key lock
            note(UnhookWindowsHookEx(*key_hook) ? "keyboard hook removed: Alt+Tab and the Windows key work"
                                                : "keyboard hook: UnhookWindowsHookEx failed");
            key_hook = NULL;
        }
        if (!login_instance) swap_pointers((UINT64)overrides[0].vtable, (UINT64)overrides[0].copy);
        Sleep(login_instance ? 500 : LOGIN_SCAN_MS);
    }
    DeleteFileA(go);
    note("logged in; scanning for the config vars");
    for (;;) {
        for (int i = 0; i < COUNT; i++) {
            int swapped = swap_pointers((UINT64)overrides[i].vtable, (UINT64)overrides[i].copy);
            if (swapped) {
                char text[128];
                wsprintfA(text, "%s: redirected %d instance(s)", overrides[i].name, swapped);
                note(text);
            }
        }
        Sleep(RESCAN_MS);
    }
}
