"""Statescript on ch1: frames that put graph instances on an entity and set their states and
variables. The client runs the graphs; the server says which instances exist and what is on.

Read in the client (our IDA; the chunk reader 0x7FF7898A6450, the frame 0x7FF78991E3C0):
- A chunk is `w_u32 first | w_var span | bit fragmented | w_var size in bits | payload`. first 0 is a
  full frame. The client keeps the highest `last` (first + span) it applied and drops chunks at or
  below it, so an unchanged chunk can be sent again safely.
- The payload starts `bit lead | bit H`; with H (an owner frame): `bit C | w_var CmFD`, and in a delta
  also `w_var LBSS` (the body's length). An owner frame is skipped unless its CmFD is larger than the
  last one (unsigned, so 0 never applies).
- A full frame lists the instances (w_u16 index steps, each with a descriptor: 16 bits graph, bit other
  entity, bit flag, bit parent -> w_var parent instance and w_var parent STATE index, bit references),
  then the entity's variables, then per instance its variables, one bit per state, and in an owner
  frame its event list; then the frame's event list. It destroys the network instances it does not
  list and first clears the variables the network set, so every full frame carries everything.
- In an owner frame the states are the graph's m_states minus the client-only and server-only ones.
  A SubScript state that is on carries its child's instance id (w_u16) and a bit.
- An owner delta lists changed instances and masks their changed variable and state slots. Those
  masks use the unfiltered tables, including gaps and client-only slots.
- Instances the server makes never run their own Entry nodes (only client-only ones run), so the
  server sends what those would have done: the states that are on and the variables.
"""

import json
import struct
from dataclasses import dataclass, field, replace
from functools import lru_cache
from pathlib import Path

from ow174.game.bits import BitWriter
from ow174.game.world import EntityUpdate


@dataclass(frozen=True)
class Graph:
    index: int  # the 01B graph's 16-bit index
    owner_states: int  # state bits in an owner frame
    sync_vars: int  # per-instance variable presence bits
    remote_states: int = 0  # m_remoteSyncNodes; distinct from the owner state list
    # Remote slots the client reads no presence bit for (reader 0x7FF6D1C2BE30 skips an entry that is not a state
    # node or is client-/server-only); tools/assets/graph_layout.py lists them. A slot after one of these is
    # misread unless it is skipped here too.
    silent: frozenset = frozenset()


# From the graph data (the counts match what worked in ProCore's live tests).
CONTROLLER = Graph(0x13C1, 43, 12)  # the player's controller in the Practice Range mode
PVP_CONTROLLER = Graph(0x0C90, 118, 29)  # the controller of both teams in the PvP modes
HUD = Graph(0x20E4, 1, 0)  # 13C1's weapon and ability HUD (0C90 starts its own)
HERO_SELECT_HOST = Graph(0x288A, 6, 0)
HERO_SELECT = Graph(0x288B, 30, 4)  # presents the hero select screen 008C.05A
TEAM_ENTRY = Graph(0x288D, 0, 0)  # client-only: posts its entity to the local player's team list
# The game mode script of Assault, Escort and Hybrid (the modes' m_CF63B633): the server creates it on the game
# mode entity. Counts from the graph: owner = states - client-only - server-only, sync vars = instance scope.
OBJECTIVE_MODE = Graph(0x0F4A, 165, 59, 102, frozenset({37, 38, 45, 73, 74, 75, 76, 77, 80, 89, 90, 91, 92, 93, 94}))
# 0F4A's SubScripts the server starts (the client starts only the client-only ones, such as the objective
# widget 1FEB): 0CBB (state 12) shows the phase banners, "ATTACK OBJECTIVE A"; 0C35 (state 49) is the
# round-complete screen.
ANNOUNCER, ANNOUNCER_STATE = Graph(0x0CBB, 69, 13, 50, frozenset({45, 46})), 12
ROUND_COMPLETE, ROUND_COMPLETE_STATE = Graph(0x0C35, 24, 27, 10, frozenset({0})), 49
# The objective entity's script (entity 0x1C22 on Hanamura): its client-only SubScripts (to 1824) draw the
# objective icons under the widget. Counts from tools/assets/graph_layout.py.
OBJECTIVE = Graph(0x0E8E, 59, 31, 29, frozenset({19, 21, 22, 24, 25, 26}))


class Value:
    tag: int

    def write(self, out: BitWriter) -> None:
        raise NotImplementedError


