use serde::Deserialize;

use super::{super::machine::Event, *};

const LINKED: &str = "STU_9028E035";

#[derive(Deserialize)]
struct Contract {
    input: Input,
    expected: Expected,
    registry_only_pair: RegistryCases,
    animation_registry: AnimationRegistry,
}

#[derive(Deserialize)]
struct Input {
    roots: Vec<Example>,
    unsupported_types: Vec<Json>,
}

#[derive(Deserialize)]
struct Example {
    graph: u32,
    position: u16,
    #[serde(rename = "type")]
    kind: Json,
    server_only: bool,
}

#[derive(Deserialize)]
struct Expected {
    links_after_enter: Vec<Option<(u16, u16)>>,
    links_after_final_root_ends: Vec<Option<(u16, u16)>>,
}

#[derive(Deserialize)]
struct RegistryCases {
    roots: Vec<RegistryExample>,
    rejected: Vec<RegistryExample>,
    client_only: Vec<RegistryExample>,
    initial_links: Vec<Option<(u16, u16)>>,
}

#[derive(Deserialize)]
struct AnimationRegistry {
    roots: Vec<RegistryExample>,
    initial_links: Vec<Option<(u16, u16)>>,
}

#[derive(Deserialize)]
struct RegistryExample {
    graph: u32,
    position: u16,
    class: String,
    #[serde(default)]
    configured: serde_json::Map<String, Json>,
    #[serde(default)]
    server_only: bool,
    #[serde(default)]
    client_only: bool,
    #[serde(default)]
    has_remote_slot: bool,
}

fn contract() -> Contract {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-linked-state-lifecycle-v1.json"
    )))
    .unwrap()
}

fn start(examples: &[Example]) -> Component {
    let graphs = examples
        .iter()
        .map(|example| {
            let mut linked = node(
                example.position,
                LINKED,
                Some(0),
                json!({"m_type": example.kind}),
            );
            linked["server_only"] = json!(example.server_only);
            let entry = node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug": plug(&[(example.position, "m_beginPlug")], &[])}),
            );
            let mut graph = graph(
                example.graph,
                vec![entry, linked],
                json!([example.position]),
                json!([0]),
            );
            graph["items"] = json!(example.position + 1);
            graph
        })
        .collect();
    let roots = (1..)
        .zip(examples)
        .map(|(id, root)| Root::new(id, root.graph))
        .collect();
    Component::start(
        Arc::new(store(graphs)),
        913,
        Variables::new(),
        roots,
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap()
}

fn assert_links(component: &Component, expected: &[Option<(u16, u16)>], active_roots: usize) {
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    for (index, (instance, next)) in snapshot.instances.values().zip(expected).enumerate() {
        let state = &instance.states[&0];
        assert_eq!(state.active, index < active_roots);
        assert_eq!(state.kind, StateKind::Registered { next: *next });
    }
    assert_eq!(snapshot.instances.len(), expected.len());
}

#[test]
fn native_type_list_links_across_roots_and_skips_server_only_entries() {
    let contract = contract();
    let mut component = start(&contract.input.roots);
    assert_links(&component, &contract.expected.links_after_enter, 3);
    component
        .machine
        .queues
        .push(3, 0, Event::Abort(0))
        .unwrap();
    component.run_now(&mut EmptyWorld).unwrap();
    assert_links(
        &component,
        &contract.expected.links_after_final_root_ends,
        2,
    );
}

#[test]
fn native_type_list_keeps_unsupported_shapes_diagnostic() {
    let contract = contract();
    for kind in contract.input.unsupported_types {
        let component = start(&[Example {
            graph: contract.input.roots[0].graph,
            position: contract.input.roots[0].position,
            kind,
            server_only: false,
        }]);
        let snapshot = component.snapshot().unwrap();
        assert!(!snapshot.initialization_complete());
        assert!(matches!(
            snapshot.diagnostics.as_slice(),
            [Diagnostic {
                issue: Issue::UnsupportedState(class),
                ..
            }] if class == LINKED
        ));
    }
}

