# Runtime Data Guide (`rust`)

Public Rust tests use synthetic JSON fixtures and run without game data. Starting `orpheus_server` or running `--ignored` prepared-runtime tests requires an authorized **1.74.0.0.104319 base Practice Range** pack (`ORPHEUS_RUNTIME_DIR`).

Keep extracted game assets, XML dumps, keys, and generated packs out of Git.

---

## Preflight & Packing

Validate any existing pack before starting the server:

```powershell
$env:ORPHEUS_RUNTIME_DIR = 'C:\path\to\authorized-base-runtime'
cargo run --locked --bin orpheus_server -- --check-runtime
```

`--check-runtime` verifies 22 required files (including `collision/0688.col` and the 64-byte `data/lobby_state_blob.key`).

Assemble a clean external pack with SHA-256 manifest from an existing prepared source:

```powershell
cargo run --locked -p orpheus-runtime --bin prepare_server_runtime -- --source 'C:\path\to\prepared-source' --destination 'C:\path\to\orpheus-runtime' --map 0688
```

To pack both Practice Range (`0688`) and Junkenstein (`06C9` / `0CD3`) with custom graphs and NPC target tags/movers:

```powershell
cargo run --locked -p orpheus-runtime --bin prepare_server_runtime -- `
    --source 'C:\path\to\prepared-source' --destination 'C:\path\to\unified-runtime' `
    --map 0688 --map 06C9 --game-graphs 'C:\path\to\tagged-graphs.json.gz' `
    --target-tags-xml 'C:\path\to\xml\003'
```

- `--game-graphs`: supplies a graph bundle exported on `main` (e.g. with `--mode-xml` and `--entity` bot bodies) without polluting the source tree.
- `--target-tags-xml`: enriches body/entity metadata and Junkenstein common-NPC mover tables from local `003` XML dumps.

---

## Input Provenance (Three Categories)

### 1. Tool Outputs from Local 104319 Game Files / XML (Generated via tools on [`main`](https://github.com/leafall903/ORPHEUS/tree/main/tools))

| Runtime File | Generator (`main` branch) |
| --- | --- |
| `data/extracted_{items,general_unlocks,heroes,maps}.json` | `tools/prepare_client_catalogs.py` (pinned OWLib DataTool) |
| `data/arcade_cards_174.json`, `data/game_{heroes,maps,movers,queues,stats}_174.json` | `tools/extract_*.py` from local XML exports |
| `data/ai_spawners_174.json`, `data/game_graphs_174.json.gz` | `tools/extract_ai_spawners.py`, `tools/extract_game_graphs.py` |
| `data/collision_keys_174.json`, `collision/0688.col` (and optional `*.rays.col`) | `tools/export_casc_records.ps1`, `tools/build_collision.py` |

### 2. Prepared Research Data (Must Be Supplied in Source Pack)

| Runtime File | Role |
| --- | --- |
| `data/schemas_174.json`, `data/game_schemas_174.json` | 707 lobby schemas and game-link message layouts |
| `data/statescript_codecs_174.json` | StateScript owner/observer frame field codecs |
| `data/retail_templates.json`, `bnet/descriptors.pb` | Lobby response templates and Battle.net protobuf descriptors |
| `data/collision_props_174.json` | Per-map prop placement keys and mode filters |

### 3. Private Local Inputs (Keep Confidential)

| Runtime File | Role |
| --- | --- |
| `data/resource_keys_174.json` | Content keys for event resources |
| `data/lobby_state_blob.key` | Exact 64-byte lobby handshake signing key |
| `data/junkenstein/junkenstein-npcs.json` | Explicit NPC definitions required for Junkenstein enemy tests/runs |
