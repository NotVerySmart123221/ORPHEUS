import struct
import unittest

from model_hitboxes import bones, chunks, transform


class ModelHitboxTests(unittest.TestCase):
    def test_chunks_split_tagged_blocks(self):
        blob = b"\0" * 16 + b"UTSM" + struct.pack("<III", 2, 2, 0) + b"ab" + b"lksm" + struct.pack("<III", 1, 1, 0) + b"c"
        self.assertEqual(chunks(blob), {"MSTU": b"ab", "mskl": b"c"})

    def test_bone_matrix_places_local_points(self):
        # one bone, ID 17, turned 90 degrees about Y (local +X to world +Z) at (0, 1, 0)
        matrix = (0, 0, 1, 0, 0, 1, 0, 0, -1, 0, 0, 0, 0, 1, 0, 1)
        skeleton = bytearray(96 + 64 + 4)
        struct.pack_into("<Q", skeleton, 8, 96)
        struct.pack_into("<Q", skeleton, 48, 160)
        struct.pack_into("<H", skeleton, 92, 1)
        struct.pack_into("<16f", skeleton, 96, *matrix)
        struct.pack_into("<I", skeleton, 160, 17)
        pose = bones(bytes(skeleton))
        self.assertEqual(list(pose), [17])
        self.assertEqual(transform(pose[17], (0.5, 0.2, 0)), (0, 1.2, 0.5))


if __name__ == "__main__":
    unittest.main()
