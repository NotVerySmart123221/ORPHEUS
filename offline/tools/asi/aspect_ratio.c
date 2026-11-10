// aspect_ratio (ow_fixes.asi): the 32:9 aspect choice (research/aspect-ratio-32-9/REPORT.md).
#include <windows.h>
#include <intrin.h>
#include <stdint.h>
#include <stdio.h>
#include "fixes.h"

#define VA(x) ((SIZE_T)((x##ULL) - 0x7FF6D1220000ULL))
#define PTR(type, x) ((type)(base + VA(x)))
#define Q(p, off) (*(BYTE **)((BYTE *)(p) + (off)))
#define I(p, off) (*(int *)((BYTE *)(p) + (off)))
#define HANDLE_AT(p, off) (*(UINT64 *)((BYTE *)(p) + (off)))

static BYTE *base, *renderer;
static volatile LONG enabled, resource_ready;
static void *volatile changed_cameras[64];
static char settings_path[MAX_PATH];
static UINT64 *present_table, *camera_table;
static UINT64 *base_camera_table;
static UINT64 *swapchain_table;
static HRESULT (STDMETHODCALLTYPE *present_original)(void *, UINT, UINT);
static HRESULT (STDMETHODCALLTYPE *present1_original)(void *, UINT, UINT, const void *);
static void (__fastcall *player_camera_original)(void *, float);
static void (__fastcall *base_camera_original)(void *, float);
static UINT64 (__fastcall *create_texture)(void *, const DWORD *);
static BYTE (__fastcall *needs_target)(void *,const DWORD *);
static BYTE *(__fastcall *texture_of)(void *, UINT64);
static void (__fastcall *release_texture)(void *, UINT64);
static void (__fastcall *resize_camera)(void *, int, int, int);
static void (__fastcall *native_fit)(int,int,int *,int *);
static void (__fastcall *ortho_camera)(void *, const float *, const float *, int);
static BYTE (__fastcall *resize_canvas)(void *);
static void (__fastcall *drop_depth)(void *);
static void (__fastcall *create_depth)(void *);
static void (__fastcall *queue_resize)(void *, void *, int, int, unsigned, unsigned);
static UINT64 *screen_table;
static void (__fastcall *screen_tick_original)(void *);
static void (__fastcall *screen_init_original)(void *);
static void (__fastcall *screen_apply_original)(void *);
static void (__fastcall *screen_revert_original)(void *);
static BYTE *(__fastcall *view_model)(void *);
static void (__fastcall *set_index)(void *, UINT64, int, int);
static void (__fastcall *set_boolean)(void *, UINT64, BYTE, int);
static BYTE *last_screen;
static BYTE *selectors[256];
static volatile LONG selector_count;
static BYTE *aspect_selector;
static LONG ui_stage;
static BYTE *ui_candidate;
static volatile LONG ui_queued;
static void note(const char *message);
static LONG ui_error(EXCEPTION_POINTERS *error);
static void add_choice(void);

static LONG ui_error(EXCEPTION_POINTERS *error) {
    char text[160];
    snprintf(text,sizeof text,"selector %p stage %ld exception %lX at %p",ui_candidate,ui_stage,
        error->ExceptionRecord->ExceptionCode,error->ExceptionRecord->ExceptionAddress);
    note(text);
    for (LONG i=0;i<selector_count;i++) if (selectors[i] == ui_candidate) selectors[i] = NULL;
    return EXCEPTION_EXECUTE_HANDLER;
}

typedef struct { UINT64 owner, data; } UIValue;
static void (__fastcall *enter_ui)(BYTE *,const UIValue *,BYTE);
static void (__fastcall *exit_ui)(BYTE *);
static BYTE *(__fastcall *ui_job)(unsigned);
static void (__fastcall *enqueue_ui)(int,void *);
static void (__fastcall *get_items)(void *, UIValue *);
static void (__fastcall *literal_string)(UINT64 *, const char *);
static UINT64 (__fastcall *box_string)(const UINT64 *);
static void (__fastcall *drop_string)(UINT64 *);
static void (__fastcall *drop_value)(void *, UIValue *);

