"""Abilities that move the hero on a kits body: the timed states a frame carries, the mods the server's mover takes
from them, and the rules while one runs (research/lizrollin-runtime.md section 23)."""
import math
import unittest

import practice  # noqa: F401  (puts vendor/ on the path)
import kits
from kits import (ASHE, BAPTISTE, CASSIDY, DOOMFIST, ECHO, GENJI, HANZO, JUNKRAT, MEI, MERCY, MOIRA, PHARAH, REAPER, REINHARDT, SOMBRA, TRACER, WINSTON, WRECKING_BALL, Timed,
                  ability_instance, phased)
from ow174.game.commands import FIRE, Command
from ow174.game.content import heroes
from ow174.game.mover import ADD, COLLISION_MOD, JUMP_SPEED_MOD, SET, SPEED
from player_mover import PlayerMover, frame_mods
from test_roster import equipped

DT, QUANTA = 0.007, 7000
SHIFT = 0x08


def at(hero, index, seconds):
    """(the hero's ability, its Timed, its instance) `seconds` after the press that started it."""
    ability = kits.ABILITIES[hero][index]
    timed = Timed(QUANTA, ability.button, ability.seconds, *ability.timed, ability=ability)
    timed.command(Command(1000, buttons=ability.button))
    timed.command(Command(1000 + round(seconds / DT)))
    return ability, timed, ability_instance(ability, timed)


def mods(hero, index, seconds):
    return set(frame_mods([at(hero, index, seconds)[2]]).values())


def walk(mover, seconds, start, forward=127):
    for frame in range(start, start + round(seconds / DT)):
        mover.step(Command(frame, forward=forward), DT)
    return start + round(seconds / DT)


class TimedStatesTests(unittest.TestCase):
    def test_a_timed_state_is_on_only_between_its_times(self):
        # Wraith Form: the MovementMod (4) for 3 s, the client's cancel buttons (24, 25) from 0.4 s, the recovery
        # Wait (6) after it
        for seconds, on, off in ((0.2, {0, 4, 26}, {24, 25, 6}), (1.0, {0, 4, 24, 25}, {26, 6}), (3.2, {6, 10}, {0, 4, 24})):
            active = at(REAPER, 0, seconds)[2].active
            self.assertTrue(on <= set(active), seconds)
            self.assertFalse(off & set(active), seconds)

    def test_the_instances_are_built_anew_only_when_a_state_changes(self):
        ability = kits.ABILITIES[REAPER][0]
        timed = Timed(QUANTA, ability.button, ability.seconds, *ability.timed, ability=ability)
        timed.command(Command(1000, buttons=SHIFT))
        changes = []
        for frame in range(1001, 1001 + round(3.6 / DT)):
            timed.command(Command(frame))
            if timed.active and timed.due():
                changes.append(round((frame - 1000) * DT, 1))
        self.assertEqual(changes, [0.4, 2.5, 3.0])  # 3.4 and 3.5 are past its end: the cooldown's frame

    def test_a_press_of_its_button_or_of_cancel_on_ends_it_after_the_cancel_time(self):
        for button, frames, ended in ((FIRE, 30, False), (FIRE, 80, True), (SHIFT, 80, True), (0x10, 80, False)):
            ability = kits.ABILITIES[REAPER][0]
            timed = Timed(QUANTA, ability.button, ability.seconds, *ability.timed, ability=ability)
            timed.command(Command(1000, buttons=SHIFT))
            timed.command(Command(1001))
            timed.command(Command(1000 + frames, buttons=button))
            self.assertEqual(not timed.active, ended, (button, frames))
            self.assertEqual(timed.ready_at > 0, ended)  # the cooldown starts when it ends

    def test_stealth_lasts_until_it_is_ended(self):
        _, timed, instance = at(SOMBRA, 1, 600.0)
        self.assertTrue(timed.active)
        self.assertIn(2, instance.active)  # its MovementMod


class FrameModsTests(unittest.TestCase):
    def test_a_frames_movement_states_give_the_graphs_own_mods(self):
        self.assertEqual(mods(REAPER, 0, 1.0), {(SPEED, ADD, 0.5, 0.0), (COLLISION_MOD, SET, 0.0, 0.0)})
        self.assertEqual(mods(REAPER, 0, 3.2), set())  # the recovery: none
        self.assertIn((JUMP_SPEED_MOD, ADD, 6.0, 0.0), mods(MOIRA, 0, 0.3))
        self.assertIn((SPEED, ADD, 2.5, 0.0), mods(MOIRA, 0, 0.3))

    def test_a_synced_variable_is_what_the_frame_sends_for_it(self):
        # Stealth's speed is var7779, a synced variable of 0C9B: unsent, the live client's mod added nothing
        self.assertEqual(mods(SOMBRA, 1, 0.3), set())  # still fading out
        ability, _, instance = at(SOMBRA, 1, 2.0)
        self.assertEqual(ability.extra, (kits.STEALTH_SPEED,))
        (index, op, value, _), = frame_mods([instance]).values()
        self.assertEqual((index, op), (SPEED, ADD))
        self.assertAlmostEqual(value, 0.65, places=6)
        del instance.presence[kits.STEALTH_SPEED[0]]
        self.assertEqual(frame_mods([instance]), {})

    def test_the_mover_takes_the_mods_from_its_next_step(self):
        mover = PlayerMover((0.0, 0.0, 0.0), 0, None, heroes()[REAPER].body)
        frame = walk(mover, 1.0, 1)
        self.assertAlmostEqual(math.hypot(mover.velocity[0], mover.velocity[2]), 5.5, places=2)
        mover.ability_mods = frame_mods([at(REAPER, 0, 1.0)[2]])
        frame = walk(mover, 1.0, frame)
        self.assertAlmostEqual(math.hypot(mover.velocity[0], mover.velocity[2]), 8.25, places=2)
        mover.ability_mods = {}
        walk(mover, 1.0, frame)
        self.assertAlmostEqual(math.hypot(mover.velocity[0], mover.velocity[2]), 5.5, places=2)

    def test_throttle_mods_root_the_mover_or_force_it_forward(self):
        mover = PlayerMover((0.0, 0.0, 0.0), 0, None, heroes()[MEI].body)
        mover.ability_mods = frame_mods([at(MEI, 0, 1.0)[2]])  # Cryo-Freeze: all three throttles 0
        walk(mover, 1.0, 1)
        self.assertEqual(tuple(mover.position), (0.0, 0.0, 0.0))
        charger = PlayerMover((0.0, 0.0, 0.0), 0, None, heroes()[REINHARDT].body)
        charger.ability_mods = frame_mods([at(REINHARDT, 0, 0.3)[2]])  # Charge's wind-up: rooted
        frame = walk(charger, 0.3, 1)
        self.assertEqual(tuple(charger.position), (0.0, 0.0, 0.0))
        charger.ability_mods = frame_mods([at(REINHARDT, 0, 2.0)[2]])  # then forward whatever the input, SPEED +2
        walk(charger, 2.0, frame, forward=-127)
        self.assertAlmostEqual(math.hypot(charger.velocity[0], charger.velocity[2]), 16.5, places=1)
        self.assertGreater(charger.position[2], 20.0)  # along its facing, against the held S


