use serde::Deserialize;

use super::*;

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    case_id: String,
    evidence: String,
    input: Json,
    expected: [ExpectedInstance; 2],
}

#[derive(Deserialize)]
struct ExpectedInstance {
    graph: u32,
    instance: u16,
    variables: BTreeMap<u32, i64>,
}

struct LocalOnlyWorld;

impl World for LocalOnlyWorld {
    fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
        None
    }

    fn send_message(
        &mut self,
        _site: Site,
        _target: u32,
        _message: u64,
        _parameters: &Variables,
    ) -> bool {
        panic!("local messages must not send through the World")
    }

    fn route_game_message(&mut self, _site: Site, _message: u64, _parameters: &Variables) -> bool {
        panic!("local messages must not route through the World")
    }
}

#[test]
fn authored_local_message_snapshots_parameters_and_uses_declared_receiver_order() {
    let contract: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-local-message-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.case_id, "script-local-message.synthetic.v1");
    assert_eq!(contract.evidence, "synthetic-rust");

    let bundle = serde_json::to_vec(&contract.input).unwrap();
    let graphs = GraphStore::from_json(&bundle).unwrap();
    let roots = contract
        .expected
        .iter()
        .map(|expected| Root::new(expected.instance, expected.graph))
        .collect();
    let snapshot = initialize(
        &graphs,
        4242,
        Variables::new(),
        roots,
        &mut LocalOnlyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();

    assert!(
        snapshot.diagnostics.is_empty(),
        "{:?}",
        snapshot.diagnostics
    );
    assert!(snapshot.variables.is_empty());

    // The sender overwrites its source variable in the continuation. Both
    // receivers must get the earlier value. The second receiver also reads
    // the first receiver's write, even though it has a smaller node position.
    for expected in contract.expected {
        let instance = &snapshot.instances[&expected.instance];
        assert_eq!(instance.graph, expected.graph);
        for (variable, value) in &expected.variables {
            assert_eq!(
                instance.variables.get(variable),
                Some(&Value::Integer(*value))
            );
        }
        assert_eq!(
            instance.variables.len(),
            expected.variables.len(),
            "unexpected writes in graph {}: {:?}",
            expected.graph,
            instance.variables
        );
    }
}
