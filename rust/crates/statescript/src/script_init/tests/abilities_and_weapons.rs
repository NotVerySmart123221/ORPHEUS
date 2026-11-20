use super::*;
use crate::{
    script_expr::{EntityRelation, Scope},
    script_modifiers::Operation,
};

#[test]
fn ordered_stack_entries_use_the_value_below_and_restore_the_hidden_base() {
    let message = |position, message, target| {
        node(
            position,
            "STUStatescriptEntryGameMessage",
            None,
            json!({
                "m_gameMessage":{"m_gameMessage":message},
                "m_outPlug":plug(&[(target,ABORT_PLUG)],&[])
            }),
        )
    };
    let expression = json!({"$":"STU_919FD47C","m_expression":{
        "m_opcodes":[8,0,6,0,23,0],
        "m_dynamicVars":[var(100,true)],"m_D99EF254":[1]
    }});
    let fixture = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_beginPlug"),(2,"m_beginPlug")],&[])}),
            ),
            node(
                1,
                "STUStatescriptStateStack",
                Some(0),
                json!({
                    "m_out_Var":var(100,true), "m_value":10,
                    "m_priority":{"m_priority":"0x0C40000000000001"}
                }),
            ),
            node(
                2,
                "STUStatescriptStateStack",
                Some(1),
                json!({
                    "m_out_Var":var(100,true), "m_value":expression
                }),
            ),
            message(3, "0x0240000000000001", 1),
            message(4, "0x0240000000000002", 2),
        ],
        json!([1, 2]),
        json!([0, 3, 4]),
    );
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
    let variable = Variable {
        scope: Scope::Entity,
        id: 100,
    };
    assert_eq!(
        component.resolved_variable(1, variable),
        Some(Value::Float(11.0))
    );
    let snapshot = component.snapshot().unwrap();
    assert_eq!(snapshot.variables[&100], Value::Integer(5));
    assert_eq!(
        snapshot.bindings[&(0, 100)],
        [
            StackBinding {
                instance: 1,
                raw_state: 0,
                slot: 0,
                priority: StackPriority {
                    level: 0,
                    above_equals: false
                }
            },
            StackBinding {
                instance: 1,
                raw_state: 1,
                slot: 0,
                priority: StackPriority {
                    level: 0,
                    above_equals: true
                }
            }
        ]
    );
    assert_eq!(snapshot.resolved_bindings[&(0, 100)], Value::Float(11.0));
    assert_eq!(
        snapshot.instances[&1].states[&0].kind,
        StateKind::Stack {
            top: false,
            under: true
        }
    );
    assert_eq!(
        snapshot.instances[&1].states[&1].kind,
        StateKind::Stack {
            top: true,
            under: false
        }
    );
    component
        .set_entity_variable(100, Value::Integer(7), &mut EmptyWorld)
        .unwrap();
    component.run_now(&mut EmptyWorld).unwrap();
    assert_eq!(
        component.resolved_variable(1, variable),
        Some(Value::Float(11.0))
    );
    for (message, expected) in [(1, Value::Float(8.0)), (2, Value::Integer(7))] {
        component
            .queue_message(1, 0x0240_0000_0000_0000 | message, Variables::new())
            .unwrap();
        component.run_now(&mut EmptyWorld).unwrap();
        assert_eq!(component.resolved_variable(1, variable), Some(expected));
    }
    assert!(component.snapshot().unwrap().bindings.is_empty());
}

