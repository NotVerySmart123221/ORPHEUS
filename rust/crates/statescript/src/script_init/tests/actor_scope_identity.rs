use std::cell::Cell;

use super::*;
use crate::{script_expr::SpatialCandidate, script_runtime::frame_start};

const ROOT: u16 = 1;
const TARGET_VARIABLE: u32 = 7;
const COPIED_TARGET: u32 = 8;

struct IdentityWorld {
    target: u32,
    token: Option<u64>,
    identity_reads: Cell<usize>,
    candidates: Option<Vec<SpatialCandidate>>,
    frame: u32,
    pulses: Vec<(u32, Option<u64>, u32, f64)>,
}

impl IdentityWorld {
    fn candidates(&self) -> Vec<SpatialCandidate> {
        vec![SpatialCandidate {
            entity: self.target,
            position: [1.0, 0.0, 0.0],
            team: None,
        }]
    }
}

impl World for IdentityWorld {
    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }

    fn spatial_candidates(&self) -> Option<&[SpatialCandidate]> {
        self.candidates.as_deref()
    }

    fn room_actor_identity(&self, entity: u32) -> Option<u64> {
        self.identity_reads.set(self.identity_reads.get() + 1);
        (entity == self.target).then_some(self.token).flatten()
    }

    fn queue_health_change(&mut self, _: Site, target: u32, delta: f64) -> bool {
        self.pulses.push((self.frame, self.token, target, delta));
        true
    }
}

fn start(scoped: bool, known: bool) -> (Component, IdentityWorld, Json, u32) {
    let fixture: Json = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-target-children-v1.json"
    )))
    .unwrap();
    assert_eq!(fixture["case_id"], "script-target-children.synthetic.v1");
    assert_eq!(fixture["evidence"], "synthetic-rust");
    let input = fixture["input"]["actor_identity"].clone();
    let mut bundle = fixture["input"]["bundle"].clone();
    let root = u32::try_from(bundle["graphs"][0]["index"].as_u64().unwrap()).unwrap();
    let child = u32::try_from(bundle["graphs"][1]["index"].as_u64().unwrap()).unwrap();
    let tracker = &mut bundle["graphs"][0]["nodes"][0];
    tracker["server_only"] = json!(true);
    tracker["fields"]["targets"] = fixture["input"]["radius_expression"].clone();
    if scoped {
        tracker["fields"]["identity"] = json!("room_actor");
    }
    let graph = &mut bundle["graphs"][1];
    graph["items"] = json!(5);
    graph["nodes"][2]["fields"]["interval_frames"] = input["interval_frames"].clone();
    graph["nodes"][2]["fields"]["on_interval"] = plug(&[(4, "m_inPlug")], &[]);
    graph["nodes"][3]["fields"]["m_outPlug"] = plug(&[(4, "m_inPlug")], &[]);
    let mut pulse = node(
        4,
        "AuthoredIdentityPulse",
        None,
        json!({"target":var(TARGET_VARIABLE,false),"delta":-1,
            "m_outPlug":plug(&[],&[])}),
    );
    pulse["server_only"] = json!(true);
    pulse["action_behavior"] = json!("health_change");
    graph["nodes"].as_array_mut().unwrap().push(pulse);
    let target = u32::try_from(input["target"].as_u64().unwrap()).unwrap();
    let mut world = IdentityWorld {
        target,
        token: known.then(|| input["initial_token"].as_u64().unwrap()),
        identity_reads: Cell::new(0),
        candidates: None,
        frame: 0,
        pulses: Vec::new(),
    };
    world.candidates = Some(world.candidates());
    let component = Component::start(
        Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap()),
        847,
        Variables::from([(2, Value::Vector([0.0; 3]))]),
        vec![Root::listener(ROOT, root, Some(0))],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    (component, world, input, child)
}

fn frame(input: &Json, name: &str) -> u32 {
    u32::try_from(input[name].as_u64().unwrap()).unwrap()
}

