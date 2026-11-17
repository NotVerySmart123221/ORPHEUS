use super::{evaluator::Evaluator, *};
use std::{collections::BTreeMap, path::PathBuf};

use serde_json::json;

use crate::script_graph::GraphStore;

#[derive(Default)]
struct Vars {
    instance: BTreeMap<u32, Value>,
    entity: BTreeMap<u32, Value>,
    reads: Vec<Variable>,
}

impl Context for Vars {
    fn variable(&mut self, variable: Variable) -> Value {
        self.reads.push(variable);
        let values = match variable.scope {
            Scope::Instance => &self.instance,
            Scope::Entity => &self.entity,
        };
        values.get(&variable.id).cloned().unwrap_or(Value::Null)
    }

    fn parent_variable(&mut self, levels: u8, variable: Variable) -> Value {
        match (levels, variable.scope, variable.id) {
            (1, Scope::Instance, 14152) => Value::Asset(0x02e0_0000_0000_0002),
            _ => Value::Null,
        }
    }

    fn entity_member(&mut self, entity: u32, id: u32) -> Option<Value> {
        (entity == 0xa000_0100).then_some(match id {
            7423 => Value::Integer(23),
            _ => Value::Null,
        })
    }

    fn ruleset(&mut self, id: u32) -> Value {
        match id {
            10265 => Value::Float(1.0),
            _ => Value::Null,
        }
    }

    fn native(&mut self, config: &Json) -> Option<Value> {
        match class(config)? {
            "STU_37F20D24" => Some(Value::Asset(0x0230_0000_0000_0018)),
            _ => None,
        }
    }
}

#[test]
fn inline_played_graph_uses_native_map_order_and_duplicate_low_keys() {
    let inline = |rows| {
        json!({"$":"STU_9DB46B98","m_6525E98B":{
            "$":"STUStatescriptGraphWithOverrides","m_graph":"0x058000000000224E","m_1EB5A024":rows}})
    };
    let row = |key, value| json!({"$":"STUStatescriptSchemaEntry","m_0D09D2D9":format!("0x0D800000{key:08X}"),"m_value":value});
    let config = inline(vec![
        row(1, Json::from(10)),
        row(9, Json::Null),
        row(1, Json::from(20)),
        row(0, Json::from(99)),
    ]);
    assert_eq!(
        evaluate(&config, &mut Vars::default()).unwrap(),
        Value::Map(vec![
            (Value::Integer(1), Value::Integer(20)),
            (Value::Integer(9), Value::Integer(0)),
            (Value::Integer(0x4065), Value::Asset(0x0580_0000_0000_224e))
        ])
    );
    assert!(
        evaluate(
            &inline(vec![row(0x8000_0000u32, Json::from(1))]),
            &mut Vars::default()
        )
        .is_err()
    );
    let mut empty = config;
    empty["m_6525E98B"]["m_graph"] = Json::Null;
    empty["m_6525E98B"]["m_1EB5A024"] = json!([{"m_value":{"$":"ShouldNotEvaluate"}}]);
    assert_eq!(
        evaluate(&empty, &mut Vars::default()).unwrap(),
        Value::Integer(0)
    );
}

#[test]
fn authored_map_growth_reinserts_rows_in_bucket_order() {
    let mut rows: Vec<_> = [15, 31, 1, 2, 3, 4, 5, 6, 8]
        .into_iter()
        .map(|key| {
            json!({
                "$": "STU_D8196947",
                "m_identifier": format!("0x0D8000000000{key:04X}"),
                "m_value": {"$": "STUConfigVarInt", "m_value": key}
            })
        })
        .collect();
    rows.push(json!({
        "$": "STU_D8196947",
        "m_identifier": "0x0D8000000000001F",
        "m_value": null
    }));
    let config = json!({"$": "STU_7042F028", "m_92829780": rows});
    let expected = [15, 1, 2, 3, 4, 5, 6, 8, 31]
        .into_iter()
        .map(|key| {
            (
                Value::Integer(key),
                Value::Integer(if key == 31 { 0 } else { key }),
            )
        })
        .collect();
    assert_eq!(
        evaluate(&config, &mut Vars::default()),
        Ok(Value::Map(expected))
    );
    assert!(matches!(
        evaluate_with_budget(&config, &mut Vars::default(), 1),
        Err(EvalError::BudgetExceeded { .. })
    ));
}

#[test]
fn static_queries_preserve_a_world_native_result_before_evaluating_the_operand() {
    struct Native;
    impl Context for Native {
        fn native(&mut self, _config: &Json) -> Option<Value> {
            Some(Value::Null)
        }
    }
    for config in [
        json!({"$":"STU_71A87E92","m_entity":{"$":"unavailable"}}),
        json!({"$":"STU_021D01CD","m_entity":{"$":"unavailable"}}),
        json!({"$":"STU_C8C4C20D","m_entity":{"$":"unavailable"}}),
        json!({"$":"STU_93DCDDFD","m_entity":{"$":"unavailable"}}),
        json!({"$":"STU_B3E67A36","m_filter":null,"m_8B2A4DF3":null,"m_9C7BD8B8":null,"m_B486603E":{"$":"unavailable"}}),
    ] {
        assert_eq!(evaluate(&config, &mut Native), Ok(Value::Null));
    }
}

#[test]
fn entity_team_returns_no_team_for_an_unset_target() {
    let query = json!({"$":"STU_65ABFCCC","m_target":{
        "$":"STUConfigVarDynamic","m_60DB8F99":0,"m_identifier":27
    }});
    let mut world = Vars::default();
    for value in [Value::Null, Value::Entity(0), Value::Array(Vec::new())] {
        world.instance.insert(27, value);
        assert_eq!(evaluate(&query, &mut world), Ok(Value::Integer(-1)));
    }
    world.instance.insert(27, Value::Entity(0xa000_0101));
    assert_eq!(
        evaluate(&query, &mut world),
        Err(EvalError::UnsupportedConfig("STU_65ABFCCC".into()))
    );
}

#[test]
fn transform_heading_matches_reference_axes_and_rejects_unknown_transforms() {
    struct Transform(Option<[f64; 3]>);
    impl Context for Transform {
        fn owner(&mut self) -> Value {
            Value::Entity(17)
        }
        fn forward(&mut self, entity: u32) -> Option<[f64; 3]> {
            assert_eq!(entity, 17);
            self.0
        }
    }
    let query = json!({"$":"STU_C8C4C20D","m_entity":{"$":"STU_B5A0CAF0"}});
    for (forward, degrees) in [
        ([1.0, 0.0, 0.0], 90.0),
        ([-1.0, 0.0, 0.0], -90.0),
        ([0.0, 0.0, 1.0], 0.0),
        ([0.0, 0.0, -1.0], 0.0),
    ] {
        assert_eq!(
            evaluate(&query, &mut Transform(Some(forward))),
            Ok(Value::Float(degrees))
        );
    }
    assert!(evaluate(&query, &mut Transform(None)).is_err());
    assert!(evaluate(&query, &mut Transform(Some([f64::NAN, 0.0, 1.0]))).is_err());
}

fn variable(id: u32, scope: Scope) -> Json {
    json!({
        "$": "STUConfigVarDynamic",
        "m_identifier": id,
        "m_60DB8F99": scope == Scope::Entity,
    })
}

fn expression(opcodes: Json, dynamics: &[Json], floats: &[f64], configs: &[Json]) -> Json {
    json!({
        "$": "STU_919FD47C",
        "m_configVars": configs,
        "m_expression": {
            "m_opcodes": opcodes,
            "m_dynamicVars": dynamics,
            "m_D99EF254": floats,
        },
    })
}

