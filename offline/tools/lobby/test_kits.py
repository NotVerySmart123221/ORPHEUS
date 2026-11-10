import unittest

import practice  # noqa: F401  installs the pinned vendor path
from kits import ABILITIES, ASHE, CASSIDY, DOOMFIST, DVA, JUNKRAT, Held, Timed, KITS, LUCIO, TRACER, WINSTON, ZARYA, Ability, Charges, _bits, ability_instance, frame_builder, model_entity, weapon_instance
from ow174.game.statescript import Asset, Bool, Float, Graph, Instance, Int, StateBound
from ow174.game.commands import Command
from soldier import FIELD_BUTTON, FIELD_COOLDOWN, RifleUpdate, Volley, ability_payload, chase_since, field_instance
from target import PLAYER_KIT
from ow174.game.content import heroes


class KitTests(unittest.TestCase):
    def test_the_client_reads_every_kit_frame(self):
        import oracle
        from kits import BASTION_SENTRY, DVA_PILOT, MERCY_BLASTER
        kits = {f'{hero:#x}': kit for hero, kit in KITS.items()}
        kits.update(mercy_blaster=MERCY_BLASTER, bastion_sentry=BASTION_SENTRY, dva_pilot=DVA_PILOT)
        for name, kit in kits.items():
            updates = {'idle': None, 'firing': RifleUpdate(kit.capacity - 1, 1, True, True),
                       'released': RifleUpdate(kit.capacity - 1, 2, False, True),
                       'reloading': RifleUpdate(0, 3, False, True, True)}
            for label, update in updates.items():
                with self.subTest(kit=name, update=label):
                    frame = oracle.read_full(frame_builder(kit)(1, update))
                    self.assertEqual(frame.instances[kit.instance].graph, kit.graph.index)

    def test_weapon_frames_keep_the_constants_each_graph_sets_itself(self):
        # the graph's own SetVars of the Volley's spread range (229 / 230) and of var476, which our defaults overrode
        import gzip
        import json
        from pathlib import Path
        import oracle
        from kits import AUTHORED
        path = Path(practice.__file__).parent / 'vendor' / 'data' / 'game_graphs_174.json.gz'
        graphs = {g['index']: g for g in json.load(gzip.open(path, 'rt', encoding='utf-8'))['graphs']}
        kinds = {'STUConfigVarFloat': Float, 'STUConfigVarInt': Int}
        for kit in KITS.values():
            found = {}
            for node in graphs.get(kit.graph.index, {'nodes': ()})['nodes']:
                var = (node['fields'].get('m_out_Var') or {}).get('m_identifier')
                if node['class'] == 'STUStatescriptActionSetVar' and var in (229, 230, 476) and var in kit.bits:
                    found.setdefault(var, set()).add(json.dumps(node['fields']['m_value'], sort_keys=True))
            expected = {}
            for var, values in found.items():
                value = json.loads(next(iter(values)))
                if len(values) == 1 and value['$'] in kinds:
                    expected[var] = kinds[value['$']](value['m_value'])
            with self.subTest(graph=hex(kit.graph.index)):
                self.assertEqual(AUTHORED.get(kit.graph.index, {}), expected)
        tracer = oracle.read_full(frame_builder(KITS[TRACER])(1)).instances[KITS[TRACER].instance]
        self.assertEqual((tracer.vars[229][0], tracer.vars[230][0]), (0.0, 3.5))  # (value, bindings)
    def test_mercy_weapon_notifies_the_first_person_reload_category(self):
        from kits import MERCY, MERCY_BLASTER

        common = Instance(75, Graph(75, 39, 4), active={19: None})
        for kit, category in ((KITS[MERCY], 0x028000000000007F), (MERCY_BLASTER, 0x028000000000007D)):
            instances, variables = frame_builder(kit)(1, extra_instances=(common,),
                serialize=lambda _, instances, variables: (instances, variables))
            self.assertEqual(variables[1226], Asset(category))
            self.assertIn(23, instances[1].active)
        self.assertNotIn(23, common.active)

    def test_scope_uses_the_hero_camera_states_and_unlinked_reference_bit(self):
        from kits import ANA, WIDOWMAKER
        for hero, progress, request, camera, button, angle, pose in (
            (ASHE, 11, 22, (6, 83), 48, 40, (80, 24)), (ANA, 7, 15, (23,), 50, 30, (28, 26)),
            (WIDOWMAKER, 4, 16, (7,), 70, 30, (44, 46)),
        ):
            with self.subTest(hero=hero):
                kit = KITS[hero]
                scoped = weapon_instance(kit, kit.capacity, scoped=True)
                self.assertEqual((scoped.presence[progress], scoped.presence[request]), (Float(1), Bool(True)))
                self.assertEqual(scoped.extra[172], Int(angle))
                self.assertEqual(scoped.active[button].count, 5)
                self.assertTrue(set(pose) <= scoped.active.keys())
                for state in camera:
                    self.assertEqual((scoped.active[state].count, scoped.active[state].value), (1, 0))
                hip = weapon_instance(kit, kit.capacity)
                reload = weapon_instance(kit, kit.capacity - 1,
                                         RifleUpdate(kit.capacity - 1, 1, False, True, True), scoped=True)
                for instance in (hip, reload):
                    self.assertEqual((instance.presence[progress], instance.presence[request]), (Float(0), Bool(False)))
                if hero != ASHE:
                    self.assertEqual((scoped.extra[168], hip.extra[168]), (Int(angle), Int(0)))
                common = Instance(75, Graph(75, 39, 4), active={19: None})
                for scoped, update, category in (
                    (True, None, 0x0280000000000081), (False, None, 0),
                    (True, RifleUpdate(kit.capacity - 1, 1, False, True, True), 0),
                ):
                    instances, variables = frame_builder(kit)(1, update, scoped=scoped,
                        extra_instances=(common,), serialize=lambda _, instances, variables: (instances, variables))
                    self.assertEqual(variables[1273], Asset(category))
                    self.assertIn(23, instances[1].active)
                    self.assertNotIn(23, common.active)

    def test_ashe_reload_phases_send_the_native_wait_and_animation(self):
        for phase, pair in {'open': {68, 69}, 'insert': {70, 71}, 'close': {73, 74}}.items():
            with self.subTest(phase=phase):
                instance = weapon_instance(KITS[ASHE], 10, RifleUpdate(10, 2, False, True, True, phase))
                self.assertEqual(instance.active.keys() & {68, 69, 70, 71, 73, 74}, pair)
                self.assertEqual(instance.presence[16], Int(10))
                if phase == 'insert':
                    self.assertEqual(instance.extra[8743], Int(1))

    def test_doomfist_recharge_sends_the_refill_wait_and_its_animation(self):
        kit = KITS[DOOMFIST]
        recharging = weapon_instance(kit, 2, RifleUpdate(2, 3, False, True, True, 'recharge'))
        self.assertEqual(recharging.active.keys() & {17, 19, 20, 54}, {17, 19, 20, 54})
        self.assertEqual((recharging.presence[13], recharging.extra[51]), (Int(2), Bool(False)))
        full = weapon_instance(kit, 4, RifleUpdate(4, 3, False, True))
        self.assertFalse(full.active.keys() & {17, 19, 20, 54})

    def test_reload_delta_falls_back_when_instances_change_or_lack_a_layout(self):
        from ow174.game.statescript import _owner_changes
        hud = Instance(463, Graph(463, 41, 22), active={39: None})
        weapon = weapon_instance(KITS[CASSIDY], 5)
        self.assertIsNone(_owner_changes(1, ([hud], {}), ([hud, weapon], {})))
        host = Instance(4429, Graph(4429, 1, 0), active={0: None})  # not in the pinned catalog
        self.assertIsNone(_owner_changes(1, ([host], {}), ([Instance(4429, Graph(4429, 1, 0), active={})], {})))
        self.assertIsNotNone(_owner_changes(1, ([host, hud], {}), ([host, Instance(463, Graph(463, 41, 22), active={})], {})))

    def test_owner_ammo_delta_reconciles_native_clock_without_replaying_sprint(self):
        from ow174.game.bits import BitReader
        from ow174.game.statescript import Stream
        from soldier import anim_payload

        stream = Stream(0xA0000101)
        sprint = Instance(597, Graph(597, 31, 9), active={15: anim_payload(1)})
        rifle = lambda ammo: Instance(1, Graph(596, 81, 23), presence={8: Int(ammo)})
        stream.reload_frame(100, [rifle(30), sprint], {7184: Int(30)}, refresh=True)
        stream.arrived(stream.data_last)
        update = stream.reload_frame(116, [rifle(29), sprint], {7184: Int(29)}, refresh=True)
        self.assertEqual((update.chunk.count, update.chunk.getvalue().hex()),
                         (211, '040000f06a742e0560101cd4010000000010000875000000000000'))

        reader = BitReader(update.chunk.getvalue())
        self.assertEqual((reader.bit(), reader.bits(16), reader.var_b()), (0, 2, 0))
        self.assertEqual(reader.bit(), 0)  # unfragmented
        payload_bits = reader.var_b()
        payload_start = reader.pos
        # Native1C2EE10 applies CmFD to the DSR context only for C1. C0 left ammo stale in live firing.
        self.assertEqual(reader.bits(3), 0b110)
        self.assertEqual(reader.var_b(), 116)
        body_bits = reader.var_b()
        body_start = reader.pos
        self.assertEqual(reader.bits(5), 2)  # instance1; unchanged Sprint597 is absent
        self.assertEqual(reader.bits(2), 0)  # existing descriptor
        self.assertEqual(reader.bits(5), 0)  # end instances
        self.assertEqual(reader.bit(), 1)
        self.assertEqual((reader.var_b(), reader.bits(4), reader.bits(32), reader.bit()), (7184, 4, 29, 0))
        self.assertEqual(reader.var_b(), 0)  # end entity variables
        # Native1C2CA20 consumes this24-bit mask and selects variable53 in the128-slot table.
        self.assertEqual((reader.bits(16), reader.bits(8)), (1 << 6, 1 << 5))
        self.assertEqual((reader.bits(4), reader.bits(32), reader.bit()), (4, 29, 0))
        self.assertEqual(reader.bit(), 0)  # no extra variables
        self.assertEqual(reader.bits(11), 0)  # no changed rifle states
        self.assertEqual(reader.bit(), 0)  # no instance events
        self.assertEqual(reader.pos - body_start, body_bits)
        self.assertEqual(reader.bits(3), 0)  # no more frames or frame events
        self.assertEqual(reader.pos - payload_start, payload_bits)

    def test_timed_state_output_binding_requests_native_per_frame_cache(self):
        from ow174.game.bits import BitReader, BitWriter
        from ow174.game.statescript import write_variable

        out = BitWriter()
        write_variable(out, StateBound(Float(0), 1, 19, 7, per_frame=True))
        reader = BitReader(out.getvalue())
        self.assertEqual((reader.bits(4), reader.bits(32)), (8, 0))
        self.assertEqual(reader.bits(2), 1)  # compact binding count1
        self.assertEqual(reader.bit(), 0)  # no parameters
        self.assertEqual((reader.bit(), reader.bit(), reader.bits(5)), (1, 0, 2))
        self.assertEqual(reader.bits(7), 19)
        # Native1C2D4F0 stores final11 at binding+41/+42. 2DC6BE0 refreshes it once per frame.
        self.assertEqual(reader.bits(4), 0b1100)  # default output/priority, dynamic, per-frame cache
        self.assertEqual(reader.pos, out.count)
        for value in (1, None, 'yes'):
            with self.subTest(per_frame=value), self.assertRaises(ValueError):
                StateBound(Float(0), 1, 19, 7, per_frame=value)

    def test_reload_initial_and_fallback_full_frames_reconcile_clock(self):
        from ow174.game.bits import BitReader
        from ow174.game.statescript import Stream, owner_full_frame

        rifle = Instance(1, Graph(596, 81, 23), presence={8: Int(30)})
        sprint = Instance(597, Graph(597, 31, 9))
        self.assertEqual(BitReader(owner_full_frame(100, [rifle], {}).getvalue()).bits(3), 0b010)
        stream = Stream(0xA0000101)
        first = stream.reload_frame(100, [rifle], {}, refresh=True)
        stream.arrived(stream.data_last)
        fallback = stream.reload_frame(116, [rifle, sprint], {}, refresh=True)
        for update in (first, fallback):
            reader = BitReader(update.chunk.getvalue())
            self.assertEqual((reader.bit(), reader.bits(16)), (0, 0))  # full chunk
            reader.var_b()  # last frame
            self.assertEqual(reader.bit(), 0)
            reader.var_b()  # payload length
            self.assertEqual(reader.bits(3), 0b110)  # authoritative full data also needs CmFD clock

    def test_idle_weapon_shows_a_full_magazine_and_no_volley(self):
        for kit in KITS.values():
            instance = weapon_instance(kit, kit.capacity)
            if 53 in kit.bits:
                self.assertEqual(instance.presence[kit.bits[53]], Int(kit.capacity))
            self.assertTrue(kit.model is None or kit.model in instance.active)
            if kit.volley is not None:
                self.assertNotIn(kit.volley, instance.active)
            self.assertEqual(49 in kit.bits and kit.model is not None, isinstance(instance.presence.get(kit.bits.get(49)), StateBound))

    def test_a_shot_turns_the_volley_and_its_stack_on(self):
        kit = KITS[CASSIDY]
        instance = weapon_instance(kit, 5, RifleUpdate(5, 1, True, True))
        self.assertEqual(instance.presence[kit.bits[53]], Int(5))
        self.assertIsNotNone(instance.active[kit.volley])
        self.assertTrue(set(kit.fire) <= set(instance.active))

    def test_dva_cannons_alternate_muzzle_and_kick_while_held(self):
        from kits import DVA_HELD
        kit, muzzle = KITS[DVA], KITS[DVA].bits[58]
        first, second = (weapon_instance(kit, kit.capacity, RifleUpdate(kit.capacity, n, True, True)) for n in (1, 2))
        self.assertEqual((first.presence[muzzle], second.presence[muzzle]),
                         (Asset(0x0DC0000000000005), Asset(0x0DC0000000000007)))
        self.assertEqual((11 in first.active, 12 in first.active, 11 in second.active, 12 in second.active),
                         (True, False, False, True))
        self.assertTrue(set(DVA_HELD) <= set(first.active))
        released = weapon_instance(kit, kit.capacity, RifleUpdate(kit.capacity, 2, False, True))
        self.assertFalse(set(DVA_HELD) & set(released.active))
        idle = weapon_instance(kit, kit.capacity)
        self.assertNotIn(muzzle, idle.presence)
        # each cannon's time since its shot is ChaseVar 12's / 11's output, 100 while off; var233 is the firing flag
        self.assertEqual((idle.presence[kit.bits[350]], idle.presence[kit.bits[351]], first.presence[kit.bits[233]]),
                         (StateBound(Float(100), 1, 12, 6, per_frame=True), StateBound(Float(100), 1, 11, 6, per_frame=True),
                          Bool(True)))

    def test_dva_pilot_gun_settles_after_a_burst(self):
        from kits import DVA_PILOT
        kit, timer = DVA_PILOT, DVA_PILOT.bits[350]
        idle, firing = weapon_instance(kit, 20), weapon_instance(kit, 19, RifleUpdate(19, 1, True, True), fire_elapsed=50)
        self.assertEqual((idle.presence[timer], firing.presence[timer], firing.presence[kit.bits[233]]),
                         (Float(100), Float(100), Bool(True)))
        self.assertNotIn(10, firing.active)
        released = weapon_instance(kit, 19, RifleUpdate(19, 1, False, True), fire_elapsed=250)
        self.assertEqual((released.presence[timer], released.presence[kit.bits[233]], released.active[10].getvalue()),
                         (StateBound(Float(100), 1, 10, 6, per_frame=True), Bool(False), chase_since(250).getvalue()))

    def test_a_kit_without_a_volley_still_sends_a_shot(self):
        from kits import REINHARDT
        kit = KITS[REINHARDT]
        self.assertEqual(set(weapon_instance(kit, 5, RifleUpdate(5, 1, True, True)).active), set(kit.states))

    def test_winston_tesla_effect_lasts_through_the_burst(self):
        kit = KITS[WINSTON]
        route = {6, 10, 40, 42, 43}
        first = weapon_instance(kit, 99, RifleUpdate(99, 1, True, True))
        second = weapon_instance(kit, 98, RifleUpdate(98, 2, True, True))
        self.assertEqual(first.active.keys() & route, route)
        for state in (10, 43):
            self.assertEqual(first.active[state].count, 3)
            self.assertEqual(first.active[state].getvalue(), second.active[state].getvalue())
        for reloading in (False, True):
            stop = weapon_instance(kit, 98, RifleUpdate(98, 2, False, True, reloading))
            self.assertEqual(stop.active.keys() & route, {10, 43})
            self.assertEqual([(stop.active[state].count, stop.active[state].value) for state in (10, 43)],
                             [(3, 2), (3, 2)])

    def test_a_reload_flips_the_reload_branch_and_plays_its_anim(self):
        kit = KITS[CASSIDY]
        path, anim = kit.reload
        reloading = weapon_instance(kit, 2, RifleUpdate(2, 3, False, True, True))
        self.assertTrue(set(path) | {anim} <= set(reloading.active))
        self.assertEqual(reloading.active[1].getvalue(), _bits((1, 1)).getvalue())  # the reloading switch's true branch
        self.assertEqual(reloading.extra, {51: Bool(True)})
        done = weapon_instance(kit, 6, RifleUpdate(6, 3, False, True))
        self.assertNotIn(anim, done.active)
        self.assertEqual((done.active[1].getvalue(), done.extra), (_bits((0, 1)).getvalue(), {51: Bool(False)}))

    def test_ashe_sends_the_var_her_rifle_model_reads(self):
        from kits import ASHE
        kit = KITS[ASHE]
        self.assertEqual(weapon_instance(kit, 12).presence[kit.bits[16922]], Float(1.176))

    def test_alt_fire_spends_the_magazine_and_pulses_its_volley(self):
        from kits import AltFire, alt_states
        from soldier import Rifle
        from target import ALT_KIT
        kit, rifle = KITS[CASSIDY], Rifle(16666, PLAYER_KIT['Cassidy'][1])
        alt = AltFire(16666, rifle, *ALT_KIT['Cassidy'])
        self.assertTrue(alt.command(Command(100, buttons=2)))
        self.assertEqual((rifle.ammo, len(rifle.updates), len(alt.updates)), (5, 1, 2))
        self.assertFalse(alt.command(Command(101, buttons=2)))  # cadence
        start = alt_states(kit, alt.updates[0])
        self.assertEqual(set(start), set(kit.alt[0]) | {kit.alt[1]})
        instance = weapon_instance(kit, rifle.ammo, rifle.updates[0], start)
        self.assertIn(kit.alt[1], instance.active)
        rifle.ammo = 1
        alt.command(Command(200, buttons=2))
        self.assertTrue(rifle.updates[-1].reloading)  # an emptied magazine reloads
        self.assertFalse(alt.command(Command(300, buttons=2)))  # not while reloading

    def test_alt_fire_waits_for_room_to_update_ammo_and_reload(self):
        from kits import AltFire
        from soldier import MAX_UPDATES, Rifle
        from target import ALT_KIT

        rifle = Rifle(16666, PLAYER_KIT['Cassidy'][1])
        rifle.ammo = 1
        rifle.updates.extend([RifleUpdate(1, 0, False, True)] * (MAX_UPDATES - 1))
        alt = AltFire(16666, rifle, *ALT_KIT['Cassidy'])
        command = Command(100, buttons=2)

        self.assertFalse(alt.command(command))
        self.assertEqual((rifle.ammo, len(rifle.updates)), (1, MAX_UPDATES - 1))
        self.assertEqual((alt.serial, alt.ready_at), (0, 0))
        self.assertFalse(alt.updates)

        rifle.updates.popleft()
        self.assertTrue(alt.command(command))
        self.assertEqual((rifle.ammo, len(rifle.updates), len(alt.updates)), (0, MAX_UPDATES, 2))
        self.assertTrue(rifle.updates[-1].reloading)

    def test_moira_primary_owns_spray_and_explicit_stop(self):
        from kits import MOIRA

        kit = KITS[MOIRA]
        self.assertFalse(weapon_instance(kit, kit.capacity).active.keys() & {7, 8})
        for firing in (True, False):
            frame = weapon_instance(kit, kit.capacity, RifleUpdate(kit.capacity, 1, firing, True))
            self.assertEqual([(frame.active[state].count, frame.active[state].value) for state in (7, 8)],
                             [(3, 2 + firing), (3, 2 + firing)])

    def test_mei_icicle_waits_for_windup_and_cancel_preserves_ammo(self):
        from kits import AltFire, MEI, alt_states
        from soldier import Rifle
        from target import ALT_KIT

        kit = KITS[MEI]
        rifle = Rifle(10000, PLAYER_KIT['Mei'][1])
        alt = AltFire(10000, rifle, *ALT_KIT['Mei'], windup=kit.alt_windup)
        self.assertFalse(alt.command(Command(1, buttons=2)))
        self.assertFalse(alt.command(Command(20)))
        self.assertFalse(alt.command(Command(30, buttons=2)))
        self.assertFalse(alt.command(Command(69, buttons=2)))
        self.assertEqual(rifle.ammo, 120)
        self.assertTrue(alt.command(Command(70, buttons=2)))
        self.assertEqual(rifle.ammo, 110)
        self.assertIn(16, alt_states(kit, alt.updates[0]))
        self.assertFalse(alt.command(Command(110, buttons=2)))
        self.assertTrue(alt.command(Command(150, buttons=2)))
        self.assertEqual(rifle.ammo, 100)

    def test_symmetra_orb_is_the_right_mouse_after_a_full_charge(self):
        from kits import AltFire, SYMMETRA, alt_states, weapon_instance
        from soldier import Rifle, RifleUpdate
        from target import ALT_KIT

        kit = KITS[SYMMETRA]
        idle = weapon_instance(kit, 70)
        self.assertIn(40, idle.active)  # StateAbility above the right-mouse button, sent without a cooldown
        self.assertNotIn(34, weapon_instance(kit, 70, RifleUpdate(69, 1, True, True)).active)  # the beam: no orb
        rifle = Rifle(10000, PLAYER_KIT['Symmetra'][1])
        alt = AltFire(10000, rifle, *ALT_KIT['Symmetra'], windup=kit.alt_windup)
        self.assertFalse(alt.command(Command(1, buttons=2)))
        self.assertFalse(alt.command(Command(99, buttons=2)))  # 1 s charge at 10 ms frames
        self.assertTrue(alt.command(Command(101, buttons=2)))
        self.assertEqual(rifle.ammo, 60)
        self.assertIn(34, alt_states(kit, alt.updates[0]))

    def test_zenyatta_orbs_load_while_held_and_fire_on_release(self):
        from kits import ChargedAlt, ZENYATTA, alt_states
        from soldier import Rifle
        from target import ALT_KIT

        kit = KITS[ZENYATTA]
        rifle = Rifle(10000, PLAYER_KIT['Zenyatta'][1])
        orbs = ChargedAlt(10000, rifle, *ALT_KIT['Zenyatta'], windup=kit.alt_windup, most=kit.alt_most)
        self.assertFalse(orbs.command(Command(1, buttons=2)))
        self.assertFalse(orbs.command(Command(40)))  # let go under one orb's 0.524 s: nothing
        self.assertEqual(rifle.ammo, 20)
        self.assertFalse(orbs.command(Command(100, buttons=2)))
        self.assertFalse(orbs.command(Command(220, buttons=2)))
        self.assertTrue(orbs.command(Command(220)))  # 1.2 s held: two orbs
        self.assertEqual((rifle.ammo, orbs.weapon.pellets), (18, 2))
        self.assertEqual(alt_states(kit, orbs.updates[0]).keys(), {22, 18})
        self.assertFalse(orbs.command(Command(230, buttons=2)))
        self.assertFalse(orbs.command(Command(1300)))  # held from inside the 0.6 s recovery: not loading
        self.assertFalse(orbs.command(Command(1301, buttons=2)))
        self.assertTrue(orbs.command(Command(5000)))  # held far past five orbs: five
        self.assertEqual((rifle.ammo, orbs.weapon.pellets), (13, 5))

    def test_a_melee_press_swings_and_hurts_at_reach(self):
        from soldier import MELEE, MELEE_BUTTON, melee_instance
        melee = Volley(16666, MELEE_BUTTON, 1.0)
        idle = melee_instance(melee)
        self.assertEqual((idle.index, idle.graph, set(idle.active)), (67, MELEE, {5}))
        self.assertTrue(melee.command(Command(100, buttons=MELEE_BUTTON)))
        swing = melee_instance(melee, melee.updates[0])
        self.assertEqual(set(swing.active), {5, 6, 7, 1, 11})
        self.assertFalse(melee.command(Command(110, buttons=MELEE_BUTTON)))  # 1 s cooldown

    def test_ammo_stays_within_the_magazine(self):
        kit = KITS[CASSIDY]
        with self.assertRaises(ValueError):
            weapon_instance(kit, kit.capacity + 1)
        for kit in KITS.values():
            self.assertGreater(frame_builder(kit)(1, extra_active={}, weapon_entity=None).count, kit.graph.owner_states)


