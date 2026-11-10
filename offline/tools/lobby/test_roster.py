import dataclasses
import math
import unittest

import practice
from assault import Assault
from ow174.game.bits import BitReader
from ow174.game.commands import SECONDARY, Command
from ow174.game.content import PRACTICE_RANGE, heroes
from ow174.game.match import ASSAULT, Match
from soldier import MELEE_BUTTON, RELOAD
from target import CALL_MECH_CAST, INFINITE
from test_soldier import _Client
from unittest.mock import patch


def equipped(hero):
    match = Match(PRACTICE_RANGE, gameplay=True, body_setup=practice.GameServer.body_setup)
    player = match.add_player(1, 'Player', hero, 0, True)
    player.client = _Client()
    player.spawned = player.has_body = True
    player.body_picked_at = 1
    match.update(3.0, 1)
    player.body_script.arrived(player.body_script.data_last)
    return match, player


class RosterTests(unittest.TestCase):
    def test_matrix_blocks_cannons_on_activation_and_restores_them_on_release_or_depletion(self):
        from kits import DVA, Held
        from ow174.game.commands import FIRE
        for ending in ('release', 'depletion'):
            with self.subTest(ending=ending):
                match, player = equipped(DVA)
                matrix = next(c for c in player.cooldowns if isinstance(c, Held))
                player.take_commands([Command(10, buttons=FIRE)])
                self.assertEqual(player.rifle.serial, 1)
                frame = 10 + player.rifle.interval
                player.take_commands([Command(frame, buttons=FIRE | SECONDARY)])
                self.assertTrue(matrix.active)
                self.assertEqual(player.rifle.serial, 1)
                self.assertFalse(player.rifle.firing)
                if ending == 'release':
                    player.take_commands([Command(frame + 1, buttons=FIRE)])
                else:
                    player.take_commands([Command(frame + 300, buttons=FIRE | SECONDARY)])
                self.assertFalse(matrix.active)
                self.assertEqual(player.rifle.serial, 2)

    def test_boosters_hit_an_enemy_bot_once_and_knock_it_along_the_flight(self):
        from kits import DVA
        from ow174.game.content import SOLDIER
        from target import BOOST_DAMAGE, TrainingTarget
        match, player = equipped(DVA)
        player.mover.position, player.select_open, match.mesh = [0.0, 0.0, 0.0], False, None
        bot = TrainingTarget((0, 0, 1.0), 180, team=1 - player.team, hero=SOLDIER, entity=0xB0000201)
        match.bots = [bot]
        player.bots_seen.add(bot.entity)
        player.take_commands([Command(10, buttons=0x08)])  # Shift: Boosters, flying along yaw 0 (+z)
        player.take_commands([Command(11), Command(12)])
        self.assertEqual(bot.health, bot.max_health - BOOST_DAMAGE)  # once per flight
        match.update(match.now + 1.0, 2)
        self.assertGreater(bot.position[2], 1.0 + 4.0)
        self.assertIsNone(bot.knocked)

    def test_dva_ejects_at_one_health_and_her_pilot_respawns_in_a_mech(self):
        from kits import DVA
        from ow174.game.content import PILOT_BODY, SPAWN_BODY
        from soldier import DVA_EJECT_VOICE
        match, player = equipped(DVA)
        match.bots, player.select_open, match.mesh = [], False, None
        player.mover.position, player.mover.yaw = [5.0, 0.0, 7.0], 0  # facing +z
        mech = player.body
        match._damage_player(player, 1000, 4.0)
        self.assertEqual((player.health, player.form_change), (1, 'eject'))
        match.update(4.0, 2)
        self.assertEqual((player.hero.body, player.health), (PILOT_BODY[DVA], 150))
        self.assertEqual([round(v, 6) for v in player.mover.position], [5.0, 0.0, 6.0])  # EJECT_BACK behind
        self.assertEqual(player.mover.velocity, (0.0, 5.0, -6.0))  # thrown back and up (target.EJECT_LAUNCH)
        self.assertNotEqual(player.body, mech)
        self.assertEqual(player.pending_spawn_voice['+0x90'] & 0xFFFF, DVA_EJECT_VOICE[1])
        match._damage_player(player, 100, 5.0)  # the Eject! cast: invulnerable
        self.assertEqual(player.health, 150)
        player.body_picked_at = 1
        match.update(5.0, 3)
        self.assertEqual(player.rifle.ammo, 20)  # the Light Gun
        hosts = {item.index for item in player.body_extra_instances}
        self.assertIn(0x026A, hosts)  # the pilot's ult meter, and none of the mech's HUD
        self.assertFalse(hosts & {0x0276, 0x02CC, 0x0B2F})
        match._damage_player(player, 200, 6.0)
        self.assertEqual((player.health, player.respawn_at), (0, 9.0))
        match.update(9.0, 4)
        self.assertEqual((player.hero.body, player.health), (SPAWN_BODY[DVA], 600))

    def test_self_destruct_blows_the_mech_up_after_its_fuse_and_fills_call_mech(self):
        from kits import DVA
        from ow174.game.content import PILOT_BODY, SOLDIER, SPAWN_BODY
        from ow174.game.world import OP_CREATE, OP_DESTROY
        from target import TrainingTarget
        match, player = equipped(DVA)
        player.mover.position, player.select_open, match.mesh = [0.0, 0.0, 0.0], False, None
        near = TrainingTarget((0, 0, 5.0), 180, team=1 - player.team, hero=SOLDIER, entity=0xB0000201)
        far = TrainingTarget((0, 0, 25.0), 180, team=1 - player.team, hero=SOLDIER, entity=0xB0000202)
        match.bots = [near, far]
        player.bots_seen.update({near.entity, far.entity})
        near.next_attack_at = far.next_attack_at = None
        mech = player.body
        player.take_commands([Command(10, buttons=0x20)])  # Q, not charged
        self.assertIsNone(player.form_change)
        player.ult_charge = player.ult_most
        player.take_commands([Command(11), Command(12, buttons=0x20)])
        self.assertEqual(player.form_change, 'self_destruct')
        match.update(4.0, 2)
        self.assertEqual(player.hero.body, PILOT_BODY[DVA])
        destroyed = lambda: [u.entity for batch in player.client.updates for u in batch if u.op == OP_DESTROY]
        self.assertNotIn(mech, destroyed())
        match.update(6.9, 3)
        self.assertEqual(near.health, near.max_health)
        match.update(7.0, 4)
        self.assertEqual((near.health, far.health), (0, far.max_health))
        self.assertNotIn(mech, destroyed())  # its blast effect plays out first
        match.update(8.5, 5)
        self.assertIn(mech, destroyed())
        self.assertEqual(player.ult_charge, player.ult_most)  # Call Mech is ready
        player.body_picked_at = 1
        match.update(8.6, 6)
        player.take_commands([Command(20), Command(21, buttons=0x20)])
        self.assertEqual(player.form_change, 'call_mech')
        feet, yaw = tuple(player.mover.position), player.mover.yaw
        match.update(8.7, 7)
        self.assertEqual(player.hero.body, PILOT_BODY[DVA])  # her call plays first (kits.dva_call_mech)
        self.assertIn(21, next(i for i in player.body_extra_instances if i.index == 0x026A).active)
        # the mech falls onto CALL_MECH_AHEAD in front of her, from CALL_MECH_DROP up, faster as it comes
        drop, angle = player.entity | 0xFF, yaw * 2 * math.pi / 65536
        spot = (feet[0] + 1.5 * math.sin(angle), feet[1], feet[2] + 1.5 * math.cos(angle))
        sent = lambda op: [u.entity for batch in player.client.updates for u in batch if u.op == op]
        self.assertIn(drop, sent(OP_CREATE))
        self.assertNotIn(drop, [u.entity for u in match.remote_movements(player)])  # the create's frame
        match.now = 8.7 + CALL_MECH_CAST / 2
        falling = next(u.movement for u in match.remote_movements(player) if u.entity == drop)
        self.assertEqual([round(v, 6) for v in falling.position], [round(v, 6) for v in (spot[0], spot[1] + 15.0, spot[2])])
        self.assertLess(falling.velocity[1], 0)
        match.update(8.7 + CALL_MECH_CAST, 8)
        self.assertEqual((player.hero.body, player.health, player.ult_charge), (SPAWN_BODY[DVA], 600, 0))
        self.assertIn(drop, sent(OP_DESTROY))
        self.assertEqual([round(v, 6) for v in player.mover.position], [round(v, 6) for v in spot])  # she boards it there
        self.assertIsNone(player.mech_drop)

    def test_matrix_eats_range_shots_in_front_without_retroactive_or_friendly_blocks(self):
        from kits import DVA
        from target import Projectile
        for kind, start, target in (
            ('front', (0, 1.6, 15), (0, 1.4, 0)),
            ('behind', (0, 1.6, -15), (0, 1.4, 0)),
            ('side', (3, 1.6, 15), (3, 1.4, 0)),
            ('above', (0, 4, 15), (0, 4, 0)),
            ('released', (0, 1.6, 15), (0, 1.4, 0)),
            ('friendly', (0, 1.6, 15), (0, 1.4, 0)),
        ):
            with self.subTest(kind=kind):
                match, player = equipped(DVA)
                player.mover.position = [0.0, 0.0, 0.0]
                player.select_open = False
                match.bots = []
                player.take_commands([Command(10, buttons=SECONDARY)])
                if kind == 'released':
                    player.take_commands([Command(11)])
                shot = Projectile(start, target, 3.0)
                match.projectiles = [(None if kind == 'friendly' else player, shot)]
                match.update(3.2, 2)
                self.assertEqual(shot.landed, kind == 'front')
                if kind == 'front':
                    player.take_commands([Command(12)])
                    match.update(3.6, 3)
                    self.assertEqual(player.health, player.max_health)

    def test_matrix_sweeps_fast_shots_along_yaw_and_pitch(self):
        from kits import DVA
        from target import Projectile
        for yaw, pitch, start in ((0, 0, (0, 1.6, 15)), (16384, 0, (15, 1.6, 0)),
                                  (0, -8192, (0, 12.2, 10.6))):
            with self.subTest(yaw=yaw, pitch=pitch):
                match, player = equipped(DVA)
                player.mover.position, player.select_open, match.bots = [0.0, 0.0, 0.0], False, []
                player.take_commands([Command(10, buttons=SECONDARY, yaw=yaw, pitch=pitch)])
                shot = Projectile(start, (0, 1.6, 0), 3.0)
                match.projectiles = [(player, shot)]
                match.update(3.42, 2)  # beyond the near face, but not yet at the target
                self.assertTrue(shot.landed)
                self.assertEqual(player.health, player.max_health)

    def test_matrix_cannot_eat_a_shot_that_already_passed_its_volume(self):
        from kits import DVA
        from target import Projectile
        match, player = equipped(DVA)
        player.mover.position, player.select_open, match.bots = [0.0, 0.0, 0.0], False, []
        shot = Projectile((0, 1.6, 15), (0, 1.4, 0), 3.0)
        match.projectiles = [(player, shot)]
        match.update(3.426, 2)
        self.assertFalse(shot.landed)
        player.take_commands([Command(10, buttons=SECONDARY)])
        match.update(3.428, 3)
        self.assertFalse(shot.landed)
        match.update(3.5, 4)
        self.assertLess(player.health, player.max_health)

    @patch('target.random.gauss', return_value=0)
    def test_matrix_protects_allies_without_eating_friendly_bot_shots(self, _):
        from kits import DVA
        from ow174.game.content import SOLDIER
        from target import TrainingTarget
        for shooter_team in (0, 1):
            with self.subTest(shooter_team=shooter_team):
                match, player = equipped(DVA)
                player.mover.position, player.select_open = [0.0, 0.0, 5.0], False
                target = TrainingTarget((0, 0, 0), 0, team=1 - shooter_team, hero=SOLDIER, entity=0xB0000201)
                bot = TrainingTarget((0, 0, 20), 180, team=shooter_team, behaviour='shoot', hero=SOLDIER)
                bot.target, bot.retarget_at, bot.next_attack_at = target, 1000, 3.0
                match.bots = [bot, target]
                player.take_commands([Command(10, buttons=SECONDARY)])
                match.update(3.01, 2)
                self.assertEqual(target.health == target.max_health, shooter_team == 1)

    @patch('target.random.gauss', return_value=0)
    def test_matrix_blocks_hostile_bot_bullets_and_projectiles_but_not_beams_or_melee(self, _):
        from kits import DVA
        from target import PLAYER_KIT, TrainingTarget
        for name, blocked in (('Soldier: 76', True), ('Lúcio', True), ('Winston', False), ('Brigitte', False)):
            with self.subTest(hero=name):
                match, player = equipped(DVA)
                player.mover.position = [0.0, 0.0, 0.0]
                player.select_open = False
                hero = next(hero for hero in heroes().values() if hero.name == name)
                distance = 3 if name == 'Brigitte' else 8
                bot = TrainingTarget((0, 0, distance), 180, team=1, behaviour='shoot', hero=hero.guid,
                                     weapon=PLAYER_KIT[name][1])
                bot.target, bot.retarget_at, bot.next_attack_at, bot.ability = player, 1000, 3.0, None
                match.bots = [bot]
                player.take_commands([Command(10, buttons=SECONDARY)])
                before = player.health
                match.update(3.01, 2)
                match.update(3.6, 3)
                self.assertEqual(player.health == before, blocked)

    def test_matrix_blocks_projectile_casts_but_not_soundwave(self):
        from kits import DVA
        from target import ABILITIES, TrainingTarget
        for name, blocked in (('Soldier: 76', True), ('Ana', True), ('Cassidy', True), ('Lúcio', False)):
            with self.subTest(hero=name):
                match, player = equipped(DVA)
                player.mover.position = [0.0, 0.0, 0.0]
                player.select_open = False
                hero = next(hero for hero in heroes().values() if hero.name == name)
                bot = TrainingTarget((0, 0, 3), 180, team=1, behaviour='shoot', hero=hero.guid,
                                     ability=ABILITIES[name])
                bot.target, bot.retarget_at, bot.next_attack_at = player, 1000, 1000
                match.bots = [bot]
                player.take_commands([Command(10, buttons=SECONDARY)])
                before = player.health
                match.update(3.01, 2)
                self.assertEqual(bot.ability_uses, 1)
                self.assertEqual(player.health == before, blocked)

    def test_scope_input_refreshes_the_body_on_press_and_release(self):
        from kits import ANA, ASHE, WIDOWMAKER
        for hero in (ASHE, ANA, WIDOWMAKER):
            with self.subTest(hero=hero):
                match, player = equipped(hero)
                for frame, buttons in enumerate((SECONDARY, 0), 10):
                    player.body_dirty = False
                    player.take_commands([Command(frame, buttons=buttons)])
                    self.assertTrue(player.body_dirty)
                    match.update(3.1 + .1 * (frame - 10), frame)
                    self.assertFalse(player.body_dirty)
                    self.assertTrue(player.body_script.data_in_flight)
                    player.body_script.arrived(player.body_script.data_last)
                self.assertEqual(player.rifle.serial, 0)

    def assert_body_delta(self, player):
        update = player.client.updates[-1][0]
        self.assertIs(update.stream, player.body_script)
        self.assertTrue(update.resend)
        reader = BitReader(update.chunk.getvalue())
        self.assertEqual(reader.bit(), 0)
        self.assertGreater(reader.bits(16), 0)
        player.body_script.arrived(update.chunk_last)

    def test_widow_charge_rises_resets_and_clears(self):
        from kits import WIDOWMAKER

        match, player = equipped(WIDOWMAKER)
        player.take_commands([Command(10, buttons=SECONDARY)])
        match.update(3.1, 2)
        match.update(3.5, 3)
        self.assertAlmostEqual(player.widow_charge, 5)
        player.take_commands([Command(30, buttons=SECONDARY | 1)])
        match.update(3.51, 4)
        self.assertEqual(player.widow_charge, 1)
        match.update(3.99, 5)
        self.assertEqual(player.widow_charge, 1)
        match.update(4.92, 6)
        self.assertEqual(player.widow_charge, 10)
        player.take_commands([Command(31)])
        match.update(4.93, 7)
        self.assertEqual(player.widow_charge, 1)

    def test_reload_and_combat_refreshes_for_every_selectable_hero(self):
        for guid in practice.PracticeRange.menu_heroes():
            with self.subTest(hero=heroes()[guid].name):
                match, player = equipped(guid)
                rifle = player.rifle
                player.take_commands([Command(10, buttons=1), Command(11)])
                self.assertEqual(rifle.serial, 1)
                if rifle.capacity == INFINITE:
                    player.take_commands([Command(12, buttons=RELOAD)])
                    self.assertEqual(rifle.ammo, INFINITE)
                    self.assertIsNone(rifle.reload_until)
                    continue
                self.assertEqual(rifle.ammo, rifle.capacity - 1)
                # Drain shot presentation before triggering the reload.
                for i in range(2):
                    match.update(3.1 + i * .1, 2 + i)
                    player.body_script.arrived(player.body_script.data_last)
                # A recharging weapon (Doomfist) has no reload: its refill starts once the shot timer runs out.
                player.take_commands([Command(12 if rifle.recharge is None else 10 + rifle.interval, buttons=RELOAD)])
                match.update(3.3, 4)
                self.assertTrue(player.last_rifle_update.reloading)
                player.body_script.arrived(player.body_script.data_last)
                match._damage_player(player, 20, 3.4)
                match.update(3.4, 5)
                self.assert_body_delta(player)
                # not Hover Jets: they come on after the button has been held a while (test_ability_moves.py)
                cooldowns = [*(c for c in player.cooldowns if not getattr(c, 'delay', 0)),
                             *([player.field, player.helix] if player.field else [])]
                for i, cooldown in enumerate(cooldowns):
                    frame, now = 13 + i, 3.5 + .2 * i
                    self.assertTrue(cooldown.command(Command(frame, buttons=cooldown.button)))
                    # take_commands refreshes cast instances when the ability is queued.
                    player.refresh_abilities()
                    match.update(now, 6 + 2 * i)
                    self.assert_body_delta(player)
                    match.update(now + .1, 7 + 2 * i)
                    self.assert_body_delta(player)
                    self.assertTrue(player.last_rifle_update.reloading)
                player.take_commands([Command(rifle.reload_until or rifle.recharge_at)])
                self.assertEqual(rifle.ammo, rifle.capacity)
                self.assertFalse(rifle.updates[-1].reloading)

    def test_bastion_sentry_notifies_animator_and_survives_reload_refresh(self):
        from kits import BASTION
        from ow174.game.commands import FIRE
        from ow174.game.match import BASTION_RECONFIGURE
        from ow174.game.statescript import Asset

        match, player = equipped(BASTION)
        position = tuple(player.mover.position)
        player.take_commands([Command(10, forward=127, buttons=BASTION_RECONFIGURE | FIRE)])
        self.assertEqual((player.bastion_phase, player.rifle.ammo), (1, 300))
        self.assertEqual(tuple(player.mover.position), position)
        match.update(3.1, 2)
        player.body_script.arrived(player.body_script.data_last)
        match.update(4.1, 3)
        player.body_script.arrived(player.body_script.data_last)
        self.assertEqual(player.bastion_phase, 2)

        player.take_commands([Command(11, buttons=BASTION_RECONFIGURE | FIRE), Command(12)])
        self.assertEqual((player.bastion_phase, player.rifle.ammo), (2, 299))
        for now, tick in ((4.2, 4), (4.4, 5)):
            match.update(now, tick)
            player.body_script.arrived(player.body_script.data_last)
        player.take_commands([Command(13, buttons=RELOAD)])
        match.update(4.6, 6)
        instances, variables = player.body_script._reload_snapshot
        common = next(item for item in instances if item.graph.index == 75)
        self.assertIn(23, common.active)
        self.assertEqual((variables[1226], variables[1273]),
                         (Asset(0x028000000000007D), Asset(0x028000000000008E)))
        player.body_script.arrived(player.body_script.data_last)
        match._damage_player(player, 20, 4.7)
        match.update(4.7, 7)
        self.assert_body_delta(player)
        self.assertTrue(player.last_rifle_update.reloading)

        player.take_commands([Command(14, buttons=BASTION_RECONFIGURE)])
        match.update(4.8, 8)
        player.body_script.arrived(player.body_script.data_last)
        match.update(5.3, 9)
        self.assertEqual((player.bastion_phase, player.rifle.ammo), (0, 35))
        match.switch_hero(player, player.hero)
        self.assertIsNone(player.bastion_other_rifle)
        self.assertEqual((player.bastion_phase, player.bastion_switch_at), (0, 0.0))

    def test_bastion_cannot_move_on_his_client_whenever_the_server_roots_him(self):
        from kits import bastion_body
        from ow174.game.statescript import Bool

        for phase in range(4):  # Recon, to Sentry, Sentry, to Recon
            instances, variables, _ = bastion_body(phase, 0, [], {}, {})
            body = next(item for item in instances if item.graph.index == 0x012F)
            self.assertEqual({5, 34, 35} <= set(body.active), bool(phase))  # the no-move state and its switch
            self.assertEqual(variables[2291], Bool(bool(phase)))  # the switch's own variable

    def test_bastion_repair_animates_the_hold_and_releases_before_reload(self):
        from kits import BASTION, bastion_body

        match, player = equipped(BASTION)
        match._damage_player(player, 23, 3.05)  # 20 through its armour (target.armour_cut)
        player.take_commands([Command(10, buttons=SECONDARY)])
        for now, tick, phase in ((3.1, 2, 1), (3.36, 3, 2)):
            match.update(now, tick)
            player.body_script.arrived(player.body_script.data_last)
            self.assertEqual(player.bastion_repair_phase, phase)
            instances, _, _ = bastion_body(0, phase, player.body_extra_instances, {}, {})
            effect = next(item for item in instances if item.graph.index == 339).active[5]
            self.assertEqual(BitReader(effect.getvalue()).bit(), int(phase == 2))
        match.update(3.7, 4)
        self.assertAlmostEqual(player.health, player.max_health - 11)
        player.body_script.arrived(player.body_script.data_last)
        player.take_commands([Command(11)])
        match.update(3.8, 5)
        self.assertEqual(player.bastion_repair_phase, 3)
        self.assertAlmostEqual(player.health, player.max_health - 11)
        player.body_script.arrived(player.body_script.data_last)
        match.update(4.06, 6)
        self.assertEqual(player.bastion_repair_phase, 0)
        player.body_script.arrived(player.body_script.data_last)
        player.rifle.ammo -= 1
        player.take_commands([Command(12, buttons=SECONDARY | RELOAD)])
        match.update(4.2, 7)
        self.assertTrue(player.last_rifle_update.reloading)
        self.assertEqual(player.bastion_repair_phase, 0)

    def test_round_result_blocks_inputs_for_every_selectable_hero(self):
        for guid in practice.PracticeRange.menu_heroes():
            with self.subTest(hero=heroes()[guid].name):
                match, player = equipped(guid)
                match.game_map = dataclasses.replace(match.game_map, mode_guid=ASSAULT, objectives=((0, 0, 0),))
                match.assault = Assault(match.game_map.objectives, 0)
                match.assault.tick(match.assault.ends, 0, 0)
                before = tuple(player.mover.position)
                player.take_commands([Command(100, forward=127, buttons=1 | 2 | MELEE_BUTTON)])
                self.assertEqual(player.commands.last_frame, 100)
                self.assertEqual(tuple(player.mover.position), before)
                self.assertEqual((player.rifle.ammo, player.rifle.serial), (player.rifle.capacity, 0))
                self.assertEqual(player.melee.serial, 0)
