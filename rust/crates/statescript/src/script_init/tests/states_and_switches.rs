use super::*;

fn recent_input_fixture() -> Component {
    let entries =
        [(1, "m_beginPlug"), (2, "m_F4DD996D"), (3, ABORT_PLUG)].map(|(position, input)| {
            node(
                position,
                "STUStatescriptEntryGameMessage",
                None,
                json!({"m_gameMessage":{"m_gameMessage":format!("0x024000000000{position:04X}")},
                "m_outPlug":plug(&[(0,input)], &[])}),
            )
        });
    let mut nodes = vec![node(
        0,
        "STU_5E805B5A",
        Some(0),
        json!({
            "m_C51AF683":var(101,true), "m_82317B5F":var(103,true),
            "m_814F8EC3":var(100,true), "m_1405CDCB":plug(&[(4,"m_inPlug")], &[]),
        }),
    )];
    nodes.extend(entries);
    nodes.push(node(
        4,
        "STUStatescriptActionSetVar",
        None,
        json!({"m_out_Var":var(102,true),"m_value":true}),
    ));
    Component::start(
        Arc::new(store(vec![graph(
            0x0cba,
            nodes,
            json!([0]),
            json!([1, 2, 3]),
        )])),
        42,
        Variables::from([(103, Value::Float(1.0))]),
        vec![Root::new(1, 0x0cba)],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap()
}

fn recent_input_message(component: &mut Component, message: u64) {
    component
        .queue_message(1, 0x0240_0000_0000_0000 | message, Variables::new())
        .unwrap();
    component.run_now(&mut EmptyWorld).unwrap();
    assert!(component.snapshot().unwrap().initialization_complete());
}

#[test]
fn recent_input_avoids_history_expires_and_resets_on_reentry() {
    let mut component = recent_input_fixture();
    let positions = (0..3)
        .map(|i| Value::Vector([f64::from(i), 0.0, 0.0]))
        .collect::<Vec<_>>();
    component
        .set_entity_variable(100, Value::Array(positions.clone()), &mut EmptyWorld)
        .unwrap();
    recent_input_message(&mut component, 2);
    assert!(!component.snapshot().unwrap().variables.contains_key(&101));
    recent_input_message(&mut component, 1);
    let mut selected = Vec::new();
    for _ in 0..3 {
        recent_input_message(&mut component, 2);
        let snapshot = component.snapshot().unwrap();
        let position = snapshot.variables[&101].clone();
        assert!(positions.contains(&position));
        assert!(!selected.contains(&position));
        selected.push(position);
        assert_eq!(snapshot.variables[&102], Value::Bool(true));
    }
    recent_input_message(&mut component, 2);
    assert_eq!(component.snapshot().unwrap().variables[&101], selected[0]);
    component.advance_frame(67, &mut EmptyWorld).unwrap();
    recent_input_message(&mut component, 2);
    assert_eq!(
        component.snapshot().unwrap().instances[&1].states[&0]
            .recent_inputs
            .len(),
        1
    );
    recent_input_message(&mut component, 3);
    recent_input_message(&mut component, 1);
    assert!(
        component.snapshot().unwrap().instances[&1].states[&0]
            .recent_inputs
            .is_empty()
    );
}

#[test]
fn recent_input_handles_shrinking_empty_and_zero_lifetime_candidates() {
    let mut component = recent_input_fixture();
    recent_input_message(&mut component, 1);
    let positions = (0..5)
        .map(|i| Value::Vector([f64::from(i), 0.0, 0.0]))
        .collect::<Vec<_>>();
    component
        .set_entity_variable(100, Value::Array(positions), &mut EmptyWorld)
        .unwrap();
    for _ in 0..5 {
        recent_input_message(&mut component, 2);
    }
    let only = Value::Vector([9.0, 0.0, 0.0]);
    component
        .set_entity_variable(100, Value::Array(vec![only.clone()]), &mut EmptyWorld)
        .unwrap();
    recent_input_message(&mut component, 2);
    assert_eq!(component.snapshot().unwrap().variables[&101], only);
    component
        .set_entity_variable(100, Value::Array(vec![]), &mut EmptyWorld)
        .unwrap();
    recent_input_message(&mut component, 2);
    assert_eq!(component.snapshot().unwrap().variables[&101], Value::Null);
    component
        .set_entity_variable(103, Value::Float(-1.0), &mut EmptyWorld)
        .unwrap();
    component
        .set_entity_variable(100, Value::Array(vec![only.clone()]), &mut EmptyWorld)
        .unwrap();
    recent_input_message(&mut component, 2);
    assert_eq!(component.snapshot().unwrap().variables[&101], only);
    assert!(
        component.snapshot().unwrap().instances[&1].states[&0]
            .recent_inputs
            .is_empty()
    );
    component
        .set_entity_variable(103, Value::Float(0.0), &mut EmptyWorld)
        .unwrap();
    recent_input_message(&mut component, 2);
    assert!(
        component.snapshot().unwrap().instances[&1].states[&0]
            .recent_inputs
            .is_empty()
    );
}

#[test]
fn bridge_forwards_input_only_between_begin_and_abort() {
    let message = |position, input| {
        node(
            position,
            "STUStatescriptEntryGameMessage",
            None,
            json!({
                "m_gameMessage":{"m_gameMessage":format!("0x024000000000{position:04X}")},
                "m_outPlug":plug(&[(0,input)], &[]),
            }),
        )
    };
    let fixture = graph(
        42,
        vec![
            node(
                0,
                "STU_7B9E8909",
                Some(0),
                json!({"m_outPlug":plug(&[(1,"m_inPlug")], &[])}),
            ),
            node(
                1,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var":var(100,true), "m_value":true}),
            ),
            message(2, "m_inPlug"),
            message(3, "m_beginPlug"),
            message(4, ABORT_PLUG),
        ],
        json!([0]),
        json!([2, 3, 4]),
    );
    let mut component = Component::start(
        Arc::new(store(vec![fixture])),
        42,
        Variables::new(),
        vec![Root::new(1, 42)],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    for (message, active, activation, forwarded) in [
        (2, false, 0, false),
        (3, true, 1, false),
        (2, true, 1, true),
        (2, true, 1, true),
        (4, false, 1, false),
        (2, false, 1, false),
    ] {
        component
            .set_entity_variable(100, Value::Bool(false), &mut EmptyWorld)
            .unwrap();
        component
            .queue_message(1, 0x0240_0000_0000_0000 | message, Variables::new())
            .unwrap();
        component.run_now(&mut EmptyWorld).unwrap();
        let snapshot = component.snapshot().unwrap();
        let state = &snapshot.instances[&1].states[&0];
        assert_eq!((state.active, state.activation), (active, activation));
        assert_eq!(snapshot.variables[&100], Value::Bool(forwarded));
        assert!(snapshot.initialization_complete());
    }
}

#[test]
fn client_subgraph_controller_keeps_its_branch_off_the_server() {
    for client_only in [true, false] {
        let mut target = node(2, "STUStatescriptState", Some(1), json!({}));
        target["client_only"] = json!(client_only);
        let graphs = store(vec![graph(
            42,
            vec![
                node(
                    0,
                    "STUStatescriptEntry",
                    None,
                    json!({"m_outPlug":plug(&[(1, "m_beginPlug")], &[])}),
                ),
                node(
                    1,
                    "STU_BD02E168",
                    Some(0),
                    json!({"m_5B10D01F":{
                        "$":"STU_904BFCEC","links":[[2, "m_beginPlug"]],"m_FF3DAF1E":[1]
                    }}),
                ),
                target,
            ],
            json!([1, 2]),
            json!([0]),
        )]);
        let snapshot = initialize(
            &graphs,
            0xa000_0100,
            Variables::new(),
            vec![Root::new(1, 42)],
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap();
        let states = &snapshot.instances[&1].states;
        assert!(!states.contains_key(&1));
        if client_only {
            assert!(snapshot.initialization_complete());
            assert!(states[&0].active);
            assert_eq!(states[&0].kind, StateKind::Plain);
        } else {
            assert!(!states.contains_key(&0));
            assert!(matches!(
                snapshot.diagnostics.as_slice(),
                [Diagnostic { issue: Issue::UnsupportedState(class), .. }]
                    if class == "STU_BD02E168"
            ));
        }
    }
}

#[test]
fn networked_for_code_state_keeps_its_subgraph_lifetime() {
    for (server_only, flags) in [(false, 0), (true, 0), (false, 1)] {
        let mut for_code = node(
            1,
            "STU_CD46AF93",
            Some(0),
            json!({
                "m_DC897C87":1, "m_598E32D5":flags, "m_0582B0E4":0,
                "m_values":[{
                    "$":"STUStatescriptForCodeValue",
                    "m_identifier":"0x0D80000000000042",
                    "m_value":var(100,true),
                }],
                "m_subgraphPlug":plug(&[(2,"m_beginPlug")], &[1]),
            }),
        );
        for_code["remote"] = json!(0);
        for_code["server_only"] = json!(server_only);
        let mut fixture = graph(
            42,
            vec![
                node(
                    0,
                    "STUStatescriptEntry",
                    None,
                    json!({"m_outPlug":plug(&[(1,"m_beginPlug")], &[])}),
                ),
                for_code,
                node(2, "STUStatescriptState", Some(1), json!({})),
                node(
                    3,
                    "STUStatescriptEntryGameMessage",
                    None,
                    json!({
                        "m_gameMessage":{"m_gameMessage":"0x0240000000000003"},
                        "m_outPlug":plug(&[(1,ABORT_PLUG)], &[]),
                    }),
                ),
            ],
            json!([1, 2]),
            json!([0, 3]),
        );
        fixture["remote"] = json!([1]);
        let mut component = Component::start(
            Arc::new(store(vec![fixture])),
            42,
            Variables::from([(100, Value::Integer(5))]),
            vec![Root::new(1, 42)],
            0,
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap();
        let snapshot = component.snapshot().unwrap();
        let states = &snapshot.instances[&1].states;
        if server_only || flags != 0 {
            assert!(states.is_empty());
            assert!(matches!(
                snapshot.diagnostics.as_slice(),
                [Diagnostic { issue: Issue::UnsupportedState(class), .. }]
                    if class == "STU_CD46AF93"
            ));
            continue;
        }
        assert!(snapshot.initialization_complete());
        assert!(states[&0].active && states[&1].active);
        assert_eq!(states[&0].kind, StateKind::ForCode);
        assert!(!snapshot.variables.contains_key(&0x42));
        component
            .queue_message(1, 0x0240_0000_0000_0003, Variables::new())
            .unwrap();
        component.run_now(&mut EmptyWorld).unwrap();
        let snapshot = component.snapshot().unwrap();
        let states = &snapshot.instances[&1].states;
        assert!(!states[&0].active && !states[&1].active);
    }
}

#[test]
fn for_code_values_preserve_first_match_swap_removal_and_deferred_observer_deduplication() {
    #[derive(Default)]
    struct CodeWorld(Vec<u64>);
    impl World for CodeWorld {
        fn for_code_transport(&self, _: Site) -> Option<bool> {
            Some(false)
        }
        fn code_changed(&mut self, _: u32, token: u64) -> bool {
            self.0.push(token);
            true
        }
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }
    }
    let a = 0x0d80_0000_0000_0001;
    let b = 0x0d80_0000_0000_0002;
    let row = |id: u64, value: Json| json!({"$":"STUStatescriptForCodeValue","m_identifier":format!("0x{id:016X}"),"m_value":value});
    let fixture = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_beginPlug"),(2,"m_beginPlug"),(3,"m_beginPlug")],&[])}),
            ),
            node(
                1,
                "STU_6A926B21",
                Some(0),
                json!({"m_DC897C87":1,"m_values":[row(a,Json::Null)]}),
            ),
            node(
                2,
                "STU_6A926B21",
                Some(1),
                json!({"m_DC897C87":1,"m_values":[row(b,json!({"$":"STUConfigVarBool","m_value":true})),row(a,var(100,true))]}),
            ),
            node(
                3,
                "STU_6A926B21",
                Some(2),
                json!({"m_DC897C87":1,"m_values":[row(a,json!({"$":"STUConfigVarFloat","m_value":9}))]}),
            ),
            node(
                4,
                "STUStatescriptEntryGameMessage",
                None,
                json!({"m_gameMessage":{"m_gameMessage":"0x0240000000000003"},"m_outPlug":plug(&[(1,ABORT_PLUG)],&[])}),
            ),
        ],
        json!([1, 2, 3]),
        json!([0, 4]),
    );
    let mut world = CodeWorld::default();
    let mut component = Component::start(
        Arc::new(store(vec![fixture])),
        42,
        Variables::from([(100, Value::Float(4.0))]),
        vec![Root::new(1, 42)],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert!(component.snapshot().unwrap().initialization_complete());
    assert_eq!(
        component
            .code_value(a, Value::Integer(7), &mut world)
            .unwrap(),
        (true, Value::Integer(7))
    );
    assert_eq!(
        component
            .code_value(b, Value::Integer(7), &mut world)
            .unwrap(),
        (true, Value::Bool(true))
    );
    assert_eq!(
        component
            .code_value(a + 10, Value::Integer(7), &mut world)
            .unwrap(),
        (false, Value::Null)
    );
    for observer in [(1, None), (2, Some(a)), (3, Some(b)), (1, Some(a))] {
        component.register_code_observer(observer.0, observer.1);
    }
    component.advance_frame(1, &mut world).unwrap();
    assert_eq!(world.0, [1, 2, 3]);
    world.0.clear();
    component
        .queue_message(1, 0x0240_0000_0000_0003, Variables::new())
        .unwrap();
    component.run_now(&mut world).unwrap();
    assert_eq!(world.0, [1, 2]);
    assert_eq!(
        component.code_value(a, Value::Null, &mut world).unwrap(),
        (true, Value::Float(9.0))
    );
    component.unregister_code_observer(1);
    world.0.clear();
    component.advance_frame(2, &mut world).unwrap();
    world.0.sort();
    assert_eq!(world.0, [2, 3]);
    component.teardown(&mut world).unwrap();
    assert!(component.machine.code_states.is_empty());
}

#[test]
fn state_elapsed_condition_rearms_one_timer_and_completes_at_the_authored_age() {
    let fixture = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_beginPlug")],&[])}),
            ),
            node(
                1,
                "STUStatescriptState",
                Some(0),
                json!({"m_transitionPlug":[plug(&[(2,"m_inPlug")],&[])]}),
            ),
            node(
                2,
                "STU_4E2BCAAB",
                None,
                json!({"m_AA6E611E":{"$":"STUConfigVarFloat","m_value":0.03},"m_truePlug":plug(&[(3,"m_inPlug")],&[])}),
            ),
            node(
                3,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var":var(100,true),"m_value":true}),
            ),
            node(
                4,
                "STUStatescriptEntryGameMessage",
                None,
                json!({"m_gameMessage":{"m_gameMessage":"0x0240000000000003"},"m_outPlug":plug(&[(1,"m_beginPlug")],&[])}),
            ),
        ],
        json!([1]),
        json!([0, 4]),
    );
    let mut component = Component::start(
        Arc::new(store(vec![fixture])),
        42,
        Variables::new(),
        vec![Root::new(1, 42)],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    for _ in 0..2 {
        component
            .queue_message(1, 0x0240_0000_0000_0003, Variables::new())
            .unwrap();
        component.run_now(&mut EmptyWorld).unwrap();
    }
    let timers = &component.snapshot().unwrap().pending_timers[&1];
    assert_eq!(timers.len(), 1);
    assert_eq!(timers[0].due_ms, 30);
    assert_eq!(timers[0].parameter, 0);
    component.advance_frame(1, &mut EmptyWorld).unwrap();
    assert_eq!(
        component.snapshot().unwrap().variables[&100],
        Value::Bool(true)
    );
    assert!(!component.snapshot().unwrap().instances[&1].states[&0].active);
    assert!(component.snapshot().unwrap().pending_timers.is_empty());
}

