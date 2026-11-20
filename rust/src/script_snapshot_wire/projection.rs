//! Project initialized statescript snapshots and pack state payloads.

use std::collections::{BTreeMap, BTreeSet};

use crate::script_codecs::{Codec, PayloadError, StatePayload};
use crate::script_graph::{Graph, Node, asset_guid};
use crate::script_init::{Issue, Snapshot, StackBinding, State, StateKind};
use crate::script_wire::{Binding, Value, Variables};

use super::{
    OWNER_HEALTH, ProjectedEvent, ProjectedInstance, ProjectedSnapshot, ProjectedState,
    ProjectedStateLink, ProjectedVariables, SnapshotEncoder, SnapshotError, StateSite, View,
    VolleyActivation,
};

impl<'a> SnapshotEncoder<'a> {
    pub fn project(
        &self,
        snapshot: &Snapshot,
        view: View,
    ) -> Result<ProjectedSnapshot, SnapshotError> {
        self.project_with_exclusions(snapshot, view, None, None)
    }

    /// A later projection keeps the initial graph tree's entity exclusions.
    /// Per-instance exclusions still follow each instance's own definition.
    pub fn project_next(
        &self,
        snapshot: &Snapshot,
        baseline: &ProjectedSnapshot,
    ) -> Result<ProjectedSnapshot, SnapshotError> {
        if snapshot.entity != baseline.entity {
            return Err(SnapshotError::DifferentStream);
        }
        self.project_with_exclusions(
            snapshot,
            baseline.view,
            Some(&baseline.entity_exclusions),
            None,
        )
    }

    fn project_with_exclusions(
        &self,
        snapshot: &Snapshot,
        view: View,
        frozen: Option<&BTreeSet<u32>>,
        only_instance: Option<u16>,
    ) -> Result<ProjectedSnapshot, SnapshotError> {
        let relevant = |diagnostic: &&crate::script_init::Diagnostic| {
            !diagnostic.server_only
                && diagnostic.issue != Issue::ObservationUnavailable
                && only_instance.is_none_or(|id| diagnostic.site.instance == id)
        };
        if snapshot
            .diagnostics
            .iter()
            .any(|diagnostic| relevant(&diagnostic))
        {
            return Err(SnapshotError::Incomplete {
                entity: snapshot.entity,
                diagnostics: snapshot
                    .diagnostics
                    .iter()
                    .filter(relevant)
                    .cloned()
                    .collect(),
            });
        }
        let mut entity_exclusions = frozen
            .cloned()
            .unwrap_or_else(|| BTreeSet::from([OWNER_HEALTH]));
        let mut instances = BTreeMap::new();
        for (&id, instance) in &snapshot.instances {
            if instance.is_server_only() || only_instance.is_some_and(|only| id != only) {
                continue;
            }
            let graph = self.graph(id, instance.graph)?;
            let exclusions = server_only_variables(graph);
            let variables =
                self.project_variables(snapshot, id, &instance.variables, &exclusions, view)?;
            if frozen.is_none() {
                entity_exclusions.extend(exclusions);
            }
            let mut states = BTreeMap::new();
            for (&raw, state) in &instance.states {
                let site = StateSite {
                    instance: id,
                    graph: graph.index,
                    state: raw,
                };
                let node = graph
                    .state(usize::from(raw))
                    .ok_or(SnapshotError::MissingState(site))?;
                if !node.is_networked()
                    || (view == View::Observer
                        && (presenter(node) || !graph.remote_positions().contains(&node.pos)))
                {
                    continue;
                }
                let codec = self.codecs.codec(&node.class);
                if !state.active && !(state.activation != 0 && codec.is_some_and(Codec::stec)) {
                    continue;
                }
                if view == View::Owner && uncertain_polled_switch(node, state) {
                    continue;
                }
                let payload = codec
                    .filter(|codec| codec.projected())
                    .and_then(|codec| state_payload(state, codec, snapshot.now_ms))
                    .ok_or_else(|| SnapshotError::UnsupportedState {
                        site,
                        class: node.class.clone(),
                        kind: state.kind,
                    })?;
                states.insert(
                    raw,
                    ProjectedState {
                        active: state.active,
                        payload,
                        link: match state.kind {
                            StateKind::Registered { next: Some(link) }
                                if !server_only_instance(snapshot, link.0) =>
                            {
                                Some(self.project_state_link(snapshot, view, link).ok_or_else(
                                    || SnapshotError::Payload {
                                        site,
                                        source: PayloadError::Field("link".into()),
                                    },
                                )?)
                            }
                            _ => None,
                        },
                        volley: match state.kind {
                            StateKind::WeaponVolley {
                                ammo_var, ammo0, ..
                            } => Some(VolleyActivation {
                                identity: (state.activation, state.start_ms),
                                ammo: ammo_var.map(|variable| (variable, ammo0)),
                            }),
                            _ => None,
                        },
                    },
                );
            }
            instances.insert(
                id,
                ProjectedInstance {
                    graph: instance.graph,
                    parent: instance.parent,
                    variables,
                    states,
                    events: if view == View::Owner {
                        self.project_timers(snapshot, id, graph)?
                    } else {
                        Vec::new()
                    },
                },
            );
        }
        let variables = if let Some(id) = only_instance {
            let graph = self.graph(id, snapshot.instances[&id].graph)?;
            let related: BTreeSet<_> = graph
                .sync_vars
                .iter()
                .filter(|var| var.1 == 1)
                .filter_map(|var| var.0)
                .collect();
            let excluded = snapshot
                .variables
                .keys()
                .filter(|&variable| !related.contains(variable))
                .copied()
                .chain(entity_exclusions.iter().copied())
                .collect();
            self.project_variables(snapshot, 0, &snapshot.variables, &excluded, view)?
        } else {
            self.project_variables(snapshot, 0, &snapshot.variables, &entity_exclusions, view)?
        };
        Ok(ProjectedSnapshot {
            entity: snapshot.entity,
            view,
            variables,
            entity_exclusions,
            instances,
            callout: 0,
            callouts: Vec::new(),
            emit_events: true,
        })
    }

