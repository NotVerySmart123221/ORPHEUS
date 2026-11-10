// tracer.asi: logs what the client does with the lobby's bytes. Research only, no game change.
// - Every server message: the JamMessage dispatchers are registered as 48-byte delegate objects (vtable
//   0x7FF6D38368D8, +24 = the dispatcher, slot 2 invokes it). One copy of that vtable catches them all;
//   the log has the message id (u16 at +28 of the message header) and the dispatcher's result.
// - Every statescript state begin / end: the script system (vtable 0x7FF6D384C010) is told by the state
//   activation (0x7FF6D2D90790 -> slot 32, refresh slot 33) and deactivation (0x7FF6D2D90480 -> slot 35)
//   with (system, instance, state item). Instance: graph GUID +0x1E8, id u16 +24. Item: node index +112,
//   flags +64 (bit 0 active), class vtable at +0.
// - The bits each state reads: a state's data is read by slot 43 of its own class vtable (0x7FF6D2D92060 ->
//   item vtable +344) with (item, bit reader, flag, 1); the reader keeps its data at +0 and its bit position
//   at +28. In the begin hook the item gets a copy of its class vtable with slot 43 replaced; that copy logs
//   the absolute bit range read and the bits (LSB first, as the reader consumes them). Dumped for a state
//   right after it begins, and for the first few reads of each class, not for every per-frame re-read.
// Vtable pointers are swapped in private read-write memory (the image cannot be made writable); new
// objects are picked up by a rescan every few seconds. Waits for <plugin>.go (tools/lobby/run.py) first.
// Log: <plugin>.log, written by its own thread from a ring buffer. Build: build.bat. research/asi-tracer.md.
#include <windows.h>
#include <stdio.h>

// RVAs in Overwatch.exe 1.74.0.0.104319 (image base 0x7FF6D1220000 in the IDA dump)
#define DELEGATE_VTABLE_RVA 0x26168D8
#define INVOKE_RVA 0x0E3C210  // 0x7FF6D205C210: delegate slot 2
#define DELEGATE_SLOTS 4
#define INVOKE_SLOT 2
#define SYSTEM_VTABLE_RVA 0x262C010
#define BEGIN_RVA 0x097A1E0  // 0x7FF6D1B9A1E0: system slot 32
#define REFRESH_RVA 0x097A510  // 0x7FF6D1B9A510: slot 33 (not hooked: once per frame per active state)
#define END_RVA 0x097A310  // 0x7FF6D1B9A310: slot 35
#define SYSTEM_SLOTS 80
#define BEGIN_SLOT 32
#define END_SLOT 35
#define STATE_SLOTS 256  // the copy runs to the end of the mapped region at most; a class with more slots would crash
#define READ_SLOT 43
#define MAX_CLASSES 1024
#define FIRST_READS 2  // per state (graph, node), dumped even when not right after a begin
#define SEEN 8192  // open-addressing set of (graph, node) keys
#define PENDING 64
#define MAX_BITS 512
#define RESCAN_MS 1000
#define MAX_REGION (64u << 20)  // larger private regions are asset caches, not object heaps
#define LINE 768
#define RING 8192

typedef INT64 (__fastcall *invoke_fn)(INT64, INT64, INT64, char);
typedef INT64 (__fastcall *state_fn)(INT64, INT64, INT64);
typedef INT64 (__fastcall *read_fn)(INT64, INT64, INT64, INT64);
typedef INT64 (__fastcall *header_fn)(INT64);

static BYTE *base;
static SIZE_T image_size;
static invoke_fn invoke_orig;
static state_fn begin_orig, end_orig;
static UINT64 *delegate_copy, *system_copy;

static struct cls {
    UINT64 *vtable, *copy;
} classes[MAX_CLASSES];
static struct seen {
    UINT64 key;
    LONG reads;
} seen[SEEN];
static int class_count;
static CRITICAL_SECTION classes_lock;
static volatile INT64 pending[PENDING];  // items that just began: their next data read is dumped
static volatile LONG pending_head;

