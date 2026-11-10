import unittest
from unittest.mock import patch

import practice  # installs the pinned vendor path
from ow174.game.commands import Command
from ow174.game import messages
from ow174.game import world
from ow174.game.content import SOLDIER
from ow174.game.match import PLAYER_DEFINITION
from test_target import _match
from target import TrainingTarget
from combat import aim_direction


class FeedbackTests(unittest.TestCase):
    def test_bot_roster_slots_remain_stable_after_another_player_joins(self):
        match, player = _match()
        roster = match.game_struct(player)['+0x0']['+0x0']
        bot = match.bots[0]
        slot = bot.slot
        self.assertEqual(roster['+0x0']['+0x0'][slot]['+0x50'], 'Training Bot')
        self.assertIn({'+0x0': bot.controller}, roster['+0xC0'][bot.team]['+0x18'])
        later = match.add_player(2, 'Second Player', SOLDIER, 0, True)
        roster = match.game_struct(later)['+0x0']['+0x0']
        self.assertEqual(bot.slot, slot)
        self.assertNotIn(later.slot, [b.slot for b in match.bots])
        self.assertEqual(roster['+0x0']['+0x0'][later.slot]['+0x50'], later.name)
        self.assertTrue(messages.encode(20301, {'+0x78': match.game_struct(later)}))

    def test_bot_controller_repoints_to_the_respawned_body(self):
        match, player = _match()
        bot = TrainingTarget((0, 0, 8), 180)
        match.bots = [bot]
        with patch.object(world, 'create', return_value=b'') as create:
            for update in match._target_updates(player):
                update.build(0)
        controller, body = create.call_args_list
        self.assertEqual(controller.args[1], PLAYER_DEFINITION)
        self.assertEqual(controller.args[2][44], [bot.entity] * 3)
        self.assertNotIn(123, body.args[2])  # unsupported on Range bodies; caused the native assertion
        identity = bot.controller, bot.slot
        bot.hero = 1  # a hero bot comes back as a new body; a Range bot revives its own (test_nameplates)
        bot.damage(200, 100)
        bot.tick(103)
        updates = match._target_updates(player)
        with patch.object(world, 'update', return_value=b'') as update:
            updates[0].build(0)
        self.assertEqual((bot.controller, bot.slot), identity)
        self.assertEqual(update.call_args.args[1][44], [bot.entity] * 3)

    def test_rendered_hovering_bot_head_is_hittable(self):
        bot = TrainingTarget((0, 0, 8), 180)
        hit = bot.ray((0, 1.6, 0), aim_direction(0, -469))
        self.assertIsNotNone(hit)  # live: crosshair on the dome, previously a miss
        self.assertTrue(hit[1])
        self.assertIsNone(bot.ray((0, -.1, 0), (0, 0, 1)))  # no ghost hit below the hovering body

    def test_folded_gun_arm_is_hittable_and_forward_bind_pose_ghost_is_gone(self):
        bot = TrainingTarget((0, 0, 8), 0)
        hit = bot.ray((.6, .7, 0), (0, 0, 1))
        self.assertIsNotNone(hit)  # rendered idle gun, previously below the static arm capsule
        self.assertFalse(hit[1])
        self.assertIsNone(bot.ray((.55, 3, 8.75), (0, -1, 0)))

    def test_elimination_is_sent_only_to_the_shooter_once_per_life(self):
        match, player = _match()
        bot = TrainingTarget((0, .6, 7), 180, entity=0xB0000101)
        match.bots = [bot]
        player.bots_seen.add(bot.entity)
        player.mover.position = [0, 0, 0]
        for frame in range(10):
            match.fire_at_target(player, Command(frame))
        self.assertEqual(bot.health, 0)
        elimination, = [(mid, data) for mid, data in player.client.messages if mid == 25005]
        self.assertEqual(elimination[1]['+0x84'], {'+0x0': bot.entity})
        self.assertEqual(elimination[1]['+0x88'], 100)
        self.assertTrue(messages.encode(*elimination))
        match.fire_at_target(player, Command(2))
        self.assertEqual(sum(mid == 25005 for mid, _ in player.client.messages), 1)

    def test_nonlethal_damage_and_misses_do_not_send_elimination(self):
        match, player = _match()
        bot = TrainingTarget((0, .6, 7), 180, entity=0xB0000101)
        match.bots = [bot]
        player.bots_seen.add(bot.entity)
        player.mover.position = [0, 0, 0]
        match.fire_at_target(player, Command(1))
        self.assertEqual(bot.health, 180)
        match.fire_at_target(player, Command(2, yaw=16384))
        self.assertFalse(any(mid == 25005 for mid, _ in player.client.messages))
        hit, = [(mid, data) for mid, data in player.client.messages if mid == 25001]
        self.assertEqual(hit[1]['+0x78'], {'+0x0': {'+0x0': bot.entity}, '+0x4': 20, '+0x8': 0})
        self.assertTrue(messages.encode(*hit))

    def test_critical_confirmation_uses_actual_damage_and_the_match_clock(self):
        match, player = _match()
        match.started = 100
        bot = TrainingTarget((0, 0, 8), 180)
        bot.health = 15
        match._damage_bot(player, bot, 40, 101, critical=True)
        hit, elimination = player.client.messages
        self.assertEqual(hit[0], 25001)
        self.assertEqual(hit[1]['+0x78']['+0x4'], 15)
        self.assertEqual(hit[1]['+0x78']['+0x8'], 3)  # critical, and the kill
        self.assertEqual(hit[1]['+0x84'], 1000 + round(1_000_000 / player.quanta))
        self.assertTrue(messages.encode(*hit))
        self.assertEqual(elimination[1]['+0x88'], 8)
        self.assertEqual(bot.damage_by, {player.entity: 15})
        bot.tick(104)
        self.assertEqual(bot.damage_by, {})
        self.assertEqual(bot.health, 200)
        match._damage_bot(player, bot, 200, 105)
        self.assertEqual(player.client.messages[-1][1]['+0x88'], 100)

    def test_dead_target_does_not_repeat_feedback(self):
        match, player = _match()
        bot = TrainingTarget((0, 0, 8), 180)
        match._damage_bot(player, bot, 200, 100)
        player.client.messages.clear()
        match._damage_bot(player, bot, 200, 101)
        self.assertEqual(player.client.messages, [])
