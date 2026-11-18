use super::*;

mod benchmark;
mod harness;
mod isolation;
mod junkenstein_validation;
mod world_harness;

pub use harness::*;
pub use world_harness::*;

pub(super) use crate::{
    bits::{BitReader, BitWriter},
    game_content::PRACTICE_RANGE_MAP,
    game_link::{FIN, KIND_CONTROL, LinkCipher, MAX_DATAGRAM, ReceiveWindow, SYN},
    game_messages::{read_channel, write_channel},
};
pub(super) use std::{collections::HashSet, net::UdpSocket as ClientSocket};

#[test]
fn worker_serves_a_match_seat_and_retries_before_map_entry() {
    exercise_loading(fixture());
}

#[test]
fn debug_snapshot_excludes_unprepared_rooms_and_stops_with_the_worker() {
    let mut server = GameService::bind(local(), Arc::new(fixture())).unwrap();
    let idle = server.debug_snapshot().unwrap();
    assert!(idle.rooms.is_empty());
    assert!(!idle.truncated);
    assert!(idle.timing.is_none());
    server
        .open_match(MATCH, GameMap::practice_range(), player())
        .unwrap();
    assert!(server.debug_snapshot().unwrap().rooms.is_empty());
    server.stop().unwrap();
    assert_eq!(
        server.debug_snapshot().err().unwrap().kind(),
        io::ErrorKind::BrokenPipe
    );
}

#[test]
fn timing_opt_in_reports_empty_histograms_while_no_world_is_running() {
    let mut server = GameService::bind_with_timing(local(), Arc::new(fixture()), true).unwrap();
    let idle = server.debug_snapshot().unwrap();
    let timing = idle.timing.unwrap();
    assert_eq!(timing.tick_duration_us.samples, 0);
    assert_eq!(timing.schedule_lateness_us.samples, 0);
    server
        .open_match(MATCH, GameMap::practice_range(), player())
        .unwrap();
    let ready = server.debug_snapshot().unwrap();
    assert!(ready.rooms.is_empty());
    let timing = ready.timing.unwrap();
    assert_eq!(timing.tick_duration_us.samples, 0);
    assert_eq!(timing.schedule_lateness_us.samples, 0);
    assert_eq!(timing.input_budget.turns, 0);
    server.stop().unwrap();
}

