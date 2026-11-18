# Native Server Architecture & Migration Guide

Target build: **Overwatch 1 retail 1.74.0.0.104319**.
This document defines the Rust workspace boundaries, simulation authorities, scripting host, optional 3D inspector, and Python-to-Rust migration rules. Track current parity and test coverage in [STATE.md](STATE.md) and open milestones in [ROADMAP.md](ROADMAP.md).

---

## Rust Contribution Rules

1. **Dedicated server authority:** Rust owns sockets, encryption, protocol codecs, tick scheduling, lifetimes, collision queries, and command validation.
2. **One room authority:** Keep bots, player bodies (`PlayerBodyState`), physical projectiles, and eventually encounter gates under `RoomSimulation`'s room-local `bevy_ecs::world::World` with generation-safe handles and explicit stage order ([#65](https://github.com/leafall903/ORPHEUS/issues/65)).
3. **Data & script-driven gameplay:** Use authored StateScript graphs (`orpheus-statescript` under `rust/crates/statescript/`) for hero/mode logic and vendored Lua 5.4 (`mlua`) for hot-reloadable patrol/AI decisions bounded per room.
4. **Fault isolation:** Errors and panics in one room retire only that room and revoke its links (`game_service/room.rs`), leaving sibling rooms running (`fixtures/behavior/room-isolation-v1.json`).
5. **Non-intrusive diagnostics:** The loopback web inspector (`--features inspector`) reads bounded, tick-stamped projections off the simulation thread without mutating match state.
6. **MSRV, portability & parity:** Keep the workspace compiling cleanly on Rust **1.85.0**, keep `orpheus-wire`, `orpheus-collision`, `orpheus-runtime`, and `orpheus-statescript` Linux-portable and free of server/ECS/socket deps, and verify shared semantics via `fixtures/behavior/*.json` ([BEHAVIOR_CONTRACTS.md](BEHAVIOR_CONTRACTS.md)).

---

## Workspace Crates & Module Boundaries

The Cargo workspace separates portable protocol/physics/packing kernels from the monolithic server crate (`Cargo.toml`):

| Crate | Path | Dependencies | Role |
| --- | --- | --- | --- |
| `orpheus-wire` | `rust/crates/wire/` | `aes-gcm`, `hmac`, `sha2`, `crc32fast`, `serde` | Bits, JAM cipher/framing/codecs, commands, game UDP framing |
| `orpheus-collision` | `rust/crates/collision/` | `lzma-rust2`, `sha2`, `serde` | `.col`/`.rays.col` cache decoding, spatial triangle grid, static rays, GJK/EPA solver |
| `orpheus-runtime` | `rust/crates/runtime/` | `flate2`, `roxmltree`, `serde_json`, `sha2` | Pack manifest validation, `--check-runtime`, `prepare_server_runtime` binary |
| `orpheus-statescript` | `rust/crates/statescript/` | `orpheus-runtime`, `orpheus-wire`, `flate2`, `indexmap`, `serde` | Portable StateScript graph loader, expression evaluator, codec tables, and decoupled VM core |
| `orpheus` (root) | `rust/src/` | Workspace crates + `bevy_ecs`, `mio`, `mlua`, `prost` | Lobby/BNet services, UDP worker, replication, host adapters, room simulation |

Verify crate dependency boundaries at any time with:

```powershell
cargo metadata --locked --no-deps
cargo tree --locked -p orpheus-collision -p orpheus-runtime -p orpheus-statescript -p orpheus-wire
```

### Modularized Subsystems inside `rust/crates/statescript/` & `rust/src/`

Recent refactors separated portable StateScript execution and broke up server monoliths into focused submodule trees:
- **`orpheus-statescript` (`rust/crates/statescript/`):** portable `script_graph`, `script_wire`, `script_codecs`, `script_expr/`, `script_entity_state`, `script_destination`, `script_modifiers`, `script_runtime`, `script_voice`, and `script_init/` VM core (tested on Linux in `rust-portable-tests`).
- **`game_combat/`** (`combat`, `enemy.rs`, `player.rs`, `projectiles.rs`, `tests/`): `RoomSimulation` Bevy ECS world housing bot vitals/movement, room-owned `PlayerBodyState` bodies, and physical projectiles.
- **`scene_assets/`** (`encounter.rs`, `prepared.rs`, `queries.rs`, `tests/`): `SceneAssets` cold map/collision loader separated from `PreparedScene` and `BotQueries`.
- **`game_service/`** (`room.rs`, `state.rs`, `tests/isolation.rs`): peer routing, input turn budgets, and room panic/fault retirement.

