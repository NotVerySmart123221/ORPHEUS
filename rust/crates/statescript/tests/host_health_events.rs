use std::sync::Arc;

use orpheus_statescript::{
    script_graph::GraphStore,
    script_init::{Component, EmptyWorld, HitTags, HostEvent, MAX_INIT_WORK, RoomMessage, Root},
    script_wire::{Value, Variables},
};
use serde::Deserialize;
use serde_json::Value as Json;

const ROOT: u16 = 1;
const GRAPH: u32 = 302;

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    case_id: String,
    evidence: String,
    input: Input,
    expected: Expected,
}

#[derive(Deserialize)]
struct Input {
    bundle: Json,
    message: String,
    forged_delta: f64,
    source: u32,
    target: u32,
    unrelated: u32,
    events: Vec<Event>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Recipient {
    Source,
    Target,
    Unrelated,
    WithoutListener,
}

#[derive(Deserialize)]
struct Event {
    name: String,
    recipient: Recipient,
    source: Option<u32>,
    target: u32,
    delta: Json,
    killed: bool,
    hit_tags: Option<Vec<u64>>,
}

type Delivery = (Option<u32>, u32, f64, bool, Option<Vec<u64>>);

#[derive(Deserialize)]
struct Expected {
    ordinary_variable: u32,
    room_variable: u32,
    host_variables: [u32; 6],
    deliveries: Vec<Option<Delivery>>,
}

fn variables(body: &Component) -> Option<&Variables> {
    Some(&body.snapshot()?.instances.get(&ROOT)?.variables)
}

#[test]
fn host_health_entries_only_receive_settled_events_on_the_next_frame() {
    let fixture: Contract = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/script-host-health-events-v1.json"
    ))
    .unwrap();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.case_id, "script-host-health-events.synthetic.v1");
    assert_eq!(fixture.evidence, "synthetic-rust");

    let input = fixture.input;
    let expected = fixture.expected;
    let graphs =
        Arc::new(GraphStore::from_json(&serde_json::to_vec(&input.bundle).unwrap()).unwrap());
    let start = |entity, subscribed| {
        let roots = if subscribed {
            vec![Root::new(ROOT, GRAPH)]
        } else {
            Vec::new()
        };
        Component::start(
            Arc::clone(&graphs),
            entity,
            Variables::new(),
            roots,
            0,
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap()
    };
    let mut target = start(input.target, true);
    let mut source = start(input.source, true);
    let mut unrelated = start(input.unrelated, true);
    let mut without_listener = start(input.target, false);

    assert!(variables(&target).unwrap().is_empty());
    let message = u64::from_str_radix(input.message.strip_prefix("0x").unwrap(), 16).unwrap();
    let forged = Variables::from([
        (1, Value::Entity(input.source)),
        (2, Value::Entity(input.target)),
        (3, Value::Float(input.forged_delta)),
        (4, Value::Bool(true)),
    ]);
    assert_eq!(
        target.queue_message(ROOT, message, forged.clone()).unwrap(),
        1
    );
    target.run_now(&mut EmptyWorld).unwrap();
    assert_eq!(
        variables(&target).unwrap(),
        &Variables::from([(expected.ordinary_variable, Value::Float(input.forged_delta))])
    );
    assert_eq!(
        target
            .queue_room_message_from(
                input.source,
                RoomMessage {
                    id: message,
                    parameters: forged,
                },
            )
            .unwrap(),
        1
    );
    target.advance_frame(1, &mut EmptyWorld).unwrap();
    assert_eq!(
        variables(&target).unwrap(),
        &Variables::from([
            (expected.ordinary_variable, Value::Float(input.forged_delta)),
            (expected.room_variable, Value::Float(input.forged_delta)),
        ])
    );

    assert_eq!(input.events.len(), expected.deliveries.len());
    for (index, (event, delivery)) in input
        .events
        .into_iter()
        .zip(expected.deliveries)
        .enumerate()
    {
        let recipient = match event.recipient {
            Recipient::Source => &mut source,
            Recipient::Target => &mut target,
            Recipient::Unrelated => &mut unrelated,
            Recipient::WithoutListener => &mut without_listener,
        };
        let frame = u32::try_from(index).unwrap() * 2 + 2;
        recipient.advance_frame(frame, &mut EmptyWorld).unwrap();
        let before = variables(recipient).cloned();
        let delta = match event.delta.as_str() {
            Some("nan") => f64::NAN,
            Some("infinity") => f64::INFINITY,
            _ => event.delta.as_f64().unwrap(),
        };
        let hit_tags = event.hit_tags.map(|tags| {
            let mut hit_tags = HitTags::EMPTY;
            for tag in tags {
                assert!(hit_tags.push(tag));
            }
            hit_tags
        });
        assert_eq!(
            recipient
                .queue_host_event(HostEvent::HealthApplied {
                    source: event.source,
                    target: event.target,
                    delta,
                    killed: event.killed,
                    hit_tags,
                })
                .unwrap(),
            usize::from(delivery.is_some()),
            "{}",
            event.name
        );
        recipient.run_now(&mut EmptyWorld).unwrap();
        assert_eq!(variables(recipient), before.as_ref(), "{}", event.name);
        recipient.advance_frame(frame + 1, &mut EmptyWorld).unwrap();

        let mut after = before;
        if let Some((source, target, delta, killed, tags)) = delivery {
            let after = after.as_mut().unwrap();
            for (variable, value) in expected.host_variables.into_iter().zip([
                source.map_or(Value::Null, Value::Entity),
                Value::Entity(target),
                Value::Float(delta),
                Value::Bool(killed),
                Value::Float(delta),
                tags.map_or(Value::Null, |tags| {
                    Value::Array(tags.into_iter().map(Value::Asset).collect())
                }),
            ]) {
                after.insert(variable, value);
            }
        }
        assert_eq!(variables(recipient), after.as_ref(), "{}", event.name);
    }
}
