//! Project an initialized statescript body into the client baseline stream.
//!
//! The owner receives an H0 tree with all networked states off. An observer
//! receives the same tree and the initialized remote-synced states.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use crate::bits::{BitError, BitWriter};
use crate::script_codecs::{Codec, CodecTable, PayloadError, PayloadMode, StatePayload};
use crate::script_expr::Variable;
use crate::script_graph::{Graph, GraphStore};
use crate::script_init::{ChasePayload, Snapshot, StackBinding, StateKind};
use crate::script_stream::StreamError;
use crate::script_wire::{self, Bindings, Value, ValueEncoding, Variables, WireError};

pub(super) const OWNER_HEALTH: u32 = 253;
pub(crate) const COUNTDOWN_VAR: u32 = 1303;
// Hero readiness, battle countdown, and overtime all write entity clock 1303.
pub(crate) const COUNTDOWN_STATES: [u16; 4] = [52, 53, 37, 60];

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CountdownFrame {
    pub value: Value,
    pub states: [Option<ChasePayload>; 4],
}

mod encode;
mod projection;
#[cfg(test)]
mod tests;

pub(crate) use encode::{encounter_countdown_delta, encounter_message_delta, mode_action_delta};
#[cfg(test)]
pub(crate) use projection::uncertain_polled_switch;
#[cfg(test)]
pub(crate) use tests::CODECS as FIXTURE_CODECS;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum View {
    Owner,
    Observer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameCallout {
    pub serial: u32,
    pub instance: u16,
    pub position: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectedSnapshot {
    pub(super) entity: u32,
    pub(super) view: View,
    pub(super) entity_exclusions: BTreeSet<u32>,
    pub(super) variables: ProjectedVariables,
    pub(super) instances: BTreeMap<u16, ProjectedInstance>,
    pub(super) callout: u32,
    pub(super) callouts: Vec<FrameCallout>,
    pub(super) emit_events: bool,
}

impl ProjectedSnapshot {
    pub fn entity(&self) -> u32 {
        self.entity
    }

    pub fn view(&self) -> View {
        self.view
    }

    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }

    pub fn acknowledged_callout(&self) -> u32 {
        self.callout
    }

    pub fn with_callout(mut self, callout: FrameCallout) -> Self {
        self.callouts.push(callout);
        self
    }

    /// Omit per-instance event queues when matching action-only owner deltas.
    pub fn without_events(mut self) -> Self {
        self.emit_events = false;
        self
    }

    /// The activation counter and absolute start in milliseconds from this frame.
    pub fn volley_identity(&self, instance: u16, state: u16) -> Option<(u16, u64)> {
        self.instances
            .get(&instance)?
            .states
            .get(&state)?
            .volley
            .map(|volley| volley.identity)
    }

    #[cfg(test)]
    pub(crate) fn entity_variables(&self) -> &Variables {
        &self.variables.values
    }

    #[cfg(test)]
    pub(crate) fn instance_variables(&self, instance: u16) -> Option<&Variables> {
        Some(&self.instances.get(&instance)?.variables.values)
    }

    /// The raw indices of the states this frame lists for the instance.
    #[cfg(test)]
    pub(crate) fn instance_states(&self, instance: u16) -> Option<BTreeSet<u16>> {
        Some(
            self.instances
                .get(&instance)?
                .states
                .keys()
                .copied()
                .collect(),
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ProjectedInstance {
    pub(super) graph: u32,
    pub(super) parent: Option<(u16, u16)>,
    pub(super) variables: ProjectedVariables,
    pub(super) states: BTreeMap<u16, ProjectedState>,
    pub(super) events: Vec<ProjectedEvent>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ProjectedState {
    pub(super) active: bool,
    pub(super) payload: StatePayload,
    pub(super) link: Option<ProjectedStateLink>,
    pub(super) volley: Option<VolleyActivation>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ProjectedStateLink {
    pub(super) instance: u16,
    pub(super) index: u16,
    pub(super) width: u8,
}

impl ProjectedState {
    pub(super) fn encode_payload(
        &self,
        codec: &Codec,
        mode: PayloadMode,
    ) -> Result<BitWriter, PayloadError> {
        let Some(link) = self.link else {
            return codec.payload(&self.payload, mode);
        };
        codec.payload_with_link(&self.payload, mode, |out, _| {
            out.bit(true);
            out.w_u16(link.instance);
            out.bits(u64::from(link.index), usize::from(link.width))?;
            Ok(())
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct VolleyActivation {
    pub(super) identity: (u16, u64), // activation counter and start time
    pub(super) ammo: Option<(Variable, i64)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct ProjectedVariables {
    pub(super) values: Variables,
    pub(super) bindings: Bindings,
}

impl ProjectedVariables {
    pub(super) fn write(&self, out: &mut BitWriter, owner: u16) -> Result<(), WireError> {
        script_wire::bound_variable_list(
            out,
            &self.values,
            ValueEncoding::Compact,
            owner,
            &self.bindings,
        )
    }

    pub(super) fn seed_ammo(&mut self, variable: u32, ammo: i64) {
        self.values.insert(variable, Value::Integer(ammo));
        self.bindings.remove(&variable);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ProjectedEvent {
    pub(super) due_ms: u64,
    pub(super) raw_state: u16,
    pub(super) finish: bool,
    pub(super) parameter: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StateSite {
    pub instance: u16,
    pub graph: u32,
    pub state: u16,
}

#[derive(Debug)]
pub enum SnapshotError {
    Incomplete {
        entity: u32,
        diagnostics: Vec<String>,
    },
    MissingBoundValue {
        owner: u16,
        variable: u32,
    },
    UnsupportedMap {
        owner: u16,
        variable: u32,
    },
    InvalidBinding {
        owner: u16,
        variable: u32,
        binding: StackBinding,
    },
    MissingGraph {
        instance: u16,
        graph: u32,
    },
    MissingState(StateSite),
    MissingStateIndex {
        instance: u16,
        graph: u32,
        node: u16,
    },
    UnsupportedState {
        site: StateSite,
        class: String,
        kind: StateKind,
    },
    UnsupportedTimer {
        site: StateSite,
        class: String,
    },
    Payload {
        site: StateSite,
        source: PayloadError,
    },
    DifferentStream,
    ReboundInstance(u16),
    FrameTime,
    Wire(WireError),
    Stream(StreamError),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Incomplete {
                entity,
                diagnostics,
            } => {
                write!(
                    f,
                    "body {entity:08X} has {} initialization diagnostics: {}",
                    diagnostics.len(),
                    diagnostics.join(", ")
                )
            }
            Self::MissingBoundValue { owner, variable } => {
                write!(
                    f,
                    "missing resolved value for bound variable {owner}:{variable}"
                )
            }
            Self::UnsupportedMap { owner, variable } => {
                write!(
                    f,
                    "variable {owner}:{variable} holds a map without a verified wire form"
                )
            }
            Self::InvalidBinding {
                owner,
                variable,
                binding,
            } => {
                write!(
                    f,
                    "invalid stack binding {binding:?} for variable {owner}:{variable}"
                )
            }
            Self::MissingGraph { instance, graph } => {
                write!(f, "instance {instance} has no graph {graph:04X}")
            }
            Self::MissingState(site) => write!(f, "missing statescript state {site:?}"),
            Self::MissingStateIndex {
                instance,
                graph,
                node,
            } => {
                write!(
                    f,
                    "instance {instance} graph {graph:04X} remote node {node} has no state index"
                )
            }
            Self::UnsupportedState { site, class, kind } => {
                write!(f, "unsupported active state {class} ({kind:?}) at {site:?}")
            }
            Self::UnsupportedTimer { site, class } => {
                write!(f, "unsupported owner timer for {class} at {site:?}")
            }
            Self::Payload { site, source } => write!(f, "statescript state {site:?}: {source}"),
            Self::DifferentStream => f.write_str(
                "statescript baseline belongs to a different body, view or variable scope",
            ),
            Self::ReboundInstance(id) => {
                write!(f, "statescript instance {id} changed its definition")
            }
            Self::FrameTime => f.write_str("statescript frame time is outside the supported range"),
            Self::Wire(error) => error.fmt(f),
            Self::Stream(error) => error.fmt(f),
        }
    }
}

impl Error for SnapshotError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Payload { source, .. } => Some(source),
            Self::Wire(error) => Some(error),
            Self::Stream(error) => Some(error),
            _ => None,
        }
    }
}

impl From<WireError> for SnapshotError {
    fn from(error: WireError) -> Self {
        Self::Wire(error)
    }
}

impl From<BitError> for SnapshotError {
    fn from(error: BitError) -> Self {
        Self::Wire(error.into())
    }
}

impl From<StreamError> for SnapshotError {
    fn from(error: StreamError) -> Self {
        Self::Stream(error)
    }
}

pub(crate) struct SceneProjection<'a> {
    pub snapshot: &'a Snapshot,
    pub encoder: SnapshotEncoder<'a>,
}

pub struct SnapshotEncoder<'a> {
    pub(super) graphs: &'a GraphStore,
    pub(super) codecs: &'a CodecTable,
}

impl<'a> SnapshotEncoder<'a> {
    pub fn new(graphs: &'a GraphStore, codecs: &'a CodecTable) -> Self {
        Self { graphs, codecs }
    }

    pub(super) fn graph(&self, instance: u16, graph: u32) -> Result<&Graph, SnapshotError> {
        self.graphs
            .graph(graph)
            .ok_or(SnapshotError::MissingGraph { instance, graph })
    }
}
