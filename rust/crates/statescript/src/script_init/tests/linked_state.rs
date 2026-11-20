use serde::Deserialize;

use super::{super::machine::Event, *};

const LINKED: &str = "STU_9028E035";

#[derive(Deserialize)]
struct Contract {
    input: Input,
    expected: Expected,
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
