use super::{
    death_notice::{acknowledge_world_frame, pick_body},
    *,
};
use crate::{
    commands::Command,
    contact_world::ContactWorld,
    game_clock::TICK_PERIOD,
    game_combat::{PosedHitscan, ShotContext},
    game_heroes::{HeroCatalog, SOLDIER},
    game_hitscan::{DamageTags, HitDefinition, HurtVolumeCatalog, TargetHistory},
    game_replication::{BodyId, BodyStep},
    mover::Mover,
    mover_settings::MoverSettings,
    script_player::fixture_body_factory_with_nodes,
    vitals::Death,
};
use serde::Deserialize;
use serde_json::{Value as Json, json};

#[derive(Deserialize)]
struct Contract {
    input: Input,
    expected: Expected,
}

#[derive(Deserialize)]
struct Input {
    damage: f64,
    stimulus: u64,
    line: u64,
}

#[derive(Deserialize)]
struct Expected {
    shooter_notices: usize,
    victim_notices: usize,
    after_replacement: usize,
    reliable_sequence: u32,
}

const ALTERNATE_HERO: u64 = 0x02e0_0000_0052_0044;

enum CueTrigger {
    DamageReceived,
    CriticalDealt,
}

fn feedback_graph(stimulus: u64, line: u64, trigger: CueTrigger) -> Vec<Json> {
    let variable = |id| json!({"$": "STUConfigVarDynamic", "m_identifier": id});
    let plug = |index| json!({"$": "Plug", "links": [[index, "m_inPlug"]]});
    let empty = json!({"$": "Plug", "links": []});
    let node = |pos, class, fields| {
        json!({
            "pos":pos,"uid":pos,"rid":pos,"remote":-1,"state":null,
            "class":class,"client_only":false,"server_only":true,"fields":fields,
        })
    };
    let (recipient_slot, condition_slot, condition) = match trigger {
        CueTrigger::DamageReceived => (
            2,
            3,
            json!({
                "$":"STUConfigVarExpression",
                "m_expression":{"m_opcodes":[8,0,6,0,34,0],"m_dynamicVars":[variable(2)],"m_D99EF254":[0]},
            }),
        ),
        CueTrigger::CriticalDealt => (1, 7, variable(2)),
    };
    let binding = |slot, id| {
        json!({
            "m_B5051BCE":format!("0x{slot:016X}"),"m_out_Var":variable(id),
        })
    };
    let mut entry = node(
        0,
        "EntryHostEvent",
        json!({
            "m_params": [
                binding(recipient_slot, 1),
                binding(condition_slot, 2),
            ],
            "m_outPlug":plug(1),
        }),
    );
    entry["host_event_entry"] = json!("health_applied");
    let branch = |pos, condition| {
        let mut node = node(
            pos,
            "FixtureBranchAction",
            json!({
                "m_ADEB6E05":true,"m_condition":condition,
                "m_truePlug":plug(pos + 1),"m_falsePlug":empty,
            }),
        );
        node["action_behavior"] = json!("branch");
        node
    };
    let recipient = branch(
        1,
        json!({
            "$":"STUConfigVarExpression","m_configVars":[{"$":"STU_B5A0CAF0"}],
            "m_expression":{"m_opcodes":[8,0,14,0,37,0],"m_dynamicVars":[variable(1)]},
        }),
    );
    let damage = branch(2, condition);
    let mut cue = node(
        3,
        "FixtureCueAction",
        json!({
            "m_ADEB6E05":true,"m_outPlug":empty,
            "stimulus":format!("0x{stimulus:016X}"),
            "line":format!("0x{line:016X}"),
            "audience":{"$":"STUConfigVarExpression","m_expression":{
                "m_opcodes":[8,0,20,1,0],"m_dynamicVars":[variable(1)],
            }},
        }),
    );
    cue["action_behavior"] = json!("room_cue");
    vec![entry, recipient, damage, cue]
}

