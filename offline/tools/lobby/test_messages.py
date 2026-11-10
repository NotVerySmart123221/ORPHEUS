import unittest

import practice  # installs the pinned vendor path
from ow174.game import messages
from ow174.game.bits import BitReader, BitWriter


class MessageBoundaryTests(unittest.TestCase):
    def test_final_elimination_boolean_leaves_next_channel_at_exact_bit(self):
        elimination = messages.encode(25005, {
            '+0x78': 0, '+0x80': {'+0x0': 0xA0000101},
            '+0x84': {'+0x0': 0xB0000501}, '+0x88': 100, '+0x89': False})
        hit = messages.encode(25001, {
            '+0x78': {'+0x0': {'+0x0': 0xB0000501}, '+0x4': 20, '+0x8': 0},
            '+0x84': 2013})
        for reliable, unreliable, expected_count, expected_reliable, expected_unreliable in (
                ([(0, hit), (1, elimination)], [], 353, [25001, 25005], []),
                ([], [elimination], 169, [], [25005]),
                ([(0, elimination), (1, hit)], [], 360, [25005, 25001], [])):
            with self.subTest(reliable=len(reliable), unreliable=len(unreliable)):
                out = BitWriter()
                messages.write_channel(out, reliable, unreliable)
                self.assertEqual(out.count, expected_count)
                out.var_a(2013)
                out.bits(0x35, 6)
                reader = BitReader(out.getvalue())
                got_reliable, got_unreliable = messages.read_channel(reader)
                self.assertEqual([item[1] for item in got_reliable], expected_reliable)
                self.assertEqual([item[0] for item in got_unreliable], expected_unreliable)
                self.assertEqual(reader.pos, expected_count)
                self.assertEqual((reader.var_a(), reader.bits(6)), (2013, 0x35))