#[test]
fn shared_body_angle_contract() {
    struct Host {
        result: AngleQuery,
        axis: BodyAngle,
        reads: usize,
    }
    impl Context for Host {
        fn variable(&mut self, _: Variable) -> Value {
            Value::Entity(42)
        }
        fn body_angle(&mut self, entity: u32, axis: BodyAngle) -> AngleQuery {
            assert_eq!(entity, 42);
            assert_eq!(axis, self.axis);
            self.reads += 1;
            self.result
        }
    }
    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../../fixtures/behavior/statescript-body-angle-v1.json"
    ))
    .unwrap();
    let tolerance = fixture["tolerance"].as_f64().unwrap();
    for case in fixture["native_cases"].as_array().unwrap() {
        let class = format!("STU_{}", case["stu"].as_str().unwrap());
        let axis = if class == "STU_CC53F52B" {
            BodyAngle::Yaw
        } else {
            BodyAngle::Pitch
        };
        let result = AngleQuery::from_packed(case["units"].as_i64().unwrap() as i32);
        let mut host = Host {
            result,
            axis,
            reads: 0,
        };
        let config = serde_json::json!({"$":class,"m_EE729DCB":0,
                "m_entity":{"$":"STUConfigVarDynamic","m_identifier":1}});
        let Value::Float(actual) = evaluate(&config, &mut host).unwrap() else {
            panic!("not float");
        };
        assert!(
            (actual - case["actual"].as_f64().unwrap()).abs() <= tolerance,
            "{case}: {actual}"
        );
        assert_eq!(host.reads, 1);
        if axis == BodyAngle::Pitch {
            assert_eq!(
                (actual as f32).to_bits(),
                (case["actual"].as_f64().unwrap() as f32).to_bits()
            );
        }
    }
    for (result, expected) in [
        (AngleQuery::Unbound, Some(19.0)),
        (AngleQuery::Unavailable, None),
        (AngleQuery::Present(f64::INFINITY), None),
        (AngleQuery::Present(f64::NAN), None),
    ] {
        let mut host = Host {
            result,
            axis: BodyAngle::Yaw,
            reads: 0,
        };
        let mut config = serde_json::json!({"$":"STU_CC53F52B",
                "m_entity":{"$":"STUConfigVarDynamic","m_identifier":1}});
        let value = evaluate_with_initial(&config, &mut host, Value::Float(19.0));
        if let Some(value_expected) = expected {
            assert_eq!(value.unwrap(), Value::Float(value_expected));
        } else {
            assert!(value.is_err());
        }
        host.reads = 0;
        config["unexpected"] = serde_json::json!(true);
        assert!(evaluate(&config, &mut host).is_err());
        assert_eq!(host.reads, 0);
    }
    assert_eq!(AngleQuery::from_packed(32768), AngleQuery::Unavailable);
    assert_eq!(AngleQuery::from_packed(-32769), AngleQuery::Unavailable);
}

