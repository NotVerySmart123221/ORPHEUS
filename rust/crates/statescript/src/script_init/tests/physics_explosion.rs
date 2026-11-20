use serde::Deserialize;

use super::*;

const CLASS: &str = "STUStatescriptStatePhysicsExplosion";
const ENTITY: u32 = 749;
const INSTANCE: u16 = 1;
const TRANSITION_MARKER: u32 = 985;

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    case_id: String,
    evidence: String,
    input: Input,
}

#[derive(Deserialize)]
struct Input {
    placements: Vec<Placement>,
    fields: Json,
    contexts: Vec<ContextCase>,
}

#[derive(Deserialize)]
struct Placement {
    graph: u32,
    position: u16,
    networked: bool,
}

impl Placement {
    fn site(&self) -> Site {
        Site {
            entity: ENTITY,
            instance: INSTANCE,
            graph: self.graph,
            node: Some(self.position),
        }
    }
}

#[derive(Deserialize)]
struct ContextCase {
    has_dynamic_map_rigid_bodies: Option<bool>,
    admitted: bool,
}

struct StaticMapHost {
    site: Site,
    has_dynamic_map_rigid_bodies: Option<bool>,
}

impl World for StaticMapHost {
    fn has_dynamic_map_rigid_bodies(&self, site: Site) -> Option<bool> {
        assert_eq!(site, self.site);
        self.has_dynamic_map_rigid_bodies
    }

    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        panic!("map explosion evaluated a dormant position")
    }

    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        panic!("map explosion read a dormant entity variable")
    }

    fn native(&mut self, _: Site, _: &Json) -> Option<Value> {
        panic!("map explosion evaluated a dormant native operand")
    }

    fn queue_health_change(&mut self, _: Site, _: u32, _: f64) -> bool {
        panic!("map explosion attempted an actor health change")
    }
}

fn contract() -> Input {
    let contract: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-static-map-explosion-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.case_id, "script-static-map-explosion.synthetic.v1");
    assert_eq!(contract.evidence, "synthetic-rust");
    contract.input
}

fn start(
    placement: &Placement,
    fields: Json,
    reciprocal_remote: bool,
    world: &mut StaticMapHost,
) -> Component {
    let position = placement.position;
    let mut explosion = node(position, CLASS, Some(0), fields);
    explosion["server_only"] = json!(!placement.networked);
    if placement.networked {
        explosion["remote"] = json!(0);
    }
    let nodes = vec![
        node(
            0,
            "STUStatescriptEntry",
            None,
            json!({"m_outPlug": plug(&[(position, "m_beginPlug")], &[])}),
        ),
        node(
            1,
            "STUStatescriptActionSetVar",
            None,
            json!({"m_out_Var": var(TRANSITION_MARKER, true), "m_value": true}),
        ),
        explosion,
    ];
    let mut definition = graph(placement.graph, nodes, json!([position]), json!([0]));
    definition["items"] = json!(position + 1);
    if placement.networked && reciprocal_remote {
        definition["remote"] = json!([position]);
    }
    Component::start(
        Arc::new(store(vec![definition])),
        ENTITY,
        Variables::new(),
        vec![Root::new(INSTANCE, placement.graph)],
        0,
        world,
        MAX_INIT_WORK,
    )
    .unwrap()
}

fn assert_unsupported(body: &Component, site: Site) {
    let snapshot = body.snapshot().unwrap();
    assert!(
        matches!(
            snapshot.diagnostics.as_slice(),
            [Diagnostic {
                site: actual_site,
                issue: Issue::UnsupportedState(class),
                registration_rejected: false,
                ..
            }] if actual_site == &site && class == CLASS
        ),
        "{:?}",
        snapshot.diagnostics
    );
    assert!(active(snapshot, INSTANCE).is_empty());
}

#[test]
fn static_map_explosion_uses_ordinary_lifecycle_at_relocated_sites() {
    let input = contract();
    for placement in &input.placements {
        for case in &input.contexts {
            let mut world = StaticMapHost {
                site: placement.site(),
                has_dynamic_map_rigid_bodies: case.has_dynamic_map_rigid_bodies,
            };
            let mut body = start(placement, input.fields.clone(), true, &mut world);
            if !case.admitted {
                assert_unsupported(&body, world.site);
                continue;
            }

            let snapshot = body.snapshot().unwrap();
            assert!(
                snapshot.diagnostics.is_empty(),
                "{:?}",
                snapshot.diagnostics
            );
            assert_eq!(active(snapshot, INSTANCE), [0]);
            assert_eq!(
                snapshot.instances[&INSTANCE].states[&0].kind,
                StateKind::Plain
            );
            assert!(
                snapshot
                    .pending_timers
                    .get(&INSTANCE)
                    .is_none_or(Vec::is_empty)
            );
            body.debug_end_state(INSTANCE, 0, &mut world).unwrap();
            assert!(active(body.snapshot().unwrap(), INSTANCE).is_empty());
        }
    }
}

#[test]
fn static_map_explosion_requires_supported_fields_and_reciprocal_visibility() {
    let input = contract();
    let placement = &input.placements[0];
    assert!(placement.networked);
    let mut world = StaticMapHost {
        site: placement.site(),
        has_dynamic_map_rigid_bodies: Some(false),
    };

    let mut malformed = input.fields.clone();
    malformed["m_66D29548"] = Json::Null;
    let body = start(placement, malformed, true, &mut world);
    assert_unsupported(&body, world.site);

    let body = start(placement, input.fields, false, &mut world);
    assert_unsupported(&body, world.site);
}

#[test]
fn static_map_explosion_preserves_the_networked_base_transition() {
    let input = contract();
    let placement = &input.placements[0];
    assert!(placement.networked);
    let mut fields = input.fields;
    fields["m_transitionPlug"] = json!([plug(&[(1, "m_inPlug")], &[])]);
    let mut world = StaticMapHost {
        site: placement.site(),
        has_dynamic_map_rigid_bodies: Some(false),
    };

    let body = start(placement, fields, true, &mut world);
    let snapshot = body.snapshot().unwrap();
    assert!(
        snapshot.diagnostics.is_empty(),
        "{:?}",
        snapshot.diagnostics
    );
    assert!(active(snapshot, INSTANCE).is_empty());
    assert_eq!(
        snapshot.variables.get(&TRANSITION_MARKER),
        Some(&Value::Bool(true))
    );
}
