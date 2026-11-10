import struct
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import collision
from collision import Mesh


def box_room():
    """A 20 x 20 m floor at y=0 with a 3 m wall along x=5 (facing -x) and a 0.3 m step at z>4."""
    v = [(-10, 0, -10), (10, 0, -10), (10, 0, 10), (-10, 0, 10),
         (5, 0, -10), (5, 3, -10), (5, 3, 10), (5, 0, 10),
         (-10, 0.3, 4), (4, 0.3, 4), (4, 0.3, 10), (-10, 0.3, 10)]
    quads = [(0, 1, 2, 3), (4, 5, 6, 7), (8, 9, 10, 11)]
    tris = [i for a, b, c, d in quads for i in (a, b, c, a, c, d)]
    return Mesh([k for p in v for k in p], tris)


class CollisionTests(unittest.TestCase):
    def test_ray_hits_nearest_surface_and_respects_length(self):
        mesh = box_room()
        dist, tri = mesh.ray((0, 1, 0), (1, 0, 0), 50)
        self.assertAlmostEqual(dist, 5)
        self.assertAlmostEqual(abs(mesh.normal(tri)[0]), 1)
        self.assertIsNone(mesh.ray((0, 1, 0), (1, 0, 0), 4.9))
        self.assertAlmostEqual(mesh.ray((0, 1, 0), (0, -1, 0), 5)[0], 1)
        self.assertFalse(mesh.clear((0, 1, 0), (8, 1, 0)))
        self.assertTrue(mesh.clear((0, 1, 0), (4, 1, -3)))

    def test_load_reads_the_current_format_and_refuses_meshes_without_hulls(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / '0688.collision'
            body = struct.pack('<II9f3II', 3, 1, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 2, 0x80)
            path.write_bytes(collision.MAGIC + body)
            self.assertTrue(collision.current(path))
            self.assertAlmostEqual(Mesh.load(path, 0x80).ray((0.2, 1, 0.2), (0, -1, 0), 2)[0], 1)
            self.assertIsNone(Mesh.load(path, 0x1).ray((0.2, 1, 0.2), (0, -1, 0), 2))
            path.write_bytes(b'OWCOL1' + body)  # the same layout before the props' hulls were merged
            self.assertFalse(collision.current(path))
            with self.assertRaisesRegex(ValueError, 'map_collision.py'):
                Mesh.load(path)
        self.assertFalse(collision.current(Path(tmp) / 'missing.collision'))

    @unittest.skipUnless(collision.current(collision.DATA / '0688.collision'), 'needs the extracted Range mesh')
    def test_range_mesh_has_the_spawn_room_table(self):
        mesh = collision.for_map(0x0800000000000688)
        for height in (0.3, 0.6):  # the table's hull (model 0x1217) spans 0.1-0.76 m above the floor
            dist, _ = mesh.ray((54.57, 1.0 + height, 42.15), (1, 0, 0), 10)
            self.assertAlmostEqual(dist, 2.35, places=2)

    def test_range_mesh_blocks_shots_and_bot_sight(self):
        from test_target import _match
        mesh = box_room()
        match, player = _match()
        match.mesh = mesh
        match.update(3, 100)
        shooter = next(bot for bot in match.bots if bot.behaviour == 'shoot' and bot.team == 1)
        for bot in match.bots:
            bot.health = bot.health if bot is shooter else 0
        shooter.position = (8, 0, 0)  # behind the wall at x=5
        player.mover.position = [0, 0, 0]
        with patch('target.random.uniform', return_value=0.5):
            for now in (4, 5, 6, 7):
                match.update(now, now)
        self.assertEqual((match.projectiles, shooter.next_attack_at), ([], None))
        shooter.position = (3, 0, 0)  # in the open
        with patch('target.random.uniform', return_value=0.5):
            for now in (8, 9):
                match.update(now, now)
        self.assertEqual(len(match.projectiles), 1)


if __name__ == '__main__':
    unittest.main()
