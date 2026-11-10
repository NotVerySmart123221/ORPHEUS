import sys
import unittest
from dataclasses import replace
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, mock_open, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "lobby"))
sys.path.insert(0, str(Path(__file__).resolve().parent))

import practice
import soldier
from body_probe import BodyProbeServer, PHASES, ProbeSchedule, frame_for_phase, launch as launch_probe
from body_probe import queue_probe_frame, serve, volley_payload, hud_frame
from body_probe import rifle_states
from ow174.game.statescript import Bool, Float, Graph, Instance, Int, owner_full_frame


class BodyProbeFrameTests(unittest.TestCase):
    def test_remote_bot_initialization_preserves_authored_entry_inputs(self):
        from ow174.game.statescript import Stream, remote_full_frame
        player = SimpleNamespace(entity=0xA0000100, body=0xA0000101,
            body_script=Stream(0xA0000101),
            match=SimpleNamespace(bots=[SimpleNamespace(entity=0xB0000001, behaviour=None, position=(0, 0, 0))]),
            mover=SimpleNamespace(position=(0, 0, 0)),
            client=SimpleNamespace(queue_entities=lambda updates: None))
        with patch('body_probe.remote_full_frame', wraps=remote_full_frame) as build:
            queue_probe_frame(player, 'rifle-bot-remote-init', 10)
        bot, = build.call_args.args[0]
        self.assertEqual(set(bot.active), {3, 7})
        self.assertEqual(bot.presence[4], Int(0))
        self.assertEqual(bot.presence[5], Float(2.75))
        self.assertEqual(bot.presence[8], Bool(True))
        self.assertEqual(bot.extra, {198: Int(20), 53: Int(20), 6884: Float(.489)})

    def test_remote_bot_probe_uses_verified_remote_slot_and_short_volley_payload(self):
        from ow174.game.statescript import Stream, Asset, remote_full_frame
        player = SimpleNamespace(entity=0xA0000100, body=0xA0000101,
            body_script=Stream(0xA0000101),
            match=SimpleNamespace(bots=[SimpleNamespace(entity=0xB0000001, behaviour=None, position=(0, 0, 0))]),
            mover=SimpleNamespace(position=(0, 0, 0)),
            client=SimpleNamespace(queue_entities=lambda updates: None))
        with patch('body_probe.remote_full_frame', wraps=remote_full_frame) as build:
            queue_probe_frame(player, 'rifle-bot-remote', 10)
        bot, = build.call_args.args[0]
        self.assertEqual(bot.graph, Graph(0xBA3, 32, 13, 21))
        self.assertEqual(bot.presence, {12: Asset(0x0DC0000000000005)})
        self.assertEqual(set(bot.active), {7})
        self.assertEqual((bot.active[7].count, bot.active[7].value), (8, 5))
        self.assertEqual(player.body_extra_instances[-1].graph, Graph(463, 41, 22))

    def test_normal_practice_loadout_matches_native_verified_probe_bytes(self):
        player = SimpleNamespace(quanta=16000, health=175, max_health=200)
        soldier.initialize_practice_soldier(player)
        self.assertEqual(player.body_extra_variables[1732], Float(175))
        self.assertEqual(player.body_extra_variables[1737], Float(200))
        self.assertIn(soldier.MELEE.index, {item.index for item in player.body_extra_instances})
        # Compare the part of the loadout that predates abilities, quick melee and numeric health.
        # Later fixes left out: native reload input (state3), fire effect21 off at spawn, and Sprint's anim
        # category (var1273 and graph75's ForCodeValue state23).
        states = {bit: value for bit, value in player.weapon_extra_states.items()
                  if bit != 3 and bit not in soldier.helix_states()}
        states[21] = soldier.muzzle_payload(True, 0)
        instances = [replace(item, active={k: v for k, v in item.active.items() if not (item.index == 75 and k == 23)})
                     for item in player.body_extra_instances if item.index not in (soldier.MELEE.index, 597, 599, 601)]
        variables = {var: value for var, value in player.body_extra_variables.items()
                     if var not in (soldier.ULT_CHARGE, 1732, 1737, soldier.ANIM_CATEGORY)}
        actual = soldier.body_frame(10, extra_active=states,
            weapon_entity=player.weapon_entity, extra_instances=instances, extra_variables=variables)
        expected = frame_for_phase('rifle-shared-hud', 10, player.weapon_entity)
        self.assertEqual((actual.count, actual.getvalue()), (expected.count, expected.getvalue()))

    def test_shared_hud_parent_probe_uses_authored_body_subscript(self):
        from body_probe import body_startup
        from ow174.game.statescript import subscript
        instances, _ = body_startup('rifle-shared-parent')
        body = next(item for item in instances if item.index == 75)
        child = next(item for item in instances if item.index == 463)
        self.assertEqual(child.parent, (75, 3))
        self.assertEqual(body.active[3].getvalue(), subscript(463).getvalue())
        self.assertTrue({39, 65, 66, 79} <= rifle_states('rifle-shared-parent').keys())

    def test_bot_feedback_connects_authored_shoot_flag_to_dataflow(self):
        from ow174.game.statescript import Stream
        queued = []
        player = SimpleNamespace(entity=0xA0000100, body=0xA0000101,
            body_script=Stream(0xA0000101),
            match=SimpleNamespace(bots=[SimpleNamespace(entity=0xB0000001, behaviour=None, position=(0, 0, 0))]),
            mover=SimpleNamespace(position=(0, 0, 0)),
            client=SimpleNamespace(queue_entities=queued.extend))
        with patch('body_probe.owner_full_frame', wraps=owner_full_frame) as build:
            queue_probe_frame(player, 'rifle-bot-feedback', 10)
        bot = build.call_args.args[1][0]
        self.assertEqual(set(bot.active), {7, 10, 13, 26})
        self.assertEqual(bot.presence[8], Bool(True))
        self.assertEqual(build.call_args.args[2], {233: Bool(True)})
        self.assertEqual(player.body_extra_instances[-1].graph, Graph(463, 41, 22))

    def test_attack_probe_keeps_shared_hud_and_native_muzzle_binding(self):
        from ow174.game.statescript import Entity, StateBound
        queued = []
        player = SimpleNamespace(entity=0xA0000100, body=0xA0000101, body_script=None,
            client=SimpleNamespace(queue_entities=queued.extend))
        queue_probe_frame(player, 'rifle-attack', 10)
        self.assertTrue(player._probe_ui_flags)
        self.assertEqual(player.body_extra_instances[-1].graph, Graph(463, 41, 22))
        self.assertEqual(player.weapon_entity, StateBound(Entity(0), 1, 11, 7))
        self.assertTrue({39, 65, 66, 79} <= player.weapon_extra_states.keys())

    def test_shared_body_hud_probe_keeps_client_host_and_weapon_ui(self):
        from body_probe import body_startup
        instances, variables = body_startup('rifle-shared-hud')
        self.assertEqual([item.index for item in instances], [21, 75, 4429, 463])
        self.assertEqual(instances[-1].graph, Graph(463, 41, 22))
        self.assertEqual(instances[-1].active, {39: None, 40: None})
        from ow174.game.statescript import Asset
        self.assertEqual(instances[-1].presence, {0: Asset(0x0C00000000007DA7)})
        states = rifle_states('rifle-shared-hud')
        self.assertTrue({39, 65, 66, 79} <= states.keys())
        self.assertEqual((states[63].count, states[63].value), (1, 0))

    def test_mode_hud_probe_preserves_verified_controller_flag_and_body_root(self):
        from ow174.game.statescript import Stream
        queued = []
        player = SimpleNamespace(entity=0xA0000100, body=0xA0000101, body_script=None,
            script=Stream(0xA0000100), mode_script=Stream(0xD0000010),
            client=SimpleNamespace(queue_entities=queued.extend))
        queue_probe_frame(player, 'rifle-hud-mode', 10)
        self.assertEqual([update.entity for update in queued],
            [0xD0000010, player.entity, player.entity, player.body])
        self.assertTrue(player._probe_ui_flags)
        self.assertEqual([item.index for item in player.body_extra_instances], [21, 75, 4429])
        self.assertEqual(player.weapon_extra_states.keys(), rifle_states('rifle-reload').keys())

    def test_body_hud_probe_adds_client_graph_beneath_working_root_setup(self):
        from body_probe import body_startup
        instances, variables = body_startup('rifle-root-hud')
        self.assertEqual([item.index for item in instances], [21, 75, 4429, 4418])
        self.assertEqual(instances[-1].graph, Graph(4418, 1, 0))
        self.assertEqual(instances[-1].active, {0: None})
        self.assertEqual(variables[3772], Bool(True))

    def test_root_flags_probe_updates_the_player_and_clears_on_phase_change(self):
        from ow174.game import world
        queued = []
        player = SimpleNamespace(entity=0xA0000100, body=0xA0000101, body_script=None,
            client=SimpleNamespace(queue_entities=queued.extend))
        queue_probe_frame(player, 'rifle-root-flags', 10)
        flag_update, body_update = queued
        self.assertEqual((flag_update.entity, flag_update.op), (player.entity, world.OP_UPDATE))
        self.assertEqual(flag_update.build(0).getvalue(), bytes.fromhex('015501010101'))
        self.assertEqual(player.body_extra_instances[-1].graph, Graph(4429, 1, 0))
        queued.clear()
        queue_probe_frame(player, 'rifle-reload', 20)
        flag_update, body_update = queued
        self.assertEqual(flag_update.build(0).getvalue(), bytes.fromhex('0155010100'))
        self.assertFalse(player._probe_ui_flags)

    def test_projectile_probe_uses_authored_entity_and_retains_rifle_binding(self):
        from ow174.game import world
        from ow174.game.statescript import StateBound, Entity
        queued = []
        player = SimpleNamespace(body=0xA0000101, body_script=None,
            mover=SimpleNamespace(position=(1, 2, 3), yaw=0),
            client=SimpleNamespace(queue_entities=queued.extend))
        queue_probe_frame(player, 'rifle-projectile', 10)
        fixture, frame = queued
        self.assertEqual(player.weapon_entity, StateBound(Entity(0), 1, 11, 7))
        self.assertIsNone(fixture.movement)  # Native1476 has no ordinary movement component.
        with patch.object(world, 'create') as create:
            fixture.build(0)
        self.assertEqual(create.call_args.args[:3], (0, 0x0400000000001476, {}))
        self.assertEqual(create.call_args.kwargs, {'position': (1, 3.6, 3.5)})
        queued.clear()
        queue_probe_frame(player, 'rifle-reload', 20)
        self.assertEqual((queued[0].entity, queued[0].op), (fixture.entity, world.OP_DESTROY))

    def test_ui_root_probe_is_owned_by_the_possessed_body_and_survives_shots(self):
        from ow174.game.statescript import Stream, StateBound, Entity
        queued = []
        player = SimpleNamespace(body=0xA0000101, body_script=Stream(0xA0000101),
            client=SimpleNamespace(queue_entities=queued.extend))
        queue_probe_frame(player, 'rifle-root-body', 10)
        self.assertEqual(queued[-1].entity, player.body)
        root = player.body_extra_instances[-1]
        self.assertEqual(root.graph, Graph(4429, 1, 0))
        self.assertEqual(root.index, 4429)
        self.assertIsNone(root.source_entity)
        self.assertEqual(root.active, {0: None})
        self.assertEqual(player.weapon_entity, StateBound(Entity(0), 1, 11, 7))
        self.assertEqual(player.weapon_extra_states.keys(), rifle_states('rifle-reload').keys())
        with patch('soldier.owner_full_frame', wraps=owner_full_frame) as build:
            soldier.body_frame(11, soldier.RifleUpdate(29, 1, True, True),
                extra_active=player.weapon_extra_states, weapon_entity=player.weapon_entity,
                extra_instances=player.body_extra_instances,
                extra_variables=player.body_extra_variables)
        self.assertIn(root, build.call_args.args[1])

    def test_bot_probe_reuses_transport_stream_and_replaces_it_on_target_respawn(self):
        from ow174.game.statescript import Stream
        queued = []
        player = SimpleNamespace(body=0xA0000101, body_script=Stream(0xA0000101),
            match=SimpleNamespace(bots=[SimpleNamespace(entity=0xB0000001, behaviour=None, position=(0, 0, 0))]),
            mover=SimpleNamespace(position=(0, 0, 0)),
            client=SimpleNamespace(queue_entities=queued.extend))
        queue_probe_frame(player, 'rifle-bot-graph', 10)
        from ow174.game.statescript import StateBound, Entity
        self.assertEqual(player.weapon_entity, StateBound(Entity(0), 1, 11, 7))
        self.assertEqual([item.index for item in player.body_extra_instances], [21, 75])
        first = queued[-1]
        self.assertEqual(first.entity, 0xB0000001)
        self.assertIsNot(first.stream, player.body_script)
        queued.clear()
        with patch('body_probe.owner_full_frame', wraps=owner_full_frame) as build:
            queue_probe_frame(player, 'rifle-bot-volley', 20)
        bot, = build.call_args.args[1]
        from ow174.game.statescript import Asset
        self.assertEqual(bot.graph, Graph(0xBA3, 32, 13))
        self.assertEqual(bot.presence, {12: Asset(0x0DC0000000000005)})
        self.assertEqual(set(bot.active), {13})
        self.assertIs(queued[-1].stream, first.stream)
        self.assertGreater(queued[-1].chunk_last, first.chunk_last)
        player.match.bots[0].entity += 1
        queued.clear()
        queue_probe_frame(player, 'rifle-bot-graph', 30)
        self.assertEqual(queued[-1].entity, 0xB0000002)
        self.assertIsNot(queued[-1].stream, first.stream)

    def test_body_startup_does_not_reuse_manager_or_health_instance_ids(self):
        from body_probe import body_startup
        instances, variables = body_startup('rifle-body')
        instance, = instances
        self.assertNotIn(instance.index, (1, 2, 3))
        self.assertEqual(instance.graph, Graph(75, 39, 4))
        self.assertEqual(instance.presence, {2: Int(1)})
        self.assertEqual(variables, {338: Int(1650), 982: Int(1)})
        combined, combined_vars = body_startup('rifle-manager')
        self.assertEqual([item.index for item in combined], [21, 75])
        self.assertEqual(set(combined[0].active), {11, 23, 29, 32, 33})
        self.assertEqual((combined[0].active[11].count, combined[0].active[11].value), (1, 0))
        self.assertEqual((combined[0].active[23].count, combined[0].active[23].value), (2, 1))
        self.assertEqual(combined_vars[3772], Bool(True))

    def test_muzzle_entity_fixture_uses_transform_without_mover_and_fresh_ids(self):
        from ow174.game import world
        queued = []
        player = SimpleNamespace(body=0xA0000101, body_script=None,
            mover=SimpleNamespace(position=(1, 2, 3), yaw=0),
            client=SimpleNamespace(queue_entities=queued.extend))
        queue_probe_frame(player, 'rifle-muzzle-entity', 10)
        fixture, frame = queued
        first = player.weapon_entity
        self.assertEqual(fixture.entity, first)
        self.assertIsNone(fixture.movement)
        with patch.object(world, 'create') as create:
            fixture.build(0)
            create.assert_called_once_with(0, 0x04000000000004BF, {}, position=(1, 3.6, 3.5))
        queued.clear()
        player.body_script.arrived(frame.chunk_last)
        queue_probe_frame(player, 'rifle-muzzle-entity', 20)
        self.assertEqual((queued[0].entity, queued[0].op), (first, world.OP_DESTROY))
        self.assertGreater(player.weapon_entity, first)
        queued.clear()
        queue_probe_frame(player, 'rifle-input', 30)
        self.assertEqual(queued[0].op, world.OP_DESTROY)
        self.assertIsNone(player.weapon_entity)

    def test_weapon_presenter_and_stack_probes_keep_recovered_wire_shapes(self):
        ui = rifle_states('rifle-hud')
        self.assertEqual({9, 63, 65, 66} - ui.keys(), set())
        self.assertEqual((ui[63].count, ui[63].value), (1, 0))
        self.assertIsNone(ui[65])
        self.assertIsNone(ui[66])
        for phase, value in (('top', 1), ('under', 2), ('both', 3)):
            flags = rifle_states('rifle-stack-' + phase)[29]
            self.assertEqual((flags.count, flags.value), (2, value))
        effect = rifle_states('rifle-effect')
        self.assertEqual((effect[21].count, effect[21].value), (3, 1))
        self.assertIsNone(effect[57])
        client = rifle_states('rifle-client')
        self.assertEqual(client.keys() - effect.keys(), {79})
        self.assertIsNone(client[79])
        self.assertEqual(frame_for_phase('rifle-client', 9).count,
                         frame_for_phase('rifle-effect', 9).count)
        dataflow = rifle_states('rifle-dataflow')
        self.assertEqual(dataflow.keys() - client.keys(), {6, 17, 18, 22, 33, 34})
        self.assertTrue(all(dataflow[index] is None for index in dataflow.keys() - client.keys()))
        self.assertEqual(frame_for_phase('rifle-dataflow', 9).count,
                         frame_for_phase('rifle-client', 9).count)
        entry = rifle_states('rifle-entry')
        self.assertEqual((entry[60].count, entry[60].value), (2, 1))
        self.assertEqual(frame_for_phase('rifle-entry', 9).count,
                         frame_for_phase('rifle-dataflow', 9).count + 2)

    def test_hud_reset_removes_the_existing_child_before_recreation(self):
        player = SimpleNamespace(entity=0xA0000100, body=0xA0000101)
        self.assert_same_frame(hud_frame('hud-reset', 100, player), owner_full_frame(100,
            [Instance(1, __import__('body_probe').CONTROLLER, presence={5: Bool(True)})],
            {2438: Bool(False), 31435: Bool(False)}))

    def assert_same_frame(self, actual, expected):
        self.assertEqual((actual.value, actual.count), (expected.value, expected.count))

    def test_each_phase_builds_an_isolated_known_frame(self):
        graph = Graph(596, 81, 23)
        expected = {
            "empty": owner_full_frame(9, [], {}),
            "graph": owner_full_frame(9, [Instance(1, graph)], {}),
            "int": owner_full_frame(9, [Instance(1, graph, presence={4: Int(1)})], {}),
            "draw": owner_full_frame(9, [Instance(1, graph, presence={4: Int(1)}, active={11: None})],
                {31: Int(1), 34: Bool(False)}),
            'float': owner_full_frame(9, [Instance(1, graph, presence={4: Int(1), 15: Float(20)}, active={11: None})],
                {31: Int(1), 34: Bool(False)}),
            'root': owner_full_frame(9, [Instance(1, graph, presence={4: Int(1)}, active={11: None, 0: None, 12: None})],
                {31: Int(1), 34: Bool(False)}),
            'stack': owner_full_frame(9, [Instance(1, graph, presence={4: Int(1)}, active={11: None, 60: None})],
                {31: Int(1), 34: Bool(False)}),
        }
        self.assertEqual(set(PHASES), set(expected) | {'sequence', 'control', 'full', 'root-only', 'root-bool', 'ready', 'combined', 'draw-zero', 'draw-one', 'hud-reset', 'body-roots', 'manager-root', 'rifle-input', 'rifle-hud', 'rifle-stack-top', 'rifle-stack-under', 'rifle-stack-both',
            'rifle-effect', 'rifle-client', 'rifle-dataflow', 'rifle-entry', 'rifle-body', 'rifle-manager', 'rifle-binding', 'rifle-feedback', 'rifle-reload', 'rifle-root-body', 'rifle-root-flags', 'rifle-root-hud', 'rifle-hud-mode', 'rifle-shared-hud', 'rifle-projectile', 'rifle-bot-graph', 'rifle-bot-volley', 'rifle-muzzle-body', 'rifle-muzzle-entity', 'rifle-hud-body', 'rifle-attack', 'ammo', 'volley', 'volley2', 'volley-trailer1', 'hud-bool', 'hud-seeded', 'hud-entity', 'hud-local',
            'hud-gate-bool', 'hud-gate-entity', 'hud-mode', 'hud-native', 'hud-bootstrap', 'hud-bootstrap-entity', 'hud-bootstrap-ref', 'hud-root', 'hud-root-body', 'rifle-bot-feedback', 'rifle-bot-remote', 'rifle-bot-remote-init', 'rifle-shared-parent'})
        self.assertEqual(frame_for_phase('root-bool', 9).count, frame_for_phase('root-only', 9).count + 2)
        self.assertEqual(frame_for_phase('ready', 9).count, frame_for_phase('root-bool', 9).count + 1)
        self.assertEqual(frame_for_phase('combined', 9).count, frame_for_phase('root-bool', 9).count)
        for phase in ('root-only', 'draw-zero', 'draw-one'):
            self.assertGreater(frame_for_phase(phase, 9).count, 81)
        self.assertGreater(frame_for_phase('full', 9).count, frame_for_phase('draw', 9).count)
        self.assertEqual(frame_for_phase('manager-root', 9).count,
                         frame_for_phase('body-roots', 9).count + 1)  # BooleanSwitch branch payload
        self.assertEqual(frame_for_phase('rifle-input', 9).count,
                         soldier.body_frame(9).count + 9)  # four branch bits, Begin and counter4
        for phase, frame in expected.items():
            with self.subTest(phase=phase):
                self.assert_same_frame(frame_for_phase(phase, 9), frame)
        with self.assertRaises(ValueError):
            frame_for_phase("sequence", 9)

    def test_volley_payload_follows_native_reader_order(self):
        from ow174.game.bits import BitReader
        payload = volley_payload(1)
        reader = BitReader(payload.getvalue())
        self.assertEqual(reader.bits(1), 1)
        self.assertEqual(reader.bits(10), 0)
        self.assertEqual(reader.bits(1), 1)
        self.assertEqual(reader.bits(10), 0)
        self.assertEqual(reader.bits(1), 0)
        self.assertEqual(reader.bits(4), 1)
        self.assertEqual(reader.bits(1), 0)
        self.assertEqual(reader.bits(6), 0)
        for count in (0, -1, 65536):
            with self.assertRaises(ValueError):
                volley_payload(count)

    def test_hud_probe_uses_controller_stream(self):
        from ow174.game.statescript import Stream
        queued = []
        player = SimpleNamespace(entity=0xA0000100, body=0xA0000101,
            script=Stream(0xA0000100), body_script=None,
            client=SimpleNamespace(queue_entities=queued.extend))
        update = queue_probe_frame(player, 'hud-seeded', 19)
        self.assertIs(update.stream, player.script)
        self.assertEqual(update.stream.entity, player.entity)
        self.assertIsNone(player.body_script)

    def test_hud_mode_probe_updates_mode_and_controller(self):
        from ow174.game.statescript import Stream
        queued = []
        player = SimpleNamespace(entity=0xA0000100, body=0xA0000101,
            script=Stream(0xA0000100), mode_script=Stream(0xD0000010),
            client=SimpleNamespace(queue_entities=queued.extend))
        queue_probe_frame(player, 'hud-mode', 20)
        self.assertEqual([item.entity for item in queued], [player.entity, 0xD0000010])
        self.assertIs(queued[1].stream, player.mode_script)

    def test_native_hud_uses_only_recovered_mode_gate_and_no_guessed_bindings(self):
        from ow174.game.statescript import CONTROLLER, HUD, Stream, subscript
        queued = []
        player = SimpleNamespace(entity=0xA0000100, body=0xA0000101,
            script=Stream(0xA0000100), mode_script=Stream(0xD0000010),
            client=SimpleNamespace(queue_entities=queued.extend))
        self.assert_same_frame(hud_frame('hud-native', 100, player), owner_full_frame(100, [
            Instance(1, CONTROLLER, presence={5: Bool(True)}, active={36: subscript(4)}),
            Instance(4, HUD, parent=(1, 42), active={0: None})],
            {2438: Bool(False), 31435: Bool(False)}))
        queue_probe_frame(player, 'hud-native', 20)
        self.assertEqual([item.entity for item in queued], [0xD0000010, player.entity])

    def test_mode_bootstrap_keeps_catalog_parent_state_and_child_graph(self):
        from ow174.game.statescript import Entity, ScriptRef, Stream, subscript
        for phase, parent in (('hud-bootstrap', Bool(True)),
                ('hud-bootstrap-entity', Entity(0xD0000010)), ('hud-bootstrap-ref', ScriptRef(2, 5))):
            with self.subTest(phase=phase):
                queued = []
                player = SimpleNamespace(entity=0xA0000100, body=0xA0000101,
                    script=Stream(0xA0000100), mode_script=Stream(0xD0000010),
                    client=SimpleNamespace(queue_entities=queued.extend))
                with patch.object(player.mode_script, 'full_frame', wraps=player.mode_script.full_frame) as frame:
                    queue_probe_frame(player, phase, 20)
                self.assert_same_frame(frame.call_args.args[0], owner_full_frame(20, [
                    Instance(1, Graph(5056, 20, 3), active={19: subscript(2)}),
                    Instance(2, Graph(3254, 1, 0), parent=(1, 21))], {23604: parent}))
                self.assertEqual([item.entity for item in queued], [0xD0000010, player.entity])

    def test_verified_root_precedes_hud_without_local_presenter_bindings(self):
        from ow174.game.statescript import ScriptRef, Stream
        queued = []
        player = SimpleNamespace(entity=0xA0000100, body=0xA0000101,
            script=Stream(0xA0000100), mode_script=Stream(0xD0000010),
            client=SimpleNamespace(queue_entities=queued.extend))
        with patch.object(player.mode_script, 'full_frame', wraps=player.mode_script.full_frame) as frame:
            queue_probe_frame(player, 'hud-root', 20)
        self.assert_same_frame(frame.call_args.args[0], owner_full_frame(20, [
            Instance(3, Graph(4429, 1, 0), active={0: None})], {23604: ScriptRef(3, 0)}))
        self.assertEqual([item.entity for item in queued], [0xD0000010, player.entity])
        self.assert_same_frame(hud_frame('hud-root', 100, player), hud_frame('hud-native', 100, player))
        with patch.object(player.mode_script, 'full_frame', wraps=player.mode_script.full_frame) as frame:
            queue_probe_frame(player, 'hud-root-body', 21)
        self.assert_same_frame(frame.call_args.args[0], owner_full_frame(21, [
            Instance(3, Graph(4429, 1, 0), active={0: None}, source_entity=player.body)],
            {23604: ScriptRef(3, 0)}))


