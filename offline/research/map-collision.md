# Practice Range collision (2026-11-02)

The client never sends its position (`commands.py`), so the server has to move the player with the same collision as the client. The original collision mesh ships with the map. This note covers what it is, how it was checked, and how the server uses it.

## Map chunks

The Range (map index `0x688`) has map chunks `0x0DD000TT00000688` in the root CMF, where TT is the chunk type.

| TT | size |
|---:|---|
| `01` | 212,768 |
| `0B` | 163,664 |
| `11` | 19,880,560 |
| `12` | 8,309,312 |
| others | under 24 KB |

OWLib's `teMAP_PLACEABLE_TYPE` stops at `0x11` (planar reflection) and guesses `COLLISION = 0x14`. In build 104319, `0x14` is empty (16 bytes), and `0x11` is the collision mesh. `0x12` is STUD data (pathing or regions, not decoded).

## Chunk 0x11 layout

| offset | contents |
|---|---|
| `+0x00` | u32 sphere count, u32 capsule count (0 on the Range and Hanamura; Anubis 0/1, Volskaya 0/4, Paris 22/0) |
| `+0x08` | u32 convex hull count (36 on the Range), u32 mesh count (1; 2 on Paris) |
| `+0x10` | u64 offset of the 36-byte sphere records, or 0 |
| `+0x18` | u64 offset of the 56-byte capsule records, or 0 |
| `+0x20` | u64 offset of the 88-byte hull records (`0x12F0910`, right after the triangles) |
| `+0x28` | u64 footer offset (`0x12F5A34`) |
| footer | one 56-byte record per mesh: u64 node offset `0x2CC8D0`, u64 vertex offset `0x30`, u64 triangle offset `0x9644B0`, u32 node count 216,031, u32 vertex count 183,434, u32 triangle count 312,867, then a u32, a float, a u32 and a u64 filter mask (`0x1FDF`) |
| vertices | 16 bytes each: f32 x, y, z, pad 0; world space, y up |
| nodes | 32 bytes each: a bounding box (6 floats) plus two u32. The root box is (-103.5, -38.5, -161.0) to (206.1, 51.1, 148.3). This is a BVH; the server does not use it. |
| triangles | 312,867, 32 bytes each: u32 a, b, c; three u32 neighbour vertices (-1 at an open edge); u32 material (17 values); u32 filter mask |

The block between the triangle end and the footer holds the props' convex hulls (decoded 2026-11-08). On the Range it is 36 records, then their data, and it ends exactly at the footer.

| hull record | contents |
|---|---|
| `+0x00` | u64 offsets (from the chunk start) of the vertices, planes, half-edges and per-face first half-edges |
| `+0x20` | f32 x, y, z centre (world space) |
| `+0x2C` | u32 vertex, face and half-edge counts (the half-edge count is twice the edge count) |
| `+0x38` | f32 volume, then f32 `m_2746D7E4` (unknown), 0.6, 0, 0.2 (likely friction and restitution) |
| `+0x4C` | u32 filter mask (`0x21FDF` or `0x391FDF` on the Range), then u32 (1 or 0), u32 0 |

- **Vertices:** 12 bytes each (f32 x, y, z), world space.
- **Planes:** 16 bytes each (normal, d, with n·v = d), one per face.
- **Half-edges:** 4 bytes each: s8 offset to the twin half-edge, u8 origin vertex, u8 face, u8 next half-edge round the face.
- **Face starts:** one u8 per face: its first half-edge.
- **Checks:** on all seven extracted maps (Range 36 hulls, Hanamura 22, Anubis 41, Volskaya 28, `0x6D3` 435, Uprising 56, Paris 28), every face loop closes, every loop vertex lies within 21 µm of its face plane, and the data ends at the footer.

The sphere records (36 bytes: f32 centre x, y, z, radius 0.0675, then 0.6, 0, 0.2, mask, u32) and capsule records (56 bytes: 8 zero bytes, two f32 x, y, z ends, radius 0.05–0.10, then 0.6, 0.33, 0.2, mask, u32) are not used: none are on the Range.

## Props

