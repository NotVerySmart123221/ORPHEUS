import unittest

from assault import BONUS_SECONDS, CAPTURE_SECONDS, ROUND_SECONDS, Assault, on_point


def run(game, start, end, attackers, defenders=0, step=0.5):
    events, now = [], start
    while now <= end:
        event = game.tick(now, attackers, defenders)
        if event:
            events.append((event, now))
        now += step
    return events


class AssaultTests(unittest.TestCase):
    def test_taking_both_points_wins(self):
        game = Assault([(0, 0, 0), (50, 0, 0)], 0)
        events = run(game, 0, 2 * CAPTURE_SECONDS + 1, attackers=1)
        self.assertEqual([e for e, _ in events], ['captured', 'won'])
        self.assertEqual(game.ends, ROUND_SECONDS + BONUS_SECONDS)
        self.assertEqual(game.winner, 0)

    def test_a_defender_contests(self):
        game = Assault([(0, 0, 0), (50, 0, 0)], 0)
        run(game, 0, CAPTURE_SECONDS * 2, attackers=1, defenders=1)
        self.assertEqual((game.index, game.progress), (0, 0.0))

    def test_time_out_loses_unless_an_attacker_holds_the_point(self):
        game = Assault([(0, 0, 0)], 0)
        run(game, ROUND_SECONDS - 1, ROUND_SECONDS + 5, attackers=1, defenders=1)
        self.assertTrue(game.overtime(ROUND_SECONDS + 5))
        self.assertEqual(run(game, ROUND_SECONDS + 6, ROUND_SECONDS + 7, attackers=0), [('lost', ROUND_SECONDS + 6)])
        self.assertEqual(game.winner, 1)

    def test_on_point(self):
        self.assertTrue(on_point((3, 1, 4), (0, 0, 0)))
        self.assertFalse(on_point((7, 0, 0), (0, 0, 0)))
        self.assertFalse(on_point((0, 9, 0), (0, 0, 0)))


class MatchEndTests(unittest.TestCase):
    def test_players_go_back_to_the_menu_after_either_result(self):
        import dataclasses
        import practice  # noqa: F401  (puts vendor/ on the path)
        from test_target import _match
        from ow174.game.match import ASSAULT, LEAVE_GAME, RESULT_SECONDS, SETUP_SECONDS

        for position, duration, winner in (([1.0, 0.0, 1.0], CAPTURE_SECONDS, 0),
                                           ([10.0, 0.0, 10.0], ROUND_SECONDS, 1)):
            with self.subTest(winner=winner):
                match, player = _match()
                match.game_map = dataclasses.replace(match.game_map, mode_guid=ASSAULT,
                                                     objectives=((0, 0, 0),))
                match.assembled, match.assemble_ends = True, 0.0
                player.mover.position = position
                sent = []
                player.client.queue_reliable = lambda kind, value: sent.append((kind, value))
                match._tick_assault(SETUP_SECONDS)
                result = SETUP_SECONDS + duration
                match._tick_assault(result)
                self.assertEqual(match.assault.winner, winner)
                match._tick_assault(result + RESULT_SECONDS - 0.1)
                self.assertEqual(sent, [])
                match._tick_assault(result + RESULT_SECONDS)
                match._tick_assault(result + RESULT_SECONDS + 1)
                self.assertEqual(sent, [(LEAVE_GAME, {'+0x78': 0, '+0x79': False})])

    def test_result_freezes_combat_and_still_tracks_input_frames(self):
        import dataclasses
        from types import SimpleNamespace
        from unittest.mock import Mock

        import practice  # noqa: F401
        from ow174.game.commands import Command
        from ow174.game.match import ASSAULT, SETUP_SECONDS
        from ow174.game.statescript import Stream
        from ow174.game.world import OP_DESTROY
        from test_target import _match

        match, player = _match()
        match.game_map = dataclasses.replace(match.game_map, mode_guid=ASSAULT, objectives=((0, 0, 0),))
        match.assembled, match.assemble_ends = True, 0.0
        player.mover.position = [0.0, 0.0, 0.0]
        player.body_script = Stream(player.body)
        player.steps_done = 4
        match._tick_assault(SETUP_SECONDS)
        end = SETUP_SECONDS + CAPTURE_SECONDS
        shot = SimpleNamespace(landed=False)
        match.projectiles = [(player, shot)]
        match.rockets = [(end + 1,)]
        player.health = 180
        field = 0xB2000001
        match.fields = [[field, player.mover.position, player.team, end, end + 1, [player], player]]
        match._tick_assault(end)
        self.assertEqual(match.assault.winner, 0)
        self.assertEqual(player.health, 180)
        self.assertEqual((match.rockets, match.fields), ([], []))
        self.assertTrue(shot.landed)
        update, = player.client.updates[-1]
        self.assertEqual((update.entity, update.op), (field, OP_DESTROY))

        player.take_commands([Command(20, forward=127, buttons=1)])
        self.assertEqual(player.commands.last_frame, 20)
        self.assertEqual(player.mover.position, [0.0, 0.0, 0.0])
        self.assertEqual(player.rifle.ammo, player.rifle.capacity)

        match._land_rockets = Mock()
        match._bots_heal = Mock()
        match._target_updates = Mock(return_value=[])
        player.respawn_at = end
        body = player.body
        match.update(end + 0.1, 21)
        self.assertEqual(player.body, body)
        match._land_rockets.assert_not_called()
        match._bots_heal.assert_not_called()
        self.assertGreater(player.mode_script.last, 0)


if __name__ == '__main__':
    unittest.main()
