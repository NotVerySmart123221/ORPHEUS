use serde::Deserialize;
use std::sync::Arc;

use super::{BotState, HitTags, HostEvent, PlayerBodyState, RoomSimulation};
use crate::{
    contact_world::ContactWorld,
    game_combat::{
        HealthEffect, RoomScriptRequests,
        tests::{FIXTURE_CONTEXT, fixture_bots, fixture_ground, fixture_impact},
    },
    game_heroes::SOLDIER,
    game_replication::BodyId,
    mover::Mover,
    mover_settings::MoverSettings,
    script_entity_state::EntityScriptState,
    script_init::{HealthTargetFacts, OWNER_HEALTH},
    script_player::fixture_body_factory,
    script_wire::Value,
    vitals::{Death, NamedPoolLoss},
};

#[derive(Deserialize)]
struct Contract {
    input: Input,
    expected: Expected,
}

#[derive(Deserialize)]
struct Input {
    bot_impact: f64,
    player_impact: f64,
    bot_lethal_impact: f64,
    ordered_hit_tags: Vec<u64>,
    unavailable_target_tag: u64,
    generic_healing: f64,
    generic_damage: f64,
    named_damage: NamedDamage,
}

#[derive(Deserialize)]
struct NamedDamage {
    grants: Vec<(u64, f64)>,
    weapon_damage: f64,
    scripted_damage: f64,
    heal: f64,
    ordinary_damage: f64,
    expected_losses: Vec<Option<Vec<(u64, f64)>>>,
}

#[derive(Deserialize)]
struct Expected {
    ordered_outcomes: Vec<Outcome>,
    bot_health: f64,
    player_health: f64,
    health_after_stale_request: f64,
    health_after_current_dead_source_request: f64,
    replacement: Replacement,
}

#[derive(Deserialize)]
struct Outcome {
    target: Target,
    delta: f64,
    killed: bool,
    hit_tags: Option<Vec<u64>>,
    critical_hit: Option<bool>,
    target_facts: Option<TargetFacts>,
}

#[derive(Deserialize)]
struct TargetFacts {
    filter_flags: u64,
    owner_health: Option<f64>,
}

#[derive(Deserialize)]
struct Replacement {
    heal_delta: f64,
    heal_target_facts: TargetFacts,
    lethal_delta: f64,
    target_facts: TargetFacts,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Target {
    Bot,
    Player,
}

fn contract() -> Contract {
    serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/script-settled-hit-tags-v1.json"
    ))
    .unwrap()
}

fn hit_tags(values: &[u64]) -> HitTags {
    let mut tags = HitTags::EMPTY;
    for &value in values {
        assert!(tags.push(value));
    }
    tags
}

fn assert_target_facts(
    facts: &HealthTargetFacts,
    expected: &TargetFacts,
    target: u32,
    unavailable_tag: u64,
) {
    assert_eq!(facts.target, target);
    assert_eq!(facts.filter_flags, Some(expected.filter_flags));
    assert_eq!(facts.tags.presence(unavailable_tag), None);
    match (&facts.script_state, expected.owner_health) {
        (EntityScriptState::Absent, None) => (),
        (EntityScriptState::Running(variables), Some(health)) => {
            assert_eq!(variables.get(&OWNER_HEALTH), Some(&Value::Float(health)));
        }
        (state, health) => {
            panic!("unexpected script state {state:?} for expected health {health:?}")
        }
    }
}

fn spawn_player(room: &mut RoomSimulation, generation: u64) -> BodyId {
    let (factory, spawn) = fixture_body_factory();
    let mover = Mover::new(
        MoverSettings::default(),
        Arc::new(ContactWorld::new(Vec::new()).unwrap()),
        spawn.pose.position,
        0,
        0x08,
    )
    .unwrap();
    let id = BodyId {
        entity: spawn.entity,
        generation,
    };
    room.spawn_player_body(PlayerBodyState::new(
        id,
        spawn.controller,
        spawn.slot,
        0,
        SOLDIER,
        mover,
        Some(Box::new(factory.start(spawn).unwrap())),
        spawn.frame,
        None,
    ))
    .unwrap();
    id
}

