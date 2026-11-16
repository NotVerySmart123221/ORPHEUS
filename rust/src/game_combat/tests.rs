use super::*;
use crate::{game_content::GameMap, game_instance::RETAIL_QUANTA, scene_assets::SceneAssets};

pub(super) fn fixture_bots() -> Vec<Bot> {
    crate::game_bots::BotCatalog::from_json(
        r#"{
        "heroes":{"0x01":{"name":"Test bot","body":"0x02","health":100}},
        "maps":{"0x03":{"hints":{},"spawners":[
            {"hero":"0x01","team":1,"position":[1,0,0],"rotation":[0,0,0,1],"graphs":[]},
            {"hero":"0x01","team":1,"position":[2,0,0],"rotation":[0,0,0,1],"graphs":[]}
        ]}}
    }"#,
    )
    .unwrap()
    .make(3, |_| crate::game_bots::BotWalk::default())
}

#[test]
fn corpse_health_is_available_after_live_target_selection_excludes_it() {
    let mut room = RoomSimulation::new(fixture_bots()).unwrap();
    let victim = room.living_enemies(0).unwrap().next().unwrap().0.entity;
    room.apply_bot_impact(
        7,
        FIXTURE_CONTEXT.tick,
        FIXTURE_CONTEXT,
        &fixture_impact(victim, 10000.0),
    )
    .unwrap()
    .unwrap();
    assert!(
        !room
            .living_enemies(0)
            .unwrap()
            .any(|(bot, _)| bot.entity == victim)
    );
    assert_eq!(room.body_healths().unwrap().get(&victim), Some(&0.0));
    room.respawn_due(
        FIXTURE_CONTEXT.tick + 1,
        FIXTURE_CONTEXT.now + BOT_RESPAWN_TIME,
    )
    .unwrap();
    assert!(!room.body_healths().unwrap().contains_key(&victim));
}

#[test]
#[ignore = "requires separately prepared 104319 Junkenstein runtime"]
fn normal_waves_destroy_gate_with_invulnerable_cassidy() {
    junkenstein::verify_natural_ending(junkenstein::NormalEnding::EarlyDefeat);
}

#[test]
#[ignore = "requires separately prepared 104319 Junkenstein runtime"]
fn normal_late_waves_destroy_gate_after_all_boss_intros() {
    junkenstein::verify_natural_ending(junkenstein::NormalEnding::LateDefeat);
}

#[test]
#[ignore = "requires separately prepared 104319 Junkenstein runtime"]
fn normal_deadeye_clears_natural_bosses_and_reaches_allied_victory() {
    junkenstein::verify_natural_ending(junkenstein::NormalEnding::DeadeyeVictory);
}

mod junkenstein;

#[test]
fn scene_enemy_creation_is_atomic_and_retains_late_join_health_and_skin() {
    use crate::{
        script_enemy::{EnemySpawn, fixture},
        script_wire::Variables,
    };
    let mut room = RoomSimulation::new(Vec::new()).unwrap();
    for fail in [true, false, false] {
        let (graphs, codecs, definition, request) = fixture(fail);
        let next = room.next_bot;
        let count = room.bodies().unwrap().len();
        let result = room.create_enemy(EnemySpawn {
            graphs,
            codecs,
            definition: &definition,
            request: &request,
            mode: 0x0230_0000_0000_019f,
            mode_variables: Arc::new(Variables::new()),
            tick: 1000,
            walk: crate::game_bots::BotWalk::default(),
            gate: None,
        });
        if fail {
            assert!(result.unwrap_err().starts_with("enemy controller startup:"));
            assert_eq!(room.next_bot, next);
            assert_eq!(room.bodies().unwrap().len(), count);
            assert!(room.take_created().is_empty());
            continue;
        }
        let entity = result.unwrap();
        assert_eq!(entity, next);
        assert_eq!(room.next_bot, next + 2);
        assert_eq!(room.bodies().unwrap().len(), count + 1);
        let updates = room.take_created();
        assert_eq!(updates.len(), 1);
        assert_eq!(room.initial_creates().unwrap().last().unwrap(), &updates[0]);
        let enemy = state(&room, entity).unwrap().enemy.as_ref().unwrap();
        assert_eq!(enemy.binding.controller, entity + 1);
        assert_eq!(
            enemy.scripts.controller.snapshot().unwrap().entity,
            entity + 1
        );
        assert_eq!(room.enemy_projections().unwrap().count(), count + 1);
        let hit = room
            .apply_bot_impact(1, 1000, FIXTURE_CONTEXT, &fixture_impact(entity, 500.0))
            .unwrap()
            .unwrap();
        assert!(matches!(
            hit.notices[0],
            Some(CombatNotice::Hit { killed: true, .. })
        ));
        assert!(state(&room, entity).unwrap().respawn_at.is_none());
        let late = room.initial_creates().unwrap().pop().unwrap();
        let Some(Operation::Create(create)) = late.operation else {
            panic!("late join create")
        };
        assert_eq!(create.skin.guid, 0x0b20_0000_0000_0010);
        assert_ne!(room.initial_creates().unwrap().last().unwrap(), &updates[0]);
    }
}

#[test]
#[ignore = "requires the external 104319 Junkenstein NPC catalog and graph bundle"]
fn authored_zomnic_starts_both_components_before_room_publication() {
    use crate::{
        npc_catalog::NpcCatalog,
        script_codecs::CodecTable,
        script_enemy::EnemySpawn,
        script_graph::GraphStore,
        script_init::{CreationKind, EntityCreation},
        script_wire::{Value, Variables},
    };
    let root = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let graphs = Arc::new(GraphStore::load(root.join("data/game_graphs_174.json.gz")).unwrap());
    let codecs = Arc::new(CodecTable::load(root.join("data/statescript_codecs_174.json")).unwrap());
    let catalog = NpcCatalog::load(root.join("data/junkenstein/junkenstein-npcs.json")).unwrap();
    let definition = catalog.get(0x02e0_0000_0000_0175).unwrap();
    let request = EntityCreation {
        position: [74.0, 12.6, -99.0],
        kind: CreationKind::Hero {
            hero: definition.hero,
            team: 1,
            yaw: 180.0,
            flags: [Value::Bool(false), Value::Bool(false)],
            controller: None,
            skin: None,
        },
    };
    let mut room = RoomSimulation::new(Vec::new()).unwrap();
    let entity = room
        .create_enemy(EnemySpawn {
            graphs,
            codecs,
            definition,
            request: &request,
            mode: 0x0230_0000_0000_019f,
            mode_variables: Arc::new(Variables::new()),
            tick: 4200,
            walk: crate::game_bots::BotWalk::default(),
            gate: None,
        })
        .unwrap();
    assert_eq!(entity, BOT_ENTITY);
    assert_eq!(room.next_bot, BOT_ENTITY + 2);
    assert_eq!(room.bodies().unwrap().len(), 1);
    assert_eq!(room.world.entities().len(), 1);
    assert_eq!(room.take_created().len(), 1);
    assert_eq!(room.enemy_projections().unwrap().count(), 1);
    let state = room.state_mut(entity).unwrap().unwrap();
    let enemy = state.enemy.as_mut().unwrap();
    assert_eq!(enemy.binding.body, entity);
    assert_eq!(enemy.binding.controller, entity + 1);
    assert_eq!(state.vitals.maximum(), 200.0);
    let requests = enemy.scripts.behavior_requests().collect::<Vec<_>>();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].class, "STU_600E3C59");
    assert_eq!(requests[0].priority, 10.0);
    assert_eq!(requests[0].inputs, [2.0, 2.0, 1.0, f64::from(0.2f32), 1.0]);
    assert_eq!(requests[1].class, "STU_B61E6681");
    assert_eq!(requests[1].priority, 0.0);
    assert_eq!(requests[1].inputs, [1.5, 1.0, 0.0, 0.0, 0.0]);
    assert!(
        enemy
            .scripts
            .body
            .snapshot()
            .unwrap()
            .initialization_complete()
    );
    assert!(
        enemy
            .scripts
            .controller
            .snapshot()
            .unwrap()
            .initialization_complete()
    );
}

