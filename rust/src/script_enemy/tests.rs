use super::{policy::*, *};
use crate::script_control::LEGACY_PROJECTILE_CONTROL;

#[test]
fn authored_pose_queries_read_live_npc_pose_with_static_fallback() {
    let (graphs, _, definition, _) = fixture(false);
    let gate = EnemyGate {
        entity: 0x8000_0077,
        position: [0.0, 5.5, -3.0],
    };
    let pose = Movement {
        position: [-2.0, 0.0, 9.0],
        yaw: -16_384,
        pitch: 8_192,
        ..Movement::default()
    };
    let binding = EnemyBinding {
        body: 0xc000_1000,
        controller: 0xc000_1001,
        hero: definition.hero,
        pair: graphs.enemy_pair(definition.hero).unwrap(),
        mode: crate::game_content::PRACTICE_RANGE_MODE,
        mode_variables: Arc::default(),
        team: 1,
        skin: None,
        pose,
        gate: Some(gate),
        spatial_frame: None,
        roster: None,
        retired_entities: Arc::default(),
        frame: 0,
        now_secs: 0.0,
        spawn_anchors: None,
        mode_voice_set: None,
    };
    let mut vitals = definition.pools.vitals();
    let mut retirement_requested = false;
    let mut messages = VecDeque::new();
    let mut tags = TargetTagEffects::default();
    let mut behaviors = Default::default();
    let mut control = AiControlRecords::default();
    let empty = Variables::new();
    let mut world = EnemyWorld {
        retirement_requested: &mut retirement_requested,
        messages: &mut messages,
        target_tag_effects: &mut tags,
        behavior_records: &mut behaviors,
        control_records: &mut control,
        binding: &binding,
        graphs: &graphs,
        vitals: &mut vitals,
        body_variables: &empty,
        controller_variables: &empty,
    };
    assert_eq!(world.authored_position(binding.body), Some(pose.position));
    assert_eq!(world.forward(binding.body), None);
    let axis = std::f64::consts::FRAC_1_SQRT_2;
    let direction = world.authored_forward(binding.body).unwrap();
    for (actual, expected) in direction.into_iter().zip([-axis, -axis, 0.0]) {
        assert!((actual - expected).abs() < 1e-12);
    }

    assert_eq!(world.authored_position(gate.entity), Some(gate.position));
    assert_eq!(world.authored_forward(gate.entity), None);
    assert_eq!(world.authored_position(binding.controller), None);
    assert_eq!(world.authored_forward(binding.controller), None);
}

#[test]
fn npc_bound_body_angles_follow_the_live_pose_outside_encounter_mode() {
    use crate::script_expr::{AngleQuery, BodyAngle};
    let contract: Json = serde_json::from_str(include_str!(
        "../../../fixtures/behavior/script-bound-body-angle-v1.json"
    ))
    .unwrap();
    let poses = contract["input"]["poses"].as_array().unwrap();
    let expected = contract["expected"].as_array().unwrap();
    assert_eq!(poses.len(), expected.len());
    let (graphs, _, definition, _) = fixture(false);
    let mut binding = EnemyBinding {
        body: 0xc000_1000,
        controller: 0xc000_1001,
        hero: definition.hero,
        pair: graphs.enemy_pair(definition.hero).unwrap(),
        mode: crate::game_content::PRACTICE_RANGE_MODE,
        mode_variables: Arc::default(),
        team: 1,
        skin: None,
        pose: Movement::default(),
        gate: None,
        spatial_frame: None,
        roster: None,
        retired_entities: Arc::default(),
        frame: 0,
        now_secs: 0.0,
        spawn_anchors: None,
        mode_voice_set: None,
    };
    let mut vitals = definition.pools.vitals();
    let mut retirement_requested = false;
    let mut messages = VecDeque::new();
    let mut tags = TargetTagEffects::default();
    let mut behaviors = Default::default();
    let mut control = AiControlRecords::default();
    let empty = Variables::new();
    for (pose, angles) in poses.iter().zip(expected) {
        binding.pose.yaw = i32::try_from(pose["yaw"].as_i64().unwrap()).unwrap();
        binding.pose.pitch = i32::try_from(pose["pitch"].as_i64().unwrap()).unwrap();
        let world = EnemyWorld {
            retirement_requested: &mut retirement_requested,
            messages: &mut messages,
            target_tag_effects: &mut tags,
            behavior_records: &mut behaviors,
            control_records: &mut control,
            binding: &binding,
            graphs: &graphs,
            vitals: &mut vitals,
            body_variables: &empty,
            controller_variables: &empty,
        };
        for (axis, key) in [(BodyAngle::Yaw, "yaw"), (BodyAngle::Pitch, "pitch")] {
            assert_eq!(world.body_angle(0, axis), AngleQuery::Unbound);
            assert_eq!(
                world.body_angle(binding.body, axis),
                angles[key]
                    .as_f64()
                    .map_or(AngleQuery::Unavailable, AngleQuery::Present)
            );
            assert_eq!(
                world.body_angle(binding.controller, axis),
                AngleQuery::Unavailable
            );
            assert_eq!(
                world.body_angle(binding.body + 2, axis),
                AngleQuery::Unavailable
            );
        }
    }
}

#[test]
fn shared_captured_aim_lifecycle_contract() {
    let fixture: Json = serde_json::from_str(include_str!(
        "../../../fixtures/behavior/flashbang-captured-aim-lock-v1.json"
    ))
    .unwrap();
    let (graphs, _, definition, _) = fixture_with_graphs(false, &[fixture["graph"].clone()]);
    let binding = EnemyBinding {
        body: 0xc0001000,
        controller: 0xc0001001,
        hero: definition.hero,
        pair: graphs.enemy_pair(definition.hero).unwrap(),
        mode: crate::game_content::JUNKENSTEIN_REVENGE,
        mode_variables: Arc::default(),
        team: 1,
        skin: None,
        pose: Movement::default(),
        gate: None,
        spatial_frame: None,
        roster: None,
        retired_entities: Arc::default(),
        frame: 0,
        now_secs: 0.0,
        spawn_anchors: None,
        mode_voice_set: None,
    };
    let mut vitals = definition.pools.vitals();
    let mut retirement_requested = false;
    let mut messages = VecDeque::new();
    let mut tags = TargetTagEffects::default();
    let mut behaviors = Default::default();
    let mut control = AiControlRecords::default();
    let empty = Variables::new();
    let earlier = Site {
        entity: binding.body,
        instance: 99,
        graph: 0x0ce0,
        node: Some(45),
    };
    control.cinematic_aim.insert(earlier, [8192, 0]);
    let mut world = EnemyWorld {
        retirement_requested: &mut retirement_requested,
        messages: &mut messages,
        target_tag_effects: &mut tags,
        behavior_records: &mut behaviors,
        control_records: &mut control,
        binding: &binding,
        graphs: &graphs,
        vitals: &mut vitals,
        body_variables: &empty,
        controller_variables: &empty,
    };
    let mut roots = Vec::new();
    for (id, yaw, pitch) in [(1, 30.5, -22.5), (2, 90.0, 90.0)] {
        let mut root = script_init::Root::new(id, 0x224e);
        root.variables
            .extend([(501, Value::Float(yaw)), (502, Value::Float(pitch))]);
        roots.push(root);
    }
    let mut component = Component::start(
        Arc::clone(&graphs),
        binding.body,
        Variables::new(),
        roots,
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    let first = Site {
        instance: 1,
        graph: 0x224e,
        node: Some(34),
        ..earlier
    };
    let second = Site {
        instance: 2,
        ..first
    };
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot.initialization_complete(),
        "{:?}",
        snapshot.diagnostics
    );
    assert_eq!(world.control_records.cinematic_aim[&first], [5552, -4096]);
    assert_eq!(world.control_records.cinematic_aim[&second], [16384, 16202]);
    assert!(matches!(
        snapshot.instances[&1].states[&0].kind,
        script_init::StateKind::Registered { next: Some((2, 0)) }
    ));
    component
        .set_instance_variable(1, 501, Value::Float(-90.0), &mut world)
        .unwrap();
    component.advance_frame(1, &mut world).unwrap();
    assert_eq!(
        world.control_records.cinematic_aim[&first],
        [5552, -4096],
        "selector1 retains captured angles"
    );
    component
        .queue_message(2, 0x0240000000000002, Variables::new())
        .unwrap();
    component.run_now(&mut world).unwrap();
    assert!(!world.control_records.cinematic_aim.contains_key(&second));
    assert_eq!(
        world.control_records.cinematic_aim.last(),
        Some((&first, &[5552, -4096]))
    );
    assert!(matches!(
        component.snapshot().unwrap().instances[&1].states[&0].kind,
        script_init::StateKind::Registered { next: None }
    ));
    component
        .queue_message(1, 0x0240000000000002, Variables::new())
        .unwrap();
    component.run_now(&mut world).unwrap();
    assert_eq!(
        world.control_records.cinematic_aim.last(),
        Some((&earlier, &[8192, 0]))
    );
    component
        .queue_message(1, 0x0240000000000003, Variables::new())
        .unwrap();
    component.run_now(&mut world).unwrap();
    assert_eq!(world.control_records.cinematic_aim[&first], [-16384, -4096]);
    assert!(!world.set_captured_aim_lock(
        Site {
            node: Some(35),
            ..first
        },
        Some([0.0, 0.0])
    ));
    assert!(!world.set_captured_aim_lock(first, Some([f64::NAN, 0.0])));
    component
        .queue_message(1, 0x0240000000000002, Variables::new())
        .unwrap();
    component.run_now(&mut world).unwrap();
    assert_eq!(world.control_records.cinematic_aim.len(), 1);
}

