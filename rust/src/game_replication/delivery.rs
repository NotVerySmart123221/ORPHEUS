use super::{body::*, *};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Lifetime {
    Live,
    Retiring,
    ReusableAt(Duration),
}

#[derive(Clone, Copy)]
pub(super) enum Delivery {
    Baseline,
    BotCreate {
        after: Option<u32>,
    },
    Health {
        revision: u64,
    },
    BotDestroy,
    RemotePlayerCreate(BodyId),
    RemotePlayerDestroy(BodyId),
    CreateBody(BodyId),
    Possession(BodyId),
    DestroyBody(BodyId),
    BodyScript {
        #[cfg(test)]
        rifle_ammo_written: Option<i64>,
        id: BodyId,
        last: u32,
        delta: bool,
    },
    SceneScript {
        last: u32,
    },
    ProjectileConfirm {
        id: BodyId,
        token: ProjectileToken,
    },
    ProjectileDestroy {
        id: BodyId,
        token: ProjectileToken,
    },
    BioticCreate,
    BioticFrame {
        last: u32,
    },
    BioticDestroy,
    Script {
        last: u32,
        close: bool,
        resend: bool,
    },
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum BotLifetime {
    Live,
    Retiring,
    RetiringEnemy,
}

impl WorldDelivery {
    pub(crate) fn new(owner: u32) -> Self {
        Self {
            owner,
            queued: Vec::new(),
            created: HashSet::new(),
            bots: HashMap::new(),
            health_updates: HashMap::new(),
            health_revision: 0,
            entity_notice_ordinals: HashMap::new(),
            started: None,
            presentation_due: false,
            controller: Stream::new(owner),
            mode: Stream::new(GAME_MODE_ENTITY),
            countdown: None,
            encounter_post_match_queued: false,
            encounter_screens: None,
            encounter_notifications: None,
            encounter_notification_turn: true,
            encounter_phases: BTreeMap::new(),
            encounter_result_queued: false,
            encounter_outcome_queued: false,
            select_open: true,
            body: None,
            bound: None,
            generation: 0,
            bodies: HashMap::new(),
            remote_players: HashMap::new(),
            input: InputQueue::default(),
            body_script: None,
            detached_motion: None,
            #[cfg(test)]
            owner_script_fault: None,
            projectile_destroys: HashMap::new(),
            biotic_views: BTreeMap::new(),
            scene_views: BTreeMap::new(),
            bot_chunk_waited: false,
            gate: None,
            next_biotic_view: FIRST_BIOTIC_VIEW,
        }
    }

    pub(crate) fn owner(&self) -> u32 {
        self.owner
    }

    pub(crate) fn clear(&mut self) {
        self.queued.clear();
        self.created.clear();
        self.bots.clear();
        self.remote_players.clear();
        self.health_updates.clear();
        self.started = None;
        self.presentation_due = false;
        self.countdown = None;
        self.encounter_post_match_queued = false;
        self.encounter_screens = None;
        self.encounter_notifications = None;
        self.encounter_notification_turn = true;
        self.encounter_phases.clear();
        self.encounter_result_queued = false;
        self.encounter_outcome_queued = false;
        self.body_script = None;
        self.detached_motion = None;
        self.projectile_destroys.clear();
        self.biotic_views.clear();
        self.scene_views.clear();
        self.bot_chunk_waited = false;
        self.gate = None;
        self.input.clear();
    }

    pub(crate) fn clear_in_room(&mut self, simulation: &mut RoomSimulation) {
        simulation.retire_player_owner(self.owner);
        self.clear();
    }

    pub(super) fn push(&mut self, record: Record) {
        self.queued.push(Arc::new(record));
    }

    pub(crate) fn world_started(&self) -> bool {
        self.started.is_some()
    }

    pub(crate) fn spawned_body(&self) -> Option<u32> {
        self.body
            .filter(|_| self.world_started())
            .map(|body| body.id.entity)
    }

    pub(crate) fn active_body_id(&self) -> Option<BodyId> {
        self.body
            .filter(|_| self.world_started())
            .map(|body| body.id)
    }

    pub(crate) fn identity_ready(&self, entity: u32) -> bool {
        self.world_started() && self.created.contains(&entity)
    }

    pub(crate) fn track_entity_notice(&mut self, entity: u32, ordinal: u32) {
        self.entity_notice_ordinals.insert(entity, ordinal);
    }

    pub(crate) fn pending_entity_notice(
        &self,
        entity: u32,
        egress: &GameEgress<Arc<Record>>,
    ) -> bool {
        self.entity_notice_ordinals
            .get(&entity)
            .is_some_and(|&ordinal| !egress.delivered_through(ordinal))
    }

    pub(crate) fn mode_ready(&self) -> bool {
        self.mode.delivered() != 0
    }

    pub(crate) fn health_pending(&self, entity: u32) -> bool {
        self.health_updates.contains_key(&entity)
    }

    pub(crate) fn tracks_current_bot(&self, entity: u32) -> bool {
        self.bots.get(&entity) == Some(&BotLifetime::Live)
    }

    pub(crate) fn queue_health(&mut self, update: EntityUpdate) {
        let entity = update.entity;
        let live_bot = self.tracks_current_bot(entity);
        let live_remote = self
            .remote_players
            .get(&entity)
            .is_some_and(|&(_, life)| life == BotLifetime::Live);
        let live_body = self.body.is_some_and(|body| {
            body.id.entity == entity
                && self.bodies.get(&entity) == Some(&(body.id.generation, Lifetime::Live))
        });
        if !live_bot && !live_remote && !live_body {
            return;
        }
        self.health_revision = self.health_revision.wrapping_add(1);
        let revision = self.health_revision;
        self.health_updates.insert(entity, revision);
        self.remove_stale();
        self.push(Record {
            update,
            delivery: Delivery::Health { revision },
        });
    }

    pub(crate) fn create_remote_player(&mut self, id: BodyId, update: EntityUpdate) {
        if !self.world_started() || self.remote_players.contains_key(&id.entity) {
            return;
        }
        self.remote_players
            .insert(id.entity, (id, BotLifetime::Live));
        self.push(Record {
            update,
            delivery: Delivery::RemotePlayerCreate(id),
        });
    }

    pub(crate) fn retire_remote_player(&mut self, id: BodyId) {
        if self.remote_players.get(&id.entity) != Some(&(id, BotLifetime::Live)) {
            return;
        }
        let unsent_create = !self.created.contains(&id.entity)
            && self.queued.iter().any(
                |record| matches!(record.delivery, Delivery::RemotePlayerCreate(qid) if qid == id),
            );
        self.health_updates.remove(&id.entity);
        self.scene_views.remove(&id.entity);
        if unsent_create {
            self.remote_players.remove(&id.entity);
            self.remove_stale();
        } else {
            self.remote_players
                .insert(id.entity, (id, BotLifetime::RetiringEnemy));
            self.remove_stale();
            self.push(Record {
                update: EntityUpdate {
                    entity: id.entity,
                    operation: Some(Operation::Destroy),
                    ..Default::default()
                },
                delivery: Delivery::RemotePlayerDestroy(id),
            });
        }
    }

    pub(crate) fn create_bot(&mut self, update: EntityUpdate) {
        if !self.world_started() || self.bots.contains_key(&update.entity) {
            return;
        }
        self.bots.insert(update.entity, BotLifetime::Live);
        self.push(Record {
            update,
            delivery: Delivery::BotCreate { after: None },
        });
    }

