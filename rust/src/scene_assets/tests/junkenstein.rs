//! Junkenstein encounter, gate, boss damage, and hero replay regressions.

use super::*;

#[test]
fn junkenstein_participant_queries_use_live_room_bodies_and_reject_unknown_filters() {
    use serde_json::json;
    let catalog = BotCatalog::from_json(
        &json!({
            "heroes":{"0x01":{"name":"fixture","body":"0x02","health":100}},
            "maps":{"0x03":{"hints":{},"spawners":[
                {"hero":"0x01","team":1,"position":[0,0,0],"rotation":[0,0,0,1],"graphs":[]},
                {"hero":"0x01","team":1,"position":[0,0,0],"rotation":[0,0,0,1],"graphs":[]},
                {"hero":"0x01","team":0,"position":[0,0,0],"rotation":[0,0,0,1],"graphs":[]},
                {"hero":"0x01","team":1,"position":[0,0,0],"rotation":[0,0,0,1],"graphs":[]}
            ]}}
        })
        .to_string(),
    )
    .unwrap();
    let mut bodies = RoomSimulation::new(catalog.make(3, |_| BotWalk::default())).unwrap();
    bodies
        .for_each_body_mut(|bot| bot.dead = bot.entity == BOT_ENTITY + 1)
        .unwrap();
    let gate = JunkensteinGate {
        entity: 0x8000_0032,
        position: [0.0; 3],
        graph: 0x0ca7,
    };
    let graphs =
        GraphStore::from_json(br#"{"version":1,"graphs":[],"bodies":{},"missing":[]}"#).unwrap();
    let world = EncounterWorld {
        graphs: &graphs,
        mode: crate::game_content::JUNKENSTEIN_REVENGE,
        gate: &gate,
        anchors: &SpawnAnchors::default(),
        gate_variables: &Variables::new(),
        bodies: &mut bodies,
        creator: None,
        result_action: &mut None,
        screens_complete: &mut false,
        team: 0,
        difficulty: 0,
    };
    let site = Site {
        entity: GAME_MODE_ENTITY,
        instance: 1,
        graph: 0x0cba,
        node: Some(56),
    };
    let filter =
        json!({"$":"STUConfigVarFilter","m_84065FD1":1,"m_1E14CCA8":0x1000000,"m_4EA686DB":0x10});
    assert_eq!(
        world.participant_targets(site, &filter),
        Some(vec![BOT_ENTITY, BOT_ENTITY + 3])
    );
    let mut unsupported = filter.clone();
    unsupported["m_condition"] = json!(true);
    assert_eq!(world.participant_targets(site, &unsupported), None);
    let mut unsupported = filter.clone();
    unsupported["m_1E14CCA8"] = json!(0x800000);
    assert_eq!(world.participant_targets(site, &unsupported), None);
    assert_eq!(
        world.participant_targets(Site { entity: 0, ..site }, &filter),
        None
    );
    let mut unsupported = filter.clone();
    unsupported["m_B486603E"] = json!(0);
    assert_eq!(world.participant_targets(site, &unsupported), None);
}

#[test]
#[ignore = "requires the separately prepared 104319 Junkenstein collision and graph data"]
fn junkenstein_prepares_all_card_difficulties_and_challenges() {
    use crate::{
        game_content::{JUNKENSTEIN_MAP, MapCatalog},
        lobby_arcade::JUNKENSTEIN,
    };

    let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let assets = SceneAssets::load(&runtime).unwrap();
    let maps = MapCatalog::load(runtime.join("data/game_maps_174.json")).unwrap();
    for &index in JUNKENSTEIN {
        let card = JunkensteinCard::from_index(index).unwrap();
        let map = maps
            .game_map(
                JUNKENSTEIN_MAP,
                card.mode,
                "Junkenstein's Revenge",
                false,
                &[card.players, 0],
            )
            .unwrap();
        assert!(map.placeables.contains(&50));
        let scene = assets
            .prepare_junkenstein(map, card, 0)
            .unwrap_or_else(|error| panic!("{index:03X}: {error}"));
        assert!(scene.movement_ready());
        let bytes = scene.mode_ui().as_bytes();
        let mut wire = crate::bits::BitReader::new(bytes);
        assert_eq!(wire.bits(2).unwrap(), 0); // observer full frame
        let snapshot = scene.encounter_snapshot().unwrap();
        let mut id = 0;
        let mut descriptors = Vec::new();
        loop {
            let delta = wire.w_u16().unwrap();
            if delta == 0 {
                break;
            }
            id += delta;
            let graph = wire.bits(16).unwrap() as u32;
            assert_eq!(wire.bits(2).unwrap(), 0); // descriptor local to this entity
            let parent = if wire.bit().unwrap() {
                Some((
                    wire.w_var().unwrap().try_into().unwrap(),
                    wire.w_var().unwrap().try_into().unwrap(),
                ))
            } else {
                None
            };
            if id == 1 {
                // The client common UI root differs from the server common graph.
                assert_eq!((graph, parent), (0x0cb6, None));
            } else {
                assert_eq!(snapshot.instances[&id].graph, graph);
                assert_eq!(snapshot.instances[&id].parent, parent);
            }
            assert!(!wire.bit().unwrap()); // no descriptor references
            descriptors.push((id, graph, parent));
            assert!(descriptors.len() <= snapshot.instances.len());
        }
        let hud = descriptors
            .iter()
            .find(|(_, graph, parent)| *graph == 0x0cbb && *parent == Some((2, 6)))
            .expect("common HUD descriptor")
            .0;
        for (graph, parent) in [
            (0x0cb6, None),
            (0x0c8c, None),
            (0x0cbb, Some((2, 6))),
            (0x03f2, Some((hud, 44))),
            (0x180a, Some((hud, 45))),
            (0x1188, Some((2, 68))),
        ] {
            assert!(
                descriptors
                    .iter()
                    .any(|(_, actual, actual_parent)| *actual == graph && *actual_parent == parent),
                "card {index:03X} missing HUD descriptor {graph:04X} with parent {parent:?}"
            );
        }
        let gate = scene.junkenstein_gate().unwrap();
        assert_eq!(gate.entity, 0x8000_0032);
        assert_eq!(gate.graph, 0x0ca7);
        assert_eq!(
            gate.position,
            [89.34700012207031, 12.671699523925781, -62.7287712097168]
        );
        let snapshot = scene.encounter_snapshot().unwrap();
        assert!(
            !snapshot
                .diagnostics
                .iter()
                .any(|diagnostic| matches!(&diagnostic.issue,
            script_init::Issue::Expression(crate::script_expr::EvalError::UnsupportedConfig(class))
                if class=="STU_A753DC79")),
            "card {index:03X}: {:?}",
            snapshot.diagnostics
        );
        assert_eq!(
            snapshot.variables.get(&6751),
            Some(&Value::Asset(card.difficulty))
        );
        for flag in [0x7d53, 0x7d54, 0x7d60, 0x7d61, 0x7d62, 0x7d63] {
            assert_eq!(
                snapshot.variables.get(&flag) == Some(&Value::Bool(true)),
                card.challenge == Some(flag)
            );
        }
        assert_eq!(
            snapshot.variables.get(&0x7e0f),
            (index == 0x16f).then_some(&Value::Float(1.0))
        );
        assert_eq!(
            snapshot.variables.get(&8439),
            Some(&Value::Array(vec![Value::Entity(gate.entity)]))
        );
        assert!(
            !snapshot
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.site.graph == 0x0c8c
                    && matches!(diagnostic.site.node, Some(6 | 15 | 263))),
            "{:?}",
            snapshot.diagnostics
        );
    }
}

