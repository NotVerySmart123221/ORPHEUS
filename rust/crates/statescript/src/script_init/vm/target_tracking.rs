//! State-owned child instances keyed by target identity.

use std::collections::BTreeMap;

use crate::{
    script_expr::{self, EvalError},
    script_graph::Node,
    script_init::{ViewTargetQuery, machine::ActorTargetGuard},
    script_wire::{Value, Variables},
};

use super::super::shapes::pool_number;
use super::{InitFault, Issue, Startup, StateKey, StateKind, Vm, World, field, source_key};

const MAX_TRACKED_TARGETS: usize = 64;

impl<W: World> Vm<'_, '_, '_, W> {
    pub(super) fn sample_view_targets(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<bool, InitFault> {
        let track = node.view_track().expect("registered view target shape");
        let site = self.site(key.instance, node);
        let frame = self.machine.frame;
        let state = self.state_mut(key).expect("active view tracker");
        let StateKind::ViewTargets { next_sample } = &mut state.kind else {
            unreachable!("view tracker dispatcher");
        };
        let elapsed = frame.saturating_sub(*next_sample);
        *next_sample = frame.saturating_add(track.period - elapsed % track.period);
        let mut evaluate = |config| self.evaluate_at(site, config, None);
        let (source, eye_heading) = if let Some(heading) = track.eye_heading {
            let Some(heading) = evaluate(heading)
                .and_then(pool_number)
                .filter(|v| v.is_finite())
            else {
                return Ok(false);
            };
            (site.entity, Some(heading))
        } else {
            let Some(Value::Entity(source)) = evaluate(track.origin) else {
                return Ok(false);
            };
            (source, None)
        };
        let limit = if track.limit.is_null() && eye_heading.is_some() {
            Some(Value::Integer(MAX_TRACKED_TARGETS as i64))
        } else {
            evaluate(track.limit)
        };
        let (Some(Value::Vector(offset)), Some(range), Some(aperture), Some(Value::Integer(limit))) = (
            evaluate(track.offset),
            evaluate(track.range).and_then(pool_number),
            evaluate(track.aperture).and_then(pool_number),
            limit,
        ) else {
            return Ok(false);
        };
        let Some(limit) = usize::try_from(limit)
            .ok()
            .filter(|limit| *limit <= MAX_TRACKED_TARGETS)
        else {
            self.report(site, Issue::UnsupportedField("view target limit"));
            return Ok(false);
        };
        if source != site.entity
            || !offset.into_iter().all(f64::is_finite)
            || !range.is_finite()
            || range <= 0.0
            || !(0.0 < aperture && aperture < 180.0)
        {
            self.report(site, Issue::UnsupportedField("view target geometry"));
            return Ok(false);
        }
        let query = ViewTargetQuery {
            source,
            offset,
            eye_heading,
            range,
            aperture,
            limit,
            profile: track.profile,
            masks: track.masks,
            ranking: track.ranking,
            sight: self.graphs.view_sight_profile(track.profile),
        };
        let Some(targets) = self.world.view_targets(site, &query) else {
            self.report(
                site,
                Issue::UnsupportedField("view target query unavailable"),
            );
            return Ok(false);
        };
        if targets.len() > limit
            || targets.iter().enumerate().any(|(index, &target)| {
                target == source
                    || !script_expr::valid_entity(target)
                    || targets[..index].contains(&target)
            })
        {
            self.report(site, Issue::UnsupportedField("view target results"));
            return Ok(false);
        }
        self.write(
            source_key(key.instance, track.output),
            Value::Array(targets.iter().copied().map(Value::Entity).collect()),
        )?;
        self.state_mut(key)
            .expect("active view tracker")
            .tracked_targets = targets;
        Ok(true)
    }

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
        let mut targets = BTreeMap::new();
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
            targets.insert(entity, None);
            if targets.len() > MAX_TRACKED_TARGETS {
                self.report(site, Issue::UnsupportedField("tracked target limit"));
                return Ok(());
            }
        }

        if node.tracks_room_actor_identity() {
            targets.retain(|&entity, identity| {
                *identity = self.world.room_actor_identity(entity);
                identity.is_some()
            });
        }
        let selected: Vec<_> = targets.keys().copied().collect();
        self.retain_target_children(key, &selected)?;
        for (entity, identity) in targets {
            self.ensure_target_child(key, entity, child_graph, true, identity, || {
                Variables::from([(variable, Value::Entity(entity))])
            })?;
        }
        self.state_mut(key)
            .expect("active entity tracker")
            .tracked_targets = selected;
        self.try_complete(key, node, 0)
    }

    /// Clear scopes before dispatching any work against a new room sample.
    pub(super) fn retire_stale_actor_children(&mut self) -> Result<(), InitFault> {
        if self.machine.actor_target_guards.is_empty() {
            return Ok(());
        }
        for _ in 0..self.machine.actor_target_guards.len() {
            self.spend("tracked actor identity")?;
        }
        let stale: Vec<_> = self
            .machine
            .actor_target_guards
            .iter()
            .filter_map(|(&child, guard)| {
                (self.world.room_actor_identity(guard.entity) != Some(guard.identity))
                    .then_some(child)
            })
            .collect();
        if stale.is_empty() {
            return Ok(());
        }
        // Releasing a child may resynchronize inputs in other children.
        // Mark the whole retiring set before releasing the first effect.
        self.quiesce_actor_scopes(&stale)?;
        for child in stale {
            self.destroy(child)?;
        }
        Ok(())
    }

    pub(super) fn detach_actor_target_guard(&mut self, child: u16) {
        let Some(guard) = self.machine.actor_target_guards.remove(&child) else {
            return;
        };
        if let Some(tracker) = self
            .state_mut(guard.tracker)
            .filter(|tracker| tracker.target_children.get(&guard.entity) == Some(&child))
        {
            tracker.target_children.remove(&guard.entity);
            tracker
                .tracked_targets
                .retain(|&entity| entity != guard.entity);
        }
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
        actor_identity: Option<u64>,
        variables: impl FnOnce() -> Variables,
    ) -> Result<Option<u16>, InitFault> {
        let children = &self
            .state(key)
            .expect("target tracker state")
            .target_children;
        if let Some(&child) = children.get(&entity) {
            return Ok(Some(child));
        }
        let scope = self.instance_scope(key.instance).hide_if(server_only);
        let child = self.machine.next_instance_id(scope.is_isolated())?;
        let created = self.create(child, graph, None, variables(), Startup::Ordinary, scope);
        // Track a partially initialized child too, so teardown can reclaim it.
        if self.machine.snapshot.instances.contains_key(&child) {
            self.state_mut(key)
                .expect("target tracker state")
                .target_children
                .insert(entity, child);
            if let Some(identity) = actor_identity {
                self.machine.actor_target_guards.insert(
                    child,
                    ActorTargetGuard {
                        tracker: key,
                        entity,
                        identity,
                    },
                );
            }
        }
        Ok(created?.then_some(child))
    }
}
