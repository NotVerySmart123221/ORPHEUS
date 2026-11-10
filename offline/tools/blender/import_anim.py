"""Build skinned models and one action per clip from tools/assets/anim_export.py JSON (Blender 4.4+, tested on 5.2).

  blender --python import_anim.py -- soldier.json [--clip 0x0A00000000001C00] [--base CLIP] [--save soldier.blend]
  blender -b --python import_anim.py -- soldier.json --save soldier.blend     # headless, with the self-check

The scene starts empty. Each exported entity becomes an armature (bones bone_XXXX, by skeleton bone ID as DataTool
names them) with its LOD 1 submeshes skinned to it, untextured. A child entity (the first-person gun) is parented to
the bone its hardpoint sits on. The scene camera is an approximate first-person view (Numpad 0).
Each action is named "<clip GUID> <length>" and keeps a fake user, so every clip stays in the file; pick one per
armature in the Action editor (Dope Sheet > Action). --clip picks the first entity's clip. Its children play known
counterparts or hold their base pose (bind pose when no base clip was paired).
Additive clips (priority bit 0x4: 260, 268) are baked on top of a base pose, frame 0 of --base (default --clip),
which is also kept as the action "<GUID> base pose". Keys are baked on every frame from the game's keys (linear
position/scale, slerp rotation). The game's space is
Y up; the first armature is turned +90 degrees on X so it stands up in Blender.
"""
import json
import math
import sys
from pathlib import Path

import bpy
from bpy_extras import anim_utils
from mathutils import Matrix, Quaternion, Vector

sys.path.insert(0, str(Path(__file__).resolve().parent))
from clip_pairs import paired_clip


def quat(v):
    """Game (x, y, z, w) -> mathutils (w, x, y, z)."""
    return Quaternion((v[3], v[0], v[1], v[2]))


def sampler(keys, rest, kind):
    """value(frame) from {frame: value}; no keys -> the rest value. Holds the first/last key outside them."""
    frames = sorted(int(f) for f in keys)
    convert = quat if kind == "rot" else Vector
    values = [convert(keys[str(f)]) for f in frames]

    def value(f):
        if not frames:
            return rest
        if f <= frames[0]:
            return values[0]
        if f >= frames[-1]:
            return values[-1]
        i = next(i for i in range(1, len(frames)) if frames[i] >= f)
        t = (f - frames[i - 1]) / (frames[i] - frames[i - 1])
        a, b = values[i - 1], values[i]
        return a.slerp(b, t) if kind == "rot" else a.lerp(b, t)
    return value


ADDITIVE = 0x4  # clip priority bit: 260/268 clips key deltas (rotations within 2 degrees of identity, positions ~0)


def clip_local(tracks, rest, additive):
    """local(frame) of one bone: the keys (rest where a channel has none), or for an additive clip its deltas on
    top of the bind pose (rest @ delta, no key -> no change), as io_scene_owm's additive import applies them."""
    if additive:
        sample = {k: sampler(tracks.get(k, {}), r, k)
                  for k, r in (("pos", Vector()), ("rot", Quaternion()), ("scale", Vector((1, 1, 1))))}
        return lambda f: rest @ Matrix.LocRotScale(sample["pos"](f), sample["rot"](f), sample["scale"](f))
    pos, rot, scale = rest.decompose()
    sample = {k: sampler(tracks.get(k, {}), r, k) for k, r in (("pos", pos), ("rot", rot), ("scale", scale))}
    return lambda f: Matrix.LocRotScale(sample["pos"](f), sample["rot"](f), sample["scale"](f))


def bone_name(bone_id):
    return f"bone_{bone_id:04X}"


def build_armature(entry):
    skeleton = entry["skeleton"]
    rest_local = [Matrix.LocRotScale(Vector(b["pos"]), quat(b["rot"]), Vector(b["scale"])) for b in skeleton]
    rest_model = []
    for i, b in enumerate(skeleton):
        rest_model.append(rest_local[i] if b["parent"] < 0 else rest_model[b["parent"]] @ rest_local[i])
    arm = bpy.data.armatures.new(f"OW {entry['model']}")
    obj = bpy.data.objects.new(f"OW {entry['entity']}", arm)
    bpy.context.scene.collection.objects.link(obj)
    bpy.context.view_layer.objects.active = obj
    obj.show_in_front = True
    arm.display_type = "STICK"
    bpy.ops.object.mode_set(mode="EDIT")
    edit = []
    for i, b in enumerate(skeleton):
        eb = arm.edit_bones.new(bone_name(b["id"]))
        eb.head, eb.tail = (0, 0, 0), (0, 0.02, 0)
        eb.matrix = rest_model[i].normalized()
        if b["parent"] >= 0:
            eb.parent = edit[b["parent"]]
        edit.append(eb)
    bpy.ops.object.mode_set(mode="OBJECT")
    return obj, rest_local, rest_model


