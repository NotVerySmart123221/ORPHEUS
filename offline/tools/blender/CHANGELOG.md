# Changelog

## 2026-11-04

- Blender now pairs Ashe’s arms and rifle for the default insert clip and both category variants of reload alias `84`.

- Import and rendering use authored pairs from the export alongside the reviewed Soldier pair. Actions keep their armature instance so both copies of a dual-wield model animate.

- Import and rendering use the same verified clip pairs keyed by both entity and clip GUID. Soldier's arms and gun reload pair works in both render directions. Unmatched armatures hold their base or bind pose. Rendering resolves output paths against the working directory.
