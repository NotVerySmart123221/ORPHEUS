use serde::Deserialize;

use super::*;

const ENTITY: u32 = 907;
const ROOT: u16 = 1;
const STATE: u16 = 0;
const STATE_POS: u16 = 1;
const BEGIN_MESSAGE: u64 = 31;
const ABORT_MESSAGE: u64 = 32;
const BEGAN: u32 = 301;
const ABORTED: u32 = 302;
const ENDED: u32 = 303;

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    case_id: String,
    evidence: String,
    input: Input,
    expected: Expected,
}

#[derive(Deserialize)]
struct Input {
    class: String,
    runtime_state_metadata: Json,
    lifecycle: Vec<Lifecycle>,
    handlers: Vec<Handler>,
}

#[derive(Deserialize)]
struct Lifecycle {
    graph: u32,
    #[serde(default)]
    server_only: bool,
    ending: Ending,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Ending {
    Wait,
    Message,
    Client,
}

#[derive(Deserialize)]
struct Handler {
    graph: u32,
    class: String,
    case: HandlerCase,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum HandlerCase {
    Wait,
    BlockedWait,
    BlockedPlacement,
}

#[derive(Deserialize)]
struct Expected {
    active_before_end: Vec<u32>,
    owner_state_graphs: Vec<u32>,
    remote_state_graphs: Vec<u32>,
}

fn contract() -> Contract {
    let fixture: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-runtime-state-metadata-v1.json"
    )))
    .unwrap();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(
        fixture.case_id,
        "script-runtime-state-metadata.synthetic.v1"
    );
    assert_eq!(fixture.evidence, "synthetic-rust");
    fixture
}

fn message_node(pos: u16, message: u64, input: &str) -> Json {
    node(
        pos,
        "STUStatescriptEntryGameMessage",
        None,
        json!({
            "m_gameMessage": {"m_gameMessage": format!("0x{message:016X}")},
            "m_outPlug": plug(&[(STATE_POS, input)], &[]),
        }),
    )
}

fn set_true(pos: u16, variable: u32) -> Json {
    node(
        pos,
        "STUStatescriptActionSetVar",
        None,
        json!({"m_out_Var": var(variable, false), "m_value": true}),
    )
}

fn timeout_fields() -> Json {
    json!({"m_timeout": {"$": "STUConfigVarFloat", "m_value": 0.05}})
}

fn lifecycle_graph(class: &str, case: &Lifecycle) -> Json {
    let mut on_begin = vec![(2, "m_inPlug")];
    if matches!(case.ending, Ending::Wait) {
        on_begin.push((6, "m_beginPlug"));
    }
    let mut state = node(
        STATE_POS,
        class,
        Some(STATE),
        json!({
            "m_onBeginPlug": plug(&on_begin, &[]),
            "m_onAbortPlug": plug(&[(3, "m_inPlug")], &[]),
            "m_onEndPlug": plug(&[(4, "m_inPlug")], &[]),
        }),
    );
    state["rid"] = json!(9);
    state["remote"] = json!(0);
    state["server_only"] = json!(case.server_only);
    let mut nodes = vec![
        message_node(0, BEGIN_MESSAGE, "m_beginPlug"),
        state,
        set_true(2, BEGAN),
        set_true(3, ABORTED),
        set_true(4, ENDED),
        message_node(5, ABORT_MESSAGE, ABORT_PLUG),
    ];
    let mut state_order = vec![STATE_POS];
    if matches!(case.ending, Ending::Wait) {
        let mut fields = timeout_fields();
        fields["m_onFinishedPlug"] = plug(&[(STATE_POS, ABORT_PLUG)], &[]);
        nodes.push(node(6, "STUStatescriptStateWait", Some(1), fields));
        state_order.push(6);
    }
    let mut graph = graph(case.graph, nodes, json!(state_order), json!([0, 5]));
    graph["remote"] = json!([STATE_POS]);
    graph
}

fn handler_graph(case: &Handler) -> Json {
    let mut fields = timeout_fields();
    if matches!(case.case, HandlerCase::BlockedWait) {
        fields["m_BF1A93B0"] = json!(true);
    }
    graph(
        case.graph,
        vec![
            message_node(0, BEGIN_MESSAGE, "m_beginPlug"),
            node(STATE_POS, &case.class, Some(STATE), fields),
        ],
        json!([STATE_POS]),
        json!([0]),
    )
}

fn graph_store(input: &Input, with_metadata: bool) -> Arc<GraphStore> {
    let graphs: Vec<_> = input
        .lifecycle
        .iter()
        .map(|case| lifecycle_graph(&input.class, case))
        .chain(input.handlers.iter().map(handler_graph))
        .collect();
    let mut bundle = json!({"version":1,"graphs":graphs,"bodies":{},"missing":[]});
    if with_metadata {
        bundle["runtime_state_metadata"] = input.runtime_state_metadata.clone();
    }
    Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap())
}