#[test]
fn authored_player_death_preserves_attribution_without_advancing_input() {
    use crate::script_player::fixture_body_factory_with_nodes;
    use serde_json::{Value as Json, json};

    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/player-authored-death-v1.json"
    ))
    .unwrap();
    let params: Vec<_> = fixture["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|binding| {
            json!({
                "m_B5051BCE": binding["parameter"],
                "m_out_Var": {
                    "$": "STUConfigVarDynamic",
                    "m_60DB8F99": 1,
                    "m_identifier": binding["variable"]
                }
            })
        })
        .collect();
    let entry = json!({
        "pos":0, "uid":0, "rid":0, "remote":-1, "state":null,
        "class":"STUStatescriptEntryGameMessage",
        "client_only":false, "server_only":false,
        "fields":{"m_gameMessage":{"m_gameMessage":fixture["event"]}, "m_params":params}
    });
    let bots = fixture_bots();
    let killer = bots[0].entity;
    let mut room = RoomSimulation::new(bots).unwrap();
    let (factory, spawn) = fixture_body_factory_with_nodes(vec![entry]);
    let id = BodyId {
        entity: spawn.entity,
        generation: 1,
    };
    let mover = Mover::new(
        MoverSettings::default(),
        Arc::new(ContactWorld::new(Vec::new()).unwrap()),
        spawn.pose.position,
        0,
        0x08,
    )
    .unwrap();
    let runtime = factory.start(spawn).unwrap();
    let health = crate::game_replication::OwnerBodyRuntime::vitals(&runtime)
        .unwrap()
        .total();
    room.spawn_player_body(PlayerBodyState::new(
        id,
        spawn.controller,
        spawn.slot,
        0,
        SOLDIER,
        mover,
        Some(Box::new(runtime)),
        spawn.frame,
        None,
    ))
    .unwrap();
    let death = Death {
        time: 2.0,
        frame: spawn.frame + 12,
        killer,
    };
    let facts = room.player_body(id).unwrap().health_target_facts();
    let mut impact = fixture_impact(id.entity, health);
    impact.critical = true;
    let expected_loadout = 0x0b90_0000_0000_0041;
    impact.loadout = Some(expected_loadout);
    let (_, wounded, _) = room
        .damage_player_body_by_wire(id, health / 2.0, death, None)
        .unwrap();
    assert!(!wounded.killed);
    assert!(
        !room
            .player_body(id)
            .unwrap()
            .projection()
            .unwrap()
            .snapshot
            .variables
            .contains_key(&101)
    );
    let (_, killed, _) = room
        .damage_player_body_by_wire(id, health, death, Some((&impact, &facts)))
        .unwrap();
    assert!(killed.killed);
    let state = room.player_body(id).unwrap();
    let death_snapshot = state.projection().unwrap().snapshot;
    let variables = &death_snapshot.variables;
    assert_eq!(variables.get(&101), Some(&Value::Entity(killer)));
    assert_eq!(variables.get(&102), Some(&Value::Bool(true)));
    assert_eq!(variables.get(&103), Some(&Value::Asset(expected_loadout)));
    assert_eq!(variables.get(&104), Some(&Value::Array(Vec::new())));
    assert_eq!(state.frame, spawn.frame);

    let other = Death { killer: 0, ..death };
    let (_, unchanged, _) = room
        .damage_player_body_by_wire(id, health, other, None)
        .unwrap();
    assert!(!unchanged.killed);
    assert_eq!(
        room.player_body(id)
            .unwrap()
            .projection()
            .unwrap()
            .snapshot
            .variables
            .get(&101),
        Some(&Value::Entity(killer)),
    );
}

