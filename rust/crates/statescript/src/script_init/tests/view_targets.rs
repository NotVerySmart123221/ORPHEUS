use std::cell::Cell;

use super::*;
use crate::script_init::ViewTargetQuery;

struct ViewWorld {
    expected: Json,
    responses: Option<Vec<u32>>,
    calls: Cell<usize>,
    owner: u32,
}

impl World for ViewWorld {
    fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
        None
    }

    fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
        None
    }

    fn view_targets(&self, site: Site, query: &ViewTargetQuery<'_>) -> Option<Vec<u32>> {
        let value = |key| self.expected[key].as_f64().unwrap();
        assert_eq!((site.entity, query.source), (self.owner, self.owner));
        assert_eq!(query.range, value("range"));
        assert_eq!(query.aperture, value("aperture"));
        assert_eq!(query.limit as f64, value("limit"));
        assert_eq!(
            query.offset,
            serde_json::from_value::<[f64; 3]>(self.expected["offset"].clone()).unwrap()
        );
        assert_eq!(
            query.profile,
            crate::script_graph::asset_guid(&self.expected["profile"]).unwrap()
        );
        assert_eq!(query.masks, [1, 0, 16]);
        assert_eq!(query.ranking["$"], "STU_B61DA2A5");
        let sight = query.sight.unwrap();
        assert_eq!(sight.static_mask, 2);
        assert_eq!(sight.block_actor_flags, 0);
        assert_eq!(sight.samples, [0.5]);
        self.calls.set(self.calls.get() + 1);
        self.responses.clone()
    }
}

#[test]
fn view_tracker_keeps_order_resamples_and_distinguishes_unavailable() {
    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../../../fixtures/behavior/script-view-targets-v1.json"
    ))
    .unwrap();
    let input = &fixture["input"];
    let expected = &fixture["expected"];
    let owner = u32::try_from(input["owner"].as_u64().unwrap()).unwrap();
    let output = u32::try_from(expected["output_variable"].as_u64().unwrap()).unwrap();
    let period = u32::try_from(expected["period_frames"].as_u64().unwrap()).unwrap();
    let targets: Vec<u32> = serde_json::from_value(input["targets"].clone()).unwrap();
    let expected_targets = Value::Array(targets.iter().copied().map(Value::Entity).collect());
    let make = |index, fields: Json| {
        let data = json!({"version":1,"bodies":{},"missing":[],
            "view_sight_profiles":{(expected["profile"].as_str().unwrap()):{
                "static_mask":2,"block_actor_flags":0,"samples":[0.5]
            }},
            "graphs":[{
            "index":index,"items":3,"entries":[0],"states":[1],"remote":[],"sync_vars":[],
            "containers":{},"unknown_nodes":[],"fields":{},
            "header":{"m_nodesBitCount":2,"m_statesBitCount":2,"m_remoteSyncNodesBitCount":2,"m_syncVarsBitCount":2},
            "nodes":[
                {"pos":0,"rid":0,"uid":0,"remote":-1,"state":null,"server_only":false,"client_only":false,
                 "class":"STUStatescriptEntry","fields":{"m_outPlug":{"$":"Plug","links":[[1,"m_beginPlug"]]}}},
                {"pos":1,"rid":1,"uid":1,"remote":-1,"state":0,"server_only":false,"client_only":false,
                 "class":"STUStatescriptStateTrackTargets","fields":fields},
                {"pos":2,"rid":2,"uid":2,"remote":-1,"state":null,"server_only":false,"client_only":false,
                 "class":"STUStatescriptActionSetVar","fields":{
                    "m_ADEB6E05":true,
                    "m_out_Var":{"$":"STUConfigVarDynamic","m_identifier":65,"m_60DB8F99":false},
                    "m_value":17
                 }}
            ]
        }]});
        Arc::new(GraphStore::from_json(&serde_json::to_vec(&data).unwrap()).unwrap())
    };
    for index in [607, 816] {
        let graphs = make(index, input["field_shape"].clone());
        assert!(graphs.uses_room_spatial_frame());
        assert!(graphs.uses_spatial_actor_rays());
        let mut world = ViewWorld {
            expected: expected.clone(),
            responses: Some(targets.clone()),
            calls: Cell::new(0),
            owner,
        };
        let mut component = Component::start(
            graphs,
            owner,
            Variables::new(),
            vec![Root::new(1, index)],
            10,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        let published = |component: &Component| {
            component.snapshot().unwrap().instances[&1]
                .variables
                .get(&output)
                .cloned()
        };
        assert_eq!(published(&component), Some(expected_targets.clone()));
        world.responses = Some(Vec::new());
        component
            .advance_frame(10 + period - 1, &mut world)
            .unwrap();
        assert_eq!(world.calls.get(), 1);
        assert_eq!(published(&component), Some(expected_targets.clone()));
        component.advance_frame(10 + period, &mut world).unwrap();
        assert_eq!(world.calls.get(), 2);
        assert_eq!(published(&component), Some(Value::Array(Vec::new())));
        world.responses = None;
        component
            .advance_frame(10 + 2 * period, &mut world)
            .unwrap();
        assert!(
            component
                .snapshot()
                .unwrap()
                .diagnostics
                .iter()
                .any(|entry| {
                    entry.issue == Issue::UnsupportedField("view target query unavailable")
                })
        );
        assert_eq!(published(&component), Some(Value::Array(Vec::new())));
    }

    let mut fields = input["field_shape"].clone();
    fields["m_transitionPlug"] = json!([{"$":"Plug","links":[[2,"m_inPlug"]]}]);
    let graphs = make(915, fields);
    for response in [Some(Vec::new()), None] {
        let available = response.is_some();
        let mut world = ViewWorld {
            expected: expected.clone(),
            responses: response,
            calls: Cell::new(0),
            owner,
        };
        let component = Component::start(
            Arc::clone(&graphs),
            owner,
            Variables::new(),
            vec![Root::new(1, 915)],
            10,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        let snapshot = component.snapshot().unwrap();
        assert_eq!(
            snapshot.instances[&1].variables.get(&65),
            available.then_some(&Value::Integer(17))
        );
    }
}