#[test]
#[ignore = "requires separately prepared 104319 Junkenstein runtime"]
fn normal_cassidy_receives_victory_view_and_result() {
    use crate::game_content::{JUNKENSTEIN_MAP, MapCatalog};
    use crate::{
        npc_catalog::{NpcCatalog, encounter_definition},
        script_enemy::EnemySpawn,
        script_init::{CreationKind, EntityCreation},
    };
    let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let assets = SceneAssets::load(&runtime).unwrap();
    let card = JunkensteinCard::from_index(0x3c).unwrap();
    let map = MapCatalog::load(runtime.join("data/game_maps_174.json"))
        .unwrap()
        .game_map(
            JUNKENSTEIN_MAP,
            card.mode,
            "Junkenstein's Revenge",
            false,
            &[4, 0],
        )
        .unwrap();
    let mut scene = assets.prepare_junkenstein(map, card, 0).unwrap();
    let boss = crate::junkenstein_navigation::ROADHOG;
    let catalog = NpcCatalog::load(runtime.join("data/junkenstein/junkenstein-npcs.json")).unwrap();
    let definition =
        encounter_definition(Some(&catalog), &assets.graphs, scene.heroes(), boss).unwrap();
    let request = EntityCreation {
        position: [82.63870239257813, 12.682595252990723, -83.56700115966802],
        kind: CreationKind::Hero {
            hero: boss,
            team: 1,
            yaw: 180.0,
            flags: [Value::Bool(false), Value::Bool(false)],
            controller: Some(assets.graphs.enemy_pair(boss).unwrap().controller),
            skin: None,
        },
    };
    let subject = scene
        .bot_simulation()
        .0
        .create_enemy(EnemySpawn {
            graphs: Arc::clone(&assets.graphs),
            codecs: Arc::clone(assets.codecs.as_ref().unwrap()),
            definition: &definition,
            request: &request,
            mode: card.mode,
            mode_variables: Arc::new(Variables::from([
                (6751, Value::Asset(card.difficulty)),
                (9948, Value::Integer(0)),
            ])),
            tick: FIRST_TICK,
            walk: BotWalk::default(),
            gate: None,
        })
        .unwrap();
    assert_eq!(
        scene.simulation.boss_view_subjects().unwrap()[&subject]
            .tags
            .presence(0x0d60_0000_0000_011e),
        Some(true)
    );
    let hero = scene.heroes().find_hero("Cassidy").unwrap();
    let mover = scene.player_spawn_mover(hero.body, 0).unwrap();
    let entity = 0xa000_0101;
    let mut owner = scene
        .owner_body_runtime(
            hero,
            OwnerBodySpawn {
                slot: 0,
                controller: entity - 1,
                entity,
                body_filter: team_bit(0).unwrap(),
                frame: FIRST_TICK,
                pose: mover.pose(),
                eye_position: script_eye_position(hero.guid, &mover),
                quanta: RETAIL_QUANTA,
            },
        )
        .unwrap()
        .unwrap();
    for (message, params) in [
        (
            crate::junkenstein_messages::VICTORY_VIEW,
            Variables::from([(0x390, Value::Bool(true)), (8, Value::Entity(entity))]),
        ),
        (
            crate::junkenstein_messages::VICTORY_VIEW,
            Variables::from([(0x390, Value::Bool(false))]),
        ),
        (
            crate::junkenstein_messages::VICTORY_VIEW,
            Variables::from([(0x390, Value::Bool(true)), (8, Value::Entity(subject))]),
        ),
        (
            crate::junkenstein_messages::VICTORY_VIEW,
            Variables::from([(0x390, Value::Bool(false))]),
        ),
        (
            crate::junkenstein_messages::ENCOUNTER_RESULT,
            Variables::from([
                (0x3ae, Value::Bool(true)),
                (0x3ad, Value::Entity(subject)),
                (0x3b4, Value::Bool(true)),
                (0x3b5, Value::Integer(3)),
            ]),
        ),
    ] {
        owner.set_deadeye_scene(&scene, 0).unwrap();
        assert!(
            owner
                .receive_defender_message(DefenderMessage {
                    target: None,
                    source: GAME_MODE_ENTITY,
                    message,
                    parameters: params,
                })
                .unwrap()
                > 0
        );
        let snapshot = owner.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "message {message:X}: {:?}",
            snapshot.diagnostics
        );
        let encoder = owner.encoder();
        encoder
            .full(
                &encoder
                    .project(snapshot, crate::script_snapshot_wire::View::Owner)
                    .unwrap(),
            )
            .unwrap();
    }
}

#[test]
#[ignore = "requires separately prepared 104319 Junkenstein runtime"]
fn configured_normal_validation_changes_gate_capacity_and_scopes_immunity() {
    use crate::game_content::{JUNKENSTEIN_MAP, MapCatalog};
    use crate::junkenstein_validation::JunkensteinValidation;

    let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let mut assets = SceneAssets::load(&runtime).unwrap();
    let maps = MapCatalog::load(runtime.join("data/game_maps_174.json")).unwrap();
    let prepare = |assets: &SceneAssets, index| {
        let card = JunkensteinCard::from_index(index).unwrap();
        let map = maps
            .game_map(
                JUNKENSTEIN_MAP,
                card.mode,
                "Junkenstein's Revenge",
                false,
                &[4, 0],
            )
            .unwrap();
        assets.prepare_junkenstein(map, card, 0).unwrap()
    };
    let capacity = |scene: &PreparedScene| {
        scene.gate_resolved_variable(
            0,
            crate::script_expr::Variable {
                scope: crate::script_expr::Scope::Entity,
                id: 7786,
            },
        )
    };
    let default = prepare(&assets, 0x3c);
    let harder = prepare(&assets, 0x3d);
    assert!(!default.protects_cassidy(Some(CASSIDY)));
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        file.path(),
        br#"{"invulnerable_cassidy":true,"gate_capacity":1000000000}"#,
    )
    .unwrap();
    assets.set_junkenstein_validation(JunkensteinValidation::load(file.path()).unwrap());
    let configured = prepare(&assets, 0x3c);
    assert!(configured.protects_cassidy(Some(CASSIDY)));
    assert!(!configured.protects_cassidy(Some(crate::game_heroes::SOLDIER)));
    assert_eq!(capacity(&configured), Some(Value::Float(1_000_000_000.0)));
    assert_ne!(capacity(&default), capacity(&configured));
    let untouched = prepare(&assets, 0x3d);
    assert!(!untouched.protects_cassidy(Some(CASSIDY)));
    assert_eq!(capacity(&untouched), capacity(&harder));
    assert_eq!(untouched.mode_ui().as_bytes(), harder.mode_ui().as_bytes());
}

#[test]
#[ignore = "requires separately prepared 104319 Junkenstein runtime"]
fn normal_gate_damage_stages_publish_the_authored_animations() {
    use crate::game_content::{JUNKENSTEIN_MAP, MapCatalog};
    use crate::script_snapshot_wire::View;

    let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let assets = SceneAssets::load(&runtime).unwrap();
    let card = JunkensteinCard::from_index(0x3c).unwrap();
    let map = MapCatalog::load(runtime.join("data/game_maps_174.json"))
        .unwrap()
        .game_map(
            JUNKENSTEIN_MAP,
            card.mode,
            "Junkenstein's Revenge",
            false,
            &[4, 0],
        )
        .unwrap();
    let mut scene = assets.prepare_junkenstein(map, card, 0).unwrap();
    let mut previous = None;
    for (frame, health, raw, alias) in [
        (1, 1.0, 10, 0x050d),
        (2, 0.75, 20, 0x050f),
        (3, 0.5, 21, 0x0510),
        (4, 0.25, 22, 0x0511),
        (5, 0.0, 22, 0x0511),
    ] {
        if frame != 0 {
            scene
                .step_test_gate_frame(
                    frame,
                    0x0240_0000_0000_06b2,
                    Variables::from([
                        (0x03ac, Value::Float(if frame == 1 { 0.0 } else { 250.0 })),
                        (0x12, Value::Entity(0)),
                    ]),
                )
                .unwrap();
        }
        let snapshot = scene.gate_snapshot().unwrap();
        assert!(
            snapshot.diagnostics.is_empty(),
            "{frame}: {:?}",
            snapshot.diagnostics
        );
        assert_eq!(
            crate::script_expr::to_float(&snapshot.variables[&8571]),
            health
        );
        assert_eq!(
            snapshot.instances[&1].states[&2].kind,
            script_init::StateKind::BooleanSwitch(health > 0.0)
        );
        let graph = assets.graphs.graph(0x0ca7).unwrap();
        let node = graph.node(raw).unwrap();
        assert!(
            snapshot.instances[&1]
                .states
                .get(&node.state.unwrap())
                .is_some_and(|state| state.active),
            "frame {frame}: expected active gate animation {raw}; states={:?}",
            snapshot.instances[&1]
                .states
                .iter()
                .map(|(&raw, state)| (raw, state.active))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            asset_guid(&node.fields["m_11E0A658"]["m_animAlias"]),
            Some(0x0c80_0000_0000_0000 | alias),
        );
        let projection = scene.gate_projection().unwrap();
        let observer = projection
            .encoder
            .project(projection.snapshot, View::Observer)
            .unwrap();
        projection.encoder.full(&observer).unwrap();
        if let Some(before) = previous {
            assert_ne!(
                before, observer,
                "damage stage did not change observer state"
            );
        }
        previous = Some(observer);
    }
}