#[test]
#[ignore = "requires a prepared 104319 stock owner graph and codec catalog"]
fn prepared_npc_kill_projects_the_player_death_state() {
    use crate::{
        game_clock::FIRST_TICK, game_content::GameMap, game_instance::RETAIL_QUANTA,
        match_scene::team_bit, scene_assets::SceneAssets, script_player::OwnerBodySpawn,
        script_snapshot_wire::View,
    };
    let path = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let assets = SceneAssets::load(path).unwrap();
    let mut scene = assets.prepare(GameMap::practice_range()).unwrap();
    let hero = assets.heroes().get(SOLDIER).unwrap();
    let mover = scene.player_spawn_mover(hero.body, 0).unwrap();
    let spawn = OwnerBodySpawn {
        slot: 0,
        controller: 0xa000_0100,
        entity: 0xa000_0101,
        body_filter: team_bit(0).unwrap(),
        frame: FIRST_TICK,
        pose: mover.pose(),
        eye_position: crate::game_combat::script_eye_position(hero.guid, &mover),
        skin: 0,
        quanta: RETAIL_QUANTA,
    };
    let runtime = scene.owner_body_runtime(hero, spawn).unwrap().unwrap();
    let id = BodyId {
        entity: spawn.entity,
        generation: 1,
    };
    let room = scene.simulation_mut();
    let killer = room
        .bodies()
        .unwrap()
        .find(|bot| bot.spawn.team == 1)
        .unwrap()
        .entity;
    room.spawn_player_body(PlayerBodyState::new(
        id,
        spawn.controller,
        spawn.slot,
        0,
        hero.guid,
        mover,
        Some(runtime),
        FIRST_TICK,
        None,
    ))
    .unwrap();
    assert!(
        room.settle_health(
            id.entity,
            HealthEffect::Damage(f64::MAX),
            Death {
                time: 0.16,
                frame: FIRST_TICK + 10,
                killer
            },
            std::time::Duration::from_millis(160),
        )
        .unwrap()
    );
    let state = room.player_body(id).unwrap();
    assert_eq!(state.frame, FIRST_TICK);
    let projection = state
        .projection()
        .expect("player death retained the StateScript runtime");
    let (&instance_id, instance) = projection
        .snapshot
        .instances
        .iter()
        .find(|(_, instance)| instance.graph == 0x01cf)
        .unwrap();
    assert_eq!(instance.variables.get(&392), Some(&Value::Entity(killer)));
    let observer = projection
        .encoder
        .project(projection.snapshot, View::Observer)
        .unwrap();
    let states = observer.instance_states(instance_id).unwrap();
    assert!(states.contains(&108), "observer states: {states:?}");
    projection.encoder.full(&observer).unwrap();
    let owner = projection
        .encoder
        .project(projection.snapshot, View::Owner)
        .unwrap();
    projection.encoder.full(&owner).unwrap();
}

#[test]
fn admitted_health_request_cannot_credit_a_replacement_source() {
    let Contract { input, expected } = contract();
    let bots = fixture_bots();
    let source = bots[0].entity;
    let target = bots[1].entity;
    let mut room = RoomSimulation::new(bots).unwrap();
    room.enable_host_health_events();
    room.enable_spatial_queries(
        false,
        Arc::new(fixture_ground()),
        crate::script_graph::RoomSpatialNeeds::Queries,
    );
    let tick = FIXTURE_CONTEXT.tick;
    let now = FIXTURE_CONTEXT.now;
    room.prepare_spatial_frame(tick).unwrap();
    let queue_damage = |room: &RoomSimulation| {
        let mut requests = RoomScriptRequests::default();
        let frame = &room.spatial_frame.as_ref().unwrap().1;
        assert!(requests.queue_health(frame, target, -input.generic_damage));
        requests.take().pop().unwrap()
    };
    let previous = room.sample_health_body(source).unwrap();
    let stale = queue_damage(&room);
    let body = room
        .world
        .entity_mut(previous.entity)
        .take::<BotState>()
        .unwrap();
    assert!(room.world.despawn(previous.entity));
    let replacement = room.world.spawn(body).id();
    assert_ne!(replacement, previous.entity);
    assert_eq!(
        room.by_wire.insert(source, replacement),
        Some(previous.entity)
    );

    room.settle_script_request(stale, previous, tick, now)
        .unwrap();
    assert!(room.health_events.pending.is_empty());
    assert!(room.take_scripted_health().is_empty());
    assert_eq!(
        room.body_healths().unwrap()[&target],
        expected.health_after_stale_request
    );

    let current = room.sample_health_body(source).unwrap();
    let queued_before_death = queue_damage(&room);
    let dealer = Death {
        killer: target,
        frame: tick,
        time: now.as_secs_f64(),
    };
    room.settle_health(
        source,
        HealthEffect::Damage(input.bot_lethal_impact),
        dealer,
        now,
    )
    .unwrap();
    assert!(room.observed_bot_death(source).unwrap().is_some());
    room.flush_health_events();

    room.settle_script_request(queued_before_death, current, tick, now)
        .unwrap();
    assert_eq!(
        room.body_healths().unwrap()[&target],
        expected.health_after_current_dead_source_request
    );
    let change = room.health_events.pending.pop_front().unwrap();
    assert!(change.source == Some(current));
    assert_eq!(change.target.wire, target);
    assert_eq!(change.delta, -input.generic_damage);
    assert!(room.health_events.pending.is_empty());
}

