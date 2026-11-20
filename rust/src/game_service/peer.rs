//! Per-peer delivery, loading readiness, and combat UI state.

use bevy_ecs::entity::Entity;

use super::*;
use crate::{
    game_combat::{PlayerDeath, RoomSimulation},
    game_replication::BodyId,
};

const MAX_PENDING_COMBAT_NOTICES: usize = 256;

pub(super) struct Peer {
    pub(super) ingress: GameIngress,
    pub(super) egress: GameEgress<Arc<Record>>,
    pub(super) loading: MapReadiness,
    pub(super) ownership: Option<u32>,
    pub(super) open: bool,
    pub(super) world: WorldDelivery,
    pub(super) combat_ui: CombatUi,
    authored_cue_ordinal: Option<u32>,
    #[cfg(test)]
    pub(super) observed_receipts: Option<Vec<crate::game_egress::DeliveryReport<Arc<Record>>>>,
}

#[derive(Default)]
pub(super) struct CombatUi {
    notices: VecDeque<PendingCombatNotice>,
    replacements: VecDeque<(Duration, BotReplacement)>,
}

struct AuthoredCueIdentity {
    listener: BodyId,
    source: Entity,
}

struct PendingCombatNotice {
    notice: CombatNotice,
    audience: CombatAudience,
}

enum CombatAudience {
    Legacy,
    Victim(BodyId),
    Authored(AuthoredCueIdentity),
}

impl PendingCombatNotice {
    fn matches_audience(
        &self,
        body: Option<BodyId>,
        owner: u32,
        simulation: Option<&RoomSimulation>,
    ) -> bool {
        match &self.audience {
            CombatAudience::Legacy => match self.notice {
                CombatNotice::Voice { listener, .. } => {
                    body.is_some_and(|body| body.entity == listener)
                }
                _ => true,
            },
            CombatAudience::Victim(victim) => body == Some(*victim),
            CombatAudience::Authored(cue) => {
                let CombatNotice::Voice {
                    anchor, listener, ..
                } = self.notice
                else {
                    return false;
                };
                body == Some(cue.listener)
                    && cue.listener.entity == listener
                    && simulation.is_some_and(|room| {
                        room.room_cue_source_current(anchor, cue.source)
                            && room.room_cue_listener_current(owner, cue.listener)
                    })
            }
        }
    }
}

pub(super) fn initial_controller(
    scene: &PreparedScene,
    tick: u32,
) -> io::Result<crate::bits::BitWriter> {
    scene
        .controller()
        .controller_frame(
            tick,
            ControllerFrame {
                select_open: true,
                skins: !scene.map().is_junkenstein(),
                hide_assemble: !scene.map().is_junkenstein(),
                ..ControllerFrame::default()
            },
        )
        .map_err(invalid)
}

pub(super) fn script_record(
    entity: u32,
    payload: &crate::bits::BitWriter,
) -> io::Result<EntityUpdate> {
    Ok(EntityUpdate {
        entity,
        chunk: Some(chunk(payload, 0, 1).map_err(invalid)?),
        ..EntityUpdate::default()
    })
}

pub(crate) fn player_entity(slot: usize) -> u32 {
    0xa000_0000 | ((slot as u32 + 1) << 8)
}

impl Peer {
    pub(super) fn new(owner: u32) -> Self {
        Self {
            ingress: GameIngress::default(),
            egress: GameEgress::default(),
            loading: MapReadiness::default(),
            ownership: None,
            open: false,
            world: WorldDelivery::new(owner),
            combat_ui: CombatUi::default(),
            authored_cue_ordinal: None,
            #[cfg(test)]
            observed_receipts: None,
        }
    }

    pub(super) fn clear_world(&mut self) {
        self.world.clear();
        self.combat_ui = CombatUi::default();
    }

    pub(super) fn clear_world_in_room(
        &mut self,
        simulation: &mut crate::game_combat::RoomSimulation,
    ) {
        self.world.clear_in_room(simulation);
        self.combat_ui = CombatUi::default();
    }

