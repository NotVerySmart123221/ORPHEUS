use serde::Deserialize;

use crate::script_runtime::frame_start;

use super::*;

const WAIT: &str = "STUStatescriptStateWait";
const INSTANCE: u16 = 1;
const TIMEOUT: u32 = 613;
const FINISHED: u32 = 614;
const ABORTED: u32 = 615;
const FINISHED_ON_ENTRY: u32 = 616;
const BEGIN: u64 = 14;
const ABORT: u64 = 18;

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    evidence: String,
    input: Input,
}

#[derive(Deserialize)]
struct Input {
    cases: Vec<Case>,
    invalid_shapes: Vec<serde_json::Map<String, Json>>,
}

#[derive(Deserialize)]
struct Case {
    graph: u32,
    rearm: bool,
    initial_seconds: f64,
    steps: Vec<Step>,
}

#[derive(Deserialize)]
struct Step {
    frame: u32,
    seconds: Option<f64>,
    event: Option<Trigger>,
    active: bool,
    #[serde(default)]
    finished: bool,
    #[serde(default)]
    aborted: bool,
    started_at_frame: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Trigger {
    Begin,
    Abort,
}

fn message_node(position: u16, message: u64, input: &str) -> Json {
    node(
        position,
        "STUStatescriptEntryGameMessage",
        None,
        json!({
            "m_gameMessage": {"m_gameMessage": format!("0x{message:016X}")},
            "m_outPlug": plug(&[(1, input)], &[]),
        }),
    )
}

fn mark(position: u16, variable: u32) -> Json {
    node(
        position,
        "STUStatescriptActionSetVar",
        None,
        json!({"m_out_Var": var(variable, false), "m_value": true}),
    )
}

fn start(graph_index: u32, rearm: bool, overrides: serde_json::Map<String, Json>) -> Component {
    let mut fields = json!({
        "m_BF1A93B0": 1,
        "m_0DBAFD7F": u8::from(rearm),
        "m_timeout": var(TIMEOUT, false),
        "m_onFinishedPlug": plug(&[(2, "m_inPlug")], &[]),
        "m_onAbortPlug": plug(&[(3, "m_inPlug")], &[]),
    });
    fields.as_object_mut().unwrap().extend(overrides);
    let mut begin = message_node(0, BEGIN, "m_beginPlug");
    begin["fields"]["m_outPlug"] = plug(&[(5, "m_inPlug")], &[]);
    let fixture = graph(
        graph_index,
        vec![
            begin,
            node(1, WAIT, Some(0), fields),
            mark(2, FINISHED),
            mark(3, ABORTED),
            message_node(4, ABORT, ABORT_PLUG),
            node(
                5,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var": var(FINISHED_ON_ENTRY, false), "m_value": var(FINISHED, false),
                    "m_outPlug": plug(&[(1, "m_beginPlug")], &[])}),
            ),
        ],
        json!([1]),
        json!([0, 4]),
    );
    Component::start(
        Arc::new(store(vec![fixture])),
        631,
        Variables::new(),
        vec![Root::listener(INSTANCE, graph_index, None)],
        0,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap()
}

fn send(component: &mut Component, frame: u32, message: u64) {
    assert_eq!(
        component
            .queue_frame_message(frame, INSTANCE, message, Variables::new())
            .unwrap(),
        1
    );
}

#[test]
fn restartable_wait_deadline_orders_message_actions_and_pending_begin() {
    for (next, expired, finished_on_entry) in [(4, false, false), (5, true, false), (6, true, true)]
    {
        let mut component = start(
            60577,
            true,
            serde_json::Map::from_iter([("m_BF1A93B0".into(), json!(0))]),
        );
        for (variable, value) in [
            (TIMEOUT, Value::Float(0.064)),
            (FINISHED, Value::Bool(false)),
        ] {
            component
                .set_instance_variable(INSTANCE, variable, value, &mut EmptyWorld)
                .unwrap();
        }
        send(&mut component, 1, BEGIN);
        component.advance_frame(1, &mut EmptyWorld).unwrap();
        for frame in 2..next {
            component.advance_frame(frame, &mut EmptyWorld).unwrap();
        }
        send(&mut component, next, BEGIN);
        component.advance_frame(next, &mut EmptyWorld).unwrap();
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.diagnostics.is_empty(),
            "{:?}",
            snapshot.diagnostics
        );
        let instance = &snapshot.instances[&INSTANCE];
        assert_eq!(
            instance.variables[&FINISHED],
            Value::Bool(expired),
            "frame {next}"
        );
        assert_eq!(
            instance.variables[&FINISHED_ON_ENTRY],
            Value::Bool(finished_on_entry),
            "frame {next}",
        );
        // Finishing the old wait removes a begin already queued for that state.
        assert_eq!(instance.states[&0].active, next != 5, "frame {next}");
        if instance.states[&0].active {
            assert_eq!(instance.states[&0].start_ms, frame_start(next));
        }
    }
}

