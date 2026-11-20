use std::cell::Cell;

use serde::Deserialize;

use super::*;
use crate::script_expr::SpatialRayHit;
use crate::script_init::machine::Event;

mod held_input;

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
    unavailable_callback_delta: f64,
    ownership: Ownership,
    deferred_facing: DeferredFacing,
    held_branch: held_input::HeldBranch,
    steps: Vec<Step>,
}

#[derive(Deserialize)]
struct DeferredFacing {
    directions: [[f64; 3]; 2],
    ordinary_direction: [f64; 3],
    distance: f64,
    max_distance: f64,
    frame: u32,
    ordinary_message: u64,
}

#[derive(Deserialize)]
struct Ownership {
    samples: [i64; 2],
    overwrite: i64,
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
        _site: Site,
        _vm_frame: u32,
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
                    available: true,
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
                    available: true,
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
                    available: true,
                },
            )
        })
        .unwrap();
    component.run_now(&mut host).unwrap();
    assert!(!component.snapshot().unwrap().instances[&1].states[&0].active);
    assert!(host.callbacks.is_empty());
}

fn shared_output_bundle(graph_id: u32, ownership: &Ownership, predecessor: bool) -> Json {
    let entry = node(
        0,
        "STUStatescriptEntry",
        None,
        json!({"m_outPlug":plug(&[(1,"m_beginPlug"),(2,"m_beginPlug")], &[])}),
    );
    let observe = |position, raw, value, callback| {
        let mut observer = node(
            position,
            "AuthoredSharedSample",
            Some(raw),
            json!({
                "value":value,
                "output":var(OUTPUT,true),
                "on_sample":plug(&[(callback,"m_inPlug")], &[])
            }),
        );
        observer["state_behavior"] = json!("observe");
        observer["server_only"] = json!(true);
        observer
    };
    let copy = |position, destination, links: &[(u16, &str)]| {
        node(
            position,
            "STUStatescriptActionSetVar",
            None,
            json!({
                "m_out_Var":var(destination,true),
                "m_value":var(OUTPUT,true),
                "m_outPlug":plug(links, &[])
            }),
        )
    };
    let predecessor_links: &[(u16, &str)] = if predecessor { &[(5, "m_inPlug")] } else { &[] };
    let overwrite = node(
        5,
        "STUStatescriptActionSetVar",
        None,
        json!({
            "m_out_Var":var(OUTPUT,true),
            "m_value":ownership.overwrite,
            "m_outPlug":plug(&[(6,"m_inPlug")], &[])
        }),
    );
    graph(
        graph_id,
        vec![
            entry,
            observe(1, 0, ownership.samples[0], 3),
            observe(2, 1, ownership.samples[1], 4),
            copy(3, 44, predecessor_links),
            copy(4, 45, &[]),
            overwrite,
            copy(6, 46, &[]),
        ],
        json!([1, 2]),
        json!([0]),
    )
}

#[test]
fn observation_samples_restore_each_shared_output_before_its_callback() {
    let input = fixture().input;
    let graph = shared_output_bundle(input.graph, &input.ownership, false);
    let (mut component, mut host, _) = start(graph, input.body, input.hit_distance);
    component.advance_frame(1, &mut host).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert_eq!(
        snapshot.variables[&44],
        Value::Integer(input.ownership.samples[0])
    );
    assert_eq!(
        snapshot.variables[&45],
        Value::Integer(input.ownership.samples[1])
    );
    assert_eq!(
        snapshot.variables[&OUTPUT],
        Value::Integer(input.ownership.samples[1])
    );
    assert!(snapshot.diagnostics.is_empty());
    assert_eq!(host.queries.get(), 0);
}

#[test]
fn observation_sample_restores_output_after_predecessor_callback() {
    let input = fixture().input;
    let graph = shared_output_bundle(input.graph, &input.ownership, true);
    let (mut component, mut host, _) = start(graph, input.body, input.hit_distance);
    component.advance_frame(1, &mut host).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert_eq!(
        snapshot.variables[&44],
        Value::Integer(input.ownership.samples[0])
    );
    assert_eq!(
        snapshot.variables[&46],
        Value::Integer(input.ownership.overwrite)
    );
    assert_eq!(
        snapshot.variables[&45],
        Value::Integer(input.ownership.samples[1])
    );
    assert_eq!(
        snapshot.variables[&OUTPUT],
        Value::Integer(input.ownership.samples[1])
    );
    assert!(snapshot.diagnostics.is_empty());
}

