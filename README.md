# ORPHEUS

[![Rust 1.85+](https://img.shields.io/badge/Rust-1.85%2B-orange?logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Public source checks](https://github.com/leafall903/ORPHEUS/actions/workflows/public-checks.yml/badge.svg?branch=rust)](https://github.com/leafall903/ORPHEUS/actions/workflows/public-checks.yml?query=branch%3Arust)
[![Discord](https://img.shields.io/badge/Discord-Join-5865F2?logo=discord&logoColor=white)](https://discord.gg/tqukqjAk7)
[![PRs welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)

[简体中文](README.zh-CN.md) · [Quick start](#public-rust-tests-no-game-files-required) · [Current status](#current-status) · [Contribute](#contributing) · [Discord](https://discord.gg/tqukqjAk7)

**ORPHEUS is an independent, open-source Rust dedicated server reimplementation for Overwatch 1 (retail build 1.74.0.0.104319).** This developer preview supports research and game preservation, with headless regressions for the lobby, Practice Range, and parts of Junkenstein's Revenge.

- **Rust simulation and StateScript.** Bevy ECS runs room simulation, with separate crates for the protocol, collision, runtime data and StateScript VM.
- **Try the source without a game install.** The public workspace suite runs on synthetic fixtures.
- **Live Web Scene Inspector.** Follow actors, AI routes, collision samples and rays in top and elevation views. The timing panel reports worker percentiles from tick and scheduling histograms.

[![Scene Inspector preview showing patrol routes, ray hits, collision geometry, and worker timing on a synthetic arena](docs/assets/scene-inspector.gif)](docs/assets/scene-inspector.png)

*The project’s Inspector rendered with a synthetic arena and example timing data. [Open the static screenshot](docs/assets/scene-inspector.png).*

**Branches.** [`rust`](https://github.com/leafall903/ORPHEUS/tree/rust) is the GitHub default and the target for active development. [`main`](https://github.com/leafall903/ORPHEUS/tree/main) preserves the unmaintained Python server, launcher, extractors and reference tests. [Branch guide](docs/BRANCH_TRANSITION.md)

## Public Rust Tests (No Game Files Required)

Install **Git** and **stable Rust** (`>= 1.85.0`). On Windows, install the **x64 MSVC Build Tools** and **Windows SDK** for Rust's MSVC toolchain.

```powershell
git clone https://github.com/leafall903/ORPHEUS.git
cd ORPHEUS
cargo test --locked --workspace --all-targets
```

The default build excludes the Lua runtime. The server uses StateScript for patrol decisions. The included patrol graph selects waits and route slots from map-provided waypoints. Rust owns pathfinding, steering, collision and per-tick movement in the room simulation.

<details>
<summary>Optional Lua override and additional checks</summary>

Build with `--features lua-debug` and pass `--ai-script <file>` to use a local Lua override with hot reload. Enabling the feature by itself keeps the StateScript policy.

The [contribution guide](CONTRIBUTING.md) covers Clippy, the optional Inspector and Lua checks, MSRV, and the shared StateScript contracts in public CI.

</details>

---

## Run the Native Server (One-Click Unified Launcher)

Starting the native server requires a single unified **1.74.0.0.104319** runtime pack sourced strictly from your own local game files (never commit game clients, commercial assets, or keys). See [data/README.md](data/README.md) for the input checklist.

Use the unified launcher to pack (if `-SourceDir` is supplied), build `orpheus_server` (`inspector` + `lua-mods`), run `--check-runtime`, and launch with persistent saves (`%LOCALAPPDATA%\ORPHEUS\state`):

```powershell
# Double-click scripts\PLAY_ORPHEUS.cmd, or run:
powershell -ExecutionPolicy Bypass -File scripts\setup_and_play.ps1 -RuntimeDir 'C:\path\to\orpheus-runtime-104319'

# Launch with a Lua 5.4 Workshop Mod:
powershell -ExecutionPolicy Bypass -File scripts\setup_and_play.ps1 -RuntimeDir 'C:\path\to\orpheus-runtime-104319' -ModScript rust\scripts\mods\low_gravity_arena.lua
```

<details>
<summary>Manual CLI & Live Web Scene Inspector</summary>

```powershell
$env:ORPHEUS_RUNTIME_DIR = 'C:\path\to\orpheus-runtime-104319'
cargo run --locked --bin orpheus_server -- --check-runtime
cargo run --locked --features inspector,lua-mods --bin orpheus_server -- --data-dir 'C:\path\to\local-state' --debug-port 3726
```

Open `http://127.0.0.1:3726/` for the live Web Scene Inspector (actors, routes, dual-collision `/api/debug/v1/collision-compare`, worker timing percentiles, and room-fault records).

</details>

---

## Lua 5.4 Mod & Workshop Scripting

Build with `--features lua-mods` (or `inspector,lua-mods`) and pass `--mod-script <file.lua>` to customize gameplay rules and react to room events in a **sandboxed Lua 5.4 VM** (`StdLib::NONE`, 256 KiB memory cap, bounded instruction hooks, zero filesystem/OS/network access, and atomic `reload` rollback):

- **Rules:** `damage_scale`, `healing_scale`, `gravity_scale`, `speed_scale`, `ult_charge_scale`, `cooldown_scale`, `friendly_fire`
- **Event Hooks:** `on_room_start`, `on_player_spawn`, `on_damage`, `on_kill`, `on_button`, `on_chat`, `on_tick`
- **Included Examples:** [`rust/scripts/mods/low_gravity_arena.lua`](rust/scripts/mods/low_gravity_arena.lua) · [`rust/scripts/mods/headshot_only_practice.lua`](rust/scripts/mods/headshot_only_practice.lua)

```lua
return {
    abi = 1,
    info = { name = "Headshot-Only Practice", version = "1.0.0", author = "ORPHEUS" },
    on_damage = function(ctx, event)
        if not event.headshot and not event.melee then
            return { damage = 0.0, actions = {} }
        end
    end,
}
```

Type `reload` + Enter in the server console to hot-reload your mod without restarting the server.

---

## Current Status

| Area | Implemented (`rust`) | Next Steps |
| --- | --- | --- |
| **Login, Profiles & Gallery** | Native Battle.net RPC, JAM lobby, profile settings persistence (`profile-settings-v1`), and Hero Gallery loadout previews (including 23 NPC previews). | Expand lobby flows and polish Hero Gallery framing. |
| **Practice Range & Combat** | Local UDP rooms use Bevy ECS to simulate bots, breakable props/hollow railings (`STU_E62A2EFC`), interactive props (`room_primitives.rs`), voice/emote/Need Healing cues, and 32/32 generic retail hero startup. | Expand live-client ability acceptance across all 32 heroes and multi-client matches. |
| **PvE & PvP Modes** | Consolidated `pve_encounter/` (all 14 Junkenstein cards) and composable PvP mode contracts (`pvp_mode.rs`: Escort, Assault, Hybrid, Control). | Expand PvE hero roster and live multi-player PvP match flow. |
| **Architecture & Modding** | Independent `orpheus-wire`, `orpheus-collision`, `orpheus-runtime`, and `orpheus-statescript` crates, Inspector dual-collision compare, and sandboxed Lua 5.4 Mod API (`mod_script.rs`). | Extend Lua Mod actions into live room entity/physics hooks. |

See the [architecture](docs/NATIVE_SERVER_ARCHITECTURE.md), [evidence and status](docs/STATE.md), [behavior contracts](docs/BEHAVIOR_CONTRACTS.md), and [roadmap](docs/ROADMAP.md).

---

## Architecture and Workspace Layout

```mermaid
flowchart LR
    Client["104319 client"] <--> Server["Native server / orpheus-wire"]
    Server --> ECS["Bevy ECS room simulation"]
    Runtime["Prepared runtime data"] --> Server
    ECS --> Collision["Collision queries"]
    ECS --> StateScript["StateScript VM"]
    ECS -.-> Inspector["Live Web Scene Inspector"]
    Lua["Optional Lua debug override"] -.-> ECS
```

| Path | Role |
| --- | --- |
| `rust/src/` | Main server binary (`orpheus_server`), lobby, match worker, replication, ECS simulation (`game_combat/`), scene prep (`scene_assets/`), and host VM adapters |
| `rust/crates/statescript/` (`orpheus-statescript`) | Portable StateScript graph loader, expression evaluator, codec tables, and decoupled VM core |
| `rust/crates/wire/` (`orpheus-wire`) | JAM cipher, framing, codec readers/writers, game datagrams, commands |
| `rust/crates/collision/` (`orpheus-collision`) | Collision cache reader, spatial triangle index, static rays, GJK/contact solver |
| `rust/crates/runtime/` (`orpheus-runtime`) | Runtime manifest inventory, preflight checks, and `prepare_server_runtime` CLI |
| `rust/scripts/` | StateScript patrol policy and sample Lua debug override |
| `rust/assets/inspector/`, `rust/tests/` | Web Scene Inspector assets and integration test targets (`world_queries`, `profile_settings`, `protocol`, `runtime_preflight`) |
| `fixtures/behavior/`, `scripts/` | JSON behavior fixtures (including Rust-only regressions) and cross-language verifier scripts |
| `relay/` | Windows C++ client TLS relay source |

## Contributing

- **Rust and protocol.** Extend shared hero abilities and the delivery of StateScript state to clients. [Delivery coverage example (#166)](https://github.com/leafall903/ORPHEUS/issues/166)
- **Inspector, navigation and ECS.** Build on the completed work in [#62](https://github.com/leafall903/ORPHEUS/issues/62), [#63](https://github.com/leafall903/ORPHEUS/issues/63), and [#65](https://github.com/leafall903/ORPHEUS/issues/65).
- **Gameplay feedback.** Play the Practice Range or Junkenstein's Revenge locally with build 104319 and report reproducible gameplay or protocol discrepancies. [Roadmap](docs/ROADMAP.md)

See the [contribution guide](CONTRIBUTING.md), browse [open issues](https://github.com/leafall903/ORPHEUS/issues?q=is%3Aissue+is%3Aopen), or join [Discord](https://discord.gg/tqukqjAk7).

## License & Credits

The source is [MIT-licensed](LICENSE). Upstream research and contributor credits are documented in [CREDITS.md](CREDITS.md). ORPHEUS is an independent research and preservation project with no affiliation to Blizzard Entertainment. Never commit game clients, commercial assets, signing inputs, keys, credentials, or raw packet captures.
