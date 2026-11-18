"""Observe one prepared Soldier room against the frozen Python reference.

The caller supplies decoded commands and drives one production room tick per call.
The production owner delivery path sends to memory and acknowledges committed groups.
"""

from golden_python_delivery import OwnerDelivery, bits
from tests.runtime.test_server_journal import FixedSource

from ow174.game import collision, content
from ow174.game.commands import Command
from ow174.game.match import Match
from ow174.game.script import expr
from ow174.game.script.driver import OWNER_HEALTH
from ow174.game.script.nodes import WeaponVolley
from ow174.game.server import FIRST_TICK


class RoomClock(FixedSource):
    """Keep the fixture's deterministic entropy and advance all clocks with the schedule."""

    elapsed_us = 0

    def wall_time(self, *, callsite):
        return super().wall_time(callsite=callsite) + self.elapsed_us / 1_000_000

    def monotonic(self, *, callsite):
        return super().monotonic(callsite=callsite) + self.elapsed_us / 1_000_000

    def wall_time_ns(self, *, callsite):
        return super().wall_time_ns(callsite=callsite) + self.elapsed_us * 1000


def rifle_ammo(rifle):
    """Read stored ammo or its known pure volley binding without evaluating a VM graph."""
    var = rifle.vars.get(53)
    if var is None:
        return None
    active = [link for link in var.links if link.state.active]
    if not active:
        return var.base if type(var.base) is int else None
    if len(active) == 1 and type(active[0].state) is WeaponVolley and active[0].slot == 0:
        volley = active[0].state
        return volley.ammo_at(volley.now - volley.start - volley.offset)
    return None


def rifle_reloading(rifle):
    """The authored rifle tests !#51 and writes only booleans during reload."""
    var = rifle.vars.get(51)
    if var is None or any(link.state.active for link in var.links):
        return None
    # An existing Null is the initial stored value; the graph's boolean
    # expression treats it as false before the first reload begins.
    return expr.truthy(var.base) if var.base is None or type(var.base) is bool else None


class PythonRoom:
    """A single stock Soldier and an identified Practice Range spawner, after selection."""

    def __init__(self, bot_slot):
        scene = content.PRACTICE_RANGE
        terrain = collision.world_for(scene.map_guid, scene.name, build_missing=False, mode=scene.mode_guid)
        if terrain is None:
            raise ValueError("prepared Practice Range collision is unavailable")
        provider = collision.StaticCollisionProvider(terrain)
        self.clock = RoomClock()
        self.seed = self.clock.counter
        self.match = Match(scene, environment=self.clock, collision_provider=provider)
        self.player = self.match.add_player(1, "player", content.SOLDIER, 0, False)
        self.match.tick = FIRST_TICK
        self.start_tick = self.match.tick
        self.delivery = OwnerDelivery(self.match, self.player)
        if not self.match.switch_hero(self.player, self.player.hero) or self.player.body_script is None:
            raise ValueError("stock Soldier failed to initialize")
        if not 0 <= bot_slot < len(self.match.bots) or self.match.bots[bot_slot].team == self.player.team:
            raise ValueError("select a hostile Practice Range bot slot")
        self.spawner = self.match.bots[bot_slot].spawner
        self.bot = self.match.bots[bot_slot]
        self.bot_generation = 1
        self.player_script = self.player.body_script
        self.player_entity = self.player.body
        self.player_generation = 1
        self.ambiguous_player_body = False
        self.identities = {}
        self.owner_effects = None

    def _remember(self, entity, actor):
        if self.identities.setdefault(entity, actor) != actor:
            self.identities[entity] = None

    def step(self, command):
        """Take at most one semantic submission before one production room update."""
        self.clock.elapsed_us = (self.match.tick - FIRST_TICK) * self.player.quanta
        before = self.player.commands.last_frame
        boundary = None
        if command is not None:
            if not isinstance(command, Command):
                raise ValueError("submit a decoded reference Command")
            if not self.player.spawned or self.player.gone or not self.player.has_body:
                raise ValueError("command target body is absent")
            boundary = self.player.take_commands([command])
        after = self.player.commands.last_frame
        accepted = command is not None and boundary is None and before != after and after == command.frame
        now = self.match.started + self.clock.elapsed_us / 1_000_000
        self.match.update(now, self.match.tick + 1)
        state, missing = self.observe()
        self.owner_effects, delivery_gaps = self.delivery.transmit(self.match.tick, now, self.identities)
        return accepted, (state, sorted(set(missing) | delivery_gaps))

    def observe(self):
        """Return available comparator state rows and explicit observation gaps."""
        rows = []
        missing = set()
        p = self.player
        body = p.body_script
        if not p.gone and p.has_body and body is None:
            missing.add("player.body.vm")
        if p.has_body and not p.gone and body is not None:
            if p.body != self.player_entity or self.player_script is None:
                self.player_generation += 1
                self.ambiguous_player_body = False
            elif self.player_script is not None and body is not self.player_script:
                self.ambiguous_player_body = True
            if self.ambiguous_player_body:
                missing.add("player.body.lifecycle")
            self.player_entity, self.player_script = p.body, body
            key = f"player/0/body/{self.player_generation}"
            self._remember(p.body, key)
            rows.extend([
                ["player/0", "body", key],
                [key, "accepted_frame", p.commands.last_frame],
                [key, "aim", [p.mover.yaw, p.mover.pitch]],
                [key, "alive", not self.match.combat.is_dead(p)],
                [key, "position_bits", [bits(n) for n in p.mover.position]],
            ])
            vital = self.match.combat.vitals.get(p.body)
            shared_health = body.component.vars.get(OWNER_HEALTH)
            if vital is not None:
                rows.append([key, "health_bits", bits(vital.total)])
            elif (shared_health is not None and not shared_health.links
                  and type(shared_health.base) in (int, float)):
                rows.append([key, "health_bits", bits(shared_health.base)])
            else:
                missing.add("player.health")
            rifles = [
                instance for instance in body.component.instances.values()
                if instance.graph.index == 0x0254 and instance.parent is None
            ]
            ammo = rifle_ammo(rifles[0]) if len(rifles) == 1 else None
            reloading = rifle_reloading(rifles[0]) if len(rifles) == 1 else None
            if ammo is None:
                missing.add("player.ammo")
            else:
                rows.append([key, "ammo", ammo])
            if reloading is None:
                missing.add("player.reloading")
            else:
                rows.append([key, "reloading", reloading])
        elif p.gone or not p.has_body:
            rows.append(["player/0", "body", None])
            self.player_script = None
        matches = [bot for bot in self.match.bots if bot.spawner is self.spawner]
        if len(matches) > 1:
            missing.add("bot.body.lifecycle")
        elif not matches:
            rows.append(["bot/0", "body", None])
            self.bot = None
        else:
            bot = matches[0]
            if bot is not self.bot:
                self.bot_generation += 1
                self.bot = bot
            key = f"bot/0/body/{self.bot_generation}"
            self._remember(bot.entity, key)
            vital = self.match.combat.vitals.get(bot.entity)
            rows.extend([
                ["bot/0", "body", key],
                [key, "alive", not bot.dead],
                [key, "position_bits", [bits(n) for n in bot.position]],
            ])
            if vital is not None:
                rows.append([key, "health_bits", bits(vital.total)])
            elif not bot.dead:
                # Combat.body_vitals lazily initializes the bot's sole pool from this field.
                rows.append([key, "health_bits", bits(bot.health)])
            else:
                missing.add("bot.health")
        return sorted(rows), sorted(missing)
