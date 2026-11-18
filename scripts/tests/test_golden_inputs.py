"""Public replay evidence checks using invented files and observations."""
import hashlib
import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from capture_room_pair import (
    built_artifact,
    finish_receipt,
    recording_environment,
    validate_observation,
)
from golden_inputs import ReplayInputs
from golden_reference import write_trace


class InputTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.pack = self.root / "pack"
        self.pack.mkdir()
        self.manifest = self.pack / "manifest.json"
        self.manifest.write_bytes(b"{}")
        self.schedule = self.root / "schedule"
        self.schedule.write_bytes(b'{"elapsed_us":0,"inputs":[],"missed":0,"tick":0}\n')

    def inputs(self):
        return ReplayInputs.read(self.pack, self.schedule)

    def test_hidden_supplemental_inputs_change_the_profile_even_with_fixed_manifest(self):
        sub = self.pack / "data"
        sub.mkdir()
        extra = sub / "extra"
        extra.write_bytes(b"first")
        start = self.inputs()
        manifest = hashlib.sha256(b"{}").digest()
        expected = hashlib.sha256(b"orpheus-runtime-tree-v1\0Ddata\0Fmanifest.json\0" + manifest
                                  + b"Fdata/extra\0" + hashlib.sha256(b"first").digest()).hexdigest()
        self.assertEqual(start.profile_sha256, expected)
        extra.write_bytes(b"second")
        changed = self.inputs()
        self.assertEqual(changed.runtime_sha256, start.runtime_sha256)
        self.assertNotEqual(changed.profile_sha256, start.profile_sha256)
        (self.pack / "empty").mkdir()
        self.assertNotEqual(changed.profile_sha256, self.inputs().profile_sha256)

    def test_noncanonical_or_incomplete_schedule_cannot_be_bound(self):
        row = self.schedule.read_bytes()
        for bad in (b"", row[:-1], row.replace(b"\n", b"\r\n"), row + row, row.replace(b'"inputs"', b' "inputs"')):
            with self.subTest(bad=bad):
                self.schedule.write_bytes(bad)
                with self.assertRaises(ValueError):
                    self.inputs()

    def test_observations_must_echo_every_offered_input(self):
        inputs = self.inputs()
        row = json.loads(self.schedule.read_bytes()) | {"state": [], "missing": ["player.ammo"]}
        path = self.root / "observation.json"
        data = {"status": "incomplete", "ticks": [row], "bot_spawner_slot": 2, "quanta_us": 16000}
        path.write_text(json.dumps(data))
        first = validate_observation(path, inputs, 2)
        with self.assertRaisesRegex(ValueError, "spawner"):
            validate_observation(path, inputs, 1)
        path.write_text(json.dumps(data, indent=2))
        self.assertEqual(validate_observation(path, inputs, 2), first)
        row["inputs"] = [{}]
        path.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, "offered schedule"):
            validate_observation(path, inputs, 2)
        data["ticks"] = []
        path.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, "per offered tick"):
            validate_observation(path, inputs, 2)

    def test_build_selects_only_the_named_package_library_test(self):
        manifest = self.root / "Cargo.toml"
        native = {"reason": "compiler-artifact", "executable": str(self.root / "native"),
                  "manifest_path": str(manifest), "target": {"name": "orpheus", "kind": ["lib"]},
                  "profile": {"test": True}}
        def select(*items):
            return built_artifact([json.dumps(item) for item in items], manifest, "orpheus", True)
        other = [native | {"manifest_path": str(self.root / "other/Cargo.toml")},
                 native | {"profile": {"test": False}}, {"reason": "build-finished"},
                 native | {"target": {"name": "orpheus", "kind": ["bin"]}}]
        self.assertEqual(select(*other, native), self.root / "native")
        for rejected in (other, [native, native]):
            with self.assertRaisesRegex(ValueError, "exactly one"):
                select(*rejected)

    @staticmethod
    def observation(inputs, rows):
        return {
            "missing": ["provenance.effective_data_sha256"], "source_rev": "a"*40, "manifest_sha256": inputs.runtime_sha256,
            "quanta_us": 16000, "bot_spawner_slot": 1, "bot_roster_size": 2,
            "bot_spawner": {"team": 1, "hero": "1"*16, "position_bits": ["0"*16]*3, "yaw": 2},
            "initial_room_tick": 10, "first_scheduled_room_tick": 11, "setup_delivery_ticks": 0,
            "initial_owner_delivery_phase": "before-first-send", "host_random_model": "counter", "host_initial_seed": 0,
            "player_vm_random_model": "synthetic-vm", "player_vm_initial_seed": 2**64-1, "ticks": rows,
        }

    def test_binding_records_distinct_sources_of_randomness_and_checks_every_tick(self):
        inputs = self.inputs()
        row = json.loads(self.schedule.read_bytes()) | {"state": [], "effects": [], "missing": []}
        data = self.observation(inputs, [row])
        def encoded(role):
            stream = io.BytesIO()
            write_trace(stream, data, inputs, role=role, source_revision="a"*40 if role=="reference" else "b"*40,
                        reference_revision="a"*40, adapter_revision="c"*40, platform="test", toolchain="test")
            return stream.getvalue().splitlines(keepends=True)
        for role in ("reference", "candidate"):
            header, tick, footer = encoded(role)
            self.assertEqual(json.loads(header)["role"], role)
            self.assertNotIn("seed", json.loads(header))
            setup = json.loads(header)["setup"]
            self.assertEqual((setup["host_initial_seed"], setup["player_vm_initial_seed"]), (0, 2**64-1))
            self.assertEqual(json.loads(footer)["sha256"], hashlib.sha256(header + tick).hexdigest())
        row["missing"] = ["effects"]
        with self.assertRaisesRegex(ValueError, "unobserved"):
            encoded("candidate")
        row["missing"] = []
        del data["host_initial_seed"]
        with self.assertRaises(KeyError):
            encoded("candidate")

    def test_recording_environment_excludes_inherited_gameplay_switches(self):
        inherited = {"PATH": "tools", "OW174_NO_COMBAT_NOTICES": "1", "OW174_BODY_TRACE": "1",
                     "ORPHEUS_GRAPH_BUNDLE": "other", "ORPHEUS_RUNTIME_DIR": "other", "PYTHONPATH": "other"}
        with patch.dict(os.environ, inherited, clear=True):
            env = recording_environment(self.pack, self.schedule)
        self.assertEqual(env, {"PATH": "tools", "ORPHEUS_RUNTIME_DIR": str(self.pack),
                               "ORPHEUS_ROOM_SCHEDULE": str(self.schedule)})

    def test_bad_reference_metadata_preserves_independently_validated_candidate(self):
        fixture = Path(__file__).resolve().parents[2] / "rust/crates/golden-trace/tests/fixtures/reference.jsonl"
        header, *ticks, _ = [json.loads(line) for line in fixture.read_bytes().splitlines()]
        inputs = ReplayInputs(header["runtime_sha256"], header["profile_sha256"], header["schedule_sha256"], len(ticks))
        data = self.observation(inputs, [row | {"missing": []} for row in ticks])
        (self.root / "candidate-1.json").write_text(json.dumps(data))
        (self.root / "reference-1.json").write_text(json.dumps(data | {"platform": ["test"]}))  # missing toolchain
        receipt = {"repeatable": {"reference": True, "candidate": True}, "platform": ["test"], "status": "incomplete"}
        with patch("capture_room_pair.subprocess.run") as validator, patch("sys.stdout", new_callable=io.StringIO) as output:
            result = finish_receipt(self.root, "validator", inputs, ["a"*40, "b"*40, "c"*40], "test", receipt)
        self.assertEqual(result, 2)
        self.assertEqual(output.getvalue(), "incomplete\n")
        validator.assert_called_once()
        self.assertEqual(validator.call_args.args[0], ["validator", "--check-candidate", self.root / "candidate.pending.jsonl"])
        recorded = json.loads((self.root / "receipt.json").read_bytes())
        self.assertEqual(recorded["status"], "incomplete")
        self.assertEqual(recorded["invalid_roles"], ["reference"])
        self.assertNotIn("validated_reference_sha256", recorded)
        candidate = (self.root / "candidate.jsonl").read_bytes()
        self.assertEqual(recorded["validated_candidate_sha256"], hashlib.sha256(candidate).hexdigest())
