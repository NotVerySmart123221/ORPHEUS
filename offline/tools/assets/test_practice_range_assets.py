import struct
import unittest

from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes

from practice_range_assets import cmf_iv, cmf_key, parse_cmf, resource_type


class CMFTests(unittest.TestCase):
    def test_104319_key_and_iv_vectors(self):
        self.assertEqual(
            cmf_key(104319).hex(),
            "0df4735032f5f57958de8bfec75f4f05e214aa25bf4349f1bde9c01de5461600",
        )
        self.assertEqual(
            cmf_iv("Win_SPWin_RDEV_EExt.cmf").hex(),
            "da57764d77f843be21073b60dbc3246f",
        )

    def test_parse_cmf_hash_record(self):
        filename = "Win_SPWin_RDEV_EExt.cmf"
        ckey = bytes(range(16))
        plain = struct.pack("<QIB", 0x0790000000000688, 372, 0) + ckey
        plain += bytes(16 - len(plain) % 16)
        encryptor = Cipher(
            algorithms.AES(cmf_key(104319)),
            modes.CBC(cmf_iv(filename)),
        ).encryptor()
        encrypted = encryptor.update(plain) + encryptor.finalize()
        fields = [0] * 12
        fields[0] = 104319
        fields[8] = 1
        fields[11] = 0x636D661B
        blob = struct.pack("<12I", *fields) + encrypted

        parsed_fields, records = parse_cmf(blob, filename)

        self.assertEqual(parsed_fields[8], 1)
        self.assertEqual(records, [(0x0790000000000688, 372, ckey)])

    def test_mapheader_guid_type(self):
        self.assertEqual(resource_type(0x0790000000000688), 0x09F)
        self.assertEqual(resource_type(0x0800000000000688), 0x002)


if __name__ == "__main__":
    unittest.main()
