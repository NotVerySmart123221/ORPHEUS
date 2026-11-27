use orpheus_wire::commands::Command;

use super::*;
use crate::script_expr::SpatialRayHit;

struct SemanticProbeWorld {
    combat_mods: Vec<(Site, Option<CombatModEntry>)>,
    velocity_ops: Vec<VelocityOp>,
    teleports: Vec<[f64; 3]>,
    explosions: Vec<([f64; 3], f64, f64)>,
    routed_messages: Vec<(u64, Variables)>,
    ray_hit: Option<SpatialRayHit>,
    tracked: Vec<u32>,
}

impl Default for SemanticProbeWorld {
    fn default() -> Self {
        Self {
            combat_mods: Vec::new(),
            velocity_ops: Vec::new(),
            teleports: Vec::new(),
            explosions: Vec::new(),
            routed_messages: Vec::new(),
            ray_hit: Some(SpatialRayHit {
                distance: 4.0,
                point: [0.0, 0.0, 4.0],
                normal: [0.0, 1.0, 0.0],
            }),
            tracked: vec![99],
        }
    }
}

impl World for SemanticProbeWorld {
    fn admits_generic_player_runtime(&self, _site: Site) -> bool {
        true
    }

    fn position(&mut self, entity: u32) -> Option<[f64; 3]> {
        match entity {
            10 => Some([0.0, 0.0, 0.0]),
            99 => Some([2.0, 0.0, 0.0]),
            _ => None,
        }
    }

    fn forward(&self, _entity: u32) -> Option<[f64; 3]> {
        Some([0.0, 0.0, 1.0])
    }

    fn authored_forward(&self, _entity: u32) -> Option<[f64; 3]> {
        Some([0.0, 0.0, 1.0])
    }

    fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
        None
    }

    fn set_combat_mod(&mut self, site: Site, entry: Option<CombatModEntry>) -> bool {
        self.combat_mods.push((site, entry));
        true
    }

    fn apply_velocity_ops(&mut self, _site: Site, _frame: u32, ops: &[VelocityOp]) -> bool {
        self.velocity_ops.extend_from_slice(ops);
        true
    }

    fn queue_scripted_motion(
        &mut self,
        _site: Site,
        _frame: u32,
        motion: ScriptedMotion,
    ) -> Option<bool> {
        if let ScriptedMotion::Teleport(reloc) = motion {
            if let Some(dest) = reloc.teleport_destination() {
                self.teleports.push(dest);
            }
        }
        Some(true)
    }

    fn apply_physics_explosion(
        &mut self,
        _site: Site,
        center: [f64; 3],
        radius: f64,
        impulse: f64,
        _direction: Option<[f64; 3]>,
    ) -> bool {
        self.explosions.push((center, radius, impulse));
        true
    }

    fn spatial_ray_cast(
        &self,
        _site: Site,
        _frame: u32,
        _origin: [f64; 3],
        _direction: [f64; 3],
        _distance: f64,
    ) -> Option<Option<SpatialRayHit>> {
        Some(self.ray_hit)
    }

    fn tracked_targets(
        &mut self,
        _site: Site,
        _origin_entity: u32,
        _offset: [f64; 3],
        _radius: f64,
        _filter: &Json,
    ) -> Option<Vec<u32>> {
        Some(self.tracked.clone())
    }

    fn route_game_message(&mut self, _site: Site, message: u64, parameters: &Variables) -> bool {
        self.routed_messages.push((message, parameters.clone()));
        true
    }
}

