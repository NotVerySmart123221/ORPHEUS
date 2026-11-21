use serde_json::json;

use super::*;
use crate::{game_bots::BotCatalog, game_combat::RoomSimulation, script_snapshot_wire::View};

#[test]
fn unsupported_public_stock_bot_script_does_not_remove_room_bot_or_peer_projection() {
    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/stock-bot-script-isolation-v1.json"
    ))
    .unwrap();
    let cases = fixture["input"].as_array().unwrap();
    let bots = BotCatalog::from_json(
        r#"{
            "heroes": {
                "0x01": {"name":"First","body":"0x11","health":100},
                "0x02": {"name":"Second","body":"0x12","health":100}
            },
            "maps": {"0x03": {"hints": {}, "spawners": [
                {"hero":"0x01","team":1,"position":[1,0,0],"rotation":[0,0,0,1],"graphs":[]},
                {"hero":"0x02","team":1,"position":[2,0,0],"rotation":[0,0,0,1],"graphs":[]}
            ]}}
        }"#,
    )
    .unwrap()
    .make(3, |_| crate::game_bots::BotWalk::default());
    let ids: Vec<_> = bots.iter().map(|bot| bot.entity).collect();
    let mut definitions = BTreeMap::new();
    let mut graphs = Vec::new();
    for (index, (bot, case)) in bots.iter().zip(cases).enumerate() {
        let graph = 601 + index;
        definitions.insert(
            format!("0x{:016X}", bot.spawn.body),
            json!({"name": "Fixture bot", "graphs": [{"graph": graph, "m_75408640": 0}],
                   "weapons": []}),
        );
        graphs.push(json!({
            "index": graph, "items": 2, "entries": [0], "states": [], "remote": [],
            "sync_vars": [], "containers": {}, "unknown_nodes": [], "fields": {},
            "header": {"m_nodesBitCount": 2, "m_statesBitCount": 2,
                "m_remoteSyncNodesBitCount": 2, "m_syncVarsBitCount": 2},
            "nodes": [
                {"pos": 0, "rid": 0, "uid": 0, "remote": -1, "class": "STUStatescriptEntry",
                    "state": null, "server_only": false, "client_only": false,
                    "fields": {"m_outPlug": {"$": "Plug", "links": [[1, "m_inPlug"]]}}},
                {"pos": 1, "rid": 1, "uid": 1, "remote": -1, "class": "STUStatescriptActionSetVar",
                    "state": null, "server_only": false, "client_only": false,
                    "fields": {"m_ADEB6E05": true,
                        "m_out_Var": {"$": "STUConfigVarDynamic", "m_identifier": 29},
                        "m_value": case["selector"]}}
            ]
        }));
    }
    assert_eq!(ids.len(), cases.len());
    let bundle = json!({
        "version": 1, "missing": [], "bodies": {}, "entities": definitions, "graphs": graphs,
    });
    let mut room = RoomSimulation::new(bots).unwrap();
    room.enable_stock_bot_scripts(
        Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap()),
        Arc::new(CodecTable::from_json(r#"{"classes":{}}"#).unwrap()),
        1,
    );
    let created: Vec<_> = room
        .initial_creates()
        .unwrap()
        .iter()
        .map(|create| create.entity)
        .collect();
    assert_eq!(created, ids);
    let projected: Vec<_> = room
        .enemy_projections()
        .unwrap()
        .map(|projection| {
            projection
                .encoder
                .project(projection.snapshot, View::Observer)
                .unwrap();
            projection.snapshot.entity
        })
        .collect();
    let expected: Vec<_> = ids
        .iter()
        .zip(cases)
        .filter_map(|(&entity, case)| case["projects"].as_bool().unwrap().then_some(entity))
        .collect();
    assert_eq!(projected, expected);
}
