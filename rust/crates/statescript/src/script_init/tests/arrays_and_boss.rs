use super::*;
use crate::script_expr::EntityRelation;

fn array_insert_fixture(
    initial: Value,
    value: Value,
    index: Option<Value>,
    unique: bool,
    single: bool,
    extra: Json,
) -> Result<Component, InitFault> {
    let mut fields = json!({"m_26E3B366":var(100,true), "m_value":var(102,true),
        "m_index":index.as_ref().map(|_| var(103,true)), "m_8AD67B20":unique,
        "m_outPlug":plug(&[(2,"m_inPlug")],&[])});
    for (key, value) in extra.as_object().unwrap() {
        fields[key] = value.clone();
    }
    let fixture = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_inPlug")],&[])}),
            ),
            node(
                1,
                if single {
                    "STU_CBE8BDAA"
                } else {
                    "STU_4C3509FB"
                },
                None,
                fields,
            ),
            node(
                2,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var":var(101,true),"m_value":true}),
            ),
        ],
        json!([]),
        json!([0]),
    );
    let mut variables = Variables::from([(100, initial), (102, value)]);
    if let Some(index) = index {
        variables.insert(103, index);
    }
    Component::start(
        Arc::new(store(vec![fixture])),
        123,
        variables,
        vec![Root::new(1, 42)],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
}

#[test]
fn engine_setting_watchers_poll_the_live_host_after_startup() {
    struct Host(script_expr::EngineSetting);
    impl World for Host {
        fn engine_setting(&self, site: Site, key: u32) -> script_expr::EngineSetting {
            assert_eq!((site.entity, site.graph, key), (123, 42, 1));
            self.0.clone()
        }
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }
    }
    let fixture = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_beginPlug")],&[])}),
            ),
            node(
                1,
                "STUStatescriptStateBooleanSwitch",
                Some(0),
                json!({
            "m_condition":{"$":"STU_A753DC79","m_EE729DCB":0,"m_74A2248F":1},
            "m_trueSubgraphPlug":plug(&[(2,"m_beginPlug")],&[1]),
            "m_falseSubgraphPlug":plug(&[(3,"m_beginPlug")],&[2])}),
            ),
            node(2, "STUStatescriptState", Some(1), json!({})),
            node(3, "STUStatescriptState", Some(2), json!({})),
        ],
        json!([1, 2, 3]),
        json!([0]),
    );
    let mut host = Host(script_expr::EngineSetting::Present(Value::Bool(false)));
    let mut component = Component::start(
        Arc::new(store(vec![fixture])),
        123,
        Variables::new(),
        vec![Root::new(1, 42)],
        0,
        &mut host,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert!(component.snapshot().unwrap().instances[&1].states[&2].active);
    for (frame, value, expected) in [
        (
            1,
            script_expr::EngineSetting::Present(Value::Bool(true)),
            true,
        ),
        (
            2,
            script_expr::EngineSetting::Present(Value::Bool(false)),
            false,
        ),
        (
            3,
            script_expr::EngineSetting::Present(Value::Bool(true)),
            true,
        ),
        (4, script_expr::EngineSetting::Missing, false),
    ] {
        host.0 = value;
        component.advance_frame(frame, &mut host).unwrap();
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(
            snapshot.instances[&1].states[&0].kind,
            StateKind::BooleanSwitch(expected)
        );
        assert_eq!(snapshot.instances[&1].states[&1].active, expected);
        assert_eq!(snapshot.instances[&1].states[&2].active, !expected);
    }
}

#[test]
fn velocity_watchers_poll_live_mover_changes() {
    struct Host(script_expr::VelocityQuery);
    impl World for Host {
        fn entity_velocity(&self, entity: u32) -> script_expr::VelocityQuery {
            assert_eq!(entity, 123);
            self.0
        }
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }
    }
    let fixture = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_beginPlug")],&[])}),
            ),
            node(
                1,
                "STUStatescriptStateBooleanSwitch",
                Some(0),
                json!({
            "m_condition":{"$":"STU_919FD47C","m_configVars":[{"$":"STU_309B0570","m_entity":{"$":"STU_B5A0CAF0"},"m_5B94EDB8":null}],"m_expression":{"m_opcodes":[14,0,50,0]}},
            "m_trueSubgraphPlug":plug(&[(2,"m_beginPlug")],&[1]),
            "m_falseSubgraphPlug":plug(&[(3,"m_beginPlug")],&[2])}),
            ),
            node(2, "STUStatescriptState", Some(1), json!({})),
            node(3, "STUStatescriptState", Some(2), json!({})),
        ],
        json!([1, 2, 3]),
        json!([0]),
    );
    let mut host = Host(script_expr::VelocityQuery::Present([0.0; 3]));
    let mut component = Component::start(
        Arc::new(store(vec![fixture])),
        123,
        Variables::new(),
        vec![Root::new(1, 42)],
        0,
        &mut host,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert!(component.snapshot().unwrap().instances[&1].states[&2].active);
    for (frame, value, expected) in [
        (
            1,
            script_expr::VelocityQuery::Present([1.0, 0.0, 0.0]),
            true,
        ),
        (2, script_expr::VelocityQuery::Present([0.0; 3]), false),
        (
            3,
            script_expr::VelocityQuery::Present([1.0, 0.0, 0.0]),
            true,
        ),
        (4, script_expr::VelocityQuery::Unbound, false),
    ] {
        host.0 = value;
        component.advance_frame(frame, &mut host).unwrap();
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(
            snapshot.instances[&1].states[&0].kind,
            StateKind::BooleanSwitch(expected)
        );
        assert_eq!(snapshot.instances[&1].states[&1].active, expected);
        assert_eq!(snapshot.instances[&1].states[&2].active, !expected);
    }
}

