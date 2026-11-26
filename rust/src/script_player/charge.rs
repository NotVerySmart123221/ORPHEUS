//! Charge admission and damage credit for the supported owner profile.

use std::{io, sync::Arc};

use crate::{
    game_combat::AppliedDamage,
    game_content::is_junkenstein_mode,
    game_replication::OwnerRoomActivity,
    script_expr::{Scope, Variable},
    script_graph::Body,
    script_init::{Root, Snapshot, State, StateKind, named_pool_records},
    script_wire::{Value, Variables},
};

use super::{OwnerBodyPreparationError, lifecycle::PlayerBodyRuntime, unsupported};

const COMMON_CHARGE_PARENT_GRAPH: u32 = 0x004b;
const COMMON_CHARGE_GRAPH: u32 = 0x0b8b;
const COMMON_CHARGE_ADMISSION_VAR: u32 = 2426;
const COMMON_ULT_CHARGE_VAR: u32 = 342;
const COMMON_ULT_CAP_VAR: u32 = 338;
const COMMON_DAMAGE_CHARGE_MESSAGE: u64 = 0x0240_0000_0000_0012;
const COMMON_HEAL_CHARGE_MESSAGE: u64 = 0x0240_0000_0000_0044;
const COMMON_DIRECT_CHARGE_MESSAGE: u64 = 0x0240_0000_0000_0148;
#[allow(dead_code)]
const COMMON_PASSIVE_CHARGE_MESSAGE: u64 = 0x0240_0000_0000_00eb;

#[derive(Clone, Copy)]
pub(super) struct ChargeBinding {
    recipient: ChargeRecipient,
    admission: u32,
    damage_message: u64,
    heal_message: Option<u64>,
    inhibited_by: Option<(u32, u16)>,
    require_encounter: bool,
}

#[derive(Clone, Copy)]
enum ChargeRecipient {
    CommonGraph(u32),
    MountedRoot {
        graph: u32,
        instance: u16,
    },
    SubScript {
        graph: u32,
        parent_graph: u32,
        parent_instance: u16,
        parent_state: u16,
    },
}

pub(super) enum ChargeRoute {
    Disabled,
    Configured(ChargeBinding),
}

impl ChargeRoute {
    pub(super) fn prepare(
        body: &Body,
        mode: u64,
        roots: &[Root],
    ) -> Result<Self, OwnerBodyPreparationError> {
        let Some(profile) = &body.charge_compatibility else {
            if body.authored_charge_graph.is_some() {
                return Ok(Self::Disabled);
            }
            // Data-driven common retail ult-charge binding: any hero whose roots mount
            // the retail 1.74 common charge graph (`0x004B` -> `0x0B8B`, or `0x0B8B` directly)
            // gets passive, damage (`0x0012`), and healing (`0x0044`) charge admission.
            // `0x0B8B` itself inhibits gain while an ultimate is active via `shared_var[842]`.
            let mounts_common_charge = roots
                .iter()
                .any(|root| matches!(root.graph, COMMON_CHARGE_PARENT_GRAPH | COMMON_CHARGE_GRAPH));
            return Ok(if mounts_common_charge {
                Self::Configured(ChargeBinding {
                    recipient: ChargeRecipient::CommonGraph(COMMON_CHARGE_GRAPH),
                    admission: COMMON_CHARGE_ADMISSION_VAR,
                    damage_message: COMMON_DAMAGE_CHARGE_MESSAGE,
                    heal_message: Some(COMMON_HEAL_CHARGE_MESSAGE),
                    inhibited_by: None,
                    require_encounter: is_junkenstein_mode(mode),
                })
            } else {
                Self::Disabled
            });
        };
        if !profile.modes.contains(&mode) {
            return Ok(Self::Disabled);
        }
        let graph = profile.receiver_graph;
        let mount_graph = profile.parent.map_or(graph, |parent| parent.graph);
        let mut matching = roots.iter().filter(|root| root.graph == mount_graph);
        let root = matching
            .next()
            .filter(|root| !root.is_server_only() && matching.next().is_none())
            .ok_or(OwnerBodyPreparationError::CompatibilityChargeRoot(
                mount_graph,
            ))?;
        let recipient = match profile.parent {
            Some(parent) => ChargeRecipient::SubScript {
                graph,
                parent_graph: parent.graph,
                parent_instance: root.id,
                parent_state: parent.state,
            },
            None => ChargeRecipient::MountedRoot {
                graph,
                instance: root.id,
            },
        };
        Ok(Self::Configured(ChargeBinding {
            recipient,
            admission: profile.admission_variable,
            damage_message: profile.damage_message,
            heal_message: (graph == COMMON_CHARGE_GRAPH).then_some(COMMON_HEAL_CHARGE_MESSAGE),
            inhibited_by: profile
                .inhibit
                .as_ref()
                .map(|rule| (rule.graph, rule.state)),
            require_encounter: false,
        }))
    }