    pub(crate) fn replace_bot(&mut self, replacement: BotReplacement) {
        let old = replacement.retired;
        let new = replacement.created.entity;
        if !self.tracks_current_bot(old) || self.bots.contains_key(&new) {
            return;
        }
        self.bots.insert(old, BotLifetime::Retiring);
        self.bots.insert(new, BotLifetime::Live);
        self.created.remove(&old);
        self.health_updates.remove(&old);
        self.remove_stale();
        self.push(Record {
            update: EntityUpdate {
                entity: old,
                operation: Some(Operation::Destroy),
                ..EntityUpdate::default()
            },
            delivery: Delivery::BotDestroy,
        });
        self.push(Record {
            update: replacement.created,
            delivery: Delivery::BotCreate { after: Some(old) },
        });
    }

    pub(crate) fn retire_enemy(&mut self, entity: u32) {
        if self.bots.get(&entity) != Some(&BotLifetime::Live) {
            return;
        }
        self.bots.insert(entity, BotLifetime::RetiringEnemy);
        self.health_updates.remove(&entity);
        self.scene_views.remove(&entity);
        self.remove_stale();
        self.push(Record {
            update: EntityUpdate {
                entity,
                operation: Some(Operation::Destroy),
                ..Default::default()
            },
            delivery: Delivery::BotDestroy,
        });
    }

    fn current(&self, record: &Record) -> bool {
        let entity = record.update.entity;
        match record.delivery {
            Delivery::Baseline | Delivery::Script { close: false, .. } => true,
            Delivery::BotCreate { .. } => {
                self.bots.get(&entity) == Some(&BotLifetime::Live)
                    || (self.bots.get(&entity) == Some(&BotLifetime::RetiringEnemy)
                        && !self.created.contains(&entity))
            }
            Delivery::BotDestroy => matches!(
                self.bots.get(&entity),
                Some(BotLifetime::Retiring | BotLifetime::RetiringEnemy)
            ),
            Delivery::RemotePlayerCreate(id) => {
                matches!(
                    self.remote_players.get(&id.entity),
                    Some(&(current, BotLifetime::Live)) if current == id
                ) || (matches!(
                    self.remote_players.get(&id.entity),
                    Some(&(current, BotLifetime::RetiringEnemy)) if current == id
                ) && !self.created.contains(&id.entity))
            }
            Delivery::RemotePlayerDestroy(id) => matches!(
                self.remote_players.get(&id.entity),
                Some(&(current, BotLifetime::RetiringEnemy)) if current == id
            ),
            Delivery::SceneScript { last } => {
                (self.gate == Some(entity)
                    || self.bots.get(&entity) == Some(&BotLifetime::Live)
                    || self
                        .remote_players
                        .get(&entity)
                        .is_some_and(|&(_, life)| life == BotLifetime::Live))
                    && self.scene_views.get(&entity).is_some_and(|stream| {
                        stream.pending().any(|(pending, _, _)| pending == last)
                    })
            }
            Delivery::Health { revision } => self.health_updates.get(&entity) == Some(&revision),
            Delivery::Script {
                last, close: true, ..
            } => last == self.controller.latest_data(),
            Delivery::CreateBody(id) => {
                self.bodies.get(&id.entity) == Some(&(id.generation, Lifetime::Live))
            }
            Delivery::DestroyBody(id) => {
                self.bodies.get(&id.entity) == Some(&(id.generation, Lifetime::Retiring))
            }
            Delivery::Possession(id) => self.body.is_some_and(|body| body.id == id),
            Delivery::BodyScript { id, .. } => self
                .body_script
                .as_ref()
                .is_some_and(|script| script.id == id),
            Delivery::ProjectileConfirm { id, token } => self
                .body_script
                .as_ref()
                .filter(|script| script.id == id)
                .and_then(|script| script.projectiles.as_ref())
                .is_some_and(|projectiles| projectiles.confirmation_current(token, entity)),
            Delivery::ProjectileDestroy { id, token } => {
                self.projectile_destroys.get(&entity) == Some(&(id, token))
            }
            Delivery::BioticCreate => self
                .biotic_views
                .get(&entity)
                .is_some_and(|view| !view.retiring && !view.created),
            Delivery::BioticFrame { last } => self.biotic_views.get(&entity).is_some_and(|view| {
                !view.retiring
                    && view.stream.latest_data() == last
                    && view.stream.delivered() < last
            }),
            Delivery::BioticDestroy => self
                .biotic_views
                .get(&entity)
                .is_some_and(|view| view.retiring),
        }
    }

    pub(crate) fn acknowledge(&mut self, report: DeliveryReport<Arc<Record>>, now: Duration) {
        if self.started.is_none() {
            return;
        }
        let mut retry = Vec::new();
        for packet in report.packets {
            for record in packet.records {
                if !self.current(&record) {
                    continue;
                }
                match (packet.arrived, record.delivery) {
                    (arrived, Delivery::SceneScript { last }) => {
                        let stream = self
                            .scene_views
                            .get_mut(&record.update.entity)
                            .expect("current scene stream");
                        let delivery = if arrived {
                            stream.arrived(last)
                        } else {
                            stream.lost(last)
                        };
                        if delivery.retry {
                            retry.push(record);
                        }
                    }
                    (arrived, Delivery::BodyScript { last, .. }) => {
                        let script = self.body_script.as_mut().expect("current body stream");
                        let delivery = if arrived {
                            script.stream.arrived(last)
                        } else {
                            script.stream.lost(last)
                        };
                        if let Some((projectiles, arrived)) =
                            script.projectiles.as_mut().zip(delivery.owner_report)
                        {
                            projectiles.owner_arrival(last, arrived);
                        }
                        if delivery.retry {
                            retry.push(record);
                        }
                    }
                    (false, Delivery::Script { resend: false, .. }) => {}
                    (false, _) => retry.push(record),
                    (true, Delivery::Baseline) => {
                        if matches!(
                            record.update.operation,
                            Some(Operation::Create(_) | Operation::Placeable(_))
                        ) {
                            self.created.insert(record.update.entity);
                        }
                    }
                    (true, Delivery::BotCreate { .. }) => {
                        self.created.insert(record.update.entity);
                        if let Some(stream) = self.scene_views.get_mut(&record.update.entity) {
                            stream.create_arrived();
                        }
                    }
                    (true, Delivery::BotDestroy) => {
                        self.bots.remove(&record.update.entity);
                        self.created.remove(&record.update.entity);
                        self.scene_views.remove(&record.update.entity);
                    }
                    (true, Delivery::RemotePlayerCreate(id)) => {
                        self.created.insert(id.entity);
                        if let Some(stream) = self.scene_views.get_mut(&id.entity) {
                            stream.create_arrived();
                        }
                    }
                    (true, Delivery::RemotePlayerDestroy(id)) => {
                        if self.remote_players.get(&id.entity)
                            == Some(&(id, BotLifetime::RetiringEnemy))
                        {
                            self.remote_players.remove(&id.entity);
                        }
                        self.created.remove(&id.entity);
                        self.scene_views.remove(&id.entity);
                    }
                    (true, Delivery::Health { .. }) => {
                        self.health_updates.remove(&record.update.entity);
                    }
                    (true, Delivery::CreateBody(id)) => {
                        self.created.insert(id.entity);
                        if let Some(script) =
                            self.body_script.as_mut().filter(|script| script.id == id)
                        {
                            script.stream.create_arrived();
                        }
                    }
                    (true, Delivery::Possession(id)) => self.bound = Some(id),
                    (true, Delivery::DestroyBody(id)) => {
                        self.bodies.insert(
                            id.entity,
                            (
                                id.generation,
                                Lifetime::ReusableAt(now.saturating_add(BODY_REUSE_DELAY)),
                            ),
                        );
                    }
                    (true, Delivery::ProjectileConfirm { token, .. }) => {
                        self.body_script
                            .as_mut()
                            .and_then(|script| script.projectiles.as_mut())
                            .expect("current projectile tracker")
                            .create_arrived(token, record.update.entity);
                    }
                    (true, Delivery::ProjectileDestroy { .. }) => {
                        self.projectile_destroys.remove(&record.update.entity);
                    }
                    (true, Delivery::BioticCreate) => {
                        self.biotic_views
                            .get_mut(&record.update.entity)
                            .expect("current field view")
                            .created = true;
                    }
                    (true, Delivery::BioticFrame { last }) => {
                        self.biotic_views
                            .get_mut(&record.update.entity)
                            .expect("current field view")
                            .stream
                            .arrived(last);
                    }
                    (true, Delivery::BioticDestroy) => {
                        self.biotic_views.remove(&record.update.entity);
                    }
                    (true, Delivery::Script { last, .. }) if record.update.entity == self.owner => {
                        self.controller.arrived(last);
                    }
                    (true, Delivery::Script { last, .. }) => {
                        self.mode.arrived(last);
                    }
                }
            }
        }
        if !retry.is_empty() {
            retry.append(&mut self.queued);
            self.queued = retry;
            self.remove_stale();
        }
    }

