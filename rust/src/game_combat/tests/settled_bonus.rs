use serde::Deserialize;
use serde_json::{Value as Json, json};

use super::*;
use crate::{
    contact_world::ContactWorld,
    game_clock::TICK_PERIOD,
    game_content::PRACTICE_RANGE_MODE,
    game_heroes::Hero,
    game_replication::{BodyStep, OwnerRoomActivity},
    mover_settings::{MoverCatalog, MoverSettings},
    script_codecs::CodecTable,
    script_expr::to_float,
    script_graph::GraphStore,
    script_player::{OwnerBodySetup, OwnerBodySpawn, PlayerBodyFactory, fixture_body_factory},
};

const GRAPH: u32 = 628;
const HERO: u64 = 0x02e0_0000_0052_0031;
const BODY: u64 = 0x0400_0000_0052_0032;
const TOPIC: &str = "0x0240000000520033";
const AMOUNT: &str = "0x0A40000000005234";
const SOURCE_SLOT: u32 = 10;
const TARGET_SLOT: u32 = 11;
const DELTA_SLOT: u32 = 12;
const GRANT_SLOT: u32 = 13;
const METER_SLOT: u32 = 14;

#[derive(Deserialize)]
struct Contract {
    input: Input,
    expected: Expected,
}

#[derive(Deserialize)]
struct Input {
    damage: f64,
    heal_request: f64,
    meter_cap: f64,
}

#[derive(Deserialize)]
struct Expected {
    after_deliveries: [f64; 2],
    replacement_meter: f64,
    replacement_before_delivery_meter: f64,
    fresh_grant_after_replacement_meter: f64,
}

fn bonus_bundle(cap: f64) -> GraphStore {
    let variable = |id: u32| {
        json!({
            "$": "STUConfigVarDynamic", "m_identifier": id, "m_60DB8F99": false
        })
    };
    let binding = |parameter, slot| {
        json!({
            "m_B5051BCE": parameter, "m_out_Var": variable(slot)
        })
    };
    let plug = |next: Option<u16>| {
        let links = next.map_or_else(Vec::new, |next| vec![(next, "m_inPlug")]);
        json!({"$": "STUStatescriptOutputPlug", "links": links})
    };
    let message = json!({"$":"STUConfigVarGameMessage", "m_gameMessage":TOPIC});
    let node = |pos: u16, class: &str, server_only: bool, fields: Json| {
        json!({
            "pos":pos, "uid":pos, "rid":pos, "state":null, "remote":-1,
            "class":class, "client_only":false, "server_only":server_only, "fields":fields
        })
    };
    let branch = |pos, condition, next| {
        let mut node = node(
            pos,
            "fixture.bonus_gate",
            true,
            json!({
                "m_condition":condition, "m_truePlug":plug(Some(next)), "m_falsePlug":plug(None)
            }),
        );
        node["action_behavior"] = json!("branch");
        node
    };
    let set_meter = |pos, amount| {
        node(
            pos,
            "STUStatescriptActionSetVar",
            true,
            json!({
                "m_ADEB6E05":true, "m_out_Var":variable(METER_SLOT), "m_value":amount
            }),
        )
    };
    let expression = |opcodes, vars, constants, configs| {
        json!({
            "$":"STU_919FD47C", "m_configVars":configs, "m_expression":{
                "m_opcodes":opcodes, "m_dynamicVars":vars, "m_D99EF254":constants
            }
        })
    };
    let mut health_event = node(
        2,
        "EntryHostEvent",
        true,
        json!({
            "m_params":[binding("0x0000000000000001",SOURCE_SLOT),
                        binding("0x0000000000000002",TARGET_SLOT),
                        binding("0x0000000000000003",DELTA_SLOT)],
            "m_outPlug":plug(Some(3))
        }),
    );
    health_event["host_event_entry"] = json!("health_applied");
    let nodes = [
        node(
            0,
            "STUStatescriptEntry",
            true,
            json!({"m_outPlug":plug(Some(1))}),
        ),
        set_meter(1, json!(0.0)),
        health_event,
        branch(
            3,
            expression(
                vec![8, 0, 14, 0, 37, 0],
                vec![variable(SOURCE_SLOT)],
                vec![],
                vec![json!({"$":"STU_B5A0CAF0"})],
            ),
            4,
        ),
        branch(
            4,
            expression(
                vec![8, 0, 6, 0, 36, 0],
                vec![variable(DELTA_SLOT)],
                vec![0.0],
                vec![],
            ),
            5,
        ),
        node(
            5,
            "STUStatescriptActionSendGameMessage",
            false,
            json!({
                "m_ADEB6E05":1, "m_gameMessage":message, "m_targets":variable(TARGET_SLOT),
                "m_params":[{"$":"STU_814D595E", "m_B5051BCE":AMOUNT, "m_value":variable(DELTA_SLOT)}],
                "m_outPlug":plug(None)
            }),
        ),
        node(
            6,
            "STUStatescriptEntryGameMessage",
            true,
            json!({
                "m_gameMessage":message,
                "m_params":[{"$":"STU_EF930FD7", "m_B5051BCE":AMOUNT, "m_out_Var":variable(GRANT_SLOT)}],
                "m_outPlug":plug(Some(7))
            }),
        ),
        branch(7, json!({"$":"BodyActive"}), 8),
        set_meter(
            8,
            expression(
                vec![8, 0, 8, 1, 23, 6, 0, 6, 1, 65, 0],
                vec![variable(METER_SLOT), variable(GRANT_SLOT)],
                vec![0.0, cap],
                vec![],
            ),
        ),
    ];
    let bundle = json!({
        "version":1, "missing":[],
        "runtime_state_metadata":{"directed_numeric_messages":[{"topic":TOPIC,"parameter":AMOUNT}]},
        "graphs":[{
            "index":GRAPH, "items":nodes.len(), "fields":{}, "containers":{},
            "states":[], "remote":[], "sync_vars":[], "unknown_nodes":[], "entries":[0,2,6],
            "header":{"m_nodesBitCount":4,"m_statesBitCount":4,
                "m_remoteSyncNodesBitCount":4,"m_syncVarsBitCount":4},
            "nodes":nodes
        }],
        "bodies":{
            format!("0x{BODY:016X}"): {
                "hero":format!("0x{HERO:016X}"), "name":"Fixture", "graphs":[],
                "server_graphs":[{"graph":GRAPH,"m_75408640":0}],
                "authored_charge_graph":format!("0x{:016X}", 0x0580_0000_0000_0000 | u64::from(GRAPH)),
                "manager":null, "weapons":[]
            }
        }
    });
    GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap()
}

