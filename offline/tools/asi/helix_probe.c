// helix_probe.asi: traces the client's own projectile shots with in-process hardware breakpoints.
// A debugger attach crashes the 1.74 client (research/soldier-runtime/helix-visual-findings.md), so this plugin
// arms the debug registers itself (SetThreadContext on its own process's threads) and catches the traps in a
// vectored exception handler. No code or data of the game is written.
//
// Breakpoints (RVAs in Overwatch.exe 1.74.0.0.104319, image base 0x7FF6D1220000):
//   0 executor queues the projectile job   0x0AE724B  rsi = Volley:  key [rsi+0x1C0]
//   1 shot builder entry                   0x0AE43F0  rcx = request: asset [rcx+0x30]
//   2 creation entry                       0x0B21790  rdx = request: asset [rdx+0x30], flag a4 = r9b
//   3 creation return site                 0x0AE4609  rsi = request: status [rsi+0xF8], entity rax, asset [rsi+0x30]
// Creation status codes: research/soldier-runtime/helix-visual-findings.md (projectile creation trace).
//
// Log: build/helix_probe.log, one line per hit plus re-arm counts. Runs for RUN_SECONDS, then disarms.
// Control: with build/helix_probe.dummy present, all four breakpoints sit on never_called() in this DLL, which
// no game thread runs: a crash then comes from the armed debug registers themselves, not from a trap.
#include <windows.h>
#include <tlhelp32.h>
#include <stdio.h>

#define RUN_SECONDS 600
#define ARM_MS 500
#define RING 4096

static const UINT64 rvas[4] = {0x0AE724B, 0x0AE43F0, 0x0B21790, 0x0AE4609};
static UINT64 addr[4];
static DWORD self_tid;

static struct hit {
    LONG ready;
    int slot;
    DWORD tid, tick;
    UINT64 a, b, c;  // per slot: see the table above
} ring[RING];
static volatile LONG head, dropped;

static UINT64 peek(UINT64 address) {
    UINT64 value = 0;
    __try {
        value = *(volatile UINT64 *)address;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        value = 0xBAD0BAD0BAD0BAD0ULL;
    }
    return value;
}

static LONG CALLBACK handler(EXCEPTION_POINTERS *info) {
    if (info->ExceptionRecord->ExceptionCode != EXCEPTION_SINGLE_STEP)
        return EXCEPTION_CONTINUE_SEARCH;
    CONTEXT *ctx = info->ContextRecord;
    int slot = -1;
    for (int i = 0; i < 4; i++)
        if (ctx->Rip == addr[i])
            slot = i;
    if (slot < 0)
        return EXCEPTION_CONTINUE_SEARCH;
    LONG n = InterlockedIncrement(&head) - 1;
    struct hit *h = &ring[n % RING];
    if (h->ready) {
        InterlockedIncrement(&dropped);
    } else {
        h->slot = slot;
        h->tid = GetCurrentThreadId();
        h->tick = GetTickCount();
        if (slot == 0) {
            h->a = peek(ctx->Rsi + 0x1C0);
            h->b = ctx->Rsi;
            h->c = 0;
        } else if (slot == 1) {
            h->a = peek(ctx->Rcx + 0x30);
            h->b = ctx->Rcx;
            h->c = 0;
        } else if (slot == 2) {
            h->a = peek(ctx->Rdx + 0x30);
            h->b = ctx->Rdx;
            h->c = ctx->R9 & 0xFF;
        } else {
            h->a = peek(ctx->Rsi + 0x30);
            h->b = peek(ctx->Rsi + 0xF8) & 0xFFFFFFFF;
            h->c = ctx->Rax;
        }
        InterlockedExchange(&h->ready, 1);
    }
    ctx->Dr6 = 0;
    ctx->EFlags |= 0x10000;  // resume flag: run the trapped instruction once
    return EXCEPTION_CONTINUE_EXECUTION;
}

// Sets (or clears) DR0-DR3 on one thread; returns 1 when they had to be (re)written.
static int arm_thread(DWORD tid, int on) {
    HANDLE thread = OpenThread(THREAD_GET_CONTEXT | THREAD_SET_CONTEXT | THREAD_SUSPEND_RESUME, FALSE, tid);
    if (!thread)
        return 0;
    int changed = 0;
    if (SuspendThread(thread) != (DWORD)-1) {
        CONTEXT ctx = {0};
        ctx.ContextFlags = CONTEXT_DEBUG_REGISTERS;
        if (GetThreadContext(thread, &ctx)) {
            DWORD64 dr7 = on ? 0x55 : 0;  // L0-L3, RW/LEN 00: instruction breakpoints
            DWORD64 want[4] = {on ? addr[0] : 0, on ? addr[1] : 0, on ? addr[2] : 0, on ? addr[3] : 0};
            if (ctx.Dr0 != want[0] || ctx.Dr1 != want[1] || ctx.Dr2 != want[2] || ctx.Dr3 != want[3] ||
                (ctx.Dr7 & 0xFFFF00FF) != dr7) {
                ctx.Dr0 = want[0], ctx.Dr1 = want[1], ctx.Dr2 = want[2], ctx.Dr3 = want[3];
                ctx.Dr6 = 0;
                ctx.Dr7 = dr7;
                changed = SetThreadContext(thread, &ctx) ? 1 : 0;
            }
        }
        ResumeThread(thread);
    }
    CloseHandle(thread);
    return changed;
}

