# Practice Range resources in retail 1.74

## Result

Practice Range is present in build `1.74.0.0.104319` as world/map resource GUID `0x0800000000000688` and map-header GUID `0x0790000000000688` (resource type `0x09F`, low index `0x688`). The extracted map catalog names `000000000688.09F` “Practice Range”, marks it Enabled, and identifies its `MapGUID` as `000000000688.002`. The local retail CMF independently contains both GUIDs at that same index.

| Resource | GUID | Manifest size | Manifest CKey | Direct encoding lookup |
|---|---:|---:|---|---|
| Map header (`0x09F`) | `0x0790000000000688` | 372 | `4AC5916B979F9A1A19ED03BE8CBB817F` | Not found |
| World/map (`0x002`) | `0x0800000000000688` | 912 | `32FD86999325F7ADBE7C85FD104122B9` | Not found |

“Not found” means these CKeys did not resolve as loose entries in this installation's local CASC encoding table. It does not prove the assets are absent: map resources may be available through package/bundle records, which this small tool does not resolve. I did not hash either asset payload because direct CKey lookup returned no data.

The same extracted record reports two associated game-mode GUIDs (`0x0230000000000018`, `0x023000000000005A`), `MapType: OffenseDefenseMap`, no objective entries, and four placeable IDs (`136`, `558`, `564`, `649`). It lists two spawn records. This is catalog evidence for the prototype's map selection and basic spawn/placeable metadata, not a claim that the catalog fully describes every runtime dependency.

## Verification

- Local `.build.info` version field is `1.74.0.0.104319`; its matching build configuration names `prometheus-1_74_0_0-104319`.
- Read-only CASC probe verified the retail encoding and root MD5s against their local build configuration. Root CSV contains `Manifest/Win_SPWin_RDEV_EExt.cmf` with content key `473A98CEA8209538AF13CB0E487F54E4`.
- `tools/assets/practice_range_assets.py --map-index 0x688` parsed that CMF, checked its MD5 against the root content key, found 132 type-`0x09F` records, and returned the two rows above. The two asset CKeys are recorded in the manifest, but neither is in the local encoding table.
- Focused parser tests: `python -m unittest -v test_practice_range_assets` from `tools/assets` (3 tests passed). Tests cover the build-104319 key/IV vectors, encrypted CMF record parsing, and GUID resource-type decoding.

## Method and limits

TACTLib's `ContentManifestFile` specifies the 1.48+ 48-byte CMF header, 20-byte package entries, 29-byte GUID/size/flag/CKey hash records, and their ordering. Its `ProCMF_104319` source supplies the build-specific AES key and IV procedures. The tool mirrors those routines and uses the already-installed `cryptography` package; it imports the old local CASC reader read-only with an explicit retail install path. It does not write to the game or old project checkout.

Overtools' map listing enumerates `TrackedFiles[0x9F]` and loads each `MapHeader`; its `MapHeader` model maps `STUMapHeader.m_map` to `MapGUID`. Those references support the relationship between the header and world identifiers. The Boi-027 1.74 catalog is external, community-produced extraction data rather than a Blizzard source; its map name and metadata are cross-checked here only at GUID/index level against the local CMF.

Package/bundle resolution and hashing of the map-header/world payloads remain open. Do not treat the CMF CKey as proof of local loose-file availability or infer geometry completeness from the catalog entry alone.

Subsequent real-client evidence: original Practice Range loaded, acknowledged 21601/21602, rendered the spawn room and supported first-person walking/camera, repeated entry and teardown. This independently demonstrates runtime map availability without resolving standalone bundle extraction; see [entry report](practice-range-entry/REPORT.md). TACTLib's license is preserved in `tools/assets/LICENSE.TACTLib`.

## Sources

- Local build metadata: `<game-root>\.build.info` (version field); the local build configuration selected by the existing CASC reader.
- Local implementation: [`practice_range_assets.py`](../tools/assets/practice_range_assets.py), especially `parse_cmf` (line 66), `resource_type` (line 61), and filtered output (lines 140–157). Existing CASC reader: `<beta-project>\tools\casc.py`, `Storage` (lines 31–95), read without modification.
- [TACTLib `ContentManifestFile.cs`](https://github.com/overtools/TACTLib/blob/master/TACTLib/Core/Product/Tank/ContentManifestFile.cs#L11-L18) defines the hash record; [header and parser](https://github.com/overtools/TACTLib/blob/master/TACTLib/Core/Product/Tank/ContentManifestFile.cs#L56-L115) define v26 fields and payload order.
- [TACTLib `AssetPackageManifest.cs`](https://github.com/overtools/TACTLib/blob/master/TACTLib/Core/Product/Tank/AssetPackageManifest.cs#L21-L26) defines the 20-byte package-entry shape preceding hash records.
- [TACTLib build 104319 CMF crypto](https://github.com/overtools/TACTLib/blob/master/TACTLib/Core/Product/Tank/CMF/ProCMF_104319.cs#L7-L42) provides key/IV generation; [manifest crypto handler](https://github.com/overtools/TACTLib/blob/master/TACTLib/Core/Product/Tank/ManifestCryptoHandler.cs#L30-L91) specifies filename digest and AES-CBC use.
- [Overtools `ListMaps.cs`](https://github.com/overtools/OWLib/blob/develop/DataTool/ToolLogic/List/ListMaps.cs#L51-L57) loads resource type `0x9F`; [Overtools `MapHeader.cs`](https://github.com/overtools/OWLib/blob/develop/DataTool/DataModels/MapHeader.cs#L20-L50) exposes `MapGUID` from `STUMapHeader.m_map`.
- [Boi-027 `extracted_maps.json`, branch `wip`](https://raw.githubusercontent.com/Boi-027/overwatch-1.74-lobby-server/wip/data/extracted_maps.json) supplies the Practice Range name, header/world GUIDs, type, modes, placeables, and spawns; [game map catalog](https://raw.githubusercontent.com/Boi-027/overwatch-1.74-lobby-server/wip/data/game_maps_174.json) marks world `0x0800000000000688` Enabled.