#[test]
fn self_impact_updates_health_without_shooter_hit_or_elimination_feedback() {
    let mut room = RoomSimulation::new(Vec::new()).unwrap();
    let player = spawn_player(&mut room, 1);
    let health =
        |room: &RoomSimulation| room.player_body(player).unwrap().vitals().unwrap().total();
    let starting = health(&room);
    let context = FIXTURE_CONTEXT;
    for amount in [1.0, starting] {
        let impact = fixture_impact(player.entity, amount);
        assert!(
            room.apply_player_impact(player.entity, context.tick, context, &impact)
                .unwrap()
                .is_none()
        );
        assert_eq!(room.take_scripted_health()[0].entity, player.entity);
    }
    assert_eq!(health(&room), 0.0);
    let [death]: [_; 1] = room.take_player_deaths().try_into().ok().unwrap();
    assert_eq!(death.body, player);
    assert_eq!(death.source, player.entity);
    assert!(!death.killed_by);
}

#[test]
fn native_room_health_events_keep_actual_deltas_and_hit_tag_availability_in_order() {
    let Contract { input, expected } = contract();
    let bots = fixture_bots();
    let source = bots[0].entity;
    let bot = bots[1].entity;
    let mut room = RoomSimulation::new(bots).unwrap();
    let player = spawn_player(&mut room, 1);
    room.enable_host_health_events();
    let now = FIXTURE_CONTEXT.now;
    let dealer = Death {
        killer: source,
        frame: FIXTURE_CONTEXT.tick,
        time: now.as_secs_f64(),
    };

    let mut bot_impact = fixture_impact(bot, input.bot_impact);
    bot_impact.tags = hit_tags(&input.ordered_hit_tags);
    bot_impact.critical = true;
    let bot_hit = room
        .apply_bot_impact(
            source,
            FIXTURE_CONTEXT.tick - 4,
            FIXTURE_CONTEXT,
            &bot_impact,
        )
        .unwrap()
        .unwrap();

    for effect in [
        HealthEffect::Damage(input.generic_damage),
        HealthEffect::Heal(input.generic_healing),
        HealthEffect::Heal(input.generic_healing),
    ] {
        assert!(
            room.settle_health(player.entity, effect, dealer, now)
                .unwrap()
        );
    }

    let player_impact = fixture_impact(player.entity, input.player_impact);
    let player_hit = room
        .apply_player_impact(
            source,
            FIXTURE_CONTEXT.tick - 4,
            FIXTURE_CONTEXT,
            &player_impact,
        )
        .unwrap()
        .unwrap();
    let applied_player_damage = -expected
        .ordered_outcomes
        .iter()
        .find(|outcome| matches!(outcome.target, Target::Player) && outcome.hit_tags.is_some())
        .unwrap()
        .delta;
    assert_eq!(player_hit.damage.amount, applied_player_damage);
    assert!(matches!(
        &player_hit.notices[0],
        Some(crate::game_combat_messages::CombatNotice::Hit { dealt, killed: true, .. })
            if *dealt == applied_player_damage
    ));

    assert!(
        room.settle_health(
            player.entity,
            HealthEffect::Heal(input.generic_healing),
            dealer,
            now
        )
        .unwrap()
    );
    for effect in [
        HealthEffect::Heal(input.generic_healing),
        HealthEffect::Heal(input.generic_healing),
        HealthEffect::Damage(input.generic_damage),
    ] {
        assert!(room.settle_health(bot, effect, dealer, now).unwrap());
    }

    let changes = std::mem::take(&mut room.health_events.pending);
    assert_eq!(changes.len(), expected.ordered_outcomes.len());
    for (change, outcome) in changes.into_iter().zip(expected.ordered_outcomes) {
        // Every damage on this fixture's ordinary pools reports a known empty
        // list. Healing has no damage-loss records.
        assert_eq!(
            change.named_pool_losses.as_deref(),
            (outcome.delta < 0.0).then_some(&[][..])
        );
        let (target, impact_facts) = match outcome.target {
            Target::Bot => (bot, &bot_hit.damage.target_facts),
            Target::Player => (player.entity, &player_hit.damage.target_facts),
        };
        assert_eq!(change.critical_hit, outcome.critical_hit);
        assert_eq!(
            change.damage_frame,
            (outcome.delta < 0.0).then_some(FIXTURE_CONTEXT.tick)
        );
        if let Some(expected_facts) = &outcome.target_facts {
            if outcome.hit_tags.is_some() {
                assert_target_facts(
                    impact_facts,
                    expected_facts,
                    target,
                    input.unavailable_target_tag,
                );
            }
            assert_target_facts(
                change.target_facts.as_deref().unwrap(),
                expected_facts,
                target,
                input.unavailable_target_tag,
            );
        } else {
            assert!(change.target_facts.is_none());
        }
        let (event, receivers) = change.resolve(&room).unwrap();
        assert_eq!(
            event,
            HostEvent::HealthApplied {
                source: Some(source),
                target,
                delta: outcome.delta,
                killed: outcome.killed,
                hit_tags: outcome.hit_tags.as_deref().map(hit_tags),
            }
        );
        assert_eq!(
            receivers.map(|body| body.map(|body| body.wire)),
            [Some(source), Some(target)]
        );
    }
    assert_eq!(room.body_healths().unwrap()[&bot], expected.bot_health);
    let player = room.player_body(player).unwrap();
    assert_eq!(
        player.runtime.as_ref().unwrap().vitals().unwrap().total(),
        expected.player_health
    );
}

