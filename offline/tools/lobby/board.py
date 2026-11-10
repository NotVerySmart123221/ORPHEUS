"""The Tab board's own numbers: component 64 of the player's own entity.

The layout and the stat lists are LizRollin's ow174/game/stats.py and onfire.py @ 9f8f3bc (MIT, see
vendor/README.md), who could not check them: their board did not open. Ours opens (match.PRACTICE_BOARD), and the
layout was checked on it (research/lizrollin-runtime.md section 22).

- Fields 5-12 name 8 stats (062 GUIDs) with their values in 21-28 and a medal each in 37-44 (1 gold, 2 silver,
  3 bronze, 0 none); fields 13-18 name 6 more with their values in 29-34. A GUID of 0 is no entry. The client
  prints each stat's name and its value by the stat's own display type.
- Which stats: vendor/data/game_stats_174.json (their tools/extract_game_stats.py, from the game data): the mode's
  player stats in the medal slots, the hero's own in the other six.
- The values are what Match counts for the Career Profile (Player.career, career.TOTALS), and Weapon Accuracy from
  the primary's shots and hits, and Healing Done from the health the player's healing gave back. A stat nothing
  counts (objective kills and time, the hero abilities' own) shows 0.
- Fields 19, 20 and 36 are On Fire's: the score, its threshold and the flag that burns the portrait (onfire.py).
- Medals: gold for a counted stat above 0 that the data does not bar from medals (Deaths). shortcut: retail ranked
  the stat among teammates, and bots are not ranked here; rank them when a second player or scoring bots exist.
"""
import json
from functools import cache

import onfire
from career import STAT, TOTALS
from ow174.paths import DATA_DIR

GMP_STATS = 64
MEDAL_STAT, OTHER_STAT, MEDAL_VALUE, OTHER_VALUE, MEDAL = 5, 13, 21, 29, 37  # the first field of each run
MEDAL_SLOTS, OTHER_SLOTS = 8, 6
FIRE_SCORE, FIRE_THRESHOLD, ON_FIRE = 19, 20, 36
FIELDS = 45
GOLD = 1
# stat -> (Player.count's counter, 0 the hero's own or 1 every hero's in this match)
COUNTERS = {STAT | ids[level]: (name, level) for name, ids in TOTALS.items() for level in (0, 1)}
COUNTERS[STAT | 0x223] = ('kill_streak', 0)  # Kill Streak - Best
COUNTERS[STAT | 0x1D1] = ('healing', 1)  # Healing Done (Match._heal): the health a player's healing gave back
ACCURACY = STAT | 0x2F  # Weapon Accuracy, a fraction: the primary's shots that hit (one ray a shot, as match.py fires)


@cache
def _data() -> dict:
    return json.loads((DATA_DIR / 'game_stats_174.json').read_text(encoding='utf-8'))


def _pick(lists, mode: int) -> list:
    """[(stat, no medal)] of a "default" and "modes" pair for a game mode; [] without one (an unknown hero)."""
    if not lists:
        return []
    return [(int(stat, 16), bool(barred)) for stat, barred in lists['modes'].get(f'0x{mode:016X}', lists['default'])]


def value(counted: dict, hero: int, stat: int) -> float:
    """The stat's number from a match's counters (Player.career: hero -> {counter: amount})."""
    if stat == ACCURACY:
        own = counted.get(hero, {})
        return own.get('shots_hit', 0) / own['shots'] if own.get('shots') else 0.0
    name, every = COUNTERS.get(stat, (None, 0))
    if name is None:
        return 0.0
    return float(sum(c.get(name, 0) for c in counted.values()) if every else counted.get(hero, {}).get(name, 0))


def component(counted: dict, mode: int, hero: int | None, fire=None) -> list:
    """Component 64's board fields. Every slot is written (GUID 0 past a list's end), so a hero switch clears the
    old hero's stats. hero None: no hero yet, no hero stats. fire: the player's onfire.Fire, for the On Fire fields
    (the score in whole points: it drains all the time)."""
    data = _data()
    values = [None] * FIELDS
    if fire is not None:
        values[FIRE_SCORE], values[FIRE_THRESHOLD], values[ON_FIRE] = float(round(fire.score)), onfire.THRESHOLD, int(fire.burning)
    medal_stats = _pick(data['player'], mode)[:MEDAL_SLOTS]
    hero_stats = _pick(data['heroes'].get(f'0x{hero:016X}') if hero else None, mode)[:OTHER_SLOTS]
    for entries, slots, stat_field, value_field in ((medal_stats, MEDAL_SLOTS, MEDAL_STAT, MEDAL_VALUE),
                                                    (hero_stats, OTHER_SLOTS, OTHER_STAT, OTHER_VALUE)):
        for k in range(slots):
            stat = entries[k][0] if k < len(entries) else 0
            values[stat_field + k] = stat
            values[value_field + k] = value(counted, hero, stat) if stat else 0.0
    for k in range(MEDAL_SLOTS):
        barred = k >= len(medal_stats) or medal_stats[k][1]
        values[MEDAL + k] = 0 if barred or values[MEDAL_VALUE + k] <= 0 else GOLD
    return values
