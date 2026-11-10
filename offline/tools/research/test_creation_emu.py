import struct
import unittest

import creation_emu


class RequestTests(unittest.TestCase):
    def test_request_layout(self):
        raw = creation_emu.request_bytes(0x1111, 0x2222, 0x040000000000072C, tick=1234)
        self.assertEqual(struct.unpack_from('<I', raw, 0)[0], 1234)
        self.assertEqual(len(raw), creation_emu.REQUEST_SIZE)
        self.assertEqual(struct.unpack_from('<QQQ', raw, 0x08)[0], 0x1111)
        self.assertEqual(struct.unpack_from('<Q', raw, 0x20)[0], 0x2222)
        self.assertEqual(struct.unpack_from('<Q', raw, 0x30)[0], 0x040000000000072C)
        self.assertEqual(creation_emu.STATUS - creation_emu.REQUEST, 0xF8)


if __name__ == '__main__':
    unittest.main()
