use std::{collections::BTreeMap, fs::File, path::PathBuf, sync::Arc};

use flate2::read::GzDecoder;
use orpheus_statescript::{
    script_expr::{EntityRelation, Scope, Variable, native_shape, to_float},
    script_graph::{GraphStore, ModeValue},
    script_init::{Component, MAX_INIT_WORK, RoomMessage, Root, Site, World, body_roots},
    script_runtime::{FRAME_MS, GAME_MODE_ENTITY},
    script_wire::{Value, Variables},
};
use serde_json::{Value as Json, json};

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
    outgoing: Vec<(u32, RoomMessage)>,
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

    fn queue_room_message(&mut self, _: Site, target: u32, message: RoomMessage) -> bool {
        self.outgoing.push((target, message));
        true
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
        outgoing: Vec::new(),
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

// A synthetic entry isolates one stock action from the surrounding ability.
// Its original inputs, parameter expression and output plug are preserved.
fn bonus_sender_probe(
    bundle: &Json,
    stock_graph: u32,
    position: u16,
    probe_graph: u32,
    trigger: u64,
) -> Json {
    let graph = bundle["graphs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|graph| graph["index"] == stock_graph)
        .expect("prepared sender graph");
    let mut action = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["pos"] == position)
        .expect("prepared sender action")
        .clone();
    assert_eq!(action["class"], "STUStatescriptActionSendGameMessage");
    for key in ["pos", "rid", "uid"] {
        action[key] = json!(1);
    }
    json!({
        "index": probe_graph, "items": 2, "fields": {}, "containers": {},
        "states": [], "entries": [0], "remote": [], "sync_vars": [], "unknown_nodes": [],
        "header": {
            "m_nodesBitCount": 2, "m_statesBitCount": 2,
            "m_remoteSyncNodesBitCount": 2, "m_syncVarsBitCount": 2
        },
        "nodes": [{
            "pos": 0, "rid": 0, "uid": 0, "class": "STUStatescriptEntryGameMessage",
            "state": null, "remote": -1, "client_only": false, "server_only": true,
            "fields": {
                "m_gameMessage": {
                    "$": "STUConfigVarGameMessage",
                    "m_gameMessage": format!("0x{trigger:016X}")
                },
                "m_outPlug": {"$": "Plug", "links": [[1, "m_inPlug"]]}
            }
        }, action]
    })
}

#[test]
#[ignore = "requires a private 104319 bundle with a directed 00EB declaration"]
fn prepared_bonus_actions_preserve_local_and_directed_delivery() {
    const LOCAL: u32 = 61243;
    const FOREIGN: u32 = 61244;
    const TRIGGER: u64 = 0x0240_0000_0000_fa19;
    const SENDER: u32 = 781;

    let path = PathBuf::from(
        std::env::var_os("ORPHEUS_DIRECTED_GRAPH_BUNDLE")
            .expect("set ORPHEUS_DIRECTED_GRAPH_BUNDLE to the annotated private pack"),
    );
    let mut bundle: Json =
        serde_json::from_reader(GzDecoder::new(File::open(path).unwrap())).unwrap();
    let local = bonus_sender_probe(&bundle, 0x006b, 189, LOCAL, TRIGGER);
    let foreign = bonus_sender_probe(&bundle, 0x0b9e, 80, FOREIGN, TRIGGER);
    bundle["graphs"]
        .as_array_mut()
        .unwrap()
        .extend([local, foreign]);
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap());
    let mut world = ChargeHost {
        mode_variables: graphs.mode_variables(PRACTICE_MODE).unwrap(),
        outgoing: Vec::new(),
    };
    let mut local = Root::listener(2, LOCAL, None);
    local.variables.insert(476, Value::Float(2.0));
    let mut receiver = Component::start(
        Arc::clone(&graphs),
        OWNER,
        Variables::from([
            (METER_CAP, Value::Float(11.0)),
            (METER, Value::Float(0.0)),
            (ADMITTED, Value::Bool(true)),
            (INHIBITED, Value::Bool(false)),
        ]),
        vec![Root::listener(ROOT, GRAPH, None), local],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert_eq!(
        receiver
            .queue_message(2, TRIGGER, Variables::new())
            .unwrap(),
        1
    );
    receiver.run_now(&mut world).unwrap();
    assert_eq!(meter(&receiver), 2.0);
    assert!(
        world.outgoing.is_empty(),
        "the stock self sender stays local"
    );

    let mut foreign = Root::listener(ROOT, FOREIGN, None);
    foreign.variables = Variables::from([
        (41, Value::Entity(OWNER)),
        (499, Value::Float(4.0)),
        (174, Value::Float(1.5)),
    ]);
    let mut sender = Component::start(
        Arc::clone(&graphs),
        SENDER,
        Variables::new(),
        vec![foreign],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert_eq!(
        sender
            .queue_message(ROOT, TRIGGER, Variables::new())
            .unwrap(),
        1
    );
    sender.run_now(&mut world).unwrap();
    assert_eq!(world.outgoing.len(), 1);
    let (target, message) = world.outgoing.pop().unwrap();
    assert_eq!(target, OWNER);
    assert_eq!(message.id, BONUS);
    assert_eq!(
        message.parameters,
        Variables::from([(BONUS_AMOUNT, Value::Float(6.0))])
    );
    assert_eq!(
        receiver.queue_room_message_from(SENDER, message).unwrap(),
        1
    );
    receiver.run_now(&mut world).unwrap();
    assert_eq!(meter(&receiver), 2.0);
    receiver.advance_frame(1, &mut world).unwrap();
    assert_eq!(meter(&receiver), 8.0);
    for component in [&receiver, &sender] {
        let diagnostics = &component.snapshot().unwrap().diagnostics;
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
}

#[test]
#[ignore = "requires an authorized 104319 graph pack in ORPHEUS_RUNTIME_DIR"]
fn prepared_stock_admission_controls_local_bonus_credit() {
    const STOCK_BODY: u64 = 0x0400_0000_0000_03cf;
    const CAP_GRAPH: u32 = 0x004b;
    const ADMISSION_GRAPH: u32 = 0x01c7;
    const ADMISSION_ROOT: u16 = 2;
    const ADMISSION_RECEIVER: u16 = 23;
    const ADMISSION_CLAIM: u16 = 24;
    const OPEN_ADMISSION: u64 = 0x0240_0000_0000_0700;
    const DONOR_ROOT: u16 = 3;
    const DONOR_GRAPH: u32 = 61245;
    const DONOR_TRIGGER: u64 = 0x0240_0000_0000_fa1a;
    const DONOR_AMOUNT: u32 = 476;

    let runtime =
        PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").expect("set ORPHEUS_RUNTIME_DIR"));
    let pack = File::open(runtime.join("data/game_graphs_174.json.gz")).unwrap();
    let mut bundle: Json = serde_json::from_reader(GzDecoder::new(pack)).unwrap();
    let donor = bonus_sender_probe(&bundle, 0x006b, 189, DONOR_GRAPH, DONOR_TRIGGER);
    bundle["graphs"].as_array_mut().unwrap().push(donor);
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap());

    let stock_roots = body_roots(&graphs.bodies()[&STOCK_BODY]).unwrap();
    let cap_binding = stock_roots
        .iter()
        .find(|root| root.graph == CAP_GRAPH)
        .expect("stock charge cap binding");
    let cap = cap_binding.variables[&METER_CAP].clone();
    let maximum = to_float(&cap);
    assert!(maximum.is_finite() && maximum > 0.0);
    let credit = maximum * 0.75;
    let mut donor = Root::listener(DONOR_ROOT, DONOR_GRAPH, None);
    donor.variables.insert(DONOR_AMOUNT, Value::Float(credit));
    let mut world = ChargeHost {
        mode_variables: graphs.mode_variables(PRACTICE_MODE).unwrap(),
        outgoing: Vec::new(),
    };
    // Isolate the native admission provider and bonus receiver from room startup
    // and passive credit. The admission variable is written only by the stock graph.
    let mut body = Component::start(
        Arc::clone(&graphs),
        OWNER,
        Variables::from([(METER_CAP, cap), (METER, Value::Float(0.0))]),
        vec![
            Root::listener(ROOT, GRAPH, None),
            Root::listener(ADMISSION_ROOT, ADMISSION_GRAPH, Some(ADMISSION_RECEIVER)),
            donor,
        ],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    let admission = Variable {
        scope: Scope::Entity,
        id: ADMITTED,
    };
    let send_bonus = |body: &mut Component, world: &mut ChargeHost<'_>| {
        assert_eq!(
            body.queue_message(DONOR_ROOT, DONOR_TRIGGER, Variables::new())
                .unwrap(),
            1
        );
        body.run_now(world).unwrap();
        assert!(
            world.outgoing.is_empty(),
            "the stock self sender stays local"
        );
    };

    assert_eq!(body.resolved_variable(ROOT, admission), Some(Value::Null));
    send_bonus(&mut body, &mut world);
    assert_eq!(meter(&body), 0.0);

    assert_eq!(
        body.queue_message(ADMISSION_ROOT, OPEN_ADMISSION, Variables::new())
            .unwrap(),
        1
    );
    body.run_now(&mut world).unwrap();
    assert_eq!(
        body.resolved_variable(ROOT, admission),
        Some(Value::Bool(true))
    );
    send_bonus(&mut body, &mut world);
    assert_eq!(meter(&body), credit);
    send_bonus(&mut body, &mut world);
    assert_eq!(meter(&body), maximum);

    // Spend some charge so the cap cannot mask a stale admission claim.
    body.set_entity_variable(METER, Value::Float(credit), &mut world)
        .unwrap();
    body.debug_end_state(ADMISSION_ROOT, ADMISSION_CLAIM, &mut world)
        .unwrap();
    assert_eq!(body.resolved_variable(ROOT, admission), Some(Value::Null));
    send_bonus(&mut body, &mut world);
    assert_eq!(meter(&body), credit);
    let diagnostics = &body.snapshot().unwrap().diagnostics;
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
#[ignore = "requires an authorized 104319 graph pack in ORPHEUS_RUNTIME_DIR"]
fn prepared_stock_damage_credit_consumes_typed_pool_records() {
    const DAMAGE: u64 = 0x0240_0000_0000_0012;
    const TARGET: u32 = 703;
    const EXCLUDED_POOL: u64 = 0x0d80_0000_0000_3ebb;
    const OTHER_POOL: u64 = 0x0d80_0000_0000_42c6;
    const CAP: f64 = 10.0;

    struct DamageHost;

    impl World for DamageHost {
        fn position(&mut self, _: u32) -> Option<[f64; 3]> {
            None
        }

        fn entity_variable(&mut self, entity: u32, _: u32) -> Option<Value> {
            (entity == TARGET).then_some(Value::Null)
        }

        fn entity_filter_flags(&self, site: Site, entity: u32) -> Option<u64> {
            // Admit the known target by the receiver's ordinary mask branch.
            (site.entity == OWNER && entity == TARGET).then_some(1)
        }

        fn entities_hostile(
            &self,
            site: Site,
            source: u32,
            target: u32,
            option: bool,
        ) -> Option<bool> {
            (site.entity == OWNER && source == OWNER && target == TARGET && !option).then_some(true)
        }

        fn has_target_tag(&self, site: Site, entity: u32, _: u64) -> Option<bool> {
            // The same target has neither damage-credit scaling tag in any case.
            (site.entity == OWNER && entity == TARGET).then_some(false)
        }
    }

    let runtime =
        PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").expect("set ORPHEUS_RUNTIME_DIR"));
    let graphs = Arc::new(GraphStore::load(runtime.join("data/game_graphs_174.json.gz")).unwrap());
    let pool_record = |pool, drain| {
        Value::Map(vec![
            (Value::Integer(16056), Value::Asset(pool)),
            (Value::Integer(499), Value::Float(drain)),
        ])
    };

    for (case, records, expected) in [
        ("empty", vec![], CAP),
        ("unrelated", vec![pool_record(OTHER_POOL, 7.0)], CAP),
        (
            "wire alias",
            vec![pool_record(EXCLUDED_POOL | (1 << 32), 7.0)],
            CAP,
        ),
        (
            "matching and unrelated",
            vec![
                pool_record(EXCLUDED_POOL, 2.0),
                pool_record(OTHER_POOL, 7.0),
                pool_record(EXCLUDED_POOL, 3.0),
            ],
            7.0,
        ),
        (
            "nonnegative floor",
            vec![pool_record(EXCLUDED_POOL, 13.0)],
            0.0,
        ),
    ] {
        let mut world = DamageHost;
        let mut body = Component::start(
            Arc::clone(&graphs),
            OWNER,
            Variables::from([
                (METER_CAP, Value::Float(CAP)),
                (METER, Value::Float(0.0)),
                (ADMITTED, Value::Bool(true)),
                (INHIBITED, Value::Bool(false)),
            ]),
            vec![Root::listener(ROOT, GRAPH, None)],
            0,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();

        // Supply records directly to the stock consumer. This detached test
        // does not model the original engine's damage event producer.
        let parameters = Variables::from([
            (0x13, Value::Float(12.0)),
            (0x15, Value::Entity(TARGET)),
            (0x3d, Value::Array(Vec::new())),
            (0x624, Value::Array(records)),
        ]);
        assert_eq!(body.queue_message(ROOT, DAMAGE, parameters).unwrap(), 1);
        body.run_now(&mut world).unwrap();
        assert_eq!(meter(&body), expected, "{case}");
        let diagnostics = &body.snapshot().unwrap().diagnostics;
        assert!(diagnostics.is_empty(), "{case}: {diagnostics:?}");
    }
}

#[test]
#[ignore = "requires an authorized 104319 graph pack in ORPHEUS_RUNTIME_DIR"]
fn prepared_stock_0044_charge_applies_receiver_gates_and_scales() {
    const TOPIC: u64 = 0x0240_0000_0000_0044;
    const TARGET: u32 = 704;
    const TARGET_EXCLUSION: u32 = 17625;
    const FIRST_EXCLUSION: u64 = 0x0d80_0000_0000_0292;
    const SECOND_EXCLUSION: u64 = 0x0d80_0000_0000_025a;
    const OTHER_IDENTIFIER: u64 = 0x0d80_0000_0000_090d;
    const CAP: f64 = 10.0;

    struct BonusHost {
        target_excluded: bool,
    }

    impl World for BonusHost {
        fn position(&mut self, _: u32) -> Option<[f64; 3]> {
            None
        }

        fn entity_variable(&mut self, entity: u32, id: u32) -> Option<Value> {
            (entity == TARGET && id == TARGET_EXCLUSION)
                .then_some(Value::Bool(self.target_excluded))
        }
    }

    let runtime =
        PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").expect("set ORPHEUS_RUNTIME_DIR"));
    let graphs = Arc::new(GraphStore::load(runtime.join("data/game_graphs_174.json.gz")).unwrap());
    let mut world = BonusHost {
        target_excluded: false,
    };
    let mut body = Component::start(
        graphs,
        OWNER,
        Variables::from([
            (METER_CAP, Value::Float(CAP)),
            (METER, Value::Float(1.0)),
            (ADMITTED, Value::Bool(true)),
            (INHIBITED, Value::Bool(false)),
            (9532, Value::Float(1.5)),
            (13022, Value::Float(0.5)),
        ]),
        vec![Root::listener(ROOT, GRAPH, None)],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();

    // Inject parameters into the stock listener. The original producer of
    // this topic is outside the probe.
    let deliver =
        |body: &mut Component, world: &mut BonusHost, amount, identifiers: &[u64], expected| {
            let parameters = Variables::from([
                (0x3f, Value::Entity(TARGET)),
                (0x40, Value::Float(amount)),
                (
                    0x41,
                    Value::Array(identifiers.iter().copied().map(Value::Asset).collect()),
                ),
            ]);
            assert_eq!(body.queue_message(ROOT, TOPIC, parameters).unwrap(), 1);
            body.run_now(world).unwrap();
            assert_eq!(meter(body), expected);
            let diagnostics = &body.snapshot().unwrap().diagnostics;
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
        };

    deliver(&mut body, &mut world, 4.0, &[OTHER_IDENTIFIER], 4.0);
    deliver(&mut body, &mut world, 4.0, &[FIRST_EXCLUSION], 4.0);
    deliver(&mut body, &mut world, 4.0, &[SECOND_EXCLUSION], 4.0);

    world.target_excluded = true;
    deliver(&mut body, &mut world, 4.0, &[], 4.0);
    world.target_excluded = false;

    body.set_entity_variable(ADMITTED, Value::Bool(false), &mut world)
        .unwrap();
    deliver(&mut body, &mut world, 4.0, &[], 4.0);

    body.set_entity_variable(ADMITTED, Value::Bool(true), &mut world)
        .unwrap();
    deliver(&mut body, &mut world, 16.0, &[], CAP);
}
