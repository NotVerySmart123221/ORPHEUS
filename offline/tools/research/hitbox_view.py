"""Draw the server's hitscan volumes over the game (research/hitbox-drift.md).

Start the lobby with OW_HITBOX_VIEW=PORT (tools/lobby/hitbox_feed.py), then:

    python hitbox_view.py [--port 47123] [--always] [--drift] live click-through overlay on the game window
    python hitbox_view.py --snapshot OUT.png [--port 47123]  one capture with the volumes drawn in

Volumes are projected from the server's eye and aim, so they show exactly where the crosshair has to be for the
server to count a hit: red = the critical (head) volume, green = the rest. Where they sit off the rendered bots,
the server's copy of the player (or of the bot) has drifted from the client's. The live overlay hides while the
game is not the foreground window, unless --always. --drift also reads the client's own position from the game's
memory (client_position.py; the first find takes about 8 s) and shows how far the server's copy is off at the same
command frame.

Projection: the client's vertical FOV is 70.5328 degrees at every aspect ratio (Hor+;
research/aspect-ratio-32-9/REPORT.md), so the focal length is half the viewport height times 1.41410816; the
viewport is the largest centred rectangle of the game's aspect ratio (16:9, or 32:9 with ow_fixes.asi on) in the
client area.
"""
import argparse
import ctypes
import ctypes.wintypes as wt
import json
import math
from pathlib import Path
import socket
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lobby'))
from combat import aim_direction  # noqa: E402

TURN = 2 * math.pi / 65536
VERTICAL_SCALE = 1.41410816  # 1 / tan(vertical FOV / 2)
NEAR = 0.1  # metres; closer points are not drawn
PORT = 47123
CRITICAL, BODY, TEXT = '#ff3030', '#30ff60', '#ffff40'


def camera(yaw, pitch):
    """(forward, right, up) unit vectors of the command angles; right is (-cos yaw, 0, sin yaw) as in mover.py."""
    forward = aim_direction(yaw, pitch)
    angle = yaw * TURN
    right = (-math.cos(angle), 0.0, math.sin(angle))
    up = (right[1] * forward[2] - right[2] * forward[1], right[2] * forward[0] - right[0] * forward[2],
          right[0] * forward[1] - right[1] * forward[0])
    return forward, right, up


def aspect():
    """The viewport's width / height: 32:9 when ow_fixes.asi has it on, else 16:9."""
    ini = Path(__file__).resolve().parents[1] / 'asi' / 'build' / 'ow_fixes.ini'
    on = ini.exists() and 'aspect32x9=1' in ini.read_text(encoding='utf-8', errors='replace').lower().replace(' ', '')
    return 32 / 9 if on else 16 / 9


def projector(state, width, height, ratio=16 / 9):
    """A function taking a world point to (x, y, pixels per metre) on a width x height client area, or None
    behind the near plane. The viewport is the largest centred `ratio` rectangle in the area."""
    forward, right, up = camera(state['yaw'], state['pitch'])
    eye = state['eye']
    focal = min(height, width / ratio) / 2 * VERTICAL_SCALE

    def project(point):
        d = [p - e for p, e in zip(point, eye)]
        z = sum(a * b for a, b in zip(d, forward))
        if z < NEAR:
            return None
        x, y = sum(a * b for a, b in zip(d, right)), sum(a * b for a, b in zip(d, up))
        return width / 2 + focal * x / z, height / 2 - focal * y / z, focal / z
    return project


def shapes(state, width, height, ratio=16 / 9):
    """The overlay as ('circle', x, y, r, colour) and ('line', x0, y0, x1, y1, colour) in client pixels."""
    project = projector(state, width, height, ratio)
    out = []
    for bot in state['bots']:
        for ax, ay, az, bx, by, bz, radius, critical in bot:
            colour = CRITICAL if critical else BODY
            a, b = project((ax, ay, az)), project((bx, by, bz))
            if a is None or b is None:
                continue
            ra, rb = radius * a[2], radius * b[2]
            out.append(('circle', a[0], a[1], ra, colour))
            dx, dy = b[0] - a[0], b[1] - a[1]
            length = math.hypot(dx, dy)
            if length > 1:  # a capsule: the far end and the two side lines
                out.append(('circle', b[0], b[1], rb, colour))
                nx, ny = -dy / length, dx / length
                for side in (1, -1):
                    out.append(('line', a[0] + nx * ra * side, a[1] + ny * ra * side,
                                b[0] + nx * rb * side, b[1] + ny * rb * side, colour))
    return out


