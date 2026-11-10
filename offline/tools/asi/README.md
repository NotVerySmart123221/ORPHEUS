# ASI plugins

Opt-in client plugins (DLLs) for Overwatch 1.74.0.0.104319. They change client code at run time, never game files.

## ow_fixes.asi

Every client fix in one plugin. Each fix is a module in its own source file with its own thread; `ow_fixes.c` lists them. Research probes (below) stay separate opt-in plugins.

```bash
python tools/lobby/run.py <game-root> --asi tools/asi/build/ow_fixes.asi
```

| Module | Source | What it fixes |
|---|---|---|
| retail_menu | `retail_menu.c` | Retail menus on the tournament route, Pod/Name login bypass, Alt+Tab, random owned menu skins |
| aspect_ratio | `aspect_ratio.c` | 32:9 aspect choice |
| client_feed | `client_feed.c` | Hitbox drift: sends the client's own position to the lobby (`OW_CLIENT_SYNC`) |

Shared files in `build/`: `ow_fixes.log` (one line per event, `HH:MM:SS.mmm module text`; all modules append to it), `ow_fixes.ini` (settings), `ow_fixes.go` (`run.py` creates it after the login).

To add a fix: write `<fix>.c` with `DWORD WINAPI <fix>_run(LPVOID)`, include `fixes.h` for `fix_log` / `fix_path`, add the function to `modules[]` in `ow_fixes.c` and the file to the `ow_fixes.asi` line in `build.bat`. A self-check `test_<fix>.c` can `#include "<fix>.c"`; `build.bat` runs the self-checks before it builds the plugin.

`retail_menu.asi` and `aspect_ratio.asi` are gone (merged on 2026-11-08); `build.bat` deletes old copies so they cannot hook a second time next to `ow_fixes.asi`.

### retail_menu

Shows the retail menus on the tournament route: the full main menu, TRAINING enabled, the Training screen's own Practice vs. AI card with its EASY / MEDIUM / HARD choice, and the PLAY screen's Quick Play, Arcade, Competitive Play and Game Browser cards (they open, but the lobby does not serve their queues, arcade list or game list yet). It makes six Bool config vars report fixed values:

| Config var class | What reads it | Game value | Forced |
|---|---|---|---|
| `STU_2B33EE38` | `--tank_TournamentMode`: PLAY layout (graph `0x0B7C`), TRAINING enable | 1 | 0 |
| `STU_0CFBAF2D` | TRAINING enable (graph `0x1214`, `A && !B && !C && !tournament`) | 0 | 1 |
| `STU_27FF1349` | Training's Practice vs. AI card (graph `0x0B7F`) | 0 | 1 |
| `STU_DEA278FD` | PLAY cards (graph `0x0B7C`, presenters 339 and 814): cvar byte `0x7FF6D4052680` | 0 | 1 |
| `STU_74BA7BFC` | PLAY card (presenter 604): cvar byte `0x7FF6D40528B0` | 0 | 1 |
| `STU_ECD42997` | PLAY card (presenter 664): cvar byte `0x7FF6D4053790` | 0 | 1 |

Details and how they were found: `research/play-menu.md`.

With the lobby's default `--event none`, ordinary backdrops show a random hero and owned skin. On menu UI graph `0134`, the plugin enables the offline Showcase publisher gate (entity variable `8646`). The graph's native main-screen input `29323` controls message `062C`, which shows/hides the Showcase through its existing variable stack. Gallery and Training therefore hide the menu model; returning to the main menu shows it again. The earlier permanent visibility seed was removed after a distant menu actor was found in Training.

On Showcase graph `0B4A`, an empty hero/skin input pair gets a cached random skin from the selected hero's native owned-unlock catalog. Unavailable assets and non-skins are skipped. Explicit previews, equipped Gallery/gameplay skins and ownership data stay untouched. Native fallback remains when no candidate is available. [Verification and limits](../../research/main-menu-normal/REPORT.md), [native owned-skin route](../../research/main-menu-normal/owned-skins.md).

It also bypasses creation of the tournament route's Pod and Name screens. On startup graph `0194`'s own thread, it supplies the explicit `lobbyServer` address and `Player` to the normal native connection APIs, then selects the graph's existing route around both screens. Fresh-launch verification reached the menu, Training and Practice Range with both screen-state slots null ([research](../../research/login-bypass-2026-11-07.md)). It also removes the tournament route's low-level keyboard hook, so Alt+Tab, Alt+F4 and the Windows key work again (`research/tournament-input-lock.md`).

