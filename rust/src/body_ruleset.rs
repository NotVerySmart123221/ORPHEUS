//! Body rules resolved from the mode profile before compatibility defaults.

use crate::{
    game_content::{PRACTICE_RANGE_MODE, is_junkenstein_mode},
    game_heroes::{CASSIDY, SOLDIER},
    junkenstein_navigation::{REAPER, ROADHOG, SUMMONER, WITCH},
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
}

impl ModeScope {
    fn contains(&self, mode: u64) -> bool {
        match *self {
            Self::Exact(expected) => mode == expected,
            Self::Junkenstein => is_junkenstein_mode(mode),
        }
    }
}

struct Profile {
    mode: ModeScope,
    actors: &'static [(ActorKind, &'static [u64])],
    settings: &'static [(u32, Value)],
}

const COMPATIBILITY: &[Profile] = &[
    // Startup defaults from ow174/game/script/rulesets.py.
    Profile {
        mode: ModeScope::Exact(PRACTICE_RANGE_MODE),
        actors: &[(ActorKind::Player, &[SOLDIER])],
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
    // The Cassidy overrides are from 104319 schemas 001B/0026 and Python e1128509.
    Profile {
        mode: ModeScope::Junkenstein,
        actors: &[
            (ActorKind::Player, &[CASSIDY]),
            (ActorKind::Npc, &[REAPER, ROADHOG, WITCH, SUMMONER]),
        ],
        settings: &[(0x2819, Value::Float(1.0))],
    },
    Profile {
        mode: ModeScope::Junkenstein,
        actors: &[
            (ActorKind::Player, &[CASSIDY]),
            (ActorKind::Npc, &[ROADHOG, SUMMONER]),
        ],
        settings: &[(0x290c, Value::Bool(true))],
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
            .filter(|profile| {
                profile.mode.contains(mode)
                    && profile
                        .actors
                        .iter()
                        .any(|(kind, heroes)| *kind == actor && heroes.contains(&hero))
            })
            .flat_map(|profile| profile.settings)
            .find_map(|(key, value)| (*key == id).then(|| value.clone()))
    })
}

#[cfg(test)]
mod tests {
    use super::{ActorKind::*, *};
    use crate::game_content::{JUNKENSTEIN_ENDLESS, JUNKENSTEIN_REVENGE};

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
                Some(expected)
            );
            assert_eq!(get(PRACTICE_RANGE_MODE, SOLDIER, Npc, id), None);
            assert_eq!(get(PRACTICE_RANGE_MODE, CASSIDY, Player, id), None);
        }
        for mode in [JUNKENSTEIN_REVENGE, JUNKENSTEIN_ENDLESS] {
            for (hero, actor, scale, fan) in [
                (CASSIDY, Player, true, true),
                (SOLDIER, Player, false, false),
                (ROADHOG, Player, false, false),
                (CASSIDY, Npc, false, false),
                (REAPER, Npc, true, false),
                (ROADHOG, Npc, true, true),
                (WITCH, Npc, true, false),
                (SUMMONER, Npc, true, true),
            ] {
                assert_eq!(
                    get(mode, hero, actor, 0x2819),
                    scale.then_some(Value::Float(1.0))
                );
                assert_eq!(
                    get(mode, hero, actor, 0x290c),
                    fan.then_some(Value::Bool(true))
                );
                assert_eq!(get(mode, hero, actor, 0x290b), None);
            }
        }
        assert_eq!(get(0, ROADHOG, Npc, 0x2819), None);
    }
}
