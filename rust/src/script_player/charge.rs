//! Charge admission and damage credit for the supported owner profile.

use std::{io, sync::Arc};

use crate::{
    game_combat::AppliedDamage,
    game_content::is_junkenstein_mode,
    game_heroes::CASSIDY,
    game_replication::OwnerRoomActivity,
    script_expr::{Scope, Variable},
    script_init::{State, named_pool_records},
    script_wire::{Value, Variables},
};

use super::{
    lifecycle::{DEADEYE_GRAPH, PlayerBodyRuntime},
    unsupported,
    world::BodyWorld,
};

struct ChargeBinding {
    graph: u32,
    admission: u32,
    damage_message: u64,
    inhibited_by: (u32, u16),
}

impl BodyWorld {
    fn charge_binding(&self) -> Option<ChargeBinding> {
        // A declared authored charge root owns this body's meter. Otherwise the
        // existing profile uses the native charge admission and credit path.
        (!self.uses_authored_charge && self.hero == CASSIDY && is_junkenstein_mode(self.mode))
            .then_some(ChargeBinding {
                graph: 0x0b8b,
                admission: 2426,
                damage_message: 0x0240_0000_0000_0012,
                inhibited_by: (DEADEYE_GRAPH, 0),
            })
    }
}

impl PlayerBodyRuntime {
    pub(super) fn sync_charge_admission(&mut self, active: bool) -> io::Result<()> {
        let Some(binding) = self.world.charge_binding() else {
            return Ok(());
        };
        let enabled = Value::Bool(active && self.world.vitals.death().is_none());
        let variable = Variable {
            scope: Scope::Entity,
            id: binding.admission,
        };
        if self.component.resolved_variable(0, variable).as_ref() != Some(&enabled) {
            // The room admits charging. The graph owns the passive wait,
            // amount, multipliers, damage gates and cap.
            self.component
                .set_entity_variable(binding.admission, enabled, &mut self.world)
                .map_err(unsupported)?;
            self.component
                .run_now(&mut self.world)
                .map_err(unsupported)?;
        }
        Ok(())
    }

    pub(super) fn credit_applied_damage(&mut self, damage: AppliedDamage) -> io::Result<()> {
        let Some(binding) = self.world.charge_binding() else {
            return Ok(());
        };
        // Old detached callers bootstrap charge even without room activity.
        // A known room phase takes precedence for live damage credit.
        if !self
            .world
            .room_activity
            .is_none_or(OwnerRoomActivity::encounter_active)
            || damage.source != self.world.entity
            || self.world.vitals.death().is_some()
            || !damage.amount.is_finite()
            || damage.amount <= 0.0
        {
            return Ok(());
        }
        let snapshot = self.component.snapshot();
        let (inhibit_graph, inhibit_state) = binding.inhibited_by;
        if snapshot.is_some_and(|snapshot| {
            snapshot.instances.values().any(|instance| {
                instance.graph == inhibit_graph
                    && instance
                        .states
                        .get(&inhibit_state)
                        .is_some_and(State::ability_running)
            })
        }) {
            return Ok(());
        }
        let instance = snapshot
            .and_then(|snapshot| {
                snapshot
                    .instances
                    .iter()
                    .find(|(_, instance)| instance.graph == binding.graph)
                    .map(|(&id, _)| id)
            })
            .ok_or_else(|| {
                unsupported("damage credit requires the authored common charge graph")
            })?;
        let parameters = Variables::from([
            (0x13, Value::Float(f64::from(damage.amount as f32))),
            (0x15, Value::Entity(damage.target_facts.target)),
            (
                0x3d,
                Value::Array(
                    damage
                        .tags
                        .as_slice()
                        .iter()
                        .copied()
                        .map(Value::Asset)
                        .collect(),
                ),
            ),
            (0x624, named_pool_records(&damage.named_pool_losses)),
        ]);
        self.component
            .queue_settled_damage_message(
                instance,
                binding.damage_message,
                parameters,
                Arc::new(damage.target_facts),
            )
            .map_err(unsupported)?;
        self.component.run_now(&mut self.world).map_err(unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vitals::{Death, Vitals};

    #[test]
    fn settled_damage_host_filter_uses_the_sampled_target_and_current_owner() {
        use crate::{
            game_content::PRACTICE_RANGE_MODE,
            script_expr::{FilterPredicate, FilterRelation, SceneFilterPolicy},
            script_init::{HealthTargetFacts, Site, World},
            script_player::fixture_body_factory,
        };
        use serde_json::json;

        let (factory, spawn) = fixture_body_factory();
        let mut runtime = factory.start(spawn).unwrap();
        let world = &mut runtime.world;
        world.mode = PRACTICE_RANGE_MODE;
        world.body_filter = 1 << 23;
        let site = Site {
            entity: spawn.entity,
            instance: 2,
            graph: 61_042,
            node: Some(5),
        };
        let mut facts = HealthTargetFacts {
            target: spawn.entity + 1,
            filter_flags: None,
            room_status: None,
            tags: Default::default(),
            script_state: crate::script_entity_state::EntityScriptState::Unavailable,
        };
        let filter = json!({"m_66747E7D":2, "m_89831602":0});
        let policy = world
            .settled_scene_filter_policy(site, &filter, &facts)
            .unwrap();
        assert!(matches!(
            policy,
            SceneFilterPolicy {
                relation: FilterRelation::Hostile,
                predicate: FilterPredicate::None,
            }
        ));
        assert!(world.scene_filter_policy(site, &filter).is_none());
        assert!(
            world
                .settled_scene_filter_policy(site, &json!({"m_66747E7D":3}), &facts)
                .is_none()
        );

        for (flags, expected) in [
            (Some(1 << 24), Some(true)),
            (Some(1 << 23), Some(false)),
            (Some(1 << 24 | 1 << 28), Some(false)),
            (Some(1 << 25), None),
            (None, None),
        ] {
            facts.filter_flags = flags;
            assert_eq!(
                world.settled_entities_hostile(site, spawn.entity, &facts, false),
                expected
            );
        }
        facts.filter_flags = Some(1 << 24);
        assert_eq!(
            world.settled_entities_hostile(site, spawn.controller, &facts, false),
            None
        );
        assert_eq!(
            world.settled_entities_hostile(site, spawn.entity, &facts, true),
            None
        );
    }

    #[test]
    fn damage_credit_projects_only_drained_named_pools_in_order() {
        const FIRST: u64 = 0x0d80_1200_0000_1001;
        const SECOND: u64 = 0x0d80_3400_0000_1002;
        let source = Death {
            time: 0.0,
            frame: 1,
            killer: 7,
        };
        let mut vitals = Vitals::new(25.0, 0.0, 0.0);
        assert_eq!(
            named_pool_records(&vitals.damage(1.0, source).named_pool_losses),
            Value::Array(Vec::new())
        );
        assert!(vitals.add_temporary_shield(FIRST, 3.25, true).is_some());
        assert!(vitals.add_temporary_shield(SECOND, 4.0, true).is_some());
        let damage = vitals.damage(5.0, source);
        assert_eq!(damage.dealt, 5.0);
        let record = |identifier, amount| {
            Value::Map(vec![
                (Value::Integer(16056), Value::Asset(identifier)),
                (Value::Integer(499), Value::Float(amount)),
            ])
        };
        assert_eq!(
            named_pool_records(&damage.named_pool_losses),
            Value::Array(vec![record(FIRST, 3.25), record(SECOND, 1.75)])
        );
    }
}
