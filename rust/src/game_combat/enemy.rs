//! ECS bot and PvE encounter enemy component state.

use std::{collections::HashMap, io, sync::Arc, time::Duration};

use bevy_ecs::component::Component;

use crate::{
    commands::Command,
    game_bots::Bot,
    game_hitscan::TargetHistory,
    junkenstein_navigation::{
        BossPursuit, BossTarget, GateApproach, JUNKRAT, REAPER, ROADHOG, ReaperExit, SUMMONER,
        WITCH,
    },
    scene_assets::BotQueries,
    static_ray::norm,
    vitals::Vitals,
};

use super::eye_height;

#[derive(Component)]
pub(super) struct BotState {
    pub(super) bot: Bot,
    pub(super) vitals: Vitals,
    pub(super) history: TargetHistory,
    pub(super) respawn_at: Option<Duration>,
    pub(super) enemy: Option<EnemyState>,
}

pub(super) struct EnemyState {
    pub(super) scripts: crate::script_enemy::EnemyScripts,
    pub(super) binding: crate::script_enemy::EnemyBinding,
    pub(super) graphs: Arc<crate::script_graph::GraphStore>,
    pub(super) codecs: Arc<crate::script_codecs::CodecTable>,
    pub(super) fault: Option<String>,
    pub(super) gate_approach: Option<GateApproach>,
    pub(super) objective_mover_attached: bool,
    pub(super) reaper_exit: Option<ReaperExit>,
    pub(super) boss_pursuit: Option<Box<BossPursuit>>,
    pub(super) boss_handoff_complete: bool,
}

impl EnemyState {
    pub(super) fn halt(&mut self, bot: &mut Bot, error: String) {
        self.scripts.clear_pending_script_motion();
        bot.clear_velocity_changes();
        self.fault = Some(error);
    }

    pub(super) fn select_witch_targets(
        &mut self,
        bot: &mut Bot,
        tick: u32,
        queries: &BotQueries<'_>,
        targets: &[crate::script_enemy::EnemyTarget],
        allies: &HashMap<u32, (bool, f64)>,
    ) -> Result<(), crate::script_init::InitFault> {
        let mut ally = None;
        let mut enemy = None;
        if self.scripts.caster_ready() && !bot.dead {
            for target in targets
                .iter()
                .filter(|target| target.alive && target.entity != bot.entity)
            {
                let distance = norm(std::array::from_fn(|axis| {
                    target.pose.feet[axis] - bot.position[axis]
                }));
                if target.team == self.binding.team && distance <= 40.0 {
                    if let Some(&(favored, fraction)) = allies.get(&target.entity) {
                        let priority = (!favored, fraction, distance, target.entity);
                        if ally.is_none_or(|best| priority < best) {
                            ally = Some(priority);
                        }
                    }
                } else if target.team == 0 && distance <= 60.0 {
                    let priority = (distance, target.entity);
                    if enemy.is_none_or(|best| priority < best) {
                        enemy = Some(priority);
                    }
                }
            }
        }
        let selected = ally
            .map(|(_, _, distance, entity)| (distance, entity, 12.0))
            .or_else(|| enemy.map(|(distance, entity)| (distance, entity, 20.0)))
            .and_then(|(distance, entity, reach)| {
                let target = targets.iter().find(|target| target.entity == entity)?;
                Some((distance, target, reach))
            });
        let aim = selected
            .filter(|&(distance, _, reach)| distance <= reach)
            .and_then(|(distance, target, _)| {
                let [x, y, z] = bot.position;
                let direction =
                    queries.visible_aim([x, y + eye_height(WITCH, false), z], target.pose)?;
                Some((distance, direction))
            });
        if let Some(pursuit) = self.boss_pursuit.as_mut() {
            match (aim, selected) {
                (Some((_, direction)), _) => {
                    pursuit.stop(bot, tick);
                    bot.apply_objective_aim(direction, tick);
                }
                (None, Some((_, target, reach))) => {
                    if !self.objective_mover_attached {
                        self.objective_mover_attached = queries.attach_objective_mover(bot);
                    }
                    if self.objective_mover_attached {
                        queries.pursue_caster_target(pursuit, bot, tick, target, reach);
                    }
                }
                (None, None) => pursuit.stop(bot, tick),
            }
        }
        self.scripts
            .set_witch_target_distance(aim.map(|(distance, _)| distance));
        self.scripts.publish_witch_targets(
            tick,
            enemy.map(|(_, entity)| entity),
            ally.map(|(_, _, _, entity)| entity),
        )
    }

