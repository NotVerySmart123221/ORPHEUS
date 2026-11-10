import math
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import practice  # noqa: F401  (puts vendor/ on the path)
from test_target import _match


class ViewerTests(unittest.TestCase):
    def test_mannequin_stands_in_front_of_the_spawn_facing_it_and_does_no_damage(self):
        import viewer
        bot = viewer.mannequin('reaper', ((10.0, 1.0, 20.0), 90.0))
        self.assertEqual(bot.team, 1)
        self.assertEqual(bot.behaviour, 'shoot')
        self.assertTrue(math.isclose(bot.position[0], 10.0 + viewer.DISTANCE) and math.isclose(bot.position[2], 20.0, abs_tol=1e-9))
        self.assertEqual(bot.yaw, 270.0)
        self.assertEqual((bot.weapon.damage, bot.weapon.far_damage), (0, 0))
        self.assertIsNone(viewer.mannequin('Nobody', ((0, 0, 0), 0)))
        self.assertIsNotNone(viewer.mannequin('Tracer', ((0, 0, 0), 0)))  # no bot kit or hit volumes of its own

    def test_the_match_swaps_the_mannequin_when_the_file_names_another_hero(self):
        from ow174.game.world import OP_DESTROY
        with tempfile.TemporaryDirectory() as folder:
            control = Path(folder) / 'viewer.txt'
            control.write_text('Reaper', encoding='utf-8')
            with patch.dict(os.environ, {'OW_VIEWER_FILE': str(control)}):
                match, player = _match()
                match._target_updates(player)  # the Range bots
                match._tick_viewer(0.0)
                first = match.mannequin
                self.assertIn(first, match.bots)
                match._target_updates(player)
                self.assertIn(first.entity, player.bots_seen)
                control.write_text('Mei', encoding='utf-8')
                match._tick_viewer(0.5)  # not checked again yet
                self.assertIs(match.mannequin, first)
                match._tick_viewer(1.0)
                self.assertNotIn(first, match.bots)
                self.assertIsNone(match.mannequin)
                destroyed = lambda: [u.entity for batch in player.client.updates for u in batch if u.op == OP_DESTROY]
                self.assertEqual(destroyed(), [])  # out of play; destroyed, and Mei made, at the next check
                match._tick_viewer(2.0)
                self.assertEqual(destroyed(), [first.entity])
                self.assertNotIn(first.entity, player.bots_seen)
                self.assertIn(match.mannequin, match.bots)
                self.assertNotEqual(match.mannequin.entity, first.entity)
                control.write_text('Me', encoding='utf-8')  # half-typed: keep Mei
                match._tick_viewer(3.0)
                self.assertIn(match.mannequin, match.bots)

    def test_a_swapped_out_mannequin_stops_firing_before_its_destroy(self):
        from types import SimpleNamespace
        from target import WEAPON_EFFECTS, WEAPON_GRAPHS
        from ow174.game.script import decode
        from ow174.game.world import OP_DESTROY
        import oracle
        with tempfile.TemporaryDirectory() as folder:
            control = Path(folder) / 'viewer.txt'
            control.write_text('Mei', encoding='utf-8')
            with patch.dict(os.environ, {'OW_VIEWER_FILE': str(control)}):
                match, player = _match()
                match._target_updates(player)
                match._tick_viewer(0.0)
                mei = match.mannequin
                match._target_updates(player)
                mei.next_attack_at, mei.target = 0.0, player
                match._send_bot_weapons()
                self.assertTrue(player.bot_weapons[mei.entity][1][2])
                player.client.updates.clear()
                control.write_text('Reaper', encoding='utf-8')
                match._tick_viewer(1.0)
                updates = lambda: [u for batch in player.client.updates for u in batch if u.entity == mei.entity]
                self.assertEqual([u.op for u in updates()], [0])  # the frame with her fire off, no destroy yet
                match._tick_viewer(2.0)
                self.assertEqual([u.op for u in updates()], [0, OP_DESTROY])
                frame = decode.ClientBody().apply(SimpleNamespace(chunk=updates()[0].chunk))
                graph = oracle._graph(WEAPON_GRAPHS['Mei'][0][0])
                states = frame.instances[1].states
                for slot in WEAPON_EFFECTS['Mei']:
                    self.assertFalse(states[graph.remote_node(slot).state][0])  # the spray ended


if __name__ == '__main__':
    unittest.main()