@dataclass(frozen=True)
class Bool(Value):
    value: bool

    def write(self, out: BitWriter) -> None:
        out.bits(0, 4)
        out.bit(self.value)


@dataclass(frozen=True)
class Int(Value):
    value: int

    def write(self, out: BitWriter) -> None:
        out.bits(4, 4)
        out.bits(self.value & 0xFFFFFFFF, 32)


@dataclass(frozen=True)
class Float(Value):
    value: float

    def write(self, out: BitWriter) -> None:
        out.bits(8, 4)
        out.bits(struct.unpack("<I", struct.pack("<f", self.value))[0], 32)


@dataclass(frozen=True)
class Vec3(Value):
    """A vector (tag 12, as LizRollin's script/wire.py writes it): a bit per part that is not 0, then those parts."""

    x: float
    y: float
    z: float

    def write(self, out: BitWriter) -> None:
        out.bits(12, 4)
        parts = (self.x, self.y, self.z)
        for part in parts:
            out.bit(part != 0.0)
        for part in parts:
            if part != 0.0:
                out.bits(struct.unpack("<I", struct.pack("<f", part))[0], 32)


@dataclass(frozen=True)
class Asset(Value):
    """A GUID read as an asset (a hero, for example): its type bits and its low 32 bits."""

    guid: int

    def write(self, out: BitWriter) -> None:
        out.bits(9, 4)
        out.bit(1)
        out.bits(self.guid >> 48, 16)
        if self.guid >> 48:
            out.w_var(self.guid & 0xFFFFFFFF)


@dataclass(frozen=True)
class Entity(Value):
    entity: int

    def write(self, out: BitWriter) -> None:
        out.bits(13, 4)
        out.bit(1)
        out.bits(self.entity & 0xFFFFFFFF, 32)


@dataclass(frozen=True)
class Array(Value):
    """A plain list (tag 15, subtype 6; reader 0x7FF6D1F77430): count, not keyed, each element tagged."""

    items: tuple

    def write(self, out: BitWriter) -> None:
        out.bits(15, 4)
        out.bits(6, 3)
        out.w_u16(len(self.items))
        out.bit(0)  # not keyed
        if self.items:
            out.bit(0)  # elements keep their own tags
        for item in self.items:
            item.write(out)


@dataclass(frozen=True)
class ScriptRef(Value):
    instance: int
    state: int

    def write(self, out: BitWriter) -> None:
        if (type(self.instance) is not int or not 1 <= self.instance <= 65535
                or type(self.state) is not int or not 0 <= self.state <= 65535):
            raise ValueError('invalid statescript reference')
        out.bits(14, 4)
        out.bits(4, 3)  # native state-reference handle kind, observed on the HUD's own output
        out.w_var(self.instance)
        out.w_u16(self.state)
        out.w_u16(0)


@dataclass(frozen=True)
class StateBound(Value):
    value: Value
    instance: int
    state: int
    state_bits: int
    per_frame: bool = False

    def __post_init__(self):
        if (not isinstance(self.value, Value) or isinstance(self.value, StateBound)
                or type(self.instance) is not int or not 1 <= self.instance <= 32767
                or type(self.state_bits) is not int or not 1 <= self.state_bits <= 9
                or type(self.state) is not int or not 0 <= self.state < 1 << self.state_bits
                or type(self.per_frame) is not bool):
            raise ValueError('invalid state output binding')

    def write(self, out: BitWriter) -> None:
        self.value.write(out)

    def write_binding(self, out: BitWriter) -> None:
        out.bits(1, 2)  # compact binding count1, not a presence flag
        out.bit(0)  # compact parameter count0
        out.bit(1)  # explicit instance
        out.bit(0)  # positive signed instance ID
        out.w_u16(self.instance)
        out.bits(self.state, self.state_bits)  # m_states index; in an H=0 frame an m_remoteSyncNodes slot
        out.bits(0, 2)  # default output index and priority
        out.bit(self.per_frame)  # time-dependent output
        if self.per_frame:
            out.bit(1)  # cache only for the current native context frame


def write_variable(out: BitWriter, value: Value) -> None:
    value.write(out)
    if isinstance(value, StateBound):
        value.write_binding(out)
    else:
        out.bit(0)  # compact binding count0


def subscript(child: int) -> BitWriter:
    """The payload of a SubScript state that is on: its child instance."""
    out = BitWriter()
    out.w_u16(child)
    out.bit(0)
    return out


