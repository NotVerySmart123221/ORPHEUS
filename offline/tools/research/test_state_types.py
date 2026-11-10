import struct
import unittest

from state_types import registry_entries


class StateRegistryTests(unittest.TestCase):
    def fixture(self):
        memory = {
            0x100: struct.pack('<16xIIQ', 2, 2, 0x200),
            0x200: struct.pack('<QQ', 0x300, 0),
            0x300: struct.pack('<QQQQ', 0x400, 0x500, 0, 0x700),
            0x400: struct.pack('<QQQQ', 0, 0x600, 0, 0x800),
        }
        def read(address, count):
            data = memory[address]
            if len(data) != count:
                raise ValueError('bad read')
            return data
        return memory, read

    def test_enumerates_collision_chain(self):
        _, read = self.fixture()
        self.assertEqual(registry_entries(read, 0x100), [(0x700, 0x500), (0x800, 0x600)])

    def test_rejects_cyclic_or_inconsistent_registry(self):
        memory, read = self.fixture()
        memory[0x400] = struct.pack('<QQQQ', 0x300, 0x600, 0, 0x800)
        with self.assertRaisesRegex(ValueError, 'cycle'):
            registry_entries(read, 0x100)
        memory[0x400] = struct.pack('<QQQQ', 0, 0x600, 0, 0x800)
        memory[0x100] = struct.pack('<16xIIQ', 2, 1, 0x200)
        with self.assertRaises(ValueError):
            registry_entries(read, 0x100)

    def test_rejects_unbounded_capacity_before_reading_buckets(self):
        _, read = self.fixture()
        with self.assertRaises(ValueError):
            registry_entries(lambda a, n: struct.pack('<16xIIQ', 8192, 2, 0x200), 0x100)


if __name__ == '__main__':
    unittest.main()
