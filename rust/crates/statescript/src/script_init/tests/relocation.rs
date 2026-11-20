use super::*;

const DESTINATION: u32 = 1;
const CONTINUED: u32 = 2;
const RELOCATION_CLASS: &str = "authored.relocation";
const INVALID_DESTINATION: &str = "expected a finite float32-representable relocation destination";

fn fixture() -> Json {
    let fixture: Json = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-relocation-v1.json"
    )))
    .unwrap();
    assert_eq!(fixture["schema_version"], 1);
    assert_eq!(fixture["case_id"], "script-relocation.synthetic.v1");
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
        RELOCATION_CLASS,
        None,
        json!({"destination":var(DESTINATION,true),"m_outPlug":plug(&[(2,"m_inPlug")],&[])}),
    );
    action["server_only"] = json!(true);
    action["action_behavior"] = json!("relocation");
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
    requests: Vec<(Site, u32, ScriptedMotion)>,
}

impl World for Host {
    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        panic!("relocation must bind its origin at consumption, not VM admission")
    }

    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }

    fn queue_scripted_motion(
        &mut self,
        site: Site,
        frame: u32,
        motion: ScriptedMotion,
    ) -> Option<bool> {
        self.requests.push((site, frame, motion));
        self.admission
    }
}

#[test]
fn relocation_validates_coordinates_and_shares_the_consuming_pose_distance_bound() {
    for case in fixture()["input"]["constructor_cases"].as_array().unwrap() {
        let destination = vector(&case["destination"]);
        let relocation = Relocation::new(destination);
        assert_eq!(relocation.is_some(), case["constructed"] == true, "{case}");
        let actual =
            relocation.and_then(|request| request.destination_from(vector(&case["origin"])));
        let expected = (case["reachable"] == true).then_some(destination);
        assert_eq!(actual, expected, "{case}");
    }
}

#[test]
fn relocation_loader_validates_the_action_and_authored_alias() {
    let fixture = fixture();
    let source = model(&fixture["input"]);
    for alias in [false, true] {
        let mut data = source.clone();
        if alias {
            data["authored_classes"] =
                json!([{"class":RELOCATION_CLASS,"behavior":"relocation_action"}]);
            data["graphs"][0]["nodes"][1]
                .as_object_mut()
                .unwrap()
                .remove("action_behavior");
            data["graphs"][0]["nodes"][1]["server_only"] = json!(false);
        }
        let graphs = GraphStore::from_json(&serde_json::to_vec(&data).unwrap()).unwrap();
        let graph_id = u32::try_from(fixture["input"]["graph"].as_u64().unwrap()).unwrap();
        let action = graphs.graph(graph_id).unwrap().node(1).unwrap();
        assert!(action.declares_relocation_action());
        assert!(action.server_only && action.is_action() && !action.is_networked());
        assert_eq!(action.class, "ActionRelocation");
    }
    for failure in [
        "missing_destination",
        "null",
        "server",
        "client",
        "state",
        "plug",
    ] {
        let mut data = source.clone();
        let action = &mut data["graphs"][0]["nodes"][1];
        match failure {
            "missing_destination" => {
                action["fields"]
                    .as_object_mut()
                    .unwrap()
                    .remove("destination");
            }
            "null" => action["fields"]["destination"] = Json::Null,
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
fn relocation_queues_only_valid_self_destinations_at_the_next_frame() {
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
        let destination = step["destination"]
            .as_bool()
            .map_or_else(|| Value::Vector(vector(&step["destination"])), Value::Bool);
        let mut data = model(input);
        if step["invalid_expression"] == true {
            data["graphs"][0]["nodes"][1]["fields"]["destination"] =
                json!({"$":"authored.unknown_destination"});
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
            Variables::from([
                (DESTINATION, destination.clone()),
                (CONTINUED, Value::Bool(false)),
            ]),
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
            let Value::Vector(destination) = destination else {
                panic!("valid request vector")
            };
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
                    ScriptedMotion::Relocate(Relocation::new(destination).unwrap()),
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
            Some("destination") => assert_eq!(
                issues,
                [&Issue::Expression(EvalError::MalformedConfig(
                    INVALID_DESTINATION
                ))]
            ),
            Some("source") => assert_eq!(issues, [&Issue::UnsupportedField("relocation source")]),
            Some("service") => assert_eq!(issues, [&Issue::UnsupportedField("relocation service")]),
            Some("frame") => assert_eq!(issues, [&Issue::UnsupportedField("relocation frame")]),
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
fn legacy_swept_admission_forwards_into_the_shared_motion_service() {
    let site = Site {
        entity: 1202,
        instance: 1,
        graph: 720,
        node: Some(1),
    };
    let displacement = SweptDisplacement::new([1.0, 0.0, 0.0]).unwrap();
    let mut host = Host {
        admission: Some(true),
        ..Host::default()
    };
    assert_eq!(
        host.queue_swept_displacement(site, 21, displacement),
        Some(true)
    );
    assert_eq!(
        host.requests,
        [(site, 21, ScriptedMotion::Sweep(displacement))]
    );
    assert_eq!(
        EmptyWorld.queue_swept_displacement(site, 21, displacement),
        None
    );
    assert_eq!(
        EmptyWorld.queue_scripted_motion(
            site,
            21,
            ScriptedMotion::Relocate(Relocation::new([1.0, 0.0, 0.0]).unwrap())
        ),
        None
    );
}
