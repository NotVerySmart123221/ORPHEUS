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
        direction: Option<[f64; 3]>,
        #[serde(default)]
        geometry_unavailable: bool,
        #[serde(default)]
        blocked: Vec<[f64; 3]>,
        message: Option<u64>,
        override_output: Option<Vec<u32>>,
        #[serde(default)]
        subframe_due_ms: Vec<u64>,
    }

    #[derive(Deserialize)]
    struct Input {
        bundle: Json,
        initial_direction: [f64; 3],
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
        direction: [f64; 3],
        geometry_unavailable: bool,
        blocked: Vec<[f64; 3]>,
        reads: Cell<usize>,
    }

    impl World for SpatialWorld {
        fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
            None
        }

        fn position(&mut self, entity: u32) -> Option<[f64; 3]> {
            (entity == ENTITY).then_some([0.0; 3])
        }

        fn forward(&self, entity: u32) -> Option<[f64; 3]> {
            (entity == ENTITY).then_some(self.direction)
        }

        fn spatial_candidates(&self) -> Option<&[SpatialCandidate]> {
            self.reads.set(self.reads.get() + 1);
            self.candidates.as_deref()
        }

        fn spatial_line_clear(&self, origin: [f64; 3], target: [f64; 3]) -> Option<bool> {
            assert_eq!(origin, [0.0; 3]);
            (!self.geometry_unavailable).then_some(!self.blocked.contains(&target))
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

    let mut world = SpatialWorld {
        direction: contract.input.initial_direction,
        ..SpatialWorld::default()
    };
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
        if let Some(direction) = step.direction {
            world.direction = direction;
        }
        world.geometry_unavailable = step.geometry_unavailable;
        world.blocked = step.blocked;
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
                    server_only: false,
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
fn authored_observer_recovers_when_room_entity_facts_return() {
    use crate::script_expr::RoomEntityFacts;
    use std::collections::{BTreeMap, BTreeSet};

    const GRAPH: u32 = 94;
    const INSTANCE: u16 = 1;
    const SOURCE: u32 = 61;
    const TARGET: u32 = 62;
    const TAG: u64 = 0x9;
    const OUTPUT: u32 = 81;
    const FIRED: u32 = 82;

    #[derive(Default)]
    struct FactsWorld(Option<BTreeMap<u32, RoomEntityFacts>>);

    impl World for FactsWorld {
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }

        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }

        fn room_entities(&self) -> Option<&BTreeMap<u32, RoomEntityFacts>> {
            self.0.as_ref()
        }
    }

    let input: Json = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-observe-radius-v1.json"
    )))
    .unwrap();
    let mut bundle = input["input"]["bundle"].clone();
    bundle["graphs"][0]["nodes"][1]["fields"]["value"] = json!({
        "$": "EntityMatches",
        "entity": {"$": "STUConfigVarDynamic", "m_identifier": 1, "m_60DB8F99": true},
        "filter": {
            "alive": true,
            "relation": "opponent",
            "source": {"$": "STUConfigVarDynamic", "m_identifier": 2, "m_60DB8F99": true},
            "tags": {"all": ["0x0000000000000009"]}
        },
    });
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap());
    assert!(graphs.uses_room_spatial_frame());

    let mut world = FactsWorld::default();
    let mut component = Component::start(
        graphs,
        SOURCE,
        Variables::from([
            (1, Value::Entity(TARGET)),
            (2, Value::Entity(SOURCE)),
            (FIRED, Value::Bool(false)),
        ]),
        vec![Root::new(INSTANCE, GRAPH)],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();

    let facts = |alive, tags| {
        Some(BTreeMap::from([
            (
                SOURCE,
                RoomEntityFacts {
                    team: Some(0),
                    alive: true,
                    tags: Default::default(),
                },
            ),
            (
                TARGET,
                RoomEntityFacts {
                    team: Some(1),
                    alive,
                    tags,
                },
            ),
        ]))
    };
    let granted = TargetTagEffects::default().snapshot(Some(&BTreeSet::from([TAG])));
    for (index, (sample, matched, changed)) in [
        (facts(true, granted.clone()), true, true),
        (None, true, false),
        (facts(false, granted.clone()), false, true),
        (facts(true, Default::default()), false, false),
        (facts(true, granted.clone()), true, true),
    ]
    .into_iter()
    .enumerate()
    {
        component
            .set_entity_variable(FIRED, Value::Bool(false), &mut world)
            .unwrap();
        world.0 = sample;
        component
            .advance_frame(u32::try_from(index + 1).unwrap(), &mut world)
            .unwrap();
        let snapshot = component.snapshot().unwrap();
        assert_eq!(snapshot.variables[&OUTPUT], Value::Bool(matched));
        assert_eq!(snapshot.variables[&FIRED], Value::Bool(changed));
        assert!(snapshot.initialization_complete());
    }
    let snapshot = component.snapshot().unwrap();
    assert_eq!(snapshot.diagnostics.len(), 1);
    assert_eq!(snapshot.diagnostics[0].issue, Issue::ObservationUnavailable);
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

#[test]
fn authored_health_change_admits_live_operands_before_continuing() {
    const GRAPH: u32 = 109;
    const INSTANCE: u16 = 1;
    const ENTITY: u32 = 4242;
    const MESSAGE: u64 = 0x49;
    const CONTINUED: u32 = 3;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Operand {
        Entity { entity: u32 },
        Integer(i64),
        Float(f64),
        Bool(bool),
        Nonfinite { nonfinite: String },
    }

    impl From<Operand> for Value {
        fn from(value: Operand) -> Self {
            match value {
                Operand::Entity { entity } => Self::Entity(entity),
                Operand::Integer(value) => Self::Integer(value),
                Operand::Float(value) => Self::Float(value),
                Operand::Bool(value) => Self::Bool(value),
                Operand::Nonfinite { nonfinite } => Self::Float(match nonfinite.as_str() {
                    "infinity" => f64::INFINITY,
                    "nan" => f64::NAN,
                    _ => panic!("unknown nonfinite operand {nonfinite}"),
                }),
            }
        }
    }

    #[derive(Deserialize)]
    struct Step {
        target: Operand,
        delta: Operand,
        accept: bool,
    }

    #[derive(Deserialize)]
    struct Input {
        bundle: Json,
        steps: Vec<Step>,
    }

    #[derive(Deserialize)]
    struct Expected {
        request: Option<(u32, f64)>,
        continued: bool,
        diagnostic: Option<String>,
    }

    #[derive(Deserialize)]
    struct HealthChangeContract {
        schema_version: u8,
        case_id: String,
        evidence: String,
        input: Input,
        expected: Vec<Expected>,
    }

    #[derive(Default)]
    struct HealthWorld {
        accept: bool,
        requests: Vec<(Site, u32, f64)>,
    }

    impl World for HealthWorld {
        fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
            None
        }

        fn position(&mut self, _: u32) -> Option<[f64; 3]> {
            None
        }

        fn queue_health_change(&mut self, site: Site, target: u32, delta: f64) -> bool {
            self.requests.push((site, target, delta));
            self.accept
        }
    }

    let contract: HealthChangeContract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-health-change-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.case_id, "script-health-change.synthetic.v1");
    assert_eq!(contract.evidence, "synthetic-rust");
    assert_eq!(contract.input.steps.len(), contract.expected.len());

    let bundle = serde_json::to_vec(&contract.input.bundle).unwrap();
    let graphs = Arc::new(GraphStore::from_json(&bundle).unwrap());
    let node = graphs.graph(GRAPH).unwrap().node(1).unwrap();
    assert!(node.declares_health_change_action());
    assert_eq!(node.class, "ActionHealthChange");
    assert!(node.server_only && !node.is_networked());

    let mut world = HealthWorld::default();
    let mut component = Component::start(
        graphs,
        ENTITY,
        Variables::new(),
        vec![Root::listener(INSTANCE, GRAPH, None)],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert!(world.requests.is_empty());
    let site = Site {
        entity: ENTITY,
        instance: INSTANCE,
        graph: GRAPH,
        node: Some(1),
    };
    let mut diagnostics = Vec::new();
    for (step, expected) in contract.input.steps.into_iter().zip(contract.expected) {
        world.accept = step.accept;
        for (id, value) in [
            (1, step.target.into()),
            (2, step.delta.into()),
            (CONTINUED, Value::Bool(false)),
        ] {
            component
                .set_entity_variable(id, value, &mut world)
                .unwrap();
        }
        assert_eq!(
            component
                .queue_message(INSTANCE, MESSAGE, Variables::new())
                .unwrap(),
            1
        );
        component.run_now(&mut world).unwrap();

        let request = expected
            .request
            .map(|(target, delta)| (site, target, delta));
        assert_eq!(
            std::mem::take(&mut world.requests).as_slice(),
            request.as_slice()
        );
        if let Some(diagnostic) = expected.diagnostic {
            let issue = match diagnostic.as_str() {
                "target" => Issue::Expression(EvalError::MalformedConfig(
                    "expected a bound health change target",
                )),
                "delta" => Issue::Expression(EvalError::MalformedConfig(
                    "expected a finite nonzero health change delta",
                )),
                "unavailable" => Issue::UnsupportedField("health change request"),
                _ => panic!("unknown diagnostic {diagnostic}"),
            };
            let diagnostic = Diagnostic {
                site,
                issue,
                registration_rejected: false,
                server_only: false,
            };
            if !diagnostics.contains(&diagnostic) {
                diagnostics.push(diagnostic);
            }
        }
        let snapshot = component.snapshot().unwrap();
        assert_eq!(
            snapshot.variables[&CONTINUED],
            Value::Bool(expected.continued)
        );
        assert_eq!(snapshot.diagnostics, diagnostics);
    }
}

