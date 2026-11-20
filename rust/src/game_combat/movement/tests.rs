use std::{collections::BTreeMap, sync::Arc};

use super::super::{PlayerBodyState, RoomSimulation, tests::fixture_bots};
use crate::{
    collision_index::TriangleIndex,
    commands::Command,
    contact_world::ContactWorld,
    game_replication::BodyId,
    mover::Mover,
    mover_modifiers::{Modifier, ModifierKey, Operation},
    mover_settings::{MoverCatalog, MoverSettings},
};

#[test]
fn movement_snapshot_honors_base_and_next_pose_contact_participation() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/movement-contact-participation-v1.json"
    ))
    .unwrap();
    assert_eq!(fixture["schema_version"], 1);
    assert_eq!(fixture["evidence"], "synthetic-rust");
    let input = &fixture["input"];
    let number = |key: &str| u32::try_from(input[key].as_u64().unwrap()).unwrap();
    let birth_frame = number("birth_frame");
    let disabled_frame = number("disabled_sample_frame");
    let restored_frame = number("restored_sample_frame");
    let source = BodyId {
        entity: number("source_wire"),
        generation: 1,
    };
    let target = BodyId {
        entity: number("target_wire"),
        generation: 1,
    };
    let catalog = Arc::new(
        MoverCatalog::from_json(&serde_json::to_string(&input["mover_catalog"]).unwrap()).unwrap(),
    );
    let mut bots = fixture_bots();
    Arc::get_mut(&mut bots[0].spawn).unwrap().body = input["disabled_npc_body"].as_u64().unwrap();
    Arc::get_mut(&mut bots[1].spawn).unwrap().body = input["enabled_npc_body"].as_u64().unwrap();
    let enabled_npc = bots[1].contact_capsule(catalog.for_body(Some(bots[1].spawn.body)));
    let disabled_npc = bots[0].contact_capsule(catalog.for_body(Some(bots[0].spawn.body)));
    let [disabled_npc_wire, enabled_npc_wire] = [bots[0].entity, bots[1].entity];
    let mut room = RoomSimulation::new(bots).unwrap();
    for (slot, (id, team, position)) in [
        (source, 0, &input["source_position"]),
        (target, 1, &input["target_position"]),
    ]
    .into_iter()
    .enumerate()
    {
        let mover = Mover::new(
            MoverSettings::default(),
            Arc::new(ContactWorld::new(Vec::new()).unwrap()),
            serde_json::from_value(position.clone()).unwrap(),
            0,
            0x18,
        )
        .unwrap();
        room.spawn_player_body(PlayerBodyState::new(
            id,
            id.entity + 100,
            u8::try_from(slot).unwrap(),
            team,
            7,
            mover,
            None,
            birth_frame,
            None,
        ))
        .unwrap();
    }
    let target_capsule = room.player_body(target).unwrap().mover.contact_capsule();
    let source_capsule = room.player_body(source).unwrap().mover.contact_capsule();
    room.player_body_mut(target).unwrap().mover.track_modifiers(
        birth_frame,
        &BTreeMap::from([(
            ModifierKey {
                instance: 1,
                state: 1,
                slot: 0,
            },
            Modifier {
                index: u8::try_from(number("modifier_slot")).unwrap(),
                operation: Operation::Set,
                value: 0.0,
                priority: 0.0,
            },
        )]),
    );
    room.enable_movement_contacts(false, catalog);
    room.prepare_movement_frame(disabled_frame).unwrap();
    let initial = room.movement_contact_frame().unwrap();
    let initial_blockers = initial.blockers(source.entity, 0);
    assert_eq!(
        initial_blockers.len(),
        fixture["expected"]["disabled_blockers"].as_u64().unwrap() as usize
    );
    assert_eq!(initial_blockers, [enabled_npc]);
    assert!(!initial_blockers.contains(&disabled_npc));
    assert!(!initial_blockers.contains(&target_capsule));
    for (wire, expected) in [
        (disabled_npc_wire, &[][..]),
        (enabled_npc_wire, std::slice::from_ref(&source_capsule)),
    ] {
        let bot = room.body_mut(wire).unwrap();
        initial.stage_bot(bot, disabled_frame);
        assert_eq!(bot.movement_blockers(disabled_frame), expected);
    }

    // Publish the modifier's retirement after frame 101. It applies to the
    // next pose; the room's already frozen frame remains immutable.
    let target_state = room.player_body_mut(target).unwrap();
    target_state
        .mover
        .step(
            &TriangleIndex::new([]).unwrap(),
            Command {
                frame: disabled_frame,
                ..Command::default()
            },
            0.016,
            |_, _| {},
        )
        .unwrap();
    target_state
        .mover
        .track_modifiers(disabled_frame, &BTreeMap::new());
    let restored_target = target_state.mover.contact_capsule();
    room.prepare_movement_frame(disabled_frame).unwrap();
    assert!(Arc::ptr_eq(
        &initial,
        &room.movement_contact_frame().unwrap()
    ));
    assert_eq!(initial.blockers(source.entity, 0), initial_blockers);

    room.prepare_movement_frame(restored_frame).unwrap();
    let restored = room.movement_contact_frame().unwrap();
    let blockers = restored.blockers(source.entity, 0);
    assert_eq!(
        blockers.len(),
        fixture["expected"]["restored_blockers"].as_u64().unwrap() as usize
    );
    assert!(blockers.contains(&enabled_npc));
    assert!(blockers.contains(&restored_target));
    assert!(!blockers.contains(&disabled_npc));
    // A same-team observer sees only the opposing source body.
    assert_eq!(restored.blockers(999, 1), [source_capsule]);
}
