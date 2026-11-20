//! Deferred script operations for generation-checked room bodies.

use super::{
    BotState, CombatError, PlayerBodyState, RoomSimulation, RoomSpatialFrame, health::HealthEffect,
};
use crate::{
    game_combat_messages::CombatNotice,
    game_replication::BodyId,
    script_init::{RoomCue, RoomMessage},
    vitals::Death,
};
use bevy_ecs::entity::Entity;
use std::{collections::HashSet, time::Duration};

pub(super) const MAX_BODY_SCRIPT_REQUESTS: usize = 64;

#[derive(Clone)]
pub(super) struct RoomScriptTarget {
    pub(super) entity: Entity,
    pub(super) candidate_index: usize,
    pub(super) angles: [i32; 2],
    pub(super) health: bool,
    pub(super) messages: bool,
    pub(super) cue_listener: bool,
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
            ScriptEffect::Cue(_, _) => target.cue_listener,
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
    Cue(RoomCue, Entity),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlayerRoomCue {
    pub(crate) owner: u32,
    pub(crate) listener: BodyId,
    pub(crate) source: Entity,
    pub(crate) notice: CombatNotice,
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

    pub(crate) fn queue_cue(
        &mut self,
        frame: &RoomSpatialFrame,
        source: u32,
        targets: &[u32],
        cue: RoomCue,
    ) -> bool {
        if cue.stimulus == 0 || cue.line == 0 || targets.len() > RoomCue::MAX_TARGETS {
            return false;
        }
        let Some(source) = frame
            .script_targets
            .get(&source)
            .map(|target| target.entity)
        else {
            return false;
        };
        let mut seen = HashSet::with_capacity(targets.len());
        let staged: Vec<_> = targets
            .iter()
            .copied()
            .filter(|wire| seen.insert(*wire))
            .filter_map(|wire| {
                RoomScriptRequest::for_target(frame, wire, ScriptEffect::Cue(cue, source))
            })
            .collect();
        if staged.len() > MAX_BODY_SCRIPT_REQUESTS.saturating_sub(self.0.len()) {
            return false;
        }
        self.0.extend(staged);
        true
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
            ScriptEffect::Cue(cue, source_entity) => {
                if !self.room_cue_source_current(source, source_entity) {
                    return Ok(());
                }
                if let Some(player) = self
                    .world
                    .get::<PlayerBodyState>(request.entity)
                    .filter(|player| player.alive())
                {
                    self.player_cues.push(PlayerRoomCue {
                        owner: player.owner,
                        listener: player.id,
                        source: source_entity,
                        notice: CombatNotice::Voice {
                            anchor: source,
                            speaker: source,
                            listener: request.wire,
                            stimulus: cue.stimulus,
                            line: cue.line,
                        },
                    });
                }
            }
        }
        Ok(())
    }

    pub(crate) fn room_cue_source_current(&self, wire: u32, source: Entity) -> bool {
        self.by_wire
            .get(&wire)
            .or_else(|| self.players_by_wire.get(&wire))
            .is_some_and(|entity| *entity == source)
    }

