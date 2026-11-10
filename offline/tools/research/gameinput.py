"""Place the game window and send it input, preferring background (unfocused) delivery.

  python gameinput.py place X Y            move the window's top-left to screen X,Y
  python gameinput.py bg-click X Y         post a left click at client X,Y (no focus change)
  python gameinput.py bg-key KEY [MS]      post a key press; KEY is a name below or a VK number
  python gameinput.py bg-text TEXT         post characters
  python gameinput.py fg-click X Y         warn, take focus, real click, then hand focus back
  python gameinput.py fg-key KEY[+KEY] [MS] [KEY[+KEY] MS ...]
                                           warn, take focus, hold each chord in turn, then hand focus back;
                                           KEY may be lmb or rmb (mouse buttons); a step look:DX:DY
                                           moves the mouse, then waits MS
  python gameinput.py fg-rec OUT.mp4 KEY[+KEY] MS ...
                                           fg-key while recording the game's client area at 60 fps,
                                           half size (ffmpeg desktop duplication)
  python gameinput.py fg-look DX [DY]      warn, take focus, move the mouse by DX,DY counts (aim)
  python gameinput.py fg-text TEXT        warn, take focus, type, then hand focus back

Foreground commands first wait until the user has been idle for IDLE_SECONDS (exit 3 after
MAX_WAIT_SECONDS), then show a 10-second on-top countdown; moving the mouse cancels (exit 2).
Menus accept background input; gameplay (raw input) only accepts foreground input.
"""
import ctypes
import ctypes.wintypes as wt
import subprocess
import sys
import time

user32 = ctypes.WinDLL("user32")
user32.SetProcessDPIAware()

WM_KEYDOWN, WM_KEYUP, WM_CHAR = 0x100, 0x101, 0x102
WM_MOUSEMOVE, WM_LBUTTONDOWN, WM_LBUTTONUP = 0x200, 0x201, 0x202
KEYS = {"enter": 0x0D, "esc": 0x1B, "space": 0x20, "tab": 0x09, "back": 0x08,
        "up": 0x26, "down": 0x28, "left": 0x25, "right": 0x27, "shift": 0x10, "ctrl": 0x11,
        "lmb": -1, "rmb": -2}  # mouse buttons, foreground only
MOUSE = {-1: (2, 4), -2: (8, 0x10)}  # mouse_event down/up flags
GUARD_SECONDS = 10
IDLE_SECONDS = 30
MAX_WAIT_SECONDS = 1800


class LASTINPUTINFO(ctypes.Structure):
    _fields_ = [("cbSize", wt.UINT), ("dwTime", wt.DWORD)]


def idle_seconds():
    info = LASTINPUTINFO(ctypes.sizeof(LASTINPUTINFO))
    user32.GetLastInputInfo(ctypes.byref(info))
    return (ctypes.windll.kernel32.GetTickCount() - info.dwTime) / 1000


def wait_for_idle():
    deadline = time.monotonic() + MAX_WAIT_SECONDS
    while idle_seconds() < IDLE_SECONDS:
        if time.monotonic() > deadline:
            print("user stayed active; gave up")
            sys.exit(3)
        time.sleep(1)


def window():
    hwnd = user32.FindWindowW("tankWindowClass", None)
    if not hwnd:
        sys.exit("game window not found")
    return hwnd


def vk(name):
    name = name.lower()
    if name in KEYS:
        return KEYS[name]
    if len(name) == 1:
        return ord(name.upper())
    return int(name, 0)


def key_lparam(code, up):
    scan = user32.MapVirtualKeyW(code, 0)
    return 1 | (scan << 16) | ((0xC0000000) if up else 0)


def bg_click(hwnd, x, y):
    pos = (y << 16) | (x & 0xFFFF)
    user32.PostMessageW(hwnd, WM_MOUSEMOVE, 0, pos)
    user32.PostMessageW(hwnd, WM_LBUTTONDOWN, 1, pos)
    time.sleep(0.05)
    user32.PostMessageW(hwnd, WM_LBUTTONUP, 0, pos)


def bg_key(hwnd, code, ms):
    user32.PostMessageW(hwnd, WM_KEYDOWN, code, key_lparam(code, False))
    time.sleep(ms / 1000)
    user32.PostMessageW(hwnd, WM_KEYUP, code, key_lparam(code, True))


def bg_text(hwnd, text):
    for ch in text:
        user32.PostMessageW(hwnd, WM_CHAR, ord(ch), 1)
        time.sleep(0.03)


def guard():
    """On-top countdown; returns False if the user moves the mouse."""
    import tkinter as tk
    root = tk.Tk()
    root.overrideredirect(True)
    root.attributes("-topmost", True)
    root.geometry(f"+{root.winfo_screenwidth() // 2 - 220}+40")
    label = tk.Label(root, font=("Segoe UI", 14), bg="#202020", fg="white", padx=16, pady=10)
    label.pack()
    # The cursor was seen to jump once when this window appears (not user input); start after that.
    root.update()
    time.sleep(0.5)
    start = wt.POINT()
    user32.GetCursorPos(ctypes.byref(start))
    deadline = time.monotonic() + GUARD_SECONDS
    ok = True
    while (left := deadline - time.monotonic()) > 0:
        now = wt.POINT()
        user32.GetCursorPos(ctypes.byref(now))
        if abs(now.x - start.x) + abs(now.y - start.y) > 3:
            ok = False
            break
        label.config(text=f"Overwatch test takes keyboard/mouse in {left:.0f}s - move mouse to cancel")
        root.update()
        time.sleep(0.1)
    root.destroy()
    return ok


