//! Read-only map collision and navigation queries shared with bot simulation.

use super::*;
use crate::{
    collision_index::IndexError,
    contact_gjk::sweep::MeshProbe,
    static_ray::{SurfaceHit, Vec3, along},
};

/// Map queries used by bot simulation without borrowing its entity storage.
pub(crate) struct BotQueries<'a> {
    free_for_all: bool,
    movers: &'a MoverCatalog,
    ground: &'a CollisionScene,
    combat_rays: &'a CollisionScene,
    combat_sweeps: Option<&'a Arc<ContactWorld>>,
    contacts: Option<&'a Arc<ContactWorld>>,
    hurt_volumes: Option<&'a HurtVolumeCatalog>,
}

impl<'a> BotQueries<'a> {
    pub(crate) fn new(
        free_for_all: bool,
        movers: &'a MoverCatalog,
        ground: &'a CollisionScene,
        contacts: Option<&'a Arc<ContactWorld>>,
    ) -> Self {
        Self {
            free_for_all,
            movers,
            ground,
            combat_rays: ground,
            combat_sweeps: contacts,
            contacts,
            hurt_volumes: None,
        }
    }

    pub(super) fn with_combat_geometry(
        mut self,
        rays: &'a CollisionScene,
        sweeps: Option<&'a Arc<ContactWorld>>,
    ) -> Self {
        self.combat_rays = rays;
        self.combat_sweeps = sweeps.or(self.combat_sweeps);
        self
    }

    pub(crate) fn with_hurt_volumes(mut self, volumes: &'a HurtVolumeCatalog) -> Self {
        self.hurt_volumes = Some(volumes);
        self
    }

    pub(crate) fn free_for_all(&self) -> bool {
        self.free_for_all
    }

    pub(crate) fn attach_objective_mover(&self, bot: &mut Bot) -> bool {
        self.attach_bot_mover(bot, MOVER_CATEGORY)
    }

    pub(crate) fn step_ai_walk(&self, bot: &mut Bot, tick: u32, destination: Vec3, speed: f64) {
        use crate::navigation::{PathOptions, find_path};

        let category = mover_category_for_team(self.free_for_all, bot.spawn.team);
        if bot.has_collision() || self.attach_bot_mover(bot, category) {
            let agent = self
                .movers
                .for_body(Some(bot.spawn.body))
                .nav_agent(category);
            bot.follow_walk_request(tick, destination, speed, |start| {
                find_path(
                    self.ground,
                    self.contacts.map(Arc::as_ref),
                    &agent,
                    start,
                    destination,
                    &PathOptions::default(),
                )
            });
        }
    }

    fn attach_bot_mover(&self, bot: &mut Bot, category: u16) -> bool {
        let Some(contacts) = self.contacts else {
            return false;
        };
        bot.attach_collision(
            self.movers.for_body(Some(bot.spawn.body)),
            Arc::clone(contacts),
            category,
            ground_height(self.ground.triangles(), category),
        );
        true
    }

    pub(crate) fn step_gate_approach(
        &self,
        approach: &mut crate::junkenstein_navigation::GateApproach,
        bot: &mut Bot,
        tick: u32,
        speed_scale: f64,
    ) -> bool {
        approach.step(
            bot,
            tick,
            self.ground,
            self.movers.for_body(Some(bot.spawn.body)),
            speed_scale,
        )
    }

    pub(crate) fn step_reaper_exit(
        &self,
        exit: &mut crate::junkenstein_navigation::ReaperExit,
        bot: &mut Bot,
        tick: u32,
        can_walk: bool,
    ) -> io::Result<bool> {
        exit.step(bot, self.ground.triangles(), tick, can_walk, |bot| {
            self.contacts
                .map(|contacts| {
                    Mover::new(
                        *self.movers.for_body(Some(bot.spawn.body)),
                        Arc::clone(contacts),
                        bot.position,
                        bot.movement(tick).yaw,
                        MOVER_CATEGORY,
                    )
                    .map_err(invalid)
                })
                .transpose()
        })
    }

    pub(crate) fn step_boss_pursuit(
        &self,
        pursuit: &mut crate::junkenstein_navigation::BossPursuit,
        bot: &mut Bot,
        tick: u32,
        defenders: &[crate::script_enemy::EnemyTarget],
        arc: Option<crate::junkenstein_navigation::GrenadeArc>,
    ) -> Option<crate::junkenstein_navigation::BossTarget> {
        pursuit.step(
            bot,
            tick,
            self.ground,
            (self.movers.for_body(Some(bot.spawn.body)), arc),
            defenders,
            |origin, direction, range| {
                self.hitscan_wall(origin, direction, range)
                    .is_ok_and(|wall| wall.is_none())
            },
        )
    }

    pub(crate) fn pursue_caster_target(
        &self,
        pursuit: &mut crate::junkenstein_navigation::BossPursuit,
        bot: &mut Bot,
        tick: u32,
        target: &crate::script_enemy::EnemyTarget,
        reach: f64,
    ) {
        let eye = crate::game_combat::eye_height(bot.spawn.hero, false);
        pursuit.pursue(
            bot,
            tick,
            (self.ground, self.movers.for_body(Some(bot.spawn.body))),
            target,
            reach,
            |[x, y, z], pose| self.visible_aim([x, y + eye, z], pose).is_some(),
        );
    }

    pub(crate) fn target(&self, bot: &Bot) -> crate::game_hitscan::TargetPose {
        let settings = self.movers.for_body(Some(bot.spawn.body));
        crate::game_hitscan::TargetPose {
            feet: bot.position,
            height: settings.stand_height,
            radius: settings.stand_radius,
            eye: crate::game_combat::eye_height(bot.spawn.hero, false),
        }
    }