pub(super) fn shared_stack_admission_contract() {
    let contract: Json = serde_json::from_str(include_str!(
        "../../../../../../fixtures/behavior/statescript-stack-admission-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "statescript.stack-admission.v1");
    let priority =
        script_expr::stack_priority(&json!({"m_priority":contract["input"]["candidate"]})).unwrap();
    for case in contract["input"]["cases"].as_array().unwrap() {
        let mut nodes = vec![node(1, "STUStatescriptState", Some(0), json!({}))];
        let mut starts = vec![(1, "m_beginPlug")];
        let mut stops = Vec::new();
        let mut states = vec![1];
        for (i, priority) in case["priorities"].as_array().unwrap().iter().enumerate() {
            let pos = u16::try_from(i + 2).unwrap();
            nodes.push(node(
                pos,
                "STUStatescriptStateStack",
                Some(pos - 1),
                json!({
                    "m_out_Var":var(900,true),"m_value":case["values"][i],
                    "m_priority":{"m_priority":priority}
                }),
            ));
            starts.push((pos, "m_beginPlug"));
            stops.push((pos, "m_F198FD3A"));
            states.push(pos);
        }
        nodes.push(node(
            0,
            "STUStatescriptEntry",
            None,
            json!({"m_outPlug":plug(&starts,&[])}),
        ));
        let message = u16::try_from(nodes.len()).unwrap();
        nodes.push(node(
            message,
            "STUStatescriptEntryGameMessage",
            None,
            json!({
                "m_gameMessage":{"m_gameMessage":"0x0240000000000001"}, "m_outPlug":plug(&stops,&[])
            }),
        ));
        let model = graph(42, nodes, json!(states), json!([0, message]));
        let mut component = Component::start(
            Arc::new(store(vec![model])),
            42,
            Variables::new(),
            vec![Root::new(1, 42)],
            0,
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap();
        if case["release"] == true {
            component
                .queue_message(1, 0x0240_0000_0000_0001, Variables::new())
                .unwrap();
            component.run_now(&mut EmptyWorld).unwrap();
        }
        assert!(component.snapshot().unwrap().initialization_complete());
        let expected = case["expected"].as_bool().unwrap();
        assert_eq!(
            component.permits_entity_stack(900, priority),
            Some(expected),
            "{}",
            case["name"]
        );
        let query = json!({"$":"STU_3ACE35FB","m_E50CF556":var(900,true),"m_012B7AF7":{"m_priority":contract["input"]["candidate"]}});
        assert_eq!(
            component
                .evaluate_state_config(1, 0, &query, &mut EmptyWorld)
                .unwrap(),
            Value::Bool(expected)
        );
    }
}

pub(super) fn shared_ability_press_window_contract() {
    let contract: Json = serde_json::from_str(include_str!(
        "../../../../../../fixtures/behavior/statescript-ability-press-window-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "statescript.ability-press-window.v1");
    for case in contract["input"]["cases"].as_array().unwrap() {
        let mut model = graph(
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
                    "STUStatescriptStateAbility",
                    Some(0),
                    json!({
                        "m_A5F1A77B":36, "m_246438AD":contract["input"]["window_seconds"],
                        "m_150F0D92":var(900,true),
                        "m_EF3C5C73":[{"m_out_Var":var(902,true),"m_value":true}]
                    }),
                ),
            ],
            json!([1]),
            json!([0]),
        );
        model["fields"]["m_680A2CB2"] = json!([0]);
        let mut component = Component::start(
            Arc::new(store(vec![model])),
            42,
            Variables::from([(900, Value::Bool(false)), (902, Value::Bool(false))]),
            vec![Root::new(1, 42)],
            0,
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap();
        let mut observed = Vec::new();
        for frame in 1..=contract["input"]["frames"].as_u64().unwrap() as u32 {
            if u64::from(frame) == case["ready_frame"].as_u64().unwrap() {
                component
                    .set_entity_variable(900, Value::Bool(true), &mut EmptyWorld)
                    .unwrap();
            }
            component
                .advance_command(
                    Command {
                        frame,
                        buttons: if case["press_frames"]
                            .as_array()
                            .unwrap()
                            .contains(&json!(frame))
                        {
                            16
                        } else {
                            0
                        },
                        ..Default::default()
                    },
                    &mut EmptyWorld,
                )
                .unwrap();
            assert!(component.snapshot().unwrap().initialization_complete());
            if contract["input"]["sample_frames"]
                .as_array()
                .unwrap()
                .contains(&json!(frame))
            {
                observed.push(component.snapshot().unwrap().variables[&902] == Value::Bool(true));
            }
        }
        assert_eq!(json!(observed), case["expected"], "{}", case["name"]);
    }
}

#[test]
fn control_block_interrupts_authored_channels_releases_buttons_and_recovers() {
    let mut fixture = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_beginPlug"),(2,"m_beginPlug")],&[])}),
            ),
            node(
                1,
                "STUStatescriptStateAbility",
                Some(0),
                json!({
            "m_out_CooldownVar":var(60,true),"m_A5F1A77B":35,
            "m_EF3C5C73":[{"m_out_Var":var(62,true),"m_value":true}],
            "m_19899EAC":[{"m_out_Var":var(62,true),"m_value":false}]}),
            ),
            node(
                2,
                "STUStatescriptStateLogicalButton",
                Some(1),
                json!({
            "m_logicalButton":{"$":"STUConfigVarLogicalButton","m_logicalButton":20}}),
            ),
        ],
        json!([1, 2]),
        json!([0]),
    );
    fixture["fields"]["m_680A2CB2"] = json!([0, 1]);
    let mut component = Component::start(
        Arc::new(store(vec![fixture])),
        42,
        Variables::from([(3652, Value::Float(1.0)), (60, Value::Float(0.0))]),
        vec![Root::new(1, 42)],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    let command = |frame| Command {
        frame,
        buttons: 8 | 1,
        ..Default::default()
    };
    component
        .advance_command(command(1), &mut EmptyWorld)
        .unwrap();
    assert_eq!(
        component.snapshot().unwrap().variables[&62],
        Value::Bool(true)
    );
    assert_eq!(
        component.snapshot().unwrap().instances[&1].states[&1].kind,
        StateKind::LogicalButton { down: true }
    );
    component
        .set_control_blocked(true, &mut EmptyWorld)
        .unwrap();
    assert_eq!(
        component.snapshot().unwrap().variables[&62],
        Value::Bool(false)
    );
    assert_eq!(
        component.snapshot().unwrap().instances[&1].states[&1].kind,
        StateKind::LogicalButton { down: false }
    );
    assert!(component.machine.held_buttons.is_empty());
    for frame in 2..=20 {
        component
            .advance_command(command(frame), &mut EmptyWorld)
            .unwrap();
        assert_eq!(
            component.snapshot().unwrap().variables[&62],
            Value::Bool(false)
        );
        assert!(component.machine.held_buttons.is_empty());
    }
    component
        .set_control_blocked(false, &mut EmptyWorld)
        .unwrap();
    component
        .advance_command(command(21), &mut EmptyWorld)
        .unwrap();
    assert_eq!(
        component.snapshot().unwrap().variables[&62],
        Value::Bool(true)
    );
    assert_eq!(
        component.snapshot().unwrap().instances[&1].states[&1].kind,
        StateKind::LogicalButton { down: true }
    );
    assert!(component.snapshot().unwrap().initialization_complete());
}

#[test]
fn ability_cooldown_restarts_and_stack_displacement_stops_the_running_ability() {
    let message = |position, message, target, input| {
        node(
            position,
            "STUStatescriptEntryGameMessage",
            None,
            json!({
                "m_gameMessage":{"m_gameMessage":message},
                "m_outPlug":plug(&[(target, input)], &[]),
            }),
        )
    };
    let mut fixture = graph(
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
                "STUStatescriptStateAbility",
                Some(0),
                json!({
                    "m_out_CooldownVar":var(60,true),
                    "m_out_StackVar":var(61,true),
                    "m_5E561E77":{"m_priority":"0x0C40000000000001"},
                    "m_A5F1A77B":35,
                    "m_EF3C5C73":[{"m_out_Var":var(60,true), "m_value":0.1},
                                  {"m_out_Var":var(62,true), "m_value":true}],
                    "m_19899EAC":[{"m_out_Var":var(62,true), "m_value":false}]
                }),
            ),
            node(
                2,
                "STUStatescriptStateStack",
                Some(1),
                json!({
                    "m_out_Var":var(61,true),
                    "m_value":7,
                    "m_priority":{"m_priority":"0x0C40000000000003"}
                }),
            ),
            message(3, "0x0240000000000001", 2, "m_beginPlug"),
            message(4, "0x0240000000000002", 1, ABORT_PLUG),
        ],
        json!([1, 2]),
        json!([0, 3, 4]),
    );
    fixture["fields"]["m_680A2CB2"] = json!([0]);
    let mut component = Component::start(
        Arc::new(store(vec![fixture])),
        42,
        Variables::from([(3652, Value::Float(1.0)), (60, Value::Float(0.032))]),
        vec![Root::new(1, 42)],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    let timer = |component: &Component| {
        component
            .snapshot()
            .unwrap()
            .pending_timers
            .get(&1)
            .and_then(|timers| timers.first().copied())
    };
    assert_eq!(
        timer(&component),
        Some(PendingTimer {
            due_ms: 16,
            raw_state: 0,
            finish: false,
            parameter: 2
        })
    );
    for frame in 0..=1 {
        component
            .advance_command(
                Command {
                    frame,
                    buttons: 8,
                    ..Command::default()
                },
                &mut EmptyWorld,
            )
            .unwrap();
    }
    let snapshot = component.snapshot().unwrap();
    assert_eq!(snapshot.variables[&62], Value::Bool(true));
    assert_eq!(
        snapshot.instances[&1].states[&0].kind,
        StateKind::Ability { flags: 7 }
    );
    assert_eq!(snapshot.resolved_bindings[&(0, 61)], Value::Bool(true));
    assert_eq!(snapshot.bindings[&(0, 61)][0].slot, 1);
    assert_eq!(timer(&component).map(|timer| timer.due_ms), Some(66));
    let remaining = snapshot.resolved_bindings[&(0, 60)].clone();
    for message in [1, 2] {
        component
            .queue_message(1, 0x0240_0000_0000_0000 | message, Variables::new())
            .unwrap();
        component.run_now(&mut EmptyWorld).unwrap();
    }
    let snapshot = component.snapshot().unwrap();
    assert_eq!(snapshot.variables[&62], Value::Bool(false));
    assert_eq!(snapshot.variables[&60], remaining);
    assert_eq!(snapshot.resolved_bindings[&(0, 61)], Value::Integer(7));
    assert_eq!(snapshot.bindings[&(0, 61)][0].slot, 0);
    assert!(!snapshot.instances[&1].states[&0].active);
    assert!(timer(&component).is_none());
}

#[test]
fn switch_exits_old_branch_and_active_query_does_not_begin_its_target() {
    let set = |position, variable, value| {
        node(
            position,
            "STUStatescriptActionSetVar",
            None,
            json!({
                "m_out_Var":var(variable,true), "m_value":value,
            }),
        )
    };
    let fixture = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({
                    "m_outPlug":plug(&[(1,"m_beginPlug"),(2,"m_inPlug"),(3,"m_beginPlug")],&[])
                }),
            ),
            node(
                1,
                "STU_DEBD057B",
                Some(0),
                json!({
                    "m_262AE982":var(100,true),
                    "m_05706D1C":[
                        {"$":"Plug", "links":[[4,"m_beginPlug"]], "m_FF3DAF1E":[3], "m_value":2},
                        {"$":"Plug", "links":[[5,"m_beginPlug"]], "m_FF3DAF1E":[4], "m_value":2}
                    ],
                    "m_A5EF385D":plug(&[(6,"m_beginPlug")], &[5]),
                }),
            ),
            node(
                2,
                "STU_01178155",
                Some(1),
                json!({
                    "m_7273C81B":plug(&[(7,"m_inPlug")],&[]),
                    "m_C6F3744E":plug(&[(8,"m_inPlug")],&[]),
                }),
            ),
            node(
                3,
                "STUStatescriptStateGamePadVibration",
                Some(2),
                json!({
                    "m_onBeginPlug":plug(&[(9,"m_inPlug")],&[]),
                    "m_onFinishedPlug":plug(&[(10,"m_inPlug")],&[]),
                }),
            ),
            node(4, "STUStatescriptState", Some(3), json!({})),
            node(5, "STUStatescriptState", Some(4), json!({})),
            node(6, "STUStatescriptState", Some(5), json!({})),
            set(7, 101, 1),
            set(8, 101, 0),
            set(9, 102, 1),
            set(10, 102, 2),
            node(
                11,
                "STUStatescriptEntryGameMessage",
                None,
                json!({
                    "m_gameMessage":{"m_gameMessage":"0x0240000000000001"},
                    "m_outPlug":plug(&[(2,"m_beginPlug")],&[]),
                }),
            ),
            node(
                12,
                "STUStatescriptEntryGameMessage",
                None,
                json!({
                    "m_gameMessage":{"m_gameMessage":"0x0240000000000002"},
                    "m_outPlug":plug(&[(2,"m_inPlug")],&[]),
                }),
            ),
        ],
        json!([1, 2, 3, 4, 5, 6]),
        json!([0, 11, 12]),
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
    let initial = component.snapshot().unwrap();
    let states = &initial.instances[&1].states;
    assert_eq!(states[&0].kind, StateKind::Switch { case: None });
    assert!(!states[&1].active);
    assert_eq!(states[&1].activation, 0);
    assert!(!states[&2].active);
    assert!(states[&5].active);
    assert_eq!(initial.variables[&101], Value::Integer(0));
    assert_eq!(initial.variables[&102], Value::Integer(2));
    component
        .set_entity_variable(100, Value::Integer(2), &mut EmptyWorld)
        .unwrap();
    for message in [1, 2] {
        component
            .queue_message(1, 0x0240_0000_0000_0000 | message, Variables::new())
            .unwrap();
        component.run_now(&mut EmptyWorld).unwrap();
    }
    let updated = component.snapshot().unwrap();
    let states = &updated.instances[&1].states;
    assert_eq!(states[&0].kind, StateKind::Switch { case: Some(0) });
    assert!(states[&1].active);
    assert_eq!(states[&1].activation, 1);
    assert!(states[&3].active);
    assert!(!states.contains_key(&4));
    assert!(!states[&5].active);
    assert_eq!(updated.variables[&101], Value::Integer(1));
    assert!(updated.initialization_complete());
}