class PhasedTests(unittest.TestCase):
    def test_wraith_form_fade_and_cryo_freeze_are_phased_while_their_shared_state_is_on(self):
        for hero, during, after in ((REAPER, 1.0, 3.2), (MOIRA, 0.3, 0.8), (MEI, 1.0, 4.1)):
            self.assertTrue(phased([at(hero, 0, during)[2]]), hero)
            self.assertFalse(phased([at(hero, 0, after)[2]]), hero)
        self.assertFalse(phased([at(REINHARDT, 0, 2.0)[2]]))

    def test_nothing_hurts_or_leaves_reaper_in_wraith_form_and_his_copy_runs_faster(self):
        match, player = equipped(REAPER)
        player.take_commands([Command(100, buttons=SHIFT), Command(101)])
        match.update(3.1, 2)  # the body frame that carries Wraith Form's states
        self.assertTrue(player.phased)
        self.assertIn((SPEED, ADD, 0.5, 0.0), player.mover.ability_mods.values())
        health, ammo = player.health, player.rifle.ammo
        match._damage_player(player, 100, 3.2)
        player.take_commands([Command(102, buttons=FIRE), Command(103)])
        self.assertEqual((player.health, player.rifle.ammo), (health, ammo))
        player.body_script.arrived(player.body_script.data_last)
        player.take_commands([Command(101 + round(3.1 / DT))])  # past its three seconds
        match.update(6.3, 3)
        self.assertFalse(player.phased)
        self.assertEqual(player.mover.ability_mods, {})
        match._damage_player(player, 100, 6.4)
        self.assertLess(player.health, health)


class DashTests(unittest.TestCase):
    """Blink: the server carries the body, the client follows by the owner's records."""

    def test_a_dash_carries_the_body_its_distance_along_the_input_and_leaves_the_velocity(self):
        for forward, right, heading in ((0, 0, (0.0, 1.0)), (127, 0, (0.0, 1.0)), (-127, 0, (0.0, -1.0)), (0, 127, (-1.0, 0.0))):
            mover = PlayerMover((0.0, 0.0, 0.0), 0, None, heroes()[TRACER].body)
            mover.dash(Command(1, forward=forward, right=right), 7.5, 0.1)
            for frame in range(2, 2 + 14):  # 0.098 s: under way
                mover.step(Command(frame), DT)
            self.assertIsNotNone(mover.dashing)
            mover.step(Command(16), DT)
            self.assertIsNone(mover.dashing)
            x, _, z = mover.position
            self.assertAlmostEqual(x, heading[0] * 7.5, delta=0.02)  # each step's part is quantised
            self.assertAlmostEqual(z, heading[1] * 7.5, delta=0.02)
            self.assertEqual(tuple(mover.velocity), (0.0, 0.0, 0.0))  # a record with the dash's speed overshoots
            self.assertEqual(tuple(mover.history[-1].position), tuple(mover.position))  # what the records send

    def test_a_dash_adds_to_the_run(self):
        mover = PlayerMover((0.0, 0.0, 0.0), 0, None, heroes()[TRACER].body)
        frame = walk(mover, 1.0, 1)
        z, speed = mover.position[2], mover.velocity[2]
        mover.dash(Command(frame, forward=127), 7.5, 0.1)
        walk(mover, 0.105, frame)
        self.assertAlmostEqual(mover.position[2] - z, 7.5 + speed * 0.105, places=1)
        self.assertAlmostEqual(mover.velocity[2], speed, places=3)

    def test_a_press_of_blink_dashes_tracer_and_uses_a_charge(self):
        match, player = equipped(TRACER)
        blink = player.cooldowns[1]
        self.assertEqual(blink.spec.moves, ('dash', 7.5, 0.1))
        z = player.mover.position[2]
        player.take_commands([Command(100, buttons=SHIFT), Command(101)])
        self.assertEqual(player.mover.position[2], z)  # it starts with the body frame that follows the press
        played(player, match, 102, 20)
        self.assertAlmostEqual(player.mover.position[2] - z, 7.5, delta=0.02)
        self.assertIsNone(player.mover.dashing)
        match.gameplay_enabled = False  # the hero select and the like: no dash
        played(player, match, 400, 2, buttons=SHIFT)
        self.assertIsNone(player.mover.dashing)


def played(player, match, first, frames, **held):
    """`frames` more command frames after `first`, each with its body frame built and acknowledged: the mover takes
    an ability's mods from the frames."""
    for frame in range(first, first + frames):
        player.take_commands([Command(frame, **held)])
        match.update(3.1 + (frame - 100) * DT, frame)
        player.body_script.arrived(player.body_script.data_last)
    return first + frames