#[test]
fn lua_fault_in_one_room_leaves_sibling_room_and_patrol_alive() {
    use crate::{
        game_bots::{BOT_ENTITY, Bot},
        game_service::room::InjectedFault,
        scene_assets::PreparedScene,
    };

    for (label, faulty_source) in [
        (
            "think_loop",
            "return { abi = 2, think = function() while true do end end }",
        ),
        (
            "think_oom",
            r#"return { abi = 2, think = function() local s = "0123456789abcdef"; for i = 1, 16 do s = s .. s end; return {} end }"#,
        ),
        (
            "think_runtime_error",
            "return { abi = 2, think = function() local x = nil; return x.missing end }",
        ),
    ] {
        let schemas = Arc::new(fixture());
        let mut state = State::new(schemas, PatrolScript::load().unwrap());
        let socket = UdpSocket::bind(local()).unwrap();
        let failed_id = MATCH;
        let survivor_id = [5, 9];

        state
            .open_match(
                failed_id,
                GameMap::practice_range(),
                player(),
                Some(WorldStart {
                    scene: Box::new(PreparedScene::fixture_for_bots(vec![Bot::fixture_walker(
                        [0.0, 0.0, 10.0],
                    )])),
                    card: PlayerCard::default(),
                }),
                Duration::ZERO,
            )
            .unwrap();
        state
            .rooms
            .get_mut(&failed_id)
            .unwrap()
            .world
            .as_mut()
            .unwrap()
            .patrol = PatrolScript::from_source(faulty_source).unwrap();

        state
            .open_match(
                survivor_id,
                GameMap::practice_range(),
                player(),
                Some(WorldStart {
                    scene: Box::new(PreparedScene::fixture_for_bots(vec![Bot::fixture_walker(
                        [0.0, 0.0, 10.0],
                    )])),
                    card: PlayerCard::default(),
                }),
                Duration::ZERO,
            )
            .unwrap();

        for _ in 0..220 {
            state.tick(&socket, Duration::ZERO, 0).unwrap();
        }

        // Lua faults are contained per-bot inside PreparedScene::step_bots:
        // neither room is retired, failed room's bot stops cleanly, and survivor advances.
        assert!(state.rooms.contains_key(&failed_id), "{label}");
        assert!(state.rooms.contains_key(&survivor_id), "{label}");
        assert!(state.recent_faults.is_empty(), "{label}");
        let failed_bot = state.rooms[&failed_id]
            .world
            .as_ref()
            .unwrap()
            .scene
            .bots()
            .unwrap()
            .find(|bot| bot.entity == BOT_ENTITY)
            .unwrap();
        assert!(!failed_bot.patrol_active(), "{label}");
        assert_eq!(failed_bot.position, [0.0; 3], "{label}");

        let survivor_before = state.rooms[&survivor_id]
            .world
            .as_ref()
            .unwrap()
            .scene
            .bots()
            .unwrap()
            .find(|bot| bot.entity == BOT_ENTITY)
            .unwrap()
            .position;
        assert_ne!(survivor_before, [0.0; 3], "{label}");

        // A room-level Bots-stage fault retires only the faulted room; survivor keeps patrolling.
        state.rooms.get_mut(&failed_id).unwrap().injection =
            Some((RoomStage::Bots, InjectedFault::BrokenBot));
        state.tick(&socket, Duration::ZERO, 0).unwrap();
        assert!(!state.rooms.contains_key(&failed_id), "{label}");
        assert!(state.rooms.contains_key(&survivor_id), "{label}");
        assert_eq!(state.recent_faults.len(), 1, "{label}");
        let survivor_after = state.rooms[&survivor_id]
            .world
            .as_ref()
            .unwrap()
            .scene
            .bots()
            .unwrap()
            .find(|bot| bot.entity == BOT_ENTITY)
            .unwrap()
            .position;
        assert_ne!(survivor_after, survivor_before, "{label}");
    }
}

#[test]
#[ignore = "requires a prepared 104319 Practice Range pack"]
fn prepared_rooms_keep_patrol_script_state_local() {
    use crate::{ai_script::AiActorSnapshot, patrol_script::WaitEvent, scene_assets::SceneAssets};

    let runtime = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let schemas = Arc::new(Schemas::load(runtime.join("data/game_schemas_174.json")).unwrap());
    let assets = SceneAssets::load(runtime).unwrap();
    let stateful = PatrolScript::from_source(
        r#"
        local count = 10
        return {
            abi = 2,
            think = function()
                count = count + 1
                return { { kind = "wait", seconds = count } }
            end,
        }
        "#,
    )
    .unwrap();
    let mut state = State::new(schemas, stateful);
    let second = [MATCH[0], MATCH[1] + 1];
    for id in [MATCH, second] {
        let map = GameMap::practice_range();
        let scene = assets.prepare(map.clone()).unwrap();
        state
            .open_match(
                id,
                map,
                player(),
                Some(WorldStart {
                    scene: Box::new(scene),
                    card: PlayerCard::default(),
                }),
                Duration::ZERO,
            )
            .unwrap();
    }
    let wait = |id, frame| {
        state.rooms[&id]
            .world
            .as_ref()
            .unwrap()
            .patrol
            .wait(
                AiActorSnapshot::patrol_fixture(0, 1),
                WaitEvent::Spawn,
                frame,
            )
            .unwrap()
    };
    assert_eq!(wait(MATCH, FIRST_TICK), 11.0);
    assert_eq!(wait(second, FIRST_TICK), 3.0);
    assert_eq!(wait(MATCH, FIRST_TICK + 1), 12.0);
    assert_eq!(wait(second, FIRST_TICK + 1), 3.0);
}

#[test]
#[ignore = "requires the separately prepared 104319 game schemas"]
fn worker_serves_loading_using_the_retail_schemas() {
    let root = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let schemas = Schemas::load(root.join("data/game_schemas_174.json")).unwrap();
    exercise_loading(schemas);
}

