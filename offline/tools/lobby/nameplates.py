"""Retail D24 name/health presenter, hosted on each bot's stable controller.

Entity var 29415 is the authored target-body override (client Stack 71 -> instance var 8154).
Create this graph only after the body create is acknowledged. After the graph's own acknowledgement,
start client-only UX state 35 through the unfiltered owner-delta table.
Full-frame state readers skip client-only states. No native code or game assets are changed.
"""
from ow174.game.bits import BitWriter
from ow174.game.statescript import (Bool, Entity, Graph, Instance, _delta_mask,
    remote_full_frame)

GRAPH = Graph(0xD24, 3, 4, 3)


def setup(bot):
    return (Instance(1, GRAPH, source_entity=bot.entity, presence={0: Bool(True), 3: Bool(True)}),
            {29415: Entity(bot.entity), 478: Bool(False), 11264: Bool(False), 20017: Bool(True)})


def create(bot):
    instance, variables = setup(bot)
    return remote_full_frame([instance], variables)


def show(cmfd):
    changes = BitWriter()
    _delta_mask(changes, 32, [])
    changes.bit(0)  # native Stack 71 owns the body binding
    _delta_mask(changes, 77, [35])
    changes.bit(1)
    payload = BitWriter()
    payload.w_u16(1)
    payload.bits(0, 2)  # existing instance
    payload.w_u16(0)
    payload.bit(0)  # no entity variables
    payload.append(changes)
    payload.bit(0)  # no instance events
    out = BitWriter()
    out.bits(0b010, 3)
    out.w_var(cmfd)
    out.w_var(payload.count)
    out.append(payload)
    out.bits(0, 3)  # no more subframes or frame events
    return out
