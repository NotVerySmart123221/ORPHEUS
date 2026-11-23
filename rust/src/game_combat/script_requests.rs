//! Deferred script operations for generation-checked room bodies.

use super::{
    BotState, CombatError, EliminationStreak, PlayerBodyState, RoomSimulation, RoomSpatialFrame,
    SampledBody, Vec3, health::HealthEffect,
};
use crate::{
    game_combat_messages::CombatNotice,
    game_replication::BodyId,
    script_init::{RoomBodyIdentity, RoomCue, RoomCueSpeaker, RoomMessage},
    vitals::Death,
    world::GAME_MODE_ENTITY,
};
use bevy_ecs::entity::Entity;
use std::{collections::HashSet, time::Duration};

pub(super) const MAX_BODY_SCRIPT_REQUESTS: usize = 64;

#[derive(Clone)]
pub(super) struct RoomScriptTarget {
    pub(super) entity: Entity,
    pub(super) candidate_index: usize,
    pub(super) angles: [i32; 2],
    pub(super) eye_position: Option<Vec3>,
    pub(super) filter_flags: Option<u64>,
    pub(super) health: bool,
    pub(super) messages: bool,
    pub(super) cue_listener: bool,
    pub(super) voice: Option<(u64, u64)>,
}

impl RoomSpatialFrame {
    pub(crate) fn actor_voice_available(&self, source: u32) -> bool {
        self.actor_voice.is_some()
            && self
                .script_targets
                .get(&source)
                .is_some_and(|body| body.voice.is_some())
    }

    pub(crate) fn actor_voice_request(
        &self,
        source: u32,
        criterion: u64,
        choose: &mut dyn FnMut(usize) -> usize,
    ) -> Option<(RoomCue, Vec<u32>)> {
        let catalog = self.actor_voice.as_ref()?;
        let (actor, source_body) = self.actor_sample(source)?;
        let (hero, skin) = source_body.voice?;
        let options = catalog.options(hero, skin, criterion)?;
        let listeners: Vec<_> = self
            .candidates
            .iter()
            .filter(|listener| {
                (listener.entity == source || actor.team.is_some() && actor.team == listener.team)
                    && self
                        .script_targets
                        .get(&listener.entity)
                        .is_some_and(|body| body.cue_listener)
            })
            .map(|listener| listener.entity)
            .collect();
        if listeners.is_empty() || listeners.len() > RoomCue::MAX_TARGETS {
            return None;
        }
        let (stimulus, line) = options.choose(choose)?;
        Some((
            RoomCue {
                stimulus,
                line,
                speaker: RoomCueSpeaker::Actor,
            },
            listeners,
        ))
    }
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
            ScriptEffect::Health(_) | ScriptEffect::Contact { .. } => target.health,
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
    Contact {
        source_frame: u32,
        damage: f64,
        impulse: Option<Vec3>,
    },
    Message(RoomMessage),
    Cue(RoomCue, Entity),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlayerRoomCue {
    pub(crate) owner: u32,
    pub(crate) listener: BodyId,
    pub(crate) source: RoomBodyIdentity,
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
        self.queue_batch(
            frame,
            changes.iter().map(|&(wire, delta)| {
                Some((wire, ScriptEffect::Health(HealthEffect::from_delta(delta)?)))
            }),
        )
    }

    pub(crate) fn queue_contact_damage(
        &mut self,
        frame: &RoomSpatialFrame,
        source_frame: u32,
        damage: f64,
        changes: &[(u32, Option<Vec3>)],
    ) -> bool {
        if !damage.is_finite() || damage < 0.0 {
            return false;
        }
        self.queue_batch(
            frame,
            changes.iter().map(|&(wire, impulse)| {
                let valid = impulse
                    .is_some_and(|delta| crate::mover::VelocityChange::Impulse(delta).valid());
                (if impulse.is_some() {
                    valid
                } else {
                    damage > 0.0
                })
                .then_some((
                    wire,
                    ScriptEffect::Contact {
                        source_frame,
                        damage,
                        impulse,
                    },
                ))
            }),
        )
    }