fn start_registry_pair(examples: &[RegistryExample]) -> Component {
    let mut graphs = Vec::new();
    let mut roots = Vec::new();
    for (id, example) in (1..).zip(examples) {
        let child = example.position + 1;
        let entry = node(
            0,
            "STUStatescriptEntry",
            None,
            json!({"m_outPlug": plug(&[(example.position, "m_beginPlug")], &[])}),
        );
        let mut fields = json!({
            "m_4A5CA514": {"$": "STU_B5A0CAF0"},
            "m_subgraphPlug": plug(&[(child, "m_beginPlug")], &[1]),
        });
        fields
            .as_object_mut()
            .unwrap()
            .extend(example.configured.clone());
        let mut state = node(example.position, &example.class, Some(0), fields);
        state["server_only"] = json!(example.server_only);
        state["client_only"] = json!(example.client_only);
        let wait = node(
            child,
            "STUStatescriptStateWait",
            Some(1),
            json!({"m_timeout": {"$": "STUConfigVarFloat", "m_value": 3.5}}),
        );
        if example.has_remote_slot {
            state["remote"] = json!(0);
        }
        let mut definition = graph(
            example.graph,
            vec![entry, state, wait],
            json!([example.position, child]),
            json!([0]),
        );
        definition["items"] = json!(child + 1);
        if example.has_remote_slot {
            definition["remote"] = json!([example.position]);
        }
        graphs.push(definition);
        roots.push(Root::new(id, example.graph));
    }
    Component::start(
        Arc::new(store(graphs)),
        731,
        Variables::new(),
        roots,
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap()
}

#[test]
fn resource_free_states_register_per_class_and_keep_nested_lifecycle() {
    let cases = contract().registry_only_pair;
    let mut component = start_registry_pair(&cases.roots);
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    for (id, next) in (1..).zip(cases.initial_links) {
        let states = &snapshot.instances[&id].states;
        assert_eq!(states[&0].kind, StateKind::Registered { next });
        assert!(states[&0].active && states[&1].active);
    }

    component.debug_end_state(3, 0, &mut EmptyWorld).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert_eq!(
        snapshot.instances[&1].states[&0].kind,
        StateKind::Registered { next: None }
    );
    assert_eq!(
        snapshot.instances[&2].states[&0].kind,
        StateKind::Registered { next: Some((4, 0)) }
    );
    assert!(!snapshot.instances[&3].states[&0].active);
    assert!(!snapshot.instances[&3].states[&1].active);

    component.debug_exit_instance(4, &mut EmptyWorld).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    assert!(!snapshot.instances.contains_key(&4));
    assert_eq!(
        snapshot.instances[&2].states[&0].kind,
        StateKind::Registered { next: None }
    );
    assert!(snapshot.instances[&2].states[&1].active);
}

#[test]
fn animation_blend_tree_links_follow_registration_and_retirement() {
    let cases = contract().animation_registry;
    let mut component = start_registry_pair(&cases.roots);
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot.diagnostics.is_empty(),
        "{:?}",
        snapshot.diagnostics
    );
    for (id, next) in (1..).zip(cases.initial_links) {
        let states = &snapshot.instances[&id].states;
        assert_eq!(states[&0].kind, StateKind::Registered { next });
        assert!(states[&0].active && states[&1].active);
    }

    component.debug_end_state(3, 0, &mut EmptyWorld).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert_eq!(
        snapshot.instances[&1].states[&0].kind,
        StateKind::Registered { next: Some((4, 0)) }
    );
    assert!(!snapshot.instances[&3].states[&1].active);
    component.debug_exit_instance(4, &mut EmptyWorld).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert_eq!(
        snapshot.instances[&1].states[&0].kind,
        StateKind::Registered { next: None }
    );
    assert!(snapshot.instances[&1].states[&1].active);
    assert!(
        snapshot.diagnostics.is_empty(),
        "{:?}",
        snapshot.diagnostics
    );
}

#[test]
fn resource_free_registry_requires_supported_authored_shape_and_scope() {
    let cases = contract().registry_only_pair;
    for example in &cases.rejected {
        let component = start_registry_pair(std::slice::from_ref(example));
        let snapshot = component.snapshot().unwrap();
        assert!(
            matches!(
                snapshot.diagnostics.as_slice(),
                [Diagnostic { issue: Issue::UnsupportedState(class), .. }] if class == &example.class
            ),
            "{}: {:?}",
            example.graph,
            snapshot.diagnostics
        );
        assert!(snapshot.instances[&1].states.is_empty());
    }
    for example in &cases.client_only {
        let component = start_registry_pair(std::slice::from_ref(example));
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        assert!(snapshot.instances[&1].states.is_empty());
    }
}
