use std::sync::Arc;

use orpheus_statescript::{
    script_graph::{GraphStore, RoomSpatialNeeds},
    script_init::{Component, MAX_INIT_WORK, RoomMessage, Root, Site, World},
    script_wire::{Value, Variables},
};
use serde_json::{Value as Json, json};

const GRAPH: u32 = 520;
const ROOT: u16 = 1;
const SOURCE: u32 = 501;
const RECIPIENT: u32 = 502;
const FOREIGN_SEND: u64 = 0x0240_0000_0000_41b0;
const LOCAL_SEND: u64 = FOREIGN_SEND + 1;
const NUMERIC_TOPIC: u64 = FOREIGN_SEND + 2;
const NUMERIC_PARAMETER: u64 = 0x0a40_0000_0000_020f;
const NUMERIC_SLOT: u32 = 0x020f;
const RECEIVED: u32 = 10;

#[derive(Default)]
struct Room {
    legacy_attempts: usize,
    requests: Vec<(u32, RoomMessage)>,
}

impl World for Room {
    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }

    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn send_message(&mut self, _: Site, _: u32, _: u64, _: &Variables) -> bool {
        self.legacy_attempts += 1;
        false
    }

    fn queue_room_message(&mut self, _: Site, entity: u32, message: RoomMessage) -> bool {
        self.requests.push((entity, message));
        true
    }
}

fn bundle() -> Json {
    let variable = |id: u32| json!({"$":"STUConfigVarDynamic","m_identifier":id,"m_60DB8F99":0});
    let topic =
        |id: u64| json!({"$":"STUConfigVarGameMessage","m_gameMessage":format!("0x{id:016X}")});
    let plug = |links| json!({"$":"STUStatescriptOutputPlug","links":links});
    let node = |pos: u16, class: &str, server_only, fields| {
        json!({"pos":pos,"rid":pos,"uid":pos,"class":class,"state":null,"remote":-1,
            "client_only":false,"server_only":server_only,"fields":fields})
    };
    let trigger = |pos, message, target| {
        node(
            pos,
            "STUStatescriptEntryGameMessage",
            false,
            json!({
                "m_gameMessage":topic(message),
                "m_params":[
                    {"m_B5051BCE":"0x0000000000000001","m_out_Var":variable(1)},
                    {"m_B5051BCE":"0x0000000000000002","m_out_Var":variable(2)}
                ],
                "m_outPlug":plug(json!([[target,"m_inPlug"]]))
            }),
        )
    };
    let send = |pos, target: Json| {
        node(
            pos,
            "STUStatescriptActionSendGameMessage",
            false,
            json!({
                "m_ADEB6E05":1,"m_gameMessage":topic(NUMERIC_TOPIC),"m_targets":target,
                "m_params":[{"$":"STU_814D595E","m_B5051BCE":format!("0x{NUMERIC_PARAMETER:016X}"),"m_value":variable(2)}],
                "m_outPlug":plug(json!([]))
            }),
        )
    };
    let receiver = node(
        4,
        "STUStatescriptEntryGameMessage",
        true,
        json!({
            "m_gameMessage":topic(NUMERIC_TOPIC),
            "m_params":[{"$":"STU_EF930FD7","m_B5051BCE":format!("0x{NUMERIC_PARAMETER:016X}"),"m_out_Var":variable(RECEIVED)}],
            "m_outPlug":plug(json!([]))
        }),
    );
    json!({"version":1,"missing":[],"bodies":{},"graphs":[{
        "index":GRAPH,"items":5,"fields":{},"containers":{},
        "nodes":[trigger(0, FOREIGN_SEND, 2),trigger(1, LOCAL_SEND, 3),
            send(2, variable(1)),send(3, json!({"$":"STU_B5A0CAF0"})),receiver],
        "entries":[0,1,4],"states":[],"remote":[],"sync_vars":[],"unknown_nodes":[],
        "header":{"m_nodesBitCount":3,"m_statesBitCount":3,"m_remoteSyncNodesBitCount":3,"m_syncVarsBitCount":3}
    }]})
}

fn enable_route(bundle: &mut Json) {
    bundle["runtime_state_metadata"] = json!({"directed_numeric_messages":[{
        "topic":format!("0x{NUMERIC_TOPIC:016X}"),"parameter":format!("0x{NUMERIC_PARAMETER:016X}")
    }]});
}

fn load(bundle: &Json) -> std::io::Result<GraphStore> {
    GraphStore::from_json(&serde_json::to_vec(bundle).unwrap())
}

