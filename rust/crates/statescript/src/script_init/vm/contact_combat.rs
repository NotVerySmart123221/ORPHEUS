//! One damage settlement per activation of a state linked to a contact volume.

use crate::{
    script_expr::{self, EvalError, Scope, Variable, native_shape},
    script_graph::Node,
    script_init::{
        ContactVolume, InitFault, Issue, Knockback, World,
        machine::StateKey,
        shapes::{pool_number, source_key},
    },
    script_wire::Value,
};

use super::Vm;

impl<'graph, W: World> Vm<'graph, '_, '_, W> {
    pub(super) fn contact_damage_tracker(
        &self,
        key: StateKey,
        node: &Node,
    ) -> Option<(&'graph Node, Variable, u64)> {
        let fields = &node.fields;
        let health = &fields["m_modifyHealth"];
        if node.class != "STUStatescriptStateModifyHealth"
            || fields["m_DD792FF0"].as_u64() != Some(3)
            || fields["m_8B707686"].as_u64().unwrap_or(0) != 0
            || !fields["m_95F1FB0B"].is_null()
            || health["m_93CDFD2F"].as_u64() != Some(1)
            || !native_shape(&health["m_instigator"], "STU_B5A0CAF0", None)
            || !native_shape(&health["m_724CBA57"], "STU_B5A0CAF0", None)
        {
            return None;
        }
        let target = script_expr::lvalue(&fields["m_FA3ECC43"]).ok()??;
        let paired = script_expr::lvalue(&fields["m_B12BFE8B"]).ok()??;
        if target.scope != Scope::Instance || paired.scope != Scope::Instance {
            return None;
        }
        let mut trackers = self.graph(key.instance).nodes.iter().filter_map(|tracker| {
            let (output, pairs, profile) = tracker.contact_volume_bindings()?;
            let raw = tracker.state?;
            (output == target
                && pairs == paired
                && tracker
                    .links("m_subgraphPlug")
                    .any(|(pos, _)| pos == Some(node.pos))
                && self
                    .state(StateKey {
                        instance: key.instance,
                        raw,
                    })
                    .is_some_and(|state| state.active))
            .then_some((tracker, target, profile))
        });
        let tracker = trackers.next()?;
        trackers.next().is_none().then_some(tracker)
    }

    pub(super) fn begin_contact_damage(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let Some((tracker, output, profile)) = self.contact_damage_tracker(key, node) else {
            return Ok(());
        };
        let sample_site = self.site(key.instance, tracker);
        if self.graphs.static_ray_category(profile).is_none()
            || self.world.room_actor_identity(sample_site.entity).is_none()
        {
            // Loading and projection-only callers have no posed room body.
            return Ok(());
        }
        let source_frame = self.machine.frame;
        let sample = self.with_instance_context(sample_site, None, |context| {
            let eval = |field: &str, context: &mut _| {
                script_expr::evaluate(&tracker.fields[field], context)
            };
            let Value::Vector(offset) = eval("m_05BE62DD", context)? else {
                return Err(EvalError::MalformedConfig("contact volume offset"));
            };
            let Value::Vector(sight_origin) = eval("m_D59188D5", context)? else {
                return Err(EvalError::MalformedConfig("contact volume sight origin"));
            };
            let radius = pool_number(eval("m_radius", context)?)
                .filter(|radius| *radius > 0.0)
                .ok_or(EvalError::MalformedConfig("contact volume radius"))?;
            Ok(ContactVolume {
                source_frame,
                offset,
                sight_origin,
                radius,
                profile,
            })
        });
        let sample = match sample {
            Ok(sample) => sample,
            Err(error) => {
                self.report(sample_site, Issue::Expression(error));
                return Ok(());
            }
        };
        let Some(targets) = self.world.contact_volume_targets(sample_site, sample) else {
            // No authored profile or no posed sample. The animation has its own lifecycle.
            return Ok(());
        };
        let site = self.site(key.instance, node);
        if !targets.is_empty() {
            let health = &node.fields["m_modifyHealth"];
            let payload = self.with_instance_context(site, None, |context| {
                let mut eval = |config: &serde_json::Value| script_expr::evaluate(config, context);
                let raw_amount = eval(&health["m_amount"])?;
                let amount = if matches!(raw_amount, Value::Null) {
                    0.0
                } else {
                    pool_number(raw_amount)
                        .filter(|value| (0.0..=f64::from(f32::MAX)).contains(value))
                        .ok_or(EvalError::MalformedConfig("contact damage amount"))?
                };
                let force = Knockback::from_authored(health, &mut eval);
                let origin = if force.is_some() {
                    let Value::Vector(origin) = eval(&health["m_C0139FA6"])? else {
                        return Err(EvalError::MalformedConfig("contact impulse origin"));
                    };
                    origin
                } else {
                    sample.sight_origin
                };
                Ok((f64::from(amount as f32), force, origin))
            });
            let (amount, force, origin) = match payload {
                Ok(payload) => payload,
                Err(error) => {
                    self.report(site, Issue::Expression(error));
                    return Ok(());
                }
            };
            let forward = self.world.authored_forward(site.entity);
            let mut changes = Vec::with_capacity(targets.len());
            for target in &targets {
                self.spend("contact damage target")?;
                let [x, _, z] = target.feet;
                let planar = [x - origin[0], z - origin[2]];
                let distance = planar[0].hypot(planar[1]);
                let direction = if distance == 0.0 {
                    forward.map_or([0.0; 2], |[x, _, z]| [x, z])
                } else {
                    planar
                };
                let impulse = force
                    .as_ref()
                    .and_then(|force| force.radial_delta(direction, distance, true, 1.0));
                if amount > 0.0 || impulse.is_some() {
                    changes.push((target.entity, impulse));
                }
            }
            if !changes.is_empty()
                && !self
                    .world
                    .queue_contact_damage(site, source_frame, amount, &changes)
            {
                self.report(site, Issue::UnsupportedField("contact damage request"));
                return Ok(());
            }
        }
        self.write(
            source_key(key.instance, output),
            Value::Array(
                targets
                    .into_iter()
                    .map(|target| Value::Entity(target.entity))
                    .collect(),
            ),
        )?;
        Ok(())
    }
}
