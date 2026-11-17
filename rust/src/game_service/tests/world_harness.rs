use super::*;

#[derive(Clone, Copy)]
pub enum PreparedFixture {
    Stock,
    StockOwner,
    StockProjectile,
    StockBiotic,
    Owner,
    Combat,
    Projectile,
}

pub fn exercise_prepared_world(fixture: PreparedFixture) {
    use crate::{
        accounts::Accounts,
        catalog::ItemDb,
        collection::Collection,
        game_bots::BOT_ENTITY,
        jam_codec::id16,
        lobby_login::Identity,
        lobby_practice::{CREATE_GAME, GAME_REQUEST, SoloPracticeHandoff, dispatch_scene},
        lobby_session::PreparedLogin,
        match_handoff::{HANDOFF, HANDOFF_MESSAGE},
        retail_templates::RetailTemplates,
        scene_assets::SceneAssets,
        script_player::{
            fixture_body_factory, fixture_hitscan_factory, fixture_projectile_factory,
        },
    };

    const CONTROLLER: u32 = 0xa000_0100;
    let runtime = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let data = runtime.join("data");
    let schemas = Arc::new(Schemas::load(data.join("game_schemas_174.json")).unwrap());
    let lobby = Schemas::load(data.join("schemas_174.json")).unwrap();
    let assets = SceneAssets::load(runtime).unwrap();
    let scripted_bots = assets.graphs().entity_body(0x0400_0000_0000_143e).is_some();
    let collection = Collection::new(
        &RetailTemplates::load(data.join("retail_templates.json")).unwrap(),
        Arc::new(ItemDb::load(&data).unwrap()),
    )
    .unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let accounts = Accounts::new(
        temporary.path().join("profiles"),
        temporary.path().join("seed"),
    )
    .unwrap();
    let account = accounts.get("world fixture").unwrap();
    let login = PreparedLogin {
        identity: Identity::create(account.account_lo, 1).unwrap(),
        account,
        tournament: false,
    };
    let fields = lobby
        .decode(GAME_REQUEST, CREATE_GAME, b"\x02\x00\x04\x00\x00\x00", true)
        .unwrap();
    let request = (GAME_REQUEST, CREATE_GAME, fields);
    let mut server = GameService::bind_with_timing(
        local(),
        Arc::clone(&schemas),
        matches!(fixture, PreparedFixture::Stock),
    )
    .unwrap();
    let channel = |id: MatchId, kind| {
        Fields::from([
            (0, Value::Struct(id16(id[0], id[1]))),
            (0x10, Value::Unsigned(kind)),
        ])
    };
    let mut outgoing = Vec::new();
    let result = dispatch_scene(
        &request,
        &login,
        &server,
        SoloPracticeHandoff {
            match_id: MATCH,
            host: "127.0.0.1",
            general: channel([7, 8], 7),
            match_channel: channel(MATCH, 6),
        },
        (&assets, &collection),
        |map| {
            let mut scene = assets.prepare(map)?;
            if matches!(fixture, PreparedFixture::StockOwner) {
                assert!(!scripted_bots);
                let [x, y, z] = scene.map().spawn_point(0, 0).unwrap().0;
                let (bots, _) = scene.bot_simulation();
                bots.body_mut(BOT_ENTITY + 1).unwrap().position = [x - 4.0, y, z];
            }
            if matches!(
                fixture,
                PreparedFixture::Projectile | PreparedFixture::StockProjectile
            ) {
                assert!(!scripted_bots);
                let [x, y, z] = scene.map().spawn_point(0, 0).unwrap().0;
                let (bots, _) = scene.bot_simulation();
                assert_eq!(
                    bots.bodies()
                        .unwrap()
                        .take(3)
                        .map(|bot| bot.spawn.team)
                        .collect::<Vec<_>>(),
                    [0, 1, 1]
                );
                assert!(
                    bots.bodies()
                        .unwrap()
                        .take(3)
                        .all(|bot| bot.spawn.route.is_empty())
                );
                bots.body_mut(BOT_ENTITY).unwrap().position = [x - 3.5, y, z];
                bots.body_mut(BOT_ENTITY + 1).unwrap().position = [x - 5.0, y, z];
                bots.body_mut(BOT_ENTITY + 2).unwrap().position = [x - 5.0, y, z + 0.8];
            }
            if !matches!(
                fixture,
                PreparedFixture::Stock
                    | PreparedFixture::StockOwner
                    | PreparedFixture::StockProjectile
                    | PreparedFixture::StockBiotic
            ) {
                assert!(!scripted_bots);
                let factory = match fixture {
                    PreparedFixture::Combat => {
                        let [x, y, z] = scene.map().spawn_point(0, 0).unwrap().0;
                        let (bots, _) = scene.bot_simulation();
                        bots.body_mut(BOT_ENTITY + 1).unwrap().position = [x - 4.0, y, z];
                        fixture_hitscan_factory(100.0)
                    }
                    PreparedFixture::Projectile => {
                        let [x, y, z] = scene.map().spawn_point(0, 0).unwrap().0;
                        let (bots, _) = scene.bot_simulation();
                        assert_eq!(
                            bots.bodies()
                                .unwrap()
                                .take(3)
                                .map(|bot| bot.spawn.team)
                                .collect::<Vec<_>>(),
                            [0, 1, 1]
                        );
                        assert!(
                            bots.bodies()
                                .unwrap()
                                .take(3)
                                .all(|bot| bot.spawn.route.is_empty())
                        );
                        bots.body_mut(BOT_ENTITY).unwrap().position = [x - 3.5, y, z];
                        bots.body_mut(BOT_ENTITY + 1).unwrap().position = [x - 5.0, y, z];
                        bots.body_mut(BOT_ENTITY + 2).unwrap().position = [x - 5.0, y, z + 0.8];
                        fixture_projectile_factory(40.0)
                    }
                    _ => fixture_body_factory().0,
                };
                scene.owner_body_factory = Some(Arc::new(move |hero, spawn| {
                    if hero.guid != crate::game_heroes::SOLDIER {
                        return Ok(None);
                    }
                    Ok(Some(Box::new(factory.start(spawn)?)))
                }));
            }
            Ok(scene)
        },
        |(crc, id, fields)| {
            let bytes = lobby.encode(*crc, *id, fields).unwrap();
            outgoing.push((*crc, *id, lobby.decode(*crc, *id, &bytes, true).unwrap()));
            Ok(true)
        },
    );
    if scripted_bots {
        assert_eq!(result.err().unwrap().kind(), io::ErrorKind::Unsupported);
        assert!(outgoing.is_empty());
        return;
    }
    let ticket = result.unwrap().unwrap().handoff;
    assert_eq!(
        outgoing.iter().map(|(_, id, _)| *id).collect::<Vec<_>>(),
        [20404, 20402, HANDOFF_MESSAGE]
    );
    assert_eq!(outgoing[2].0, HANDOFF);
    let Value::Struct(wire) = &outgoing[2].2[&0x80] else {
        panic!("the decoded handoff has no seat body");
    };
    let key = |offset| {
        let Value::Array(bytes) = &wire[&offset] else {
            panic!("the decoded handoff has no direction key");
        };
        std::array::from_fn(|index| match bytes[index] {
            Value::Unsigned(byte) => u8::try_from(byte).unwrap(),
            _ => panic!("the handoff key contains an invalid byte"),
        })
    };
    assert_eq!(wire[&0x28], Value::Unsigned(u64::from(ticket.conn)));
    assert_eq!(
        wire[&0x2c],
        Value::Unsigned(u64::from(server.local_addr().port()))
    );
    let client_ticket = Handoff {
        key_in: key(0xae),
        key_out: key(0xce),
        ..ticket
    };
    let mut client = Client::new(server.local_addr(), &client_ticket);
    client.handshake();
    let load = client.receive();
    assert_eq!(messages(&schemas, &load)[0].message.id, 20300);
    assert!(client.window.accept(load.seq));
    let mut payload = BitWriter::new();
    let loaded = encode(&schemas, 21602, &Fields::new()).unwrap();
    write_channel(&mut payload, &[(0, loaded)], &[]).unwrap();
    let started = Instant::now();
    client.send(ACK, payload.into_bytes());

    let mut ownership = false;
    while started.elapsed() < Duration::from_millis(650) {
        let frame = client.receive();
        assert!(client.window.accept(frame.seq));
        assert!(world_records(&schemas, &frame).is_empty());
        ownership |= messages(&schemas, &frame)
            .iter()
            .any(|message| message.message.id == 20302);
    }
    assert!(ownership);
    client.send(ACK, Vec::new());

    let deadline = Instant::now() + Duration::from_secs(3);
    let lost = loop {
        let frame = client.receive();
        let records = world_records(&schemas, &frame);
        let creates: Vec<_> = records
            .iter()
            .filter(|record| record.is_create())
            .map(|record| record.entity)
            .collect();
        if !creates.is_empty() {
            assert!(creates.contains(&CONTROLLER) && creates.contains(&GAME_MODE_ENTITY));
            break creates; // Deliberately leave the first baseline out of the ACK window.
        }
        assert!(client.window.accept(frame.seq));
        client.send(ACK, Vec::new());
        assert!(Instant::now() < deadline, "waiting for first world creates");
    };
    let mut creates = HashSet::new();
    loop {
        let frame = client.receive();
        assert!(client.window.accept(frame.seq));
        let records = world_records(&schemas, &frame);
        assert!(records.iter().all(|record| record.script.is_none()));
        for record in records.iter().filter(|record| record.is_create()) {
            assert!(creates.insert(record.entity));
        }
        if lost.iter().all(|id| creates.contains(id)) {
            break;
        }
        client.send(ACK, Vec::new());
        assert!(Instant::now() < deadline, "waiting for baseline retransmit");
    }
    assert_eq!(creates.len(), 21);

    // Even after the retransmission, the script stays behind its create ACK.
    let withheld = client.receive();
    assert!(client.window.accept(withheld.seq));
    assert!(
        world_records(&schemas, &withheld)
            .iter()
            .all(|record| record.script.is_none())
    );
    client.send(ACK, Vec::new());

    let mut mode = false;
    let mut controller = false;
    let mut moving_bot = false;
    while !(mode && controller && moving_bot) && Instant::now() < deadline {
        let frame = client.receive();
        assert!(client.window.accept(frame.seq));
        for record in world_records(&schemas, &frame) {
            assert!(!record.is_create());
            if record.movement {
                assert!((BOT_ENTITY..BOT_ENTITY + 15).contains(&record.entity));
                assert!(creates.contains(&record.entity));
                moving_bot = true;
            }
            if let Some(script) = record.script {
                assert_eq!((script.first, script.span), (0, 1));
                if record.entity == GAME_MODE_ENTITY {
                    assert_eq!(script.data, [0x08, 0x5b, 0x06, 0x00, 0x0c]);
                    mode = true;
                } else {
                    assert_eq!(record.entity, CONTROLLER);
                    let mut bits = script.bits();
                    assert_eq!(bits.bits(3).unwrap(), 2);
                    assert!(bits.w_var().unwrap() >= crate::game_clock::FIRST_TICK);
                    controller = true;
                }
            }
        }
        client.send(ACK, Vec::new());
    }
    assert!(mode && controller && moving_bot);
    match fixture {
        PreparedFixture::Owner => {
            exercise_owner_vm(&mut client, &schemas, true);
        }
        PreparedFixture::StockOwner => {
            let frame = exercise_owner_vm(&mut client, &schemas, false);
            exercise_stock_rifle(&mut client, &schemas, frame + 1, BOT_ENTITY + 1);
        }
        PreparedFixture::Combat => {
            exercise_combat(&mut client, &schemas, BOT_ENTITY + 1, BOT_ENTITY + 15)
        }
        PreparedFixture::Projectile => exercise_projectile(&mut client, &schemas, None),
        PreparedFixture::StockProjectile => {
            let frame = exercise_owner_vm(&mut client, &schemas, false);
            let launch = wait_for_stock_weapon(&mut client, &schemas, frame + 1);
            exercise_projectile(&mut client, &schemas, Some(launch));
        }
        PreparedFixture::StockBiotic => {
            assert!(!scripted_bots);
            let frame = exercise_owner_vm(&mut client, &schemas, false);
            let launch = wait_for_stock_weapon(&mut client, &schemas, frame + 1);
            exercise_stock_biotic(&mut client, &schemas, launch);
        }
        PreparedFixture::Stock => {
            exercise_hero_selection(&mut client, &schemas, &assets, &server, &ticket);
            let snapshot = server.debug_snapshot().unwrap();
            let timing = snapshot.timing.as_ref().unwrap();
            assert!(timing.tick_duration_us.samples > 0);
            assert_eq!(
                timing.schedule_lateness_us.samples,
                timing.tick_duration_us.samples
            );
            assert_eq!(
                timing.tick_duration_us.counts.iter().sum::<u64>(),
                timing.tick_duration_us.samples
            );
            let [room] = snapshot.rooms.as_slice() else {
                panic!("expected one prepared room");
            };
            assert!(!snapshot.truncated);
            assert_eq!(
                room.map,
                format!("{:016X}", GameMap::practice_range().map_guid)
            );
            assert_eq!(room.actors.len(), 16);
            let actor = room
                .actors
                .iter()
                .find(|actor| actor.entity == CONTROLLER | 3)
                .unwrap();
            assert!(matches!(actor.kind, game_debug::ActorKind::Player));
            assert_eq!(actor.generation, Some(3));
            assert!(actor.position.into_iter().all(f64::is_finite));
            let bot = room
                .actors
                .iter()
                .find(|actor| actor.entity == BOT_ENTITY)
                .unwrap();
            assert!(matches!(bot.kind, game_debug::ActorKind::Bot));
            assert!(bot.alive && bot.health.is_some_and(|health| health > 0.0));
            assert!(!bot.script_active);
            assert!(room.actors.iter().any(|actor| {
                matches!(actor.kind, game_debug::ActorKind::Bot) && actor.script_active
            }));
        }
    }
    server.stop().unwrap();
}

