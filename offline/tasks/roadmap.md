# Roadmap and settled decisions (2026-11-01)

Agreed in a planning interview. This supersedes the scope parts of [plan.md](plan.md); [todo.md](todo.md) still holds the detailed task history.

## Goal

- End goal: an offline-only game whose headline mode is Training > Practice vs. AI.
- Later: local progress, XP and loot boxes. No storage design until the Practice Range is complete.
- Never: online play.

## Phase order

1. Practice Range, Soldier: 76 only (current phase).
2. Hero switching.
3. Practice vs. AI.
4. Tutorial (optional, last).

## Practice Range is complete when

- Soldier's full kit works: rifle, Helix Rockets, Sprint, Biotic Field, Tactical Visor, and his ultimate.
- Every bot that the 1.74 map's own data spawns is present and behaves as in the original.
- The full HUD works, including the numeric health.
- The player can die and respawn.
- Leaving returns cleanly to the menu.
- The user plays one session and signs off.

## Work order inside the phase

1. Numeric health (currently 0/200).
2. Bots shoot back; player death and respawn in normal play.
3. Server-side map collision and line of sight.
4. Abilities and ultimate, with their HUD.
5. Original hitboxes, replacing the sphere.

Each step ends with in-game proof before the next starts.

## Rules

- No game-file patches, DLL injection or process writes; gameplay lives in the local server. Running the game's own code (e.g. ASI loaders) is a later-phase experiment.
- Exception (2026-11-04): breakpoints on the running game are allowed for tracing (debugger attach; prefer hardware breakpoints, which write no memory). Clear them and detach without killing the game.
- Exception (2026-11-07): user explicitly approved `tools/asi/build/retail_menu.asi` for menu/Alt-Tab support (2026-11-08: merged at the user's request into `tools/asi/build/ow_fixes.asi` with the 32:9 choice and the read-only hitbox-drift feed) and the existing guarded `gameinput.py` for held-input verification. Launch without a timeout unless requested.
- Approximations are allowed if each is logged with its limit. Match the original where players would notice (damage, hitboxes, HUD).
- If the client contains bot behaviour code, translate it faithfully into the server. Otherwise rebuild the behaviour from the client's data, footage and docs.
- The bot roster comes from the 1.74 map data. Wiki pages may be read through their 2016–2022 revision history; web research is allowed (never TCRF).

## Working arrangement

- Runs happen on the local machine without per-run approval. A VM is only used if the user asks for one.
- The game runs in a small window. The game settings file is backed up once, and the windowed settings stay until the Range is complete, then the backup is restored.
- Before the game takes keyboard/mouse focus: a desktop notification and a 10-second countdown that the user can cancel by moving the mouse.
- Windows Firewall pop-ups are reported, never answered.
- Work proceeds autonomously in the order above, with a short report after each verified step; questions only for decisions or blockers.
- Local git repo, committed after each verified step, never pushed. IDA files and `.scratchpad` are excluded.

## Setup before step 1

1. Copy `casc.py` and `screengrab.py` from the beta project into `tools/`.
2. Initialise the local git repo.
3. Back up the game settings and switch to windowed mode.
4. Switch screenshots to a capture method that works when the window is covered.
5. Test whether the game accepts input while unfocused.
6. Check whether the client contains the bot AI code.
