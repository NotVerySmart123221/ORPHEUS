use std::{collections::BTreeMap, sync::Arc};

use orpheus_statescript::{
    script_expr::RoomEntityFacts,
    script_graph::GraphStore,
    script_init::{Component, HostEvent, MAX_INIT_WORK, World, player_roots},
    script_wire::{Value, Variables},
};
use serde::Deserialize;
use serde_json::Value as Json;

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

struct MeterWorld(BTreeMap<u32, RoomEntityFacts>);

impl World for MeterWorld {
    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }

    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn room_entities(&self) -> Option<&BTreeMap<u32, RoomEntityFacts>> {
        Some(&self.0)
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
    let fixture: Contract = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/script-authored-meter-v1.json"
    ))
    .unwrap();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.case_id, "script-authored-meter.synthetic.v1");
    assert_eq!(fixture.evidence, "synthetic-rust");
    let input = fixture.input;
    let expected = fixture.expected;
    assert_eq!(input.steps.len(), expected.meters.len());
    let graphs =
        Arc::new(GraphStore::from_json(&serde_json::to_vec(&input.bundle).unwrap()).unwrap());
    assert!(graphs.uses_host_health_events());
    let mut world = MeterWorld(
        input
            .actors
            .iter()
            .map(|actor| {
                (
                    actor.entity,
                    RoomEntityFacts {
                        team: Some(actor.team),
                        alive: true,
                        tags: Default::default(),
                    },
                )
            })
            .collect(),
    );
    let mut actors = input.actors.each_ref().map(|actor| {
        let guid = u64::from_str_radix(actor.body.strip_prefix("0x").unwrap(), 16).unwrap();
        let roots = player_roots(&graphs.bodies()[&guid], &graphs).unwrap();
        Component::start(
            Arc::clone(&graphs),
            actor.entity,
            Variables::new(),
            roots,
            0,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap()
    });
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
