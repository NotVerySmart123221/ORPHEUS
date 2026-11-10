# Changelog

## 2026-11-10

- Two more marker recordings (`shadow-step-marker-wall-2026-11-10.rec.gz`: 196 aims at the room's far wall from two places; `shadow-step-marker-fine-2026-11-10.rec.gz`: 143 aims in steps of 0.1 degree of pitch from three distances, up and down; each holds several sessions, their frames 100000 apart) and their tests in `test_marker_compare.py`, with the numbers after section 40 (533 of 541 to the centimetre, none one-sided). `marker_compare.aims` counts the same heading and pitch from another place as another aim (the upward recording: 46 aims, 37 before).

- A fourth marker recording (`shadow-step-marker-pitch-2026-11-10.rec.gz`: one degree of pitch a step, six headings in the spawn room) and its test in `test_marker_compare.py`; the room's numbers after section 39 (77 of 80 to the centimetre, none one-sided).

- `test_marker_compare.py`: the numbers of the rule as section 38 has it (the three recordings to the centimetre: 46 of 47, 75 of 80, 5 of 6).

- `dump_asm.py START END`: a capstone sweep of the client's runtime image, for the code IDA leaves out of obfuscated functions. `test_marker_compare.py`: a third recording (aims level and upward) and the new numbers.

- `ida_dump.py` (IDAPython, for headless `idat.exe`): functions' disassembly with the float constants their SSE operands name, to a text file; nothing is saved to the database. Read the client's targeting query with it (two obfuscated functions the decompiler gives up on).

- `marker_compare.py REC`: Shadow Step's marker as the client drew it (a recording's `M` rows) against the server's own rule for the same aims. `mover_record.py` writes the `M` rows. `test_marker_compare.py`: two recordings.