class BodyProbeScheduleTests(unittest.TestCase):
    def test_sequence_waits_for_ack_and_five_seconds_between_frames(self):
        soldier_a = SimpleNamespace(hero=SimpleNamespace(guid=practice.SOLDIER),
            has_body=True, spawned=True, client=object(), body=0xA0000101, body_script=None)
        soldier_b = SimpleNamespace(hero=SimpleNamespace(guid=practice.SOLDIER),
            has_body=True, spawned=True, client=object(), body=0xA0000201, body_script=None)
        loading = SimpleNamespace(hero=SimpleNamespace(guid=practice.SOLDIER),
            has_body=True, spawned=False, client=object(), body=0xA0000301)
        other_hero = SimpleNamespace(hero=SimpleNamespace(guid=0),
            has_body=True, spawned=True, client=object(), body=0xA0000401)
        match = SimpleNamespace(players=[soldier_a, soldier_b, soldier_a, loading, other_hero])
        schedule = ProbeSchedule("sequence")

        self.assertEqual(schedule.due([match], 100.0), [])
        self.assertEqual(schedule.due([match], 101.99), [])
        first = [(soldier_a, "empty"), (soldier_b, "empty")]
        self.assertEqual(schedule.due([match], 102.0), first)
        for player, phase in first:
            player.body_script = SimpleNamespace(entity=player.body, data_in_flight=True)
            schedule.queued(player, phase, 102.0)
        self.assertEqual(schedule.due([match], 102.0), [])
        self.assertEqual(schedule.due([match], 106.99), [])
        self.assertEqual(schedule.due([match], 107.0), [])  # still in flight
        for player, _ in first:
            player.body_script.data_in_flight = False
        second = [(soldier_a, "graph"), (soldier_b, "graph")]
        self.assertEqual(schedule.due([match], 107.0), second)
        for player, phase in second:
            player.body_script.data_in_flight = True
            schedule.queued(player, phase, 107.0)
        self.assertEqual(schedule.due([match], 112.0), [])
        for player, _ in second:
            player.body_script.data_in_flight = False
        third = [(soldier_a, "int"), (soldier_b, "int")]
        self.assertEqual(schedule.due([match], 112.0), third)
        for player, phase in third:
            player.body_script.data_in_flight = True
            schedule.queued(player, phase, 112.0)
        self.assertEqual(schedule.due([match], 117.0), [])
        for player, _ in third:
            player.body_script.data_in_flight = False
        fourth = [(soldier_a, "draw"), (soldier_b, "draw")]
        self.assertEqual(schedule.due([match], 117.0), fourth)
        for player, phase in fourth:
            player.body_script.data_in_flight = True
            schedule.queued(player, phase, 117.0)
        for player, _ in fourth:
            player.body_script.data_in_flight = False
        fifth = [(soldier_a, "float"), (soldier_b, "float")]
        self.assertEqual(schedule.due([match], 122.0), fifth)
        for player, phase in fifth:
            player.body_script.data_in_flight = True
            schedule.queued(player, phase, 122.0)
        for player, _ in fifth:
            player.body_script.data_in_flight = False
        for moment, name in ((127.0, 'root'), (132.0, 'stack'), (137.0, 'full')):
            due = [(soldier_a, name), (soldier_b, name)]
            self.assertEqual(schedule.due([match], moment), due)
            for player, phase in due:
                schedule.queued(player, phase, moment)
        self.assertEqual(schedule.due([match], 142.0), [])

    def test_single_phase_sends_once_and_new_body_gets_a_fresh_delay(self):
        player = SimpleNamespace(hero=SimpleNamespace(guid=practice.SOLDIER),
            has_body=True, spawned=True, client=object(), body=0xA0000101, body_script=None)
        match = SimpleNamespace(players=[player])
        schedule = ProbeSchedule("int")
        self.assertEqual(schedule.due([match], 10.0), [])
        due = [(player, "int")]
        self.assertEqual(schedule.due([match], 12.0), due)
        schedule.queued(player, "int", 12.0)
        self.assertEqual(schedule.due([match], 20.0), [])

        player.body = 0xA0000102
        player.body_script = SimpleNamespace(entity=0xA0000101, data_in_flight=False)
        self.assertEqual(schedule.due([match], 30.0), [])
        self.assertEqual(schedule.due([match], 31.99), [])
        self.assertEqual(schedule.due([match], 32.0), due)

    def test_sequence_reuses_body_stream_with_increasing_cmfd(self):
        queued = []
        player = SimpleNamespace(body=0xA0000101, body_script=None,
            client=SimpleNamespace(queue_entities=queued.extend))
        first = queue_probe_frame(player, "empty", 10)
        stream = player.body_script
        stream.arrived(first.chunk_last)
        second = queue_probe_frame(player, "graph", 20)
        self.assertIs(player.body_script, stream)
        self.assertEqual((first.stream.entity, second.stream.entity), (player.body, player.body))
        self.assertGreater(stream.cmfd, 10)
        self.assertEqual(stream.cmfd, 20)
        self.assertEqual([update.chunk_last for update in queued], [1, 2])
        queue_probe_frame(player, 'rifle-input', 21)
        self.assertIn(8, player.weapon_extra_states)
        queue_probe_frame(player, 'rifle-muzzle-body', 22)
        self.assertEqual(player.weapon_entity, player.body)
        queue_probe_frame(player, 'ammo', 22)
        self.assertEqual(player.weapon_extra_states, {})
        self.assertIsNone(player.weapon_entity)

        player.body = 0xA0000102
        third = queue_probe_frame(player, "empty", 21)
        self.assertIsNot(player.body_script, stream)
        self.assertEqual(player.body_script.entity, player.body)
        self.assertEqual(third.chunk_last, 1)