#[test]
fn room_events_carry_actual_named_shield_losses_for_weapon_and_direct_damage() {
    let input = contract().input.named_damage;
    let bots = fixture_bots();
    let source = bots[0].entity;
    let target = bots[1].entity;
    let mut room = RoomSimulation::new(bots).unwrap();
    room.enable_host_health_events();
    let bot = room.state_mut(target).unwrap().unwrap();
    for &(identifier, amount) in &input.grants {
        assert!(
            bot.vitals
                .add_temporary_shield(identifier, amount, true)
                .is_some()
        );
    }
    let impact = fixture_impact(target, input.weapon_damage);
    let hit = room
        .apply_bot_impact(source, FIXTURE_CONTEXT.tick, FIXTURE_CONTEXT, &impact)
        .unwrap()
        .unwrap();
    let dealer = Death {
        killer: source,
        frame: FIXTURE_CONTEXT.tick,
        time: FIXTURE_CONTEXT.now.as_secs_f64(),
    };
    for effect in [
        HealthEffect::Damage(input.scripted_damage),
        HealthEffect::Heal(input.heal),
        HealthEffect::Damage(input.ordinary_damage),
    ] {
        assert!(
            room.settle_health(target, effect, dealer, FIXTURE_CONTEXT.now)
                .unwrap()
        );
    }

    assert_eq!(
        room.health_events.pending.len(),
        input.expected_losses.len()
    );
    assert_eq!(
        room.health_events.pending[0].named_pool_losses.as_deref(),
        Some(hit.damage.named_pool_losses.as_slice())
    );
    for (change, expected) in room.health_events.pending.iter().zip(input.expected_losses) {
        let expected = expected.map(|losses| {
            losses
                .into_iter()
                .map(|(identifier, amount)| NamedPoolLoss { identifier, amount })
                .collect::<Vec<_>>()
        });
        assert_eq!(change.named_pool_losses.as_deref(), expected.as_deref());
    }
}

