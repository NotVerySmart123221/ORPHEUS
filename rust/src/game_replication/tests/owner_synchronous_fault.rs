use super::*;
use crate::{
    game_combat::PlayerBodyEvent,
    game_content::JUNKENSTEIN_REVENGE,
    game_heroes::Hero,
    game_pools::HealthPools,
    pve_encounter::messages::{DefenderMessage, DefenderSighting, SIGHTING, SightingSource},
    script_player::{OwnerBodySpawn, PlayerBodyFactory},
    world::Create,
};
use serde_json::{Value as Json, json};

const MESSAGE: u64 = 0x0240_0000_0000_6ff1;

fn factory(health: f64) -> PlayerBodyFactory {
    let hero = Hero {
        guid: SOLDIER,
        name: "Defender fixture".to_owned(),
        role: "Damage".to_owned(),
        body: 0x0400_0000_0000_b193,
    };
    let graph = |index: u32, nodes: Vec<Json>, entries: Json| {
        json!({
            "index": index, "items": nodes.len(), "nodes": nodes, "states": [],
            "entries": entries, "remote": [], "sync_vars": [], "containers": {},
            "unknown_nodes": [], "fields": {},
            "header": {"m_nodesBitCount": 4,"m_statesBitCount": 4,
                "m_remoteSyncNodesBitCount": 4,"m_syncVarsBitCount": 4}
        })
    };
    let output = json!({"$": "Plug", "links": [[2, "m_inPlug"]], "m_FF3DAF1E": []});
    let node = |pos: usize, class: &str, mut fields: Json| {
        fields["m_2BBEEAB8"] = json!(false);
        fields["m_ADEB6E05"] = json!(false);
        json!({
            "pos": pos, "uid": pos, "rid": pos, "state": null, "remote": -1,
            "client_only": false, "server_only": false, "class": class, "fields": fields
        })
    };
    let receiver = |pos, message| {
        node(
            pos,
            "STUStatescriptEntryGameMessage",
            json!({
                "m_gameMessage": {"m_gameMessage": format!("0x{message:016X}")},
                "m_outPlug": output,
            }),
        )
    };
    let root = 42;
    let data = json!({
        "version": 1, "missing": [],
        "graphs": [
            graph(root, Vec::new(), json!([])),
            graph(0x0ca5, vec![
                receiver(0, MESSAGE),
                receiver(1, SIGHTING),
                node(2, "STUStatescriptEntry", json!({"m_outPlug": output})),
            ], json!([0, 1])),
        ],
        "bodies": {format!("0x{:016X}", hero.body): {
            "hero": format!("0x{:016X}", hero.guid), "name": &hero.name,
            "graphs": [{"graph": root, "m_75408640": 0}], "manager": root, "weapons": [null, root]
        }}
    });
    PlayerBodyFactory::from_body(
        Arc::new(GraphStore::from_json(&serde_json::to_vec(&data).unwrap()).unwrap()),
        Arc::new(CodecTable::from_json(r#"{"classes":{}}"#).unwrap()),
        JUNKENSTEIN_REVENGE,
        &hero,
        Some(HealthPools {
            health,
            armour: 0.0,
            shields: 0.0,
        }),
        false,
    )
    .unwrap()
}

#[test]
fn synchronous_defender_fault_retires_the_exact_owner_script_before_identity_ack() {
    let contract: Json = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/owner-synchronous-script-halt-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "owner-synchronous-script-halt-v1");
    let input = &contract["input"];
    let expected = &contract["expected"];
    let factory = factory(input["health"].as_f64().unwrap());
    let rays = TriangleIndex::new([]).unwrap();
    let yaw = input["actor_yaw"].as_i64().unwrap() as i32;
    let id = avatar(1, 2);
    let spawn = OwnerBodySpawn {
        controller: OWNER,
        entity: id.entity,
        skin: 0,
        body_filter: crate::match_scene::team_bit(0).unwrap(),
        slot: 0,
        frame: FIRST_TICK,
        pose: Movement::default(),
        eye_position: None,
        quanta: RETAIL_QUANTA,
    };
    let update = |health| EntityUpdate {
        entity: id.entity,
        operation: Some(Operation::Update(vec![health])),
        ..Default::default()
    };

    for callback in input["callbacks"].as_array().unwrap() {
        let mut runtime = factory.start(spawn).unwrap();
        let older = update(runtime.vitals().unwrap().cumulative_component());
        runtime.receive_damage(
            input["damage"].as_f64().unwrap(),
            crate::vitals::Death {
                time: 0.0,
                frame: FIRST_TICK,
                killer: 0xc000_5101,
            },
        );
        let body = body(id);
        let mut owner = WorldDelivery::new(OWNER);
        owner.started = Some(FIRST_TICK);
        owner.body = Some(body);
        owner
            .bodies
            .insert(id.entity, (id.generation, Lifetime::Live));
        owner.body_script = Some(BodyScript::new(body, FIRST_TICK, None, &runtime).unwrap());
        let mut mover = empty_mover();
        mover.set_cinematic_aim(Some([10_011, 0]));
        let mut room = RoomSimulation::new(Vec::new()).unwrap();
        room.spawn_player_body(PlayerBodyState::new(
            id,
            OWNER,
            0,
            0,
            body.hero,
            mover,
            Some(Box::new(runtime)),
            FIRST_TICK,
            None,
        ))
        .unwrap();
        room.player_body_mut(id)
            .unwrap()
            .mover
            .set_authored_facing(Some([12_011, 0]));
        let source = SightingSource {
            entity: 0xc000_5101,
            position: [0.0; 3],
            tags: [None; 3],
            filter_flags: None,
        };
        match callback.as_str().unwrap() {
            "message" => assert!(!room.deliver_player_defender_message(
                id,
                None,
                DefenderMessage {
                    target: Some(id.entity),
                    source: source.entity,
                    message: MESSAGE,
                    parameters: Default::default(),
                }
            )),
            "sighting" => assert!(room.deliver_player_defender_sighting(DefenderSighting {
                target: id.entity,
                source,
                parameters: Default::default(),
            })),
            name => panic!("unexpected fixture callback {name}"),
        }
        let events = room.take_player_events();
        assert_eq!(
            events.len() as u64,
            expected["halt_events"].as_u64().unwrap()
        );
        let [
            PlayerBodyEvent::ScriptHalted {
                owner: affected_owner,
                id: affected_id,
                health: Some(health),
            },
        ] = events.as_slice()
        else {
            panic!("expected one generation-bound script stop with health");
        };
        assert_eq!((*affected_owner, *affected_id), (OWNER, id));
        let state = room.player_body_mut(id).unwrap();
        assert!(state.runtime.is_none());
        let vitals = state.vitals().unwrap();
        assert_eq!(
            vitals.total(),
            expected["health_after_fault"].as_f64().unwrap()
        );
        assert_eq!(&vitals.cumulative_component(), health);
        state
            .mover
            .step(
                &rays,
                Command {
                    frame: FIRST_TICK + 1,
                    yaw,
                    ..Default::default()
                },
                0.016,
                |_, _| {},
            )
            .unwrap();
        assert_eq!(state.mover.pose().yaw, yaw);

        let mut observer = WorldDelivery::new(OWNER + 0x100);
        observer.started = Some(FIRST_TICK);
        observer.create_remote_player(
            id,
            EntityUpdate {
                entity: id.entity,
                operation: Some(Operation::Create(Box::new(Create::new(1, vec![])))),
                ..Default::default()
            },
        );
        let mut observer_egress = GameEgress::default();
        transmit(&mut observer, None, &mut observer_egress, FIRST_TICK, 1);
        transport_outcome(&mut observer, &mut observer_egress, 1, true);
        observer
            .scene_views
            .insert(id.entity, BodyStream::new(id.entity));

        let stale = BodyId {
            generation: id.generation - 1,
            ..id
        };
        owner.player_script_halted(OWNER, stale, Some(health.clone()));
        observer.player_script_halted(OWNER, stale, Some(health.clone()));
        assert!(owner.body_script.is_some());
        assert!(!observer.health_pending(id.entity));
        assert!(observer.scene_views.contains_key(&id.entity));

        for delivery in [&mut owner, &mut observer] {
            delivery.queue_health(older.clone());
            delivery.player_script_halted(OWNER, id, Some(health.clone()));
        }
        assert!(owner.body_script.is_none());
        assert!(!observer.scene_views.contains_key(&id.entity));
        assert!(owner.health_pending(id.entity));
        assert!(owner.ready().is_empty()); // create ACK is still pending
        owner.created.insert(id.entity);
        let final_update = update(health.clone());
        assert_ne!(older, final_update);
        for delivery in [&owner, &observer] {
            let ready = delivery.ready();
            assert_eq!(ready.len(), 1);
            assert_eq!(ready[0].record.update, final_update);
        }
    }
}
