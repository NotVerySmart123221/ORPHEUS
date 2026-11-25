use super::*;
use crate::{
    collision_cache::{CollisionCache, Mesh, Triangle},
    game_bots::{BotCatalog, BotWalk},
    game_combat::RoomSimulation,
    mover_settings::MoverCatalog,
    script_graph::RoomSpatialNeeds,
};
use serde_json::{Value as Json, json};
use std::sync::Arc;

fn fixture() -> Json {
    serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/reinhardt-earthshatter-wave-v1.json"
    ))
    .unwrap()
}

fn floor(z_end: f32, wall: bool) -> CollisionScene {
    let mut vertices = vec![
        [-30.0, 0.0, -5.0],
        [-30.0, 0.0, z_end],
        [30.0, 0.0, z_end],
        [30.0, 0.0, -5.0],
    ];
    let mut faces = vec![[0, 1, 2], [0, 2, 3]];
    if wall {
        vertices.extend([
            [-30.0, 0.0, 6.0],
            [-30.0, 5.0, 6.0],
            [30.0, 5.0, 6.0],
            [30.0, 0.0, 6.0],
        ]);
        faces.extend([[4, 5, 6], [4, 6, 7]]);
    }
    let cache = CollisionCache {
        header:serde_json::from_value(json!({"meshes":[[vertices.len(),faces.len()]],"hulls":0,"models":0,"placements":0,"identifiers":[]})).unwrap(),
        meshes:vec![Mesh { vertices, triangles:faces.into_iter().map(|corners| Triangle{corners,wings:[-1;3],mask:0x18,surface:0}).collect() }],
        hulls:vec![], models:vec![], placements:vec![],
    };
    CollisionScene::movement_probe(&cache, &Default::default()).unwrap()
}

fn room(floor: CollisionScene) -> (RoomSimulation, Vec<u32>) {
    let positions = fixture()["input"]["positions"].as_array().unwrap().clone();
    let spawns: Vec<_> = positions
        .iter()
        .enumerate()
        .map(|(i, position)| {
            json!({
                "hero":if i == 2 {"0x04"} else {"0x01"},"team": if i == 0 || i == 7 {0} else {1},"position":position,
                "rotation":[0,0,0,1],"graphs":[]
            })
        })
        .collect();
    let bots = BotCatalog::from_json(
        &json!({
            "heroes":{"0x01":{"name":"Test","body":"0x02","health":200},
                      "0x04":{"name":"High health target","body":"0x02","health":600}},
            "maps":{"0x03":{"hints":{},"spawners":spawns}}
        })
        .to_string(),
    )
    .unwrap()
    .make(3, |_| BotWalk::default());
    let ids = bots.iter().map(|b| b.entity).collect();
    let mut room = RoomSimulation::new(bots).unwrap();
    room.enable_spatial_queries(false, Arc::new(floor), RoomSpatialNeeds::Queries);
    room.enable_spatial_actor_rays(Arc::new(MoverCatalog::default()));
    room.prepare_spatial_frame(1).unwrap();
    (room, ids)
}

fn wave() -> GroundWave {
    let f = fixture();
    let input = &f["input"];
    GroundWave {
        centre: [0.0; 3],
        sight_origin: [0.0, 2.0, 0.0],
        yaw: 0.0,
        aperture: input["angle"].as_f64().unwrap(),
        distance: input["distance"].as_f64().unwrap(),
        speed: input["speed"].as_f64().unwrap(),
        max_rise: 1.75,
        max_drop: 1.75,
        ground_range: 1.75,
    }
}

#[test]
fn travelling_wave_selects_grounded_opponents_at_the_authored_arrival_times() {
    let wave = wave();
    let expected = fixture()["expected"]["arrival_ms"]
        .as_array()
        .unwrap()
        .clone();
    for (ground, count) in [
        (floor(25.0, false), 3),
        (floor(7.0, false), 1),
        (floor(25.0, true), 1),
    ] {
        let (room, ids) = room(ground);
        let frame = &room.spatial_frame.as_ref().unwrap().1;
        for elapsed in [0, 99, 100, 299, 300, 499, 500, 900] {
            let n = expected
                .iter()
                .take(count)
                .filter(|t| t.as_u64().unwrap() <= elapsed)
                .count();
            assert_eq!(
                frame
                    .ground_wave_targets(ids[0], &wave, wave.reach(elapsed))
                    .unwrap(),
                ids[1..1 + n],
                "at {elapsed} ms"
            );
        }
    }
}