pub(super) fn verify_normal_cassidy_damage_and_charge(heroes: &[u64], variants: &[(bool, bool)]) {
    use crate::{
        commands::{Command, FIRE, SECONDARY},
        game_combat::{PosedShot, ShotContext},
        game_combat_messages::CombatNotice,
        game_content::{JUNKENSTEIN_MAP, MapCatalog},
        game_replication::BodyStep,
        npc_catalog::{NpcCatalog, encounter_definition},
        script_enemy::EnemySpawn,
        script_init::{CreationKind, EntityCreation, PlayScript},
    };
    let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let assets = SceneAssets::load(&runtime).unwrap();
    let graphs = Arc::clone(&assets.graphs);
    let codecs = Arc::clone(assets.codecs.as_ref().unwrap());
    let card = JunkensteinCard::from_index(0x3c).unwrap();
    let map = MapCatalog::load(runtime.join("data/game_maps_174.json"))
        .unwrap()
        .game_map(
            JUNKENSTEIN_MAP,
            card.mode,
            "Junkenstein's Revenge",
            false,
            &[4, 0],
        )
        .unwrap();
    let mut scene = assets.prepare_junkenstein(map, card, 0).unwrap();
    let cassidy = scene.heroes().find_hero("Cassidy").unwrap().clone();
    let mut retirement_failures = vec![];
    let mut score_total = 0.0_f32;
    let mut score_tick = FIRST_TICK;
    let catalog = NpcCatalog::load(runtime.join("data/junkenstein/junkenstein-npcs.json")).unwrap();
    for &boss in heroes {
        let boss_body = crate::npc_catalog::is_junkenstein_boss(boss);
        let definition =
            encounter_definition(Some(&catalog), &graphs, scene.heroes(), boss).unwrap();
        for &(secondary, head) in variants {
            scene.reset_test_bodies();
            let mover = Mover::new(
                *scene.movers.for_body(Some(cassidy.body)),
                Arc::clone(scene.contacts.as_ref().unwrap()),
                [74.0, 12.6, -105.0],
                0,
                MOVER_CATEGORY,
            )
            .unwrap();
            let mut owner = scene
                .owner_body_runtime(
                    &cassidy,
                    OwnerBodySpawn {
                        slot: 0,
                        controller: 0xa000_0100,
                        entity: 0xa000_0101,
                        body_filter: team_bit(0).unwrap(),
                        frame: FIRST_TICK,
                        pose: mover.pose(),
                        eye_position: script_eye_position(cassidy.guid, &mover),
                        quanta: RETAIL_QUANTA,
                    },
                )
                .unwrap()
                .unwrap();
            let request = EntityCreation {
                position: [74.0, 12.6, -99.0],
                kind: CreationKind::Hero {
                    hero: boss,
                    team: 1,
                    yaw: 180.0,
                    flags: [Value::Bool(false), Value::Bool(false)],
                    controller: Some(graphs.enemy_pair(boss).unwrap().controller),
                    skin: None,
                },
            };
            let (room, queries) = scene.bot_simulation();
            let entity = room
                .create_enemy(EnemySpawn {
                    graphs: Arc::clone(&graphs),
                    codecs: Arc::clone(&codecs),
                    definition: &definition,
                    request: &request,
                    mode: card.mode,
                    mode_variables: Arc::new(Variables::from([
                        (6751, Value::Asset(card.difficulty)),
                        (9948, Value::Integer(0)),
                    ])),
                    tick: FIRST_TICK,
                    walk: BotWalk::default(),
                    gate: None,
                })
                .unwrap_or_else(|error| panic!("Boss {boss:X}: {error}"));
            room.take_created();
            room.set_external_targets(vec![crate::script_enemy::EnemyTarget {
                hero: Some(CASSIDY),
                filter_flags: owner.target_filter_flags(),
                tags: owner.target_tags(),
                health: owner.vitals().map(|vitals| vitals.total()),
                velocity: Some(mover.pose().velocity),
                script_state: crate::script_entity_state::EntityScriptState::Running(Arc::new(
                    owner.snapshot().unwrap().variables.clone(),
                )),
                entity: 0xa000_0101,
                team: 0,
                pose: crate::game_hitscan::TargetPose {
                    feet: mover.pose().position,
                    height: 1.8,
                    radius: 0.35,
                    eye: 1.55,
                },
                alive: true,
            }]);
            room.activate_combat();
            let caller = Site {
                entity: GAME_MODE_ENTITY,
                instance: 1,
                graph: 0x0cba,
                node: Some(9),
            };
            assert!(room.queue_encounter_wave(
                caller,
                entity + 1,
                PlayScript {
                    policy: 0,
                    overrides: vec![],
                    graph: 0x0c95,
                    source: GAME_MODE_ENTITY,
                    parameters: vec![(
                        0x0a40_0000_0000_03a1,
                        Value::Array(vec![Value::Vector(request.position)])
                    )],
                    auto_retire: true,
                }
            ));
            room.finish_tick(
                &queries,
                FIRST_TICK,
                std::time::Duration::from_millis(u64::from(FIRST_TICK) * 16),
            )
            .unwrap();
            let maximum = room
                .living_enemies(0)
                .unwrap()
                .find(|(bot, _)| bot.entity == entity)
                .unwrap()
                .1;
            let common_aim = if boss_body {
                None
            } else {
                let bot = room
                    .bodies()
                    .unwrap()
                    .find(|bot| bot.entity == entity)
                    .unwrap();
                let target = queries.target(bot);
                let delta = [
                    target.feet[0] - mover.pose().position[0],
                    target.feet[1] + target.height.min((target.eye - 0.2).max(0.1)) * 0.5
                        - mover.pose().position[1]
                        - 1.55,
                    target.feet[2] - mover.pose().position[2],
                ];
                let yaw =
                    (delta[0].atan2(delta[2]) / std::f64::consts::TAU * 65536.0).round() as i32;
                let pitch = ((-delta[1]).atan2(delta[0].hypot(delta[2])) / std::f64::consts::TAU
                    * 65536.0)
                    .round() as i32;
                Some((yaw, pitch))
            };
            let mut shots = 0;
            let mut total_dealt = 0.0;
            let mut killed = false;
            let mut death_frame = None;
            let mut last_frame = FIRST_TICK;
            for step in 1..=4000 {
                let fan = secondary && step <= 90;
                let command = Command {
                    frame: FIRST_TICK + step,
                    yaw: common_aim.map_or(0, |aim| aim.0),
                    pitch: common_aim.map_or(if head { 0 } else { 954 }, |aim| aim.1),
                    buttons: if fan {
                        SECONDARY
                    } else if step == 1 || step > 90 {
                        FIRE
                    } else {
                        0
                    },
                    ..Command::default()
                };
                last_frame = command.frame;
                owner.advance(command, &mover, BodyStep::Posed).unwrap();
                for shot in owner.take_shots(Some(command), &mover) {
                    let PosedShot::Hitscan(shot) = shot else {
                        panic!("expected Cassidy hitscan")
                    };
                    if step <= 90 {
                        shots += 1;
                    }
                    assert_eq!(shot.hit.critical, if fan { 1.0 } else { 2.0 });
                    assert_eq!(shot.hit.amount, if fan { 50.0 } else { 70.0 });
                    let context = ShotContext {
                        team: 0,
                        quanta: RETAIL_QUANTA,
                        tick: command.frame,
                        now: std::time::Duration::from_millis(u64::from(command.frame) * 16),
                    };
                    assert!(
                        room.hitscan(&queries, *shot, context, |_| false)
                            .unwrap()
                            .is_none(),
                        "unacknowledged target received damage"
                    );
                    let hit = room.hitscan(&queries, *shot, context, |_| true).unwrap();
                    if killed {
                        assert!(hit.is_none(), "dead Boss received another hit");
                        continue;
                    }
                    let hit = hit.expect("visible Boss on the street must be hit");
                    assert_eq!(hit.health.entity, entity);
                    let before_credit =
                        crate::script_expr::to_float(&owner.snapshot().unwrap().variables[&342]);
                    let mut stale_source = hit.damage.clone();
                    stale_source.source ^= 1;
                    owner.receive_applied_damage(stale_source).unwrap();
                    assert_eq!(
                        crate::script_expr::to_float(&owner.snapshot().unwrap().variables[&342]),
                        before_credit,
                        "a foreign source cannot charge this owner"
                    );
                    owner.receive_applied_damage(hit.damage.clone()).unwrap();
                    let credited = owner.snapshot().unwrap();
                    assert!(
                        credited.initialization_complete(),
                        "{:?}",
                        credited.diagnostics
                    );
                    let after_credit = crate::script_expr::to_float(&credited.variables[&342]);
                    let cost = crate::script_expr::to_float(&credited.variables[&338]);
                    // Independent normal-mode expectation from 0B8B's
                    // ordered 59/97 target-tag branches, before 23's cap.
                    let factor = if hit
                        .damage
                        .target_tags
                        .presence(0x0d60_0000_0000_03c3)
                        .unwrap()
                    {
                        0.5_f32
                    } else if hit
                        .damage
                        .target_tags
                        .presence(0x0d60_0000_0000_011e)
                        .unwrap()
                    {
                        0.75_f32
                    } else {
                        1.0_f32
                    };
                    assert_eq!(
                        after_credit,
                        f64::from(
                            (before_credit as f32 + hit.damage.amount as f32 * factor)
                                .min(cost as f32)
                        ),
                        "Boss {boss:X}: net pool loss, target reduction and charge cap"
                    );
                    let Some(CombatNotice::Hit {
                        target,
                        dealt,
                        critical,
                        killed: terminal,
                        shot_frame,
                    }) = hit.notices[0]
                    else {
                        panic!("missing hit feedback")
                    };
                    assert_eq!(
                        (target, critical, shot_frame),
                        (entity, head && !fan, shot.shot_frame)
                    );
                    let expected = (if fan {
                        50.0_f64
                    } else if head {
                        140.0_f64
                    } else {
                        70.0_f64
                    })
                    .min(maximum - total_dealt);
                    assert_eq!(dealt, expected);
                    total_dealt += dealt;
                    killed = terminal;
                    if terminal {
                        death_frame = Some(command.frame);
                    }
                    assert_eq!(terminal, total_dealt == maximum);
                    assert_eq!(
                        hit.notices[1],
                        terminal.then_some(CombatNotice::Elimination {
                            killer: shot.source,
                            victim: entity,
                        })
                    );
                    let current = room
                        .debug_actors()
                        .unwrap()
                        .find(|actor| actor.entity == entity)
                        .unwrap()
                        .health
                        .unwrap();
                    assert!((current - (maximum - total_dealt)).abs() < 0.001);
                    let Some(crate::world::Operation::Update(components)) = hit.health.operation
                    else {
                        panic!("missing health component update")
                    };
                    let pools = components
                        .iter()
                        .find(|component| component.index == 51)
                        .unwrap();
                    let wire_health: f64 = pools
                        .fields
                        .iter()
                        .flatten()
                        .map(|value| {
                            let crate::world::ComponentValue::Blob20(bytes) = value else {
                                panic!("health pool blob")
                            };
                            f64::from(f32::from_le_bytes(bytes[8..12].try_into().unwrap()))
                        })
                        .sum();
                    assert!(
                        (wire_health - current).abs() < 0.001,
                        "Boss health wire differs from room pools"
                    );
                }
                let snapshot = owner.snapshot().unwrap();
                assert!(
                    snapshot.initialization_complete(),
                    "{:?}",
                    snapshot.diagnostics
                );
                if step == 90 {
                    assert_eq!(shots, if secondary { 6 } else { 1 });
                    let weapon = snapshot
                        .instances
                        .values()
                        .find(|instance| instance.graph == 0x01d2)
                        .unwrap();
                    assert_eq!(
                        weapon.variables[&53],
                        Value::Integer(if secondary { 0 } else { 5 })
                    );
                }
                if step >= 90 && killed {
                    break;
                }
            }
            assert!(killed, "Boss {boss:X} survived ordinary Cassidy fire");
            assert!(!room.encounter_body_alive(entity));
            let mut voices = room.take_encounter_voice_requests();
            let mut retired = vec![];
            let death_frame = death_frame.unwrap();
            let mut score_carriers = Vec::new();
            for frame in last_frame + 1..=last_frame + 625 {
                let replacements = room
                    .finish_tick(
                        &queries,
                        frame,
                        std::time::Duration::from_millis(u64::from(frame) * 16),
                    )
                    .unwrap();
                assert!(
                    replacements.is_empty(),
                    "encounter Boss respawned as a training bot"
                );
                score_carriers.extend(room.take_scene_messages());
                voices.extend(room.take_encounter_voice_requests());
                retired.extend(room.take_retired_enemies());
                if matches!(boss, 0x02e0_0000_0000_0175 | 0x02e0_0000_0000_0177) {
                    assert_eq!(
                        retired == vec![entity],
                        frame - death_frame >= 500,
                        "common corpse must retire at exactly the eight-second boundary"
                    );
                }
            }
            use crate::junkenstein_voice::{self, BossSpeech};
            let defeat = voices
                .iter()
                .filter(|request| {
                    matches!(
                        junkenstein_voice::boss_speech(boss, None, request.stimulus),
                        Some(BossSpeech::Defeat(_))
                    )
                })
                .collect::<Vec<_>>();
            if boss_body {
                assert_eq!(
                    defeat.len(),
                    1,
                    "Boss {boss:X} must submit exactly one defeat voice: {voices:?}"
                );
                let notice = defeat[0];
                assert_eq!((notice.anchor, notice.speaker), (entity, GAME_MODE_ENTITY));
                let Some(BossSpeech::Defeat(rows)) =
                    junkenstein_voice::boss_speech(boss, None, notice.stimulus)
                else {
                    unreachable!()
                };
                assert!(rows.iter().any(|row| row.line == notice.line
                    && (row.killer.is_none() || row.killer == Some(CASSIDY))));
                assert!(
                    junkenstein_voice::boss_duration(notice.line as u32, notice.stimulus, true)
                        .is_some_and(|duration| duration > 0.0)
                );
            } else {
                assert!(
                    defeat.is_empty(),
                    "common NPC cannot submit a Boss defeat cue"
                );
            }
            if retired != vec![entity] || room.bodies().unwrap().any(|body| body.entity == entity) {
                retirement_failures.push((boss, secondary, head, retired));
            }
            // Independent 0CBC target-tag branches: 00F6 -> 200;
            // 00FA/00FF/011E/0120 -> 400; 031A -> 40; 0318/default -> 10.
            let expected_points = match boss as u16 {
                0x2 => 200.0,
                0x4 | 0x16 | 0x40 | 0x65 => 400.0,
                0x175 | 0x177 => 10.0,
                0x176 => 40.0,
                _ => panic!("unestablished score fixture hero {boss:X}"),
            };
            let points: Vec<_> = score_carriers
                .iter()
                .filter(|message| message.message == 0x0240_0000_0000_06a4)
                .map(|message| {
                    assert_eq!(message.target, GAME_MODE_ENTITY);
                    crate::script_expr::to_float(&message.parameters[&0x39e])
                })
                .collect();
            assert_eq!(
                points,
                [expected_points],
                "hero {boss:X}: one authored base score per death"
            );
            for message in &score_carriers {
                let parameter = match message.message {
                    0x0240_0000_0000_06a4 => Some(0x39e),
                    0x0240_0000_0000_06c8 => Some(0x3af),
                    _ => None,
                };
                if let Some(parameter) = parameter {
                    score_total +=
                        crate::script_expr::to_float(&message.parameters[&parameter]) as f32;
                }
            }
            // Return real body messages to the normal scene delivery path;
            // no score values or substitute messages are injected.
            for message in score_carriers {
                room.queue_scene_message(message);
            }
            score_tick += 1;
            scene.step_encounter(score_tick, Some(CASSIDY));
            let snapshot = scene.encounter_snapshot().unwrap();
            let expected_score = f64::from(score_total * 1.2_f32);
            assert_eq!(
                snapshot.resolved_bindings.get(&(0, 2499)),
                Some(&Value::Float(expected_score)),
                "hero {boss:X}: actual mode accumulation after Cassidy defeats the NPC"
            );
            let (projected, payload) = scene.encounter_notification_delta(None).unwrap().unwrap();
            assert_eq!(
                projected.entity_variables().get(&2499),
                Some(&Value::Float(expected_score))
            );
            assert!(payload.bit_len() > 0);
        }
    }
    assert!(
        retirement_failures.is_empty(),
        "Boss death retirement failures (hero, fan, head, retired): {retirement_failures:?}"
    );
}

