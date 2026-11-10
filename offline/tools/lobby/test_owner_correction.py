"""owner_correction: the frame a record of the own body carries, the fields the client compares, one record per
frame, its command ack and when none go. The reader below is LizRollin's tests/test_game_world.py @ 9f8f3bc (MIT,
vendor/README.md): the client's order of a movement-state block (their 0x7FF789B6F680, ours 0x7FF6D1E9F680)."""
import struct
import unittest
from unittest import mock

import practice  # noqa: F401  (puts vendor/ on the path)
from ow174.game import world
from ow174.game.bits import BitReader
from ow174.game.commands import CROUCH, JUMP, Command
from ow174.game.content import PRACTICE_RANGE, SOLDIER
from ow174.game.match import Match
from ow174.game.mover import Mover, f32, mover_data
from ow174.game.world import NO_INPUT, Movement

import owner_correction

SOLDIER_BODY = 0x04000000000003CF
TRACER, DVA = 0x02E0000000000003, 0x02E000000000007A


def _field(reader, widths, reference=0):
    if not reader.bit():
        return reference
    for width in widths:
        if reader.bit():
            return reference + reader.signed(width)
    return reference + reader.signed(32)


def _frame_field(reader, reference):
    if not reader.bit():
        return reference
    if reader.bit():
        return (reference + reader.signed(8)) & 0xFFFFFFFF
    return reader.bits(32)


def _throttle(reader):
    return reader.signed(8) if reader.bit() else 0


def _frames_back(reader):
    if not reader.bit():
        return 0
    for width in (4, 10):
        if reader.bit():
            return reader.bits(width)
    return reader.bits(32)


def _floats(reader):
    if not reader.bit():
        return (0.0, 0.0, 0.0)
    return tuple(struct.unpack("<f", struct.pack("<I", reader.bits(32)))[0] if reader.bit() else 0.0
                 for _ in range(3))


def read_movement(reader, frame):
    state = {"flags": 0}
    if reader.bit():
        for k in range(4):
            if reader.bit():
                state["flags"] |= reader.bits(8) << (8 * k)
    assert reader.bit() == 0  # +4: 1/1024 m
    state["frame"] = frame - _frames_back(reader)  # +8
    assert _frame_field(reader, 0) == 0  # +12
    state["pitch"] = _field(reader, (8, 10, 16))
    state["air_ticks"] = _field(reader, (4, 10))
    state["timer"] = _frame_field(reader, 0xFFFFFFFF)  # +20
    state["gravity"] = _field(reader, (8, 10, 24)) / 1024
    state["speed_scale"] = 1.0 + _field(reader, (8, 10, 24)) / 1024
    state["input_frame"] = _frame_field(reader, NO_INPUT) if reader.bit() else state["frame"]
    state["+40"] = _frame_field(reader, 0xFFFFFFFF)
    state["throttles"] = (_throttle(reader), _throttle(reader))
    if state["input_frame"] == state["frame"]:
        state["input_throttles"] = state["throttles"]
    else:
        state["input_throttles"] = (_throttle(reader), _throttle(reader))
    state["yaw"] = _field(reader, (8, 10, 16))
    assert _field(reader, (8, 10, 16)) == 0 and _field(reader, (8, 10, 16)) == 0  # Euler pitch, roll
    state["position"] = tuple(_field(reader, (8, 10, 24)) / 1024 for _ in range(3))
    state["velocity"] = tuple(_field(reader, (8, 10, 24)) / 1024 for _ in range(3)) if reader.bit() else (0.0,) * 3
    assert reader.bits(9) == 0 and reader.bit() == 0  # no optional parts, no parent
    first = _field(reader, (8, 12, 17))  # +52
    state["spring_speed"] = _field(reader, (8, 10, 24)) / 1024  # +60
    state["spring"] = (first, _field(reader, (8, 12, 17)))  # +56
    assert reader.bit() == 0  # +384
    state["fall"] = _floats(reader)  # +832
    return state


def read(state: Movement, tick: int) -> dict:
    """A ch3 record read back as the client reads it in packet frame `tick`."""
    record = world.movement_record(state, tick)
    reader = BitReader(record.getvalue())
    length = reader.bits(12)
    assert reader.bit() == 0  # against the reference state
    fields = read_movement(reader, tick)
    assert reader.pos == length == record.count
    return fields