#[test]
fn observation_sample_restores_output_after_change_callback() {
    let input = fixture().input;
    let mut graph = bundle(input.graph, true);
    graph["nodes"][1]["fields"]["value"] = json!(input.ownership.samples[0]);
    graph["nodes"][2]["fields"] = json!({
        "m_ADEB6E05":true,
        "m_out_Var":var(OUTPUT,true),
        "m_value":input.ownership.overwrite,
        "m_outPlug":plug(&[(3,"m_inPlug")], &[])
    });
    graph["nodes"][3] = node(
        3,
        "STUStatescriptActionSetVar",
        None,
        json!({
            "m_out_Var":var(CHANGE_COPY,true),
            "m_value":var(OUTPUT,true),
            "m_outPlug":plug(&[], &[])
        }),
    );
    let (mut component, mut host, _) = start(graph, input.body, input.hit_distance);
    component.advance_frame(1, &mut host).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert_eq!(
        snapshot.variables[&CHANGE_COPY],
        Value::Integer(input.ownership.overwrite)
    );
    assert_eq!(
        snapshot.variables[&SAMPLE_COPY],
        Value::Integer(input.ownership.samples[0])
    );
    assert_eq!(
        snapshot.variables[&OUTPUT],
        Value::Integer(input.ownership.samples[0])
    );
    assert_eq!(host.callbacks, [2.0]);
    assert!(snapshot.diagnostics.is_empty());
}

#[test]
fn observation_sample_waits_for_output_binding_to_release() {
    let input = fixture().input;
    let mut graph = bundle(input.graph, true);
    graph["nodes"][0]["fields"]["m_outPlug"] = plug(&[(7, "m_beginPlug"), (1, "m_beginPlug")], &[]);
    graph["nodes"][1]["fields"]["value"] = json!(input.ownership.samples[0]);
    graph["nodes"][1]["fields"]["on_change"] = serde_json::Value::Null;
    let mut claim = node(
        7,
        "AuthoredOutputClaim",
        Some(1),
        json!({
            "variable":{"scope":"entity","id":OUTPUT},
            "priority":{"level":10,"above_equals":true},
            "value":input.ownership.overwrite
        }),
    );
    claim["state_behavior"] = json!("stack_claim");
    claim["server_only"] = json!(true);
    graph["nodes"].as_array_mut().unwrap().push(claim);
    graph["states"] = json!([1, 7]);
    graph["items"] = json!(8);
    let (mut component, mut host, _) = start(graph, input.body, input.hit_distance);
    component.advance_frame(1, &mut host).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert_eq!(
        snapshot.variables[&OUTPUT],
        Value::Integer(input.ownership.samples[0])
    );
    assert_eq!(
        snapshot.resolved_bindings[&(0, OUTPUT)],
        Value::Integer(input.ownership.overwrite)
    );
    assert!(!snapshot.variables.contains_key(&SAMPLE_COPY));
    assert!(host.callbacks.is_empty());
    assert!(matches!(
        snapshot.diagnostics.as_slice(),
        [Diagnostic {
            issue: Issue::ObservationUnavailable,
            ..
        }]
    ));
    let blocked_diagnostics = snapshot.diagnostics.clone();

    component
        .run(&mut host, |vm| {
            vm.end(
                StateKey {
                    instance: 1,
                    raw: 1,
                },
                false,
                0,
            )
        })
        .unwrap();
    component.advance_frame(2, &mut host).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert_eq!(
        snapshot.variables[&OUTPUT],
        Value::Integer(input.ownership.samples[0])
    );
    assert_eq!(
        snapshot.variables[&SAMPLE_COPY],
        Value::Integer(input.ownership.samples[0])
    );
    assert_eq!(host.callbacks, [2.0]);
    assert_eq!(snapshot.diagnostics, blocked_diagnostics);
    assert!(snapshot.initialization_complete());
}

fn unavailable_bundle(input: &Input) -> Json {
    let mut graph = bundle(input.graph, true);
    graph["nodes"][1]["fields"]["on_unavailable"] = plug(&[(7, "m_inPlug")], &[]);
    let mut callback = node(
        7,
        "AuthoredUnavailableCallback",
        None,
        json!({
            "target":var(1,true),
            "delta":input.unavailable_callback_delta,
            "m_outPlug":plug(&[], &[])
        }),
    );
    callback["action_behavior"] = json!("health_change");
    callback["server_only"] = json!(true);
    graph["nodes"].as_array_mut().unwrap().push(callback);
    graph["items"] = json!(8);
    graph
}