static int fit32(int width, int height, int *out_width, int *out_height) {
    if (width <= 0 || height <= 0 || width > 32768 || height > 32768) return 0;
    *out_width = width;
    *out_height = height;
    if ((INT64)width * 9 > (INT64)height * 32)
        *out_width = (height * 32 + 4) / 9;
    else
        *out_height = (width * 9 + 16) / 32;
    return *out_width > 0 && *out_height > 0;
}

static void note(const char *message) { fix_log("aspect_ratio", message); }

static UINT64 *copy_table(UINT64 *source, SIZE_T slots) {
    UINT64 *copy = VirtualAlloc(NULL, (slots + 1) * sizeof *copy,
        MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if (!copy) return NULL;
    __try { memcpy(copy, source - 1, (slots + 1) * sizeof *copy); }
    __except (EXCEPTION_EXECUTE_HANDLER) { VirtualFree(copy,0,MEM_RELEASE); return NULL; }
    return copy + 1;
}

static int wide_size(int *width, int *height) {
    return enabled && renderer && !I(renderer,1544) && !I(renderer,1548) && I(renderer,1416) > 8 && I(renderer,1420) > 8 &&
        fit32(I(renderer, 1416), I(renderer, 1420), width, height);
}

static void repair_viewport(void) {
    int width, height;
    if (!wide_size(&width, &height)) { if (enabled) InterlockedExchange(&resource_ready,0); return; }
    BYTE *device = Q(renderer, 0x4D0);
    BYTE *window = device ? Q(device, 88) : NULL;
    if (!window) return;
    UINT64 old_handle = HANDLE_AT(window, 24);
    BYTE *texture = texture_of(device, old_handle);
    DWORD dimensions[12] = {0};
    dimensions[8] = I(renderer,1416);
    dimensions[9] = I(renderer,1420);
    dimensions[10] = width;
    dimensions[11] = height;
    if (!needs_target(device,dimensions)) {
        BYTE *backbuffer = texture_of(device,HANDLE_AT(window,16));
        if (!backbuffer || I(backbuffer,8) != width || I(backbuffer,12) != height) {
            InterlockedExchange(&resource_ready,0);
            return;
        }
        HANDLE_AT(window,24) = ~0ULL;
        if (texture) release_texture(device,old_handle);
    } else if (!texture || I(texture, 8) != width || I(texture, 12) != height) {
        DWORD desc[9] = {1, 28, 0, (DWORD)width, (DWORD)height, 1, 1, 1, 0};
        UINT64 handle = create_texture(device, desc);
        BYTE *replacement = texture_of(device, handle);
        if (!replacement) {
            InterlockedExchange(&resource_ready, 0);
            note("32:9 render target allocation failed; native viewport retained");
            return;
        }
        HANDLE_AT(window, 24) = handle;
        if (texture) release_texture(device, old_handle);
        char text[128];
        snprintf(text, sizeof text, "32:9 render target %dx%d, handle %I64X", width, height, handle);
        note(text);
    }
    int changed = !resource_ready || I(window,40) != width || I(window,44) != height ||
        I(renderer,1424) != width || I(renderer,1428) != height;
    I(window, 40) = I(renderer, 1424) = width;
    I(window, 44) = I(renderer, 1428) = height;
    BYTE *depth = texture_of(device,HANDLE_AT(device,144));
    if (!depth || I(depth,8) != width || I(depth,12) != height) {
        drop_depth(device);
        create_depth(device);
        depth = texture_of(device,HANDLE_AT(device,144));
        if (!depth || I(depth,8) != width || I(depth,12) != height) {
            InterlockedExchange(&resource_ready,0);
            note("32:9 depth allocation failed");
            return;
        }
        note("32:9 color and depth dimensions matched");
    }
    if (changed) {
        BYTE *canvas = Q(renderer,120);
        if (canvas && !resize_canvas(canvas)) { note("32:9 canvas resize failed"); InterlockedExchange(&resource_ready,0); return; }
        float rect[4] = {0, 0, (float)width, (float)height};
        ortho_camera(renderer + 576, rect, NULL, 0);
    }
    InterlockedExchange(&resource_ready, 1);
}

static HRESULT STDMETHODCALLTYPE present(void *self, UINT sync, UINT flags) {
    HRESULT result = present_original(self, sync, flags);
    if (result == S_OK && !(flags & 1)) repair_viewport();
    return result;
}

static HRESULT STDMETHODCALLTYPE present1(void *self, UINT sync, UINT flags, const void *parameters) {
    HRESULT result = present1_original(self, sync, flags, parameters);
    if (result == S_OK && !(flags & 1)) repair_viewport();
    return result;
}

static void hook_swapchain(void) {
    BYTE *device = Q(renderer, 0x4D0);
    BYTE *window = device ? Q(device, 88) : NULL;
    BYTE *swapchain = window ? Q(window, 8) : NULL;
    if (!swapchain || *(UINT64 **)swapchain == present_table) return;
    UINT64 *source = *(UINT64 **)swapchain;
    if (present_table && source != swapchain_table) {
        note("swapchain implementation changed; replacement not hooked");
        return;
    }
    if (!present_table) {
        swapchain_table = source;
        present_table = copy_table(source, 64);
        if (!present_table) { note("swapchain vtable allocation failed"); return; }
        present_original = (HRESULT (STDMETHODCALLTYPE *)(void *, UINT, UINT))source[8];
        present1_original = (HRESULT (STDMETHODCALLTYPE *)(void *, UINT, UINT, const void *))source[22];
        present_table[8] = (UINT64)present;
        present_table[22] = (UINT64)present1;
        DWORD old;
        VirtualProtect(present_table - 1, 65 * 8, PAGE_READONLY, &old);
    }
    InterlockedCompareExchangePointer((void *volatile *)swapchain, present_table, source);
    note("swapchain hooked; native engine vtables retained");
}

static void widen_camera(void *self) {
    LONG wide = enabled;
    if (renderer && (I(self,576) & 2) &&
            *(float *)((BYTE *)self+496) == 0.0f && *(float *)((BYTE *)self+500) == 0.0f) {
        int w = I(renderer, 1424), h = I(renderer, 1428);
        float cw = *(float *)((BYTE *)self+504), ch = *(float *)((BYTE *)self+508);
        int native_w = (I(renderer,1420) * 16 + 4) / 9;
        int slot = -1;
        for (int i=0;i<64;i++) if (changed_cameras[i] == self) { slot=i; break; }
        if (w <= 0 || h <= 0) return;
        if (wide) {
            if (!resource_ready || ch != (float)h || (cw != (float)native_w && cw != (float)w)) return;
            if (slot < 0) for (int i=0;i<64;i++) {
                if (!InterlockedCompareExchangePointer(&changed_cameras[i],self,NULL)) { slot=i; break; }
            }
        } else {
            if (slot < 0) return;
            // The queued native resize can still be pending when this callback runs.
            native_fit(I(renderer,1416),I(renderer,1420),&w,&h);
            if (w <= 0 || h <= 0) return;
        }
        if (slot >= 0) {
            int flags = I(self,576);
            // Preserve vertical FOV; native cameras can otherwise retain both angles and stretch.
            I(self,576) = (flags & ~0x100) | 0x200;
            resize_camera(self, w, h, 0);
            I(self,576) = flags;
            if (!wide) InterlockedCompareExchangePointer(&changed_cameras[slot],NULL,self);
        }
    }
}

static void __fastcall player_camera_update(void *self,float dt) {
    player_camera_original(self,dt);
    widen_camera(self);
}

static void __fastcall base_camera_update(void *self,float dt) {
    base_camera_original(self,dt);
    widen_camera(self);
}

static BYTE *ui_payload(UIValue *value) {
    if (!(value->owner & 1)) return value->owner ? (BYTE *)value : NULL;
    UINT64 owner = value->owner & ~1ULL;
    return owner && (*(UINT64 *)(owner+8) & 0x1FFFFF) ? (BYTE *)(owner+16) : NULL;
}

static int label_is(UIValue *value, const char *expected) {
    BYTE *payload = ui_payload(value);
    if (!payload || *(UINT64 *)payload != (UINT64)PTR(void *, 0x7FF6D39DE640)) return 0;
    BYTE *text = Q(payload,8);
    SIZE_T length = strlen(expected);
    if (!text || I(text,16) != (int)length) return 0;
    const char *chars = I(text,20) <= 16 ? (const char *)text : (const char *)Q(text,0);
    return chars && !memcmp(chars,expected,length);
}

static BYTE *property(BYTE *vm, UINT64 key) {
    int buckets = I(vm,88);
    if (buckets <= 0 || buckets > 65536 || (buckets & (buckets-1))) return NULL;
    BYTE *indices = Q(vm,64), *links = Q(vm,72), *values = Q(vm,80);
    if (!indices || !links || !values) return NULL;
    int index = ((int *)indices)[(unsigned)key & (buckets-1)];
    for (int n = 0; index >= 0 && index < 65536 && n < 256; n++) {
        if (HANDLE_AT(links,16LL*index+8) == key) return values+32LL*index;
        index = I(links,16LL*index+4);
    }
    return NULL;
}

static BYTE *graphics_vm(void *screen) {
    BYTE *vm = view_model(screen);
    return vm && HANDLE_AT(vm,128) == 0x0A8800000000020DULL ? vm : NULL;
}

static void add_choice(void) {
    if (aspect_selector) return;
    if (!ui_payload(PTR(UIValue *,0x7FF6D40DA728))) return;
    for (LONG i = 0, n = selector_count; i < n; i++) {
        BYTE *selector = selectors[i];
        if (!selector) continue;
        ui_candidate = selector;
        ui_stage = 1;
        if (HANDLE_AT(selector,0) != (UINT64)PTR(void *,0x7FF6D39E3958)) continue;
        if (!(HANDLE_AT(selector,-8) & 0x1FFFFF)) continue;
        if (HANDLE_AT(selector,648) != (UINT64)PTR(void *,0x7FF6D39F5938)) continue;
        UIValue source = {1,0};
        get_items(selector,&source);
        ui_stage = 2;
        BYTE *collection = ui_payload(&source);
        if (!collection || *(UINT64 *)collection != (UINT64)PTR(void *,0x7FF6D39FA4A0)) continue;
        UINT64 *table = *(UINT64 **)collection;
        ui_stage = 3;
        int (__fastcall *count)(void *) = (int (__fastcall *)(void *))table[35];
        int length = count(collection);
        if (length != 3 && length != 4) continue;
        void (__fastcall *at)(void *,UIValue *,int) = (void (__fastcall *)(void *,UIValue *,int))table[36];
        UIValue items[3] = {{1,0},{1,0},{1,0}};
        ui_stage = 4;
        for (int j=0;j<3;j++) at(collection,&items[j],j);
        if (!label_is(&items[0],"16:9") || !label_is(&items[1],"16:10") || !label_is(&items[2],"21:9")) continue;
        if (length == 4) {
            UIValue fourth = {1,0};
            at(collection,&fourth,3);
            if (label_is(&fourth,"32:9")) { aspect_selector = selector; last_screen = NULL; }
            continue;
        }
        UINT64 text = 0;
        ui_stage = 5;
        literal_string(&text,"32:9");
        ui_stage = 6;
        UIValue label = {box_string(&text) | 1,0};
        drop_string(&text);
        if (!label_is(&label,"32:9")) { note("constructed label invalid"); drop_value(NULL,&label); return; }
        if (ui_payload(&label)) {
            ui_stage = 7;
            ((void (__fastcall *)(void *,const UIValue *))table[29])(collection,&label);
            if (count(collection) == 4) {
                aspect_selector = selector;
                last_screen = NULL;
                note("added 32:9 to native aspect ratio choices");
            }
        }
        ui_stage = 8;
        drop_value(NULL,&label);
    }
}

typedef struct { UINT64 *table; int id; } UIAction;

static void *__fastcall free_ui_action(UIAction *action,unsigned flags) {
    (void)flags;
    HeapFree(GetProcessHeap(),0,action);
    InterlockedExchange(&ui_queued,0);
    return NULL;
}

static void __fastcall execute_ui_action(UIAction *action,int id) {
    (void)action;
    int buckets = *PTR(int *,0x7FF6D40DAB20);
    BYTE *indices = *PTR(BYTE **,0x7FF6D40DAB08);
    BYTE *links = *PTR(BYTE **,0x7FF6D40DAB10);
    UIValue *values = *PTR(UIValue **,0x7FF6D40DAB18);
    if (!indices || !links || !values || buckets <= 0 || buckets > 65536 || (buckets & (buckets-1))) return;
    int index = ((int *)indices)[(unsigned)id & (buckets-1)];
    for (int n=0;index>=0 && index<65536 && n<256;n++,index=I(links,12LL*index+4)) {
        if (I(links,12LL*index+8) != id) continue;
        UIValue context = values[index];
        if (!ui_payload(&context)) return;
        BYTE scope = 0;
        enter_ui(&scope,&context,0);
        __try {
            __try { add_choice(); } __except (ui_error(GetExceptionInformation())) { }
        } __finally { exit_ui(&scope); }
        return;
    }
}

static void queue_choice(BYTE *vm) {
    static UINT64 table[3] = {(UINT64)free_ui_action,0,(UINT64)execute_ui_action};
    if (aspect_selector || ui_queued) return;
    BYTE *root = vm;
    for (int n=0;Q(root,48) && n<64;n++) root = Q(root,48);
    if (Q(root,48)) return;
    BYTE *job = ui_job((unsigned)I(root,40));
    if (!job || InterlockedCompareExchange(&ui_queued,1,0)) return;
    UIAction *action = HeapAlloc(GetProcessHeap(),HEAP_ZERO_MEMORY,sizeof *action);
    if (!action) { InterlockedExchange(&ui_queued,0); return; }
    action->table = table;
    enqueue_ui(I(job,0),action);
}

static void __fastcall screen_tick(void *self) {
    BYTE *vm = graphics_vm(self);
    if (!vm) { screen_tick_original(self); return; }
    queue_choice(vm);
    if (last_screen != self) {
        last_screen = self;
        if (enabled && aspect_selector) set_index(vm,0x0D800000000065CAULL,3,0);
    }
    BYTE *value = property(vm,0x0D800000000065CAULL);
    int pending = value ? I(value,0) : -1;
    if (pending == 3) I(value,0) = 0;
    screen_tick_original(self);
    value = property(vm,0x0D800000000065CAULL);
    if (pending == 3 && value) I(value,0) = 3;
    if (pending >= 0 && pending <= 3 && ((pending == 3) != (enabled != 0)))
        set_boolean(vm,0x0D800000000064F6ULL,1,0);
}

static void __fastcall screen_init(void *self) {
    screen_init_original(self);
    aspect_selector = NULL;
    last_screen = NULL;
}

static void __fastcall screen_apply(void *self) {
    BYTE *vm = graphics_vm(self);
    BYTE *value = vm ? property(vm,0x0D800000000065CAULL) : NULL;
    int pending = value ? I(value,0) : -1;
    LONG previous = enabled;
    if (pending >= 0 && pending <= 3 && previous != (pending == 3)) InterlockedExchange(&resource_ready,0);
    screen_apply_original(self);
    if (pending < 0 || pending > 3) return;
    InterlockedExchange(&enabled,pending == 3);
    if (!WritePrivateProfileStringA("Graphics","Aspect32x9",enabled ? "1" : "0",settings_path))
        note("aspect ratio applied, but saving plugin setting failed");
    if (previous != enabled && !I(renderer,1544))
        queue_resize(renderer,renderer+1328,-1,-1,I(renderer,1416),I(renderer,1420));
    if (pending == 3) set_index(vm,0x0D800000000065CAULL,3,0);
    note(enabled ? "32:9 applied" : "native aspect ratio applied");
}

static void __fastcall screen_revert(void *self) {
    screen_revert_original(self);
    BYTE *vm = graphics_vm(self);
    if (vm && enabled && aspect_selector) set_index(vm,0x0D800000000065CAULL,3,0);
}

static int scan_cameras(UINT64 from, UINT64 to) {
    int count = 0;
    BYTE *at = (BYTE *)0x10000;
    MEMORY_BASIC_INFORMATION info;
    ULONG_PTR stack_low, stack_high;
    GetCurrentThreadStackLimits(&stack_low, &stack_high);
    while (VirtualQuery(at, &info, sizeof info) == sizeof info) {
        BYTE *end = (BYTE *)info.BaseAddress + info.RegionSize;
        if (info.State == MEM_COMMIT && info.Type == MEM_PRIVATE &&
                info.Protect == PAGE_READWRITE && info.RegionSize <= (64u << 20) &&
                !((UINT_PTR)info.BaseAddress < stack_high && (UINT_PTR)end > stack_low)) {
            __try {
                for (UINT64 *q = (UINT64 *)info.BaseAddress; (BYTE *)(q + 1) <= end; q++) {
                    if (*q == from) {
                        InterlockedCompareExchange64((LONG64 *)q, to, from);
                        count++;
                    }
                    else if (*q == (UINT64)PTR(void *,0x7FF6D39C9100)) {
                        InterlockedCompareExchangePointer((void *volatile *)q,base_camera_table,(void *)*q);
                        count++;
                    }
                    else if (*q == (UINT64)PTR(void *,0x7FF6D386C3A0)) {
                        InterlockedCompareExchangePointer((void *volatile *)q,screen_table,(void *)*q);
                        last_screen = NULL;
                        aspect_selector = NULL;
                    }
                    else if (*q == (UINT64)PTR(void *,0x7FF6D39F5938)) {
                        BYTE *candidate = (BYTE *)q - 648;
                        if (HANDLE_AT(candidate,0) != (UINT64)PTR(void *,0x7FF6D39E3958)) continue;
                        int known = 0;
                        for (LONG j=0;j<selector_count;j++) if (selectors[j] == candidate) { known=1; break; }
                        if (!known) {
                            static LONG next;
                            LONG slot = next++ % 256;
                            selectors[slot] = candidate;
                            aspect_selector = NULL;
                            if (selector_count < 256) InterlockedIncrement(&selector_count);
                        }
                    }
                }
            } __except (EXCEPTION_EXECUTE_HANDLER) { }
        }
        if (end <= at) break;
        at = end;
    }
    for (int i=0;i<64;i++) {
        BYTE *camera = changed_cameras[i];
        if (!camera) continue;
        __try {
            UINT64 *table = *(UINT64 **)camera;
            if (table == camera_table || table == base_camera_table) continue;
        } __except (EXCEPTION_EXECUTE_HANDLER) { }
        InterlockedCompareExchangePointer(&changed_cameras[i],NULL,camera);
    }
    return count;
}

DWORD WINAPI aspect_ratio_run(LPVOID unused) {
    (void)unused;
    base = (BYTE *)GetModuleHandleW(NULL);
    if (!fix_path(settings_path, ".ini")) return 1;
    enabled = GetPrivateProfileIntA("Graphics", "Aspect32x9", 0, settings_path) == 1;
    UINT64 *rv = PTR(UINT64 *, 0x7FF6D39B2A20);
    UINT64 *camera = PTR(UINT64 *, 0x7FF6D38335B0);
    for (int attempt = 0;; attempt++) {
        __try {
            renderer = (BYTE *)(*(UINT64 *)(base + VA(0x7FF6D407A6B0)) ^ 0x9E3B404B34CBD81EULL);
            if (renderer && *(UINT64 **)renderer == rv &&
                    rv[81] == (UINT64)PTR(void *, 0x7FF6D2C14740) &&
                    camera[5] == (UINT64)PTR(void *, 0x7FF6D18BF3E0) &&
                    PTR(UINT64 *,0x7FF6D39C9100)[5] == (UINT64)PTR(void *,0x7FF6D2CA3280) &&
                    PTR(UINT64 *,0x7FF6D386C3A0)[14] == (UINT64)PTR(void *,0x7FF6D1CCCA70)) break;
        } __except (EXCEPTION_EXECUTE_HANDLER) { }
        if (attempt >= 1200) { note("build/graphics signature mismatch; no hooks installed"); return 1; }
        Sleep(100);
    }
    if (enabled) {
        // Keep the native 16:9 camera policy; native 21:9 applies a vertical crop.
        *PTR(BYTE *,0x7FF6D4447148) = 1;
        *PTR(BYTE *,0x7FF6D44471A8) = 0;
    }
    camera_table = copy_table(camera, 6);
    base_camera_table = copy_table(PTR(UINT64 *,0x7FF6D39C9100),6);
    UINT64 *screen = PTR(UINT64 *,0x7FF6D386C3A0);
    screen_table = copy_table(screen,69);
    if (!camera_table || !screen_table || !base_camera_table) {
        note("vtable allocation failed; no hooks installed");
        return 1;
    }
    player_camera_original = (void (__fastcall *)(void *,float))camera[5];
    base_camera_original = (void (__fastcall *)(void *,float))PTR(UINT64 *,0x7FF6D39C9100)[5];
    camera_table[5] = (UINT64)player_camera_update;
    base_camera_table[5] = (UINT64)base_camera_update;
    screen_tick_original = (void (__fastcall *)(void *))screen[14];
    screen_init_original = (void (__fastcall *)(void *))screen[65];
    screen_apply_original = (void (__fastcall *)(void *))screen[66];
    screen_revert_original = (void (__fastcall *)(void *))screen[67];
    screen_table[14] = (UINT64)screen_tick;
    screen_table[65] = (UINT64)screen_init;
    screen_table[66] = (UINT64)screen_apply;
    screen_table[67] = (UINT64)screen_revert;
    create_texture = PTR(UINT64 (__fastcall *)(void *, const DWORD *), 0x7FF6D2C1A160);
    needs_target = PTR(BYTE (__fastcall *)(void *,const DWORD *),0x7FF6D2C2B190);
    texture_of = PTR(BYTE *(__fastcall *)(void *, UINT64), 0x7FF6D2C2BA50);
    release_texture = PTR(void (__fastcall *)(void *, UINT64), 0x7FF6D2C202C0);
    resize_camera = PTR(void (__fastcall *)(void *, int, int, int), 0x7FF6D2C964B0);
    native_fit = PTR(void (__fastcall *)(int,int,int *,int *),0x7FF6D2C6F8F0);
    ortho_camera = PTR(void (__fastcall *)(void *,const float *,const float *,int),0x7FF6D2C95A70);
    resize_canvas = PTR(BYTE (__fastcall *)(void *),0x7FF6D2C363F0);
    drop_depth = PTR(void (__fastcall *)(void *),0x7FF6D2C284E0);
    create_depth = PTR(void (__fastcall *)(void *),0x7FF6D2C244A0);
    queue_resize = PTR(void (__fastcall *)(void *,void *,int,int,unsigned,unsigned),0x7FF6D2C7F700);
    view_model = PTR(BYTE *(__fastcall *)(void *),0x7FF6D1C0E1F0);
    set_index = PTR(void (__fastcall *)(void *,UINT64,int,int),0x7FF6D2FECC80);
    set_boolean = PTR(void (__fastcall *)(void *,UINT64,BYTE,int),0x7FF6D2FECCD0);
    get_items = PTR(void (__fastcall *)(void *,UIValue *),0x7FF6D2EF2390);
    literal_string = PTR(void (__fastcall *)(UINT64 *,const char *),0x7FF6D2F6F110);
    box_string = PTR(UINT64 (__fastcall *)(const UINT64 *),0x7FF6D2E1CE70);
    drop_string = PTR(void (__fastcall *)(UINT64 *),0x7FF6D2F6F220);
    drop_value = PTR(void (__fastcall *)(void *,UIValue *),0x7FF6D2FDD460);
    enter_ui = PTR(void (__fastcall *)(BYTE *,const UIValue *,BYTE),0x7FF6D2E77EA0);
    exit_ui = PTR(void (__fastcall *)(BYTE *),0x7FF6D2E78540);
    ui_job = PTR(BYTE *(__fastcall *)(unsigned),0x7FF6D2E83470);
    enqueue_ui = PTR(void (__fastcall *)(int,void *),0x7FF6D2E848A0);
    DWORD old;
    VirtualProtect(camera_table - 1, 7 * 8, PAGE_READONLY, &old);
    VirtualProtect(base_camera_table - 1,7*8,PAGE_READONLY,&old);
    VirtualProtect(screen_table - 1,70*8,PAGE_READONLY,&old);
    note(enabled ? "32:9 enabled" : "32:9 disabled");
    for (;;) {
        __try { hook_swapchain(); } __except (EXCEPTION_EXECUTE_HANDLER) { }
        int count = scan_cameras((UINT64)camera, (UINT64)camera_table);
        if (count) { char text[80]; snprintf(text, sizeof text, "camera hooks: %d", count); note(text); }
        Sleep(2000);
    }
}