- `graph_tree.py GRAPH...`: a catalog graph as a tree of what starts what (Entry, plugs, owner slots, a MovementMod's mods). Read Exo Boots' rule off graph 1983 with it.

- `test_ability_states.py`: the Roll table (the tool's, plus the three data-less states it leaves out).

- `test_launch_replay.py`: Coach Gun and Rising Uppercut against `vm-coach-gun-` and `vm-rising-uppercut-2026-11-10.rec.gz`. `test_ability_states.py`: their tables.

- `test_launch_replay.py`: Echo's Flight against `vm-flight-2026-11-10`, `vm-flight-held-2026-11-10` and `vm-flight-launch-2026-11-10.rec.gz`. `test_ability_states.py`: the Flight table.

- `body_moves.py HERO...`: every graph on the hero's VM body with a movement node (MovementMod, launch, MoveTo, mover swap, dash, thrust), with the buttons it waits on. `body_graphs.py HERO [GRAPH...]`: the body's graphs with their node classes, or one graph's nodes with their fields decompiled.

- `launch_replay.py` and `follow.py`: `--button jump`. `test_launch_replay.py`: Hover Jets against `vm-hover-jets-2026-11-10.rec.gz`.

- `launch_replay.py REC HERO`: replays the ability presses of a recording of the client's own prediction (VM body, no owner corrections) through the match code on a kits body, and prints how far the server's copy is from the client. `test_launch_replay.py` runs it on three recordings in `research/lizrollin-runtime/` (Jump Jet, Jump Pack, Combat Roll).
- `follow.py REC`: per ability press of an `--observe` recording, how far the client was behind the server's copy and when the two were together again.
- `test_ability_states.py`: the Combat Roll, Swift Strike and Jump Pack tables.

- `ability_states.py HERO BUTTON [--graph INDEX] [--hold S] [--seconds S] [--forward] [--kit]`: runs the hero's body offline on the vendored statescript VM, presses one button, and lists every state of the ability's graph with the seconds it was on, its class, its codec and what it does (a MovementMod's mods, a Stack's variable, a Wait's timeout). `--kit` prints the states as `kits.Ability.running` takes them. `STU_54FEB21C` is made to end at once in this process, since the VM has no class for it. Test: `test_ability_states.py`, which also keeps the kits' tables equal to the output. 85 research tests pass (research/lizrollin-runtime.md section 23).

- `mover_mods.py REC[.gz] HERO [--first FRAME] [--save OUT.rec.gz]`: replays a `mover_record.py --observe` recording of a hero's abilities, cut into stretches at every rest, with the plain mover and with the movement mods the vendored VM turns on for the hero's body. Per stretch: the buttons, the VM's MovementMod states and how far each replay ends from the client's feet. `--save` writes the gzipped recording the lobby tests replay (research/lizrollin-runtime.md section 21). Test: `test_mover_mods.py`. 82 research tests pass.
- `mover_walk.py --course ability`: a walk with each of Shift, E, a held jump, each fire button and Q, then a crouch walk, a jump from a crouch and strafes; for any hero. `--course sentry`: Bastion's Reconfigure both ways.
- `mover_record.py`: with `OW_RECORD_ULT=1` in the environment each new body starts with its ultimate charged, so a course's Q casts it.

## 2026-11-09

- `bot_records.py [--seconds 30] [--map NAME] [--mode GUID]`: what the game server sends about its bots' positions, measured offline through the real `server.Client.send_frame` (the Practice Range, then a Practice vs. AI map): datagram sizes, bot records a second, how many repeat the one before, and whether a record leaves before or with its bot's create. LizRollin's rule is counted beside it. Test: `test_bot_records.py`. Results: research/lizrollin-runtime.md section 20.
- `vm_shadow.py` builds our frame with each hero's own `reload_seconds` (`target.PLAYER_KIT`), as the match does. It used the default 1.5, which showed six var581 differences that were not real. Report rerun: research/lizrollin-runtime/vm-shadow-2026-11-09.md.
- `vm_heroes.py [--hero NAME ...] [--out FILE] [--volleys]`: runs every hero's body on the vendored statescript VM offline, idle and then through each button, with the lobby tests' client model reading every frame. Reports per hero whether it runs, whether the client's states match the server's, the Volleys fired and the button that fired each (split by whether `body_vm.effect` plays them), whether an ultimate ran, and the VM's warnings. `--volleys` prints the `body_vm.VOLLEYS` table. Report: `research/lizrollin-runtime/vm-heroes-2026-11-09.md` (research/lizrollin-runtime.md section 18).
- `vm_live.py launch` keeps an `OW_BODY_VM` already set in the environment (`OW_BODY_VM=all` puts every hero on the VM); it still defaults to `1`.
- `career_live.py launch GAME_ROOT [TIMEOUT]`: starts the game with a lobby that sends the stat catalogue 27202 at login and fills the profile answer 20807 with stats for All Heroes and Soldier: 76, to see what the Career Profile shows. A value is the stat's place in its list plus 1000 times the category key. `CAREER_CATALOG=0` sends no 27202, `CAREER_CATALOG=bare` an empty catalogue. `CAREER_VALUES=id` makes each value the stat's id in one category, so a row on screen names its stat id. Reads the catalogue and the 27202 layout from the local copy of LizRollin's `data/`; runs none of their code. Results: research/lizrollin-lobby.md section 3.
- `vm_live.py launch` with `OW_LIVE_BOT=<hero name>` set: the still bot is that hero's harmless mannequin (`tools/lobby/viewer.py`) 5 m ahead, in place of the Training Bot 8 m ahead. Used for a hero bot's kill feed lines (research/lizrollin-runtime.md section 16).
- `vm_live.py launch GAME_ROOT OUTDIR [PLUGIN ...]`: extra ASIs after `ow_fixes.asi`, such as `tools/asi/build/tracer.asi` (used for the kill feed, research/lizrollin-runtime.md section 16).
- `vm_live.py` has two modes now (research/lizrollin-runtime.md section 15, second live check):
  - `launch GAME_ROOT OUTDIR` starts the game like `run.py` with `ow_fixes.asi` and a 420 s limit. Its lobby runs with `OW_BODY_VM=1` and moves one Training Bot patrol to stand still 8 m along the client's spawn aim (yaw −32768, team 1). The lobby reads `OUTDIR/cmd.txt` each tick: `ult` fills the ultimate and `kill` kills the player.
  - `drive OUTDIR [abilities]` runs by default the bot-damage, ultimate, Tactical Visor, death and respawn sequence. `abilities` runs the earlier fire, reload, Helix, Field, Sprint and melee sequence. Start it after `launch` prints "Game PID": it drives the first game window it finds.
- `vm_live.py OUTDIR research/main-menu`: with the game started by `run.py` (`OW_BODY_VM=1`), enters the Practice Range as Soldier: 76 (background clicks, retried until the lobby log shows the map and the pick), then fires, reloads, and presses Helix, Field, Sprint and melee in the foreground, grabbing a frame after each (research/lizrollin-runtime.md section 15).
- `hitdef_shadow.py [--hero NAME ...] [--out FILE] [--json FILE]`: evaluates every hero's WeaponVolleys with LizRollin's VM (their `combat.hit_def`: damage, falloff, critical multiplier, explosion rings, rate, pellets, speed) and lists them beside `target.PLAYER_KIT`. Report and snapshot: `research/damage-data/` (research/lizrollin-runtime.md section 14).
- `test_hitdef_shadow.py`: our weapon, alt-fire and ability tables against that snapshot. 80 research tests pass.

- `vm_shadow.py [--hero NAME ...] [--out FILE]`: runs LizRollin's VM (in its own process, from their copy) beside our `kits.frame_builder` and compares each weapon graph's states and variables in four phases. Report: `research/lizrollin-runtime/vm-shadow-2026-11-09.md` (section 10).
- `mover_record.py --nudge M`: lifts the server's copy M metres at the first crouch (an `N` line), to see whether the client takes owner corrections. `--observe` now sets `client_sync.OBSERVE`, which keeps owner corrections on.
- `mover_walk.py --course fire`: walks and a jump while firing (D.Va's cannons).
- Recordings (research/lizrollin-runtime.md sections 9 and 11): `range-correction-nudge`, `range-fire-dva`, `range-walk-tracer`, `range-walk-reinhardt`, `range-walk-torbjorn` (all 2026-11-09).

- Mover check, three tools (research/lizrollin-runtime.md section 6):
  - `mover_record.py GAME_ROOT OUT.rec` (with `OW_CLIENT_SYNC` set): starts the Range like `run.py` with `ow_fixes.asi`, itself as the lobby, and writes every player mover step (command and server feet) and every client feet sample by command frame.
  - `mover_walk.py`: enters the Range on a fresh launch and walks a fixed 57 s course in the foreground.
  - `mover_replay.py WALK.rec[.gz]`: replays the walk through our mover and LizRollin's port (each in its own process) from the client's feet and prints the drift and per-frame step error of each.
  - The first walk is `research/lizrollin-runtime/range-walk-2026-11-09.rec.gz`.
  - After the mover port, `mover_record.py` logs `PlayerMover.step`, and "ours" in `mover_replay.py` is `PlayerMover` on the Range cache `run.py` builds. "Theirs" stays as a check of a newer pin. The second walk is `range-walk-2026-11-09-port.rec.gz`.
- Checks of the ported mover (research/lizrollin-runtime.md section 7):
  - `mover_record.py --observe`: logs the client's feet but leaves the server's copy uncorrected, so the recording shows the dead reckoning's own drift.
  - `mover_record.py --map NAME [--mode GUID]`: hosts a catalog map, opened from Practice vs. AI.
  - `mover_walk.py --hero NAME --course walk|boost [--no-enter]`: picks any hero by its portrait. `boost` is D.Va's Boosters course.
  - `mover_replay.py` adds a `live` row (the server's own feet from the recording); `--live` prints that row alone, for other maps and heroes.
  - Recordings: `range-observe-dva-2026-11-09.rec.gz` and `hanamura-observe-2026-11-09.rec.gz`.
  - `mover_walk.py` steps `look:DX[:DY]` aim up or down too. The boost course ends with a flight aimed up and one aimed down. Its recording: `range-boost-dva-2026-11-09.rec.gz` (section 8).
- `walkplan.py` replays its legs through `PlayerMover` on the map's mover cache (a flat floor without one), at Soldier: 76's run speed, with one running command frame.
- `volley_probe.py --records GRAPH [--seconds N]`: watches every Volley of one graph (read-only, about 1 ms a sample) and prints each change of its key `+0x1C0`, event counter `+0x44`, gate `+0x1EC`, shot index `+0x260` and flags, with the entity's pending ch2 shot records (component 35: event, key, age, consumed). It also finds Volleys whose vtable `tracer.asi` replaced (`volley_vtables`: read-only copies of the class vtable). `scan_for_bytes(..., protections=...)` picks the page protections to scan. Test: `shot_records`. Used for research/bot-fire.md.

## 2026-11-08

- `client_position.py`: the `--feed` mode is gone; `ow_fixes.asi` feeds the lobby from inside the client (`tools/asi/client_feed.c`). The drift report is unchanged. `hitbox_view.py` reads the 32:9 setting from `tools/asi/build/ow_fixes.ini`.

- `client_position.py` now reads the client's own mover, not the copies of the spawn position. The object (vtable `img+0x260E7C8`, second vtable `img+0x260E880` at +0x20) holds the simulated feet at +0x70 (1/1024 m grid) and the command frame at +0xA0. Those are the feet after that frame's command, and the frame is the server's `Command.frame`.
  - `ClientMover` finds the slot at any time with a vtable scan (about 8 s on a loaded map). The free slots of the array hold no position. When the body changes, it checks the known slots and their neighbours 0x1610 bytes apart, then rescans. Each sample is two equal reads (retried up to four times), because the frame and the position are written separately.
  - Drift mode compares the client and the hitbox feed at the same command frame, so it is exact while moving too.
  - `--feed PORT [--pid PID]` sends every new `(frame, feet, slot)` to the lobby's `OW_CLIENT_SYNC` port (`tools/lobby/client_sync.py`).
  - Tests: `test_client_position.py` covers sampling, torn reads, free slots, the throttled scan, a new body next to the pool, and the stale-frame re-find.
- `hitbox_view.py --drift` uses the new reader on a background thread and shows the drift at the feed's frame. This also fixes the live overlay: `tick()` assigned `client` without `nonlocal`, so it would have raised `UnboundLocalError` once the game was focused.
- `volley_probe.scan_for_bytes(..., max_region=None)` can skip regions larger than `max_region` bytes.

- Added `hitbox_view.py`: draws the server's hit volumes over the game, projected from the server's eye, so they show where the crosshair must be for a hit (needs the lobby's `OW_HITBOX_VIEW=PORT`). Live mode is a click-through overlay that hides when the game isn't focused. `--snapshot OUT.png` saves one capture with the volumes drawn in. `--drift` also shows the client-position drift. The viewport follows `aspect_ratio.asi`'s 32:9 setting. Tests: `test_hitbox_view.py`.
- Added `client_position.py`: finds the client's own position in game memory (read-only, hash-checked module) right after a spawn and prints how far the server's copy has drifted. Readings are exact while standing still. Tests: `test_client_position.py`. Written for research/hitbox-drift.md.
- `volley_probe.scan_for_bytes(k32, process, needle, align)`: the memory scan for any byte string; `scan_for_qword` now uses it.

- `gameinput.py fg-rec OUT.mp4 KEY MS ...`: `fg-key` while recording the game's client area at 60 fps, half size (ffmpeg `ddagrab` desktop duplication, x264 ultrafast; this ffmpeg's NVENC needs a newer driver). Assumes the game sits on DXGI output 0 at the desktop origin.

## 2026-11-07

- The historical body-probe byte comparison excludes Soldier's later native reload-input registration; current reload/input behavior has dedicated command-path and wire-format regressions in `test_soldier.py`.
- `weapon_sweep.py sprint-transitions`: captures walking, Sprint entry/loop and return to walking through the existing guarded input helper.
- Live follow-up passed Matrix cannon suppression/depletion and two Range-fire protection windows, Mercy's server-side stow cleanup, Moira switch/reacquisition, and an actual Alt+Tab check. Results and presentation limits: `research/live-verification-2026-11-07/REPORT.md`.
- `scriptlist.py` and `stateitems.py` use their adjacent modules instead of prepending the source fork's absolute path during live reads.
- `weapon_sweep.py matrix-fire`: records D.Va primary fire before Matrix, both triggers through depletion, then primary alone, using the existing guarded input helper.
- `weapon_sweep.py` resolves run logs relative to this checkout, so migrated copies verify their own lobby rather than the source fork.
- `weapon_sweep.py`: `heal` (left mouse 2 s) and `boost` (right mouse 2 s) actions for Mercy's staff, grabbed from the lobby's `staff heal` / `staff boost` lock line. Aim at an ally bot first.

## 2026-11-06

- `weapon_sweep.newest_log` only considers run folders (a PNG saved into `research/main-menu` was picked as the newest run).
- `weapon_sweep.py`: `grasp` (right mouse 2.5 s, Moira) and `hack` (right mouse 1.2 s, Sombra) actions, grabbed from the lobby's lock / start line. Aim at a bot first (`gameinput.py fg-look DX DY`; about 1 count per pixel at 1274x720).

- Added `bot_walls.py [MAP MODE SECONDS SEED]`: runs a match's server bots headlessly (a still player at its spawn) and reports every per-tick move through solid geometry at chest/head height, and ticks off the floor; exits 1 on any wall crossing. Covers vs. AI maps, the Range (`0x0800000000000688`) and Uprising (`0x08000000000006FE`). Written for research/bot-movement.md.

- `screengrab.grab` no longer returns DWM's all-black frame silently: it asks `tools/asi/frame_grab.asi` for the game's back buffer instead (request file `tools/asi/build/frame_grab.req`, 3 s wait), and raises `BlackFrame` when the plugin is not loaded. `BlackFrame` is not a RuntimeError, so `weapon_sweep.grab`'s PrintWindow retry does not swallow it. The CLI exits with that message instead of saving a black PNG. Test: `test_screengrab.py`. Why: one Range run (2026-11-06) captured only black frames with no foreground window; a desktop switch and a monitor standby did not reproduce it, so the cause (likely the lock screen) is unconfirmed (research/background-input.md).

## 2026-11-05

- Added `assettab.py PID [GUID ...]` (the loaded-asset table: record states, map headers, modes, maps), `loadstate.py PID MAP MODE` (the two map-loading systems holding a pending game: state, attached and ready flags, the compared arrays) and `stateitems.py PID INSTANCE [STATE ...]` (a statescript instance's state items with their runtime objects, as the loading screen's predicate reads them). All read-only; written to find why Uprising stayed at ENTERING GAME (research/play-menu.md, fourth round).
- Added `ux_bindings.py PID [NAME ...]`: lists the live client's UX binding objects (vtable img+`0x27B5B90`) with the property name from the widget template and the `0x0D8` identifier it resolved to (the data-context var-map key). Read-only. Found the party card's `IsLeaver`/`LeaverGamesLeft` ids (research/hero-menu.md).
- Added `creation_emu.py PID [--state N] [--watch S]`: emulates the client's projectile creation `0x7FF6D1D41790` for a live shot config and prints its status code. It polls the info map, snapshots each config the moment it is complete (it lives about 30 ms), builds the executor's request (tick, entity, config, asset) and stops at the spawn call. Found Helix's status 8: no predicted-entity ID reservation (research/soldier-runtime/helix-visual-findings.md).
- `live_emu.py`: rebases dump addresses onto the live image base instead of refusing another base, points the emulated TEB's stack range at the emulator's stack (`__chkstk` probes to it), and adds `copy`, `live` and `stop_at` plus `fault`/`stopped_at` reporting.
- `hw_watch.py`: `--execute` arms instruction breakpoints (DR7 RW/LEN 00; the resume sets RF so the trapped instruction runs once), `--rva` takes addresses as RVAs in the verified client, and `--deref REG+OFF` records a qword per sample. Each sample now also records rax, rcx, rdx, rsi and r8. Hardware breakpoints are allowed on the client since 2026-11-04 (tasks/roadmap.md, Rules). On the 1.74 client the attach still crashes the game within about 1 s (`0xC0000005`, 0 hits; research/soldier-runtime/helix-visual-findings.md).

## 2026-11-04

- Restored the Soldier body probe byte comparison. The fixture supplies health, verifies numeric health and quick melee are present, then compares the original loadout fields against the native-verified probe.

- `weapon_sweep.py`: the hero select bar and SELECT button are recalibrated for the 1274x720 16:9 window (bar at y 615, 36 px apart; SELECT at 636, 671); the reload burst is 14 frames 0.2 s apart.
- Added `weapon_sweep.py HERO OUTDIR [ACTION...]`: picks a hero on the select bar (H, portrait, SELECT), then grabs frame bursts of the held weapon, a left-mouse burst, R, a right-mouse press and V, each from the lobby log's line for the press; `contact_sheet.py DIR [COLS]` tiles a folder's frames. `gameinput.fg_key` now moves the cursor over the game before a mouse button (the client ignored buttons with the cursor elsewhere).

## 2026-11-03

- Added `config_var_eval.py HASH...`: resolves a config-var class hash to its runtime vtable and evaluator in a client runtime snapshot (`OW_SNAPSHOT`, default `.scratchpad/runtime-weapon`) and disassembles the evaluator. Found the PLAY card gates without IDA.

## 2026-11-02

- Added `walkplan.py COUNTS_PER_DEGREE YAW X Y Z [--objective N] [--legs N]`: plans `gameinput.py fg-key` mouse turns and W holds from a logged server position to a Hanamura Assault objective along the nav path's straight runs, replayed through the server Mover first. Mouse right lowers yaw, about -10.1 counts per degree at default sensitivity.
- Added three read-only statescript probes: `scriptlist.py PID [GRAPH...]` lists live instances by their vtable with active states; `guidhits.py PID GUID...` finds a graph GUID in memory and shows the qword at hit-0x1E8 (an instance's vtable when it is one); `inststates.py PID ADDRESS...` prints an instance's active states.
- Added `volley_probe.py`: scans the client's writable memory for the rifle Volley state, decodes its entity and the per-weapon info map, and reports `[info+0x40]` (`--watch` waits for an active burst). Read-only. When a 1 MB chunk read fails, the scan retries page by page so one bad page cannot hide an object.
- Added `live_emu.py`: runs client functions in unicorn over pages copied on demand from the live client. It uses real call-site return addresses (so the client's caller checks pass) and a real thread's TEB for TLS. Writes stay in the emulator. It evaluates the fire-animation gates and lists the play call's writes. Needs `unicorn` (2.1.4 installed).
- `gameinput.py`: `fg-key` accepts the mouse buttons `lmb` and `rmb`, and steps `look:DX:DY` that move the mouse and then wait (`look:0:0 MS` is a pure wait). New `fg-look DX [DY]` for aiming. One mouse count turns the camera by about -18 yaw units.
- `test_body_probe.py`: the normal-loadout check leaves out the ability graphs, Helix states and the ultimate charge (var342) added since the probe was verified.

## 2026-11-01

- Vendored the beta project's `screengrab.py` (GDI grab of the game window's client area) so captures no longer depend on the beta folder.
- `screengrab.py` now uses PrintWindow with PW_RENDERFULLCONTENT, so captures stay correct when other windows cover the game.
- Added `hw_watch.py`: hardware data-breakpoint access counter (attach, arm DR0-DR3 per thread, disarm before detach). Works on ordinary processes; on the 1.74 client it got no hits and the game crashed after detach (anti-debug). Do not use it on the client without a new approach.
- Added `enter_range.py`: background-input route from the start screen into the Practice Range as Soldier: 76.
- Added `gameinput.py`: window placement, background (posted-message) input for menus, and idle-gated foreground input with a cancellable countdown for gameplay. Findings: `research/background-input.md`.

- Added `rifle-bot-remote` using the live-verified BA3 remote-state slot7 and native H0 Volley grammar. Session215255 confirms remote flags0x40E1/event1/hardpoint5 without assertions, but no visible shot. Added `rifle-bot-remote-init` with authored Entry constants and firing data-flow mapping; session221710 confirms replicated values and normal exit0, with no visible shot. Neither probe changes normal gameplay. Resume notes: `research/CONTINUE.md`.

- Added `rifle-shared-parent`, linking shared HUD463 beneath graph75/state3 with the existing compact child-ID helper. Native37D754C8 final vtable `0x7FF6D387B9C8` uses reader `0x7FF6D2DC1A30` and writer `0x7FF6D2DC21A0`: compact16 child ID plus one flag. Session201524 verifies active graph75 (42 total states), state3 child463/flag0; numeric health remains0/200.

- Added isolated `rifle-bot-feedback`, preserving Soldier's shared HUD while wiring BA3 shoot variable233 in both scopes, its data-flow state7 and nested Stack10/26 around Volley13. Session195232 accepts the frame; visible shots remain unconfirmed.
- `rifle-attack` now retains the shared health/ammo HUD and native rifle binding across damage/respawn. Session191414 verifies shrinking white health bars, damage indicators, low-health vignette and fresh bodies; numeric health text remains0.

- Verified graph75's authored Soldier body texture override for variable1807 (`0x0C00000000007DA7`, CKey/MD5 `FB0295F3D9B927C8248DFCA2926E251C`) and graph75 node8/state3 passthrough to graph463 presence bit0. Native session185431 renders the Soldier portrait and green health bar; displayed health remains0/200 while server health is200. Session184745 had state139 active but state79 false and UX37 null before texture delivery.
- Extended `rifle-shared-hud` with graph463 owner39/state139, the catalog-backed client subgraph leading to UX37. Its texture gate now passes and the portrait/green bar render; numeric health binding remains unresolved.

- Verified phase `rifle-shared-hud` in native session `20261101T182643.809872Z`: player component85 flag1 plus body graph463 owner state40, `Graph(463, 41, 22)`. Ammo panel changed30→29 on a real primary-fire click and returned to30 on R; captures `research/soldier-runtime/ammo-hud-{30,29,reloaded}.png`. Body vars10964/11815 are ScriptReferences;29677 resolves to player `A0000100`. Health panel rendering is now verified separately; numeric health remains incorrect. Combined GameMode phase174917 still did not resolve F7 parent slot228; F7 remains unclassified.
- `rifle-root-flags` is native-verified: component85 flag1 accompanies the ammo-panel result; switching away clears the diagnostic field.
- Added `rifle-projectile`, an isolated native1476 transform fixture, retaining rifle binding and cleaning up when switching phase. Session165658 identifies an unsupported ordinary movement record; removed it. Session170344 accepts transform-only creation; visible projectile effect remains unconfirmed.
- Added `rifle-root-body`, placing graph4429 on the possessed body with its default self descriptor and preserving it across firing frames. Session162514 makes local-player gates1/6 true; state7's controller-flags condition remains false and root0 is absent.
- Added staged original Training Bot graph/Volley probes (BA3:32 owner states,13 instance presence values), retaining Soldier initialization and a separate bot stream. Session154228 reaches counter1/event1 after delivery. Session160735 confirms muzzle6973 as native kind4/hardpoint5 and counter1/event1 after a phase transition; visible projectile remains unconfirmed.
- Added `rifle-reload` using recovered Anim39 linked-reference/event/override payload and reload variable51. Native session150638 verifies manual/empty-magazine reload animation, refill30 and subsequent shot29.
- Added isolated `rifle-feedback`: native session143641 accepts shot-only Stack23/72 (owner bits23/68), Bool689 in both scopes, ten shots and target removal/respawn. No additional recoil animation is established by that capture.
- Added `rifle-binding`, retaining the combined manager/body setup and binding instance var49 to CosmeticEntity state11. Session140210 accepts native reference `0xB00010001`; ten shots originate at the held rifle and damage/remove/respawn the target.
- Added `rifle-manager`, combining the verified weapon/body initializers with manager21's client-subgraph hosts42/43 (owner32/33). Session132505 creates presenter38/root2BC and a visible native centre reticle; both survive later shot frames.
- Gave graph75 a separate instance ID after session125210 showed that reusing manager instance2 retained the old graph. Removing it first and recreating manager/health yielded native graph counts45/31.
- Added isolated `rifle-entry` and `rifle-body` probes. Session125210 accepts Entry Stack64 (its output472 remains absent) and graph75 state19. Graph75 survives ten-shot Start/Stop frames with ammo20 and bot damage/removal/respawn.
- Added isolated `rifle-dataflow` selected-weapon mapping states6/17/18/22/33/34. Session123109 accepts all six and visibly changes the rifle ammo strip30→20 during ten confirmed shots; muzzle alignment/recoil remain unresolved.
- Added isolated `rifle-client` owner-state83 probe. Native session120636 creates state83 and client state59, whose corrected branch byte at+238 is1. Ten shots advance Volley10/ammo20 and damage/respawn the bot; aligned effects remain unresolved.
- Added `hud-root` and `hud-root-body` from the verified loose graph114D:9states,1owner,0instance presence values, root270 at state0. Native sessions102956/112553 survive; entry condition remains false and no HUD root is created.
- Added `rifle-muzzle-entity`, creating the verified rifle definition04BF without a mover and binding variable49 to its entity ID. Native session100047 accepts ten shots, ammo30→20 and target200→0/respawn; the tracer remains misaligned. Fixture attachment is unverified.
- Added `hud-bootstrap-ref`, supplying GameMode23604 as a state reference to helper3254 instance2/state5. Native parent resolution succeeds; root001F's slot expects type1B4, incompatible with HUD F7, so rendering remains unresolved.
- Corrected GameMode bootstrap presence counts to3/0 by excluding empty/entity-scoped catalog entries after the first16/4 frame asserted. Added separate typed-entity23604 probe; both remain experimental.
- Added an isolated GameMode bootstrap probe from verified mode0018→graph5056→state21/graph3254. HUD presenter parent is null without a root context; bootstrap rendering validation pending.
- Added `hud-native`: deliver the native-verified GameMode23604 gate before HUD initialization, without guessed local/entity presenter bindings. Native validation pending.
- Added a body-scoped variable23604 probe after tracing the HUD expression's external variable lookup. Native frame survives but HUD branch8 remains false.
- Corrected the muzzle-effect payload to Begin plus a two-bit event counter after tracing the final derived vtable. Native initialization and ten input-driven hits/removal/respawn survive.
- Added the explicit `rifle-attack` phase; other phases and invalid control files disable bot attacks.
- Applied attack gating before the match tick so switching away cannot cause one final due hit after the new phase has been read.
- Native weapon-UX and all three Stack29 variants survive but do not restore health/ammo HUD or establish aligned muzzle firing.

## 2026-10-31

- Added weapon UI69/70 and firing Stack29 flag probes, preserving their extra owner states in subsequent shot frames. UI class DSR/Begin shape is native-verified; Stack flag meanings remain diagnostic candidates.

- Added a rifle-input phase for the catalog selected-weapon route through gates1/2/44/76 and LogicalButton20. Owner bit72 represents state76; button wire shape comes from native dispatch/field filtering.

- Added catalog-backed manager21/health51 root probes alongside the rifle. A separate phase activates manager root BooleanSwitch11 and group39; class helper false classification was checked in the native image. Native initialization verification pending.

- Added a HUD reset phase that removes the existing child graph before a later binding probe recreates it; checks prevent treating variable changes as fresh initialization.

- Native tests confirm advertising bot hero content resolves body1403. Component123 then asserted and was removed; supported team/health creation survives, but rendering remains unverified. Player/game-mode variable23604 probes survived without HUD output.

- Diagnostic login now advertises Training Bot hero content alongside Soldier. First native target creation returned to Play with a missing body1403 definition; preload verification is pending.

- Added bounded, read-only state-factory registry inspection to resolve dynamic runtime deserializer addresses without debugger attachment. Validates the client path/hash and registry bounds; emits constructor/vtable and transient registry address metadata.
- Added diagnostic control-file phase selection to reuse a surviving owned game session. Only known concrete phases are accepted, and a pending full frame must arrive before another is sent.
- Added native-derived Volley count payload, initialized-ammo and HUD-binding/gate probes. A Volley count update produced a visible tracer and recoil; input-controlled pulse/stop behavior still requires native verification.

- Added a read-only debugger observer for the verified 1.74 client. It checks PID/module path/hash before attach, limits context and memory capture, and continues target exceptions without handling them except the attach breakpoint.
- Added an isolated, bounded native body-frame probe, reusing launcher ownership and the existing TCP/UDP lobby. Phases separate empty frames, graph creation, integer values and held-entity activation. Normal launcher behavior remains unchanged.
- Added root-only, explicit cosmetic-control-bit and candidate BooleanSwitch-payload probes for the instance event-boundary assertion. These are experimental frames, not normal launcher initialization.
- Root-bool survived and exited normally; combining it with explicit cosmetic Begin1 still asserted. Retained both isolated phases as reproducible protocol evidence.

## 2026-11-02 (bots)

- `gameinput.py fg-key` accepts chords (`w+d`): every key is held together for the given milliseconds. It also takes several `KEYS MS` pairs, held one after another behind one guard (`fg-key w 3000 w+d 5000`).
- `body_probe.py`: bot phases drive the nearest still roster bot (`bot_probe_update`). `rifle-bot-remote-init` re-sends the remote Volley every second with the next 6-bit counter and sends sync vars 1 and 6. The probe no longer overrides `login_records`: the lobby preloads every bot hero.
