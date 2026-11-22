use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use orpheus_statescript::{
    script_entity_state::EntityScriptState,
    script_expr::{RoomEntityFacts, SceneFilterPolicy},
    script_graph::GraphStore,
    script_init::{
        Component, EmptyWorld, HealthTargetFacts, HitTags, HostEvent, HostHealthContext,
        MAX_INIT_WORK, NamedPoolLoss, RoomBodyIdentity, RoomEntityStatus, RoomMessage, Root, Site,
        TargetTagEffects, World,
    },
    script_wire::{Value, Variables},
};
use serde::Deserialize;
use serde_json::{Value as Json, json};

const ROOT: u16 = 1;
const GRAPH: u32 = 302;

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
    bundle: Json,
    message: String,
    forged_delta: f64,
    source: u32,
    target: u32,
    unrelated: u32,
    events: Vec<Event>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Recipient {
    Source,
    Target,
    Unrelated,
    WithoutListener,
}

#[derive(Deserialize)]
struct Event {
    name: String,
    recipient: Recipient,
    source: Option<u32>,
    target: u32,
    delta: Json,
    killed: bool,
    hit_tags: Option<Vec<u64>>,
    named_pool_losses: Option<Vec<(u64, f64)>>,
    critical_hit: Option<bool>,
}

type Delivery = (
    Option<u32>,
    u32,
    f64,
    bool,
    Option<Vec<u64>>,
    Option<Vec<(u64, f64)>>,
    Option<bool>,
);

#[derive(Deserialize)]
struct Expected {
    ordinary_variable: u32,
    room_variable: u32,
    host_variables: [u32; 8],
    deliveries: Vec<Option<Delivery>>,
    target_tag_query: ExpectedTargetTagQuery,
    settled_damage_query: ExpectedSettledDamageQuery,
    target_generation_relay: ExpectedGenerationRelay,
    source_generation_relay: ExpectedGenerationRelay,
}

#[derive(Deserialize)]
struct ExpectedTargetTagQuery {
    settled_match: bool,
    ordinary_match: bool,
    unrelated_match: bool,
    retired_match: bool,
    retired_alive_match: bool,
    unavailable_fact_diagnostics: usize,
}

type DamageQueryResult = (Option<i64>, bool, bool, bool);

#[derive(Deserialize)]
struct ExpectedSettledDamageQuery {
    sampled: DamageQueryResult,
    ordinary: DamageQueryResult,
    host_event: DamageQueryResult,
    absent: DamageQueryResult,
    unavailable_diagnostics: usize,
}

#[derive(Deserialize)]
struct ExpectedGenerationRelay {
    stable: Vec<f64>,
    replaced_with_ordinary_deliveries: Vec<f64>,
}

fn variables(body: &Component) -> Option<&Variables> {
    Some(&body.snapshot()?.instances.get(&ROOT)?.variables)
}

