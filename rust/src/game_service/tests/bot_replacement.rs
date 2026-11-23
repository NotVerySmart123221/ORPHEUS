use super::*;
use crate::{
    game_bots::BOT_RESPAWN_TIME,
    game_heroes::HeroCatalog,
    world::{Create, Operation, health},
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    schema_version: u32,
    input: Input,
    expected: Expected,
}

#[derive(Deserialize)]
struct Input {
    bot_ids: [u32; 5],
    replacement_interval_ms: u64,
    notice_grace_ms: u64,
    room_weapon_hit: f64,
}

#[derive(Deserialize)]
struct Expected {
    independent_transition: [(u32, u8); 2],
    coalesced_transition: [(u32, u8); 2],
    first_transition: [(u32, u8); 2],
    second_transition: [(u32, u8); 2],
}

fn contract() -> Fixture {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/training-bot-respawn-delivery-v1.json"
    ))
    .unwrap();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(
        BOT_RESPAWN_TIME,
        Duration::from_millis(fixture.input.replacement_interval_ms)
    );
    assert_eq!(
        SILENCE_TIME,
        Duration::from_millis(fixture.input.notice_grace_ms)
    );
    fixture
}

fn create_bot(entity: u32) -> EntityUpdate {
    EntityUpdate {
        entity,
        operation: Some(Operation::Create(Box::new(Create::new(1, Vec::new())))),
        ..EntityUpdate::default()
    }
}

fn enqueue(peer: &mut Peer, retired: u32, created: u32, now: Duration) {
    peer.queue_bot_replacement(
        BotReplacement {
            retired,
            created: create_bot(created),
            waits_for_presentation: false,
        },
        now + SILENCE_TIME,
    );
}

fn send_records(peer: &mut Peer, schemas: &Schemas, sequence: &mut u32) -> (u32, Vec<(u32, u8)>) {
    *sequence += 1;
    let mut packet = Packet::default();
    let sent = peer
        .world
        .send_frame(
            FIRST_TICK,
            Vec::new(),
            false,
            None,
            &mut peer.egress,
            |payload| {
                packet.payload = payload;
                Ok::<_, io::Error>(*sequence)
            },
        )
        .unwrap();
    let records = world_records(schemas, &packet)
        .into_iter()
        .filter_map(|record| record.operation.map(|op| (record.entity, op)))
        .collect();
    (sent.sequence, records)
}

fn acknowledge(peer: &mut Peer, sent_sequence: u32, now: Duration) {
    let report = peer.egress.acknowledge(sent_sequence + 1, 1);
    peer.world.acknowledge(report, now);
}

fn started_peer(schemas: &Schemas, bots: &[u32], slot: i8) -> (Peer, u32) {
    let owner = player_entity(slot as usize);
    let card = PlayerCard::default();
    let viewer = PlayerAppearance {
        entity: owner,
        slot,
        team: 0,
        card: &card,
    };
    let scene = PreparedScene::fixture_for_owner_admission(HeroCatalog::default());
    let mut peer = Peer::new(owner);
    peer.world.start(&scene, &viewer, FIRST_TICK).unwrap();
    for &bot in bots {
        peer.world.create_bot(create_bot(bot));
    }
    let mut sequence = 0;
    while !peer.world.ready().is_empty() {
        assert!(sequence < 8, "fixture world did not finish loading");
        let (sent_sequence, _) = send_records(&mut peer, schemas, &mut sequence);
        acknowledge(&mut peer, sent_sequence, Duration::ZERO);
    }
    for &bot in bots {
        assert!(peer.world.identity_ready(bot));
    }
    (peer, sequence)
}

#[test]
fn settled_bot_weapon_damage_reaches_shooter_and_observer_delivery() {
    use crate::{
        commands::Command,
        game_bots::Bot,
        game_combat::{PosedHitscan, ShotContext},
        game_hitscan::{DamageTags, HitDefinition},
        game_instance::RETAIL_QUANTA,
    };

    let input = contract().input;
    let target = input.bot_ids[0];
    let mut bot = Bot::fixture_walker([4.0, 0.0, 0.0]);
    bot.entity = target;
    bot.position = [4.0, 0.0, 0.0];
    let maximum = bot.spawn.health;
    let mut scene = PreparedScene::fixture_for_bots(vec![bot]);
    let (room, queries) = scene.bot_simulation();
    let schemas = fixture();
    let (mut shooter, mut shooter_sequence) = started_peer(&schemas, &[target], 0);
    let (mut observer, mut observer_sequence) = started_peer(&schemas, &[target], 1);
    let tick = FIRST_TICK;
    let hit = room
        .hitscan(
            &queries,
            PosedHitscan {
                source: shooter.world.owner() | 1,
                origin: [0.0, 0.8, 0.0],
                direction: [1.0, 0.0, 0.0],
                command: Command {
                    frame: tick,
                    ..Command::default()
                },
                shot_frame: tick,
                reach: 8.0,
                hit: HitDefinition {
                    tags: DamageTags::EMPTY,
                    amount: input.room_weapon_hit,
                    loadout: None,
                    critical: 1.0,
                    falloff: None,
                },
            },
            ShotContext {
                team: 0,
                quanta: RETAIL_QUANTA,
                tick,
                now: Duration::ZERO,
            },
            |entity| entity == target,
        )
        .unwrap()
        .unwrap();
    shooter.bot_hit(hit);
    assert_eq!(
        room.body_healths().unwrap()[&target],
        maximum - input.room_weapon_hit
    );
    let [update]: [EntityUpdate; 1] = room.take_scripted_health().try_into().unwrap();
    for (peer, sequence) in [
        (&mut shooter, &mut shooter_sequence),
        (&mut observer, &mut observer_sequence),
    ] {
        assert!(!peer.world.health_pending(target));
        peer.world.queue_health(update.clone());
        let (_, records) = send_records(peer, &schemas, sequence);
        assert_eq!(records, [(target, 2)]);
    }
}

