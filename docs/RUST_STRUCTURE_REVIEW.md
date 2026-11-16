# Rust Structure Review

Architectural boundary notes comparing the workspace crates and internal modules inside `rust/`. See [NATIVE_SERVER_ARCHITECTURE.md](NATIVE_SERVER_ARCHITECTURE.md) for active design rules and [RUST_FOUNDATION_REVIEW.md](RUST_FOUNDATION_REVIEW.md) for tracked tasks.

---

## Completed Structural Separations

1. **Workspace crates (`rust/crates/`):**
   - `orpheus-collision`: collision cache decoding, triangle grids, static rays, and GJK/EPA solver — zero dependency on networking, ECS, or Lua.
   - `orpheus-runtime`: external pack preparation (`prepare_server_runtime`), SHA-256 manifests, and `--check-runtime` inventory.
   - `orpheus-wire`: bit reader/writer, JAM cipher/framing/codecs, commands, and game-link codecs (root library re-exports existing paths).
2. **Server monolith refactors (PR #53 & PR #55):**
   - `rust/src/script_init/` splits the 21k-line VM/contracts file into runtime (`vm.rs`, `machine.rs`, `shapes.rs`), domain action modules, and focused unit test suites.
   - `rust/src/game_combat/` separates `RoomSimulation` ECS combat from bot/enemy (`enemy.rs`) and projectile (`projectiles.rs`) systems.
   - `rust/src/scene_assets/` separates `SceneAssets` cold preparation from `PreparedScene` (`prepared.rs`), `BotQueries` (`queries.rs`), and Junkenstein encounter/gate adapters (`encounter.rs`).
   - `rust/src/game_service/` isolates room execution (`room.rs`) and routing state (`state.rs`) behind room-level fault/panic retirement.
3. **Optional 3D inspector packaging:** HTTP server and embedded web UI assets under `rust/assets/inspector/` are strictly gated behind `--features inspector` so default builds stay lean.
4. **Python separation:** Packaging files (`pyproject.toml`, `requirements.txt`, Python launchers) remain cleanly on `main`.

---

## Remaining Boundary Cleanups

1. **Remaining large `rust/src/` files ([#60](https://github.com/leafall903/ORPHEUS/issues/60)):** Split `script_enemy.rs`, `script_expr.rs`, `game_replication.rs`, `script_snapshot_wire.rs`, `script_player.rs`, and `game_service.rs`.
2. **Portable StateScript crate ([#64](https://github.com/leafall903/ORPHEUS/issues/64)):** Extract `script_graph`, `script_wire`, `script_codecs`, `script_expr`, `script_entity_state`, and `script_init` into `rust/crates/statescript` so all VM contracts run in `rust-portable-tests` on Linux.
3. **Room ECS authority ([#65](https://github.com/leafall903/ORPHEUS/issues/65)):** Move player bodies (`OwnerBodyRuntime`) out of `WorldDelivery` and Junkenstein gate/encounter entities out of `PreparedScene` into `RoomSimulation`'s `bevy_ecs` world, removing manual `set_external_targets` sync.
4. **Cross-cutting adapters:** Separate shared ticket types in `match_handoff` from lobby messages, and move remaining VM/host wire adapters out of the core interpreter.

Verify workspace dependency directions at any time with:

```powershell
cargo metadata --locked --no-deps
cargo tree --locked -p orpheus-collision -p orpheus-runtime -p orpheus-wire
```
