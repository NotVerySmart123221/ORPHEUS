use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use orpheus_statescript::{
    script_expr::{EntityRelation, native_shape, to_float},
    script_graph::{GraphStore, ModeValue},
    script_init::{Component, MAX_INIT_WORK, Root, Site, World},
    script_runtime::{FRAME_MS, GAME_MODE_ENTITY},
    script_wire::{Value, Variables},
};
use serde_json::Value as Json;

const GRAPH: u32 = 0x0b8b;
const ROOT: u16 = 1;
const OWNER: u32 = 701;
const CONTROLLER: u32 = 702;
const PRACTICE_MODE: u64 = 0x0230_0000_0000_0018;
const PASSIVE_ENTRY_STATE: u16 = 0;
const PASSIVE_WAIT_STATE: u16 = 1;
const BONUS: u64 = 0x0240_0000_0000_00eb;
const BONUS_AMOUNT: u32 = 0x2a;
const METER: u32 = 342;
const METER_CAP: u32 = 338;
const ADMITTED: u32 = 2426;
const INHIBITED: u32 = 842;

struct ChargeHost<'a> {
    mode_variables: &'a BTreeMap<u32, ModeValue>,
}

impl World for ChargeHost<'_> {
    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn related_entity(&mut self, site: Site, relation: EntityRelation, entity: u32) -> Option<u32> {
        (site.entity == OWNER && entity == OWNER && relation == EntityRelation::CurrentPossessor)
            .then_some(CONTROLLER)
    }

    fn native(&mut self, site: Site, config: &Json) -> Option<Value> {
        (site.entity == OWNER && native_shape(config, "STU_A3DEF368", None))
            .then_some(Value::Entity(GAME_MODE_ENTITY))
    }

    fn entity_variable(&mut self, entity: u32, id: u32) -> Option<Value> {
        (entity == GAME_MODE_ENTITY).then(|| {
            self.mode_variables
                .get(&id)
                .copied()
                .map_or(Value::Null, Value::from)
        })
    }

    fn player_stats_available(&self, site: Site) -> Option<bool> {
        // This detached graph harness does not mount a player stats record.
        (site.entity == OWNER).then_some(false)
    }
}

fn meter(body: &Component) -> f64 {
    to_float(&body.snapshot().unwrap().variables[&METER])
}

fn wait_deadline(body: &Component) -> Option<u64> {
    let snapshot = body.snapshot().unwrap();
    let mut waits = snapshot
        .pending_timers
        .get(&ROOT)
        .into_iter()
        .flatten()
        .filter(|timer| timer.raw_state == PASSIVE_WAIT_STATE && !timer.finish);
    let due = waits.next().map(|timer| timer.due_ms);
    assert!(waits.next().is_none(), "duplicate passive wait");
    due
}

fn check_passive_credit(
    body: &mut Component,
    world: &mut impl World,
    due: u64,
    before: f64,
    after: f64,
) {
    assert_eq!(wait_deadline(body), Some(due));
    let due_frame = u32::try_from(due / FRAME_MS).unwrap();
    body.advance_frame(due_frame - 1, world).unwrap();
    assert_eq!(meter(body), before, "no early passive credit");
    body.advance_frame(due_frame, world).unwrap();
    assert_eq!(meter(body), after);
}

#[test]
#[ignore = "requires an authorized 104319 graph pack in ORPHEUS_RUNTIME_DIR"]
fn prepared_stock_charge_uses_authored_passive_clock_and_bonus_entry() {
    let runtime =
        PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").expect("set ORPHEUS_RUNTIME_DIR"));
    let graphs = Arc::new(GraphStore::load(runtime.join("data/game_graphs_174.json.gz")).unwrap());
    let mut world = ChargeHost {
        mode_variables: graphs
            .mode_variables(PRACTICE_MODE)
            .expect("prepared Practice mode variables"),
    };
    let variables = Variables::from([
        (METER_CAP, Value::Float(11.0)),
        (METER, Value::Float(0.0)),
        (ADMITTED, Value::Bool(false)),
        (INHIBITED, Value::Bool(false)),
    ]);
    // Start the actual passive state directly. The graph and its bonus
    // receiver are unmodified, and unrelated entry chains are outside this test.
    let mut body = Component::start(
        Arc::clone(&graphs),
        OWNER,
        variables,
        vec![Root::listener(ROOT, GRAPH, Some(PASSIVE_ENTRY_STATE))],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    let initial_rng = body.random_checkpoint();
    assert_eq!(wait_deadline(&body), None);

    body.set_entity_variable(ADMITTED, Value::Bool(true), &mut world)
        .unwrap();
    body.run_now(&mut world).unwrap();
    check_passive_credit(&mut body, &mut world, 1_000, 0.0, 5.0);
    assert_eq!(wait_deadline(&body), Some(2_000));

    body.set_entity_variable(ADMITTED, Value::Bool(false), &mut world)
        .unwrap();
    body.run_now(&mut world).unwrap();
    assert_eq!(wait_deadline(&body), None);
    let bonus = Variables::from([(BONUS_AMOUNT, Value::Float(2.5))]);
    assert_eq!(body.queue_message(ROOT, BONUS, bonus.clone()).unwrap(), 1);
    body.run_now(&mut world).unwrap();
    body.advance_frame(u32::try_from(2_000 / FRAME_MS).unwrap(), &mut world)
        .unwrap();
    assert_eq!(
        meter(&body),
        5.0,
        "closed admission blocks the timer and bonus"
    );

    let resumed_at = body.now_ms();
    body.set_entity_variable(ADMITTED, Value::Bool(true), &mut world)
        .unwrap();
    body.run_now(&mut world).unwrap();
    check_passive_credit(&mut body, &mut world, resumed_at + 1_000, 5.0, 10.0);
    assert_eq!(wait_deadline(&body), Some(resumed_at + 2_000));
    assert_eq!(body.queue_message(ROOT, BONUS, bonus).unwrap(), 1);
    body.run_now(&mut world).unwrap();
    assert_eq!(
        meter(&body),
        11.0,
        "native bonus clamps to the configured cap"
    );
    assert_eq!(wait_deadline(&body), None);
    assert_eq!(body.random_checkpoint(), initial_rng);
    let diagnostics = &body.snapshot().unwrap().diagnostics;
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}
