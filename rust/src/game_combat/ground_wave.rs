//! Ground continuity and visibility for travelling ground-wave placements.

use super::{RoomSpatialFrame, Vec3, script_requests::MAX_BODY_SCRIPT_REQUESTS};
use crate::{collision_scene::CollisionScene, script_init::GroundWave};

// Ground support uses the Python 104319 probe profile 0016. Static sight uses 002D.
const GROUND_CATEGORY: u16 = 0x1f9f;
const SIGHT_CATEGORY: u16 = 0x1fdf;

impl RoomSpatialFrame {
    pub(crate) fn ground_wave_targets(
        &self,
        source: u32,
        wave: &GroundWave,
        reach: f64,
    ) -> Option<Vec<u32>> {
        let owner = self.actor_sample(source)?.0;
        let capsules = self.actor_capsules.as_ref()?;
        let (x_forward, z_forward) = wave.yaw.to_radians().sin_cos();
        let min_cos = (wave.aperture.to_radians() * 0.5).cos();
        let mut found = Vec::new();
        for candidate in &self.candidates {
            if candidate.entity == source
                || owner.team.is_some_and(|team| candidate.team == Some(team))
                || !self.actor_sample(candidate.entity)?.1.health
            {
                continue;
            }
            let pose = capsules.get(&candidate.entity)?;
            let x = pose.feet[0] - wave.centre[0];
            let z = pose.feet[2] - wave.centre[2];
            let distance = x.hypot(z);
            if distance > reach || x * x_forward + z * z_forward < distance * min_cos {
                continue;
            }
            // Same lower/upper fallback anchors as the reference for bodies without a posed model.
            let mut visible = false;
            for fraction in [0.3, 0.6] {
                let mut point = pose.feet;
                point[1] += pose.height * fraction;
                if self.ground.segment_clear_with_breakables(
                    wave.sight_origin,
                    point,
                    SIGHT_CATEGORY,
                    self.breakables.as_deref(),
                )? {
                    visible = true;
                    break;
                }
            }
            if visible && connected_ground(&self.ground, wave, pose.feet)? {
                if found.len() >= MAX_BODY_SCRIPT_REQUESTS {
                    return None;
                }
                found.push(candidate.entity);
            }
        }
        Some(found)
    }
}

#[cfg(test)]
mod tests;

/// Port of collision.World.shockwave_ground. A missing support breaks the wave.
fn connected_ground(scene: &CollisionScene, wave: &GroundWave, feet: Vec3) -> Option<bool> {
    let index = scene.triangles();
    let [x, y, z] = feet;
    let start_y = y + 1.0;
    let Some(mut previous) =
        index.ground(x, z, start_y, y - wave.ground_range - 1.0, GROUND_CATEGORY)
    else {
        return Some(false);
    };
    let [dx, dy, dz] = [
        wave.sight_origin[0] - x,
        wave.sight_origin[1] - start_y,
        wave.sight_origin[2] - z,
    ];
    let length = dx.hypot(dy).hypot(dz);
    // Bound work if a malformed graph places its sight origin outside the map.
    if !length.is_finite() || length > 1024.0 {
        return None;
    }
    let steps = length.ceil() as u32;
    let mut accumulated: f64 = 0.0;
    for step in 1..=steps {
        let fraction = f64::from(step) / f64::from(steps);
        let top = start_y + fraction * dy;
        let Some(support) = index.ground(
            x + dx * fraction,
            z + dz * fraction,
            top,
            top - wave.ground_range - 2.0,
            GROUND_CATEGORY,
        ) else {
            return Some(false);
        };
        let delta = support.y - previous.y;
        let allowance = if step == 1 {
            dx.hypot(dz) / f64::from(steps)
        } else {
            1.0
        };
        if delta.abs() < allowance {
            accumulated = (accumulated.abs() - allowance)
                .max(0.0)
                .copysign(accumulated);
        }
        accumulated += delta;
        if accumulated < -wave.max_drop || accumulated > wave.max_rise {
            return Some(false);
        }
        previous = support;
    }
    Some(true)
}