# submesh flag (unnamed in TankLib) on Soldier's three small sight pieces (two quads and a 48-vertex shell, likely
# glass and screens that only read with textures): they start hidden so the untextured sight stays visible
CARD = 0x20
# shortcut: the first-person camera is placed by eye (game (0, 1.6, -0.1), facing +Z, 90 degrees); the game's camera
# offset and viewmodel FOV are not read. Upgrade if framing has to match the game exactly.
FP_CAMERA = ((0, 0.1, 1.6), (math.radians(90), 0, math.radians(180)), math.radians(90))


def build_meshes(entry, arm_obj):
    """One object per submesh, in its bind pose (model space = armature space), skinned with an Armature modifier.
    Objects are named "<model> sub<index> mat<material>"; submeshes with the CARD flag start hidden."""
    skeleton = entry["skeleton"]
    for sub in entry["submeshes"]:
        name = f"OW {entry['model']} sub{sub['index']} mat{sub['material']}"
        mesh = bpy.data.meshes.new(name)
        mesh.from_pydata(sub["positions"], [], sub["triangles"])
        mesh.validate()
        obj = bpy.data.objects.new(name, mesh)
        bpy.context.scene.collection.objects.link(obj)
        obj.parent = arm_obj
        obj.hide_set(bool(sub["flags"] & CARD))
        obj.hide_render = bool(sub["flags"] & CARD)
        groups = {}
        for v, influences in enumerate(sub["weights"]):
            for bone_index, weight in influences:
                group = bone_name(skeleton[bone_index]["id"])
                if group not in groups:
                    groups[group] = obj.vertex_groups.new(name=group)
                groups[group].add([v], weight, "ADD")
        obj.modifiers.new("Armature", "ARMATURE").object = arm_obj
        for poly in mesh.polygons:
            poly.use_smooth = True


def bake_clip(obj, entry, guid, rest_local, base_local, frames=None, name=None):
    """One action for a clip. An additive clip keys every bone, on top of `base_local` (the base pose)."""
    clip = entry["clips"][guid]
    skeleton = entry["skeleton"]
    frames = frames or clip["frames"]
    action = bpy.data.actions.new(name or f"{guid} {clip['duration']:.3f}s")
    action.use_fake_user = True
    action.use_frame_range = True
    action.frame_start, action.frame_end = 0, frames - 1
    action["fps"], action["duration"], action["priority"] = clip["fps"], clip["duration"], clip["priority"]
    action["entity"] = entry["entity"]
    action["entity_index"] = obj["entity_index"]
    action["clip"] = guid
    action["additive"] = additive = bool(clip["priority"] & ADDITIVE)
    slot = action.slots.new(id_type="OBJECT", name=obj.name)
    bag = anim_utils.action_ensure_channelbag_for_slot(action, slot)
    bones = obj.data.bones
    for i, b in enumerate(skeleton):
        tracks = clip["bones"].get(str(b["id"]))
        if not tracks and not additive:
            continue
        local_at = clip_local(tracks or {}, base_local[i] if additive else rest_local[i], additive)
        bone = bones[bone_name(b["id"])]
        # Blender: pose = parent_pose @ parent_rest^-1 @ rest @ basis; the game: pose = parent_pose @ local
        to_basis = bone.matrix_local.inverted() @ (bone.parent.matrix_local if bone.parent else Matrix())
        channels = {"location": [], "rotation_quaternion": [], "scale": []}
        previous = None
        for f in range(frames):
            loc, q, s = (to_basis @ local_at(f)).decompose()
            if previous is not None and previous.dot(q) < 0:
                q.negate()  # keep neighbouring keys on one hemisphere so interpolation takes the short way
            previous = q
            channels["location"].append(loc)
            channels["rotation_quaternion"].append(q)
            channels["scale"].append(s)
        for prop, values in channels.items():
            for axis in range(len(values[0])):
                fc = bag.fcurves.new(f'pose.bones["{bone.name}"].{prop}', index=axis, group_name=bone.name)
                fc.keyframe_points.add(frames)
                fc.keyframe_points.foreach_set("co", [c for f in range(frames) for c in (f, values[f][axis])])
                fc.keyframe_points.foreach_set("interpolation", [1] * frames)  # LINEAR
                fc.update()
    return action, slot


def activate(obj, action, slot):
    obj.animation_data_create()
    obj.animation_data.action = action
    obj.animation_data.action_slot = slot


def game_pose(entry, guid, rest_local, base_local, f):
    """Model-space bone matrices of a clip at frame f, chained the game's way (parent_pose @ local)."""
    clip = entry["clips"][guid] if guid else {"bones": {}, "priority": 0}
    additive = clip["priority"] & ADDITIVE
    out = []
    for i, b in enumerate(entry["skeleton"]):
        local = clip_local(clip["bones"].get(str(b["id"]), {}), (base_local if additive else rest_local)[i],
                           additive)(f)
        out.append(local if b["parent"] < 0 else out[b["parent"]] @ local)
    return out


