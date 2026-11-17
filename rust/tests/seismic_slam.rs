use std::{collections::VecDeque, path::PathBuf, sync::Arc};

use orpheus::{
    script_expr::{Scope, Variable},
    script_graph::GraphStore,
    script_init::{Component, LandingSelection, MAX_INIT_WORK, Root, Site, World},
    script_wire::{Value, Variables},
    world::MAX_MOVEMENT_AXIS,
};
use serde_json::{Value as Json, json};

fn load_contract(name: &str, case_id: &str) -> Json {
    let dir = std::env::var_os("ORPHEUS_BEHAVIOR_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/behavior"));
    let contract: Json = serde_json::from_slice(&std::fs::read(dir.join(name)).unwrap()).unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], case_id);
    assert_eq!(contract["target_build"], 104319);
    contract
}

fn dyn_var(identifier: u32, scope: u8) -> Json {
    json!({
        "$": "STUConfigVarDynamic",
        "m_identifier": identifier,
        "m_60DB8F99": scope
    })
}

fn plug_link(pos: u16, plug: &str) -> Json {
    json!({ "$": "STUStatescriptBasePlug", "links": [[pos, plug]] })
}

fn node_json(pos: u16, class: &str, state: Option<u16>, rid: u16, mut fields: Json) -> Json {
    if state.is_some() {
        if let Some(map) = fields.as_object_mut() {
            map.entry("m_2BBEEAB8").or_insert_with(|| json!(1));
        }
    }
    json!({
        "pos": pos,
        "class": class,
        "uid": 0,
        "rid": rid,
        "remote": -1,
        "state": state,
        "client_only": 0,
        "server_only": 0,
        "fields": fields
    })
}

#[derive(Default)]
struct SamplingWorld {
    samples: VecDeque<Vec<u32>>,
    events: Vec<(String, f64)>,
}

impl World for SamplingWorld {
    fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
        None
    }

    fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
        None
    }

    fn shockwave_targets(&mut self, _site: Site, reach: f64) -> Option<Vec<u32>> {
        self.events.push(("sample".into(), reach));
        self.samples.pop_front()
    }

    fn modify_health(&mut self, _site: Site, _elapsed_ms: u64) -> bool {
        self.events.push(("modify_health".into(), 0.0));
        true
    }
}

#[derive(Default)]
struct LandingWorld {
    point: Option<[f64; 3]>,
    query_frames: Vec<u32>,
}

impl World for LandingWorld {
    fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
        None
    }

    fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
        None
    }

    fn select_landing(&mut self, site: Site) -> LandingSelection {
        let _ = site;
        // Frame numbers are tracked in the test loop per advance_frame call.
        self.query_frames.push(1);
        match self.point {
            Some(point) => LandingSelection::Point(point),
            None => LandingSelection::Invalid,
        }
    }
}