#[test]
#[ignore = "requires external 104319 encounter body definitions"]
fn boss_controller_mismatch_is_rejected_before_room_allocation() {
    use crate::{
        game_heroes::HeroCatalog,
        npc_catalog::encounter_definition,
        script_codecs::CodecTable,
        script_enemy::EnemySpawn,
        script_graph::GraphStore,
        script_init::{CreationKind, EntityCreation},
        script_wire::{Value, Variables},
    };
    let root = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let graphs = Arc::new(GraphStore::load(root.join("data/game_graphs_174.json.gz")).unwrap());
    let codecs = Arc::new(CodecTable::load(root.join("data/statescript_codecs_174.json")).unwrap());
    let heroes = HeroCatalog::load(root.join("data/game_heroes_174.json")).unwrap();
    for index in [2, 4, 0x16, 0x40, 0x65] {
        let hero = 0x02e0_0000_0000_0000 | index;
        let definition = encounter_definition(None, &graphs, &heroes, hero).unwrap();
        for controller in [None, Some(0x0400_0000_0000_ffff)] {
            let mut room = RoomSimulation::new(Vec::new()).unwrap();
            let next = room.next_bot;
            let request = EntityCreation {
                position: [74.0, 12.6, -99.0],
                kind: CreationKind::Hero {
                    hero,
                    team: 1,
                    yaw: 180.0,
                    flags: [Value::Bool(false), Value::Bool(false)],
                    controller,
                    skin: None,
                },
            };
            let result = room.create_enemy(EnemySpawn {
                graphs: Arc::clone(&graphs),
                codecs: Arc::clone(&codecs),
                definition: &definition,
                request: &request,
                mode: 0x0230_0000_0000_019f,
                mode_variables: Arc::new(Variables::new()),
                tick: 4200,
                walk: crate::game_bots::BotWalk::default(),
                gate: None,
            });
            assert_eq!(
                result.unwrap_err(),
                "scene enemy creation differs from its validated definition"
            );
            assert_eq!(room.next_bot, next);
            assert_eq!(room.bodies().unwrap().len(), 0);
            assert!(room.take_created().is_empty());
        }
    }
}

pub(super) fn fixture_impact(entity: u32, damage: f64) -> Impact {
    Impact {
        tags: crate::game_hitscan::DamageTags::EMPTY,
        entity,
        distance: 1.0,
        damage,
        critical: false,
        loadout: None,
    }
}

