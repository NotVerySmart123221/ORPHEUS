import hashlib
import hmac
import socket
import struct
import threading
import unittest

import lobby
from panama import Jam


class LobbyTests(unittest.TestCase):
    def test_cipher_known_answer_and_split_stream(self):
        expected = bytes.fromhex(
            "f07e5df1c8d51c0d754da6ddde34cca01db0ed26334eb94ae277135a0764c6a6"
            "794d7ab0cc0978ef43de30766a7ee1d7c90e159df239d4016d450fa3d8207842")
        cipher = Jam(bytes(range(32)))
        self.assertEqual(cipher.crypt(bytes(7)) + cipher.crypt(bytes(57)), expected)

    def test_blob_channel_and_mac(self):
        self.assertEqual(len(lobby.blob_key()), 64)
        blob = lobby.state_blob(123, 3724)
        self.assertEqual(len(blob), 292)
        peer = bytes.fromhex("7b00000000050001efbe000000000000")
        channel = bytes.fromhex("7b000000000d000100008c0e7f000001")
        self.assertEqual(blob[256:], peer + channel + struct.pack('<I', 1))
        self.assertEqual(blob[176:208], hmac.digest(lobby.blob_key(), blob[:176] + blob[208:256], 'sha256'))
        self.assertNotEqual(lobby.state_blob(124, 3724)[256:], blob[256:])

    def test_groups_use_client_order_and_validate_lengths(self):
        groups = [lobby.IN_CONNECT, lobby.OUT_CONNECT, 0x779A8581]
        payload = b'\0\0' + struct.pack('<I3I', 3, *groups)
        self.assertEqual(lobby.protocols(payload), {crc: i + 1 for i, crc in enumerate(groups)})
        for bad in (payload[:-1], payload + b'\0', b'\0\0' + struct.pack('<I', 10000)):
            with self.assertRaises(ValueError):
                lobby.protocols(bad)

    def test_menu_records_match_documented_wire_lengths(self):
        records = lobby.login_records(83, (0x02E000000000006E,))
        self.assertEqual([len(p) for p in records], [76, 73, 10, 42])
        self.assertEqual(records[2], bytes.fromhex('53040000000000000000'))
        self.assertEqual(records[3], b'\x53\x05' + struct.pack('<IQIIQIQ',
            1, 0x02E000000000006E, 0, 1, 0x0790000000000688, 1, 0x0308000000000001))
        self.assertEqual(records[0][:2], bytes.fromhex('5300'))

    def test_custom_name_reaches_each_login_name_field(self):
        for name in ('A', 'Ranger7', 'ABCDEFGHIJKL'):
            with self.subTest(name=name):
                original = lobby.login_records(83)
                records = lobby.login_records(83, name=name)
                wire_name = name.encode('ascii') + b'\0'
                self.assertEqual(records[0][50:].split(b'\0', 2)[:2], [wire_name[:-1]] * 2)
                self.assertEqual(records[1][66:], wire_name)
                self.assertEqual(records[0][:50], original[0][:50])
                self.assertEqual(records[1][:66], original[1][:66])
                self.assertEqual(records[2:], original[2:])

    def test_skin_themes_fill_the_second_content_list(self):
        import json
        import tempfile
        from pathlib import Path
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'hero_content.json'
            path.write_text(json.dumps({'02E000000000006E': {'skin_themes': ['0A50000000000001', '0A50000000000002'],
                                                             'unlocks': []}}))
            skins = lobby.skin_themes((0x02E000000000006E, 0x02E000000000016B), lobby.hero_content(path))
            self.assertEqual(lobby.hero_content(Path(folder) / 'none.json'), {})
        self.assertEqual(skins, [0x0A50000000000001, 0x0A50000000000002])
        content = lobby.login_records(83, (0x02E000000000006E,), skins=skins)[3]
        self.assertEqual(content, b'\x53\x05' + struct.pack('<IQI2QIQIQ', 1, 0x02E000000000006E, 2, *skins,
            1, 0x0790000000000688, 1, 0x0308000000000001))

    def test_content_keys_fill_the_keyring_record(self):
        keys = lobby.content_keys()
        self.assertIn(0x28B8EBB462B83EEA, dict(keys))  # 1.74 farewell
        record = lobby.login_records(83, keys=keys[:2])[2]
        self.assertEqual(record, b'\x53\x04' + struct.pack('<I2QI', 2, keys[0][0], keys[1][0], 32) + keys[0][1] + keys[1][1])

    def test_framing_rejects_short_and_oversized_payloads(self):
        for length in (0, 1, lobby.MAX_FRAME + 1):
            with self.assertRaises(ValueError):
                lobby.frame(bytes(length))
        self.assertEqual(lobby.frame(b'\0\2'), bytes.fromhex('0000020002'))

    def test_default_content_includes_range_bots_without_changing_other_lists(self):
        heroes = (0x02E000000000006E, 0x02E000000000016B, 0x02E000000000016C, 0x02E000000000016D,
            0x02E000000000016E)
        content = lobby.login_records(83)[3]
        self.assertEqual(content[:46], b'\x53\x05' + struct.pack('<I5Q', 5, *heroes))
        self.assertEqual(content[46:], lobby.login_records(83, (heroes[0],))[3][14:])

    def test_complete_handshake_and_login_over_socket(self):
        server, client = socket.socketpair()
        errors = []
        def run():
            try:
                lobby.handle(server, 3724)
            except (ConnectionError, OSError):
                pass
            except Exception as exc:
                errors.append(exc)
            finally:
                server.close()
        thread = threading.Thread(target=run)
        thread.start()
        client.settimeout(3)
        try:
            client.sendall(lobby.HELLO_CLIENT)
            self.assertEqual(lobby.recvn(client, 17), lobby.HELLO_SERVER)
            cn = bytes(range(32))
            client.sendall(bytes(8) + cn)
            challenge = lobby.recvn(client, 65)
            sn = challenge[33:]
            client.sendall(struct.pack('<Q', 1) + hmac.digest(bytes(32), cn + sn, 'sha256'))
            proof = lobby.recvn(client, 32)
            self.assertEqual(proof, hmac.digest(bytes(32), sn + cn, 'sha256'))
            tx = Jam(hmac.digest(bytes(32), sn + cn, 'sha256'))
            rx = Jam(hmac.digest(bytes(32), cn + sn, 'sha256'))
            self.assertEqual(len(rx.crypt(lobby.recvn(client, 292))), 292)
            groups = struct.pack('<I2I', 2, lobby.OUT_CONNECT, lobby.IN_CONNECT)
            announcement = tx.crypt(lobby.frame(b'\0\0' + groups))
            client.sendall(announcement[:4])
            client.sendall(announcement[4:])
            self.assertEqual(rx.crypt(lobby.recvn(client, 5)), lobby.frame(b'\0\2'))
            client.sendall(tx.crypt(lobby.frame(b'\1\0Player\0')))
            for expected in lobby.login_records(2, skins=lobby.skin_themes(lobby.LOGIN_HEROES, lobby.hero_content()),
                                                keys=lobby.content_keys()):
                size = int.from_bytes(rx.crypt(lobby.recvn(client, 3)), 'big')
                self.assertEqual(rx.crypt(lobby.recvn(client, size)), expected)
            client.sendall(tx.crypt(lobby.frame(b'\0\3')))
            self.assertEqual(rx.crypt(lobby.recvn(client, 5)), lobby.frame(b'\0\3'))
        finally:
            client.close()
            thread.join(4)
        self.assertFalse(thread.is_alive())
        self.assertFalse(errors, errors)


if __name__ == '__main__':
    unittest.main()
