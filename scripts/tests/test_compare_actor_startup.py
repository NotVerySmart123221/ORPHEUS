"""Independent actor startup comparison contracts with invented data."""

import hashlib
import importlib
from pathlib import Path
import sys
import tempfile
import unittest
from copy import deepcopy


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
startup = importlib.import_module("compare_actor_startup")

MATRIX_PATHS = {
    "collision/0688.col",
    "data/ai_spawners_174.json",
    "data/collision_props_174.json",
    "data/game_graphs_174.json.gz",
    "data/game_heroes_174.json",
    "data/game_movers_174.json",
    "data/game_stats_174.json",
    "data/statescript_codecs_174.json",
}
HERO_A = "0xA000000000000031"
HERO_B = "0xA000000000000045"
BODY_A = "0xB000000000000071"
BODY_B = "0xB000000000000082"
BODY_OTHER = "0xB000000000000083"


class RuntimeInputTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.runtime = self.root / "runtime"
        self.runtime.mkdir()
        self.inputs = {}
        for name in sorted(MATRIX_PATHS):
            path = self.runtime / name
            path.parent.mkdir(parents=True, exist_ok=True)
            data = f"Original fixture input for {name}".encode()
            path.write_bytes(data)
            self.inputs[name] = {"sha256": hashlib.sha256(data).hexdigest()}

    def test_all_eight_exact_files_verify(self):
        self.assertEqual(set(startup.EXPECTED_INPUTS), MATRIX_PATHS)
        self.assertIsNone(startup.verify_runtime_inputs(self.runtime, self.inputs))

    def test_present_file_with_a_different_digest_is_rejected(self):
        path = self.runtime / "data/game_heroes_174.json"
        data = path.read_bytes()
        path.write_bytes(bytes([data[0] ^ 1]) + data[1:])
        with self.assertRaises(ValueError):
            startup.verify_runtime_inputs(self.runtime, self.inputs)

    def test_missing_file_is_rejected(self):
        path = self.runtime / "data/game_graphs_174.json.gz"
        path.unlink()
        with self.assertRaises((OSError, ValueError)):
            startup.verify_runtime_inputs(self.runtime, self.inputs)

    def test_input_map_is_exact_and_canonical(self):
        name = "data/game_graphs_174.json.gz"
        digest = self.inputs[name]["sha256"]
        without_graphs = {key: row for key, row in self.inputs.items() if key != name}
        for description, candidate in (
            ("missing row", without_graphs),
            ("extra row", {**self.inputs, "data/synthetic-extra.json": {"sha256": digest}}),
            ("path traversal", {**without_graphs, "../outside": {"sha256": digest}}),
            ("absolute path", {**without_graphs, str(self.root / "outside"): {"sha256": digest}}),
            ("digest uppercase", {**self.inputs, name: {"sha256": digest.upper()}}),
            ("digest short", {**self.inputs, name: {"sha256": digest[:-1]}}),
            ("unknown row field", {**self.inputs, name: {"sha256": digest, "note": "extra"}}),
            ("file unavailable", {**self.inputs, name: {"error_kind": "NotFound"}}),
        ):
            with self.subTest(description=description), self.assertRaises(ValueError):
                startup.verify_runtime_inputs(self.runtime, candidate)

    def test_expected_file_cannot_resolve_outside_runtime(self):
        path = self.runtime / "data/game_graphs_174.json.gz"
        outside = self.root / "external-copy"
        outside.write_bytes(path.read_bytes())
        path.unlink()
        try:
            path.symlink_to(outside)
        except (NotImplementedError, OSError):
            self.skipTest("creating test symlinks is unavailable")
        with self.assertRaises((OSError, ValueError)):
            startup.verify_runtime_inputs(self.runtime, self.inputs)


class ActorMergeTests(unittest.TestCase):
    def setUp(self):
        self.rust = [
            {
                "hero": HERO_B,
                "body": BODY_B,
                "bootstrap": {"status": "started", "instances": 4},
                "admission": {"status": "started"},
                "prerequisites": {"body_status": "matched"},
            },
            {
                "hero": HERO_A,
                "body": BODY_A,
                "bootstrap": {
                    "status": "factory_error",
                    "failure": {"stage": "projection", "category": "incomplete"},
                },
                "admission": {"status": "not_admitted"},
            },
        ]
        self.python = [
            {
                "hero": HERO_A,
                "body": BODY_A,
                "status": "decoded",
                "codec_warnings": ["synthetic warning"],
            },
            {
                "hero": HERO_B,
                "body": BODY_B,
                "status": "failed",
                "stage": "body_init",
            },
        ]

    def test_sorted_join_keeps_each_implementation_outcome(self):
        original = deepcopy((self.rust, self.python))
        rows = startup.merge_actor_rows(self.rust, self.python)
        self.assertEqual(
            rows,
            [
                {
                    "hero": HERO_A,
                    "body": BODY_A,
                    "python": {"status": "decoded", "codec_warnings": ["synthetic warning"]},
                    "rust_bootstrap": self.rust[1]["bootstrap"],
                    "rust_room_factory": self.rust[1]["admission"],
                },
                {
                    "hero": HERO_B,
                    "body": BODY_B,
                    "python": {"status": "failed", "stage": "body_init"},
                    "rust_bootstrap": self.rust[0]["bootstrap"],
                    "rust_room_factory": self.rust[0]["admission"],
                },
            ],
        )
        self.assertEqual(rows[0]["rust_bootstrap"]["status"], "factory_error")
        self.assertEqual((self.rust, self.python), original)

    def test_rejects_missing_extra_duplicate_and_noncanonical_actor_identities(self):
        different_hero = {**self.python[0], "hero": "0xA000000000000046"}
        for description, rust, python in (
            ("empty", [], []),
            ("missing actor", self.rust, self.python[1:]),
            ("different hero", self.rust, [different_hero, self.python[1]]),
            ("different body", self.rust, [{**self.python[0], "body": BODY_OTHER}, self.python[1]]),
            ("duplicate Python actor", self.rust, [*self.python, self.python[0]]),
            ("duplicate Rust actor", [*self.rust, self.rust[0]], self.python),
            ("noncanonical Rust hero", [{**self.rust[0], "hero": HERO_B.lower()}, self.rust[1]], self.python),
            ("noncanonical Python body", self.rust, [{**self.python[0], "body": "0xB071"}, self.python[1]]),
            ("missing body field", self.rust, [{"hero": HERO_A, "status": "decoded"}, self.python[1]]),
            ("numeric hero", self.rust, [{**self.python[0], "hero": 17}, self.python[1]]),
        ):
            with self.subTest(description=description), self.assertRaises(ValueError):
                startup.merge_actor_rows(rust, python)


if __name__ == "__main__":
    unittest.main()
