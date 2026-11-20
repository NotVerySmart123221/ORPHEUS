//! Deferred script operations for generation-checked room bodies.

use super::{
    BotState, CombatError, PlayerBodyState, RoomSimulation, RoomSpatialFrame, health::HealthEffect,
};
use crate::{script_init::RoomMessage, vitals::Death};
use bevy_ecs::entity::Entity;
use std::time::Duration;

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

enum ScriptEffect {
    Health(HealthEffect),
    Message(RoomMessage),
}

#[derive(Default)]
pub(crate) struct RoomScriptRequests(Vec<RoomScriptRequest>);

impl RoomScriptRequests {
    pub(crate) fn queue_health(&mut self, frame: &RoomSpatialFrame, wire: u32, delta: f64) -> bool {
        if !delta.is_finite() || delta == 0.0 {
            return false;
        }
        let effect = if delta < 0.0 {
            HealthEffect::Damage(-delta)
        } else {
            HealthEffect::Heal(delta)
        };
        self.queue(frame, wire, ScriptEffect::Health(effect))
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
        let Some(target) = frame.script_targets.get(&wire) else {
            return false;
        };
        let supported = match &effect {
            ScriptEffect::Health(_) => target.health,
            ScriptEffect::Message(_) => target.messages,
        };
        if !supported {
            return false;
        }
        self.0.push(RoomScriptRequest {
            wire,
            entity: target.entity,
            effect,
        });
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