#[test]
#[ignore = "requires separately prepared 104319 Junkenstein collision and graph data"]
fn normal_cassidy_flashbang_input_produces_authored_gravity_and_range_blast() {
    use crate::{
        game_combat::PosedShot,
        game_content::{JUNKENSTEIN_MAP, MapCatalog},
        game_replication::BodyStep,
        projectile_flight::{ProjectileFlight, lifted_direction},
    };
    let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let assets = SceneAssets::load(&runtime).unwrap();
    let card = JunkensteinCard::from_index(0x3c).unwrap();
    let map = MapCatalog::load(runtime.join("data/game_maps_174.json"))
        .unwrap()
        .game_map(
            JUNKENSTEIN_MAP,
            card.mode,
            "Junkenstein's Revenge",
            false,
            &[4, 0],
        )
        .unwrap();
    let scene = assets.prepare_junkenstein(map, card, 0).unwrap();
    let hero = scene.heroes().find_hero("Cassidy").unwrap();
    let mover = scene.player_spawn_mover(hero.body, 0).unwrap();
    let mut owner = scene
        .owner_body_runtime(
            hero,
            OwnerBodySpawn {
                slot: 0,
                controller: 0xa000_0100,
                entity: 0xa000_0101,
                body_filter: team_bit(0).unwrap(),
                frame: FIRST_TICK,
                pose: mover.pose(),
                eye_position: script_eye_position(hero.guid, &mover),
                quanta: RETAIL_QUANTA,
            },
        )
        .unwrap()
        .unwrap();
    let mut launches = Vec::new();
    for step in 1..=800 {
        owner
            .advance(
                crate::commands::Command {
                    frame: FIRST_TICK + step,
                    buttons: if matches!(step, 1 | 50 | 700) { 16 } else { 0 },
                    ..Default::default()
                },
                &mover,
                BodyStep::Posed,
            )
            .unwrap();
        for shot in owner.take_shots(
            Some(crate::commands::Command {
                frame: FIRST_TICK + step,
                ..Default::default()
            }),
            &mover,
        ) {
            let PosedShot::Projectile(projectile) = shot else {
                panic!("Flashbang must be a projectile")
            };
            launches.push(*projectile);
        }
    }
    assert_eq!(
        launches.len(),
        2,
        "early cooldown input must be rejected; later input must recover"
    );
    assert!(launches[0].shot.frame < FIRST_TICK + 50);
    assert!(
        launches[1].shot.frame >= FIRST_TICK + 700,
        "launch frames {:?}, first tick {FIRST_TICK}",
        launches
            .iter()
            .map(|launch| launch.shot.frame)
            .collect::<Vec<_>>()
    );
    let launch = launches.remove(0);
    assert_eq!((launch.shot.graph, launch.shot.ordinal), (0x01e9, 0));
    assert_eq!(launch.stun, crate::game_combat::Stun::from_seconds(0.8));
    assert_eq!(launch.stun.unwrap().ticks, 50);
    assert_eq!(launch.gravity, Some(20.0));
    assert!(launch.expiry_blast);
    assert_eq!(launch.ballistics.speed, 30.0);
    assert!((launch.ballistics.lifetime - 7.0 / 30.0).abs() < 1e-8);
    assert_eq!(launch.ballistics.definition, Some(0x0400_0000_0000_037d));
    let blast = launch.ballistics.blast.as_ref().unwrap();
    assert_eq!(blast.hit.amount, 25.0);
    assert_eq!(blast.rings, [(3.0, 1.0)]);
    assert_eq!(
        launch.direction,
        lifted_direction([0.0, 0.0, 1.0], 0.05).unwrap()
    );
    let mut flight =
        ProjectileFlight::new(launch.ballistics, launch.origin, launch.direction, Some(0))
            .unwrap()
            .with_gravity(launch.gravity.unwrap())
            .with_expiry_blast(launch.expiry_blast);
    let target = crate::game_hitscan::TargetPose {
        feet: [
            launch.origin[0] + 2.0,
            launch.origin[1] - 1.55,
            launch.origin[2] + 7.0,
        ],
        height: 1.8,
        radius: 0.35,
        eye: 1.55,
    };
    let end = (1..=30)
        .find_map(|tick| {
            flight
                .advance(tick, &[(42, target)], |_, _, _| Ok(None))
                .unwrap()
        })
        .expect("Flashbang range expiry");
    assert!(end.expired);
    assert_eq!(
        end.impacts
            .iter()
            .map(|hit| (hit.entity, hit.damage))
            .collect::<Vec<_>>(),
        [(42, 25.0)]
    );
    let snapshot = owner.snapshot().unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    let encoder = owner.encoder();
    let projected = encoder
        .project(snapshot, crate::script_snapshot_wire::View::Owner)
        .unwrap();
    encoder.full(&projected).unwrap();
}