fn start(graphs: Arc<GraphStore>, owner: u32, room: &mut Room) -> Component {
    Component::start(
        graphs,
        owner,
        Variables::new(),
        vec![Root::new(ROOT, GRAPH)],
        0,
        room,
        MAX_INIT_WORK,
    )
    .unwrap()
}

fn received(body: &Component) -> Option<&Value> {
    body.snapshot()?.instances[&ROOT].variables.get(&RECEIVED)
}

fn trigger(body: &mut Component, room: &mut Room, message: u64, target: Value, amount: Value) {
    assert_eq!(
        body.queue_message(ROOT, message, Variables::from([(1, target), (2, amount)]))
            .unwrap(),
        1
    );
    body.run_now(room).unwrap();
}

#[test]
fn opted_in_native_numeric_route_preserves_self_delivery_and_limits_room_delivery() {
    let mut bundle = bundle();
    let mut room = Room::default();
    let graphs = Arc::new(load(&bundle).unwrap());
    assert_eq!(graphs.room_spatial_needs(), RoomSpatialNeeds::None);
    let mut legacy = start(graphs, SOURCE, &mut room);
    trigger(
        &mut legacy,
        &mut room,
        FOREIGN_SEND,
        Value::Entity(RECIPIENT),
        Value::Float(2.5),
    );
    assert_eq!(room.legacy_attempts, 1);
    assert!(room.requests.is_empty());

    enable_route(&mut bundle);
    let graphs = Arc::new(load(&bundle).unwrap());
    assert_eq!(graphs.room_spatial_needs(), RoomSpatialNeeds::Queries);
    let mut sender = start(Arc::clone(&graphs), SOURCE, &mut room);
    let mut receiver = start(graphs, RECIPIENT, &mut room);

    trigger(
        &mut sender,
        &mut room,
        LOCAL_SEND,
        Value::Null,
        Value::Integer(3),
    );
    assert_eq!(received(&sender), Some(&Value::Integer(3)));
    assert!(room.requests.is_empty());
    for (target, amount) in [
        (
            Value::Array(vec![Value::Entity(RECIPIENT)]),
            Value::Integer(1),
        ),
        (Value::Entity(RECIPIENT), Value::Bool(true)),
        (Value::Entity(RECIPIENT), Value::Float(f64::NAN)),
    ] {
        trigger(&mut sender, &mut room, FOREIGN_SEND, target, amount);
        assert!(room.requests.is_empty());
    }
    trigger(
        &mut sender,
        &mut room,
        FOREIGN_SEND,
        Value::Entity(RECIPIENT),
        Value::Float(2.5),
    );
    let [(target, request)] = std::mem::take(&mut room.requests).try_into().unwrap();
    assert_eq!(target, RECIPIENT);
    assert_eq!(request.id, NUMERIC_TOPIC);
    assert_eq!(
        request.parameters,
        Variables::from([(NUMERIC_SLOT, Value::Float(2.5))])
    );

    let mut malformed = request.clone();
    malformed.parameters.insert(1, Value::Integer(99));
    assert_eq!(
        receiver.queue_room_message_from(SOURCE, malformed).unwrap(),
        0
    );
    assert_eq!(
        receiver.queue_room_message_from(SOURCE, request).unwrap(),
        1
    );
    receiver.run_now(&mut room).unwrap();
    assert_eq!(received(&receiver), None);
    receiver.advance_frame(1, &mut room).unwrap();
    assert_eq!(received(&receiver), Some(&Value::Float(2.5)));
    assert_eq!(room.legacy_attempts, 1);
}

#[test]
fn directed_numeric_metadata_rejects_missing_shapes_and_duplicate_topics() {
    let mut bundle = bundle();
    enable_route(&mut bundle);
    for (position, field, value) in [
        (2, "m_8B34E9C3", json!({"$":"STU_B5A0CAF0"})),
        (4, "m_filter", json!({"$":"STUConfigVarFilter"})),
    ] {
        let mut invalid = bundle.clone();
        invalid["graphs"][0]["nodes"][position]["fields"][field] = value;
        assert!(
            load(&invalid)
                .unwrap_err()
                .to_string()
                .contains("matching native sender and receiver")
        );
    }
    let routes = bundle["runtime_state_metadata"]["directed_numeric_messages"]
        .as_array_mut()
        .unwrap();
    routes.push(routes[0].clone());
    assert!(
        load(&bundle)
            .unwrap_err()
            .to_string()
            .contains("duplicate directed numeric message topic")
    );
}
