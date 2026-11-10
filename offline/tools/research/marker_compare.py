"""Shadow Step's marker as the client drew it against the server's own rule for the same aim.

    python tools/research/marker_compare.py REC [--worst N]

REC: a mover_record.py --observe recording made with Reaper's aim open (E) while the view moves; its M rows are the
client's marker (tools/asi/client_feed.c), frame by frame. For each aim that was held still, the server's rule
(Match.step_point, without the client's feed) is worked out from the same feet and aim on the Practice Range's
collision. Printed: how many aims agree on good / no place, how far apart the two places are, and the worst aims.
"""
import argparse
import gzip
import logging
import math
import statistics
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / 'tools' / 'lobby'))

import practice  # noqa: E402,F401  (puts vendor/ on the path)
import collision  # noqa: E402
from ow174.game.commands import Command  # noqa: E402
from ow174.game.content import PRACTICE_RANGE, heroes  # noqa: E402
from ow174.game.match import Match  # noqa: E402

STILL = 12  # frames the aim has to have been the same for: the marker has settled


def rows(path):
    """{frame: (feet, yaw, pitch)} and {frame: (place, good)} of a recording."""
    steps, marks = {}, {}
    for line in (gzip.open if str(path).endswith('.gz') else open)(path, 'rt', encoding='utf-8'):
        kind, frame, *v = line.split()
        if kind == 'S':
            steps[int(frame)] = (tuple(float(x) for x in v[6:9]), int(v[2]), int(v[3]))
        elif kind == 'M':
            marks[int(frame)] = (tuple(float(x) for x in v[:3]), v[3] == '1')
    return steps, marks


def aims(steps, marks):
    """One (frame, feet, yaw, pitch, place, good) per aim held still for STILL frames."""
    seen, out = set(), []
    for frame in sorted(marks):
        if frame not in steps or any(steps.get(frame - k, (None, None, None))[1:] != steps[frame][1:] for k in range(STILL)):
            continue
        feet, yaw, pitch = steps[frame]
        if (feet, yaw, pitch) not in seen:  # the same aim from another place is another aim
            seen.add((feet, yaw, pitch))
            out.append((frame, feet, yaw, pitch, *marks[frame]))
    return out


def compare(path):
    """[(frame, yaw, pitch, the client's place, good, the server's place or None)] for each still aim."""
    logging.disable(logging.CRITICAL)
    steps, marks = rows(path)
    match = Match(PRACTICE_RANGE, gameplay=True, body_setup=practice.GameServer.body_setup,
                  mesh=collision.for_map(PRACTICE_RANGE.map_guid), world=collision.movement_world(PRACTICE_RANGE))
    reaper = next(h for h in heroes().values() if h.name == 'Reaper')
    player = match.add_player(1, 'Player', reaper.guid, 0, True)
    out = []
    for frame, feet, yaw, pitch, place, good in aims(steps, marks):
        player.mover.position = list(feet)
        ours = match.step_point(player, Command(frame, yaw=yaw, pitch=pitch), 35.0)
        out.append((frame, yaw, pitch, place, good, ours))
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('rec', type=Path)
    parser.add_argument('--worst', type=int, default=8)
    args = parser.parse_args()
    found = compare(args.rec)
    both = [(math.dist(place, ours), frame, yaw, pitch, place, ours) for frame, yaw, pitch, place, good, ours in found
            if good and ours is not None]
    print(f'{len(found)} aims: {len(both)} with a place on both sides, '
          f'{sum(good and ours is None for *_, good, ours in found)} only the client, '
          f'{sum(not good and ours is not None for *_, good, ours in found)} only the server, '
          f'{sum(not good and ours is None for *_, good, ours in found)} neither')
    if both:
        apart = sorted(d for d, *_ in both)
        print(f'apart: median {statistics.median(apart):.2f} m, 90th percentile {apart[int(len(apart) * 0.9)]:.2f} m, '
              f'most {apart[-1]:.2f} m; within 0.25 m: {sum(d <= 0.25 for d in apart)}')
        for d, frame, yaw, pitch, place, ours in sorted(both, reverse=True)[:args.worst]:
            print(f'  frame {frame} yaw {yaw * 180 / 32768:7.1f} pitch {pitch * 180 / 32768:5.1f}: client '
                  f'({place[0]:.2f}, {place[1]:.2f}, {place[2]:.2f}) server ({ours[0]:.2f}, {ours[1]:.2f}, {ours[2]:.2f}) '
                  f'{d:.2f} m')


if __name__ == '__main__':
    main()