#[test]
fn total_health_watchers_poll_damage_death_and_unbinding() {
    struct Host(script_expr::HealthQuery);
    impl World for Host {
        fn total_health(&self, _site: Site, entity: u32) -> script_expr::HealthQuery {
            assert_eq!(entity, 123);
            self.0
        }
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }
    }
    let fixture = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_beginPlug")],&[])}),
            ),
            node(
                1,
                "STUStatescriptStateBooleanSwitch",
                Some(0),
                json!({
            "m_condition":{"$":"STU_9B7648C3","m_entity":{"$":"STU_B5A0CAF0"}},
            "m_trueSubgraphPlug":plug(&[(2,"m_beginPlug")],&[1]),
            "m_falseSubgraphPlug":plug(&[(3,"m_beginPlug")],&[2])}),
            ),
            node(2, "STUStatescriptState", Some(1), json!({})),
            node(3, "STUStatescriptState", Some(2), json!({})),
        ],
        json!([1, 2, 3]),
        json!([0]),
    );
    let mut host = Host(script_expr::HealthQuery::Present(0.0));
    let mut component = Component::start(
        Arc::new(store(vec![fixture])),
        123,
        Variables::new(),
        vec![Root::new(1, 42)],
        0,
        &mut host,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert!(component.snapshot().unwrap().instances[&1].states[&2].active);
    for (frame, value, expected) in [
        (1, script_expr::HealthQuery::Present(50.0), true),
        (2, script_expr::HealthQuery::Present(0.0), false),
        (3, script_expr::HealthQuery::Present(50.0), true),
        (4, script_expr::HealthQuery::Unbound, false),
    ] {
        host.0 = value;
        component.advance_frame(frame, &mut host).unwrap();
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(
            snapshot.instances[&1].states[&0].kind,
            StateKind::BooleanSwitch(expected)
        );
        assert_eq!(snapshot.instances[&1].states[&1].active, expected);
        assert_eq!(snapshot.instances[&1].states[&2].active, !expected);
    }
}

#[test]
fn foreign_component_watchers_poll_updated_room_snapshots() {
    struct Host(crate::script_entity_state::EntityScriptState);
    impl World for Host {
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
        fn entity_variable(&mut self, entity: u32, id: u32) -> Option<Value> {
            assert_eq!((entity, id), (0xa000_0100, 7));
            self.0.read(id)
        }
    }
    let fixture = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_beginPlug")],&[])}),
            ),
            node(
                1,
                "STUStatescriptStateBooleanSwitch",
                Some(0),
                json!({
            "m_condition":{"$":"STU_919FD47C","m_configVars":[{"$":"STUConfigVarInt","m_value":7}],"m_expression":{"m_opcodes":[8,0,14,0,15,0],"m_dynamicVars":[var(99,true)]}},
            "m_trueSubgraphPlug":plug(&[(2,"m_beginPlug")],&[1]),
            "m_falseSubgraphPlug":plug(&[(3,"m_beginPlug")],&[2])}),
            ),
            node(2, "STUStatescriptState", Some(1), json!({})),
            node(3, "STUStatescriptState", Some(2), json!({})),
        ],
        json!([1, 2, 3]),
        json!([0]),
    );
    let live = |value| {
        crate::script_entity_state::EntityScriptState::Running(Arc::new(Variables::from([(
            7, value,
        )])))
    };
    let mut host = Host(live(Value::Bool(false)));
    let mut component = Component::start(
        Arc::new(store(vec![fixture])),
        123,
        Variables::from([(99, Value::Entity(0xa000_0100))]),
        vec![Root::new(1, 42)],
        0,
        &mut host,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert!(component.snapshot().unwrap().instances[&1].states[&2].active);
    for (frame, value, expected) in [
        (1, live(Value::Bool(true)), true),
        (2, live(Value::Bool(false)), false),
        (3, live(Value::Bool(true)), true),
        (
            4,
            crate::script_entity_state::EntityScriptState::Absent,
            false,
        ),
    ] {
        host.0 = value;
        component.advance_frame(frame, &mut host).unwrap();
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(
            snapshot.instances[&1].states[&0].kind,
            StateKind::BooleanSwitch(expected)
        );
        assert_eq!(snapshot.instances[&1].states[&1].active, expected);
        assert_eq!(snapshot.instances[&1].states[&2].active, !expected);
    }
}