fn start(graphs: &Arc<GraphStore>, graph: u32, init_state: Option<u16>) -> Component {
    Component::start(
        Arc::clone(graphs),
        ENTITY,
        Variables::new(),
        vec![Root::listener(ROOT, graph, init_state)],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap()
}

fn send(component: &mut Component, message: u64) {
    assert_eq!(
        component
            .queue_message(ROOT, message, Variables::new())
            .unwrap(),
        1
    );
    component.run_now(&mut EmptyWorld).unwrap();
}

#[test]
fn runtime_class_metadata_preserves_wire_shape_and_runs_plain_lifecycle() {
    let Contract {
        input, expected, ..
    } = contract();
    let baseline = graph_store(&input, false);
    let graphs = graph_store(&input, true);

    for case in input.lifecycle {
        let before = baseline.graph(case.graph).unwrap();
        let after = graphs.graph(case.graph).unwrap();
        let old_node = before.state(usize::from(STATE)).unwrap();
        let node = after.state(usize::from(STATE)).unwrap();
        assert_eq!(node.class, input.class);
        assert_eq!(node.class, old_node.class);
        assert_eq!(
            (node.pos, node.uid, node.rid, node.state, node.remote),
            (
                old_node.pos,
                old_node.uid,
                old_node.rid,
                old_node.state,
                old_node.remote
            )
        );
        assert_eq!(
            (node.client_only, node.server_only),
            (false, case.server_only)
        );
        assert_eq!(node.fields, old_node.fields);
        assert_eq!(after.owner_positions(), before.owner_positions());
        assert_eq!(after.remote_positions(), before.remote_positions());
        assert_eq!(
            after.owner_positions().contains(&STATE_POS),
            expected.owner_state_graphs.contains(&case.graph)
        );
        assert_eq!(
            after.remote_positions().contains(&STATE_POS),
            expected.remote_state_graphs.contains(&case.graph)
        );

        let mut unannotated = start(&baseline, case.graph, None);
        send(&mut unannotated, BEGIN_MESSAGE);
        assert!(matches!(
            unannotated.snapshot().unwrap().diagnostics.as_slice(),
            [Diagnostic { issue: Issue::UnsupportedState(class), .. }] if *class == input.class
        ));

        let direct_start = matches!(case.ending, Ending::Client).then_some(STATE);
        let mut component = start(&graphs, case.graph, direct_start);
        send(&mut component, BEGIN_MESSAGE);
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.diagnostics.is_empty(),
            "{:?}",
            snapshot.diagnostics
        );
        let instance = &snapshot.instances[&ROOT];
        let started = expected.active_before_end.contains(&case.graph);
        if !started {
            assert!(!instance.states.contains_key(&STATE));
            assert!(!instance.variables.contains_key(&BEGAN));
            assert!(!instance.variables.contains_key(&ENDED));
            continue;
        }
        assert_eq!(instance.states[&STATE].kind, StateKind::Plain);
        assert_eq!(instance.states[&STATE].activation, 1);
        assert!(instance.states[&STATE].active);
        assert_eq!(instance.variables.get(&BEGAN), Some(&Value::Bool(true)));
        assert!(!instance.variables.contains_key(&ENDED));
        component.advance_frame(1, &mut EmptyWorld).unwrap();
        assert!(component.snapshot().unwrap().instances[&ROOT].states[&STATE].active);
        match case.ending {
            Ending::Wait => component.advance_frame(5, &mut EmptyWorld).unwrap(),
            Ending::Message => send(&mut component, ABORT_MESSAGE),
            Ending::Client => unreachable!(),
        }
        let instance = &component.snapshot().unwrap().instances[&ROOT];
        assert!(!instance.states[&STATE].active);
        assert_eq!(instance.states[&STATE].activation, 1);
        assert_eq!(instance.variables.get(&ABORTED), Some(&Value::Bool(true)));
        assert_eq!(instance.variables.get(&ENDED), Some(&Value::Bool(true)));
    }
}