#[test]
fn movement_sample_updates_only_changed_authored_outputs() {
    struct SampleWorld(Option<u16>);
    impl World for SampleWorld {
        fn movement_sample(&self, _: Site) -> Option<u16> {
            self.0
        }
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }
    }

    let fixture = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({
                    "m_outPlug":plug(&[(1,"m_beginPlug")], &[])
                }),
            ),
            node(
                1,
                "STU_96F1DBCF",
                Some(0),
                json!({
                    "m_49F4B33C":var(100,true),
                    "m_103A8581":var(101,false),
                    "m_subgraphPlug":plug(&[(2,"m_beginPlug")], &[1])
                }),
            ),
            node(
                2,
                "STUStatescriptStateBooleanSwitch",
                Some(1),
                json!({
                    "m_condition":var(100,true)
                }),
            ),
        ],
        json!([1, 2]),
        json!([0]),
    );
    let graphs = Arc::new(store(vec![fixture]));
    let start = |world: &mut SampleWorld| {
        Component::start(
            Arc::clone(&graphs),
            42,
            Variables::new(),
            vec![Root::new(1, 42)],
            0,
            world,
            MAX_INIT_WORK,
        )
        .unwrap()
    };
    let mut world = SampleWorld(Some(2));
    let mut component = start(&mut world);
    let snapshot = component.snapshot().unwrap();
    assert!(snapshot.initialization_complete());
    let owner = &snapshot.instances[&1];
    assert_eq!(owner.states[&0].kind, StateKind::MovementSample(2));
    assert_eq!(owner.states[&1].kind, StateKind::BooleanSwitch(true));
    assert_eq!(snapshot.variables[&100], Value::Bool(true));
    assert!(!owner.variables.contains_key(&101));

    world.0 = Some(1 | (1 << 6));
    component.advance_frame(1, &mut world).unwrap();
    let snapshot = component.snapshot().unwrap();
    let owner = &snapshot.instances[&1];
    assert_eq!(owner.states[&0].kind, StateKind::MovementSample(65));
    assert_eq!(owner.states[&1].kind, StateKind::BooleanSwitch(false));
    assert_eq!(snapshot.variables[&100], Value::Bool(false));
    assert_eq!(owner.variables[&101], Value::Bool(true));

    world.0 = Some(0);
    component.advance_frame(2, &mut world).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert_eq!(
        snapshot.instances[&1].states[&0].kind,
        StateKind::MovementSample(0)
    );
    assert_eq!(snapshot.instances[&1].variables[&101], Value::Bool(false));

    let unsupported = start(&mut SampleWorld(None));
    assert!(matches!(
        unsupported.snapshot().unwrap().diagnostics.as_slice(),
        [Diagnostic {
            issue: Issue::UnsupportedField("owner movement sample"),
            ..
        }]
    ));
}