    pub(super) fn remove_stale(&mut self) {
        let mut queued = std::mem::take(&mut self.queued);
        queued.retain(|record| self.current(record));
        self.queued = queued;
    }

    pub(crate) fn ready(&self) -> Vec<Ready<'_>> {
        let body = self.body.map(|body| body.id);
        let mut created = body.is_none_or(|id| self.created.contains(&id.entity));
        let mut bound = body == self.bound;
        let mut destroyed = HashSet::new();
        self.queued
            .iter()
            .enumerate()
            .filter_map(|(index, record)| {
                if matches!(record.update.operation, Some(Operation::Destroy))
                    && self
                        .entity_notice_ordinals
                        .contains_key(&record.update.entity)
                {
                    return None;
                }
                match record.delivery {
                    Delivery::SceneScript { last }
                        if !self
                            .scene_views
                            .get(&record.update.entity)
                            .is_some_and(|stream| stream.ready_to_send(last)) =>
                    {
                        return None;
                    }
                    Delivery::BotDestroy => {
                        if self.bots.get(&record.update.entity) == Some(&BotLifetime::RetiringEnemy)
                            && !self.identity_ready(record.update.entity)
                        {
                            return None;
                        }
                        destroyed.insert(record.update.entity);
                    }
                    Delivery::RemotePlayerDestroy(id) => {
                        if self.remote_players.get(&id.entity)
                            == Some(&(id, BotLifetime::RetiringEnemy))
                            && !self.identity_ready(id.entity)
                        {
                            return None;
                        }
                        destroyed.insert(id.entity);
                    }
                    Delivery::BotCreate { after: Some(old) }
                        if self.bots.contains_key(&old) && !destroyed.contains(&old) =>
                    {
                        return None;
                    }
                    Delivery::Health { .. } if !self.identity_ready(record.update.entity) => {
                        return None;
                    }
                    Delivery::CreateBody(id) if Some(id) == body => created = true,
                    Delivery::Possession(_) if !created => return None,
                    Delivery::Possession(_) => bound = true,
                    Delivery::DestroyBody(_) | Delivery::Script { close: true, .. } if !bound => {
                        return None;
                    }
                    Delivery::BodyScript {
                        id, last, delta, ..
                    } if (delta && !bound)
                        || !self.body_script.as_ref().is_some_and(|script| {
                            script.id == id && script.stream.ready_to_send(last)
                        }) =>
                    {
                        return None;
                    }
                    Delivery::ProjectileDestroy { .. }
                        if self
                            .biotic_views
                            .values()
                            .any(|view| view.parent == record.update.entity) =>
                    {
                        return None;
                    }
                    Delivery::ProjectileConfirm { id, .. }
                        if self.bound != Some(id) || !self.created.contains(&id.entity) =>
                    {
                        // Even a fallback confirmation needs an acknowledged owner lease.
                        return None;
                    }
                    _ => {}
                }
                Some(Ready { index, record })
            })
            .collect()
    }

    /// Retain blocked records and remove only the exact records selected by the
    /// successful send, even when eligibility skipped an earlier queue entry.
    pub(crate) fn sent(&mut self, indexes: &[usize]) {
        if indexes.is_empty() {
            return;
        }
        for &index in indexes {
            let record = &self.queued[index];
            if let Delivery::SceneScript { last } = record.delivery {
                if let Some(stream) = self.scene_views.get_mut(&record.update.entity) {
                    stream.sent(last);
                }
            }
        }
        if let Some(script) = &mut self.body_script {
            for &index in indexes {
                match self.queued[index].delivery {
                    Delivery::BodyScript { id, last, .. } if id == script.id => script.sent(last),
                    _ => {}
                }
            }
        }
        let mut selected = indexes.iter().copied().peekable();
        let mut index = 0;
        self.queued.retain(|_| {
            let keep = selected.next_if_eq(&index).is_none();
            index += 1;
            keep
        });
    }

    pub(crate) fn sent_indices(ready: &[Ready<'_>], sent: &[RecordSource]) -> Vec<usize> {
        sent.iter()
            .filter_map(|source| match *source {
                RecordSource::Queued(index) => Some(ready[index].index),
                RecordSource::Movement(_) => None,
            })
            .collect()
    }

    pub(crate) fn send_frame<E>(
        &mut self,
        tick: u32,
        mut movements: Vec<EntityUpdate>,
        reliable_only: bool,
        simulation: Option<&mut RoomSimulation>,
        egress: &mut GameEgress<Arc<Record>>,
        send: impl FnOnce(Vec<u8>) -> Result<u32, E>,
    ) -> Result<SentFrame, SendError<E>> {
        // Numbered notices can still be retransmitted after an entity starts
        // retiring. Keep its destroy behind the acknowledged reliable prefix.
        let pending = egress.oldest_pending_reliable();
        self.entity_notice_ordinals
            .retain(|_, ordinal| u64::from(*ordinal) >= pending);
        if !reliable_only {
            if let Some(simulation) = simulation {
                self.prepare_body_updates(simulation);
            }
        }
        let script = self.body_script.as_ref();
        let script_history = script.and_then(|script| {
            script
                .corrections
                .as_ref()
                .map(|history| (script.id, history))
        });
        let detached = script_history.is_none();
        let owner_history = script_history.or_else(|| {
            let (id, history) = self.detached_motion.as_ref()?;
            (self.body.is_some_and(|body| body.id == *id)).then_some((*id, history))
        });
        let history = owner_history.map(|(_, history)| history);
        let correction = if reliable_only {
            None
        } else {
            owner_history.and_then(|(id, history)| {
                history.candidate(tick, self.created.contains(&id.entity))
            })
        };
        let owner_index = correction.map(|correction| {
            let index = movements.len();
            movements.push(correction.update());
            index
        });
        let mut queued = self.ready();
        let is_bot_chunk = |item: &Ready<'_>| {
            let record = item.record;
            matches!(record.delivery, Delivery::SceneScript { .. })
                && record.update.chunk.is_some()
                && self.bots.contains_key(&record.update.entity)
        };
        // A mixed packet can be acknowledged even when the receiver drops
        // the NPC chunk. Give each class a send opportunity under sustained
        // traffic and let the ordinary stream handle acknowledgements.
        let contention =
            !reliable_only && egress.has_queued_reliable() && queued.iter().any(is_bot_chunk);
        let bot_turn = contention && self.bot_chunk_waited;
        if contention {
            queued.retain(|item| is_bot_chunk(item) == bot_turn);
        }
        let newest = self.command_ack(tick);
        let sent = egress.send_frame_with_messages(
            FrameRecords {
                tick,
                queued: &queued,
                movements: &movements,
                owner_correction: owner_index.is_some(),
                reliable_only,
            },
            !bot_turn,
            |records| {
                let selected = owner_index
                    .filter(|&index| {
                        records
                            .iter()
                            .any(|record| std::ptr::eq(*record, &movements[index]))
                    })
                    .and(correction.as_ref());
                history.map_or(newest, |history| {
                    history.command_ack(tick, Some(newest), selected)
                })
            },
            |source| match source {
                RecordSource::Queued(index) => queued[index].tracked(),
                RecordSource::Movement(_) => unreachable!("movement records are not retried"),
            },
            send,
        )?;
        let indexes = Self::sent_indices(&queued, &sent.records);
        self.sent(&indexes);
        if !reliable_only {
            self.bot_chunk_waited = contention && !bot_turn;
        }
        let selected = owner_index
            .zip(correction)
            .filter(|(index, _)| sent.records.contains(&RecordSource::Movement(*index)));
        if let Some((_, correction)) = selected {
            if detached {
                if let Some((_, history)) = &mut self.detached_motion {
                    if history.sent(&correction) && history.impulse_correction().is_none() {
                        self.detached_motion = None;
                    }
                }
            } else if let Some(script) = &mut self.body_script {
                script.correction_sent(&correction);
            }
        }
        Ok(sent)
    }