class LaunchTests(unittest.TestCase):
    """Jump Jet, Jump Pack, Combat Roll and Swift Strike: the numbers are the client's own, measured on a VM body
    (research/lizrollin-runtime.md section 25; tools/research/test_launch_replay.py replays the recordings)."""

    def test_jump_jet_throws_her_20_up_and_10_ahead_from_the_next_step(self):
        mover = PlayerMover((0.0, 0.0, 0.0), 0, None, heroes()[PHARAH].body)
        frame = walk(mover, 1.0, 1)
        ran, z = mover.velocity[2], mover.position[2]
        mover.jet(Command(frame), 10.0, 20.0)
        mover.step(Command(frame), DT)
        self.assertAlmostEqual(mover.position[1], 20.0 * DT, delta=0.002)  # the first move has the launch's own speed
        self.assertAlmostEqual(mover.position[2] - z, (ran + 10.0) * DT, delta=0.002)  # added to her run
        top = 0.0
        while mover.airborne:
            frame += 1
            mover.step(Command(frame), DT)
            top = max(top, mover.position[1])
        self.assertAlmostEqual(top, 20.0 ** 2 / (2 * 17.5), delta=0.15)  # gravity 17.5: 11.4 m

    def test_jump_pack_leaps_at_30_along_the_aim_lifted_and_kept_between_20_and_70_degrees(self):
        up = 32768 / 180  # a degree of pitch; negative looks up
        for pitch, rise in ((0, 20.0), (39.6, 20.0), (-9.9, 20.0), (-19.8, 25.0), (-39.6, 43.7), (-74.3, 70.0)):
            mover = PlayerMover((0.0, 0.0, 0.0), 0, None, heroes()[WINSTON].body)
            mover.leap(Command(1, pitch=round(pitch * up)), 30.0)
            mover.step(Command(2), DT)
            x, y, z = mover.position
            self.assertAlmostEqual(math.degrees(math.atan2(y, z)), rise, delta=0.5, msg=pitch)
            self.assertAlmostEqual(math.hypot(y, z) / DT, 30.0, delta=0.2, msg=pitch)

    def test_jump_packs_states_stay_on_until_he_lands(self):
        match, player = equipped(WINSTON)
        leap, start = player.cooldowns[1], tuple(player.mover.position)
        frame = played(player, match, 100, 1, buttons=SHIFT)
        frame = played(player, match, frame, 40)
        self.assertTrue(leap.active and player.mover.airborne)
        self.assertIn((11, 1.5), {(mod[0], mod[2]) for mod in player.mover.ability_mods.values()})  # gravity x 1.5
        while player.mover.airborne:
            frame = played(player, match, frame, 1)
        played(player, match, frame, 2)
        self.assertFalse(leap.active)
        self.assertGreater(leap.ready_at, 0)
        self.assertAlmostEqual(math.dist(player.mover.position, start), 17.4, delta=0.5)  # level ground, a level aim

    def test_combat_roll_is_15_metres_a_second_for_its_time_then_no_more_than_a_run(self):
        match, player = equipped(CASSIDY)
        z = player.mover.position[2]
        player.take_commands([Command(90, buttons=FIRE), Command(91)])
        self.assertLess(player.rifle.ammo, player.rifle.capacity)
        frame = played(player, match, 100, 1, buttons=SHIFT)
        self.assertEqual(player.rifle.ammo, player.rifle.capacity)  # the roll reloads
        frame = played(player, match, frame, 57)
        self.assertAlmostEqual(player.mover.position[2] - z, 15.0 * 57 * DT, delta=0.05)  # 6 m: no input, nothing slows it
        self.assertEqual(math.hypot(player.mover.velocity[0], player.mover.velocity[2]), 5.5)
        played(player, match, frame, 40)
        self.assertAlmostEqual(player.mover.position[2] - z, 6.21, delta=0.05)  # the client's own roll, standing
        self.assertIsNone(player.mover.capping)  # the roll's end did not play it again

    def test_swift_strike_is_15_metres_along_the_aim_and_his_whole_movement(self):
        up = 32768 / 180
        mover = PlayerMover((0.0, 0.0, 0.0), 0, None, heroes()[GENJI].body)
        frame = walk(mover, 1.0, 1)
        z = mover.position[2]
        mover.strike(Command(frame, pitch=round(-30 * up)), 15.0, 50.0)
        while mover.dashing:
            mover.step(Command(frame, forward=127), DT)
            frame += 1
        self.assertAlmostEqual(mover.position[2] - z, 15.0 * math.cos(math.radians(30)), delta=0.05)  # no run on top
        self.assertAlmostEqual(mover.position[1], 15.0 * math.sin(math.radians(30)), delta=0.05)
        self.assertTrue(mover.airborne)

    def test_swift_strike_cuts_each_bot_on_its_way_once(self):
        from target import STRIKE_DAMAGE
        match, player = equipped(GENJI)
        match._target_updates(player)
        bot = next(b for b in match.bots if b.team != player.team)
        match.bots, player.bots_seen = [bot], {bot.entity}
        x, y, z = player.mover.position
        bot.position = (x, y, z + 5.0)
        health = bot.health
        frame = played(player, match, 100, 1, buttons=SHIFT)
        played(player, match, frame, 60)
        self.assertEqual(health - bot.health, STRIKE_DAMAGE)
        self.assertFalse(bot.knocked)
        self.assertAlmostEqual(player.mover.position[2] - z, 15.0, delta=0.05)


class HoverTests(unittest.TestCase):
    """Pharah's Hover Jets: a Held ability on the jump button whose thrust state the client runs itself."""

    def test_the_jets_come_on_once_jump_has_been_held_and_burn_fuel(self):
        from kits import Held
        hover = Held(QUANTA, 0x40, 0.2, 100.0, 50.0, 43.0, delay=0.2, refills=True)
        changes = {frame: None for frame in range(100, 800) if hover.command(Command(frame, buttons=0x40 if frame < 300 else 0))}
        self.assertEqual(list(changes), [129, 300, 329, 528])  # on 0.2 s in, off at the release, filling from 0.2 s on, full
        self.assertEqual(hover.meter, 100.0)
        hover.command(Command(900, buttons=0x40))
        for frame in range(901, 1300):
            hover.command(Command(frame, buttons=0x40))
        self.assertFalse(hover.active)  # 2 s of fuel
        self.assertEqual(hover.meter, 0.0)

    def test_she_rises_as_the_client_does_and_the_frame_carries_the_thrust(self):
        match, player = equipped(PHARAH)
        hover, y = player.cooldowns[1], player.mover.position[1]
        frame = played(player, match, 100, 100, buttons=0x40)
        self.assertTrue(hover.active)
        self.assertEqual(player.mover.lift, (50.0, 6.875))
        self.assertIn((0, 0.2), {(mod[0], round(mod[2], 3)) for mod in player.mover.ability_mods.values()})  # SPEED +0.2
        self.assertAlmostEqual(player.mover.position[1] - y, 2.81, delta=0.1)  # the client's own hover, 0.7 s in
        instance = kits.ability_instance(kits.ABILITIES[PHARAH][1], hover)
        self.assertTrue({6, 37, 4} <= set(instance.active))  # the thrust, its speed mod, the fuel's ChaseVar
        frame = played(player, match, frame, 100, buttons=0x40)
        self.assertAlmostEqual(player.mover.position[1] - y, 6.02, delta=0.15)
        self.assertAlmostEqual(player.mover.velocity[1], 4.52, delta=0.1)  # where push and gravity meet
        played(player, match, frame, 40)
        self.assertIsNone(player.mover.lift)
        self.assertIn(2, kits.ability_instance(kits.ABILITIES[PHARAH][1], hover).active)  # the refill's ChaseVar


