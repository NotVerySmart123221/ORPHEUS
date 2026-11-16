use super::*;

pub(super) fn shared_voice_lifecycle_contract() {
    use crate::script_voice::Stimulus;
    fn float(json: &Json) -> Option<f64> {
        json.as_f64().or_else(|| match json["float"].as_str()? {
            "NaN" => Some(f64::NAN),
            "Infinity" => Some(f64::INFINITY),
            "-Infinity" => Some(f64::NEG_INFINITY),
            _ => None,
        })
    }
    fn bits(value: f64) -> Json {
        json!({"bits64":format!("{:016x}",value.to_bits())})
    }
    fn value(json: &Json) -> Value {
        match json {
            Json::Null => Value::Null,
            Json::Bool(v) => Value::Bool(*v),
            Json::Number(n) => n
                .as_i64()
                .map_or_else(|| Value::Float(n.as_f64().unwrap()), Value::Integer),
            Json::Array(items) => Value::Array(items.iter().map(value).collect()),
            Json::Object(fields) if fields.contains_key("asset") => Value::Asset(
                u64::from_str_radix(json["asset"].as_str().unwrap().trim_start_matches("0x"), 16)
                    .unwrap(),
            ),
            Json::Object(_) => Value::Entity(json["entity"].as_u64().unwrap().try_into().unwrap()),
            _ => panic!("invalid voice fixture value"),
        }
    }
    struct Host {
        case: Json,
        events: Vec<Json>,
        clock: Option<f64>,
        indices: BTreeMap<String, usize>,
    }
    impl Host {
        fn selected(&mut self, key: &str, sequence: Json) -> Json {
            let sequence = sequence.as_array().unwrap();
            let index = self.indices.entry(key.into()).or_default();
            let result = sequence[(*index).min(sequence.len() - 1)].clone();
            *index += 1;
            result
        }
        fn availability(&self, key: &str) -> Option<bool> {
            match self.case[key].as_str().unwrap_or("present") {
                "present" => Some(true),
                "absent" => Some(false),
                "unavailable" => None,
                _ => panic!("invalid host availability"),
            }
        }
    }
    impl World for Host {
        fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
            None
        }
        fn position(&mut self, _: u32) -> Option<[f64; 3]> {
            None
        }
        fn voice_context(&mut self, _: Site) -> Option<bool> {
            self.events.push(json!(["context"]));
            self.availability("context")
        }
        fn voice_manager(&mut self, _: Site) -> Option<bool> {
            self.events.push(json!(["manager"]));
            self.availability("manager")
        }
        fn voice_clock(&mut self, _: Site, _: u64) -> Option<f64> {
            self.events.push(json!(["clock"]));
            self.clock
        }
        fn voice_stimulus(&mut self, _: Site, guid: u64) -> Option<Option<Stimulus>> {
            self.events.push(json!(["load", format!("0x{guid:016X}")]));
            let sequence = self
                .case
                .get("resource")
                .cloned()
                .unwrap_or(json!(["present"]));
            match self.selected("resource", sequence).as_str().unwrap() {
                "present" => Some(Some(Stimulus {
                    guid,
                    playable: self.case["playable"].as_bool().unwrap_or(true),
                    mode: self.case["mode"].as_bool().unwrap_or(false),
                })),
                "absent" => Some(None),
                "unavailable" => None,
                _ => panic!("invalid resource availability"),
            }
        }
        fn voice_allow(&mut self, _: Site, stimulus: Stimulus) -> Option<bool> {
            self.events.push(json!(["allow", stimulus.mode]));
            Some(self.case["allow"].as_bool().unwrap_or(true))
        }
        fn voice_submit(
            &mut self,
            _: Site,
            _: Stimulus,
            targets: &[u32],
            _: Option<&Value>,
        ) -> Result<Option<u32>, &'static str> {
            self.events.push(json!(["submit", targets]));
            Ok(Some(
                self.case
                    .get("handle")
                    .map_or(17, |v| v.as_u64().unwrap().try_into().unwrap()),
            ))
        }
        fn voice_duration(&mut self, _: Site, handle: u32, _: u64, mode: bool) -> Option<f64> {
            self.events.push(json!(["duration", handle, mode]));
            let sequence = self.case.get("duration").cloned().unwrap_or(json!([0.25]));
            float(&self.selected("duration", sequence))
        }
        fn voice_part(&mut self, _: Site, number: u16) -> Option<f64> {
            self.events.push(json!(["part", number]));
            let sequence = self.case["parts"]
                .get(number.to_string())
                .cloned()
                .unwrap_or(json!([0.0]));
            float(&self.selected(&format!("part-{number}"), sequence))
        }
    }
    fn observation(component: &Component, host: &mut Host, outcome: Json) -> Json {
        let instance = component.machine.snapshot.instances.get(&1);
        let state = instance.and_then(|instance| instance.states.get(&0));
        let voice=state.and_then(|state|state.voice.as_deref()).map(|voice|json!({
            "handle":voice.handle,"attempts":voice.attempts,"duration":bits(voice.duration),"part":voice.part,
            "started_at":bits(voice.started_at),"delay":bits(voice.delay),"cooldown":bits(voice.cooldown),
            "retry_at":bits(voice.retry_at),"submitted":voice.submitted,"failed":voice.failed,
        }));
        let values = instance.map(|instance| {
            [79, 80, 81, 82, 83]
                .into_iter()
                .map(|key| {
                    let value = match instance.variables.get(&key) {
                        Some(Value::Integer(v)) => json!(v),
                        Some(Value::Float(v)) => bits(*v),
                        None | Some(Value::Null) => Json::Null,
                        other => panic!("unexpected voice output {other:?}"),
                    };
                    (key.to_string(), value)
                })
                .collect::<serde_json::Map<_, _>>()
        });
        json!({"events":std::mem::take(&mut host.events),"exists":instance.is_some(),"active":state.map(|state|state.active),
            "registered":component.machine.voice_poll.len(),"voice":voice,"values":values,"outcome":outcome})
    }
    let contract: Json = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/statescript-voice-lifecycle-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "statescript.voice-lifecycle.v1");
    assert_eq!(contract["target_build"], 104319);
    let cases = contract["input"]["cases"].as_array().unwrap();
    let expected = contract["expected"]["results"].as_array().unwrap();
    assert_eq!(cases.len(), expected.len());
    for (case, expected) in cases.iter().zip(expected) {
        assert_eq!(case["name"], expected["name"]);
        let mut graph = contract["input"]["graph"].clone();
        if let Some(fields) = case["fields"].as_object() {
            graph["nodes"][0]["fields"]
                .as_object_mut()
                .unwrap()
                .extend(fields.clone());
        }
        let index = graph["index"].as_u64().unwrap().try_into().unwrap();
        let graphs = Arc::new(store(vec![graph]));
        let owner = contract["input"]["owner"]
            .as_u64()
            .unwrap()
            .try_into()
            .unwrap();
        let mut root = Root::listener(1, index, Some(0));
        for key in [80, 81, 82, 83] {
            root.variables.insert(key, Value::Integer(0));
        }
        root.variables.insert(
            1,
            value(case.get("targets").unwrap_or(&json!([{"entity":owner}]))),
        );
        if let Some(variables) = case["variables"].as_object() {
            for (key, selected) in variables {
                root.variables.insert(key.parse().unwrap(), value(selected));
            }
        }
        let mut host = Host {
            case: case.clone(),
            events: Vec::new(),
            clock: case.get("clock").map_or(Some(100.0), float),
            indices: BTreeMap::new(),
        };
        let mut component = None;
        let mut root = Some(root);
        let mut actual = Vec::new();
        for step in case["steps"].as_array().unwrap() {
            if let Some(clock) = step.get("clock") {
                host.clock = float(clock);
            }
            let mut outcome = Json::Null;
            let key = StateKey {
                instance: 1,
                raw: 0,
            };
            let op = step["op"].as_str().unwrap();
            if op == "begin" && component.is_none() {
                component = Some(
                    Component::start(
                        Arc::clone(&graphs),
                        owner,
                        Variables::new(),
                        vec![root.take().unwrap()],
                        0,
                        &mut host,
                        MAX_INIT_WORK,
                    )
                    .unwrap(),
                );
            } else {
                let component = component.as_mut().unwrap();
                match op {
                    "begin" => component.run(&mut host, |vm| vm.begin(key, 0)).unwrap(),
                    "advance" => {
                        let frame = step["frame"].as_u64().unwrap().try_into().unwrap();
                        component.advance_frame(frame, &mut host).unwrap();
                    }
                    "drain" => component
                        .run(&mut host, |vm| {
                            vm.update()?;
                            vm.notify_code(1)
                        })
                        .unwrap(),
                    "poll" => component.poll_voice(&mut host).unwrap(),
                    "failure" => {
                        outcome = json!(component.notify_voice_failure(1, 0, &mut host).unwrap())
                    }
                    "abort" | "finish" => component
                        .run(&mut host, |vm| vm.end(key, op == "finish", 0))
                        .unwrap(),
                    "destroy" => component.run(&mut host, |vm| vm.destroy(1)).unwrap(),
                    "set_output" => component
                        .run(&mut host, |vm| {
                            vm.write(
                                VarKey::Instance(1, 79),
                                Value::Float(step["value"].as_f64().unwrap()),
                            )
                        })
                        .unwrap(),
                    "timer" => component
                        .run(&mut host, |vm| {
                            vm.timer(key, step["parameter"].as_u64().unwrap().try_into().unwrap())
                        })
                        .unwrap(),
                    _ => panic!("unknown voice operation {op}"),
                }
            }
            let component = component.as_ref().unwrap();
            if let Some(diagnostic) = component.machine.snapshot.diagnostics.last() {
                let (feature, class) = match &diagnostic.issue {
                    Issue::UnsupportedField(feature) => (*feature, "STU_40E05F6B"),
                    Issue::Expression(EvalError::UnsupportedConfig(class)) => {
                        ("world query", class.as_str())
                    }
                    _ => panic!(
                        "{}: unexpected voice diagnostic {diagnostic:?}",
                        case["name"]
                    ),
                };
                actual.push(json!({"error":feature,"class":class,"events":std::mem::take(&mut host.events)}));
                break;
            }
            actual.push(observation(component, &mut host, outcome));
        }
        let expected = expected["steps"].as_array().unwrap();
        assert_eq!(actual.len(), expected.len(), "{}", case["name"]);
        for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
            assert_eq!(actual, expected, "{} step {index}", case["name"]);
        }
    }
}

