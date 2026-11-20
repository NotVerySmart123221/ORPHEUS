//! Settle authored map hazards from a room's final actor poses.

use std::time::Duration;

use crate::{map_environment::MapEnvironment, vitals::Death};

use super::{CombatError, PlayerBodyState, RoomSimulation, health::HealthEffect};

#[cfg(test)]
mod tests;

impl RoomSimulation {
    pub(crate) fn set_environment(&mut self, environment: MapEnvironment) {
        self.environment = environment;
    }

    pub(crate) fn player_respawn_delay(&self, mode: u64) -> Option<Duration> {
        self.environment.player_respawn_delay(mode)
    }

    pub(super) fn settle_environment(
        &mut self,
        tick: u32,
        now: Duration,
    ) -> Result<(), CombatError> {
        if self.environment.is_empty() {
            return Ok(());
        }

        let mut victims = Vec::new();
        for state in self.states() {
            let state = state?;
            if !state.bot.dead
                && state.vitals.death().is_none()
                && self.environment.lethal_at(state.bot.position)
            {
                victims.push(state.bot.entity);
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
            if state.alive()
                && state.vitals().is_some()
                && self.environment.lethal_at(state.mover.pose().position)
            {
                victims.push(wire);
            }
        }

        victims.sort_unstable();
        let cause = Death {
            time: now.as_secs_f64(),
            frame: tick,
            killer: 0,
        };
        for wire in victims {
            // Use the ordinary health/death path and consume all finite pools,
            // including armor and temporary shields.
            self.settle_health(wire, HealthEffect::Damage(f64::MAX), cause, now)?;
        }
        Ok(())
    }
}