    fn prepare_body_updates(&mut self, simulation: &mut RoomSimulation) {
        // A split input gap can update physics history immediately. The owner
        // VM and health publish only after the queued command completes.
        if matches!(
            self.input.work.front(),
            Some(InputWork::Command {
                pending: Some(_),
                ..
            })
        ) {
            return;
        }
        let Some(id) = self.body_script.as_ref().map(|script| script.id) else {
            return;
        };
        let created = self.created.contains(&id.entity);
        let (health, next) = {
            let Some(runtime) = simulation
                .player_body_mut(id)
                .and_then(|state| state.runtime.as_deref_mut())
            else {
                self.cancel_body_script(Some(simulation));
                return;
            };
            let health = runtime.take_health_update();
            let next = self
                .body_script
                .as_mut()
                .expect("checked body script")
                .next_record(runtime, created);
            (health, next)
        };
        self.publish_owner_health(simulation, id, health);
        let (record, replaced) = match next {
            Ok(Some(record)) => record,
            Ok(None) => return,
            Err(error) => {
                eprintln!("body {:08X} owner statescript stopped: {error}", id.entity);
                #[cfg(test)]
                {
                    self.owner_script_fault =
                        self.body_script.as_ref().map(|script| OwnerScriptFault {
                            id,
                            frame: script.frame,
                            step: None,
                            error,
                        });
                }
                self.cancel_body_script(Some(simulation));
                return;
            }
        };
        if let Some(replaced) = replaced {
            self.queued.retain(|queued| {
                !matches!(queued.delivery, Delivery::BodyScript { id: queued_id, last, .. }
                    if queued_id == id && last == replaced)
            });
        }
        self.push(record);
    }

    fn publish_owner_health(
        &mut self,
        simulation: &mut RoomSimulation,
        id: BodyId,
        health: Option<WorldComponent>,
    ) {
        if let Some(health) = health {
            let update = EntityUpdate {
                entity: id.entity,
                operation: Some(Operation::Update(vec![health])),
                ..EntityUpdate::default()
            };
            simulation.record_observer_player_health(self.owner, update.clone());
            self.queue_health(update);
        }
    }

    pub(super) fn cancel_body_script(&mut self, simulation: Option<&mut RoomSimulation>) {
        if let Some(mut script) = self.body_script.take() {
            if self.body.is_some_and(|body| body.id == script.id) {
                self.detached_motion = script
                    .corrections
                    .take()
                    .filter(|history| history.impulse_correction().is_some())
                    .map(|history| (script.id, history));
            }
            self.retire_biotic_views(|_| false);
            script.stream.retire();
            if let Some(sim) = simulation {
                sim.clear_player_runtime(script.id);
                let health = sim
                    .player_body_mut(script.id)
                    .and_then(PlayerBodyState::take_health_update);
                self.publish_owner_health(sim, script.id, health);
            }
            if let Some(projectiles) = &mut script.projectiles {
                self.queue_projectiles(script.id, projectiles.retire())
                    .expect("projectile retirement has no release payload");
            }
            self.remove_stale();
        }
    }

    pub(crate) fn player_script_halted(
        &mut self,
        owner: u32,
        id: BodyId,
        health: Option<WorldComponent>,
    ) {
        if owner == self.owner {
            if self.active_body_id() != Some(id) {
                return;
            }
            if self
                .body_script
                .as_ref()
                .is_some_and(|script| script.id == id)
            {
                // The room already transferred the runtime and its health.
                self.cancel_body_script(None);
            }
        } else {
            if self.remote_players.get(&id.entity) != Some(&(id, BotLifetime::Live)) {
                return;
            }
            self.scene_views.remove(&id.entity);
            self.remove_stale();
        }
        if let Some(health) = health {
            self.queue_health(EntityUpdate {
                entity: id.entity,
                operation: Some(Operation::Update(vec![health])),
                ..EntityUpdate::default()
            });
        }
    }

    pub(crate) fn start(
        &mut self,
        scene: &PreparedScene,
        viewer: &PlayerAppearance<'_>,
        tick: u32,
    ) -> io::Result<()> {
        let records = scene.initial_world(viewer)?;
        self.gate = scene
            .simulation()
            .junkenstein_gate()
            .map(|gate| gate.entity);
        self.bots.extend(
            scene
                .bots()
                .map_err(io::Error::other)?
                .map(|bot| (bot.entity, BotLifetime::Live)),
        );
        for update in records {
            if self.bots.contains_key(&update.entity) {
                self.push(Record {
                    update,
                    delivery: Delivery::BotCreate { after: None },
                });
            } else {
                self.push(Record::baseline(update));
            }
        }
        for (id, update) in scene.simulation().observer_player_creates(self.owner) {
            self.remote_players
                .insert(id.entity, (id, BotLifetime::Live));
            self.push(Record {
                update,
                delivery: Delivery::RemotePlayerCreate(id),
            });
        }
        self.started = Some(tick);
        Ok(())
    }

    pub(crate) fn present(&mut self, scene: &PreparedScene, tick: u32) -> io::Result<()> {
        if self.controller.latest_data() == 0
            && self.created.contains(&self.owner)
            && (!scene.map().is_junkenstein() || self.mode_ready())
        {
            self.queue_controller(scene, tick, true, None)?;
        }
        if self.mode.latest_data() == 0 && self.created.contains(&GAME_MODE_ENTITY) {
            let chunk = self.mode.full(scene.mode_ui()).map_err(invalid)?;
            self.push(Record::script(chunk, false));
        }
        self.presentation_due = self.controller.latest_data() == 0 || self.mode.latest_data() == 0;
        Ok(())
    }

