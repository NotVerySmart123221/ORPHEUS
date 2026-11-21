use super::*;
use crate::static_ray::Vec3;

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
