use std::{cell::Cell, sync::Arc};

use orpheus_statescript::{
    script_expr::EvalError,
    script_graph::{GraphStore, asset_guid},
    script_init::{Component, Diagnostic, Issue, MAX_INIT_WORK, Root, Site, World},
    script_wire::{Value, Variables},
};
use serde_json::{Value as Json, json};

const OWNER: u32 = 831;
const GRAPH: u32 = 61091;
const SELECTOR: u32 = 74;
const OUTPUT: u32 = 83;

struct ModelWorld {
    expected: (u32, u64),
    membership: Option<bool>,
    queries: Cell<usize>,
}

impl World for ModelWorld {
    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }

    fn body_has_hardpoint(&self, site: Site, entity: u32, hardpoint: u64) -> Option<bool> {
        assert_eq!(
            (site.entity, site.graph, site.node),
            (OWNER, GRAPH, Some(1))
        );
        assert_eq!((entity, hardpoint), self.expected);
        self.queries.set(self.queries.get() + 1);
        self.membership
    }
}

fn node(pos: u16, class: &str, state: Option<u16>, mut fields: Json) -> Json {
    fields["m_2BBEEAB8"] = json!(state.is_some());
    fields["m_ADEB6E05"] = json!(class == "STUStatescriptActionSetVar");
    json!({
        "class":class, "pos":pos, "rid":pos, "uid":pos, "state":state,
        "remote":-1, "client_only":false, "server_only":false, "fields":fields,
    })
}

fn start(condition: Json, selected: Value, world: &mut ModelWorld) -> Component {
    let plug = |pos, input| json!({"$":"Plug","links":[[pos,input]],"m_FF3DAF1E":[]});
    let mark = |pos, result| {
        node(
            pos,
            "STUStatescriptActionSetVar",
            None,
            json!({
                "m_out_Var":{"$":"STUConfigVarDynamic","m_identifier":OUTPUT,"m_60DB8F99":true},
                "m_value":result,
            }),
        )
    };
    let bundle = json!({"version":1,"bodies":{},"missing":[],"graphs":[{
        "index":GRAPH,"items":4,"states":[1],"entries":[0],"remote":[],"sync_vars":[],
        "unknown_nodes":[],"fields":{},"containers":{},"header":{
            "m_nodesBitCount":4,"m_statesBitCount":4,"m_remoteSyncNodesBitCount":4,"m_syncVarsBitCount":4,
        },
        "nodes":[
            node(0,"STUStatescriptEntry",None,json!({"m_outPlug":plug(1,"m_beginPlug")})),
            node(1,"STUStatescriptStateBooleanSwitch",Some(0),json!({
                "m_condition":condition,"m_onTruePlug":plug(2,"m_inPlug"),"m_onFalsePlug":plug(3,"m_inPlug"),
            })),
            mark(2,true),
            mark(3,false),
        ],
    }]});
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap());
    Component::start(
        graphs,
        OWNER,
        Variables::from([(SELECTOR, selected)]),
        vec![Root::new(1, GRAPH)],
        0,
        world,
        MAX_INIT_WORK,
    )
    .unwrap()
}

#[test]
fn native_hardpoint_condition_reads_only_typed_model_membership() {
    let contract: Json = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/statescript-hardpoint-membership-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["evidence"], "synthetic-rust");
    let input = &contract["input"];
    let hardpoint = asset_guid(&input["predicate"]["m_hardPoint"]["m_hardPoint"]).unwrap();
    let foreign = u32::try_from(input["foreign_entity"].as_u64().unwrap()).unwrap();
    let mut world = ModelWorld {
        expected: (OWNER, hardpoint),
        membership: None,
        queries: Cell::new(0),
    };
    for case in input["valid_cases"].as_array().unwrap() {
        let mut predicate = input["predicate"].clone();
        predicate["m_entity"] = case["selector"].clone();
        world.expected.0 = if case["foreign"].as_bool().unwrap() {
            foreign
        } else {
            OWNER
        };
        world.membership = case["membership"].as_bool();
        world.queries.set(0);
        let component = start(predicate, Value::Entity(foreign), &mut world);
        let snapshot = component.snapshot().unwrap();
        assert_eq!(world.queries.get(), 1);
        assert_eq!(
            snapshot.variables.get(&OUTPUT),
            world.membership.map(Value::Bool).as_ref()
        );
        if world.membership.is_some() {
            assert!(snapshot.diagnostics.is_empty());
        } else {
            assert!(matches!(
                snapshot.diagnostics.as_slice(),
                [Diagnostic { issue: Issue::Expression(EvalError::UnavailableBodyHardpoint {
                    entity, hardpoint: requested,
                }), .. }] if *entity == world.expected.0 && *requested == hardpoint
            ));
        }
    }

    for malformed in input["malformed_cases"].as_array().unwrap() {
        let mut predicate = input["predicate"].clone();
        predicate[malformed["field"].as_str().unwrap()] = malformed["value"].clone();
        world.queries.set(0);
        let selected = malformed["selected"]
            .as_u64()
            .map_or(foreign, |entity| u32::try_from(entity).unwrap());
        let component = start(predicate, Value::Entity(selected), &mut world);
        let snapshot = component.snapshot().unwrap();
        assert_eq!(world.queries.get(), 0);
        assert!(!snapshot.variables.contains_key(&OUTPUT));
        assert!(matches!(
            snapshot.diagnostics.as_slice(),
            [Diagnostic {
                issue: Issue::Expression(EvalError::MalformedConfig(_)),
                ..
            }]
        ));
    }
}
