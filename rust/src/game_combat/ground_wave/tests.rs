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
fn charge_guard_enclosure_uses_capsules_and_excludes_its_owner_and_allies() {
    let (room, ids) = room(floor(25.0, false));
    let frame = &room.spatial_frame.as_ref().unwrap().1;
    let near = frame
        .hostile_sphere_candidates(ids[0], [0.0, 0.5, 0.0], 4.0)
        .unwrap();
    assert_eq!(near, vec![ids[1], ids[6]]);
    assert!(
        frame
            .hostile_sphere_candidates(ids[0], [0.0; 3], 1.0)
            .unwrap()
            .is_empty()
    );
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

/// Exercise the stock ground probe, travelling wave, and both authored health placements.
/// The independent close-contact selection and presentation subgraphs are outside this probe.
#[test]
#[ignore = "requires the prepared 104319 Practice Range StateScript pack"]
fn prepared_earthshatter_wave_settles_authored_damage_as_it_reaches_each_bot() {
    use crate::{
        game_combat::RoomScriptRequests,
        script_graph::GraphStore,
        script_init::{Component, MAX_INIT_WORK, Root, Site, World},
        script_wire::{Value, Variables},
    };
    use flate2::read::GzDecoder;
    use std::{fs::File, path::PathBuf, time::Duration};

    struct Host {
        frame: Arc<RoomSpatialFrame>,
        requests: RoomScriptRequests,
        scale: f64,
        ray_origin: [f64; 3],
    }
    impl World for Host {
        fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
            None
        }
        fn position(&mut self, entity: u32) -> Option<Vec3> {
            Some(self.frame.actor_pose(entity)?.0)
        }
        fn authored_position(&mut self, _: u32) -> Option<Vec3> {
            Some(self.ray_origin)
        }
        fn ability_ground_ray(
            &self,
            site: Site,
            frame: u32,
            origin: Vec3,
            direction: Vec3,
            length: f64,
        ) -> Option<Option<crate::script_expr::SpatialRayHit>> {
            assert_eq!((direction, length), ([0.0, -1.0, 0.0], 1.5));
            self.frame
                .profile_ray_cast(site, frame, origin, direction, length, 0x1fdf)
        }
        fn body_scale(&mut self, _: Site, _: u32, _: &Value) -> Option<f64> {
            Some(self.scale)
        }
        fn ground_wave_targets(&self, site: Site, w: &GroundWave, reach: f64) -> Option<Vec<u32>> {
            assert_eq!(
                (w.speed, w.distance),
                (40.0 * self.scale, 20.0 * self.scale)
            );
            self.frame.ground_wave_targets(site.entity, w, reach)
        }
        fn queue_health_changes(&mut self, _: Site, changes: &[(u32, f64)]) -> bool {
            self.requests.queue_health_changes(&self.frame, changes)
        }
    }
    let path = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap())
        .join("data/game_graphs_174.json.gz");
    let codecs =
        crate::script_codecs::CodecTable::load(path.with_file_name("statescript_codecs_174.json"))
            .unwrap();
    let stock: Json = serde_json::from_reader(GzDecoder::new(File::open(path).unwrap())).unwrap();
    let graph = stock["graphs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["index"] == 0xbe)
        .unwrap();
    let mut graph = graph.clone();
    let nodes = graph["nodes"].as_array_mut().unwrap();
    nodes.retain(|n| matches!(n["pos"].as_u64(), Some(1 | 3 | 105 | 117)));
    for n in nodes {
        for (key, value) in n["fields"].as_object_mut().unwrap().iter_mut() {
            if key == "m_0B1AA8CA" {
                *value = Json::Null;
            } else if value.get("links").is_some() {
                value["links"] = json!([]);
                if value.get("m_FF3DAF1E").is_some() {
                    value["m_FF3DAF1E"] = Json::Null;
                }
            }
        }
        if n["pos"] == 1 {
            n["fields"]["m_subgraphPlug"] =
                json!({"$":"STU_904BFCEC","m_FF3DAF1E":[1],"links":[[3,"m_beginPlug"]]});
        }
    }
    for pos in graph["states"].as_array_mut().unwrap() {
        if !matches!(pos.as_u64(), Some(1 | 3 | 105 | 117)) {
            *pos = Json::Null;
        }
    }
    graph["entries"] = json!([]);
    for pos in graph["remote"].as_array_mut().unwrap() {
        if !matches!(pos.as_u64(), Some(1 | 105)) {
            *pos = Json::Null;
        }
    }
    let bundle = json!({"version":1,"graphs":[graph],"bodies":{},"missing":[]});
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap());
    let (mut room, ids) = room(floor(25.0, false));
    let mut root = Root::new(1, 0xbe);
    root.variables = Variables::from([
        (431, Value::Vector([0.0; 3])),
        (634, Value::Float(0.0)),
        (375, Value::Integer(0)),
        (468, Value::Array(vec![Value::Entity(ids[2])])),
    ]);
    let mut host = Host {
        frame: room.spatial_frame.as_ref().unwrap().1.clone(),
        requests: Default::default(),
        scale: 1.0,
        ray_origin: [0.0, 0.5, 1.0],
    };
    let mut component = Component::start(
        graphs.clone(),
        ids[0],
        Variables::new(),
        vec![root],
        0,
        &mut host,
        MAX_INIT_WORK,
    )
    .unwrap();
    // Verify the stock ray's bindings on a hit and on a subsequent miss above the same floor.
    component.begin_state(1, 37, &mut host).unwrap();
    component.run_now(&mut host).unwrap();
    let hit = &component.snapshot().unwrap().instances[&1].variables;
    assert_eq!(hit.get(&10360), Some(&Value::Bool(true)));
    assert_eq!(hit.get(&1724), Some(&Value::Vector([0.0, 0.0, 1.0])));
    host.ray_origin[1] = 3.0;
    component.advance_frame(1, &mut host).unwrap();
    assert_eq!(
        component.snapshot().unwrap().instances[&1]
            .variables
            .get(&10360),
        Some(&Value::Bool(false))
    );
    let direct_damage = fixture()["expected"]["direct_damage"].as_f64().unwrap();
    for cast in 0..2_u32 {
        let first = cast * 48 + 2;
        // The contact tracker supplies this list independently of the wave. Its geometry
        // is outside this test. Begin the stock 250-point discrete health state.
        component.begin_state(1, 41, &mut host).unwrap();
        component.run_now(&mut host).unwrap();
        // In the full graph action 00BE:2 clears this output before each impact.
        component
            .set_instance_variable(1, 375, Value::Array(Vec::new()), &mut host)
            .unwrap();
        component.begin_state(1, 0, &mut host).unwrap();
        component.run_now(&mut host).unwrap();
        let begin = component.snapshot().unwrap().instances[&1].states[&0].start_ms;
        let damage = fixture()["expected"]["base_damage"].as_f64().unwrap();
        for frame in first..first + 48 {
            component.advance_frame(frame, &mut host).unwrap();
            let snapshot = component.snapshot().unwrap();
            assert!(
                snapshot.diagnostics.is_empty(),
                "frame {frame}: {:?}",
                snapshot.diagnostics
            );
            if frame == first {
                use crate::script_snapshot_wire::{SnapshotEncoder, View};
                let encoder = SnapshotEncoder::new(&graphs, &codecs);
                for view in [View::Owner, View::Observer] {
                    let projected = encoder.project(snapshot, view).unwrap();
                    let state = &projected.instances[&1].states[&0];
                    assert!(state.active);
                    assert_eq!(state.payload.get("has_list"), Some(&Value::Bool(false)));
                }
            }
            let source = room.sample_health_body(ids[0]).unwrap();
            for effect in host.requests.take() {
                room.settle_script_request(effect, source, frame, Duration::ZERO)
                    .unwrap();
            }
            let elapsed = snapshot.now_ms - begin;
            let hp = room.body_healths().unwrap();
            for (i, id) in ids.iter().enumerate() {
                let struck = (i == 1 && elapsed >= 100) || (i == 3 && elapsed >= 500);
                assert_eq!(
                    hp[id],
                    if i == 2 {
                        600.0 - direct_damage * f64::from(cast + 1)
                    } else if i == 1 || i == 3 {
                        200.0 - damage * (f64::from(cast) + f64::from(u8::from(struck)))
                    } else {
                        200.0
                    },
                    "bot {i}, elapsed {elapsed} ms"
                );
            }
            assert_eq!(snapshot.instances[&1].states[&0].active, elapsed < 500);
        }
    }
}
