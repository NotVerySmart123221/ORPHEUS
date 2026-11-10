import os
import unittest
from pathlib import Path

INSTALL = Path(os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))


@unittest.skipUnless(INSTALL.exists(), 'needs the retail install')
class GraphEdgesTests(unittest.TestCase):
    def test_zarya_fire_button_chain(self):
        from graph_edges import edges
        from retail_bundle_lookup import find_asset_payload, load_storage
        blob = find_asset_payload(load_storage(str(INSTALL)), 0x0580000000000241).payload
        links = {(source, dest) for source, _, dest in edges(blob)}
        self.assertEqual(len(edges(blob)), 172)
        # button 5 -> Stack 40 -> Stack 17 -> state 85 -> TrackRay 14 (research/bot-fire.md)
        self.assertTrue({(311, 317), (317, 323), (323, 578), (578, 582)} <= links)


if __name__ == '__main__':
    unittest.main()
