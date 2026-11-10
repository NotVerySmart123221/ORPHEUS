// client_feed (ow_fixes.asi): sends the client's own body position to the lobby, which moves its copy of the
// player onto it (tools/lobby/client_sync.py, research/hitbox-drift.md). The client never sends its position, and
// the server's hitscan shoots from that copy. Read-only: it writes no game memory.
//
// The client's mover object (research/hitbox-drift.md, "The client's mover"):
//   +0x00  vtable img+0x260E7C8; a second vtable img+0x260E880 at +0x20
//   +0x70  f32 x, y, z  the feet after that frame's command, on the 1/1024 m grid
//   +0xA0  u32          the command frame: the server's Command.frame
// Movers sit in an array 0x1610 bytes apart; the free ones hold no position. A scan for the vtable finds them at any
// time. A new body (respawn, hero switch) takes another slot, sometimes just outside the ones found, so the
// neighbours are checked before scanning again.
//
// On only with OW_CLIENT_SYNC=PORT in the game's environment (run.py's child inherits it). Every new frame goes to
// 127.0.0.1:PORT as client_sync.MESSAGE: u32 frame, f32 x y z, u64 slot (which body).
//
// Shadow Step's aim (research/lizrollin-runtime.md section 36): the targeting state's item (statescript class
// STU_4C96470A, vtable img+0x261E920; a state item's +0x40 bit 0 is "active", research/asi-tracer.md) keeps where it
// would put him at +0x350 (f32 x, y, z: its tick 0x7FF6D1B08B40 has the placement query 0x7FF6D1EAEB30 write
// there) and whether the query found the place good at +0x374. While one is active, each new frame also goes out
// as client_sync.MARKER: u32 frame, f32 x y z, u8 good. The items are looked for on a thread of their own, so
// the feet's feed never waits for that scan.
#include <winsock2.h>
#include <math.h>
#include <stdlib.h>
#include "fixes.h"

#define TAG "client_feed"
#define VTABLE_RVA 0x260E7C8
#define SECOND_VTABLE_RVA 0x260E880
#define POSITION 0x70
#define FRAME 0xA0
#define STRIDE 0x1610
#define NEIGHBOURS 16          // neighbours checked on each side of every known slot
#define POOL 256               // shortcut: scan hits past this are dropped; raise it if the log says the pool filled
#define MAX_REGION (64 << 20)  // larger regions (asset caches) are not scanned
#define STALE_MS 2000          // without a new frame this long, the pool is checked again (respawn, hero switch)
#define RESCAN_MS 10000        // between full scans while no slot holds a body (menus, a new map's pool)
#define MARKER_VTABLE_RVA 0x261E920
#define ITEM_FLAGS 0x40
#define MARKER_AT 0x350
#define MARKER_GOOD 0x374
#define MARKER_ITEMS 32        // targeting state items kept from a scan (one per Reaper body the client has)
#define MARKER_RESCAN_MS 3000  // between scans while none of them is a targeting state any more
#ifndef CREATE_WAITABLE_TIMER_HIGH_RESOLUTION
#define CREATE_WAITABLE_TIMER_HIGH_RESOLUTION 0x2
#endif

#pragma pack(push, 1)
typedef struct {
    UINT32 frame;
    float feet[3];
    UINT64 slot;
} Sample;
typedef struct {
    UINT32 frame;
    float at[3];
    UINT8 good;
} Marker;
#pragma pack(pop)

static UINT64 marker_vtable;
static volatile UINT64 marker_items[MARKER_ITEMS];
static volatile LONG marker_count;
static UINT64 vtable, second_vtable;  // runtime addresses
static UINT64 pool[POOL], slot;       // slot: the one holding the body, 0 for none
static int pool_count, scans, have_frame;
static UINT32 last_frame;
static ULONGLONG next_scan, last_change;
static ULONGLONG WINAPI tick(void) { return GetTickCount64(); }
static ULONGLONG (WINAPI *now_ms)(void) = tick;  // the self-check sets its own clock

// Committed read-write memory without a guard page: a probe next to a known slot never trips a thread's stack guard.
static int readable(UINT64 at, SIZE_T size) {
    MEMORY_BASIC_INFORMATION info;
    if (VirtualQuery((void *)at, &info, sizeof info) != sizeof info) return 0;
    return info.State == MEM_COMMIT && info.Protect == PAGE_READWRITE &&
           at + size <= (UINT64)info.BaseAddress + info.RegionSize;
}

