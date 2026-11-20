//! Charge admission and damage credit for the supported owner profile.

use std::{io, sync::Arc};

use crate::{
    game_combat::AppliedDamage,
    game_content::is_junkenstein_mode,
    game_heroes::CASSIDY,
    game_replication::OwnerRoomActivity,
    script_expr::{Scope, Variable},
    script_graph::Body,
    script_init::{Root, Snapshot, State, StateKind, named_pool_records},
    script_wire::{Value, Variables},
};

use super::{
    OwnerBodyPreparationError,
    lifecycle::{DEADEYE_GRAPH, PlayerBodyRuntime},
    unsupported,
};

#[derive(Clone, Copy)]
pub(super) struct ChargeBinding {
    recipient: ChargeRecipient,
    admission: u32,
    damage_message: u64,
    inhibited_by: Option<(u32, u16)>,
}

#[derive(Clone, Copy)]
enum ChargeRecipient {
    LegacyGraph(u32),
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
    LegacyDefault,
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
            return Ok(if body.authored_charge_graph.is_some() {
                Self::Disabled
            } else {
                Self::LegacyDefault
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
            inhibited_by: profile
                .inhibit
                .as_ref()
                .map(|rule| (rule.graph, rule.state)),
        }))
    }

    pub(super) fn bind(&self, mode: u64, hero: u64) -> Option<ChargeBinding> {
        match self {
            Self::LegacyDefault => {
                (hero == CASSIDY && is_junkenstein_mode(mode)).then_some(ChargeBinding {
                    recipient: ChargeRecipient::LegacyGraph(0x0b8b),
                    admission: 2426,
                    damage_message: 0x0240_0000_0000_0012,
                    inhibited_by: Some((DEADEYE_GRAPH, 0)),
                })
            }
            Self::Disabled => None,
            Self::Configured(binding) => Some(*binding),
        }
    }
}

impl ChargeBinding {
    pub(super) fn is_configured(self) -> bool {
        !matches!(self.recipient, ChargeRecipient::LegacyGraph(_))
    }

    fn activity_allows(self, activity: Option<OwnerRoomActivity>) -> bool {
        match self.recipient {
            ChargeRecipient::LegacyGraph(_) => {
                activity.is_none_or(OwnerRoomActivity::encounter_active)
            }
            ChargeRecipient::MountedRoot { .. } | ChargeRecipient::SubScript { .. } => {
                activity.is_none_or(OwnerRoomActivity::allows_body_activity)
            }
        }
    }

    fn receiving_instance(self, snapshot: &Snapshot) -> Option<u16> {
        match self.recipient {
            ChargeRecipient::LegacyGraph(graph) => snapshot
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
            && (!binding.is_configured() || binding.activity_allows(self.world.room_activity));
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
            if matches!(binding.recipient, ChargeRecipient::SubScript { .. }) {
                return Ok(());
            }
            return Err(unsupported(
                "damage credit requires the authored common charge graph",
            ));
        };
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