pub fn send_game_message(
    client: &mut Client,
    schemas: &Schemas,
    ordinal: u32,
    guid: u64,
    parameters: Vec<u8>,
) {
    let fields = Fields::from([
        (
            0x78,
            Value::Struct(Fields::from([(
                0,
                Value::Unsigned(u64::from(player_entity(0))),
            )])),
        ),
        (0x80, Value::Blob(parameters)),
        (0xa8, Value::Unsigned(guid)),
        (0xb0, Value::Signed(3)),
    ]);
    let message = encode(
        schemas,
        crate::game_parameters::CLIENT_GAME_MESSAGE,
        &fields,
    )
    .unwrap();
    let mut payload = BitWriter::new();
    write_channel(&mut payload, &[(ordinal, message)], &[]).unwrap();
    client.send(ACK, payload.into_bytes());
}

pub fn pick_soldier_for_weapon(client: &mut Client, schemas: &Schemas) -> u32 {
    use crate::{game_heroes::SOLDIER, game_parameters::PICK_HERO};
    let body = player_entity(0) | 1;
    send_game_message(
        client,
        schemas,
        1,
        PICK_HERO,
        SOLDIER.to_le_bytes().to_vec(),
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let packet = client.receive();
        assert!(client.window.accept(packet.seq));
        client.send(ACK, Vec::new());
        if world_records(schemas, &packet)
            .iter()
            .any(|record| record.entity == body && record.is_create())
        {
            return BitReader::new(&packet.payload).var_a().unwrap() + 1;
        }
        assert!(Instant::now() < deadline, "waiting for weapon fixture body");
    }
}

