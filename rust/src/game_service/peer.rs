//! Per-peer delivery, loading readiness, and combat UI state.

use bevy_ecs::entity::Entity;

use super::*;
use crate::{game_combat::RoomSimulation, game_replication::BodyId};

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
    pub(super) replacements: VecDeque<(Duration, BotReplacement)>,
}

struct AuthoredCueIdentity {
    listener: BodyId,
    source: Entity,
}

struct PendingCombatNotice {
    notice: CombatNotice,
    authored: Option<AuthoredCueIdentity>,
}

impl PendingCombatNotice {
    fn matches_body(&self, body: Option<BodyId>) -> bool {
        if self
            .authored
            .as_ref()
            .is_some_and(|cue| body != Some(cue.listener))
        {
            return false;
        }
        match self.notice {
            CombatNotice::Voice { listener, .. } => {
                body.is_some_and(|body| body.entity == listener)
            }
            _ => true,
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

    pub(super) fn queue_combat_notice(&mut self, notice: CombatNotice) {
        if self.combat_ui.notices.len() == MAX_PENDING_COMBAT_NOTICES {
            self.combat_ui.notices.pop_front();
        }
        self.combat_ui.notices.push_back(PendingCombatNotice {
            notice,
            authored: None,
        });
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
            authored: Some(AuthoredCueIdentity { listener, source }),
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
            if !pending.matches_body(self.world.active_body_id()) {
                continue;
            }
            let notice = pending.notice;
            if let Some(cue) = &pending.authored {
                let CombatNotice::Voice { anchor, .. } = notice else {
                    continue;
                };
                if !simulation.is_some_and(|room| room.room_cue_source_current(anchor, cue.source))
                {
                    continue;
                }
                if !self.world.identity_ready(cue.listener.entity)
                    || self.authored_cue_ordinal.is_some_and(|ordinal| {
                        self.egress.oldest_pending_reliable() <= u64::from(ordinal)
                    })
                {
                    self.combat_ui.notices.push_back(pending);
                    continue;
                }
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
            if pending.authored.is_some() {
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
            let has_notice = self
                .combat_ui
                .notices
                .iter()
                .any(|pending| pending.notice.required_identity() == Some(entity));
            let waiting_for_reliable = self.world.pending_entity_notice(entity, &self.egress);
            if now < expires
                && (has_notice || waiting_for_reliable || self.world.health_pending(entity))
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
    fn queued_player_cue_requires_the_original_body_generation() {
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

        let authored = &peer.combat_ui.notices[0];
        assert!(authored.matches_body(Some(listener)));
        assert!(!authored.matches_body(Some(replacement)));
        let legacy = &peer.combat_ui.notices[1];
        assert!(legacy.matches_body(Some(replacement)));

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
}
