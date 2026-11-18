//! Room-local execution and fault types. Unwinding never resumes a damaged room.

use super::*;
use std::panic::{AssertUnwindSafe, catch_unwind};
use thiserror::Error;

pub(super) struct WorldStart {
    pub(super) scene: Box<PreparedScene>,
    pub(super) card: PlayerCard,
}

pub(super) struct RoomWorld {
    pub(super) scene: Box<PreparedScene>,
    pub(super) card: PlayerCard,
    pub(super) patrol: PatrolScript,
    pub(super) patrol_generation: u64,
}

pub(super) struct Player {
    pub(super) info: MatchPlayer,
    pub(super) entity: u32,
    pub(super) gone: bool,
}

impl Player {
    pub(super) fn quanta(&self) -> u32 {
        if self.info.tournament {
            TOURNAMENT_QUANTA
        } else {
            RETAIL_QUANTA
        }
    }
}

pub(super) struct Room {
    pub(super) map: GameMap,
    pub(super) players: Vec<Player>,
    pub(super) members: IndexMap<u32, RoomMember>,
    pub(super) world: Option<RoomWorld>,
    #[cfg(test)]
    pub(super) injection: Option<(RoomStage, InjectedFault)>,
}

pub(super) struct RoomMember {
    pub(super) slot: usize,
    pub(super) peer: Option<Peer>,
}

