use super::*;
use crate::{
    bits::BitWriter,
    game_ingress::{GameIngress, MessageKind},
    game_parameters::CLIENT_WHEEL_ACTION,
    script_wire::Value as ScriptValue,
    vitals::Vitals,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    input: Input,
    expected: Expected,
}

#[derive(Deserialize)]
struct Input {
    action: String,
    foreign_asset: String,
    unvalidated_marker: u32,
    confirmed: Vec<bool>,
}

#[derive(Deserialize)]
struct Expected {
    topic: String,
    action_parameter: u32,
    confirmed_parameter: u32,
    delivered_with_current_body: usize,
    delivered_to_replacement: usize,
}

#[test]
fn delayed_spray_reports_require_a_recent_owner_transmission() {
    use crate::script_snapshot_wire::{ProjectedInstance, ProjectedState};
    use serde_json::Value as Json;
    use std::collections::BTreeSet;

    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/wheel-spray-use-report-v1.json"
    ))
    .unwrap();
    let history = &fixture["sent_history"];
    let input = &fixture["input"];
    let guid = |value: &Json| {
        u64::from_str_radix(value.as_str().unwrap().trim_start_matches("0x"), 16).unwrap()
    };
    let report = ClientWheelUse {
        entity: guid(&input["entity"]) as u32,
        instance: input["instance"].as_u64().unwrap() as u16,
        unlock: guid(&input["unlock"]),
    };
    let raw = history["state"].as_u64().unwrap() as u16;
    let start_ms = history["instance_start_ms"].as_u64().unwrap();
    let sent_frame = history["sent_frame"].as_u64().unwrap() as u32;
    let id = BodyId {
        entity: report.entity,
        generation: 1,
    };
    let runtime = synthetic_owner_with_vitals(id.entity, sent_frame, Arc::default(), None);
    let mut script = BodyScript::new(body(id), sent_frame, None, &runtime).unwrap();
    let empty = ProjectedSnapshot {
        entity: id.entity,
        view: View::Owner,
        entity_exclusions: BTreeSet::new(),
        variables: Default::default(),
        instances: BTreeMap::new(),
        callout: 0,
        callouts: Vec::new(),
        emit_events: true,
    };
    let bits = BitWriter::new();
    script.stream.create_arrived();
    let baseline = script.stream.full(&bits, empty.clone()).unwrap();
    script.sent(baseline.last);
    assert!(script.stream.arrived(baseline.last).advanced);

    let mut active = empty;
    active.instances.insert(
        report.instance,
        ProjectedInstance {
            graph: history["graph"].as_u64().unwrap() as u32,
            start_ms,
            parent: None,
            variables: Default::default(),
            states: BTreeMap::from([(
                raw,
                ProjectedState {
                    active: true,
                    payload: BTreeMap::from([("unlock".into(), ScriptValue::Asset(report.unlock))]),
                    link: None,
                    volley: None,
                },
            )]),
            events: Vec::new(),
        },
    );
    let chunk = script
        .stream
        .delta(
            &bits,
            active,
            BTreeSet::from([ChangeKey::State(report.instance, raw)]),
            DeltaKind::InitialOwner(sent_frame),
        )
        .unwrap();
    assert_eq!(script.sent_spray_origin(report), None);
    script.sent(chunk.last);
    assert!(script.stream.arrived(chunk.last).advanced);
    assert_eq!(script.sent_spray_origin(report), Some(start_ms));
    for wrong in [
        ClientWheelUse {
            unlock: report.unlock + 1,
            ..report
        },
        ClientWheelUse {
            entity: report.entity + 1,
            ..report
        },
        ClientWheelUse {
            instance: report.instance + 1,
            ..report
        },
    ] {
        assert_eq!(script.sent_spray_origin(wrong), None);
    }

    script.frame = history["last_eligible_frame"].as_u64().unwrap() as u32;
    assert_eq!(script.sent_spray_origin(report), Some(start_ms));
    script.frame = history["first_expired_frame"].as_u64().unwrap() as u32;
    assert_eq!(script.sent_spray_origin(report), None);
}

