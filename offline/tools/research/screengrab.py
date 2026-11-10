"""Capture the game window's client area, even when other windows cover it.

  python screengrab.py out.png [maxwidth]

Uses PrintWindow with PW_RENDERFULLCONTENT, which reads the DWM copy of the
window instead of the screen. A minimized window cannot be captured.
When DWM hands back an all-black frame (seen once with the user away; cause not
reproduced), grab() asks tools/asi/frame_grab.asi for the game's back buffer
instead, and raises BlackFrame when that plugin is not loaded.
"""
import ctypes
import ctypes.wintypes as wt
import os
from pathlib import Path
import sys
import tempfile
import time

from PIL import Image

PW_CLIENTONLY_RENDERFULLCONTENT = 1 | 2

user32 = ctypes.WinDLL("user32")
gdi32 = ctypes.WinDLL("gdi32")
user32.SetProcessDPIAware()
PLUGIN_REQUEST = Path(__file__).resolve().parents[1] / 'asi' / 'build' / 'frame_grab.req'
PLUGIN_WAIT = 3  # seconds; the plugin answers on the game's next Present


class BlackFrame(Exception):
    """Not a RuntimeError on purpose: callers that retry PrintWindow failures must not swallow this."""


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [("biSize", wt.DWORD), ("biWidth", wt.LONG), ("biHeight", wt.LONG),
                ("biPlanes", wt.WORD), ("biBitCount", wt.WORD), ("biCompression", wt.DWORD),
                ("biSizeImage", wt.DWORD), ("biXPelsPerMeter", wt.LONG),
                ("biYPelsPerMeter", wt.LONG), ("biClrUsed", wt.DWORD), ("biClrImportant", wt.DWORD)]


def grab(hwnd):
    img = print_window(hwnd)
    return img if img.getbbox() else plugin_frame()


def plugin_frame():
    """The game's back buffer from frame_grab.asi (request file in, BMP out)."""
    out = Path(tempfile.gettempdir()) / f'frame_grab_{os.getpid()}.bmp'
    out.unlink(missing_ok=True)
    PLUGIN_REQUEST.write_text(str(out))
    deadline = time.monotonic() + PLUGIN_WAIT
    while time.monotonic() < deadline:
        if out.exists():
            img = Image.open(out).convert('RGB')
            out.unlink()
            return img
        time.sleep(0.02)
    PLUGIN_REQUEST.unlink(missing_ok=True)
    raise BlackFrame('PrintWindow gave an all-black frame and frame_grab.asi did not answer '
                     '(load it: python tools/asi/load_asi.py PID tools/asi/build/frame_grab.asi)')


def print_window(hwnd):
    if user32.IsIconic(hwnd):
        raise RuntimeError("game window is minimized")
    rect = wt.RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    w, h = rect.right, rect.bottom
    screen = user32.GetDC(None)
    dc = gdi32.CreateCompatibleDC(screen)
    bmp = gdi32.CreateCompatibleBitmap(screen, w, h)
    old = gdi32.SelectObject(dc, bmp)
    try:
        if not user32.PrintWindow(hwnd, dc, PW_CLIENTONLY_RENDERFULLCONTENT):
            raise RuntimeError("PrintWindow failed")
        header = BITMAPINFOHEADER(ctypes.sizeof(BITMAPINFOHEADER), w, -h, 1, 32, 0)
        buf = ctypes.create_string_buffer(w * h * 4)
        if gdi32.GetDIBits(dc, bmp, 0, h, buf, ctypes.byref(header), 0) != h:
            raise RuntimeError("GetDIBits failed")
        return Image.frombuffer("RGB", (w, h), buf, "raw", "BGRX", 0, 1)
    finally:
        gdi32.SelectObject(dc, old)
        gdi32.DeleteObject(bmp)
        gdi32.DeleteDC(dc)
        user32.ReleaseDC(None, screen)


def main():
    hwnd = user32.FindWindowW("tankWindowClass", None)
    if not hwnd:
        sys.exit("game window not found")
    try:
        img = grab(hwnd)
    except (RuntimeError, BlackFrame) as exc:
        sys.exit(str(exc))
    if len(sys.argv) > 2:
        w = int(sys.argv[2])
        if img.width > w:
            img = img.resize((w, int(img.height * w / img.width)))
    img.save(sys.argv[1])
    print("saved", sys.argv[1], img.size)


if __name__ == "__main__":
    main()
