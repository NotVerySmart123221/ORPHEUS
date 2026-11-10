import struct
import tempfile
import unittest
from pathlib import Path

from map_collision import MAGIC, parse_chunk, write_mesh


CUBE = [(x, y, z) for x in (0, 1) for y in (0, 1) for z in (0, 1)]  # vertex 4x + 2y + z
CUBE_FACES = [(0, 1, 3, 2), (4, 6, 7, 5), (0, 4, 5, 1), (2, 3, 7, 6), (0, 2, 6, 4), (1, 5, 7, 3)]


def chunk(vertices, triangles, hulls=()):
    """A type-0x11 chunk in the build-104319 layout, with an empty node block. hulls: (vertices, face loops, mask)
    each; their half-edge twins and planes are left 0 (unread)."""
    vert_off = 0x30
    node_off = vert_off + 16 * len(vertices)
    tri_off = node_off  # no nodes
    tri_end = tri_off + 32 * len(triangles)
    body = b''.join(struct.pack('<4f', *v, 0) for v in vertices)
    body += b''.join(struct.pack('<8I', a, b, c, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 2, mask)
                     for a, b, c, mask in triangles)
    records, data = b'', b''
    at = tri_end + 88 * len(hulls)
    for points, faces, mask in hulls:
        edges, first = [], []
        for face, loop in enumerate(faces):
            first.append(len(edges))
            edges += [(0, v, face, first[-1] + (k + 1) % len(loop)) for k, v in enumerate(loop)]
        edge_off = at + len(data) + 12 * len(points)
        face_off = edge_off + 4 * len(edges)
        records += struct.pack('<4Q3f3I5f3I', at + len(data), 0, edge_off, face_off, 0, 0, 0,
                               len(points), len(faces), len(edges), 0, 0, 0.6, 0, 0.2, mask, 1, 0)
        data += b''.join(struct.pack('<3f', *p) for p in points)
        data += b''.join(struct.pack('<bBBB', *e) for e in edges) + bytes(first)
    head = bytearray(0x30)
    struct.pack_into('<II', head, 0x08, len(hulls), 1)
    struct.pack_into('<QQ', head, 0x20, tri_end, tri_end + len(records) + len(data))
    return (bytes(head) + body + records + data
            + struct.pack('<QQQIII', node_off, vert_off, tri_off, 0, len(vertices), len(triangles)))


class MapCollisionTests(unittest.TestCase):
    def test_parse_reads_vertices_triangles_and_masks(self):
        blob = chunk([(0, 0, 0), (1, 0, 0), (0, 0, 1)], [(0, 1, 2, 0x1FDF)])
        vertices, indices, masks = parse_chunk(blob)
        self.assertEqual(vertices, [0, 0, 0, 1, 0, 0, 0, 0, 1])
        self.assertEqual((indices, masks), ([0, 1, 2], [0x1FDF]))

    def test_triangles_stop_at_the_footer_count(self):
        blob = bytearray(chunk([(0, 0, 0), (1, 0, 0), (0, 0, 1)], [(0, 1, 2, 7), (9, 9, 9, 8)]))
        struct.pack_into('<I', blob, len(blob) - 4, 1)  # the second record belongs to another mesh (Paris)
        self.assertEqual(parse_chunk(bytes(blob))[1:], ([0, 1, 2], [7]))

    def test_parse_rejects_bad_indices_and_counts(self):
        with self.assertRaises(ValueError):
            parse_chunk(chunk([(0, 0, 0)], [(0, 1, 2, 0)]))
        bad = bytearray(chunk([(0, 0, 0)], []))
        struct.pack_into('<I', bad, len(bad) - 8, 2)  # vertex count disagrees with the block
        with self.assertRaises(ValueError):
            parse_chunk(bytes(bad))

    def test_hulls_follow_the_mesh_as_triangulated_faces(self):
        blob = chunk([(0, 0, 0), (1, 0, 0), (0, 0, 1)], [(0, 1, 2, 7)], [(CUBE, CUBE_FACES, 0x21FDF)])
        vertices, indices, masks = parse_chunk(blob)
        self.assertEqual(vertices[9:], [k for p in CUBE for k in p])
        self.assertEqual(masks, [7] + [0x21FDF] * 12)  # six quads, two triangles each
        self.assertEqual(indices[:9], [0, 1, 2, 3, 4, 6, 3, 6, 5])  # the first face, offset past the mesh's vertices
        faces = {frozenset(indices[k:k + 3]) for k in range(3, len(indices), 3)}
        self.assertEqual(len(faces), 12)
        self.assertTrue(all(min(face) >= 3 for face in faces))

    def test_hull_rejects_open_loops_and_bad_vertices(self):
        good = chunk([], [], [(CUBE, CUBE_FACES, 1)])
        edge_off = struct.unpack_from('<Q', good, struct.unpack_from('<Q', good, 0x20)[0] + 0x10)[0]
        # edge 1 loops on itself; edge 0 points past the half-edges; edge 0 starts at vertex 8 of 8
        for at, value in ((edge_off + 7, 1), (edge_off + 3, 200), (edge_off + 1, 8)):
            bad = bytearray(good)
            bad[at] = value
            with self.subTest(at=at - edge_off, value=value), self.assertRaises(ValueError):
                parse_chunk(bytes(bad))

    def test_write_mesh_layout(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'm.collision'
            write_mesh(path, [0, 1, 2, 3, 4, 5, 6, 7, 8], [0, 1, 2], [7])
            data = path.read_bytes()
        self.assertEqual(data[:6], MAGIC)
        self.assertEqual(struct.unpack_from('<II', data, 6), (3, 1))
        self.assertEqual(len(data), 6 + 8 + 36 + 12 + 4)


if __name__ == '__main__':
    unittest.main()
