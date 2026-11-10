"""Plan an open-loop walk for the live game: mouse turns and W holds that take the player's server body from a
position to a map objective, checked by replaying them through the server Mover.

    python walkplan.py COUNTS_PER_DEGREE YAW_DEGREES X Y Z [--objective N] [--legs N]

Prints each leg (yaw, seconds, how far the replay ends from the leg's corner) to stderr and the
`gameinput.py fg-key` steps on stdout. YAW and X Y Z come from the lobby log's last "input frame" line (yaw
in 1/65536 turns: multiply by 360/65536). COUNTS_PER_DEGREE is signed: mouse right lowers yaw, about -10.1 at
the game's default sensitivity. Legs follow the nav path's straight runs, so they stay on its floors; replan
from the logged position every few legs (--legs), since the live body drifts from the replay.
Hanamura Assault only (the map the run scripts host).
"""
import argparse
import math
from pathlib import Path
import sys

LOBBY = Path(__file__).resolve().parents[1] / 'lobby'
sys.path[:0] = [str(LOBBY), str(LOBBY / 'vendor')]
import collision, nav
from practice import pick_map
from ow174.game.commands import Command
from ow174.game.mover import mover_data
from player_mover import PlayerMover


def corners(path, start):
    """start, then the last nav point of each straight run (every point within 0.5 m of the leg)."""
    def straight(i, k):
        (ax, _, az), (bx, _, bz) = path[i], path[k]
        length = math.hypot(bx - ax, bz - az) or 1
        return all(abs((p[0] - ax) * (bz - az) - (p[2] - az) * (bx - ax)) / length <= 0.5 for p in path[i:k + 1])
    out, i = [start], 0
    while i < len(path) - 1:
        i = max(k for k in range(i + 1, len(path)) if k == i + 1 or straight(i, k))
        out.append(path[i])
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('counts_per_degree', type=float)
    parser.add_argument('yaw', type=float)
    parser.add_argument('position', type=float, nargs=3)
    parser.add_argument('--objective', type=int, default=0)
    parser.add_argument('--legs', type=int, default=0, help='print only the first N legs (0: all)')
    args = parser.parse_args()
    game_map = pick_map('Hanamura', '0x0230000000000014')
    goal = game_map.objectives[args.objective]
    start = tuple(args.position)
    points = corners(nav.for_map(game_map.map_guid, nav.nav_points(game_map)).path(start, goal), start)
    run_speed = mover_data(None).run_forward  # Soldier: 76's
    legs = [(math.degrees(math.atan2(b[0] - a[0], b[2] - a[2])), math.hypot(b[0] - a[0], b[2] - a[2]) / run_speed)
            for a, b in zip(points, points[1:])]
    mover, frame = PlayerMover(start, 0, collision.movement_world(game_map)), 0  # no cache: a flat floor (logged)
    for n, (yaw, seconds) in enumerate(legs):
        for _ in range(round(seconds * 60)):
            frame += 1
            mover.step(Command(frame, forward=127, yaw=round(yaw * 65536 / 360) & 0xFFFF), 1 / 60)
        off = math.hypot(points[n + 1][0] - mover.position[0], points[n + 1][2] - mover.position[2])
        print(f'leg {n}: yaw {yaw:7.1f} {seconds:5.2f}s  off {off:.2f}', file=sys.stderr)
    print(f'replay ends {math.dist(mover.position, goal):.2f} m from the objective', file=sys.stderr)
    steps, yaw = [], args.yaw
    for leg_yaw, seconds in legs[:args.legs or None]:
        steps += [f'look:{round(((leg_yaw - yaw + 180) % 360 - 180) * args.counts_per_degree)}:0', '150',
                  'w', str(round(seconds * 1000))]
        yaw = leg_yaw
    print(' '.join(steps))


if __name__ == '__main__':
    main()
