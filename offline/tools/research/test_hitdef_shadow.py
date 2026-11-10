"""Our weapon tables against the 1.74 damage data saved by hitdef_shadow.py (research/damage-data): the numbers we took
from it, and the wiki numbers it confirmed, must keep matching it."""
import json
import math
import sys
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SNAPSHOT = REPO / 'research' / 'damage-data' / 'hitdef-theirs-2026-11-09.json'
sys.path.insert(0, str(REPO / 'tools' / 'lobby'))
import practice  # noqa: E402,F401  (puts vendor/ on the path)
from target import ABILITY_HITS, ALT_KIT, BASTION_SENTRY_WEAPON, PLAYER_KIT, SOLDIER_RIFLE  # noqa: E402

# hero -> (graph, state) of the Volley our table models, and which of its numbers we model alike
PRIMARY = {
    'Ana': (0x0B94, 5), 'Ashe': (0x14FC, 122), 'Bastion': (0x012E, 14), 'Cassidy': (0x01D2, 11),
    'Doomfist': (0x0D0A, 2), 'Echo': (0x19E5, 3), 'Genji': (0x0304, 4), 'Lúcio': (0x0776, 14), 'Mei': (0x05E6, 13),
    'Orisa': (0x0BD4, 24), 'Reaper': (0x0019, 6), 'Roadhog': (0x01C6, 9), 'Sombra': (0x0B1B, 6),
    'Torbjörn': (0x1683, 10), 'Tracer': (0x0016, 8), 'Widowmaker': (0x001B, 6), 'Wrecking Ball': (0x1514, 9),
    'Zenyatta': (0x0164, 14), 'D.Va pilot': (0x02B8, 5), 'Soldier: 76': (0x0254, 10), 'Baptiste': (0x1932, 6),
}
AUTOMATIC = {'Soldier: 76', 'Sombra', 'Tracer', 'Widowmaker', 'Wrecking Ball', 'Orisa', 'Mei', 'D.Va pilot', 'Bastion'}
LUMPED = {'Pharah': (0x0023, 6), 'Junkrat': (0x022B, 12), 'Sigma': (0x187F, 20)}  # impact + explosion as one hit


def volleys():
    data = json.loads(SNAPSHOT.read_text(encoding='utf-8'))
    return {(v['graph'], v['state']): v for hero in data.values() for v in hero.get('volleys', [])}


@unittest.skipUnless(SNAPSHOT.is_file(), 'needs the local damage snapshot (tools/research/hitdef_shadow.py)')
class SnapshotTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.data = volleys()

    def close(self, ours, theirs, msg=None):
        self.assertAlmostEqual(ours, theirs, places=4, msg=msg)  # the data's numbers are float32

    def check_weapon(self, weapon, volley, rate=False, label=''):
        direct = volley['direct']
        self.close(weapon.damage, direct['amount'], msg=label)
        if direct['start'] is not None:
            self.assertEqual(weapon.falloff, (direct['start'], direct['end']), label)
            self.close(weapon.far_damage, direct['amount'] * direct['floor'], msg=label)
        if volley['pellets'] and volley['pellets'] > 1:
            self.assertEqual(weapon.pellets, volley['pellets'], label)
        self.assertEqual(weapon.speed, volley['speed'], label)
        if rate:
            self.close(1 / weapon.shot_seconds, volley['rate'], msg=label)

    def test_primaries(self):
        for name, key in PRIMARY.items():
            self.check_weapon(PLAYER_KIT[name][1], self.data[key], name in AUTOMATIC, name)

    def test_projectiles_with_an_explosion_count_both_as_one_hit(self):
        for name, key in LUMPED.items():
            volley = self.data[key]
            weapon = PLAYER_KIT[name][1]
            self.close(weapon.damage, volley['direct']['amount'] + volley['blast']['amount'], msg=name)
            self.assertEqual((weapon.speed, weapon.crit), (volley['speed'], volley['direct']['critical']), name)

    def test_alternate_fires_and_the_sentry(self):
        self.check_weapon(ALT_KIT['Mei'][0], self.data[(0x05E6, 16)], label='icicle')
        self.check_weapon(ALT_KIT['Cassidy'][0], self.data[(0x01D2, 15)], True, 'fan')
        self.check_weapon(ALT_KIT['Torbjörn'][0], self.data[(0x1683, 13)], label='torbjorn alt')
        self.check_weapon(BASTION_SENTRY_WEAPON, self.data[(0x013A, 11)], True, 'sentry')

    def test_a_set_critical_multiplier_is_ours(self):
        pairs = [(SOLDIER_RIFLE, (0x0254, 10)), (PLAYER_KIT['Mei'][1], (0x05E6, 13)),
                 (ALT_KIT['Cassidy'][0], (0x01D2, 15)), (BASTION_SENTRY_WEAPON, (0x013A, 11))]
        for weapon, key in pairs:
            self.assertEqual(weapon.crit, self.data[key]['direct']['critical'], key)

    def test_abilities(self):
        cases = {  # (hero, command bit): (graph, state, copies of the Volley in one cast)
            ('Cassidy', 0x10): (0x01E9, 1, 1), ('Ana', 0x10): (0x0B97, 0, 1), ('Ana', 0x08): (0x0BDD, 6, 1),
            ('Junkrat', 0x08): (0x096A, 8, 1), ('Sigma', 0x10): (0x1BBB, 3, 1), ('Echo', 0x02): (0x1C36, 0, 6),
            ('Reinhardt', 0x10): (0x00E2, 0, 1),
        }
        for key, (graph, state, copies) in cases.items():
            hit, volley = ABILITY_HITS[key], self.data[(graph, state)]
            direct = (volley.get('direct') or {}).get('amount', 0)
            blast = volley.get('blast') or {}
            self.close(hit.direct, direct * copies, msg=key)
            self.close(hit.splash, blast.get('amount', 0) * copies, msg=key)
            if blast.get('amount') and blast.get('rings'):  # Fire Strike's empty explosion has a ring too
                self.assertEqual([tuple(round(v, 4) for v in r) for r in hit.rings],
                                 [tuple(round(v, 4) for v in r) for r in blast['rings']], key)
                self.assertEqual(hit.radius, blast['rings'][-1][0], key)
            self.assertTrue(math.isclose(hit.speed, volley['speed'], abs_tol=0.5), key)


if __name__ == '__main__':
    unittest.main()