#[test]
#[ignore = "requires separately prepared 104319 Junkenstein collision and graph data"]
fn normal_cassidy_roll_moves_without_stopping_the_owner_vm() {
    use crate::{
        commands::Command,
        game_content::{JUNKENSTEIN_MAP, MapCatalog},
        game_replication::BodyStep,
        script_snapshot_wire::View,
    };
    let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let assets = SceneAssets::load(&runtime).unwrap();
    let card = JunkensteinCard::from_index(0x3c).unwrap();
    let map = MapCatalog::load(runtime.join("data/game_maps_174.json"))
        .unwrap()
        .game_map(
            JUNKENSTEIN_MAP,
            card.mode,
            "Junkenstein's Revenge",
            false,
            &[4, 0],
        )
        .unwrap();
    let scene = assets.prepare_junkenstein(map, card, 0).unwrap();
    use crate::{
        collision_cache::{Mesh, Triangle},
        collision_index::SceneTriangle,
        contact_mesh::ContactMesh,
        contact_world::{StaticGeometry, StaticShape},
    };
    let vertices = [
        [-50.0, 0.0, -50.0],
        [-50.0, 0.0, 50.0],
        [50.0, 0.0, 50.0],
        [50.0, 0.0, -50.0],
    ];
    let corners = [[0, 1, 2], [0, 2, 3]];
    let floor = TriangleIndex::new(corners.map(|indices| SceneTriangle {
        vertices: indices.map(|i| vertices[i]),
        mask: 0x18,
        surface: 0,
    }))
    .unwrap();
    let mesh = Mesh {
        vertices: vertices.map(|v| v.map(|x| x as f32)).to_vec(),
        triangles: corners
            .map(|indices| Triangle {
                corners: indices.map(|i| i as i32),
                wings: [-1; 3],
                mask: 0x18,
                surface: 0,
            })
            .to_vec(),
    };
    let contacts = Arc::new(
        ContactWorld::new(vec![
            StaticShape::new(
                StaticGeometry::Mesh(Arc::new(ContactMesh::new(mesh, [1.0; 3], 0x18).unwrap())),
                None,
            )
            .unwrap(),
        ])
        .unwrap(),
    );
    let cassidy = scene.heroes().find_hero("Cassidy").unwrap();
    for (case, yaw, forward, right, expected) in [
        (0, 0, 0, 0, [0.0, 15.0]),
        (1, 0, 0, 0, [0.0, 15.0]),
        (2, 0, 0, 127, [-15.0, 0.0]),
        (3, 0, -127, 0, [0.0, -15.0]),
        (4, 16384, 127, 0, [15.0, 0.0]),
        (
            5,
            0,
            127,
            127,
            [-15.0 / 2.0_f64.sqrt(), 15.0 / 2.0_f64.sqrt()],
        ),
    ] {
        let mut mover = if case == 0 {
            scene.player_spawn_mover(cassidy.body, 0).unwrap()
        } else {
            Mover::new(
                *scene.movers.for_body(Some(cassidy.body)),
                Arc::clone(&contacts),
                [0.0; 3],
                yaw,
                0x18,
            )
            .unwrap()
        };
        let mut owner = scene
            .owner_body_runtime(
                cassidy,
                OwnerBodySpawn {
                    slot: 0,
                    controller: 0xa000_0100,
                    entity: 0xa000_0101,
                    body_filter: team_bit(0).unwrap(),
                    frame: FIRST_TICK,
                    pose: mover.pose(),
                    eye_position: script_eye_position(cassidy.guid, &mover),
                    quanta: RETAIL_QUANTA,
                },
            )
            .unwrap()
            .unwrap();

        let start = mover.pose().position;
        let rays = if case == 0 {
            scene.movement_index()
        } else {
            &floor
        };
        let mut fastest: f64 = 0.0;
        for step in 1..=130 {
            let command = Command {
                frame: FIRST_TICK + step,
                yaw,
                buttons: if step == 5 || step == 125 {
                    1
                } else if step == 50 {
                    8
                } else {
                    0
                },
                forward: if step >= 50 { forward } else { 0 },
                right: if step >= 50 { right } else { 0 },
                ..Default::default()
            };
            mover
                .step(rays, command, 0.016, |mover, command| {
                    owner.advance(command, mover, BodyStep::Posed).unwrap();
                    let snapshot = owner.snapshot().unwrap();
                    assert!(
                        snapshot.initialization_complete(),
                        "case {case} step {step}: {:?}",
                        snapshot.diagnostics
                    );
                    owner.encoder().project(snapshot, View::Owner).unwrap();
                    for (frame, change) in owner.take_velocity_changes() {
                        mover.queue_velocity_change(frame, change);
                    }
                    mover.track_modifiers(command.frame, owner.movement_modifiers().unwrap());
                })
                .unwrap();
            let weapon = owner
                .snapshot()
                .unwrap()
                .instances
                .values()
                .find(|i| i.graph == 0x01d2)
                .unwrap();
            if step == 45 {
                assert_eq!(weapon.variables[&53], Value::Integer(5));
            }
            if step == 90 {
                assert_eq!(weapon.variables[&53], Value::Integer(6));
            }
            if step == 130 {
                assert_eq!(weapon.variables[&53], Value::Integer(5));
            }
            let [x, _, z] = mover.pose().velocity;
            fastest = fastest.max(x.hypot(z));
            if case != 0 && step == 51 {
                assert!(
                    (x - expected[0]).abs() < 0.02 && (z - expected[1]).abs() < 0.02,
                    "case {case}: {x}, {z}"
                );
            }
            if case == 1 && step == 50 {
                assert_eq!(mover.pose().position, start);
            }
            if case == 1 && step == 90 {
                let distance = mover.pose().position[2] - start[2];
                assert!((5.8..6.5).contains(&distance), "Roll distance {distance}");
                assert!(x.hypot(z) <= 5.5);
            }
        }
        assert!(fastest > 10.0, "Roll never accelerated: {fastest}");
        if case == 0 {
            let end = mover.pose().position;
            let distance = (end[0] - start[0]).hypot(end[2] - start[2]);
            assert!(
                (1.0..2.0).contains(&distance),
                "Spawn collision displacement: {distance}"
            );
        }
    }
}