    pub(crate) fn hurt_pose(
        &self,
        body: u64,
        pose: crate::game_hitscan::TargetPose,
    ) -> crate::game_hitscan::HurtPose {
        self.hurt_volumes
            .map_or_else(|| pose.into(), |volumes| volumes.pose(body, pose))
    }

    pub(crate) fn hitscan_wall(
        &self,
        origin: [f64; 3],
        direction: [f64; 3],
        reach: f64,
    ) -> Result<Option<f64>, crate::collision_index::IndexError> {
        self.combat_wall(origin, direction, reach)
            .map(|hit| hit.map(|hit| hit.distance))
    }

    pub(crate) fn combat_wall(
        &self,
        origin: [f64; 3],
        direction: [f64; 3],
        reach: f64,
    ) -> Result<Option<crate::collision_index::RayHit>, crate::collision_index::IndexError> {
        self.combat_rays
            .cast_ray(origin, direction, reach, MOVER_CATEGORY)
    }

    #[cfg(any(test, feature = "inspector"))]
    pub(crate) fn combat_ray_geometry(&self) -> &'static str {
        if std::ptr::eq(self.combat_rays, self.ground) {
            "movement"
        } else {
            "expanded"
        }
    }

    #[cfg(any(test, feature = "inspector"))]
    pub(crate) fn combat_ray_source(
        &self,
        hit: crate::collision_index::RayHit,
    ) -> Option<crate::collision_scene::StaticRaySource> {
        self.combat_rays.ray_source(hit)
    }

    /// Sweep the authored terrain hull through the prepared combat geometry.
    /// A zero-radius call is also used for point visibility from a blast.
    pub(crate) fn projectile_wall(
        &self,
        origin: Vec3,
        direction: Vec3,
        reach: f64,
        radius: f32,
    ) -> Result<Option<SurfaceHit>, IndexError> {
        if reach <= 0.0 {
            return Ok(None);
        }
        if let Some(world) = self.combat_sweeps.filter(|_| radius > 0.0) {
            let start = origin.map(|axis| axis as f32);
            let end = along(origin, direction, reach).map(|axis| axis as f32);
            if !start.into_iter().chain(end).all(f32::is_finite) {
                return Err(IndexError::InvalidSweep);
            }
            return world
                .projectile_sweep(MeshProbe {
                    points: &[[0.0; 3]],
                    radius,
                    start,
                    end,
                    category: MOVER_CATEGORY,
                })
                .map(|hit| {
                    hit.map(|result| SurfaceHit {
                        distance: reach * f64::from(result.hit.fraction),
                        normal: result.hit.normal.map(f64::from),
                    })
                })
                .map_err(|_| IndexError::InvalidSweep);
        }
        self.combat_wall(origin, direction, reach).map(|hit| {
            hit.map(|hit| SurfaceHit {
                distance: hit.distance,
                normal: hit.normal,
            })
        })
    }

    pub(crate) fn summoner_beam_contacts(
        &self,
        origin: [f64; 3],
        direction: [f64; 3],
        reach: f64,
        targets: &[crate::script_enemy::EnemyTarget],
    ) -> [script_init::BeamContact; 2] {
        use crate::static_ray::capsule_entry_distance;
        let Ok(wall) = self.hitscan_wall(origin, direction, reach) else {
            return Default::default();
        };
        [0.0, 0.2].map(|radius| {
            let entity = targets
                .iter()
                .filter(|target| target.team == 0 && target.alive)
                .filter_map(|target| {
                    let [bottom, top] = target.pose.segment();
                    let contact = capsule_entry_distance(
                        origin,
                        direction,
                        reach,
                        bottom,
                        top,
                        target.pose.radius + radius,
                    )?;
                    wall.is_none_or(|wall| contact < wall)
                        .then_some((contact, target.entity))
                })
                .min_by(|(a, first), (b, second)| a.total_cmp(b).then_with(|| first.cmp(second)))
                .map(|(_, entity)| entity);
            script_init::BeamContact {
                entity,
                hit: entity.is_some() || wall.is_some(),
            }
        })
    }

    pub(crate) fn visible_aim(
        &self,
        origin: [f64; 3],
        target: crate::game_hitscan::TargetPose,
    ) -> Option<[f64; 3]> {
        use crate::static_ray::{capsule_entry_distance, norm};
        let [bottom, top] = target.segment();
        let [x, y, z] = target.feet;
        let aim = |point: [f64; 3]| {
            let delta = std::array::from_fn(|axis| point[axis] - origin[axis]);
            let distance = norm(delta);
            if distance <= 1e-9 {
                return None;
            }
            let direction = delta.map(|axis| axis / distance);
            let contact =
                capsule_entry_distance(origin, direction, distance, bottom, top, target.radius)?;
            self.hitscan_wall(origin, direction, contact)
                .is_ok_and(|wall| wall.is_none_or(|wall| wall >= contact))
                .then_some(direction)
        };
        let eye = [x, y + target.eye.min(target.height - 0.01), z];
        [eye, top, bottom].into_iter().find_map(aim).or_else(|| {
            let dx = x - origin[0];
            let dz = z - origin[2];
            let horizontal = dx.hypot(dz);
            if horizontal <= 1e-9 {
                return None;
            }
            let width = target.radius * 0.75 / horizontal;
            let middle = (bottom[1] + top[1]) * 0.5;
            aim([x + dz * width, middle, z - dx * width])
                .or_else(|| aim([x - dz * width, middle, z + dx * width]))
        })
    }
}