#[test]
fn runtime_class_metadata_keeps_existing_state_handlers_and_rejections() {
    let Contract { input, .. } = contract();
    let baseline = graph_store(&input, false);
    let graphs = graph_store(&input, true);
    for case in input.handlers {
        let mut before = start(&baseline, case.graph, None);
        let mut after = start(&graphs, case.graph, None);
        send(&mut before, BEGIN_MESSAGE);
        send(&mut after, BEGIN_MESSAGE);
        let baseline_snapshot = before.snapshot().unwrap();
        let snapshot = after.snapshot().unwrap();
        assert_eq!(snapshot.diagnostics, baseline_snapshot.diagnostics);
        let state = snapshot.instances[&ROOT].states.get(&STATE);
        match case.case {
            HandlerCase::Wait => {
                assert!(snapshot.diagnostics.is_empty());
                assert_eq!(state.unwrap().kind, StateKind::Wait);
                assert_eq!(snapshot.pending_timers, baseline_snapshot.pending_timers);
                assert!(!snapshot.pending_timers[&ROOT].is_empty());
            }
            HandlerCase::BlockedWait | HandlerCase::BlockedPlacement => {
                assert!(state.is_none());
                assert!(matches!(
                    snapshot.diagnostics.as_slice(),
                    [Diagnostic { issue: Issue::UnsupportedState(class), .. }] if *class == case.class
                ));
            }
        }
    }
}

#[derive(Deserialize)]
struct FactoryContract {
    schema_version: u8,
    evidence: String,
    input: FactoryInput,
}

#[derive(Deserialize)]
struct FactoryInput {
    fixture_metadata: Json,
    cases: Vec<FactoryCase>,
}

#[derive(Deserialize)]
struct FactoryCase {
    graph: u32,
    class: String,
    #[serde(default)]
    selector: Json,
    #[serde(default)]
    client_only: bool,
    expected: FactoryOutcome,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum FactoryOutcome {
    Plain,
    ClientSkipped,
    Unsupported,
}

#[test]
fn inherited_base_factory_preserves_lifecycle_across_graph_placements() {
    let fixture: FactoryContract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-inherited-base-factory-v1.json"
    )))
    .unwrap();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.evidence, "synthetic-rust");

    let input = fixture.input;
    let graphs: Vec<_> = input
        .cases
        .iter()
        .map(|case| {
            let mut graph = lifecycle_graph(
                &case.class,
                &Lifecycle {
                    graph: case.graph,
                    server_only: false,
                    ending: Ending::Message,
                },
            );
            graph["remote"] = json!([]);
            let state = &mut graph["nodes"][usize::from(STATE_POS)];
            state["remote"] = json!(-1);
            state["client_only"] = json!(case.client_only);
            state["fields"]["m_64C3E07E"] = case.selector.clone();
            graph
        })
        .collect();

    for with_metadata in [false, true] {
        let mut bundle = json!({"version":1,"graphs":graphs,"bodies":{},"missing":[]});
        if with_metadata {
            bundle["runtime_state_metadata"] = input.fixture_metadata.clone();
        }
        let graphs =
            Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap());
        for case in &input.cases {
            let context = format!("graph {}, metadata {with_metadata}", case.graph);
            let mut component = start(&graphs, case.graph, None);
            send(&mut component, BEGIN_MESSAGE);
            let snapshot = component.snapshot().unwrap();
            let instance = &snapshot.instances[&ROOT];
            match case.expected {
                FactoryOutcome::Plain => {
                    assert!(
                        snapshot.diagnostics.is_empty(),
                        "{context}: {:?}",
                        snapshot.diagnostics
                    );
                    assert!(instance.states[&STATE].active, "{context}");
                    assert_eq!(instance.states[&STATE].kind, StateKind::Plain, "{context}");
                    assert_eq!(
                        instance.variables.get(&BEGAN),
                        Some(&Value::Bool(true)),
                        "{context}"
                    );
                    assert!(!instance.variables.contains_key(&ENDED), "{context}");

                    send(&mut component, ABORT_MESSAGE);
                    let ended = component.snapshot().unwrap();
                    let instance = &ended.instances[&ROOT];
                    assert!(!instance.states[&STATE].active, "{context}");
                    for marker in [ABORTED, ENDED] {
                        assert_eq!(
                            instance.variables.get(&marker),
                            Some(&Value::Bool(true)),
                            "{context}"
                        );
                    }
                }
                FactoryOutcome::ClientSkipped => {
                    assert!(snapshot.diagnostics.is_empty(), "{context}");
                    assert!(instance.states.is_empty(), "{context}");
                    assert!(instance.variables.is_empty(), "{context}");
                }
                FactoryOutcome::Unsupported => {
                    assert!(
                        matches!(
                            snapshot.diagnostics.as_slice(),
                            [Diagnostic { issue: Issue::UnsupportedState(class), .. }]
                            if class == &case.class
                        ),
                        "{context}: {:?}",
                        snapshot.diagnostics
                    );
                    assert!(instance.states.is_empty(), "{context}");
                    assert!(instance.variables.is_empty(), "{context}");
                }
            }
        }
    }
}