#[test]
fn observation_unavailable_hook_runs_without_a_previous_value_and_recovers() {
    let input = fixture().input;
    let (mut component, mut host, hit_value) =
        start(unavailable_bundle(&input), input.body, input.hit_distance);
    let hit = host.reply;
    let sentinel = Value::Integer(input.ownership.overwrite);
    component
        .set_entity_variable(OUTPUT, sentinel.clone(), &mut host)
        .unwrap();
    host.reply = None;
    for frame in 1..=2 {
        host.callbacks.clear();
        let queries = host.queries.get();
        component.advance_frame(frame, &mut host).unwrap();
        let snapshot = component.snapshot().unwrap();
        assert_eq!(host.queries.get() - queries, 1);
        assert_eq!(host.callbacks, [input.unavailable_callback_delta]);
        assert_eq!(snapshot.variables[&OUTPUT], sentinel);
        assert_eq!(snapshot.instances[&1].states[&0].watch_seen, None);
    }
    let diagnostics = component.snapshot().unwrap().diagnostics.clone();
    assert!(matches!(
        diagnostics.as_slice(),
        [Diagnostic {
            issue: Issue::ObservationUnavailable,
            ..
        }]
    ));
    host.reply = hit;
    host.callbacks.clear();
    component.advance_frame(3, &mut host).unwrap();
    assert_eq!(host.callbacks, [1.0, 2.0]);
    assert_eq!(component.snapshot().unwrap().variables[&OUTPUT], hit_value);
    let snapshot = component.snapshot().unwrap();
    assert_eq!(snapshot.diagnostics, diagnostics);
    assert!(snapshot.initialization_complete());

    host.reply = None;
    host.callbacks.clear();
    component.advance_frame(4, &mut host).unwrap();
    assert_eq!(host.callbacks, [input.unavailable_callback_delta]);
    assert_eq!(component.snapshot().unwrap().variables[&OUTPUT], hit_value);

    host.reply = Some(None);
    host.callbacks.clear();
    component.advance_frame(5, &mut host).unwrap();
    assert_eq!(host.callbacks, [1.0, 2.0]);
    assert_eq!(
        component.snapshot().unwrap().variables[&OUTPUT],
        Value::Array(Vec::new())
    );
    let snapshot = component.snapshot().unwrap();
    assert_eq!(snapshot.diagnostics, diagnostics);
    assert!(snapshot.initialization_complete());
}

#[test]
fn observation_unavailable_hook_excludes_invalid_expressions() {
    let input = fixture().input;
    for unsupported in [false, true] {
        let mut graph = unavailable_bundle(&input);
        if unsupported {
            graph["nodes"][1]["fields"]["value"] = json!({"$":"authored.unknown_operand"});
        }
        let (mut component, mut host, _) = start(graph, input.body, input.hit_distance);
        if !unsupported {
            component
                .set_entity_variable(32, Value::Vector([0.0; 3]), &mut host)
                .unwrap();
        }
        let sentinel = Value::Integer(input.ownership.overwrite);
        component
            .set_entity_variable(OUTPUT, sentinel.clone(), &mut host)
            .unwrap();
        component.advance_frame(1, &mut host).unwrap();
        let snapshot = component.snapshot().unwrap();
        assert!(host.callbacks.is_empty());
        assert_eq!(host.queries.get(), 0);
        assert_eq!(snapshot.variables[&OUTPUT], sentinel);
        assert!(matches!(
            snapshot.diagnostics.as_slice(),
            [Diagnostic {
                issue: Issue::Expression(_),
                ..
            }]
        ));
    }
}

#[test]
fn observation_unavailable_hook_respects_frame_activation_and_retirement() {
    let input = fixture().input;
    let (mut component, mut host, _) =
        start(unavailable_bundle(&input), input.body, input.hit_distance);
    host.reply = None;
    component.advance_frame(1, &mut host).unwrap();
    assert_eq!(host.callbacks, [input.unavailable_callback_delta]);
    let initial = component.snapshot().unwrap().instances[&1].states[&0].activation;
    host.callbacks.clear();
    component
        .run(&mut host, |vm| {
            vm.post(
                1,
                Event::ObserveSample {
                    raw: 0,
                    activation: initial,
                    frame: 1,
                    available: false,
                },
            )?;
            vm.end(KEY, false, 0)?;
            vm.begin(KEY, 0)?;
            vm.post(
                1,
                Event::ObserveSample {
                    raw: 0,
                    activation: initial,
                    frame: 1,
                    available: false,
                },
            )
        })
        .unwrap();
    component.run_now(&mut host).unwrap();
    let current = component.snapshot().unwrap().instances[&1].states[&0].activation;
    assert_ne!(initial, current);
    assert!(host.callbacks.is_empty());
    component
        .run(&mut host, |vm| {
            vm.post(
                1,
                Event::ObserveSample {
                    raw: 0,
                    activation: current,
                    frame: 0,
                    available: false,
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
                    available: false,
                },
            )
        })
        .unwrap();
    component.run_now(&mut host).unwrap();
    assert!(!component.snapshot().unwrap().instances[&1].states[&0].active);
    assert!(host.callbacks.is_empty());
    assert_eq!(host.queries.get(), 1);
}