static int arm_all(int on, int *threads) {
    HANDLE snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
    int changed = 0;
    *threads = 0;
    if (snap == INVALID_HANDLE_VALUE)
        return 0;
    THREADENTRY32 te = {sizeof te};
    DWORD pid = GetCurrentProcessId();
    for (BOOL ok = Thread32First(snap, &te); ok; ok = Thread32Next(snap, &te)) {
        if (te.th32OwnerProcessID != pid || te.th32ThreadID == self_tid)
            continue;
        ++*threads;
        changed += arm_thread(te.th32ThreadID, on);
    }
    CloseHandle(snap);
    return changed;
}

static __declspec(noinline) void never_called(void) { OutputDebugStringA("helix_probe: never_called ran"); }

static const char *names[4] = {"queue", "builder", "create", "return"};

static void drain(FILE *log, LONG *tail) {
    for (; *tail < head; ++*tail) {
        struct hit *h = &ring[*tail % RING];
        if (!h->ready)
            break;
        if (h->slot == 0)
            fprintf(log, "%lu tid %lu queue   volley %llX key %llX\n", h->tick, h->tid, h->b, h->a);
        else if (h->slot == 1)
            fprintf(log, "%lu tid %lu builder request %llX asset %llX\n", h->tick, h->tid, h->b, h->a);
        else if (h->slot == 2)
            fprintf(log, "%lu tid %lu create  request %llX asset %llX flag %llu\n", h->tick, h->tid, h->b, h->a, h->c);
        else
            fprintf(log, "%lu tid %lu return  asset %llX status %llu entity %llX\n", h->tick, h->tid, h->a, h->b, h->c);
        InterlockedExchange(&h->ready, 0);
    }
}

static DWORD WINAPI run(LPVOID self) {
    char path[MAX_PATH];
    DWORD n = GetModuleFileNameA((HMODULE)self, path, MAX_PATH);
    if (!n || n < 4 || n >= MAX_PATH)
        return 1;
    lstrcpyA(path + n - 4, ".log");
    FILE *log = fopen(path, "w");
    if (!log)
        return 1;
    self_tid = GetCurrentThreadId();
    UINT64 base = (UINT64)GetModuleHandleA(NULL);
    char dummy[MAX_PATH];
    lstrcpyA(dummy, path);
    lstrcpyA(dummy + n - 4, ".dummy");
    int control = GetFileAttributesA(dummy) != INVALID_FILE_ATTRIBUTES;
    for (int i = 0; i < 4; i++)
        addr[i] = control ? (UINT64)never_called : base + rvas[i];
    if (control)
        fprintf(log, "control run: breakpoints on never_called\n");
    PVOID veh = AddVectoredExceptionHandler(1, handler);
    fprintf(log, "base %llX veh %p; breakpoints", base, veh);
    for (int i = 0; i < 4; i++)
        fprintf(log, " %s %llX", names[i], addr[i]);
    fprintf(log, "\n");
    fflush(log);
    LONG tail = 0;
    int threads = 0, last_changed = -1;
    for (DWORD start = GetTickCount(); GetTickCount() - start < RUN_SECONDS * 1000;) {
        int changed = arm_all(1, &threads);
        if (changed != last_changed || changed > 0) {
            fprintf(log, "%lu armed %d of %d threads (re)written\n", GetTickCount(), changed, threads);
            last_changed = changed;
        }
        for (int i = 0; i < ARM_MS / 50; i++) {  // drain often: a crash may follow a hit
            drain(log, &tail);
            if (dropped)
                fprintf(log, "dropped %ld\n", InterlockedExchange(&dropped, 0));
            fflush(log);
            Sleep(50);
        }
    }
    arm_all(0, &threads);
    Sleep(200);
    drain(log, &tail);
    fprintf(log, "%lu disarmed %d threads; done\n", GetTickCount(), threads);
    fclose(log);
    return 0;  // the handler stays registered: a trap already in flight must still find it
}

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID reserved) {
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(instance);
        HANDLE thread = CreateThread(NULL, 0, run, instance, 0, NULL);
        if (thread)
            CloseHandle(thread);
    }
    return TRUE;
}
