use std::{fs::File, path::PathBuf, sync::Arc};

use flate2::read::GzDecoder;
use serde_json::{Value as Json, json};

use crate::{
    script_expr::{Scope, Variable, to_float},
    script_graph::GraphStore,
    script_init::{Component, MAX_INIT_WORK, RoomMessage, Root, Site, World},
    script_wire::{Value, Variables},
};

const STOCK_CHARGE_GRAPH: u32 = 0x0b8b;
const RECEIVER_TOPIC: u64 = 0x0240_0000_0000_14f2;
const FRACTION_PARAMETER: u32 = 0x0951;
const FRACTION_BINDING: u32 = 32280;
const METER_CAP: u32 = 338;
const METER: u32 = 342;
const ENTITY_GATE: u32 = 32365;
const RULESET_GATE: u32 = 13024;
const OWNER: u32 = 0xa000_0101;
const INSTANCE: u16 = 1;
const CAP: f64 = 12.0;
const INITIAL: f64 = 9.0;

#[derive(Default)]
struct ChargeReceiverHost {
    ruleset_blocks: bool,
    outgoing: Vec<(u32, RoomMessage)>,
}

impl World for ChargeReceiverHost {
    fn ruleset(&mut self, id: u32) -> Option<Value> {
        (id == RULESET_GATE).then_some(Value::Bool(self.ruleset_blocks))
    }

    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }

    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn queue_room_message(&mut self, _: Site, target: u32, message: RoomMessage) -> bool {
        self.outgoing.push((target, message));
        true
    }
}

fn start_receiver(
    graphs: Arc<GraphStore>,
    entity_blocks: bool,
    world: &mut ChargeReceiverHost,
) -> Component {
    Component::start(
        graphs,
        OWNER,
        Variables::from([
            (METER_CAP, Value::Float(CAP)),
            (METER, Value::Float(INITIAL)),
            (ENTITY_GATE, Value::Bool(entity_blocks)),
        ]),
        vec![Root::listener(INSTANCE, STOCK_CHARGE_GRAPH, None)],
        0,
        world,
        MAX_INIT_WORK,
    )
    .unwrap()
}

fn graph_pack() -> PathBuf {
    PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").expect("set ORPHEUS_RUNTIME_DIR"))
        .join("data/game_graphs_174.json.gz")
}

#[test]
#[ignore = "requires an authorized 104319 graph pack in ORPHEUS_RUNTIME_DIR"]
fn prepared_stock_14f2_sets_absolute_charge_fraction_and_respects_gates() {
    let graphs = Arc::new(GraphStore::load(graph_pack()).unwrap());
    let fraction_binding = Variable {
        scope: Scope::Instance,
        id: FRACTION_BINDING,
    };
    for (case, fraction, entity_blocks, ruleset_blocks, expected) in [
        ("absolute reduction", 0.25, false, false, 3.0),
        ("exact cap", 1.0, false, false, CAP),
        ("over cap", 1.25, false, false, CAP),
        ("entity gate", 0.25, true, false, INITIAL),
        ("ruleset gate", 0.25, false, true, INITIAL),
    ] {
        let mut world = ChargeReceiverHost {
            ruleset_blocks,
            ..ChargeReceiverHost::default()
        };
        let mut body = start_receiver(Arc::clone(&graphs), entity_blocks, &mut world);

        // Exercise the original receiver with an explicit input. This detached
        // probe supplies no evidence of the engine's producer or timing.
        let parameters = Variables::from([(FRACTION_PARAMETER, Value::Float(fraction))]);
        assert_eq!(
            body.queue_message(INSTANCE, RECEIVER_TOPIC, parameters)
                .unwrap(),
            1,
            "{case}"
        );
        body.run_now(&mut world).unwrap();
        assert_eq!(
            body.resolved_variable(INSTANCE, fraction_binding),
            Some(Value::Float(fraction)),
            "{case}"
        );
        let snapshot = body.snapshot().unwrap();
        assert_eq!(to_float(&snapshot.variables[&METER]), expected, "{case}");
        assert!(
            snapshot.diagnostics.is_empty(),
            "{case}: {:?}",
            snapshot.diagnostics
        );
    }
}

