"""Export 1.74 animations (type 006) with the skinned models they play on to JSON for tools/blender/import_anim.py.

Formats ported from OWLib's 1.74 TankLib (teAnimation.cs, Chunks/teModelChunk_Skeleton.cs, teModelChunk_RenderMesh.cs,
MIT). An animation keys bones by skeleton bone ID; each key is the bone's transform relative to its parent (row
vectors, metres, Y up). Clip names do not ship in retail data, so clips are listed by GUID with their length.

An entity's child entities (STUChildEntityDefinition: the first-person gun under the arms) are exported too, each
with its own model, clips and the parent bone its hardpoint (STUModelHardpoint) sits on.

  python anim_export.py list 0x02E000000000006E                  # Soldier's first-person clips, with lengths
  python anim_export.py export 0x02E000000000006E -o soldier.json  # all of them (or name clips after the GUID)
  blender --python ../blender/import_anim.py -- soldier.json
  python anim_export.py all ../../research/anim --blender E:/.../blender.exe  # every hero, 1p + 3p JSON and .blend
"""
import os
import argparse
import json
import re
import struct
import subprocess
import sys
from pathlib import Path

from model_hitboxes import chunks
from animation_aliases import direct_bindings, shared_pairs, unambiguous_bindings
from practice_range_assets import resource_type
from retail_bundle_lookup import find_asset_payload, hero_gameplay_entity_guid, load_storage
from stu_graph import instances
from stu_refs import decode_refs

ANIMATION, BLEND_TREE_SET, BLEND_TREE, ENTITY, MODEL = 0x006, 0x021, 0x020, 0x003, 0x00C
FIRST_PERSON = 0x9D28963F  # STUFirstPersonComponent
CHILD, CHILD_HARDPOINT = 0x436BEFE6, 0x2D3CDFA9  # STUChildEntityDefinition m_child, m_hardPoint
# STUModelHardpoint (in the model's MSTU): hardpoint GUID, parent bone GUID (low 32 bits: bone ID), offset, rotation
HARDPOINT, HP_ID, HP_BONE, HP_POS, HP_ROT = 0xBE9DDFDF, 0xEDF0511C, 0xFF592924, 0x7DC1550F, 0xAF9D3A0C
ANY_PREFIX = set(range(0x1000))
POSITION, BLEND_INDICES, BLEND_WEIGHTS = 0, 4, 5  # teShaderInstance.ShaderInputUse
FORMATS = {0x2: "<3f", 0x4: "<2e", 0x6: "<4B", 0x8: "<4B", 0x9: "<4b", 0xC: "<I"}  # SemanticFormat -> struct


def unpack_rotation(a, b, c):
    """Smallest-three quaternion: the two top bits of a and b name the dropped component. Returns (x, y, z, w)."""
    axis = (b >> 15) << 1 | (a >> 15)
    a, b = a & 0x7FFF, b & 0x7FFF
    x = 1.41421 * (a - 0x4000) / 0x8000
    y = 1.41421 * (b - 0x4000) / 0x8000
    z = 1.41421 * (c - 0x8000) / 0x10000
    w = max(0.0, 1.0 - x * x - y * y - z * z) ** 0.5
    q = [x, y, z]
    q.insert(axis, w)
    return tuple(q)


def unpack_scale(a, b, c):
    return tuple(1 + (1 - v / 1024) / 1024 for v in (a, b, c))


def parse_animation(blob):
    """{fps, duration, frames, priority, bones: {bone ID: {"pos"|"rot"|"scale": {frame: values}}}}."""
    priority, bone_count = struct.unpack_from("<hH", blob, 0)
    duration, fps = struct.unpack_from("<f4xf", blob, 16)
    bone_list, info_tables = struct.unpack_from("<q", blob, 48)[0], struct.unpack_from("<q", blob, 72)[0]
    frames = int(fps * duration) + 1
    ids = struct.unpack_from(f"<{bone_count}i", blob, bone_list)
    index_fmt, index_size = ("B", 1) if frames <= 255 else ("h", 2)
    bones = {}
    for i in range(bone_count):
        table = info_tables + 32 * i
        counts = struct.unpack_from("<3H", blob, table)  # scale, position, rotation
        offsets = [table + 4 * o for o in struct.unpack_from("<6i", blob, table + 8)]
        tracks = {}
        for name, count, index_pos, data_pos, size in (("scale", counts[0], offsets[0], offsets[3], 6),
                                                       ("pos", counts[1], offsets[1], offsets[4], 12),
                                                       ("rot", counts[2], offsets[2], offsets[5], 6)):
            keys = {}
            for j in range(count):
                frame = abs(struct.unpack_from("<" + index_fmt, blob, index_pos + index_size * j)[0]) % frames
                if name == "pos":
                    keys[frame] = struct.unpack_from("<3f", blob, data_pos + size * j)
                else:
                    raw = struct.unpack_from("<3H", blob, data_pos + size * j)
                    keys[frame] = unpack_rotation(*raw) if name == "rot" else unpack_scale(*raw)
            tracks[name] = keys
        bones[ids[i]] = tracks
    return {"fps": fps, "duration": duration, "frames": frames, "priority": priority, "bones": bones}


