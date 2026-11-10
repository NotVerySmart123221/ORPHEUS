"""Replay a mover_record.py walk through our mover and LizRollin's port, and compare each with the client's feet.

    python tools/research/mover_replay.py WALK.rec[.gz] [--lizrollin DIR] [--live]

Both start at the client's feet 40 frames before the first input, with Soldier: 76's mover data. Ours: tools/lobby's
PlayerMover (their port, vendored) on the Range cache run.py builds (tools/lobby/vendor/cache/collision). Theirs:
DIR/ow174/game/mover.py on its own cache (DIR/cache/collision, built by DIR/tools/build_collision.py): a check
that a newer pin of theirs still matches. Each runs in its own process: both packages are ow174.
Prints, per mover, the horizontal drift (at the end, the most), the vertical error and the per-frame step error,
after the same for the live server's own feet ('live'; a drift only in a mover_record.py --observe recording).
--live prints that row alone: the replays assume the Range and Soldier: 76.
"""
import argparse
import gzip
import math
import statistics
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]


def read(path):
    """The commands (forward right yaw pitch buttons seconds), the client's feet and the live server's, by frame."""
    commands, client, server = {}, {}, {}
    with (gzip.open if str(path).endswith('.gz') else open)(path, 'rt', encoding='utf-8') as lines:
        for line in lines:
            kind, *v = line.split()
            if kind == 'S':
                commands[int(v[0])] = tuple(int(x) for x in v[1:6]) + (float(v[6]),)
                server[int(v[0])] = tuple(float(x) for x in v[7:10])
            elif kind == 'C':
                client[int(v[0])] = tuple(float(x) for x in v[1:4])
    return commands, client, server


def emit(which, path, lizrollin):
    """Print `frame x y z` after each replayed frame."""
    commands, client, _ = read(path)
    first = min(f for f, c in commands.items() if c[0] or c[1] or c[4])
    start = max(f for f in client if f <= first - 40)
    if which == 'ours':
        sys.path.insert(0, str(REPO / 'tools' / 'lobby'))
        import practice  # noqa: F401
        import collision
        from ow174.game.commands import Command
        from ow174.game.content import PRACTICE_RANGE
        from player_mover import PlayerMover
        mover = PlayerMover(client[start], commands[start][2], collision.movement_world(PRACTICE_RANGE),
                            0x04000000000003CF)  # Soldier: 76
    else:
        sys.path.insert(0, str(lizrollin))
        from ow174.game import collision, props
        from ow174.game.commands import Command
        from ow174.game.content import PRACTICE_RANGE
        from ow174.game.mover import Mover, mover_data
        world = collision.World.load(collision.cache_path(PRACTICE_RANGE.map_guid),
                                     props.mode_identifiers(PRACTICE_RANGE.mode_guid))
        mover = Mover(client[start], commands[start][2], world, mover_data(0x04000000000003CF))  # Soldier: 76
    for frame in sorted(f for f in commands if f > start):
        forward, right, yaw, pitch, buttons, seconds = commands[frame]
        mover.step(Command(frame, forward, right, yaw, pitch, buttons), seconds)
        print('%d %.5f %.5f %.5f' % (frame, *mover.position))


def report(which, feet, client):
    common = sorted(feet.keys() & client.keys())
    flat = [math.hypot(feet[f][0] - client[f][0], feet[f][2] - client[f][2]) for f in common]
    up = [abs(feet[f][1] - client[f][1]) for f in common]
    steps = sorted(math.dist([a - b for a, b in zip(feet[f], feet[f - 1])],
                             [a - b for a, b in zip(client[f], client[f - 1])])
                   for f in common if f - 1 in feet and f - 1 in client)
    print(f'{which:6} {len(common)} frames  horizontal drift: end {flat[-1]:.3f} m, most {max(flat):.3f} m  '
          f'vertical: median {statistics.median(up):.3f} m, most {max(up):.3f} m  '
          f'step error: median {statistics.median(steps) * 1000:.1f} mm, p99 '
          f'{steps[int(0.99 * (len(steps) - 1))] * 1000:.1f} mm, most {steps[-1] * 1000:.1f} mm')


def compare(path, lizrollin, live_only=False):
    _, client, server = read(path)
    report('live', server, client)  # the server's own feet: drift only with mover_record.py --observe
    for which in () if live_only else ('ours', 'theirs'):
        out = subprocess.run([sys.executable, '-B', __file__, str(path), '--emit', which, '--lizrollin', str(lizrollin)],
                             capture_output=True, text=True, check=True).stdout
        report(which, {int(f): tuple(map(float, xyz)) for f, *xyz in (line.split() for line in out.splitlines())},
               client)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('walk', type=Path)
    parser.add_argument('--lizrollin', type=Path, default=REPO / 'lizrollin-lobby-server')
    parser.add_argument('--live', action='store_true', help="only the live server's feet (any map or hero)")
    parser.add_argument('--emit', choices=('ours', 'theirs'), help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.emit:
        emit(args.emit, args.walk, args.lizrollin)
    else:
        compare(args.walk, args.lizrollin, args.live)


if __name__ == '__main__':
    main()
