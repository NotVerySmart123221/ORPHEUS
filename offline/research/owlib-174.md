# OWLib 1.74 branch (TankLib)

Local copy: `OWLib-overwatch-1.74/` (git-ignored), branch `overwatch/1.74` of overtools/OWLib, MIT. Used as a reference only: nothing from it is built or run. The `TACTLib` and `revorbstd` submodules are not included, so DataTool does not build as-is. A scan of the copy found no prompt-injection text or destructive scripts (C# sources, data JSON and a CI workflow).

## What it gave

- `TankLib/STU/Types/` names 586 STU types and some fields for build 1.74. Our `tools/assets/stu_names.json` already holds 7,782 classes, so the gain is the occasional field name.
- **Bastion's hull hit shape** `STU_B3800E70`: vertices `m_88FCECD7` (vec3[]), planes `m_0674FD43` (vec4[]), centre `m_7D1FC63E`, and `m_B1C6CDA2` = `m_volume`. We had read the volume as a radius. `model_hitboxes.py` now uses the sphere of that volume.
- **STU v2 arrays** (`TankLib/STU/STUInstance.cs`): a non-inline array field holds an int offset into the dyn-data block (header words 6 and 7: size, offset). There: count (u32), unknown (u32), data offset (u64, within dyn-data), then the elements.

## Practice vs AI data located, not decoded

- 51 entity definitions carry `STUAIBehaviorComponent` (`0x8FE674F9`); scan in `.scratchpad/find_class.py` (scratch).
- Entity `0x0400000000000462` is the full hero AI. Its component has:
  - 3 `m_difficultyProfiles` (`STUAIDifficultyProfile`: identifiers `0x0D8000000000195E`, `195F` and `1960`, plus an asset ref each). Most likely easy, medium and hard; not confirmed.
  - Per-game-mode behaviour scripts (`STUGameModeSpecificBehaviorScripts`), which are statescript graphs: `0x83E`, `0x6FE`, `0xBA7`, `0xB6F`, `0xC0E`, `0xC11`, `0x778`, `0xB50`, `0x340`.
- Graph `0x83E` has 300 instances. Most of its AI node types are unnamed even in TankLib 1.74, and it holds 11 `STUConfigVarFloat`. The bots' real aim and reaction values are in these graphs. Reading them is a statescript reverse-engineering task, the same kind as the earlier graph 596 and 0F4A work.
- Assault's timings (setup, round, capture) are not float literals in graph `0F4A`. They come through config vars and identifiers. `STUGameRulesetSchemaEntry` holds custom-game settings (min, max and default, mostly multipliers), not those base times.
