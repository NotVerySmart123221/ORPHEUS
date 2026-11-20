//! Charge admission and damage credit for the supported owner profile.

use std::io;

use crate::{
    game_combat::AppliedDamage,
    game_content::is_junkenstein_mode,
    game_heroes::CASSIDY,
    script_expr::{Scope, Variable},
    script_init::State,
    script_wire::{Value, Variables},
};

use super::{
    lifecycle::{DEADEYE_GRAPH, PlayerBodyRuntime},
    unsupported,
    world::BodyWorld,
};

pub(super) struct ChargeBinding {
    pub(super) graph: u32,
    admission: u32,
    damage_message: u64,
    inhibited_by: (u32, u16),
}

impl BodyWorld {
    pub(super) fn charge_binding(&self) -> Option<ChargeBinding> {
        // The common graph can be present on other bodies. Only this existing
        // profile has a room-backed charge admission and damage credit path.
        (self.hero == CASSIDY && is_junkenstein_mode(self.mode)).then_some(ChargeBinding {
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
        self.encounter_active = active;
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
        if !self.encounter_active
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
            (0x15, Value::Entity(damage.target)),
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
            // The existing credit projection has no separate mitigation entries.
            (0x624, Value::Array(Vec::new())),
        ]);
        self.component
            .queue_message(instance, binding.damage_message, parameters)
            .map_err(unsupported)?;
        self.world.damage_credit = Some(damage);
        let result = self.component.run_now(&mut self.world).map_err(unsupported);
        self.world.damage_credit = None;
        result
    }
}