Every map prop is a model-group placement (chunk `0x01`, 217 records and 2,227 instances on the Range, read with OWLib's `teMapPlaceableModelGroup` layout: model, per-group look, per instance translation, scale and quaternion; world = T + R·(S·v)). A model's collision shapes are the body parts (`STU_2DD96D6D`) of its STUModel (`MSTU` chunk). Each part is either a triangle mesh (`STU_537E56E4`: vertices `teVec3A[]` and triangles in the same 32-byte layout as chunk 0x11) or a convex hull (`STU_B3800E70`: vertices, planes, half-edges and face starts as above, with `m_D833F59A = 1`).

All 2,227 instances were transformed and compared with chunk 0x11 (vertex match at 2 mm):
- **Meshes:** 97% of their triangles are already in the static mesh, so chunk 0x11 is the props' triangle shapes baked into world space. The rest are float-rounding misses, plus one placement 35 m under the Range (models `0x12C4`/`0x10F5`, 7,526 triangles, unreachable; not used).
- **Hulls:** none are in the static mesh. They are exactly the chunk's 36 hull records (models `0x1073` ×4 with five hulls each, `0x0FD1` ×10, `0x1217` ×6), already in world space.

The table right of the Range spawn is in the static mesh. Its top is at y 1.70–1.95 (0.70–0.95 m above the floor), x 56.8–57.6 and z ≤ 41.7, with a shelf at 0.18–0.25 m. Rays along the spawn's z (42.15) miss it. The mover's knee (0.55 m) and waist (1.1 m) rays passed between the top's faces, so the server's body walked onto the shelf and climbed the table, where the client is blocked. The mover now casts every 0.2 m from 0.55 m to 1.75 m.

The missing obstacle beside it is hull record 32 (model `0x1217`, centre (57.18, 1.43, 41.95)): a 0.3 × 0.6 m box with chamfered top edges, turned about 101° so the short side faces z, 0.66 m tall and 0.1–0.76 m above the floor. Its face is 2.35 m along +x from the spawn and its +z edge is at z ≈ 42.17, 2 cm past the spawn's z. So a centre-line ray from the spawn just hits it, and a centre-line-only mover slid off its edge and through it. A 0.4 m body goes round it.

Entities (chunk `0x0B`) near the spawn are small loose objects, such as stacked items at (59.2, 1.84, 40.1). Their collision is not extracted.

Paris has a second mesh: 16 vertices, 7 nodes and 14 triangles, stored between the first mesh's triangles and the A records. It is a flat plane at y = 6 m, about 1.4 km across, with material `0x33` and mask `0x242`. Its triangles index their own vertices from 0. The extractor reads only the first mesh.

Common filter masks are `0x4C0` (206,079 triangles), `0xC0`, `0x1FDF` (floors under every standing point checked), `0x1EC5`, `0x1B9F` and `0x19A`. Their bits likely select which kinds of objects collide (players, projectiles, cameras). Here every mask counts as solid.

## Checks

- **Floors:** a ray cast down from 1 m above the spawn point and five bot positions (276, 19, 70, 75, 408) hits a floor at exactly 1.000 m every time.
- **Open paths:** from the spawn, the ray toward the doorway bots runs 33 m before it hits anything. The spawn to bot 276 line is clear.
- **The earlier walk:** a server replay of the earlier in-game W+D walk now stops after about 2 s near the spawn-room console. That is where the client body got stuck. Without collision the server body walked 30 m through walls.

## In-game check (session 20261102T033922.336662Z)

A route search over the server mover (WASD only, spawn yaw) found W for 3 s, then W+D for 5 s, into shooter 19's sight. The run went like this:
1. **Leg 1:** the server logged (54.57, 1.00, 25.48). The client had walked out of the spawn room past the doorway bots (`range-bots/collision-route-leg1.png`).
2. **Leg 2:** the server logged (74.14, -2.00, 5.90), down in the lower hangar, and the client is there too.
3. **Shots:** both shooters then landed ten hits (200→0) while the client's health bar emptied (`range-bots/collision-route-end.png`).

An earlier run without collision had the server walk through walls while the client stuck on the spawn console. The client never reports its position, so agreement is judged from captures, not coordinates.

## Server model

- `tools/assets/map_collision.py` extracts the mesh and the hulls (fan-triangulated faces, with the hull's mask) to `tools/lobby/data/0688.collision` (`OWCOL2`). The launcher runs it when the file is missing or in the older `OWCOL1` format. The file is not in git. A cached `.nav` grid is rebuilt when its `.collision` file is newer.
- `tools/lobby/collision.py` casts rays in pure Python over a 2 m 3D grid. Loading takes about 1.4 s; a ray takes about 0.05-0.25 ms.
- `mover.Mover`:
  - walks with wall slides: rays every 0.2 m from 0.55 m to 1.75 m above the feet (`mover.WALL_HEIGHTS`), from five lines across the move (the centre, and ±0.5 and ±0.9 of the 0.4 m radius; `mover.SIDES`). The side lines catch props the centre line passes, and the body slides round their edges. A slide re-checks the centre line only;
  - steps up ledges of 0.5 m or less and follows steps down;
  - falls under 17.5 m/s² gravity and jumps at 6 m/s.
  - shortcut: capsule size, step height, gravity and jump speed are estimates, not the client's mover data.
- **Line of sight:**
  - bots fire only when the line from their chest to the player's chest is clear;
  - a player shot misses any bot behind the first surface on its ray.
