"""Render clips of a scene built by import_anim.py from its first-person camera, for contact sheets.

  blender -b scene.blend --python render_clips.py -- OUTDIR CLIP [CLIP...] [--frames 6]

Each CLIP is a GUID of an armature's clip. Other armatures play a verified counterpart or hold their base pose
(import_anim.py --base) or bind pose. Writes OUTDIR/<CLIP>/fNN.png (Workbench,
arms and gun in two colours); tools/research/contact_sheet.py OUTDIR/<CLIP> makes the sheet.
"""
import json
import re
import sys
from pathlib import Path

import bpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
from clip_pairs import paired_clip


def entity_of(obj):
    return obj.get("entity") or obj.name.split(" ", 1)[1]


def belongs_to(action, obj):
    index = obj.get('entity_index')
    return action.get('entity') == entity_of(obj) and (index is None or action.get('entity_index') == index)


def main():
    argv = sys.argv[sys.argv.index("--") + 1:]
    frames_wanted = int(argv[argv.index("--frames") + 1]) if "--frames" in argv else 6
    argv = [a for i, a in enumerate(argv) if a != "--frames" and (i == 0 or argv[i - 1] != "--frames")]
    out, clips = Path(argv[0]).resolve(), argv[1:]
    scene = bpy.context.scene
    authored = json.loads(scene.get('clip_pairs', '[]'))
    scene.render.engine = "BLENDER_WORKBENCH"
    scene.display.shading.color_type = "OBJECT"
    scene.render.resolution_x, scene.render.resolution_y = 640, 360
    armatures = [o for o in bpy.data.objects if o.type == "ARMATURE"]
    for o in armatures:
        o.hide_render = True
        for child in o.children:
            if child.type == "MESH":
                child.color = (0.4, 0.5, 0.8, 1) if o.parent_type == "BONE" else (0.8, 0.6, 0.5, 1)
    by_obj = {o: {a.get('clip') or a.name.split(' ')[0]: a for a in bpy.data.actions
                  if belongs_to(a, o) and not a.get('base_pose')} for o in armatures}
    bases = {o: next((a for a in bpy.data.actions if a.get('base_pose') and belongs_to(a, o)), None)
             for o in armatures}
    for clip in clips:
        if not re.fullmatch(r"0x[0-9A-Fa-f]{16}", clip):
            raise ValueError(f"Invalid clip GUID {clip!r}")
        owners = [o for o, acts in by_obj.items() if clip in acts]
        if not owners:
            raise ValueError(f"No armature has clip {clip}")
        action = by_obj[owners[0]][clip]
        scene.render.fps = round(action["fps"])
        frames = int(action.frame_end) + 1
        for o, acts in by_obj.items():
            entity = entity_of(o)
            candidates = {paired_clip(entity_of(owner), clip, entity, acts, authored) for owner in owners} - {None}
            counterpart = next(iter(candidates)) if len(candidates) == 1 else None
            pick = acts.get(clip) or acts.get(counterpart) or bases[o]
            o.animation_data_create()
            o.animation_data.action = None
            for bone in o.pose.bones:
                bone.matrix_basis.identity()
            o.animation_data.action = pick
            if pick:
                o.animation_data.action_slot = pick.slots[0]
            print(f"render_clips: {entity} = {pick.name if pick else 'bind pose'}")
        n = min(frames_wanted, frames)
        for k in range(n):
            f = round(k * (frames - 1) / max(1, n - 1))
            scene.frame_set(f)
            scene.render.filepath = str(out / clip / f"f{f:02d}.png")
            bpy.ops.render.render(write_still=True)
        print(f"render_clips: {clip} {frames} frames -> {out}/{clip}")


main()