#[test]
fn canceled_seat_is_rejected_before_tick_and_released() {
    let mut server = GameService::bind(local(), Arc::new(fixture())).unwrap();
    let ticket = server
        .open_match(MATCH, GameMap::practice_range(), player())
        .unwrap()
        .pop()
        .unwrap();
    assert!(server.handoff_pending(ticket.conn).unwrap());
    assert!(server.cancel_handoff(ticket.conn).unwrap());
    assert!(!server.handoff_pending(ticket.conn).unwrap());
    let mut client = Client::new(server.local_addr(), &ticket);
    client
        .socket
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    client.send(SYN, Vec::new());
    let mut buffer = [0; MAX_DATAGRAM];
    let error = client.socket.recv(&mut buffer).unwrap_err();
    assert!(matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ));
    let retry = server
        .open_match(MATCH, GameMap::practice_range(), player())
        .unwrap()
        .pop()
        .unwrap();
    assert_ne!(ticket.conn, retry.conn);
    server.stop().unwrap();
}

#[test]
#[ignore = "requires a prepared 104319 Practice Range scene"]
fn busy_peers_resume_in_round_robin_order_after_the_global_input_budget() {
    use crate::{commands::Command, scene_assets::SceneAssets};

    let assets = SceneAssets::load(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap()).unwrap();
    let scene = assets.prepare(GameMap::practice_range()).unwrap();
    let mut state = State::new(Arc::new(fixture()), PatrolScript::load().unwrap());
    state.timing = Some(WorkerTiming::new());
    // Five synthetic peers share a prepared fixture solely to test the
    // worker scheduler. The actual admission route remains solo.
    let players: Vec<_> = (0..5)
        .map(|slot| Player {
            info: player().pop().unwrap(),
            entity: player_entity(slot),
            gone: false,
        })
        .collect();
    state.rooms.insert(
        MATCH,
        Room {
            map: scene.map().clone(),
            members: IndexMap::new(),
            players,
            injection: None,
            world: Some(RoomWorld {
                scene: Box::new(scene),
                card: PlayerCard::default(),
                patrol: PatrolScript::load().unwrap(),
            }),
        },
    );
    let first = FIRST_TICK + 1;
    let commands: Vec<_> = (first..first + 32)
        .map(|frame| Command {
            frame,
            ..Command::default()
        })
        .collect();
    for slot in 0..state.rooms[&MATCH].players.len() {
        let room = &state.rooms[&MATCH];
        let connection = slot as u32 + 1;
        let viewer = room.appearance(slot).unwrap();
        let mut peer = Peer::new(viewer.entity);
        peer.world
            .start(&room.world.as_ref().unwrap().scene, &viewer, FIRST_TICK)
            .unwrap();
        peer.world.queue_commands(FIRST_TICK, &commands);
        state.routes.insert(connection, MATCH);
        state.order.insert(connection, ());
        state.rooms.get_mut(&MATCH).unwrap().members.insert(
            connection,
            RoomMember {
                slot,
                peer: Some(peer),
            },
        );
    }
    let acks = |state: &State| {
        state
            .order
            .keys()
            .map(|connection| {
                state.rooms[&MATCH].members[connection]
                    .peer
                    .as_ref()
                    .unwrap()
                    .world
                    .command_ack(FIRST_TICK)
            })
            .collect::<Vec<_>>()
    };
    assert!(state.advance_input(Duration::ZERO).unwrap());
    assert_eq!(
        acks(&state),
        [first + 15, first + 15, first + 15, first + 15, FIRST_TICK]
    );
    assert!(state.advance_input(Duration::ZERO).unwrap());
    assert_eq!(
        acks(&state),
        [first + 31, first + 31, first + 31, first + 15, first + 15]
    );
    assert!(!state.advance_input(Duration::ZERO).unwrap());
    assert_eq!(acks(&state), [first + 31; 5]);
    let timing = state.debug_snapshot().unwrap().timing.unwrap();
    assert_eq!(timing.input_budget.turns, 3);
    assert_eq!(timing.input_budget.used, 160);
    assert_eq!(timing.input_budget.peer_limit_hits, 10);
    assert_eq!(timing.input_budget.turn_limit_hits, 2);
    assert_eq!(timing.input_budget.turns_with_backlog, 2);
}

