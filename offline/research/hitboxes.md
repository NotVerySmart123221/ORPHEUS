# Hitboxes

Original hit volumes for the Practice Range's bodies, read from the 1.74 model assets.
Extractor: `tools/assets/model_hitboxes.py MODEL_GUID`. Server table: `tools/lobby/hitbox.py`.

2026-11-07 follow-up supersedes the earlier 0.5 m hover-only correction. Native hardpoints identify the rendered idle as clip 3952 frame 0 plus additive breathing loop 3950, rather than the straight-arm 39FC/39FD poses. `tools/assets/range_bot_hitboxes.py` transforms every original volume through that bone hierarchy and encloses the complete four-second breathing loop. Guns now fold down/back beside the body. The server uses those seven generated capsules/spheres; [implementation and live evidence](bot-nameplates-2026-11-07.md).

## Where they live

1. Entity → `STUModelComponent` (`0x191835F9`) → `m_model` (`0x2D124BE2`): a model GUID (prefix `0x0D0`).
2. The model is chunked data:
   - 16-byte header.
   - Chunks of `(tag, size, size, hash)` followed by `size` bytes.
   - Tags: `MDLC`, `MESD`, `MRNM`, `MSTU`, `mskl` (byte-reversed in the file).
3. `MSTU` = `(offset, 0, size, 0)`, then an STU v2 blob. Its root is `STUModel` (`0x871BD3D0`).
4. Body parts are `STU_2DD96D6D` instances:

   | Field | Meaning |
   |---|---|
   | `m_30F925F4` | Skeleton bone ID. |
   | `m_89AABA94` | Kind: 1 = hit volume, 4 = ragdoll body. |
   | `m_9B0424EB` | Part info. For hit volumes it is `STU_00066F4C`. Ragdoll bodies use `STU_82C2BA2C` (physics values and a contact set). |
   | `m_B7C8314A` | Shape in the bone's frame. `STU_4614D0E5` is a sphere: `m_A8D12F28` centre, `m_radius`. `STU_4C8D9F5E` is a capsule: `m_10EFDE90` and `m_C320C90B` ends, `m_radius`. |
   | `m_D833F59A` | Shape type: 4 = sphere, 2 = capsule. |

5. **Critical (headshot):** the only hit volume whose part info has `m_EE483487`, a ref to `STU_A0CFB0F7`. On both models it sits on the head bone.
6. **`mskl` (skeleton):**
   - Header of 11 u64 offsets. Offset 1 holds the bind-pose 4x4 matrices: row vectors, translation in the last row. Offset 6 holds the u32 bone IDs.
   - The u16 at byte 92 is the bone count.

## Bodies

| Body | Model | Volumes |
|---|---|---|
| Range bots `0x1403`, `0x143E`, `0x146E`, `0x14ED` (all four) | `0x0D0000000000312E` | Head sphere r 0.44 at 1.48 m (critical); chest sphere r 0.48 at 0.87 m; lower sphere r 0.5 at 0.30 m; two shoulder spheres r 0.35; two forward arm capsules r 0.093 |
| Soldier: 76 body `0x03CF` | `0x0D00000000001137` | 13 capsules. Head 1.63–1.73 m r 0.18 (critical); chest r 0.273; pelvis r 0.256; legs r 0.2; arms r 0.05–0.062 |
| The other 11 Practice vs AI bodies | from each body's `STUModelComponent.m_model` | `tools/lobby/hitbox.py` `HEROES`. Bastion's 22 volumes include hulls (`STU_B3800E70`), read as bounding spheres |

- The model faces +Z.
- The server poses the volumes at the body's position, turned to its yaw.

## Damage (wiki values)

- Source: the Overwatch Wiki Soldier: 76 page, revision of 2022-09-30 (`research/wiki/`). These values are not traced in graph 596.
- **Heavy Pulse Rifle:** 20 damage up to 30 m, falling linearly to 6 at 50 m.
- **Headshots:** ×2.
- **Tactical Visor:** turns off both falloff and headshots.

## Evidence

Session `20261102T054221.964833Z`, bot `B0000501` at 32.6 m:

| Pitch | Result | Damage |
|---|---|---|
| Level | Body hit | 18 (falloff) |
| −252 / −324 | `critical=True` | 36 each |
| +108 | Body hit | 18 |

- `hitboxes/head-shot-zoom.png`: the crosshair rests just under the bot's head dome when the shot is critical. The authored head sphere covers the dome and upper shoulders.
- Geometry check: at 9.3 px/° the top of the server volume lands within about 2 px (≈0.1 m) of the rendered bot's top.
- `hitboxes/low-shot-zoom.png`: a shot just under the hovering bot still hits, because the lower sphere reaches the floor.

## Shortcuts

- Range volumes cover the authored idle animation, with up to 3.8 cm extra radius for its unsynchronized breathing phase. They follow entity position/yaw. Other animations and hero bot poses remain bind-pose approximations; no server/client skeleton clock is synchronized.
- Bot shots hit the player when the aimed point is within 0.1 m of a Soldier volume on arrival. The projectile's own radius is not decoded.
- The server eye stays at 1.6 m above the feet.
- Helix splash still measures to the bot's centre, not to the nearest volume.
- Not checked in game: bot shots against Soldier's volumes (unit-tested only).

## Wiki values adopted in the polish pass

From the same 2022-09-30 revision (details in `abilities.md`):

| Value | Wiki | Before | Now |
|---|---|---|---|
| Biotic Field healing | 35/s | 40/s | 35/s |
| Biotic Field radius | 4.5 m | 5 m | 4.5 m |
| Tactical Visor cost | 2310 points | 1650 | 2310 points, shown on the 1650-unit meter |
| Tactical Visor cast time | 1.2 s | — | still not modelled |
