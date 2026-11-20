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
        skin: 0,
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

const HELD_SAMPLE: u32 = 881;
const HELD_INPUT: u32 = 882;
const HELD_STATUS: u32 = 883;

fn held_aim_factory(spawn: OwnerBodySpawn, authored_listener: bool) -> PlayerBodyFactory {
    let hero = Hero {
        guid: 0x02e0_0000_0000_f911,
        body: 0x0400_0000_0000_f912,
        name: "Held aim fixture".into(),
        role: "Fixture".into(),
    };
    let variable = |id| json!({"$":"STUConfigVarDynamic","m_identifier":id,"m_60DB8F99":false});
    let plug = |links: &[(u16, &str)], states: &[u16]| json!({"$":"Plug","links":links,"m_FF3DAF1E":states});
    let node = |pos: u16, class: &str, state: Option<u16>, mut fields: serde_json::Value| {
        fields["m_2BBEEAB8"] = json!(state.is_some());
        json!({"pos":pos,"rid":pos,"uid":pos,"remote":-1,"state":state,
            "class":class,"server_only":true,"client_only":false,"fields":fields})
    };
    let setter = |pos, id, value, links: &[(u16, &str)]| {
        node(
            pos,
            "STUStatescriptActionSetVar",
            None,
            json!({
            "m_out_Var":variable(id),"m_value":value,"m_outPlug":plug(links,&[])}),
        )
    };
    let first = json!({"$":"STU_4BD39CFD","m_index":0,"m_814F8EC3":variable(HELD_SAMPLE)});
    let mut claim = node(
        4,
        "authored.held_status",
        Some(1),
        json!({
        "variable":{"scope":"instance","id":HELD_STATUS},
        "priority":{"level":5,"above_equals":false},"value":variable(HELD_INPUT)}),
    );
    claim["state_behavior"] = json!("stack_claim");
    let mut observe = node(
        5,
        "authored.held_sample",
        Some(2),
        json!({
        "value":{"$":"EntityRayCast",
            "origin":{"$":"EntityPosition","entity":{"$":"STU_B5A0CAF0"}},
            "direction":{"$":"EntityDirection","entity":{"$":"STU_B5A0CAF0"}},
            "max_distance":6,"filter":{"alive":true,"relation":"opponent",
                "source":{"$":"STU_B5A0CAF0"}}},
        "output":variable(HELD_SAMPLE),"on_sample":plug(&[(7,"m_inPlug")],&[]),
        "on_unavailable":plug(&[(9,"m_inPlug")],&[])}),
    );
    observe["state_behavior"] = json!("observe");
    let mut movement = node(6, "authored.held_speed", Some(3), json!({"factor":0.5}));
    movement["state_behavior"] = json!("movement_scale");
    let mut branch = node(
        7,
        "authored.held_target",
        None,
        json!({
        "m_condition":first,"m_truePlug":plug(&[(8,"m_inPlug")],&[]),
        "m_falsePlug":plug(&[(9,"m_inPlug")],&[])}),
    );
    branch["action_behavior"] = json!("branch");
    let mut graph_fields = json!({"m_9CEC6985":[format!("0x000000000000{HELD_SAMPLE:04X}")]});
    let listener = if authored_listener {
        let mut listener = node(
            3,
            "authored.held_aim_input",
            Some(0),
            json!({
                "button":29,"held_states":[1,2,3],
                "on_press":plug(&[(9,"m_inPlug")],&[]),
                "on_release":plug(&[(9,"m_inPlug")],&[])
            }),
        );
        listener["state_behavior"] = json!("held_input");
        listener
    } else {
        graph_fields["m_680A2CB2"] = json!([0]);
        node(
            3,
            "STUStatescriptStateLogicalButton",
            Some(0),
            json!({
                "m_logicalButton":{"$":"STUConfigVarLogicalButton","m_logicalButton":29},
                "m_onGoingDownPlug":plug(&[(9,"m_inPlug")],&[]),
                "m_onComingUpPlug":plug(&[(9,"m_inPlug")],&[]),
                "m_80AF45FB":plug(&[(4,"m_beginPlug"),(5,"m_beginPlug"),
                    (6,"m_beginPlug")],&[1,2,3]),"m_1C9EF058":plug(&[],&[])
            }),
        )
    };
    let bundle = json!({"version":1,"missing":[],
    "bodies":{format!("0x{:016X}",hero.body):{
        "hero":format!("0x{:016X}",hero.guid),"name":"Held aim fixture",
        "graphs":[{"graph":94,"m_75408640":0}],"manager":null,"weapons":[]}},
    "graphs":[{"index":94,"items":10,"entries":[0],"states":[3,4,5,6],
        "remote":[],"sync_vars":[[HELD_STATUS,0,0]],"containers":{},"unknown_nodes":[],
        "owner_only_instance_vars":[HELD_INPUT,HELD_STATUS],
        "fields":graph_fields,
        "header":{"m_nodesBitCount":4,"m_statesBitCount":4,
            "m_remoteSyncNodesBitCount":4,"m_syncVarsBitCount":4},
        "nodes":[
            node(0,"STUStatescriptEntry",None,
                json!({"m_outPlug":plug(&[(1,"m_inPlug")],&[])})),
            setter(1,HELD_INPUT,false,&[(2,"m_inPlug")]),
            setter(2,HELD_STATUS,false,&[(3,"m_beginPlug")]),
            listener,claim,observe,movement,branch,
            setter(8,HELD_INPUT,true,&[]),setter(9,HELD_INPUT,false,&[])
        ]}]});
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

fn advance_held_aim(
    room: &mut RoomSimulation,
    source: BodyId,
    frame: u32,
    held: bool,
    available: bool,
) {
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
                buttons: if held { crate::commands::SECONDARY } else { 0 },
                ..Command::default()
            },
            &state.mover,
            BodyStep::Posed,
        )
        .unwrap();
}

