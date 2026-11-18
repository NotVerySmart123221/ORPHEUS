use super::*;
use crate::script_wire::Value as ScriptValue;

fn rifle_state(body: &PlayerBodyState) -> (i64, Option<bool>) {
    let snapshot = body.runtime.as_ref().unwrap().snapshot().unwrap();
    assert!(snapshot.initialization_complete());
    // Resolve the authored rifle root independently of VM allocation order.
    let mut rifles = snapshot
        .instances
        .iter()
        .filter(|(_, instance)| instance.graph == 0x0254 && instance.parent.is_none());
    let (&id, rifle) = rifles.next().expect("stock Soldier rifle root");
    assert!(rifles.next().is_none(), "ambiguous stock rifle root");
    let value = |key| {
        snapshot
            .resolved_bindings
            .get(&(id, key))
            .or_else(|| rifle.variables.get(&key))
    };
    let Some(ScriptValue::Integer(ammo)) = value(53) else {
        panic!("stock rifle has no integer magazine value");
    };
    let reload = match value(51) {
        None | Some(ScriptValue::Null) => None,
        Some(ScriptValue::Bool(active)) => Some(*active),
        _ => panic!("unexpected stock rifle reload state"),
    };
    (*ammo, reload)
}

fn deliver_until(
    world: &mut WorldDelivery,
    scene: &mut PreparedScene,
    egress: &mut GameEgress<Arc<Record>>,
    tick: &mut u32,
    ready: impl Fn(&WorldDelivery) -> bool,
) {
    // Receipt is modelled only after the real egress selected the frame and
    // submitted it to the in-memory transport.
    for _ in 0..32 {
        world.present(scene, *tick).unwrap();
        if ready(world) {
            return;
        }
        let (_, sent) = transmit(
            world,
            Some(scene.simulation_mut()),
            egress,
            *tick,
            *tick - FIRST_TICK,
        );
        transport_outcome(world, egress, sent.sequence, true);
        *tick += 1;
    }
    panic!("owner delivery prerequisite was never reached");
}

#[test]
#[ignore = "requires the target-tagged 104319 Practice Range runtime"]
fn stock_practice_owner_observes_processed_commands_and_rifle_state() {
    let runtime = std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap();
    let assets = SceneAssets::load(runtime).unwrap();
    let mut scene = assets.prepare(GameMap::practice_range()).unwrap();
    assert!(scene.movement_ready());
    let viewer = PlayerAppearance {
        entity: OWNER,
        slot: 0,
        team: 0,
        card: &PlayerCard::default(),
    };
    let mut world = WorldDelivery::new(OWNER);
    let mut egress = GameEgress::default();
    let mut tick = FIRST_TICK;
    world.start(&scene, &viewer, tick).unwrap();
    assert_eq!(world.command_ack(tick), tick);
    assert_eq!(world.input.processed, None);
    world.queue_game_message(tick, Duration::ZERO, pick_hero(OWNER, SOLDIER));
    assert!(!world.has_input_work());

    deliver_until(&mut world, &mut scene, &mut egress, &mut tick, |world| {
        world.identity_ready(OWNER) && world.controller.latest_data() != 0
    });
    world.queue_game_message(tick, Duration::ZERO, pick_hero(OWNER, SOLDIER));
    assert_eq!(
        world
            .advance_input(&mut scene, &viewer, RETAIL_QUANTA, 1)
            .unwrap(),
        (1, false)
    );
    let body = scene.simulation().player_body_by_owner(OWNER).unwrap();
    let id = body.id;
    assert_eq!(body.hero, SOLDIER);
    assert_eq!(
        body.runtime.as_ref().unwrap().vitals().unwrap().total(),
        200.0
    );
    assert_eq!(rifle_state(body).0, 30);
    deliver_until(&mut world, &mut scene, &mut egress, &mut tick, |world| {
        world.identity_ready(id.entity) && world.bound == Some(id)
    });
    assert_eq!(world.input.processed, None);

    let start = tick;
    for elapsed in 1..=164 {
        tick = start + elapsed;
        let command = Command {
            frame: tick,
            yaw: 137,
            pitch: 23,
            buttons: match elapsed {
                40 => 1,
                70 => 0x400,
                _ => 0,
            },
            ..Command::default()
        };
        let previous = world.input.processed;
        world.queue_commands(tick, &[command]);
        assert_eq!(world.input.admitted, Some(tick));
        assert_eq!(world.input.processed, previous);
        let (_, leave) = world
            .advance_input(&mut scene, &viewer, RETAIL_QUANTA, 16)
            .unwrap();
        assert!(!leave);
        assert_eq!(world.input.processed, Some(tick));
        let body = scene.simulation().player_body(id).expect("same live body");
        let pose = body.mover.pose();
        assert_eq!(
            (pose.frame, pose.yaw, pose.pitch),
            (Some(tick), command.yaw, command.pitch)
        );
        let (ammo, reload) = rifle_state(body);
        assert_eq!(
            ammo,
            if (40..117).contains(&elapsed) { 29 } else { 30 },
            "tick {elapsed}"
        );
        if elapsed >= 70 {
            assert_eq!(reload, Some(elapsed < 163), "tick {elapsed}");
        } else {
            assert_ne!(reload, Some(true), "tick {elapsed}");
        }
    }
}
