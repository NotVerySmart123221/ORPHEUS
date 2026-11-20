use super::*;

const ENTER: u64 = 0x0240_0000_0000_8231;

fn lifecycle_fixture() -> Json {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-logical-input-owner-lifecycle-v1.json"
    )))
    .unwrap()
}

#[test]
fn logical_button_buffer_keeps_its_original_input_owner_after_rebinding() {
    const BINDING: u32 = 71;
    const PRESSED: u32 = 72;
    let fixture = lifecycle_fixture();
    let input = &fixture["input"]["buffered_input"];
    let graph_index = u32::try_from(input["graph"].as_u64().unwrap()).unwrap();
    let body = u32::try_from(input["body"].as_u64().unwrap()).unwrap();
    let button = input["button"].as_i64().unwrap();
    let new_button = input["new_button"].as_i64().unwrap();
    let command_mask = u32::try_from(input["command_mask"].as_u64().unwrap()).unwrap();
    let buffer_seconds = input["buffer_ms"].as_f64().unwrap() / 1000.0;
    let mut listener = node(
        1,
        "STUStatescriptStateLogicalButton",
        Some(0),
        json!({
            "m_logicalButton":var(BINDING,true),
            "m_246438AD":{"$":"STUConfigVarFloat","m_value":buffer_seconds},
            "m_onGoingDownPlug":plug(&[(3,"m_inPlug")],&[]),
            "m_80AF45FB":plug(&[(2,"m_beginPlug")],&[1])
        }),
    );
    listener["server_only"] = json!(true);
    let mut scale = node(2, "AuthoredBufferedScale", Some(1), json!({"factor":0.5}));
    scale["state_behavior"] = json!("movement_scale");
    scale["server_only"] = json!(true);
    let mut model = graph(
        graph_index,
        vec![
            node(
                0,
                "STUStatescriptEntryGameMessage",
                None,
                json!({
                    "m_gameMessage":{"m_gameMessage":format!("0x{ENTER:016X}")},
                    "m_outPlug":plug(&[(1,"m_beginPlug")],&[])
                }),
            ),
            listener,
            scale,
            node(
                3,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var":var(PRESSED,true),"m_value":true}),
            ),
        ],
        json!([1, 2]),
        json!([0]),
    );
    model["fields"]["m_680A2CB2"] = json!([0]);
    let graphs = Arc::new(store(vec![model]));
    for (binding, expected) in [(button, true), (new_button, false)] {
        let mut component = Component::start(
            Arc::clone(&graphs),
            body,
            Variables::from([(BINDING, Value::Integer(button))]),
            vec![Root::new(1, graph_index)],
            0,
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap();
        component
            .advance_command(
                Command {
                    frame: 1,
                    buttons: command_mask,
                    ..Command::default()
                },
                &mut EmptyWorld,
            )
            .unwrap();
        component
            .advance_command(
                Command {
                    frame: 2,
                    ..Command::default()
                },
                &mut EmptyWorld,
            )
            .unwrap();
        assert!(component.machine.held_buttons.is_empty());
        component
            .set_entity_variable(BINDING, Value::Integer(binding), &mut EmptyWorld)
            .unwrap();
        assert_eq!(
            component.queue_message(1, ENTER, Variables::new()).unwrap(),
            1
        );
        component.run_now(&mut EmptyWorld).unwrap();
        let snapshot = component.snapshot().unwrap();
        assert_eq!(
            snapshot.instances[&1].states[&0].kind,
            StateKind::LogicalButton { down: expected }
        );
        assert_eq!(
            snapshot.variables.get(&PRESSED),
            expected.then_some(&Value::Bool(true)),
            "binding {binding}"
        );
        assert_eq!(
            snapshot.instances[&1]
                .states
                .get(&1)
                .is_some_and(|state| state.active),
            expected
        );
        assert_eq!(
            component
                .movement_modifiers(&mut EmptyWorld)
                .unwrap()
                .unwrap()
                .len(),
            usize::from(expected)
        );
        component.advance_frame(3, &mut EmptyWorld).unwrap();
        assert_eq!(
            component.snapshot().unwrap().instances[&1].states[&0].kind,
            StateKind::LogicalButton { down: false }
        );
        assert!(
            component
                .movement_modifiers(&mut EmptyWorld)
                .unwrap()
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn authored_held_input_registers_without_legacy_graph_metadata_and_releases_owned_states() {
    const EDGE: u32 = 73;
    let fixture = lifecycle_fixture();
    let input = &fixture["input"]["authored_input"];
    let graph_index = u32::try_from(input["graph"].as_u64().unwrap()).unwrap();
    let body = u32::try_from(input["body"].as_u64().unwrap()).unwrap();
    let command_mask = u32::try_from(input["command_mask"].as_u64().unwrap()).unwrap();
    let mut listener = node(
        1,
        "AuthoredHeldInput",
        Some(0),
        json!({
            "button":input["button"],
            "held_states":[1,3],
            "idle_states":[2],
            "on_press":plug(&[(4,"m_inPlug")],&[]),
            "on_release":plug(&[(5,"m_inPlug")],&[])
        }),
    );
    listener["state_behavior"] = json!("held_input");
    listener["server_only"] = json!(true);
    let mut scale = node(
        2,
        "AuthoredHeldScale",
        Some(1),
        json!({"factor":input["held_scale"]}),
    );
    scale["state_behavior"] = json!("movement_scale");
    scale["server_only"] = json!(true);
    let mut idle = node(
        3,
        "AuthoredIdle",
        Some(2),
        json!({"factor":input["idle_scale"]}),
    );
    idle["state_behavior"] = json!("movement_scale");
    idle["server_only"] = json!(true);
    let mut restart_peer = node(
        6,
        "AuthoredHeldEnd",
        Some(3),
        json!({"m_onEndPlug":plug(&[(2,"m_beginPlug")],&[])}),
    );
    restart_peer["state_behavior"] = json!("plain");
    restart_peer["server_only"] = json!(true);
    let setter = |position, value| {
        node(
            position,
            "STUStatescriptActionSetVar",
            None,
            json!({"m_out_Var":var(EDGE,true),"m_value":value}),
        )
    };
    let model = graph(
        graph_index,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_beginPlug")],&[])}),
            ),
            listener,
            scale,
            idle,
            setter(4, true),
            setter(5, false),
            restart_peer,
        ],
        json!([1, 2, 3, 6]),
        json!([0]),
    );
    assert!(model["fields"].get("m_680A2CB2").is_none());
    let graphs = Arc::new(store(vec![model]));
    let mut component = Component::start(
        graphs,
        body,
        Variables::new(),
        vec![Root::new(1, graph_index)],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    let initial = &component.snapshot().unwrap().instances[&1].states;
    assert!(initial[&0].active && initial[&2].active);
    assert!(!initial.get(&1).is_some_and(|state| state.active));
    component
        .advance_command(
            Command {
                frame: 1,
                buttons: command_mask,
                ..Command::default()
            },
            &mut EmptyWorld,
        )
        .unwrap();
    let snapshot = component.snapshot().unwrap();
    assert_eq!(snapshot.variables[&EDGE], Value::Bool(true));
    assert!(snapshot.instances[&1].states[&1].active);
    assert!(!snapshot.instances[&1].states[&2].active);
    assert!(snapshot.instances[&1].states[&3].active);
    let held_activation = snapshot.instances[&1].states[&1].activation;
    assert_eq!(
        component
            .movement_modifiers(&mut EmptyWorld)
            .unwrap()
            .unwrap()
            .len(),
        1
    );
    component
        .advance_command(
            Command {
                frame: 2,
                buttons: command_mask,
                ..Command::default()
            },
            &mut EmptyWorld,
        )
        .unwrap();
    assert_eq!(
        component.snapshot().unwrap().instances[&1].states[&1].activation,
        held_activation
    );
    component
        .advance_command(
            Command {
                frame: 3,
                ..Command::default()
            },
            &mut EmptyWorld,
        )
        .unwrap();
    let snapshot = component.snapshot().unwrap();
    assert_eq!(snapshot.variables[&EDGE], Value::Bool(false));
    assert!(!snapshot.instances[&1].states[&1].active);
    assert!(snapshot.instances[&1].states[&2].active);
    assert!(!snapshot.instances[&1].states[&3].active);
    assert_eq!(
        component
            .movement_modifiers(&mut EmptyWorld)
            .unwrap()
            .unwrap()
            .len(),
        1
    );

    component
        .advance_command(
            Command {
                frame: 4,
                buttons: command_mask,
                ..Command::default()
            },
            &mut EmptyWorld,
        )
        .unwrap();
    assert!(component.snapshot().unwrap().instances[&1].states[&1].active);
    component.debug_end_state(1, 0, &mut EmptyWorld).unwrap();
    let states = &component.snapshot().unwrap().instances[&1].states;
    assert!(!states[&0].active && !states[&1].active && !states[&2].active);
    assert!(!states[&3].active);
    assert!(
        component
            .movement_modifiers(&mut EmptyWorld)
            .unwrap()
            .unwrap()
            .is_empty()
    );
    component.teardown(&mut EmptyWorld).unwrap();
    assert!(component.snapshot().unwrap().instances.is_empty());
}

#[test]
fn logical_hold_applies_death_admission_to_edges_entry_and_lock_resync() {
    let fixture: Json = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-logical-input-owner-lifecycle-v1.json"
    )))
    .unwrap();
    assert_eq!(fixture["schema_version"], 1);
    assert_eq!(
        fixture["case_id"],
        "script-logical-input-owner-lifecycle.synthetic.v1"
    );
    let input = &fixture["input"];
    let number = |name| u32::try_from(input[name].as_u64().unwrap()).unwrap();
    let body = number("body");
    let graph_index = number("graph");
    let button = number("button");
    let held = Command {
        frame: 1,
        buttons: number("command_mask"),
        ..Command::default()
    };
    let enter = |component: &mut Component| {
        assert_eq!(
            component.queue_message(1, ENTER, Variables::new()).unwrap(),
            1
        );
        component.run_now(&mut EmptyWorld).unwrap();
    };
    let check = |component: &mut Component, expected: bool| {
        let snapshot = component.snapshot().unwrap();
        assert!(snapshot.initialization_complete());
        let states = &snapshot.instances[&1].states;
        assert_eq!(states[&0].kind, StateKind::LogicalButton { down: expected });
        assert_eq!(states.get(&1).is_some_and(|state| state.active), expected);
        assert_eq!(
            component
                .movement_modifiers(&mut EmptyWorld)
                .unwrap()
                .unwrap()
                .len(),
            usize::from(expected)
        );
    };

    for (field, allow_dead) in [
        ("default_dead_listener", false),
        ("allow_dead_listener", true),
    ] {
        let expected = fixture["expected"][field].as_bool().unwrap();
        let mut listener = node(
            1,
            "STUStatescriptStateLogicalButton",
            Some(0),
            json!({
                "m_logicalButton":{"$":"STUConfigVarLogicalButton","m_logicalButton":button},
                "m_80AF45FB":plug(&[(2,"m_beginPlug")],&[1])
            }),
        );
        if allow_dead {
            listener["fields"]["m_5AE05408"] = json!(true);
        }
        let mut scale = node(
            2,
            "AuthoredHoldScale",
            Some(1),
            json!({"factor":input["hold_scale"]}),
        );
        scale["state_behavior"] = json!("movement_scale");
        scale["server_only"] = json!(true);
        let mut lock = node(3, "AuthoredHoldLock", Some(2), json!({"button":button}));
        lock["state_behavior"] = json!("button_lock");
        lock["server_only"] = json!(true);
        let mut model = graph(
            graph_index,
            vec![
                node(
                    0,
                    "STUStatescriptEntryGameMessage",
                    None,
                    json!({
                        "m_gameMessage":{"m_gameMessage":format!("0x{ENTER:016X}")},
                        "m_outPlug":plug(&[(1,"m_beginPlug")],&[])
                    }),
                ),
                listener,
                scale,
                lock,
            ],
            json!([1, 2, 3]),
            json!([0]),
        );
        model["fields"]["m_680A2CB2"] = json!([0]);
        let graphs = Arc::new(store(vec![model]));
        let start = || {
            Component::start(
                Arc::clone(&graphs),
                body,
                Variables::new(),
                vec![Root::new(1, graph_index)],
                0,
                &mut EmptyWorld,
                MAX_INIT_WORK,
            )
            .unwrap()
        };

        let mut late_listener = start();
        late_listener.set_owner_dead(true);
        late_listener
            .advance_command(held, &mut EmptyWorld)
            .unwrap();
        enter(&mut late_listener);
        check(&mut late_listener, expected);

        let mut alive_hold = start();
        enter(&mut alive_hold);
        alive_hold.advance_command(held, &mut EmptyWorld).unwrap();
        check(&mut alive_hold, true);
        alive_hold.set_owner_dead(true);
        alive_hold.advance_frame(2, &mut EmptyWorld).unwrap();
        check(&mut alive_hold, expected);
        alive_hold
            .advance_command(
                Command {
                    frame: 3,
                    ..Command::default()
                },
                &mut EmptyWorld,
            )
            .unwrap();
        check(&mut alive_hold, false);

        let mut locked_hold = start();
        enter(&mut locked_hold);
        locked_hold.advance_command(held, &mut EmptyWorld).unwrap();
        locked_hold.begin_state(1, 2, &mut EmptyWorld).unwrap();
        check(&mut locked_hold, false);
        locked_hold.set_owner_dead(true);
        locked_hold.debug_end_state(1, 2, &mut EmptyWorld).unwrap();
        assert_eq!(
            locked_hold.snapshot().unwrap().instances[&1].states[&0].kind,
            StateKind::LogicalButton { down: expected }
        );
        locked_hold.advance_frame(2, &mut EmptyWorld).unwrap();
        check(&mut locked_hold, expected);
    }
}
