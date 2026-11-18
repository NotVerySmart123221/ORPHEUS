//! Per-peer delivery, loading readiness, and combat UI state.

use super::*;

const MAX_PENDING_COMBAT_NOTICES: usize = 256;

pub(super) struct Peer {
    pub(super) ingress: GameIngress,
    pub(super) egress: GameEgress<Arc<Record>>,
    pub(super) loading: MapReadiness,
    pub(super) ownership: Option<u32>,
    pub(super) open: bool,
    pub(super) world: WorldDelivery,
    pub(super) combat_ui: CombatUi,
}

#[derive(Default)]
pub(super) struct CombatUi {
    pub(super) notices: VecDeque<CombatNotice>,
    pub(super) last_notice_by_entity: HashMap<u32, u32>,
    pub(super) replacements: VecDeque<(Duration, BotReplacement)>,
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
        self.combat_ui.notices.push_back(notice);
    }

    pub(super) fn flush_combat(
        &mut self,
        schemas: &Schemas,
        now: Duration,
    ) -> Result<(), RoomError> {
        for _ in 0..self.combat_ui.notices.len() {
            let notice = self
                .combat_ui
                .notices
                .pop_front()
                .ok_or(RoomError::Invariant("pending combat notice disappeared"))?;
            if let CombatNotice::Voice { listener, .. } = notice {
                if self.world.spawned_body() != Some(listener) {
                    continue;
                }
            }
            let entity = notice.required_identity();
            if entity.is_some_and(|entity| !self.world.identity_ready(entity))
                || (notice.needs_mode_ui() && !self.world.mode_ready())
            {
                self.combat_ui.notices.push_back(notice);
                continue;
            }
            let bytes = notice.encode(schemas)?;
            let ordinal = self.egress.queue_reliable(bytes)?;
            if let Some(entity) = entity {
                self.combat_ui.last_notice_by_entity.insert(entity, ordinal);
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
                .any(|notice| notice.required_identity() == Some(entity));
            let waiting_for_reliable = self
                .combat_ui
                .last_notice_by_entity
                .get(&entity)
                .is_some_and(|&ordinal| !self.egress.delivered_through(ordinal));
            if now < expires
                && (has_notice || waiting_for_reliable || self.world.health_pending(entity))
            {
                self.combat_ui
                    .replacements
                    .push_back((expires, replacement));
            } else {
                self.combat_ui
                    .notices
                    .retain(|notice| notice.required_identity() != Some(entity));
                self.combat_ui.last_notice_by_entity.remove(&entity);
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