#[test]
fn instance_exit_drains_in_order_and_finishes_the_linked_parent() {
    let entry = |links: &[(u16, &str)]| {
        node(
            0,
            "STUStatescriptEntry",
            None,
            json!({"m_outPlug":plug(links, &[])}),
        )
    };
    let mark = |position, identifier| {
        node(
            position,
            "STUStatescriptActionSetVar",
            None,
            json!({
                "m_out_Var":var(identifier,true), "m_value":true
            }),
        )
    };
    let subscript = |position, raw, child: u16, keep_parent| {
        node(
            position,
            "STU_37D754C8",
            Some(raw),
            json!({
                "m_36F7B64C":{"m_36F7B64C":{"m_C974E369":{
                    "m_graph":format!("0x058000000000{child:04X}")
                }}},
                "m_24AB04BB":keep_parent
            }),
        )
    };
    let child = graph(
        43,
        vec![
            entry(&[
                (1, "m_beginPlug"),
                (2, "m_beginPlug"),
                (3, "m_inPlug"),
                (4, "m_beginPlug"),
            ]),
            node(
                1,
                "STUStatescriptStateWait",
                Some(0),
                json!({
                    "m_timeout":{"$":"STUConfigVarFloat","m_value":1.0},
                    "m_onBeginPlug":plug(&[(5,"m_inPlug")], &[]),
                    "m_onAbortPlug":plug(&[(6,"m_inPlug")], &[]),
                    "m_onEndPlug":plug(&[(6,"m_inPlug")], &[])
                }),
            ),
            subscript(2, 1, 44, false),
            node(
                3,
                "STU_22926252",
                None,
                json!({"m_outPlug":plug(&[(7,"m_inPlug")], &[])}),
            ),
            node(
                4,
                "STUStatescriptState",
                Some(2),
                json!({"m_onBeginPlug":plug(&[(8,"m_inPlug")], &[])}),
            ),
            mark(5, 100),
            mark(6, 101),
            mark(7, 102),
            mark(8, 103),
        ],
        json!([1, 2, 4]),
        json!([0]),
    );
    let grandchild = graph(
        44,
        vec![entry(&[(1, "m_inPlug")]), mark(1, 104)],
        json!([]),
        json!([0]),
    );
    for keep_parent in [false, true] {
        let mut owner = subscript(1, 0, 43, keep_parent);
        owner["fields"]["m_onFinishedPlug"] = plug(&[(2, "m_inPlug")], &[]);
        owner["fields"]["m_onAbortPlug"] = plug(&[(3, "m_inPlug")], &[]);
        let parent = graph(
            42,
            vec![
                entry(&[(1, "m_beginPlug")]),
                owner,
                mark(2, 105),
                mark(3, 106),
            ],
            json!([1]),
            json!([0]),
        );
        let snapshot = initialize(
            &store(vec![parent, child.clone(), grandchild.clone()]),
            42,
            Variables::new(),
            vec![Root::new(1, 42)],
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(snapshot.instances.keys().copied().collect::<Vec<_>>(), [1]);
        assert!(snapshot.pending_timers.is_empty());
        let state = &snapshot.instances[&1].states[&0];
        assert_eq!(state.active, keep_parent);
        assert_eq!(state.kind, StateKind::SubScript { child: None });
        let mut expected = Variables::from([(100, Value::Bool(true)), (102, Value::Bool(true))]);
        if !keep_parent {
            expected.insert(105, Value::Bool(true));
        }
        assert_eq!(snapshot.variables, expected);
    }
}

#[test]
fn target_tag_releases_on_abort_and_subscript_teardown() {
    #[derive(Default)]
    struct TagWorld(Vec<(TargetTagEffect, bool)>);

    impl World for TagWorld {
        fn set_target_tag_effect(
            &mut self,
            _: Site,
            effect: TargetTagEffect,
            active: bool,
        ) -> bool {
            self.0.push((effect, active));
            true
        }
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }
    }

    const ABORT: u64 = 0x0240_0000_0000_0002;
    const RESTART: u64 = 0x0240_0000_0000_0003;
    let entry = |position, message: Option<u64>, input| {
        let (class, mut fields) = match message {
            Some(message) => (
                "STUStatescriptEntryGameMessage",
                json!({"m_gameMessage":{"m_gameMessage":format!("0x{message:016X}")}}),
            ),
            None => ("STUStatescriptEntry", json!({})),
        };
        fields["m_outPlug"] = plug(&[(1, input)], &[]);
        node(position, class, None, fields)
    };
    let parent = graph(
        42,
        vec![
            entry(0, None, "m_beginPlug"),
            node(
                1,
                "STU_37D754C8",
                Some(0),
                json!({
                    "m_36F7B64C":{"m_36F7B64C":{"m_C974E369":{"m_graph":"0x058000000000002B"}}}
                }),
            ),
            entry(2, Some(ABORT), ABORT_PLUG),
        ],
        json!([1]),
        json!([0, 2]),
    );
    let child = graph(
        43,
        vec![
            entry(0, None, "m_beginPlug"),
            node(
                1,
                "STU_E56BA8D8",
                Some(0),
                json!({
                    "m_31714AFC":{"$":"STU_B5A0CAF0"},
                    "m_44D1BE94":{"$":"STU_357DABBB","m_7D7C86A1":"0x0D60000000000042"}
                }),
            ),
            entry(2, Some(ABORT), ABORT_PLUG),
            entry(3, Some(RESTART), "m_beginPlug"),
        ],
        json!([1]),
        json!([0, 2, 3]),
    );
    let effect = TargetTagEffect {
        entity: 42,
        tag: 0x0d60_0000_0000_0042,
        present: false,
    };
    let mut world = TagWorld::default();
    let mut component = Component::start(
        Arc::new(store(vec![parent, child])),
        effect.entity,
        Variables::new(),
        vec![Root::new(1, 42)],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert!(component.snapshot().unwrap().initialization_complete());
    assert_eq!(world.0, [(effect, true)]);

    for (id, message) in [(2, ABORT), (2, RESTART), (1, ABORT)] {
        assert_eq!(
            component
                .queue_message(id, message, Variables::new())
                .unwrap(),
            1
        );
        component.run_now(&mut world).unwrap();
    }
    assert!(!component.snapshot().unwrap().instances.contains_key(&2));
    assert_eq!(
        world.0,
        [
            (effect, true),
            (effect, false),
            (effect, true),
            (effect, false)
        ]
    );
}

#[test]
fn display_name_binds_and_releases_with_the_authored_state() {
    #[derive(Default)]
    struct NameWorld(Vec<(u32, u64, bool)>);
    impl World for NameWorld {
        fn entity_component92(&self, _: Site, _: u32) -> Option<bool> {
            Some(true)
        }
        fn set_display_name(&mut self, _: Site, entity: u32, name: u64, active: bool) -> bool {
            self.0.push((entity, name, active));
            true
        }
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }
    }
    const ABORT: u64 = 0x0240_0000_0000_0002;
    let mut graph = graph(
        0x0cf3,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(22,"m_beginPlug")],&[])}),
            ),
            node(
                1,
                "STUStatescriptEntryGameMessage",
                None,
                json!({
                    "m_gameMessage":{"m_gameMessage":format!("0x{ABORT:016X}")},
                    "m_outPlug":plug(&[(22,ABORT_PLUG)],&[])
                }),
            ),
            node(
                22,
                "STU_B7D2C9C0",
                Some(0),
                json!({
                    "m_14EDDDA7":{"$":"STU_B5A0CAF0"},
                    "m_overrideName":"0x0DE0000000007106"
                }),
            ),
        ],
        json!([22]),
        json!([0, 1]),
    );
    graph["items"] = json!(23);
    let mut world = NameWorld::default();
    let mut component = Component::start(
        Arc::new(store(vec![graph])),
        42,
        Variables::new(),
        vec![Root::new(1, 0x0cf3)],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert!(component.snapshot().unwrap().initialization_complete());
    assert_eq!(world.0, [(42, 0x0de0_0000_0000_7106, true)]);
    assert_eq!(
        component.queue_message(1, ABORT, Variables::new()).unwrap(),
        1
    );
    component.run_now(&mut world).unwrap();
    assert_eq!(
        world.0,
        [
            (42, 0x0de0_0000_0000_7106, true),
            (42, 0x0de0_0000_0000_7106, false)
        ]
    );
}
fn delayed_switch_fixture(mut fields: Json) -> Arc<GraphStore> {
    fields["m_onTruePlug"] = plug(&[(6, "m_inPlug")], &[]);
    fields["m_onFalsePlug"] = plug(&[(7, "m_inPlug")], &[]);
    fields["m_trueSubgraphPlug"] = plug(&[(2, "m_beginPlug")], &[1]);
    fields["m_falseSubgraphPlug"] = plug(&[(3, "m_beginPlug")], &[2]);
    let entry = |position, input| {
        node(
            position,
            "STUStatescriptEntryGameMessage",
            None,
            json!({
                "m_gameMessage":{"m_gameMessage":format!("0x024000000000{position:04X}")},
                "m_outPlug":plug(&[(1,input)], &[]),
            }),
        )
    };
    let record_branch = |position, branch| {
        node(
            position,
            "STUStatescriptActionSetVar",
            None,
            json!({"m_out_Var":var(103,true), "m_value":branch}),
        )
    };
    Arc::new(store(vec![graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1, "m_beginPlug")], &[])}),
            ),
            node(1, "STU_B591EBFE", Some(0), fields),
            node(2, "STUStatescriptState", Some(1), json!({})),
            node(3, "STUStatescriptState", Some(2), json!({})),
            entry(4, ABORT_PLUG),
            entry(5, "m_beginPlug"),
            record_branch(6, true),
            record_branch(7, false),
        ],
        json!([1, 2, 3]),
        json!([0, 4, 5]),
    )]))
}