pub(super) fn shared_nested_destination_contract() {
    use crate::script_destination::{Step, assign};
    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/nested-destination-v1.json"
    ))
    .unwrap();
    assert_eq!(fixture["case_id"], "statescript.nested-destination.v1");
    for case in fixture["storage_cases"].as_array().unwrap() {
        let mut root = match case["initial_type"].as_u64().unwrap() {
            255 => Value::Null,
            2 => Value::Integer(19),
            3 => Value::Float(f64::from(f32::from_bits(19))),
            _ => unreachable!(),
        };
        let index = case["index"].as_u64().unwrap() as u16;
        let result = assign(
            &mut root,
            &[Step::Index(index), Step::Field(27606)],
            Value::Integer(123),
            &mut 0,
        )
        .unwrap();
        assert_eq!(result, case["result"].as_bool().unwrap());
        let Value::Array(items) = root else {
            panic!("array coercion")
        };
        assert_eq!(items.len(), case["array_count"].as_u64().unwrap() as usize);
        for (position, item) in items.iter().enumerate() {
            if position == usize::from(index) {
                let Value::IdentifierMap(map) = item else {
                    panic!("field coercion")
                };
                assert_eq!(map.capacity(), case["map_width"].as_u64().unwrap() as usize);
                assert_eq!(
                    map.pairs(),
                    vec![(Value::Integer(27606), Value::Integer(123))]
                );
            } else {
                assert_eq!(item, &Value::Integer(0));
            }
        }
    }
    for sequence in fixture["map_sequences"].as_array().unwrap() {
        let mut root = Value::Null;
        for step in sequence["steps"].as_array().unwrap() {
            let key = step["key"].as_u64().unwrap() as u16;
            let value = step["value"].as_i64().unwrap();
            assert!(
                assign(
                    &mut root,
                    &[Step::Field(key)],
                    Value::Integer(value),
                    &mut 0
                )
                .unwrap()
            );
            let Value::IdentifierMap(map) = &root else {
                unreachable!()
            };
            assert_eq!(map.capacity(), step["capacity"].as_u64().unwrap() as usize);
            let expected: Vec<_> = step["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|pair| {
                    (
                        Value::Integer(pair[0].as_i64().unwrap()),
                        Value::Integer(pair[1].as_i64().unwrap()),
                    )
                })
                .collect();
            assert_eq!(map.pairs(), expected, "native bucket order");
            let mut native = crate::bits::BitWriter::new();
            let mut logical = crate::bits::BitWriter::new();
            crate::script_wire::write_value(
                &mut native,
                &root,
                crate::script_wire::ValueEncoding::Compact,
            )
            .unwrap();
            crate::script_wire::write_value(
                &mut logical,
                &Value::Map(expected),
                crate::script_wire::ValueEncoding::Compact,
            )
            .unwrap();
            assert_eq!(native, logical, "identifier-map wire representation");
        }
    }
    for case in fixture["path_cases"].as_array().unwrap() {
        let entity = case["scope"] == 1;
        let index = case["index"].as_f64().unwrap();
        let output = json!({"$":"STU_919FD47C","m_expression":{
            "m_opcodes":[8,0,8,1,17,7,2,15,0], "m_D99EF254":[],
            "m_dynamicVars":[var(31528,entity),var(39,false),var(27606,false)]}});
        let mut declared = output.clone();
        declared["m_expression"]["m_opcodes"] = fixture["path_opcodes"].clone();
        declared["m_expression"]["m_dynamicVars"] = json!([var(31528, entity), var(27606, false)]);
        declared["m_expression"]["m_D99EF254"] = json!([index]);
        let parsed = script_expr::destination(&declared).unwrap().unwrap();
        assert_eq!(
            parsed.variable.id,
            case["variable"].as_u64().unwrap() as u32
        );
        assert_eq!(
            parsed.variable.scope,
            if entity {
                script_expr::Scope::Entity
            } else {
                script_expr::Scope::Instance
            }
        );
        let mut steps = Vec::new();
        for part in parsed.path {
            steps.push(match part {
                script_expr::DestinationPart::Index(config) => {
                    let value =
                        script_expr::evaluate(&config, &mut crate::script_expr::EmptyContext)
                            .unwrap();
                    json!([script_expr::to_int(&value, 0) as u16, 0])
                }
                script_expr::DestinationPart::Field(field) => json!([0, field]),
            });
        }
        assert_eq!(json!(steps), case["path"]);
        // The actual VM path uses a live local index instead of the
        // constant index used by the restricted native path oracle.
        let graphs = Arc::new(store(vec![graph(
            42,
            vec![
                node(
                    0,
                    "STUStatescriptEntry",
                    None,
                    json!({"m_outPlug":plug(&[(1,"m_inPlug")], &[])}),
                ),
                node(
                    1,
                    "STU_12F5D52D",
                    None,
                    json!({"m_143D4C5B":[
                {"m_out_Var":var(39,false),"m_value":{"$":"STUConfigVarFloat","m_value":index}},
                {"m_out_Var":output,"m_value":{"$":"STUConfigVarInt","m_value":123}}
            ],"m_outPlug":plug(&[(2,"m_inPlug")], &[])}),
                ),
                node(
                    2,
                    "STUStatescriptActionSetVar",
                    None,
                    json!({"m_out_Var":var(900,true),"m_value":output}),
                ),
            ],
            json!([]),
            json!([0]),
        )]));
        let component = Component::start(
            graphs,
            42,
            Variables::new(),
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
        let index = case["path"][0][0].as_u64().unwrap();
        assert_eq!(
            snapshot.variables[&900],
            if index < 1000 {
                Value::Integer(123)
            } else {
                Value::Null
            }
        );
    }
}

