# Shared Behavior Contracts (`rust`)

ORPHEUS stores language-neutral JSON contracts under `fixtures/behavior/` (`54` files total). Cross-language consumers compare Python (`main`) and Rust (`rust`) through production code paths; Rust-only regression contracts identify their narrower scope explicitly. Fixtures do not bundle game assets or captures.

The Rust-only `script-actions-v1.json` adds 15 historical assignment/branch cases pinned to `f763beb`. Both `orpheus-statescript` public workspace lanes run `script_init::tests::actions_contract::actions_preserve_the_pinned_synthetic_reference_contract`; it is not registered in the Python/Rust differential job. See the [action development guide](STATESCRIPT_DEVELOPMENT.md) for provenance, current module boundaries and the synthetic benchmark.

The Rust-only [class declaration fixture](../fixtures/behavior/script-authored-classes-v1.json) uses two invented graphs to check shared class behavior through the production loader and VM. It runs in both workspace lanes and needs no prepared runtime.

The Rust-only [local message fixture](../fixtures/behavior/script-local-message-v1.json) checks parameter capture, continuation, graph-order delivery and client-only scope through the production VM. It needs no prepared runtime.

The Rust-only [entity spatial fixture](../fixtures/behavior/statescript-entity-spatial-v1.json) checks authored position and direction reads at two host snapshots. An in-memory VM test also exercises these operands through the ordinary component message queue.

The Rust-only [radius query fixture](../fixtures/behavior/statescript-entities-in-radius-v1.json) checks the inclusive boundary, sorting, deduplication, and optional team and directional filtering. Room coverage checks shared per-frame samples in team and free-for-all modes.

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
