use std::{collections::BTreeMap, sync::Arc};

use orpheus_statescript::{
    script_expr::RoomEntityFacts,
    script_graph::GraphStore,
    script_init::{Component, HostEvent, MAX_INIT_WORK, Site, World, player_roots},
    script_wire::{Value, Variables},
};
use serde::Deserialize;
use serde_json::{Value as Json, json};

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    case_id: String,
    evidence: String,
    input: Input,
    expected: Expected,
}

#[derive(Deserialize)]
struct Input {
    bundle: Json,
    actors: [Actor; 3],
    steps: Vec<Step>,
}

#[derive(Deserialize)]
struct Actor {
    entity: u32,
    body: String,
    team: u8,
}

#[derive(Deserialize)]
struct Step {
    frame: u32,
    event: Option<Event>,
}

#[derive(Clone, Copy, Deserialize)]
struct Event {
    source: Option<u32>,
    target: u32,
    delta: f64,
    killed: bool,
}

#[derive(Deserialize)]
struct Expected {
    graph: u32,
    meter_variable: u32,
    settings_variables: [u32; 4],
    settings: [Option<[f64; 4]>; 3],
    meters: Vec<[Option<f64>; 3]>,
}

#[derive(Default)]
struct MeterWorld {
    facts: BTreeMap<u32, RoomEntityFacts>,
    activity: BTreeMap<u32, bool>,
}

impl MeterWorld {
    fn add_actor(&mut self, actor: &Actor) {
        self.facts.insert(
            actor.entity,
            RoomEntityFacts {
                team: Some(actor.team),
                alive: true,
                tags: Default::default(),
            },
        );
    }
}

impl World for MeterWorld {
    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }

    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn room_entities(&self) -> Option<&BTreeMap<u32, RoomEntityFacts>> {
        Some(&self.facts)
    }

    fn body_active(&self, site: Site) -> Option<bool> {
        Some(*self.activity.get(&site.entity)? && self.facts.get(&site.entity)?.alive)
    }
}

fn meter_variables(component: &Component, graph: u32) -> Option<&Variables> {
    let snapshot = component.snapshot().unwrap();
    assert!(
        snapshot.diagnostics.is_empty(),
        "actor {}: {:?}",
        snapshot.entity,
        snapshot.diagnostics
    );
    let mut instances = snapshot
        .instances
        .values()
        .filter(|instance| instance.graph == graph);
    let variables = instances.next().map(|instance| &instance.variables);
    assert!(
        instances.next().is_none(),
        "duplicate meter on {}",
        snapshot.entity
    );
    variables
}

fn number(variables: &Variables, id: u32) -> f64 {
    match &variables[&id] {
        Value::Float(value) => *value,
        Value::Integer(value) => *value as f64,
        other => panic!("meter variable {id} contains {other:?}"),
    }
}

#[test]
fn authored_meter_applies_host_events_and_passive_gain_per_body_binding() {
    let fixture = meter_contract();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.case_id, "script-authored-meter.synthetic.v1");
    assert_eq!(fixture.evidence, "synthetic-rust");
    let input = fixture.input;
    let expected = fixture.expected;
    assert_eq!(input.steps.len(), expected.meters.len());
    let graphs =
        Arc::new(GraphStore::from_json(&serde_json::to_vec(&input.bundle).unwrap()).unwrap());
    assert!(graphs.uses_host_health_events());
    let mut world = MeterWorld::default();
    for actor in &input.actors {
        world.add_actor(actor);
    }
    let mut actors = input
        .actors
        .each_ref()
        .map(|actor| start_meter(&graphs, actor, 0, &mut world));
    for (actor, settings) in actors.iter().zip(expected.settings) {
        let variables = meter_variables(actor, expected.graph);
        assert_eq!(
            variables.map(|variables| expected.settings_variables.map(|id| number(variables, id))),
            settings
        );
    }
    let read_meters = |actors: &[Component; 3]| {
        actors.each_ref().map(|actor| {
            meter_variables(actor, expected.graph)
                .map(|variables| number(variables, expected.meter_variable))
        })
    };
    assert_eq!(
        read_meters(&actors),
        expected.settings.map(|settings| settings.map(|_| 0.0))
    );

    let mut previous_frame = 0;
    for (step, meters) in input.steps.into_iter().zip(expected.meters) {
        assert_eq!(step.frame, previous_frame + 1);
        if let Some(event) = step.event {
            let before = read_meters(&actors);
            for ((actor, spec), settings) in
                actors.iter_mut().zip(&input.actors).zip(expected.settings)
            {
                let participates = event.source == Some(spec.entity) || event.target == spec.entity;
                assert_eq!(
                    actor
                        .queue_host_event(HostEvent::HealthApplied {
                            source: event.source,
                            target: event.target,
                            delta: event.delta,
                            killed: event.killed,
                            hit_tags: None,
                        })
                        .unwrap(),
                    usize::from(settings.is_some() && participates && event.delta != 0.0),
                    "actor {}, frame {}",
                    spec.entity,
                    step.frame,
                );
                actor.run_now(&mut world).unwrap();
            }
            assert_eq!(read_meters(&actors), before, "before frame {}", step.frame);
        }
        for actor in &mut actors {
            actor.advance_frame(step.frame, &mut world).unwrap();
        }
        assert_eq!(read_meters(&actors), meters, "after frame {}", step.frame);
        previous_frame = step.frame;
    }
}