#[test]
fn shared_flashbang_velocity_action_contract() {
    use crate::mover::VelocityChange;

    let fixture: Json = serde_json::from_str(include_str!(
        "../../../fixtures/behavior/flashbang-horizontal-cap-v1.json"
    ))
    .unwrap();
    let source_frame = u32::try_from(fixture["source_frame"].as_u64().unwrap()).unwrap();
    let active_frame = u32::try_from(fixture["active_frame"].as_u64().unwrap()).unwrap();
    let original = (0x224e, 31);
    let relocated = (0xf402, 12);
    for ((index, position), low, high, selector, encounter, accepted) in [
        (original, 0.0, 3.0, 1, true, true),
        (relocated, 0.0, 1.5, 1, true, true),
        (relocated, 1.0, 3.0, 1, true, false),
        (relocated, 0.0, -1.0, 1, true, false),
        (relocated, 0.0, 3.0, 0, true, false),
        (relocated, 0.0, 3.0, 1, false, false),
    ] {
        let mut graph = fixture["graph"].clone();
        graph["index"] = Json::from(index);
        graph["nodes"][0]["fields"]["m_outPlug"]["links"][0][0] = Json::from(position);
        graph["nodes"][1]["pos"] = Json::from(position);
        let row = &mut graph["nodes"][1]["fields"]["m_BC5E91CF"][0];
        row["m_8AD739CE"]["m_value"] = serde_json::json!(low);
        row["m_8213EB22"]["m_value"] = serde_json::json!(high);
        row["m_A9561CA0"] = serde_json::json!(selector);
        let (graphs, _, definition, _) = fixture_with_graphs(false, &[graph]);
        let binding = EnemyBinding {
            body: 0xc000_1000,
            controller: 0xc000_1001,
            hero: definition.hero,
            pair: graphs.enemy_pair(definition.hero).unwrap(),
            mode: if encounter {
                crate::game_content::JUNKENSTEIN_REVENGE
            } else {
                TUTORIAL_MODE
            },
            mode_variables: Arc::default(),
            team: 1,
            skin: None,
            pose: Movement::default(),
            gate: None,
            spatial_frame: None,
            roster: None,
            retired_entities: Arc::default(),
            frame: source_frame,
            now_secs: 1.6,
            spawn_anchors: None,
            mode_voice_set: None,
        };
        let mut vitals = definition.pools.vitals();
        let mut retirement_requested = false;
        let mut messages = VecDeque::new();
        let mut tags = TargetTagEffects::default();
        let mut behaviors = Default::default();
        let mut control = AiControlRecords::default();
        let empty = Variables::new();
        let mut world = EnemyWorld {
            retirement_requested: &mut retirement_requested,
            messages: &mut messages,
            target_tag_effects: &mut tags,
            behavior_records: &mut behaviors,
            control_records: &mut control,
            binding: &binding,
            graphs: &graphs,
            vitals: &mut vitals,
            body_variables: &empty,
            controller_variables: &empty,
        };
        let site = Site {
            entity: binding.body,
            instance: 1,
            graph: index,
            node: Some(position),
        };
        assert_eq!(
            world.player_stats_available(site),
            encounter.then_some(false)
        );
        for entity in [binding.controller, 999] {
            assert_eq!(world.player_stats_available(Site { entity, ..site }), None);
        }
        let component = Component::start(
            Arc::clone(&graphs),
            binding.body,
            Variables::new(),
            vec![script_init::Root::new(1, index)],
            source_frame,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        let snapshot = component.snapshot().unwrap();
        if selector == 0 {
            assert!(matches!(
                snapshot.diagnostics.as_slice(),
                [diagnostic]
                    if diagnostic.site == site
                        && diagnostic.issue
                            == script_init::Issue::UnsupportedAction("STU_FF3471E9".into())
            ));
        } else {
            assert!(
                snapshot.initialization_complete(),
                "{:?}",
                snapshot.diagnostics
            );
        }
        assert_eq!(
            snapshot.variables.get(&900),
            accepted.then_some(&Value::Bool(true))
        );
        assert_eq!(
            world.control_records.velocity_changes,
            if accepted {
                vec![(active_frame, VelocityChange::HorizontalCap(high))]
            } else {
                vec![]
            }
        );
        if accepted && (index, position) == relocated {
            for entity in [binding.controller, 999] {
                assert!(!world.cap_horizontal_velocity(
                    Site { entity, ..site },
                    source_frame,
                    high
                ));
            }
            for invalid in [-1.0, f64::INFINITY, f64::NAN, f64::MAX] {
                assert!(!world.cap_horizontal_velocity(site, source_frame, invalid));
            }
            assert!(!world.edit_velocity(site, source_frame, Value::Float(high), true));
            world.control_records.velocity_changes.resize(
                MAX_INIT_WORK,
                (active_frame, VelocityChange::HorizontalCap(high)),
            );
            assert!(!world.cap_horizontal_velocity(site, source_frame, high));
            assert_eq!(world.control_records.velocity_changes.len(), MAX_INIT_WORK);
            world.control_records.velocity_changes.clear();
            assert!(
                world
                    .vitals
                    .damage(
                        f64::MAX,
                        crate::vitals::Death {
                            time: binding.now_secs,
                            frame: source_frame,
                            killer: 999,
                        },
                    )
                    .killed
            );
            assert!(!world.cap_horizontal_velocity(site, source_frame, high));
            assert!(world.control_records.velocity_changes.is_empty());
        }
    }
}

#[test]
fn shared_flashbang_targeted_message_contract() {
    let contract: Json = serde_json::from_str(include_str!(
        "../../../fixtures/behavior/flashbang-targeted-message-v1.json"
    ))
    .unwrap();
    let (graphs, _, definition, _) = fixture_with_graphs(
        false,
        &[
            contract["sender_graph"].clone(),
            contract["receiver_graph"].clone(),
        ],
    );
    let source = contract["source"].as_u64().unwrap() as u32;
    let target = contract["target"].as_u64().unwrap() as u32;
    let other = contract["other"].as_u64().unwrap() as u32;
    let amount = Value::Float(contract["parameter"].as_f64().unwrap());
    let binding = EnemyBinding {
        body: source,
        controller: source + 1,
        hero: definition.hero,
        pair: graphs.enemy_pair(definition.hero).unwrap(),
        mode: crate::game_content::JUNKENSTEIN_REVENGE,
        mode_variables: Arc::default(),
        team: 1,
        skin: None,
        pose: Movement::default(),
        gate: None,
        spatial_frame: None,
        roster: None,
        retired_entities: Arc::default(),
        frame: 0,
        now_secs: 0.0,
        spawn_anchors: None,
        mode_voice_set: None,
    };
    let mut vitals = definition.pools.vitals();
    let mut retirement_requested = false;
    let mut messages = VecDeque::new();
    let mut tags = TargetTagEffects::default();
    let mut behaviors = Default::default();
    let mut control = AiControlRecords {
        defenders: Arc::from([target, other]),
        ..Default::default()
    };
    let empty = Variables::new();
    let mut world = EnemyWorld {
        retirement_requested: &mut retirement_requested,
        messages: &mut messages,
        target_tag_effects: &mut tags,
        behavior_records: &mut behaviors,
        control_records: &mut control,
        binding: &binding,
        graphs: &graphs,
        vitals: &mut vitals,
        body_variables: &empty,
        controller_variables: &empty,
    };
    let mut root = script_init::Root::new(1, 0x224e);
    root.variables
        .extend([(28714, Value::Entity(target)), (1903, amount.clone())]);
    let sender = Component::start(
        Arc::clone(&graphs),
        source,
        Variables::new(),
        vec![root],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert!(sender.snapshot().unwrap().initialization_complete());
    let site = Site {
        entity: source,
        instance: 1,
        graph: 0x224e,
        node: Some(22),
    };
    let params = Variables::from([(0x2b, amount.clone())]);
    for (invalid_site, invalid_target, invalid_params) in [
        (
            Site {
                entity: source + 1,
                ..site
            },
            target,
            params.clone(),
        ),
        (Site { graph: 42, ..site }, target, params.clone()),
        (
            Site {
                node: Some(23),
                ..site
            },
            target,
            params.clone(),
        ),
        (site, other + 2, params.clone()),
        (site, target, Variables::new()),
    ] {
        assert!(!world.send_message(
            invalid_site,
            invalid_target,
            junkenstein_messages::FLASHBANG_HIT,
            &invalid_params
        ));
    }
    world.control_records.defender_messages.resize(
        MAX_INIT_WORK,
        DefenderMessage::flashbang(site, target, &params).unwrap(),
    );
    assert!(!world.send_message(site, target, junkenstein_messages::FLASHBANG_HIT, &params));
    world.control_records.defender_messages.truncate(1);
    let request = world.control_records.defender_messages.pop().unwrap();
    assert_eq!(request.source, source);
    assert_eq!(request.target, Some(target));
    assert_eq!(request.parameters, params);
    assert!(world.messages.is_empty());
    for body in [target, other] {
        let mut receiver = Component::start(
            Arc::clone(&graphs),
            body,
            Variables::new(),
            vec![script_init::Root::listener(1, 0x01d8, None)],
            0,
            &mut script_init::EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap();
        if request.accepts_body(body) {
            assert_eq!(
                receiver
                    .queue_entity_message_from(
                        request.source,
                        request.message,
                        request.parameters.clone()
                    )
                    .unwrap(),
                1
            );
            receiver.run_now(&mut script_init::EmptyWorld).unwrap();
        }
        let snapshot = receiver.snapshot().unwrap();
        assert!(snapshot.initialization_complete());
        assert_eq!(
            snapshot.instances[&1].variables.get(&215),
            (body == target).then_some(&amount)
        );
        assert_eq!(
            snapshot.instances[&1].variables.get(&40),
            (body == target).then_some(&Value::Entity(source))
        );
    }
    let broadcast = DefenderMessage::authored(
        Site {
            graph: 0x0ce0,
            node: Some(2),
            ..site
        },
        BOSS_INTRO_ENDED,
    )
    .unwrap();
    assert!(broadcast.accepts_body(target) && broadcast.accepts_body(other));
}

#[test]
fn shared_self_damage_clock_contract() {
    let contract: Json = serde_json::from_str(include_str!(
        "../../../fixtures/behavior/enemy-self-damage-clock-v1.json"
    ))
    .unwrap();
    let (graphs, _, definition, _) = fixture(false);
    for case in contract["cases"].as_array().unwrap() {
        let frame = case["room_frame"].as_u64().unwrap() as u32;
        let time = case["now_secs"].as_f64().unwrap();
        let binding = EnemyBinding {
            mode_voice_set: None,
            body: 0xc000_1000,
            controller: 0xc000_1001,
            hero: definition.hero,
            pair: graphs.enemy_pair(definition.hero).unwrap(),
            mode: crate::game_content::JUNKENSTEIN_REVENGE,
            mode_variables: Arc::default(),
            team: 1,
            skin: None,
            pose: Movement {
                frame: case["wire_frame"].as_u64().map(|value| value as u32),
                ..Movement::default()
            },
            gate: None,
            spatial_frame: None,
            roster: None,
            retired_entities: Arc::default(),
            frame,
            now_secs: time,
            spawn_anchors: None,
        };
        let mut vitals = Vitals::new(contract["health"].as_f64().unwrap(), 0.0, 0.0);
        let mut retirement = false;
        let mut messages = VecDeque::new();
        let mut behaviors = IndexMap::new();
        let mut controls = AiControlRecords::default();
        let mut tags = TargetTagEffects::default();
        let empty = Variables::new();
        let mut world = EnemyWorld {
            retirement_requested: &mut retirement,
            messages: &mut messages,
            behavior_records: &mut behaviors,
            control_records: &mut controls,
            target_tag_effects: &mut tags,
            binding: &binding,
            graphs: &graphs,
            vitals: &mut vitals,
            body_variables: &empty,
            controller_variables: &empty,
        };
        let site = Site {
            entity: binding.body,
            instance: 1,
            graph: 0x0c89,
            node: Some(46),
        };
        assert!(!world.self_damage(
            Site {
                entity: binding.controller,
                ..site
            },
            10_000.0
        ));
        assert!(world.vitals.death().is_none());
        assert!(world.self_damage(site, contract["damage"].as_f64().unwrap()));
        assert_eq!(world.vitals.total(), 0.0);
        assert_eq!(
            world.vitals.death(),
            Some(crate::vitals::Death {
                time,
                frame,
                killer: binding.body,
            })
        );
        for (key, expected) in [
            ("corpse_before_due_frames", false),
            ("corpse_due_frames", true),
        ] {
            assert_eq!(
                crate::junkenstein_host::common_corpse_due(
                    0x02e0_0000_0000_0175,
                    world.vitals.death().map(|death| death.frame),
                    frame + contract[key].as_u64().unwrap() as u32,
                ),
                expected,
                "{case}"
            );
        }
    }
}

#[test]
fn local_health_events_capture_ai_self_damage_only_when_enabled() {
    let contract: Json = serde_json::from_str(include_str!(
        "../../../fixtures/behavior/script-local-health-outcomes-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(
        contract["case_id"],
        "script-local-health-outcomes.synthetic.v1"
    );
    assert_eq!(contract["evidence"], "synthetic-rust");
    let input = &contract["input"]["ai"];
    let expected = &contract["expected"]["ai"];
    let body = u32::try_from(input["body"].as_u64().unwrap()).unwrap();
    let controller = u32::try_from(input["controller"].as_u64().unwrap()).unwrap();
    let attempts = input["attempts"].as_array().unwrap();
    let accepted = expected["accepted"].as_array().unwrap();
    let remaining = expected["remaining_health"].as_array().unwrap();
    assert_eq!(attempts.len(), accepted.len());
    assert_eq!(attempts.len(), remaining.len());
    let expected_changes: Vec<_> = expected["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|change| {
            (
                u32::try_from(change["source"].as_u64().unwrap()).unwrap(),
                change["delta"].as_f64().unwrap(),
                change["killed"].as_bool().unwrap(),
            )
        })
        .collect();

    for enabled in [false, true] {
        let extra = enabled.then(|| input["listener_graph"].clone());
        let (graphs, _, definition, _) = fixture_with_graphs(false, extra.as_slice());
        assert_eq!(graphs.uses_host_health_events(), enabled);
        let binding = EnemyBinding {
            body,
            controller,
            hero: definition.hero,
            pair: graphs.enemy_pair(definition.hero).unwrap(),
            mode: 0,
            mode_variables: Arc::default(),
            team: 1,
            skin: None,
            pose: Movement::default(),
            gate: None,
            spatial_frame: None,
            roster: None,
            retired_entities: Arc::default(),
            frame: 3,
            now_secs: 0.048,
            spawn_anchors: None,
            mode_voice_set: None,
        };
        let mut vitals = Vitals::new(input["initial_health"].as_f64().unwrap(), 0.0, 0.0);
        let mut scripts = EnemyScripts::start(
            Arc::clone(&graphs),
            &definition,
            &binding,
            binding.frame,
            &mut vitals,
        )
        .unwrap();
        let empty = Variables::new();
        let mut world = EnemyWorld {
            retirement_requested: &mut scripts.retirement_requested,
            messages: &mut scripts.messages,
            behavior_records: &mut scripts.behavior_records,
            control_records: &mut scripts.control_records,
            target_tag_effects: &mut scripts.target_tag_effects,
            binding: &binding,
            graphs: &graphs,
            vitals: &mut vitals,
            body_variables: &empty,
            controller_variables: &empty,
        };
        for ((attempt, accepted), remaining) in attempts.iter().zip(accepted).zip(remaining) {
            let site = Site {
                entity: u32::try_from(attempt["actor"].as_u64().unwrap()).unwrap(),
                instance: 1,
                graph: 42,
                node: None,
            };
            let damage = attempt["damage"].as_f64().unwrap();
            assert_eq!(
                world.self_damage(site, damage),
                accepted.as_bool().unwrap(),
                "{attempt}"
            );
            assert_eq!(
                world.vitals.total(),
                remaining.as_f64().unwrap(),
                "{attempt}"
            );
        }
        let changes: Vec<_> = scripts
            .take_local_health_changes()
            .into_iter()
            .map(|change| (change.source, change.delta, change.killed))
            .collect();
        let expected = if enabled {
            expected_changes.as_slice()
        } else {
            &[]
        };
        assert_eq!(changes.as_slice(), expected);
        assert!(scripts.take_local_health_changes().is_empty());
    }
}

#[test]
fn shared_flashbang_exclusion_guard_contract() {
    use serde_json::json;
    let contract: Json = serde_json::from_str(include_str!(
        "../../../fixtures/behavior/statescript-filter-tag-clauses-v1.json"
    ))
    .unwrap();
    let mut checked = 0;
    for case in contract["cases"].as_array().unwrap() {
        if !case["name"].as_str().unwrap().starts_with("flashbang-") {
            continue;
        }
        let tags: Vec<_> = case["excluded"]
                .as_array()
                .unwrap()
                .iter()
                .map(|tag| {
                    json!({"$":"STU_357DABBB", "m_resourceKey":null,
                        "m_7D7C86A1":format!("0x{:016X}", 0x0d60_0000_0000_0000 | tag.as_u64().unwrap())})
                })
                .collect();
        let filter = json!({"$":"STUConfigVarFilter",
                "m_8B2A4DF3":{"$":"STU_06610EBC", "m_configVars":tags}});
        let inventory: BTreeSet<_> = case["inventory"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tag| 0x0d60_0000_0000_0000 | tag.as_u64().unwrap())
            .collect();
        let mut effects = TargetTagEffects::default();
        let actual = LEGACY_PROJECTILE_CONTROL
            .exclusions_admit(&filter, |tag| {
                effects
                    .presence(tag)
                    .or_else(|| Some(inventory.contains(&tag)))
            })
            .unwrap();
        assert_eq!(actual, case["accepted"].as_bool().unwrap(), "{case}");
        assert!(
            LEGACY_PROJECTILE_CONTROL
                .exclusions_admit(&filter, |_| None)
                .is_err()
        );
        let grant = TargetTagEffect {
            entity: 1,
            tag: 0x0d60_0000_0000_02cb,
            present: true,
        };
        assert!(effects.apply(grant, true));
        assert!(
            !LEGACY_PROJECTILE_CONTROL
                .exclusions_admit(&filter, |tag| {
                    effects
                        .presence(tag)
                        .or_else(|| Some(inventory.contains(&tag)))
                })
                .unwrap()
        );
        assert!(effects.apply(grant, false));
        // A live suppression masks an immutable tag and releasing that
        // suppression restores the base immunity on the next query.
        for &tag in &inventory {
            assert!(effects.apply(
                TargetTagEffect {
                    entity: 1,
                    tag,
                    present: false
                },
                true
            ));
        }
        assert!(
            LEGACY_PROJECTILE_CONTROL
                .exclusions_admit(&filter, |tag| {
                    effects
                        .presence(tag)
                        .or_else(|| Some(inventory.contains(&tag)))
                })
                .unwrap()
        );
        for &tag in &inventory {
            assert!(effects.apply(
                TargetTagEffect {
                    entity: 1,
                    tag,
                    present: false
                },
                false
            ));
        }
        assert_eq!(
            LEGACY_PROJECTILE_CONTROL
                .exclusions_admit(&filter, |tag| {
                    effects
                        .presence(tag)
                        .or_else(|| Some(inventory.contains(&tag)))
                })
                .unwrap(),
            actual
        );
        checked += 1;
    }
    assert_eq!(checked, 8);
}

#[test]
fn shared_flashbang_entry_guard_contract() {
    use serde_json::json;
    struct Empty;
    impl World for Empty {
        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }
        fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
            None
        }
    }
    let contract: Json = serde_json::from_str(include_str!(
        "../../../fixtures/behavior/flashbang-entry-guard-v1.json"
    ))
    .unwrap();
    let node = |pos, class, state, fields| {
        json!({
            "pos":pos,"class":class,"uid":pos,"rid":pos,"remote":-1,
            "state":state,"client_only":false,"server_only":false,"fields":fields
        })
    };
    let graph = |index, nodes: Vec<Json>, fields, states, entries| {
        json!({
            "index":index,"items":nodes.iter().map(|n| n["pos"].as_u64().unwrap()+1).max().unwrap_or(0),"nodes":nodes,"fields":fields,
            "states":states,"entries":entries,"remote":[],"sync_vars":[],
            "containers":{},"unknown_nodes":[],"header":{"m_nodesBitCount":4,
            "m_statesBitCount":4,"m_remoteSyncNodesBitCount":4,"m_syncVarsBitCount":4}
        })
    };
    for case in contract["cases"].as_array().unwrap() {
        let schema = json!({"m_publicSchema":{"$":"STUStatescriptSchema","m_entries":[{
            "m_0D09D2D9":"0x0D80000000007312",
            "m_value":{"$":"STUConfigVarBool","m_value":case["force_default"]}
        }]}});
        let overrides = if case["wrapper_override"] == true {
            json!([{
                "m_0D09D2D9":"0x0D80000000007312", "m_value":true
            }])
        } else {
            json!([])
        };
        let effect = graph(
            0x224e,
            vec![node(
                4,
                "STU_3387AB5D",
                Json::Null,
                json!({"m_condition":contract["condition"]}),
            )],
            schema,
            json!([]),
            json!([]),
        );
        let wrapper = graph(
            0x228,
            vec![node(
                7,
                "STUStatescriptActionPlayScript",
                Json::Null,
                json!({"m_69F4B52C":{"m_6525E98B":{"m_1EB5A024":overrides}}}),
            )],
            json!({}),
            json!([]),
            json!([]),
        );
        let linked = !case["stack_value"].is_null();
        let mut nodes = vec![node(
            0,
            "STUStatescriptEntry",
            Json::Null,
            json!({"m_outPlug":{"$":"Plug","links":if linked {json!([[1,"m_beginPlug"]])} else {json!([])}}}),
        )];
        if linked {
            nodes.push(node(
                1,
                "STUStatescriptStateStack",
                json!(0),
                json!({
                    "m_2BBEEAB8":true,
                    "m_out_Var":{"$":"STUConfigVarDynamic","m_60DB8F99":1,"m_identifier":2547},
                    "m_value":{"$":"STUConfigVarBool","m_value":case["stack_value"]},
                    "m_priority":{"$":"STUConfigVarFloat","m_value":0}
                }),
            ));
        }
        let body_graph = graph(
            9000,
            nodes,
            json!({}),
            if linked { json!([1]) } else { json!([]) },
            json!([0]),
        );
        let (graphs, _, _, _) = fixture_with_graphs(false, &[effect, wrapper, body_graph]);
        let base = case["base"].as_bool().map_or(Value::Null, Value::Bool);
        let component = Component::start(
            Arc::clone(&graphs),
            42,
            Variables::from([(2547, base)]),
            vec![script_init::Root::new(1, 9000)],
            0,
            &mut Empty,
            MAX_INIT_WORK,
        )
        .unwrap();
        assert!(component.snapshot().unwrap().initialization_complete());
        let actual = LEGACY_PROJECTILE_CONTROL.entry_admits(&graphs, |variable| {
            assert_eq!(variable.scope, crate::script_expr::Scope::Entity);
            assert_eq!(variable.id, 2547);
            if case["unavailable"] == true {
                None
            } else {
                component.resolved_variable(0, variable)
            }
        });
        if case["error"] == true {
            assert!(actual.is_err(), "{}", case["name"]);
        } else {
            assert_eq!(
                actual,
                Ok(case["accepted"].as_bool().unwrap()),
                "{}",
                case["name"]
            );
        }
    }
}

fn projectile_control_fixture(
    mode: u64,
    extra: &[Json],
) -> (Arc<GraphStore>, EnemyBinding, EnemyScripts, Vitals) {
    let (graphs, _, definition, _) = fixture_with_graphs(false, extra);
    let binding = EnemyBinding {
        body: 0xc0001000,
        controller: 0xc0001001,
        hero: definition.hero,
        pair: graphs.enemy_pair(definition.hero).unwrap(),
        mode,
        mode_variables: Arc::default(),
        team: 1,
        skin: None,
        pose: Default::default(),
        gate: None,
        spatial_frame: None,
        roster: None,
        retired_entities: Arc::default(),
        frame: 0,
        now_secs: 0.0,
        spawn_anchors: None,
        mode_voice_set: None,
    };
    let mut vitals = definition.pools.vitals();
    let scripts = EnemyScripts::start(
        Arc::clone(&graphs),
        &definition,
        &binding,
        1000,
        &mut vitals,
    )
    .unwrap();
    (graphs, binding, scripts, vitals)
}

#[test]
fn shared_flashbang_effect_dispatch_contract() {
    use serde_json::json;
    let contract: Json = serde_json::from_str(include_str!(
        "../../../fixtures/behavior/flashbang-effect-dispatch-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "flashbang.effect-dispatch.v1");
    for case in contract["input"]["cases"].as_array().unwrap() {
        let mode = crate::accounts::parse_guid(case["mode"].as_str().unwrap()).unwrap();
        let (graphs, binding, mut scripts, mut vitals) =
            projectile_control_fixture(mode, &[contract["input"]["graph"].clone()]);
        scripts.queue_legacy_projectile_effect(
            &binding,
            Site {
                entity: u32::try_from(case["source"].as_u64().unwrap()).unwrap(),
                instance: 1,
                graph: u32::try_from(case["graph"].as_u64().unwrap()).unwrap(),
                node: Some(u16::try_from(case["node"].as_u64().unwrap()).unwrap()),
            },
        );
        scripts
            .deliver_messages(&graphs, &binding, &mut vitals)
            .unwrap();
        let snapshot = scripts.body.snapshot().unwrap();
        let marker = snapshot.variables.get(&900).map(|value| match value {
            Value::Integer(value) => json!(value),
            other => panic!("{other:?}"),
        });
        let source = snapshot.variables.get(&901).map(|value| match value {
            Value::Entity(value) => json!(value),
            other => panic!("{other:?}"),
        });
        assert_eq!(
            json!({"marker":marker,"source":source,"wrappers":snapshot.instances.values().filter(|instance| instance.graph == 0x0228).count()}),
            case["expected"],
            "{}",
            case["name"]
        );
    }
}

#[test]
fn legacy_projectile_control_keeps_effect_delivery_separate_from_admission() {
    use serde_json::json;

    let dispatch: Json = serde_json::from_str(include_str!(
        "../../../fixtures/behavior/flashbang-effect-dispatch-v1.json"
    ))
    .unwrap();
    let guard: Json = serde_json::from_str(include_str!(
        "../../../fixtures/behavior/flashbang-entry-guard-v1.json"
    ))
    .unwrap();
    let normal = &dispatch["input"]["cases"][0];
    assert_eq!(normal["name"], "normal");
    let other_mode = &dispatch["input"]["cases"][1];
    assert_eq!(other_mode["name"], "other-mode");
    let mode = |case: &Json| crate::accounts::parse_guid(case["mode"].as_str().unwrap()).unwrap();
    let caller = Site {
        entity: u32::try_from(normal["source"].as_u64().unwrap()).unwrap(),
        instance: 1,
        graph: u32::try_from(normal["graph"].as_u64().unwrap()).unwrap(),
        node: Some(u16::try_from(normal["node"].as_u64().unwrap()).unwrap()),
    };
    let mut wrapper = dispatch["input"]["graph"].clone();
    let wrapper_index = u32::try_from(wrapper["index"].as_u64().unwrap()).unwrap();
    let hit = asset_guid(&wrapper["nodes"][0]["fields"]["m_gameMessage"]["m_gameMessage"]).unwrap();
    let test_node = |position, fields| {
        json!({
            "pos": position, "class": "FixtureDescriptor", "uid": position, "rid": position,
            "remote": -1, "state": null, "client_only": false, "server_only": false, "fields": fields,
        })
    };
    let mut effect = wrapper.clone();
    effect["index"] = json!(0x224e);
    effect["items"] = json!(123);
    effect["header"]["m_nodesBitCount"] = json!(7);
    effect["entries"] = json!([]);
    effect["nodes"] = json!([
        test_node(4, json!({"m_condition":guard["condition"]})),
        test_node(
            9,
            json!({"m_condition":{"m_filter":{"$":"STUConfigVarFilter"}}})
        ),
        test_node(
            122,
            json!({"m_condition":{
                "$":"STU_3ACE35FB",
                "m_E50CF556":{"$":"STUConfigVarDynamic","m_identifier":668,"m_60DB8F99":true}
            }})
        )
    ]);
    effect["fields"] = json!({
        "m_publicSchema":{"m_entries":[{
            "m_0D09D2D9":"0x0D80000000007312",
            "m_value":false
        }]}
    });
    wrapper["items"] = json!(8);
    wrapper["nodes"].as_array_mut().unwrap().push(test_node(
        7,
        json!({"m_69F4B52C":{"m_6525E98B":{"m_1EB5A024":[{
            "m_0D09D2D9":"0x0D800000000000D7","m_value":1.25
        }]}}}),
    ));

    let compatible_mode = mode(normal);
    for (mode, include_guard, blocked, expected) in [
        (compatible_mode, true, false, Ok(true)),
        (compatible_mode, true, true, Ok(false)),
        (compatible_mode, false, false, Err(())),
        (mode(other_mode), false, false, Ok(true)),
    ] {
        let mut extra = vec![wrapper.clone()];
        if include_guard {
            extra.push(effect.clone());
        }
        let (graphs, binding, mut scripts, mut vitals) = projectile_control_fixture(mode, &extra);
        assert_eq!(
            LEGACY_PROJECTILE_CONTROL.duration(&graphs),
            Some(&json!(1.25))
        );
        scripts
            .body
            .set_entity_variable(2547, Value::Bool(blocked), &mut script_init::EmptyWorld)
            .unwrap();
        let admitted = scripts.apply_legacy_projectile_control(&graphs, &binding, caller);
        assert_eq!(admitted.map_err(|_| ()), expected);

        if mode != compatible_mode {
            assert!(scripts.messages.is_empty());
            assert!(
                !scripts
                    .body
                    .snapshot()
                    .unwrap()
                    .variables
                    .contains_key(&900)
            );
            continue;
        }

        assert_eq!(scripts.messages.len(), 2);
        let play = &scripts.messages[0];
        assert_eq!(play.target, binding.body);
        assert!(matches!(
            &play.input,
            PairInput::PlayScript { caller: site, request }
                if *site == caller && request.graph == wrapper_index && request.source == caller.entity
        ));
        let message = &scripts.messages[1];
        assert_eq!(message.target, binding.body);
        assert!(matches!(
            &message.input,
            PairInput::Message { sender, message, required_graph, .. }
                if *sender == caller.entity && *message == hit && *required_graph == Some(wrapper_index)
        ));
        if expected.is_ok() {
            scripts
                .deliver_messages(&graphs, &binding, &mut vitals)
                .unwrap();
            let snapshot = scripts.body.snapshot().unwrap();
            assert_eq!(snapshot.variables[&900], Value::Integer(1));
            assert_eq!(snapshot.variables[&901], Value::Entity(caller.entity));
        }
    }
}

#[test]
#[ignore = "requires the prepared 104319 Boss intro and controller graphs"]
fn authored_boss_intro_end_releases_the_paired_controller_facing() {
    use crate::{game_content::JUNKENSTEIN_REVENGE, script_init::Root};
    let runtime = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let graphs = Arc::new(GraphStore::load(runtime.join("data/game_graphs_174.json.gz")).unwrap());
    let hero = 0x02e0_0000_0000_0002;
    let binding = EnemyBinding {
        mode_voice_set: None,
        body: 0xc000_1000,
        controller: 0xc000_1001,
        hero,
        pair: graphs.enemy_pair(hero).unwrap(),
        mode: JUNKENSTEIN_REVENGE,
        mode_variables: Arc::new(Variables::new()),
        team: 1,
        skin: None,
        pose: Movement::default(),
        gate: None,
        spatial_frame: None,
        roster: None,
        retired_entities: Arc::default(),
        frame: 0,
        now_secs: 0.0,
        spawn_anchors: None,
    };
    let mut vitals = Vitals::new(250.0, 0.0, 0.0);
    let mut messages = VecDeque::new();
    let mut behavior_records = IndexMap::new();
    let mut control_records = AiControlRecords::default();
    let mut target_tag_effects = TargetTagEffects::default();
    let (body, controller) = {
        let empty = Variables::new();
        let mut retirement_requested = false;
        let mut world = EnemyWorld {
            retirement_requested: &mut retirement_requested,
            messages: &mut messages,
            behavior_records: &mut behavior_records,
            control_records: &mut control_records,
            target_tag_effects: &mut target_tag_effects,
            binding: &binding,
            graphs: &graphs,
            vitals: &mut vitals,
            body_variables: &empty,
            controller_variables: &empty,
        };
        let mut intro = Root::listener(1, 0x0ce0, Some(3));
        intro
            .variables
            .extend([(8190, Value::Float(0.05)), (8504, Value::Float(0.0))]);
        let body = Component::start(
            Arc::clone(&graphs),
            binding.body,
            Variables::new(),
            vec![intro],
            0,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        let mut wave = Root::listener(1, 0x0c95, Some(1));
        wave.variables
            .insert(8370, Value::Array(vec![Value::Vector([74.0, 12.6, -99.0])]));
        let controller = Component::start(
            Arc::clone(&graphs),
            binding.controller,
            Variables::new(),
            vec![wave, Root::listener(2, 0x0ce2, None)],
            0,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        (body, controller)
    };
    let mut scripts = EnemyScripts {
        body,
        controller,
        control_records,
        behavior_records,
        target_tag_effects,
        messages,
        scene_messages: Vec::new(),
        retirement_requested: false,
        movement_modifiers: Default::default(),
    };
    assert!(scripts.facing_direction().is_some());
    let snapshot = scripts.controller.snapshot().unwrap();
    assert!(
        snapshot
            .instances
            .values()
            .any(|instance| instance.graph == 0x0cdc)
    );
    for frame in 1..=4 {
        scripts
            .advance(&graphs, &binding, &mut vitals, frame)
            .unwrap();
    }
    let snapshot = scripts.controller.snapshot().unwrap();
    assert!(
        !snapshot
            .instances
            .values()
            .any(|instance| instance.graph == 0x0cdc)
    );
    assert!(!snapshot.instances[&1].states[&1].active);
    assert_eq!(scripts.facing_direction(), None);
    assert_eq!(snapshot.instances[&2].variables[&699], Value::Bool(true));
    assert_eq!(
        scripts
            .take_defender_messages()
            .iter()
            .map(|message| (message.source, message.message))
            .collect::<Vec<_>>(),
        [
            (binding.body, BOSS_INTRO_ENDED),
            (binding.body, junkenstein_messages::RESET_HIGHLIGHTS)
        ],
    );
    scripts.advance(&graphs, &binding, &mut vitals, 5).unwrap();
    assert!(scripts.take_defender_messages().is_empty());
}

#[test]
#[ignore = "requires separately prepared 104319 Flashbang effect graphs"]
fn authored_flashbang_wrapper_sets_the_loaded_hit_effect_schema() {
    use crate::{game_heroes::HeroCatalog, npc_catalog::encounter_definition};
    use crate::{
        script_codecs::CodecTable,
        script_snapshot_wire::{SnapshotEncoder, View},
    };
    let root = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let graphs = Arc::new(GraphStore::load(root.join("data/game_graphs_174.json.gz")).unwrap());
    let heroes = HeroCatalog::load(root.join("data/game_heroes_174.json")).unwrap();
    let codecs = CodecTable::load(root.join("data/statescript_codecs_174.json")).unwrap();
    let encoder = SnapshotEncoder::new(&graphs, &codecs);
    for (hero, control_immune, entry_rejected) in [
        (REAPER, false, false),
        (ROADHOG, false, false),
        (SUMMONER, false, false),
        (JUNKRAT, false, false),
        (WITCH, false, false),
        (REAPER, true, false),
        (REAPER, false, true),
    ] {
        // Isolated body-adapter probe: declare Normal difficulty and an
        // explicitly empty combat/placement namespace. This does not run
        // the paired controller or establish the projectile-to-effect bridge.
        let definition = encounter_definition(None, &graphs, &heroes, hero).unwrap();
        let binding = EnemyBinding {
            mode_voice_set: None,
            body: 0xc000_1000,
            controller: 0xc000_1001,
            hero,
            pair: graphs.enemy_pair(hero).unwrap(),
            mode: crate::game_content::JUNKENSTEIN_REVENGE,
            mode_variables: Arc::new(Variables::from([(
                6751,
                Value::Asset(0x0d80_0000_0000_195e),
            )])),
            team: 1,
            skin: None,
            pose: Movement::default(),
            gate: None,
            spatial_frame: None,
            roster: Some(Arc::new(Vec::new())),
            retired_entities: Arc::default(),
            frame: 0,
            now_secs: 0.0,
            spawn_anchors: Some(Arc::default()),
        };
        let mut vitals = definition.pools.vitals();
        let mut retirement_requested = false;
        let mut messages = VecDeque::new();
        let mut tags = TargetTagEffects::default();
        let mut behaviors = Default::default();
        let mut control = AiControlRecords::default();
        let empty = Variables::new();
        let record = graphs.enemy_body(binding.pair.body).unwrap();
        let initial =
            script_init::player_entity_variables(record, vitals.total(), vitals.maximum());
        let mut roots = script_init::player_roots(record, &graphs).unwrap();
        if control_immune {
            roots.push(script_init::Root::listener(100, 0x01c9, Some(20)));
        }
        vitals = Vitals::empty();
        let mut world = EnemyWorld {
            retirement_requested: &mut retirement_requested,
            messages: &mut messages,
            target_tag_effects: &mut tags,
            behavior_records: &mut behaviors,
            control_records: &mut control,
            binding: &binding,
            graphs: &graphs,
            vitals: &mut vitals,
            body_variables: &initial,
            controller_variables: &empty,
        };
        let mut component = Component::start_with_public_schema(
            Arc::clone(&graphs),
            binding.body,
            initial.clone(),
            roots,
            1000,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        let exclusion_filter = LEGACY_PROJECTILE_CONTROL.exclusion_filter(&graphs).unwrap();
        let tag_site = Site {
            entity: binding.body,
            instance: 0,
            graph: 0x224e,
            node: Some(9),
        };
        assert!(
            LEGACY_PROJECTILE_CONTROL
                .exclusions_admit(exclusion_filter, |tag| {
                    world.has_target_tag(tag_site, binding.body, tag)
                })
                .unwrap()
        );
        let immunity = TargetTagEffect {
            entity: binding.body,
            tag: 0x0d60_0000_0000_02cb,
            present: true,
        };
        assert!(world.set_target_tag_effect(tag_site, immunity, true));
        assert!(
            !LEGACY_PROJECTILE_CONTROL
                .exclusions_admit(exclusion_filter, |tag| {
                    world.has_target_tag(tag_site, binding.body, tag)
                })
                .unwrap()
        );
        assert!(world.set_target_tag_effect(tag_site, immunity, false));
        assert!(
            LEGACY_PROJECTILE_CONTROL
                .exclusions_admit(exclusion_filter, |tag| {
                    world.has_target_tag(tag_site, binding.body, tag)
                })
                .unwrap()
        );
        if entry_rejected {
            component
                .set_entity_variable(2547, Value::Bool(true), &mut world)
                .unwrap();
            component.run_now(&mut world).unwrap();
        }
        let entry_guard = graphs
            .graph(0x224e)
            .unwrap()
            .node(4)
            .unwrap()
            .field("m_condition")
            .unwrap();
        let native_entry = component
            .evaluate_state_config(1, 0, entry_guard, &mut world)
            .unwrap();
        let admitted = LEGACY_PROJECTILE_CONTROL
            .entry_admits(&graphs, |var| component.resolved_variable(0, var))
            .unwrap();
        assert_eq!(admitted, crate::script_expr::truthy(&native_entry));
        if entry_rejected {
            assert!(!admitted);
        }
        let gate = graphs
            .graph(0x224e)
            .unwrap()
            .node(122)
            .unwrap()
            .field("m_condition")
            .unwrap();
        assert_eq!(
            LEGACY_PROJECTILE_CONTROL
                .stack_admits(&component, &graphs)
                .unwrap(),
            !control_immune
        );
        assert_eq!(
            component
                .evaluate_state_config(1, 0, gate, &mut world)
                .unwrap(),
            Value::Bool(!control_immune)
        );
        component
            .receive_play_script(
                Site {
                    entity: 0xa000_0101,
                    instance: 1,
                    graph: 0x01e9,
                    node: Some(7),
                },
                &PlayScript {
                    policy: 2,
                    overrides: vec![],
                    graph: 0x0228,
                    source: 0xa000_0101,
                    parameters: vec![],
                    auto_retire: false,
                },
                &mut world,
            )
            .unwrap();
        component
            .queue_entity_message_from(0xa000_0101, 0x0240_0000_0000_000f, Variables::new())
            .unwrap();
        component.run_now(&mut world).unwrap();
        let snapshot = component.snapshot().unwrap();
        let effects: Vec<_> = snapshot
            .instances
            .values()
            .filter(|instance| instance.graph == 0x224e)
            .collect();
        assert_eq!(effects.len(), 1);
        let effect = effects[0];
        assert_eq!(effect.variables[&215], Value::Float(f64::from(0.8f32)));
        assert_eq!(
            effect.variables[&27606],
            Value::Asset(0x0d80_0000_0000_4eb6)
        );
        assert_eq!(effect.variables[&9292], Value::Asset(0x0b90_0000_0000_0060));
        assert_eq!(
            effect.variables[&28383],
            Value::Asset(0x0300_0000_0000_05d9)
        );
        assert_eq!(effect.variables[&0x1b], Value::Entity(0xa000_0101));
        assert!(
            snapshot.initialization_complete(),
            "Boss {hero:X}: {:?}",
            snapshot.diagnostics
        );
        encoder
            .full(&encoder.project(snapshot, View::Observer).unwrap())
            .unwrap();
        assert!(!effect.states[&64].active, "deferred queue drain completed");
        assert!(
            effect.states[&71].active,
            "authored remote-schema state retained"
        );
        assert!(
            snapshot
                .variables
                .get(&31528)
                .is_none_or(|value| { *value == Value::Array(vec![]) })
        );
        for frame in 1001..=1100 {
            component.advance_frame(frame, &mut world).unwrap();
            let snapshot = component.snapshot().unwrap();
            assert!(
                snapshot.initialization_complete(),
                "Boss {hero:X}, frame {frame}: {:?}",
                snapshot.diagnostics
            );
            encoder
                .full(&encoder.project(snapshot, View::Observer).unwrap())
                .unwrap();
        }
    }
}

#[test]
#[ignore = "requires the external 104319 Boss body/controller graphs"]
fn prepared_boss_pools_register_without_profile_duplication() {
    use crate::{game_heroes::HeroCatalog, npc_catalog::encounter_definition};
    let root = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let graphs = Arc::new(GraphStore::load(root.join("data/game_graphs_174.json.gz")).unwrap());
    let heroes = HeroCatalog::load(root.join("data/game_heroes_174.json")).unwrap();
    for index in [2, 4, 0x16, 0x40, 0x65] {
        let hero = 0x02e0_0000_0000_0000 | index;
        let definition = encounter_definition(None, &graphs, &heroes, hero).unwrap();
        let binding = EnemyBinding {
            mode_voice_set: None,
            body: 0xc000_1000,
            controller: 0xc000_1001,
            hero,
            pair: graphs.enemy_pair(hero).unwrap(),
            mode: crate::game_content::JUNKENSTEIN_REVENGE,
            mode_variables: Arc::new(Variables::new()),
            team: 1,
            skin: None,
            pose: Movement::default(),
            gate: None,
            spatial_frame: None,
            roster: None,
            retired_entities: Arc::default(),
            frame: 0,
            now_secs: 0.0,
            spawn_anchors: None,
        };
        let mut vitals = definition.pools.vitals();
        let startup =
            EnemyScripts::start(Arc::clone(&graphs), &definition, &binding, 0, &mut vitals);
        assert_eq!(vitals.maximum(), definition.pools.total(), "Boss {hero:X}");
        for (kind, expected) in [
            (PoolKind::Health, definition.pools.health),
            (PoolKind::Armour, definition.pools.armour),
            (PoolKind::Shields, definition.pools.shields),
        ] {
            assert_eq!(
                vitals.amount(Some(kind), true),
                expected,
                "Boss {hero:X} {kind:?}"
            );
            assert_eq!(
                vitals.amount(Some(kind), false),
                expected,
                "Boss {hero:X} {kind:?}"
            );
        }
        // This isolates pool registration even while other Boss operations
        // remain unsupported. It must continue to pass after full startup works.
        if let Err(error) = startup {
            for issue in [
                "health pool registration",
                "health pool target",
                "health pool capacity",
                "health pool initial amount",
                "AI weapon control component",
            ] {
                assert!(!error.contains(issue), "Boss {hero:X}: {error}");
            }
            if matches!(index, 2 | 0x40) {
                assert!(error.starts_with("enemy controller startup:"), "{error}");
                for class in [
                    "STU_200B27C5",
                    "STU_AF1A4047",
                    "STU_55F72184",
                    "STU_E834E718",
                    "STU_632B74B9",
                    "STU_D52B9026",
                    "STU_E0D2F3AA",
                ] {
                    assert!(!error.contains(class), "Boss {hero:X}: {error}");
                }
            }
            eprintln!("Boss {hero:X} remaining startup diagnostics: {error}");
        }
    }
}

#[test]
fn common_enemy_pools_replace_bootstrap_and_follow_damage_abort_and_reentry() {
    use serde_json::json;
    let node = |pos, class, state: Option<u16>, mut fields: Json| {
        fields["m_2BBEEAB8"] = json!(state.is_some());
        fields["m_ADEB6E05"] = json!(class == "STUStatescriptActionSetVar");
        json!({"pos":pos,"class":class,"uid":pos,"rid":pos,"remote":-1,
                "state":state,"fields":fields,"client_only":false,"server_only":false})
    };
    let variable = |id| json!({"$":"STUConfigVarDynamic","m_identifier":id,"m_60DB8F99":true});
    let owner = json!({"$":"STU_B5A0CAF0"});
    let pool = |pos: u16, kind, amount, current, output| {
        node(
            pos,
            "STUStatescriptStateHealthPool",
            Some(pos - 1),
            json!({
                "m_FA3ECC43":owner,"m_7E8B666C":kind,"m_amount":{"$":"STUConfigVarInt","m_value":amount},
                "m_BCFDD2E9":current,"m_69A20070":{"$":"STU_FEDD683C","m_entity":owner},
                "m_4895B8B6":{"$":"STUConfigVarBool","m_value":true},
                "m_64839023":{"$":"STUConfigVarBool","m_value":false},
                "m_0E8A5C8E":{"$":"STUConfigVarBool","m_value":false},"m_CEAB19E8":output,
                "m_onEndPlug":{"$":"STUStatescriptOutputPlug","links":if kind==0 {json!([[6,"m_inPlug"]])}else{json!([])}}
            }),
        )
    };
    let nodes = vec![
        node(
            0,
            "STUStatescriptEntry",
            None,
            json!({"m_outPlug":{"$":"STUStatescriptOutputPlug","links":[[1,"m_beginPlug"],[2,"m_beginPlug"],[3,"m_beginPlug"]]}}),
        ),
        pool(
            1,
            0,
            200,
            json!({"$":"STUConfigVarInt","m_value":175}),
            variable(9001),
        ),
        pool(2, 1, 50, Json::Null, Json::Null),
        pool(3, 2, 25, Json::Null, Json::Null),
        node(
            4,
            "STUStatescriptEntryGameMessage",
            None,
            json!({"m_gameMessage":{"m_gameMessage":"0x0240000000000002"},"m_outPlug":{"$":"STUStatescriptOutputPlug","links":[[1,"m_F198FD3A"]]}}),
        ),
        node(
            5,
            "STUStatescriptEntryGameMessage",
            None,
            json!({"m_gameMessage":{"m_gameMessage":"0x0240000000000003"},"m_outPlug":{"$":"STUStatescriptOutputPlug","links":[[1,"m_beginPlug"]]}}),
        ),
        node(
            6,
            "STUStatescriptActionSetVar",
            None,
            json!({"m_out_Var":variable(9002),"m_value":variable(9001)}),
        ),
    ];
    let graph = |index, nodes: Vec<Json>, states, entries| {
        json!({"index":index,"items":nodes.len(),"nodes":nodes,
            "states":states,"entries":entries,"fields":{},"remote":[],"sync_vars":[],"containers":{},"unknown_nodes":[],
            "header":{"m_nodesBitCount":4,"m_statesBitCount":4,"m_remoteSyncNodesBitCount":4,"m_syncVarsBitCount":4}})
    };
    let body = |index| {
        json!({"hero":if index==51 {Some("0x02E0000000000175")}else{None},"name":"Fixture","manager":null,"weapons":[],"target_tags":[],
            "graphs":[{"graph":index,"m_1EB5A024":null,"m_75408640":0}]})
    };
    let data = json!({"version":1,"graphs":[graph(51,nodes,json!([1,2,3]),json!([0,4,5])),graph(43,vec![],json!([]),json!([]))],"bodies":{},"missing":[],
            "junkenstein":{"pairs":{"0x02E0000000000175":{"body":"0x04000000000016BE","controller":"0x04000000000016BF"}},
                "entities":{"0x04000000000016BE":body(51),"0x04000000000016BF":body(43)}}});
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&data).unwrap()).unwrap());
    let (_, _, definition, _) = fixture(false);
    let binding = EnemyBinding {
        mode_voice_set: None,
        body: 0xc000_1000,
        controller: 0xc000_1001,
        hero: definition.hero,
        pair: graphs.enemy_pair(definition.hero).unwrap(),
        mode: 0x0230_0000_0000_019f,
        mode_variables: Arc::new(Variables::new()),
        team: 1,
        skin: None,
        pose: Movement::default(),
        gate: None,
        spatial_frame: None,
        roster: None,
        retired_entities: Arc::default(),
        frame: 0,
        now_secs: 0.0,
        spawn_anchors: None,
    };
    let mut vitals = definition.pools.vitals();
    let mut scripts =
        EnemyScripts::start(Arc::clone(&graphs), &definition, &binding, 0, &mut vitals).unwrap();
    assert_eq!((vitals.total(), vitals.maximum()), (250.0, 275.0));
    let pool_id = match scripts.body.snapshot().unwrap().instances[&1].states[&0].kind {
        script_init::StateKind::HealthPool { pool: Some(id) } => id,
        _ => panic!("missing health state"),
    };
    let mut expected = Vitals::with_current_health(200.0, 50.0, 25.0, 175.0);
    let death = crate::vitals::Death {
        time: 0.0,
        frame: 0,
        killer: 99,
    };
    assert_eq!(vitals.damage(100.0, death), expected.damage(100.0, death));
    scripts.advance(&graphs, &binding, &mut vitals, 1).unwrap();
    let remaining = expected.amount(Some(PoolKind::Health), false);
    assert_eq!(
        scripts.body.snapshot().unwrap().variables[&9001],
        Value::Float(remaining)
    );
    scripts
        .body
        .queue_entity_message(0x0240_0000_0000_0002, Variables::new())
        .unwrap();
    scripts.advance(&graphs, &binding, &mut vitals, 2).unwrap();
    assert_eq!(
        scripts.body.snapshot().unwrap().variables[&9002],
        Value::Float(remaining)
    );
    assert_eq!(vitals.pool_current(pool_id), None);
    assert_eq!(vitals.maximum(), 75.0);
    scripts
        .body
        .queue_entity_message(0x0240_0000_0000_0003, Variables::new())
        .unwrap();
    scripts.advance(&graphs, &binding, &mut vitals, 3).unwrap();
    let next_id = match scripts.body.snapshot().unwrap().instances[&1].states[&0].kind {
        script_init::StateKind::HealthPool { pool: Some(id) } => id,
        _ => panic!("missing restarted health state"),
    };
    assert!(next_id > pool_id);
    assert_eq!(vitals.pool_current(next_id), Some(175.0));
    assert_eq!(vitals.maximum(), 275.0);
    assert_eq!(vitals.heal(25.0), 25.0);
    scripts.advance(&graphs, &binding, &mut vitals, 4).unwrap();
    assert_eq!(
        scripts.body.snapshot().unwrap().variables[&9001],
        Value::Float(200.0)
    );
    assert_eq!(
        scripts.body.snapshot().unwrap().variables[&1737],
        Value::Float(275.0)
    );
}

#[test]
fn shared_retired_entity_contract() {
    use crate::{
        script_entity_state::EntityScriptState,
        script_expr::{Context, Variable},
    };
    use serde_json::json;
    struct Host<'a, 'b> {
        world: &'a mut EnemyWorld<'b>,
        target: u32,
    }
    impl Context for Host<'_, '_> {
        fn variable(&mut self, variable: Variable) -> Value {
            assert_eq!(variable.id, 1);
            Value::Entity(self.target)
        }
        fn entity_member(&mut self, entity: u32, id: u32) -> Option<Value> {
            self.world.entity_variable(entity, id)
        }
        fn entity_exists(&mut self, entity: u32) -> Option<bool> {
            self.world.entity_exists(entity)
        }
        fn entity_team(&mut self, entity: u32) -> Option<i8> {
            self.world.entity_team(
                Site {
                    entity: self.world.binding.body,
                    instance: 1,
                    graph: 0x7000,
                    node: Some(0),
                },
                entity,
            )
        }
    }
    let contract: Json = serde_json::from_str(include_str!(
        "../../../fixtures/behavior/statescript-retired-entity-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["case_id"], "statescript.retired-entity.v1");
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["target_build"], 104319);
    let cases = contract["input"]["cases"].as_array().unwrap();
    let results = contract["expected"]["results"].as_array().unwrap();
    assert_eq!(cases.len(), results.len());
    for (case, expected) in cases.iter().zip(results) {
        assert_eq!(case["name"], expected["name"]);
        let (graphs, _, definition, _) = fixture(false);
        let target = u32::try_from(case["entity"].as_u64().unwrap()).unwrap();
        let state = case["state"].as_str().unwrap();
        let variables = case
            .get("value")
            .and_then(Json::as_bool)
            .map(|value| Variables::from([(6604, Value::Bool(value))]))
            .unwrap_or_default();
        let binding = EnemyBinding {
            mode_voice_set: None,
            body: 0xc000_1000,
            controller: 0xc000_1001,
            hero: definition.hero,
            pair: graphs.enemy_pair(definition.hero).unwrap(),
            mode: 0x0230_0000_0000_0003,
            mode_variables: Arc::default(),
            team: 1,
            skin: None,
            pose: Movement::default(),
            gate: None,
            frame: 0,
            now_secs: 0.0,
            spawn_anchors: None,
            retired_entities: Arc::new(if state == "retired" {
                BTreeSet::from([target])
            } else {
                BTreeSet::new()
            }),
            spatial_frame: None,
            roster: (!matches!(state, "retired" | "unavailable")).then(|| {
                Arc::new(vec![EnemyTarget {
                    hero: None,
                    filter_flags: None,
                    tags: Default::default(),
                    health: None,
                    velocity: None,
                    entity: target,
                    team: u8::try_from(case["team"].as_u64().unwrap()).unwrap(),
                    pose: TargetPose {
                        feet: [0.0; 3],
                        height: 1.8,
                        radius: 0.35,
                        eye: 1.55,
                    },
                    alive: true,
                    script_state: match state {
                        "running" => EntityScriptState::Running(Arc::new(variables)),
                        "absent" => EntityScriptState::Absent,
                        "faulted" => EntityScriptState::Unavailable,
                        _ => panic!("unknown state {state}"),
                    },
                }])
            }),
        };
        let mut behaviors = IndexMap::new();
        let mut controls = AiControlRecords::default();
        let mut messages = VecDeque::new();
        let mut retirement_requested = false;
        let mut target_tag_effects = TargetTagEffects::default();
        let mut vitals = definition.pools.vitals();
        let empty = Variables::new();
        let mut world = EnemyWorld {
            retirement_requested: &mut retirement_requested,
            binding: &binding,
            graphs: &graphs,
            vitals: &mut vitals,
            body_variables: &empty,
            controller_variables: &empty,
            behavior_records: &mut behaviors,
            control_records: &mut controls,
            messages: &mut messages,
            target_tag_effects: &mut target_tag_effects,
        };
        let mut host = Host {
            world: &mut world,
            target,
        };
        let mut actual = serde_json::Map::new();
        for (key, config) in contract["input"]["queries"].as_object().unwrap() {
            let result = crate::script_expr::evaluate(config, &mut host);
            let value = match result {
                Ok(Value::Null) => Json::Null,
                Ok(Value::Bool(value)) => json!(value),
                Ok(Value::Integer(value)) => json!(value),
                Err(
                    crate::script_expr::EvalError::UnavailableEntityVariable { .. }
                    | crate::script_expr::EvalError::UnsupportedConfig(_),
                ) => json!("unavailable"),
                other => panic!("{} {key}: {other:?}", case["name"]),
            };
            actual.insert(key.clone(), value);
        }
        assert_eq!(Json::Object(actual), expected["output"], "{}", case["name"]);
    }
}

#[test]
fn enemy_registries_validate_owners_and_preserve_priority_order() {
    let (graphs, _, definition, _) = fixture(false);
    let binding = EnemyBinding {
        mode_voice_set: None,
        body: 0xc000_1000,
        controller: 0xc000_1001,
        hero: definition.hero,
        pair: graphs.enemy_pair(definition.hero).unwrap(),
        mode: 0x0230_0000_0000_019f,
        mode_variables: Arc::new(Variables::new()),
        team: 1,
        skin: None,
        pose: Movement::default(),
        gate: None,
        spatial_frame: None,
        roster: None,
        retired_entities: Arc::default(),
        frame: 0,
        now_secs: 0.0,
        spawn_anchors: None,
    };
    let mut behaviors = indexmap::IndexMap::new();
    let mut controls = AiControlRecords::default();
    let mut messages = VecDeque::new();
    let mut retirement_requested = false;
    let mut target_tag_effects = TargetTagEffects::default();
    let mut vitals = definition.pools.vitals();
    let empty = Variables::new();
    let mut world = EnemyWorld {
        retirement_requested: &mut retirement_requested,
        binding: &binding,
        graphs: &graphs,
        vitals: &mut vitals,
        body_variables: &empty,
        controller_variables: &empty,
        behavior_records: &mut behaviors,
        control_records: &mut controls,
        messages: &mut messages,
        target_tag_effects: &mut target_tag_effects,
    };
    let site = |node| Site {
        entity: binding.controller,
        instance: 1,
        graph: 0x0cf3,
        node: Some(node),
    };
    let record = |priority, prefer_later| script_init::AiBehaviorRequest {
        class: "STU_B61E6681",
        priority,
        prefer_later,
        flag: false,
        shared: [0.0; 2],
        inputs: [0.0; 5],
    };
    for (node, priority, prefer_later) in [
        (1, 0.0, false),
        (2, 10.0, false),
        (3, 10.0, false),
        (4, 10.0, true),
    ] {
        assert_eq!(
            world.ai_behavior_request(site(node), record(priority, prefer_later), true),
            Some(true)
        );
    }
    assert_eq!(
        world
            .behavior_records
            .keys()
            .map(|key| key.2)
            .collect::<Vec<_>>(),
        [4, 2, 3, 1]
    );
    let mut updated = record(0.0, false);
    updated.inputs[0] = 3.0;
    world.ai_behavior_request(site(1), updated, true);
    assert_eq!(
        world
            .behavior_records
            .keys()
            .map(|key| key.2)
            .collect::<Vec<_>>(),
        [4, 2, 3, 1]
    );
    assert_eq!(
        world.behavior_records[&(binding.controller, 1, 1)].inputs[0],
        3.0
    );
    world.ai_behavior_request(site(2), record(10.0, false), false);
    assert_eq!(
        world
            .behavior_records
            .keys()
            .map(|key| key.2)
            .collect::<Vec<_>>(),
        [4, 3, 1]
    );
    let mut foreign = site(5);
    foreign.entity = binding.body;
    assert_eq!(
        world.ai_behavior_request(foreign, record(10.0, false), true),
        None
    );
    let weapon = AiWeaponOverride {
        target: binding.body,
        slot: 0,
        identifier: 0x0d80_0000_0000_023a,
        enabled: false,
        priority: 0.0,
        prefer_later: false,
    };
    let stronger = AiWeaponOverride {
        enabled: true,
        priority: 10.0,
        ..weapon
    };
    let replace_equal = AiWeaponOverride {
        prefer_later: true,
        ..stronger
    };
    for (node, record) in [
        (1, weapon),
        (2, stronger),
        (3, stronger),
        (4, replace_equal),
    ] {
        assert_eq!(
            world.ai_weapon_override(site(node), record, true),
            Some(true)
        );
    }
    let ordered = |world: &EnemyWorld<'_>| {
        world
            .control_records
            .weapons
            .keys()
            .map(|site| site.node.unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(ordered(&world), [4, 2, 3, 1]);
    assert_eq!(world.control_records.weapons[&site(1)], weapon);
    assert_eq!(
        world.ai_weapon_override(site(5), AiWeaponOverride { slot: 1, ..weapon }, true),
        Some(false)
    );
    assert_eq!(
        world.ai_weapon_override(
            site(5),
            AiWeaponOverride {
                target: binding.controller,
                ..weapon
            },
            true
        ),
        None
    );
    assert_eq!(ordered(&world), [4, 2, 3, 1]);
    assert_eq!(
        world.ai_weapon_override(site(4), replace_equal, false),
        Some(true)
    );
    assert_eq!(ordered(&world), [2, 3, 1]);

    let source = site(1);
    let body_source = Site {
        entity: binding.body,
        ..source
    };
    let effect = TargetTagEffect {
        entity: binding.body,
        tag: 0x0d60_0000_0000_0422,
        present: true,
    };
    assert_eq!(
        world.has_target_tag(source, binding.body, effect.tag),
        Some(false)
    );
    assert!(!world.set_target_tag_effect(source, effect, true));
    assert!(world.set_target_tag_effect(body_source, effect, true));
    assert_eq!(
        world.has_target_tag(source, binding.body, effect.tag),
        Some(true)
    );
    assert_eq!(
        world.has_target_tag(source, binding.controller, effect.tag),
        Some(false)
    );
    assert!(world.set_target_tag_effect(body_source, effect, false));
    assert_eq!(
        world.has_target_tag(source, binding.body, effect.tag),
        Some(false)
    );

    for config in [
        serde_json::json!({"$":"STU_37F20D24"}),
        serde_json::json!({"$":"STU_37F20D24","m_EE729DCB":0,"m_resourceKey":null}),
    ] {
        assert_eq!(
            world.native(source, &config),
            Some(Value::Asset(binding.mode))
        );
    }
    for config in [
        serde_json::json!({"$":"STU_37F20D24","m_resourceKey":"0x0D80000000000001"}),
        serde_json::json!({"$":"STU_37F20D24","m_unknown":null}),
    ] {
        assert_eq!(world.native(source, &config), None);
    }
    let body_site = Site {
        entity: binding.body,
        ..source
    };
    let foreign_site = Site {
        entity: 999,
        ..source
    };
    assert_eq!(world.health_pool_current(body_site, 3), Some(200.0));
    for invalid in [source, foreign_site] {
        assert_eq!(
            world.add_health_pool(invalid, PoolKind::Health, 100.0, 100.0),
            None
        );
        assert_eq!(world.health_pool_current(invalid, 3), None);
        assert_eq!(world.remove_health_pool(invalid, 3), None);
    }
    assert_eq!(world.vitals.total(), 275.0);
    let wave_site = Site {
        entity: binding.body,
        graph: 0x0cbc,
        ..source
    };
    let bonus = world.add_wave_health(wave_site, 750.0).unwrap().unwrap();
    assert_eq!(world.vitals.total(), 1025.0);
    assert_eq!(world.add_wave_health(wave_site, 750.0), None);
    assert_eq!(world.remove_health_pool(wave_site, bonus), Some(750.0));
    assert_eq!(world.vitals.total(), 275.0);
    assert_eq!(world.entity_hero(source, binding.body), Some(binding.hero));
    assert_eq!(world.entity_hero(source, binding.controller), None);
    assert_eq!(
        world.entity_definition(source, binding.body),
        Some(binding.pair.body)
    );
    assert_eq!(
        world.entity_definition(source, binding.controller),
        Some(binding.pair.controller)
    );
    assert_eq!(world.entity_definition(source, 999), None);
    assert_eq!(world.entity_definition(foreign_site, binding.body), None);
    assert_eq!(
        world.entity_hero(
            Site {
                entity: 999,
                ..source
            },
            binding.body
        ),
        None
    );
    assert_eq!(
        world.entity_health(source, binding.body, Some(PoolKind::Health), false),
        Some(200.0)
    );
    assert_eq!(
        world.entity_health(
            Site {
                entity: 999,
                ..source
            },
            binding.body,
            None,
            false
        ),
        None
    );
    assert_eq!(
        world.entity_filter_flags(source, binding.body),
        Some(0x1000001)
    );
    world.vitals.damage(
        1000.0,
        crate::vitals::Death {
            time: 0.0,
            frame: 1,
            killer: 999,
        },
    );
    assert_eq!(
        world.entity_filter_flags(source, binding.body),
        Some(0x1000011)
    );
    assert_eq!(
        world.entity_health(source, binding.body, None, false),
        Some(0.0)
    );
    assert_eq!(
        world.entity_health(source, binding.body, Some(PoolKind::Health), true),
        Some(200.0)
    );
    assert_eq!(world.add_wave_health(wave_site, 750.0), Some(None));
}
