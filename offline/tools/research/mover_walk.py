"""The courses mover_record.py records: on a fresh launch, enter the Practice Range (or the map run.py opens from its
card; 2560x720 window) as a hero, then run a course in the foreground (gameinput waits for the user to be idle;
moving the mouse cancels).

    python tools/research/mover_walk.py [--hero NAME] [--course walk|boost|fire|ability|sentry] [--no-enter]

walk: straights, a backpedal, strafes, diagonals, a jump, a crouch walk, turns and a strafe into the spawn table.
boost (D.Va): Boosters straight ahead, steered by a turn, with W held, into a wall, aimed up and aimed down.
fire: walks and a jump while firing, then the same jump without (D.Va's cannons change her air deceleration).
ability (any hero; mover_mods.py reads it): a walk forward with each of Shift, E, a jump with the jump then held in
the air, each fire button held and Q (mover_record.py charges the ultimate with OW_RECORD_ULT=1), with a rest and a
walk back between them; then a crouch walk, a jump out of a crouch and a strafe each way.
sentry (Bastion): Reconfigure with W held, walks in Sentry, Reconfigure back, then a walk, a jump and a crouch walk.
--no-enter: the game is already in the map with the hero picked.
"""
import argparse
import time

import gameinput as gi
from weapon_sweep import PORTRAITS

COURSES = {
    'walk': [('w', 800), (None, 600), ('s', 1000), (None, 600), ('a', 600), (None, 400), ('d', 900), (None, 400),
             ('w+a', 700), (None, 400), ('s+d', 700), (None, 400), ('w+space', 900), (None, 800),
             ('ctrl+w', 900), (None, 600), ('look:400', 300), ('w', 700), (None, 400), ('look:-800', 300),
             ('w', 900), ('d', 1500), (None, 400), ('s+a', 800), (None, 600), ('w+d', 600), ('space', 100),
             (None, 1200)],
    'boost': [('w', 400), ('shift', 100), (None, 2600), (None, 4000),  # a straight flight; the cooldown
              ('look:-1200', 300), ('shift', 100), ('look:600', 600), (None, 2000), (None, 4000),  # steered
              ('look:1200', 300), ('shift+w', 2600), (None, 4000),  # with W held
              ('look:-1600', 300), ('shift', 100), (None, 3000), ('w', 600), (None, 1200), (None, 3000),
              ('look:0:-400', 300), ('shift', 100), (None, 2600), (None, 4000),  # aimed up
              ('look:0:800', 300), ('shift', 100), (None, 2600), ('look:0:-400', 300), (None, 1500)],  # down
    'fire': [('w', 800), ('w+lmb', 1500), (None, 600), ('d+lmb', 1200), (None, 600), ('s+lmb', 1000), (None, 600),
             ('w+space+lmb', 900), (None, 1000), ('w+space', 900), (None, 1200)],
    'ability': [('w', 600), ('w+shift', 120), ('w', 2400), (None, 900), ('s', 2400), (None, 900),  # ability 1
                ('w+e', 120), ('w', 2400), (None, 900), ('s', 2400), (None, 900),  # ability 2
                ('w+space', 120), ('w', 250), ('w+space', 1400), (None, 1200), ('s', 2000), (None, 900),  # jump held
                ('w+rmb', 1500), (None, 900), ('s', 1300), (None, 900), ('w+lmb', 1500), (None, 900),  # the fires
                ('s', 1300), (None, 900), ('w+q', 120), ('w', 2400), (None, 900), ('s', 2400), (None, 900),  # ultimate
                ('ctrl+w', 900), (None, 700), ('ctrl', 700), ('ctrl+space', 150), (None, 1300),  # crouched; a jump from it
                ('a', 700), (None, 700), ('d', 700), (None, 1500)],
    'sentry': [('w', 600), ('w+shift', 120), ('w', 2400), (None, 900), ('s', 1500), (None, 900),  # into Sentry
               ('w+shift', 120), ('w', 2400), (None, 900), ('s', 2400), (None, 900),  # back to Recon
               ('w+space', 900), (None, 1200), ('ctrl+w', 900), (None, 1300)],
}
WIDE = 639  # the 32:9 window's offset of weapon_sweep's 16:9 coordinates (Soldier: 76 at 1412, SELECT at 1280)


def course(legs):
    def run():
        for keys, ms in legs:
            if keys is None:
                time.sleep(ms / 1000)
            elif keys.startswith('look:'):
                gi.fg_look(*(int(v) for v in keys[5:].split(':')))  # dx[:dy], counts
                time.sleep(ms / 1000)
            else:
                gi.fg_key([gi.vk(k) for k in keys.split('+')], ms)
    return run


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--hero', default='Soldier: 76', choices=sorted(PORTRAITS))
    parser.add_argument('--course', default='walk', choices=sorted(COURSES))
    parser.add_argument('--no-enter', action='store_true')
    args = parser.parse_args()
    deadline = time.monotonic() + 120
    while not gi.user32.FindWindowW('tankWindowClass', None):
        assert time.monotonic() < deadline, 'no game window'
        time.sleep(1)
    h = gi.window()
    if not args.no_enter:
        time.sleep(40)
        gi.bg_click(h, 102, 218); time.sleep(4)  # the Practice Range card, its Play, the hero, its lock-in
        gi.bg_click(h, 1280, 358); time.sleep(32)
        x, y = PORTRAITS[args.hero]
        gi.bg_click(h, x + WIDE, y + 3); time.sleep(1.5); gi.bg_click(h, 1280, 673); time.sleep(6)
    gi.foreground(h, course(COURSES[args.course]))
    print('course done', flush=True)


if __name__ == '__main__':
    main()
