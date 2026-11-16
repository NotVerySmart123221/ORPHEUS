# Verification & Implementation State (`rust`)

Tracks what has been verified on the `rust` native server branch targeting **Overwatch 1.74.0.0.104319**. For detailed Python comparison parity see [RUST_PARITY.md](RUST_PARITY.md), and for baseline prepared test names see [RUST_FOUNDATION_BASELINE.md](RUST_FOUNDATION_BASELINE.md).

*(Note: historical Python `9f8f3bcb` notes reside on `main`; this file tracks the Rust developer preview.)*

---

## Current Capabilities on `rust`

### 1. Public CI & Workspace Checks (No Game Data Required)
- **Workspace crates:** `orpheus-wire`, `orpheus-collision`, and `orpheus-runtime` build and pass isolated Linux & Windows tests (`cargo test --locked --all-targets -p orpheus-collision -p orpheus-runtime -p orpheus-wire`).
- **Server public suites:** `532` library unit tests (`406` run publicly, `126` `#[ignore]` prepared-data tests) in `default`, and `536` (`410` public, `126` `#[ignore]`) in `--features inspector`, plus zero-warning `clippy`, `cargo fmt --check`, and MSRV `1.85.0` checks.
- **Cross-language contracts (`shared-behavior-contracts`):** Hosted CI checks out `main` (`python-reference`) and verifies identical behavior across `jam-framing-v1.json`, `world-queries-v1.json`, and `profile-settings-v1.json`, plus `12` public Python tests and `38` exact StateScript VM contracts on `rust`.
- **C++ gallery preview policy:** `scripts/check_gallery_preview_policy.py` compiles the client preview relay header cleanly and verifies 18 resource fallback + 6 owned-tree pruning cases.

### 2. Prepared-Runtime Regressions (`--ignored` with Base / Junkenstein 104319 Packs)
- **Lobby & Match Loopback:** Encrypted Battle.net RPC + JAM lobby login, gallery skin selection, runtime pack preparation (`prepare_server_runtime`), UDP authentication, Practice Range & all 14 Junkenstein card loading.
- **Practice Range & Soldier: 76:** 43 collision-backed movers match Python reference kinematics; authored Soldier body tree, rifle/reload, Sprint modifier, Helix Rockets, and Biotic Field run in loopback rooms alongside Bevy ECS bots and Lua patrols.
- **Junkenstein & Normal Cassidy:** Full headless replays cover Normal Cassidy Primary/Fan damage against all 5 Bosses and common NPCs, Combat Roll, Flashbang, Deadeye, `0B8B` ultimate charge, team score, Boss voice selection (`40E05F6B`), and Natural Victory / Defeat outcomes.
- **Fault Isolation & Worker Timing:** Room errors and `catch_unwind` panics retire only the broken room while sibling rooms keep serving encrypted world updates and hits; two-room timing workload benchmark verifies worker histograms (`p95`/`p99`) cleanly.

---

## Remaining Open Work

### Live Original-Client Gaps
- **Practice Range:** Live 60s Soldier: 76 weapon/ammo HUD acceptance (#3), bot overhead health bars (#7), localized kill feed names (#8), headshot SFX (#9), boundary death plane (#13), and upstairs patrol verification.
- **Junkenstein (`0x3C`):** Recap elapsed-time display, Boss intro cameras & AI pursuit, Witch pistol fallback, Zombardier attacks, live Flashbang reaction animations, Hero Gallery client rendering, and Natural Victory client pass.

### Low-Level Engineering Roadmap (Issues `#60`–`#65` & `#12`)
- **#60:** Split remaining large modules (`script_enemy.rs`, `script_expr.rs`, `game_replication.rs`, `script_snapshot_wire.rs`, `script_player.rs`, `game_service.rs`).
- **#61:** Add Rust consumers for the 4 synced `main` behavior fixtures (`server-only-movement-v1`, `body-short-action-retry-v1`, `seismic-slam-*`) and port 19 VM contracts back to `main`.
- **#62:** Add raycast, contact-normal, nav-path, and live StateScript inspection overlays to `--features inspector`.
- **#63:** Generalize walkable-surface / NavMesh extraction across maps (including King's Row).
- **#64:** Extract portable `orpheus-statescript` workspace crate.
- **#65:** Unify player/gate entities into `bevy_ecs` and expose bounded hot-reloadable AI `think()` scripting.
- **#12:** Prevent recycled body IDs from receiving stale reliable combat notices.