#[derive(Default)]
struct SprintWorld {
    forward: i8,
}

impl World for SprintWorld {
    fn native(&mut self, site: Site, config: &Json) -> Option<Value> {
        if site.graph != 0x0255 {
            return None;
        }
        match config.get("$")?.as_str()? {
            "STU_D3EB8D87" | "STU_A77AC2AC" => Some(Value::Bool(false)),
            "STU_310D6529" => {
                let unit = (1.0_f64 / 127.0) as f32;
                let forward = f64::from(f32::from(self.forward) * unit);
                Some(Value::Vector([0.0, forward, 0.0]))
            }
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

pub(super) fn authored_sprint_stack_gates_track_input_and_reveal_the_base_on_release() {
    let runtime =
        PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").expect("set ORPHEUS_RUNTIME_DIR"));
    let graphs = Arc::new(GraphStore::load(runtime.join("data/game_graphs_174.json.gz")).unwrap());
    let mut world = SprintWorld::default();
    let mut component = Component::start(
        graphs,
        0xa000_0101,
        Variables::new(),
        vec![Root::new(5, 0x0255)],
        1029,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    let var = |component: &Component, id| {
        component.resolved_variable(
            5,
            Variable {
                scope: Scope::Instance,
                id,
            },
        )
    };
    assert_eq!(var(&component, 9570), Some(Value::Bool(true)));
    for (frame, forward, buttons, throttle, held) in [
        (1030, 63, 0, Value::Null, Value::Null),
        (1031, 64, 0, Value::Bool(true), Value::Null),
        (1032, 127, 0x881, Value::Bool(true), Value::Bool(true)),
        (1033, 0, 0, Value::Null, Value::Null),
    ] {
        world.forward = forward;
        component
            .advance_command(
                Command {
                    frame,
                    forward,
                    buttons,
                    ..Command::default()
                },
                &mut world,
            )
            .unwrap();
        assert_eq!(var(&component, 2333), Some(throttle));
        assert_eq!(var(&component, 7681), Some(held));
    }
    component
        .set_entity_variable(9535, Value::Bool(false), &mut world)
        .unwrap();
    component.run_now(&mut world).unwrap();
    assert_eq!(var(&component, 9570), Some(Value::Null));
    assert!(
        !component
            .snapshot()
            .unwrap()
            .bindings
            .contains_key(&(5, 9570))
    );
}

pub(super) fn isolated_soldier_sprint_ability_matches_the_authored_toggle_and_modifier_states() {
    let runtime =
        PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").expect("set ORPHEUS_RUNTIME_DIR"));
    let graphs = Arc::new(GraphStore::load(runtime.join("data/game_graphs_174.json.gz")).unwrap());
    let mut world = SprintWorld::default();
    let mut component = Component::start(
        graphs,
        0xa000_0101,
        Variables::new(),
        vec![Root::new(5, 0x0255)],
        1029,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    let rows = [
        (1030, 0, 8, 0, None),
        (1031, 63, 8, 0, None),
        (1032, 64, 8, 0, None),
        (1033, 64, 8, 5, Some(1)),
        (1034, 127, 0, 1, Some(1)),
        (1035, 127, 8, 4, Some(0)),
        (1036, 127, 8, 4, Some(0)),
        (1037, 127, 0, 0, Some(0)),
        (1038, 127, 8, 5, Some(1)),
        (1039, 63, 0, 0, Some(0)),
        (1040, 127, 0, 0, Some(0)),
        (1041, 127, 9, 0, Some(0)),
    ];
    for (frame, forward, buttons, flags, hud) in rows {
        world.forward = forward;
        component
            .advance_command(
                Command {
                    frame,
                    forward,
                    buttons,
                    ..Command::default()
                },
                &mut world,
            )
            .unwrap();
        let snapshot = component.snapshot().unwrap();
        let instance = &snapshot.instances[&5];
        assert_eq!(
            instance.states[&1].kind,
            StateKind::Ability { flags },
            "frame {frame}"
        );
        assert_eq!(
            instance.variables.get(&699).cloned(),
            hud.map(Value::Integer),
            "frame {frame}"
        );
        for raw in [0, 20] {
            assert_eq!(
                instance.states.get(&raw).is_some_and(|state| state.active),
                flags & 1 != 0,
                "frame {frame}, state {raw}"
            );
        }
        // The activation passes the payload-free STU_E9694EFD action as
        // the reference's fallback does, so the body stays complete.
        assert!(
            snapshot.initialization_complete(),
            "frame {frame}: {:?}",
            snapshot.diagnostics
        );
        let projected = component.movement_modifiers(&mut world).unwrap().unwrap();
        let modifiers: Vec<_> = projected
            .iter()
            .map(|(key, modifier)| {
                (
                    key.state,
                    modifier.index,
                    modifier.operation,
                    modifier.value,
                    modifier.priority,
                )
            })
            .collect();
        if flags & 1 == 0 {
            assert!(modifiers.is_empty());
        } else {
            assert_eq!(
                modifiers,
                [
                    (0, 0, Operation::Add, 0.5, 0.0),
                    (20, 21, Operation::Set, 0.0, 0.0)
                ]
            );
        }
    }
}

#[test]
fn volley_reports_exact_times_and_bakes_linked_ammo_on_finish() {
    let weapon = graph(
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
                "STUStatescriptStateWeaponVolley",
                Some(0),
                json!({
                    "m_A17BE89B":3,
                    "m_numShotsPerSecond":9,
                    "m_24E351A4":var(53,false),
                    "m_3FFC9EB9":var(231,false),
                    "m_FEC435C6":180
                }),
            ),
        ],
        json!([1]),
        json!([0]),
    );
    let mut root = Root::new(1, 42);
    root.variables.insert(53, Value::Integer(30));
    root.variables.insert(231, Value::Float(0.25));
    let mut component = Component::start(
        Arc::new(store(vec![weapon])),
        42,
        Variables::new(),
        vec![root],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    let mut shots = Vec::new();
    for frame in 0..=14 {
        component.advance_frame(frame, &mut EmptyWorld).unwrap();
        shots.extend(component.take_shots().unwrap());
        if frame == 6 {
            let snapshot = component.snapshot().unwrap();
            assert!(snapshot.bindings.is_empty());
            assert_eq!(snapshot.instances[&1].variables[&53], Value::Integer(30));
            assert_eq!(snapshot.resolved_bindings[&(1, 53)], Value::Integer(29));
            assert_eq!(snapshot.resolved_bindings[&(1, 231)], Value::Float(0.25));
        }
    }
    assert_eq!(
        shots
            .iter()
            .map(|shot| (shot.frame, shot.time_ms, shot.ordinal))
            .collect::<Vec<_>>(),
        [(0, 0, 0), (6, 111, 1), (13, 222, 2)]
    );
    assert!(
        shots
            .iter()
            .all(|shot| shot.activation == 1 && shot.activation_ms == 0)
    );
    let node = component.shot_node(&shots[2]).unwrap();
    assert_eq!(
        component.evaluate_shot_config(
            &shots[2],
            node.field("m_FEC435C6").unwrap(),
            &mut EmptyWorld
        ),
        Ok(Value::Integer(180))
    );
    let snapshot = component.snapshot().unwrap();
    assert!(snapshot.pending_timers.is_empty());
    assert!(snapshot.resolved_bindings.is_empty());
    assert!(!snapshot.instances[&1].states[&0].active);
    assert_eq!(snapshot.instances[&1].variables[&53], Value::Integer(27));
}
#[test]
fn animation_callback_routes_a_typed_self_message_to_the_live_receiver() {
    let message = json!({"$":"STUConfigVarGameMessage","m_gameMessage":"0x0240000000000001"});
    let source = graph(
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
                "STUStatescriptStateAnim",
                Some(0),
                json!({
                    "m_86701C3E":{"$":"STUConfigVarFloat","m_value":0.025},
                    "m_A3B86BB0":plug(&[(2,"m_inPlug")],&[]),
                    "m_onFinishedPlug":plug(&[(3,"m_inPlug")],&[])
                }),
            ),
            node(
                2,
                "STUStatescriptActionSendGameMessage",
                None,
                json!({
                    "m_gameMessage":message,
                    "m_params":[{"m_B5051BCE":"0x0A40000000000123","m_value":true}]
                }),
            ),
            node(
                3,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var":var(101,false),"m_value":true}),
            ),
        ],
        json!([1]),
        json!([0]),
    );
    let receiver = graph(
        43,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_beginPlug")],&[])}),
            ),
            node(
                1,
                "STUStatescriptStateGameMessageEntry",
                Some(0),
                json!({
                    "m_gameMessage":message,
                    "m_33DA5163":plug(&[(2,"m_beginPlug")],&[1]),
                    "m_452862E6":plug(&[(3,"m_beginPlug")],&[2]),
                    "m_params":[{"m_B5051BCE":"0x0A40000000000123","m_out_Var":var(7,false)}]
                }),
            ),
            node(2, "STUStatescriptState", Some(1), json!({})),
            node(3, "STU_691BFA55", Some(2), json!({})),
        ],
        json!([1, 2, 3]),
        json!([0]),
    );
    let mut component = Component::start(
        Arc::new(store(vec![source, receiver])),
        42,
        Variables::new(),
        vec![Root::new(1, 42), Root::new(2, 43)],
        100,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    let initial = component.snapshot().unwrap();
    assert_eq!(initial.instances[&1].states[&0].kind, StateKind::Anim);
    assert_eq!(active(initial, 2), [0, 1]);
    assert_eq!(
        initial.pending_timers[&1],
        [
            PendingTimer {
                due_ms: 1625,
                raw_state: 0,
                finish: false,
                parameter: 1
            },
            PendingTimer {
                due_ms: 1625,
                raw_state: 0,
                finish: true,
                parameter: 0
            },
        ]
    );
    component.advance_frame(100, &mut EmptyWorld).unwrap();
    assert_eq!(active(component.snapshot().unwrap(), 2), [0, 1]);
    component.advance_frame(101, &mut EmptyWorld).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    assert_eq!(snapshot.instances[&1].variables[&101], Value::Bool(true));
    assert!(!snapshot.instances[&1].states[&0].active);
    assert_eq!(active(snapshot, 2), [0, 2]);
    assert_eq!(snapshot.instances[&2].variables[&7], Value::Bool(true));
    assert_eq!(snapshot.instances[&2].states[&2].kind, StateKind::Effect);
    assert_eq!(snapshot.instances[&2].states[&2].start_ms, 1625);
}

#[test]
fn logical_button_block_resyncs_until_last_owner_exits() {
    let button = json!({"$":"STUConfigVarLogicalButton","m_logicalButton":20});
    let message = |position, key, target, input| {
        node(
            position,
            "STUStatescriptEntryGameMessage",
            None,
            json!({
                "m_gameMessage":{"m_gameMessage":format!("0x024000000000{key:04X}")},
                "m_outPlug":plug(&[(target,input)],&[]),
            }),
        )
    };
    let mut listener = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptStateLogicalButton",
                Some(0),
                json!({"m_logicalButton":button}),
            ),
            node(
                1,
                "STUStatescriptStateLogicalButton",
                Some(1),
                json!({
                    "m_logicalButton":button,"m_53C4B6A6":true,
                }),
            ),
            node(
                2,
                "STU_E4A30BCF",
                Some(2),
                json!({"m_logicalButton":button}),
            ),
            node(
                3,
                "STU_7DEB522D",
                None,
                json!({
                    "m_logicalButton":button,
                    "m_truePlug":plug(&[(4,"m_inPlug")],&[]),
                    "m_falsePlug":plug(&[(5,"m_inPlug")],&[]),
                }),
            ),
            node(
                4,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var":var(42,false),"m_value":true}),
            ),
            node(
                5,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var":var(42,false),"m_value":false}),
            ),
            node(
                6,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(0,"m_beginPlug"),(1,"m_beginPlug")],&[])}),
            ),
            message(7, 1, 2, "m_beginPlug"),
            message(8, 2, 2, "m_F198FD3A"),
            message(9, 3, 3, "m_inPlug"),
        ],
        json!([0, 1, 2]),
        json!([6, 7, 8, 9]),
    );
    listener["fields"]["m_680A2CB2"] = json!([0, 1]);
    let other_block = graph(
        43,
        vec![
            node(
                0,
                "STU_E4A30BCF",
                Some(0),
                json!({"m_logicalButton":button}),
            ),
            message(1, 1, 0, "m_beginPlug"),
            message(2, 2, 3, "m_inPlug"),
            node(3, "STU_22926252", None, json!({})),
        ],
        json!([0]),
        json!([1, 2]),
    );
    let mut component = Component::start(
        Arc::new(store(vec![listener, other_block])),
        42,
        Variables::new(),
        vec![Root::new(1, 42), Root::new(2, 43)],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    component
        .advance_command(
            Command {
                frame: 1,
                buttons: 1,
                ..Command::default()
            },
            &mut EmptyWorld,
        )
        .unwrap();
    let send = |component: &mut Component, id, key: u64| {
        assert_eq!(
            component
                .queue_message(id, 0x0240_0000_0000_0000 | key, Variables::new())
                .unwrap(),
            1
        );
        component.run_now(&mut EmptyWorld).unwrap();
    };
    let check = |component: &mut Component, held| {
        send(component, 1, 3);
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        let instance = &snapshot.instances[&1];
        assert_eq!(instance.variables[&42], Value::Bool(held));
        assert_eq!(
            instance.states[&0].kind,
            StateKind::LogicalButton { down: held }
        );
        assert_eq!(
            instance.states[&1].kind,
            StateKind::LogicalButton { down: true }
        );
    };
    check(&mut component, true);
    send(&mut component, 1, 1);
    check(&mut component, false);
    send(&mut component, 2, 1);
    send(&mut component, 1, 2);
    check(&mut component, false);
    send(&mut component, 2, 2);
    assert!(!component.snapshot().unwrap().instances.contains_key(&2));
    check(&mut component, true);
}

