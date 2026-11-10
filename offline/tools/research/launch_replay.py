"""Replay the Shift presses of a mover_record.py recording through the lobby's own match code, and compare the server's
copy of the hero with what the client did by itself.

    python tools/research/launch_replay.py REC HERO [--frames N] [--button shift|e|rmb|jump]

REC: an --observe recording made with OW_BODY_VM=all and OW_CORRECTIONS=0. On a VM body the client predicts the
ability itself and nothing pulls it back, so its feet are the game's own answer to the press. For each press the hero
(on a kits body, with the Practice Range's collision) is put where the client last stood still before it and fed the
recorded commands. Printed per press: how far apart the two get in the N frames after it (default 150), how far apart
they are at the end, and how far each went.
"""
import argparse
import logging
import math
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / 'tools' / 'lobby'))

import practice  # noqa: E402  (puts vendor/ on the path)
import collision  # noqa: E402
from mover_replay import read  # noqa: E402
from ow174.game.commands import Command  # noqa: E402
from ow174.game.content import PRACTICE_RANGE, heroes  # noqa: E402
from ow174.game.match import Match  # noqa: E402

BUTTONS = {'shift': 0x08, 'e': 0x10, 'rmb': 0x02, 'jump': 0x40}
LEAD = 400  # frames before a press in which the client's last still frame is looked for


class _Client:
    def queue_reliable(self, message, value):
        pass

    def queue_entities(self, updates):
        pass


def player_at(hero, world, feet):
    """A spawned player of `hero` on a kits body standing at `feet`."""
    match = Match(PRACTICE_RANGE, gameplay=True, body_setup=practice.GameServer.body_setup, world=world)
    player = match.add_player(1, 'Player', hero.guid, 0, True)
    player.client = _Client()
    player.spawned = player.has_body = True
    player.body_picked_at = 1
    match.update(3.0, 1)
    player.body_script.arrived(player.body_script.data_last)
    player.mover.position = list(feet)
    return match, player


def replay(commands, client, hero, world, press, frames):
    """{frame: the server's feet} from the client's last still frame before `press` to `frames` after it."""
    still = [f for f in range(press - LEAD, press) if f in client and client.get(f - 1) == client[f]]
    start = still[-1] if still else press - 20
    match, player = player_at(hero, world, client[start])
    feet = {}
    for tick, frame in enumerate(range(start + 1, press + frames + 1), 2):
        if frame not in commands:
            continue
        forward, right, yaw, pitch, buttons, _ = commands[frame]
        player.take_commands([Command(frame, forward, right, yaw, pitch, buttons)])
        match.update(3.0 + (frame - start) * player.quanta / 1e6, tick)
        player.body_script.arrived(player.body_script.data_last)
        feet[frame] = tuple(player.mover.position)
    return feet


def compare(path, hero, button, frames):
    commands, client, _ = read(path)
    world = collision.movement_world(PRACTICE_RANGE)
    order = sorted(commands)
    rows = []
    for before, press in zip(order, order[1:]):
        if not (commands[press][4] & button and not commands[before][4] & button) or press not in client:
            continue
        feet = replay(commands, client, hero, world, press, frames)
        common = [f for f in range(press, press + frames + 1) if f in feet and f in client]
        apart = {f: math.dist(feet[f], client[f]) for f in common}
        worst = max(apart, key=apart.get)
        rows.append((press, commands[press], apart[worst], worst - press, apart[common[-1]],
                     math.dist(client[common[-1]], client[press]), math.dist(feet[common[-1]], feet[press])))
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('rec', type=Path)
    parser.add_argument('hero')
    parser.add_argument('--frames', type=int, default=150)
    parser.add_argument('--button', choices=BUTTONS, default='shift')
    args = parser.parse_args()
    logging.disable(logging.CRITICAL)
    hero = next(h for h in heroes().values() if h.name == args.hero)
    for press, command, most, at, end, client_went, server_went in compare(args.rec, hero, BUTTONS[args.button], args.frames):
        print(f'press {press} (forward {command[0]}, right {command[1]}, pitch {command[3] * 180 / 32768:.1f}): '
              f'apart at most {most:.2f} m ({at:+d} frames), {end:.2f} m at the end; '
              f'the client went {client_went:.2f} m, the server {server_went:.2f} m')


if __name__ == '__main__':
    main()
