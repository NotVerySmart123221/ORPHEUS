use serde_json::json;

use super::*;
use crate::{
    game_bots::BotCatalog, game_combat::RoomSimulation, script_runtime::FRAME_MS,
    script_snapshot_wire::View,
};

#[test]
fn stock_bot_criteria_uses_shared_cue_delivery_to_living_allies() {
    use std::time::Duration;

    use crate::{
        actor_voice::ActorVoiceCatalog,
        contact_world::ContactWorld,
        game_clock::FIRST_TICK,
        game_combat::{
            BotState, PlayerBodyState,
            tests::{fixture_bots, fixture_ground},
        },
        game_combat_messages::CombatNotice,
        game_replication::BodyId,
        mover::Mover,
        mover_settings::{MoverCatalog, MoverSettings},
        runtime::format::parse_guid,
        scene_assets::BotQueries,
        script_graph::RoomSpatialNeeds,
    };

    const TRIGGER: u64 = 0xf000_0000_0000_1350;
    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/player-voice-v1.json"
    ))
    .unwrap();
    let input = &fixture["input"];
    let criterion = &fixture["rust_selection"]["criterion"];
    let cue = &fixture["expected"]["wheel_friendly"][0];
    let mut data = input["fixtures"].clone();
    let skin = input["skin"].as_str().unwrap();
    let hero = input["hero"].as_str().unwrap();
    let base = data["heroes"][hero].as_str().unwrap().to_owned();
    data["heroes"]["0x01"] = data["skins"][skin][base].clone();
    let catalog = Arc::new(serde_json::from_value::<ActorVoiceCatalog>(data).unwrap());
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&json!({
        "version":1,"bodies":{},"missing":[],
        "entities":{"0x02":{"name":"Fixture speaker","weapons":[],"graphs":[{"graph":42,"m_75408640":0}]}},
        "graphs":[{
            "index":42,"items":2,"entries":[0],"states":[],"remote":[],"sync_vars":[],
            "containers":{},"unknown_nodes":[],"fields":{},
            "header":{"m_nodesBitCount":2,"m_statesBitCount":2,"m_remoteSyncNodesBitCount":2,"m_syncVarsBitCount":2},
            "nodes":[
                {"pos":0,"rid":0,"uid":0,"remote":-1,"state":null,"server_only":false,"client_only":false,
                 "class":"STUStatescriptEntryGameMessage","fields":{
                    "m_gameMessage":{"m_gameMessage":format!("0x{TRIGGER:016X}")},
                    "m_outPlug":{"$":"Plug","links":[[1,"m_inPlug"]]}
                 }},
                {"pos":1,"rid":1,"uid":1,"remote":-1,"state":null,"server_only":false,"client_only":false,
                 "class":"STUStatescriptActionCriteria","fields":{
                    "m_identifier":{"$":"STUConfigVarIdentifier","m_identifier":criterion},
                    "m_targets":{"$":"STU_B5A0CAF0"}
                 }}
            ]
        }]
    })).unwrap()).unwrap());
    let codecs = Arc::new(CodecTable::from_json(r#"{"classes":{}}"#).unwrap());
    let ground = Arc::new(fixture_ground());
    let movers = MoverCatalog::default();
    for free_for_all in [false, true] {
        let mut bots = fixture_bots();
        bots.truncate(1);
        let speaker = bots[0].entity;
        let mut room = RoomSimulation::new(bots).unwrap();
        room.enable_stock_bot_scripts(Arc::clone(&graphs), Arc::clone(&codecs), 0);
        room.enable_spatial_queries(free_for_all, Arc::clone(&ground), RoomSpatialNeeds::Queries);
        room.enable_actor_voice(Arc::clone(&catalog));
        for (team, entity) in [(0, 30), (1, 31)] {
            let mover = Mover::new(
                MoverSettings::default(),
                Arc::new(ContactWorld::new(Vec::new()).unwrap()),
                [3.0, 0.0, 0.0],
                0,
                0x18,
            )
            .unwrap();
            room.spawn_player_body(PlayerBodyState::new(
                BodyId {
                    entity,
                    generation: 1,
                },
                entity - 10,
                team,
                team,
                0,
                mover,
                None,
                FIRST_TICK,
                None,
            ))
            .unwrap();
        }
        let mut state = room
            .world
            .get_mut::<BotState>(room.by_wire[&speaker])
            .unwrap();
        assert_eq!(
            state
                .stock_script
                .as_mut()
                .unwrap()
                .component
                .queue_entity_message(TRIGGER, Variables::new())
                .unwrap(),
            1
        );
        let queries = BotQueries::new(free_for_all, &movers, &ground, None);
        room.finish_tick(&queries, FIRST_TICK + 1, Duration::ZERO)
            .unwrap();
        let notices = room.take_player_cues();
        if free_for_all {
            assert!(notices.is_empty());
        } else {
            let [notice] = notices.as_slice() else {
                panic!("expected exactly one allied listener");
            };
            assert_eq!(
                notice.notice,
                CombatNotice::Voice {
                    anchor: speaker,
                    speaker,
                    listener: 31,
                    stimulus: parse_guid(cue[0].as_str().unwrap()).unwrap(),
                    line: parse_guid(cue[1].as_str().unwrap()).unwrap(),
                }
            );
        }
    }
}

#[test]
fn stock_bot_room_messages_run_in_the_next_frame_and_expire_on_death() {
    use std::time::Duration;

    use crate::{
        game_clock::FIRST_TICK,
        game_combat::tests::{fixture_bots, fixture_ground},
        mover_settings::MoverCatalog,
        scene_assets::BotQueries,
    };

    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/script-room-message-v1.json"
    ))
    .unwrap();
    let input = &fixture["input"];
    let expected = &fixture["expected"];
    let mut bundle = input["bundle"].clone();
    bundle["entities"] = json!({
        "0x02": {"name":"Test bot","weapons":[],"graphs":[
            {"graph":input["source_graph"],"m_75408640":0},
            {"graph":input["target_graph"],"m_75408640":0}
        ]}
    });
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap());
    let codecs = Arc::new(CodecTable::from_json(r#"{"classes":{}}"#).unwrap());
    let bots = fixture_bots();
    let sender = bots[0].entity;
    let receiver = bots[1].entity;
    let fixture_vector: [f64; 3] = serde_json::from_value(input["vector"].clone()).unwrap();
    let admitted = input["integer"].as_i64().unwrap();
    let expired = fixture["stock_bot_lifecycle"]["payload_before_recipient_death"]
        .as_i64()
        .unwrap();
    let ground = Arc::new(fixture_ground());
    let movers = MoverCatalog::default();
    let queries = BotQueries::new(false, &movers, &ground, None);
    let mut room = RoomSimulation::new(bots).unwrap();
    room.enable_stock_bot_scripts(Arc::clone(&graphs), codecs, 0);
    room.enable_spatial_queries(false, ground.clone(), graphs.room_spatial_needs());

    let send = |room: &mut RoomSimulation, value| {
        let mut state = room
            .world
            .get_mut::<super::super::BotState>(room.by_wire[&sender])
            .unwrap();
        let script = state.stock_script.as_mut().unwrap();
        for (id, value) in [
            (1, Value::Entity(receiver)),
            (2, Value::Integer(value)),
            (3, Value::Vector(fixture_vector)),
        ] {
            script
                .component
                .set_entity_variable(id, value, &mut script.world)
                .unwrap();
        }
        assert_eq!(
            script
                .component
                .queue_entity_message(input["trigger"].as_u64().unwrap(), Variables::new())
                .unwrap(),
            1
        );
    };
    let received = |room: &RoomSimulation| {
        let state = room
            .world
            .get::<super::super::BotState>(room.by_wire[&receiver])
            .unwrap();
        let script = state.stock_script.as_ref().unwrap();
        let snapshot = script.component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        snapshot.instances[&2].variables.clone()
    };
    let variable = |name: &str| u32::try_from(expected[name].as_u64().unwrap()).unwrap();
    let integer = variable("integer_variable");

    send(&mut room, admitted);
    room.finish_tick(&queries, FIRST_TICK + 1, Duration::ZERO)
        .unwrap();
    assert!(!received(&room).contains_key(&integer));
    room.finish_tick(&queries, FIRST_TICK + 2, Duration::ZERO)
        .unwrap();
    let vars = received(&room);
    assert_eq!(vars.get(&integer), Some(&Value::Integer(admitted)));
    assert_eq!(
        vars.get(&variable("sender_variable")),
        Some(&Value::Entity(sender))
    );
    assert_eq!(
        vars.get(&variable("vector_variable")),
        Some(&Value::Vector(fixture_vector))
    );
    assert!(!vars.contains_key(&variable("ordinary_variable")));

    send(&mut room, expired);
    room.finish_tick(&queries, FIRST_TICK + 3, Duration::ZERO)
        .unwrap();
    room.kill_bot_for_test(receiver, FIRST_TICK + 4, Duration::ZERO);
    assert_eq!(
        received(&room).get(&integer),
        Some(&Value::Integer(admitted))
    );
}

#[test]
fn stock_bot_weapon_placement_uses_map_graphs_and_body_slot_bindings() {
    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/stock-bot-weapon-placement-v1.json"
    ))
    .unwrap();
    let input = &fixture["input"];
    let bot_catalog = BotCatalog::from_json(
        &json!({
            "heroes": {
                "0x01":{"name":"Fixture bot","body":"0x11","health":100},
                "0x02":{"name":"Fixture unarmed","body":"0x12","health":100}
            },
            "maps": {"0x03":{"hints":{},"spawners":[
                {"hero":"0x01","team":1,"position":[0,0,0],"rotation":[0,0,0,1],
                 "graphs":input["graphs"]},
                {"hero":"0x02","team":1,"position":[1,0,0],"rotation":[0,0,0,1],
                 "graphs":input["graphs"]}
            ]}}
        })
        .to_string(),
    )
    .unwrap();
    let bots = bot_catalog.make(3, |_| Default::default());
    let graph = |index, class, client_only| {
        json!({
            "index": index, "items": 2, "entries": [0], "states": [1], "remote": [],
            "sync_vars": [], "containers": {}, "unknown_nodes": [], "fields": {},
            "header": {"m_nodesBitCount": 2, "m_statesBitCount": 2,
                "m_remoteSyncNodesBitCount": 2, "m_syncVarsBitCount": 2},
            "nodes": [
                {"pos": 0, "rid": 0, "uid": 0, "remote": -1, "class": "STUStatescriptEntry",
                 "state": null, "server_only": false, "client_only": false,
                 "fields": {"m_outPlug": {"$":"Plug", "links":[[1, "m_beginPlug"]]}}},
                {"pos": 1, "rid": 1, "uid": 1, "remote": -1, "class":class,
                 "state": 0, "server_only": false, "client_only":client_only,
                 "fields": {"m_2BBEEAB8":1,
                   "m_14EDDDA7":{"$":"STU_B5A0CAF0"},
                   "m_20BCEE62":{"$":"STUConfigVarBool","m_value":true},
                   "m_A60150E2":{"$":"STUConfigVarInt","m_value":input["slot"]}
                 }}
            ]
        })
    };
    let slot = input["slot"].as_u64().unwrap().to_string();
    let catalog = StockBotCatalog {
        graphs: Arc::new(
            GraphStore::from_json(
                &serde_json::to_vec(&json!({
                    "version": 1, "missing": [], "bodies": {},
                    "entities":{
                        "0x11":{"name":"Fixture bot","graphs":[],"weapons":[],
                            "ai_weapon_buttons":{(slot):input["logical_button"]}},
                        "0x12":{"name":"Fixture unarmed","graphs":[],"weapons":[]}
                    },
                    "graphs":[
                        graph(0x0611, "STU_D7F9E1C3", false),
                        graph(0x0612, "STUStatescriptState", false),
                        graph(0x0613, "STU_D7F9E1C3", false),
                        graph(0x0614, "STU_D7F9E1C3", true)
                    ]
                }))
                .unwrap(),
            )
            .unwrap(),
        ),
        codecs: Arc::new(CodecTable::from_json(r#"{"classes":{}}"#).unwrap()),
        mode: 1,
    };
    assert_eq!(bots.len(), 2);
    let mut armed = StockBotScript::start(&catalog, &bots[0], 100, BodyEntry::Initial).unwrap();
    let roots: Vec<_> = armed
        .component
        .snapshot()
        .unwrap()
        .instances
        .values()
        .map(|root| {
            assert!(root.is_server_only());
            root.graph
        })
        .collect();
    assert_eq!(json!(roots), fixture["expected"]["server_roots"]);
    armed.advance(101, &bots[0]);
    assert_eq!(
        json!(armed.component.command_buttons()),
        fixture["expected"]["command_buttons"]
    );

    let unarmed = StockBotScript::start(&catalog, &bots[1], 100, BodyEntry::Initial).unwrap();
    assert!(unarmed.component.snapshot().unwrap().instances.is_empty());
}

#[test]
#[ignore = "requires a prepared 104319 Practice Range pack with friendly bot graphs"]
fn prepared_friendly_bot_nameplate_uses_settled_damage_and_healing() {
    use crate::{
        game_combat::{HealingPulse, health::HealthEffect},
        game_content::GameMap,
        scene_assets::SceneAssets,
    };
    use std::time::Duration;

    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/npc-nameplate-staged-admission-v1.json"
    ))
    .unwrap();
    let values = &fixture["allied_health"];
    let damage = values["bot_damage"].as_f64().unwrap();
    let scripted = values["script_heal"].as_f64().unwrap();
    let field = values["field_heal"].as_f64().unwrap();
    let path = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let assets = SceneAssets::load(path).unwrap();
    let mut scene = assets.prepare(GameMap::practice_range()).unwrap();
    let (room, queries) = scene.bot_simulation();
    let (wire, team, position, maximum, attacker) = {
        let bots: Vec<_> = room.bodies().unwrap().collect();
        let ally = bots.iter().find(|bot| bot.spawn.team == 0).unwrap();
        let enemy = bots
            .iter()
            .find(|bot| bot.spawn.team != ally.spawn.team)
            .unwrap();
        (
            ally.entity,
            ally.spawn.team,
            ally.position,
            ally.spawn.health,
            enemy.entity,
        )
    };
    let source = Death {
        time: 0.0,
        frame: crate::game_clock::FIRST_TICK,
        killer: attacker,
    };
    let check = |room: &mut RoomSimulation, expected| {
        let state = room.state_mut(wire).unwrap().unwrap();
        assert_eq!(state.vitals.total(), expected);
        let script = state.stock_script.as_ref().unwrap();
        assert_eq!(script.world.vitals.total(), expected);
        let snapshot = script.projection().unwrap().snapshot;
        assert!(
            snapshot
                .instances
                .values()
                .any(|instance| instance.graph == 0x0d24)
        );
        assert_eq!(
            snapshot.variables.get(&HUD_HEALTH),
            Some(&Value::Float(expected))
        );
    };
    room.settle_health(wire, HealthEffect::Damage(damage), source, Duration::ZERO)
        .unwrap();
    check(room, maximum - damage);
    room.settle_health(wire, HealthEffect::Heal(scripted), source, Duration::ZERO)
        .unwrap();
    check(room, maximum - damage + scripted);
    let updates = room
        .heal_allies(
            &queries,
            team,
            None,
            None,
            &[HealingPulse {
                centre: position,
                amount: field,
            }],
        )
        .unwrap();
    assert!(updates.iter().any(|update| update.entity == wire));
    check(room, maximum - damage + scripted + field);
}