// The body in a slot, if it holds one. Two equal copies: the position and its frame are written separately, and one
// copy could catch them from different frames; a few tries get past the client's write.
static int sample(UINT64 at, Sample *out) {
    BYTE a[FRAME + 4], b[FRAME + 4];
    int tries = 0;
    if (!readable(at, sizeof a)) return 0;
    for (;; tries++) {
        if (tries == 4) return 0;
        __try {
            memcpy(a, (void *)at, sizeof a);
            memcpy(b, (void *)at, sizeof b);
        } __except (EXCEPTION_EXECUTE_HANDLER) {
            return 0;  // freed since the check
        }
        if (!memcmp(a, b, sizeof a)) break;
    }
    if (*(UINT64 *)a != vtable || *(UINT64 *)(a + 0x20) != second_vtable) return 0;
    const float *feet = (const float *)(a + POSITION);
    if (!isfinite(feet[0]) || !isfinite(feet[1]) || !isfinite(feet[2])) return 0;
    if (feet[0] == 0 && feet[1] == 0 && feet[2] == 0) return 0;  // a free slot
    out->frame = *(UINT32 *)(a + FRAME);
    memcpy(out->feet, feet, sizeof out->feet);
    out->slot = at;
    return 1;
}

// Every aligned qword equal to `value` (a class vtable) in committed read-write memory outside images: up to `most`
// addresses into `found`. This thread's own stack is skipped: sample()'s copies of a slot sit there and would pass
// for a body.
static int scan_for(UINT64 value, UINT64 *found, int most) {
    int count = 0;
    BYTE *at = (BYTE *)0x10000;
    MEMORY_BASIC_INFORMATION info;
    ULONG_PTR stack_low, stack_high;
    GetCurrentThreadStackLimits(&stack_low, &stack_high);
    while (count < most && VirtualQuery(at, &info, sizeof info) == sizeof info) {
        BYTE *end = (BYTE *)info.BaseAddress + info.RegionSize;
        ULONG_PTR start = (ULONG_PTR)info.BaseAddress;
        if (info.State == MEM_COMMIT && info.Type != MEM_IMAGE && info.Protect == PAGE_READWRITE &&
                info.RegionSize <= MAX_REGION && (start < stack_low || start >= stack_high)) {
            __try {
                for (UINT64 *q = info.BaseAddress; (BYTE *)(q + 1) <= end && count < most; q++)
                    if (*q == value) found[count++] = (UINT64)q;
            } __except (EXCEPTION_EXECUTE_HANDLER) {}  // a region freed while we read it
        }
        if (end <= at) break;
        at = end;
    }
    return count;
}

static int scan_movers(void) {
    scans++;
    int count = scan_for(vtable, pool, POOL);
    if (count == POOL) fix_log(TAG, "scan: pool full, later hits dropped");
    return count;
}

// A targeting state item's bytes up to its "good" flag; 0 when `at` is no such item (freed, reused).
static int marker_item(UINT64 at, BYTE *copy) {
    if (!at || !readable(at, MARKER_GOOD + 1)) return 0;
    __try {
        memcpy(copy, (void *)at, MARKER_GOOD + 1);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return 0;
    }
    return *(UINT64 *)copy == marker_vtable;
}

// Where the open aim would put him: the first active item's place. 0 while no aim is open.
static int read_marker(Marker *out) {
    BYTE copy[MARKER_GOOD + 1];
    for (LONG i = 0; i < marker_count; i++) {
        if (!marker_item(marker_items[i], copy) || !(copy[ITEM_FLAGS] & 1)) continue;
        const float *place = (const float *)(copy + MARKER_AT);
        if (!isfinite(place[0]) || !isfinite(place[1]) || !isfinite(place[2])) continue;
        memcpy(out->at, place, sizeof out->at);
        out->good = copy[MARKER_GOOD] != 0;
        return 1;
    }
    return 0;
}

// The marker thread's pass: a scan when none of the known items is a targeting state any more (a new body).
static void find_markers(void) {
    BYTE copy[MARKER_GOOD + 1];
    UINT64 found[MARKER_ITEMS];
    for (LONG i = 0; i < marker_count; i++)
        if (marker_item(marker_items[i], copy)) return;
    int count = scan_for(marker_vtable, found, MARKER_ITEMS);
    InterlockedExchange(&marker_count, 0);
    for (int i = 0; i < count; i++) marker_items[i] = found[i];
    InterlockedExchange(&marker_count, count);
}

static DWORD WINAPI marker_run(LPVOID unused) {
    (void)unused;
    for (;;) {
        find_markers();
        Sleep(MARKER_RESCAN_MS);
    }
}

