use super::*;

mod junkenstein;

#[test]
#[ignore = "requires separately prepared 104319 Junkenstein runtime"]
fn normal_cassidy_primary_and_fan_damage_all_boss_bodies() {
    use crate::junkenstein_navigation::{JUNKRAT, REAPER, ROADHOG, SUMMONER, WITCH};
    junkenstein::verify_normal_cassidy_damage_and_charge(
        &[REAPER, ROADHOG, JUNKRAT, WITCH, SUMMONER],
        &[(false, false), (false, true), (true, true)],
    );
}

#[test]
#[ignore = "requires separately prepared 104319 Junkenstein runtime"]
fn normal_cassidy_common_npc_damage_charge_and_retirement() {
    use crate::junkenstein_navigation::{SHOCK_TIRE, ZOMBARDIER};
    const ZOMNIC: u64 = 0x02e0_0000_0000_0175;
    junkenstein::verify_normal_cassidy_damage_and_charge(
        &[ZOMNIC, ZOMBARDIER, SHOCK_TIRE],
        &[(false, false)],
    );
}

use crate::game_bots::BOT_ENTITY;

#[test]
#[ignore = "requires a prepared 104319 Practice Range pack"]
fn repeated_rooms_share_collision_indexes_and_keep_independent_bodies() {
    let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let assets = SceneAssets::load(runtime).unwrap();
    let first = assets.prepare(GameMap::practice_range()).unwrap();
    let mut second = assets.prepare(GameMap::practice_range()).unwrap();
    assert!(Arc::ptr_eq(&first.ground, &second.ground));
    assert!(Arc::ptr_eq(
        first.contacts.as_ref().unwrap(),
        second.contacts.as_ref().unwrap()
    ));
    let before = first.bots().unwrap().next().unwrap().position;
    let after = [before[0] + 1.0, before[1], before[2]];
    second
        .bot_simulation()
        .0
        .body_mut(BOT_ENTITY)
        .unwrap()
        .position = after;
    assert_eq!(first.bots().unwrap().next().unwrap().position, before);
    assert_eq!(second.bots().unwrap().next().unwrap().position, after);
}

#[test]
#[ignore = "requires a prepared 104319 Practice Range pack"]
fn prepared_practice_debug_view_contains_real_collision_layers() {
    let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let assets = SceneAssets::load(runtime).unwrap();
    let scene = assets.prepare(GameMap::practice_range()).unwrap();
    let view = serde_json::to_value(scene.debug_geometry().view()).unwrap();

    assert_eq!(view["map"], "0800000000000688");
    assert_eq!(view["sources"]["movement_cache"], "0688.col");
    assert_eq!(
        view["origin"],
        serde_json::to_value(scene.map().spawn_point(0, 0).unwrap().0).unwrap()
    );
    assert!(view["counts"]["triangles"].as_u64().unwrap() > 100);
    assert!(view["counts"]["proxies"].as_u64().unwrap() > 10);
    assert_eq!(
        view["totals"]["triangles"],
        scene.movement_index().triangle_count()
    );
    assert_eq!(
        view["totals"]["proxies"],
        scene.contacts.as_ref().unwrap().shapes().len()
    );
}

