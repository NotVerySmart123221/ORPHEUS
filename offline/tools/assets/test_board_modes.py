import os
import sys
import unittest
from pathlib import Path

INSTALL = Path(os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))


@unittest.skipUnless(INSTALL.exists(), 'needs the retail install')
class BoardModesTests(unittest.TestCase):
    def test_the_states_the_lobby_turns_on_post_the_message_that_opens_the_tab_board(self):
        # each mode's graph by its mode asset, laid out as the lobby's Graph(...), with 06DF.025 posted from the
        # remote slots match.py turns on (research/lizrollin-runtime.md section 22)
        from board_modes import board
        from retail_bundle_lookup import load_storage
        sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lobby'))
        import practice  # noqa: F401  (puts vendor/ on the path)
        from ow174.game import match as M
        storage = load_storage(str(INSTALL))
        cases = [(0x0230000000000018, M.PRACTICE_MODE, {M.PRACTICE_BOARD}),
                 (M.ASSAULT, M.OBJECTIVE_MODE, set(M.PHASE_BOARD.values())),
                 *((mode, graph, {slot}) for mode, (graph, slot) in M.BOARD_MODES.items()),
                 *((mode, graph, {M.GATE_BOARD[graph.index]}) for mode, (graph, _) in M.LOADING_GATES.items())]
        for mode, graph, slots in cases:
            with self.subTest(mode=f'{mode:#018x}'):
                guid, info, posting = board(storage, mode)
                self.assertEqual(guid & 0xFFFF, graph.index)
                self.assertEqual((info['owner_states'], info['instance_vars'], info['remote_slots'], frozenset(info['silent'])),
                                 (graph.owner_states, graph.sync_vars, graph.remote_states, graph.silent))
                self.assertLessEqual(slots, set(posting))

    def test_a_mode_whose_graph_never_posts_it_has_no_board(self):
        from board_modes import board
        from retail_bundle_lookup import load_storage
        graph, _, posting = board(load_storage(str(INSTALL)), 0x0230000000000019)
        self.assertEqual((graph & 0xFFFF, posting), (0x13A3, []))


if __name__ == '__main__':
    unittest.main()
