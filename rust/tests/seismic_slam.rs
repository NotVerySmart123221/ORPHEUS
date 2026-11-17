use std::{collections::VecDeque, path::PathBuf, sync::Arc};

use orpheus::{
    script_graph::GraphStore,
    script_init::{Component, MAX_INIT_WORK, Root, Site, World},
    script_wire::{Value, Variables},
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
