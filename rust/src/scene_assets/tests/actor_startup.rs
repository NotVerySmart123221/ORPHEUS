//! Opt-in inventory and startup probes of the unchanged native owner factory.

use super::*;
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use std::{env, io::Write};

const SCENE_INPUTS: &[&str] = &[
    "data/ai_spawners_174.json",
    "data/collision_props_174.json",
    "data/game_graphs_174.json.gz",
    "data/game_heroes_174.json",
    "data/game_movers_174.json",
    "data/game_stats_174.json",
    "data/statescript_codecs_174.json",
];

fn scene_inputs(runtime: &Path) -> BTreeMap<String, Json> {
    let movement_cache = format!(
        "collision/{:04X}.col",
        GameMap::practice_range().map_guid as u32
    );
    SCENE_INPUTS
        .iter()
        .copied()
        .chain(std::iter::once(movement_cache.as_str()))
        .map(|path| {
            let fingerprint = File::open(runtime.join(path)).and_then(|mut file| {
                let mut digest = Sha256::new();
                io::copy(&mut file, &mut digest)?;
                Ok(format!("{:x}", digest.finalize()))
            });
            let row = match fingerprint {
                Ok(sha256) => json!({"sha256": sha256}),
                Err(error) => json!({"error_kind": format!("{:?}", error.kind())}),
            };
            (path.to_owned(), row)
        })
        .collect()
}

fn diagnostic_category(diagnostic: &script_init::Diagnostic) -> &'static str {
    use script_init::Issue;

    if diagnostic.registration_rejected {
        return "registration_rejection";
    }
    match &diagnostic.issue {
        Issue::MissingGraph(_) => "missing_graph",
        Issue::MissingNode(_) => "missing_node",
        Issue::MissingState(_) => "missing_state",
        Issue::UnsupportedAction(_) => "unsupported_action",
        Issue::UnsupportedState(_) => "unsupported_state",
        Issue::UnsupportedField(_) => "unsupported_field",
        Issue::EntityCreation(_) => "entity_creation",
        Issue::InvalidSubScript => "invalid_subscript",
        Issue::Expression(_) => "expression",
        Issue::ObservationUnavailable => "observation_unavailable",
    }
}

fn factory_result(
    result: io::Result<Option<Box<dyn OwnerBodyRuntime>>>,
    not_admitted: &'static str,
) -> Json {
    let runtime = match result {
        Err(error) => {
            return json!({"status": "factory_error", "error_kind": format!("{:?}", error.kind())});
        }
        Ok(None) => return json!({"status": not_admitted}),
        Ok(Some(runtime)) => runtime,
    };
    let Some(snapshot) = runtime.snapshot() else {
        return json!({"status": "snapshot_unavailable"});
    };
    let diagnostics: BTreeSet<_> = snapshot
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.site.graph,
                diagnostic.site.node,
                diagnostic_category(diagnostic),
                diagnostic.server_only,
            )
        })
        .collect();
    let sites: Vec<_> = diagnostics
        .into_iter()
        .map(|(graph, node, category, server_only)| {
            json!({
                "graph": graph, "node": node, "category": category, "server_only": server_only,
            })
        })
        .collect();
    let observations = snapshot.node_observations().map(|observed| {
        let sites: Vec<_> = observed.sites().map(|(graph, node, client_scope_skipped)| {
            json!({"graph": graph, "node": node, "client_scope_skipped": client_scope_skipped})
        }).collect();
        json!({"truncated": observed.truncated(), "sites": sites})
    });
    json!({
        "status": "started",
        "initialization_complete": snapshot.initialization_complete(),
        "instances": snapshot.instances.len(),
        "diagnostics": sites,
        "observations": observations,
    })
}

