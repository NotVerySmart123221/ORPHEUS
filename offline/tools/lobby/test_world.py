import math
import unittest
from types import SimpleNamespace
from unittest.mock import patch

import practice  # installs the pinned vendor path
from ow174.game import world
from ow174.game.bits import BitReader, BitWriter
from ow174.game.server import Client


class ProjectileStateTests(unittest.TestCase):
    def test_absolute_snapshot_matches_native_decoder_fixture(self):
        state = world.projectile_state(1234, (54.5, 1, 42), (0, 0, 50), event_count=1,
                                       source_entities=(0x80000001, 0x80000001))
        # Native1F88980 decoded every one of these233 state bits after the52-bit tick header.
        self.assertEqual(state.count, 285)
        self.assertEqual(state.getvalue().hex(),
                         'ffffffff374d00008096100000e00f00008a500000000000000000000024213888000400')
        reader = BitReader(state.getvalue())
        self.assertEqual(reader.var_b(), 0xFFFFFFFF)
        self.assertEqual(reader.var_b(), 1235)

    def test_a_stopped_snapshot_has_its_two_flags_and_no_velocity(self):
        moving = world.projectile_state(1234, (1, 2, 3), (0, 0, 50))
        stopped = world.projectile_state(1234, (1, 2, 3), (0, 0, 50), stopped=True)
        self.assertEqual(moving.count - stopped.count, 1 + 96 - 2)  # the velocity bit and block, less flags +2 and +3
        reader = BitReader(stopped.getvalue())
        reader.var_b(), reader.var_b()
        self.assertEqual([reader.bit() for _ in range(4)], [1, 0, 0, 0])  # +4 stopped, +1, +2, +3

    def test_state_only_and_combined_shot_use_channel2_state_bit(self):
        state = world.projectile_state(1234, (1, 2, 3), (0, 0, 50))
        shot = world.shot_record(0, 1, 16, 2, (1, 2, 3), (0, 0, 1))
        for event in (None, shot):
            with self.subTest(shot=event is not None):
                out = BitWriter()
                world._write_entity(out, world.EntityUpdate(0xA1000000, shot=event, projectile=state))
                reader = BitReader(out.getvalue())
                self.assertEqual(reader.entity_id(), 0xA1000000)
                self.assertEqual(reader.bits(4), 0b1011)  # ch0, ch1, no chunk, ch2
                self.assertEqual(reader.bit(), event is not None)
                if event is not None:
                    self.assertEqual(reader.bit(), 1)
                    self.assertEqual(reader.var_b(), 0)
                    self.assertEqual(reader.bits(shot.count), shot.value)
                    self.assertEqual(reader.bit(), 0)  # no more events
                self.assertEqual(reader.bit(), 1)  # projectile state follows
                self.assertEqual(reader.bits(state.count), state.value)
                self.assertEqual(reader.bit(), 0)  # end before ch3

    def test_snapshot_rejects_invalid_wire_values(self):
        for change in ({'tick': -1}, {'tick': 0xFFFFFFFF}, {'tick': 1.5},
                       {'event_count': -1}, {'event_count': 1 << 32},
                       {'position': (1, 2)}, {'position': (1, math.nan, 3)},
                       {'velocity': (0, math.inf, 1)}, {'velocity': (0, 1e39, 1)}, {'source_entities': (1,)},
                       {'source_entities': (1, 1 << 32)}):
            args = dict(tick=1234, position=(1, 2, 3), velocity=(0, 0, 50))
            args.update(change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                world.projectile_state(**args)

    def test_shot_only_wire_is_unchanged(self):
        shot = world.shot_record(0, 1, 16, 2, (1, 2, 3), (0, 0, 1))
        out = BitWriter()
        world._write_entity(out, world.EntityUpdate(0xA1000000, shot=shot))
        self.assertEqual(out.count, 217)
        self.assertEqual(out.getvalue().hex(), '0d000010b00300882084020000f00700000008000008080010001000')

    def test_linked_projectile_shot_has_current_tick_for_native_initialization(self):
        shot = world.shot_record(0, 1, 16, 2, (1, 2, 3), (0, 0, 1), entities=(0xA1000000,))
        out = BitWriter()
        world._write_entity(out, world.EntityUpdate(0xA0000101, shot=shot, shot_time=1234))
        reader = BitReader(out.getvalue())
        self.assertEqual(reader.entity_id(), 0xA0000101)
        self.assertEqual(reader.bits(6), 0b111011)  # channels0/1/2, no chunk, one event
        self.assertEqual(reader.var_b(), 1234)
        self.assertEqual(reader.bits(shot.count), shot.value)
        for tick in (-1, 1 << 32, 1.5):
            with self.subTest(tick=tick), self.assertRaises(ValueError):
                world.EntityUpdate(0xA0000101, shot=shot, shot_time=tick)

    def test_shot_links_projectile_entities_without_changing_volley_key(self):
        plain = world.shot_record(0, 1, 16, 2, (1, 2, 3), (0, 0, 1))
        linked = world.shot_record(0, 1, 16, 2, (1, 2, 3), (0, 0, 1), entities=(0xA1000000,))
        self.assertEqual((linked.count, linked.getvalue().hex()),
                         (219, '00118450000000fe00000000010000010100020016fdff0700000800'))
        self.assertEqual(linked.value & ((1 << (plain.count - 1)) - 1), plain.value)
        reader = BitReader(linked.getvalue())
        reader.bits(plain.count - 1)
        self.assertEqual(reader.bit(), 1)
        self.assertEqual((reader.bit(), reader.bits(4)), (0, 1))  # native w_u16 count1
        self.assertEqual(reader.bits(2), 1)  # entity kind
        self.assertEqual((reader.bit(), reader.bits(16), reader.bits(32)), (1, 0xFFFF, 0x1000000))
        self.assertEqual(reader.pos, linked.count)
        for entities in ((-1,), (1 << 32,), (0,), (0x80000000,), (0xA1000000,) * 65536):
            with self.subTest(count=len(entities)), self.assertRaises(ValueError):
                world.shot_record(0, 1, 16, 2, (1, 2, 3), (0, 0, 1), entities=entities)

    def test_linked_shot_matches_native_mixed_entity_delta_fixture(self):
        shot = world.shot_record(0, 1, 16, 2, (1, 2, 3), (0, 0, 1),
                                 entities=(0xA1000000, 0xA1000001, 0xA1000101, 0xA0FFFFFF))
        self.assertEqual(shot.count, 306)
        self.assertEqual(shot.getvalue().hex(),
                         '00118450000000fe00000000010000010100020046fdff07000008c87f80000000ff0b04000000')

    def test_projectile_state_leaves_send_queue_and_resends_only_after_loss(self):
        for delivered in (True, False):
            with self.subTest(delivered=delivered):
                record = world.EntityUpdate(0xA1000000, projectile=world.projectile_state(1234, (1, 2, 3), (0, 0, 50)))
                client = Client.__new__(Client)
                client.reliable, client.unreliable, client.entities = [], [], [record]
                client.in_flight, client.seq, client.conn = {}, 7, 1
                client.player = SimpleNamespace(match=SimpleNamespace(gameplay_enabled=False, remote_movements=lambda viewer, frame: []))
                with patch.object(client, '_payload', return_value=b''), patch.object(client, '_send'):
                    client.send_frame(1234, 0)
                self.assertEqual(client.entities, [])
                self.assertEqual(client.in_flight[7][1], [record])
                with patch('ow174.game.server.delivery', return_value=delivered), patch.object(client, 'log'):
                    client.acked(7, 0)
                self.assertEqual(record.delivered, delivered)
                self.assertEqual(client.entities, [] if delivered else [record])

    def test_a_destroy_cut_from_a_full_frame_waits_for_the_next(self):
        # a projectile step's destroy lost this way left D.Va's missiles frozen in the air
        from ow174.game.server import MAX_PAYLOAD
        move, destroy, create = (world.EntityUpdate(0xB1000101), world.EntityUpdate(0xB1000101, 3),
                                 world.EntityUpdate(0xB1000102, 1))
        create.resend = False  # a step's create is not replayed
        client = Client.__new__(Client)
        client.reliable, client.unreliable, client.entities = [], [], []
        client.in_flight, client.seq, client.conn = {}, 7, 1
        client.player = SimpleNamespace(match=SimpleNamespace(gameplay_enabled=False, remote_movements=lambda viewer, frame: [move, destroy, create]))
        oversized = lambda frame, reliable, unreliable, entities: bytes(MAX_PAYLOAD + 1 if len(entities) > 1 else 0)
        with patch.object(client, '_payload', side_effect=oversized), patch.object(client, '_send'):
            client.send_frame(1234, 0)
        self.assertEqual(client.entities, [destroy])

    def test_reliable_messages_that_overfill_a_frame_wait_for_the_next(self):
        # A new mannequin's 20301 and one entity record made a 1237-byte frame; the client never acked it and every
        # resend was the same, which stalled the reliable channel for good (run 29).
        from ow174.game.server import MAX_PAYLOAD
        big, small, destroy = (0, bytes(MAX_PAYLOAD - 5)), (1, bytes(100)), world.EntityUpdate(0xB1000101, 3)
        client = Client.__new__(Client)
        client.reliable, client.unreliable, client.entities = [big, small], [], [destroy]
        client.in_flight, client.seq, client.conn = {}, 7, 1
        client.player = SimpleNamespace(match=SimpleNamespace(gameplay_enabled=False, remote_movements=lambda viewer, frame: []))
        size = lambda frame, reliable, unreliable, entities: bytes(sum(len(m) for _, m in reliable) + 8 * len(entities))
        with patch.object(client, '_payload', side_effect=size), patch.object(client, '_send'), \
                patch.object(client, 'log'):
            client.send_frame(1234, 0)
            self.assertEqual(client.in_flight[7], ([big], []))
            self.assertEqual((client.reliable, client.entities), ([small], [destroy]))
            client.send_frame(1235, 0)
        self.assertEqual(client.in_flight[7], ([small], [destroy]))


if __name__ == '__main__':
    unittest.main()
