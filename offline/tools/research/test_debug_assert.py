import ctypes
import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from debug_assert import (
    CONTEXT_SIZE,
    CONTEXT_RIP_OFFSET,
    CONTEXT_RSP_OFFSET,
    DEBUG_EVENT,
    EXCEPTION_RECORD,
    MODULEENTRY32W,
    filtered_return_rvas,
    module_rva,
    parse_context_buffer,
    validate_pid,
)


class DebugAssertTests(unittest.TestCase):
    def test_windows_x64_layout_matches_expected_sizes_and_register_offsets(self):
        self.assertEqual(CONTEXT_SIZE, 0x4D0)
        self.assertEqual(CONTEXT_RSP_OFFSET, 0x98)
        self.assertEqual(CONTEXT_RIP_OFFSET, 0xF8)
        self.assertEqual(ctypes.sizeof(EXCEPTION_RECORD), 152)
        self.assertEqual(ctypes.sizeof(DEBUG_EVENT), 176)
        self.assertEqual(ctypes.sizeof(MODULEENTRY32W), 1080)
        self.assertEqual(MODULEENTRY32W.modBaseAddr.offset, 24)
        self.assertEqual(MODULEENTRY32W.modBaseSize.offset, 32)
        self.assertEqual(MODULEENTRY32W.szExePath.offset, 560)

    def test_parses_rip_rsp_from_mock_context_bytes(self):
        raw = bytearray(CONTEXT_SIZE)
        struct.pack_into("<Q", raw, CONTEXT_RSP_OFFSET, 0x000000ABCD000000)
        struct.pack_into("<Q", raw, CONTEXT_RIP_OFFSET, 0x00007FF612345678)
        self.assertEqual(parse_context_buffer(raw), (0x00007FF612345678, 0x000000ABCD000000))

    def test_filters_stack_words_to_main_module_rvas(self):
        base, size = 0x00007FF600000000, 0x1000
        raw = struct.pack("<QQQQ", base + 0x20, 0x1000, base + size, base + 0xABC)
        self.assertEqual(filtered_return_rvas(raw, base, size), [0x20, 0xABC])
        self.assertEqual(module_rva(base + 0x20, base, size), 0x20)
        self.assertIsNone(module_rva(base + size, base, size))

    def test_rejects_invalid_process_ids(self):
        for pid in (0, -1, 0x100000000, True, "42"):
            with self.subTest(pid=pid), self.assertRaises(ValueError):
                validate_pid(pid)
        self.assertEqual(validate_pid(42), 42)


if __name__ == "__main__":
    unittest.main()