#[test]
fn authored_interval_composes_with_live_health_requests_until_aborted() {
    const GRAPH: u32 = 113;
    const INSTANCE: u16 = 1;
    const ACTOR: u32 = 73;
    const WAKE_MESSAGE: u64 = 53;

    #[derive(Deserialize)]
    struct Step {
        frame: u32,
        active: bool,
        message: Option<u64>,
        operands: Option<(u32, f64)>,
        #[serde(default)]
        wake_offsets: Vec<u64>,
        request: Option<(u32, f64)>,
    }

    #[derive(Deserialize)]
    struct Input {
        bundle: Json,
        start_frame: u32,
        initial_operands: (u32, f64),
        steps: Vec<Step>,
        after_teardown_frame: u32,
    }

    #[derive(Deserialize)]
    struct IntervalContract {
        schema_version: u8,
        case_id: String,
        evidence: String,
        input: Input,
    }

    #[derive(Default)]
    struct RecordingWorld(Vec<(Site, u32, f64)>);

    impl World for RecordingWorld {
        fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
            None
        }

        fn position(&mut self, _: u32) -> Option<[f64; 3]> {
            None
        }

        fn queue_health_change(&mut self, site: Site, target: u32, delta: f64) -> bool {
            self.0.push((site, target, delta));
            true
        }
    }

    let contract: IntervalContract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-interval-health-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.case_id, "script-interval-health.synthetic.v1");
    assert_eq!(contract.evidence, "synthetic-rust");

    let graphs = Arc::new(
        GraphStore::from_json(&serde_json::to_vec(&contract.input.bundle).unwrap()).unwrap(),
    );
    let graph = graphs.graph(GRAPH).unwrap();
    assert!(
        graph
            .state(0)
            .is_some_and(|node| node.server_only && !node.is_networked())
    );

    let (target, delta) = contract.input.initial_operands;
    let mut world = RecordingWorld::default();
    let mut component = Component::start(
        graphs,
        ACTOR,
        Variables::from([(1, Value::Entity(target)), (2, Value::Float(delta))]),
        vec![Root::listener(INSTANCE, GRAPH, Some(0))],
        contract.input.start_frame,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert_eq!(active(component.snapshot().unwrap(), INSTANCE), [0]);
    assert!(world.0.is_empty());

    let site = Site {
        entity: ACTOR,
        instance: INSTANCE,
        graph: GRAPH,
        node: Some(1),
    };
    for step in contract.input.steps {
        if let Some((target, delta)) = step.operands {
            component
                .set_entity_variable(1, Value::Entity(target), &mut world)
                .unwrap();
            component
                .set_entity_variable(2, Value::Float(delta), &mut world)
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
        for offset in step.wake_offsets {
            let due = u64::from(step.frame) * crate::script_runtime::FRAME_MS + offset;
            assert_eq!(
                component
                    .queue_message_at(due, INSTANCE, WAKE_MESSAGE, Variables::new())
                    .unwrap(),
                1
            );
        }
        component.advance_frame(step.frame, &mut world).unwrap();
        component.run_now(&mut world).unwrap();

        let expected = step.request.map(|(target, delta)| (site, target, delta));
        assert_eq!(
            std::mem::take(&mut world.0).as_slice(),
            expected.as_slice(),
            "frame {}",
            step.frame
        );
        let snapshot = component.snapshot().unwrap();
        assert_eq!(snapshot.instances[&INSTANCE].states[&0].active, step.active);
        assert!(
            snapshot.diagnostics.is_empty(),
            "{:?}",
            snapshot.diagnostics
        );
    }

    component.teardown(&mut world).unwrap();
    component
        .advance_frame(contract.input.after_teardown_frame, &mut world)
        .unwrap();
    assert!(world.0.is_empty());
    assert!(component.snapshot().unwrap().instances.is_empty());
}

#[test]
fn authored_target_children_reuse_instances_and_clear_retired_schedules() {
    use std::{cell::Cell, collections::BTreeSet};

    use crate::script_expr::SpatialCandidate;

    const GRAPH: u32 = 115;
    const CHILD_GRAPH: u32 = 116;
    const ROOT: u16 = 1;
    const WAKE: u64 = 51;
    const CHILD_MESSAGE: u64 = 52;

    #[derive(Deserialize)]
    struct Step {
        frame: u32,
        values: Option<Vec<Json>>,
        unique_count: Option<u32>,
        candidates: Option<Vec<u32>>,
        reads: Option<usize>,
        message: Option<u64>,
        #[serde(default)]
        wake_offsets: Vec<u64>,
        active: Option<bool>,
        children: Vec<(u32, u16)>,
    }

    #[derive(Deserialize)]
    struct Case {
        name: String,
        #[serde(default)]
        radius: bool,
        steps: Vec<Step>,
    }

    #[derive(Deserialize)]
    struct Input {
        bundle: Json,
        radius_expression: Json,
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct TargetContract {
        schema_version: u8,
        case_id: String,
        evidence: String,
        input: Input,
    }

    #[derive(Default)]
    struct TargetWorld {
        reads: Cell<usize>,
        candidates: Option<Vec<SpatialCandidate>>,
    }

    impl World for TargetWorld {
        fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
            None
        }

        fn position(&mut self, _: u32) -> Option<[f64; 3]> {
            None
        }

        fn spatial_candidates(&self) -> Option<&[SpatialCandidate]> {
            self.reads.set(self.reads.get() + 1);
            self.candidates.as_deref()
        }
    }

    let contract: TargetContract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-target-children-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.case_id, "script-target-children.synthetic.v1");
    assert_eq!(contract.evidence, "synthetic-rust");

    for case in contract.input.cases {
        let mut bundle = contract.input.bundle.clone();
        if case.radius {
            bundle["graphs"][0]["nodes"][0]["fields"]["targets"] =
                contract.input.radius_expression.clone();
        }
        let graphs = GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap();
        let mut world = TargetWorld::default();
        let mut component = Component::start(
            Arc::new(graphs),
            91,
            Variables::from([
                (1, Value::Array(vec![Value::Entity(99)])),
                (2, Value::Vector([0.0; 3])),
            ]),
            vec![Root::listener(ROOT, GRAPH, Some(0))],
            0,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        assert_eq!(component.snapshot().unwrap().instances.len(), 1);
        assert_eq!(world.reads.get(), 0);
        let mut seen_children = BTreeSet::new();

        for step in case.steps {
            world.reads.set(0);
            world.candidates = step.candidates.map(|entities| {
                entities
                    .into_iter()
                    .map(|entity| SpatialCandidate {
                        entity,
                        position: [1.0, 0.0, 0.0],
                        team: None,
                    })
                    .collect()
            });
            if let Some(values) = step.values {
                let values = values
                    .into_iter()
                    .map(|value| match value {
                        Json::Bool(value) => Value::Bool(value),
                        value => Value::Entity(u32::try_from(value.as_u64().unwrap()).unwrap()),
                    })
                    .collect();
                component
                    .set_entity_variable(1, Value::Array(values), &mut world)
                    .unwrap();
            }
            if let Some(count) = step.unique_count {
                let values = (100..100 + count).map(Value::Entity).collect();
                component
                    .set_entity_variable(1, Value::Array(values), &mut world)
                    .unwrap();
            }
            if let Some(message) = step.message {
                assert_eq!(
                    component
                        .queue_frame_message(step.frame, ROOT, message, Variables::new())
                        .unwrap(),
                    1
                );
            }
            for offset in step.wake_offsets {
                let due = u64::from(step.frame) * crate::script_runtime::FRAME_MS + offset;
                assert_eq!(
                    component
                        .queue_message_at(due, ROOT, WAKE, Variables::new())
                        .unwrap(),
                    1
                );
            }
            component.advance_frame(step.frame, &mut world).unwrap();
            if let Some(reads) = step.reads {
                assert_eq!(
                    world.reads.get(),
                    reads,
                    "{} frame {}",
                    case.name,
                    step.frame
                );
            }

            let snapshot = component.snapshot().unwrap();
            assert_eq!(
                snapshot.instances[&ROOT].states[&0].active,
                step.active.unwrap_or(true)
            );
            let mut children = Vec::new();
            for (&id, instance) in &snapshot.instances {
                if instance.graph == CHILD_GRAPH {
                    let Value::Entity(target) = instance.variables[&7] else {
                        panic!("target was not bound before child startup");
                    };
                    assert_eq!(instance.variables[&8], Value::Entity(target));
                    assert!(instance.is_server_only());
                    assert!(instance.states[&0].active);
                    assert!(!snapshot.pending_timers[&id].is_empty());
                    children.push((target, id));
                }
            }
            children.sort_unstable();
            assert_eq!(
                children, step.children,
                "{} frame {}",
                case.name, step.frame
            );

            let live_children: BTreeSet<_> = children.into_iter().map(|(_, id)| id).collect();
            for &child in &live_children {
                if seen_children.insert(child) {
                    assert_eq!(
                        component
                            .queue_frame_message(200, child, CHILD_MESSAGE, Variables::new())
                            .unwrap(),
                        1
                    );
                }
            }
            for child in seen_children.difference(&live_children) {
                assert!(!component.machine.ticking.contains_key(child));
                assert!(component.machine.queues.iter(*child).next().is_none());
            }
        }

        component.teardown(&mut world).unwrap();
        assert!(component.snapshot().unwrap().instances.is_empty());
        assert!(component.machine.ticking.is_empty());
        assert!(component.machine.queues.next_due().is_none());
    }
}

#[derive(Deserialize)]
struct RoomMessageContract {
    schema_version: u8,
    case_id: String,
    evidence: String,
    input: RoomMessageInput,
    expected: RoomMessageExpected,
}

#[derive(Deserialize)]
struct RoomMessageInput {
    bundle: Json,
    source_entity: u32,
    target_entity: u32,
    source_graph: u32,
    target_graph: u32,
    trigger: u64,
    frame: u32,
    integer: i64,
    vector: [f64; 3],
}

#[derive(Deserialize)]
struct RoomMessageExpected {
    continuation_value: i64,
    room_receiver_count: usize,
    ordinary_receiver_count: usize,
    integer_variable: u32,
    vector_variable: u32,
    ordered_read_variable: u32,
    sender_variable: u32,
    ordinary_variable: u32,
}

fn room_message_contract() -> RoomMessageContract {
    let contract: RoomMessageContract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-room-message-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.case_id, "script-room-message.synthetic.v1");
    assert_eq!(contract.evidence, "synthetic-rust");
    contract
}

