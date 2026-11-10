"""Drive a freshly launched game from the start screen into the Practice Range as Soldier: 76,
using background (unfocused) input only. Coordinates are for the 1274x720 window (clamped at 3830 60).

  python enter_range.py [X Y]     optional screen position for the window (default 3830 60)
"""
import subprocess
import sys
import time

import gameinput

STEPS = [  # (action, args, seconds to wait afterwards); the 1274x720 16:9 window (weapon_sweep.py)
    ("click", (80, 658), 0.4), ("key", ("back",), 0.1), ("text", ("1",), 1),  # Pod 1
    ("click", (636, 665), 6),                                                  # Start
    ("click", (640, 560), 0.4), ("text", ("Tester",), 0.4),                    # local name
    ("click", (640, 614), 14),                                                 # Enter Game
    ("click", (70, 160), 30),                                                  # Play: loads the Practice Range
    ("click", (773, 615), 1),                                                  # Soldier: 76
    ("click", (636, 671), 8),                                                  # Select
]


def main(argv):
    deadline = time.monotonic() + 90
    while not gameinput.user32.FindWindowW("tankWindowClass", None):
        if time.monotonic() > deadline:
            sys.exit("game window not found")
        time.sleep(1)
    time.sleep(8)
    hwnd = gameinput.window()
    x, y = (int(argv[1]), int(argv[2])) if len(argv) > 2 else (3830, 60)
    gameinput.user32.SetWindowPos(hwnd, 0, x, y, 0, 0, 0x0001 | 0x0004 | 0x0010)
    for action, args, wait in STEPS:
        if action == "click":
            gameinput.bg_click(hwnd, *args)
        elif action == "key":
            gameinput.bg_key(hwnd, gameinput.vk(args[0]), 60)
        else:
            gameinput.bg_text(hwnd, args[0])
        time.sleep(wait)
    pid = subprocess.run(["powershell", "-NoProfile", "-Command", "(Get-Process Overwatch).Id"],
                         capture_output=True, text=True).stdout.strip()
    print(f"in range; game PID {pid}")


if __name__ == "__main__":
    main(sys.argv)
