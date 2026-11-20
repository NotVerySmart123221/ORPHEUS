use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value as Json, json};

use crate::{
    game_bots::BotWalk,
    game_combat::{
        CombatError, ENCOUNTER_INSTANCE, EncounterPhase, EncounterRegistration, EncounterWorld,
        GateDamageStage, GateHealth, GateState, GateWorld, RoomSimulation,
    },
    game_heroes::HeroCatalog,
    junkenstein_messages::{BOSS_PAUSE, ENCOUNTER_RESULT, POST_MATCH_COMPLETE, POST_MATCH_PHASE},
    mover_settings::{MoverCatalog, MoverSettings},
    npc_catalog::NpcDefinition,
    scene_assets::{BotQueries, JunkensteinGate, SpawnAnchors},
    script_codecs::CodecTable,
    script_enemy::SceneMessage,
    script_expr::{Scope, Variable},
    script_graph::GraphStore,
    script_init::{self, InitFault, Root, StateKind},
    script_stream::{BodyStream, DeltaKind},
    script_wire::{Value, Variables},
    world::GAME_MODE_ENTITY,
};

const GATE_ENTITY: u32 = 0x8000_0032;
const FIXTURE_MODE: u64 = 0x0230_0000_0000_019f;
const GATE_DAMAGE_MSG: u64 = 0x0240_0000_0000_06b2;
const GATE_PING_MSG: u64 = 0x0240_0000_0000_06f9;
const CLEAR_COUNTDOWN_MSG: u64 = 0x0240_0000_0000_06fa;
const TRIGGER_END_MSG: u64 = 0x0240_0000_0000_06fb;
const TRIGGER_FAULT_MSG: u64 = 0x0240_0000_0000_06fc;
const READY_MSG: u64 = 0x0240_0000_0000_06fd;
const TRIGGER_COMPLETE_MSG: u64 = 0x0240_0000_0000_06fe;
const READY_PARAM: u32 = 0x039f;

const ENCOUNTER_CODECS: &str = r#"{"classes":{
    "12340001":{"name":"STUStatescriptState","codec":"none","stec":false,"status":"VERIFIED","layout":[]},
    "12340002":{"name":"STUStatescriptStateBooleanSwitch","codec":"switch","stec":false,"status":"VERIFIED",
        "layout":[{"op":"bits","f":"current","n":1}]},
    "12340004":{"name":"STUStatescriptStateGameMessageEntry","codec":"message","stec":false,"status":"VERIFIED",
        "layout":[{"op":"if","c":"H","then":[{"op":"bits","f":"has_sender","n":1}],"else":[]},
                  {"op":"bits","f":"stacked","n":1}]},
    "594C2D80":{"name":"STUStatescriptStateStack","codec":"stack","stec":false,"status":"VERIFIED",
        "layout":[{"op":"if","c":"H","then":[{"op":"bits","f":"top","n":1},
                  {"op":"bits","f":"under","n":1}],"else":[]}]},
    "6AE5D1A1":{"name":"STUStatescriptStateChaseVar","codec":"chase","stec":false,"status":"VERIFIED",
        "layout":[{"op":"bits","f":"vector","n":1},{"op":"bits","f":"has_remaining","n":1},
                  {"op":"bits","f":"reached","n":1},{"op":"f32","f":"x"},
                  {"op":"if","c":"vector","then":[{"op":"f32","f":"y"},{"op":"f32","f":"z"}],"else":[]},
                  {"op":"if","c":"has_remaining","then":[{"op":"bits","f":"big","n":1},
                   {"op":"if","c":"big","then":[{"op":"bits","f":"remaining","n":32}],
                    "else":[{"op":"bits","f":"remaining","n":16}]}],"else":[]},
                  {"op":"bits","f":"has_last","n":1},{"op":"if","c":"has_last",
                   "then":[{"op":"var_b","f":"t"}],"else":[]}]},
    "38EE1100":{"name":"STU_38EE1100","codec":"send","stec":false,"status":"VERIFIED","layout":[]}
}}"#;

fn var(id: u32, entity: bool) -> Json {
    json!({"$": "STUConfigVarDynamic", "m_identifier": id, "m_60DB8F99": entity})
}

fn plug(links: &[(u16, &str)], states: &[u16]) -> Json {
    json!({"$": "Plug", "links": links, "m_FF3DAF1E": states})
}

fn node(position: u16, class: &str, state: Option<u16>, mut fields: Json) -> Json {
    fields["m_2BBEEAB8"] = json!(state.is_some());
    fields["m_ADEB6E05"] = json!(
        class.contains("Action")
            || matches!(
                class,
                "STU_0F647E55" | "STU_57692020" | "STU_100EF8C2" | "STU_C43F9944" | "STU_523EE38D"
            )
    );
    json!({
        "pos": position,
        "class": class,
        "uid": position,
        "rid": position,
        "remote": -1,
        "state": state,
        "client_only": false,
        "server_only": false,
        "fields": fields,
    })
}

fn remote_node(
    position: u16,
    class: &str,
    state: Option<u16>,
    remote: i16,
    mut fields: Json,
) -> Json {
    fields["m_2BBEEAB8"] = json!(state.is_some());
    fields["m_ADEB6E05"] = json!(matches!(class, "STU_100EF8C2"));
    json!({
        "pos": position,
        "class": class,
        "uid": position,
        "rid": position,
        "remote": remote,
        "state": state,
        "client_only": false,
        "server_only": false,
        "fields": fields,
    })
}

fn graph(
    index: u32,
    items: u16,
    nodes: Vec<Json>,
    states: Json,
    entries: Json,
    remote: Json,
    sync_vars: Json,
) -> Json {
    json!({
        "index": index,
        "items": items,
        "nodes": nodes,
        "states": states,
        "entries": entries,
        "remote": remote,
        "sync_vars": sync_vars,
        "containers": {},
        "unknown_nodes": [],
        "fields": {},
        "header": {
            "m_nodesBitCount": 6,
            "m_statesBitCount": 6,
            "m_remoteSyncNodesBitCount": 4,
            "m_syncVarsBitCount": 4
        }
    })
}

