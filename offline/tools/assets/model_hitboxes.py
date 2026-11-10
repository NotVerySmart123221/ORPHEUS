"""Print a model's hit volumes (research/hitboxes.md) in model space, bind pose, metres.

A model asset (GUID prefix 0x0D0) is chunked data. Its MSTU chunk holds STUModel, whose body parts
(STU_2DD96D6D) each name a skeleton bone ID, a kind (1 hit volume, 4 ragdoll body) and a shape:
STU_4614D0E5 sphere (centre, radius) or STU_4C8D9F5E capsule (two ends, radius), in the bone's frame.
A hit volume whose part info carries m_EE483487 is the critical (headshot) one. The mskl chunk gives
each bone's bind-pose matrix (4x4, row vectors, translation last) and bone ID.

  python model_hitboxes.py 0x0D0000000000312E
"""
import os
import argparse
import math
import struct

from retail_bundle_lookup import find_asset_payload, load_storage
from stu_graph import instances

PART, SPHERE, CAPSULE = 0x2DD96D6D, 0x4614D0E5, 0x4C8D9F5E
# shortcut: STU_B3800E70 (Bastion's convex hulls; vertices m_88FCECD7, planes m_0674FD43) is read as the sphere of
# its volume (m_volume, named in OWLib's 1.74 TankLib) at its centre m_7D1FC63E. Upgrade to a hull test if it matters.
HULL, HULL_CENTER, HULL_VOLUME = 0xB3800E70, 0x7D1FC63E, 0xB1C6CDA2
BONE, KIND, INFO, SHAPE = 0x30F925F4, 0x89AABA94, 0x9B0424EB, 0xB7C8314A
CENTER, END_A, END_B, RADIUS, CRITICAL = 0xA8D12F28, 0x10EFDE90, 0xC320C90B, 0xED61D926, 0xEE483487
HIT_VOLUME = 1


def chunks(blob):
    """{tag: data} of chunked data: a 16-byte header, then (tag, size, size, hash) and size bytes each."""
    out, pos = {}, 16
    while pos + 16 <= len(blob):
        size = struct.unpack_from("<I", blob, pos + 4)[0]
        out[blob[pos:pos + 4][::-1].decode("ascii")] = blob[pos + 16:pos + 16 + size]
        pos += 16 + size
    return out


def bones(skeleton):
    """{bone ID: bind-pose 4x4 matrix as 16 floats}."""
    matrices, ids = struct.unpack_from("<Q", skeleton, 8)[0], struct.unpack_from("<Q", skeleton, 48)[0]
    count = struct.unpack_from("<H", skeleton, 92)[0]
    return {struct.unpack_from("<I", skeleton, ids + 4 * i)[0]: struct.unpack_from("<16f", skeleton, matrices + 64 * i)
        for i in range(count)}


def transform(matrix, point):
    return tuple(sum(point[r] * matrix[4 * r + c] for r in range(3)) + matrix[12 + c] for c in range(3))


def hitboxes(model, pose=None):
    """[(a, b, radius, critical)]; optional bone matrices replace bind pose. Caller handles radius scaling."""
    parts = chunks(model)
    stu = parts["MSTU"]
    offset, _, size = struct.unpack_from("<3I", stu)
    rows = instances(stu[offset:offset + size])
    pose = bones(parts["mskl"]) if pose is None else pose
    out = []
    for cls, _, _, fields in rows:
        if cls != PART or fields[KIND][0] != HIT_VOLUME:
            continue
        matrix = pose[struct.unpack("<i", fields[BONE])[0]]
        shape_cls, _, _, shape = rows[struct.unpack("<i", fields[SHAPE])[0]]
        vec = lambda key: struct.unpack("<3f", shape[key]) if key in shape else (0.0, 0.0, 0.0)
        if shape_cls == SPHERE:
            a = b = vec(CENTER)
        elif shape_cls == CAPSULE:
            a, b = vec(END_A), vec(END_B)
        elif shape_cls == HULL:
            a = b = vec(HULL_CENTER)
        else:
            raise ValueError(f"unknown hit shape {shape_cls:08X}")
        critical = CRITICAL in rows[struct.unpack("<i", fields[INFO])[0]][3]
        if shape_cls == HULL:
            radius = (3 * struct.unpack("<f", shape[HULL_VOLUME])[0] / (4 * math.pi)) ** (1 / 3)
        else:
            radius = struct.unpack("<f", shape[RADIUS])[0]
        out.append((transform(matrix, a), transform(matrix, b), radius, critical))
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("guid", type=lambda v: int(v, 0))
    parser.add_argument("--game-install", default=os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
    args = parser.parse_args()
    storage = load_storage(args.game_install)
    try:
        model = find_asset_payload(storage, args.guid).payload
    finally:
        for handle in storage._files.values():
            handle.close()
    r = lambda v: tuple(round(x, 3) for x in v)
    for a, b, radius, critical in hitboxes(model):
        print(f"    ({r(a)}, {r(b)}, {round(radius, 3)}, {critical}),")


if __name__ == "__main__":
    main()