#[test]
#[ignore = "requires a prepared 104319 Practice Range pack"]
fn prepared_bot_respawn_projects_its_authored_animation_in_the_first_frame() {
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
    let bot = bots
        .iter()
        .find(|bot| {
            bot.spawn
                .placement_graphs
                .iter()
                .copied()
                .filter_map(graph_asset_index)
                .any(|index| {
                    catalog
                        .graphs
                        .graph(index)
                        .is_some_and(|graph| graph.drives_ai_weapon_input())
                })
        })
        .unwrap();
    let replacement = bot.respawned(bot.entity + 100, 300);
    let script = StockBotScript::start(&catalog, &replacement, 292, BodyEntry::Respawn).unwrap();
    let snapshot = script.component.snapshot().unwrap();
    let projection = script
        .projection()
        .unwrap_or_else(|| panic!("{:?}", snapshot.diagnostics));
    assert_eq!(projection.snapshot.entity, replacement.entity);
    let (&instance_id, instance) = snapshot
        .instances
        .iter()
        .find(|(_, instance)| instance.graph == 0x0ba8)
        .unwrap();
    let active: Vec<_> = instance
        .states
        .iter()
        .filter_map(|(&slot, state)| {
            (state.active && state.kind == script_init::StateKind::Anim).then_some(slot)
        })
        .collect();
    assert_eq!(active.len(), 1, "{:?}", snapshot.diagnostics);
    let projected = projection
        .encoder
        .project(projection.snapshot, View::Observer)
        .unwrap();
    assert!(
        projected
            .instance_states(instance_id)
            .unwrap()
            .contains(&active[0])
    );
    projection.encoder.full(&projected).unwrap();
}

