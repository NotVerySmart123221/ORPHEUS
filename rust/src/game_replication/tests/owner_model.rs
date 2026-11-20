use super::*;
use crate::{
    accounts::parse_guid, game_heroes::HeroCatalog, script_player::PlayerBodyFactory,
    script_wire::Value as ScriptValue,
};
use serde_json::{Value as Json, json};

#[test]
fn owner_model_hardpoints_follow_the_selected_body_and_skin() {
    #[derive(serde::Deserialize)]
    struct Contract {
        schema_version: u32,
        input: Input,
        expected: Expected,
    }
    #[derive(serde::Deserialize)]
    struct Input {
        hero: u64,
        body: String,
        hardpoint: String,
        model_hardpoints: Json,
        selections: [String; 3],
    }
    #[derive(serde::Deserialize)]
    struct Expected {
        model_has_hardpoint: [Option<bool>; 3],
        body_generations: [u64; 3],
    }

    let Contract {
        schema_version,
        input,
        expected,
    } = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/owner-model-hardpoints-v1.json"
    ))
    .unwrap();
    assert_eq!(schema_version, 1);

    const ROOT: u32 = 93;
    const OUTPUT: u32 = 419;
    let hero = input.hero;
    let owner_mode = GameMap::practice_range().mode_guid;
    let heroes = HeroCatalog::from_json(
        &json!({
            format!("0x{hero:016X}"): {
                "name": "Fixture model wearer", "class": "Damage", "body": &input.body,
            },
        })
        .to_string(),
    )
    .unwrap();

    let mark = |position, membership| {
        json!({
            "class":"STUStatescriptActionSetVar", "pos":position, "rid":position,
            "uid":position, "state":null, "remote":-1,
            "server_only":false, "client_only":false, "fields":{
                "m_2BBEEAB8":false, "m_ADEB6E05":true,
                "m_out_Var":{"$":"STUConfigVarDynamic","m_identifier":OUTPUT,"m_60DB8F99":true},
                "m_value":membership,
            },
        })
    };
    let plug = |position, input| json!({"$":"Plug","links":[[position,input]],"m_FF3DAF1E":[]});
    let graphs = json!({"version":1,"missing":[],"bodies":{
        (&input.body): {
            "hero":format!("0x{hero:016X}"),"name":"Fixture body",
            "graphs":[{"graph":ROOT,"m_75408640":0}],"manager":ROOT,"weapons":[null,ROOT],
            "model_hardpoints":input.model_hardpoints,
        }
    },"graphs":[{
        "index":ROOT,"items":4,"states":[1],"entries":[0],"remote":[],
        "sync_vars":[],"containers":{},"unknown_nodes":[],"fields":{},"header":{
            "m_nodesBitCount":4,"m_statesBitCount":4,"m_remoteSyncNodesBitCount":4,"m_syncVarsBitCount":4
        },
        "nodes":[
            {"class":"STUStatescriptEntry","pos":0,"rid":0,"uid":0,"state":null,"remote":-1,
             "server_only":false,"client_only":false,
             "fields":{"m_outPlug":plug(1,"m_beginPlug"),"m_2BBEEAB8":false,"m_ADEB6E05":false}},
            {"class":"STUStatescriptStateBooleanSwitch","pos":1,"rid":1,"uid":1,"state":0,"remote":-1,
             "server_only":false,"client_only":false,"fields":{
                "m_2BBEEAB8":true,"m_ADEB6E05":false,
                "m_condition":{"$":"STU_719E45B8","m_EE729DCB":0,"m_entity":{"$":"STU_B5A0CAF0","m_EE729DCB":0},
                    "m_hardPoint":{"$":"STUConfigVarHardPoint","m_EE729DCB":0,"m_resourceKey":null,
                        "m_hardPoint":input.hardpoint}},
                "m_onTruePlug":plug(2,"m_inPlug"),"m_onFalsePlug":plug(3,"m_inPlug")
             }},
            mark(2,true),mark(3,false)
        ],
    }]});
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&graphs).unwrap()).unwrap());
    let factory = PlayerBodyFactory::from_body(
        graphs,
        Arc::new(CodecTable::from_json(r#"{"classes":{}}"#).unwrap()),
        owner_mode,
        heroes.get(hero).unwrap(),
        None,
        false,
    )
    .unwrap();

    let mut scene = PreparedScene::fixture_for_owner_admission(heroes);
    scene.owner_body_factory = Some(Arc::new(move |_, spawn, setup| {
        Ok(Some(Box::new(factory.start_with_setup(spawn, setup)?)))
    }));
    let mut world = WorldDelivery::new(OWNER);
    world.started = Some(FIRST_TICK);
    world.created.insert(OWNER);
    world
        .controller
        .full_owner(FIRST_TICK, |_| Ok(crate::bits::BitWriter::new()))
        .unwrap();

    for (selection, skin) in input.selections.into_iter().enumerate() {
        let skin = parse_guid(&skin).unwrap();
        let mut card = PlayerCard::default();
        card.skins.insert(
            hero,
            SkinTheme {
                guid: skin,
                ..SkinTheme::default()
            },
        );
        let viewer = PlayerAppearance {
            entity: OWNER,
            slot: 0,
            team: 0,
            card: &card,
        };
        let previous = world.body;
        if previous.is_some() {
            world.queue_skin(hero, FIRST_TICK, Duration::ZERO);
        } else {
            world.queue_game_message(FIRST_TICK, Duration::ZERO, pick_hero(OWNER, hero));
        }
        assert_eq!(
            world
                .advance_input(&mut scene, &viewer, RETAIL_QUANTA, 1)
                .unwrap(),
            (1, false)
        );
        let current = world.body.unwrap();
        assert_eq!(current.id.generation, expected.body_generations[selection]);
        let snapshot = scene
            .simulation()
            .player_body(current.id)
            .unwrap()
            .runtime
            .as_ref()
            .unwrap()
            .snapshot()
            .unwrap();
        if let Some(membership) = expected.model_has_hardpoint[selection] {
            assert_eq!(current.skin.guid, skin);
            assert_eq!(
                snapshot.variables.get(&OUTPUT),
                Some(&ScriptValue::Bool(membership))
            );
        } else {
            let previous = previous.unwrap();
            assert_eq!(
                current.id, previous.id,
                "unknown model must reject the candidate"
            );
            assert_eq!(current.skin, previous.skin);
            assert_eq!(
                snapshot.variables.get(&OUTPUT),
                Some(&ScriptValue::Bool(false))
            );
        }
        assert!(snapshot.initialization_complete());
    }
}
