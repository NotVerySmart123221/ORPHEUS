//! Visible body selection from one frozen room frame.

use serde_json::Value as Json;

use super::RoomSpatialFrame;
use crate::{
    contact_triangle::closest_segments,
    game_hitscan::{TargetPose, direction_for_angles},
    script_init::{ViewSightProfile, ViewTargetQuery},
    static_ray::{Vec3, along, capsule_entry_hit, norm, segment_distance},
};

struct Camera {
    eye: Vec3,
    origin: Vec3,
    forward: Vec3,
    right: Vec3,
    up: Vec3,
}

impl Camera {
    fn new(feet: Vec3, eye: Vec3, [yaw, pitch]: [i32; 2], offset: Vec3) -> Self {
        let forward = direction_for_angles(yaw, pitch);
        let right = direction_for_angles(yaw.wrapping_add(16_384), 0);
        let up = direction_for_angles(yaw, pitch.wrapping_sub(16_384));
        let origin = along(
            along(
                along(eye, right, offset[0]),
                up,
                offset[1] - (eye[1] - feet[1]),
            ),
            forward,
            offset[2],
        );
        Self {
            eye,
            origin,
            forward,
            right,
            up,
        }
    }

    fn screen_distance(&self, pose: TargetPose) -> f64 {
        let lower = delta(along(pose.feet, self.right, -pose.radius), self.eye);
        let upper = delta(
            along(
                along(pose.feet, self.right, pose.radius),
                [0.0, 1.0, 0.0],
                pose.height,
            ),
            self.eye,
        );
        let lower_depth = dot(lower, self.forward);
        let upper_depth = dot(upper, self.forward);
        if lower_depth <= 0.0 || upper_depth <= 0.0 {
            return 1.0;
        }
        let error = |axis| {
            let left = dot(lower, axis) / lower_depth;
            let right = dot(upper, axis) / upper_depth;
            left.min(right).max(-left.max(right)).max(0.0)
        };
        (error(self.right).hypot(error(self.up)) / 2.0_f64.sqrt()).min(1.0)
    }

    fn contains(&self, pose: TargetPose, range: f64, cosine: f64) -> bool {
        let [foot, head] = pose.segment();
        if segment_distance(self.origin, foot, head) > range + pose.radius {
            return false;
        }
        let rear = along(self.origin, self.forward, -range);
        let front = along(self.origin, self.forward, range);
        let contact_point = |point: Vec3| point.map(|coordinate| coordinate as f32);
        let (point, axis) = closest_segments(
            contact_point(foot),
            contact_point(head),
            contact_point(rear),
            contact_point(front),
        );
        let point = point.map(f64::from);
        let axis = axis.map(f64::from);
        let gap = norm(delta(axis, point));
        let surface = if gap > pose.radius {
            along(point, delta(axis, point), pose.radius / gap)
        } else {
            point
        };
        let direction = delta(surface, self.origin);
        dot(direction, self.forward) >= norm(direction) * cosine
    }
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a.into_iter().zip(b).map(|(left, right)| left * right).sum()
}

fn delta(a: Vec3, b: Vec3) -> Vec3 {
    std::array::from_fn(|axis| a[axis] - b[axis])
}

enum Metric {
    Screen,
    Distance,
}

struct Score {
    metric: Metric,
    minimum: f64,
    span: f64,
    weight: f64,
}

impl Score {
    fn parse(row: &Json) -> Option<Self> {
        if row["$"] != "STU_2399BBA2"
            || !row
                .as_object()?
                .keys()
                .all(|field| matches!(field.as_str(), "$" | "m_84E5DE4E" | "m_weight"))
        {
            return None;
        }
        let config = &row["m_84E5DE4E"];
        if !config
            .as_object()?
            .keys()
            .all(|field| matches!(field.as_str(), "$" | "m_ABC68B33" | "m_2AFC083F"))
        {
            return None;
        }
        let metric = match config["$"].as_str()? {
            "STU_2FD4C482" => Metric::Screen,
            "STU_AD7331B0" => Metric::Distance,
            _ => return None,
        };
        let number = |config: &Json| {
            if !config
                .as_object()?
                .keys()
                .all(|field| matches!(field.as_str(), "$" | "m_value" | "m_EE729DCB"))
                || !(config["m_EE729DCB"].is_null() || config["m_EE729DCB"] == 0)
            {
                return None;
            }
            match config["$"].as_str()? {
                "STUConfigVarInt" => config["m_value"].as_i64().map(|value| value as f64),
                "STUConfigVarFloat" => config["m_value"].as_f64().filter(|value| value.is_finite()),
                _ => None,
            }
        };
        let minimum = number(&config["m_ABC68B33"])?;
        let span = number(&config["m_2AFC083F"])? - minimum;
        let weight = number(&row["m_weight"])?;
        (span.is_finite() && span > 0.0 && weight >= 0.0).then_some(Self {
            metric,
            minimum,
            span,
            weight,
        })
    }

