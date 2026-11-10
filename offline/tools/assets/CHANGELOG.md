# Changelog

## 2026-11-10

- Added `board_modes.py [MODE ...]`: each game mode's mode graph (the mode asset's `CF63B633`), its layout as the lobby's `Graph(...)` and the remote slots whose `STU_38EE1100` state posts game message `06DF.025`, which the client needs before Tab opens the board (research/lizrollin-runtime.md section 22). Without MODE: every mode of the lobby's map data. `test_board_modes.py` checks the slots `tools/lobby`'s `match.py` turns on.

## 2026-11-09

- Added `asset_users.py GUID... [--types 580,400]`, the reverse of `stu_refs.py`: which install assets reference these GUIDs. STU v2 stores a reference as `swap(GUID) ^ (field | field << 32) ^ header CRC64`, so each asset's field hashes and CRC give the exact bytes to find. A full scan reads 470,105 assets. `test_asset_users.py` checks the encoding against `_decode_asset_ref`.
- `retail_bundle_lookup.iter_payloads(storage, keep)` yields `(GUID, payload)` for every CMF asset that passes `keep`, loose or from its bundle (MD5-checked), parsing the bundle indexes once. `graph_messages.find()` uses it.

- Added `eye_heights.py`, which prints each hero body's first-person camera heights from the install: `STUFirstPersonComponent` (0x9D28963F) `m_E29993A5` standing and `m_816B830F` crouched, with OWLib's defaults 1.55 and 1.15 for a field the body leaves out. It covers 32 bodies plus D.Va's pilot, pasted into `tools/lobby/target.EYE_HEIGHTS`. Soldier: 76's 1.55/1.085 matches LizRollin's data reading.

- Added `graph_record.py OUT.json GUID...`, which writes graph catalog records in the vendored decoder's format (`tools/lobby/vendor/ow174/game/script/graph.py`) for graphs the pinned catalog lacks, read from the retail install with `graph_layout`'s reader. Each record holds only what `decode.py` reads.
  - Node classes take the codec table's names, so the decoder's StEC test matches.
  - Header bit counts are `max(1, (n - 1).bit_length())`, which holds in all 373 catalog graphs.
  - On the 109 catalog graphs our lobby sends, the decoder sees identical owner, remote and presence lists. The one difference is 0D24's flags list: the catalog has a node there that DataTool could not read, and the decoder's lists stay identical.
  - It wrote `tools/lobby/oracle_graphs_174.json` (006C 0079 0276 02CC 047E 050A 0B2F 0BA8 0CE4 0D28 0D4A 0D4B 0E8E 0E95 100B 1128 114D 214E) for the statescript oracle.

## 2026-11-08

- `reload_layouts.py` covers D.Va's graphs (mech HUD, Matrix meter child, pilot Light Gun, both ultimates); graphs the catalog lacks are read from the retail install (`--game-install`) with `graph_layout.layout` / `sync_vars`, whose rows match the catalog's on graphs 75, 463 and 596. The layouts file gains 8 rows; the 94 existing ones are unchanged.

- Added `voice_lines.py ENTITY_OR_VOICESET [--stimulus 0x2B] [--label HrSlct]`: a hero's voice lines from the speech manifest (`STUVoiceSetComponent` -> voice set -> line keys, stimulus GUIDs and debug labels). Reproduces Soldier's known spawn keys. `retail_bundle_lookup.find_asset_payload` takes the manifest to search (`manifest=`, default the root CMF).

- `map_collision.py` also extracts the props' convex hulls, which the map's static mesh lacks. They sit in the same chunk 0x11: header `+0x08` hull count, `+0x20` record offset; 88-byte world-space records pointing at vertices and half-edge face loops (layout in the docstring and research/map-collision.md). Each face is fan-triangulated into the triangle list with the hull's filter mask (`0x21FDF` or `0x391FDF`, both solid for `nav.SOLID`). Output magic `OWCOL2` (same layout; `OWCOL1` files lack the hulls). The Range gains 36 hulls (784 triangles, 313,651 in all), such as the 0.3 m-wide prop 2.35 m right of the spawn. Checked on all seven extracted maps: every face loop closes, every vertex is within 21 µm of its face plane, and the hull data ends exactly at the footer. Malformed loops or indices raise `ValueError`. Tests cover a cube hull after a mesh and three corruptions.
  - How the gap was found: the 2,227 model-group placements (chunk 0x01) were transformed with their models' STUModel collision shapes. 97% of those triangles match chunk 0x11 vertices to 2 mm, so the static mesh is the props' baked triangle shapes. Only the convex hulls (`STU_B3800E70`, 36 instances) are not baked into the mesh, plus one placement 35 m under the Range (models `0x12C4`/`0x10F5`, unreachable, skipped).
  - shortcut: the chunk's sphere (`+0x00`, 36 bytes: centre, radius) and capsule (`+0x04`, 56 bytes: two ends, radius) records are skipped. They are 5–10 cm thick, with none on the Range (Paris 22 spheres, Anubis 1 and Volskaya 4 capsules).