pub(super) fn shared_contains_condition_contract() {
    fn value(json: &Json) -> Value {
        match json {
            Json::Null => Value::Null,
            Json::Bool(v) => Value::Bool(*v),
            Json::Number(n) => n
                .as_i64()
                .map_or_else(|| Value::Float(n.as_f64().unwrap()), Value::Integer),
            Json::Array(items) => Value::Array(items.iter().map(value).collect()),
            other => Value::Entity(other["entity"].as_u64().unwrap().try_into().unwrap()),
        }
    }
    let contract: Json = serde_json::from_str(include_str!(
        "../../../../../../fixtures/behavior/statescript-contains-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(
        contract["case_id"],
        "statescript.container-contains-condition.v1"
    );
    assert_eq!(contract["target_build"], 104319);
    let cases = contract["input"]["cases"].as_array().unwrap();
    let expected = contract["expected"]["results"].as_array().unwrap();
    assert_eq!(cases.len(), expected.len());
    for (case, expected) in cases.iter().zip(expected) {
        assert_eq!(case["name"], expected["name"]);
        let links = case["links"].as_bool().unwrap();
        let fixture = graph(
            42,
            vec![
                node(
                    0,
                    "STUStatescriptEntry",
                    None,
                    json!({"m_outPlug":plug(&[(1,"m_inPlug")],&[])}),
                ),
                node(
                    1,
                    "STU_821116A6",
                    None,
                    json!({"m_814F8EC3":var(100,true),"m_value":var(101,true),"m_truePlug":plug(if links {&[(2,"m_inPlug")]} else {&[]},&[]),"m_falsePlug":plug(if links {&[(3,"m_inPlug")]} else {&[]},&[])}),
                ),
                node(
                    2,
                    "STUStatescriptActionSetVar",
                    None,
                    json!({"m_out_Var":var(90,true),"m_value":{"$":"STUConfigVarInt","m_value":19}}),
                ),
                node(
                    3,
                    "STUStatescriptActionSetVar",
                    None,
                    json!({"m_out_Var":var(90,true),"m_value":{"$":"STUConfigVarInt","m_value":23}}),
                ),
            ],
            json!([]),
            json!([0]),
        );
        let snapshot = initialize(
            &store(vec![fixture]),
            123,
            Variables::from([
                (100, value(&case["container"])),
                (101, value(&case["value"])),
            ]),
            vec![Root::new(1, 42)],
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap();
        assert!(
            snapshot.diagnostics.is_empty(),
            "{}: {:?}",
            case["name"],
            snapshot.diagnostics
        );
        assert_eq!(
            snapshot.variables.get(&90).cloned().unwrap_or(Value::Null),
            value(&expected["marker"]),
            "{}",
            case["name"]
        );
    }
}

pub(super) fn shared_corpse_retirement_contract() {
    struct Host {
        accept: bool,
        requests: Vec<(u32, bool)>,
    }
    impl World for Host {
        fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
            None
        }
        fn position(&mut self, _: u32) -> Option<[f64; 3]> {
            None
        }
        fn request_corpse_retirement(&mut self, _: Site, entity: u32, phase_exit: bool) -> bool {
            self.requests.push((entity, phase_exit));
            self.accept
        }
    }
    let contract: Json = serde_json::from_str(include_str!(
        "../../../../../../fixtures/behavior/statescript-corpse-retirement-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(
        contract["case_id"],
        "statescript.corpse-retirement-action.v1"
    );
    assert_eq!(contract["target_build"], 104319);
    let cases = contract["input"]["cases"].as_array().unwrap();
    let expected = contract["expected"]["results"].as_array().unwrap();
    assert_eq!(cases.len(), expected.len());
    for (case, expected) in cases.iter().zip(expected) {
        assert_eq!(case["name"], expected["name"]);
        let index = case["graph"].as_u64().unwrap() as u32;
        let position: u16 = case["position"].as_u64().unwrap().try_into().unwrap();
        let target = match &case["target"] {
            Json::Null => Value::Null,
            Json::Number(n) => Value::Integer(n.as_i64().unwrap()),
            json => Value::Entity(json["entity"].as_u64().unwrap().try_into().unwrap()),
        };
        let mut fixture = graph(
            index,
            vec![
                node(
                    0,
                    "STUStatescriptEntry",
                    None,
                    json!({"m_outPlug":plug(&[(position,"m_inPlug")],&[])}),
                ),
                node(
                    position,
                    "STU_523EE38D",
                    None,
                    json!({"m_entity":var(1,true),"m_outPlug":plug(&[(99,"m_inPlug")],&[])}),
                ),
                node(
                    99,
                    "STUStatescriptActionSetVar",
                    None,
                    json!({"m_out_Var":var(90,true),"m_value":{"$":"STUConfigVarBool","m_value":true}}),
                ),
            ],
            json!([]),
            json!([0]),
        );
        fixture["items"] = json!(100);
        fixture["header"]["m_nodesBitCount"] = json!(7);
        let mut host = Host {
            accept: case["accept"].as_bool().unwrap(),
            requests: Vec::new(),
        };
        let snapshot = initialize(
            &store(vec![fixture]),
            contract["input"]["owner"]
                .as_u64()
                .unwrap()
                .try_into()
                .unwrap(),
            Variables::from([(1, target)]),
            vec![Root::new(1, index)],
            &mut host,
            MAX_INIT_WORK,
        )
        .unwrap();
        let requests: Vec<_> = expected["requests"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| (row[0].as_u64().unwrap() as u32, row[1].as_bool().unwrap()))
            .collect();
        assert_eq!(host.requests, requests, "{}", case["name"]);
        assert_eq!(
            snapshot.variables.get(&90) == Some(&Value::Bool(true)),
            expected["followed"].as_bool().unwrap(),
            "{}",
            case["name"]
        );
        assert_eq!(
            !snapshot.diagnostics.is_empty(),
            expected["failed"].as_bool().unwrap(),
            "{}: {:?}",
            case["name"],
            snapshot.diagnostics
        );
    }
}

pub(super) fn shared_unkeyed_array_contract() {
    fn value(json: &Json) -> Value {
        match json {
            Json::Null => Value::Null,
            Json::Bool(value) => Value::Bool(*value),
            Json::Number(number) => number
                .as_i64()
                .map_or_else(|| Value::Float(number.as_f64().unwrap()), Value::Integer),
            Json::Array(items) => Value::Array(items.iter().map(value).collect()),
            Json::Object(fields) if fields.len() == 1 && fields.contains_key("entity") => {
                Value::Entity(fields["entity"].as_u64().unwrap().try_into().unwrap())
            }
            Json::Object(fields) if fields.len() == 1 && fields.contains_key("float") => {
                Value::Float(match fields["float"].as_str().unwrap() {
                    "NaN" => f64::NAN,
                    "Infinity" => f64::INFINITY,
                    other => panic!("unsupported float token {other}"),
                })
            }
            other => panic!("unsupported contract value {other}"),
        }
    }
    let contract: Json = serde_json::from_str(include_str!(
        "../../../../../../fixtures/behavior/statescript-array-insert-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "statescript.unkeyed-array-insert.v1");
    assert_eq!(contract["target_build"], 104319);
    assert_eq!(contract["input"]["output_scope"], "entity");
    let cases = contract["input"]["cases"].as_array().unwrap();
    let expected = contract["expected"]["states"].as_array().unwrap();
    assert_eq!(cases.len(), expected.len());
    for (case, expected) in cases.iter().zip(expected) {
        assert_eq!(case["name"], expected["name"]);
        let mut extra = json!({});
        for (input, field) in [
            ("source_config", "m_value"),
            ("index_config", "m_index"),
            ("key_config", "m_key"),
        ] {
            if let Some(config) = case.get(input) {
                extra[field] = config.clone();
            }
        }
        let component = array_insert_fixture(
            value(&case["current"]),
            value(&case["source"]),
            case["index_present"]
                .as_bool()
                .unwrap()
                .then(|| value(&case["index"])),
            case["unique"].as_u64().unwrap() != 0,
            case["single"].as_bool().unwrap(),
            extra,
        )
        .unwrap();
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{}: {:?}",
            case["name"],
            snapshot.diagnostics
        );
        assert_eq!(
            snapshot.variables[&100],
            value(&expected["value"]),
            "{}",
            case["name"]
        );
        assert_eq!(
            snapshot.variables.get(&101) == Some(&Value::Bool(true)),
            expected["followed"].as_bool().unwrap(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn unkeyed_array_insertion_matches_native_merge_single_value_and_index_rules() {
    let ints = |items: &[i64]| Value::Array(items.iter().copied().map(Value::Integer).collect());
    let nested = Value::Array(vec![
        Value::Integer(2),
        Value::Array(vec![Value::Integer(3), Value::Null]),
        Value::Integer(4),
    ]);
    for (initial, source, index, unique, single, expected) in [
        (
            ints(&[1]),
            nested.clone(),
            None,
            false,
            false,
            Value::Array(vec![
                Value::Integer(1),
                Value::Integer(2),
                Value::Integer(3),
                Value::Null,
                Value::Integer(4),
            ]),
        ),
        (
            ints(&[1, 5]),
            nested.clone(),
            Some(Value::Integer(1)),
            false,
            true,
            Value::Array(vec![Value::Integer(1), nested, Value::Integer(5)]),
        ),
        (
            Value::Integer(99),
            Value::Integer(7),
            None,
            false,
            false,
            ints(&[7]),
        ),
        (
            ints(&[1, 2, 3]),
            ints(&[7, 8]),
            Some(Value::Integer(1)),
            false,
            false,
            ints(&[1, 7, 8, 2, 3]),
        ),
        (
            ints(&[1, 2]),
            Value::Integer(7),
            Some(Value::Integer(4)),
            false,
            false,
            ints(&[1, 2, 0, 0, 7]),
        ),
        (
            Value::Integer(99),
            Value::Null,
            Some(Value::Integer(3)),
            false,
            false,
            ints(&[0, 0, 0]),
        ),
        (
            ints(&[1]),
            ints(&[0, 2, 2, 3]),
            Some(Value::Integer(3)),
            true,
            false,
            ints(&[1, 0, 0, 2, 3]),
        ),
        (
            ints(&[1]),
            Value::Array(vec![
                Value::Float(1.00001),
                Value::Bool(true),
                Value::Entity(1),
            ]),
            None,
            true,
            false,
            Value::Array(vec![Value::Integer(1), Value::Entity(1)]),
        ),
        (
            ints(&[1, 2, 3]),
            Value::Integer(7),
            Some(Value::Float(1.5)),
            false,
            false,
            ints(&[1, 2, 7, 3]),
        ),
        (
            ints(&[1, 2, 3]),
            Value::Integer(7),
            Some(Value::Float(-0.5)),
            false,
            false,
            ints(&[1, 2, 3, 7]),
        ),
        (
            ints(&[1, 2, 3]),
            Value::Integer(7),
            Some(Value::Float(f64::NAN)),
            false,
            false,
            ints(&[1, 2, 3, 7]),
        ),
        (
            ints(&[1, 2, 3]),
            Value::Integer(7),
            Some(Value::Float(f64::INFINITY)),
            false,
            false,
            ints(&[1, 2, 3, 7]),
        ),
        (
            ints(&[1, 2, 3]),
            Value::Integer(7),
            Some(Value::Null),
            false,
            false,
            ints(&[7, 1, 2, 3]),
        ),
        (
            ints(&[1, 2, 3]),
            Value::Integer(7),
            Some(ints(&[])),
            false,
            false,
            ints(&[1, 2, 3, 7]),
        ),
        (
            ints(&[1]),
            Value::Null,
            None,
            false,
            true,
            Value::Array(vec![Value::Integer(1), Value::Null]),
        ),
    ] {
        let component =
            array_insert_fixture(initial, source, index, unique, single, json!({})).unwrap();
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(snapshot.variables[&100], expected);
        assert_eq!(snapshot.variables[&101], Value::Bool(true));
    }
}

#[test]
fn array_insert_providers_see_prior_rows_and_self_reference_stops_the_iterator() {
    let expression = json!({"$":"STU_919FD47C","m_configVars":null,"m_expression":{
        "m_D99EF254":null,"m_dynamicVars":[var(100,true)],"m_fragments":null,"m_opcodes":[8,0,0]}});
    for (source, expected) in [
        (
            json!({"$":"STU_06610EBC","m_configVars":[7,expression,9]}),
            vec![7, 7, 9],
        ),
        (
            json!({"$":"STU_06610EBC","m_configVars":[7,var(100,true),{"$":"UnknownAfterSelf"}]}),
            vec![7],
        ),
    ] {
        let component = array_insert_fixture(Value::Integer(99),Value::Null,None,false,false,
            json!({"m_value":source,"m_index":{"$":"STU_06610EBC","m_configVars":[-1,{"$":"UnknownSecondIndex"}]}})).unwrap();
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(
            snapshot.variables[&100],
            Value::Array(expected.into_iter().map(Value::Integer).collect())
        );
        assert_eq!(snapshot.variables[&101], Value::Bool(true));
    }
}

#[test]
fn array_insert_skips_absent_invalid_and_zero_targets_but_keeps_storage_limits_explicit() {
    for extra in [
        json!({"m_26E3B366":null}),
        json!({"m_value":null}),
        json!({"m_26E3B366":{"$":"STUConfigVarInt","m_value":1}}),
        json!({"m_26E3B366":var(0,true)}),
        json!({"m_key":0}),
    ] {
        let component = array_insert_fixture(
            Value::Integer(99),
            Value::Null,
            None,
            false,
            false,
            json!({"m_value":{"$":"UnknownSource"}})
                .as_object()
                .unwrap()
                .iter()
                .chain(extra.as_object().unwrap().iter())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        )
        .unwrap();
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(snapshot.variables[&100], Value::Integer(99));
        assert_eq!(snapshot.variables[&101], Value::Bool(true));
    }
    for (extra, feature, length) in [
        (
            json!({"m_index":201}),
            "array insertion native limit diagnostic",
            None,
        ),
        (
            json!({"m_index":200}),
            "array insertion native limit diagnostic",
            Some(201),
        ),
        (
            json!({"m_key":{"$":"STUConfigVarIdentifier","m_identifier":"0x0D80000000000011"}}),
            "array insertion keyed row storage",
            None,
        ),
    ] {
        let component = array_insert_fixture(
            Value::Integer(99),
            Value::Integer(7),
            None,
            false,
            false,
            extra,
        )
        .unwrap();
        let snapshot = component.snapshot().unwrap();
        assert_eq!(snapshot.diagnostics.len(), 1, "{:?}", snapshot.diagnostics);
        assert_eq!(
            snapshot.diagnostics[0].issue,
            Issue::UnsupportedField(feature)
        );
        assert!(!snapshot.variables.contains_key(&101));
        match length {
            Some(length) => assert!(
                matches!(&snapshot.variables[&100],Value::Array(values) if values.len()==length)
            ),
            None => assert_eq!(snapshot.variables[&100], Value::Integer(99)),
        }
    }
}

#[test]
fn array_insert_failed_native_preserves_scalar_defaults_and_single_value_zero() {
    let failed = json!({"$":"STU_7BD2CA19","m_EE729DCB":0,"m_resourceKey":null});
    for (extra, single, expected) in [
        (json!({"m_index":failed.clone()}), false, vec![1, 7]),
        (json!({"m_key":failed.clone()}), false, vec![1]),
        (json!({"m_value":failed.clone()}), true, vec![1, 0]),
        (json!({"m_value":failed}), false, vec![1]),
    ] {
        let component = array_insert_fixture(
            Value::Array(vec![Value::Integer(1)]),
            Value::Integer(7),
            None,
            false,
            single,
            extra,
        )
        .unwrap();
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(
            snapshot.variables[&100],
            Value::Array(expected.into_iter().map(Value::Integer).collect())
        );
        assert_eq!(snapshot.variables[&101], Value::Bool(true));
    }
}

#[test]
fn array_insert_work_depth_and_linked_storage_are_bounded() {
    let fixture = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_inPlug")],&[])}),
            ),
            node(
                1,
                "STU_4C3509FB",
                None,
                json!({"m_26E3B366":var(100,true),"m_value":var(102,true)}),
            ),
        ],
        json!([]),
        json!([0]),
    );
    let mut nested = Value::Integer(7);
    for _ in 0..MAX_PLUG_DEPTH {
        nested = Value::Array(vec![nested]);
    }
    assert!(matches!(
        Component::start(
            Arc::new(store(vec![fixture])),
            123,
            Variables::from([(102, nested)]),
            vec![Root::new(1, 42)],
            0,
            &mut EmptyWorld,
            MAX_INIT_WORK
        ),
        Err(InitFault::PlugDepth)
    ));
    let component = array_insert_fixture(
        Value::Integer(99),
        Value::Array(vec![Value::Integer(7); 200]),
        None,
        false,
        false,
        json!({}),
    )
    .unwrap();
    assert_eq!(
        component.snapshot().unwrap().variables[&100],
        Value::Array(vec![Value::Integer(7); 200])
    );
    let fixture = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_inPlug")],&[])}),
            ),
            node(
                1,
                "STU_4C3509FB",
                None,
                json!({"m_26E3B366":var(100,true),"m_value":var(102,true)}),
            ),
        ],
        json!([]),
        json!([0]),
    );
    assert!(matches!(
        Component::start(
            Arc::new(store(vec![fixture])),
            123,
            Variables::from([(102, Value::Array(vec![Value::Integer(7); 200]))]),
            vec![Root::new(1, 42)],
            0,
            &mut EmptyWorld,
            100
        ),
        Err(InitFault::WorkBudget { .. })
    ));
    let mut component = array_insert_fixture(
        Value::Integer(99),
        Value::Integer(7),
        None,
        false,
        false,
        json!({}),
    )
    .unwrap();
    let target = VarKey::Entity(100);
    component.machine.links.insert(
        target,
        vec![VariableLink {
            state: StateKey {
                instance: 1,
                raw: 0,
            },
            priority: StackPriority {
                level: 0,
                above_equals: true,
            },
            output: LinkValue::Fixed(Value::Integer(4)),
            stack: true,
        }],
    );
    component
        .run(&mut EmptyWorld, |vm| {
            let node = vm.graph(1).node(1).unwrap();
            assert!(!vm.insert_array(1, node, None, 0)?);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        component
            .snapshot()
            .unwrap()
            .diagnostics
            .last()
            .unwrap()
            .issue,
        Issue::UnsupportedField("array insertion linked variable checkpoint")
    );
}

#[test]
fn ordered_array_removal_keeps_order_and_ignores_missing_or_out_of_range_values() {
    for (values, index, expected) in [
        (
            Value::Array(vec![
                Value::Integer(1),
                Value::Integer(2),
                Value::Integer(3),
            ]),
            1,
            Value::Array(vec![Value::Integer(1), Value::Integer(3)]),
        ),
        (
            Value::Array(vec![Value::Integer(1)]),
            -1,
            Value::Array(vec![Value::Integer(1)]),
        ),
        (
            Value::Array(vec![Value::Integer(1)]),
            9,
            Value::Array(vec![Value::Integer(1)]),
        ),
        (Value::Null, 1, Value::Null),
    ] {
        let fixture = graph(
            42,
            vec![
                node(
                    0,
                    "STUStatescriptEntry",
                    None,
                    json!({"m_outPlug":plug(&[(1,"m_inPlug")],&[])}),
                ),
                node(
                    1,
                    "STU_DA54F8F7",
                    None,
                    json!({"m_26E3B366":var(100,true),"m_index":index,"m_outPlug":plug(&[(2,"m_inPlug")],&[])}),
                ),
                node(
                    2,
                    "STUStatescriptActionSetVar",
                    None,
                    json!({"m_out_Var":var(101,true),"m_value":true}),
                ),
            ],
            json!([]),
            json!([0]),
        );
        let component = Component::start(
            Arc::new(store(vec![fixture])),
            42,
            Variables::from([(100, values)]),
            vec![Root::new(1, 42)],
            0,
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap();
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(snapshot.variables[&100], expected);
        assert_eq!(snapshot.variables[&101], Value::Bool(true));
    }
}

#[test]
fn reused_play_script_does_not_replay_once_only_start_entries() {
    let callee = graph(
        43,
        vec![
            node(
                0,
                "STU_BB4AC733",
                None,
                json!({"m_outPlug":plug(&[(1,"m_inPlug")],&[])}),
            ),
            node(
                1,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var":var(900,true),"m_value":var(7,false)}),
            ),
            node(
                2,
                "STU_BB4AC733",
                None,
                json!({"m_FDBDCB70":1,"m_outPlug":plug(&[(3,"m_inPlug")],&[])}),
            ),
            node(
                3,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var":var(901,true),"m_value":var(7,false)}),
            ),
        ],
        json!([]),
        json!([0, 2]),
    );
    let mut component = Component::start(
        Arc::new(store(vec![callee])),
        0xa000_0101,
        Variables::new(),
        vec![],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    let site = Site {
        entity: 0xa000_0201,
        instance: 1,
        graph: 42,
        node: None,
    };
    for (policy, value, once) in [(1, 40, 40), (2, 99, 40), (3, 100, 100)] {
        component
            .receive_play_script(
                site,
                &PlayScript {
                    policy,
                    overrides: vec![(7, Value::Integer(value))],
                    graph: 43,
                    source: site.entity,
                    parameters: vec![],
                    auto_retire: false,
                },
                &mut EmptyWorld,
            )
            .unwrap();
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(snapshot.variables[&900], Value::Integer(value));
        assert_eq!(snapshot.variables[&901], Value::Integer(once));
        assert_eq!(component.machine.disabled_start_entries.len(), 1);
    }
}

#[test]
fn play_script_binds_named_arrays_and_retires_only_after_pending_work() {
    const OWNER: u32 = 0xc000_1000;
    const SOURCE: u32 = 0xc000_1001;
    const REPLAY: u64 = 0x0240_0000_0000_0009;
    let caller = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(2,"m_inPlug")], &[])}),
            ),
            node(
                1,
                "STUStatescriptEntryGameMessage",
                None,
                json!({"m_gameMessage":{"m_gameMessage":"0x0240000000000009"},
                       "m_outPlug":plug(&[(2,"m_inPlug")], &[])}),
            ),
            node(
                2,
                "STUStatescriptActionPlayScript",
                None,
                json!({
                    "m_script":{"$":"STU_105E1BCC","m_graph":"0x058000000000002B","m_resourceKey":null},
                    "m_69F4B52C":{"$":"STU_9DB46B98","m_6525E98B":null},
                    "m_targets":{"$":"STU_B5A0CAF0"},
                    "m_7ABA409C":0,
                    "m_params":[{"m_B5051BCE":"0x0A400000000003A1","m_value":var(37,false)}],
                }),
            ),
        ],
        json!([]),
        json!([0, 1]),
    );
    let callee = graph(
        43,
        vec![
            node(
                0,
                "STU_BB4AC733",
                None,
                json!({
                    "m_B60ABDA4":var(28,false),
                    "m_params":[{"m_B5051BCE":"0x0A400000000003A1","m_out_Var":var(37,false)}],
                    "m_outPlug":plug(&[(1,"m_inPlug"),(2,"m_beginPlug")], &[]),
                }),
            ),
            node(
                1,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var":var(38,false),"m_value":var(37,false)}),
            ),
            node(
                2,
                "STUStatescriptStateWait",
                Some(0),
                json!({"m_timeout":0.05}),
            ),
            node(
                3,
                "STU_BB4AC733",
                None,
                json!({"m_5AEA0A51":1,
                       "m_outPlug":plug(&[(4,"m_inPlug")], &[])}),
            ),
            node(
                4,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var":var(99,false),"m_value":true}),
            ),
        ],
        json!([2]),
        json!([0, 3]),
    );
    let values = Value::Array(vec![Value::Vector([1.0, 2.0, 3.0]), Value::Integer(4)]);
    let mut root = Root::new(1, 42);
    root.variables.insert(SCRIPT_SOURCE, Value::Entity(SOURCE));
    root.variables.insert(37, values.clone());
    let mut component = Component::start(
        Arc::new(store(vec![caller, callee])),
        OWNER,
        Variables::new(),
        vec![root],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    let played = &snapshot.instances[&2];
    assert_eq!(played.parent, None);
    assert_eq!(played.variables[&SCRIPT_SOURCE], Value::Entity(SOURCE));
    assert_eq!(played.variables[&28], Value::Entity(OWNER));
    assert_eq!(played.variables[&38], values);
    assert!(!played.variables.contains_key(&99));
    component
        .queue_message(1, REPLAY, Variables::new())
        .unwrap();
    component.run_now(&mut EmptyWorld).unwrap();
    assert_eq!(component.snapshot().unwrap().instances.len(), 2);
    component.advance_frame(2, &mut EmptyWorld).unwrap();
    assert!(component.snapshot().unwrap().instances.contains_key(&2));
    component.advance_frame(4, &mut EmptyWorld).unwrap();
    assert!(!component.snapshot().unwrap().instances.contains_key(&2));
    component
        .queue_message(1, REPLAY, Variables::new())
        .unwrap();
    component.run_now(&mut EmptyWorld).unwrap();
    assert!(component.snapshot().unwrap().instances.contains_key(&3));
}

