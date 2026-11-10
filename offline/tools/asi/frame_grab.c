// frame_grab.asi: saves the game's back buffer as a BMP on request, right before Present. Opt-in, research only.
// Independent of DWM (PrintWindow sometimes returns black). No game change beyond one vtable copy.
// - A dummy D3D11 device + swap chain gives the IDXGISwapChain vtable pointer; the game's own swap chain object
//   should carry the same pointer (dxgi.dll's class; unverified). The vtable is copied with Present (slot 8) and
//   Present1 (slot 22) replaced, and the pointer is swapped in private read-write memory (the image is read-only).
//   Rescanned every 2 s for 5 minutes, then every 10 s (a resize can recreate the swap chain).
// - Request: <plugin>.req, first line = absolute output path. The worker deletes it and raises a flag; the next
//   Present writes <path>.tmp, then renames it onto <path>. No Present within 3 s drops the request.
// Log: <plugin>.log. Build: build.bat. Notes: README.md.
#define COBJMACROS
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <stdio.h>

#define SLOT_PRESENT 8
#define SLOT_PRESENT1 22
#define VTABLE_SLOTS 64
#define MAX_REGION (64u << 20)  // larger private regions are asset caches, not object heaps
#define LINE 512
#define RING 256
#define WAIT_MS 3000

typedef HRESULT (STDMETHODCALLTYPE *present_fn)(void *, UINT, UINT);
typedef HRESULT (STDMETHODCALLTYPE *present1_fn)(void *, UINT, UINT, const void *);

static present_fn present_orig;
static present1_fn present1_orig;
static char req_path[MAX_PATH];
static volatile LONG req_state;  // 0 idle, 1 pending (worker -> hook), 2 claimed by the hook
static char log_path[MAX_PATH], req_file[MAX_PATH];

// ---- log ring: the render thread and the worker produce, the worker drains ----
static char ring[RING][LINE];
static volatile LONG ready[RING];
static volatile LONG head;