fn apply_and_advance(room: &mut Room, bodies: [BodyId; 2], tick: u32, damage: f64) {
    let [shooter, victim] = bodies;
    let simulation = room.world.as_mut().unwrap().scene.simulation_mut();
    let (_, outcome, _) = simulation
        .damage_player_body_by_wire(
            victim,
            damage,
            Death {
                time: (tick * TICK_PERIOD).as_secs_f64(),
                frame: tick,
                killer: shooter.entity,
            },
            None,
        )
        .unwrap();
    assert_eq!(outcome.dealt, damage);
    advance_feedback(room, bodies, tick);
}

fn advance_feedback(room: &mut Room, bodies: [BodyId; 2], tick: u32) {
    room.finish_simulation(tick, tick * TICK_PERIOD).unwrap();
    let next_tick = tick + 1;
    let simulation = room.world.as_mut().unwrap().scene.simulation_mut();
    simulation.prepare_spatial_frame(next_tick).unwrap();
    for id in bodies {
        let actor = simulation.player_body_mut(id).unwrap();
        let runtime = actor.runtime.as_mut().unwrap();
        runtime
            .advance(
                Command {
                    frame: next_tick,
                    ..Command::default()
                },
                &actor.mover,
                BodyStep::UnposedTimer,
            )
            .unwrap();
        assert!(runtime.snapshot().unwrap().diagnostics.is_empty());
        actor.frame = next_tick;
    }
    room.finish_simulation(next_tick, next_tick * TICK_PERIOD)
        .unwrap();
}

fn feedback_room(graph: Vec<Json>, teams: [u8; 2]) -> (Room, [BodyId; 2], [u32; 2]) {
    let hero_entry =
        json!({"name":"Synthetic cue actor","class":"Damage","body":"0x0400000000000001"});
    let heroes = HeroCatalog::from_json(
        &json!({
            format!("0x{SOLDIER:016X}"):hero_entry,
            format!("0x{ALTERNATE_HERO:016X}"):hero_entry,
        })
        .to_string(),
    )
    .unwrap();
    let (factory, _) = fixture_body_factory_with_nodes(graph);
    let mut scene = PreparedScene::fixture_for_owner_admission(heroes);
    scene.fixture_enable_room_health_scripts();
    scene.owner_body_factory = Some(Arc::new(move |_, spawn, setup| {
        Ok(Some(Box::new(factory.start_with_setup(spawn, setup)?)))
    }));
    let card = PlayerCard::default();
    let mut sequence = [0; 2];
    let [shooter, victim] = [0, 1].map(|slot| {
        let owner = player_entity(slot);
        let viewer = PlayerAppearance {
            entity: owner,
            slot: i8::try_from(slot).unwrap(),
            team: teams[slot],
            card: &card,
        };
        let mut peer = Peer::new(owner);
        peer.world.start(&scene, &viewer, FIRST_TICK).unwrap();
        while !peer.world.identity_ready(owner) || !peer.world.identity_ready(GAME_MODE_ENTITY) {
            assert!(sequence[slot] < 8);
            acknowledge_world_frame(&mut peer, &mut sequence[slot]);
        }
        peer.world.present(&scene, FIRST_TICK).unwrap();
        let id = pick_body(&mut peer, &mut scene, &viewer, SOLDIER);
        (id, peer)
    });
    let (shooter, shooter_peer) = shooter;
    let (victim, victim_peer) = victim;
    let room = Room {
        map: scene.map().clone(),
        players: [0, 1]
            .into_iter()
            .map(|slot| {
                let mut info = player().pop().unwrap();
                info.team = teams[slot];
                Player {
                    info,
                    entity: player_entity(slot),
                    gone: false,
                }
            })
            .collect(),
        members: IndexMap::from([
            (
                1,
                RoomMember {
                    slot: 0,
                    peer: Some(shooter_peer),
                },
            ),
            (
                2,
                RoomMember {
                    slot: 1,
                    peer: Some(victim_peer),
                },
            ),
        ]),
        world: Some(RoomWorld {
            scene: Box::new(scene),
            card,
            patrol: PatrolScript::load().unwrap(),
            patrol_generation: 0,
        }),
        debug_control: room::RoomDebugControl::new(FIRST_TICK, Duration::ZERO),
        injection: None,
    };
    (room, [shooter, victim], sequence)
}

