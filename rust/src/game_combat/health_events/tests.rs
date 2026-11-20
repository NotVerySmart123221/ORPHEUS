use serde::Deserialize;
use std::sync::Arc;

use super::{BotState, HitTags, HostEvent, PlayerBodyState, RoomSimulation};
use crate::{
    contact_world::ContactWorld,
    game_combat::{
        HealthEffect,
        tests::{FIXTURE_CONTEXT, fixture_bots, fixture_impact},
    },
    game_heroes::SOLDIER,
    game_replication::BodyId,
    mover::Mover,
    mover_settings::MoverSettings,
    script_entity_state::EntityScriptState,
    script_init::{HealthTargetFacts, OWNER_HEALTH},
    script_player::fixture_body_factory,
    script_wire::Value,
    vitals::Death,
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
}

#[derive(Deserialize)]
struct Expected {
    ordered_outcomes: Vec<Outcome>,
    bot_health: f64,
    player_health: f64,
    replacement: Replacement,
}

#[derive(Deserialize)]
struct Outcome {
    target: Target,
    delta: f64,
    killed: bool,
    hit_tags: Option<Vec<u64>>,
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
    let bot_hit = room
        .apply_bot_impact(source, FIXTURE_CONTEXT.tick, FIXTURE_CONTEXT, &bot_impact)
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
            FIXTURE_CONTEXT.tick,
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
        let (target, impact_facts) = match outcome.target {
            Target::Bot => (bot, &bot_hit.damage.target_facts),
            Target::Player => (player.entity, &player_hit.damage.target_facts),
        };
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
fn lethal_impact_notifies_source_with_settled_room_status_after_target_retirement() {
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
    }

    let Contract { input, expected } = contract();
    let ground = Arc::new(fixture_ground());
    let movers = MoverCatalog::default();
    for (room_mode, expected_status) in [
        (Some(false), Some((Some(1), false))),
        (Some(true), Some((None, false))),
        (None, None),
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

        let impact = fixture_impact(target, input.bot_lethal_impact);
        room.apply_bot_impact(
            source.entity,
            FIXTURE_CONTEXT.tick,
            FIXTURE_CONTEXT,
            &impact,
        )
        .unwrap()
        .unwrap();
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
                hit_tags: Some(HitTags::EMPTY),
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