class TimedLaunchTests(unittest.TestCase):
    """Launches that come some time after the press (PlayerMover._throw's `after`): Coach Gun, Rising Uppercut."""

    def test_coach_gun_kicks_her_straight_back_from_the_aim_at_its_shot(self):
        for pitch, (rise, back) in ((0, (6.14, 16.88)), (8192, (13.53, 11.86)), (-8192, (6.14, 16.88))):  # the client's own
            match, player = equipped(ASHE)
            match.mesh = None
            frame = played(player, match, 100, 24, buttons=SHIFT, pitch=pitch)  # the shot: 0.165 s in
            self.assertEqual(player.mover.velocity, (0.0, 0.0, 0.0))
            played(player, match, frame, 1, forward=127, pitch=pitch)  # whatever she ran at: the kick replaces it
            self.assertAlmostEqual(player.mover.velocity[1], rise, delta=0.05)
            self.assertAlmostEqual(player.mover.velocity[2], -back, delta=0.05)  # she faces +z

    def test_coach_gun_fires_once(self):
        match, player = equipped(ASHE)
        shots = []
        match.fire_ability = lambda *args: shots.append(args)
        played(player, match, 100, 120, buttons=SHIFT)  # through its 0.55 s and the Timed's end
        self.assertEqual(len(shots), 1)

    def test_rising_uppercut_lunges_then_rises_then_hangs(self):
        match, player = equipped(DOOMFIST)
        match.mesh = None
        uppercut = player.cooldowns[1]
        frame = played(player, match, 100, 2, buttons=SHIFT, forward=127)
        self.assertAlmostEqual(math.hypot(*player.mover.velocity), 10.0, delta=0.2)  # in place of his run
        self.assertAlmostEqual(player.mover.velocity[1], 2.87, delta=0.1)  # lifted 0.3: 16.7 degrees
        frame = played(player, match, frame, 21)  # 0.15 s: a fifth of the lunge, 1 m/s ahead, 40 up
        self.assertAlmostEqual(player.mover.velocity[1], 40.0, delta=0.3)
        self.assertAlmostEqual(player.mover.velocity[2], 2.8, delta=0.2)
        frame = played(player, match, frame, 15)  # 0.25 s: a fifth of the rise
        self.assertAlmostEqual(player.mover.velocity[1], 7.5, delta=0.6)
        frame = played(player, match, frame, 80)  # falling at the state's terminal velocity, 1 m/s
        self.assertAlmostEqual(player.mover.velocity[1], -1.0, delta=0.05)
        self.assertTrue(uppercut.active)
        played(player, match, frame, 60)
        self.assertFalse(uppercut.active)


class AirJumpTests(unittest.TestCase):
    """Genji's second jump and Hanzo's Lunge: a press of jump in the air launches the server's copy."""

    def top(self, hero, second, **held):
        """(the highest the feet got, the level distance) for a jump with another press `second` frames in."""
        match, player = equipped(hero)
        match.mesh = None
        x, y, z = player.mover.position
        frame = played(player, match, 100, 3, buttons=0x40)
        frame = played(player, match, frame, second - 3)
        frame = played(player, match, frame, 3, buttons=0x40, **held)
        top = 0.0
        while player.mover.airborne:
            frame = played(player, match, frame, 1, **held)
            top = max(top, player.mover.position[1] - y)
        return top, math.hypot(player.mover.position[0] - x, player.mover.position[2] - z), player

    def test_genji_jumps_again_at_one_and_a_half_jumps_once(self):
        plain, _, _ = self.top(GENJI, 10)  # 0.07 s in: not armed yet
        self.assertAlmostEqual(plain, 6.0 ** 2 / (2 * 17.5), delta=0.05)
        again, _, player = self.top(GENJI, 49)  # at the top of the jump: 9 m/s more
        self.assertAlmostEqual(again, plain + 9.0 ** 2 / (2 * 17.5), delta=0.1)
        self.assertEqual(player.air_jump_at, None)  # used up until he has landed and the next step sees it
        played(player, player.match, 2000, 2)
        self.assertIsNotNone(player.air_jump_at)

    def test_hanzo_lunges_15_along_his_input_and_waits_five_seconds(self):
        top, went, player = self.top(HANZO, 49, right=127)
        self.assertAlmostEqual(top, 6.0 ** 2 / (2 * 17.5) + 4.5 ** 2 / (2 * 17.5), delta=0.1)  # 0.3 x 15 upward
        self.assertAlmostEqual(went, 7.5, delta=0.3)  # 15 m/s sideways, slowed by the air, until he lands
        self.assertEqual(player.mover.position[2], equipped(HANZO)[1].mover.position[2])  # none of it forward
        played(player, player.match, 3000, 2)
        self.assertEqual(player.air_jump_at - player.air_used, round(5.0 / DT))


