"""Replay a mover_record.py --observe recording of a hero's abilities: how far the server's mover ends from the
client's feet without the statescript's movement mods (a kits body's mover), and with the ones the vendored VM turns
on when it runs the hero's body on the side.

    python tools/research/mover_mods.py REC[.gz] HERO [--first FRAME] [--save OUT.rec.gz]

The recording is cut into stretches at every rest (no keys and the client's feet still for REST frames), and each
replay starts a stretch on the client's feet, so one stretch's error does not carry into the next. Per stretch: the
buttons pressed, the MovementMod states the VM had on (graph/state and the frames), and the most each replay was off
(horizontal / vertical, metres):
  plain   no mods but the recorded Sprint (what a kits body's mover gets);
  vm      every mod the VM has on.
--save writes the recording from REST frames before its first input, gzipped: what test_player_mover.py replays for
each hero (research/lizrollin-runtime/abilities/). Record with mover_walk.py --course ability. Results:
research/lizrollin-runtime.md section 21.
"""
import argparse
import gzip
import logging
import math
import statistics
import sys
from pathlib import Path

LOBBY = Path(__file__).resolve().parents[1] / 'lobby'
sys.path.insert(0, str(LOBBY))
import practice  # noqa: E402,F401  (puts vendor/ on the path)
import collision  # noqa: E402
from ow174.game.commands import Command  # noqa: E402
from ow174.game.content import PRACTICE_RANGE, heroes  # noqa: E402
from ow174.game.mover import statescript_mods  # noqa: E402
from ow174.game.script.driver import BodyScript  # noqa: E402
from player_mover import PlayerMover  # noqa: E402

REST = 40  # frames without keys and with the client's feet still that end a stretch
BUTTONS = {0x01: 'primary', 0x02: 'secondary', 0x08: 'ability1', 0x10: 'ability2', 0x20: 'ultimate', 0x40: 'jump',
           0x80: 'crouch', 0x400: 'reload', 0x800: 'melee'}
REPLAYS = ('plain', 'vm')


class _Create:
    delivered = True


def read(path, first=0):
    """The commands (forward right yaw pitch buttons seconds, and the live mover's Sprint scale) and the client's
    feet, by frame."""
    commands, client = {}, {}
    with (gzip.open if str(path).endswith('.gz') else open)(path, 'rt', encoding='utf-8') as lines:
        for line in lines:
            kind, frame, *v = line.split()
            if int(frame) < first:
                continue
            if kind == 'S':
                commands[int(frame)] = tuple(int(x) for x in v[:5]) + (float(v[5]), float(v[9]))
            elif kind == 'C':
                client[int(frame)] = tuple(float(x) for x in v[:3])
    return commands, client


def save(source, out, commands):
    """A gzipped copy of the recording from REST frames before its first input: the part a test replays."""
    first = min(frame for frame, c in commands.items() if c[0] or c[1] or c[4]) - REST
    with (gzip.open if str(source).endswith('.gz') else open)(source, 'rt', encoding='utf-8') as lines, \
            gzip.open(out, 'wt', encoding='utf-8', newline='\n') as kept:
        kept.writelines(line for line in lines if int(line.split()[1]) >= first)


def stretches(commands, client):
    """The frames a stretch starts at: the first one, and each where the body has rested for REST frames."""
    frames = sorted(f for f in commands if f in client)
    cuts, still = [frames[0]], 0
    for before, frame in zip(frames, frames[1:]):
        forward, right, _, _, buttons, _, _ = commands[frame]
        still = still + 1 if not (forward or right or buttons) and client[frame] == client[before] else 0
        if still == REST:
            cuts.append(frame)
    return cuts


def replay(hero, commands, client, cuts):
    """Per stretch, a dict: 'frames' (first, last), 'buttons' pressed, 'states' {graph/state: [first, last frame on]}
    of the VM's MovementMod states, and 'off' {replay: (horizontal, vertical)}, the most each mover was off."""
    world = collision.movement_world(PRACTICE_RANGE)
    start = min(cuts)
    quanta = round(statistics.median(c[5] for c in commands.values()) * 1e6)
    script = BodyScript(hero.guid, 0xA0000102, start, quanta=quanta, health=None, ult_seconds=0)
    script.created(_Create()).stream.arrived(0)
    movers = {name: PlayerMover(client[start], commands[start][2], world, hero.body, 0) for name in REPLAYS}
    movers['vm'].script_mods = {}
    rows = []
    for frame in sorted(f for f in commands if f > start):
        if frame in cuts or not rows:
            rows.append({'frames': (frame, frame), 'buttons': set(), 'states': {}, 'off': dict.fromkeys(REPLAYS, (0.0, 0.0))})
            if frame in cuts:
                for mover in movers.values():  # at rest: every replay goes on from the client's feet
                    mover.position, mover.velocity = list(client[frame]), (0.0, 0.0, 0.0)
        row = rows[-1]
        forward, right, yaw, pitch, buttons, seconds, movers['plain'].speed_scale = commands[frame]  # match.py sets it
        for mover in movers.values():
            mover.step(Command(frame, forward, right, yaw, pitch, buttons), seconds)
        script.command(frame, buttons, 0, forward, right)
        script.component.shots[:] = []
        for update in script.frames():  # an owner who gets every frame
            update.stream.arrived(update.chunk_last)
        mods = statescript_mods(script.component)
        named = {key: f'{script.component.instances[key[0]].graph.index:04X}/{key[1]}' for key in mods}
        movers['vm'].script_mods = mods
        for name in set(named.values()):
            row['states'].setdefault(name, [frame, frame])[1] = frame
        row['frames'] = (row['frames'][0], frame)
        row['buttons'] |= {label for bit, label in BUTTONS.items() if buttons & bit}
        if frame in client:
            cx, cy, cz = client[frame]
            for name, mover in movers.items():
                x, y, z = mover.position
                flat, up = row['off'][name]
                row['off'][name] = (max(flat, math.hypot(x - cx, z - cz)), max(up, abs(y - cy)))
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('rec', type=Path)
    parser.add_argument('hero', help='hero name (a prefix, any case)')
    parser.add_argument('--first', type=int, default=0, help='skip the recording before this frame')
    parser.add_argument('--save', type=Path, help='also write the recording from its first input on, gzipped, here')
    args = parser.parse_args()
    logging.disable(logging.CRITICAL)
    hero = next(h for h in sorted(heroes().values(), key=lambda h: h.name) if h.name.lower().startswith(args.hero.lower()))
    commands, client = read(args.rec, args.first)
    if args.save:
        save(args.rec, args.save, commands)
    rows = replay(hero, commands, client, set(stretches(commands, client)))
    always = set.intersection(*(set(row['states']) for row in rows)) if rows else set()
    print(f'{hero.name}: {len(rows)} stretches; on throughout: {", ".join(sorted(always)) or "-"}')
    print('| Frames | Buttons | VM MovementMod states (frames on) | plain | vm |')
    print('|---|---|---|---|---|')
    for row in rows:
        states = ', '.join(f'{name} ({b - a + 1})' for name, (a, b) in sorted(row['states'].items()) if name not in always)
        off = ' | '.join('%.3f / %.3f' % row['off'][name] for name in REPLAYS)
        print(f"| {row['frames'][0]}-{row['frames'][1]} | {' '.join(sorted(row['buttons'])) or '-'} | {states or '-'} | {off} |")


if __name__ == '__main__':
    main()