- Added `hero_movies.py OUTDIR HERO...`: each hero's Hero Info ability clips (STUHero loadouts field `0x77FED604` → STULoadout `m_infoMovie`, MOVI-wrapped Bink 2) decoded to MP4 through the install's own `bink2w64.dll` (ffmpeg has no Bink 2 decoder) plus `index.json`. Soldier: five 1280x720 29.97 fps clips. Usage and caveats: research/hero-info/README.md.

- Added `progression_catalog.py`: batch content-hash-verified extraction of typed hero/global loot pools, defaults, source membership, rarity, currency amounts and portrait-frame level awards. Preserves pool boundaries instead of treating the all-unlocked marker scan as a loot table. Uses the existing vendored login keyring for event-encrypted bundles; `load_storage().keys` exposes the reader's TACT key table. Output replaces its cache atomically; version 2 identifies icons separately from hero loadout slots. Regression covers bundled payload integrity and malformed unlock arrays. Build 104319 produces 6,603 items and 301 frame-award references.

## 2026-11-07

- `range_bot_hitboxes.py` generates the Range bot's seven volumes from model 312E, idle 3952 frame 0 and complete additive loop 3950. Parent rotations, uniform scale, shortest-arc quaternion sampling and local hit-volume transforms are preserved. The small motion envelope avoids guessing the native breathing clock. Regression checks independent native hardpoints (<3 mm RMS) and all intermediate motion samples against the served volumes. `model_hitboxes.hitboxes` accepts optional bone matrices; its default bind-pose behavior is unchanged.
- `stu_refs.py` includes model (`0x0D00`) and effect (`0x0300`) GUID namespaces in its default reference scan. Their omission hid the native Helix projectile's authored model and visual effects. Regression uses the verified model/effect field hashes and GUIDs; reserved-bit/index checks remain enforced.

## 2026-11-06

- `anim_export.py all OUTDIR [--blender EXE] [--hero GUID]`: every hero of `lobby/vendor/data/game_heroes_174.json` to `<hero>_1p.json` (first-person entity) and `<hero>_3p.json` (the body entity from the same table) in one process. Existing files are skipped, each JSON is written through a `.tmp` rename, and with `--blender` every new JSON also becomes a `.blend` (headless `import_anim.py` with its self-check). A failing hero is reported and skipped. Tracer: 89 + 173 clips, both `.blend`s, 1 min 44 s.

- `weapon_kit.py`: StateAbility gets the `'ability'` kind (`tools/lobby/kits.py` PAYLOADS: `soldier.ability_payload`, decoded for Helix), so routes through one are no longer cut. `alt()` now finds Symmetra's orb (graph 0x157C: button 41 under StateAbility 40, Volley 34), and `kit()` may list StateAbility states it used to leave out.
- `weapon_kit.py`: TrackRay, `STU_DEBD057B` and `STU_9D7BF987` are payload-free (`BARE`: their DSRs read no bits, tracer-verified on Sombra and Moira, research/statescript-payloads.md), and TrackTargets gets the `'targets'` kind (the no-target bit). `KINDS` holds the class-to-kind map once. Sombra's weapon set gains owner states 32 and 58; `ability_kit.cast` now finds Moira's orb (graph 4324: path (17,), Volley 10, anim (17, 18) -> 13). `STU_54FEB21C` too (empty-stub DSR, tracer-verified as Reinhardt): `ability_kit.cast` finds Fire Strike (graph 226: path (10, 4, 5, 16, 1), Volley 0) and an anim for Reaper's graph 32 (no cast there, so unused).

## 2026-11-05

- New `map_markers.py`: a map's placeable markers of one entity definition in one mode, from the map's entity placeable chunk 0x0DD0000B, as `{map GUID: [[x, y, z, yaw], ...]}` for the lobby (King's Row (Uprising)'s 68 omnic spawn markers, entity 0x16DC). Shortcut: the yaw's sign is unchecked.
- `graph_layout.py` reports the remote slots the client reads no presence bit for (`silent`: nodes that are not states, client-/server-only states) in the Graph(...) line and in `layout()`.
- New `graph_messages.py`: a graph's game-message nodes (state, remote slot, class, message GUID) and `--find` for every graph in the install that uses a message; test in `test_graph_messages.py` (needs the install).

