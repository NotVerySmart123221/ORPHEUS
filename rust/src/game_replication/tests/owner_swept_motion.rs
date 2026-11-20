use super::*;

#[test]
fn swept_owner_correction_uses_the_applied_pose_and_outlives_a_script_fault() {
    use crate::script_init::SweptDisplacement;

    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/owner-swept-motion-v1.json"
    ))
    .unwrap();
    let input = &fixture["input"];
    let frame = |name: &str| u32::try_from(input[name].as_u64().unwrap()).unwrap();
    let requested =
        SweptDisplacement::new(serde_json::from_value(input["displacement"].clone()).unwrap())
            .unwrap();
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

    for fault_after_pose in [false, true] {
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
                .queue_swept_displacement(due, requested)
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
