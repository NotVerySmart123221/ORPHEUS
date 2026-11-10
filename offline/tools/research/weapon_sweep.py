"""Drive a running Practice Range session through one hero's weapon checks and grab frames.

  python weapon_sweep.py HERO OUTDIR [ACTION...]

Opens hero select (H), clicks the hero's portrait (1280x720 window, 32-hero bar), presses SELECT, waits for the
lobby log's "Player plays HERO", then runs each ACTION (default: all) and saves OUTDIR/<action>-fNN.png bursts:
  idle    one frame of the held weapon
  fire    left mouse held 0.6 s, 8 frames
  reload  R, 14 frames over the reload (2.8 s: Orisa's is 2.5 s)
  alt     right mouse 0.3 s, 8 frames
  grasp   right mouse 2.5 s, 16 frames from the logged lock (Moira; aim at a bot first)
  hack    right mouse 1.2 s, 12 frames from the logged start (Sombra; aim at a bot first)
  heal    left mouse 2 s, 12 frames from the logged lock (Mercy's staff; aim at an ally bot first)
  boost   right mouse 2 s, 12 frames from the logged lock (Mercy's damage boost)
  melee   V 0.1 s, 8 frames, and the command buttons the lobby logged for the press
  shift, e, matrix  a kits.ABILITIES press: Shift 0.1 s, E 0.1 s, right mouse held 1.5 s (D.Va's mech)
  matrix-fire  fire, hold both triggers through Matrix depletion, then continue primary fire
Foreground input waits for the user to be idle (gameinput.foreground); each burst is grabbed from the first logged
press. Coordinates assume the window placed by enter_range.py.
"""
import os
from pathlib import Path
import re
import subprocess
import sys
import time

import gameinput
import screengrab

LOG_ROOT = str(Path(__file__).resolve().parents[2] / 'research' / 'main-menu')
# portrait centres on the hero select bar (y 615): tanks, damage, supports, each alphabetical, 36 px apart, for the
# 1274x720 window at 16:9 (Settings_v0.ini Use219AspectRatio = "0"; with it on, the old letterboxed bar was y 550)
ROLES = (('D.Va', 'Orisa', 'Reinhardt', 'Roadhog', 'Sigma', 'Winston', 'Wrecking Ball', 'Zarya'),
         ('Ashe', 'Bastion', 'Cassidy', 'Doomfist', 'Echo', 'Genji', 'Hanzo', 'Junkrat', 'Mei', 'Pharah', 'Reaper',
          'Soldier: 76', 'Sombra', 'Symmetra', 'Torbjörn', 'Tracer', 'Widowmaker'),
         ('Ana', 'Baptiste', 'Brigitte', 'Lúcio', 'Mercy', 'Moira', 'Zenyatta'))
PORTRAITS = {}
for start, names in zip((72, 377, 1000), ROLES):
    PORTRAITS.update({name: (start + 36 * i, 615) for i, name in enumerate(names)})
SELECT = (636, 671)
ACTIONS = {  # name -> (fg-key args, log text that marks the press, frames, seconds between frames)
    'idle': ((), None, 1, 0),
    'fire': (('lmb', '600'), b'rifle shot', 8, 0.08),
    'reload': (('r', '80'), b'buttons=000400', 14, 0.2),
    'alt': (('rmb', '300'), b'alt fire', 8, 0.08),
    'grasp': (('rmb', '2500'), b'grasp target=B', 16, 0.12),  # Moira's held beam, on a bot in the cone
    'hack': (('rmb', '1200'), b'Hack start', 12, 0.06),  # Sombra's 0.65 s hack channel
    'heal': (('lmb', '2000'), b'staff heal target=B', 12, 0.15),  # Mercy's staff beams, on an ally bot
    'boost': (('rmb', '2000'), b'staff boost target=B', 12, 0.15),
    'melee': (('v', '100'), b'ability 0x800', 8, 0.08),
    'sprint': (('shift+w', '2500'), b'sprint start', 10, 0.2),  # Soldier: 76's abilities
    'sprint-transitions': (('w', '400', 'shift+w', '1500', 'w', '900'), b'sprint start', 60, 0.05),
    'helix': (('rmb', '100'), b'helix serial', 12, 0.06),
    'field': (('e', '100'), b'biotic field serial', 16, 0.1),
    'shift': (('shift', '100'), b'ability 0x08', 10, 0.15),  # kits.ABILITIES presses (D.Va: Boosters)
    'e': (('e', '100'), b'ability 0x10', 12, 0.08),  # D.Va: Micro Missiles
    'matrix': (('rmb', '1500'), b'ability 0x02', 10, 0.15),  # D.Va: Defense Matrix (held)
    'matrix-fire': (('lmb', '400', 'lmb+rmb', '2500', 'lmb', '400'), b'ability 0x02', 26, 0.12),
}


def newest_log():
    runs = (os.path.join(LOG_ROOT, x) for x in os.listdir(LOG_ROOT))
    return os.path.join(max((x for x in runs if os.path.isdir(x)), key=os.path.getmtime), 'lobby.log')


def wait_log(path, start, text, seconds):
    deadline = time.time() + seconds
    while time.time() < deadline:
        with open(path, 'rb') as f:
            f.seek(start)
            data = f.read()
        if text in data:
            return data
        time.sleep(0.005)
    return None


def grab(hwnd, out):
    for _ in range(20):
        try:
            screengrab.grab(hwnd).save(out)
            return
        except RuntimeError:  # PrintWindow fails now and then right after the focus hand-back
            time.sleep(0.05)


def pick(hwnd, hero, log):
    name = hero.encode('cp1252')  # the lobby log's encoding (Torbjörn, Lúcio)
    with open(log, 'rb') as f:
        plays = re.findall(rb'Player plays (.+)', f.read())
    if plays and plays[-1].strip() == name:
        return  # already played: a re-pick logs nothing
    start = os.path.getsize(log)
    subprocess.run([sys.executable, 'gameinput.py', 'fg-key', 'h', '60'], cwd=os.path.dirname(__file__) or '.')
    time.sleep(2)
    gameinput.bg_click(hwnd, *PORTRAITS[hero])
    time.sleep(1)
    gameinput.bg_click(hwnd, *SELECT)
    if wait_log(log, start, b'Player plays ' + name, 20) is None:
        sys.exit(f'{hero}: no "Player plays" in the lobby log')
    time.sleep(6)  # body setup and the weapon frame


def run(hwnd, log, name, out):
    keys, mark, frames, gap = ACTIONS[name]
    start = os.path.getsize(log)
    if keys:
        press = subprocess.Popen([sys.executable, 'gameinput.py', 'fg-key', *keys], cwd=os.path.dirname(__file__) or '.')
        data = wait_log(log, start, mark, 120)
        if name == 'melee' and data:
            buttons = sorted(set(re.findall(rb'buttons=([0-9A-F]+)', data)))
            print('melee press buttons:', [b.decode() for b in buttons])
    for i in range(frames):
        grab(hwnd, os.path.join(out, f'{name}-f{i:02d}.png'))
        time.sleep(gap)
    if keys:
        press.wait()
        if data is None:
            print(f'{name}: no "{mark.decode()}" logged')


def main(argv):
    hero, out, actions = argv[1], argv[2], argv[3:] or list(ACTIONS)
    os.makedirs(out, exist_ok=True)
    hwnd = gameinput.window()
    log = newest_log()
    pick(hwnd, hero, log)
    for name in actions:
        run(hwnd, log, name, out)
        time.sleep(1.5)
    print('done', hero, actions)


if __name__ == '__main__':
    main(sys.argv)