    fn evaluate(&self, camera: &Camera, pose: TargetPose) -> f64 {
        let value = match self.metric {
            Metric::Screen => camera.screen_distance(pose),
            Metric::Distance => norm(delta(pose.feet, camera.origin)),
        };
        ((value - self.minimum) / self.span).clamp(0.0, 1.0) * self.weight
    }
}

impl RoomSpatialFrame {
    pub(crate) fn view_targets(&self, query: &ViewTargetQuery<'_>) -> Option<Vec<u32>> {
        if let Some(empty) = self.empty_view_targets(query) {
            return Some(empty);
        }
        let profile = query.sight?;
        let (feet, angles) = self.actor_pose(query.source)?;
        let eye = self.actor_eye_position(query.source)?;
        let camera = Camera::new(feet, eye, angles, query.offset);
        if !camera.origin.into_iter().all(f64::is_finite) {
            return None;
        }
        let rows = query.ranking["m_503F0304"].as_array()?;
        if rows.len() > 8 {
            return None;
        }
        let scoring: Vec<_> = rows.iter().map(Score::parse).collect::<Option<_>>()?;
        let [any, all, exclude] = query.masks;
        let cosine = (query.aperture.to_radians() * 0.5).cos();
        let mut ranked = Vec::new();
        // The view tracker body filter selects participating player bodies.
        // Bots may still occlude a sight sample when the supplied profile admits them.
        for &entity in self.player_wires.values() {
            if entity == query.source {
                continue;
            }
            let Some((_, target)) = self.actor_sample(entity) else {
                continue;
            };
            let flags = target.filter_flags?;
            if (any != 0 && flags & any == 0) || flags & all != all || flags & exclude != 0 {
                continue;
            }
            let pose = self.view_capsule(entity)?;
            if !camera.contains(pose, query.range, cosine)
                || !self.view_visible(camera.origin, query.source, entity, pose, profile)?
            {
                continue;
            }
            let score: f64 = scoring
                .iter()
                .map(|term| term.evaluate(&camera, pose))
                .sum();
            if !score.is_finite() {
                return None;
            }
            ranked.push((entity, score));
        }
        ranked.sort_by(|(a, distance_a), (b, distance_b)| {
            distance_a.total_cmp(distance_b).then(a.cmp(b))
        });
        Some(
            ranked
                .into_iter()
                .take(query.limit)
                .map(|(entity, _)| entity)
                .collect(),
        )
    }

    fn view_capsule(&self, entity: u32) -> Option<TargetPose> {
        let pose = *self.actor_capsules.as_ref()?.get(&entity)?;
        (pose.feet.into_iter().all(f64::is_finite)
            && pose.height.is_finite()
            && pose.height >= 0.0
            && pose.radius.is_finite()
            && pose.radius > 0.0)
            .then_some(pose)
    }

    fn view_visible(
        &self,
        origin: Vec3,
        source: u32,
        candidate: u32,
        pose: TargetPose,
        profile: &ViewSightProfile,
    ) -> Option<bool> {
        for &height in &profile.samples {
            let point = along(pose.feet, [0.0, 1.0, 0.0], pose.height * height);
            if !self
                .ground
                .segment_clear(origin, point, profile.static_mask)?
            {
                continue;
            }
            let ray = delta(point, origin);
            let distance = norm(ray);
            if distance == 0.0 || profile.block_actor_flags == 0 {
                return Some(true);
            }
            let mut blocked = false;
            let direction = ray.map(|axis| axis / distance);
            for actor in &self.candidates {
                if actor.entity == source || actor.entity == candidate {
                    continue;
                }
                let flags = self.actor_sample(actor.entity)?.1.filter_flags?;
                if flags & profile.block_actor_flags == 0 {
                    continue;
                }
                let occluder = self.view_capsule(actor.entity)?;
                let [bottom, top] = occluder.segment();
                if capsule_entry_hit(origin, direction, distance, bottom, top, occluder.radius)
                    .is_some()
                {
                    blocked = true;
                    break;
                }
            }
            if !blocked {
                return Some(true);
            }
        }
        Some(false)
    }
}

#[cfg(test)]
mod tests;
