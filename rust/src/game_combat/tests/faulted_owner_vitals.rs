//! The room preserves established body health across owner script unload.

use std::{sync::Arc, time::Duration};

use serde::Deserialize;
use serde_json::json;

use super::{fixture_bots, fixture_ground};
use crate::{
    contact_world::ContactWorld,
    game_combat::{PlayerBodyState, RoomScriptRequests, RoomSimulation, health::HealthEffect},
    game_content::PRACTICE_RANGE_MODE,
    game_heroes::{Hero, SOLDIER},
    game_instance::RETAIL_QUANTA,
    game_pools::HealthPools,
    game_replication::BodyId,
    match_scene::team_bit,
    mover::Mover,
    mover_settings::MoverSettings,
    script_codecs::CodecTable,
    script_graph::{GraphStore, RoomSpatialNeeds},
    script_player::{OwnerBodySpawn, PlayerBodyFactory},
    vitals::{Death, PoolKind},
    world::{Component, EntityUpdate, Movement, Operation},
};

#[derive(Deserialize)]
struct Contract {
    schema_version: u32,
    input: Input,
    expected: Expected,
}

#[derive(Deserialize)]
struct Input {
    base_health: f64,
    initial_shields: f64,
    damage_before_unload: f64,
    damage_after_unload: f64,
    healing_after_unload: f64,
    lethal_damage: f64,
}

#[derive(Deserialize)]
struct Expected {
    maximum_health: f64,
    health_before_unload: f64,
    health_after_damage: f64,
    health_after_healing: f64,
}