def caption(state):
    x, y, z = state['eye']
    return f"server eye {x:.2f} {y:.2f} {z:.2f}  yaw {state['yaw']}  pitch {state['pitch']}  frame {state['frame']}"


def listen(port):
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(('127.0.0.1', port))
    return sock


def newest(sock):
    """The last datagram waiting on the non-blocking socket, or None."""
    state = None
    while True:
        try:
            state = json.loads(sock.recv(65536))
        except BlockingIOError:
            return state


def snapshot(port, out):
    import screengrab
    from PIL import ImageDraw
    sock = listen(port)
    sock.settimeout(10)
    state = json.loads(sock.recv(65536))  # blocking: a fresh state, then the frame
    image = screengrab.grab(screengrab.user32.FindWindowW('tankWindowClass', None))
    sock.setblocking(False)
    state = newest(sock) or state
    draw = ImageDraw.Draw(image)
    for kind, *rest, colour in shapes(state, image.width, image.height, aspect()):
        if kind == 'circle':
            x, y, r = rest
            draw.ellipse((x - r, y - r, x + r, y + r), outline=colour, width=2)
        else:
            draw.line(rest, fill=colour, width=2)
    draw.text((8, 8), caption(state), fill=TEXT)
    image.save(out)
    print(caption(state))


def overlay(port, always, drift):
    import tkinter as tk
    user32 = ctypes.WinDLL('user32')
    user32.SetProcessDPIAware()
    GWL_EXSTYLE = -20
    EX = 0x80000 | 0x20 | 0x8000000 | 0x80  # layered, transparent (click-through), no-activate, tool window
    key = '#010101'
    root = tk.Tk()
    root.overrideredirect(True)
    root.attributes('-topmost', True)
    root.attributes('-transparentcolor', key)
    canvas = tk.Canvas(root, bg=key, highlightthickness=0)
    canvas.pack(fill='both', expand=True)
    root.update_idletasks()
    frame = user32.GetParent(root.winfo_id()) or root.winfo_id()
    user32.SetWindowLongW(frame, GWL_EXSTYLE, user32.GetWindowLongW(frame, GWL_EXSTYLE) | EX)
    sock = listen(port)
    sock.setblocking(False)
    state, ratio, seen = None, aspect(), {}  # seen: client command frame -> feet
    if drift:
        import threading
        import time
        from client_position import ClientMover, game_pid
        client = ClientMover(game_pid())

        def poll():  # its own thread: finding the client's mover blocks for seconds
            while True:
                sample = client.read()
                if sample is not None:
                    seen[sample[0]] = sample[1]
                    seen.pop(sample[0] - 1024, None)
                time.sleep(0.0005)

        threading.Thread(target=poll, daemon=True).start()
    shown = ''

    def tick():
        nonlocal state, shown
        state = newest(sock) or state
        game = user32.FindWindowW('tankWindowClass', None)
        if not game or (not always and user32.GetForegroundWindow() != game) or state is None:
            root.withdraw()
            root.after(100, tick)
            return
        rect, origin = wt.RECT(), wt.POINT(0, 0)
        user32.GetClientRect(game, ctypes.byref(rect))
        user32.ClientToScreen(game, ctypes.byref(origin))
        root.deiconify()
        root.geometry(f'{rect.right}x{rect.bottom}+{origin.x}+{origin.y}')
        canvas.delete('all')
        for kind, *rest, colour in shapes(state, rect.right, rect.bottom, ratio):
            if kind == 'circle':
                x, y, r = rest
                canvas.create_oval(x - r, y - r, x + r, y + r, outline=colour, width=2)
            else:
                canvas.create_line(*rest, fill=colour, width=2)
        text = caption(state)
        if drift:
            from client_position import feet
            position = seen.get(state['frame'])
            if position is not None:
                d = [c - s for c, s in zip(position, feet(state))]
                shown = f'  client drift {d[0]:+.2f} {d[1]:+.2f} {d[2]:+.2f} m'
            text += shown or '  client drift: finding the client'
        canvas.create_text(8, 8, text=text, fill=TEXT, anchor='nw')
        root.after(16, tick)

    tick()
    root.mainloop()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--port', type=int, default=PORT)
    parser.add_argument('--snapshot', metavar='OUT.png')
    parser.add_argument('--always', action='store_true', help='keep the overlay up when the game is not focused')
    parser.add_argument('--drift', action='store_true', help="show the server's drift from the client's position")
    args = parser.parse_args(argv)
    if args.snapshot:
        snapshot(args.port, args.snapshot)
    else:
        overlay(args.port, args.always, args.drift)


if __name__ == '__main__':
    main()
