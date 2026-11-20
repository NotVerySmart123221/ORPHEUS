//! Deferred notifications for room-authoritative health changes.

use std::collections::VecDeque;

use bevy_ecs::entity::Entity;
use indexmap::IndexMap;

use crate::{script_expr::valid_entity, script_init::HostEvent};

use super::{BotState, PlayerBodyState, RoomSimulation};

const MAX_ROOM_HEALTH_EVENTS: usize = 512;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct SampledBody {
    pub(super) wire: u32,
    pub(super) entity: Entity,
}

impl SampledBody {
    fn lookup(
        wire: u32,
        bots: &IndexMap<u32, Entity>,
        players: &IndexMap<u32, Entity>,
    ) -> Option<Self> {
        if !valid_entity(wire) {
            return None;
        }
        let &entity = bots.get(&wire).or_else(|| players.get(&wire))?;
        Some(Self { wire, entity })
    }
}

struct HealthApplied {
    source: Option<SampledBody>,
    target: SampledBody,
    delta: f64,
    killed: bool,
}

impl HealthApplied {
    fn resolve(self, room: &RoomSimulation) -> Option<(HostEvent, [Option<SampledBody>; 2])> {
        let target = room.sample_health_body(self.target.wire);
        if target.is_some_and(|body| body != self.target) {
            // A replacement must not inherit a stale notification or its target ID.
            return None;
        }
        let source = self
            .source
            .filter(|&body| room.sample_health_body(body.wire) == Some(body));
        let event = HostEvent::HealthApplied {
            source: source.map(|body| body.wire),
            target: self.target.wire,
            delta: self.delta,
            killed: self.killed,
        };
        Some((event, [source.filter(|&body| body != self.target), target]))
    }
}

#[derive(Default)]
pub(super) struct RoomHealthEvents {
    enabled: bool,
    pending: VecDeque<HealthApplied>,
}

impl RoomHealthEvents {
    pub(super) fn stage(
        &mut self,
        source: Option<SampledBody>,
        target: SampledBody,
        delta: f64,
        killed: bool,
    ) {
        if !self.enabled
            || self.pending.len() >= MAX_ROOM_HEALTH_EVENTS
            || !delta.is_finite()
            || delta == 0.0
            || (killed && delta > 0.0)
        {
            return;
        }
        self.pending.push_back(HealthApplied {
            source,
            target,
            delta,
            killed,
        });
    }
}

impl RoomSimulation {
    pub(crate) fn enable_host_health_events(&mut self) {
        self.health_events.enabled = true;
    }

    pub(super) fn record_health_applied(
        &mut self,
        source: u32,
        target: u32,
        delta: f64,
        killed: bool,
    ) {
        if !self.health_events.enabled {
            return;
        }
        let Some(target) = self.sample_health_body(target) else {
            return;
        };
        let source = self.sample_health_body(source);
        self.health_events.stage(source, target, delta, killed);
    }

    /// Queue each event for the recipients' next VM frames, in settlement order.
    pub(super) fn flush_health_events(&mut self) {
        while let Some(change) = self.health_events.pending.pop_front() {
            if let Some((event, receivers)) = change.resolve(self) {
                for body in receivers.into_iter().flatten() {
                    self.queue_health_event(body.entity, event);
                }
            }
        }
    }

    pub(super) fn sample_health_body(&self, wire: u32) -> Option<SampledBody> {
        SampledBody::lookup(wire, &self.by_wire, &self.players_by_wire)
    }

    /// Collect already applied VM-local mutations before processing retirement.
    pub(super) fn collect_local_health_events(&mut self) {
        if !self.health_events.enabled {
            return;
        }
        for (&wire, &entity) in &self.by_wire {
            let Some(mut state) = self.world.get_mut::<BotState>(entity) else {
                continue;
            };
            let Some(enemy) = state.enemy.as_mut() else {
                continue;
            };
            let target = SampledBody { wire, entity };
            for change in enemy.scripts.take_local_health_changes() {
                let source =
                    SampledBody::lookup(change.source, &self.by_wire, &self.players_by_wire);
                self.health_events
                    .stage(source, target, change.delta, change.killed);
            }
        }
    }