    pub(super) fn bind(&self, _mode: u64, _hero: u64) -> Option<ChargeBinding> {
        match self {
            Self::Disabled => None,
            Self::Configured(binding) => Some(*binding),
        }
    }
}

impl ChargeBinding {
    pub(super) fn admits_regular_room(self) -> bool {
        !self.require_encounter
    }

    fn activity_allows(self, activity: Option<OwnerRoomActivity>) -> bool {
        if self.require_encounter {
            activity.is_none_or(OwnerRoomActivity::encounter_active)
        } else {
            activity.is_none_or(OwnerRoomActivity::allows_body_activity)
        }
    }

    fn receiving_instance(self, snapshot: &Snapshot) -> Option<u16> {
        match self.recipient {
            ChargeRecipient::CommonGraph(graph) => snapshot
                .instances
                .iter()
                .find_map(|(&id, instance)| (instance.graph == graph).then_some(id)),
            ChargeRecipient::MountedRoot { graph, instance } => snapshot
                .instances
                .get(&instance)
                .filter(|root| root.graph == graph && root.parent.is_none())
                .map(|_| instance),
            ChargeRecipient::SubScript {
                graph,
                parent_graph,
                parent_instance,
                parent_state,
            } => {
                let root = snapshot.instances.get(&parent_instance)?;
                if root.graph != parent_graph || root.parent.is_some() {
                    return None;
                }
                let state = root.states.get(&parent_state)?;
                let StateKind::SubScript { child: Some(child) } = state.kind else {
                    return None;
                };
                let receiver = snapshot.instances.get(&child)?;
                (state.active
                    && receiver.graph == graph
                    && receiver.parent == Some((parent_instance, parent_state)))
                .then_some(child)
            }
        }
    }
}

