use super::*;

const SOURCE: u16 = 1;
const IDLE: u16 = 3;
const LOCK: u16 = 9;
const LISTENER: u16 = 10;
const HELD: u16 = 11;
const TERMINATOR: u16 = 12;
const IDLE_STATE: u16 = 2;
const LISTENER_STATE: u16 = 6;
const HELD_STATE: u16 = 7;

#[derive(Deserialize)]
pub(super) struct HeldBranch {
    button: u8,
    command_mask: u32,
    press_frame: u32,
    sample_frame: u32,
    lock_frames: u32,
    cases: Vec<Case>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Retirement {
    None,
    BeforeBranch,
    BeforeChild,
    Listener,
}

#[derive(Deserialize)]
struct Case {
    retirement: Retirement,
    idle_direction: Option<usize>,
    held_after_unlock: bool,
}

fn branch_graph(input: &Input, retirement: Retirement) -> Json {
    let mut model = deferred_facing_graph(input, "on_sample", false);
    let fields = &input.held_branch;
    let authored_state = |position, name, slot, behavior, fields| {
        let mut state = node(position, name, Some(slot), fields);
        state["state_behavior"] = json!(behavior);
        state["server_only"] = json!(true);
        state
    };
    model["items"] = json!(13);
    model["states"] = json!([1, 2, IDLE, 4, 7, LOCK, LISTENER, HELD, TERMINATOR]);
    let nodes = model["nodes"].as_array_mut().unwrap();
    let mut sample_route = vec![(LOCK, "m_beginPlug")];
    match retirement {
        Retirement::None => (),
        Retirement::BeforeBranch => sample_route.push((SOURCE, ABORT_PLUG)),
        Retirement::BeforeChild => sample_route.push((TERMINATOR, "m_beginPlug")),
        Retirement::Listener => sample_route.push((LISTENER, ABORT_PLUG)),
    }
    nodes[usize::from(SOURCE)]["fields"]["on_sample"] = plug(&sample_route, &[]);
    nodes[0]["fields"]["m_outPlug"] = plug(&[(LISTENER, "m_beginPlug")], &[]);
    nodes[5]["fields"]["m_outPlug"] = plug(&[(SOURCE, "m_beginPlug"), (2, "m_beginPlug")], &[]);
    nodes.extend([
        authored_state(
            LOCK,
            "AuthoredSampleButtonLock",
            5,
            "button_lock",
            json!({"button": fields.button, "duration_frames": fields.lock_frames}),
        ),
        authored_state(
            LISTENER,
            "AuthoredSampleHeldInput",
            LISTENER_STATE,
            "held_input",
            json!({"button": fields.button, "held_states": [HELD_STATE], "idle_states": [IDLE_STATE]}),
        ),
        authored_state(
            HELD,
            "AuthoredSampleHeldScale",
            HELD_STATE,
            "movement_scale",
            json!({"factor": 0.5}),
        ),
        authored_state(
            TERMINATOR,
            "AuthoredSampleTerminator",
            8,
            "plain",
            json!({"m_onBeginPlug": plug(&[(SOURCE, ABORT_PLUG)], &[])}),
        ),
    ]);
    model
}

#[test]
fn committed_held_input_branches_keep_live_samples_and_finish_after_sample_retirement() {
    let input = fixture().input;
    let fields = &input.held_branch;
    let rays = &input.deferred_facing;
    let is_active = |component: &Component, raw| {
        component.snapshot().unwrap().instances[&1]
            .states
            .get(&raw)
            .is_some_and(|state| state.active)
    };
    for case in &fields.cases {
        let (mut component, mut host) = start_deferred_model(
            &input,
            branch_graph(&input, case.retirement),
            Variables::from([(
                OUTPUT,
                deferred_ray_value(rays.ordinary_direction, rays.distance),
            )]),
        );
        assert!(is_active(&component, IDLE_STATE));
        component
            .advance_command(
                Command {
                    frame: fields.press_frame,
                    buttons: fields.command_mask,
                    ..Command::default()
                },
                &mut host,
            )
            .unwrap();
        assert!(is_active(&component, HELD_STATE));
        assert!(!is_active(&component, IDLE_STATE));
        assert_eq!(
            component
                .queue_message(1, rays.ordinary_message, Variables::new())
                .unwrap(),
            1
        );
        component.run_now(&mut host).unwrap();
        component
            .advance_frame(fields.sample_frame, &mut host)
            .unwrap();

        assert_eq!(host.queries.get(), 2);
        assert!(!is_active(&component, HELD_STATE));
        assert_eq!(
            is_active(&component, LISTENER_STATE),
            case.held_after_unlock
        );
        assert_eq!(
            is_active(&component, IDLE_STATE),
            case.idle_direction.is_some()
        );
        let idle_facing = host.active.iter().find(|(site, _)| site.node == Some(IDLE));
        assert_eq!(
            idle_facing.map(|(_, claim)| claim.direction),
            case.idle_direction
                .map(|direction| rays.directions[direction]),
            "source retirement: {:?}",
            case.retirement
        );
        assert_eq!(
            component.snapshot().unwrap().variables[&OUTPUT],
            deferred_ray_value(rays.directions[1], rays.distance)
        );

        // Retire both observers before the lock's timer opens the held stage again.
        component.debug_end_state(1, 0, &mut host).unwrap();
        component.debug_end_state(1, 1, &mut host).unwrap();
        component
            .advance_frame(fields.sample_frame + fields.lock_frames, &mut host)
            .unwrap();
        assert!(!is_active(&component, IDLE_STATE));
        assert_eq!(is_active(&component, HELD_STATE), case.held_after_unlock);
        assert!(host.active.iter().all(|(site, _)| site.node != Some(IDLE)));
        component.teardown(&mut host).unwrap();
        assert!(host.active.is_empty());
    }
}

#[test]
fn starting_a_listener_keeps_its_idle_branch_after_the_sample_ends() {
    let input = fixture().input;
    let fields = &input.held_branch;
    let rays = &input.deferred_facing;
    let mut model = branch_graph(&input, Retirement::None);
    model["nodes"][0]["fields"]["m_outPlug"] = plug(&[], &[]);
    model["nodes"][usize::from(SOURCE)]["fields"]["on_sample"] =
        plug(&[(LISTENER, "m_beginPlug"), (SOURCE, ABORT_PLUG)], &[]);
    let (mut component, mut host) = start_deferred_model(&input, model, Variables::new());
    component
        .advance_frame(fields.press_frame, &mut host)
        .unwrap();
    component
        .queue_message(1, rays.ordinary_message, Variables::new())
        .unwrap();
    component.run_now(&mut host).unwrap();
    component
        .advance_frame(fields.sample_frame, &mut host)
        .unwrap();

    let states = &component.snapshot().unwrap().instances[&1].states;
    assert!(!states[&0].active);
    assert!(states[&LISTENER_STATE].active);
    assert_eq!(
        states[&LISTENER_STATE].kind,
        StateKind::LogicalButton { down: false }
    );
    assert!(states[&IDLE_STATE].active);
    assert_eq!(
        host.active
            .iter()
            .find(|(site, _)| site.node == Some(IDLE))
            .map(|(_, claim)| claim.direction),
        Some(rays.directions[1])
    );
    component.teardown(&mut host).unwrap();
    assert!(host.active.is_empty());
}
