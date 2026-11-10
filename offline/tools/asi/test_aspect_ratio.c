#include <assert.h>
#include <windows.h>
static BOOL save_setting(LPCSTR,LPCSTR,LPCSTR,LPCSTR);
#define WritePrivateProfileStringA save_setting
#include "aspect_ratio.c"

static BYTE app_data[1600], device_data[256], window_data[80], old_target[128], new_target[128], depth_target[128];
static int allocations, releases, projections, fail_allocation, depth_rebuilds;
static int canvas_resizes, fail_canvas, camera_resizes, camera_width;
static int disable_during_camera;
static int intermediate = 1;
static HRESULT present_status;
static BYTE *test_vm;
static int native_queued, resize_requests, saved_settings;

static BOOL save_setting(LPCSTR section,LPCSTR key,LPCSTR value,LPCSTR path) {
    (void)path;
    assert(!strcmp(section,"Graphics") && !strcmp(key,"Aspect32x9") && !strcmp(value,"0"));
    saved_settings++;
    return TRUE;
}

static BYTE *__fastcall screen_vm(void *screen) { (void)screen; return test_vm; }

static void __fastcall native_apply(void *screen) {
    (void)screen;
    assert(enabled);
    if (native_queued == 1) {
        I(renderer,1544) = 1;
        I(renderer,1416) = I(renderer,1420) = 8;
    } else if (native_queued == 2) I(renderer,1548) = 1;
}

static void __fastcall request_resize(void *app,void *window,int x,int y,unsigned width,unsigned height) {
    assert(app == renderer && window == renderer+1328 && x == -1 && y == -1);
    assert(width == 5120 && height == 1440);
    resize_requests++;
}

static BYTE __fastcall intermediate_target(void *device,const DWORD *dimensions) {
    assert(device == device_data && dimensions[8] == 5120 && dimensions[9] == 1440);
    assert(dimensions[10] == 5120 && dimensions[11] == 1440);
    return (BYTE)intermediate;
}

static HRESULT STDMETHODCALLTYPE present_result(void *self,UINT sync,UINT flags) {
    (void)self; (void)sync; (void)flags;
    return present_status;
}

static HRESULT STDMETHODCALLTYPE present1_result(void *self,UINT sync,UINT flags,const void *parameters) {
    (void)parameters;
    return present_result(self,sync,flags);
}

static BYTE __fastcall refresh_canvas(void *canvas) {
    assert(canvas == old_target);
    canvas_resizes++;
    return !fail_canvas;
}

static void __fastcall fitted_native(int width,int height,int *out_width,int *out_height) {
    assert(width == 5120 && height == 1440);
    *out_width = 2560;
    *out_height = 1440;
}

static void __fastcall refresh_camera(void *camera,int width,int height,int unused) {
    assert((I(camera,576) & 0x300) == 0x200 && height == 1440 && unused == 0);
    assert(*(float *)((BYTE *)camera+548) == 0.6f);
    camera_width = width;
    camera_resizes++;
    *(float *)((BYTE *)camera+504) = (float)width;
    if (disable_during_camera) { disable_during_camera=0; enabled=0; }
}

static UINT64 __fastcall allocate_target(void *device, const DWORD *desc) {
    assert(device == device_data && desc[0] == 1 && desc[1] == 28);
    allocations++;
    I(new_target,8) = desc[3];
    I(new_target,12) = desc[4];
    return fail_allocation ? ~0ULL : 2;
}

static BYTE *__fastcall find_target(void *device, UINT64 handle) {
    assert(device == device_data);
    return handle == 1 ? old_target : handle == 2 || handle == 4 ? new_target : handle == 3 ? depth_target : NULL;
}

static void __fastcall clear_depth(void *device) { assert(device == device_data); }
static void __fastcall rebuild_depth(void *device) {
    assert(device == device_data);
    I(depth_target,8) = I(window_data,40);
    I(depth_target,12) = I(window_data,44);
    depth_rebuilds++;
}

static void __fastcall release_target(void *device, UINT64 handle) {
    assert(device == device_data && handle == 1);
    releases++;
}