pub(super) fn nested_destination_writes_notify_existing_readers() {
    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/nested-destination-v1.json"
    ))
    .unwrap();
    for entity in [false, true] {
        let output = json!({"$":"STU_919FD47C","m_expression":{
            "m_opcodes":[8,0,8,1,17,7,2,15,0],"m_D99EF254":[],
            "m_dynamicVars":[var(31528,entity),var(39,false),var(27606,false)]}});
        let graphs = Arc::new(store(vec![graph(
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
                    json!({"m_condition":output}),
                ),
                node(
                    2,
                    "STUStatescriptEntryGameMessage",
                    None,
                    json!({
                "m_gameMessage":{"$":"STUConfigVarGameMessage","m_gameMessage":"0x024000000000000F"},
                "m_outPlug":plug(&[(3,"m_inPlug")],&[])}),
                ),
                node(
                    3,
                    "STUStatescriptActionSetVar",
                    None,
                    json!({"m_out_Var":output,"m_value":var(900,true)}),
                ),
            ],
            json!([1]),
            json!([0, 2]),
        )]));
        let mut component = Component::start(
            graphs,
            42,
            Variables::new(),
            vec![Root::new(1, 42)],
            0,
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap();
        assert_eq!(
            component.snapshot().unwrap().instances[&1].states[&0].kind,
            StateKind::BooleanSwitch(false)
        );
        for value in fixture["host_watcher_values"].as_array().unwrap() {
            let value = value.as_i64().unwrap();
            component
                .set_entity_variable(900, Value::Integer(value), &mut EmptyWorld)
                .unwrap();
            assert_eq!(
                component
                    .queue_entity_message(0x0240_0000_0000_000f, Variables::new())
                    .unwrap(),
                1
            );
            component.run_now(&mut EmptyWorld).unwrap();
            let snapshot = component.snapshot().unwrap();
            assert!(
                snapshot.initialization_complete(),
                "{:?}",
                snapshot.diagnostics
            );
            assert_eq!(
                snapshot.instances[&1].states[&0].kind,
                StateKind::BooleanSwitch(value != 0)
            );
        }
    }
}