fn advance(component: &mut Component, world: &mut IdentityWorld, frame: u32) {
    world.frame = frame;
    component.advance_frame(frame, world).unwrap();
}

fn child_at(component: &Component, graph: u32) -> Option<(u16, &Instance)> {
    component
        .snapshot()
        .unwrap()
        .instances
        .iter()
        .find_map(|(&id, instance)| (instance.graph == graph).then_some((id, instance)))
}

fn assert_fresh_child(
    component: &Component,
    graph: u32,
    target: u32,
    born: u32,
    interval: u32,
) -> u16 {
    assert_eq!(
        component
            .snapshot()
            .unwrap()
            .instances
            .values()
            .filter(|instance| instance.graph == graph)
            .count(),
        1,
        "one actor identity must own exactly one child"
    );
    let (id, instance) = child_at(component, graph).expect("known actor owns a child");
    assert_eq!(instance.variables[&TARGET_VARIABLE], Value::Entity(target));
    assert_eq!(instance.variables[&COPIED_TARGET], Value::Entity(target));
    assert!(instance.is_server_only());
    let pulse = &instance.states[&0];
    assert!(pulse.active);
    // The tracker samples at the first scheduler wake in this frame. With
    // no due work that can be the last millisecond of the frame.
    assert!(
        (frame_start(born)..frame_start(born + 1)).contains(&pulse.start_ms),
        "child started at {} ms outside frame {born}",
        pulse.start_ms
    );
    let due = frame_start(born + interval);
    assert!(
        component.snapshot().unwrap().pending_timers[&id]
            .iter()
            .any(|timer| timer.due_ms == due)
    );
    id
}

#[test]
fn room_actor_scopes_replace_children_before_stale_work_and_recover_after_unavailability() {
    let (mut component, mut world, input, child_graph) = start(true, true);
    let initial = frame(&input, "initial_frame");
    let retained = frame(&input, "retained_frame");
    let replaced = frame(&input, "replacement_frame");
    let interval = frame(&input, "interval_frames");
    let first_pulse = frame(&input, "first_pulse_frame");
    let unavailable = frame(&input, "unavailable_frame");
    let recovered = frame(&input, "recovery_frame");
    let message = input["child_message"].as_u64().unwrap();
    advance(&mut component, &mut world, initial);
    let original = assert_fresh_child(&component, child_graph, world.target, initial, interval);
    let original_start = child_at(&component, child_graph).unwrap().1.states[&0].start_ms;
    component
        .set_instance_variable(
            original,
            COPIED_TARGET,
            Value::Integer(input["stale_child_value"].as_i64().unwrap()),
            &mut world,
        )
        .unwrap();
    world.candidates = None;
    advance(&mut component, &mut world, retained);
    let (same, child) = child_at(&component, child_graph).unwrap();
    assert_eq!(same, original);
    assert_eq!(child.states[&0].start_ms, original_start);
    assert_eq!(
        child.variables[&COPIED_TARGET],
        Value::Integer(input["stale_child_value"].as_i64().unwrap())
    );
    assert!(world.pulses.is_empty());
    component
        .queue_frame_message(replaced, original, message, Variables::new())
        .unwrap();

    world.token = input["replacement_token"].as_u64();
    world.candidates = Some(world.candidates());
    advance(&mut component, &mut world, replaced);
    // Child IDs can be recycled: the fresh state clock and copied target prove replacement.
    let replacement = assert_fresh_child(&component, child_graph, world.target, replaced, interval);
    assert!(world.pulses.is_empty(), "retired actor's due work ran");
    advance(&mut component, &mut world, first_pulse - 1);
    assert!(world.pulses.is_empty(), "replacement pulse ran early");
    advance(&mut component, &mut world, first_pulse);
    assert_eq!(
        world.pulses,
        [(first_pulse, world.token, world.target, -1.0)]
    );
    component
        .queue_frame_message(unavailable, replacement, message, Variables::new())
        .unwrap();

    world.token = None;
    world.candidates = None;
    advance(&mut component, &mut world, unavailable);
    assert!(child_at(&component, child_graph).is_none());
    assert!(component.machine.queues.iter(replacement).next().is_none());
    assert!(component.snapshot().unwrap().pending_timers.is_empty());
    assert_eq!(
        world.pulses.len(),
        1,
        "unavailable identity kept a due child alive"
    );

    world.token = input["recovered_token"].as_u64();
    advance(
        &mut component,
        &mut world,
        frame(&input, "identity_only_frame"),
    );
    assert!(
        child_at(&component, child_graph).is_none(),
        "identity alone must not reuse an unavailable target sample"
    );
    world.candidates = Some(world.candidates());
    advance(&mut component, &mut world, recovered);
    assert_fresh_child(&component, child_graph, world.target, recovered, interval);
    let recovered_pulse = frame(&input, "recovery_pulse_frame");
    advance(&mut component, &mut world, recovered_pulse - 1);
    assert_eq!(world.pulses.len(), 1, "recovered pulse ran early");
    advance(&mut component, &mut world, recovered_pulse);
    assert_eq!(
        world.pulses,
        [
            (
                first_pulse,
                input["replacement_token"].as_u64(),
                world.target,
                -1.0
            ),
            (
                recovered_pulse,
                input["recovered_token"].as_u64(),
                world.target,
                -1.0
            ),
        ]
    );
    component.teardown(&mut world).unwrap();
    assert!(component.snapshot().unwrap().instances.is_empty());
    assert!(component.machine.queues.next_due().is_none());
    assert!(component.machine.actor_target_guards.is_empty());
}

