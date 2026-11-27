//! Authored rules and engine settings with captured compatibility defaults.

use crate::{
    game_content::{PRACTICE_RANGE_MODE, is_junkenstein_mode},
    script_expr::EngineSetting,
    script_graph::GraphStore,
    script_wire::Value,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActorKind {
    Player,
    Npc,
}

enum ModeScope {
    Exact(u64),
    Junkenstein,
    AnyNonZero,
}

impl ModeScope {
    fn contains(&self, mode: u64) -> bool {
        match *self {
            Self::Exact(expected) => mode == expected,
            Self::Junkenstein => is_junkenstein_mode(mode),
            Self::AnyNonZero => mode != 0 && !is_junkenstein_mode(mode),
        }
    }
}

struct Profile {
    mode: ModeScope,
    actors: &'static [ActorKind],
    settings: &'static [(u32, Value)],
}

// Standard playable-hero & mode startup defaults (used when game_graphs_174.json.gz
// omits authored `mode_rulesets`). Every entry has a single uniform default across
// playable heroes in Practice Range / standard PvP modes (e.g. 0x2531 / 9521 = 1.0
// for Doomfist Seismic Slam movement speed and generic movement scalars).
const STANDARD_PLAYER_DEFAULTS: &[(u32, Value)] = &[
    (0x228f, Value::Bool(false)),
    (0x2531, Value::Float(1.0)),
    (0x2532, Value::Float(1.0)),
    (0x2533, Value::Bool(true)),
    (0x2534, Value::Float(1.0)),
    (0x2535, Value::Bool(true)),
    (0x2536, Value::Float(1.0)),
    (0x2537, Value::Float(1.0)),
    (0x2538, Value::Float(1.0)),
    (0x2539, Value::Float(1.0)),
    (0x253a, Value::Bool(true)),
    (0x253b, Value::Float(1.0)),
    (0x253c, Value::Float(1.0)),
    (0x253d, Value::Float(1.0)),
    (0x253e, Value::Float(1.0)),
    (0x253f, Value::Bool(true)),
    (0x2540, Value::Float(1.0)),
    (0x2541, Value::Float(1.0)),
    (0x2542, Value::Bool(true)),
    (0x2543, Value::Bool(true)),
    (0x2544, Value::Bool(true)),
    (0x2545, Value::Bool(true)),
    (0x254a, Value::Bool(false)),
    (0x2564, Value::Float(1.0)),
    (0x2565, Value::Float(1.0)),
    (0x260f, Value::Bool(true)),
    (0x2610, Value::Float(1.0)),
    (0x262f, Value::Float(1.0)),
    (0x2631, Value::Float(1.0)),
    (0x2632, Value::Float(1.0)),
    (0x2637, Value::Bool(true)),
    (0x2638, Value::Bool(true)),
    (0x263a, Value::Bool(true)),
    (0x263b, Value::Bool(false)),
    (0x263c, Value::Float(1.0)),
    (0x263d, Value::Float(1.0)),
    (0x263e, Value::Float(1.0)),
    (0x263f, Value::Float(1.0)),
    (0x2819, Value::Float(1.0)),
    (0x281a, Value::Bool(false)),
    (0x290c, Value::Bool(true)),
    (0x292d, Value::Bool(false)),
    (0x292e, Value::Bool(false)),
    (0x296b, Value::Float(1.0)),
    (0x296c, Value::Float(1.0)),
    (0x296d, Value::Float(1.0)),
    (0x296e, Value::Float(1.0)),
    (0x2973, Value::Float(1.0)),
    (0x2976, Value::Float(1.0)),
    (0x2977, Value::Float(1.0)),
    (0x297a, Value::Float(1.0)),
    (0x297b, Value::Float(1.0)),
    (0x297c, Value::Float(1.0)),
    (0x297d, Value::Float(1.0)),
    (0x297e, Value::Float(1.0)),
    (0x2982, Value::Float(1.0)),
    (0x2986, Value::Float(1.0)),
    (0x298e, Value::Float(1.0)),
    (0x2990, Value::Float(1.0)),
    (0x2991, Value::Float(1.0)),
    (0x2992, Value::Float(1.0)),
    (0x2a7c, Value::Float(1.0)),
    (0x2a7d, Value::Float(1.0)),
    (0x2ac5, Value::Float(1.0)),
    (0x2acd, Value::Float(1.0)),
    (0x2ace, Value::Float(1.0)),
    (0x2acf, Value::Float(1.0)),
    (0x2fa2, Value::Float(1.0)),
    (0x2fa3, Value::Float(1.0)),
    (0x2fe6, Value::Float(1.0)),
    (0x2fea, Value::Float(0.3)),
    (0x2feb, Value::Float(0.5)),
    (0x2fec, Value::Float(1.0)),
    (0x300d, Value::Float(1.0)),
    (0x301f, Value::Bool(false)),
    (0x308c, Value::Bool(false)),
    (0x314a, Value::Float(1.0)),
    (0x314b, Value::Float(1.0)),
    (0x3276, Value::Float(1.0)),
    (0x32de, Value::Float(1.0)),
    (0x32e0, Value::Bool(false)),
    (0x32e8, Value::Float(1.0)),
    (0x32e9, Value::Bool(false)),
    (0x3724, Value::Bool(true)),
    (0x3725, Value::Float(1.0)),
    (0x3732, Value::Float(1.0)),
    (0x3733, Value::Float(1.0)),
    (0x373c, Value::Float(1.0)),
    (0x3a6e, Value::Bool(true)),
    (0x3a6f, Value::Float(1.0)),
    (0x3a70, Value::Float(1.0)),
    (0x3aa3, Value::Integer(5)),
    (0x3fd9, Value::Float(1.0)),
    (0x3fda, Value::Bool(true)),
    (0x3fdb, Value::Float(1.0)),
    (0x3fdd, Value::Float(1.0)),
    (0x3fdf, Value::Bool(false)),
    (0x4400, Value::Float(1.0)),
    (0x4401, Value::Float(1.0)),
    (0x4402, Value::Float(1.0)),
    (0x4501, Value::Float(1.0)),
    (0x5917, Value::Float(1.0)),
    (0x591a, Value::Float(1.0)),
];

const COMPATIBILITY: &[Profile] = &[
    // Startup defaults (generic across all playable heroes).
    Profile {
        mode: ModeScope::Exact(PRACTICE_RANGE_MODE),
        actors: &[ActorKind::Player],
        settings: STANDARD_PLAYER_DEFAULTS,
    },
    Profile {
        mode: ModeScope::Junkenstein,
        actors: &[ActorKind::Player, ActorKind::Npc],
        settings: &[(0x2819, Value::Float(1.0)), (0x290c, Value::Bool(true))],
    },
    // Generic PvP player startup ruleset defaults (Escort / Assault / Hybrid / Control / Arcade).
    Profile {
        mode: ModeScope::AnyNonZero,
        actors: &[ActorKind::Player],
        settings: STANDARD_PLAYER_DEFAULTS,
    },
];

pub(crate) fn get(
    graphs: &GraphStore,
    mode: u64,
    hero: u64,
    actor: ActorKind,
    id: u32,
) -> Option<Value> {
    graphs.mode_ruleset_value(mode, hero, id).or_else(|| {
        COMPATIBILITY
            .iter()
            .filter(|profile| profile.mode.contains(mode) && profile.actors.contains(&actor))
            .flat_map(|profile| profile.settings)
            .find_map(|(key, value)| (*key == id).then(|| value.clone()))
    })
}

pub(crate) fn engine_setting(graphs: &GraphStore, mode: u64, key: u32) -> EngineSetting {
    graphs.mode_engine_setting(mode, key).unwrap_or_else(|| {
        if is_junkenstein_mode(mode) {
            crate::pve_encounter::host::engine_setting(key)
        } else {
            EngineSetting::Unavailable
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{ActorKind::*, *};
    use crate::game_content::{JUNKENSTEIN_ENDLESS, JUNKENSTEIN_REVENGE};
    use crate::game_heroes::{CASSIDY, SOLDIER};
    use crate::pve_encounter::navigation::{REAPER, ROADHOG, SUMMONER, WITCH};

    #[test]
    fn authored_null_stops_native_compatibility_fallback() {
        let mode = PRACTICE_RANGE_MODE;
        let bundle = serde_json::json!({
            "version": 1, "graphs": [], "bodies": {}, "missing": [],
            "mode_rulesets": [{
                "mode": format!("0x{mode:016X}"),
                "rules": [
                    {"id": 0x2819, "value": 0.75},
                    {"id": 0x2819, "body": format!("0x{SOLDIER:016X}"), "value": null},
                ],
            }],
        });
        let graphs = GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap();
        assert_eq!(
            get(&graphs, mode, SOLDIER, Player, 0x2819),
            Some(Value::Null)
        );
        assert_eq!(
            get(&graphs, mode, SOLDIER, Player, 0x260f),
            Some(Value::Bool(true))
        );
        assert_eq!(
            get(&graphs, mode, CASSIDY, Player, 0x2819),
            Some(Value::Float(0.75))
        );
        assert_eq!(
            get(&graphs, mode, ROADHOG, Npc, 0x2819),
            Some(Value::Float(0.75))
        );
    }

    #[test]
    fn authored_engine_table_replaces_the_captured_table_for_its_mode() {
        let mode = JUNKENSTEIN_REVENGE;
        let bundle = serde_json::json!({
            "version": 1, "graphs": [], "bodies": {}, "missing": [],
            "mode_rulesets": [{
                "mode": format!("0x{mode:016X}"),
                "engine_settings": [{"id": 0xe1c1_e402u32, "value": null}],
            }],
        });
        let graphs = GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap();
        assert_eq!(
            engine_setting(&graphs, mode, 0xe1c1_e402),
            EngineSetting::Present(Value::Null)
        );
        assert_eq!(
            engine_setting(&graphs, mode, 0x6ce2_9bab),
            EngineSetting::Missing
        );
        assert_eq!(
            engine_setting(&graphs, JUNKENSTEIN_ENDLESS, 0xe1c1_e402),
            EngineSetting::Present(Value::Integer(128))
        );
    }

    #[test]
    fn compatibility_rules_preserve_mode_and_actor_scope() {
        let graphs =
            GraphStore::from_json(br#"{"version":1,"graphs":[],"bodies":{},"missing":[]}"#)
                .unwrap();
        let get = |mode, hero, actor, id| super::get(&graphs, mode, hero, actor, id);
        for (id, expected) in [
            (0x2531, Value::Float(1.0)),
            (0x2538, Value::Float(1.0)),
            (0x260f, Value::Bool(true)),
            (0x2819, Value::Float(1.0)),
            (0x281a, Value::Bool(false)),
            (0x32e0, Value::Bool(false)),
            (0x32e8, Value::Float(1.0)),
            (0x32e9, Value::Bool(false)),
        ] {
            assert_eq!(
                get(PRACTICE_RANGE_MODE, SOLDIER, Player, id),
                Some(expected.clone())
            );
            assert_eq!(
                get(PRACTICE_RANGE_MODE, CASSIDY, Player, id),
                Some(expected)
            );
            assert_eq!(get(PRACTICE_RANGE_MODE, SOLDIER, Npc, id), None);
        }
        for mode in [JUNKENSTEIN_REVENGE, JUNKENSTEIN_ENDLESS] {
            for (hero, actor) in [
                (CASSIDY, Player),
                (SOLDIER, Player),
                (REAPER, Npc),
                (ROADHOG, Npc),
                (WITCH, Npc),
                (SUMMONER, Npc),
            ] {
                assert_eq!(get(mode, hero, actor, 0x2819), Some(Value::Float(1.0)));
                assert_eq!(get(mode, hero, actor, 0x290c), Some(Value::Bool(true)));
                assert_eq!(get(mode, hero, actor, 0x290b), None);
            }
        }
        assert_eq!(get(0, ROADHOG, Npc, 0x2819), None);
    }
}
