# Practice Range map resource review

## Verified local assets

Map header `0x0790000000000688` is CKey-verified (372 bytes, MD5 `4ac5916b979f9a1a19ed03be8cbb817f`). Its `m_map` field (`0x4E87690F`) points to compiled map resource `0x0800000000000688`. The header's localized dynamic string names it `Maps\TrainingRoom\Training Room - Practice`.

Compiled map resource `0x0800000000000688` is CKey-verified (912 bytes, MD5 `32fd86999325f7adbe7c85fd104122b9`). Its leading words decode as floats; the first six are approximately `(-2261.499, -149.289, -2472.438, -1417.880, 2427.583, 599.236)`. I found seven aligned embedded resource GUIDs in the payload; each exists in the root CMF and its payload CKey verifies:

| Offset | GUID | Size | MD5 |
|---:|---|---:|---|
| `0xF8` | `0x0C00000000008087` | 4,112 | `3dfcb66584be754003a2b78c0f376b2e` |
| `0x110` | `0x0C000000000033F2` | 131,104 | `3591e265eda42fe4ed95b570c13f7d5e` |
| `0x130` | `0x0C0000000000740D` | 1,048,928 | `e3afe4ef57e5476d56decb047f8c3e27` |
| `0x138` | `0x0C0000000000740E` | 2,097,824 | `81af32bf7e96891a31d819785dea0b45` |
| `0x198` | `0x0C000000000080FF` | 4,112 | `ece01a2326ba65bd3f8ecfeb10ee6d94` |
| `0x1A0` | `0x0C00000000008146` | 43,720 | `04e2ad95bd02269fb774bf43724e3e09` |
| `0x1A8` | `0x0C00000000008145` | 87,408 | `977c871c26f4de4c8aa7afc10be56dd1` |

These references are evidence of linked map subresources. Their role as render geometry, collision, or another map subsystem is unresolved. The 912-byte payload has no aligned embedded `0x0BC` map-chunk GUID or `0x040` entity-definition GUID; this does not establish that the map has no separate placement chunks.

## OWLib format evidence and limits

OWLib defines `STUMapHeader.m_map` as a structured-data reference. Its `DataTool/SaveLogic/Map.cs` retrieves placeable streams by calling `map.GetChunkKey(variantGUID, type)` and parses the results as `teMapPlaceableData`. The format is file type `0x0BC`; its header has a placeable count, instance-data offset, placeable offset, and an unknown field. The parser dispatches separate entity, model, light, sound, effect, sequence, and area placeable records. Area records contain box, sphere, and capsule arrays, but OWLib does not establish that these are world collision geometry.

The same `Map.cs` contains a `teMap map = reader.Read<teMap>()` example only inside a block comment. The current OWLib source tree has `TankLib/teMapChunk.cs` but no `teMap` struct definition; `STUMapHeader.cs` contains fields only and no `GetChunkKey` implementation. I cannot confirm the 0x080 header layout, the chunk-key derivation, or a direct relationship between the seven 0x0C resources and collision/placement data from this source.

Primary sources: [STUMapHeader.cs](https://github.com/overtools/OWLib/blob/develop/TankLib/STU/Types/STUMapHeader.cs), [Map.cs](https://github.com/overtools/OWLib/blob/develop/DataTool/SaveLogic/Map.cs), [teMapChunk.cs](https://github.com/overtools/OWLib/blob/develop/TankLib/teMapChunk.cs). Local reads used the existing CKey-verifying resolver; no game or install files changed.
