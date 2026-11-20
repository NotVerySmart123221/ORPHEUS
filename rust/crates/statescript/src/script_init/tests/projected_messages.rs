use serde::Deserialize;

use crate::script_graph::asset_guid;

use super::*;

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    evidence: String,
    input: Input,
    expected: Markers,
}

#[derive(Deserialize)]
struct Input {
    topic: String,
    abort_message: String,
    sender_graph: u32,
    receiver_graph: u32,
    parameters: Vec<Parameter>,
}

#[derive(Deserialize)]
struct Parameter {
    id: String,
    variable: u32,
}

#[derive(Deserialize)]
struct Markers {
    began: u32,
    ended: u32,
    received: u32,
}

fn contract() -> Contract {
    let fixture: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-projected-message-topics-v1.json"
    )))
    .unwrap();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.evidence, "synthetic-rust");
    fixture
}

fn marker(position: u16, variable: u32) -> Json {
    node(
        position,
        "STUStatescriptActionSetVar",
        None,
        json!({"m_out_Var": var(variable, true), "m_value": true}),
    )
}

fn sender(fixture: &Contract) -> Json {
    let input = &fixture.input;
    let parameters: Vec<_> = input
        .parameters
        .iter()
        .map(|row| {
            json!({
                "m_B5051BCE": row.id,
                "m_value": {"$": "UnreachableProjectionOperand"}
            })
        })
        .collect();
    let mut state = node(
        1,
        "STU_38EE1100",
        Some(0),
        json!({
            "m_gameMessage": {"m_gameMessage": input.topic},
            "m_target": {"$": "STU_9B213168", "m_targets": {"$": "STU_B5A0CAF0"}},
            "m_67FD6A60": 1,
            "m_params": parameters,
            "m_onBeginPlug": plug(&[(2, "m_inPlug")], &[]),
            "m_onEndPlug": plug(&[(3, "m_inPlug")], &[])
        }),
    );
    state["remote"] = json!(0);
    let mut sender = graph(
        input.sender_graph,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug": plug(&[(1, "m_beginPlug")], &[])}),
            ),
            state,
            marker(2, fixture.expected.began),
            marker(3, fixture.expected.ended),
            node(
                4,
                "STUStatescriptEntryGameMessage",
                None,
                json!({
                    "m_gameMessage": {"m_gameMessage": input.abort_message},
                    "m_outPlug": plug(&[(1, ABORT_PLUG)], &[]),
                }),
            ),
        ],
        json!([1]),
        json!([0, 4]),
    );
    sender["remote"] = json!([1]);
    sender
}

fn receiver(fixture: &Contract, index: u32, client_only: bool, stateful: bool, room: bool) -> Json {
    let input = &fixture.input;
    let parameters: Vec<_> = input
        .parameters
        .iter()
        .map(|row| json!({"m_B5051BCE": row.id, "m_out_Var": var(row.variable, true)}))
        .collect();
    let mut receiver = node(
        0,
        if stateful {
            "STUStatescriptStateGameMessageEntry"
        } else {
            "STUStatescriptEntryGameMessage"
        },
        stateful.then_some(0),
        json!({
            "m_gameMessage": {"m_gameMessage": input.topic},
            "m_outPlug": plug(&[(1, "m_inPlug")], &[]),
            "m_params": parameters,
        }),
    );
    receiver["client_only"] = json!(client_only);
    if room {
        receiver["room_message_entry"] = json!(true);
        receiver["server_only"] = json!(true);
    }
    let (states, entries) = if stateful {
        (json!([0]), json!([]))
    } else {
        (json!([]), json!([0]))
    };
    graph(
        index,
        vec![receiver, marker(1, fixture.expected.received)],
        states,
        entries,
    )
}

fn bundle(fixture: &Contract) -> Json {
    json!({
        "version": 1, "bodies": {}, "missing": [],
        "runtime_state_metadata": {"projected_message_topics": [fixture.input.topic]},
        "graphs": [sender(fixture), receiver(fixture, fixture.input.receiver_graph, true, false, false)]
    })
}

fn load(data: &Json) -> std::io::Result<GraphStore> {
    GraphStore::from_json(&serde_json::to_vec(data).unwrap())
}

struct ProjectionWorld;

impl World for ProjectionWorld {
    fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
        None
    }

    fn native(&mut self, _site: Site, config: &Json) -> Option<Value> {
        assert_ne!(
            config["$"], "UnreachableProjectionOperand",
            "server evaluated a projected parameter"
        );
        None
    }

    fn send_message(
        &mut self,
        _site: Site,
        _target: u32,
        _message: u64,
        _parameters: &Variables,
    ) -> bool {
        panic!("a projected sender must not send a server message")
    }

    fn route_game_message(&mut self, _site: Site, _message: u64, _parameters: &Variables) -> bool {
        panic!("a projected sender must not route a server message")
    }
}

fn start(data: &Json, input: &Input) -> Component {
    Component::start(
        Arc::new(load(data).unwrap()),
        834,
        Variables::new(),
        vec![
            Root::new(1, input.sender_graph),
            Root::new(2, input.receiver_graph),
        ],
        0,
        &mut ProjectionWorld,
        MAX_INIT_WORK,
    )
    .unwrap()
}

