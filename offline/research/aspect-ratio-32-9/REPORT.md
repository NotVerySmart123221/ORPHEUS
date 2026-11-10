# 32:9 aspect-ratio implementation

2026-11-08. Overwatch 1.74.0.0.104319, owned offline client.

Implemented opt-in [`aspect_ratio.asi`](../../tools/asi/aspect_ratio.c). **Options → Video → Aspect Ratio → 32:9** shows more world horizontally while preserving vertical FOV. Native resolution detection and all three existing aspect choices remain available. Selection persists in the plugin's adjacent `.ini`.

## Live verification

At the same Practice Range spawn position and viewing direction, Soldier's world camera measured:

| Measurement | Native 16:9 | 32:9 |
|---|---:|---:|
| Fitted windowed viewport | 2519×1417 | 5038×1417 |
| Horizontal FOV | 102.99754° | 136.62106° |
| Vertical FOV | 70.53280° | 70.53280° |
| Projection horizontal scale | −0.79547095 | −0.39773548 |
| Projection vertical scale | 1.41410816 | 1.41410816 |

The horizontal scale halves while vertical scale stays identical. Native 16:9 is restored when switching back. The first-person weapon camera also preserved its 50° vertical FOV, with horizontal projection scale changing from −1.20633709 to −0.60316855.

![Native 16:9 at the same pose](range-16-9.png)

![32:9 reveals additional lateral scenery](range-32-9.png)

Additional live checks passed: saved 32:9 on fresh launch; closing/reopening Video settings; native 16:9, 16:10 and 21:9 selections; returning to 32:9; menu and HUD rendering; pointer hit testing; resizing a window; borderless 5120×1440; and windowed/borderless transitions. A maximized 5120×1417 client fits 5038×1417, leaving small side margins. Borderless 5120×1440 fills the display.

![Saved fourth choice in native Video settings](graphics-borderless-32-9.png)

## Native paths used

Addresses are RVAs in this pinned build, relocated against the running image base.

| Purpose | RVA / location |
|---|---|
| Aspect UI variable | `0x0D800000000065CA` |
| Graphics controller vtable | `0x264C3A0`, 69 slots |
| Arrow-selector vtable | `0x27C3958` |
| UI queue / enter scope / exit scope | `0x1C648A0` / `0x1C57EA0` / `0x1C58540` |
| Render-target allocation / validation / release | `0x19FA160` / `0x1A0BA50` / `0x1A002C0` |
| Native intermediate-target predicate | `0x1A0B190` |
| Native depth release / recreation | `0x1A084E0` / `0x1A044A0` |
| Native render-canvas resize | `0x1A163F0` |
| Camera viewport/projection rebuild | `0x1A764B0` |
| Base / player camera vtables | `0x27A9100` / `0x26135B0`, six slots each |

The fourth literal label is appended through the native ItemCollection API on the serialized UI worker, inside a valid native UI scope. GetItems/GetAt values are borrowed; only newly owned values are released. Freed selector instances are skipped, and subsequent carousels trigger another bounded update.

The plugin retains native 16:9 backing flags for 32:9, avoiding the native 21:9 vertical crop. It tracks modified full-view perspective cameras, applies the vertical-preserving projection policy after native updates, and restores native dimensions when disabling the new mode. Orthographic and partial-view cameras are excluded.

Successful non-test Present/Present1 calls provide the render boundary. Color, depth and canvas dimensions must agree. An intermediate color target is created only when the native predicate requires one; at full-size/default color settings, rendering uses the existing back buffer. The predicate also retains native gamma/brightness/contrast behavior.

Matching depth was essential: the prototype initially widened the color target while retaining native 16:9 depth, producing broken UI. Native depth recreation fixed that defect. Direct3D requires matching render-target/depth dimensions. [Microsoft target-binding requirements](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11devicecontext-omsetrendertargets).

Native borderless changes temporarily queue 0×0, floored to 8×8. Repair skips these states and pending resize/reset work. Apply preserves an existing native resize request; overwriting its valid coordinates with −1 prevented the native desktop-size refresh in an earlier prototype. Positive presentation status is also excluded: `SUCCEEDED` includes `DXGI_STATUS_OCCLUDED`. [Microsoft Present return values](https://learn.microsoft.com/en-us/windows/win32/api/dxgi/nf-dxgi-idxgiswapchain-present).

Executable pages and renderer/application root vtables are retained. Earlier root-table prototypes triggered native table-range guards; those approaches were removed. Runtime copies cover only the COM presentation interface, graphics controller and camera classes. Existing executable-page protection changes were unsuccessful and are not required by the final implementation.

## Build and checks

`tools/asi/build.bat` builds the plugin and runs [`test_aspect_ratio.c`](../../tools/asi/test_aspect_ratio.c). MSVC x64 builds pass with `/W4` for this plugin and its tests.

Final `build/aspect_ratio.asi`: 159,744 bytes; SHA-256 `56252222d12a73d99046bb2396e461b589b3fe28c7e99bc8287fc58197130f7d`.

Checks cover integer fitting and invalid sizes; one-shot allocation; color-allocation rollback; canvas failure/retry; direct-backbuffer reuse; occluded/test presents; transient resize/reset states; vertical FOV policy; restoring modified cameras before queued native resize commits; a mode change during a camera callback; UI value lifetimes; bounded property lookup; and preserving native borderless resize requests during Apply.

## Limits

Validated on this executable/build and current D3D11 system. New camera/controller instances can take up to two seconds to be discovered. No exhaustive testing of every hero, scope/ADS, spectator, cinematic, map-culling case, exclusive fullscreen, alternate graphics driver, or actual GPU out-of-memory condition. Native defaults/revert routes were inspected; a full GUI reset of existing user graphics settings was deliberately not performed.
