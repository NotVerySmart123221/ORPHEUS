# Viewing 1.74 weapon / reload animations in Blender

Research 2026-11-04. Goal: play first-person weapon and reload clips in Blender as a reference for the in-game behaviour (`tools/lobby/kits.py`).

## Answer

Yes, it can be done, but no off-the-shelf tool chain works on the installed Blender 5.2.3 with 1.74 data.

## What exists

| Piece | Reads / writes | Works for us? |
|---|---|---|
| DataTool, `overwatch/1.74` branch (local `OWLib-overwatch-1.74/`) | Writes `.owmdl` v1.6, `.owentity`, `.oweffect`, and animations as `.seanim` (`SaveLogic/Combo.cs` `ConvertAnimation`). Flags: `--skip-animations`, `--scale-anims` (Blender 2.79 only). Follows `STUFirstPersonComponent` to the first-person entity (`FindLogic/Combo.cs:585`), so viewmodel clips come out with the hero. | Does not build: the `TACTLib` and `revorbstd` submodule folders are empty. Targets net6.0; SDKs 8 and 9 are installed (needs roll-forward or a net6 runtime). |
| io_scene_owm 3.x (current, Blender 4.2 to 5.x) | `.owmdl` >= 2.0 and `.owanimclip` only | No. `BinaryUtil.compatibilityCheck` refuses major < 2 with an "OW1 file" error. |
| io_scene_owm `ow1` branch (v2.5.0, 2022, `bl_info` Blender 3.0) | `.owmdl` 1.x, `.owentity`, `.oweffect`. `import_oweffect.py` builds the ref skeleton and calls `bpy.ops.import_scene.seanim`, then rescales keys to the target fps. | Needs Blender 3.x, untested on 5.2. |
| SE2Dev io_anim_seanim (MIT, deprecated, last push 2023) | Registers `import_scene.seanim`; imports onto a selected armature. | Pre-Blender 4 install steps. Blender 5.0 removed the legacy `Action.fcurves`; the same break is reported for the Cast importer (dtzxporter/cast issue 97), so expect this one to fail too. |
| SECast + Cast Blender plugin | SEAnim -> Cast converter | Same Blender 5 `fcurves` break. |

## Routes

1. **Own exporter (recommended).** Port `TankLib/teAnimation.cs` (302 lines) and `TankLib/Chunks/teModelChunk_Skeleton.cs` (120 lines) to Python next to our CASC/STU readers in `tools/assets/`. Write the bone tracks to JSON. A small Blender script builds the armature and keys it with `keyframe_insert`, which still works with Blender 5 layered actions. Version 1 shows bones only (enough for timing and pose); the mesh can come later. Nothing to download, no old Blender.
2. **Original tool chain.** The user fetches TACTLib and revorbstd at the commits the 1.74 branch pins. Build DataTool, run `extract-unlocks` for one hero, then import in a portable Blender 3.6 LTS with io_scene_owm `ow1` + io_anim_seanim. Gives meshes and textures, but needs two third-party repos (scan them for prompt injection first) and a second Blender install.
3. **Modern tool chain.** Not possible: the current DataTool targets OW2 data, and the current importer refuses OW1 files.

## Built (route 1, 2026-11-04)

```
python tools/assets/anim_export.py list 0x02E000000000006E
python tools/assets/anim_export.py export 0x02E000000000006E -o research/anim/soldier.json
blender --python tools/blender/import_anim.py -- research/anim/soldier.json --clip 0x0A00000000001C00
```