impl Room {
    pub(super) fn appearance(&self, slot: usize) -> Option<PlayerAppearance<'_>> {
        let player = self.players.get(slot)?;
        Some(PlayerAppearance {
            entity: player.entity,
            slot: i8::try_from(slot).ok()?,
            team: player.info.team,
            card: &self.world.as_ref()?.card,
        })
    }

    pub(super) fn message(&self, slot: usize, initial: bool) -> Result<Message, RoomError> {
        let members: Vec<_> = self
            .players
            .iter()
            .map(|player| InstanceMember {
                entity: player.entity,
                name: &player.info.name,
                team: player.info.team,
                gone: player.gone,
                ranked: player.info.ranked,
            })
            .collect();
        let instance = Instance {
            map: &self.map,
            members: &members,
        };
        let player = self
            .players
            .get(slot)
            .ok_or(RoomError::Invariant("member slot outside roster"))?;
        Ok(if initial {
            instance.load(player.entity, player.quanta())
        } else {
            instance.update(player.entity)
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RoomStage {
    Admission,
    Routing,
    Receive,
    Input,
    Bots,
    Projectiles,
    FinishTick,
    Loading,
    Send,
    Retire,
    Skin,
    #[cfg(any(test, feature = "inspector"))]
    Debug,
    Drain,
}

impl RoomStage {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Admission => "admission",
            Self::Routing => "routing",
            Self::Receive => "receive",
            Self::Input => "input",
            Self::Bots => "bots",
            Self::Projectiles => "projectiles",
            Self::FinishTick => "finish_tick",
            Self::Loading => "loading",
            Self::Send => "send",
            Self::Retire => "retire",
            Self::Skin => "skin",
            #[cfg(any(test, feature = "inspector"))]
            Self::Debug => "debug",
            Self::Drain => "drain",
        }
    }
}

#[derive(Debug, Error)]
pub(super) enum RoomError {
    #[error("room invariant: {0}")]
    Invariant(&'static str),
    #[error("room operation: {0}")]
    Operation(#[from] io::Error),
    #[error("room combat: {0}")]
    Combat(#[from] crate::game_combat::CombatError),
    #[error("room frame: {0}")]
    Frame(#[from] crate::world::WorldError),
    #[error("room message: {0}")]
    Channel(#[from] crate::game_messages::ChannelError),
    #[error("room reliable stream: {0}")]
    Reliable(#[from] crate::game_egress::OrdinalExhausted),
    #[error("room link: {0:?}")]
    Link(crate::game_connection::TransmitError),
    #[error("socket transport: {0}")]
    Transport(#[source] io::Error),
    #[error("room operation panicked")]
    Panicked,
}

#[derive(Debug, Error)]
pub(super) enum WorkerError {
    #[error("game worker routing invariant: {0}")]
    Routing(&'static str),
    #[error("game worker transport: {0}")]
    Transport(#[source] io::Error),
}

impl From<WorkerError> for io::Error {
    fn from(error: WorkerError) -> Self {
        Self::other(error)
    }
}

#[derive(Clone, Debug)]
pub(super) struct FaultRecord {
    pub(super) id: MatchId,
    pub(super) stage: RoomStage,
    pub(super) connection: Option<u32>,
    pub(super) tick: u32,
    pub(super) reason: &'static str,
}

impl RoomError {
    pub(super) fn category(&self) -> &'static str {
        match self {
            Self::Invariant(_) => "invariant",
            Self::Operation(_) => "operation",
            Self::Combat(_) => "combat",
            Self::Frame(_) => "frame",
            Self::Channel(_) => "message",
            Self::Reliable(_) => "reliable",
            Self::Link(_) => "link",
            Self::Transport(_) => "transport",
            Self::Panicked => "panic",
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(super) enum InjectedFault {
    Error,
    Panic,
    BrokenBot,
}

/// The closure may borrow only this room, immutable catalogs and the current
/// connection's Sender. On unwind the caller removes the room and its links;
/// neither its peer nor ECS state is reused. Global routing is outside this
/// boundary. This fallback requires panic=unwind; abort/OOM remain fatal.
pub(super) fn guard_room<T>(
    room: &mut Room,
    stage: RoomStage,
    operation: impl FnOnce(&mut Room) -> Result<T, RoomError>,
) -> Result<T, RoomError> {
    catch_unwind(AssertUnwindSafe(|| {
        #[cfg(test)]
        if room.injection.is_some_and(|(at, _)| at == stage) {
            match room.injection.take().map(|(_, fault)| fault) {
                Some(InjectedFault::Error) => {
                    return Err(RoomError::Invariant("injected room failure"));
                }
                Some(InjectedFault::Panic) => panic!("injected room failure"),
                Some(InjectedFault::BrokenBot) => {
                    let scene = &mut room
                        .world
                        .as_mut()
                        .ok_or(RoomError::Invariant("bot fault requires a prepared room"))?
                        .scene;
                    if !scene.invalidate_bot_component() {
                        return Err(RoomError::Invariant("bot fault requires a bot"));
                    }
                }
                None => (),
            }
        }
        #[cfg(not(test))]
        let _ = stage;
        operation(room)
    }))
    .unwrap_or(Err(RoomError::Panicked))
}

impl Room {
    /// Temporarily detach just this peer to permit simultaneous room simulation
    /// and delivery borrows. Failure leaves it detached, and retires the room.
    pub(super) fn with_peer<T>(
        &mut self,
        connection: u32,
        operation: impl FnOnce(&mut Room, &mut Peer, usize) -> Result<T, RoomError>,
    ) -> Result<T, RoomError> {
        let member = self
            .members
            .get_mut(&connection)
            .ok_or(RoomError::Invariant("routed member is missing"))?;
        let slot = member.slot;
        let mut peer = member
            .peer
            .take()
            .ok_or(RoomError::Invariant("active peer is missing"))?;
        if self.players.get(slot).is_none_or(|player| player.gone) {
            return Err(RoomError::Invariant(
                "active member has no live roster slot",
            ));
        }
        let result = operation(self, &mut peer, slot)?;
        self.members
            .get_mut(&connection)
            .ok_or(RoomError::Invariant("member disappeared during room work"))?
            .peer = Some(peer);
        Ok(result)
    }

    pub(super) fn admit(&mut self, connection: u32) -> Result<(), RoomError> {
        let member = self
            .members
            .get_mut(&connection)
            .ok_or(RoomError::Invariant("admitted connection has no member"))?;
        if member.peer.is_some() {
            return Err(RoomError::Invariant("connection admitted twice"));
        }
        let player = self
            .players
            .get(member.slot)
            .filter(|player| !player.gone)
            .ok_or(RoomError::Invariant(
                "admitted member has no live roster slot",
            ))?;
        member.peer = Some(Peer::new(player.entity));
        Ok(())
    }

    pub(super) fn retire_member(&mut self, connection: u32) -> Result<(), RoomError> {
        if let Some(member) = self.members.shift_remove(&connection) {
            let player = self
                .players
                .get_mut(member.slot)
                .ok_or(RoomError::Invariant("retired member has no roster slot"))?;
            if player.gone {
                return Err(RoomError::Invariant("retired roster slot already gone"));
            }
            player.gone = true;
            if let Some(world) = &mut self.world {
                world.scene.retire_projectiles(connection);
                world
                    .scene
                    .simulation_mut()
                    .retire_player_owner(player.entity);
            }
        }
        Ok(())
    }

    pub(super) fn receive(
        &mut self,
        connection: u32,
        received: &crate::game_connection::Received,
        schemas: &Schemas,
        tick: u32,
        now: Duration,
        draining: bool,
    ) -> Result<(), RoomError> {
        if received.admitted {
            self.admit(connection)?;
        }
        self.with_peer(connection, |room, peer, slot| {
            if let Some((ack, bits)) = received.ack {
                let report = peer.egress.acknowledge(ack, bits);
                if !draining {
                    peer.world.acknowledge(report, now);
                }
            }
            if received.closed {
                return Ok(());
            }
            peer.open |= received.opened;
            if draining {
                return Ok(());
            }
            if received.opened {
                peer.reliable(schemas, room.message(slot, true)?)?;
            }
            if let Some(payload) = &received.payload {
                let Peer {
                    ingress,
                    egress,
                    loading,
                    world,
                    ..
                } = peer;
                let mut response_error = None;
                let commands =
                    ingress.receive(schemas, payload, |message| {
                        match MessageKind::from(message.id) {
                            MessageKind::Ping => {
                                let received = wall_nanoseconds();
                                if let Ok(pong) =
                                    pong(&message.fields, received, wall_nanoseconds())
                                {
                                    let result = encode(schemas, pong.id, &pong.fields)
                                        .map_err(RoomError::from)
                                        .and_then(|bytes| {
                                            egress.queue_reliable(bytes).map_err(RoomError::from)
                                        });
                                    if let Err(error) = result {
                                        response_error.get_or_insert(error);
                                    }
                                }
                            }
                            MessageKind::MapLoaded => {
                                loading.mark_loaded(now);
                            }
                            MessageKind::GameMessage if room.world.is_some() => {
                                world.queue_game_message(tick, now, message);
                            }
                            _ => (),
                        }
                    });
                if let Some(error) = response_error {
                    return Err(error);
                }
                // Invalid external input remains a rejected packet, not a room fault.
                if let (Ok(commands), Some(_)) = (commands, &room.world) {
                    world.queue_commands(tick, &commands);
                }
            }
            Ok(())
        })
    }

    pub(super) fn input(
        &mut self,
        connection: u32,
        tick: u32,
        now: Duration,
        budget: usize,
    ) -> Result<(usize, bool), RoomError> {
        self.with_peer(connection, |room, peer, slot| {
            let (quanta, player_entity, team) = {
                let player = room
                    .players
                    .get(slot)
                    .ok_or(RoomError::Invariant("input without viewer"))?;
                (player.quanta(), player.entity, player.info.team)
            };
            let world = room
                .world
                .as_mut()
                .ok_or(RoomError::Invariant("input without world"))?;
            let viewer = PlayerAppearance {
                entity: player_entity,
                slot: i8::try_from(slot)
                    .map_err(|_| RoomError::Invariant("input without viewer"))?,
                team,
                card: &world.card,
            };
            let context = ShotContext {
                team: viewer.team,
                quanta,
                tick,
                now,
            };
            // Movement/input failure previously closes this member. Keep that
            // scope and report the cause; body-script failures are handled locally.
            let active = world.scene.encounter_active();
            if let Some(id) = peer.world.active_body_id() {
                world
                    .scene
                    .simulation_mut()
                    .set_player_encounter_active(id, active);
            }
            let (used, leave) = match peer.world.advance_input(
                &mut world.scene,
                &viewer,
                context.quanta,
                budget,
            ) {
                Ok(result) => result,
                Err(error) => {
                    eprintln!(
                        "Game input for connection {connection} at frame {tick} stopped: {error}"
                    );
                    (budget, true)
                }
            };
            if used > budget {
                return Err(RoomError::Invariant("input exceeded assigned budget"));
            }
            let shots = peer.world.take_hitscan(world.scene.simulation_mut());
            if (!leave && peer.world.combat_body_active()) || !shots.is_empty() {
                let (bots, queries) = world.scene.bot_simulation();
                bots.activate_combat();
                for shot in shots {
                    if let Some(hit) = bots.hitscan(&queries, shot, context, |entity| {
                        peer.world.identity_ready(entity)
                    })? {
                        bots.deliver_applied_damage(peer.world.owner(), hit.damage.clone());
                        peer.bot_hit(hit);
                    }
                }
            }
            if leave {
                peer.clear_world_in_room(world.scene.simulation_mut());
                world.scene.retire_projectiles(connection);
                peer.loading = MapReadiness::default();
            }
            Ok((used, leave))
        })
    }
}

#[derive(Default)]
pub(super) struct RoomUpdates {
    pub(super) replacements: Vec<BotReplacement>,
    pub(super) created: Vec<EntityUpdate>,
    pub(super) health: Vec<EntityUpdate>,
    pub(super) retired: Vec<u32>,
    pub(super) player_events: Vec<crate::game_combat::PlayerBodyEvent>,
    pub(super) observer_player_health: Vec<(u32, EntityUpdate)>,
}

impl Room {
    pub(super) fn finish_simulation(
        &mut self,
        tick: u32,
        now: Duration,
    ) -> Result<RoomUpdates, RoomError> {
        let Some(world) = &mut self.world else {
            return Ok(RoomUpdates::default());
        };
        let junkenstein = world.scene.map().is_junkenstein();
        let active = world.scene.encounter_active();
        world.scene.simulation_mut().sync_player_encounter(active);

        let (
            replacements,
            health,
            retired,
            messages,
            bot_sightings,
            deaths,
            player_events,
            observer_player_health,
        ) = {
            let (bots, queries) = world.scene.bot_simulation();
            let sightings = bots.take_player_defender_sightings();
            let replacements = bots.finish_tick(&queries, tick, now)?;
            let mut all_sightings = sightings;
            all_sightings.extend(bots.take_defender_sightings());
            (
                replacements,
                bots.take_scripted_health(),
                bots.take_retired_enemies(),
                bots.take_defender_messages(),
                all_sightings,
                bots.take_player_deaths(),
                bots.take_player_events(),
                bots.take_observer_player_health(),
            )
        };
        for death in deaths {
            for member in self.members.values_mut() {
                if let Some(peer) = &mut member.peer {
                    if peer.world.owner() == death.owner {
                        peer.queue_combat_notice(CombatNotice::Death {
                            source: death.source,
                            killed_by: true,
                        });
                    }
                }
            }
        }
        if junkenstein {
            if !messages.is_empty() {
                let deadeye = world.scene.deadeye_scene(0).ok();
                world
                    .scene
                    .simulation_mut()
                    .deliver_player_defender_messages(deadeye.as_ref(), &messages);
            }
            let voice_targets: Vec<u32> = self
                .members
                .values()
                .filter_map(|member| {
                    let player = self.players.get(member.slot)?;
                    if player.gone || player.info.team != 0 {
                        return None;
                    }
                    let peer = member.peer.as_ref()?;
                    world
                        .scene
                        .simulation()
                        .player_body_by_owner(peer.world.owner())
                        .map(|state| state.id.entity)
                })
                .collect();
            for request in world.scene.take_encounter_voice_requests() {
                for &listener in &voice_targets {
                    for member in self.members.values_mut() {
                        if let Some(peer) = &mut member.peer {
                            if peer.world.spawned_body() == Some(listener) {
                                peer.queue_combat_notice(CombatNotice::Voice {
                                    anchor: request.anchor,
                                    speaker: request.speaker,
                                    listener,
                                    stimulus: request.stimulus,
                                    line: request.line,
                                });
                            }
                        }
                    }
                }
            }
            for message in bot_sightings {
                world
                    .scene
                    .simulation_mut()
                    .deliver_player_defender_sighting(message);
            }
        }
        Ok(RoomUpdates {
            replacements,
            created: world.scene.take_enemy_creates(),
            health,
            retired,
            player_events,
            observer_player_health,
        })
    }
}