#[test]
#[ignore = "requires the prepared 104319 Junkenstein PlayScript callers"]
fn authored_boss_play_script_requests_resolve_the_controller_and_body() {
    const MODE: u32 = 0xd000_0010;
    const BODY: u32 = 0xc000_1000;
    const CONTROLLER: u32 = BODY + 1;
    #[derive(Default)]
    struct Pair(Vec<(Site, u32, PlayScript)>);
    impl World for Pair {
        fn related_entity(
            &mut self,
            _site: Site,
            relation: EntityRelation,
            entity: u32,
        ) -> Option<u32> {
            match (relation, entity) {
                (EntityRelation::CurrentPossessor, BODY) => Some(CONTROLLER),
                (EntityRelation::PossessedBody, CONTROLLER) => Some(BODY),
                _ => None,
            }
        }
        fn play_script(&mut self, site: Site, target: u32, request: PlayScript) -> bool {
            self.0.push((site, target, request));
            true
        }
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }
    }
    let runtime = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let graphs = Arc::new(GraphStore::load(runtime.join("data/game_graphs_174.json.gz")).unwrap());
    let position = Value::Vector([71.5, 18.5, -105.0]);
    let mut world = Pair::default();
    for (graph, owner, call, variables) in [
        (
            0x0cba,
            MODE,
            9,
            Variables::from([
                (38, Value::Entity(BODY)),
                (889, Value::Integer(0)),
                (8370, position.clone()),
            ]),
        ),
        (
            0x0c95,
            CONTROLLER,
            2,
            Variables::from([(SCRIPT_SOURCE, Value::Entity(MODE))]),
        ),
    ] {
        let mut root = Root::listener(1, graph, None);
        root.variables = variables;
        let mut component = Component::start(
            Arc::clone(&graphs),
            owner,
            Variables::new(),
            vec![root],
            0,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        component
            .run(&mut world, |vm| {
                let node = vm.graph(1).node(call).unwrap();
                assert!(vm.run_node(1, node, None, 0)?);
                vm.run()
            })
            .unwrap();
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        if graph == 0x0cba {
            assert_eq!(
                snapshot.instances[&1].variables[&8399],
                Value::Array(vec![Value::Entity(BODY)])
            );
        }
    }
    assert_eq!(world.0.len(), 2);
    assert_eq!(
        world.0[0],
        (
            Site {
                entity: MODE,
                instance: 1,
                graph: 0x0cba,
                node: Some(9)
            },
            CONTROLLER,
            PlayScript {
                policy: 0,
                overrides: Vec::new(),
                graph: 0x0c95,
                source: MODE,
                parameters: vec![(0x0a40_0000_0000_03a1, Value::Array(vec![position]))],
                auto_retire: true,
            }
        )
    );
    assert_eq!(
        world.0[1],
        (
            Site {
                entity: CONTROLLER,
                instance: 1,
                graph: 0x0c95,
                node: Some(2)
            },
            BODY,
            PlayScript {
                policy: 0,
                overrides: Vec::new(),
                graph: 0x0cbc,
                source: MODE,
                parameters: vec![],
                auto_retire: true,
            }
        )
    );
}

