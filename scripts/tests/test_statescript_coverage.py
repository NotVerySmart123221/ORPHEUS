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


def node(pos, rid, state, client_only=False, **overrides):
    return {
        "pos": pos, "rid": rid, "state": state, "class": "SyntheticState",
        "client_only": client_only, "server_only": False,
        "fields": {"private": PRIVATE_VALUE, "m_2BBEEAB8": state is not None},
        **overrides,
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
        self.prepared = {
            "version": 1,
            "graphs": [
                {"index": 51, "states": [None, None, None, None, 6], "nodes": [node(6, 502, 4)]},
                {"index": 42, "states": [2, 3, 4], "nodes": [
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
        self.write_bundle()
        self.catalog.write_text(
            json.dumps({NO_BODY[2:]: {"body": OTHER_BODY}, HERO: {"body": BODY}}),
            encoding="utf-8",
        )

    def write_bundle(self):
        self.bundle.write_bytes(gzip.compress(json.dumps(self.prepared).encode(), mtime=0))
        self.bundle_digest = hashlib.sha256(self.bundle.read_bytes()).hexdigest()

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

    def test_static_target_children_include_unvisited_transitive_and_missing_graphs(self):
        def tracker(pos, state, child, **overrides):
            return node(pos, pos + 200, state, server_only=True,
                        state_behavior="track_entities", fields={
                            "targets": [], "child_graph": child, "target_variable": 7,
                        }, **overrides)

        self.prepared["graphs"][1] = {
            "index": 42, "states": [0], "nodes": [tracker(0, 0, 51)],
        }
        declared = tracker(1, 0, 60)
        declared.pop("state_behavior")
        declared["class"] = "SyntheticTracker"
        declared["server_only"] = False
        self.prepared["authored_classes"] = [
            {"class": "SyntheticTracker", "behavior": "track_entities_state"},
        ]
        self.prepared["graphs"][0] = {"index": 51, "states": [1], "nodes": [declared]}
        self.prepared["graphs"].extend([
            {"index": 60, "states": [2, 3], "nodes": [tracker(2, 0, 42), tracker(3, 1, 61)]},
            {"index": 70, "states": [4], "nodes": [tracker(4, 0, 71)]},
        ])
        self.prepared["graphs"][2]["nodes"][0]["fields"]["child_graph"] = 72
        self.write_bundle()
        scenario = self.read_report()["heroes"][0]["scenarios"][0]
        graphs = {row["graph"]: row for row in scenario["graphs"]}
        self.assertEqual(set(graphs), {42, 43, 44, 45, 51, 60, 61})
        self.assertEqual(scenario["missing_dependency_graphs"], [61])
        self.assertEqual(
            [(edge["source_graph"], edge["source_node"], edge["graph"], edge["present"])
             for edge in scenario["static_dependencies"]],
            [(42, 0, 51, True), (51, 1, 60, True), (60, 2, 42, True), (60, 3, 61, False)],
        )
        for graph in (51, 60, 61):
            self.assertEqual(graphs[graph]["origin"], "referenced_child")
        self.assertFalse(graphs[61]["present"])
        self.assertEqual(graphs[51]["nodes"][0]["scope"], "server_only")
        for graph in graphs.values():
            for row in graph["nodes"]:
                self.assertEqual(row["coverage"], "no_scenario")
                self.assertIsNone(row["client_scope_skipped"])

        self.output.unlink()
        self.write_observation(self.observation, observations=[target(51, 1)], diagnostics=[])
        scenario = self.read_report(self.observation)["heroes"][0]["scenarios"][0]
        graphs = {row["graph"]: row for row in scenario["graphs"]}
        self.assertEqual(graphs[51]["origin"], "observed_non_root")
        self.assertEqual(graphs[51]["nodes"][0]["coverage"], "observed")
        self.assertEqual(graphs[60]["origin"], "referenced_child")
        self.assertTrue(all(row["coverage"] == "not_observed" for row in graphs[60]["nodes"]))

    def test_invalid_declared_target_children_are_rejected(self):
        fields = {"targets": [], "child_graph": 51, "target_variable": 7}
        tracker = node(0, 200, 0, server_only=True,
                       state_behavior="track_entities", fields=fields)
        self.prepared["graphs"][1] = {"index": 42, "states": [0], "nodes": [tracker]}
        for key, value in (("child_graph", True), ("child_graph", -1),
                           ("child_graph", 2**32), ("target_variable", 0),
                           ("target_variable", 0.5), ("targets", None)):
            with self.subTest(key=key, value=value):
                tracker["fields"] = {**fields, key: value}
                self.write_bundle()
                result = self.run_report()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("invalid declared track entities child binding", result.stderr)
                self.assertFalse(self.output.exists())

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
            "scope": "client_only", "server_scope": "client_only",
            "runtime_classification": None, "coverage": "client_skipped",
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

    def test_static_state_metadata_respects_each_state_and_keeps_observations_separate(self):
        self.prepared["runtime_state_metadata"] = {
            "plain_classes": ["SyntheticState"],
            "client_states": [
                {"graph": 42, "state": 1, "rid": 101, "class": "SyntheticState"},
                {"graph": 42, "state": 0, "rid": 109, "class": "SyntheticState"},
            ],
        }
        self.prepared["authored_classes"] = [
            {"class": "authored.overlay", "behavior": "client_only"},
        ]
        graph = self.prepared["graphs"][1]
        graph["states"].extend([8, 9, 10, None, 12])
        graph["nodes"].extend([
            node(8, 201, 3, server_only=True),
            node(9, 202, 4, fields={}),
            node(10, 203, 5, server_only=True, state_behavior="plain"),
            node(11, 204, 6),
            node(12, 205, 7, **{"class": "authored.overlay"}),
        ])
        self.write_bundle()
        self.write_observation(
            self.observation,
            observations=[target(42, 3, True)],
            diagnostics=[diagnostic(42, 4, "registration_rejection")],
        )
        report = self.read_report(self.observation)
        scenario = report["heroes"][0]["scenarios"][0]
        rows = scenario["graphs"][0]["nodes"]
        self.assertEqual([row["node"] for row in rows], [1, 2, 3, 4, 5, 8, 9, 10, 11, 12])
        nodes = {row["node"]: row for row in rows}
        for pos, raw, server, annotation, status in (
            (1, "networked", "eligible", None, "not_observed"),
            (2, "client_only", "client_only", "client_state", "not_observed"),
            (3, "networked", "client_only", "client_state", "client_skipped"),
            (4, "networked", "eligible", "plain_candidate", "registration_rejection"),
            (8, "server_only", "eligible", "plain_candidate", "not_observed"),
            (9, "networked", "eligible", None, "not_observed"),
            (10, "server_only", "eligible", None, "not_observed"),
            (11, "networked", "eligible", None, "not_observed"),
            (12, "networked", "client_only", None, "not_observed"),
        ):
            with self.subTest(pos=pos):
                row = nodes[pos]
                self.assertEqual(
                    (row["scope"], row["server_scope"], row["runtime_classification"], row["coverage"]),
                    (raw, server, annotation, status),
                )
        self.assertIsNone(nodes[2]["client_scope_skipped"])
        self.assertEqual(scenario["counts"]["client_skipped"], 1)

    def test_directed_numeric_messages_validate_syntax_without_affecting_coverage(self):
        baseline = self.read_report()
        self.output.unlink()
        route = {"topic": "0x02400000000001D2", "parameter": "0x0A40000000000217"}
        metadata = self.prepared["runtime_state_metadata"] = {
            "directed_numeric_messages": [route],
        }
        self.write_bundle()
        annotated = self.read_report()
        self.assertNotEqual(annotated.pop("graph_bundle_sha256"), baseline.pop("graph_bundle_sha256"))
        self.assertEqual(annotated, baseline)
        self.output.unlink()

        for routes in ([route, route], [dict(route, parameter="0x0A40000000000000")]):
            with self.subTest(routes=routes):
                metadata["directed_numeric_messages"] = routes
                self.write_bundle()
                result = self.run_report()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("directed numeric message", result.stderr)

    def test_projected_message_topics_validate_syntax_without_affecting_coverage(self):
        baseline = self.read_report()
        self.output.unlink()

        topic = "0x9C00000000000001"
        metadata = self.prepared["runtime_state_metadata"] = {
            "projected_message_topics": [topic],
        }
        self.write_bundle()
        annotated = self.read_report()
        self.assertNotIn(topic, self.output.read_text(encoding="utf-8"))
        self.assertNotEqual(annotated.pop("graph_bundle_sha256"), baseline.pop("graph_bundle_sha256"))
        self.assertEqual(annotated, baseline)
        self.output.unlink()

        for topics in ([topic, topic], [topic.lower()], ["0x1"], ["0x" + "0" * 16], [1], topic):
            with self.subTest(topics=topics):
                metadata["projected_message_topics"] = topics
                self.write_bundle()
                result = self.run_report()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("projected message topic", result.stderr)
                self.assertFalse(self.output.exists())

    def test_runtime_client_rows_require_exact_eligible_states(self):
        client = {"graph": 42, "state": 1, "rid": 101, "class": "SyntheticState"}
        graph = self.prepared["graphs"][1]
        for annotations, states, server_only in (
            ([{**client, "graph": 88}], [2, 3, 4], False),
            ([{**client, "state": 0}], [2, 3, 4], False),
            ([{**client, "rid": 102}], [2, 3, 4], False),
            ([{**client, "class": "DifferentState"}], [2, 3, 4], False),
            ([client], [2, 4, 3], False),
            ([client], [2, 3, 4], True),
            ([client, client], [2, 3, 4], False),
        ):
            with self.subTest(annotations=annotations, states=states, server_only=server_only):
                self.prepared["runtime_state_metadata"] = {"client_states": annotations}
                graph["states"] = states
                next(row for row in graph["nodes"] if row["pos"] == 3)["server_only"] = server_only
                self.write_bundle()
                result = self.run_report()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("runtime client state", result.stderr)
                self.assertFalse(self.output.exists())

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
