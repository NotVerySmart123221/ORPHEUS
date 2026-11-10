# ORPHEUS

[简体中文](README.zh-CN.md)

[Discord](https://discord.gg/tqukqjAk7)

ORPHEUS is an open-source server reimplementation for **Overwatch 1 retail 1.74.0.0.104319**. It is an independent, unofficial research and game preservation project. The long-term goal is to reproduce the server behavior and client-facing events for this build across its heroes, maps and modes. The server is still in development, with Soldier: 76 in the Practice Range as the first end-to-end target.

[Contribute](CONTRIBUTING.md) · [Roadmap](docs/ROADMAP.md) · [Rust branch](https://github.com/leafall903/ORPHEUS/tree/rust) · [Verification record](docs/STATE.md) · [Changelog](CHANGELOG.md) · [Open issues](https://github.com/leafall903/ORPHEUS/issues)

## Current status

| Area | Implemented and tested on the server | Evidence from the original client and remaining work |
| --- | --- | --- |
| Local login and lobby | Local authentication emulation, lobby, hero selection and match handoff. Automated checks cover handshake and server startup with prepared data. | Login, Practice Range loading and hero selection or switching have been observed. Broader lobby flows and match rules need verification. |
| Soldier: 76 | Body and weapon state, movement and Sprint, rifle fire and reload, Helix Rockets and Biotic Field. Automated tests cover selected scenarios including repeated switching and rejoining, lost packets, and ammunition state. | The tester has fired on and killed training bots. Stable weapon presentation, ammunition HUD, input response and ability behavior still need a complete client pass. |
| Practice Range combat | Hit and damage resolution, bot health, death and respawn, and replication of the original overhead health display graph for stock bots. Automated tests use controlled fixtures and prepared collision from the target map. | The tester has reported missing overhead health bars, unnamed eliminations, missing kill feed and missing headshot audio. The display graph change has wire evidence. Its rendering still needs a documented client result. |
| Networking and research | Encrypted UDP, player input, entity lifecycles, server snapshots, replay and selected loss/reorder checks. | Client prediction, combat feedback, two-client agreement, longer sessions and broader map behavior remain open. |
| Heroes, modes and audio | Shared systems and partial coverage exist. | Full hero abilities, objectives, match flow, hit and multikill feedback, and contextual voice playback are still in development. |

An automated result covers the fixture or replay named in its evidence. A manual observation covers the build, commit and steps that were actually tried. No complete original-client acceptance pass has been recorded for the first target scenario. See the [verification record](docs/STATE.md) and [changelog](CHANGELOG.md).

## Get involved

- **Developers and AI-assisted contributors** can start with the public source tests. The guide covers Codex, Claude Code and the same review requirements for all submissions. [Contribution guide](CONTRIBUTING.md)
- **Reverse engineers** can help pin a single field, state transition, event or timing rule to build 104319. Start with the relevant evidence and report uncertainty explicitly. [Research evidence](docs/RESEARCH_EVIDENCE.txt)
- **Players** with the target client and authorized runtime can report a short, repeatable gameplay result. Reports of working behavior are welcome alongside bugs. [Gameplay verification](https://github.com/leafall903/ORPHEUS/issues/new?template=gameplay-verification.yml)
- **New contributors** can look for tasks that use the public checkout and synthetic data. [Good first issues](https://github.com/leafall903/ORPHEUS/labels/good%20first%20issue)

## Run locally

The `main` branch contains the community-maintained Python server and Windows launcher. Native server development continues on the separate `rust` branch.

### Source-only checks

Use Windows with 64-bit Python 3.12. The source tests below use synthetic fixtures. Node.js 24 is used for the separate dashboard test.

```powershell
py -3.12 -m venv .venv
.\.venv\Scripts\python.exe -m pip install -r requirements.txt "ruff==0.17.3"
.\.venv\Scripts\python.exe -B -m unittest discover -s tests -p test_game_commands.py
```

See the [full public checks](CONTRIBUTING.md#public-source-checks) before running broader test discovery.

### Separate deployment data

A local play session requires both the **1.74.0.0.104319** game installation and an authorized, prepared server runtime. The runtime provides the protocol data, signing input, gameplay graphs and map collision caches used by the Python server. Game-folder-only setup is still a delivery goal. See the [runtime guide](data/README.md) for required files and available preparation steps.

Set `ORPHEUS_RUNTIME_DIR` to your prepared pack. The preflight lists missing startup inputs and checks the signing input length. Run it before installing Python dependencies if needed. Map collision is checked separately when a scene loads.

```powershell
$env:ORPHEUS_RUNTIME_DIR = 'C:\path\to\orpheus-runtime'
py -3.12 -B -m ow174 --check-runtime
```

If you already have the required inputs in a local research checkout, the Python packer copies them into a new directory and writes a manifest. Use `--map 0688` for the Practice Range or omit it to copy all prepared map caches.

```powershell
py -3.12 -B tools/prepare_server_runtime.py --source 'C:\path\to\local-research' --destination 'C:\path\to\orpheus-runtime' --map 0688
```

### Start the server or play locally

For server-only checks, run `START_SERVER_LOCAL.cmd` with `ORPHEUS_RUNTIME_DIR` set. It starts the local loopback services and dashboard at `http://127.0.0.1:3725` using the prepared runtime. This entry needs only the runtime and Python environment.

To play locally, first build the Windows relay if you do not already have a trusted local `relay/owwfd_relay.dll` build. Install the x64 MSVC Build Tools, open the x64 Native Tools Command Prompt, and run this from the repository root.

```bat
.\relay\build.bat
```

With the runtime configured and relay built, pass the installation directory or its `_retail_\Overwatch.exe` to the local launcher. With no path argument, the launcher uses its saved client path or opens a file picker.

```powershell
.\START_LOCAL.cmd --game-exe 'C:\path\to\Overwatch'
```

The launcher creates a Python environment if needed and remembers the selected installation for future launches and collision preparation. See [original-client verification](CONTRIBUTING.md#original-client-verification) for what to record during a playtest.

## Project layout

| Path | Role |
| --- | --- |
| `ow174/bnet`, `ow174/lobby` | Local login, accounts, social and lobby services, matchmaking and handoff |
| `ow174/game`, `ow174/game/script` | Encrypted game transport, inputs, simulation, combat and statescript replication |
| `ow174/dashboard`, `relay` | Local administration and the Windows client relay source |
| `tests`, `tools`, `docs` | Verification, extraction and replay tools, and research records |

The [roadmap](docs/ROADMAP.md) tracks the full build scope. [CREDITS.md](CREDITS.md) records the original 1.74 lobby project, research sources and upstream authors. The public source retains the original [MIT license notice](LICENSE). Overwatch and its assets belong to their respective rights holders. Keep game clients, commercial assets, keys, private logs, raw captures and dumps out of public submissions.
