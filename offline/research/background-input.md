# Background input and capture (2026-11-02)

Session `research/main-menu/20261102T010548.011110Z`, 1280×720 windowed, exit 0.

## Results

- **Capture:** `tools/research/screengrab.py` now uses `PrintWindow(PW_CLIENTONLY | PW_RENDERFULLCONTENT)`. It captured every frame of the session correctly while the window sat in a screen corner. It cannot capture a minimized window.
- **Black frames (2026-11-06):** one Range run with the user away got all-black PrintWindow frames (no overlay either) while `GetForegroundWindow` returned 0 and foreground keys were dropped; background keys still worked. Not reproduced by switching to another desktop (fg 0, frames fine) or by a 12 s monitor standby (frames fine). Display timeout is never on AC and no screensaver is set, so the likely cause is the lock screen, untested. Fix that does not depend on the cause: `tools/asi/frame_grab.asi` copies the D3D11 back buffer before Present, and `screengrab.grab` falls back to it on a black frame (verified: 10/10 lit frames with PrintWindow forced black).
- **Menus accept background input.** Posted `WM_LBUTTONDOWN/UP`, `WM_KEYDOWN/UP` and `WM_CHAR` (no focus change) drove the whole path: Pod field → Start → name → Enter Game → Play → Practice Range → Select Soldier → Esc → Exit to Desktop → Yes.
- **Gameplay does not.** In the Range, posted left clicks and a 1.5 s `W` press changed nothing: ammo stayed 30/30 and the view didn't move. Gameplay reads raw input, which goes only to the foreground window.
- **Foreground fallback:** `gameinput.py fg-*` takes focus, sends real input and hands focus back. It now waits for 60 s of user idle time, then shows a 10 s on-top countdown that a mouse move cancels. The first attempt was cancelled by user activity, so foreground gameplay input is not yet verified.

## Consequence

Menu navigation, loading and anything the server drives (bot fire, player damage, death/respawn) can run without disturbing the user. Player-driven gameplay checks (firing, movement, abilities) need foreground input and therefore wait for user idle time, or the VM later.