class ShadowStepTests(unittest.TestCase):
    """Reaper's Shadow Step: E opens the aim, primary fire runs it, the server moves him 1.5 s on."""

    def aimed(self):
        match, player = equipped(REAPER)
        match.mesh = None
        frame = played(player, match, 100, 3, buttons=0x10, pitch=2000)  # looking 11 degrees down
        return match, player, player.cooldowns[1], played(player, match, frame, 30, pitch=2000)

    def test_e_opens_the_aim_and_a_click_takes_him_there(self):
        match, player, step, frame = self.aimed()
        start, shots = tuple(player.mover.position), player.rifle.ammo
        self.assertTrue(step.aiming)
        instance = ability_instance(kits.ABILITIES[REAPER][1], step)
        self.assertTrue({0, 2, 4} <= set(instance.active))  # the targeting state, its button, the marker
        self.assertTrue(instance.presence[1].value)
        frame = played(player, match, frame, 3, buttons=FIRE, pitch=2000)
        self.assertTrue(step.active)
        self.assertFalse(step.aiming)
        self.assertEqual(player.rifle.ammo, shots)  # the click is no shot
        x, y, z = step.target
        self.assertEqual(y, start[1])
        self.assertGreater(z - start[2], 5.0)
        frame = played(player, match, frame, 205, forward=127)  # 1.46 s: he has not moved, whatever the input
        self.assertEqual(tuple(player.mover.position), start)
        frame = played(player, match, frame, 12)
        self.assertEqual(tuple(player.mover.position), step.target)
        self.assertTrue(player.phased)
        played(player, match, frame, 80)
        self.assertFalse(step.active)
        self.assertGreater(step.ready_at - step.frame, round(9.5 / DT))  # its 10 s run from its end, 2 s after the click

    def test_the_button_again_or_secondary_fire_puts_the_aim_away(self):
        for button in (0x10, 0x02):
            match, player, step, frame = self.aimed()
            played(player, match, frame, 3, buttons=button)
            self.assertFalse(step.aiming)
            self.assertFalse(step.active)
            self.assertFalse(step.holstered)

    def test_the_place_is_where_a_small_sphere_swept_along_the_aim_touches_the_floor(self):
        match, player, _, _ = self.aimed()
        eye, feet = player.eye(), player.mover.position
        for pitch in (2000, 4000, 8000):  # 11, 22 and 44 degrees down
            x, y, z = match.step_point(player, Command(1, pitch=pitch), 35.0)
            self.assertAlmostEqual(y, feet[1], delta=0.002)
            # the sphere's centre is 0.115 m over the floor when it touches: the client's own marker, to the centimetre
            self.assertAlmostEqual(z - eye[2], (eye[1] - feet[1] - 0.115) / math.tan(pitch * math.pi / 32768), delta=0.02)
        self.assertIsNone(match.step_point(player, Command(1, pitch=-8000), 35.0))  # into the sky: no floor under its end

    def test_a_place_lies_inside_a_cone_under_the_aim_and_the_one_nearest_the_crosshair_wins(self):
        import shadow_step
        depth = shadow_step._depth
        self.assertAlmostEqual(depth(10.0, 0.0, 35.0), 10.0 * math.tan(shadow_step.CONE) + 0.5, delta=1e-6)  # level
        self.assertAlmostEqual(depth(3.26, -math.sin(math.radians(5.9)), 35.0), 1.10, delta=0.01)  # the client's own
        self.assertEqual(depth(300.0, 0.0, 35.0), 35.5)  # the aim's own length at most
        match, player, _, _ = self.aimed()
        eye = player.eye()
        # a level aim over open floor: the floor under the aim's end is in its cone, and looks nearest the crosshair
        x, y, z = match.step_point(player, Command(1, pitch=0), 35.0)
        self.assertAlmostEqual(z - eye[2], 35.0, delta=0.01)
        self.assertAlmostEqual(y, player.mover.position[1], delta=0.002)

    def room(self, *extra):
        """A floor at 0 with a wall across z = 8 (and more triangles), and an eye 1.55 m up looking level at it."""
        from ow174.game import collision
        quad = lambda a, b, c, d: [(*a, *b, *c), (*a, *c, *d)]
        triangles = quad((-50, 0, -50), (-50, 0, 50), (50, 0, 50), (50, 0, -50))
        triangles += quad((-50, 0, 8), (50, 0, 8), (50, 9, 8), (-50, 9, 8))
        for corners in extra:
            triangles += quad(*corners)
        return collision.World(triangles), (0.0, 1.55, 0.0), (0.0, 0.0, 1.0)

    def test_the_line_back_to_the_eye_is_let_down_and_stands_him_on_what_it_touches(self):
        import shadow_step
        world, eye, aim = self.room()
        x, y, z = shadow_step.place(world, eye, aim, 35.0)
        self.assertAlmostEqual(z, 7.5, delta=0.02)  # half a metre off the wall the sphere touched, by a ray: on the floor
        self.assertAlmostEqual(y, 0.0, delta=0.001)
        table = ((-1, 1, 4), (-1, 1, 6), (1, 1, 6), (1, 1, 4))  # a top 0.55 m under the aim's line, 4 to 6 m on
        x, y, z = shadow_step.place(*self.room(table), 35.0)
        self.assertAlmostEqual(y, 1.01, delta=0.001)  # nearer the crosshair than the floor at the wall; the line's 1 cm
        self.assertTrue(4.0 <= z <= 6.0)

    def test_a_place_needs_the_eye_in_sight_and_room_for_a_sphere_over_it(self):
        import shadow_step
        world, eye, _ = self.room()
        stands = lambda z: shadow_step._stands(world, eye, (0.0, 0.0, z), 1.0)
        self.assertTrue(stands(7.5))
        self.assertFalse(stands(7.7))  # 0.3 m from the wall: the sphere starts in it
        self.assertFalse(stands(9.0))  # behind the wall: out of the eye's sight
        lintel = ((-1, 1.9, 4), (1, 1.9, 4), (1, 1.9, 6), (-1, 1.9, 6))  # 1.9 m over the floor: no room to rise to 2 m
        world, eye, _ = self.room(lintel)
        self.assertFalse(stands(5.0))

    def test_a_floor_is_judged_by_the_faces_about_the_hit_not_by_how_the_cast_touched(self):
        import shadow_step
        top = ((-1, 1, 4), (-1, 1, 6), (1, 1, 6), (1, 1, 4))
        side = ((-1, 0, 6), (1, 0, 6), (1, 1, 6), (-1, 1, 6))  # the top's far side, a wall
        ramp = ((-1, 0, -3), (-1, 2, -2), (1, 2, -2), (1, 0, -3))  # 63 degrees steep
        world, _, _ = self.room(top, side, ramp)
        mesh, = shadow_step._meshes(world)
        self.assertTrue(shadow_step._floor(mesh, (0.0, 1.0, 5.99)))  # on the top's edge: the wall under it is not looked at
        self.assertFalse(shadow_step._floor(mesh, (0.0, 1.0, -2.5)))  # on the ramp
        self.assertIsNone(shadow_step._floor(mesh, (0.0, 0.5, 3.0)))  # no triangle near
        ground = shadow_step._ground
        self.assertTrue(ground((mesh,), (0.0, 1.0, 5.99), 0.6, 0))  # how steeply a line met the edge does not count
        self.assertTrue(ground((), (0.0, 0.0, 0.0), 0.8, 0))  # a polytope: the contact's own normal,
        self.assertFalse(ground((), (0.0, 0.0, 0.0), 0.7, 0))
        self.assertFalse(ground((), (0.0, 0.0, 0.0), 1.0, 1))  # and the chunk's flag

    def test_the_casts_go_through_a_placed_prop_with_the_desks_flag(self):
        import shadow_step
        from ow174.game import collision
        corners = [(-50, 0, -50), (-50, 0, 50), (50, 0, 50), (50, 0, -50), (-50, 0, 8), (50, 0, 8), (50, 9, 8), (-50, 9, 8)]
        chunk = (corners, [(a, b, c, -1, -1, -1, 0x18, 0) for a, b, c in ((0, 1, 2), (0, 2, 3), (4, 5, 6), (4, 6, 7))])
        top = [(-1, 1, 6.5), (-1, 1, 7.6), (1, 1, 7.6), (1, 1, 6.5)]  # under the place a level aim at the wall finds

        def place(flags):
            prop = [[('mesh', flags, top, [(0, 1, 2, -1, -1, -1, flags, 0), (0, 2, 3, -1, -1, -1, flags, 0)])]]
            put = [(0, 0, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0), (0.0, 0.0, 0.0, 1.0))]
            world = collision.World(*collision.probe_triangles([chunk], [], prop, put), 'test',
                                    collision.engine_world([chunk], [], prop, put))
            return shadow_step.place(world, (0.0, 1.55, 0.0), (0.0, 0.0, 1.0), 35.0)

        self.assertAlmostEqual(place(0x111A)[1], 1.0, delta=0.001)  # a railing's flags: he stands on it
        self.assertAlmostEqual(place(0x1FDF)[1], 0.0, delta=0.001)  # a desk's: the floor under it

    def test_with_the_clients_feed_the_place_is_its_own_marker(self):
        import client_sync
        match, player, step, frame = self.aimed()
        resync = client_sync.Resync()
        self.addCleanup(setattr, client_sync, '_resync', client_sync._resync)
        client_sync._resync = resync
        resync.aimed(frame, (50.0, 1.03451, 30.0), True)
        self.assertEqual(match.step_point(player, Command(frame + 5, pitch=2000), 35.0), (50.0, 1059 / 1024, 30.0))  # on the feet's grid
        resync.aimed(frame + 6, (50.0, 1.0, 30.0), False)  # the marker shows no place
        self.assertIsNone(match.step_point(player, Command(frame + 8, pitch=2000), 35.0))
        self.assertIsNotNone(match.step_point(player, Command(frame + 100, pitch=2000), 35.0))  # stale: the server's own

    def test_a_click_with_no_floor_to_stand_on_keeps_the_aim_open(self):
        match, player, step, frame = self.aimed()
        played(player, match, frame, 3, buttons=FIRE, pitch=-8000)  # at the sky
        self.assertTrue(step.aiming)
        self.assertFalse(step.active)


