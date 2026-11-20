use serde_json::json;

use super::*;
use crate::{
    contact_world::ContactWorld,
    game_bots::BotWalk,
    game_pools::HealthPools,
    mover_settings::MoverCatalog,
    npc_catalog::NpcDefinition,
    script_codecs::CodecTable,
    script_enemy::EnemySpawn,
    script_graph::GraphStore,
    script_init::{CreationKind, EntityCreation},
    script_wire::{Value, Variables},
};

#[test]
fn authored_npc_displacement_is_consumed_once_with_room_team_contacts() {
    authored_npc_motion_is_consumed_once(false);
}

#[test]
fn authored_npc_relocation_checks_its_destination_with_room_team_contacts() {
    authored_npc_motion_is_consumed_once(true);
}

fn authored_npc_motion_is_consumed_once(relocate: bool) {
    const HERO: u64 = 0x02e0_0000_0052_0401;
    const BODY: u64 = 0x0400_0000_0052_0402;
    const CONTROLLER: u64 = 0x0400_0000_0052_0403;
    let guid = |value| format!("0x{value:016X}");
    let header = json!({"m_nodesBitCount":3, "m_statesBitCount":3,
        "m_remoteSyncNodesBitCount":3, "m_syncVarsBitCount":3});
    let graph = |index, nodes: Vec<serde_json::Value>, entries: Vec<u16>| {
        json!({"index":index, "items":nodes.len(), "nodes":nodes, "entries":entries, "states":[],
            "remote":[], "sync_vars":[], "containers":{}, "unknown_nodes":[], "fields":{},
            "header":header})
    };
    let destination = [4.25, 0.0, 2.5];
    let (behavior, operand, [x, y, z]) = if relocate {
        ("relocation", "destination", destination)
    } else {
        ("swept_displacement", "delta", [2.25, 0.0, -0.5])
    };
    let authored = graph(
        124,
        vec![
            json!({"pos":0, "rid":0, "uid":0, "remote":-1, "state":null,
                "class":"STUStatescriptEntry", "server_only":true, "client_only":false,
                "fields":{"m_outPlug":{"$":"Plug", "links":[[1,"m_inPlug"]]}}}),
            json!({"pos":1, "rid":1, "uid":1, "remote":-1, "state":null,
                "class":"authored.npc_motion", "server_only":true, "client_only":false,
                "action_behavior":behavior,
                "fields":{(operand):{"$":"STUConfigVarVec3", "m_value":{"X":x,"Y":y,"Z":z}},
                    "m_outPlug":{"$":"Plug","links":[]}}}),
        ],
        vec![0],
    );
    let bundle = json!({
        "version":1, "bodies":{}, "missing":[],
        "graphs":[authored, graph(125, Vec::new(), Vec::new())],
        "junkenstein":{
            "pairs":{(guid(HERO)):{"body":guid(BODY), "controller":guid(CONTROLLER)}},
            "entities":{
                (guid(BODY)):{"hero":guid(HERO), "name":"Swept fixture",
                    "manager":null, "weapons":[], "target_tags":[],
                    "graphs":[{"graph":124,"m_75408640":0}]},
                (guid(CONTROLLER)):{"hero":null, "name":"Swept fixture controller",
                    "manager":null, "weapons":[], "target_tags":[],
                    "graphs":[{"graph":125,"m_75408640":0}]}}}
    });
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap());
    let codecs = Arc::new(CodecTable::from_json(r#"{"classes":{}}"#).unwrap());
    let definition = NpcDefinition {
        hero: HERO,
        body: BODY,
        name: "Swept fixture".into(),
        pools: HealthPools {
            health: 100.0,
            armour: 0.0,
            shields: 0.0,
        },
    };
    let start = [2.0, 0.0, 3.0];
    let request = EntityCreation {
        position: start,
        kind: CreationKind::Hero {
            hero: HERO,
            team: 1,
            yaw: 0.0,
            flags: [Value::Bool(false), Value::Bool(false)],
            controller: None,
            skin: None,
        },
    };
    let contacts = Arc::new(ContactWorld::new(Vec::new()).unwrap());
    let ground = Arc::new(fixture_ground());
    let movers = Arc::new(MoverCatalog::default());
    assert_eq!(graphs.room_spatial_needs(), RoomSpatialNeeds::None);
    assert!(!graphs.uses_spatial_actor_rays());
    let obstacle_position = [3.6875, 0.0, 2.625];
    for (geometry_available, blocker_team, free_for_all, blocker_dead) in [
        (true, None, false, false),
        (false, None, false, false),
        (true, Some(1), false, false),
        (true, Some(0), false, false),
        (true, Some(0), false, true),
        (true, Some(1), true, false),
    ] {
        let supported = BotQueries::new(free_for_all, &movers, &ground, Some(&contacts));
        let unavailable = BotQueries::new(free_for_all, &movers, &ground, None);
        let obstacle = blocker_team.map(|team| {
            let mut blocker = Bot::scene_enemy(
                BOT_ENTITY,
                &definition,
                team,
                obstacle_position,
                0.0,
                BotWalk::default(),
                0,
            );
            blocker.dead = blocker_dead;
            blocker
        });
        let mut room = RoomSimulation::new(obstacle.into_iter().collect()).unwrap();
        room.enable_movement_contacts(free_for_all, Arc::clone(&movers));
        let wire = room
            .create_enemy(EnemySpawn {
                graphs: Arc::clone(&graphs),
                codecs: Arc::clone(&codecs),
                definition: &definition,
                request: &request,
                mode: 0,
                mode_variables: Arc::new(Variables::new()),
                tick: 0,
                walk: BotWalk::default(),
                gate: None,
            })
            .unwrap();
        let first_queries = if geometry_available {
            &supported
        } else {
            &unavailable
        };
        room.finish_tick(first_queries, 0, Duration::ZERO).unwrap();
        let initial_contacts = room.movement_contact_frame().unwrap();
        room.prepare_movement_frame(0).unwrap();
        assert!(Arc::ptr_eq(
            &initial_contacts,
            &room.movement_contact_frame().unwrap()
        ));
        let blocking = !blocker_dead && blocker_team.is_some_and(|team| team != 1 || free_for_all);
        assert_eq!(
            initial_contacts.blockers(wire, 1).len(),
            usize::from(blocking)
        );
        assert_eq!(state(&room, wire).unwrap().bot.position, start);
        room.finish_tick(first_queries, 1, Duration::from_millis(16))
            .unwrap();
        assert!(!Arc::ptr_eq(
            &initial_contacts,
            &room.movement_contact_frame().unwrap()
        ));
        let moved = state(&room, wire).unwrap().bot.position;
        if !geometry_available || (blocking && relocate) {
            assert_eq!(moved, start);
        } else if blocking {
            assert!(
                start[0] < moved[0] && moved[0] < obstacle_position[0],
                "{moved:?}"
            );
        } else {
            assert_eq!(moved, destination);
        }
        room.finish_tick(&supported, 2, Duration::from_millis(32))
            .unwrap();
        let body = state(&room, wire).unwrap();
        assert_eq!(body.bot.position, moved);
        assert_eq!(body.bot.velocity, [0.0; 3]);
        assert!(body.enemy.as_ref().unwrap().fault.is_none());
    }
}