#[test]
fn room_actor_scopes_wait_for_a_known_identity_and_a_new_sample() {
    let (mut component, mut world, input, child_graph) = start(true, false);
    let first = frame(&input, "initial_frame");
    advance(&mut component, &mut world, first);
    assert!(child_at(&component, child_graph).is_none());
    world.token = input["initial_token"].as_u64();
    component.run_now(&mut world).unwrap();
    assert!(child_at(&component, child_graph).is_none());
    let next = frame(&input, "retained_frame");
    advance(&mut component, &mut world, next);
    assert_fresh_child(
        &component,
        child_graph,
        world.target,
        next,
        frame(&input, "interval_frames"),
    );
    world.token = None;
    advance(
        &mut component,
        &mut world,
        frame(&input, "known_query_identity_loss_frame"),
    );
    assert!(child_at(&component, child_graph).is_none());
    assert!(component.snapshot().unwrap().pending_timers.is_empty());
    assert!(
        world.pulses.is_empty(),
        "a successful target query cannot authorize an unknown identity"
    );
    world.token = input["initial_token"].as_u64();
    let recovered = frame(&input, "known_query_identity_recovery_frame");
    advance(&mut component, &mut world, recovered);
    assert_fresh_child(
        &component,
        child_graph,
        world.target,
        recovered,
        frame(&input, "interval_frames"),
    );
}

#[test]
fn default_target_children_keep_wire_membership_without_querying_actor_identity() {
    let (mut component, mut world, input, child_graph) = start(false, true);
    let first = frame(&input, "initial_frame");
    advance(&mut component, &mut world, first);
    let original = assert_fresh_child(
        &component,
        child_graph,
        world.target,
        first,
        frame(&input, "interval_frames"),
    );
    let original_start = child_at(&component, child_graph).unwrap().1.states[&0].start_ms;
    world.token = input["replacement_token"].as_u64();
    let due = frame(&input, "replacement_frame");
    advance(&mut component, &mut world, due - 1);
    assert!(world.pulses.is_empty());
    advance(&mut component, &mut world, due);
    let (current, child) = child_at(&component, child_graph).unwrap();
    assert_eq!(current, original);
    assert_eq!(child.states[&0].start_ms, original_start);
    assert_eq!(world.pulses, [(due, world.token, world.target, -1.0)]);
    assert_eq!(world.identity_reads.get(), 0);
}
