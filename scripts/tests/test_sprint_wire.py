"""Invented full remote frames read by the pinned Python client reader."""

import os
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

sys.path.append(str(Path(__file__).resolve().parents[1]))
from check_sprint_wire import apply_chunk, load_reference, validate_writes


def number(value, width):
    if not 0 <= value < 1 << width:
        raise ValueError("synthetic integer width")
    return [(value >> i) & 1 for i in range(width)]


def small(value, width):
    return [0] + number(value, width)


def variable(value):
    return number(1, 4) + number(value, 4) + [0]


def chunk(instance, duplicate):
    descriptor = number(37, 16) + [0, 0, 0, 0]
    extra = [1, 1, 0] + number(699, 16) + variable(2) + small(0, 8) if duplicate else [0]
    payload = ([0, 0] + small(instance, 4) + descriptor + small(0, 4) + [0]
               + [1] + variable(1) + extra + [0, 0])
    bits = small(0, 16) + small(0, 8) + [0] + small(len(payload), 8) + payload
    return sum(bit << i for i, bit in enumerate(bits)).to_bytes((len(bits) + 7) // 8, "little"), len(bits)


@unittest.skipUnless(os.environ.get("ORPHEUS_PYTHON_REFERENCE"), "set ORPHEUS_PYTHON_REFERENCE to the pinned checkout")
class SprintWireTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.decode, _, _, _ = load_reference(Path(os.environ["ORPHEUS_PYTHON_REFERENCE"]))

    def read_frame(self, instance, duplicate, truncated=False):
        raw, width = chunk(instance, duplicate)
        self.assertEqual(width, 118 if duplicate else 82)
        graph = SimpleNamespace(presence_vars=lambda: [SimpleNamespace(var=699)], remote_states=list)
        with patch.object(self.decode, "graph_of", return_value=graph):
            return apply_chunk(self.decode, self.decode.ClientBody(), raw, width - int(truncated))

    def test_single_full_remote_frame(self):
        frame, observed = self.read_frame(1, False)
        self.assertEqual((frame.first, frame.last, frame.owner), (0, 0, False))
        self.assertEqual([entry[:3] for entry in observed], [(1, 37, (1, ()))])
        self.assertEqual(frame.instances[1].vars[699], (1, ()))
        validate_writes(observed, 1, 37, [(1, ())])

    def test_duplicate_named_variable_is_observed_before_overwrite(self):
        frame, observed = self.read_frame(1, True)
        self.assertEqual((frame.first, frame.last, frame.owner), (0, 0, False))
        self.assertEqual([entry[:3] for entry in observed], [(1, 37, (1, ())), (1, 37, (2, ()))])
        self.assertLess(observed[0][3], observed[1][3])
        self.assertEqual(frame.instances[1].vars[699], (2, ()))
        with self.assertRaisesRegex(ValueError, "^duplicate 699 write$"):
            validate_writes(observed, 1, 37, [(1, ())])

    def test_foreign_instance_is_rejected_by_this_scenario(self):
        _, observed = self.read_frame(2, False)
        self.assertEqual([entry[:3] for entry in observed], [(2, 37, (1, ()))])
        with self.assertRaisesRegex(ValueError, "^unexpected 699 owner in scenario$"):
            validate_writes(observed, 1, 37, [(1, ())])

    def test_truncated_chunk_fails(self):
        with self.assertRaisesRegex(self.decode.DecodeError, "^the payload runs past the chunk$"):
            self.read_frame(1, True, truncated=True)

    def test_foreign_graph_ammo_write_is_observed_and_rejected(self):
        raw, width = chunk(2, False)
        graph = SimpleNamespace(presence_vars=lambda: [SimpleNamespace(var=53)], remote_states=list)
        extra = []
        with patch.object(self.decode, "graph_of", return_value=graph):
            frame, _ = apply_chunk(self.decode, self.decode.ClientBody(), raw, width, extra)
        self.assertEqual(frame.instances[2].vars[53], (1, ()))
        self.assertEqual([item[:4] for item in extra], [(53, 2, 37, (1, ()))])
        with self.assertRaisesRegex(ValueError, "^unexpected 53 owner in scenario$"):
            validate_writes([item[1:] for item in extra], 1, 0x254, [], 53, int)


class ValidateWritesTests(unittest.TestCase):
    def test_boolean_value_is_rejected(self):
        observed = [(1, 37, (True, ()), 0)]
        with self.assertRaisesRegex(ValueError, "^699 type or bindings$"):
            validate_writes(observed, 1, 37, [(1, ())])

    def test_non_empty_bindings_are_rejected(self):
        observed = [(1, 37, (1, (1,)), 0)]
        with self.assertRaisesRegex(ValueError, "^699 type or bindings$"):
            validate_writes(observed, 1, 37, [(1, (1,))])

    def test_rifle_reload_requires_boolean_not_integer(self):
        with self.assertRaisesRegex(ValueError, "^51 type or bindings$"):
            validate_writes([(1, 0x254, (1, ()), 0)], 1, 0x254, [(True, ())], 51, bool)

    def test_rifle_missing_ammo_write_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "^53 writes$"):
            validate_writes([], 1, 0x254, [(29, ())], 53, int)

    def test_rifle_duplicate_reload_write_is_rejected(self):
        observed = [(1, 0x254, (True, ()), 0), (1, 0x254, (False, ()), 7)]
        with self.assertRaisesRegex(ValueError, "^duplicate 51 write$"):
            validate_writes(observed, 1, 0x254, [(True, ())], 51, bool)


if __name__ == "__main__":
    unittest.main()
