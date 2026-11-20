use std::cell::Cell;

use serde::Deserialize;

use crate::script_expr::{EvalError, SpatialCandidate};

use super::*;

const GRAPH: u32 = 94;
const INSTANCE: u16 = 1;
const ENTITY: u32 = 824;
const OUTPUT: u32 = 81;
const CHANGED: u32 = 82;

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    evidence: String,
    input: Input,
    expected: Vec<Expected>,
}

#[derive(Deserialize)]
struct Input {
    center: Json,
    malformed_center: Json,
    radius: f64,
    candidates: Vec<(u32, [f64; 3])>,
    eye_positions: Vec<Option<[f64; 3]>>,
}

#[derive(Deserialize)]
struct Expected {
    targets: Option<Vec<u32>>,
    changed: bool,
}

struct EyeWorld {
    position: Option<[f64; 3]>,
    reads: Cell<usize>,
    candidates: Vec<SpatialCandidate>,
}

impl World for EyeWorld {
    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }

    fn body_eye_position(&self, entity: u32) -> Option<[f64; 3]> {
        assert_eq!(entity, ENTITY);
        self.reads.set(self.reads.get() + 1);
        self.position
    }

    fn spatial_candidates(&self) -> Option<&[SpatialCandidate]> {
        Some(&self.candidates)
    }
}

#[test]
fn authored_radius_observer_recovers_when_eye_position_returns() {
    let contract: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-observe-eye-position-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.evidence, "synthetic-rust");
    assert_eq!(contract.input.eye_positions.len(), contract.expected.len());

    let observe: Json = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-observe-radius-v1.json"
    )))
    .unwrap();
    let start = |center, world: &mut EyeWorld| {
        let mut bundle = observe["input"]["bundle"].clone();
        bundle["graphs"][0]["nodes"][1]["fields"]["value"] = json!({
            "$": "EntitiesInRadius", "center": center, "radius": contract.input.radius,
        });
        let graphs = GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap();
        assert!(graphs.uses_room_spatial_frame());
        Component::start(
            Arc::new(graphs),
            ENTITY,
            Variables::new(),
            vec![Root::new(INSTANCE, GRAPH)],
            0,
            world,
            MAX_INIT_WORK,
        )
        .unwrap()
    };
    let mut world = EyeWorld {
        position: None,
        reads: Cell::new(0),
        candidates: contract
            .input
            .candidates
            .iter()
            .map(|&(entity, position)| SpatialCandidate {
                entity,
                position,
                team: None,
            })
            .collect(),
    };
    let mut component = start(contract.input.center.clone(), &mut world);
    for (index, (&position, expected)) in contract
        .input
        .eye_positions
        .iter()
        .zip(contract.expected)
        .enumerate()
    {
        world.position = position;
        component
            .set_entity_variable(CHANGED, Value::Bool(false), &mut world)
            .unwrap();
        component
            .advance_frame(u32::try_from(index + 1).unwrap(), &mut world)
            .unwrap();

        let snapshot = component.snapshot().unwrap();
        let targets = expected
            .targets
            .map(|targets| Value::Array(targets.into_iter().map(Value::Entity).collect()));
        assert_eq!(snapshot.variables.get(&OUTPUT), targets.as_ref());
        assert_eq!(snapshot.variables[&CHANGED], Value::Bool(expected.changed));
        assert_eq!(world.reads.get(), index + 1);
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(snapshot.diagnostics.len(), 1);
        assert_eq!(snapshot.diagnostics[0].issue, Issue::ObservationUnavailable);
        assert!(snapshot.instances[&INSTANCE].states[&0].active);
    }

    let reads = world.reads.get();
    let mut malformed = start(contract.input.malformed_center.clone(), &mut world);
    malformed.advance_frame(1, &mut world).unwrap();
    let snapshot = malformed.snapshot().unwrap();
    assert!(matches!(
        snapshot.diagnostics.as_slice(),
        [Diagnostic {
            issue: Issue::Expression(EvalError::MalformedConfig(_)),
            ..
        }]
    ));
    assert!(!snapshot.initialization_complete());
    assert!(!snapshot.variables.contains_key(&OUTPUT));
    assert_eq!(world.reads.get(), reads);
}
