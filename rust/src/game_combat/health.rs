//! Room-authoritative health effects for player and NPC bodies.

use super::{CombatError, HealthDamageEvidence, RoomSimulation, player::PlayerDeath};
use crate::{
    script_expr::valid_entity,
    vitals::Death,
    world::{EntityUpdate, Operation},
};
use std::time::Duration;

pub(super) enum HealthEffect {
    Damage(f64),
    Heal(f64),
}

impl HealthEffect {
    pub(super) fn from_delta(delta: f64) -> Option<Self> {
        if !delta.is_finite() || delta == 0.0 {
            None
        } else if delta < 0.0 {
            Some(Self::Damage(-delta))
        } else {
            Some(Self::Heal(delta))
        }
    }
}

impl RoomSimulation {
    /// Return whether this room owns the target, including a dead or immune body.
    pub(super) fn settle_health(
        &mut self,
        wire: u32,
        effect: HealthEffect,
        source: Death,
        now: Duration,
    ) -> Result<bool, CombatError> {
        if let Some(state) = self.state_mut(wire)? {
            let (delta, killed, named_pool_losses) = match effect {
                HealthEffect::Damage(amount) => Self::damage_bot(state, amount, source, now)
                    .map_or((0.0, false, None), |outcome| {
                        (
                            -outcome.dealt,
                            outcome.killed,
                            Some(outcome.named_pool_losses),
                        )
                    }),
                HealthEffect::Heal(amount) if !state.bot.dead => {
                    (state.vitals.heal(amount), false, None)
                }
                HealthEffect::Heal(_) => (0.0, false, None),
            };
            if delta != 0.0 {
                let health = state.vitals.component();
                state.vitals.clear_changed();
                self.scripted_health.push(EntityUpdate {
                    entity: wire,
                    operation: Some(Operation::Update(vec![health])),
                    ..Default::default()
                });
                self.record_health_applied(
                    source.killer,
                    wire,
                    delta,
                    killed,
                    named_pool_losses
                        .as_deref()
                        .map(|named_pool_losses| HealthDamageEvidence {
                            impact: None,
                            named_pool_losses,
                        }),
                );
            }
            return Ok(true);
        }
        let Some(id) = self.player_handle_for_wire(wire) else {
            return Ok(false);
        };
        match effect {
            HealthEffect::Damage(amount) => {
                if let Some((owner, outcome, health)) =
                    self.damage_player_body_by_wire(id, amount, source, None)
                {
                    self.scripted_health.extend(health);
                    if outcome.killed {
                        self.player_deaths.push(PlayerDeath {
                            owner,
                            body: id,
                            source: source.killer,
                            killed_by: valid_entity(source.killer) && source.killer != wire,
                        });
                    }
                }
            }
            HealthEffect::Heal(amount) => {
                if let Some(state) = self
                    .player_body_mut(id)
                    .filter(|state| state.alive() && state.vitals().is_some())
                {
                    let healed = state.heal(amount);
                    if let Some(health) = state.take_health_update() {
                        self.scripted_health.push(EntityUpdate {
                            entity: wire,
                            operation: Some(Operation::Update(vec![health])),
                            ..Default::default()
                        });
                    }
                    self.record_health_applied(source.killer, wire, healed, false, None);
                }
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_combat::{
        BOT_RESPAWN_TIME,
        tests::{fixture_bots, fixture_ground},
    };
    use crate::game_combat::{RoomScriptRequests, script_requests::MAX_BODY_SCRIPT_REQUESTS};
    use std::sync::Arc;

    #[test]
    fn contact_damage_and_impulse_follow_room_team_wall_and_body_admission() {
        use crate::{mover_settings::MoverCatalog, script_graph::RoomSpatialNeeds};
        let bots = fixture_bots();
        let (source, target) = (bots[0].entity, bots[1].entity);
        for free_for_all in [false, true] {
            let mut room = RoomSimulation::new(fixture_bots()).unwrap();
            room.enable_spatial_queries(
                free_for_all,
                Arc::new(fixture_ground()),
                RoomSpatialNeeds::Queries,
            );
            room.enable_spatial_actor_rays(Arc::new(MoverCatalog::default()));
            let tick = 7;
            room.prepare_spatial_frame(tick).unwrap();
            let frame = &room.spatial_frame.as_ref().unwrap().1;
            let targets = frame
                .hostile_contact_targets(source, [2.0, 0.5, 0.0], [1.0, 0.5, 0.0], 0.25, 0x18)
                .unwrap();
            assert_eq!(
                targets
                    .iter()
                    .map(|target| target.entity)
                    .collect::<Vec<_>>(),
                if free_for_all { vec![target] } else { vec![] }
            );
            if !free_for_all {
                continue;
            }
            assert!(
                frame
                    .hostile_contact_targets(source, [2.0, 0.5, 0.0], [-3.0, 0.5, 0.25], 0.25, 0x18)
                    .unwrap()
                    .is_empty()
            );
            let mut requests = RoomScriptRequests::default();
            assert!(!requests.queue_contact_damage(frame, 25.0, &[(target, None), (target, None)]));
            assert!(requests.queue_contact_damage(frame, 25.0, &[(target, Some([5.0, 2.0, 0.0]))]));
            let source = room.sample_health_body(source).unwrap();
            let [effect] = requests.take().try_into().ok().unwrap();
            room.settle_script_request(effect, source, tick, Duration::ZERO)
                .unwrap();
            assert_eq!(room.body_healths().unwrap()[&target], 75.0);
            assert!(
                room.state_mut(target)
                    .unwrap()
                    .unwrap()
                    .bot
                    .has_combat_motion()
            );
        }
    }

    #[test]
    #[ignore = "requires tagged 104319 Practice Range graphs and authored static ray profiles"]
    fn prepared_stock_quick_melee_settles_damage_and_knockback_once() {
        use crate::{
            commands::Command,
            game_clock::FIRST_TICK,
            game_combat::{PlayerBodyState, script_eye_position},
            game_content::GameMap,
            game_heroes::SOLDIER,
            game_instance::RETAIL_QUANTA,
            game_replication::{BodyId, BodyStep},
            match_scene::team_bit,
            scene_assets::SceneAssets,
            script_player::OwnerBodySpawn,
            script_snapshot_wire::View,
        };

        let path = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
        let assets = SceneAssets::load(path).unwrap();
        let mut scene = assets.prepare(GameMap::practice_range()).unwrap();
        let hero = assets.heroes().get(SOLDIER).unwrap();
        let mut mover = scene.player_spawn_mover(hero.body, 0).unwrap();
        let owner = 0xa000_0100;
        let id = BodyId {
            entity: owner + 1,
            generation: 1,
        };
        let spawn = OwnerBodySpawn {
            slot: 0,
            controller: owner,
            entity: id.entity,
            body_filter: team_bit(0).unwrap(),
            frame: FIRST_TICK,
            pose: mover.pose(),
            eye_position: script_eye_position(hero.guid, &mover),
            skin: 0,
            quanta: RETAIL_QUANTA,
        };
        let mut runtime = scene.owner_body_runtime(hero, spawn).unwrap().unwrap();
        let source_pose = scene.player_spawn_mover(hero.body, 0).unwrap();
        let room = scene.simulation_mut();
        room.spawn_player_body(PlayerBodyState::new(
            id,
            owner,
            0,
            0,
            hero.guid,
            source_pose,
            None,
            FIRST_TICK,
            None,
        ))
        .unwrap();
        let target = room
            .bodies()
            .unwrap()
            .find(|bot| bot.spawn.team == 1)
            .unwrap()
            .entity;
        let [x, y, z] = mover.pose().position;
        room.body_mut(target).unwrap().position = [x - 1.5, y, z];
        let health = room.body_healths().unwrap()[&target];
        let mut settled = 0;
        for dt in 1..45 {
            let frame = FIRST_TICK + dt;
            let command = Command {
                frame,
                yaw: -16384,
                buttons: if dt == 20 { 0x800 } else { 0 },
                ..Command::default()
            };
            mover
                .step(scene.movement_index(), command, 0.016, |_, _| {})
                .unwrap();
            let room = scene.simulation_mut();
            room.prepare_spatial_frame(frame).unwrap();
            runtime.set_spatial_frame(Arc::clone(&room.spatial_frame.as_ref().unwrap().1));
            runtime.advance(command, &mover, BodyStep::Posed).unwrap();
            let source = room.sample_health_body(id.entity).unwrap();
            let requests = runtime.take_script_requests();
            let snapshot = runtime.snapshot().unwrap();
            assert!(
                snapshot.diagnostics.is_empty(),
                "frame {frame}: {:?}",
                snapshot.diagnostics
            );
            if !requests.is_empty() {
                for view in [View::Owner, View::Observer] {
                    runtime.encoder().project(snapshot, view).unwrap();
                }
            }
            for effect in requests {
                room.settle_script_request(effect, source, frame, Duration::ZERO)
                    .unwrap();
                settled += 1;
            }
        }
        let room = scene.simulation_mut();
        assert_eq!(settled, 1);
        assert_eq!(room.body_healths().unwrap()[&target], health - 30.0);
        assert!(
            room.state_mut(target)
                .unwrap()
                .unwrap()
                .bot
                .has_combat_motion()
        );
    }

    #[test]
    fn authored_health_settlement_uses_bot_health_and_death_lifecycle() {
        let bots = fixture_bots();
        let source = bots[0].entity;
        let target = bots[1].entity;
        let mut room = RoomSimulation::new(bots).unwrap();
        let source_body = room.sample_health_body(source).unwrap();
        room.enable_spatial_queries(
            false,
            Arc::new(fixture_ground()),
            crate::script_graph::RoomSpatialNeeds::Queries,
        );
        let tick = 3;
        let now = Duration::from_millis(48);
        room.prepare_spatial_frame(tick).unwrap();
        let frame = Arc::clone(&room.spatial_frame.as_ref().unwrap().1);
        let mut requests = RoomScriptRequests::default();
        assert!(!requests.queue_health(&frame, target, f64::NAN));
        assert!(!requests.queue_health(&frame, target, 0.0));
        assert!(!requests.queue_health(&frame, 0, 1.0));

        for (amount, expected, changed) in [
            (-25.0, 75.0, true),
            (f64::MAX, 100.0, true),
            (-200.0, 0.0, true),
            (100.0, 0.0, false),
        ] {
            assert!(requests.queue_health(&frame, target, amount));
            for request in requests.take() {
                room.settle_script_request(request, source_body, tick, now)
                    .unwrap();
            }
            assert_eq!(room.body_healths().unwrap()[&target], expected);
            let updates = room.take_scripted_health();
            assert_eq!(updates.len(), usize::from(changed));
            assert!(updates.iter().all(|update| update.entity == target));
        }
        let (death, respawn) = room.observed_bot_death(target).unwrap().unwrap();
        assert_eq!(death.killer, source);
        assert_eq!(death.frame, tick);
        assert_eq!(respawn, now + BOT_RESPAWN_TIME);
    }

    #[test]
    fn scripted_kill_respawns_once_when_combat_is_inactive() {
        #[derive(serde::Deserialize)]
        struct Contract {
            schema_version: u32,
            input: Input,
            expected: Expected,
        }
        #[derive(serde::Deserialize)]
        struct Input {
            kill_tick: u32,
            kill_time_ms: u64,
            health_change: f64,
        }
        #[derive(serde::Deserialize)]
        struct Expected {
            replacement_counts: [usize; 3],
            fresh_health: f64,
        }

        let contract: Contract = serde_json::from_str(include_str!(
            "../../../fixtures/behavior/training-bot-scripted-respawn-v1.json"
        ))
        .unwrap();
        assert_eq!(contract.schema_version, 1);
        let tick = contract.input.kill_tick;
        let now = Duration::from_millis(contract.input.kill_time_ms);
        let due = now + BOT_RESPAWN_TIME;
        let one_ns = Duration::from_nanos(1);
        let [before_due, at_due, after_due] = contract.expected.replacement_counts;

        let bots = fixture_bots();
        let source = bots[0].entity;
        let target = bots[1].entity;
        let mut room = RoomSimulation::new(bots).unwrap();
        let source_body = room.sample_health_body(source).unwrap();
        let target_body = room.sample_health_body(target).unwrap();
        let ground = Arc::new(fixture_ground());
        let movers = crate::mover_settings::MoverCatalog::default();
        let queries = crate::scene_assets::BotQueries::new(false, &movers, &ground, None);
        room.enable_spatial_queries(
            false,
            Arc::clone(&ground),
            crate::script_graph::RoomSpatialNeeds::Queries,
        );
        room.prepare_spatial_frame(tick).unwrap();

        let mut requests = RoomScriptRequests::default();
        let frame = &room.spatial_frame.as_ref().unwrap().1;
        assert!(requests.queue_health(frame, target, contract.input.health_change));
        for request in requests.take() {
            room.settle_script_request(request, source_body, tick, now)
                .unwrap();
        }
        assert_eq!(room.observed_bot_death(target).unwrap().unwrap().1, due);
        assert!(!room.combat_active);

        let early = room.finish_tick(&queries, tick + 1, due - one_ns).unwrap();
        assert_eq!(early.len(), before_due);
        assert!(room.sample_health_body(target) == Some(target_body));

        let replacements = room.finish_tick(&queries, tick + 2, due).unwrap();
        assert_eq!(replacements.len(), at_due);
        let [replacement] = replacements.as_slice() else {
            panic!("expected one replacement at the respawn deadline");
        };
        assert_eq!(replacement.retired, target);
        let fresh = replacement.created.entity;
        assert_ne!(fresh, target);
        assert!(matches!(
            &replacement.created.operation,
            Some(Operation::Create(_))
        ));
        assert!(room.sample_health_body(target).is_none());
        assert!(room.world.get_entity(target_body.entity).is_err());
        let fresh_body = room.sample_health_body(fresh).unwrap();
        assert_ne!(fresh_body.entity, target_body.entity);
        assert_eq!(
            room.body_healths().unwrap()[&fresh],
            contract.expected.fresh_health
        );

        let repeated = room.finish_tick(&queries, tick + 3, due + one_ns).unwrap();
        assert_eq!(repeated.len(), after_due);
        assert!(room.sample_health_body(fresh) == Some(fresh_body));
        assert!(room.sample_health_body(source) == Some(source_body));
        assert!(!room.combat_active);
    }

    #[test]
    fn authored_health_queue_has_a_body_local_limit() {
        let bots = fixture_bots();
        let target = bots[0].entity;
        let mut room = RoomSimulation::new(bots).unwrap();
        room.enable_spatial_queries(
            false,
            Arc::new(fixture_ground()),
            crate::script_graph::RoomSpatialNeeds::Queries,
        );
        room.prepare_spatial_frame(3).unwrap();
        let frame = &room.spatial_frame.as_ref().unwrap().1;
        let mut requests = RoomScriptRequests::default();
        for _ in 0..MAX_BODY_SCRIPT_REQUESTS {
            assert!(requests.queue_health(frame, target, -1.0));
        }
        assert!(!requests.queue_health(frame, target, -1.0));
        assert_eq!(requests.take().len(), MAX_BODY_SCRIPT_REQUESTS);
        assert!(requests.queue_health(frame, target, -1.0));
    }

    #[test]
    fn admitted_health_batches_keep_order_and_skip_replaced_targets_at_settlement() {
        use crate::game_combat::BotState;

        let bots = fixture_bots();
        let first = bots[0].entity;
        let second = bots[1].entity;
        let mut room = RoomSimulation::new(bots).unwrap();
        let source = room.sample_health_body(first).unwrap();
        room.enable_spatial_queries(
            false,
            Arc::new(fixture_ground()),
            crate::script_graph::RoomSpatialNeeds::Queries,
        );
        let mut requests = RoomScriptRequests::default();
        let tick = 3;
        let now = Duration::from_millis(48);
        room.prepare_spatial_frame(tick).unwrap();
        let frame = &room.spatial_frame.as_ref().unwrap().1;
        assert!(requests.queue_health_changes(frame, &[(second, -20.0), (first, -10.0)]));
        assert_eq!(room.body_healths().unwrap()[&first], 100.0);
        assert_eq!(room.body_healths().unwrap()[&second], 100.0);
        for request in requests.take() {
            room.settle_script_request(request, source, tick, now)
                .unwrap();
        }
        assert_eq!(
            room.take_scripted_health()
                .into_iter()
                .map(|update| update.entity)
                .collect::<Vec<_>>(),
            [second, first],
        );

        room.prepare_spatial_frame(tick + 1).unwrap();
        let frame = &room.spatial_frame.as_ref().unwrap().1;
        assert!(requests.queue_health_changes(frame, &[(second, 10.0), (first, -5.0)]));
        // Model replacement between admission and settlement while keeping the public ID.
        let previous = room.by_wire[&second];
        let body = room.world.entity_mut(previous).take::<BotState>().unwrap();
        assert!(room.world.despawn(previous));
        let replacement = room.world.spawn(body).id();
        assert_ne!(previous, replacement);
        assert_eq!(room.by_wire.insert(second, replacement), Some(previous));
        for request in requests.take() {
            room.settle_script_request(request, source, tick + 1, now)
                .unwrap();
        }
        assert_eq!(room.body_healths().unwrap()[&first], 85.0);
        assert_eq!(room.body_healths().unwrap()[&second], 80.0);
        let updates = room.take_scripted_health();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].entity, first);
    }
}
