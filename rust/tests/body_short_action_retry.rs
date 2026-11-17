use std::path::PathBuf;

use orpheus::{
    script_codecs::CodecTable,
    script_graph::GraphStore,
    script_init::{EmptyWorld, MAX_INIT_WORK, Root, initialize},
    script_snapshot_wire::{FrameCallout, SnapshotEncoder, View},
    script_stream::{BodyStream, DeltaKind},
    script_wire::Variables,
};
use serde_json::{Value as Json, json};

fn load_contract() -> Json {
    let dir = std::env::var_os("ORPHEUS_BEHAVIOR_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/behavior"));
    let contract: Json = serde_json::from_slice(
        &std::fs::read(dir.join("body-short-action-retry-v1.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(
        contract["case_id"],
        "statescript.short-frame-action-retry.v1"
    );
    assert_eq!(contract["target_build"], 104319);
    contract
}

#[test]
fn owner_and_observer_keep_first_stamp_until_ack() {
    let contract = load_contract();
    let inputs = &contract["input"];
    let expected = &contract["expected"];

    let entity = inputs["entity"].as_u64().unwrap() as u32;
    let cmfd = inputs["command_frame"].as_u64().unwrap() as u32;
    let instance_id = inputs["instance"]["id"].as_u64().unwrap() as u16;
    let graph_index = inputs["instance"]["graph_index"].as_u64().unwrap() as u32;
    let action = &inputs["action"];
    let serial = action["serial"].as_u64().unwrap() as u32;
    let position = action["position"].as_u64().unwrap() as u16;
    let raw_index = action["raw_index"].as_u64().unwrap() as u16;
    let raw_bits = action["raw_bits"].as_u64().unwrap() as u8;
    let remote_index = action["remote_index"].as_u64().unwrap() as usize;
    let remote_bits = action["remote_bits"].as_u64().unwrap() as u8;

    let mut remote = vec![Json::Null; remote_index + 1];
    remote[remote_index] = json!(position);
    let graphs = GraphStore::from_json(
        &serde_json::to_vec(&json!({
            "version": 1,
            "graphs": [{
                "index": graph_index,
                "header": {
                    "m_nodesBitCount": raw_bits,
                    "m_statesBitCount": 0,
                    "m_remoteSyncNodesBitCount": remote_bits,
                    "m_syncVarsBitCount": 0
                },
                "fields": {},
                "items": position + 1,
                "containers": {},
                "unknown_nodes": [],
                "nodes": [{
                    "pos": position,
                    "class": "SyntheticAction",
                    "uid": 0,
                    "rid": raw_index,
                    "remote": remote_index as i32,
                    "state": null,
                    "client_only": 0,
                    "server_only": 0,
                    "fields": { "m_ADEB6E05": 1 }
                }],
                "states": [],
                "entries": [],
                "remote": remote,
                "sync_vars": []
            }],
            "bodies": {},
            "missing": []
        }))
        .unwrap(),
    )
    .unwrap();
    let codecs = CodecTable::from_json(r#"{"version":1,"classes":{}}"#).unwrap();
    let encoder = SnapshotEncoder::new(&graphs, &codecs);
    let snapshot = initialize(
        &graphs,
        entity,
        Variables::new(),
        vec![Root::new(instance_id, graph_index)],
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();

    let callout = FrameCallout {
        serial,
        instance: instance_id,
        position,
    };
    let delivery = inputs["delivery"].as_array().unwrap();
    let chunk_last = expected["chunk_last"].as_array().unwrap();
    let acknowledged_serial = expected["acknowledged_serial"].as_u64().unwrap() as u32;

    for (view, mode_name, kind) in [
        (
            View::Observer,
            "observer",
            DeltaKind::Observer { resend: false },
        ),
        (View::Owner, "owner", DeltaKind::InitialOwner(cmfd)),
    ] {
        let mut stream = BodyStream::new(entity);
        let base_proj = encoder.project(&snapshot, view).unwrap();
        let full = encoder.queue_full(&mut stream, &base_proj).unwrap();
        stream.create_arrived();
        assert!(stream.arrived(full.last).advanced);

        let pending = if view == View::Owner {
            base_proj.without_events().with_callout(callout)
        } else {
            base_proj.with_callout(callout)
        };
        let want_bits = expected[mode_name]["bit_count"].as_u64().unwrap() as usize;
        let want_hex = expected[mode_name]["chunk_hex"].as_array().unwrap();

        for (index, outcome) in delivery.iter().enumerate() {
            let chunk = encoder
                .queue_delta(&mut stream, &pending, kind, 16)
                .unwrap()
                .expect("expected delta chunk");
            let number = chunk_last[index].as_u64().unwrap() as u32;
            assert_eq!(chunk.last, number, "{mode_name} step {index}");

            let wire_chunk = chunk.update.chunk.as_ref().unwrap();
            assert_eq!(wire_chunk.bit_len(), want_bits, "{mode_name} step {index}");
            let hex: String = wire_chunk
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            assert_eq!(
                hex,
                want_hex[index].as_str().unwrap(),
                "{mode_name} step {index}"
            );

            match outcome.as_str().unwrap() {
                "lost" => {
                    stream.lost(number);
                }
                "acknowledged" => {
                    assert!(stream.arrived(number).advanced);
                }
                other => panic!("unexpected delivery outcome: {other}"),
            }
        }

        let (_, acked_base) = stream.base().unwrap();
        assert_eq!(acked_base.acknowledged_callout(), acknowledged_serial);
        if view == View::Observer {
            assert!(
                encoder
                    .queue_delta(&mut stream, &pending, kind, 16)
                    .unwrap()
                    .is_none()
            );
        }
    }
}
