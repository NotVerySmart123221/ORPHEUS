//! Synthetic action semantics pinned to the pre-lowering Rust executor.
//! This is not Python or original-client equivalence evidence.

use super::*;
use crate::script_expr::EvalError;
use std::collections::HashSet;

const RANDOM_INCREMENT: u64 = 0x9e37_79b9_7f4a_7c15;

fn literal(value: &Json) -> Value {
    match value {
        Json::Null => Value::Null,
        Json::Bool(value) => Value::Bool(*value),
        Json::Number(value) if value.is_i64() => Value::Integer(value.as_i64().unwrap()),
        Json::Number(value) => Value::Float(value.as_f64().unwrap()),
        Json::String(value) => Value::String(value.clone()),
        Json::Array(items) => Value::Array(items.iter().map(literal).collect()),
        Json::Object(_) => panic!("contract variable values must be JSON literals"),
    }
}

fn variables(input: Option<&Json>) -> Variables {
    input
        .and_then(Json::as_object)
        .into_iter()
        .flatten()
        .map(|(key, value)| (key.parse().unwrap(), literal(value)))
        .collect()
}

fn value_json(value: &Value) -> Json {
    match value {
        Value::Null => Json::Null,
        Value::Bool(value) => json!(value),
        Value::Integer(value) => json!(value),
        Value::Float(value) => json!(value),
        Value::String(value) => json!(value),
        Value::Array(items) => Json::Array(items.iter().map(value_json).collect()),
        other => {
            panic!("action contract produced a value outside its declared encoding: {other:?}")
        }
    }
}

fn variables_json(variables: &Variables) -> Json {
    Json::Object(
        variables
            .iter()
            .map(|(id, value)| (id.to_string(), value_json(value)))
            .collect(),
    )
}

fn diagnostics_json(snapshot: &Snapshot, instance: u16, graph: u32) -> Json {
    Json::Array(snapshot.diagnostics.iter().map(|diagnostic| {
        assert_eq!(diagnostic.site.entity, snapshot.entity);
        assert_eq!(diagnostic.site.instance, instance);
        assert_eq!(diagnostic.site.graph, graph);
        let position = diagnostic.site.node.unwrap();
        match &diagnostic.issue {
            Issue::Expression(EvalError::MalformedConfig(detail)) => json!({
                "node":position,"kind":"expression","error":"malformed_config","detail":detail,
            }),
            Issue::Expression(EvalError::UnsupportedConfig(detail)) => json!({
                "node":position,"kind":"expression","error":"unsupported_config","detail":detail,
            }),
            Issue::UnsupportedField(detail) => json!({"node":position,"kind":"unsupported_field","detail":detail}),
            Issue::MissingNode(target) => json!({"node":position,"kind":"missing_node","position":target}),
            Issue::UnsupportedAction(class) => json!({"node":position,"kind":"unsupported_action","class":class}),
            other => panic!("unexpected action contract diagnostic: {other:?}"),
        }
    }).collect())
}

fn assert_result(result: Result<(), InitFault>, expected: &Json, case: &str) {
    match expected.get("fault") {
        Some(fault) => {
            assert_eq!(fault["kind"], "work_budget", "{case}");
            let Err(InitFault::WorkBudget { limit, operation }) = result else {
                panic!("{case}: expected a work-budget fault, got {result:?}");
            };
            assert_eq!(limit as u64, fault["limit"].as_u64().unwrap(), "{case}");
            assert_eq!(operation, fault["operation"].as_str().unwrap(), "{case}");
        }
        None => result.unwrap_or_else(|error| panic!("{case}: {error}")),
    }
}

fn assert_snapshot(
    component: &Component,
    expected: &Json,
    instance: u16,
    graph: u32,
    seed: u64,
    case: &str,
) {
    // The retained machine also lets an exhaustion case prove that the write
    // was not committed before Component correctly entered its halted state.
    let snapshot = &component.machine.snapshot;
    assert_eq!(
        variables_json(&snapshot.variables),
        expected["entity"],
        "{case}: entity variables"
    );
    assert_eq!(
        variables_json(&snapshot.instances[&instance].variables),
        expected["instance"],
        "{case}: instance variables"
    );
    assert_eq!(
        diagnostics_json(snapshot, instance, graph),
        expected["diagnostics"],
        "{case}: diagnostics"
    );
    let draws = expected["random_draws_total"].as_u64().unwrap();
    assert_eq!(
        component
            .machine
            .random_state
            .load(std::sync::atomic::Ordering::Relaxed),
        seed.wrapping_add(draws.wrapping_mul(RANDOM_INCREMENT)),
        "{case}: random consumption"
    );
    let checkpoint = component.random_checkpoint();
    assert_eq!(checkpoint.initial_seed, seed, "{case}: random provenance");
    assert_eq!(checkpoint.draws, draws, "{case}: checkpoint draws");
    if let Some(remaining) = expected.get("work_remaining") {
        assert_eq!(
            component.machine.work_left as u64,
            remaining.as_u64().unwrap(),
            "{case}: work remaining"
        );
    }
    assert_eq!(
        component.halted,
        expected.get("fault").is_some(),
        "{case}: halted state"
    );
}