#[test]
#[ignore = "requires the external 104319 Junkenstein NPC, graph and collision pack"]
fn authored_zomnic_attack_updates_gate_damage_and_retires_the_pair() {
    use crate::{
        game_content::{JUNKENSTEIN_MAP, JunkensteinCard, MapCatalog},
        npc_catalog::NpcCatalog,
        script_codecs::CodecTable,
        script_enemy::{EnemyGate, EnemySpawn, SceneMessage},
        script_expr::to_float,
        script_graph::GraphStore,
        script_init::{CreationKind, EntityCreation},
        script_snapshot_wire::View,
        script_stream::{BodyStream, DeltaKind},
        script_wire::{Value, Variables},
    };
    let root = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let assets = SceneAssets::load(&root).unwrap();
    let maps = MapCatalog::load(root.join("data/game_maps_174.json")).unwrap();
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
    assert!(scene.encounter_result_wire().unwrap().is_none());
    assert!(scene.encounter_outcome_wire(1).unwrap().is_none());
    let projection = scene.gate_projection().unwrap();
    let before = projection
        .encoder
        .project(projection.snapshot, View::Observer)
        .unwrap();
    let mut stream = BodyStream::new(projection.snapshot.entity);
    let baseline = projection.encoder.queue_full(&mut stream, &before).unwrap();
    assert!(!stream.ready_to_send(baseline.last));
    stream.create_arrived();
    assert!(stream.ready_to_send(baseline.last));
    stream.sent(baseline.last);
    stream.arrived(baseline.last);
    let initial_gate_damage = scene
        .gate_snapshot()
        .unwrap()
        .variables
        .get(&8569)
        .cloned()
        .unwrap_or(Value::Null);
    let gate = scene.junkenstein_gate().unwrap();
    let gate = EnemyGate {
        entity: gate.entity,
        position: gate.position,
    };
    let graphs = Arc::new(GraphStore::load(root.join("data/game_graphs_174.json.gz")).unwrap());
    let codecs = Arc::new(CodecTable::load(root.join("data/statescript_codecs_174.json")).unwrap());
    let catalog = NpcCatalog::load(root.join("data/junkenstein/junkenstein-npcs.json")).unwrap();
    let definition = catalog.get(0x02e0_0000_0000_0175).unwrap();
    let variables = Arc::new(scene.encounter_snapshot().unwrap().variables.clone());
    let request = EntityCreation {
        position: [gate.position[0] + 1.0, gate.position[1], gate.position[2]],
        kind: CreationKind::Hero {
            hero: definition.hero,
            team: 1,
            yaw: 0.0,
            flags: [Value::Bool(false), Value::Bool(false)],
            controller: None,
            skin: None,
        },
    };
    let spawn = |scene: &mut crate::scene_assets::PreparedScene, request: &EntityCreation, tick| {
        scene
            .bot_simulation()
            .0
            .create_enemy(EnemySpawn {
                graphs: Arc::clone(&graphs),
                codecs: Arc::clone(&codecs),
                definition,
                request,
                mode: card.mode,
                mode_variables: Arc::clone(&variables),
                tick,
                walk: crate::game_bots::BotWalk::default(),
                gate: Some(gate),
            })
            .unwrap()
    };
    let entity = spawn(&mut scene, &request, 4200);
    let mut saw_dead = false;
    for tick in 4200..=4800 {
        scene.step_encounter(tick, Some(crate::game_heroes::CASSIDY));
        let (room, queries) = scene.bot_simulation();
        room.set_external_targets(Vec::new());
        room.finish_tick(
            &queries,
            tick,
            Duration::from_millis(u64::from(tick - 4200) * 16),
        )
        .unwrap();
        if let Some(state) = state(room, entity) {
            assert!(
                state.enemy.as_ref().unwrap().fault.is_none(),
                "tick {tick}: {:?}",
                state.enemy.as_ref().unwrap().fault
            );
            if state.bot.dead {
                if !saw_dead {
                    assert_eq!(state.vitals.death().unwrap().frame, tick);
                }
                saw_dead = true;
                assert!(state.respawn_at.is_none());
            }
        }
    }
    let (room, _) = scene.bot_simulation();
    assert!(
        saw_dead,
        "Zomnic did not die: gate={gate:?}, actor={:?}",
        state(room, entity).map(|state| (
            state.bot.position,
            state.vitals.total(),
            state.enemy.as_ref().map(|enemy| &enemy.fault),
        ))
    );
    assert!(state(room, entity).is_none());
    assert_eq!(room.take_retired_enemies(), [entity]);
    assert!(!room.take_scripted_health().is_empty());
    let gate = scene.gate_snapshot().unwrap();
    assert!(gate.diagnostics.is_empty(), "{:?}", gate.diagnostics);
    assert_ne!(
        gate.variables.get(&8569),
        Some(&initial_gate_damage),
        "gate damage counter did not change: {:?}",
        gate.variables
    );
    assert_eq!(gate.instances[&1].states[&19].activation, 1);
    let projection = scene.gate_projection().unwrap();
    let after = projection
        .encoder
        .project_next(projection.snapshot, &before)
        .unwrap();
    assert!(after.instance_states(1).unwrap().contains(&0));
    let hit = projection
        .encoder
        .queue_delta(
            &mut stream,
            &after,
            DeltaKind::Observer { resend: true },
            16,
        )
        .unwrap()
        .unwrap();
    stream.sent(hit.last);
    stream.arrived(hit.last);
    assert_eq!(
        scene.take_encounter_voice_requests(),
        [crate::junkenstein_voice::VoiceRequest {
            anchor: 0x8000_0032,
            speaker: crate::world::GAME_MODE_ENTITY,
            stimulus: 0x0ee0_0000_0000_06c7,
            line: 0x0760_0000_0000_8c71,
        }]
    );
    scene.step_encounter(5400, None);
    assert!(!scene.gate_snapshot().unwrap().instances[&1].states[&19].active);
    assert!(scene.take_encounter_voice_requests().is_empty());

    let gate = scene.junkenstein_gate().unwrap().entity;
    for (tick, remaining, expected_state) in [
        (5401, 0.625, 6),
        (5402, 0.375, 7),
        (5403, 0.125, 8),
        (5404, 0.0, 8),
    ] {
        let snapshot = scene.gate_snapshot().unwrap();
        let capacity = to_float(&snapshot.resolved_bindings[&(0, 7786)]);
        assert!(capacity > 0.0);
        let consumed = to_float(&snapshot.variables[&8569]);
        scene.bot_simulation().0.queue_scene_message(SceneMessage {
            target: gate,
            message: 0x0240_0000_0000_06b2,
            parameters: Variables::from([
                (0x3ac, Value::Float(capacity * (1.0 - remaining) - consumed)),
                (0x12, Value::Entity(entity)),
            ]),
        });
        scene.step_encounter(tick, None);
        let projection = scene.gate_projection().unwrap();
        assert!(
            projection.snapshot.diagnostics.is_empty(),
            "{:?}",
            projection.snapshot.diagnostics
        );
        assert_eq!(to_float(&projection.snapshot.variables[&8571]), remaining);
        let next = projection
            .encoder
            .project_next(projection.snapshot, stream.base().unwrap().1)
            .unwrap();
        projection.encoder.full(&next).unwrap();
        let states = next.instance_states(1).unwrap();
        assert_eq!(
            states
                .into_iter()
                .filter(|raw| [0, 6, 7, 8].contains(raw))
                .collect::<Vec<_>>(),
            [expected_state],
        );
        if let Some(delta) = projection
            .encoder
            .queue_delta(&mut stream, &next, DeltaKind::Observer { resend: true }, 16)
            .unwrap()
        {
            stream.sent(delta.last);
            stream.arrived(delta.last);
        } else {
            assert_eq!(remaining, 0.0);
        }
    }
    let notices = scene.take_encounter_voice_requests();
    assert_eq!(
        notices
            .iter()
            .map(|notice| notice.stimulus)
            .collect::<Vec<_>>(),
        [0x0ee0_0000_0000_06b8, 0x0ee0_0000_0000_06b9],
    );
    let street = EntityCreation {
        position: [74.0, 12.6, -99.0],
        ..request
    };
    let survivor = spawn(&mut scene, &street, 5405);
    let (room, queries) = scene.bot_simulation();
    let source = crate::world::GAME_MODE_ENTITY;
    let caller = crate::script_init::Site {
        entity: source,
        instance: 2,
        graph: 0x0cba,
        node: Some(9),
    };
    assert!(room.queue_encounter_wave(
        caller,
        survivor + 1,
        crate::script_init::PlayScript {
            policy: 0,
            overrides: Vec::new(),
            graph: 0x0c95,
            source,
            parameters: vec![(
                0x0a40_0000_0000_03a1,
                Value::Array(vec![Value::Vector(street.position)])
            )],
            auto_retire: true,
        }
    ));
    room.finish_tick(&queries, 5405, Duration::from_millis((5405 - 4200) * 16))
        .unwrap();
    assert!(room.encounter_body_alive(survivor));
    assert!(
        state(room, survivor)
            .unwrap()
            .enemy
            .as_ref()
            .unwrap()
            .scripts
            .body
            .snapshot()
            .unwrap()
            .instances
            .values()
            .any(|instance| instance.graph == 0x0cbc)
    );
    for tick in 5405..=5900 {
        scene.step_encounter(tick, Some(crate::game_heroes::CASSIDY));
    }
    let (room, queries) = scene.bot_simulation();
    room.finish_tick(&queries, 5900, Duration::from_millis((5900 - 4200) * 16))
        .unwrap();
    if let Some(survivor) = state(room, survivor) {
        let enemy = survivor.enemy.as_ref().unwrap();
        let snapshot = enemy.scripts.body.snapshot().unwrap();
        panic!(
            "result did not retire live Zomnic: fault={:?} requested={} roots={:?}",
            enemy.fault,
            enemy.scripts.retirement_requested,
            snapshot
                .instances
                .values()
                .map(|instance| (instance.graph, instance.variables.get(&8617)))
                .collect::<Vec<_>>()
        );
    }
    assert_eq!(room.take_retired_enemies(), [survivor]);
    let mode = scene.encounter_snapshot().unwrap();
    let root = &mode.instances[&2];
    assert_eq!(root.variables[&59], Value::Integer(5));
    assert_eq!(root.states[&24].activation, 1);
    let handoff = root
        .states
        .get(&8)
        .unwrap_or_else(|| panic!("missing authored phase handoff: {:?}", mode.diagnostics));
    assert_eq!(handoff.activation, 1);
    assert_eq!(
        handoff.kind,
        crate::script_init::StateKind::PersistentSend(
            crate::junkenstein_messages::POST_MATCH_PHASE
        ),
    );
    let phase = scene.encounter_post_match_wire().unwrap().unwrap();
    let native_result = scene.encounter_match_result().unwrap();
    assert_eq!(native_result.index, 114);
    assert_eq!(
        native_result.fields[7],
        Some(crate::world::ComponentValue::I32(1)),
        "native recap must identify the enemy winner after gate destruction"
    );
    assert!(matches!(
        native_result.fields[9],
        Some(crate::world::ComponentValue::F32(seconds)) if seconds > 0.0
    ));
    let mut wire = crate::bits::BitReader::new(phase.as_bytes());
    assert_eq!(wire.bits(2).unwrap(), 0);
    assert_eq!(wire.w_u16().unwrap(), 2);
    assert_eq!(wire.bits(2).unwrap(), 0); // existing mode root
    assert_eq!(wire.w_u16().unwrap(), 0);
    let screens = mode
        .instances
        .values()
        .find(|instance| instance.graph == 0x180a)
        .unwrap();
    assert_eq!(
        screens.states.get(&3).map(|state| state.activation),
        Some(1),
        "{:?}",
        mode.diagnostics
    );

    assert_eq!(screens.states[&21].activation, 1);
    assert!((to_float(&screens.variables[&15076]) - 41.1).abs() < 0.0001);
    let Value::Array(sequence) = &screens.variables[&16638] else {
        panic!("the post-match screen sequence must be an array");
    };
    assert_eq!(sequence.len(), 4);
    let Value::Map(summary) = &sequence[3] else {
        panic!("the fourth screen phase must have a configuration map");
    };
    assert!(summary.iter().any(|(key, value)| {
        *key == Value::Integer(16748)
            && matches!(value, Value::Array(graphs) if graphs.first() == Some(&Value::Asset(0x0580_0000_0000_0283)))
    }));

    let (&result_id, results) = mode
        .instances
        .iter()
        .find(|(_, instance)| instance.graph == 0x0cc7)
        .unwrap();
    assert_eq!(results.states[&2].activation, 1);
    assert_eq!(
        results.states[&2].kind,
        crate::script_init::StateKind::PersistentSend(crate::junkenstein_messages::RESULT_SUMMARY),
    );
    assert_eq!(mode.variables[&8525], Value::Integer(0));
    let payload = scene.encounter_result_wire().unwrap().unwrap();
    let mut wire = crate::bits::BitReader::new(payload.as_bytes());
    assert_eq!(wire.bits(2).unwrap(), 0);
    assert_eq!(wire.w_u16().unwrap(), result_id);
    assert_eq!(wire.bits(2).unwrap(), 2);
    assert_eq!(wire.bits(16).unwrap(), 0x0cc7);
    assert_eq!(wire.bits(2).unwrap(), 0);
    assert!(wire.bit().unwrap());
    assert_eq!((wire.w_var().unwrap(), wire.w_var().unwrap()), (2, 13));
    let outcome = scene.encounter_outcome_wire(41).unwrap().unwrap();
    let mut wire = crate::bits::BitReader::new(outcome.as_bytes());
    assert_eq!(wire.bits(2).unwrap(), 0);
    assert_eq!(wire.w_u16().unwrap(), 0);
    assert!(!wire.bit().unwrap()); // empty entity variables
    assert!(wire.bit().unwrap()); // frame action present
    assert_eq!(wire.w_u32().unwrap(), 41);
    wire.w_var().unwrap();
    assert_eq!(wire.bits(2).unwrap(), 1);
    wire.bits(8).unwrap();
    assert!(wire.bit().unwrap());
    assert_eq!(wire.w_u16().unwrap(), 2);
    assert_eq!(wire.bits(6).unwrap(), 45);
    assert_eq!(wire.bits(3).unwrap(), 0);
    let gate = scene.gate_snapshot().unwrap();
    assert_eq!(gate.instances[&1].variables[&1303], Value::Integer(3));
    let messages: Vec<_> = scene
        .bot_simulation()
        .0
        .take_defender_messages()
        .into_iter()
        .filter(|message| message.message == crate::junkenstein_messages::ENCOUNTER_RESULT)
        .collect();
    let [result] = messages.as_slice() else {
        panic!("expected one authored match result, received {messages:?}");
    };
    assert_eq!(result.source, crate::world::GAME_MODE_ENTITY);
    assert!(!crate::script_expr::truthy(&result.parameters[&0x3ae]));
    assert_eq!(result.parameters[&0x3b5], Value::Integer(3));
    let hero = scene.heroes().find_hero("Cassidy").unwrap();
    let mut mover = scene.player_spawn_mover(hero.body, 0).unwrap();
    let pose = mover.pose();
    let mut owner = scene
        .owner_body_runtime(
            hero,
            crate::script_player::OwnerBodySpawn {
                slot: 0,
                controller: 0xa000_0100,
                entity: 0xa000_0101,
                body_filter: crate::match_scene::team_bit(0).unwrap(),
                frame: 5900,
                pose,
                quanta: RETAIL_QUANTA,
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(owner.receive_defender_message(result.clone()).unwrap(), 1);
    let snapshot = owner.snapshot().unwrap();
    let defender = snapshot
        .instances
        .values()
        .find(|instance| instance.graph == 0x0ca5)
        .unwrap();
    assert_eq!(defender.states[&32].activation, 1);
    assert_eq!(defender.variables[&8504], Value::Integer(3));
    assert!(
        snapshot.diagnostics.is_empty(),
        "{:?}",
        snapshot.diagnostics
    );
    let encoder = owner.encoder();
    let projected = encoder.project(snapshot, View::Owner).unwrap();
    encoder.full(&projected).unwrap();
    assert_eq!(owner.cinematic_aim(), Some([pose.yaw, pose.pitch]));
    mover.set_cinematic_aim(owner.cinematic_aim());
    mover
        .step(
            scene.movement_index(),
            Command {
                frame: 5901,
                yaw: pose.yaw.wrapping_add(1200),
                pitch: pose.pitch.wrapping_sub(1200),
                ..Command::default()
            },
            0.016,
            |_, _| {},
        )
        .unwrap();
    assert_eq!(
        [mover.pose().yaw, mover.pose().pitch],
        [pose.yaw, pose.pitch]
    );
    let mut phases = Vec::new();
    let mut previous = Vec::new();
    let mut summary_instance = None;
    let mut screen_projection = None;
    assert!(!scene.encounter_screens_complete());

    for tick in 5901..=8500 {
        scene.step_encounter(tick, Some(crate::game_heroes::CASSIDY));
        let mode = scene.encounter_snapshot().unwrap();
        let scopes: Vec<_> = mode
            .instances
            .iter()
            .filter_map(|(&id, instance)| {
                let (scope, 16) = instance.parent? else {
                    return None;
                };
                let (manager, 17) = mode.instances.get(&scope)?.parent? else {
                    return None;
                };
                (mode.instances[&manager].graph == 0x180a).then_some((id, instance.graph))
            })
            .collect();
        if let Some((id, _)) = scopes.iter().find(|(_, graph)| *graph == 0x0283) {
            assert_eq!(summary_instance.get_or_insert(*id), id);
            assert!(!scene.encounter_screens_complete());
        }
        if scopes != previous {
            let (projection, payload) = scene
                .encounter_screen_delta(screen_projection.as_ref())
                .unwrap()
                .unwrap();
            if previous.is_empty() {
                let (&manager, instance) = mode
                    .instances
                    .iter()
                    .find(|(_, instance)| {
                        instance.graph == 0x180a && instance.states.contains_key(&4)
                    })
                    .unwrap();
                assert_eq!(instance.states[&4].activation, 1);
                assert_eq!(
                    instance.states[&4].kind,
                    crate::script_init::StateKind::PersistentSend(
                        crate::junkenstein_messages::POST_MATCH_SIGNAL
                    )
                );
                assert!(projection.instance_states(manager).unwrap().contains(&4));
                assert_eq!(
                    projection.instance_variables(manager).unwrap().get(&7525),
                    Some(&Value::Integer(1)),
                    "the client screen manager needs the resolved defeat winner"
                );
            }
            if let Some(&(banner, _)) = scopes.iter().find(|(_, graph)| *graph == 0x027f) {
                assert_eq!(
                    projection.instance_variables(banner).unwrap()[&7525],
                    Value::Integer(1)
                );
            }
            if scopes.iter().any(|&(_, graph)| graph == 0x0283) {
                assert_eq!(
                    projection.entity_variables().get(&10274),
                    Some(&Value::Bool(true)),
                    "the recap presenter needs its authored entity input"
                );
            }
            let mut wire = crate::bits::BitReader::new(payload.as_bytes());
            assert_eq!(wire.bits(2).unwrap(), 0);
            let mut seen = std::collections::BTreeMap::new();
            let mut id = 0;
            loop {
                let increment = wire.w_u16().unwrap();
                if increment == 0 {
                    break;
                }
                id += increment;
                if wire.bit().unwrap() {
                    assert!(!mode.instances.contains_key(&id));
                    seen.insert(id, None);
                } else if wire.bit().unwrap() {
                    let graph = wire.bits(16).unwrap();
                    assert_eq!(wire.bits(2).unwrap(), 0);
                    let parent = if wire.bit().unwrap() {
                        Some((
                            wire.w_var().unwrap().try_into().unwrap(),
                            wire.w_var().unwrap().try_into().unwrap(),
                        ))
                    } else {
                        None
                    };
                    assert!(!wire.bit().unwrap());
                    if scopes.iter().any(|&(scope, _)| scope == id) {
                        assert_eq!(parent, None);
                    } else {
                        assert_eq!(mode.instances[&id].parent, parent);
                        assert_ne!(
                            parent.map(|(_, raw)| raw),
                            Some(17),
                            "server list row was sent"
                        );
                    }
                    assert_eq!(u64::from(mode.instances[&id].graph), graph);
                    seen.insert(id, Some(graph));
                }
            }
            for &(id, graph) in &scopes {
                if !previous.iter().any(|&(previous, _)| previous == id) {
                    assert_eq!(seen[&id], Some(u64::from(graph)));
                }
            }
            for &(id, _) in &previous {
                if !scopes.iter().any(|&(current, _)| current == id) {
                    assert_eq!(seen[&id], None);
                }
            }
            let mut stream = crate::script_stream::Stream::new(crate::world::GAME_MODE_ENTITY);
            let initial = stream.full(scene.mode_ui()).unwrap();
            assert!(stream.arrived(initial.last));
            let chunk = stream.reliable_delta(&payload).unwrap();
            assert!(stream.data_in_flight());
            assert!(stream.arrived(chunk.last));
            assert!(!stream.data_in_flight());
            let packet =
                crate::world::frame(u32::MAX, u32::MAX, &[], &[], &[&chunk.update]).unwrap();
            assert!(
                packet.len() <= crate::world::MAX_PAYLOAD,
                "screen delta too large: {}",
                packet.len()
            );
            screen_projection = Some(projection);
            phases.push(scopes.clone());
            previous = scopes;
        }
    }
    let graphs: Vec<Vec<_>> = phases
        .iter()
        .map(|phase| {
            let mut graphs: Vec<_> = phase.iter().map(|(_, graph)| *graph).collect();
            graphs.sort_unstable();
            graphs
        })
        .collect();
    assert_eq!(
        graphs,
        [
            vec![0x027f, 0x0d94, 0x17df, 0x181e, 0x184c],
            vec![0x0281, 0x17df, 0x17e2, 0x181d, 0x184c, 0x1f93],
            vec![0x0282, 0x17df, 0x17e2, 0x1818, 0x184d],
            vec![0x0283, 0x0b46, 0x17e3, 0x17e4, 0x184d],
            vec![],
        ]
    );
    for (graph, earlier, later) in [(0x17df, 0, 1), (0x17e2, 1, 2), (0x184d, 2, 3)] {
        let id = |index: usize| {
            phases[index]
                .iter()
                .find(|(_, candidate)| *candidate == graph)
                .unwrap()
                .0
        };
        assert_eq!(id(earlier), id(later));
    }
    let summary_instance = summary_instance.expect("one authored results graph");
    assert!(scene.encounter_screens_complete());
    assert!(
        !scene
            .encounter_snapshot()
            .unwrap()
            .instances
            .contains_key(&summary_instance)
    );
}

pub(super) const FIXTURE_CONTEXT: ShotContext = ShotContext {
    team: 0,
    quanta: RETAIL_QUANTA,
    tick: 1000,
    now: Duration::from_secs(4),
};

pub(super) fn state(bots: &RoomSimulation, wire_id: u32) -> Option<&BotState> {
    bots.world.get(*bots.by_wire.get(&wire_id)?)
}

pub(super) fn flight_fixture(owner: BodyId, speed: f64) -> AdmittedFlight {
    use crate::game_projectiles::{OwnerProjectiles, ProjectileLeases};

    let frame = FIXTURE_CONTEXT.tick;
    let definition = 0x0400_0000_0000_072c;
    let leases = ProjectileLeases::new(0, frame, [definition]).unwrap();
    let token = OwnerProjectiles::new(owner.entity, leases)
        .admit(
            ShotEvent {
                entity: owner.entity,
                instance: 1,
                graph: 42,
                raw_state: 0,
                frame,
                time_ms: 0,
                activation: 1,
                activation_ms: 0,
                ordinal: 0,
            },
            definition,
            3,
        )
        .unwrap();
    let flight = ProjectileFlight::new(
        ProjectileBallistics {
            speed,
            lifetime: 0.04,
            definition: Some(definition),
            direct: None,
            blast: None,
        },
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        None,
    )
    .unwrap();
    AdmittedFlight {
        token,
        flight,
        stun: None,
        hit_effect: None,
    }
}

#[test]
fn projectile_stun_applies_only_to_surviving_hits_and_expires_exclusively() {
    use crate::{
        collision_cache::{CollisionCache, Mesh, Triangle},
        collision_scene::CollisionScene,
        mover_settings::MoverCatalog,
    };
    let cache = CollisionCache {
        header: serde_json::from_str(
            r#"{"meshes":[[3,1]],"hulls":0,"models":0,"placements":0,"identifiers":[]}"#,
        )
        .unwrap(),
        meshes: vec![Mesh {
            vertices: vec![[-1.0, 0.0, 0.0], [-1.0, 2.0, 0.0], [-1.0, 0.0, 1.0]],
            triangles: vec![Triangle {
                corners: [0, 1, 2],
                wings: [-1; 3],
                mask: 0x18,
                surface: 0,
            }],
        }],
        hulls: vec![],
        models: vec![],
        placements: vec![],
    };
    let ground = CollisionScene::movement_probe(&cache, &Default::default()).unwrap();
    let movers = MoverCatalog::default();
    let queries = BotQueries::new(false, &movers, &ground, None);
    assert_eq!(Stun::from_seconds(f64::from(0.8f32)).unwrap().ticks, 50);
    assert_eq!(Stun::from_seconds(f64::from(1.7f32)).unwrap().ticks, 107);
    for value in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(Stun::from_seconds(value).is_none());
    }
    for amount in [25.0, 1000.0] {
        let bots = fixture_bots();
        let victim = bots[0].entity;
        let unhit = bots[1].entity;
        let mut room = RoomSimulation::new(bots).unwrap();
        let owner = BodyId {
            entity: 0xa000_0101,
            generation: 1,
        };
        let mut launch = flight_fixture(owner, 100.0);
        launch.stun = Stun::from_seconds(0.8);
        launch.flight = ProjectileFlight::new(
            ProjectileBallistics {
                speed: 100.0,
                lifetime: 1.0,
                definition: None,
                direct: Some(HitDefinition {
                    tags: crate::game_hitscan::DamageTags::EMPTY,
                    amount,
                    loadout: None,
                    critical: 1.0,
                    falloff: None,
                }),
                blast: None,
            },
            [0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
            None,
        )
        .unwrap();
        let (_, hits) = room
            .advance_projectiles(&queries, 7, owner, vec![launch], FIXTURE_CONTEXT)
            .unwrap();
        assert_eq!(hits.len(), 1);
        let bot = &state(&room, victim).unwrap().bot;
        assert_eq!(bot.is_stunned(1049), amount == 25.0);
        assert!(!bot.is_stunned(1050));
        assert!(!state(&room, unhit).unwrap().bot.is_stunned(1001));
    }
}

pub(super) fn fixture_ground() -> crate::collision_scene::CollisionScene {
    use crate::{
        collision_cache::{CollisionCache, Mesh, Triangle},
        collision_scene::CollisionScene,
    };

    let cache = CollisionCache {
        header: serde_json::from_str(
            r#"{"meshes":[[3,1]],"hulls":0,"models":0,"placements":0,"identifiers":[]}"#,
        )
        .unwrap(),
        meshes: vec![Mesh {
            vertices: vec![[-1.0, 0.0, 0.0], [-1.0, 2.0, 0.0], [-1.0, 0.0, 1.0]],
            triangles: vec![Triangle {
                corners: [0, 1, 2],
                wings: [-1; 3],
                mask: 0x18,
                surface: 0,
            }],
        }],
        hulls: vec![],
        models: vec![],
        placements: vec![],
    };
    CollisionScene::movement_probe(&cache, &Default::default()).unwrap()
}

#[test]
fn projectile_flights_keep_peer_generation_and_report_terminal_outcomes() {
    use crate::mover_settings::MoverCatalog;

    let ground = fixture_ground();
    let movers = MoverCatalog::default();
    let queries = BotQueries::new(false, &movers, &ground, None);
    let mut simulation = RoomSimulation::new(Vec::new()).unwrap();
    let mut context = FIXTURE_CONTEXT;
    let owner = BodyId {
        entity: 0xa000_0101,
        generation: 1,
    };
    let other = BodyId {
        entity: 0xa100_0101,
        generation: 1,
    };
    let first = flight_fixture(owner, 1.0);
    let second = flight_fixture(other, 1.0);
    let token = first.token;
    assert_eq!(token, second.token);
    for (connection, owner, flight) in [(7, owner, first), (8, other, second)] {
        let (outcomes, hits) = simulation
            .advance_projectiles(&queries, connection, owner, vec![flight], context)
            .unwrap();
        assert!(outcomes.is_empty() && hits.is_empty());
    }
    let old_handle = simulation.projectiles[&7].order[0];
    let other_handle = simulation.projectiles[&8].order[0];

    context.tick += 3;
    let owner = BodyId {
        generation: 2,
        ..owner
    };
    let (outcomes, hits) = simulation
        .advance_projectiles(&queries, 7, owner, Vec::new(), context)
        .unwrap();
    assert!(outcomes.is_empty() && hits.is_empty());
    assert!(simulation.world.get::<AdmittedFlight>(old_handle).is_none());
    assert!(
        simulation
            .world
            .get::<AdmittedFlight>(other_handle)
            .is_some()
    );

    let (outcomes, hits) = simulation
        .advance_projectiles(&queries, 8, other, Vec::new(), context)
        .unwrap();
    assert!(hits.is_empty());
    assert!(matches!(
        outcomes.as_slice(),
        [FlightOutcome::Ended { token: observed, travelled }]
            if *observed == token && (*travelled - 0.04).abs() < 1e-12
    ));
    assert!(
        simulation
            .world
            .get::<AdmittedFlight>(other_handle)
            .is_none()
    );

    let too_far = flight_fixture(owner, 10_000_000.0);
    let (outcomes, hits) = simulation
        .advance_projectiles(&queries, 7, owner, vec![too_far], context)
        .unwrap();
    assert!(hits.is_empty());
    assert!(matches!(outcomes.as_slice(), [FlightOutcome::Failed(observed)] if *observed == token));
    assert!(simulation.projectiles.is_empty());
    assert!(simulation.world.entities().is_empty());
}

#[test]
fn broken_bot_indexes_are_errors_and_do_not_partially_respawn_a_batch() {
    use crate::mover_settings::MoverCatalog;

    let context = FIXTURE_CONTEXT;
    let ground = fixture_ground();
    let movers = MoverCatalog::default();
    let queries = BotQueries::new(false, &movers, &ground, None);
    let mut simulation = RoomSimulation::new(fixture_bots()).unwrap();
    let first = BOT_ENTITY;
    let second = first + 1;
    let first_handle = simulation.by_wire[&first];
    let broken_handle = simulation.by_wire[&second];
    simulation
        .apply_bot_impact(7, context.tick, context, &fixture_impact(first, 100.0))
        .unwrap();
    simulation
        .world
        .entity_mut(broken_handle)
        .remove::<BotState>();
    let missing = |error: CombatError| {
        assert!(matches!(error, CombatError::MissingBot { wire, entity }
            if wire == second && entity == broken_handle));
    };
    missing(simulation.bodies().err().unwrap());
    missing(simulation.body_healths().unwrap_err());
    missing(simulation.debug_actors().err().unwrap());
    missing(simulation.for_each_body_mut(|_| {}).unwrap_err());
    missing(simulation.record_poses(&queries, context.tick).unwrap_err());
    missing(
        simulation
            .apply_bot_impact(7, context.tick, context, &fixture_impact(second, 1.0))
            .err()
            .unwrap(),
    );
    let next_bot = simulation.next_bot;
    missing(
        simulation
            .respawn_due(context.tick + 1, context.now + BOT_RESPAWN_TIME)
            .err()
            .unwrap(),
    );
    assert_eq!(simulation.next_bot, next_bot);
    assert_eq!(simulation.by_wire[&first], first_handle);
    assert!(state(&simulation, first).unwrap().bot.dead);

    // A present component with a different wire identity is also invalid,
    // rather than an empty body list or a different target to damage.
    let mut simulation = RoomSimulation::new(fixture_bots()).unwrap();
    simulation.body_mut(first).unwrap().entity = second;
    assert!(
        matches!(simulation.bodies(), Err(CombatError::BotIdentity { wire, actual })
        if wire == first && actual == second)
    );
}

#[test]
fn missing_flight_component_is_explicit_and_retirement_is_idempotent() {
    use crate::mover_settings::MoverCatalog;

    let ground = fixture_ground();
    let movers = MoverCatalog::default();
    let queries = BotQueries::new(false, &movers, &ground, None);
    let mut simulation = RoomSimulation::new(Vec::new()).unwrap();
    let owner = BodyId {
        entity: 0xa000_0101,
        generation: 1,
    };
    simulation
        .advance_projectiles(
            &queries,
            7,
            owner,
            vec![flight_fixture(owner, 1.0)],
            FIXTURE_CONTEXT,
        )
        .unwrap();
    let entity = simulation.projectiles[&7].order[0];
    simulation
        .world
        .entity_mut(entity)
        .remove::<AdmittedFlight>();
    assert!(matches!(
        simulation.advance_projectiles(&queries, 7, owner, Vec::new(), FIXTURE_CONTEXT),
        Err(CombatError::MissingFlight { connection: 7, entity: missing }) if missing == entity
    ));
    simulation.retire_projectiles(7);
    simulation.retire_projectiles(7);
    simulation.clear_projectiles();
    simulation.clear_projectiles();
    assert!(simulation.world.entities().is_empty());
    assert!(simulation.projectiles.is_empty());
}

#[test]
fn stale_generation_does_not_retire_or_advance_the_replacement_flight() {
    use crate::mover_settings::MoverCatalog;

    let ground = fixture_ground();
    let movers = MoverCatalog::default();
    let queries = BotQueries::new(false, &movers, &ground, None);
    let mut simulation = RoomSimulation::new(Vec::new()).unwrap();
    let owner = BodyId {
        entity: 0xa000_0102,
        generation: 2,
    };
    let old = BodyId {
        entity: 0xa000_0101,
        generation: 1,
    };
    simulation
        .advance_projectiles(
            &queries,
            7,
            owner,
            vec![flight_fixture(owner, 1.0)],
            FIXTURE_CONTEXT,
        )
        .unwrap();
    let handle = simulation.projectiles[&7].order[0];
    let later = ShotContext {
        tick: FIXTURE_CONTEXT.tick + 3,
        ..FIXTURE_CONTEXT
    };
    let (outcomes, hits) = simulation
        .advance_projectiles(&queries, 7, old, vec![flight_fixture(old, 1.0)], later)
        .unwrap();
    assert!(outcomes.is_empty() && hits.is_empty());
    assert_eq!(simulation.projectiles[&7].order, [handle]);
    assert_eq!(simulation.world.entities().len(), 1);
    assert!(matches!(
        simulation.advance_projectiles(
            &queries,
            7,
            BodyId {
                entity: old.entity,
                ..owner
            },
            Vec::new(),
            later
        ),
        Err(CombatError::FlightOwnerIdentity { .. })
    ));
    let (outcomes, hits) = simulation
        .advance_projectiles(&queries, 7, owner, Vec::new(), later)
        .unwrap();
    assert!(hits.is_empty());
    assert!(
        matches!(outcomes.as_slice(), [FlightOutcome::Ended { travelled, .. }]
        if (*travelled - 0.04).abs() < 1e-12)
    );
    assert!(simulation.world.entities().is_empty());
}

#[test]
fn duplicate_or_exhausted_bot_identities_are_rejected_at_admission() {
    let mut bots = fixture_bots();
    bots[1].entity = bots[0].entity;
    assert!(matches!(
        RoomSimulation::new(bots),
        Err(CombatError::BotIdInUse(BOT_ENTITY))
    ));
    let mut bots = fixture_bots();
    bots[1].entity = u32::MAX;
    assert!(matches!(
        RoomSimulation::new(bots),
        Err(CombatError::BotIdsExhausted)
    ));
}

#[test]
fn bot_damage_history_and_respawn_follow_identity_after_scene_reorder() {
    use crate::game_hitscan::TargetPose;

    let mut bots = RoomSimulation::new(fixture_bots()).unwrap();
    bots.activate_combat();
    let first = BOT_ENTITY;
    let second = BOT_ENTITY + 1;
    let first_handle = bots.by_wire[&first];
    let second_handle = bots.by_wire[&second];
    let context = FIXTURE_CONTEXT;
    let pose = TargetPose {
        feet: state(&bots, first).unwrap().bot.position,
        height: 1.8,
        radius: 0.5,
        eye: 1.6,
    };
    let second_pose = TargetPose {
        feet: state(&bots, second).unwrap().bot.position,
        ..pose
    };
    bots.state_mut(first)
        .unwrap()
        .unwrap()
        .history
        .record(context.tick, pose);
    bots.state_mut(second)
        .unwrap()
        .unwrap()
        .history
        .record(context.tick, second_pose);

    bots.by_wire.swap_indices(0, 1);
    bots.body_mut(first).unwrap().position = [7.0, 0.0, 0.0];
    let result = bots
        .apply_bot_impact(7, context.tick, context, &fixture_impact(first, 100.0))
        .unwrap()
        .unwrap();
    let due = context.now + BOT_RESPAWN_TIME;
    let view = ViewFrame::from_command(
        &Command {
            frame: context.tick,
            ..Command::default()
        },
        context.quanta,
    );
    assert_eq!(result.health.entity, first);
    assert_eq!(
        result.notices[1],
        Some(CombatNotice::Elimination {
            killer: 7,
            victim: first
        })
    );
    let actors = bots.debug_actors().unwrap().collect::<Vec<_>>();
    assert_eq!(actors[0].entity, second);
    assert!(actors[0].alive);
    assert_eq!(actors[0].health, Some(100.0));
    assert_eq!(actors[1].entity, first);
    assert_eq!(actors[1].position, [7.0, 0.0, 0.0]);
    assert!(!actors[1].alive);
    assert_eq!(actors[1].health, Some(0.0));
    let target = state(&bots, first).unwrap();
    assert_eq!(target.respawn_at, Some(due));
    assert_eq!(target.history.at(view, second_pose), Some(pose));
    let other = state(&bots, second).unwrap();
    assert_eq!(other.respawn_at, None);
    assert_eq!(other.history.at(view, pose), Some(second_pose));

    let replacements = bots.respawn_due(context.tick + 1, due).unwrap();
    let [replacement] = replacements.as_slice() else {
        panic!("expected one due replacement");
    };
    let fresh = replacement.created.entity;
    assert_eq!(replacement.retired, first);
    assert_eq!(fresh, BOT_ENTITY + 2);
    assert_eq!(
        bots.by_wire.keys().copied().collect::<Vec<_>>(),
        [second, fresh]
    );
    assert!(state(&bots, first).is_none());
    assert!(bots.world.get::<BotState>(first_handle).is_none());
    assert_eq!(bots.by_wire[&second], second_handle);
    let target = state(&bots, fresh).unwrap();
    assert!(!target.bot.dead);
    assert_eq!(target.bot.position, pose.feet);
    assert_eq!(target.vitals.total(), 100.0);
    assert_eq!(target.respawn_at, None);
    assert_eq!(target.history.at(view, pose), None);
    assert_eq!(
        state(&bots, second).unwrap().history.at(view, pose),
        Some(second_pose)
    );
    assert!(
        bots.apply_bot_impact(7, context.tick, context, &fixture_impact(first, 100.0))
            .unwrap()
            .is_none()
    );
}

#[test]
fn failed_respawn_batch_preserves_all_due_bodies() {
    let mut bots = RoomSimulation::new(fixture_bots()).unwrap();
    let identities = [BOT_ENTITY, BOT_ENTITY + 1];
    let handles = [bots.by_wire[&identities[0]], bots.by_wire[&identities[1]]];
    let context = FIXTURE_CONTEXT;
    for identity in identities {
        bots.apply_bot_impact(7, context.tick, context, &fixture_impact(identity, 100.0))
            .unwrap();
    }
    let due = context.now + BOT_RESPAWN_TIME;
    let first_free = u32::MAX - 1;
    bots.next_bot = first_free;

    assert!(bots.respawn_due(context.tick + 1, due).is_err());
    assert_eq!(bots.next_bot, first_free);
    assert_eq!(bots.by_wire.keys().copied().collect::<Vec<_>>(), identities);
    for (identity, handle) in identities.into_iter().zip(handles) {
        assert_eq!(bots.by_wire[&identity], handle);
        let target = state(&bots, identity).unwrap();
        assert!(target.bot.dead);
        assert_eq!(target.vitals.total(), 0.0);
        assert_eq!(target.respawn_at, Some(due));
    }
}

#[test]
fn biotic_field_ticks_are_bounded_and_capsule_scoped() {
    let mut field = BioticField::new([0.0; 3]);
    for tick in [100, 111, 112] {
        let (pulse, expired) = field.advance(tick);
        assert!(pulse.is_none() && !expired);
    }
    let (pulse, expired) = field.advance(115);
    assert!(!expired);
    let pulse = pulse.unwrap();
    assert_eq!(pulse.amount, 3.0 * HEAL_PER_TICK);
    let pose = TargetPose {
        feet: [5.5, 0.0, 0.0],
        height: 2.0,
        radius: 0.5,
        eye: 1.55,
    };
    assert!(pulse.reaches(pose));
    assert!(!pulse.reaches(TargetPose {
        feet: [5.501, 0.0, 0.0],
        ..pose
    }));
    assert!(field.advance(115).0.is_none());
    let (tail, expired) = field.advance(900);
    assert!(expired);
    assert!((pulse.amount + tail.unwrap().amount - 312.0 * HEAL_PER_TICK).abs() < 1e-9);
    assert!(field.advance(901).0.is_none());
}

#[test]
#[ignore = "requires a prepared 104319 Practice Range scene"]
fn biotic_field_replicates_healing_only_for_live_allies_in_range() {
    let runtime = std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap();
    let mut scene = SceneAssets::load(runtime)
        .unwrap()
        .prepare(GameMap::practice_range())
        .unwrap();
    let (bots, queries) = scene.bot_simulation();
    let [near, far, dead] = {
        let mut allies = bots
            .bodies()
            .unwrap()
            .filter(|bot| bot.spawn.team == 0)
            .map(|bot| bot.entity);
        std::array::from_fn(|_| allies.next().unwrap())
    };
    let enemy = bots
        .bodies()
        .unwrap()
        .find(|bot| bot.spawn.team == 1)
        .unwrap()
        .entity;
    for (id, position) in [
        (near, [1.0, 0.0, 0.0]),
        (far, [7.0, 0.0, 0.0]),
        (dead, [1.0, 0.0, 0.0]),
        (enemy, [1.0, 0.0, 0.0]),
    ] {
        let state = bots.state_mut(id).unwrap().unwrap();
        state.bot.position = position;
        state.vitals.damage(
            if id == dead { 1_000.0 } else { 20.0 },
            Death {
                frame: 100,
                time: 1.6,
                killer: 1,
            },
        );
    }
    bots.body_mut(dead).unwrap().died(100);
    let mut field = BioticField::new([0.0; 3]);
    field.advance(100);
    let pulse = field.advance(117).0.unwrap();
    bots.by_wire.swap_indices(
        bots.by_wire.get_index_of(&near).unwrap(),
        bots.by_wire.get_index_of(&enemy).unwrap(),
    );
    let updates = bots.heal_bots(&queries, 0, &[pulse]).unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].entity, near);
    let near = state(bots, near).unwrap();
    let far = state(bots, far).unwrap();
    let enemy = state(bots, enemy).unwrap();
    assert_eq!(
        near.vitals.total(),
        near.bot.spawn.health - 20.0 + pulse.amount
    );
    assert_eq!(far.vitals.total(), far.bot.spawn.health - 20.0);
    assert_eq!(enemy.vitals.total(), enemy.bot.spawn.health - 20.0);
    assert_eq!(state(bots, dead).unwrap().vitals.total(), 0.0);
}

