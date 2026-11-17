use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use orpheus::{
    commands::Command,
    mover::Mover,
    mover_correction::CorrectionHistory,
    mover_modifiers::{DirectionModifier, Operation, directed_throttles},
    mover_settings::MoverSettings,
};
use orpheus_collision::{collision_index::TriangleIndex, contact_world::ContactWorld};
use serde_json::Value as Json;

fn load_contract() -> Json {
    let dir = std::env::var_os("ORPHEUS_BEHAVIOR_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/behavior"));
    let contract: Json =
        serde_json::from_slice(&std::fs::read(dir.join("server-only-movement-v1.json")).unwrap())
            .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "movement.server-only-direction.v1");
    assert_eq!(contract["target_build"], 104319);
    contract
}

#[test]
fn direction_mod_turns_throttles() {
    let contract = load_contract();
    let inputs = contract["input"]["direction_cases"].as_array().unwrap();
    let expected = contract["expected"]["direction_cases"].as_array().unwrap();
    assert_eq!(inputs.len(), expected.len());

    for (case, want) in inputs.iter().zip(expected) {
        let facing = [
            case["facing"][0].as_f64().unwrap() as f32,
            case["facing"][1].as_f64().unwrap() as f32,
        ];
        let throttles = [
            case["throttles"][0].as_f64().unwrap() as f32,
            case["throttles"][1].as_f64().unwrap() as f32,
        ];
        let mods: Vec<DirectionModifier> = case["mods"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                let op = match entry[0].as_str().unwrap() {
                    "add" => Operation::Add,
                    "set" => Operation::Set,
                    "mul" => Operation::Multiply,
                    other => panic!("unsupported direction mod op: {other}"),
                };
                let v = &entry[1];
                DirectionModifier {
                    operation: op,
                    value: [
                        v[0].as_f64().unwrap() as f32,
                        v[1].as_f64().unwrap() as f32,
                        v[2].as_f64().unwrap() as f32,
                    ],
                }
            })
            .collect();
        let actual = directed_throttles(&mods, throttles[0], throttles[1], facing);
        let want_vec = [
            want[0].as_f64().unwrap() as f32,
            want[1].as_f64().unwrap() as f32,
        ];
        assert_eq!(actual, want_vec, "case {case}");
    }
}

#[test]
fn server_only_motion_window_extends_and_retires_after_tail() {
    let contract = load_contract();
    let inputs = &contract["input"]["records"];
    let expected = &contract["expected"]["records"];
    let frame_us = inputs["frame_us"].as_u64().unwrap() as u32;
    let first = inputs["frames"][0].as_u64().unwrap() as u32;
    let last = inputs["frames"][1].as_u64().unwrap() as u32;
    let on_from = inputs["server_only_frames"][0].as_u64().unwrap() as u32;
    let on_through = inputs["server_only_frames"][1].as_u64().unwrap() as u32;

    let mut mover = Mover::new(
        MoverSettings::default(),
        Arc::new(ContactWorld::new(Vec::new()).unwrap()),
        [0.0, 1.0, 0.0],
        0,
        0,
    )
    .unwrap();
    let rays = TriangleIndex::new(Vec::new()).unwrap();
    let mut history = CorrectionHistory::new(0x1234, 1);
    let mut windows = BTreeMap::new();
    let mut sent = Vec::new();
    let dt = f64::from(frame_us) / 1e6;

    for frame in first..=last {
        let cmd = Command {
            frame,
            ..Command::default()
        };
        mover.step(&rays, cmd, dt, |_, _| {}).unwrap();
        history.record(mover.pose());
        if (on_from..=on_through).contains(&frame) {
            history.server_motion(frame, frame_us);
        }
        if let Some(update) = history.candidate_with_hero(frame, true, false) {
            assert!(history.sent(&update));
            sent.push(update.frame());
        }
        windows.insert(frame, history.impulse_correction());
    }

    for entry in expected["windows"].as_array().unwrap() {
        let frame = entry[0].as_u64().unwrap() as u32;
        let want = if entry[1].is_null() {
            None
        } else {
            Some((
                entry[1][0].as_u64().unwrap() as u32,
                entry[1][1].as_u64().unwrap() as u32,
            ))
        };
        assert_eq!(windows[&frame], want, "window mismatch at frame {frame}");
    }
    let sent_from = expected["sent_frames"][0].as_u64().unwrap() as u32;
    let sent_through = expected["sent_frames"][1].as_u64().unwrap() as u32;
    assert_eq!(sent, (sent_from..=sent_through).collect::<Vec<_>>());
}