fn voice_schema() -> Schemas {
    Schemas::from_json(
        r#"{"82A2F991":{"27500":[
            {"type":14,"off":124,"count":0,"array":false,"fields":[
                {"type":6,"off":0,"count":0,"array":false}
            ]},
            {"type":8,"off":144,"count":0,"array":false}
        ]}}"#,
    )
    .unwrap()
}

#[test]
fn received_damage_audio_follows_only_the_current_victim_connection() {
    let contract: Contract = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/received-damage-audio-v1.json",
    ))
    .unwrap();
    let graph = feedback_graph(
        contract.input.stimulus,
        contract.input.line,
        CueTrigger::DamageReceived,
    );
    let (mut room, [shooter, victim], mut sequence) = feedback_room(graph, [0, 0]);
    let schemas = voice_schema();
    let now = FIRST_TICK * TICK_PERIOD;
    apply_and_advance(
        &mut room,
        [shooter, victim],
        FIRST_TICK + 1,
        contract.input.damage,
    );
    assert_eq!(
        room.members[&1]
            .peer
            .as_ref()
            .unwrap()
            .pending_combat_notices()
            .count(),
        contract.expected.shooter_notices
    );

    let world = room.world.as_mut().unwrap();
    let peer = room.members.get_mut(&2).unwrap().peer.as_mut().unwrap();
    assert_eq!(
        peer.pending_combat_notices().count(),
        contract.expected.victim_notices
    );
    peer.flush_combat(&schemas, now, Some(world.scene.simulation()))
        .unwrap();
    assert!(
        !peer.egress.has_queued_reliable(),
        "old body identity has not been acknowledged"
    );
    let viewer = PlayerAppearance {
        entity: player_entity(1),
        slot: 1,
        team: 0,
        card: &world.card,
    };
    let replacement = pick_body(peer, &mut world.scene, &viewer, ALTERNATE_HERO);
    assert_ne!(victim, replacement);
    peer.flush_combat(&schemas, now, Some(world.scene.simulation()))
        .unwrap();
    assert_eq!(
        peer.pending_combat_notices().count(),
        contract.expected.after_replacement
    );
    assert!(!peer.egress.has_queued_reliable());

    apply_and_advance(
        &mut room,
        [shooter, replacement],
        FIRST_TICK + 4,
        contract.input.damage,
    );
    let peer = room.members[&2].peer.as_ref().unwrap();
    assert_eq!(
        peer.pending_combat_notices().copied().collect::<Vec<_>>(),
        [CombatNotice::Voice {
            anchor: replacement.entity,
            speaker: replacement.entity,
            listener: replacement.entity,
            stimulus: contract.input.stimulus,
            line: contract.input.line,
        },]
    );
    assert_reliable_cue(
        &mut room,
        2,
        replacement,
        &mut sequence[1],
        FIRST_TICK + 5,
        contract.input.line,
        contract.expected.reliable_sequence,
    );
}

fn assert_reliable_cue(
    room: &mut Room,
    connection: u32,
    listener: BodyId,
    sequence: &mut u32,
    tick: u32,
    line: u64,
    reliable_sequence: u32,
) {
    let schemas = voice_schema();
    let simulation = room.world.as_ref().unwrap().scene.simulation();
    let peer = room
        .members
        .get_mut(&connection)
        .unwrap()
        .peer
        .as_mut()
        .unwrap();
    while !peer.world.identity_ready(listener.entity) {
        assert!(*sequence < 16);
        acknowledge_world_frame(peer, sequence);
    }
    peer.flush_combat(&schemas, tick * TICK_PERIOD, Some(simulation))
        .unwrap();
    assert_eq!(peer.pending_combat_notices().count(), 0);
    peer.world
        .send_frame(tick, Vec::new(), false, None, &mut peer.egress, |payload| {
            let mut reader = BitReader::new(&payload);
            reader.var_a().unwrap();
            let channel = read_channel(&mut reader, &schemas).unwrap();
            let [voice] = channel.reliable.as_slice() else {
                panic!("expected one cue, got {:?}", channel.reliable);
            };
            assert_eq!(voice.message.id, 27500);
            assert_eq!(voice.sequence, reliable_sequence);
            assert_eq!(voice.message.fields[&0x90], Value::Unsigned(line));
            assert_eq!(
                voice.message.fields[&0x7c],
                Value::Struct(Fields::from([(
                    0,
                    Value::Unsigned(u64::from(listener.entity))
                )]))
            );
            Ok::<_, io::Error>(*sequence + 1)
        })
        .unwrap();
}

