use super::*;

const OWNER: u32 = 81;
const SOURCE: u32 = 82;
const TARGET: u32 = 83;
const KILLED: u32 = 84;
const FRAME: u32 = 85;
const PREVIOUS: u32 = 86;
const COUNT: u32 = 87;
const AUDIENCE: u32 = 88;

#[derive(Deserialize)]
struct Contract {
    owner: u32,
    ally: u32,
    opponents: Vec<u32>,
    window_frames: u32,
    cues: Vec<Cue>,
    events: Vec<Elimination>,
}

#[derive(Deserialize)]
struct Cue {
    count: u32,
    line: u64,
}

#[derive(Deserialize)]
struct Elimination {
    name: String,
    source: Option<u32>,
    target: u32,
    frame: Option<u32>,
    killed: bool,
    count: u32,
    cue: Option<u64>,
}

fn expression(code: &[u8], variables: &[u32], numbers: &[u32]) -> Json {
    json!({
        "$": "STUConfigVarExpression",
        "m_expression": {
            "m_opcodes": code,
            "m_dynamicVars": variables.iter().map(|&id| var(id, true)).collect::<Vec<_>>(),
            "m_D99EF254": numbers,
        },
    })
}

fn authored_counter(contract: &Contract) -> Vec<Json> {
    let next = |pos| plug(&[(pos, "m_inPlug")], &[]);
    let branch = |pos, condition, pass, fail: Option<u16>| {
        let mut branch = node(
            pos,
            "SyntheticBranchAction",
            None,
            json!({
                "m_condition": condition,
                "m_truePlug": next(pass),
                "m_falsePlug": fail.map_or_else(|| plug(&[], &[]), next),
            }),
        );
        branch["server_only"] = json!(true);
        branch["action_behavior"] = json!("branch");
        branch
    };
    let set = |pos, variable, value, then| {
        node(
            pos,
            "STUStatescriptActionSetVar",
            None,
            json!({
                "m_out_Var": var(variable, true), "m_value": value, "m_outPlug": next(then),
            }),
        )
    };
    let binding = |slot, variable| {
        json!({
            "m_B5051BCE": format!("0x{slot:016X}"),
            "m_out_Var": var(variable, true),
        })
    };
    let mut entry = node(
        0,
        "EntryHostEvent",
        None,
        json!({
            "m_params": [binding(1, SOURCE), binding(2, TARGET), binding(4, KILLED), binding(8, FRAME)],
            "m_outPlug": next(1),
        }),
    );
    entry["server_only"] = json!(true);
    entry["host_event_entry"] = json!("health_applied");
    let cue_position = |index| 11 + u16::try_from(index).unwrap();
    let cases: Vec<_> = contract
        .cues
        .iter()
        .enumerate()
        .map(|(index, cue)| {
            let mut case = next(cue_position(index));
            case["m_value"] = json!(cue.count);
            case
        })
        .collect();
    let mut nodes = vec![
        entry,
        branch(1, var(KILLED, true), 2, None),
        branch(
            2,
            expression(&[8, 0, 8, 1, 37, 0], &[SOURCE, OWNER], &[]),
            3,
            None,
        ),
        branch(
            3,
            json!({
                "$": "EntityMatches",
                "entity": var(TARGET, true),
                "filter": {"relation": "opponent", "source": var(OWNER, true)},
            }),
            4,
            None,
        ),
        branch(4, expression(&[8, 0, 6, 0, 36, 0], &[FRAME], &[0]), 5, None),
        branch(
            5,
            expression(&[8, 0, 8, 1, 35, 0], &[FRAME, PREVIOUS], &[]),
            6,
            None,
        ),
        branch(
            6,
            expression(
                &[8, 0, 8, 1, 24, 6, 0, 33, 0],
                &[FRAME, PREVIOUS],
                &[contract.window_frames],
            ),
            7,
            Some(8),
        ),
        set(
            7,
            COUNT,
            expression(&[8, 0, 6, 0, 23, 0], &[COUNT], &[1]),
            9,
        ),
        set(8, COUNT, json!(1), 9),
        set(9, PREVIOUS, var(FRAME, true), 10),
        node(
            10,
            "STUStatescriptActionSwitch",
            None,
            json!({
                "m_262AE982": var(COUNT, true), "m_FDD34C25": cases,
            }),
        ),
    ];
    for (index, cue) in contract.cues.iter().enumerate() {
        let mut action = node(
            cue_position(index),
            "SyntheticCueAction",
            None,
            json!({
                "stimulus": "0x0000000000000031",
                "line": format!("0x{:016X}", cue.line),
                "speaker": "mode",
                "audience": var(AUDIENCE, true),
                "m_outPlug": plug(&[], &[]),
            }),
        );
        action["server_only"] = json!(true);
        action["action_behavior"] = json!("room_cue");
        nodes.push(action);
    }
    nodes
}

#[test]
fn timed_elimination_cues_use_settlement_frames_and_authored_thresholds() {
    let contract: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-timed-elimination-cues-v1.json",
    )))
    .unwrap();
    let facts = [contract.owner, contract.ally]
        .into_iter()
        .map(|id| (id, 1))
        .chain(contract.opponents.iter().map(|&id| (id, 2)))
        .map(|(id, team)| {
            (
                id,
                RoomEntityFacts {
                    team: Some(team),
                    alive: true,
                    tags: TargetTagSnapshot::default(),
                },
            )
        })
        .collect();
    let mut world = CueWorld {
        speaker: contract.owner,
        center: [0.0; 3],
        candidates: Vec::new(),
        facts: Some(facts),
        reject: false,
        generations: BTreeMap::new(),
        attempts: Vec::new(),
        queued: Vec::new(),
    };
    let graph = graph(GRAPH, authored_counter(&contract), json!([]), json!([]));
    let mut component = Component::start(
        Arc::new(store(vec![graph])),
        contract.owner,
        Variables::from([
            (OWNER, Value::Entity(contract.owner)),
            (COUNT, Value::Integer(0)),
            (PREVIOUS, Value::Integer(0)),
            (AUDIENCE, Value::Array(vec![Value::Entity(contract.owner)])),
        ]),
        vec![Root::listener(INSTANCE, GRAPH, None)],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();

    for (index, event) in contract.events.iter().enumerate() {
        assert_eq!(
            component
                .queue_host_event_with_context(
                    HostEvent::HealthApplied {
                        source: event.source,
                        target: event.target,
                        delta: -10.0,
                        killed: event.killed,
                        hit_tags: None,
                    },
                    HostHealthContext {
                        damage_frame: event.frame,
                        ..HostHealthContext::default()
                    },
                )
                .unwrap(),
            1
        );
        // VM delivery advances by a different interval than the settlement frames.
        component
            .advance_frame(150 + 20 * u32::try_from(index).unwrap(), &mut world)
            .unwrap();
        let actual = &component.snapshot().unwrap().variables[&COUNT];
        assert!(
            crate::script_expr::equal(actual, &Value::Integer(i64::from(event.count))),
            "{}",
            event.name
        );
        let cues = std::mem::take(&mut world.queued);
        assert_eq!(
            cues.len(),
            usize::from(event.cue.is_some()),
            "{}",
            event.name
        );
        if let Some(line) = event.cue {
            assert_eq!(cues[0].1, [contract.owner]);
            assert_eq!(
                cues[0].2,
                RoomCue {
                    stimulus: 0x31,
                    line,
                    speaker: RoomCueSpeaker::Mode
                }
            );
        }
    }
}
