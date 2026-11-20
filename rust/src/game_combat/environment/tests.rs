use std::{sync::Arc, time::Duration};

use serde_json::json;

use crate::{
    contact_world::ContactWorld,
    game_bots::BOT_RESPAWN_TIME,
    game_combat::{
        PlayerBodyState, RoomSimulation,
        tests::{fixture_bots, fixture_ground},
    },
    game_content::PRACTICE_RANGE_MODE,
    game_heroes::Hero,
    game_instance::RETAIL_QUANTA,
    game_pools::HealthPools,
    game_replication::BodyId,
    map_environment::EnvironmentCatalog,
    match_scene::team_bit,
    mover::Mover,
    mover_settings::{MoverCatalog, MoverSettings},
    scene_assets::BotQueries,
    script_codecs::CodecTable,
    script_graph::GraphStore,
    script_player::{OwnerBodySpawn, PlayerBodyFactory},
    world::{Movement, Operation},
};

fn environment_player(pools: HealthPools) -> (Hero, PlayerBodyFactory) {
    let hero = Hero {
        guid: 0x02e0_0000_00e9_7001,
        body: 0x0400_0000_00e9_7002,
        name: "Environment fixture".into(),
        role: "Damage".into(),
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
    let factory = PlayerBodyFactory::from_body(
        Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap()),
        Arc::new(CodecTable::from_json(r#"{"classes":{}}"#).unwrap()),
        PRACTICE_RANGE_MODE,
        &hero,
        Some(pools),
        false,
    )
    .unwrap();
    (hero, factory)
}

fn environment_mover(position: [f64; 3]) -> Mover {
    Mover::new(
        MoverSettings::default(),
        Arc::new(ContactWorld::new(Vec::new()).unwrap()),
        position,
        0,
        0x18,
    )
    .unwrap()
}

#[test]
fn environment_volumes_settle_source_free_deaths_once_and_preserve_safe_lower_positions() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/room-environment-settlement-v1.json"
    ))
    .unwrap();
    assert_eq!(fixture["schema_version"], 1);
    assert_eq!(
        fixture["case_id"],
        "room-environment-settlement.synthetic.v1"
    );
    assert_eq!(fixture["evidence"], "synthetic-native");
    let input = &fixture["input"];
    let expected = &fixture["expected"];
    let catalog =
        EnvironmentCatalog::from_json(&serde_json::to_string(&fixture["environment"]).unwrap())
            .unwrap();
    let map =
        u64::from_str_radix(input["map"].as_str().unwrap().trim_start_matches("0x"), 16).unwrap();
    let environment = catalog.for_map(map);
    let safe: [f64; 3] = serde_json::from_value(input["safe_lower_position"].clone()).unwrap();
    let lethal: [f64; 3] = serde_json::from_value(input["lethal_position"].clone()).unwrap();
    assert_eq!(safe[1], lethal[1]);
    assert!(!environment.lethal_at(safe));
    assert!(environment.lethal_at(lethal));
    let safe_tick = u32::try_from(input["safe_tick"].as_u64().unwrap()).unwrap();
    let kill_tick = u32::try_from(input["kill_tick"].as_u64().unwrap()).unwrap();
    let safe_now = Duration::from_millis(input["safe_time_ms"].as_u64().unwrap());
    let now = Duration::from_millis(input["kill_time_ms"].as_u64().unwrap());
    let deadline = now + BOT_RESPAWN_TIME;
    let pools = HealthPools {
        health: input["health"].as_f64().unwrap(),
        armour: input["armour"].as_f64().unwrap(),
        shields: input["shields"].as_f64().unwrap(),
    };
    let (hero, factory) = environment_player(pools);
    let ground = fixture_ground();
    let movers = MoverCatalog::default();
    let queries = BotQueries::new(false, &movers, &ground, None);
    let owner = 0xa000_0100;
    let id = BodyId {
        entity: owner + 1,
        generation: 1,
    };

    for combat_active in [false, true] {
        for halted in [false, true] {
            let bots = fixture_bots();
            let target = bots[0].entity;
            let mut room = RoomSimulation::new(bots).unwrap();
            room.combat_active = combat_active;
            room.set_environment(environment.clone());
            let spawn = OwnerBodySpawn {
                controller: owner,
                entity: id.entity,
                skin: 0,
                body_filter: team_bit(0).unwrap(),
                slot: 0,
                frame: safe_tick,
                pose: Movement {
                    position: safe,
                    ..Movement::default()
                },
                eye_position: None,
                quanta: RETAIL_QUANTA,
            };
            room.spawn_player_body(PlayerBodyState::new(
                id,
                owner,
                0,
                0,
                hero.guid,
                environment_mover(safe),
                Some(Box::new(factory.start(spawn).unwrap())),
                safe_tick,
                None,
            ))
            .unwrap();
            if halted {
                assert!(room.clear_player_runtime(id));
                assert!(room.player_body(id).unwrap().runtime.is_none());
            }
            assert!(
                room.finish_tick(&queries, safe_tick, safe_now)
                    .unwrap()
                    .is_empty()
            );
            assert!(room.player_body(id).unwrap().alive());
            assert_eq!(room.player_body(id).unwrap().mover.pose().position, safe);
            assert!(room.take_player_deaths().is_empty());
            assert!(room.take_scripted_health().is_empty());

            room.player_body_mut(id).unwrap().mover = environment_mover(lethal);
            room.body_mut(target).unwrap().position = lethal;
            assert!(
                room.finish_tick(&queries, kill_tick, now)
                    .unwrap()
                    .is_empty()
            );
            let player = room.player_body(id).unwrap();
            assert!(!player.alive());
            let player_death = player.vitals().unwrap().death().unwrap();
            assert_eq!(
                player_death.killer,
                expected["source"].as_u64().unwrap() as u32
            );
            assert_eq!(player_death.frame, kill_tick);
            assert_eq!(player_death.time, now.as_secs_f64());
            assert_eq!(player.vitals().unwrap().total(), 0.0);
            let deaths = room.take_player_deaths();
            assert_eq!(
                deaths.len(),
                expected["death_count"].as_u64().unwrap() as usize
            );
            assert_eq!(deaths[0].owner, owner);
            assert_eq!(deaths[0].body, id);
            assert_eq!(deaths[0].source, 0);
            assert_eq!(
                deaths[0].killed_by,
                expected["killed_by"].as_bool().unwrap()
            );
            let updates = room.take_scripted_health();
            assert_eq!(updates.len(), 2);
            assert!(updates.iter().any(|update| update.entity == id.entity));
            assert!(updates.iter().any(|update| update.entity == target));
            let (bot_death, respawn_at) = room.observed_bot_death(target).unwrap().unwrap();
            assert_eq!(bot_death.killer, 0);
            assert_eq!(bot_death.frame, kill_tick);
            assert_eq!(respawn_at, deadline);

            assert!(
                room.finish_tick(&queries, kill_tick + 1, now + Duration::from_millis(1))
                    .unwrap()
                    .is_empty()
            );
            assert!(room.take_player_deaths().is_empty());
            assert!(room.take_scripted_health().is_empty());
            assert_eq!(
                room.observed_bot_death(target).unwrap().unwrap().1,
                deadline
            );
            assert!(
                room.finish_tick(&queries, kill_tick + 2, deadline - Duration::from_nanos(1))
                    .unwrap()
                    .is_empty()
            );
            let replacements = room.finish_tick(&queries, kill_tick + 3, deadline).unwrap();
            assert_eq!(
                replacements.len(),
                expected["replacement_count"].as_u64().unwrap() as usize
            );
            let replacement = &replacements[0];
            assert_eq!(replacement.retired, target);
            assert_ne!(replacement.created.entity, target);
            assert!(matches!(
                &replacement.created.operation,
                Some(Operation::Create(_))
            ));
            assert!(room.sample_health_body(target).is_none());
            assert_eq!(
                room.body_healths().unwrap()[&replacement.created.entity],
                expected["fresh_bot_health"].as_f64().unwrap()
            );
            assert!(
                room.finish_tick(&queries, kill_tick + 4, deadline + Duration::from_nanos(1))
                    .unwrap()
                    .is_empty()
            );
            assert!(room.take_player_deaths().is_empty());
            assert!(!room.player_body(id).unwrap().alive());
        }
    }
}
