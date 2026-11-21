//! Evaluate radial combat impulses independently of health loss.

use serde_json::Value as Json;

use crate::{
    game_projectiles::authored_number, script_expr::EvalError, script_wire::Value, static_ray::Vec3,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Knockback {
    horizontal: f64,
    upward: f64,
    rings: Vec<(f64, [f64; 2])>,
}

impl Knockback {
    pub(crate) fn from_authored(
        modify_health: &Json,
        evaluate: &mut impl FnMut(&Json) -> Result<Value, EvalError>,
    ) -> Option<Self> {
        let horizontal = authored_number(modify_health.get("m_E33EFEE1"), 0.0, evaluate).ok()?;
        let upward = authored_number(modify_health.get("m_10399AE7"), 0.0, evaluate).ok()?;
        if [horizontal, upward] == [0.0; 2] || ![horizontal, upward].into_iter().all(f64::is_finite)
        {
            return None;
        }
        let mut rings = Vec::new();
        let rows = modify_health
            .get("m_knockbackRings")
            .filter(|rows| !rows.is_null());
        if let Some(rows) = rows {
            for row in rows.as_array()? {
                if row["$"].as_str() != Some("STUStatescriptModifyHealthKnockbackRing") {
                    return None;
                }
                let radius = authored_number(row.get("m_radius"), 0.0, evaluate).ok()?;
                let horizontal = authored_number(row.get("m_579D7CFF"), 1.0, evaluate).ok()?;
                let upward = authored_number(row.get("m_10399AE7"), 1.0, evaluate).ok()?;
                if !radius.is_finite()
                    || radius < 0.0
                    || !horizontal.is_finite()
                    || !upward.is_finite()
                {
                    return None;
                }
                rings.push((radius, [horizontal, upward]));
            }
        }
        rings.sort_by(|a, b| a.0.total_cmp(&b.0));
        Some(Self {
            horizontal,
            upward,
            rings,
        })
    }

    pub(crate) fn delta(
        &self,
        point: Vec3,
        feet: Vec3,
        distance: f64,
        in_damage_range: bool,
    ) -> Option<Vec3> {
        let scale = self.scale_at(distance, in_damage_range)?;
        let horizontal = self.horizontal * scale[0];
        let upward = self.upward * scale[1];
        let x = feet[0] - point[0];
        let z = feet[2] - point[2];
        let planar = x.hypot(z);
        let delta = if planar > 0.0 {
            [x / planar * horizontal, upward, z / planar * horizontal]
        } else {
            [0.0, upward, 0.0]
        };
        (delta != [0.0; 3] && delta.iter().all(|axis| (*axis as f32).is_finite())).then_some(delta)
    }

    fn scale_at(&self, distance: f64, in_damage_range: bool) -> Option<[f64; 2]> {
        let Some(&(mut previous, mut scale)) = self.rings.first() else {
            return in_damage_range.then_some([1.0; 2]);
        };
        if distance <= previous {
            return Some(scale);
        }
        for &(radius, next) in &self.rings[1..] {
            if distance <= radius {
                let share = (distance - previous) / (radius - previous);
                return Some(std::array::from_fn(|axis| {
                    scale[axis] + (next[axis] - scale[axis]) * share
                }));
            }
            previous = radius;
            scale = next;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_knockback_sorts_and_interpolates_range_independently_of_damage() {
        let fixture: Json = serde_json::from_str(include_str!(
            "../../../fixtures/behavior/combat-knockback-v1.json"
        ))
        .unwrap();
        let input = &fixture["input"];
        let rings: Vec<_> = input["rings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|ring| {
                serde_json::json!({
                    "$": "STUStatescriptModifyHealthKnockbackRing",
                    "m_radius": ring["radius"],
                    "m_579D7CFF": ring["horizontal"],
                    "m_10399AE7": ring["upward"]
                })
            })
            .collect();
        let data = serde_json::json!({
            "m_E33EFEE1": input["base_horizontal"],
            "m_10399AE7": input["base_upward"],
            "m_knockbackRings": rings,
        });
        let profile = Knockback::from_authored(&data, &mut |value| {
            Ok(value.as_f64().map_or(Value::Null, Value::Float))
        })
        .unwrap();
        for (name, distance) in [("near", 0.25), ("middle", 1.5)] {
            let expected: Vec3 = serde_json::from_value(fixture["expected"][name].clone()).unwrap();
            assert_eq!(
                profile.delta([0.0; 3], [0.0, 0.0, -2.0], distance, false),
                Some(expected)
            );
        }
        assert_eq!(profile.delta([0.0; 3], [0.0, 0.0, -3.0], 2.6, true), None);
        assert_eq!(
            profile.delta([0.0; 3], [0.0; 3], 0.0, false),
            Some([0.0, 7.0, 0.0])
        );
    }
}