#[test]
fn confirmed_headshot_cue_reaches_only_the_shooter_connection() {
    #[derive(Deserialize)]
    struct Headshots {
        stimulus: u64,
        line: u64,
        volumes: Json,
        rays: Vec<Ray>,
    }
    #[derive(Deserialize)]
    struct Ray {
        name: String,
        height: f64,
        multiplier: f64,
        confirmed: Option<bool>,
    }
    let contract: Headshots = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/confirmed-headshot-audio-v1.json",
    ))
    .unwrap();
    let graph = feedback_graph(contract.stimulus, contract.line, CueTrigger::CriticalDealt);
    let (mut room, [shooter, victim], mut sequence) = feedback_room(graph, [0, 1]);
    let scene = &mut room.world.as_mut().unwrap().scene;
    scene.fixture_set_hurt_volumes(
        HurtVolumeCatalog::from_json(&contract.volumes.to_string()).unwrap(),
    );
    let victim_state = scene.simulation_mut().player_body_mut(victim).unwrap();
    victim_state.mover = Mover::new(
        MoverSettings::default(),
        Arc::new(ContactWorld::new(Vec::new()).unwrap()),
        [6.0, 0.0, 2.0],
        0,
        8,
    )
    .unwrap();
    victim_state.history = TargetHistory::new(None);

    let mut tick = FIRST_TICK + 1;
    for ray in contract.rays {
        let scene = &mut room.world.as_mut().unwrap().scene;
        let (simulation, queries) = scene.bot_simulation();
        let result = simulation
            .hitscan(
                &queries,
                PosedHitscan {
                    source: shooter.entity,
                    origin: [0.0, ray.height, 2.0],
                    direction: [1.0, 0.0, 0.0],
                    command: Command {
                        frame: tick,
                        ..Command::default()
                    },
                    shot_frame: tick,
                    reach: 12.0,
                    hit: HitDefinition {
                        tags: DamageTags::EMPTY,
                        amount: 5.0,
                        loadout: None,
                        critical: ray.multiplier,
                        falloff: None,
                    },
                },
                ShotContext {
                    team: 0,
                    quanta: RETAIL_QUANTA,
                    tick,
                    now: tick * TICK_PERIOD,
                },
                |entity| entity == victim.entity,
            )
            .unwrap();
        let confirmed = result.as_ref().map(|hit| match hit.notices[0] {
            Some(CombatNotice::Hit { critical, .. }) => critical,
            _ => panic!("missing hit confirmation"),
        });
        assert_eq!(confirmed, ray.confirmed, "{}", ray.name);
        advance_feedback(&mut room, [shooter, victim], tick);
        let shooter_peer = room.members[&1].peer.as_ref().unwrap();
        assert_eq!(
            shooter_peer.pending_combat_notices().count(),
            usize::from(confirmed == Some(true)),
            "{}",
            ray.name
        );
        assert_eq!(
            room.members[&2]
                .peer
                .as_ref()
                .unwrap()
                .pending_combat_notices()
                .count(),
            0,
            "{}",
            ray.name
        );
        if confirmed == Some(true) {
            assert_reliable_cue(
                &mut room,
                1,
                shooter,
                &mut sequence[0],
                tick + 1,
                contract.line,
                0,
            );
        }
        tick += 2;
    }
}
