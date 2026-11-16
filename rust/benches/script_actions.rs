//! Synthetic public-API benchmark; no prepared runtime or client evidence.
//!
//! `cargo bench --locked --bench script_actions`
//! Compare the same benchmark source, lockfile, toolchain, host and release
//! profile on both revisions. Graph construction is outside every timed region.
//! Loading includes JSON parsing, graph compilation and graph destruction.
//! Execution excludes graph loading, Component startup and destruction; it
//! includes public input writes, message queueing, VM execution and observation.
//! Each iteration starts fresh and executes the same 32-message sequence.
//! Entity, initial frame and work budget are fixed; there are no RNG opcodes.
//! These timings do not measure allocations, resident memory or client latency.

use std::{hint::black_box, sync::Arc, time::Duration};

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use orpheus::{
    script_graph::GraphStore,
    script_init::{Component, EmptyWorld, Root},
    script_wire::{Value, Variables},
};
use serde_json::{Value as Json, json};

const GRAPH: u32 = 42;
const INSTANCE: u16 = 1;
const ENTITY: u32 = 0xa000_0042;
const MESSAGE: u64 = 0x0240_0000_0000_0001;
const WORK_BUDGET: usize = 10_000;
const MESSAGES: usize = 32;

fn variable(id: u32) -> Json {
    json!({"$":"STUConfigVarDynamic", "m_identifier":id, "m_60DB8F99":true})
}

fn increment(id: u32) -> Json {
    json!({"$":"STU_919FD47C", "m_expression":{
        "m_opcodes":[8,0,6,0,23,0],
        "m_dynamicVars":[variable(id)], "m_D99EF254":[1]
    }})
}

fn plug(target: u16) -> Json {
    json!({"$":"Plug", "links":[[target,"m_inPlug"]]})
}

fn node(position: u16, class: &str, mut fields: Json) -> Json {
    fields["m_2BBEEAB8"] = json!(false);
    fields["m_ADEB6E05"] = json!(class.contains("Action") || class == "STU_12F5D52D");
    json!({
        "pos":position, "class":class, "uid":position, "rid":position,
        "remote":-1, "state":null, "client_only":false, "server_only":false,
        "fields":fields
    })
}

fn assignment(position: u16, id: u32, value: Json, next: Option<u16>) -> Json {
    let mut fields = json!({"m_out_Var":variable(id), "m_value":value});
    if let Some(next) = next {
        fields["m_outPlug"] = plug(next);
    }
    node(position, "STUStatescriptActionSetVar", fields)
}

fn fixture() -> Vec<u8> {
    // A message writes dynamic values, chooses both conditional paths and all
    // switch paths over the fixed input sequence, then increments a counter.
    let nodes = vec![
        node(
            0,
            "STUStatescriptEntryGameMessage",
            json!({
                "m_gameMessage":{"m_gameMessage":"0x0240000000000001"},
                "m_outPlug":plug(1)
            }),
        ),
        assignment(1, 200, increment(100), Some(2)),
        node(
            2,
            "STU_12F5D52D",
            json!({
                "m_143D4C5B":[
                    {"m_out_Var":variable(201), "m_value":variable(100)},
                    {"m_out_Var":variable(202), "m_value":variable(200)}
                ],
                "m_outPlug":plug(3)
            }),
        ),
        node(
            3,
            "STU_3387AB5D",
            json!({
                "m_condition":variable(101), "m_truePlug":plug(4), "m_falsePlug":plug(5)
            }),
        ),
        assignment(4, 203, json!(1), Some(6)),
        assignment(5, 203, json!(-1), Some(6)),
        node(
            6,
            "STUStatescriptActionSwitch",
            json!({
                "m_262AE982":variable(100),
                "m_FDD34C25":[
                    {"$":"Plug", "m_value":0, "links":[[7,"m_inPlug"]]},
                    {"$":"Plug", "m_value":1, "links":[[8,"m_inPlug"]]}
                ],
                "m_CD38D5DB":plug(9), "m_outPlug":plug(10)
            }),
        ),
        assignment(7, 204, json!(10), None),
        assignment(8, 204, json!(20), None),
        assignment(9, 204, json!(30), None),
        assignment(10, 205, increment(205), None),
    ];
    serde_json::to_vec(&json!({
        "version":1, "bodies":{}, "missing":[],
        "graphs":[{
            "index":GRAPH, "items":nodes.len(), "nodes":nodes,
            "states":[], "entries":[0], "remote":[], "sync_vars":[],
            "containers":{}, "unknown_nodes":[], "fields":{},
            "header":{
                "m_nodesBitCount":4, "m_statesBitCount":4,
                "m_remoteSyncNodesBitCount":4, "m_syncVarsBitCount":4
            }
        }]
    }))
    .expect("synthetic benchmark graph serializes")
}