#[test]
#[ignore = "requires the separately prepared 104319 Junkenstein collision and graph data"]
fn normal_cassidy_charges_locks_and_fires_the_authored_deadeye() {
    use crate::{
        commands::Command,
        game_combat::{PosedShot, ShotContext},
        game_content::{JUNKENSTEIN_MAP, MapCatalog},
        game_hitscan::aim_direction,
        game_replication::BodyStep,
        script_snapshot_wire::View,
        script_stream::{BodyStream, ChangeKey, DeltaKind},
    };

    let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let assets = SceneAssets::load(&runtime).unwrap();
    let card = JunkensteinCard::from_index(0x3c).unwrap();
    let map = MapCatalog::load(runtime.join("data/game_maps_174.json"))
        .unwrap()
        .game_map(
            JUNKENSTEIN_MAP,
            card.mode,
            "Junkenstein's Revenge",
            false,
            &[4, 0],
        )
        .unwrap();
    let mut scene = assets.prepare_junkenstein(map, card, 0).unwrap();
    let cassidy = scene.heroes().find_hero("Cassidy").unwrap();
    let mover = scene.player_spawn_mover(cassidy.body, 0).unwrap();
    let mut owner = scene
        .owner_body_runtime(
            cassidy,
            OwnerBodySpawn {
                slot: 0,
                controller: 0xa000_0100,
                entity: 0xa000_0101,
                body_filter: team_bit(0).unwrap(),
                frame: FIRST_TICK,
                pose: mover.pose(),
                eye_position: script_eye_position(cassidy.guid, &mover),
                quanta: RETAIL_QUANTA,
            },
        )
        .unwrap()
        .unwrap();
    let initial = owner.snapshot().unwrap();
    let (&defender_id, defender) = initial
        .instances
        .iter()
        .find(|(_, instance)| instance.graph == 0x0ca5)
        .unwrap();
    assert!(
        defender.states[&59].active,
        "full defender startup must mount the body-field stack"
    );
    assert_eq!(
        initial.resolved_bindings.get(&(defender_id, 17630)),
        Some(&Value::Entity(0))
    );
    let charge = |owner: &dyn crate::game_replication::OwnerBodyRuntime| {
        crate::script_expr::to_float(
            owner
                .snapshot()
                .unwrap()
                .variables
                .get(&342)
                .unwrap_or(&Value::Null),
        )
    };
    let cost = crate::script_expr::to_float(&owner.snapshot().unwrap().variables[&338]);
    assert!(cost > 0.0);
    assert_eq!(charge(&*owner), 0.0);
    let fill_steps = (cost / 5.0).ceil() as u32 * 1000 / 16 + 1;
    for step in 1..=fill_steps {
        owner
            .advance(
                Command {
                    frame: FIRST_TICK + step,
                    ..Default::default()
                },
                &mover,
                BodyStep::Posed,
            )
            .unwrap();
        if step == 1 {
            assert_eq!(charge(&*owner), 0.0);
        }
        if step == 62 {
            assert_eq!(charge(&*owner), 0.0);
        }
        if step == 63 {
            assert_eq!(charge(&*owner), 5.0);
        }
    }
    assert_eq!(charge(&*owner), cost);
    let snapshot = owner.snapshot().unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    let encoder = owner.encoder();
    let idle = encoder.project(snapshot, View::Owner).unwrap();
    let mut stream = BodyStream::new(snapshot.entity);
    stream.create_arrived();
    let baseline = encoder.queue_full(&mut stream, &idle).unwrap();
    stream.arrived(baseline.last);
    let initial = encoder
        .queue_delta(
            &mut stream,
            &idle,
            DeltaKind::InitialOwner(FIRST_TICK + fill_steps),
            16,
        )
        .unwrap()
        .unwrap();
    stream.sent(initial.last);
    stream.arrived(initial.last);
    let pose = mover.pose();
    let forward = aim_direction(&Command {
        yaw: pose.yaw,
        pitch: pose.pitch,
        ..Command::default()
    });
    let position = |distance: f64| -> [f64; 3] {
        std::array::from_fn(|axis| pose.position[axis] + distance * forward[axis])
    };
    let [front_x, front_y, front_z] = position(2.0);
    let off_axis = [
        front_x + 0.8 * forward[2],
        front_y,
        front_z - 0.8 * forward[0],
    ];
    let [left_x, left_y, left_z] = position(2.2);
    let left_of_aim = [left_x - 0.9 * forward[2], left_y, left_z + 0.9 * forward[0]];
    let [x, y, z] = pose.position;
    let eye = [
        x,
        y + crate::game_combat::eye_height(cassidy.guid, false),
        z,
    ];
    let wall = scene
        .ground
        .cast_ray(eye, forward, 190.0, MOVER_CATEGORY)
        .unwrap()
        .expect("the Normal Cassidy spawn faces castle geometry");
    let bot_catalog = BotCatalog::from_json(
        &serde_json::json!({
            "heroes": {"0x01": {"name": "Fixture body", "body": "0x02", "health": 1000}},
            "maps": {"0x03": {"hints": {}, "spawners": [
                {"hero": "0x01", "team": 1, "position": off_axis,
                 "rotation": [0, 0, 0, 1], "graphs": []},
                {"hero": "0x01", "team": 0, "position": position(3.0),
                 "rotation": [0, 0, 0, 1], "graphs": []},
                {"hero": "0x01", "team": 1, "position": position(-3.0),
                 "rotation": [0, 0, 0, 1], "graphs": []},
                {"hero": "0x01", "team": 1, "position": position(220.0),
                 "rotation": [0, 0, 0, 1], "graphs": []},
                {"hero": "0x01", "team": 1, "position": position(wall.distance + 4.0),
                 "rotation": [0, 0, 0, 1], "graphs": []},
                {"hero": "0x01", "team": 1, "position": left_of_aim,
                 "rotation": [0, 0, 0, 1], "graphs": []}
            ]}}
        })
        .to_string(),
    )
    .unwrap();
    let bots = bot_catalog.make(3, |_| BotWalk::default());
    let target = bots[0].entity;
    let occluded = bots[4].entity;
    let second = bots[5].entity;
    scene.simulation = RoomSimulation::new(bots).unwrap();
    owner.set_deadeye_scene(&scene, 0).unwrap();
    owner
        .advance(
            Command {
                frame: FIRST_TICK + (fill_steps + 1),
                buttons: 0x20,
                ..Command::default()
            },
            &mover,
            BodyStep::Posed,
        )
        .unwrap();
    assert_eq!(charge(&*owner), 0.0);
    for step in (fill_steps + 2)..=(fill_steps + 41) {
        owner
            .advance(
                Command {
                    frame: FIRST_TICK + step,
                    ..Command::default()
                },
                &mover,
                BodyStep::Posed,
            )
            .unwrap();
    }
    let snapshot = owner.snapshot().unwrap();
    assert_eq!(
        snapshot.variables.get(&2551),
        Some(&Value::Array(vec![
            Value::Entity(target),
            Value::Entity(second),
            Value::Entity(occluded)
        ])),
        "{:?}",
        snapshot.diagnostics
    );
    assert!(!snapshot.diagnostics.iter().any(|diagnostic| {
        diagnostic.site.graph == 0x022f && matches!(diagnostic.site.node, Some(26 | 86))
    }));
    assert_eq!(
        snapshot.variables.get(&5874),
        Some(&Value::Array(vec![
            Value::Entity(target),
            Value::Entity(second)
        ]))
    );
    let effects = snapshot
        .instances
        .iter()
        .filter(|(_, instance)| instance.graph == 0x01ee)
        .map(|(&id, instance)| (id, instance.variables[&536].clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        effects
            .iter()
            .map(|(_, target)| target.clone())
            .collect::<Vec<_>>(),
        [
            Value::Entity(target),
            Value::Entity(second),
            Value::Entity(occluded)
        ]
    );
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    let encoder = owner.encoder();
    let lock_snapshot = encoder.project_next(snapshot, &idle).unwrap();
    encoder.full(&lock_snapshot).unwrap();
    let lock = encoder
        .queue_delta(
            &mut stream,
            &lock_snapshot,
            DeltaKind::Correction(FIRST_TICK + (fill_steps + 41)),
            16,
        )
        .unwrap()
        .unwrap();
    let sent = stream.sent(lock.last).unwrap();
    for &(instance, _) in &effects {
        assert!(sent.changes.contains(&ChangeKey::Instance(instance)));
    }
    stream.arrived(lock.last);
    let roster = std::mem::replace(
        &mut scene.simulation,
        RoomSimulation::new(Vec::new()).unwrap(),
    );
    owner.set_deadeye_scene(&scene, 0).unwrap();
    owner
        .advance(
            Command {
                frame: FIRST_TICK + (fill_steps + 42),
                ..Command::default()
            },
            &mover,
            BodyStep::Posed,
        )
        .unwrap();
    for variable in [2551, 5874] {
        assert_eq!(
            owner.snapshot().unwrap().variables.get(&variable),
            Some(&Value::Array(Vec::new()))
        );
    }
    assert!(
        !owner
            .snapshot()
            .unwrap()
            .instances
            .values()
            .any(|instance| instance.graph == 0x01ee)
    );
    scene.simulation = roster;
    let mut hit_entities = Vec::new();
    let mut last_damage = None;
    for step in (fill_steps + 43)..=(fill_steps + 400) {
        owner.set_deadeye_scene(&scene, 0).unwrap();
        let command = Command {
            frame: FIRST_TICK + step,
            buttons: if step == (fill_steps + 343) { 0x20 } else { 0 },
            yaw: pose.yaw,
            pitch: pose.pitch,
            ..Command::default()
        };
        owner.advance(command, &mover, BodyStep::Posed).unwrap();
        let snapshot = owner.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{step}: {:?}",
            snapshot.diagnostics
        );
        owner
            .encoder()
            .project_next(snapshot, &idle)
            .unwrap_or_else(|error| panic!("{step}: {error}"));
        for shot in owner.take_shots(Some(command), &mover) {
            let PosedShot::Hitscan(shot) = shot else {
                panic!("Deadeye must fire the authored hitscan");
            };
            let (simulation, queries) = scene.bot_simulation();
            let context = ShotContext {
                team: 0,
                quanta: RETAIL_QUANTA,
                tick: command.frame,
                now: std::time::Duration::ZERO,
            };
            if let Some(impact) = simulation
                .hitscan(&queries, *shot, context, |_| true)
                .unwrap()
            {
                last_damage = Some(impact.damage.clone());
                owner.receive_applied_damage(impact.damage).unwrap();
                assert_eq!(charge(&*owner), 0.0, "Deadeye must not recharge itself");
                hit_entities.push(impact.health.entity);
            }
        }
    }
    hit_entities.sort_unstable();
    assert_eq!(hit_entities, [target, second]);
    assert!(!scene.simulation.encounter_body_alive(target));
    assert!(!scene.simulation.encounter_body_alive(second));
    assert!(scene.simulation.encounter_body_alive(occluded));
    let snapshot = owner.snapshot().unwrap();
    assert!(
        !snapshot
            .instances
            .values()
            .any(|instance| instance.graph == 0x01ee)
    );
    let encoder = owner.encoder();
    let release_snapshot = encoder.project_next(snapshot, &idle).unwrap();
    encoder.full(&release_snapshot).unwrap();
    let release = encoder
        .queue_delta(
            &mut stream,
            &release_snapshot,
            DeltaKind::Correction(FIRST_TICK + (fill_steps + 400)),
            16,
        )
        .unwrap()
        .unwrap();
    let sent = stream.sent(release.last).unwrap();
    for (instance, _) in effects {
        assert!(sent.changes.contains(&ChangeKey::Instance(instance)));
    }
    let charge_at_end = charge(&*owner);
    owner.set_encounter_active(false).unwrap();
    owner.receive_applied_damage(last_damage.unwrap()).unwrap();
    for step in fill_steps + 401..=fill_steps + 501 {
        owner
            .advance(
                Command {
                    frame: FIRST_TICK + step,
                    ..Command::default()
                },
                &mover,
                BodyStep::Posed,
            )
            .unwrap();
    }
    assert_eq!(
        charge(&*owner),
        charge_at_end,
        "round end must cancel passive and damage charging"
    );
    assert_eq!(
        owner.snapshot().unwrap().variables.get(&2426),
        Some(&Value::Bool(false))
    );
}