pub fn exercise_combat(client: &mut Client, schemas: &Schemas, target: u32, replacement: u32) {
    let frame = pick_soldier_for_weapon(client, schemas);
    let fire = crate::commands::FIRE;
    let commands = command_batch(frame, 1, -16384, fire, false);
    client.send(ACK, commands.clone());
    client.send(ACK, commands);

    let deadline = Instant::now() + Duration::from_secs(7);
    let mut lost = None;
    let mut delivered_hit = false;
    let mut delivered_kill = false;
    let mut delivered_health = false;
    let mut destroyed = false;
    let mut created = false;
    while !created {
        let packet = client.receive();
        let reliable = messages(schemas, &packet);
        let hit = reliable.iter().find(|item| item.message.id == 25001);
        if lost.is_none() {
            if let Some(hit) = hit {
                lost = Some((hit.sequence, hit.message.fields.clone()));
                continue; // let a following ACK reveal loss of the original hit
            }
        }
        for item in reliable {
            match item.message.id {
                25001 => {
                    let (ordinal, fields) = lost.as_ref().unwrap();
                    assert_eq!((&item.sequence, &item.message.fields), (ordinal, fields));
                    assert_eq!(fields[&0x84], Value::Unsigned(u64::from(frame)));
                    let Value::Struct(impact) = &fields[&0x78] else {
                        panic!("hit body")
                    };
                    assert_eq!(impact[&4], Value::Unsigned(200));
                    assert_eq!(impact[&8], Value::Unsigned(3));
                    delivered_hit = true;
                }
                25005 => {
                    let Value::Struct(victim) = &item.message.fields[&0x84] else {
                        panic!("elimination body")
                    };
                    assert_eq!(victim[&0], Value::Unsigned(u64::from(target)));
                    delivered_kill = true;
                }
                _ => {}
            }
        }
        for record in world_records(schemas, &packet) {
            if record.entity == target {
                if record.operation == Some(2) {
                    delivered_health = true;
                }
                if record.operation == Some(3) {
                    assert!(delivered_hit && delivered_kill && delivered_health);
                    destroyed = true;
                }
            } else if record.entity == replacement && record.is_create() {
                assert!(destroyed);
                created = true;
            }
        }
        assert!(client.window.accept(packet.seq));
        client.send(ACK, Vec::new());
        assert!(
            Instant::now() < deadline,
            "waiting for combat recovery and bot respawn"
        );
    }
}

