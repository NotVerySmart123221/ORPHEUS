// Self-check of client_feed.c (the same cases as tools/research/test_client_position.py), over fake mover slots in
// this process's own memory. build.bat runs it before building ow_fixes.asi.
#include <assert.h>
#include <stddef.h>
#include "client_feed.c"

#define SLOTS 64
static UINT64 tables[2];  // in the image, so no scan finds the fake vtable values outside the slots
static ULONGLONG clock_now;
static ULONGLONG WINAPI fake_clock(void) { return clock_now; }
static BYTE *slots;

static UINT64 at(int i) { return (UINT64)(slots + (SIZE_T)i * STRIDE); }

static void put(int i, UINT32 frame, float x, float y, float z, UINT64 table) {
    BYTE *p = (BYTE *)at(i);
    *(UINT64 *)p = table;
    *(UINT64 *)(p + 0x20) = second_vtable;
    float *feet = (float *)(p + POSITION);
    feet[0] = x, feet[1] = y, feet[2] = z;
    *(UINT32 *)(p + FRAME) = frame;
}

static void reset(void) {
    memset(slots, 0, (SIZE_T)SLOTS * STRIDE);
    pool_count = scans = have_frame = 0;
    slot = 0, next_scan = last_change = 0;
}

int main(void) {
    assert(sizeof(Sample) == 24 && offsetof(Sample, feet) == 4 && offsetof(Sample, slot) == 16);  // <I3fQ
    assert(sizeof(Marker) == 17 && offsetof(Marker, at) == 4 && offsetof(Marker, good) == 16);  // <I3fB

    // Shadow Step's aim: the active targeting state item's place and whether it is good; none while no aim is open
    static UINT64 marker_table;  // in the image, like `tables`
    marker_vtable = (UINT64)&marker_table;
    BYTE *items = VirtualAlloc(NULL, 0x2000, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    assert(items);
    Marker m;
    find_markers();
    assert(marker_count == 0 && !read_marker(&m));
    *(UINT64 *)(items + 0x800) = marker_vtable;
    float *place = (float *)(items + 0x800 + MARKER_AT);
    place[0] = 54.5f, place[1] = 1.0f, place[2] = 35.25f;
    items[0x800 + MARKER_GOOD] = 1;
    find_markers();
    assert(marker_count == 1 && marker_items[0] == (UINT64)(items + 0x800));
    assert(!read_marker(&m));  // not active: no aim is open
    items[0x800 + ITEM_FLAGS] = 1;
    assert(read_marker(&m) && m.at[0] == 54.5f && m.at[2] == 35.25f && m.good == 1);
    items[0x800 + MARKER_GOOD] = 0;
    assert(read_marker(&m) && m.good == 0);
    *(UINT64 *)(items + 0x800) = 0;  // the body is gone: the item is no targeting state any more
    assert(!read_marker(&m));
    *(UINT64 *)(items + 0x1000) = marker_vtable;  // a new body's item: found by the next pass
    find_markers();
    assert(marker_count == 1 && marker_items[0] == (UINT64)(items + 0x1000));
    vtable = (UINT64)&tables[0], second_vtable = (UINT64)&tables[1];
    now_ms = fake_clock;
    clock_now = 1000000;
    slots = VirtualAlloc(NULL, (SIZE_T)SLOTS * STRIDE, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    assert(slots);
    Sample s;

    // frame and feet; free slot, wrong vtable, NaN and uncommitted memory are not bodies
    reset();
    put(30, 77, 54.5f, 1.0f, 42.0f, vtable);
    assert(sample(at(30), &s) && s.frame == 77 && s.feet[0] == 54.5f && s.feet[2] == 42.0f && s.slot == at(30));
    put(31, 5, 0, 0, 0, vtable);
    put(32, 5, 1, 1, 1, vtable + 8);
    put(33, 5, NAN, 1, 1, vtable);
    for (int i = 31; i <= 34; i++) assert(!sample(at(i), &s));
    BYTE *reserved = VirtualAlloc(NULL, 0x1000, MEM_RESERVE, PAGE_NOACCESS);
    assert(!sample((UINT64)reserved, &s));

    // a guard page is skipped and keeps its guard (a thread's stack guard must not be tripped)
    BYTE *guard = VirtualAlloc(NULL, 0x1000, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE | PAGE_GUARD);
    MEMORY_BASIC_INFORMATION info;
    assert(!sample((UINT64)guard, &s));
    assert(VirtualQuery(guard, &info, sizeof info) && (info.Protect & PAGE_GUARD));

    // a scan, then the slot with a position; a respawn uses the remembered pool, no new scan
    reset();
    put(20, 90, 0, 0, 0, vtable);
    put(21, 100, 3, 1, 4, vtable);
    assert(find() && slot == at(21) && scans == 1);
    put(20, 120, 5, 1, 5, vtable);
    put(21, 100, 0, 0, 0, vtable);
    assert(find() && slot == at(20) && scans == 1);

    // a new body just outside the pool is found without a scan, and joins the pool
    reset();
    put(30, 100, 3, 1, 4, vtable);
    assert(read_body(&s) && s.frame == 100);
    memset((void *)at(30), 0, STRIDE);  // freed
    put(29, 101, 5, 1, 5, vtable);
    assert(read_body(&s) && s.frame == 101 && s.slot == at(29) && scans == 1);
    int pooled = 0;
    for (int i = 0; i < pool_count; i++) pooled |= pool[i] == at(29);
    assert(pooled);

    // full scans are throttled; a copy of a slot on this thread's stack is no body
    reset();
    put(30, 100, 3, 1, 4, vtable);
    volatile BYTE copy[FRAME + 4];
    memcpy((void *)copy, (void *)at(30), sizeof copy);
    memset((void *)at(30), 0, STRIDE);
    assert(!find() && !find() && scans == 1);
    clock_now += RESCAN_MS;
    assert(!find() && scans == 2);

    // a stalled frame makes it look again: the new body wins over the old one's last frame
    reset();
    put(40, 100, 3, 1, 4, vtable);
    put(41, 0, 0, 0, 0, vtable);
    assert(read_body(&s) && s.frame == 100);
    put(41, 200, 6, 1, 6, vtable);
    clock_now += STALE_MS + 1000;
    assert(read_body(&s) && s.frame == 200 && slot == at(41));

    puts("client feed checks passed");
    return 0;
}
