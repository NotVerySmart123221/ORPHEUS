use super::*;

#[test]
fn posed_owner_fault_cancels_future_velocity_before_the_recovered_batch_continues() {
    use crate::mover::VelocityChange;

    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/owner-script-motion-cancellation-v1.json"
    ))
    .unwrap();
    assert_eq!(fixture["schema_version"], 1);
    assert_eq!(fixture["evidence"], "synthetic-rust");
    let input = &fixture["input"];
    let frame = |name: &str| u32::try_from(input[name].as_u64().unwrap()).unwrap();
    let spawn = frame("spawn_frame");
    let warm = frame("warm_pose_frame");
    let first_fault = frame("fault_pose_frame");
    let due = frame("velocity_due_frame");
    let recovered = frame("recovered_command_frame");
    let delta = serde_json::from_value(input["velocity_delta"].clone()).unwrap();
    let actor = input["actor"].as_u64().unwrap();
    assert_eq!(first_fault, warm + 1);
    assert!(first_fault < due && due < recovered);
    let card = PlayerCard::default();
    let viewer = PlayerAppearance {
        entity: OWNER,
        slot: 0,
        team: 0,
        card: &card,
    };

    for fault_after_pose in [false, true] {
        let mut scene = PreparedScene::fixture_for_bots(Vec::new());
        let mut world = WorldDelivery::new(OWNER);
        let id = avatar(1, 1);
        let fault = Arc::new(AtomicBool::new(false));
        world.started = Some(spawn);
        world.input.processed = Some(spawn);
        activate_script(
            &mut world,
            scene.simulation_mut(),
            id,
            actor,
            Arc::clone(&fault),
        );
        world.queue_commands(
            warm,
            &[Command {
                frame: warm,
                ..Command::default()
            }],
        );
        world
            .advance_input(&mut scene, &viewer, RETAIL_QUANTA, 16)
            .unwrap();
        let mover = &mut scene.simulation_mut().player_body_mut(id).unwrap().mover;
        assert_eq!(mover.pose().frame, Some(warm));
        assert_eq!(mover.pose().position[0], 0.0);
        mover.queue_velocity_change(due, VelocityChange::Delta(delta));

        // The fixture still advances its component, but snapshot() becomes
        // unavailable. The first recovered physics pose therefore completes
        // before the owner's posed VM check reports the failure.
        fault.store(fault_after_pose, Ordering::Relaxed);
        world.queue_commands(
            recovered,
            &[Command {
                frame: recovered,
                ..Command::default()
            }],
        );
        world
            .advance_input(&mut scene, &viewer, RETAIL_QUANTA, 16)
            .unwrap();
        let pose = scene.simulation().player_body(id).unwrap().mover.pose();
        assert_eq!(pose.frame, Some(recovered));
        assert_eq!(world.input.processed, Some(recovered));
        assert!(world.input.work.is_empty());
        assert_eq!(world.body_script.is_none(), fault_after_pose);
        if fault_after_pose {
            let report = world.owner_script_fault.as_ref().unwrap();
            assert_eq!(report.id, id);
            assert_eq!(report.frame, first_fault);
            assert_eq!(report.step, Some(BodyStep::Posed));
            assert_eq!(
                pose.position[0],
                fixture["expected"]["faulted_final_x"].as_f64().unwrap()
            );
            assert_eq!(pose.velocity[0], 0.0);
        } else {
            assert!(world.owner_script_fault.is_none());
            assert!(pose.position[0] > 0.0, "control pose: {pose:?}");
            assert!(pose.velocity[0] > 0.0);
        }
    }
}

#[test]
fn player_displacement_uses_room_actor_contacts_for_the_authoritative_pose() {
    use crate::{mover::VelocityChange, script_init::SweptDisplacement};

    let card = PlayerCard::default();
    let viewer = PlayerAppearance {
        entity: OWNER,
        slot: 0,
        team: 0,
        card: &card,
    };
    let displacement = SweptDisplacement::new([4.0, 0.0, 0.0]).unwrap();
    let target_x = 2.0;
    for (blocker_team, velocity_x) in [(0, 0.0), (1, 0.0), (1, 160.0)] {
        let mut scene = PreparedScene::fixture_for_bots(Vec::new());
        let room = scene.simulation_mut();
        let mut world = WorldDelivery::new(OWNER);
        let id = avatar(1, 1);
        let mut target = avatar(2, 1);
        target.entity += 0x100;
        world.started = Some(100);
        activate_script(&mut world, room, id, 7, Arc::default());
        room.spawn_player_body(PlayerBodyState::new(
            target,
            OWNER + 0x100,
            1,
            blocker_team,
            7,
            mover_at([target_x, 0.0, 0.0]),
            None,
            100,
            None,
        ))
        .unwrap();
        room.enable_movement_contacts(
            false,
            Arc::new(crate::mover_settings::MoverCatalog::default()),
        );
        room.prepare_movement_frame(101).unwrap();
        let mover = &mut room.player_body_mut(id).unwrap().mover;
        assert!(mover.queue_swept_displacement(101, displacement));
        mover.queue_velocity_change(101, VelocityChange::Delta([velocity_x, 0.0, 0.0]));

        world.queue_commands(
            101,
            &[Command {
                frame: 101,
                ..Command::default()
            }],
        );
        world
            .advance_input(&mut scene, &viewer, RETAIL_QUANTA, 16)
            .unwrap();
        let moved_x = scene
            .simulation()
            .player_body(id)
            .unwrap()
            .mover
            .pose()
            .position[0];
        if blocker_team == 0 {
            assert_eq!(moved_x, 4.0);
        } else {
            assert!(0.0 < moved_x && moved_x < target_x, "position: {moved_x}");
        }
    }
}

