use std::collections::HashMap;

use super::*;
use crate::{scene_assets::SceneAssets, script_graph::graph_asset_index};

#[test]
#[ignore = "requires a prepared 104319 Practice Range scene with stock bot definitions"]
fn prepared_worker_delivers_stock_projectile_launches_for_both_teams() {
    let runtime = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let schemas = Arc::new(Schemas::load(runtime.join("data/game_schemas_174.json")).unwrap());
    let assets = SceneAssets::load(runtime).unwrap();
    let scene = assets.prepare(GameMap::practice_range()).unwrap();
    let mut firing_bodies = HashMap::new();
    let mut firing_controllers = HashMap::new();
    for bot in scene.bots().unwrap() {
        if bot
            .spawn
            .placement_graphs
            .iter()
            .copied()
            .filter_map(graph_asset_index)
            .filter_map(|graph| assets.graphs().graph(graph))
            .any(|graph| graph.drives_ai_weapon_input())
        {
            firing_bodies.insert(bot.entity, bot.spawn.team);
            if let Some(controller) = bot.controller() {
                firing_controllers.insert(controller, bot.spawn.team);
            }
        }
    }
    assert!(firing_bodies.values().any(|&team| team == 0));
    assert!(firing_bodies.values().any(|&team| team == 1));

    #[cfg(feature = "inspector")]
    crate::owner_trace::start_playtest_capture();
    let server = GameService::bind(local(), Arc::clone(&schemas)).unwrap();
    let ticket = server
        .open_prepared_solo(MATCH, scene, player().pop().unwrap(), PlayerCard::default())
        .unwrap();
    let mut client = Client::new(server.local_addr(), &ticket);
    client.handshake();
    let load = client.receive();
    assert!(client.window.accept(load.seq));
    assert_eq!(messages(&schemas, &load)[0].message.id, 20300);
    let loaded = encode(&schemas, 21602, &Fields::new()).unwrap();
    let mut payload = BitWriter::new();
    write_channel(&mut payload, &[(0, loaded)], &[]).unwrap();
    client.send(ACK, payload.into_bytes());

    let mut acknowledged_baselines = HashSet::new();
    let mut teams = [false; 2];
    let deadline = Instant::now() + Duration::from_secs(16);
    let mut projectile_creates = 0;
    while teams != [true; 2] {
        let packet = client.receive();
        assert!(client.window.accept(packet.seq));
        let records = world_records(&schemas, &packet);
        for record in &records {
            projectile_creates += usize::from(
                record.is_create() && (0xa00c_0000..0xa010_0000).contains(&record.entity),
            );
            if let Some((&team, body)) = firing_controllers
                .get(&record.entity)
                .zip(record.possessed_body)
            {
                firing_bodies.insert(body, team);
            }
            if record.has_projectile_launch {
                let &team = firing_bodies.get(&record.entity).unwrap();
                assert!(acknowledged_baselines.contains(&record.entity));
                teams[usize::from(team)] = true;
            }
        }
        client.send(ACK, Vec::new());
        acknowledged_baselines.extend(records.iter().filter_map(|record| {
            record
                .script
                .as_ref()
                .filter(|script| script.first == 0)
                .map(|_| record.entity)
        }));
        assert!(
            Instant::now() < deadline,
            "missing NPC launch on a team: {teams:?}; acknowledged {} baselines, received {projectile_creates} projectile creates",
            acknowledged_baselines.len()
        );
    }
}
