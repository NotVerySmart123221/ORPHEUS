"""What the game server sends about its bots' positions, measured offline, beside LizRollin's rule.

    python tools/research/bot_records.py [--seconds 30] [--map NAME] [--mode GUID]

Runs a match (the Practice Range, then --map in --mode, a Practice vs. AI match) through the real
server.Client.send_frame with a socket that sends nothing, and reports per datagram: its size, the bots' movement
records in it, how many of those say what the one before said, and whether a record ever left before its bot's
create or in the same frame. Beside it, the records LizRollin's rule would send (their bots.Bot.record_due: a
datagram every third tick; in each, the walking bots and those that stopped under a second ago; a resting bot once
a second). Results: research/lizrollin-runtime.md section 20.
"""
import argparse
import logging
import random
import sys
from pathlib import Path
from types import SimpleNamespace

LOBBY = Path(__file__).resolve().parents[1] / 'lobby'
sys.path.insert(0, str(LOBBY))
import practice  # noqa: E402  (puts vendor/ on the path)
from ow174.game import server  # noqa: E402
from ow174.game.content import PRACTICE_RANGE, SOLDIER  # noqa: E402
from ow174.game.match import Match  # noqa: E402
from ow174.game.world import OP_CREATE  # noqa: E402

THEIR_SEND_EVERY, THEIR_SETTLE_TICKS = 3, 63  # their match.SEND_EVERY and bots.SETTLE_TICKS
START = 100.0


def measure(game_map, seconds):
    """One match's numbers as a dict."""
    random.seed(1)
    match = Match(game_map, gameplay=True)
    player = match.add_player(1, 'Player', SOLDIER, 0, True)
    quiet = SimpleNamespace(sock=SimpleNamespace(sendto=lambda data, address: None))
    client = server.Client(quiet, ('local', 1), 1, SimpleNamespace(seal=lambda packet: b''), player)
    client.open = True
    player.client, player.has_body, player.spawned, player.select_open, player.body_picked_at = client, True, True, False, 1
    match.started = 0.0
    sent, build = [], client._payload

    def payload(frame, reliable, unreliable, entities):  # send_frame's last call is what leaves
        sent[:] = entities
        return build(frame, reliable, unreliable, entities)
    client._payload = payload
    match.now = START
    client.queue_entities(match._target_updates(player))
    found = {'bots': len(match.bots), 'frames': int(seconds / server.TICK_SECONDS), 'bytes': [], 'records': 0, 'same': 0,
             'before': 0, 'with': 0, 'theirs': 0, 'walking': 0}
    created, last, walked = set(), {}, {}
    for tick in range(1, found['frames'] + 1):
        now = START + tick * server.TICK_SECONDS
        match.update(now, server.FIRST_TICK + tick)
        client.send_frame(server.FIRST_TICK + tick, now)
        client.in_flight.clear()  # nothing is lost on this wire
        bots = {bot.entity: bot for bot in match.bots}
        new = {update.entity for update in sent if update.op == OP_CREATE and update.entity in bots}
        for update in sent:
            if update.movement is not None and update.entity in bots:
                found['records'] += 1
                found['before'] += update.entity not in created and update.entity not in new
                found['with'] += update.entity in new
                said = (update.movement.position, update.movement.yaw, update.movement.pitch, update.movement.velocity)
                found['same'] += last.get(update.entity) == said
                last[update.entity] = said
        created |= new
        found['bytes'].append(len(build(server.FIRST_TICK + tick, [], [], sent)))
        for entity, bot in bots.items():
            if not bot.health or entity not in created:
                continue
            if bot.held != (0, 0):
                walked[entity] = tick
                found['walking'] += 1
            if tick % THEIR_SEND_EVERY == 0:
                moving = tick - walked.get(entity, -THEIR_SETTLE_TICKS) < THEIR_SETTLE_TICKS
                period = THEIR_SETTLE_TICKS // THEIR_SEND_EVERY
                found['theirs'] += moving or (tick // THEIR_SEND_EVERY) % period == entity % period
    return found


def report(name, found, seconds):
    sizes = found['bytes']
    return [f'{name}: {found["bots"]} bots, {seconds:g} s',
            f'  ours:   {found["frames"] / seconds:.1f} datagrams/s, {sum(sizes) / len(sizes):.0f} bytes each (largest {max(sizes)}, '
            f'the client takes {server.MAX_PAYLOAD}); {found["records"] / seconds:.0f} bot records/s, '
            f'{100 * found["same"] / max(1, found["records"]):.0f}% saying what the one before said',
            f'  theirs: {found["frames"] / seconds / THEIR_SEND_EVERY:.1f} datagrams/s; {found["theirs"] / seconds:.0f} bot records/s',
            f'  walking bots at a time: {found["walking"] / found["frames"]:.1f}',
            f'  a record before its bot\'s create: {found["before"]}; in the create\'s frame: {found["with"]}']


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--seconds', type=float, default=30)
    parser.add_argument('--map', default='Hanamura')
    parser.add_argument('--mode', default='0x0230000000000010', help='game mode GUID for --map (default Skirmish)')
    args = parser.parse_args()
    logging.disable(logging.CRITICAL)
    lines = []
    for name, game_map in (('Practice Range', PRACTICE_RANGE), (args.map, practice.pick_map(args.map, args.mode))):
        found = measure(game_map, args.seconds)
        assert not found['before'], 'a movement record left before its create'  # the client has no such entity yet
        lines += report(name, found, args.seconds)
    print('\n'.join(lines))


if __name__ == '__main__':
    main()
