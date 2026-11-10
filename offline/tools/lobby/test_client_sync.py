import math
import socket
import unittest
from unittest import mock

import client_sync
import practice  # noqa: F401  (puts vendor/ on the path)
from ow174.game.commands import Command
from ow174.game.mover import Mover


class ResyncTest(unittest.TestCase):
    def test_nothing_moves_without_a_common_frame(self):
        resync = client_sync.Resync()
        resync.stepped(10, (1.0, 0.0, 1.0))
        resync.observed(11, (5.0, 0.0, 5.0))
        position = [1.0, 0.0, 1.0]
        self.assertIsNone(resync.correct(position))
        self.assertEqual(position, [1.0, 0.0, 1.0])

    def test_offset_at_the_newest_common_frame_moves_the_copy_now(self):
        resync = client_sync.Resync()
        for frame, x in ((10, 1.0), (11, 1.1), (12, 1.2)):
            resync.stepped(frame, (x, 0.0, 0.0))
        resync.observed(10, (1.3, 0.0, 0.0))
        resync.observed(11, (1.5, 0.5, 0.0))  # the newest frame both know
        resync.observed(13, (9.0, 0.0, 0.0))  # the client is ahead: not stepped yet
        position = [1.2, 0.0, 0.0]  # after frame 12
        delta = resync.correct(position)
        self.assertEqual([round(d, 6) for d in delta], [0.4, 0.5, 0.0])
        self.assertEqual([round(p, 6) for p in position], [1.6, 0.5, 0.0])
        self.assertEqual(sorted(resync.server), [12])  # used-up frames dropped, the later one moved along
        self.assertAlmostEqual(resync.server[12][0], 1.6)
        self.assertEqual(sorted(resync.client), [13])

    def test_the_previous_bodys_slot_is_ignored_unless_close(self):
        resync = client_sync.Resync(stale=0xA0)
        resync.stepped(20, (0.0, 0.0, 0.0))  # the new body, at the spawn
        resync.observed(20, (4.0, 0.0, 2.0), 0xA0)  # the old body, still simulated for a frame
        position = [0.0, 0.0, 0.0]
        self.assertIsNone(resync.correct(position))
        self.assertEqual(position, [0.0, 0.0, 0.0])
        self.assertIsNone(resync.slot)
        resync.stepped(21, (0.1, 0.0, 0.0))
        resync.observed(21, (0.12, 0.0, 0.0), 0xA0)  # the slot reused for the new body: it agrees
        position = [0.1, 0.0, 0.0]
        self.assertIsNotNone(resync.correct(position))
        resync.stepped(22, (0.2, 0.0, 0.0))
        resync.observed(22, (3.0, 0.0, 0.0), 0xB0)  # any other slot is the new body, however far
        position = [0.2, 0.0, 0.0]
        self.assertAlmostEqual(resync.correct(position)[0], 2.8)
        self.assertEqual(resync.slot, 0xB0)

    def test_a_used_frame_does_not_correct_twice(self):
        resync = client_sync.Resync()
        resync.stepped(5, (0.0, 0.0, 0.0))
        resync.observed(5, (1.0, 0.0, 0.0))
        position = [0.0, 0.0, 0.0]
        resync.correct(position)
        resync.observed(5, (1.0, 0.0, 0.0))  # a late duplicate
        self.assertIsNone(resync.correct(position))
        self.assertEqual(position, [1.0, 0.0, 0.0])

    def test_history_is_bounded(self):
        resync = client_sync.Resync()
        for frame in range(client_sync.KEEP + 50):
            resync.stepped(frame, (0.0, 0.0, 0.0))
            resync.observed(frame + 1000, (0.0, 0.0, 0.0))
        self.assertEqual(len(resync.server), client_sync.KEEP)
        self.assertEqual(min(resync.server), 50)
        self.assertEqual(len(resync.client), client_sync.KEEP)