pub fn exercise_projectile(client: &mut Client, schemas: &Schemas, stock_frame: Option<u32>) {
    use crate::{
        commands::SECONDARY, game_bots::BOT_ENTITY, game_projectiles::ProjectileLeases,
        script_player::FIXTURE_PROJECTILE_DEFINITION,
    };

    let frame = stock_frame.unwrap_or_else(|| pick_soldier_for_weapon(client, schemas));
    let definition = FIXTURE_PROJECTILE_DEFINITION;
    let mut original = stock_frame.is_none().then(|| {
        ProjectileLeases::new(0, frame, [definition])
            .unwrap()
            .entity(definition, frame, 0)
            .unwrap()
    });
    let commands = command_batch(frame, 1, -16384, SECONDARY, false);
    client.send(ACK, commands.clone());
    client.send(ACK, commands);

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut noticed = HashSet::new();
    let mut impacts = Vec::new();
    let mut health = HashMap::new();
    let mut creates = HashSet::new();
    let mut releases = HashSet::new();
    let mut lost_original = false;
    while !original.is_some_and(|id| releases.contains(&id))
        || health.len() < 2
        || impacts.len() < 3
    {
        let packet = client.receive();
        let tick = BitReader::new(&packet.payload).var_a().unwrap();
        let records = world_records(schemas, &packet);
        let first_create = records.iter().find(|record| {
            original.is_none_or(|id| record.entity == id)
                && record.definition == Some(definition as u32)
        });
        if let (false, Some(create)) = (lost_original, first_create) {
            assert!(records.iter().all(|record| {
                record.entity != create.entity || record.projectile_release.is_none()
            }));
            original = Some(create.entity);
            lost_original = true;
            continue; // a later ACK will expose the lost confirmation datagram
        }
        for item in messages(schemas, &packet) {
            assert_ne!(
                item.message.id, 25005,
                "one Helix flight cannot eliminate this target"
            );
            if item.message.id != 25001 || !noticed.insert(item.sequence) {
                continue;
            }
            let fields = &item.message.fields;
            let Value::Unsigned(impact_frame) = fields[&0x84] else {
                panic!("impact frame")
            };
            assert!(impact_frame > u64::from(frame));
            let Value::Struct(impact) = &fields[&0x78] else {
                panic!("hit body")
            };
            let Value::Struct(target) = &impact[&0] else {
                panic!("hit target")
            };
            assert_eq!(impact[&8], Value::Unsigned(0));
            impacts.push((target[&0].clone(), impact[&4].clone()));
        }
        for record in records {
            if let Some(current) = record.base_health {
                health.insert(record.entity, current);
            }
            if record.definition == Some(definition as u32) {
                creates.insert(record.entity);
            }
            if let Some(release_frame) = record.projectile_release {
                assert!(creates.contains(&record.entity));
                assert_eq!(record.operation, Some(3));
                assert!(release_frame > tick);
                releases.insert(record.entity);
            }
        }
        assert!(client.window.accept(packet.seq));
        client.send(ACK, Vec::new());
        assert!(
            Instant::now() < deadline,
            "waiting for Helix: shot {frame}, entity {original:?}, impacts {impacts:?}, health {health:?}, creates {creates:?}, releases {releases:?}"
        );
    }
    assert!(lost_original);
    let impact = |entity, damage| (Value::Unsigned(u64::from(entity)), Value::Unsigned(damage));
    assert_eq!(
        impacts,
        [
            impact(BOT_ENTITY + 1, 40),
            impact(BOT_ENTITY + 1, 80),
            impact(BOT_ENTITY + 2, 80)
        ]
    );
    assert_eq!(
        health,
        HashMap::from([(BOT_ENTITY + 1, 80.0), (BOT_ENTITY + 2, 120.0)])
    );
}