#[test]
#[ignore = "requires the prepared 104319 Junkenstein wave body graph"]
fn authored_boss_team_contact_releases_on_end_and_instance_retirement() {
    #[derive(Default)]
    struct Contacts(Vec<(Site, bool)>);
    impl World for Contacts {
        fn set_same_team_contact_suppressed(&mut self, site: Site, suppressed: bool) -> bool {
            self.0.push((site, suppressed));
            true
        }
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }
    }
    let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let graphs = Arc::new(GraphStore::load(runtime.join("data/game_graphs_174.json.gz")).unwrap());
    let body = 0xc000_1000;
    let mut world = Contacts::default();
    let mut component = Component::start(
        graphs,
        body,
        Variables::new(),
        vec![Root::listener(1, 0x0cbc, Some(52))],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    let key = StateKey {
        instance: 1,
        raw: 52,
    };
    let site = Site {
        entity: body,
        instance: 1,
        graph: 0x0cbc,
        node: Some(129),
    };
    assert_eq!(world.0, [(site, true)]);
    component
        .run(&mut world, |vm| vm.end(key, false, 0))
        .unwrap();
    assert_eq!(world.0, [(site, true), (site, false)]);
    assert_eq!(
        component.snapshot().unwrap().instances[&1].states[&52].kind,
        StateKind::SameTeamContact { registered: false }
    );
    component.run(&mut world, |vm| vm.begin(key, 0)).unwrap();
    component.run(&mut world, |vm| vm.exit_instance(1)).unwrap();
    assert_eq!(
        world.0,
        [(site, true), (site, false), (site, true), (site, false)]
    );
    assert!(component.snapshot().unwrap().initialization_complete());
}