fn actor_row(assets: &SceneAssets, scene: &PreparedScene, hero: &Hero) -> Json {
    let body = assets.graphs.bodies().get(&hero.body);
    let mover = scene.player_spawn_mover(hero.body, 0);
    let body_status = match body {
        None => "missing",
        Some(body) if body.hero.as_deref().and_then(parse_guid) == Some(hero.guid) => "matched",
        Some(_) => "catalog_mismatch",
    };
    let mut roots = BTreeSet::new();
    if let Some(body) = body {
        roots.extend(body.manager);
        roots.extend(body.weapons.iter().copied().flatten());
        roots.extend(
            body.graphs
                .iter()
                .chain(&body.server_graphs)
                .filter_map(|binding| binding.graph),
        );
    }
    let missing_roots: Vec<_> = roots
        .iter()
        .filter(|&&graph| assets.graphs.graph(graph).is_none())
        .collect();
    let prerequisites = json!({
        "body_status": body_status,
        "direct_graph_roots": roots,
        "missing_direct_graph_roots": missing_roots,
        "target_tags_present": body.is_some_and(|body| body.target_tags.is_some()),
        "authored_mover_present": assets.movers.authored_for_body(hero.body).is_some(),
        "codec_table_present": assets.codecs.is_some(),
        "controller_on_fire_present": initial_on_fire(&scene.initial_board).is_some(),
        "spawn_error_kind": mover.as_ref().err().map(|error| format!("{:?}", error.kind())),
    });
    let (admission, bootstrap) = if let Ok(mover) = mover {
        let spawn = OwnerBodySpawn {
            slot: 0,
            controller: 0xa000_0100,
            entity: 0xa000_0101,
            body_filter: team_bit(0).unwrap(),
            frame: FIRST_TICK,
            pose: mover.pose(),
            quanta: RETAIL_QUANTA,
        };
        let admission = factory_result(scene.owner_body_runtime(hero, spawn), "not_admitted");
        // Probe the same factory for this hero without adding it to the scene.
        let bootstrap = factory_result(
            assets
                .prepare_owner_body(scene, hero.guid)
                .map_or(Ok(None), |factory| factory(hero, spawn)),
            "factory_unavailable",
        );
        (admission, bootstrap)
    } else {
        (
            json!({"status": "not_probed"}),
            json!({"status": "not_probed"}),
        )
    };
    json!({
        "hero": format!("0x{:016X}", hero.guid),
        "body": format!("0x{:016X}", hero.body),
        "prerequisites": prerequisites,
        "admission": admission,
        "bootstrap": bootstrap,
    })
}

fn measure_scene(runtime: &Path) -> Result<(Json, Vec<Json>), (&'static str, io::Error)> {
    let assets = SceneAssets::load(runtime).map_err(|error| ("asset_load", error))?;
    let scene = assets
        .prepare(GameMap::practice_range())
        .map_err(|error| ("scene_prepare", error))?;
    let mut heroes: Vec<_> = assets.heroes().heroes().collect();
    heroes.sort_unstable_by_key(|hero| hero.guid);
    let rows = heroes
        .into_iter()
        .map(|hero| actor_row(&assets, &scene, hero))
        .collect();
    Ok((
        json!({
            "status": "prepared",
            "movement_ready": scene.movement_ready(),
            "range_bots": scene.bots().unwrap().len(),
            "scripted_range_bots": scene.scripted_bots(),
            "owner_factory_present": scene.owner_body_factory.is_some(),
        }),
        rows,
    ))
}

#[test]
#[ignore = "requires a private prepared runtime and opt-in report destination"]
fn prepared_practice_actor_startup_matrix() {
    let Some(path) = env::var_os("ORPHEUS_ACTOR_STARTUP_REPORT").map(PathBuf::from) else {
        return;
    };
    assert!(!path.try_exists().unwrap(), "startup report must be new");
    let revision = env::var("ORPHEUS_ACTOR_STARTUP_REVISION")
        .expect("startup report requires its source revision");
    assert!(
        revision.len() == 40 && revision.bytes().all(|b| b.is_ascii_hexdigit()),
        "startup report source revision must be a full Git SHA-1"
    );
    let runtime = PathBuf::from(env::var_os("ORPHEUS_RUNTIME_DIR").expect("prepared runtime"));
    let inputs = scene_inputs(&runtime);
    let (scene, actors) = match measure_scene(&runtime) {
        Ok(measured) => measured,
        Err((stage, error)) => (
            json!({"status": "unavailable", "stage": stage, "error_kind": format!("{:?}", error.kind())}),
            Vec::new(),
        ),
    };
    assert_eq!(
        inputs,
        scene_inputs(&runtime),
        "startup report inputs changed"
    );
    let report = json!({
        "schema_version": 1,
        "scenario": "practice-owner-startup-v1",
        "source_revision": revision.to_ascii_lowercase(),
        "inputs": inputs,
        "scene": scene,
        "actors": actors,
    });
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut output = tempfile::NamedTempFile::new_in(parent).unwrap();
    serde_json::to_writer(output.as_file_mut(), &report).unwrap();
    writeln!(output).unwrap();
    output
        .persist_noclobber(path)
        .expect("startup report must be new");
}