static void note(const char *fmt, ...) {
    LONG n = InterlockedIncrement(&head) - 1;
    char *line = ring[n % RING];
    if (ready[n % RING]) return;
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

static void drain(void) {
    static LONG tail;
    HANDLE f = INVALID_HANDLE_VALUE;
    while (ready[tail % RING]) {
        if (f == INVALID_HANDLE_VALUE) {
            f = CreateFileA(log_path, FILE_APPEND_DATA, FILE_SHARE_READ, NULL, OPEN_ALWAYS, 0, NULL);
            if (f == INVALID_HANDLE_VALUE) return;
        }
        DWORD written;
        WriteFile(f, ring[tail % RING], (DWORD)strlen(ring[tail % RING]), &written, NULL);
        InterlockedExchange(&ready[tail % RING], 0);
        tail++;
    }
    if (f != INVALID_HANDLE_VALUE) CloseHandle(f);
}

// ---- vtable patching (same technique as tracer.c) ----
static int swap_pointers(UINT64 from, UINT64 to) {
    int swapped = 0;
    BYTE *at = (BYTE *)0x10000;
    MEMORY_BASIC_INFORMATION info;
    while (VirtualQuery(at, &info, sizeof info) == sizeof info) {
        BYTE *end = (BYTE *)info.BaseAddress + info.RegionSize;
        int own_stack = (BYTE *)&info >= (BYTE *)info.BaseAddress && (BYTE *)&info < end;  // holds `from` too
        if (!own_stack && info.RegionSize <= MAX_REGION && info.State == MEM_COMMIT && info.Type == MEM_PRIVATE &&
            info.Protect == PAGE_READWRITE) {
            __try {
                for (UINT64 *q = (UINT64 *)info.BaseAddress; (BYTE *)(q + 1) <= end; q++)
                    if (*q == from) { *q = to; swapped++; }
            } __except (EXCEPTION_EXECUTE_HANDLER) {}
        }
        if (end <= at) break;
        at = end;
    }
    return swapped;
}

// A read-only copy of `slots` qwords of a vtable with two slots replaced; clamps to mapped memory.
static UINT64 *copy_vtable(UINT64 *vtable, int slots, int slot_a, void *hook_a, int slot_b, void *hook_b) {
    MEMORY_BASIC_INFORMATION info;
    if (VirtualQuery(vtable, &info, sizeof info) != sizeof info) return NULL;
    SIZE_T room = ((BYTE *)info.BaseAddress + info.RegionSize - (BYTE *)vtable) / sizeof *vtable;
    if ((SIZE_T)slots > room) slots = (int)room;
    if (slot_a >= slots || slot_b >= slots) return NULL;
    UINT64 *copy = VirtualAlloc(NULL, slots * sizeof *copy, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if (!copy) return NULL;
    memcpy(copy, vtable, slots * sizeof *copy);
    copy[slot_a] = (UINT64)hook_a;
    copy[slot_b] = (UINT64)hook_b;
    DWORD old;
    VirtualProtect(copy, slots * sizeof *copy, PAGE_READONLY, &old);
    return copy;
}

// ---- capture ----
static void put_bmp(const char *path, const BYTE *bgra, int w, int h) {
    char tmp[MAX_PATH + 8];
    snprintf(tmp, sizeof tmp, "%s.tmp", path);
    HANDLE f = CreateFileA(tmp, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS, 0, NULL);
    if (f == INVALID_HANDLE_VALUE) { note("cannot create %s (%u)", tmp, GetLastError()); return; }
    DWORD size = (DWORD)w * h * 4, written;
    BITMAPFILEHEADER fh = {0x4D42, 14 + 40 + size, 0, 0, 14 + 40};
    BITMAPINFOHEADER ih = {40, w, -h, 1, 32, BI_RGB, size, 0, 0, 0, 0};  // negative height = top-down
    int ok = WriteFile(f, &fh, 14, &written, NULL) && WriteFile(f, &ih, 40, &written, NULL) &&
             WriteFile(f, bgra, size, &written, NULL) && written == size;
    CloseHandle(f);
    if (!ok || !MoveFileExA(tmp, path, MOVEFILE_REPLACE_EXISTING)) {
        note("write/rename failed for %s (%u)", path, GetLastError());
        DeleteFileA(tmp);
    }
}

static void capture(void *chain, const char *path) {
    ID3D11Texture2D *back = NULL, *stage = NULL, *resolved = NULL;
    ID3D11Device *dev = NULL;
    ID3D11DeviceContext *ctx = NULL;
    BYTE *out = NULL;
    D3D11_TEXTURE2D_DESC desc;
    D3D11_MAPPED_SUBRESOURCE map;
    int mapped = 0;
    if (FAILED(IDXGISwapChain_GetBuffer((IDXGISwapChain *)chain, 0, &IID_ID3D11Texture2D, (void **)&back))) {
        note("GetBuffer failed");
        return;
    }
    ID3D11Texture2D_GetDevice(back, &dev);
    if (!dev) { note("GetDevice failed"); goto done; }
    ID3D11Device_GetImmediateContext(dev, &ctx);
    ID3D11Texture2D_GetDesc(back, &desc);
    int fmt = desc.Format;
    if (fmt != DXGI_FORMAT_B8G8R8A8_UNORM && fmt != DXGI_FORMAT_B8G8R8A8_UNORM_SRGB &&
        fmt != DXGI_FORMAT_B8G8R8X8_UNORM && fmt != DXGI_FORMAT_B8G8R8X8_UNORM_SRGB &&
        fmt != DXGI_FORMAT_R8G8B8A8_UNORM && fmt != DXGI_FORMAT_R8G8B8A8_UNORM_SRGB &&
        fmt != DXGI_FORMAT_R10G10B10A2_UNORM) {
        note("unsupported format %d", fmt);
        goto done;
    }
    ID3D11Resource *source = (ID3D11Resource *)back;
    if (desc.SampleDesc.Count > 1) {
        D3D11_TEXTURE2D_DESC rd = desc;
        rd.Usage = D3D11_USAGE_DEFAULT; rd.BindFlags = 0; rd.CPUAccessFlags = 0; rd.MiscFlags = 0;
        rd.SampleDesc.Count = 1; rd.SampleDesc.Quality = 0;
        if (FAILED(ID3D11Device_CreateTexture2D(dev, &rd, NULL, &resolved))) { note("resolve texture failed"); goto done; }
        ID3D11DeviceContext_ResolveSubresource(ctx, (ID3D11Resource *)resolved, 0, source, 0, desc.Format);
        source = (ID3D11Resource *)resolved;
    }
    D3D11_TEXTURE2D_DESC sd = desc;
    sd.Usage = D3D11_USAGE_STAGING; sd.BindFlags = 0; sd.CPUAccessFlags = D3D11_CPU_ACCESS_READ; sd.MiscFlags = 0;
    sd.SampleDesc.Count = 1; sd.SampleDesc.Quality = 0;
    if (FAILED(ID3D11Device_CreateTexture2D(dev, &sd, NULL, &stage))) { note("staging texture failed"); goto done; }
    ID3D11DeviceContext_CopyResource(ctx, (ID3D11Resource *)stage, source);
    if (FAILED(ID3D11DeviceContext_Map(ctx, (ID3D11Resource *)stage, 0, D3D11_MAP_READ, 0, &map))) { note("Map failed"); goto done; }
    mapped = 1;
    UINT w = desc.Width, h = desc.Height;
    out = HeapAlloc(GetProcessHeap(), 0, (SIZE_T)w * h * 4);
    if (!out) { note("out of memory"); goto done; }
    for (UINT y = 0; y < h; y++) {
        const BYTE *s = (const BYTE *)map.pData + (SIZE_T)y * map.RowPitch;
        BYTE *d = out + (SIZE_T)y * w * 4;
        for (UINT x = 0; x < w; x++, s += 4, d += 4) {
            if (fmt == DXGI_FORMAT_R10G10B10A2_UNORM) {
                UINT v = *(const UINT *)s;
                d[2] = (BYTE)((v & 0x3FF) >> 2); d[1] = (BYTE)(((v >> 10) & 0x3FF) >> 2); d[0] = (BYTE)(((v >> 20) & 0x3FF) >> 2);
            } else if (fmt == DXGI_FORMAT_R8G8B8A8_UNORM || fmt == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB) {
                d[0] = s[2]; d[1] = s[1]; d[2] = s[0];
            } else {
                d[0] = s[0]; d[1] = s[1]; d[2] = s[2];
            }
            d[3] = 255;
        }
    }
    ID3D11DeviceContext_Unmap(ctx, (ID3D11Resource *)stage, 0);
    mapped = 0;
    put_bmp(path, out, (int)w, (int)h);
    note("shot %s %ux%u fmt=%d", path, w, h, fmt);
done:
    if (mapped) ID3D11DeviceContext_Unmap(ctx, (ID3D11Resource *)stage, 0);
    if (out) HeapFree(GetProcessHeap(), 0, out);
    if (stage) ID3D11Texture2D_Release(stage);
    if (resolved) ID3D11Texture2D_Release(resolved);
    if (ctx) ID3D11DeviceContext_Release(ctx);
    if (dev) ID3D11Device_Release(dev);
    ID3D11Texture2D_Release(back);
}

static void maybe_capture(void *chain) {
    if (InterlockedCompareExchange(&req_state, 2, 1) != 1) return;
    __try {
        capture(chain, req_path);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        note("exception %X in capture", GetExceptionCode());
    }
    InterlockedExchange(&req_state, 0);
}

static HRESULT STDMETHODCALLTYPE present_hook(void *chain, UINT interval, UINT flags) {
    maybe_capture(chain);
    return present_orig(chain, interval, flags);
}

static HRESULT STDMETHODCALLTYPE present1_hook(void *chain, UINT interval, UINT flags, const void *params) {
    maybe_capture(chain);
    return present1_orig(chain, interval, flags, params);
}

// ---- worker ----
// The vtable pointer of IDXGISwapChain as dxgi.dll hands it out; the dummy objects are gone before the scan.
static UINT64 *dummy_vtable(void) {
    UINT64 *vtable = NULL;
    HWND wnd = CreateWindowExA(0, "STATIC", "frame_grab", WS_POPUP, 0, 0, 64, 64, NULL, NULL, NULL, NULL);
    if (!wnd) return NULL;
    DXGI_SWAP_CHAIN_DESC sd = {0};
    sd.BufferDesc.Width = 64; sd.BufferDesc.Height = 64; sd.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    sd.SampleDesc.Count = 1;
    sd.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    sd.BufferCount = 1;
    sd.OutputWindow = wnd;
    sd.Windowed = TRUE;
    sd.SwapEffect = DXGI_SWAP_EFFECT_DISCARD;
    IDXGISwapChain *chain = NULL;
    ID3D11Device *dev = NULL;
    ID3D11DeviceContext *ctx = NULL;
    HRESULT hr = D3D11CreateDeviceAndSwapChain(NULL, D3D_DRIVER_TYPE_HARDWARE, NULL, 0, NULL, 0, D3D11_SDK_VERSION,
                                               &sd, &chain, &dev, NULL, &ctx);
    if (FAILED(hr))
        hr = D3D11CreateDeviceAndSwapChain(NULL, D3D_DRIVER_TYPE_WARP, NULL, 0, NULL, 0, D3D11_SDK_VERSION, &sd,
                                           &chain, &dev, NULL, &ctx);
    if (SUCCEEDED(hr) && chain) vtable = *(UINT64 **)chain;
    else note("dummy swap chain failed 0x%X", hr);
    if (ctx) ID3D11DeviceContext_Release(ctx);
    if (chain) IDXGISwapChain_Release(chain);
    if (dev) ID3D11Device_Release(dev);
    DestroyWindow(wnd);
    return vtable;
}

static void poll_request(void) {
    HANDLE f = CreateFileA(req_file, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_EXISTING, 0, NULL);
    if (f == INVALID_HANDLE_VALUE) return;
    char text[MAX_PATH + 2];
    DWORD got = 0;
    ReadFile(f, text, MAX_PATH, &got, NULL);
    CloseHandle(f);
    text[got] = 0;
    char *eol = text + strcspn(text, "\r\n");
    *eol = 0;
    if (!text[0]) return;  // still being written: try again next tick
    DeleteFileA(req_file);
    lstrcpyA(req_path, text);
    InterlockedExchange(&req_state, 1);
}

static DWORD WINAPI run(LPVOID unused) {
    HMODULE self;
    GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                       (LPCSTR)run, &self);
    DWORD n = GetModuleFileNameA(self, log_path, MAX_PATH);
    if (n < 4) return 0;
    lstrcpyA(req_file, log_path);
    lstrcpyA(log_path + n - 4, ".log");
    lstrcpyA(req_file + n - 4, ".req");
    note("frame_grab: pid %u", GetCurrentProcessId());

    UINT64 *vtable = dummy_vtable();
    UINT64 *copy = NULL;
    if (vtable) {
        note("IDXGISwapChain vtable %p, Present %p, Present1 %p", vtable, (void *)vtable[SLOT_PRESENT],
             (void *)vtable[SLOT_PRESENT1]);
        present_orig = (present_fn)vtable[SLOT_PRESENT];
        present1_orig = (present1_fn)vtable[SLOT_PRESENT1];
        copy = copy_vtable(vtable, VTABLE_SLOTS, SLOT_PRESENT, present_hook, SLOT_PRESENT1, present1_hook);
        if (!copy) note("vtable copy failed");
    }
    DWORD start = GetTickCount(), last_scan = 0, pending_since = 0;
    for (;;) {
        DWORD now = GetTickCount();
        if (copy && (!last_scan || now - last_scan >= (now - start < 300000 ? 2000u : 10000u))) {
            int swapped = swap_pointers((UINT64)vtable, (UINT64)copy);
            last_scan = GetTickCount();
            if (swapped) note("swapped %d swap chain object(s)", swapped);
        }
        if (req_state == 0) {
            poll_request();
            if (req_state == 1) pending_since = now;
        } else if (req_state == 1 && now - pending_since >= WAIT_MS) {
            if (InterlockedCompareExchange(&req_state, 0, 1) == 1) note("no present within 3s");
        }
        drain();
        Sleep(100);
    }
}

BOOL WINAPI DllMain(HINSTANCE inst, DWORD reason, LPVOID reserved) {
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(inst);
        HANDLE t = CreateThread(NULL, 0, run, NULL, 0, NULL);
        if (t) CloseHandle(t);
    }
    return TRUE;
}