Remaining boundary cleanups:
- **Large module splits ([#60](https://github.com/leafall903/ORPHEUS/issues/60)):** split `script_enemy.rs`, `script_expr.rs`, `game_replication.rs`, `script_snapshot_wire.rs`, `script_player.rs`, and `game_service.rs`.
- **Cross-cutting adapters:** separate shared ticket types in `match_handoff` from lobby messages and move remaining host wire adapters out of the core server crate.

---

## Known Architectural Boundary Splits (Open Work)

| Current Gap | Impact | Tracked Issue |
| --- | --- | --- |
| Junkenstein gate/mode entities and scene scripts still live on `PreparedScene` rather than `RoomSimulation` ECS (player bodies moved into `RoomSimulation` ECS and production `set_external_targets(...)` copying has been retired). | Remaining gate/mode state must finish moving into `RoomSimulation` alongside bounded AI `think()` scripting. | [#65](https://github.com/leafall903/ORPHEUS/issues/65) |
| Navigation relies on Practice Range Lua hints (`patrol.lua`) and Junkenstein searches rather than a general NavMesh. | Maps without cached nav data (e.g. King's Row) lack general AI pathfinding. | [#63](https://github.com/leafall903/ORPHEUS/issues/63) |
| Combat wall queries use legacy `CollisionScene` while movement uses `ContactWorld`. | Discrepancies between contact bounds and wall rays must be diagnosed via inspector before changing masks. | [#62](https://github.com/leafall903/ORPHEUS/issues/62) |

---

## Room Authority & Worker Scheduling

- **Peer scheduling:** `game_service` processes incoming commands round-robin (up to `16` poses/events per peer, `64` per event-loop turn; `96`-frame lead limit) to prevent one noisy peer from starving others.
- **Room ECS tick:** steps patrolling/pursuing bots via `BotQueries`, queries room-owned `PlayerBodyState` and `BotState` components directly for combat targets/hitscan/projectiles/defenders (with friendly and dead pass-through and hostile blocking), advances projectile flight, executes scheduled respawns, records rewind poses (`TargetHistory`), and prepares encrypted owner/observer deltas.
- **Room-owned player authority vs. per-peer `WorldDelivery` state:**
  - **`RoomSimulation` (`bevy_ecs`):** owns shared gameplay truth per active player body (`PlayerBodyState`): `BodyId` / `PlayerBodyHandle`, owner wire ID, team/slot/hero, `Mover` physics, rewind `TargetHistory`, `OwnerBodyRuntime` VM & `Vitals`, active `BioticField`s, admitted `launches`, and posed `hitscan` shots, plus `PlayerBodyEvent` observer create/retire notifications.
  - **Generation-safe handle/lifecycle contract:** `next_player_generation(owner, floor)` advances per-owner generation monotonically across switches, leaves (`retire_player_owner`), and reconnects without resetting prior wire-slot history. `spawn_player_body` replaces any prior owner or wire-slot body and binds `(entity, generation)` inside `PlayerBodyHandle`; `player_body`, `player_body_mut`, `player_body_generation_current`, `clear_player_runtime`, `damage_player_body_by_wire`, and `advance_projectiles` reject stale generations.
  - **`WorldDelivery` / `BodyScript`:** stays per-peer and owns transport/projection concerns only — wire entity prerequisite tracking (`bodies`, `remote_players`, `bots`), delta/stream cursors (`BodyStream`), predicted projectile leases (`OwnerProjectiles`), and prediction `CorrectionHistory`.
- **Fault retirement:** all room ticks run inside typed error/panic guards (`panic=unwind`). On error or panic, only that room is retired (all seats/links detached and up to `64` bounded fault records stored for inspector viewing).

---

## Lua Script Host (`ai_script.rs` / `patrol.lua`)

- Each room creates an isolated `!Send` vendored Lua 5.4 VM (`mlua 0.11.6`, `StdLib::NONE`, `ChunkMode::Text`, empty `_ENV` table) on the single-threaded `orpheus-game` worker with filesystem, sockets, dynamic libraries, and wall-clock time disabled.
- `rust/src/ai_script.rs` defines the versioned `think(snapshot, events) -> commands` bridge (`AI_ABI_VERSION = 2`) with named, documented caps (`AI_MEMORY_LIMIT_BYTES` = 256 KiB, `AI_INSTRUCTION_HOOK_INTERVAL` = 64, `AI_INIT_HOOK_BUDGET` = 32 hooks, `AI_THINK_HOOK_BUDGET` = 64 hooks, `AI_MAX_SNAPSHOT_TARGETS` = 32, `AI_MAX_EVENTS_PER_TICK` = 16, `AI_MAX_COMMANDS_PER_TICK` = 8, `AI_MAX_WAIT_SECONDS` = 60.0) and raw-table validation returning deterministic `AiScriptError` values without panicking or hanging.
- `validate_player_body_handle` / `query_body_handle` reject stale, zero-generation, and unknown `BodyId` handles against `RoomSimulation`'s Stage 1 generation model without mutating ECS state, and Lua faults disable only the failing bot or faulted room while sibling rooms keep running.
- `rust/src/patrol_script.rs` adapts `rust/scripts/patrol.lua` onto `AiScript` (`DEFAULT_SOURCE`, `from_source_checked`) to drive Practice Range bot patrol decisions without rewriting authored StateScript graphs; future AI policies can use `AiScript` directly, and safe-boundary hot reload follows in Stage 4 ([#65](https://github.com/leafall903/ORPHEUS/issues/65)).

---

## Live 3D Inspector (`--features inspector`) & Benchmarks

Disabled in default builds. Enable `--features inspector --debug-port 3726` to serve a loopback HTTP viewer (assets live under `rust/assets/inspector/`):
- Displays a read-only `48 x 32 x 48` m window of ground/hitscan triangle layers (`4,096` triangles max per sample), broad-phase contact bounds (`256` max), active actors, and fraction-limiting bot sweep clips.
- Shows worker tick timing histograms (`p95`/`p99` bin bounds and missed ticks) and up to `64` room retirement records without exposing credentials or local file paths.
- Planned overlays (raycast/hitscan lines, AI navigation paths, and live StateScript state) are tracked in [#62](https://github.com/leafall903/ORPHEUS/issues/62).

### Multi-Map Packing & Two-Room Workload Benchmark

```powershell
# Prepare a unified runtime pack containing both Practice Range (0688) and Junkenstein (06C9)
cargo run --locked -p orpheus-runtime --bin prepare_server_runtime -- `
    --source 'C:\path\to\prepared-source' `
    --destination 'C:\path\to\unified-runtime' `
    --map 0688 --map 06C9 --game-graphs 'C:\path\to\tagged-graphs.json.gz'

# Two-room encrypted worker timing benchmark (default ports on server runs: TCP 3724, 1119/21119; UDP 3730)
$env:ORPHEUS_TIMING_BENCH_RUNTIME = 'C:\path\to\authorized-base-runtime'
$env:ORPHEUS_TIMING_BENCH_SECONDS = '60'
cargo test --locked --release --lib prepared_worker_timing_two_solo_rooms -- --ignored --nocapture
```

---

## Cutover & Provenance Rules

1. **Reference pinning:** Python (`main`) remains the default server until `rust` matches or exceeds Python's verified hero/mode coverage and passes original-client validation on the audited public checkout. Compare both using identical prepared `104319` packs, collision caches, commands, and packet-loss schedules.
2. **Legacy path comments:** Older Rust modules occasionally cite `ow174/...` paths from Python `74f36cd`, which predate `main`'s reorganization into `src/ow174/`, `tests/public/`, and `tests/runtime/`. Inspect a separate `main` checkout (or `git show 74f36cd:<path>`) when cross-checking those notes.
