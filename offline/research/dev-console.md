# Hidden developer console and debug cvars (2026-11-06)

Question: does the 1.74 retail client have a hidden debug mode, and can we view each hero's animations in first and third person?

## Answer

1. **Yes, there is a developer console.** It is a Dear ImGui window that the retail build still contains. It opens in game with a small plugin and **Alt+`**. A "DEV ENABLED" badge shows in the top right corner (`dev-console/console-open.png`).
2. **The console has almost no commands.** The retail build registers one: `runCommand`. The console is still useful as a live engine log (map loads, config warnings).
3. **Many named debug cvars survive** (`anim.*`, `firstPerson.*`, `camera.*`, ...). Some still have live readers. There is no command to set them, but a plugin can write their globals.
4. **Animation viewer: works both ways.**
   - In game: the first-person arms can be moved or turned in front of the camera through `firstPerson.Offset*`, and a server-side "mannequin" hero bot shows any hero in third person.
   - Offline: the Blender route covers third person too.

## Console: how it opens

| Piece | Address (image base 0x7FF6D1220000) | Notes |
|---|---|---|
| Command-line switch `tank_Console` | read in `0x7FF6D2A03D70` | stored in cvar byte `0x7FF6D404F640` |
| Tournament preset `0x7FF6D2A27F10` | | sets `0x404F640` = 1, `0x404F5C8` = 0 (our launch uses `--tank_TournamentMode`) |
| Toggle `0x7FF6D2A393E0` | | needs `0x404F640`. With the tournament byte `0x404F448` set, it also needs `0x404F4B0` (`tank_TournamentModeAdmin`) and `0x404F5C8` (unnamed dev gate). |
| Key handler `0x7FF6D2C7B900` | | engine key `0x60` = GRAVE with Alt (flag `0x20000000`; the same handler takes Alt+Enter as key `0x0A`) |
| Console object | `0x7FF6D445BC58` | created at start (21512 bytes), open flag at +21504 |
| Command table | `0x7FF6D445BC60` | 256 hash buckets; registrar `0x7FF6D2A3C2D0`; count at `0x7FF6D445C468` |

- `tools/asi/dev_console.asi` keeps `0x404F640`, `0x404F4B0` and `0x404F5C8` at 1. It logs the console object and open flag, and dumps the command table the first time the console opens.
- Run: `python tools/lobby/run.py <game-root> --asi tools/asi/build/retail_menu.asi --asi tools/asi/build/dev_console.asi`. At the main menu, press Alt+` with the game focused (`gameinput.py fg-key 18+192 150`).
- Posted (background) key messages do not toggle it. The engine reads keys through raw input. Typing into the open console does work with posted `WM_CHAR` (`gameinput.py bg-text`).
- Alt+` also types a `` ` `` into the input line.
- Other dev keys in the same handler, all gated on `0x404F5C8`:
  - Alt+C re-runs the last console line.
  - Alt+Ctrl/Shift+F toggles bit `0x80000` of a system's flags.
  - Alt+Ctrl/Shift+R calls that system's vtable slot +224.
  - All three are untested.

## Console: what it can do

- Command dump (`dev-console/dev_console.log`): `command table: 1 entries` → `runCommand | args 1..255 | handler 0x7FF6D199D8C0`.
- Unknown words (`help`, `list`, `?`, cvar names) fail silently. The executor `0x7FF6D2A3CA80` returns 0 when a name is not in the table.
- `runCommand a b ...` joins its arguments and hands them to the system at `0x7FF6D41786E0` (`0x7FF6D205E880`). At the main menu it changed nothing on screen and the lobby received no new message. In a match it may reach the game server. Untested.
- Lines in `<data>/CCS` files register "Console Command Shortcut" commands (`0x7FF6D2A3CD10` → `0x7FF6D2A3BCE0`). They only chain existing commands, so they add nothing while the table holds one command.

## Named debug cvars

Registered by name with a description (registrar `0x7FF6D2A36770`; bool, int and float variants), each in its own global. Live = some code besides the registration reads the global.

| Cvar | Global | Live? | Description in the binary |
|---|---|---|---|
| `firstPerson.DetachModelFlag` | `0x7FF6D417E418` | yes (`0x7FF6D1A10299`, first-person model update) | detaches the first person model and places it at the last known position |
| `firstPerson.OffsetX` (Y, Z, Yaw, Pitch, Roll at +4 ...) | `0x7FF6D417E41C` | X yes (`0x7FF6D1A10423`) | offset the first person model |
| `firstPerson.DrawThirdPersonModel` | `0x7FF6D417E450` | no | in first person view show third person model or not |
| `firstPerson.ShowFrameFlag`, `firstPerson.ModelGuid` | `0x417E44C`, `0x3F28788` | no | |
| `anim.ShowBindPose` | `0x7FF6D4445C7C` | yes (`0x7FF6D29B6A20`, `0x7FF6D29BC500`) | |
| `anim.ShowAnimNames` | `0x7FF6D4044F3B` | no | Try and show animation names in 'in-game' animation debugger |
| `anim.DebugTransitions`, `anim.debugDrawIK`, `anim.DumpPoseInfoForGUID`, `anim.animateMultiplier`, `anim.Enable*` | `0x4445C80`, `0x4445CC0`, `0x4445CC8`, `0x4044F44`, `0x4044F58...` | no direct reader | |
| `camera.EnableUpdate` | `0x7FF6D3F232B4` | yes (`0x7FF6D18B41C7`, gates a camera update) | |
| `camera.moveSpeed`, `lookSpeed`, `forceInput`, ... | `0x3F23A00`, `0x3F23A04`, `0x4179662` | no (`0x7FF6D18B2C70` is their registration, not a reader) | |
| `player.GenerateRandomMovement` | `0x7FF6D417BD78` | yes | For local player generate movement commands |
| `debug.DisableErrorCorrection` | `0x7FF6D417E530` | yes | Disable movement error correction for local players |
| `combat.disableReticle`, `localhiteffects.*`, `input.ForceInputMode` | | yes | |