    fn queue_batch(
        &mut self,
        frame: &RoomSpatialFrame,
        changes: impl ExactSizeIterator<Item = Option<(u32, ScriptEffect)>>,
    ) -> bool {
        if changes.len() > MAX_BODY_SCRIPT_REQUESTS.saturating_sub(self.0.len()) {
            return false;
        }
        let mut targets = HashSet::with_capacity(changes.len());
        let mut staged = Vec::with_capacity(changes.len());
        for change in changes {
            let Some((wire, effect)) = change else {
                return false;
            };
            if !targets.insert(wire) {
                return false;
            }
            let Some(request) = RoomScriptRequest::for_target(frame, wire, effect) else {
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
        for (&wire, &entity) in &self.players_by_wire {
            if let Some(mut state) = self.world.get_mut::<PlayerBodyState>(entity) {
                if let Some(runtime) = state.runtime.as_mut() {
                    let requests = runtime.take_script_requests();
                    if !requests.is_empty() {
                        batches.push((SampledBody { wire, entity }, requests));
                    }
                }
            }
        }
        for (&wire, &entity) in &self.by_wire {
            if let Some(mut state) = self.world.get_mut::<BotState>(entity) {
                if let Some(enemy) = &mut state.enemy {
                    let requests = enemy.scripts.take_script_requests();
                    if !requests.is_empty()
                        && enemy.fault.is_none()
                        && !enemy.scripts.retirement_requested
                    {
                        batches.push((SampledBody { wire, entity }, requests));
                    }
                }
                if let Some(script) = &mut state.stock_script {
                    let requests = script.take_script_requests();
                    if !requests.is_empty() {
                        batches.push((SampledBody { wire, entity }, requests));
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
        source: SampledBody,
        tick: u32,
        now: Duration,
    ) -> Result<(), CombatError> {
        if !self.room_body_generation_current(request.wire, request.entity)
            || !self.room_body_generation_current(source.wire, source.entity)
        {
            return Ok(());
        }
        let death = Death {
            time: now.as_secs_f64(),
            frame: tick,
            killer: source.wire,
        };
        match request.effect {
            ScriptEffect::Health(effect) => {
                self.settle_health(request.wire, effect, death, now)?;
            }
            ScriptEffect::Contact {
                source_frame,
                damage,
                impulse,
            } => {
                if damage > 0.0 {
                    let settled = self.apply_health(
                        request.wire,
                        HealthEffect::Damage(damage),
                        death,
                        now,
                        None,
                    )?;
                    if let Some(outcome) = settled.filter(|outcome| outcome.delta < 0.0) {
                        self.record_contact_feedback(
                            source,
                            request.wire,
                            source_frame,
                            -outcome.delta,
                            outcome.killed,
                        );
                    }
                }
                if let Some(delta) = impulse {
                    self.queue_combat_impulse(request.wire, tick, delta)?;
                }
            }
            ScriptEffect::Message(message) => {
                self.route_room_message(request.entity, source.wire, message);
            }
            ScriptEffect::Cue(cue, source_entity) => {
                if source_entity != source.entity {
                    return Ok(());
                }
                if let Some(player) = self
                    .world
                    .get::<PlayerBodyState>(request.entity)
                    .filter(|player| player.alive())
                {
                    let speaker = match cue.speaker {
                        RoomCueSpeaker::Actor => source.wire,
                        RoomCueSpeaker::Mode => GAME_MODE_ENTITY,
                    };
                    self.player_cues.push(PlayerRoomCue {
                        owner: player.owner,
                        listener: player.id,
                        source: source.identity(),
                        notice: CombatNotice::Voice {
                            anchor: speaker,
                            speaker,
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

    fn record_contact_feedback(
        &mut self,
        source: SampledBody,
        target: u32,
        shot_frame: u32,
        dealt: f64,
        killed: bool,
    ) {
        let Some(shooter) = self.world.get::<PlayerBodyState>(source.entity) else {
            return;
        };
        let source = source.identity();
        let notices = [
            Some(CombatNotice::Hit {
                target,
                dealt,
                critical: false,
                killed,
                shot_frame,
            }),
            killed.then_some(CombatNotice::Elimination {
                killer: source.wire,
                victim: target,
            }),
        ];
        self.player_cues
            .extend(notices.into_iter().flatten().map(|notice| PlayerRoomCue {
                owner: shooter.owner,
                listener: shooter.id,
                source,
                notice,
            }));
    }

    pub(super) fn record_received_damage(
        &mut self,
        owner: u32,
        victim: BodyId,
        source: u32,
        dealt: f32,
    ) {
        if !self.received_damage_feedback
            || victim.entity == source
            || !dealt.is_finite()
            || dealt <= 0.0
        {
            return;
        }
        let Some(source) = self.sample_health_body(source) else {
            return;
        };
        self.player_cues.push(PlayerRoomCue {
            owner,
            listener: victim,
            source: source.identity(),
            notice: CombatNotice::DamageReceived {
                source: source.wire,
                victim: victim.entity,
                dealt,
            },
        });
    }

    pub(crate) fn room_cue_source_current(&self, source: RoomBodyIdentity) -> bool {
        self.sample_health_body(source.wire)
            .is_some_and(|body| body.identity() == source)
    }

    fn room_body_generation_current(&self, wire: u32, entity: Entity) -> bool {
        self.by_wire
            .get(&wire)
            .or_else(|| self.players_by_wire.get(&wire))
            == Some(&entity)
    }

    pub(crate) fn room_cue_listener_current(&self, owner: u32, listener: BodyId) -> bool {
        self.player_body(listener)
            .is_some_and(|player| player.owner == owner && player.alive())
    }

    pub(crate) fn take_player_cues(&mut self) -> Vec<PlayerRoomCue> {
        let mut cues = std::mem::take(&mut self.player_cues);
        cues.retain(|cue| {
            self.room_cue_source_current(cue.source)
                && self.room_cue_listener_current(cue.owner, cue.listener)
        });
        cues
    }

    pub(super) fn record_actor_hurt_voice(
        &mut self,
        speaker_wire: u32,
        is_bot: bool,
        critical: bool,
        killed: bool,
        frame: u32,
    ) {
        if !self.combat_voice.has_hurt_cues() || !crate::script_expr::valid_entity(speaker_wire) {
            return;
        }
        let prev = self.hurt_voice_history.get(&speaker_wire).copied();
        let cooldown = self.combat_voice.hurt_cooldown_ticks();
        if !killed
            && prev.is_some_and(|(last_frame, _)| frame.saturating_sub(last_frame) < cooldown)
        {
            return;
        }
        let pick = prev.map_or(0, |(_, count)| count);
        let body = if is_bot {
            self.by_wire.get(&speaker_wire).and_then(|&entity| {
                self.world
                    .get::<BotState>(entity)
                    .map(|state| state.bot.spawn.body)
            })
        } else {
            self.players_by_wire.get(&speaker_wire).and_then(|&entity| {
                self.world
                    .get::<PlayerBodyState>(entity)
                    .map(|state| state.body_template)
            })
        };
        let Some((stimulus, line)) = self
            .combat_voice
            .hurt_cue(body, is_bot, critical, killed, pick)
        else {
            return;
        };
        let last_frame = prev.map_or(frame, |(last, _)| last.max(frame));
        self.hurt_voice_history
            .insert(speaker_wire, (last_frame, pick.wrapping_add(1)));
        for (&listener_wire, &player_entity) in &self.players_by_wire {
            let Some(player) = self
                .world
                .get::<PlayerBodyState>(player_entity)
                .filter(|player| player.id.entity == listener_wire && player.alive())
            else {
                continue;
            };
            self.player_cues.push(PlayerRoomCue {
                owner: player.owner,
                listener: player.id,
                source: RoomBodyIdentity {
                    wire: listener_wire,
                    generation: player_entity.to_bits(),
                },
                notice: CombatNotice::Voice {
                    anchor: speaker_wire,
                    speaker: speaker_wire,
                    listener: listener_wire,
                    stimulus,
                    line,
                },
            });
        }
    }

    pub(super) fn record_enemy_elimination(
        &mut self,
        killer_wire: u32,
        victim_wire: u32,
        victim_team: u8,
        frame: u32,
    ) {
        if killer_wire == victim_wire || !crate::script_expr::valid_entity(killer_wire) {
            return;
        }
        let Some(&entity) = self.players_by_wire.get(&killer_wire) else {
            return;
        };
        let Some(player) = self
            .world
            .get::<PlayerBodyState>(entity)
            .filter(|player| player.id.entity == killer_wire && player.alive())
        else {
            return;
        };
        if !self.free_for_all && player.team == victim_team {
            return;
        }
        let owner = player.owner;
        let listener = player.id;
        let target = self.authored_elimination_target(victim_wire);
        let Some(mut player) = self.world.get_mut::<PlayerBodyState>(entity) else {
            return;
        };
        if let Some((target, runtime)) = target.zip(player.runtime.as_mut()) {
            let _ = runtime.receive_authored_elimination_feedback(target.into());
        }
        if self.combat_voice.is_empty() {
            return;
        }
        let window = self.combat_voice.multikill_window_ticks();
        let streak = player.elimination_streak.get_or_insert(EliminationStreak {
            last_frame: frame,
            count: 0,
        });
        let Some(gap) = frame.checked_sub(streak.last_frame) else {
            // Older settlements cannot rewind the announcer's window.
            return;
        };
        streak.count = if gap <= window {
            streak.count.saturating_add(1)
        } else {
            1
        };
        streak.last_frame = frame;
        let count = streak.count;
        if let Some((stimulus, line)) = self.combat_voice.multikill_cue(count, count as usize) {
            self.player_cues.push(PlayerRoomCue {
                owner,
                listener,
                source: RoomBodyIdentity {
                    wire: killer_wire,
                    generation: entity.to_bits(),
                },
                notice: CombatNotice::Voice {
                    anchor: GAME_MODE_ENTITY,
                    speaker: GAME_MODE_ENTITY,
                    listener: killer_wire,
                    stimulus,
                    line,
                },
            });
        }
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
                } else if let Some(script) = &mut state.stock_script {
                    script.receive_room_message(source, message);
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
        let sender = SampledBody {
            wire: speaker,
            entity: source,
        };
        let cue = RoomCue {
            stimulus: 41,
            line: 42,
            speaker: RoomCueSpeaker::Actor,
        };
        let mode_cue = RoomCue {
            speaker: RoomCueSpeaker::Mode,
            ..cue
        };
        let mut outbox = RoomScriptRequests::default();
        assert!(outbox.queue_cue(
            frame,
            speaker,
            &[first.entity, speaker, second.entity, first.entity],
            cue
        ));
        assert!(outbox.queue_cue(frame, speaker, &[second.entity], cue));
        assert!(outbox.queue_cue(frame, speaker, &[first.entity], mode_cue));
        assert!(outbox.queue_cue(frame, speaker, &[], cue));
        let [
            valid,
            expires_after_settlement,
            expires_before_settlement,
            mode,
        ]: [RoomScriptRequest; 4] = outbox.take().try_into().ok().unwrap();
        assert_eq!(valid.wire, first.entity);
        assert_eq!(expires_after_settlement.wire, second.entity);

        for request in [valid, expires_after_settlement, mode] {
            room.settle_script_request(request, sender, 1, Duration::ZERO)
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
        room.settle_script_request(expires_before_settlement, sender, 1, Duration::ZERO)
            .unwrap();
        assert_eq!(
            room.take_player_cues(),
            [
                PlayerRoomCue {
                    owner: 2,
                    listener: first,
                    source: sender.identity(),
                    notice: CombatNotice::Voice {
                        anchor: speaker,
                        speaker,
                        listener: first.entity,
                        stimulus: 41,
                        line: 42,
                    },
                },
                PlayerRoomCue {
                    owner: 2,
                    listener: first,
                    source: sender.identity(),
                    notice: CombatNotice::Voice {
                        anchor: GAME_MODE_ENTITY,
                        speaker: GAME_MODE_ENTITY,
                        listener: first.entity,
                        stimulus: 41,
                        line: 42,
                    },
                }
            ]
        );
        assert!(room.take_player_cues().is_empty());

        room.prepare_spatial_frame(2).unwrap();
        let frame = &room.spatial_frame.as_ref().unwrap().1;
        assert!(outbox.queue_cue(frame, outsider.entity, &[first.entity], mode_cue));
        let request = outbox.take().pop().unwrap();
        let outsider_sender = room.sample_health_body(outsider.entity).unwrap();
        room.settle_script_request(request, outsider_sender, 2, Duration::ZERO)
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
            eye_position: None,
            filter_flags: None,
            health,
            messages,
            cue_listener: health,
            voice: None,
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
            actor_voice: None,
            captured_tick: 0,
            debug_rays: Arc::default(),
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
                line: 10,
                speaker: RoomCueSpeaker::Actor,
            }
        ));

        let queued = outbox.take();
        assert_eq!(queued.len(), MAX_BODY_SCRIPT_REQUESTS);
        assert!(matches!(queued[0].effect, ScriptEffect::Message(_)));
        assert_eq!(queued.last().unwrap().wire, 2);
    }
}
