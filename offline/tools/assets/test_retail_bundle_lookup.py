import struct
import unittest
import hashlib
from unittest.mock import Mock, patch

from retail_bundle_lookup import (
    STU_HERO,
    STU_HERO_GAMEPLAY_ENTITY,
    hero_gameplay_entity_guid,
    find_asset_payload,
    parse_bundle_index,
    stu_header_crc64,
)


class BundleIndexTests(unittest.TestCase):
    def test_loose_asset_requires_manifest_size_and_hash(self):
        import retail_bundle_lookup as lookup
        guid, root, payload = 0x058000000000114D, b'root', b'asset'
        root_key, key = hashlib.md5(root).digest(), hashlib.md5(payload).digest()
        storage = Mock()
        with patch.object(lookup, 'root_manifest_key', return_value=(root_key, 'root')), \
                patch.object(lookup, 'parse_cmf', return_value=(None, [(guid, len(payload), key)])):
            storage.cat.side_effect = lambda candidate: root if candidate == root_key else payload
            result = find_asset_payload(storage, guid)
            self.assertEqual((result.payload, result.bundle_guid, result.offset), (payload, 0, 0))
            for invalid in (b'wrong', payload + b'extra'):
                storage.cat.side_effect = lambda candidate: root if candidate == root_key else invalid
                with self.assertRaises(LookupError):
                    find_asset_payload(storage, guid)

    def test_parse_guid_offset_entries(self):
        payload = b"PAYLOAD"
        offset = 5 + 2 * 12
        bundle = struct.pack("<iB", 2, 4)
        bundle += struct.pack("<Q", 0x02E000000000016B) + offset.to_bytes(4, "little")
        bundle += struct.pack("<Q", 0x0400000000000001) + (offset + len(payload)).to_bytes(4, "little")
        bundle += payload
        self.assertEqual(
            parse_bundle_index(bundle),
            {0x02E000000000016B: offset, 0x0400000000000001: offset + len(payload)},
        )

    def test_rejects_bad_offset_width_and_truncated_table(self):
        with self.assertRaises(ValueError):
            parse_bundle_index(struct.pack("<iB", 0, 3))
        with self.assertRaises(ValueError):
            parse_bundle_index(struct.pack("<iB", 1, 4))

    def test_rejects_offset_into_index(self):
        bundle = struct.pack("<iB", 1, 4)
        bundle += struct.pack("<Q", 1) + (4).to_bytes(4, "little")
        with self.assertRaises(ValueError):
            parse_bundle_index(bundle)

    def test_asset_guid_must_fit_unsigned_64_bits(self):
        for guid in (-1, 1 << 64, True, False, 1.0):
            with self.subTest(guid=guid), self.assertRaises(ValueError):
                find_asset_payload(None, guid)


class HeroReferenceTests(unittest.TestCase):
    def test_crc64_and_decode_gameplay_entity_reference(self):
        header = struct.pack("<IIIIIIIII", 1, 36, 0, 52, 1, 52, 0, 0, 68)
        instance = struct.pack("<IIii", STU_HERO, 0, -1, 12)
        field_bag = struct.pack("<II", 1, 60)
        field_info = struct.pack("<II", STU_HERO_GAMEPLAY_ENTITY, 8)
        partial = header + instance + field_bag + field_info
        checksum = stu_header_crc64(partial)
        expected = 0x0400000000001403
        encoded = expected.to_bytes(8, "little")
        for left, right in ((0, 3), (7, 1), (2, 6), (4, 5)):
            encoded = bytearray(encoded)
            encoded[left], encoded[right] = encoded[right], encoded[left]
        raw = int.from_bytes(encoded, "little") ^ (STU_HERO_GAMEPLAY_ENTITY * 0x100000001) ^ checksum
        blob = partial + struct.pack("<iQ", 0, raw)
        self.assertEqual(hero_gameplay_entity_guid(blob), expected)
        self.assertEqual(stu_header_crc64(header), checksum)

    def test_rejects_non_hero_root_type(self):
        header = struct.pack("<IIIIIIIII", 1, 36, 0, 52, 1, 52, 0, 0, 68)
        blob = header + struct.pack("<IIii", 1, 0, -1, 12) + bytes(20)
        with self.assertRaisesRegex(ValueError, "not STUHero"):
            hero_gameplay_entity_guid(blob)


if __name__ == "__main__":
    unittest.main()
