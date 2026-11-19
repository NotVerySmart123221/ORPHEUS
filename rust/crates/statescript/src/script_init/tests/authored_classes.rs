use serde::Deserialize;

use super::*;

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    case_id: String,
    evidence: String,
    input: Json,
    expected: [ExpectedGraph; 2],
}

#[derive(Deserialize)]
struct ExpectedGraph {
    graph: u32,
    instance: u16,
    active_state: u16,
    server_targets: Vec<u16>,
    owner_positions: Vec<u16>,
    remote_positions: Vec<u16>,
    server_only_nodes: Vec<u16>,
    client_only_nodes: Vec<u16>,
    variables: BTreeMap<u32, i64>,
    absent_variables: Vec<u32>,
}

#[test]
fn authored_class_aliases_apply_across_graphs_without_network_slots() {
    let contract: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-authored-classes-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.case_id, "script-authored-classes.synthetic.v1");
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
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();

    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    assert!(snapshot.diagnostics.is_empty());
    assert!(
        snapshot.variables.is_empty(),
        "writes must stay instance-local"
    );
    let observations = snapshot.node_observations();
    assert_eq!(observations.is_some(), cfg!(feature = "node-observations"));
    let mut matched_sites = 0;
    for expected in contract.expected {
        if let Some(observations) = observations {
            assert!(!observations.truncated());
            let (server_targets, client_targets): (Vec<_>, Vec<_>) = observations
                .sites()
                .filter(|(graph, _, _)| *graph == expected.graph)
                .partition(|(_, _, client_scope_skipped)| !client_scope_skipped);
            let positions = |sites: Vec<(u32, u16, bool)>| {
                sites
                    .into_iter()
                    .map(|(_, node, _)| node)
                    .collect::<Vec<_>>()
            };
            matched_sites += server_targets.len() + client_targets.len();
            assert_eq!(positions(server_targets), expected.server_targets);
            assert_eq!(positions(client_targets), expected.client_only_nodes);
        }
        let graph = graphs.graph(expected.graph).unwrap();
        assert_eq!(graph.owner_positions(), expected.owner_positions);
        assert_eq!(graph.remote_positions(), expected.remote_positions);
        for position in expected.server_only_nodes {
            let node = graph.node(position).unwrap();
            assert!(node.server_only && !node.client_only && !node.is_networked());
        }
        for position in expected.client_only_nodes {
            let node = graph.node(position).unwrap();
            assert!(node.client_only && !node.server_only && !node.is_networked());
        }

        let instance = &snapshot.instances[&expected.instance];
        assert_eq!(instance.graph, expected.graph);
        assert_eq!(
            active(&snapshot, expected.instance),
            [expected.active_state]
        );
        let state = &instance.states[&expected.active_state];
        assert_eq!(state.kind, StateKind::Plain);
        assert_eq!(state.activation, 1);

        // The second output reads the first output's write. Reversing the
        // authored links would change its value even if both actions ran.
        for (variable, value) in expected.variables {
            assert_eq!(
                instance.variables.get(&variable),
                Some(&Value::Integer(value))
            );
        }
        for variable in expected.absent_variables {
            assert!(!instance.variables.contains_key(&variable));
        }
    }
    if let Some(observations) = observations {
        assert_eq!(observations.sites().count(), matched_sites);
    }
}

#[cfg(feature = "node-observations")]
#[test]
fn node_observation_limit_counts_only_distinct_sites() {
    let mut observations = NodeObservations::default();
    for node in 0..128 {
        observations.record(42, node, false);
    }
    observations.record(42, 0, false);
    assert!(!observations.truncated());
    assert_eq!(observations.sites().count(), 128);

    observations.record(77, 0, true);
    assert!(observations.truncated());
    assert_eq!(observations.sites().count(), 128);
    assert!(
        observations
            .sites()
            .all(|(graph, _, skipped)| graph == 42 && !skipped)
    );
}