fn gate_graph_json() -> Json {
    let nodes = vec![
        node(
            0,
            "STUStatescriptEntry",
            None,
            json!({
                "m_outPlug": plug(&[
                    (1, "m_inPlug"),
                    (2, "m_inPlug"),
                    (3, "m_inPlug"),
                    (4, "m_inPlug"),
                    (5, "m_beginPlug"),
                    (6, "m_beginPlug"),
                    (14, "m_beginPlug"),
                    (15, "m_beginPlug"),
                    (16, "m_beginPlug"),
                    (17, "m_beginPlug"),
                ], &[])
            }),
        ),
        node(
            1,
            "STU_0F647E55",
            None,
            json!({
                "m_67465E75": {"$": "STUConfigVarInt", "m_value": 2},
                "m_targets": {"$": "STU_B5A0CAF0"}
            }),
        ),
        node(
            2,
            "STUStatescriptActionSetVar",
            None,
            json!({
                "m_out_Var": var(8571, true),
                "m_value": {"$": "STUConfigVarInt", "m_value": 1}
            }),
        ),
        node(
            3,
            "STUStatescriptActionSetVar",
            None,
            json!({
                "m_out_Var": var(8569, true),
                "m_value": {"$": "STUConfigVarFloat", "m_value": 0.0}
            }),
        ),
        node(
            4,
            "STUStatescriptActionSetVar",
            None,
            json!({
                "m_out_Var": var(7786, true),
                "m_value": {"$": "STUConfigVarFloat", "m_value": 1000.0}
            }),
        ),
        node(
            5,
            "STUStatescriptStateStack",
            Some(0),
            json!({
                "m_out_Var": var(7786, true),
                "m_value": var(8567, false)
            }),
        ),
        node(
            6,
            "STUStatescriptStateBooleanSwitch",
            Some(2),
            json!({
                "m_condition": {
                    "$": "STU_919FD47C",
                    "m_expression": {
                        "m_opcodes": [8, 0, 6, 0, 36, 0],
                        "m_dynamicVars": [var(8571, true)],
                        "m_D99EF254": [0.0]
                    }
                }
            }),
        ),
        node(
            7,
            "STUStatescriptEntryGameMessage",
            None,
            json!({
                "m_gameMessage": {"m_gameMessage": format!("0x{GATE_DAMAGE_MSG:016X}")},
                "m_outPlug": plug(&[(8, "m_inPlug")], &[])
            }),
        ),
        node(
            8,
            "STUStatescriptActionSetVar",
            None,
            json!({
                "m_out_Var": var(8569, true),
                "m_value": {
                    "$": "STU_919FD47C",
                    "m_expression": {
                        "m_opcodes": [8, 0, 13, 1, 23, 0],
                        "m_dynamicVars": [var(8569, true), var(0x03ac, false)],
                        "m_D99EF254": []
                    }
                },
                "m_outPlug": plug(&[(9, "m_inPlug")], &[])
            }),
        ),
        node(
            9,
            "STUStatescriptActionSetVar",
            None,
            json!({
                "m_out_Var": var(8571, true),
                "m_value": {
                    "$": "STU_919FD47C",
                    "m_expression": {
                        "m_opcodes": [6, 0, 8, 0, 8, 1, 24, 8, 0, 26, 64, 0],
                        "m_dynamicVars": [var(7786, true), var(8569, true)],
                        "m_D99EF254": [0.0]
                    }
                },
                "m_outPlug": plug(&[(11, "m_inPlug")], &[])
            }),
        ),
        node(10, "STUStatescriptState", Some(3), json!({})),
        node(
            11,
            "STUStatescriptActionSendGameMessage",
            None,
            json!({
                "m_gameMessage": {"m_gameMessage": format!("0x{GATE_PING_MSG:016X}")},
                "m_targets": {"$": "STU_A3DEF368"}
            }),
        ),
        node(
            12,
            "STUStatescriptEntryGameMessage",
            None,
            json!({
                "m_gameMessage": {"m_gameMessage": format!("0x{ENCOUNTER_RESULT:016X}")},
                "m_outPlug": plug(&[(13, "m_inPlug")], &[])
            }),
        ),
        node(
            13,
            "STUStatescriptActionSetVar",
            None,
            json!({
                "m_out_Var": var(1303, false),
                "m_value": {
                    "$": "STU_919FD47C",
                    "m_expression": {
                        "m_opcodes": [13, 0, 0],
                        "m_dynamicVars": [var(0x03b5, false)],
                        "m_D99EF254": []
                    }
                }
            }),
        ),
        node(
            14,
            "STUStatescriptStateBooleanSwitch",
            Some(7),
            json!({
                "m_condition": {
                    "$": "STU_919FD47C",
                    "m_expression": {
                        "m_opcodes": [8, 0, 6, 0, 36, 0],
                        "m_dynamicVars": [var(8571, true)],
                        "m_D99EF254": [0.75]
                    }
                },
                "m_trueSubgraphPlug": plug(&[(10, "m_beginPlug")], &[3])
            }),
        ),
        node(
            15,
            "STUStatescriptStateBooleanSwitch",
            Some(8),
            json!({
                "m_condition": {
                    "$": "STU_919FD47C",
                    "m_expression": {
                        "m_opcodes": [8, 0, 6, 0, 33, 8, 0, 6, 1, 36, 25, 0],
                        "m_dynamicVars": [var(8571, true)],
                        "m_D99EF254": [0.75, 0.50]
                    }
                },
                "m_trueSubgraphPlug": plug(&[(20, "m_beginPlug")], &[4])
            }),
        ),
        node(
            16,
            "STUStatescriptStateBooleanSwitch",
            Some(9),
            json!({
                "m_condition": {
                    "$": "STU_919FD47C",
                    "m_expression": {
                        "m_opcodes": [8, 0, 6, 0, 33, 8, 0, 6, 1, 36, 25, 0],
                        "m_dynamicVars": [var(8571, true)],
                        "m_D99EF254": [0.50, 0.25]
                    }
                },
                "m_trueSubgraphPlug": plug(&[(21, "m_beginPlug")], &[5])
            }),
        ),
        node(
            17,
            "STUStatescriptStateBooleanSwitch",
            Some(10),
            json!({
                "m_condition": {
                    "$": "STU_919FD47C",
                    "m_expression": {
                        "m_opcodes": [8, 0, 6, 0, 33, 8, 0, 6, 1, 36, 25, 0],
                        "m_dynamicVars": [var(8571, true)],
                        "m_D99EF254": [0.25, 0.0]
                    }
                },
                "m_trueSubgraphPlug": plug(&[(22, "m_beginPlug")], &[6])
            }),
        ),
        node(20, "STUStatescriptState", Some(4), json!({})),
        node(21, "STUStatescriptState", Some(5), json!({})),
        node(22, "STUStatescriptState", Some(6), json!({})),
    ];
    graph(
        0x0ca7,
        23,
        nodes,
        json!([5, null, 6, 10, 20, 21, 22, 14, 15, 16, 17]),
        json!([0, 7, 12]),
        json!([6, 10, 20, 21, 22]),
        json!([[8571, 0, 0], [8569, 0, 0], [7786, 0, 0]]),
    )
}