fn attach(
    room: &mut RoomSimulation,
    factory: &PlayerBodyFactory,
    template: OwnerBodySpawn,
    slot: u8,
    frame: u32,
    generation: u64,
) -> BodyId {
    let offset = u32::from(slot) * 2;
    let mut spawn = OwnerBodySpawn {
        controller: template.controller + offset,
        entity: template.entity + offset,
        slot,
        frame,
        ..template
    };
    spawn.pose.position[0] += f64::from(slot);
    let runtime = factory
        .start_with_setup(
            spawn,
            OwnerBodySetup {
                room_activity: Some(OwnerRoomActivity::Regular),
                ..OwnerBodySetup::default()
            },
        )
        .unwrap();
    let id = BodyId {
        entity: spawn.entity,
        generation,
    };
    let mover = Mover::new(
        MoverSettings::default(),
        Arc::new(ContactWorld::new(Vec::new()).unwrap()),
        spawn.pose.position,
        0,
        MOVER_CATEGORY,
    )
    .unwrap();
    room.spawn_player_body(PlayerBodyState::new(
        id,
        spawn.controller,
        slot,
        0,
        HERO,
        mover,
        Some(Box::new(runtime)),
        frame,
        None,
    ))
    .unwrap();
    id
}

fn advance(room: &mut RoomSimulation, frame: u32, ids: [BodyId; 3]) {
    room.prepare_spatial_frame(frame).unwrap();
    for id in ids {
        let actor = room.player_body_mut(id).unwrap();
        actor
            .runtime
            .as_mut()
            .unwrap()
            .advance(
                Command {
                    frame,
                    ..Command::default()
                },
                &actor.mover,
                BodyStep::UnposedTimer,
            )
            .unwrap();
        actor.frame = frame;
    }
}

fn meter(room: &RoomSimulation, id: BodyId) -> f64 {
    let snapshot = room
        .player_body(id)
        .unwrap()
        .runtime
        .as_ref()
        .unwrap()
        .snapshot()
        .unwrap();
    assert!(
        snapshot.diagnostics.is_empty(),
        "{:?}",
        snapshot.diagnostics
    );
    let instance = snapshot
        .instances
        .values()
        .find(|instance| instance.graph == GRAPH)
        .unwrap();
    to_float(&instance.variables[&METER_SLOT])
}