fn held_aim_projection(
    room: &RoomSimulation,
    source: BodyId,
    expected: bool,
) -> ((usize, Vec<u8>), (usize, Vec<u8>)) {
    use crate::script_snapshot_wire::View;

    let runtime = room.player_body(source).unwrap().runtime.as_ref().unwrap();
    let snapshot = runtime.snapshot().unwrap();
    let encoder = runtime.encoder();
    let owner = encoder.project(snapshot, View::Owner).unwrap();
    let observer = encoder.project(snapshot, View::Observer).unwrap();
    assert_eq!(
        owner.instance_variables(1).unwrap().get(&HELD_STATUS),
        Some(&Value::Bool(expected))
    );
    for id in [HELD_SAMPLE, HELD_INPUT, HELD_STATUS] {
        assert!(!observer.instance_variables(1).unwrap().contains_key(&id));
    }
    assert!(
        !owner
            .instance_variables(1)
            .unwrap()
            .contains_key(&HELD_SAMPLE)
    );
    let bytes = |view| {
        let bits = encoder.full(view).unwrap();
        (bits.bit_len(), bits.as_bytes().to_vec())
    };
    (bytes(&owner), bytes(&observer))
}

#[test]
fn held_aim_composes_private_status_scoped_movement_and_fresh_actor_queries() {
    assert_held_aim_lifecycle(false);
}

#[test]
fn authored_held_aim_uses_the_shared_room_and_snapshot_lifecycle() {
    assert_held_aim_lifecycle(true);
}

