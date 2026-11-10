"""Live check for body_vm (research/lizrollin-runtime.md section 15), as Soldier: 76 in the Practice Range.

    python tools/research/vm_live.py launch GAME_ROOT OUTDIR [PLUGIN ...]   game + lobby, OW_BODY_VM=1, closes
                                                     after LIMIT s; PLUGINs: more ASIs (tools/asi/build/tracer.asi)
    python tools/research/vm_live.py drive OUTDIR [abilities]  in a second process after launch prints "Game PID"
                                                               (it drives whatever game window it finds first)

launch: run.py's Practice Range with ow_fixes.asi, one Training Bot patrol moved to stand still AHEAD m in front of
the spawn (a 16th bot overflows 20300's frame), and OUTDIR/cmd.txt read each tick: 'ult' fills the ultimate,
'kill' kills the player. With OW_LIVE_BOT set to a hero name, the still bot is that hero's harmless mannequin
(tools/lobby/viewer.py) 5 m ahead instead.
drive: mover_walk.py's background clicks (2560x720 window), retried until the lobby log shows each step, then in
the foreground a frame grab after each step of
  - the default check: shots at the bot, a full ultimate, Tactical Visor and its shots, death, the respawn, and
    the rifle and Helix on the new body;
  - abilities: fire, reload, Helix, Field, Sprint and melee.
"""
import logging
import math
import os
import runpy
import sys
import time
from pathlib import Path

RESEARCH = Path(__file__).resolve().parent
ROOT, LOBBY = RESEARCH.parents[1], RESEARCH.parent / 'lobby'
LIMIT = 420
AHEAD = 8.0  # metres from the spawn to the still bot, along the client's facing
CLIENT_YAW = -32768  # the client faces 180 degrees at the Range spawn (every run's input log), not its -170.3
WIDE = 639


def lobby(cmd, args):
    os.environ.setdefault('OW_BODY_VM', '1')  # OW_BODY_VM=all in the environment: every hero on the VM
    sys.path.insert(0, str(LOBBY))
    import practice  # noqa: F401  (puts vendor/ on the path)
    import viewer
    import ow174.game.match as M
    from ow174.game.content import PRACTICE_RANGE, spawn_point
    from target import BOT_BODY, TrainingTarget
    from combat import aim_direction
    log = logging.getLogger('ow174.game')
    made, update = M.range_bots, M.Match.update
    still = []

    def range_bots(grid=None, mesh=None):
        bots = made(grid, mesh)
        bot = next(b for b in bots if b.behaviour == 'patrol' and b.body == BOT_BODY)
        (x, y, z), _ = spawn_point(PRACTICE_RANGE, 1, 0)  # the player's spawn (section 13's live check)
        ahead = aim_direction(CLIENT_YAW, 0)
        at = (x + AHEAD * ahead[0], y, z + AHEAD * ahead[2])
        hero = os.environ.get('OW_LIVE_BOT')
        still[:] = [viewer.mannequin(hero, ((x, y, z), 180.0)) if hero  # 180: along the client's facing
                    else TrainingTarget(at, 0.0, BOT_BODY, 1, None, (), bot.entity)]  # the player's foe
        bots[bots.index(bot)] = still[0]
        log.info('[live] still bot %08X at %s', still[0].entity, still[0].position)
        return bots

    def patched(self, now, tick):
        update(self, now, tick)
        if not cmd.exists():
            return
        word = cmd.read_text().strip()
        cmd.unlink()
        for player in self.players:
            if not player.has_body:
                continue
            log.info('[live] %s', word)
            if word == 'ult':
                player.ult_charge = player.ult_most
                if player.body_vm is not None:
                    player.body_vm.sync(player)
            elif word == 'kill':
                self._damage_player(player, 99999, now, still[0].entity if still else None)

    M.range_bots, M.Match.update = range_bots, patched
    sys.argv = [str(LOBBY / 'lobby.py'), *args]
    runpy.run_path(str(LOBBY / 'lobby.py'), run_name='__main__')


def launch(game_root, out, plugins=()):
    out.mkdir(parents=True, exist_ok=True)
    (out / 'cmd.txt').unlink(missing_ok=True)
    os.chdir(ROOT)
    sys.path.insert(0, str(LOBBY))
    import run
    command = [sys.executable, '-u', __file__, '--lobby', str(out / 'cmd.txt'), '--event', 'none',
               '--game-install', str(game_root), '--name', 'Player', '--retail-menu']
    return run.launch(game_root, LIMIT, server_command=command, plugins=['tools/asi/build/ow_fixes.asi', *plugins])