#[test]
#[ignore = "requires a prepared 104319 Practice Range scene"]
fn prepared_world_waits_for_ownership_and_retries_baselines_before_scripts() {
    exercise_prepared_world(PreparedFixture::Stock);
}

#[test]
#[ignore = "requires a prepared 104319 Practice Range scene without bot VM graphs"]
fn prepared_worker_retries_a_concrete_owner_vm_on_encrypted_loopback() {
    exercise_prepared_world(PreparedFixture::Owner);
}

#[test]
#[ignore = "requires a prepared 104319 Practice Range scene without bot VM graphs"]
fn prepared_worker_delivers_a_volley_hit_and_replaces_the_bot_over_encrypted_udp() {
    exercise_prepared_world(PreparedFixture::Combat);
}

#[test]
#[ignore = "requires a prepared 104319 Practice Range scene without bot VM graphs"]
fn prepared_worker_delivers_helix_flight_damage_and_predicted_release_over_encrypted_udp() {
    exercise_prepared_world(PreparedFixture::Projectile);
}

#[test]
#[ignore = "requires a tagged 104319 Practice Range scene without bot VM graphs"]
fn prepared_worker_retries_the_stock_soldier_vm_on_encrypted_loopback() {
    exercise_prepared_world(PreparedFixture::StockOwner);
}

#[test]
#[ignore = "requires a tagged 104319 Practice Range scene without bot VM graphs"]
fn prepared_worker_delivers_stock_helix_over_encrypted_udp() {
    exercise_prepared_world(PreparedFixture::StockProjectile);
}

#[test]
#[ignore = "requires a tagged 104319 Practice Range scene without bot VM graphs"]
fn prepared_worker_delivers_stock_biotic_canister_and_field() {
    exercise_prepared_world(PreparedFixture::StockBiotic);
}

