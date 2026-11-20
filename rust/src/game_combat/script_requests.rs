//! Deferred script operations for generation-checked room bodies.

use super::{
    BotState, CombatError, PlayerBodyState, RoomSimulation, RoomSpatialFrame, health::HealthEffect,
};
use crate::{script_init::RoomMessage, vitals::Death};
use bevy_ecs::entity::Entity;
use std::{collections::HashSet, time::Duration};

pub(super) const MAX_BODY_SCRIPT_REQUESTS: usize = 64;

pub(super) struct RoomScriptTarget {
    pub(super) entity: Entity,
    pub(super) health: bool,
    pub(super) messages: bool,
}

pub(crate) struct RoomScriptRequest {
    wire: u32,
    entity: Entity,
    effect: ScriptEffect,
}

impl RoomScriptRequest {
    fn for_target(frame: &RoomSpatialFrame, wire: u32, effect: ScriptEffect) -> Option<Self> {
        let target = frame.script_targets.get(&wire)?;
        let supported = match &effect {
            ScriptEffect::Health(_) => target.health,
            ScriptEffect::Message(_) => target.messages,
        };
        supported.then_some(Self {
            wire,
            entity: target.entity,
            effect,
        })
    }
}

enum ScriptEffect {
    Health(HealthEffect),
    Message(RoomMessage),
}

#[derive(Default)]
pub(crate) struct RoomScriptRequests(Vec<RoomScriptRequest>);

impl RoomScriptRequests {
    pub(crate) fn queue_health(&mut self, frame: &RoomSpatialFrame, wire: u32, delta: f64) -> bool {
        let Some(effect) = HealthEffect::from_delta(delta) else {
            return false;
        };
        self.queue(frame, wire, ScriptEffect::Health(effect))
    }

    /// Admit every target into the body outbox, or leave existing work intact.
    pub(crate) fn queue_health_changes(
        &mut self,
        frame: &RoomSpatialFrame,
        changes: &[(u32, f64)],
    ) -> bool {
        if changes.is_empty() {
            return true;
        }
        if changes.len() > MAX_BODY_SCRIPT_REQUESTS.saturating_sub(self.0.len()) {
            return false;
        }
        let mut targets = HashSet::with_capacity(changes.len());
        let mut staged = Vec::with_capacity(changes.len());
        for &(wire, delta) in changes {
            if !targets.insert(wire) {
                return false;
            }
            let Some(effect) = HealthEffect::from_delta(delta) else {
                return false;
            };
            let Some(request) =
                RoomScriptRequest::for_target(frame, wire, ScriptEffect::Health(effect))
            else {
                return false;
            };
            staged.push(request);
        }
        self.0.extend(staged);
        true
    }

    pub(crate) fn queue_message(
        &mut self,
        frame: &RoomSpatialFrame,
        wire: u32,
        message: RoomMessage,
    ) -> bool {
        if message.id == 0 {
            return false;
        }
        self.queue(frame, wire, ScriptEffect::Message(message))
    }

    fn queue(&mut self, frame: &RoomSpatialFrame, wire: u32, effect: ScriptEffect) -> bool {
        if self.0.len() >= MAX_BODY_SCRIPT_REQUESTS {
            return false;
        }
        let Some(request) = RoomScriptRequest::for_target(frame, wire, effect) else {
            return false;
        };
        self.0.push(request);
        true
    }

    pub(crate) fn take(&mut self) -> Vec<RoomScriptRequest> {
        std::mem::take(&mut self.0)
    }
}

impl RoomSimulation {
    /// Collect every actor's outgoing work before routing to any recipient.
    pub(super) fn settle_authored_requests(
        &mut self,
        tick: u32,
        now: Duration,
    ) -> Result<(), CombatError> {
        if self.spatial_frame.is_none() {
            return Ok(());
        }
        let mut batches = Vec::new();
        for (&source, &entity) in &self.players_by_wire {
            if let Some(mut state) = self.world.get_mut::<PlayerBodyState>(entity) {
                if let Some(runtime) = state.runtime.as_mut() {
                    let requests = runtime.take_script_requests();
                    if !requests.is_empty() {
                        batches.push((source, requests));
                    }
                }
            }
        }
        for (&source, &entity) in &self.by_wire {
            if let Some(mut state) = self.world.get_mut::<BotState>(entity) {
                if let Some(enemy) = &mut state.enemy {
                    let requests = enemy.scripts.take_script_requests();
                    if !requests.is_empty()
                        && enemy.fault.is_none()
                        && !enemy.scripts.retirement_requested
                    {
                        batches.push((source, requests));
                    }
                }
            }
        }
        for (source, requests) in batches {
            for request in requests {
                self.settle_script_request(request, source, tick, now)?;
            }
        }
        Ok(())
    }

