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
    ready: impl Fn(&WorldDelivery) -> bool,
) -> bool {
    // Receipt is modelled only after the real egress selected the frame and
    // submitted it to the in-memory transport.
    for _ in 0..32 {
        world.present(scene, *tick).unwrap();
        if ready(world) {
            return true;
        }
        let (_, sent) = transmit(
            world,
            Some(scene.simulation_mut()),
            egress,
            *tick,
            *tick - FIRST_TICK,
        );
        let receipt = egress.acknowledge(sent.sequence + 1, 1);
        world.acknowledge(receipt, Duration::ZERO);
        *tick += 1;
    }
    ready(world)
}

pub(crate) fn processed(world: &WorldDelivery) -> Option<u32> {
    world.input.processed
}

pub(crate) fn controller_acked(world: &WorldDelivery) -> bool {
    world.identity_ready(world.owner()) && world.controller.delivered() != 0
}

pub(crate) fn possession_acked(world: &WorldDelivery) -> bool {
    world.bound.is_some() && world.bound == world.active_body_id()
}

pub(crate) fn body_script_frame(world: &WorldDelivery) -> Option<u32> {
    world.body_script.as_ref().map(|script| script.frame)
}

fn prepare_owner(
    scene: &mut PreparedScene,
    world: &mut WorldDelivery,
    egress: &mut GameEgress<Arc<Record>>,
) -> u32 {
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

    assert!(deliver_until(world, scene, egress, &mut tick, |world| {
        world.identity_ready(OWNER) && world.controller.latest_data() != 0
    }));
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
    assert!(deliver_until(world, scene, egress, &mut tick, |world| {
        world.identity_ready(id.entity) && world.bound == Some(id)
    }));
    assert_eq!(world.input.processed, None);

    tick
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
    let mut tick = prepare_owner(&mut scene, &mut world, &mut egress);
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

#[derive(Default)]
struct InputObservation {
    admitted: bool,
    owner_full_ack: bool,
    owner_delta_ack: bool,
    completed_frames: u32,
    hitscan: usize,
    launches: usize,
}

fn observe_prepared_actor_input(
    scene: &mut PreparedScene,
    hero: u64,
    observation: &mut InputObservation,
) -> Result<(), (&'static str, u32, &'static str)> {
    use crate::commands::{FIRE, SECONDARY};
    use crate::game_instance::TeamMemberCounts;

    let viewer = PlayerAppearance {
        entity: OWNER,
        slot: 0,
        team: 0,
        card: &PlayerCard::default(),
    };
    let roster = TeamMemberCounts::from_teams(scene.map().free_for_all, [viewer.team]);
    scene.simulation_mut().set_team_member_counts(roster);
    let mut world = WorldDelivery::new(OWNER);
    let mut egress = GameEgress::default();
    let mut tick = FIRST_TICK;
    world.start(scene, &viewer, tick).unwrap();
    if !deliver_until(&mut world, scene, &mut egress, &mut tick, controller_acked) {
        return Err(("controller", tick, "pending acknowledgement"));
    }
    scene.simulation_mut().prepare_spatial_frame(tick).unwrap();
    world.queue_game_message(tick, Duration::ZERO, pick_hero(OWNER, hero));
    let work = world
        .advance_input(scene, &viewer, RETAIL_QUANTA, 16)
        .map_err(|_| ("selection", tick, "input error"))?;
    if work != (1, false) {
        return Err(("selection", tick, "request did not complete"));
    }
    let Some(id) = world.active_body_id() else {
        return Err(("admission", tick, "rejected"));
    };
    let Some(actor) = scene.simulation().player_body(id) else {
        return Err(("admission", tick, "missing committed body"));
    };
    if actor.hero != hero || actor.runtime.is_none() || !world.combat_body_active() {
        return Err(("admission", tick, "missing selected runtime"));
    }
    observation.admitted = true;
    let spawn_pose = actor.mover.pose();

    let owner_baseline = |world: &WorldDelivery| {
        world
            .body_script
            .as_ref()
            .and_then(|script| script.stream.base())
            .map(|(last, _)| last)
    };
    let delivered = deliver_until(&mut world, scene, &mut egress, &mut tick, |world| {
        possession_acked(world) && owner_baseline(world).is_some()
    });
    let owner_full = owner_baseline(&world);
    observation.owner_full_ack = owner_full.is_some();
    let Some(owner_full) = owner_full.filter(|_| delivered) else {
        return Err((
            "possession",
            tick,
            "missing owner baseline or body acknowledgement",
        ));
    };

    for (phase, frames, forward, buttons) in [
        ("idle", 4, 0, 0),
        ("movement", 8, 127, 0),
        ("primary", 24, 0, FIRE),
        ("primary release", 8, 0, 0),
        ("secondary", 24, 0, SECONDARY),
        ("settle", 32, 0, 0),
    ] {
        for _ in 0..frames {
            tick += 1;
            scene.simulation_mut().prepare_spatial_frame(tick).unwrap();
            let command = Command {
                frame: tick,
                forward,
                yaw: spawn_pose.yaw,
                pitch: spawn_pose.pitch,
                buttons,
                ..Command::default()
            };
            world.queue_commands(tick, &[command]);
            let (_, leave) = world
                .advance_input(scene, &viewer, RETAIL_QUANTA, 16)
                .map_err(|_| (phase, tick, "input error"))?;
            if leave || world.input.processed != Some(tick) {
                return Err((phase, tick, "command not processed"));
            }
            let actor = scene
                .simulation()
                .player_body(id)
                .filter(|actor| actor.runtime.is_some() && actor.frame == tick)
                .filter(|actor| actor.mover.pose().frame == Some(tick));
            if actor.is_none() || body_script_frame(&world) != Some(tick) {
                return Err((phase, tick, "runtime or posed frame lost after input"));
            }
            let simulation = scene.simulation_mut();
            observation.hitscan += world.take_hitscan(simulation).len();
            observation.launches += simulation
                .take_player_projectile_launches(id)
                .map_or(0, |launches| launches.len());
            let (_, sent) = transmit(
                &mut world,
                Some(simulation),
                &mut egress,
                tick,
                tick - FIRST_TICK,
            );
            transport_outcome(&mut world, &mut egress, sent.sequence, true);
            if world.active_body_id() != Some(id)
                || body_script_frame(&world) != Some(tick)
                || scene
                    .simulation()
                    .player_body(id)
                    .is_none_or(|actor| actor.runtime.is_none())
            {
                return Err((phase, tick, "runtime lost during owner egress"));
            }
            observation.owner_delta_ack |=
                owner_baseline(&world).is_some_and(|last| last > owner_full);
            observation.completed_frames += 1;
        }
    }
    if !observation.owner_delta_ack {
        return Err(("owner delta", tick, "pending acknowledgement"));
    }
    Ok(())
}

#[test]
#[ignore = "requires a prepared, tagged 104319 Practice Range runtime"]
fn prepared_practice_catalog_observes_input_survival_and_owner_egress() {
    let runtime = std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap();
    let assets = SceneAssets::load(runtime).unwrap();
    let mut admitted = 0;
    let mut completed = 0;
    let mut total = 0;
    let mut soldier_passed = false;
    for hero in assets.heroes().heroes() {
        total += 1;
        let mut scene = assets.prepare(GameMap::practice_range()).unwrap();
        let mut observation = InputObservation::default();
        let result = observe_prepared_actor_input(&mut scene, hero.guid, &mut observation);
        admitted += usize::from(observation.admitted);
        completed += usize::from(result.is_ok());
        if hero.guid == SOLDIER {
            soldier_passed = result.is_ok() && observation.hitscan > 0 && observation.launches > 0;
        }
        let outcome = match result {
            Ok(()) => "completed".to_owned(),
            Err((stage, frame, reason)) => format!("{stage} at {frame} ({reason})"),
        };
        eprintln!(
            "{}: {outcome}, admitted={}, frames={}, owner_full_ack={}, owner_delta_ack={}, hitscan={}, launches={}",
            hero.name,
            observation.admitted,
            observation.completed_frames,
            observation.owner_full_ack,
            observation.owner_delta_ack,
            observation.hitscan,
            observation.launches,
        );
    }
    eprintln!(
        "prepared Practice Range input survival: {admitted}/{total} admitted, {completed}/{admitted} completed"
    );
    assert!(admitted >= 11, "catalog admission regressed");
    assert!(
        soldier_passed,
        "expected both weapon outputs from the reference control"
    );
}
