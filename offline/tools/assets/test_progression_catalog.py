import hashlib
import struct
import unittest
from unittest.mock import patch

from progression_catalog import assets, unlock_refs


class CatalogTests(unittest.TestCase):
    def test_bundled_asset_is_verified_against_its_own_content_hash(self):
        item, bundle = 0x0250000000000001, 0x0990000000000001
        payload, root = b'valid item', b'manifest'
        key = hashlib.md5(payload).digest()
        for contents, valid in ((payload, True), (b'wrong item', False)):
            data = struct.pack('<iBQI', 1, 4, item, 17) + contents
            bundle_key = hashlib.md5(data).digest()
            root_key = hashlib.md5(root).digest()
            storage = type('Storage', (), {'cat': lambda self, k: {root_key: root, bundle_key: data}.get(k)})()
            with patch('progression_catalog.root_manifest_key', return_value=(root_key, 'root.cmf')), \
                    patch('progression_catalog.parse_cmf', return_value=((), [(item, len(payload), key),
                                                                            (bundle, len(data), bundle_key)])), \
                    patch('progression_catalog.resource_type', return_value=0x09C):
                if valid:
                    self.assertEqual(assets(storage, [item]), {item: payload})
                else:
                    with self.assertRaisesRegex(ValueError, 'failed content verification'):
                        assets(storage, [item])

    def test_unlock_arrays_reject_bad_reference_or_out_of_bounds_count(self):
        with self.assertRaises(ValueError):
            unlock_refs(bytes(64), b'bad')
        blob = struct.pack('<9I', 0, 0, 0, 0, 0, 0, 0, 36, 0) + struct.pack('<QQ', 10001, 0)
        with self.assertRaisesRegex(ValueError, 'exceeds'):
            unlock_refs(blob, bytes(4))


if __name__ == '__main__':
    unittest.main()
