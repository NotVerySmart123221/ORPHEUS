//! Invoke the same local comparison command used by maintainers. All rows are invented.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::process::{Command, Output};
use tempfile::TempDir;

const REFERENCE: &[u8] = include_bytes!("fixtures/reference.jsonl");

fn digest(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

fn line(row: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(row).unwrap();
    bytes.push(b'\n');
    bytes
}

fn candidate(edit: impl FnOnce(&mut [Value])) -> Vec<u8> {
    trace("candidate", edit)
}

fn trace(role: &str, edit: impl FnOnce(&mut [Value])) -> Vec<u8> {
    let mut rows: Vec<Value> = serde_json::Deserializer::from_slice(REFERENCE)
        .into_iter()
        .collect::<Result<_, _>>()
        .unwrap();
    if role == "candidate" {
        rows[0]["role"] = json!("candidate");
        rows[0]["source_rev"] = json!("e".repeat(40));
        rows[0]["toolchain"] = json!("synthetic-rust");
    }
    edit(&mut rows);
    let last = rows.len() - 1;
    let mut schedule = Vec::new();
    for tick in &mut rows[1..last] {
        let projection = json!({"effects": tick["effects"], "state": tick["state"]});
        tick["digest"] = json!(digest(&serde_json::to_vec(&projection).unwrap()));
        schedule.extend(line(&json!({
            "elapsed_us": tick["elapsed_us"], "inputs": tick["inputs"],
            "missed": tick["missed"], "tick": tick["tick"]
        })));
    }
    rows[0]["schedule_sha256"] = json!(digest(&schedule));
    let mut bytes: Vec<_> = rows[..last].iter().flat_map(line).collect();
    rows[last]["sha256"] = json!(digest(&bytes));
    bytes.extend(line(&rows[last]));
    bytes
}

struct Files(TempDir);

impl Files {
    fn new(candidate: &[u8]) -> Self {
        Self::with_traces(REFERENCE, candidate)
    }

    fn with_traces(reference: &[u8], candidate: &[u8]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("reference"), reference).unwrap();
        fs::write(dir.path().join("candidate"), candidate).unwrap();
        Self(dir)
    }

    fn compare(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_compare_golden"))
            .current_dir(self.0.path())
            .args(["reference", "candidate"])
            .args(args)
            .output()
            .unwrap()
    }

    fn report(&self) -> Value {
        serde_json::from_slice(&fs::read(self.0.path().join("report.json")).unwrap()).unwrap()
    }
}

fn field_mut<'a>(tick: &'a mut Value, entity: &str, field: &str) -> &'a mut Value {
    let row = tick["state"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row[0] == entity && row[1] == field)
        .unwrap();
    &mut row[2]
}

#[test]
fn accepts_a_python_written_reference_with_a_retirement_gap() {
    let files = Files::new(&candidate(|_| {}));
    let outcome = files.compare(&[]);
    assert!(outcome.status.success(), "{outcome:?}");
    assert_eq!(outcome.stdout, b"match\n");
    assert!(outcome.stderr.is_empty());
}

#[test]
fn reports_the_earliest_field_privately_and_preserves_an_existing_report() {
    let files = Files::new(&candidate(|rows| {
        *field_mut(&mut rows[2], "player/0/body/1", "ammo") = json!(1);
        rows[2]["effects"][0].as_array_mut().unwrap().reverse();
        *field_mut(&mut rows[4], "player/0/body/1", "ammo") = json!(0);
    }));
    let result = files.compare(&["--report", "report.json"]);
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(result.stdout, b"mismatch\n");
    assert!(result.stderr.is_empty());
    let report = files.report();
    assert_eq!(report["tick"], 1);
    assert_eq!(report["phase"], "state");
    assert_eq!(report["entity"], "player/0/body/1");
    assert_eq!(report["field"], "ammo");
    assert_eq!(report["expected"], json!({"present":true,"value":2}));
    assert_eq!(report["actual"], json!({"present":true,"value":1}));
    assert_eq!(
        files.compare(&["--report", "report.json"]).status.code(),
        Some(2)
    );
    assert_eq!(files.report(), report);
    assert_eq!(
        files.compare(&["--report", "reference"]).status.code(),
        Some(2)
    );
    assert_eq!(
        fs::read(files.0.path().join("reference")).unwrap(),
        REFERENCE
    );
}

#[test]
fn a_later_truncation_invalidates_an_earlier_difference() {
    let mut bytes = candidate(|rows| {
        *field_mut(&mut rows[1], "player/0/body/1", "ammo") = json!(0);
    });
    let end = bytes[..bytes.len() - 1]
        .iter()
        .rposition(|b| *b == b'\n')
        .unwrap();
    bytes.truncate(end + 1);
    let result = Files::new(&bytes).compare(&[]);
    assert_eq!(result.status.code(), Some(2));
    assert_eq!(result.stdout, b"incomplete\n");
    assert!(result.stderr.is_empty());
}

