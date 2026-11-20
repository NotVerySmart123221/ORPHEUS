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
fn authored_npc_displacement_waits_for_the_next_room_tick_and_is_consumed_once() {
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
    let authored = graph(
        124,
        vec![
            json!({"pos":0, "rid":0, "uid":0, "remote":-1, "state":null,
                "class":"STUStatescriptEntry", "server_only":true, "client_only":false,
                "fields":{"m_outPlug":{"$":"Plug", "links":[[1,"m_inPlug"]]}}}),
            json!({"pos":1, "rid":1, "uid":1, "remote":-1, "state":null,
                "class":"AuthoredCapsuleMove", "server_only":true, "client_only":false,
                "action_behavior":"swept_displacement",
                "fields":{"delta":{"$":"STUConfigVarVec3", "m_value":{"X":2.25,"Y":0,"Z":-0.5}},
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
    let ground = fixture_ground();
    let movers = MoverCatalog::default();
    let supported = BotQueries::new(false, &movers, &ground, Some(&contacts));
    let unavailable = BotQueries::new(false, &movers, &ground, None);
    for geometry_available in [true, false] {
        let mut room = RoomSimulation::new(Vec::new()).unwrap();
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
        assert_eq!(state(&room, wire).unwrap().bot.position, start);
        room.finish_tick(first_queries, 1, Duration::from_millis(16))
            .unwrap();
        let expected = if geometry_available {
            [4.25, 0.0, 2.5]
        } else {
            start
        };
        assert_eq!(state(&room, wire).unwrap().bot.position, expected);
        room.finish_tick(&supported, 2, Duration::from_millis(32))
            .unwrap();
        let body = state(&room, wire).unwrap();
        assert_eq!(body.bot.position, expected);
        assert_eq!(body.bot.velocity, [0.0; 3]);
        assert!(body.enemy.as_ref().unwrap().fault.is_none());
    }
}
