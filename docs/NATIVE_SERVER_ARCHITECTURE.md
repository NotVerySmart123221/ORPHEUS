# Native Server Architecture

Target build: **Overwatch 1 retail 1.74.0.0.104319**.
This document defines the Rust workspace boundaries, simulation authorities, scripting host, and optional 3D inspector. See [RUST_PARITY.md](RUST_PARITY.md) for Python/Rust parity and [ROADMAP.md](ROADMAP.md) for acceptance status.

---

## Rust Contribution Rules

1. **Dedicated server boundary:** Rust owns sockets, encryption, protocol codecs, tick scheduling, lifetimes, collision queries, and command validation.
2. **One room authority:** Move all room entities (bots, projectiles, and eventually players/gates) under `RoomSimulation`'s room-local `bevy_ecs::world::World` with generation-safe handles and explicit stage order (#65).
3. **Data & script-driven gameplay:** Use authored StateScript graphs (`script_init`) for hero/mode logic and vendored Lua 5.4 (`mlua`) for hot-reloadable patrol/AI decisions bounded per room.
4. **Fault isolation:** Errors and panics in one room retire only that room and revoke its links (`game_service/room.rs`), leaving other active rooms running.
5. **Non-intrusive diagnostics:** The loopback web inspector (`--features inspector`) reads bounded, tick-stamped projections off the simulation thread without mutating match state.

---

## Workspace Crates & Module Boundaries

The Cargo workspace cleanly separates low-level portable kernels from the monolithic server binary:

| Crate | Path | Dependencies | Purpose |
| --- | --- | --- | --- |
| `orpheus-wire` | `rust/crates/wire/` | `aes-gcm`, `hmac`, `sha2`, `crc32fast`, `serde` | Bits, JAM cipher/framing/codecs, commands, game UDP framing |
| `orpheus-collision` | `rust/crates/collision/` | `sha2`, `thiserror` | `.col`/`.rays.col` cache decoding, spatial triangle grid, static rays, GJK/EPA solver |
| `orpheus-runtime` | `rust/crates/runtime/` | `serde`, `serde_json`, `flate2`, `sha2` | Pack manifest validation, `--check-runtime`, `prepare_server_runtime` binary |
| `orpheus` (root) | `rust/src/` | Workspace crates + `bevy_ecs`, `mio`, `mlua`, `prost` | Lobby/BNet services, UDP worker, replication, StateScript VM, room simulation |

### Modularized Subsystems inside `rust/src/`

Recent P0/P1 refactors broke up the largest server monoliths into focused submodule trees:
- **`script_init/`** (`15` submodules + `vm.rs`): authored StateScript VM loop, state stacks, abilities, arrays/vars, switches, enemy/boss actions, and offline/CI contracts.
- **`game_combat/`** (`combat`, `enemy.rs`, `projectiles.rs`, `tests/`): `RoomSimulation` Bevy ECS world housing bot vitals/movement and physical projectiles.
- **`scene_assets/`** (`encounter.rs`, `prepared.rs`, `queries.rs`, `tests/`): `SceneAssets` cold map/collision loader separated from `PreparedScene` and `BotQueries`.
- **`game_service/`** (`room.rs`, `state.rs`, `tests/isolation.rs`): peer routing, input turn budgets, and room panic/fault retirement.

Remaining monolithic files (`script_enemy.rs`, `script_expr.rs`, `game_replication.rs`, `script_snapshot_wire.rs`, `script_player.rs`) and extracting `orpheus-statescript` into `rust/crates/statescript` are tracked in [#60](https://github.com/leafall903/ORPHEUS/issues/60) and [#64](https://github.com/leafall903/ORPHEUS/issues/64).

---

## Known Architectural Boundary Splits (Open Work)

| Current Gap | Impact | Tracked Issue |
| --- | --- | --- |
| Player bodies (`OwnerBodyRuntime`) and Junkenstein gate/mode entities live on `WorldDelivery` / `PreparedScene` instead of `RoomSimulation` ECS. | Requires manual per-tick `set_external_targets(...)` snapshot copying; player/bot authority is split. | [#65](https://github.com/leafall903/ORPHEUS/issues/65) |
| Navigation relies on map-specific Practice Range Lua hints (`patrol.lua`) and Junkenstein searches rather than a general NavMesh. | Maps without cached nav data (e.g. King's Row) lack general AI pathfinding. | [#63](https://github.com/leafall903/ORPHEUS/issues/63) |
| `script_expr.rs` / `script_init/` live in the root server crate rather than a portable crate. | Prevents running all 38 StateScript contract tests in `rust-portable-tests` on Linux. | [#64](https://github.com/leafall903/ORPHEUS/issues/64) |
| Combat wall queries use legacy `CollisionScene` while movement uses `ContactWorld`. | Discrepancies between contact bounds and wall rays must be diagnosed via inspector before changing masks. | [#62](https://github.com/leafall903/ORPHEUS/issues/62) |

---

## Room Authority & Worker Scheduling

- **Peer scheduling:** `game_service` processes incoming commands round-robin (up to `16` poses/events per peer, `64` per event-loop turn; `96`-frame lead limit) to prevent one noisy peer from starving others.
- **Room ECS tick:** Steps patrolling/pursuing bots via `BotQueries`, advances projectile flight against static map geometry and target capsules, executes scheduled respawns, records rewind poses (`TargetHistory`), and prepares encrypted owner/observer deltas.
- **Fault retirement:** All room ticks run inside typed error/panic guards (`panic=unwind`). On error or panic, only that room is retired (all seats/links detached and up to `64` bounded fault records stored for inspector viewing).

---

## Lua Script Host (`patrol.lua`)

- Each room creates an isolated vendored Lua 5.4 VM (`mlua 0.11.6`) with filesystem, sockets, dynamic libraries, and wall-clock time disabled, plus strict memory and instruction budgets.
- Today, `rust/scripts/patrol.lua` drives stateless Practice Range bot patrol decisions (returning one wait duration or route slot validated by Rust). Expanding Lua hooks to AI `think()` loops and wave orchestration is part of [#65](https://github.com/leafall903/ORPHEUS/issues/65).

---

## Collision & Live Inspector (`--features inspector`)

Disabled in default builds. Enable `--features inspector --debug-port 3726` to inspect a read-only `48 x 32 x 48` m window around spawn or any sampled actor:
- Displays ground/hitscan triangle layers (capped at `4,096` triangles per sample), broad-phase contact bounds (`256` max), active actors, and fraction-limiting bot movement sweep clips (proxy IDs + previous/intended/final feet positions).
- Includes worker tick timing histograms (`p95`/`p99` bin bounds and missed ticks) and up to `64` room retirement records without exposing credentials or local file paths.
- Planned enhancements (raycast/hitscan line rendering, AI navigation path overlay, and live StateScript state inspection) are tracked in [#62](https://github.com/leafall903/ORPHEUS/issues/62).

### Two-Room Headless Workload Benchmark

```powershell
$env:ORPHEUS_TIMING_BENCH_RUNTIME = 'C:\path\to\authorized-base-runtime'
$env:ORPHEUS_TIMING_BENCH_SECONDS = '60'
cargo test --locked --release --lib prepared_worker_timing_two_solo_rooms -- --ignored --nocapture
```

Runs two encrypted solo rooms on one worker for `60` seconds with fixed hitscan cadence and prints manifest digest, tick histograms, packet gaps, and `prepare_scene_us` timings.
