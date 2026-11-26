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

const COMPATIBILITY: &[Profile] = &[
    // Startup defaults from ow174/game/script/rulesets.py (generic across all playable heroes).
    Profile {
        mode: ModeScope::Exact(PRACTICE_RANGE_MODE),
        actors: &[ActorKind::Player],
        settings: &[
            (0x2538, Value::Float(1.0)),
            (0x260f, Value::Bool(true)),
            (0x2819, Value::Float(1.0)),
            (0x281a, Value::Bool(false)),
            (0x32e0, Value::Bool(false)),
            (0x32e8, Value::Float(1.0)),
            (0x32e9, Value::Bool(false)),
        ],
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
        settings: &[
            (0x2538, Value::Float(1.0)),
            (0x260f, Value::Bool(true)),
            (0x2819, Value::Float(1.0)),
            (0x281a, Value::Bool(false)),
            (0x32e0, Value::Bool(false)),
            (0x32e8, Value::Float(1.0)),
            (0x32e9, Value::Bool(false)),
        ],
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