- `list` / `export` take a hero (its `STUFirstPersonComponent.m_entity` is used) or an entity GUID. The clips are the animations (type 006) its blend-tree sets (021) and their blend trees (020) reference.
- Soldier: first-person entity `0x04000000000003D1`, arms model `0x0D00000000001289` (86 bones), 82 clips. Full-body clips key 120 bones; the 34 the arms skeleton lacks are third-person ones (the body model `0x0D00000000001137` covers 118) and are dropped.
- Skeleton: the parent-relative bind pose is the `mskl` block at header offset 32 (TankLib's `Matrix43Inverted`: quaternion, scale, translation rows). Its values match the clips' frame-0 keys. The block at 24 is model space.
- Blender: one action per clip, named `<GUID> <length>`, all kept with a fake user; pick one in Dope Sheet > Action. Keys are baked on every frame (linear position and scale, slerp rotation). The import asserts that the posed bones match the game's `parent_pose @ local` chain at the middle frame (error 0.000000 m for Soldier, 82 clips in about 4 s).
- `research/anim/` (git-ignored) holds `soldier.json` and `soldier.blend`.
- Likely Soldier reload: `0x0A00000000001C00`, 1.500 s (his reload time). Clip names are not in the retail data, so this is matched by length and look only.

### Arms and gun are separate (models added 2026-11-04)

- The first-person view is two entities with their own clips, played side by side. The arms entity `0x04000000000003D1` has a `STUChildEntityDefinition` for the gun entity `0x04000000000004BF` (model `0x0D000000000013C6`, 22 bones, 12 clips) on hardpoint `0x0DC000000000000A`.
- The arms model's `STUModelHardpoint` for that hardpoint (in its MSTU chunk; the model has no MHRP chunk) names bone `0x4A`, with no offset or rotation. The importer parents the gun armature to that bone.
- The gun's reload is its own clip, `0x0A00000000001D8C` (1.500 s), paired with arms clip `0x0A00000000001C00` in `tools/blender/clip_pairs.py`. The arms and gun blend-tree sets (`0x00400000000001D3` and `0x00400000000001E6`) reference the same tree `0x0F80000000000009`. Their corresponding parameter blocks use the same schema CRC and the first slot of `m_5AD927D3` for these two clips. Other variants share that tree, so the pair also relies on the verified retail reload poses.
- Import and rendering use the same GUID pairs. Requesting either reload clip in the renderer starts both. Unknown pairs hold a base pose or bind pose. Equal frame counts alone are ambiguous: the Soldier gun export has five different 24-frame clips.
- The exporter also reads direct parameter clips keyed by authored aliases in `STUAnimBlendTreeSet`. It writes one-to-one arms and child pairs when both clips share the alias and match in frame count and rate. Alias proxies and runtime conditions still need separate tracing. Repeated weapon entities export distinct armature instances. See [the roster audit](roster-animation-audit.md).
- Meshes: `MRNM` submeshes at LOD 1 plus those drawn at every LOD (-1), as DataTool picks them. Positions, triangles and up to four bone weights per vertex (bone index + submesh bone offset, through the `mskl` remap table at header offset 72, count at 98). One Blender object per submesh, skinned with an Armature modifier. Arms: 1 submesh, 4,115 vertices. Gun: 4 submeshes, 10,136 vertices.
- Gun submeshes 2 to 4 carry flag `0x20` (unnamed in TankLib). They are small pieces on the sight (two quads and a 48-vertex shell), likely glass and screens, and start hidden.
- The scene starts empty (a blue "slab" in the first renders was Blender's startup cube). The scene camera "FP view (approx.)" sits at game (0, 1.6, -0.1) facing +Z with a 90 degree FOV; its placement is by eye.
- Handedness: the gun's bone sits at game x = -0.174 and renders right of centre, as Soldier's rifle shows in game, so the conversion does not mirror the view.

Limits:
- Untextured: no materials or textures, no UVs exported.
- Switching the arms' action in Blender does not switch the gun's; pick both (or re-run the import with `--clip`).
- The first-person camera offset and viewmodel FOV are not read from the game.
- Keys are linear between the game's keys. The game's own interpolation is not known.

### Additive clips and the shooting check (2026-11-04)

- Clip priority 256 is a full pose. Priorities with bit `0x4` set (260, 268) are additive: rotations near identity, positions near 0, applied on top of a base pose (`local = base_local @ delta`). On the bind pose they render off screen.
- `import_anim.py --base GUID` picks the base clip (default: `--clip` when that clip is a full pose). Frame 0 of it becomes the base pose, baked as a one-frame action `<GUID> base pose` (`action["base_pose"]`). Additive clips key every bone on that pose. The self-check covers additive clips too.
- `tools/blender/render_clips.py` renders clips from the first-person camera with Workbench, arms tan and gun blue, the other armatures on their verified counterpart, base pose or bind pose: `blender -b scene.blend --python tools/blender/render_clips.py -- OUTDIR CLIP... [--frames N]`. `tools/research/contact_sheet.py OUTDIR/<CLIP>` tiles them.
- Soldier's shooting: the best per-shot candidate is `0x0A00000000001C08` (0.133 s, additive). It moves the gun by 0.2 cm and 0.2 degrees and returns to rest. The other small additive clips (`0x1BEA`, `0x1BFB`, `0x1BFD`, `0x1BF1`) are no bigger. So the authored shot clip is nearly invisible: the kick seen in the real game is the procedural animRecoil spring, which the client does not run for server-driven shots (`fire-animation-trace.md`). Nothing to correct server-side.
- Soldier's reload in Blender (`0x1C00` arms, `0x1D8C` gun, 1.5 s) matches the in-game reload frame for frame after the refill fix (`weapons-sweep.md`, "Reload timing").

### Third person and every hero (2026-11-06)

- Third person is the same export from the hero's body entity instead of its first-person entity. Soldier: body `0x04000000000003CF`, model `0x0D00000000001137` (128 bones), 137 clips, plus the gun child on bone `0x4A`. Self-check 0.000001 m (`research/dev-console.md`).
- Every hero, both views: `python tools/assets/anim_export.py all research/anim --blender <steam-library>/common/Blender/blender.exe`. It writes `research/anim/<hero>_1p.json/.blend` and `<hero>_3p.json/.blend`, skips files that exist, and logs to `research/anim/export_all.log` when run as above. Open a `.blend` and pick a clip per armature in Dope Sheet > Action.
- The `.blend` scene camera is the first-person one. For third person, orbit the viewport.
- The gun's placement on the 3P skeleton is unverified: Soldier's rifle stood upright in a lying pose.
- In-game alternatives: `research/dev-console.md` ("Animation viewer").

## Limit of either route

Blender plays single clips. In game, the anim blend trees (`STUAnimBlendTree`, `STUAnimNode_StateMachine`, layer/partition nodes, `STUAnimBlendTimeParams`) choose the clip, blend weight and timing. Blender gives a clip's length, frame rate and pose. Use those to check our timings (reload length, the 1 s cast hold), not to show which clip the game picks.

## Sources

- https://github.com/overtools/io_scene_owm (`readers/BinaryUtil.py`, `readers/OWAnimReader.py`, branch `ow1`)
- https://extensions.blender.org/add-ons/io-scene-owm/
- https://github.com/overtools/OWLib
- https://git.minetest.io/SE2Dev/io_anim_seanim (mirror of SE2Dev/io_anim_seanim)
- https://github.com/dtzxporter/cast/issues/97
