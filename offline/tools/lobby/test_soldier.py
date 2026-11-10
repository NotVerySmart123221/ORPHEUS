import unittest
from unittest.mock import patch

import practice  # installs the pinned vendor path
from ow174.game.content import PRACTICE_RANGE, SOLDIER
from ow174.game.match import Match
from ow174.game.commands import Command
from soldier import body_frame, weapon_instance


class _Client:
    def __init__(self):
        self.updates = []
        self.messages = []

    def queue_reliable(self, message, value):
        self.messages.append((message, value))

    def queue_entities(self, updates):
        self.updates.append(updates)


class SoldierTests(unittest.TestCase):
    def test_reload_commands_disable_native_fire_until_refilled(self):
        from ow174.game.bits import BitReader
        from soldier import RELOAD

        game = practice.PracticeRange(port=0).game
        game.create_match(PRACTICE_RANGE, [(1, 'Player', SOLDIER, 0, True)])
        match, player = game.matches[0], game.matches[0].players[0]
        player.client = _Client()
        player.spawned = player.has_body = True
        player.body_picked_at = 1
        match.update(3, 1)
        stream = player.body_script
        stream.arrived(stream.data_last)

        def frame(commands, now):
            player.take_commands(commands)
            match.update(now, 2)
            stream.arrived(stream.data_last)
            return stream._reload_snapshot[0][0]

        ready = frame([Command(100, buttons=1)], 3.1)
        self.assertEqual(BitReader(ready.active[8].getvalue()).bit(), 1)
        reload = frame([Command(101, buttons=RELOAD), Command(102, buttons=1), Command(103)], 3.2)
        self.assertEqual((player.rifle.serial, player.rifle.ammo), (1, 29))
        self.assertEqual(BitReader(reload.active[8].getvalue()).bit(), 0)
        self.assertEqual(BitReader(reload.active[10].getvalue()).bit(), 0)
        self.assertIn(39, reload.active)
        ready = frame([Command(316)], 3.3)
        self.assertEqual((player.rifle.ammo, BitReader(ready.active[8].getvalue()).bit()), (30, 1))

        player.rifle.ammo = 1
        last = frame([Command(400, buttons=1)], 3.4)
        self.assertEqual((player.rifle.serial, player.rifle.ammo), (2, 0))
        self.assertEqual(BitReader(last.active[8].getvalue()).bit(), 0)
        self.assertEqual(BitReader(last.active[10].getvalue()).bit(), 1)  # final accepted shot still draws
        stopped = frame([Command(401, buttons=1)], 3.5)
        self.assertEqual(player.rifle.serial, 2)
        self.assertEqual(BitReader(stopped.active[8].getvalue()).bit(), 0)
        self.assertEqual(BitReader(stopped.active[10].getvalue()).bit(), 0)

    def test_primary_input_is_disabled_during_reload_and_empty_magazine(self):
        from ow174.game.bits import BitReader
        from soldier import RifleUpdate, initialize_practice_soldier

        match = Match(PRACTICE_RANGE)
        player = match.add_player(1, 'Player', SOLDIER, 0, True)
        initialize_practice_soldier(player)
        reload = player.weapon_extra_states[3]
        reader = BitReader(reload.getvalue())
        self.assertEqual((reader.bit(), reader.bits(4), reload.count), (1, 0, 5))
        cases = ((30, None, True), (5, RifleUpdate(5, 25, False, True, True), False),
                 (0, RifleUpdate(0, 30, True, True), False),
                 (0, RifleUpdate(0, 30, False, True, True), False),
                 (0, RifleUpdate(0, 30, False, True), False),
                 (30, RifleUpdate(30, 30, False, True), True),
                 (29, RifleUpdate(29, 31, True, True), True))
        for ammo, update, enabled in cases:
            with self.subTest(ammo=ammo, update=update):
                instance = weapon_instance(ammo, update, player.weapon_extra_states)
                button = instance.active[8]
                reader = BitReader(button.getvalue())
                # Native helper state needs an explicit control bit; omission only skips its update.
                self.assertEqual((reader.bit(), reader.bits(4), button.count), (enabled, 0, 5))
                if update is not None:
                    self.assertIn(10, instance.active)  # retain the explicit Volley Stop payload
                    self.assertEqual(BitReader(instance.active[10].getvalue()).bit(), update.active)

    def test_cooldown_bindings_keep_advancing_across_preserved_owner_states(self):
        from soldier import cooldown_left, field_instance, initialize_practice_soldier, melee_instance

        match = Match(PRACTICE_RANGE)
        player = match.add_player(1, 'Player', SOLDIER, 0, True)
        initialize_practice_soldier(player)
        values = (cooldown_left(1, 31, 7), field_instance(None).presence[5],
                  melee_instance(player.melee).presence[6])
        self.assertTrue(all(value.per_frame for value in values))

    def test_primary_fire_uses_live_native_timer_output(self):
        from ow174.game.bits import BitReader
        from ow174.game.statescript import Float, StateBound

        instance = weapon_instance(fire_elapsed=112)
        self.assertEqual(instance.presence[20], StateBound(Float(100), 1, 19, 7, per_frame=True))
        self.assertEqual(instance.presence[21], Float(100))
        reader = BitReader(instance.active[19].getvalue())
        self.assertEqual((reader.bits(3), reader.bits(32), reader.bit(), reader.var_b()), (0, 0, 1, 112))
        self.assertEqual(reader.pos, instance.active[19].count)
        self.assertNotIn(19, weapon_instance().active)
        for elapsed in (-1, True, 1.5, 1 << 31):
            with self.subTest(elapsed=elapsed), self.assertRaises(ValueError):
                weapon_instance(fire_elapsed=elapsed)

    def test_primary_timer_survives_held_shots_release_and_hud_refresh(self):
        from ow174.game.bits import BitReader
        from soldier import RifleUpdate, initialize_practice_soldier

        game = practice.PracticeRange(port=0).game
        game.create_match(PRACTICE_RANGE, [(1, 'Player', SOLDIER, 0, True)])
        match, player = game.matches[0], game.matches[0].players[0]
        player.client = _Client()
        player.spawned = player.has_body = True
        player.body_picked_at = 1
        match.update(3, 1)
        self.assertNotIn(19, player.body_script._reload_snapshot[0][0].active)
        sequence = ((100, True, 0), (116, True, 112), (120, False, 140),
                    (150, False, 350), (200, True, 0), (210, True, 70))
        for serial, (cmfd, active, elapsed) in enumerate(sequence, 1):
            match._body_weapon_frame(player, cmfd, RifleUpdate(29, serial, active, True), refresh=True)
            instance = player.body_script._reload_snapshot[0][0]
            reader = BitReader(instance.active[19].getvalue())
            reader.bits(36)
            self.assertEqual(reader.var_b(), elapsed)
        initialize_practice_soldier(player)
        match._body_weapon_frame(player, 250, None)
        self.assertNotIn(19, player.body_script._reload_snapshot[0][0].active)

    def test_field_spawns_positioned_native_ring_for_viewers_and_expires(self):
        from ow174.game import world
        from ow174.game.bits import BitReader

        match = Match(PRACTICE_RANGE)
        players = [match.add_player(index, 'Player', SOLDIER, index - 1, True) for index in (1, 2)]
        for player in players:
            player.client, player.spawned = _Client(), True
            player.commands.last_frame = 1234
        owner = players[0]
        position = (54.5, 1.0, 42.0)
        owner.mover.position = list(position)
        match.now = 10.0
        match.throw_field(owner)
        owner.mover.position[0] = 60.0  # the field stays at its throw position
        for player in players:
            self.assertEqual(len(player.client.updates[0]), 2)
            create, effect = player.client.updates[0]
            self.assertEqual((create.op, create.entity), (world.OP_CREATE, effect.entity))
            with patch.object(world, 'write_movement', wraps=world.write_movement) as movement:
                record = create.build(0)
            reader = BitReader(record.getvalue())
            self.assertEqual(reader.bits(2), 0b10)  # definition, 12-bit GUID index
            self.assertEqual(reader.bits(12), 0x03DC)
            self.assertTrue(reader.bits(8) & world.WITH_MOVEMENT)
            self.assertEqual(movement.call_args.args[1].position, position)
            # Native-verified owner graph600: type7 output of CosmeticEntity3, not a plain entity ID.
            self.assertEqual(effect.chunk.getvalue().hex(), '000004e0a7a409049600003b0000004012035cc000')
            self.assertTrue(effect.resend)
        match._tick_fields(14.99)
        self.assertTrue(match.fields)
        self.assertTrue(all(len(player.client.updates) == 1 for player in players))
        match._tick_fields(15.0)
        self.assertEqual(match.fields, [])
        for player in players:
            destroy, = player.client.updates[-1]
            self.assertEqual((destroy.entity, destroy.op), (create.entity, world.OP_DESTROY))

    def test_field_lands_when_the_volley_throws_not_at_the_press(self):
        game = practice.PracticeRange(port=0).game
        game.create_match(PRACTICE_RANGE, [(1, 'Player', SOLDIER, 0, True)])
        match, player = game.matches[0], game.matches[0].players[0]
        player.client = _Client()
        player.spawned = player.has_body = True
        player.body_picked_at = 1
        match.update(3, 1)
        player.field_release_at = 3.4  # an E press at 3.0
        match.update(3.3, 2)
        self.assertEqual(match.fields, [])
        match.update(3.4, 3)
        self.assertEqual((len(match.fields), player.field_release_at), (1, 0.0))
        player.field_release_at, player.health = 3.8, 0  # died with the canister in hand
        match.update(3.8, 4)
        self.assertEqual((len(match.fields), player.field_release_at), (1, 0.0))

    def test_reload_finishing_does_not_replay_sprint_enter(self):
        from soldier import RELOAD, SPRINT_BUTTON
        from ow174.game.bits import BitReader
        game = practice.PracticeRange(port=0).game
        game.create_match(PRACTICE_RANGE, [(1, 'Player', SOLDIER, 0, True)])
        match, player = game.matches[0], game.matches[0].players[0]
        player.client = _Client()
        player.spawned = player.has_body = True
        player.body_picked_at = 1
        match.update(3, 1)
        player.body_script.arrived(player.body_script.data_last)
        player.rifle.ammo = 29
        player.take_commands([Command(100, buttons=RELOAD)])
        match.update(3.1, 2)
        player.body_script.arrived(player.body_script.data_last)
        player.take_commands([Command(295, forward=127, buttons=SPRINT_BUTTON)])
        match.update(3.2, 3)
        player.body_script.arrived(player.body_script.data_last)
        player.take_commands([Command(315, forward=127, buttons=SPRINT_BUTTON)])
        self.assertIsNone(player.rifle.reload_until)
        self.assertEqual(player.sprint.animation, 15)
        match.update(3.3, 4)
        reader = BitReader(player.client.updates[-1][0].chunk.getvalue())
        self.assertEqual(reader.bit(), 0)
        self.assertGreater(reader.bits(16), 0)

    def test_damage_during_sprint_transition_does_not_restart_animation(self):
        from soldier import SPRINT_BUTTON
        from ow174.game.bits import BitReader
        game = practice.PracticeRange(port=0).game
        game.create_match(PRACTICE_RANGE, [(1, 'Player', SOLDIER, 0, True)])
        match, player = game.matches[0], game.matches[0].players[0]
        player.client = _Client()
        player.spawned = player.has_body = True
        player.body_picked_at = 1
        match.update(3, 1)
        player.body_script.arrived(player.body_script.data_last)
        player.take_commands([Command(100, forward=127, buttons=SPRINT_BUTTON)])
        match.update(3.1, 2)
        player.body_script.arrived(player.body_script.data_last)
        match._damage_player(player, 20, 3.2)
        match.update(3.2, 3)
        reader = BitReader(player.client.updates[-1][0].chunk.getvalue())
        self.assertEqual(reader.bit(), 0)
        self.assertGreater(reader.bits(16), 0)  # delta, not a full replay of StateAnim15
        reader.var_b()
        reader.bit()
        reader.var_b()
        reader.bits(3)
        reader.var_b()
        reader.var_b()
        self.assertEqual(reader.bits(5), 0)  # no changed instances: health only

    def test_spawn_voice_waits_for_body_ack_and_plays_once_per_spawn(self):
        for hero, lines in ((SOLDIER, ((0x2B, 0x6F9C), (0x2C, 0x7036))),  # heard in game
                            (0x02E000000000007A, ((0x2B, 0x776D), (0x2C, 0x7765))),  # D.Va, heard in game
                            (0x02E0000000000003, ((0x2B, 0x5A59), (0x2C, 0x59CA)))):  # Tracer
            with self.subTest(hero=hex(hero)):
                self._check_spawn_voice(hero, lines)

    def test_every_roster_hero_has_spawn_lines(self):
        from soldier import SPAWN_VOICES
        from ow174.game.content import heroes
        self.assertEqual(set(SPAWN_VOICES), set(heroes()))

    def _check_spawn_voice(self, hero, lines):
        game = practice.PracticeRange(port=0).game
        game.create_match(PRACTICE_RANGE, [(1, 'Player', hero, 0, True)])
        match, player = game.matches[0], game.matches[0].players[0]
        player.client, player.spawned = _Client(), True
        for spawn, (stimulus, key) in enumerate(lines):
            player.respawn_at = 1 if spawn else None
            match.switch_hero(player, player.hero)
            player.body_picked_at = 1
            match.update(3, 10 * spawn + 1)
            voices = lambda: [value for number, value in player.client.messages if number == 27500]
            self.assertEqual(len(voices()), spawn)
            player.body_script.arrived(player.body_script.data_last)
            player.take_commands([Command(100 + 100 * spawn, buttons=1)])
            match.update(3.01, 10 * spawn + 2)
            self.assertEqual(len(voices()), spawn + 1)
            message = voices()[-1]
            self.assertEqual(message['+0x78'], {'+0x0': player.body})
            self.assertEqual(message['+0x88'], 0x0EE0000000000000 | stimulus)
            self.assertEqual(message['+0x90'], 0x0760000000000000 | key)
            player.body_dirty = True
            match.update(3.02, 10 * spawn + 3)
            player.body_script.arrived(player.body_script.data_last)
            match.update(3.03, 10 * spawn + 4)
            self.assertEqual(len(voices()), spawn + 1)

    def test_sprint_transitions_play_once_expire_and_cancel_for_fire(self):
        from soldier import Sprint, SPRINT_BUTTON, sprint_instance

        sprint = Sprint(10000)
        self.assertTrue(sprint.command(Command(100, forward=127, buttons=SPRINT_BUTTON)))
        self.assertIn(15, sprint_instance(sprint).active)
        self.assertFalse(sprint.command(Command(120, forward=127, buttons=SPRINT_BUTTON)))
        self.assertTrue(sprint.command(Command(154, forward=127, buttons=SPRINT_BUTTON)))
        self.assertNotIn(15, sprint_instance(sprint).active)
        self.assertTrue(sprint.active)
        self.assertTrue(sprint.command(Command(155, forward=127)))
        self.assertIn(14, sprint_instance(sprint).active)
        self.assertNotIn(15, sprint_instance(sprint).active)
        self.assertTrue(sprint.command(Command(222, forward=127)))
        self.assertNotIn(14, sprint_instance(sprint).active)
        sprint.command(Command(223, forward=127, buttons=SPRINT_BUTTON))
        sprint.command(Command(224, forward=127, buttons=SPRINT_BUTTON | 1))
        self.assertFalse(sprint.active)
        self.assertTrue({14, 15}.isdisjoint(sprint_instance(sprint).active))

    def test_visor_cast_plays_the_gesture_then_starts_the_timer(self):
        from soldier import visor_instance
        cast, active = visor_instance('cast').active, visor_instance('active').active
        self.assertTrue({1, 2, 41}.issubset(cast))  # gesture Anim, var33 Stack, Wait 91
        self.assertNotIn(6, cast)  # ChaseVar 8 waits for Wait 91
        self.assertIn(6, active)
        self.assertTrue({1, 2, 41}.isdisjoint(active))
        self.assertEqual(set(cast) - {1, 2, 41}, set(active) - {6})
        self.assertEqual(set(visor_instance().active), {32})

    def test_visor_end_stops_the_tint_effect(self):
        from kits import _bits
        from soldier import visor_instance
        self.assertEqual(visor_instance('active', 2).active[3].getvalue(), _bits((1, 1), (2, 2)).getvalue())
        ended = visor_instance(None, 2).active  # left out, effect 3 (the screen tint) kept playing
        self.assertEqual(set(ended), {32, 3})
        self.assertEqual(ended[3].getvalue(), _bits((0, 1), (2, 2)).getvalue())

    def test_helix_reservation_codec_and_body_omits_client_prediction(self):
        import struct
        from ow174.game import world
        from ow174.game.world import OP_CREATE, OP_DESTROY, OP_UPDATE
        from soldier import helix_reservations
        reserved = helix_reservations(0x01000000)
        out = world.RecordWriter(0)
        world._write_components(out, {90: [reserved]})
        records = b''.join(struct.pack('<QIIIB', *r) for r in reserved)
        # count, index 90, mask length 1, mask 0x01, u32 count, then 21 raw bytes per STU_5E1D1776 record
        self.assertEqual(out.getvalue(), bytes([1, 90, 1, 1]) + struct.pack('<I', 2) + records)
        # IDs per tick 0: every tick gets the same, server-known ID
        self.assertEqual([(r[3], r[4]) for r in reserved], [(0x01000000, 0), (0x08000000, 0)])
        with patch.object(world, 'create', side_effect=lambda origin, body, components, **kw: components):
            match = Match(PRACTICE_RANGE)
            player = match.add_player(1, 'Player', SOLDIER, 0, True)
            self.assertNotIn(90, match._body_create(0, player, None))

    def _native_helix_match(self):
        game = practice.PracticeRange(port=0).game
        game.create_match(PRACTICE_RANGE, [(1, 'Player', SOLDIER, 0, True)])
        match, player = game.matches[0], game.matches[0].players[0]
        player.client = _Client()
        player.spawned = player.has_body = True
        player.body_picked_at = 1
        match.update(3, 1)
        player.body_script.arrived(player.body_script.data_last)
        match.bots, match.mesh = [], None
        player.client.updates.clear()
        return match, player

    def test_native_helix_waits_for_volley_ack_and_preserves_other_viewer_fallback(self):
        from combat import aim_direction
        from ow174.game import world
        from ow174.game.bits import BitReader
        match, player = self._native_helix_match()
        player.take_commands([Command(100, buttons=2, yaw=16384)])
        rocket, = match.helix_rockets
        self.assertEqual((rocket.entity, rocket.shot.end - rocket.shot.start), (0xB3000000, 5.0))
        match.update(3.01, 2)
        self.assertGreater(rocket.begin_frame, 0)
        match.update(3.02, 3)
        self.assertFalse(rocket.records)
        self.assertEqual(len(player.helix.updates), 1)  # Stop must retain the config until native startup
        self.assertEqual(match._projectile_updates(player), [])
        other = match.add_player(2, 'Viewer', SOLDIER, 1, True)
        self.assertTrue(match._projectile_updates(other))
        player.body_script.arrived(rocket.begin_frame)
        with (patch.object(world, 'projectile_state', wraps=world.projectile_state) as snapshot,
              patch.object(world, 'shot_record', wraps=world.shot_record) as event):
            match.update(3.03, 4)
        create, shot, state = rocket.records
        self.assertEqual((create.entity, create.op), (rocket.entity, world.OP_CREATE))
        self.assertEqual((shot.entity, shot.shot_time), (player.body, 100))
        position = rocket.shot.position(3.03)
        self.assertNotEqual(position, rocket.shot.origin)
        self.assertEqual(event.call_args.args[4], rocket.shot.origin)  # muzzle FX must stay at the launch origin
        velocity = tuple(value * 50 for value in aim_direction(16384, 0))
        self.assertEqual(snapshot.call_args.args[:2], (100, position))
        for actual, expected in zip(snapshot.call_args.args[2], velocity):
            self.assertAlmostEqual(actual, expected)
        self.assertEqual(snapshot.call_args.kwargs, dict(event_count=0, source_entities=(player.body, player.body)))
        reader = BitReader(create.build(0).getvalue())
        self.assertEqual((reader.bits(2), reader.bits(12)), (0b10, 0x072C))
        self.assertFalse(reader.bits(8) & world.WITH_MOVEMENT)
        self.assertEqual(len(player.helix.updates), 1)
        for record in rocket.records:
            record.delivered = True
        player.body_script.arrived(player.body_script.data_last)
        match.update(3.11, 5)
        self.assertFalse(player.helix.updates)
        match._tick_rockets(3.6)
        self.assertEqual(match.rocket_ids, 0x01000000)  # flight never re-arms client prediction
        match._tick_rockets(8.0)
        self.assertEqual(match.helix_rockets, [])
        self.assertEqual(player.client.updates[-1][0].op, world.OP_DESTROY)
        self.assertEqual({item.entity for item in player.client.updates[-1]}, {rocket.entity})
        player.body_script.arrived(player.body_script.data_last)
        match.now = 9
        player.take_commands([Command(1000, buttons=2)])
        following, = match.helix_rockets
        self.assertEqual(following.entity, 0xB3000001)
        self.assertEqual(following.serial, rocket.serial + 1)

    def test_native_helix_retires_impact_body_switch_and_disconnect(self):
        from ow174.game import world
        for reason in ('impact', 'body', 'disconnect'):
            with self.subTest(reason=reason):
                match, player = self._native_helix_match()
                player.take_commands([Command(100, buttons=2)])
                match.update(3.01, 2)
                rocket, = match.helix_rockets
                player.body_script.arrived(rocket.begin_frame)
                match.update(3.02, 3)
                client = player.client
                if reason == 'impact':
                    rocket.shot.landed = True
                elif reason == 'body':
                    player.body += 0x100
                else:
                    player.client = None
                match._tick_rockets(3.03)
                self.assertEqual(match.helix_rockets, [])
                if reason != 'disconnect':
                    destroyed = {item.entity for group in client.updates for item in group
                                 if item.op == world.OP_DESTROY}
                    self.assertIn(rocket.entity, destroyed)

    def test_remote_volley_frame_omits_owner_header_and_instance_events(self):
        from ow174.game.statescript import Graph, Instance, remote_full_frame, variables_frame
        from ow174.game.bits import BitWriter, BitReader
        payload = BitWriter()
        payload.bit(1)  # desired active
        payload.bit(0)  # optional integer defaults to1
        payload.bits(1, 6)
        frame = remote_full_frame([Instance(1, Graph(0xBA3, 32, 0, 1), active={0: payload})], {})
        self.assertEqual(frame.count, 46)
        bits = BitReader(frame.getvalue())
        self.assertEqual(bits.bits(2), 0)  # lead0,H0; no C/CmFD
        self.assertEqual(bits.bits(5), 2)  # compact instance1
        self.assertEqual(bits.bits(16), 0xBA3)
        self.assertEqual(bits.bits(4), 0)  # descriptor flags
        self.assertEqual(bits.bits(5), 0)  # descriptor sentinel
        self.assertEqual(bits.bits(2), 0)  # entity vars, instance extra vars
        self.assertEqual(bits.bits(3), 7)  # remote presence, custom gate, desired active
        self.assertEqual(bits.bit(), 0)
        self.assertEqual(bits.bits(6), 1)
        self.assertEqual(bits.bits(2), 0)  # frame events; no instance event terminator
        self.assertEqual(remote_full_frame([], {}).getvalue(), variables_frame({}).getvalue())
        for bit in (-1, 1, True):
            with self.assertRaises(ValueError):
                remote_full_frame([Instance(1, Graph(0xBA3, 32, 0, 1), active={bit: payload})], {})
        # A silent slot (not a state node) takes no presence bit and cannot be set.
        silent = Graph(0xD28, 78, 7, 3, frozenset({1}))
        frame = remote_full_frame([Instance(1, silent, active={2: None})], {})
        bits = BitReader(frame.getvalue())
        bits.bits(2 + 5 + 16 + 4 + 5 + 1 + 7 + 1)  # header, descriptor, entity vars, 7 sync vars, extra vars
        self.assertEqual(bits.bits(4), 0b0110)  # slot 0 absent, slot 2 present + control; no bit for slot 1
        with self.assertRaises(ValueError):
            remote_full_frame([Instance(1, silent, active={1: None})], {})

    def test_normal_practice_initializes_hud_and_reloadable_rifle_after_body_delay(self):
        from ow174.game.statescript import StateBound, Entity
        game = practice.PracticeRange(port=0).game
        game.create_match(PRACTICE_RANGE, [(1, 'Player', SOLDIER, 0, True)])
        match = game.matches[0]
        self.assertTrue(match.gameplay_enabled)
        player = match.players[0]
        player.client = _Client()
        player.spawned = player.has_body = True
        player.body_picked_at = 1
        match.update(2.99, 1)
        self.assertEqual(player.client.updates, [])
        match.update(3, 2)
        self.assertIsNotNone(player.body_script)
        self.assertIsNotNone(match.target)
        self.assertEqual(player.weapon_entity, StateBound(Entity(0), 1, 11, 7))
        self.assertEqual([item.index for item in player.body_extra_instances], [21, 75, 4429, 463, 67, 597, 599, 601])
        self.assertTrue({39, 65, 66, 79} <= player.weapon_extra_states.keys())
        player.take_commands([Command(3, buttons=1), Command(4)])
        self.assertEqual(player.rifle.ammo, 29)
        self.assertEqual(len(player.rifle.updates), 2)

    def test_player_health_is_synced_for_the_hud_number(self):
        from ow174.game.statescript import Float
        game = practice.PracticeRange(port=0).game
        game.create_match(PRACTICE_RANGE, [(1, 'Player', SOLDIER, 0, True)])
        match = game.matches[0]
        player = match.players[0]
        player.client = _Client()
        player.spawned = player.has_body = True
        player.body_picked_at = 1
        match.update(3, 2)
        self.assertEqual((player.body_extra_variables[1732], player.body_extra_variables[1737]), (Float(200.0), Float(200.0)))
        player.body_dirty = False
        match._damage_player(player, 40, 3)
        self.assertEqual(player.body_extra_variables[1732], Float(160.0))
        self.assertTrue(player.body_dirty)

    def test_sprint_runs_server_side_and_resends_the_body_frame(self):
        from soldier import ANIM_CATEGORY, SPRINT_BUTTON, SPRINT_CATEGORY
        from ow174.game.statescript import Asset, Float
        game = practice.PracticeRange(port=0).game
        game.create_match(PRACTICE_RANGE, [(1, 'Player', SOLDIER, 0, True)])
        match = game.matches[0]
        player = match.players[0]
        player.client = _Client()
        player.spawned = player.has_body = True
        player.body_picked_at = 1
        match.update(3, 2)
        player.body_script.delivered = player.body_script.last  # the first frame arrived
        start = player.mover.position[2]
        player.take_commands([Command(10 + i, forward=127, buttons=SPRINT_BUTTON) for i in range(10)])
        self.assertTrue(player.sprint.active)
        self.assertAlmostEqual(player.mover.speed_scale, 1.5)
        sprint = next(item for item in player.body_extra_instances if item.index == 597)
        self.assertTrue({0, 1, 5, 9, 13, 19, 20, 27, 29} <= sprint.active.keys())
        self.assertEqual(sprint.presence, {5: Float(0.5)})
        # Stack 5's anim category, and graph 75's ForCodeValue state that makes the animator reread it
        self.assertEqual(player.body_extra_variables[ANIM_CATEGORY], Asset(SPRINT_CATEGORY))
        self.assertIn(23, next(item for item in player.body_extra_instances if item.index == 75).active)
        self.assertTrue(player.body_dirty)
        sent = len(player.client.updates)
        match.update(3.1, 3)
        self.assertEqual(len(player.client.updates), sent + 1)
        self.assertFalse(player.body_dirty)
        player.take_commands([Command(20, forward=127, buttons=SPRINT_BUTTON | 1)])  # firing cancels
        self.assertFalse(player.sprint.active)
        self.assertEqual(player.mover.speed_scale, 1.0)
        self.assertNotIn(0, next(item for item in player.body_extra_instances if item.index == 597).active)
        self.assertEqual(player.body_extra_variables[ANIM_CATEGORY], Asset(0))
        fire_effect = player.weapon_extra_states[21]  # graph 596's fire effect: off until a shot
        self.assertEqual((fire_effect.count, fire_effect.value), (3, 0))
        self.assertNotEqual(player.mover.position[2], start)

    def test_damage_and_ability_refresh_do_not_resend_reload_start(self):
        from ow174.game.bits import BitReader
        from soldier import RifleUpdate

        game = practice.PracticeRange(port=0).game
        game.create_match(PRACTICE_RANGE, [(1, 'Player', SOLDIER, 0, True)])
        match = game.matches[0]
        player = match.players[0]
        player.client = _Client()
        player.spawned = player.has_body = True
        player.body_picked_at = 1
        match.update(3, 2)
        stream = player.body_script
        stream.arrived(stream.data_last)
        player.rifle.updates.append(RifleUpdate(29, 1, False, True, True))
        match.update(3.1, 3)
        stream.arrived(stream.data_last)

        match._damage_player(player, 20, 3.2)
        match.update(3.2, 4)
        damage = player.client.updates[-1][0]
        self.assertTrue(damage.resend)
        self.assertTrue(stream.data_in_flight)
        reader = BitReader(damage.chunk.getvalue())
        self.assertEqual(reader.bit(), 0)  # first fits the 16-bit chunk-number field
        self.assertGreater(reader.bits(16), 0)  # delta, not a full frame
        reader.var_b()  # span
        self.assertEqual(reader.bit(), 0)  # unfragmented
        reader.var_b()  # payload size
        self.assertEqual(reader.bits(3), 0b110)  # authoritative owner data header
        reader.var_b()  # CmFD
        reader.var_b()  # delta body length
        self.assertEqual(reader.bits(5), 0)  # no changed graph instances, including the rifle
        self.assertEqual(reader.bit(), 1)
        self.assertEqual(reader.var_b(), 1732)  # HUD current health
        stream.arrived(stream.data_last)

        player.field.command(Command(100, buttons=player.field.button))
        match.update(3.3, 5)
        ability = player.client.updates[-1][0]
        self.assertTrue(ability.resend)
        reader = BitReader(ability.chunk.getvalue())
        self.assertEqual(reader.bit(), 0)
        self.assertGreater(reader.bits(16), 0)

        stream.arrived(stream.data_last)
        match.update(3.4, 6)  # Field stop
        stream.arrived(stream.data_last)
        player.helix.command(Command(110, buttons=player.helix.button))
        match.update(3.5, 7)
        helix = player.client.updates[-1][0]
        reader = BitReader(helix.chunk.getvalue())
        self.assertEqual(reader.bit(), 0)
        self.assertGreater(reader.bits(16), 0)
        reader.var_b()  # span
        reader.bit()  # fragmentation
        reader.var_b()  # payload size
        reader.bits(3)  # owner header
        reader.var_b()  # CmFD
        reader.var_b()  # delta body length
        self.assertEqual(reader.bits(5), 2)  # instance step 1, compact four-bit integer
        self.assertEqual(reader.bits(2), 0)  # existing instance, no descriptor change
        self.assertEqual(reader.bits(5), 0)  # end of instance list
        self.assertEqual(reader.bit(), 0)  # no entity vars changed
        self.assertEqual(reader.bits(16), 0)  # native rifle table has 128 sync slots
        self.assertEqual(reader.bit(), 0)  # no extra instance vars
        self.assertEqual(reader.bits(11), 0b1110)  # native rifle table has 85 state slots
        self.assertEqual((reader.bits(8), reader.bits(8), reader.bits(8)), (0x80, 0x01, 0xC0))

    def test_ability_payload_is_flag_byte_linked_bit_and_timing_bit(self):
        from soldier import ability_payload
        from ow174.game.bits import BitReader
        payload = ability_payload(1)
        self.assertEqual(payload.count, 10)
        reader = BitReader(payload.getvalue())
        self.assertEqual(reader.bits(8), 1)
        self.assertEqual(reader.bits(2), 0)

    def test_reload_animation_uses_native_reference_counter_tail_and_clears_on_finish(self):
        from soldier import RifleUpdate
        from ow174.game.statescript import Bool
        from ow174.game.bits import BitReader
        marker = {39: None}
        update = RifleUpdate(20, 10, False, True, reloading=True)
        instance = weapon_instance(20, update, marker)
        self.assertEqual(instance.extra[51], Bool(True))
        reader = BitReader(instance.active[39].getvalue())
        self.assertEqual(reader.bit(), 0)  # no linked state reference
        self.assertEqual(reader.bits(3), 2)  # event counter wraps
        self.assertEqual(reader.bit(), 0)  # optional time override absent
        self.assertEqual(instance.active[39].count, 5)
        for update in (None, RifleUpdate(30, 10, False, True)):
            instance = weapon_instance(30, update, marker)
            self.assertEqual(instance.extra[51], Bool(False))
            self.assertNotIn(39, instance.active)
        self.assertNotIn(51, weapon_instance().extra)

    def test_firing_feedback_is_transient_and_keeps_both_variable_scopes(self):
        from soldier import RifleUpdate
        from ow174.game.statescript import Bool
        marker = {23: None, 68: None}
        for update, firing in ((None, False), (RifleUpdate(29, 1, True, True), True),
                               (RifleUpdate(29, 1, False, True), False)):
            instance = weapon_instance(29, update, marker)
            self.assertEqual(instance.presence[6], Bool(firing))
            self.assertEqual(23 in instance.active, firing)
            self.assertEqual(68 in instance.active, firing)
            if firing:
                for bit in (23, 68):
                    self.assertEqual((instance.active[bit].count, instance.active[bit].value), (2, 1))
            with patch('soldier.owner_full_frame', return_value=None) as frame:
                body_frame(10, update, marker)
            self.assertEqual(frame.call_args.args[2][689], Bool(firing))
        self.assertNotIn(6, weapon_instance().presence)

    def test_state_bound_value_uses_native_binding_record_and_signed_instance(self):
        from ow174.game.statescript import Entity, StateBound, write_variable
        from ow174.game.bits import BitWriter, BitReader
        value = StateBound(Entity(0), 1, 11, 7)
        out = BitWriter()
        write_variable(out, value)
        reader = BitReader(out.getvalue())
        self.assertEqual(reader.bits(4), 13)
        self.assertEqual(reader.bit(), 1)
        self.assertEqual(reader.bits(32), 0)
        self.assertEqual(reader.bits(2), 1)  # compact count1: first1, then0
        self.assertEqual(reader.bit(), 0)  # no parameters
        self.assertEqual(reader.bit(), 1)  # explicit instance
        self.assertEqual(reader.bit(), 0)  # positive signed instance
        self.assertEqual(reader.bits(5), 2)  # w_u16(1)
        self.assertEqual(reader.bits(7), 11)
        self.assertEqual(reader.bits(3), 0)  # default counter, priority and flags
        self.assertEqual(out.count, 57)
        for instance, state, width in ((0, 11, 7), (32768, 11, 7), (True, 11, 7),
                                      (1, -1, 7), (1, 128, 7), (1, 11, 0), (1, 11, 10)):
            with self.assertRaises(ValueError):
                StateBound(Entity(0), instance, state, width)

    def test_foreign_descriptor_entity_uses_existing_packed_id_and_keeps_flags_aligned(self):
        from ow174.game.statescript import Graph, Instance, _descriptor
        from ow174.game.bits import BitWriter, BitReader
        for entity in (0x80000001, 0xA0000101, 0xD0000010, 0xE0000101):
            out = BitWriter()
            _descriptor(out, Instance(3, Graph(4429,1,0), source_entity=entity))
            reader = BitReader(out.getvalue())
            self.assertEqual(reader.bits(16),4429)
            self.assertEqual(reader.bit(),1)
            self.assertEqual(reader.entity_id(),entity)
            self.assertEqual(reader.bits(3),0)
        for entity in (0, True, 0x100, 0xA0000000, 0xFFFFFFFF, 0x100000000):
            with self.assertRaises(ValueError):
                _descriptor(BitWriter(), Instance(3, Graph(4429,1,0), source_entity=entity))

    def test_script_reference_uses_native_handle_word_order(self):
        from ow174.game.statescript import ScriptRef
        from ow174.game.bits import BitWriter, BitReader
        out = BitWriter()
        ScriptRef(4, 3).write(out)
        reader = BitReader(out.getvalue())
        self.assertEqual(reader.bits(4), 14)
        self.assertEqual(reader.bits(3), 4)
        self.assertEqual(reader.var_b(), 4)
        self.assertEqual(reader.bits(5), 3 << 1)
        self.assertEqual(reader.bits(5), 0)
        for instance, state in ((0, 3), (65536, 3), (4, -1), (4, 65536), (True, 3)):
            with self.assertRaises(ValueError):
                ScriptRef(instance, state).write(BitWriter())

    def test_array_is_a_plain_list_of_tagged_values(self):
        from ow174.game.statescript import Array, Int
        from ow174.game.bits import BitWriter, BitReader
        out = BitWriter()
        Array((Int(7),)).write(out)
        reader = BitReader(out.getvalue())
        self.assertEqual((reader.bits(4), reader.bits(3)), (15, 6))
        self.assertEqual(reader.bits(5), 1 << 1)  # w_u16 count 1
        self.assertEqual((reader.bit(), reader.bit()), (0, 0))
        self.assertEqual((reader.bits(4), reader.bits(32)), (4, 7))

    def test_explicit_muzzle_entity_uses_native_presence_slot(self):
        from ow174.game.statescript import Entity
        self.assertNotIn(7, weapon_instance().presence)
        self.assertEqual(weapon_instance(weapon_entity=0xA0000101).presence[7], Entity(0xA0000101))
        for value in (0, -1, 0x100000000, True):
            with self.assertRaises(ValueError):
                weapon_instance(weapon_entity=value)

    def test_volley_keeps_the_six_bit_event_counter_across_stop_and_wrap(self):
        from soldier import RifleUpdate, volley_payload
        for serial in (1, 63, 64, 65535):
            for active in (True, False):
                payload = volley_payload(RifleUpdate(29, serial, active, True))
                self.assertEqual((payload.value >> (payload.count - 6)) & 63, serial & 63)

    def test_muzzle_event_counter_wraps_and_survives_stop(self):
        from soldier import RifleUpdate
        from ow174.game.bits import BitReader
        for serial in (1, 4, 65535):
            for firing in (True, False):
                instance = weapon_instance(29, RifleUpdate(29, serial, firing, True), {21: None})
                payload = instance.active[21]
                self.assertEqual(payload.count, 3)
                bits = BitReader(payload.getvalue())
                self.assertEqual(bits.bits(1), int(firing))
                self.assertEqual(bits.bits(2), serial & 3)
        self.assertNotIn(21, weapon_instance(29, RifleUpdate(29, 1, True, True)).active)

    def test_normal_launcher_keeps_unverified_gameplay_disabled(self):
        match = Match(PRACTICE_RANGE)
        player = match.add_player(1, 'Player', SOLDIER, 0, True)
        player.has_body = player.spawned = True
        player.body_picked_at = 1
        player.client = _Client()
        match.update(3, 10)
        player.take_commands([Command(11, buttons=1)])
        self.assertIsNone(match.target)
        self.assertEqual(player.rifle.serial, 0)

    def test_command_frame_zero_is_not_replaced_with_server_tick(self):
        match = Match(PRACTICE_RANGE)
        player = match.add_player(1, 'Player', SOLDIER, 0, True)
        match.tick = 1000
        self.assertEqual(match._newest_frame(player), 1000)
        player.commands.last_frame = 0
        self.assertEqual(match._newest_frame(player), 0)

    def test_held_weapon_uses_verified_minimum_state(self):
        instance = weapon_instance()
        self.assertEqual(instance.graph.index, 596)
        self.assertEqual(instance.graph.owner_states, 81)
        self.assertEqual(instance.graph.sync_vars, 23)
        self.assertIn(11, instance.active)  # state11 is the held cosmetic entity
        self.assertEqual(instance.presence[4].value, 1)  # weapon target id
        self.assertEqual(instance.presence[8].value, 30)
        self.assertEqual(instance.presence[13].value, 30)
        self.assertEqual(set(instance.active), {0, 11, 12})
        self.assertEqual((instance.active[0].value, instance.active[0].count), (1, 1))
        self.assertEqual((instance.active[12].value, instance.active[12].count), (0, 1))

    def test_body_frame_requires_a_fresh_command_frame(self):
        with self.assertRaises(ValueError):
            body_frame(0)
        self.assertGreater(body_frame(1).count, 81)

    def test_body_frame_waits_then_sends_once_for_soldier(self):
        match = Match(PRACTICE_RANGE)
        player = match.add_player(1, 'Player', SOLDIER, 0, True)
        player.client = _Client()
        player.spawned = player.has_body = True
        player.body_picked_at = 1.0

        match.update(2.99, 1)
        self.assertEqual(player.client.updates, [])
        match.update(3.0, 2)
        self.assertEqual(len(player.client.updates), 1)
        self.assertIs(player.client.updates[0][0].stream, player.body_script)
        match.update(4.0, 3)
        self.assertEqual(len(player.client.updates), 1)

    def test_new_pick_resets_body_frame_and_delay(self):
        match = Match(PRACTICE_RANGE)
        player = match.add_player(1, 'Player', SOLDIER, 0, True)
        player.has_body = True
        player.body_script = object()
        player.body_picked_at = 2.0
        player.rifle.command(Command(1, buttons=1))
        old_body = player.body

        with patch('ow174.game.match.time.monotonic', return_value=10.0), \
                patch('ow174.game.match.time.time', return_value=1000.0):
            match.switch_hero(player, player.hero)

        self.assertNotEqual(player.body, old_body)
        self.assertIsNone(player.body_script)
        self.assertEqual(player.body_picked_at, 1000.0)
        self.assertEqual(player.rifle.ammo, 30)
        self.assertFalse(player.rifle.updates)

    def test_body_ack_waits_for_initial_frame_delivery(self):
        match = Match(PRACTICE_RANGE)
        player = match.add_player(1, 'Player', SOLDIER, 0, True)
        player.client = _Client()
        player.spawned = player.has_body = True
        player.body_picked_at = 1.0
        player.commands.last_frame = 10
        match.update(3.0, 2)
        stream = player.body_script
        player.commands.last_frame = 20
        match.update(3.1, 3)
        self.assertEqual(len(player.client.updates), 1)
        stream.arrived(stream.data_last)
        match.update(3.3, 4)
        self.assertEqual(len(player.client.updates), 2)
        ack = player.client.updates[-1][0]
        self.assertIs(ack.stream, stream)
        self.assertFalse(ack.resend)
        self.assertEqual(stream.cmfd, 20)

    def test_a_tap_queues_delivered_start_then_explicit_stop(self):
        match = Match(PRACTICE_RANGE, gameplay=True)
        player = match.add_player(1, 'Player', SOLDIER, 0, True)
        player.client = _Client()
        player.spawned = player.has_body = True
        player.body_picked_at = 1
        match.update(3, 10)
        player.weapon_extra_states = {65: None}
        from ow174.game.statescript import Entity, Graph, Instance, Int, StateBound
        player.weapon_entity = StateBound(Entity(0), 1, 11, 7)
        player.body_extra_instances = [Instance(2, Graph(75, 39, 4),
            presence={2: Int(1)}, active={19: None})]
        player.body_extra_variables = {338: Int(1650), 982: Int(1)}
        player.take_commands([Command(11, buttons=1), Command(12)])
        self.assertEqual(player.rifle.ammo, 29)
        self.assertEqual(len(player.rifle.updates), 2)
        match.update(3.01, 11)
        self.assertEqual(len(player.client.updates), 1)
        stream = player.body_script
        stream.arrived(stream.data_last)
        with patch.object(player, 'kit_frame', wraps=body_frame) as build:
            match.update(3.02, 12)
        self.assertEqual(build.call_args.args[2], {65: None})
        self.assertEqual(build.call_args.args[3], player.weapon_entity)
        self.assertEqual(build.call_args.kwargs['extra_instances'], player.body_extra_instances)
        self.assertEqual(build.call_args.kwargs['extra_variables'], player.body_extra_variables)
        self.assertEqual(len(player.rifle.updates), 1)
        match.update(3.03, 13)
        self.assertEqual(len(player.rifle.updates), 1)
        stream.arrived(stream.data_last)
        match.update(3.04, 14)
        self.assertEqual(len(player.rifle.updates), 1)
        with patch.object(player, 'kit_frame', wraps=body_frame) as build:
            match.update(3.06, 16)
        self.assertEqual(build.call_args.args[3], player.weapon_entity)
        self.assertEqual(build.call_args.kwargs['extra_instances'], player.body_extra_instances)
        self.assertEqual(build.call_args.kwargs['extra_variables'], player.body_extra_variables)
        self.assertEqual(len(player.rifle.updates), 0)


if __name__ == '__main__':
    unittest.main()
