use serde::Deserialize;

use super::*;

const FRAME_WAIT: &str = "STU_E3CEF6B9";
const MARKER: u32 = 947;

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    evidence: String,
    input: Input,
}

#[derive(Deserialize)]
struct Input {
    start_frame: u32,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    graph: u32,
    node: u16,
    server_only: bool,
    #[serde(default)]
    remote_bound: bool,
    fields: serde_json::Map<String, Json>,
    finishes_at: Option<u32>,
}

fn graph_for(case: &Case) -> Json {
    let marker = 3 - case.node;
    let mut fields = json!({"m_onFinishedPlug": plug(&[(marker, "m_inPlug")], &[])});
    fields.as_object_mut().unwrap().extend(case.fields.clone());
    let mut wait = node(case.node, FRAME_WAIT, Some(0), fields);
    wait["server_only"] = json!(case.server_only);
    if case.remote_bound {
        wait["remote"] = json!(0);
    }
    let mut fixture = graph(
        case.graph,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug": plug(&[(case.node, "m_beginPlug")], &[])}),
            ),
            wait,
            node(
                marker,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var": var(MARKER, true), "m_value": true}),
            ),
        ],
        json!([case.node]),
        json!([0]),
    );
    if case.remote_bound {
        fixture["remote"] = json!([case.node]);
    }
    fixture
}

#[test]
fn shared_server_frame_wait_checks_shape_and_finishes_on_the_vm_clock() {
    let contract: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-frame-wait-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.evidence, "synthetic-rust");

    let start = contract.input.start_frame;
    for case in &contract.input.cases {
        let mut component = Component::start(
            Arc::new(store(vec![graph_for(case)])),
            841,
            Variables::new(),
            vec![Root::new(1, case.graph)],
            start,
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap();

        if let Some(target) = case.finishes_at {
            for frame in start..=target {
                if frame != start {
                    component.advance_frame(frame, &mut EmptyWorld).unwrap();
                }
                let snapshot = component.snapshot().unwrap();
                let state = &snapshot.instances[&1].states[&0];
                assert!(
                    snapshot.diagnostics.is_empty(),
                    "{} at frame {frame}: {:?}",
                    case.name,
                    snapshot.diagnostics
                );
                assert_eq!(
                    state.kind,
                    StateKind::WaitFrames { target },
                    "{}",
                    case.name
                );
                assert_eq!(
                    state.active,
                    frame < target,
                    "{} at frame {frame}",
                    case.name
                );
                assert_eq!(
                    snapshot.variables.get(&MARKER),
                    (frame == target).then_some(&Value::Bool(true)),
                    "{} at frame {frame}",
                    case.name
                );
            }
        } else {
            let snapshot = component.snapshot().unwrap();
            assert!(
                matches!(
                    snapshot.diagnostics.as_slice(),
                    [Diagnostic { issue: Issue::UnsupportedState(class), .. }]
                    if class == FRAME_WAIT
                ),
                "{}: {:?}",
                case.name,
                snapshot.diagnostics
            );
            assert!(
                !snapshot.instances[&1].states.contains_key(&0),
                "{}",
                case.name
            );
            assert!(!snapshot.variables.contains_key(&MARKER), "{}", case.name);
        }
    }
}
