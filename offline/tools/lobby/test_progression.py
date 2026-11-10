"""Earned-account behavior through real SQLite transactions."""
from pathlib import Path
from contextlib import closing
import sqlite3
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor

from progression import Account, xp_needed


HERO = 0x02E000000000006E
DEFAULT = '0250000000000001'
COMMON = '0250000000000002'
RARE = '0250000000000003'
CURRENCY = '0250000000000004'


def catalog():
    return {'version': 2, 'items': {
        DEFAULT: {'hero': HERO, 'heroes': [HERO], 'rarity': 0, 'slot': 'Skin', 'boxes': [], 'default': True},
        COMMON: {'hero': HERO, 'heroes': [HERO], 'rarity': 0, 'slot': 'Skin', 'boxes': [0], 'price': 25},
        RARE: {'hero': HERO, 'heroes': [HERO], 'rarity': 1, 'slot': 'Skin', 'boxes': [0], 'price': 75},
        '0250000000000918': {'hero': 0, 'heroes': [], 'rarity': 0, 'slot': None, 'boxes': []},
        '0250000000000919': {'hero': 0, 'heroes': [], 'rarity': 0, 'slot': None, 'boxes': []},
    }, 'frames': [[1, '0250000000000918'], [11, '0250000000000919']]}


class ProgressionTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.addCleanup(self.folder.cleanup)
        self.path = Path(self.folder.name) / 'account.sqlite3'
        self.account = Account(self.path, catalog())

    def test_new_account_owns_defaults_and_persists_multiple_levels_once(self):
        self.assertEqual(self.account.snapshot()['owned'], [DEFAULT])
        result = self.account.settle('match-1', 14400, True, '2026-11-08')
        state = Account(self.path, catalog()).snapshot()
        self.assertEqual(state['level'], 3)
        self.assertEqual(len(state['boxes']), 2)
        self.assertEqual(state['xp'], result['xp'] - 2000 - 20000)
        self.assertEqual(xp_needed(101), 20000)
        self.assertIsNone(self.account.settle('match-1', 14400, True, '2026-11-08'))
        self.assertEqual(self.account.snapshot(), state)

    def test_first_win_bonus_applies_once_per_utc_day(self):
        first = self.account.settle('first', 120, True, '2026-11-08')
        second = self.account.settle('second', 120, True, '2026-11-08')
        third = self.account.settle('third', 120, True, '2026-11-09')
        earlier = self.account.settle('earlier-clock', 120, True, '2026-11-08')
        self.assertEqual(first['xp'] - second['xp'], 1500)
        self.assertEqual(third['xp'], first['xp'])
        self.assertEqual(earlier['xp'], second['xp'])

    def test_open_is_committed_before_reply_and_replay_never_pays_twice(self):
        self.account.settle('earned', 120, True, '2026-11-08')
        box = self.account.snapshot()['boxes'][0]['id']
        result = self.account.open_box(box)
        self.assertEqual(len(result), 4)
        self.assertTrue(any(self.account.items[r['item']]['rarity'] >= 1 for r in result))
        state = self.account.snapshot()
        self.assertEqual(state['boxes'], [])
        self.assertIn(COMMON, state['owned'])
        self.assertIn(RARE, state['owned'])
        reopened = Account(self.path, catalog())
        self.assertEqual(reopened.open_box(box), result)
        self.assertEqual(reopened.snapshot(), state)
        self.assertIsNone(reopened.open_box('0' * 32))

    def test_a_box_rolls_each_slot_by_the_slot_odds_with_one_rare_or_better(self):
        import progression
        from unittest.mock import patch
        common, rare, epic, legendary = progression.SLOT_ODDS
        self.assertAlmostEqual(common + rare + epic + legendary, 100)
        self.assertAlmostEqual(1 - (1 - epic / 100) ** 4, 0.185, places=3)  # a box with an Epic
        self.assertAlmostEqual(1 - (1 - legendary / 100) ** 4, 0.075, places=3)  # with a Legendary
        data = catalog()
        for rarity in range(4):
            for n in range(4):
                data['items'][f'02500000000010{rarity}{n}'] = {'hero': HERO, 'heroes': [HERO], 'rarity': rarity,
                                                                'slot': 'Skin', 'boxes': [0]}
        account = Account(Path(self.folder.name) / 'odds.sqlite3', data)
        account.settle('earned', 120, True, '2026-11-08')
        rolls = []

        class Rng(progression.random.Random):
            def choices(self, population, weights=None, **kwargs):
                rolls.append(dict(zip(population, weights)))
                return super().choices(population, weights=weights, **kwargs)

        with patch.object(progression.random, 'SystemRandom', Rng):
            account.open_box(account.snapshot()['boxes'][0]['id'])
        self.assertEqual(rolls[0], {1: common + rare, 2: epic, 3: legendary})  # the Rare-or-better slot
        self.assertEqual(rolls[1:], [dict(enumerate(progression.SLOT_ODDS))] * 3)

    def test_currency_drops_pay_the_asset_amount_instead_of_becoming_owned(self):
        data = catalog()
        data['items'] = {DEFAULT: data['items'][DEFAULT], CURRENCY: {
            'hero': 0, 'heroes': [], 'rarity': 1, 'slot': None, 'boxes': [0], 'currency': 150}}
        data['frames'] = []
        account = Account(Path(self.folder.name) / 'currency.sqlite3', data)
        account.settle('earned', 120, True, '2026-11-08')
        result = account.open_box(account.snapshot()['boxes'][0]['id'])
        self.assertEqual(account.snapshot()['credits'], 600)
        self.assertNotIn(CURRENCY, account.snapshot()['owned'])
        self.assertFalse(any(r['duplicate'] for r in result))

    def test_purchase_and_equip_validate_ownership_membership_and_slots(self):
        with self.assertRaises(ValueError):
            self.account.purchase(COMMON)
        with self.assertRaises(ValueError):
            self.account.equip(HERO, COMMON, 0)
        self.account.settle('earned', 120, True, '2026-11-08')
        self.account.open_box(self.account.snapshot()['boxes'][0]['id'])
        self.account.equip(HERO, RARE, 0)
        self.assertEqual(self.account.snapshot()['equips'], [[HERO, int(RARE, 16), 0, 'Skin']])
        for hero, item, index in ((HERO + 1, RARE, 0), (HERO, RARE, 4), (HERO, '025000000000FFFF', 0)):
            with self.assertRaises(ValueError):
                self.account.equip(hero, item, index)

    def test_successful_purchase_charges_once_and_survives_reopen(self):
        sale = '0250000000000005'
        data = catalog()
        data['items'][sale] = {**data['items'][COMMON], 'boxes': [], 'price': 5}
        account = Account(Path(self.folder.name) / 'shop.sqlite3', data)
        account.settle('earned', 120, True, '2026-11-08')
        account.open_box(account.snapshot()['boxes'][0]['id'])
        before = account.snapshot()['credits']
        self.assertGreaterEqual(before, 5)
        self.assertTrue(account.purchase(sale))
        reopened = Account(account.path, data)
        self.assertFalse(reopened.purchase(sale))
        self.assertEqual(reopened.snapshot()['credits'], before - 5)

    def test_invalid_input_and_corrupt_database_are_not_reset(self):
        for seconds in (-1, float('nan'), float('inf'), 14401):
            with self.assertRaises(ValueError):
                self.account.settle('bad', seconds, True, '2026-11-08')
        original = self.path.read_bytes()
        broken = Path(self.folder.name) / 'broken.sqlite3'
        broken.write_bytes(b'not a database')
        with self.assertRaises(sqlite3.DatabaseError):
            Account(broken, catalog())
        self.assertEqual(broken.read_bytes(), b'not a database')
        self.assertEqual(self.path.read_bytes(), original)

    def test_existing_database_with_missing_account_is_never_reseeded(self):
        with closing(sqlite3.connect(self.path)) as db, db:
            db.execute('DELETE FROM account')
        with self.assertRaisesRegex(ValueError, 'missing account'):
            Account(self.path, catalog())
        with closing(sqlite3.connect(self.path)) as db:
            self.assertEqual(db.execute('SELECT count(*) FROM account').fetchone()[0], 0)

    def test_failed_save_rolls_back_consumption_rewards_and_receipt(self):
        self.account.settle('earned', 120, True, '2026-11-08')
        before = self.account.snapshot()
        box = before['boxes'][0]['id']
        with closing(sqlite3.connect(self.path)) as db, db:
            db.execute("CREATE TRIGGER fail_save BEFORE UPDATE ON account BEGIN SELECT RAISE(ABORT,'save failed'); END")
        with self.assertRaises(sqlite3.IntegrityError):
            self.account.open_box(box)
        self.assertEqual(self.account.snapshot(), before)
        with closing(sqlite3.connect(self.path)) as db, db:
            self.assertEqual(db.execute("SELECT count(*) FROM receipts WHERE kind='box'").fetchone()[0], 0)
            db.execute('DROP TRIGGER fail_save')
        self.assertEqual(len(self.account.open_box(box)), 4)

    def test_concurrent_opens_replay_one_committed_result(self):
        self.account.settle('earned', 120, True, '2026-11-08')
        box = self.account.snapshot()['boxes'][0]['id']
        with ThreadPoolExecutor(max_workers=2) as workers:
            results = list(workers.map(self.account.open_box, [box, box]))
        self.assertEqual(results[0], results[1])
        self.assertEqual(self.account.snapshot()['boxes'], [])


if __name__ == '__main__':
    unittest.main()