pub fn exercise_stock_biotic(client: &mut Client, schemas: &Schemas, launch: u32) {
    client.send(ACK, command_batch(launch, 32, 0, 0x10, false));
    client.send(ACK, command_batch(launch + 32, 32, 0, 0, false));
    let deadline = Instant::now() + Duration::from_secs(9);
    let mut canisters = HashMap::new();
    let mut fields = HashMap::new();
    loop {
        let packet = client.receive();
        let tick = BitReader::new(&packet.payload).var_a().unwrap();
        assert!(client.window.accept(packet.seq));
        let mut retired = false;
        let records = world_records(schemas, &packet);
        for record in &records {
            if record.is_create() && record.definition == Some(0x03dc) {
                assert!(
                    !canisters.is_empty(),
                    "field preceded canister confirmation"
                );
                fields.entry(record.entity).or_insert((0, None));
            }
            if let Some((phase, destroyed)) = fields.get_mut(&record.entity) {
                if let Some(script) = &record.script {
                    assert!(!record.is_create(), "field script preceded create ACK");
                    match (script.width, script.data.as_slice()) {
                        (48, [0x08, 0x2c, 0x01, 0, 0x78, 0x0f]) => {
                            assert!(*phase <= 1);
                            *phase = 1;
                        }
                        (47, [0x08, 0x2c, 0x01, 0, 0x78, 0x03]) => {
                            assert!(*phase >= 1, "fade preceded active field");
                            *phase = 2;
                        }
                        _ => panic!("unexpected field script {script:?}"),
                    }
                }
                if record.operation == Some(3) {
                    assert_eq!(*phase, 2, "field retired before fade");
                    destroyed.get_or_insert(tick);
                }
            }
            if let (Some(3), Some(&created)) = (record.operation, canisters.get(&record.entity)) {
                assert!(record.projectile_release.is_none());
                // The create can arrive after prediction acknowledgement.
                assert!(
                    tick - created >= 250,
                    "canister retired before field expiry"
                );
                assert!(!fields.is_empty(), "no field entity was replicated");
                assert!(
                    fields
                        .values()
                        .all(|(_, destroyed)| destroyed.is_some_and(|t| t < tick)),
                    "canister retired before field destroy ACK"
                );
                retired = true;
            }
        }
        for record in records {
            if record.is_create()
                && record.definition == Some(crate::game_combat::BIOTIC_CANISTER as u32)
            {
                canisters.entry(record.entity).or_insert(tick);
            }
        }
        client.send(ACK, Vec::new());
        if retired {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "waiting for canister and field lifecycle: {canisters:?}, {fields:?}"
        );
    }
}