pub(super) fn nested_destination_keeps_unavailable_storage_explicit() {
    let output = json!({"$":"STU_919FD47C","m_expression":{
        "m_opcodes":[8,0,6,0,17,7,1,15,0],"m_D99EF254":[0],
        "m_dynamicVars":[var(31528,false),var(27606,false)]}});
    let graphs = Arc::new(store(vec![graph(
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
                "STUStatescriptStateStack",
                Some(0),
                json!({
            "m_out_Var":var(31528,false),"m_value":{"$":"STUConfigVarInt","m_value":19},
            "m_priority":{"$":"STUConfigVarFloat","m_value":0},
            "m_onBeginPlug":plug(&[(2,"m_inPlug")],&[])}),
            ),
            node(
                2,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var":output,"m_value":{"$":"STUConfigVarInt","m_value":123}}),
            ),
        ],
        json!([1]),
        json!([0]),
    )]));
    let component = Component::start(
        graphs,
        42,
        Variables::new(),
        vec![Root::new(1, 42)],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue
                == Issue::UnsupportedField("nested destination linked storage"))
    );
    assert_eq!(
        component.resolved_variable(
            1,
            script_expr::Variable {
                scope: script_expr::Scope::Instance,
                id: 31528
            }
        ),
        Some(Value::Integer(19))
    );
    let mut legacy = Value::Map(vec![(Value::Integer(1), Value::Integer(9))]);
    let before = legacy.clone();
    assert!(
        crate::script_destination::assign(
            &mut legacy,
            &[crate::script_destination::Step::Field(2)],
            Value::Integer(123),
            &mut 0
        )
        .is_err()
    );
    assert_eq!(legacy, before);
}

#[test]
fn target_tag_snapshot_preserves_unknown_base_and_effect_lifetimes() {
    let tag = 0x0d60_0000_0000_0001;
    let mut effects = TargetTagEffects::default();
    let grant = TargetTagEffect {
        entity: 123,
        tag,
        present: true,
    };
    let suppress = TargetTagEffect {
        present: false,
        ..grant
    };
    let base = std::collections::BTreeSet::from([tag]);
    assert_eq!(effects.snapshot(None).presence(tag), None);
    assert_eq!(effects.snapshot(Some(&base)).presence(tag), Some(true));
    assert!(effects.apply(grant, true));
    let granted = effects.snapshot(None);
    assert!(effects.apply(suppress, true));
    assert_eq!(effects.snapshot(Some(&base)).presence(tag), Some(false));
    assert_eq!(granted.presence(tag), Some(true));
    assert!(!effects.apply(grant, true));
    assert!(effects.apply(suppress, false));
    assert_eq!(effects.snapshot(None).presence(tag), Some(true));
    assert!(effects.apply(grant, false));
    assert_eq!(effects.snapshot(None).presence(tag), None);
    assert_eq!(effects.snapshot(Some(&base)).presence(tag + 1), Some(false));
}

