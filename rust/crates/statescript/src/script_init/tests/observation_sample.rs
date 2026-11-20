use std::cell::Cell;

use serde::Deserialize;

use super::*;
use crate::script_expr::SpatialRayHit;
use crate::script_init::machine::Event;

const OUTPUT: u32 = 42;
const CHANGE_COPY: u32 = 43;
const SAMPLE_COPY: u32 = 44;
const ABORT_MESSAGE: u64 = 83;
const KEY: StateKey = StateKey {
    instance: 1,
    raw: 0,
};

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    case_id: String,
    evidence: String,
    input: Input,
}

#[derive(Deserialize)]
struct Input {
    body: u32,
    graph: u32,
    hit_distance: f64,
    steps: Vec<Step>,
}

#[derive(Deserialize)]
struct Step {
    frame: u32,
    #[serde(default)]
    override_output: bool,
    #[serde(default)]
    unavailable: bool,
    #[serde(default)]
    invalid_direction: bool,
    #[serde(default)]
    miss: bool,
    #[serde(default)]
    abort: bool,
    queries: usize,
    callbacks: Vec<f64>,
    output: String,
}

struct Host {
    body: u32,
    reply: Option<Option<SpatialRayHit>>,
    queries: Cell<usize>,
    callbacks: Vec<f64>,
}

impl World for Host {
    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }

    fn spatial_ray_cast(
        &self,
        origin: [f64; 3],
        direction: [f64; 3],
        max_distance: f64,
    ) -> Option<Option<SpatialRayHit>> {
        assert_eq!(
            (origin, direction, max_distance),
            ([0.0; 3], [0.0, 0.0, 1.0], 4.0)
        );
        self.queries.set(self.queries.get() + 1);
        self.reply
    }

    fn queue_health_change(&mut self, _: Site, target: u32, delta: f64) -> bool {
        assert_eq!(target, self.body);
        self.callbacks.push(delta);
        true
    }
}

fn fixture() -> Contract {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/statescript-observation-sample-v1.json"
    )))
    .unwrap()
}

fn bundle(graph_id: u32, on_sample: bool) -> Json {
    let entry = node(
        0,
        "STUStatescriptEntry",
        None,
        json!({"m_outPlug":plug(&[(1,"m_beginPlug")], &[])}),
    );
    let mut observer = node(
        1,
        "AuthoredSample",
        Some(0),
        json!({
            "value":{
                "$":"RayCast",
                "origin":var(31,true),
                "direction":var(32,true),
                "max_distance":4
            },
            "output":var(OUTPUT,true),
            "on_change":plug(&[(2,"m_inPlug")], &[])
        }),
    );
    observer["state_behavior"] = json!("observe");
    observer["server_only"] = json!(true);
    if on_sample {
        observer["fields"]["on_sample"] = plug(&[(4, "m_inPlug")], &[]);
    }
    let copy = |position, output, next| {
        node(
            position,
            "STUStatescriptActionSetVar",
            None,
            json!({
                "m_out_Var":var(output,true),
                "m_value":var(OUTPUT,true),
                "m_outPlug":plug(&[(next,"m_inPlug")], &[])
            }),
        )
    };
    let pulse = |position, delta| {
        let mut action = node(
            position,
            "AuthoredCallbackAction",
            None,
            json!({"target":var(1,true),"delta":delta,"m_outPlug":plug(&[], &[])}),
        );
        action["action_behavior"] = json!("health_change");
        action["server_only"] = json!(true);
        action
    };
    let abort = node(
        6,
        "STUStatescriptEntryGameMessage",
        None,
        json!({
            "m_gameMessage":{"m_gameMessage":format!("0x{ABORT_MESSAGE:016X}")},
            "m_outPlug":plug(&[(1,ABORT_PLUG)], &[])
        }),
    );
    graph(
        graph_id,
        vec![
            entry,
            observer,
            copy(2, CHANGE_COPY, 3),
            pulse(3, 1),
            copy(4, SAMPLE_COPY, 5),
            pulse(5, 2),
            abort,
        ],
        json!([1]),
        json!([0, 6]),
    )
}

fn start(graph: Json, body: u32, hit_distance: f64) -> (Component, Host, Value) {
    let graph_id = graph["index"].as_u64().unwrap() as u32;
    let hit = SpatialRayHit {
        distance: hit_distance,
        point: [0.0, 0.0, hit_distance],
        normal: [0.0, 0.0, -1.0],
    };
    let hit_value = Value::Array(vec![
        Value::Float(hit.distance),
        Value::Vector(hit.point),
        Value::Vector(hit.normal),
    ]);
    let mut host = Host {
        body,
        reply: Some(Some(hit)),
        queries: Cell::new(0),
        callbacks: Vec::new(),
    };
    let component = Component::start(
        Arc::new(store(vec![graph])),
        body,
        Variables::from([
            (1, Value::Entity(body)),
            (31, Value::Vector([0.0; 3])),
            (32, Value::Vector([0.0, 0.0, 1.0])),
        ]),
        vec![Root::new(1, graph_id)],
        0,
        &mut host,
        MAX_INIT_WORK,
    )
    .unwrap();
    (component, host, hit_value)
}