fn assert_delayed_switch(component: &Component, branch: Option<bool>, timer: Option<(u64, u16)>) {
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    let states = &snapshot.instances[&1].states;
    assert!(states[&0].active);
    assert_eq!(
        states[&0].kind,
        StateKind::DelayedSwitch {
            current: branch == Some(true),
            undecided: branch.is_none(),
        },
    );
    assert_eq!(
        states.get(&1).is_some_and(|state| state.active),
        branch == Some(true)
    );
    assert_eq!(
        states.get(&2).is_some_and(|state| state.active),
        branch == Some(false)
    );
    if let Some(branch) = branch {
        assert_eq!(snapshot.variables[&103], Value::Bool(branch));
    }
    let expected: Vec<_> = timer
        .map(|(due_ms, parameter)| PendingTimer {
            due_ms,
            raw_state: 0,
            finish: false,
            parameter,
        })
        .into_iter()
        .collect();
    assert_eq!(
        snapshot
            .pending_timers
            .get(&1)
            .map_or(&[][..], Vec::as_slice),
        expected,
    );
}

#[test]
fn delayed_switch_uses_initial_override_and_asymmetric_interruptible_timers() {
    let mut component = Component::start(
        delayed_switch_fixture(json!({
            "m_418D9D9D": true,
            "m_BD83C8E6": var(100,true),
            "m_E8FC060D": var(101,true),
            "m_7130456B": 0.0259,
            "m_ED60694A": 0.04075,
        })),
        42,
        Variables::from([(100, Value::Bool(false)), (101, Value::Bool(false))]),
        vec![Root::new(1, 42)],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    let write = |component: &mut Component, id, value| {
        component
            .set_entity_variable(id, Value::Bool(value), &mut EmptyWorld)
            .unwrap();
        component.run_now(&mut EmptyWorld).unwrap();
    };

    assert_delayed_switch(&component, Some(true), None);
    write(&mut component, 101, true);
    assert_delayed_switch(&component, Some(true), Some((40, SWITCH_TO_FALSE)));
    component.advance_frame(1, &mut EmptyWorld).unwrap();
    assert_delayed_switch(&component, Some(true), Some((40, SWITCH_TO_FALSE)));
    write(&mut component, 101, false);
    component.advance_frame(2, &mut EmptyWorld).unwrap();
    assert_delayed_switch(&component, Some(true), None);
    write(&mut component, 101, true);
    assert_delayed_switch(&component, Some(true), Some((87, SWITCH_TO_FALSE)));
    for frame in 3..=5 {
        component.advance_frame(frame, &mut EmptyWorld).unwrap();
    }
    assert_delayed_switch(&component, Some(false), None);
    write(&mut component, 101, false);
    write(&mut component, 100, true);
    assert_delayed_switch(&component, Some(false), Some((120, SWITCH_TO_TRUE)));
    for frame in 6..=7 {
        component.advance_frame(frame, &mut EmptyWorld).unwrap();
    }
    assert_delayed_switch(&component, Some(true), None);
}

#[test]
fn delayed_switch_polls_undecided_inputs_and_clears_timers_on_abort() {
    #[derive(Default)]
    struct Conditions {
        positive: bool,
        negative: bool,
    }

    impl World for Conditions {
        fn native(&mut self, _site: Site, config: &Json) -> Option<Value> {
            match config["$"].as_str()? {
                "test-delayed-positive" => Some(Value::Bool(self.positive)),
                "test-delayed-negative" => Some(Value::Bool(self.negative)),
                _ => None,
            }
        }
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }
    }

    let mut world = Conditions::default();
    let mut component = Component::start(
        delayed_switch_fixture(json!({
            "m_308B8A80": true,
            "m_418D9D9D": true,
            "m_BD83C8E6": {"$":"test-delayed-positive"},
            "m_E8FC060D": {"$":"test-delayed-negative"},
            "m_7130456B": 0.075,
            "m_ED60694A": 0.001,
        })),
        42,
        Variables::new(),
        vec![Root::new(1, 42)],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert_delayed_switch(&component, None, None);
    assert!(!component.snapshot().unwrap().variables.contains_key(&103));
    world.positive = true;
    component.advance_frame(0, &mut world).unwrap();
    assert_delayed_switch(&component, None, Some((90, SWITCH_TO_TRUE)));
    world.positive = false;
    component.advance_frame(1, &mut world).unwrap();
    assert_delayed_switch(&component, None, Some((90, SWITCH_TO_TRUE)));
    world.negative = true;
    component.advance_frame(2, &mut world).unwrap();
    assert_delayed_switch(&component, None, Some((48, SWITCH_TO_FALSE)));
    component.advance_frame(3, &mut world).unwrap();
    assert_delayed_switch(&component, Some(false), None);

    world.positive = true;
    component.advance_frame(4, &mut world).unwrap();
    assert_delayed_switch(&component, Some(false), Some((154, SWITCH_TO_TRUE)));
    component
        .queue_message(1, 0x0240_0000_0000_0004, Variables::new())
        .unwrap();
    component.run_now(&mut world).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert!(snapshot.pending_timers.is_empty());
    assert!(!snapshot.instances[&1].states[&0].active);
    assert!(!snapshot.instances[&1].states[&2].active);

    world = Conditions::default();
    component
        .queue_message(1, 0x0240_0000_0000_0005, Variables::new())
        .unwrap();
    component.run_now(&mut world).unwrap();
    component.advance_frame(10, &mut world).unwrap();
    assert_delayed_switch(&component, None, None);
    assert_eq!(
        component.snapshot().unwrap().instances[&1].states[&0].activation,
        2
    );
}

#[test]
fn startup_queues_states_rechecks_scoped_writes_and_starts_children_after_roots() {
    let mut client_only = node(6, "UnportedClientAction", None, json!({}));
    client_only["client_only"] = json!(true);
    let first = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({
                    "m_outPlug":plug(&[(1,"m_beginPlug"),(2,"m_beginPlug")], &[])
                }),
            ),
            node(
                1,
                "STUStatescriptStateBooleanSwitch",
                Some(0),
                json!({
                    "m_condition":var(100, true),
                    "m_falseSubgraphPlug":plug(&[(3,"m_beginPlug")], &[2]),
                    "m_trueSubgraphPlug":plug(&[(4,"m_beginPlug")], &[3]),
                }),
            ),
            node(
                2,
                "STUStatescriptState",
                Some(1),
                json!({
                    "m_onBeginPlug":plug(&[(5,"m_inPlug"),(6,"m_inPlug")], &[])
                }),
            ),
            node(3, "STUStatescriptState", Some(2), json!({})),
            node(
                4,
                "STU_37D754C8",
                Some(3),
                json!({
                    "m_36F7B64C":{"m_36F7B64C":{"m_C974E369":{"m_graph":"0x058000000000002B"}}},
                    "m_89402D74":[{"m_out_Var":var(4, false), "m_value":var(9, false)}]
                }),
            ),
            node(
                5,
                "STUStatescriptActionSetVar",
                None,
                json!({
                    "m_out_Var":var(100, true), "m_value":{"$":"STUConfigVarBool","m_value":true}
                }),
            ),
            client_only,
        ],
        json!([1, 2, 3, 4]),
        json!([0]),
    );
    let child = graph(
        43,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_inPlug")], &[])}),
            ),
            node(
                1,
                "STU_12F5D52D",
                None,
                json!({
                    "m_143D4C5B":[
                        {"m_out_Var":var(12, true),"m_value":parent_var(9)},
                        {"m_out_Var":var(14, true),"m_value":var(4, false)},
                        {"m_out_Var":var(15, true),"m_value":var(13, true)}
                    ]
                }),
            ),
        ],
        json!([]),
        json!([0]),
    );
    let second = graph(
        44,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_inPlug")], &[])}),
            ),
            node(
                1,
                "STUStatescriptActionSetVar",
                None,
                json!({
                    "m_out_Var":var(13, true), "m_value":{"$":"STUConfigVarInt","m_value":99}
                }),
            ),
        ],
        json!([]),
        json!([0]),
    );
    let graphs = store(vec![first, child, second]);
    let mut root = Root::new(7, 42);
    root.variables.insert(9, Value::Integer(3));
    let snapshot = initialize(
        &graphs,
        0xa000_0100,
        Variables::from([(100, Value::Bool(false))]),
        vec![root, Root::new(1, 44)],
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    assert_eq!(
        snapshot.instances.keys().copied().collect::<Vec<_>>(),
        [7, 1, 8]
    );
    assert_eq!(snapshot.instances[&8].parent, Some((7, 3)));
    assert_eq!(snapshot.instances[&8].variables[&4], Value::Integer(3));
    let states = &snapshot.instances[&7].states;
    assert_eq!(states[&0].kind, StateKind::BooleanSwitch(true));
    assert!(!states[&2].active);
    assert_eq!(states[&2].activation, 1);
    assert_eq!(states[&3].kind, StateKind::SubScript { child: Some(8) });
    assert_eq!(snapshot.variables[&12], Value::Integer(3));
    assert_eq!(snapshot.variables[&14], Value::Integer(3));
    assert_eq!(snapshot.variables[&15], Value::Integer(99));
}

