use serde::Deserialize;

use super::*;
use crate::script_expr::{RoomEntityFacts, SpatialCandidate};

mod timed_eliminations;

const GRAPH: u32 = 211;
const INSTANCE: u16 = 1;
const TRIGGER: u64 = 61;
const CONTINUED: u32 = 3;

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
    speaker: u32,
    center: [f64; 3],
    actors: Vec<(u32, [f64; 3], Option<u8>, bool)>,
    scenarios: Vec<Scenario>,
}

#[derive(Deserialize)]
struct Scenario {
    name: String,
    #[serde(default)]
    facts_available: bool,
    audience: Option<Vec<Json>>,
    audience_count: Option<u32>,
    #[serde(default)]
    mode_speaker: bool,
    #[serde(default)]
    reject: bool,
}

#[derive(Deserialize)]
struct Expected {
    stimulus: String,
    line: String,
    outputs: Vec<Output>,
}

#[derive(Deserialize)]
struct Output {
    attempted: Option<Vec<u32>>,
    queued: Option<Vec<u32>>,
    continued: bool,
}

type Submission = (Site, Vec<u32>, RoomCue);

struct CueWorld {
    speaker: u32,
    center: [f64; 3],
    candidates: Vec<SpatialCandidate>,
    facts: Option<BTreeMap<u32, RoomEntityFacts>>,
    reject: bool,
    attempts: Vec<Submission>,
    queued: Vec<Submission>,
}

impl World for CueWorld {
    fn position(&mut self, entity: u32) -> Option<[f64; 3]> {
        (entity == self.speaker).then_some(self.center)
    }

    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }

    fn spatial_candidates(&self) -> Option<&[SpatialCandidate]> {
        Some(&self.candidates)
    }

    fn room_entities(&self) -> Option<&BTreeMap<u32, RoomEntityFacts>> {
        self.facts.as_ref()
    }

    fn queue_room_cue(&mut self, site: Site, targets: &[u32], cue: RoomCue) -> bool {
        let submission = (site, targets.to_vec(), cue);
        if !self.reject {
            self.queued.push(submission.clone());
        }
        self.attempts.push(submission);
        !self.reject
    }
}

#[test]
fn authored_room_cue_stages_only_a_valid_scoped_audience() {
    let contract: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-room-cue-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.case_id, "script-room-cue.synthetic.v1");
    assert_eq!(contract.evidence, "synthetic-rust");

    let Input {
        bundle,
        speaker,
        center,
        actors,
        scenarios,
    } = contract.input;
    let Expected {
        stimulus,
        line,
        outputs,
    } = contract.expected;
    assert_eq!(scenarios.len(), outputs.len());
    let stimulus = u64::from_str_radix(stimulus.strip_prefix("0x").unwrap(), 16).unwrap();
    let line = u64::from_str_radix(line.strip_prefix("0x").unwrap(), 16).unwrap();
    let site = Site {
        entity: speaker,
        instance: INSTANCE,
        graph: GRAPH,
        node: Some(1),
    };

    let mut inline = bundle.clone();
    inline.as_object_mut().unwrap().remove("authored_classes");
    let cue_node = &mut inline["graphs"][0]["nodes"][1];
    cue_node["class"] = json!("authored.direct_cue");
    cue_node["server_only"] = json!(true);
    cue_node["action_behavior"] = json!("room_cue");
    cue_node["fields"]["audience"] = var(2, true);
    let load = |source| {
        let store = GraphStore::from_json(&serde_json::to_vec(&source).unwrap()).unwrap();
        let node = store.graph(GRAPH).unwrap().node(1).unwrap();
        assert!(node.server_only && !node.is_networked());
        Arc::new(store)
    };
    let radius_graph = load(bundle);
    let inline_graph = load(inline.clone());
    inline["graphs"][0]["nodes"][1]["fields"]["speaker"] = json!("mode");
    let mode_graph = load(inline);

    for (scenario, expected) in scenarios.into_iter().zip(outputs) {
        let mut world = CueWorld {
            speaker,
            center,
            candidates: actors
                .iter()
                .map(|&(entity, position, team, _)| SpatialCandidate {
                    entity,
                    position,
                    team,
                })
                .collect(),
            facts: scenario.facts_available.then(|| {
                actors
                    .iter()
                    .map(|&(entity, _, team, alive)| {
                        (
                            entity,
                            RoomEntityFacts {
                                team,
                                alive,
                                tags: TargetTagSnapshot::default(),
                            },
                        )
                    })
                    .collect()
            }),
            reject: scenario.reject,
            attempts: Vec::new(),
            queued: Vec::new(),
        };
        let audience = scenario
            .audience
            .map(|audience| {
                audience
                    .into_iter()
                    .map(|member| match member.as_u64() {
                        Some(id) => Value::Entity(u32::try_from(id).unwrap()),
                        None => Value::Null,
                    })
                    .collect()
            })
            .or_else(|| {
                scenario
                    .audience_count
                    .map(|count| (1_000..1_000 + count).map(Value::Entity).collect())
            });
        let mut variables =
            Variables::from([(1, Value::Entity(speaker)), (CONTINUED, Value::Bool(false))]);
        let graphs = if let Some(audience) = audience {
            variables.insert(2, Value::Array(audience));
            if scenario.mode_speaker {
                &mode_graph
            } else {
                &inline_graph
            }
        } else {
            &radius_graph
        };
        let mut component = Component::start(
            Arc::clone(graphs),
            speaker,
            variables,
            vec![Root::listener(INSTANCE, GRAPH, None)],
            0,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        assert!(world.attempts.is_empty());
        assert_eq!(
            component
                .queue_message(INSTANCE, TRIGGER, Variables::new())
                .unwrap(),
            1
        );
        component.run_now(&mut world).unwrap();

        let submission = |audience| {
            (
                site,
                audience,
                RoomCue {
                    stimulus,
                    line,
                    speaker: if scenario.mode_speaker {
                        RoomCueSpeaker::Mode
                    } else {
                        RoomCueSpeaker::Actor
                    },
                },
            )
        };
        assert_eq!(
            world.attempts.as_slice(),
            expected.attempted.map(submission).as_slice(),
            "{}",
            scenario.name
        );
        assert_eq!(
            world.queued.as_slice(),
            expected.queued.map(submission).as_slice(),
            "{}",
            scenario.name
        );
        assert_eq!(
            component.snapshot().unwrap().variables[&CONTINUED],
            Value::Bool(expected.continued),
            "{}",
            scenario.name
        );
    }
}

