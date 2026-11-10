"""career.py: stats counted in a match, kept in a file, and sent the way the Career Profile reads them."""
from pathlib import Path
import tempfile
import unittest

import practice
from career import ALL_HEROES, QUICK_PLAY, STAT, Career
from practice import LOBBY, MODE_RULES, SOCIAL_OUT, PracticeRange
from ow174.game.content import PRACTICE_RANGE, SOLDIER
from target import TrainingTarget
from test_target import _match

REAPER = 0x02E0000000000002


def by_stat(category, hero):
    """{stat index: value} of one hero in a category of Career.categories()."""
    entry, = (e for e in category['+0x0'] if e['+0x18'] == hero)
    return {stat['+0x0'] ^ STAT: stat['+0x8'] for stat in entry['+0x0']}


class CareerTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.addCleanup(self.folder.cleanup)
        self.path = Path(self.folder.name) / 'career.json'

    def test_a_match_counts_for_each_hero_and_once_for_all_heroes(self):
        career = Career(self.path)
        career.add_match(QUICK_PLAY, {SOLDIER: {'time': 300, 'eliminations': 7, 'final_blows': 7, 'damage': 1500,
                                                'deaths': 2, 'kill_streak': 4},
                                      REAPER: {'time': 120, 'eliminations': 3, 'kill_streak': 3}, 0x123: {}}, won=True)
        career.add_match(QUICK_PLAY, {SOLDIER: {'time': 60, 'eliminations': 2, 'kill_streak': 2}}, won=False)
        career.add_match(QUICK_PLAY, {SOLDIER: {'time': 30, 'eliminations': 9}})  # left early: no game counted
        category, = Career(self.path).categories()  # read back from the file
        self.assertEqual(category['+0x18'], QUICK_PLAY)
        self.assertEqual({e['+0x18'] for e in category['+0x0']}, {SOLDIER, REAPER, ALL_HEROES})
        soldier, everyone = by_stat(category, SOLDIER), by_stat(category, ALL_HEROES)
        # a hero's ids: Time Played, Games Played / Won / Lost, Eliminations and its best game, Deaths, Best Kill Streak
        self.assertEqual([soldier[i] for i in (0x21, 0x38, 0x39, 0x430, 0x1F, 0x2DA, 0x2A, 0x223)],
                         [390, 2, 1, 1, 18, 9, 2, 4])
        # All Heroes' own ids: one game per match, and a best over the whole game (7 + 3 eliminations)
        self.assertEqual([everyone[i] for i in (0x26, 0x385, 0x3F5, 0x42E, 0x25, 0x1B9, 0x29, 0x497, 0x472)],
                         [510, 2, 1, 1, 21, 10, 2, 4, 4])
        self.assertEqual(by_stat(category, REAPER)[0x38], 1)  # only the game Reaper was played in

    def test_the_catalogue_lists_exactly_the_stats_sent(self):
        career = Career(self.path)
        played = {name: 1 for name in ('time', 'eliminations', 'final_blows', 'deaths', 'damage', 'hero_damage', 'kill_streak')}
        career.add_match(QUICK_PLAY, {SOLDIER: played}, won=True)
        career.add_match(QUICK_PLAY, {SOLDIER: {'time': 1}}, won=False)
        listed = {entry['+0x0']: {stat ^ STAT for stat in entry['+0x8']} for entry in Career.catalogue([SOLDIER, REAPER])}
        category, = career.categories()
        self.assertEqual(set(listed), {SOLDIER, REAPER, ALL_HEROES})
        self.assertEqual(set(by_stat(category, SOLDIER)), listed[SOLDIER])
        self.assertEqual(set(by_stat(category, ALL_HEROES)), listed[ALL_HEROES])
        self.assertFalse(listed[SOLDIER] & listed[ALL_HEROES])  # the two levels share no id

    def test_a_damaged_file_starts_an_empty_career(self):
        self.path.write_text('{"1": {"zz": 5}}', encoding='utf-8')
        self.assertEqual(Career(self.path).categories(), [])

    def test_the_match_counts_what_the_player_does(self):
        match, player = _match()
        bot = TrainingTarget((0, 0, 8), 180, hero=REAPER)
        match.bots = [bot]
        match.update(10.0, 100)
        match.update(12.5, 300)  # 2.5 s on the hero
        match._damage_bot(player, bot, 120, 12.5)
        match._damage_bot(player, bot, 500, 12.6)  # the rest of its 200: the kill
        counted = player.career[SOLDIER]
        self.assertEqual((counted['time'], counted['damage'], counted['hero_damage']), (2.5, 200, 200))
        self.assertEqual((counted['eliminations'], counted['final_blows'], counted['kill_streak']), (1, 1, 1))
        match._damage_player(player, 9999, 13.0)
        self.assertEqual((counted['deaths'], player.streak), (1, 0))
        bot.health = bot.max_health
        match._damage_bot(player, bot, 500, 14.0)
        self.assertEqual((counted['eliminations'], counted['kill_streak']), (2, 1))  # the death ended the streak

    def test_a_finished_match_reaches_the_profile_and_the_range_counts_nothing(self):
        groups = {SOCIAL_OUT: 41, LOBBY: 20, MODE_RULES: 30}
        with PracticeRange(port=0, equips=self.path.with_name('equips.json')) as lobby:
            match, player = _match()
            player.career = {SOLDIER: {'time': 50, 'eliminations': 4}}
            lobby.count_career(match, player, True)  # _match() is the Practice Range
            self.assertEqual((lobby.career.categories(), player.career), ([], {}))
            match.game_map = practice.pick_map('Hanamura', '0x0230000000000014')
            player.career = {SOLDIER: {'time': 50, 'eliminations': 4}}
            lobby.count_career(match, player, True)
            lobby.player_left(player)  # nothing new since the finish: no second fold
            ask = {'+0x78': {'+0x0': 7, '+0x8': 8}, '+0x88': practice.ACCOUNT}
            catalogue, answer = lobby.request(lobby.schemas.header(SOCIAL_OUT, 41, 22206)
                                              + lobby.schemas.encode(SOCIAL_OUT, 22206, ask), groups)
            msg_id, body = lobby.schemas.split(MODE_RULES, catalogue)
            sent = lobby.schemas.decode(MODE_RULES, 27202, body)
            self.assertEqual((catalogue[0], msg_id, len(sent['+0x90'])), (30, 27202, 33))  # 32 heroes and All Heroes
            self.assertEqual(sent, lobby.schemas.decode(MODE_RULES, 27202,
                                                        lobby.schemas.split(MODE_RULES, lobby.career_stats(groups))[1]))
            profile = lobby.schemas.decode(LOBBY, 20807, lobby.schemas.split(LOBBY, answer)[1])['+0x90']
            self.assertEqual(profile['+0xB0'], sent['+0x78'])
            soldier = by_stat(profile['+0xB0'][0], SOLDIER)
            self.assertEqual((soldier[0x21], soldier[0x38], soldier[0x39], soldier[0x1F]), (50, 1, 1, 4))
            self.assertIsNone(lobby.career_stats({LOBBY: 20}))


if __name__ == '__main__':
    unittest.main()
