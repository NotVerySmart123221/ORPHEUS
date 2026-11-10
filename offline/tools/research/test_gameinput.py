import unittest

import gameinput


class GameInputTests(unittest.TestCase):
    def test_key_names(self):
        self.assertEqual(gameinput.vk("enter"), 0x0D)
        self.assertEqual(gameinput.vk("r"), ord("R"))
        self.assertEqual(gameinput.vk("0x70"), 0x70)

    def test_key_lparam_marks_release(self):
        down = gameinput.key_lparam(0x0D, False)
        up = gameinput.key_lparam(0x0D, True)
        self.assertEqual(down & 0xC0000000, 0)
        self.assertEqual(up & 0xC0000000, 0xC0000000)
        self.assertEqual(down & 1, 1)


if __name__ == "__main__":
    unittest.main()