    pub(crate) fn present_scene_scripts(&mut self, scene: &PreparedScene) -> io::Result<()> {
        if let Some(projection) = scene.simulation().gate_projection() {
            self.present_scene_script(projection)?;
        }
        for projection in scene.enemy_projections().map_err(io::Error::other)? {
            self.present_scene_script(projection)?;
        }
        for projection in scene.simulation().player_projections(self.owner) {
            self.present_scene_script(projection)?;
        }
        Ok(())
    }

    pub(super) fn present_scene_script(
        &mut self,
        projection: crate::script_snapshot_wire::SceneProjection<'_>,
    ) -> io::Result<()> {
        let entity = projection.snapshot.entity;
        if self.gate != Some(entity)
            && self.bots.get(&entity) != Some(&BotLifetime::Live)
            && !self
                .remote_players
                .get(&entity)
                .is_some_and(|&(_, life)| life == BotLifetime::Live)
        {
            return Ok(());
        }
        let stream = self
            .scene_views
            .entry(entity)
            .or_insert_with(|| BodyStream::new(entity));
        if self.created.contains(&entity) {
            stream.create_arrived();
        }
        if stream.pending().next().is_some() {
            return Ok(());
        }
        let encoder = projection.encoder;
        let chunk = if let Some((_, base)) = stream.base() {
            let snapshot = encoder
                .project_next(projection.snapshot, base)
                .map_err(invalid)?;
            encoder
                .queue_delta(stream, &snapshot, DeltaKind::Observer { resend: true }, 16)
                .map_err(invalid)?
        } else {
            let snapshot = encoder
                .project(projection.snapshot, View::Observer)
                .map_err(invalid)?;
            Some(encoder.queue_full(stream, &snapshot).map_err(invalid)?)
        };
        if let Some(chunk) = chunk {
            ensure_script_chunk_fits(
                &chunk.update,
                "scene script chunk exceeds the datagram budget",
            )?;
            self.push(Record {
                update: chunk.update,
                delivery: Delivery::SceneScript { last: chunk.last },
            });
        }
        Ok(())
    }

    pub(crate) fn queue_encounter_presentation(&mut self, scene: &PreparedScene) -> io::Result<()> {
        if !self.mode_ready() || self.mode.data_in_flight() {
            return Ok(());
        }
        let sim = scene.simulation();
        if !self.encounter_post_match_queued {
            if let Some(payload) = sim.encounter_post_match_wire()? {
                let chunk = self.mode.reliable_delta(&payload).map_err(invalid)?;
                self.push(Record::script(chunk, false));
                self.encounter_post_match_queued = true;
                return Ok(());
            }
        }
        if !self.encounter_result_queued {
            if let Some(payload) = sim.encounter_result_wire()? {
                let chunk = self.mode.reliable_delta(&payload).map_err(invalid)?;
                self.push(Record::script(chunk, false));
                self.encounter_result_queued = true;
                return Ok(());
            }
        }
        if !self.encounter_outcome_queued {
            if let Some(stamp) = self.mode.latest_data().checked_add(1) {
                if let Some(payload) = sim.encounter_outcome_wire(stamp)? {
                    let mut chunk = self.mode.reliable_delta(&payload).map_err(invalid)?;
                    if let Some(result) = sim.encounter_match_result() {
                        chunk.update.operation = Some(Operation::Update(vec![result]));
                    }
                    self.push(Record::script(chunk, false));
                    self.encounter_outcome_queued = true;
                    return Ok(());
                }
            }
        }
        if self.encounter_post_match_queued {
            if let Some((snapshot, payload)) =
                sim.encounter_screen_delta(self.encounter_screens.as_ref())?
            {
                let chunk = self.mode.reliable_delta(&payload).map_err(invalid)?;
                self.push(Record::script(chunk, false));
                self.encounter_screens = Some(snapshot);
                return Ok(());
            }
        }
        if self.encounter_result_queued || self.encounter_outcome_queued {
            return Ok(());
        }
        if self.encounter_notifications.is_some() {
            if let Some((raw, activation, payload)) =
                sim.encounter_phase_wire(&self.encounter_phases)?
            {
                let chunk = self.mode.reliable_delta(&payload).map_err(invalid)?;
                self.push(Record::script(chunk, false));
                self.encounter_phases.insert(raw, activation);
                return Ok(());
            }
        }
        let countdown = sim
            .encounter_countdown()
            .filter(|value| self.countdown.as_ref() != Some(value));
        // Both projections can change every tick because they read the clock.
        // Alternate when both are pending so either leaves room for the other.
        if !self.encounter_post_match_queued
            && (self.encounter_notification_turn || countdown.is_none())
        {
            if let Some((snapshot, payload)) =
                sim.encounter_notification_delta(self.encounter_notifications.as_ref())?
            {
                let chunk = self.mode.reliable_delta(&payload).map_err(invalid)?;
                self.push(Record::script(chunk, false));
                self.encounter_notifications = Some(snapshot);
                self.encounter_notification_turn = false;
                return Ok(());
            }
        }
        if let Some(countdown) = countdown {
            if let Some(payload) = sim.encounter_countdown_wire(&countdown)? {
                let chunk = self.mode.reliable_delta(&payload).map_err(invalid)?;
                self.push(Record::script(chunk, false));
                self.countdown = Some(countdown);
                self.encounter_notification_turn = true;
            }
        }
        Ok(())
    }

    pub(crate) fn encounter_screens_delivered(&self) -> bool {
        // After cleanup the manager is the only instance in this projection.
        self.encounter_screens
            .as_ref()
            .is_some_and(|screens| screens.instance_count() == 1)
            && !self.mode.data_in_flight()
    }

    fn can_handle_game_message(&self) -> bool {
        self.started.is_some()
            && self.controller.latest_data() != 0
            && self.created.contains(&self.owner)
    }

    fn game_message(
        &mut self,
        scene: &mut PreparedScene,
        viewer: &PlayerAppearance<'_>,
        tick: u32,
        now: Duration,
        quanta: u32,
        message: &Message,
    ) -> io::Result<()> {
        if !self.can_handle_game_message() {
            return Ok(());
        }
        let Some(message) = ClientGameMessage::read(message) else {
            return Ok(());
        };
        if message.entity != self.owner {
            return Ok(());
        }
        enum HeroSelectAction {
            Pick(Hero),
            OpenOrClose(Option<bool>),
        }
        let action = match message.hero_select(scene.heroes()) {
            Some(HeroSelectRequest::Pick(Some(hero))) => Some(HeroSelectAction::Pick(hero.clone())),
            Some(HeroSelectRequest::OpenOrClose(wanted)) => {
                Some(HeroSelectAction::OpenOrClose(wanted))
            }
            _ => None,
        };
        match action {
            Some(HeroSelectAction::Pick(hero)) => {
                if viewer
                    .card
                    .role
                    .is_some_and(|role| role != 0 && hero.role_number() != u8::try_from(role).ok())
                {
                    return Ok(());
                }
                let skin = viewer
                    .card
                    .skins
                    .get(&hero.guid)
                    .copied()
                    .unwrap_or_default();
                if self
                    .body
                    .is_some_and(|body| (body.hero, body.skin) == (hero.guid, skin))
                {
                    self.queue_controller(scene, tick, false, Some(hero.guid))?;
                } else if let Some(entity) = self.next_body(now) {
                    self.switch(scene, viewer, tick, quanta, entity, (&hero, skin))?;
                }
            }
            Some(HeroSelectAction::OpenOrClose(wanted)) => {
                let open = self.body.is_none() || wanted.unwrap_or(!self.select_open);
                self.queue_controller(scene, tick, open, self.body.map(|body| body.hero))?;
            }
            None => {}
        }
        Ok(())
    }