#[test]
fn projected_self_send_runs_lifecycle_without_server_parameter_evaluation() {
    let fixture = contract();
    let mut component = start(&bundle(&fixture), &fixture.input);
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot.diagnostics.is_empty(),
        "{:?}",
        snapshot.diagnostics
    );
    let sender = &snapshot.instances[&1].states[&0];
    assert!(sender.active);
    assert_eq!(sender.kind, StateKind::ProjectedSelfSend);
    assert_eq!(
        snapshot.variables[&fixture.expected.began],
        Value::Bool(true)
    );

    let parameters = Variables::from([(
        u32::from(asset_guid(&json!(fixture.input.parameters[0].id)).unwrap() as u16),
        Value::Integer(59),
    )]);
    assert_eq!(
        component
            .queue_entity_message(asset_guid(&json!(fixture.input.topic)).unwrap(), parameters)
            .unwrap(),
        0
    );
    assert_eq!(
        component
            .queue_message(
                1,
                asset_guid(&json!(fixture.input.abort_message)).unwrap(),
                Variables::new()
            )
            .unwrap(),
        1
    );
    component.run_now(&mut ProjectionWorld).unwrap();

    let ended = component.snapshot().unwrap();
    assert!(ended.diagnostics.is_empty(), "{:?}", ended.diagnostics);
    assert!(!ended.instances[&1].states[&0].active);
    assert_eq!(ended.variables[&fixture.expected.ended], Value::Bool(true));
    assert!(!ended.variables.contains_key(&fixture.expected.received));
    for row in &fixture.input.parameters {
        assert!(!ended.variables.contains_key(&row.variable));
    }
}

#[test]
fn projected_self_send_rejects_undeclared_or_malformed_senders() {
    let fixture = contract();
    let mut undeclared = bundle(&fixture);
    undeclared
        .as_object_mut()
        .unwrap()
        .remove("runtime_state_metadata");
    let mut malformed = bundle(&fixture);
    malformed["graphs"][0]["nodes"][1]["fields"]["m_params"][0]
        .as_object_mut()
        .unwrap()
        .remove("m_value");
    for data in [undeclared, malformed] {
        let component = start(&data, &fixture.input);
        let snapshot = component.snapshot().unwrap();
        assert!(
            matches!(
                snapshot.diagnostics.as_slice(),
                [Diagnostic {
                    issue: Issue::UnsupportedMessageRegistration,
                    ..
                }]
            ),
            "{:?}",
            snapshot.diagnostics
        );
    }
}

#[test]
fn projected_message_topics_require_client_evidence_and_disallow_server_receivers() {
    let fixture = contract();
    let rejects = |data: &Json, reason| {
        let error = load(data).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains(reason), "{error}");
    };
    let mut absent_client = bundle(&fixture);
    absent_client["graphs"].as_array_mut().unwrap().pop();
    rejects(&absent_client, "has no client receiver");

    let mut dynamic_client = bundle(&fixture);
    dynamic_client["graphs"][1]["nodes"][0]["fields"]["m_gameMessage"]["$"] =
        json!("STUConfigVarDynamic");
    rejects(&dynamic_client, "has no client receiver");

    let mut duplicate_topic = bundle(&fixture);
    duplicate_topic["runtime_state_metadata"]["projected_message_topics"] =
        json!([fixture.input.topic, fixture.input.topic]);
    rejects(&duplicate_topic, "duplicate projected message topic");

    for (stateful, room) in [(false, false), (true, false), (false, true)] {
        let mut competing = bundle(&fixture);
        competing["graphs"].as_array_mut().unwrap().push(receiver(
            &fixture,
            fixture.input.receiver_graph + 1,
            false,
            stateful,
            room,
        ));
        rejects(&competing, "has a server receiver");
    }

    let mut annotated_client = bundle(&fixture);
    annotated_client["graphs"][1] =
        receiver(&fixture, fixture.input.receiver_graph, false, true, false);
    annotated_client["runtime_state_metadata"]["client_states"] = json!([{
        "graph": fixture.input.receiver_graph,
        "state": 0, "rid": 0, "class": "STUStatescriptStateGameMessageEntry"
    }]);
    assert!(load(&annotated_client).is_ok());
}

#[test]
fn projected_topic_rejects_an_ordinary_server_send() {
    let fixture = contract();
    let mut data = bundle(&fixture);
    let mut action = node(
        2,
        "STU_57692020",
        None,
        json!({"m_gameMessage":{"m_gameMessage":fixture.input.topic}}),
    );
    action["fields"]["m_ADEB6E05"] = json!(true);
    data["graphs"][0]["nodes"][2] = action;

    let component = start(&data, &fixture.input);
    let snapshot = component.snapshot().unwrap();
    assert!(matches!(
        snapshot.diagnostics.as_slice(),
        [Diagnostic {
            site: Site { node: Some(2), .. },
            issue: Issue::UnsupportedMessageRegistration,
            ..
        }]
    ));
    assert_eq!(
        snapshot.instances[&1].states[&0].kind,
        StateKind::ProjectedSelfSend
    );
}
