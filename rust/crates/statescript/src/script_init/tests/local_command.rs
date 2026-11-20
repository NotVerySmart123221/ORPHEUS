use std::cell::RefCell;

use serde::Deserialize;

use crate::script_expr::EvalError;

use super::*;

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    case_id: String,
    evidence: String,
    input: Input,
    expected: Vec<Expected>,
}

#[derive(Deserialize)]
struct Input {
    remote_frame: u32,
    remote_buttons: u32,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    with_remote_command: bool,
    local_command_present: Option<bool>,
    operand: Json,
}

#[derive(Deserialize)]
struct Expected {
    branch_value: Option<bool>,
    unsupported: Option<String>,
    query_count: usize,
}

struct ProcessWorld {
    local_command_present: Option<bool>,
    queries: RefCell<Vec<Site>>,
}

impl World for ProcessWorld {
    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }

    fn local_command_present(&self, site: Site) -> Option<bool> {
        self.queries.borrow_mut().push(site);
        self.local_command_present
    }
}

#[test]
fn local_command_absence_is_independent_of_remote_held_buttons() {
    let contract: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/statescript-local-command-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.case_id, "statescript-local-command.synthetic.v1");
    assert_eq!(contract.evidence, "synthetic-rust");
    let Input {
        remote_frame,
        remote_buttons,
        cases,
    } = contract.input;
    assert_eq!(cases.len(), contract.expected.len());

    const ENTITY: u32 = 136;
    const INSTANCE: u16 = 1;
    const GRAPH: u32 = 827;
    const RESULT: u32 = 47;
    let site = Site {
        entity: ENTITY,
        instance: INSTANCE,
        graph: GRAPH,
        node: Some(0),
    };
    for (case, expected) in cases.into_iter().zip(contract.expected) {
        let graphs = store(vec![graph(
            GRAPH,
            vec![
                node(
                    0,
                    "STUStatescriptStateBooleanSwitch",
                    Some(0),
                    json!({
                        "m_condition": case.operand,
                        "m_onFalsePlug": plug(&[(1, "m_inPlug")], &[]),
                    }),
                ),
                node(
                    1,
                    "STUStatescriptActionSetVar",
                    None,
                    json!({"m_out_Var": var(RESULT, false), "m_value": false}),
                ),
            ],
            json!([0]),
            json!([]),
        )]);
        let mut world = ProcessWorld {
            local_command_present: case.local_command_present,
            queries: RefCell::default(),
        };
        let initial_state = (!case.with_remote_command).then_some(0);
        let mut component = Component::start(
            Arc::new(graphs),
            ENTITY,
            Variables::new(),
            vec![Root::listener(INSTANCE, GRAPH, initial_state)],
            0,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        if case.with_remote_command {
            component
                .advance_command(
                    Command {
                        frame: remote_frame,
                        buttons: remote_buttons,
                        ..Command::default()
                    },
                    &mut world,
                )
                .unwrap();
            assert_eq!(component.command_buttons(), remote_buttons);
            assert!(!component.machine.held_buttons.is_empty());
            component.begin_state(INSTANCE, 0, &mut world).unwrap();
        } else {
            assert_eq!(component.command_buttons(), 0);
            assert!(component.machine.held_buttons.is_empty());
        }

        let snapshot = component.snapshot().unwrap();
        let instance = &snapshot.instances[&INSTANCE];
        assert_eq!(
            instance.variables.get(&RESULT),
            expected.branch_value.map(Value::Bool).as_ref(),
            "{}",
            case.name
        );
        let state = &instance.states[&0];
        assert!(state.reads_untrusted_native(), "{}", case.name);
        if let Some(class) = expected.unsupported {
            assert!(
                matches!(
                    snapshot.diagnostics.as_slice(),
                    [Diagnostic {
                        site: diagnostic_site,
                        issue: Issue::Expression(EvalError::UnsupportedConfig(actual)),
                        ..
                    }] if *diagnostic_site == site && *actual == class
                ),
                "{}: {:?}",
                case.name,
                snapshot.diagnostics
            );
        } else {
            assert!(snapshot.diagnostics.is_empty(), "{}", case.name);
        }
        assert_eq!(
            world.queries.into_inner(),
            vec![site; expected.query_count],
            "{}",
            case.name
        );
    }
}