class GuardianAngelTests(unittest.TestCase):
    """Mercy's Guardian Angel: Shift at an ally in view flies her to a point by him at 17 m/s and ends there."""

    def with_ally(self, ahead=15.0):
        from target import TrainingTarget
        match, player = equipped(MERCY)
        match.mesh = None
        x, y, z = player.mover.position
        ally = TrainingTarget((x, y, z + ahead), 180, team=player.team, entity=0xB0000101)
        match.bots, player.bots_seen = [ally], {ally.entity}
        return match, player, ally, player.cooldowns[0], tuple(player.mover.position)

    def test_shift_at_an_ally_flies_her_to_him_and_the_cooldown_runs_from_her_arrival(self):
        match, player, ally, angel, start = self.with_ally()
        frame = played(player, match, 100, 2, buttons=SHIFT)
        self.assertTrue(angel.active)
        self.assertIn(2, ability_instance(kits.ABILITIES[MERCY][0], angel).active)  # the flight's pose
        frame = played(player, match, frame, 50)  # 0.35 s on
        x, y, z = player.mover.position
        self.assertAlmostEqual(z - start[2], 17.0 * 51 * DT, delta=0.3)  # 17 m/s, whatever the input
        self.assertGreater(y, start[1])  # toward half a metre over his feet: no gravity on the way
        frame = played(player, match, frame, 70)  # 12.5 m take 0.74 s
        self.assertFalse(angel.active)
        x, y, z = player.mover.position
        self.assertAlmostEqual(ally.position[2] - z, 2.5, delta=0.05)  # short of him
        self.assertGreater(angel.ready_at - angel.frame, round(1.0 / DT))  # 1.5 s from the end, not from the press
        self.assertIsNone(player.guardian)

    def test_without_an_ally_in_view_the_press_does_nothing(self):
        for change in ('turned away', 'too far', 'an enemy'):
            match, player, ally, angel, start = self.with_ally(40.0 if change == 'too far' else 15.0)
            if change == 'an enemy':
                ally.team = 1 - player.team
            played(player, match, 100, 3, buttons=SHIFT, yaw=16384 if change == 'turned away' else 0)
            self.assertFalse(angel.active, change)
            self.assertEqual(angel.ready_at, 0, change)  # no cooldown either

    def test_the_button_again_ends_it_on_the_way(self):
        match, player, ally, angel, start = self.with_ally()
        frame = played(player, match, 100, 2, buttons=SHIFT)
        frame = played(player, match, frame, 30)
        frame = played(player, match, frame, 2, buttons=SHIFT)
        self.assertFalse(angel.active)
        there = player.mover.position[2]
        self.assertLess(there - start[2], 5.0)
        played(player, match, frame, 30)
        self.assertLess(player.mover.position[2] - there, 1.0)  # the flight is over with it