struct DeferredFacingHost {
    directions: [[f64; 3]; 2],
    distance: f64,
    max_distance: f64,
    queries: Cell<usize>,
    active: Vec<(Site, AiFacing)>,
}

impl World for DeferredFacingHost {
    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }

    fn spatial_ray_cast(
        &self,
        _: Site,
        _: u32,
        origin: [f64; 3],
        direction: [f64; 3],
        max_distance: f64,
    ) -> Option<Option<SpatialRayHit>> {
        assert_eq!(origin, [0.0; 3]);
        assert_eq!(max_distance, self.max_distance);
        assert!(self.directions.contains(&direction));
        self.queries.set(self.queries.get() + 1);
        Some(Some(SpatialRayHit {
            distance: self.distance,
            point: direction.map(|axis| axis * self.distance),
            normal: direction,
        }))
    }

    fn set_facing_claim(&mut self, site: Site, record: AiFacing, active: bool) -> Option<bool> {
        assert_eq!(record.target, site.entity);
        self.active.retain(|(owner, _)| *owner != site);
        if active {
            self.active.push((site, record));
        }
        Some(true)
    }
}

fn deferred_ray_value(direction: [f64; 3], distance: f64) -> Value {
    Value::Array(vec![
        Value::Float(distance),
        Value::Vector(direction.map(|axis| axis * distance)),
        Value::Vector(direction),
    ])
}

fn deferred_facing_graph(input: &Input, hook: &str, indirect: bool) -> Json {
    let fields = &input.deferred_facing;
    let observe = |position, raw, direction, next| {
        let mut observer = node(
            position,
            "AuthoredDeferredRaySample",
            Some(raw),
            json!({"value":{"$":"RayCast","origin":var(31,true),
                "direction":var(direction,true),"max_distance":fields.max_distance},
                "output":var(OUTPUT,true)}),
        );
        observer["fields"][hook] = plug(&[next], &[]);
        observer["state_behavior"] = json!("observe");
        observer["server_only"] = json!(true);
        observer
    };
    let facing = |position, raw, level| {
        let mut claim = node(
            position,
            "AuthoredDeferredFacing",
            Some(raw),
            json!({"direction":{"$":"STU_4BD39CFD","m_index":2,
                "m_814F8EC3":var(OUTPUT,true)},
                "priority":{"level":level,"above_equals":false}}),
        );
        claim["state_behavior"] = json!("facing_claim");
        claim["server_only"] = json!(true);
        claim
    };
    let mut relay = node(
        8,
        "AuthoredSampleRelay",
        None,
        json!({"m_outPlug":plug(&[(3,"m_beginPlug")],&[])}),
    );
    relay["action_behavior"] = json!("forward");
    relay["server_only"] = json!(true);
    graph(
        input.graph,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_beginPlug"),(2,"m_beginPlug")],&[])}),
            ),
            observe(
                1,
                0,
                32,
                if indirect {
                    (8, "m_inPlug")
                } else {
                    (3, "m_beginPlug")
                },
            ),
            observe(2, 1, 33, (4, "m_beginPlug")),
            facing(3, 2, 3),
            facing(4, 3, 4),
            node(
                5,
                "STUStatescriptEntryGameMessage",
                None,
                json!({"m_gameMessage":{"m_gameMessage":format!("0x{:016X}",fields.ordinary_message)},
                    "m_outPlug":plug(&[(6,"m_inPlug"),(7,"m_beginPlug")],&[])}),
            ),
            node(
                6,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var":var(SAMPLE_COPY,true),"m_value":var(OUTPUT,true),
                    "m_outPlug":plug(&[],&[])}),
            ),
            facing(7, 4, 1),
            relay,
        ],
        json!([1, 2, 3, 4, 7]),
        json!([0, 5]),
    )
}

fn start_deferred_facing(
    input: &Input,
    hook: &str,
    indirect: bool,
) -> (Component, DeferredFacingHost) {
    start_deferred_model(
        input,
        deferred_facing_graph(input, hook, indirect),
        Variables::new(),
    )
}