fn mode_graphs_json() -> Vec<Json> {
    let shared = graph(
        0x0c8e,
        0,
        vec![],
        json!([]),
        json!([]),
        json!([]),
        json!([]),
    );

    let mut encounter_states = vec![Json::Null; 53];
    encounter_states[0] = json!(1); // BooleanSwitch watching gate 8571 < 1.0
    encounter_states[1] = json!(2); // State activated on foreign change
    encounter_states[25] = json!(76); // BOSS_PAUSE persistent send
    encounter_states[3] = json!(18); // POST_MATCH_PHASE persistent send
    encounter_states[4] = json!(9); // 0x180a post-match subscript
    encounter_states[52] = json!(4); // Countdown ChaseVar

    let encounter_nodes = vec![
        node(
            0,
            "STUStatescriptEntry",
            None,
            json!({
                "m_outPlug": plug(&[(1, "m_beginPlug"), (4, "m_beginPlug")], &[])
            }),
        ),
        node(
            1,
            "STUStatescriptStateBooleanSwitch",
            Some(0),
            json!({
                "m_condition": {
                    "$": "STU_919FD47C",
                    "m_configVars": [{"$": "STUConfigVarInt", "m_value": 8571}],
                    "m_expression": {
                        "m_opcodes": [8, 0, 14, 0, 15, 6, 0, 34, 0],
                        "m_dynamicVars": [var(8439, true)],
                        "m_D99EF254": [1.0]
                    }
                },
                "m_trueSubgraphPlug": plug(&[(2, "m_beginPlug")], &[1])
            }),
        ),
        node(
            2,
            "STUStatescriptState",
            Some(1),
            json!({
                "m_onBeginPlug": plug(&[(3, "m_inPlug")], &[])
            }),
        ),
        node(
            3,
            "STUStatescriptActionSetVar",
            None,
            json!({
                "m_out_Var": var(9001, true),
                "m_value": {"$": "STUConfigVarInt", "m_value": 1}
            }),
        ),
        node(
            4,
            "STUStatescriptStateChaseVar",
            Some(52),
            json!({
                "m_out_Var": var(1303, true),
                "m_EB0FC261": {"$": "STUConfigVarFloat", "m_value": 3.0}
            }),
        ),
        node(
            5,
            "STUStatescriptEntryGameMessage",
            None,
            json!({
                "m_gameMessage": {"m_gameMessage": format!("0x{GATE_PING_MSG:016X}")},
                "m_outPlug": plug(&[(6, "m_inPlug")], &[])
            }),
        ),
        node(
            6,
            "STUStatescriptActionSetVar",
            None,
            json!({
                "m_out_Var": var(9002, true),
                "m_value": {"$": "STUConfigVarInt", "m_value": 1}
            }),
        ),
        node(
            7,
            "STUStatescriptEntryGameMessage",
            None,
            json!({
                "m_gameMessage": {"m_gameMessage": format!("0x{CLEAR_COUNTDOWN_MSG:016X}")},
                "m_outPlug": plug(&[(4, script_init::ABORT_PLUG)], &[])
            }),
        ),
        node(
            8,
            "STUStatescriptEntryGameMessage",
            None,
            json!({
                "m_gameMessage": {"m_gameMessage": format!("0x{TRIGGER_END_MSG:016X}")},
                "m_outPlug": plug(&[
                    (76, "m_beginPlug"),
                    (18, "m_beginPlug"),
                    (265, "m_inPlug"),
                    (84, "m_inPlug"),
                ], &[])
            }),
        ),
        node(
            9,
            "STU_37D754C8",
            Some(4),
            json!({
                "m_36F7B64C": {
                    "m_36F7B64C": {
                        "m_C974E369": {"m_graph": "0x058000000000180A"}
                    }
                }
            }),
        ),
        node(
            12,
            "STUStatescriptEntryGameMessage",
            None,
            json!({
                "m_gameMessage": {"m_gameMessage": format!("0x{TRIGGER_COMPLETE_MSG:016X}")},
                "m_outPlug": plug(&[(9, "m_beginPlug")], &[])
            }),
        ),
        node(
            10,
            "STUStatescriptEntryGameMessage",
            None,
            json!({
                "m_gameMessage": {"m_gameMessage": format!("0x{TRIGGER_FAULT_MSG:016X}")},
                "m_outPlug": plug(&[(11, "m_inPlug")], &[])
            }),
        ),
        node(
            11,
            "STUStatescriptEntry",
            None,
            json!({
                "m_outPlug": plug(&[(11, "m_inPlug")], &[])
            }),
        ),
        node(
            18,
            "STU_38EE1100",
            Some(3),
            json!({
                "m_gameMessage": {"m_gameMessage": format!("0x{POST_MATCH_PHASE:016X}")},
                "m_target": {
                    "$": "STU_1D719351",
                    "m_111E4A0E": 0,
                    "m_1199BFE0": 1,
                    "m_6A8DB059": 1
                },
                "m_67FD6A60": 1
            }),
        ),
        node(
            76,
            "STU_38EE1100",
            Some(25),
            json!({
                "m_gameMessage": {"m_gameMessage": format!("0x{BOSS_PAUSE:016X}")},
                "m_target": {
                    "$": "STU_1D719351",
                    "m_111E4A0E": 0,
                    "m_1199BFE0": 1,
                    "m_6A8DB059": 1
                },
                "m_67FD6A60": 1
            }),
        ),
        node(
            84,
            "STU_57692020",
            None,
            json!({
                "m_gameMessage": {"m_gameMessage": format!("0x{ENCOUNTER_RESULT:016X}")},
                "m_params": [
                    {"m_B5051BCE": "0x0A400000000003AE", "m_value": {"$": "STUConfigVarInt", "m_value": 1}},
                    {"m_B5051BCE": "0x0A400000000003AD", "m_value": {"$": "STUConfigVarInt", "m_value": 2}},
                    {"m_B5051BCE": "0x0A400000000003B4", "m_value": {"$": "STUConfigVarInt", "m_value": 100}},
                    {"m_B5051BCE": "0x0A400000000003B5", "m_value": {"$": "STUConfigVarInt", "m_value": 3}}
                ]
            }),
        ),
        remote_node(
            265,
            "STU_100EF8C2",
            None,
            0,
            json!({
                "m_gameMessage": {"m_gameMessage": "0x02400000000006E0"}
            }),
        ),
    ];

    let encounter = graph(
        0x0c8c,
        266,
        encounter_nodes,
        Json::Array(encounter_states),
        json!([0, 5, 7, 8, 10, 12]),
        json!([4, 265]),
        json!([[1303, 0, 0]]),
    );

    let post_match_child = graph(
        0x180a,
        29,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({
                    "m_outPlug": plug(&[(28, "m_inPlug")], &[])
                }),
            ),
            node(
                28,
                "STU_C43F9944",
                None,
                json!({
                    "m_gameMessage": {"m_gameMessage": format!("0x{POST_MATCH_COMPLETE:016X}")}
                }),
            ),
        ],
        json!([]),
        json!([0]),
        json!([]),
        json!([]),
    );

    let enemy_body = graph(
        0x0cbc,
        58,
        vec![
            node(
                0,
                "STUStatescriptEntryGameMessage",
                None,
                json!({
                    "m_gameMessage": {"m_gameMessage": format!("0x{ENCOUNTER_RESULT:016X}")},
                    "m_outPlug": plug(&[(57, "m_inPlug")], &[])
                }),
            ),
            node(
                57,
                "STU_523EE38D",
                None,
                json!({
                    "m_entity": {"$": "STU_B5A0CAF0"}
                }),
            ),
        ],
        json!([]),
        json!([0]),
        json!([]),
        json!([]),
    );

    let enemy_control = graph(43, 0, vec![], json!([]), json!([]), json!([]), json!([]));

    vec![
        shared,
        encounter,
        post_match_child,
        enemy_body,
        enemy_control,
    ]
}

