# Shared Behavior Contracts (`rust`)

ORPHEUS stores language-neutral JSON contracts under `fixtures/behavior/`. Cross-language consumers compare Python (`main`) and Rust (`rust`) through production code paths. Rust-only regression contracts identify their narrower scope explicitly. Public fixtures use synthetic inputs.

The Rust-only `script-actions-v1.json` adds 15 historical assignment/branch cases pinned to `f763beb`. Both `orpheus-statescript` public workspace lanes run `script_init::tests::actions_contract::actions_preserve_the_pinned_synthetic_reference_contract`; it is not registered in the Python/Rust differential job. See the [action development guide](STATESCRIPT_DEVELOPMENT.md) for provenance, current module boundaries and the synthetic benchmark.

The Rust-only [runtime state fixture](../fixtures/behavior/script-runtime-state-metadata-v1.json) checks class-level plain fallback and instance-level client scope through the production loader and VM. Native coverage compares both wire views and preserves codec validation.

The Rust-only [inherited base factory fixture](../fixtures/behavior/script-inherited-base-factory-v1.json) checks the base lifecycle at two new graph placements in both tagged and legacy bundles. It also checks client-only exclusion and diagnostics for an unrelated unknown class.

The Rust-only fixtures for [parent-owned state](../fixtures/behavior/script-parent-owned-state-v1.json), [native direction](../fixtures/behavior/statescript-native-direction-v1.json), and [placement collection](../fixtures/behavior/script-tagged-placement-collection-v1.json) use synthetic graphs or host inputs. They cover child cleanup, pitched direction lookup, and ordered scene placement queries. The tests check unsupported operands and missing host data at the same entry points.

The [meter activity contract](../fixtures/behavior/script-meter-body-activity-v1.json) checks gain and deferred health awards across activity changes. The [owner activity contract](../fixtures/behavior/script-owner-room-activity-v1.json) exercises the native room and body factory at startup, death, and same-wire replacement.

The Rust-only [base action fixture](../fixtures/behavior/script-base-action-placement-v1.json) runs the same valid continuation at two original placements. It also checks malformed action markers and an invalid remote slot on an action.

The Rust-only [stat state fixture](../fixtures/behavior/script-stat-state-facts-v1.json) checks the lifetime of both validated stat selectors when the bound actor has no player stats record. It covers the distinct stat and combat host queries and rejects unknown stats context and malformed selectors.

The Rust-only [frame wait fixture](../fixtures/behavior/script-frame-wait-v1.json) checks the existing VM clock for literal waits at two original placements and in both server-running scopes. It covers zero duration and rejects malformed durations, rearm and remote bindings.

The Rust-only [class declaration fixture](../fixtures/behavior/script-authored-classes-v1.json) uses two invented graphs to check shared class behavior through the production loader and VM. It runs in both workspace lanes and needs no prepared runtime.

The Rust-only [local message fixture](../fixtures/behavior/script-local-message-v1.json) checks parameter capture, continuation, graph-order delivery and client-only scope through the production VM. It also checks that a malformed receiver binding leaves earlier outputs and the active receiver branch intact. It needs no prepared runtime.

The Rust-only [room message fixture](../fixtures/behavior/script-room-message-v1.json) checks typed admission and payload capture, explicit listener scope, and delivery in the recipient's next VM frame. Native room tests cover generation checks and the shared request order.

The Rust-only [projected message topics fixture](../fixtures/behavior/script-projected-message-topics-v1.json) checks explicit client scope, loading conflicts, ordinary state entry and exit, and server dispatch exclusion. The native test compares owner and observer activation frames and removal deltas with the existing send codec.

The Rust-only [room cue fixture](../fixtures/behavior/script-room-cue-v1.json) covers authored audience selection and host admission. Native coverage checks selected player generations and the reliable notice path.