#[test]
fn immediate_wave_samples_zero_then_reach_and_finishes() {
    let contract = load_contract(
        "seismic-slam-wave-v1.json",
        "seismic-slam.immediate-wave-lifecycle.v1",
    );
    let inputs = &contract["input"];
    let expected = &contract["expected"];

    let owner = inputs["owner"].as_u64().unwrap() as u32;
    let first_tick_frame = inputs["first_tick_frame"].as_u64().unwrap() as u32;
    let reach = inputs["reach"].as_f64().unwrap();
    let initial_targets = inputs["initial_targets"].as_i64().unwrap();
    let begin_candidates: Vec<u32> = inputs["begin_candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u32)
        .collect();
    let tick_candidates: Vec<u32> = inputs["tick_candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u32)
        .collect();

    let graphs = GraphStore::from_json(
        &serde_json::to_vec(&json!({
            "version": 1,
            "graphs": [{
                "index": 0x0B27,
                "header": {
                    "m_nodesBitCount": 8,
                    "m_statesBitCount": 2,
                    "m_remoteSyncNodesBitCount": 0,
                    "m_syncVarsBitCount": 0
                },
                "fields": {},
                "items": 96,
                "containers": {},
                "unknown_nodes": [],
                "nodes": [
                    node_json(93, "STUStatescriptStateModifyHealth", Some(1), 0, json!({})),
                    node_json(94, "STUStatescriptActionSetVar", None, 1, json!({
                        "m_out_Var": dyn_var(7, 0),
                        "m_value": { "$": "STUConfigVarInt", "m_value": 99 }
                    })),
                    node_json(95, "STUStatescriptStateShockwave", Some(0), 2, json!({
                        "m_3418CEA4": { "$": "STUConfigVarBool", "m_value": 1 },
                        "m_6A79BD98": { "$": "STUConfigVarFloat", "m_value": reach },
                        "m_8A2905A0": dyn_var(375, 0),
                        "m_subgraphPlug": {
                            "$": "STU_904BFCEC",
                            "m_FF3DAF1E": [1],
                            "links": [[93, "m_beginPlug"]]
                        },
                        "m_onFinishedPlug": plug_link(94, "m_inPlug")
                    }))
                ],
                "states": [95, 93],
                "entries": [],
                "remote": [],
                "sync_vars": []
            }],
            "bodies": {},
            "missing": []
        }))
        .unwrap(),
    )
    .unwrap();

    let seed = inputs["seed"].as_u64().unwrap() as u32;
    let mut world = SamplingWorld {
        samples: VecDeque::from([begin_candidates, tick_candidates]),
        events: Vec::new(),
    };
    let mut root = Root::new(1, 0x0B27);
    root.variables.insert(375, Value::Integer(initial_targets));

    let mut runtime = Component::start(
        Arc::new(graphs),
        owner,
        Variables::new(),
        vec![root],
        seed,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();

    runtime.begin_state(1, 0, &mut world).unwrap();
    runtime.run_now(&mut world).unwrap();

    let snap = runtime.snapshot().unwrap();
    let inst = &snap.instances[&1];
    assert_eq!(
        inst.states[&0].active,
        expected["active_after_begin"].as_bool().unwrap()
    );
    assert_eq!(
        inst.states[&1].active,
        expected["active_after_begin"].as_bool().unwrap()
    );
    assert_eq!(
        inst.states[&0].kind,
        orpheus::script_init::StateKind::Shockwave {
            managed: !expected["auxiliary_list_present"].as_bool().unwrap()
        }
    );

    let events = expected["events"].as_array().unwrap();
    assert_eq!(
        world.events[0],
        ("sample".into(), events[0][1].as_f64().unwrap())
    );
    assert_eq!(world.events[1].0, "modify_health");
    let want_begin_targets: Vec<Value> = events[1][1]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| Value::Entity(v.as_u64().unwrap() as u32))
        .collect();
    assert_eq!(inst.variables[&375], Value::Array(want_begin_targets));

    runtime.advance_frame(first_tick_frame, &mut world).unwrap();
    let snap = runtime.snapshot().unwrap();
    let inst = &snap.instances[&1];
    assert_eq!(
        inst.states[&0].active,
        expected["active_after_tick"].as_bool().unwrap()
    );
    assert_eq!(
        inst.states[&1].active,
        expected["active_after_tick"].as_bool().unwrap()
    );
    assert_eq!(
        inst.variables[&7],
        Value::Integer(expected["finish_marker"].as_i64().unwrap())
    );
    assert_eq!(
        world.events[2],
        ("sample".into(), events[2][1].as_f64().unwrap())
    );
    assert_eq!(world.events[3].0, "modify_health");
    let want_tick_targets: Vec<Value> = events[3][1]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| Value::Entity(v.as_u64().unwrap() as u32))
        .collect();
    assert_eq!(inst.variables[&375], Value::Array(want_tick_targets));
}

#[test]
fn confirmation_publishes_static_destination_before_travel() {
    let contract = load_contract(
        "seismic-slam-placement-v1.json",
        "seismic-slam.airborne-placement.v1",
    );
    let inputs = &contract["input"];
    let expected = &contract["expected"];

    let owner = inputs["owner"].as_u64().unwrap() as u32;
    let speed = inputs["speed"].as_f64().unwrap();
    let old_dest = inputs["old_destination"].as_array().unwrap();
    let old_dest_vec = [
        old_dest[0].as_f64().unwrap(),
        old_dest[1].as_f64().unwrap(),
        old_dest[2].as_f64().unwrap(),
    ];
    let old_entity = inputs["old_entity"].as_u64().unwrap() as u32;

    let graphs = GraphStore::from_json(
        &serde_json::to_vec(&json!({
            "version": 1,
            "graphs": [{
                "index": 0x0B27,
                "header": {
                    "m_nodesBitCount": 8,
                    "m_statesBitCount": 3,
                    "m_remoteSyncNodesBitCount": 0,
                    "m_syncVarsBitCount": 0
                },
                "fields": {},
                "items": 165,
                "containers": {},
                "unknown_nodes": [],
                "nodes": [
                    node_json(15, "STU_4C96470A", Some(0), 0, json!({
                        "m_A6BB0869": { "$": "STUConfigVarBool", "m_value": 1 },
                        "m_B02B1FD7": dyn_var(1321, 0),
                        "m_C3240FFC": dyn_var(1627, 0),
                        "m_30710B76": dyn_var(431, 0),
                        "m_1549F0F9": dyn_var(1883, 0),
                        "m_transitionPlug": [plug_link(155, "m_inPlug")],
                        "m_F6955E81": { "$": "STUStatescriptBasePlug", "links": [[15, "m_F198FD3A"], [70, "m_inPlug"]] }
                    })),
                    node_json(18, "STUStatescriptStateMoveTo", Some(2), 1, json!({
                        "m_B060F688": 0,
                        "m_865F3D01": dyn_var(431, 0),
                        "m_destinationEntity": dyn_var(1883, 0),
                        "m_speed": { "$": "STUConfigVarFloat", "m_value": speed }
                    })),
                    node_json(70, "STUStatescriptActionSetVar", None, 2, json!({
                        "m_out_Var": dyn_var(11174, 0),
                        "m_value": { "$": "STUConfigVarBool", "m_value": 1 }
                    })),
                    node_json(76, "STUStatescriptStateBooleanSwitch", Some(1), 3, json!({
                        "m_condition": dyn_var(10473, 1),
                        "m_onBeginPlug": plug_link(164, "m_inPlug"),
                        "m_trueSubgraphPlug": {
                            "$": "STU_904BFCEC",
                            "m_FF3DAF1E": [2],
                            "links": [[18, "m_beginPlug"]]
                        }
                    })),
                    node_json(155, "STU_3387AB5D", None, 4, json!({
                        "m_condition": {
                            "$": "STU_919FD47C",
                            "m_expression": {
                                "m_opcodes": [8, 0, 2, 4, 8, 1, 0],
                                "m_dynamicVars": [dyn_var(1627, 0), dyn_var(1321, 0)]
                            }
                        },
                        "m_truePlug": plug_link(76, "m_beginPlug")
                    })),
                    node_json(164, "STUStatescriptActionSetVar", None, 5, json!({
                        "m_out_Var": dyn_var(432, 0),
                        "m_value": dyn_var(431, 0)
                    }))
                ],
                "states": [15, 76, 18],
                "entries": [],
                "remote": [],
                "sync_vars": []
            }],
            "bodies": {},
            "missing": []
        }))
        .unwrap(),
    )
    .unwrap();

    let seed = inputs["seed"].as_u64().unwrap() as u32;
    let mut world = LandingWorld::default();
    let entity_vars = Variables::from([(10473, Value::Bool(true))]);
    let mut root = Root::new(1, 0x0B27);
    root.variables.extend([
        (431, Value::Vector(old_dest_vec)),
        (1883, Value::Entity(old_entity)),
        (11174, Value::Bool(false)),
    ]);

    let mut runtime = Component::start(
        Arc::new(graphs),
        owner,
        entity_vars,
        vec![root],
        seed,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    runtime.begin_state(1, 0, &mut world).unwrap();
    runtime.run_now(&mut world).unwrap();

    let ticks = inputs["ticks"].as_array().unwrap();
    let want_ticks = expected["ticks"].as_array().unwrap();
    let mut queried_frames = Vec::new();

    for (tick, want) in ticks.iter().zip(want_ticks) {
        let frame = tick["frame"].as_u64().unwrap() as u32;
        let confirm = tick["confirm"].as_bool().unwrap();
        runtime
            .set_instance_variable(1, 1321, Value::Bool(confirm), &mut world)
            .unwrap();
        world.point = tick["point"].as_array().map(|arr| {
            [
                arr[0].as_f64().unwrap(),
                arr[1].as_f64().unwrap(),
                arr[2].as_f64().unwrap(),
            ]
        });
        let before_queries = world.query_frames.len();
        runtime.advance_frame(frame, &mut world).unwrap();
        if world.query_frames.len() > before_queries {
            queried_frames.push(frame);
        }

        let snap = runtime.snapshot().unwrap();
        let inst = &snap.instances[&1];
        assert_eq!(
            inst.states[&0].active,
            want["selector_active"].as_bool().unwrap()
        );

        let valid = runtime.resolved_variable(
            1,
            Variable {
                id: 1627,
                scope: Scope::Instance,
            },
        );
        match want["valid"].as_bool() {
            None => assert!(matches!(valid, None | Some(Value::Null)), "frame {frame}"),
            Some(b) => assert_eq!(valid, Some(Value::Bool(b)), "frame {frame}"),
        }

        let want_dest = want["destination"].as_array().unwrap();
        assert_eq!(
            inst.variables[&431],
            Value::Vector([
                want_dest[0].as_f64().unwrap(),
                want_dest[1].as_f64().unwrap(),
                want_dest[2].as_f64().unwrap(),
            ]),
            "frame {frame}"
        );
        assert_eq!(
            inst.variables[&1883],
            Value::Entity(want["entity"].as_u64().unwrap() as u32),
            "frame {frame}"
        );

        let actual_move = inst.states.get(&2).and_then(|s| s.move_request());
        match &want["move_to"] {
            Json::Null => assert_eq!(actual_move, None, "frame {frame}"),
            Json::Array(arr) => {
                let p = arr[0].as_array().unwrap();
                let want_req = (
                    [
                        p[0].as_f64().unwrap(),
                        p[1].as_f64().unwrap(),
                        p[2].as_f64().unwrap(),
                    ],
                    arr[1].as_f64().unwrap(),
                );
                assert_eq!(actual_move, Some(want_req), "frame {frame}");
            }
            other => panic!("unexpected move_to expectation: {other}"),
        }
    }

    let want_query_frames: Vec<u32> = expected["query_frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u32)
        .collect();
    assert_eq!(queried_frames, want_query_frames);

    let snap = runtime.snapshot().unwrap();
    let inst = &snap.instances[&1];
    let preview = expected["confirmed_preview"].as_array().unwrap();
    assert_eq!(
        inst.variables[&432],
        Value::Vector([
            preview[0].as_f64().unwrap(),
            preview[1].as_f64().unwrap(),
            preview[2].as_f64().unwrap(),
        ])
    );
    assert_eq!(
        inst.variables[&11174],
        Value::Bool(expected["manual_confirm_plug_followed"].as_bool().unwrap())
    );
}

#[test]
fn move_to_ignores_destination_outside_movement_axis_bounds() {
    let graphs = GraphStore::from_json(
        &serde_json::to_vec(&json!({
            "version": 1,
            "graphs": [{
                "index": 0x0B27,
                "header": {
                    "m_nodesBitCount": 8,
                    "m_statesBitCount": 1,
                    "m_remoteSyncNodesBitCount": 0,
                    "m_syncVarsBitCount": 0
                },
                "fields": {},
                "items": 19,
                "containers": {},
                "unknown_nodes": [],
                "nodes": [
                    node_json(18, "STUStatescriptStateMoveTo", Some(0), 0, json!({
                        "m_B060F688": 0,
                        "m_865F3D01": dyn_var(431, 0),
                        "m_destinationEntity": dyn_var(1883, 0),
                        "m_speed": { "$": "STUConfigVarFloat", "m_value": 25.0 }
                    }))
                ],
                "states": [18],
                "entries": [],
                "remote": [],
                "sync_vars": []
            }],
            "bodies": {},
            "missing": []
        }))
        .unwrap(),
    )
    .unwrap();

    let mut world = LandingWorld::default();
    let mut root = Root::new(1, 0x0B27);
    root.variables.extend([
        (431, Value::Vector([MAX_MOVEMENT_AXIS + 1.0, 0.0, 0.0])),
        (1883, Value::Entity(0)),
    ]);

    let mut runtime = Component::start(
        Arc::new(graphs),
        123,
        Variables::new(),
        vec![root],
        1,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    runtime.begin_state(1, 0, &mut world).unwrap();
    runtime.run_now(&mut world).unwrap();

    let inst = &runtime.snapshot().unwrap().instances[&1];
    assert!(inst.states[&0].active);
    assert_eq!(inst.states[&0].move_request(), None);
}
