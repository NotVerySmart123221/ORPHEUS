"""The player's career stats: counted in matches, kept in a file, shown by the Career Profile.

The profile shows a stat only when the stat catalogue (27202 +0x90) lists it for the hero, so `catalogue` lists
exactly the stats counted here. Stats go out in categories keyed by game mode, each holding a list of stats per hero.
"All Heroes" is a hero of its own (ALL_HEROES) with stat ids of its own. The ids were read off the profile with each
stat's value set to its id (tools/research/career_live.py, research/lizrollin-lobby.md section 3).
"""
import json
import threading

ALL_HEROES = 0x02E00000FFFFFFFF
STAT = 0x0860000000000000  # a stat's GUID (type 062) from its index
# Stat category keys (client 0x7FF6D1662770): 1 Quick Play, 5 Game Browser, 3 | season << 16 a competitive season.
# A category with key 0 shows under no mode.
QUICK_PLAY = 1
# counter -> its total's stat (a hero's, All Heroes'); what Match counts per hero (Player.count)
TOTALS = {
    'time': (0x21, 0x26),  # Time Played, seconds
    'games': (0x38, 0x385),
    'wins': (0x39, 0x3F5),
    'losses': (0x430, 0x42E),
    'eliminations': (0x1F, 0x25),
    'final_blows': (0x2B, 0x2C),
    'deaths': (0x2A, 0x29),
    'damage': (0x1BF, 0xC9),  # All Damage Done
    'hero_damage': (0x4B7, 0x4B8),
}
# counter -> the stats of its best single game (the table's Best column); All Heroes' highlight reads 0x472
BESTS = {
    'eliminations': ((0x2DA,), (0x1B9,)),
    'final_blows': ((0x2DB,), (0x1E2,)),
    'damage': ((0x2A4,), (0x1E3,)),
    'hero_damage': ((0x4BB,), (0x4BA,)),
    'kill_streak': ((0x223,), (0x497, 0x472)),  # Best Kill Streak
}
BEST = 'best_'  # a stored best's name: this before its counter


def _stats(values, level):
    """A hero's stored values as the message's stat list; level 0 a hero's ids, 1 All Heroes'."""
    out = []
    for name, value in values.items():
        if name.startswith(BEST):
            ids = BESTS.get(name[len(BEST):], ((), ()))[level]
        else:
            ids = (TOTALS[name][level],) if name in TOTALS else ()  # a name this version does not know: skipped
        out += [{'+0x0': STAT | stat, '+0x8': float(value)} for stat in ids]
    return out


class Career:
    def __init__(self, path):
        self.path, self.lock = path, threading.Lock()  # counted on the game server's thread, read on the lobby's
        try:
            saved = json.loads(path.read_text(encoding='utf-8'))
            # {category key: {hero GUID: {name: value}}}; names are TOTALS counters and BEST + a BESTS counter
            self.stats = {int(key): {int(hero, 16): {name: float(value) for name, value in values.items()}
                                     for hero, values in heroes.items()} for key, heroes in saved.items()}
        except (OSError, ValueError, TypeError, AttributeError):  # none saved yet, or a damaged file: an empty career
            self.stats = {}

    def add_match(self, key, played, won=None):
        """Fold one match in: `played` is {hero GUID: {counter: amount}} as Player.count kept it. `won` None is a
        match left before its end: its numbers count, but no game. A finished game counts for each hero played and
        once for All Heroes; a best is the most in one game, for a hero and for the whole game."""
        played = {hero: counted for hero, counted in played.items() if counted}
        if not played:
            return
        whole = {}
        for counted in played.values():
            for name, amount in counted.items():
                whole[name] = max(whole.get(name, 0), amount) if name == 'kill_streak' else whole.get(name, 0) + amount
        with self.lock:
            category = self.stats.setdefault(key, {})
            for hero, counted in (*played.items(), (ALL_HEROES, whole)):
                values = category.setdefault(hero, {})
                if won is not None:
                    counted = {**counted, 'games': 1, 'wins' if won else 'losses': 1}
                for name, amount in counted.items():
                    if name in TOTALS:
                        values[name] = values.get(name, 0) + amount
                    if name in BESTS:
                        values[BEST + name] = max(values.get(BEST + name, 0), amount)
            text = json.dumps({str(key): {f'{hero:016X}': values for hero, values in heroes.items()}
                               for key, heroes in self.stats.items()})
            self.path.parent.mkdir(parents=True, exist_ok=True)
            self.path.write_text(text, encoding='utf-8')

    def categories(self):
        """The stats as 27202 (+0x78, +0xA8) and the profile 20807 (+0xB0, +0xC8) carry them."""
        with self.lock:
            return [{'+0x0': [{'+0x0': _stats(values, hero == ALL_HEROES), '+0x18': hero}
                              for hero, values in heroes.items()], '+0x18': key}
                    for key, heroes in self.stats.items()]

    @staticmethod
    def catalogue(heroes):
        """27202 +0x90: the stats each of these heroes has, and All Heroes'."""
        def ids(level):
            return [STAT | stat for stat in (*(pair[level] for pair in TOTALS.values()),
                                             *(stat for pair in BESTS.values() for stat in pair[level]))]
        return [*({'+0x0': hero, '+0x8': ids(0)} for hero in heroes), {'+0x0': ALL_HEROES, '+0x8': ids(1)}]
