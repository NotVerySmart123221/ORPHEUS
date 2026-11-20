//! State-owned child instances keyed by target identity.

use std::collections::BTreeSet;

use crate::{
    script_expr::{self, EvalError},
    script_wire::{Value, Variables},
};

use super::{InitFault, Instance, Issue, Startup, StateKey, StateKind, Vm, World, field};

const MAX_TRACKED_TARGETS: usize = 64;

impl<W: World> Vm<'_, '_, '_, W> {
    pub(super) fn sample_target_children(&mut self, key: StateKey) -> Result<(), InitFault> {
        let frame = self.machine.frame;
        let node = self
            .graph(key.instance)
            .state(usize::from(key.raw))
            .expect("active entity tracker");
        let (child_graph, variable) = node
            .target_child_binding()
            .expect("validated entity tracker");
        self.state_mut(key).expect("active entity tracker").kind = StateKind::TrackEntities {
            sampled_frame: frame,
        };

        let site = self.site(key.instance, node);
        let Some(sample) = self.observation_value(site, field(&node.fields, "targets")) else {
            return Ok(());
        };
        let Value::Array(entities) = sample else {
            self.report(
                site,
                Issue::Expression(EvalError::MalformedConfig("expected a target entity array")),
            );
            return Ok(());
        };
        let mut targets = BTreeSet::new();
        for target in entities {
            self.spend("tracked target entity")?;
            let entity = match target {
                Value::Entity(entity) if script_expr::valid_entity(entity) => entity,
                _ => {
                    self.report(
                        site,
                        Issue::Expression(EvalError::MalformedConfig("invalid tracked entity")),
                    );
                    return Ok(());
                }
            };
            targets.insert(entity);
            if targets.len() > MAX_TRACKED_TARGETS {
                self.report(site, Issue::UnsupportedField("tracked target limit"));
                return Ok(());
            }
        }

        let targets: Vec<_> = targets.into_iter().collect();
        self.retain_target_children(key, &targets)?;
        for &entity in &targets {
            self.ensure_target_child(key, entity, child_graph, true, || {
                Variables::from([(variable, Value::Entity(entity))])
            })?;
        }
        self.state_mut(key)
            .expect("active entity tracker")
            .tracked_targets = targets;
        self.try_complete(key, node, 0)
    }

    /// Discard dead child records and tear down children whose targets left.
    pub(super) fn retain_target_children(
        &mut self,
        key: StateKey,
        targets: &[u32],
    ) -> Result<(), InitFault> {
        let children = &self
            .state(key)
            .expect("target tracker state")
            .target_children;
        let removed: Vec<_> = children
            .iter()
            .filter_map(|(&entity, &child)| {
                let live = self.machine.snapshot.instances.contains_key(&child);
                (!live || !targets.contains(&entity)).then_some((entity, child, live))
            })
            .collect();
        for (entity, child, live) in removed {
            if live {
                self.destroy(child)?;
            }
            self.state_mut(key)
                .expect("target tracker state")
                .target_children
                .remove(&entity);
        }
        Ok(())
    }

    pub(super) fn ensure_target_child(
        &mut self,
        key: StateKey,
        entity: u32,
        graph: u32,
        server_only: bool,
        variables: impl FnOnce() -> Variables,
    ) -> Result<Option<u16>, InitFault> {
        let children = &self
            .state(key)
            .expect("target tracker state")
            .target_children;
        if let Some(&child) = children.get(&entity) {
            return Ok(Some(child));
        }
        let child =
            u16::try_from(self.machine.next_id).map_err(|_| InitFault::InstanceIdExhausted)?;
        let server_only = server_only
            || self
                .machine
                .snapshot
                .instances
                .get(&key.instance)
                .is_some_and(Instance::is_server_only);
        let created = self.create(
            child,
            graph,
            None,
            variables(),
            Startup::Ordinary,
            server_only,
        );
        // Track a partially initialized child too, so teardown can reclaim it.
        if self.machine.snapshot.instances.contains_key(&child) {
            self.state_mut(key)
                .expect("target tracker state")
                .target_children
                .insert(entity, child);
        }
        Ok(created?.then_some(child))
    }
}
