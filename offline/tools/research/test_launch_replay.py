"""The server's launches against the client's own: recordings of the client predicting Jump Jet, Jump Pack and Combat
Roll on a VM body with no owner corrections (research/lizrollin-runtime.md section 25), replayed through the match
code on kits bodies."""
import logging
import unittest
from pathlib import Path

import launch_replay
from ow174.game.content import heroes

RECORDINGS = Path(__file__).resolve().parents[2] / 'research' / 'lizrollin-runtime'
SHIFT, JUMP = launch_replay.BUTTONS['shift'], launch_replay.BUTTONS['jump']


class LaunchReplayTests(unittest.TestCase):
    def setUp(self):
        logging.disable(logging.CRITICAL)
        self.addCleanup(logging.disable, logging.NOTSET)

    def apart(self, name, recording, frames, button=SHIFT):
        """{press frame: the most the server's feet were off the client's in `frames` frames after it}."""
        hero = next(h for h in heroes().values() if h.name == name)
        return {press: most for press, _, most, *_ in launch_replay.compare(RECORDINGS / recording, hero, button, frames)}

    def test_hover_jets_rise_as_the_clients_do(self):
        # jump held 2.5 s from the ground (up 8 m, out of fuel, down), then a Jump Jet with the jets held on its fall
        recording = 'vm-hover-jets-2026-11-10.rec.gz'
        self.assertLess(max(self.apart('Pharah', recording, 340, JUMP).values()), 0.1)
        self.assertLess(max(self.apart('Pharah', recording, 220).values()), 0.12)

    def test_flight_stops_where_the_clients_does(self):
        # standing, no input: 28 m/s along the ground to a stop 7.80 m on (the second press is on its cooldown)
        apart = self.apart('Echo', 'vm-flight-2026-11-10.rec.gz', 440)
        self.assertLess(min(apart.values()), 0.05)

    def test_flight_is_the_clients_with_a_key_held(self):
        # W for its 3 s (28.7 m); W aimed 30 degrees up, into a wall and on up along it; jump held: up to the ceiling
        level, wall, up = self.apart('Echo', 'vm-flight-held-2026-11-10.rec.gz', 440).values()
        self.assertLess(level, 0.15)
        self.assertLess(wall, 1.3)
        self.assertLess(up, 0.1)

    def test_flights_launch_goes_the_inputs_way(self):
        # no input aimed 30 degrees up (the client's speed limit cut off a frame early there: 0.46 m); to the right;
        # jump let go in the press's own frame, which the client still took as held (not replayed); backward
        up, right, _, back = self.apart('Echo', 'vm-flight-launch-2026-11-10.rec.gz', 440).values()
        self.assertLess(up, 0.5)
        self.assertLess(right, 0.25)
        self.assertLess(back, 0.35)

    def test_coach_gun_kicks_as_the_clients_does(self):
        # level, 45 and 85 degrees down (beside a wall), 44 degrees up; the fifth press starts in a fall, not replayed
        apart = list(self.apart('Ashe', 'vm-coach-gun-2026-11-10.rec.gz', 200).values())
        self.assertEqual(len(apart), 5)
        self.assertLess(max(apart[0], apart[3]), 0.03)
        self.assertLess(max(apart[1:3]), 0.15)

    def test_rising_uppercut_rises_as_the_clients_does(self):
        # standing: 0.03 m over 2.3 s. At a run the same to 1.1 s, then both slide down a wall
        standing, running = self.apart('Doomfist', 'vm-rising-uppercut-2026-11-10.rec.gz', 160).values()
        self.assertLess(standing, 0.04)
        self.assertLess(running, 0.15)

    def test_jump_jet_flies_the_clients_arc(self):
        # standing and at a run, 1.75 s each: up to the Range's ceiling and down again
        apart = self.apart('Pharah', 'vm-jump-jet-2026-11-10.rec.gz', 250)
        self.assertEqual(len(apart), 2)
        self.assertLess(max(apart.values()), 0.15)

    def test_combat_roll_ends_where_the_clients_does(self):
        # standing, forward, and to the left from a slope (its first frames differ by 0.11 m)
        apart = self.apart('Cassidy', 'vm-combat-roll-2026-11-10.rec.gz', 100)
        self.assertEqual(len(apart), 3)
        self.assertLess(sorted(apart.values())[1], 0.02)
        self.assertLess(max(apart.values()), 0.15)

    def test_jump_pack_leaves_at_the_clients_speed_and_angle(self):
        # looking up 39.6 degrees, and level at a run (the press between them starts from a slide, not replayed well).
        # The first frames only for the first: on the VM body its leap lost its rise 0.2 s in, when the server's copy,
        # still on the ground, ended the ability.
        recording = 'vm-jump-pack-2026-11-10.rec.gz'
        up, _, run = sorted(self.apart('Winston', recording, 25))
        self.assertLess(self.apart('Winston', recording, 25)[up], 0.02)
        self.assertLess(self.apart('Winston', recording, 100)[run], 0.2)


if __name__ == '__main__':
    unittest.main()