def parse_skeleton(mskl):
    """[{id, parent (index or -1), rot (x, y, z, w), scale, pos}] in skeleton order: the bind pose relative to the
    parent. That is the block TankLib calls Matrix43Inverted (rows: quaternion, scale, translation); its values
    match the clips' keys, the Matrix43 block holds model-space transforms."""
    hierarchy, local, ids = (struct.unpack_from("<q", mskl, o)[0] for o in (0, 32, 48))
    count = struct.unpack_from("<H", mskl, 92)[0]
    out = []
    for i in range(count):
        m = struct.unpack_from("<12f", mskl, local + 48 * i)
        out.append({"id": struct.unpack_from("<I", mskl, ids + 4 * i)[0],
                    "parent": struct.unpack_from("<h", mskl, hierarchy + 6 * i + 4)[0] if hierarchy else -1,
                    "rot": m[0:4], "scale": m[4:7], "pos": m[8:11]})
    return out


def bone_remap(mskl):
    """The mesh bone-index -> skeleton-index table (TankLib's Lookup)."""
    offset, count = struct.unpack_from("<q", mskl, 72)[0], struct.unpack_from("<H", mskl, 98)[0]
    return struct.unpack_from(f"<{count}H", mskl, offset) if offset else ()


def parse_mesh(mrnm, remap, lod=1):
    """[{index, flags, material, positions, triangles, weights}] of the submeshes drawn at `lod` (and those drawn at
    every LOD, -1). weights: per vertex [[skeleton index, weight], ...]; empty when the model has no skeleton."""
    vb_ptr, ib_ptr, sm_ptr = struct.unpack_from("<3q", mrnm, 112)
    submeshes, vb_count, ib_count = mrnm[4], mrnm[10], mrnm[11]
    buffers = []
    for i in range(vb_count):
        count, _, stride1, stride2, n_elements, _, _, elements, data1, data2 = struct.unpack_from(
            "<IIBBBBIqqq", mrnm, vb_ptr + 40 * i)
        layout = [struct.unpack_from("<BBBBHH", mrnm, elements + 8 * j) for j in range(n_elements)]
        buffers.append((layout, (data1, data2), (stride1, stride2)))
    index_data = [struct.unpack_from("<IIq", mrnm, ib_ptr + 16 * i)[2] for i in range(ib_count)]
    out = []
    for s in range(submeshes):
        base = sm_ptr + 96 * s
        vertex_start, index_start = struct.unpack_from("<IH", mrnm, base + 52)
        indices_to_draw, _, bone_offset, index_buffer = struct.unpack_from("<HHHB", mrnm, base + 74)
        vertex_buffer, flags, material = mrnm[base + 88], mrnm[base + 89], mrnm[base + 90]
        if struct.unpack_from("<b", mrnm, base + 91)[0] not in (lod, -1):
            continue
        raw = struct.unpack_from(f"<{indices_to_draw}H", mrnm, index_data[index_buffer] + 2 * index_start)
        local = {}
        for index in raw:
            local.setdefault(index, len(local))
        positions, weights = [], []
        triangles = [[local[a], local[b], local[c]] for a, b, c in zip(raw[0::3], raw[1::3], raw[2::3])]
        layout, data, strides = buffers[vertex_buffer]
        for index in local:  # dicts keep insertion order: local index order
            vertex = vertex_start + index
            values = {}
            for use, layer, fmt, stream, _, offset in layout:
                if layer == 0 and use in (POSITION, BLEND_INDICES, BLEND_WEIGHTS) and fmt in FORMATS:
                    values[use] = struct.unpack_from(FORMATS[fmt], mrnm, data[stream] + strides[stream] * vertex + offset)
            positions.append(values[POSITION])
            if BLEND_INDICES in values and remap:
                w = values.get(BLEND_WEIGHTS, (255, 0, 0, 0))
                weights.append([[remap[b + bone_offset], v / 255] for b, v in zip(values[BLEND_INDICES], w) if v])
            else:
                weights.append([])
        out.append({"index": s, "flags": flags, "material": material, "positions": positions,
                    "triangles": triangles, "weights": weights if any(weights) else []})
    return out


