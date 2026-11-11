"""Target-build Quad Cannons through the shared body and combat paths."""

import unittest

from test_game_combat import KANEZAKA_DM, Run, aim

from ow174.game import combat
from ow174.game.commands import FIRE
from ow174.game.content import heroes

WRECKING_BALL = 0x02E00000000001CA
QUAD_CANNONS = 0x1514
AMMO = 53
RELOAD = 0x400


class WreckingBallCannonsTests(unittest.TestCase):
    def setUp(self):
        self.run = Run(KANEZAKA_DM, ("Wrecking Ball", "Target"))
        self.match = self.run.match
        self.match.bots.clear()
        self.match._pending_bot_scripts.clear()
        self.ball, self.victim = self.run.players
        self.assertTrue(self.match.switch_hero(self.ball, heroes()[WRECKING_BALL]))
        x, y, z = self.ball.mover.position
        self.victim.mover.position = [x, y, z + 2.0]
        self.fight = self.match.combat
        self.yaw, self.pitch = aim(self.fight._eye(self.ball), (x, y + 1.1, z + 2.0))
        self.advance(24)
        target = next(item for item in self.fight._targets() if item.owner is self.victim)
        self.vitals = self.fight.vitals_of(target)
        self.weapon = next(
            instance for instance in self.ball.body_script.component.instances.values()
            if instance.graph.index == QUAD_CANNONS
        )

    def advance(self, ticks, buttons=0):
        for _ in range(ticks):
            self.run.step({self.ball: (self.yaw, self.pitch, buttons)})

    def test_held_primary_spends_ammo_hits_and_stops_on_release(self):
        self.assertEqual(self.weapon.vars[AMMO].value(), 80)
        self.advance(9, FIRE)
        self.assertEqual((self.weapon.vars[AMMO].value(), self.vitals.total), (76, 180.0))

        self.advance(24)
        self.assertEqual((self.weapon.vars[AMMO].value(), self.vitals.total), (76, 180.0))
        self.advance(1, RELOAD)
        self.advance(160)
        self.assertEqual(self.weapon.vars[AMMO].value(), 80)

        self.advance(1, FIRE)
        self.assertEqual((self.weapon.vars[AMMO].value(), self.vitals.total), (79, 175.0))

    def test_primary_uses_authored_damage_falloff(self):
        volley = self.weapon.state(9)
        hit = combat.hit_def(volley, volley.node.field("m_6396149F"))
        self.assertEqual((hit.amount, hit.falloff_start, hit.falloff_end), (5.0, 15.0, 25.0))
        self.assertAlmostEqual(hit.scale(20.0), 0.65)
        self.assertAlmostEqual(hit.scale(30.0), 0.30)
