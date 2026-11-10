import struct
import unittest

import stu_refs
from retail_bundle_lookup import stu_header_crc64


def mangle(guid, field, crc):
    raw = bytearray(guid.to_bytes(8, "little"))
    for a, b in ((0, 3), (7, 1), (2, 6), (4, 5)):
        raw[a], raw[b] = raw[b], raw[a]
    return (int.from_bytes(raw, "little") ^ (field | field << 32) ^ crc).to_bytes(8, "little")


class StuRefsTests(unittest.TestCase):
    def test_native_helix_model_and_effect_references_are_not_filtered_out(self):
        for field, guid in ((0x2D124BE2, 0x0D00000000001883), (0x6CB564CC, 0x0300000000000976)):
            with self.subTest(guid=hex(guid)):
                header = struct.pack('<9I', 1, 0, 0, 0, 1, 36, 0, 0, 0)
                header += struct.pack('<II', 1, 44) + struct.pack('<II', field, 0)
                blob = bytearray(header + bytes(24))
                blob[68:76] = mangle(guid, field, stu_header_crc64(bytes(blob)))
                self.assertIn((68, field, guid), stu_refs.decode_refs(bytes(blob)))

    def test_round_trip_through_field_bag(self):
        field, guid = 0x950BBA06, 0x0400000000001403
        # bag table at 36 -> one bag {count 1, entries at 44}; entry {field hash, 0}
        header = struct.pack("<9I", 1, 0, 0, 0, 1, 36, 0, 0, 0) + struct.pack("<II", 1, 44) + struct.pack("<II", field, 0)
        blob = bytearray(header + bytes(16))
        crc = stu_header_crc64(bytes(blob))
        blob[68:76] = mangle(guid, field, crc)
        self.assertIn((68, field, guid), stu_refs.decode_refs(bytes(blob)))

    def test_plausible_rejects_reserved_bits(self):
        self.assertTrue(stu_refs.plausible(0x0580000000000BA3))
        self.assertFalse(stu_refs.plausible(0x0580000100000BA3))
        self.assertFalse(stu_refs.plausible(0x0FF0000000000001))
        self.assertTrue(stu_refs.plausible(0x0400000000001403))


if __name__ == "__main__":
    unittest.main()