def self_check(built, active, bases, frame):
    """Worst distance (m) between Blender's posed bones and the game's chain, attachments included."""
    bpy.context.scene.frame_set(frame)
    bpy.context.view_layer.update()
    world, worst = [], 0.0
    for e, (entry, obj, rest_local, base_local) in enumerate(built):
        guid = active[e] or bases[e]
        pose = game_pose(entry, guid, rest_local, base_local, frame if active[e] else 0)
        if entry["parent"] < 0:
            base = built[0][1].matrix_world
        else:
            parent_entry = built[entry["parent"]][0]
            bone = next(i for i, b in enumerate(parent_entry["skeleton"]) if b["id"] == entry["parent_bone"])
            base = world[entry["parent"]][bone] @ Matrix.LocRotScale(
                Vector(entry["offset_pos"]), quat(entry["offset_rot"]), None)
        world.append([base @ m for m in pose])
        for i, b in enumerate(entry["skeleton"]):
            got = obj.matrix_world @ obj.pose.bones[bone_name(b["id"])].matrix
            worst = max(worst, (got.translation - world[-1][i].translation).length)
    return worst


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    if not argv:
        raise SystemExit(__doc__)
    opt = {argv[i]: argv[i + 1] for i in range(1, len(argv) - 1, 2)}
    with open(argv[0]) as f:
        entities = json.load(f)["entities"]
    bpy.ops.wm.read_homefile(use_empty=True)  # no startup cube, camera or light in the way
    camera = bpy.data.objects.new("FP view (approx.)", bpy.data.cameras.new("FP view (approx.)"))
    bpy.context.scene.collection.objects.link(camera)
    camera.location, camera.rotation_euler, camera.data.angle = FP_CAMERA
    bpy.context.scene.camera = camera
    built = []
    for index, entry in enumerate(entities):
        obj, rest_local, rest_model = build_armature(entry)
        obj["entity"] = entry["entity"]
        obj["entity_index"] = index
        build_meshes(entry, obj)
        if entry["parent"] < 0:
            obj.rotation_euler = (math.radians(90), 0, 0)
        else:
            parent_obj, parent_rest = built[entry["parent"]][1], built[entry["parent"]][3]
            bone = next(i for i, b in enumerate(built[entry["parent"]][0]["skeleton"]) if b["id"] == entry["parent_bone"])
            obj.parent, obj.parent_type, obj.parent_bone = parent_obj, "BONE", bone_name(entry["parent_bone"])
            bpy.context.view_layer.update()  # parents are still in their bind pose here
            obj.matrix_world = parent_obj.matrix_world @ parent_rest[bone] @ Matrix.LocRotScale(
                Vector(entry["offset_pos"]), quat(entry["offset_rot"]), None)
        built.append((entry, obj, rest_local, rest_model))
    root_clips = entities[0]["clips"]
    first = opt.get("--clip", next(iter(root_clips), None))
    frames = root_clips[first]["frames"] if first else 0
    root = entities[0]["entity"]
    authored = [((entities[entry['parent']]['entity'], first_guid), (entry['entity'], second_guid))
                for entry in entities if entry['parent'] >= 0 for first_guid, second_guid in entry.get('pairs', {}).items()]
    active = [first] + [paired_clip(root, first, entry["entity"], entry["clips"], authored) for entry in entities[1:]]
    # Frame zero of the selected base clip (and its paired child clip) supplies the additive pose.
    base = opt.get("--base", first if first and not root_clips[first]["priority"] & ADDITIVE else None)
    bases = [base] + [paired_clip(root, base, entry["entity"], entry["clips"], authored) for entry in entities[1:]]
    built = [(entry, obj, rest_local, [clip_local(entry["clips"][g]["bones"].get(str(b["id"]), {}), rest_local[i],
                                                  False)(0) for i, b in enumerate(entry["skeleton"])] if g else rest_local)
             for (entry, obj, rest_local, _), g in zip(built, bases)]
    for (entry, obj, rest_local, base_local), base_guid, active_guid in zip(built, bases, active):
        for guid in entry["clips"]:
            action, slot = bake_clip(obj, entry, guid, rest_local, base_local)
            if guid == active_guid:
                activate(obj, action, slot)
        if base_guid:
            action, slot = bake_clip(obj, entry, base_guid, rest_local, base_local, frames=1, name=f"{base_guid} base pose")
            action["base_pose"] = True
            if active_guid is None:
                activate(obj, action, slot)
    scene = bpy.context.scene
    scene['clip_pairs'] = json.dumps(authored)
    if first:
        scene.render.fps = round(root_clips[first]["fps"])
        scene.frame_start, scene.frame_end = 0, frames - 1
    error = self_check(built, active, bases, frames // 2)
    print(f"import_anim: {len(built)} entities, {sum(len(e['clips']) for e in entities)} clips; active "
          + ", ".join(f"{e['entity']}={g or (b + ' base' if b else 'bind pose')}" for e, g, b in zip(entities, active, bases))
          + f"; self-check worst bone error {error:.6f} m")
    assert error < 1e-3, "posed bones do not follow the game's transform chain"
    scene.frame_set(0)
    if "--save" in opt:
        bpy.ops.wm.save_as_mainfile(filepath=opt["--save"])


main()