    pub(super) fn select_summoner_targets(
        &mut self,
        bot: &mut Bot,
        tick: u32,
        queries: &BotQueries<'_>,
        targets: &[crate::script_enemy::EnemyTarget],
    ) -> Result<(), crate::script_init::InitFault> {
        let mut slots = [None; 2];
        let mut beam_rays = Default::default();
        let mut weapon = None;
        let pursuit = self.boss_pursuit.as_mut().expect("Summoner pursuit");
        let ready = self.scripts.caster_ready() && !bot.dead;
        let nearest = targets
            .iter()
            .filter(|target| ready && target.alive && target.team == 0)
            .map(|target| {
                let distance = norm(std::array::from_fn(|axis| {
                    target.pose.feet[axis] - bot.position[axis]
                }));
                (distance, target)
            })
            .min_by(|(a, first), (b, second)| {
                a.total_cmp(b)
                    .then_with(|| first.entity.cmp(&second.entity))
            });
        if let (Some([primary, secondary]), Some((distance, target))) =
            (self.scripts.summoner_reaches(), nearest)
        {
            slots = [primary * 1.25, secondary]
                .map(|reach| (distance <= reach).then_some(target.entity));
            let use_secondary = distance > primary + 1.0;
            let reach = if use_secondary {
                secondary
            } else {
                primary + 1.0
            };
            let [x, y, z] = bot.position;
            let aim = (distance <= reach)
                .then(|| queries.visible_aim([x, y + eye_height(SUMMONER, false), z], target.pose))
                .flatten();
            if let Some(aim) = aim {
                pursuit.stop(bot, tick);
                bot.apply_objective_aim(aim, tick);
                weapon = Some(BossTarget {
                    entity: target.entity,
                    secondary: use_secondary,
                });
                if !use_secondary {
                    if let Some(reach) = self.scripts.summoner_beam_reach() {
                        let pose = bot.movement(tick);
                        let forward = crate::game_hitscan::aim_direction(&Command {
                            yaw: pose.yaw,
                            pitch: pose.pitch,
                            ..Command::default()
                        });
                        beam_rays = queries.summoner_beam_contacts(
                            [x, y + eye_height(SUMMONER, false), z],
                            forward,
                            reach,
                            targets,
                        );
                    }
                }
            } else {
                if !self.objective_mover_attached {
                    self.objective_mover_attached = queries.attach_objective_mover(bot);
                }
                if self.objective_mover_attached {
                    queries.pursue_caster_target(pursuit, bot, tick, target, primary);
                }
            }
        } else {
            pursuit.stop(bot, tick);
        }
        self.scripts.set_summoner_rays(beam_rays);
        self.scripts.publish_summoner_targets(tick, slots, weapon)
    }

    pub(super) fn aim_zombardier(
        &mut self,
        bot: &mut Bot,
        tick: u32,
        queries: &BotQueries<'_>,
        targets: &[crate::script_enemy::EnemyTarget],
    ) {
        let target = self
            .scripts
            .grenade_arc(&self.graphs, &self.binding)
            .and_then(|arc| {
                let mut enemies = targets
                    .iter()
                    .filter(|target| target.alive && target.team != self.binding.team)
                    .map(|target| {
                        let distance = norm(std::array::from_fn(|axis| {
                            target.pose.feet[axis] - bot.position[axis]
                        }));
                        (distance, target)
                    })
                    .filter(|(distance, _)| *distance <= arc.range)
                    .collect::<Vec<_>>();
                enemies.sort_unstable_by(|(a, first), (b, second)| {
                    a.total_cmp(b)
                        .then_with(|| first.entity.cmp(&second.entity))
                });
                enemies.into_iter().find_map(|(_, target)| {
                    let direction =
                        arc.aim(bot.position, target.pose, &|origin, direction, length| {
                            queries
                                .hitscan_wall(origin, direction, length)
                                .is_ok_and(|wall| wall.is_none())
                        })?;
                    bot.apply_objective_aim(direction, tick);
                    Some(BossTarget {
                        entity: target.entity,
                        secondary: false,
                    })
                })
            });
        self.scripts.set_weapon_target(target);
    }