#[test]
fn wheel_input_follows_reliable_order_and_current_body_generation() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/wheel-selection-ingress-v1.json"
    ))
    .unwrap();
    let guid = |hex: &str| u64::from_str_radix(hex.trim_start_matches("0x"), 16).unwrap();
    let action = guid(&fixture.input.action);
    let topic = guid(&fixture.expected.topic);
    let schemas = crate::jam_codec::Schemas::from_json(
        r#"{"A7FBBD0C":{"21608":[
            {"type":8,"off":120,"size":8,"count":0,"array":false},
            {"type":0,"off":128,"size":1,"count":0,"array":false},
            {"type":14,"off":132,"size":4,"count":0,"array":false,
             "fields":[{"type":6,"off":0,"size":4,"count":0,"array":false}]}
        ]}}"#,
    )
    .unwrap();
    let packet = |ordinal, asset, confirmed, marker| {
        let fields = Fields::from([
            (0x78, Value::Unsigned(asset)),
            (0x80, Value::Bool(confirmed)),
            (
                0x84,
                Value::Struct(Fields::from([(0, Value::Unsigned(u64::from(marker)))])),
            ),
        ]);
        let encoded = crate::game_messages::encode(&schemas, CLIENT_WHEEL_ACTION, &fields).unwrap();
        let mut packet = BitWriter::new();
        crate::game_messages::write_channel(&mut packet, &[(ordinal, encoded)], &[]).unwrap();
        packet.bit(false);
        packet
    };
    let card = PlayerCard::default();
    let viewer = PlayerAppearance {
        entity: OWNER,
        slot: 0,
        team: 0,
        card: &card,
    };
    let mut scene = PreparedScene::fixture_for_owner_admission(Default::default());
    let mut world = WorldDelivery::new(OWNER);
    let original = BodyId {
        entity: OWNER | 1,
        generation: 1,
    };
    let spawn = |scene: &mut PreparedScene, id| {
        let mut state = fixture_player_state(id, OWNER, 0, 0, [0.0; 3], 200.0, false);
        let runtime = synthetic_owner_with_vitals(
            state.id.entity,
            100,
            Arc::default(),
            Some(Vitals::new(200.0, 0.0, 0.0)),
        );
        let received = Arc::clone(&runtime.received_room_messages);
        state.runtime = Some(Box::new(runtime));
        scene.simulation_mut().spawn_player_body(state).unwrap();
        received
    };
    let recorded = spawn(&mut scene, original);
    world.started = Some(100);
    world.body = Some(body(original));
    world.created.insert(original.entity);

    let mut ingress = GameIngress::new();
    let mut receive = |world: &mut WorldDelivery, scene: &PreparedScene, packet: BitWriter| {
        ingress
            .receive(&schemas, packet.as_bytes(), |message, command_frame| {
                assert_eq!(MessageKind::from(message.id), MessageKind::WheelAction);
                world.queue_wheel_action(scene.simulation(), &message, command_frame);
            })
            .unwrap();
    };
    receive(&mut world, &scene, packet(0, action, true, 0)); // possession not delivered
    world.bound = Some(original);
    receive(
        &mut world,
        &scene,
        packet(1, guid(&fixture.input.foreign_asset), true, 0),
    );
    receive(
        &mut world,
        &scene,
        packet(2, action, true, fixture.input.unvalidated_marker),
    );
    assert!(!world.has_input_work());

    for (index, &confirmed) in fixture.input.confirmed.iter().enumerate().rev() {
        receive(
            &mut world,
            &scene,
            packet(3 + index as u32, action, confirmed, 0),
        );
    }
    receive(&mut world, &scene, packet(3, action, false, 0)); // reliable retry
    let used = world
        .advance_input(&mut scene, &viewer, RETAIL_QUANTA, 10)
        .unwrap();
    assert_eq!(used, (fixture.expected.delivered_with_current_body, false));
    let expected = |confirmed| RoomMessage {
        id: topic,
        parameters: crate::script_wire::Variables::from([
            (
                fixture.expected.action_parameter,
                ScriptValue::Asset(action),
            ),
            (
                fixture.expected.confirmed_parameter,
                ScriptValue::Bool(confirmed),
            ),
        ]),
    };
    assert_eq!(
        *recorded.lock().unwrap(),
        fixture
            .input
            .confirmed
            .iter()
            .map(|&confirmed| (original.entity, expected(confirmed)))
            .collect::<Vec<_>>()
    );

    receive(&mut world, &scene, packet(5, action, true, 0));
    let replacement = BodyId {
        generation: 2,
        ..original
    };
    let new_recorded = spawn(&mut scene, replacement);
    world.body = Some(body(replacement));
    world.bound = Some(replacement);
    assert_eq!(
        world
            .advance_input(&mut scene, &viewer, RETAIL_QUANTA, 10)
            .unwrap(),
        (1, false)
    );
    assert_eq!(
        new_recorded.lock().unwrap().len(),
        fixture.expected.delivered_to_replacement
    );

    receive(&mut world, &scene, packet(6, action, true, 0));
    scene
        .simulation_mut()
        .damage_player_body_by_wire(
            replacement,
            f64::MAX,
            Death {
                time: 0.0,
                frame: 100,
                killer: 0,
            },
            None,
        )
        .unwrap();
    assert_eq!(
        world
            .advance_input(&mut scene, &viewer, RETAIL_QUANTA, 10)
            .unwrap(),
        (1, false)
    );
    assert!(new_recorded.lock().unwrap().is_empty());
}