#[test]
fn assignments_collect_config_groups_and_preserve_array_null_members() {
    let group = |children| json!({"$":"STU_06610EBC","m_configVars":children,"m_EE729DCB":0});
    let graphs = store(vec![graph(
        1,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_inPlug")], &[])}),
            ),
            node(
                1,
                "STUStatescriptActionSetVar",
                None,
                json!({
                    "m_out_Var":var(42,true), "m_value":group(Json::Null),
                    "m_outPlug":plug(&[(2,"m_inPlug")], &[])
                }),
            ),
            node(
                2,
                "STUStatescriptActionSetVar",
                None,
                json!({
                    "m_out_Var":var(43,true),
                    "m_value":group(json!([null, {"$":"STUConfigVarInt","m_value":7}, group(json!([var(44,true)]))]))
                }),
            ),
        ],
        json!([]),
        json!([0]),
    )]);
    let snapshot = initialize(
        &graphs,
        42,
        Variables::from([(44, Value::Array(vec![Value::Null, Value::Integer(9)]))]),
        vec![Root::new(1, 1)],
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    assert_eq!(snapshot.variables[&42], Value::Array(Vec::new()));
    assert_eq!(
        snapshot.variables[&43],
        Value::Array(vec![Value::Integer(7), Value::Null, Value::Integer(9)])
    );
}