#[test]
fn authored_cue_requires_a_settled_critical_impact() {
    let speaker = 701;
    let target = 702;
    let mut entry = node(
        0,
        "EntryHostEvent",
        None,
        json!({
            "m_params": [{"m_B5051BCE": "0x0000000000000007", "m_out_Var": var(2, true)}],
            "m_outPlug": plug(&[(1, "m_inPlug")], &[]),
        }),
    );
    entry["server_only"] = json!(true);
    entry["host_event_entry"] = json!("health_applied");

    let mut branch = node(
        1,
        "SyntheticBranchAction",
        None,
        json!({
            "m_condition": var(2, true),
            "m_truePlug": plug(&[(2, "m_inPlug")], &[]),
            "m_falsePlug": plug(&[], &[]),
        }),
    );
    branch["server_only"] = json!(true);
    branch["action_behavior"] = json!("branch");

    let mut cue = node(
        2,
        "SyntheticCueAction",
        None,
        json!({
            "audience": var(1, true),
            "stimulus": "0x0000000000000031",
            "line": "0x0000000000000032",
            "speaker": "mode",
            "m_outPlug": plug(&[], &[]),
        }),
    );
    cue["server_only"] = json!(true);
    cue["action_behavior"] = json!("room_cue");

    let mut world = CueWorld {
        speaker,
        center: [0.0; 3],
        candidates: Vec::new(),
        facts: None,
        reject: false,
        attempts: Vec::new(),
        queued: Vec::new(),
    };
    let graphs = Arc::new(store(vec![graph(
        GRAPH,
        vec![entry, branch, cue],
        json!([]),
        json!([]),
    )]));
    let mut component = Component::start(
        graphs,
        speaker,
        Variables::from([(1, Value::Array(vec![Value::Entity(speaker)]))]),
        vec![Root::listener(INSTANCE, GRAPH, None)],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();

    for (frame, delta, critical_hit, queued) in [
        (1, -5.0, Some(false), 0),
        (2, 0.0, Some(true), 0),
        (3, -5.0, Some(true), 1),
        (4, -5.0, None, 1),
        (5, 5.0, Some(true), 1),
    ] {
        let count = component
            .queue_host_event_with_context(
                HostEvent::HealthApplied {
                    source: Some(speaker),
                    target,
                    delta,
                    killed: false,
                    hit_tags: None,
                },
                HostHealthContext {
                    critical_hit,
                    ..HostHealthContext::default()
                },
            )
            .unwrap();
        assert_eq!(count, usize::from(delta != 0.0));
        component.advance_frame(frame, &mut world).unwrap();
        assert_eq!(world.queued.len(), queued, "frame {frame}");
    }
    assert_eq!(world.queued[0].1, [speaker]);
    assert_eq!(
        world.queued[0].2,
        RoomCue {
            stimulus: 0x31,
            line: 0x32,
            speaker: RoomCueSpeaker::Mode,
        }
    );
}