    fn project_state_link(
        &self,
        snapshot: &Snapshot,
        view: View,
        (instance, raw): (u16, u16),
    ) -> Option<ProjectedStateLink> {
        let target = snapshot.instances.get(&instance)?;
        let graph = self.graphs.graph(target.graph)?;
        let node = graph.state(usize::from(raw))?;
        if !node.is_networked() || node.server_only {
            return None;
        }
        let (index, width) = match view {
            View::Owner => (raw, graph.header.states_bits),
            View::Observer => {
                let remote = u16::try_from(node.remote).ok()?;
                let mapped = graph.remote_node(usize::from(remote))?;
                if mapped.pos != node.pos {
                    return None;
                }
                (remote, graph.header.remote_bits)
            }
        };
        (width <= 32 && u64::from(index) < 1_u64 << width).then_some(ProjectedStateLink {
            instance,
            index,
            width,
        })
    }

    fn project_variables(
        &self,
        snapshot: &Snapshot,
        owner: u16,
        values: &Variables,
        excluded: &BTreeSet<u32>,
        view: View,
    ) -> Result<ProjectedVariables, SnapshotError> {
        let owner_only = if view == View::Observer && owner != 0 {
            snapshot
                .instances
                .get(&owner)
                .map(|instance| self.graph(owner, instance.graph))
                .transpose()?
                .map(Graph::owner_only_instance_vars)
        } else {
            None
        };
        let mut projected = ProjectedVariables::default();
        for (&variable, base) in values {
            if excluded.contains(&variable)
                || owner_only.is_some_and(|private| private.binary_search(&variable).is_ok())
            {
                continue;
            }
            let key = (owner, variable);
            let bindings = snapshot.bindings.get(&key);
            let value = match snapshot.resolved_bindings.get(&key) {
                Some(resolved) => resolved,
                None if bindings.is_some() => {
                    return Err(SnapshotError::MissingBoundValue { owner, variable });
                }
                None => base,
            };
            // Python evaluates an authored `STU_7757D403` message table to None
            // and never sends an unbound None (`script.driver` `_add_var`,
            // `observers._add_vars`), so an unbound map is omitted like null.
            // A bound map has no reference form and stays refused.
            if matches!(value, Value::Map(_) | Value::IdentifierMap(_)) {
                if bindings.is_some() {
                    return Err(SnapshotError::UnsupportedMap { owner, variable });
                }
                continue;
            }
            if let Some(bindings) = bindings.filter(|_| view == View::Owner) {
                let links = self.project_bindings(snapshot, owner, variable, bindings)?;
                if !links.is_empty() {
                    projected.bindings.insert(variable, links);
                }
            }
            if !matches!(value, Value::Null) || projected.bindings.contains_key(&variable) {
                projected.values.insert(variable, value.clone());
            }
        }
        Ok(projected)
    }

