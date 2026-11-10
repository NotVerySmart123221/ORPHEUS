import struct
import unittest

from anim_export import parse_animation, parse_mesh, parse_skeleton, unpack_rotation
from animation_aliases import shared_pairs, unambiguous_bindings


def render_mesh(lods):
    """An MRNM with one 3-vertex buffer (position, blend indices, blend weights; stride 20), one index buffer
    holding the triangle (2, 0, 1), and one submesh per LOD in `lods` drawing it with bone offset 1."""
    blob = bytearray(292 + 96 * len(lods))
    struct.pack_into("<I", blob, 0, 3)
    blob[4], blob[10], blob[11] = len(lods), 1, 1
    struct.pack_into("<3q", blob, 112, 144, 268, 292)
    struct.pack_into("<IIBBBBIqqq", blob, 144, 3, 0, 20, 0, 3, 0, 0, 184, 208, 0)
    for j, (use, fmt, offset) in enumerate(((0, 0x2, 0), (4, 0x6, 12), (5, 0x8, 16))):
        struct.pack_into("<BBBBHH", blob, 184 + 8 * j, use, 0, fmt, 0, 0, offset)
    for v in range(3):
        struct.pack_into("<3f4B4B", blob, 208 + 20 * v, v, 0, 0, v % 2, 0, 0, 0, 255, 0, 0, 0)
    struct.pack_into("<IIq", blob, 268, 3, 0, 284)
    struct.pack_into("<3H", blob, 284, 2, 0, 1)
    for s, lod in enumerate(lods):
        base = 292 + 96 * s
        struct.pack_into("<HHHB", blob, base + 74, 3, 3, 1, 0)
        blob[base + 89], blob[base + 90] = 0x20 * s, s
        struct.pack_into("<b", blob, base + 91, lod)
    return bytes(blob)


def packed_rotation(q, axis):
    """Inverse of unpack_rotation for a unit quaternion whose dropped component (at axis) is positive."""
    x, y, z = [v for i, v in enumerate(q) if i != axis]
    a = round(x * 0x8000 / 1.41421 + 0x4000) | (axis & 1) << 15
    b = round(y * 0x8000 / 1.41421 + 0x4000) | (axis >> 1) << 15
    c = round(z * 0x10000 / 1.41421 + 0x8000)
    return a, b, c


class AnimExportTests(unittest.TestCase):
    def test_alias_pairs_require_unambiguous_bindings_on_both_entities(self):
        parent = unambiguous_bindings([(1, 10), (2, 20), (3, 30), (3, 31), (4, 40)])
        child = unambiguous_bindings([(1, 100), (2, 200), (3, 300), (4, 200)])
        self.assertEqual(shared_pairs(parent, child), {10: 100})

    def test_rotation_puts_the_dropped_component_at_its_axis(self):
        q = (0.5, -0.5, 0.5, 0.5)
        for axis in range(4):
            q_axis = tuple(q[i] if i != axis else abs(q[i]) for i in range(4))
            got = unpack_rotation(*packed_rotation(q_axis, axis))
            for g, w in zip(got, q_axis):
                self.assertAlmostEqual(g, w, places=3)

    def test_one_bone_clip(self):
        # 2 frames at 10 fps (0.1 s); bone 7 has a position key on frame 1 and an identity rotation on frame 0
        blob = bytearray(84 + 52 + 6)
        struct.pack_into("<hH", blob, 0, 160, 1)
        struct.pack_into("<f4xf", blob, 16, 0.1, 10.0)
        struct.pack_into("<q", blob, 48, 80)
        struct.pack_into("<q", blob, 72, 84)
        struct.pack_into("<i", blob, 80, 7)
        table = 84  # data offsets are in 4-byte words from the table: indices at +32/+36, data at +40/+52
        struct.pack_into("<3H2x6i", blob, table, 0, 1, 1, 0, 8, 9, 0, 10, 13)
        struct.pack_into("<B", blob, table + 32, 1)
        struct.pack_into("<B", blob, table + 36, 0)
        struct.pack_into("<3f", blob, table + 40, 1.0, 2.0, 3.0)
        struct.pack_into("<3H", blob, table + 52, *packed_rotation((0, 0, 0, 1), 3))
        clip = parse_animation(bytes(blob))
        self.assertEqual(clip["frames"], 2)
        bone = clip["bones"][7]
        self.assertEqual(bone["pos"], {1: (1.0, 2.0, 3.0)})
        self.assertEqual(bone["scale"], {})
        self.assertAlmostEqual(bone["rot"][0][3], 1.0, places=3)

    def test_skeleton_reads_local_bind_pose(self):
        mskl = bytearray(96 + 12 + 96 + 8)
        struct.pack_into("<q", mskl, 0, 96)  # hierarchy: 4 bytes skipped, then the parent
        struct.pack_into("<q", mskl, 32, 108)
        struct.pack_into("<q", mskl, 48, 204)
        struct.pack_into("<H", mskl, 92, 2)
        struct.pack_into("<4xh4xh", mskl, 96, -1, 0)
        struct.pack_into("<12f", mskl, 156, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0.5, 0, 0)
        struct.pack_into("<2I", mskl, 204, 1, 0x3BD)
        root, child = parse_skeleton(bytes(mskl))
        self.assertEqual((root["parent"], child["parent"], child["id"]), (-1, 0, 0x3BD))
        self.assertEqual(child["pos"], (0.0, 0.5, 0.0))
        self.assertEqual(child["rot"], (0.0, 0.0, 0.0, 1.0))


    def test_mesh_keeps_lod_1_and_all_lod_submeshes_with_remapped_bones(self):
        subs = parse_mesh(render_mesh((1, 2, -1)), remap=(10, 11, 12))
        self.assertEqual([(s["index"], s["flags"], s["material"]) for s in subs], [(0, 0, 0), (2, 0x40, 2)])
        sub = subs[0]
        # vertices in first-use order of the index buffer (2, 0, 1); bone index (v % 2) + offset 1, through remap
        self.assertEqual(sub["triangles"], [[0, 1, 2]])
        self.assertEqual([p[0] for p in sub["positions"]], [2.0, 0.0, 1.0])
        self.assertEqual(sub["weights"], [[[11, 1.0]], [[11, 1.0]], [[12, 1.0]]])


if __name__ == "__main__":
    unittest.main()
