import dataclasses
import math
import struct
import unittest
from unittest.mock import patch

import practice  # noqa: F401  (puts vendor/ on the path for target's ow174 imports)
from target import BODIES, ROSTER, TrainingTarget, eye_height, range_bots
from ow174.game.world import NO_INPUT


def shooter():
    return TrainingTarget((0, 0, 3), 180, behaviour='shoot')


def hero_shooter(position=(0, 0, 0)):
    import hitbox
    return TrainingTarget(position, 0, team=1, behaviour='shoot', hero=1, volumes=hitbox.SOLDIER)


def _match():
    import practice  # noqa: F401  (puts vendor/ on the path)
    from ow174.game.content import PRACTICE_RANGE, SOLDIER
    from ow174.game.match import Match
    from test_soldier import _Client
    match = Match(PRACTICE_RANGE, gameplay=True)
    player = match.add_player(1, 'Player', SOLDIER, 0, True)
    player.client = _Client()
    player.has_body = player.spawned = True
    player.select_open = False
    player.body_picked_at = 1
    return match, player


class TargetTests(unittest.TestCase):
    def test_health_replication_preserves_maximum_and_exposes_current_health(self):
        import struct
        import practice  # noqa: F401
        from ow174.game import world
        for current in (200, 80, 0):
            part, = world.health(current, 200)[51]
            self.assertEqual(struct.unpack('<IffI4B', part),
                (1, 200, current, 3, 1, 1, 0, 0))

    def test_armour_and_shields_are_pools_of_their_own_damage_takes_first(self):
        import struct
        from ow174.game import world
        parts = world.health(450, 500, 200)[51]  # Reinhardt: 300 health and 200 armour, 50 lost
        self.assertEqual(struct.unpack('<IffI4B', parts[0]), (1, 300, 300, 3, 1, 1, 0, 0))
        self.assertEqual(parts[1:16], [None] * 15)
        self.assertEqual(struct.unpack('<IffI4B', parts[16]), (2, 200, 150, 1, 0, 1, 0, 1))
        parts = world.health(150, 400, 0, 200)[51]  # Zarya: 200 health and 200 shields, the shields gone
        self.assertEqual(struct.unpack('<IffI4B', parts[0])[1:3], (200, 150))
        self.assertEqual((len(parts), struct.unpack('<IffI4B', parts[32])), (33, (3, 200, 0, 2, 0, 1, 0, 1)))

    def test_hero_health_totals_match_their_body_pools(self):
        from ow174.game.content import heroes
        from ow174.game.pools import hero_pools
        from target import PLAYER_KIT
        for guid, hero in heroes().items():
            if hero.name != 'D.Va':  # her mech's pools are in her own graphs
                self.assertEqual(PLAYER_KIT[hero.name][0], sum(hero_pools(guid)), hero.name)

    def test_a_killed_player_learns_who_killed_him_and_lies_dead(self):
        import owner_correction
        match, player = _match()
        bot = TrainingTarget((0, 0, 5), 0, entity=0xB0000101)
        match._damage_player(player, 150, 10, bot.entity)
        self.assertNotIn(25004, [m[0] for m in player.client.messages])
        match._damage_player(player, 150, 11, bot.entity)
        self.assertEqual(player.client.messages[-1], (25004, {'+0x78': 2, '+0x7C': {'+0x0': bot.entity}}))
        self.assertTrue(player.movement().flags & 0x2000)
        self.assertFalse(owner_correction.correcting(player))
        sent = len(player.client.messages)
        match._damage_player(player, 20, 12, bot.entity)  # already dead: nothing more
        self.assertEqual((len(player.client.messages), player.respawn_at), (sent, 14))
        match, player = _match()
        match._damage_player(player, 500, 10)  # his own: "YOU DIED"
        self.assertEqual(player.client.messages[-1], (25004, {'+0x78': 0, '+0x7C': {'+0x0': player.body}}))

    def test_armour_stops_three_of_each_hit_at_most_half(self):
        from target import armour_cut
        self.assertEqual(armour_cut(20, 500, 500, 200, 0), 17)  # Reinhardt
        self.assertEqual(armour_cut(4, 500, 500, 200, 0), 2)  # at most half
        self.assertAlmostEqual(armour_cut(6.6 * 25, 500, 500, 200, 0, hits=25), 3.6 * 25)  # each pellet
        self.assertEqual(armour_cut(30, 410, 500, 200, 100), 10 + 17)  # the last 10 shields, then armour stops 3
        self.assertEqual(armour_cut(30, 320, 500, 200, 0), 20 + 7)  # cut to 27: the last 20 armour, then 7 health
        self.assertEqual(armour_cut(20, 200, 200, 0, 0), 20)  # no armour

    def test_a_dead_hero_waits_the_modes_respawn_time(self):
        import dataclasses
        match, player = _match()
        match._damage_player(player, 999, 10)
        self.assertEqual(player.respawn_at, 13)  # the Practice Range's 13C0 sends 3
        match, player = _match()
        match.game_map = dataclasses.replace(match.game_map, mode_guid=0x0230000000000014)  # Assault: 0F4A's 10
        match._damage_player(player, 999, 10)
        self.assertEqual((player.respawn_at, match.respawn_seconds()), (20, 10))

    def test_hero_bots_take_turns_on_their_team_spawns(self):
        from ow174.game.content import Spawn
        from target import hero_bots
        spawns = (Spawn(0, (0, 0, 0), 0), Spawn(1, (5, 0, 0), 90), Spawn(1, (9, 0, 0), 90))
        bots = hero_bots(spawns, 1, [(0x02E000000000006E, 0x04000000000003CF, 'Soldier: 76')] * 6)
        self.assertEqual([bot.position[0] for bot in bots], [5, 9, 5, 9, 5, 9])
        self.assertEqual(len({bot.entity for bot in bots}), 6)
        self.assertEqual(hero_bots(spawns, 2, [(0x02E000000000006E, 0x04000000000003CF, 'Soldier: 76')]), [])

    def test_payload_stand_in_is_a_still_friendly_range_bot(self):
        from target import BOT_BODY, PAYLOAD, PAYLOAD_HEALTH, payload_target
        bot = payload_target((1.0, 2.0, 3.0))
        self.assertEqual((bot.team, bot.hero, bot.behaviour, bot.body, bot.health, bot.entity, bot.position),
                         (0, None, PAYLOAD, BOT_BODY, PAYLOAD_HEALTH, 0xB000FF01, (1.0, 2.0, 3.0)))
        self.assertIsNone(bot.attack_player((1.0, 2.0, 6.0), 1.0, True))  # it never shoots

    def test_omnic_wave_takes_the_markers_in_turn(self):
        import hitbox
        from ow174.game.content import Spawn
        from target import DIFFICULTY, OMNICS, hero_name, omnic_bots
        spawns = (Spawn(0, (0, 0, 0), 0), Spawn(1, (5, 0, 0), 90), Spawn(1, (9, 0, 0), 90))
        bots = omnic_bots(spawns)
        self.assertEqual([hero_name(bot.hero) for bot in bots], ['Nulltrooper'] * 4 + ['Slicer'] * 2)
        self.assertEqual([(bot.team, bot.position[0], bot.body) for bot in bots[:2]],
                         [(1, 5, 0x040000000000184A), (1, 9, 0x040000000000184A)])
        self.assertEqual(hero_name(0x02E000000000006E), 'Soldier: 76')
        self.assertTrue(all(body in hitbox.HEROES for _, body in OMNICS.values()))
        # the wave focused on one player at medium difficulty: 200 health lasts about 3 s, not Soldier's half second
        dps = sum(bot.weapon.damage / bot.weapon.shot_seconds for bot in bots) * DIFFICULTY['medium'][0]
        self.assertTrue(50 <= dps <= 70, dps)

    def test_roster_matches_map_data_counts(self):
        bots = range_bots()
        self.assertEqual(len(bots), 15)
        self.assertEqual(sum(bot.team == 0 for bot in bots), 4)  # wiki 2019: four friendly, eleven hostile
        self.assertEqual(len({bot.entity for bot in bots}), 15)
        self.assertEqual({bot.body for bot in bots}, set(BODIES.values()))
        self.assertEqual(sorted(bot.behaviour or '' for bot in bots).count('shoot'), 3)
        for _, _, _, _, _, behaviour, waypoints in ROSTER:
            self.assertEqual(behaviour == 'patrol', bool(waypoints))

    def test_hostile_shooters_face_the_friendly_pair(self):
        by_record = {row[0]: row for row in ROSTER}
        for shooter_record in (19, 429):
            _, _, team, (x, _, z), yaw, behaviour, _ = by_record[shooter_record]
            self.assertEqual((team, behaviour), (1, 'shoot'))
            tx, _, tz = by_record[3][3]
            facing = (math.sin(math.radians(yaw)), math.cos(math.radians(yaw)))
            aim = ((tx - x) / math.hypot(tx - x, tz - z), (tz - z) / math.hypot(tx - x, tz - z))
            self.assertGreater(facing[0] * aim[0] + facing[1] * aim[1], math.cos(math.radians(30)))

    def test_helix_splash_follows_authored_rings(self):
        from soldier import helix_splash
        self.assertEqual([helix_splash(d) for d in (0, 1, 2, 3, 3.01)], [80, 80, 60, 40, 0])

    def test_helix_splash_hurts_the_shooter_by_half(self):
        match, player = _match()
        player.has_body, player.health = True, 200
        player.mover.position = [0.0, 0.0, 0.0]
        match.rockets = [(100.0, (0.0, 1.0, 2.0), None, [], player, None)]  # 2 m from its centre: splash 60
        match._land_rockets(100.0)
        self.assertEqual(player.health, 170)

    def test_shots_leave_from_the_heros_own_eye_height(self):
        match, player = _match()
        player.mover.position = [0.0, 0.0, 0.0]
        self.assertEqual(player.eye(), (0.0, 1.55, 0.0))  # Soldier: 76's STUFirstPersonComponent
        from ow174.game.mover import CROUCHED
        player.mover.flags_word |= CROUCHED
        self.assertEqual(player.eye(), (0.0, 1.085, 0.0))
        self.assertEqual(eye_height(0x04000000000028B6), 1.0)  # Torbjörn
        self.assertEqual(eye_height(0x0400000000009999, True), 1.15)  # an unlisted body: the component's default

    def test_bot_pose_at_a_past_time_interpolates_its_kept_poses(self):
        bot = TrainingTarget((0, 0, 0), 0)
        self.assertEqual(bot.pose_at(5.0), ((0, 0, 0), 0))  # nothing kept: the current pose
        bot.remember(1.0)
        bot.position, bot.yaw = (2.0, 0.0, 0.0), 90
        bot.remember(2.0)
        self.assertEqual(bot.pose_at(1.25), ((0.5, 0.0, 0.0), 0))
        self.assertEqual(bot.pose_at(1.75)[1], 90)  # the nearer pose's yaw
        self.assertEqual(bot.pose_at(0.0), ((0, 0, 0), 0))  # before the oldest: the oldest
        self.assertEqual(bot.pose_at(9.0), ((2.0, 0.0, 0.0), 90))  # after the newest: the newest

    def test_shots_hit_the_bot_where_the_shooter_saw_it(self):
        from ow174.game.commands import Command
        match, player = _match()
        bot = TrainingTarget((0, 0, 10), 180, team=1, entity=0xB0000101)
        match.bots, player.bots_seen = [bot], {bot.entity}
        player.mover.position = [0.0, 1.0 - eye_height(player.hero.body), 0.0]  # eye at the bot's chest
        match.started = match.now = 100.0
        bot.remember(100.0)  # in the aim
        bot.position = (1.5, 0, 10)  # then walked out of it
        bot.remember(100.2)
        match.now = 100.2
        frame = 1000 + round(0.2e6 / player.quanta)  # the command frame of match time 100.2
        match.fire_at_target(player, Command(frame, yaw=0, extra=200))  # its client drew the bot 0.2 s back
        self.assertLess(bot.health, 200)
        health = bot.health
        match.fire_at_target(player, Command(frame + 1, yaw=0))  # no view delay: the bot is out of the aim
        self.assertEqual(bot.health, health)

    def test_helix_rocket_lands_after_flight_and_spares_friendly_bots(self):
        from ow174.game.commands import Command
        match, player = _match()
        enemy = TrainingTarget((0, 0, 10), 180, team=1, entity=0xB0000101)
        enemy.volumes = (((0, 1.373, 0), (0, 1.373, 0), .48, False),)  # known sphere for flight timing
        friend = TrainingTarget((1.5, 0, 10), 180, team=0, entity=0xB0000201)
        match.bots = [enemy, friend]
        player.bots_seen = {enemy.entity, friend.entity}
        player.mover.position = [0.0, 1.5 - eye_height(player.hero.body), 0.0]  # eye 1.5: the hovering bots' chest
        with patch('ow174.game.match.time.time', return_value=100.0):
            match.fire_helix(player, Command(1, yaw=0))
        (land, *_), = match.rockets
        self.assertAlmostEqual(land, 100 + (10 - math.sqrt(0.48 ** 2 - 0.127 ** 2)) / 50)  # the body sphere
        match._land_rockets(land - 0.01)
        self.assertEqual(enemy.health, 200)
        match._land_rockets(land)
        self.assertEqual(enemy.health, 200 - 40 - 80)  # direct hit plus the full inner ring
        self.assertEqual(friend.health, 200)
        self.assertEqual(match.rockets, [])
        (_, shot), = match.projectiles  # drawn as Volley 38's projectile child
        self.assertEqual(shot.entity, 0x040000000000072E)

    def test_a_helix_rocket_into_the_open_is_drawn_without_exploding(self):
        from ow174.game.commands import Command
        match, player = _match()
        match.bots = []
        match.fire_helix(player, Command(1, yaw=0))
        self.assertEqual((match.rockets, len(match.projectiles)), ([], 1))

    def test_a_rocket_flies_through_a_friendly_bot_in_its_way(self):
        from ow174.game.commands import Command
        match, player = _match()
        enemy = TrainingTarget((0, 0, 10), 180, team=1, entity=0xB0000101)
        friend = TrainingTarget((0, 0, 5), 180, team=0, entity=0xB0000201)
        match.bots = [enemy, friend]
        player.bots_seen = {enemy.entity, friend.entity}
        player.mover.position = [0.0, -0.6, 0.0]
        with patch('ow174.game.match.time.time', return_value=100.0):
            match.fire_helix(player, Command(1, yaw=0))
        (land, _, direct, *_), = match.rockets
        self.assertIs(direct, enemy)
        match._land_rockets(land)
        self.assertEqual((enemy.health, friend.health), (200 - 40 - 80, 200))

    def test_dynamite_hurts_the_thrower_by_half_its_splash(self):
        from ow174.game.commands import Command
        from target import ABILITY_HITS
        dynamite = ABILITY_HITS[('Ashe', 0x10)]  # 50 within 5 m
        match, player = _match()
        player.mover.position = [0.0, -0.6, 0.0]
        wall = patch.object(match, 'mesh', type('Mesh', (), {'ray': lambda self, o, d, r: (0.5,)})())  # it lands by the feet
        with wall, patch('ow174.game.match.time.time', return_value=100.0):
            match.fire_ability(player, Command(1, yaw=0), dynamite)
        (land, *_), = match.rockets
        player.has_body, player.health = True, 200
        match._land_rockets(land)
        self.assertEqual(player.health, 200 - 25)

    def test_an_ability_cast_lands_at_its_range_and_splashes(self):
        from ow174.game.commands import Command
        from target import ABILITY_HITS, ability_splash
        mine = ABILITY_HITS[('Junkrat', 0x08)]  # 1.74's ring: 120 anywhere within 3 m
        self.assertEqual([ability_splash(mine, d) for d in (0, 1, 2, 3, 3.01)], [120, 120, 120, 120, 0])
        accretion = ABILITY_HITS[('Sigma', 0x10)]  # 40 to 1.5 m, 30 % of it at 2.5 m
        self.assertEqual([ability_splash(accretion, d) for d in (1.5, 2, 2.5, 2.6)], [40, 26, 12, 0])
        dynamite = ABILITY_HITS[('Ashe', 0x10)]  # no data rings: Helix's shape, full to a third, half at the radius
        self.assertEqual([ability_splash(dynamite, d) for d in (5 / 3, 5, 5.01)], [50, 25, 0])
        match, player = _match()
        enemy = TrainingTarget((0, 0, 26), 180, team=1, entity=0xB0000101)  # 1 m past the mine's range
        match.bots, player.bots_seen = [enemy], {enemy.entity}
        player.mover.position = [0.0, -0.6, 0.0]
        with patch('ow174.game.match.time.time', return_value=100.0):
            match.fire_ability(player, Command(1, yaw=0), mine)
        (land, impact, direct, *_), = match.rockets
        self.assertEqual((land, direct), (101.0, None))  # nothing in its way: it lands at 25 m after 1 s
        (_, shot), = match.projectiles  # drawn as the Volley's own projectile
        self.assertEqual((shot.entity, shot.end - shot.start), (mine.entity, 1.0))
        player.has_body, player.health = True, 200
        match._land_rockets(land)
        self.assertEqual((enemy.health, player.health), (200 - 120, 200))  # within a metre; no self-damage

    def test_micro_missiles_stream_from_both_pods_at_the_crosshair(self):
        from ow174.game.commands import Command
        from target import ABILITY_HITS
        missiles = ABILITY_HITS[('D.Va', 0x10)]
        match, player = _match()
        player.mover.position, player.mover.yaw, player.has_body, player.health = [0.0, 0.0, 0.0], 0, True, 600
        with patch('ow174.game.match.time.time', return_value=100.0):
            match.fire_ability(player, Command(1, yaw=0), missiles)
        match._tick_salvos(100.0 + 0.5)  # 11/s: shots 0..5
        starts = [shot.origin for _, shot in match.projectiles]
        self.assertEqual(len(starts), 6)
        self.assertEqual({round(s[0], 2) for s in starts}, {-0.74, 0.74})  # alternating pods (x = -right at yaw 0)
        self.assertTrue(all(abs(shot.target[0]) < 0.01 for _, shot in match.projectiles))  # all at the crosshair line
        match._tick_salvos(100.0 + 2.0)
        self.assertEqual((len(match.projectiles), match.salvos), (18, []))

    def test_symmetra_orb_flies_and_lands_120_on_a_direct_hit(self):
        from ow174.game.commands import Command
        from target import ABILITY_HITS
        orb = ABILITY_HITS[('Symmetra', 0x02)]
        match, player = _match()
        struck = TrainingTarget((0, 0, 10), 200, team=1, entity=0xB0000101)
        match.bots, player.bots_seen = [struck], {struck.entity}
        player.mover.position = [0.0, -0.6, 0.0]
        with patch('ow174.game.match.time.time', return_value=100.0):
            match.fire_ability(player, Command(1, yaw=0), orb)
        (land_at, *_), = match.rockets
        self.assertAlmostEqual(land_at, 100.0 + 10 / 25, delta=0.05)  # 25 m/s
        match._land_rockets(land_at)
        self.assertEqual(struck.health, 80)

    def test_biotic_grasp_drains_the_bot_in_its_cone_and_heals_moira(self):
        from kits import MOIRA_GRASP_STATES
        from ow174.game.commands import Command
        match, player = _match()
        drained = TrainingTarget((1, 0, 10), 200, team=1, entity=0xB0000101)  # about 6 degrees off the aim
        far = TrainingTarget((0, 0, 25), 200, team=1, entity=0xB0000102)
        match.bots, player.bots_seen = [drained, far], {drained.entity, far.entity}
        player.mover.position = [0.0, -0.6, 0.0]
        player.health = 100
        match.now = 100.0
        match.update_grasp(player, Command(1, buttons=0x02, yaw=0))
        self.assertEqual(player.weapon_extra_states.keys() & set(MOIRA_GRASP_STATES), set(MOIRA_GRASP_STATES))
        match.now = 100.0 + 0.192 * 5 + 0.01
        match.update_grasp(player, Command(2, buttons=0x02, yaw=0))
        self.assertAlmostEqual(drained.health, 200 - 5 * 9.6)
        self.assertAlmostEqual(player.health, 100 + 5 * 4.61)
        self.assertEqual(far.health, 200)  # past the 20 m reach
        match.update_grasp(player, Command(3, yaw=0))
        self.assertFalse(player.weapon_extra_states.keys() & set(MOIRA_GRASP_STATES))

    def test_mercy_staff_heals_the_ally_in_its_cone_and_tags_its_remote_frame(self):
        from ow174.game.commands import Command
        from kits import MERCY_STAFF_STATES, PAYLOADS
        from ow174.game.match import HEAL_STATUS, remote_volley_frame
        match, player = _match()
        ally = TrainingTarget((1, 0, 10), 200, team=player.team, entity=0xB0000101, max_health=200)
        enemy = TrainingTarget((0, 0, 8), 200, team=1 - player.team, entity=0xB0000102, max_health=200)
        ally.health = enemy.health = 100
        match.bots, player.bots_seen = [ally, enemy], {ally.entity, enemy.entity}
        player.mover.position = [0.0, -0.6, 0.0]
        match.now = 100.0
        match.update_staff(player, Command(1, buttons=0x01, yaw=0))
        self.assertEqual(player.staff_target, ally.entity)  # the enemy nearer the crosshair is not healed
        match.now = 100.0 + 0.1 * 5 + 0.01
        match.update_staff(player, Command(2, buttons=0x01, yaw=0))
        self.assertAlmostEqual(ally.health, 100 + 5 * 6.0)
        self.assertEqual(enemy.health, 100)
        self.assertEqual(player.weapon_extra_states[26].getvalue(), PAYLOADS[1]().getvalue())  # switch 26: heal
        self.assertIn(25, player.weapon_extra_states)  # the heal glow
        plain = remote_volley_frame('Soldier: 76', 30, False, 0).getvalue()
        heal = remote_volley_frame('Soldier: 76', 30, False, 0, healer=(0xA0000101, False)).getvalue()
        boost = remote_volley_frame('Soldier: 76', 30, False, 0, healer=(0xA0000101, True)).getvalue()
        self.assertEqual(len({plain, heal, boost}), 3)
        self.assertEqual(HEAL_STATUS.index, 0x0079)
        match.update_staff(player, Command(3, buttons=0x02, yaw=0))  # the secondary boosts: no healing, boost states
        self.assertTrue(player.staff_boost)
        self.assertEqual(player.staff_target, ally.entity)
        self.assertIn(24, player.weapon_extra_states)
        self.assertNotIn(25, player.weapon_extra_states)
        health = ally.health
        match.now += 1.0
        match.update_staff(player, Command(4, buttons=0x02, yaw=0))
        self.assertEqual(ally.health, health)
        match.update_staff(player, Command(5, yaw=0))
        self.assertIsNone(player.staff_target)
        self.assertFalse(any(state in player.weapon_extra_states for state in MERCY_STAFF_STATES))

    def test_mercy_beam_ends_when_staff_or_body_stops_being_used(self):
        from kits import MERCY, MERCY_STAFF_STATES
        from ow174.game.commands import Command
        from ow174.game.content import SOLDIER, heroes
        from test_roster import equipped
        for transition in ('blaster', 'death', 'hero', 'leave'):
            with self.subTest(transition=transition):
                match, player = equipped(MERCY)
                ally = TrainingTarget((0, 0, 10), 180, team=player.team)
                match.bots, player.bots_seen = [ally], {ally.entity}
                player.mover.position = [0.0, -0.6, 0.0]
                match.update_staff(player, Command(1, buttons=0x01))
                self.assertEqual(player.staff_target, ally.entity)
                if transition == 'blaster':
                    player.switch_mercy_weapon(True)
                elif transition == 'death':
                    match._damage_player(player, player.health, match.now)
                elif transition == 'hero':
                    match.switch_hero(player, heroes()[SOLDIER])
                else:
                    match.leave(player)
                self.assertIsNone(player.staff_target)
                self.assertFalse(set(MERCY_STAFF_STATES) & player.weapon_extra_states.keys())

    def test_mercy_heal_status_reaches_an_ally_hero_without_a_served_weapon(self):
        # Reinhardt has no WEAPON_GRAPHS entry; his remote frame must still carry heal status 0x0079 (the beam)
        from kits import MERCY
        from ow174.game.commands import Command
        from test_roster import equipped
        match, player = equipped(MERCY)
        ally = TrainingTarget((0, 0, 10), 180, team=player.team, hero=0x02E0000000000007, entity=0xB0000301)
        ally.target = object()  # a hero bot with a target must not need a weapon graph for shot events
        match.bots, player.bots_seen = [ally], {ally.entity}
        player.mover.position = [0.0, -0.6, 0.0]
        frames = lambda: [u for batch in player.client.updates for u in batch if u.entity == ally.entity and u.stream]
        match._send_bot_weapons()
        self.assertEqual(frames(), [])  # nothing to show while unhealed
        match.update_staff(player, Command(1, buttons=0x01))
        match._send_bot_weapons()
        self.assertEqual(len(frames()), 1)
        self.assertEqual(player.bot_weapons[ally.entity][1][5], (player.body, False))
        match.update_staff(player, Command(2))
        match._send_bot_weapons()
        self.assertEqual(len(frames()), 2)  # the release clears the status, ending the beam

    def test_moira_grasp_ends_when_body_stops_being_used(self):
        from kits import MOIRA, MOIRA_GRASP_STATES
        from ow174.game.commands import Command
        from ow174.game.content import SOLDIER, heroes
        from test_roster import equipped
        for transition in ('death', 'hero', 'leave'):
            with self.subTest(transition=transition):
                match, player = equipped(MOIRA)
                enemy = TrainingTarget((0, 0, 10), 180, team=1 - player.team)
                match.bots, player.bots_seen = [enemy], {enemy.entity}
                player.mover.position = [0.0, -0.6, 0.0]
                match.update_grasp(player, Command(1, buttons=0x02))
                self.assertEqual(player.grasp_target, enemy.entity)
                if transition == 'death':
                    match._damage_player(player, player.health, match.now)
                elif transition == 'hero':
                    match.switch_hero(player, heroes()[SOLDIER])
                else:
                    match.leave(player)
                self.assertIsNone(player.grasp_target)
                self.assertFalse(set(MOIRA_GRASP_STATES) & player.weapon_extra_states.keys())

    def test_fire_strike_pierces_each_enemy_when_it_gets_there(self):
        from ow174.game.commands import Command
        from target import ABILITY_HITS
        strike = ABILITY_HITS[('Reinhardt', 0x10)]  # 100 to each enemy it passes, 25 m/s
        match, player = _match()
        near = TrainingTarget((0, 0, 5), 180, team=1, entity=0xB0000101)
        far = TrainingTarget((0, 0, 10), 180, team=1, entity=0xB0000102)
        friend = TrainingTarget((0, 0, 7), 180, team=0, entity=0xB0000201)
        match.bots, player.bots_seen = [near, far, friend], {near.entity, far.entity, friend.entity}
        player.mover.position = [0.0, -0.6, 0.0]
        with patch('ow174.game.match.time.time', return_value=100.0):
            match.fire_ability(player, Command(1, yaw=0), strike)
        first, second = sorted(match.rockets, key=lambda rocket: rocket[0])
        self.assertEqual((first[2], second[2]), (near, far))
        match._land_rockets(first[0])
        self.assertEqual((near.health, far.health), (100, 200))  # the far one is not reached yet
        match._land_rockets(second[0])
        self.assertEqual((near.health, far.health, friend.health), (100, 100, 200))
        (_, shot), = match.projectiles  # drawn on to its range: nothing stops it
        self.assertEqual((shot.entity, shot.end - shot.start), (strike.entity, 40 / 25))

    def test_biotic_orb_waits_for_its_pick_after_the_press(self):
        from kits import Choice
        from ow174.game.commands import Command
        orb = Choice(16666, 0x10, 8, (0x01, 0x02))
        self.assertFalse(orb.command(Command(10, buttons=0x10)))  # E arms it
        self.assertTrue(orb.choosing)
        self.assertFalse(orb.command(Command(11)))
        self.assertTrue(orb.command(Command(12, buttons=0x02)))  # right mouse: the damage orb
        self.assertEqual((orb.pick, len(orb.updates)), (0x02, 2))
        self.assertFalse(orb.command(Command(13, buttons=0x02)))  # still held: no second cast, and no weapon shot
        self.assertTrue(orb.choosing)
        self.assertFalse(orb.command(Command(14, buttons=0x10)))  # let go; on cooldown, E does not arm it again
        self.assertFalse(orb.choosing)

    def test_a_picked_orb_sends_that_volley(self):
        from kits import ABILITIES, MOIRA, Choice, ability_instance
        orb_ability = next(a for a in ABILITIES[MOIRA] if a.choice)
        from ow174.game.commands import Command
        cooldown = Choice(16666, 0x10, 8, (0x01, 0x02))
        cooldown.command(Command(1, buttons=0x10))
        cooldown.command(Command(2, buttons=0x02))  # right mouse
        active = ability_instance(orb_ability, cooldown, cooldown.updates[0]).active
        self.assertIn(11, active)  # Volley 11, the damage orb
        self.assertNotIn(10, active)

    def test_damage_orb_stops_at_a_wall_and_spends_at_most_its_total(self):
        from ow174.game.commands import Command
        from target import ORBS
        match, player = _match()
        enemy = TrainingTarget((0, 0, 2), 180, team=1, entity=0xB0000101)
        match.bots, player.bots_seen = [enemy], {enemy.entity}
        player.mover.position = [0.0, -0.6, 0.0]
        match.mesh = type('Mesh', (), {'ray': lambda self, o, d, r: (2.0,)})()
        match.now = 100.0
        match.launch_orb(player, Command(1, yaw=0), ORBS[('Moira', 0x10, 0x02)])  # 50/s up to 200, 4 m
        match._tick_orbs(100.5)
        self.assertLess(enemy.health, 200)
        match._tick_orbs(108.0)  # 7 s at the wall would be 350: the orb runs dry at 200
        self.assertAlmostEqual(enemy.health, 0)
        self.assertEqual(match.orbs, [])

    def test_healing_orb_heals_hurt_allies_only(self):
        from ow174.game.commands import Command
        from target import ORBS
        match, player = _match()
        friend = TrainingTarget((0, 0, 2), 180, team=0, entity=0xB0000201)
        enemy = TrainingTarget((1, 0, 2), 180, team=1, entity=0xB0000101)
        friend.health = enemy.health = 100
        match.bots, player.bots_seen = [friend, enemy], {friend.entity, enemy.entity}
        player.mover.position = [0.0, -0.6, 0.0]
        match.mesh = type('Mesh', (), {'ray': lambda self, o, d, r: (2.0,)})()
        match.now = 100.0
        match.launch_orb(player, Command(1, yaw=0), ORBS[('Moira', 0x10, 0x01)])  # 65/s up to 300, 5 m
        match._tick_orbs(108.0)
        self.assertEqual((friend.health, enemy.health), (200, 100))
        self.assertEqual(match.orbs, [])  # its lifetime is over, 200 of its 300 unspent

    def test_helix_cooldown_is_six_seconds_of_command_frames(self):
        from soldier import Volley
        from ow174.game.commands import Command
        helix = Volley(16000)  # 62.5 command frames per second
        self.assertTrue(helix.command(Command(100, buttons=2)))
        self.assertFalse(helix.command(Command(200, buttons=2)))
        self.assertTrue(helix.command(Command(100 + 375, buttons=2)))
        self.assertEqual([u.active for u in helix.updates], [True, False, True, False])

    def test_biotic_field_heals_teammates_in_radius_for_five_seconds(self):
        match, player = _match()
        other = match.add_player(2, 'Other', player.hero.guid, 0, True)
        other.has_body = True
        other.mover.position = [5.0, 0.0, 0.0]  # outside the 4.5 m radius
        player.mover.position = [0.0, 0.0, 0.0]
        player.health = other.health = 100
        match.now = 10.0
        match.throw_field(player)
        (entity, *_), = match.fields
        match._tick_fields(10.0)
        self.assertEqual((player.health, other.health), (107, 100))
        bot = TrainingTarget((1.0, 0, 0), 0, team=0, entity=0xB0000301, hero=1, max_health=200)
        bot.health = 150
        match.bots = [bot]
        player.mover.position[0] = 5.0  # the field stays where it was thrown
        match._tick_fields(10.2)
        self.assertEqual((player.health, bot.health), (107, 157))  # a hero bot of the team heals too
        player.health = 100.5  # after a healer bot's fractional heal; the ult meter stays an Int (it crashed the frame)
        from ow174.game.statescript import BitWriter, Int
        player.body_extra_variables = {342: Int(0)}
        match._tick_fields(10.4)
        player.body_extra_variables[342].write(BitWriter())
        player.health = 107
        match.bots = []
        player.mover.position[0] = 0.0
        for tick in range(2, 25):  # ticks at 10.4 .. 14.8: 35 per second
            match._tick_fields(10 + tick * 0.2)
        self.assertEqual(player.health, 200)
        match._tick_fields(15.0)
        self.assertEqual(match.fields, [])
        from ow174.game.world import OP_DESTROY
        destroy, = player.client.updates[-1]
        self.assertEqual((destroy.entity, destroy.op), (entity, OP_DESTROY))

    def test_damage_dealt_charges_the_ultimate_up_to_its_cost(self):
        from soldier import ULT_CHARGE
        from ow174.game.statescript import Int
        match, player = _match()
        player.body_extra_variables = {ULT_CHARGE: Int(0)}
        match._charge_ult(player, 120)
        self.assertEqual((player.ult_charge, player.body_dirty), (120, True))
        match._charge_ult(player, 5000)
        self.assertEqual(player.ult_charge, 2310)  # points; the meter var counts in the 1650 cost units
        self.assertEqual(player.body_extra_variables[ULT_CHARGE], Int(1650))
        match._charge_ult(player, 1)
        self.assertEqual(player.body_extra_variables[ULT_CHARGE], Int(1650))

    def test_tactical_visor_hits_the_enemy_nearest_the_crosshair_without_aiming(self):
        from soldier import ULT_CHARGE
        from ow174.game.commands import Command
        from ow174.game.statescript import Int
        match, player = _match()
        near = TrainingTarget((3, 0, 10), 180, team=1, entity=0xB0000101)  # ~17 degrees off the aim
        far = TrainingTarget((-8, 0, 10), 180, team=1, entity=0xB0000201)
        behind = TrainingTarget((0, 0, -5), 0, team=1, entity=0xB0000301)
        match.bots = [near, far, behind]
        player.bots_seen = {bot.entity for bot in match.bots}
        player.mover.position = [0.0, -0.6, 0.0]
        player.body_extra_variables = {ULT_CHARGE: Int(1650)}
        player.ult_charge, player.refresh_abilities = 2310, lambda: None
        match.now = 50.0
        match.start_visor(player)
        self.assertEqual((player.visor_cast_until, player.visor_until, player.ult_charge), (51.2, 57.2, 0))
        self.assertEqual(player.body_extra_variables[ULT_CHARGE], Int(0))
        match.fire_at_target(player, Command(1, yaw=0))
        self.assertEqual(near.health, 200)  # no lock-on during the 1.2 s cast
        player.visor_cast_until = 0.0  # the tick clears it when the cast ends
        match.fire_at_target(player, Command(1, yaw=0))
        self.assertEqual((near.health, far.health, behind.health), (180, 200, 200))
        self.assertEqual(player.ult_charge, 0)  # no charge while the ultimate runs

    def test_biotic_field_cooldown_is_fifteen_seconds(self):
        import practice  # noqa: F401
        from soldier import FIELD_BUTTON, FIELD_COOLDOWN, Volley
        from ow174.game.commands import Command
        field = Volley(16000, FIELD_BUTTON, FIELD_COOLDOWN)
        self.assertFalse(field.command(Command(100, buttons=2)))
        self.assertTrue(field.command(Command(100, buttons=FIELD_BUTTON)))
        self.assertFalse(field.command(Command(100 + 937, buttons=FIELD_BUTTON)))
        self.assertTrue(field.command(Command(100 + 938, buttons=FIELD_BUTTON)))

    def test_cooldown_timing_block_carries_elapsed_time(self):
        import practice  # noqa: F401
        from soldier import FIELD_BUTTON, FIELD_COOLDOWN, Volley, ability_payload
        from ow174.game.commands import Command
        field = Volley(16000, FIELD_BUTTON, FIELD_COOLDOWN)
        key = lambda payload: (payload.value, payload.count)
        self.assertEqual(key(field.ability()), key(ability_payload()))  # ready: no timing block
        field.command(Command(100, buttons=FIELD_BUTTON))
        field.command(Command(162))  # 62 ticks of 16 ms later
        self.assertEqual(key(field.ability()), key(ability_payload(0, (15.0, 992))))
        field.command(Command(100 + 938))
        self.assertEqual(key(field.ability()), key(ability_payload()))

    def test_patrol_walks_as_retails_bots_did(self):
        """FIRST_WAIT, then keys held: up to the Training Bot body's 4.5 m/s at 4.5 m/s^2, letting go where its 67.5 m/s^2
        friction stops it on the stop, WALK_GAP, then back, turning 60 degrees a second."""
        bot = TrainingTarget((0, 0, 0), 0, behaviour='patrol', waypoints=((0, 0, 0), (0, 0, 20)))
        clock = iter(100 + k * 0.016 for k in range(2000))

        def walk(seconds):
            for _ in range(round(seconds / 0.016)):
                bot.patrol(next(clock))

        walk(2.9)
        self.assertEqual((bot.position, bot.held), ((0, 0, 0), (0, 0)))
        walk(0.2)
        self.assertEqual(bot.held, (0, 127))
        self.assertTrue(bot.keys)
        walk(1.1)
        self.assertAlmostEqual(bot.pace, 4.5)
        self.assertAlmostEqual(bot.velocity[2], 4.5)
        walk(4.0)
        self.assertEqual((bot.position, bot.velocity, bot.held, bot.last_held), ((0, 0, 20), (0, 0, 0), (0, 0), (0, 127)))
        self.assertFalse(bot.keys)
        walk(0.6)  # WALK_GAP (1.2 s) from letting go
        self.assertEqual(bot.position, (0, 0, 20))
        walk(0.5)
        self.assertLess(bot.position[2], 20)
        right, forward = bot.held
        self.assertLess(forward, 0)  # it sets off backward, turning toward the way it walks
        self.assertGreater(abs(bot.yaw), 0)
        walk(3.0)
        self.assertAlmostEqual(abs(bot.yaw), 180)
        self.assertEqual(bot.held, (0, 127))
        bot.health = 0
        last = bot.position
        walk(1.0)
        self.assertEqual(bot.position, last)

    def test_patrol_waits_only_at_its_stops(self):
        bot = TrainingTarget((0, 0, 0), 0, behaviour='patrol', waypoints=((0, 0, 0), (0, 0, 3), (0, 0, 6)),
                             stops=((0, 0, 0), (0, 0, 6)), body=BODIES[0x02E000000000016E])
        for k in range(300):
            bot.patrol(k * 0.016)
            if 2.5 < bot.position[2] < 3.5:  # on through the corner, keys held
                self.assertTrue(bot.keys)
        self.assertEqual(bot.position, (0, 0, 6))
        self.assertAlmostEqual(bot.speed, 6.0)  # the far runners' body

    def test_patrol_tolerates_repeated_waypoints(self):
        bot = TrainingTarget((1, 0, 0), 0, behaviour='patrol', waypoints=((1, 0, 0), (1, 0, 0), (3, 0, 0)))
        for k in range(280):
            bot.patrol(k * 0.016)
        self.assertEqual(bot.position, (3, 0, 0))

    def test_still_bots_neither_move_nor_shoot(self):
        bot = TrainingTarget((0, 0, 3), 180)
        bot.patrol(100)
        bot.patrol(105)
        self.assertEqual(bot.position, (0, 0, 3))
        self.assertEqual([bot.attack_player((0, 0, 0), t) for t in range(100, 110)], [None] * 10)

    @patch('target.random.uniform', return_value=2.0)
    def test_shooter_turns_waits_an_interval_then_keeps_cadence(self, _):
        bot = shooter()
        self.assertIsNone(bot.attack_player((3, 0, 3), 100))
        self.assertAlmostEqual(bot.yaw, 90)
        self.assertIsNone(bot.attack_player((3, 0, 3), 101.99))
        shot = bot.attack_player((3, 0, 3), 102)
        self.assertEqual((shot.origin, shot.target), ((0, 1.2, 3), (3, 1.4, 3)))
        self.assertAlmostEqual(shot.end - shot.start, math.dist(shot.origin, shot.target) / 35)
        self.assertIsNone(bot.attack_player((3, 0, 3), 103))
        self.assertIsNotNone(bot.attack_player((3, 0, 3), 104))

    @patch('target.random.uniform', return_value=2.0)
    def test_shooter_range_resets_its_cadence(self, _):
        bot = shooter()
        self.assertIsNone(bot.attack_player((0, 0, 40), 100))
        self.assertIsNone(bot.next_attack_at)
        bot.attack_player((0, 0, 0), 100)
        bot.attack_player((0, 0, -40), 101)
        self.assertIsNone(bot.attack_player((0, 0, 0), 102))
        self.assertIsNotNone(bot.attack_player((0, 0, 0), 104))

    def test_hero_bot_fires_soldiers_magazine_then_reloads(self):
        from target import REACTION_SECONDS, SOLDIER_RIFLE
        RIFLE_RELOAD_SECONDS, RIFLE_SHOT_SECONDS = SOLDIER_RIFLE.reload_seconds, SOLDIER_RIFLE.shot_seconds
        bot = hero_shooter()
        self.assertEqual(bot.fire((0, 0, 10), 0, 100, aim_error=0), [])
        last = 100 + REACTION_SECONDS + 29 * RIFLE_SHOT_SECONDS
        hits, now = [], 100.0
        while now < last + RIFLE_RELOAD_SECONDS - 0.01:
            now += 1 / 90
            hits += bot.fire((0, 0, 10), 0, now, aim_error=0)
        self.assertEqual(len(hits), 30)
        self.assertAlmostEqual(hits[0], 10 - 0.25, places=1)  # the torso capsule's front
        self.assertEqual(len(bot.fire((0, 0, 10), 0, last + RIFLE_RELOAD_SECONDS + 0.01, aim_error=0)), 1)

    def test_hero_bot_neither_bursts_after_a_pause_nor_shoots_out_of_reach(self):
        bot = hero_shooter()
        bot.fire((0, 0, 10), 0, 100, aim_error=0)
        self.assertLessEqual(len(bot.fire((0, 0, 10), 0, 110, aim_error=0)), 2)
        self.assertEqual(bot.fire((0, 0, 60), 0, 111, aim_error=0), [])
        self.assertIsNone(bot.next_attack_at)

    @patch('target.random.gauss', return_value=0)
    def test_hero_bot_damage_follows_difficulty(self, _):
        for difficulty, damage in (('easy', 5), ('medium', 10), ('hard', 20)):
            match, player = _match()
            match.update(3, 100)
            match.difficulty = difficulty
            bot = hero_shooter()
            match.bots = [bot]
            player.bots_seen.add(bot.entity)
            player.mover.position = (0, 0, 10)
            match._bots_fight(4)
            match._bots_fight(4.5)
            self.assertEqual(player.health, 200 - damage)

    def test_hero_bots_spread_round_the_point_and_hunt_a_fresh_sighting(self):
        import dataclasses
        import nav
        from test_nav import walled_floor
        from ow174.game.match import CHASE_SECONDS, LEASH, SPREAD
        match, player = _match()
        point = (-5, 0, -5)
        match.game_map = dataclasses.replace(match.game_map, map_guid=0x0800000000000FFF, objectives=(point,))
        nav._grids[0xFFF] = nav.build(walled_floor(), [(-8, 0, -8), (8, 0, -8)])
        self.addCleanup(nav._grids.pop, 0xFFF)
        match.bots = [hero_shooter((-8, 0, -8)) for _ in range(6)]
        ends = lambda: [bot.waypoints[-1] for bot in match.bots]
        match._steer_bots(0)
        self.assertEqual(len(set(ends())), 6)
        self.assertTrue(all(math.dist(end, point) < SPREAD + 1.5 for end in ends()))
        match.sightings[1] = ((5, 0, 8), 1)  # past the wall, within the leash
        match._steer_bots(1)
        self.assertTrue(all(math.dist(end, (5, 0, 8)) < SPREAD + 1.5 for end in ends()))
        match._steer_bots(1 + CHASE_SECONDS + 0.1)  # the sighting went stale
        self.assertTrue(all(math.dist(end, point) < SPREAD + 1.5 for end in ends()))
        match.sightings[1] = ((point[0] + LEASH + 1, 0, point[2]), 8)
        match._steer_bots(8)  # too far from the point to leave it for
        self.assertTrue(all(math.dist(end, point) < SPREAD + 1.5 for end in ends()))

    def test_allies_gather_round_the_player_near_the_point(self):
        import dataclasses
        import nav
        from test_nav import walled_floor
        from ow174.game.match import LEASH, SPREAD
        match, player = _match()
        point = (-5, 0, -5)
        match.game_map = dataclasses.replace(match.game_map, map_guid=0x0800000000000FFF, objectives=(point,))
        nav._grids[0xFFF] = nav.build(walled_floor(), [(-8, 0, -8), (8, 0, -8)])
        self.addCleanup(nav._grids.pop, 0xFFF)
        ally, enemy = hero_shooter((-8, 0, -8)), hero_shooter((-8, 0, -8))
        ally.team = player.team
        enemy.team = 1 - player.team
        match.bots = [ally, enemy]
        player.has_body, player.health = True, 200
        player.mover.position = (5, 0, 8)  # past the wall, within the leash
        match._steer_bots(0)
        self.assertLess(math.dist(ally.waypoints[-1], (5, 0, 8)), SPREAD + 1.5)
        self.assertLess(math.dist(enemy.waypoints[-1], point), SPREAD + 1.5)
        player.mover.position = (point[0] + LEASH + 1, 0, point[2])  # too far: the allies play the point
        match._steer_bots(1)
        self.assertLess(math.dist(ally.waypoints[-1], point), SPREAD + 1.5)

    def test_engaged_hero_bots_strafe_across_their_target(self):
        import dataclasses
        import nav
        from test_nav import walled_floor
        from ow174.game.match import STRAFE
        match, player = _match()
        match.game_map = dataclasses.replace(match.game_map, map_guid=0x0800000000000FFF, objectives=((-5, 0, -5),))
        nav._grids[0xFFF] = nav.build(walled_floor(), [(-8, 0, -8), (8, 0, -8)])
        self.addCleanup(nav._grids.pop, 0xFFF)
        bot = hero_shooter((-5, 0, -6))
        bot.target, bot.next_attack_at = player, 1.0
        player.mover.position = (-5, 0, 4)  # straight ahead along +z
        match.bots = [bot]
        match._steer_bots(0)
        (end,) = bot.waypoints
        self.assertAlmostEqual(abs(end[0] - bot.position[0]), STRAFE, delta=nav.SIZE)
        self.assertLess(abs(end[2] - bot.position[2]), nav.SIZE)
        bot.yaw = 0.0  # facing the player along +z
        bot.patrol(1)
        bot.patrol(1.2)
        self.assertNotEqual(bot.position, (-5, 0, -6))  # it walks while it fights
        self.assertEqual(bot.yaw, 0.0)  # sidesteps without turning away from its target

    def test_a_strafe_never_steps_through_a_wall(self):
        import dataclasses
        import nav
        from test_nav import walled_floor
        match, player = _match()
        match.mesh = walled_floor()
        match.game_map = dataclasses.replace(match.game_map, map_guid=0x0800000000000FFF, objectives=((-5, 0, -5),))
        nav._grids[0xFFF] = nav.build(match.mesh, [(-8, 0, -8), (8, 0, -8)])
        self.addCleanup(nav._grids.pop, 0xFFF)
        bot = hero_shooter((-1, 0, -6))  # beside the wall along x=0
        bot.target, bot.next_attack_at = player, 1.0
        player.mover.position = (-1, 0, 4)
        match.bots = [bot]
        with patch('ow174.game.match.random.choice', return_value=-1):  # the sidestep across the wall
            match._steer_bots(0)
        self.assertEqual(bot.waypoints, ())
        with patch('ow174.game.match.random.choice', return_value=1):
            match._steer_bots(1)
        self.assertLess(bot.waypoints[0][0], -2)

    def test_allies_take_their_own_entity_ids_on_the_players_spawns(self):
        from ow174.game.content import Spawn
        from target import hero_bots
        spawns = (Spawn(0, (0, 0, 0), 0), Spawn(1, (9, 0, 0), 90))
        bots = hero_bots(spawns, 1, [(0x02E000000000006E, 0x04000000000003CF, 'Soldier: 76')] * 6) + hero_bots(spawns, 0, [(0x02E000000000006E, 0x04000000000003CF, 'Soldier: 76')] * 5, first=6)
        self.assertEqual(len({bot.entity for bot in bots}), 11)
        self.assertEqual([bot.team for bot in bots], [1] * 6 + [0] * 5)
        self.assertEqual({bot.position for bot in bots[6:]}, {(0, 0, 0)})

    @patch('target.random.gauss', return_value=0)
    def test_bots_shoot_the_nearest_enemy_bot_never_a_teammate(self, _):
        match, player = _match()
        match.update(3, 100)
        enemy, ally, mate = hero_shooter((0, 0, 0)), hero_shooter((0, 0, 10)), hero_shooter((0, 0, 4))
        ally.team = mate.team = 0
        match.bots = [enemy, ally, mate]
        player.mover.position = (0, 0, 40)
        match._bots_fight(4)
        self.assertEqual((enemy.target, ally.target, mate.target), (mate, enemy, enemy))
        match._bots_fight(4.5)
        self.assertEqual((enemy.health, mate.health, ally.health, player.health), (180, 190, 200, 200))

    def test_healers_heal_hurt_allies_in_reach_as_their_kit_says(self):
        from target import HEAL_PERIOD, HEALS
        match, player = _match()
        player.mover.position, player.team = (0, 0, 40), 0
        lucio, zen, ana = hero_shooter((0, 0, 0)), hero_shooter((30, 0, 0)), hero_shooter((-30, 0, 0))
        near, far, worst, enemy = hero_shooter((0, 0, 5)), hero_shooter((0, 0, 20)), hero_shooter((30, 0, 5)), hero_shooter((0, 0, 6))
        lucio.heal, zen.heal, ana.heal = HEALS['Lúcio'], HEALS['Zenyatta'], HEALS['Ana']
        for bot in (lucio, zen, ana, near, far, worst):
            bot.team = 0
        near.health = far.health = enemy.health = lucio.health = 100
        worst.health = 50
        ana.target = enemy  # Ana heals only while she has no one to shoot
        match.bots = [lucio, zen, ana, near, far, worst, enemy]
        match._bots_heal(10)
        match._bots_heal(10.1)  # before the next tick: nothing
        tick = HEAL_PERIOD
        self.assertAlmostEqual(near.health, 100 + 16 * tick)
        self.assertAlmostEqual(lucio.health, 100 + 12 * tick)
        self.assertAlmostEqual(worst.health, 50 + 30 * tick)  # the orb picks the most hurt ally
        self.assertEqual((far.health, enemy.health, player.health), (100, 100, 200))
        ana.target = None
        match._bots_heal(11)
        self.assertAlmostEqual(far.health, 100 + 70 / 0.8 * tick)  # the most hurt in her reach; worst is 60 m off

    @patch('target.random.gauss', return_value=0)
    def test_signature_abilities_change_damage_and_health(self, _):
        from target import ABILITIES, HEAL_PERIOD
        match, player = _match()
        match.update(3, 100)
        player.mover.position = (0, 0, 60)
        hog, zarya, cassidy, reaper, zen, foe = (hero_shooter(at) for at in
            ((0, 0, 0), (10, 0, 0), (20, 0, 0), (30, 0, 0), (35, 0, 0), (0, 0, 5)))
        for bot, name in ((hog, 'Roadhog'), (zarya, 'Zarya'), (cassidy, 'Cassidy'), (reaper, 'Reaper'), (zen, 'Zenyatta')):
            bot.ability = ABILITIES[name]
        foe.team, foe.max_health = 0, 1000
        foe.health = 1000
        match.bots = [hog, zarya, cassidy, reaper, zen, foe]
        for bot in match.bots[:5]:
            bot.target, bot.retarget_at = foe, 99
        foe.target, foe.retarget_at = zarya, 99
        hog.health, zarya.health, reaper.health = 90, 150, 100
        match._bots_fight(4)
        self.assertGreater(hog.effect_until, 4)  # Take a Breather: below half health
        self.assertIsNone(hog.next_attack_at)
        hog.damage(20, 4.1)
        self.assertEqual(hog.health, 80)  # half damage while it mends
        match._bots_heal(4.2)
        self.assertAlmostEqual(hog.health, 80 + 300 * HEAL_PERIOD)
        self.assertEqual((zarya.soak, zarya.effect_until), (200, 6))  # the barrier, hurt in a fight
        zarya.damage(50, 4.5)
        self.assertEqual((zarya.health, zarya.soak), (150, 150))
        self.assertEqual(foe.discorded, (4.4, 0.25))
        self.assertEqual(foe.health, 1000)  # Flashbang reaches 7 m; foe is 20 m from Cassidy
        cassidy.position = (0, 0, 10)
        match._bots_fight(4.3)
        self.assertAlmostEqual(foe.health, 1000 - 25 * 0.5 * 1.25)  # medium damage, discorded
        self.assertEqual(cassidy.ability_ready, 14.3)
        reaper.next_attack_at = 4.0
        before = foe.health
        match._bots_fight(4.31)
        dealt = before - foe.health
        self.assertGreater(dealt, 0)
        self.assertGreater(reaper.health, 100)  # The Reaping heals a share of everything dealt this tick

    def test_attacking_bots_wait_for_the_end_of_setup(self):
        import dataclasses
        import nav
        from test_nav import walled_floor
        from ow174.game.match import ASSAULT, SETUP_SECONDS
        match, player = _match()
        match.game_map = dataclasses.replace(match.game_map, map_guid=0x0800000000000FFF, mode_guid=ASSAULT,
            objectives=((-5, 0, -5),))
        nav._grids[0xFFF] = nav.build(walled_floor(), [(-8, 0, -8), (8, 0, -8)])
        self.addCleanup(nav._grids.pop, 0xFFF)
        attacker, defender = hero_shooter((-8, 0, -8)), hero_shooter((-8, 0, 8))
        attacker.team = 0
        match.bots = [defender, attacker]
        match.assemble_ends = 0.0
        match._steer_bots(1)
        self.assertEqual((bool(defender.waypoints), attacker.waypoints), (True, ()))
        match._steer_bots(SETUP_SECONDS)
        self.assertTrue(attacker.waypoints)

    def test_every_roster_hero_resolves_to_a_body_with_hit_volumes(self):
        import hitbox
        import practice  # noqa: F401
        from ow174.game.content import heroes
        from target import HERO_KIT
        bodies = {hero.name: hero.body for hero in heroes().values()}
        self.assertEqual(len(HERO_KIT), 12)
        for name in HERO_KIT:
            self.assertIn(bodies[name], hitbox.HEROES, name)

    def test_shotgun_pellets_spread_and_fall_off(self):
        import random
        from target import HERO_KIT
        health, scrap = HERO_KIT['Roadhog']
        random.seed(7)
        bot = hero_shooter()
        bot.weapon = scrap
        def hits(distance):
            bot.rounds, bot.next_attack_at, bot.in_flight = scrap.rounds, 100, []
            bot.fire((0, 0, distance), 0, 100, aim_error=0)
            return len(bot.fire((0, 0, distance), 0, 101, aim_error=0))  # the pellets fly 80 m/s
        close, far = sum(hits(6) for _ in range(5)), sum(hits(12) for _ in range(5))
        self.assertTrue(far < close < 5 * scrap.pellets)  # the 20 degree cone loses pellets, more with range
        self.assertEqual((scrap.damage_at(10), scrap.damage_at(40), health), (6.6, 1.98, 600))

    def test_projectiles_land_after_their_flight_and_miss_a_target_that_moved(self):
        from target import HERO_KIT
        bot = hero_shooter()
        bot.weapon = HERO_KIT['Ana'][1]  # 125 m/s
        bot.next_attack_at = 100
        self.assertEqual(bot.fire((0, 0, 25), 0, 100, aim_error=0), [])  # fired, in flight for 0.2 s
        self.assertEqual(bot.fire((0, 0, 25), 0, 100.1, aim_error=0), [])
        self.assertEqual(len(bot.fire((0, 0, 25), 0, 100.25, aim_error=0)), 1)
        bot.next_attack_at = 200
        bot.fire((0, 0, 25), 0, 200, aim_error=0)
        self.assertEqual(bot.fire((3, 0, 25), 0, 200.25, aim_error=0), [])  # stepped 3 m aside: missed
        self.assertEqual(bot.in_flight, [])

    def test_a_pick_without_a_kit_plays_soldier(self):
        from ow174.game import heroselect
        from ow174.game.content import SOLDIER, find_hero
        match, player = _match()
        player.has_body = False  # the first pick
        import ow174.game.match as match_module
        genji = find_hero('Genji').guid
        with patch.object(heroselect, 'picked_hero', return_value=genji), \
                patch.object(match_module, 'KITS', tuple(g for g in match_module.KITS if g != genji)), \
                patch.object(match, 'switch_hero') as switch:
            match.game_message(player, {'+0xA8': heroselect.PICK_HERO, '+0x80': b''}, 5)
        self.assertEqual(switch.call_args[0][1].guid, SOLDIER)

    @patch('target.random.gauss', return_value=0)
    def test_hero_bots_send_their_weapon_to_clients_only_when_it_changes(self, _):
        match, player = _match()
        match.update(3, 100)
        bot = hero_shooter((0, 0, 0))
        bot.hero = 0x02E000000000006E
        match.bots = [bot]
        player.bots_seen.add(bot.entity)
        player.mover.position = (0, 0, 10)
        frames = lambda: [u for batch in player.client.updates for u in batch if u.entity == bot.entity and u.stream]
        match._send_bot_weapons()
        match._send_bot_weapons()
        self.assertEqual(len(frames()), 1)  # held, not firing
        match._bots_fight(4)
        match._bots_fight(4.6)  # past the reaction time: firing, shots counted
        match._send_bot_weapons()
        self.assertEqual(len(frames()), 2)
        self.assertEqual(player.bot_weapons[bot.entity][1][2:4], (True, bot.shots))
        self.assertGreater(bot.shots, 0)
        shots = [u for batch in player.client.updates for u in batch if u.entity == bot.entity and u.shot is not None]
        self.assertEqual(len(shots), 1)  # the shots since the last frame, as one ch2 event
        bot.ability_uses += 1  # a Helix
        match._send_bot_weapons()
        shots = [u for batch in player.client.updates for u in batch if u.entity == bot.entity and u.shot is not None]
        self.assertEqual(len(shots), 2)  # the Helix as its own ch2 event
        self.assertEqual(len(frames()), 3)  # with Volley 16's remote slot on

    @patch('target.random.gauss', return_value=0)
    def test_an_automatic_volley_restarts_with_each_magazine(self, _):
        # The client fires an automatic Volley itself, up to the ammo var per activation (research/bot-fire.md).
        from target import AUTOMATIC
        match, player = _match()
        match.update(3, 100)
        bot = hero_shooter((0, 0, 0))
        bot.hero = 0x02E000000000006E  # Soldier: 76
        bot.weapon = dataclasses.replace(bot.weapon, damage=0, far_damage=0)  # the player outlives a magazine
        self.assertIn('Soldier: 76', AUTOMATIC)
        match.bots = [bot]
        player.bots_seen.add(bot.entity)
        player.mover.position = (0, 0, 10)
        firing = lambda: player.bot_weapons[bot.entity][1][2]
        match._bots_fight(4)
        match._bots_fight(4.6)
        match._send_bot_weapons()
        self.assertTrue(firing())
        now = 4.6
        while bot.rounds and now < 20:  # the magazine runs dry
            now += 0.05
            match._bots_fight(now)
        self.assertEqual(bot.rounds, 0)
        match._send_bot_weapons()
        self.assertFalse(firing())  # reloading: the Volley ends
        match._bots_fight(now + bot.weapon.reload_seconds + 0.1)
        match._send_bot_weapons()
        self.assertTrue(firing())  # the next magazine starts it again
        bot.health = 0
        match._send_bot_weapons()
        self.assertFalse(firing())  # dead: off before the respawn destroys the body

    def test_a_fighting_bot_aims_at_its_targets_chest(self):
        bot = hero_shooter((0, 0, 0))
        bot._face((0, 0, 10), True, 30)
        eye = eye_height(bot.body)
        self.assertAlmostEqual(bot.pitch, math.degrees(math.atan2(eye - 1.4, 10)))
        bot._face((0, 0, 10), False, 30)
        self.assertEqual(bot.pitch, 0.0)

    def test_a_shot_event_reads_back_as_the_client_parses_it(self):
        import practice  # noqa: F401  (puts vendor/ on the path)
        from ow174.game import world
        from ow174.game.bits import BitReader, BitWriter
        out = BitWriter()
        world._write_entity(out, world.EntityUpdate(0xB0000201, shot=world.shot_record(70, 1, 11, 70, (1.0, 2.0, 3.0),
                                                                                         (0, 0, 5))))
        r = BitReader(out.getvalue())
        self.assertEqual(r.entity_id(), 0xB0000201)
        self.assertEqual([r.bit() for _ in range(6)], [1, 1, 0, 1, 1, 1])  # ch0, ch1 without a chunk, ch2 events
        self.assertEqual(r.var_b(), 0)
        u4 = lambda: r.bits(4) if not r.bit() else r.bits(8)
        u16 = lambda: r.bits(4) if not r.bit() else r.bits(16 if r.bit() else 8)
        self.assertEqual(u4(), 70)
        self.assertEqual((r.bit(), r.bit()), (0, 0))  # no +8 word, keyed by state
        self.assertEqual((u16(), u16(), r.bits(6), r.bit(), u16()), (1, 11, 70 & 63, 1, 1))
        origin = [struct.unpack('<f', struct.pack('<I', r.bits(32)))[0] for _ in range(3)]
        self.assertEqual((origin, r.bits(16), r.bits(16)), ([1.0, 2.0, 3.0], 0x8000, 0x8000))  # straight ahead
        self.assertEqual([r.bit() for _ in range(4)], [0, 0, 0, 0])  # no entity list, no more events, no state, ch3

    def test_a_shot_record_aims_where_the_client_reads_it(self):
        import practice  # noqa: F401  (puts vendor/ on the path)
        from ow174.game import world
        from ow174.game.bits import BitReader
        direction = (0.48, 0.6, 0.64)  # up and to the side
        r = BitReader(world.shot_record(0, 1, 11, 0, (0.0, 0.0, 0.0), direction).getvalue())
        r.bits(5 + 2 + 5 + 5 + 6 + 1 + 5 + 96)  # shot, flags, instance, state, event, activation, key instance, origin
        # the client's direction from the record's angles (0x7FF6D1FD1360)
        yaw, pitch = (r.bits(16) - 0x8000) * math.pi / 32768, (r.bits(16) - 0x8000) * math.pi / 65536
        read = (math.sin(yaw) * math.cos(pitch), math.sin(pitch), math.cos(yaw) * math.cos(pitch))
        for got, want in zip(read, direction):
            self.assertAlmostEqual(got, want, places=3)

    def test_every_roster_hero_has_a_remote_volley_frame(self):
        from target import HERO_KIT, WEAPON_EFFECTS, WEAPON_GRAPHS, WEAPON_MODELS, WEAPON_SHOTS
        from ow174.game.match import remote_volley_frame
        # Zarya's Volley is her alternate fire; D.Va's mech is for the viewer mannequin (no roster D.Va)
        self.assertEqual(set(WEAPON_GRAPHS) - {'D.Va'}, set(HERO_KIT) - {'Zarya'})
        import oracle
        for hero, (layout, state, slot, ammo_bit) in WEAPON_GRAPHS.items():
            self.assertLess(ammo_bit, layout[2])
            graph = oracle._graph(layout[0])
            for healer in (None, (0xA0000102, False), (0xA0000102, True)):
                with self.subTest(hero=hero, healer=healer):
                    # the client's reader: the Volley and the ammo where it reads them, and a status after them
                    frame = oracle.read_full(remote_volley_frame(hero, 30, True, 70, 3, healer=healer))
                    weapon = frame.instances[1]
                    self.assertEqual(weapon.states[graph.remote_node(slot).state][0], True)
                    self.assertEqual(weapon.vars[graph.presence_vars()[ammo_bit].var][0], 30)
                    self.assertEqual(len(frame.instances), 1 + (healer is not None))
                    if hero in WEAPON_MODELS:  # var49, in the scope the graph gives it, bound to the gun's CosmeticEntity
                        slot, _, bit = WEAPON_MODELS[hero]
                        model = graph.remote_node(slot)
                        self.assertEqual(model.cls, 'STUStatescriptStateCosmeticEntity')
                        self.assertEqual(model.fields['m_1DEF59FA']['m_60DB8F99'], int(bit is None))
                        (binding,) = (frame.entity_vars if bit is None else weapon.vars)[49][1]
                        self.assertEqual(binding.state, model.state)
                    for effect in WEAPON_EFFECTS.get(hero, ()):  # begun, and only on a bound gun
                        self.assertIn(hero, WEAPON_MODELS)
                        node = graph.remote_node(effect)
                        self.assertEqual(node.cls, 'STU_691BFA55')
                        self.assertEqual(weapon.states[node.state][0], True)
            if hero in WEAPON_SHOTS:  # the primary Volley's own projectile
                projectile = graph.state(state).fields['m_projectileEntity']['m_entityDef']['m_entityDef']
                self.assertEqual(int(projectile, 16), WEAPON_SHOTS[hero])

    def test_the_shot_modes_read_live_follow_the_volleys_authored_sync(self):
        # LizRollin's rule (projectiles._sync, "as the PS4 build resolves it"): m_5E4A4E85 if set, else 3 for 2+ shots a
        # volley at 5.5/s or more, 2 without a lifetime or with more than 7 projectiles a shot, else 1. A dynamic value
        # counts as 0: Sombra's rate is var 8421, and her mode reads 2 live (research/bot-fire.md).
        from target import AUTOMATIC, WEAPON_GRAPHS, WEAPON_SHOTS
        import oracle

        def mode(node):
            def number(name, default=0.0):
                value = node.fields.get(name)
                return value.get('m_value', 0.0) if isinstance(value, dict) else default if value is None else value
            if number('m_5E4A4E85'):
                return number('m_5E4A4E85')
            if number('m_A17BE89B', 1) >= 2 and number('m_numShotsPerSecond') >= 5.5:
                return 3
            return 2 if number('m_projectileLifetime') < 1.19e-5 or number('m_numProjectilesPerShot', 1) > 7 else 1

        for hero, (layout, state, _, _) in WEAPON_GRAPHS.items():
            if hero == 'D.Va':
                continue  # her mech's graph comes from graph_record.py, which keeps no Volley config
            with self.subTest(hero=hero):
                found = mode(oracle._graph(layout[0]).state(state))
                self.assertEqual((found == 3, found == 1), (hero in AUTOMATIC, hero in WEAPON_SHOTS))

    def _mode_1_shot(self, hero):
        """A match whose one `hero` bot has just fired a shot at the player 20 m away: the match, the player and the
        records the shot sent, after the Volley's frame."""
        from target import HERO_KIT
        from ow174.game.content import heroes
        match, player = _match()
        bot = hero_shooter((0, 0, 0))
        bot.hero = next(guid for guid, found in heroes().items() if found.name == hero)
        bot.weapon = HERO_KIT[hero][1]
        match.bots = [bot]
        player.bots_seen.add(bot.entity)
        player.mover.position = (0, 0, 20)
        bot.next_attack_at, bot.target = 0.0, player
        match._send_bot_weapons()
        bot.shots += 1
        player.client.updates.clear()
        match.now = 1.0
        match._send_bot_weapons()
        updates = [u for batch in player.client.updates for u in batch][1:]
        player.client.updates.clear()
        return match, player, bot, updates

    def test_a_mode_1_bot_shot_names_a_projectile_made_for_it_until_it_lands(self):
        import math
        from target import HERO_KIT, chest
        from ow174.game.match import SHOT_LINGER
        from ow174.game.world import OP_CREATE, OP_DESTROY
        match, player, bot, (create, record, visual) = self._mode_1_shot('Ana')
        self.assertEqual((create.op, record.entity, visual.entity), (OP_CREATE, bot.entity, create.entity))
        self.assertIsNotNone(record.shot)
        self.assertIsNotNone(visual.projectile)
        flight = math.dist(chest((0, 0, 0), 1.2), chest((0, 0, 20))) / HERO_KIT['Ana'][1].speed
        player.client.updates.clear()
        match.now = 1.0 + flight / 2
        match._send_bot_weapons()
        self.assertEqual([u.op for batch in player.client.updates for u in batch], [])  # still flying
        match.now = 1.0 + flight
        match._send_bot_weapons()
        (stop,) = [u for batch in player.client.updates for u in batch]  # stopped at the chest, not yet destroyed
        self.assertEqual((stop.entity, stop.op), (create.entity, 0))
        self.assertIsNotNone(stop.projectile)
        player.client.updates.clear()
        match.now += SHOT_LINGER
        match._send_bot_weapons()
        self.assertEqual([(u.entity, u.op) for batch in player.client.updates for u in batch], [(create.entity, OP_DESTROY)])

    def test_a_hitscan_mode_1_shot_is_drawn_by_its_record_alone(self):
        # Cassidy's tracer draws from the record naming the projectile; no snapshot, no stop (run 31)
        from ow174.game.match import SHOT_LINGER
        from ow174.game.world import OP_CREATE, OP_DESTROY
        match, player, bot, (create, record) = self._mode_1_shot('Cassidy')
        self.assertEqual((create.op, record.entity), (OP_CREATE, bot.entity))
        self.assertIsNotNone(record.shot)
        self.assertIsNone(record.projectile)
        match.now = 1.0 + SHOT_LINGER - 0.01
        match._send_bot_weapons()
        self.assertEqual([u.op for batch in player.client.updates for u in batch], [])
        match.now = 1.0 + SHOT_LINGER
        match._send_bot_weapons()
        self.assertEqual([(u.entity, u.op) for batch in player.client.updates for u in batch], [(create.entity, OP_DESTROY)])

    def test_beam_heroes_hold_fire_beyond_their_reach(self):
        from target import HERO_KIT
        bot = hero_shooter()
        bot.weapon = HERO_KIT['Mei'][1]
        self.assertEqual(bot.fire((0, 0, 12), 0, 100, aim_error=0), [])
        self.assertIsNone(bot.next_attack_at)

    def test_engaged_range_shooter_holds_and_a_respawned_hero_bot_drops_its_route(self):
        shooter = TrainingTarget((0, 0, 0), 0, behaviour='shoot', waypoints=((0, 0, 5),))
        shooter.fire((0, 0, 10), 0, 100, aim_error=0)
        shooter.patrol(100)
        shooter.patrol(101)
        self.assertEqual(shooter.position, (0, 0, 0))
        bot = hero_shooter()
        bot.waypoints, bot.goal = ((0, 0, 5),), (0, 0, 5)
        bot.damage(200, 101)
        bot.tick(104)
        self.assertEqual((bot.waypoints, bot.goal), ((), None))

    def test_dead_target_cannot_attack(self):
        bot = shooter()
        bot.health = 0
        self.assertIsNone(bot.attack_player((0, 0, 0), 101))

    def test_range_bot_body_stays_dead_then_revives_where_it_fell(self):
        bot = TrainingTarget((0, 0, 3), 180, behaviour='patrol', waypoints=((0, 0, 3), (0, 0, 9)), entity=0xB0000301)
        self.assertIsNone(bot.hit((0, 1.6, 0), (1, 0, 0), 100))
        for _ in range(10):
            self.assertIsNotNone(bot.hit((0, 1.6, 0), (0, 0, 1), 100))
        self.assertEqual(bot.health, 0)
        self.assertIsNone(bot.hit((0, 1.6, 0), (0, 0, 1), 100))
        bot.position, bot.leg = (0, 0, 6), 1
        self.assertIsNone(bot.tick(100.2))  # the client ragdolls the body it still has
        self.assertIsNone(bot.tick(102.9))
        self.assertFalse(bot.rebuilding(102.9))
        self.assertEqual(bot.tick(103), ('revive', 0xB0000301))
        self.assertEqual((bot.entity, bot.health, bot.position, bot.leg), (0xB0000301, 200, (0, 0, 6), 1))
        self.assertTrue(bot.rebuilding(105.6))
        self.assertFalse(bot.rebuilding(105.7))

    def test_hero_bot_respawns_as_a_fresh_body_at_home_and_the_low_byte_wraps(self):
        bot = TrainingTarget((0, 0, 3), 180, behaviour='shoot', entity=0xB00003FF, hero=1)
        bot.damage(500, 100)
        bot.position = (5, 0, 5)
        self.assertIsNone(bot.tick(102.9))
        self.assertEqual(bot.tick(103), ('respawn', 0xB00003FF))
        self.assertEqual((bot.entity, bot.health, bot.position, bot.yaw), (0xB0000301, 200, (0, 0, 3), 180))
        self.assertFalse(bot.rebuilding(103))

    def test_revived_range_bot_runs_its_reassembly_anim_for_the_clip(self):
        import ow174.game.match as M
        from target import REBUILD, REBUILD_SECONDS, REBUILD_SLOT
        match, player = _match()
        bot = TrainingTarget((0, 0, 8), 180)
        match.bots = [bot]
        match._target_updates(player)
        with patch.object(M, 'remote_full_frame', wraps=M.remote_full_frame) as frame:
            match._send_range_bot_frames(100)  # never died: nothing to show
            bot.damage(200, 100)
            bot.tick(103)
            match._send_range_bot_frames(103)
            match._send_range_bot_frames(104)  # unchanged
            match._send_range_bot_frames(103 + REBUILD_SECONDS)
        (on, _), (off, _) = [call.args[0] for call in frame.call_args_list]
        self.assertEqual((on.graph, list(on.active)), (REBUILD, [REBUILD_SLOT]))
        self.assertEqual((off.graph, off.active), (REBUILD, {}))

    def test_range_bot_death_pulses_its_kill_feed_line_once(self):
        import oracle
        import ow174.game.match as M
        from ow174.game.script import expr
        from target import KILL_FEED, kill_feed
        match, player = _match()
        bot = TrainingTarget((0, 0, 8), 180)
        match.bots = [bot]
        match._target_updates(player)
        with patch.object(M, 'remote_full_frame', wraps=M.remote_full_frame) as frame:
            match._damage_bot(player, bot, 200, 100)
            match._send_range_bot_frames(100)  # the death: a fresh pulse that names the killer
            match._send_range_bot_frames(101)  # unchanged
            bot.tick(103)
            match._send_range_bot_frames(103)  # revived: the count the client has seen, so no second line
            match._damage_bot(player, bot, 200, 110)
            match._send_range_bot_frames(110)  # the next death moves the count on
        graph = oracle._graph(KILL_FEED[0].index)
        pulser = graph.remote_node(KILL_FEED[1]).state
        self.assertEqual((pulser, graph.node(graph.remote_node(KILL_FEED[1]).pos).cls), (8, 'STU_B9898052'))
        read = [oracle.read_full(M.remote_full_frame(*call.args)).instances[2] for call in frame.call_args_list]
        self.assertEqual([feed.states[pulser] for feed in read], [
            (True, {'fresh': True, 'count': 1}), (True, {'fresh': False, 'count': 1}), (True, {'fresh': True, 'count': 2})])
        self.assertEqual({feed.vars[392][0] for feed in read}, {expr.Entity(player.body)})  # the client's #392: the line's killer
        unknown = oracle.read_full(M.remote_full_frame([kill_feed(2, False, None, False)], {})).instances[2]
        self.assertEqual((unknown.vars, unknown.states[pulser]), ({}, (True, {'fresh': False, 'count': 1})))

    def test_dead_hero_bot_pulses_its_kill_feed_line(self):
        import oracle
        from ow174.game.match import remote_volley_frame
        from ow174.game.script import expr
        match, player = _match()
        bot = hero_shooter((0, 0, 0))
        bot.hero = 0x02E000000000006E
        match.bots = [bot]
        player.bots_seen.add(bot.entity)
        frames = lambda: [u for batch in player.client.updates for u in batch if u.entity == bot.entity and u.stream]
        match._send_bot_weapons()
        self.assertIsNone(player.bot_weapons[bot.entity][1][6])  # alive: no line
        match._damage_bot(player, bot, 500, 100)
        match._send_bot_weapons()
        match._send_bot_weapons()  # unchanged
        self.assertEqual((len(frames()), player.bot_weapons[bot.entity][1][6]), (2, (player.body,)))
        hud = oracle.read_full(remote_volley_frame('Soldier: 76', 30, False, 70, killed=(player.body,))).instances[3]
        self.assertEqual((hud.graph, hud.states[108]), (0x01CF, (True, {'fresh': True, 'count': 1})))
        self.assertEqual(hud.vars[392][0], expr.Entity(player.body))  # the client's #392: the line's killer

    def test_match_creates_every_bot_with_its_body_and_team(self):
        from ow174.game import world
        from ow174.game.match import TEAM_BIT
        match, player = _match()
        with patch.object(world, 'create') as create:
            for update in match._target_updates(player):
                update.build(0)
        bodies = [call for call in create.call_args_list if call.args[1] in BODIES.values()]
        self.assertEqual(len(bodies), 15)
        self.assertEqual(len(create.call_args_list), 30)
        for call, bot in zip(bodies, match.bots):
            self.assertEqual(call.args[1], bot.body)
            self.assertEqual(set(call.args[2]), {26, 51})
            self.assertEqual(call.args[2][26], [TEAM_BIT << bot.team])
            self.assertEqual(call.kwargs['position'], bot.position)
        self.assertEqual(match._target_updates(player), [])

    def test_create_record_keeps_queued_health_snapshot(self):
        match, player = _match()
        update = next(u for u in match._target_updates(player) if u.entity == match.bots[0].entity)
        before = update.build(0).getvalue()
        match.bots[0].health = 20
        self.assertEqual(update.build(0).getvalue(), before)

    def test_movements_start_after_create_and_follow_patrol(self):
        match, player = _match()
        match._target_updates(player)
        self.assertEqual(match.remote_movements(player), [])
        self.assertEqual(len(match.remote_movements(player)), 15)
        walker = next(bot for bot in match.bots if bot.behaviour == 'patrol')
        match.update(10, 1)
        idle = {update.entity: update.movement for update in match.remote_movements(player, 1000)}[walker.entity]
        self.assertEqual((idle.throttles, idle.input_frame), ((0, 0), NO_INPUT))
        match.update(13.5, 2)
        match.update(14, 3)
        moved = {update.entity: update.movement for update in match.remote_movements(player, 1000)}
        self.assertTrue(any(moved[walker.entity].velocity))
        self.assertEqual((moved[walker.entity].throttles, moved[walker.entity].input_frame), (walker.held, None))
        self.assertNotEqual(walker.held, (0, 0))
        self.assertEqual(moved[walker.entity].gravity, 1.75)
        walker.held, walker.held_at = (0, 0), match.started + 0.5  # keys let go: +36 names that frame
        record = {u.entity: u.movement for u in match.remote_movements(player, 2000)}[walker.entity]
        self.assertEqual((record.input_frame, record.input_throttles), (player.frame_at(walker.held_at), walker.last_held))
        from test_owner_correction import read  # LizRollin's reader: the client's field order, every bit read
        walking, released = read(moved[walker.entity], 1000), read(record, 2000)
        self.assertEqual((walking['throttles'], walking['input_frame'], walking['gravity']), (moved[walker.entity].throttles, 1000, 1.75))
        self.assertEqual((released['throttles'], released['input_frame'], released['input_throttles']),
                         ((0, 0), player.frame_at(walker.held_at), walker.last_held))
        walker.health = 0
        self.assertNotIn(walker.entity, {u.entity for u in match.remote_movements(player)})

    def test_weapons_that_cannot_headshot_neither_double_nor_flag_it(self):
        from ow174.game.commands import Command
        from target import ALT_KIT, MERCY_BLASTER_WEAPON
        match, player = _match()
        match._target_updates(player)
        bot = next(b for b in match.bots if b.team != player.team)
        match.bots = [bot]
        with patch.object(bot, 'ray', return_value=(5.0, True)),                 patch('ow174.game.match.time.time', return_value=100):
            match.fire_at_target(player, Command(1, yaw=0, pitch=0), ALT_KIT['Cassidy'][0])  # Fan the Hammer
            fan = player.client.messages[-1][1]['+0x78']
            match.fire_at_target(player, Command(2, yaw=0, pitch=0), MERCY_BLASTER_WEAPON)
            match._tick_shots(match.now + 1)  # a projectile: it lands when it has flown there
            blaster = player.client.messages[-1][1]['+0x78']
        self.assertEqual((fan['+0x4'], fan['+0x8'], blaster['+0x4'], blaster['+0x8']), (50, 0, 40, 1))

    def test_a_projectile_shot_flies_to_its_target_and_misses_one_that_moved(self):
        from ow174.game.commands import Command
        from target import PLAYER_KIT
        match, player = _match()
        match._target_updates(player)
        bot = next(b for b in match.bots if b.team != player.team)
        match.bots = [bot]
        player.mover.position = (0.0, 0.5, 0.0)
        bot.position = (0, 0.6, 14)
        rocket = PLAYER_KIT['Pharah'][1]
        self.assertEqual((rocket.speed, rocket.damage), (35, 120))
        match.fire_at_target(player, Command(1, yaw=0, pitch=0), rocket)
        match._tick_shots(match.now + 0.2)  # 7 m out: not there yet
        self.assertEqual((bot.health, len(match.shots)), (200, 1))
        match._tick_shots(match.now + 0.5)
        self.assertEqual((bot.health, match.shots), (80, []))
        match.fire_at_target(player, Command(2, yaw=0, pitch=0), rocket)
        bot.position = (6, 0.6, 14)  # it stepped aside while the rocket flew
        match._tick_shots(match.now + 0.5)
        match._tick_shots(match.now + 5)  # past its reach: gone
        self.assertEqual((bot.health, match.shots), (80, []))

    def test_headshots_double_and_range_falls_off(self):
        from ow174.game.commands import Command
        match, player = _match()
        match._target_updates(player)
        head, body = [b for b in match.bots if b.team != player.team][:2]
        match.bots = [head, body]
        player.mover.position = (0.0, 0.5, 0.0)
        head.position, body.position = (0, 0, 7), (5, 0.6, 40)  # eye at 2.1: hovering head; raised bot: body
        with patch('ow174.game.match.time.time', return_value=100):
            match.fire_at_target(player, Command(1, yaw=0, pitch=0))
            match.fire_at_target(player, Command(2, yaw=int(math.atan2(5, 40) * 32768 / math.pi), pitch=0))
        self.assertEqual((head.health, body.health), (160, 200 - 13))  # 20 x2; 20 to 6 over 30-50 m

    def test_bot_shot_needs_the_players_hit_volumes(self):
        from target import Projectile
        shot = Projectile((0, 1.2, 10), (0, 1.4, 0), 0)
        self.assertTrue(shot.hits((0, 0, 0)))
        self.assertFalse(shot.hits((0.5, 0, 0)))  # a sidestep clears Soldier's chest

    def test_shot_damages_only_the_nearest_bot(self):
        from ow174.game.commands import Command
        match, player = _match()
        match._target_updates(player)
        friend, near, far = next(b for b in match.bots if b.team == player.team), \
            *[b for b in match.bots if b.team != player.team][:2]
        player.mover.position = (0.0, 0.0, 0.0)
        friend.position, near.position, far.position = (0, 0.6, 5), (0, 0.6, 7), (0, 0.6, 10)
        with patch('ow174.game.match.time.time', return_value=100):
            match.fire_at_target(player, Command(1, yaw=0, pitch=0))
        self.assertEqual((friend.health, near.health, far.health), (200, 180, 200))  # no friendly fire

    @patch('target.random.uniform', return_value=2.0)
    def test_hostile_shooter_kills_player_who_respawns_fresh(self, _):
        match, player = _match()
        match.update(3, 100)  # body script and bots
        self.assertEqual(len(player.bots_seen), 15)
        shooter_bot = next(bot for bot in match.bots if bot.behaviour == 'shoot' and bot.team == 1)
        friendly = next(bot for bot in match.bots if bot.behaviour == 'shoot' and bot.team == 0)
        x, y, z = shooter_bot.position
        player.mover.position = (x, y, z + 5)
        others = [bot for bot in match.bots if bot.behaviour == 'shoot' and bot not in (shooter_bot, friendly)]
        for bot in others:
            bot.health = 0
        now = 4.0
        while player.health and now < 100:
            match.update(now, int(now * 10))
            now += 0.5
        self.assertEqual(player.health, 0)
        self.assertAlmostEqual(now - 0.5, 4 + 2 * 10 + 0.5)  # shots every 2 s from 6, each landing an update later
        self.assertEqual(player.respawn_at, now - 0.5 + 3)
        old_body = player.body
        with patch('ow174.game.match.time.time', return_value=player.respawn_at):
            match.update(player.respawn_at, 999)
        self.assertNotEqual(player.body, old_body)
        self.assertEqual(player.health, 200)
        self.assertIsNone(player.respawn_at)
        self.assertTrue(any(update.entity == old_body and update.op == 3
            for batch in player.client.updates for update in batch))

    @patch('target.random.uniform', return_value=2.0)
    def test_projectile_flies_as_recreated_entities_and_can_be_dodged(self, _):
        match, player = _match()
        match.update(3, 100)
        shooter_bot = next(bot for bot in match.bots if bot.behaviour == 'shoot' and bot.team == 1)
        for bot in match.bots:
            bot.health = bot.health if bot is shooter_bot else 0
        x, y, z = shooter_bot.position
        player.mover.position = (x, y, z + 20)
        match.update(4, 101)
        match.update(6, 102)  # fires; 20 m at 35 m/s lands at about 6.57
        (_, shot), = match.projectiles
        first = [u for u in match.remote_movements(player) if u.entity >> 24 == 0xB1]
        self.assertEqual([(u.op, u.entity & 0xFF) for u in first], [(1, 1)])
        match.update(6.3, 103)
        steps = [u for u in match.remote_movements(player) if u.entity >> 24 == 0xB1]
        self.assertEqual([(u.op, u.entity) for u in steps], [(3, first[0].entity), (1, first[0].entity + 1)])
        self.assertFalse(steps[1].resend)
        player.mover.position = (x + 2, y, z + 20)  # sidestep 2 m before it lands
        match.update(6.6, 104)
        self.assertTrue(shot.landed)
        self.assertEqual(player.health, 200)
        last = [u for u in match.remote_movements(player) if u.entity >> 24 == 0xB1]
        self.assertEqual([(u.op, u.entity) for u in last], [(3, first[0].entity + 1)])
        self.assertEqual(match.projectiles, [])

    def test_friendly_shooter_never_damages_its_team(self):
        match, player = _match()
        match.update(3, 100)
        friendly = next(bot for bot in match.bots if bot.behaviour == 'shoot' and bot.team == 0)
        for bot in match.bots:
            bot.health = bot.health if bot is friendly else 0
        x, y, z = friendly.position
        player.mover.position = (x, y, z + 3)
        for now in range(4, 30):
            match.update(now, now)
        self.assertEqual(player.health, 200)


if __name__ == '__main__':
    unittest.main()
