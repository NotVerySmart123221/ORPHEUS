use super::*;

const BOOTSTRAP: u32 = 0x7110;
const SHIELD: u32 = 0x7111;
const OWNER: u32 = 777;
const GRANT: u32 = 5;

#[derive(Default)]
struct ShieldWorld {
    grant: Option<(u64, f64, bool)>,
    removals: usize,
}

impl World for ShieldWorld {
    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }

    fn add_temporary_shield(
        &mut self,
        site: Site,
        identifier: u64,
        amount: f64,
        stacked: bool,
    ) -> Option<u32> {
        assert_eq!(
            (site.entity, site.graph, site.node),
            (OWNER, SHIELD, Some(1))
        );
        assert!(self.grant.replace((identifier, amount, stacked)).is_none());
        Some(GRANT)
    }

    fn temporary_shield_current(&self, site: Site, identifier: u64) -> Option<f64> {
        (site.entity == OWNER).then(|| {
            self.grant
                .filter(|grant| grant.0 == identifier)
                .map_or(0.0, |grant| grant.1)
        })
    }

    fn health_pool_current(&self, _: Site, pool: u32) -> Option<f64> {
        assert_eq!(pool, GRANT);
        self.grant.map(|grant| grant.1)
    }

    fn remove_health_pool(&mut self, _: Site, pool: u32) -> Option<f64> {
        assert_eq!(pool, GRANT);
        self.removals += 1;
        self.grant.take().map(|grant| grant.1)
    }
}

#[test]
fn named_owner_shield_uses_the_pool_lifecycle_and_self_query() {
    let contract: Json = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-named-shield-v1.json"
    )))
    .unwrap();
    let pool = &contract["input"]["pool_identifier"];
    let identifier = asset_guid(pool).unwrap();
    let amount = contract["input"]["amount"].as_f64().unwrap();
    let expected = |name: &str| Value::Float(contract["expected"][name].as_f64().unwrap());
    let owner = json!({"$":"STU_B5A0CAF0"});
    let pool_key = json!({
        "$":"STUConfigVarIdentifier", "m_identifier":pool, "m_resourceKey":null
    });
    let graphs = Arc::new(store(vec![
        graph(BOOTSTRAP, Vec::new(), json!([]), json!([])),
        graph(
            SHIELD,
            vec![
                node(
                    0,
                    "STUStatescriptEntry",
                    None,
                    json!({"m_outPlug":plug(&[(1,"m_beginPlug")],&[])}),
                ),
                node(
                    1,
                    "STUStatescriptStateHealthPool",
                    Some(0),
                    json!({
                        "m_FA3ECC43":owner,
                        "m_69A20070":{"$":"STU_FEDD683C", "m_entity":owner},
                        "m_7E8B666C":2,
                        "m_7BE02F89":pool_key,
                        "m_64839023":{"$":"STUConfigVarBool","m_value":0},
                        "m_0E8A5C8E":{"$":"STUConfigVarBool","m_value":1},
                        "m_CEAB19E8":var(40,false),
                        "m_C99FAFDD":var(41,false),
                        "m_amount":amount
                    }),
                ),
            ],
            json!([1]),
            json!([0]),
        ),
    ]));
    let query = json!({
        "$":"STU_783B4CF0", "m_entity":owner, "m_7BE02F89":pool_key,
        "m_7E8B666C":{"$":"STU_297E1A30","m_7E8B666C":2}
    });
    for stacked in [false, true] {
        let mut world = ShieldWorld::default();
        let mut component = Component::start(
            Arc::clone(&graphs),
            OWNER,
            Variables::new(),
            vec![Root::listener(1, BOOTSTRAP, None)],
            0,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        let site = Site {
            entity: OWNER,
            instance: 1,
            graph: BOOTSTRAP,
            node: None,
        };
        component
            .receive_play_script(
                site,
                &PlayScript {
                    policy: 0,
                    source: OWNER,
                    graph: SHIELD,
                    overrides: vec![],
                    parameters: vec![],
                    auto_retire: stacked,
                },
                &mut world,
            )
            .unwrap();
        assert_eq!(world.grant, Some((identifier, amount, stacked)));
        let snapshot = component.snapshot().unwrap();
        let (&id, instance) = snapshot
            .instances
            .iter()
            .find(|(_, instance)| instance.graph == SHIELD)
            .unwrap();
        assert!(instance.states[&0].active);
        assert!(!instance.variables.contains_key(&40));
        assert!(!instance.variables.contains_key(&41));
        assert_eq!(
            component.evaluate_state_config(id, 0, &query, &mut world),
            Ok(expected("current_after_grant"))
        );

        if stacked {
            world.grant.as_mut().unwrap().1 = 0.0;
            component.advance_frame(1, &mut world).unwrap();
        } else {
            component.debug_end_state(id, 0, &mut world).unwrap();
        }
        assert_eq!(world.removals, 1);
        assert!(world.grant.is_none());
        let snapshot = component.snapshot().unwrap();
        assert!(!snapshot.instances[&id].states[&0].active);
        assert_eq!(
            component.evaluate_state_config(id, 0, &query, &mut world),
            Ok(expected("current_after_removal"))
        );
        assert!(component.snapshot().unwrap().diagnostics.is_empty());
        component.advance_frame(2, &mut world).unwrap();
        assert_eq!(world.removals, 1);
    }
}