#[test]
fn movement_modifier_values_follow_lifecycle_and_dependencies_without_read_side_effects() {
    for per_frame in [false, true] {
        let graphs = Arc::new(store(vec![graph(
            1,
            vec![
                node(
                    0,
                    "STUStatescriptEntry",
                    None,
                    json!({"m_outPlug":plug(&[(1,"m_beginPlug")],&[])}),
                ),
                node(
                    1,
                    "STU_316CFEF2",
                    Some(0),
                    json!({
                    "m_485109CB":per_frame,
                    "m_BC5E91CF":[
                        {"$":"STU_D8CC6B10","m_A9561CA0":0,"m_9B7A63EA":var(77,true)},
                        {"$":"STU_D8CC6B10","m_A9561CA0":1,"m_9B7A63EA":
                            {"$":"STU_BE0BD267","m_min":0.8,"m_max":1.2}}
                    ]}),
                ),
                node(
                    2,
                    "STUStatescriptEntryGameMessage",
                    None,
                    json!({
                "m_gameMessage":{"m_gameMessage":"0x0240000000000002"},
                "m_outPlug":plug(&[(1,ABORT_PLUG)],&[])}),
                ),
                node(
                    3,
                    "STUStatescriptEntryGameMessage",
                    None,
                    json!({
                "m_gameMessage":{"m_gameMessage":"0x0240000000000003"},
                "m_outPlug":plug(&[(1,"m_beginPlug")],&[])}),
                ),
            ],
            json!([1]),
            json!([0, 2, 3]),
        )]));
        let mut component = Component::start(
            graphs,
            42,
            Variables::from([(77, Value::Float(2.0))]),
            vec![Root::new(1, 1)],
            0,
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap();
        let read = |component: &mut Component| {
            let values = component
                .movement_modifiers(&mut EmptyWorld)
                .unwrap()
                .unwrap();
            assert_eq!(values.len(), 2);
            let row = |slot| {
                values[&ModifierKey {
                    instance: 1,
                    state: 0,
                    slot,
                }]
                    .value
            };
            (row(0), row(1))
        };
        let initial = read(&mut component);
        assert_eq!(initial.0, 2.0);
        assert_eq!(read(&mut component), initial);
        component.advance_frame(1, &mut EmptyWorld).unwrap();
        let next = read(&mut component);
        assert_eq!(next.0, 2.0);
        assert_eq!(next.1 == initial.1, !per_frame);
        component
            .set_entity_variable(77, Value::Float(3.0), &mut EmptyWorld)
            .unwrap();
        component.run_now(&mut EmptyWorld).unwrap();
        let changed = read(&mut component);
        assert_eq!(changed.0, 3.0);
        assert_ne!(changed.1, next.1);
        assert_eq!(read(&mut component), changed);
        component
            .queue_message(1, 0x0240_0000_0000_0002, Variables::new())
            .unwrap();
        component.run_now(&mut EmptyWorld).unwrap();
        assert!(
            component
                .movement_modifiers(&mut EmptyWorld)
                .unwrap()
                .unwrap()
                .is_empty()
        );
        component
            .queue_message(1, 0x0240_0000_0000_0003, Variables::new())
            .unwrap();
        component.run_now(&mut EmptyWorld).unwrap();
        let restarted = read(&mut component);
        assert_eq!(restarted.0, 3.0);
        assert_ne!(restarted.1, changed.1);
    }
}

#[test]
fn authored_encounter_cleanup_damages_only_a_living_owner_once() {
    #[derive(Default)]
    struct Damage(Vec<f64>);
    impl World for Damage {
        fn self_damage(&mut self, site: Site, amount: f64) -> bool {
            assert_eq!((site.entity, site.graph, site.node), (42, 0x0cbc, Some(69)));
            self.0.push(amount);
            true
        }
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }
    }

    let owner = json!({"$":"STU_B5A0CAF0"});
    let mut authored = graph(
        0x0cbc,
        vec![node(
            69,
            "STUStatescriptStateModifyHealth",
            Some(0),
            json!({
                "m_FA3ECC43":owner,
                "m_8B707686":1,
                "m_modifyHealth":{
                    "m_724CBA57":owner,
                    "m_93CDFD2F":1,
                    "m_amount":{"$":"STUConfigVarInt","m_value":10000}
                }
            }),
        )],
        json!([69]),
        json!([]),
    );
    authored["items"] = json!(70);
    authored["header"]["m_nodesBitCount"] = json!(7);
    let graphs = Arc::new(store(vec![authored]));
    for already_dead in [false, true] {
        let mut world = Damage::default();
        let mut component = Component::start(
            Arc::clone(&graphs),
            42,
            Variables::new(),
            vec![Root::listener(1, 0x0cbc, None)],
            0,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        component.set_owner_dead(already_dead);
        let key = StateKey {
            instance: 1,
            raw: 0,
        };
        component.run(&mut world, |vm| vm.begin(key, 0)).unwrap();
        for frame in 1..=4 {
            component.advance_frame(frame, &mut world).unwrap();
        }
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(world.0, if already_dead { vec![] } else { vec![10_000.0] });
    }
}

pub(super) fn shared_foreign_notification_contract() {
    struct Host {
        target: u32,
        value: bool,
    }
    impl World for Host {
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
        fn entity_variable(&mut self, entity: u32, id: u32) -> Option<Value> {
            (entity == self.target && id == 6604).then_some(Value::Bool(self.value))
        }
    }
    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/statescript-foreign-notification-v1.json"
    ))
    .unwrap();
    assert_eq!(fixture["case_id"], "statescript.foreign-notification.v1");
    assert_eq!(fixture["schema_version"], 1);
    assert_eq!(fixture["target_build"], 104319);
    let cases = fixture["input"]["cases"].as_array().unwrap();
    let results = fixture["expected"]["results"].as_array().unwrap();
    assert_eq!(cases.len(), results.len());
    for (case, expected) in cases.iter().zip(results) {
        assert_eq!(case["name"], expected["name"]);
        let owner = u32::try_from(fixture["input"]["owner"].as_u64().unwrap()).unwrap();
        let target = u32::try_from(fixture["input"]["target"].as_u64().unwrap()).unwrap();
        let mut world = Host {
            target,
            value: case["initial"].as_bool().unwrap(),
        };
        let mut root = Root::new(1, 0x0c8c);
        root.variables.insert(1, Value::Entity(target));
        let mut component = Component::start(
            Arc::new(store(
                fixture["input"]["graphs"].as_array().unwrap().clone(),
            )),
            owner,
            Variables::new(),
            vec![root],
            0,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        let snapshot = |component: &Component| {
            let view = component.snapshot().unwrap();
            json!({"active":view.instances[&1].states[&0].active,
                "complete":view.variables.get(&9)==Some(&Value::Bool(true)),
                "children":view.instances.values().filter(|item|item.graph==0x7000).count(),
                "subscriptions":component.machine.foreign_watchers.get(&(target,6604)).map_or(0,Vec::len)})
        };
        let mut actual = vec![snapshot(&component)];
        for step in case["steps"].as_array().unwrap() {
            let frame = u32::try_from(step["frame"].as_u64().unwrap()).unwrap();
            match step["op"].as_str().unwrap() {
                "set" => {
                    let value = step["value"].as_bool().unwrap();
                    if value != world.value {
                        world.value = value;
                        component
                            .queue_foreign_variable_change(frame, target, 6604)
                            .unwrap();
                        // Duplicate room notifications coalesce at the state.
                        component
                            .queue_foreign_variable_change(frame, target, 6604)
                            .unwrap();
                    }
                }
                "abort" => component
                    .machine
                    .queues
                    .push(1, frame_start(frame), Event::Abort(0))
                    .unwrap(),
                "poll" => (),
                other => panic!("unknown step {other}"),
            }
            component.advance_frame(frame, &mut world).unwrap();
            actual.push(snapshot(&component));
        }
        assert_eq!(json!(actual), expected["output"], "{}", case["name"]);
    }
}

