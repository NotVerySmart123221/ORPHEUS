# Local inputs of the offline service

Nothing in this folder but this file is in the repository. The offline service and its tests read these files from
here, and each machine prepares its own copies from an authorized installation and prepared data. The same rule as
the server's runtime pack applies ([data/README.md](../../../../../data/README.md)): game data, keys and signing
input stay local.

| File | What it is | Where it comes from |
| --- | --- | --- |
| `schemas_174.json`, `game_schemas_174.json` | Message and component layouts | The pinned upstream commits named in [../README.md](../README.md) |
| `game_graphs_174.json.gz`, `statescript_codecs_174.json` | Statescript graph catalog and the client's codec table | Same |
| `game_heroes_174.json`, `game_maps_174.json`, `game_movers_174.json`, `game_stats_174.json` | Hero, map, mover and stat tables | Same |
| `collision_keys_174.json`, `collision_props_174.json` | Inputs of the map collision reader | Same |
| `resource_keys_174.json` | Content key names and bytes | Same |
| `arcade_174.json` | Mode cards | Same; [../README.md](../README.md) says how it was put together |
| `owner_delta_layouts_174.json` | Owner-delta layouts of the weapon graphs | `tools/assets/reload_layouts.py`, from the installation |
| `lobby_state_blob.key` | The build's 64-byte state-blob signing input | The same input as the server's `data/lobby_state_blob.key` |
| `cmf_keytable_104319.bin` | Build 104319's 512-byte manifest key table | TACTLib's `ProCMF_104319` |
| `tact_keys_174.json` | Nine TACT keys by key name, as `{"NAME": "hex"}` | The client's own key table; the same values are in `resource_keys_174.json` |

Three more local files live beside the code that reads them:

- `tools/lobby/oracle_graphs_174.json`: graphs the pinned catalog lacks, written by `tools/assets/graph_record.py`.
- `tools/assets/stu_names.json`: STU names, written by `tools/assets/stu_names.py` from an OWLib checkout.
- `tools/lobby/data/` and `tools/lobby/vendor/cache/`: collision meshes, navigation grids, catalogs, the mover's map
  caches and player profiles. `tools/lobby/run.py` makes them from the installation when it starts.

Without the files in the table the test suite stops with a message that names the missing ones.