def hardpoints(model_parts):
    """{hardpoint GUID: (bone ID, offset position, rotation (x, y, z, w))} from the model's STUModel."""
    if "MSTU" not in model_parts:
        return {}
    stu = model_parts["MSTU"]
    offset, _, size = struct.unpack_from("<3I", stu)
    out = {}
    for cls, _, _, fields in instances(stu[offset:offset + size]):
        if cls == HARDPOINT and HP_ID in fields and HP_BONE in fields:
            out[struct.unpack("<Q", fields[HP_ID])[0]] = (
                struct.unpack("<Q", fields[HP_BONE])[0] & 0xFFFFFFFF,
                struct.unpack("<3f", fields[HP_POS]) if HP_POS in fields else (0.0, 0.0, 0.0),
                struct.unpack("<4f", fields[HP_ROT]) if HP_ROT in fields else (0.0, 0.0, 0.0, 1.0))
    return out


class Assets:
    """Cache read-only payloads and decoded references for one export."""

    def __init__(self, game_install):
        self.storage = load_storage(game_install)
        self.cache = {}
        self.references = {}

    def get(self, guid):
        if guid not in self.cache:
            self.cache[guid] = find_asset_payload(self.storage, guid).payload
        return self.cache[guid]

    def refs(self, guid, types):
        if guid not in self.references:
            self.references[guid] = decode_refs(self.get(guid), ANY_PREFIX)
        return sorted({g for _, _, g in self.references[guid] if resource_type(g) in types})

    def close(self):
        for handle in self.storage._files.values():
            handle.close()


def entity_clips(assets, entity):
    """{model GUIDs}, {animation GUIDs} an entity reaches through its blend-tree sets and their blend trees."""
    models, clips = set(assets.refs(entity, {MODEL})), set()
    for tree_set in assets.refs(entity, {BLEND_TREE_SET}):
        for ref in assets.refs(tree_set, {ANIMATION, BLEND_TREE, MODEL}):
            if resource_type(ref) == ANIMATION:
                clips.add(ref)
            elif resource_type(ref) == MODEL:
                models.add(ref)
            else:
                clips.update(assets.refs(ref, {ANIMATION}))
    return models, clips


def entity_aliases(assets, entity):
    bindings = []
    for tree_set in assets.refs(entity, {BLEND_TREE_SET}):
        try:
            bindings.extend(list(direct_bindings(assets.get(tree_set))))
        except (IndexError, KeyError, LookupError, ValueError, struct.error) as exc:
            print(f'0x{tree_set:016X}: skipped animation bindings ({exc})', file=sys.stderr)
    return unambiguous_bindings(bindings)


def first_person_entity(assets, hero):
    """The entity a hero's gameplay entity names in its STUFirstPersonComponent (m_entity)."""
    body = hero_gameplay_entity_guid(assets.get(hero))
    blob = assets.get(body)
    refs = decode_refs(blob, ANY_PREFIX)
    for cls, start, size, _ in instances(blob):
        if cls == FIRST_PERSON:
            return next(g for off, _, g in refs if start <= off < start + size and resource_type(g) == ENTITY)
    raise LookupError(f"hero 0x{hero:016X} has no first-person component")


def children(assets, entity):
    """[(child entity, hardpoint GUID or 0)] from the entity's STUChildEntityDefinition entries."""
    refs = decode_refs(assets.get(entity), ANY_PREFIX)
    points = [(off, g) for off, field, g in refs if field == CHILD_HARDPOINT]
    out = []
    for off, field, g in refs:
        if field == CHILD and resource_type(g) == ENTITY:
            # an entry's m_hardPoint sits next to its m_child (8 bytes before it in 1.74 data)
            near = [p for o, p in points if abs(o - off) <= 16]
            out.append((g, near[0] if near else 0))
    return out


