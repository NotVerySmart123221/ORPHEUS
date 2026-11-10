import math
import unittest

from collision import Mesh

import nav


def walled_floor():
    """A 20 x 20 m floor at y=0 split by a 3 m wall along x=0, open for z > 6."""
    v = [(-10, 0, -10), (10, 0, -10), (10, 0, 10), (-10, 0, 10),
         (0, 0, -10), (0, 3, -10), (0, 3, 6), (0, 0, 6)]
    quads = [(0, 1, 2, 3), (4, 5, 6, 7)]
    return Mesh([k for p in v for k in p], [i for a, b, c, d in quads for i in (a, b, c, a, c, d)])


class NavTests(unittest.TestCase):
    def test_path_goes_round_the_wall(self):
        grid = nav.build(walled_floor(), [(-8, 0, -8), (8, 0, -8)])
        route = grid.path((-5, 0, 0), (5, 0, 0))
        self.assertLess(abs(route[0][0] + 5), 1)
        self.assertLess(abs(route[-1][0] - 5), 1)
        crossings = [(a, b) for a, b in zip(route, route[1:]) if a[0] < 0 < b[0]]
        self.assertTrue(crossings)
        self.assertTrue(all(min(a[2], b[2]) > 6 for a, b in crossings))  # past the wall's end

    def test_unreachable_goal_stops_at_the_closest_node(self):
        grid = nav.build(walled_floor(), [(-8, 0, -8), (8, 0, -8)])
        route = grid.path((-5, 0, 0), (0, 50, 0))  # above the floor, nothing reaches it
        self.assertTrue(route)

    def test_region_leaves_out_a_ledge_and_paths_snap_into_it(self):
        ledge = Mesh([k for p in [(-10, 0, -10), (10, 0, -10), (10, 0, 10), (-10, 0, 10),
                                  (4, 2, -10), (10, 2, -10), (10, 2, 10), (4, 2, 10)] for k in p],
                     [i for a, b, c, d in [(0, 1, 2, 3), (4, 5, 6, 7)] for i in (a, b, c, a, c, d)])
        grid = nav.build(ledge, [(-8, 0, -8), (8, 0, -8)])
        region = grid.region((-5, 0, 0))
        self.assertTrue(region)
        self.assertTrue(all(grid.nodes[n][1] < 1 for n in region))  # the 2 m ledge can be dropped from, not climbed
        self.assertLess(grid.path((-5, 0, 0), (7, 2, 0), region)[-1][1], 1)

    def test_smooth_cuts_the_zigzag_but_not_through_the_wall(self):
        mesh = walled_floor()
        route = nav.build(mesh, [(-8, 0, -8), (8, 0, -8)]).path((-5, 0, 0), (5, 0, 0))
        smooth = nav.smooth(mesh, route)
        self.assertLess(len(smooth), len(route) // 2)
        self.assertEqual((smooth[0], smooth[-1]), (route[0], route[-1]))
        self.assertTrue(all(nav.passable(mesh, a, b) for a, b in zip(smooth, smooth[1:])))

    def test_smooth_steps_off_a_ledge_instead_of_gliding(self):
        ledge = Mesh([k for p in [(-10, 0, -10), (10, 0, -10), (10, 0, 10), (-10, 0, 10),
                                  (4, 2, -10), (10, 2, -10), (10, 2, 10), (4, 2, 10)] for k in p],
                     [i for a, b, c, d in [(0, 1, 2, 3), (4, 5, 6, 7)] for i in (a, b, c, a, c, d)])
        route = nav.smooth(ledge, nav.build(ledge, [(-8, 0, -8), (8, 0, -8)]).path((8, 2, 5), (-8, 0, -5)))
        drops = [(a, b) for a, b in zip(route, route[1:]) if abs(a[1] - b[1]) > 1]
        self.assertTrue(drops)
        self.assertTrue(all(math.dist((a[0], a[2]), (b[0], b[2])) < 1.5 for a, b in drops))

    def test_loop_goes_round_a_blocked_leg_and_starts_where_it_walks_straight_to(self):
        mesh = walled_floor()
        grid = nav.build(mesh, [(-8, 0, -8), (8, 0, -8)])
        loop = nav.loop(grid, mesh, (5, 0, -1), ((-5, 0, 0), (5, 0, 0)))
        self.assertLess(math.dist(loop[0], (5, 0, -1)), 1)  # the routed point nearest the bot
        self.assertIn((-5, 0, 0), loop)
        crossings = [a[2] + (b[2] - a[2]) * a[0] / (a[0] - b[0]) for a, b in zip(loop, loop[1:] + loop[:1]) if a[0] * b[0] < 0]
        self.assertTrue(crossings and all(z > 6 for z in crossings))  # past the wall's end both ways
        self.assertTrue(all(nav.passable(mesh, a, b) for a, b in zip(loop, loop[1:] + loop[:1])))


if __name__ == '__main__':
    unittest.main()