#[test]
fn respawn_encoding_failure_preserves_its_source_and_the_entire_batch() {
    let mut bots = fixture_bots();
    std::sync::Arc::get_mut(&mut bots[1].spawn).unwrap().team = u8::MAX;
    let mut simulation = RoomSimulation::new(bots).unwrap();
    let before = simulation.by_wire.clone();
    let next_bot = simulation.next_bot;
    let context = FIXTURE_CONTEXT;
    for identity in [BOT_ENTITY, BOT_ENTITY + 1] {
        simulation
            .apply_bot_impact(7, context.tick, context, &fixture_impact(identity, 100.0))
            .unwrap();
    }
    let error = simulation
        .respawn_due(context.tick + 1, context.now + BOT_RESPAWN_TIME)
        .err()
        .unwrap();
    assert!(matches!(&error, CombatError::RespawnCreate { wire, .. } if *wire == next_bot + 1));
    assert!(std::error::Error::source(&error).unwrap().is::<io::Error>());
    assert_eq!(simulation.by_wire, before);
    assert_eq!(simulation.next_bot, next_bot);
    assert!(simulation.bodies().unwrap().all(|bot| bot.dead));
}

#[test]
#[ignore = "requires a prepared 104319 Practice Range scene"]
fn repeated_rifle_hits_retire_and_respawn_only_the_hit_bot() {
    let runtime = std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap();
    let mut scene = SceneAssets::load(runtime)
        .unwrap()
        .prepare(GameMap::practice_range())
        .unwrap();
    let (bots, queries) = scene.bot_simulation();
    let slot = bots
        .bodies()
        .unwrap()
        .position(|bot| bot.spawn.team == 1)
        .unwrap();
    let bot = bots.bodies().unwrap().nth(slot).unwrap();
    let entity = bot.entity;
    let [x, y, z] = bot.position;
    let count = bots.bodies().unwrap().len();
    let mut context = ShotContext {
        team: 0,
        quanta: RETAIL_QUANTA,
        tick: 1000,
        now: Duration::from_secs(4),
    };
    // Before combat activation, moving bodies must not seed rewind history.
    let cold_frame = context.tick - 1;
    let mut fallback = queries.target(&state(bots, entity).unwrap().bot);
    fallback.feet[0] += 1.0;
    let view = ViewFrame::from_command(
        &Command {
            frame: cold_frame,
            ..Command::default()
        },
        context.quanta,
    );
    assert!(
        bots.finish_tick(&queries, cold_frame, context.now)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        state(bots, entity).unwrap().history.at(view, fallback),
        Some(fallback)
    );
    bots.activate_combat();
    bots.finish_tick(&queries, context.tick, context.now)
        .unwrap();
    let mut shot = PosedHitscan {
        source: 0xa000_0101,
        origin: [x - 8.0, y + eye_height(SOLDIER, false), z],
        direction: [1.0, 0.0, 0.0],
        command: Command {
            frame: context.tick,
            yaw: 16384,
            ..Command::default()
        },
        shot_frame: 999,
        reach: 180.0,
        hit: HitDefinition {
            tags: crate::game_hitscan::DamageTags::EMPTY,
            amount: 20.0,
            loadout: None,
            critical: 2.0,
            falloff: None,
        },
    };
    assert!(
        bots.hitscan(&queries, shot, context, |_| false)
            .unwrap()
            .is_none()
    );
    assert!(
        bots.hitscan(&queries, shot, ShotContext { team: 1, ..context }, |_| {
            true
        })
        .unwrap()
        .is_none()
    );
    for index in 0..5 {
        let result = bots
            .hitscan(&queries, shot, context, |_| true)
            .unwrap()
            .unwrap();
        let terminal = index == 4;
        assert_eq!(result.health.entity, entity);
        assert_eq!(
            result.notices[0],
            Some(CombatNotice::Hit {
                target: entity,
                dealt: 40.0,
                critical: true,
                killed: terminal,
                shot_frame: shot.shot_frame,
            })
        );
        assert_eq!(
            result.notices[1],
            terminal.then_some(CombatNotice::Elimination {
                killer: shot.source,
                victim: entity,
            })
        );
        assert_eq!(
            state(bots, entity).unwrap().vitals.total(),
            160.0 - f64::from(index) * 40.0
        );
        assert_eq!(bots.bodies().unwrap().nth(slot).unwrap().dead, terminal);
    }
    assert!(
        bots.hitscan(&queries, shot, context, |_| true)
            .unwrap()
            .is_none()
    );
    let due = context.now + BOT_RESPAWN_TIME;
    assert!(
        bots.finish_tick(&queries, 1187, due - Duration::from_millis(1))
            .unwrap()
            .is_empty()
    );
    let [replacement] = bots
        .finish_tick(&queries, 1188, due)
        .unwrap()
        .try_into()
        .ok()
        .unwrap();
    assert_eq!(replacement.retired, entity);
    let next = BOT_ENTITY + count as u32;
    assert_eq!(replacement.created.entity, next);
    assert!(matches!(
        replacement.created.operation,
        Some(Operation::Create(_))
    ));
    assert!(!bots.bodies().unwrap().nth(slot).unwrap().dead);
    assert_eq!(state(bots, next).unwrap().vitals.total(), 200.0);
    context.tick = 1188;
    context.now = due;
    assert!(
        bots.hitscan(&queries, shot, context, |_| true)
            .unwrap()
            .is_none()
    );
    shot.command.frame = context.tick;
    assert!(
        bots.hitscan(&queries, shot, context, |visible| visible == next)
            .unwrap()
            .is_some()
    );
    assert_eq!(state(bots, next).unwrap().vitals.total(), 160.0);
}

#[test]
#[ignore = "requires separately prepared 104319 Junkenstein runtime"]
fn normal_common_enemy_death_updates_authored_team_score() {
    junkenstein::normal_common_enemy_death_updates_authored_team_score();
}

#[test]
#[ignore = "requires separately prepared 104319 Junkenstein runtime"]
fn normal_encounter_first_reaper_finishes_intro_and_pursues_live_defender() {
    junkenstein::normal_encounter_first_reaper_finishes_intro_and_pursues_live_defender();
}
