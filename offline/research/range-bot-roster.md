# Practice Range bot roster from 1.74 map data (2026-11-02)

Source: map entity-placement chunk `0x0DD0000B00000688` (type `0x0BC`, placeable category `0xB`), read through `find_asset_payload` (CKey-verified).

## Chunk layout

- Header: `u32 count = 538`, `u32 end of records = 77800`, then 16-byte records start at offset 16.
- Record: 16-byte UUID; `+0x10` u32; `+0x14` u32 record size; `+0x18` u64 entity definition; `+0x30` position `f32 x, y, z` (Y up); `+0x3C` scale `f32 x3`; `+0x48` rotation quaternion `f32 x, y, z, w`; the last u32 is a **self-relative offset** to the record's STUv1 override block (`DUTS`) after the records.
- 70 distinct entity definitions are placed. None is a bot body: bots are spawned at runtime.

## Bot spawners

All 15 bot spawners are entity `0x04000000000013F7` (164-byte definition, one component). Each override block names the bot hero to spawn (the hero reference sits after class `0x99257985` at block `+0x30`).

| # | hero | position (x, y, z) | yawÂ° | override |
|---:|---|---|---:|---|
| 3 | 16C | 74.34, -2.00, 6.20 | 145 | 92 B, flag 0 |
| 19 | 16B | 82.36, -2.00, -12.02 | 325 | 156 B, flag 1 |
| 69 | 16B | 43.22, 2.00, 7.86 | -36 | 92 B, flag 1 |
| 70 | 16E | 93.65, -4.00, 43.80 | -125 | 492 B (extra lists) |
| 75 | 16C | 43.58, 11.00, 0.01 | -215 | 812 B (extra lists) |
| 276 | 16B | 46.48, 2.00, 10.19 | -36 | 92 B, flag 1 |
| 287 | 16D | 61.70, -2.00, -2.77 | 145 | 156 B, flag 0 |
| 366 | 16B | 92.66, -2.00, -7.46 | -36 | 92 B, flag 1 |
| 403 | 16B | 73.27, -2.00, -19.08 | 325 | 92 B, flag 1 |
| 408 | 16B | 51.10, 5.00, 6.92 | 145 | 492 B (extra lists) |
| 429 | 16B | 85.62, -2.00, -9.71 | 325 | 156 B, flag 1 |
| 501 | 16E | 100.59, -4.00, 34.01 | -125 | 620 B (extra lists) |
| 506 | 16E | 104.06, -4.00, 29.12 | -125 | 556 B (extra lists) |
| 522 | 16E | 97.12, -4.00, 38.90 | -125 | 492 B (extra lists) |
| 523 | 16C | 71.08, -2.00, 3.89 | 145 | 92 B, flag 0 |

"flag" is the u32 just before `01010000` in the short blocks; its meaning (hostile, or "fires"?) is unconfirmed.

## Bot heroes

| hero | gameplay entity | graphs |
|---|---|---|
| `0x02E000000000016B` Training Bot | `0x0400000000001403` | 0033, 0BA8, 0BE2, **0BA3 (Volley weapon)**, 174F, 2206 |
| `0x02E000000000016C` | `0x040000000000143E` | 0033, 0BA8, 0BE2, 0BA3, 174F |
| `0x02E000000000016D` | `0x040000000000146E` | 0033, 0BA8, 0BE2, 0BA3, 174F |
| `0x02E000000000016E` | `0x04000000000014ED` | 0033, 0BA8, 0BE2, 0BA3, 174F, 2206 |

All four share contact set `0x012000000000055A` and hero-level refs `0x0400000000000C5E` and `0x0400000000001430`. Names for 16Câ€“16E are not decoded yet (candidates per the 2016â€“2022 wiki: friendly, heavy/tank, sniper and rocket bots).

Graph references were decoded with `tools/assets/stu_refs.py`. That decoder also unlocks the obfuscated ids in UI asset `0x0A8800000000019C` (numeric-health lead).

## Server implementation (2026-11-02)

`tools/lobby/target.py` `ROSTER` holds all 15 spawners (hero body, team, position, quaternion yaw `2·atan2(qy, qw)`, behaviour, waypoints). The match creates them with the first player's body; login preloads heroes 16B–16E (without 16C/16D the client logs `Missing entity definition` and leaves).

Approximations (each a `shortcut:` in code):

- Patrol speed 2.5 m/s; B8E's mover speed and int var `0x0027` are undecoded.
- Shooters (B92) turn straight to the nearest enemy player within 30 m and deal 20 per shot every 1.75–2.5 s (B92's random range; 20 matches the Training Bot's Volley instance vars 53/198). No wall occlusion yet (roadmap step 3). Shots are not visible yet (see `remote-bot-statescript.md`).
- The friendly shooter (16D) never fires: bot-versus-bot combat is not modelled.
- All bots use 200 health; per-hero health is undecoded.
- Server movement is flat, so shots at bots on another floor use the wrong eye height until collision lands.

## Game-mode graphs (2026-11-02)

