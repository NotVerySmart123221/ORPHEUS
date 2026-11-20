use super::*;
use crate::script_init::TargetTagEffects;
use serde::Deserialize;
use std::collections::BTreeSet;

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
    query: Json,
    origin: [f64; 3],
    direction: [f64; 3],
    source: u32,
    candidates: Vec<(u32, [f64; 3], Option<u8>)>,
    dead: u32,
    excluded_by_tag: u32,
    excluded_tag: String,
}

#[derive(Deserialize)]
struct Expected {
    eligible: Vec<u32>,
    hit: (u32, f64, [f64; 3]),
    inside_hit: (u32, f64, [f64; 3]),
}

struct Host {
    values: Vars,
    candidates: Vec<SpatialCandidate>,
    facts: BTreeMap<u32, RoomEntityFacts>,
    query: Option<(f64, Vec<u32>)>,
    hit: Option<Option<SpatialActorRayHit>>,
}

impl Context for Host {
    fn variable(&mut self, variable: Variable) -> Value {
        self.values.variable(variable)
    }

    fn spatial_candidates(&mut self) -> Option<&[SpatialCandidate]> {
        Some(&self.candidates)
    }

    fn room_entities(&mut self) -> Option<&BTreeMap<u32, RoomEntityFacts>> {
        Some(&self.facts)
    }

    fn spatial_actor_ray_cast(
        &mut self,
        origin: [f64; 3],
        direction: [f64; 3],
        max_distance: f64,
        targets: &[u32],
    ) -> Option<Option<SpatialActorRayHit>> {
        assert_eq!(self.values.instance[&1], Value::Vector(origin));
        assert_eq!(direction, [0.0, 0.0, 1.0]);
        assert!(
            self.query
                .replace((max_distance, targets.to_vec()))
                .is_none()
        );
        self.hit
    }
}

#[test]
fn entity_ray_queries_frozen_candidates_and_distinguishes_an_unavailable_service() {
    let contract: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/statescript-entity-ray-cast-v1.json"
    )))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.case_id, "statescript-entity-ray-cast.synthetic.v1");
    assert_eq!(contract.evidence, "synthetic-rust");
    let Input {
        query,
        origin,
        direction,
        source,
        candidates,
        dead,
        excluded_by_tag,
        excluded_tag,
    } = contract.input;
    let Expected {
        eligible,
        hit,
        inside_hit,
    } = contract.expected;
    let facts = candidates
        .iter()
        .map(|&(entity, _, team)| {
            let mut tags = BTreeSet::new();
            if entity == excluded_by_tag {
                tags.insert(parse_guid(&excluded_tag).unwrap());
            }
            (
                entity,
                RoomEntityFacts {
                    team,
                    alive: entity != dead,
                    tags: TargetTagEffects::default().snapshot(Some(&tags)),
                },
            )
        })
        .collect();
    let mut host = Host {
        values: Vars {
            instance: BTreeMap::from([
                (1, Value::Vector(origin)),
                (2, Value::Vector(direction)),
                (3, Value::Entity(source)),
            ]),
            ..Vars::default()
        },
        candidates: candidates
            .into_iter()
            .map(|(entity, position, team)| SpatialCandidate {
                entity,
                position,
                team,
            })
            .collect(),
        facts,
        query: None,
        hit: None,
    };
    let length = query["max_distance"].as_f64().unwrap();
    for (entity, distance, point) in [hit, inside_hit] {
        host.hit = Some(Some(SpatialActorRayHit {
            entity,
            distance,
            normal: [0.0, 0.0, -1.0],
        }));
        assert_eq!(
            evaluate(&query, &mut host),
            Ok(Value::Array(vec![
                Value::Entity(entity),
                Value::Float(distance),
                Value::Vector(point),
            ]))
        );
        assert_eq!(host.query.take(), Some((length, eligible.clone())));
    }

    host.hit = Some(None);
    assert_eq!(evaluate(&query, &mut host), Ok(Value::Array(Vec::new())));
    assert_eq!(host.query.take(), Some((length, eligible.clone())));
    for hit in [
        None,
        Some(SpatialActorRayHit {
            entity: excluded_by_tag,
            distance: 2.0,
            normal: [0.0, 0.0, -1.0],
        }),
        Some(SpatialActorRayHit {
            entity: eligible[0],
            distance: -1.0,
            normal: [0.0, 0.0, -1.0],
        }),
        Some(SpatialActorRayHit {
            entity: eligible[0],
            distance: length + 1.0,
            normal: [0.0, 0.0, -1.0],
        }),
    ] {
        host.hit = hit.map(Some);
        assert_eq!(
            evaluate(&query, &mut host),
            Err(EvalError::UnavailableSpatialRayCast)
        );
        assert_eq!(host.query.take(), Some((length, eligible.clone())));
    }

    assert!(matches!(
        evaluate_with_budget(&query, &mut host, 1),
        Err(EvalError::BudgetExceeded { .. })
    ));
    assert_eq!(host.query, None);
    let known_tags = std::mem::take(&mut host.facts.get_mut(&eligible[0]).unwrap().tags);
    assert_eq!(
        evaluate(&query, &mut host),
        Err(EvalError::UnavailableRoomEntityFacts)
    );
    assert_eq!(host.query, None);
    host.facts.get_mut(&eligible[0]).unwrap().tags = known_tags;

    let all_candidates = std::mem::take(&mut host.candidates);
    host.candidates.extend(
        all_candidates
            .iter()
            .filter(|row| row.entity == source)
            .copied(),
    );
    host.hit = Some(None);
    assert_eq!(evaluate(&query, &mut host), Ok(Value::Array(Vec::new())));
    assert_eq!(host.query.take(), Some((length, Vec::new())));
    host.candidates = all_candidates;

    host.candidates
        .retain(|candidate| candidate.entity != source);
    assert_eq!(
        evaluate(&query, &mut host),
        Err(EvalError::UnavailableSpatialCandidates)
    );
    assert_eq!(host.query, None);
}