#[test]
#[ignore = "requires an authorized 104319 graph pack in ORPHEUS_RUNTIME_DIR"]
fn prepared_stock_14f2_entity_sender_reaches_the_receiver_on_its_next_frame() {
    const STOCK_SENDER_GRAPH: u32 = 0x0b49;
    const STOCK_SEND_POSITION: u16 = 30;
    const SENDER_GRAPH: u32 = 61248;
    const SENDER: u32 = 0xa000_0102;
    const TARGET_BINDING: u32 = 2171;
    const TRIGGER: u64 = 0x0240_0000_0000_fa1b;
    const PARAMETER_ID: u64 = 0x0a40_0000_0000_0951;
    const FRACTION: f64 = 0.5;

    let file = File::open(graph_pack()).unwrap();
    let mut bundle: Json = serde_json::from_reader(GzDecoder::new(file)).unwrap();
    let graphs = bundle["graphs"].as_array_mut().unwrap();
    let stock_send = graphs
        .iter()
        .find(|graph| graph["index"] == STOCK_SENDER_GRAPH)
        .expect("stock sender graph")["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["pos"] == STOCK_SEND_POSITION)
        .expect("stock sender node")
        .clone();
    assert_eq!(stock_send["class"], "STUStatescriptActionSendGameMessage");

    // Isolate the unchanged terminal action. Only the trigger and caller
    // values are synthetic. The pack is amended in memory for this probe.
    graphs.push(json!({
        "index": SENDER_GRAPH, "items": STOCK_SEND_POSITION + 1,
        "fields": {}, "containers": {}, "states": [], "entries": [0],
        "remote": [], "sync_vars": [], "unknown_nodes": [],
        "header": {"m_nodesBitCount": 5, "m_statesBitCount": 1,
            "m_remoteSyncNodesBitCount": 1, "m_syncVarsBitCount": 1},
        "nodes": [{
            "pos": 0, "rid": 0, "uid": 0, "class": "STUStatescriptEntryGameMessage",
            "state": null, "remote": -1, "client_only": false, "server_only": true,
            "fields": {
                "m_gameMessage": {"$": "STUConfigVarGameMessage",
                    "m_gameMessage": format!("0x{TRIGGER:016X}")},
                "m_outPlug": {"$": "Plug", "links": [[STOCK_SEND_POSITION, "m_inPlug"]]}
            }
        }, stock_send]
    }));
    let metadata = bundle
        .as_object_mut()
        .unwrap()
        .entry("runtime_state_metadata")
        .or_insert_with(|| json!({}));
    let routes = metadata
        .as_object_mut()
        .unwrap()
        .entry("directed_numeric_messages")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .unwrap();
    let route = json!({
        "topic": format!("0x{RECEIVER_TOPIC:016X}"),
        "parameter": format!("0x{PARAMETER_ID:016X}")
    });
    if !routes.contains(&route) {
        routes.push(route);
    }
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap());

    let mut world = ChargeReceiverHost::default();
    let mut root = Root::listener(INSTANCE, SENDER_GRAPH, None);
    root.variables
        .insert(FRACTION_BINDING, Value::Float(FRACTION));
    let mut sender = Component::start(
        Arc::clone(&graphs),
        SENDER,
        Variables::from([(TARGET_BINDING, Value::Entity(OWNER))]),
        vec![root],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert_eq!(
        sender
            .queue_message(INSTANCE, TRIGGER, Variables::new())
            .unwrap(),
        1
    );
    sender.run_now(&mut world).unwrap();
    let [(target, message)]: [(u32, RoomMessage); 1] =
        std::mem::take(&mut world.outgoing).try_into().unwrap();
    assert_eq!(target, OWNER);
    assert_eq!(
        message,
        RoomMessage {
            id: RECEIVER_TOPIC,
            parameters: Variables::from([(FRACTION_PARAMETER, Value::Float(FRACTION))]),
        }
    );

    let mut receiver = start_receiver(graphs, false, &mut world);
    assert_eq!(
        receiver.queue_room_message_from(SENDER, message).unwrap(),
        1
    );
    receiver.run_now(&mut world).unwrap();
    assert_eq!(
        to_float(&receiver.snapshot().unwrap().variables[&METER]),
        INITIAL
    );
    receiver.advance_frame(1, &mut world).unwrap();
    assert_eq!(
        to_float(&receiver.snapshot().unwrap().variables[&METER]),
        6.0
    );
    for body in [&sender, &receiver] {
        let diagnostics = &body.snapshot().unwrap().diagnostics;
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
}
