# Changelog

## 2026-11-10

- `client_feed` (in `ow_fixes.asi`) also sends Shadow Step's marker while its aim is open (research/lizrollin-runtime.md section 36): the targeting state items are found by their class vtable (`img+0x261E920`) on a thread of their own, and each frame the active one's place (+0x350) and good flag (+0x374) go to the lobby as a 17-byte datagram (`client_sync.MARKER`). `scan_for` is the scan both kinds of object share. Self-check cases in `test_client_feed.c`.

## 2026-11-09

- `tracer.asi` logs config-var evaluators by class: `classeval <class> graph= node= ok= kind= value=`, once per distinct (class, graph, node, result) and second.
  - Every evaluator object of a listed class (`class_evals`: its delegate vtable RVA) gets pointed at a vtable copy whose slot 4, the evaluator, logs its result.
  - Listed: `A3DEF368` (the game mode entity: the kill feed message's target), `CD7F3617` (an entity's name), and the natives of 0DEA's kill-feed gate (`A77AC2AC`, `0D179CAF`, `33C5B1DE`, `CEFF290A`), `F76C486C` / `AECBBA41`, which only 0DEA's line path reads, and `016D65A8` / `F4C77F4A`, which set #1962, the condition of the feed's presenter (research/lizrollin-runtime.md section 16).
  - `system 9: ...` whenever client system 9 changes: its vtable, `+168`, `+216`, the 16-byte `+224` and the bytes `+0x2B0..+0x2F7` (`+700` = `+0x2BC` decides native `016D65A8`, the kill feed panel's condition; `+753` = `+0x2F1`).
  - The rescan swaps every pointer kind in one pass (`swap_pointers` takes a table), with an image-range test first.

## 2026-11-08

- Added `ow_fixes.asi`: every client fix in one plugin, one module per source file, each started on its own thread by `ow_fixes.c`. Modules: `retail_menu` (`retail_menu.c`), `aspect_ratio` (`aspect_ratio.c`) and the new `client_feed` (`client_feed.c`). Launch with `--asi tools/asi/build/ow_fixes.asi`. Research probes stay separate plugins.
  - `retail_menu.asi` and `aspect_ratio.asi` are no longer built; `build.bat` deletes old copies, which would hook a second time next to `ow_fixes.asi`. `retail_menu.c` no longer loads `aspect_ratio.asi`.
  - Shared files: one log `build/ow_fixes.log` (`HH:MM:SS.mmm module text`, each line one append-only write so the modules' threads never overwrite each other), settings `build/ow_fixes.ini` (the existing `aspect_ratio.ini` was moved there, `Aspect32x9=1` kept), login signal `build/ow_fixes.go`. Helpers `fix_log` / `fix_path` in `fixes.h`.
  - `client_feed`: the hitbox-drift feeder, moved from `tools/research/client_position.py --feed` into the client. With `OW_CLIENT_SYNC=PORT` in the game's environment it reads the client's mover in-process and sends every new `(frame, feet, slot)` to the lobby's `client_sync.py`. Same slot logic as the Python reader (neighbours first, scans at most every 10 s, 2 s stale rule, two equal copies per sample). Neighbour probes skip memory that is not committed read-write (no guard-page trips), and the scan skips its own stack. Polls every 0.5 ms (high-resolution waitable timer).
  - `build.bat` runs the new `test_client_feed.c` (the cases of `test_client_position.py`, plus guard page and stack copy) before building. All three self-checks pass.
  - Live, Practice Range as Soldier, `ow_fixes.asi` only: all three modules logged (login bypass, Alt+Tab, menu skin, 32:9, config-var redirects), the feed found the body after 7 in-process scans, and on a 63.5 m walk (strafes, jumps, backpedals) the client-vs-server drift at the same command frame was at most 0.001 m over 54 samples at 2 Hz (the Python feeder: up to 4.3 cm while moving, from a one-frame lag).

- `retail_menu.asi` now loads `aspect_ratio.asi` from its own folder at startup, so the 32:9 choice appears with the usual launch command (it was opt-in through a second `--asi`, which ordinary launches omitted). Missing plugin: logged, native three choices remain.

- `build.bat` now compiles and runs `test_retail_menu.c` before building `retail_menu.asi` (it covered the login bypass but never ran). Both C self-checks pass.

- Added opt-in `aspect_ratio.asi`: native 32:9 aspect choice, separate persisted selection, matching color/depth/canvas sizing and horizontal expansion with vertical FOV preserved. Existing ratios remain available. Build includes native-boundary checks for sizing, allocation rollback/retry, FOV policy/restore and UI values. [Live evidence](../../research/aspect-ratio-32-9/REPORT.md).
- `retail_menu.asi` restores the native menu visibility publisher (`0134`, entity gate8646), retaining main-screen input29323 and message062C's show/hide stack. Removed the initial permanent1962 seed after a distant menu actor was reproduced in Training. Hero Gallery and gameplay returns are verified with the rebuilt plugin ([evidence](../../research/main-menu-normal/REPORT.md)).
- The menu Showcase chooses a cached random owned skin through the client's native catalog and SkinUnlock converter. Unavailable themes/non-skins are skipped; explicit previews and equipped Gallery/gameplay loadouts are retained. Native-boundary checks, a production build and352 Python checks pass ([native route](../../research/main-menu-normal/owned-skins.md)).

## 2026-11-07

- `retail_menu.asi`: replaced automatic START / ENTER GAME events with startup graph `0194`'s existing bypass branch. A guarded dynamic-variable evaluator calls the native address/login APIs with `Player`, then lets the graph skip creation of both screens. The launcher now injects immediately. Fresh launch reached the menu, Training and Practice Range with Pod/Name state slots null; native-boundary MSVC self-check and 251 lobby tests pass ([research](../../research/login-bypass-2026-11-07.md)).
- `tracer.asi`: when a beam state (`STU_9932D728`, vtable `0x7FF6D384D1A0`) begins, each config-var evaluator its descriptor points at gets a vtable copy whose evaluate slot (+0x20) logs the result: `eval beam=... off=<descriptor offset> kind=<value kind> value=...` (6 lines per evaluator per beam). Evaluators are shared per class, so `off` is where one was first seen. Found that Mercy's heal beam's owner (+0xF8, var27) was the healed bot itself.

## 2026-11-06

- `tracer.asi`: begin / end lines also print the state object (`item=`), for watching one state with `guard_probe.asi`.
- `guard_probe.asi`: the `.addr` file takes an optional hex length (`ADDRESS LENGTH`); only accesses inside that range are logged and counted, the rest of the page re-arms silently. The hit cap is 4000 (under the ring size), the ring is read from the current head each round and drained after disarming (before, a fast round lost all its hits). Used to find Zarya's zero-length beam ray (research/asi-tracer.md).
- `dev_console.asi`: applies `build/dev_console.set` (`name value` per line) whenever the file changes, writing the live debug cvars `firstPerson.DetachModelFlag`, `firstPerson.Offset{X,Y,Z,Yaw,Pitch,Roll}` (metres, degrees), `camera.EnableUpdate` and `anim.ShowBindPose`. Checked in the Range: +Z pushes the first-person arms forward, +X left, +Y up. With yaw 180 and Z 1.9 the arms and rifle are seen from the front while fire and reload play (`research/dev-console/fp-*.png`).
- Added `dev_console.asi` (opt-in, research): keeps the developer console's gate bytes at 1 (`0x7FF6D404F640` tank_Console, `0x7FF6D404F4B0` tournament admin, `0x7FF6D404F5C8` dev gate). Logs the console object and its open flag, and dumps the console command table the first time the console opens. Open the console with Alt+` (foreground key). Checked in game: the console opens at the main menu with a "DEV ENABLED" badge, and the table holds one command, `runCommand` (`research/dev-console.md`).
- `build.bat` also builds `dev_console.asi` (links user32).

- Added `frame_grab.asi` (opt-in): saves the D3D11 back buffer as a BMP right before Present, on request via `build/frame_grab.req` (first line = output path). Vtable copy of `IDXGISwapChain` (Present slot 8, Present1 slot 22, pointer taken from a dummy swap chain, swapped in private memory like `tracer.asi`). Tested in the Practice Range: the game's swap chain shares the dummy's vtable (1 object swapped), back buffer format 28 (R8G8B8A8_UNORM), 1274x720; 11 shots in a row, the game kept running. The frame has no overlay (third-party overlays draw after Present).
- `build.bat` also builds `frame_grab.asi` (links user32, d3d11, dxgi, dxguid).
- Added `tracer.asi`: logs every server message (one vtable copy of the JamMessage delegate class, `0x7FF6D38368D8`, slot 2), every statescript state begin / end (script system vtable `0x7FF6D384C010`, slots 32 / 35, called by the state activation and deactivation with (system, instance, item)) and the bits each state reads from a frame (slot 43 of the state's class vtable; the item gets a per-class vtable copy, installed in the begin hook or when a system is found). Log `build/tracer.log`. Checked in game on Uprising: 20301 / 20302 / 20306 logged, the hero and director graphs' reads with absolute bit positions. Two crashes on the way: the first build copied 64 slots of each state class vtable (a class had more; 256 now, clamped to the mapped region), and a full rescan took 45 s on a loaded game (regions over 64 MB are skipped now, about 0.4 s a pass).
- Added `guard_probe.asi`: a data watchpoint by `PAGE_GUARD` + vectored exception handler + trap-flag single step, re-armed per hit, address from `build/guard_probe.addr`. Checked in game: 300 hits on a statescript instance page, no crash, so watchpoints are possible on this client after all (the debug-register routes crash it).
- `build.bat` also builds `tracer.asi` and `guard_probe.asi`.

## 2026-11-05

- `retail_menu.asi`: removes the WH_KEYBOARD_LL hook that tournament and show mode install (`0x7FF6D2B0B410`, handle at RVA `0x3285B00`), which swallowed Alt+Tab, Alt+Esc, Ctrl+Esc, Alt+F4, Alt+Space and the Windows keys. One `UnhookWindowsHookEx` call once the handle appears (`research/tournament-input-lock.md`).

## 2026-11-05

- Added `helix_probe.asi` (user-approved 2026-11-05; breakpoints allowed since 2026-11-04, tasks/roadmap.md): in-process hardware execution breakpoints on the projectile shot path (executor job queue, shot builder, creation entry, creation return), caught by a vectored exception handler and logged to `build/helix_probe.log` with the creation status code. It re-arms every thread's debug registers every 0.5 s and logs how many it had to rewrite. It writes no game code or data. Runs 600 s, then disarms. Load it into a running game: `python tools/asi/load_asi.py PID tools/asi/build/helix_probe.asi`. Result: the client crashes (`0xC0000005`) within about 1 s of its threads being armed, also in the control mode (`build/helix_probe.dummy`: breakpoints on a function no game thread runs), so in-process hardware breakpoints are unusable too (research/soldier-runtime/helix-visual-findings.md).
- `build.bat` also builds `helix_probe.asi`.

## 2026-11-04

- `retail_menu.asi`: skips the Pod and Name screens. Both are native screens. Each gets a vtable copy whose per-frame slot 58 sends the screen's own START / ENTER GAME event once, 30 frames after the screen appears. The Name screen is filled with `Player` first. Checked in game: boot to the retail main menu with no input (`research/login-skip.md`, `research/main-menu/login-skip-main-menu.png`). The first build copied 64 slots and crashed on slot 64; 65 are copied now.

## 2026-11-03

- `retail_menu.asi`: also forces `STU_DEA278FD`, `STU_74BA7BFC` and `STU_ECD42997` true: the PLAY screen's card gates, each a cvar byte that is on by default and off in tournament mode (found with `.scratchpad/cfgeval.py` on the runtime snapshot). Checked in game: PLAY shows Quick Play, Arcade, Competitive Play and Game Browser (unlocked). Quick Play opens its role select (roles do not toggle), Arcade opens empty, Game Browser opens an empty list with CREATE (`research/main-menu/retail-play-cards.png`, `retail-quick-play-roles.png`, `retail-arcade-empty.png`, `retail-game-browser.png`).
- `retail_menu.asi`: forces `STU_2B33EE38` (tournament flag) false, `STU_0CFBAF2D` (TRAINING enable) and `STU_27FF1349` (Practice vs. AI card) true, by giving their runtime instances vtable copies with a new evaluator; logs the game's own values. Waits for `retail_menu.go` (login). Checked in game: TRAINING opens Tutorial / Practice Range / Practice vs. AI, the difficulty screen queues, and the lobby's handoff loads "VERSUS AI EASY" / "VERSUS AI HARD" on Hanamura (`research/main-menu/retail-*.png`).
- `load_asi.py`: loads plugins into the running game (remote `LoadLibraryW`).
- `build.bat`: MSVC 2022 x64 build to `build/`.