#[test]
fn host_health_entries_only_receive_settled_events_on_the_next_frame() {
    let fixture: Contract = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/script-host-health-events-v1.json"
    ))
    .unwrap();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.case_id, "script-host-health-events.synthetic.v1");
    assert_eq!(fixture.evidence, "synthetic-rust");

    let input = fixture.input;
    let expected = fixture.expected;
    let graphs =
        Arc::new(GraphStore::from_json(&serde_json::to_vec(&input.bundle).unwrap()).unwrap());
    let start = |entity, subscribed| {
        let roots = if subscribed {
            vec![Root::new(ROOT, GRAPH)]
        } else {
            Vec::new()
        };
        Component::start(
            Arc::clone(&graphs),
            entity,
            Variables::new(),
            roots,
            0,
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap()
    };
    let mut target = start(input.target, true);
    let mut source = start(input.source, true);
    let mut unrelated = start(input.unrelated, true);
    let mut without_listener = start(input.target, false);

    assert!(variables(&target).unwrap().is_empty());
    let message = u64::from_str_radix(input.message.strip_prefix("0x").unwrap(), 16).unwrap();
    let forged = Variables::from([
        (1, Value::Entity(input.source)),
        (2, Value::Entity(input.target)),
        (3, Value::Float(input.forged_delta)),
        (4, Value::Bool(true)),
        (6, Value::Float(999.0)),
        (7, Value::Bool(true)),
    ]);
    assert_eq!(
        target.queue_message(ROOT, message, forged.clone()).unwrap(),
        1
    );
    target.run_now(&mut EmptyWorld).unwrap();
    assert_eq!(
        variables(&target).unwrap(),
        &Variables::from([(expected.ordinary_variable, Value::Float(input.forged_delta))])
    );
    assert_eq!(
        target
            .queue_room_message_from(
                input.source,
                RoomMessage {
                    id: message,
                    parameters: forged,
                },
            )
            .unwrap(),
        1
    );
    target.advance_frame(1, &mut EmptyWorld).unwrap();
    assert_eq!(
        variables(&target).unwrap(),
        &Variables::from([
            (expected.ordinary_variable, Value::Float(input.forged_delta)),
            (expected.room_variable, Value::Float(input.forged_delta)),
        ])
    );

    assert_eq!(input.events.len(), expected.deliveries.len());
    for (index, (event, delivery)) in input
        .events
        .into_iter()
        .zip(expected.deliveries)
        .enumerate()
    {
        let recipient = match event.recipient {
            Recipient::Source => &mut source,
            Recipient::Target => &mut target,
            Recipient::Unrelated => &mut unrelated,
            Recipient::WithoutListener => &mut without_listener,
        };
        let frame = u32::try_from(index).unwrap() * 2 + 2;
        recipient.advance_frame(frame, &mut EmptyWorld).unwrap();
        let before = variables(recipient).cloned();
        let delta = match event.delta.as_str() {
            Some("nan") => f64::NAN,
            Some("infinity") => f64::INFINITY,
            _ => event.delta.as_f64().unwrap(),
        };
        let hit_tags = event.hit_tags.map(|tags| {
            let mut hit_tags = HitTags::EMPTY;
            for tag in tags {
                assert!(hit_tags.push(tag));
            }
            hit_tags
        });
        assert_eq!(
            recipient
                .queue_host_event_with_context(
                    HostEvent::HealthApplied {
                        source: event.source,
                        target: event.target,
                        delta,
                        killed: event.killed,
                        hit_tags,
                    },
                    HostHealthContext {
                        critical_hit: event.critical_hit,
                        named_pool_losses: event.named_pool_losses.map(|rows| {
                            rows.into_iter()
                                .map(|(identifier, amount)| NamedPoolLoss { identifier, amount })
                                .collect()
                        }),
                        ..HostHealthContext::default()
                    },
                )
                .unwrap(),
            usize::from(delivery.is_some()),
            "{}",
            event.name
        );
        recipient.run_now(&mut EmptyWorld).unwrap();
        assert_eq!(variables(recipient), before.as_ref(), "{}", event.name);
        recipient.advance_frame(frame + 1, &mut EmptyWorld).unwrap();

        let mut after = before;
        if let Some((source, target, delta, killed, tags, losses, critical)) = delivery {
            let after = after.as_mut().unwrap();
            for (variable, value) in expected.host_variables.into_iter().zip([
                source.map_or(Value::Null, Value::Entity),
                Value::Entity(target),
                Value::Float(delta),
                Value::Bool(killed),
                Value::Float(delta),
                tags.map_or(Value::Null, |tags| {
                    Value::Array(tags.into_iter().map(Value::Asset).collect())
                }),
                losses.map_or(Value::Null, |losses| {
                    Value::Array(
                        losses
                            .into_iter()
                            .map(|(identifier, amount)| {
                                Value::Map(vec![
                                    (Value::Integer(16056), Value::Asset(identifier)),
                                    (Value::Integer(499), Value::Float(amount)),
                                ])
                            })
                            .collect(),
                    )
                }),
                critical.map_or(Value::Null, Value::Bool),
            ]) {
                after.insert(variable, value);
            }
        }
        assert_eq!(variables(recipient), after.as_ref(), "{}", event.name);
    }
}