    pub(crate) fn command_ack(&self, tick: u32) -> u32 {
        self.input.processed.unwrap_or_else(|| {
            self.input
                .first_admitted
                .map_or(tick, |frame| tick.min(frame.saturating_sub(1)))
        })
    }

    pub(crate) fn queue_game_message(&mut self, tick: u32, received: Duration, message: Message) {
        if self.can_handle_game_message() {
            self.input.push(InputWork::Message {
                message,
                tick,
                received,
            });
        }
    }

    pub(crate) fn queue_skin(&mut self, hero: u64, tick: u32, received: Duration) {
        if self.can_handle_game_message() {
            self.input.push(InputWork::Skin {
                hero,
                tick,
                received,
            });
        }
    }

    fn apply_skin(
        &mut self,
        scene: &mut PreparedScene,
        viewer: &PlayerAppearance<'_>,
        hero: u64,
        tick: u32,
        now: Duration,
        quanta: u32,
    ) -> io::Result<()> {
        let Some(body) = self.body.filter(|body| body.hero == hero) else {
            return Ok(());
        };
        let Some(&skin) = viewer
            .card
            .skins
            .get(&hero)
            .filter(|&&skin| skin != body.skin)
        else {
            return Ok(());
        };
        if let Some(entity) = self.next_body(now) {
            let hero =
                scene.heroes().get(hero).cloned().ok_or_else(|| {
                    invalid("active player hero is absent from the scene catalog")
                })?;
            self.switch(scene, viewer, tick, quanta, entity, (&hero, skin))?;
        }
        Ok(())
    }

    pub(crate) fn queue_commands(&mut self, tick: u32, commands: &[Command]) {
        if self.started.is_some() {
            self.input
                .commands(tick, commands, self.body_script.as_ref().map(|s| s.frame));
        }
    }

    pub(crate) fn has_input_work(&self) -> bool {
        !self.input.work.is_empty()
    }

    pub(crate) fn combat_body_active(&self) -> bool {
        self.body_script.is_some()
    }

    pub(crate) fn take_hitscan(&self, simulation: &mut RoomSimulation) -> Vec<PosedHitscan> {
        let Some(id) = self.active_body_id() else {
            return Vec::new();
        };
        simulation.take_player_hitscan(id)
    }

    pub(crate) fn advance_fields(
        &mut self,
        simulation: &mut RoomSimulation,
        tick: u32,
    ) -> io::Result<Option<(BodyId, Vec<HealingPulse>)>> {
        let Some(id) = self
            .body_script
            .as_ref()
            .filter(|script| script.projectiles.is_some())
            .map(|script| script.id)
        else {
            return Ok(None);
        };
        let Some((pulses, expired_tokens)) = simulation.advance_player_fields(id, tick) else {
            return Ok(None);
        };
        let Some(projectiles) = self
            .body_script
            .as_mut()
            .and_then(|script| script.projectiles.as_mut())
        else {
            return Ok(None);
        };
        let mut deliveries = Vec::new();
        for token in expired_tokens {
            projectiles.cancel(token, &mut deliveries);
        }
        self.queue_projectiles(id, deliveries)?;
        Ok((!pulses.is_empty()).then_some((id, pulses)))
    }

    fn retire_biotic_views(&mut self, keep: impl Fn(&BioticView) -> bool) {
        for (&entity, view) in &mut self.biotic_views {
            if !view.retiring && !keep(view) {
                view.retiring = true;
                self.queued.push(Arc::new(Record {
                    update: EntityUpdate {
                        entity,
                        operation: Some(Operation::Destroy),
                        ..EntityUpdate::default()
                    },
                    delivery: Delivery::BioticDestroy,
                }));
            }
        }
    }

    pub(crate) fn advance_biotic_views(
        &mut self,
        simulation: &RoomSimulation,
        tick: u32,
    ) -> io::Result<()> {
        let Some((body, script)) = self.body.zip(self.body_script.as_ref()) else {
            return Ok(());
        };
        let Some(projectiles) = &script.projectiles else {
            return Ok(());
        };
        let Some(state) = simulation.player_body(script.id) else {
            return Ok(());
        };
        if state.fields.is_empty() && self.biotic_views.is_empty() {
            return Ok(());
        }
        let mut active = Vec::new();
        for (token, field) in &state.fields {
            if let Some(faded) = field.presentation(tick) {
                active.extend(
                    projectiles
                        .confirmed_entities(*token)
                        .map(|parent| (*token, parent, faded)),
                );
            }
        }
        // Retire the attached child before allowing its predicted parent to be destroyed.
        self.retire_biotic_views(|view| {
            active
                .iter()
                .any(|&(token, parent, _)| token == view.token && parent == view.parent)
        });
        for (token, parent, faded) in active {
            let existing = self
                .biotic_views
                .iter_mut()
                .find(|(_, view)| view.token == token && view.parent == parent);
            if let Some((_, view)) = existing {
                if view.created && !view.retiring && view.faded != Some(faded) {
                    let Some(runtime) = &state.runtime else {
                        return Ok(());
                    };
                    let payload = runtime
                        .encoder()
                        .biotic_field_frame(faded)
                        .map_err(invalid)?;
                    let chunk = view.stream.full(&payload).map_err(invalid)?;
                    view.faded = Some(faded);
                    self.push(Record {
                        update: chunk.update,
                        delivery: Delivery::BioticFrame { last: chunk.last },
                    });
                }
                continue;
            }
            if self.next_biotic_view == END_BIOTIC_VIEW {
                continue;
            }
            let entity = self.next_biotic_view;
            self.next_biotic_view += 1;
            let mut create = Create::new(BIOTIC_FIELD_ENTITY, Vec::new());
            create.skin.guid = body.skin.guid;
            create.movement_parent = Some(parent);
            create.movement = Some(Movement {
                flags: KEEP_MOVEMENT_ROTATION,
                ..Movement::default()
            });
            self.biotic_views.insert(
                entity,
                BioticView {
                    token,
                    parent,
                    created: false,
                    faded: None,
                    retiring: false,
                    stream: Stream::new(entity),
                },
            );
            self.push(Record {
                update: EntityUpdate {
                    entity,
                    operation: Some(Operation::Create(Box::new(create))),
                    ..EntityUpdate::default()
                },
                delivery: Delivery::BioticCreate,
            });
        }
        self.remove_stale();
        Ok(())
    }

    pub(crate) fn take_projectile_launches(
        &self,
        simulation: &mut RoomSimulation,
    ) -> Option<(BodyId, Vec<AdmittedFlight>)> {
        let script = self.body_script.as_ref()?;
        script.projectiles.as_ref()?;
        let launches = simulation.take_player_projectile_launches(script.id)?;
        Some((script.id, launches))
    }

    pub(crate) fn finish_projectiles(
        &mut self,
        id: BodyId,
        tick: u32,
        outcomes: Vec<FlightOutcome>,
    ) -> io::Result<()> {
        let Some(projectiles) = self
            .body_script
            .as_mut()
            .filter(|script| script.id == id)
            .and_then(|script| script.projectiles.as_mut())
        else {
            return Ok(());
        };
        let mut deliveries = Vec::new();
        for outcome in outcomes {
            match outcome {
                FlightOutcome::Ended { token, travelled } => {
                    let travelled = (travelled / TICK_PERIOD.as_secs_f64()).ceil() as u32;
                    projectiles.flight_ended(token, travelled, tick);
                }
                FlightOutcome::Failed(token) => projectiles.cancel(token, &mut deliveries),
            }
        }
        deliveries.extend(projectiles.advance(tick));
        self.queue_projectiles(id, deliveries)
    }