// ---- log ring: producers are game threads, one writer thread drains in order ----
static char ring[RING][LINE];
static volatile LONG ready[RING];
static volatile LONG head, dropped;
static HANDLE logfile;

static void note(const char *fmt, ...) {
    LONG n = InterlockedIncrement(&head) - 1;
    char *line = ring[n % RING];
    if (ready[n % RING]) { InterlockedIncrement(&dropped); return; }
    int len = snprintf(line, LINE, "%u %u ", GetTickCount(), GetCurrentThreadId());
    va_list args;
    va_start(args, fmt);
    len += vsnprintf(line + len, LINE - len - 1, fmt, args);
    va_end(args);
    if (len > LINE - 2) len = LINE - 2;
    line[len] = '\n';
    line[len + 1] = 0;
    InterlockedExchange(&ready[n % RING], 1);
}

static DWORD WINAPI writer(LPVOID unused) {
    LONG tail = 0, last_dropped = 0;
    for (;;) {
        while (ready[tail % RING]) {
            char *line = ring[tail % RING];
            DWORD written;
            WriteFile(logfile, line, (DWORD)strlen(line), &written, NULL);
            InterlockedExchange(&ready[tail % RING], 0);
            tail++;
        }
        if (dropped != last_dropped) {
            char text[64];
            last_dropped = dropped;
            int len = snprintf(text, sizeof text, "dropped %d lines (ring full)\n", last_dropped);
            DWORD written;
            WriteFile(logfile, text, len, &written, NULL);
        }
        Sleep(100);
    }
}

// ---- helpers ----
static unsigned rva(const void *p) {
    return (BYTE *)p >= base && (BYTE *)p < base + image_size ? (unsigned)((BYTE *)p - base) : 0;
}

static unsigned message_id(INT64 msg) {
    __try {
        INT64 header = ((header_fn)(*(UINT64 **)msg)[2])(msg);
        return *(USHORT *)(header + 28);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return 0xFFFF;
    }
}

// Point every private read-write qword equal to a `from` at its `to`, in one pass; counts per entry. `found` gets
// each address. Every `from` is an image address, so most qwords fail the range test before the table loop.
struct swap { UINT64 from, to; void (*found)(INT64); int count; };

static void swap_pointers(struct swap *table, int n) {
    BYTE *at = (BYTE *)0x10000;
    MEMORY_BASIC_INFORMATION info;
    while (VirtualQuery(at, &info, sizeof info) == sizeof info) {
        BYTE *end = (BYTE *)info.BaseAddress + info.RegionSize;
        int own_stack = (BYTE *)&info >= (BYTE *)info.BaseAddress && (BYTE *)&info < end;  // holds `from` too
        if (!own_stack && info.RegionSize <= MAX_REGION && info.State == MEM_COMMIT && info.Type == MEM_PRIVATE &&
            info.Protect == PAGE_READWRITE) {
            __try {
                for (UINT64 *q = (UINT64 *)info.BaseAddress; (BYTE *)(q + 1) <= end; q++) {
                    if (*q - (UINT64)base >= image_size) continue;
                    for (int i = 0; i < n; i++)
                        if (*q == table[i].from) {
                            *q = table[i].to;
                            table[i].count++;
                            if (table[i].found) table[i].found((INT64)q);
                            break;
                        }
                }
            } __except (EXCEPTION_EXECUTE_HANDLER) {}
        }
        if (end <= at) break;
        at = end;
    }
}

