// guard_probe.asi: a data watchpoint without debug registers. Research only, no game change.
// Hardware breakpoints crash the 1.74 client (helix_probe.c). This probe instead puts PAGE_GUARD on the page
// of an address written to <plugin>.addr (hex, e.g. "1E698D634B0"; an optional second hex number limits the log to
// [address, address + length), e.g. "1E698D634B0 40"), catches STATUS_GUARD_PAGE_VIOLATION in a
// vectored exception handler, logs the faulting instruction and the accessed address, and re-arms the guard
// after single-stepping the instruction (trap flag). It disarms after MAX_HITS hits or RUN_SECONDS, then waits
// for the next .addr.
// The whole page faults, not just the watched qword: expect neighbours in the log. No game memory is written.
// Log: <plugin>.log. Build: build.bat. Load into a running game: python tools/asi/load_asi.py PID build/guard_probe.asi
#include <windows.h>
#include <stdio.h>

#define MAX_HITS 4000  // below RING, so nothing is overwritten before it is logged
#define RUN_SECONDS 600
#define RING 4096

static UINT64 watched, page, span;  // span 0: log the whole page
static volatile LONG armed, hits, dropped;
static DWORD self_tid;
static struct hit {
    LONG ready;
    DWORD tid;
    UINT64 rip, address, info;
} ring[RING];
static volatile LONG head;

static int arm(void) {
    DWORD old;
    return VirtualProtect((void *)page, 1, PAGE_READWRITE | PAGE_GUARD, &old) ? 1 : 0;
}

static LONG CALLBACK handler(EXCEPTION_POINTERS *info) {
    EXCEPTION_RECORD *rec = info->ExceptionRecord;
    CONTEXT *ctx = info->ContextRecord;
    if (rec->ExceptionCode == STATUS_GUARD_PAGE_VIOLATION) {
        UINT64 address = rec->NumberParameters > 1 ? rec->ExceptionInformation[1] : 0;
        if ((address & ~0xFFFULL) != page)
            return EXCEPTION_CONTINUE_SEARCH;  // a guard page of the game's own (stack growth); Windows re-arms nothing, so leave it
        if (armed && span && (address < watched || address >= watched + span)) {
            ctx->EFlags |= 0x100;  // outside the range: not logged, re-armed after this instruction
        } else if (armed) {
            LONG n = InterlockedIncrement(&head) - 1;
            struct hit *h = &ring[n % RING];
            if (h->ready) {
                InterlockedIncrement(&dropped);
            } else {
                h->tid = GetCurrentThreadId();
                h->rip = ctx->Rip;
                h->address = address;
                h->info = rec->ExceptionInformation[0];  // 0 read, 1 write
                InterlockedExchange(&h->ready, 1);
            }
            if (InterlockedIncrement(&hits) < MAX_HITS)
                ctx->EFlags |= 0x100;  // trap flag: re-arm after this instruction
            else
                InterlockedExchange(&armed, 0);
        }
        return EXCEPTION_CONTINUE_EXECUTION;  // the guard is gone until re-armed
    }
    if (rec->ExceptionCode == EXCEPTION_SINGLE_STEP && armed) {
        arm();
        return EXCEPTION_CONTINUE_EXECUTION;
    }
    return EXCEPTION_CONTINUE_SEARCH;
}

static DWORD WINAPI run(LPVOID unused) {
    char path[MAX_PATH], text[256];
    HMODULE self;
    GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                       (LPCSTR)run, &self);
    DWORD n = GetModuleFileNameA(self, path, MAX_PATH);
    if (n < 4) return 0;
    lstrcpyA(path + n - 4, ".log");
    FILE *log = fopen(path, "a");
    if (!log) return 0;
    setvbuf(log, NULL, _IONBF, 0);
    self_tid = GetCurrentThreadId();
    BYTE *base = (BYTE *)GetModuleHandleW(NULL);
    fprintf(log, "guard_probe: pid %u base %p\n", GetCurrentProcessId(), base);
    lstrcpyA(path + n - 4, ".addr");
  next:
    watched = 0;
    InterlockedExchange(&hits, 0);
    for (;;) {  // wait for the address file
        FILE *f = fopen(path, "r");
        if (f) {
            if (fgets(text, sizeof text, f)) {
                char *end;
                watched = _strtoui64(text, &end, 16);
                span = _strtoui64(end, NULL, 16);
            }
            fclose(f);
            if (watched) break;
        }
        Sleep(500);
    }
    DeleteFileA(path);
    page = watched & ~0xFFFULL;
    MEMORY_BASIC_INFORMATION info;
    if (VirtualQuery((void *)page, &info, sizeof info) != sizeof info || info.State != MEM_COMMIT ||
        info.Protect != PAGE_READWRITE) {
        fprintf(log, "%I64X: not a committed read-write page (protect %X): nothing armed\n", watched, info.Protect);
        goto next;
    }
    static void *veh;
    if (!veh) veh = AddVectoredExceptionHandler(1, handler);
    InterlockedExchange(&armed, 1);
    fprintf(log, "watching %I64X (page %I64X) veh %p: %s\n", watched, page, veh, arm() ? "armed" : "VirtualProtect failed");
    DWORD start = GetTickCount();
    LONG tail = head;  // head keeps counting across rounds
    for (int last = 0;; Sleep(50)) {
        while (ring[tail % RING].ready) {
            struct hit *h = &ring[tail % RING];
            fprintf(log, "%u tid %u %s %I64X at rip %I64X (img+%I64X)\n", GetTickCount() - start, h->tid,
                    h->info == 1 ? "write" : "read ", h->address, h->rip,
                    h->rip >= (UINT64)base ? h->rip - (UINT64)base : 0);
            InterlockedExchange(&h->ready, 0);
            tail++;
        }
        if (last) break;
        if (!armed || GetTickCount() - start >= RUN_SECONDS * 1000) {  // disarm, then drain the last hits
            InterlockedExchange(&armed, 0);
            Sleep(200);
            last = 1;
        }
    }
    fprintf(log, "done: %d hits, %d dropped, game still running\n", hits, dropped);
    goto next;
}

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID reserved) {
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(instance);
        HANDLE thread = CreateThread(NULL, 0, run, NULL, 0, NULL);
        if (thread) CloseHandle(thread);
    }
    return TRUE;
}