#[test]
fn pending_respawn_chain_preserves_notice_gate_and_independent_bot_delivery() {
    let Fixture {
        input: Input { bot_ids, .. },
        expected,
        ..
    } = contract();
    let [a, b, c, d, e] = bot_ids;
    let schemas = fixture();
    let (mut peer, mut sequence) = started_peer(&schemas, &[a, d], 0);
    let first = BOT_RESPAWN_TIME;
    let second = first + BOT_RESPAWN_TIME;

    // Leave A's last HP update in flight while both respawns occur in the room.
    peer.world.queue_health(EntityUpdate {
        entity: a,
        operation: Some(Operation::Update(vec![health(0.0, 200.0, 0.0, 0.0)])),
        ..EntityUpdate::default()
    });
    let (_, old_health) = send_records(&mut peer, &schemas, &mut sequence);
    assert_eq!(old_health, [(a, 2)]);
    enqueue(&mut peer, a, b, first);
    peer.flush_combat(&schemas, first, None).unwrap();
    assert!(peer.world.ready().is_empty());

    enqueue(&mut peer, b, c, second);
    enqueue(&mut peer, d, e, second);
    peer.flush_combat(&schemas, second, None).unwrap();
    let (unrelated, records) = send_records(&mut peer, &schemas, &mut sequence);
    assert_eq!(records, expected.independent_transition);
    acknowledge(&mut peer, unrelated, second);
    assert!(peer.world.identity_ready(e));
    assert!(peer.world.health_pending(a));

    // The later receipt reports A's update missing. Retry it before A can retire.
    let (retry, records) = send_records(&mut peer, &schemas, &mut sequence);
    assert_eq!(records, [(a, 2)]);
    acknowledge(&mut peer, retry, second);
    peer.flush_combat(&schemas, second, None).unwrap();
    let (coalesced, records) = send_records(&mut peer, &schemas, &mut sequence);
    assert_eq!(records, expected.coalesced_transition);
    acknowledge(&mut peer, coalesced, second);
    assert!(!peer.world.identity_ready(b));
    assert!(peer.world.identity_ready(c));
}

#[test]
fn next_respawn_waits_for_the_preceding_create_acknowledgement() {
    let Fixture {
        input: Input { bot_ids, .. },
        expected,
        ..
    } = contract();
    let [a, b, c, ..] = bot_ids;
    let schemas = fixture();
    let (mut peer, mut sequence) = started_peer(&schemas, &[a], 0);
    let first = BOT_RESPAWN_TIME;
    let second = first + BOT_RESPAWN_TIME;

    enqueue(&mut peer, a, b, first);
    peer.flush_combat(&schemas, first, None).unwrap();
    let (first_packet, records) = send_records(&mut peer, &schemas, &mut sequence);
    assert_eq!(records, expected.first_transition);
    assert!(!peer.world.identity_ready(b));

    enqueue(&mut peer, b, c, second);
    peer.flush_combat(&schemas, second, None).unwrap();
    assert!(peer.world.ready().is_empty());
    let after_notice_expiry = second + SILENCE_TIME;
    peer.flush_combat(&schemas, after_notice_expiry, None)
        .unwrap();
    assert!(peer.world.ready().is_empty());

    acknowledge(&mut peer, first_packet, after_notice_expiry);
    assert!(peer.world.identity_ready(b));
    peer.flush_combat(&schemas, after_notice_expiry, None)
        .unwrap();
    let (second_packet, records) = send_records(&mut peer, &schemas, &mut sequence);
    assert_eq!(records, expected.second_transition);
    acknowledge(&mut peer, second_packet, after_notice_expiry);
    assert!(peer.world.identity_ready(c));
}

#[test]
fn stale_respawn_from_before_peer_start_does_not_block_next_generation() {
    let Fixture {
        input: Input { bot_ids, .. },
        expected,
        ..
    } = contract();
    let [a, b, c, ..] = bot_ids;
    let schemas = fixture();
    let (mut peer, mut sequence) = started_peer(&schemas, &[b], 0);
    let first = BOT_RESPAWN_TIME;
    let second = first + BOT_RESPAWN_TIME;

    enqueue(&mut peer, a, b, first);
    enqueue(&mut peer, b, c, second);
    peer.flush_combat(&schemas, second, None).unwrap();
    let (current, records) = send_records(&mut peer, &schemas, &mut sequence);
    assert_eq!(records, expected.second_transition);
    acknowledge(&mut peer, current, second);
    assert!(peer.world.identity_ready(c));
}