    fn project_bindings(
        &self,
        snapshot: &Snapshot,
        owner: u16,
        variable: u32,
        bindings: &[StackBinding],
    ) -> Result<Vec<Binding>, SnapshotError> {
        let mut links = Vec::with_capacity(bindings.len());
        for &binding in bindings {
            let invalid = || SnapshotError::InvalidBinding {
                owner,
                variable,
                binding,
            };
            let source = snapshot
                .instances
                .get(&binding.instance)
                .ok_or_else(invalid)?;
            if source.is_server_only() {
                continue;
            }
            if !source.states.get(&binding.raw_state).is_some_and(|state| {
                state.active
                    && matches!(
                        (state.kind, binding.slot),
                        (StateKind::Stack { .. }, 0) | (StateKind::Ability { .. }, 1)
                    )
            }) {
                return Err(invalid());
            }
            let graph = self.graph(binding.instance, source.graph)?;
            let node = graph
                .state(usize::from(binding.raw_state))
                .ok_or_else(invalid)?;
            if node.is_networked() {
                links.push(Binding {
                    instance: binding.instance,
                    state: binding.raw_state,
                    state_bits: graph.header.states_bits,
                    slot: binding.slot,
                    priority: binding.priority.level,
                    above: binding.priority.above_equals,
                });
            }
        }
        Ok(links)
    }

    /// Project the already-mounted mission notification graph without pulling
    /// server spawners or unrelated entity variables into its observer stream.
    /// Its authored entity sync slots supply difficulty, gate and timer inputs
    /// consumed by the client-only presenters.
    pub(crate) fn mission_notifications(
        &self,
        snapshot: &Snapshot,
    ) -> Result<Option<ProjectedSnapshot>, SnapshotError> {
        let Some((&id, _)) = snapshot.instances.iter().find(|(_, instance)| {
            !instance.is_server_only()
                && instance.graph == 0x1188
                && instance.parent == Some((2, 68))
        }) else {
            return Ok(None);
        };
        self.project_with_exclusions(snapshot, View::Observer, None, Some(id))
            .map(Some)
    }