#[test]
#[ignore = "requires a prepared 104319 Practice Range pack without bot VM graphs"]
fn stock_soldier_owner_session_idles_fires_reloads_and_sprints() {
    use crate::commands::Command;
    use crate::game_replication::BodyStep;
    use crate::script_init::{Snapshot, StateKind};
    use crate::script_snapshot_wire::{View, uncertain_polled_switch};
    use crate::script_stream::{BodyStream, DeltaKind};
    use crate::script_wire::Value;
    use std::collections::BTreeSet;

    let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let assets = SceneAssets::load(runtime).unwrap();
    let scene = assets.prepare(GameMap::practice_range()).unwrap();
    assert!(scene.movement_ready());
    let hero = scene.heroes().get(SOLDIER).unwrap();
    let spawn = OwnerBodySpawn {
        slot: 0,
        controller: 0xa000_0100,
        entity: 0xa000_0101,
        body_filter: team_bit(0).unwrap(),
        frame: FIRST_TICK,
        pose: scene.player_spawn_mover(hero.body, 0).unwrap().pose(),
        quanta: RETAIL_QUANTA,
    };
    if assets.graphs.bodies()[&hero.body].target_tags.is_some() {
        let mut runtime = scene.owner_body_runtime(hero, spawn).unwrap().unwrap();
        let snapshot = runtime.snapshot().unwrap();
        let encoder = runtime.encoder();
        // Resolve each graph once at startup. Every later assertion then follows
        // the same instance, including its attachment to the expected parent.
        let resolve = |graph: u32, parent: Option<(u16, u16)>| {
            let mut matches = snapshot
                .instances
                .iter()
                .filter(|(_, instance)| instance.graph == graph && instance.parent == parent);
            let (&id, _) = matches
                .next()
                .unwrap_or_else(|| panic!("missing graph {graph:04x} at {parent:?}"));
            assert!(
                matches.next().is_none(),
                "ambiguous graph {graph:04x} at {parent:?}"
            );
            id
        };
        let body = resolve(0x0033, None);
        let manager = resolve(0x004b, None);
        let sprint_id = resolve(0x0255, None);
        let rifle_id = resolve(0x0254, None);
        let movement = resolve(0x01c7, Some((manager, 0)));
        let equipment = resolve(0x01cf, Some((manager, 3)));
        let idle_id = resolve(0x02c5, Some((manager, 5)));
        let weapon_switch = resolve(0x0ba4, Some((manager, 11)));
        let table_id = resolve(0x05ad, Some((manager, 7)));
        // The authored message table stays in the VM for lookups.
        let table = &snapshot.instances[&table_id];
        assert_eq!(table.graph, 0x05ad);
        assert!(matches!(&table.variables[&29859], Value::Map(entries) if entries.len() == 20));
        let mut streams = Vec::new();
        for view in [View::Owner, View::Observer] {
            let projected = encoder.project(snapshot, view).unwrap();
            let mut stream = BodyStream::new(spawn.entity);
            let baseline = encoder.queue_full(&mut stream, &projected).unwrap();
            stream.create_arrived();
            stream.arrived(baseline.last);
            // Only the unbound table is omitted, as Python omits its None.
            let sent = projected.instance_variables(table_id).unwrap();
            assert!(!sent.contains_key(&29859));
            for (&variable, value) in &table.variables {
                let value = snapshot
                    .resolved_bindings
                    .get(&(table_id, variable))
                    .unwrap_or(value);
                if variable != 29859 && !matches!(value, Value::Null) {
                    assert_eq!(sent.get(&variable), Some(value), "{variable}");
                }
            }
            streams.push((stream, projected));
        }
        // A live session: idle for about 3 s, hold fire for 1.5 s, reload,
        // fire briefly, stand until the idle fidget plays, fire through it,
        // then hold forward and Sprint. Both streams project and encode
        // every frame, as the worker sends them.
        let int = |value: Option<&Value>| match value {
            None => None,
            Some(Value::Integer(value)) => Some(*value),
            Some(value) => panic!("unexpected integer {value:?}"),
        };
        let flag = |value: Option<&Value>| match value {
            None => None,
            Some(Value::Bool(value)) => Some(*value),
            Some(value) => panic!("unexpected flag {value:?}"),
        };
        // A switch polled on a native the owner client may answer
        // differently stays off the owner stream.
        let uncertain = |snapshot: &Snapshot| {
            let mut found = BTreeSet::new();
            for (&id, instance) in &snapshot.instances {
                let graph = assets.graphs.graph(instance.graph).unwrap();
                for (&raw, state) in &instance.states {
                    let node = graph.state(usize::from(raw)).unwrap();
                    if state.active && uncertain_polled_switch(node, state) {
                        found.insert((id, raw));
                    }
                }
            }
            found
        };
        // Check the exact set on this owner, including the switch attached
        // to the weapon manager. Startup allocation can differ across hosts.
        let withheld = BTreeSet::from([
            (body, 10),
            (body, 15),
            (body, 20),
            (body, 21),
            (body, 22),
            (body, 23),
            (manager, 12),
            (manager, 13),
            (manager, 14),
            (movement, 25),
            (movement, 28),
            (movement, 30),
            (movement, 48),
            (equipment, 17),
            (weapon_switch, 0),
        ]);
        assert_eq!(uncertain(snapshot), withheld);
        for (_, projected) in &streams {
            for &(id, raw) in &withheld {
                assert!(!projected.instance_states(id).unwrap().contains(&raw));
            }
        }
        let mut mover = scene.player_spawn_mover(hero.body, 0).unwrap();
        let mut owner = Vec::new();
        let mut owner_hud = Vec::new();
        for t in 1..=900 {
            let frame = FIRST_TICK + t;
            let (forward, buttons) = match t {
                188..=281 | 520..=540 | 600..=640 => (0, 1),
                300 => (0, 0x400),
                700 => (127, 8),
                701..=760 => (127, 0),
                _ => (0, 0),
            };
            let command = Command {
                frame,
                forward,
                buttons,
                ..Command::default()
            };
            mover
                .step(scene.movement_index(), command, 0.016, |_, _| {})
                .unwrap();
            runtime.advance(command, &mover, BodyStep::Posed).unwrap();
            if let Some(modifiers) = runtime.movement_modifiers() {
                mover.track_modifiers(frame, modifiers);
            }
            runtime.take_shots(Some(command), &mover);
            let snapshot = runtime.snapshot().unwrap();
            // The worker cancels the body script at the first diagnostic.
            assert!(
                snapshot.diagnostics.is_empty(),
                "{t}: {:?}",
                snapshot.diagnostics
            );
            // The idle fidget animation has no authored duration. Like
            // the reference, it stays on without a timer until held fire
            // ends it.
            let idle = &snapshot.instances[&idle_id];
            assert_eq!(idle.graph, 0x02c5);
            let playing = idle.states.get(&19).is_some_and(|state| state.active);
            if playing {
                assert_eq!(idle.states[&19].kind, StateKind::Anim);
                assert!(
                    snapshot
                        .pending_timers
                        .get(&idle_id)
                        .into_iter()
                        .flatten()
                        .all(|timer| timer.raw_state != 19)
                );
            }
            assert_eq!(playing, (586..600).contains(&t), "{t}");
            // Sprint starts on the press with forward held, runs while
            // forward stays held and drives its HUD flag, as in the
            // reference.
            let sprint = &snapshot.instances[&sprint_id];
            assert_eq!(sprint.graph, 0x0255);
            let (flags, hud) = match t {
                700 => (5, Some(1)),
                701..=760 => (1, Some(1)),
                761.. => (0, Some(0)),
                _ => (0, None),
            };
            assert_eq!(
                (sprint.states[&1].kind, int(sprint.variables.get(&699))),
                (StateKind::Ability { flags }, hud),
                "{t}"
            );
            let excluded = uncertain(snapshot);
            let encoder = runtime.encoder();
            for (stream, initial) in &mut streams {
                let next = encoder.project_next(snapshot, initial).unwrap();
                let kind = match initial.view() {
                    View::Owner => stream.owner_kind(frame, false, false).unwrap(),
                    View::Observer => Some(DeltaKind::Observer { resend: false }),
                };
                let Some(kind) = kind else { continue };
                let delta = encoder.queue_delta(stream, &next, kind, runtime.frame_ms());
                if let Some(chunk) = delta.unwrap() {
                    stream.sent(chunk.last);
                    stream.arrived(chunk.last);
                }
                if next.view() != View::Owner {
                    continue;
                }
                for &(id, raw) in &excluded {
                    assert!(!next.instance_states(id).unwrap().contains(&raw), "{t}");
                }
                let rifle = next.instance_variables(rifle_id).unwrap();
                let fact = (int(rifle.get(&53)), flag(rifle.get(&51)));
                if owner.last().is_none_or(|&(_, last)| last != fact) {
                    owner.push((t, fact));
                }
                let hud = int(next.instance_variables(sprint_id).unwrap().get(&699));
                if owner_hud.last().is_none_or(|&(_, last)| last != hud) {
                    owner_hud.push((t, hud));
                }
            }
        }
        assert_eq!(streams[0].0.command_frame(), FIRST_TICK + 900);
        // The owner frames carry the Sprint HUD flag the client shows.
        assert_eq!(owner_hud, [(1, None), (700, Some(1)), (761, Some(0))]);
        // The frozen reference produces the same rifle ammo and reload
        // transitions from these inputs: 14 rounds in 1.5 s, the magazine
        // refilled 47 frames after the reload press, the reload ended 46
        // frames later, and six rounds through the idle fidget.
        let mut expected = vec![(1, (Some(30), None))];
        for (t, ammo) in [
            188, 195, 201, 208, 215, 222, 229, 236, 243, 250, 257, 264, 271, 278,
        ]
        .into_iter()
        .zip((16..30).rev())
        {
            expected.push((t, (Some(ammo), None)));
        }
        expected.extend([
            (300, (Some(16), Some(true))),
            (347, (Some(30), Some(true))),
            (393, (Some(30), Some(false))),
            (520, (Some(29), Some(false))),
            (527, (Some(28), Some(false))),
            (533, (Some(27), Some(false))),
            (540, (Some(26), Some(false))),
            (600, (Some(25), Some(false))),
            (607, (Some(24), Some(false))),
            (613, (Some(23), Some(false))),
            (620, (Some(22), Some(false))),
            (627, (Some(21), Some(false))),
            (634, (Some(20), Some(false))),
        ]);
        assert_eq!(owner, expected);
    } else {
        let Err(blocked) = scene.owner_body_runtime(hero, spawn) else {
            panic!("the untagged stock owner body was expected to stop before attach");
        };
        assert_eq!(blocked.kind(), io::ErrorKind::Unsupported);
        assert!(
            blocked
                .to_string()
                .contains("UnsupportedConfig(\"STU_AC829876\")"),
            "{blocked}"
        );
    }
    let tracer = scene.heroes().find_hero("Tracer").unwrap();
    assert!(scene.owner_body_runtime(tracer, spawn).unwrap().is_none());
}

