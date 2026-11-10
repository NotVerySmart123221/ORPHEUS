import math
import unittest

from combat import aim_direction, ray_capsule_distance, ray_sphere_distance


class HitscanGeometryTests(unittest.TestCase):
    def test_command_angles_match_game_axes(self):
        self.assertEqual(aim_direction(0, 0), (0.0, -0.0, 1.0))
        self.assertAlmostEqual(aim_direction(16384, 0)[0], 1.0)
        self.assertAlmostEqual(aim_direction(0, 16384)[1], -1.0)

    def test_hit_returns_nearest_distance_and_normalizes_direction(self):
        distance = ray_sphere_distance((0, 0, 0), (0, 0, 4), (0, 0, 5), 1)
        self.assertEqual(distance, 4.0)

    def test_miss_tangent_and_max_distance(self):
        self.assertIsNone(ray_sphere_distance((0, 0, 0), (0, 0, 1), (2, 0, 5), 1))
        self.assertEqual(ray_sphere_distance((0, 0, 0), (0, 0, 1), (1, 0, 5), 1), 5.0)
        self.assertIsNone(ray_sphere_distance((0, 0, 0), (0, 0, 1), (0, 0, 5), 1, 3.9))

    def test_origin_inside_sphere_is_a_zero_distance_hit(self):
        self.assertEqual(ray_sphere_distance((0, 0, 0), (1, 0, 0), (0.5, 0, 0), 1), 0.0)

    def test_invalid_inputs_are_rejected(self):
        for args in (
            ((0, 0), (0, 0, 1), (0, 0, 5), 1),
            ((0, 0, 0), (0, 0, 0), (0, 0, 5), 1),
            ((0, 0, 0), (0, 0, 1), (0, 0, 5), 0),
            ((0, 0, 0), (0, 0, 1), (0, 0, 5), 1, math.nan),
            ((0, math.inf, 0), (0, 0, 1), (0, 0, 5), 1),
            ((1e308, 0, 0), (0, 0, 1), (-1e308, 0, 0), 1),
            ((0, 0, 0), (0, 0, 1), (0, 0, 5), 1e308),
        ):
            with self.subTest(args=args), self.assertRaises(ValueError):
                ray_sphere_distance(*args)

        for yaw, pitch in ((32768, 0), (0, -32769), (1.5, 0)):
            with self.subTest(yaw=yaw, pitch=pitch), self.assertRaises(ValueError):
                aim_direction(yaw, pitch)

    def test_ray_capsule_hits_side_ends_and_inside(self):
        a, b = (0, 0, 5), (0, 2, 5)  # upright, radius 0.5
        self.assertAlmostEqual(ray_capsule_distance((0, 1, 0), (0, 0, 1), a, b, 0.5), 4.5)  # the side
        self.assertAlmostEqual(ray_capsule_distance((0, 2.3, 0), (0, 0, 1), a, b, 0.5), 5 - math.sqrt(0.16))  # top cap
        self.assertIsNone(ray_capsule_distance((0, 2.6, 0), (0, 0, 1), a, b, 0.5))  # over it
        self.assertIsNone(ray_capsule_distance((0, 1, 0), (0, 0, 1), a, b, 0.5, 4))  # out of range
        self.assertEqual(ray_capsule_distance((0, 1, 5), (1, 0, 0), a, b, 0.5), 0.0)  # inside
        self.assertAlmostEqual(ray_capsule_distance((0, -3, 5), (0, 1, 0), a, b, 0.5), 2.5)  # along the axis


if __name__ == "__main__":
    unittest.main()
