# Fork review and continuation — 2026-11-07

Later continuation: Matrix cannon suppression and hostile bot-shot interception are implemented and have [live evidence](live-verification-2026-11-07/REPORT.md); desktop initialization was repaired. The reference scanner exposes native Helix model/effect assets. Current suite:309 passing tests. See [current status](../STATUS.md), [Matrix details](dva-mech.md#continuation-matrix-mechanics), and [Soldier findings](soldier-runtime/presentation-next-steps-2026-11-07.md). The audit below describes the earlier import/reliability milestone; later updates supersede its next-work list. Full Range acceptance and Mercy's reproduced presentation issue remain open.

## Verdict

What this repo does: a loopback lobby and game simulation for offline Overwatch retail build 1.74.0.0.104319, with optional native ASI diagnostics/menu helpers. Assumed load: one local player plus bots; online play is outside the project's scope.

The fork is substantially ahead of this workspace. It provides working menu flow and a broad playable prototype, but visual activation often exceeds simulated gameplay. Finish and verify existing mechanics before adding another hero or mode. Three confirmed reliability bugs were fixed during this continuation.

## Imported state

- Source: `<repo>`; destination: this project root. Source HEAD was `0396274` (D.Va mech/Fusion Cannons).
- Copied 1,892 new files and 69 changed files, including research captures, IDA databases, Git history, and uncommitted D.Va ability work. These counts exclude Git internals, scratchpads, Python/test caches, and settings backups; the settings backup was copied separately.
- Preserved 47 destination-only files, including anti-debug research and `hw_detector_probe.py` with its test. Ten old loose IDA recovery files were moved into `.fork-import-backup/ida/` so they cannot shadow the imported packed databases; the other 37 remain in place. No source-only mirror/delete operation was used.
- Replaced destination files are retained in `.fork-import-backup/`, outside the disposable scratchpad. Existing source scratchpads and caches were not imported.
- Local commit `bcf0759` preserves the inherited working state separately from subsequent fixes. Source code and source Git history were not edited by this work.

## What the fork completed

1. **Boot and menus:** verified-build launcher, owned loopback services, menu/profile/gallery flow, Practice Range handoff, Quick Play and Practice vs. AI routes. The launcher cleans up owned processes on exit.
2. **Soldier slice:** numeric health/ammo, rifle/reload, melee, Sprint, Helix, Biotic Field, Tactical Visor, damaging bots, player death/respawn and leaving the match. Some presentation details remain open.
3. **Hero coverage:** 32 weapon-stat entries, 31 generic kits plus Soldier's dedicated code; 29 heroes have ability HUD entries including Soldier. Coverage varies by hero. Ashe can fire and reload individual rounds; Ana/Widowmaker/Ashe have scoped views; Widowmaker's charge overlay exists, though scope damage remains approximate.
4. **Native effects:** Bastion transformation/repair, Mercy weapon switching and healing/boost beams, Moira grasp, Sombra hack/tether, Zarya/Symmetra beams and several cast/projectile animations. A rendered effect does not establish the corresponding gameplay rule.
5. **Latest unfinished slice:** D.Va's mech, Fusion Cannons, two-second Boosters flight, Defense Matrix visual/resource state and an 18-shot Micro Missiles salvo. Nov 7 changes were uncommitted in the source and are included here.
6. **Bots/modes:** map-derived Range bots, collision/line-of-sight work, reduced bot jitter, hero bots and partial Uprising waves plus DEFEND-to-ESCORT transition.
7. **Research infrastructure:** ASI tracer, guard probe, screenshot fallback, developer console, live mannequin viewer and all-hero animation export. Existing captures/logs document prior live checks; they were not independently repeated in this pass.

Primary evidence: `tools/lobby/CHANGELOG.md`, `research/ads-restoration.md`, `research/roster-animation-audit.md`, `research/asi-tracer.md`, `research/dva-mech.md`, `research/bot-movement.md`, `research/uprising-mission.md`, and the corresponding runtime/tests.

## Confirmed bugs fixed here

1. **Wall probes accelerated movement.** `Mover._blocked` probes past the intended step to maintain body clearance; `_walk` used the returned clearance without a distance cap. Starting at x=4.2 beside the test wall, a walking frame requesting 0.0917 m moved 0.4 m. Fix caps clearance to the requested distance, shared by walking and flight. A regression exercises both paths. Commit: `a5ec1e6`.
2. **Beam locks outlived their weapon/body.** Mercy's staff target/states survived switching to Blaster, and Mercy/Moira target identity survived death, hero replacement and leaving. Remote bot frames could still carry the old healer, or a new Moira body could skip rebuilding states for the same target. Fix reuses existing release logic on teardown and resets identity/timing on body replacement. Tests establish real locks before each transition. Commit: `4ed478d`.
3. **Failed extraction poisoned future launches.** `run.launch` created the final PvE marker cache before its extractor succeeded. A partial cache was then skipped on subsequent starts. Fix stages output beside the destination, checks JSON, then replaces atomically; failures remove staged output. Regression covers subprocess failure, malformed successful output and retry, without launching a client. Commit: `2c893d2`.
4. **Continuation instructions were stale.** `STATUS.md` still described missing Ashe fire, scopes, Tesla effects and all healing despite newer code/evidence. Updated current status and marked the Nov 3 issue list historical. Without this correction, another session would repeat completed investigations.

## Remaining shortcomings and improvements

1. **D.Va Matrix has no interception.** It displays a shield frame but eats no shots, has no visible resource bar and permits cannon fire. Next complete slice: block cannons only while Matrix is actually active; then intercept eligible hostile projectiles within the authored volume. Test activation/release/depletion, friendly and outside-volume shots, and excluded beam/melee attacks before a live check. Reuse the current Held state and projectile loop; do not add a second combat framework.
2. **D.Va damage/body fidelity is incomplete.** Boosters lack collision damage/knockback; missiles omit authored cast delay/spread and use bind-pose launch points. The mech uses Soldier hit volumes. Decode the actual volumes and authored timing before tuning by eye. Self-Destruct, eject, pilot and Call Mech remain separate future slices.
3. **Weapon and status fidelity varies.** Scoped cadence/ammo/damage, hero-specific melee and several weapon models remain approximate. Reinhardt/Brigitte primary presentation, most movement abilities, barriers/deployables and non-Soldier ultimates are incomplete. Stun, slow, knockback, anti-heal and similar effects need simulation. Mercy's boost is visual only. Compare a single mechanic against the pinned 1.74 data, lock it down with one meaningful test, then capture the client result.
4. **Soldier phase still lacks acceptance.** Native Helix spiral, Biotic Field ring and spawn voices need presentation checks. Run the roadmap's Soldier Range acceptance scenario: full kit, all map bots, death/respawn and clean leave. User playthrough/sign-off is still required; do not mark the phase complete or restore settings before it.
5. **Protocol layout knowledge is incomplete.** `graph_layout.py` disagrees with observed presence ordering for graph `0x0079` and the count for `0x0D28`. Keep observed layouts explicit until the ordering rule is proven; add fixtures for known graphs before replacing these exceptions with a general rule.
6. **Projectile rendering recreates entities every frame.** This is a documented approximation, not evidence of original movement fidelity. A native mover needs protocol research and live comparison. Measure client-visible discontinuity and server cost before replacing the current implementation.
7. **Match/PvE flow is partial.** Victory banners, round/time presentation and several Uprising phases/payload indicators remain unresolved. Keep these behind Range completion rather than treating successful map loading as a complete mode.
8. **Research/navigation drift makes handoffs expensive.** Keep `STATUS.md` current and old notes explicitly historical. The existing `Match` coordinates transport, weapons, abilities, bots and mission flow; reuse `kits`, `target`, `collision` and existing helpers for small fixes. Extract one subsystem only when a concrete new mechanic cannot be tested at the existing seam; no broad rewrite is justified by this audit.
9. **Tests depend on import order in places.** Standalone collection of `test_target.py` fails without the `practice` vendor-path bootstrap; suite collection supplies it indirectly. Make focused-test setup explicit when touching that module next. Full-suite success also does not establish native rendering or original-game timing.

## Next work, in order

1. Re-run the Range acceptance scenario after these Python repairs, including near-wall walking/Boosters and staff stow/death/switch/leave. Acceptance: no over-speed step or retained beam, normal return to menu, and no protocol errors. Record a new capture/log before claiming live verification.
2. Continue the latest D.Va slice with Matrix cannon suppression, then projectile interception. Acceptance: depletion/release restores firing, only eligible projectiles are removed, resource state agrees with the client. Keep each change independently testable.
3. Close Soldier's presentation gaps and obtain user Range sign-off. Only then restore backed-up settings and advance the roadmap phase. Defer local progression/storage and complete Uprising until this milestone.

## Verification and limits

- Imported baseline: 297 tests passed with `python -B -m pytest tools/lobby tools/assets tools/research tools/blender -q`.
- Final suite after all three fixes: **301 passed**. Focused checks also passed: collision (8), launcher (5), and the lobby suite after beam cleanup (207, before adding the launcher regression).
- SHA-256 comparison verified all 1,961 imported new/replaced paths, allowing only the listed continuation edits and Graphify query timestamp. All 69 replaced originals and all 47 destination-only files were verified before relocating the ten IDA recovery files into backup. `.fork-import-backup/import-manifest.json` records the file lists.
- Each repaired behavior first failed its regression on the inherited implementation, then passed after the fix. One independent review agent approved the fixes; no nested delegation.
- Graphify queried the inherited graph (1,981 nodes, 4,583 edges). Vocabulary expansion used actual graph labels; BFS identified `Match`, `Player`, `Projectile`, world movement and test relationships. Example graph locations: `tools/lobby/target.py:L275` (Projectile), `tools/lobby/vendor/ow174/game/match.py:L406` (Match). These are graph-snapshot locations, not current line numbers. The graph predates recent work; code/tests supplied current facts. No rebuild or new semantic-extraction token cost was incurred.
- IDALib reported no open database sessions. These repairs concern Python code and needed no new binary analysis. Imported IDA databases remain available for the protocol/mechanic work ahead.
- No live client, ASI build/load, Blender export, complete native-code audit or exhaustive asset-parser audit was performed. Earlier ASI experiments also outgrew the roadmap's original no-injection rule; reconcile that policy explicitly before expanding native runtime experiments.
- An already-corrupt marker cache from an older launcher still needs removal before regeneration. Abrupt process termination can leave a `.tmp` file, but the final cache remains unpublished and the next extraction overwrites that temporary file.

Verdict: continue from the imported fork, retain its evidence, and finish gameplay semantics before expanding breadth. Reliability fixes are tested; live presentation and phase acceptance remain open.
