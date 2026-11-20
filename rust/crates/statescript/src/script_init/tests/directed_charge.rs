use serde::Deserialize;

use super::*;
use crate::script_expr::EntityRelation;

const ROOT: u16 = 1;
const SENDER_GRAPH: u32 = 0xf195;
const RECEIVER_GRAPH: u32 = 0xf196;
const TOPIC: &str = "0x024000000000F284";
const PARAMETER: &str = "0x0A4000000000F285";
const TARGET_SLOT: u32 = 72;
const AMOUNT_SLOT: u32 = 73;
const RECEIVED_SLOT: u32 = 74;

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    evidence: String,
    input: Input,
}

#[derive(Deserialize)]
struct Input {
    source: u32,
    recipient: u32,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    possession: bool,
    fraction: f64,
}

#[derive(Default)]
struct MessageWorld {
    possession: Option<(u32, u32)>,
    requests: Vec<(Site, u32, RoomMessage)>,
}

impl World for MessageWorld {
    fn related_entity(&mut self, _: Site, relation: EntityRelation, entity: u32) -> Option<u32> {
        let (controller, body) = self.possession?;
        (relation == EntityRelation::PossessedBody && controller == entity).then_some(body)
    }

    fn queue_room_message(&mut self, site: Site, target: u32, message: RoomMessage) -> bool {
        self.requests.push((site, target, message));
        true
    }

    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }
}

fn graphs(target: Json) -> Arc<GraphStore> {
    let message = json!({"$":"STUConfigVarGameMessage", "m_gameMessage":TOPIC});
    let mut sender = node(
        1,
        "STUStatescriptActionSendGameMessage",
        None,
        json!({
            "m_gameMessage":message, "m_targets":target, "m_outPlug":plug(&[], &[]),
            "m_params":[{"$":"STU_814D595E", "m_B5051BCE":PARAMETER,
                "m_value":var(AMOUNT_SLOT, false)}]
        }),
    );
    sender["server_only"] = json!(true);
    let mut receiver = node(
        0,
        "STUStatescriptEntryGameMessage",
        None,
        json!({
            "m_gameMessage":message, "m_outPlug":plug(&[], &[]),
            "m_params":[{"$":"STU_EF930FD7", "m_B5051BCE":PARAMETER,
                "m_out_Var":var(RECEIVED_SLOT, false)}]
        }),
    );
    receiver["server_only"] = json!(true);
    let bundle = json!({
        "version":1, "bodies":{}, "missing":[],
        "graphs":[
            graph(SENDER_GRAPH, vec![
                node(0, "STUStatescriptEntry", None,
                    json!({"m_outPlug":plug(&[(1, "m_inPlug")], &[])})),
                sender,
            ], json!([]), json!([0])),
            graph(RECEIVER_GRAPH, vec![receiver], json!([]), json!([0])),
        ],
        "runtime_state_metadata":{
            "directed_numeric_messages":[{"topic":TOPIC, "parameter":PARAMETER}]
        }
    });
    Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap())
}

#[test]
fn server_only_directed_charge_routes_bind_numeric_amount_on_receiver_frame() {
    let contract: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-directed-charge-sender-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.evidence, "synthetic-rust");
    let Input {
        source,
        recipient,
        cases,
    } = contract.input;
    for Case {
        possession,
        fraction,
    } in cases
    {
        let target = if possession {
            json!({"$":"STU_0AC15F78", "m_entity":{"$":"STU_B5A0CAF0"}})
        } else {
            var(TARGET_SLOT, true)
        };
        let graphs = graphs(target);
        let mut world = MessageWorld {
            possession: possession.then_some((source, recipient)),
            ..MessageWorld::default()
        };
        let mut root = Root::new(ROOT, SENDER_GRAPH);
        root.variables.insert(AMOUNT_SLOT, Value::Float(fraction));
        let sender = Component::start(
            Arc::clone(&graphs),
            source,
            Variables::from([(TARGET_SLOT, Value::Entity(recipient))]),
            vec![root],
            0,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        assert!(sender.snapshot().unwrap().diagnostics.is_empty());
        let [(site, target, message)] = world.requests.as_slice() else {
            panic!("expected a single directed request");
        };
        assert_eq!(
            *site,
            Site {
                entity: source,
                instance: ROOT,
                graph: SENDER_GRAPH,
                node: Some(1),
            }
        );
        assert_eq!(*target, recipient);
        assert_eq!(message.parameters[&0xf285], Value::Float(fraction));

        let mut receiver = Component::start(
            graphs,
            recipient,
            Variables::new(),
            vec![Root::listener(ROOT, RECEIVER_GRAPH, None)],
            0,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        let (_, _, message) = world.requests.pop().unwrap();
        assert_eq!(
            receiver.queue_room_message_from(source, message).unwrap(),
            1
        );
        assert!(
            !receiver.snapshot().unwrap().instances[&ROOT]
                .variables
                .contains_key(&RECEIVED_SLOT)
        );
        receiver.advance_frame(1, &mut world).unwrap();
        let snapshot = receiver.snapshot().unwrap();
        assert!(snapshot.diagnostics.is_empty());
        assert_eq!(
            snapshot.instances[&ROOT].variables[&RECEIVED_SLOT],
            Value::Float(fraction)
        );
    }
}
