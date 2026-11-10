from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
import sqlite3
from unittest.mock import patch

from practice import GALLERY, HERO_CATALOG, PARTY, PROGRESSION, PROGRESSION_OUT, PracticeRange
from progression import Account
from test_progression import COMMON, DEFAULT, HERO, RARE, catalog


class ProgressionMessageTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.addCleanup(self.folder.cleanup)
        self.practice = PracticeRange(port=0, equips=Path(self.folder.name) / 'equips.json')
        self.practice.account = Account(Path(self.folder.name) / 'account.sqlite3', catalog())
        self.groups = {PROGRESSION: 7, PROGRESSION_OUT: 8, HERO_CATALOG: 9, GALLERY: 10, PARTY: 11}
        self.table = {f'{HERO:016X}': {'unlocks': [DEFAULT, COMMON, RARE], 'defaults': {'Skin': DEFAULT}}}
        self.practice.hero_catalog(self.groups, self.table)

    def decode(self, crc, message):
        schemas = self.practice.schemas
        msg_id, body = schemas.split(crc, message)
        return msg_id, schemas.decode(crc, msg_id, body)

    def request(self, msg_id, value):
        schemas = self.practice.schemas
        return self.practice.request(schemas.header(PROGRESSION_OUT, 8, msg_id)
            + schemas.encode(PROGRESSION_OUT, msg_id, value), self.groups)

    def test_earned_ownership_keeps_the_complete_catalog_and_prices(self):
        _, value = self.decode(HERO_CATALOG, self.practice.hero_catalog(self.groups, self.table))
        self.assertEqual([r['+0x0'] for r in value['+0x80'][0]['+0x0']], [int(DEFAULT, 16)])
        store = value['+0x98'][0]['+0x0']
        self.assertEqual([r['+0x0'] for r in store], [int(g, 16) for g in (DEFAULT, COMMON, RARE)])
        self.assertEqual([r['+0x14'] for r in store], [0, 25, 75])

    def test_only_earned_profiles_clear_native_cosmetic_ownership_override(self):
        from practice import CONFIG, UNLOCK_ALL
        _, value = self.decode(CONFIG, self.practice.menu_hero({CONFIG: 12}, HERO))
        self.assertIn({'+0x0': UNLOCK_ALL, '+0x8': '0'}, value['+0x78'])
        self.practice.account = None
        _, value = self.decode(CONFIG, self.practice.menu_hero({CONFIG: 12}, HERO))
        self.assertNotIn(UNLOCK_ALL, [entry['+0x0'] for entry in value['+0x78']])

    def test_queued_finish_updates_level_boxes_and_party_without_a_client_request(self):
        from ow174.game.match import SETUP_SECONDS
        match = SimpleNamespace(id=(1, 2), assemble_ends=0, assault=SimpleNamespace(winner=0),
            game_map=SimpleNamespace(map_guid=0x0800000000000091),
            players=[SimpleNamespace(account_lo=1, client=object(), steps_done=4, team=0, career={})])
        self.practice.match_finished(match, SETUP_SECONDS + 120)
        messages = self.practice.pending_rewards(self.groups)
        self.assertEqual(len(messages), 3)
        _, value = self.decode(PROGRESSION, messages[1])
        record = value['+0x78']
        self.assertEqual((record['+0x68'], record['+0x48'], len(record['+0x0'])), (2, 35, 1))
        _, party = self.decode(PARTY, messages[2])
        self.assertEqual(party['+0x78']['+0x0'][0]['+0x0']['+0x38'], 2)
        self.practice.match_finished(match, SETUP_SECONDS + 120)
        self.assertEqual(self.practice.pending_rewards(self.groups), [])

    def test_open_returns_matching_id_four_cards_and_durable_grants(self):
        self.practice.account.settle('earned', 120, True, '2026-11-08')
        _, value = self.decode(PROGRESSION, self.practice.progression(self.groups))
        box = value['+0x78']['+0x0'][0]['+0x0']
        messages = self.request(24201, {'+0x78': box})
        msg, value = self.decode(PROGRESSION, messages[0])
        result = value['+0x78']
        self.assertEqual((msg, result['+0x18']), (24305, box))
        self.assertEqual(len(result['+0x0']), 4)
        self.assertTrue(any(not card['+0x1C'] for card in result['+0x0']))
        self.assertEqual(self.practice.account.snapshot()['boxes'], [])
        state = self.practice.account.snapshot()
        self.request(24201, {'+0x78': box})
        self.assertEqual(self.practice.account.snapshot(), state)

    def test_unknown_box_releases_reveal_without_touching_inventory(self):
        self.practice.account.settle('earned', 120, True, '2026-11-08')
        state = self.practice.account.snapshot()
        _, value = self.decode(PROGRESSION, self.request(24201, {'+0x78': {'+0x0': [1, 2]}})[0])
        self.assertEqual(value['+0x78']['+0x0'], [])
        self.assertEqual(self.practice.account.snapshot(), state)

    def test_unowned_equip_rejected_and_buy_failure_keeps_balance(self):
        schemas = self.practice.schemas
        equip = {'+0x78': HERO, '+0x80': int(COMMON, 16), '+0x88': 0}
        self.practice.request(schemas.header(GALLERY, 10, 24500) + schemas.encode(GALLERY, 24500, equip), self.groups)
        self.assertEqual(self.practice.account.snapshot()['equips'], [])
        self.request(24203, {'+0x78': int(COMMON, 16)})
        self.assertEqual(self.practice.account.snapshot()['credits'], 0)

    def test_database_failure_retains_settlement_for_retry(self):
        settlement = ('retry-match', 120, True, '2026-11-08')
        self.practice.reward_queue.put(settlement)
        with patch.object(self.practice.account, 'settle', side_effect=sqlite3.OperationalError('busy')):
            self.assertEqual(self.practice.pending_rewards(self.groups), [])
        self.practice.reward_retry_at = 0
        self.assertTrue(self.practice.pending_rewards(self.groups))
        self.assertEqual(self.practice.account.snapshot()['level'], 2)
        self.assertEqual(self.practice.pending_rewards(self.groups), [])

    def test_sparse_wheel_equip_survives_later_slot_and_relogin(self):
        from practice import equipped
        entry = {'defaults': {'VoiceLine': [DEFAULT]}}
        saved = [(int(RARE, 16), 3, 'VoiceLine'), (int(COMMON, 16), 1, 'VoiceLine')]
        self.assertEqual(equipped(entry, saved)['VoiceLine'], [DEFAULT, COMMON, '0000000000000000', RARE])

    def test_global_and_hero_icons_update_persistent_account_and_native_cards(self):
        from lobby import IN_CONNECT
        import struct
        for hero in (0, HERO):
            with self.subTest(hero=hero):
                data = catalog()
                icon = '02500000000000F0'
                data['items'][icon] = {'hero': hero, 'heroes': [hero] if hero else [], 'slot': 'Icon',
                                      'rarity': 0, 'boxes': [], 'default': True}
                path = Path(self.folder.name) / f'icon-{hero}.sqlite3'
                self.practice.account = Account(path, data)
                self.groups[IN_CONNECT] = 12
                schemas = self.practice.schemas
                equip = {'+0x78': hero, '+0x80': int(icon, 16), '+0x88': 0}
                replies = self.practice.request(schemas.header(GALLERY, 10, 24500)
                    + schemas.encode(GALLERY, 24500, equip), self.groups)
                self.assertEqual(replies[1][:2], bytes((12, 2)))
                self.assertEqual(struct.unpack_from('<Q', replies[1], 2 + 32)[0], int(icon, 16))
                self.assertEqual(Account(path, data).snapshot()['icon'], int(icon, 16))
                _, party = self.decode(PARTY, replies[2])
                self.assertEqual(party['+0x78']['+0x0'][0]['+0x0']['+0x20'], int(icon, 16))

    def test_open_save_failure_releases_ui_and_restores_unconsumed_box(self):
        self.practice.account.settle('earned', 120, True, '2026-11-08')
        _, value = self.decode(PROGRESSION, self.practice.progression(self.groups))
        box = value['+0x78']['+0x0'][0]['+0x0']
        with patch.object(self.practice.account, 'open_box', side_effect=sqlite3.OperationalError('disk full')):
            replies = self.request(24201, {'+0x78': box})
        _, result = self.decode(PROGRESSION, replies[0])
        self.assertEqual(result['+0x78']['+0x18'], box)
        self.assertEqual(result['+0x78']['+0x0'], [])
        _, snapshot = self.decode(PROGRESSION, replies[1])
        self.assertEqual(snapshot['+0x78']['+0x0'][0]['+0x0'], box)

    def test_actual_assault_completion_queues_one_settlement(self):
        from dataclasses import replace
        from test_target import _match
        from ow174.game.match import ASSAULT, SETUP_SECONDS
        from assault import CAPTURE_SECONDS
        match, player = _match()
        match.game_map = replace(match.game_map, mode_guid=ASSAULT, objectives=((0, 0, 0),))
        match.assembled, match.assemble_ends = True, 0.0
        match.on_finish = self.practice.match_finished
        player.mover.position = [0.0, 0.0, 0.0]
        player.steps_done = 4
        match._tick_assault(SETUP_SECONDS)
        match._tick_assault(SETUP_SECONDS + CAPTURE_SECONDS)
        self.assertTrue(self.practice.pending_rewards(self.groups))
        match._tick_assault(SETUP_SECONDS + CAPTURE_SECONDS + 1)
        self.assertEqual(self.practice.pending_rewards(self.groups), [])

    def test_purchase_and_equip_save_failures_do_not_acknowledge_success_or_raise(self):
        state = self.practice.account.snapshot()
        with patch.object(self.practice.account, 'purchase', side_effect=sqlite3.OperationalError('busy')):
            replies = self.request(24203, {'+0x78': int(COMMON, 16)})
        self.assertEqual(self.decode(PROGRESSION, replies[0])[0], 24300)
        schemas = self.practice.schemas
        equip = schemas.header(GALLERY, 10, 24500) + schemas.encode(GALLERY, 24500,
            {'+0x78': HERO, '+0x80': int(DEFAULT, 16), '+0x88': 0})
        with patch.object(self.practice.account, 'equip', side_effect=sqlite3.OperationalError('busy')):
            reply = self.practice.request(equip, self.groups)
        self.assertEqual(self.decode(HERO_CATALOG, reply)[0], 24900)
        self.assertEqual(self.practice.account.snapshot(), state)

    def test_unreadable_database_after_failed_purchase_or_equip_keeps_connection_handler_alive(self):
        schemas = self.practice.schemas
        equip = schemas.header(GALLERY, 10, 24500) + schemas.encode(GALLERY, 24500,
            {'+0x78': HERO, '+0x80': int(DEFAULT, 16), '+0x88': 0})
        with patch.object(self.practice.account, 'purchase', side_effect=sqlite3.OperationalError('unavailable')), \
                patch.object(self.practice.account, 'equip', side_effect=sqlite3.OperationalError('unavailable')), \
                patch.object(self.practice.account, 'snapshot', side_effect=sqlite3.OperationalError('unavailable')):
            self.assertIsNone(self.request(24203, {'+0x78': int(COMMON, 16)}))
            self.assertIsNone(self.practice.request(equip, self.groups))

    def test_committed_purchase_and_equip_are_acknowledged_when_refresh_read_fails(self):
        with self.practice.account._change() as (_, state):
            state['credits'] = 100
        with patch.object(self.practice.account, 'snapshot', side_effect=sqlite3.OperationalError('read unavailable')):
            replies = self.request(24203, {'+0x78': int(COMMON, 16)})
        self.assertEqual(self.decode(PROGRESSION, replies[0])[0], 24306)
        self.assertEqual(self.practice.account.snapshot()['credits'], 75)
        self.assertIn(COMMON, self.practice.account.snapshot()['owned'])
        schemas = self.practice.schemas
        equip = schemas.header(GALLERY, 10, 24500) + schemas.encode(GALLERY, 24500,
            {'+0x78': HERO, '+0x80': int(COMMON, 16), '+0x88': 0})
        with patch.object(self.practice.account, 'snapshot', side_effect=sqlite3.OperationalError('read unavailable')):
            reply = self.practice.request(equip, self.groups)
        self.assertEqual(self.decode(HERO_CATALOG, reply)[0], 24902)
        self.assertEqual(self.practice.account.snapshot()['equips'], [[HERO, int(COMMON, 16), 0, 'Skin']])


if __name__ == '__main__':
    unittest.main()