if __name__ == '__main__':
    unittest.main()


class AbilityTests(unittest.TestCase):
    def test_soldiers_biotic_field_derives_as_hand_mapped(self):
        # tools/assets/ability_kit.py gives (599, 21, 6, 15, 0, 0x10, [16], 3, 5, 15, 5) for Soldier's body
        field = Ability(Graph(599, 21, 6, 15), 0, FIELD_BUTTON, (16,), 3, 5, FIELD_COOLDOWN, 5)
        ours, theirs = ability_instance(field, Volley(16666, FIELD_BUTTON, FIELD_COOLDOWN)), field_instance()
        self.assertEqual((ours.presence, ours.active.keys()), (theirs.presence, theirs.active.keys()))

    def test_biotic_fields_throw_sends_its_animation(self):
        from soldier import RifleUpdate
        thrown = field_instance(RifleUpdate(0, 1, True, True)).active
        self.assertTrue({1, 2, 3, 4} <= thrown.keys())  # Volley 4, throw Anim 5, Waits 6 and 8
        self.assertEqual(thrown[2].count, 5)
        self.assertNotIn(2, field_instance().active)

    def test_a_press_starts_the_cooldown_sweep(self):
        ability = ABILITIES[CASSIDY][0]
        cooldown = Volley(16666, ability.button, ability.seconds)
        payload = ability_instance(ability, cooldown).active[ability.state]
        idle = payload().getvalue()
        self.assertTrue(cooldown.command(type('C', (), {'frame': 10, 'buttons': ability.button})()))
        cooldown.frame = 20
        self.assertNotEqual(payload().getvalue(), idle)  # the same instance, later: the sweep has run
        # 0x40: Pharah's Hover Jets, on the jump button
        # state None: a passive (Angelic Descent, Glide), which has no cooldown
        # cooldown_bit None: a toggle with no cooldown (Wrecking Ball's Roll)
        self.assertTrue(all(a.button in (0x02, 0x08, 0x10, 0x40) and (a.seconds > 0 or a.cooldown_bit is None)
                            for kit in ABILITIES.values() for a in kit if a.state is not None))

    def test_the_cooldown_binds_the_full_state_index(self):
        # Zarya's E: owner state 6 is graph state 9 (client-only states first); bound to 6 it showed no countdown
        zarya_e = ABILITIES[ZARYA][1]
        left = ability_instance(zarya_e, Volley(16666, zarya_e.button, zarya_e.seconds)).presence[zarya_e.left_bit]
        self.assertEqual((zarya_e.state, left.state), (6, 9))

    def test_defense_matrix_runs_while_held_and_its_meter_lasts(self):
        matrix = Held(10000, 0x02, 1.0, 100.0, 50.0, 16.0)  # 10 ms frames
        self.assertTrue(matrix.command(Command(1, buttons=0x02)))
        self.assertFalse(matrix.command(Command(101, buttons=0x02)))  # 1 s: half drained, still up
        self.assertAlmostEqual(matrix.meter, 50.0)
        self.assertTrue(matrix.command(Command(201, buttons=0x02)))  # empty after 2 s: it drops while held
        self.assertFalse(matrix.active)
        self.assertFalse(matrix.command(Command(250, buttons=0x02)))  # its 1 s cooldown runs
        matrix.command(Command(400))  # refills 16/s once the cooldown is over (frame 301)
        self.assertAlmostEqual(matrix.meter, 16.0 * 0.99, places=3)
        running = ability_instance(ABILITIES[DVA][2], matrix).active
        self.assertEqual(running[4].getvalue(), _bits((0, 1), (1, 2)).getvalue())  # effect stopped, Begin 0

    def test_boosters_run_two_seconds_and_a_second_press_cancels(self):
        boost = Timed(10000, 0x08, 4.0, 2.0, 0.4, 11.99)
        self.assertTrue(boost.command(Command(1, buttons=0x08)))
        self.assertFalse(boost.command(Command(30, buttons=0x08)))  # held: no second press
        self.assertFalse(boost.command(Command(31)))
        self.assertFalse(boost.command(Command(32, buttons=0x08)))  # pressed before 0.4 s: no cancel
        self.assertTrue(boost.command(Command(201)))  # 2 s: over, its cooldown starts
        self.assertFalse(boost.active)
        self.assertTrue(boost.command(Command(601, buttons=0x08)))  # 4 s later: again
        self.assertTrue(boost.command(Command(650)) or boost.command(Command(651, buttons=0x08)))  # cancelled

    def test_boosters_ring_clock_is_read_when_the_frame_is_sent(self):
        # built once at the press it stayed 0.0 in every resend, and the centre ring (UX 32) never drew
        from ow174.game.statescript import owner_full_frame
        boosters = ABILITIES[DVA][0]
        boost = Timed(10000, 0x08, 4.0, 2.0, 0.4, 11.99)
        boost.command(Command(1, buttons=0x08))
        instance = ability_instance(boosters, boost)
        boost.command(Command(51))  # 0.5 s into the 2 s flight
        self.assertEqual(instance.presence[boosters.clock](), Float(1.5))  # seconds left: the ring counts down
        owner_full_frame(1, [instance], {})  # the send resolves it

    def test_a_cast_pulses_its_animation_state(self):
        mine = ABILITIES[JUNKRAT][0]  # Concussion Mine: path 10 -> Volley 8; its throw is Anim 2 under state 10
        clock = Charges(16000, mine.button, mine.seconds, mine.charges)
        clock.command(Command(100, buttons=0x08))
        active = ability_instance(mine, clock, clock.updates[0]).active
        self.assertEqual((active[10], active[2].getvalue()), (None, bytes(1)))
        self.assertEqual(active[2].count, 5)  # a 4-bit payload ate the next state's bits
        self.assertNotIn(2, ability_instance(mine, clock).active)  # off again after the stop frame

    def test_charge_abilities_send_their_charges(self):
        # without var11 Tracer's Blink drew a red crossed icon (no charges)
        blink = next(a for a in ABILITIES[TRACER] if a.button == 0x08)
        clock = Charges(16000, blink.button, blink.seconds, blink.charges)
        presence = ability_instance(blink, clock).presence
        self.assertEqual((presence[0], presence[8]), (Float(3.0), Float(3.0)))
        clock.command(Command(100, buttons=0x08))
        self.assertEqual(ability_instance(blink, clock).presence[0], Float(2.0))  # var11 counts the charges left

    def test_charges_spend_per_press_and_come_back_one_every_cooldown(self):
        clock = Charges(16000, 0x08, 3.0, 3)  # Blink: 3 charges, 3 s each = 188 frames
        self.assertTrue(clock.command(Command(100, buttons=0x08)))
        self.assertFalse(clock.command(Command(101, buttons=0x08)))  # held, not pressed again
        self.assertTrue(clock.command(Command(102, buttons=0)) is False and clock.command(Command(103, buttons=0x08)))
        self.assertEqual((clock.charges, clock.ready_at), (1, 288))  # the clock started at the first press
        clock.command(Command(104, buttons=0)); self.assertTrue(clock.command(Command(105, buttons=0x08)))
        self.assertEqual(clock.charges, 0)
        self.assertFalse(clock.command(Command(107, buttons=0x08)))  # none left
        clock.command(Command(200, buttons=0))
        self.assertEqual(clock.ability().getvalue(), ability_payload(0, (3.0, (188 - 88) * 16000 // 1000)).getvalue())
        clock.command(Command(288, buttons=0))
        self.assertEqual((clock.charges, clock.ready_at), (1, 476))  # one back, the next on its way
        clock.command(Command(700, buttons=0))
        self.assertEqual((clock.charges, clock.ready_at), (3, 664))  # full: no sweep, one more resend
        self.assertEqual(clock.ability().getvalue(), ability_payload().getvalue())
        self.assertTrue(clock.showing_cooldown())  # the HUD got stuck at 2 without the resend after the last return
        clock.command(Command(800, buttons=0))
        self.assertFalse(clock.showing_cooldown())


class ModelEntityTests(unittest.TestCase):
    def test_the_weapon_binding_counts_every_state_of_the_graph(self):
        # Lucio's graph: 65 states, 61 owner states; 6 bits crashed the client (frame misaligned)
        self.assertEqual(model_entity(KITS[LUCIO]).state_bits, 7)


class StatsTests(unittest.TestCase):
    def test_reload_length_var_is_the_graphs_own(self):
        # var581 as each weapon graph's Entry sets it (pinned catalog): the reload anim is stretched over it
        from kits import ROADHOG
        names = {hero.guid: hero.name for hero in heroes().values()}
        authored = {ROADHOG: 2.0, TRACER: 1.0, CASSIDY: 1.5}
        for guid, seconds in authored.items():
            kit = KITS[guid]
            sent = PLAYER_KIT[names[guid]][1].reload_seconds
            self.assertEqual(weapon_instance(kit, 0, reload_seconds=sent).presence[kit.bits[581]], Float(seconds),
                             names[guid])

    def test_weapon_frames_carry_what_the_script_sets_on_an_idle_body(self):
        # var476 (the Volley's damage) and var581 (the reload's length): where the hero's own script has set one on an
        # idle body (the vendored VM runs it), the frame carries the same. 20 and 1.5 went out before, whoever the hero
        import logging
        from ow174.game.script.driver import BodyScript
        names = {hero.guid: hero.name for hero in heroes().values()}
        logging.disable(logging.CRITICAL)
        self.addCleanup(logging.disable, logging.NOTSET)
        checked = set()
        for guid, kit in KITS.items():
            script = BodyScript(guid, 0xA0000102, 1000, health=None, ult_seconds=0)
            script.command(1001, 0, 0, 0, 0)
            scripted = next((i.vars for i in script.component.instances.values() if i.graph.index == kit.graph.index), {})
            sent = weapon_instance(kit, 0, reload_seconds=PLAYER_KIT[names[guid]][1].reload_seconds).presence
            for var in (476, 581):
                value = scripted[var].value() if var in kit.bits and var in scripted else None
                if value is not None:
                    checked.add((names[guid], var))
                    with self.subTest(hero=names[guid], var=var):
                        self.assertAlmostEqual(sent[kit.bits[var]].value, value, 5)
        self.assertLessEqual({('Sombra', 476), ('Baptiste', 476), ('Wrecking Ball', 476), ('Sigma', 581)}, checked)

    def test_every_kit_hero_has_player_stats_with_its_magazine(self):
        names = {hero.guid: hero.name for hero in heroes().values()}
        for guid, kit in KITS.items():
            self.assertEqual(PLAYER_KIT[names[guid]][1].rounds, kit.capacity, names[guid])
