import logging
import unittest
from pathlib import Path

import mover_mods

MERCY = Path(__file__).resolve().parents[2] / 'research' / 'lizrollin-runtime' / 'abilities' / 'mercy.rec.gz'


class MoverModsTests(unittest.TestCase):
    def test_a_held_jump_on_a_kits_body_follows_the_plain_mover_not_the_vms_mods(self):
        # Mercy's jump held in the air (frames 9907-10288): the VM turns Angelic Descent on (0x0071 state 1), which a
        # kits body's client does not run (research/lizrollin-runtime.md section 21)
        logging.disable(logging.CRITICAL)
        self.addCleanup(logging.disable, logging.NOTSET)
        commands, client = mover_mods.read(MERCY)
        commands = {frame: command for frame, command in commands.items() if 9860 <= frame <= 10330}
        client = {frame: feet for frame, feet in client.items() if frame in commands}
        cuts = mover_mods.stretches(commands, client)
        hero = next(h for h in mover_mods.heroes().values() if h.name == 'Mercy')
        (jump,) = (row for row in mover_mods.replay(hero, commands, client, set(cuts)) if 'jump' in row['buttons'])
        self.assertIn('0071/1', jump['states'])
        self.assertLess(max(jump['off']['plain']), 0.05)
        self.assertGreater(jump['off']['vm'][0], 1.0)


if __name__ == '__main__':
    unittest.main()
