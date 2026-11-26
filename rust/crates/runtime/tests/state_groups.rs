//! State-group (subgraph box) serialization from DataTool `.01B` graph XML.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

fn temp_dir(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join("orpheus-runtime");
    fs::create_dir_all(&root).unwrap();
    let dir = root.join(format!("state-groups-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Graph index 90: a subgraph box (item 0) whose transition plug links to
/// the wait state's (item 1) finish plug — the launch edge a tag-only
/// container drops.
const HOOK_EXPORT: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<STUStatescriptGraph dragon:id="0" xmlns:dragon="https://yretenai.com/dragonml/v1">
  <dragon:array dragon:id="1" dragon:name="m_items">
    <STUStatescriptSubGraph dragon:id="2">
      <dragon:array dragon:id="3" dragon:name="m_transitionPlug">
        <STU_EE275B33 dragon:id="4" dragon:name="m_transitionItem">
          <dragon:array dragon:id="5" dragon:name="m_links">
            <STUGraphLink dragon:id="6">
              <STUStatescriptInputPlug dragon:id="7" dragon:name="m_inputPlug" m_links="{null}">
                <STUStatescriptStateWait dragon:id="8" dragon:name="m_parentNode" m_uniqueID="9">
                  <dragon:ref dragon:id="7" dragon:name="m_onFinishedPlug"/>
                </STUStatescriptStateWait>
              </STUStatescriptInputPlug>
            </STUGraphLink>
          </dragon:array>
        </STU_EE275B33>
      </dragon:array>
    </STUStatescriptSubGraph>
    <dragon:ref dragon:id="8"/>
  </dragon:array>
</STUStatescriptGraph>
"#;

fn write_export(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    path
}

fn enrich(graphs: &mut Value, xml: &Path) -> Result<(), String> {
    orpheus_runtime::state_groups_apply(Path::new("bundle"), xml, graphs)
        .map_err(|error| error.to_string())
}

#[test]
fn exported_transition_plug_restores_the_box_begin_edge() {
    let dir = temp_dir("begin-edge");
    write_export(&dir, "00000000005A.01B.xml", HOOK_EXPORT);
    let mut graphs = json!([
        {"index": 90, "containers": {"0": "STUStatescriptSubGraph"}}
    ]);
    enrich(&mut graphs, &dir).unwrap();
    assert_eq!(
        graphs[0]["containers"]["0"],
        json!({"class": "STUStatescriptSubGraph", "links": [[1, "m_onFinishedPlug"]]}),
        "the export's transition plug must surface as the box begin edge"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn enrichment_is_an_eager_round_trip() {
    let dir = temp_dir("round-trip");
    write_export(&dir, "00000000005A.01B.xml", HOOK_EXPORT);
    let mut graphs = json!([
        {"index": 90, "containers": {"0": "STUStatescriptSubGraph"}}
    ]);
    enrich(&mut graphs, &dir).unwrap();
    let once = graphs.clone();
    enrich(&mut graphs, &dir).unwrap();
    assert_eq!(once, graphs, "serializing enriched records is stable");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn missing_or_unrelated_exports_leave_records_unchanged() {
    let dir = temp_dir("absent");
    write_export(&dir, "00000000005B.01B.xml", HOOK_EXPORT);
    let mut graphs = json!([
        {"index": 90, "containers": {"0": "STUStatescriptSubGraph"}},
        {"index": 91}
    ]);
    enrich(&mut graphs, &dir).unwrap();
    assert_eq!(
        graphs[0]["containers"]["0"],
        json!("STUStatescriptSubGraph"),
        "no export means the packer form stands; no guessing"
    );
    assert_eq!(graphs[1].as_object().unwrap().len(), 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn class_mismatch_with_the_export_is_loud() {
    let dir = temp_dir("mismatch");
    write_export(&dir, "00000000005A.01B.xml", HOOK_EXPORT);
    let mut graphs = json!([
        {"index": 90, "containers": {"0": "STUGraphContainer"}}
    ]);
    let error = enrich(&mut graphs, &dir).unwrap_err();
    assert!(
        error.contains("is not the recorded class"),
        "a container the export does not confirm must fail: {error}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Scratch regeneration over the locally exported DataTool graph XML for the
/// hook graph, reading the recorded container map from the pinned pack
/// without touching it: every recorded group must be confirmed by the
/// export (class identity only � no guessing), and the restored edge set is
/// pinned to what the export actually carries. The 104319 export shows zero
/// transition-plug edges for these groups, which is why the pinned pack
/// ships them as bare class tags: the packer drops nothing the export has.
#[test]
#[ignore = "requires the locally exported .01B graph XML (ORPHEUS_EXPORTED_GRAPH_XML) and the pinned pack"]
fn real_hook_export_confirms_the_recorded_state_groups_and_their_edges() {
    use flate2::read::GzDecoder;
    use std::fs::File;
    use std::io::Read;

    let xml_dir = PathBuf::from(
        std::env::var_os("ORPHEUS_EXPORTED_GRAPH_XML").expect("ORPHEUS_EXPORTED_GRAPH_XML"),
    );
    let runtime =
        PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").expect("ORPHEUS_RUNTIME_DIR"));
    let mut packed = Vec::new();
    GzDecoder::new(File::open(runtime.join("data/game_graphs_174.json.gz")).unwrap())
        .read_to_end(&mut packed)
        .unwrap();
    let bundle: Value = serde_json::from_slice(&packed).unwrap();
    let recorded = bundle["graphs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|graph| graph["index"].as_u64() == Some(90))
        .and_then(|graph| graph.get("containers"))
        .cloned()
        .expect("the pinned hook graph records its state groups");
    assert_eq!(recorded.as_object().unwrap().len(), 15);
    let mut graphs = json!([{"index": 90, "containers": recorded.clone()}]);
    enrich(&mut graphs, &xml_dir).unwrap();
    let enriched = graphs[0]["containers"].as_object().unwrap();
    assert_eq!(enriched.len(), 15);
    for (key, class) in recorded.as_object().unwrap() {
        assert_eq!(
            enriched[key]["class"], *class,
            "item {key} must keep the recorded class"
        );
        assert_eq!(
            enriched[key]["links"],
            json!([]),
            "the 104319 export carries no transition-plug edges for item {key}"
        );
    }
}