#[test]
#[ignore = "requires the separately prepared 104319 Junkenstein collision and graph data"]
fn junkenstein_notification_updates_follow_authored_messages() {
    use crate::game_content::{JUNKENSTEIN_MAP, MapCatalog};

    let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let assets = SceneAssets::load(&runtime).unwrap();
    let maps = MapCatalog::load(runtime.join("data/game_maps_174.json")).unwrap();
    let card = JunkensteinCard::from_index(0x3c).unwrap();
    let map = maps
        .game_map(
            JUNKENSTEIN_MAP,
            card.mode,
            "Junkenstein's Revenge",
            false,
            &[4, 0],
        )
        .unwrap();
    let mut scene = assets.prepare_junkenstein(map, card, 0).unwrap();
    let (initial, payload) = scene.encounter_notification_delta(None).unwrap().unwrap();
    assert_eq!(initial.instance_count(), 1);
    assert!(payload.bit_len() > 0);
    assert!(
        scene
            .encounter_notification_delta(Some(&initial))
            .unwrap()
            .is_none()
    );
    scene.step_encounter(2700, None);
    assert!(
        scene
            .queue_test_encounter_entity_message(0x0240_0000_0000_0694, Variables::new())
            .unwrap()
            > 0
    );
    scene.step_encounter(2701, None);
    let (changed, payload) = scene
        .encounter_notification_delta(Some(&initial))
        .unwrap()
        .unwrap();
    assert_eq!(changed.instance_count(), 1);
    assert!(payload.bit_len() > 0);
    assert!(
        scene
            .encounter_notification_delta(Some(&changed))
            .unwrap()
            .is_none()
    );
    assert!(
        scene
            .queue_test_encounter_entity_message(0x0240_0000_0000_06a8, Variables::new())
            .unwrap()
            > 0
    );
    let mut baseline = changed;
    let mut updates = 0;
    for tick in 2702..=2822 {
        scene.step_encounter(tick, None);
        assert!(
            scene.encounter_snapshot().is_some(),
            "encounter halted at {tick}"
        );
        if let Some((current, payload)) =
            scene.encounter_notification_delta(Some(&baseline)).unwrap()
        {
            assert_eq!(current.instance_count(), 1);
            assert!(payload.bit_len() > 0);
            baseline = current;
            updates += 1;
        }
    }
    assert!(updates > 0, "06A8 must change the notification graph");
    assert!(
        updates < 121,
        "notifications must leave ticks for the countdown"
    );
}

