use super::*;
use crate::{
    commands::Command,
    contact_world::ContactWorld,
    game_heroes::Hero,
    game_replication::BodyStep,
    mover::Mover,
    mover_settings::{MoverCatalog, MoverSettings},
    script_codecs::CodecTable,
    script_graph::GraphStore,
    script_player::{OwnerBodySetup, OwnerBodySpawn, PlayerBodyFactory, fixture_body_factory},
    script_wire::Value,
    world::Movement,
};
use serde_json::json;

const SAMPLE: u32 = 871;

fn sampled_aim_factory(spawn: OwnerBodySpawn) -> PlayerBodyFactory {
    let hero = Hero {
        guid: 0x02e0_0000_0000_f901,
        body: 0x0400_0000_0000_f902,
        name: "Sampled aim fixture".into(),
        role: "Fixture".into(),
    };
    let output = json!({"$":"STUConfigVarDynamic","m_identifier":SAMPLE,"m_60DB8F99":true});
    let first = json!({"$":"STU_4BD39CFD","m_index":0,"m_814F8EC3":output});
    // The change callback reuses the output before the fresh-sample hook.
    let overwrite = json!({
        "pos":4,"rid":4,"uid":4,"remote":-1,"state":null,
        "class":"STUStatescriptActionSetVar","server_only":true,"client_only":false,
        "fields":{"m_out_Var":output,"m_value":999,
            "m_outPlug":{"$":"Plug","links":[]}}
    });
    let observation = json!({"pos":1,"rid":1,"uid":1,"remote":-1,"state":0,
                    "class":"authored.sampled_aim","server_only":true,"client_only":false,
                    "state_behavior":"observe","fields":{
                        "value":{"$":"EntityRayCast",
                            "origin":{"$":"EntityPosition","entity":{"$":"STU_B5A0CAF0"}},
                            "direction":{"$":"EntityDirection","entity":{"$":"STU_B5A0CAF0"}},
                            "max_distance":6,
                            "filter":{"alive":true,"relation":"opponent",
                                "source":{"$":"STU_B5A0CAF0"}}},
                        "output":output,
                        "on_change":{"$":"Plug","links":[[4,"m_inPlug"]]},
                        "on_sample":{"$":"Plug","links":[[2,"m_inPlug"]]}}});
    let bundle = json!({
        "version":1,"missing":[],
        "bodies":{format!("0x{:016X}",hero.body):{
            "hero":format!("0x{:016X}",hero.guid),"name":"Sampled aim fixture",
            "graphs":[{"graph":93,"m_75408640":0}],"manager":null,"weapons":[]
        }},
        "graphs":[{
            "index":93,"items":5,"entries":[0],"states":[1],"remote":[],
            "sync_vars":[],"containers":{},"unknown_nodes":[],"fields":{},
            "header":{"m_nodesBitCount":4,"m_statesBitCount":4,
                "m_remoteSyncNodesBitCount":4,"m_syncVarsBitCount":4},
            "nodes":[
                {"pos":0,"rid":0,"uid":0,"remote":-1,"state":null,
                    "class":"STUStatescriptEntry","server_only":true,"client_only":false,
                    "fields":{"m_outPlug":{"$":"Plug","links":[[1,"m_beginPlug"]]}}},
                observation,
                {"pos":2,"rid":2,"uid":2,"remote":-1,"state":null,
                    "class":"authored.has_fresh_target","server_only":true,"client_only":false,
                    "action_behavior":"branch","fields":{
                        "m_condition":first,
                        "m_truePlug":{"$":"Plug","links":[[3,"m_inPlug"]]},
                        "m_falsePlug":{"$":"Plug","links":[]}}},
                {"pos":3,"rid":3,"uid":3,"remote":-1,"state":null,
                    "class":"authored.small_health_effect","server_only":true,"client_only":false,
                    "action_behavior":"health_change","fields":{
                        "target":first,"delta":-3,
                        "m_outPlug":{"$":"Plug","links":[]}}},
                overwrite
            ]
        }]
    });
    PlayerBodyFactory::prepare(
        Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap()),
        Arc::new(CodecTable::from_json(r#"{"classes":{}}"#).unwrap()),
        0,
        &hero,
        None,
        false,
        (spawn, OwnerBodySetup::default()),
    )
    .unwrap()
}

fn mover(position: Vec3, yaw: i32) -> Mover {
    Mover::new(
        MoverSettings {
            stand_height: 1.0,
            stand_radius: 0.25,
            ..MoverSettings::default()
        },
        Arc::new(ContactWorld::new(Vec::new()).unwrap()),
        position,
        yaw,
        MOVER_CATEGORY,
    )
    .unwrap()
}

fn advance_aim(room: &mut RoomSimulation, source: BodyId, frame: u32, available: bool) {
    room.prepare_spatial_frame(frame).unwrap();
    if !available {
        let mut unavailable = room.spatial_frame.as_ref().unwrap().1.as_ref().clone();
        unavailable.actor_capsules = None;
        room.player_body_mut(source)
            .unwrap()
            .runtime
            .as_mut()
            .unwrap()
            .set_spatial_frame(Arc::new(unavailable));
    }
    let state = room.player_body_mut(source).unwrap();
    state
        .runtime
        .as_mut()
        .unwrap()
        .advance(
            Command {
                frame,
                ..Command::default()
            },
            &state.mover,
            BodyStep::Posed,
        )
        .unwrap();
    let snapshot = state.runtime.as_ref().unwrap().snapshot().unwrap();
    assert!(snapshot.instances[&1].states[&0].active);
}

fn sample(room: &RoomSimulation, source: BodyId) -> Option<Value> {
    room.player_body(source)
        .unwrap()
        .runtime
        .as_ref()
        .unwrap()
        .snapshot()
        .unwrap()
        .variables
        .get(&SAMPLE)
        .cloned()
}

fn health(room: &RoomSimulation, target: BodyId) -> f64 {
    room.player_body(target)
        .unwrap()
        .runtime
        .as_ref()
        .unwrap()
        .vitals()
        .unwrap()
        .total()
}

fn settle(room: &mut RoomSimulation, frame: u32) {
    room.settle_authored_requests(frame, Duration::from_millis(u64::from(frame) * 16))
        .unwrap();
}

#[test]
fn fresh_actor_aim_samples_gate_room_effects_through_occlusion_and_generation_changes() {
    let source = BodyId {
        entity: 71,
        generation: 1,
    };
    let target = BodyId {
        entity: 81,
        generation: 1,
    };
    let source_spawn = OwnerBodySpawn {
        controller: 70,
        entity: source.entity,
        body_filter: crate::match_scene::team_bit(0).unwrap(),
        slot: 0,
        frame: 0,
        pose: Movement {
            position: [-3.0, 0.5, 0.25],
            yaw: 16_384,
            ..Movement::default()
        },
        eye_position: None,
        quanta: RETAIL_QUANTA,
    };
    let source_runtime = sampled_aim_factory(source_spawn)
        .start(source_spawn)
        .unwrap();
    let (target_factory, target_spawn) = fixture_body_factory();
    let target_spawn = OwnerBodySpawn {
        controller: 80,
        entity: target.entity,
        frame: 0,
        body_filter: crate::match_scene::team_bit(1).unwrap(),
        pose: Movement {
            position: [-2.0, 0.0, 0.25],
            ..Movement::default()
        },
        ..target_spawn
    };
    let target_body = |id| {
        PlayerBodyState::new(
            id,
            80,
            1,
            1,
            1,
            mover(target_spawn.pose.position, 0),
            Some(Box::new(target_factory.start(target_spawn).unwrap())),
            0,
            None,
        )
    };
    let mut room = RoomSimulation::new(Vec::new()).unwrap();
    room.spawn_player_body(PlayerBodyState::new(
        source,
        70,
        0,
        0,
        1,
        mover(source_spawn.pose.position, source_spawn.pose.yaw),
        Some(Box::new(source_runtime)),
        0,
        None,
    ))
    .unwrap();
    room.spawn_player_body(target_body(target)).unwrap();
    room.enable_spatial_queries(
        false,
        Arc::new(fixture_ground()),
        RoomSpatialNeeds::EntityFacts,
    );
    room.enable_spatial_actor_rays(Arc::new(MoverCatalog::default()));
    let initial = health(&room, target);
    assert_eq!(sample(&room, source), None);

    advance_aim(&mut room, source, 1, true);
    let first = sample(&room, source).unwrap();
    assert!(
        matches!(&first, Value::Array(items) if items.first()==Some(&Value::Entity(target.entity)))
    );
    assert_eq!(health(&room, target), initial);
    settle(&mut room, 1);
    assert_eq!(health(&room, target), initial - 3.0);
    advance_aim(&mut room, source, 2, true);
    assert_eq!(sample(&room, source), Some(first.clone()));
    settle(&mut room, 2);
    assert_eq!(health(&room, target), initial - 6.0);

    // Retain the previous successful value while the current host sample
    // lacks actor-ray data. The fresh-sample hook must produce no request.
    advance_aim(&mut room, source, 3, false);
    assert_eq!(sample(&room, source), Some(first));
    settle(&mut room, 3);
    assert_eq!(health(&room, target), initial - 6.0);

    room.player_body_mut(target).unwrap().mover = mover([0.0, 0.0, 0.25], 0);
    advance_aim(&mut room, source, 4, true);
    assert_eq!(sample(&room, source), Some(Value::Array(Vec::new())));
    settle(&mut room, 4);
    assert_eq!(health(&room, target), initial - 6.0);

    room.player_body_mut(target).unwrap().mover = mover(target_spawn.pose.position, 0);
    advance_aim(&mut room, source, 5, true);
    let replacement = BodyId {
        generation: 2,
        ..target
    };
    room.spawn_player_body(target_body(replacement)).unwrap();
    assert!(room.player_body(target).is_none());
    settle(&mut room, 5);
    assert_eq!(health(&room, replacement), initial);
    advance_aim(&mut room, source, 6, true);
    settle(&mut room, 6);
    assert_eq!(health(&room, replacement), initial - 3.0);

    assert!(room.clear_player_runtime(source));
    room.prepare_spatial_frame(7).unwrap();
    settle(&mut room, 7);
    assert_eq!(health(&room, replacement), initial - 3.0);
}
