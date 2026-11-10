# Unattended run summary (2026-11-03, third run)

Plan: `tasks/unattended-plan-2026-11-03b.md`. Mid-run the user allowed ASI plugins for client code changes; step 1 used that. Every code change has its own local commit and was checked in game. Nothing was pushed. No TEMP probes are left; the game is closed.

## Steps

| # | Step | Result | Commit | Evidence |
|---|---|---|---|---|
| 0 | String scan | done: "DEFEAT" only in `0x027F`, "EASY" only in `0x0BE9`; the other Victory/Defeat/Difficulty strings are in no graph | - | `.scratchpad/victory-scan.txt` |
| 1 | Difficulty in game | **done via the retail menu**: `tools/asi/retail_menu.asi` + `run.py --asi`; Training > Practice vs. AI > EASY / MEDIUM / HARD queues natively, the lobby answers queue request `0x1C6EC712` (0x70 / 0x71 / 0x72) with the hosted map at that difficulty | d765fbb | `main-menu/retail-vs-ai-hard.png`, `research/play-menu.md` |
| 2 | Tournament card padlock | superseded by step 1 (the retail menu has no Tournament card); open for the plain tournament route | - | - |
| 3 | Victory/defeat screen | partial: the banner is graph `0x027F` (VICTORY! / DEFEAT / DRAW); it picks VICTORY when its input v7525 equals the local team. Its inputs are v7525, v2328, v13784. Not referenced by code, entities, game modes or maps; the graph scan (who starts it) is below | - | `research/end-of-match.md` |
| 4 | Round-complete booleans | done (write-up): bit 8 (v7967) shows "SWITCHING SIDES"; bits 0 and 1 are not the round number or time | 3afd53e | `assault-capture/round-complete-switching-sides.png` |
| 5-7 | Helix retest, capture HUD arrays, Zarya reader | not started this run (time went to steps 1 and 3) | - | - |
| 8 | Cassidy, code only | step 1 done: `Rifle` takes the hero kit's cadence/magazine/reload; step 2 (weapon instance per hero) left, it needs owner-state probes | see git log | `tools/lobby/test_rifle.py` |

## Findings worth keeping

- `0x692C511B` (group 37) is a presence ping on every menu change, not a Tournament-card request. The tournament-route lobby still treats it as the card click; with `--retail-menu` it is ignored.
- Config-var expressions: opcodes `0E i` (call config var), `1E` (not), `02 n` (and: jump n when false), `03 n` (or), `04 n` (ternary), `22`/`25`/`26` (compares), `00` (end). Arrays are a u64 count and a u64 offset into the asset's dynamic area (header word 7).
- Runtime config-var classes: STU class registration (last argument) and runtime registration share a pointer; the runtime record's first field creates the object, whose first qword is the vtable; slot 4 evaluates.

## Left / next

1. Step 3: start `0x027F` at match end with v7525 = the winning team (needs its starter; see `research/end-of-match.md`).
2. Steps 5-7 carry over unchanged; step 8's weapon instance per hero (scope doc step 2) next.
3. Retail PLAY screen: Quick Play / Arcade cards hidden, Game Browser locked ("Only available to the group leader.").
4. Step 10 (all heroes in the menus): `research/hero-menu.md`.
