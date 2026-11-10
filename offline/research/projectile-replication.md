# Soldier primary projectile replication

## Finding

Graph 596 state 10 selects projectile entity definitions `0x0400000000001C85` or `0x04000000000003D4` through dynamic variable 842. Both build-104319 assets are locally extracted and hash-verified. Each is an `STUEntityDefinition` with three component entries: `STUProjectileVisualComponent` (`0x2B1A5E8C`), `STUTargetTagComponent` (`0x445E7678`), and `STUEffectComponent` (`0x9B3898C8`). Neither definition contains `STUCharacterMoverComponent`, `STUModelComponent`, health, a statescript component, or an identified skin reference. The projectile visuals therefore come through their projectile/effect component data; there is no authored model or mover component to copy into a server-created body.

`world.create` can include a movement state, and `movement_record` can carry later position/velocity states ([world.py](../tools/lobby/vendor/ow174/game/world.py#L152), [create](../tools/lobby/vendor/ow174/game/world.py#L226), [movement record](../tools/lobby/vendor/ow174/game/world.py#L280)). That makes a server-ticked cosmetic path technically plausible. It does not establish that the client renders or simulates these particular projectile definitions from generic entity movement. The current writer serializes movement values; server code must advance position itself.

## Verified asset data

The resolver found both entity definitions in bundle `0x0D90A9C33A6B619C`, checked each payload MD5 against its CMF CKey, and decoded their component maps using OWLib's `STUEntityDefinition`, `teStructuredDataHashMap`, and asset-reference schema ([resolver](../tools/assets/retail_bundle_lookup.py#L119), [STUEntityDefinition](https://github.com/overtools/OWLib/blob/develop/TankLib/STU/Types/STUEntityDefinition.cs), [hash map reader](https://github.com/overtools/OWLib/blob/develop/TankLib/STU/teStructuredDataHashMap.cs), [asset reference reader](https://github.com/overtools/OWLib/blob/develop/TankLib/STU/teStructuredDataAssetRef.cs)).

| Asset | Bytes | CKey = payload MD5 | Bundle offset |
|---|---:|---|---:|
| `0x04000000000003D4` | 480 | `E111FDBA98E094B006408EC9533D6A05` | `0x408D6` |
| `0x0400000000001C85` | 496 | `4D9AE8AB73C3FD152D46BB62D0AE39DB` | `0x42BF3` |

Both `STUProjectileVisualComponent` instances reference contact set `0x012000000000030D`; the local payload also passed its CKey check (`D0B2C8A53C6618BBCC5C701C0F662AC7`). Hashed visual fields resolve to `.00D` resource references `0x03000000000016E7`, `0x0300000000000532`, and `0x03000000000008B0` (the last appears twice). The first two are locally readable and hash-verified (384 bytes / `363CA6DF1B9F7102A2D7BE9BFE244B2F`; 624 bytes / `3C9F0AF76FD24C25038EF2DD4A7820B9`). The resolver could not read `0x03000000000008B0`. `STUEffectComponent.m_effect` resolves to `0x0300000000000876`, also unavailable through the local readable-bundle resolver. These refs are evidence of attached visual/effect resources, but their effect payloads were not decoded.

The projectile visual schema exposes `m_contactSet` but leaves most reference, float, and flag fields hashed. The serialized variants share numeric values `1.0`, `3.0`, and a nested pair of `0.15` floats; their semantics are not identified by the available schema. Neither variant has an explicit speed or lifetime field whose name can be established from this STU record. The target-tag component is present; no collision-shape component or native mover was identified in the component map.

Graph 596 state 10 itself has `m_projectileMotions`, `m_projectileSpeed`, `m_projectileLifetime`, and `m_aimID` null. It specifies one projectile per shot, nine shots per second, and projectile visual recoil factor `0.1`. The other volley state (index 16) has speed `50` and lifetime `5`, but references different entity definitions (`0x0400000000001C86` and `0x040000000000072C`); those values do not establish primary-fire speed or lifetime. Source is the pinned [1.74 graph catalog](https://github.com/Boi-027/overwatch-1.74-lobby-server/blob/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/data/game_graphs_174.json.gz) and OWLib's [projectile visual component schema](https://github.com/overtools/OWLib/blob/develop/TankLib/STU/Types/STUProjectileVisualComponent.cs), [effect component schema](https://github.com/overtools/OWLib/blob/develop/TankLib/STU/Types/STUEffectComponent.cs), and [target-tag component schema](https://github.com/overtools/OWLib/blob/develop/TankLib/STU/Types/STUTargetTagComponent.cs).

## Smallest grounded path

Use the original projectile definition as a cosmetic network entity, if the client accepts its GUID index and default components; create it at a server-chosen shot origin and send server-computed position updates. Resolve impact/damage separately on the server. This avoids depending on the currently misaligned native tracer, but it is a diagnostic implementation choice, not proven client behavior. Calibrate the server origin against the graph's authored muzzle hardpoint `0x0DC000000000002E`; the exact world-space hardpoint offset is not decoded here. Keep projectile speed, lifetime, collision, and occlusion as explicit unknowns until measured or sourced.

The result can reproduce an authoritative straight visual path and independently validated hit logic if the client renders the entity. It cannot yet claim native projectile movement, collision, damage, or muzzle placement. No game files or settings were changed.