#[test]
fn settled_target_facts_are_scoped_to_immediate_host_delivery() {
    const SOURCE: u32 = 731;
    const TARGET: u32 = 732;
    const OTHER: u32 = 733;
    const MESSAGE: u64 = 197;
    const TAG: u64 = 0x0d60_0000_0000_5521;

    let fixture: Contract = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/script-host-health-events-v1.json"
    ))
    .unwrap();
    let expected = fixture.expected.target_tag_query;
    struct LiveWorld(BTreeMap<u32, RoomEntityFacts>);
    impl World for LiveWorld {
        fn position(&mut self, _: u32) -> Option<[f64; 3]> {
            None
        }

        fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
            Some(Value::Integer(11))
        }

        fn has_target_tag(&self, _: Site, entity: u32, _: u64) -> Option<bool> {
            Some(entity == OTHER)
        }

        fn entity_filter_flags(&self, _: Site, _: u32) -> Option<u64> {
            Some(2)
        }

        fn room_entities(&self) -> Option<&BTreeMap<u32, RoomEntityFacts>> {
            Some(&self.0)
        }
    }

    let var = |id| json!({"$":"STUConfigVarDynamic","m_identifier":id,"m_60DB8F99":false});
    let tag =
        |entity| json!({"$":"STU_25ECB55F","m_entity":entity,"m_88B0050A":format!("0x{TAG:016X}")});
    let matches_opponent_tag = |entity, alive: Option<bool>| {
        let mut filter = json!({
            "relation":"opponent", "source":{"$":"STU_B5A0CAF0"},
            "tags":{"all":[format!("0x{TAG:016X}")]}
        });
        if let Some(alive) = alive {
            filter["alive"] = json!(alive);
        }
        json!({"$":"EntityMatches", "entity":entity, "filter":filter})
    };
    let outputs = [
        json!({"$":"STU_919FD47C","m_configVars":[{"$":"STUConfigVarInt","m_value":7}],
            "m_expression":{"m_opcodes":[8,0,14,0,15,0],"m_dynamicVars":[var(1)]}}),
        tag(var(1)),
        json!({"$":"STU_AC829876","m_entity":var(1),
            "m_filter":{"$":"STUConfigVarFilter","m_84065FD1":1}}),
        tag(var(2)),
        matches_opponent_tag(var(1), None),
        matches_opponent_tag(var(2), Some(true)),
        matches_opponent_tag(var(1), Some(true)),
    ];
    let parameter = json!({"m_B5051BCE":"0x0000000000000002","m_out_Var":var(1)});
    let links: Vec<_> = (2..2 + outputs.len())
        .map(|pos| (pos, "m_inPlug"))
        .collect();
    let links = json!({"$":"Plug","links":links});
    let mut nodes = vec![
        json!({"pos":0,"rid":0,"uid":0,"class":"EntryHostEvent","state":null,
            "remote":-1,"client_only":false,"server_only":true,"host_event_entry":"health_applied",
            "fields":{"m_params":[parameter.clone()],"m_outPlug":links.clone()}}),
        json!({"pos":1,"rid":1,"uid":1,"class":"STUStatescriptEntryGameMessage","state":null,
            "remote":-1,"client_only":false,"server_only":true,"room_message_entry":true,
            "fields":{"m_gameMessage":{"m_gameMessage":format!("0x{MESSAGE:016X}")},
                "m_params":[parameter],"m_outPlug":links}}),
    ];
    nodes.extend(outputs.into_iter().enumerate().map(|(i, value)| {
        let pos = i + 2;
        json!({"pos":pos,"rid":pos,"uid":pos,"class":"STUStatescriptActionSetVar","state":null,
            "remote":-1,"client_only":false,"server_only":true,
            "fields":{"m_ADEB6E05":true,"m_out_Var":var(i+10),"m_value":value}})
    }));
    let bundle = json!({"version":1,"bodies":{},"missing":[],"graphs":[{
        "index":GRAPH,"items":nodes.len(),"nodes":nodes,"fields":{},"containers":{},"entries":[1],
        "states":[],"remote":[],"sync_vars":[],"unknown_nodes":[],
        "header":{"m_nodesBitCount":4,"m_statesBitCount":3,"m_remoteSyncNodesBitCount":3,"m_syncVarsBitCount":3}
    }]});
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap());
    let mut root = Root::new(ROOT, GRAPH);
    root.variables.insert(2, Value::Entity(OTHER));
    let live_facts = |team, tags: &BTreeSet<u64>| RoomEntityFacts {
        team: Some(team),
        alive: true,
        tags: TargetTagEffects::default().snapshot(Some(tags)),
    };
    let without_tag = BTreeSet::new();
    let with_tag = BTreeSet::from([TAG]);
    let mut world = LiveWorld(BTreeMap::from([
        (SOURCE, live_facts(0, &without_tag)),
        (TARGET, live_facts(1, &without_tag)),
        (OTHER, live_facts(1, &with_tag)),
    ]));
    let mut body = Component::start(
        graphs,
        SOURCE,
        Variables::new(),
        vec![root],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    let reads = |body: &Component| {
        [10, 11, 12, 13, 14, 15, 16].map(|id| variables(body).unwrap()[&id].clone())
    };
    let event = HostEvent::HealthApplied {
        source: Some(SOURCE),
        target: TARGET,
        delta: -2.5,
        killed: false,
        hit_tags: None,
    };
    let sampled = HealthTargetFacts {
        target: TARGET,
        room_status: None,
        filter_flags: Some(1),
        tags: TargetTagEffects::default().snapshot(Some(&BTreeSet::from([TAG]))),
        script_state: EntityScriptState::Running(Arc::new(Variables::from([(
            7,
            Value::Integer(9),
        )]))),
    };
    let wrong_target = HealthTargetFacts {
        target: OTHER,
        ..sampled.clone()
    };
    assert_eq!(
        body.queue_host_event_with_target_facts(event, Some(Arc::new(wrong_target)))
            .unwrap(),
        0
    );
    assert_eq!(
        body.queue_host_event_with_target_facts(event, Some(Arc::new(sampled.clone())))
            .unwrap(),
        1
    );
    body.advance_frame(1, &mut world).unwrap();
    assert_eq!(
        reads(&body),
        [
            Value::Integer(9),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(expected.settled_match),
            Value::Bool(expected.unrelated_match),
            Value::Bool(expected.settled_match),
        ]
    );

    let plain_event_values = [
        Value::Integer(11),
        Value::Bool(false),
        Value::Bool(false),
        Value::Bool(true),
        Value::Bool(expected.ordinary_match),
        Value::Bool(expected.unrelated_match),
        Value::Bool(expected.ordinary_match),
    ];
    body.queue_host_event(event).unwrap();
    body.advance_frame(2, &mut world).unwrap();
    assert_eq!(reads(&body), plain_event_values);
    assert!(body.snapshot().unwrap().diagnostics.is_empty());

    let unavailable = HealthTargetFacts {
        target: TARGET,
        room_status: None,
        filter_flags: None,
        tags: Default::default(),
        script_state: EntityScriptState::Unavailable,
    };
    body.queue_host_event_with_target_facts(event, Some(Arc::new(unavailable)))
        .unwrap();
    let room_message = RoomMessage {
        id: MESSAGE,
        parameters: Variables::from([(2, Value::Entity(TARGET))]),
    };
    assert_eq!(
        body.queue_room_message_from(OTHER, room_message).unwrap(),
        1
    );
    body.advance_frame(3, &mut world).unwrap();
    assert_eq!(reads(&body), plain_event_values);
    assert_eq!(
        body.snapshot().unwrap().diagnostics.len(),
        expected.unavailable_fact_diagnostics
    );

    world.0.remove(&TARGET).unwrap();
    let retired = HealthTargetFacts {
        room_status: Some(RoomEntityStatus {
            team: Some(1),
            alive: false,
        }),
        ..sampled
    };
    let killing_blow = HostEvent::HealthApplied {
        source: Some(SOURCE),
        target: TARGET,
        delta: -2.5,
        killed: true,
        hit_tags: None,
    };
    body.queue_host_event_with_target_facts(killing_blow, Some(Arc::new(retired)))
        .unwrap();
    body.advance_frame(4, &mut world).unwrap();
    let tags_and_liveness = &reads(&body)[4..];
    assert_eq!(
        tags_and_liveness,
        [
            Value::Bool(expected.retired_match),
            Value::Bool(expected.unrelated_match),
            Value::Bool(expected.retired_alive_match),
        ]
    );
    assert_eq!(
        body.snapshot().unwrap().diagnostics.len(),
        expected.unavailable_fact_diagnostics
    );
}