struct RoomMessageWorld {
    accept: bool,
    requests: Vec<(Site, u32, RoomMessage)>,
}

impl World for RoomMessageWorld {
    fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
        None
    }

    fn queue_room_message(&mut self, site: Site, target: u32, message: RoomMessage) -> bool {
        self.requests.push((site, target, message));
        self.accept
    }
}

#[test]
fn authored_room_messages_copy_typed_parameters_and_wait_for_the_receiver_frame() {
    const ROOT: u16 = 1;
    const INTEGER_SLOT: u32 = 45;
    const VECTOR_SLOT: u32 = 46;
    let RoomMessageContract {
        input, expected, ..
    } = room_message_contract();
    let graphs =
        Arc::new(GraphStore::from_json(&serde_json::to_vec(&input.bundle).unwrap()).unwrap());
    assert!(graphs.uses_room_spatial_frame());
    let cases = [
        (
            Value::Entity(input.target_entity),
            Value::Vector(input.vector),
            true,
            true,
        ),
        (
            Value::Entity(input.target_entity),
            Value::Vector(input.vector),
            false,
            true,
        ),
        (
            Value::Integer(i64::from(input.target_entity)),
            Value::Vector(input.vector),
            true,
            false,
        ),
        (
            Value::Entity(input.source_entity),
            Value::Vector(input.vector),
            true,
            false,
        ),
        (
            Value::Entity(input.target_entity),
            Value::Array(Vec::new()),
            true,
            false,
        ),
        (
            Value::Entity(input.target_entity),
            Value::Vector([f64::NAN, 0.0, 0.0]),
            true,
            false,
        ),
    ];
    let mut message = None;
    for (target, vector, accept, requested) in cases {
        let mut world = RoomMessageWorld {
            accept,
            requests: Vec::new(),
        };
        let mut source = Component::start(
            Arc::clone(&graphs),
            input.source_entity,
            Variables::from([(1, target), (2, Value::Integer(input.integer)), (3, vector)]),
            vec![Root::new(ROOT, input.source_graph)],
            0,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        assert_eq!(
            source
                .queue_message(ROOT, input.trigger, Variables::new())
                .unwrap(),
            1
        );
        source.run_now(&mut world).unwrap();
        let state = source.snapshot().unwrap();
        let continued = accept && requested;
        let remaining = if continued {
            expected.continuation_value
        } else {
            input.integer
        };
        assert_eq!(state.variables[&2], Value::Integer(remaining));
        assert_eq!(state.diagnostics.len(), usize::from(!continued));
        assert_eq!(world.requests.len(), usize::from(requested));
        if let Some((site, target, envelope)) = world.requests.pop() {
            assert_eq!(site.entity, input.source_entity);
            assert_eq!((site.graph, site.node), (input.source_graph, Some(1)));
            assert_eq!(target, input.target_entity);
            assert_eq!(
                envelope.parameters,
                Variables::from([
                    (INTEGER_SLOT, Value::Integer(input.integer)),
                    (VECTOR_SLOT, Value::Vector(input.vector)),
                ])
            );
            if continued {
                message = Some(envelope);
            }
        }
    }

    let message = message.unwrap();
    let mut receiver = Component::start(
        graphs,
        input.target_entity,
        Variables::new(),
        vec![Root::new(ROOT, input.target_graph)],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    receiver
        .advance_frame(input.frame, &mut EmptyWorld)
        .unwrap();
    assert_eq!(
        receiver
            .queue_room_message_from(input.target_entity, message.clone())
            .unwrap(),
        0
    );
    assert_eq!(
        receiver
            .queue_room_message_from(input.source_entity, message.clone())
            .unwrap(),
        expected.room_receiver_count
    );
    receiver.run_now(&mut EmptyWorld).unwrap();
    assert!(
        receiver.snapshot().unwrap().instances[&ROOT]
            .variables
            .is_empty()
    );
    receiver
        .advance_frame(input.frame + 1, &mut EmptyWorld)
        .unwrap();

    let snapshot = receiver.snapshot().unwrap();
    assert!(snapshot.diagnostics.is_empty());
    let variables = &snapshot.instances[&ROOT].variables;
    assert_eq!(
        variables[&expected.integer_variable],
        Value::Integer(input.integer)
    );
    assert_eq!(
        variables[&expected.vector_variable],
        Value::Vector(input.vector)
    );
    assert_eq!(
        variables[&expected.ordered_read_variable],
        Value::Integer(input.integer)
    );
    assert_eq!(
        variables[&expected.sender_variable],
        Value::Entity(input.source_entity)
    );
    assert!(!variables.contains_key(&expected.ordinary_variable));

    assert_eq!(
        receiver
            .queue_frame_message_from(
                input.frame + 2,
                input.source_entity,
                message.id,
                Variables::from([(INTEGER_SLOT, Value::Integer(3))]),
            )
            .unwrap(),
        expected.ordinary_receiver_count
    );
    receiver
        .advance_frame(input.frame + 2, &mut EmptyWorld)
        .unwrap();
    let variables = &receiver.snapshot().unwrap().instances[&ROOT].variables;
    assert_eq!(variables[&expected.ordinary_variable], Value::Integer(3));
    assert_eq!(
        variables[&expected.integer_variable],
        Value::Integer(input.integer)
    );
    let occupied = crate::script_init::MAX_PENDING_EVENTS - 1;
    let queued_at = frame_start(input.frame + 5);
    for _ in 0..occupied {
        receiver
            .machine
            .queues
            .push(
                ROOT,
                queued_at,
                crate::script_init::machine::Event::Finish(0),
            )
            .unwrap();
    }
    assert!(matches!(
        receiver.queue_room_message_from(input.source_entity, message),
        Err(InitFault::PendingEvents { .. })
    ));
    assert_eq!(receiver.machine.queues.iter(ROOT).count(), occupied);
}

#[test]
fn authored_room_messages_require_explicit_server_receiver_and_bounded_sender() {
    let original = room_message_contract().input.bundle;
    let mut invalid = original.clone();
    invalid["graphs"][1]["nodes"][0]["server_only"] = Json::Bool(false);
    let error = GraphStore::from_json(&serde_json::to_vec(&invalid).unwrap()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("room message entry in graph 202 at node 2")
    );
    invalid = original.clone();
    invalid["graphs"][0]["nodes"][1]["fields"]["m_target"] = Json::Null;
    let error = GraphStore::from_json(&serde_json::to_vec(&invalid).unwrap()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("room message action in graph 201 at node 1")
    );
    invalid = original;
    invalid["graphs"][0]["nodes"][1]["fields"]["m_params"] = Json::Array(
        (1..=17)
            .map(|slot| serde_json::json!({"m_B5051BCE": format!("0x{slot:016X}"), "m_value": 1}))
            .collect(),
    );
    let error = GraphStore::from_json(&serde_json::to_vec(&invalid).unwrap()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("at most 16 unique parameter slots")
    );
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
