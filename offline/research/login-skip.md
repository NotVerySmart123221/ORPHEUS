# Skipping the Pod and Name screens (2026-11-04)

**Historical implementation:** the body below describes the original screen automation. On 2026-11-07 it was replaced by a [native startup graph bypass](login-bypass-2026-11-07.md) that avoids creating Pod and Name.

Goal: boot straight to the main menu on the tournament route, with no Pod Config or Name Entry screens. **Status: done in `tools/asi/retail_menu.asi`.**

## What the screens are

Both screens are native C++ screen classes, not UX graph logic (IDA, runtime dump, image base `0x7FF6D1220000`).

| | Pod screen | Name screen |
|---|---|---|
| Vtable | `0x7FF6D38676C0` (ctor `0x7FF6D1CB0290`) | `0x7FF6D3867900` (ctor `0x7FF6D1CB01E0`) |
| Slot 63, UX event handler | `0x7FF6D1CB4040` | `0x7FF6D1CB3FC0` |
| Button event | START `0xD80000000006503` | ENTER GAME `0xD800000000061E2` |
| Check run by the button | `0x7FF6D1CB3B40` | `0x7FF6D1CB3880` |
| Slot 58, per-frame update | base `0x7FF6D1C0ECC0` | `0x7FF6D1CB3800` (enables ENTER GAME while the lobby object is idle) |

- **Pod check (`0x7FF6D1CB3B40`).** It reads the `lobbyServer` cvar. With `--lobbyServer` set and the tournament flag (`0x7FF6D404F448`) on, it skips the pod and group fields. It stores the address in the lobby object (`[qword_7FF6D41786E0+632]+48`) and returns true. The handler then closes the screen (`0x7FF6D2D911D0(screen, 0, 1)`). Pod and group only matter for ShowMode (`0x7FF6D404F420`) and `showmodeLobbyLocations.json` (`0x7FF6D1CB31D0`). `PodId` is an int cvar (`0x7FF6D4178370`, setter `0x7FF6D186F1C0`).
- **Name check (`0x7FF6D1CB3880`).** It reads UX var `0xD800000000001D0`, the name field. In tournament mode the name must be 1–12 characters; otherwise it is 3–12 characters with letters only. It passes the name to a player object (`0x7FF6D1858320`) and closes the screen. Errors are shown in UX var `0xD800000000061F5`.

## How the plugin skips them

1. It gives each screen instance a copy of its vtable, the same method the plugin already uses for config vars. In the copy, slot 58 is replaced and slot -1 is kept.
2. The new slot 58 calls the original, counts frames per instance, and 30 frames after a screen appears:
   - on the Name screen, sets UX var `0xD800000000001D0` to `Player` (`0x7FF6D2FECCA0(store, var, text, 0)`; store = `0x7FF6D1CAFA00(0x7FF6D1C0E1F0(screen))`);
   - sends the screen's own button event to slot 63, once.
3. So the game runs its own checks on its own UI thread, as it would for a click. The menu still shows the name the lobby sends in `0x5014` (`Player`).
4. The rescan runs every 0.5 s until ENTER GAME has been sent, then the plugin waits for `retail_menu.go` as before.

## Launcher timeout

`run.py --timeout` is now optional. Without it, the launcher waits for the game to exit, then stops the lobby. With it (10..3600 s), the launcher closes both processes after that many seconds, as before. The game and lobby have no time limit of their own.

## Results

- Run 1 crashed (`EXCEPTION_ACCESS_VIOLATION executing 0`) as soon as the name was filled in. The name var's change listener (`0x7FF6D1C10770`) calls screen slot 64 (`+0x200`), but the copy only held slots 0–63. Both primary vtables have 65 slots: slot 64 is the shared `0x7FF6D2F05640`, and slot 65 starts the secondary vtable at `+448`. Fixed by copying 65 slots.
- In run 2 (`main-menu/20261105T032655.907858Z`), START was sent 1 s after the plugin loaded and ENTER GAME 1 s after that. The lobby logged the player in, and the retail main menu showed `PLAYER` with no input ([screenshot](main-menu/login-skip-main-menu.png)). The config var redirects followed as before.
- The pod and name vtable pointers turn up again on several rescans before START. Each scan redirects them again, which is harmless; the rescan stops once ENTER GAME has been sent.