#[test]
fn settled_damage_facts_are_scoped_to_the_targeted_ordinary_message() {
    const SOURCE: u32 = 739;
    const TARGET: u32 = 740;
    const OTHER: u32 = 741;
    const OTHER_ROOT: u16 = 2;
    const MESSAGE: u64 = 0x0240_0000_0055_0022;
    const TAG: u64 = 0x0d60_0000_0055_0023;
    const TARGET_SLOT: u32 = 2;
    const SENDER_SLOT: u32 = 4;
    const SENDER_MARKER: i64 = 47;

    #[derive(Default)]
    struct LiveWorld {
        settled_filters: Cell<usize>,
        settled_relations: Cell<usize>,
    }

    impl World for LiveWorld {
        fn position(&mut self, _: u32) -> Option<[f64; 3]> {
            None
        }

        fn entity_variable(&mut self, entity: u32, _: u32) -> Option<Value> {
            (entity == TARGET).then_some(Value::Integer(11))
        }

        fn has_target_tag(&self, _: Site, entity: u32, tag: u64) -> Option<bool> {
            (entity == TARGET && tag == TAG).then_some(true)
        }

        fn entity_filter_flags(&self, _: Site, entity: u32) -> Option<u64> {
            (entity == TARGET).then_some(2)
        }

        fn settled_scene_filter_policy(
            &self,
            site: Site,
            filter: &Json,
            target: &HealthTargetFacts,
        ) -> Option<SceneFilterPolicy> {
            assert_eq!(
                (site.entity, site.instance, target.target),
                (SOURCE, ROOT, TARGET)
            );
            assert_eq!(filter["m_84065FD1"], 1);
            self.settled_filters.set(self.settled_filters.get() + 1);
            None
        }

        fn entities_hostile(
            &self,
            _: Site,
            source: u32,
            target: u32,
            option: bool,
        ) -> Option<bool> {
            assert_eq!((source, target, option), (SOURCE, TARGET, false));
            Some(false)
        }

        fn settled_entities_hostile(
            &self,
            site: Site,
            source: u32,
            target: &HealthTargetFacts,
            option: bool,
        ) -> Option<bool> {
            assert_eq!(
                (site.entity, site.instance, source, target.target, option),
                (SOURCE, ROOT, SOURCE, TARGET, false)
            );
            self.settled_relations.set(self.settled_relations.get() + 1);
            target.filter_flags.map(|flags| flags == 1)
        }
    }

    let contract: Contract = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/script-host-health-events-v1.json"
    ))
    .unwrap();
    let expected = contract.expected.settled_damage_query;
    let var = |id| json!({"$":"STUConfigVarDynamic","m_identifier":id,"m_60DB8F99":false});
    let outputs = [
        json!({"$":"STU_919FD47C","m_configVars":[{"$":"STUConfigVarInt","m_value":7}],
            "m_expression":{"m_opcodes":[8,0,14,0,15,0],"m_dynamicVars":[var(TARGET_SLOT)]}}),
        json!({"$":"STU_25ECB55F","m_entity":var(TARGET_SLOT),"m_88B0050A":format!("0x{TAG:016X}")}),
        json!({"$":"STU_AC829876","m_entity":var(TARGET_SLOT),
            "m_filter":{"$":"STUConfigVarFilter","m_84065FD1":1}}),
        json!({"$":"STU_A58C1E63","m_source":{"$":"STU_B5A0CAF0"},"m_target":var(TARGET_SLOT),
            "m_AFBBE9C3":{"$":"STUConfigVarBool","m_value":0}}),
    ];
    let links: Vec<_> = (2..2 + outputs.len())
        .map(|pos| (pos, "m_inPlug"))
        .collect();
    let binding =
        json!({"m_B5051BCE":format!("0x{TARGET_SLOT:016X}"), "m_out_Var":var(TARGET_SLOT)});
    let mut nodes = vec![
        json!({
            "pos":0,"rid":0,"uid":0,"class":"STUStatescriptEntryGameMessage","state":null,
            "remote":-1,"client_only":false,"server_only":true,
            "fields":{
                "m_gameMessage":{"$":"STUConfigVarGameMessage","m_gameMessage":format!("0x{MESSAGE:016X}")},
                "m_params":[binding],"m_FF703F20":var(SENDER_SLOT),
                "m_outPlug":{"$":"STUStatescriptOutputPlug","links":links}
            }
        }),
        json!({
            "pos":1,"rid":1,"uid":1,"class":"EntryHostEvent","state":null,
            "remote":-1,"client_only":false,"server_only":true,"host_event_entry":"health_applied",
            "fields":{"m_params":[binding],"m_outPlug":{"$":"STUStatescriptOutputPlug","links":links}}
        }),
    ];
    nodes.extend(outputs.into_iter().enumerate().map(|(i, value)| {
        let pos = i + 2;
        json!({"pos":pos,"rid":pos,"uid":pos,"class":"STUStatescriptActionSetVar","state":null,
            "remote":-1,"client_only":false,"server_only":true,
            "fields":{"m_ADEB6E05":true,"m_out_Var":var((i + 10) as u32),"m_value":value}})
    }));
    let bundle = json!({"version":1,"bodies":{},"missing":[],"graphs":[{
        "index":GRAPH,"items":nodes.len(),"nodes":nodes,"fields":{},"containers":{},"entries":[0],
        "states":[],"remote":[],"sync_vars":[],"unknown_nodes":[],
        "header":{"m_nodesBitCount":3,"m_statesBitCount":3,"m_remoteSyncNodesBitCount":3,"m_syncVarsBitCount":3}
    }]});
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap());
    let mut world = LiveWorld::default();
    let roots = [ROOT, OTHER_ROOT].map(|id| {
        let mut root = Root::new(id, GRAPH);
        root.variables
            .insert(SENDER_SLOT, Value::Integer(SENDER_MARKER));
        root
    });
    let mut body = Component::start(
        graphs,
        SOURCE,
        Variables::new(),
        roots.into(),
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    let reads = |body: &Component, instance| {
        let variables = &body.snapshot().unwrap().instances[&instance].variables;
        assert_eq!(variables[&SENDER_SLOT], Value::Integer(SENDER_MARKER));
        [10, 11, 12, 13].map(|id| variables.get(&id).cloned().unwrap_or(Value::Null))
    };
    let expected_reads = |(member, tag, mask, hostile): DamageQueryResult| {
        [
            member.map_or(Value::Null, Value::Integer),
            Value::Bool(tag),
            Value::Bool(mask),
            Value::Bool(hostile),
        ]
    };
    let settled_calls =
        |world: &LiveWorld| (world.settled_filters.get(), world.settled_relations.get());
    let sampled = HealthTargetFacts {
        target: TARGET,
        room_status: None,
        filter_flags: Some(1),
        tags: TargetTagEffects::default().snapshot(Some(&BTreeSet::new())),
        script_state: EntityScriptState::Running(Arc::new(Variables::from([(
            7,
            Value::Integer(9),
        )]))),
    };
    let parameters = |target| Variables::from([(TARGET_SLOT, target)]);
    for invalid in [
        Value::Entity(OTHER),
        Value::Integer(i64::from(TARGET)),
        Value::Array(vec![Value::Entity(TARGET)]),
    ] {
        assert_eq!(
            body.queue_settled_damage_message(
                ROOT,
                MESSAGE,
                parameters(invalid),
                Arc::new(sampled.clone())
            )
            .unwrap(),
            0
        );
    }
    let bound = parameters(Value::Entity(TARGET));
    assert_eq!(
        body.queue_settled_damage_message(ROOT, MESSAGE, bound.clone(), Arc::new(sampled.clone()))
            .unwrap(),
        1
    );
    assert_eq!(
        body.queue_message(OTHER_ROOT, MESSAGE, bound.clone())
            .unwrap(),
        1
    );
    body.run_now(&mut world).unwrap();
    assert_eq!(reads(&body, ROOT), expected_reads(expected.sampled));
    assert_eq!(reads(&body, OTHER_ROOT), expected_reads(expected.ordinary));

    assert_eq!(body.queue_message(ROOT, MESSAGE, bound.clone()).unwrap(), 1);
    body.run_now(&mut world).unwrap();
    assert_eq!(reads(&body, ROOT), expected_reads(expected.ordinary));

    let host_event = HostEvent::HealthApplied {
        source: Some(SOURCE),
        target: TARGET,
        delta: -2.5,
        killed: false,
        hit_tags: None,
    };
    assert_eq!(
        body.queue_host_event_with_target_facts(host_event, Some(Arc::new(sampled.clone())))
            .unwrap(),
        2
    );
    body.advance_frame(1, &mut world).unwrap();
    assert_eq!(reads(&body, ROOT), expected_reads(expected.host_event));
    assert_eq!(
        reads(&body, OTHER_ROOT),
        expected_reads(expected.host_event)
    );
    assert_eq!(settled_calls(&world), (1, 1));

    let absent = HealthTargetFacts {
        script_state: EntityScriptState::Absent,
        ..sampled
    };
    assert_eq!(
        body.queue_settled_damage_message(ROOT, MESSAGE, bound.clone(), Arc::new(absent))
            .unwrap(),
        1
    );
    body.run_now(&mut world).unwrap();
    assert_eq!(reads(&body, ROOT), expected_reads(expected.absent));
    assert!(body.snapshot().unwrap().diagnostics.is_empty());

    let unavailable = HealthTargetFacts {
        target: TARGET,
        room_status: None,
        filter_flags: None,
        tags: Default::default(),
        script_state: EntityScriptState::Unavailable,
    };
    assert_eq!(
        body.queue_settled_damage_message(ROOT, MESSAGE, bound, Arc::new(unavailable))
            .unwrap(),
        1
    );
    body.run_now(&mut world).unwrap();
    assert_eq!(reads(&body, ROOT), expected_reads(expected.absent));
    assert_eq!(
        body.snapshot().unwrap().diagnostics.len(),
        expected.unavailable_diagnostics
    );
    assert_eq!(settled_calls(&world), (3, 3));
}