fn native_factory(pools: HealthPools) -> PlayerBodyFactory {
    let hero = Hero {
        guid: SOLDIER,
        name: "Health fixture".into(),
        role: "Damage".into(),
        body: 0x0400_0000_0000_b193,
    };
    let root = 42;
    let bundle = json!({
        "version": 1, "missing": [],
        "graphs": [{
            "index": root, "items": 0, "nodes": [], "states": [], "entries": [], "remote": [],
            "sync_vars": [], "containers": {}, "unknown_nodes": [], "fields": {},
            "header": {"m_nodesBitCount": 4, "m_statesBitCount": 4,
                "m_remoteSyncNodesBitCount": 4, "m_syncVarsBitCount": 4}
        }],
        "bodies": {format!("0x{:016X}", hero.body): {
            "hero": format!("0x{:016X}", hero.guid), "name": &hero.name,
            "graphs": [{"graph": root, "m_75408640": 0}], "manager": root, "weapons": [null, root]
        }}
    });
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap());
    let codecs = Arc::new(CodecTable::from_json(r#"{"classes":{}}"#).unwrap());
    PlayerBodyFactory::from_body(
        graphs,
        codecs,
        PRACTICE_RANGE_MODE,
        &hero,
        Some(pools),
        false,
    )
    .unwrap()
}

fn assert_health_update(
    room: &mut RoomSimulation,
    id: BodyId,
    current: f64,
    maximum: f64,
) -> Component {
    let vitals = room.player_body(id).unwrap().vitals().unwrap();
    assert_eq!(vitals.total(), current);
    assert_eq!(vitals.maximum(), maximum);
    let component = vitals.cumulative_component();
    assert_eq!(
        room.take_scripted_health(),
        [EntityUpdate {
            entity: id.entity,
            operation: Some(Operation::Update(vec![component.clone()])),
            ..Default::default()
        }],
    );
    component
}

fn settle_authored_health(
    room: &mut RoomSimulation,
    source: u32,
    id: BodyId,
    tick: u32,
    delta: f64,
) {
    let frame = room.capture_spatial_frame(tick).unwrap().unwrap();
    let mut outbox = RoomScriptRequests::default();
    assert!(outbox.queue_health(&frame, id.entity, delta));
    let source = room.sample_health_body(source).unwrap();
    for request in outbox.take() {
        room.settle_script_request(request, source, tick, Duration::ZERO)
            .unwrap();
    }
}

#[test]
fn scripted_owner_unload_keeps_room_health_and_death_on_the_same_body() {
    let contract: Contract = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/owner-script-fault-vitals-v1.json"
    ))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    let Input {
        base_health,
        initial_shields,
        damage_before_unload,
        damage_after_unload,
        healing_after_unload,
        lethal_damage,
    } = contract.input;
    let Expected {
        maximum_health,
        health_before_unload,
        health_after_damage,
        health_after_healing,
    } = contract.expected;

    let factory = native_factory(HealthPools {
        health: base_health,
        shields: initial_shields,
        armour: 0.0,
    });
    let spawn = OwnerBodySpawn {
        controller: 0xa000_0100,
        entity: 0xa000_0101,
        body_filter: team_bit(0).unwrap(),
        slot: 0,
        frame: 100,
        pose: Movement::default(),
        eye_position: None,
        quanta: RETAIL_QUANTA,
    };
    let bots = fixture_bots();
    let source = bots[0].entity;
    let mut room = RoomSimulation::new(bots).unwrap();
    room.enable_spatial_queries(
        false,
        Arc::new(fixture_ground()),
        RoomSpatialNeeds::EntityFacts,
    );
    let contact = Arc::new(ContactWorld::new(Vec::new()).unwrap());
    let make_mover = || {
        Mover::new(
            MoverSettings::default(),
            Arc::clone(&contact),
            spawn.pose.position,
            0,
            0x08,
        )
        .unwrap()
    };
    let id = BodyId {
        entity: spawn.entity,
        generation: 1,
    };
    let handle = room
        .spawn_player_body(PlayerBodyState::new(
            id,
            spawn.controller,
            spawn.slot,
            0,
            SOLDIER,
            make_mover(),
            Some(Box::new(factory.start(spawn).unwrap())),
            spawn.frame,
            None,
        ))
        .unwrap();
    settle_authored_health(&mut room, source, id, spawn.frame, -damage_before_unload);
    let retained = assert_health_update(&mut room, id, health_before_unload, maximum_health);

    assert!(room.clear_player_runtime(id));
    let body = room.player_body(id).unwrap();
    assert!(body.runtime.is_none());
    assert!(body.alive());
    let vitals = body.vitals().unwrap();
    assert_eq!(vitals.cumulative_component(), retained);
    assert_eq!(
        vitals.amount(Some(PoolKind::Shields), true),
        initial_shields
    );
    assert_eq!(vitals.amount(Some(PoolKind::Shields), false), 0.0);
    assert_eq!(
        room.sample_health_body(id.entity).unwrap().entity,
        handle.entity
    );

    settle_authored_health(&mut room, source, id, spawn.frame + 1, -damage_after_unload);
    // The unchanged shield slot must be resent if the last revision was not acknowledged.
    assert_health_update(&mut room, id, health_after_damage, maximum_health);
    settle_authored_health(&mut room, source, id, spawn.frame + 2, healing_after_unload);
    assert_health_update(&mut room, id, health_after_healing, maximum_health);
    assert!(room.take_player_deaths().is_empty());

    let lethal_tick = spawn.frame + 3;
    settle_authored_health(&mut room, source, id, lethal_tick, -lethal_damage);
    assert_health_update(&mut room, id, 0.0, maximum_health);
    let deaths = room.take_player_deaths();
    assert_eq!(deaths.len(), 1);
    assert_eq!(deaths[0].owner, spawn.controller);
    assert_eq!(deaths[0].body, id);
    assert_eq!(deaths[0].source, source);
    assert!(deaths[0].killed_by);

    assert!(room.clear_player_runtime(id));
    let body = room.player_body(id).unwrap();
    assert!(!body.alive());
    let death = Death {
        time: 0.0,
        frame: lethal_tick,
        killer: source,
    };
    assert_eq!(body.vitals().unwrap().death(), Some(death));
    assert_eq!(room.player_handle_for_wire(id.entity), Some(id));

    let unmodeled = BodyId {
        entity: spawn.entity + 0x100,
        generation: 1,
    };
    room.spawn_player_body(PlayerBodyState::new(
        unmodeled,
        spawn.controller + 0x100,
        spawn.slot + 1,
        0,
        SOLDIER,
        make_mover(),
        None,
        spawn.frame,
        None,
    ))
    .unwrap();
    assert!(room.clear_player_runtime(unmodeled));
    assert!(room.player_body(unmodeled).unwrap().vitals().is_none());
    assert_eq!(
        room.player_handle_for_wire(unmodeled.entity),
        Some(unmodeled)
    );

    let frame = room
        .capture_spatial_frame(lethal_tick + 1)
        .unwrap()
        .unwrap();
    assert!(!frame.entity_facts().unwrap()[&id.entity].alive);
    let mut outbox = RoomScriptRequests::default();
    assert!(!outbox.queue_health(&frame, id.entity, healing_after_unload));
    assert!(!outbox.queue_health(&frame, unmodeled.entity, healing_after_unload));
    assert!(
        room.settle_health(
            id.entity,
            HealthEffect::Heal(healing_after_unload),
            death,
            Duration::ZERO
        )
        .unwrap()
    );
    assert_eq!(room.player_body(id).unwrap().vitals().unwrap().total(), 0.0);
    assert!(room.take_scripted_health().is_empty());
    assert!(room.take_player_deaths().is_empty());
}