    pub(super) fn queue_projectiles(
        &mut self,
        id: BodyId,
        deliveries: Vec<ProjectileDelivery>,
    ) -> io::Result<()> {
        if deliveries.is_empty() {
            return Ok(());
        }
        for delivery in deliveries {
            let update = delivery.update().map_err(invalid)?;
            let delivery = match delivery {
                ProjectileDelivery::Confirm { token, .. } => {
                    self.projectile_destroys.remove(&update.entity);
                    Delivery::ProjectileConfirm { id, token }
                }
                ProjectileDelivery::Destroy { token, .. } => {
                    self.projectile_destroys.insert(update.entity, (id, token));
                    Delivery::ProjectileDestroy { id, token }
                }
            };
            self.push(Record { update, delivery });
        }
        self.remove_stale();
        Ok(())
    }

    /// One unit per physics pose, unposed VM frame, pre-body command, or queued game message.
    /// A fresh frame is committed only after its final pose has completed.
    pub(crate) fn advance_input(
        &mut self,
        scene: &mut PreparedScene,
        viewer: &PlayerAppearance<'_>,
        quanta: u32,
        budget: usize,
    ) -> io::Result<(usize, bool)> {
        let mut remaining = budget;
        while remaining != 0 {
            let Some(work) = self.input.work.pop_front() else {
                break;
            };
            // Let the room settle these shots before a selection despawns
            // the body that owns them, even if its script has stopped.
            if matches!(work, InputWork::Message { .. } | InputWork::Skin { .. })
                && self
                    .active_body_id()
                    .and_then(|id| scene.simulation().player_body(id))
                    .is_some_and(|body| !body.hitscan.is_empty())
            {
                self.input.work.push_front(work);
                break;
            }
            match work {
                InputWork::Message {
                    message,
                    tick,
                    received,
                } => {
                    self.game_message(scene, viewer, tick, received, quanta, &message)?;
                    remaining -= 1;
                }
                InputWork::Skin {
                    hero,
                    tick,
                    received,
                } => {
                    self.apply_skin(scene, viewer, hero, tick, received, quanta)?;
                    remaining -= 1;
                }
                InputWork::Command {
                    command,
                    mut pending,
                } => {
                    if let Some(id) = self.body_script.as_ref().map(|script| script.id) {
                        if scene.simulation().player_has_deadeye(id) {
                            let deadeye =
                                scene.deadeye_scene(viewer.team).map_err(io::Error::other)?;
                            scene
                                .simulation_mut()
                                .set_player_deadeye_targets(id, deadeye);
                        }
                    }
                    let (used, complete) =
                        self.advance_command(scene, command, &mut pending, quanta, remaining)?;
                    remaining -= used;
                    if !complete {
                        self.input
                            .work
                            .push_front(InputWork::Command { command, pending });
                        break;
                    }
                    self.input.processed = Some(command.frame);
                }
                InputWork::Leave => return Ok((budget - remaining + 1, true)),
            }
        }
        Ok((budget - remaining, false))
    }

    fn advance_command(
        &mut self,
        scene: &mut PreparedScene,
        command: Command,
        pending: &mut Option<PendingCommand>,
        frame_us: u32,
        budget: usize,
    ) -> io::Result<(usize, bool)> {
        let Some(id) = self.body.map(|body| body.id) else {
            return Ok((1, true));
        };
        let (simulation, rays) = scene.simulation_and_rays();
        let motion_frame = simulation.movement_contact_frame();
        let Some(state) = simulation.player_body_mut(id) else {
            return Ok((1, true));
        };
        let pending = pending.get_or_insert_with(|| {
            PendingCommand::new(
                state.mover.pending_poses(command),
                self.body_script.as_ref(),
            )
        });
        let mut used = 0;
        let mut fault = None;
        let team = state.team;
        let previously_processed = self.input.processed;
        let script = &mut self.body_script;
        let mut detached_motion = self
            .detached_motion
            .as_mut()
            .filter(|(owner, _)| *owner == id)
            .map(|(_, history)| history);
        let PlayerBodyState {
            mover,
            runtime,
            frame: body_frame,
            fields,
            launches,
            hitscan,
            ..
        } = state;
        let mut buffers = StepBuffers {
            frame: body_frame,
            fields,
            launches,
            hitscan,
        };
        if let Some((script, runtime)) = script.as_mut().zip(runtime.as_deref_mut()) {
            for (frame, step) in pending.gap.by_ref().take(budget) {
                used += 1;
                let neutral = Command {
                    frame,
                    ..Command::default()
                };
                if let Err(error) = script.advance_step(runtime, mover, &mut buffers, neutral, step)
                {
                    mover.track_modifiers(frame, &BTreeMap::new());
                    mover.clear_pending_script_motion();
                    pending.gap = ScriptGap::default();
                    fault = Some((frame, step, error));
                    break;
                }
            }
        }
        if fault.is_none() && used < budget && pending.poses.len() != 0 {
            if let Some(runtime) = runtime.as_deref_mut() {
                stage_scripted_motion(runtime, mover);
            }
        }
        let stepped = mover.step_pending(
            rays,
            &mut pending.poses,
            f64::from(frame_us) / 1_000_000.0,
            budget - used,
            || {
                motion_frame
                    .as_ref()
                    .map_or_else(Vec::new, |frame| frame.blockers(id.entity, team))
            },
            |mover, command| {
                if let Some(history) = detached_motion.as_deref_mut() {
                    history.record(mover.pose());
                }
                if let Some(script) = script.as_mut() {
                    // The pose already moved even if the state's next update fails.
                    script.record_scripted_movement(
                        mover,
                        command.frame,
                        frame_us,
                        previously_processed,
                    );
                    if fault.is_some() {
                        if let Some(history) = script.scripted_motion_history() {
                            history.record(mover.pose());
                        }
                    }
                }
                if let Some((script, runtime)) = script
                    .as_mut()
                    .zip(runtime.as_deref_mut())
                    .filter(|_| fault.is_none())
                {
                    if let Err(error) =
                        script.advance_step(runtime, mover, &mut buffers, command, BodyStep::Posed)
                    {
                        mover.track_modifiers(command.frame, &BTreeMap::new());
                        mover.clear_pending_script_motion();
                        fault = Some((command.frame, BodyStep::Posed, error));
                    }
                }
            },
        );
        let complete = pending.poses.len() == 0;
        if let Some((frame, step, error)) = fault {
            eprintln!("body owner statescript stopped at frame {frame} during {step:?}: {error}");
            #[cfg(test)]
            {
                self.owner_script_fault = Some(OwnerScriptFault {
                    id,
                    frame,
                    step: Some(step),
                    error,
                });
            }
            self.cancel_body_script(Some(simulation));
        }
        used += stepped.map_err(invalid)?;
        Ok((used, complete))
    }

    pub(super) fn next_body(&self, now: Duration) -> Option<u32> {
        let suffix = self.body.map_or(0, |body| body.id.entity & 0xff);
        (0..255)
            .map(|offset| self.owner | ((suffix + offset) % 255 + 1))
            .find(|&entity| {
                self.bodies.get(&entity).is_none_or(
                    |(_, lifetime)| matches!(lifetime, Lifetime::ReusableAt(at) if now >= *at),
                )
            })
    }

