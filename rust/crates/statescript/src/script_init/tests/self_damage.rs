use serde::Deserialize;

use super::*;

const STATE: &str = "STUStatescriptStateModifyHealth";
const OWNER: u32 = 51;
const INSTANCE: u16 = 1;

#[derive(Deserialize)]
struct Contract {
    input: Vec<Scenario>,
}

#[derive(Deserialize)]
struct Scenario {
    name: String,
    graph: u32,
    pos: u16,
    self_instigator: bool,
    amount: i64,
    #[serde(default)]
    owner_dead: bool,
    #[serde(default)]
    node_fields: serde_json::Map<String, Json>,
    #[serde(default)]
    effect_fields: serde_json::Map<String, Json>,
    admitted: bool,
    expected_damage: Vec<f64>,
}

struct DamageWorld<'a> {
    scenario: &'a Scenario,
    modify_calls: u8,
    damage: Vec<f64>,
}

impl World for DamageWorld<'_> {
    fn modify_health(&mut self, site: Site, elapsed_ms: u64) -> bool {
        assert_eq!(
            (site.graph, site.node),
            (self.scenario.graph, Some(self.scenario.pos))
        );
        assert_eq!(elapsed_ms, 0);
        self.modify_calls += 1;
        false
    }

    fn self_damage(&mut self, site: Site, amount: f64) -> bool {
        assert_eq!(
            (site.entity, site.graph, site.node),
            (OWNER, self.scenario.graph, Some(self.scenario.pos))
        );
        self.damage.push(amount);
        true
    }

    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }
}

#[test]
fn authored_immediate_self_damage_is_placement_independent_and_applies_once() {
    let fixture: Contract = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/behavior/script-immediate-self-damage-v1.json"
    )))
    .unwrap();

    for scenario in &fixture.input {
        let owner = json!({"$": "STU_B5A0CAF0"});
        let instigator = if scenario.self_instigator {
            owner.clone()
        } else {
            Json::Null
        };
        let mut fields = json!({
            "m_DD792FF0": 3,
            "m_7B2B3F5C": 0,
            "m_8B707686": 1,
            "m_FA3ECC43": owner,
            "m_48BED081": {"$": "STUConfigVarBool", "m_value": false},
            "m_onFinishedPlug": {"$": "STUStatescriptOutputPlug", "links": []},
            "m_subgraphPlug": {"$": "STU_904BFCEC", "links": [], "m_FF3DAF1E": null},
            "m_modifyHealth": {
                "$": "STUStatescriptModifyHealth",
                "m_93CDFD2F": 1,
                "m_430E83BE": 0,
                "m_amount": {"$": "STUConfigVarInt", "m_value": scenario.amount},
                "m_724CBA57": owner,
                "m_instigator": instigator
            }
        });
        fields["m_modifyHealth"]
            .as_object_mut()
            .unwrap()
            .extend(scenario.effect_fields.clone());
        fields
            .as_object_mut()
            .unwrap()
            .extend(scenario.node_fields.clone());
        let mut authored = graph(
            scenario.graph,
            vec![node(scenario.pos, STATE, Some(0), fields)],
            json!([scenario.pos]),
            json!([]),
        );
        authored["items"] = json!(scenario.pos + 1);
        let mut world = DamageWorld {
            scenario,
            modify_calls: 0,
            damage: Vec::new(),
        };
        let mut component = Component::start(
            Arc::new(store(vec![authored])),
            OWNER,
            Variables::new(),
            vec![Root::listener(INSTANCE, scenario.graph, None)],
            0,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        component.set_owner_dead(scenario.owner_dead);
        let key = StateKey {
            instance: INSTANCE,
            raw: 0,
        };
        component.run(&mut world, |vm| vm.begin(key, 0)).unwrap();
        if scenario.admitted {
            component.run(&mut world, |vm| vm.begin(key, 0)).unwrap();
        }
        for frame in 1..=4 {
            component.advance_frame(frame, &mut world).unwrap();
        }
        let snapshot = component.snapshot().unwrap();
        if scenario.admitted {
            assert!(
                snapshot.diagnostics.is_empty(),
                "{}: {:?}",
                scenario.name,
                snapshot.diagnostics
            );
            assert_eq!(
                snapshot.instances[&INSTANCE].states[&0].kind,
                StateKind::ModifyHealth
            );
        } else {
            assert!(
                matches!(
                    snapshot.diagnostics.as_slice(),
                    [Diagnostic { issue: Issue::UnsupportedState(class), .. }] if class == STATE
                ),
                "{}: {:?}",
                scenario.name,
                snapshot.diagnostics
            );
            assert!(!snapshot.instances[&INSTANCE].states.contains_key(&0));
        }
        assert_eq!(
            world.modify_calls,
            u8::from(scenario.admitted),
            "{}",
            scenario.name
        );
        assert_eq!(world.damage, scenario.expected_damage, "{}", scenario.name);
    }
}
