use super::*;
use crate::script_init::machine::Event;

fn contract() -> Json {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-parent-owned-state-v1.json"
    )))
    .unwrap()
}

fn fixture(graph_index: u32, position: u16, fields: &Json, transition: bool) -> Component {
    let contract = contract();
    let mut fields = fields.clone();
    fields["m_subgraphPlug"] = plug(&[(2, "m_beginPlug")], &[1]);
    fields["m_onBeginPlug"] = plug(&[(7, "m_inPlug")], &[]);
    fields["m_onEndPlug"] = plug(&[(8, "m_inPlug")], &[]);
    fields["m_onAbortPlug"] = plug(&[(9, "m_inPlug")], &[]);
    fields["m_onFinishedPlug"] = plug(&[(10, "m_inPlug")], &[]);
    if transition {
        fields["m_transitionPlug"] = json!([plug(&[(11, "m_inPlug")], &[])]);
    }
    let marker = |pos, variable| {
        node(
            pos,
            "STUStatescriptActionSetVar",
            None,
            json!({"m_out_Var": var(variable, true), "m_value": true}),
        )
    };
    let nodes = vec![
        node(
            0,
            "STUStatescriptEntry",
            None,
            json!({"m_outPlug": plug(&[(1, "m_beginPlug")], &[])}),
        ),
        node(
            1,
            "STUStatescriptState",
            Some(0),
            json!({"m_subgraphPlug": plug(&[(position, "m_beginPlug")], &[2])}),
        ),
        node(
            2,
            "STUStatescriptStateWait",
            Some(1),
            json!({
                "m_timeout": {"$": "STUConfigVarFloat", "m_value": contract["input"]["child_wait_seconds"]},
                "m_onFinishedPlug": plug(&[(12, "m_inPlug")], &[])
            }),
        ),
        node(position, "STU_54FEB21C", Some(2), fields),
        marker(7, 901),
        marker(8, 902),
        marker(9, 903),
        marker(10, 904),
        node(
            11,
            "STU_4E2BCAAB",
            None,
            json!({"m_AA6E611E": {"$": "STUConfigVarFloat", "m_value": 0.0}}),
        ),
        marker(12, 905),
    ];
    let mut data = graph(graph_index, nodes, json!([1, 2, position]), json!([0]));
    data["items"] = json!(position.max(12) + 1);
    Component::start(
        Arc::new(store(vec![data])),
        71,
        Variables::new(),
        vec![Root::new(1, graph_index)],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap()
}

fn assert_cleaned(component: &mut Component, finished: bool) {
    let snapshot = component.snapshot().unwrap();
    assert!(snapshot.initialization_complete());
    assert!(!snapshot.instances[&1].states[&2].active);
    assert!(!snapshot.instances[&1].states[&1].active);
    assert_eq!(snapshot.variables.get(&902), Some(&Value::Bool(true)));
    assert_eq!(snapshot.variables.contains_key(&903), !finished);
    assert_eq!(snapshot.variables.contains_key(&904), finished);
    assert!(!snapshot.variables.contains_key(&905));
    assert!(snapshot.pending_timers.get(&1).is_none_or(Vec::is_empty));
    let late_frame = contract()["input"]["late_frame"].as_u64().unwrap() as u32;
    component
        .advance_frame(late_frame, &mut EmptyWorld)
        .unwrap();
    assert!(!component.snapshot().unwrap().variables.contains_key(&905));
}

#[test]
fn parent_owned_state_runs_at_arbitrary_placements_and_reclaims_children() {
    let contract = contract();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(
        contract["case_id"],
        "script-parent-owned-state.synthetic.v1"
    );
    assert_eq!(contract["evidence"], "synthetic-rust");
    for placement in contract["input"]["placements"].as_array().unwrap() {
        let graph = placement[0].as_u64().unwrap() as u32;
        let position = placement[1].as_u64().unwrap() as u16;
        for fields in contract["input"]["empty_fields"].as_array().unwrap() {
            for (raw, finished) in [(0, false), (2, true), (2, false)] {
                let mut component = fixture(graph, position, fields, false);
                let snapshot = component.snapshot().unwrap();
                assert!(
                    snapshot.initialization_complete(),
                    "{:?}",
                    snapshot.diagnostics
                );
                assert_eq!(snapshot.instances[&1].states[&2].kind, StateKind::Plain);
                assert!(snapshot.instances[&1].states[&2].active);
                assert!(snapshot.instances[&1].states[&1].active);
                assert_eq!(snapshot.variables.get(&901), Some(&Value::Bool(true)));
                assert_eq!(snapshot.pending_timers[&1].len(), 1);
                component.advance_frame(1, &mut EmptyWorld).unwrap();
                let event = if finished {
                    Event::Finish(raw)
                } else {
                    Event::Abort(raw)
                };
                let now_ms = component.now_ms();
                component.machine.queues.push(1, now_ms, event).unwrap();
                component.run_now(&mut EmptyWorld).unwrap();
                assert_cleaned(&mut component, finished);
            }
        }
    }
}

#[test]
fn parent_owned_state_transition_keeps_base_abort_and_cleanup() {
    let mut component = fixture(211, 3, &json!({}), true);
    assert_cleaned(&mut component, false);
}

#[test]
fn parent_owned_state_retains_the_historical_duration_compatibility() {
    let mut component = fixture(
        0x01f0,
        14,
        &json!({"m_ED97A97C": contract()["input"]["child_wait_seconds"]}),
        false,
    );
    assert!(component.snapshot().unwrap().initialization_complete());
    component.advance_frame(20, &mut EmptyWorld).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert!(snapshot.instances[&1].states[&2].active);
    assert!(!snapshot.variables.contains_key(&902));
    assert_eq!(snapshot.variables.get(&905), Some(&Value::Bool(true)));
}

#[test]
fn parent_owned_state_rejects_configured_predicates_duration_and_alternate_graph() {
    let contract = contract();
    for fields in contract["input"]["rejected_fields"].as_array().unwrap() {
        let component = fixture(173, 6, fields, false);
        let snapshot = component.snapshot().unwrap();
        assert!(!snapshot.initialization_complete(), "{fields:?}");
        assert_eq!(snapshot.diagnostics.len(), 1, "{fields:?}");
        assert_eq!(
            snapshot.diagnostics[0].issue,
            Issue::UnsupportedState("STU_54FEB21C".into())
        );
        assert!(!snapshot.variables.contains_key(&901));
        assert!(snapshot.pending_timers.get(&1).is_none_or(Vec::is_empty));
    }
}