fn synthetic_bundle() -> (Arc<GraphStore>, Arc<CodecTable>) {
    let mut graphs = vec![gate_graph_json()];
    graphs.extend(mode_graphs_json());
    let body_def = json!({
        "hero": "0x02E0000000000175",
        "name": "Synthetic Zomnic",
        "manager": null,
        "weapons": [],
        "graphs": [{"graph": 0x0cbc, "m_1EB5A024": null, "m_75408640": 0}],
        "target_tags": []
    });
    let control_def = json!({
        "hero": null,
        "name": "Synthetic Zomnic Control",
        "manager": null,
        "weapons": [],
        "ai_weapon_identifiers": {"0": [0x0d80_0000_0000_023au64]},
        "graphs": [{"graph": 43, "m_1EB5A024": null, "m_75408640": 0}],
        "target_tags": []
    });
    let bundle = json!({
        "version": 1,
        "graphs": graphs,
        "bodies": {},
        "entities": {},
        "missing": [],
        "junkenstein": {
            "pairs": {
                "0x02E0000000000175": {
                    "body": "0x04000000000016BE",
                    "controller": "0x04000000000016BF"
                }
            },
            "entities": {
                "0x04000000000016BE": body_def,
                "0x04000000000016BF": control_def
            }
        }
    });
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap());
    let codecs = Arc::new(CodecTable::from_json(ENCOUNTER_CODECS).unwrap());
    (graphs, codecs)
}

fn synthetic_gate() -> JunkensteinGate {
    JunkensteinGate {
        entity: GATE_ENTITY,
        position: [0.0, 0.0, 0.0],
        graph: 0x0ca7,
    }
}

fn build_registration(
    room: &mut RoomSimulation,
    graphs: Arc<GraphStore>,
    codecs: Arc<CodecTable>,
) -> EncounterRegistration {
    let gate = synthetic_gate();
    let mut gate_state = GateState::default();
    let mode_variables = Variables::from([(8439, Value::Entity(gate.entity))]);
    let gate_script = script_init::Component::start(
        Arc::clone(&graphs),
        gate.entity,
        Variables::new(),
        vec![Root::new(1, gate.graph)],
        0,
        &mut GateWorld {
            graphs: &graphs,
            mode: FIXTURE_MODE,
            entity: gate.entity,
            state: &mut gate_state,
            bodies: room,
            mode_variables: &mode_variables,
        },
        script_init::MAX_INIT_WORK,
    )
    .unwrap();
    let gate_variables = gate_script
        .snapshot()
        .expect("started gate script")
        .variables
        .clone();
    let anchors = SpawnAnchors::default();
    let mut result_action = None;
    let mut screens_complete = false;
    let script = {
        let mut startup = EncounterWorld {
            graphs: &graphs,
            mode: FIXTURE_MODE,
            gate: &gate,
            anchors: &anchors,
            gate_variables: &gate_variables,
            bodies: room,
            creator: None,
            result_action: &mut result_action,
            screens_complete: &mut screens_complete,
            team: 0,
            difficulty: 0,
        };
        script_init::Component::start(
            Arc::clone(&graphs),
            GAME_MODE_ENTITY,
            mode_variables,
            vec![Root::new(1, 0x0c8e), Root::new(ENCOUNTER_INSTANCE, 0x0c8c)],
            0,
            &mut startup,
            script_init::MAX_INIT_WORK,
        )
        .unwrap()
    };
    assert!(
        script.snapshot().unwrap().initialization_complete(),
        "encounter diagnostics: {:?}",
        script.snapshot().unwrap().diagnostics
    );
    EncounterRegistration {
        voice_set: crate::junkenstein_voice::ENCOUNTER_VOICE_SET,
        graphs,
        npcs: None,
        heroes: Arc::new(HeroCatalog::default()),
        movers: Arc::new(MoverCatalog::default()),
        mode: FIXTURE_MODE,
        script,
        codecs: Some(codecs),
        gate_script,
        gate_state,
        team: 0,
        difficulty: 0,
        ready_message: READY_MSG,
        ready_parameter: READY_PARAM,
        result_action: None,
        screens_complete: false,
    }
}

