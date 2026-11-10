"""Record a live walk for mover checks: every player mover step and every client feet sample, by command frame.

    OW_CLIENT_SYNC=47123 python tools/research/mover_record.py GAME_ROOT OUT.rec [--observe] [--nudge M]
        [--map NAME [--mode GUID]]
    (from the repo root)

Starts the Practice Range (or --map, from its card, like run.py) with ow_fixes.asi (whose client_feed sends the
client's feet), with this file as the lobby, which writes OUT.rec as it goes:
    S frame forward right yaw pitch buttons seconds x y z speed_scale   the server's feet after the step
    C frame x y z slot                                                  the client's feet after that frame
    N frame metres                                                      --nudge lifted the server's copy here
--observe logs the client's feet but leaves the server's copy alone (no client_sync correction), so S - C is the
drift the server's dead reckoning leaves on its own (owner_correction still sends its records). --nudge M lifts the
server's copy M metres at the first crouch: the client's feet follow only if it takes owner_correction's records.
With OW_RECORD_ULT=1 in the environment each new body starts with its ultimate charged (mover_walk.py's ability
course presses Q). Walk with mover_walk.py (or by hand) and close the game; mover_replay.py replays the file, and
mover_mods.py an ability course.
"""
import argparse
import os
import runpy
import sys
from pathlib import Path

LOBBY = Path(__file__).resolve().parents[1] / 'lobby'
sys.path.insert(0, str(LOBBY))


def lobby(out, observe, nudge, args):
    import practice  # noqa: F401  (puts vendor/ on the path)
    import client_sync
    from ow174.game.commands import CROUCH
    from player_mover import PlayerMover
    sink = open(out, 'w', buffering=1, encoding='utf-8')
    step, observed = PlayerMover.step, client_sync.Resync.observed

    nudged = []

    def logged_step(self, command, seconds, tick=0):
        if nudge and not nudged and command.buttons & CROUCH:  # the server alone moves the body: owner_correction
            nudged.append(command.frame)
            self.position = [self.position[0], self.position[1] + nudge, self.position[2]]
            sink.write('N %d %.3f\n' % (command.frame, nudge))
        step(self, command, seconds, tick)
        sink.write('S %d %d %d %d %d %d %.6f %.5f %.5f %.5f %.3f\n' % (
            command.frame, command.forward, command.right, command.yaw, command.pitch, command.buttons, seconds,
            *self.position, self.speed_scale))

    def logged_observed(self, frame, feet, slot=0):
        observed(self, frame, feet, slot)
        sink.write('C %d %.5f %.5f %.5f %d\n' % (frame, *feet, slot))

    aimed = client_sync.Resync.aimed

    def logged_aimed(self, frame, place, good):
        sink.write('M %d %.5f %.5f %.5f %d\n' % (frame, *place, good))
        return aimed(self, frame, place, good)

    PlayerMover.step, client_sync.Resync.observed, client_sync.Resync.aimed = logged_step, logged_observed, logged_aimed
    if os.environ.get('OW_RECORD_ULT'):
        import ow174.game.match as match_module
        update, charged = match_module.Match.update, {}

        def charged_update(self, now, tick):
            for player in self.players:
                if player.has_body and charged.get(player) != player.body:
                    charged[player], player.ult_charge = player.body, player.ult_most
            update(self, now, tick)
        match_module.Match.update = charged_update
    if observe:
        client_sync.OBSERVE = True
    sys.argv = [str(LOBBY / 'lobby.py'), *args]
    runpy.run_path(str(LOBBY / 'lobby.py'), run_name='__main__')


def main():
    if sys.argv[1:2] == ['--lobby']:
        rest = sys.argv[3:]
        observe = rest[:1] == ['--observe']
        rest = rest[observe:]
        nudge = float(rest[1]) if rest[:1] == ['--nudge'] else 0.0
        return lobby(sys.argv[2], observe, nudge, rest[2:] if nudge else rest)
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('game_root', type=Path)
    parser.add_argument('out', type=Path)
    parser.add_argument('--observe', action='store_true', help='log the client but do not correct the server copy')
    parser.add_argument('--map', help='a catalog map (name or GUID) instead of the Practice Range')
    parser.add_argument('--mode', default='0x0230000000000010', help='its game mode GUID (default Skirmish)')
    parser.add_argument('--nudge', type=float, default=0.0, help='lift the server copy this many metres at the first '
                        'crouch (an N line): the client follows only through owner_correction')
    args = parser.parse_args()
    if not os.environ.get('OW_CLIENT_SYNC'):
        parser.error('set OW_CLIENT_SYNC: the client feed sends the feet to that port')
    import run
    command = [sys.executable, '-u', __file__, '--lobby', str(args.out.resolve()), *(['--observe'] * args.observe),
               *(['--nudge', str(args.nudge)] if args.nudge else []),
               '--event', 'none', '--game-install', str(args.game_root), '--name', 'Player', '--retail-menu']
    if args.map:
        command += ['--map', args.map, '--mode', args.mode, '--difficulty', 'easy']
    return run.launch(args.game_root, None, server_command=command, map_name=args.map, mode=args.mode,
                      plugins=['tools/asi/build/ow_fixes.asi'])


if __name__ == '__main__':
    sys.exit(main())