@dataclass
class Instance:
    index: int
    graph: Graph
    parent: tuple[int, int] | None = None  # (parent instance, the parent graph's state index)
    presence: dict[int, Value] = field(default_factory=dict)  # presence bit -> value
    extra: dict[int, Value] = field(default_factory=dict)  # variable id -> value
    active: dict[int, BitWriter | None] = field(default_factory=dict)  # selected owner/remote table slot -> payload
    source_entity: int | None = None  # optional packed entity in the native graph descriptor


def _descriptor(out: BitWriter, instance: Instance) -> None:
    source = instance.source_entity
    if source is not None and (type(source) is not int or not 0x80000000 <= source <= 0xFFFFFFFF
            or source & 0x1FFFFFFF in (0, 0x1FFFFFFF)):
        raise ValueError('invalid descriptor entity')
    out.bits(instance.graph.index, 16)
    out.bit(source is not None)
    if source is not None:
        out.entity_id(source)
    out.bit(0)  # flag
    if instance.parent is None:
        out.bit(0)
    else:
        out.bit(1)
        out.w_var(instance.parent[0])
        out.w_var(instance.parent[1])
    out.bit(0)  # no references


def _variables(out: BitWriter, variables: dict[int, Value]) -> None:
    for number, value in variables.items():
        out.w_var(number)
        write_variable(out, value)
    out.w_var(0)


def owner_full_frame(cmfd: int, instances: list[Instance], entity_vars: dict[int, Value]) -> BitWriter:
    """An owner (H=1) full frame. `cmfd` must be larger than any CmFD sent to this entity before."""
    if cmfd < 1:
        raise ValueError("CmFD 0 is always stale")
    out = BitWriter()
    out.bit(0)  # lead
    out.bit(1)  # H
    out.bit(0)  # C
    out.w_var(cmfd)
    last = 0
    for instance in sorted(instances, key=lambda item: item.index):
        out.w_u16(instance.index - last)
        last = instance.index
        _descriptor(out, instance)
    out.w_u16(0)
    if entity_vars:
        out.bit(1)
        _variables(out, entity_vars)
    else:
        out.bit(0)
    for instance in sorted(instances, key=lambda item: item.index):
        for bit in range(instance.graph.sync_vars):
            value = instance.presence.get(bit)
            if callable(value):  # built at send time, as active payloads below
                value = value()
            out.bit(value is not None)
            if value is not None:
                write_variable(out, value)
        if instance.extra:
            out.bit(1)
            _variables(out, instance.extra)
        else:
            out.bit(0)
        for bit in range(instance.graph.owner_states):
            on = bit in instance.active
            out.bit(on)
            payload = instance.active.get(bit)
            if callable(payload):  # built at send time, e.g. a cooldown's elapsed time
                payload = payload()
            if on and payload is not None:
                out.append(payload)
        out.bits(0b11, 2)  # the instance's event list ends (code 3)
    out.bits(0, 2)  # the frame's event lists: none
    return out


def variables_frame(entity_vars: dict[int, Value]) -> BitWriter:
    """A plain (H=0) full frame with no instances, only entity variables: how an entity without
    graphs of its own, such as the game mode entity, gets values that other graphs read from it."""
    out = BitWriter()
    out.bit(0)  # lead
    out.bit(0)  # H
    out.w_u16(0)  # no instances
    if entity_vars:
        out.bit(1)
        _variables(out, entity_vars)
    else:
        out.bit(0)
    out.bits(0, 2)  # the frame's event lists: none
    return out


def message_state() -> BitWriter:
    """The remote data of a message state (38EE1100) or message entry (9D6CF8AC) with no fields: the client's state
    reader takes one more bit after the start bit (0: no optional integer); the loading marker 9FC136D7 takes none.
    Calibrated live on 0D28 (research/uprising-mission.md); a missing bit shifts every later slot by one."""
    out = BitWriter()
    out.bit(0)
    return out


def pulser_state(fresh: bool, count: int) -> BitWriter:
    """The remote data of a ClientOnlyPulser (B9898052): a bit, then its 8-bit pulse count. The client follows the
    state's client-only plug once per count it has not seen; a pulser it reads for the first time pulses once, and
    only with the bit set (script/nodes.py ClientOnlyPulser)."""
    out = BitWriter()
    out.bit(fresh)
    out.bits(count & 0xFF, 8)
    return out