fn setup_registered_room() -> (RoomSimulation, Arc<GraphStore>, Arc<CodecTable>) {
    let (graphs, codecs) = synthetic_bundle();
    let mut room = RoomSimulation::new(Vec::new()).unwrap();
    room.register_junkenstein_gate(synthetic_gate(), Arc::new(SpawnAnchors::default()))
        .unwrap();
    let reg = build_registration(&mut room, Arc::clone(&graphs), Arc::clone(&codecs));
    room.register_junkenstein_encounter(reg).unwrap();
    (room, graphs, codecs)
}

#[test]
fn gate_and_encounter_hosts_read_the_selected_mode_engine_table() {
    use crate::{
        script_expr::EngineSetting,
        script_init::{Site, World},
    };

    let contract: Json = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/script-mode-rulesets-v1.json"
    ))
    .unwrap();
    let graphs =
        GraphStore::from_json(&serde_json::to_vec(&contract["input"]["bundle"]).unwrap()).unwrap();
    let gate = synthetic_gate();
    let anchors = SpawnAnchors::default();
    let variables = Variables::new();
    let mut room = RoomSimulation::new(Vec::new()).unwrap();
    let gate_site = Site {
        entity: gate.entity,
        instance: 1,
        graph: gate.graph,
        node: None,
    };
    let mode_site = Site {
        entity: GAME_MODE_ENTITY,
        instance: ENCOUNTER_INSTANCE,
        graph: 911,
        node: None,
    };
    for (mode, expected) in [(0xa1, false), (0xa4, true)] {
        let gate_world = GateWorld {
            graphs: &graphs,
            mode,
            entity: gate.entity,
            state: &mut GateState::default(),
            bodies: &room,
            mode_variables: &variables,
        };
        let expected = EngineSetting::Present(Value::Bool(expected));
        assert_eq!(gate_world.engine_setting(gate_site, 101), expected);

        let encounter = EncounterWorld {
            graphs: &graphs,
            mode,
            gate: &gate,
            anchors: &anchors,
            gate_variables: &variables,
            bodies: &mut room,
            creator: None,
            result_action: &mut None,
            screens_complete: &mut false,
            team: 0,
            difficulty: 0,
        };
        assert_eq!(encounter.engine_setting(mode_site, 101), expected);
    }
}

#[test]
fn gate_damage_stages_and_observer_projections_transition_in_ecs() {
    let (mut room, _graphs, _codecs) = setup_registered_room();
    room.set_gate_capacity(1000.0).unwrap();

    let initial = room.gate_health().expect("initial ECS gate health");
    assert_eq!(initial.remaining, 1.0);
    assert_eq!(initial.consumed, 0.0);
    assert_eq!(initial.capacity, Some(1000.0));
    assert_eq!(initial.stage, GateDamageStage::Intact);
    assert_eq!(room.gate_damage_stage(), Some(GateDamageStage::Intact));

    let collision = super::fixture_ground();
    let mover = MoverSettings::default();
    let mut stream = BodyStream::new(GATE_ENTITY);
    stream.create_arrived();
    let initial_proj = room.gate_projection().unwrap();
    let initial_obs = initial_proj
        .encoder
        .project(
            initial_proj.snapshot,
            crate::script_snapshot_wire::View::Observer,
        )
        .unwrap();
    initial_proj.encoder.full(&initial_obs).unwrap();
    let full_chunk = initial_proj
        .encoder
        .queue_full(&mut stream, &initial_obs)
        .unwrap();
    stream.arrived(full_chunk.last);

    let mut previous_obs = initial_obs;
    for (tick, expected_remaining, expected_consumed, expected_stage, alive) in [
        (1, 0.75, 250.0, GateDamageStage::Stage1, true),
        (2, 0.50, 500.0, GateDamageStage::Stage2, true),
        (3, 0.25, 750.0, GateDamageStage::Stage3, true),
        (4, 0.0, 1000.0, GateDamageStage::Destroyed, false),
    ] {
        room.queue_scene_message(SceneMessage {
            target: GATE_ENTITY,
            message: GATE_DAMAGE_MSG,
            parameters: Variables::from([(0x03ac, Value::Float(250.0))]),
        });
        room.step_encounter(&collision, &mover, tick, None).unwrap();

        let health = room.gate_health().unwrap();
        assert_eq!(health.remaining, expected_remaining);
        assert_eq!(health.consumed, expected_consumed);
        assert_eq!(health.capacity, Some(1000.0));
        assert_eq!(health.stage, expected_stage);
        assert_eq!(room.gate_damage_stage(), Some(expected_stage));

        let snapshot = room.gate_snapshot().unwrap();
        assert_eq!(
            snapshot.instances[&1].states[&2].kind,
            StateKind::BooleanSwitch(alive)
        );

        let proj = room.gate_projection().unwrap();
        let next_obs = proj
            .encoder
            .project_next(proj.snapshot, &previous_obs)
            .unwrap();
        assert_ne!(
            next_obs, previous_obs,
            "stage {expected_stage:?} must change observer state"
        );
        let delta = proj
            .encoder
            .queue_delta(
                &mut stream,
                &next_obs,
                DeltaKind::Observer { resend: true },
                16,
            )
            .unwrap()
            .expect("observer delta emitted on stage transition");
        stream.arrived(delta.last);
        previous_obs = next_obs;
    }
}

