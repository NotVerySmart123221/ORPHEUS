//! Room-authoritative health effects for player and NPC bodies.

use super::{CombatError, RoomSimulation, player::PlayerDeath};
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
            let (delta, killed) = match effect {
                HealthEffect::Damage(amount) => Self::damage_bot(state, amount, source, now)
                    .map_or((0.0, false), |outcome| (-outcome.dealt, outcome.killed)),
                HealthEffect::Heal(amount) if !state.bot.dead => (state.vitals.heal(amount), false),
                HealthEffect::Heal(_) => (0.0, false),
            };
            if delta != 0.0 {
                let health = state.vitals.component();
                state.vitals.clear_changed();
                self.scripted_health.push(EntityUpdate {
                    entity: wire,
                    operation: Some(Operation::Update(vec![health])),
                    ..Default::default()
                });
                self.record_health_applied(source.killer, wire, delta, killed, None);
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