fn start_deferred_model(
    input: &Input,
    model: Json,
    initial: Variables,
) -> (Component, DeferredFacingHost) {
    let fields = &input.deferred_facing;
    let mut host = DeferredFacingHost {
        directions: fields.directions,
        distance: fields.distance,
        max_distance: fields.max_distance,
        queries: Cell::new(0),
        active: Vec::new(),
    };
    let mut variables = Variables::from([
        (31, Value::Vector([0.0; 3])),
        (32, Value::Vector(fields.directions[0])),
        (33, Value::Vector(fields.directions[1])),
    ]);
    variables.extend(initial);
    let component = Component::start(
        Arc::new(store(vec![model])),
        input.body,
        variables,
        vec![Root::new(1, input.graph)],
        0,
        &mut host,
        MAX_INIT_WORK,
    )
    .unwrap();
    (component, host)
}

#[test]
fn observation_samples_preserve_their_ray_results_for_queued_facing_starts() {
    let input = fixture().input;
    let fields = &input.deferred_facing;
    for (hook, indirect) in [
        ("on_sample", false),
        ("on_sample", true),
        ("on_change", false),
    ] {
        let (mut component, mut host) = start_deferred_facing(&input, hook, indirect);
        assert!(host.active.is_empty());
        component.advance_frame(fields.frame, &mut host).unwrap();
        assert_eq!(host.queries.get(), 2);
        assert_eq!(host.active.len(), 2);
        for (position, direction) in [(3, fields.directions[0]), (4, fields.directions[1])] {
            let (_, claim) = host
                .active
                .iter()
                .find(|(site, _)| site.node == Some(position))
                .unwrap();
            assert_eq!(
                claim.direction, direction,
                "hook: {hook}, indirect relay: {indirect}, claim {position}"
            );
        }
        let last = deferred_ray_value(fields.directions[1], fields.distance);
        assert_eq!(component.snapshot().unwrap().variables[&OUTPUT], last);
        let ordinary = deferred_ray_value(fields.ordinary_direction, fields.distance);
        component
            .set_entity_variable(OUTPUT, ordinary.clone(), &mut host)
            .unwrap();
        component
            .queue_message(1, fields.ordinary_message, Variables::new())
            .unwrap();
        component.run_now(&mut host).unwrap();
        let (_, ordinary_claim) = host
            .active
            .iter()
            .find(|(site, _)| site.node == Some(7))
            .unwrap();
        assert_eq!(ordinary_claim.direction, fields.ordinary_direction);
        let snapshot = component.snapshot().unwrap();
        assert_eq!(snapshot.variables[&OUTPUT], ordinary);
        assert_eq!(snapshot.variables[&SAMPLE_COPY], ordinary);
        assert!(
            snapshot.diagnostics.is_empty(),
            "{:?}",
            snapshot.diagnostics
        );
        component.teardown(&mut host).unwrap();
        assert!(host.active.is_empty());
    }
}

#[test]
fn queued_sample_facing_starts_respect_source_and_destination_cancellation() {
    let input = fixture().input;
    let fields = &input.deferred_facing;
    for (hook, available, reactivate, cancel_destination) in [
        ("on_sample", true, false, false),
        ("on_sample", true, true, false),
        ("on_unavailable", false, true, false),
        ("on_sample", true, false, true),
    ] {
        let (mut component, mut host) = start_deferred_facing(&input, hook, true);
        component.advance_frame(fields.frame, &mut host).unwrap();
        let activation = component.snapshot().unwrap().instances[&1].states[&KEY.raw].activation;
        component
            .run(&mut host, |vm| {
                vm.end(
                    StateKey {
                        instance: 1,
                        raw: 2,
                    },
                    false,
                    0,
                )?;
                vm.deliver_observed_sample(KEY, activation, fields.frame, available)?;
                if cancel_destination {
                    vm.drop_begin(1, 2);
                    return Ok(());
                }
                vm.end(KEY, false, 0)?;
                if reactivate {
                    vm.begin(KEY, 0)?;
                }
                Ok(())
            })
            .unwrap();
        component.run_now(&mut host).unwrap();
        assert!(
            host.active.iter().all(|(site, _)| site.node != Some(3)),
            "cancelled start ran ({hook}, reactivated: {reactivate}, destination: {cancel_destination})"
        );
        assert!(
            !component.snapshot().unwrap().instances[&1]
                .states
                .get(&2)
                .is_some_and(|state| state.active)
        );
        assert_eq!(host.queries.get(), 2);
        component.teardown(&mut host).unwrap();
        assert!(host.active.is_empty());
    }
}