#[test]
fn settled_health_facts_stay_with_the_original_target_generation() {
    let Contract { input, expected } = contract();
    let expected = expected.replacement;
    let bots = fixture_bots();
    let target = bots[0].entity;
    let mut room = RoomSimulation::new(bots).unwrap();
    let source = spawn_player(&mut room, 1);
    let now = FIXTURE_CONTEXT.now;
    let dealer = Death {
        killer: source.entity,
        frame: FIXTURE_CONTEXT.tick,
        time: now.as_secs_f64(),
    };
    assert!(
        room.settle_health(
            target,
            HealthEffect::Damage(input.generic_damage),
            dealer,
            now
        )
        .unwrap()
    );
    assert!(room.health_events.pending.is_empty());
    room.enable_host_health_events();
    assert!(
        room.settle_health(
            target,
            HealthEffect::Heal(input.generic_healing),
            dealer,
            now
        )
        .unwrap()
    );

    let mut impact = fixture_impact(target, input.bot_lethal_impact);
    let tags = hit_tags(&input.ordered_hit_tags);
    impact.tags = tags;
    room.apply_bot_impact(
        source.entity,
        FIXTURE_CONTEXT.tick,
        FIXTURE_CONTEXT,
        &impact,
    )
    .unwrap()
    .unwrap();
    let heal = room.health_events.pending.pop_front().unwrap();
    let hit = room.health_events.pending.pop_front().unwrap();
    assert_eq!(spawn_player(&mut room, 2).entity, source.entity);
    for (change, delta, killed, hit_tags, facts) in [
        (
            &heal,
            expected.heal_delta,
            false,
            None,
            &expected.heal_target_facts,
        ),
        (
            &hit,
            expected.lethal_delta,
            true,
            Some(tags),
            &expected.target_facts,
        ),
    ] {
        let (event, receivers) = change.resolve(&room).unwrap();
        assert_target_facts(
            change.target_facts.as_deref().unwrap(),
            facts,
            target,
            input.unavailable_target_tag,
        );
        assert_eq!(
            event,
            HostEvent::HealthApplied {
                source: None,
                target,
                delta,
                killed,
                hit_tags,
            }
        );
        assert_eq!(
            receivers.map(|body| body.map(|body| body.wire)),
            [None, Some(target)]
        );
    }

    let previous = room.by_wire[&target];
    let body = room.world.entity_mut(previous).take::<BotState>().unwrap();
    assert!(room.world.despawn(previous));
    let replacement = room.world.spawn(body).id();
    assert_ne!(replacement, previous);
    assert_eq!(room.by_wire.insert(target, replacement), Some(previous));
    assert!(heal.resolve(&room).is_none());
    assert!(hit.resolve(&room).is_none());
    assert!(room.health_events.pending.is_empty());
}

