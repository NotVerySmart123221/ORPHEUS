use super::*;

const DELTA: u32 = 1;
const CONTINUED: u32 = 2;
const INVALID_DELTA: &str =
    "expected a finite nonzero swept displacement of at most 32 world units";

fn fixture() -> Json {
    let fixture: Json = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-swept-displacement-v1.json"
    )))
    .unwrap();
    assert_eq!(fixture["schema_version"], 1);
    assert_eq!(fixture["case_id"], "script-swept-displacement.synthetic.v1");
    assert_eq!(fixture["evidence"], "synthetic-rust");
    fixture
}

fn vector(value: &Json) -> [f64; 3] {
    let axes = value.as_array().unwrap();
    assert_eq!(axes.len(), 3);
    std::array::from_fn(|axis| {
        axes[axis]
            .as_f64()
            .unwrap_or_else(|| match axes[axis].as_str().unwrap() {
                "nan" => f64::NAN,
                "infinity" => f64::INFINITY,
                value => panic!("unknown synthetic scalar {value}"),
            })
    })
}

fn model(input: &Json) -> Json {
    let graph_id = u32::try_from(input["graph"].as_u64().unwrap()).unwrap();
    let message = input["message"].as_u64().unwrap();
    let mut action = node(
        1,
        "AuthoredSweepAction",
        None,
        json!({"delta":var(DELTA,true),"m_outPlug":plug(&[(2,"m_inPlug")],&[])}),
    );
    action["server_only"] = json!(true);
    action["action_behavior"] = json!("swept_displacement");
    let graph = graph(
        graph_id,
        vec![
            node(
                0,
                "STUStatescriptEntryGameMessage",
                None,
                json!({"m_gameMessage":{"m_gameMessage":format!("0x{message:016X}")},
                    "m_outPlug":plug(&[(1,"m_inPlug")],&[])}),
            ),
            action,
            node(
                2,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var":var(CONTINUED,true),"m_value":true}),
            ),
        ],
        json!([]),
        json!([0]),
    );
    json!({"version":1,"graphs":[graph],"bodies":{},"missing":[]})
}

#[derive(Default)]
struct Host {
    admission: Option<bool>,
    requests: Vec<(Site, u32, SweptDisplacement)>,
}

impl World for Host {
    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }

    fn queue_swept_displacement(
        &mut self,
        site: Site,
        frame: u32,
        displacement: SweptDisplacement,
    ) -> Option<bool> {
        self.requests.push((site, frame, displacement));
        self.admission
    }
}

#[test]
fn swept_displacement_bounds_the_host_vector_after_conversion() {
    let fixture = fixture();
    for case in fixture["input"]["constructor_cases"].as_array().unwrap() {
        let actual = SweptDisplacement::new(vector(&case["delta"]));
        let expected = (!case["expected"].is_null())
            .then(|| vector(&case["expected"]).map(|axis| axis as f32));
        assert_eq!(
            actual.map(SweptDisplacement::delta),
            expected,
            "{}",
            case["name"]
        );
    }
}

#[test]
fn swept_displacement_loader_requires_server_only_action_and_delta() {
    let fixture = fixture();
    let source = model(&fixture["input"]);
    for alias in [false, true] {
        let mut data = source.clone();
        if alias {
            data["authored_classes"] = json!([{
                "class":"AuthoredSweepAction","behavior":"swept_displacement_action"}]);
            data["graphs"][0]["nodes"][1]
                .as_object_mut()
                .unwrap()
                .remove("action_behavior");
            data["graphs"][0]["nodes"][1]["server_only"] = json!(false);
        }
        let graphs = GraphStore::from_json(&serde_json::to_vec(&data).unwrap()).unwrap();
        let graph_id = u32::try_from(fixture["input"]["graph"].as_u64().unwrap()).unwrap();
        let action = graphs.graph(graph_id).unwrap().node(1).unwrap();
        assert!(action.declares_swept_displacement_action());
        assert!(action.server_only && action.is_action() && !action.is_networked());
        assert_eq!(action.class, "ActionSweptDisplacement");
    }
    for failure in [
        "delta",
        "missing_delta",
        "server",
        "client",
        "state",
        "plug",
    ] {
        let mut data = source.clone();
        let action = &mut data["graphs"][0]["nodes"][1];
        match failure {
            "delta" => action["fields"]["delta"] = Json::Null,
            "missing_delta" => {
                action["fields"].as_object_mut().unwrap().remove("delta");
            }
            "server" => action["server_only"] = json!(false),
            "client" => action["client_only"] = json!(true),
            "state" => action["state"] = json!(0),
            "plug" => action["fields"]["m_outPlug"] = json!({"$":"Plug","links":[[2,false]]}),
            _ => unreachable!(),
        }
        assert!(
            GraphStore::from_json(&serde_json::to_vec(&data).unwrap()).is_err(),
            "{failure}"
        );
    }
}