    pub(super) fn bot_hit(&mut self, hit: BotHit) {
        self.world.queue_health(hit.health);
        for notice in hit.notices.into_iter().flatten() {
            self.queue_combat_notice(notice);
        }
    }

    pub(super) fn queue_bot_replacement(&mut self, replacement: BotReplacement, expires: Duration) {
        if let Some((_, pending)) = self
            .combat_ui
            .replacements
            .iter_mut()
            .find(|(_, pending)| pending.created.entity == replacement.retired)
        {
            // Intermediate generations held here were never published to this peer.
            pending.created = replacement.created;
        } else if self.world.tracks_current_bot(replacement.retired) {
            self.combat_ui
                .replacements
                .push_back((expires, replacement));
        }
    }

    pub(super) fn queue_combat_notice(&mut self, notice: CombatNotice) {
        self.queue_notice(notice, CombatAudience::Legacy);
    }

    pub(super) fn queue_player_death(&mut self, death: PlayerDeath) {
        if self.world.owner() == death.owner && self.world.active_body_id() == Some(death.body) {
            self.queue_notice(
                CombatNotice::Death {
                    source: death.source,
                    killed_by: death.killed_by,
                },
                CombatAudience::Victim(death.body),
            );
        }
    }

    fn queue_notice(&mut self, notice: CombatNotice, audience: CombatAudience) {
        if self.combat_ui.notices.len() == MAX_PENDING_COMBAT_NOTICES {
            self.combat_ui.notices.pop_front();
        }
        self.combat_ui
            .notices
            .push_back(PendingCombatNotice { notice, audience });
    }

    pub(super) fn queue_player_cue(
        &mut self,
        listener: BodyId,
        source: Entity,
        notice: CombatNotice,
    ) {
        if self.combat_ui.notices.len() == MAX_PENDING_COMBAT_NOTICES {
            return;
        }
        self.combat_ui.notices.push_back(PendingCombatNotice {
            notice,
            audience: CombatAudience::Authored(AuthoredCueIdentity { listener, source }),
        });
    }

    #[cfg(test)]
    pub(super) fn pending_combat_notices(&self) -> impl Iterator<Item = &CombatNotice> {
        self.combat_ui.notices.iter().map(|pending| &pending.notice)
    }

    pub(super) fn flush_combat(
        &mut self,
        schemas: &Schemas,
        now: Duration,
        simulation: Option<&RoomSimulation>,
    ) -> Result<(), RoomError> {
        for _ in 0..self.combat_ui.notices.len() {
            let pending = self
                .combat_ui
                .notices
                .pop_front()
                .ok_or(RoomError::Invariant("pending combat notice disappeared"))?;
            if !pending.matches_audience(
                self.world.active_body_id(),
                self.world.owner(),
                simulation,
            ) {
                continue;
            }
            let notice = pending.notice;
            let authored = match &pending.audience {
                CombatAudience::Authored(cue) => Some(cue),
                _ => None,
            };
            if authored.is_some_and(|cue| {
                !self.world.identity_ready(cue.listener.entity)
                    || self.authored_cue_ordinal.is_some_and(|ordinal| {
                        self.egress.oldest_pending_reliable() <= u64::from(ordinal)
                    })
            }) {
                self.combat_ui.notices.push_back(pending);
                continue;
            }
            if notice
                .required_identity()
                .is_some_and(|entity| !self.world.identity_ready(entity))
                || (notice.needs_mode_ui() && !self.world.mode_ready())
            {
                self.combat_ui.notices.push_back(pending);
                continue;
            }
            let bytes = notice.encode(schemas)?;
            let ordinal = self.egress.queue_reliable(bytes)?;
            if authored.is_some() {
                self.authored_cue_ordinal = Some(ordinal);
            }
            for entity in notice.referenced_entities() {
                self.world.track_entity_notice(entity, ordinal);
            }
        }
        for _ in 0..self.combat_ui.replacements.len() {
            let (expires, replacement) = self
                .combat_ui
                .replacements
                .pop_front()
                .ok_or(RoomError::Invariant("pending bot replacement disappeared"))?;
            let entity = replacement.retired;
            if !self.world.tracks_current_bot(entity) {
                continue;
            }
            let has_notice = self
                .combat_ui
                .notices
                .iter()
                .any(|pending| pending.notice.required_identity() == Some(entity));
            let waiting_for_reliable = self.world.pending_entity_notice(entity, &self.egress);
            // A live replacement can itself have a pending create behind the
            // earlier generation's destroy. Keep that ordering across respawns.
            if !self.world.identity_ready(entity)
                || (now < expires
                    && (has_notice || waiting_for_reliable || self.world.health_pending(entity)))
            {
                self.combat_ui
                    .replacements
                    .push_back((expires, replacement));
            } else {
                self.combat_ui
                    .notices
                    .retain(|pending| pending.notice.required_identity() != Some(entity));
                self.world.replace_bot(replacement);
            }
        }
        Ok(())
    }

