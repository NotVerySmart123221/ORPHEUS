"""Experiment: feed the Career Profile's Overview and Statistics tabs (research/lizrollin-lobby.md section 3).

    python tools/research/career_live.py launch GAME_ROOT [TIMEOUT]   run.py's launch with this file as the lobby

The lobby sends the stat catalogue 27202 at login and answers the profile request (22206) with stats in 20807.
The catalogue and the layout of 27202 are read from LizRollin's copy (lizrollin-lobby-server/data), which the
experiment needs beside the repository; nothing of theirs is run.

The stats: All Heroes and Soldier: 76 in the categories 0, 1, 2 and 5. A stat's value is its place in the hero's
list plus 1000 times the category key, so a screenshot shows which list entry and which key a number came from.

CAREER_CATALOG=0     no 27202 at all (the profile stays empty)
CAREER_CATALOG=bare  27202 with the stats but an empty catalogue (the profile stays empty)
CAREER_VALUES=id     a stat's value is its id's low 16 bits, in category 1 only: a row then names its stat id
"""
import json
import os
import runpy
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LOBBY_DIR = ROOT / 'tools' / 'lobby'
THEIRS = ROOT / 'lizrollin-lobby-server' / 'data'
MODE_RULES = 0xB9DCA497  # 27202: the stat catalogue per hero and map, and the player's stats
ALL, SOLDIER = 0x02E00000FFFFFFFF, 0x02E000000000006E  # "All Heroes" is a hero id of its own
KEYS = (0, 1, 2, 5)  # stat categories: 1 Quick Play, 5 Game Browser; 0 and 2 only count toward All Modes


def lobby(args):
    sys.path.insert(0, str(LOBBY_DIR))
    import practice
    from practice import LOBBY, PracticeRange
    layouts = json.loads((THEIRS / 'schemas_174.json').read_text(encoding='utf-8'))
    templates = json.loads((THEIRS / 'retail_templates.json').read_text(encoding='utf-8'))
    catalog = next(value for _, msg_id, value in templates['server'] if int(msg_id) == 27202)
    listing = {entry['+0x0']: entry['+0x8'] for entry in catalog['+0x90']}
    mode = os.environ.get('CAREER_CATALOG', '1')

    by_id = os.environ.get('CAREER_VALUES') == 'id'

    def stats():
        def hero(guid, key):
            return {'+0x0': [{'+0x0': stat, '+0x8': float(stat & 0xFFFF if by_id else place + 1 + 1000 * key)}
                             for place, stat in enumerate(listing[guid])], '+0x18': guid}
        return [{'+0x0': [hero(ALL, key), hero(SOLDIER, key)], '+0x18': key} for key in ((1,) if by_id else KEYS)]

    init, leaver = PracticeRange.__init__, PracticeRange.leaver_status

    def new_init(self, *a, **k):
        init(self, *a, **k)
        group = {int(msg_id): fields for msg_id, fields in layouts[f'{MODE_RULES:08X}'].items()}
        self.schemas.groups[MODE_RULES] = group
        self.schemas._first_ids[MODE_RULES] = min(group)

    def new_leaver(self, groups):  # one of the login extras: the catalogue rides along
        out = [leaver(self, groups)]
        if MODE_RULES in groups and mode != '0':
            value = dict(catalog)
            if mode == 'bare':
                value['+0x90'], value['+0xC0'] = [], []
            value['+0x78'] = value['+0xA8'] = stats()
            out.append(self.schemas.header(MODE_RULES, groups[MODE_RULES], 27202)
                       + self.schemas.encode(MODE_RULES, 27202, value))
        practice.LOG.info('[career] 27202 %s', 'sent' if len(out) > 1 else 'not sent')
        return out

    def new_profile(self, groups, payload):
        if LOBBY not in groups:
            return None
        _, body = self.schemas.split(practice.SOCIAL_OUT, payload)
        request = self.schemas.decode(practice.SOCIAL_OUT, 22206, body)
        if self.account:
            self.hero_catalog(groups, self.hero_table)
        profile = {'+0x0': self.account_record(), '+0x80': self.catalog_heroes, '+0x100': practice.ACCOUNT,
                   '+0x138': self.name, '+0xB0': stats(), '+0xC8': stats()}
        practice.LOG.info('[career] profile answer with stats')
        return (self.schemas.header(LOBBY, groups[LOBBY], 20807)
                + self.schemas.encode(LOBBY, 20807, {'+0x78': request['+0x78'], '+0x90': profile}))

    PracticeRange.__init__, PracticeRange.leaver_status, PracticeRange.career_profile = new_init, new_leaver, new_profile
    sys.argv = [str(LOBBY_DIR / 'lobby.py'), *args]
    runpy.run_path(str(LOBBY_DIR / 'lobby.py'), run_name='__main__')


def launch(game_root, timeout):
    os.chdir(ROOT)
    sys.path.insert(0, str(LOBBY_DIR))
    import run
    command = [sys.executable, '-u', __file__, '--lobby', '--event', 'none', '--game-install', str(game_root),
               '--name', 'Player', '--retail-menu']
    return run.launch(game_root, timeout, server_command=command, plugins=['tools/asi/build/ow_fixes.asi'])


if __name__ == '__main__':
    if sys.argv[1] == '--lobby':
        lobby(sys.argv[2:])
    else:
        sys.exit(launch(Path(sys.argv[2]), int(sys.argv[3]) if len(sys.argv) > 3 else 600))