#[test]
#[ignore = "requires a prepared 104319 Practice Range pack"]
fn prepared_stock_death_delivers_critical_and_normal_kills_with_the_pulse() {
    use std::time::Duration;

    use crate::{
        game_clock::{FIRST_TICK, TICK_PERIOD},
        game_combat::{ShotContext, tests::fixture_ground},
        game_combat_messages::CombatNotice,
        game_hitscan::{DamageTags, Impact},
        game_instance::RETAIL_QUANTA,
        mover_settings::MoverCatalog,
        scene_assets::BotQueries,
        script_stream::{BodyStream, ChangeKey, DeltaKind},
    };

    let runtime = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let data = runtime.join("data");
    let bots = BotCatalog::load(data.join("ai_spawners_174.json"))
        .unwrap()
        .make(crate::game_content::PRACTICE_RANGE_MAP, |_| {
            Default::default()
        });
    let targets: Vec<_> = bots
        .iter()
        .filter(|bot| bot.spawn.team == 1)
        .take(2)
        .map(|bot| bot.entity)
        .collect();
    assert_eq!(targets.len(), 2);
    let mut room = RoomSimulation::new(bots).unwrap();
    room.enable_stock_bot_scripts(
        Arc::new(GraphStore::load(data.join("game_graphs_174.json.gz")).unwrap()),
        Arc::new(CodecTable::load(data.join("statescript_codecs_174.json")).unwrap()),
        crate::game_content::PRACTICE_RANGE_MODE,
    );

    let ground = fixture_ground();
    let movers = MoverCatalog::default();
    let queries = BotQueries::new(false, &movers, &ground, None);
    let killer = 0xa000_0100;
    let loadout = 0x0b90_0000_0000_008d;
    let mut tick = FIRST_TICK + 1;
    let mut now = Duration::from_secs(1);
    for (target, critical) in targets.into_iter().zip([false, true]) {
        let mut stream = BodyStream::new(target);
        stream.create_arrived();
        let projection = room
            .enemy_projections()
            .unwrap()
            .find(|p| p.snapshot.entity == target)
            .unwrap();
        let before = projection
            .encoder
            .project(projection.snapshot, View::Observer)
            .unwrap();
        let full = projection.encoder.queue_full(&mut stream, &before).unwrap();
        assert!(stream.arrived(full.last).advanced);
        let hit = room
            .apply_bot_impact(
                killer,
                tick,
                ShotContext {
                    team: 0,
                    quanta: RETAIL_QUANTA,
                    tick,
                    now,
                },
                &Impact {
                    tags: DamageTags::EMPTY,
                    entity: target,
                    distance: 1.0,
                    damage: 10000.0,
                    critical,
                    loadout: Some(loadout),
                    impulse: None,
                    geometry: None,
                },
            )
            .unwrap()
            .unwrap();
        assert!(
            matches!(hit.notices[0], Some(CombatNotice::Hit { critical: flag, killed: true, .. }) if flag == critical)
        );

        for _ in 0..8 {
            tick += 1;
            now += TICK_PERIOD;
            assert!(room.finish_tick(&queries, tick, now).unwrap().is_empty());
        }
        let projection = room
            .enemy_projections()
            .unwrap()
            .find(|p| p.snapshot.entity == target)
            .unwrap();
        let (&instance, graph) = projection
            .snapshot
            .instances
            .iter()
            .find(|(_, graph)| graph.graph == 0x0c18)
            .unwrap();
        assert!(matches!(
            graph.states[&8].kind,
            script_init::StateKind::ClientPulse { count: 1, .. }
        ));
        let after = projection
            .encoder
            .project_next(projection.snapshot, stream.base().unwrap().1)
            .unwrap();
        let fields = after.instance_variables(instance).unwrap();
        assert_eq!(fields.get(&392), Some(&Value::Entity(killer)));
        assert_eq!(fields.get(&1821), Some(&Value::Bool(critical)));
        assert_eq!(fields.get(&9300), Some(&Value::Asset(loadout)));
        let delta = projection
            .encoder
            .queue_delta(
                &mut stream,
                &after,
                DeltaKind::Observer { resend: true },
                FRAME_MS as u32,
            )
            .unwrap()
            .unwrap();
        let changed = stream.touched();
        assert!(changed.contains(&ChangeKey::State(instance, 8)));
        let previously_sent = before
            .instance_variables(instance)
            .and_then(|values| values.get(&1821));
        assert_eq!(
            changed.contains(&ChangeKey::Variable(instance, 1821)),
            previously_sent != Some(&Value::Bool(critical)),
        );
        assert!(stream.arrived(delta.last).advanced);
        assert_eq!(
            stream
                .base()
                .unwrap()
                .1
                .instance_variables(instance)
                .unwrap()
                .get(&1821),
            Some(&Value::Bool(critical)),
        );
    }
}

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
    let mut teams = std::collections::BTreeSet::new();
    for bot in &bots {
        if bot
            .spawn
            .placement_graphs
            .iter()
            .copied()
            .filter_map(graph_asset_index)
            .any(|index| {
                catalog
                    .graphs
                    .graph(index)
                    .is_some_and(|graph| graph.drives_ai_weapon_input())
            })
        {
            check_stock_bot_firing(&catalog, bot);
            teams.insert(bot.spawn.team);
        }
    }
    assert_eq!(teams, std::collections::BTreeSet::from([0, 1]));
}

