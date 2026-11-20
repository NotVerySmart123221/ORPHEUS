use std::cell::Cell;

use serde::Deserialize;

use super::*;

const GRAPH: u32 = 761;
const ENTITY: u32 = 614;
const AIRBORNE: u32 = 831;
const TRACE: u32 = 833;
const RISE: i64 = 1;
const FALL: i64 = -1;

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    evidence: String,
    input: Input,
}

#[derive(Deserialize)]
struct Input {
    steps: Vec<Step>,
    end_sample: Sample,
}

#[derive(Deserialize)]
struct Step {
    frame: u32,
    sample: Sample,
    expected_edges: Vec<(i64, bool)>,
}

#[derive(Deserialize)]
struct Sample {
    flags: u16,
    air_ticks: u32,
    pose_frame: Option<u32>,
}

impl From<&Sample> for MovementSample {
    fn from(sample: &Sample) -> Self {
        Self {
            flags: sample.flags,
            air_ticks: sample.air_ticks,
            pose_frame: sample.pose_frame,
        }
    }
}

#[derive(Default)]
struct SampleWorld {
    sample: Option<MovementSample>,
    reads: Cell<usize>,
}

impl World for SampleWorld {
    fn movement_sample(&self, site: Site) -> Option<MovementSample> {
        assert_eq!(
            (site.entity, site.graph, site.node),
            (ENTITY, GRAPH, Some(1))
        );
        self.reads.set(self.reads.get() + 1);
        self.sample
    }

    fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
        None
    }
}

fn append_value(position: u16, value: Json, links: &[(u16, &str)]) -> Json {
    node(
        position,
        "STU_CBE8BDAA",
        None,
        json!({
            "m_26E3B366": var(TRACE, true),
            "m_value": value,
            "m_outPlug": plug(links, &[]),
        }),
    )
}

fn start(world: &mut SampleWorld, frame: u32) -> Component {
    let fixture = graph(
        GRAPH,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug": plug(&[(1, "m_beginPlug")], &[])}),
            ),
            node(
                1,
                "STU_96F1DBCF",
                Some(0),
                json!({
                    "m_105DF1D6": var(AIRBORNE, true),
                    "m_E37BC6C2": plug(&[(2, "m_inPlug")], &[]),
                    "m_2102C541": plug(&[(3, "m_inPlug")], &[]),
                }),
            ),
            append_value(2, json!(RISE), &[(4, "m_inPlug")]),
            append_value(3, json!(FALL), &[(4, "m_inPlug")]),
            append_value(4, var(AIRBORNE, true), &[]),
        ],
        json!([1]),
        json!([0]),
    );
    Component::start(
        Arc::new(store(vec![fixture])),
        ENTITY,
        Variables::from([(TRACE, Value::Array(Vec::new()))]),
        vec![Root::new(1, GRAPH)],
        frame,
        world,
        MAX_INIT_WORK,
    )
    .unwrap()
}

#[test]
fn movement_edges_publish_flags_in_order_once_per_frame() {
    let contract: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-movement-edges-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.evidence, "synthetic-rust");

    let steps = &contract.input.steps;
    let mut world = SampleWorld {
        sample: Some(MovementSample::from(&steps[0].sample)),
        ..Default::default()
    };
    let mut component = start(&mut world, steps[0].frame);
    let mut expected = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        if i != 0 {
            world.sample = Some(MovementSample::from(&step.sample));
            component.advance_frame(step.frame, &mut world).unwrap();
        }
        expected.extend(
            step.expected_edges
                .iter()
                .flat_map(|&(edge, airborne)| [Value::Integer(edge), Value::Bool(airborne)]),
        );
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(
            snapshot.variables[&TRACE],
            Value::Array(expected.clone()),
            "frame {}",
            step.frame
        );
        assert_eq!(world.reads.get(), i + 1, "frame {}", step.frame);
    }

    // The host changes the sample before end in the same VM frame.
    world.sample = Some(MovementSample::from(&contract.input.end_sample));
    component.debug_end_state(1, 0, &mut world).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert!(!snapshot.instances[&1].states[&0].active);
    assert_eq!(snapshot.variables[&TRACE], Value::Array(expected));
    assert_eq!(snapshot.variables[&AIRBORNE], Value::Bool(true));
    assert_eq!(world.reads.get(), steps.len());
}

#[test]
fn movement_edges_reject_an_unknown_sample_without_publishing() {
    let mut world = SampleWorld::default();
    let component = start(&mut world, 0);
    let snapshot = component.snapshot().unwrap();
    assert!(matches!(
        snapshot.diagnostics.as_slice(),
        [Diagnostic {
            issue: Issue::UnsupportedField("owner movement sample"),
            ..
        }]
    ));
    assert_eq!(snapshot.variables[&TRACE], Value::Array(Vec::new()));
    assert!(!snapshot.variables.contains_key(&AIRBORNE));
}
