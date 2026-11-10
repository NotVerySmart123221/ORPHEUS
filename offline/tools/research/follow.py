"""How the client follows the server's copy of the hero after each ability press of a mover_record.py --observe
recording (owner corrections on).

    python tools/research/follow.py REC [--button shift|e|rmb|jump] [--frames N] [--near METRES]

Per press: how far the server's copy went in the N frames after it (default 300), the most the client's feet were off
the server's and when, the frame from which they stay within METRES of each other (default 0.05), and how far apart
they are at the end. An ability the server alone moves (a dash, a launch) shows the client behind at first and
together again a few records later.
"""
import argparse
import math
from pathlib import Path

from mover_replay import read

BUTTONS = {'shift': 0x08, 'e': 0x10, 'rmb': 0x02, 'jump': 0x40}


def presses(commands, button):
    order = sorted(commands)
    return [frame for before, frame in zip(order, order[1:]) if commands[frame][4] & button and not commands[before][4] & button]


def follow(client, server, press, frames, near):
    """(metres the server went, most apart, frames after the press of that, frames until together, apart at the end)."""
    common = [f for f in range(press, press + frames + 1) if f in client and f in server]
    apart = {f: math.dist(client[f], server[f]) for f in common}
    worst = max(apart, key=apart.get)
    far = [f for f in common if apart[f] > near]
    together = far[-1] + 1 - press if far else 0
    went = sum(math.dist(server[a], server[b]) for a, b in zip(common, common[1:]))
    return went, apart[worst], worst - press, together, apart[common[-1]]


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('rec', type=Path)
    parser.add_argument('--button', choices=BUTTONS, default='shift')
    parser.add_argument('--frames', type=int, default=300)
    parser.add_argument('--near', type=float, default=0.05)
    args = parser.parse_args()
    commands, client, server = read(args.rec)
    for press in presses(commands, BUTTONS[args.button]):
        went, most, at, together, end = follow(client, server, press, args.frames, args.near)
        forward, right, _, pitch, _, seconds = commands[press]
        print(f'press {press} (forward {forward}, right {right}, pitch {pitch * 180 / 32768:.1f}): the server went {went:.2f} m; '
              f'the client was at most {most:.2f} m off ({at * seconds:.2f} s after), within {args.near} m from '
              f'{together * seconds:.2f} s on, {end:.2f} m off at the end')


if __name__ == '__main__':
    main()
