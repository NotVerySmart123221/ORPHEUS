use super::*;

const ENTER: u64 = 0x0240_0000_0000_8231;

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