    pub(crate) fn take_player_cues(&mut self) -> Vec<PlayerRoomCue> {
        let mut cues = std::mem::take(&mut self.player_cues);
        cues.retain(|cue| {
            cue.notice
                .required_identity()
                .is_some_and(|wire| self.room_cue_source_current(wire, cue.source))
                && self
                    .player_body(cue.listener)
                    .is_some_and(|player| player.owner == cue.owner && player.alive())
        });
        cues
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
    fn room_cues_reach_only_selected_live_player_generations() {
        use crate::{
            contact_world::ContactWorld, game_combat::tests::fixture_bots, mover::Mover,
            mover_settings::MoverSettings, script_graph::RoomSpatialNeeds,
        };

        let bots = fixture_bots();
        let speaker = bots[0].entity;
        let mut room = RoomSimulation::new(bots).unwrap();
        let first = BodyId {
            entity: 30,
            generation: 1,
        };
        let second = BodyId {
            entity: 31,
            generation: 1,
        };
        let outsider = BodyId {
            entity: 32,
            generation: 1,
        };
        let spawn = |room: &mut RoomSimulation, id, owner, slot| {
            let mover = Mover::new(
                MoverSettings::default(),
                Arc::new(ContactWorld::new(Vec::new()).unwrap()),
                [1.0, 0.0, 0.0],
                0,
                8,
            )
            .unwrap();
            room.spawn_player_body(PlayerBodyState::new(
                id, owner, slot, 1, 1, mover, None, 1, None,
            ))
            .unwrap();
        };
        spawn(&mut room, first, 2, 0);
        spawn(&mut room, second, 3, 1);
        spawn(&mut room, outsider, 4, 2);
        room.enable_spatial_queries(false, Arc::new(fixture_ground()), RoomSpatialNeeds::Queries);
        room.prepare_spatial_frame(1).unwrap();

        let frame = &room.spatial_frame.as_ref().unwrap().1;
        let source = frame.script_targets[&speaker].entity;
        let cue = RoomCue {
            stimulus: 41,
            line: 42,
        };
        let mut outbox = RoomScriptRequests::default();
        assert!(outbox.queue_cue(
            frame,
            speaker,
            &[first.entity, speaker, second.entity, first.entity],
            cue
        ));
        assert!(outbox.queue_cue(frame, speaker, &[second.entity], cue));
        assert!(outbox.queue_cue(frame, speaker, &[], cue));
        let [valid, expires_after_settlement, expires_before_settlement]: [RoomScriptRequest; 3] =
            outbox.take().try_into().ok().unwrap();
        assert_eq!(valid.wire, first.entity);
        assert_eq!(expires_after_settlement.wire, second.entity);

        for request in [valid, expires_after_settlement] {
            room.settle_script_request(request, speaker, 1, Duration::ZERO)
                .unwrap();
        }
        assert!(room.retire_player_body(second));
        spawn(
            &mut room,
            BodyId {
                generation: 2,
                ..second
            },
            3,
            1,
        );
        room.settle_script_request(expires_before_settlement, speaker, 1, Duration::ZERO)
            .unwrap();
        assert_eq!(
            room.take_player_cues(),
            [PlayerRoomCue {
                owner: 2,
                listener: first,
                source,
                notice: CombatNotice::Voice {
                    anchor: speaker,
                    speaker,
                    listener: first.entity,
                    stimulus: 41,
                    line: 42,
                },
            }]
        );
        assert!(room.take_player_cues().is_empty());

        room.prepare_spatial_frame(2).unwrap();
        let frame = &room.spatial_frame.as_ref().unwrap().1;
        assert!(outbox.queue_cue(frame, outsider.entity, &[first.entity], cue));
        let request = outbox.take().pop().unwrap();
        room.settle_script_request(request, outsider.entity, 2, Duration::ZERO)
            .unwrap();
        assert!(room.retire_player_body(outsider));
        spawn(
            &mut room,
            BodyId {
                generation: 2,
                ..outsider
            },
            4,
            2,
        );
        assert!(room.take_player_cues().is_empty());
    }

    #[test]
    fn invalid_batches_leave_mixed_outbox_work_and_capacity_intact() {
        let mut world = bevy_ecs::world::World::new();
        let mut target = |candidate_index, health, messages| RoomScriptTarget {
            entity: world.spawn_empty().id(),
            candidate_index,
            angles: [0; 2],
            health,
            messages,
            cue_listener: health,
        };
        let frame = RoomSpatialFrame {
            candidates: [1, 2, 3]
                .into_iter()
                .map(|entity| crate::script_expr::SpatialCandidate {
                    entity,
                    position: [0.0; 3],
                    team: None,
                })
                .collect(),
            actor_capsules: None,
            entity_facts: None,
            script_targets: HashMap::from([
                (1, target(0, true, false)),
                (2, target(1, true, false)),
                (3, target(2, false, true)),
            ]),
            player_wires: HashMap::new(),
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
        assert!(!outbox.queue_cue(
            &frame,
            3,
            &[2],
            RoomCue {
                stimulus: 9,
                line: 10
            }
        ));

        let queued = outbox.take();
        assert_eq!(queued.len(), MAX_BODY_SCRIPT_REQUESTS);
        assert!(matches!(queued[0].effect, ScriptEffect::Message(_)));
        assert_eq!(queued.last().unwrap().wire, 2);
    }
}
