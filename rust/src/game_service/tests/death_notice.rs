use super::*;
use crate::{
    game_combat::PlayerDeath,
    game_heroes::HeroCatalog,
    game_parameters::{CLIENT_GAME_MESSAGE, PICK_HERO},
    game_replication::BodyId,
    script_player::fixture_body_factory,
    vitals::Death,
};
use serde::Deserialize;

fn acknowledge_world_frame(peer: &mut Peer, sequence: &mut u32) {
    *sequence += 1;
    peer.world
        .send_frame(
            FIRST_TICK,
            Vec::new(),
            false,
            None,
            &mut peer.egress,
            |_| Ok::<_, io::Error>(*sequence),
        )
        .unwrap();
    // Bit zero acknowledges the sequence immediately before the window's next value.
    let report = peer.egress.acknowledge(*sequence + 1, 1);
    peer.world.acknowledge(report, Duration::ZERO);
}

fn pick_body(
    peer: &mut Peer,
    scene: &mut PreparedScene,
    viewer: &PlayerAppearance<'_>,
    hero: u64,
) -> BodyId {
    peer.world.queue_game_message(
        FIRST_TICK,
        Duration::ZERO,
        Message {
            id: CLIENT_GAME_MESSAGE,
            fields: Fields::from([
                (
                    0x78,
                    Value::Struct(Fields::from([(
                        0,
                        Value::Unsigned(u64::from(viewer.entity)),
                    )])),
                ),
                (0x80, Value::Blob(hero.to_le_bytes().to_vec())),
                (0xa8, Value::Unsigned(PICK_HERO)),
                (0xb0, Value::Signed(3)),
            ]),
        },
    );
    assert_eq!(
        peer.world
            .advance_input(scene, viewer, RETAIL_QUANTA, 1)
            .unwrap(),
        (1, false)
    );
    peer.world.active_body_id().unwrap()
}

fn kill_body(scene: &mut PreparedScene, body: BodyId, damage: f64) -> PlayerDeath {
    let simulation = scene.simulation_mut();
    let (owner, killed, _) = simulation
        .damage_player_body_by_wire(
            body,
            damage,
            Death {
                time: 0.0,
                frame: FIRST_TICK,
                killer: body.entity,
            },
            None,
        )
        .unwrap();
    assert!(killed);
    assert!(!simulation.player_body(body).unwrap().alive());
    PlayerDeath {
        body,
        owner,
        source: body.entity,
        killed_by: false,
    }
}

#[test]
fn death_notice_is_scoped_to_the_active_body_generation_until_reliable_admission() {
    #[derive(Deserialize)]
    struct Contract {
        schema_version: u32,
        input: Input,
        expected: Expected,
    }
    #[derive(Deserialize)]
    struct Input {
        heroes: [u64; 2],
        lethal_damage: f64,
    }
    #[derive(Deserialize)]
    struct Expected {
        held_before_mode_ack: usize,
        pending_after_replacement: usize,
        admitted_after_mode_ack: u32,
    }

    let Contract {
        schema_version,
        input,
        expected,
    } = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/owner-death-notice-v1.json"
    ))
    .unwrap();
    assert_eq!(schema_version, 1);

    let [first_hero, next_hero] = input.heroes;
    let heroes = HeroCatalog::from_json(
        &serde_json::json!({
            format!("0x{first_hero:016X}"): {
                "name": "First fixture hero", "class": "Damage", "body": "0x0400000000000001",
            },
            format!("0x{next_hero:016X}"): {
                "name": "Second fixture hero", "class": "Damage", "body": "0x0400000000000001",
            },
        })
        .to_string(),
    )
    .unwrap();
    let mut scene = PreparedScene::fixture_for_owner_admission(heroes);
    let (factory, _) = fixture_body_factory();
    scene.owner_body_factory = Some(Arc::new(move |_, spawn, setup| {
        Ok(Some(Box::new(factory.start_with_setup(spawn, setup)?)))
    }));

    let card = PlayerCard::default();
    let owner = player_entity(0);
    let viewer = PlayerAppearance {
        entity: owner,
        slot: 0,
        team: 0,
        card: &card,
    };
    let mut peer = Peer::new(owner);
    peer.world.start(&scene, &viewer, FIRST_TICK).unwrap();
    let mut sequence = 0;
    while !peer.world.identity_ready(owner) || !peer.world.identity_ready(GAME_MODE_ENTITY) {
        assert!(sequence < 8, "world identities were not acknowledged");
        acknowledge_world_frame(&mut peer, &mut sequence);
    }
    peer.world.present(&scene, FIRST_TICK).unwrap();
    assert!(!peer.world.mode_ready());

    // The schema only provides an envelope for this reliable delivery test.
    let schemas = Schemas::from_json(r#"{"8F89E0DF":{"25004":[]}}"#).unwrap();
    let first = pick_body(&mut peer, &mut scene, &viewer, first_hero);
    let death = kill_body(&mut scene, first, input.lethal_damage);
    peer.queue_player_death(death);
    peer.flush_combat(&schemas, Duration::ZERO, Some(scene.simulation()))
        .unwrap();
    assert_eq!(
        peer.pending_combat_notices().count(),
        expected.held_before_mode_ack
    );

    let replacement = pick_body(&mut peer, &mut scene, &viewer, next_hero);
    assert!(replacement.generation > first.generation);
    assert!(scene.simulation().player_body(replacement).unwrap().alive());
    peer.queue_player_death(death);
    peer.queue_player_death(PlayerDeath {
        body: BodyId {
            generation: first.generation,
            ..replacement
        },
        ..death
    });
    assert_eq!(
        peer.pending_combat_notices().count(),
        expected.held_before_mode_ack,
        "a death dispatched late must be rejected, even after recycling a wire ID"
    );
    peer.flush_combat(&schemas, Duration::ZERO, Some(scene.simulation()))
        .unwrap();
    assert_eq!(
        peer.pending_combat_notices().count(),
        expected.pending_after_replacement
    );

    // An actual death of the active replacement still reaches reliable delivery.
    let death = kill_body(&mut scene, replacement, input.lethal_damage);
    peer.queue_player_death(death);
    assert_eq!(
        peer.pending_combat_notices().copied().collect::<Vec<_>>(),
        [CombatNotice::Death {
            source: replacement.entity,
            killed_by: false,
        }]
    );
    while !peer.world.mode_ready() {
        assert!(sequence < 16, "mode UI was not acknowledged");
        acknowledge_world_frame(&mut peer, &mut sequence);
    }
    assert!(!peer.egress.delivered_through(0));
    peer.flush_combat(&schemas, Duration::ZERO, Some(scene.simulation()))
        .unwrap();
    assert_eq!(peer.pending_combat_notices().count(), 0);
    acknowledge_world_frame(&mut peer, &mut sequence);
    assert!(
        peer.egress
            .delivered_through(expected.admitted_after_mode_ack - 1)
    );
    assert!(
        !peer
            .egress
            .delivered_through(expected.admitted_after_mode_ack)
    );
}