static void __fastcall refresh_projection(void *camera, const float *rect, const float *custom, int unused) {
    assert(camera == app_data+576 && rect[0] == 0 && rect[1] == 0);
    assert(rect[2] == 5120 && rect[3] == 1440 && !custom && unused == 0);
    projections++;
}

static void prepare_target(void) {
    memset(app_data,0,sizeof app_data);
    memset(device_data,0,sizeof device_data);
    memset(window_data,0,sizeof window_data);
    memset(old_target,0,sizeof old_target);
    memset(new_target,0,sizeof new_target);
    allocations = releases = projections = fail_allocation = depth_rebuilds = canvas_resizes = fail_canvas = 0;
    memset(depth_target,0,sizeof depth_target);
    renderer = app_data;
    Q(renderer,0x4D0) = device_data;
    Q(device_data,88) = window_data;
    I(renderer,1416) = 5120;
    I(renderer,1420) = I(renderer,1428) = I(window_data,44) = I(old_target,12) = 1440;
    I(renderer,1424) = I(window_data,40) = I(old_target,8) = 2560;
    HANDLE_AT(window_data,24) = 1;
    HANDLE_AT(device_data,144) = 3;
    enabled = 1;
    resource_ready = 0;
    create_texture = allocate_target;
    needs_target = intermediate_target;
    intermediate = 1;
    texture_of = find_target;
    release_texture = release_target;
    ortho_camera = refresh_projection;
    drop_depth = clear_depth;
    create_depth = rebuild_depth;
    resize_canvas = refresh_canvas;
}

