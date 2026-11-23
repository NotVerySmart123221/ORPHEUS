use super::*;
use crate::{script_enemy::fixture, script_snapshot_wire::SceneProjection, world::frame};

#[test]
fn scripted_respawn_becomes_a_weapon_target_after_its_observer_baseline() {
    use crate::world::Create;

    let contract: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/respawn-weapon-visibility-v1.json"
    ))
    .unwrap();
    let input = &contract["input"];
    let number = |key: &str| u32::try_from(input[key].as_u64().unwrap()).unwrap();
    let retired = number("retired");
    let replacement = number("replacement");
    let tick = number("first_tick");
    let expected = |phase: &str| contract["expected"][phase].as_bool().unwrap();
    let (graphs, codecs, _, _) = fixture(false);
    let script = Component::start(
        Arc::clone(&graphs),
        replacement,
        crate::script_wire::Variables::new(),
        vec![Root::new(1, 42)],
        tick,
        &mut EmptyWorld,
        MAX_INIT_WORK,
    )
    .unwrap();
    let start = |owner| {
        let mut world = WorldDelivery::new(owner);
        world.started = Some(tick);
        world.bots.insert(retired, BotLifetime::Live);
        world.created.insert(retired);
        world.replace_bot(BotReplacement {
            retired,
            created: EntityUpdate {
                entity: replacement,
                operation: Some(Operation::Create(Box::new(Create::new(1, vec![])))),
                ..EntityUpdate::default()
            },
            waits_for_presentation: true,
        });
        world
            .present_scene_script(SceneProjection {
                snapshot: script.snapshot().unwrap(),
                encoder: SnapshotEncoder::new(&graphs, &codecs),
            })
            .unwrap();
        world
    };
    let mut fast = start(OWNER);
    let mut slow = start(OWNER + 1);
    let mut fast_egress = GameEgress::default();
    let mut slow_egress = GameEgress::default();
    for (world, egress) in [(&mut fast, &mut fast_egress), (&mut slow, &mut slow_egress)] {
        assert!(!world.weapon_target_ready(retired));
        assert_eq!(
            world.weapon_target_ready(replacement),
            expected("weapon_target_before_create_ack")
        );
        transmit(world, None, egress, tick, 0);
        transport_outcome(world, egress, 0, true);
        assert!(world.identity_ready(replacement));
        assert_eq!(
            world.weapon_target_ready(replacement),
            expected("weapon_target_after_create_ack")
        );
        transmit(world, None, egress, tick + 1, 1);
    }
    transport_outcome(&mut fast, &mut fast_egress, 1, true);
    transport_outcome(&mut slow, &mut slow_egress, 1, false);
    assert_eq!(
        fast.weapon_target_ready(replacement),
        expected("weapon_target_after_animation_ack")
    );
    assert_eq!(
        slow.weapon_target_ready(replacement),
        expected("weapon_target_after_animation_loss")
    );
    transmit(&mut slow, None, &mut slow_egress, tick + 2, 2);
    transport_outcome(&mut slow, &mut slow_egress, 2, true);
    assert_eq!(
        slow.weapon_target_ready(replacement),
        expected("weapon_target_after_animation_ack")
    );
}

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
    let message = crate::game_messages::EncodedMessage::from_bytes(vec![1, 2]);
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
