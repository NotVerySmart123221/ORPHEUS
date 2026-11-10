import logging
import unittest

import ability_states
import kits
from ow174.game.content import heroes

SHIFT = ability_states.BUTTONS['shift']


class AbilityStatesTests(unittest.TestCase):
    def setUp(self):
        logging.disable(logging.CRITICAL)
        self.addCleanup(logging.disable, logging.NOTSET)

    def rows(self, name, graph):
        hero = next(h for h in heroes().values() if h.name == name)
        return ability_states.run(hero, SHIFT, graph, 0.1, 6.0, True)

    def test_the_kits_tables_are_what_each_graph_turns_on(self):
        # the tables in tools/lobby/kits.py are this tool's --kit output: regenerate one that differs
        for name, graph, table in (('Reaper', 0x0020, kits.WRAITH_FORM), ('Moira', 0x0F7C, kits.FADE),
                                   ('Mei', 0x05FE, kits.CRYO_FREEZE), ('Reinhardt', 0x007D, kits.CHARGE),
                                   ('Sombra', 0x0C9B, kits.STEALTH), ('Cassidy', 0x01F0, kits.COMBAT_ROLL),
                                   ('Genji', 0x01DB, kits.SWIFT_STRIKE), ('Winston', 0x010A, kits.JUMP_PACK),
                                   ('Ashe', 0x1513, kits.COACH_GUN), ('Doomfist', 0x0DD8, kits.RISING_UPPERCUT)):
            with self.subTest(hero=name):
                running, _ = ability_states.kit(self.rows(name, graph))
                self.assertEqual(tuple(running), table)

    def test_rolls_table_is_the_graphs_with_three_states_the_tool_leaves_out(self):
        running, _ = ability_states.kit(self.rows('Wrecking Ball', 0x1203))
        extra = {(0, None, 0.0, None), (14, None, 0.0, None), (18, None, 0.0, None)}  # camera, switch, pose: no data
        self.assertEqual({*running} | extra, {*kits.ROLL})

    def test_flights_table_is_the_graphs_but_for_its_speed_limit(self):
        running, _ = ability_states.kit(self.rows('Echo', 0x1BB6))
        limit = (24, None, 0.15, 0.24)  # sent later and longer, with the value it has by then (kits.FLIGHT)
        self.assertIn(limit, running)
        self.assertEqual({*running} - {limit}, {*kits.FLIGHT} - {(24, None, 0.23, 0.35)})

    def test_wraith_form_runs_three_seconds_at_half_again_the_speed(self):
        rows = {slot: (cls, on, off, text) for slot, _, cls, on, off, text, _ in self.rows('Reaper', 0x0020)}
        self.assertEqual(rows[4], ('STU_316CFEF2', 0.0, rows[4][2], 'SPEED ADD 0.5; COLLISION_MOD SET 0'))
        self.assertAlmostEqual(rows[4][2], 3.0, delta=0.01)
        self.assertEqual(rows[0][0], kits.PHASED)  # on for the same three seconds
        self.assertAlmostEqual(rows[0][2], 3.0, delta=0.01)

    def test_states_the_kits_cannot_write_are_listed_not_sent(self):
        running, left = ability_states.kit(self.rows('Mei', 0x05FE))
        sent = {slot for slot, *_ in running}
        self.assertIn((11, 'STUStatescriptStatePet', 'not a class the kits send'), left)  # the ice block: the server's
        self.assertFalse(sent & {slot for slot, *_ in left})


if __name__ == '__main__':
    unittest.main()
