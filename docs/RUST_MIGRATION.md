# Rust Migration Guide

Target build: **Overwatch 1 retail 1.74.0.0.104319**.
Python (`main`) serves as the behavioral reference and holds the extraction tooling; `rust` hosts the native dedicated server. Track per-workflow parity in [RUST_PARITY.md](RUST_PARITY.md) and architecture boundaries in [NATIVE_SERVER_ARCHITECTURE.md](NATIVE_SERVER_ARCHITECTURE.md).

---

## Migration Stages

| Stage | Scope | Current Status |
| --- | --- | --- |
| **1. Transport & Codecs** | Bits, commands, JAM cipher/framing/codecs, game UDP | Extracted into `orpheus-wire`; framing matches Python byte-for-byte in CI. |
| **2. Login, Lobby & Gallery** | Battle.net RPC, accounts, profiles/settings, Hero Gallery | Native lobby + settings persistence (`profile-settings-v1`) pass CI; 23 NPC previews fixed. |
| **3. Collision & Physics** | Map `.col` / `.rays.col`, triangle grids, GJK/contact solver | Extracted into `orpheus-collision`; 43 movers match Python reference records. |
| **4. Runtime Prep & Preflight** | External pack manifest, SHA-256 checks, `--check-runtime` | Extracted into `orpheus-runtime` (`prepare_server_runtime` CLI). |
| **5. StateScript VM & World** | Expression evaluator, World queries, host adapters | Core engine lives in `script_init/` + `script_expr.rs`; 38 VM contracts verified in CI; extraction to `orpheus-statescript` tracked in [#64](https://github.com/leafall903/ORPHEUS/issues/64). |
| **6. Match Loop & Modes** | Practice Range (Soldier: 76), Junkenstein (Normal Cassidy) | Bots/projectiles run in Bevy ECS + Lua; player bodies/gate migration tracked in [#65](https://github.com/leafall903/ORPHEUS/issues/65); cross-map NavMesh in [#63](https://github.com/leafall903/ORPHEUS/issues/63). |
| **7. Cutover** | Parity coverage + original-client validation | Open; Python remains the default branch until live client workflows match or exceed Python. |

---

## Historical Reference Note

Provenance comments in older Rust modules that cite `ow174/...` paths from Python `74f36cd` predate `main`'s reorganization into `src/ow174/`, `tests/public/`, and `tests/runtime/`. Use a separate `main` checkout (or `git show 74f36cd:<path>`) when cross-checking old file paths.

---

## Running & Packing Both Modes

```powershell
# Prepare a unified runtime pack containing both Practice Range (0688) and Junkenstein (06C9)
cargo run --locked -p orpheus-runtime --bin prepare_server_runtime -- `
    --source 'C:\path\to\prepared-source' `
    --destination 'C:\path\to\unified-runtime' `
    --map 0688 --map 06C9 --game-graphs 'C:\path\to\tagged-graphs.json.gz'

# Start native server (with optional 3D inspector)
$env:ORPHEUS_RUNTIME_DIR = 'C:\path\to\unified-runtime'
cargo run --locked --features inspector --bin orpheus_server -- --data-dir 'C:\path\to\local-state' --debug-port 3726
```

Default listening ports are TCP `3724` (lobby), `1119` / `21119` (local Battle.net), and UDP `3730` (game server). Keep raw assets, keys, and local state outside the repo.

---

## Migration Rules

- **Verify behavior through shared fixtures:** Keep wire formats and behavior cases aligned via `fixtures/behavior/*.json` ([BEHAVIOR_CONTRACTS.md](BEHAVIOR_CONTRACTS.md)).
- **Respect MSRV & portability:** Keep the workspace compiling cleanly on Rust `1.85.0` and keep `orpheus-wire`, `orpheus-collision`, and `orpheus-runtime` Linux-portable.
- **Attribute upstream work:** Credit borrowed behavior/constants in [CREDITS.md](../CREDITS.md) and commit messages; treat client observations separately from offline fixture runs.
