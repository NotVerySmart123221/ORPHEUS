use super::*;
use crate::game_heroes::HeroCatalog;

#[test]
fn same_hero_pick_retries_a_halted_script() {
    #[derive(serde::Deserialize)]
    struct Contract {
        schema_version: u32,
        input: Input,
        expected: Vec<u64>,
    }

    #[derive(serde::Deserialize)]
    struct Input {
        hero: u64,
        cases: Vec<Case>,
    }

    #[derive(serde::Deserialize)]
    struct Case {
        scripted: bool,
        input_fault: bool,
    }

    let Contract {
        schema_version,
        input,
        expected,
    } = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/owner-hero-reselect-v1.json"
    ))
    .unwrap();
    assert_eq!(schema_version, 1);
    assert_eq!(input.cases.len(), expected.len());

    let hero = input.hero;
    let heroes = serde_json::json!({
        format!("0x{hero:016X}"): {
            "name": "Synthetic tank", "class": "Tank", "body": "0x0400000000000713",
        },
    });
    let viewer = PlayerAppearance {
        entity: OWNER,
        slot: 0,
        team: 0,
        card: &PlayerCard::default(),
    };
    let choose = |world: &mut WorldDelivery, scene: &mut PreparedScene, tick| {
        world.queue_game_message(tick, Duration::ZERO, pick_hero(OWNER, hero));
        assert_eq!(
            world
                .advance_input(scene, &viewer, RETAIL_QUANTA, 16)
                .unwrap(),
            (1, false)
        );
    };

    for (case, generation) in input.cases.into_iter().zip(expected) {
        let mut scene = PreparedScene::fixture_for_owner_admission(
            HeroCatalog::from_json(&heroes.to_string()).unwrap(),
        );
        let input_fault = Arc::new(AtomicBool::new(false));
        let startup_fault = Arc::new(AtomicBool::new(false));
        let reject = Arc::clone(&startup_fault);
        let fail = Arc::clone(&input_fault);
        let scripted = case.scripted;
        scene.owner_body_factory = Some(Arc::new(move |_, spawn, _| {
            if reject.load(Ordering::Relaxed) {
                return Err(io::Error::other("synthetic startup failure"));
            }
            Ok(scripted.then(|| {
                let runtime: Box<dyn OwnerBodyRuntime> = Box::new(synthetic_owner(
                    spawn.entity,
                    spawn.frame,
                    Arc::clone(&fail),
                ));
                runtime
            }))
        }));
        let mut world = WorldDelivery::new(OWNER);
        world.started = Some(FIRST_TICK);
        world.created.insert(OWNER);
        world.input.processed = Some(FIRST_TICK);
        world
            .controller
            .full_owner(FIRST_TICK, |_| Ok(crate::bits::BitWriter::new()))
            .unwrap();
        choose(&mut world, &mut scene, FIRST_TICK);
        let original = world.active_body_id().unwrap();
        assert_eq!(world.body_script.is_some(), scripted);

        let command = forward(FIRST_TICK + 1);
        input_fault.store(case.input_fault, Ordering::Relaxed);
        world.queue_commands(command.frame, &[command]);
        assert_eq!(
            world
                .advance_input(&mut scene, &viewer, RETAIL_QUANTA, 16)
                .unwrap(),
            (1, false)
        );
        assert_eq!(world.body_script.is_some(), scripted && !case.input_fault);
        assert_eq!(world.active_body_id(), Some(original));
        input_fault.store(false, Ordering::Relaxed);

        world.select_open = true;
        if case.input_fault {
            startup_fault.store(true, Ordering::Relaxed);
            choose(&mut world, &mut scene, command.frame);
            assert_eq!(world.active_body_id(), Some(original));
            assert!(world.select_open);
            startup_fault.store(false, Ordering::Relaxed);
        }

        choose(&mut world, &mut scene, command.frame);
        let selected = world.active_body_id().unwrap();
        assert_eq!(selected.generation, generation);
        assert_eq!(world.body.unwrap().hero, hero);
        assert_eq!(world.command_ack(command.frame + 1), command.frame);
        assert_eq!(world.body_script.is_some(), scripted);
        assert!(!world.select_open);
        assert_eq!(selected != original, case.input_fault);
        assert_eq!(
            scene.simulation().player_body(original).is_none(),
            case.input_fault
        );
        assert_eq!(
            scene
                .simulation()
                .player_body(selected)
                .unwrap()
                .runtime
                .is_some(),
            scripted
        );

        if case.input_fault {
            let next = forward(command.frame + 1);
            world.queue_commands(next.frame, &[next]);
            assert_eq!(
                world
                    .advance_input(&mut scene, &viewer, RETAIL_QUANTA, 16)
                    .unwrap(),
                (1, false)
            );
            assert_eq!(world.body_script.as_ref().unwrap().frame, next.frame);
        }
    }
}
