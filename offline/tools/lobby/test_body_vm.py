"""body_vm: a Soldier: 76 body run by the vendored VM inside the match. Its frames are read by the client's rules
(test_vm_driver.ClientModel), and its shots, melee, ultimate, Sprint, health and charge reach our server's code."""
import os
import unittest
from unittest import mock

import practice  # noqa: F401  installs the pinned vendor path
from ow174.game.commands import Command
from ow174.game.content import PRACTICE_RANGE, SOLDIER
from ow174.game.mover import ADD, SPEED
from ow174.game.script import expr
from ow174.game.script.driver import BodyStream, HUD_HEALTH, ULT_CHARGE

import body_vm
from target import MELEE
from test_soldier import _Client
from test_vm_driver import ClientModel, server_on

FIRE, HELIX, SPRINT, FIELD, VISOR, MELEE_BUTTON = 0x01, 0x02, 0x08, 0x10, 0x20, 0x800


class BodyVMTests(unittest.TestCase):
    def setUp(self):
        patcher = mock.patch.dict(os.environ, {'OW_BODY_VM': '1'})
        patcher.start()
        self.addCleanup(patcher.stop)
        game = practice.PracticeRange(port=0).game
        game.create_match(PRACTICE_RANGE, [(1, 'Player', SOLDIER, 0, True)])
        self.match = match = game.matches[0]
        self.player = player = match.players[0]
        player.client = _Client()
        player.spawned = player.has_body = True
        match.own_body_updates(player)  # the body's create starts its VM
        player.body_create.delivered = True
        player.body_create.stream.arrived(0)  # the transport's ack of the create
        self.client = ClientModel()
        self.frame, self.now = 1000, 3.0

    def tick(self, buttons=0, frames=1, forward=0):
        """Commands for `frames` frames, a match tick, and the VM's frames read and acknowledged by the client."""
        player = self.player
        commands = [Command(self.frame + 1 + n, forward=forward, buttons=buttons) for n in range(frames)]
        self.frame += frames
        self.now += frames * 0.007
        player.take_commands(commands)
        self.match.update(self.now, self.frame)
        sent = [update for batch in player.client.updates for update in batch if isinstance(update.stream, BodyStream)]
        player.client.updates.clear()
        for update in sent:
            self.client.apply(update)  # raises decode.ClientCrash where the client would crash
            update.stream.arrived(update.chunk_last)
        return sent

    def test_off_unless_asked(self):
        with mock.patch.dict(os.environ, {'OW_BODY_VM': ''}):
            self.assertFalse(body_vm.wanted(SOLDIER))
        self.assertTrue(body_vm.wanted(SOLDIER))
        self.assertFalse(body_vm.wanted(0x02E0000000000003))  # Tracer: only with OW_BODY_VM=all
        with mock.patch.dict(os.environ, {'OW_BODY_VM': 'all'}):
            self.assertTrue(body_vm.wanted(0x02E0000000000003))

    def test_the_client_reads_every_frame_and_ends_with_the_servers_states(self):
        self.assertIsNotNone(self.player.body_vm)
        [full] = self.tick()
        self.assertEqual(full.chunk_last, 1)
        sent = []
        for buttons, frames in ((0, 30), (FIRE, 140), (0, 60), (HELIX, 3), (0, 100)):
            sent += self.tick(buttons, frames)
        self.assertGreater(len(sent), 3)
        script = self.player.body_vm.script
        self.assertEqual(self.client.on(), server_on(script))
        self.assertIsNone(self.player.body_script)  # kits.py's stream never started

    def test_shots_melee_and_field_reach_our_server(self):
        self.tick()
        self.tick(0, 30)
        with mock.patch.object(self.match, 'fire_at_target') as rifle, \
                mock.patch.object(self.match, 'fire_helix') as helix, \
                mock.patch.object(self.match, 'throw_field') as field:
            self.tick(FIRE, 140)  # about 1 s of held fire: 9 shots a second
            self.assertEqual(rifle.call_count, 9)
            self.tick(0, 100)
            self.tick(HELIX, 3)
            self.tick(0, 100)
            self.assertEqual(helix.call_count, 1)
            self.tick(FIELD, 3)
            self.tick(0, 300)
            self.assertEqual(field.call_count, 1)
            rifle.reset_mock()
            self.tick(MELEE_BUTTON, 3)
            self.tick(0, 100)
            self.assertEqual([call.args[2:] for call in rifle.call_args_list], [(MELEE,)])

    def test_sprint_comes_from_the_vm(self):
        self.tick()
        self.tick(SPRINT, 60, forward=127)
        mods = self.player.mover.script_mods
        self.assertIn((0, ADD, 0.5, 0.0), mods.values())  # Sprint's SPEED ADD var1002
        self.assertEqual(SPEED, 0)
        self.tick(0, 100)  # stopped: Sprint ends
        self.assertNotIn((0, ADD, 0.5, 0.0), self.player.mover.script_mods.values())

    def test_health_and_ultimate_charge_go_into_the_vm(self):
        self.tick()
        player, script = self.player, self.player.body_vm.script
        self.match._damage_player(player, 50, self.now)
        self.assertEqual(expr.to_float(script.component.entity_var(HUD_HEALTH).value()), player.health)
        cost = expr.to_float(script.component.entity_var(body_vm.ULT_COST).value())
        self.match._charge_ult(player, player.ult_most)
        self.assertEqual(expr.to_float(script.component.entity_var(ULT_CHARGE).value()), cost)
        self.tick(0, 5)
        self.tick(VISOR, 3)
        self.assertEqual(player.ult_charge, 0)
        self.assertGreater(player.visor_until, self.now)  # the server aims Tactical Visor's shots
        self.tick(0, 300)
        self.match.update(player.visor_until + 1, self.frame)  # the visor's end needs no kit
        self.assertEqual(player.visor_until, 0.0)

    def test_a_death_pulses_the_kill_feed_line(self):
        self.tick()
        self.tick(0, 30)
        script = self.player.body_vm.script
        feed = next(i for i in script.component.instances.values() if i.graph.index == 0x01CF).state(108)
        self.assertEqual(feed.payload()['count'], 0)
        self.match._damage_player(self.player, 999, self.now, by=0xB0000201)  # 003E.025 to 01CF
        self.assertTrue(script.component.owner_dead)
        self.tick(0, 30)
        self.assertGreaterEqual(feed.payload()['count'], 1)  # st108, the kill feed line
        self.assertEqual(self.client.on(), server_on(script))  # the client read the frames that carry it
        line = next(number for number, index in self.client.graphs.items() if index == 0x01CF)
        self.assertEqual(self.client.vars[(line, 392)], expr.Entity(0xB0000201))  # the killer the line names

    def test_a_failing_vm_hands_the_body_to_kits(self):
        self.tick()
        with mock.patch.object(self.player.body_vm.script, 'command', side_effect=RuntimeError('boom')), \
                self.assertLogs('ow174.game', 'ERROR'):
            self.player.take_commands([Command(self.frame + 1)])
        self.assertIsNone(self.player.body_vm)
        self.assertIsNone(self.player.mover.script_mods)
        self.player.body_picked_at = 0
        self.match.update(self.now + 1, self.frame + 1)
        self.assertIsNotNone(self.player.body_script)  # kits.py's frames from here on

    def test_a_new_body_gets_a_new_vm(self):
        self.tick()
        old = self.player.body_vm
        self.match.switch_hero(self.player, self.player.hero)
        self.assertTrue(old.script.retired)
        self.assertIsNot(self.player.body_vm, old)
        self.assertEqual(self.player.body_vm.script.entity, self.player.body)


