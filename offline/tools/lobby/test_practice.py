from pathlib import Path
import socket
import struct
import tempfile
import unittest

from practice import HANDOFF, REQUEST, PracticeRange
from ow174.jam.codec import Schemas
from ow174.game.bits import BitReader
from ow174.game import messages
from ow174.game.link import ACK, SYN, KIND_FRAME, LinkCipher, Packet
from ow174.game.commands import Command, FIRE, JUMP
from ow174.game.content import PRACTICE_RANGE, SOLDIER
from ow174.game.match import Match
from ow174.game.server import Client
from ow174.game import world


def setUpModule():
    import practice
    practice.EQUIPS = Path(tempfile.mkdtemp()) / 'equips.json'  # never the real saved equips


def test_equipped_places_items_by_slot_and_index():
    from practice import equipped
    entry = {'defaults': {'Skin': '0250000000000001', 'Emote': ['0250000000000002']}}
    saved = [(0x0250000000000003, 0, 'Skin'), (0x0250000000000004, 0, 'Emote'), (0x0250000000000005, 1, 'Spray')]
    assert equipped(entry, saved) == {'Skin': '0250000000000003', 'Emote': ['0250000000000004'],
                                      'Spray': ['0000000000000000', '0250000000000005']}


class PracticeTests(unittest.TestCase):
    def test_custom_name_reaches_profile_party_and_match_roster(self):
        from practice import ACCOUNT, LOBBY, PARTY, SOCIAL_OUT
        practice = PracticeRange(port=0, name='Ranger7')
        _, body = practice.schemas.split(PARTY, practice.party({PARTY: 4}, SOLDIER))
        member, = practice.schemas.decode(PARTY, 20700, body)['+0x78']['+0x0']
        self.assertEqual(member['+0x0']['+0x40'], 'Ranger7')
        self.assertEqual(member['+0x0']['+0x0'], ACCOUNT)
        ask = {'+0x78': {'+0x0': 7, '+0x8': 8}, '+0x88': ACCOUNT}
        request = practice.schemas.header(SOCIAL_OUT, 41, 22206) + practice.schemas.encode(SOCIAL_OUT, 22206, ask)
        _, body = practice.schemas.split(LOBBY, practice.request(request, {SOCIAL_OUT: 41, LOBBY: 20})[-1])
        profile = practice.schemas.decode(LOBBY, 20807, body)['+0x90']
        self.assertEqual((profile['+0x138'], profile['+0x100']), ('Ranger7', ACCOUNT))
        practice._handoff({HANDOFF: 3}, PRACTICE_RANGE)
        match, = practice.game.matches
        player, = match.players
        self.assertEqual((player.name, player.account_lo), ('Ranger7', 1))
        roster = match.game_struct(player)['+0x0']['+0x0']['+0x0']['+0x0']
        self.assertEqual(roster[player.slot]['+0x50'], 'Ranger7')

    def test_gameplay_world_clock_tracks_player_quanta(self):
        match = Match(PRACTICE_RANGE, gameplay=True)
        player = match.add_player(1, 'Player', SOLDIER, 0, True)
        client = Client(None, ('127.0.0.1', 0), 1, None, player)
        # the frame is wall time since the match started in the player's command frames, whatever the tick count
        for tick, seconds, expected in ((1, 0.0, 1000), (2, 0.007, 1001), (2, 0.018, 1003), (2, 7.0, 2000)):
            self.assertEqual(client._frame(tick, match.started + seconds), expected)

        retail_match = Match(PRACTICE_RANGE, gameplay=True)
        retail_player = retail_match.add_player(1, 'Player', SOLDIER, 0, False)
        retail_client = Client(None, ('127.0.0.1', 0), 1, None, retail_player)
        self.assertEqual(retail_client._frame(10, retail_match.started + 0.16), 1010)

    def test_gameplay_ack_uses_latest_command_frame_and_accepts_zero(self):
        match = Match(PRACTICE_RANGE, gameplay=True)
        player = match.add_player(1, 'Player', SOLDIER, 0, True)
        client = Client(None, ('127.0.0.1', 0), 1, None, player)

        def ack_from_payload():
            bits = BitReader(client._payload(client._frame(1, match.started), (), (), ()))
            bits.var_a()
            messages.read_channel(bits)
            return bits.var_a()

        self.assertEqual(player.commands.last_frame, None)
        self.assertEqual(ack_from_payload(), 0)
        player.commands.last_frame = 0
        self.assertEqual(ack_from_payload(), 0)
        player.commands.last_frame = 9
        self.assertEqual(ack_from_payload(), 9)

    def test_disabled_gameplay_payload_is_byte_identical(self):
        match = Match(PRACTICE_RANGE)
        player = match.add_player(1, 'Player', SOLDIER, 0, True)
        client = Client(None, ('127.0.0.1', 0), 1, None, player)
        self.assertEqual(client._payload(client._frame(123, 0.0), (), (), ()), world.frame(123, 123, (), (), ()))

    def test_delta_angles_wrap_as_signed_sixteen_bit_values(self):
        from ow174.game.bits import BitWriter
        from ow174.game.commands import read_commands
        body = BitWriter()
        body.bits(0, 24)
        body.bits(2, 8)
        body.bits(10, 32)
        body.bit(0)
        body.bits(-32764 & 65535, 16)
        body.bits(32764, 16)
        body.bits(0, 48)
        body.bit(0)  # next frame
        body.bit(1)  # changed command
        body.bit(0)  # unchanged throttle
        for delta in (8, -8):  # pitch then yaw
            body.bit(1)
            body.bit(1)  # signed8
            body.bits(delta & 255, 8)
        body.bits(0, 3)  # unchanged extra/buttons/action
        record = BitWriter()
        record.bit(1)
        record.bits(body.count, 32)
        record.append(body)
        commands = read_commands(BitReader(record.getvalue()))
        self.assertEqual((commands[1].yaw, commands[1].pitch), (32764, -32764))

    def test_a_click_carries_its_aim_and_the_view_delay(self):
        from ow174.game.bits import BitWriter
        from ow174.game.commands import FIRE, SUBFRAME, read_commands
        body = BitWriter()
        body.bits(0, 24)
        body.bits(2, 8)
        body.bits(10, 32)
        body.bit(0)  # no throttles
        body.bits(200, 16)  # yaw
        body.bits(0, 16)  # pitch
        body.bits(FIRE | SUBFRAME, 24)
        body.bits(0, 8)  # action
        body.bits(128, 8)  # the click came 128/255 into the frame
        body.bits(100, 16)  # ... aimed at yaw 100
        body.bits(-50 & 65535, 16)  # ... and pitch -50
        body.bits(40, 16)  # extra: the view delay, ms
        body.bit(0)  # next frame
        body.bit(1)  # changed command
        body.bit(0)  # unchanged throttle
        body.bits(0, 2)  # unchanged pitch and yaw
        body.bit(1)  # extra += 5
        body.bit(1)
        body.bits(5, 5)
        body.bit(1)  # buttons: fire held, no click
        body.bits(FIRE, 24)
        body.bit(0)  # unchanged action
        record = BitWriter()
        record.bit(1)
        record.bits(body.count, 32)
        record.append(body)
        click, held = read_commands(BitReader(record.getvalue()))
        self.assertEqual((click.aim(), click.subframe, click.extra), ((100, -50), 128, 40))
        self.assertAlmostEqual(click.view_delay(16000), 0.048)  # 40 ms + round((1 - 128/255) * 16) ms
        self.assertEqual((held.aim(), held.extra, held.click_yaw), ((200, 0), 45, None))
        self.assertAlmostEqual(held.view_delay(16000), 0.045)  # no click: the frame's end

    def test_input_deduplication_and_diagnostics(self):
        player = Match(PRACTICE_RANGE).add_player(1, 'Player', SOLDIER, 0, True)
        player.has_body = True
        command = Command(1, forward=127, buttons=FIRE | JUMP)
        before = tuple(player.mover.position)
        with self.assertLogs('ow174.game', level='INFO') as logs:
            player.take_commands([command])
            after = tuple(player.mover.position)
            player.take_commands([command])
        self.assertNotEqual(before, after)  # server copy only; client movement needs runtime proof
        self.assertEqual(tuple(player.mover.position), after)
        self.assertEqual(len(logs.output), 1)
        self.assertIn('buttons=000041', logs.output[0])

    def test_sombra_hack_pose_tracks_a_living_aimed_enemy(self):
        from kits import SOMBRA
        from ow174.game.commands import SECONDARY
        from ow174.game.statescript import Stream
        from target import Ability, range_bots

        match = Match(PRACTICE_RANGE, gameplay=True)
        match.bots = range_bots()
        player = match.add_player(1, 'Player', SOMBRA, 0, True)
        player.has_body = True
        player.body_script = Stream(player.body)
        enemy = next(bot for bot in match.bots if bot.team != player.team)
        x, y, z = player.mover.position
        enemy.position = (x, y, z + 4.5)
        player.bots_seen.add(enemy.entity)
        route = {21, 25, 33, 60}
        player.take_commands([Command(1, buttons=SECONDARY)])
        self.assertEqual(player.weapon_extra_states.keys() & route, route)
        target = BitReader(player.weapon_extra_states[25].getvalue())
        self.assertEqual((target.bit(), target.bit(), target.bits(4), target.bits(32), target.var_b()),
                         (1, 0, 1, enemy.entity, 0))
        player.take_commands([Command(2, buttons=SECONDARY, yaw=-32768)])
        self.assertFalse(player.weapon_extra_states.keys() & route)
        player.take_commands([Command(3, buttons=SECONDARY), Command(4)])
        self.assertFalse(player.weapon_extra_states.keys() & route)

        match.now = 2.1
        player.take_commands([Command(5, buttons=SECONDARY)])
        self.assertEqual(player.hack_target, enemy.entity)
        match._damage_player(player, 20, 2.2)
        self.assertIsNone(player.hack_target)
        self.assertEqual(enemy.hacked_until, 0)

        match.now = 4.3
        player.take_commands([Command(6, buttons=SECONDARY)])
        match.now = 4.94
        player.take_commands([Command(7, buttons=SECONDARY)])
        self.assertEqual(enemy.hacked_until, 0)
        match.now = 4.96
        player.take_commands([Command(8, buttons=SECONDARY)])
        self.assertIsNone(player.hack_target)
        self.assertAlmostEqual(enemy.hacked_until, 9.96)
        enemy.ability = Ability('mend', 50, 3, 12, taken=0)
        enemy.health = 70
        self.assertFalse(match._signature(enemy, 9.95, 1))
        self.assertTrue(match._signature(enemy, 9.97, 1))

    def test_mercy_switch_preserves_pistol_ammo(self):
        from kits import MERCY, MERCY_BLASTER, initialize_practice_kit, model_entity, weapon_instance
        from ow174.game.statescript import Stream

        match = Match(PRACTICE_RANGE, gameplay=True)
        player = match.add_player(1, 'Player', MERCY, 0, True)
        player.has_body = True
        player.body_script = Stream(player.body)
        initialize_practice_kit(player)
        player.take_commands([Command(1, action=2, buttons=FIRE)])
        self.assertEqual(player.rifle.ammo, 19)
        player.take_commands([Command(2, action=1)])
        self.assertEqual(player.rifle.ammo, 9999)
        player.take_commands([Command(3, action=2)])
        self.assertEqual(player.rifle.ammo, 19)
        self.assertEqual(player.last_rifle_update.ammo, 19)
        self.assertEqual((weapon_instance(MERCY_BLASTER, 19).index, model_entity(MERCY_BLASTER).instance), (2, 2))
        player.take_commands([Command(4, action=2, buttons=FIRE)])
        self.assertEqual(player.rifle.ammo, 19)  # reselecting leaves the fire timer in place

    def test_dva_matrix_meter_host_and_child_follow_the_abilities(self):
        from kits import DVA, initialize_practice_kit
        from ow174.game.statescript import ScriptRef, Stream

        match = Match(PRACTICE_RANGE, gameplay=True)
        player = match.add_player(1, 'Player', DVA, 0, True)
        player.has_body = True
        player.body_script = Stream(player.body)
        initialize_practice_kit(player)
        order = [item.index for item in player.body_extra_instances]
        self.assertLess(order.index(0x02CC), order.index(0x214E))
        self.assertIn(0x0276, order)
        self.assertEqual(player.body_extra_instances[order.index(0x214E)].parent, (0x02CC, 46))
        self.assertEqual(player.body_extra_instances[order.index(0x02CC)].active[34].count, 1)
        self.assertEqual(player.body_extra_variables[11821], ScriptRef(0x0276, 55))
        self.assertEqual(player.body_extra_variables[11815], ScriptRef(4, 10))  # Boosters' ring parent

    def test_request_handoff_and_authenticated_map_load(self):
        with PracticeRange(port=0) as practice:
            groups = {REQUEST: 57, HANDOFF: 3}
            request = b'\x39\0\2\0' + struct.pack('<i', 4)
            record = practice.request(request, groups)
            self.assertEqual(record[:2], b'\3\0')
            handoff = next(iter(practice.game.handoffs.values()))[0]
            address = b'127.0.0.1'.ljust(64, b'\0')
            expected = b'\0' + struct.pack('<QQQQQIH', *handoff.match_id, 0, 0, 0,
                handoff.conn, practice.game.port) + address * 2
            expected += handoff.key_in + handoff.key_out + b'\0'
            self.assertEqual(record[2:], expected)
            self.assertIsNone(practice.request(request, groups))  # duplicate cannot create a second match
            self.assertEqual(len(practice.game.matches), 1)
            cipher = LinkCipher(handoff.key_out, handoff.key_in)
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as client:
                client.settimeout(2)
                client.connect(('127.0.0.1', practice.game.port))
                syn = cipher.seal(Packet(conn=handoff.conn, flags=SYN))
                damaged = bytearray(syn)
                damaged[0] ^= 1
                client.send(damaged)
                client.send(syn)
                answer = cipher.open(client.recv(4096))
                self.assertEqual(answer.flags, SYN | ACK)
                client.send(cipher.seal(Packet(conn=handoff.conn, seq=1, ack=answer.seq + 1,
                    ack_bits=1, flags=ACK)))
                answer = cipher.open(client.recv(4096))
                self.assertEqual(answer.kind, KIND_FRAME)
                bits = BitReader(answer.payload)
                bits.var_a()
                reliable, _ = messages.read_channel(bits)
                self.assertEqual(reliable[0][1], 20300)
                self.assertEqual(reliable[0][2]['+0x4A8'], 0x0800000000000688)
                self.assertEqual(reliable[0][2]['+0x500'], 7000)
        self.assertFalse(practice.game.thread.is_alive())

    def test_hero_catalog_owns_each_heros_unlocks(self):
        from practice import HERO_CATALOG
        practice = PracticeRange.__new__(PracticeRange)
        practice.schemas = Schemas()
        practice.equips = []
        self.assertIsNone(practice.hero_catalog({HANDOFF: 3}, {}))
        table = {'02E000000000006E': {'skin_themes': [], 'unlocks': ['0250000000000001', '0250000000000002']}}
        message = practice.hero_catalog({HERO_CATALOG: 9}, table)
        msg_id, body = practice.schemas.split(HERO_CATALOG, message)
        self.assertEqual((message[0], msg_id), (9, 24900))
        catalog = practice.schemas.decode(HERO_CATALOG, 24900, body)
        (hero,) = catalog['+0x80']
        self.assertEqual(hero['+0x30'], 0x02E000000000006E)
        self.assertEqual([(item['+0x0'], item['+0x8']) for item in hero['+0x0']],
                         [(0x0250000000000001, True), (0x0250000000000002, True)])
        (store,) = catalog['+0x98']
        self.assertEqual(store['+0x18'], 0x02E000000000006E)
        self.assertEqual([item['+0x0'] for item in store['+0x0']], [0x0250000000000001, 0x0250000000000002])
        table['02E000000000006E']['defaults'] = {'Skin': '0250000000000002', 'Emote': ['0250000000000001']}
        _, body = practice.schemas.split(HERO_CATALOG, practice.hero_catalog({HERO_CATALOG: 9}, table))
        catalog = practice.schemas.decode(HERO_CATALOG, 24900, body)
        (hero,) = catalog['+0x80']
        self.assertEqual((hero['+0x38'], hero['+0x88'], hero['+0x48']), (0x0250000000000002, [0x0250000000000001], 0))
        self.assertEqual([(item['+0x10'], item['+0x17']) for item in catalog['+0x98'][0]['+0x0']], [(0, True), (0, True)])

    def test_progression_is_a_level_one_account(self):
        from practice import FIRST_BORDER, PROGRESSION
        practice = PracticeRange.__new__(PracticeRange)
        practice.schemas = Schemas()
        self.assertIsNone(practice.progression({HANDOFF: 3}))
        message = practice.progression({PROGRESSION: 7})
        msg_id, body = practice.schemas.split(PROGRESSION, message)
        self.assertEqual((message[0], msg_id), (7, 24300))
        record = practice.schemas.decode(PROGRESSION, 24300, body)['+0x78']
        self.assertEqual((record['+0x58'], record['+0x68'], record['+0x6C']), (FIRST_BORDER, 1, 0))

    def test_login_settings_turn_on_the_kill_feed_option(self):
        from practice import LOBBY
        practice = PracticeRange.__new__(PracticeRange)
        practice.schemas, practice.saved = Schemas(), {}
        self.assertIsNone(practice.settings({HANDOFF: 3}))
        message = practice.settings({LOBBY: 20})
        msg_id, body = practice.schemas.split(LOBBY, message)
        self.assertEqual((message[0], msg_id), (20, 20802))
        value = practice.schemas.decode(LOBBY, 20802, body)
        self.assertTrue(value['+0x120']['+0x4'])  # settings system +700: 016D65A8, the kill feed panel's gate
        self.assertTrue(value['+0x120']['+0x7'])
        self.assertEqual((value['+0x108']['+0x4'], value['+0x10D']['+0x0'], value['+0x10D']['+0x5']), (True, True, 1))

    def test_saved_options_come_back_at_the_next_login(self):
        from practice import LOBBY, SOCIAL_OUT
        groups = {SOCIAL_OUT: 41, LOBBY: 20}

        def login(practice):
            _, body = practice.schemas.split(LOBBY, practice.settings(groups))
            return practice.schemas.decode(LOBBY, 20802, body)

        def save(practice, msg_id, value):
            message = practice.schemas.header(SOCIAL_OUT, 41, msg_id) + practice.schemas.encode(SOCIAL_OUT, msg_id, {'+0x78': value})
            self.assertIsNone(practice.request(message, groups))

        with tempfile.TemporaryDirectory() as folder:
            saved = Path(folder) / 'equips.json'
            with PracticeRange(port=0, equips=saved) as practice:
                block = login(practice)['+0x120']
                self.assertTrue(block['+0x4'])
                save(practice, 22203, {**block, '+0x4': False, '+0x0': 7})  # a whole block: the kill feed off
                save(practice, 22204, [{'+0x0': 1, '+0x8': 0xE06894}, {'+0x0': 5, '+0x8': 0xE061C0}])
                save(practice, 22204, [{'+0x0': 2, '+0x8': 0xE06894}])  # a changed entry replaces its key's
            with PracticeRange(port=0, equips=saved) as later:
                value = login(later)
                self.assertEqual((value['+0x120']['+0x4'], value['+0x120']['+0x0'], value['+0x120']['+0x7']), (False, 7, True))
                self.assertEqual(value['+0x130'], [{'+0x0': 2, '+0x8': 0xE06894}, {'+0x0': 5, '+0x8': 0xE061C0}])
                self.assertTrue(value['+0x108']['+0x4'])  # a block never saved keeps its default
            for text, logs in (('{"+0x120": 5}', self.assertNoLogs), ('{"+0x120": {"+0x0": "x"}}', self.assertLogs)):
                (Path(folder) / 'settings.json').write_text(text, encoding='utf-8')
                with PracticeRange(port=0, equips=saved) as damaged, logs('lobby', 'ERROR'):
                    self.assertTrue(login(damaged)['+0x120']['+0x4'])  # the login still goes out, with the defaults

    def test_party_and_menu_config_name_the_menu_hero(self):
        from practice import CONFIG, MENU_HERO, PARTY, SUPPORTED_BUILDS
        practice = PracticeRange.__new__(PracticeRange)
        practice.name = 'Player'
        practice.schemas = Schemas()
        self.assertIsNone(practice.party({HANDOFF: 3}, 1))
        self.assertIsNone(practice.menu_hero({HANDOFF: 3}, 1))
        _, body = practice.schemas.split(PARTY, practice.party({PARTY: 4}, 0x02E000000000006E))
        (member,) = practice.schemas.decode(PARTY, 20700, body)['+0x78']['+0x0']
        self.assertEqual((member['+0xB8'], member['+0xE0'], member['+0xE2'], member['+0x0']['+0x40']),
                         (0x02E000000000006E, 5, True, 'Player'))
        _, body = practice.schemas.split(CONFIG, practice.menu_hero({CONFIG: 6}, 0x02E000000000006E))
        self.assertEqual(practice.schemas.decode(CONFIG, 36600, body)['+0x78'],
                         [{'+0x0': SUPPORTED_BUILDS, '+0x8': '[104319]'}, {'+0x0': MENU_HERO, '+0x8': '0x02e000000000006e'}])

    def test_gallery_equip_is_confirmed_with_24902(self):
        from practice import GALLERY, HERO_CATALOG
        groups = {GALLERY: 12, HERO_CATALOG: 9}
        saved = Path(tempfile.mkdtemp()) / 'equips.json'
        with PracticeRange(port=0, equips=saved) as practice:
            practice.slot_of = lambda item: 'Skin'  # what the install says of Soldier's Classic
            equip = {'+0x78': 0x02E000000000006E, '+0x80': 0x02500000000004C7, '+0x88': 0}
            request = practice.schemas.header(GALLERY, 12, 24500) + practice.schemas.encode(GALLERY, 24500, equip)
            reply = practice.request(request, groups)
            msg_id, body = practice.schemas.split(HERO_CATALOG, reply)
            self.assertEqual((reply[0], msg_id, practice.schemas.decode(HERO_CATALOG, 24902, body)), (9, 24902, equip))
            seen = practice.schemas.header(GALLERY, 12, 24501) + practice.schemas.encode(GALLERY, 24501, {})
            self.assertIsNone(practice.request(seen, groups))
            self.assertFalse(practice.game.matches)
        with PracticeRange(port=0, equips=saved) as later:  # the next login's 24900 wears it
            table = {'02E000000000006E': {'unlocks': [], 'defaults': {'Skin': '0250000000000001'}}}
            msg_id, body = later.schemas.split(HERO_CATALOG, later.hero_catalog(groups, table))
            self.assertEqual(later.schemas.decode(HERO_CATALOG, 24900, body)['+0x80'][0]['+0x38'], 0x02500000000004C7)

    def test_match_wears_the_equipped_skin_theme_when_preloaded(self):
        import practice as module
        saved = Path(tempfile.mkdtemp()) / 'equips.json'
        soldier, atlantic, theme = 0x02E000000000006E, 0x0250000000004B56, 0x0A500000000047D5
        saved.write_text(f'[[{soldier}, 1, 0, "Skin"], [{soldier}, {atlantic}, 0, "Skin"], [{soldier}, 2, 0, "Spray"]]')
        real = module.skin_theme
        module.skin_theme = lambda storage, unlock: {atlantic: theme}.get(unlock, 0)
        try:
            with PracticeRange(port=0, equips=saved, game_install='install') as practice:
                practice.storage = object()  # no install read: skin_theme is stubbed
                self.assertEqual(practice.game.skin_of, practice.skin_of)  # the game server asks the lobby
                self.assertEqual(practice.skin_of(0, soldier), (0, False))  # 20505 did not preload it
                practice.hero_table = {'02E000000000006E': {'skin_themes': [f'{theme:016X}']}}
                self.assertEqual(practice.skin_of(0, soldier), (theme, False))  # the last saved Skin wins
                self.assertEqual(practice.skin_of(0, 0x02E0000000000003), (0, False))  # nothing saved for Tracer
        finally:
            module.skin_theme = real

    def test_career_profile_request_gets_our_full_profile(self):
        from practice import ACCOUNT_RECORD, HERO_CATALOG, LOBBY, SOCIAL_OUT
        groups = {SOCIAL_OUT: 41, LOBBY: 20, HERO_CATALOG: 9}
        with PracticeRange(port=0) as practice:
            practice.hero_catalog(groups, {'02E000000000006E': {'skin_themes': [], 'unlocks': ['0250000000000001']}})
            ask = {'+0x78': {'+0x0': 7, '+0x8': 8}, '+0x88': {'+0x0': 1, '+0x8': 1 << 56}, '+0x98': 0}
            reply = practice.request(practice.schemas.header(SOCIAL_OUT, 41, 22206)
                                     + practice.schemas.encode(SOCIAL_OUT, 22206, ask), groups)[-1]
            msg_id, body = practice.schemas.split(LOBBY, reply)
            value = practice.schemas.decode(LOBBY, 20807, body)
            self.assertEqual((reply[0], msg_id, value['+0x78'], value['+0x90']['+0x138']), (20, 20807, ask['+0x78'], 'Player'))
            self.assertEqual(value['+0x90']['+0x0']['+0x68'], ACCOUNT_RECORD['+0x68'])
            self.assertEqual([hero['+0x30'] for hero in value['+0x90']['+0x80']], [0x02E000000000006E])
            settings = practice.schemas.header(SOCIAL_OUT, 41, 22204) + practice.schemas.encode(SOCIAL_OUT, 22204, {})
            self.assertIsNone(practice.request(settings, groups))

    def test_an_event_hosts_the_maps_variant_when_it_has_one(self):
        from practice import pick_map
        assault = '0x0230000000000014'
        self.assertEqual(pick_map('Hanamura', assault).name, 'Hanamura')
        self.assertEqual(pick_map('Hanamura', assault, 'none').name, 'Hanamura')
        self.assertEqual(pick_map('Hanamura', assault, 'winter').name, 'Hanamura (Winter)')
        self.assertEqual(pick_map('hanamura', assault, 'lunar').name, 'Hanamura')  # no Lunar Hanamura
        self.assertEqual(pick_map('Hanamura (Winter)', assault).name, 'Hanamura (Winter)')  # still by its own name

    def test_normal_menu_has_no_active_celebration(self):
        from practice import EVENTS, MENU_EVENTS
        practice = PracticeRange.__new__(PracticeRange)
        practice.schemas = Schemas()
        practice.event = MENU_EVENTS['none']
        self.assertIsNone(practice.events({HANDOFF: 3}))
        _, body = practice.schemas.split(EVENTS, practice.events({EVENTS: 8}))
        self.assertEqual(practice.schemas.decode(EVENTS, 38900, body)['+0x78'], [])

    def test_menu_event_record(self):
        from practice import EVENTS, MENU_EVENTS, stu_datetime
        practice = PracticeRange.__new__(PracticeRange)
        practice.schemas = Schemas()
        practice.event = MENU_EVENTS['farewell']
        self.assertIsNone(practice.events({HANDOFF: 3}))
        self.assertEqual(stu_datetime(0x5F5E1000), 20 << 36 | 9 << 32 | 13 << 27 | 12 << 22 | 26 << 16 | 40 << 10)
        _, body = practice.schemas.split(EVENTS, practice.events({EVENTS: 8}, now=0x5F5E1000))
        (record,) = practice.schemas.decode(EVENTS, 38900, body)['+0x78']
        self.assertEqual((record['+0x30'], record['+0x38'], record['+0x40'], record['+0x70'], record['+0xB0']),
                         (0x28B8EBB462B83EEA, 0x0F100000000001A3, 0x0430000000000128, 0, -1))
        self.assertEqual(record['+0x48']['+0x0'], stu_datetime(0x5F5E1000 - 2 * 86400))
        practice.event = MENU_EVENTS['owl']  # no content key
        _, body = practice.schemas.split(EVENTS, practice.events({EVENTS: 8}))
        (record,) = practice.schemas.decode(EVENTS, 38900, body)['+0x78']
        self.assertEqual((record['+0x30'], record['+0x38'], record['+0x40']), (0, 0, 0x0430000000000125))
        practice.event = MENU_EVENTS['lunar']
        _, body = practice.schemas.split(EVENTS, practice.events({EVENTS: 8}))
        self.assertEqual(practice.schemas.decode(EVENTS, 38900, body)['+0x78'][0]['+0x70'], 0x0D8000000000237C)

    def test_login_advertises_every_playable_hero(self):
        heroes = PracticeRange.menu_heroes()
        self.assertEqual(len(heroes), 32)
        self.assertIn(0x02E000000000006E, heroes)  # Soldier: 76
        self.assertIn(0x02E000000000023B, heroes)  # Sigma

    def test_tournament_card_opens_the_vs_ai_map_and_the_range_card_the_range(self):
        from practice import TOURNAMENT_MATCH, pick_map
        groups = {REQUEST: 57, HANDOFF: 3, TOURNAMENT_MATCH: 37}
        card = bytes.fromhex('25 00 01 00 00 00 83 dd e9 6a 00 00 00 00 8e dd e9 6a 00 00 00 00 69 7b dd 00')
        with PracticeRange(port=0, game_map=pick_map('Hanamura', '0x0230000000000014')) as practice:
            self.assertEqual(practice.request(card, groups)[:2], b'\3\0')
            (match,) = practice.game.matches
            self.assertEqual(match.game_map.name, 'Hanamura')
            self.assertIsNone(practice.request(card, groups))  # one match per player
        with PracticeRange(port=0, game_map=pick_map('Hanamura', '0x0230000000000014')) as practice:
            practice.request(b'\x39\0\2\0' + struct.pack('<i', 4), groups)
            (match,) = practice.game.matches
            self.assertEqual(match.game_map, PRACTICE_RANGE)

    def test_arcade_lists_the_pve_event_cards_and_their_queues_search_until_cancelled(self):
        from practice import ARCADE, ARCHIVES, JUNKENSTEIN, PARTY, QUEUE, QUEUE_STATE, QUEUE_WAITS, pick_map
        groups = {HANDOFF: 3, QUEUE: 10, QUEUE_STATE: 64, QUEUE_WAITS: 54, PARTY: 18, ARCADE: 20}
        with PracticeRange(port=0, game_map=pick_map('Hanamura', '0x0230000000000014'), retail_menu=True) as practice:
            self.assertIsNone(practice.arcade({HANDOFF: 3}))
            cards, windows, busy = practice.arcade(groups, now=1_700_000_000)
            value = practice.schemas.decode(ARCADE, 39802, practice.schemas.split(ARCADE, cards)[1])['+0x78']
            listed = [card['+0x0'] & 0xFFFF for card in value['+0x0']]
            self.assertEqual(listed[:len(JUNKENSTEIN)], list(JUNKENSTEIN))
            self.assertTrue(set(ARCHIVES) <= set(listed))
            self.assertEqual((value['+0x18']['+0x0'] & 0xFFFF, len(value['+0xD0'])), (0xED, 2))
            dates = practice.schemas.decode(ARCADE, 39810, practice.schemas.split(ARCADE, windows)[1])['+0x78']
            self.assertEqual((len(dates), dates[0]['+0x8'] < 1_700_000_000 < dates[0]['+0x10']), (len(listed), True))
            # Uprising (All Heroes), Normal: queued, no roles, no match; cancel leaves
            enter = bytes.fromhex('0a 00 24 00 00 00 00 00 30 06') + bytes(28) + bytes.fromhex('40 86 f3 00 66 72 f4 00')
            (joined,) = practice.request(enter, groups)
            self.assertEqual(practice.schemas.split(QUEUE_STATE, joined)[0], 44201)
            self.assertFalse(practice.game.matches)
            left, party = practice.request(b'\x0a\2' + enter[2:], groups)
            self.assertEqual(practice.schemas.split(QUEUE_STATE, left)[0], 44202)
            # Uprising (Story), Normal: with the experiment on, it hands off to King's Row (Uprising) in mode 0x0F
            (joined,) = practice.request(b'\x0a\0\x29' + enter[3:], groups)
            practice.request(b'\x0a\2\x29' + enter[3:], groups)
            self.assertEqual((practice.handoff_maps(), practice.handoff_heroes()), (set(), ()))  # off: 20505 advertises none
            practice.pve_handoff = True
            self.assertEqual(practice.handoff_maps(), {0x6FE})  # on: the client preloads King's Row (Uprising)'s
            self.assertEqual(len(practice.handoff_heroes()), 4)  # header and the omnics' hero content
            joined, popped, handoff = practice.request(b'\x0a\0\x29' + enter[3:], groups)
            self.assertEqual((handoff[:2], practice.game.matches[0].game_map.name), (b'\3\0', "King's Row (Uprising)"))
            spawns = practice.game.matches[0].game_map.spawns  # the players' point, then the 68 omnic markers, nearest first
            self.assertEqual((len(spawns), spawns[0].team, {spawn.team for spawn in spawns[1:]}), (69, 0, {1}))
            self.assertEqual([round(spawn.position[0]) for spawn in spawns[1:3]], [-64, -46])
            self.assertEqual(practice.game.matches[0].game_map.objectives, (spawns[0].position,))  # the omnics' goal

    def test_pve_gate_frame_carries_the_mission_hud(self):
        from ow174.game.match import pve_instances
        instances = pve_instances(0x023000000000000F)  # the HUD bootstrap and widget under the mode graph, the director
        self.assertEqual([(i.index, i.graph.index, i.parent, sorted(i.active)) for i in instances],
                         [(2, 0x0CB6, (1, 12), []), (3, 0x0E95, (1, 31), []), (4, 0x0D4A, None, [])])
        self.assertEqual(sorted(pve_instances(0x023000000000000F, defended=True)[2].active), [0])  # opens the widget
        escort = pve_instances(0x023000000000000F, defended=True, escort=True)[2]  # the defend timer ran out
        self.assertEqual((escort.index, escort.graph.index, sorted(escort.active)), (5, 0x0D4B, [0]))
        self.assertEqual(pve_instances(0x0230000000000014), [])

    def test_quick_play_role_queue_joins_starts_on_ready_and_cancels(self):
        from practice import PARTY, QUEUE, QUEUE_STATE, QUEUE_WAITS, pick_map
        groups = {HANDOFF: 3, QUEUE: 10, QUEUE_STATE: 64, QUEUE_WAITS: 54, PARTY: 18}
        enter = bytes.fromhex('0a 00 01 00 00 00 00 00 30 06') + bytes(28) + bytes.fromhex('40 86 f3 00 66 72 f4 00')
        ready, cancel = b'\x0a\3\1\1', b'\x0a\2' + enter[2:]

        def decode(group, msg_id, frame):
            return practice.schemas.decode(group, msg_id, practice.schemas.split(group, frame)[1])

        with PracticeRange(port=0, game_map=pick_map('Hanamura', '0x0230000000000014'), retail_menu=True) as practice:
            self.assertIsNone(practice.request(ready, groups))  # Ready before any queue
            joined, waits, party = practice.request(enter, groups)
            self.assertEqual((joined[0], waits[0], party[0]), (64, 54, 18))
            self.assertEqual(practice.schemas.split(QUEUE_STATE, joined)[0], 44201)
            state = decode(PARTY, 20700, party)['+0x78']
            self.assertEqual((state['+0x18'][0]['+0x54'], state['+0x0'][0]['+0xC8'][0]['+0x69']), (1, False))
            self.assertFalse(practice.game.matches)
            joined, party, popped, party2, handoff = practice.request(ready, groups)  # Ready with damage
            self.assertEqual(handoff[:2], b'\3\0')
            state = decode(PARTY, 20700, party)['+0x78']
            self.assertEqual((state['+0x18'][0]['+0x54'], state['+0x0'][0]['+0xC8'][0]['+0x69']), (3, True))
            self.assertEqual(decode(QUEUE_STATE, 44202, popped)['+0xA8'], 1)  # match found: the entry is dropped
            self.assertEqual(decode(PARTY, 20700, party2)['+0x78']['+0x18'], [])
            self.assertEqual(practice.game.matches[0].game_map.name, 'Hanamura')
            self.assertIsNone(practice.request(cancel, groups))  # not queued any more
            practice.request(enter, groups)
            left, party = practice.request(cancel, groups)
            self.assertEqual((practice.schemas.split(QUEUE_STATE, left)[0], decode(QUEUE_STATE, 44202, left)['+0xA8']), (44202, 0))
            self.assertEqual(decode(PARTY, 20700, party)['+0x78']['+0x18'], [])
            self.assertIsNone(practice.request(ready, groups))

    def test_retail_vs_ai_queue_sets_the_difficulty_and_menu_pings_start_nothing(self):
        from practice import QUEUE, TOURNAMENT_MATCH, pick_map
        groups = {REQUEST: 57, HANDOFF: 3, TOURNAMENT_MATCH: 37, QUEUE: 10}
        ping = bytes.fromhex('25 00 01 00 00 00 83 dd e9 6a 00 00 00 00 8e dd e9 6a 00 00 00 00 69 7b dd 00')
        hard = bytes.fromhex('0a 00 72 00 00 00 00 00 30 06') + bytes(28) + bytes.fromhex('40 86 f3 00 66 72 f4 00')
        with PracticeRange(port=0, game_map=pick_map('Hanamura', '0x0230000000000014'), retail_menu=True) as practice:
            self.assertIsNone(practice.request(ping, groups))
            self.assertIsNone(practice.request(b'\x0a\0\x40' + hard[3:], groups))  # another queue
            self.assertFalse(practice.game.matches)
            self.assertEqual(practice.request(hard, groups)[:2], b'\3\0')
            (match,) = practice.game.matches
            self.assertEqual((match.game_map.name, match.difficulty), ('Hanamura', 'hard'))
            with self.assertRaises(ValueError):
                practice.request(hard[:-1], groups)

    def test_unsupported_and_malformed_requests(self):
        with PracticeRange(port=0) as practice:
            groups = {REQUEST: 57, HANDOFF: 3}
            for payload in (b'\x39\0', b'\x39\0\2\0', b'\x39\0\2name',
                            b'\x39\0\2\0\4\0\0\0extra'):
                with self.assertRaises(ValueError):
                    practice.request(payload, groups)
            self.assertIsNone(practice.request(b'\x39\0\3\0\4\0\0\0', groups))
            self.assertIsNone(practice.request(b'\x38\0\2\0\4\0\0\0', groups))
            self.assertFalse(practice.game.matches)


if __name__ == '__main__':
    unittest.main()
