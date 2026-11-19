//! Compare local semantic gameplay traces and keep detailed differences on disk.

use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use orpheus_golden_trace::{
    Comparison, Difference, compare_bound, compare_with_rng_setup_diagnostic, validate_candidate,
    validate_reference,
};
use serde_json::{Value, json};

const USAGE: &str = "Usage: compare_golden REFERENCE.jsonl CANDIDATE.jsonl [--report NEW_FILE]\n       compare_golden --check-reference REFERENCE.jsonl\n       compare_golden --check-candidate CANDIDATE.jsonl";

fn write_report(path: &Path, report: &Value) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut file, report)?;
    writeln!(file)?;
    file.flush()?;
    file.persist_noclobber(path)?;
    Ok(())
}

fn difference_report(diff: Difference) -> Value {
    json!({
        "tick": diff.tick,
        "phase": diff.phase,
        "entity": diff.entity,
        "field": diff.field,
        "expected": {"present": diff.expected.is_some(), "value": diff.expected},
        "actual": {"present": diff.actual.is_some(), "value": diff.actual}
    })
}

fn run(args: &[OsString]) -> Result<u8, &'static str> {
    if matches!(args, [arg] if arg == "--help" || arg == "-h") {
        println!("{USAGE}");
        return Ok(0);
    }
    let (reference, candidate, output) = match args {
        [flag, path] if flag == "--check-reference" || flag == "--check-candidate" => {
            let reader = BufReader::new(File::open(path).map_err(|_| "Cannot open trace.")?);
            let (label, result) = if flag == "--check-reference" {
                ("valid-reference", validate_reference(reader))
            } else {
                ("valid-candidate", validate_candidate(reader))
            };
            if let Err(error) = result {
                eprintln!("{error}");
                return Ok(2);
            }
            println!("{label}");
            return Ok(0);
        }
        [a, b] => (a, b, None),
        [a, b, flag, path] if flag == "--report" => (a, b, Some(PathBuf::from(path))),
        _ => return Err(USAGE),
    };
    let reference = File::open(reference).map_err(|_| "Cannot open the reference trace.")?;
    let candidate = File::open(candidate).map_err(|_| "Cannot open the candidate trace.")?;
    let reference = BufReader::new(reference);
    let candidate = BufReader::new(candidate);
    let comparison = if output.is_some() {
        compare_with_rng_setup_diagnostic(reference, candidate)
    } else {
        compare_bound(reference, candidate).map(|result| (result, None))
    };
    let (code, status, report) = match comparison {
        Ok((Comparison::Match, _)) => (0, "match", json!({"result": "match"})),
        Ok((Comparison::Mismatch(diff), _)) => {
            let mut report = difference_report(diff);
            report["result"] = json!("mismatch");
            (1, "mismatch", report)
        }
        Ok((Comparison::Incompatible(fields), diagnostic)) => {
            let mut report = json!({"result":"incompatible","fields":fields});
            if let Some(diagnostic) = diagnostic {
                report["rng_setup_diagnostic"] = json!({
                    "compared_ticks": diagnostic.compared_ticks,
                    "first_observed_difference": diagnostic.first_observed_difference.map(difference_report),
                });
            }
            (2, "incompatible", report)
        }
        Err(error) => (
            2,
            "incomplete",
            json!({"result": "incomplete", "reason": error.to_string()}),
        ),
    };
    if let Some(path) = output {
        write_report(&path, &report)
            .map_err(|_| "Could not create the requested private report.")?;
    }
    println!("{status}");
    Ok(code)
}

fn main() -> ExitCode {
    match run(&env::args_os().skip(1).collect::<Vec<_>>()) {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}