fn start(graphs: &Arc<GraphStore>) -> Component {
    Component::start(
        graphs.clone(),
        ENTITY,
        Variables::from([(205, Value::Integer(0))]),
        vec![Root::new(INSTANCE, GRAPH)],
        0,
        &mut EmptyWorld,
        WORK_BUDGET,
    )
    .expect("synthetic benchmark component starts")
}

fn execute(component: &mut Component, inputs: &[(i64, bool); MESSAGES]) {
    for &(value, condition) in black_box(inputs) {
        component
            .set_entity_variable(100, Value::Integer(value), &mut EmptyWorld)
            .expect("benchmark input accepted");
        component
            .set_entity_variable(101, Value::Bool(condition), &mut EmptyWorld)
            .expect("benchmark branch input accepted");
        component
            .queue_message(INSTANCE, MESSAGE, Variables::new())
            .expect("benchmark message accepted");
        component
            .run_now(&mut EmptyWorld)
            .expect("benchmark actions finish within fixed budget");
    }
    black_box(
        component
            .snapshot()
            .expect("benchmark component stays live"),
    );
}

fn verify(graphs: &Arc<GraphStore>, inputs: &[(i64, bool); MESSAGES]) {
    let mut component = start(graphs);
    execute(&mut component, inputs);
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot.diagnostics.is_empty(),
        "{:?}",
        snapshot.diagnostics
    );
    for (id, expected) in [
        (200, Value::Float(2.0)),
        (201, Value::Integer(1)),
        (202, Value::Float(2.0)),
        (203, Value::Integer(-1)),
        (204, Value::Integer(20)),
        (205, Value::Float(32.0)),
    ] {
        assert_eq!(
            snapshot.variables.get(&id),
            Some(&expected),
            "variable {id}"
        );
    }
    assert!(snapshot.pending_timers.values().all(Vec::is_empty));
    assert!(snapshot.bindings.is_empty());
    // Also validate each conditional and Switch path before timed sampling.
    // Rotate only the final input: every run retains the same 32-message count.
    for value in 0..3 {
        for condition in [false, true] {
            let mut varied = *inputs;
            varied[MESSAGES - 1] = (value, condition);
            let mut component = start(graphs);
            execute(&mut component, &varied);
            let snapshot = component.snapshot().unwrap();
            assert!(snapshot.diagnostics.is_empty());
            assert_eq!(
                snapshot.variables.get(&200),
                Some(&Value::Float((value + 1) as f64))
            );
            assert_eq!(
                snapshot.variables.get(&203),
                Some(&Value::Integer(if condition { 1 } else { -1 }))
            );
            assert_eq!(
                snapshot.variables.get(&204),
                Some(&Value::Integer((value + 1) * 10))
            );
            assert_eq!(snapshot.variables.get(&205), Some(&Value::Float(32.0)));
        }
    }
}

fn script_actions(c: &mut Criterion) {
    let data = fixture();
    let graphs = Arc::new(GraphStore::from_json(&data).expect("benchmark graph loads"));
    let inputs = std::array::from_fn(|index| ((index % 3) as i64, index % 2 == 0));
    // This check also runs with `cargo test --all-targets`; timing samples only
    // have meaning if the public execution path actually produces these values.
    verify(&graphs, &inputs);

    let mut load = c.benchmark_group("script_actions/load");
    load.throughput(Throughput::Bytes(data.len() as u64));
    load.bench_function("synthetic_11_nodes", |b| {
        b.iter(|| black_box(GraphStore::from_json(black_box(&data)).unwrap()));
    });
    load.finish();

    let mut execution = c.benchmark_group("script_actions/execute");
    execution.throughput(Throughput::Elements(MESSAGES as u64));
    execution.bench_function("dynamic_32_messages", |b| {
        b.iter_batched_ref(
            || start(&graphs),
            |component| execute(component, &inputs),
            BatchSize::LargeInput,
        );
    });
    execution.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(50)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(5))
        .nresamples(20_000);
    targets = script_actions
}
criterion_main!(benches);
