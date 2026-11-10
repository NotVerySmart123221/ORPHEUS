import unittest

import volley_probe as vp


class ResolveInfoTests(unittest.TestCase):
    def test_lookup_follows_the_client_decoding(self):
        base, entity, world, table, component, pairs, key = 0x1000000, 0x200000, 0x300000, 0x400000, 0x500000, 0x600000, 0x700003
        table_key = 0x1111
        memory = {
            base + vp.TABLE_KEY_RVA: table_key,
            entity + 0x28: world ^ 0x98DE97B50B3BE1E3,
            world + 0x140: table ^ ((0x4E3494C633194824 - table_key) & vp.MASK),
            table + 8 * vp.INFO_COMPONENT: component,
            component + 0x20 + 0x90 * 3 + 8: pairs,
            component + 0x20 + 0x90 * 3 + 16: 2,
            pairs: 0xAAAA, pairs + 8: 0x700013,
            pairs + 16: 0xBBBB, pairs + 24: key,
        }
        qword = lambda a: memory.get(a, 0)
        self.assertEqual(vp.info_map(qword, qword, base, entity), {0x700013: 0xAAAA, key: 0xBBBB})

    def test_state_entity_unmasks_the_entity_pointer(self):
        base, instance, inner, holder = 0x1000000, 0x200000, 0x300000, 0x400000
        memory = {base + vp.ENTITY_KEY_RVA: 1 << 63, instance + 0x10: inner, inner + 0x528: holder,
                  holder + 8: 0x1234 ^ 2 ^ 0xF320E42472A94F14}
        self.assertEqual(vp.state_entity(lambda a: memory.get(a, 0), base, instance), 0x1234)

    def test_shot_records_reads_the_entitys_pending_records(self):
        base, entity, world, table, component, entries = 0x1000000, 0x200000, 0x300000, 0x400000, 0x500000, 0x600000
        table_key, key = 0x1111, 0xB000440100080E01
        second = entries + vp.RECORD_SIZE
        memory = {
            base + vp.TABLE_KEY_RVA: table_key,
            entity + 0x28: world ^ 0x98DE97B50B3BE1E3,
            world + 0x140: table ^ ((0x4E3494C633194824 - table_key) & vp.MASK),
            table + 8 * vp.RECORDS_COMPONENT: component,
            component + 0xB0: entries, component + 0xB8: 2,
            entries + 24: 19, entries + 144: key, entries + 160: 600,
            second + 24: 20, second + 144: key, second + 160: 599 | 1 << 32,  # consumed: byte +164
        }
        qword = lambda a: memory.get(a, 0)
        dword = lambda a: memory.get(a, 0) & 0xFFFFFFFF
        self.assertEqual(vp.shot_records(qword, dword, base, entity), [(19, key, 600, 0), (20, key, 599, 1)])


if __name__ == '__main__':
    unittest.main()