#[test]
fn observation_sample_runs_only_for_fresh_successful_samples() {
    let contract = fixture();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(
        contract.case_id,
        "statescript-observation-sample.synthetic.v1"
    );
    assert_eq!(contract.evidence, "synthetic-rust");
    let input = contract.input;
    let (mut component, mut host, hit_value) =
        start(bundle(input.graph, true), input.body, input.hit_distance);
    assert_eq!(host.queries.get(), 0);
    assert!(host.callbacks.is_empty());
    let hit = host.reply;
    for step in input.steps {
        host.reply = if step.unavailable {
            None
        } else if step.miss {
            Some(None)
        } else {
            hit
        };
        host.callbacks.clear();
        component
            .set_entity_variable(
                32,
                Value::Vector(if step.invalid_direction {
                    [0.0; 3]
                } else {
                    [0.0, 0.0, 1.0]
                }),
                &mut host,
            )
            .unwrap();
        if step.override_output {
            component
                .set_entity_variable(OUTPUT, Value::Integer(999), &mut host)
                .unwrap();
        }
        if step.abort {
            component
                .queue_frame_message(step.frame, 1, ABORT_MESSAGE, Variables::new())
                .unwrap();
        }
        let before = host.queries.get();
        component.advance_frame(step.frame, &mut host).unwrap();
        let snapshot = component.snapshot().unwrap();
        let expected = if step.output == "empty" {
            Value::Array(Vec::new())
        } else {
            hit_value.clone()
        };
        assert_eq!(
            host.queries.get() - before,
            step.queries,
            "frame {}",
            step.frame
        );
        assert_eq!(host.callbacks, step.callbacks, "frame {}", step.frame);
        assert_eq!(
            snapshot.variables[&OUTPUT], expected,
            "frame {}",
            step.frame
        );
        if host.callbacks.contains(&1.0) {
            assert_eq!(snapshot.variables[&CHANGE_COPY], expected);
        }
        if host.callbacks.contains(&2.0) {
            assert_eq!(snapshot.variables[&SAMPLE_COPY], expected);
        }
    }
}

#[test]
fn observation_without_sample_hook_preserves_unchanged_fast_path() {
    let input = fixture().input;
    let (mut component, mut host, _) =
        start(bundle(input.graph, false), input.body, input.hit_distance);
    component.advance_frame(1, &mut host).unwrap();
    assert_eq!(host.callbacks, [1.0]);
    component
        .set_entity_variable(OUTPUT, Value::Integer(999), &mut host)
        .unwrap();
    component.advance_frame(2, &mut host).unwrap();
    assert_eq!(
        component.snapshot().unwrap().variables[&OUTPUT],
        Value::Integer(999)
    );
    assert_eq!(host.callbacks, [1.0]);
}

#[test]
fn observation_sample_does_not_cross_retirement_or_reactivation() {
    let input = fixture().input;
    let mut graph = bundle(input.graph, true);
    graph["nodes"][1]["fields"]["on_change"] = plug(&[(1, ABORT_PLUG)], &[]);
    graph["nodes"][1]["fields"]["m_onAbortPlug"] = plug(&[(1, "m_beginPlug")], &[]);
    let (mut component, mut host, _) = start(graph, input.body, input.hit_distance);
    let initial = component.snapshot().unwrap().instances[&1].states[&0].activation;
    component.advance_frame(1, &mut host).unwrap();
    let current = component.snapshot().unwrap().instances[&1].states[&0].activation;
    assert_ne!(initial, current);
    assert!(component.snapshot().unwrap().instances[&1].states[&0].active);
    assert!(host.callbacks.is_empty());
    component
        .run(&mut host, |vm| {
            vm.post(
                1,
                Event::ObserveSample {
                    raw: 0,
                    activation: initial,
                    frame: 1,
                },
            )
        })
        .unwrap();
    component.run_now(&mut host).unwrap();
    assert!(host.callbacks.is_empty());

    // A callback from an earlier frame cannot run in a later VM update.
    component
        .run(&mut host, |vm| {
            vm.post(
                1,
                Event::ObserveSample {
                    raw: 0,
                    activation: current,
                    frame: 0,
                },
            )
        })
        .unwrap();
    component.run_now(&mut host).unwrap();
    assert!(host.callbacks.is_empty());

    component
        .run(&mut host, |vm| {
            vm.end(KEY, true, 0)?;
            vm.post(
                1,
                Event::ObserveSample {
                    raw: 0,
                    activation: current,
                    frame: 1,
                },
            )
        })
        .unwrap();
    component.run_now(&mut host).unwrap();
    assert!(!component.snapshot().unwrap().instances[&1].states[&0].active);
    assert!(host.callbacks.is_empty());
}
