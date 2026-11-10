# Fork review and import — 2026-11-08

Source: `<old-workbench>` (branch `codex/bot-hit-feedback`, HEAD `4655246`, plus an uncommitted batch). It was itself forked from this repo at `0396274` on 2026-11-07, including our then-uncommitted D.Va work (byte-identical in its commit `bcf0759`).

## Import

1. Our uncommitted D.Va work was stashed (`git stash list`: "pre-fork-import backup"); it is identical to fork commit `bcf0759`, so nothing was lost.
2. `main` fast-forwarded to the fork's `4655246` (12 commits, no deletions).
3. The fork's uncommitted batch (35 modified + 205 untracked files) was applied; all 240 files were hash-checked equal to the fork.
4. `ida/Overwatch-runtime.dump.i64` and `Overwatch-state-analysis.dump.i64` replaced with the fork's newer copies (built on ours). Older runtime copy kept as `*.pre-fork.i64`.
5. ASI plugins rebuilt; `tools/lobby/data/` (ignored, regenerated per install) was not copied.

## What the fork did

| Area | State | Evidence |
|---|---|---|
| D.Va Defense Matrix: cannon suppression, eats hostile bot shots | done, live-checked | research/live-verification-2026-11-07 |
| Reliability: wall-probe over-speed, beam locks outliving body/weapon, marker cache poisoning | fixed with regressions | fork-review-2026-11-07.md |
| Soldier presentation: native Helix, Biotic Field ring, spawn voice, Sprint transitions, 1P fire animation | done, live-checked | research/soldier-runtime/five-items-progress-2026-11-07.md |
| Native bot nameplates, hitmarkers, ELIMINATED feed, idle-pose hitboxes | done, live-checked | research/bot-nameplates-2026-11-07.md |
| JAM Bool bit-boundary fix (lost feedback after eliminations) | done | soldier-runtime/controller-reload-2026-11-07.md |
| Earned accounts (`--progression --name`): XP, boxes, credits, purchases | done, live-checked in a QA profile | research/progression/implementation-2026-11-08.md |
| Login bypass (graph 0194) replacing the Pod/Name vtable hack; randomized menu hero + owned skins | done | research/login-bypass-2026-11-07.md |
| 32:9 aspect ratio plugin (opt-in) | done, live-checked | research/aspect-ratio-32-9.md |
| Anti-debug / hardware-breakpoint research, `hw_detector_probe.py` | research only | research/anti-debug-2026-11-05.md |

## Shortcomings found

1. **Mercy "pistol/staff mismatch" was misdiagnosed.** The capture shows the staff (same model as our d4e4c96 captures). The real cause: the run healed an ally Reinhardt, and `_send_bot_weapons` skipped every bot whose hero has no `WEAPON_GRAPHS` entry, so heal status 0x0079 (the beam) never reached the client. Found by a headless bit-diff of Mercy's weapon frame against d4e4c96 (identical apart from the per-frame cooldown flag) and the session logs (verified runs used a Soldier: 76 ally). **Fixed** here; near end still wrong for such allies (open).
2. `test_retail_menu.c` covered the login bypass but was never built or run. **Fixed** (`build.bat`).
3. `test_target.py` failed when collected alone (import order). **Fixed**.
4. STATUS.md test count was stale (353 vs 387). **Fixed** (388 now).
5. `retail_menu.c` hardcodes `LOGIN_NAME "Player"`; `--name` reaches the server records but not the plugin. Probably cosmetic; unchecked.
6. `progression.py` `xp_needed` is a stub (2,000 then 20,000). Saved accounts validate `xp < xp_needed(level)`, so changing the curve later can reject existing profiles; needs a migration when the real curve lands.
7. `run.py` reuses `tools/lobby/data/progression_catalog.json` when its version matches, even from another install.
8. `run.py` dropped the 3 s wait before remote `LoadLibraryW`; reliable in this session's four launches, not stress-tested.
9. `practice.pending_rewards` catches only `sqlite3.Error`; a `ValueError` from `settle` would retry forever. The only producer cannot emit invalid values today, so left as is.
10. Docs: `research/progression/*.md` link absolute `<old-workbench>/...` paths; `research/login-skip.md` still describes the removed vtable approach.
11. Global per-frame cooldown flag (`soldier.cooldown_left`) is verified for Soldier only; other heroes' ability countdowns were not re-checked live.

## Improvements, in order

1. Mercy near end for unarmed allies: run tracer.asi's STU_9932D728 evaluator probe on a Reinhardt and a Soldier ally and compare var27 / var49 resolution (the 2026-11-07 Mercy heal method).
2. User Range session and sign-off (roadmap phase 1 gate), then restore `Settings_v0.ini`.
3. Re-check ability countdowns on a few non-Soldier heroes after the per-frame flag change.
4. Pass `--name` to the plugin or confirm the hardcoded name is invisible; fix the stale login-skip and absolute-path docs.
5. Give progression a schema/curve version before replacing the XP stub.

## Graph

Graphify was refreshed code-only (94 changed code files; 79 docs and 496 images not re-extracted): 2,630 nodes, 5,852 edges, 132 communities. God nodes: `Match`, `TargetTests`, `TrainingTarget`, `_match()`, `PracticeRange`, `SoldierTests`, `Player`, `Instance`, `Graph`, `Rifle`. `Match` (community "Match Orchestration", 133 nodes, cohesion 0.034) remains the hub joining transport, weapons, abilities, bots and progression.