    pub(super) fn settle_script_request(
        &mut self,
        request: RoomScriptRequest,
        source: u32,
        tick: u32,
        now: Duration,
    ) -> Result<(), CombatError> {
        let current = self
            .by_wire
            .get(&request.wire)
            .or_else(|| self.players_by_wire.get(&request.wire));
        if current != Some(&request.entity) {
            return Ok(());
        }
        match request.effect {
            ScriptEffect::Health(effect) => {
                self.settle_health(
                    request.wire,
                    effect,
                    Death {
                        time: now.as_secs_f64(),
                        frame: tick,
                        killer: source,
                    },
                    now,
                )?;
            }
            ScriptEffect::Message(message) => {
                self.route_room_message(request.entity, source, message)
            }
        }
        Ok(())
    }

    fn route_room_message(&mut self, entity: Entity, source: u32, message: RoomMessage) {
        // A full or halted recipient can drop the message. Scheduling does
        // not execute that VM or turn its local queue rejection into a room fault.
        if let Some(mut state) = self.world.get_mut::<PlayerBodyState>(entity) {
            if state.alive() {
                if let Some(runtime) = &mut state.runtime {
                    let _ = runtime.receive_room_message(source, message);
                }
            }
        } else if let Some(mut state) = self.world.get_mut::<BotState>(entity) {
            if !state.bot.dead && state.vitals.death().is_none() {
                if let Some(enemy) = state
                    .enemy
                    .as_mut()
                    .filter(|enemy| enemy.fault.is_none() && !enemy.scripts.retirement_requested)
                {
                    let _ = enemy.scripts.receive_room_message(source, message);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use super::*;
    use crate::game_combat::tests::fixture_ground;

    #[test]
    fn invalid_batches_leave_mixed_outbox_work_and_capacity_intact() {
        let mut world = bevy_ecs::world::World::new();
        let mut target = |health, messages| RoomScriptTarget {
            entity: world.spawn_empty().id(),
            health,
            messages,
        };
        let frame = RoomSpatialFrame {
            candidates: Box::default(),
            entity_facts: None,
            script_targets: HashMap::from([
                (1, target(true, false)),
                (2, target(true, false)),
                (3, target(false, true)),
            ]),
            ground: Arc::new(fixture_ground()),
        };
        let mut outbox = RoomScriptRequests::default();
        let message = RoomMessage {
            id: 71,
            parameters: Default::default(),
        };
        assert!(outbox.queue_message(&frame, 3, message));

        for invalid in [(0, -2.0), (3, -2.0), (2, f64::NAN), (2, 0.0), (1, 2.0)] {
            assert!(!outbox.queue_health_changes(&frame, &[(1, -1.0), invalid]));
            assert_eq!(outbox.0.len(), 1);
        }
        for _ in 0..MAX_BODY_SCRIPT_REQUESTS - 2 {
            assert!(outbox.queue_health(&frame, 1, -1.0));
        }
        assert!(!outbox.queue_health_changes(&frame, &[(1, -1.0), (2, 2.0)]));
        assert_eq!(outbox.0.len(), MAX_BODY_SCRIPT_REQUESTS - 1);
        assert!(outbox.queue_health_changes(&frame, &[(2, 2.0)]));
        assert!(outbox.queue_health_changes(&frame, &[]));

        let queued = outbox.take();
        assert_eq!(queued.len(), MAX_BODY_SCRIPT_REQUESTS);
        assert!(matches!(queued[0].effect, ScriptEffect::Message(_)));
        assert_eq!(queued.last().unwrap().wire, 2);
    }
}
