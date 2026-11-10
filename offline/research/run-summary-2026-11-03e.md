# Unattended run summary (2026-11-03, fifth run)

Order asked: item 3 (menu polish), then 1 (more heroes), 2 (banner/round values, via headless `idat.exe`), 4 (old plan steps 5-7). Every code change has its own local commit and was checked in game. Nothing was pushed. No TEMP probes are left; the game is closed.

## Steps

| Item | Result | Commits | Evidence |
|---|---|---|---|
| 3 | Menu polish: equips kept across logins, PLAY cards shown, XP strip and anniversary scene probed | 990264a, 0288493, cdbd0c5, f9af9ce, 2e29124 | `research/hero-menu.md`, `research/play-menu.md` |
| 1 | **20 heroes playable** (weapon, HUD portrait, magazine, fire, reload): Soldier, Cassidy, Roadhog, Torbjörn, Sombra, Mei, Bastion, Lúcio, Reaper, Ana, Zenyatta, Tracer, Pharah, Widowmaker, Junkrat, Orisa, Wrecking Ball, Ashe, Echo, Baptiste, Mercy (blaster) | 9899526, b805838, 08c4718 | `research/main-menu/kit-*.png`, `research/non-soldier-hero-scope.md` |
| 2 | Banner state `STU_B9898052` decoded with idat (1 flag bit + 8-bit counter); sent, still no banner. Round-complete bits mapped (v8027 = "FINAL ATTEMPT!", v8017 hides the screen); round number and time still not found | 20559f6 | `research/end-of-match.md`, `research/assault-mode-graph.md` |
| 4.5 | Helix sender re-added and committed (second Volley + ch2 record); Helix damage lands, the rocket was not seen (bots not on screen) | 35e7105 | `research/bot-fire.md` |
| 4.6 | Capture HUD arrays naming the objective entity or the player: accepted, padlocks unchanged | b803374 | `research/assault-mode-graph.md` |
| 4.7 | Zarya reader: not started (time) | - | - |

## Findings worth keeping

- var49 (the weapon entity) binds its state with as many bits as the graph has states in all, not owner states. Lúcio (65 states, 61 owner) crashed with 6 bits (assert `0xF0E7E025(85)`, a misaligned frame).
- Stacks need their 2-bit payload; StateAbility, TrackRay and `STU_DEBD057B` states sent bare misalign the frame (Sombra). `tools/assets/weapon_kit.py` now leaves out undecoded-payload states.
- A hero's weapon graph is the body's weapon graph with a WeaponVolley; the primary is its lowest Volley state in every hero checked.
- Headless `idat.exe -A -S<script> -L<log> <db>.i64` decompiles in about a minute (`.scratchpad/idadecomp.py` pattern: env `IDA_ADDRS`, `IDA_OUT`).

## Left / next

1. Heroes not playable yet: Zarya (Volley is the alt fire), Hanzo, D.Va, Sigma (no magazine), Symmetra, Moira (beams), Genji, Doomfist (no fire button found), Reinhardt, Winston, Brigitte (no Volley). Ashe's rifle model does not show.
2. No abilities, ult or reload animation for non-Soldier heroes.
3. Banner: try 027F slot 1 (`STU_9D7BF987`) or after the round-complete screen closes.
4. Round number / time remaining: not 0C35 presence; trace the UI's format arguments.
5. Helix rocket visual; Zarya reader (old step 7).