// A read-only copy of `slots` qwords of a vtable with one slot replaced; clamps to mapped memory.
static UINT64 *copy_vtable(UINT64 *vtable, int slots, int slot, void *hook) {
    MEMORY_BASIC_INFORMATION info;
    if (VirtualQuery(vtable, &info, sizeof info) != sizeof info) return NULL;
    SIZE_T room = ((BYTE *)info.BaseAddress + info.RegionSize - (BYTE *)vtable) / sizeof *vtable;
    if ((SIZE_T)slots > room) slots = (int)room;
    if (slot >= slots) return NULL;
    UINT64 *copy = VirtualAlloc(NULL, slots * sizeof *copy, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if (!copy) return NULL;
    memcpy(copy, vtable, slots * sizeof *copy);
    copy[slot] = (UINT64)hook;
    DWORD old;
    VirtualProtect(copy, slots * sizeof *copy, PAGE_READONLY, &old);
    return copy;
}

// ---- state data reads (slot 43 of every state class the begin hook has seen) ----
static struct cls *class_of_copy(UINT64 *copy) {
    for (int i = 0; i < class_count; i++)
        if (classes[i].copy == copy) return &classes[i];
    return NULL;
}

static INT64 __fastcall read_hook(INT64 item, INT64 reader, INT64 a3, INT64 a4) {
    struct cls *c = class_of_copy(*(UINT64 **)item);
    if (!c) return 0;  // unreachable: only copies point here
    INT64 inst = *(INT64 *)(item + 120);
    UINT64 key = (*(UINT64 *)(inst + 0x1E8) << 16) ^ (UINT64)*(int *)(item + 112) ^ 1;  // never 0
    int dump = 0;
    for (UINT64 h = (key * 0x9E3779B97F4A7C15ULL) >> 51, i = 0; i < SEEN; i++, h = (h + 1) % SEEN) {
        if (!seen[h].key) InterlockedCompareExchange64((volatile INT64 *)&seen[h].key, key, 0);  // claim an empty slot
        if (seen[h].key != key) continue;
        dump = InterlockedIncrement(&seen[h].reads) <= FIRST_READS;
        break;
    }
    for (int i = 0; i < PENDING; i++)
        if (pending[i] == item && InterlockedCompareExchange64(&pending[i], 0, item) == item) dump = 1;
    DWORD before = *(DWORD *)(reader + 28);
    INT64 result = ((read_fn)c->vtable[READ_SLOT])(item, reader, a3, a4);
    if (!dump) return result;
    DWORD after = *(DWORD *)(reader + 28), n = after - before;
    char bits[MAX_BITS + 4];
    __try {
        const UINT64 *data = *(const UINT64 **)reader;
        DWORD i;
        for (i = 0; i < n && i < MAX_BITS; i++) {
            DWORD at = before + i;
            bits[i] = (char)('0' + ((data[at >> 6] >> (at & 63)) & 1));
        }
        if (n > MAX_BITS) bits[i++] = '.', bits[i++] = '.';
        bits[i] = 0;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        lstrcpyA(bits, "?");
    }
    note("data graph=%llX inst=%u node=%d cls=img+%X at=%u bits=%u %s flag=%d", *(UINT64 *)(inst + 0x1E8),
         *(USHORT *)(inst + 24), *(int *)(item + 112), rva(c->vtable), before, n, bits, (int)(a3 & 0xFF));
    return result;
}

// Give the item a copy of its class vtable whose slot 43 logs the bits read.
static void shadow(INT64 item) {
    UINT64 *vtable = *(UINT64 **)item;
    EnterCriticalSection(&classes_lock);
    struct cls *c = NULL;
    for (int i = 0; i < class_count && !c; i++)
        if (classes[i].vtable == vtable || classes[i].copy == vtable) c = &classes[i];
    if (!c && class_count < MAX_CLASSES && rva(vtable)) {
        UINT64 *copy = copy_vtable(vtable, STATE_SLOTS, READ_SLOT, read_hook);
        if (copy) {
            c = &classes[class_count++];
            c->vtable = vtable;
            c->copy = copy;
            note("class img+%X shadowed (%d classes)", rva(vtable), class_count);
        }
    }
    LeaveCriticalSection(&classes_lock);
    if (c && vtable != c->copy) *(UINT64 **)item = c->copy;
}

// A script system found by the rescan: shadow the state items its instances already have (states that began
// before the redirect would otherwise never be dumped). System: instances at +8, count +16; instance: items
// at +0xA0, count +0xA8.
static void shadow_system(INT64 sys) {
    __try {
        int count = *(int *)(sys + 16);
        INT64 *instances = *(INT64 **)(sys + 8);
        if (count <= 0 || count > 65536 || !instances) return;
        for (int i = 0; i < count; i++) {
            INT64 inst = instances[i];
            if (!inst) continue;
            int states = *(int *)(inst + 0xA8);
            INT64 *items = *(INT64 **)(inst + 0xA0);
            if (states <= 0 || states > 4096 || !items) continue;
            for (int j = 0; j < states; j++)
                if (items[j]) shadow(items[j]);
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
}

// ---- beam config vars: what each STU_9932D728 field evaluates to ----
// The beam state (vtable 0x7FF6D384D1A0) reads its descriptor (the graph's node array: instance +504 -> +96/+104,
// as sub_7FF6D2D91C90) and evaluates config vars through [field + 8] (an evaluator) slot 4 (+0x20) with
// (evaluator, context, item, field, out); out is a 16-byte value, kind u32 at +12 (10 entity id, 12 array).
// When a beam begins, each evaluator its descriptor points at gets a vtable copy whose slot 4 logs the result.
#define BEAM_VTABLE_RVA 0x262D1A0
#define EVAL_SLOT 4
#define EVAL_SLOTS 16
#define MAX_EVALS 256
#define EVAL_LOGS 6  // per evaluator object
typedef char (__fastcall *eval_fn)(INT64, INT64, INT64, INT64, INT64);
static struct eval {
    INT64 object;
    UINT64 *vtable, *copy;
    int offset;
    LONG logs;
} evals[MAX_EVALS];
static LONG eval_count;

static char __fastcall eval_hook(INT64 ev, INT64 ctx, INT64 item, INT64 field, INT64 out) {
    struct eval *e = NULL;
    for (LONG i = 0; i < eval_count && !e; i++)
        if (evals[i].object == ev) e = &evals[i];
    if (!e) return 0;  // unreachable: only hooked objects point here
    char ok = ((eval_fn)e->vtable[EVAL_SLOT])(ev, ctx, item, field, out);
    if (InterlockedIncrement(&e->logs) > EVAL_LOGS) return ok;
    __try {
        UINT64 lo = *(UINT64 *)out, hi = *(UINT64 *)(out + 8);
        unsigned kind = *(unsigned *)(out + 12), first = 0, first_kind = 0;
        if (kind == 12 && *(int *)(lo + 8) > 0) {
            INT64 elem = *(INT64 *)lo + 16;
            first = *(unsigned *)elem, first_kind = *(unsigned *)(elem + 12);
        }
        note("eval beam=%llX node=%d off=%X ev=img+%X ok=%d kind=%u value=%016llX %016llX first=%X/%u", item,
             *(int *)(item + 112), e->offset, rva(e->vtable), ok, kind, lo, hi, first, first_kind);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        note("eval beam=%llX off=%X ok=%d (unreadable out)", item, e->offset, ok);
    }
    return ok;
}

static void watch_beam(INT64 item) {
    __try {
        INT64 graph = *(INT64 *)(*(INT64 *)(item + 120) + 504);
        INT64 *nodes = *(INT64 **)(graph + 96);
        INT64 desc = *(INT64 *)((*(INT64 *)(graph + 104) == 1 ? nodes[0] : nodes[1]) + 8 * *(int *)(item + 112));
        for (int off = 0x40; off < 0x1C0; off += 8) {
            INT64 field = *(INT64 *)(desc + off);
            MEMORY_BASIC_INFORMATION info;
            if (field < 0x10000 || VirtualQuery((void *)field, &info, sizeof info) != sizeof info ||
                info.Type != MEM_PRIVATE || info.State != MEM_COMMIT)
                continue;
            INT64 ev = *(INT64 *)(field + 8);
            if (ev < 0x10000 || VirtualQuery((void *)ev, &info, sizeof info) != sizeof info ||
                info.Protect != PAGE_READWRITE)
                continue;
            UINT64 *vtable = *(UINT64 **)ev;
            if (!rva(vtable) || !rva((void *)vtable[EVAL_SLOT])) continue;
            LONG i;
            for (i = 0; i < eval_count && evals[i].object != ev; i++) {}
            if (i < eval_count) { evals[i].logs = 0; continue; }  // seen: log it again for this beam
            if (eval_count >= MAX_EVALS) return;
            UINT64 *copy = copy_vtable(vtable, EVAL_SLOTS, EVAL_SLOT, eval_hook);
            if (!copy) continue;
            evals[i].object = ev, evals[i].vtable = vtable, evals[i].copy = copy, evals[i].offset = off;
            InterlockedIncrement(&eval_count);
            *(UINT64 **)ev = copy;
            note("beam %llX node=%d desc=%llX off=%X field=%llX ev=%llX cls=img+%X", item, *(int *)(item + 112),
                 desc, off, field, ev, rva(vtable));
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        note("beam %llX: descriptor unreadable", item);
    }
}

// ---- config var evaluators by class (research/lizrollin-runtime.md section 16, the kill feed) ----
// Every evaluator object of these classes (its vtable RVA; the class's delegate factory builds it) is pointed at a
// copy of its vtable whose slot 4 logs each new (class, graph, node, result) once a second: the kill feed message's
// target, the natives of 0DEA's gate, and two natives only its line path reads. `item` is the state the evaluation runs for, when it is one.
static const struct { unsigned vtable; const char *name; } class_evals[] = {
    {0x2662760, "A3DEF368(game mode entity)"}, {0x26619F0, "CD7F3617(entity name)"}, {0x2663700, "A77AC2AC"},
    {0x2627350, "0D179CAF"}, {0x2628DA8, "33C5B1DE"}, {0x26274A0, "CEFF290A"},
    {0x2662E40, "F76C486C(0DEA line)"}, {0x2663C78, "AECBBA41(0DEA line)"},  // 0DEA nodes 50 / 56: only after its gate
    {0x26284A8, "016D65A8(feed panel)"}, {0x27ADB28, "F4C77F4A(ruleset 2544)"},  // 0DEA #1962: the panel's condition
};
#define CLASS_EVALS ((int)(sizeof class_evals / sizeof *class_evals))
#define EVAL_SEEN 4096
static UINT64 *class_copy[CLASS_EVALS];
static volatile INT64 eval_seen[EVAL_SEEN];

static char __fastcall class_eval_hook(INT64 ev, INT64 ctx, INT64 item, INT64 field, INT64 out) {
    UINT64 *copy = *(UINT64 **)ev;
    int k = 0;
    while (k < CLASS_EVALS && class_copy[k] != copy) k++;
    if (k == CLASS_EVALS) return 0;  // unreachable: only redirected objects point here
    char ok = ((eval_fn)((UINT64 *)(base + class_evals[k].vtable))[EVAL_SLOT])(ev, ctx, item, field, out);
    __try {
        UINT64 value = *(UINT64 *)out, graph = 0;
        unsigned kind = *(unsigned *)(out + 12);
        int node = -1;
        if (item > 0x10000) {
            INT64 inst = *(INT64 *)(item + 120);
            node = *(int *)(item + 112);
            if (inst > 0x10000) graph = *(UINT64 *)(inst + 0x1E8);
        }
        UINT64 second = GetTickCount() / 1000;
        INT64 key = (INT64)((((((graph * 31 + (UINT64)node) * 31 + value) * 31 + kind) * 31 + k) * 2 + ok) * 31 + second) | 1;
        for (UINT64 h = ((UINT64)key * 0x9E3779B97F4A7C15ULL) >> 52, i = 0; i < EVAL_SEEN; i++, h = (h + 1) % EVAL_SEEN) {
            if (eval_seen[h] == key) return ok;  // logged before
            if (!eval_seen[h] && InterlockedCompareExchange64(&eval_seen[h], key, 0) == 0) break;
        }
        note("classeval %s graph=%llX node=%d ok=%d kind=%u value=%016llX", class_evals[k].name, graph, node, ok,
             kind, value);
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
    return ok;
}

// ---- client system 9: its +700 decides whether 0DEA's kill feed panel shows (native 016D65A8) ----
// 016D65A8 (0x7FF6D1B55C50) = !valid || [sys9+700]; valid (0x7FF6D1993E30) = game state 4 && [sys9+224] (16 bytes)
// set && [sys9+753]. Systems: table = (0x4E3494C633194824 - [KEY]) ^ [[GAME] + 320], system n = [table + 8n].
#define GAME_RVA 0x2F586E0  // qword_7FF6D41786E0
#define KEY_RVA 0x2EE4BD0  // qword_7FF6D4104BD0
static char system9_last[320];

static void dump_system9(void) {
    char line[320];
    __try {
        INT64 game = *(INT64 *)(base + GAME_RVA);
        if (!game) return;
        UINT64 table = (0x4E3494C633194824ULL - *(UINT64 *)(base + KEY_RVA)) ^ *(UINT64 *)(game + 320);
        INT64 sys = *(INT64 *)(table + 9 * 8);
        if (!sys) lstrcpyA(line, "none");
        else
        {
            int n = _snprintf_s(line, sizeof line, _TRUNCATE, "%llX vtable=img+%X +168=%u +216=%u +224=%016llX%016llX +2B0:",
                                sys, rva(*(void **)sys), *(BYTE *)(sys + 168), *(BYTE *)(sys + 216),
                                *(UINT64 *)(sys + 232), *(UINT64 *)(sys + 224));
            for (int i = 0x2B0; i < 0x2F8 && n > 0 && n < (int)sizeof line - 3; i++)
                n += _snprintf_s(line + n, sizeof line - n, _TRUNCATE, "%02X", *(BYTE *)(sys + i));
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        lstrcpyA(line, "unreadable");
    }
    if (lstrcmpA(line, system9_last)) {
        lstrcpyA(system9_last, line);
        note("system 9: %s", line);
    }
}

// ---- state begin / end (system slots 32 / 35) ----
static void describe(const char *what, INT64 inst, INT64 item) {
    UINT64 *vtable = *(UINT64 **)item;
    struct cls *c = class_of_copy(vtable);
    note("%s graph=%llX inst=%u node=%d cls=img+%X flags=%X item=%llX", what, *(UINT64 *)(inst + 0x1E8),
         *(USHORT *)(inst + 24), *(int *)(item + 112), rva(c ? c->vtable : vtable), *(DWORD *)(item + 64), item);
}

static INT64 __fastcall begin_hook(INT64 sys, INT64 inst, INT64 item) {
    describe("begin", inst, item);
    struct cls *c = class_of_copy(*(UINT64 **)item);
    if (rva(c ? c->vtable : *(UINT64 **)item) == BEAM_VTABLE_RVA) watch_beam(item);
    shadow(item);
    pending[(InterlockedIncrement(&pending_head) - 1) % PENDING] = item;
    return begin_orig(sys, inst, item);
}

static INT64 __fastcall end_hook(INT64 sys, INT64 inst, INT64 item) {
    describe("end", inst, item);
    return end_orig(sys, inst, item);
}

// ---- server messages (delegate slot 2) ----
static INT64 __fastcall invoke_hook(INT64 delegate, INT64 a2, INT64 msg, char a4) {
    unsigned id = message_id(msg);
    INT64 result = invoke_orig(delegate, a2, msg, a4);
    note("msg %u handler=img+%X result=%d", id, rva((void *)*(UINT64 *)(delegate + 24)), (int)result);
    return result;
}

// ---- setup ----
static int slot_is(UINT64 *vtable, int slot, unsigned expected) {
    for (int t = 0; t < 1200; t++) {  // the packed image fills in after start
        if (vtable[slot] == (UINT64)(base + expected)) return 1;
        Sleep(100);
    }
    return 0;
}

static DWORD WINAPI run(LPVOID unused) {
    char path[MAX_PATH];
    HMODULE self;
    GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                       (LPCSTR)run, &self);
    DWORD n = GetModuleFileNameA(self, path, MAX_PATH);
    if (n < 4) return 0;
    lstrcpyA(path + n - 4, ".log");
    logfile = CreateFileA(path, FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_ALWAYS,
                          FILE_ATTRIBUTE_NORMAL, NULL);
    if (logfile == INVALID_HANDLE_VALUE) return 0;
    InitializeCriticalSection(&classes_lock);
    HANDLE thread = CreateThread(NULL, 0, writer, NULL, 0, NULL);
    if (thread) CloseHandle(thread);

    base = (BYTE *)GetModuleHandleW(NULL);
    IMAGE_NT_HEADERS *nt = (IMAGE_NT_HEADERS *)(base + ((IMAGE_DOS_HEADER *)base)->e_lfanew);
    image_size = nt->OptionalHeader.SizeOfImage;
    note("tracer: pid %u base %p", GetCurrentProcessId(), base);

    UINT64 *delegate_vtable = (UINT64 *)(base + DELEGATE_VTABLE_RVA);
    UINT64 *system_vtable = (UINT64 *)(base + SYSTEM_VTABLE_RVA);
    if (!slot_is(delegate_vtable, INVOKE_SLOT, INVOKE_RVA) || !slot_is(system_vtable, BEGIN_SLOT, BEGIN_RVA) ||
        system_vtable[END_SLOT] != (UINT64)(base + END_RVA)) {
        note("vtable slots do not match this build: nothing hooked");
        return 0;
    }
    invoke_orig = (invoke_fn)delegate_vtable[INVOKE_SLOT];
    begin_orig = (state_fn)system_vtable[BEGIN_SLOT];
    end_orig = (state_fn)system_vtable[END_SLOT];
    delegate_copy = copy_vtable(delegate_vtable, DELEGATE_SLOTS, INVOKE_SLOT, invoke_hook);
    system_copy = copy_vtable(system_vtable, SYSTEM_SLOTS, BEGIN_SLOT, begin_hook);
    if (!delegate_copy || !system_copy) { note("no memory for the vtable copies"); return 0; }
    DWORD old;
    VirtualProtect(system_copy, SYSTEM_SLOTS * sizeof *system_copy, PAGE_READWRITE, &old);
    system_copy[END_SLOT] = (UINT64)end_hook;
    VirtualProtect(system_copy, SYSTEM_SLOTS * sizeof *system_copy, PAGE_READONLY, &old);

    lstrcpyA(path + n - 4, ".go");
    note("waiting for %s", path);
    while (GetFileAttributesA(path) == INVALID_FILE_ATTRIBUTES) Sleep(500);
    DeleteFileA(path);
    struct swap table[2 + CLASS_EVALS] = {{(UINT64)delegate_vtable, (UINT64)delegate_copy, NULL},
                                          {(UINT64)system_vtable, (UINT64)system_copy, shadow_system}};
    int swaps = 2;
    for (int k = 0; k < CLASS_EVALS; k++) {
        UINT64 *vtable = (UINT64 *)(base + class_evals[k].vtable);
        if (!rva((void *)vtable[EVAL_SLOT])) { note("evaluator %s: slot 4 outside the image", class_evals[k].name); continue; }
        class_copy[k] = copy_vtable(vtable, EVAL_SLOTS, EVAL_SLOT, class_eval_hook);
        if (class_copy[k]) table[swaps++] = (struct swap){(UINT64)vtable, (UINT64)class_copy[k], NULL, 0};
    }
    for (;;) {
        DWORD t0 = GetTickCount();
        int e = 0;
        for (int i = 0; i < swaps; i++) table[i].count = 0;
        swap_pointers(table, swaps);
        for (int i = 2; i < swaps; i++) e += table[i].count;
        if (table[0].count || table[1].count || e)
            note("redirected %d delegate(s), %d script system(s), %d evaluator(s) in %u ms", table[0].count,
                 table[1].count, e, GetTickCount() - t0);
        dump_system9();
        Sleep(RESCAN_MS);
    }
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