def drive(out, abilities):
    import gameinput as gi
    import screengrab
    from weapon_sweep import PORTRAITS
    out.mkdir(parents=True, exist_ok=True)
    logs, started = ROOT / 'research' / 'main-menu', time.time()

    def lobby_log():
        runs = [p for p in logs.iterdir() if p.is_dir() and p.stat().st_mtime >= started - 600]
        return max(runs, key=lambda p: p.stat().st_mtime) / 'lobby.log' if runs else None

    def logged(text):
        log = lobby_log()
        return log.read_text(errors='replace').count(text) if log and log.exists() else 0

    def wait_for(text, seconds, more=0):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if logged(text) > more:
                return True
            time.sleep(0.25)
        return False

    deadline = time.monotonic() + 180
    while not gi.user32.FindWindowW('tankWindowClass', None):
        assert time.monotonic() < deadline, 'no game window'
        time.sleep(1)
    h = gi.window()
    time.sleep(60)
    for attempt in range(4):
        gi.bg_click(h, 102, 218); time.sleep(4)  # TRAINING, then the Practice Range card
        gi.bg_click(h, 1280, 358)
        if wait_for('loaded the map', 60):
            break
        print('retry entering', attempt, flush=True)
    else:
        sys.exit('never entered the Range')
    time.sleep(8)
    x, y = PORTRAITS['Soldier: 76']
    for attempt in range(4):
        gi.bg_click(h, x + WIDE, y + 3); time.sleep(1.5); gi.bg_click(h, 1280, 673)
        if wait_for('plays Soldier', 10):
            break
        print('retry pick', attempt, flush=True)
    else:
        sys.exit('never picked Soldier')
    time.sleep(6)
    print('in the Range', flush=True)

    def shot(name):
        screengrab.grab(h).save(out / f'{name}.png')
        print('grab', name, round(time.time() - started, 1), flush=True)

    def tell(word):
        before = logged(f'[live] {word}')
        (out / 'cmd.txt').write_text(word)
        print('cmd', word, 'ok' if wait_for(f'[live] {word}', 3, before) else 'NOT SEEN', flush=True)

    def check_abilities():
        shot('0-idle')
        gi.fg_key([gi.vk('lmb')], 900); shot('1-after-fire')
        gi.fg_key([gi.vk('lmb')], 2400); time.sleep(0.2); shot('2-after-long-fire')
        gi.fg_key([gi.vk('r')], 100); time.sleep(2.0); shot('3-reloaded')
        gi.fg_key([gi.vk('rmb')], 100); time.sleep(0.25); shot('4-helix')
        time.sleep(1.0)
        gi.fg_key([gi.vk('e')], 100); time.sleep(0.8); shot('5-field')
        time.sleep(1.0)
        gi.fg_key([gi.vk('shift'), gi.vk('w')], 1500); shot('6-sprint-end')
        time.sleep(0.5)
        gi.fg_key([gi.vk('v')], 100); time.sleep(0.15); shot('7-melee')
        time.sleep(1.0); shot('8-end')

    def check_ult_death():
        shot('0-idle')
        gi.fg_key([gi.vk('lmb')], 600); time.sleep(0.2); shot('1-bot-hit')  # ~6 shots: the bot (200) lives
        gi.fg_key([gi.vk('r')], 100); time.sleep(1.8)
        tell('ult'); time.sleep(0.6); shot('2-ult-ready')
        gi.fg_key([gi.vk('q')], 100); time.sleep(0.4); shot('3-visor-cast')
        time.sleep(1.0); shot('4-visor')
        gi.fg_key([gi.vk('lmb')], 1500); shot('5-visor-fire')
        time.sleep(5.0); shot('6-visor-end')  # Q + VISOR_CAST 1.2 + VISOR_SECONDS 6 has passed
        respawns = logged('player respawn')
        tell('kill'); time.sleep(0.8); shot('7-dead')
        print('respawn', 'ok' if wait_for('player respawn', 20, respawns) else 'NOT SEEN', flush=True)
        time.sleep(1.5); shot('8-respawned')
        gi.fg_key([gi.vk('lmb')], 900); time.sleep(0.2); shot('9-fire-after-respawn')
        gi.fg_key([gi.vk('rmb')], 100); time.sleep(0.3); shot('10-helix-after-respawn')
        time.sleep(1.0); shot('11-end')

    gi.foreground(h, check_abilities if abilities else check_ult_death)
    print('done', flush=True)


if __name__ == '__main__':
    if sys.argv[1] == '--lobby':
        lobby(Path(sys.argv[2]), sys.argv[3:])
    elif sys.argv[1] == 'launch':
        sys.exit(launch(Path(sys.argv[2]), Path(sys.argv[3]).resolve(), sys.argv[4:]))
    else:
        drive(Path(sys.argv[2]), sys.argv[3:] == ['abilities'])
