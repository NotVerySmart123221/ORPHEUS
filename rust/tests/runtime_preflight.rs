use std::{fs, path::Path, process::Command};

use orpheus::runtime::{BNET_DESCRIPTORS, LOBBY_RUNTIME_DATA, MATCH_RUNTIME_DATA};

fn check(root: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_orpheus_server"))
        .arg("--check-runtime")
        .arg("--runtime")
        .arg(root)
        .output()
        .unwrap()
}

#[test]
fn check_runtime_groups_missing_inputs_without_starting_the_server() {
    let dir = tempfile::tempdir().unwrap();
    let result = check(dir.path());
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    let error = String::from_utf8(result.stderr).unwrap();
    for text in [
        "Tool outputs from authorized local inputs",
        "Bring prepared research data",
        "Blocked without restricted local inputs",
        "collision/0688.col",
        BNET_DESCRIPTORS,
    ] {
        assert!(error.contains(text), "missing diagnostic for {text}");
    }
}

#[test]
fn check_runtime_checks_signing_length_without_reading_runtime_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for path in ["data", "bnet", "collision"] {
        fs::create_dir(root.join(path)).unwrap();
    }
    for path in LOBBY_RUNTIME_DATA.iter().chain(MATCH_RUNTIME_DATA) {
        fs::write(root.join("data").join(path), []).unwrap();
    }
    for path in [BNET_DESCRIPTORS, "collision/0688.col"] {
        fs::write(root.join(path), []).unwrap();
    }

    let malformed = check(root);
    assert!(!malformed.status.success());
    assert!(
        String::from_utf8(malformed.stderr)
            .unwrap()
            .contains("Expected exactly 64 bytes.")
    );

    fs::write(root.join("data/lobby_state_blob.key"), [0; 64]).unwrap();
    let valid = check(root);
    assert!(valid.status.success());
    assert!(
        String::from_utf8(valid.stdout)
            .unwrap()
            .contains("Runtime file presence and signing input length checks passed.")
    );
}
