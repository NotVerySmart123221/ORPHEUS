"""Synthetic command-line tests for the private StateScript coverage report."""

import gzip
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "summarize_statescript_coverage.py"
HERO = "0xA400000000000011"
NO_BODY = "0xA400000000000022"
UNKNOWN_HERO = "0xA400000000000033"
BODY = "0xA300000000000011"
OTHER_BODY = "0xA300000000000022"
PRIVATE_VALUE = "synthetic operand value excluded by the report"
REVISION = "a" * 40


def node(pos, rid, state, client_only=False):
    return {
        "pos": pos, "rid": rid, "state": state, "class": "SyntheticState",
        "client_only": client_only, "server_only": False, "fields": {"private": PRIVATE_VALUE},
    }


def target(graph, pos, client_scope_skipped=False):
    return {"graph": graph, "node": pos, "client_scope_skipped": client_scope_skipped}


def diagnostic(graph, pos, kind):
    return {"graph": graph, "node": pos, "kind": kind}


class StateScriptCoverageTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        self.bundle = root / "game_graphs.json.gz"
        self.catalog = root / "heroes.json"
        self.observation = root / "observation.json"
        self.secondary = root / "secondary.json"
        self.output = root / "coverage.json"
        prepared = {
            "version": 1,
            "graphs": [
                {"index": 51, "nodes": [node(6, 502, 4)]},
                {"index": 42, "nodes": [
                    node(5, 105, None), node(4, 107, 2), node(3, 101, 1),
                    node(2, 109, 0, client_only=True), node(1, 102, None),
                ]},
                {"index": 44, "nodes": [node(0, 104, None)]},
            ],
            "bodies": {
                BODY: {
                    "hero": HERO, "graphs": [{"graph": 42}, {"graph": None}],
                    "server_graphs": [{"graph": 44}], "manager": 43, "weapons": [None, 45],
                },
            },
        }
        self.bundle.write_bytes(gzip.compress(json.dumps(prepared).encode(), mtime=0))
        self.bundle_digest = hashlib.sha256(self.bundle.read_bytes()).hexdigest()
        self.catalog.write_text(
            json.dumps({NO_BODY[2:]: {"body": OTHER_BODY}, HERO: {"body": BODY}}),
            encoding="utf-8",
        )

    def write_observation(self, path, **overrides):
        sidecar = {
            "schema_version": 1, "scenario": "owner-probe-900-frames-v1", "hero": HERO,
            "available": True, "frames": 900, "truncated": True, "source_revision": REVISION,
            "graph_bundle_sha256": self.bundle_digest,
            "observations": [
                target(51, 6), target(42, 1), target(42, 2, True), target(42, 3),
                target(42, 600), target(79, 1),
            ],
            "diagnostics": [
                diagnostic(42, 3, "unsupported"),
                diagnostic(42, 4, "registration_rejection"),
                diagnostic(79, 2, "unsupported"),
            ],
            **overrides,
        }
        path.write_text(json.dumps(sidecar), encoding="utf-8")

    def run_report(self, *observations):
        command = [
            sys.executable, "-B", str(SCRIPT), "--graph-bundle", str(self.bundle),
            "--hero-catalog", str(self.catalog), "--output", str(self.output),
        ]
        for observation in observations:
            command.extend(("--observation", str(observation)))
        return subprocess.run(command, capture_output=True, text=True)

    def read_report(self, *observations):
        result = self.run_report(*observations)
        self.assertEqual(result.returncode, 0, result.stderr)
        payload = self.output.read_text(encoding="utf-8")
        self.assertNotIn(PRIVATE_VALUE, payload)
        self.assertNotIn(HERO, result.stdout)
        return json.loads(payload)

    def test_unobserved_roots_and_absent_body_are_explicit(self):
        report = self.read_report()
        self.assertEqual(report["observation_count"], 0)
        known, missing = report["heroes"]
        self.assertEqual(known["hero"], HERO)
        self.assertEqual(known["root_graphs"], [42, 43, 44, 45])
        self.assertEqual(known["missing_root_graphs"], [43, 45])
        self.assertEqual(known["scenarios"][0]["counts"]["no_scenario"], 6)
        self.assertIsNone(known["scenarios"][0]["frames"])
        self.assertEqual(missing["body_status"], "missing")
        self.assertEqual(missing["scenarios"][0]["graphs"], [])

    def test_site_join_keeps_rejections_truncation_and_child_graphs(self):
        self.write_observation(self.observation)
        report = self.read_report(self.observation)
        scenario = report["heroes"][0]["scenarios"][0]
        self.assertTrue(scenario["truncated"])
        self.assertEqual(scenario["source_revision"], REVISION)
        self.assertEqual(scenario["frames"], 900)
        self.assertEqual(report["graph_bundle_sha256"], self.bundle_digest)
        self.assertEqual(scenario["counts"], {
            "observed": 2, "client_skipped": 1, "unsupported": 1,
            "registration_rejection": 1, "not_observed": 2, "no_scenario": 0,
        })
        graph = scenario["graphs"][0]
        self.assertEqual([row["coverage"] for row in graph["nodes"]], [
            "observed", "client_skipped", "unsupported", "registration_rejection", "not_observed",
        ])
        self.assertEqual(graph["nodes"][1], {
            "node": 2, "rid": 109, "class": "SyntheticState", "state": 0,
            "scope": "client_only", "coverage": "client_skipped",
            "client_scope_skipped": True, "diagnostics": [],
        })
        self.assertFalse(graph["nodes"][2]["client_scope_skipped"])
        self.assertIsNone(graph["nodes"][3]["client_scope_skipped"])
        self.assertEqual(scenario["graphs"][-1]["origin"], "observed_non_root")
        self.assertEqual(scenario["graphs"][-1]["graph"], 51)
        self.assertEqual(
            [(row["graph"], row["node"], row["reason"]) for row in scenario["unmatched_sites"]],
            [(42, 600, "unknown_node"), (79, 1, "unknown_graph"), (79, 2, "unknown_graph")],
        )

    def test_observations_of_unknown_hero_stay_unmatched(self):
        self.write_observation(self.observation, hero=UNKNOWN_HERO, observations=[target(42, 1)],
                               diagnostics=[])
        self.write_observation(self.secondary)
        report = self.read_report(self.observation, self.secondary)
        self.assertEqual(report["observation_count"], 2)
        row, = report["unmatched_hero_observations"]
        self.assertEqual(row["hero"], UNKNOWN_HERO)
        self.assertEqual(row["graphs"], [])
        self.assertEqual(row["unmatched_sites"][0]["reason"], "unknown_hero")

    def test_sidecar_validation_and_output_exclusivity(self):
        for overrides in (
            {"graph_bundle_sha256": "b" * 64},
            {"source_revision": "not-a-revision"},
            {"available": False},
            {"observations": [target(42, 1), target(42, 1, True)]},
            {"observations": [target(True, 1)]},
            {"diagnostics": [diagnostic(42, 2, "unknown")]},
        ):
            with self.subTest(overrides=overrides):
                self.write_observation(self.observation, **overrides)
                result = self.run_report(self.observation)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.output.exists())
        self.output.write_bytes(b"existing")
        result = self.run_report()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.output.read_bytes(), b"existing")


if __name__ == "__main__":
    unittest.main()