def remote_full_frame(instances: list[Instance], entity_vars: dict[int, Value]) -> BitWriter:
    """H=0 instance frame; payloads include state control and remote DSR, not owner DSR. Per present remote slot
    the client reads the presence bit, one control/start bit, then the state's own data (`active[slot]`, see
    message_state); slots in `graph.silent` take no bit at all."""
    out = BitWriter()
    out.bits(0, 2)  # lead0,H0; no owner C/CmFD
    ordered = sorted(instances, key=lambda item: item.index)
    last = 0
    for instance in ordered:
        if not 0 <= instance.graph.remote_states <= 512 or any(
                type(bit) is not int or not 0 <= bit < instance.graph.remote_states or bit in instance.graph.silent
                for bit in instance.active):
            raise ValueError('remote state outside graph table')
        out.w_u16(instance.index - last)
        last = instance.index
        _descriptor(out, instance)
    out.w_u16(0)
    out.bit(bool(entity_vars))
    if entity_vars:
        _variables(out, entity_vars)
    for instance in ordered:
        for bit in range(instance.graph.sync_vars):
            value = instance.presence.get(bit)
            out.bit(value is not None)
            if value is not None:
                write_variable(out, value)
        out.bit(bool(instance.extra))
        if instance.extra:
            _variables(out, instance.extra)
        for bit in range(instance.graph.remote_states):
            if bit in instance.graph.silent:
                continue
            present = bit in instance.active
            out.bit(present)
            if present:
                out.bit(1)  # state-control update follows
                if instance.active[bit] is not None:
                    out.append(instance.active[bit])
    out.bits(0, 2)  # frame events; H0 has no per-instance event list
    return out


def owner_ack(cmfd: int) -> BitWriter:
    """An owner delta that only moves CmFD on: the client then replays fewer of its own commands when it
    rolls its prediction back."""
    out = BitWriter()
    out.bit(0)  # lead
    out.bit(1)  # H
    out.bit(0)  # C
    out.w_var(cmfd)
    out.w_var(7)  # LBSS: the body below, up to the "more" bit
    out.w_u16(0)  # no instances
    out.bit(0)  # no entity variables
    out.bit(0)  # no instance events
    out.bit(0)  # no more sub-frames
    out.bits(0, 2)  # no frame events
    return out


