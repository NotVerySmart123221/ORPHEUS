use serde::Deserialize;

use super::*;

const ACTION: &str = "STU_FF3471E9";
const OWNER: u32 = 731;
const INSTANCE: u16 = 1;
const MARKER: u32 = 900;

#[derive(Deserialize)]
struct Fixture {
    graph: Json,
    source_frame: u32,
    portable_action_cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    graph: u32,
    node: u16,
    #[serde(default)]
    row_fields: serde_json::Map<String, Json>,
    #[serde(default)]
    repeat_row: bool,
    host_accepts: bool,
    attempted_cap: Option<f64>,
    unsupported_shape: bool,
}

struct ClampWorld {
    accepts: bool,
    attempts: Vec<(Site, u32, f64)>,
}

impl World for ClampWorld {
    fn cap_horizontal_velocity(&mut self, site: Site, frame: u32, cap: f64) -> bool {
        self.attempts.push((site, frame, cap));
        self.accepts
    }

    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }
}

#[test]
fn native_horizontal_clamp_uses_pinned_selector_and_numeric_bounds_at_any_site() {
    let fixture: Fixture = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/flashbang-horizontal-cap-v1.json"
    )))
    .unwrap();

    for case in &fixture.portable_action_cases {
        let mut graph = fixture.graph.clone();
        graph["index"] = json!(case.graph);
        let nodes = graph["nodes"].as_array_mut().unwrap();
        nodes[0]["fields"]["m_outPlug"]["links"][0][0] = json!(case.node);
        let action = &mut nodes[1];
        for key in ["pos", "uid", "rid"] {
            action[key] = json!(case.node);
        }
        let rows = action["fields"]["m_BC5E91CF"].as_array_mut().unwrap();
        rows[0]
            .as_object_mut()
            .unwrap()
            .extend(case.row_fields.clone());
        if case.repeat_row {
            rows.push(rows[0].clone());
        }

        let mut world = ClampWorld {
            accepts: case.host_accepts,
            attempts: Vec::new(),
        };
        let component = Component::start(
            Arc::new(store(vec![graph])),
            OWNER,
            Variables::new(),
            vec![Root::new(INSTANCE, case.graph)],
            fixture.source_frame,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        let snapshot = component.snapshot().unwrap();
        let site = Site {
            entity: OWNER,
            instance: INSTANCE,
            graph: case.graph,
            node: Some(case.node),
        };
        let expected_attempt = case
            .attempted_cap
            .map(|cap| (site, fixture.source_frame, cap));
        assert_eq!(
            world.attempts.as_slice(),
            expected_attempt.as_slice(),
            "{}",
            case.name
        );
        assert_eq!(
            snapshot.variables.get(&MARKER),
            (expected_attempt.is_some() && case.host_accepts).then_some(&Value::Bool(true)),
            "{}",
            case.name
        );
        if case.unsupported_shape {
            assert!(
                matches!(
                    snapshot.diagnostics.as_slice(),
                    [Diagnostic { site: actual, issue: Issue::UnsupportedAction(class), .. }]
                    if *actual == site && class == ACTION
                ),
                "{}: {:?}",
                case.name,
                snapshot.diagnostics
            );
        } else {
            assert!(
                snapshot.diagnostics.is_empty(),
                "{}: {:?}",
                case.name,
                snapshot.diagnostics
            );
        }
    }
}
