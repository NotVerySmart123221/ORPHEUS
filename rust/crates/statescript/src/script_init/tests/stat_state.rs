use serde::Deserialize;

use super::*;

const MARKER: u32 = 741;
const ENTITY: u32 = 829;
const STATE_CLASS: &str = "STU_441B50E4";

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    evidence: String,
    input: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    graph: u32,
    server_only: bool,
    player_stats_available: Option<bool>,
    local_combat_available: Option<bool>,
    fields: serde_json::Map<String, Json>,
    expected_admission: bool,
}

struct StatWorld<'a>(&'a Case);

impl World for StatWorld<'_> {
    fn player_stats_available(&self, site: Site) -> Option<bool> {
        assert_eq!(
            (site.entity, site.graph, site.node),
            (ENTITY, self.0.graph, Some(1))
        );
        self.0.player_stats_available
    }

    fn local_combat_context_available(&self, _: Site) -> Option<bool> {
        self.0.local_combat_available
    }

    fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
        None
    }
}

fn mark(position: u16, value: bool) -> Json {
    node(
        position,
        "STUStatescriptActionSetVar",
        None,
        json!({"m_out_Var": var(MARKER, true), "m_value": value}),
    )
}

#[test]
fn stat_state_fallback_requires_known_player_stat_absence() {
    let contract: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-stat-state-facts-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.evidence, "synthetic-rust");

    for case in &contract.input {
        let mut fields = json!({
            "m_targets": {"$": "STU_B5A0CAF0"},
            "m_onBeginPlug": plug(&[(2, "m_inPlug")], &[]),
            "m_onEndPlug": plug(&[(3, "m_inPlug")], &[]),
        });
        fields.as_object_mut().unwrap().extend(case.fields.clone());
        let mut state = node(1, STATE_CLASS, Some(0), fields);
        state["server_only"] = json!(case.server_only);
        let fixture = graph(
            case.graph,
            vec![
                node(
                    0,
                    "STUStatescriptEntry",
                    None,
                    json!({"m_outPlug": plug(&[(1, "m_beginPlug")], &[])}),
                ),
                state,
                mark(2, true),
                mark(3, false),
            ],
            json!([1]),
            json!([0]),
        );
        let mut world = StatWorld(case);
        let mut component = Component::start(
            Arc::new(store(vec![fixture])),
            ENTITY,
            Variables::new(),
            vec![Root::new(1, case.graph)],
            0,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        let snapshot = component.snapshot().unwrap();
        if case.expected_admission {
            assert!(
                snapshot.diagnostics.is_empty(),
                "{}: {:?}",
                case.name,
                snapshot.diagnostics
            );
            let state = &snapshot.instances[&1].states[&0];
            assert!(state.active, "{}", case.name);
            assert_eq!(state.kind, StateKind::Plain, "{}", case.name);
            assert_eq!(
                snapshot.variables[&MARKER],
                Value::Bool(true),
                "{}",
                case.name
            );

            component.debug_end_state(1, 0, &mut world).unwrap();
            let ended = component.snapshot().unwrap();
            assert!(!ended.instances[&1].states[&0].active, "{}", case.name);
            assert_eq!(
                ended.variables[&MARKER],
                Value::Bool(false),
                "{}",
                case.name
            );
        } else {
            assert!(
                matches!(
                    snapshot.diagnostics.as_slice(),
                    [Diagnostic { issue: Issue::UnsupportedState(class), .. }]
                    if class == STATE_CLASS
                ),
                "{}: {:?}",
                case.name,
                snapshot.diagnostics
            );
            assert!(
                !snapshot.instances[&1].states.contains_key(&0),
                "{}",
                case.name
            );
            assert!(!snapshot.variables.contains_key(&MARKER), "{}", case.name);
        }
    }
}
