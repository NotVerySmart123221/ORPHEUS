//! Settle authored map hazards from a room's final actor poses.

use std::time::Duration;

use crate::{map_environment::MapEnvironment, vitals::Death};

use super::{CombatError, PlayerBodyState, RoomSimulation, health::HealthEffect};

#[cfg(test)]
mod tests;

impl RoomSimulation {
    pub(crate) fn set_environment(&mut self, environment: MapEnvironment) {
        let pack_count = environment.health_packs().len();
        self.environment = environment;
        self.health_pack_states = vec![Default::default(); pack_count];
    }

    pub(crate) fn player_respawn_delay(&self, mode: u64) -> Option<Duration> {
        self.environment.player_respawn_delay(mode)
    }

    #[allow(dead_code)]
    pub(crate) fn hack_health_pack(&mut self, position: [f64; 3], team: u8, tick: u32) -> bool {
        let packs = self.environment.health_packs();
        if self.health_pack_states.len() != packs.len() {
            self.health_pack_states
                .resize(packs.len(), Default::default());
        }
        let mut best: Option<(usize, f64)> = None;
        for (idx, pack) in packs.iter().enumerate() {
            let d = (pack.position[0] - position[0])
                .hypot(pack.position[1] - position[1])
                .hypot(pack.position[2] - position[2]);
            if d <= 15.0 && best.is_none_or(|(_, prev)| d < prev) {
                best = Some((idx, d));
            }
        }
        let Some((idx, _)) = best else {
            return false;
        };
        self.health_pack_states[idx].hacked_by = Some((team, tick.saturating_add(62 * 45)));
        true
    }

    pub(super) fn settle_environment(
        &mut self,
        tick: u32,
        now: Duration,
    ) -> Result<(), CombatError> {
        self.step_health_packs_and_jump_pads(tick, now)?;
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

    fn step_health_packs_and_jump_pads(
        &mut self,
        tick: u32,
        now: Duration,
    ) -> Result<(), CombatError> {
        let pack_specs: Vec<_> = self.environment.health_packs().to_vec();
        if !pack_specs.is_empty() {
            if self.health_pack_states.len() != pack_specs.len() {
                self.health_pack_states
                    .resize(pack_specs.len(), Default::default());
            }
            for state in &mut self.health_pack_states {
                if state.hacked_by.is_some_and(|(_, until)| tick >= until) {
                    state.hacked_by = None;
                }
            }
            let players: Vec<(u32, u8, [f64; 3], bool)> = self
                .players_by_wire
                .iter()
                .filter_map(|(&wire, &entity)| {
                    let p = self.world.get::<PlayerBodyState>(entity)?;
                    let v = p.vitals()?;
                    let wounded = p.alive() && v.total() + 0.5 < v.maximum();
                    Some((wire, p.team, p.mover.pose().position, wounded))
                })
                .collect();
            let mut pickups: Vec<(usize, u32, f64, bool)> = Vec::new();
            for (idx, pack) in pack_specs.iter().enumerate() {
                let st = self.health_pack_states[idx];
                if tick < st.ready_at_tick {
                    continue;
                }
                for &(wire, team, pos, wounded) in &players {
                    if !wounded {
                        continue;
                    }
                    if st.hacked_by.is_some_and(|(hack_team, _)| hack_team != team) {
                        continue;
                    }
                    let d = (pos[0] - pack.position[0]).hypot(pos[2] - pack.position[2]);
                    let dy = (pos[1] - pack.position[1]).abs();
                    if d <= 1.6 && dy <= 1.5 {
                        pickups.push((idx, wire, pack.heal_amount(), st.hacked_by.is_some()));
                        break;
                    }
                }
            }
            let cause = Death {
                time: now.as_secs_f64(),
                frame: tick,
                killer: 0,
            };
            for (idx, wire, amount, hacked) in pickups {
                if let Some(outcome) =
                    self.apply_health(wire, HealthEffect::Heal(amount), cause, now, None)?
                {
                    if outcome.delta > 0.0 {
                        self.health_pack_states[idx].ready_at_tick =
                            tick.saturating_add(pack_specs[idx].respawn_ticks(hacked));
                        self.deliver_applied_heal(wire, wire, outcome.delta);
                    }
                }
            }
        }
        let pads: Vec<_> = self.environment.jump_pads().to_vec();
        if !pads.is_empty() {
            let mut launches = Vec::new();
            for (&wire, &entity) in &self.players_by_wire {
                let Some(p) = self.world.get::<PlayerBodyState>(entity) else {
                    continue;
                };
                if !p.alive() {
                    continue;
                }
                if self
                    .jump_pad_cooldowns
                    .get(&wire)
                    .is_some_and(|&until| tick < until)
                {
                    continue;
                }
                let pos = p.mover.pose().position;
                for pad in &pads {
                    let d = (pos[0] - pad.position[0]).hypot(pos[2] - pad.position[2]);
                    let dy = (pos[1] - pad.position[1]).abs();
                    if d <= pad.radius && dy <= 1.2 {
                        launches.push((wire, pad.velocity));
                        break;
                    }
                }
            }
            for (wire, vel) in launches {
                self.jump_pad_cooldowns
                    .insert(wire, tick.saturating_add(24));
                self.queue_combat_impulse(wire, tick, vel)?;
            }
        }
        let platforms: Vec<_> = self.environment.moving_platforms().to_vec();
        if !platforms.is_empty() {
            let dt = crate::game_clock::TICK_PERIOD.as_secs_f64();
            let now_secs = now.as_secs_f64();
            let prev_secs = (now_secs - dt).max(0.0);
            let mut carries = Vec::new();
            for (&wire, &entity) in &self.players_by_wire {
                let Some(p) = self.world.get::<PlayerBodyState>(entity) else {
                    continue;
                };
                if !p.alive() {
                    continue;
                }
                let pos = p.mover.pose().position;
                for plat in &platforms {
                    let prev_c = plat.position_at(prev_secs);
                    let cur_c = plat.position_at(now_secs);
                    let top_y = prev_c[1] + plat.half_extents[1];
                    if (pos[0] - prev_c[0]).abs() <= plat.half_extents[0] + 0.35
                        && (pos[2] - prev_c[2]).abs() <= plat.half_extents[2] + 0.35
                        && (pos[1] - top_y).abs() <= 0.55
                    {
                        let vel = [
                            (cur_c[0] - prev_c[0]) / dt,
                            (cur_c[1] - prev_c[1]) / dt,
                            (cur_c[2] - prev_c[2]) / dt,
                        ];
                        if vel[0].abs() + vel[1].abs() + vel[2].abs() > 1e-3 {
                            carries.push((wire, vel));
                        }
                        break;
                    }
                }
            }
            for (wire, vel) in carries {
                let _ = self.queue_combat_impulse(
                    wire,
                    tick,
                    [vel[0] * 0.06, vel[1] * 0.06, vel[2] * 0.06],
                );
            }
        }
        Ok(())
    }
}
