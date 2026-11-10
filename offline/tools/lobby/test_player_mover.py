import gzip
import math
import unicodedata
import unittest
from pathlib import Path

import practice  # noqa: F401  (puts vendor/ on the path)
from ow174.game import collision as movement
from ow174.game.commands import CROUCH, JUMP, Command
from ow174.game.content import PRACTICE_RANGE, heroes
from ow174.game.mover import mover_data
from player_mover import MECH, PlayerMover, body_values

import collision
import owner_correction
from kits import BASTION

DT = 0.007  # the client's command frame
RECORDINGS = Path(__file__).resolve().parents[2] / 'research' / 'lizrollin-runtime'
WALK = RECORDINGS / 'range-walk-2026-11-09.rec.gz'
DVA_WALK = RECORDINGS / 'range-observe-dva-2026-11-09.rec.gz'  # Soldier: 76's walk, then D.Va's Boosters
BOOST = RECORDINGS / 'range-boost-dva-2026-11-09.rec.gz'  # D.Va: tools/research/mover_walk.py --course boost


def box(lo, hi):
    """A closed box's triangles (9 floats each), wound to face out."""
    tris = []
    for k in range(3):
        a, b = (i for i in range(3) if i != k)
        for side, out in ((lo[k], -1), (hi[k], 1)):
            corners = []
            for u, v in ((lo[a], lo[b]), (hi[a], lo[b]), (hi[a], hi[b]), (lo[a], hi[b])):
                p = [0.0] * 3
                p[k], p[a], p[b] = side, u, v
                corners.append(p)
            for tri in ((corners[0], corners[1], corners[2]), (corners[0], corners[2], corners[3])):
                e1, e2 = ([t - s for t, s in zip(c, tri[0])] for c in tri[1:])
                if (e1[(k + 1) % 3] * e2[(k + 2) % 3] - e1[(k + 2) % 3] * e2[(k + 1) % 3]) * out < 0:
                    tri = (tri[0], tri[2], tri[1])
                tris.append(tuple(x for p in tri for x in p))
    return tris


def room():
    """A 20 x 20 m floor at y=0, a 3 m wall from x=5 and a 0.3 m step from z=4 (for x < 4)."""
    return movement.World(box((-10, -1, -10), (10, 0, 10)) + box((5, 0, -10), (6, 3, 10)) + box((-10, 0, 4), (4, 0.3, 10)))


def walk(path, first=0):
    """A tools/research/mover_record.py recording from frame `first`: the commands (forward right yaw pitch
    buttons seconds, and the live mover's Sprint scale) and the client's feet, by frame."""
    commands, client = {}, {}
    with gzip.open(path, 'rt', encoding='utf-8') as lines:
        for line in lines:
            kind, frame, *v = line.split()
            if int(frame) < first:
                continue
            if kind == 'S':
                commands[int(frame)] = tuple(int(x) for x in v[:5]) + (float(v[5]), float(v[9]) if len(v) > 9 else 1.0)
            elif kind == 'C':
                client[int(frame)] = tuple(float(x) for x in v[:3])
    return commands, client


def replay(commands, client, body):
    """A recording replayed from its start, or from 40 frames before its first input: the frames replayed, and the
    most the mover's feet were off the client's flat and up, and how far flat at the end."""
    first = min(f for f, c in commands.items() if c[0] or c[1] or c[4])
    start = max((f for f in client if f <= first - 40), default=min(client))
    mover = PlayerMover(client[start], commands[start][2], collision.movement_world(PRACTICE_RANGE), body)
    flat = up = 0.0
    for frame in sorted(f for f in commands if f > start):
        forward, right, yaw, pitch, buttons, seconds, mover.speed_scale = commands[frame]  # Sprint: as match.py sets it
        mover.step(Command(frame, forward, right, yaw, pitch, buttons), seconds)
        if frame in client:
            feet = client[frame]
            flat = max(flat, math.hypot(mover.position[0] - feet[0], mover.position[2] - feet[2]))
            up = max(up, abs(mover.position[1] - feet[1]))
    return frame - start, flat, up, math.hypot(mover.position[0] - feet[0], mover.position[2] - feet[2])