#[test]
fn health_delivery_checks_original_target_generation_for_direct_and_relayed_requests() {
    check_health_delivery_generation(HealthEndpoint::Target);
}

#[test]
fn health_delivery_checks_original_source_generation_for_direct_and_relayed_requests() {
    check_health_delivery_generation(HealthEndpoint::Source);
}

#[derive(Clone, Copy)]
enum HealthEndpoint {
    Source,
    Target,
}

fn check_health_delivery_generation(endpoint: HealthEndpoint) {
    const SOURCE: u32 = 741;
    const TARGET: u32 = 742;
    const OTHER: u32 = 743;
    const INCOMING: u64 = 198;
    const OUTGOING: u64 = 199;
    const TYPED_RELAY: u64 = 200;
    const NUMERIC_RELAY: u64 = 201;
    const GENERATION: u64 = 7;

    let fixture: Contract = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/script-host-health-events-v1.json"
    ))
    .unwrap();
    let (listener, subject, slot, source_var, target_var, expected) = match endpoint {
        HealthEndpoint::Source => (
            TARGET,
            SOURCE,
            1,
            1,
            3,
            fixture.expected.source_generation_relay,
        ),
        HealthEndpoint::Target => (
            SOURCE,
            TARGET,
            2,
            3,
            1,
            fixture.expected.target_generation_relay,
        ),
    };
    let subject_parameter = format!("0x{slot:016X}");

    struct Room {
        generations: BTreeMap<u32, u64>,
        messages: Vec<(u32, RoomMessage)>,
        health: Vec<(u32, f64)>,
    }

    impl World for Room {
        fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
            None
        }

        fn position(&mut self, _: u32) -> Option<[f64; 3]> {
            None
        }

        fn room_body_generation(&self, wire: u32) -> Option<u64> {
            self.generations.get(&wire).copied()
        }

        fn queue_room_message(&mut self, _: Site, target: u32, message: RoomMessage) -> bool {
            self.messages.push((target, message));
            true
        }

        fn queue_health_change(&mut self, _: Site, target: u32, delta: f64) -> bool {
            self.health.push((target, delta));
            true
        }
    }

    let var = |id| json!({"$":"STUConfigVarDynamic","m_identifier":id,"m_60DB8F99":false});
    let parameters = json!([
        {"m_B5051BCE":subject_parameter,"m_out_Var":var(1)},
        {"m_B5051BCE":"0x0000000000000003","m_out_Var":var(2)}
    ]);
    let host_parameters = json!([
        {"m_B5051BCE":"0x0000000000000001","m_out_Var":var(source_var)},
        {"m_B5051BCE":"0x0000000000000002","m_out_Var":var(target_var)},
        {"m_B5051BCE":"0x0000000000000003","m_out_Var":var(2)}
    ]);
    let requests = json!({"$":"Plug","links":[[2,"m_inPlug"],[3,"m_inPlug"]]});
    let host_requests = json!({"$":"Plug",
        "links":[[8,"m_inPlug"],[2,"m_inPlug"],[3,"m_inPlug"],[4,"m_inPlug"],[5,"m_inPlug"]]});
    let empty = json!({"$":"Plug","links":[]});
    let relay = |pos, topic, parameters: Json| {
        json!({"pos":pos,"rid":pos,"uid":pos,"class":"STUStatescriptActionSendGameMessage",
            "state":null,"remote":-1,"client_only":false,"server_only":true,
            "fields":{"m_ADEB6E05":1,"m_gameMessage":{"m_gameMessage":format!("0x{topic:016X}")},
                "m_params":parameters,"m_outPlug":empty}})
    };
    let relay_entry = |pos, topic, parameters: Json| {
        json!({"pos":pos,"rid":pos,"uid":pos,"class":"STUStatescriptEntryGameMessage",
            "state":null,"remote":-1,"client_only":false,"server_only":true,
            "fields":{"m_gameMessage":{"m_gameMessage":format!("0x{topic:016X}")},
                "m_params":parameters,"m_outPlug":requests}})
    };
    let outgoing_subject = json!({"m_B5051BCE":subject_parameter,"m_value":var(1)});
    let outgoing_delta = json!({"m_B5051BCE":"0x0000000000000003","m_value":var(2)});
    let nodes = json!([
        {"pos":0,"rid":0,"uid":0,"class":"EntryHostEvent","state":null,"remote":-1,
            "client_only":false,"server_only":true,"host_event_entry":"health_applied",
            "fields":{"m_params":host_parameters,"m_outPlug":host_requests}},
        {"pos":1,"rid":1,"uid":1,"class":"STUStatescriptEntryGameMessage","state":null,"remote":-1,
            "client_only":false,"server_only":true,"room_message_entry":true,
            "fields":{"m_gameMessage":{"m_gameMessage":format!("0x{INCOMING:016X}")},
                "m_params":parameters,"m_outPlug":requests}},
        {"pos":2,"rid":2,"uid":2,"class":"authored.send","state":null,"remote":-1,
            "client_only":false,"server_only":true,"action_behavior":"room_message",
            "fields":{"m_gameMessage":{"m_gameMessage":format!("0x{OUTGOING:016X}")},
                "m_target":var(1),"m_params":[{"m_B5051BCE":"0x0000000000000003","m_value":var(2)}],
                "m_outPlug":empty}},
        {"pos":3,"rid":3,"uid":3,"class":"authored.health","state":null,"remote":-1,
            "client_only":false,"server_only":true,"action_behavior":"health_change",
            "fields":{"target":var(1),"delta":var(2),"m_outPlug":empty}},
        relay(4, TYPED_RELAY, json!([outgoing_subject, outgoing_delta])),
        relay(5, NUMERIC_RELAY, json!([
            {"m_B5051BCE":subject_parameter,
                "m_value":{"$":"STUConfigVarInt","m_value":subject}},
            {"m_B5051BCE":"0x0000000000000004","m_value":{"$":"STU_B5A0CAF0"}},
            {"m_B5051BCE":"0x0000000000000003",
                "m_value":{"$":"STUConfigVarFloat","m_value":-1.75}}
        ])),
        relay_entry(6, TYPED_RELAY, parameters.clone()),
        // The other endpoint's typed entity cannot carry provenance for the
        // subject, which is only a number in this relay's parameters.
        relay_entry(7, NUMERIC_RELAY, json!([
            {"m_B5051BCE":"0x0000000000000003","m_out_Var":var(2)}
        ])),
        {"pos":8,"rid":8,"uid":8,"class":"STUStatescriptActionSetVar","state":null,
            "remote":-1,"client_only":false,"server_only":true,
            "fields":{"m_ADEB6E05":true,"m_out_Var":var(4),"m_value":var(source_var)}}
    ]);
    let bundle = json!({"version":1,"bodies":{},"missing":[],"graphs":[{
        "index":GRAPH,"items":9,"nodes":nodes,"fields":{},"containers":{},"entries":[1,6,7],
        "states":[],"remote":[],"sync_vars":[],"unknown_nodes":[],
        "header":{"m_nodesBitCount":4,"m_statesBitCount":3,"m_remoteSyncNodesBitCount":3,"m_syncVarsBitCount":3}
    }]});
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap());
    let mut world = Room {
        generations: BTreeMap::from([(SOURCE, GENERATION), (TARGET, GENERATION)]),
        messages: Vec::new(),
        health: Vec::new(),
    };
    let mut body = Component::start(
        graphs,
        listener,
        Variables::new(),
        vec![Root::new(ROOT, GRAPH)],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    let event = HostEvent::HealthApplied {
        source: Some(SOURCE),
        target: TARGET,
        delta: -2.5,
        killed: false,
        hit_tags: None,
    };
    let context = |generation| HostHealthContext {
        named_pool_losses: None,
        critical_hit: None,
        source_body: Some(RoomBodyIdentity {
            wire: SOURCE,
            generation: if subject == SOURCE {
                generation
            } else {
                GENERATION
            },
        }),
        target_body: Some(RoomBodyIdentity {
            wire: TARGET,
            generation: if subject == TARGET {
                generation
            } else {
                GENERATION
            },
        }),
        target_facts: None,
    };
    let message = |id, amount| RoomMessage {
        id,
        parameters: Variables::from([(3, Value::Float(amount))]),
    };
    let assert_deliveries = |world: &mut Room, amounts: &[f64]| {
        let messages: Vec<_> = amounts
            .iter()
            .map(|&amount| (subject, message(OUTGOING, amount)))
            .collect();
        let health: Vec<_> = amounts.iter().map(|&amount| (subject, amount)).collect();
        assert_eq!(std::mem::take(&mut world.messages), messages);
        assert_eq!(std::mem::take(&mut world.health), health);
    };

    assert_eq!(
        body.queue_host_event_with_context(event, context(GENERATION))
            .unwrap(),
        1
    );
    body.advance_frame(1, &mut world).unwrap();
    assert_deliveries(&mut world, &expected.stable);

    assert_eq!(
        body.queue_host_event_with_context(event, context(GENERATION))
            .unwrap(),
        1
    );
    let fresh_generation = GENERATION + 1;
    world.generations.insert(subject, fresh_generation);
    let mut ordinary = message(TYPED_RELAY, 0.75);
    ordinary.parameters.insert(slot, Value::Entity(subject));
    assert_eq!(
        body.queue_entity_frame_message(2, TYPED_RELAY, ordinary.parameters)
            .unwrap(),
        1
    );
    let mut external = message(INCOMING, 1.25);
    external.parameters.insert(slot, Value::Entity(subject));
    assert_eq!(body.queue_room_message_from(OTHER, external).unwrap(), 1);
    body.advance_frame(2, &mut world).unwrap();

    assert_deliveries(&mut world, &expected.replaced_with_ordinary_deliveries);
    assert_eq!(
        variables(&body).unwrap().get(&4),
        Some(&Value::Entity(SOURCE))
    );

    assert_eq!(
        body.queue_host_event_with_context(event, context(fresh_generation))
            .unwrap(),
        1
    );
    body.advance_frame(3, &mut world).unwrap();
    assert_deliveries(&mut world, &expected.stable);
    assert!(body.snapshot().unwrap().diagnostics.is_empty());

    let wrong_target = HostHealthContext {
        target_body: Some(RoomBodyIdentity {
            wire: OTHER,
            generation: GENERATION,
        }),
        ..context(fresh_generation)
    };
    assert_eq!(
        body.queue_host_event_with_context(event, wrong_target)
            .unwrap(),
        0
    );
    if matches!(endpoint, HealthEndpoint::Source) {
        let wrong_source = HostHealthContext {
            source_body: Some(RoomBodyIdentity {
                wire: OTHER,
                generation: GENERATION,
            }),
            ..context(fresh_generation)
        };
        assert_eq!(
            body.queue_host_event_with_context(event, wrong_source)
                .unwrap(),
            0
        );
        let unattributed = HostEvent::HealthApplied {
            source: None,
            target: TARGET,
            delta: 2.5,
            killed: false,
            hit_tags: None,
        };
        assert_eq!(
            body.queue_host_event_with_context(unattributed, context(fresh_generation))
                .unwrap(),
            0
        );
    }
}
