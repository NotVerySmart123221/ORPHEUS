use super::*;

#[test]
fn direction_limits_match_the_python_reference_evaluator() {
    #[derive(serde::Deserialize)]
    struct Contract {
        schema_version: u8,
        input: Vec<Case>,
    }

    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        direction: [f64; 3],
        reference: [f64; 3],
        limits: [f64; 2],
        expected: Option<[f64; 3]>,
    }

    let fixture: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/statescript-direction-limits-v1.json"
    )))
    .unwrap();
    assert_eq!(fixture.schema_version, 1);
    let vector =
        |[x, y, z]: [f64; 3]| json!({"$": "STUConfigVarVec3", "m_value": {"X": x, "Y": y, "Z": z}});
    let degrees = |value: f64| json!({"$": "STUConfigVarFloat", "m_value": value});

    for case in fixture.input {
        let config = json!({
            "$": "STU_0762B33A",
            "m_32B00625": vector(case.direction),
            "m_3661BC45": vector(case.reference),
            "m_5023B9B0": degrees(case.limits[0]),
            "m_E26E5E09": degrees(case.limits[1]),
            "m_EE729DCB": 0,
        });
        let actual = evaluate(&config, &mut Vars::default()).unwrap();
        if let Some(expected) = case.expected {
            let Value::Vector(actual) = actual else {
                panic!("{}: expected a direction, got {actual:?}", case.name);
            };
            for (actual, expected) in actual.into_iter().zip(expected) {
                assert!(
                    (actual - expected).abs() < 1e-6,
                    "{}: {actual} != {expected}",
                    case.name
                );
            }
        } else {
            assert_eq!(actual, Value::Null, "{}", case.name);
        }
    }
}
