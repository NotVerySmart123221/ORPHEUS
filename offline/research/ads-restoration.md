# Aim-down-sights in retail build 104319

The showcase shows Ashe aiming through The Viper's sights at 0:51–0:54, Ana's blue scope at 0:07–0:12, and Widowmaker's charged scope at 23:50–23:55. Earlier retail captures kept the hip view when RMB was held. [Chapter comparison](hero-showcase-comparison.md)

Weapon graphs `14FC` (Ashe), `0B94` (Ana), and `001B` (Widowmaker) share LogicalButton 29, requested aim `v2137`, scope progress `v169`, progress DataFlow `048000000000006D`, and camera class `STU_75D1C308`. Ashe's camera reads progress and the graph Entry parameter `v172=40`. Ana and Widowmaker read `v168`, which their fully scoped Stacks copy from Entry parameter `v172=30`. In IDALib, the retail camera state's recompute path `0x7FF6D1B36B00` multiplies its angle inputs by `0.0174532924` before applying them.

| Hero | Owner input gate, hold branch | Camera owner states | Scope-progress and request presence bits | Scoped Volley owner state |
| --- | --- | --- | --- | --- |
| Ashe | 58=0, 47=0, 48=LogicalButton | 6, 83 | 11, 22 | 113 |
| Ana | 64=0, 51=0, 50=LogicalButton | 23 | 7, 15 | 43 |
| Widowmaker | 81=0, 69=0, 70=LogicalButton | 7 | 4, 16 | 9 |

Ashe's input-only probe accepted RMB (`0x02`), combined fire (`0x03`), and release. All three shots decremented the magazine. The view stayed at hip through the hold and shot. Capture median interval 29.7 ms. [Frames](../.scratchpad/retail-ads/Ashe-button.zip)

The first camera probe crashed as the client activated the camera state. The crash stack maps to camera callback `0x7FF6D1B32B10` and native StateRef reader `0x7FF6D2D937D0`. An empty camera payload caused the reader to consume the next state's branch bit as a reference and dereference a missing instance at `+0x1F8`. Encode a zero bit for an unlinked camera state. The camera's optional reference is separate from the BooleanSwitch payload. [Retail run](main-menu/20261104T195710.460484Z/)

The corrected camera payload produced the following held-RMB behavior. Each clip includes a hip shot, a shot with RMB held, release, and a second hip shot. Median capture interval was 16.6 ms. The progress DataFlow without the camera states left Ashe's view at hip. [Retail log](main-menu/20261104T200646.926459Z/lobby.log) · [Captures](../.scratchpad/retail-ads-corrected/)

| Hero | Camera zoom visible by | Hip view visible after release by | Presentation compared with the showcase |
| --- | --- | --- | --- |
| Ashe | +0.085 s | +0.087 s | The rifle stays off center. The showcase centers the iron sights. Sending Ashe's scope-in and scope-out Anim aliases alongside ChaseVar did not center the sights in the trial. |
| Ana | +0.084 s | +0.087 s | The scope ring appears and clears. The rifle remains visible in the zoomed view. |
| Widowmaker | +0.083 s | +0.070 s | The scope lens unfolds, then folds back after release. The charged scope overlay is absent. |

Production uses the tested hold input, camera and progress states, with the scope suppressed during reload. The shot server still uses the primary weapon model for cadence, ammo and damage. Isolated scoped Volley probes exercised the candidate wire states and registered shots, but did not establish the correct recoil, cadence, or charge. The showcase timing is a reference for the remaining transitions. The current camera updates apply at the requested endpoints.

The standard lobby reproduced entry, shots with RMB held, and exit on all three heroes at 58.0–58.1 fps. [Production captures](anim/ads-production.jpg) · [Retail input and shot log](main-menu/20261104T201353.866915Z/lobby.log)

Common graph 75 state 23 now notifies the native animator to reread category `81` while scoped and clear it on release or reload. The standard lobby capture at 60.0–60.1 fps shows Ashe’s sights centered by +0.39 s, Ana’s rifle clear of the scope by +0.19 s, and Widowmaker’s rifle clear of the scope by +0.19 s. All three fire with RMB held and return to the hip pose on release. Widowmaker’s charged overlay remains absent. [Production captures](anim/ads-notify-production-60fps.jpg) · [Retail log](main-menu/20261104T232416.645586Z/lobby.log)

Widowmaker's weapon graph `001B` starts client-only presenter state 95, which reads the parent presenter through v11821 and selector `6694`. Body graph `0076` creates `HUD_Widowmaker` at state 6. Keeping `ScriptRef(118, 6)` across weapon updates preserves the parent binding. Entity Boolean v1032 controls scope visibility and instance v174 in slot 7 supplies charge from 1 to 10.

The standard retail build captured the meter at 36% before the early shot and 0% after it, then at 100% before the second shot and 0% after it. RMB release returned to hip view. The capture ran at 60.0 fps with a maximum gap of 32.2 ms. This matches the charge and shot reset sequence in the showcase at 23:50–23:55. The lobby still uses its existing rifle damage and ammo behavior. [Retail capture](anim/widow-charge-production-60fps.jpg) · [Run log](main-menu/20261105T002042.871041Z/lobby.log)