def run(mover, seconds, start=1, **command):
    """Step consecutive command frames from `start`; the frame after the last."""
    end = start + round(seconds / DT)
    for frame in range(start, end):
        mover.step(Command(frame, **command), DT)
    return end


class PlayerMoverTests(unittest.TestCase):
    def test_without_a_world_it_walks_flat_and_the_floor_follows_a_teleport(self):
        mover = PlayerMover([0.0, 1.0, 0.0], 0)
        end = run(mover, 1, forward=127)
        self.assertEqual(mover.position[1], 1.0)
        self.assertTrue(5.3 < mover.position[2] < 5.5)  # 5.5 m/s, after its acceleration
        mover.position = [0.0, 0.0, 0.0]  # client_sync, on a map without its collision
        run(mover, 1, end, forward=127)
        self.assertEqual(mover.position[1], 0.0)
        self.assertTrue(mover.grounded)

    def test_a_wall_stops_it_at_its_radius(self):
        mover = PlayerMover([0.0, 0.0, 0.0], 0, room())
        run(mover, 2, forward=127, yaw=16384)  # yaw +90 degrees: forward is +x
        self.assertAlmostEqual(mover.position[0], 5 - mover.values.stand_radius, places=2)
        self.assertEqual(mover.position[1], 0.0)

    def test_it_steps_up_a_low_ledge_and_falls_off_a_high_one(self):
        mover = PlayerMover([0.0, 0.0, 0.0], 0, room())
        run(mover, 1.5, forward=127)  # +z, onto the step
        self.assertAlmostEqual(mover.position[1], 0.3, places=3)
        self.assertTrue(mover.grounded)
        high = PlayerMover([0.0, 2.0, -5.0], 0, room())
        run(high, 1)
        self.assertEqual((high.position[1], high.grounded), (0.0, True))

    def test_a_jump_rises_about_a_metre_and_lands(self):
        mover, peak = PlayerMover([0.0, 0.0, -5.0], 0, room()), 0.0
        for frame in range(1, 200):
            mover.step(Command(frame, buttons=JUMP if frame < 3 else 0), DT)
            peak = max(peak, mover.position[1])
        self.assertTrue(0.9 < peak < 1.2)
        self.assertEqual((mover.position[1], mover.grounded), (0.0, True))

    def test_a_launch_flies_its_arc_and_stops_on_landing(self):
        mover = PlayerMover([0.0, 1.0, -2.0], 0, room())  # D.Va's eject: target.EJECT_RISE, EJECT_LAUNCH back along -z
        mover.launch((0.0, 5.0, -6.0))
        self.assertFalse(mover.grounded)
        end = run(mover, 1)
        self.assertEqual((mover.position[1], mover.grounded), (0.0, True))
        self.assertTrue(-7.0 < mover.position[2] < -6.0)  # about 0.75 s at 6 m/s
        run(mover, 0.5, end)
        self.assertEqual(mover.velocity, (0.0, 0.0, 0.0))

    def test_the_mech_has_its_own_capsule(self):
        self.assertEqual((body_values(MECH).stand_radius, body_values(MECH).run_forward), (0.7, 5.5))
        self.assertIs(body_values(None), mover_data(None))

    def test_boosters_push_forward_at_their_speed_into_a_wall(self):
        mover = PlayerMover([0.0, 0.0, 0.0], 0, room(), MECH)
        mover.flight = 11.99
        end = run(mover, 0.3, yaw=16384)  # no keys held: Boosters drive forward (+x)
        self.assertAlmostEqual(math.hypot(mover.velocity[0], mover.velocity[2]), 5.5 * 2.18, places=2)
        self.assertTrue(mover.airborne)
        run(mover, 1, end, yaw=16384)
        self.assertAlmostEqual(mover.position[0], 5 - 0.7, delta=0.01)  # the boost's 0.7 m capsule, less a skin
        self.assertEqual(mover.position[1], 0.0)

    def test_boosters_fly_level_off_a_ledge_and_fall_when_they_end(self):
        mover = PlayerMover([0.0, 0.3, 6.0], 32768, room(), MECH)  # on the step, facing -z
        mover.flight = 11.99
        end = run(mover, 0.4, yaw=32768)
        self.assertLess(mover.position[2], 4 - 0.7)  # past the step's edge
        self.assertAlmostEqual(mover.position[1], 0.3, places=3)  # no gravity while they run (0.3 on the 1/1024 grid)
        mover.flight = 0.0
        run(mover, 1, end, yaw=32768)
        self.assertAlmostEqual(mover.position[1], 0.0, places=2)  # landed; the spring still settles by 1 mm
        self.assertTrue(mover.grounded)
        self.assertIs(mover.data, body_values(MECH))  # the mech's own values again

    def test_boosters_fly_along_the_aim(self):
        up = PlayerMover([0.0, 0.0, -5.0], 0, room(), MECH)
        up.flight = 11.99
        run(up, 0.3, pitch=-7209)  # 39.6 degrees up
        vx, vy, vz = up.velocity
        self.assertAlmostEqual(math.hypot(vx, vy, vz), 5.5 * 2.18, places=2)
        self.assertAlmostEqual(math.degrees(math.atan2(vy, vz)), 7209 * 360 / 65536, places=2)
        down = PlayerMover([0.0, 0.0, -5.0], 0, room(), MECH)
        down.flight = 11.99
        run(down, 0.3, pitch=7209)
        # along the floor at the client's 11.58-11.62 m/s (research/lizrollin-runtime.md section 8), its capsule on
        # the floor: the feet drop by the walking lift
        self.assertAlmostEqual(math.hypot(down.velocity[0], down.velocity[2]), 11.6, places=2)
        self.assertEqual(down.velocity[1], 0.0)
        self.assertAlmostEqual(down.position[1], -0.5, delta=0.01)

    @unittest.skipUnless(movement.cache_version(movement.cache_path(PRACTICE_RANGE.map_guid)) == movement.VERSION,
                         "needs the Range's mover cache (run.py builds it from the game)")
    def test_recorded_boosters_flights_stay_near_the_client(self):
        # Boosters pressed at these frames (the lobby log's "ability 0x08"); D.Va from frame 22332 of DVA_WALK.
        # BOOST's are straight, steered, with W held, into a wall, aimed up and aimed down.
        for path, first, flights in (
                (DVA_WALK, 22340, ((27818, 28104), (28909, 29195), (30044, 30330), (31151, 31437))),
                (BOOST, 0, ((8040, 8326), (9130, 9416), (10266, 10552), (11372, 11658), (12574, 12860),
                            (13635, 13921)))):
            with self.subTest(path=path.name):
                commands, client = walk(path, first)
                start = min(client)
                mover = PlayerMover(client[start], commands[start][2], collision.movement_world(PRACTICE_RANGE),
                                    MECH, 0)
                most = 0.0
                for frame in sorted(f for f in commands if f > start):
                    forward, right, yaw, pitch, buttons, seconds, _ = commands[frame]
                    mover.step(Command(frame, forward, right, yaw, pitch, buttons), seconds)
                    mover.flight = 11.99 if any(a <= frame < b for a, b in flights) else 0.0  # match.py: after the step
                    if frame in client:
                        most = max(most, math.dist(mover.position, client[frame]))
                # measured 0.58 and 0.85 m, from the flights' ends +-1 frame; a level flight was 21.6 m off aimed up
                self.assertLess(most, 1.0)

    def test_backpedal_and_crouch_reach_their_own_speeds(self):
        for command, speed in (({'forward': -127}, 'run_backward'), ({'right': 127}, 'run_strafe'),
                               ({'forward': -127, 'buttons': CROUCH}, 'crouch_backward')):
            with self.subTest(speed=speed):
                mover = PlayerMover([0.0, 0.0, -5.0], 0, room())
                run(mover, 1, **command)
                self.assertAlmostEqual(math.hypot(mover.velocity[0], mover.velocity[2]),
                                       getattr(mover.values, speed), places=2)

    def test_sprint_acts_from_the_frame_after_it_starts(self):
        mover = PlayerMover([0.0, 0.0, -8.0], 0, room())
        end = run(mover, 0.5, forward=127)
        top = mover.values.run_forward
        mover.speed_scale = 1.5  # match.py, when Sprint starts in this frame
        mover.step(Command(end, forward=127), DT)
        self.assertEqual(mover.velocity[2], top)
        mover.step(Command(end + 1, forward=127), DT)
        self.assertGreater(mover.velocity[2], top)
        end = run(mover, 0.5, end + 2, forward=127)
        self.assertAlmostEqual(mover.velocity[2], top * 1.5, places=2)
        mover.speed_scale = 1.0
        run(mover, 0.5, end, forward=127)
        self.assertAlmostEqual(mover.velocity[2], top, places=2)

    @unittest.skipUnless(movement.cache_version(movement.cache_path(PRACTICE_RANGE.map_guid)) == movement.VERSION,
                         "needs the Range's mover cache (run.py builds it from the game)")
    def test_recorded_range_walks_replay_onto_the_clients_feet(self):
        # the same course (tools/research/mover_walk.py) on four bodies: Soldier: 76's, Tracer's 6 m/s, Reinhardt's
        # tank capsule, Torbjorn's short one; measured at most 0.038-0.042 m flat, 0.024-0.040 m up
        # (research/lizrollin-runtime.md sections 5 and 11)
        for path, body in ((WALK, 0x04000000000003CF), (RECORDINGS / 'range-walk-tracer-2026-11-09.rec.gz', 0x0400000000000001),
                           (RECORDINGS / 'range-walk-reinhardt-2026-11-09.rec.gz', 0x0400000000000024),
                           (RECORDINGS / 'range-walk-torbjorn-2026-11-09.rec.gz', 0x04000000000028B6)):
            with self.subTest(path=path.name):
                frames, flat, up, end = replay(*walk(path), body)  # as tools/research/mover_replay.py does
                self.assertGreater(frames, 2500)  # the course: about 20 s of 7 ms frames
                self.assertLess(end, 0.005)
                self.assertLess(flat, 0.05)
                self.assertLess(up, 0.05)

    @unittest.skipUnless(movement.cache_version(movement.cache_path(PRACTICE_RANGE.map_guid)) == movement.VERSION,
                         "needs the Range's mover cache (run.py builds it from the game)")
    def test_every_hero_that_gets_owner_corrections_replays_its_recorded_abilities(self):
        # mover_walk.py --course ability on each hero's kits body, no records but Soldier: 76's: Shift, E, a held jump,
        # both fires, Q, a crouch walk, a jump from a crouch, strafes. The client runs no ability's movement there,
        # so the mover alone (with Soldier: 76's Sprint) stays on its feet: at most 0.042 m flat, 0.041 m up.
        # Bastion's is --course sentry with records on: 0.061 m for the 2 frames before his client has the no-move
        # state (research/lizrollin-runtime.md section 21)
        for hero in heroes().values():
            if hero.guid in owner_correction.OFF:
                continue
            with self.subTest(hero=hero.name):
                name = ''.join(c for c in unicodedata.normalize('NFKD', hero.name).lower() if c.isascii() and c.isalnum())
                frames, flat, up, end = replay(*walk(RECORDINGS / 'abilities' / f'{name}.rec.gz'), hero.body)
                sentry = hero.guid == BASTION
                self.assertGreater(frames, 2000 if sentry else 6000)  # the course: 43 s of 7 ms frames (sentry: 16 s)
                self.assertLess(end, 0.005)
                self.assertLess(flat, 0.07 if sentry else 0.05)
                self.assertLess(up, 0.05)


if __name__ == '__main__':
    unittest.main()