#[test]
fn promoted_retail_states_execute_real_semantics_and_cleanup() {
    let g = graph(
        0x9100,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({
                    "m_outPlug": plug(&[(1, "in"), (2, "in"), (3, "in"), (4, "in"), (5, "in"), (6, "in")], &[])
                }),
            ),
            // 1: CC-immunity status (STU_C5D842D8)
            node(1, "STU_C5D842D8", Some(0), json!({})),
            // 2: 3D Flight (STU_C1BA7545)
            node(
                2,
                "STU_C1BA7545",
                Some(1),
                json!({"m_7E678A84": {"$": "STUConfigVarFloat", "m_value": 8.0}}),
            ),
            // 3: PlacementCast (STU_43DB06B1) writes hit point to var 100
            node(
                3,
                "STU_43DB06B1",
                Some(2),
                json!({
                    "m_B747F285": var(100, false),
                    "m_1EB0DDDA": var(101, false)
                }),
            ),
            // 4: ProximityTrigger (STU_FAC489FF) detects enemy 99 -> var 102
            node(
                4,
                "STU_FAC489FF",
                Some(3),
                json!({
                    "m_radius": {"$": "STUConfigVarFloat", "m_value": 5.0},
                    "m_CB6ABF9A": var(102, false)
                }),
            ),
            // 5: SyncVars (STU_BF916E6E) writes Color config var into var 103
            node(
                5,
                "STU_BF916E6E",
                Some(4),
                json!({
                    "m_value": {"$": "STUConfigVarColor", "m_value": {"R": 1.0, "G": 0.5, "B": 0.25}},
                    "m_out_Var": var(103, false)
                }),
            ),
            // 6: TeleportEffect (STU_746F8777) queues unbounded Teleport
            node(
                6,
                "STU_746F8777",
                Some(5),
                json!({
                    "m_AF774F7D": {"$": "STUConfigVarVec3", "m_value": {"X": 80.0, "Y": 5.0, "Z": -40.0}}
                }),
            ),
        ],
        json!([1, 2, 3, 4, 5, 6]),
        json!([0]),
    );
    let graphs = Arc::new(store(vec![g]));
    let mut world = SemanticProbeWorld::default();
    let mut comp = Component::start(
        graphs,
        10,
        Variables::new(),
        vec![Root::new(1, 0x9100)],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    let snap = comp.snapshot().unwrap();
    assert!(snap.diagnostics.is_empty(), "{:?}", snap.diagnostics);

    // CC-immunity emitted
    assert!(
        world
            .combat_mods
            .iter()
            .any(|(_, entry)| entry.is_some_and(|e| e.cc_immune))
    );
    // Teleport queued
    assert_eq!(world.teleports, vec![[80.0, 5.0, -40.0]]);
    // PlacementCast wrote hit point & valid bool
    let inst = &snap.instances[&1];
    assert_eq!(inst.variables.get(&101), Some(&Value::Bool(true)));
    assert_eq!(
        inst.variables.get(&100),
        Some(&Value::Vector([0.0, 0.0, 4.0]))
    );
    // SyncVars evaluated STUConfigVarColor
    assert_eq!(
        inst.variables.get(&103),
        Some(&Value::Vector([1.0, 0.5, 0.25]))
    );

    // Step 1 tick: ProximityTrigger fires & 3D Flight applies vertical control
    comp.advance(
        16,
        Some(Command {
            frame: 1,
            buttons: 0x0040,
            ..Command::default()
        }),
        false,
        &mut world,
    )
    .unwrap();
    let snap2 = comp.snapshot().unwrap();
    assert_eq!(
        snap2.instances[&1].variables.get(&102),
        Some(&Value::Entity(99))
    );
    assert!(world.velocity_ops.iter().any(|op| matches!(
        op,
        VelocityOp::SetSpeed {
            axis: VelocityAxis::Vertical,
            value
        } if *value > 0.0
    )));
}

#[test]
fn retail_catalog_actions_execute_and_synthetic_unknowns_stay_rejected() {
    let g = graph(
        0x9101,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug": plug(&[(1, "in")], &[])}),
            ),
            // RemoteSetVar (STU_0BDBC7B2) sets var 200 to STUConfigVarUInt(42)
            node(
                1,
                "STU_0BDBC7B2",
                None,
                json!({
                    "m_entity": {"$": "STUConfigVarEntityReference", "m_entity": var(201, true)},
                    "m_value": {"$": "STUConfigVarUInt", "m_value": 42},
                    "m_out_Var": var(200, false),
                    "m_outPlug": plug(&[(2, "in")], &[])
                }),
            ),
            // SendGameMessage (STU_1470EF32) routes authored message
            node(
                2,
                "STU_1470EF32",
                None,
                json!({
                    "m_gameMessage": {"$": "STUConfigVarGameMessage", "m_gameMessage": "0x0240000000000099"},
                    "m_39C7E0B7": []
                }),
            ),
        ],
        json!([]),
        json!([0]),
    );
    let graphs = Arc::new(store(vec![g]));
    let mut world = SemanticProbeWorld::default();
    let comp = Component::start(
        graphs,
        10,
        Variables::from([(201, Value::Entity(10))]),
        vec![Root::new(1, 0x9101)],
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    let snap = comp.snapshot().unwrap();
    assert!(snap.diagnostics.is_empty(), "{:?}", snap.diagnostics);
    assert_eq!(
        snap.instances[&1].variables.get(&200),
        Some(&Value::Integer(42))
    );
    assert_eq!(world.routed_messages.len(), 1);
    assert_eq!(world.routed_messages[0].0, 0x0240_0000_0000_0099);

    // Unknown synthetic state class MUST still produce UnsupportedState diagnostic!
    let unknown_g = graph(
        0x9102,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug": plug(&[(1, "in")], &[])}),
            ),
            node(1, "STU_SYNTHETIC_NOT_IN_RETAIL", Some(0), json!({})),
        ],
        json!([1]),
        json!([0]),
    );
    let unknown_store = Arc::new(store(vec![unknown_g]));
    let mut world2 = SemanticProbeWorld::default();
    let comp2 = Component::start(
        unknown_store,
        10,
        Variables::new(),
        vec![Root::new(1, 0x9102)],
        0,
        &mut world2,
        MAX_INIT_WORK,
    )
    .unwrap();
    let snap2 = comp2.snapshot().unwrap();
    assert!(
        snap2
            .diagnostics
            .iter()
            .any(|d| matches!(d.issue, Issue::UnsupportedState(_))),
        "expected UnsupportedState for unknown synthetic class, got {:?}",
        snap2.diagnostics
    );
}
