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

fn seed_data_files(root: &Path) {
    let data = root.join("data");
    fs::create_dir_all(&data).unwrap();
    for path in LOBBY_RUNTIME_DATA.iter().chain(MATCH_RUNTIME_DATA) {
        fs::write(data.join(path), []).unwrap();
    }
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
    for path in ["bnet", "collision"] {
        fs::create_dir(root.join(path)).unwrap();
    }
    seed_data_files(root);
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

#[test]
fn server_loads_the_graph_bundle_enriched_by_the_runtime_packer() {
    use std::collections::BTreeSet;

    use flate2::{Compression, write::GzEncoder};
    use orpheus::{runtime::prepare_with_target_tags, script_graph::GraphStore};
    use serde_json::json;

    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    seed_data_files(&source);
    for path in ["cache/collision", "ow174/bnet", "003"] {
        fs::create_dir_all(source.join(path)).unwrap();
    }
    fs::write(source.join("cache/collision/0688.col"), []).unwrap();
    fs::write(source.join("ow174").join(BNET_DESCRIPTORS), []).unwrap();
    let graph_bundle = "data/game_graphs_174.json.gz";
    let mut encoder = GzEncoder::new(
        fs::File::create(source.join(graph_bundle)).unwrap(),
        Compression::default(),
    );
    serde_json::to_writer(
        &mut encoder,
        &json!({
            "version":1,"graphs":[],"missing":[],
            "bodies":{"0x0400000000000001":{"name":"Fixture","hero":null,"graphs":[],
                "manager":null,"weapons":[]}}
        }),
    )
    .unwrap();
    encoder.finish().unwrap();
    fs::write(
        source.join("003/000000000001.003.xml"),
        br#"<STUEntityDefinition xmlns:d="https://yretenai.com/dragonml/v1">
            <d:map d:name="m_componentMap">
                <STUTargetTagComponent>
                    <STUTargetTagComponent d:name="dragon:value">
                        <d:array d:name="m_targetTags">
                            <d:ref GUID="000000000001.06C" Target="STUTargetTag"/>
                        </d:array>
                    </STUTargetTagComponent>
                </STUTargetTagComponent>
            </d:map>
        </STUEntityDefinition>"#,
    )
    .unwrap();

    let pack = prepare_with_target_tags(
        &source,
        &dir.path().join("pack"),
        &["0688".into()],
        None,
        Some(&source.join("003")),
    )
    .unwrap();
    let graphs = GraphStore::load(pack.join(graph_bundle)).unwrap();
    assert_eq!(
        graphs.bodies()[&0x0400_0000_0000_0001].target_tags,
        Some(BTreeSet::from([0x0d60_0000_0000_0001]))
    );
}