class OtherHeroTests(unittest.TestCase):
    """OW_BODY_VM=all: any hero's Volleys reach the server by the button that fires each (body_vm.VOLLEYS)."""

    def body(self, hero):
        patcher = mock.patch.dict(os.environ, {'OW_BODY_VM': 'all'})
        patcher.start()
        self.addCleanup(patcher.stop)
        game = practice.PracticeRange(port=0).game
        game.create_match(PRACTICE_RANGE, [(1, 'Player', hero, 0, True)])
        self.match = game.matches[0]
        self.player = player = self.match.players[0]
        player.client = _Client()
        player.spawned = player.has_body = True
        self.match.own_body_updates(player)
        player.body_create.delivered = True
        player.body_create.stream.arrived(0)
        self.client, self.frame, self.now = ClientModel(hero), 1000, 3.0

    tick = BodyVMTests.tick

    def test_every_hero_with_a_volley_has_its_primary_played(self):
        from ow174.game.content import heroes
        names = {hero.name for hero in heroes().values()}
        primaries = {key for key, button in body_vm.VOLLEYS.items() if button == body_vm.FIRE}
        self.assertEqual(len(body_vm.VOLLEYS), 56)
        self.assertTrue(all(body_vm.effect(name, key) == 'primary' for name in names for key in primaries))
        self.assertIsNone(body_vm.effect('Zarya', (0x025F, 0)))  # Graviton Surge: nothing on the server yet

    def test_tracers_pistols_and_pharahs_concussive_blast_reach_the_server(self):
        self.body(0x02E0000000000003)  # Tracer
        self.tick()
        self.tick(0, 30)
        with mock.patch.object(self.match, 'fire_at_target') as shots:
            self.tick(FIRE, 60)
            self.assertGreater(shots.call_count, 5)  # each Volley shot, at most one a command frame
            self.assertLessEqual(shots.call_count, 60)
        self.body(0x02E0000000000008)  # Pharah
        self.tick()
        self.tick(0, 30)
        with mock.patch.object(self.match, 'fire_ability') as casts, mock.patch.object(self.match, 'fire_at_target') as shots:
            self.tick(FIELD, 3)  # ability 2: Concussive Blast, a target.AbilityHit
            self.tick(0, 200)
            self.assertEqual((casts.call_count, shots.call_count), (1, 0))
            self.assertIs(casts.call_args.args[2], body_vm.ABILITY_HITS[('Pharah', FIELD)])


if __name__ == '__main__':
    unittest.main()