#[test]
fn swept_displacement_admits_only_valid_self_requests_at_the_next_frame() {
    let fixture = fixture();
    let input = &fixture["input"];
    let graph_id = u32::try_from(input["graph"].as_u64().unwrap()).unwrap();
    let message = input["message"].as_u64().unwrap();
    for step in input["steps"].as_array().unwrap() {
        let body = u32::try_from(
            step["body"]
                .as_u64()
                .unwrap_or(input["body"].as_u64().unwrap()),
        )
        .unwrap();
        let frame = u32::try_from(step["frame"].as_u64().unwrap()).unwrap();
        let delta = if let Some(value) = step["delta"].as_bool() {
            Value::Bool(value)
        } else {
            Value::Vector(vector(&step["delta"]))
        };
        let mut data = model(input);
        if step["invalid_expression"] == true {
            data["graphs"][0]["nodes"][1]["fields"]["delta"] =
                json!({"$":"authored.unknown_delta"});
        }
        let mut host = Host {
            admission: match step["admission"].as_str().unwrap() {
                "accepted" => Some(true),
                "refused" => Some(false),
                "unavailable" => None,
                value => panic!("unknown admission {value}"),
            },
            requests: Vec::new(),
        };
        let mut component = Component::start(
            Arc::new(GraphStore::from_json(&serde_json::to_vec(&data).unwrap()).unwrap()),
            body,
            Variables::from([(DELTA, delta.clone()), (CONTINUED, Value::Bool(false))]),
            vec![Root::listener(1, graph_id, None)],
            0,
            &mut host,
            MAX_INIT_WORK,
        )
        .unwrap();
        assert!(host.requests.is_empty());
        assert_eq!(
            component
                .queue_frame_message(frame, 1, message, Variables::new())
                .unwrap(),
            1
        );
        component.advance_frame(frame, &mut host).unwrap();
        if step["host_call"] == true {
            let Value::Vector(delta) = delta else {
                panic!("valid request vector")
            };
            let displacement = SweptDisplacement::new(delta).unwrap();
            assert_eq!(
                host.requests,
                [(
                    Site {
                        entity: body,
                        instance: 1,
                        graph: graph_id,
                        node: Some(1)
                    },
                    frame.checked_add(1).unwrap(),
                    displacement
                )],
                "{}",
                step["name"]
            );
        } else {
            assert!(host.requests.is_empty(), "{}", step["name"]);
        }
        let snapshot = component.snapshot().unwrap();
        assert_eq!(
            snapshot.variables[&CONTINUED],
            Value::Bool(step["continued"].as_bool().unwrap()),
            "{}",
            step["name"]
        );
        let issues: Vec<_> = snapshot
            .diagnostics
            .iter()
            .map(|diagnostic| &diagnostic.issue)
            .collect();
        match step["diagnostic"].as_str() {
            None => assert!(issues.is_empty(), "{}: {issues:?}", step["name"]),
            Some("delta") => assert_eq!(
                issues,
                [&Issue::Expression(EvalError::MalformedConfig(
                    INVALID_DELTA
                ))]
            ),
            Some("source") => assert_eq!(
                issues,
                [&Issue::UnsupportedField("swept displacement source")]
            ),
            Some("service") => assert_eq!(
                issues,
                [&Issue::UnsupportedField("swept displacement service")]
            ),
            Some("frame") => assert_eq!(
                issues,
                [&Issue::UnsupportedField("swept displacement frame")]
            ),
            Some("expression") => assert!(matches!(
                snapshot.diagnostics.as_slice(),
                [Diagnostic {
                    issue: Issue::Expression(EvalError::UnsupportedConfig(_)),
                    ..
                }]
            )),
            value => panic!("unexpected diagnostic {value:?}"),
        }
    }
}

#[test]
fn swept_displacement_default_world_reports_an_unavailable_service() {
    let fixture = fixture();
    let input = &fixture["input"];
    let graph_id = u32::try_from(input["graph"].as_u64().unwrap()).unwrap();
    let mut component = Component::start(
        Arc::new(GraphStore::from_json(&serde_json::to_vec(&model(input)).unwrap()).unwrap()),
        1201,
        Variables::from([(DELTA, Value::Vector([1.0, 0.0, 0.0]))]),
        vec![Root::listener(1, graph_id, None)],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    component
        .queue_message(1, input["message"].as_u64().unwrap(), Variables::new())
        .unwrap();
    component.run_now(&mut EmptyWorld).unwrap();
    assert_eq!(
        component.snapshot().unwrap().diagnostics[0].issue,
        Issue::UnsupportedField("swept displacement service")
    );
    assert!(
        !component
            .snapshot()
            .unwrap()
            .variables
            .contains_key(&CONTINUED)
    );
}
