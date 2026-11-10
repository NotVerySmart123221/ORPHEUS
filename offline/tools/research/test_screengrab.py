import tempfile
import unittest
from pathlib import Path
from unittest import mock

from PIL import Image

import screengrab


class GrabTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        patches = [mock.patch.object(screengrab, 'PLUGIN_REQUEST', self.tmp / 'frame_grab.req'),
                   mock.patch.object(screengrab, 'PLUGIN_WAIT', 0.1)]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)

    def test_lit_frame_passes_through(self):
        with mock.patch.object(screengrab, 'print_window', lambda hwnd: Image.new('RGB', (4, 4), (9, 9, 9))):
            self.assertEqual(screengrab.grab(0).getpixel((0, 0)), (9, 9, 9))

    def test_black_frame_without_plugin_raises_and_cleans_up(self):
        with mock.patch.object(screengrab, 'print_window', lambda hwnd: Image.new('RGB', (4, 4))):
            with self.assertRaises(screengrab.BlackFrame):
                screengrab.grab(0)
        self.assertFalse(screengrab.PLUGIN_REQUEST.exists())

    def test_black_frame_uses_plugin_answer(self):
        def answer(text):  # the plugin's side: write the BMP where the request says
            Path(text).parent.mkdir(exist_ok=True)
            Image.new('RGB', (4, 4), (1, 2, 3)).save(text, 'BMP')
        with mock.patch.object(screengrab, 'print_window', lambda hwnd: Image.new('RGB', (4, 4))), \
                mock.patch.object(Path, 'write_text', lambda self, text: answer(text)):
            self.assertEqual(screengrab.grab(0).getpixel((0, 0)), (1, 2, 3))


if __name__ == '__main__':
    unittest.main()
