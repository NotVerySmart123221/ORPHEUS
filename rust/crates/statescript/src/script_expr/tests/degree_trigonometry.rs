use super::*;

#[test]
fn degree_trigonometry_rounds_results_and_composes_with_vector_expressions() {
    let contract: Json = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-degree-trigonometry-v1.json"
    )))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "script-degree-trigonometry-v1");
    let cases = contract["input"]["scalars"].as_array().unwrap();
    let results = contract["expected"]["scalars"].as_array().unwrap();
    assert_eq!(cases.len(), results.len());
    for (case, expected) in cases.iter().zip(results) {
        let opcode = &case["opcode"];
        let config = if let Some(angle) = case["angle"].as_f64() {
            expression(json!([6, 0, opcode, 0]), &[], &[angle], &[])
        } else {
            assert!(case["angle"].is_null());
            expression(json!([14, 0, opcode, 0]), &[], &[], &[Json::Null])
        };
        assert_eq!(
            evaluate(&config, &mut EmptyContext),
            Ok(Value::Float(f64::from(expected.as_f64().unwrap() as f32))),
            "{case}"
        );
    }

    let vector = &contract["input"]["vector"];
    let floats = [
        vector["angle"].as_f64().unwrap(),
        vector["vertical"].as_f64().unwrap(),
    ];
    let config = expression(json!([6, 0, 46, 6, 1, 6, 0, 47, 58, 0]), &[], &floats, &[]);
    let expected: [f32; 3] =
        serde_json::from_value(contract["expected"]["vector"].clone()).unwrap();
    assert_eq!(
        evaluate(&config, &mut EmptyContext),
        Ok(Value::Vector(expected.map(f64::from)))
    );
}

#[test]
fn degree_trigonometry_reports_stack_underflow_and_infinite_operands() {
    assert_eq!(
        evaluate(
            &expression(json!([46, 0]), &[], &[], &[]),
            &mut EmptyContext
        ),
        Err(EvalError::MalformedBytecode {
            offset: 0,
            reason: "expression stack underflow",
        })
    );
    let config = expression(
        json!([6, 0, 6, 1, 28, 46, 0]),
        &[],
        &[f64::from(f32::MAX), 2.0],
        &[],
    );
    assert_eq!(
        evaluate(&config, &mut EmptyContext),
        Err(EvalError::MalformedBytecode {
            offset: 5,
            reason: "infinite degree operand",
        })
    );
}