    pub(crate) fn post_match_screens(
        &self,
        snapshot: &Snapshot,
    ) -> Result<Option<ProjectedSnapshot>, SnapshotError> {
        let Some((&manager, screen_manager)) = snapshot.instances.iter().find(|(_, instance)| {
            !instance.is_server_only()
                && instance.graph == 0x180a
                && instance.parent.is_some_and(|(parent, raw)| {
                    raw == 45 && snapshot.instances[&parent].graph == 0x0cbb
                })
        }) else {
            return Ok(None);
        };
        let mut instances = BTreeMap::new();
        let mut entity_inputs = BTreeSet::new();
        // The native observer excludes server-only list rows. A graph played
        // by a row is an independent observer root with its resolved start values.
        for (&id, instance) in &snapshot.instances {
            if instance.is_server_only() {
                continue;
            }
            let mut parent = instance.parent;
            if id != manager {
                let Some((owner, raw)) = parent else { continue };
                if owner == manager {
                    continue; // same-graph list row
                }
                if snapshot.instances[&owner].parent == Some((manager, 17)) && raw == 16 {
                    parent = None;
                } else if !instances.contains_key(&owner) {
                    continue;
                }
            }
            let graph = self.graph(id, instance.graph)?;
            entity_inputs.extend(
                graph
                    .sync_vars
                    .iter()
                    .filter(|var| var.1 == 1)
                    .filter_map(|var| var.0),
            );
            let mut states = BTreeMap::new();
            for (&raw, state) in &instance.states {
                if !state.active
                    || (id == manager
                        && raw != 21
                        && state.kind
                            != StateKind::PersistentSend(
                                crate::junkenstein_messages::POST_MATCH_SIGNAL,
                            ))
                {
                    continue;
                }
                let Some(node) = graph.state(usize::from(raw)).filter(|node| {
                    node.is_networked()
                        && graph.remote_positions().contains(&node.pos)
                        && !presenter(node)
                }) else {
                    continue;
                };
                let Some(codec) = self
                    .codecs
                    .codec(&node.class)
                    .filter(|codec| codec.projected())
                else {
                    continue;
                };
                let Some(mut payload) = state_payload(state, codec, snapshot.now_ms) else {
                    continue;
                };
                if matches!(state.kind, StateKind::ClientPulse { fresh_at: None, .. }) {
                    // The screen graph's first pulse opens its client-only UI.
                    // Deliver it even when it was authored on the instance's first tick.
                    payload.insert("fresh".into(), Value::Bool(true));
                }
                states.insert(
                    raw,
                    ProjectedState {
                        active: true,
                        payload,
                        link: None,
                        volley: None,
                    },
                );
            }
            let mut variables = ProjectedVariables::default();
            let excluded = server_only_variables(graph);
            for (&key, value) in &instance.variables {
                // Preserve the manager's resolved 0695 inputs alongside its
                // sequence clock. Its observer listener does not run the
                // server-side parameter assignment on the client.
                if (id == manager && !matches!(key, 59 | 7525 | 10186 | 11251 | 13151))
                    || excluded.contains(&key)
                {
                    continue;
                }
                let value = snapshot.resolved_bindings.get(&(id, key)).unwrap_or(value);
                if !matches!(value, Value::Null) {
                    variables.values.insert(key, value.clone());
                }
            }
            if graph.index == 0x027f {
                if let Some(winner) = screen_manager.variables.get(&7525) {
                    // The banner starts its client-only 0695 listener after the
                    // phase handoff. Seed the same resolved parameter at creation.
                    variables.values.insert(7525, winner.clone());
                }
            }
            instances.insert(
                id,
                ProjectedInstance {
                    graph: instance.graph,
                    parent,
                    variables,
                    states,
                    events: Vec::new(),
                },
            );
        }
        let excluded = snapshot
            .variables
            .keys()
            .filter(|key| !entity_inputs.contains(key))
            .copied()
            .collect();
        let variables =
            self.project_variables(snapshot, 0, &snapshot.variables, &excluded, View::Observer)?;
        Ok(Some(ProjectedSnapshot {
            entity: snapshot.entity,
            view: View::Observer,
            entity_exclusions: BTreeSet::new(),
            variables,
            instances,
            callout: 0,
            callouts: Vec::new(),
            emit_events: true,
        }))
    }

    fn project_timers(
        &self,
        snapshot: &Snapshot,
        id: u16,
        graph: &Graph,
    ) -> Result<Vec<ProjectedEvent>, SnapshotError> {
        let Some(timers) = snapshot.pending_timers.get(&id) else {
            return Ok(Vec::new());
        };
        let mut events = Vec::with_capacity(timers.len());
        for timer in timers {
            let site = StateSite {
                instance: id,
                graph: graph.index,
                state: timer.raw_state,
            };
            let node = graph
                .state(usize::from(timer.raw_state))
                .ok_or(SnapshotError::MissingState(site))?;
            if !node.is_networked() {
                continue;
            }
            if !self.codecs.sendable(&node.class, true) {
                return Err(SnapshotError::UnsupportedTimer {
                    site,
                    class: node.class.clone(),
                });
            }
            events.push(ProjectedEvent {
                due_ms: timer.due_ms,
                raw_state: timer.raw_state,
                finish: timer.finish,
                parameter: timer.parameter,
            });
        }
        Ok(events)
    }
}