class BodyProbeServerTests(unittest.TestCase):
    def control_server(self):
        server = object.__new__(BodyProbeServer)
        server.phase = 'control'
        server.matches = []
        server._probe_schedule = ProbeSchedule('draw')
        server._control_phase = None
        server._control_valid = False
        server._next_control_read = 0
        return server

    def test_control_accepts_only_known_concrete_phases(self):
        import body_probe
        server = self.control_server()
        with patch.object(practice.GameServer, '_tick'), \
                patch.object(body_probe, 'CONTROL_FILE') as control:
            control.open = mock_open(read_data='ammo\n')
            server._tick(100.0)
            self.assertEqual(server._probe_schedule.phase, 'ammo')
            self.assertTrue(server._control_valid)
            control.open = mock_open(read_data='control\n')
            server._tick(101.0)
            self.assertFalse(server._control_valid)
            server._probe_schedule.due = Mock()
            server._tick(101.1)
            server._probe_schedule.due.assert_not_called()

    def test_attacking_bot_requires_explicit_diagnostic_phase(self):
        import body_probe
        server = self.control_server()
        match = SimpleNamespace(bot_attacks_enabled=False, players=[])
        server.matches = [match]
        observed = []
        with patch.object(practice.GameServer, '_tick', side_effect=lambda now:
                observed.append(match.bot_attacks_enabled)), patch.object(body_probe, 'CONTROL_FILE') as control:
            control.open = mock_open(read_data='rifle-attack\n')
            server._tick(100)
            self.assertTrue(match.bot_attacks_enabled)
            control.open = mock_open(read_data='rifle-input\n')
            server._tick(101)
            self.assertFalse(match.bot_attacks_enabled)
            match.bot_attacks_enabled = True
            control.open = mock_open(read_data='invalid\n')
            server._tick(102)
            self.assertFalse(match.bot_attacks_enabled)
        self.assertEqual(observed, [True, False, False])

    def test_pending_data_prevents_control_frame_overtaking(self):
        server = self.control_server()
        server._control_valid = True
        server._next_control_read = 200
        player = SimpleNamespace(body_script=SimpleNamespace(data_in_flight=True))
        server._probe_schedule = Mock()
        server._probe_schedule.due.return_value = [(player, 'ammo')]
        with patch.object(practice.GameServer, '_tick'):
            server._tick(100)
        server._probe_schedule.queued.assert_not_called()

    def test_combined_hud_probe_waits_for_controller_and_mode_acknowledgments(self):
        for controller_pending, mode_pending in ((True, False), (False, True)):
            server = self.control_server()
            server._control_valid = True
            server._next_control_read = 200
            player = SimpleNamespace(body_script=SimpleNamespace(data_in_flight=False),
                script=SimpleNamespace(data_in_flight=controller_pending),
                mode_script=SimpleNamespace(data_in_flight=mode_pending))
            server._probe_schedule = Mock()
            server._probe_schedule.due.return_value = [(player, 'rifle-hud-mode')]
            with patch.object(practice.GameServer, '_tick'):
                server._tick(100)
            server._probe_schedule.queued.assert_not_called()

    def test_launcher_owns_the_helper_server_command(self):
        import run

        root = Path("unused-test-root")
        with patch.object(run, "launch", return_value=23) as launch_owned:
            self.assertEqual(launch_probe(root, "sequence", 180), 23)
        args, kwargs = launch_owned.call_args
        self.assertEqual(args, (root, 180))
        self.assertEqual(kwargs["server_command"], [
            sys.executable, "-u", str((Path(__file__).resolve().parent / "body_probe.py").resolve()),
            "--serve", "--phase", "sequence",
        ])

    def test_serve_uses_the_owned_tcp_lobby_entrypoint(self):
        import lobby

        original_server = practice.GameServer
        original_phase = BodyProbeServer.phase
        original_login = lobby.login_records
        try:
            with patch.object(lobby, "serve", return_value=None) as serve_lobby:
                self.assertIsNone(serve("int"))
            self.assertIs(practice.GameServer, BodyProbeServer)
            self.assertEqual(BodyProbeServer.phase, "int")
            self.assertIs(lobby.login_records, original_login)  # the lobby's preload covers every Range bot
            serve_lobby.assert_called_once_with()
        finally:
            practice.GameServer = original_server
            BodyProbeServer.phase = original_phase
            lobby.login_records = original_login


if __name__ == "__main__":
    unittest.main()