fn assert_held_aim_lifecycle(authored_listener: bool) {
    let source = BodyId {
        entity: 71,
        generation: 1,
    };
    let target = BodyId {
        entity: 81,
        generation: 1,
    };
    let spawn = OwnerBodySpawn {
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
        skin: 0,
        quanta: RETAIL_QUANTA,
    };
    let factory = held_aim_factory(spawn, authored_listener);
    let source_body = |id, spawn: OwnerBodySpawn| {
        PlayerBodyState::new(
            id,
            70,
            0,
            0,
            1,
            mover(spawn.pose.position, spawn.pose.yaw),
            Some(Box::new(factory.start(spawn).unwrap())),
            spawn.frame,
            None,
        )
    };
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
    let mut room = RoomSimulation::new(Vec::new()).unwrap();
    room.spawn_player_body(source_body(source, spawn)).unwrap();
    room.spawn_player_body(PlayerBodyState::new(
        target,
        80,
        1,
        1,
        1,
        mover(target_spawn.pose.position, 0),
        Some(Box::new(target_factory.start(target_spawn).unwrap())),
        0,
        None,
    ))
    .unwrap();
    room.enable_spatial_queries(
        false,
        Arc::new(fixture_ground()),
        RoomSpatialNeeds::EntityFacts,
    );
    room.enable_spatial_actor_rays(Arc::new(MoverCatalog::default()));
    let (idle_owner, observer_bytes) = held_aim_projection(&room, source, false);

    advance_held_aim(&mut room, source, 1, true, true);
    assert_eq!(held_aim_projection(&room, source, false).1, observer_bytes);
    let mut timeline = crate::mover_modifiers::ModifierTimeline::default();
    let modifiers = room
        .player_body(source)
        .unwrap()
        .runtime
        .as_ref()
        .unwrap()
        .movement_modifiers()
        .unwrap()
        .clone();
    assert_eq!(modifiers.len(), 1);
    timeline.track(1, &modifiers);
    let base = MoverSettings::default();
    assert_eq!(timeline.at(1).settings(&base).run_forward, base.run_forward);
    assert_eq!(
        timeline.at(2).settings(&base).run_forward,
        base.run_forward * 0.5
    );

    advance_held_aim(&mut room, source, 2, true, true);
    let (hit_owner, observer) = held_aim_projection(&room, source, true);
    assert_ne!(hit_owner, idle_owner);
    assert_eq!(observer, observer_bytes);
    let raw_hit = room
        .player_body(source)
        .unwrap()
        .runtime
        .as_ref()
        .unwrap()
        .snapshot()
        .unwrap()
        .instances[&1]
        .variables[&HELD_SAMPLE]
        .clone();
    assert!(matches!(&raw_hit,Value::Array(items)
        if items.first()==Some(&Value::Entity(target.entity))));
    advance_held_aim(&mut room, source, 3, true, true);
    assert_eq!(
        held_aim_projection(&room, source, true),
        (hit_owner.clone(), observer_bytes.clone())
    );
    assert_eq!(
        room.player_body(source)
            .unwrap()
            .runtime
            .as_ref()
            .unwrap()
            .snapshot()
            .unwrap()
            .instances[&1]
            .states[&2]
            .activation,
        1
    );

    // The wall in fixture_ground lies between the source and this pose.
    room.player_body_mut(target).unwrap().mover = mover([0.0, 0.0, 0.25], 0);
    advance_held_aim(&mut room, source, 4, true, true);
    assert_eq!(held_aim_projection(&room, source, false).1, observer_bytes);
    assert_eq!(
        room.player_body(source)
            .unwrap()
            .runtime
            .as_ref()
            .unwrap()
            .snapshot()
            .unwrap()
            .instances[&1]
            .variables[&HELD_SAMPLE],
        Value::Array(Vec::new())
    );
    room.player_body_mut(target).unwrap().mover = mover(target_spawn.pose.position, 0);
    advance_held_aim(&mut room, source, 5, true, true);
    assert_eq!(
        held_aim_projection(&room, source, true),
        (hit_owner.clone(), observer_bytes.clone())
    );

    // Missing host data clears the derived owner status, retaining the raw
    // sample for diagnostics. Recovery can publish the same successful hit.
    advance_held_aim(&mut room, source, 6, true, false);
    assert_eq!(held_aim_projection(&room, source, false).1, observer_bytes);
    assert_eq!(
        room.player_body(source)
            .unwrap()
            .runtime
            .as_ref()
            .unwrap()
            .snapshot()
            .unwrap()
            .instances[&1]
            .variables[&HELD_SAMPLE],
        raw_hit
    );
    advance_held_aim(&mut room, source, 7, true, true);
    assert_eq!(
        held_aim_projection(&room, source, true),
        (hit_owner, observer_bytes.clone())
    );

    advance_held_aim(&mut room, source, 8, false, true);
    assert_eq!(
        held_aim_projection(&room, source, false),
        (idle_owner, observer_bytes.clone())
    );
    let runtime = room.player_body(source).unwrap().runtime.as_ref().unwrap();
    let released = runtime.snapshot().unwrap();
    for raw in [1, 2, 3] {
        assert!(!released.instances[&1].states[&raw].active);
    }
    assert!(runtime.movement_modifiers().unwrap().is_empty());
    timeline.track(8, runtime.movement_modifiers().unwrap());
    assert_eq!(
        timeline.at(8).settings(&base).run_forward,
        base.run_forward * 0.5
    );
    assert_eq!(timeline.at(9).settings(&base).run_forward, base.run_forward);

    advance_held_aim(&mut room, source, 9, true, true);
    advance_held_aim(&mut room, source, 10, true, true);
    held_aim_projection(&room, source, true);
    let replacement = BodyId {
        generation: 2,
        ..source
    };
    room.spawn_player_body(source_body(
        replacement,
        OwnerBodySpawn { frame: 10, ..spawn },
    ))
    .unwrap();
    assert!(room.player_body(source).is_none());
    assert_eq!(
        held_aim_projection(&room, replacement, false).1,
        observer_bytes
    );
    advance_held_aim(&mut room, replacement, 11, false, true);
    let runtime = room
        .player_body(replacement)
        .unwrap()
        .runtime
        .as_ref()
        .unwrap();
    assert!(runtime.movement_modifiers().unwrap().is_empty());
    assert!(
        !runtime.snapshot().unwrap().instances[&1]
            .states
            .contains_key(&2)
    );
    held_aim_projection(&room, replacement, false);
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/statescript-entity-ray-cast-v1.json"
    ))
    .unwrap();
    let expected = &fixture["expected"]["inspector_held_samples"];
    let overlay = room.debug_physics().unwrap().unwrap();
    let rays = serde_json::to_value(overlay.rays).unwrap();
    let rays = rays.as_array().unwrap();
    for expected_sample in expected["frames"].as_array().unwrap() {
        let tick = &expected_sample[0];
        let hit = &expected_sample[1];
        let sample = rays
            .iter()
            .find(|sample| &sample["tick"] == tick && &sample["hit"] == hit)
            .unwrap_or_else(|| panic!("missing inspector ray for {expected_sample}: {rays:?}"));
        assert_eq!(sample["source"], source.entity);
        assert_eq!(sample["kind"], "script_actor");
        assert_eq!(sample["script"]["graph"], expected["graph"]);
        assert_eq!(sample["script"]["instance"], expected["instance"]);
        assert_eq!(sample["script"]["node"], expected["node"]);
        assert_eq!(sample["script"]["vm_frame"], *tick);
        if hit == "actor" {
            assert_eq!(sample["target"], target.entity);
        } else {
            assert!(sample.get("target").is_none());
        }
    }
    assert!(
        rays.iter()
            .all(|sample| sample["tick"] != expected["retired_frame"])
    );
}