class ExoBootsTests(unittest.TestCase):
    """Baptiste's Exo Boots: crouch builds the charge the frame carries, and the jump's speed is the graph's own
    expression of it on both sides."""

    def jump(self, crouched):
        """The speed he leaves the ground at after `crouched` frames of crouch, and his charge once in the air."""
        match, player = equipped(BAPTISTE)
        match.mesh = None
        frame = played(player, match, 100, crouched, buttons=0x80) if crouched else 100
        played(player, match, frame, 2, buttons=0x40)
        return player.mover.velocity[1], player.cooldowns[2].meter

    def test_a_full_charge_triples_the_jump(self):
        self.assertAlmostEqual(self.jump(0)[0], 6.0, delta=0.3)  # two steps of gravity on
        rise, left = self.jump(120)  # 0.84 s: seven steps of 0.1428
        self.assertAlmostEqual(rise, 18.0, delta=0.3)
        self.assertEqual(left, 0.0)  # spent
        self.assertAlmostEqual(self.jump(31)[0], 6.0 * (1 + 2 * 3 * 0.1428), delta=0.3)  # three steps: at 0, 0.1, 0.2 s

    def test_the_charge_is_lost_a_second_after_he_stands_up(self):
        match, player = equipped(BAPTISTE)
        charge = player.cooldowns[2]
        frame = played(player, match, 100, 120, buttons=0x80)
        self.assertEqual(charge.meter, 1.0)
        frame = played(player, match, frame, 100)
        self.assertEqual(charge.meter, 1.0)
        played(player, match, frame, 60)
        self.assertEqual(charge.meter, 0.0)

    def test_an_expression_of_unsent_variables_gives_no_mod(self):
        ability = kits.ABILITIES[BAPTISTE][2]
        instance = ability_instance(ability, kits.Crouched(QUANTA, 0.1428, 0.1, 1.0))
        (index, _, value, _), = frame_mods([instance]).values()
        self.assertEqual((index, value), (JUMP_SPEED_MOD, 1.0))
        del instance.presence[0]  # var176 unsent
        self.assertEqual(frame_mods([instance]), {})


class RecallTests(unittest.TestCase):
    def test_recall_takes_tracer_back_three_seconds_with_her_best_health(self):
        match, player = equipped(TRACER)
        match.mesh = None
        recall = player.cooldowns[0]
        frame = played(player, match, 100, 300, forward=127)  # 2.1 s at a run
        then = tuple(player.mover.position)
        player.health = 40
        frame = played(player, match, frame, 430, forward=127)  # 3 s more: 18 m on, hurt all the way
        here = tuple(player.mover.position)
        self.assertAlmostEqual(here[2] - then[2], 18.0, delta=0.3)
        frame = played(player, match, frame, 3, buttons=0x10)
        self.assertTrue(recall.active)
        self.assertTrue(player.phased)
        frame = played(player, match, frame, 90)  # half of its 1.25 s: half of the way back
        self.assertAlmostEqual(player.mover.position[2], (here[2] + then[2]) / 2, delta=0.5)
        played(player, match, frame, 100)
        self.assertFalse(recall.active)
        self.assertAlmostEqual(player.mover.position[2], then[2], delta=0.1)
        self.assertEqual(player.health, 40)  # no better health on that path
        self.assertLess(len(player.trail), 20)  # the path starts anew when it ends

    def test_recall_gives_back_the_health_she_had(self):
        match, player = equipped(TRACER)
        frame = played(player, match, 100, 200)
        player.health = 30  # hurt a moment ago: three seconds back she was whole
        frame = played(player, match, frame, 50)
        played(player, match, frame, 200, buttons=0x10)
        self.assertEqual(player.health, player.max_health)


class MineJumpTests(unittest.TestCase):
    def test_junkrats_own_mine_throws_him_15_away_and_15_up(self):
        from target import ABILITY_HITS
        match, player = equipped(JUNKRAT)
        match.mesh = None
        x, y, z = player.mover.position
        hit = ABILITY_HITS[('Junkrat', SHIFT)]
        match.rockets.append((0.0, (x, y, z - 1.0), None, [], player, hit))  # a metre behind him
        match._land_rockets(1.0)
        played(player, match, 100, 1)
        self.assertEqual(player.mover.velocity, (0.0, 15.0, 15.0))
        self.assertTrue(player.mover.airborne)
        self.assertEqual(player.health, player.max_health)  # his own explosives do not hurt him
        far, other = equipped(JUNKRAT)
        far.rockets.append((0.0, (x, y, z - 5.0), None, [], other, hit))  # past its 3 m
        far._land_rockets(1.0)
        played(other, far, 100, 1)
        self.assertEqual(other.mover.velocity, (0.0, 0.0, 0.0))


class RollTests(unittest.TestCase):
    """Wrecking Ball's Roll: a toggle whose MovementMod state swaps the ball's mover values in."""

    def test_the_ball_rolls_at_10_until_shift_is_pressed_again(self):
        match, player = equipped(WRECKING_BALL)
        match.mesh = None
        roll = player.cooldowns[2]
        frame = played(player, match, 100, 3, buttons=SHIFT)
        self.assertEqual(player.mover.swapped.run_forward, 10.0)
        self.assertFalse(player.mover.aimed)
        frame = played(player, match, frame, 300, forward=127)
        self.assertAlmostEqual(player.mover.velocity[2], 10.0, delta=0.1)
        frame = played(player, match, frame, 3, buttons=SHIFT)
        self.assertFalse(roll.active)
        self.assertIsNone(player.mover.swapped)
        played(player, match, frame, 300, forward=127)
        self.assertAlmostEqual(player.mover.velocity[2], 5.5, delta=0.1)