fn check_stock_bot_firing(catalog: &StockBotCatalog, bot: &Bot) {
    use crate::script_stream::{BodyStream, ChangeKey, DeltaKind};

    let start = 100;
    let mut script = StockBotScript::start(catalog, bot, start, BodyEntry::Initial).unwrap();
    let mut held_ticks = 0;
    let mut released_after_holding = false;
    let mut projectiles = 0;
    let mut projected_volley = false;
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
        for batch in script.take_projectiles(tick, bot) {
            if let Some(presentation) = batch.presentation {
                assert_eq!(presentation.volley.event.entity, bot.entity);
                assert_eq!(presentation.launched, tick);
                let projection = script.projection().unwrap();
                let mut projected = projection
                    .encoder
                    .project(projection.snapshot, View::Observer)
                    .unwrap();
                let instance = presentation.volley.event.instance;
                // 104319's authored client data flow reads this transient
                // instance variable from the single-shot volley's stack.
                let firing_value = |snapshot: &crate::script_snapshot_wire::ProjectedSnapshot| {
                    snapshot
                        .instance_variables(instance)
                        .and_then(|vars| vars.get(&233))
                        .cloned()
                };
                assert_ne!(firing_value(&projected), Some(Value::Bool(true)));
                assert!(!projected.has_volley(&presentation.volley));
                let mut stream = BodyStream::new(bot.entity);
                stream.create_arrived();
                let full = projection
                    .encoder
                    .queue_full(&mut stream, &projected)
                    .unwrap();
                assert!(stream.arrived(full.last).advanced);
                assert!(projected.retain_volley(&presentation.volley));
                assert_eq!(firing_value(&projected), Some(Value::Bool(true)));
                assert!(projected.has_volley(&presentation.volley));
                let delta = projection
                    .encoder
                    .queue_delta(
                        &mut stream,
                        &projected,
                        DeltaKind::Observer { resend: true },
                        crate::script_runtime::FRAME_MS as u32,
                    )
                    .unwrap()
                    .unwrap();
                assert!(
                    stream
                        .touched()
                        .contains(&ChangeKey::Variable(instance, 233))
                );
                assert!(stream.arrived(delta.last).advanced);
                assert!(stream.base().unwrap().1.has_volley(&presentation.volley));
                projected_volley = true;
            }
            for projectile in batch.flights {
                assert_eq!(projectile.source, bot.entity);
                assert_eq!(projectile.team, bot.spawn.team);
                projectiles += 1;
            }
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
    // A single-shot volley can already be off. Its activation still
    // must be projected so the observer can associate the launch.
    assert!(
        projected_volley,
        "the observer never received the volley identity"
    );
}

#[test]
#[ignore = "requires a prepared 104319 Practice Range pack with firing graph state metadata"]
fn prepared_stock_firing_bot_walks_back_after_displacement() {
    use crate::{
        contact_world::ContactWorld,
        game_combat::{BotQueries, tests::fixture_ground},
        mover_settings::MoverCatalog,
    };

    let data =
        std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap()).join("data");
    let graphs = Arc::new(GraphStore::load(data.join("game_graphs_174.json.gz")).unwrap());
    let codecs = Arc::new(CodecTable::load(data.join("statescript_codecs_174.json")).unwrap());
    let mut bot = BotCatalog::load(data.join("ai_spawners_174.json"))
        .unwrap()
        .make(crate::game_content::PRACTICE_RANGE_MAP, |_| {
            Default::default()
        })
        .into_iter()
        .find(|bot| {
            bot.spawn
                .placement_graphs
                .iter()
                .copied()
                .filter_map(graph_asset_index)
                .any(|index| {
                    graphs
                        .graph(index)
                        .is_some_and(|graph| graph.drives_ai_weapon_input())
                })
        })
        .unwrap();
    let start = [0.0; 3];
    bot.position = start;
    let aim = bot.yaw;
    let wire = bot.entity;
    let mut room = RoomSimulation::new(vec![bot]).unwrap();
    room.enable_stock_bot_scripts(graphs, codecs, crate::game_content::PRACTICE_RANGE_MODE);
    let ground = fixture_ground();
    let movers = MoverCatalog::default();
    let contact = Arc::new(ContactWorld::new(Vec::new()).unwrap());
    let queries = BotQueries::new(false, &movers, &ground, Some(&contact));
    let first_tick = 1000;
    for tick in first_tick..first_tick + 200 {
        room.advance_stock_bots(&queries, tick);
    }
    room.body_mut(wire).unwrap().position = [2.0, 0.0, 0.0];
    let mut walked = false;
    for tick in first_tick + 200..first_tick + 380 {
        room.advance_stock_bots(&queries, tick);
        let state = room.state_mut(wire).unwrap().unwrap();
        let script = state.stock_script.as_ref().unwrap();
        walked |= script.walk_request().is_some();
        let close = state.bot.position[0].abs() < 0.4;
        if walked && close && script.walk_request().is_none() {
            assert_eq!(state.bot.yaw, aim);
            assert!(
                !script.faulted,
                "{:?}",
                script.component.snapshot().unwrap().diagnostics
            );
            return;
        }
    }
    let state = room.state_mut(wire).unwrap().unwrap();
    let script = state.stock_script.as_ref().unwrap();
    panic!(
        "walked={walked}, position={:?}, diagnostics={:?}",
        state.bot.position,
        script.component.snapshot().map(|s| &s.diagnostics)
    );
}

