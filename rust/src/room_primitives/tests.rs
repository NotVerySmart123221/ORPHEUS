use std::collections::BTreeMap;

use orpheus_statescript::script_wire::Value;

use super::*;

#[test]
fn obb_trigger_handles_rotation_and_excludes_spectators() {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let obb = ObbTrigger::from_raw([0.0, s, 0.0, s, 20.0, 1.0, 0.0, 4.0, 2.0, 2.0]);
    assert!(obb.contains([20.0, 1.0, 3.0]));
    assert!(!obb.contains([23.0, 1.0, 0.0]));

    let zone = ObbZone::new(vec![obb]);
    let actors = vec![
        ActorSample::living(1, 1, [20.0, 1.0, 0.0]),
        ActorSample::living(2, 0, [20.0, 1.0, 1.0]),
        ActorSample::living(3, SPECTATOR_TEAM, [20.0, 1.0, 0.0]),
        ActorSample {
            entity: 4,
            team: 1,
            position: [20.0, 1.0, 0.0],
            alive: false,
        },
    ];
    let occ = zone.occupants(&actors);
    assert_eq!(occ.team0, vec![2]);
    assert_eq!(occ.team1, vec![1]);
}

#[test]
fn spline_mover_and_rider_carry_track_uphill_progress() {
    let end = [0.0, 2.0, 4.0];
    let mut n0 = SplineNode::simple([0.0, 0.0, 0.0]);
    n0.curve = Some([[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], end]);
    let mut n1 = SplineNode::simple(end);
    n1.checkpoint = Some(30);
    let track = SplineTrack::from_nodes(vec![n0, n1]);
    let mut mover = SplineMover::new(track, SplineSpeedCurve::default(), false);

    let start_pose = mover.pose();
    assert_eq!(start_pose.pitch, -4836);
    let rider_start = [0.0, 1.0, 0.0];
    let local = RiderCarry::local_anchor(start_pose, rider_start);

    let dt = 0.016;
    for tick in 1..=60 {
        let now = (tick as f64) * dt;
        let pose = mover.pose();
        let carried = RiderCarry::world_target(pose, local);
        mover.step(now, &[ActorSample::living(101, 1, carried)], 1, 0);
        let new_pose = mover.pose();
        let new_carried = RiderCarry::world_target(new_pose, local);
        let drift = dist3(RiderCarry::local_anchor(new_pose, new_carried), local);
        assert!(drift < 0.08);
    }
    assert!(mover.distance > 1.2);
}

#[test]
fn interactive_prop_basketball_bounces_scores_and_resets() {
    let mut rt = InteractivePropRuntime::default();
    let ball = PhysicsProp::new(1001, [0.0, 1.0, 0.0], 0.15);
    rt.props.insert(1001, ball);
    rt.goals.push(TriggerGoal {
        entity: 2001,
        volume: ObbTrigger::from_raw([0.0, 0.0, 0.0, 1.0, 1.3, 1.2, 0.0, 0.5, 0.5, 0.5]),
        require_downward: true,
        effect_cue_id: 0x0300_0000_0000_CAFE,
        triggered_entities: BTreeSet::new(),
    });
    rt.buttons.insert(
        3001,
        ResetButtonProp {
            entity: 3001,
            linked_props: vec![1001],
            reset_cue_id: 0x0300_0000_0000_BEEF,
        },
    );

    // Hit ball toward hoop (vx=2.0, vy=5.5 -> descends through [1.3, 1.2, 0.0] around t=0.65s)
    let strength = (2.0_f64 * 2.0 + 5.5 * 5.5).sqrt();
    assert!(rt.apply_hit_impulse(1001, [2.0, 5.5, 0.0], strength));
    let mut scored = false;
    for _ in 0..120 {
        rt.step(0.016, 0.0, &[]);
        if rt
            .emitted_cues
            .iter()
            .any(|c| c.source_entity == 2001 && c.cue_id == 0x0300_0000_0000_CAFE)
        {
            scored = true;
            break;
        }
    }
    assert!(scored, "basketball should score through hoop trigger");

    // Press refresh button to reset basketball cleanly
    assert!(rt.press_button(3001));
    let prop = &rt.props[&1001];
    assert_eq!(prop.position, [0.0, 1.0, 0.0]);
    assert_eq!(prop.velocity, [0.0, 0.0, 0.0]);
    assert!(
        rt.emitted_cues
            .iter()
            .any(|c| c.source_entity == 3001 && c.cue_id == 0x0300_0000_0000_BEEF)
    );
}

#[test]
fn dynamic_barrier_registry_and_voice_router_behave_cleanly() {
    let mut gate = BarrierEntry::new(43, BarrierKind::CheckpointGate, vec![(1, 1)]);
    gate.gate_tag = Some(1);
    gate.open_seconds = 7.0;
    gate.open_delay = 0.5;
    gate.shapes.push(BarrierShape::Sphere {
        center: [0.0, 0.0, 0.0],
        radius: 1.0,
        mask: 0x18,
        translation: [43.0, 0.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
    });
    let mut reg = DynamicBarrierRegistry::new(vec![gate]);
    let ent = MAP_ENTITY_BIT | 43;
    assert!(!reg.door_passable(ent, 90.0, 0, true));
    assert_eq!(
        reg.active_contact_world(90.0, 0, true)
            .unwrap()
            .shapes()
            .len(),
        1
    );
    assert!(reg.request_route_gate(1, true, 90.0, 0.0));
    let opening = reg.door_state(ent, 90.5, 1, true, None);
    assert_eq!(opening.pose, DoorPose::Opening);
    let opened = reg.door_state(ent, 97.5, 1, true, Some(&opening));
    assert_eq!(opened.pose, DoorPose::Open);
    assert!(reg.door_passable(ent, 97.5, 1, true));
    assert_eq!(
        reg.active_contact_world(97.5, 1, true)
            .unwrap()
            .shapes()
            .len(),
        0
    );

    let mut bus = DynamicEntityBus::default();
    bus.spawn(55);
    bus.set_var(55, 12, Value::Integer(99));
    assert_eq!(bus.get_var(55, 12), Some(&Value::Integer(99)));
    bus.send(55, 0x0240_0000_0000_0001, BTreeMap::new());
    assert_eq!(bus.queue.len(), 1);

    let mut voice = RoomVoiceRouter::default();
    let cues = [VoiceCue::new(0x060, 0x060D), VoiceCue::new(0x194, 0x0619)];
    voice.enqueue(1, CAPTURE_FOLLOW_UP_GAP, 1, &cues);
    assert_eq!(voice.poll(1.0, 1, |_| true), None);
    assert_eq!(voice.poll(4.49, 1, |_| true), None);
    assert_eq!(voice.poll(4.5, 1, |_| true), Some(cues[1]));
}
