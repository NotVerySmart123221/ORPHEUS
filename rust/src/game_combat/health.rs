//! Room-authoritative health effects for player and NPC bodies.

use super::{CombatError, RoomSimulation, player::PlayerDeath};
use crate::{
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
                if let Some((owner, killed, health)) =
                    self.damage_player_body_by_wire(id, amount, source, None)
                {
                    self.scripted_health.extend(health);
                    if killed {
                        self.player_deaths.push(PlayerDeath {
                            owner,
                            source: source.killer,
                            killed_by: source.killer != wire,
                        });
                    }
                }
            }
            HealthEffect::Heal(amount) => {
                if let Some(state) = self.player_body_mut(id).filter(|state| state.alive()) {
                    if let Some(runtime) = &mut state.runtime {
                        let healed = runtime.heal(amount);
                        if let Some(health) = runtime.take_health_update() {
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