fn wound_and_heal(
    room: &mut RoomSimulation,
    input: &Input,
    source: BodyId,
    target: BodyId,
    frame: u32,
) {
    let now = frame * TICK_PERIOD;
    let attribution = Death {
        frame,
        time: now.as_secs_f64(),
        killer: source.entity,
    };
    assert!(
        room.settle_health(
            target.entity,
            HealthEffect::Damage(input.damage),
            attribution,
            now
        )
        .unwrap()
    );
    assert!(
        room.settle_health(
            target.entity,
            HealthEffect::Heal(input.heal_request),
            attribution,
            now
        )
        .unwrap()
    );
}

#[test]
fn settled_healing_binds_a_directed_bonus_through_the_live_room() {
    let contract: Contract = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/script-settled-bonus-v1.json"
    ))
    .unwrap();
    let graph_store = Arc::new(bonus_bundle(contract.input.meter_cap));
    assert!(graph_store.uses_host_health_events());
    assert_eq!(graph_store.room_spatial_needs(), RoomSpatialNeeds::Queries);
    let ground = Arc::new(fixture_ground());
    let movers = MoverCatalog::default();
    let queries = BotQueries::new(false, &movers, ground.as_ref(), None);
    let mut room = RoomSimulation::new(Vec::new()).unwrap();
    room.enable_host_health_events();
    room.enable_spatial_queries(false, Arc::clone(&ground), graph_store.room_spatial_needs());

    let (_, template) = fixture_body_factory();
    let codecs = Arc::new(CodecTable::from_json(r#"{"classes":{}}"#).unwrap());
    let hero = Hero {
        guid: HERO,
        name: "Fixture".into(),
        role: "Support".into(),
        body: BODY,
    };
    let factory = PlayerBodyFactory::prepare(
        graph_store,
        codecs,
        PRACTICE_RANGE_MODE,
        &hero,
        None,
        false,
        (template, OwnerBodySetup::default()),
    )
    .unwrap();
    let mut frame = template.frame;
    let source = attach(&mut room, &factory, template, 0, frame, 1);
    let recipient = attach(&mut room, &factory, template, 1, frame, 1);
    let bystander = attach(&mut room, &factory, template, 2, frame, 1);
    let actors = [source, recipient, bystander];
    let finish = |room: &mut RoomSimulation, frame| {
        room.finish_tick(&queries, frame, frame * TICK_PERIOD)
            .unwrap();
    };
    let mut before = 0.0;
    for expected in contract.expected.after_deliveries {
        wound_and_heal(&mut room, &contract.input, source, recipient, frame);
        finish(&mut room, frame);
        assert_eq!(meter(&room, recipient), before);
        advance(&mut room, frame + 1, actors);
        finish(&mut room, frame + 1);
        assert_eq!(meter(&room, recipient), before);
        advance(&mut room, frame + 2, actors);
        assert_eq!(meter(&room, recipient), expected);
        assert_eq!(meter(&room, source), 0.0);
        assert_eq!(meter(&room, bystander), 0.0);
        frame += 2;
        before = expected;
    }

    wound_and_heal(&mut room, &contract.input, source, recipient, frame);
    finish(&mut room, frame);
    advance(&mut room, frame + 1, actors);
    let replacement = attach(&mut room, &factory, template, 1, frame + 1, 2);
    assert_eq!(replacement.entity, recipient.entity);
    assert!(room.player_body(recipient).is_none());
    finish(&mut room, frame + 1);
    advance(&mut room, frame + 2, [source, replacement, bystander]);
    assert_eq!(
        meter(&room, replacement),
        contract.expected.replacement_meter
    );

    // The source already has the health event when the target changes bodies.
    frame += 2;
    wound_and_heal(&mut room, &contract.input, source, replacement, frame);
    finish(&mut room, frame);
    let next = attach(&mut room, &factory, template, 1, frame, 3);
    assert_eq!(next.entity, replacement.entity);
    assert!(room.player_body(replacement).is_none());
    let actors = [source, next, bystander];
    advance(&mut room, frame + 1, actors);
    finish(&mut room, frame + 1);
    advance(&mut room, frame + 2, actors);
    assert_eq!(
        meter(&room, next),
        contract.expected.replacement_before_delivery_meter
    );
    assert_eq!(meter(&room, source), 0.0);
    assert_eq!(meter(&room, bystander), 0.0);

    frame += 2;
    wound_and_heal(&mut room, &contract.input, source, next, frame);
    finish(&mut room, frame);
    advance(&mut room, frame + 1, actors);
    finish(&mut room, frame + 1);
    advance(&mut room, frame + 2, actors);
    assert_eq!(
        meter(&room, next),
        contract.expected.fresh_grant_after_replacement_meter
    );
    assert_eq!(meter(&room, source), 0.0);
    assert_eq!(meter(&room, bystander), 0.0);
}