def entity_tree(assets, entity, parent=-1, hardpoint=0, out=None):
    """[(entity, model or None, parent index, hardpoint GUID, clips)] depth first, the given entity first."""
    out = [] if out is None else out
    models, clips = entity_clips(assets, entity)
    skinned = [m for m in sorted(models) if "mskl" in chunks(assets.get(m))]
    if len(skinned) > 1:
        print(f"0x{entity:016X}: several skinned models, using the first of "
              + " ".join(f"0x{m:016X}" for m in skinned), file=sys.stderr)
    here = len(out)
    out.append((entity, skinned[0] if skinned else None, parent, hardpoint, clips))
    for child, point in children(assets, entity):
        entity_tree(assets, child, here, point, out)
    return out


def clip_json(assets, guid, ids):
    a = parse_animation(assets.get(guid))
    # bones the skeleton lacks (a full-body clip keys the third-person legs too) are dropped
    a["bones"] = {str(k): {n: {str(f): v for f, v in t.items()} for n, t in tr.items()}
                  for k, tr in a["bones"].items() if k in ids}
    return a


def export(assets, root, wanted):
    """{"entities": [...]}: per entity with a skinned model its skeleton, mesh, clips and where it attaches."""
    tree = entity_tree(assets, root)
    aliases = [entity_aliases(assets, entity) if model is not None else {} for entity, model, *_ in tree]
    pairs = [shared_pairs(aliases[parent], aliases[i]) if parent >= 0 else {}
             for i, (_, _, parent, _, _) in enumerate(tree)]
    if wanted:
        wanted = wanted | {clip for pair in pairs for first, second in pair.items()
                           if first in wanted or second in wanted for clip in (first, second)}
    entities, index_of = [], {}
    for i, (entity, model, parent, hardpoint, clips) in enumerate(tree):
        if model is None:
            continue
        parts = chunks(assets.get(model))
        skeleton = parse_skeleton(parts["mskl"])
        ids = {b["id"] for b in skeleton}
        entry = {"entity": f"0x{entity:016X}", "model": f"0x{model:016X}", "parent": -1, "skeleton": skeleton,
                 "submeshes": parse_mesh(parts["MRNM"], bone_remap(parts["mskl"])) if "MRNM" in parts else [],
                 "clips": {}}
        if parent in index_of:
            parent_entry = entities[index_of[parent]]
            points = hardpoints(chunks(assets.get(int(parent_entry["model"], 16))))
            if hardpoint not in points:
                raise LookupError(f"hardpoint 0x{hardpoint:016X} of 0x{entity:016X} is not on the parent model")
            bone, pos, rot = points[hardpoint]
            entry.update(parent=index_of[parent], parent_bone=bone, offset_pos=pos, offset_rot=rot)
        for guid in sorted(clips if not wanted else clips & wanted):
            try:
                entry["clips"][f"0x{guid:016X}"] = clip_json(assets, guid, ids)
            except (LookupError, KeyError, struct.error) as e:
                print(f"0x{guid:016X}: skipped, unreadable ({e})", file=sys.stderr)
        index_of[i] = len(entities)
        if entry['parent'] >= 0:
            parent_clips = entities[entry['parent']]['clips']
            entry['pairs'] = {first: second for a, b in pairs[i].items()
                              if (first := f'0x{a:016X}') in parent_clips
                              and (second := f'0x{b:016X}') in entry['clips']
                              and all(parent_clips[first][key] == entry['clips'][second][key]
                                      for key in ('frames', 'fps'))}
        entities.append(entry)
    return {"entities": entities}


HEROES = Path(__file__).resolve().parents[1] / "lobby" / "vendor" / "data" / "game_heroes_174.json"


