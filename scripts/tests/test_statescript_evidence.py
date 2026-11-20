"""Synthetic end-to-end checks for local StateScript evidence summaries."""

import gzip
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from zipfile import ZipFile


SCRIPT = Path(__file__).resolve().parents[1] / "summarize_statescript_evidence.py"
GRAPH_A = "A90000000000002A"
GRAPH_B = "A90000000000002B"
MESSAGE_ID = "0xA100000000008821"
PARAMETER_ID = "0xA200000000000101"
UNBOUND_PARAMETER_ID = "0xA200000000000102"
LISTING_LITERAL = "invented-listing-value-stays-local"
PREPARED_LITERAL = "invented-prepared-value-stays-local"
OUTPUTS = ("field-contracts.json", "gamemessage-catalog.json", "evidence-manifest.json")

LISTINGS = {
    GRAPH_A: f"""# graph {GRAPH_A}
state Stage_19:  # SyntheticStage, state 0
    SyntheticStage(
        m_data=Config(m_value=var[401], m_note="{LISTING_LITERAL}"),
        m_present=True,
    )
state Stage_20:  # SyntheticStage, state 1
    SyntheticStage(m_present=False)
state Stage_21:  # SyntheticStage, state 2
    pass

def SendGameMessage_75():  # STUStatescriptActionSendGameMessage
    send_game_message(
        GameMessage({MESSAGE_ID}),
        targets=shared_var[510],
        params=[Param(m_value=var[511], m_B5051BCE=asset({PARAMETER_ID}))],
    )
    publish_game_message(
        m_gameMessage=GameMessage({MESSAGE_ID}),
        m_target=Recipient(),
        m_params=[Param(m_value=member(var[518]), m_B5051BCE=asset({PARAMETER_ID}))],
    )
    publish_game_message(m_gameMessage=var[512], m_target=var[513])
    publish_game_message(m_gameMessage=UnknownMessage(Nested(m_target=var[514])))
""",
    GRAPH_B: f"""# graph {GRAPH_B}
state Listen_83:  # STUStatescriptStateGameMessageEntry, state 0
    subscribe_game_message(
        m_gameMessage=GameMessage({MESSAGE_ID}),
        m_params=[
            Param(m_out_Var=shared_var[515], m_B5051BCE=asset({PARAMETER_ID})),
            Param(m_out_Var=var[516]),
            Param(m_B5051BCE=asset({UNBOUND_PARAMETER_ID})),
        ],
    )
on_game_message(
    GameMessage({MESSAGE_ID}), rid=88,
    m_params=[Param(m_out_Var=shared_var[519], m_B5051BCE=asset({PARAMETER_ID}))],
)(var[517] <- param {PARAMETER_ID}):  # STUStatescriptEntryGameMessage
    pass
""",
}


def prepared_node(pos, rid, state, fields):
    return {
        "pos": pos,
        "rid": rid,
        "state": state,
        "class": "SyntheticStage",
        "client_only": False,
        "server_only": False,
        "fields": {"m_2BBEEAB8": True, **fields},
    }


def run_summary(source, bundle, families, output):
    return subprocess.run(
        [
            sys.executable,
            "-B",
            str(SCRIPT),
            "--evidence",
            str(source),
            "--graph-bundle",
            str(bundle),
            "--families",
            str(families),
            "--output-dir",
            str(output),
        ],
        capture_output=True,
        text=True,
    )