fn meter_contract() -> Contract {
    serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/script-authored-meter-v1.json"
    ))
    .unwrap()
}

fn start_meter(
    graphs: &Arc<GraphStore>,
    actor: &Actor,
    frame: u32,
    world: &mut MeterWorld,
) -> Component {
    let guid = u64::from_str_radix(actor.body.strip_prefix("0x").unwrap(), 16).unwrap();
    let roots = player_roots(&graphs.bodies()[&guid], graphs).unwrap();
    Component::start(
        Arc::clone(graphs),
        actor.entity,
        Variables::new(),
        roots,
        frame,
        world,
        MAX_INIT_WORK,
    )
    .unwrap()
}

#[derive(Deserialize)]
struct ActivityContract {
    schema_version: u8,
    case_id: String,
    evidence: String,
    input: ActivityInput,
    expected: ActivityExpected,
}

#[derive(Deserialize)]
struct ActivityInput {
    initial_active: bool,
    replacement_entity: u32,
    replacement_frame: u32,
    primary_steps: Vec<ActivityStep>,
    replacement_steps: Vec<ActivityStep>,
}

#[derive(Deserialize)]
struct ActivityStep {
    frame: u32,
    event: Option<Event>,
    active_after_enqueue: Option<bool>,
    #[serde(default)]
    death_after_enqueue: bool,
}

#[derive(Deserialize)]
struct ActivityExpected {
    primary_meters: Vec<f64>,
    replacement_meters: Vec<f64>,
}

fn add_body_activity_gates(bundle: &mut Json) {
    let graph = &mut bundle["graphs"][0];
    let nodes = graph["nodes"].as_array_mut().unwrap();
    for (upstream, plug) in [(2, "on_interval"), (4, "m_outPlug")] {
        let position = nodes.len();
        let output = std::mem::replace(
            &mut nodes[upstream]["fields"][plug],
            json!({"$": "Plug", "links": [[position, "m_inPlug"]]}),
        );
        nodes.push(json!({
            "pos": position,
            "class": "authored.meter_gate",
            "uid": position,
            "rid": position,
            "remote": -1,
            "state": null,
            "client_only": false,
            "server_only": true,
            "fields": {
                "m_condition": {"$": "BodyActive"},
                "m_truePlug": output,
                "m_falsePlug": {"$": "Plug", "links": []}
            }
        }));
    }
    let items = nodes.len();
    graph["items"] = json!(items);
}

fn assert_meter_timeline(
    actor: &mut Component,
    entity: u32,
    world: &mut MeterWorld,
    meter: &Expected,
    steps: &[ActivityStep],
    values: &[f64],
) {
    let value = |actor: &Component| {
        number(
            meter_variables(actor, meter.graph).unwrap(),
            meter.meter_variable,
        )
    };
    assert_eq!(value(actor), 0.0);
    assert_eq!(steps.len(), values.len());
    for (step, &expected) in steps.iter().zip(values) {
        if let Some(event) = step.event {
            let before = value(actor);
            let participates = event.source == Some(entity) || event.target == entity;
            assert_eq!(
                actor
                    .queue_host_event(HostEvent::HealthApplied {
                        source: event.source,
                        target: event.target,
                        delta: event.delta,
                        killed: event.killed,
                        hit_tags: None,
                    })
                    .unwrap(),
                usize::from(participates)
            );
            actor.run_now(world).unwrap();
            assert_eq!(value(actor), before, "before frame {}", step.frame);
        }
        if let Some(active) = step.active_after_enqueue {
            world.activity.insert(entity, active);
        }
        if step.death_after_enqueue {
            actor.set_owner_dead(true);
            world.facts.get_mut(&entity).unwrap().alive = false;
        }
        actor.advance_frame(step.frame, world).unwrap();
        assert_eq!(value(actor), expected, "after frame {}", step.frame);
    }
}

#[test]
fn authored_meter_reads_body_activity_at_dispatch_for_each_body() {
    let fixture: ActivityContract = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/script-meter-body-activity-v1.json"
    ))
    .unwrap();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.case_id, "script-meter-body-activity.synthetic.v1");
    assert_eq!(fixture.evidence, "synthetic-rust");

    let mut meter = meter_contract();
    add_body_activity_gates(&mut meter.input.bundle);
    let graphs =
        Arc::new(GraphStore::from_json(&serde_json::to_vec(&meter.input.bundle).unwrap()).unwrap());
    let primary = &meter.input.actors[0];
    let mut world = MeterWorld::default();
    for actor in &meter.input.actors {
        world.add_actor(actor);
    }
    world
        .activity
        .insert(primary.entity, fixture.input.initial_active);
    let mut actor = start_meter(&graphs, primary, 0, &mut world);
    assert_meter_timeline(
        &mut actor,
        primary.entity,
        &mut world,
        &meter.expected,
        &fixture.input.primary_steps,
        &fixture.expected.primary_meters,
    );

    let replacement = Actor {
        entity: fixture.input.replacement_entity,
        body: primary.body.clone(),
        team: primary.team,
    };
    world.add_actor(&replacement);
    world
        .activity
        .insert(replacement.entity, fixture.input.initial_active);
    let mut actor = start_meter(
        &graphs,
        &replacement,
        fixture.input.replacement_frame,
        &mut world,
    );
    assert_meter_timeline(
        &mut actor,
        replacement.entity,
        &mut world,
        &meter.expected,
        &fixture.input.replacement_steps,
        &fixture.expected.replacement_meters,
    );
}