impl PlayerBodyRuntime {
    pub(super) fn sync_charge_admission(&mut self, active: bool) -> io::Result<()> {
        let Some(binding) = self.charge_binding else {
            return Ok(());
        };
        let active = active
            && (!binding.admits_regular_room()
                || binding.activity_allows(self.world.room_activity));
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
        let Some(binding) = self.charge_binding else {
            return Ok(());
        };
        // Old detached callers bootstrap charge even without room activity.
        // A known room phase takes precedence for live damage credit.
        if !binding.activity_allows(self.world.room_activity)
            || damage.source != self.world.entity
            || self.world.vitals.death().is_some()
            || !damage.amount.is_finite()
            || damage.amount <= 0.0
        {
            return Ok(());
        }
        let snapshot = self.component.snapshot();
        if binding.inhibited_by.is_some_and(|(graph, state)| {
            snapshot.is_some_and(|snapshot| {
                snapshot.instances.values().any(|instance| {
                    instance.graph == graph
                        && instance
                            .states
                            .get(&state)
                            .is_some_and(State::ability_running)
                })
            })
        }) {
            return Ok(());
        }
        let Some(instance) = snapshot.and_then(|snapshot| binding.receiving_instance(snapshot))
        else {
            if matches!(
                binding.recipient,
                ChargeRecipient::SubScript { .. } | ChargeRecipient::CommonGraph(_)
            ) {
                return Ok(());
            }
            return Err(unsupported(
                "damage credit requires the authored common charge graph",
            ));
        };
        let scaled_amount = damage.amount * self.ult_charge_scale;
        if !scaled_amount.is_finite() || scaled_amount <= 0.0 {
            return Ok(());
        }
        let parameters = Variables::from([
            (0x13, Value::Float(f64::from(scaled_amount as f32))),
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

    pub(super) fn credit_applied_heal(
        &mut self,
        source: u32,
        target: u32,
        amount: f64,
    ) -> io::Result<()> {
        let Some(binding) = self.charge_binding else {
            return Ok(());
        };
        let Some(heal_message) = binding.heal_message else {
            return Ok(());
        };
        if !binding.activity_allows(self.world.room_activity)
            || source != self.world.entity
            || self.world.vitals.death().is_some()
            || !amount.is_finite()
            || amount <= 0.0
        {
            return Ok(());
        }
        // Retail heal event (`GameMessage 0x0240000000000044`):
        //   param 0x3F = target entity, 0x40 = heal amount, 0x41 = tags
        // Deliver to ALL matching instances on the healer so `0x0B8B` (ult charge),
        // `0x01D8` (on-fire), and hero-specific healing graphs (`0x0B94`, `0x1932`,
        // `0x0F76`, `0x006B`, `0x0263`, `0x02F3`, etc.) all receive the heal event.
        let scaled_amount = amount * self.ult_charge_scale;
        if !scaled_amount.is_finite() || scaled_amount <= 0.0 {
            return Ok(());
        }
        let parameters = Variables::from([
            (0x3f, Value::Entity(target)),
            (0x40, Value::Float(f64::from(scaled_amount as f32))),
            (0x41, Value::Array(Vec::new())),
        ]);
        let queued = self
            .component
            .queue_entity_message(heal_message, parameters)
            .map_err(unsupported)?;
        if queued > 0 {
            self.component
                .run_now(&mut self.world)
                .map_err(unsupported)?;
        }
        Ok(())
    }

    pub(crate) fn ult_charge_percent(&self) -> f64 {
        let cap_var = Variable {
            scope: Scope::Entity,
            id: COMMON_ULT_CAP_VAR,
        };
        let cur_var = Variable {
            scope: Scope::Entity,
            id: COMMON_ULT_CHARGE_VAR,
        };
        let cap = self
            .component
            .resolved_variable(0, cap_var)
            .map(|v| crate::script_expr::to_float(&v))
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(1500.0);
        let cur = self
            .component
            .resolved_variable(0, cur_var)
            .map(|v| crate::script_expr::to_float(&v))
            .filter(|v| v.is_finite())
            .unwrap_or(0.0);
        ((cur / cap) * 100.0).clamp(0.0, 100.0)
    }

    /// Credit direct ultimate charge via retail `GameMessage 0x0240000000000148`
    /// or direct entity variable update (`#342` capped by `#338`).
    pub(crate) fn add_ult_charge_points(&mut self, points: f64) -> io::Result<()> {
        if !points.is_finite() || points == 0.0 {
            return Ok(());
        }
        let cap_var = Variable {
            scope: Scope::Entity,
            id: COMMON_ULT_CAP_VAR,
        };
        let cur_var = Variable {
            scope: Scope::Entity,
            id: COMMON_ULT_CHARGE_VAR,
        };
        let cap = self
            .component
            .resolved_variable(0, cap_var)
            .map(|v| crate::script_expr::to_float(&v))
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(1500.0);
        let cur = self
            .component
            .resolved_variable(0, cur_var)
            .map(|v| crate::script_expr::to_float(&v))
            .filter(|v| v.is_finite())
            .unwrap_or(0.0);
        let next = (cur + points).clamp(0.0, cap);
        self.component
            .set_entity_variable(COMMON_ULT_CHARGE_VAR, Value::Float(next), &mut self.world)
            .map_err(unsupported)?;
        let _ = self
            .component
            .queue_entity_message(COMMON_DIRECT_CHARGE_MESSAGE, Variables::new());
        self.component.run_now(&mut self.world).map_err(unsupported)
    }

    pub(crate) fn set_ult_charge_percent(&mut self, percent: f64) -> io::Result<()> {
        if !percent.is_finite() {
            return Ok(());
        }
        let frac = (percent / 100.0).clamp(0.0, 1.0);
        let cap_var = Variable {
            scope: Scope::Entity,
            id: COMMON_ULT_CAP_VAR,
        };
        let cap = self
            .component
            .resolved_variable(0, cap_var)
            .map(|v| crate::script_expr::to_float(&v))
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(1500.0);
        self.component
            .set_entity_variable(
                COMMON_ULT_CHARGE_VAR,
                Value::Float(cap * frac),
                &mut self.world,
            )
            .map_err(unsupported)?;
        let _ = self
            .component
            .queue_entity_message(COMMON_DIRECT_CHARGE_MESSAGE, Variables::new());
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
