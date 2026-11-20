use serde::Deserialize;

use super::*;

const CLASS: &str = "STU_33A46033";

#[derive(Deserialize)]
struct Contract {
    input: Input,
}

#[derive(Deserialize)]
struct Input {
    owner: u32,
    allowed_body: u32,
    active_roots: Vec<Example>,
    rejected_roots: Vec<Example>,
}

#[derive(Deserialize)]
struct Example {
    graph: u32,
    position: u16,
    rule: Json,
    #[serde(default)]
    other_rules: serde_json::Map<String, Json>,
    #[serde(default)]
    isolated: bool,
    #[serde(default)]
    without_remote_slot: bool,
    #[serde(default)]
    needs_context: bool,
}

struct ProtectedModel {
    allowed_body: u32,
    writes: Vec<(Site, bool)>,
}

impl World for ProtectedModel {
    fn supports_static_model_protection(&self, site: Site) -> bool {
        site.entity == self.allowed_body
    }

    fn set_encounter_model_enabled(&mut self, site: Site, enabled: bool) -> bool {
        self.writes.push((site, enabled));
        true
    }

    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }
}

fn contract() -> Input {
    serde_json::from_str::<Contract>(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-static-model-protection-v1.json"
    )))
    .unwrap()
    .input
}

fn start(examples: &[Example], owner: u32, world: &mut ProtectedModel) -> Component {
    let mut graphs = Vec::new();
    let mut roots = Vec::new();
    for (id, example) in (1..).zip(examples) {
        let position = example.position;
        let child = position + 1;
        let entry = node(
            0,
            "STUStatescriptEntry",
            None,
            json!({"m_outPlug": plug(&[(position, "m_beginPlug")], &[])}),
        );
        let mut fields = json!({
            "m_7E42F868": {"$": "STUConfigVarBool", "m_value": example.rule},
            "m_subgraphPlug": plug(&[(child, "m_beginPlug")], &[1]),
        });
        fields
            .as_object_mut()
            .unwrap()
            .extend(example.other_rules.clone());
        let mut protection = node(position, CLASS, Some(0), fields);
        let wait = node(
            child,
            "STUStatescriptStateWait",
            Some(1),
            json!({"m_timeout": {"$": "STUConfigVarFloat", "m_value": 7.5}}),
        );
        if !example.without_remote_slot {
            protection["remote"] = json!(0);
        }
        let mut definition = graph(
            example.graph,
            vec![entry, protection, wait],
            json!([position, child]),
            json!([0]),
        );
        definition["items"] = json!(child + 1);
        if !example.without_remote_slot {
            definition["remote"] = json!([position]);
        }
        graphs.push(definition);
        roots.push(if example.isolated {
            Root::server_only(example.graph)
        } else {
            Root::new(id, example.graph)
        });
    }
    Component::start(
        Arc::new(store(graphs)),
        owner,
        Variables::new(),
        roots,
        0,
        world,
        MAX_INIT_WORK,
    )
    .unwrap()
}

#[test]
fn static_model_protection_uses_host_and_clears_only_after_last_claim() {
    let input = contract();
    let mut world = ProtectedModel {
        allowed_body: input.allowed_body,
        writes: Vec::new(),
    };
    let mut component = start(&input.active_roots, input.owner, &mut world);
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    let first = Site {
        entity: input.owner,
        instance: 1,
        graph: input.active_roots[0].graph,
        node: Some(input.active_roots[0].position),
    };
    let second = Site {
        instance: 2,
        graph: input.active_roots[1].graph,
        node: Some(input.active_roots[1].position),
        ..first
    };
    assert_eq!(world.writes, [(first, false), (second, false)]);
    assert_eq!(
        snapshot.instances[&1].states[&0].kind,
        StateKind::Registered { next: Some((2, 0)) }
    );
    for id in [1, 2] {
        assert!(snapshot.instances[&id].states[&1].active);
    }

    component.debug_end_state(1, 0, &mut world).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert!(!snapshot.instances[&1].states[&1].active);
    assert!(snapshot.instances[&2].states[&1].active);
    assert_eq!(world.writes, [(first, false), (second, false)]);
    component.debug_exit_instance(2, &mut world).unwrap();
    assert_eq!(
        world.writes,
        [(first, false), (second, false), (second, true)]
    );
}

#[test]
fn static_model_protection_refuses_unknown_host_and_unsupported_shapes_before_begin() {
    let input = contract();
    let mut world = ProtectedModel {
        allowed_body: input.allowed_body,
        writes: Vec::new(),
    };
    let wrong_body = start(&input.active_roots[..1], input.owner + 1, &mut world);
    let snapshot = wrong_body.snapshot().unwrap();
    assert!(world.writes.is_empty());
    assert!(snapshot.instances[&1].states.is_empty());
    assert!(matches!(
        snapshot.diagnostics.as_slice(),
        [Diagnostic {
            issue: Issue::UnsupportedField("static model protection context"),
            ..
        }]
    ));

    for example in &input.rejected_roots {
        let component = start(std::slice::from_ref(example), input.owner, &mut world);
        let snapshot = component.snapshot().unwrap();
        assert!(world.writes.is_empty());
        assert!(
            snapshot
                .instances
                .values()
                .all(|instance| instance.states.is_empty())
        );
        let expected = if example.needs_context {
            Issue::UnsupportedField("static model protection context")
        } else {
            Issue::UnsupportedState(CLASS.into())
        };
        assert!(
            matches!(snapshot.diagnostics.as_slice(), [diagnostic] if diagnostic.issue == expected),
            "{}: {:?}",
            example.graph,
            snapshot.diagnostics
        );
    }
}
