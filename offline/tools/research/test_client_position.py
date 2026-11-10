import itertools
import struct
import unittest
from unittest import mock

import client_position as cp

BASE = 0x7FF600000000


def slot_bytes(frame, position, vtable=BASE + cp.VTABLE_RVA):
    raw = bytearray(cp.FRAME + 4)
    struct.pack_into('<Q', raw, 0, vtable)
    struct.pack_into('<Q', raw, 0x20, BASE + cp.SECOND_VTABLE_RVA)
    struct.pack_into('<3f', raw, cp.POSITION, *position)
    struct.pack_into('<I', raw, cp.FRAME, frame)
    return bytes(raw)


def mover(memory):
    """A ClientMover over {address: slot bytes}; reads elsewhere come back short."""
    client = object.__new__(cp.ClientMover)
    client.base, client.k32, client.process = BASE, None, None
    client.pool, client.slot, client.next_scan = [], None, 0.0
    client.last_frame, client.last_change = None, 0.0
    client._read = lambda address, size: memory.get(address, b'')[:size]
    return client


class SampleTest(unittest.TestCase):
    def test_frame_and_feet(self):
        client = mover({0x1000: slot_bytes(77, (54.5, 1.0, 42.0))})
        self.assertEqual(client.sample(0x1000), (77, (54.5, 1.0, 42.0)))

    def test_free_slot_wrong_vtable_and_nan_are_not_bodies(self):
        client = mover({1: slot_bytes(5, (0.0, 0.0, 0.0)), 2: slot_bytes(5, (1.0, 1.0, 1.0), vtable=BASE),
                        3: slot_bytes(5, (float('nan'), 1.0, 1.0))})
        for slot in (1, 2, 3, 4):
            self.assertIsNone(client.sample(slot))

    def test_torn_reads_are_retried_then_dropped(self):
        frames = itertools.count()
        client = mover({})
        client._read = lambda address, size: slot_bytes(next(frames), (1.0, 1.0, 1.0))  # written under every read
        self.assertIsNone(client.sample(0x1000))
        reads = iter([slot_bytes(5, (1.0, 1.0, 1.0)), *[slot_bytes(6, (1.1, 1.0, 1.0))] * 3])
        client._read = lambda address, size: next(reads)
        self.assertEqual(client.sample(0x1000), (6, (1.100000023841858, 1.0, 1.0)))


class FindTest(unittest.TestCase):
    def test_scan_then_the_positioned_slot(self):
        memory = {0x10: slot_bytes(90, (0.0, 0.0, 0.0)), 0x20: slot_bytes(100, (3.0, 1.0, 4.0))}
        client = mover(memory)
        with mock.patch.object(cp, 'scan_for_bytes', return_value=[0x10, 0x20]) as scan:
            self.assertEqual(client.find(), 0x20)
            memory[0x10], memory[0x20] = slot_bytes(120, (5.0, 1.0, 5.0)), slot_bytes(100, (0.0, 0.0, 0.0))
            self.assertEqual(client.find(), 0x10)  # a respawn: the remembered pool, no new scan
        self.assertEqual(scan.call_count, 1)

    def test_a_new_body_just_outside_the_pool_is_found_without_a_scan(self):
        memory = {0x10000: slot_bytes(100, (3.0, 1.0, 4.0))}
        client = mover(memory)
        with mock.patch.object(cp, 'scan_for_bytes', return_value=[0x10000]) as scan:
            self.assertEqual(client.read(), (100, (3.0, 1.0, 4.0)))
            del memory[0x10000]  # freed
            memory[0x10000 - cp.STRIDE] = slot_bytes(101, (5.0, 1.0, 5.0))
            self.assertEqual(client.read(), (101, (5.0, 1.0, 5.0)))
        self.assertEqual(scan.call_count, 1)
        self.assertIn(0x10000 - cp.STRIDE, client.pool)

    def test_full_scans_are_throttled(self):
        client = mover({})
        with mock.patch.object(cp, 'scan_for_bytes', return_value=[]) as scan:
            self.assertIsNone(client.find())
            self.assertIsNone(client.find())
        self.assertEqual(scan.call_count, 1)

    def test_read_looks_again_when_the_frame_stalls(self):
        memory = {0x10: slot_bytes(100, (3.0, 1.0, 4.0)), 0x20: slot_bytes(0, (0.0, 0.0, 0.0))}
        client = mover(memory)
        with mock.patch.object(cp, 'scan_for_bytes', return_value=[0x10, 0x20]), \
                mock.patch.object(cp.time, 'monotonic', return_value=1000.0):
            self.assertEqual(client.read(), (100, (3.0, 1.0, 4.0)))
        memory[0x20] = slot_bytes(200, (6.0, 1.0, 6.0))  # the new body; the old one keeps its last frame
        with mock.patch.object(cp.time, 'monotonic', return_value=1000.0 + cp.STALE + 1):
            self.assertEqual(client.read(), (200, (6.0, 1.0, 6.0)))
        self.assertEqual(client.slot, 0x20)


if __name__ == '__main__':
    unittest.main()
