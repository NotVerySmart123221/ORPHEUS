import os
import unittest
from pathlib import Path

INSTALL = Path(os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))


@unittest.skipUnless(INSTALL.exists(), 'needs the retail install')
class GraphMessagesTests(unittest.TestCase):
    def test_uprising_mode_graph_messages(self):
        from graph_layout import layout
        from graph_messages import messages
        from retail_bundle_lookup import find_asset_payload, load_storage
        blob = find_asset_payload(load_storage(str(INSTALL)), 0x0580000000000D28).payload
        found = messages(blob)
        by_state = {row['state']: row['instance'] for row in layout(blob)['states']}
        # state 2 (remote slot 0) posts 6DF.025, state 14 (slot 4) waits for 74E.025 (research/uprising-mission.md)
        self.assertEqual(found[by_state[2]], 0x02400000000006DF)
        self.assertEqual(found[by_state[14]], 0x024000000000074E)
        self.assertNotIn(by_state[29], found)  # the loading marker carries no message
        self.assertEqual(len(found), 47)  # states, entries, actions and the remote action nodes


if __name__ == '__main__':
    unittest.main()