#[test]
fn encounter_phases_and_vm_event_order_preserve_step_sequence() {
    let (mut room, graphs, codecs) = setup_registered_room();
    let collision = super::fixture_ground();
    let mover = MoverSettings::default();
    let movers = MoverCatalog::default();
    let queries = BotQueries::new(false, &movers, &collision, None);

    let definition = NpcDefinition {
        hero: 0x02e0_0000_0000_0175,
        body: 0x0400_0000_0000_16be,
        name: "Synthetic Zomnic".into(),
        pools: crate::game_pools::HealthPools {
            health: 200.0,
            armour: 0.0,
            shields: 0.0,
        },
    };
    let request = script_init::EntityCreation {
        position: [1.0, 0.0, 1.0],
        kind: script_init::CreationKind::Hero {
            hero: definition.hero,
            team: 1,
            yaw: 0.0,
            flags: [Value::Bool(false), Value::Bool(false)],
            controller: None,
            skin: None,
        },
    };
    let enemy_wire = room
        .create_enemy(crate::script_enemy::EnemySpawn {
            graphs: Arc::clone(&graphs),
            codecs: Arc::clone(&codecs),
            definition: &definition,
            request: &request,
            mode: FIXTURE_MODE,
            mode_variables: Arc::new(Variables::new()),
            tick: 1,
            walk: BotWalk::default(),
            gate: None,
        })
        .unwrap();

    assert_eq!(room.encounter_phase(), Some(EncounterPhase::PendingStart));

    // Tick 1: Countdown active.
    room.step_encounter(&collision, &mover, 1, None).unwrap();
    assert_eq!(room.encounter_phase(), Some(EncounterPhase::Countdown));
    assert!(room.encounter_countdown().is_some());
    assert!(room.encounter_active());

    // Tick 2: Clear countdown, damage gate, verify E2 -> E5 -> E6 & E7 arrive before E9.
    room.queue_scene_message(SceneMessage {
        target: GAME_MODE_ENTITY,
        message: CLEAR_COUNTDOWN_MSG,
        parameters: Variables::new(),
    });
    room.queue_scene_message(SceneMessage {
        target: GATE_ENTITY,
        message: GATE_DAMAGE_MSG,
        parameters: Variables::from([(0x03ac, Value::Float(250.0))]),
    });
    room.step_encounter(&collision, &mover, 2, None).unwrap();
    assert_eq!(room.encounter_phase(), Some(EncounterPhase::Active));
    let mode_snapshot = room.encounter_snapshot().unwrap();
    assert_eq!(
        mode_snapshot.variables.get(&9001),
        Some(&Value::Integer(1)),
        "E6 foreign variable change from gate 8571 must reach mode before E9"
    );
    assert_eq!(
        mode_snapshot.variables.get(&9002),
        Some(&Value::Integer(1)),
        "E7 gate->mode message must reach mode before E9"
    );

    // Tick 3: Trigger BOSS_PAUSE, result_action, and ENCOUNTER_RESULT -> PostMatch.
    room.queue_scene_message(SceneMessage {
        target: GAME_MODE_ENTITY,
        message: TRIGGER_END_MSG,
        parameters: Variables::new(),
    });
    room.step_encounter(&collision, &mover, 3, None).unwrap();
    let diag = &room.encounter_snapshot().unwrap().diagnostics;
    assert!(
        diag.is_empty(),
        "unexpected encounter diagnostics: {diag:?}"
    );
    assert!(room.encounter_boss_pause(), "E10 must publish BOSS_PAUSE");
    assert_eq!(room.encounter_phase(), Some(EncounterPhase::PostMatch));
    assert!(!room.encounter_active());
    assert!(!room.encounter_screens_complete());
    assert!(!room.take_defender_messages().is_empty());

    // Tick 4 + finish_tick: E2->E5 delivers ENCOUNTER_RESULT to gate_script; 0x0cbc enemy retires; 0x180a completes screens.
    room.queue_scene_message(SceneMessage {
        target: GAME_MODE_ENTITY,
        message: TRIGGER_COMPLETE_MSG,
        parameters: Variables::new(),
    });
    room.step_encounter(&collision, &mover, 4, None).unwrap();
    let gate_snap = room.gate_snapshot().unwrap();
    assert_eq!(
        gate_snap.instances[&1].variables.get(&1303),
        Some(&Value::Integer(3))
    );
    assert_eq!(room.encounter_phase(), Some(EncounterPhase::Complete));
    assert!(room.encounter_screens_complete());
    room.finish_tick(&queries, 4, Duration::from_millis(48))
        .unwrap();
    assert_eq!(room.take_retired_enemies(), vec![enemy_wire]);
}

#[test]
fn encounter_init_fault_retires_encounter_and_clears_boss_pause() {
    let (mut room, _graphs, _codecs) = setup_registered_room();
    let collision = super::fixture_ground();
    let mover = MoverSettings::default();
    let movers = MoverCatalog::default();
    let queries = BotQueries::new(false, &movers, &collision, None);

    // Enter BOSS_PAUSE first.
    room.queue_scene_message(SceneMessage {
        target: GAME_MODE_ENTITY,
        message: TRIGGER_END_MSG,
        parameters: Variables::new(),
    });
    room.step_encounter(&collision, &mover, 1, None).unwrap();
    assert!(room.encounter_boss_pause());

    // Trigger an infinite plug cycle to force an InitFault during encounter advance_frame.
    room.queue_scene_message(SceneMessage {
        target: GAME_MODE_ENTITY,
        message: TRIGGER_FAULT_MSG,
        parameters: Variables::new(),
    });
    room.step_encounter(&collision, &mover, 2, None).unwrap();

    assert!(room.encounter_snapshot().is_none());
    assert!(room.encounter_phase().is_none());
    assert!(!room.encounter_boss_pause());
    assert!(room.junkenstein_gate().is_some());
    assert!(room.gate_snapshot().is_none());
    room.finish_tick(&queries, 2, Duration::from_millis(16))
        .unwrap();
}

