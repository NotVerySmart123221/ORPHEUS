use super::*;
use crate::{script_enemy::fixture, script_snapshot_wire::SceneProjection, world::frame};

#[test]
fn npc_script_chunks_and_reliable_messages_advance_on_separate_frames() {
    let contract: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/npc-script-packet-isolation-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "npc-script-packet-isolation-v1");
    assert_eq!(contract["evidence"], "rust-synthetic");
    let input = &contract["input"];
    let npc = input["npc"].as_u64().unwrap() as u32;
    let placeable = input["placeable"].as_u64().unwrap() as u32;
    let start = input["first_tick"].as_u64().unwrap() as u32;
    let first = input["first_sequence"].as_u64().unwrap() as u32;

    let (graphs, codecs, _, _) = fixture(false);
    let mut world = WorldDelivery::new(OWNER);
    world.started = Some(start);
    world.bots.insert(npc, BotLifetime::Live);
    world.gate = Some(placeable);
    world.created.extend([npc, placeable]);
    for entity in [npc, placeable] {
        let script = Component::start(
            Arc::clone(&graphs),
            entity,
            crate::script_wire::Variables::new(),
            vec![Root::new(1, 42)],
            start,
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap();
        world
            .present_scene_script(SceneProjection {
                snapshot: script.snapshot().unwrap(),
                encoder: SnapshotEncoder::new(&graphs, &codecs),
            })
            .unwrap();
    }
    let ready = world.ready();
    assert_eq!(ready.len(), 2);
    let npc_chunk = ready[0].tracked();
    let placeable_chunk = ready[1].tracked();

    let mut egress = GameEgress::default();
    let message = vec![1, 2];
    let frames = input["frames"].as_array().unwrap();
    let expected_frames = contract["expected"]["frames"].as_array().unwrap();
    assert_eq!(frames.len(), expected_frames.len());
    for (index, (step, expected)) in frames.iter().zip(expected_frames).enumerate() {
        let offset = index as u32;
        let tick = start + offset;
        let sequence = first + offset;
        if let Some(ordinal) = step["queue_reliable"].as_u64() {
            assert_eq!(
                u64::from(egress.queue_reliable(message.clone()).unwrap()),
                ordinal
            );
        }
        if step["queue_unreliable"].as_bool() == Some(true) {
            egress.queue_unreliable(message.clone());
        }
        let expected_messages: Vec<_> = expected["reliable_ordinal"]
            .as_u64()
            .map(|ordinal| (ordinal as u32, message.clone()))
            .into_iter()
            .collect();
        let expected_records: Vec<_> = [&npc_chunk, &placeable_chunk]
            .into_iter()
            .filter(|record| {
                Some(u64::from(record.update.entity)) == expected["script_entity"].as_u64()
            })
            .map(|record| &record.update)
            .collect();
        let unreliable = if expected["unreliable"].as_bool() == Some(true) {
            std::slice::from_ref(&message)
        } else {
            &[]
        };
        let expected_payload = frame(
            tick,
            world.command_ack(tick),
            &expected_messages,
            unreliable,
            &expected_records,
        )
        .unwrap();

        if step["transport_retries"].as_bool() == Some(true) {
            // Failed transport must preserve the reserved turn and both queues.
            let failed = world.send_frame(tick, vec![], false, None, &mut egress, |payload| {
                assert_eq!(payload, expected_payload);
                Err(())
            });
            assert!(matches!(failed, Err(SendError::Transport(()))));
        }

        let (payload, sent) = transmit(&mut world, None, &mut egress, tick, sequence);
        assert_eq!(payload, expected_payload);
        assert_eq!(sent.records.len(), expected_records.len());
        transport_outcome(
            &mut world,
            &mut egress,
            sequence,
            step["arrived"].as_bool().unwrap(),
        );
        let npc_base = world.scene_views[&npc].base().is_some();
        assert_eq!(Some(npc_base), expected["npc_base"].as_bool());
        if npc_base {
            assert!(world.ready().is_empty());
        } else {
            assert!(Arc::ptr_eq(&npc_chunk, &world.ready()[0].tracked()));
        }
    }
    assert!(world.scene_views[&placeable].base().is_some());
    assert!(egress.delivered_through(2));
}