#[test]
fn distinguishes_an_unaccepted_command_from_a_missing_spawn() {
    let files = Files::new(&candidate(|rows| {
        *field_mut(&mut rows[2], "player/0/body/1", "accepted_frame") = Value::Null;
        *field_mut(&mut rows[2], "bot/0/body/1", "alive") = json!(true);
    }));
    assert_eq!(
        files.compare(&["--report", "report.json"]).status.code(),
        Some(1)
    );
    let report = files.report();
    assert_eq!(report["phase"], "acceptance");
    assert_eq!(report["actual"], json!({"present":true,"value":null}));
    let files = Files::new(&candidate(|rows| {
        for tick in &mut rows[1..5] {
            *field_mut(tick, "player/0", "body") = Value::Null;
            tick["state"]
                .as_array_mut()
                .unwrap()
                .retain(|r| r[0] != "player/0/body/1");
        }
        rows[2]["effects"][0].as_array_mut().unwrap().remove(0);
    }));
    assert_eq!(
        files.compare(&["--report", "report.json"]).status.code(),
        Some(1)
    );
    let report = files.report();
    assert_eq!(report["tick"], 0);
    assert_eq!(report["entity"], "player/0");
    assert_eq!(report["field"], "body");
}

#[test]
fn preserves_signed_zero_and_rejects_nonfinite_coordinates() {
    for (bits, exit) in [("8000000000000000", 1), ("7ff8000000000001", 2)] {
        let files = Files::new(&candidate(|rows| {
            field_mut(&mut rows[1], "player/0/body/1", "position_bits")[1] = json!(bits);
        }));
        assert_eq!(
            files.compare(&["--report", "report.json"]).status.code(),
            Some(exit)
        );
        if exit == 1 {
            assert_eq!(files.report()["field"], "position_bits");
        }
    }
}

#[test]
fn preserves_delivery_order_and_group_boundaries() {
    for regroup in [true, false] {
        let bytes = candidate(|rows| {
            let group = rows[2]["effects"][0].as_array().unwrap().clone();
            rows[2]["effects"] = if regroup {
                json!([[group[0]], [group[1]]])
            } else {
                json!([[group[1], group[0]]])
            };
        });
        let files = Files::new(&bytes);
        assert_eq!(
            files.compare(&["--report", "report.json"]).status.code(),
            Some(1)
        );
        let report = files.report();
        assert_eq!(report["phase"], "effects");
        assert_eq!(report["tick"], 1);
    }
}

#[test]
fn empty_observations_cannot_certify_a_reference_scenario() {
    let empty = |rows: &mut [Value]| {
        for tick in &mut rows[1..5] {
            tick["inputs"] = json!([]);
            tick["effects"] = json!([]);
            tick["state"] = json!([["bot/0", "body", null], ["player/0", "body", null]]);
        }
    };
    let files = Files::with_traces(&trace("reference", empty), &candidate(empty));
    assert_eq!(files.compare(&[]).status.code(), Some(2));
}

#[test]
fn input_comparison_uses_the_decoded_subframe_flag() {
    for (buttons, click, exit) in [
        (0x10000, json!([255, -10, 20]), 0),
        (1, Value::Null, 0),
        (0x10000, Value::Null, 2),
        (1, json!([0, 1, 2]), 2),
        (1 << 24, Value::Null, 2),
    ] {
        let edit = |rows: &mut [Value]| {
            rows[2]["inputs"][0]["buttons"] = json!(buttons);
            rows[2]["inputs"][0]["click"] = click.clone();
        };
        let files = Files::with_traces(&trace("reference", edit), &candidate(edit));
        assert_eq!(files.compare(&[]).status.code(), Some(exit));
    }
}

#[test]
fn incompatible_evidence_stays_incomplete_even_with_valid_hashes() {
    for change in [
        "platform",
        "profile",
        "input",
        "stale generation",
        "missing field",
        "raw output",
    ] {
        let bytes = candidate(|rows| match change {
            "platform" => rows[0]["platform"] = json!("windows-x86_64"),
            "profile" => rows[0]["profile_sha256"] = json!("9".repeat(64)),
            "input" => rows[2]["inputs"][0]["buttons"] = json!(0),
            "stale generation" => {
                for row in rows[4]["state"].as_array_mut().unwrap() {
                    if row[0] == "bot/0/body/2" {
                        row[0] = json!("bot/0/body/1");
                    }
                    if row[0] == "bot/0" && row[1] == "body" {
                        row[2] = json!("bot/0/body/1");
                    }
                }
                rows[4]["effects"][0][0][2] = json!("bot/0/body/1");
            }
            "missing field" => {
                rows[1]["state"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|r| r[1] != "ammo");
            }
            _ => {
                rows[1]["effects"] = json!([[["owner", "raw", "player/0/body/1", "private-value"]]])
            }
        });
        let result = Files::new(&bytes).compare(&[]);
        assert_eq!(result.status.code(), Some(2), "{change}: {result:?}");
        assert_eq!(result.stdout, b"incomplete\n");
        assert!(result.stderr.is_empty());
    }
}