def foreground(hwnd, action):
    wait_for_idle()
    if not guard():
        print("cancelled by user")
        sys.exit(2)
    previous = user32.GetForegroundWindow()
    cursor = wt.POINT()
    user32.GetCursorPos(ctypes.byref(cursor))
    # Alt tap lets this process move the foreground (Windows foreground-lock rule).
    user32.keybd_event(0x12, 0, 0, 0)
    user32.keybd_event(0x12, 0, 2, 0)
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.3)
    try:
        action()
    finally:
        time.sleep(0.2)
        user32.SetCursorPos(cursor.x, cursor.y)
        if previous:
            user32.keybd_event(0x12, 0, 0, 0)
            user32.keybd_event(0x12, 0, 2, 0)
            user32.SetForegroundWindow(previous)


def fg_click(hwnd, x, y):
    pt = wt.POINT(x, y)
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    user32.SetCursorPos(pt.x, pt.y)
    time.sleep(0.05)
    user32.mouse_event(2, 0, 0, 0, 0)
    time.sleep(0.05)
    user32.mouse_event(4, 0, 0, 0, 0)


def fg_key(codes, ms):
    """Hold every key in codes together for ms milliseconds."""
    def press(code, up):
        if code in MOUSE:
            user32.mouse_event(MOUSE[code][up], 0, 0, 0, 0)
        else:
            user32.keybd_event(code, user32.MapVirtualKeyW(code, 0), 2 if up else 0, 0)
    if any(code in MOUSE for code in codes):  # the game reads mouse buttons only with the cursor over it
        rect = wt.RECT()
        user32.GetWindowRect(window(), ctypes.byref(rect))
        user32.SetCursorPos((rect.left + rect.right) // 2, (rect.top + rect.bottom) // 2)
        time.sleep(0.05)
    for code in codes:
        press(code, False)
    time.sleep(ms / 1000)
    for code in reversed(codes):
        press(code, True)


def fg_look(dx, dy=0):
    """Relative mouse motion in small steps, as a hand would move it."""
    steps = max(1, max(abs(dx), abs(dy)) // 20)
    for i in range(steps):
        user32.mouse_event(1, dx * (i + 1) // steps - dx * i // steps, dy * (i + 1) // steps - dy * i // steps, 0, 0)
        time.sleep(0.01)


def record(hwnd, path):
    """Start recording the client area; stop with stop_recording(). The game must be in front (desktop duplication
    sees the screen). shortcut: assumes DXGI output 0 is the monitor at the desktop origin (one monitor here)."""
    origin, rect = wt.POINT(), wt.RECT()
    user32.ClientToScreen(hwnd, ctypes.byref(origin))
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    grab = (f"ddagrab=output_idx=0:framerate=60:offset_x={origin.x}:offset_y={origin.y}"
            f":video_size={rect.right & ~1}x{rect.bottom & ~1}")
    # half size on the CPU: this ffmpeg's NVENC needs a newer driver than installed
    proc = subprocess.Popen(["ffmpeg", "-loglevel", "error", "-y", "-f", "lavfi", "-i", grab,
                             "-vf", "hwdownload,format=bgra,scale=iw/2:-2", "-c:v", "libx264", "-preset", "ultrafast",
                             "-crf", "18", "-pix_fmt", "yuv420p", path], stdin=subprocess.PIPE)
    time.sleep(1)  # ffmpeg start-up
    return proc


def stop_recording(proc):
    proc.communicate(b"q", timeout=30)


def fg_text(text):
    for ch in text:
        code = user32.VkKeyScanW(ord(ch))
        shift = code & 0x100
        if shift:
            user32.keybd_event(0x10, 0, 0, 0)
        fg_key([code & 0xFF], 30)
        if shift:
            user32.keybd_event(0x10, 0, 2, 0)


def main(argv):
    if len(argv) < 2:
        sys.exit(__doc__)
    cmd, args = argv[1], argv[2:]
    hwnd = window()
    if cmd == "place":
        user32.SetWindowPos(hwnd, 0, int(args[0]), int(args[1]), 0, 0, 0x0001 | 0x0004 | 0x0010)
    elif cmd == "bg-click":
        bg_click(hwnd, int(args[0]), int(args[1]))
    elif cmd == "bg-key":
        bg_key(hwnd, vk(args[0]), int(args[1]) if len(args) > 1 else 60)
    elif cmd == "bg-text":
        bg_text(hwnd, args[0])
    elif cmd == "fg-click":
        foreground(hwnd, lambda: fg_click(hwnd, int(args[0]), int(args[1])))
    elif cmd in ("fg-key", "fg-rec"):
        out = args.pop(0) if cmd == "fg-rec" else None

        def step(keys, ms):
            if keys.startswith("look:"):
                fg_look(*(int(v) for v in keys.split(":")[1:]))
                time.sleep(ms / 1000)
            else:
                fg_key([vk(name) for name in keys.split("+")], ms)
        holds = list(zip(args[::2], (int(ms) for ms in args[1::2] or ["60"])))

        def act():
            proc = record(hwnd, out) if out else None
            try:
                for keys, ms in holds:
                    step(keys, ms)
            finally:
                if proc:
                    stop_recording(proc)
        foreground(hwnd, act)
    elif cmd == "fg-look":
        foreground(hwnd, lambda: fg_look(int(args[0]), int(args[1]) if len(args) > 1 else 0))
    elif cmd == "fg-text":
        foreground(hwnd, lambda: fg_text(args[0]))
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv)
