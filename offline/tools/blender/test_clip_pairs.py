import unittest

from clip_pairs import paired_clip


ARMS, GUN = '0x04000000000003D1', '0x04000000000004BF'
ARMS_RELOAD, GUN_RELOAD = '0x0A00000000001C00', '0x0A00000000001D8C'


class ClipPairTests(unittest.TestCase):
    def test_reload_resolves_in_both_directions(self):
        self.assertEqual(paired_clip(ARMS, ARMS_RELOAD, GUN, [GUN_RELOAD]), GUN_RELOAD)
        self.assertEqual(paired_clip(GUN, GUN_RELOAD, ARMS, [ARMS_RELOAD]), ARMS_RELOAD)

    def test_missing_or_unknown_pair_never_selects_a_different_clip(self):
        self.assertIsNone(paired_clip(ARMS, ARMS_RELOAD, GUN, ['0x0A00000000002181']))
        self.assertIsNone(paired_clip(ARMS, '0x0A000000000027D7', GUN, [GUN_RELOAD]))
        self.assertIsNone(paired_clip(ARMS, ARMS_RELOAD, ARMS, [GUN_RELOAD]))

    def test_ashe_insert_uses_the_weapon_clip_for_each_category(self):
        arms, gun = '0x040000000000251D', '0x0400000000002523'
        weapon_clips = ['0x0A00000000005A4E', '0x0A00000000006051', '0x0A00000000006050']
        for clip, expected in zip(('0x0A00000000005A0A', '0x0A0000000000603E', '0x0A00000000006020'), weapon_clips):
            with self.subTest(clip=clip):
                self.assertEqual(paired_clip(arms, clip, gun, weapon_clips), expected)