// The slot holding the body: the known slots and their neighbours first, then (at most every RESCAN_MS) a scan.
// shortcut: with several bodies (never seen in the Range) the newest frame wins; upgrade if a mode gives remote
// bodies a client mover.
static int find(void) {
    for (int scanning = 0; scanning < 2; scanning++) {
        if (scanning) {
            if (now_ms() < next_scan) break;
            next_scan = now_ms() + RESCAN_MS;
            pool_count = scan_movers();
        }
        Sample best = {0}, s;
        int found = 0;
        for (int i = 0; i < pool_count; i++)
            for (INT64 k = -NEIGHBOURS; k <= NEIGHBOURS; k++)
                if (sample(pool[i] + k * STRIDE, &s) &&
                        (!found || s.frame > best.frame || (s.frame == best.frame && s.slot > best.slot)))
                    best = s, found = 1;
        if (found) {
            int known = 0;
            for (int i = 0; i < pool_count; i++) known |= pool[i] == best.slot;
            if (!known && pool_count < POOL) pool[pool_count++] = best.slot;
            slot = best.slot;
            last_change = now_ms();
            return 1;
        }
    }
    slot = 0;
    return 0;
}

// The body now; looks for it again when its slot is freed or its frame has not moved for STALE_MS.
static int read_body(Sample *out) {
    int ok = slot && sample(slot, out);
    if (!ok || now_ms() - last_change > STALE_MS) {
        if (!find()) return 0;
        ok = sample(slot, out);
    }
    if (ok && (!have_frame || out->frame != last_frame)) {
        last_frame = out->frame, have_frame = 1;
        last_change = now_ms();
        next_scan = last_change + RESCAN_MS;  // a lost body is looked for near the old slots first
    }
    return ok;
}

// 0.5 ms: a command frame is 7 ms and the client simulates some in pairs. Sleep(1) can mean 15.6 ms.
static void nap(HANDLE timer) {
    LARGE_INTEGER due;
    due.QuadPart = -5000;  // relative, 100 ns units
    if (timer && SetWaitableTimer(timer, &due, 0, NULL, NULL, FALSE)) WaitForSingleObject(timer, INFINITE);
    else Sleep(1);
}

DWORD WINAPI client_feed_run(LPVOID unused) {
    (void)unused;
    char text[128];
    DWORD n = GetEnvironmentVariableA("OW_CLIENT_SYNC", text, sizeof text);
    int port = n && n < sizeof text ? atoi(text) : 0;
    if (port <= 0 || port > 65535) {
        fix_log(TAG, n ? "OW_CLIENT_SYNC is not a port: off" : "off (no OW_CLIENT_SYNC)");
        return 0;
    }
    WSADATA wsa;
    if (WSAStartup(MAKEWORD(2, 2), &wsa)) { fix_log(TAG, "WSAStartup failed: off"); return 0; }
    SOCKET sock = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
    if (sock == INVALID_SOCKET) { fix_log(TAG, "no socket: off"); return 0; }
    struct sockaddr_in to = {0};
    to.sin_family = AF_INET;
    to.sin_port = htons((u_short)port);
    to.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    UINT64 base = (UINT64)GetModuleHandleW(NULL);
    vtable = base + VTABLE_RVA, second_vtable = base + SECOND_VTABLE_RVA;
    marker_vtable = base + MARKER_VTABLE_RVA;
    CreateThread(NULL, 0, marker_run, NULL, 0, NULL);
    HANDLE timer = CreateWaitableTimerExW(NULL, NULL, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS);
    _snprintf_s(text, sizeof text, _TRUNCATE, "feeding 127.0.0.1:%d", port);
    fix_log(TAG, text);
    UINT32 sent = 0;
    UINT64 logged = 0;
    for (int have_sent = 0;;) {
        Sample s;
        int ok = read_body(&s);
        if (!ok && !slot) {  // no body (menu, hero select): look again later
            Sleep(1000);
            continue;
        }
        if (ok && (!have_sent || s.frame != sent)) {
            sent = s.frame, have_sent = 1;
            sendto(sock, (const char *)&s, sizeof s, 0, (const struct sockaddr *)&to, sizeof to);
            Marker m;
            if (read_marker(&m)) {
                m.frame = s.frame;
                sendto(sock, (const char *)&m, sizeof m, 0, (const struct sockaddr *)&to, sizeof to);
            }
            if (s.slot != logged) {
                _snprintf_s(text, sizeof text, _TRUNCATE, "body at %llX, frame %u, %d scan(s)", s.slot, s.frame, scans);
                fix_log(TAG, text);
                logged = s.slot;
            }
        }
        nap(timer);
    }
}