How it works: the image cannot be made writable, so the plugin copies runtime vtables with slot 4 (the evaluator) replaced and redirects matching instances in private read-write memory. The startup override targets dynamic variable `2275` only in graph `0194`, retaining the native result when prerequisites fail. It starts scanning immediately, with a 10 ms sleep between passes until a connection is requested. The six retail-menu overrides still wait for `retail_menu.go`, then rescan every 3 s: changing the tournament value during login selects the unsupported Battle.net route.

Log lines (`retail_menu ...`): one per redirect batch and per change of the game's own value.

### aspect_ratio

Adds **32:9** to **Options → Video → Aspect Ratio**, retaining 16:9, 16:10 and 21:9. Select 32:9 and Apply. Resolution detection remains native. The new mode preserves vertical FOV and shows more world horizontally; the FOV slider retains its native 16:9 reference value. At a slider value of 103, the measured 32:9 horizontal view is approximately 136.6°.

Selection persists in `build/ow_fixes.ini`, `[Graphics] Aspect32x9=1`. Existing modes save `0`.

The plugin fits 32:9 inside the client area, rebuilds matching color/depth targets and the native render canvas, and preserves vertical projection on full-view perspective cameras. It uses the native UI worker queue for the fourth choice. It checks this build's graphics signatures before installing heap/COM vtable copies; executable pages and engine-root vtables stay intact. New camera/controller instances are found every two seconds. [Implementation and live evidence](../../research/aspect-ratio-32-9/REPORT.md).

### client_feed

Fixes shots drifting off the bots the longer you move. The client never sends its position, so the lobby dead-reckons a copy of the player from the commands, and hitscan shots start from that copy ([research](../../research/hitbox-drift.md)). This module reads the client's own mover (read-only: vtable `img+0x260E7C8`, feet at +0x70, command frame at +0xA0, slots 0x1610 apart) and sends every new `(frame, feet, slot)` as a 24-byte UDP datagram to `127.0.0.1:PORT`. The lobby's `tools/lobby/client_sync.py` moves its copy onto it after each command step.

Off unless the game's environment has `OW_CLIENT_SYNC=PORT`. `run.py` passes its environment to the game, so:

```bash
OW_CLIENT_SYNC=47124 python tools/lobby/run.py <game-root> --asi tools/asi/build/ow_fixes.asi
```

It finds the mover with an in-process scan (fractions of a second, against about 8 s for the old Python feeder), then checks the known slots and their neighbours for a new body (respawn, hero switch) before scanning again. Neighbour probes skip anything that is not committed read-write memory, so they never trip a guard page. The scan skips its own thread's stack, where its copies of a slot sit. It polls every 0.5 ms with a high-resolution timer. Log lines: `client_feed feeding 127.0.0.1:PORT`, `client_feed body at <slot>, frame <n>, <k> scan(s)`.

## helix_probe.asi

A research probe, not a game change. It sets hardware execution breakpoints from inside the client (a debugger attach crashes the 1.74 client) on the projectile shot path:
- the executor's job queue
- the shot builder
- creation's entry and return

Each hit is logged with the projectile asset and creation's status code (`research/soldier-runtime/helix-visual-findings.md`). Load it by hand into a game that is already in the Range:

```bash
python tools/asi/load_asi.py PID tools/asi/build/helix_probe.asi
```

Log: `build/helix_probe.log`. It disarms itself after 600 s.

## tracer.asi

A research probe, not a game change: it logs what the client does with the lobby's bytes (`research/asi-tracer.md`).

- **Server messages.** Every JamMessage dispatcher is registered as a 48-byte delegate object (vtable `0x7FF6D38368D8`, slot 2 invokes the dispatcher at +24). One copy of that vtable catches all of them: `msg <id> handler=img+<rva> result=<n>`.
- **State begin / end.** State activation (`0x7FF6D2D90790`) and deactivation (`0x7FF6D2D90480`) tell the script system (vtable `0x7FF6D384C010`) through its slots 32 and 35 with (system, instance, state item): `begin graph=<guid> inst=<id> node=<index> cls=img+<state class vtable> flags=<item flags>`.
- **The bits each state reads.** A state's data is read by slot 43 of its own class vtable (`0x7FF6D2D92060`) from the frame's bit reader (data at +0, bit position at +28). Every state item the tracer sees gets a copy of its class vtable with slot 43 replaced: `data graph=.. inst=.. node=.. cls=.. at=<absolute bit> bits=<count> <bits LSB first> flag=<create frame>`. Dumped right after a state begins and for the first two reads of each (graph, node), not for every per-frame re-read.