#[test]
fn gate_and_encounter_registration_roll_back_cleanly_on_invalid_inputs() {
    let (graphs, codecs) = synthetic_bundle();
    let mut room = RoomSimulation::new(Vec::new()).unwrap();
    let initial_entities = room.world.entities().len();

    // Missing gate before encounter registration.
    let reg_without_gate = build_registration(&mut room, Arc::clone(&graphs), Arc::clone(&codecs));
    assert!(matches!(
        room.register_junkenstein_encounter(reg_without_gate),
        Err(CombatError::MissingEncounterGate)
    ));
    assert_eq!(room.world.entities().len(), initial_entities);
    assert!(room.mode_voice_set.is_none());

    // Invalid gate entity == 0 and NaN position.
    let bad_entity = JunkensteinGate {
        entity: 0,
        position: [0.0; 3],
        graph: 0x0ca7,
    };
    assert!(matches!(
        room.register_junkenstein_gate(bad_entity, Arc::new(SpawnAnchors::default())),
        Err(CombatError::InvalidGate(_))
    ));
    assert_eq!(room.world.entities().len(), initial_entities);
    assert!(room.spawn_anchors().is_none());

    let bad_pos = JunkensteinGate {
        entity: GATE_ENTITY,
        position: [f64::NAN, 0.0, 0.0],
        graph: 0x0ca7,
    };
    assert!(matches!(
        room.register_junkenstein_gate(bad_pos, Arc::new(SpawnAnchors::default())),
        Err(CombatError::InvalidGate(_))
    ));
    assert_eq!(room.world.entities().len(), initial_entities);
    assert!(room.spawn_anchors().is_none());

    // Register valid gate, then reject duplicate gate registration.
    room.register_junkenstein_gate(synthetic_gate(), Arc::new(SpawnAnchors::default()))
        .unwrap();
    let after_gate_entities = room.world.entities().len();
    assert!(matches!(
        room.register_junkenstein_gate(synthetic_gate(), Arc::new(SpawnAnchors::default())),
        Err(CombatError::GateAlreadyRegistered(GATE_ENTITY))
    ));
    assert_eq!(room.world.entities().len(), after_gate_entities);

    // Reject encounter with invalid gate team != Some(2) without leaking entities or voice_set.
    let mut bad_reg = build_registration(&mut room, Arc::clone(&graphs), Arc::clone(&codecs));
    bad_reg.gate_state.team = Some(1);
    assert!(matches!(
        room.register_junkenstein_encounter(bad_reg),
        Err(CombatError::InvalidEncounter(_))
    ));
    assert_eq!(room.world.entities().len(), after_gate_entities);
    assert!(room.encounter_snapshot().is_none());
    assert!(room.mode_voice_set.is_none());

    // Register valid encounter, then reject duplicate registration.
    let valid_reg = build_registration(&mut room, Arc::clone(&graphs), Arc::clone(&codecs));
    room.register_junkenstein_encounter(valid_reg).unwrap();
    let after_enc_entities = room.world.entities().len();
    let dup_reg = build_registration(&mut room, Arc::clone(&graphs), Arc::clone(&codecs));
    assert!(matches!(
        room.register_junkenstein_encounter(dup_reg),
        Err(CombatError::EncounterAlreadyRegistered)
    ));
    assert_eq!(room.world.entities().len(), after_enc_entities);
}

#[test]
fn set_gate_capacity_rejects_invalid_values_without_mutating_ecs_state() {
    let (mut room, _graphs, _codecs) = setup_registered_room();
    room.set_gate_capacity(1000.0).unwrap();

    for invalid_capacity in [0.0, -100.0, f64::NAN, f64::INFINITY] {
        assert!(
            room.set_gate_capacity(invalid_capacity).is_err(),
            "capacity {invalid_capacity} must fail"
        );
        assert_eq!(room.gate_health().unwrap().capacity, Some(1000.0));
        assert_eq!(
            room.gate_snapshot()
                .unwrap()
                .resolved_bindings
                .get(&(0, 7786)),
            Some(&Value::Float(1000.0))
        );
    }
}

#[test]
fn finish_tick_catches_corrupted_ecs_gate_and_encounter_components_as_combat_error() {
    let collision = super::fixture_ground();
    let movers = MoverCatalog::default();
    let queries = BotQueries::new(false, &movers, &collision, None);

    let (mut missing_gate_room, _graphs, _codecs) = setup_registered_room();
    assert!(missing_gate_room.invalidate_gate_component());
    assert!(matches!(
        missing_gate_room.finish_tick(&queries, 1, Duration::ZERO),
        Err(CombatError::MissingGate {
            wire: GATE_ENTITY,
            ..
        })
    ));

    let (mut bad_identity_room, _graphs, _codecs) = setup_registered_room();
    assert!(bad_identity_room.corrupt_gate_identity(GATE_ENTITY + 1));
    assert!(matches!(
        bad_identity_room.finish_tick(&queries, 1, Duration::ZERO),
        Err(CombatError::GateIdentity {
            wire: GATE_ENTITY,
            actual,
        }) if actual == GATE_ENTITY + 1
    ));

    let (mut missing_encounter_room, _graphs, _codecs) = setup_registered_room();
    assert!(missing_encounter_room.invalidate_encounter_component());
    assert!(matches!(
        missing_encounter_room.finish_tick(&queries, 1, Duration::ZERO),
        Err(CombatError::MissingEncounter { .. })
    ));
}

fn setup_room_with_gate_graph(
    gate_graph: Json,
) -> (RoomSimulation, Arc<GraphStore>, Arc<CodecTable>) {
    let mut graphs = vec![gate_graph];
    graphs.extend(mode_graphs_json());
    let bundle = json!({
        "version": 1,
        "graphs": graphs,
        "bodies": {},
        "entities": {},
        "missing": []
    });
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap());
    let codecs = Arc::new(CodecTable::from_json(ENCOUNTER_CODECS).unwrap());
    let mut room = RoomSimulation::new(Vec::new()).unwrap();
    room.register_junkenstein_gate(synthetic_gate(), Arc::new(SpawnAnchors::default()))
        .unwrap();
    let reg = build_registration(&mut room, Arc::clone(&graphs), Arc::clone(&codecs));
    room.register_junkenstein_encounter(reg).unwrap();
    (room, graphs, codecs)
}