#[test]
fn encounter_base_action_forwards_and_clear_replaces_a_scoped_array() {
    let graphs = store(vec![graph(
        0x0c8c,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_inPlug")], &[])}),
            ),
            node(
                1,
                "STU_D38D0035",
                None,
                json!({"m_outPlug":plug(&[(2,"m_inPlug")], &[])}),
            ),
            node(
                2,
                "STU_1C2CC6D5",
                None,
                json!({"m_26E3B366":var(42,false),"m_outPlug":plug(&[(3,"m_inPlug")], &[])}),
            ),
            node(
                3,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var":var(43,true),"m_value":var(42,false)}),
            ),
        ],
        json!([]),
        json!([0]),
    )]);
    let populated = Value::Array(vec![Value::Integer(9)]);
    let mut root = Root::new(1, 0x0c8c);
    root.variables.insert(42, populated.clone());
    let snapshot = initialize(
        &graphs,
        42,
        Variables::from([(42, populated.clone())]),
        vec![root],
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    assert_eq!(snapshot.variables[&42], populated);
    assert_eq!(
        snapshot.instances[&1].variables[&42],
        Value::Array(Vec::new())
    );
    assert_eq!(snapshot.variables[&43], Value::Array(Vec::new()));
}

#[test]
fn unsupported_branches_are_reported_and_cyclic_actions_are_bounded() {
    let diagnostics = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({
                    "m_outPlug":plug(&[(1,"m_inPlug"),(2,"m_beginPlug"),(4,"m_beginPlug"),(5,"m_beginPlug")], &[])
                }),
            ),
            node(
                1,
                "UnknownAction",
                None,
                json!({"m_outPlug":plug(&[(3,"m_inPlug")], &[])}),
            ),
            node(2, "STUStatescriptState", Some(0), json!({})),
            node(
                3,
                "STUStatescriptActionSetVar",
                None,
                json!({
                    "m_out_Var":var(42, true), "m_value":true
                }),
            ),
            node(4, "STUStatescriptStateAnimatedCamera", Some(1), json!({})),
            node(5, "UnknownState", Some(2), json!({})),
            node(6, "UnreachedState", Some(3), json!({})),
        ],
        json!([2, 4, 5, 6]),
        json!([0]),
    );
    let cycle = graph(
        43,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_inPlug")], &[])}),
            ),
            node(
                1,
                "STUStatescriptActionSetVar",
                None,
                json!({
                    "m_out_Var":var(1, false), "m_value":true, "m_outPlug":plug(&[(1,"m_inPlug")], &[])
                }),
            ),
        ],
        json!([]),
        json!([0]),
    );
    let graphs = store(vec![diagnostics, cycle]);
    let run = |graph, budget| {
        initialize(
            &graphs,
            42,
            Variables::new(),
            vec![Root::new(1, graph)],
            &mut EmptyWorld,
            budget,
        )
    };
    let snapshot = run(42, MAX_INIT_WORK).unwrap();
    assert!(!snapshot.initialization_complete());
    let states = &snapshot.instances[&1].states;
    assert!(states[&0].active);
    assert_eq!(states[&0].kind, StateKind::Plain);
    assert!(!snapshot.variables.contains_key(&42));
    assert_eq!(
        snapshot
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.site.node, diagnostic.registration_rejected))
            .collect::<Vec<_>>(),
        [(Some(1), false), (Some(4), true), (Some(5), false)]
    );
    assert_eq!(snapshot.diagnostics[0].site.node, Some(1));
    assert_eq!(
        snapshot.diagnostics[0].issue,
        Issue::UnsupportedAction("UnknownAction".into())
    );
    assert!(matches!(
        run(43, 9),
        Err(InitFault::WorkBudget { limit: 9, .. })
    ));
    assert!(matches!(run(43, MAX_INIT_WORK), Err(InitFault::PlugDepth)));
}

#[test]
fn effect_action_continues_its_output_without_server_state() {
    let run = |dynamic: Json| {
        let fixture = graph(
            42,
            vec![
                node(
                    0,
                    "STUStatescriptEntry",
                    None,
                    json!({"m_outPlug":plug(&[(1,"m_inPlug")], &[])}),
                ),
                node(
                    1,
                    "STUStatescriptActionEffect",
                    None,
                    json!({
                        "m_effect":{"$":"STUConfigVarEffect","m_effect":"0x0300000000000001"},
                        "m_dataFlow":[{"$":"STU_13D35171","m_answer":var(7, false)}],
                        "m_37B02425":dynamic,
                        "m_outPlug":plug(&[(2,"m_inPlug")], &[]),
                    }),
                ),
                node(
                    2,
                    "STUStatescriptActionSetVar",
                    None,
                    json!({"m_out_Var":var(100, true), "m_value":true}),
                ),
            ],
            json!([]),
            json!([0]),
        );
        initialize(
            &store(vec![fixture]),
            42,
            Variables::new(),
            vec![Root::new(1, 42)],
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap()
    };
    let snapshot = run(Json::Null);
    assert!(snapshot.initialization_complete());
    assert_eq!(
        snapshot.variables,
        Variables::from([(100, Value::Bool(true))])
    );
    assert!(snapshot.instances[&1].variables.is_empty());
    assert!(snapshot.instances[&1].states.is_empty());

    let snapshot = run(var(9, true));
    assert!(snapshot.variables.is_empty());
    assert_eq!(snapshot.diagnostics.len(), 1);
    assert_eq!(snapshot.diagnostics[0].site.node, Some(1));
    assert_eq!(
        snapshot.diagnostics[0].issue,
        Issue::UnsupportedAction("STUStatescriptActionEffect".into())
    );
}

#[test]
fn criteria_actions_continue_their_output() {
    let fixture = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_inPlug")], &[])}),
            ),
            node(
                1,
                "STUStatescriptActionCriteria",
                None,
                json!({
                    "m_identifier":{"$":"STUConfigVarIdentifier","m_identifier":"0x0D80000000000001"},
                    "m_targets":{"$":"test-unanswered"},
                    "m_outPlug":plug(&[(2,"m_inPlug")], &[]),
                }),
            ),
            node(
                2,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var":var(100, true), "m_value":true}),
            ),
        ],
        json!([]),
        json!([0]),
    );
    let snapshot = initialize(
        &store(vec![fixture]),
        42,
        Variables::new(),
        vec![Root::new(1, 42)],
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    assert_eq!(
        snapshot.variables,
        Variables::from([(100, Value::Bool(true))])
    );
    assert!(snapshot.instances[&1].states.is_empty());
}