## 2026-11-04

- `reload_layouts.py` also covers the Mercy Blaster, Bastion Sentry, Bastion body, Widowmaker HUD, common animation and HUD texture graphs.

- `anim_export.py` exports one-to-one arms and child clip pairs from shared authored blend-tree aliases when timing matches. Filtered exports include the matching clip, and repeated child entities keep separate attachments.

- Added `reload_layouts.py` to generate the unfiltered owner delta masks for lobby weapon and ability graphs from the pinned 1.74 catalog.
- `weapon_kit.py` skips null variable identifiers (Mercy's staff graph `0x006B` raised on them).
- `tools/blender/import_anim.py` plays additive clips (priority bit `0x4`) on a base pose (`--base`, default the full-pose `--clip`; baked as a one-frame `<GUID> base pose` action) and self-checks them. Added `tools/blender/render_clips.py`: Workbench renders of clips from the first-person camera for contact sheets.
- `anim_export.py` exports models too: LOD 1 render-mesh submeshes (positions, triangles, bone weights through the `mskl` remap) and, through `STUChildEntityDefinition`, child entities with the parent bone their `STUModelHardpoint` names. The JSON is now `{"entities": [...]}`, and `list` prints each entity's clips. Soldier's first-person gun (`0x04000000000004BF`) comes out on arm bone `0x4A`. `tools/blender/import_anim.py` builds one skinned object per submesh, parents child armatures to their bone, pairs child clips by frame count, starts from an empty scene with an approximate first-person camera, and checks the attachment in its self-check.

- Added `anim_export.py` (`list`, `export`): reads 1.74 animations (type 006, ported from OWLib's `teAnimation.cs`) and the `mskl` parent-relative bind pose, for a hero's first-person entity or any entity. It writes JSON for the new `tools/blender/import_anim.py`, which builds an armature with one baked action per clip in Blender 5.2 and checks the posed bones against the game's transform chain. Tests: `test_anim_export.py`. Notes: `research/blender-animations.md`.

- `weapon_kit.py` prints `reload()`: the reload StateAnim (shared alias `0x0C80000000000004`) with the owner states from the root down to it, and `alt()`: the button-29 LogicalButton route to its WeaponVolley (the states shown with the weapon, the states pulsed with a shot, the Volley), None where a state with an undecoded payload is on the way (Symmetra) or the right mouse is a scope or ability.

## 2026-11-03

- `ability_kit.py` prints each ability's `anim()`: the path from the StateAbility to its first StateAnim and that state, as `cast()` finds the Volley (`kits.Ability.anim`).
- `ability_kit.py` prints each ability's cast (`cast()`): the payload-free owner states from the StateAbility to its first WeaponVolley, and the Volley's owner state.
- `ability_kit.py` also prints the StateAbility's full state index (last field): var61 binds it, not the owner index (Zarya's E: owner 6, state 9).
- Added `ability_kit.py`: per hero body graph (weapon graphs and the shared graph 67 skipped), the first StateAbility waiting on Shift, E or right mouse, its icon UX presenters (those not under the StateAbility), the var472 / var61 presence bits, the authored cooldown (the var472 Stack's `STU_7197C080.m_01AFADB6`) and the graph's state-index width. It gives Soldier's Biotic Field (graph 599) as hand-mapped; `tools/lobby/kits.py` ABILITIES takes its output.
- `weapon_kit.py`: a BooleanSwitch on the chain always gets its 1-bit payload; one reached through `m_subgraphPlug` (neither true nor false) gets 1 (shortcut: the branch the client then runs is not checked). It used to be sent bare, which misaligned Brigitte's frame. Some data-flow mappings under such switches now count as entered too (Sigma, Genji, Ana, Pharah, Junkrat gain one mapping each); `tools/lobby/kits.py` keeps the in-game-checked sets for those.
- Added `weapon_kit.py`: derives a hero weapon graph's owner-state set from the pinned graph catalog (the switches above the ammo HUD, the weapon model and the fire button, plus the data-flow mappings under them), the volley's fire Stacks, the instance sync-var presence bits, the graph's total state count and, with `--hero`, the body's HUD texture. Stacks get their 2-bit payload; states whose payload is not decoded (StateAbility, TrackRay, `STU_DEBD057B`) are left out (shortcut: Sombra's sent bare misaligned the frame). It gives Cassidy's hand-mapped set exactly; `tools/lobby/kits.py` takes its output.
- `hero_content.py`: per hero also "defaults", the progression's default items (field `0x7846C401`, an inline list whose `m_unlocks` arrays hold 8 items) sorted into loadout slots by unlock class; the first item of a single slot wins ("Heroic" is listed before "RANDOM"). All 32 heroes give Classic / Heroic / DEFAULT plus one voice line; none has a default spray. `entries` skips null elements (bag -1; Orisa's list ends with one).
- `skin_themes.py` renamed `hero_content.py`: per hero it now also lists the unlocks (type 0x25) its progression asset names (`m_heroProgression`, field hash `0xDB803F2F` after an 8 x 0xFF marker); output `{hero: {skin_themes, unlocks}}`. All 32 heroes: 2,459 themes, 6,003 unlocks.
- `skin_themes.py`: writes each hero's skin themes (STUHero `m_skinThemes`, type 0xA6 GUIDs) as JSON for the lobby's login record.
- `graph_edges.py`: an output plug's field is only taken from a field of the plug's own node, so an int field that equals some plug's instance number no longer names the plug.
- Added `graph_edges.py`: prints a statescript graph's links (source node, output plug field, destination node, with state indexes). Moved from a scratch script; it gives the same 172 links on Zarya's weapon graph `0x0241`.
- `map_collision.py`: the triangle count now comes from the footer's mesh record (u32 at `+0x20`), not from the next block's offset. Paris stores a second, 14-triangle mesh right after the first mesh's triangles; only the first mesh is read. Paris now extracts (714,711 triangles), and the other five maps are unchanged byte for byte.
- `map_collision.py`: the triangle block ends at the first non-zero offset of header `+0x10`, `+0x18`, `+0x20`. Some maps carry one or two record blocks (counts at `+0x00`/`+0x04`) between the triangles and `+0x20`. Temple of Anubis, Volskaya Industries and Horizon Lunar Colony now extract; Hanamura's mesh is unchanged byte for byte. Paris still fails (a triangle index past the vertex block, not looked into).

## 2026-11-02

- `model_hitboxes.py` reads `STU_B3800E70` (Bastion's convex hulls) as the sphere of the hull's volume at its centre. OWLib's 1.74 TankLib names `m_B1C6CDA2` `m_volume`; it was first read as a radius. Vertices and planes are not used (a shortcut).

- Added `graph_layout.py`: reads a statescript graph from the install and prints its network layout (`Graph(index, owner states, instance sync vars, remote slots)`), entity-scope variables, and per state its remote slot and client-/server-only flag. Matches the pinned catalog on 0F4A, 0C90, 0C8F, 0C35 and 0CBB, and works for graphs the catalog lacks.

## 2026-11-02

- Added `model_hitboxes.py`: prints a model's hit volumes (STUModel body parts of kind 1, spheres and capsules) placed on the `mskl` bind pose, with the critical (headshot) flag. Feeds `tools/lobby/hitbox.py`. Two tests on synthetic chunk and skeleton data.

- Added `map_collision.py`: extracts a map's collision mesh (map chunk `0x0DD00011` + map index; vertices, triangles, filter masks) into the `OWCOL1` file the lobby's `collision.py` loads. Three tests on a synthetic chunk.

- Added `stu_refs.py`: brute-force decoder for mangled STU v2 asset references (each field hash × offset, kept when the result is a plausible GUID by its printed 12-bit prefix). Recovers the Training Bot's six known graphs. Two focused tests.

## 2026-11-01

- Vendored the beta project's read-only `casc.py` next to `practice_range_assets.py`, so the asset tools no longer depend on the beta folder.

- Resolve loose CASC assets before searching bundles, requiring both manifest size and payload CKey/MD5. Source bundle0/offset0 denotes a loose asset. This exposes previously missed native graph114D for HUD research.

- 2026-10-31: Preserved TACTLib MIT attribution in LICENSE.TACTLib; normalized the default install path.

- 2026-10-31: Added a read-only build 104319 CMF indexer and focused tests. It verifies the local retail build, checks the shared CMF content hash, and reports world/map-header GUIDs, manifest CKeys, and direct encoding lookup status. CMF format and crypto follow TACTLib's MIT-licensed `ContentManifestFile` and `ProCMF_104319` sources.

- 2026-10-31: Added a read-only retail bundle GUID/offset resolver and focused STUHero gameplay-entity reader. Asset bytes must match their CMF CKey; unreadable encrypted bundles are skipped.
- 2026-10-31: Require asset GUIDs to be exact integers in the unsigned 64-bit range; reject booleans and floats before CMF lookup.

## 2026-11-02 (later)

- Added `stu_graph.py`: dumps STU v2 instances (class, fields, decoded refs) with names when `stu_names.json` exists.
- Added `stu_names.py`: builds `stu_names.json` (class/field hash ? name) from OWLib's generated STU type sources, read as text.
