import unittest

from asset_users import encoded
from retail_bundle_lookup import _decode_asset_ref


class EncodedTests(unittest.TestCase):
    def test_the_stored_bytes_decode_back_to_the_guid(self):
        for guid, field, crc in ((0x05800000000013C0, 0xCF63B633, 0x0123456789ABCDEF),
                                 (0x05800000000012B8, 0x76E8C82A, 0xFEDCBA9876543210)):
            raw = int.from_bytes(encoded(guid, field, crc), 'little')
            self.assertEqual(_decode_asset_ref(raw, field, crc), guid)


if __name__ == '__main__':
    unittest.main()