#[test]
fn authored_button_lock_stacks_expires_and_removes_only_matching_buffered_presses() {
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Contract {
        schema_version: u8,
        case_id: String,
        evidence: String,
        input: Input,
        expected: Vec<Expected>,
    }

    #[derive(Deserialize)]
    struct Input {
        bundle: Json,
        steps: Vec<Step>,
    }

    #[derive(Deserialize)]
    #[serde(tag = "op", rename_all = "snake_case")]
    enum Step {
        Command { frame: u32, buttons: u32 },
        Lock { private: bool },
        Abort,
        Global { blocked: bool },
        Teardown,
    }

    #[derive(Deserialize)]
    struct Expected {
        held: Option<[bool; 2]>,
        owners: usize,
        buffered: Option<Vec<u8>>,
    }

    const PUBLIC: u16 = 1;
    const PUBLIC_GRAPH: u32 = 531;
    const PRIVATE_GRAPH: u32 = 532;
    const PUBLIC_LOCK_RAW: u16 = 3;
    const BUTTON: usize = 20;
    const ABORT: u64 = 0xf000_0000_0000_0069;
    let fixture: Contract = serde_json::from_str(include_str!(
        "../../../../../../fixtures/behavior/script-button-lock-v1.json"
    ))
    .unwrap();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.case_id, "script-button-lock.synthetic.v1");
    assert_eq!(fixture.evidence, "synthetic-rust");
    assert_eq!(fixture.input.steps.len(), fixture.expected.len());
    let graphs = Arc::new(
        GraphStore::from_json(&serde_json::to_vec(&fixture.input.bundle).unwrap()).unwrap(),
    );
    assert!(
        graphs
            .graph(PUBLIC_GRAPH)
            .unwrap()
            .node(3)
            .unwrap()
            .server_only
    );
    assert!(
        graphs
            .graph(PRIVATE_GRAPH)
            .unwrap()
            .node(0)
            .unwrap()
            .server_only
    );
    let mut component = Component::start(
        graphs,
        821,
        Variables::new(),
        vec![
            Root::new(PUBLIC, PUBLIC_GRAPH),
            Root::server_only(PRIVATE_GRAPH),
        ],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    let (&private, instance) = component
        .snapshot()
        .unwrap()
        .instances
        .iter()
        .find(|(_, instance)| instance.graph == PRIVATE_GRAPH)
        .unwrap();
    assert!(instance.is_server_only());
    let ability = StateKey {
        instance: PUBLIC,
        raw: 2,
    };

    for (index, (step, expected)) in fixture
        .input
        .steps
        .into_iter()
        .zip(fixture.expected)
        .enumerate()
    {
        match step {
            Step::Command { frame, buttons } => component
                .advance_command(
                    Command {
                        frame,
                        buttons,
                        ..Command::default()
                    },
                    &mut EmptyWorld,
                )
                .unwrap(),
            Step::Lock { private: isolated } => {
                let (instance, raw) = if isolated {
                    (private, 0)
                } else {
                    (PUBLIC, PUBLIC_LOCK_RAW)
                };
                component
                    .begin_state(instance, raw, &mut EmptyWorld)
                    .unwrap();
            }
            Step::Abort => {
                assert_eq!(
                    component
                        .queue_message(PUBLIC, ABORT, Variables::new())
                        .unwrap(),
                    1
                );
                component.run_now(&mut EmptyWorld).unwrap();
            }
            Step::Global { blocked } => component
                .set_control_blocked(blocked, &mut EmptyWorld)
                .unwrap(),
            Step::Teardown => component.teardown(&mut EmptyWorld).unwrap(),
        }
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.diagnostics.is_empty(),
            "step {index}: {:?}",
            snapshot.diagnostics
        );
        let held = snapshot.instances.get(&PUBLIC).map(|instance| {
            [0, 1].map(|state| {
                matches!(
                    instance.states[&state].kind,
                    StateKind::LogicalButton { down: true }
                )
            })
        });
        assert_eq!(held, expected.held, "step {index}");
        assert_eq!(
            component.machine.button_blocks[BUTTON], expected.owners,
            "step {index}"
        );
        if let Some(buffered) = expected.buffered {
            let actual: Vec<_> = component
                .machine
                .ability_presses
                .get(&ability)
                .into_iter()
                .flatten()
                .map(|press| press.button)
                .collect();
            assert_eq!(actual, buffered, "step {index}");
        }
        if expected.held.is_none() {
            assert!(snapshot.instances.is_empty());
            assert!(snapshot.pending_timers.is_empty());
        }
    }
}

