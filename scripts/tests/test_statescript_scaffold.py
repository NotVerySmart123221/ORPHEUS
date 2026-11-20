"""Check that scaffold generation exports only invented examples."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "generate_statescript_scaffold.py"
SHAPES = (
    "array", "boolean", "call", "expression", "integer", "null", "number", "object",
    "reference", "string",
)


def section(label, kinds):
    return {
        "classes": [
            {
                "class": f"PrivateClass{label}",
                "fields": [
                    {
                        "path": f"PrivateField{label}.other",
                        "kinds": kinds,
                        "examples": [{"graph": 987654321, "rid": 496}],
                        "value": f"PrivateLiteral{label}",
                    },
                ],
            },
        ],
    }


def run_generator(report, output, *selection):
    return subprocess.run(
        [
            sys.executable, "-B", str(SCRIPT),
            "--field-contracts", str(report), "--output", str(output), *selection,
        ],
        capture_output=True,
        text=True,
    )


class StateScriptScaffoldTest(unittest.TestCase):
    def test_scaffold_depends_only_on_allowlisted_shapes(self):
        left_report = {
            "schema_version": 1,
            "source_sha256": "private-first-provenance",
            "listing": section("First", {
                kind: index for index, kind in enumerate(SHAPES, start=1) if kind != "integer"
            }),
            "prepared_bundle": section("Prepared", {"integer": 2, "class:PrivateType": 8}),
            "timing": [2_137_992.75, "private-unpublished-timing"],
        }
        right_report = {
            "schema_version": 1,
            "source_sha256": "private-second-provenance",
            "listing": section("Second", {kind: 100 for kind in reversed(SHAPES)}),
            "prepared_bundle": None,
            "sequence": ["private-unpublished-sequence"],
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first = root / "first-report.json"
            second = root / "second-report.json"
            first.write_text(json.dumps(left_report), encoding="utf-8")
            second.write_text(json.dumps(right_report), encoding="utf-8")
            first_output = root / "first-scaffold.json"
            second_output = root / "second-scaffold.json"
            for report, output in ((first, first_output), (second, second_output)):
                result = run_generator(report, output)
                self.assertEqual(result.returncode, 0, result.stderr)

            original = first_output.read_bytes()
            self.assertEqual(original, second_output.read_bytes())
            self.assertNotIn(b"Private", original)
            self.assertNotIn(b"private-", original)
            self.assertNotIn(b"987654321", original)
            self.assertNotIn(b"2137992", original)
            fixture = json.loads(original)
            self.assertEqual(fixture["schema_version"], 1)
            self.assertEqual(fixture["review_status"], "draft")
            self.assertEqual(fixture["evidence"], "synthetic-scaffold")
            draft = fixture["input"]
            self.assertEqual(draft["bundle"]["version"], 1)
            graph, = draft["bundle"]["graphs"]
            self.assertEqual(graph["index"], draft["start"]["graph"])
            node, = graph["nodes"]
            self.assertEqual(node["class"], "ScaffoldState")
            self.assertEqual(node["state_behavior"], "plain")
            self.assertTrue(node["server_only"])
            self.assertFalse(node["client_only"])
            self.assertEqual(graph["states"], [node["pos"]])
            self.assertEqual(graph["remote"], [])
            self.assertEqual(node["state"], draft["start"]["initial_state"])
            self.assertEqual(tuple(node["fields"]), tuple(f"scaffold_example_{kind}" for kind in SHAPES))
            self.assertIs(node["fields"]["scaffold_example_boolean"], True)
            self.assertIsNone(node["fields"]["scaffold_example_null"])
            self.assertEqual(fixture["expected"], {"active_state_slots": [0], "diagnostics": []})

            refused = run_generator(second, first_output)
            self.assertNotEqual(refused.returncode, 0)
            self.assertIn("already exists", refused.stderr)
            self.assertEqual(first_output.read_bytes(), original)

    def test_exact_class_and_family_select_shapes_without_retaining_scope(self):
        family = "Private Capability Group"
        report = {
            "schema_version": 1,
            "listing": {"classes": [
                *section("Alpha", {"boolean": 4})["classes"],
                *section("Beta", {"array": 2})["classes"],
                *section("Excluded", {"null": 9})["classes"],
            ]},
            "prepared_bundle": section("Alpha", {"integer": 3}),
            "families": {family: [
                {"class": "PrivateClassBeta", "listing_occurrences": 2},
                {"class": "PrivateClassUnobserved", "listing_occurrences": 0},
                {"class": "PrivateClassAlpha", "prepared_matched_states": 3},
            ]},
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "field-contracts.json"
            source.write_text(json.dumps(report), encoding="utf-8")
            for scope, flags, kinds in (
                ("default", (), {"array", "boolean", "integer", "null"}),
                ("class", ("--class", "PrivateClassAlpha"), {"boolean", "integer"}),
                ("family", ("--family", family), {"array", "boolean", "integer"}),
            ):
                with self.subTest(scope=scope):
                    destination = root / f"{scope}.json"
                    result = run_generator(source, destination, *flags)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    original = destination.read_bytes()
                    self.assertNotIn(b"Private", original)
                    graph, = json.loads(original)["input"]["bundle"]["graphs"]
                    self.assertEqual(
                        set(graph["nodes"][0]["fields"]),
                        {f"scaffold_example_{kind}" for kind in kinds},
                    )
                    refused = run_generator(source, destination, *flags)
                    self.assertNotEqual(refused.returncode, 0)
                    self.assertIn("already exists", refused.stderr)
                    self.assertEqual(destination.read_bytes(), original)

            for flags in (
                ("--class", "privateclassalpha"),
                ("--class", "PrivateClassAlpha*"),
                ("--family", "Unknown Private Family"),
                ("--class", "PrivateClassAlpha", "--family", family),
            ):
                with self.subTest(flags=flags):
                    destination = root / "rejected.json"
                    refused = run_generator(source, destination, *flags)
                    self.assertNotEqual(refused.returncode, 0)
                    self.assertFalse(destination.exists())

    def test_wrong_schema_and_unknown_kinds_leave_no_output(self):
        cases = (
            {"schema_version": True, "listing": section("WrongVersion", {"integer": 1})},
            {"schema_version": 1, "listing": section("Empty", {
                "integer": 0, "boolean": True, "class:PrivateType": 1, "unknown": 8,
            })},
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for index, report in enumerate(cases):
                with self.subTest(index=index):
                    source = root / f"report-{index}.json"
                    output = root / f"scaffold-{index}.json"
                    source.write_text(json.dumps(report), encoding="utf-8")
                    result = run_generator(source, output)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
