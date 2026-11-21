//! Evaluate radial combat impulses independently of health loss.

use serde_json::Value as Json;

use crate::{
    game_projectiles::authored_number, script_expr::EvalError, script_wire::Value, static_ray::Vec3,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Knockback {
    horizontal: f32,
    upward: f32,
    rings: Vec<(f32, [f32; 2])>,
}

impl Knockback {
    pub(crate) fn from_authored(
        modify_health: &Json,
        evaluate: &mut impl FnMut(&Json) -> Result<Value, EvalError>,
    ) -> Option<Self> {
        let horizontal =
            authored_number(modify_health.get("m_E33EFEE1"), 0.0, evaluate).ok()? as f32;
        let upward = authored_number(modify_health.get("m_10399AE7"), 0.0, evaluate).ok()? as f32;
        if [horizontal, upward] == [0.0; 2] || ![horizontal, upward].into_iter().all(f32::is_finite)
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
                let radius = authored_number(row.get("m_radius"), 0.0, evaluate).ok()? as f32;
                let horizontal = authored_number(row.get("m_579D7CFF"), 0.0, evaluate).ok()? as f32;
                let upward = authored_number(row.get("m_10399AE7"), 0.0, evaluate).ok()? as f32;
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
        target_scale: f32,
    ) -> Option<Vec3> {
        if target_scale <= 0.0 || !distance.is_finite() {
            return None;
        }
        let scale = self.scale_at(distance, in_damage_range)?;
        let horizontal = f64::from(self.horizontal * scale[0] * target_scale);
        let upward = f64::from(self.upward * scale[1] * target_scale);
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

    fn scale_at(&self, distance: f64, in_damage_range: bool) -> Option<[f32; 2]> {
        let distance = distance.max(0.0) as f32;
        let Some(&(mut previous, mut scale)) = self.rings.first() else {
            return in_damage_range.then_some([1.0; 2]);
        };
        if distance < previous {
            return Some(scale);
        }
        for &(radius, next) in &self.rings[1..] {
            if distance < radius {
                let share = (distance - previous) / (radius - previous).max(0.0001);
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
                profile.delta([0.0; 3], [0.0, 0.0, -2.0], distance, false, 1.0),
                Some(expected)
            );
        }
        assert_eq!(
            profile.delta([0.0; 3], [0.0, 0.0, -3.0], 2.5, true, 1.0),
            None
        );
        assert_eq!(
            profile.delta([0.0; 3], [0.0; 3], 0.0, false, 1.0),
            Some([0.0, 7.0, 0.0])
        );
    }

    #[test]
    #[ignore = "requires prepared 104319 graphs with default mode and actor rules"]
    fn prepared_helix_and_particle_blasts_resolve_knockback_from_mode_rules() {
        use crate::{
            body_ruleset::{self, ActorKind},
            game_content::PRACTICE_RANGE_MODE,
            script_expr::{self, Context, Variable},
            script_graph::GraphStore,
        };

        struct ShotRules<'a> {
            graphs: &'a GraphStore,
            hero: u64,
            radius: Value,
        }

        impl Context for ShotRules<'_> {
            fn ruleset(&mut self, id: u32) -> Value {
                body_ruleset::get(
                    self.graphs,
                    PRACTICE_RANGE_MODE,
                    self.hero,
                    ActorKind::Player,
                    id,
                )
                .unwrap_or(Value::Integer(0))
            }

            fn variable(&mut self, variable: Variable) -> Value {
                match variable.id {
                    11861 => self.radius.clone(),
                    _ => Value::Null,
                }
            }
        }

        let runtime = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
        let graphs = GraphStore::load(runtime.join("data/game_graphs_174.json.gz")).unwrap();
        for (index, hero, radius, expected) in [
            (0x0254, 0x02e0_0000_0000_006e, Value::Null, [4.0, 6.0, 0.0]),
            (
                0x0241,
                0x02e0_0000_0000_0068,
                Value::Float(2.5),
                [5.0, 8.0, 0.0],
            ),
        ] {
            let mut rules = ShotRules {
                graphs: &graphs,
                hero,
                radius,
            };
            let config = graphs
                .graph(index)
                .unwrap()
                .nodes
                .iter()
                .find_map(|node| node.field("m_D2D11CE9.m_modifyHealth"))
                .unwrap();
            let knockback = Knockback::from_authored(config, &mut |value| {
                script_expr::evaluate(value, &mut rules)
            })
            .unwrap_or_else(|| panic!("graph {index:04X} requires its authored mode rules"));
            assert_eq!(
                knockback.delta([0.0; 3], [1.0, 0.0, 0.0], 0.25, true, 1.0),
                Some(expected)
            );
        }
    }
}