pub(super) fn shared_unbound_loadout_action_contract() {
    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../../../fixtures/behavior/unbound-loadout-action-v1.json"
    ))
    .unwrap();
    assert_eq!(fixture["class"], "STU_545AB3B8");
    for case in fixture["cases"].as_array().unwrap() {
        let has_output = case["out_plug_present"].as_bool().unwrap();
        let graphs = Arc::new(store(vec![graph(
            42,
            vec![
                node(0, "STUStatescriptEntry", None, json!({})),
                node(
                    1,
                    "STU_545AB3B8",
                    None,
                    json!({
                        "m_target":{"$":"unsupported-if-evaluated"},
                        "m_loadout":{"$":"unsupported-if-evaluated"},
                        "m_0B897113":{"$":"unsupported-if-evaluated"},
                        "m_outPlug":plug(if has_output { &[(2,"m_inPlug")][..] } else { &[] }, &[]),
                    }),
                ),
                node(
                    2,
                    "STUStatescriptActionSetVar",
                    None,
                    json!({"m_out_Var":var(100,true),"m_value":true}),
                ),
            ],
            json!([]),
            json!([]),
        )]));
        let mut component = Component::start(
            graphs,
            42,
            Variables::new(),
            vec![Root::listener(1, 42, None)],
            0,
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap();
        for _ in 0..2 {
            component
                .set_entity_variable(100, Value::Bool(false), &mut EmptyWorld)
                .unwrap();
            component
                .run(&mut EmptyWorld, |vm| {
                    let action = vm.graph(1).node(1).unwrap();
                    assert_eq!(
                        vm.run_node(1, action, None, 0)?,
                        case["return"].as_u64() == Some(1)
                    );
                    vm.run()
                })
                .unwrap();
            let snapshot = component.snapshot().unwrap();
            assert!(
                snapshot.initialization_complete(),
                "{:?}",
                snapshot.diagnostics
            );
            assert_eq!(snapshot.variables[&100], Value::Bool(has_output));
            assert!(snapshot.instances[&1].states.is_empty());
        }
    }
}

#[test]
fn payload_free_header_action_passes_through_like_the_reference_fallback() {
    let authored = json!({
        "m_2BBEEAB8":0, "m_8BF03679":null, "m_A2287776":0, "m_ADEB6E05":1,
        "m_AED90719":0, "m_BF5B22B7":null,
        "m_inPlug":{"$":"STUStatescriptInputPlug","links":[]},
        "m_outPlug":{"$":"STUStatescriptOutputPlug","links":[]},
    });
    let run = |fields: &Json, state: Json, remote: i32| {
        // The helper sets the header flags; the authored values replace them.
        let mut action = node(1, "STU_E9694EFD", None, json!({}));
        action["fields"] = fields.clone();
        action["state"] = state;
        action["remote"] = json!(remote);
        let fixture = graph(
            42,
            vec![
                node(
                    0,
                    "STUStatescriptEntry",
                    None,
                    json!({"m_outPlug":plug(&[(1,"m_inPlug"),(2,"m_inPlug")], &[])}),
                ),
                action,
                node(
                    2,
                    "STUStatescriptActionSetVar",
                    None,
                    json!({"m_out_Var":var(100, true), "m_value":true}),
                ),
            ],
            json!([]),
            json!([0]),
        );
        initialize(
            &store(vec![fixture]),
            42,
            Variables::new(),
            vec![Root::new(1, 42)],
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap()
    };
    let snapshot = run(&authored, Json::Null, -1);
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    assert_eq!(
        snapshot.variables,
        Variables::from([(100, Value::Bool(true))])
    );

    let mut linked = authored.clone();
    linked["m_outPlug"]["links"] = json!([[2, "m_inPlug"]]);
    let mut operand = authored.clone();
    operand["m_value"] = json!({"$":"STUConfigVarBool","m_value":true});
    let mut synced = authored.clone();
    synced["m_8BF03679"] = json!([]);
    for (fields, state, remote) in [
        (&linked, Json::Null, -1),
        (&operand, Json::Null, -1),
        (&synced, Json::Null, -1),
        (&authored, json!(0), -1),
        (&authored, Json::Null, 0),
    ] {
        let snapshot = run(fields, state.clone(), remote);
        assert_eq!(
            snapshot.diagnostics.len(),
            1,
            "{fields} state {state} remote {remote}"
        );
        assert_eq!(snapshot.diagnostics[0].site.node, Some(1));
        assert_eq!(
            snapshot.diagnostics[0].issue,
            Issue::UnsupportedAction("STU_E9694EFD".into())
        );
    }
}

#[test]
fn animation_without_an_authored_duration_stays_on_without_a_timer() {
    let run = |overrides: Json| {
        let mut fields = json!({
            "m_86701C3E":null,
            "m_154DBA91":null,
            "m_A3B86BB0":{"$":"STUStatescriptOutputPlug","links":[]},
            "m_onFinishedPlug":{"$":"STUStatescriptOutputPlug","links":[]},
            "m_onEndPlug":plug(&[(2,"m_inPlug")], &[]),
        });
        for (name, value) in overrides.as_object().unwrap() {
            fields[name] = value.clone();
        }
        let fixture = graph(
            42,
            vec![
                node(
                    0,
                    "STUStatescriptEntry",
                    None,
                    json!({"m_outPlug":plug(&[(1,"m_beginPlug")], &[])}),
                ),
                node(1, "STUStatescriptStateAnim", Some(0), fields),
                node(
                    2,
                    "STUStatescriptActionSetVar",
                    None,
                    json!({"m_out_Var":var(100, true), "m_value":true}),
                ),
            ],
            json!([1]),
            json!([0]),
        );
        Component::start(
            Arc::new(store(vec![fixture])),
            42,
            Variables::new(),
            vec![Root::new(1, 42)],
            100,
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap()
    };
    let mut component = run(json!({}));
    for frame in 101..=1000 {
        component.advance_frame(frame, &mut EmptyWorld).unwrap();
    }
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    assert_eq!(snapshot.instances[&1].states[&0].kind, StateKind::Anim);
    assert_eq!(active(snapshot, 1), [0]);
    assert!(snapshot.pending_timers.is_empty());
    assert!(snapshot.variables.is_empty());

    // A consumer of the animation's end or a client removal flag would
    // need the asset's length.
    for rejected in [
        json!({"m_154DBA91":{"$":"STUConfigVarBool","m_value":true}}),
        json!({"m_A3B86BB0":{"$":"STUStatescriptOutputPlug","links":[[2,"m_inPlug"]]}}),
        json!({"m_A3B86BB0":null}),
        json!({"m_onFinishedPlug":{"$":"STUStatescriptOutputPlug","links":[[2,"m_inPlug"]]}}),
        json!({"m_onFinishedPlug":null}),
    ] {
        let component = run(rejected.clone());
        let snapshot = component.snapshot().unwrap();
        assert_eq!(snapshot.diagnostics.len(), 1, "{rejected}");
        assert_eq!(snapshot.diagnostics[0].site.node, Some(1));
        assert_eq!(
            snapshot.diagnostics[0].issue,
            Issue::UnsupportedField("animation asset duration")
        );
    }
}

#[test]
fn game_message_without_recipients_leaves_its_parameters_unevaluated() {
    struct Recipients(Value);

    impl World for Recipients {
        fn native(&mut self, _site: Site, config: &Json) -> Option<Value> {
            (config["$"] == "test-recipients").then(|| self.0.clone())
        }
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }
    }

    let message = json!({"$":"STUConfigVarGameMessage","m_gameMessage":"0x0240000000000001"});
    let fixture = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_inPlug")], &[])}),
            ),
            node(
                1,
                "STUStatescriptActionSendGameMessage",
                None,
                json!({
                    "m_gameMessage":message,
                    "m_targets":{"$":"test-recipients"},
                    "m_params":[{"m_B5051BCE":"0x0A40000000000123",
                        "m_value":{"$":"test-unanswered"}}],
                    "m_outPlug":plug(&[(2,"m_inPlug")], &[]),
                }),
            ),
            node(
                2,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var":var(100, true), "m_value":true}),
            ),
        ],
        json!([]),
        json!([0]),
    );
    let graphs = store(vec![fixture]);
    let run = |targets| {
        initialize(
            &graphs,
            42,
            Variables::new(),
            vec![Root::new(1, 42)],
            &mut Recipients(targets),
            MAX_INIT_WORK,
        )
        .unwrap()
    };
    for targets in [
        Value::Null,
        Value::Entity(0),
        Value::Array(vec![
            Value::Null,
            Value::Entity(0),
            Value::Entity(0x1fff_ffff),
        ]),
    ] {
        let snapshot = run(targets);
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(
            snapshot.variables,
            Variables::from([(100, Value::Bool(true))])
        );
    }
    // A routed message still needs its unanswered parameter.
    let snapshot = run(Value::Entity(42));
    assert!(snapshot.variables.is_empty());
    assert_eq!(snapshot.diagnostics.len(), 1);
    assert_eq!(snapshot.diagnostics[0].site.node, Some(1));
    assert_eq!(
        snapshot.diagnostics[0].issue,
        Issue::Expression(EvalError::UnsupportedConfig("test-unanswered".into()))
    );
}