class ReceiveTest(unittest.TestCase):
    def test_only_frames_with_finite_feet_are_kept(self):
        a, b = socket.socket(socket.AF_INET, socket.SOCK_DGRAM), socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        a.bind(('127.0.0.1', 0))
        a.setblocking(False)
        try:
            for data in (client_sync.MESSAGE.pack(7, 1.0, 2.0, 3.0, 0xC0), client_sync.MESSAGE.pack(8, math.nan, 0.0, 0.0, 0xC0),
                         b'short', client_sync.MESSAGE.pack(9, 4.0, 5.0, 6.0, 0xC0) + b'x'):
                b.sendto(data, a.getsockname())
            resync = client_sync.Resync()
            for _ in range(100):  # loopback delivery is quick but not instant
                client_sync.receive(a, resync)
                if 7 in resync.client:
                    break
            client_sync.receive(a, resync)
            self.assertEqual(resync.client, {7: ((1.0, 2.0, 3.0), 0xC0)})
        finally:
            a.close()
            b.close()


    def test_a_marker_datagram_is_shadow_steps_place_as_the_client_drew_it(self):
        a, b = socket.socket(socket.AF_INET, socket.SOCK_DGRAM), socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        a.bind(('127.0.0.1', 0))
        a.setblocking(False)
        try:
            b.sendto(client_sync.MARKER.pack(11, 1.0, 2.0, 3.0, 1), a.getsockname())
            b.sendto(client_sync.MARKER.pack(12, math.nan, 2.0, 3.0, 1), a.getsockname())  # dropped
            resync = client_sync.Resync()
            for _ in range(100):
                client_sync.receive(a, resync)
                if resync.marked:
                    break
            client_sync.receive(a, resync)
            self.assertEqual(resync.marked, (11, (1.0, 2.0, 3.0), True))
            with mock.patch.object(client_sync, '_resync', resync):
                self.assertEqual(client_sync.marker(20), ((1.0, 2.0, 3.0), True))
                self.assertIsNone(client_sync.marker(11 + client_sync.MARKER_FRAMES + 1))  # stale: the aim was put away
        finally:
            a.close()
            b.close()


class SteppedTest(unittest.TestCase):
    def test_off_without_the_port(self):
        mover = Mover((0.0, 0.0, 0.0))
        with mock.patch.object(client_sync, '_port', None), mock.patch.object(client_sync, '_listen') as listen:
            client_sync.stepped(mover, 1)
        self.assertEqual(mover.position, [0.0, 0.0, 0.0])
        listen.assert_not_called()

    def test_the_port_is_bound_on_the_first_step_not_on_import(self):
        with mock.patch.object(client_sync, '_port', '47124'), mock.patch.object(client_sync, '_sock', None), \
                mock.patch.object(client_sync, '_listen', return_value=object()) as listen, \
                mock.patch.object(client_sync, 'receive'):
            client_sync.stepped(Mover((0.0, 0.0, 0.0)), 1)
            client_sync.stepped(Mover((0.0, 0.0, 0.0)), 2)
        listen.assert_called_once_with('47124')

    def test_the_mover_follows_the_client_and_a_new_body_starts_over(self):
        mover = Mover((0.0, 0.0, 0.0))
        right = Command(frame=1, forward=0, right=127, yaw=0, pitch=0, buttons=0)  # -x at yaw 0

        def feed(resync, frame, feet):
            resync.observed(frame, feet, 0xA0)

        with mock.patch.object(client_sync, '_port', '1'), mock.patch.object(client_sync, '_sock', object()), \
                mock.patch.object(client_sync, '_mover', None), \
                mock.patch.object(client_sync, 'receive', side_effect=lambda sock, resync: feed(resync, 1, (-0.03, 0.0, 0.01))):
            mover.step(right, 0.007)
            client_sync.stepped(mover, 1)
            self.assertEqual([round(v, 6) for v in mover.position], [-0.03, 0.0, 0.01])
            first = client_sync._resync
            body = Mover((5.0, 0.0, 5.0))
            client_sync.stepped(body, 2)  # frame 1 belongs to the old body
            self.assertIsNot(client_sync._resync, first)
            self.assertEqual(client_sync._resync.stale, 0xA0)
            self.assertEqual(body.position, [5.0, 0.0, 5.0])


if __name__ == '__main__':
    unittest.main()
