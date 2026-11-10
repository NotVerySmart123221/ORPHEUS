import struct
import unittest

import stu_graph
import stu_names


class StuGraphTests(unittest.TestCase):
    def test_instances_reads_field_bag(self):
        # header: 1 instance, table at 36; 1 bag, bag table at 52; cursor (instance data) at 68
        header = struct.pack("<9I", 1, 36, 0, 0, 1, 52, 0, 0, 68)
        table = struct.pack("<4I", 0x75001A77, 0, 0, 12)
        bag = struct.pack("<II", 1, 60) + struct.pack("<II", 0x07DD813E, 8)
        data = struct.pack("<i", 0) + struct.pack("<Q", 42)
        rows = stu_graph.instances(header + table + bag + data)
        self.assertEqual(rows, [(0x75001A77, 68, 12, {0x07DD813E: struct.pack("<Q", 42)})])

    def test_names_parse_class_and_field(self):
        text = """[STU(0x75001A77, 136)]
    public class STUStatescriptActionSetVar : STUStatescriptAction
    {
        [STUField(0x07DD813E, 128, ReaderType = typeof(EmbeddedInstanceFieldReader))] // size: 8
        public STUConfigVar m_value;
    }"""
        entry = stu_names.parse(text)["75001A77"]
        self.assertEqual(entry["name"], "STUStatescriptActionSetVar")
        self.assertEqual(entry["fields"]["07DD813E"], ["m_value", "STUConfigVar"])


if __name__ == "__main__":
    unittest.main()
