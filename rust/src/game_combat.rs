//! Match-authoritative direct damage and training bot lifetimes.
//!
//! The projectile and body scripts supply shot events. The match supplies
//! the command and pose at consumption, and owns identity-gated delivery.
//! Hit selection, damage, and respawn order follow ow174/game/combat.py.

use std::{collections::HashMap, io, time::Duration};

use bevy_ecs::{change_detection::Mut, component::Component, entity::Entity, world::World};

use crate::{
    commands::Command,
    game_bots::{BOT_ENTITY, BOT_RECORD_DELAY, BOT_RESPAWN_TIME, Bot},
    game_combat_messages::CombatNotice,
    game_heroes::SOLDIER,
    game_hitscan::{HitDefinition, HitscanShot, Impact, TargetHistory, ViewFrame},
    match_scene::bot_create,
    projectile_flight::{FlightEnd, ProjectileBallistics, ProjectileFlight},
    scene_assets::PreparedScene,
    script_init::ShotEvent,
    vitals::{Death, Vitals},
    world::{EntityUpdate, Operation},
};

pub(crate) fn eye_height(hero: u64, crouched: bool) -> f64 {
    match (crouched, hero) {
        (false, _) => 1.55,
        (true, SOLDIER) => 1.085,
        (true, _) => 1.15,
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PosedHitscan {
    pub source: u32,
    pub origin: [f64; 3],
    pub command: Command,
    pub shot_frame: u32,
    pub reach: f64,
    pub hit: HitDefinition,
}

pub(crate) enum PosedShot {
    Hitscan(PosedHitscan),
    Projectile(PosedProjectile),
}

pub(crate) struct PosedProjectile {
    pub shot: ShotEvent,
    pub ballistics: ProjectileBallistics,
    pub origin: [f64; 3],
    pub direction: [f64; 3],
}

#[derive(Clone, Copy)]
pub(crate) struct ShotContext {
    pub team: u8,
    pub quanta: u32,
    pub tick: u32,
    pub now: Duration,
}

pub(crate) struct BotHit {
    pub notices: [Option<CombatNotice>; 2],
    pub health: EntityUpdate,
}

#[derive(Clone)]
pub(crate) struct BotReplacement {
    pub retired: u32,
    pub created: EntityUpdate,
}

#[derive(Component)]
struct Target {
    vitals: Vitals,
    history: TargetHistory,
    respawn_at: Option<Duration>,
}

impl Target {
    fn new(health: f64, replacement: Option<u32>) -> Self {
        Self {
            vitals: Vitals::new(health, 0.0, 0.0),
            history: TargetHistory::new(replacement),
            respawn_at: None,
        }
    }
}

/// Combat state belongs to a body generation in this room.
pub(crate) struct Combat {
    world: World,
    by_wire: HashMap<u32, Entity>,
    next_bot: u32,
}

impl Combat {
    pub(crate) fn new(scene: &PreparedScene) -> Self {
        Self::for_bots(scene.bots())
    }

    fn for_bots(bots: &[Bot]) -> Self {
        let mut world = World::new();
        let by_wire = bots
            .iter()
            .map(|bot| {
                let entity = world.spawn(Target::new(bot.spawn.health, None)).id();
                (bot.entity, entity)
            })
            .collect();
        Self {
            world,
            by_wire,
            next_bot: BOT_ENTITY + bots.len() as u32,
        }
    }

    fn target(&self, wire_id: u32) -> Option<&Target> {
        self.world.get(*self.by_wire.get(&wire_id)?)
    }

    fn target_mut(&mut self, wire_id: u32) -> Option<Mut<'_, Target>> {
        self.world.get_mut(*self.by_wire.get(&wire_id)?)
    }

    pub(crate) fn debug_health(&self, wire_id: u32) -> Option<f64> {
        self.target(wire_id).map(|target| target.vitals.total())
    }

    pub(crate) fn record_poses(&mut self, scene: &PreparedScene, tick: u32) {
        for bot in scene.bots().iter().filter(|bot| !bot.dead) {
            if let Some(mut target) = self.target_mut(bot.entity) {
                target.history.record(tick, scene.bot_target(bot));
            }
        }
    }

    pub(crate) fn hitscan(
        &mut self,
        scene: &mut PreparedScene,
        shot: PosedHitscan,
        context: ShotContext,
        visible: impl Fn(u32) -> bool,
    ) -> io::Result<Option<BotHit>> {
        let view = ViewFrame::from_command(&shot.command, context.quanta);
        let ray = HitscanShot::from_command(shot.origin, &shot.command, shot.reach, shot.hit);
        let free_for_all = scene.map().free_for_all;
        // Scene order breaks exact ray ties.
        let candidates = scene.bots().iter().filter_map(|bot| {
            if !eligible_bot(bot, shot.source, context.team, free_for_all) || !visible(bot.entity) {
                return None;
            }
            self.target(bot.entity)?
                .history
                .at(view, scene.bot_target(bot))
                .map(|pose| (bot.entity, pose))
        });
        let Some(impact) = ray
            .resolve(candidates, |origin, direction, reach| {
                scene.hitscan_wall(origin, direction, reach)
            })
            .map_err(io::Error::other)?
        else {
            return Ok(None);
        };
        Ok(self.apply_bot_impact(
            scene.bots_mut(),
            shot.source,
            shot.shot_frame,
            context,
            &impact,
        ))
    }

    pub(crate) fn step_projectile(
        &mut self,
        scene: &mut PreparedScene,
        source: u32,
        flight: &mut ProjectileFlight,
        context: ShotContext,
        hits: &mut Vec<BotHit>,
    ) -> io::Result<Option<FlightEnd>> {
        let free_for_all = scene.map().free_for_all;
        let candidates: Vec<_> = scene
            .bots()
            .iter()
            .filter(|bot| eligible_bot(bot, source, context.team, free_for_all))
            .map(|bot| (bot.entity, scene.bot_target(bot)))
            .collect();
        let end = flight
            .advance(context.tick, &candidates, |origin, direction, reach| {
                scene.hitscan_wall(origin, direction, reach)
            })
            .map_err(io::Error::other)?;
        if let Some(end) = &end {
            hits.extend(end.impacts.iter().filter_map(|impact| {
                self.apply_bot_impact(scene.bots_mut(), source, context.tick, context, impact)
            }));
        }
        Ok(end)
    }

    fn apply_bot_impact(
        &mut self,
        bots: &mut [Bot],
        source: u32,
        shot_frame: u32,
        context: ShotContext,
        impact: &Impact,
    ) -> Option<BotHit> {
        let bot = bots
            .iter_mut()
            .find(|bot| !bot.dead && bot.entity == impact.entity)?;
        let mut target = self.target_mut(bot.entity)?;
        let result = target.vitals.damage(
            impact.damage,
            Death {
                time: context.now.as_secs_f64(),
                frame: context.tick,
                killer: source,
            },
        );
        if result.dealt == 0.0 {
            return None;
        }
        if result.killed {
            bot.died(context.tick);
            target.respawn_at = Some(context.now.saturating_add(BOT_RESPAWN_TIME));
        }
        let health = EntityUpdate {
            entity: bot.entity,
            operation: Some(Operation::Update(vec![target.vitals.component()])),
            ..EntityUpdate::default()
        };
        target.vitals.clear_changed();
        Some(BotHit {
            notices: [
                Some(CombatNotice::Hit {
                    target: bot.entity,
                    dealt: result.dealt,
                    critical: impact.critical,
                    killed: result.killed,
                    shot_frame,
                }),
                result.killed.then_some(CombatNotice::Elimination {
                    killer: source,
                    victim: bot.entity,
                }),
            ],
            health,
        })
    }

    pub(crate) fn respawn_due(
        &mut self,
        scene: &mut PreparedScene,
        tick: u32,
        now: Duration,
    ) -> io::Result<Vec<BotReplacement>> {
        self.respawn_bots(scene.bots_mut(), tick, now)
    }

    fn respawn_bots(
        &mut self,
        bots: &mut [Bot],
        tick: u32,
        now: Duration,
    ) -> io::Result<Vec<BotReplacement>> {
        // Validate the entire batch before retiring any entity handles.
        let mut prepared = Vec::new();
        let mut next_bot = self.next_bot;
        for (slot, bot) in bots.iter().enumerate() {
            let Some(&previous) = self.by_wire.get(&bot.entity) else {
                continue;
            };
            let deadline = self
                .world
                .get::<Target>(previous)
                .and_then(|target| target.respawn_at);
            if !deadline.is_some_and(|deadline| deadline <= now) {
                continue;
            }
            let fresh = bot.respawned(next_bot, tick.saturating_add(BOT_RECORD_DELAY));
            next_bot = next_bot
                .checked_add(1)
                .ok_or_else(|| io::Error::other("bot entity IDs exhausted"))?;
            let created = bot_create(&fresh, None)?;
            prepared.push((
                slot,
                previous,
                fresh,
                BotReplacement {
                    retired: bot.entity,
                    created,
                },
            ));
        }
        self.next_bot = next_bot;
        Ok(prepared
            .into_iter()
            .map(|(slot, previous, fresh, replacement)| {
                self.by_wire.remove(&replacement.retired);
                let removed = self.world.despawn(previous);
                debug_assert!(removed);
                let entity = self
                    .world
                    .spawn(Target::new(fresh.spawn.health, Some(tick)))
                    .id();
                self.by_wire.insert(fresh.entity, entity);
                bots[slot] = fresh;
                replacement
            })
            .collect())
    }
}

fn eligible_bot(bot: &Bot, source: u32, team: u8, free_for_all: bool) -> bool {
    !bot.dead && bot.entity != source && (free_for_all || bot.spawn.team != team)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{game_content::GameMap, game_instance::RETAIL_QUANTA, scene_assets::SceneAssets};

    fn fixture_bots() -> Vec<Bot> {
        crate::game_bots::BotCatalog::from_json(
            r#"{
            "heroes":{"0x01":{"name":"Test bot","body":"0x02","health":100}},
            "maps":{"0x03":{"hints":{},"spawners":[
                {"hero":"0x01","team":1,"position":[1,0,0],"rotation":[0,0,0,1],"graphs":[]},
                {"hero":"0x01","team":1,"position":[2,0,0],"rotation":[0,0,0,1],"graphs":[]}
            ]}}
        }"#,
        )
        .unwrap()
        .make(3, |_| crate::game_bots::BotWalk::default())
    }

    fn fixture_impact(entity: u32, damage: f64) -> Impact {
        Impact {
            entity,
            distance: 1.0,
            damage,
            critical: false,
            loadout: None,
        }
    }

    const FIXTURE_CONTEXT: ShotContext = ShotContext {
        team: 0,
        quanta: RETAIL_QUANTA,
        tick: 1000,
        now: Duration::from_secs(4),
    };

    #[test]
    fn bot_damage_history_and_respawn_follow_identity_after_scene_reorder() {
        use crate::game_hitscan::TargetPose;

        let mut bots = fixture_bots();
        let mut combat = Combat::for_bots(&bots);
        let first = bots[0].entity;
        let second = bots[1].entity;
        let first_handle = combat.by_wire[&first];
        let second_handle = combat.by_wire[&second];
        let context = FIXTURE_CONTEXT;
        let pose = TargetPose {
            feet: bots[0].position,
            height: 1.8,
            radius: 0.5,
            eye: 1.6,
        };
        let second_pose = TargetPose {
            feet: bots[1].position,
            ..pose
        };
        combat
            .target_mut(first)
            .unwrap()
            .history
            .record(context.tick, pose);
        combat
            .target_mut(second)
            .unwrap()
            .history
            .record(context.tick, second_pose);

        bots.swap(0, 1);
        let result = combat
            .apply_bot_impact(
                &mut bots,
                7,
                context.tick,
                context,
                &fixture_impact(first, 100.0),
            )
            .unwrap();
        let due = context.now + BOT_RESPAWN_TIME;
        let view = ViewFrame::from_command(
            &Command {
                frame: context.tick,
                ..Command::default()
            },
            context.quanta,
        );
        assert_eq!(result.health.entity, first);
        assert_eq!(
            result.notices[1],
            Some(CombatNotice::Elimination {
                killer: 7,
                victim: first
            })
        );
        assert!(bots[1].dead);
        assert!(!bots[0].dead);
        let target = combat.target(first).unwrap();
        assert_eq!(target.vitals.total(), 0.0);
        assert_eq!(target.respawn_at, Some(due));
        assert_eq!(target.history.at(view, second_pose), Some(pose));
        let other = combat.target(second).unwrap();
        assert_eq!(other.vitals.total(), 100.0);
        assert_eq!(other.respawn_at, None);
        assert_eq!(other.history.at(view, pose), Some(second_pose));

        let replacements = combat
            .respawn_bots(&mut bots, context.tick + 1, due)
            .unwrap();
        let [replacement] = replacements.as_slice() else {
            panic!("expected one due replacement");
        };
        let fresh = replacement.created.entity;
        assert_eq!(replacement.retired, first);
        assert_eq!(fresh, BOT_ENTITY + 2);
        assert_eq!(bots[1].entity, fresh);
        assert_eq!(bots[0].entity, second);
        assert!(!bots[1].dead);
        assert!(combat.target(first).is_none());
        assert!(combat.world.get::<Target>(first_handle).is_none());
        assert_eq!(combat.by_wire[&second], second_handle);
        let target = combat.target(fresh).unwrap();
        assert_eq!(target.vitals.total(), 100.0);
        assert_eq!(target.respawn_at, None);
        assert_eq!(target.history.at(view, pose), None);
        assert_eq!(
            combat.target(second).unwrap().history.at(view, pose),
            Some(second_pose)
        );
        assert!(
            combat
                .apply_bot_impact(
                    &mut bots,
                    7,
                    context.tick,
                    context,
                    &fixture_impact(first, 100.0)
                )
                .is_none()
        );
    }

    #[test]
    fn failed_respawn_batch_preserves_all_due_bodies() {
        let mut bots = fixture_bots();
        let mut combat = Combat::for_bots(&bots);
        let identities = [bots[0].entity, bots[1].entity];
        let context = FIXTURE_CONTEXT;
        for identity in identities {
            combat
                .apply_bot_impact(
                    &mut bots,
                    7,
                    context.tick,
                    context,
                    &fixture_impact(identity, 100.0),
                )
                .unwrap();
        }
        let due = context.now + BOT_RESPAWN_TIME;
        let first_free = u32::MAX - 1;
        combat.next_bot = first_free;

        assert!(
            combat
                .respawn_bots(&mut bots, context.tick + 1, due)
                .is_err()
        );
        assert_eq!(combat.next_bot, first_free);
        for (identity, bot) in identities.into_iter().zip(&bots) {
            assert_eq!(bot.entity, identity);
            assert!(bot.dead);
            let target = combat.target(identity).unwrap();
            assert_eq!(target.vitals.total(), 0.0);
            assert_eq!(target.respawn_at, Some(due));
        }
    }

    #[test]
    #[ignore = "requires a prepared 104319 Practice Range scene"]
    fn repeated_rifle_hits_retire_and_respawn_only_the_hit_bot() {
        let runtime = std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap();
        let mut scene = SceneAssets::load(runtime)
            .unwrap()
            .prepare(GameMap::practice_range())
            .unwrap();
        let slot = scene
            .bots()
            .iter()
            .position(|bot| bot.spawn.team == 1)
            .unwrap();
        let bot = &scene.bots()[slot];
        let entity = bot.entity;
        let [x, y, z] = bot.position;
        let count = scene.bots().len();
        let mut combat = Combat::new(&scene);
        let mut context = ShotContext {
            team: 0,
            quanta: RETAIL_QUANTA,
            tick: 1000,
            now: Duration::from_secs(4),
        };
        combat.record_poses(&scene, context.tick);
        let mut shot = PosedHitscan {
            source: 0xa000_0101,
            origin: [x - 8.0, y + eye_height(SOLDIER, false), z],
            command: Command {
                frame: context.tick,
                yaw: 16384,
                ..Command::default()
            },
            shot_frame: 999,
            reach: 180.0,
            hit: HitDefinition {
                amount: 20.0,
                loadout: None,
                critical: 2.0,
                falloff: None,
            },
        };
        assert!(
            combat
                .hitscan(&mut scene, shot, context, |_| false)
                .unwrap()
                .is_none()
        );
        assert!(
            combat
                .hitscan(&mut scene, shot, ShotContext { team: 1, ..context }, |_| {
                    true
                })
                .unwrap()
                .is_none()
        );
        for index in 0..5 {
            let result = combat
                .hitscan(&mut scene, shot, context, |_| true)
                .unwrap()
                .unwrap();
            let terminal = index == 4;
            assert_eq!(result.health.entity, entity);
            assert_eq!(
                result.notices[0],
                Some(CombatNotice::Hit {
                    target: entity,
                    dealt: 40.0,
                    critical: true,
                    killed: terminal,
                    shot_frame: shot.shot_frame,
                })
            );
            assert_eq!(
                result.notices[1],
                terminal.then_some(CombatNotice::Elimination {
                    killer: shot.source,
                    victim: entity,
                })
            );
            assert_eq!(
                combat.target(entity).unwrap().vitals.total(),
                160.0 - f64::from(index) * 40.0
            );
            assert_eq!(scene.bots()[slot].dead, terminal);
        }
        assert!(
            combat
                .hitscan(&mut scene, shot, context, |_| true)
                .unwrap()
                .is_none()
        );
        let due = context.now + BOT_RESPAWN_TIME;
        assert!(
            combat
                .respawn_due(&mut scene, 1187, due - Duration::from_millis(1))
                .unwrap()
                .is_empty()
        );
        let [replacement] = combat
            .respawn_due(&mut scene, 1188, due)
            .unwrap()
            .try_into()
            .ok()
            .unwrap();
        assert_eq!(replacement.retired, entity);
        let next = BOT_ENTITY + count as u32;
        assert_eq!(replacement.created.entity, next);
        assert!(matches!(
            replacement.created.operation,
            Some(Operation::Create(_))
        ));
        assert!(!scene.bots()[slot].dead);
        assert_eq!(combat.target(next).unwrap().vitals.total(), 200.0);
        context.tick = 1188;
        context.now = due;
        combat.record_poses(&scene, context.tick);
        assert!(
            combat
                .hitscan(&mut scene, shot, context, |_| true)
                .unwrap()
                .is_none()
        );
        shot.command.frame = context.tick;
        assert!(
            combat
                .hitscan(&mut scene, shot, context, |visible| visible == next)
                .unwrap()
                .is_some()
        );
        assert_eq!(combat.target(next).unwrap().vitals.total(), 160.0);
    }
}
