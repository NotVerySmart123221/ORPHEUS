import unittest

import practice
from ow174.game.bits import BitReader
from ow174.game.statescript import Stream, Bool, Entity
from target import TrainingTarget
from test_target import _match
import nameplates


class NameplateTests(unittest.TestCase):
    def test_create_uses_instance_visibility_and_authored_target_body_override(self):
        bot = TrainingTarget((0, 0, 8), 180)
        instance, variables = nameplates.setup(bot)
        self.assertEqual(instance.graph.index, 0xD24)
        self.assertEqual(instance.source_entity, bot.entity)
        self.assertEqual(instance.presence, {0: Bool(True), 3: Bool(True)})
        self.assertEqual(variables[29415], Entity(bot.entity))
        self.assertNotEqual(variables[29415], Entity(bot.controller))
        self.assertNotIn(8807, variables)  # entity scope does not make the label visible

    def test_show_activates_unfiltered_client_state_without_overriding_its_native_binding(self):
        bot = TrainingTarget((0, 0, 8), 180)
        frame = nameplates.show(100)
        r = BitReader(frame.getvalue())
        self.assertEqual(r.bits(3), 2)  # lead=0, H=1, C=0
        self.assertEqual(r.var_b(), 100)
        body_bits = r.var_b()
        start = r.pos
        self.assertEqual(r.bits(5), 2)  # instance 1
        self.assertEqual(r.bits(2), 0)  # keep descriptor
        self.assertEqual(r.bits(5), 0)  # end instance list
        self.assertEqual(r.bit(), 0)  # no entity variables
        self.assertEqual(r.bits(4), 0)  # unchanged 32 sync slots
        self.assertEqual(r.bit(), 0)  # no extra variable map; Stack 71 reads body override 29415
        self.assertEqual(r.bits(10), 1 << 4)  # state 35 belongs to group 4 of 77 slots
        self.assertEqual(r.bits(8), 1 << 3)
        self.assertEqual(r.bit(), 1)
        self.assertEqual(r.bit(), 0)  # no instance events
        self.assertEqual(r.pos - start, body_bits)
        self.assertEqual(r.bits(3), 0)  # more, frame events

    def test_reliable_delta_waits_for_create_ack_and_revival_reuses_stream(self):
        match, player = _match()
        bot = TrainingTarget((0, 0, 8), 180)
        match.bots = [bot]
        updates = match._target_updates(player)
        stream, creation, stage = player.bot_nameplates[bot.controller]
        self.assertEqual(creation.entity, bot.entity)
        self.assertEqual(stage, 0)
        self.assertEqual(len(updates), 2)  # identity and body, no UI script in the same packet
        self.assertEqual(match._nameplate_updates(player), [])
        creation.delivered = True
        created, = match._nameplate_updates(player)
        self.assertTrue(created.resend)
        self.assertEqual(match._nameplate_updates(player), [])
        stream.arrived(stream.data_last)
        shown, = match._nameplate_updates(player)
        self.assertTrue(shown.resend)
        self.assertEqual(stream.data_last, 2)
        self.assertEqual(match._nameplate_updates(player), [])
        stream.arrived(stream.data_last)
        bot.damage(200, 100)
        hidden, = match._nameplate_updates(player)
        self.assertTrue(hidden.resend)
        self.assertEqual(bot.tick(103), ('revive', bot.entity))
        match._revive_bot(bot)
        self.assertIs(player.bot_nameplates[bot.controller][0], stream)
        creation = player.bot_nameplates[bot.controller][1]
        self.assertEqual(creation.entity, bot.entity)
        self.assertEqual(match._nameplate_updates(player), [])
        creation.delivered = True
        self.assertEqual(len(match._nameplate_updates(player)), 1)
        stream.arrived(stream.data_last)
        self.assertEqual(len(match._nameplate_updates(player)), 1)

    def test_entity_delivery_is_set_only_by_a_positive_datagram_ack(self):
        from ow174.game.server import Client
        from ow174.game.world import EntityUpdate, OP_CREATE
        _, player = _match()
        client = Client(None, ('127.0.0.1', 0), 1, None, player)
        creation = EntityUpdate(0xB0000001, OP_CREATE)
        self.assertFalse(creation.delivered)
        client.in_flight[1] = [], [creation]
        client.acked(1, 0)
        self.assertFalse(creation.delivered)
        client.acked(2, 1)
        self.assertTrue(creation.delivered)
        lost = EntityUpdate(0xB0000101, OP_CREATE)
        client.in_flight[3] = [], [lost]
        client.acked(4, 0)
        self.assertFalse(lost.delivered)
        self.assertIn(lost, client.entities)

    def test_delta_is_retransmitted_and_advances_data_ack(self):
        stream = Stream(0xB0000000)
        stream.full_frame(nameplates.create(TrainingTarget((0, 0, 8), 0)))
        stream.arrived(1)
        update = stream.delta_frame(nameplates.show(100))
        self.assertTrue(update.resend)
        self.assertTrue(stream.data_in_flight)
        self.assertEqual(update.chunk_last, stream.data_last)
        stream.arrived(update.chunk_last)
        self.assertFalse(stream.data_in_flight)
