# Unattended run summary (2026-11-03, fourth run)

Items from the "next items" list, in the suggested order. Each code change has its own local commit and was checked in game. Nothing was pushed. No TEMP probes are left; the game is closed.

## Steps

| # | Step | Result | Commit | Evidence |
|---|---|---|---|---|
| 1 | Play the picked hero (Cassidy first) | **done**: Cassidy's revolver, HUD (portrait, 6/6, 0/225), fire, R refill, his damage (39 at 32.6 m) | 687942a | `main-menu/cassidy-revolver.png`, `cassidy-fired.png`, `research/non-soldier-hero-scope.md` |
| 2 | Victory/defeat banner (`0x027F`) | not done: as a remote instance on the mode entity it shows nothing; its trigger state `STU_B9898052` needs a payload not yet decoded | 62edc1c (doc) | `research/end-of-match.md` |
| 3 | Round number / time on round-complete | not done: the values are forwarded from the parent graph 0F4A and the title is a formatted UI string; four probes changed nothing | 62edc1c (doc) | `research/assault-mode-graph.md` |
| 4 | All heroes in the in-game hero select (added mid-run) | already true: the Range and the vs-AI (Hanamura) select both list all 32 since the login change; waiting on what "as it did normally" should add | - | `.scratchpad/cas2-select.png`, `ban1-select.png` |

## Findings worth keeping

- Owner states of another hero's weapon graph map from Soldier's by their place in the catalog tree (`.scratchpad/gtree.py` prints nodes with plug links): Cassidy worked on the first native run.
- Weapon var49 (the weapon entity) is instance-scoped in Soldier's graph but entity-scoped in Cassidy's; `StateBound` works as an entity var too.
- A SubScript's forwarded child vars (`STU_6364CD4F` list on the parent state) override anything sent as the child's presence.

## Left / next

1. Cassidy: reload animation (st9 Anim, var581), Combat Roll / Flashbang (scope doc step 6), ult meter (body graph75 var338 = 1200).
2. Next heroes: same mapping per `target.WEAPON_GRAPHS` (Reaper, Mei, Torbjörn are the next hitscan/projectile cases).
3. Banner and round values: need IDA (the idalib MCP server refused connections this run).
