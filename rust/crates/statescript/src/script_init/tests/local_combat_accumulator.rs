use serde::Deserialize;

use super::*;

const ACCUMULATOR: &str = "STU_B5C87F3D";
const ENTITY: u32 = 691;
const MARKER: u32 = 847;

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    evidence: String,
    input: Input,
}

#[derive(Deserialize)]
struct Input {
    placements: Vec<(u32, u16)>,
    dormant_fields: Json,
    contexts: Vec<ContextCase>,
}

#[derive(Deserialize)]
struct ContextCase {
    available: Option<bool>,
    admitted: bool,
}

struct AccumulatorWorld {
    site: Site,
    available: Option<bool>,
}

impl World for AccumulatorWorld {
    fn local_combat_context_available(&self, site: Site) -> Option<bool> {
        assert_eq!(site, self.site);
        self.available
    }

    fn body_eye_position(&self, _: u32) -> Option<[f64; 3]> {
        panic!("an unimplemented accumulator must not evaluate its position")
    }

    fn native(&mut self, _: Site, _: &Json) -> Option<Value> {
        panic!("an unimplemented accumulator must not evaluate its spatial inputs")
    }

    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
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
fn accumulator_preserves_lifecycle_only_when_local_context_is_known_absent() {
    let contract: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-local-combat-accumulator-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.evidence, "synthetic-rust");

    for &(graph_index, position) in &contract.input.placements {
        let mut fields = contract.input.dormant_fields.clone();
        fields["m_onBeginPlug"] = plug(&[(1, "m_inPlug")], &[]);
        fields["m_onEndPlug"] = plug(&[(2, "m_inPlug")], &[]);
        let mut fixture = graph(
            graph_index,
            vec![
                node(
                    0,
                    "STUStatescriptEntry",
                    None,
                    json!({"m_outPlug": plug(&[(position, "m_beginPlug")], &[])}),
                ),
                mark(1, true),
                mark(2, false),
                node(position, ACCUMULATOR, Some(0), fields),
            ],
            json!([position]),
            json!([0]),
        );
        fixture["items"] = json!(position + 1);
        fixture["header"]["m_nodesBitCount"] = json!(7);
        let graphs = Arc::new(store(vec![fixture]));
        let site = Site {
            entity: ENTITY,
            instance: 1,
            graph: graph_index,
            node: Some(position),
        };

        for &ContextCase {
            available,
            admitted,
        } in &contract.input.contexts
        {
            let label = format!("graph {graph_index:#06x}, node {position}, context {available:?}");
            let mut world = AccumulatorWorld { site, available };
            let mut component = Component::start(
                Arc::clone(&graphs),
                ENTITY,
                Variables::new(),
                vec![Root::new(1, graph_index)],
                0,
                &mut world,
                MAX_INIT_WORK,
            )
            .unwrap();
            component.advance_frame(8, &mut world).unwrap();
            let snapshot = component.snapshot().unwrap();
            if admitted {
                assert!(snapshot.diagnostics.is_empty(), "{label}");
                let state = &snapshot.instances[&1].states[&0];
                assert!(state.active, "{label}");
                assert_eq!(state.kind, StateKind::Plain, "{label}");
                assert_eq!(snapshot.variables[&MARKER], Value::Bool(true), "{label}");

                component.debug_end_state(1, 0, &mut world).unwrap();
                let ended = component.snapshot().unwrap();
                assert!(!ended.instances[&1].states[&0].active, "{label}");
                assert_eq!(ended.variables[&MARKER], Value::Bool(false), "{label}");
            } else {
                assert!(
                    matches!(
                        snapshot.diagnostics.as_slice(),
                        [Diagnostic {
                            site: diagnostic_site,
                            issue: Issue::UnsupportedState(class),
                            registration_rejected: false,
                            ..
                        }] if class == ACCUMULATOR && diagnostic_site == &site
                    ),
                    "{label}: {:?}",
                    snapshot.diagnostics
                );
                assert!(!snapshot.instances[&1].states.contains_key(&0), "{label}");
                assert!(!snapshot.variables.contains_key(&MARKER), "{label}");
            }
        }
    }
}
