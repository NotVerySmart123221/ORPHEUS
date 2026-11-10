"""board.py: the Tab board's numbers (component 64) and when they go out; the mode frames' state that lets the board
open (research/lizrollin-runtime.md section 22)."""
import dataclasses
import unittest
from unittest import mock

import practice  # noqa: F401  (puts vendor/ on the path)
import board
import oracle
from career import STAT
from ow174.game import match as M
from ow174.game import world
from ow174.game.commands import Command
from ow174.game.content import PRACTICE_RANGE, SOLDIER
from ow174.game.world import OP_CREATE, OP_UPDATE
from test_target import _match

REAPER = 0x02E0000000000002
RANGE = PRACTICE_RANGE.mode_guid
ELIMINATIONS, FIRST_VALUE = 5, 21  # component 64's fields of the first medal stat


class BoardTests(unittest.TestCase):
    def test_the_modes_stats_take_the_medal_slots_and_the_heros_own_the_rest(self):
        # as read off the live board with a number of its own per stat: Eliminations, Objective Kills, Objective Time,
        # Hero Damage Done, Healing Done, Deaths; then Weapon Accuracy, Kill Streak - Best, Final Blows, Helix Rocket
        # Kills, Tactical Visor Kills
        counted = {SOLDIER: {'eliminations': 3, 'final_blows': 2, 'kill_streak': 2, 'deaths': 1, 'shots': 40, 'shots_hit': 10},
                   REAPER: {'eliminations': 4, 'final_blows': 4, 'hero_damage': 250.5}}
        values = board.component(counted, RANGE, SOLDIER)
        self.assertEqual(len(values), len(world.COMPONENT_FIELDS[board.GMP_STATS]))
        self.assertEqual([stat ^ STAT if stat else 0 for stat in values[5:19]],
                         [0x25, 0x326, 0x327, 0x4B8, 0x1D1, 0x29, 0, 0, 0x2F, 0x223, 0x2B, 0x236, 0x237, 0])
        self.assertEqual(values[21:29], [7.0, 0.0, 0.0, 250.5, 0.0, 1.0, 0.0, 0.0])  # the whole match, every hero played
        self.assertEqual(values[29:35], [0.25, 2.0, 2.0, 0.0, 0.0, 0.0])  # Soldier: 76's own; accuracy as a fraction
        self.assertEqual(values[37:45], [1, 0, 0, 1, 0, 0, 0, 0])  # gold above 0; Deaths takes no medal
        self.assertEqual(values[:5] + values[19:21] + values[35:37], [None] * 9)  # the On Fire fields are not sent

    def test_every_slot_is_written_so_a_hero_switch_clears_the_old_heros_stats(self):
        values = board.component({}, RANGE, None)  # no hero yet
        self.assertEqual((values[13:19], values[29:35]), ([0] * 6, [0.0] * 6))
        self.assertEqual(values[ELIMINATIONS], STAT | 0x25)

    def test_the_stat_guids_go_out_in_the_compact_id_form(self):
        # raw u64s showed "UNKNOWN STAT" on the live board
        self.assertEqual(world.COMPONENT_FIELDS[board.GMP_STATS][5:19], ['id'] * 14)
        record = world.update(0, {board.GMP_STATS: board.component({}, RANGE, SOLDIER)})
        self.assertIn(bytes([0b11000001, 0x25, 0x60, 0x08]), record.getvalue())  # mask, then 0x0860...25's set bytes

    def test_the_create_carries_the_numbers_and_a_change_follows_at_most_twice_a_second(self):
        match, player = _match()
        create = next(u for u in match.spawn_updates(player) if (u.entity, u.op) == (player.entity, OP_CREATE))
        match.update(0.9, 1)
        self.assertIsNone(player.board_sent)  # nothing before the create is built
        with mock.patch.object(world, 'create', wraps=world.create) as created:
            create.build(0)
        self.assertEqual(created.call_args.args[2][board.GMP_STATS][FIRST_VALUE], 0.0)
        player.client.updates.clear()
        for now, counted, sent in ((1.0, 1, 1.0), (1.2, 2, 1.0), (1.6, 2, 2.0)):
            player.career[SOLDIER] = {'eliminations': counted}
            match.update(now, 2)
            self.assertEqual(player.board_sent[FIRST_VALUE], sent, now)
        updates = [u for batch in player.client.updates for u in batch if (u.entity, u.op) == (player.entity, OP_UPDATE)]
        self.assertEqual(len(updates), 2)
        with mock.patch.object(world, 'update', wraps=world.update) as updated:
            updates[-1].build(0)
        self.assertEqual(updated.call_args.args[1][board.GMP_STATS][FIRST_VALUE], 2.0)

    def test_weapon_accuracy_counts_the_primarys_shots_and_hits(self):
        match, player = _match()
        match._target_updates(player)
        bot = next(b for b in match.bots if b.team != player.team)
        match.bots = [bot]
        with mock.patch.object(bot, 'ray', side_effect=[(5.0, False), None]),                 mock.patch('ow174.game.match.time.time', return_value=100):
            match.fire_at_target(player, Command(1, yaw=0, pitch=0))
            match.fire_at_target(player, Command(2, yaw=0, pitch=0))
        self.assertEqual((player.career[SOLDIER]['shots'], player.career[SOLDIER]['shots_hit']), (2, 1))
        self.assertEqual(board.value(player.career, SOLDIER, board.ACCURACY), 0.5)


    def test_healing_done_is_the_health_a_players_healing_gave_back(self):
        match, player = _match()
        player.health = player.max_health - 60
        match._heal(player, 50, player)
        match._heal(player, 500, player)  # only the 10 that were missing
        match._heal(player, 50)  # nobody's: a bot's healing
        self.assertEqual(player.career[SOLDIER]['healing'], 60)
        self.assertEqual(board.value(player.career, SOLDIER, STAT | 0x1D1), 60.0)
        self.assertEqual(player.fire.score, 12.0)  # a point per 5 health
        self.assertIn(STAT | 0x1D1, board.component(player.career, RANGE, SOLDIER)[5:13])  # a medal stat of the Range


