use serde::Deserialize;

use super::*;

const STATE: &str = "STUStatescriptStateModifyHealth";
const OWNER: u32 = 835;
const INSTANCE: u16 = 1;
const RATE: u32 = 69;

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    evidence: String,
    input: Vec<Scenario>,
    expected: Vec<f64>,
}

#[derive(Deserialize)]
struct Scenario {
    name: String,
    graph: u32,
    pos: u16,
    server_only: bool,
    supports_shields: bool,
    outcome: Outcome,
    #[serde(default)]
    node_fields: serde_json::Map<String, Json>,
    #[serde(default)]
    effect_fields: serde_json::Map<String, Json>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Recharges,
    UnsupportedShape,
    UnsupportedHost,
}

struct ShieldWorld<'a> {
    scenario: &'a Scenario,
    calls: Vec<f64>,
}

impl World for ShieldWorld<'_> {
    fn recharge_shields(&mut self, site: Site, amount: f64) -> bool {
        assert_eq!(
            (site.entity, site.graph, site.node),
            (OWNER, self.scenario.graph, Some(self.scenario.pos))
        );
        self.calls.push(amount);
        self.scenario.supports_shields
    }

    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }
}

fn start(scenario: &Scenario, world: &mut ShieldWorld<'_>) -> Component {
    let mut fields = json!({
        "m_DD792FF0": 3,
        "m_7B2B3F5C": 1,
        "m_8B707686": 0,
        "m_95F1FB0B": {"$": "STUConfigVarInt", "m_value": 1},
        "m_FA3ECC43": {"$": "STU_B5A0CAF0"},
        "m_modifyHealth": {
            "$": "STUStatescriptModifyHealth",
            "m_93CDFD2F": 0,
            "m_430E83BE": 0,
            "m_amount": var(RATE, false),
            "m_724CBA57": {"$": "STU_B5A0CAF0"}
        }
    });
    fields["m_modifyHealth"]
        .as_object_mut()
        .unwrap()
        .extend(scenario.effect_fields.clone());
    fields
        .as_object_mut()
        .unwrap()
        .extend(scenario.node_fields.clone());
    let mut state = node(scenario.pos, STATE, Some(0), fields);
    state["server_only"] = json!(scenario.server_only);
    let mut bundle = graph(
        scenario.graph,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug": plug(&[(scenario.pos, "m_beginPlug")], &[])}),
            ),
            state,
        ],
        json!([scenario.pos]),
        json!([0]),
    );
    bundle["items"] = json!(scenario.pos + 1);
    Component::start(
        Arc::new(store(vec![bundle])),
        OWNER,
        Variables::new(),
        vec![Root::new(INSTANCE, scenario.graph)],
        0,
        world,
        MAX_INIT_WORK,
    )
    .unwrap()
}

#[test]
fn authored_self_shield_recharge_is_timed_placement_independent_and_strict() {
    let fixture: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-self-shield-recharge-v1.json"
    )))
    .unwrap();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.evidence, "synthetic-rust");

    for scenario in &fixture.input {
        let mut world = ShieldWorld {
            scenario,
            calls: Vec::new(),
        };
        let mut component = start(scenario, &mut world);
        let snapshot = component.snapshot().unwrap();
        match scenario.outcome {
            Outcome::UnsupportedShape => {
                assert!(
                    matches!(
                        snapshot.diagnostics.as_slice(),
                        [Diagnostic { issue: Issue::UnsupportedState(class), .. }] if class == STATE
                    ),
                    "{}: {:?}",
                    scenario.name,
                    snapshot.diagnostics
                );
                assert!(!snapshot.instances[&INSTANCE].states.contains_key(&0));
                assert!(world.calls.is_empty());
            }
            Outcome::UnsupportedHost => {
                assert!(
                    matches!(
                        snapshot.diagnostics.as_slice(),
                        [Diagnostic {
                            issue: Issue::UnsupportedField("shield recharge service"),
                            ..
                        }]
                    ),
                    "{}: {:?}",
                    scenario.name,
                    snapshot.diagnostics
                );
                component.advance_frame(1, &mut world).unwrap();
                assert_eq!(world.calls, [0.0]);
            }
            Outcome::Recharges => {
                assert!(
                    snapshot.diagnostics.is_empty(),
                    "{}: {:?}",
                    scenario.name,
                    snapshot.diagnostics
                );
                let state = &snapshot.instances[&INSTANCE].states[&0];
                assert!(state.active);
                assert_eq!(state.kind, StateKind::ShieldRecharge);
                assert_eq!(world.calls, [0.0]);
                for (frame, rate) in [(1, 62.5), (2, 125.0), (3, -1.0), (4, f64::INFINITY)] {
                    component
                        .set_instance_variable(INSTANCE, RATE, Value::Float(rate), &mut world)
                        .unwrap();
                    component.advance_frame(frame, &mut world).unwrap();
                }
                let snapshot = component.snapshot().unwrap();
                assert!(matches!(
                    snapshot.diagnostics.as_slice(),
                    [Diagnostic {
                        issue: Issue::UnsupportedField("shield recharge rate"),
                        ..
                    }]
                ));
                component
                    .set_instance_variable(INSTANCE, RATE, Value::Float(125.0), &mut world)
                    .unwrap();
                component.set_owner_dead(true);
                component.advance_frame(5, &mut world).unwrap();
                component.set_owner_dead(false);
                component.debug_end_state(INSTANCE, 0, &mut world).unwrap();
                component.advance_frame(6, &mut world).unwrap();
                assert_eq!(world.calls, fixture.expected, "{}", scenario.name);
            }
        }
    }
}
