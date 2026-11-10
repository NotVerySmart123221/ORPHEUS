import unittest

import practice
from ow174.game.commands import Command
from soldier import RELOAD, Rifle
from target import PLAYER_KIT


class RifleTests(unittest.TestCase):
    def test_ashe_inserts_one_round_at_a_time_and_closes(self):
        rifle = Rifle(10000, PLAYER_KIT['Ashe'][1])
        rifle.ammo = 9
        for frame in (100, 149, 150, 174, 175, 200, 230):
            rifle.command(Command(frame, buttons=RELOAD if frame == 100 else 0))
        self.assertEqual([(u.ammo, u.reload_phase) for u in rifle.updates],
                         [(9, 'open'), (10, 'insert'), (11, 'insert'),
                          (12, 'close'), (12, None)])

    def test_ashe_can_close_and_fire_without_waiting_for_a_full_magazine(self):
        rifle = Rifle(10000, PLAYER_KIT['Ashe'][1])
        rifle.ammo = 1
        rifle.command(Command(100, buttons=RELOAD))
        rifle.command(Command(150))
        self.assertFalse(rifle.command(Command(160, buttons=1)))
        self.assertEqual((rifle.ammo, rifle.updates[-1].reload_phase), (2, 'close'))
        self.assertTrue(rifle.command(Command(190, buttons=1)))
        self.assertEqual((rifle.ammo, rifle.serial), (1, 1))

    def test_ashe_empty_reload_catches_up_without_skipping_insert_updates(self):
        rifle = Rifle(10000, PLAYER_KIT['Ashe'][1])
        rifle.ammo = 1
        self.assertTrue(rifle.command(Command(100, buttons=1)))
        self.assertEqual(rifle.reload_until, 455)
        rifle.command(Command(455))
        self.assertEqual([u.ammo for u in rifle.updates if u.reloading], list(range(13)))
        self.assertEqual((rifle.ammo, rifle.reload_until), (12, None))

    def test_doomfist_recharges_one_round_per_wait_after_the_shot_timer(self):
        rifle = Rifle(10000, PLAYER_KIT['Doomfist'][1])
        self.assertTrue(rifle.command(Command(100, buttons=1)))
        rifle.command(Command(101, buttons=RELOAD))  # no manual reload
        self.assertIsNone(rifle.reload_until)
        for frame in range(102, 200):
            rifle.command(Command(frame))
        self.assertEqual([(u.ammo, u.reloading, u.reload_phase) for u in rifle.updates][-2:],
                         [(3, True, 'recharge'), (4, False, None)])
        self.assertEqual(rifle.ammo, 4)

    def test_doomfist_shot_aborts_the_recharge_wait(self):
        rifle = Rifle(10000, PLAYER_KIT['Doomfist'][1])
        rifle.command(Command(100, buttons=1))
        for frame in range(101, 150):
            rifle.command(Command(frame))  # the Wait starts at 132
        self.assertTrue(rifle.command(Command(150, buttons=1)))
        for frame in range(151, 197):
            rifle.command(Command(frame))  # the first Wait would have ended at 197; the shot restarted it at 182
        self.assertEqual(rifle.ammo, 2)
        for frame in range(197, 248):
            rifle.command(Command(frame))
        self.assertEqual(rifle.ammo, 3)

    def test_doomfist_empty_magazine_recharges_without_a_reload(self):
        rifle = Rifle(10000, PLAYER_KIT['Doomfist'][1])
        for frame in range(100, 230):
            rifle.command(Command(frame, buttons=1))
        self.assertEqual((rifle.ammo, rifle.reload_until, rifle.serial), (0, None, 4))
        rifle.command(Command(292, buttons=1))  # held fire with an empty magazine does not stop the refill
        self.assertEqual(rifle.ammo, 0)
        self.assertTrue(rifle.command(Command(293, buttons=1)))  # the recharged round fires at once
        self.assertEqual((rifle.ammo, rifle.serial), (0, 5))

    def test_reload_start_and_finish_are_delivered_without_another_shot(self):
        rifle = Rifle(7000)
        rifle.command(Command(100, buttons=1))
        rifle.updates.clear()
        self.assertFalse(rifle.command(Command(101, buttons=0x400)))
        start, = rifle.updates
        self.assertEqual((start.ammo, start.active, start.reloading), (29, False, True))
        rifle.updates.clear()
        rifle.command(Command(206))  # 0.489 of the 1.5 s reload (105 of 215 ticks): the magazine refills
        self.assertEqual((rifle.ammo, len(rifle.updates)), (30, 0))  # silently: an update restarts the reload Anim
        rifle.command(Command(316))
        finish, = rifle.updates
        self.assertEqual((finish.ammo, finish.active, finish.reloading), (30, False, False))

    def test_last_shot_starts_reload_on_stop_without_exceeding_queue_limit(self):
        rifle = Rifle(7000)
        rifle.ammo = 1
        rifle.command(Command(100, buttons=1))
        start, stop = rifle.updates
        self.assertFalse(start.reloading)
        self.assertTrue(stop.reloading)
        rifle.updates.extend([stop] * (128 - len(rifle.updates)))
        rifle.command(Command(400))
        self.assertEqual(rifle.ammo, 0)
        self.assertEqual(len(rifle.updates), 128)

    def test_frame_restart_preserves_remaining_reload_time(self):
        rifle = Rifle(7000)
        self.assertTrue(rifle.command(Command(2000, buttons=1)))
        rifle.command(Command(2001, buttons=0x400))
        self.assertFalse(rifle.command(Command(10, buttons=1)))
        self.assertFalse(rifle.command(Command(114, buttons=1)))
        self.assertTrue(rifle.command(Command(115, buttons=1)))  # refilled 105 ticks after the press
        self.assertFalse(rifle.command(Command(115, buttons=1)))
        self.assertEqual((rifle.ammo, rifle.serial), (29, 2))

    def test_tap_keeps_start_and_stop_in_order(self):
        rifle = Rifle(7000)
        self.assertTrue(rifle.command(Command(100, buttons=1)))
        self.assertFalse(rifle.command(Command(101)))
        self.assertEqual(rifle.ammo, 29)
        self.assertEqual([(u.active, u.serial, u.count_update) for u in rifle.updates],
                         [(True, 1, True), (False, 1, True)])

    def test_held_burst_stays_active_until_release(self):
        rifle = Rifle(7000)
        for frame in range(100, 133):  # held: three pulses 16 frames apart
            rifle.command(Command(frame, buttons=1))
        rifle.command(Command(133))
        self.assertEqual([(u.active, u.serial) for u in rifle.updates], [(True, 1), (True, 2), (True, 3), (False, 3)])

    def test_refill_ends_reload_with_a_full_magazine(self):
        rifle = Rifle(7000)
        rifle.command(Command(100, buttons=1))
        rifle.command(Command(101, buttons=0x400))
        rifle.refill()
        self.assertEqual((rifle.ammo, rifle.reload_until, rifle.updates[-1].ammo), (30, None, 30))

    def test_rate_limit_and_reload_follow_command_time(self):
        rifle = Rifle(7000)
        self.assertTrue(rifle.command(Command(100, buttons=1)))
        self.assertFalse(rifle.command(Command(115, buttons=1)))
        self.assertTrue(rifle.command(Command(116, buttons=1)))
        self.assertFalse(rifle.command(Command(117, buttons=0x400)))
        self.assertFalse(rifle.command(Command(221, buttons=1)))
        self.assertTrue(rifle.command(Command(222, buttons=1)))  # after the refill a shot cuts the reload short
        self.assertEqual((rifle.ammo, rifle.serial, rifle.reload_until), (29, 3, None))
        self.assertEqual([(u.ammo, u.reloading) for u in list(rifle.updates)[-2:]], [(28, True), (29, False)])

    def test_empty_clip_reloads_and_backpressure_preserves_ammo(self):
        rifle = Rifle(7000)
        for i in range(30):
            self.assertTrue(rifle.command(Command(100 + i * 16, buttons=1)))
        self.assertEqual(rifle.ammo, 0)
        self.assertFalse(rifle.command(Command(600, buttons=1)))
        rifle.updates.clear()
        self.assertTrue(rifle.command(Command(800, buttons=1)))
        self.assertEqual(rifle.ammo, 29)
        rifle.updates.extend([rifle.updates[-1]] * 128)
        self.assertFalse(rifle.command(Command(900, buttons=1)))
        self.assertEqual(rifle.ammo, 29)

    def test_a_hero_kit_sets_cadence_magazine_and_reload(self):
        from target import HERO_KIT, SOLDIER_RIFLE
        soldier, default = Rifle(7000, SOLDIER_RIFLE), Rifle(7000)
        self.assertEqual((soldier.interval, soldier.capacity, soldier.reload_duration),
                         (default.interval, default.capacity, default.reload_duration))
        revolver = Rifle(7000, HERO_KIT['Cassidy'][1])  # 0.496 s a shot, 6 rounds, 1.5 s reload
        self.assertEqual((revolver.interval, revolver.ammo, revolver.reload_duration), (71, 6, 215))
        self.assertTrue(revolver.command(Command(100, buttons=1)))
        self.assertFalse(revolver.command(Command(170, buttons=1)))
        self.assertTrue(revolver.command(Command(171, buttons=1)))
        revolver.ammo = 1
        revolver.command(Command(242, buttons=1))
        self.assertEqual((revolver.ammo, revolver.reload_until), (0, 242 + 215))
        revolver.command(Command(242 + 114))
        self.assertEqual(revolver.ammo, 0)
        revolver.command(Command(242 + 115))  # Cassidy's graph refills 0.53333 of the way (0.8 s)
        self.assertEqual((revolver.ammo, revolver.reload_until), (6, 242 + 215))
        revolver.command(Command(242 + 215))
        self.assertIsNone(revolver.reload_until)


if __name__ == '__main__':
    unittest.main()