pub fn wait_for_stock_weapon(client: &mut Client, schemas: &Schemas, frame: u32) -> u32 {
    // The authored weapon startup must finish before combat input.
    let warmup = command_batch(frame, 24, -16384, 0, false);
    client.send(ACK, warmup);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let packet = client.receive();
        assert!(client.window.accept(packet.seq));
        client.send(ACK, Vec::new());
        if input_ack(schemas, &packet) >= frame + 23 {
            break;
        }
        assert!(Instant::now() < deadline, "waiting for the rifle warmup");
    }
    frame + 24
}

pub fn exercise_stock_rifle(client: &mut Client, schemas: &Schemas, frame: u32, target: u32) {
    let frame = wait_for_stock_weapon(client, schemas, frame);
    client.send(
        ACK,
        command_batch(frame, 8, -16384, crate::commands::FIRE, false),
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut damaged = false;
    let mut reported = false;
    while !damaged || !reported {
        let packet = client.receive();
        assert!(client.window.accept(packet.seq));
        for message in messages(schemas, &packet) {
            if message.message.id == 25001 {
                let Value::Struct(impact) = &message.message.fields[&0x78] else {
                    panic!("missing hit body");
                };
                assert_eq!(
                    impact[&0],
                    Value::Struct(Fields::from([(0, Value::Unsigned(u64::from(target)))]))
                );
                assert!(matches!(impact[&4], Value::Unsigned(amount) if amount > 0));
                reported = true;
            }
        }
        damaged |= world_records(schemas, &packet).iter().any(|record| {
            record.entity == target
                && record.operation == Some(2)
                && record
                    .base_health
                    .is_some_and(|health| (0.0..200.0).contains(&health))
        });
        client.send(ACK, Vec::new());
        assert!(
            Instant::now() < deadline,
            "waiting for an authored rifle hit"
        );
    }
}

pub fn exercise_owner_vm(client: &mut Client, schemas: &Schemas, forward: bool) -> u32 {
    use crate::{game_heroes::SOLDIER, game_parameters::PICK_HERO};

    let owner = player_entity(0);
    let body = owner | 1;
    send_game_message(
        client,
        schemas,
        1,
        PICK_HERO,
        SOLDIER.to_le_bytes().to_vec(),
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    let start = loop {
        let packet = client.receive();
        let records = world_records(schemas, &packet);
        assert!(
            records
                .iter()
                .all(|record| record.entity != body || record.script.is_none())
        );
        assert!(client.window.accept(packet.seq));
        client.send(ACK, Vec::new());
        if records
            .iter()
            .any(|record| record.entity == body && record.is_create())
        {
            let controller = records
                .iter()
                .find(|record| record.entity == owner && record.script.is_some())
                .unwrap();
            let mut bits = controller.script.as_ref().unwrap().bits();
            assert_eq!(bits.bits(3).unwrap(), 2);
            break bits.w_var().unwrap();
        }
        assert!(Instant::now() < deadline, "waiting for fixture body create");
    };
    let wait_chunk = |client: &mut Client| {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let packet = client.receive();
            if let Some(script) = world_records(schemas, &packet)
                .into_iter()
                .find(|record| record.entity == body && record.script.is_some())
                .and_then(|record| record.script)
            {
                return (packet.seq, script);
            }
            assert!(client.window.accept(packet.seq));
            client.send(ACK, Vec::new());
            assert!(Instant::now() < deadline, "waiting for fixture body script");
        }
    };
    let skip_chunk = |client: &mut Client, payload| {
        let packet = client.receive();
        assert!(
            world_records(schemas, &packet)
                .iter()
                .all(|record| record.entity != body || record.script.is_none())
        );
        assert!(client.window.accept(packet.seq));
        client.send(ACK, payload);
    };
    let accept_chunk = |client: &mut Client, sequence| {
        assert!(client.window.accept(sequence));
        client.send(ACK, Vec::new());
    };
    let check_delta = |chunk: &ReceivedChunk, corrected, command_frame| {
        let mut bits = chunk.bits();
        assert_eq!(bits.bits(3).unwrap(), if corrected { 6 } else { 2 });
        assert_eq!(bits.w_var().unwrap(), command_frame);
    };

    let (_, h0) = wait_chunk(client);
    assert_eq!((h0.first, h0.span), (0, 1));
    assert_eq!(h0.bits().bits(2).unwrap(), 0);
    skip_chunk(client, Vec::new());
    let (sequence, retry) = wait_chunk(client);
    assert_eq!(retry, h0);
    accept_chunk(client, sequence);

    let (_, c0) = wait_chunk(client);
    assert_eq!((c0.first, c0.span), (2, 0));
    check_delta(&c0, false, start);
    skip_chunk(client, command_batch(start + 1, 16, 0, 0, forward));
    let (sequence, retry) = wait_chunk(client);
    assert_eq!(retry, c0);
    accept_chunk(client, sequence);

    let (_, c1) = wait_chunk(client);
    assert_eq!((c1.first, c1.span), (3, 0));
    check_delta(&c1, true, start + 16);
    skip_chunk(client, command_batch(start + 17, 1, 0, 0, forward));
    let (sequence, cumulative) = wait_chunk(client);
    assert_eq!((cumulative.first, cumulative.span), (3, 1));
    check_delta(&cumulative, true, start + 17);
    accept_chunk(client, sequence);
    start + 17
}

pub fn exercise_hero_selection(
    client: &mut Client,
    schemas: &Schemas,
    assets: &crate::scene_assets::SceneAssets,
    server: &GameService,
    ticket: &Handoff,
) {
    use crate::{
        game_heroes::SOLDIER,
        game_parameters::{OPEN_OR_CLOSE, PICK_HERO},
    };
    const OWNER: u32 = 0xa000_0100;
    const FIRST: u32 = OWNER | 1;
    const SECOND: u32 = OWNER | 2;

    let send = |client: &mut Client, ordinal, guid, parameters| {
        send_game_message(client, schemas, ordinal, guid, parameters);
    };
    let next_body_frame = |client: &mut Client, body| {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let packet = client.receive();
            let records = world_records(schemas, &packet);
            if records
                .iter()
                .any(|record| record.entity == body && record.is_create())
            {
                break (packet, records);
            }
            assert!(client.window.accept(packet.seq));
            client.send(ACK, Vec::new());
            assert!(Instant::now() < deadline, "waiting for the selected body");
        }
    };
    let describe = |records: &[WorldRecord]| {
        records
            .iter()
            .filter(|record| matches!(record.entity, OWNER | FIRST | SECOND))
            .map(|record| {
                (
                    record.entity,
                    record.operation,
                    record.script.as_ref().map(|script| script.span),
                )
            })
            .collect::<Vec<_>>()
    };
    let controller =
        crate::game_controller::Controller::load(assets.graphs(), PRACTICE_RANGE_MODE).unwrap();
    let verify_controller = |records: &[WorldRecord], hero, open| {
        let script = records
            .iter()
            .find(|record| record.entity == OWNER && record.script.is_some())
            .and_then(|record| record.script.as_ref())
            .expect("hero selection controller frame");
        assert_eq!(script.first, 0);
        let mut reader = script.bits();
        assert_eq!(reader.bits(3).unwrap(), 2);
        let cmfd = reader.w_var().unwrap();
        let expected = controller
            .controller_frame(
                cmfd,
                ControllerFrame {
                    select_open: open,
                    skins: open,
                    hide_assemble: true,
                    hero: Some(hero),
                    ..ControllerFrame::default()
                },
            )
            .unwrap();
        assert_eq!(script.width, expected.bit_len());
        assert_eq!(script.data, expected.as_bytes());
        cmfd
    };

    send(client, 1, PICK_HERO, SOLDIER.to_le_bytes().to_vec());
    let (_lost, records) = next_body_frame(client, FIRST);
    let expected = [
        (FIRST, Some(1), None),
        (OWNER, Some(2), None),
        (OWNER, None, Some(2)),
    ];
    assert_eq!(describe(&records), expected);
    assert!(
        records
            .iter()
            .any(|record| record.entity == FIRST && record.movement)
    );
    let first_cmfd = verify_controller(&records, SOLDIER, false);

    // A later datagram ACK identifies the first body batch as lost.
    let following = client.receive();
    assert!(client.window.accept(following.seq));
    assert!(describe(&world_records(schemas, &following)).is_empty());
    client.send(ACK, Vec::new());
    let (retry, records) = next_body_frame(client, FIRST);
    assert_eq!(describe(&records), expected);
    assert_eq!(verify_controller(&records, SOLDIER, false), first_cmfd);
    assert!(client.window.accept(retry.seq));

    let tick = BitReader::new(&retry.payload).var_a().unwrap();
    let first_input = tick + 32;
    let mut newest = first_input + 4;
    // The acknowledged stock body has an owner VM, so its own movement
    // records are owner corrections, as from Python's correction.own_record.
    let mut corrected = false;
    let mut await_input = |client: &mut Client, expected| {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let packet = client.receive();
            assert!(client.window.accept(packet.seq));
            corrected |= world_records(schemas, &packet)
                .iter()
                .any(|record| record.entity == FIRST && record.movement);
            if input_ack(schemas, &packet) == expected {
                return;
            }
            client.send(ACK, Vec::new());
            assert!(
                Instant::now() < deadline,
                "waiting for command acknowledgement"
            );
        }
    };
    client.send(ACK, input_batch(first_input, 5, -31010));
    await_input(client, newest);
    client.send(ACK, input_batch(newest + 2_000, 1, -31010));
    for _ in 0..2 {
        let packet = client.receive();
        assert!(client.window.accept(packet.seq));
        assert_eq!(input_ack(schemas, &packet), newest);
        client.send(ACK, Vec::new());
    }
    newest += 48;
    let recoverable = input_batch(newest, 1, -31010);
    for _ in 0..=RECEIVE_BATCH {
        client.send(ACK, recoverable.clone());
    }
    await_input(client, newest);
    // Only the tagged stock pack completes owner VM startup.
    let soldier = assets.heroes().get(SOLDIER).unwrap();
    let attached = assets.graphs().bodies()[&soldier.body]
        .target_tags
        .is_some();
    assert_eq!(corrected, attached);

    let mut open = vec![1, 1];
    open.extend(OWNER.to_le_bytes());
    open.extend(OWNER.to_le_bytes());
    send(client, 2, OPEN_OR_CLOSE, open);
    let deadline = Instant::now() + Duration::from_secs(2);
    let reopen_cmfd = loop {
        let packet = client.receive();
        assert!(client.window.accept(packet.seq));
        let records = world_records(schemas, &packet);
        let changes: Vec<_> = describe(&records)
            .into_iter()
            .filter(|(entity, operation, _)| *entity != FIRST || operation.is_some())
            .collect();
        if !changes.is_empty() {
            assert_eq!(changes, [(OWNER, None, Some(3))]);
            break verify_controller(&records, SOLDIER, true);
        }
        client.send(ACK, Vec::new());
        assert!(
            Instant::now() < deadline,
            "waiting for hero selection to reopen"
        );
    };
    assert!(reopen_cmfd > first_cmfd);

    let tracer = assets.heroes().find_hero("Tracer").unwrap().guid;
    send(client, 3, PICK_HERO, tracer.to_le_bytes().to_vec());
    let (swap, records) = next_body_frame(client, SECOND);
    assert_eq!(
        describe(&records),
        [
            (SECOND, Some(1), None),
            (OWNER, Some(2), None),
            (FIRST, Some(3), None),
            (OWNER, None, Some(4))
        ],
    );
    assert!(verify_controller(&records, tracer, false) > reopen_cmfd);
    assert!(client.window.accept(swap.seq));
    client.send(ACK, Vec::new());

    let skin = SkinTheme {
        guid: 0x0a50_0000_0000_1234,
        golden: true,
    };
    server.update_skin(ticket, tracer, skin).unwrap();
    let third = OWNER | 3;
    let (reskin, records) = next_body_frame(client, third);
    assert_eq!(
        records
            .iter()
            .find(|record| record.entity == third)
            .unwrap()
            .skin,
        Some((0x1234, true))
    );
    assert!(
        records
            .iter()
            .any(|record| record.entity == SECOND && record.operation == Some(3))
    );
    assert!(client.window.accept(reskin.seq));
    client.send(ACK, Vec::new());
}
