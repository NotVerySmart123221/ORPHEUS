"""Observe successful owner delivery groups from the frozen Python game server."""

import math
import struct
from weakref import WeakKeyDictionary

from ow174.game import messages, world
from ow174.game.bits import BitReader
from ow174.game.link import ACK, SYN, LinkCipher, Packet
from ow174.game.script import decode
from ow174.game.server import Client
from tests.runtime.test_game_health import bar


def bits(value):
    if not math.isfinite(value):
        raise ValueError("nonfinite simulation value")
    return struct.pack(">d", value).hex()


class MemorySink:
    """Hold the most recent send until the in-memory peer receives it."""

    def sendto(self, data, address):
        self.last = data
        return len(data)


class OwnerDelivery(Client):
    def __init__(self, *args):
        super().__init__(*args)
        self.health_records = WeakKeyDictionary()

    @classmethod
    def join(cls, host, ticket, now):
        from unittest.mock import patch

        address = ("127.0.0.1", 1)
        peer = LinkCipher(ticket.key_out, ticket.key_in)
        with patch("ow174.game.server.Client", cls):
            host._datagram(peer.seal(Packet(ticket.conn, flags=SYN)), address, now)
        delivery = host.clients[ticket.conn]
        hello = peer.open(host.sock.last)
        if hello is None or hello.flags != SYN | ACK:
            raise ValueError("room handshake did not complete")
        delivery.peer_cipher = peer
        delivery.peer_sequence = 1
        delivery.confirm(hello.seq, now)
        return delivery

    def confirm(self, sequence, now):
        ack = Packet(self.conn, seq=self.peer_sequence, flags=ACK, ack=(sequence + 1) % 2**32, ack_bits=1)
        self.server._datagram(self.peer_cipher.seal(ack), self.address, now)
        self.peer_sequence += 1

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

    def received_frame(self, previous_sequence, now, identities, on_receipt=None):
        """Project only records selected by send_frame after its successful send."""
        self.wire_frame = None
        if self.seq == previous_sequence:
            return [], set()
        packet = self.peer_cipher.open(self.server.sock.last)
        if packet is None or packet.seq != previous_sequence:
            raise ValueError("owner frame was not received")
        sequence = packet.seq
        reader = BitReader(packet.payload)
        tick = reader.var_a()
        messages.read_channel(reader)
        self.wire_frame = {"room_tick": tick, "command_ack": reader.var_a()}
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
        if on_receipt is not None:
            pending = self._pending_possessions.get(self.player.entity)
            body = self.player.body_script
            script = (body.stream.full_update, body.stream.initial_owner) if body else (None, None)
        self.confirm(sequence, now)
        if on_receipt is not None:
            on_receipt(sent[1], pending, script)
        if missing:
            return None, missing
        return ([group] if group else []), set()