fn server_only_instance(snapshot: &Snapshot, id: u16) -> bool {
    snapshot
        .instances
        .get(&id)
        .is_some_and(|instance| instance.is_server_only())
}

fn server_only_variables(graph: &Graph) -> BTreeSet<u32> {
    graph
        .fields
        .get("m_9CEC6985")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(asset_guid)
        .map(|guid| u32::from(guid as u16))
        .collect()
}

fn presenter(node: &Node) -> bool {
    node.class == "STUStatescriptStateUXPresenter" || node.fields.get("m_45216F79").is_some()
}

/// A polled switch whose condition read a native the owner client may answer
/// differently. The client re-checks such a switch every update with its own
/// answer, so the reference leaves it out of owner frames and keeps sending
/// its live subgraph (driver.py `_add_state`).
pub(crate) fn uncertain_polled_switch(node: &Node, state: &State) -> bool {
    state.reads_untrusted_native()
        && matches!(
            node.class.as_str(),
            "STUStatescriptStateBooleanSwitch" | "STU_DEBD057B"
        )
}

fn state_payload(state: &State, codec: &Codec, now_ms: u64) -> Option<StatePayload> {
    Some(match (state.kind, codec.codec.as_deref()) {
        (
            StateKind::Plain
            | StateKind::ForCode
            | StateKind::ModifyHealth
            | StateKind::Shockwave { .. }
            | StateKind::PlacementSelector { .. }
            | StateKind::MoveTo { .. }
            | StateKind::ShieldRecharge
            | StateKind::NamedShieldDecay
            | StateKind::ExplosionContact
            | StateKind::DeadeyeEffects
            | StateKind::WitchStaffEffect
            | StateKind::WitchStaffHealing
            | StateKind::SummonerBeamRay
            | StateKind::SummonerBeamDamage
            | StateKind::HealthPool { .. }
            | StateKind::HealthCapacity
            | StateKind::AiWeaponOverride { .. }
            | StateKind::AiControlOverride { .. }
            | StateKind::AiControlSetting
            | StateKind::AiFacing(_)
            | StateKind::AiBehavior { .. }
            | StateKind::DisplayName { .. }
            | StateKind::TargetTag { .. }
            | StateKind::Wait
            | StateKind::WaitFrames { .. }
            | StateKind::AiCandidate { .. }
            | StateKind::MovementMod
            | StateKind::VoiceStimulus
            | StateKind::BossIntroVoice(_)
            | StateKind::GateVoice(_)
            | StateKind::VictoryVoice
            | StateKind::SameTeamContact { .. }
            | StateKind::CosmeticEntity
            | StateKind::Watch
            | StateKind::Switch { .. }
            | StateKind::ActiveQuery
            | StateKind::GamePadVibration,
            Some("none"),
        ) if codec.empty() => StatePayload::new(),
        (StateKind::BooleanSwitch(current), Some("switch")) => {
            StatePayload::from([("current".into(), Value::Bool(current))])
        }
        (StateKind::DelayedSwitch { current, undecided }, Some("two_bits")) => {
            StatePayload::from([
                ("current".into(), Value::Bool(current)),
                ("undecided".into(), Value::Bool(undecided)),
            ])
        }
        (StateKind::MovementSample { flags: mask, .. }, Some("flags13")) => {
            StatePayload::from([("value".into(), Value::Integer(i64::from(mask)))])
        }
        (StateKind::SubScript { child }, Some("subscript")) => StatePayload::from([(
            "child".into(),
            Value::Integer(i64::from(child.unwrap_or_default())),
        )]),
        (
            StateKind::ButtonBlock(_)
            | StateKind::OutgoingDamageFilter { .. }
            | StateKind::RollMovementParameter,
            Some("link"),
        )
        | (StateKind::Registered { .. }, Some("link" | "linked_target")) => StatePayload::new(),
        (StateKind::MessageIdle | StateKind::BossPauseReceiver { .. }, Some("message")) => {
            StatePayload::new()
        }
        (StateKind::WaitFrames { target }, Some("frames")) => {
            StatePayload::from([("target".into(), Value::Integer(i64::from(target)))])
        }
        (StateKind::CinematicTimeScale { boundary_frame }, Some("value_slot")) => {
            StatePayload::from([
                ("which".into(), Value::Bool(state.active)),
                ("value".into(), Value::Integer(i64::from(boundary_frame))),
            ])
        }
        (StateKind::LogicalButton { .. }, Some("button")) => StatePayload::from([(
            "counter".into(),
            Value::Integer(i64::from(state.activation & 15)),
        )]),
        (StateKind::Stack { top, under }, Some("stack")) => StatePayload::from([
            ("top".into(), Value::Bool(top)),
            ("under".into(), Value::Bool(under)),
        ]),
        (StateKind::Ability { flags }, Some("ability")) => {
            let mut payload =
                StatePayload::from([("flags".into(), Value::Integer(i64::from(flags)))]);
            if let Some(cooldown) = &state.ability_cooldown {
                payload.extend([
                    ("cur".into(), Value::Float(cooldown.current)),
                    ("rate".into(), Value::Float(cooldown.rate)),
                    (
                        "last".into(),
                        Value::Integer(i64::try_from(cooldown.last_ms).ok()?),
                    ),
                ]);
            }
            payload
        }
        (StateKind::Anim, Some("anim")) => StatePayload::from([(
            "counter".into(),
            Value::Integer(i64::from(state.activation & 7)),
        )]),
        (StateKind::Effect, Some("counter2")) => StatePayload::from([(
            "counter".into(),
            Value::Integer(i64::from(state.activation & 3)),
        )]),
        (
            StateKind::SendMessage | StateKind::ProjectedSelfSend | StateKind::PersistentSend(_),
            Some("send"),
        ) => StatePayload::new(),
        (StateKind::ClientPulse { count, fresh_at }, Some("pulser")) => StatePayload::from([
            ("count".into(), Value::Integer(i64::from(count))),
            (
                "fresh".into(),
                Value::Bool(fresh_at.is_some_and(|at| now_ms.saturating_sub(at) <= 500)),
            ),
        ]),
        (StateKind::Plain | StateKind::PhaseScriptList, Some("hits"))
        | (StateKind::PhaseTransition, Some("bit")) => StatePayload::new(),
        (StateKind::EmptyTargets, Some("targets")) => StatePayload::new(),
        (StateKind::TrackedTargets, Some("targets")) => StatePayload::from([(
            "targets".into(),
            Value::Array(
                state
                    .tracked_targets
                    .iter()
                    .map(|entity| Value::Array(vec![Value::Entity(*entity), Value::Integer(0)]))
                    .collect(),
            ),
        )]),
        (StateKind::ChaseVar, Some("chase")) => {
            let chase = state.chase.as_ref()?;
            if !matches!(chase.current, Value::Float(_) | Value::Vector(_)) {
                return None;
            }
            let mut payload = StatePayload::from([
                ("cur".into(), chase.current.clone()),
                ("reached".into(), Value::Bool(chase.reached)),
                (
                    "last".into(),
                    Value::Integer(i64::try_from(chase.last_ms).ok()?),
                ),
            ]);
            if let Some(remaining) = chase.remaining_ms {
                payload.insert("remaining".into(), Value::Integer(remaining));
            }
            payload
        }
        (
            StateKind::WeaponVolley {
                volleys, offset_ms, ..
            },
            Some("volley"),
        ) => StatePayload::from([
            (
                "start".into(),
                Value::Integer(i64::try_from(state.start_ms).ok()?),
            ),
            (
                "offset".into(),
                Value::Integer(i64::try_from(offset_ms).ok()?),
            ),
            ("volleys".into(), Value::Integer(i64::from(volleys))),
            (
                "counter".into(),
                Value::Integer(i64::from(state.activation)),
            ),
        ]),
        _ => return None,
    })
}