#[test]
fn shared_velocity_contract() {
    fn value(json: &Json) -> Value {
        match json {
            Json::Null => Value::Null,
            Json::Bool(v) => Value::Bool(*v),
            Json::Number(n) => n
                .as_i64()
                .map_or_else(|| Value::Float(n.as_f64().unwrap()), Value::Integer),
            Json::Array(items) => Value::Array(items.iter().map(value).collect()),
            Json::Object(fields) if fields.contains_key("entity") => {
                Value::Entity(json["entity"].as_u64().unwrap().try_into().unwrap())
            }
            Json::Object(fields) if fields.contains_key("vector") => {
                Value::Vector(std::array::from_fn(|axis| {
                    json["vector"][axis].as_f64().unwrap()
                }))
            }
            other => panic!("invalid velocity value {other}"),
        }
    }
    fn number(json: &Json) -> f64 {
        json.as_f64()
            .unwrap_or_else(|| match json["float"].as_str().unwrap() {
                "NaN" => f64::NAN,
                "Infinity" => f64::INFINITY,
                other => panic!("invalid number {other}"),
            })
    }
    struct Host {
        target: Value,
        table: Option<std::collections::BTreeMap<u32, Option<[f64; 3]>>>,
    }
    impl Context for Host {
        fn variable(&mut self, var: Variable) -> Value {
            assert_eq!(var.id, 1);
            self.target.clone()
        }
        fn entity_velocity(&mut self, entity: u32) -> VelocityQuery {
            match &self.table {
                None => VelocityQuery::Unavailable,
                Some(table) => match table.get(&entity) {
                    None => VelocityQuery::Unbound,
                    Some(None) => VelocityQuery::Unavailable,
                    Some(Some(v)) => VelocityQuery::Present(*v),
                },
            }
        }
    }
    let contract: Json = serde_json::from_str(include_str!(
        "../../../../../fixtures/behavior/statescript-velocity-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "statescript.mover-velocity-read.v1");
    assert_eq!(contract["target_build"], 104319);
    let cases = contract["input"]["cases"].as_array().unwrap();
    let expected = contract["expected"]["results"].as_array().unwrap();
    assert_eq!(cases.len(), expected.len());
    for (case, expected) in cases.iter().zip(expected) {
        assert_eq!(case["name"], expected["name"]);
        let table = case["table"].as_array().map(|rows| {
            rows.iter()
                .map(|row| {
                    (
                        row[0].as_u64().unwrap().try_into().unwrap(),
                        row[1]
                            .as_array()
                            .map(|parts| std::array::from_fn(|axis| number(&parts[axis]))),
                    )
                })
                .collect()
        });
        let mut host = Host {
            target: value(&case["target"]),
            table,
        };
        let selector = case.get("selector").cloned().unwrap_or_else(
            || json!({"$":"STUConfigVarDynamic","m_identifier":1,"m_60DB8F99":false}),
        );
        let config = json!({"$":"STU_309B0570","m_EE729DCB":0,"m_entity":selector,"m_5B94EDB8":case.get("mover").unwrap_or(&Json::Null)});
        let actual = evaluate_with_initial(&config, &mut host, value(&case["initial"]));
        if let Some(error) = expected.get("error") {
            assert_eq!(
                actual,
                Err(EvalError::UnsupportedConfig(error.as_str().unwrap().into())),
                "{}",
                case["name"]
            );
        } else {
            assert_eq!(actual, Ok(value(&expected["value"])), "{}", case["name"]);
        }
    }
}

#[test]
fn shared_total_health_contract() {
    fn value(json: &Json) -> Value {
        match json {
            Json::Null => Value::Null,
            Json::Bool(v) => Value::Bool(*v),
            Json::Number(n) => n
                .as_i64()
                .map_or_else(|| Value::Float(n.as_f64().unwrap()), Value::Integer),
            Json::Array(items) => Value::Array(items.iter().map(value).collect()),
            Json::Object(fields) if fields.contains_key("bits64") => Value::Float(f64::from_bits(
                u64::from_str_radix(json["bits64"].as_str().unwrap(), 16).unwrap(),
            )),
            Json::Object(fields) if fields.contains_key("entity") => {
                Value::Entity(json["entity"].as_u64().unwrap().try_into().unwrap())
            }
            Json::Object(fields) if fields.contains_key("vector") => {
                Value::Vector(std::array::from_fn(|axis| {
                    json["vector"][axis].as_f64().unwrap()
                }))
            }
            other => panic!("invalid velocity value {other}"),
        }
    }
    fn number(json: &Json) -> f64 {
        if let Some(hex) = json["bits64"].as_str() {
            return f64::from_bits(u64::from_str_radix(hex, 16).unwrap());
        }
        json.as_f64()
            .unwrap_or_else(|| match json["float"].as_str().unwrap() {
                "NaN" => f64::NAN,
                "Infinity" => f64::INFINITY,
                other => panic!("invalid number {other}"),
            })
    }
    struct Host {
        target: Value,
        table: Option<std::collections::BTreeMap<u32, Option<f64>>>,
    }
    impl Context for Host {
        fn variable(&mut self, var: Variable) -> Value {
            assert_eq!(var.id, 1);
            self.target.clone()
        }
        fn total_health(&mut self, entity: u32) -> HealthQuery {
            match &self.table {
                None => HealthQuery::Unavailable,
                Some(table) => match table.get(&entity) {
                    None => HealthQuery::Unbound,
                    Some(None) => HealthQuery::Unavailable,
                    Some(Some(v)) => HealthQuery::Present(*v),
                },
            }
        }
    }
    let contract: Json = serde_json::from_str(include_str!(
        "../../../../../fixtures/behavior/statescript-health-total-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "statescript.total-health-read.v1");
    assert_eq!(contract["target_build"], 104319);
    let cases = contract["input"]["cases"].as_array().unwrap();
    let expected = contract["expected"]["results"].as_array().unwrap();
    assert_eq!(cases.len(), expected.len());
    for (case, expected) in cases.iter().zip(expected) {
        assert_eq!(case["name"], expected["name"]);
        let table = case["table"].as_array().map(|rows| {
            rows.iter()
                .map(|row| {
                    (
                        row[0].as_u64().unwrap().try_into().unwrap(),
                        (!row[1].is_null()).then(|| number(&row[1])),
                    )
                })
                .collect()
        });
        let mut host = Host {
            target: value(&case["target"]),
            table,
        };
        let selector = case.get("selector").cloned().unwrap_or_else(
            || json!({"$":"STUConfigVarDynamic","m_identifier":1,"m_60DB8F99":false}),
        );
        let config = json!({"$":"STU_9B7648C3","m_EE729DCB":0,"m_entity":selector});
        let actual = evaluate_with_initial(&config, &mut host, value(&case["initial"]));
        if let Some(error) = expected.get("error") {
            assert_eq!(
                actual,
                Err(EvalError::UnsupportedConfig(error.as_str().unwrap().into())),
                "{}",
                case["name"]
            );
        } else {
            assert_eq!(actual, Ok(value(&expected["value"])), "{}", case["name"]);
        }
    }
}

#[test]
fn velocity_selector_is_bounded_and_preserves_explicit_legacy_hosts() {
    struct Legacy;
    impl Context for Legacy {
        fn native(&mut self, _: &Json) -> Option<Value> {
            Some(Value::Vector([9.0, 0.0, 0.0]))
        }
    }
    let mut query = json!({"$":"STU_309B0570","m_entity":{"$":"STU_B5A0CAF0"},"m_5B94EDB8":null});
    assert_eq!(
        evaluate(&query, &mut Legacy),
        Ok(Value::Vector([9.0, 0.0, 0.0]))
    );
    query["m_unknown"] = json!(true);
    assert_eq!(
        evaluate(&query, &mut EmptyContext),
        Err(EvalError::UnsupportedConfig("STU_309B0570".into()))
    );
    query.as_object_mut().unwrap().remove("m_unknown");
    for _ in 0..MAX_CONFIG_DEPTH {
        query["m_entity"] = json!({"$":"STU_06610EBC","m_configVars":[query["m_entity"].clone()]});
    }
    assert_eq!(
        evaluate_with_budget(&query, &mut Legacy, 1),
        Err(EvalError::BudgetExceeded {
            offset: 0,
            limit: 1
        })
    );
    assert_eq!(
        evaluate(&query, &mut Legacy),
        Err(EvalError::NestingExceeded)
    );
}

#[test]
fn shared_engine_setting_contract() {
    fn value(json: &Json) -> Value {
        match json {
            Json::Null => Value::Null,
            Json::Bool(value) => Value::Bool(*value),
            Json::Number(number) => number
                .as_i64()
                .map_or_else(|| Value::Float(number.as_f64().unwrap()), Value::Integer),
            Json::Array(items) => Value::Array(items.iter().map(value).collect()),
            other => panic!("unsupported engine contract value {other}"),
        }
    }
    struct Settings(Option<std::collections::BTreeMap<u32, Value>>);
    impl Context for Settings {
        fn engine_setting(&mut self, key: u32) -> EngineSetting {
            match &self.0 {
                None => EngineSetting::Unavailable,
                Some(table) => table.get(&key).map_or(EngineSetting::Missing, |value| {
                    EngineSetting::Present(value.clone())
                }),
            }
        }
    }
    let contract: Json = serde_json::from_str(include_str!(
        "../../../../../fixtures/behavior/statescript-engine-setting-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "statescript.engine-setting-read.v1");
    assert_eq!(contract["target_build"], 104319);
    let cases = contract["input"]["cases"].as_array().unwrap();
    let expected = contract["expected"]["results"].as_array().unwrap();
    assert_eq!(cases.len(), expected.len());
    for (case, expected) in cases.iter().zip(expected) {
        assert_eq!(case["name"], expected["name"]);
        let mut context = Settings(case["settings"].as_array().map(|rows| {
            rows.iter()
                .map(|row| (row[0].as_u64().unwrap().try_into().unwrap(), value(&row[1])))
                .collect()
        }));
        let config = json!({"$":"STU_A753DC79","m_EE729DCB":0,"m_74A2248F":case["hash"]});
        let actual = evaluate_with_initial(&config, &mut context, value(&case["initial"]));
        if let Some(class) = expected.get("error") {
            assert_eq!(
                actual,
                Err(EvalError::UnsupportedConfig(class.as_str().unwrap().into())),
                "{}",
                case["name"]
            );
        } else {
            assert_eq!(actual, Ok(value(&expected["value"])), "{}", case["name"]);
        }
    }
}

#[test]
fn engine_setting_rejects_unknown_shape_and_keeps_an_explicit_legacy_override() {
    let mut config = json!({"$":"STU_A753DC79","m_EE729DCB":0,"m_74A2248F":1});
    struct Legacy;
    impl Context for Legacy {
        fn native(&mut self, _: &Json) -> Option<Value> {
            Some(Value::Bool(false))
        }
    }
    assert_eq!(
        evaluate_with_initial(&config, &mut Legacy, Value::Integer(73)),
        Ok(Value::Bool(false))
    );
    config["m_unknown"] = json!(1);
    assert_eq!(
        evaluate(&config, &mut EmptyContext),
        Err(EvalError::UnsupportedConfig("STU_A753DC79".into()))
    );
}

#[test]
fn empty_resource_getter_preserves_initial_and_explicit_host_input() {
    let config = json!({"$":"STU_7BD2CA19","m_EE729DCB":0,"m_resourceKey":null});
    assert_eq!(
        evaluate_with_initial(&config, &mut EmptyContext, Value::Integer(-1)),
        Ok(Value::Integer(-1))
    );
    struct Admitted;
    impl Context for Admitted {
        fn native(&mut self, _: &Json) -> Option<Value> {
            Some(Value::Asset(0x0d80_0000_0000_195e))
        }
    }
    assert_eq!(
        evaluate_with_initial(&config, &mut Admitted, Value::Integer(-1)),
        Ok(Value::Asset(0x0d80_0000_0000_195e))
    );
}

#[test]
fn on_fire_without_stats_retains_the_callers_initial_value() {
    struct Stats(Option<OnFire>);
    impl Context for Stats {
        fn owner(&mut self) -> Value {
            Value::Entity(42)
        }
        fn body_on_fire(&mut self, entity: u32) -> Option<OnFire> {
            assert_eq!(entity, 42);
            self.0
        }
    }
    let query = json!({
        "$":"STU_1ED4F039","m_EE729DCB":0,
        "m_entity":{"$":"STU_B5A0CAF0","m_EE729DCB":0},
    });
    let mut missing = Stats(Some(OnFire::NoStats));
    assert_eq!(evaluate(&query, &mut missing), Ok(Value::Null));
    assert_eq!(
        evaluate_with_initial(&query, &mut missing, Value::Integer(0)),
        Ok(Value::Integer(0))
    );
    let default = json!({"$":"STU_91EFD5B1","m_value":query,"m_default":true});
    assert_eq!(evaluate(&default, &mut missing), Ok(Value::Bool(true)));
    for active in [false, true] {
        assert_eq!(
            evaluate_with_initial(
                &query,
                &mut Stats(Some(OnFire::Active(active))),
                Value::Integer(0)
            ),
            Ok(Value::Bool(active))
        );
    }
    assert_eq!(
        evaluate(&query, &mut Stats(None)),
        Err(EvalError::UnsupportedConfig("STU_1ED4F039".into()))
    );
}

#[derive(Default)]
struct Natives {
    reported: Vec<String>,
    controller: u32,
    component92: Option<bool>,
}

impl Context for Natives {
    fn owner(&mut self) -> Value {
        Value::Entity(0xa000_0101)
    }

    fn related_entity(&mut self, relation: EntityRelation, entity: u32) -> Option<u32> {
        (relation == EntityRelation::AiControlEntity && entity == 0xa000_0101)
            .then_some(self.controller)
    }

    fn entity_component92(&mut self, entity: u32) -> Option<bool> {
        (entity == self.controller)
            .then_some(self.component92)
            .flatten()
    }

    fn watches_natives(&self) -> bool {
        true
    }

    fn reference_native(&mut self, class: &str) {
        self.reported.push(class.to_owned());
    }
}

#[test]
fn entity_member_bytecode_distinguishes_unset_unknown_and_invalid_entities() {
    let mut context = Vars::default();
    for opcode in [15, 16] {
        for (target, key, expected) in [
            (Value::Entity(0xa000_0100), 7423, Ok(Value::Integer(23))),
            (Value::Entity(0xa000_0100), 7424, Ok(Value::Null)),
            (
                Value::Entity(0xa000_0101),
                7423,
                Err(EvalError::UnavailableEntityVariable {
                    entity: 0xa000_0101,
                    id: 7423,
                }),
            ),
            (Value::Entity(0), 7423, Ok(Value::Null)),
            (Value::Integer(1), 7423, Ok(Value::Null)),
        ] {
            context.instance.insert(1, target);
            let config = expression(
                json!([8, 0, 7, 1, opcode, 0]),
                &[variable(1, Scope::Instance), variable(key, Scope::Entity)],
                &[],
                &[],
            );
            assert_eq!(evaluate(&config, &mut context), expected);
        }
    }
}

#[test]
fn position_bytecode_rejects_unknown_entities_without_changing_nonentity_coercion() {
    struct Positions([Value; 2]);
    impl Context for Positions {
        fn variable(&mut self, variable: Variable) -> Value {
            self.0[variable.id as usize].clone()
        }

        fn position(&mut self, entity: u32) -> Option<[f64; 3]> {
            match entity {
                0xa000_0100 => Some([0.0; 3]),
                0xa000_0101 => Some([3.0, 4.0, 0.0]),
                _ => None,
            }
        }
    }
    let dynamics = [variable(0, Scope::Instance), variable(1, Scope::Instance)];
    for opcode in [50, 51] {
        let config = expression(json!([8, 0, opcode, 0]), &dynamics, &[], &[]);
        for (value, expected) in [
            (Value::Entity(0xa000_0100), Ok(Value::Float(0.0))),
            (Value::Entity(0xa000_0101), Ok(Value::Float(5.0))),
            (Value::Float(-7.0), Ok(Value::Float(7.0))),
            (
                Value::Entity(0),
                Err(EvalError::UnavailableEntityPosition(0)),
            ),
            (
                Value::Entity(0xa000_0102),
                Err(EvalError::UnavailableEntityPosition(0xa000_0102)),
            ),
        ] {
            assert_eq!(
                evaluate(&config, &mut Positions([value, Value::Null])),
                expected,
            );
        }
    }
    for opcode in [23, 24, 53, 54] {
        let config = expression(json!([8, 0, 8, 1, opcode, 0]), &dynamics, &[], &[]);
        for unknown in [0, 0xa000_0102] {
            for values in [
                [Value::Entity(unknown), Value::Entity(0xa000_0100)],
                [Value::Vector([0.0; 3]), Value::Entity(unknown)],
            ] {
                assert_eq!(
                    evaluate(&config, &mut Positions(values)),
                    Err(EvalError::UnavailableEntityPosition(unknown)),
                );
            }
        }
        let expected = match opcode {
            23 => Value::Vector([3.0, 4.0, 0.0]),
            24 => Value::Vector([-3.0, -4.0, 0.0]),
            53 => Value::Float(5.0),
            54 => Value::Float(3.0),
            _ => unreachable!(),
        };
        assert_eq!(
            evaluate(
                &config,
                &mut Positions([Value::Entity(0xa000_0100), Value::Entity(0xa000_0101)])
            ),
            Ok(expected),
        );
        // Existing scalar arithmetic and nonentity distance coercions are
        // distinct from a requested entity position being unavailable.
        let expected = match opcode {
            23 => 5.0,
            24 => -1.0,
            _ => 0.0,
        };
        assert_eq!(
            evaluate(
                &config,
                &mut Positions([Value::Integer(2), Value::Integer(3)])
            ),
            Ok(Value::Float(expected)),
        );
    }
}

#[test]
fn reference_natives_are_reported_by_their_outermost_class() {
    let report = |config: Json| {
        let mut context = Natives::default();
        let value = evaluate(&config, &mut context).unwrap();
        (value, context.reported)
    };
    // The reference computes value-or-default itself and answers the
    // operand through its world.
    assert_eq!(
        report(json!({
            "$":"STU_91EFD5B1",
            "m_value":{"$":"STU_E95A9C77","m_EE729DCB":0},
            "m_default":{"$":"STUConfigVarBool","m_value":true},
        })),
        (Value::Bool(false), vec!["STU_E95A9C77".to_owned()])
    );
    // Its world never watches a native's operands.
    assert_eq!(
        report(json!({
            "$":"STU_D3EB8D87", "m_EE729DCB":0,
            "m_00903303":{
                "$":"STU_DB742151", "m_EE729DCB":0,
                "m_entity":{"$":"STU_B5A0CAF0","m_EE729DCB":0},
            },
        })),
        (Value::Bool(false), vec!["STU_D3EB8D87".to_owned()])
    );
    let query = json!({
        "$":"STU_D3EB8D87",
        "m_00903303":{"$":"STU_DB742151","m_entity":{"$":"STU_B5A0CAF0"}},
    });
    for component92 in [Some(true), Some(false), None] {
        let mut world = Natives {
            controller: 0xc000_0102,
            component92,
            ..Natives::default()
        };
        assert_eq!(
            evaluate(&query, &mut world),
            component92
                .map(Value::Bool)
                .ok_or_else(|| EvalError::UnsupportedConfig("STU_D3EB8D87".into()))
        );
    }
    // Resource configs with a GUID are computed; a fieldless one is not.
    assert_eq!(
        report(json!({
            "$":"STUConfigVarIdentifier", "m_identifier":"0x0D80000000000015",
            "m_resourceKey":null,
        })),
        (Value::Asset(0x0d80_0000_0000_0015), Vec::new())
    );
    assert_eq!(
        report(json!({"$":"STU_ABB6C23E","m_EE729DCB":0,"m_resourceKey":null})),
        (Value::Asset(0), vec!["STU_ABB6C23E".to_owned()])
    );
    assert_eq!(
        report(variable(7, Scope::Entity)),
        (Value::Null, Vec::new())
    );
    for (class, field) in [
        ("STUConfigVarTexture", "m_texture"),
        ("STU_08305297", "m_C21DB248"),
    ] {
        assert_eq!(
            report(json!({"$":class,field:null,"m_resourceKey":null})),
            (Value::Asset(0), Vec::new())
        );
    }
}

#[test]
fn enemy_hero_and_health_queries_preserve_known_and_unknown_entities() {
    struct EnemyFacts;
    impl Context for EnemyFacts {
        fn owner(&mut self) -> Value {
            Value::Entity(0xc000_1000)
        }
        fn entity_hero(&mut self, entity: u32) -> Option<u64> {
            (entity == 0xc000_1000).then_some(0x02e0_0000_0000_0175)
        }
        fn entity_definition(&mut self, entity: u32) -> Option<u64> {
            (entity == 0xc000_1000).then_some(0x0400_0000_0000_171c)
        }
        fn body_health(&mut self, entity: u32, _: Option<PoolKind>, _: bool) -> Option<f64> {
            (entity == 0xc000_1000).then_some(123.4)
        }
    }
    let hero = json!({"$":"STU_93DCDDFD","m_entity":{"$":"STU_B5A0CAF0"},"m_resourceKey":null});
    assert_eq!(
        evaluate(&hero, &mut EnemyFacts),
        Ok(Value::Asset(0x02e0_0000_0000_0175))
    );
    assert!(evaluate(&hero, &mut EmptyContext).is_err());
    let definition = json!({"$":"STU_1A3FA5B0","m_entity":{"$":"STU_B5A0CAF0"}});
    assert_eq!(
        evaluate(&definition, &mut EnemyFacts),
        Ok(Value::Asset(0x0400_0000_0000_171c))
    );
    assert_eq!(
        evaluate(&definition, &mut EmptyContext),
        Ok(Value::Asset(0))
    );
    struct UnknownBody;
    impl Context for UnknownBody {
        fn owner(&mut self) -> Value {
            Value::Entity(0xc000_1000)
        }
    }
    assert!(evaluate(&definition, &mut UnknownBody).is_err());
    let maximum = json!({"$":"STU_60146072","m_entity":{"$":"STU_B5A0CAF0"}});
    assert_eq!(evaluate(&maximum, &mut EnemyFacts), Ok(Value::Float(123.4)));
    assert_eq!(evaluate(&maximum, &mut EmptyContext), Ok(Value::Float(0.0)));
    assert_eq!(
        evaluate(
            &json!({"$":"STU_8556841E","m_entityDef":"0x040000000000171C","m_resourceKey":null}),
            &mut EmptyContext
        ),
        Ok(Value::Asset(0x0400_0000_0000_171c))
    );
    let mut keyed = hero;
    keyed["m_resourceKey"] = json!("0x0A90000000000001");
    assert!(evaluate(&keyed, &mut EnemyFacts).is_err());
    assert_eq!(
        evaluate(
            &json!({"$":"STU_9B7648C3","m_entity":{"$":"STU_B5A0CAF0"}}),
            &mut EnemyFacts
        ),
        Ok(Value::Float(f64::from(123.4f32)))
    );
    // The failed base evaluator never evaluates its unimplemented operand.
    assert_eq!(
        evaluate(
            &json!({"$":"STU_B40ACFBA","m_52EA7AB9":{"$":"UnsupportedFixtureOperand"}}),
            &mut EmptyContext
        ),
        Ok(Value::Null)
    );
}

#[test]
fn authored_message_map_looks_up_the_first_matching_key() {
    use crate::script_wire::{ValueEncoding, WireError, write_value};
    use orpheus_wire::bits::BitWriter;

    let row = |key: &str, message: &str| {
        json!({
            "$":"STU_98DD1872", "m_EE729DCB":0,
            "m_key":{"$":"STU_B70476F3","m_548E5344":key,"m_resourceKey":null},
            "m_value":{"$":"STUConfigVarGameMessage","m_gameMessage":message,"m_resourceKey":null},
        })
    };
    let mut literal = json!({
        "$":"STU_7757D403", "m_EE729DCB":0,
        "m_configVars":[
            row("0x0A90000000000042", "0x0240000000000003"),
            row("0x0A90000000000042", "0x0240000000000004"),
        ],
    });
    let mut context = Vars::default();
    let map = evaluate(&literal, &mut context).unwrap();
    assert_eq!(
        evaluate(&literal["m_configVars"][0]["m_key"], &mut context),
        Ok(Value::Asset(0x0a90_0000_0000_0042))
    );
    assert!(truthy(&map));
    context.instance.insert(1, map.clone());
    let lookup = expression(
        json!([8, 0, 8, 1, 17, 0]),
        &[variable(1, Scope::Instance), variable(2, Scope::Instance)],
        &[],
        &[],
    );
    for (key, result) in [
        (0x0a90_0000_0000_0042, Value::Asset(0x0240_0000_0000_0003)),
        (0x0a90_0000_0000_0043, Value::Null),
    ] {
        context.instance.insert(2, Value::Asset(key));
        assert_eq!(evaluate(&lookup, &mut context), Ok(result));
    }
    literal["m_configVars"][0]["m_key"]["m_548E5344"] = json!("0x0240000000000042");
    assert_eq!(
        evaluate(&literal["m_configVars"][0]["m_key"], &mut context),
        Err(EvalError::UnsupportedConfig("STU_B70476F3".into()))
    );
    assert_eq!(
        evaluate(&literal, &mut context),
        Err(EvalError::UnsupportedConfig("STU_7757D403".into()))
    );
    literal["m_configVars"] = Json::Null;
    let empty = evaluate(&literal, &mut context).unwrap();
    assert_eq!(empty, Value::Map(Vec::new()));
    assert!(!truthy(&empty));
    let mut bits = BitWriter::new();
    assert_eq!(
        write_value(&mut bits, &map, ValueEncoding::Explicit),
        Err(WireError::InvalidValue)
    );
    assert_eq!(bits.bit_len(), 0);
}

#[test]
fn fieldless_native_resource_evaluates_to_a_zero_asset() {
    let literal = json!({"$":"STU_ABB6C23E", "m_EE729DCB":0, "m_resourceKey":null});
    let mut context = Vars::default();
    assert_eq!(evaluate(&literal, &mut context), Ok(Value::Asset(0)));

    for (field, value) in [
        ("m_resourceKey", json!("0x0D80000000000015")),
        ("m_EE729DCB", json!(1)),
        ("m_unknown", json!("0x0D80000000000015")),
    ] {
        let mut config = literal.clone();
        config[field] = value;
        assert_eq!(
            evaluate(&config, &mut context),
            Err(EvalError::UnsupportedConfig("STU_ABB6C23E".into()))
        );
    }
}

const FILTER_OWNER: u32 = 0xa000_0101;

#[test]
fn shared_ally_filter_contract() {
    struct Host<'a>(&'a Json);
    impl Host<'_> {
        fn body(&self, entity: u32) -> Option<&Json> {
            self.0["bodies"]
                .as_array()
                .unwrap()
                .iter()
                .find(|body| body["entity"] == entity)
        }
    }
    impl Context for Host<'_> {
        fn owner(&mut self) -> Value {
            Value::Entity(123)
        }
        fn variable(&mut self, var: Variable) -> Value {
            assert_eq!(var.id, 2);
            Value::Entity(self.0["target"].as_u64().unwrap().try_into().unwrap())
        }
        fn parameter(&mut self, id: u32) -> Value {
            match id {
                8 => Value::Entity(999),
                9 => Value::Entity(self.0["parameter9"].as_u64().unwrap().try_into().unwrap()),
                10 => Value::Entity(124),
                _ => Value::Null,
            }
        }
        fn scene_filter_policy(&mut self, filter: &Json) -> Option<SceneFilterPolicy> {
            (filter["m_66747E7D"] == 1 && filter["m_89831602"] == 4).then_some(SceneFilterPolicy {
                relation: FilterRelation::FriendlyOther,
                predicate: if child(filter, "m_condition").is_null() {
                    FilterPredicate::None
                } else {
                    FilterPredicate::Target
                },
            })
        }
        fn combat_targets(&mut self, _config: &Json) -> Option<Vec<u32>> {
            Some(
                std::iter::once(123)
                    .chain(
                        self.0["bodies"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|body| body["entity"].as_u64().unwrap().try_into().unwrap()),
                    )
                    .collect(),
            )
        }
        fn entity_filter_flags(&mut self, entity: u32) -> Option<u64> {
            if entity == 123 {
                self.0["source_flags"].as_u64()
            } else {
                self.body(entity)?["flags"].as_u64()
            }
        }
        fn has_target_tag(&mut self, entity: u32, tag: u64) -> Option<bool> {
            if entity == 123 {
                return Some(false);
            }
            self.body(entity)?["tags"].as_array().map(|tags| {
                tags.iter()
                    .any(|entry| entry.as_u64().unwrap() + 0x0d60_0000_0000_0000 == tag)
            })
        }
        fn known_position(&mut self, entity: u32) -> Option<[f64; 3]> {
            if entity == 123 {
                return Some([0.0; 3]);
            }
            let position = self.body(entity)?["position"].as_array()?;
            Some(std::array::from_fn(|axis| position[axis].as_f64().unwrap()))
        }
    }
    let contract: Json = serde_json::from_str(include_str!(
        "../../../../../fixtures/behavior/statescript-ally-filter-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "statescript.ally-filter.v1");
    assert_eq!(contract["target_build"], 104319);
    let cases = contract["input"]["cases"].as_array().unwrap();
    let results = contract["expected"]["results"].as_array().unwrap();
    assert_eq!(cases.len(), results.len());
    for (case, expected) in cases.iter().zip(results) {
        assert_eq!(case["name"], expected["name"]);
        let first = case["kind"] == "first";
        let query = if first {
            json!({"$":"STU_B3E67A36","m_filter":case["filter"]})
        } else {
            json!({"$":"STU_AC829876","m_filter":case["filter"],"m_entity":{
                    "$":"STUConfigVarDynamic","m_identifier":2,"m_60DB8F99":false}})
        };
        let mut host = Host(case);
        let mut evaluator = Evaluator {
            limit: MAX_EXPRESSION_STEPS,
            steps: 0,
            folds: Vec::new(),
            scoped_parameters: Vec::new(),
            in_native: false,
        };
        let actual = evaluator.config(&query, &mut host, 0);
        let expected = if let Some(error) = expected["error"].as_str() {
            Err(EvalError::UnsupportedConfig(error.into()))
        } else if first {
            Ok(Value::Entity(
                expected["value"].as_u64().unwrap().try_into().unwrap(),
            ))
        } else {
            Ok(Value::Bool(expected["value"].as_bool().unwrap()))
        };
        assert_eq!(actual, expected, "{}", case["name"]);
        assert!(evaluator.scoped_parameters.is_empty(), "{}", case["name"]);
        assert_eq!(evaluator.parameter(&mut host, 8), Value::Entity(999));
    }
}

#[test]
fn shared_combat_target_contract() {
    struct Host<'a>(&'a Json);
    impl Context for Host<'_> {
        fn combat_targets(&mut self, _config: &Json) -> Option<Vec<u32>> {
            self.0["roster_available"].as_bool().unwrap().then(|| {
                self.0["bodies"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|body| body["entity"].as_u64().unwrap().try_into().unwrap())
                    .collect()
            })
        }
        fn entity_filter_flags(&mut self, entity: u32) -> Option<u64> {
            self.0["bodies"]
                .as_array()
                .unwrap()
                .iter()
                .find(|body| body["entity"] == entity)?["flags"]
                .as_u64()
        }
        fn has_target_tag(&mut self, entity: u32, tag: u64) -> Option<bool> {
            self.0["bodies"]
                .as_array()
                .unwrap()
                .iter()
                .find(|body| body["entity"] == entity)?["tags"]
                .as_array()
                .map(|tags| {
                    tags.iter()
                        .any(|entry| entry.as_u64().unwrap() + 0x0d60_0000_0000_0000 == tag)
                })
        }
    }
    let contract: Json = serde_json::from_str(include_str!(
        "../../../../../fixtures/behavior/statescript-combat-target-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "statescript.combat-target.v1");
    assert_eq!(contract["target_build"], 104319);
    let cases = contract["input"]["cases"].as_array().unwrap();
    let expected = contract["expected"]["results"].as_array().unwrap();
    assert_eq!(cases.len(), expected.len());
    let query = json!({"$":"STU_B3E67A36","m_9C7BD8B8":tag_config(0x0d60_0000_0000_0001),
            "m_filter":{"$":"STUConfigVarFilter","m_84065FD1":1,"m_4EA686DB":16}});
    for (case, expected) in cases.iter().zip(expected) {
        assert_eq!(case["name"], expected["name"]);
        let actual = evaluate(&query, &mut Host(case));
        let expected = if expected.get("error").is_some() {
            Err(EvalError::UnsupportedConfig("STU_B3E67A36".into()))
        } else {
            Ok(Value::Entity(
                expected["entity"].as_u64().unwrap().try_into().unwrap(),
            ))
        };
        assert_eq!(actual, expected, "{}", case["name"]);
    }
    let mut evaluator = Evaluator {
        limit: 1,
        steps: 0,
        folds: Vec::new(),
        scoped_parameters: Vec::new(),
        in_native: false,
    };
    let mut host = Host(&cases[5]);
    assert!(matches!(
        evaluator.config(&query, &mut host, 0),
        Err(EvalError::BudgetExceeded { .. })
    ));
}

#[test]
fn shared_hostile_filter_contract() {
    struct Host<'a>(&'a Json);
    impl Context for Host<'_> {
        fn owner(&mut self) -> Value {
            Value::Entity(123)
        }
        fn variable(&mut self, var: Variable) -> Value {
            match var.id {
                1 => self.0["predicate"]
                    .as_bool()
                    .map_or(Value::Null, Value::Bool),
                2 => Value::Entity(124),
                other => panic!("unexpected variable {other}"),
            }
        }
        fn scene_filter_policy(&mut self, filter: &Json) -> Option<SceneFilterPolicy> {
            Some(SceneFilterPolicy {
                relation: FilterRelation::Hostile,
                predicate: if child(filter, "m_condition").is_null() {
                    FilterPredicate::None
                } else {
                    FilterPredicate::Scalar
                },
            })
        }
        fn entity_filter_flags(&mut self, entity: u32) -> Option<u64> {
            match entity {
                123 => self.0["source_flags"].as_u64(),
                124 => self.0["target_flags"].as_u64(),
                _ => None,
            }
        }
        fn has_target_tag(&mut self, entity: u32, tag: u64) -> Option<bool> {
            assert_eq!(entity, 124);
            self.0["tags"].as_array().map(|tags| {
                tags.iter()
                    .any(|entry| entry.as_u64().unwrap() + 0x0d60_0000_0000_0000 == tag)
            })
        }
    }
    let contract: Json = serde_json::from_str(include_str!(
        "../../../../../fixtures/behavior/statescript-hostile-filter-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "statescript.hostile-filter.v1");
    assert_eq!(contract["target_build"], 104319);
    let cases = contract["input"]["cases"].as_array().unwrap();
    let expected = contract["expected"]["results"].as_array().unwrap();
    assert_eq!(cases.len(), expected.len());
    for (case, expected) in cases.iter().zip(expected) {
        assert_eq!(case["name"], expected["name"]);
        let query = json!({"$":"STU_AC829876","m_entity":{"$":"STUConfigVarDynamic","m_identifier":2,"m_60DB8F99":false},"m_filter":case["filter"]});
        let actual = evaluate(&query, &mut Host(case));
        let expected = if expected.get("error").is_some() {
            Err(EvalError::UnsupportedConfig("STU_AC829876".into()))
        } else {
            Ok(Value::Bool(expected["value"].as_bool().unwrap()))
        };
        assert_eq!(actual, expected, "{}", case["name"]);
    }
}
const REQUIRED_TAG: u64 = 0x0d60_0000_0000_0069;
const SECOND_TAG: u64 = 0x0d60_0000_0000_0042;
const EXCLUDED_TAG: u64 = 0x0d60_0000_0000_0041;

#[derive(Default)]
struct FilterWorld {
    entity: Option<u32>,
    flags: Option<u64>,
    tags: BTreeMap<u64, bool>,
}

impl Context for FilterWorld {
    fn owner(&mut self) -> Value {
        self.entity.map_or(Value::Null, Value::Entity)
    }

    fn entity_filter_flags(&mut self, entity: u32) -> Option<u64> {
        assert_eq!(entity, FILTER_OWNER);
        self.flags
    }

    fn has_target_tag(&mut self, entity: u32, tag: u64) -> Option<bool> {
        assert_eq!(entity, FILTER_OWNER);
        self.tags.get(&tag).copied()
    }
}

fn tag_config(tag: u64) -> Json {
    json!({"$":"STU_357DABBB", "m_7D7C86A1":format!("0x{tag:016X}"), "m_resourceKey":null})
}

fn tag_list(tags: &[u64]) -> Json {
    json!({
        "$":"STU_06610EBC",
        "m_configVars": tags.iter().copied().map(tag_config).collect::<Vec<_>>(),
    })
}

fn entity_filter(exclusion: u64) -> Json {
    json!({
        "$":"STU_AC829876",
        "m_entity":{"$":"STU_B5A0CAF0"},
        "m_filter":{
            "$":"STUConfigVarFilter",
            "m_84065FD1":0x3fff_ffff,
            "m_4EA686DB":exclusion,
        },
    })
}

#[test]
fn entity_filter_distinguishes_rejection_from_unavailable_facts() {
    let mut config = entity_filter(2);
    config["m_filter"]["m_9C7BD8B8"] =
        json!({"$":"STU_357DABBB", "m_7D7C86A1":null, "m_resourceKey":null});
    config["m_filter"]["m_B486603E"] = tag_config(REQUIRED_TAG);
    let mut world = FilterWorld {
        entity: Some(FILTER_OWNER),
        ..Default::default()
    };
    let unknown = Err(EvalError::UnsupportedConfig("STU_AC829876".into()));
    assert_eq!(evaluate(&config, &mut world), unknown);
    world.tags.insert(REQUIRED_TAG, false);
    assert_eq!(evaluate(&config, &mut world), Ok(Value::Bool(false)));
    world.tags.insert(REQUIRED_TAG, true);
    assert_eq!(evaluate(&config, &mut world), unknown);
    world.flags = Some(0x100);
    assert_eq!(evaluate(&config, &mut world), Ok(Value::Bool(true)));
    world.flags = Some(2 | 0x100);
    world.tags.clear();
    assert_eq!(evaluate(&config, &mut world), Ok(Value::Bool(false)));
    config["m_filter"]["m_66747E7D"] = json!(2);
    assert_eq!(evaluate(&config, &mut world), unknown);

    let mut config = entity_filter(0x10);
    config["m_filter"]["m_84065FD1"] = json!(1);
    config["m_filter"]["m_1E14CCA8"] = json!(0x100);
    world.flags = None;
    assert_eq!(evaluate(&config, &mut world), unknown);
    for (flags, accepted) in [(0x101, true), (0x100, false), (1, false), (0x111, false)] {
        world.flags = Some(flags);
        assert_eq!(evaluate(&config, &mut world), Ok(Value::Bool(accepted)));
    }
    let mut config = entity_filter(0);
    config["m_filter"]["m_84065FD1"] = json!(0);
    assert_eq!(evaluate(&config, &mut world), Ok(Value::Bool(true)));
}

#[test]
fn shared_absent_target_filter_contract() {
    struct Host {
        entity: Value,
        exists: Option<bool>,
    }
    impl Context for Host {
        fn variable(&mut self, _: Variable) -> Value {
            self.entity.clone()
        }
        fn entity_exists(&mut self, _: u32) -> Option<bool> {
            self.exists
        }
        fn entity_filter_flags(&mut self, _: u32) -> Option<u64> {
            panic!("absent target must not query flags")
        }
    }
    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../../fixtures/behavior/absent-target-filter-v1.json"
    ))
    .unwrap();
    let mut config = json!({"$":"STU_AC829876", "m_filter":fixture["filter"],
            "m_entity":{"$":"STUConfigVarDynamic","m_identifier":393,"m_60DB8F99":0}});
    for case in fixture["cases"].as_array().unwrap() {
        let mut host = Host {
            entity: case["selector"]
                .as_u64()
                .map_or(Value::Null, |id| Value::Entity(id as u32)),
            exists: case["known_exists"].as_bool(),
        };
        if case["selector"] == "absent" {
            config["m_entity"] = Json::Null;
        } else {
            config["m_entity"] =
                json!({"$":"STUConfigVarDynamic","m_identifier":393,"m_60DB8F99":0});
        }
        assert_eq!(
            evaluate(&config, &mut host),
            Ok(Value::Bool(case["result"].as_bool().unwrap()))
        );
    }
    let unsupported = Err(EvalError::UnsupportedConfig("STU_AC829876".into()));
    let mut null = Host {
        entity: Value::Null,
        exists: None,
    };
    for (key, value) in [
        ("unknown", json!(0)),
        ("m_84065FD1", json!("malformed")),
        ("m_EE729DCB", json!(1)),
    ] {
        let mut malformed = config.clone();
        malformed["m_filter"][key] = value;
        assert_eq!(evaluate(&malformed, &mut null), unsupported);
    }
    for exists in [None, Some(true)] {
        let mut live = Host {
            entity: Value::Entity(77),
            exists,
        };
        assert_eq!(evaluate(&config, &mut live), unsupported);
    }
}

#[test]
fn entity_filter_applies_any_all_and_excluded_tag_lists() {
    let mut config = entity_filter(0);
    config["m_filter"]["m_9C7BD8B8"] = tag_list(&[SECOND_TAG, REQUIRED_TAG]);
    config["m_filter"]["m_B486603E"] = tag_list(&[REQUIRED_TAG]);
    config["m_filter"]["m_8B2A4DF3"] = tag_config(EXCLUDED_TAG);
    let mut world = FilterWorld {
        entity: Some(FILTER_OWNER),
        tags: BTreeMap::from([(REQUIRED_TAG, true), (EXCLUDED_TAG, false)]),
        ..Default::default()
    };
    assert_eq!(evaluate(&config, &mut world), Ok(Value::Bool(true)));
    world.tags.insert(REQUIRED_TAG, false);
    assert_eq!(evaluate(&config, &mut world), Ok(Value::Bool(false)));
    world.tags.insert(REQUIRED_TAG, true);
    world.tags.insert(EXCLUDED_TAG, true);
    assert_eq!(evaluate(&config, &mut world), Ok(Value::Bool(false)));
    world.tags.remove(&EXCLUDED_TAG);
    let unsupported = Err(EvalError::UnsupportedConfig("STU_AC829876".into()));
    assert_eq!(evaluate(&config, &mut world), unsupported);
    world.tags.insert(EXCLUDED_TAG, false);
    config["m_filter"]["m_9C7BD8B8"]["m_configVars"] =
        json!([tag_config(REQUIRED_TAG), {"$":"unresolved-tag-config"}]);
    assert_eq!(evaluate(&config, &mut world), unsupported,);
}

#[test]
fn shared_flashbang_filter_admission_contract() {
    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../../fixtures/behavior/flashbang-filter-admission-v1.json"
    ))
    .unwrap();
    assert_eq!(fixture["case_id"], "flashbang.filter-admission.v1");
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 64);
    let excluded: Vec<_> = fixture["excluded_tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tag| 0x0d60_0000_0000_0000 | tag.as_u64().unwrap())
        .collect();
    let mut config = entity_filter(fixture["exclude"].as_u64().unwrap());
    config["m_filter"]["m_84065FD1"] = fixture["any"].clone();
    config["m_filter"]["m_1E14CCA8"] = fixture["all"].clone();
    config["m_filter"]["m_8B2A4DF3"] = tag_list(&excluded);
    for case in fixture["cases"].as_array().unwrap() {
        let inventory: std::collections::BTreeSet<_> = case["inventory"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tag| 0x0d60_0000_0000_0000 | tag.as_u64().unwrap())
            .collect();
        let mut world = FilterWorld {
            entity: Some(FILTER_OWNER),
            flags: case["flags"].as_u64(),
            tags: excluded
                .iter()
                .map(|tag| (*tag, inventory.contains(tag)))
                .collect(),
        };
        assert_eq!(
            evaluate(&config, &mut world),
            Ok(Value::Bool(case["accepted"].as_bool().unwrap())),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn shared_native_filter_tag_clauses() {
    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../../fixtures/behavior/statescript-filter-tag-clauses-v1.json"
    ))
    .unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let mut config = entity_filter(0);
        let mut world = FilterWorld {
            entity: Some(FILTER_OWNER),
            flags: Some(0x40),
            ..Default::default()
        };
        for tag in case["inventory"].as_array().unwrap() {
            world
                .tags
                .insert(0x0d60_0000_0000_0000 + tag.as_u64().unwrap(), true);
        }
        for (field, key) in [
            ("m_9C7BD8B8", "any"),
            ("m_B486603E", "all"),
            ("m_8B2A4DF3", "excluded"),
        ] {
            let tags: Vec<_> = case[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|tag| 0x0d60_0000_0000_0000 + tag.as_u64().unwrap())
                .collect();
            for &tag in &tags {
                world.tags.entry(tag).or_insert(false);
            }
            config["m_filter"][field] = tag_list(&tags);
        }
        assert_eq!(
            evaluate(&config, &mut world),
            Ok(Value::Bool(case["accepted"].as_bool().unwrap())),
            "{}",
            case["name"]
        );
    }
    let mut config = entity_filter(0);
    config["m_filter"]["m_C1903771"] = tag_config(EXCLUDED_TAG);
    let mut world = FilterWorld {
        entity: Some(FILTER_OWNER),
        ..Default::default()
    };
    assert_eq!(
        evaluate(&config, &mut world),
        Err(EvalError::UnsupportedConfig("STU_AC829876".into()))
    );
}

#[test]
fn direct_target_tag_query_reads_membership_on_the_typed_entity() {
    let query = json!({
        "$":"STU_25ECB55F", "m_88B0050A":"0x0D60000000000069", "m_entity":{"$":"STU_B5A0CAF0"}
    });
    let mut world = FilterWorld {
        entity: Some(FILTER_OWNER),
        ..Default::default()
    };
    assert_eq!(
        evaluate(&query, &mut world),
        Err(EvalError::UnsupportedConfig("STU_25ECB55F".into())),
    );
    for present in [true, false] {
        world.tags.insert(REQUIRED_TAG, present);
        assert_eq!(evaluate(&query, &mut world), Ok(Value::Bool(present)));
    }
    world.entity = None;
    assert_eq!(evaluate(&query, &mut world), Ok(Value::Bool(false)));
}

#[test]
fn distinguishes_identifiers_arrays_entities_and_float_comparisons() {
    let id = Value::Asset(5);
    assert!(!equal(&id, &Value::Integer(5)));
    assert!(equal(&id, &Value::Bool(true)));
    assert!(equal(&Value::Integer(1), &Value::Float(1.000001)));
    assert!(!equal(&Value::Integer(1), &Value::Float(1.001)));
    assert!(equal(&Value::Float(0.0), &Value::Null));
    assert!(equal(&Value::Array(vec![id.clone()]), &id));
    assert!(!truthy(&Value::Array(vec![
        Value::Bool(false),
        Value::Bool(true)
    ])));
    assert!(!truthy(&Value::Entity(0xbfff_ffff)));
    assert!(truthy(&Value::Entity(0xa000_0101)));
    assert_eq!(to_int(&Value::Array(vec![Value::Float(-2.5)]), 0), -3);
    assert_eq!(to_int(&id, 9), 9);
    assert_eq!(round_half_away(4_294_967_296.0), 0);
}

#[test]
fn short_circuit_preserves_the_deciding_operand_and_scope_reads() {
    let variables = [variable(1, Scope::Instance), variable(2, Scope::Entity)];
    let mut context = Vars {
        instance: BTreeMap::from([(1, Value::Float(0.0))]),
        entity: BTreeMap::from([(2, Value::Integer(7))]),
        ..Vars::default()
    };
    let and = expression(json!([8, 0, 2, 4, 8, 1, 0]), &variables, &[], &[]);
    assert_eq!(evaluate(&and, &mut context).unwrap(), Value::Float(0.0));
    assert_eq!(
        context.reads,
        [Variable {
            scope: Scope::Instance,
            id: 1
        }]
    );
    context.reads.clear();
    let or = expression(json!([8, 0, 3, 4, 8, 1, 0]), &variables, &[], &[]);
    assert_eq!(evaluate(&or, &mut context).unwrap(), Value::Integer(7));
    assert_eq!(
        context.reads,
        [
            Variable {
                scope: Scope::Instance,
                id: 1
            },
            Variable {
                scope: Scope::Entity,
                id: 2
            }
        ],
    );
    let lvalue_expression = expression(json!([9, 1, 0]), &variables, &[], &[]);
    assert_eq!(
        lvalue(&lvalue_expression).unwrap(),
        Some(Variable {
            scope: Scope::Entity,
            id: 2
        })
    );
}

#[test]
fn executes_only_the_selected_path_and_bounds_nested_expressions() {
    let untaken = expression(json!([6, 0, 2, 255, 6, 1, 0]), &[], &[1.0, 7.0], &[]);
    assert_eq!(
        evaluate(&untaken, &mut EmptyContext).unwrap(),
        Value::Float(7.0)
    );
    for (opcodes, offset, reason) in [
        (json!([1, 0]), 0, "non-progressing jump"),
        (json!([1, 2]), 0, "jump leaves the stream"),
        (json!([10, 1]), 0, "truncated opcode"),
        (json!([23]), 0, "expression stack underflow"),
        (json!([1, -1]), 1, "expected a byte"),
    ] {
        let config = expression(opcodes, &[], &[], &[]);
        assert_eq!(
            evaluate(&config, &mut EmptyContext),
            Err(EvalError::MalformedBytecode { offset, reason }),
        );
    }
    let inner = expression(json!([6, 0, 0]), &[], &[3.0], &[]);
    let nested = expression(json!([14, 0, 0]), &[], &[], &[inner]);
    assert_eq!(
        evaluate_with_budget(&nested, &mut EmptyContext, 3),
        Err(EvalError::BudgetExceeded {
            offset: 2,
            limit: 3
        }),
    );
    assert_eq!(
        evaluate_with_budget(&nested, &mut EmptyContext, 4).unwrap(),
        Value::Float(3.0)
    );
}

#[test]
fn reports_unsupported_operations_instead_of_returning_a_guessed_null() {
    let map = expression(json!([19, 0, 0]), &[], &[], &[]);
    assert_eq!(
        evaluate(&map, &mut EmptyContext),
        Err(EvalError::UnsupportedOpcode {
            opcode: 19,
            offset: 0
        }),
    );
    assert_eq!(
        evaluate(&json!({"$": "STU_BE0BD267"}), &mut EmptyContext),
        Err(EvalError::UnsupportedConfig("STU_BE0BD267".into())),
    );
}

#[test]
fn retail_constant_boolean_queries_reject_unrecognized_forms() {
    for class in ["STU_4A101787", "STU_E95A9C77"] {
        assert_eq!(
            evaluate(&json!({"$":class, "m_EE729DCB":0}), &mut EmptyContext),
            Ok(Value::Bool(false))
        );
        for invalid in [
            json!({"$":class, "m_EE729DCB":1}),
            json!({"$":class, "m_entity":{"$":"STU_B5A0CAF0"}}),
        ] {
            assert_eq!(
                evaluate(&invalid, &mut EmptyContext),
                Err(EvalError::UnsupportedConfig(class.into()))
            );
        }
    }
}

#[test]
fn authored_random_expression_preserves_operand_types_and_inclusive_bounds() {
    struct Samples([u64; 3], usize);

    impl Context for Samples {
        fn random_u64(&mut self) -> Option<u64> {
            let sample = self.0.get(self.1).copied();
            self.1 += 1;
            sample
        }
    }

    let delay = expression(json!([6, 0, 6, 1, 69, 0]), &[], &[6.0, 12.0], &[]);
    let mut samples = Samples([1 << 63, 0, 2], 0);
    assert_eq!(evaluate(&delay, &mut samples), Ok(Value::Float(9.0)));
    let high = json!({"$":"STUConfigVarInt","m_value":3});
    let low = json!({"$":"STUConfigVarInt","m_value":1});
    let inclusive = expression(json!([14, 0, 14, 1, 69, 0]), &[], &[], &[high, low]);
    assert_eq!(evaluate(&inclusive, &mut samples), Ok(Value::Integer(3)));
    assert_eq!(samples.1, 3);

    let integral = expression(json!([6, 0, 6, 1, 70, 0]), &[], &[1.0, 2.0], &[]);
    assert_eq!(
        evaluate(&integral, &mut Samples([0; 3], 0)),
        Ok(Value::Integer(1))
    );
    assert_eq!(
        evaluate_with_budget(&inclusive, &mut Samples([0; 3], 0), 3),
        Err(EvalError::BudgetExceeded {
            offset: 4,
            limit: 3
        }),
    );
}

#[test]
fn random_float_literal_rounds_bounds_and_supports_reverse_ranges() {
    struct Sample(u64);
    impl Context for Sample {
        fn random_u64(&mut self) -> Option<u64> {
            Some(self.0)
        }
    }
    for (low, high, sample, expected) in [
        (6.0, 12.0, 1 << 63, 9.0),
        (12.0, 6.0, 1 << 63, 9.0),
        (6.0, 12.0, 0, 6.0),
        (0.1, 0.1, u64::MAX, f64::from(0.1f32)),
    ] {
        assert_eq!(
            evaluate(
                &json!({"$":"STU_BE0BD267","m_min":low,"m_max":high}),
                &mut Sample(sample)
            ),
            Ok(Value::Float(expected))
        );
    }
    for (class, field) in [
        ("STUConfigVarTexture", "m_texture"),
        ("STU_08305297", "m_C21DB248"),
    ] {
        let config = json!({"$":class,field:null,"m_resourceKey":null});
        assert_eq!(evaluate(&config, &mut EmptyContext), Ok(Value::Asset(0)));
        let mut keyed = config;
        keyed["m_resourceKey"] = json!("0x0A90000000000010");
        assert_eq!(
            evaluate(&keyed, &mut EmptyContext),
            Err(EvalError::UnsupportedConfig(class.into()))
        );
    }
}

#[test]
#[ignore = "requires an authorized 104319 graph pack in ORPHEUS_RUNTIME_DIR"]
fn authored_controller_hud_and_soldier_config_values_match_python() {
    let data =
        PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").expect("set ORPHEUS_RUNTIME_DIR"))
            .join("data");
    let graphs = GraphStore::load(data.join("game_graphs_174.json.gz")).unwrap();
    let mut context = Vars {
        instance: BTreeMap::from([
            (28, Value::Integer(1)),
            (
                375,
                Value::Array(vec![Value::Bool(false), Value::Bool(true)]),
            ),
            (581, Value::Float(1.5)),
            (6884, Value::Float(rounded(0.511))),
            (6885, Value::Float(rounded(0.1))),
            (
                8449,
                Value::Array(vec![Value::Integer(1), Value::Integer(2)]),
            ),
            (11938, Value::Integer(7)),
            (17937, Value::Entity(0xa000_0100)),
            (25485, Value::Bool(true)),
        ]),
        entity: BTreeMap::from([
            (31, Value::Integer(1)),
            (9537, Value::Float(0.5)),
            (17153, Value::Bool(true)),
            (17154, Value::Asset(0x02e0_0000_0000_006e)),
            (27534, Value::Bool(true)),
        ]),
        ..Vars::default()
    };
    // Existing Python expr.evaluate against the same target graph fields
    // and scoped fixture variables. The timeout vector includes f32 bits.
    for (graph, rid, field, expected) in [
        (0x13c0, 6, "m_value", Value::Bool(false)),
        (0x13c0, 20, "m_condition", Value::Bool(true)),
        (0x13c1, 3, "m_condition", Value::Integer(7)),
        (0x0c90, 0, "m_16432044", Value::Asset(0x02e0_0000_0000_006e)),
        (0x288a, 10, "m_condition", Value::Bool(true)),
        (0x288b, 14, "m_hero", Value::Asset(0x02e0_0000_0000_0002)),
        (0x20e4, 3, "m_87276FE6", Value::Integer(23)),
        (0x0254, 8, "m_value", Value::Integer(30)),
    ] {
        let node = graphs.graph(graph).unwrap().by_rid(rid).unwrap();
        assert_eq!(
            evaluate(node.field(field).unwrap(), &mut context).unwrap(),
            expected,
            "{graph:04x} rid {rid} {field}"
        );
    }
    for (graph, state, field, expected) in [
        (
            0x0254,
            51,
            "m_timeout",
            Value::Float(f64::from(f32::from_bits(0x3f44_3958))),
        ),
        (0x0257, 12, "m_value", Value::Float(7.5)),
        (0x0258, 0, "m_05BE62DD", Value::Vector([0.0, 1.0, 0.0])),
    ] {
        let node = graphs.graph(graph).unwrap().state(state).unwrap();
        assert_eq!(
            evaluate(node.field(field).unwrap(), &mut context).unwrap(),
            expected,
            "{graph:04x} st {state} {field}"
        );
    }
}

#[test]
fn shared_body_field_contract() {
    fn decode(value: &Json) -> Value {
        if let Some(entity) = value.get("entity").and_then(Json::as_u64) {
            Value::Entity(entity.try_into().unwrap())
        } else if let Some(asset) = value.get("asset").and_then(Json::as_u64) {
            Value::Asset(asset)
        } else if let Some(rows) = value.as_array() {
            Value::Array(rows.iter().map(decode).collect())
        } else if let Some(flag) = value.as_bool() {
            Value::Bool(flag)
        } else if let Some(number) = value.as_i64() {
            Value::Integer(number)
        } else {
            assert!(value.is_null());
            Value::Null
        }
    }
    struct Host<'a>(&'a Json);
    impl Context for Host<'_> {
        fn variable(&mut self, variable: Variable) -> Value {
            assert_eq!(
                variable,
                Variable {
                    scope: Scope::Instance,
                    id: 1
                }
            );
            decode(&self.0["value"])
        }
        fn related_entity(&mut self, relation: EntityRelation, entity: u32) -> Option<u32> {
            assert_eq!(relation, EntityRelation::ControllerBodyField);
            if self.0["available"] == false {
                return None;
            }
            let row = self.0["targets"].get(entity.to_string())?;
            if row == "absent" || row.get("body").is_some_and(Json::is_null) {
                Some(0)
            } else {
                row.get("body")?.as_u64().map(|id| id.try_into().unwrap())
            }
        }
    }
    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../../fixtures/behavior/statescript-body-field-v1.json"
    ))
    .unwrap();
    assert_eq!(fixture["schema_version"], 1);
    assert_eq!(fixture["case_id"], "statescript.body-field.v1");
    assert_eq!(fixture["target_build"], 104319);
    let cases = fixture["input"]["cases"].as_array().unwrap();
    let results = fixture["expected"]["results"].as_array().unwrap();
    assert_eq!(cases.len(), results.len());
    for (case, expected) in cases.iter().zip(results) {
        assert_eq!(case["name"], expected["name"]);
        let actual = match evaluate(&case["config"], &mut Host(case)) {
            Ok(Value::Entity(entity)) => json!({"entity":entity}),
            Err(EvalError::UnsupportedConfig(class)) => json!({"error":class}),
            value => panic!("{}: {value:?}", case["name"]),
        };
        assert_eq!(actual, expected["output"], "{}", case["name"]);
    }
    // The native catalog establishes this class's parent fallback independently
    // of the query consumer; exercise its normal VM state classification.
    let mut graph = fixture["input"]["graph"].clone();
    graph["nodes"][0]["class"] = fixture["native_fallback"]["class"].clone();
    let index = graph["index"].as_u64().unwrap() as u32;
    let graphs = crate::script_graph::GraphStore::from_json(
        &serde_json::to_vec(&json!({"version":1,"graphs":[graph],"bodies":{},"missing":[]}))
            .unwrap(),
    )
    .unwrap();
    let component = crate::script_init::Component::start(
        std::sync::Arc::new(graphs),
        fixture["input"]["owner"].as_u64().unwrap() as u32,
        crate::script_wire::Variables::new(),
        vec![crate::script_init::Root::listener(1, index, Some(0))],
        0,
        &mut crate::script_init::EmptyWorld,
        crate::script_init::MAX_INIT_WORK,
    )
    .unwrap();
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    let state = &snapshot.instances[&1].states[&0];
    assert!(state.active);
    assert_eq!(
        format!("{:?}", state.kind),
        fixture["native_fallback"]["expected_kind"]
            .as_str()
            .unwrap()
    );
}