class FlightTests(unittest.TestCase):
    """Echo's Flight: a launch the server plays, then 3.15 s along the aim on the state's own mover values, which the
    client runs itself (research/lizrollin-runtime.md section 26; test_launch_replay.py replays her own flights)."""

    def test_the_frame_swaps_the_flights_mover_in_and_she_stops_in_the_air(self):
        match, player = equipped(ECHO)
        flight, start = player.cooldowns[1], tuple(player.mover.position)
        frame = played(player, match, 100, 3, buttons=SHIFT)
        self.assertTrue(player.mover.aimed)
        self.assertEqual((player.mover.swapped.run_forward, player.mover.swapped.decel_air_base), (8.0, 3.0))
        self.assertAlmostEqual(math.hypot(*player.mover.velocity), 27.0, delta=0.6)  # 28 m/s, two steps of air on
        frame = played(player, match, frame, 37)
        self.assertAlmostEqual(math.hypot(*player.mover.velocity), 11.0, delta=0.3)  # past the limit of 12.42
        frame = played(player, match, frame, 200)
        self.assertAlmostEqual(math.dist(player.mover.position, start), 7.8, delta=0.1)  # the client's own: 7.80 m
        self.assertEqual(player.mover.velocity, (0.0, 0.0, 0.0))  # no gravity: she hangs there
        self.assertTrue(player.mover.airborne)
        played(player, match, frame, 220)  # 3.15 s after the press
        self.assertFalse(flight.active)
        self.assertFalse(player.mover.aimed)
        self.assertIsNone(player.mover.swapped)

    def test_she_flies_at_8_along_the_aim_and_jump_and_crouch_are_up_and_down(self):
        for held, way in ((dict(forward=127, pitch=-5461), (0.0, 0.5, 0.866)), (dict(buttons=0x40), (0.0, 1.0, 0.0)),
                          (dict(buttons=0x80), (0.0, -1.0, 0.0)), (dict(right=127), (-1.0, 0.0, 0.0))):
            match, player = equipped(ECHO)
            match.mesh = None
            frame = played(player, match, 100, 2, buttons=SHIFT)
            frame = played(player, match, frame, 60)  # the launch spent, in the open air
            player.mover.position[1] += 50.0
            played(player, match, frame, 150, **held)
            for got, wanted in zip(player.mover.velocity, way):
                self.assertAlmostEqual(got, wanted * 8.0, delta=0.15, msg=held)

    def test_jump_held_at_the_press_adds_as_much_again_upward(self):
        mover = PlayerMover((0.0, 0.0, 0.0), 0, None, heroes()[ECHO].body)
        mover.fly(Command(1, buttons=0x40), 28.0)
        mover.step(Command(1), DT)
        self.assertAlmostEqual(mover.velocity[1], 28.0, delta=0.5)
        self.assertAlmostEqual(mover.velocity[2], 28.0, delta=0.5)


class GlideTests(unittest.TestCase):
    """Angelic Descent and Glide: passives on the jump button, each a MovementMod state the frame carries."""

    def airtime(self, hero, hold, mesh=None):
        """(seconds in the air, the fall's speed as she lands) for a jump with the button held `hold` frames."""
        match, player = equipped(hero)
        match.mesh = mesh
        frame, fall = played(player, match, 100, 2, buttons=0x40), 0.0
        while player.mover.airborne:
            fall = player.mover.velocity[1]
            frame = played(player, match, frame, 1, buttons=0x40 if frame - 100 < hold else 0)
        return (frame - 101) * DT, fall

    def test_mercy_falls_at_two_metres_a_second_while_jump_is_held(self):
        seconds, fall = self.airtime(MERCY, 300)
        self.assertAlmostEqual(seconds, 0.90, delta=0.04)  # the client's own, live with no corrections: 0.88 s over 2 cm up
        self.assertAlmostEqual(fall, -1.95, delta=0.1)
        self.assertAlmostEqual(self.airtime(MERCY, 5)[0], 0.67, delta=0.04)  # a tap: a plain jump

    def test_echo_glides_only_with_air_under_her(self):
        seconds, fall = self.airtime(ECHO, 300)  # no map: any fall counts
        self.assertAlmostEqual(seconds, 0.73, delta=0.04)
        self.assertGreater(fall, -3.9)  # toward the Glide's 4 m/s; a plain jump lands at 5.6
        ground = type('Mesh', (), {'ray': lambda self, origin, direction, length: (1.0, None)})()
        self.assertAlmostEqual(self.airtime(ECHO, 300, ground)[0], 0.67, delta=0.04)  # ground within 2.5 m: a plain jump


class ServerSideTests(unittest.TestCase):
    """What the server plays itself while one of these runs."""

    def test_cryo_freeze_heals_while_the_ice_holds(self):
        match, player = equipped(MEI)
        match._damage_player(player, 100, 3.05)
        hurt = player.health
        player.take_commands([Command(100, buttons=SHIFT), Command(101)])
        for step in range(1, 11):  # a second of ticks: a pulse every 0.2 s
            player.take_commands([Command(101 + round(step * 0.1 / DT))])
            match.update(3.1 + step * 0.1, 1 + step)
        self.assertAlmostEqual(player.health - hurt, 37.5, delta=7.6)
        self.assertTrue(player.phased)

    def test_no_bot_sees_sombra_once_stealth_has_faded_her_out(self):
        match, player = equipped(SOMBRA)
        player.select_open = False
        match.bot_attacks_enabled = True
        self.assertTrue(match._can_be_shot(player))
        player.take_commands([Command(100, buttons=SHIFT), Command(101), Command(100 + round(0.5 / DT))])
        self.assertTrue(match._can_be_shot(player))  # still fading
        player.take_commands([Command(100 + round(0.8 / DT))])
        self.assertFalse(match._can_be_shot(player))
        player.take_commands([Command(100 + round(2.0 / DT), buttons=FIRE)])  # a shot ends it
        self.assertTrue(match._can_be_shot(player))

    def test_charge_bumps_a_bot_once_and_a_wall_ends_it(self):
        from target import CHARGE_BUMP
        match, player = equipped(REINHARDT)
        match._target_updates(player)
        bot = next(b for b in match.bots if b.team != player.team)
        match.bots, player.bots_seen = [bot], {bot.entity}
        x, y, z = player.mover.position
        bot.position = (x, y, z + 1.0)
        timed = player.cooldowns[0]
        timed.command(Command(100, buttons=SHIFT))
        player.mover.ability_mods = frame_mods([at(REINHARDT, 0, 2.0)[2]])  # the frame that drives him forward
        self.assertTrue(player.mover.driven)
        health = bot.health
        player.mover.velocity = (0.0, 0.0, 16.5)
        for frame in (200, 201):
            timed.command(Command(frame))
            player.charge(Command(frame))
        self.assertEqual(health - bot.health, CHARGE_BUMP)  # once
        self.assertTrue(timed.active)
        player.mover.velocity = (0.0, 0.0, 0.0)  # a wall
        timed.command(Command(200 + round(0.4 / DT)))
        player.charge(Command(200 + round(0.4 / DT)))
        self.assertFalse(timed.active)
        self.assertGreater(timed.ready_at, 0)


if __name__ == '__main__':
    unittest.main()