    pub(super) fn switch(
        &mut self,
        scene: &mut PreparedScene,
        viewer: &PlayerAppearance<'_>,
        tick: u32,
        quanta: u32,
        entity: u32,
        selection: (&Hero, SkinTheme),
    ) -> io::Result<()> {
        let (hero, skin) = selection;
        let mover = scene.player_spawn_mover(hero.body, viewer.team)?;
        let movement = mover.pose();
        let pose = owner_pose(movement);
        let generation = scene
            .simulation()
            .next_player_generation(self.owner, self.generation)?;
        let id = BodyId { entity, generation };
        let body = Body {
            id,
            hero: hero.guid,
            skin,
        };
        let body_filter = team_bit(viewer.team)?;
        let start_frame = self.input.processed.unwrap_or(tick);
        let slot = u8::try_from(viewer.slot).map_err(invalid)?;
        let candidate = scene
            .owner_body_runtime(
                hero,
                OwnerBodySpawn {
                    slot,
                    controller: self.owner,
                    entity,
                    body_filter,
                    frame: start_frame,
                    pose: movement,
                    eye_position: crate::game_combat::script_eye_position(hero.guid, &mover),
                    skin: skin.guid,
                    quanta,
                },
            )
            .and_then(|runtime| {
                let script = runtime
                    .as_deref()
                    .map(|runtime| {
                        BodyScript::new(body, start_frame, self.input.processed, runtime)
                    })
                    .transpose()?;
                Ok((runtime, script))
            });
        let (runtime, script) = match candidate {
            Ok(candidate) => candidate,
            Err(error) => {
                eprintln!(
                    "body startup rejected for controller {:08X}: {error}",
                    self.owner
                );
                return Ok(());
            }
        };
        let body_create_input = BodyCreate {
            entity,
            hero,
            team_bit: body_filter,
            position: pose.position,
            yaw: pose.yaw,
            skin,
            vitals: runtime.as_deref().and_then(OwnerBodyRuntime::vitals),
        };
        let create = body_create(
            BodyCreate {
                entity,
                hero,
                team_bit: body_filter,
                position: pose.position,
                yaw: pose.yaw,
                skin,
                vitals: body_create_input.vitals,
            },
            BodyView::Owner {
                pose: Some(pose),
                leases: runtime
                    .as_deref()
                    .and_then(OwnerBodyRuntime::projectile_leases)
                    .map_or(&[], ProjectileLeases::blocks),
            },
            scene.pools(),
        );
        let observer_create = body_create(body_create_input, BodyView::Observer, scene.pools());
        let possession = Possession {
            body: entity,
            hero: hero.guid,
            skin: (skin.guid != 0).then_some(skin.guid),
        };
        let mut controller = self.controller.clone();
        let close = Self::controller_chunk(
            &mut controller,
            scene,
            self.command_ack(tick),
            false,
            Some(hero.guid),
        )?;

        let state = PlayerBodyState::new(
            id,
            self.owner,
            slot,
            viewer.team,
            hero.guid,
            mover,
            runtime,
            start_frame,
            Some(observer_create),
        );
        scene.simulation_mut().spawn_player_body(state)?;
        self.controller = controller;
        self.generation = generation;
        self.bodies.insert(entity, (generation, Lifetime::Live));
        self.created.remove(&entity);
        let old = self.body.replace(body);
        self.cancel_body_script(None);
        self.body_script = script;
        self.bound = None;
        self.select_open = false;
        self.push(Record {
            update: create,
            delivery: Delivery::CreateBody(id),
        });
        self.push(Record {
            update: EntityUpdate {
                entity: self.owner,
                operation: Some(Operation::Update(possession.components().into())),
                ..EntityUpdate::default()
            },
            delivery: Delivery::Possession(id),
        });
        if let Some(old) = old {
            self.retire_body(old.id);
        }
        self.push(close);
        self.remove_stale();
        Ok(())
    }

    pub(super) fn retire_body(&mut self, id: BodyId) {
        if self
            .detached_motion
            .as_ref()
            .is_some_and(|(owner, _)| *owner == id)
        {
            self.detached_motion = None;
        }
        self.created.remove(&id.entity);
        self.health_updates.remove(&id.entity);
        self.bodies
            .insert(id.entity, (id.generation, Lifetime::Retiring));
        self.push(Record {
            update: EntityUpdate {
                entity: id.entity,
                operation: Some(Operation::Destroy),
                ..EntityUpdate::default()
            },
            delivery: Delivery::DestroyBody(id),
        });
    }

    fn controller_chunk(
        controller: &mut Stream,
        scene: &PreparedScene,
        command_frame: u32,
        open: bool,
        hero: Option<u64>,
    ) -> io::Result<Record> {
        let chunk = controller
            .full_owner(command_frame, |cmfd| {
                scene.controller().controller_frame(
                    cmfd,
                    ControllerFrame {
                        select_open: open,
                        skins: open && !scene.map().is_junkenstein(),
                        hide_assemble: !scene.map().is_junkenstein(),
                        hero,
                        ..ControllerFrame::default()
                    },
                )
            })
            .map_err(invalid)?;
        Ok(Record::script(chunk, hero.is_some() && !open))
    }

    fn queue_controller(
        &mut self,
        scene: &PreparedScene,
        tick: u32,
        open: bool,
        hero: Option<u64>,
    ) -> io::Result<()> {
        let command_frame = self.command_ack(tick);
        let record =
            Self::controller_chunk(&mut self.controller, scene, command_frame, open, hero)?;
        self.select_open = open;
        self.push(record);
        self.remove_stale();
        Ok(())
    }

    pub(crate) fn movement(
        &self,
        scene: &PreparedScene,
        tick: u32,
    ) -> Result<Vec<EntityUpdate>, CombatError> {
        if !self
            .started
            .is_some_and(|started| tick.saturating_sub(started) >= BOT_RECORD_DELAY)
        {
            return Ok(Vec::new());
        }
        let mut updates: Vec<_> = scene
            .bots()?
            .filter(|bot| self.created.contains(&bot.entity) && bot.record_due(tick, SEND_EVERY))
            .map(|bot| EntityUpdate {
                entity: bot.entity,
                movement: Some(bot.movement(tick)),
                ..EntityUpdate::default()
            })
            .collect();
        if tick % SEND_EVERY == 0 {
            for (entity, movement) in scene.simulation().observer_player_movements(self.owner) {
                if self.created.contains(&entity) {
                    updates.push(EntityUpdate {
                        entity,
                        movement: Some(movement),
                        ..EntityUpdate::default()
                    });
                }
            }
        }
        Ok(updates)
    }
}

pub(super) fn command_prefix(
    anchor: Option<u32>,
    tick: u32,
    commands: &[Command],
) -> (Vec<Command>, Option<u32>) {
    // Classify the lease boundary independently of the resource window so a
    // fresh unencodable frame still closes its old link.
    let (mut fresh, boundary) = CommandQueue::new().before_limit(commands, MAX_LEASE_FRAME, anchor);
    let limit = tick.saturating_add(MAX_INPUT_LEAD);
    fresh.truncate(fresh.partition_point(|command| command.frame <= limit));
    (fresh, boundary)
}

pub(super) fn ensure_script_chunk_fits(update: &EntityUpdate, error: &str) -> io::Result<()> {
    let packet = frame(u32::MAX, u32::MAX, &[], &[], &[update]).map_err(invalid)?;
    if packet.len() > MAX_PAYLOAD {
        return Err(invalid(error));
    }
    Ok(())
}

pub(super) fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