#[test]
fn npc_facing_claim_controls_live_ray_samples_before_navigation_publication() {
    use crate::{
        game_debug::DebugScriptRay,
        game_pools::HealthPools,
        npc_catalog::NpcDefinition,
        script_enemy::EnemySpawn,
        script_init::{ABORT_PLUG, CreationKind, EntityCreation},
        script_wire::Variables,
    };

    const HERO: u64 = 0x02e0_0000_0050_0201;
    const BODY: u64 = 0x0400_0000_0050_0202;
    const CONTROLLER: u64 = 0x0400_0000_0050_0203;
    const RELEASE: u64 = 0x0240_0000_0050_0204;
    let contract: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/statescript-facing-claim-v1.json"
    ))
    .unwrap();
    let input = &contract["input"]["npc_navigation"];
    let expected = &contract["expected"]["npc_navigation"];
    let graph = u32::try_from(input["graph"].as_u64().unwrap()).unwrap();
    let sample = u32::try_from(input["sample_variable"].as_u64().unwrap()).unwrap();
    let position: Vec3 = serde_json::from_value(input["position"].clone()).unwrap();
    let [x, y, z]: Vec3 = serde_json::from_value(input["claim_direction"].clone()).unwrap();
    let navigation: Vec3 = serde_json::from_value(input["navigation_direction"].clone()).unwrap();
    let claimed_angles: [i32; 2] =
        serde_json::from_value(expected["claimed_angles"].clone()).unwrap();
    let navigation_angles: [i32; 2] =
        serde_json::from_value(expected["navigation_angles"].clone()).unwrap();
    let hit_point: Vec3 = serde_json::from_value(expected["hit_point"].clone()).unwrap();
    let miss_end: Vec3 = serde_json::from_value(expected["miss_end"].clone()).unwrap();
    let assert_point = |actual: Vec3, expected: Vec3| {
        assert!(
            actual
                .into_iter()
                .zip(expected)
                .all(|(actual, expected)| (actual - expected).abs() < 1e-6),
            "point {actual:?} differs from {expected:?}"
        );
    };
    let output = json!({"$":"STUConfigVarDynamic","m_identifier":sample,"m_60DB8F99":true});
    let plug = |links: &[(u16, &str)]| json!({"$":"Plug","links":links});
    let node = |pos: u16, class: &str, raw: Option<u16>, mut fields: serde_json::Value| {
        fields["m_2BBEEAB8"] = json!(raw.is_some());
        json!({"pos":pos,"rid":pos,"uid":pos,"remote":-1,"state":raw,
            "class":class,"server_only":true,"client_only":false,"fields":fields})
    };
    let mut claim = node(
        1,
        "authored.npc_sample_facing",
        Some(0),
        json!({
            "direction":{"$":"STUConfigVarVec3","m_value":{"X":x,"Y":y,"Z":z}},
            "priority":contract["input"]["priority"]
        }),
    );
    claim["state_behavior"] = json!("facing_claim");
    let mut observe = node(
        2,
        "authored.npc_forward_ray",
        Some(1),
        json!({"value":{"$":"RayCast",
            "origin":{"$":"EntityPosition","entity":{"$":"STU_B5A0CAF0"}},
            "direction":{"$":"EntityDirection","entity":{"$":"STU_B5A0CAF0"}},
            "max_distance":input["max_distance"]},"output":output}),
    );
    observe["state_behavior"] = json!("observe");
    let bundle = json!({"version":1,"bodies":{},"missing":[],
    "graphs":[{
        "index":graph,"items":4,"states":[1,2],"entries":[0,3],"remote":[],
        "sync_vars":[],"containers":{},"unknown_nodes":[],"fields":{},
        "header":{"m_nodesBitCount":3,"m_statesBitCount":3,
            "m_remoteSyncNodesBitCount":3,"m_syncVarsBitCount":3},
        "nodes":[
            node(0,"STUStatescriptEntry",None,
                json!({"m_outPlug":plug(&[(1,"m_beginPlug"),(2,"m_beginPlug")])})),
            claim,observe,
            node(3,"STUStatescriptEntryGameMessage",None,
                json!({"m_gameMessage":{"m_gameMessage":format!("0x{RELEASE:016X}")},
                    "m_outPlug":plug(&[(1,ABORT_PLUG)])}))
        ]
    },{
        "index":graph+1,"items":0,"nodes":[],"states":[],"entries":[],"remote":[],
        "sync_vars":[],"containers":{},"unknown_nodes":[],"fields":{},
        "header":{"m_nodesBitCount":3,"m_statesBitCount":3,
            "m_remoteSyncNodesBitCount":3,"m_syncVarsBitCount":3}
    }],
    "junkenstein":{
        "pairs":{format!("0x{HERO:016X}"):{
            "body":format!("0x{BODY:016X}"),"controller":format!("0x{CONTROLLER:016X}")}},
        "entities":{
            format!("0x{BODY:016X}"):{"hero":format!("0x{HERO:016X}"),
                "name":"Facing sample fixture","manager":null,"weapons":[],"target_tags":[],
                "graphs":[{"graph":graph,"m_75408640":0}]},
            format!("0x{CONTROLLER:016X}"):{"hero":null,"name":"Facing sample control",
                "manager":null,"weapons":[],"target_tags":[],
                "graphs":[{"graph":graph+1,"m_75408640":0}]}
        }
    }});
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap());
    let definition = NpcDefinition {
        hero: HERO,
        body: BODY,
        name: "Facing sample fixture".into(),
        pools: HealthPools {
            health: 100.0,
            armour: 0.0,
            shields: 0.0,
        },
    };
    let request = EntityCreation {
        position,
        kind: CreationKind::Hero {
            hero: HERO,
            team: 1,
            yaw: 0.0,
            flags: [Value::Bool(false), Value::Bool(false)],
            controller: None,
            skin: None,
        },
    };
    let ground = Arc::new(fixture_ground());
    let movers = MoverCatalog::default();
    let queries = BotQueries::new(false, &movers, &ground, None);
    let mut room = RoomSimulation::new(Vec::new()).unwrap();
    room.enable_spatial_queries(false, Arc::clone(&ground), graphs.room_spatial_needs());
    room.prepare_spatial_frame(0).unwrap();
    room.activate_combat();
    let entity = room
        .create_enemy(EnemySpawn {
            graphs,
            codecs: Arc::new(CodecTable::from_json(r#"{"classes":{}}"#).unwrap()),
            definition: &definition,
            request: &request,
            mode: 0,
            mode_variables: Arc::new(Variables::new()),
            tick: 0,
            walk: crate::game_bots::BotWalk::default(),
            gate: None,
        })
        .unwrap();
    let claimed_frames: Vec<u32> = serde_json::from_value(input["claimed_frames"].clone()).unwrap();
    let release_frame = u32::try_from(input["release_frame"].as_u64().unwrap()).unwrap();
    let released_frame = u32::try_from(input["released_frame"].as_u64().unwrap()).unwrap();

    let advance = |room: &mut RoomSimulation, frame| {
        // Supply fresh steering through the same aim publication used by navigation.
        room.body_mut(entity)
            .unwrap()
            .apply_objective_aim(navigation, frame);
        assert_eq!(
            room.body_mut(entity).unwrap().movement(frame).yaw,
            navigation_angles[0]
        );
        room.finish_tick(
            &queries,
            frame,
            Duration::from_millis(u64::from(frame) * 16),
        )
        .unwrap();
        let state = state(room, entity).unwrap();
        assert!(state.enemy.as_ref().unwrap().fault.is_none());
    };
    for &frame in &claimed_frames {
        advance(&mut room, frame);
        let state = state(&room, entity).unwrap();
        let pose = state.bot.movement(frame);
        assert_eq!([pose.yaw, pose.pitch], claimed_angles);
        let enemy = state.enemy.as_ref().unwrap();
        let result = &enemy.scripts.body.snapshot().unwrap().variables[&sample];
        let Value::Array(hit) = result else {
            panic!("expected ray result, got {result:?}")
        };
        assert_eq!(
            hit.first(),
            Some(&Value::Float(expected["hit_distance"].as_f64().unwrap()))
        );
        let Some(Value::Vector(point)) = hit.get(1) else {
            panic!("expected a ray hit point, got {hit:?}");
        };
        assert_point(*point, hit_point);
    }
    {
        let handle = room.by_wire[&entity];
        let mut state = room.world.get_mut::<BotState>(handle).unwrap();
        let queued = state
            .enemy
            .as_mut()
            .unwrap()
            .scripts
            .body
            .queue_frame_message(release_frame, 1, RELEASE, Variables::new())
            .unwrap();
        assert_eq!(queued, 1);
    }
    advance(&mut room, release_frame);
    {
        let state = state(&room, entity).unwrap();
        assert_eq!(
            state
                .enemy
                .as_ref()
                .unwrap()
                .scripts
                .authored_facing(entity),
            None
        );
        let pose = state.bot.movement(release_frame);
        assert_eq!([pose.yaw, pose.pitch], navigation_angles);
    }
    advance(&mut room, released_frame);
    let enemy = state(&room, entity).unwrap().enemy.as_ref().unwrap();
    assert_eq!(
        enemy.scripts.body.snapshot().unwrap().variables[&sample],
        Value::Array(Vec::new())
    );
    let overlay = room.debug_physics().unwrap().unwrap();
    for frame in claimed_frames.into_iter().chain([released_frame]) {
        let ray = overlay
            .rays
            .iter()
            .find(|ray| ray.source == entity && ray.tick == frame && ray.kind == "script_static")
            .expect("fresh NPC script ray");
        assert_eq!(ray.origin, position);
        assert_eq!(
            ray.script,
            Some(DebugScriptRay {
                graph,
                instance: 1,
                node: Some(2),
                vm_frame: frame
            })
        );
        if frame == released_frame {
            assert_eq!(ray.hit, "none");
            assert_point(ray.end, miss_end);
        } else {
            assert_eq!(ray.hit, "world");
            assert_point(ray.end, hit_point);
        }
    }
}
