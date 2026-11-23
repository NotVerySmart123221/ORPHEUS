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
