use std::sync::Arc;

use super::*;
use crate::{
    collision_cache::{CollisionCache, Mesh, Triangle},
    collision_scene::CollisionScene,
    contact_world::ContactWorld,
    game_combat::{PlayerBodyState, RoomSimulation},
    game_replication::BodyId,
    match_scene::team_bit,
    mover::Mover,
    mover_settings::{MoverCatalog, MoverSettings},
    script_graph::RoomSpatialNeeds,
};

#[test]
fn view_query_ranks_visible_actors_and_rejects_unavailable_profiles() {
    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/script-view-targets-v1.json"
    ))
    .unwrap();
    let data = &fixture["room"];
    let ground = CollisionScene::movement_probe(
        &CollisionCache {
            header: serde_json::from_str(
                r#"{"meshes":[[4,2]],"hulls":0,"models":0,"placements":0,"identifiers":[]}"#,
            )
            .unwrap(),
            meshes: vec![Mesh {
                vertices: vec![
                    [-0.4, -1.0, 3.0],
                    [0.4, -1.0, 3.0],
                    [0.4, 4.0, 3.0],
                    [-0.4, 4.0, 3.0],
                ],
                triangles: [[0, 2, 1], [0, 3, 2]]
                    .into_iter()
                    .map(|corners| Triangle {
                        corners,
                        wings: [-1; 3],
                        mask: 1,
                        surface: 0,
                    })
                    .collect(),
            }],
            hulls: Vec::new(),
            models: Vec::new(),
            placements: Vec::new(),
        },
        &Default::default(),
    )
    .unwrap();
    let mut room = RoomSimulation::new(Vec::new()).unwrap();
    room.enable_spatial_queries(false, Arc::new(ground), RoomSpatialNeeds::Queries);
    room.enable_spatial_actor_rays(Arc::new(MoverCatalog::default()));
    let add_actor = |room: &mut RoomSimulation, slot, entry: &Json| {
        let (entity, team, position): (u32, u8, [f64; 3]) =
            serde_json::from_value(entry.clone()).unwrap();
        let mover = Mover::new(
            MoverSettings::default(),
            Arc::new(ContactWorld::new(Vec::new()).unwrap()),
            position,
            0,
            0x18,
        )
        .unwrap();
        room.spawn_player_body(PlayerBodyState::new(
            BodyId {
                entity,
                generation: 1,
            },
            entity + 1000,
            slot,
            team,
            1,
            mover,
            None,
            100,
            None,
        ))
        .unwrap();
    };
    for (slot, entry) in data["positions"].as_array().unwrap().iter().enumerate() {
        add_actor(&mut room, slot as u8, entry);
    }
    let source = u32::try_from(data["source"].as_u64().unwrap()).unwrap();
    let mut query = ViewTargetQuery {
        source,
        offset: [0.0, 1.0, 0.0],
        eye_heading: None,
        range: 23.0,
        aperture: 34.0,
        limit: 8,
        profile: 0x0690_0000_0000_ff11,
        masks: [1, team_bit(0).unwrap(), 0],
        ranking: &data["ranking"],
        sight: None,
    };
    let frame = room.capture_spatial_frame(100).unwrap().unwrap();
    assert_eq!(frame.view_targets(&query), None);
    let sample = |static_mask, block_actor_flags| ViewSightProfile {
        static_mask,
        block_actor_flags,
        samples: vec![0.5],
    };
    let unobstructed = sample(2, 0);
    let wall = sample(1, 0);
    let actor = sample(2, team_bit(1).unwrap());
    let listed = |key: &str| serde_json::from_value::<Vec<u32>>(data[key].clone()).unwrap();
    query.sight = Some(&unobstructed);
    assert_eq!(frame.view_targets(&query), Some(listed("unblocked_order")));
    query.limit = 1;
    assert_eq!(
        frame.view_targets(&query),
        Some(vec![listed("unblocked_order")[0]])
    );
    query.limit = 8;
    query.sight = Some(&wall);
    assert_eq!(frame.view_targets(&query), Some(listed("blocked_order")));
    add_actor(&mut room, 6, &data["blocker"]);
    let frame = room.capture_spatial_frame(101).unwrap().unwrap();
    query.sight = Some(&actor);
    assert_eq!(frame.view_targets(&query), Some(listed("blocked_order")));

    query.sight = None;
    query.eye_heading = data["explicit_eye_heading"].as_f64();
    query.profile = 0;
    assert_eq!(
        frame.view_targets(&query),
        Some(listed("explicit_eye_order"))
    );
}
