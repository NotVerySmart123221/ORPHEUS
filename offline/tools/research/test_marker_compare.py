"""The server's own Shadow Step rule (Match.step_point without the client's feed) against the client's marker, as
tools/asi/client_feed.c read it in live sessions (research/lizrollin-runtime.md sections 36 to 40)."""
import math
import unittest
from pathlib import Path

import marker_compare

RECORDINGS = Path(__file__).resolve().parents[2] / 'research' / 'lizrollin-runtime'


class MarkerCompareTests(unittest.TestCase):
    def apart(self, recording):
        """(distances of the aims with a place on both sides, aims where only one side has a place, all aims)."""
        found = marker_compare.compare(RECORDINGS / f'shadow-step-marker-{recording}-2026-11-10.rec.gz')
        both = sorted(math.dist(place, ours) for _, _, _, place, good, ours in found if good and ours is not None)
        return both, sum(good != (ours is not None) for *_, good, ours in found), len(found)

    def test_outside_the_rule_is_the_markers_place(self):
        # 57 aims from the Range's yard: six headings, level to 47 degrees down
        both, one_sided, aims = self.apart('outside')
        self.assertEqual((aims, len(both), one_sided), (57, 47, 0))  # no place on a rock's ridge: its faces are too steep
        self.assertGreaterEqual(sum(d <= 0.01 for d in both), 46)  # to the centimetre (the feet's grid is 1 mm)
        self.assertLessEqual(both[-1], 0.03)  # the aim over the ramp was 18 m off before the line was let down

    def test_in_the_spawn_room_the_rule_is_the_markers_place(self):
        # 88 aims among the room's desks: eight headings, level to 59 degrees down
        both, one_sided, aims = self.apart('room')
        self.assertEqual((aims, len(both), one_sided), (88, 80, 0))  # no place on a crate the chunk flags
        self.assertGreaterEqual(sum(d <= 0.01 for d in both), 78)
        self.assertLessEqual(both[-1], 0.5)  # a level aim: a line lying flat on a table top touches it all along

    def test_pitch_by_pitch_the_pick_is_taken_from_behind_the_eye(self):
        # 98 aims in the room, six headings, level to 15 degrees down in steps of one. Seven of them choose between
        # a stool 2.4 m on and the floor at the wall 5.5 m on: from the eye the floor looks nearer the first start,
        # from a metre behind the eye the stool does, and the marker is on the stool.
        both, one_sided, aims = self.apart('pitch')
        self.assertEqual((aims, len(both), one_sided), (98, 74, 0))
        self.assertGreaterEqual(sum(d <= 0.01 for d in both), 73)
        self.assertLessEqual(both[-1], 0.5)  # the level aim over the table again

    def test_aims_over_the_horizon_find_a_place_only_on_something_high(self):
        # 46 aims, level to 48 degrees up, from the room and the yard: the cone under the aim reaches no floor
        both, one_sided, aims = self.apart('up')
        self.assertEqual((aims, len(both), one_sided), (46, 11, 0))
        self.assertLessEqual(both[-1], 0.1)  # 1.1 m off before the line was let down

    def test_a_table_edge_is_a_floor_whatever_the_line_met_it_at(self):
        # 196 aims at one wall of the room from the spawn and from 0.7 m beside it, 15 headings, level to 13 degrees
        # down. Ten stand him on the edge of a table: the line let down touches it 52 degrees off upright, and the
        # floor check goes by the table's faces.
        both, one_sided, aims = self.apart('wall')
        self.assertEqual((aims, len(both), one_sided), (196, 186, 0))
        self.assertGreaterEqual(sum(d <= 0.01 for d in both), 183)
        self.assertLessEqual(both[-1], 1.2)  # two level aims: a line lying flat on the floor

    def test_the_marker_changes_place_at_the_pitch_the_rule_does(self):
        # 143 aims at that wall from three distances, the pitch in steps of 0.1 and 0.2 degrees, up and down: where
        # the marker goes from the stool to the floor at the wall (7.6 degrees from the spawn, either way)
        both, one_sided, aims = self.apart('fine')
        self.assertEqual((aims, len(both), one_sided), (143, 143, 0))
        self.assertLessEqual(both[-1], 0.01)


if __name__ == '__main__':
    unittest.main()
