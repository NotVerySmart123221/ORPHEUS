use std::sync::Arc;

use orpheus_statescript::{
    script_graph::GraphStore,
    script_init::{Component, EmptyWorld, InitFault, MAX_INIT_WORK, Root},
    script_wire::Variables,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    case_id: String,
    evidence: String,
    input: Input,
    expected: Expected,
}

#[derive(Deserialize)]
struct Input {
    graph: u32,
    bundle: Value,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    roots: Vec<Option<u16>>,
}

#[derive(Deserialize)]
struct Expected {
    cases: Vec<Outcome>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Outcome {
    Started { instances: Vec<(u16, bool)> },
    Exhausted { exhausted: bool },
    Invalid { invalid_root: u16 },
}

#[test]
fn authored_server_roots_reserve_private_ids_without_renumbering_public_roots() {
    let fixture: Contract = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/script-server-root-ids-v1.json"
    ))
    .unwrap();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.case_id, "script-server-root-ids.synthetic.v1");
    assert_eq!(fixture.evidence, "synthetic-rust");

    let input = fixture.input;
    let graphs =
        Arc::new(GraphStore::from_json(&serde_json::to_vec(&input.bundle).unwrap()).unwrap());
    assert_eq!(input.cases.len(), fixture.expected.cases.len());
    for (case, expected) in input.cases.into_iter().zip(fixture.expected.cases) {
        let roots = case
            .roots
            .into_iter()
            .map(|id| {
                id.map_or_else(
                    || Root::server_only(input.graph),
                    |id| Root::new(id, input.graph),
                )
            })
            .collect();
        let component = Component::start(
            Arc::clone(&graphs),
            701,
            Variables::new(),
            roots,
            0,
            &mut EmptyWorld,
            MAX_INIT_WORK,
        );
        match expected {
            Outcome::Started { instances } => {
                let component = component.unwrap();
                let snapshot = component.snapshot().unwrap();
                assert!(snapshot.initialization_complete(), "{}", case.name);
                let actual = snapshot
                    .instances
                    .iter()
                    .map(|(&id, instance)| (id, instance.is_server_only()))
                    .collect::<Vec<_>>();
                assert_eq!(actual, instances, "{}", case.name);
            }
            Outcome::Exhausted { exhausted } => {
                assert!(exhausted);
                assert!(
                    matches!(component, Err(InitFault::InstanceIdExhausted)),
                    "{}",
                    case.name
                );
            }
            Outcome::Invalid { invalid_root } => {
                assert!(
                    matches!(component, Err(InitFault::InvalidRoot(id)) if id == invalid_root),
                    "{}",
                    case.name
                );
            }
        }
    }
}
