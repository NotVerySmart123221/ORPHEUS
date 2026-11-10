# Tournament input lock: Alt+Tab and the Windows key (2026-11-05)

Goal: let Alt+Tab and the Windows key work on the tournament route. **Status: done in `tools/asi/retail_menu.asi`.**

## What blocks them

A low-level keyboard hook, not a window setting (IDA, runtime dump, image base `0x7FF6D1220000`).

- The input system's setup (`0x7FF6D2B07670`) installs it when `--tank_ShowMode` (`0x7FF6D404F420`) or `--tank_TournamentMode` (`0x7FF6D404F448`) is on, and `byte_7FF6D449BAB1` is off:
  `SetWindowsHookExW(WH_KEYBOARD_LL = 13, 0x7FF6D2B0B410, GetModuleHandleW(0), 0)`, handle stored in `qword_7FF6D44A5B00`.
  Retail (neither flag) installs no such hook. A second hook, `WH_KEYBOARD` (2) with `0x7FF6D2B0B340` on the game thread, is installed on every route and left alone.
- The hook procedure (`0x7FF6D2B0B410`) returns 1 (swallows the key) for: Esc with Ctrl or Alt, Alt+F4, Alt+Tab, Alt+Space, and VK 0x5B / 0x5C (both Windows keys), unless an "unlocked" flag is set: virtual slot 272 of the window object at `qword_7FF6D407A728 ^ 0x9E3B404B34CBD81E`. Alt is read with `GetAsyncKeyState(VK_LMENU / VK_RMENU)`.
- That flag has a developer hotkey (`0x7FF6D2C7B900`, case 0x80: slot 264 toggles it) gated on show mode or tournament mode with `0x7FF6D404F4B0`; not used here.
- Shutdown (`0x7FF6D2B07400`) unhooks both handles when non-zero and ignores the result.

## How the plugin lifts it

`retail_menu.asi` reads the handle at RVA `0x3285B00` while it waits for the login and calls `UnhookWindowsHookEx` on it once. No game memory is written: the game's own shutdown unhook then fails harmlessly. Alt+F4 also works again, which closes the game without its exit prompt.

## Results

- Run `main-menu/20261105T154138.453187Z`: the plugin saw the handle 2 s after it loaded (the input setup runs after the plugin's remote load) and `UnhookWindowsHookEx` returned TRUE (`build/retail_menu.log`, 11:41:43). The login skip and the menu config vars ran as before.
- Not checked by me: pressing Alt+Tab and the Windows key in the running game needs foreground input, so the user tests it.
