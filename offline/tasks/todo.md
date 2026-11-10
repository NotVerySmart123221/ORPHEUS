# Proposed implementation tasks

Practice Range and Practice vs AI run end to end: 31 heroes spawn, walk, fire and use ability HUDs; Range bots and hero bots shoot back, die and respawn; Soldier's Helix, Biotic Field and Visor and 14 projectile abilities land server-side. What is still broken or approximated is kept in [open issues](../research/open-issues-2026-11-03.md) (updated 2026-11-03). Evidence: [range report](../research/practice-range-entry/REPORT.md), [Soldier report](../research/soldier-runtime/REPORT.md). See [plan.md](plan.md) for architecture criteria.

## 1. Record a map-entry request

- [x] Extend existing logs with announced CRC-to-index mapping; keep names, credentials and packet bodies out of routine lobby logs.
- [x] Record bounded Practice Range entry: Training disabled; Play card emits 24000 kind 2 / flags 4.
- [x] Preserve working menu login and normal exit.

Verification: focused protocol tests plus one saved trace per attempted UI path. Dependencies: none. Likely files: `tools/lobby/lobby.py`, its tests, one research note. Small implementation; investigation duration unknown.

## 2. Resolve one map's local resources

- [x] Resolve retail Practice Range map/header GUIDs and CKeys from local CMF metadata.
- [ ] Verify readable selected dependencies against their available content hashes; report missing/encrypted dependencies explicitly.
- [x] Save resource identities with the retail build fingerprint.

Verification: read-only resource extraction with hash checks. Dependencies: task 1 identifies the intended entry path; asset inspection can start independently. Likely files: one asset-inspection tool with its changelog/checks and one research note.

## 3. Establish the retail world-entry route

- [x] Confirm request/handoff schemas against live retail registrations and verify UDP/world initialization.
- [x] Establish local UDP instance service route; no native client call or injected integration needed for map entry.
- [x] Native call/thread requirement inapplicable to the selected network route; none attempted.

Verification: annotated IDA results and a bounded runtime trace where needed. Dependencies: tasks 1–2. Likely files: IDA database and one research note. Research task; split any implementation after this decision.

## Checkpoint: route evidence

- [ ] Review the route decision, resource availability and test evidence before committing to a gameplay implementation.
- [ ] Break the selected route into changes touching no more than about five files each. Do not lower the menu regression checks to make progress.

## 4. Load and unload one map

- [ ] Enter the original Practice Range through the supported route with rendered geometry/camera, collision and verified world state.
- [x] Leave and return to Play without a crash, with FIN and match cleanup.
- [x] Repeat the entry/selection/first-person cycle and capture evidence.

Verification: world/resource state plus screenshots and exit result. Dependencies: task 3 checkpoint. Files/scope: determined by route decision; decompose before coding.

## 5. Control one hero

- [x] Verify Soldier selection, possession, camera response and walking; input traces confirm forward, jump and FIRE reception.
- [x] Spawn Soldier: 76 with working camera, movement, jump and collision (server movement is flat: no map gravity).
- [x] Verify native jump flight/landing and collision against a spawn-room wall; map-wide/server collision remains unresolved.
- [x] Restore held-rifle rendering and acknowledge delivered body state.
- [x] Fire the primary weapon with observable projectile/hitscan behavior (hitscan with tracer and recoil; Helix Rockets fly).
- [x] Retain map teardown and clean exit.

Verification: repeatable input scenario and observed movement/fire results. Dependencies: task 4. Files/scope: determined after native player path is recovered.

## 6. Damage a stationary target

- [x] Spawn the original training target with explicit prototype200HP, using supported health/team components and stationary movement.
- [x] Native primary-fire input produces ten sphere hits and health200→0; original hitboxes/map occlusion remain unresolved.
- [x] Observe native removal and fresh-entity recreation after3seconds; full death animation remains unimplemented.

Verification: adjacent before/after health observations and entity lifecycle checks. Dependencies: task 5. Reference beta damage and lifecycle checks; validate retail integration separately.

## 7. Complete a one-bot combat loop

- [x] Add movement, aiming and firing for one enemy; observe damage in both directions (Range shooters and Practice vs AI hero bots).
- [x] Verify diagnostic player/bot removal and fresh-entity respawn with restored health; original death animation/feedback remains pending.
- [x] Repeat combat and return to the menu without leaking match state.

Verification: repeatable combat scenario with health/lifecycle observations and clean exit. Dependencies: task 6. Decompose controller and respawn work into separate changes before implementation.

## Checkpoint: first playable slice

- [x] Recheck menu → map → hero → target/bot → death/respawn → menu.
- [x] Review actual limitations before expanding hero kits, objectives or map coverage: see [open issues](../research/open-issues-2026-11-03.md).