    pub(super) fn reliable(
        &mut self,
        schemas: &Schemas,
        message: Message,
    ) -> Result<u32, RoomError> {
        let bytes = encode(schemas, message.id, &message.fields)?;
        Ok(self.egress.queue_reliable(bytes)?)
    }

    pub(super) fn advance_loading(
        &mut self,
        schemas: &Schemas,
        room: &Room,
        slot: usize,
        now: Duration,
        tick: u32,
    ) -> Result<(), RoomError> {
        let player = room.players.get(slot).ok_or(RoomError::Invariant(
            "loading member has invalid roster slot",
        ))?;
        let ready = WorldReadiness {
            ownership_delivered: self
                .ownership
                .is_some_and(|id| self.egress.delivered_through(id)),
            collision_ready: room.world.is_some(),
            bot_scripts_ready: room
                .world
                .as_ref()
                .is_some_and(|world| world.scene.scripted_bots() == 0),
        };
        while let Some(step) = self.loading.take_due(now, ready) {
            match step {
                MapReadyStep::Ownership => {
                    self.ownership = Some(self.reliable(schemas, ownership(player.entity))?);
                }
                MapReadyStep::Game => {
                    self.reliable(schemas, room.message(slot, false)?)?;
                }
                MapReadyStep::World => {
                    let world = room
                        .world
                        .as_ref()
                        .ok_or(RoomError::Invariant("map-entry prerequisites missing"))?;
                    let viewer = room
                        .appearance(slot)
                        .ok_or(RoomError::Invariant("prepared player missing"))?;
                    self.world.start(&world.scene, &viewer, tick)?;
                }
                MapReadyStep::Controller => self.world.presentation_due = true,
            }
        }
        if self.world.presentation_due {
            let world = room
                .world
                .as_ref()
                .ok_or(RoomError::Invariant("started world missing"))?;
            self.world.present(&world.scene, tick)?;
        }
        if self.world.world_started() {
            let world = room
                .world
                .as_ref()
                .ok_or(RoomError::Invariant("started scene scripts have no world"))?;
            self.world.present_scene_scripts(&world.scene)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_cues_preserve_legacy_notice_matching_and_capacity() {
        let listener = BodyId {
            entity: 0xa001_0100,
            generation: 4,
        };
        let replacement = BodyId {
            generation: 5,
            ..listener
        };
        let notice = CombatNotice::Voice {
            anchor: 0xc001_0000,
            speaker: 0xc001_0000,
            listener: listener.entity,
            stimulus: 1,
            line: 2,
        };
        let source = bevy_ecs::world::World::new().spawn_empty().id();
        let mut peer = Peer::new(0xa001_0000);
        peer.queue_player_cue(listener, source, notice);
        peer.queue_combat_notice(notice);

        let legacy = &peer.combat_ui.notices[1];
        assert!(legacy.matches_audience(Some(replacement), peer.world.owner(), None));

        // With no current body, flushing discards both before reliable admission.
        let schemas = Schemas::from_json("{}").unwrap();
        peer.flush_combat(&schemas, Duration::ZERO, None).unwrap();
        assert!(peer.combat_ui.notices.is_empty());

        let legacy = CombatNotice::Death {
            source: 0,
            killed_by: false,
        };
        for _ in 0..MAX_PENDING_COMBAT_NOTICES {
            peer.queue_combat_notice(legacy);
        }
        peer.queue_player_cue(listener, source, notice);
        assert_eq!(peer.combat_ui.notices.len(), MAX_PENDING_COMBAT_NOTICES);
        assert!(
            peer.pending_combat_notices()
                .all(|notice| *notice == legacy)
        );
    }

    #[test]
    fn queued_cue_requires_a_living_listener_at_reliable_admission() {
        use crate::{
            contact_world::ContactWorld, game_combat::PlayerBodyState, game_heroes::SOLDIER,
            game_replication::OwnerBodyRuntime, mover::Mover, mover_settings::MoverSettings,
            script_player::fixture_body_factory, vitals::Death,
        };
        use serde_json::{Value as Json, json};

        let fixture: Json = serde_json::from_str(include_str!(
            "../../../fixtures/behavior/script-room-cue-v1.json"
        ))
        .unwrap();
        let input = &fixture["input"]["reliable_admission"];
        let expected = &fixture["expected"]["reliable_admission"];
        let speaker = u32::try_from(fixture["input"]["speaker"].as_u64().unwrap()).unwrap();
        let (factory, spawn) = fixture_body_factory();
        let listener = BodyId {
            entity: spawn.entity,
            generation: input["listener_generation"].as_u64().unwrap(),
        };
        let mut simulation = RoomSimulation::new(Vec::new()).unwrap();
        let movement_world = Arc::new(ContactWorld::new(Vec::new()).unwrap());
        let body = |id, owner, slot, runtime: Option<Box<dyn OwnerBodyRuntime>>| {
            let mover = Mover::new(
                MoverSettings::default(),
                Arc::clone(&movement_world),
                spawn.pose.position,
                0,
                0x08,
            )
            .unwrap();
            PlayerBodyState::new(
                id,
                owner,
                slot,
                1,
                SOLDIER,
                mover,
                runtime,
                spawn.frame,
                None,
            )
        };
        let source = simulation
            .spawn_player_body(body(
                BodyId {
                    entity: speaker,
                    generation: 1,
                },
                speaker - 1,
                1,
                None,
            ))
            .unwrap();
        simulation
            .spawn_player_body(body(
                listener,
                spawn.controller,
                0,
                Some(Box::new(factory.start(spawn).unwrap())),
            ))
            .unwrap();

        let notice = CombatNotice::Voice {
            anchor: speaker,
            speaker,
            listener: listener.entity,
            stimulus: 1,
            line: 2,
        };
        let mut peer = Peer::new(spawn.controller);
        peer.queue_player_cue(listener, source.entity, notice);
        peer.queue_combat_notice(notice);
        let eligibility = |room: &RoomSimulation| {
            let matches = |index: usize| {
                peer.combat_ui.notices[index].matches_audience(
                    Some(listener),
                    peer.world.owner(),
                    Some(room),
                )
            };
            json!({"authored": matches(0), "legacy": matches(1)})
        };
        assert_eq!(eligibility(&simulation), expected["before_listener_death"]);
        assert!(!peer.combat_ui.notices[0].matches_audience(
            Some(BodyId {
                generation: listener.generation + 1,
                ..listener
            }),
            peer.world.owner(),
            Some(&simulation),
        ));
        assert!(!peer.combat_ui.notices[0].matches_audience(
            Some(listener),
            speaker - 1,
            Some(&simulation),
        ));

        let (_, killed, _) = simulation
            .damage_player_body_by_wire(
                listener,
                input["late_damage"].as_f64().unwrap(),
                Death {
                    killer: speaker,
                    frame: spawn.frame + 1,
                    time: 0.016,
                },
                None,
            )
            .unwrap();
        assert!(killed);
        assert_eq!(
            simulation.player_handle_for_wire(listener.entity),
            Some(listener)
        );
        assert_eq!(eligibility(&simulation), expected["after_listener_death"]);
    }
}