- The full name → global map for 89 cvars came from scanning for `lea rdx, name` followed by `lea r8, global`.
- The in-game animation debugger itself (`anim.ShowAnimNames`) was compiled out. Only its flag remains.
- Reading these needs no console, but setting them needs a write from a plugin, like the three console gate bytes above.

## Animation viewer

### Offline (Blender): works for first and third person

- First person was already in place (`blender-animations.md`).
- Third person uses the same exporter with the hero's gameplay (body) entity instead of the first-person one:

```
python tools/assets/anim_export.py export 0x04000000000003CF -o research/anim/soldier_3p.json   # ~100 s
blender -b --python tools/blender/import_anim.py -- research/anim/soldier_3p.json --save research/anim/soldier_3p.blend
```

- Soldier: body entity `0x04000000000003CF`, body model `0x0D00000000001137` (128 bones, 4 submeshes, 11,830 vertices), **137 clips**, plus the gun child `0x04000000000004BF` (12 clips) on bone `0x4A`.
- The import self-check passes: worst bone error 0.000001 m.
- Renders: `dev-console/soldier-3p_front.png`, `soldier-3p_side.png`. They show the first clip, `0x0A00000000000847`, which looks like a death or fall pose.
- In these renders the gun stands upright while the body lies down. Its 3P hardpoint placement is unverified.
- `hero_gameplay_entity_guid(hero)` gives the body entity for any hero, so the route covers all 31.

### In game: works (2026-11-06)

**First person.** `dev_console.asi` applies `tools/asi/build/dev_console.set` (`name value` per line) whenever the file changes. The first-person model update (`0x7FF6D1A0FB60`) always adds `firstPerson.Offset{X,Y,Z}` (metres, camera space: +X left, +Y up, +Z forward) and `Offset{Yaw,Pitch,Roll}` (degrees, × 0.0174533). With `DetachModelFlag` ≠ 0 it takes another branch, which the cvar's description calls placing the model at its last known position (untested).

```
firstPerson.OffsetYaw 180
firstPerson.OffsetZ 1.9
firstPerson.OffsetX 0.25
firstPerson.OffsetY 0.1
```

- With these values, Soldier's arms and rifle face the camera about 2 m ahead. Fire (muzzle flash toward the camera) and the reload (gun tilts, arm brings the magazine) play live: `dev-console/fp-front-fire-reload.png`. Other placements: `dev-console/fp-offsets.png`.
- Set all to 0 to restore the normal view.
- Firing and reloading need foreground input (`gameinput.py fg-key lmb 900 r 100`).

**Third person.** A "mannequin" served by our lobby (`tools/lobby/viewer.py`): set `OW_VIEWER_FILE` to a text file that holds a hero name, then launch:

```
echo Reaper > research/dev-console/viewer.txt
OW_VIEWER_FILE=<repo>/research/dev-console/viewer.txt python tools/lobby/run.py <game-root> --asi tools/asi/build/retail_menu.asi --asi tools/asi/build/dev_console.asi
```

- In the Practice Range an enemy hero bot stands 5 m in front of the spawn. It faces the player and fires and reloads at zero damage. Shooting it plays its death; it comes back 3 s later.
- Editing the file swaps the hero within a second.
- Checked live: Reaper, Mei, Tracer, Reinhardt, D.Va (pilot) and Genji (`dev-console/mannequin-*.png`).
- Limits:
  - Fire only shows for the 11 heroes with bot weapon graphs (`target.WEAPON_GRAPHS`).
  - No abilities.
  - Reinhardt showed no hammer.
  - The camera is the player's own first-person view, so get closer by walking.
- Both views can run in one session. Clip names do not ship in retail data and the name display was compiled out, so neither view can label clips.

## Files

- `tools/asi/dev_console.c` (built by `tools/asi/build.bat`), changes in `tools/asi/CHANGELOG.md`.
- `tools/lobby/viewer.py`, `Match._tick_viewer`, `tools/lobby/test_viewer.py`, changes in `tools/lobby/CHANGELOG.md`.
- `research/dev-console/`: screenshots, probe log, third-person renders.