class RecordTests(unittest.TestCase):
    def test_a_record_carries_the_state_at_its_own_command_frame(self):
        for back in (0, 1, 15, 16, 1023, 1024, 70000):
            self.assertEqual(read(Movement((1.0, 2.0, 3.0), frame=200000 - back), 200000)["frame"], 200000 - back)
        self.assertEqual(read(Movement((1.0, 2.0, 3.0)), 5000)["frame"], 5000)  # no frame: the packet's
        with self.assertRaises(ValueError):
            world.movement_record(Movement((0.0, 0.0, 0.0), frame=101), 100)

    def test_every_field_the_client_compares_comes_back_as_the_mover_has_it(self):
        mover = Mover((0.0, 1.0, 0.0), 1000, None, mover_data(SOLDIER_BODY))
        script = [(20, 127, 0, 2000), (12, 0, CROUCH, -3000), (14, 104, 0, 17000), (1, -127, JUMP, 0),
                  (45, 9, 0, 0), (10, 0, 0, 0)]
        frame, seen = 5000, set()
        for count, forward, buttons, pitch in script:
            for _ in range(count):
                mover.step(Command(frame, forward=forward, buttons=buttons, pitch=pitch, yaw=-20000), 0.016, 0)
                snapshot = mover.history[-1]
                fields = read(owner_correction.movement(snapshot), frame + 3)
                expected = {
                    "frame": frame, "flags": snapshot.flags, "pitch": snapshot.pitch, "yaw": snapshot.yaw,
                    "air_ticks": snapshot.air_ticks, "gravity": snapshot.gravity,
                    "input_frame": NO_INPUT if snapshot.input_frame is None else snapshot.input_frame,
                    "+40": 0xFFFFFFFF if snapshot.crouch_end is None else snapshot.crouch_end,
                    "throttles": snapshot.throttles, "input_throttles": snapshot.input_throttles,
                    "position": snapshot.position, "velocity": snapshot.velocity, "spring": snapshot.spring,
                    "spring_speed": snapshot.spring_speed, "fall": (0.0, snapshot.fall, 0.0),
                }
                self.assertEqual({key: fields[key] for key in expected}, expected, f"frame {frame}")
                seen.update(key for key, value in {
                    "+40": fields["+40"] != 0xFFFFFFFF, "fall": fields["fall"][1] != 0.0,
                    "spring speed": fields["spring_speed"] != 0.0,
                    "older input": fields["input_frame"] not in (frame, NO_INPUT),
                    "second spring": fields["spring"][0] != fields["spring"][1]}.items() if value)
                frame += 1
        self.assertEqual(len(seen), 5)  # each field went with a value of its own somewhere
        self.assertEqual(f32(mover.fall), mover.fall)


class CadenceTests(unittest.TestCase):
    """A Soldier: 76 in the Practice Range whose client has its body."""

    def setUp(self):
        self.match = Match(PRACTICE_RANGE, gameplay=True)
        self.player = self.add(1, SOLDIER)

    def add(self, account, hero):
        player = self.match.add_player(account, "Alpha", hero, 0, True)
        player.has_body = True
        self.match.own_body_updates(player)[0].delivered = True  # the client acknowledged the body's create
        return player

    def own(self, frame, player=None):
        """The frame's records of the player's own body, read back."""
        player = player or self.player
        updates = self.match.remote_movements(player, frame)
        return [read(update.movement, frame) for update in updates if update.entity == player.body]

    def test_one_record_per_frame_of_the_newest_state_not_after_the_packet_frame(self):
        self.player.take_commands([Command(7000 + n, forward=127) for n in range(10)])
        history = {snapshot.frame: snapshot for snapshot in self.player.mover.history}
        (record,) = self.own(7005)
        self.assertEqual((record["frame"], record["position"]), (7005, history[7005].position))
        self.assertEqual(self.own(7005), [])  # sent: not again
        self.assertEqual([record["frame"] for record in self.own(7006)], [7006])
        self.assertEqual([record["frame"] for record in self.own(7020)], [7009])  # the newest it has
        self.assertEqual(self.own(7021), [])
        self.player.take_commands([Command(7010 + n, forward=127) for n in range(3)])
        (record,) = self.own(7022)
        self.assertEqual((record["frame"], record["position"]), (7012, tuple(self.player.mover.position)))

    def test_a_server_side_move_reaches_the_record(self):
        self.player.take_commands([Command(7000 + n) for n in range(5)])
        self.player.mover.position = [3.0, 0.0, 4.0]  # a teleport: the next step starts there
        self.player.take_commands([Command(7005)])
        self.assertEqual(self.own(7005)[0]["position"], (3.0, 0.0, 4.0))

    def test_nothing_before_the_client_has_the_body_off_or_beside_client_sync(self):
        self.player.take_commands([Command(7000 + n, forward=127) for n in range(10)])
        self.player.body_create.delivered = False
        self.assertEqual(self.own(7005), [])
        self.player.body_create.delivered = True
        with mock.patch.object(owner_correction, "ENABLED", False):
            self.assertEqual(self.own(7006), [])
        with mock.patch("client_sync.active", return_value=True):
            self.assertEqual(self.own(7007), [])
        self.assertEqual(len(self.own(7008)), 1)

    def test_every_kits_hero_but_dva_whose_boosters_the_server_only_fits(self):
        tracer, dva = self.add(2, TRACER), self.add(3, DVA)
        for other in (tracer, dva):
            other.take_commands([Command(7000 + n, forward=127) for n in range(10)])
        self.assertEqual(len(self.own(7005, tracer)), 1)
        self.assertEqual(self.own(7005, dva), [])

    def test_a_vm_body_only_for_the_heroes_checked_on_the_vm(self):
        for player, records in ((self.add(2, TRACER), 0), (self.player, 1)):  # the VM runs the graphs' own movement
            player.take_commands([Command(7000 + n, forward=127) for n in range(10)])
            player.body_vm = object()
            self.assertEqual(len(self.own(7005, player)), records)

    def test_the_ack_stays_near_the_newest_record(self):
        self.player.take_commands([Command(7000 + n, forward=127) for n in range(10)])
        self.assertEqual(owner_correction.command_ack(self.player, 7004), 7009)  # no record yet
        self.own(7004)
        self.assertEqual(owner_correction.command_ack(self.player, 7004), 7004 + owner_correction.ACK_LEAD)


if __name__ == "__main__":
    unittest.main()
