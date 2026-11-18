use super::*;
use crate::script_expr::{Scope, Variable, dynamic, truthy};
use crate::script_graph::Graph;
use crate::script_wire::Value as ScriptValue;

fn declared_reload_flag(graph: &Graph) -> bool {
    let reload = Variable {
        scope: Scope::Instance,
        id: 51,
    };
    let Some(switch) = graph
        .node(2)
        .filter(|n| n.class == "STUStatescriptStateBooleanSwitch")
    else {
        return false;
    };
    let Some(write) = graph
        .node(5)
        .filter(|n| n.class == "STUStatescriptActionSetVar")
    else {
        return false;
    };
    let reads = switch
        .fields
        .pointer("/m_condition/m_expression/m_dynamicVars")
        .and_then(serde_json::Value::as_array);
    reads.is_some_and(|vars| vars.iter().any(|v| dynamic(v) == Ok(Some(reload))))
        && write
            .fields
            .get("m_out_Var")
            .is_some_and(|v| dynamic(v) == Ok(Some(reload)))
}

pub(crate) fn rifle_state(body: &PlayerBodyState) -> (i64, Option<bool>) {
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
    // Machine::value falls back to shapes::stored for an unbound local. The
    // native store returns Null before the first write. Establish that the
    // stock eligibility switch and an authored writer actually use this flag
    // before applying that lookup to the otherwise sparse snapshot.
    let graph = body
        .runtime
        .as_ref()
        .unwrap()
        .encoder()
        .graphs
        .graph(rifle.graph)
        .unwrap();
    let reload = if declared_reload_flag(graph)
        && snapshot.bindings.get(&(id, 51)).is_none_or(Vec::is_empty)
    {
        match rifle.variables.get(&51).unwrap_or(&ScriptValue::Null) {
            v @ (ScriptValue::Null | ScriptValue::Bool(_)) => Some(truthy(v)),
            _ => None,
        }
    } else {
        None
    };
    (*ammo, reload)
}

fn deliver_until(
    world: &mut WorldDelivery,
    scene: &mut PreparedScene,
    egress: &mut GameEgress<Arc<Record>>,
    tick: &mut u32,
    observed: &mut impl FnMut(&DeliveryReport<Arc<Record>>, Option<u32>),
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
        let receipt = egress.acknowledge(sent.sequence + 1, 1);
        observed(&receipt, world.spawned_body());
        world.acknowledge(receipt, Duration::ZERO);
        *tick += 1;
    }
    panic!("owner delivery prerequisite was never reached");
}

pub(crate) fn processed(world: &WorldDelivery) -> Option<u32> {
    world.input.processed
}

pub(crate) struct PreparedOwner {
    pub(crate) delivery_tick: u32,
    pub(crate) component_start_frame: u32,
}

pub(crate) fn prepare_owner(
    scene: &mut PreparedScene,
    world: &mut WorldDelivery,
    egress: &mut GameEgress<Arc<Record>>,
    mut observed: impl FnMut(&DeliveryReport<Arc<Record>>, Option<u32>),
) -> PreparedOwner {
    let viewer = PlayerAppearance {
        entity: OWNER,
        slot: 0,
        team: 0,
        card: &PlayerCard::default(),
    };
    let mut tick = FIRST_TICK;
    world.start(scene, &viewer, tick).unwrap();
    assert_eq!(world.command_ack(tick), tick);
    assert_eq!(world.input.processed, None);
    world.queue_game_message(tick, Duration::ZERO, pick_hero(OWNER, SOLDIER));
    assert!(!world.has_input_work());

    deliver_until(world, scene, egress, &mut tick, &mut observed, |world| {
        world.identity_ready(OWNER) && world.controller.latest_data() != 0
    });
    world.queue_game_message(tick, Duration::ZERO, pick_hero(OWNER, SOLDIER));
    assert_eq!(
        world
            .advance_input(scene, &viewer, RETAIL_QUANTA, 1)
            .unwrap(),
        (1, false)
    );
    let body = scene.simulation().player_body_by_owner(OWNER).unwrap();
    let id = body.id;
    let component_start_frame = world.body_script.as_ref().unwrap().frame;
    assert_eq!(component_start_frame, tick);
    assert_eq!(body.hero, SOLDIER);
    assert_eq!(
        body.runtime.as_ref().unwrap().vitals().unwrap().total(),
        200.0
    );
    assert_eq!(rifle_state(body).0, 30);
    deliver_until(world, scene, egress, &mut tick, &mut observed, |world| {
        world.identity_ready(id.entity) && world.bound == Some(id)
    });
    assert_eq!(world.input.processed, None);

    PreparedOwner {
        delivery_tick: tick,
        component_start_frame,
    }
}

#[test]
#[ignore = "requires the target-tagged 104319 Practice Range runtime"]
fn stock_practice_owner_observes_processed_commands_and_rifle_state() {
    let runtime = std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap();
    let assets = SceneAssets::load(runtime).unwrap();
    let mut scene = assets.prepare(GameMap::practice_range()).unwrap();
    assert!(scene.movement_ready());
    let mut world = WorldDelivery::new(OWNER);
    let mut egress = GameEgress::default();
    let mut tick = prepare_owner(&mut scene, &mut world, &mut egress, |_, _| {}).delivery_tick;
    let viewer = PlayerAppearance {
        entity: OWNER,
        slot: 0,
        team: 0,
        card: &PlayerCard::default(),
    };
    let id = scene.simulation().player_body_by_owner(OWNER).unwrap().id;
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