#[test]
#[ignore = "requires a tagged 104319 Practice Range pack"]
fn stock_soldier_casts_biotic_field() {
    use crate::{commands::Command, game_replication::BodyStep};

    let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let assets = SceneAssets::load(runtime).unwrap();
    let scene = assets.prepare(GameMap::practice_range()).unwrap();
    let hero = scene.heroes().get(SOLDIER).unwrap();
    let mover = scene.player_spawn_mover(hero.body, 0).unwrap();
    let spawn = OwnerBodySpawn {
        slot: 0,
        controller: 0xa000_0100,
        entity: 0xa000_0101,
        body_filter: team_bit(0).unwrap(),
        frame: FIRST_TICK,
        pose: mover.pose(),
        quanta: RETAIL_QUANTA,
    };
    let mut owner = scene.owner_body_runtime(hero, spawn).unwrap().unwrap();
    let mut shots = Vec::new();
    for frame in FIRST_TICK + 1..=FIRST_TICK + 120 {
        let command = Command {
            frame,
            buttons: if frame == FIRST_TICK + 40 { 0x10 } else { 0 },
            ..Command::default()
        };
        owner.advance(command, &mover, BodyStep::Posed).unwrap();
        let snapshot = owner.snapshot().unwrap();
        assert!(
            snapshot.initialization_complete(),
            "frame {frame}: {:?}",
            snapshot.diagnostics
        );
        shots.extend(owner.take_shots(Some(command), &mover));
    }
    let [crate::game_combat::PosedShot::BioticField(field)] = shots.as_slice() else {
        panic!("the actual ability must emit exactly one Biotic Field");
    };
    assert_eq!(field.shot.graph, 0x0257);
    assert_eq!(field.feet, mover.pose().position);
}

