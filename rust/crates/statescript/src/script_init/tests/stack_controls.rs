use super::*;
use crate::script_expr::Scope;

#[test]
fn authored_stack_claim_shares_priority_ties_and_restores_each_scope() {
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Step {
        frame: u32,
        #[serde(default)]
        messages: Vec<(u16, u64)>,
        value: Option<i64>,
        destroy: Option<u16>,
    }

    #[derive(Deserialize)]
    struct Input {
        bundle: Json,
        graph: u32,
        stack_variable: u32,
        live_variable: u32,
        query: u64,
        steps: Vec<Step>,
    }

    #[derive(Deserialize)]
    struct Expected {
        entity: i64,
        owner: Option<u16>,
        finished: bool,
        instances: Vec<(u16, i64, Option<bool>, [bool; 3])>,
    }

    #[derive(Deserialize)]
    struct Contract {
        schema_version: u8,
        case_id: String,
        evidence: String,
        input: Input,
        expected: Vec<Expected>,
    }

    let contract: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-stack-claim-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.case_id, "script-stack-claim.synthetic.v1");
    assert_eq!(contract.evidence, "synthetic-rust");
    assert_eq!(contract.input.steps.len(), contract.expected.len());

    let input = contract.input;
    let graphs =
        Arc::new(GraphStore::from_json(&serde_json::to_vec(&input.bundle).unwrap()).unwrap());
    let roots = (1..=3)
        .map(|instance| {
            let mut root = Root::new(instance, input.graph);
            root.variables.insert(
                input.stack_variable,
                Value::Integer(100 + i64::from(instance)),
            );
            root
        })
        .collect();
    let mut world = EmptyWorld;
    let mut component = Component::start(
        graphs,
        52,
        Variables::from([
            (input.stack_variable, Value::Integer(7)),
            (input.live_variable, Value::Integer(10)),
        ]),
        roots,
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    let entity_var = Variable {
        scope: Scope::Entity,
        id: input.stack_variable,
    };
    let instance_var = Variable {
        scope: Scope::Instance,
        ..entity_var
    };

    for (step, expected) in input.steps.into_iter().zip(contract.expected) {
        for (instance, message) in step.messages {
            let delivered = if step.frame == 0 {
                component.queue_message(instance, message, Variables::new())
            } else {
                component.queue_frame_message(step.frame, instance, message, Variables::new())
            };
            assert_eq!(delivered.unwrap(), 1);
        }
        if step.frame != 0 {
            component.advance_frame(step.frame, &mut world).unwrap();
        }
        if let Some(value) = step.value {
            component
                .set_entity_variable(input.live_variable, Value::Integer(value), &mut world)
                .unwrap();
        }
        if let Some(instance) = step.destroy {
            component
                .run(&mut world, |vm| vm.destroy(instance))
                .unwrap();
        }
        component.run_now(&mut world).unwrap();
        for &(instance, _, _, _) in &expected.instances {
            assert_eq!(
                component
                    .queue_message(instance, input.query, Variables::new())
                    .unwrap(),
                1
            );
        }
        component.run_now(&mut world).unwrap();

        assert_eq!(
            component.resolved_variable(1, entity_var),
            Some(Value::Integer(expected.entity)),
            "entity stack at frame {}",
            step.frame
        );
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.diagnostics.is_empty(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(snapshot.variables[&input.stack_variable], Value::Integer(7));
        let owner = snapshot
            .bindings
            .get(&(0, input.stack_variable))
            .and_then(|stack| stack.last())
            .map(|owner| owner.instance);
        assert_eq!(owner, expected.owner, "top owner at frame {}", step.frame);
        assert_eq!(snapshot.instances.len(), expected.instances.len());
        assert_eq!(
            snapshot
                .instances
                .get(&2)
                .and_then(|instance| instance.variables.get(&82))
                == Some(&Value::Bool(true)),
            expected.finished,
        );
        for (instance, value, top, allows) in expected.instances {
            let variables = &snapshot.instances[&instance].variables;
            assert_eq!(
                component.resolved_variable(instance, instance_var),
                Some(Value::Integer(value)),
                "instance {instance} stack at frame {}",
                step.frame
            );
            assert_eq!(variables.get(&80), top.map(Value::Bool).as_ref());
            for (variable, allowed) in [83, 84, 85].into_iter().zip(allows) {
                assert_eq!(
                    variables[&variable],
                    Value::Bool(allowed),
                    "instance {instance} admission {variable} at frame {}",
                    step.frame
                );
            }
        }
        if let Some(instance) = step.destroy {
            assert!(!snapshot.pending_timers.contains_key(&instance));
            assert!(
                !snapshot
                    .bindings
                    .contains_key(&(instance, input.stack_variable))
            );
            assert!(
                snapshot
                    .bindings
                    .values()
                    .flatten()
                    .all(|binding| binding.instance != instance)
            );
        }
    }
}

#[test]
fn authored_stack_claim_rejects_map_values_without_losing_a_valid_binding() {
    const ROOT: u16 = 1;
    const GRAPH: u32 = 42;
    const TARGET: u32 = 90;
    const SOURCE: u32 = 91;
    const ENTERED_TOP: u32 = 80;

    let mut claim = node(
        1,
        "authored.claim",
        Some(0),
        json!({
            "variable":{"scope":"entity","id":TARGET},
            "value":var(SOURCE,true),
            "priority":{"level":2,"above_equals":false},
            "duration_frames":3,
            "m_topSubgraphPlug":plug(&[(2,"m_inPlug")],&[]),
        }),
    );
    claim["server_only"] = json!(true);
    claim["state_behavior"] = json!("stack_claim");
    let graphs = Arc::new(store(vec![graph(
        GRAPH,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1,"m_beginPlug")],&[])}),
            ),
            claim,
            node(
                2,
                "STUStatescriptActionSetVar",
                None,
                json!({"m_out_Var":var(ENTERED_TOP,false),"m_value":true}),
            ),
        ],
        json!([1]),
        json!([0]),
    )]));
    let start = |value| {
        Component::start(
            Arc::clone(&graphs),
            52,
            Variables::from([(TARGET, Value::Integer(7)), (SOURCE, value)]),
            vec![Root::new(ROOT, GRAPH)],
            0,
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap()
    };
    let target = Variable {
        scope: Scope::Entity,
        id: TARGET,
    };
    let assert_rejection = |snapshot: &Snapshot| {
        assert_eq!(snapshot.diagnostics.len(), 1);
        let diagnostic = &snapshot.diagnostics[0];
        assert!(diagnostic.server_only);
        assert_eq!(
            (diagnostic.site.graph, diagnostic.site.node),
            (GRAPH, Some(1))
        );
        assert_eq!(
            diagnostic.issue,
            Issue::UnsupportedField("map-valued stack claim")
        );
    };

    for invalid in [
        Value::Map(vec![(Value::Integer(1), Value::Integer(2))]),
        Value::IdentifierMap(crate::script_destination::IdentifierMap::default()),
    ] {
        let rejected = start(invalid.clone());
        let snapshot = rejected.snapshot().unwrap();
        assert_rejection(snapshot);
        assert!(snapshot.bindings.is_empty());
        assert!(snapshot.pending_timers.is_empty());
        assert!(
            !snapshot.instances[&ROOT]
                .variables
                .contains_key(&ENTERED_TOP)
        );
        assert_eq!(
            rejected.resolved_variable(ROOT, target),
            Some(Value::Integer(7))
        );

        let mut active = start(Value::Integer(12));
        let initial = active.snapshot().unwrap();
        assert!(initial.diagnostics.is_empty());
        assert_eq!(
            initial.instances[&ROOT].variables[&ENTERED_TOP],
            Value::Bool(true)
        );
        assert_eq!(
            active.resolved_variable(ROOT, target),
            Some(Value::Integer(12))
        );
        let timers = initial.pending_timers.clone();
        assert_eq!(timers[&ROOT].len(), 1);
        active
            .set_entity_variable(SOURCE, invalid, &mut EmptyWorld)
            .unwrap();
        active.run_now(&mut EmptyWorld).unwrap();
        let snapshot = active.snapshot().unwrap();
        assert_rejection(snapshot);
        assert_eq!(snapshot.bindings[&(0, TARGET)].len(), 1);
        assert_eq!(snapshot.pending_timers, timers);
        assert_eq!(
            active.resolved_variable(ROOT, target),
            Some(Value::Integer(12))
        );
    }
}