#[test]
fn final_wave_join_counts_distinct_edges_and_releases_all_four_states() {
    let mut nodes = vec![node(
        0,
        "STUStatescriptEntry",
        None,
        json!({"m_outPlug":plug(&[(2,"m_beginPlug"),(3,"m_beginPlug"),(4,"m_beginPlug")], &[])}),
    )];
    nodes.push(node(1, "STUStatescriptEntry", None, json!({})));
    for raw in 0..4 {
        let mut links = vec![(7, "m_inPlug")];
        if raw == 0 {
            links.push((7, "m_inPlug"));
        }
        nodes.push(node(
            raw + 2,
            "STUStatescriptState",
            Some(raw),
            json!({"m_transitionPlug":[plug(&links, &[])]}),
        ));
    }
    nodes.push(node(
        6,
        "STUStatescriptActionSetVar",
        None,
        json!({"m_out_Var":var(8493,true),"m_value":{"$":"STUConfigVarBool","m_value":true}}),
    ));
    nodes.push(node(
        7,
        "STU_1113EBD5",
        None,
        json!({"m_122DF0CA":[0,1,2,3],"m_B24D1794":4,"m_outPlug":plug(&[(6,"m_inPlug")], &[])}),
    ));
    let graphs = Arc::new(store(vec![graph(
        0x0cc0,
        nodes,
        json!([2, 3, 4, 5]),
        json!([0]),
    )]));
    let mut component = Component::start(
        graphs,
        1,
        Variables::from([(8493, Value::Bool(false))]),
        vec![Root::new(1, 0x0cc0)],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    let snap = component.snapshot().unwrap();
    assert_eq!(snap.variables[&8493], Value::Bool(false));
    assert_eq!(snap.instances[&1].join_inputs[&7].len(), 3);
    assert!(snap.instances[&1].states.values().all(|state| state.active));

    component
        .machine
        .queues
        .push(1, frame_start(1), Event::CheckTransitions(0))
        .unwrap();
    component.advance_frame(1, &mut EmptyWorld).unwrap();
    assert_eq!(
        component.snapshot().unwrap().variables[&8493],
        Value::Bool(false)
    );
    component
        .machine
        .queues
        .push(1, frame_start(2), Event::Begin(3))
        .unwrap();
    component.advance_frame(2, &mut EmptyWorld).unwrap();
    let snap = component.snapshot().unwrap();
    assert!(snap.initialization_complete(), "{:?}", snap.diagnostics);
    assert_eq!(snap.variables[&8493], Value::Bool(true));
    assert!(snap.instances[&1].join_inputs.is_empty());
    assert!(
        snap.instances[&1]
            .states
            .values()
            .all(|state| !state.active)
    );
}

#[test]
fn final_wave_completion_rechecks_the_active_mode_subscript() {
    let mut mode = graph(
        0x0c8c,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(33,"m_beginPlug")], &[])}),
            ),
            node(
                1,
                "STU_3387AB5D",
                None,
                json!({
                    "m_condition":var(8493,true), "m_truePlug":plug(&[(2,"m_inPlug")], &[])
                }),
            ),
            node(
                2,
                "STUStatescriptActionSetVar",
                None,
                json!({
                    "m_out_Var":var(8494,true), "m_value":{"$":"STUConfigVarBool","m_value":true}
                }),
            ),
            node(
                33,
                "STU_37D754C8",
                Some(0),
                json!({
                    "m_36F7B64C":{"m_36F7B64C":{"m_C974E369":{"m_graph":"0x0580000000000CC0"}}},
                    "m_transitionPlug":[plug(&[(1,"m_inPlug")], &[])]
                }),
            ),
        ],
        json!([33]),
        json!([0]),
    );
    mode["items"] = json!(34);
    let graphs = Arc::new(store(vec![
        mode,
        graph(0x0cc0, vec![], json!([]), json!([])),
    ]));
    let mut component = Component::start(
        graphs,
        1,
        Variables::from([(8493, Value::Bool(false)), (8494, Value::Bool(false))]),
        vec![Root::new(1, 0x0c8c)],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert!(component.snapshot().unwrap().instances[&1].states[&0].active);
    component
        .machine
        .queues
        .push(
            1,
            frame_start(1),
            Event::Assign(VarKey::Entity(8493), Value::Bool(true)),
        )
        .unwrap();
    component.advance_frame(1, &mut EmptyWorld).unwrap();
    let snap = component.snapshot().unwrap();
    assert!(snap.initialization_complete(), "{:?}", snap.diagnostics);
    assert_eq!(snap.variables[&8494], Value::Bool(true));
    assert!(!snap.instances[&1].states[&0].active);
}