#[test]
#[ignore = "requires a prepared 104319 Practice Range pack"]
fn elevated_practice_patrols_reach_their_hints_without_crossing_walls() {
    let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let mut assets = SceneAssets::load(runtime).unwrap();
    let mut scene = assets.prepare(GameMap::practice_range()).unwrap();
    assets.enable_contact_trace();
    let mut traced = assets.prepare(GameMap::practice_range()).unwrap();
    assert_eq!(scene.bots().unwrap().len(), 15);
    let scripted = match assets.graphs.graph_indexes().count() {
        373 | 699 | 700 => 0,
        405 => 15,
        count => panic!("no patrol reference profile for the {count}-graph bundle"),
    };
    assert_eq!(scene.scripted_bots(), scripted);
    let bots = [BOT_ENTITY + 4, BOT_ENTITY + 9];
    let mut previous = bots.map(|entity| {
        scene
            .bots()
            .unwrap()
            .find(|bot| bot.entity == entity)
            .unwrap()
            .position
    });
    let goals = [[3, 4], [0, 1]];
    let mut visited = [0; 2];
    let patrol = PatrolScript::load().unwrap();
    for tick in 1001..=2900 {
        scene.step_bots(tick, &patrol).unwrap();
        traced.step_bots(tick, &patrol).unwrap();
        for (slot, entity) in bots.into_iter().enumerate() {
            let bot = scene
                .bots()
                .unwrap()
                .find(|bot| bot.entity == entity)
                .unwrap();
            let traced_bot = traced
                .bots()
                .unwrap()
                .find(|bot| bot.entity == entity)
                .unwrap();
            assert_eq!(
                traced_bot.movement(tick),
                bot.movement(tick),
                "contact trace changed bot {entity:x} at {tick}"
            );
            let before = previous[slot];
            previous[slot] = bot.position;
            let delta = std::array::from_fn::<_, 3, _>(|axis| bot.position[axis] - before[axis]);
            let length = delta[0].hypot(delta[1]).hypot(delta[2]);
            if length > 0.001 {
                let origin = [before[0], before[1] + 1.05, before[2]];
                let category = mover_category(scene.map(), bot.spawn.team);
                let hit = scene
                    .ground
                    .cast_ray(origin, delta.map(|axis| axis / length), length, category)
                    .unwrap();
                assert!(
                    hit.is_none_or(|hit| hit.normal[1].abs() >= 0.1),
                    "bot {entity:x} crossed a wall at {tick}: {before:?} -> {:?}, {hit:?}",
                    bot.position
                );
            }
            if let Some(&index) = goals[slot].get(visited[slot]) {
                let hint = bot.spawn.route[index];
                if (bot.position[0] - hint[0]).hypot(bot.position[2] - hint[2]) < 0.06
                    && (bot.position[1] - hint[1]).abs() < 0.15
                {
                    visited[slot] += 1;
                }
            }
        }
    }
    assert_eq!(visited, [2; 2], "last positions {previous:?}");
}