It waits for `tracer.go` (`run.py` creates it after the login), then rescans private memory every second (about 0.4 s a pass with regions over 64 MB skipped) for new delegates and script systems. The first message of a family (20300) and states that begin within that second are missed; items that already exist when a system is found are shadowed anyway.

Log: `build/tracer.log` (`<tick ms> <thread> <event>`), written by its own thread from a ring buffer.

```bash
OW_PVE_HANDOFF=1 python tools/lobby/run.py <game-root> --asi tools/asi/build/ow_fixes.asi --asi tools/asi/build/tracer.asi
```

## guard_probe.asi

A data watchpoint without debug registers (which crash this client, see `helix_probe.asi`). Write a hex address to `build/guard_probe.addr`; the probe puts `PAGE_GUARD` on that page, catches the guard fault in a vectored exception handler, logs the accessing instruction (`img+` RVA), the address and read/write, single-steps the instruction with the trap flag and re-arms. 300 hits, then it waits for the next `.addr`. The whole page faults, so neighbours show up too. Checked 2026-11-06: 300 hits on a live UX statescript instance, the game kept running (`research/asi-tracer.md`).

```bash
python tools/lobby/run.py <game-root> --asi tools/asi/build/ow_fixes.asi --asi tools/asi/build/guard_probe.asi
python tools/research/scriptlist.py PID          # pick an instance address
echo 2908C3D6DF0 > tools/asi/build/guard_probe.addr
```

Log: `build/guard_probe.log`.

## frame_grab.asi

Opt-in. Saves the game's D3D11 back buffer as a 32-bit top-down BMP right before Present, without DWM (PrintWindow sometimes returns black). A dummy D3D11 swap chain gives the `IDXGISwapChain` vtable pointer; the game's swap chain objects get a vtable copy with Present (slot 8) and Present1 (slot 22) replaced (rescan every 2 s for 5 minutes, then every 10 s). Verified 2026-11-06: the game's swap chain shares the dummy's vtable. (Original note: whether the game's swap chain shares the dummy's vtable is unverified; the log says `swapped N swap chain object(s)` when it does.

Request: write the absolute output path as the first line of `build/frame_grab.req`. The plugin deletes the file within 100 ms; the next Present writes `<path>.tmp` and renames it onto `<path>`. Log `build/frame_grab.log`: `shot <path> <w>x<h> fmt=<n>`, or `no present within 3s` (request dropped). Formats: B8G8R8A8/X8 and R8G8B8A8 (UNORM, SRGB), R10G10B10A2 UNORM; others are logged and skipped.

```bash
python tools/lobby/run.py <game-root> --asi tools/asi/build/ow_fixes.asi --asi tools/asi/build/frame_grab.asi
```

## Build

```bash
cmd /c tools\asi\build.bat
```

MSVC 2022 x64 (`vcvars64.bat`). It runs the self-checks (`test_aspect_ratio.c`, `test_retail_menu.c`, `test_client_feed.c`), then builds `build/ow_fixes.asi`, `build/helix_probe.asi`, `build/tracer.asi`, `build/guard_probe.asi`, `build/frame_grab.asi`, `build/dev_console.asi`. The game must be closed (it holds the file).

## Run

```bash
python tools/lobby/run.py <game-root> --asi tools/asi/build/ow_fixes.asi --map Hanamura --mode 0x0230000000000014
```

No `--timeout` means no time limit: the launcher waits until the game exits. `run.py` injects plugins immediately after launch with `load_asi.py` (remote `LoadLibraryW`; a proxy DLL in the game folder is never loaded, the game limits its DLL search), creates the `.go` file 10 s after the lobby's login records, and starts the lobby with `--retail-menu`. The plugin waits for the packed image's expected vtable entries before installing its overrides. Then: TRAINING, PRACTICE VS. AI, a difficulty. The lobby answers the queue request with the hosted map at that difficulty.

Changes: `CHANGELOG.md`.
