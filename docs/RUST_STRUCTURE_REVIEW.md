# Rust structure review

This review separates public build checks, prepared-runtime checks and original-client evidence. The initial source audit used Rust `be0d41d` and Python `74f36cd`. Later entries describe the changes made during this review. Use the [parity checklist](RUST_PARITY.md) for gameplay cutover and the [architecture](NATIVE_SERVER_ARCHITECTURE.md) for authority boundaries.

## Findings and disposition

| Item | Finding | Disposition |
| --- | --- | --- |
| 1. Inherited status | Python behavior, isolated Rust graphs and client results had been combined. | Both READMEs now show Python, Rust and original-client evidence separately. The stock Rust weapon and skill acceptance remains open. |
| 2. Mixed run paths | Python setup and Rust checks needed separate entry points. | The homepage starts with public Rust checks, then prepared native startup. The contribution guide lists the Rust CI lanes. Python setup and tests live on `main`. |
| 3. Developer preview | Public source alone does not provide native deployment inputs or a complete original-client connection. | Both READMEs state the developer-preview scope and the remaining startup prerequisites. |
| 4. Flat server package | Prefixes did not enforce dependencies. | A Cargo workspace now separates the collision kernel and runtime preparation from the server. Room, replication and script-host boundaries still need further work. |
| 5. Rebuild cost | Stable collision code shared the server's package fingerprint. | The collision member can be checked and tested independently. No before-and-after clean or incremental build-time result has been recorded. |
| 6. Packer dependencies | The packer belonged to the server package. | `orpheus-runtime` owns the tool, preflight and shared catalog-format helpers. Its dependency tree has no server, ECS, Lua or networking dependency. |
| 7. Dual development | The branches had diverged with seven Python-only and ten Rust-only commits at the audit point. | The parity checklist pins the reference and defines the cutover freeze. The Python reference now lives only on `main`. A policy change for community development on `main` requires a separate maintainer decision. |
| 8. Startup diagnostics | Missing inputs needed a single executable inventory. | `--check-runtime` reports generated, prepared-research and restricted inputs. Native startup uses the same preflight before opening listeners. File presence and signing-input length checks precede normal content parsing. |
| 9. Test layout | A public cross-module protocol test lived under `src`. Worker tests exercise private state. | Public protocol and native CLI integration tests live in `rust/tests`. Worker unit tests remain module-local. Independent member CLI tests stay with their member. |
| 10. Root files | The root contains conventional project files and Python launchers still used by existing callers. | Preserve root README, license, credits and AGENTS discovery. The Python launchers were removed from `rust` and remain on `main`. Existing research records remain available. |
| 11. Python dependencies | `pyproject.toml` contains tool settings. `requirements.txt` holds runtime dependencies. | Both files were removed from `rust` with the Python reference. Python packaging is maintained on `main`. |
| 12. Python namespace | `ow174` is the current reference module and launcher entry point. | Preserve the supported invocation during migration. Native packages use the ORPHEUS name. A Python namespace rename would need its own caller migration and a supported multi-build use case. |
| 13. Portability | The relay and launcher are Windows-specific. Public CI verifies Windows. | Treat native Linux and macOS deployment as unverified until the build, prepared runtime, paths and socket lifecycle are checked on those hosts. |
| 14. Inspector packaging | HTTP and browser resources were part of the default server package. | The default build omits inspector HTTP and assets. Use `--features inspector` to compile them. Browser assets live under `rust/assets/inspector`. |
| 15. Visibility | The root library exposed most modules at the audit point. | New member internals use private modules where cross-package callers do not need them. Existing server import paths remain available while callers migrate. Narrow each interface alongside its tested package boundary. |
| 16. Restricted inputs | Content keys and lobby signing inputs need a separate local boundary. | Keep values, acquisition pointers, logs, captures and binaries out of publication. Document expected paths, formats and local integrity checks. Audit the exact outgoing Git history before pushing. |

## Source relationships

- `world` encodes retail entity updates and selects records within the packet budget. Room authority is owned by the simulation layer.
- `match_scene` builds initial player, bot and mode records using protocol codecs. `game_instance` handles roster and entry messages.
- `scene_assets` prepares immutable map and collision inputs and also provides room and owner-runtime adapters. It currently depends on types from `game_combat` and `game_replication`, whose implementations in turn use prepared scene interfaces. Split preparation from live-room queries before extracting these modules as packages.
- `match_handoff` combines a shared ticket with lobby message construction. Extract the ticket contract separately from the lobby adapter.
- `game_content` loads map and spawn data. `game_parameters` parses retail parameter envelopes and hero selection using wire types. Keep the wire adapter with its protocol dependencies as the runtime boundary evolves.

The module graph was generated from `be0d41d` with `cargo modules dependencies --lib --no-externs --no-sysroot --no-owns`. Its edges include `scene_assets` to `RoomSimulation` and `OwnerBodyRuntime`, and the reverse edges from `game_combat` to `BotQueries` and `game_replication` to `PreparedScene`. The extracted collision and runtime members have no dependency back to the server. `cargo metadata --locked --no-deps` and `cargo tree --locked -p orpheus-collision` or `-p orpheus-runtime` verify those package directions.

## Collision evidence

The kernel tests include synthetic geometry, frozen arithmetic fingerprints and behavior ported from the Python reference. Prepared regressions use authorized build-104319 collision data for the upstairs route, movement and spatial samples. There is currently no independently recorded original-client trace establishing exact GJK and contact-solver arithmetic parity. Keep that acceptance item open.

## Remaining order

1. Retain the default and inspector validation lanes and the prepared native match checks after each boundary change.
2. Separate immutable scene preparation from room and owner-runtime adapters. Move player authority into the room without changing observed packet ordering or lifetimes.
3. Extract protocol and shared handoff contracts after their cross-module callers have focused regression coverage.
4. Verify stock-body and weapon cutover workflows with the exact original client before retiring the Python reference and its launchers on `main`.
5. Measure incremental rebuilds and release worker timing with matched inputs and host load. Keep those results separate from client latency and presentation.
