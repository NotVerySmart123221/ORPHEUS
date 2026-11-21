use super::{
    ADMITTED, ChargeHost, INHIBITED, MAX_INIT_WORK, METER, METER_CAP, OWNER, PRACTICE_MODE, ROOT,
    meter,
};
use orpheus_statescript::{
    script_expr::{FilterPredicate, FilterRelation, SceneFilterPolicy},
    script_graph::GraphStore,
    script_init::{Component, Issue, RoomMessage, Root, Site, World},
    script_wire::{Value, Variables},
};
use serde_json::Value as Json;
use std::{path::PathBuf, sync::Arc};

const BODY: u64 = 0x0400_0000_0000_00e4;
const WEAPON: u32 = 0x006b;
const WEAPON_ROOT: u16 = 2;
const ADMISSION: u64 = 0x0240_0000_0000_0d9a;
const TARGET: u32 = 703;
const OWNER_TEAM: u64 = 1 << 23;
const TARGET_TEAM: u64 = 1 << 24;
const TARGET_MASK: u64 = 1 | 64;
const EXCLUDED_TAG: u64 = 0x0d60_0000_0000_003a;
const TARGET_MEMBER: u32 = 6442;

struct MercyHost<'a> {
    common: ChargeHost<'a>,
    target_flags: u64,
    excluded_tag: bool,
    excluded_member: bool,
}

impl World for MercyHost<'_> {
    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, entity: u32, id: u32) -> Option<Value> {
        if entity == TARGET && id == TARGET_MEMBER {
            return Some(Value::Bool(self.excluded_member));
        }
        self.common.entity_variable(entity, id)
    }

    fn entity_filter_flags(&self, site: Site, entity: u32) -> Option<u64> {
        if site.entity != OWNER {
            return None;
        }
        match entity {
            OWNER => Some(OWNER_TEAM),
            TARGET => Some(self.target_flags),
            _ => None,
        }
    }

    fn has_target_tag(&self, site: Site, entity: u32, tag: u64) -> Option<bool> {
        (site.entity == OWNER && entity == TARGET)
            .then_some(tag == EXCLUDED_TAG && self.excluded_tag)
    }

    fn scene_filter_policy(&self, site: Site, filter: &Json) -> Option<SceneFilterPolicy> {
        // The player host does not yet supply a policy for this weapon predicate.
        // Supply it only here to exercise the authored branch with typed facts.
        (site.entity == OWNER
            && site.graph == WEAPON
            && site.node == Some(176)
            && filter["m_66747E7D"] == 2
            && filter["m_89831602"] == 0
            && filter["m_84065FD1"] == 64
            && !filter["m_condition"].is_null())
        .then_some(SceneFilterPolicy {
            relation: FilterRelation::Hostile,
            predicate: FilterPredicate::Scalar,
        })
    }

    fn native(&mut self, site: Site, config: &Json) -> Option<Value> {
        self.common.native(site, config)
    }

    fn queue_room_message(&mut self, site: Site, target: u32, message: RoomMessage) -> bool {
        self.common.queue_room_message(site, target, message)
    }

    fn player_stats_service_available(&self, site: Site) -> Option<bool> {
        self.common.player_stats_service_available(site)
    }
}

#[test]
#[ignore = "requires an authorized 104319 graph pack in ORPHEUS_RUNTIME_DIR"]
fn prepared_mercy_admission_delivers_local_bonus_with_explicit_policy() {
    const CAP: f64 = 11.0;
    let runtime =
        PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").expect("set ORPHEUS_RUNTIME_DIR"));
    let graphs = Arc::new(GraphStore::load(runtime.join("data/game_graphs_174.json.gz")).unwrap());
    assert_eq!(graphs.bodies()[&BODY].weapons[1], Some(WEAPON));
    let mut world = MercyHost {
        common: ChargeHost {
            mode_variables: graphs.mode_variables(PRACTICE_MODE).unwrap(),
            outgoing: Vec::new(),
            target: None,
        },
        target_flags: TARGET_TEAM | TARGET_MASK,
        excluded_tag: false,
        excluded_member: false,
    };
    let mut body = Component::start(
        Arc::clone(&graphs),
        OWNER,
        Variables::from([
            (METER_CAP, Value::Float(CAP)),
            (METER, Value::Float(0.0)),
            (ADMITTED, Value::Bool(true)),
            (INHIBITED, Value::Bool(false)),
        ]),
        vec![
            Root::listener(ROOT, super::GRAPH, None),
            Root::listener(WEAPON_ROOT, WEAPON, None),
        ],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    assert!(body.snapshot().unwrap().diagnostics.is_empty());
    let deliver = |body: &mut Component, world: &mut MercyHost<'_>, amount, qualification| {
        // Inject the receiver's slots directly. This does not model the native
        // producer, identify what it measures, or infer a trigger from healing.
        let parameters = Variables::from([
            (0x74d, Value::Float(amount)),
            (0x13, Value::Float(0.0)),
            (0x5c, Value::Entity(TARGET)),
            (0x74c, Value::Float(qualification)),
        ]);
        assert_eq!(
            body.queue_message(WEAPON_ROOT, ADMISSION, parameters)
                .unwrap(),
            1
        );
        body.run_now(world).unwrap();
        assert!(world.common.outgoing.is_empty(), "00EB stays on the owner");
    };

    for (amount, qualification) in [(0.0, 1.0), (-2.5, 1.0), (2.5, 0.0)] {
        deliver(&mut body, &mut world, amount, qualification);
        assert_eq!(meter(&body), 0.0, "both amounts must be positive");
    }

    world.target_flags = TARGET_TEAM | 64;
    deliver(&mut body, &mut world, 2.5, 1.0);
    assert_eq!(meter(&body), 0.0, "missing first filter mask");
    world.target_flags = OWNER_TEAM | TARGET_MASK;
    deliver(&mut body, &mut world, 2.5, 1.0);
    assert_eq!(meter(&body), 0.0, "supplied policy rejects same team");
    world.target_flags = TARGET_TEAM | TARGET_MASK;

    world.excluded_tag = true;
    deliver(&mut body, &mut world, 2.5, 1.0);
    assert_eq!(meter(&body), 0.0, "excluded tag");
    world.excluded_tag = false;
    world.excluded_member = true;
    deliver(&mut body, &mut world, 2.5, 1.0);
    assert_eq!(meter(&body), 0.0, "excluded target member");
    world.excluded_member = false;

    deliver(&mut body, &mut world, 2.5, 6.0);
    assert_eq!(meter(&body), 2.5, "00EB uses slot 074D");
    deliver(&mut body, &mut world, 10.0, 1.0);
    assert_eq!(meter(&body), CAP, "the original meter receiver caps credit");

    // The same entry also reaches the still-unsupported remote 0042 cue.
    // That sibling action is independent of the local 00EB branch.
    let diagnostics = &body.snapshot().unwrap().diagnostics;
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic.site.graph, WEAPON);
    assert_eq!(diagnostic.site.node, Some(175));
    assert_eq!(
        diagnostic.issue,
        Issue::UnsupportedAction("STU_100EF8C2".into())
    );
}
