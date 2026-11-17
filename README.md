# ORPHEUS (Rust)

[简体中文](README.zh-CN.md) · [Discord](https://discord.gg/tqukqjAk7) · [Python `main` branch](https://github.com/leafall903/ORPHEUS/tree/main)

ORPHEUS is an open-source server reimplementation for **Overwatch 1 (retail build 1.74.0.0.104319)** focused on research and preservation.

- **`rust` (this branch):** the experimental native Rust dedicated server, workspace crates, and local 3D debug inspector (`--features inspector`).
- **`main`:** the community-maintained Python server, Windows game launcher, data extractors, and reference tests ([main branch](https://github.com/leafall903/ORPHEUS/tree/main)).

[Public Rust Tests](#public-rust-tests) · [Run Native Server](#run-the-native-server) · [Current Status](#current-status) · [Architecture](docs/NATIVE_SERVER_ARCHITECTURE.md) · [Contributing](CONTRIBUTING.md) · [Issues](https://github.com/leafall903/ORPHEUS/issues)

---

## Public Rust Tests (No Game Files Required)

Install **Git** and **stable Rust** (`>= 1.85.0`). On Windows, also install the **x64 MSVC C++ Build Tools** and **Windows SDK** so Cargo can build the vendored Lua 5.4 runtime.

```powershell
git clone --branch rust https://github.com/leafall903/ORPHEUS.git
cd ORPHEUS
cargo test --locked --workspace --all-targets
```

Public tests use synthetic fixtures only. See [CONTRIBUTING.md](CONTRIBUTING.md) for running `clippy`, `--features inspector`, MSRV, and the 38 StateScript contract checks that public CI runs.

---

## Run the Native Server

Starting the native server requires a prepared **base 104319 Practice Range** runtime pack (optionally enriched with Junkenstein data). See [data/README.md](data/README.md) for the full input checklist and packing steps.

```powershell
$env:ORPHEUS_RUNTIME_DIR = 'C:\path\to\authorized-base-runtime'
cargo run --locked --bin orpheus_server -- --check-runtime
cargo run --locked --bin orpheus_server -- --data-dir 'C:\path\to\local-state'
```

Enable the optional loopback web 3D inspector (`--features inspector`):

```powershell
cargo run --locked --features inspector --bin orpheus_server -- --data-dir 'C:\path\to\local-state' --debug-port 3726
```

Then open `http://127.0.0.1:3726/` to view live room actors, sampled collision geometry, bot patrol clips, tick timing histograms, and room-fault records.

### Local Junkenstein Validation Overrides (Normal Cassidy)

For isolated Normal Revenge (`0x3C`) acceptance runs, pass `--junkenstein-validation <path.json>` before startup:

- **High-capacity gate victory check:** `{"invulnerable_cassidy": true, "gate_capacity": 1000000000}`
- **Destructible-gate defeat check:** `{"invulnerable_cassidy": true}`

These overrides apply only to card `0x3C` and do not affect default runs or other heroes/cards. Keep local profile files outside the repository.

---

## Current Status

| Area | Native Rust (`rust`) | Original-Client Status & Open Work |
| --- | --- | --- |
| **Login, Profiles & Gallery** | Native Battle.net RPC, JAM lobby, profile settings persistence (`profile-settings-v1`), and Hero Gallery loadout previews (including 23 NPC previews). | Encrypted lobby login and Practice Range entry work; client Hero Gallery NPC framing & full live flows await retest. |
| **Practice Range & Soldier: 76** | Loopback UDP rooms, Bevy ECS bots/projectiles, Lua patrol scripts, authored rifle/reload/Sprint/Helix/Biotic Field VM & fixture combat. | Headless encrypted tests pass; live 104319 weapon attachment, Sprint/ammo sync, and upstairs patrol need client acceptance. |
| **Junkenstein & Normal Cassidy** | All 14 cards load; Normal Cassidy Primary/Fan/Roll/Flashbang/Deadeye, ultimate charge (`0B8B`), team score, boss speech (`40E05F6B`), and gate stages pass headless replays. | Client Defeat heading verified; full Cassidy skills, Boss intro cameras/pursuit, recap timer, and natural Victory await live client acceptance. |
| **Architecture & Isolation** | Independent `orpheus-wire`, `orpheus-collision`, `orpheus-runtime`, and `orpheus-statescript` workspace crates; modularized `game_combat/`, `scene_assets/`, and `game_service/` with room-level fault/panic isolation. | Cross-map NavMesh (#63), unified ECS player bodies (#65), and Inspector ray/nav overlay (#62) tracked in issues. |

Details: [docs/NATIVE_SERVER_ARCHITECTURE.md](docs/NATIVE_SERVER_ARCHITECTURE.md) · [docs/STATE.md](docs/STATE.md) · [docs/BEHAVIOR_CONTRACTS.md](docs/BEHAVIOR_CONTRACTS.md) · [docs/ROADMAP.md](docs/ROADMAP.md)

---

## Workspace Layout

| Path | Role |
| --- | --- |
| `rust/src/` | Main server binary (`orpheus_server`), lobby, match worker, replication, ECS simulation (`game_combat/`), scene prep (`scene_assets/`), and host VM adapters |
| `rust/crates/statescript/` (`orpheus-statescript`) | Portable StateScript graph loader, expression evaluator, codec tables, and decoupled VM core |
| `rust/crates/wire/` (`orpheus-wire`) | JAM cipher, framing, codec readers/writers, game datagrams, commands |
| `rust/crates/collision/` (`orpheus-collision`) | Collision cache reader, spatial triangle index, static rays, GJK/contact solver |
| `rust/crates/runtime/` (`orpheus-runtime`) | Runtime manifest inventory, preflight checks, and `prepare_server_runtime` CLI |
| `rust/scripts/` | Lua patrol policy (`patrol.lua`) |
| `rust/assets/inspector/`, `rust/tests/` | Web 3D debug viewer assets and integration test targets (`world_queries`, `profile_settings`, `protocol`, `runtime_preflight`) |
| `fixtures/behavior/`, `scripts/` | 48 JSON behavior fixtures (including Rust-only regressions) and cross-language verifier scripts |
| `relay/` | Windows C++ client TLS relay source |

## License & Credits

MIT-licensed source code ([LICENSE](LICENSE)); upstream protocol research and contributor credits live in [CREDITS.md](CREDITS.md). Never commit game clients, commercial assets, signing inputs, keys, credentials, or raw packet captures.