#[test]
#[ignore = "requires a prepared 104319 Practice Range pack with authored bot return"]
fn prepared_stock_firing_bot_returns_after_room_knockback() {
    use crate::{
        game_clock::{FIRST_TICK, SEND_EVERY, TICK_PERIOD},
        game_content::GameMap,
        patrol_script::PatrolScript,
        scene_assets::SceneAssets,
    };
    use std::time::Duration;
    let path = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let assets = SceneAssets::load(&path).unwrap();
    let mut scene = assets.prepare(GameMap::practice_range()).unwrap();
    let patrol = PatrolScript::load().unwrap();
    let target = scene
        .simulation()
        .checked_states()
        .unwrap()
        .into_iter()
        .find(|state| {
            state.bot.spawn.team == 1
                && !state.bot.patrol_active()
                && state
                    .stock_script
                    .as_ref()
                    .is_some_and(|script| !script.world.placement_graphs.is_empty())
        })
        .expect("stock firing bot");
    let wire = target.bot.entity;
    let home = target.bot.position;
    let distance = |position: [f64; 3]| (position[0] - home[0]).hypot(position[2] - home[2]);
    let mut maximum: f64 = 0.0;
    let mut walked = false;
    let mut published_return_stride = false;
    for step in 1..=430 {
        let tick = FIRST_TICK + step;
        if step == 120 {
            scene
                .simulation_mut()
                .queue_combat_impulse(wire, tick - 1, [4.0, 5.0, 0.0])
                .unwrap();
        }
        scene.step_bots(tick, &patrol).unwrap();
        let (room, queries) = scene.bot_simulation();
        let now = TICK_PERIOD * step + Duration::from_secs(1);
        room.finish_tick(&queries, tick, now).unwrap();
        if step <= 120 {
            continue;
        }
        let state = room.state_mut(wire).unwrap().unwrap();
        let script = state.stock_script.as_ref().unwrap();
        maximum = maximum.max(distance(state.bot.position));
        walked |= script.walk_request().is_some();
        if script.walk_request().is_some() && !state.bot.has_combat_motion() {
            let pose = state.bot.movement(tick);
            let toward_home = [0, 2]
                .into_iter()
                .map(|axis| pose.velocity[axis] * (home[axis] - pose.position[axis]))
                .sum::<f64>()
                > 0.0;
            published_return_stride |= toward_home
                && pose.throttles != [0; 2]
                && tick % SEND_EVERY == 0
                && state.bot.record_due(tick, SEND_EVERY);
        }
        if maximum > 0.6
            && walked
            && distance(state.bot.position) < 0.4
            && script.walk_request().is_none()
        {
            assert!(!state.bot.has_combat_motion());
            assert!(
                published_return_stride,
                "no observer locomotion during return"
            );
            assert_eq!(state.bot.movement(tick).throttles, [0; 2]);
            return;
        }
    }
    let state = scene.simulation_mut().state_mut(wire).unwrap().unwrap();
    let script = state.stock_script.as_ref().unwrap();
    panic!(
        "home={home:?}; position={:?}; max={maximum}; requested={walked}; current={:?}; diagnostics={:?}",
        state.bot.position,
        script.walk_request(),
        script.component.snapshot().unwrap().diagnostics
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
    let fixture = |front_team: u8, rear_team: u8| {
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
                    {"hero":"0x02","team":front_team,"position":[0,0,2],"rotation":[0,0,0,1],"graphs":[]},
                    {"hero":"0x02","team":rear_team,"position":[0,0,4],
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
    for (has_player, front_team) in [(false, 1), (true, 1), (true, 0)] {
        let bots = fixture(front_team, u8::from(has_player));
        let front = bots[1].entity;
        let npc_target = bots[2].entity;
        let mut room = RoomSimulation::new(bots).unwrap();
        room.enable_stock_bot_scripts(
            Arc::clone(&graphs),
            Arc::clone(&codecs),
            crate::game_content::PRACTICE_RANGE_MODE,
        );
        let start = crate::game_clock::FIRST_TICK;
        let player = if has_player {
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
            hit = match (front_team, player) {
                (0, _) => room.body_healths().unwrap()[&front] < 100.0,
                (_, Some((id, initial))) => {
                    room.player_body(id).unwrap().vitals().unwrap().total() < initial
                }
                (_, None) => room.body_healths().unwrap()[&npc_target] < 100.0,
            };
            if hit {
                break;
            }
        }
        assert!(
            hit,
            "no damage with has_player={has_player}, front_team={front_team}"
        );
        if front_team == 1 {
            assert_eq!(room.body_healths().unwrap()[&front], 100.0);
        } else if let Some((id, initial)) = player {
            assert_eq!(
                room.player_body(id).unwrap().vitals().unwrap().total(),
                initial
            );
        }
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