def _delta_mask(out: BitWriter, count: int, slots) -> None:
    mask = sum(1 << slot for slot in slots)
    if count <= 8:
        out.bits(mask, count)
        return
    groups = range((count + 7) // 8)
    out.bits(sum(1 << group for group in groups if (mask >> (8 * group)) & 0xFF), len(groups))
    for group in groups:
        byte = (mask >> (8 * group)) & 0xFF
        if byte:
            out.bits(byte, min(8, count - 8 * group))


@lru_cache(maxsize=1)
def _delta_layouts():
    path = Path(__file__).resolve().parents[2] / 'data' / 'owner_delta_layouts_174.json'
    layouts = json.loads(path.read_text(encoding='utf-8'))
    return {int(index): (state_count, tuple(slot for slot in range(state_count) if slot not in excluded),
                         var_count, var_slots)
            for index, (state_count, excluded, var_count, var_slots) in layouts.items()}


def _different_payload(before, after):
    if before is None or after is None:
        return before is not after
    return (before.count, before.value) != (after.count, after.value)


def _owner_changes(cmfd, before, after):
    """Owner delta for existing instances. Unselected states keep their client runtime state. None when the
    instances changed or a changed graph has no layout: send a full frame instead."""
    old_instances, old_vars = before
    instances, variables = after
    body, changes = BitWriter(), BitWriter()
    last = 0
    if [old.index for old in old_instances] != [new.index for new in instances]:
        return None
    for old, new in zip(old_instances, instances):
        presence = {slot: value for slot, value in new.presence.items() if old.presence.get(slot) != value}
        extra = {number: value for number, value in new.extra.items() if old.extra.get(number) != value}
        states = [slot for slot in sorted(old.active.keys() | new.active.keys())
                  if (slot in old.active) != (slot in new.active)
                  or _different_payload(old.active.get(slot), new.active.get(slot))]
        if not (presence or extra or states):
            continue
        if new.graph.index not in _delta_layouts():
            return None
        body.w_u16(new.index - last)
        body.bits(0, 2)  # neither remove nor describe this existing instance
        last = new.index
        # Deltas index the unfiltered tables. Full frames skip unused and client/server-only slots.
        state_count, state_slots, var_count, var_slots = _delta_layouts()[new.graph.index]
        _delta_mask(changes, var_count, (var_slots[slot] for slot in presence))
        for slot in sorted(presence):
            write_variable(changes, presence[slot])
        changes.bit(bool(extra))
        if extra:
            _variables(changes, extra)
        _delta_mask(changes, state_count, (state_slots[slot] for slot in states))
        for slot in states:
            changes.bit(slot in new.active)
            payload = new.active.get(slot)
            if payload is not None:
                changes.append(payload)
    body.w_u16(0)
    entity_changes = {number: value for number, value in variables.items() if old_vars.get(number) != value}
    body.bit(bool(entity_changes))
    if entity_changes:
        _variables(body, entity_changes)
    body.append(changes)
    body.bit(0)  # no instance events
    out = BitWriter()
    out.bits(0b110, 3)  # C1 applies CmFD to the native authoritative state clock before this data delta.
    out.w_var(cmfd)
    out.w_var(body.count)
    out.append(body)
    out.bit(0)  # no more sub-frames
    out.bits(0, 2)  # no frame events
    return out


def chunk(payload: BitWriter, first: int, span: int) -> BitWriter:
    out = BitWriter()
    out.w_u32(first)
    out.w_var(span)
    out.bit(0)  # not fragmented
    out.w_var(payload.count)
    out.append(payload)
    return out


class Stream:
    """One entity's statescript as one client gets it: the frame numbers of the chunks sent and
    delivered, and the last CmFD. The client applies a chunk only when its last frame is new to it and
    drops a queued chunk whose range a newer chunk covers, so every chunk gets the next number and a
    chunk with data must arrive before an ack-only delta may cover it."""

    def __init__(self, entity: int) -> None:
        self.entity = entity
        self.last = 0  # the last frame number sent
        self.delivered = 0  # the highest last frame the client has
        self.data_last = 0  # the last chunk that carries data, not only an ack
        self.cmfd = 0
        self.next_ack = 0.0  # when the next ack-only delta may go
        self._reload_snapshot = None

    def next_cmfd(self, newest: int) -> int:
        """The client skips an owner frame whose CmFD is not larger than the last one."""
        self.cmfd = max(newest, self.cmfd + 1)
        return self.cmfd

    @property
    def data_in_flight(self) -> bool:
        return self.delivered < self.data_last

    def full_frame(self, frame: BitWriter) -> EntityUpdate:
        """A full frame; sent again unchanged when its datagram is lost."""
        self._reload_snapshot = None
        self.last += 1
        self.data_last = self.last
        return self._update(chunk(frame, 0, self.last), resend=True)

    def reload_frame(self, cmfd, instances, entity_vars, *, refresh=False) -> EntityUpdate:
        """Send weapon reload updates in full. Refresh health and abilities without replaying StateAnim."""
        instances = [replace(instance, presence={slot: value() if callable(value) else value
                                                 for slot, value in instance.presence.items()},
                             extra=instance.extra.copy(),
                             active={slot: value() if callable(value) else value
                                     for slot, value in instance.active.items()})
                     for instance in sorted(instances, key=lambda item: item.index)]
        current = (instances, entity_vars.copy())
        previous = self._reload_snapshot
        delta = _owner_changes(cmfd, previous, current) if refresh and previous is not None else None
        if delta is None:
            frame = owner_full_frame(cmfd, instances, entity_vars)
            frame.value |= 1 << 2  # C1: apply CmFD before decoding authoritative weapon/reload data.
            update = self.full_frame(frame)  # clears the snapshot
            self._reload_snapshot = current
            return update
        self._reload_snapshot = current
        return self.delta_frame(delta)

    def delta_frame(self, frame: BitWriter) -> EntityUpdate:
        """A data delta; retain it until acknowledged, like a full frame."""
        first = self.delivered + 1
        self.last += 1
        self.data_last = self.last
        return self._update(chunk(frame, first, self.last - first), resend=True)

    def ack_delta(self, frame: BitWriter) -> EntityUpdate:
        """A delta that only moves CmFD on. A later one covers it, so a lost one is not sent again."""
        first = self.delivered + 1
        self.last += 1
        return self._update(chunk(frame, first, self.last - first), resend=False)

    def arrived(self, last: int) -> None:
        self.delivered = max(self.delivered, last)

    def _update(self, data: BitWriter, resend: bool) -> EntityUpdate:
        update = EntityUpdate(self.entity, chunk=data)
        update.chunk_last = self.last
        update.stream = self
        update.resend = resend
        return update