    fn queue_health_event(&mut self, entity: Entity, event: HostEvent) {
        // A full or halted VM only drops its own notification.
        if let Some(mut state) = self.world.get_mut::<PlayerBodyState>(entity) {
            if state.alive() {
                if let Some(runtime) = &mut state.runtime {
                    let _ = runtime.receive_host_event(event);
                }
            }
        } else if let Some(mut state) = self.world.get_mut::<BotState>(entity) {
            if !state.bot.dead && state.vitals.death().is_none() {
                if let Some(enemy) = state
                    .enemy
                    .as_mut()
                    .filter(|enemy| enemy.fault.is_none() && !enemy.scripts.retirement_requested)
                {
                    let _ = enemy.scripts.body.queue_host_event(event);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{
        game_combat::{HealthEffect, tests::fixture_bots},
        vitals::Death,
    };

    #[test]
    fn records_actual_settled_damage_and_healing() {
        let bots = fixture_bots();
        let source = bots[0].entity;
        let target = bots[1].entity;
        let mut room = RoomSimulation::new(bots).unwrap();
        let now = Duration::from_millis(48);
        let dealer = Death {
            killer: source,
            frame: 3,
            time: now.as_secs_f64(),
        };
        room.settle_health(target, HealthEffect::Damage(1.0), dealer, now)
            .unwrap();
        assert!(room.health_events.pending.is_empty());
        room.enable_host_health_events();

        for (effect, expected) in [
            (HealthEffect::Damage(24.0), Some((-24.0, false))),
            (HealthEffect::Heal(50.0), Some((25.0, false))),
            (HealthEffect::Heal(1.0), None),
            (HealthEffect::Damage(200.0), Some((-100.0, true))),
            (HealthEffect::Damage(1.0), None),
            (HealthEffect::Heal(1.0), None),
        ] {
            assert!(room.settle_health(target, effect, dealer, now).unwrap());
            let actual = room.health_events.pending.pop_front().map(|entry| {
                assert_eq!(entry.source.map(|body| body.wire), Some(source));
                assert_eq!(entry.target.wire, target);
                (entry.delta, entry.killed)
            });
            assert_eq!(actual, expected);
            assert!(room.health_events.pending.is_empty());
        }
    }

    #[test]
    fn field_pulses_record_one_capped_change_with_checked_source_per_body() {
        use crate::{
            contact_world::ContactWorld,
            game_combat::{BioticField, BotQueries, FIELD_DELAY, tests::fixture_ground},
            game_replication::BodyId,
            mover::Mover,
            mover_settings::{MoverCatalog, MoverSettings},
        };
        use std::sync::Arc;

        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/behavior/script-local-health-outcomes-v1.json"
        ))
        .unwrap();
        assert_eq!(
            fixture["case_id"],
            "script-local-health-outcomes.synthetic.v1"
        );
        let field = &fixture["input"]["field"];
        let wounds: [f64; 2] =
            serde_json::from_value(field["initial_two_actor_wounds"].clone()).unwrap();
        let deltas: [f64; 2] =
            serde_json::from_value(fixture["expected"]["field"]["actual_deltas"].clone()).unwrap();
        let elapsed = u32::try_from(field["pulse_ticks_elapsed"].as_u64().unwrap()).unwrap();

        let bots = fixture_bots();
        let targets = [bots[0].entity, bots[1].entity];
        let mut room = RoomSimulation::new(bots).unwrap();
        let owner = BodyId {
            entity: 702,
            generation: 1,
        };
        let owner_body = |id| {
            let mover = Mover::new(
                MoverSettings::default(),
                Arc::new(ContactWorld::new(Vec::new()).unwrap()),
                [0.0; 3],
                0,
                0x08,
            )
            .unwrap();
            PlayerBodyState::new(id, 701, 0, 1, 703, mover, None, 100, None)
        };
        room.spawn_player_body(owner_body(owner)).unwrap();
        room.enable_host_health_events();
        let mut field = BioticField::new([0.0; 3]);
        assert!(field.advance(100).0.is_none());
        let pulse = field.advance(100 + FIELD_DELAY + elapsed).0.unwrap();
        assert!(pulse.amount < wounds[1]);
        let pulses = [pulse, pulse];
        let ground = fixture_ground();
        let movers = MoverCatalog::default();
        let queries = BotQueries::new(false, &movers, &ground, None);

        let wound = |room: &mut RoomSimulation, target, amount| {
            room.state_mut(target).unwrap().unwrap().vitals.damage(
                amount,
                Death {
                    killer: owner.entity,
                    frame: 100,
                    time: 1.6,
                },
            );
        };
        for (target, amount) in targets.into_iter().zip(wounds) {
            wound(&mut room, target, amount);
        }
        let updates = room
            .heal_allies(&queries, 1, Some(owner.entity), Some(owner), &pulses)
            .unwrap();
        assert_eq!(
            updates
                .iter()
                .map(|update| update.entity)
                .collect::<Vec<_>>(),
            targets
        );
        for (wire, delta) in targets.into_iter().zip(deltas) {
            let event = room.health_events.pending.pop_front().unwrap();
            assert_eq!(event.source.map(|body| body.wire), Some(owner.entity));
            assert_eq!(
                (event.target.wire, event.delta, event.killed),
                (wire, delta, false)
            );
        }
        assert!(room.health_events.pending.is_empty());
        assert!(
            room.heal_allies(&queries, 1, Some(owner.entity), Some(owner), &pulses)
                .unwrap()
                .is_empty()
        );
        assert!(room.health_events.pending.is_empty());

        room.spawn_player_body(owner_body(BodyId {
            generation: 2,
            ..owner
        }))
        .unwrap();
        assert!(
            room.advance_player_fields(owner, 100 + FIELD_DELAY + elapsed)
                .is_none()
        );
        wound(&mut room, targets[0], wounds[0]);
        room.heal_allies(&queries, 1, Some(owner.entity), Some(owner), &pulses)
            .unwrap();
        let event = room.health_events.pending.pop_front().unwrap();
        assert!(event.source.is_none());
        assert_eq!((event.target.wire, event.delta), (targets[0], deltas[0]));
        assert!(room.health_events.pending.is_empty());
    }

    #[test]
    fn bounds_pending_events_and_resolves_each_body_generation() {
        let bots = fixture_bots();
        let source = bots[0].entity;
        let target = bots[1].entity;
        let mut room = RoomSimulation::new(bots).unwrap();
        room.enable_host_health_events();

        for (delta, killed) in [
            (f64::NAN, false),
            (f64::INFINITY, false),
            (0.0, false),
            (-0.0, false),
            (1.0, true),
        ] {
            room.record_health_applied(source, target, delta, killed);
        }
        room.record_health_applied(source, 0, -1.0, false);
        assert!(room.health_events.pending.is_empty());

        room.record_health_applied(source, source, -1.0, false);
        let self_damage = room.health_events.pending.pop_front().unwrap();
        let (_, receivers) = self_damage.resolve(&room).unwrap();
        assert_eq!(receivers.into_iter().flatten().count(), 1);

        for amount in 1..=MAX_ROOM_HEALTH_EVENTS + 1 {
            room.record_health_applied(source, target, -(amount as f64), false);
        }
        assert_eq!(room.health_events.pending.len(), MAX_ROOM_HEALTH_EVENTS);
        let retired = room.by_wire.shift_remove(&target).unwrap();
        assert!(room.world.despawn(retired));
        let first = room.health_events.pending.pop_front().unwrap();
        let (event, receivers) = first.resolve(&room).unwrap();
        assert_eq!(
            event,
            HostEvent::HealthApplied {
                source: Some(source),
                target,
                delta: -1.0,
                killed: false
            }
        );
        assert_eq!(
            receivers.map(|body| body.map(|body| body.wire)),
            [Some(source), None]
        );

        let replacement = room.world.spawn_empty().id();
        assert_ne!(retired, replacement);
        room.by_wire.insert(target, replacement);
        let last = room.health_events.pending.pop_back().unwrap();
        assert_eq!(last.delta, -(MAX_ROOM_HEALTH_EVENTS as f64));
        assert!(last.resolve(&room).is_none());
        room.flush_health_events();
        assert!(room.health_events.pending.is_empty());
    }
}
