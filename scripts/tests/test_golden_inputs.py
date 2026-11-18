"""Public replay evidence checks using invented files and observations."""
import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from capture_room_pair import validate_observation
from golden_inputs import ReplayInputs


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