    pub(super) fn approach_gate(
        &mut self,
        bot: &mut Bot,
        tick: u32,
        queries: &BotQueries<'_>,
    ) -> Result<(), crate::script_init::InitFault> {
        let Some(approach) = &mut self.gate_approach else {
            return Ok(());
        };
        let Some(instance) = self.scripts.controller.gate_approach_instance() else {
            bot.stop_objective(tick);
            return Ok(());
        };
        if !self.objective_mover_attached {
            self.objective_mover_attached = queries.attach_objective_mover(bot);
            if !self.objective_mover_attached {
                self.scripts.set_objective_route(false);
                return Ok(());
            }
        }
        if queries.step_gate_approach(approach, bot, tick, self.scripts.movement_speed_scale(tick))
        {
            self.scripts.controller.queue_gate_arrival(tick, instance)?;
        }
        self.scripts.set_objective_route(approach.active());
        Ok(())
    }

    pub(super) fn advance_boss_weapon(
        &mut self,
        bot: &mut Bot,
        tick: u32,
        queries: &BotQueries<'_>,
        paused: bool,
        defenders: &[crate::script_enemy::EnemyTarget],
    ) -> io::Result<()> {
        if !matches!(self.binding.hero, REAPER | ROADHOG | JUNKRAT) {
            return Ok(());
        }
        let Some(pursuit) = self.boss_pursuit.as_mut() else {
            return Ok(());
        };
        // The completed intro graph is later retired from the body script tree.
        self.boss_handoff_complete =
            self.boss_handoff_complete || self.scripts.boss_combat_handoff(&self.binding);
        let ready = !paused
            && self.boss_handoff_complete
            && self.scripts.cinematic_aim().is_none()
            && self.scripts.facing_direction().is_none();
        let mut target = None;
        if let Some(exit) = &mut self.reaper_exit {
            let can_walk = ready
                && defenders
                    .iter()
                    .any(|target| target.team == 0 && target.alive);
            if queries.step_reaper_exit(exit, bot, tick, can_walk)? {
                self.reaper_exit = None;
            }
        } else if ready {
            if !self.objective_mover_attached {
                self.objective_mover_attached = queries.attach_objective_mover(bot);
            }
            if self.objective_mover_attached {
                let arc = self.scripts.grenade_arc(&self.graphs, &self.binding);
                target = queries.step_boss_pursuit(pursuit, bot, tick, defenders, arc);
            }
        } else {
            pursuit.stop(bot, tick);
        }
        self.scripts.set_weapon_target(target);
        Ok(())
    }
}

impl BotState {
    pub(super) fn health_target_facts(&self) -> crate::script_init::HealthTargetFacts {
        crate::script_init::HealthTargetFacts {
            target: self.bot.entity,
            room_status: None,
            filter_flags: crate::match_scene::team_bit(self.bot.spawn.team)
                .ok()
                .map(|team| 1 | team | if self.bot.dead { 0x10 } else { 0 }),
            tags: self.enemy.as_ref().map_or_else(Default::default, |enemy| {
                enemy.scripts.target_tags(&enemy.graphs, &enemy.binding)
            }),
            script_state: self.script_state(),
        }
    }

    pub(super) fn script_state(&self) -> crate::script_entity_state::EntityScriptState {
        use crate::script_entity_state::EntityScriptState;
        let Some(enemy) = &self.enemy else {
            return EntityScriptState::Absent;
        };
        if enemy.fault.is_some() {
            return EntityScriptState::Unavailable;
        }
        enemy
            .scripts
            .body
            .snapshot()
            .map_or(EntityScriptState::Unavailable, |snapshot| {
                EntityScriptState::Running(Arc::new(snapshot.variables.clone()))
            })
    }

    pub(super) fn new(bot: Bot, replacement: Option<u32>) -> Self {
        Self {
            vitals: Vitals::new(bot.spawn.health, 0.0, 0.0),
            bot,
            history: TargetHistory::new(replacement),
            respawn_at: None,
            enemy: None,
        }
    }
}