#[test]
fn actions_preserve_the_pinned_synthetic_reference_contract() {
    let contract: Json = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-actions-v1.json"
    )))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "script-actions.synthetic-reference.v1");
    assert_eq!(contract["target_build"], 104319);
    assert_eq!(contract["evidence"], "synthetic-rust-reference");
    assert_eq!(
        contract["source"]["revision"],
        "f763beb9d7017b9364a3c8fc79ae6192df590043"
    );
    let harness = &contract["harness"];
    let entity = u32::try_from(harness["entity"].as_u64().unwrap()).unwrap();
    let instance = u16::try_from(harness["instance"].as_u64().unwrap()).unwrap();
    let graph_id = u32::try_from(harness["graph"].as_u64().unwrap()).unwrap();
    let frame = u32::try_from(harness["frame"].as_u64().unwrap()).unwrap();
    let seed = u64::from(entity) << 32 | u64::from(frame);
    let cases = contract["cases"].as_array().unwrap();
    assert!(!cases.is_empty());
    let mut seen = HashSet::new();
    for case in cases {
        let name = case["id"].as_str().unwrap();
        assert!(seen.insert(name), "duplicate contract case {name}");
        let input = &case["input"];
        let entry = u16::try_from(harness["entry_node"].as_u64().unwrap()).unwrap();
        let entry_targets: Vec<_> = input
            .get("entry_targets")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .map(|target| (u16::try_from(target.as_u64().unwrap()).unwrap(), "m_inPlug"))
            .collect();
        let mut nodes = vec![node(
            entry,
            "STUStatescriptEntry",
            None,
            json!({"m_outPlug":plug(&entry_targets, &[])}),
        )];
        for declaration in input["nodes"].as_array().unwrap() {
            let position = u16::try_from(declaration["pos"].as_u64().unwrap()).unwrap();
            assert_ne!(position, entry, "{name}: action collides with the entry");
            let mut raw = node(
                position,
                declaration["class"].as_str().unwrap(),
                None,
                declaration["fields"].clone(),
            );
            if let Some(client_only) = declaration.get("client_only") {
                raw["client_only"] = client_only.clone();
            }
            nodes.push(raw);
        }
        let items = nodes
            .iter()
            .map(|node| node["pos"].as_u64().unwrap())
            .max()
            .unwrap()
            + 1;
        let entries = input
            .get("entries")
            .unwrap_or(&harness["default_entries"])
            .clone();
        let mut raw_graph = graph(graph_id, nodes, json!([]), entries);
        raw_graph["items"] = json!(items);
        // Use the same GraphStore loader as runtime packs, without a test interpreter.
        let graphs = Arc::new(store(vec![raw_graph]));
        let mut root = Root::new(instance, graph_id);
        root.variables = variables(input.get("instance"));
        let budget = input
            .get("work_budget")
            .unwrap_or(&harness["default_work_budget"])
            .as_u64()
            .unwrap();
        let mut component = Component::start(
            Arc::clone(&graphs),
            entity,
            variables(input.get("entity")),
            vec![root],
            frame,
            &mut EmptyWorld,
            usize::try_from(budget).unwrap(),
        )
        .unwrap_or_else(|error| panic!("{name}: startup failed: {error}"));
        let expected_initial = input
            .get("expected_initial_diagnostics")
            .cloned()
            .unwrap_or_else(|| json!([]));
        assert_eq!(
            diagnostics_json(component.snapshot().unwrap(), instance, graph_id),
            expected_initial,
            "{name}: dormant inputs must not emit runtime diagnostics"
        );
        let steps = input["steps"].as_array().unwrap();
        assert!(!steps.is_empty(), "{name}: empty behavior sequence");
        for step in steps {
            match step["op"].as_str().unwrap() {
                "set_entity" => {
                    let variable = u32::try_from(step["id"].as_u64().unwrap()).unwrap();
                    component
                        .set_entity_variable(variable, literal(&step["value"]), &mut EmptyWorld)
                        .unwrap();
                }
                "invoke_node" => {
                    let position = u16::try_from(step["node"].as_u64().unwrap()).unwrap();
                    let mut taken = None;
                    let result = component.run(&mut EmptyWorld, |vm| {
                        let node = vm.graph(instance).node(position).unwrap();
                        taken = Some(vm.run_node(instance, node, None, 0)?);
                        Ok(())
                    });
                    assert_result(result, &step["expected"], name);
                    assert_eq!(
                        taken,
                        step["expected"]
                            .get("taken")
                            .map(|value| value.as_bool().unwrap()),
                        "{name}: taken"
                    );
                    assert_snapshot(
                        &component,
                        &step["expected"],
                        instance,
                        graph_id,
                        seed,
                        name,
                    );
                }
                "check" => assert_snapshot(
                    &component,
                    &step["expected"],
                    instance,
                    graph_id,
                    seed,
                    name,
                ),
                other => panic!("{name}: unsupported contract operation {other}"),
            }
        }
    }
}

#[test]
fn rng_checkpoints_preserve_the_stream_and_clone_state() {
    let component = Component::start(
        Arc::new(store(vec![graph(42, Vec::new(), json!([]), json!([]))])),
        u32::MAX,
        Variables::new(),
        vec![Root::listener(1, 42, None)],
        17,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    let initial = component.random_checkpoint();
    assert_eq!(
        initial,
        RandomCheckpoint {
            model: "splitmix64",
            initial_seed: 0xffff_ffff_0000_0011,
            draws: 0,
        }
    );

    let control = component.clone();
    assert_eq!(component.random_checkpoint(), initial);
    assert_eq!(component.machine.random_u64(), control.machine.random_u64());
    let after_wrap = component.random_checkpoint();
    assert_eq!(
        after_wrap,
        RandomCheckpoint {
            draws: 1,
            ..initial
        }
    );

    let fork = component.clone();
    assert_eq!(fork.random_checkpoint(), after_wrap);
    let second = component.machine.random_u64();
    assert_eq!(fork.random_checkpoint(), after_wrap);
    assert_eq!(fork.machine.random_u64(), second);
    assert_eq!(
        component.random_checkpoint(),
        RandomCheckpoint {
            draws: 2,
            ..initial
        }
    );
    assert_eq!(fork.random_checkpoint(), component.random_checkpoint());
}
