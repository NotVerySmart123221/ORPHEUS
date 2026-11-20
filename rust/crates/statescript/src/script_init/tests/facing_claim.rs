use serde::Deserialize;

use super::*;
use crate::script_expr::StackPriority;

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    case_id: String,
    evidence: String,
    input: Input,
    expected: Expected,
}

#[derive(Deserialize)]
struct Input {
    body: u32,
    graph: u32,
    work_budget: usize,
    priority: Json,
    initial: [f64; 3],
    refresh: [f64; 3],
    invalid: [f64; 3],
}

#[derive(Deserialize)]
struct Expected {
    initial: [f64; 3],
    refresh: [f64; 3],
}

#[derive(Default)]
struct Host {
    claim: Option<(Site, AiFacing)>,
    releases: Vec<(Site, AiFacing)>,
    unavailable: bool,
}

impl World for Host {
    fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
        None
    }

    fn set_facing_claim(&mut self, site: Site, record: AiFacing, active: bool) -> Option<bool> {
        if active {
            if self.unavailable {
                return None;
            }
            self.claim = Some((site, record));
        } else {
            assert_eq!(self.claim.take(), Some((site, record)));
            self.releases.push((site, record));
        }
        Some(true)
    }
}

#[test]
fn authored_facing_refreshes_and_releases_its_own_claim() {
    const FRAME_BUDGET_TRIGGER: u64 = 0x0F01_0000_0000_0061;
    const DIRECTION: u32 = 42;
    const KEY: StateKey = StateKey {
        instance: 1,
        raw: 0,
    };

    let contract: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/statescript-facing-claim-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.case_id, "statescript-facing-claim.synthetic.v1");
    assert_eq!(contract.evidence, "synthetic-rust");
    let Input {
        body,
        graph: graph_id,
        work_budget,
        priority,
        initial,
        refresh,
        invalid,
    } = contract.input;
    let Expected {
        initial: expected_initial,
        refresh: expected_refresh,
    } = contract.expected;

    let entry = node(
        0,
        "STUStatescriptEntry",
        None,
        json!({
            "m_outPlug": plug(&[(1, "m_beginPlug")], &[]),
        }),
    );
    let mut facing = node(
        1,
        "AuthoredFacing",
        Some(0),
        json!({
            "direction": var(DIRECTION, true),
            "priority": priority,
        }),
    );
    facing["state_behavior"] = json!("facing_claim");
    facing["server_only"] = json!(true);
    let mut loop_action = node(
        2,
        "AuthoredLoopAction",
        None,
        json!({
            "m_outPlug": plug(&[(2, "m_inPlug")], &[]),
        }),
    );
    loop_action["action_behavior"] = json!("forward");
    loop_action["server_only"] = json!(true);
    let trigger = node(
        3,
        "STUStatescriptEntryGameMessage",
        None,
        json!({
            "m_gameMessage": {"m_gameMessage": format!("0x{FRAME_BUDGET_TRIGGER:016X}")},
            "m_outPlug": plug(&[(2, "m_inPlug")], &[]),
        }),
    );
    let graphs = Arc::new(store(vec![graph(
        graph_id,
        vec![entry, facing, loop_action, trigger],
        json!([1]),
        json!([0, 3]),
    )]));
    let site = Site {
        entity: body,
        instance: 1,
        graph: graph_id,
        node: Some(1),
    };
    let record = |direction| AiFacing {
        target: body,
        direction,
        priority: StackPriority {
            level: 17,
            above_equals: true,
        },
    };
    let mut host = Host::default();
    let mut component = Component::start(
        graphs,
        body,
        Variables::from([(DIRECTION, Value::Vector(initial))]),
        vec![Root::new(1, graph_id)],
        0,
        &mut host,
        work_budget,
    )
    .unwrap();
    assert_eq!(host.claim, Some((site, record(expected_initial))));

    component
        .set_entity_variable(DIRECTION, Value::Vector(refresh), &mut host)
        .unwrap();
    component.advance_frame(1, &mut host).unwrap();
    let updated = record(expected_refresh);
    assert_eq!(host.claim, Some((site, updated)));
    component
        .set_entity_variable(DIRECTION, Value::Vector(invalid), &mut host)
        .unwrap();
    component.advance_frame(2, &mut host).unwrap();
    assert_eq!(host.claim, None);
    assert_eq!(host.releases, [(site, updated)]);
    assert!(active(component.snapshot().unwrap(), 1).is_empty());

    component
        .set_entity_variable(DIRECTION, Value::Vector(refresh), &mut host)
        .unwrap();
    component.run(&mut host, |vm| vm.begin(KEY, 0)).unwrap();
    component
        .run(&mut host, |vm| vm.end(KEY, false, 0))
        .unwrap();
    assert_eq!(host.claim, None);
    assert_eq!(host.releases, [(site, updated); 2]);

    component.run(&mut host, |vm| vm.begin(KEY, 0)).unwrap();
    host.unavailable = true;
    component.advance_frame(3, &mut host).unwrap();
    assert_eq!(host.claim, None);
    assert_eq!(host.releases, [(site, updated); 3]);
    host.unavailable = false;

    component.run(&mut host, |vm| vm.begin(KEY, 0)).unwrap();
    assert_eq!(host.claim, Some((site, updated)));
    assert_eq!(
        component
            .queue_frame_message(4, 1, FRAME_BUDGET_TRIGGER, Variables::new())
            .unwrap(),
        1,
    );
    let fault = component.advance_frame(4, &mut host).unwrap_err();
    assert!(
        matches!(fault, InitFault::WorkBudget { limit, .. } if limit == work_budget),
        "{fault:?}"
    );
    assert_eq!(host.claim, Some((site, updated)));
    component.teardown(&mut host).unwrap();
    component.teardown(&mut host).unwrap();
    assert_eq!(host.claim, None);
    assert_eq!(host.releases, [(site, updated); 4]);
}
