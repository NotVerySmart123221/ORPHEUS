# Unattended run summary (2026-11-03, second run)

Plan: `tasks/unattended-plan-2026-11-03.md`, items 1-8, then the next steps A-D picked in its "After items 1–8" section. Every code change has its own local commit and was checked in game unless the table says otherwise. Nothing was pushed. No TEMP probes are left in the tree, and Overwatch.exe is not running.

## Items 1-8

| # | Item | Result | Commit | Evidence |
|---|---|---|---|---|
| 1 | Paris collision | done: triangle count from the footer's mesh record | dad9813 | `assault-capture/paris-attack.png` |
| 2 | Zarya's beam | not drawn; graph links decoded, notes | 294dfb5, 880424e | `assault-capture/zarya-no-beam.png` |
| 3 | Draw abilities and heals | partial: Helix draws but lands at the bot (not committed as code) | 483f305 | `assault-capture/bot-helix-floor.png` |
| 4 | Round-end screen | score done (v7899 / v7901); round number and time not found; no victory/defeat screen | 6f4558b | `assault-capture/round-complete-score.png` |
| 5 | PLAY menu: Practice vs. AI card and difficulty | **blocked** (but see B): the card layout is chosen by the client's tournament flag (`--tank_TournamentMode`); hiding the two locked cards or showing the native Practice vs. AI card needs the retail route or a client patch | 53095c5 | `research/play-menu.md` |
| 6 | Capture-progress HUD | decoded, not lit: the widgets read `v1030[v18108].v11947`; v18109/v16927 are entity arrays. `statescript.Array` added; probe accepted, padlocks unchanged | 47ffbc6, 5ebbae8 | `research/assault-mode-graph.md` |
| 7 | Non-Soldier player hero | scope doc (Cassidy first) | b8f3014 | `research/non-soldier-hero-scope.md` |
| 8 | Numeric health 0/200 | option 2 closed; binding type hash found, name stripped; still parked | a5e7e27 | `research/numeric-health.md` |

## Next steps A-D

| # | Step | Result | Commit |
|---|---|---|---|
| A | Strafing hero bots keep facing their target | done, unit-tested; live check inconclusive | a6a13a1 |
| B | Tournament Match card as the vs-AI entry | **done**: clicking the (padlocked) card sends group `0x692C511B`; the server answers with the handoff, so the card now opens the vs-AI match and the Practice Range card the Range | this commit |
| C | Round-complete: more vars | done: "CAPTURE PROGRESS" row sent on a loss (v2198, v8018); "FINAL ATTEMPT!" found (v8027) | 9bbb4ef |
| D | Zarya memory check | inconclusive: the reader finds no idle remote weapon instance of any hero, Zarya's included | bb9a04a + this commit |

## Left / next

1. PLAY menu: hide the Tournament card's padlock, pick the difficulty in game (the launcher flag sets it today); the native Practice vs. AI card needs the retail route.
2. Round-complete round number and time remaining; a victory/defeat screen.
3. Capture HUD: which entity the v18109/v16927 arrays should name.
4. Zarya: a reader that finds remote weapon instances, then test the beam slots one at a time.
5. First non-Soldier hero (Cassidy), per the scope doc.
