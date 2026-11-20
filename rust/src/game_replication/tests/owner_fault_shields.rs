use super::*;
use crate::{
    script_runtime::{HEALTH_POOL, SHIELDS_POOL},
    vitals::{PoolKind, Vitals},
    world::{POOLS, pool, pool_component},
};

#[test]
fn owner_fault_clears_named_shields_for_both_views_and_retries_the_same_clear() {
    let contract: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/owner-fault-temporary-shields-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "owner-fault-temporary-shields-v1");
    let input = &contract["input"];
    let expected = &contract["expected"];
    let [health_max, health_current]: [f64; 2] =
        serde_json::from_value(input["base_health"].clone()).unwrap();
    let [shield_max, shield_current]: [f64; 2] =
        serde_json::from_value(input["ordinary_shields"].clone()).unwrap();
    let [first, stacked]: [f64; 2] =
        serde_json::from_value(input["stacked_grants"].clone()).unwrap();
    let depleted = input["depleted_grant"].as_f64().unwrap();
    let [first_slot, second_slot]: [usize; 2] =
        serde_json::from_value(expected["cleared_slots"].clone()).unwrap();
    let mut vitals = Vitals::empty();
    let health = vitals
        .add_pool(PoolKind::Health, health_max, health_current)
        .unwrap();
    let shields = vitals
        .add_pool(PoolKind::Shields, shield_max, shield_current)
        .unwrap();
    let active_identifier = 0x0d80_0000_0000_0111;
    let spent_identifier = 0x0d80_0000_0000_0112;
    let grants = [
        vitals
            .add_temporary_shield(active_identifier, first, false)
            .unwrap(),
        vitals
            .add_temporary_shield(active_identifier, stacked, true)
            .unwrap(),
        vitals
            .add_temporary_shield(spent_identifier, depleted, false)
            .unwrap(),
    ];
    assert_eq!(
        vitals.drain_temporary_shield(spent_identifier, depleted),
        0.0
    );
    assert_eq!(vitals.pool_current(grants[2]), Some(0.0));
    assert!(vitals.total() > expected["current"].as_f64().unwrap());
    assert!(vitals.take_cumulative_update().is_some()); // already advertised before the fault

    let id = avatar(1, 1);
    let mut world = WorldDelivery::new(OWNER);
    world.started = Some(FIRST_TICK);
    let body = body(id);
    world.body = Some(body);
    world
        .bodies
        .insert(id.entity, (id.generation, Lifetime::Live));
    world.created.insert(id.entity);
    let failed = Arc::new(AtomicBool::new(false));
    let runtime =
        synthetic_owner_with_vitals(id.entity, FIRST_TICK, Arc::clone(&failed), Some(vitals));
    world.body_script = Some(BodyScript::new(body, FIRST_TICK, None, &runtime).unwrap());
    let mut room = RoomSimulation::new(Vec::new()).unwrap();
    room.spawn_player_body(PlayerBodyState::new(
        id,
        OWNER,
        0,
        0,
        body.hero,
        empty_mover(),
        Some(Box::new(runtime)),
        FIRST_TICK,
        None,
    ))
    .unwrap();

    failed.store(true, Ordering::Relaxed);
    let mut egress = GameEgress::default();
    let (_, sent) = transmit(&mut world, Some(&mut room), &mut egress, FIRST_TICK + 1, 1);
    assert_eq!(sent.records.len(), 1);
    assert_eq!(world.owner_script_fault.as_ref().unwrap().id, id);
    let retained = room.player_body(id).unwrap();
    assert!(retained.runtime.is_none());
    assert!(retained.alive());
    let vitals = retained.vitals().unwrap();
    assert_eq!(vitals.total(), expected["current"].as_f64().unwrap());
    assert_eq!(vitals.maximum(), expected["maximum"].as_f64().unwrap());
    assert_eq!(vitals.pool_current(health), Some(health_current));
    assert_eq!(vitals.pool_current(shields), Some(shield_current));
    assert!(
        grants
            .into_iter()
            .all(|grant| vitals.pool_current(grant).is_none())
    );
    assert!(room.take_player_deaths().is_empty());

    let clear = pool(0, SHIELDS_POOL as u32, 0.0, 0.0);
    let component = pool_component([
        (
            0,
            pool(
                health,
                HEALTH_POOL as u32,
                health_max as f32,
                health_current as f32,
            ),
        ),
        (
            SHIELDS_POOL * POOLS,
            pool(
                shields,
                SHIELDS_POOL as u32,
                shield_max as f32,
                shield_current as f32,
            ),
        ),
        (first_slot, clear),
        (second_slot, clear),
    ]);
    let expected_update = EntityUpdate {
        entity: id.entity,
        operation: Some(Operation::Update(vec![component])),
        ..Default::default()
    };
    assert_eq!(
        room.take_observer_player_health(),
        [(OWNER, expected_update.clone())]
    );

    let lost = egress.acknowledge(2, 0);
    let actual = Arc::clone(&lost.packets[0].records[0]);
    assert_eq!(actual.update, expected_update);
    world.acknowledge(lost, Duration::ZERO);
    transmit(&mut world, Some(&mut room), &mut egress, FIRST_TICK + 2, 2);
    let delivered = egress.acknowledge(3, 1);
    assert!(Arc::ptr_eq(&actual, &delivered.packets[0].records[0]));
    world.acknowledge(delivered, Duration::ZERO);
    assert!(world.ready().is_empty());
    assert!(!world.health_pending(id.entity));
    assert!(room.take_observer_player_health().is_empty());
}