#[test]
fn buffered_action_press_uses_the_shared_clock_and_releases_after_one_millisecond() {
    let mut fixture = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntryGameMessage",
                None,
                json!({
                    "m_gameMessage":{"m_gameMessage":"0x0240000000000001"},
                    "m_outPlug":plug(&[(1,"m_beginPlug")],&[]),
                }),
            ),
            node(
                1,
                "STUStatescriptStateLogicalButton",
                Some(0),
                json!({
                    "m_logicalButton":{"$":"STUConfigVarLogicalButton","m_logicalButton":15},
                    "m_246438AD":{"$":"STUConfigVarFloat","m_value":0.03},
                    "m_onGoingDownPlug":plug(&[(2,"m_inPlug")],&[]),
                    "m_onComingUpPlug":plug(&[(3,"m_inPlug")],&[]),
                }),
            ),
            node(
                2,
                "STUStatescriptActionSetVar",
                None,
                json!({
                    "m_out_Var":var(10,false), "m_value": true,
                }),
            ),
            node(
                3,
                "STUStatescriptActionSetVar",
                None,
                json!({
                    "m_out_Var":var(10,false), "m_value": false,
                }),
            ),
        ],
        json!([1]),
        json!([0]),
    );
    fixture["fields"]["m_680A2CB2"] = json!([0]);
    let graphs = Arc::new(store(vec![fixture]));
    for (dead, delay) in [(false, 0), (false, 2), (true, 0)] {
        let mut component = Component::start(
            Arc::clone(&graphs),
            42,
            Variables::new(),
            vec![Root::new(1, 42)],
            100,
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap();
        component.set_owner_dead(dead);
        component
            .advance_command(
                Command {
                    frame: 100,
                    action: 1,
                    ..Command::default()
                },
                &mut EmptyWorld,
            )
            .unwrap();
        if delay != 0 {
            component
                .advance_frame(100 + delay, &mut EmptyWorld)
                .unwrap();
        }
        let buffered = !dead && delay == 0;
        assert_eq!(
            component
                .queue_message(1, 0x0240_0000_0000_0001, Variables::new())
                .unwrap(),
            1
        );
        component.run_now(&mut EmptyWorld).unwrap();
        let snapshot = component.snapshot().unwrap();
        let instance = &snapshot.instances[&1];
        assert_eq!(instance.states[&0].activation, 1);
        assert_eq!(
            instance.states[&0].kind,
            StateKind::LogicalButton { down: buffered }
        );
        if buffered {
            assert_eq!(instance.variables[&10], Value::Bool(true));
            assert_eq!(
                snapshot.pending_timers[&1],
                [PendingTimer {
                    due_ms: 1616,
                    raw_state: 0,
                    finish: false,
                    parameter: 1
                }]
            );
            component.advance_frame(101, &mut EmptyWorld).unwrap();
            let snapshot = component.snapshot().unwrap();
            assert_eq!(
                snapshot.instances[&1].states[&0].kind,
                StateKind::LogicalButton { down: false }
            );
            assert_eq!(snapshot.instances[&1].variables[&10], Value::Bool(false));
            assert!(snapshot.pending_timers.is_empty());
        } else {
            assert!(instance.variables.get(&10).is_none());
            assert!(snapshot.pending_timers.is_empty());
        }
    }
}

