use serde_json::json;

use super::*;
use crate::{game_bots::BotCatalog, game_combat::RoomSimulation, script_snapshot_wire::View};

#[test]
#[ignore = "requires a prepared 104319 Practice Range pack with firing graph state metadata"]
fn prepared_firing_placement_drives_the_stock_weapon_graph() {
    let runtime = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let data = runtime.join("data");
    let catalog = StockBotCatalog {
        graphs: Arc::new(GraphStore::load(data.join("game_graphs_174.json.gz")).unwrap()),
        codecs: Arc::new(CodecTable::load(data.join("statescript_codecs_174.json")).unwrap()),
        mode: crate::game_content::PRACTICE_RANGE_MODE,
    };
    let bots = BotCatalog::load(data.join("ai_spawners_174.json"))
        .unwrap()
        .make(crate::game_content::PRACTICE_RANGE_MAP, |_| {
            Default::default()
        });
    let bot = bots.iter().find(|bot| bot.spawn.firing).unwrap();
    let start = 100;
    let mut script = StockBotScript::start(&catalog, bot, start).unwrap();
    let mut held_ticks = 0;
    let mut released_after_holding = false;
    let mut projectiles = 0;
    let mut first_held_weapon_state = None;
    for tick in start + 1..=start + 800 {
        script.advance(tick, bot);
        let held = script.component.command_buttons() & crate::commands::FIRE != 0;
        if held && first_held_weapon_state.is_none() {
            first_held_weapon_state = script.component.snapshot().map(|snapshot| {
                let active = snapshot
                    .instances
                    .values()
                    .find(|instance| instance.graph == 0x0ba3)
                    .map(|instance| {
                        instance
                            .states
                            .iter()
                            .filter_map(|(&slot, state)| state.active.then_some((slot, state.kind)))
                            .collect::<Vec<_>>()
                    });
                let variables = [31, 1216].map(|id| snapshot.variables.get(&id).cloned());
                (active, variables)
            });
        }
        held_ticks += u32::from(held);
        released_after_holding |= held_ticks > 0 && !held;
        for projectile in script.take_projectiles(tick, bot) {
            assert_eq!(projectile.source, bot.entity);
            assert_eq!(projectile.team, bot.spawn.team);
            projectiles += 1;
        }
    }
    let snapshot = script.component.snapshot().unwrap();
    assert!(!script.faulted, "{:?}", snapshot.diagnostics);
    assert!(held_ticks > 0, "{:?}", snapshot.diagnostics);
    assert!(released_after_holding);
    assert!(
        projectiles > 0,
        "{first_held_weapon_state:?}; {:?}",
        snapshot.diagnostics,
    );
}

#[test]
#[ignore = "requires a prepared 104319 Practice Range pack with firing graph state metadata"]
fn prepared_stock_bot_projectiles_damage_live_room_bodies() {
    use std::time::Duration;

    use crate::{
        contact_world::ContactWorld,
        game_combat::{BotQueries, PlayerBodyState, tests::fixture_ground},
        game_heroes::SOLDIER,
        game_replication::BodyId,
        mover::Mover,
        mover_settings::{MoverCatalog, MoverSettings},
        script_player::fixture_body_factory,
    };

    let runtime = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let data = runtime.join("data");
    let graphs = Arc::new(GraphStore::load(data.join("game_graphs_174.json.gz")).unwrap());
    let codecs = Arc::new(CodecTable::load(data.join("statescript_codecs_174.json")).unwrap());
    let fixture = |target_team: u8| {
        BotCatalog::from_json(
            &json!({
                "heroes": {
                    "0x02E000000000016B": {
                        "name":"Firing NPC", "body":"0x0400000000001403", "health":200
                    },
                    "0x02": {"name":"Fixture target", "body":"0x02", "health":100}
                },
                "maps": {"0x03": {"hints": {}, "spawners": [
                    {"hero":"0x02E000000000016B","team":1,"position":[0,0,0],
                     "rotation":[0,0,0,1],"graphs":["0x0580000000000B92"]},
                    {"hero":"0x02","team":1,"position":[0,0,2],"rotation":[0,0,0,1],"graphs":[]},
                    {"hero":"0x02","team":target_team,"position":[0,0,4],
                     "rotation":[0,0,0,1],"graphs":[]}
                ]}}
            })
            .to_string(),
        )
        .unwrap()
        .make(3, |_| Default::default())
    };
    let ground = fixture_ground();
    let movers = MoverCatalog::default();
    let queries = BotQueries::new(false, &movers, &ground, None);
    for hit_player in [false, true] {
        let bots = fixture(u8::from(hit_player));
        let ally = bots[1].entity;
        let npc_target = bots[2].entity;
        let mut room = RoomSimulation::new(bots).unwrap();
        room.enable_stock_bot_scripts(
            Arc::clone(&graphs),
            Arc::clone(&codecs),
            crate::game_content::PRACTICE_RANGE_MODE,
        );
        let start = crate::game_clock::FIRST_TICK;
        let player = if hit_player {
            let (factory, mut spawn) = fixture_body_factory();
            spawn.frame = start;
            spawn.pose.position = [0.0, 0.0, 4.0];
            spawn.body_filter = crate::match_scene::team_bit(0).unwrap();
            let id = BodyId {
                entity: spawn.entity,
                generation: 1,
            };
            let mover = Mover::new(
                MoverSettings::default(),
                Arc::new(ContactWorld::new(Vec::new()).unwrap()),
                spawn.pose.position,
                0,
                0,
            )
            .unwrap();
            let runtime = Box::new(factory.start(spawn).unwrap());
            let state = PlayerBodyState::new(
                id,
                spawn.controller,
                spawn.slot,
                0,
                SOLDIER,
                mover,
                Some(runtime),
                start,
                None,
            );
            let initial = state.vitals().unwrap().total();
            room.spawn_player_body(state).unwrap();
            Some((id, initial))
        } else {
            None
        };
        let mut hit = false;
        for tick in start + 1..=start + 800 {
            let elapsed = Duration::from_millis(u64::from(tick - start) * FRAME_MS);
            room.finish_tick(&queries, tick, elapsed).unwrap();
            hit = match player {
                Some((id, initial)) => {
                    room.player_body(id).unwrap().vitals().unwrap().total() < initial
                }
                None => room.body_healths().unwrap()[&npc_target] < 100.0,
            };
            if hit {
                break;
            }
        }
        assert!(hit, "no damage for hit_player={hit_player}");
        assert_eq!(room.body_healths().unwrap()[&ally], 100.0);
    }
}

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