#[test]
#[ignore = "requires the prepared 104319 Junkenstein map, collision and graph data"]
fn junkenstein_worker_delivers_castle_gate_and_mode_frame() {
    use crate::game_content::{JUNKENSTEIN_GATE, JUNKENSTEIN_MAP, JunkensteinCard, MapCatalog};
    let runtime = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let data = runtime.join("data");
    let schemas = Arc::new(Schemas::load(data.join("game_schemas_174.json")).unwrap());
    let maps = MapCatalog::load(data.join("game_maps_174.json")).unwrap();
    let assets = crate::scene_assets::SceneAssets::load(runtime).unwrap();
    let mut server = GameService::bind(local(), Arc::clone(&schemas)).unwrap();
    for index in [0x3c, 0x43, 0x16e] {
        let card = JunkensteinCard::from_index(index).unwrap();
        let mode = card.mode;
        let map = maps
            .game_map(
                JUNKENSTEIN_MAP,
                mode,
                "Junkenstein's Revenge",
                false,
                &[card.players, 0],
            )
            .unwrap();
        let scene = assets.prepare_junkenstein(map, card, 0).unwrap();
        // Verify that encrypted delivery preserves the complete prepared HUD.
        // Authored descriptor/parent structure is covered by the all-card test.
        let expected_mode = (
            scene.mode_ui().bit_len(),
            scene.mode_ui().as_bytes().to_vec(),
        );
        let id = [MATCH[0], u64::from(index)];
        let debug_id = format!("{:016X}{:016X}", id[0], id[1]);
        let encounter = || {
            server
                .debug_snapshot()
                .unwrap()
                .rooms
                .into_iter()
                .find(|room| room.id == debug_id)
                .unwrap()
                .encounter
        };
        let ticket = server
            .open_prepared_solo(id, scene, player().pop().unwrap(), PlayerCard::default())
            .unwrap();
        let mut client = Client::new(server.local_addr(), &ticket);
        client.handshake();
        let load = client.receive();
        assert!(client.window.accept(load.seq));
        let messages = messages(&schemas, &load);
        assert_eq!(messages[0].message.id, 20300);
        assert_eq!(
            messages[0].message.fields[&0x4a8],
            Value::Unsigned(JUNKENSTEIN_MAP)
        );
        assert!(encounter().is_none());
        let mut payload = BitWriter::new();
        let loaded = encode(&schemas, 21602, &Fields::new()).unwrap();
        write_channel(&mut payload, &[(0, loaded)], &[]).unwrap();
        client.send(ACK, payload.into_bytes());

        let deadline = Instant::now() + Duration::from_secs(4);
        let mut gate_created = false;
        let mut presented = false;
        let mut controller_ready = false;
        loop {
            let packet = client.receive();
            assert!(client.window.accept(packet.seq));
            for record in world_records(&schemas, &packet) {
                if record.entity == (0x8000_0000 | JUNKENSTEIN_GATE) && record.is_create() {
                    gate_created = true;
                }
                controller_ready |= record.entity == player_entity(0) && record.script.is_some();
                if let Some(frame) = record
                    .script
                    .filter(|frame| record.entity == GAME_MODE_ENTITY && frame.first == 0)
                {
                    assert_eq!(frame.width, expected_mode.0);
                    assert_eq!(frame.data, expected_mode.1);
                    presented = true;
                }
            }
            client.send(ACK, Vec::new());
            if gate_created && presented && controller_ready {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "waiting for castle gate and mode {mode:x}"
            );
        }
        let started = encounter().expect("encounter clock after world entry");
        assert!(
            started.elapsed_ms < 2_000,
            "clock must be relative to world entry"
        );
        assert!(!started.countdown_started);
        let hero = assets.heroes().find_hero("Cassidy").unwrap().guid;
        send_game_message(
            &mut client,
            &schemas,
            1,
            crate::game_parameters::PICK_HERO,
            hero.to_le_bytes().to_vec(),
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut lost = None;
        loop {
            let packet = client.receive();
            let delta = world_records(&schemas, &packet)
                .into_iter()
                .find(|record| record.entity == GAME_MODE_ENTITY)
                .and_then(|record| record.script)
                .filter(|script| {
                    if script.first == 0 {
                        return false;
                    }
                    let mut bits = script.bits();
                    bits.bits(2).ok() == Some(0)
                        && bits.w_u16().ok() == Some(2)
                        && bits.bits(2).ok() == Some(0)
                        && bits.w_u16().ok() == Some(0)
                        && bits.bit().ok() == Some(true)
                        && bits.w_var().ok() == Some(1303)
                });
            if let Some(delta) = delta {
                assert_eq!(delta.span, 0);
                let mut bits = delta.bits();
                assert_eq!(bits.bits(2).unwrap(), 0);
                assert_eq!(bits.w_u16().unwrap(), 2);
                assert_eq!(bits.bits(2).unwrap(), 0);
                assert_eq!(bits.w_u16().unwrap(), 0);
                assert!(bits.bit().unwrap());
                assert_eq!(bits.w_var().unwrap(), 1303);
                let value = match bits.bits(4).unwrap() {
                    5 => bits.bits(4).unwrap() as f32,
                    8 => bits.f32().unwrap(),
                    tag => panic!("unexpected countdown value type {tag}"),
                };
                assert!((7.5..=8.0).contains(&value));
                if let Some(first) = lost {
                    assert_eq!(delta, first);
                    assert!(client.window.accept(packet.seq));
                    client.send(ACK, Vec::new());
                    let live = encounter().unwrap();
                    assert!(live.countdown_started && live.elapsed_ms > started.elapsed_ms);
                    break;
                }
                lost = Some(delta);
            } else {
                assert!(client.window.accept(packet.seq));
                client.send(ACK, Vec::new());
            }
            assert!(
                Instant::now() < deadline,
                "waiting for countdown retransmit in mode {mode:x}"
            );
        }
    }
    server.stop().unwrap();
}
