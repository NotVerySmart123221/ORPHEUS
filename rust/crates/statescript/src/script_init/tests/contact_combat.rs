use super::*;

const OWNER: u32 = 51;
const TARGET: u32 = 53;
const PROFILE: u64 = 0x0690_0000_0000_00aa;

#[derive(Default)]
struct ContactWorld {
    volume: std::cell::Cell<Option<ContactVolume>>,
    damage: Vec<(u32, f64, Option<[f64; 3]>)>,
}

impl World for ContactWorld {
    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }

    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        Some([0.0; 3])
    }

    fn body_eye_position(&self, entity: u32) -> Option<[f64; 3]> {
        (entity == OWNER).then_some([0.0, 1.25, 0.0])
    }

    fn room_actor_identity(&self, entity: u32) -> Option<u64> {
        (entity == OWNER).then_some(300)
    }

    fn contact_volume_targets(
        &self,
        site: Site,
        volume: ContactVolume,
    ) -> Option<Vec<ContactTarget>> {
        assert_eq!(site.entity, OWNER);
        self.volume.set(Some(volume));
        Some(vec![
            ContactTarget {
                entity: TARGET,
                feet: [2.0, 0.0, 0.0],
            },
            ContactTarget {
                entity: TARGET + 1,
                feet: [3.0, 0.0, 0.0],
            },
        ])
    }

    fn queue_contact_damage(
        &mut self,
        site: Site,
        damage: f64,
        hits: &[(u32, Option<[f64; 3]>)],
    ) -> bool {
        assert_eq!(site.entity, OWNER);
        self.damage
            .extend(hits.iter().map(|&(id, impulse)| (id, damage, impulse)));
        true
    }
}

#[test]
fn authored_contact_damage_uses_the_linked_volume_once_per_activation() {
    let fixture: Json = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-contact-combat-v1.json"
    )))
    .unwrap();
    let input = &fixture["input"];
    let offset: [f64; 3] = serde_json::from_value(input["offset"].clone()).unwrap();
    let owner = json!({"$":"STU_B5A0CAF0"});
    let vector = |[x, y, z]: [f64; 3]| {
        json!({
            "$":"STUConfigVarVec3", "m_value":{"X":x,"Y":y,"Z":z}
        })
    };
    for (index, tracker, damage) in [(0x3341, 1, 2), (0x527e, 6, 8)] {
        let mut authored = graph(
            index,
            vec![
                node(
                    tracker,
                    "STUStatescriptStateTrackTargets",
                    Some(0),
                    json!({
                        "m_95FFC3C5":1,
                        "m_8DCA77B6":owner,
                        "m_05BE62DD":vector(offset),
                        "m_D59188D5":{"$":"STU_21A13145","m_entity":owner},
                        "m_radius":input["radius"],
                        "m_8A2905A0":var(401, false),
                        "m_24A4AB8A":var(412, false),
                        "m_filter":{"$":"STUConfigVarFilter","m_84065FD1":64,"m_4EA686DB":20,"m_89831602":1},
                        "m_3D667CE3":{"$":"STUConfigVarFilter","m_C1903771":{
                            "$":"STU_20DB8AB5","m_C1903771":format!("0x{PROFILE:016X}")
                        }},
                        "m_subgraphPlug":plug(&[(damage,"m_beginPlug")], &[1]),
                    }),
                ),
                node(
                    damage,
                    "STUStatescriptStateModifyHealth",
                    Some(1),
                    json!({
                        "m_DD792FF0":3,
                        "m_FA3ECC43":var(401,false),
                        "m_B12BFE8B":var(412,false),
                        "m_modifyHealth":{
                            "$":"STUStatescriptModifyHealth",
                            "m_93CDFD2F":1,
                            "m_instigator":owner,
                            "m_724CBA57":owner,
                            "m_C0139FA6":vector([0.0;3]),
                            "m_amount":input["damage"],
                            "m_E33EFEE1":input["knockback"][0],
                            "m_10399AE7":input["knockback"][1],
                            "m_knockbackRings":[{
                                "$":"STUStatescriptModifyHealthKnockbackRing",
                                "m_radius":3.0, "m_579D7CFF":1.0, "m_10399AE7":1.0
                            }]
                        }
                    }),
                ),
            ],
            json!([tracker, damage]),
            json!([]),
        );
        authored["items"] = json!(damage + 1);
        let bundle = json!({
            "version":1, "graphs":[authored], "bodies":{}, "missing":[],
            "static_ray_profiles":{(format!("0x{PROFILE:016X}")):24}
        });
        let graphs =
            Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap());
        assert!(graphs.uses_spatial_actor_rays());
        assert_eq!(graphs.static_ray_category(PROFILE), Some(24));
        let mut world = ContactWorld::default();
        let mut component = Component::start(
            graphs,
            OWNER,
            Variables::new(),
            vec![Root::listener(1, index, None)],
            0,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        let key = StateKey {
            instance: 1,
            raw: 0,
        };
        for _ in 0..2 {
            component.run(&mut world, |vm| vm.begin(key, 0)).unwrap();
        }
        component.advance_frame(1, &mut world).unwrap();
        let volume = world.volume.get().unwrap();
        assert_eq!(volume.profile, PROFILE);
        assert_eq!(volume.offset, offset);
        assert_eq!(volume.radius, input["radius"].as_f64().unwrap());
        let expected = &fixture["expected"];
        let amount = input["damage"].as_f64().unwrap();
        assert_eq!(
            world.damage,
            [
                (
                    TARGET,
                    amount,
                    serde_json::from_value(expected["full_impulse"].clone()).unwrap()
                ),
                (
                    TARGET + 1,
                    amount,
                    serde_json::from_value(expected["outside_impulse"].clone()).unwrap()
                ),
            ]
        );
        let snapshot = component.snapshot().unwrap();
        assert!(
            snapshot.diagnostics.is_empty(),
            "{:?}",
            snapshot.diagnostics
        );
        assert_eq!(
            snapshot.instances[&1].variables.get(&401),
            Some(&Value::Array(vec![
                Value::Entity(TARGET),
                Value::Entity(TARGET + 1),
            ]))
        );
    }
}