int main(void) {
    int w = 0, h = 0;
    assert(fit32(5120, 1440, &w, &h) && w == 5120 && h == 1440);
    assert(fit32(5120, 1417, &w, &h) && w == 5038 && h == 1417);
    assert(fit32(1920, 1080, &w, &h) && w == 1920 && h == 540);
    assert(fit32(3440, 1440, &w, &h) && w == 3440 && h == 968);
    assert(!fit32(0, 1440, &w, &h));
    assert(!fit32(5120, -1, &w, &h));
    assert(!fit32(32769, 1440, &w, &h));
    assert(fit32(32768, 32768, &w, &h) && w == 32768 && h == 9216);
    prepare_target();
    repair_viewport();
    assert(resource_ready && I(renderer,1424) == 5120 && I(window_data,40) == 5120);
    assert(HANDLE_AT(window_data,24) == 2 && allocations == 1 && releases == 1 && projections == 1);
    assert(depth_rebuilds == 1 && I(depth_target,8) == 5120 && I(depth_target,12) == 1440);
    repair_viewport();
    assert(allocations == 1 && releases == 1 && projections == 1 && depth_rebuilds == 1);

    prepare_target();
    fail_allocation = 1;
    repair_viewport();
    assert(!resource_ready && allocations == 1 && releases == 0 && projections == 0);
    assert(HANDLE_AT(window_data,24) == 1 && I(renderer,1424) == 2560 && I(window_data,40) == 2560);

    prepare_target();
    intermediate = 0;
    I(new_target,8) = 5120;
    I(new_target,12) = 1440;
    HANDLE_AT(window_data,16) = 4;
    repair_viewport();
    assert(resource_ready && allocations == 0 && releases == 1 && HANDLE_AT(window_data,24) == ~0ULL);
    repair_viewport();
    assert(allocations == 0 && releases == 1 && depth_rebuilds == 1);

    prepare_target();
    present_original = present_result;
    present1_original = present1_result;
    present_status = 0x087A0001; // DXGI_STATUS_OCCLUDED is positive, but no frame was presented.
    present(NULL,0,0);
    present1(NULL,0,0,NULL);
    assert(allocations == 0);
    present_status = S_OK;
    present(NULL,0,1);
    assert(allocations == 0);
    I(renderer,1544) = 1;
    present(NULL,0,0);
    assert(allocations == 0);
    I(renderer,1544) = 0;
    I(renderer,1548) = 1;
    present(NULL,0,0);
    assert(allocations == 0);
    I(renderer,1548) = 0;
    I(renderer,1416) = I(renderer,1420) = 8;
    assert(!wide_size(&w,&h));
    I(renderer,1416) = 5120;
    I(renderer,1420) = 1440;
    present(NULL,0,0);
    assert(allocations == 1);

    prepare_target();
    Q(renderer,120) = old_target;
    fail_canvas = 1;
    repair_viewport();
    assert(!resource_ready && canvas_resizes == 1 && projections == 0);
    fail_canvas = 0;
    repair_viewport();
    assert(resource_ready && canvas_resizes == 2 && projections == 1);

    BYTE camera[600] = {0};
    I(camera,576) = 0x10B;
    *(float *)(camera+504) = 2560;
    *(float *)(camera+508) = 1440;
    *(float *)(camera+548) = 0.6f;
    resize_camera = refresh_camera;
    native_fit = fitted_native;
    widen_camera(camera);
    assert(camera_resizes == 1 && camera_width == 5120 && I(camera,576) == 0x10B);
    enabled = 0;
    widen_camera(camera);
    assert(camera_resizes == 2 && camera_width == 2560 && I(camera,576) == 0x10B);
    widen_camera(camera);
    assert(camera_resizes == 2);
    enabled = 1;
    I(camera,576) = 5;
    widen_camera(camera);
    assert(camera_resizes == 2);
    I(camera,576) = 0x10B;
    disable_during_camera = 1;
    widen_camera(camera);
    assert(camera_resizes == 3 && !enabled);
    widen_camera(camera);
    assert(camera_resizes == 4 && camera_width == 2560);

    BYTE text[40] = {0}, owner[40] = {0};
    base = (BYTE *)0x10000000;
    memcpy(text,"16:9",4);
    I(text,16) = 4;
    I(text,20) = 16;
    UIValue inline_label = {(UINT64)PTR(void *,0x7FF6D39DE640),(UINT64)text};
    assert(label_is(&inline_label,"16:9") && !label_is(&inline_label,"16:10"));
    HANDLE_AT(owner,8) = 1;
    memcpy(owner+16,&inline_label,sizeof inline_label);
    UIValue shared_label = {(UINT64)owner | 1,0};
    assert(label_is(&shared_label,"16:9"));
    HANDLE_AT(owner,8) = 0;
    assert(!ui_payload(&shared_label));
    UIValue missing = {1,0};
    assert(!ui_payload(&missing));

    BYTE vm[144] = {0}, links[16] = {0}, value[32] = {0};
    int indices[16];
    for (int i=0;i<16;i++) indices[i] = -1;
    UINT64 key = 0x0D800000000065CAULL;
    indices[key & 15] = 0;
    Q(vm,64) = (BYTE *)indices;
    Q(vm,72) = links;
    Q(vm,80) = value;
    I(vm,88) = 16;
    I(links,4) = -1;
    HANDLE_AT(links,8) = key;
    assert(property(vm,key) == value && !property(vm,key+1));
    I(links,4) = 0;
    assert(!property(vm,key+16));
    I(vm,88) = 17;
    assert(!property(vm,key));

    I(vm,88) = 16;
    HANDLE_AT(vm,128) = 0x0A8800000000020DULL;
    I(value,0) = 0;
    test_vm = vm;
    view_model = screen_vm;
    screen_apply_original = native_apply;
    queue_resize = request_resize;
    native_queued = 1;
    prepare_target();
    screen_apply(NULL);
    assert(!enabled && resize_requests == 0 && saved_settings == 1 && I(renderer,1544));
    native_queued = 0;
    prepare_target();
    screen_apply(NULL);
    assert(!enabled && resize_requests == 1 && saved_settings == 2);
    native_queued = 2;
    prepare_target();
    screen_apply(NULL);
    assert(!enabled && resize_requests == 2 && saved_settings == 3);
    puts("32:9 sizing, allocation rollback/retry, FOV policy/restore and UI checks passed");
    return 0;
}