#[test]
fn lethal_health_change_notifies_source_with_settled_status_after_target_retirement() {
    use std::{io, sync::Mutex};

    use crate::{
        commands::Command,
        game_bots::BOT_RESPAWN_TIME,
        game_combat::tests::fixture_ground,
        game_replication::{BodyStep, OwnerBodyRuntime},
        mover_settings::MoverCatalog,
        scene_assets::BotQueries,
        script_graph::RoomSpatialNeeds,
        script_init::{HostHealthContext, InitFault, Snapshot},
        script_snapshot_wire::SnapshotEncoder,
    };

    type Notification = (HostEvent, Option<Arc<HealthTargetFacts>>);

    struct HealthListener {
        runtime: Box<dyn OwnerBodyRuntime>,
        notification: Arc<Mutex<Option<Notification>>>,
    }

    impl OwnerBodyRuntime for HealthListener {
        fn receive_host_event(
            &mut self,
            event: HostEvent,
            context: HostHealthContext,
        ) -> Result<usize, InitFault> {
            assert!(
                self.notification
                    .lock()
                    .unwrap()
                    .replace((event, context.target_facts))
                    .is_none()
            );
            Ok(1)
        }

        fn snapshot(&self) -> Option<&Snapshot> {
            self.runtime.snapshot()
        }

        fn encoder(&self) -> SnapshotEncoder<'_> {
            self.runtime.encoder()
        }

        fn advance(&mut self, command: Command, mover: &Mover, step: BodyStep) -> io::Result<()> {
            self.runtime.advance(command, mover, step)
        }

        fn frame_ms(&self) -> u32 {
            self.runtime.frame_ms()
        }

        fn vitals(&self) -> Option<&crate::vitals::Vitals> {
            self.runtime.vitals()
        }

        fn take_vitals(&mut self) -> Option<crate::vitals::Vitals> {
            self.runtime.take_vitals()
        }
    }

    let Contract { input, expected } = contract();
    let ground = Arc::new(fixture_ground());
    let movers = MoverCatalog::default();
    for (room_mode, expected_status, weapon_impact) in [
        (Some(false), Some((Some(1), false)), true),
        (Some(true), Some((None, false)), true),
        (None, None, true),
        (Some(false), Some((Some(1), false)), false),
    ] {
        let bots = fixture_bots();
        let target = bots[0].entity;
        let mut room = RoomSimulation::new(bots).unwrap();
        let source = spawn_player(&mut room, 1);
        let notification = Arc::new(Mutex::new(None));
        let state = room.player_body_mut(source).unwrap();
        let runtime = state.runtime.take().unwrap();
        state.runtime = Some(Box::new(HealthListener {
            runtime,
            notification: Arc::clone(&notification),
        }));
        if let Some(free_for_all) = room_mode {
            room.enable_spatial_queries(
                free_for_all,
                Arc::clone(&ground),
                RoomSpatialNeeds::EntityFacts,
            );
        }
        room.activate_combat();
        room.enable_host_health_events();
        room.prepare_spatial_frame(FIXTURE_CONTEXT.tick).unwrap();

        if weapon_impact {
            let impact = fixture_impact(target, input.bot_lethal_impact);
            room.apply_bot_impact(
                source.entity,
                FIXTURE_CONTEXT.tick,
                FIXTURE_CONTEXT,
                &impact,
            )
            .unwrap()
            .unwrap();
        } else {
            assert!(
                room.settle_health(
                    target,
                    HealthEffect::Damage(input.bot_lethal_impact),
                    Death {
                        killer: source.entity,
                        frame: FIXTURE_CONTEXT.tick,
                        time: FIXTURE_CONTEXT.now.as_secs_f64(),
                    },
                    FIXTURE_CONTEXT.now,
                )
                .unwrap()
            );
        }
        assert!(notification.lock().unwrap().is_none());

        let queries = BotQueries::new(room_mode.unwrap_or(false), &movers, &ground, None);
        let replacements = room
            .finish_tick(
                &queries,
                FIXTURE_CONTEXT.tick + 1,
                FIXTURE_CONTEXT.now + BOT_RESPAWN_TIME,
            )
            .unwrap();
        assert!(
            replacements
                .iter()
                .any(|replacement| replacement.retired == target)
        );
        assert!(room.sample_health_body(target).is_none());
        if let Some(frame) = room
            .capture_spatial_frame(FIXTURE_CONTEXT.tick + 2)
            .unwrap()
        {
            let entities = frame.entity_facts().unwrap();
            assert!(entities.contains_key(&source.entity));
            assert!(!entities.contains_key(&target));
        }

        let (event, facts) = notification.lock().unwrap().take().unwrap();
        assert_eq!(
            event,
            HostEvent::HealthApplied {
                source: Some(source.entity),
                target,
                delta: expected.replacement.lethal_delta,
                killed: true,
                hit_tags: weapon_impact.then_some(HitTags::EMPTY),
            }
        );
        let facts = facts.unwrap();
        assert_eq!(facts.target, target);
        assert_eq!(
            facts
                .room_status
                .as_ref()
                .map(|status| (status.team, status.alive)),
            expected_status
        );
    }
}
