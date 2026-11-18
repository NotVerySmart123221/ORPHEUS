"""Observe successful owner delivery groups from the frozen Python game server."""

import math
import struct
from weakref import WeakKeyDictionary

from ow174.game import world
from ow174.game.link import LinkCipher
from ow174.game.match import sends
from ow174.game.script import decode
from ow174.game.server import Client, GameServer
from tests.runtime.test_game_health import bar
from tests.runtime.test_game_packet_budget import KEY_IN, KEY_OUT


def bits(value):
    if not math.isfinite(value):
        raise ValueError("nonfinite simulation value")
    return struct.pack(">d", value).hex()


class MemorySink:
    """Accept the server's sends in memory without retaining datagrams or addresses."""

    def sendto(self, data, address):
        return len(data)


class OwnerDelivery(Client):
    def __init__(self, match, player):
        host = GameServer(environment=match.environment)
        host.sock = MemorySink()
        super().__init__(host, ("memory.test", 1), 1, LinkCipher(KEY_IN, KEY_OUT), player)
        player.client = self
        player.spawned = True
        player.steps_done = 4
        self.open = True
        self.health_records = WeakKeyDictionary()
        self.queue_ownership()
        self.queue_entities(match.spawn_updates(player))
        match.combat.spawned(player)

    def queue_body_health(self, entity, components):
        previous = self._body_health.get(entity)
        super().queue_body_health(entity, components)
        update = self._body_health.get(entity)
        if update is not None and update is not previous:
            # The queue can supersede this exact update before it is sent.
            self.health_records[update] = tuple(components[51])

    @staticmethod
    def _ammo(update):
        """Read an explicitly encoded magazine variable using that chunk's graph table."""
        pending = getattr(update.stream, "pending", {}).get(update.chunk_last)
        if pending is None:
            raise ValueError("unknown graph table for owner chunk")
        table = pending[0].instances
        roots = [idx for idx, (graph, parent) in table.items() if graph == 0x0254 and parent is None]
        if len(roots) != 1:
            raise ValueError("unknown rifle identity in owner chunk")
        # The owner receives an initial tree in the remote-frame format before
        # its owner deltas. Both can explicitly carry the magazine variable.
        graphs = {idx: entry[0] for idx, entry in table.items()}
        frame = decode.read_chunk(update.chunk.getvalue(), update.chunk.count, graphs)
        item = frame.instances.get(roots[0])
        if item is None or 53 not in item.vars:
            return None
        amount, _ = item.vars[53]
        if type(amount) is not int or not 0 <= amount <= 65535:
            raise ValueError("unsupported encoded magazine")
        return amount

    def transmit(self, tick, now, identities):
        """Project only records selected by send_frame after its successful send."""
        if not sends(tick):
            return [], set()
        sequence = self.seq
        self.send_frame(tick, now)
        sent = self.in_flight.get(sequence)
        if sent is None:
            return None, {"effects.grouped_delivery"}
        group = []
        missing = set()
        for update in sent[1]:
            if update.entity not in identities:
                continue
            actor = identities[update.entity]
            if actor is None:
                missing.add("effects.body_generation")
                continue
            # The script channel precedes the entity operation within a record.
            if actor.startswith("player/0/") and update.chunk is not None:
                try:
                    amount = self._ammo(update)
                except ValueError:
                    missing.add("effects.owner_ammo")
                else:
                    if amount is not None:
                        group.append(["owner", "ammo", actor, amount])
            if update.op in (world.OP_CREATE, world.OP_DESTROY):
                kind = "create" if update.op == world.OP_CREATE else "retire"
                group.append(["owner", kind, actor, None])
            if update in self.health_records:
                plain, over = bar(self.health_records[update])
                health = sum(current for current, _ in plain) + sum(over)
                group.append(["owner", "health", actor, bits(health)])
        self.acked((sequence + 1) % 2**32, 1)
        if missing:
            return None, missing
        return ([group] if group else []), set()
