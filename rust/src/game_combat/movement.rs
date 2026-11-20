//! A room-tick snapshot for player and NPC capsule movement.

use std::{collections::BTreeMap, sync::Arc};

use crate::{contact_cache::CapsulePose, game_bots::Bot, mover_settings::MoverCatalog};

use super::{CombatError, PlayerBodyState, RoomSimulation};

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
            bot.set_movement_blockers(tick, self.blockers(bot.entity, bot.spawn.team));
        }
    }
}

impl RoomSimulation {
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
            if !bot.dead && state.vitals.death().is_none() {
                let settings = contacts.movers.for_body(Some(bot.spawn.body));
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
            if state.alive() {
                actors.insert(wire, (team(state.team), state.mover.contact_capsule()));
            }
        }
        if let Some(contacts) = &mut self.movement_contacts {
            contacts.frame = Some((tick, Arc::new(MovementContactFrame { actors })));
        }
        Ok(())
    }
}
