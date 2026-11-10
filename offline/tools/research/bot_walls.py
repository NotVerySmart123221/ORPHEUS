"""Headless bot-movement check: run a match's server bots for a while without a client and count every per-tick
move that crosses solid geometry at chest (1.0 m) or head (1.6 m) height, and every tick a bot stands more than
1 m above the floor or inside it. Uses the extracted collision mesh (tools/lobby/data/<index>.collision).

  python bot_walls.py                          Hanamura Assault (vs. AI), 120 s
  python bot_walls.py MAP_GUID MODE_GUID [SECONDS] [SEED]
  python bot_walls.py 0x0800000000000688 0     the Practice Range's patrols
  python bot_walls.py 0x08000000000006FE 0     Uprising's omnics (PVE card 0x29)

A player stands at its spawn and never moves, so fights happen where the bots go to meet it.
"""
import math
import random
import sys
from pathlib import Path

LOBBY = Path(__file__).resolve().parents[1] / 'lobby'
sys.path[:0] = [str(LOBBY), str(LOBBY / 'vendor')]

import collision  # noqa: E402
import nav  # noqa: E402
from ow174.game.content import PRACTICE_RANGE, find_hero, game_map  # noqa: E402
from ow174.game.match import Match  # noqa: E402

UPRISING = 0x08000000000006FE
TICK = 0.016


class _Client:
    """Swallows everything the match sends."""
    def __getattr__(self, name):
        return lambda *args, **kwargs: None


def the_map(guid, mode):
    if guid == UPRISING:
        from practice import PracticeRange
        return PracticeRange.pve_map(0x29)
    return PRACTICE_RANGE if guid == PRACTICE_RANGE.map_guid else game_map(guid, mode)


def run(guid, mode, seconds, seed=1, report=print):
    """(wall crossings, ticks off the floor, bots that moved more than 2 m) over `seconds` of the match."""
    random.seed(seed)
    match = Match(the_map(guid, mode), gameplay=True, mesh=collision.for_map(guid))
    solid = collision.Mesh.load(collision.DATA / f'{guid & 0xFFFFFFFF:04X}.collision', nav.SOLID)
    player = match.add_player(1, 'Player', find_hero('Soldier: 76'), 0, True)
    player.client, player.spawned, player.has_body, player.body_picked_at = _Client(), True, True, 0
    match.assemble_ends = 0.0
    now, tick = 1.0, 1
    match.update(now, tick)
    match._target_updates(player)
    last = {id(bot): bot.position for bot in match.bots}
    walls = floors = 0
    while now < seconds:
        now, tick = now + TICK, tick + 1
        match.update(now, tick)
        for bot in match.bots:
            a, b = last[id(bot)], bot.position
            last[id(bot)] = b
            if not bot.health or a == b or math.dist(a, b) > 1.0:  # a respawn jumps home
                continue
            if not all(solid.clear((a[0], a[1] + h, a[2]), (b[0], b[1] + h, b[2])) for h in (1.0, 1.6)):
                walls += 1
                report(f'wall {now:7.2f} {bot.entity:08X} {tuple(round(v, 2) for v in a)} -> '
                       f'{tuple(round(v, 2) for v in b)} {"strafe" if bot.goal is None else "path"}')
            if solid.ray((b[0], b[1] + 0.5, b[2]), (0.0, -1.0, 0.0), 1.5) is None:
                floors += 1
    moved = sum(1 for bot in match.bots if math.dist(bot.position, bot.home) > 2)
    return walls, floors, moved


def main(argv):
    guid = int(argv[1], 0) if len(argv) > 1 else 0x0800000000000165
    mode = int(argv[2], 0) if len(argv) > 2 else 0x0230000000000014
    seconds = float(argv[3]) if len(argv) > 3 else 120.0
    seed = int(argv[4]) if len(argv) > 4 else 1
    walls, floors, moved = run(guid, mode, seconds, seed)
    print(f'{moved} bots moved; {walls} moves through walls; {floors} ticks off the floor')
    return 1 if walls else 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