The Rust-only [health action fixture](../fixtures/behavior/script-health-change-v1.json) checks signed health request admission, continuation, and rejection diagnostics. Room tests cover player and bot settlement, stale body identity, and death and health updates.

The Rust-only [ordinary health pool placement fixture](../fixtures/behavior/script-base-health-pool-placement-v1.json) exercises the same health pool shape at two positions through the VM and Vitals. It covers registration, remaining health on exit, cleanup, and rejection of an unsupported shape.

The Rust-only [health query target fixture](../fixtures/behavior/script-health-target-v1.json) checks current and maximum queries for ordinary health and selected pools. An unbound target yields zero without a host read, including an integer-zero reset in a direct target slot. Missing or malformed operands and bound targets with unavailable vitals retain their diagnostics.
[Pinned selected-pool reference](https://github.com/leafall903/ORPHEUS/blob/c73e5a499ec73d499c97eaae46357af30c9e4b39/src/ow174/game/script/scene.py#L2714-L2724), [pinned ordinary reference](https://github.com/leafall903/ORPHEUS/blob/c73e5a499ec73d499c97eaae46357af30c9e4b39/src/ow174/game/script/scene.py#L2778-L2793)

The Rust-only [button lock fixture](../fixtures/behavior/script-button-lock-v1.json) exercises scoped input suppression and release through the production VM. The native room integration also starts a timed lock through a directed message.

The Rust-only [movement scale fixture](../fixtures/behavior/script-movement-scale-v1.json) checks overlapping movement modifiers, timing and cleanup through the production VM. Native room coverage checks when a message-triggered modifier enters and exits the receiver's movement timeline.

The Rust-only [movement edge fixture](../fixtures/behavior/script-movement-edges-v1.json) checks flag writes before takeoff and landing, landing before a renewed takeoff, and event deduplication when the VM samples a retained pose. A renewed takeoff in the air requires a new source movement frame. Unknown movement samples retain a diagnostic.

The Rust-only [stack claim fixture](../fixtures/behavior/script-stack-claim-v1.json) checks priority, ties, scope, dependent values and release through the production VM. The native room regression combines stack admission with a tag filter and verifies the lifetime of the input and movement effects.

The Rust-only [server root allocation fixture](../fixtures/behavior/script-server-root-ids-v1.json) checks root identity, installation order and exhaustion at the boundary of the public and private ranges. Native projection coverage compares owner and observer wire bytes with an active private meter and a public child.

The Rust-only [authored resource meter fixture](../fixtures/behavior/script-authored-meter-v1.json) checks body-specific settings on one shared graph. It exercises settled health events, affiliation filtering, passive gains and saturation in the production loader and VM.

The Rust-only [local health outcomes fixture](../fixtures/behavior/script-local-health-outcomes-v1.json) checks actual damage from AI callbacks and capped field healing. Room tests also exercise source provenance across body replacement.

The Rust-only [host health event fixture](../fixtures/behavior/script-host-health-events-v1.json) checks authenticated delivery through the ordinary VM event timeline, typed receipt parameters, and separation from script-sent messages.

The Rust-only [mode ruleset fixture](../fixtures/behavior/script-mode-rulesets-v1.json) checks mode isolation, actor overrides, typed values, and explicit null results through the production StateScript loader.

The Rust-only [radial health fixture](../fixtures/behavior/script-radial-health-v1.json) checks the shared spatial filters, distance falloff, and action continuation. Native room tests cover atomic batch admission and generation checks.

The Rust-only [interval and health fixture](../fixtures/behavior/script-interval-health-v1.json) composes the authored interval and health action. It exercises scheduling, skipped frames, live operands, cancellation and reactivation through the component VM.

The Rust-only [target child fixture](../fixtures/behavior/script-target-children-v1.json) checks target ordering, duplicate removal, child reuse, invalid and unavailable samples, and cleanup on target removal, abort and teardown. A fatal work-budget case also checks that a halted component releases the child's host effect once and discards an already-due timer.

The Rust-only [entity spatial fixture](../fixtures/behavior/statescript-entity-spatial-v1.json) checks authored position and direction reads at two host snapshots. An in-memory VM test also exercises these operands through the ordinary component message queue.

The Rust-only [bound body angle fixture](../fixtures/behavior/script-bound-body-angle-v1.json) checks owner and NPC host reads across modes using the same packed pose samples. It distinguishes an unbound target, an unavailable foreign body, and an out-of-range angle.

The Rust-only [room actor pose fixture](../fixtures/behavior/script-room-actor-pose-v1.json) checks frozen movement, aim, and eye anchors across movement, retirement, death, and generation changes. It also covers the scoped view installed on consecutive replacements in the same room frame.

The [native eye position fixture](../fixtures/behavior/statescript-native-eye-position-v1.json) checks a finite anchor read and unavailable-host diagnostics. The [pose demand fixture](../fixtures/behavior/script-native-pose-demand-v1.json) checks when the loader requests room poses for native expressions.

The Rust-only [owner startup spatial fixture](../fixtures/behavior/script-owner-startup-spatial-v1.json) checks a one-shot query through scene admission before body publication. It covers unavailable context, an available empty room, and another actor's frozen pose with a separate owner command clock.

The [local combat accumulator fixture](../fixtures/behavior/script-local-combat-accumulator-v1.json) checks that the VM keeps the state lifecycle and skips tick operands when the host confirms the context is absent. Available and unknown contexts remain diagnostic.

The Rust-only [player host facts fixture](../fixtures/behavior/script-player-host-facts-v1.json) checks known body and controller presence, unknown entities, the team slot assigned at spawn, and membership counts from the admitted player roster.

The Rust-only [owned entity count fixture](../fixtures/behavior/statescript-owned-entity-count-v1.json) distinguishes an explicitly unbound owner, an unavailable ownership registry, and a known count. The native startup regression uses the production human owner selector and verifies the assignment completes without an AI controller.

The Rust-only [radius query fixture](../fixtures/behavior/statescript-entities-in-radius-v1.json) checks the inclusive boundary, sorting, deduplication, and optional team, directional, and static visibility filtering. Room coverage checks shared per-frame samples in team and free-for-all modes.

The Rust-only [local command fixture](../fixtures/behavior/statescript-local-command-v1.json) checks that a known missing process-local input remains absent across remote owner commands. Unknown host context and malformed operands still yield diagnostics.

The Rust-only [segment query fixture](../fixtures/behavior/statescript-segment-clear-v1.json) checks endpoint validation, evaluation order, work limits, and the distinction between a blocked segment and unavailable geometry.

The Rust-only [assignment fallback fixture](../fixtures/behavior/script-assignment-client-fallback-v1.json) checks config group selection and flattening through authored client-only wrappers, including opaque client values and scalar defaults.

The Rust-only [room entity filter fixture](../fixtures/behavior/statescript-room-entity-filter-v1.json) checks affiliation, liveness, and tag presence for individual and spatial predicates. Component and room tests cover snapshot recovery and use of shared actor metadata.

The Rust-only [observation fixture](../fixtures/behavior/script-observe-radius-v1.json) exercises the radius query in an authored state across VM frames and through state activation and abort.

Public synthetic fixtures must never contain game binaries, raw captures, signing keys, or private credentials.

For the proposed private, per-tick Python/Rust gameplay comparison and its evidence limits, see [Golden gameplay traces](GOLDEN_TRACE.md).

## Opt-in Soldier wire reader check

The Sprint HUD variable check reads Rust-produced prepared Soldier chunks with the Python client-reader model at the same pinned commit as `shared-behavior-contracts`. Supply a clean Python `main` checkout at `b28139d4fa230480c0cea147d369724f97569c30` explicitly; the scripts verify its contents before import and do not download it. These commands use isolated Python and do not require changes to CI:

```sh
# From an existing Python repository containing the pinned commit:
git -C /path/to/python-repository worktree add --detach \
  /path/to/pinned-python-checkout b28139d4fa230480c0cea147d369724f97569c30

# Invented frames only; no prepared runtime needed.
ORPHEUS_PYTHON_REFERENCE=/path/to/pinned-python-checkout \
  python3 -I -S -B -m unittest discover -s scripts/tests -p test_sprint_wire.py -v

# Separately prepared target-tagged 104319 Practice Range runtime required.
python3 -I -S -B scripts/check_sprint_wire.py \
  --python-reference /path/to/pinned-python-checkout --runtime /path/to/runtime
```

Without `ORPHEUS_PYTHON_REFERENCE`, the synthetic test is explicitly skipped; a checkout at the wrong revision or with modified reference sources fails. The prepared check writes private chunks only to a fresh temporary directory, which it removes. It checks the direct owner `BodyStream` rather than queued transport, and does not establish original-client behavior or state-link coverage.

---

## Cross-Language CI (`shared-behavior-contracts`)

In [`.github/workflows/public-checks.yml`](../.github/workflows/public-checks.yml), the `shared-behavior-contracts` job checks out the pinned Python `main` reference alongside this `rust` checkout and feeds **`rust`'s own `fixtures/behavior/` files** to both implementations:

```powershell
# Python consumer side (run from a main checkout with ORPHEUS_BEHAVIOR_FIXTURES pointed at rust)
python -S -B -m unittest tests.public.test_behavior_contracts tests.public.test_world_query_contracts tests.public.test_profile_settings_contracts -v

# Rust consumer side
cargo test --locked -p orpheus-wire jam_framing::tests::shared_framing_contract_survives_every_fragmentation -- --exact
cargo test --locked -p orpheus --test world_queries
cargo test --locked -p orpheus --test profile_settings
```

### Standard Fixture Schema

| Field | Purpose |
| --- | --- |
| `schema_version` | Contract schema version (`1`) |
| `case_id` | Stable semantic case identifier |
| `target_build` | Client build (`104319`) |
| `source` | Pinned source commit and code paths establishing expected results |
| `evidence` | Evidence category (e.g. `synthetic-reference`) |
| `client_acceptance` | Live client status (`pending` or recorded session notes) |
| `input` | Explicit inputs (hex strings for byte slices) |
| `expected` | Expected output bytes, decoded frames, errors, or state snapshots |

---

## Contract Inventory

### 1. Cross-Language Contracts Executed in Hosted CI

| Fixture | Scope | Consumers |
| --- | --- | --- |
| `jam-framing-v1.json` | JAM stream cipher framing across all byte splits and 1-byte feeding | Python `test_behavior_contracts` + Rust `orpheus-wire` |
| `world-queries-v1.json` | Strict World query semantics (`origin`, `position`, foreign vars, missing vs. null vs. unavailable service) | Python `test_world_query_contracts` + Rust `--test world_queries` |
| `profile-settings-v1.json` | Settings packets (`22200`–`22204`), `+0x130` int32/int64 bounds, save/reload & `20802` wire output | Python `test_profile_settings_contracts` + Rust `--test profile_settings` |

### 2. Synced Python Reference Fixtures from `main`

Mirrored under `fixtures/behavior/` so `rust` maintains a strict superset of `main`:
- **Verified by Python CI in `shared-behavior-contracts`** (Rust consumers tracked in [#61](https://github.com/leafall903/ORPHEUS/issues/61)):
  - `seismic-slam-wave-v1.json` (shockwave reach sampling & candidate dedup)
  - `seismic-slam-placement-v1.json` (airborne selector states & MoveTo handoff)
  - `body-short-action-retry-v1.json` (short frame-action stamps, loss/retry & ACK retirement)
  - `server-only-movement-v1.json` (channel-2 throttle mods & owner record tail window)
- **Runtime-backed Python fixtures** (verified on `main` via `tests/runtime/`):
  - `player-voice-v1.json`, `wheel-emote-presentation-v1.json`, `wheel-emote-weapon-v1.json`

### 3. Native C++, Room Policy & Preview Contracts

| Fixture | Scope & Consumer |
| --- | --- |
| `owner-first-command-anchor-v1.json` | First command admission with no body, a new initialized body, retransmissions and continuing input history. The native room regression uses the same fallback rule as the frozen Python command queue. |
| `gallery-preview-policy-v1.json` | C++ client relay NPC preview resource fallback & 64-node tree pruning (`python3 -B scripts/check_gallery_preview_policy.py`) |
| `room-isolation-v1.json` | Encrypted worker room fault/panic retirement while sibling rooms stay live (`encrypted_worker_satisfies_room_recovery_contract`) |
| `junkenstein-validation-v1.json` | Normal Cassidy validation overrides (`--junkenstein-validation`) & post-AI immunity damage routing |

### 4. StateScript VM, Combat & Junkenstein Contracts (`38` Exact CI Checks + Remaining Helper Scripts)

Checked individually by `--exact` test name in `.github/workflows/public-checks.yml` (the `main`-compatible subset was ported back to `main` under [#61](https://github.com/leafall903/ORPHEUS/issues/61) in `tests/public/test_statescript_*.py`, retiring 19 superseded `scripts/check_*_contract.py` helpers; only `check_body_field_contract.py` and `check_spatial_owner_absence_contract.py` remain here pending Draft [#32](https://github.com/leafall903/ORPHEUS/pull/32)):

- **Portable VM core (`-p orpheus-statescript`, `script_init::tests::*`):** `shared_unkeyed_array_contract`, `shared_corpse_retirement_contract`, `shared_contains_condition_contract`, `shared_voice_lifecycle_contract`, `shared_nested_destination_contract`, `shared_unbound_loadout_action_contract`, `shared_foreign_notification_contract`, `shared_spatial_owner_absence_contract`, `shared_ability_press_window_contract`, `shared_stack_admission_contract`.
- **Portable expressions & foreign reads (`-p orpheus-statescript`, `script_expr::tests::*`, `script_entity_state::tests::*`):** `shared_engine_setting_contract`, `shared_foreign_component_contract`, `shared_velocity_contract`, `shared_total_health_contract`, `shared_hostile_filter_contract`, `shared_combat_target_contract`, `shared_ally_filter_contract`, `shared_native_filter_tag_clauses`, `shared_body_field_contract`, `shared_body_angle_contract`, `shared_flashbang_filter_admission_contract`, `shared_absent_target_filter_contract`.
- **Host/Combat/Junkenstein integrations (`-p orpheus`, `script_init_host_tests::tests::*`, `script_enemy::tests::*`, `linked_aim::tests::*`, `game_bots::tests::*`, `script_player::tests::*`, `junkenstein_voice::tests::*`, `junkenstein_validation::tests::*`):** `shared_charge_contract`, `shared_flashbang_base_state_contract`, `shared_victory_view_route_contract`, `shared_boss_speech_contract`, `shared_captured_aim_numeric_contract`, `shared_captured_aim_lifecycle_contract`, `shared_retired_entity_contract`, `shared_self_damage_clock_contract`, `shared_flashbang_effect_dispatch_contract`, `shared_flashbang_entry_guard_contract`, `shared_flashbang_exclusion_guard_contract`, `shared_flashbang_targeted_message_contract`, `shared_flashbang_velocity_action_contract`, `shared_flashbang_horizontal_motion_contract`, `shared_deadeye_aim_ray_visibility_contract`, `shared_junkenstein_validation_contract`.

*(Note: when running the remaining helper verifier scripts under `scripts/check_*_contract.py` with `--python-reference`, they expect the pinned research revision `e1128509` where their fixtures originated.)*