#[test]
fn dynamic_elapsed_waits_resample_duration_and_respect_reentry_and_abort() {
    let contract: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-elapsed-wait-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.evidence, "synthetic-rust");

    for case in contract.input.cases {
        let mut component = start(case.graph, case.rearm, Default::default());
        component
            .set_instance_variable(
                INSTANCE,
                TIMEOUT,
                Value::Float(case.initial_seconds),
                &mut EmptyWorld,
            )
            .unwrap();
        for step in case.steps {
            let site = (case.graph, step.frame);
            for marker in [FINISHED, ABORTED] {
                component
                    .set_instance_variable(INSTANCE, marker, Value::Bool(false), &mut EmptyWorld)
                    .unwrap();
            }
            if let Some(seconds) = step.seconds {
                component
                    .set_instance_variable(
                        INSTANCE,
                        TIMEOUT,
                        Value::Float(seconds),
                        &mut EmptyWorld,
                    )
                    .unwrap();
            }
            if let Some(trigger) = step.event {
                let message = match trigger {
                    Trigger::Begin => BEGIN,
                    Trigger::Abort => ABORT,
                };
                send(&mut component, step.frame, message);
            }
            component
                .advance_frame(step.frame, &mut EmptyWorld)
                .unwrap();
            let snapshot = component.snapshot().unwrap();
            assert!(
                snapshot.diagnostics.is_empty(),
                "{site:?}: {:?}",
                snapshot.diagnostics
            );
            let instance = &snapshot.instances[&INSTANCE];
            assert_eq!(instance.states[&0].kind, StateKind::Wait, "{site:?}");
            assert_eq!(instance.states[&0].active, step.active, "{site:?}");
            assert_eq!(
                instance.states[&0].start_ms,
                frame_start(step.started_at_frame),
                "{site:?}"
            );
            assert_eq!(
                instance.variables[&FINISHED],
                Value::Bool(step.finished),
                "{site:?}"
            );
            assert_eq!(
                instance.variables[&ABORTED],
                Value::Bool(step.aborted),
                "{site:?}"
            );
            assert!(snapshot.pending_timers.is_empty(), "{site:?}");
            assert_eq!(
                component.machine.ticking.contains_key(&INSTANCE),
                step.active,
                "{site:?}"
            );
        }
    }

    for overrides in contract.input.invalid_shapes {
        let mut component = start(60574, true, overrides);
        send(&mut component, 1, BEGIN);
        component.advance_frame(1, &mut EmptyWorld).unwrap();
        let snapshot = component.snapshot().unwrap();
        assert!(matches!(
            snapshot.diagnostics.as_slice(),
            [Diagnostic { issue: Issue::UnsupportedState(class), .. }] if class == WAIT
        ));
        assert!(snapshot.instances[&INSTANCE].states.is_empty());
        assert!(component.machine.ticking.is_empty());
    }

    let mut component = start(60575, true, Default::default());
    component
        .set_instance_variable(INSTANCE, TIMEOUT, Value::Bool(true), &mut EmptyWorld)
        .unwrap();
    send(&mut component, 1, BEGIN);
    component.advance_frame(1, &mut EmptyWorld).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert!(matches!(
        snapshot.diagnostics.as_slice(),
        [Diagnostic {
            issue: Issue::UnsupportedField("Wait timeout"),
            ..
        }]
    ));
    assert!(
        !snapshot.instances[&INSTANCE]
            .variables
            .contains_key(&FINISHED)
    );
    assert!(snapshot.pending_timers.is_empty());
}