#[test]
fn queued_messages_abort_and_rearm_waits_without_crossing_instance_scope() {
    let message_node = |position, message, input| {
        node(
            position,
            "STUStatescriptEntryGameMessage",
            None,
            json!({
                "m_gameMessage": {"m_gameMessage":message},
                "m_params": [{"m_B5051BCE":"0x0A40000000000004","m_out_Var":var(4,false)}],
                "m_outPlug": plug(&[(1,input)],&[])
            }),
        )
    };
    let timed = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({
                    "m_outPlug":plug(&[(1,"m_beginPlug"),(5,"m_beginPlug")],&[])
                }),
            ),
            node(
                1,
                "STUStatescriptStateWait",
                Some(0),
                json!({
                    "m_timeout":{"$":"STUConfigVarFloat","m_value":0.02},
                    "m_0DBAFD7F":1,
                    "m_onFinishedPlug":plug(&[(2,"m_inPlug")],&[]),
                    "m_onAbortPlug":plug(&[(3,"m_inPlug")],&[]),
                }),
            ),
            node(
                2,
                "STUStatescriptActionSetVar",
                None,
                json!({
                    "m_out_Var":var(10,false), "m_value": true
                }),
            ),
            node(
                3,
                "STUStatescriptActionSetVar",
                None,
                json!({
                    "m_out_Var":var(10,false), "m_value": false
                }),
            ),
            message_node(4, "0x0240000000000001", "m_beginPlug"),
            node(
                5,
                "STU_D74D4F47",
                Some(1),
                json!({
                    "m_3016B9A1":{"$":"STUConfigVarInt","m_value":1},
                    "m_onFinishedPlug":plug(&[(6,"m_inPlug")],&[]),
                }),
            ),
            node(
                6,
                "STUStatescriptActionSetVar",
                None,
                json!({
                    "m_out_Var":var(11,false), "m_value": true
                }),
            ),
            message_node(7, "0x0240000000000002", ABORT_PLUG),
        ],
        json!([1, 5]),
        json!([0, 4, 7]),
    );
    let mut component = Component::start(
        Arc::new(store(vec![timed])),
        42,
        Variables::new(),
        vec![Root::new(7, 42), Root::new(1, 42)],
        1,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    let timer = |due_ms| PendingTimer {
        due_ms,
        raw_state: 0,
        finish: false,
        parameter: 1,
    };
    let snapshot = component.snapshot().unwrap();
    assert!(snapshot.initialization_complete());
    assert_eq!(snapshot.pending_timers[&7], [timer(36)]);
    assert_eq!(snapshot.pending_timers[&1], [timer(36)]);
    assert_eq!(
        snapshot.instances[&7].states[&1].kind,
        StateKind::WaitFrames { target: 2 }
    );

    component.advance_frame(1, &mut EmptyWorld).unwrap();
    assert_eq!(component.now_ms(), 31);
    assert_eq!(
        component
            .queue_message(
                7,
                0x0240_0000_0000_0001,
                Variables::from([(4, Value::Integer(15))])
            )
            .unwrap(),
        1
    );
    component.run_now(&mut EmptyWorld).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert_eq!(snapshot.pending_timers[&7], [timer(51)]);
    assert_eq!(snapshot.pending_timers[&1], [timer(36)]);
    assert_eq!(snapshot.instances[&7].variables[&4], Value::Integer(15));
    assert!(!snapshot.instances[&1].variables.contains_key(&4));
    assert_eq!(snapshot.instances[&7].states[&0].activation, 1);
    assert_eq!(snapshot.instances[&7].states[&0].start_ms, 31);
    assert!(matches!(
        component.advance_frame(1, &mut EmptyWorld),
        Err(InitFault::FrameOrder { .. })
    ));
    component.advance_frame(2, &mut EmptyWorld).unwrap();
    assert_eq!(
        component
            .queue_message(7, 0x0240_0000_0000_0002, Variables::new())
            .unwrap(),
        1
    );
    component.run_now(&mut EmptyWorld).unwrap();
    component.advance_frame(3, &mut EmptyWorld).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert!(snapshot.pending_timers.is_empty());
    assert_eq!(snapshot.instances[&7].variables[&10], Value::Bool(false));
    assert_eq!(snapshot.instances[&1].variables[&10], Value::Bool(true));
    assert_eq!(snapshot.instances[&7].variables[&11], Value::Bool(true));
    assert_eq!(snapshot.instances[&1].variables[&11], Value::Bool(true));

    for _ in 0..=MAX_EVENTS_PER_UPDATE {
        component
            .queue_message(7, 0x0240_0000_0000_0001, Variables::new())
            .unwrap();
    }
    assert_eq!(
        component.run_now(&mut EmptyWorld),
        Err(InitFault::EventBudget { instance: 7 })
    );
    assert!(component.snapshot().is_none());
    assert_eq!(component.run_now(&mut EmptyWorld), Err(InitFault::Halted));
}