class StateScriptEvidenceTest(unittest.TestCase):
    def test_local_inputs_produce_identical_bounded_evidence_without_overwrite(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            listings = root / "listings"
            listings.mkdir()
            archive = root / "listings.zip"
            with ZipFile(archive, "w") as zip_file:
                for name, contents in reversed(LISTINGS.items()):
                    zip_file.writestr(f"graphs/{name}.txt", contents)
                    listing = listings / f"{name}.txt"
                    listing.write_bytes(("\ufeff" + contents.replace("\n", "\r\n")).encode("utf-8"))

            bundle = root / "prepared.json.gz"
            with gzip.open(bundle, "wt", encoding="utf-8") as file:
                json.dump(
                    {
                        "version": 1,
                        "graphs": [
                            {
                                "index": 42,
                                "states": [5, 6, 7],
                                "nodes": [
                                    prepared_node(5, 19, 0, {
                                        "m_data": {
                                            "$": "SyntheticConfig",
                                            "m_note": PREPARED_LITERAL,
                                            "m_value": 23,
                                        },
                                        "m_present": True,
                                    }),
                                    prepared_node(6, 20, 1, {"m_present": False}),
                                    prepared_node(7, 99, 2, {"m_mismatched": 17}),
                                ],
                            },
                        ],
                    },
                    file,
                )
            family_file = root / "families.json"
            family_file.write_text(
                json.dumps({"sample": ["SyntheticStage", "UnseenClass"]}),
                encoding="utf-8",
            )

            original_sources = [path.read_bytes() for path in (archive, bundle, family_file)]
            zip_output = root / "zip-output"
            directory_output = root / "directory-output"
            for source, output in ((archive, zip_output), (listings, directory_output)):
                result = run_summary(source, bundle, family_file, output)
                self.assertEqual(result.returncode, 0, result.stderr)

            self.assertEqual(sorted(item.name for item in zip_output.iterdir()), sorted(OUTPUTS))
            reports = {name: (zip_output / name).read_bytes() for name in OUTPUTS}
            self.assertEqual(
                reports,
                {name: (directory_output / name).read_bytes() for name in OUTPUTS},
            )
            fields, messages, manifest = (json.loads(reports[name]) for name in OUTPUTS)
            self.assertEqual(manifest["schema_version"], 1)
            self.assertEqual(manifest["listing_graphs"], 2)
            self.assertEqual(manifest["state_markers"], 4)
            self.assertRegex(manifest["source_sha256"], r"^[0-9a-f]{64}$")
            self.assertTrue(all(
                report["source_sha256"] == manifest["source_sha256"]
                for report in (fields, messages)
            ))
            for text in reports.values():
                self.assertNotIn(LISTING_LITERAL.encode(), text)
                self.assertNotIn(PREPARED_LITERAL.encode(), text)
                self.assertNotIn(b"UnknownMessage", text)
                self.assertNotIn(b"m_mismatched", text)
                self.assertNotIn(b"var[", text)

            self.assert_field_summary(fields, hashlib.sha256(bundle.read_bytes()).hexdigest())
            self.assert_message_summary(messages)
            duplicate = run_summary(archive, bundle, family_file, zip_output)
            self.assertNotEqual(duplicate.returncode, 0)
            self.assertIn("already exists", duplicate.stderr)
            self.assertEqual(reports, {name: (zip_output / name).read_bytes() for name in OUTPUTS})
            self.assertEqual(
                original_sources,
                [path.read_bytes() for path in (archive, bundle, family_file)],
            )

    def assert_field_summary(self, fields, bundle_digest):
        classes = {row["class"]: row for row in fields["listing"]["classes"]}
        stage = classes["SyntheticStage"]
        self.assertEqual(
            (stage["occurrences"], stage["rendered"], stage["omitted"]),
            (3, 2, 1),
        )
        paths = {row["path"]: row for row in stage["fields"]}
        listed = paths["m_data.m_value"]
        self.assertEqual(
            (listed["present"], listed["not_rendered"], listed["rendered_without_path"]),
            (1, 2, 1),
        )
        self.assertEqual(listed["kinds"], {"reference": 1})
        self.assertEqual(listed["dynamic"], 1)
        self.assertEqual(paths["m_data.m_note"]["kinds"], {"string": 1})
        self.assertEqual(paths["m_present"]["present"], 2)

        prepared = fields["prepared_bundle"]
        self.assertEqual(prepared["bundle_sha256"], bundle_digest)
        self.assertEqual(prepared["overlapping_graphs"], 1)
        self.assertEqual(prepared["matched_states"], 2)
        self.assertEqual(prepared["unmatched_markers"], 1)
        stage = next(row for row in prepared["classes"] if row["class"] == "SyntheticStage")
        paths = {row["path"]: row for row in stage["fields"]}
        self.assertEqual(paths["m_data"]["kinds"], {"class:SyntheticConfig": 1, "object": 1})
        self.assertEqual(paths["m_data.m_value"]["kinds"], {"integer": 1})
        self.assertEqual(paths["m_data.m_value"]["present"], 1)
        self.assertEqual(paths["m_data.m_value"]["missing"], 1)
        self.assertEqual(paths["m_present"]["kinds"], {"boolean": 2})
        self.assertEqual(fields["families"]["sample"], [
            {"class": "SyntheticStage", "listing_occurrences": 3, "prepared_matched_states": 2},
            {"class": "UnseenClass", "listing_occurrences": 0, "prepared_matched_states": 0},
        ])

    def assert_message_summary(self, messages):
        self.assertEqual(messages["message_count"], 1)
        self.assertEqual(messages["observed_endpoints"], 6)
        message, = messages["messages"]
        self.assertEqual((message["message_id"], message["sends"], message["receives"]), (
            MESSAGE_ID, 2, 2,
        ))
        endpoints = {entry["form"]: entry for entry in message["endpoints"]}
        sender = endpoints["send"]
        self.assertEqual((sender["graph"], sender["rid"], sender["audience"]), (42, 75, "dynamic"))
        receiver = endpoints["subscription"]
        self.assertEqual((receiver["graph"], receiver["rid"], receiver["state"]), (43, 83, 0))
        self.assertEqual(endpoints["entry"]["rid"], 88)
        for form, slot, scope, source in (
            ("send", 511, "instance", "params"),
            ("subscription", 515, "shared", "params"),
            ("entry", 519, "shared", "params"),
            ("entry", 517, "instance", "entry_binding"),
        ):
            with self.subTest(form=form, source=source):
                row = next(row for row in endpoints[form]["parameters"] if row["source"] == source)
                self.assertEqual((row["parameter_id"], row["parameter_slot"]), (PARAMETER_ID, 257))
                self.assertEqual(row["variable"], {"scope": scope, "slot": slot})
                self.assertEqual(row["operand_resolution"], "direct")

        publisher = endpoints["publish"]
        self.assertEqual(publisher["audience"], "explicit")
        published, = publisher["parameters"]
        self.assertEqual(published["parameter_slot"], 257)
        self.assertEqual(published["operand_resolution"], "dynamic")
        self.assertIsNone(published["variable"])

        partial = receiver["parameters"][1]
        self.assertIsNone(partial["parameter_id"])
        self.assertIsNone(partial["parameter_slot"])
        self.assertEqual(partial["resolution"], "unresolved")
        self.assertEqual(partial["variable"], {"scope": "instance", "slot": 516})
        self.assertEqual(partial["operand_resolution"], "direct")
        unbound = receiver["parameters"][2]
        self.assertEqual(unbound["parameter_id"], UNBOUND_PARAMETER_ID)
        self.assertEqual(unbound["operand_resolution"], "absent")
        self.assertIsNone(unbound["variable"])
        self.assertEqual(
            [
                (row["resolution"], row["message_id"], row["audience"])
                for row in messages["unresolved_endpoints"]
            ],
            [("dynamic", None, "dynamic"), ("unresolved", None, "absent")],
        )


if __name__ == "__main__":
    unittest.main()
