import unittest

import hw_watch


class Dr7Tests(unittest.TestCase):
    def test_one_read_write_watch(self):
        # L0 enabled, RW0 = 11, LEN0 = 11 (4 bytes)
        self.assertEqual(hw_watch.dr7_value(1), 0x1 | 0xF << 16)

    def test_four_write_watches(self):
        value = hw_watch.dr7_value(4, write_only=True)
        self.assertEqual(value & 0xFF, 0b01010101)
        for i in range(4):
            self.assertEqual(value >> (16 + 4 * i) & 0xF, 0b1101)

    def test_execute_watches(self):
        # instruction breakpoints: RW and LEN both 00
        self.assertEqual(hw_watch.dr7_value(2, execute=True), 0b0101)

    def test_parse_deref(self):
        self.assertEqual(hw_watch.parse_deref("rsi+0xF8"), ("rsi", 0xF8))
        self.assertEqual(hw_watch.parse_deref("rax"), ("rax", 0))
        with self.assertRaises(ValueError):
            hw_watch.parse_deref("xmm0+8")

    def test_limits(self):
        with self.assertRaises(ValueError):
            hw_watch.dr7_value(5)
        with self.assertRaises(ValueError):
            hw_watch.dr7_value(1, length=2)


if __name__ == "__main__":
    unittest.main()
