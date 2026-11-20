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
    empty_branch: Option<u16>,
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
    let graphs = Arc::new(GraphStore::from_json(&bundle).unwrap());
    let roots = contract
        .expected
        .iter()
        .map(|expected| Root::new(expected.instance, expected.graph))
        .collect();
    let mut component = Component::start(
        Arc::clone(&graphs),
        4242,
        Variables::new(),
        roots,
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    for expected in &contract.expected {
        if let Some(position) = expected.empty_branch {
            component
                .run(&mut EmptyWorld, |vm| {
                    let branch = vm.graph(expected.instance).node(position).unwrap();
                    assert!(!vm.run_node(expected.instance, branch, None, 0)?);
                    vm.run()
                })
                .unwrap();
        }
    }
    let snapshot = component.snapshot().unwrap();

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
        assert_eq!(active(snapshot, expected.instance), [expected.active_state]);
        let state = &instance.states[&expected.active_state];
        assert_eq!(state.kind, StateKind::Plain);
        assert_eq!(state.activation, 1);

        // Each chosen branch reads the value left by the preceding outputs.
        // Reversing the authored links would change the value even if all ran.
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

#[test]
fn authored_observer_samples_radius_once_per_frame_until_aborted() {
    use std::cell::Cell;

    use crate::script_expr::SpatialCandidate;

    const GRAPH: u32 = 94;
    const INSTANCE: u16 = 1;
    const ENTITY: u32 = 4242;
    const SOURCE: u32 = 61;
    const OUTPUT: u32 = 81;
    const FIRED: u32 = 82;
    const SUBFRAME_MESSAGE: u64 = 13;

    type CandidateRow = (u32, [f64; 3], Option<u8>);

    #[derive(Deserialize)]
    struct Step {
        frame: u32,
        candidates: Option<Vec<CandidateRow>>,
        message: Option<u64>,
        override_output: Option<Vec<u32>>,
        #[serde(default)]
        subframe_due_ms: Vec<u64>,
    }

    #[derive(Deserialize)]
    struct Input {
        bundle: Json,
        common_candidates: Vec<CandidateRow>,
        steps: Vec<Step>,
    }

    #[derive(Deserialize)]
    struct Expected {
        reads: usize,
        output: Option<Vec<u32>>,
        fired: bool,
        active: bool,
        unavailable: bool,
    }

    #[derive(Deserialize)]
    struct ObservationContract {
        schema_version: u8,
        case_id: String,
        evidence: String,
        input: Input,
        expected: Vec<Expected>,
    }

    #[derive(Default)]
    struct SpatialWorld {
        candidates: Option<Vec<SpatialCandidate>>,
        reads: Cell<usize>,
    }

    impl World for SpatialWorld {
        fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
            None
        }

        fn position(&mut self, entity: u32) -> Option<[f64; 3]> {
            (entity == ENTITY).then_some([0.0; 3])
        }

        fn spatial_candidates(&self) -> Option<&[SpatialCandidate]> {
            self.reads.set(self.reads.get() + 1);
            self.candidates.as_deref()
        }
    }

    let contract: ObservationContract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-observe-radius-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.case_id, "script-observe-radius.synthetic.v1");
    assert_eq!(contract.evidence, "synthetic-rust");
    assert_eq!(contract.input.steps.len(), contract.expected.len());

    let bundle = serde_json::to_vec(&contract.input.bundle).unwrap();
    let graphs = Arc::new(GraphStore::from_json(&bundle).unwrap());
    let node = graphs.graph(GRAPH).unwrap().node(1).unwrap();
    assert!(node.server_only && !node.is_networked());

    let mut world = SpatialWorld::default();
    let mut component = Component::start(
        graphs,
        ENTITY,
        Variables::from([(1, Value::Entity(ENTITY)), (2, Value::Entity(SOURCE))]),
        vec![Root::new(INSTANCE, GRAPH)],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert_eq!(world.reads.get(), 0);
    assert!(component.snapshot().unwrap().diagnostics.is_empty());
    let entities = |ids: Vec<u32>| Value::Array(ids.into_iter().map(Value::Entity).collect());

    let common_candidates = contract.input.common_candidates;
    for (step, expected) in contract.input.steps.into_iter().zip(contract.expected) {
        let before = world.reads.get();
        world.candidates = step.candidates.map(|rows| {
            rows.into_iter()
                .chain(common_candidates.iter().copied())
                .map(|(entity, position, team)| SpatialCandidate {
                    entity,
                    position,
                    team,
                })
                .collect()
        });
        component
            .set_entity_variable(FIRED, Value::Bool(false), &mut world)
            .unwrap();
        if let Some(sentinel) = step.override_output {
            component
                .set_entity_variable(OUTPUT, entities(sentinel), &mut world)
                .unwrap();
        }
        if let Some(message) = step.message {
            assert_eq!(
                component
                    .queue_frame_message(step.frame, INSTANCE, message, Variables::new())
                    .unwrap(),
                1
            );
        }
        for due in step.subframe_due_ms {
            assert_eq!(
                component
                    .queue_message_at(due, INSTANCE, SUBFRAME_MESSAGE, Variables::new())
                    .unwrap(),
                1
            );
        }
        component.advance_frame(step.frame, &mut world).unwrap();
        assert_eq!(
            world.reads.get() - before,
            expected.reads,
            "frame {}",
            step.frame
        );

        let snapshot = component.snapshot().unwrap();
        assert_eq!(
            snapshot.variables.get(&OUTPUT),
            expected.output.map(entities).as_ref()
        );
        assert_eq!(snapshot.variables[&FIRED], Value::Bool(expected.fired));
        assert_eq!(
            snapshot.instances[&INSTANCE].states[&0].active,
            expected.active
        );
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        if expected.unavailable {
            assert_eq!(
                snapshot.diagnostics,
                [Diagnostic {
                    site: Site {
                        entity: ENTITY,
                        instance: INSTANCE,
                        graph: GRAPH,
                        node: Some(1),
                    },
                    issue: Issue::ObservationUnavailable,
                    registration_rejected: false,
                }]
            );
        } else {
            assert!(
                snapshot.diagnostics.is_empty(),
                "{:?}",
                snapshot.diagnostics
            );
        }
    }
}