#[test]
#[ignore = "requires the external 104319 Junkenstein mode graphs"]
fn authored_victory_voice_releases_the_client_outcome_action() {
    #[derive(Default)]
    struct VictoryWorld {
        actors: Vec<Value>,
        actions: Vec<Site>,
    }

    impl World for VictoryWorld {
        fn play_victory_voice(&mut self, site: Site, actor: &Value) -> Option<u64> {
            assert_eq!(site.node, Some(221));
            self.actors.push(actor.clone());
            Some(13_138)
        }

        fn queue_mode_action(&mut self, site: Site) -> bool {
            self.actions.push(site);
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
    let raw = graphs
        .graph(0x0c8c)
        .unwrap()
        .node(221)
        .unwrap()
        .state
        .unwrap();
    let actor = Value::Entity(0xc000_1010);
    let mut root = Root::listener(2, 0x0c8c, Some(raw));
    root.variables.insert(8504, Value::Float(4.5));
    let mut world = VictoryWorld::default();
    let mut component = Component::start(
        graphs,
        crate::world::GAME_MODE_ENTITY,
        Variables::from([(8602, actor.clone())]),
        vec![root],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert!(world.actions.is_empty());
    assert_eq!(world.actors, [actor]);
    for frame in 1..=300 {
        component.advance_frame(frame, &mut world).unwrap();
    }
    assert_eq!(world.actions.len(), 1);
    assert_eq!(world.actions[0].node, Some(265));
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
}

pub(super) fn shared_victory_view_route_contract() {
    use crate::junkenstein_messages::DefenderMessage;
    #[derive(Default)]
    struct RouteWorld(Vec<DefenderMessage>);
    impl World for RouteWorld {
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
        fn route_game_message(&mut self, site: Site, message: u64, params: &Variables) -> bool {
            let Some(request) = DefenderMessage::victory_view(site, message, params) else {
                return false;
            };
            self.0.push(request);
            true
        }
    }
    let contract: Json = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/statescript-victory-view-route-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "statescript.victory-view-route.v1");
    assert_eq!(contract["target_build"], 104319);
    for case in contract["input"]["cases"].as_array().unwrap() {
        let position = u16::try_from(case["position"].as_u64().unwrap()).unwrap();
        let mut action = node(
            position,
            case["class"].as_str().unwrap(),
            None,
            case["fields"].clone(),
        );
        action["fields"]["m_ADEB6E05"] = json!(true);
        action["fields"]["m_outPlug"] = json!({"$":"Plug", "links":[[5,"m_inPlug"]]});
        let mut model = graph(
            0x0cec,
            vec![
                node(
                    0,
                    "STUStatescriptEntry",
                    None,
                    json!({"m_outPlug":{"$":"Plug","links":[[position,"m_inPlug"]]}}),
                ),
                action,
                node(
                    5,
                    "STUStatescriptActionSetVar",
                    None,
                    json!({
                        "m_out_Var":{"$":"STUConfigVarDynamic","m_60DB8F99":1,"m_identifier":9},
                        "m_value":{"$":"STUConfigVarBool","m_value":true}
                    }),
                ),
            ],
            json!([]),
            json!([0]),
        );
        model["items"] = json!(position.max(5) + 1);
        let graphs = Arc::new(store(vec![model]));
        let mut root = Root::new(1, 0x0cec);
        root.variables.insert(
            8602,
            Value::Entity(u32::try_from(contract["input"]["subject"].as_u64().unwrap()).unwrap()),
        );
        let mut world = RouteWorld::default();
        let component = Component::start(
            graphs,
            crate::world::GAME_MODE_ENTITY,
            Variables::new(),
            vec![root],
            0,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(
            snapshot.variables.get(&9),
            Some(&Value::Bool(
                case["expected"]["forwarded"].as_bool().unwrap()
            ))
        );
        if case["expected"]["parameters"].is_null() {
            assert!(world.0.is_empty());
            continue;
        }
        assert_eq!(world.0.len(), 1);
        let request = &world.0[0];
        assert_eq!(request.source, crate::world::GAME_MODE_ENTITY);
        assert_eq!(request.message, crate::junkenstein_messages::VICTORY_VIEW);
        let params: serde_json::Map<String, Json> = request
            .parameters
            .iter()
            .map(|(&key, value)| {
                (
                    key.to_string(),
                    match value {
                        Value::Bool(value) => json!(value),
                        Value::Entity(value) => json!({"entity":value}),
                        other => panic!("unexpected parameter {other:?}"),
                    },
                )
            })
            .collect();
        assert_eq!(Json::Object(params), case["expected"]["parameters"]);
    }
}

pub(super) fn shared_spatial_owner_absence_contract() {
    struct Host(Option<bool>);
    impl World for Host {
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
        fn spatial_binding_owner_available(&self, _site: Site) -> Option<bool> {
            self.0
        }
    }
    let contract: Json = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/statescript-spatial-owner-absence-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "statescript.spatial-owner-absence.v1");
    assert_eq!(contract["target_build"], 104319);
    for native_only in [false, true] {
        for case in contract["input"]["cases"].as_array().unwrap() {
            let definition = if native_only {
                &contract["input"]["native_only"]
            } else {
                &contract["input"]
            };
            let position = u16::try_from(if native_only {
                definition["position"].as_u64().unwrap()
            } else {
                case["position"].as_u64().unwrap()
            })
            .unwrap();
            let mut model = graph(
                0x0ced,
                vec![node(
                    position,
                    definition["class"].as_str().unwrap(),
                    Some(0),
                    definition["fields"].clone(),
                )],
                json!([position]),
                json!([]),
            );
            model["items"] = json!(position + 1);
            let graphs = Arc::new(store(vec![model]));
            let mut world = Host(match case["service"].as_str().unwrap() {
                "absent" => Some(false),
                "present" => Some(true),
                "unknown" => None,
                _ => panic!("unknown service"),
            });
            let mut component = Component::start(
                graphs,
                0xa000_0101,
                Variables::new(),
                vec![Root::listener(1, 0x0ced, Some(0))],
                0,
                &mut world,
                MAX_INIT_WORK,
            )
            .unwrap();
            let snapshot = component.snapshot().unwrap();
            let actual = if snapshot.initialization_complete() {
                let state = &snapshot.instances[&1].states[&0];
                let before = json!([state.active, state.activation]);
                component
                    .machine
                    .queues
                    .push(1, frame_start(1), Event::Abort(0))
                    .unwrap();
                component.advance_frame(1, &mut world).unwrap();
                let state = &component.snapshot().unwrap().instances[&1].states[&0];
                json!({"status":"ok","begin":before,"end":[state.active,state.activation]})
            } else {
                assert!(snapshot.diagnostics.iter().any(|issue| matches!(&issue.issue,Issue::UnsupportedState(class) if class==definition["class"].as_str().unwrap())));
                json!({"status":"unavailable"})
            };
            assert_eq!(
                actual, case["expected"],
                "{} native_only={native_only}",
                case["name"]
            );
        }
    }
}

#[test]
#[ignore = "requires the external 104319 Junkenstein mode graphs"]
fn authored_victory_view_child_finishes_the_mode_phase() {
    #[derive(Default)]
    struct EndWorld(Vec<crate::junkenstein_messages::DefenderMessage>);
    impl World for EndWorld {
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
        fn play_victory_voice(&mut self, _site: Site, _actor: &Value) -> Option<u64> {
            Some(100)
        }
        fn queue_mode_action(&mut self, _site: Site) -> bool {
            true
        }
        fn route_game_message(&mut self, site: Site, message: u64, params: &Variables) -> bool {
            let Some(request) =
                crate::junkenstein_messages::DefenderMessage::victory_view(site, message, params)
            else {
                return false;
            };
            self.0.push(request);
            true
        }
    }
    let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let graphs = Arc::new(GraphStore::load(runtime.join("data/game_graphs_174.json.gz")).unwrap());
    let raw = graphs
        .graph(0x0c8c)
        .unwrap()
        .node(73)
        .unwrap()
        .state
        .unwrap();
    let mut root = Root::listener(2, 0x0c8c, Some(raw));
    root.variables.insert(8504, Value::Float(4.5));
    let mut world = EndWorld::default();
    let mut component = Component::start(
        graphs,
        crate::world::GAME_MODE_ENTITY,
        Variables::from([(8602, Value::Entity(0xa000_0101))]),
        vec![root],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    for frame in 1..=80 {
        component.advance_frame(frame, &mut world).unwrap();
    }
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    let mode = &snapshot.instances[&2];
    assert_eq!(mode.variables.get(&59), Some(&Value::Integer(5)));
    assert!(!mode.states[&raw].active);
    assert!(
        !snapshot
            .instances
            .values()
            .any(|instance| instance.graph == 0x0cec)
    );
    assert_eq!(
        world
            .0
            .iter()
            .map(|message| message.parameters[&0x390].clone())
            .collect::<Vec<_>>(),
        [Value::Bool(true), Value::Bool(false)]
    );
}

#[test]
fn public_schema_evaluates_before_assignment_and_inherits_to_children() {
    let entry = |links: &[(u16, &str)]| {
        node(
            0,
            "STUStatescriptEntry",
            None,
            json!({"m_outPlug":plug(links,&[])}),
        )
    };
    let set = |position, output, value| {
        node(
            position,
            "STUStatescriptActionSetVar",
            None,
            json!({"m_out_Var":var(output,true),"m_value":value}),
        )
    };
    let mut parent = graph(
        42,
        vec![
            entry(&[(1, "m_inPlug"), (2, "m_beginPlug")]),
            set(1, 100, var(2, false)),
            node(
                2,
                "STU_37D754C8",
                Some(0),
                json!({
                    "m_36F7B64C":{"m_36F7B64C":{"m_C974E369":{"m_graph":"0x058000000000002B"}}}
                }),
            ),
        ],
        json!([2]),
        json!([0]),
    );
    let mut child = graph(
        43,
        vec![entry(&[(1, "m_inPlug")]), set(1, 101, var(4, false))],
        json!([]),
        json!([0]),
    );
    let row = |id, value| json!({"m_0D09D2D9":format!("0x0D8000000000{id:04X}"),"m_value":value});
    parent["fields"]["m_publicSchema"] = json!({"$":"STU_77990ED7",
        "m_89EAAC40":"0x058000000000002A","m_entries":[
            row(1,json!(10)),row(2,json!(20)),row(3,var(1,false)),
            {"m_0D09D2D9":null,"m_value":999}]});
    child["fields"]["m_publicSchema"] =
        json!({"$":"STUStatescriptSchema","m_entries":[row(4,json!(40))]});
    let graphs = Arc::new(store(vec![parent, child]));
    let mut root = Root::new(1, 42);
    root.variables.insert(2, Value::Integer(30));
    for enabled in [false, true] {
        let mut world = EmptyWorld;
        let start = if enabled {
            Component::start_with_public_schema
        } else {
            Component::start
        };
        let component = start(
            Arc::clone(&graphs),
            42,
            Variables::new(),
            vec![root.clone()],
            0,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(snapshot.variables[&100], Value::Integer(30));
        assert_eq!(
            snapshot.variables[&101],
            if enabled {
                Value::Integer(40)
            } else {
                Value::Null
            }
        );
        if enabled {
            assert_eq!(snapshot.instances[&1].variables[&1], Value::Integer(10));
            assert_eq!(snapshot.instances[&1].variables[&3], Value::Null);
            assert!(!snapshot.instances[&1].variables.contains_key(&0));
        }
    }
}

#[test]
fn public_schema_rejects_malformed_rows_entries_and_foreign_reference() {
    for schema in [
        json!({"$":"STUStatescriptSchema","m_entries":{}}),
        json!({"$":"STUStatescriptSchema","m_entries":[7]}),
        json!({"$":"STU_77990ED7","m_89EAAC40":"0x058000000000002B"}),
    ] {
        let mut fixture = graph(42, vec![], json!([]), json!([]));
        fixture["fields"]["m_publicSchema"] = schema;
        let component = Component::start_with_public_schema(
            Arc::new(store(vec![fixture])),
            42,
            Variables::new(),
            vec![Root::new(1, 42)],
            0,
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap();
        assert!(!component.snapshot().unwrap().initialization_complete());
    }
}
