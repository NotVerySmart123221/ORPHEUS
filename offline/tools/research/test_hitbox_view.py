import math
import unittest

import hitbox_view


class ProjectionTest(unittest.TestCase):
    def test_ahead_right_up_and_behind(self):
        state = {'eye': [0.0, 1.6, 0.0], 'yaw': 0, 'pitch': 0}
        project = hitbox_view.projector(state, 1280, 720)
        x, y, scale = project((0.0, 1.6, 10.0))
        self.assertAlmostEqual((x, y), (640, 360))
        self.assertAlmostEqual(scale, 360 * hitbox_view.VERTICAL_SCALE / 10)
        self.assertGreater(project((-1.0, 1.6, 10.0))[0], 640)  # right is -X at yaw 0
        self.assertLess(project((0.0, 2.6, 10.0))[1], 360)
        self.assertIsNone(project((0.0, 1.6, -1.0)))

    def test_pitch_down_brings_the_floor_to_the_centre(self):
        pitch = round(math.radians(45) / hitbox_view.TURN)  # positive pitch looks down
        project = hitbox_view.projector({'eye': [0.0, 1.6, 0.0], 'yaw': 0, 'pitch': pitch}, 1280, 720)
        x, y, _ = project((0.0, 0.0, 1.6))
        self.assertAlmostEqual(x, 640, places=3)
        self.assertAlmostEqual(y, 360, delta=0.5)

    def test_vertical_fov_edge(self):
        project = hitbox_view.projector({'eye': [0.0, 0.0, 0.0], 'yaw': 0, 'pitch': 0}, 2560, 720, 32 / 9)
        _, y, _ = project((0.0, math.tan(math.radians(70.5328 / 2)), 1.0))
        self.assertAlmostEqual(y, 0, delta=0.05)  # 32:9: the top edge is the vertical half-FOV
        letterboxed = hitbox_view.projector({'eye': [0.0, 0.0, 0.0], 'yaw': 0, 'pitch': 0}, 1280, 720, 32 / 9)
        _, y, _ = letterboxed((0.0, math.tan(math.radians(70.5328 / 2)), 1.0))
        self.assertAlmostEqual(y, 360 - 180, delta=0.05)  # 32:9 in a 16:9 window: a 360 px tall viewport

    def test_capsule_outline(self):
        state = {'eye': [0.0, 1.0, 0.0], 'yaw': 0, 'pitch': 0,
                 'bots': [[[0.0, 1.0, 5.0, 0.0, 2.0, 5.0, 0.2, False], [0.0, 2.2, 5.0, 0.0, 2.2, 5.0, 0.3, True]]]}
        kinds = [(s[0], s[-1]) for s in hitbox_view.shapes(state, 1280, 720)]
        body, head = hitbox_view.BODY, hitbox_view.CRITICAL
        self.assertEqual(kinds, [('circle', body), ('circle', body), ('line', body), ('line', body), ('circle', head)])


if __name__ == '__main__':
    unittest.main()