#[test]
fn authored_spatial_operands_read_live_world_data_on_each_message() {
    const SUBJECT: u32 = 27;
    const SAMPLE: u64 = 0x29;

    struct PoseWorld {
        position: [f64; 3],
        direction: [f64; 3],
    }

    impl World for PoseWorld {
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }

        fn position(&mut self, entity: u32) -> Option<[f64; 3]> {
            (entity == SUBJECT).then_some(self.position)
        }

        fn forward(&self, entity: u32) -> Option<[f64; 3]> {
            (entity == SUBJECT).then_some(self.direction)
        }
    }

    let write_sample = |position, class, destination| {
        node(
            position,
            "STUStatescriptActionSetVar",
            None,
            json!({
                "m_out_Var": var(destination, true),
                "m_value": {"$": class, "entity": var(1, true)},
            }),
        )
    };
    let fixture = graph(
        42,
        vec![
            node(
                0,
                "STUStatescriptEntryGameMessage",
                None,
                json!({
                    "m_gameMessage": {"m_gameMessage": format!("0x{SAMPLE:016X}")},
                    "m_outPlug": plug(&[(1, "m_inPlug"), (2, "m_inPlug")], &[]),
                }),
            ),
            write_sample(1, "EntityPosition", 2),
            write_sample(2, "EntityDirection", 3),
        ],
        json!([]),
        json!([0]),
    );
    let mut world = PoseWorld {
        position: [0.0; 3],
        direction: [0.0, 0.0, 1.0],
    };
    let mut component = Component::start(
        Arc::new(store(vec![fixture])),
        4242,
        Variables::from([(1, Value::Entity(SUBJECT))]),
        vec![Root::listener(1, 42, None)],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();

    for (position, direction) in [
        ([0.0; 3], [0.0, 0.0, 1.0]),
        ([2.0, -3.0, 5.0], [1.0, 0.0, 0.0]),
    ] {
        world.position = position;
        world.direction = direction;
        assert_eq!(
            component
                .queue_message(1, SAMPLE, Variables::new())
                .unwrap(),
            1
        );
        component.run_now(&mut world).unwrap();

        let snapshot = component.snapshot().unwrap();
        assert_eq!(snapshot.variables.get(&2), Some(&Value::Vector(position)));
        assert_eq!(snapshot.variables.get(&3), Some(&Value::Vector(direction)));
        assert!(
            snapshot.diagnostics.is_empty(),
            "{:?}",
            snapshot.diagnostics
        );
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