class BoardOpensTests(unittest.TestCase):
    """The client lets Tab open the board only while a mode graph's state posts game message 06DF.025; the slots are
    checked against the install by tools/assets/test_board_modes.py."""

    def frames(self, match, send):
        with mock.patch.object(M, 'remote_full_frame', wraps=M.remote_full_frame) as frame:
            send()
        return [(call.args[0][0], oracle.read_full(M.remote_full_frame(*call.args)).instances[1])
                for call in frame.call_args_list]

    def on(self, instance, read, slot):
        """Whether the frame turns the mode graph's remote slot on, as the client reads it."""
        state = oracle._graph(instance.graph.index).remote_node(slot).state
        return slot in instance.active and read.states[state][0]

    def test_the_practice_range(self):
        match, player = _match()
        (instance, read), = self.frames(match, lambda: match.send_controller(player))
        self.assertEqual(instance.graph.index, 0x13C0)
        self.assertTrue(self.on(instance, read, M.PRACTICE_BOARD))

    def test_a_mode_with_no_frame_of_ours_gets_its_mode_graph_for_it(self):
        for mode, (graph, slot) in M.BOARD_MODES.items():  # Skirmish, Control, Deathmatch, Elimination, ...
            with self.subTest(mode=f'{mode:#018x}'):
                match, player = _match()
                match.game_map = dataclasses.replace(match.game_map, mode_guid=mode)
                match.assemble_ends = 30.0
                (instance, read), = self.frames(match, lambda: match._send_countdown(player, 1.0))
                self.assertEqual(instance.graph.index, graph.index)
                self.assertTrue(self.on(instance, read, slot))

    def test_each_phase_of_the_objective_modes(self):
        match, player = _match()
        match.game_map = dataclasses.replace(match.game_map, mode_guid=M.ASSAULT)
        match.assemble_ends = 30.0
        for now, assembled, phase in ((1.0, False, M.PHASE_ASSEMBLE), (31.0, True, M.PHASE_SETUP),
                                      (31.0 + M.SETUP_SECONDS, True, M.PHASE_ATTACK)):
            match.assembled = assembled
            instance, read = self.frames(match, lambda: match._send_countdown(player, now))[0]
            self.assertEqual(instance.graph.index, 0x0F4A)
            self.assertTrue(self.on(instance, read, phase), phase)
            self.assertTrue(self.on(instance, read, M.PHASE_BOARD[phase]), phase)

    def test_a_control_map_loads_with_its_three_arenas_points_in_order(self):
        # its points are keyed "stage", not "objective": the lobby raised KeyError making any match on one
        ilios = practice.pick_map('Ilios', '0x0230000000000017')
        self.assertEqual((ilios.mode_guid, len(ilios.objectives)), (0x0230000000000017, 3))
        self.assertIsNotNone(practice.pick_map('Ilios', '0x0230000000000010'))  # Skirmish there too

    def test_a_mode_without_a_checked_mode_graph_keeps_its_plain_frame(self):
        match, player = _match()
        match.game_map = dataclasses.replace(match.game_map, mode_guid=0x0230000000000067)  # its graph adds a header
        match.assemble_ends = 30.0
        self.assertEqual(self.frames(match, lambda: match._send_countdown(player, 1.0)), [])


if __name__ == '__main__':
    unittest.main()