#[test]
fn swept_owner_correction_uses_the_applied_pose_and_outlives_a_script_fault() {
    use crate::script_init::{Relocation, ScriptedMotion, SweptDisplacement};

    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/owner-swept-motion-v1.json"
    ))
    .unwrap();
    let input = &fixture["input"];
    let frame = |name: &str| u32::try_from(input[name].as_u64().unwrap()).unwrap();
    let requested =
        SweptDisplacement::new(serde_json::from_value(input["displacement"].clone()).unwrap())
            .unwrap();
    let sweep = ScriptedMotion::Sweep(requested);
    let relocation =
        ScriptedMotion::Relocate(Relocation::new(requested.delta().map(f64::from)).unwrap());
    let spawn = frame("spawn_frame");
    let due = frame("due_frame");
    let tail_pose = frame("tail_pose");
    let later_ack = frame("later_ack");
    let generic_actor = input["actor"].as_u64().unwrap();
    let card = PlayerCard::default();
    let viewer = PlayerAppearance {
        entity: OWNER,
        slot: 0,
        team: 0,
        card: &card,
    };

    for (motion, fault_after_pose) in [
        (sweep, false),
        (sweep, true),
        (relocation, false),
        (relocation, true),
    ] {
        let mut scene = PreparedScene::fixture_for_bots(Vec::new());
        let mut world = WorldDelivery::new(OWNER);
        let mut egress = GameEgress::default();
        let id = avatar(1, 1);
        let fault = Arc::new(AtomicBool::new(false));
        world.started = Some(spawn);
        world.input.processed = Some(spawn);
        activate_script(
            &mut world,
            scene.simulation_mut(),
            id,
            generic_actor,
            Arc::clone(&fault),
        );
        assert!(world.body_script.as_ref().unwrap().corrections.is_none());
        world.queue_commands(
            due - 1,
            &[Command {
                frame: due - 1,
                ..Command::default()
            }],
        );
        world
            .advance_input(&mut scene, &viewer, RETAIL_QUANTA, 16)
            .unwrap();
        let (pre_move, _) = transmit(
            &mut world,
            Some(scene.simulation_mut()),
            &mut egress,
            spawn + 1,
            0,
        );
        assert_eq!(read_ack(&pre_move), due - 1);
        transport_outcome(&mut world, &mut egress, 0, true);

        fault.store(fault_after_pose, Ordering::Relaxed);
        assert!(
            scene
                .simulation_mut()
                .player_body_mut(id)
                .unwrap()
                .mover
                .queue_scripted_motion(due, motion)
        );
        world.queue_commands(
            due,
            &[Command {
                frame: due,
                ..Command::default()
            }],
        );
        world
            .advance_input(&mut scene, &viewer, RETAIL_QUANTA, 16)
            .unwrap();
        assert_eq!(world.body_script.is_none(), fault_after_pose);
        assert_eq!(world.detached_motion.is_some(), fault_after_pose);
        let moved = scene.simulation().player_body(id).unwrap().mover.pose();
        assert_eq!(moved.frame, Some(due));
        assert_eq!(moved.position[0], f64::from(requested.delta()[0]));

        let (lead, blocked) = transmit(
            &mut world,
            Some(scene.simulation_mut()),
            &mut egress,
            spawn + 2,
            1,
        );
        assert_eq!(read_ack(&lead), due);
        assert!(!blocked.records.contains(&RecordSource::Movement(0)));
        transport_outcome(&mut world, &mut egress, 1, true);
        world.acknowledge(
            outcome(Arc::new(record(id.entity, Delivery::CreateBody(id))), true),
            Duration::ZERO,
        );
        let first = transmit(
            &mut world,
            Some(scene.simulation_mut()),
            &mut egress,
            due,
            2,
        )
        .1;
        assert!(first.records.contains(&RecordSource::Movement(0)));
        let history = if let Some((owner, history)) = &world.detached_motion {
            assert_eq!(*owner, id);
            history
        } else {
            world
                .body_script
                .as_ref()
                .unwrap()
                .corrections
                .as_ref()
                .unwrap()
        };
        assert_eq!(history.corrected(), Some(due));
        transport_outcome(&mut world, &mut egress, 2, true);

        for frame in due + 1..=tail_pose {
            world.queue_commands(
                frame,
                &[Command {
                    frame,
                    ..Command::default()
                }],
            );
            world
                .advance_input(&mut scene, &viewer, RETAIL_QUANTA, 16)
                .unwrap();
        }
        world.input.processed = Some(later_ack);
        let (capped, last) = transmit(
            &mut world,
            Some(scene.simulation_mut()),
            &mut egress,
            tail_pose,
            3,
        );
        assert_eq!(read_ack(&capped), tail_pose + 2);
        assert!(last.records.contains(&RecordSource::Movement(0)));
        transport_outcome(&mut world, &mut egress, 3, true);
        assert!(world.detached_motion.is_none());
        assert!(
            world
                .body_script
                .as_ref()
                .is_none_or(|script| script.corrections.is_none())
        );
        let (bytes, sent) = transmit(
            &mut world,
            Some(scene.simulation_mut()),
            &mut egress,
            tail_pose + 1,
            4,
        );
        assert_eq!(read_ack(&bytes), later_ack);
        assert!(!sent.records.contains(&RecordSource::Movement(0)));
    }
}
