"""onfire.py: the fire score's rule, and what the player's entity carries of it (board.py's component 64)."""
import unittest

import practice  # noqa: F401  (puts vendor/ on the path)
import board
import onfire
from ow174.game.content import PRACTICE_RANGE, SOLDIER
from test_target import _match

RANGE = PRACTICE_RANGE.mode_guid


class FireTests(unittest.TestCase):
    def test_three_whole_eliminations_in_a_row_set_him_on_fire_and_time_puts_it_out(self):
        fire = onfire.Fire()
        fire.tick(10.0)
        for _ in range(2):
            fire.add(100)
        self.assertFalse(fire.burning)
        fire.add(100)
        self.assertTrue(fire.burning)
        fire.tick(19.0)  # 5 points a second
        self.assertEqual(fire.score, 255.0)
        fire.tick(21.0)
        self.assertFalse(fire.burning)
        fire.tick(200.0)
        self.assertEqual(fire.score, 0.0)

    def test_the_component_carries_the_score_its_threshold_and_the_flag(self):
        fire = onfire.Fire()
        fire.add(260.4)
        values = board.component({}, RANGE, SOLDIER, fire)
        self.assertEqual((values[19], values[20], values[36]), (260.0, onfire.THRESHOLD, 1))  # whole points
        self.assertEqual(board.component({}, RANGE, SOLDIER, onfire.Fire())[36], 0)

    def test_an_elimination_scores_the_share_of_the_targets_health_he_took(self):
        match, player = _match()
        match._target_updates(player)
        bot = next(b for b in match.bots if b.team != player.team)
        match._damage_bot(player, bot, bot.max_health / 2, 10.0)
        self.assertEqual(player.fire.score, 0.0)  # not for damage alone
        match._damage_bot(player, bot, bot.max_health, 10.5)
        self.assertEqual(player.fire.score, 100.0)  # all of its health was his


if __name__ == '__main__':
    unittest.main()