#[test]
#[ignore = "requires the prepared 104319 Junkenstein boss intro graphs"]
fn authored_boss_facing_polls_and_retires_with_its_state() {
    const BODY: u32 = 0xc000_1000;
    const ANCHOR: u32 = 0x8000_0001;
    struct Facing {
        forward: [f64; 3],
        updates: Vec<(AiFacing, bool)>,
    }
    impl World for Facing {
        fn related_entity(&mut self, _: Site, relation: EntityRelation, owner: u32) -> Option<u32> {
            (relation == EntityRelation::PossessedBody && owner == BODY + 1).then_some(BODY)
        }
        fn forward(&self, entity: u32) -> Option<[f64; 3]> {
            (entity == ANCHOR).then_some(self.forward)
        }
        fn ai_facing(&mut self, _: Site, record: AiFacing, active: bool) -> Option<bool> {
            assert_eq!(record.target, BODY);
            self.updates.push((record, active));
            Some(true)
        }
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }
    }
    let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let graphs = Arc::new(GraphStore::load(runtime.join("data/game_graphs_174.json.gz")).unwrap());
    let mut world = Facing {
        forward: [0.0, 0.0, 1.0],
        updates: Vec::new(),
    };
    let mut root = Root::listener(1, 0x0c95, Some(2));
    root.variables
        .insert(8370, Value::Array(vec![Value::Entity(ANCHOR)]));
    let mut component = Component::start(
        graphs,
        BODY + 1,
        Variables::new(),
        vec![root],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert_eq!(world.updates[0].0.direction[2], -1.0);
    world.forward = [1.0, 0.0, 0.0];
    component.advance_frame(1, &mut world).unwrap();
    let (current, active) = world.updates[1];
    assert!(active);
    assert!((current.direction[0] + 1.0).abs() < 1e-6);
    let key = StateKey {
        instance: 1,
        raw: 2,
    };
    component
        .run(&mut world, |vm| vm.end(key, false, 0))
        .unwrap();
    assert_eq!(world.updates[2], (current, false));
    component.run(&mut world, |vm| vm.begin(key, 0)).unwrap();
    assert_eq!(world.updates[3], (current, true));
    component.run(&mut world, |vm| vm.exit_instance(1)).unwrap();
    assert_eq!(world.updates[4], (current, false));
    assert!(component.snapshot().unwrap().initialization_complete());
}