pub(super) fn soldier_sprint_graph_reads_primary_crouch_and_melee_edges_as_in_python() {
    let runtime =
        PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").expect("set ORPHEUS_RUNTIME_DIR"));
    let graphs = Arc::new(GraphStore::load(runtime.join("data/game_graphs_174.json.gz")).unwrap());
    let mut component = Component::start(
        graphs,
        0xa000_0101,
        Variables::new(),
        vec![Root::new(5, 0x0255)],
        1029,
        &mut ReferenceWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    let sprint = |snapshot: &Snapshot| {
        [4, 7, 16].map(|raw| {
            (
                snapshot.instances[&5].states[&raw].kind,
                snapshot.instances[&5].states[&raw].activation,
            )
        })
    };
    let up = StateKind::LogicalButton { down: false };
    let down = StateKind::LogicalButton { down: true };
    assert_eq!(sprint(component.snapshot().unwrap()), [(up, 1); 3]);
    component
        .advance_command(
            Command {
                frame: 1030,
                buttons: 0x881,
                ..Command::default()
            },
            &mut ReferenceWorld,
        )
        .unwrap();
    let snapshot = component.snapshot().unwrap();
    assert_eq!(sprint(snapshot), [(down, 1); 3]);
    assert_eq!(snapshot.instances[&5].variables[&1919], Value::Bool(true));
    assert!(!snapshot.initialization_complete());
    component
        .advance_command(
            Command {
                frame: 1031,
                ..Command::default()
            },
            &mut ReferenceWorld,
        )
        .unwrap();
    assert_eq!(sprint(component.snapshot().unwrap()), [(up, 1); 3]);
}

pub(super) fn firing_placement_reset_and_timer_boundaries_match_python() {
    let runtime =
        PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").expect("set ORPHEUS_RUNTIME_DIR"));
    let graphs = Arc::new(GraphStore::load(runtime.join("data/game_graphs_174.json.gz")).unwrap());
    assert!(
        graphs.graph(0x0b92).is_some(),
        "the runtime pack lacks Practice Range graph 0B92"
    );
    let mut component = Component::start(
        graphs,
        0xa100_0000,
        Variables::new(),
        vec![Root::new(1, 0x0b92)],
        100,
        &mut ReferenceWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    // Python BotWorld with no selected AI target, frame 100, scoped RESET.
    assert_eq!(
        component
            .queue_message(1, 0x0240_0000_0000_0013, Variables::new())
            .unwrap(),
        1
    );
    component.run_now(&mut ReferenceWorld).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    assert_eq!(active(snapshot, 1), [5, 12]);
    assert_eq!(snapshot.instances[&1].variables[&889], Value::Bool(false));
    assert_eq!(
        snapshot.pending_timers[&1],
        [
            PendingTimer {
                due_ms: 2100,
                raw_state: 5,
                finish: false,
                parameter: 1
            },
            PendingTimer {
                due_ms: 4350,
                raw_state: 12,
                finish: false,
                parameter: 1
            },
        ]
    );
    for frame in 101..=130 {
        component.advance_frame(frame, &mut ReferenceWorld).unwrap();
    }
    assert_eq!(component.now_ms(), 2095);
    assert_eq!(active(component.snapshot().unwrap(), 1), [5, 12]);
    component.advance_frame(131, &mut ReferenceWorld).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    assert_eq!(active(snapshot, 1), [12, 0]);
    assert_eq!(
        snapshot.instances[&1].states[&0].kind,
        StateKind::AiCandidate { selected: false }
    );
    assert_eq!(snapshot.instances[&1].states[&0].start_ms, 2100);
    assert_eq!(snapshot.instances[&1].variables[&889], Value::Bool(false));
    for frame in 132..=270 {
        component.advance_frame(frame, &mut ReferenceWorld).unwrap();
    }
    assert_eq!(component.now_ms(), 4335);
    assert_eq!(active(component.snapshot().unwrap(), 1), [12, 0]);
    component.advance_frame(271, &mut ReferenceWorld).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    assert_eq!(active(snapshot, 1), [0]);
    assert_eq!(snapshot.instances[&1].variables[&889], Value::Bool(true));
    assert!(snapshot.pending_timers.is_empty());
}

pub(super) fn controller_and_training_bot_supported_startup_edges_match_python() {
    let runtime =
        PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").expect("set ORPHEUS_RUNTIME_DIR"));
    let graphs = GraphStore::load(runtime.join("data/game_graphs_174.json.gz")).unwrap();
    let run = |entity, roots| {
        initialize(
            &graphs,
            entity,
            Variables::new(),
            roots,
            &mut ReferenceWorld,
            MAX_INIT_WORK,
        )
    };
    let hud = run(0xa000_0100, vec![Root::new(1, 0x20e4)]).unwrap();
    assert!(hud.initialization_complete(), "{:?}", hud.diagnostics);
    assert_eq!(active(&hud, 1), [25]);

    let mut controller = Root::new(1, 0x13c1);
    controller.variables.insert(8419, Value::Bool(true));
    let owner = run(0xa000_0100, vec![controller]).unwrap();
    let instances: Vec<_> = owner
        .instances
        .iter()
        .map(|(&id, instance)| (id, instance.graph, instance.parent))
        .collect();
    // Python Component with the practice controller and v8419 enabled.
    assert_eq!(
        instances,
        [
            (1, 0x13c1, None),
            (2, 0x20e4, Some((1, 42))),
            (3, 0x288a, Some((1, 43))),
            (4, 0x288b, Some((3, 0))),
            (5, 0x288d, Some((3, 1))),
        ]
    );
    assert_eq!(active(&owner, 2), [25]);
    assert!(!owner.initialization_complete());

    // Only the enriched pack includes the NPC body bindings.
    if let Some(body) = graphs.entity_body(0x0400_0000_0000_1403) {
        let roots = body_roots(body).unwrap();
        assert_eq!(
            roots
                .iter()
                .map(|root| (root.id, root.graph))
                .collect::<Vec<_>>(),
            [
                (1, 0x0033),
                (2, 0x0ba8),
                (3, 0x0be2),
                (4, 0x0ba3),
                (5, 0x174f),
                (6, 0x2206)
            ],
        );
        assert_eq!(roots[0].variables[&199], Value::Integer(200));
        let bot = run(0xa100_0000, vec![Root::new(1, 0x0c18)]).unwrap();
        assert_eq!(bot.instances[&2].graph, 0x0d24);
        assert_eq!(bot.instances[&2].parent, Some((1, 3)));
        assert_eq!(active(&bot, 2), [56, 76]);
        assert!(!bot.initialization_complete());
    }
}

#[test]
fn ai_control_state_finishes_at_begin_time_only_for_an_absent_owner_relation() {
    const OWNER: u32 = 0xa000_0101;
    const FRAME: u32 = 100;
    struct AiWorld(Option<u32>);
    impl World for AiWorld {
        fn related_entity(
            &mut self,
            site: Site,
            relation: EntityRelation,
            entity: u32,
        ) -> Option<u32> {
            assert_eq!(site.node, Some(1));
            assert_eq!(relation, EntityRelation::AiControlEntity);
            assert_eq!(entity, OWNER);
            self.0
        }
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }
    }

    let owner = json!({"$":"STU_B5A0CAF0","m_EE729DCB":0});
    for (target, relation, finishes) in [
        (owner.clone(), Some(0), true),
        (owner.clone(), None, false),
        (owner, Some(OWNER + 1), false),
        (var(100, true), Some(0), false),
    ] {
        let graphs = Arc::new(store(vec![graph(
            42,
            vec![
                node(
                    0,
                    "STUStatescriptEntry",
                    None,
                    json!({"m_outPlug":plug(&[(1,"m_beginPlug")], &[])}),
                ),
                node(
                    1,
                    "STU_066B4605",
                    Some(0),
                    json!({
                        "m_00D49978":target,
                        "m_D66E7D68":{"$":"STUConfigVarInt","m_value":0},
                        "m_F2810377":{"$":"STUConfigVarIdentifier","m_identifier":"0x0D8000000000023A"},
                        "m_F8585ABF":{"$":"STUConfigVarBool","m_value":0},
                        "m_priority":{"$":"STU_8A491388","m_priority":"0x0C40000000000002"},
                        "m_onBeginPlug":plug(&[(2,"m_inPlug")], &[]),
                        "m_onFinishedPlug":plug(&[(3,"m_inPlug")], &[]),
                        "m_onAbortPlug":plug(&[(4,"m_inPlug")], &[]),
                    }),
                ),
                node(
                    2,
                    "STUStatescriptActionSetVar",
                    None,
                    json!({"m_out_Var":var(100,false), "m_value":true}),
                ),
                node(
                    3,
                    "STUStatescriptActionSetVar",
                    None,
                    json!({"m_out_Var":var(101,false), "m_value":var(100,false)}),
                ),
                node(
                    4,
                    "STUStatescriptActionSetVar",
                    None,
                    json!({"m_out_Var":var(101,false), "m_value":false}),
                ),
            ],
            json!([1]),
            json!([0]),
        )]));
        let component = Component::start(
            graphs,
            OWNER,
            Variables::new(),
            vec![Root::new(1, 42)],
            FRAME,
            &mut AiWorld(relation),
            MAX_INIT_WORK,
        )
        .unwrap();
        let snapshot = component.snapshot().unwrap();
        let instance = &snapshot.instances[&1];
        assert_eq!(component.now_ms(), frame_start(FRAME));
        assert!(snapshot.pending_timers.is_empty());
        if finishes {
            assert!(snapshot.initialization_complete());
            let state = &instance.states[&0];
            assert!(!state.active);
            assert_eq!(state.activation, 1);
            assert_eq!(state.start_ms, frame_start(FRAME));
            assert_eq!(instance.variables[&101], Value::Bool(true));
        } else {
            assert!(matches!(
                snapshot.diagnostics.as_slice(),
                [Diagnostic {
                    issue: Issue::UnsupportedField(
                        "AI weapon control component" | "AI weapon target"
                    ),
                    ..
                }]
            ));
            assert!(instance.states[&0].active);
            assert!(!instance.variables.contains_key(&101));
        }
    }
}
#[test]
fn missing_world_queries_report_expression_sites_without_writing_guessed_values() {
    let foreign = 0xa000_0101;
    for (opcodes, expected) in [
        (
            json!([8, 0, 50, 0]),
            EvalError::UnavailableEntityPosition(foreign),
        ),
        (
            json!([8, 0, 7, 1, 15, 0]),
            EvalError::UnavailableEntityVariable {
                entity: foreign,
                id: 100,
            },
        ),
    ] {
        let config = json!({"$":"STU_919FD47C","m_expression":{
            "m_opcodes":opcodes,"m_dynamicVars":[var(99, true), var(100, true)]
        }});
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
                    "STUStatescriptActionSetVar",
                    None,
                    json!({"m_out_Var":var(101, true), "m_value":config}),
                ),
            ],
            json!([]),
            json!([0]),
        );
        let snapshot = initialize(
            &store(vec![fixture]),
            0xa000_0100,
            Variables::from([(99, Value::Entity(foreign))]),
            vec![Root::new(1, 42)],
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap();
        assert!(!snapshot.initialization_complete());
        assert!(!snapshot.variables.contains_key(&101));
        assert_eq!(
            snapshot.diagnostics,
            [Diagnostic {
                site: Site {
                    entity: 0xa000_0100,
                    instance: 1,
                    graph: 42,
                    node: Some(1),
                },
                issue: Issue::Expression(expected),
                registration_rejected: false,
                server_only: false,
            }],
        );
    }
}

#[test]
fn own_entity_member_keeps_known_unset_values_and_reactive_variable_reads() {
    let entity = 0xa000_0100;
    let condition = json!({"$":"STU_919FD47C","m_expression":{
        "m_opcodes":[8,0,7,1,15,0],"m_dynamicVars":[var(99,true),var(100,true)]
    }});
    let fixture = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_beginPlug")], &[])}),
            ),
            node(
                1,
                "STUStatescriptStateBooleanSwitch",
                Some(0),
                json!({"m_condition":condition}),
            ),
        ],
        json!([1]),
        json!([0]),
    );
    let mut component = Component::start(
        Arc::new(store(vec![fixture])),
        entity,
        Variables::from([(99, Value::Entity(entity))]),
        vec![Root::new(1, 42)],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    for (value, expected) in [
        (None, false),
        (Some(Value::Bool(true)), true),
        (Some(Value::Null), false),
    ] {
        if let Some(value) = value {
            component
                .set_entity_variable(100, value, &mut EmptyWorld)
                .unwrap();
            component.run_now(&mut EmptyWorld).unwrap();
        }
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(
            snapshot.instances[&1].states[&0].kind,
            StateKind::BooleanSwitch(expected),
        );
    }
}
