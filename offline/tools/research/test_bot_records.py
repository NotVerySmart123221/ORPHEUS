"""bot_records.py on two seconds of the Practice Range: what the server sends about its bots."""
import unittest

import bot_records


class BotRecordTests(unittest.TestCase):
    def test_a_bot_gets_a_record_in_every_datagram_and_none_before_its_create(self):
        found = bot_records.measure(bot_records.PRACTICE_RANGE, 2)
        self.assertEqual(found['before'], 0)  # the client would have no such entity yet
        self.assertLessEqual(max(found['bytes']), bot_records.server.MAX_PAYLOAD)
        # all 15, in each of the 125 frames but the few the creates took
        self.assertGreater(found['records'], 15 * (found['frames'] - 8))
        self.assertLess(found['theirs'], found['records'] / 5)  # LizRollin's rule rests the standing ones


if __name__ == '__main__':
    unittest.main()