#[test]
fn set_gate_capacity_rolls_back_on_vm_init_fault_and_binding_failure() {
    let collision = super::fixture_ground();
    let mover = MoverSettings::default();

    // Part 1: VM InitFault triggered by watching instance variable 8567 >= 1200.0.
    let mut fault_gate = gate_graph_json();
    fault_gate["items"] = json!(27);
    fault_gate["nodes"].as_array_mut().unwrap().extend([
        node(
            23,
            "STUStatescriptStateBooleanSwitch",
            Some(11),
            json!({
                "m_condition": {
                    "$": "STU_919FD47C",
                    "m_expression": {
                        "m_opcodes": [8, 0, 6, 0, 36, 0],
                        "m_dynamicVars": [var(8567, false)],
                        "m_D99EF254": [1200.0]
                    }
                },
                "m_trueSubgraphPlug": plug(&[(24, "m_beginPlug")], &[12])
            }),
        ),
        node(
            24,
            "STUStatescriptState",
            Some(12),
            json!({
                "m_onBeginPlug": plug(&[(25, "m_inPlug")], &[])
            }),
        ),
        node(
            25,
            "STUStatescriptEntry",
            None,
            json!({
                "m_outPlug": plug(&[(25, "m_inPlug")], &[])
            }),
        ),
    ]);
    fault_gate["states"]
        .as_array_mut()
        .unwrap()
        .extend([json!(23), json!(24)]);
    fault_gate["nodes"][0]["fields"]["m_outPlug"]["links"]
        .as_array_mut()
        .unwrap()
        .push(json!([23, "m_beginPlug"]));

    let (mut fault_room, _graphs, _codecs) = setup_room_with_gate_graph(fault_gate);
    fault_room.set_gate_capacity(1000.0).unwrap();
    let before_health = fault_room.gate_health().unwrap();

    assert!(
        fault_room.set_gate_capacity(1500.0).is_err(),
        "capacity crossing fault threshold must fail"
    );
    assert!(
        fault_room.gate_snapshot().is_some(),
        "failed set_gate_capacity must roll back without leaving gate script halted"
    );
    assert_eq!(fault_room.gate_health(), Some(before_health));
    assert_eq!(
        fault_room.gate_resolved_variable(
            1,
            Variable {
                scope: Scope::Instance,
                id: 8567,
            },
        ),
        Some(Value::Float(1000.0))
    );
    assert_eq!(
        fault_room.gate_resolved_variable(
            0,
            Variable {
                scope: Scope::Entity,
                id: 7786,
            },
        ),
        Some(Value::Float(1000.0))
    );
    assert!(fault_room.encounter_snapshot().is_some());
    fault_room
        .step_encounter(&collision, &mover, 1, None)
        .unwrap();
    assert!(fault_room.gate_snapshot().is_some());
    assert!(fault_room.encounter_snapshot().is_some());

    // Part 2: binding check failure when stack node 5 is disconnected from entry 0.
    let mut unbound_gate = gate_graph_json();
    unbound_gate["nodes"][0]["fields"]["m_outPlug"]["links"]
        .as_array_mut()
        .unwrap()
        .retain(|edge| edge[0] != 5);

    let (mut unbound_room, _graphs, _codecs) = setup_room_with_gate_graph(unbound_gate);
    let before_unbound_health = unbound_room.gate_health().unwrap();
    assert_eq!(
        unbound_room.gate_resolved_variable(
            1,
            Variable {
                scope: Scope::Instance,
                id: 8567,
            },
        ),
        Some(Value::Null)
    );

    assert!(
        unbound_room.set_gate_capacity(500.0).is_err(),
        "missing stack binding for Entity:7786 must fail"
    );
    assert!(unbound_room.gate_snapshot().is_some());
    assert_eq!(unbound_room.gate_health(), Some(before_unbound_health));
    assert_eq!(
        unbound_room.gate_resolved_variable(
            1,
            Variable {
                scope: Scope::Instance,
                id: 8567,
            },
        ),
        Some(Value::Null)
    );
    assert_eq!(
        unbound_room.gate_resolved_variable(
            0,
            Variable {
                scope: Scope::Entity,
                id: 7786,
            },
        ),
        Some(Value::Float(1000.0))
    );
    unbound_room
        .step_encounter(&collision, &mover, 1, None)
        .unwrap();
}

#[test]
fn step_encounter_retires_halted_gate_script_without_panicking() {
    let collision = super::fixture_ground();
    let mover = MoverSettings::default();

    let mut fault_gate = gate_graph_json();
    fault_gate["items"] = json!(25);
    fault_gate["nodes"].as_array_mut().unwrap().extend([
        node(
            23,
            "STUStatescriptEntryGameMessage",
            None,
            json!({
                "m_gameMessage": {"m_gameMessage": format!("0x{TRIGGER_FAULT_MSG:016X}")},
                "m_outPlug": plug(&[(24, "m_inPlug")], &[])
            }),
        ),
        node(
            24,
            "STUStatescriptEntry",
            None,
            json!({
                "m_outPlug": plug(&[(24, "m_inPlug")], &[])
            }),
        ),
    ]);
    fault_gate["entries"]
        .as_array_mut()
        .unwrap()
        .push(json!(23));

    let (mut room, _graphs, _codecs) = setup_room_with_gate_graph(fault_gate);
    assert_eq!(
        room.step_test_gate_frame(1, TRIGGER_FAULT_MSG, Variables::new()),
        Err(InitFault::PlugDepth)
    );
    assert!(room.gate_snapshot().is_none());
    assert!(room.encounter_snapshot().is_some());

    room.step_encounter(&collision, &mover, 2, None).unwrap();
    assert!(room.encounter_snapshot().is_none());
    assert!(room.encounter_phase().is_none());
    assert!(!room.encounter_boss_pause());
    assert!(room.junkenstein_gate().is_some());
    assert!(room.gate_snapshot().is_none());
}

#[test]
fn gate_health_from_snapshot_ignores_inactive_switch_and_treats_nan_as_destroyed() {
    let (room, graphs, _codecs) = setup_registered_room();
    let base_snapshot = room.gate_snapshot().unwrap().clone();

    // Inactive BooleanSwitch(false) on state 2 with remaining == 1.0 must stay Intact.
    let mut inactive_switch_snapshot = base_snapshot.clone();
    inactive_switch_snapshot
        .variables
        .insert(8571, Value::Float(1.0));
    {
        let inst = inactive_switch_snapshot.instances.get_mut(&1).unwrap();
        let state2 = inst.states.get_mut(&2).unwrap();
        state2.active = false;
        state2.kind = StateKind::BooleanSwitch(false);
        for raw in [3u16, 4, 5, 6] {
            if let Some(state) = inst.states.get_mut(&raw) {
                state.active = false;
            }
        }
    }
    let inactive_health = GateHealth::from_snapshot(&inactive_switch_snapshot, &graphs, 0x0ca7);
    assert_eq!(inactive_health.stage, GateDamageStage::Intact);

    // Active BooleanSwitch(false) on state 2 must report Destroyed.
    let mut active_switch_snapshot = inactive_switch_snapshot.clone();
    active_switch_snapshot
        .instances
        .get_mut(&1)
        .unwrap()
        .states
        .get_mut(&2)
        .unwrap()
        .active = true;
    let active_health = GateHealth::from_snapshot(&active_switch_snapshot, &graphs, 0x0ca7);
    assert_eq!(active_health.stage, GateDamageStage::Destroyed);

    // NaN remaining without active stage nodes must report Destroyed, never Stage3.
    let mut nan_snapshot = inactive_switch_snapshot;
    nan_snapshot.variables.insert(8571, Value::Float(f64::NAN));
    let nan_health = GateHealth::from_snapshot(&nan_snapshot, &graphs, 0x0ca7);
    assert_eq!(nan_health.stage, GateDamageStage::Destroyed);
}
