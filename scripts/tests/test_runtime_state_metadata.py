"""Offline bundle preparation tests with invented state metadata."""

import gzip
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from zipfile import ZipFile


SCRIPT = Path(__file__).resolve().parents[1] / "build_runtime_state_metadata.py"
LISTING = """# graph 058000000000002a
state Basic_19:  # SyntheticClass, state 0 (plain State at run time)
state Overlay_23:  # SyntheticClass, state 1 (plain State at run time, client-only)
state Unknown_80:  # SyntheticClient, state 2 (client-only)
state Mixed_35:  # MixedClass, state 3 (plain State at run time)
state Mixed_36:  # MixedClass, state 4
"""


class RuntimeStateMetadataTest(unittest.TestCase):
    def test_exact_slot_matches_and_no_overwrite(self):
        def node(pos, rid, slot, client):
            return {
                "pos": pos,
                "rid": rid,
                "state": slot,
                "class": "SyntheticClass",
                "client_only": client,
                "server_only": False,
                "fields": {"m_2BBEEAB8": True},
            }

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            evidence = root / "evidence.zip"
            source = root / "input.gz"
            output = root / "output.gz"
            graph = {"index": 42, "states": [6, 7], "nodes": [
                node(6, 19, 0, False), node(7, 23, 1, True),
            ]}
            bundle = {"version": 1, "graphs": [graph]}
            with ZipFile(evidence, "w") as archive:
                archive.writestr("listing/058000000000002a.txt", LISTING)
            with gzip.open(source, "wt", encoding="utf-8") as file:
                json.dump(bundle, file)
            before = source.read_bytes()
            command = [
                sys.executable, str(SCRIPT), "--graph-bundle", str(source),
                "--evidence", str(evidence), "--output", str(output),
            ]
            prepared = subprocess.run(command, capture_output=True, text=True, check=True)
            self.assertIn("1 unmatched within it", prepared.stdout)
            with gzip.open(output, "rt", encoding="utf-8") as file:
                imported = json.load(file)
            self.assertEqual(imported.pop("runtime_state_metadata"), {
                "plain_classes": ["SyntheticClass"],
                "client_states": [{
                    "graph": 42, "state": 1, "rid": 23, "class": "SyntheticClass",
                }],
            })
            self.assertEqual(imported, bundle)
            self.assertEqual(source.read_bytes(), before)

            finished = output.read_bytes()
            duplicate = subprocess.run(command, capture_output=True, text=True)
            self.assertNotEqual(duplicate.returncode, 0)
            self.assertEqual(output.read_bytes(), finished)


    def test_directed_numeric_messages_extend_classified_bundle(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "input.gz"
            output = root / "output.gz"
            bundle = {
                "version": 1,
                "graphs": [],
                "runtime_state_metadata": {"plain_classes": ["SyntheticClass"]},
            }
            with gzip.open(source, "wt", encoding="utf-8") as file:
                json.dump(bundle, file)
            before = source.read_bytes()
            command = [sys.executable, str(SCRIPT), "--graph-bundle", str(source)]
            route = "0x0240000000001F01:0x0a40000000001a02"
            duplicate = ["--directed-numeric-message", route] * 2
            for args in [
                [],
                ["--directed-numeric-message", "0x0240000000001f01:0x0A40000000000000"],
                duplicate,
            ]:
                result = subprocess.run(
                    command + args + ["--output", str(output)],
                    capture_output=True,
                    text=True,
                )
                self.assertNotEqual(result.returncode, 0, args)
                self.assertFalse(output.exists())
            subprocess.run(
                command + ["--directed-numeric-message", route, "--output", str(output)],
                capture_output=True,
                text=True,
                check=True,
            )
            with gzip.open(output, "rt", encoding="utf-8") as file:
                imported = json.load(file)
            self.assertEqual(
                imported["runtime_state_metadata"].pop("directed_numeric_messages"),
                [{"topic": "0x0240000000001F01", "parameter": "0x0A40000000001A02"}],
            )
            self.assertEqual(imported, bundle)
            self.assertEqual(source.read_bytes(), before)
            second_output = root / "second.gz"
            result = subprocess.run(
                [
                    sys.executable, str(SCRIPT), "--graph-bundle", str(output),
                    "--directed-numeric-message", route, "--output", str(second_output),
                ],
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(second_output.exists())


if __name__ == "__main__":
    unittest.main()