Map header `0x0790000000000688` lists modes `0x0230000000000018` and `0x023000000000005A`. Mode `0x18` references graphs `0x05800000000013C0`, `0x0580000000000C8E`, `0x05800000000013C1` (only in this mode) and `0x0580000000000C90`; mode `0x5A` has the same set minus `13C1`. These graphs are not in the pinned body-graph catalog, so a CASC graph parser is needed to read how spawner overrides choose bot behaviour. Web sources (Blizzard Dec 2019 notes, wiki mirror) don't settle which OW1 bots fired at the player; the map and mode data must decide it.

## Spawner behaviour decoded (2026-11-02)

Class names come from OWLib's generated STU types (sparse copy at `<external>/OWLib`, read as text; `tools/assets/stu_names.py` builds `stu_names.json`). `tools/assets/stu_graph.py` dumps STUv2 instances with those names.

- Spawner override class `0x99257985` = `STUECAISpawnerInstanceData`: `m_E74A5F9A` hero, `m_CD66847E` team (`TeamIndex`, at class `+0x38` in the map block), two bytes (`01 01`), and `m_E5CEAD94` = behaviour graphs with schema overrides (`STUStatescriptSchemaEntry` `0x5866C45E`).
- Waypoints are `STUECHintPointInstanceData` (`0xF199249B`) map entities carrying an identifier; the patrol graph's var `0x1B23` holds a list of those identifiers.
- The client registers every AI node class (`text.bin` has constructors for ECAEE401, E88BA88B, …, and the spawner class). Whether their logic runs client-side is unchecked.

| spawner | hero | team | behaviour |
|---:|---|---:|---|
| 3, 523 | 16C | 0 | static |
| 75 | 16C | 0 | patrol B8E, 6 waypoints at y=11 (1BF9?1BFE) |
| 287 | 16D | 0 | shooter B92 |
| 69, 276, 366, 403 | 16B | 1 | static |
| 19, 429 | 16B | 1 | shooter B92 (face 3/523) |
| 408 | 16B | 1 | patrol B8E (1B33, 1B34) |
| 70, 501, 506, 522 | 16E | 1 | patrol B8E at y=-4 (moving-target area) |

- Team split 4 friendly / 11 hostile matches the wiki's 2019-01-03 revision ("four friendly and eleven hostile training bots ... staying still, shooting, and moving"; export `research/wiki/`).
- Graph `B92` ("shooter"): random intervals `1.75..2.5` s (two `STU_BE0BD267` min/max), floats 0.5, 10, 0.2, 2.75. Graph `B8E` ("patrol"): Wait states, waypoint list `1B23`, int var `0x0027` (override value per spawner).
- Later wiki revisions (OW2, 9 friendly / 19 hostile) say "two standard bots shooting at two friendly bots" in the lower defence area, matching spawners 19/429 ? 3/523. Names (approximate): 16B Standard/Training Bot, 16C Friendly Bot, 16D Friendly Heavy Bot, 16E moving bot.

Hint-point positions (identifier ? x, y, z): 1B33 (71.63, 5, 21.22), 1B34 (50.37, 5, 6.36), 1B5D (104.06, -4, 29.12), 1B5E (95.91, -4, 23.33), 1B5F (100.59, -4, 34.01), 1B60 (84.28, -4, 22.44), 1B91 (97.12, -4, 38.9), 1B92 (80.81, -4, 27.33), 1BF7 (85.3, -1.74, 15.81), 1BF8 (92.23, -4, 28.08), 1BF9 (40.32, 11, 4.49), 1BFA (50.09, 11, 11.33), 1BFB (53.86, 11, 7.0), 1BFC (62.83, 11, 13.36), 1BFD (55.78, 11, 2.29), 1BFE (46.7, 11, -4.52), 1BFF (93.65, -4, 43.8), 1C00 (76.93, -4, 31.94).

Waypoint order per patrol spawner: 70 [1BFF, 1C00]; 75 [1BF9, 1BFA, 1BFB, 1BFC, 1BFB, 1BFD, 1BFE]; 408 [1B33, 1B34]; 501 [1B5F, 1B60, 1B5F, 1BF8]; 506 [1B5D, 1B5E, 1BF7]; 522 [1B91, 1B92].

## In-game proof: bots shoot back (session 20261102T022511.373996Z)

Holding W+D from spawn for 8 s put the server's player within 30 m of hostile shooters 19 and 429. The server log shows ten 20-damage hits from 22:30:33.8 to 22:30:42.8, health 200?0, then `player respawn body=A0000102` 3 s later. Client captures (`research/range-bots/shooters-kill-and-respawn.png`) show the hit-direction marker, the low-health vignette, the death view, and a fresh respawn in the spawn room with full HUD and ammo.

Divergence seen: the client body stuck on a spawn-room console, while the server's flat mover walked on into range. The player took damage through walls, and no shots were visible. Both are known limits: collision and line of sight are roadmap step 3.

## Visible bot fire (sessions 20261102T031804.046040Z, 20261102T032303.282980Z)

Shooters now fire the Volley's projectile effect (`0x040000000000148D`), re-created along its path every frame; damage lands on arrival and can be dodged ([bot-fire.md](bot-fire.md)). The probe session shows orbs flying from the doorway bot to the camera (`range-bots/bot-projectiles.png`). In normal play, ten landed shots took the player 200→0, and the player respawned 3 s later; that client view was still stuck on the console, so the orbs flew to the server's position.
