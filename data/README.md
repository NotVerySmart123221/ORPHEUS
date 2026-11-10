# Runtime data

The Python server targets Overwatch retail **1.74.0.0.104319**. A clean checkout supports the public source checks. Serving the original client requires a local runtime assembled from authorized inputs. See [separate deployment data](../README.md#separate-deployment-data).

## Generate client catalogs

The four catalog files can be generated from your authorized 104319 installation. Use the [pinned OWLib source](https://github.com/overtools/OWLib/tree/7afe5f52706e7002ed8c2717299cd60846f3fa3a) with its submodules and the x64 .NET 6 SDK.

```powershell
git clone --branch overwatch/1.74 https://github.com/overtools/OWLib.git owlib-174
git -C owlib-174 switch --detach 7afe5f52706e7002ed8c2717299cd60846f3fa3a
git -C owlib-174 submodule update --init --recursive
dotnet build .\owlib-174\DataTool\DataTool.csproj -c Release
```

From the ORPHEUS checkout, pass the built executable, the game folder and a new catalog directory outside both checkouts. The game folder must contain the active `1.74.0.0.104319` retail entry in `.build.info`.

```powershell
$game = 'C:\path\to\Overwatch'
$dataTool = 'C:\path\to\owlib-174\DataTool\bin\Release\net6.0\DataTool.exe'
$catalogs = 'C:\path\to\local\orpheus-catalogs'
py -3.12 -B tools\prepare_client_catalogs.py --game $game --data-tool $dataTool --destination $catalogs
```

The command runs OWLib's hero, map and unlock list modes and writes `extracted_heroes.json`, `extracted_maps.json`, `extracted_items.json` and `extracted_general_unlocks.json`. It checks the four JSON outputs before making the catalog directory available. The other runtime inputs still come from the prepared source described below.

## Check or prepare a local runtime

With Python 3.12 installed, unpack your authorized pack so that `runtime-data/data/`,
`runtime-data/bnet/` and `runtime-data/collision/` are directly under the checkout. ORPHEUS
discovers `runtime-data/` when it contains `manifest.json` or `data/`. An explicit
`ORPHEUS_RUNTIME_DIR` takes precedence. Without either, ORPHEUS uses the legacy checkout layout.

Run the preflight from the repository root.

```powershell
py -3.12 -B -m ow174 --check-runtime
```

The default check covers the retail lobby, Battle.net descriptor set and match data. It reports missing paths and validates the signing input's length. Practice Range also needs build 104319 collision data. For a server-only session, supply `collision/0688.col` in the pack. An enabled local client launch can build the cache using the installation and the prepared collision indexes.

If you already have a prepared local research checkout, assemble a new pack with the Python tool. It copies the required files and selected map cache into a new directory and writes a SHA-256 inventory. From a separate prepared source, you can target `runtime-data` directly.

```powershell
py -3.12 -B tools\prepare_server_runtime.py --source 'C:\path\to\prepared-checkout' --destination .\runtime-data --map 0688
```

Add `--catalogs $catalogs` to use the four locally generated catalogs in place of the source checkout's copies.

## Input provenance and current limits

| Inputs | Current preparation route |
| --- | --- |
| Four hero, map and unlock catalogs | Run the [catalog generator](#generate-client-catalogs) against the target installation. |
| Arcade cards and hero body mapping | Use local DataTool XML for types `0C7` and `075` respectively. Hero body mapping also uses the generated hero catalog. [Arcade extractor](../tools/extract_arcade_cards.py) · [Hero extractor](../tools/extract_game_heroes.py) |
| Mover parameters, state graphs and statistics | Use local DataTool XML for `003`, `003` and `01B`, or `054`, `075` and `062` respectively. Mover and statistics extraction use the hero body mapping. [Mover extractor](../tools/extract_game_movers.py) · [Graph extractor](../tools/extract_game_graphs.py) · [Statistics extractor](../tools/extract_game_stats.py) |
| Queue rules, gameplay map records and bot routes | The current extractors also rely on prepared reference templates, map placement exports or collision indexes. [Queue extractor](../tools/extract_game_queues.py) · [Map extractor](../tools/extract_game_maps.py) · [Spawner extractor](../tools/extract_ai_spawners.py) |
| Map collision cache | Run `tools/build_collision.py 0688 --game <game-folder>` from a checkout with the matching collision indexes, or with `ORPHEUS_RUNTIME_DIR` pointing to a local pack that has those indexes. |
| Retail templates, Battle.net descriptors, lobby signing input, resource keys, and required protocol and collision indexes | The current Python server needs these prepared inputs. The public repository has no complete generation or distribution path for them. |

The retail templates originate in a reference server session. Fully setting up a local host from the game folder alone remains an open task.

Keep game files, dumps, raw captures, signing input and generated runtime files in local storage. Share missing file names or a redacted error when reporting a setup failure.

| File | Purpose |
| --- | --- |
| `schemas_174.json` | Layouts of 707 lobby messages in 87 protocol groups. |
| `retail_templates.json` | Decoded reference-server templates, including catalogs and prices; display-name markers are anonymized. |
| `extracted_items.json`, `extracted_general_unlocks.json` | Cosmetic names, types, rarity, categories and rewards. |
| `extracted_heroes.json` | Hero identifiers and names. |
| `extracted_maps.json` | Map metadata and seasonal variants. |
| `ai_spawners_174.json` | The training bots: each map's AI spawners (hero, team, place, route) and hint points, and the bot heroes' bodies, health and speed (`tools/extract_ai_spawners.py`). |
| `resource_keys_174.json` | Game content keys used by event resources. These are not account credentials. |
| `lobby_state_blob.key` | Local 64-byte signing input for the target build's lobby handshake. |
| `extracted_events_174.json` | Verified scene mappings used by regression tests. |
| `announced_crcs_174.json` | Protocol identifiers used by the schema-extraction tool. |
| `arcade_cards_174.json` | The Arcade's cards (`tools/extract_arcade_cards.py`). |
| `game_schemas_174.json`, `game_schemas_174.txt` | Layouts of the game link's messages (`tools/dump_schemas.py`). |
| `game_heroes_174.json` | Each playable hero's GUID, name and body (`tools/extract_game_heroes.py`). |
| `game_maps_174.json` | Each map's modes, server-owned placeables and spawn points (`tools/extract_game_maps.py`). |
| `game_queues_174.json` | The queue cards: modes, maps and team sizes (`tools/extract_game_queues.py`). |
| `game_graphs_174.json.gz` | The statescript graphs the game server runs (`tools/extract_game_graphs.py`). |
| `game_stats_174.json` | The stats the Tab board lists for each hero and mode (`tools/extract_game_stats.py`). |
| `game_ult_charge_174.json` | Optional local override for the community estimates built into the Python server. |
| `bnet/descriptors.pb` | Protocol descriptors for the local Battle.net service. In the legacy layout this is `ow174/bnet/descriptors.pb`. |
| `game_movers_174.json` | Each hero body's character mover values (speeds, jump, gravity, capsule), from `tools/extract_game_movers.py`. |
| `collision_keys_174.json` | Storage keys of each map's collision chunk in build 104319, so the server can read the collision from your own game (`tools/build_collision.py`, cache in `cache/collision`). |
| `collision_props_174.json` | Storage keys of each map's props (fences, crates) and of their models, and which game modes keep which props; added to the same collision cache. |
| `statescript_codecs_174.json` | What the client reads for each statescript state class in a frame (property block and class reader, read in the client code by `tools/extract_statescript_codecs.py` in IDA); the encoder and the test reader use it. |
| `statescript_names_174.json` | Names for `tools/ssq.py`: class aliases (our research catalog and the VM code), the STU fields that hold a variable (OWLib's STU types), logical buttons, the hero select and HUD graphs, each game mode's scripts (the 1.74 0C5 assets) and the variable names the server code uses. |

The raw reference capture was from client 1.68 and was decoded using 1.74 schemas; templates are not a claim that every original live-service feature has been reproduced. The game client, raw captures and process dumps are not distributed here.

### Optional static ray geometry

In the prepared research checkout, use the local 104319 installation to generate the extra ray data for the selected maps.

```powershell
.\.venv\Scripts\python.exe tools\build_collision.py 0688 0CD3 --ray-geometry
```

The tool reads the installation from `game_path.txt` or an executable path given with `--game`. It writes `0688.rays.col` and `0CD3.rays.col` beside the existing movement caches. Runtime preparation copies each selected map's optional ray cache into the external pack and records its SHA-256.

Explicit static queries can use `collision.RayWorld.load(map_guid, mode_guid)`. It verifies the map and original chunk key, then selects props for the requested mode. A missing or mismatched cache produces a load error. Base coverage includes the core chunk and the supported static shapes in the existing prop index. Sphere and capsule props with positive uniform scale use analytic intersections. Nonuniformly scaled round props retain the existing triangle projection.

For a wider static query, use PowerShell 7 and the built DataTool from OWLib revision `7afe5f52706e7002ed8c2717299cd60846f3fa3a`. List unindexed client-made root definitions, export their local records and build the selected ray caches.

```powershell
$game = 'C:\path\to\Overwatch'
$dataTool = 'C:\path\to\owlib\DataTool\bin\Release\net6.0'
$records = 'C:\path\to\local-storage\ray-props.json'
$roots = @(.\.venv\Scripts\python.exe tools\build_collision.py 0688 0CD3 --list-unindexed-props | ConvertFrom-Json)
& ./tools/export_casc_records.ps1 -GameRoot $game -DataToolDir $dataTool -AssetGuid $roots -IncludeRootModels |
    Set-Content -LiteralPath $records -Encoding utf8
.\.venv\Scripts\python.exe tools\build_collision.py 0688 0CD3 --ray-geometry --prop-records $records
```

The exporter verifies the selected definitions and models against the local content manifest, including both the container and the slice for bundled records. The cache builder reopens those records through CASC and resolves authored root model references in placeable order. It records the placement content key and the catalog's SHA-256 so changed inputs trigger a rebuild. The runtime pack uses the resulting cache without the local catalog. Attached child geometry, server-generated and dynamic objects, and authored ability filtering still need verification. Keep catalogs and generated caches local.

### Local raycast filter and receiver metadata

Use the build 104319 DataTool XML dumps for asset types `097` and `098` and the same pinned OWLib checkout. Keep the dumps and the JSON catalog outside the repository.

```powershell
.\.venv\Scripts\python.exe tools\extract_raycast_metadata.py 'C:\path\to\xml' --owlib 'C:\path\to\owlib' --output 'C:\path\to\local-storage\raycast-metadata.json'
```

The export checks that receiver overrides reference present filters. It retains ordered rules and references, distinguishes unset results from zero flags, and records a SHA-256 for each source XML file. Runtime interpretation of the rules and receiver results still needs separate evidence.

### Local score event metadata

Export the build 104319 `003F.054` settings from a DataTool `054` XML dump with the pinned OWLib checkout. Keep the output outside the repository.

```powershell
.\.venv\Scripts\python.exe tools\extract_game_stats.py 'C:\path\to\xml' --owlib 'C:\path\to\owlib' --score-events 'C:\path\to\score-events.json'
```

The export records the source hash, ordered label rows and `StatEventScoreScaler` rows. Event enums use the numeric values from the pinned types. Label rows retain text resource IDs, duplicate event aliases and anonymous fields for comparison with the original asset. The catalog gives event configuration. Thresholds, score decay, kill credit, and the message that carries an award to the HUD still require separate evidence.

### Additional statescript definitions

Use the build 104319 `003` and `01B` XML dumps and the pinned OWLib checkout to include additional entity definitions in a separate local graph bundle. Repeat `--entity` for each hexadecimal `003` asset index.

```powershell
.\.venv\Scripts\python.exe -X utf8 tools\extract_game_graphs.py --xml 'C:\path\to\xml' --owlib 'C:\path\to\owlib' --entity 1403 --entity 143E --output 'C:\path\to\entity-graphs.json.gz'
```

The bundle's `entities` table holds the added definitions by full asset GUID. Their initial graphs retain definition order and use the same graph reference traversal as the hero catalog.

Add `--mode-xml 'C:\path\to\xml\0C5'` to the same extraction command to include the initial mode entity variables from build 104319. The output must be outside the repository. The server resolves these members by mode GUID, independently of the hero and mode schema defaults. Supply the bundle through `--game-graphs` when preparing the runtime.

To serve the bot spawn and death graphs from a prepared pack, include the six Practice Range bot body indexes `1403`, `143E`, `146E`, `14A9`, `14D2` and `14ED`, then pass the generated bundle to the runtime preparation tool. Point `ORPHEUS_RUNTIME_DIR` at the new pack when starting the server.

```powershell
.\.venv\Scripts\python.exe tools\prepare_server_runtime.py --source 'C:\path\to\local-research' --destination 'C:\path\to\orpheus-bot-runtime' --map 0688 --game-graphs 'C:\path\to\entity-graphs.json.gz'
```
