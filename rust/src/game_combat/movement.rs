//! A room-tick snapshot for player and NPC capsule movement.

use std::{collections::BTreeMap, sync::Arc};

use crate::{
    contact_cache::CapsulePose, game_bots::Bot, mover::VelocityChange,
    mover_settings::MoverCatalog, static_ray::Vec3,
};

use super::{CombatError, PlayerBodyState, RoomSimulation};

#[cfg(test)]
mod tests;

pub(super) struct MovementContacts {
    free_for_all: bool,
    movers: Arc<MoverCatalog>,
    frame: Option<(u32, Arc<MovementContactFrame>)>,
}

pub(crate) struct MovementContactFrame {
    actors: BTreeMap<u32, (Option<u8>, CapsulePose)>,
}

impl MovementContactFrame {
    /// Wire ordering is stable even when the ECS reuses a body slot.
    pub(crate) fn blockers(&self, source: u32, team: u8) -> Vec<CapsulePose> {
        self.actors
            .iter()
            .filter_map(|(&entity, &(other_team, pose))| {
                (entity != source && other_team != Some(team)).then_some(pose)
            })
            .collect()
    }

    pub(crate) fn stage_bot(&self, bot: &mut Bot, tick: u32) {
        if !bot.dead && !bot.has_movement_blockers(tick) {
            let blockers = if self.actors.contains_key(&bot.entity) {
                self.blockers(bot.entity, bot.spawn.team)
            } else {
                Vec::new()
            };
            bot.set_movement_blockers(tick, blockers);
        }
    }
}

impl RoomSimulation {
    pub(super) fn queue_combat_impulse(
        &mut self,
        wire: u32,
        tick: u32,
        delta: Vec3,
    ) -> Result<(), CombatError> {
        let change = VelocityChange::Impulse(delta);
        if !change.valid() {
            return Ok(());
        }
        if let Some(id) = self.player_handle_for_wire(wire) {
            if let Some(player) = self.player_body_mut(id).filter(|state| state.alive()) {
                // Input frames can run ahead of the room clock. Apply once at
                // the next actual pose so correction carries that pose's frame.
                let frame = player.mover.pose().frame.unwrap_or(player.frame);
                player
                    .mover
                    .queue_velocity_change(frame.saturating_add(1), change);
            }
        } else if let Some(state) = self.state_mut(wire)? {
            if !state.bot.dead && state.vitals.death().is_none() {
                let frame = tick.saturating_add(1);
                let applied = state
                    .enemy
                    .as_mut()
                    .filter(|enemy| enemy.fault.is_none())
                    .is_some_and(|enemy| enemy.queue_active_mover_change(frame, change));
                if !applied {
                    state.bot.queue_velocity_changes(vec![(frame, change)]);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn enable_movement_contacts(
        &mut self,
        free_for_all: bool,
        movers: Arc<MoverCatalog>,
    ) {
        self.movement_contacts = Some(MovementContacts {
            free_for_all,
            movers,
            frame: None,
        });
    }

    pub(crate) fn movement_contact_frame(&self) -> Option<Arc<MovementContactFrame>> {
        let (_, frame) = self.movement_contacts.as_ref()?.frame.as_ref()?;
        Some(Arc::clone(frame))
    }

    /// Freeze contacts before the first movement in this room tick.
    pub(crate) fn prepare_movement_frame(&mut self, tick: u32) -> Result<(), CombatError> {
        let Some(contacts) = self.movement_contacts.as_ref() else {
            return Ok(());
        };
        if contacts
            .frame
            .as_ref()
            .is_some_and(|(frame, _)| *frame == tick)
        {
            return Ok(());
        }
        let team = |team| (!contacts.free_for_all).then_some(team);
        let mut actors = BTreeMap::new();
        for state in self.states() {
            let state = state?;
            let bot = &state.bot;
            if bot.dead || state.vitals.death().is_some() {
                continue;
            }
            let settings = contacts.movers.for_body(Some(bot.spawn.body));
            let enabled = state
                .enemy
                .as_ref()
                .filter(|enemy| enemy.fault.is_none())
                .map_or(settings.character_collision != 0, |enemy| {
                    enemy.scripts.character_collision(tick, settings)
                });
            if enabled {
                actors.insert(
                    bot.entity,
                    (team(bot.spawn.team), bot.contact_capsule(settings)),
                );
            }
        }
        for (&wire, &entity) in &self.players_by_wire {
            let state = self
                .world
                .get::<PlayerBodyState>(entity)
                .ok_or(CombatError::MissingPlayerBody { wire, entity })?;
            if state.id.entity != wire {
                return Err(CombatError::PlayerBodyIdentity {
                    wire,
                    actual: state.id.entity,
                });
            }
            if state.alive() && state.mover.character_collision_for_snapshot(state.frame) {
                actors.insert(wire, (team(state.team), state.mover.contact_capsule()));
            }
        }
        if let Some(contacts) = &mut self.movement_contacts {
            contacts.frame = Some((tick, Arc::new(MovementContactFrame { actors })));
        }
        Ok(())
    }
}
