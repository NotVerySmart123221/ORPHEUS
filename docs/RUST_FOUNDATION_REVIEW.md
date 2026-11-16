# Rust Foundation Review & Tracker

Tracks architectural refactoring, modularity, CI gates, and low-level workspace boundaries on `rust`. See [RUST_FOUNDATION_BASELINE.md](RUST_FOUNDATION_BASELINE.md) for the exact list of 66+ prepared-runtime baseline tests and [RUST_STRUCTURE_REVIEW.md](RUST_STRUCTURE_REVIEW.md) for crate dependency analysis.

---

## Validation Gates

Every PR targeting `rust` runs these checks in CI (`.github/workflows/public-checks.yml`):

```powershell
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --features default -- -D warnings
cargo test --locked --workspace --all-targets --features default
cargo clippy --locked --workspace --all-targets --features inspector -- -D warnings
cargo test --locked --workspace --all-targets --features inspector
cargo +1.85.0 check --locked --workspace --all-targets --features inspector
git diff --check
```

Plus Ubuntu lanes for portable crates (`orpheus-collision`, `orpheus-runtime`, `orpheus-wire`) and C++ gallery preview policy, and Windows differential checks in `shared-behavior-contracts`.

---

## Tracker Summary

| Area | Current Status | Notes & Follow-Up Issue |
| --- | --- | --- |
| **Domain error types** | **Partial** — `CombatError`, `RoomError`, `WorkerError`, `CodecError`, `SnapshotError`, `InitFault` preserve root causes. | Remaining `io::Error` adapters can migrate gradually. |
| **Independent crates** | **Done for 3 crates** — `orpheus-wire`, `orpheus-collision`, and `orpheus-runtime` extracted with zero server/ECS deps. | Next: extract `orpheus-statescript` VM crate ([#64](https://github.com/leafall903/ORPHEUS/issues/64)). |
| **Monolith splitting** | **P0 & P1 Complete** — `script_init.rs` (21k $\rightarrow$ 15 submodules), `game_combat.rs` (7.3k $\rightarrow$ 3 submodules), `scene_assets.rs` (4.9k $\rightarrow$ 5 submodules). | Next: split `script_enemy.rs`, `script_expr.rs`, `game_replication.rs`, `script_snapshot_wire.rs`, `script_player.rs`, `game_service.rs` ([#60](https://github.com/leafall903/ORPHEUS/issues/60)). |
| **Explicit World queries** | **Done** — `World` queries distinguish unknown/unavailable services (`None`) from known unset (`Some(Null)`). | Covered by `world-queries-v1.json` in cross-language CI. |
| **Room fault isolation** | **Done** — Errors and `catch_unwind` panics retire only the failing room while sibling rooms keep serving. | Covered by `room-isolation-v1.json` and encrypted multi-room checks. |
| **Cross-language contracts** | **Synced (47 fixtures)** — Framing, World queries, and profile settings verified by both Python & Rust in CI. | Add Rust consumers for 4 `main` fixtures & port 19 VM contracts back to `main` ([#61](https://github.com/leafall903/ORPHEUS/issues/61)). |
| **Navigation & NavMesh** | **Map-specific** — Practice Range uses Lua waypoint hints; Junkenstein uses custom spatial searches. | General walkable-surface / NavMesh pipeline for PvP maps incl. King's Row ([#63](https://github.com/leafall903/ORPHEUS/issues/63)). |
| **Inspector & Profiling** | **3D collision & timing live** — `--features inspector` shows triangles, bounds, patrol clips, fault logs, and two-room benchmarks. | Visualize hitscan/raycasts, contact normals, nav paths & VM state ([#62](https://github.com/leafall903/ORPHEUS/issues/62)). |
| **Full ECS & Scripting** | **Partial** — Bots & projectiles run in `bevy_ecs` + Lua patrol pilot; player bodies remain in `WorldDelivery`. | Unify player/gate entities under `bevy_ecs` & expose AI `think()` script ABI ([#65](https://github.com/leafall903/ORPHEUS/issues/65)). |