#[test]
#[ignore = "requires the separately prepared 104319 Junkenstein collision and graph data"]
fn junkenstein_room_clock_starts_countdown_once_after_cassidy_selection() {
    use crate::{
        game_content::{JUNKENSTEIN_MAP, MapCatalog},
        script_init::StateKind,
    };
    use sha2::Digest;

    let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let assets = SceneAssets::load(&runtime).unwrap();
    let maps = MapCatalog::load(runtime.join("data/game_maps_174.json")).unwrap();
    let card = JunkensteinCard::from_index(0x3c).unwrap();
    let map = maps
        .game_map(
            JUNKENSTEIN_MAP,
            card.mode,
            "Junkenstein's Revenge",
            false,
            &[4, 0],
        )
        .unwrap();
    let mut scene = assets.prepare_junkenstein(map, card, 0).unwrap();
    // Python remote_delta vectors for the same 104319 graph and codec table.
    for (value, active, width, expected) in [
        (
            8.0,
            [true, false, false, false],
            121,
            "10c02e0a0a01000c020e010000100400",
        ),
        (
            5.0,
            [false, true, false, false],
            121,
            "10c02e0aaa00000c020e020000140800",
        ),
        (
            0.0,
            [false, false, false, false],
            85,
            "10c02e0a0a00000c020e00",
        ),
        (
            470.0,
            [false, false, true, false],
            149,
            "10c02e0a1000607d0800c020e0080080f52100",
        ),
        (
            12.0,
            [false, false, false, true],
            121,
            "10c02e0a8a01000c020e040000501000",
        ),
    ] {
        let wire = scene
            .encounter_countdown_wire(&CountdownFrame {
                value: Value::Float(value),
                states: active.map(|on| {
                    on.then_some(script_init::ChasePayload {
                        current: Value::Float(value),
                        reached: false,
                        remaining_ms: None,
                        last_ms: 0,
                    })
                }),
            })
            .unwrap()
            .unwrap();
        assert_eq!(wire.bit_len(), width);
        let actual: String = wire
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(actual, expected);
    }
    let cassidy = scene.heroes().find_hero("Cassidy").unwrap();
    let hero = cassidy.guid;
    let mover = scene.player_spawn_mover(cassidy.body, 0).unwrap();
    let mut owner = scene
        .owner_body_runtime(
            cassidy,
            OwnerBodySpawn {
                slot: 0,
                controller: 0xa000_0100,
                entity: 0xa000_0101,
                body_filter: team_bit(0).unwrap(),
                frame: FIRST_TICK,
                pose: mover.pose(),
                eye_position: script_eye_position(cassidy.guid, &mover),
                quanta: RETAIL_QUANTA,
            },
        )
        .unwrap()
        .expect("Normal Cassidy owner runtime");
    let snapshot = owner.snapshot().unwrap();
    let revolver = snapshot
        .instances
        .values()
        .find(|instance| instance.graph == 0x01d2)
        .expect("Cassidy revolver");
    assert_eq!(revolver.variables[&198], Value::Integer(6));
    assert_eq!(revolver.variables[&53], Value::Integer(6));
    let defender = snapshot
        .instances
        .values()
        .find(|instance| instance.graph == 0x0ca5)
        .expect("defender message receiver");
    assert_eq!(defender.states.len(), 1);
    let intro_receiver = &defender.states[&11];
    assert!(intro_receiver.active);
    assert_eq!(intro_receiver.kind, StateKind::MessageIdle);
    assert_eq!(owner.vitals().unwrap().total(), 225.0);
    for (source, message) in [
        (
            GAME_MODE_ENTITY,
            crate::junkenstein_messages::RESET_HIGHLIGHTS,
        ),
        (0xc000_1000, crate::junkenstein_messages::BOSS_INTRO_ENDED),
    ] {
        assert_eq!(
            owner
                .receive_defender_message(DefenderMessage {
                    target: None,
                    source,
                    message,
                    parameters: Variables::new()
                })
                .unwrap(),
            1
        );
    }
    assert!(owner.snapshot().unwrap().initialization_complete());
    let mover = scene.player_spawn_mover(cassidy.body, 0).unwrap();
    let pose = mover.pose();
    let yaw = f64::from(pose.yaw) * std::f64::consts::TAU / 65_536.0;
    let (sin, cos) = yaw.sin_cos();
    let [x, y, z] = pose.position;
    let sender = 0xc000_1000;
    for direction in [-1.0, 1.0] {
        owner
            .receive_sighting(crate::junkenstein_messages::DefenderSighting {
                target: 0xa000_0101,
                source: crate::junkenstein_messages::SightingSource {
                    entity: sender,
                    position: [x + direction * sin, y, z + direction * cos],
                    tags: [Some(false); 3],
                    filter_flags: Some(1 | team_bit(1).unwrap()),
                },
                parameters: Variables::from([
                    (0x8d, Value::Integer(1)),
                    (0x5c, Value::Entity(sender)),
                ]),
            })
            .unwrap();
        let receiver = owner
            .snapshot()
            .unwrap()
            .instances
            .values()
            .find(|instance| instance.graph == 0x0ca5)
            .unwrap();
        assert_eq!(receiver.variables[&40], Value::Entity(sender));
        assert_eq!(
            receiver.variables.get(&1725),
            (direction > 0.0).then_some(&Value::Integer(0))
        );
    }

    owner.set_defenders(Arc::from([0xa000_0101]));
    for frame in FIRST_TICK + 1..=FIRST_TICK + 62 {
        owner
            .advance(
                crate::commands::Command {
                    frame,
                    ..Default::default()
                },
                &mover,
                crate::game_replication::BodyStep::UnposedTimer,
            )
            .unwrap_or_else(|error| panic!("Cassidy stopped at frame {frame}: {error}"));
    }
    let snapshot = owner.snapshot().unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    scene.step_encounter(2700, None);
    let before = &scene.encounter_snapshot().unwrap().instances[&ENCOUNTER_INSTANCE];
    assert_eq!(before.states[&3].kind, StateKind::BooleanSwitch(false));
    assert!(!before.states.get(&52).is_some_and(|state| state.active));

    scene.step_encounter(2701, Some(hero));
    let ready = &scene.encounter_snapshot().unwrap().instances[&ENCOUNTER_INSTANCE];
    assert_eq!(
        ready.variables[&8449],
        Value::Array(vec![Value::Asset(hero)])
    );
    assert_eq!(ready.states[&3].kind, StateKind::BooleanSwitch(true));
    assert!(ready.states[&52].active);
    assert_eq!(ready.states[&52].start_ms, 16);
    assert_eq!(
        scene
            .encounter_countdown()
            .unwrap()
            .states
            .map(|state| state.is_some()),
        [true, false, false, false]
    );

    for tick in 2702..2730 {
        scene.step_encounter(tick, Some(hero));
    }
    let after = &scene.encounter_snapshot().unwrap().instances[&ENCOUNTER_INSTANCE];
    assert_eq!(after.states[&52].activation, 1);
    assert!(
        after.states[&52]
            .chase
            .as_ref()
            .is_some_and(|chase| chase.last_ms > 16)
    );
    let ana = scene.heroes().find_hero("Ana").unwrap().guid;
    scene.step_encounter(2730, Some(ana));
    let changed = &scene.encounter_snapshot().unwrap().instances[&ENCOUNTER_INSTANCE];
    assert_eq!(
        changed.variables[&8449],
        Value::Array(vec![Value::Asset(ana)])
    );
    assert_eq!(changed.states[&52].activation, 1);

    for tick in 2731..=3220 {
        scene.step_encounter(tick, Some(ana));
    }
    let started = &scene.encounter_snapshot().unwrap().instances[&ENCOUNTER_INSTANCE];
    assert_eq!(started.variables[&59], Value::Integer(1));
    assert_eq!(started.states[&53].activation, 1);
    for tick in 3221..=4080 {
        scene.step_encounter(tick, Some(ana));
    }
    let gate = scene.gate_snapshot().unwrap();
    assert_eq!(gate.variables[&8571], Value::Integer(1));
    assert_eq!(scene.gate_team(), Some(2));
    let snapshot = scene.encounter_snapshot().unwrap();
    let mode = &snapshot.instances[&ENCOUNTER_INSTANCE];
    assert_eq!(
        mode.variables[&8618],
        Value::Array(vec![Value::Entity(player_entity(0))])
    );
    assert_eq!(mode.variables[&59], Value::Integer(2));
    assert!(
        snapshot
            .instances
            .values()
            .any(|instance| instance.graph == 0x0cb3)
    );
    let first_spawner = snapshot
        .instances
        .values()
        .find(|instance| instance.graph == 0x0cba)
        .expect("authored first spawn graph");
    assert_eq!(
        first_spawner.variables[&8398],
        Value::Asset(0x02e0_0000_0000_0175)
    );
    assert_eq!(
        first_spawner.variables[&8364],
        Value::Asset(0x0d60_0000_0000_0328)
    );
    assert_eq!(first_spawner.variables[&6737], Value::Integer(8));
    let anchor = 0x8000_0107;
    assert_eq!(first_spawner.variables[&1629], Value::Entity(anchor));
    let searches = scene.encounter_spawn_search_keys();
    let (search_key, request) = searches
        .into_iter()
        .next()
        .expect("pending first spawn search");
    assert_eq!(
        request.position,
        [74.47403717041016, 12.70523452758789, -99.36385345458984]
    );
    assert_eq!(
        (request.radius, request.minimum, request.maximum),
        (8.0, 10, 300)
    );
    assert_eq!(
        request.direction,
        [-0.2588191628456116, 0.0, -0.9659258127212524]
    );
    assert!(!first_spawner.variables.contains_key(&8402));

    for tick in 4081..=4120 {
        scene.step_encounter(tick, Some(ana));
    }
    let snapshot = scene.encounter_snapshot().unwrap();
    let first_spawner = snapshot
        .instances
        .values()
        .find(|instance| instance.graph == 0x0cba)
        .unwrap();
    let Value::Array(points) = &first_spawner.variables[&8402] else {
        panic!("spawn search must publish candidate positions")
    };
    assert!((10..=300).contains(&points.len()));
    assert_eq!(points.len(), 172);
    let mut digest = sha2::Sha256::new();
    for point in points {
        let Value::Vector(point) = point else {
            panic!("candidate must be a vector")
        };
        for &axis in point {
            digest.update((axis as f32).to_le_bytes());
        }
    }
    assert_eq!(
        format!("{:x}", digest.finalize()),
        "698b0355c4711243657e6e8829b2ef2d4b9cd5ab39dcb68b899367eda21dbd97"
    );
    let samples = [
        (
            0,
            [74.55856718113806, 12.682595778099707, -99.04838341805228],
        ),
        (
            1,
            [74.73517358618709, 12.682595448753904, -99.86565797066675],
        ),
        (
            2,
            [73.78785046921642, 12.682595758201575, -99.11389116855813],
        ),
        (
            171,
            [66.65998277248119, 12.383752996930431, -100.77117497656228],
        ),
    ];
    for (index, expected) in samples {
        let Value::Vector(actual) = &points[index] else {
            panic!("candidate must be a vector")
        };
        for (actual, expected) in actual.iter().zip(expected) {
            assert!(
                (actual - expected).abs() < 1e-9,
                "candidate {index}: {actual} != {expected}"
            );
        }
    }
    for point in points {
        let Value::Vector([x, _, z]) = point else {
            panic!("candidate must be a vector")
        };
        assert!((x - request.position[0]).hypot(z - request.position[2]) <= request.radius);
    }
    let first_instance = snapshot
        .instances
        .iter()
        .find(|(_, instance)| instance.graph == 0x0cba)
        .map(|(&id, _)| id)
        .unwrap();
    let selection = &first_spawner.states[&2];
    assert!(selection.active);
    assert_eq!(selection.kind, script_init::StateKind::RecentInput);
    let roster_filter = &first_spawner.states[&1];
    assert!(roster_filter.active);
    assert_eq!(roster_filter.kind, StateKind::LivingBodyArray);
    let candidates = points.clone();
    let mut selected_tick = None;
    for tick in 4121..=4200 {
        scene.step_encounter(tick, Some(ana));
        if scene
            .encounter_snapshot()
            .unwrap()
            .instances
            .get(&first_instance)
            .is_some_and(|instance| instance.variables.contains_key(&8370))
        {
            selected_tick = Some(tick);
            break;
        }
    }
    let selected_tick = selected_tick.expect("spawner must select before retiring");
    let snapshot = scene.encounter_snapshot().unwrap();
    let spawner = &snapshot.instances[&first_instance];
    assert!(candidates.contains(&spawner.variables[&8370]));
    if runtime
        .join("data/junkenstein/junkenstein-npcs.json")
        .is_file()
    {
        assert!(
            !snapshot
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.site.instance == first_instance
                    && diagnostic.site.node == Some(7)
                    && matches!(diagnostic.issue, script_init::Issue::EntityCreation(_))),
            "{:?}",
            snapshot.diagnostics
        );
        let enemies = scene.simulation.bodies().unwrap();
        assert_eq!(enemies.len(), 3);
        let roster = enemies
            .map(|enemy| {
                assert_eq!(enemy.spawn.hero, 0x02e0_0000_0000_0175);
                assert_eq!(enemy.spawn.team, 1);
                assert!(candidates.contains(&Value::Vector(enemy.position)));
                Value::Entity(enemy.entity)
            })
            .collect::<Vec<_>>();
        // The living filter was evaluated before the newest creation.
        assert_eq!(spawner.variables[&8681], Value::Array(roster[..2].to_vec()));
        assert_eq!(spawner.variables[&8399], Value::Array(roster));
        assert!(
            !snapshot.diagnostics.iter().any(|diagnostic| {
                diagnostic.site.instance == first_instance && diagnostic.site.node == Some(9)
            }),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(
            scene.simulation.bodies().unwrap().last().unwrap().position,
            match &spawner.variables[&8370] {
                Value::Vector(position) => *position,
                _ => panic!("spawn position"),
            }
        );
    } else {
        assert_eq!(spawner.variables[&8681], Value::Array(vec![]));
        assert!(
            snapshot
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.site.instance == first_instance
                    && diagnostic.site.node == Some(7)
                    && matches!(&diagnostic.issue,script_init::Issue::EntityCreation(error)
                if error=="missing prepared enemy creation services"
                    || error=="missing prepared NPC definition")),
            "authored spawner must evaluate its creation request: {:?}",
            snapshot.diagnostics
        );
    }
    let queued = scene
        .queue_test_encounter_frame_message(
            selected_tick - 2700 + 1,
            first_instance,
            0x0240_0000_0000_0695,
            Variables::new(),
        )
        .unwrap();
    assert_eq!(queued, 1);
    scene.step_encounter(selected_tick + 1, Some(ana));
    assert!(
        !scene
            .encounter_snapshot()
            .unwrap()
            .instances
            .contains_key(&first_instance)
    );
    // A retired search must ignore a delayed completion without restoring its instance.
    scene
        .queue_test_spawn_result(selected_tick - 2700 + 2, search_key, vec![[0.0; 3]; 10])
        .unwrap();
    scene.step_encounter(selected_tick + 2, Some(ana));
    assert!(
        !scene
            .encounter_snapshot()
            .unwrap()
            .instances
            .contains_key(&first_instance)
    );
}
