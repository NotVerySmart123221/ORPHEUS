use super::*;

const BOOTSTRAP: u32 = 0x7110;
const SHIELD: u32 = 0x7111;
const DECAY: u32 = 0x7112;
const OWNER: u32 = 777;
const GRANT: u32 = 5;

#[derive(Default)]
struct ShieldWorld {
    grant: Option<(u64, f64, bool)>,
    removals: usize,
    decays: Vec<f64>,
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

    fn drain_temporary_shield(&mut self, site: Site, identifier: u64, amount: f64) -> Option<f64> {
        assert_eq!(
            (site.entity, site.graph, site.node),
            (OWNER, DECAY, Some(1))
        );
        self.decays.push(amount);
        let Some((_, current, _)) = self.grant.as_mut().filter(|grant| grant.0 == identifier)
        else {
            return Some(0.0);
        };
        *current = (*current - amount).max(0.0);
        Some(*current)
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

#[test]
fn named_shield_decay_uses_the_live_rate_and_retires_at_zero() {
    const RATE: u32 = 43;
    let contract: Json = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-named-shield-v1.json"
    )))
    .unwrap();
    let pool = &contract["input"]["pool_identifier"];
    let identifier = asset_guid(pool).unwrap();
    let decay = &contract["input"]["decay"];
    let starting_amount = decay["initial_amount"].as_f64().unwrap();
    let rates: [f64; 2] = serde_json::from_value(decay["rates_per_second"].clone()).unwrap();
    let expected_amounts: [f64; 2] =
        serde_json::from_value(contract["expected"]["after_decay_frames"].clone()).unwrap();
    let expected_calls: Vec<f64> =
        serde_json::from_value(contract["expected"]["decay_amounts"].clone()).unwrap();
    let owner = json!({"$":"STU_B5A0CAF0"});
    let graphs = Arc::new(store(vec![graph(
        DECAY,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":plug(&[(1, "m_beginPlug")], &[])}),
            ),
            node(
                1,
                "STUStatescriptStateModifyHealth",
                Some(0),
                json!({
                    "m_DD792FF0":2, "m_7B2B3F5C":1, "m_8B707686":1,
                    "m_FA3ECC43":owner,
                    "m_7BE02F89":{"$":"STUConfigVarIdentifier", "m_identifier":pool, "m_resourceKey":null},
                    "m_95F1FB0B":{"$":"STUConfigVarInt", "m_value":1},
                    "m_modifyHealth":{
                        "$":"STUStatescriptModifyHealth", "m_724CBA57":owner, "m_93CDFD2F":1,
                        "m_F8EA2036":{"$":"STUConfigVarBool","m_value":1},
                        "m_62E3835A":{"$":"STUConfigVarBool","m_value":0},
                        "m_148F3152":[{
                            "$":"STUStatescriptModifyHealthTag",
                            "m_EE3055BD":{"$":"STUConfigVarIdentifier",
                                "m_identifier":"0x0D8000000000E130", "m_resourceKey":null}
                        }],
                        "m_amount":var(RATE,false)
                    }
                }),
            ),
        ],
        json!([1]),
        json!([0]),
    )]));
    let mut root = Root::new(1, DECAY);
    root.variables.insert(RATE, Value::Float(rates[0]));
    let mut world = ShieldWorld {
        grant: Some((identifier, starting_amount, false)),
        ..ShieldWorld::default()
    };
    let mut component = Component::start(
        graphs,
        OWNER,
        Variables::new(),
        vec![root],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    let site = Site {
        entity: OWNER,
        instance: 1,
        graph: DECAY,
        node: Some(1),
    };
    assert_eq!(
        world.temporary_shield_current(site, identifier),
        Some(starting_amount)
    );
    for (frame, (rate, expected)) in (1..=2).zip(rates.into_iter().zip(expected_amounts)) {
        component
            .set_instance_variable(1, RATE, Value::Float(rate), &mut world)
            .unwrap();
        component.advance_frame(frame, &mut world).unwrap();
        assert_eq!(
            world.temporary_shield_current(site, identifier),
            Some(expected)
        );
    }
    component.advance_frame(3, &mut world).unwrap();
    let snapshot = component.snapshot().unwrap();
    assert_eq!(
        snapshot.instances[&1].states[&0].kind,
        StateKind::NamedShieldDecay
    );
    assert!(!snapshot.instances[&1].states[&0].active);
    assert!(
        snapshot.diagnostics.is_empty(),
        "{:?}",
        snapshot.diagnostics
    );
    assert_eq!(world.decays, expected_calls);
}