def export_all(assets, outdir, blender=None, only=()):
    """Every hero (HEROES, or the GUIDs in `only`) to OUTDIR/<name>_1p.json (first-person entity) and <name>_3p.json
    (body entity), skipping files that exist; with `blender`, each new JSON also becomes a .blend (import_anim.py,
    headless, with its self-check). A hero that fails is reported and skipped. Returns the failures."""
    outdir.mkdir(parents=True, exist_ok=True)
    importer = Path(__file__).resolve().parents[1] / "blender" / "import_anim.py"
    failures = []
    for key, hero in json.loads(HEROES.read_text(encoding="utf-8")).items():
        guid = int(key, 16)
        if only and guid not in only:
            continue
        name = re.sub(r"[^a-z0-9]+", "_", hero["name"].lower()).strip("_")
        for view, root in (("1p", lambda: first_person_entity(assets, guid)), ("3p", lambda: int(hero["body"], 16))):
            path = outdir / f"{name}_{view}.json"
            try:
                if not path.exists():
                    out = export(assets, root(), set())
                    tmp = path.with_suffix(".tmp")
                    tmp.write_text(json.dumps(out))
                    tmp.replace(path)  # no half-written JSON is ever skipped as done
                    print(f"{path.name}: {sum(len(e['clips']) for e in out['entities'])} clips", file=sys.stderr)
                blend = path.with_suffix(".blend")
                if blender and not blend.exists():
                    subprocess.run([blender, "-b", "--python", str(importer), "--", str(path), "--save", str(blend)],
                                   check=True, capture_output=True)
                    print(f"{blend.name}: saved", file=sys.stderr)
            except (LookupError, KeyError, ValueError, struct.error, subprocess.CalledProcessError) as e:
                failures.append(f"{path.name}: {e}")
                print(f"{path.name}: FAILED ({e})", file=sys.stderr)
    return failures


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--game-install", default=os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
    sub = parser.add_subparsers(dest="cmd", required=True)
    ls = sub.add_parser("list")
    ls.add_argument("guid", type=lambda v: int(v, 0), help="hero (its first-person entity is used) or entity")
    ex = sub.add_parser("export")
    ex.add_argument("guid", type=lambda v: int(v, 0), help="hero (its first-person entity is used) or entity")
    ex.add_argument("clips", type=lambda v: int(v, 0), nargs="*", help="default: every clip the entities reach")
    ex.add_argument("-o", "--output", required=True)
    every = sub.add_parser("all", help="every hero, first and third person, to OUTDIR/<hero>_{1p,3p}.json")
    every.add_argument("outdir", type=Path)
    every.add_argument("--blender", help="also build OUTDIR/<hero>_{1p,3p}.blend with this blender.exe")
    every.add_argument("--hero", type=lambda v: int(v, 0), action="append", default=[], help="only these heroes")
    args = parser.parse_args()
    assets = Assets(args.game_install)
    try:
        if args.cmd == "all":
            failures = export_all(assets, args.outdir, args.blender, set(args.hero))
            print(f"{len(failures)} failed" + "".join(f"\n  {f}" for f in failures), file=sys.stderr)
            return

        root = args.guid if resource_type(args.guid) == ENTITY else first_person_entity(assets, args.guid)
        if args.cmd == "list":
            for entity, model, parent, _, clips in entity_tree(assets, root):
                print(f"entity 0x{entity:016X}  model " + (f"0x{model:016X}" if model else "none")
                      + ("" if parent < 0 else "  (child)"))
                for clip in sorted(clips):
                    try:
                        a = parse_animation(assets.get(clip))
                    except (LookupError, KeyError, struct.error) as e:
                        print(f"  0x{clip:016X}  unreadable ({e})")
                        continue
                    print(f"  0x{clip:016X}  {a['duration']:6.3f} s  {a['fps']:4.0f} fps  {len(a['bones']):3d} bones"
                          f"  priority {a['priority']}")
            return
        out = export(assets, root, set(args.clips))
        with open(args.output, "w") as f:
            json.dump(out, f)
        for e in out["entities"]:
            subs = e["submeshes"]
            print(f"{e['entity']} model {e['model']}: {len(e['skeleton'])} bones, {len(subs)} submeshes, "
                  f"{sum(len(m['positions']) for m in subs)} vertices, {sum(len(m['triangles']) for m in subs)} "
                  f"triangles, {len(e['clips'])} clips"
                  + (f", on bone {e['parent_bone']:X} of entity {e['parent']}" if e["parent"] >= 0 else ""),
                  file=sys.stderr)
        print(f"-> {args.output}", file=sys.stderr)
    finally:
        assets.close()


if __name__ == "__main__":
    main()
