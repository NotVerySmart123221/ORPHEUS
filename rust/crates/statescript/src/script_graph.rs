//! State graph metadata for retail 1.74 and its owner/remote wire tables.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{self, BufReader, Read},
    num::NonZeroU32,
    path::Path,
};

use flate2::bufread::MultiGzDecoder;
use orpheus_runtime::format::{MAX_GRAPH_BYTES, TARGET_TAG_TYPE, parse_guid};
use serde::Deserialize;
use serde_json::{Map, Value};

use rulesets::{ModeRulesetDefinition, ModeRulesets};

use crate::script_expr::{
    StackPriority, Variable, authored_stack_priority, authored_stack_variable, native_fields,
};

mod rulesets;

const GRAPH_TYPE: u64 = 0x0580;
pub(crate) const MAX_ROOM_MESSAGE_PARAMETERS: usize = 16;

#[derive(Debug, Deserialize)]
pub struct Header {
    #[serde(rename = "m_nodesBitCount")]
    pub nodes_bits: u8,
    #[serde(rename = "m_statesBitCount")]
    pub states_bits: u8,
    #[serde(rename = "m_remoteSyncNodesBitCount")]
    pub remote_bits: u8,
    #[serde(rename = "m_syncVarsBitCount")]
    pub sync_vars_bits: u8,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum StateBehavior {
    Plain,
    Observe,
    Interval,
    ButtonLock,
    MovementScale,
    StackClaim,
    TrackEntities,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ActionBehavior {
    Forward,
    Branch,
    LocalMessage,
    RoomMessage,
    RoomCue,
    HealthChange,
    RadialHealth,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HostEventKind {
    HealthApplied,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum EntryTopic {
    Message(u64),
    Host(HostEventKind),
}

#[derive(Debug, Deserialize)]
pub struct Node {
    pub pos: u16,
    pub class: String,
    pub uid: u32,
    pub rid: u16,
    pub remote: i32,
    pub state: Option<u16>,
    state_behavior: Option<StateBehavior>,
    action_behavior: Option<ActionBehavior>,
    #[serde(default)]
    room_message_entry: bool,
    host_event_entry: Option<HostEventKind>,
    #[serde(deserialize_with = "node_flag")]
    pub client_only: bool,
    #[serde(deserialize_with = "node_flag")]
    pub server_only: bool,
    #[serde(skip)]
    runtime_plain_state: bool,
    #[serde(skip)]
    runtime_client_state: bool,
    pub fields: Value,
}

impl Node {
    /// Whether the node can run on the server.
    pub(crate) fn runs_on_server(&self) -> bool {
        !self.client_only && !self.runtime_client_state
    }

    pub fn is_networked(&self) -> bool {
        !self.client_only && !self.server_only
    }

    pub fn is_state(&self) -> bool {
        self.state_behavior.is_some() || self.fields.get("m_2BBEEAB8").is_some_and(truthy)
    }

    /// Whether authored metadata opts this node into the ordinary state lifecycle.
    pub fn declares_plain_state(&self) -> bool {
        matches!(self.state_behavior, Some(StateBehavior::Plain))
    }

    pub(crate) fn declares_runtime_plain_state(&self) -> bool {
        self.runtime_plain_state
    }

    pub(crate) fn declares_observe_state(&self) -> bool {
        matches!(self.state_behavior, Some(StateBehavior::Observe))
    }

    pub(crate) fn interval_frames(&self) -> Option<NonZeroU32> {
        if self.state_behavior != Some(StateBehavior::Interval) {
            return None;
        }
        let frames = self.field("interval_frames")?.as_u64()?;
        NonZeroU32::new(u32::try_from(frames).ok()?)
    }

    pub(crate) fn button_lock(&self) -> Option<(u8, Option<NonZeroU32>)> {
        if self.state_behavior != Some(StateBehavior::ButtonLock) {
            return None;
        }
        let button = u8::try_from(self.field("button")?.as_u64()?).ok()?;
        if button == 0 {
            return None;
        }
        Some((button, self.optional_duration_frames()?))
    }

    pub(crate) fn movement_scale(&self) -> Option<(f64, Option<NonZeroU32>)> {
        if self.state_behavior != Some(StateBehavior::MovementScale) {
            return None;
        }
        let factor = self.field("factor")?.as_f64()?;
        if !(0.0..=1.0).contains(&factor) {
            return None;
        }
        Some((factor, self.optional_duration_frames()?))
    }

    pub(crate) fn declares_stack_claim(&self) -> bool {
        self.state_behavior == Some(StateBehavior::StackClaim)
    }

    pub(crate) fn stack_claim(&self) -> Option<(Variable, StackPriority, Option<NonZeroU32>)> {
        if !self.declares_stack_claim() {
            return None;
        }
        let variable = authored_stack_variable(self.field("variable")?).ok()?;
        let priority = authored_stack_priority(self.field("priority")?).ok()?;
        Some((variable, priority, self.optional_duration_frames()?))
    }

    pub(crate) fn stack_value(&self) -> Option<&Value> {
        if self.declares_stack_claim() {
            self.field("value")
        } else {
            self.field("m_value")
        }
    }

    fn optional_duration_frames(&self) -> Option<Option<NonZeroU32>> {
        match self.field("duration_frames") {
            Some(value) => NonZeroU32::new(u32::try_from(value.as_u64()?).ok()?).map(Some),
            None => Some(None),
        }
    }

    pub(crate) fn target_child_binding(&self) -> Option<(u32, u32)> {
        if self.state_behavior != Some(StateBehavior::TrackEntities) {
            return None;
        }
        let graph = u32::try_from(self.field("child_graph")?.as_u64()?).ok()?;
        let variable = u32::try_from(self.field("target_variable")?.as_u64()?).ok()?;
        (variable != 0).then_some((graph, variable))
    }

    pub fn is_action(&self) -> bool {
        self.action_behavior.is_some() || self.fields.get("m_ADEB6E05").is_some_and(truthy)
    }

    pub(crate) fn declares_forward_action(&self) -> bool {
        matches!(self.action_behavior, Some(ActionBehavior::Forward))
    }

    pub(crate) fn declares_branch_action(&self) -> bool {
        matches!(self.action_behavior, Some(ActionBehavior::Branch))
    }

    pub(crate) fn declares_room_message_action(&self) -> bool {
        self.action_behavior == Some(ActionBehavior::RoomMessage)
    }

    pub(crate) fn declares_room_cue_action(&self) -> bool {
        self.action_behavior == Some(ActionBehavior::RoomCue)
    }

    pub(crate) fn room_cue_assets(&self) -> Option<(u64, u64)> {
        if !self.declares_room_cue_action() {
            return None;
        }
        let stimulus = asset_guid(self.field("stimulus")?)?;
        let line = asset_guid(self.field("line")?)?;
        (stimulus != 0 && line != 0).then_some((stimulus, line))
    }

    pub(crate) fn declares_room_message_entry(&self) -> bool {
        self.room_message_entry
    }

    pub(crate) fn host_event_kind(&self) -> Option<HostEventKind> {
        self.host_event_entry
    }

    fn valid_host_event_parameters(&self, kind: HostEventKind) -> bool {
        let slots = match kind {
            HostEventKind::HealthApplied => 1..=4,
        };
        let Some(parameters) = self.fields.get("m_params").filter(|value| !value.is_null()) else {
            return true;
        };
        let mut bound = BTreeSet::new();
        parameters.as_array().is_some_and(|parameters| {
            parameters.len() <= 4
                && parameters.iter().all(|parameter| {
                    let slot = message_parameter_id(parameter);
                    slots.contains(&slot)
                        && bound.insert(slot)
                        && parameter.get("m_out_Var").is_some_and(|output| {
                            crate::script_expr::lvalue(output)
                                .ok()
                                .flatten()
                                .is_some_and(|variable| variable.id != 0)
                        })
                })
        })
    }

    pub(crate) fn declares_health_change_action(&self) -> bool {
        matches!(self.action_behavior, Some(ActionBehavior::HealthChange))
    }

    pub(crate) fn declares_radial_health_action(&self) -> bool {
        matches!(self.action_behavior, Some(ActionBehavior::RadialHealth))
    }

    pub(crate) fn game_message_id(&self) -> Option<u64> {
        self.field("m_gameMessage.m_gameMessage")
            .and_then(asset_guid)
    }

    pub(crate) fn literal_game_message_id(&self) -> Option<u64> {
        let message = self.field("m_gameMessage")?;
        if !native_fields(message, &["m_gameMessage", "m_resourceKey"])
            || message
                .get("$")
                .is_some_and(|class| class.as_str() != Some("STUConfigVarGameMessage"))
            || message
                .get("m_resourceKey")
                .is_some_and(|key| !key.is_null())
        {
            return None;
        }
        self.game_message_id().filter(|&id| id != 0)
    }

    pub(crate) fn is_message_entry(&self) -> bool {
        self.class == "STUStatescriptEntryGameMessage"
    }

    fn valid_local_message(&self) -> bool {
        self.game_message_id().is_some_and(|message| message != 0)
            && self.fields.get("m_targets").is_none_or(Value::is_null)
            && self.fields.get("m_target").is_none_or(Value::is_null)
            && self.valid_message_parameters(usize::MAX)
    }

    fn valid_room_message(&self) -> bool {
        self.game_message_id().is_some_and(|message| message != 0)
            && self.fields.get("m_targets").is_none()
            && self
                .fields
                .get("m_target")
                .is_some_and(|target| !target.is_null())
            && self.valid_message_parameters(MAX_ROOM_MESSAGE_PARAMETERS)
    }

    pub(crate) fn valid_message_parameters(&self, max_parameters: usize) -> bool {
        let Some(parameters) = self.fields.get("m_params").filter(|value| !value.is_null()) else {
            return true;
        };
        let mut slots = BTreeSet::new();
        parameters.as_array().is_some_and(|parameters| {
            parameters.len() <= max_parameters
                && parameters.iter().all(|parameter| {
                    let slot = message_parameter_id(parameter);
                    slot != 0 && parameter.get("m_value").is_some() && slots.insert(slot)
                })
        })
    }

    pub fn field(&self, path: &str) -> Option<&Value> {
        field_at(&self.fields, path)
    }

    /// The plug's targets in data order, including unresolved targets.
    pub fn links(&self, path: &str) -> impl Iterator<Item = (Option<u16>, &str)> {
        self.field(path)
            .into_iter()
            .flat_map(raw_plug_links)
            .filter_map(decode_link)
    }

    pub fn plugs(&self) -> BTreeMap<String, &Value> {
        let mut found = BTreeMap::new();
        collect_plugs(&self.fields, "", &mut found);
        found
    }
}

fn raw_plug_links(field: &Value) -> impl Iterator<Item = &Value> {
    field
        .get("$")
        .and_then(|_| field.get("links")?.as_array())
        .into_iter()
        .flatten()
}

fn valid_output_plug(plug: &Value) -> bool {
    plug.get("$").and_then(Value::as_str).is_some()
        && plug
            .get("links")
            .and_then(Value::as_array)
            .is_some_and(|links| links.iter().all(|link| decode_link(link).is_some()))
}

fn decode_link(link: &Value) -> Option<(Option<u16>, &str)> {
    let pair = link.as_array()?;
    let position = match pair.first()? {
        Value::Null => None,
        value => Some(u16::try_from(value.as_u64()?).ok()?),
    };
    Some((position, pair.get(1)?.as_str()?))
}

/// A decoded link to a position absent from both the node and container tables.
#[derive(Debug, Eq, PartialEq)]
pub struct DanglingLink {
    pub graph: u32,
    pub source: u16,
    pub plug: String,
    /// Zero-based position in the plug's original link list.
    pub link_index: usize,
    pub target: u16,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub struct SyncVar(pub Option<u32>, pub u8, pub u8);

impl SyncVar {
    pub fn has_presence_bit(self) -> bool {
        self.0.is_some() && self.1 == 0
    }
}

#[derive(Debug, Default)]
struct WireSlots {
    by_list_index: Vec<Option<usize>>,
    positions: Vec<u16>,
}

impl WireSlots {
    fn build(order: &[Option<u16>], graph: &Graph, require_state: bool) -> Self {
        let mut positions = Vec::new();
        let mut first_slot = vec![None; graph.by_pos.len()];
        let by_list_index = order
            .iter()
            .map(|&position| {
                let node = graph.node(position?)?;
                if !node.is_networked() || (require_state && !node.is_state()) {
                    return None;
                }
                let slot = *first_slot[usize::from(node.pos)].get_or_insert(positions.len());
                positions.push(node.pos);
                Some(slot)
            })
            .collect();
        Self {
            by_list_index,
            positions,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Graph {
    pub index: u32,
    pub header: Header,
    pub fields: Value,
    pub items: u16,
    pub containers: BTreeMap<u16, String>,
    pub nodes: Vec<Node>,
    #[serde(rename = "unknown_nodes")]
    pub unknown_rids: Vec<u16>,
    pub states: Vec<Option<u16>>,
    pub entries: Vec<Option<u16>>,
    #[serde(rename = "remote")]
    pub remote_nodes: Vec<Option<u16>>,
    pub sync_vars: Vec<SyncVar>,
    #[serde(skip)]
    by_pos: Vec<Option<usize>>,
    #[serde(skip)]
    by_rid: Vec<Option<usize>>,
    #[serde(skip)]
    entry_handlers: BTreeMap<EntryTopic, Vec<usize>>,
    #[serde(skip)]
    owner: WireSlots,
    #[serde(skip)]
    remote: WireSlots,
    #[serde(skip)]
    presence: Vec<usize>,
}

impl Graph {
    fn apply_runtime_state_metadata(
        &mut self,
        plain_classes: &BTreeSet<String>,
        mut client_states: BTreeMap<u16, RuntimeClientState>,
    ) -> io::Result<()> {
        if plain_classes.is_empty() && client_states.is_empty() {
            return Ok(());
        }
        for node in &mut self.nodes {
            let Some(slot) = node.state.filter(|&slot| {
                self.states.get(usize::from(slot)).copied().flatten() == Some(node.pos)
            }) else {
                continue;
            };
            let authored = node.class.starts_with("authored.")
                || node.state_behavior.is_some()
                || node.action_behavior.is_some()
                || node.declares_room_message_entry()
                || node.host_event_kind().is_some();
            if let Some(state) = client_states.remove(&slot) {
                if node.rid != state.rid
                    || node.class != state.class
                    || !node.is_state()
                    || node.server_only
                    || authored
                {
                    return Err(invalid(format!(
                        "runtime client state in graph {} at slot {slot} does not match an eligible state",
                        self.index
                    )));
                }
                node.runtime_client_state = true;
            }
            node.runtime_plain_state = !authored
                && node.runs_on_server()
                && node.is_state()
                && plain_classes.contains(&node.class);
        }
        if let Some((slot, _)) = client_states.first_key_value() {
            return Err(invalid(format!(
                "runtime client state in graph {} at slot {slot} has no matching state",
                self.index
            )));
        }
        Ok(())
    }

    fn compile(&mut self) -> io::Result<()> {
        self.by_pos = vec![None; usize::from(self.items)];
        if let Some(max) = self.nodes.iter().map(|node| node.rid).max() {
            self.by_rid = vec![None; usize::from(max) + 1];
        }
        let mut authored_state_slots = BTreeMap::new();
        if self.nodes.iter().any(|node| {
            node.state_behavior.is_some()
                || node.action_behavior.is_some()
                || node.declares_room_message_entry()
                || node.host_event_kind().is_some()
        }) {
            for &position in self.states.iter().flatten() {
                *authored_state_slots.entry(position).or_insert(0_usize) += 1;
            }
        }
        for (index, node) in self.nodes.iter_mut().enumerate() {
            if let Some(behavior) = node.state_behavior {
                let state = match behavior {
                    StateBehavior::Plain => "plain state",
                    StateBehavior::Observe => "observe state",
                    StateBehavior::Interval => "interval state",
                    StateBehavior::ButtonLock => "button lock state",
                    StateBehavior::MovementScale => "movement scale state",
                    StateBehavior::StackClaim => "stack claim state",
                    StateBehavior::TrackEntities => "track entities state",
                };
                let occupies_state_slot = node
                    .state
                    .and_then(|slot| self.states.get(usize::from(slot)))
                    .is_some_and(|&position| position == Some(node.pos));
                if !node.server_only
                    || !node.runs_on_server()
                    || node.is_action()
                    || !occupies_state_slot
                    || authored_state_slots.get(&node.pos) != Some(&1)
                {
                    return Err(invalid(format!(
                        "{state} in graph {} at node {} must be server-only and occupy its state slot exactly once",
                        self.index, node.pos
                    )));
                }
                let stack_outputs: &[&str] = if behavior == StateBehavior::StackClaim {
                    &["m_topSubgraphPlug", "m_underSubgraphPlug"]
                } else {
                    &[]
                };
                for path in [
                    "m_onBeginPlug",
                    "m_onEndPlug",
                    "m_onAbortPlug",
                    "m_onFinishedPlug",
                ]
                .into_iter()
                .chain(node.declares_observe_state().then_some("on_change"))
                .chain(stack_outputs.iter().copied())
                {
                    if node
                        .field(path)
                        .is_some_and(|plug| !plug.is_null() && !valid_output_plug(plug))
                    {
                        return Err(invalid(format!(
                            "{state} in graph {} at node {} has an invalid {path} output plug",
                            self.index, node.pos
                        )));
                    }
                }
                node.class = match behavior {
                    // Class checks outside the main dispatch also use the ordinary state.
                    StateBehavior::Plain => "STUStatescriptState",
                    StateBehavior::Observe => {
                        let has_value = node.field("value").is_some_and(|value| !value.is_null());
                        let has_output = node
                            .field("output")
                            .is_some_and(|output| matches!(crate::script_expr::lvalue(output), Ok(Some(_))));
                        if !has_value || !has_output {
                            return Err(invalid(format!(
                                "observe state in graph {} at node {} requires a value expression and writable output",
                                self.index, node.pos
                            )));
                        }
                        "StateObserve"
                    }
                    StateBehavior::Interval => {
                        if node.interval_frames().is_none() {
                            return Err(invalid(format!(
                                "{state} in graph {} at node {} requires a nonzero u32 interval_frames",
                                self.index, node.pos
                            )));
                        }
                        if !node.field("on_interval").is_some_and(valid_output_plug) {
                            return Err(invalid(format!(
                                "{state} in graph {} at node {} requires a valid on_interval output plug",
                                self.index, node.pos
                            )));
                        }
                        "StateInterval"
                    }
                    StateBehavior::ButtonLock => {
                        if node.button_lock().is_none() {
                            return Err(invalid(format!(
                                "{state} in graph {} at node {} requires an integer button in 1..=255 and an optional nonzero u32 duration_frames",
                                self.index, node.pos
                            )));
                        }
                        "StateButtonLock"
                    }
                    StateBehavior::MovementScale => {
                        if node.movement_scale().is_none() {
                            return Err(invalid(format!(
                                "{state} in graph {} at node {} requires a finite factor in 0..=1 and an optional nonzero u32 duration_frames",
                                self.index, node.pos
                            )));
                        }
                        "StateMovementScale"
                    }
                    StateBehavior::StackClaim => {
                        if node.stack_claim().is_none() || node.stack_value().is_none() {
                            return Err(invalid(format!(
                                "{state} in graph {} at node {} requires a valid variable, priority and value, with an optional nonzero u32 duration_frames",
                                self.index, node.pos
                            )));
                        }
                        "StateStackClaim"
                    }
                    StateBehavior::TrackEntities => {
                        if node.field("targets").is_none_or(Value::is_null) {
                            return Err(invalid(format!(
                                "{state} in graph {} at node {} requires a targets expression",
                                self.index, node.pos
                            )));
                        }
                        if node.target_child_binding().is_none() {
                            return Err(invalid(format!(
                                "{state} in graph {} at node {} requires a u32 child_graph and nonzero u32 target_variable",
                                self.index, node.pos
                            )));
                        }
                        "StateTrackEntities"
                    }
                }
                .into();
            }
            if let Some(behavior) = node.action_behavior {
                let (action, outputs): (&str, &[&str]) = match behavior {
                    ActionBehavior::Forward => ("forward action", &["m_outPlug"]),
                    ActionBehavior::Branch => ("branch action", &["m_truePlug", "m_falsePlug"]),
                    ActionBehavior::LocalMessage => ("local message action", &["m_outPlug"]),
                    ActionBehavior::RoomMessage => ("room message action", &["m_outPlug"]),
                    ActionBehavior::RoomCue => ("room cue action", &["m_outPlug"]),
                    ActionBehavior::HealthChange => ("health change action", &["m_outPlug"]),
                    ActionBehavior::RadialHealth => ("radial health action", &["m_outPlug"]),
                };
                let state_slot = authored_state_slots.contains_key(&node.pos);
                if !node.server_only
                    || !node.runs_on_server()
                    || node.is_state()
                    || node.state.is_some()
                    || state_slot
                {
                    return Err(invalid(format!(
                        "{action} in graph {} at node {} must be server-only with no state slot",
                        self.index, node.pos
                    )));
                }
                for &path in outputs {
                    if !node.field(path).is_some_and(valid_output_plug) {
                        return Err(invalid(format!(
                            "{action} in graph {} at node {} requires a valid {path} output plug",
                            self.index, node.pos
                        )));
                    }
                }
                // The declaration supplies dispatch. An authored name that
                // matches an entry or another special class cannot activate it.
                node.class = match behavior {
                    ActionBehavior::Forward => "ActionForward".into(),
                    ActionBehavior::Branch => {
                        if node.field("m_condition").is_none_or(Value::is_null) {
                            return Err(invalid(format!(
                                "{action} in graph {} at node {} requires m_condition",
                                self.index, node.pos
                            )));
                        }
                        "ActionBranch".into()
                    }
                    ActionBehavior::LocalMessage => {
                        if !node.valid_local_message() {
                            return Err(invalid(format!(
                                "{action} in graph {} at node {} requires a nonzero message, unique parameter slots and no external target",
                                self.index, node.pos
                            )));
                        }
                        "STUStatescriptActionSendGameMessage".into()
                    }
                    ActionBehavior::RoomMessage => {
                        if !node.valid_room_message() {
                            return Err(invalid(format!(
                                "{action} in graph {} at node {} requires a nonzero message, one explicit m_target and at most {MAX_ROOM_MESSAGE_PARAMETERS} unique parameter slots",
                                self.index, node.pos
                            )));
                        }
                        "ActionRoomMessage".into()
                    }
                    ActionBehavior::RoomCue => {
                        if node.field("audience").is_none_or(Value::is_null) {
                            return Err(invalid(format!(
                                "{action} in graph {} at node {} requires an audience expression",
                                self.index, node.pos
                            )));
                        }
                        if node.room_cue_assets().is_none() {
                            return Err(invalid(format!(
                                "{action} in graph {} at node {} requires nonzero stimulus and line asset GUIDs",
                                self.index, node.pos
                            )));
                        }
                        "ActionRoomCue".into()
                    }
                    ActionBehavior::HealthChange => {
                        for operand in ["target", "delta"] {
                            if node.field(operand).is_none_or(Value::is_null) {
                                return Err(invalid(format!(
                                    "{action} in graph {} at node {} requires a {operand} expression",
                                    self.index, node.pos
                                )));
                            }
                        }
                        "ActionHealthChange".into()
                    }
                    ActionBehavior::RadialHealth => {
                        if node
                            .field("area")
                            .and_then(|area| area.get("$"))
                            .and_then(Value::as_str)
                            != Some("EntitiesInRadius")
                            || node.field("delta").is_none_or(Value::is_null)
                            || node.field("edge_scale").is_some_and(Value::is_null)
                        {
                            return Err(invalid(format!(
                                "{action} in graph {} at node {} requires an EntitiesInRadius area and a delta expression, with an optional edge_scale expression",
                                self.index, node.pos
                            )));
                        }
                        "ActionRadialHealth".into()
                    }
                };
            }
            if node.declares_room_message_entry()
                && (!node.server_only
                    || !node.runs_on_server()
                    || !node.is_message_entry()
                    || node.is_action()
                    || node.is_state()
                    || node.state.is_some()
                    || authored_state_slots.contains_key(&node.pos)
                    || node.game_message_id().is_none_or(|message| message == 0)
                    || !node.field("m_outPlug").is_some_and(valid_output_plug))
            {
                return Err(invalid(format!(
                    "room message entry in graph {} at node {} must be a server-only message entry with a nonzero message and a valid m_outPlug",
                    self.index, node.pos
                )));
            }
            if (node.host_event_kind().is_some() || node.class == "EntryHostEvent")
                && (node.class != "EntryHostEvent"
                    || !node.server_only
                    || !node.runs_on_server()
                    || node.is_action()
                    || node.is_state()
                    || node.state.is_some()
                    || authored_state_slots.contains_key(&node.pos)
                    || node.declares_room_message_entry()
                    || node.fields.get("m_gameMessage").is_some()
                    || !node.field("m_outPlug").is_some_and(valid_output_plug)
                    || !node
                        .host_event_kind()
                        .is_some_and(|kind| node.valid_host_event_parameters(kind)))
            {
                return Err(invalid(format!(
                    "host event entry in graph {} at node {} must be a standalone server-only EntryHostEvent with a valid host event kind, m_outPlug and parameter bindings",
                    self.index, node.pos
                )));
            }
            let position = self
                .by_pos
                .get_mut(usize::from(node.pos))
                .ok_or_else(|| invalid("graph node outside the item table"))?;
            if position.replace(index).is_some() {
                return Err(invalid(format!(
                    "graph {} defines node position {} more than once",
                    self.index, node.pos
                )));
            }
            self.by_rid[usize::from(node.rid)] = Some(index);
            let topic = node.host_event_kind().map(EntryTopic::Host).or_else(|| {
                node.game_message_id()
                    .filter(|_| {
                        node.runs_on_server()
                            && (node.is_message_entry()
                                || (node.is_state() && node.state.is_some()))
                    })
                    .map(EntryTopic::Message)
            });
            if let Some(topic) = topic {
                self.entry_handlers.entry(topic).or_default().push(index);
            }
        }
        self.owner = WireSlots::build(&self.states, self, false);
        self.remote = WireSlots::build(&self.remote_nodes, self, true);
        self.presence = self
            .sync_vars
            .iter()
            .enumerate()
            .filter_map(|(index, var)| var.has_presence_bit().then_some(index))
            .collect();
        Ok(())
    }

    pub(crate) fn entry_handlers(&self, topic: EntryTopic) -> impl Iterator<Item = &Node> {
        self.entry_handlers
            .get(&topic)
            .into_iter()
            .flatten()
            .map(|&index| &self.nodes[index])
    }

    pub fn node(&self, pos: u16) -> Option<&Node> {
        self.nodes.get((*self.by_pos.get(usize::from(pos))?)?)
    }

    pub fn by_rid(&self, rid: u16) -> Option<&Node> {
        self.nodes.get((*self.by_rid.get(usize::from(rid))?)?)
    }

    pub fn state(&self, index: usize) -> Option<&Node> {
        self.node((*self.states.get(index)?)?)
    }

    pub fn remote_node(&self, index: usize) -> Option<&Node> {
        self.node((*self.remote_nodes.get(index)?)?)
    }

    pub fn node_ref(&self, value: &Value) -> Option<&Node> {
        self.node(u16::try_from(value.get("node")?.as_u64()?).ok()?)
    }

    pub fn owner_positions(&self) -> &[u16] {
        &self.owner.positions
    }

    pub fn remote_positions(&self) -> &[u16] {
        &self.remote.positions
    }

    pub fn owner_slot(&self, index: usize) -> Option<usize> {
        *self.owner.by_list_index.get(index)?
    }

    pub fn remote_slot(&self, index: usize) -> Option<usize> {
        *self.remote.by_list_index.get(index)?
    }

    pub fn presence_vars(&self) -> impl Iterator<Item = (usize, SyncVar)> + '_ {
        self.presence
            .iter()
            .map(|&index| (index, self.sync_vars[index]))
    }

    pub fn presence_bit(&self, var: u32) -> Option<usize> {
        self.presence_vars()
            .position(|(_, entry)| entry.0 == Some(var))
    }

    fn dangling_links(&self) -> impl Iterator<Item = DanglingLink> + '_ {
        self.nodes.iter().flat_map(move |node| {
            node.plugs().into_iter().flat_map(move |(path, plug)| {
                raw_plug_links(plug)
                    .enumerate()
                    .filter_map(move |(link_index, link)| {
                        let (target, _) = decode_link(link)?;
                        let target = target?;
                        if self.node(target).is_some() || self.containers.contains_key(&target) {
                            return None;
                        }
                        Some(DanglingLink {
                            graph: self.index,
                            source: node.pos,
                            plug: path.clone(),
                            link_index,
                            target,
                        })
                    })
            })
        })
    }

    pub fn graph_refs(&self) -> BTreeSet<u64> {
        let mut found = BTreeSet::new();
        collect_graphs(&self.fields, &mut found);
        for node in &self.nodes {
            collect_graphs(&node.fields, &mut found);
        }
        found
    }
}

#[derive(Debug, Deserialize)]
pub struct GraphBinding {
    pub graph: Option<u32>,
    #[serde(rename = "m_1EB5A024")]
    pub sync: Option<Vec<Value>>,
    #[serde(rename = "m_75408640")]
    pub flags: i64,
}

#[derive(Debug, Deserialize)]
pub struct Body {
    pub hero: Option<String>,
    pub name: String,
    pub graphs: Vec<GraphBinding>,
    /// Server roots mounted without consuming the body's projected IDs.
    #[serde(default)]
    pub server_graphs: Vec<GraphBinding>,
    pub manager: Option<u32>,
    pub weapons: Vec<Option<u32>>,
    #[serde(default)]
    pub ai_weapon_buttons: Map<String, Value>,
    #[serde(default)]
    pub ai_association: Option<u64>,
    #[serde(default)]
    pub ai_weapon_identifiers: Map<String, Value>,
    /// None means the local entity definition was unavailable.
    #[serde(default)]
    pub component92_present: Option<bool>,
    /// An omitted list means the source definition was unavailable.
    #[serde(default, deserialize_with = "body_target_tags")]
    pub target_tags: Option<BTreeSet<u64>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModeValue {
    Bool(bool),
    Integer(i64),
    Asset(u64),
}

#[derive(Debug, Deserialize)]
pub struct ModeScripts {
    pub pve: bool,
    pub scripted_loading: bool,
    #[serde(default, deserialize_with = "optional_graph_index")]
    pub graph: Option<u32>,
    #[serde(default, deserialize_with = "optional_graph_index")]
    pub shared_graph: Option<u32>,
    pub teams: BTreeMap<u8, TeamScripts>,
}

#[derive(Debug, Deserialize)]
pub struct TeamScripts {
    #[serde(default, deserialize_with = "optional_graph_index")]
    pub controller: Option<u32>,
    #[serde(default, deserialize_with = "optional_graph_index")]
    pub body: Option<u32>,
}

fn optional_graph_index<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u32>, D::Error> {
    Option::<Value>::deserialize(deserializer)?
        .map(|value| {
            asset_guid(&value)
                .filter(|guid| guid >> 48 == GRAPH_TYPE)
                .and_then(|guid| u32::try_from(guid & 0x0000_ffff_ffff_ffff).ok())
                .ok_or_else(|| serde::de::Error::custom("invalid mode script graph GUID"))
        })
        .transpose()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnemyPair {
    pub body: u64,
    pub controller: u64,
}

#[derive(Debug, Deserialize)]
struct RawEnemyPair {
    body: String,
    controller: String,
}

#[derive(Debug, Default, Deserialize)]
struct EnemyCatalog {
    #[serde(default)]
    pairs: BTreeMap<String, RawEnemyPair>,
    #[serde(default)]
    entities: BTreeMap<String, Body>,
}

impl EnemyCatalog {
    fn validate(mut self) -> io::Result<Self> {
        let asset = |text: &str, kind| {
            parse_guid(text).filter(|guid| guid >> 48 == kind && guid & 0x0000_ffff_ffff_ffff != 0)
        };
        for (key, body) in std::mem::take(&mut self.entities) {
            let guid = asset(&key, 0x0400)
                .ok_or_else(|| invalid("invalid Junkenstein entity definition"))?;
            if self
                .entities
                .insert(format!("0x{guid:016X}"), body)
                .is_some()
            {
                return Err(invalid("duplicate Junkenstein entity definition"));
            }
        }
        for (hero, mut pair) in std::mem::take(&mut self.pairs) {
            if asset(&hero, 0x02e0).is_none()
                || asset(&pair.body, 0x0400).is_none()
                || asset(&pair.controller, 0x0400).is_none()
                || parse_guid(&pair.body) == parse_guid(&pair.controller)
            {
                return Err(invalid("invalid Junkenstein body/controller pair"));
            }
            pair.body = format!("0x{:016X}", parse_guid(&pair.body).unwrap());
            pair.controller = format!("0x{:016X}", parse_guid(&pair.controller).unwrap());
            let body = self
                .entities
                .get(&pair.body)
                .ok_or_else(|| invalid("missing Junkenstein enemy body"))?;
            let controller = self
                .entities
                .get(&pair.controller)
                .ok_or_else(|| invalid("missing Junkenstein enemy controller"))?;
            if body.hero.as_deref().and_then(parse_guid) != parse_guid(&hero)
                || body
                    .ai_association
                    .is_some_and(|association| Some(association) != parse_guid(&pair.controller))
                || body.graphs.is_empty()
                || controller.graphs.is_empty()
            {
                return Err(invalid("inconsistent Junkenstein body/controller binding"));
            }
            if self
                .pairs
                .insert(format!("0x{:016X}", parse_guid(&hero).unwrap()), pair)
                .is_some()
            {
                return Err(invalid("duplicate Junkenstein hero pair"));
            }
        }
        Ok(self)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthoredClass {
    class: String,
    behavior: AuthoredClassBehavior,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AuthoredClassBehavior {
    PlainState,
    ObserveState,
    IntervalState,
    ButtonLockState,
    MovementScaleState,
    StackClaimState,
    TrackEntitiesState,
    ForwardAction,
    BranchAction,
    LocalMessageAction,
    RoomMessageAction,
    RoomCueAction,
    HealthChangeAction,
    RadialHealthAction,
    ClientOnly,
}

impl AuthoredClassBehavior {
    fn apply(self, graph: u32, node: &mut Node) -> io::Result<()> {
        let (client_only, state, action) = match self {
            Self::PlainState => (false, Some(StateBehavior::Plain), None),
            Self::ObserveState => (false, Some(StateBehavior::Observe), None),
            Self::IntervalState => (false, Some(StateBehavior::Interval), None),
            Self::ButtonLockState => (false, Some(StateBehavior::ButtonLock), None),
            Self::MovementScaleState => (false, Some(StateBehavior::MovementScale), None),
            Self::StackClaimState => (false, Some(StateBehavior::StackClaim), None),
            Self::TrackEntitiesState => (false, Some(StateBehavior::TrackEntities), None),
            Self::ForwardAction => (false, None, Some(ActionBehavior::Forward)),
            Self::BranchAction => (false, None, Some(ActionBehavior::Branch)),
            Self::LocalMessageAction => (false, None, Some(ActionBehavior::LocalMessage)),
            Self::RoomMessageAction => (false, None, Some(ActionBehavior::RoomMessage)),
            Self::RoomCueAction => (false, None, Some(ActionBehavior::RoomCue)),
            Self::HealthChangeAction => (false, None, Some(ActionBehavior::HealthChange)),
            Self::RadialHealthAction => (false, None, Some(ActionBehavior::RadialHealth)),
            Self::ClientOnly => (true, None, None),
        };
        if (node.client_only && !client_only)
            || (node.server_only && client_only)
            || (node.state_behavior.is_some() && node.state_behavior != state)
            || (node.action_behavior.is_some() && node.action_behavior != action)
        {
            return Err(invalid(format!(
                "authored class {} conflicts with scope or behavior in graph {graph} at node {}",
                node.class, node.pos
            )));
        }
        node.client_only = client_only;
        node.server_only = !client_only;
        node.state_behavior = state;
        node.action_behavior = action;
        Ok(())
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeStateMetadata {
    #[serde(default)]
    plain_classes: Vec<String>,
    #[serde(default)]
    client_states: Vec<RuntimeClientState>,
    #[serde(default)]
    projected_message_topics: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeClientState {
    graph: u32,
    state: u16,
    rid: u16,
    class: String,
}

fn valid_runtime_class_name(class: &str) -> bool {
    (1..=128).contains(&class.len())
        && class.as_bytes()[0].is_ascii_alphabetic()
        && class
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn validate_projected_message_topics(
    declarations: Vec<String>,
    graphs: &BTreeMap<u32, Graph>,
) -> io::Result<BTreeSet<u64>> {
    let mut topics = BTreeSet::new();
    for declaration in declarations {
        let id = asset_guid(&Value::String(declaration))
            .filter(|&id| id != 0)
            .ok_or_else(|| invalid("invalid projected message topic"))?;
        if !topics.insert(id) {
            return Err(invalid(format!(
                "duplicate projected message topic {id:#018X}"
            )));
        }
    }
    if topics.is_empty() {
        return Ok(topics);
    }
    let mut unmatched = topics.clone();
    for graph in graphs.values() {
        for node in &graph.nodes {
            let stateful = node.class == "STUStatescriptStateGameMessageEntry";
            if !stateful && !node.is_message_entry() {
                continue;
            }
            let Some(id) = node.game_message_id().filter(|id| topics.contains(id)) else {
                continue;
            };
            if node.runs_on_server() || node.server_only {
                return Err(invalid(format!(
                    "projected message topic {id:#018X} has a server receiver in graph {} at node {}",
                    graph.index, node.pos
                )));
            }
            let valid_receiver = if stateful {
                node.is_state()
                    && node
                        .state
                        .and_then(|slot| graph.state(usize::from(slot)))
                        .is_some_and(|state| state.pos == node.pos)
            } else {
                node.state.is_none() && !node.is_state()
            };
            if valid_receiver && !node.is_action() && node.literal_game_message_id() == Some(id) {
                unmatched.remove(&id);
            }
        }
    }
    if let Some(id) = unmatched.first() {
        return Err(invalid(format!(
            "projected message topic {id:#018X} has no client receiver"
        )));
    }
    Ok(topics)
}

#[derive(Deserialize)]
struct Bundle {
    version: u32,
    #[serde(default)]
    authored_classes: Vec<AuthoredClass>,
    #[serde(default)]
    runtime_state_metadata: RuntimeStateMetadata,
    graphs: Vec<Graph>,
    bodies: BTreeMap<String, Body>,
    #[serde(default)]
    entities: BTreeMap<String, Body>,
    #[serde(default)]
    mode_variables: BTreeMap<String, BTreeMap<u32, Value>>,
    #[serde(default)]
    mode_scripts: BTreeMap<String, ModeScripts>,
    #[serde(default)]
    mode_rulesets: Vec<ModeRulesetDefinition>,
    #[serde(default)]
    junkenstein: EnemyCatalog,
    missing: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum RoomSpatialNeeds {
    #[default]
    None,
    Queries,
    EntityFacts,
    EntityTags,
}

#[derive(Clone, Copy, Default)]
struct RoomSpatialRequirements {
    needs: RoomSpatialNeeds,
    actor_rays: bool,
}

impl RoomSpatialRequirements {
    fn merge(self, other: Self) -> Self {
        Self {
            needs: self.needs.max(other.needs),
            actor_rays: self.actor_rays || other.actor_rays,
        }
    }
}

fn room_spatial_requirements(value: &Value) -> RoomSpatialRequirements {
    use RoomSpatialNeeds::{EntityFacts, EntityTags, None as NoQueries, Queries};

    if value.get("$").and_then(Value::as_str) == Some("ClientOnly") {
        return value
            .get("server_fallback")
            .map_or_else(RoomSpatialRequirements::default, room_spatial_requirements);
    }
    match value {
        Value::Object(fields) => {
            let filter = fields.get("filter").and_then(Value::as_object);
            let tags = filter.is_some_and(|filter| filter.contains_key("tags"));
            let class = fields.get("$").and_then(Value::as_str);
            let needs = match class {
                Some("EntityMatches" | "EntitiesInRadius" | "EntityRayCast") if tags => EntityTags,
                Some("EntityMatches") => EntityFacts,
                Some("EntitiesInRadius" | "EntityRayCast")
                    if filter.is_some_and(|filter| filter.contains_key("alive")) =>
                {
                    EntityFacts
                }
                Some(
                    "EntitiesInRadius" | "SegmentClear" | "RayCast" | "EntityRayCast"
                    | "EntityPosition" | "EntityDirection",
                ) => Queries,
                _ => NoQueries,
            };
            fields.values().map(room_spatial_requirements).fold(
                RoomSpatialRequirements {
                    needs,
                    actor_rays: class == Some("EntityRayCast"),
                },
                RoomSpatialRequirements::merge,
            )
        }
        Value::Array(values) => values.iter().map(room_spatial_requirements).fold(
            RoomSpatialRequirements::default(),
            RoomSpatialRequirements::merge,
        ),
        _ => RoomSpatialRequirements::default(),
    }
}

#[derive(Debug)]
pub struct GraphStore {
    graphs: BTreeMap<u32, Graph>,
    projected_message_topics: BTreeSet<u64>,
    room_spatial_needs: RoomSpatialNeeds,
    uses_spatial_actor_rays: bool,
    uses_host_health_events: bool,
    bodies: BTreeMap<u64, Body>,
    entities: BTreeMap<u64, Body>,
    mode_variables: BTreeMap<u64, BTreeMap<u32, ModeValue>>,
    mode_scripts: BTreeMap<u64, ModeScripts>,
    mode_rulesets: ModeRulesets,
    enemies: EnemyCatalog,
    missing: Vec<u32>,
}

impl GraphStore {
    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        let input = BufReader::new(File::open(path)?);
        Self::from_gzip(input)
    }

    pub fn from_gzip(input: impl io::BufRead) -> io::Result<Self> {
        let mut data = Vec::new();
        MultiGzDecoder::new(input)
            .take(MAX_GRAPH_BYTES + 1)
            .read_to_end(&mut data)?;
        if data.len() as u64 > MAX_GRAPH_BYTES {
            return Err(invalid("state graph bundle exceeds the size limit"));
        }
        Self::from_json(&data)
    }

    pub fn from_json(data: &[u8]) -> io::Result<Self> {
        let bundle: Bundle = serde_json::from_slice(data).map_err(invalid)?;
        if bundle.version != 1 {
            return Err(invalid("unsupported state graph bundle version"));
        }
        let mut authored_classes = BTreeMap::new();
        for AuthoredClass { class, behavior } in bundle.authored_classes {
            let valid_name = class.strip_prefix("authored.").is_some_and(|name| {
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            });
            if !valid_name {
                return Err(invalid(format!(
                    "class {class:?} must use an authored. name with letters, digits, underscores or hyphens"
                )));
            }
            if authored_classes.insert(class.clone(), behavior).is_some() {
                return Err(invalid(format!("duplicate authored class {class}")));
            }
        }
        let mut plain_classes = BTreeSet::new();
        for class in bundle.runtime_state_metadata.plain_classes {
            if !valid_runtime_class_name(&class) {
                return Err(invalid("invalid runtime plain state class name"));
            }
            if !plain_classes.insert(class) {
                return Err(invalid("duplicate runtime plain state class"));
            }
        }
        // The 104319 factory maps this class to the base state even in
        // version 1 bundles prepared before runtime annotations were available.
        plain_classes.insert("STU_F08E1453".into());
        let mut client_states = BTreeMap::<u32, BTreeMap<u16, RuntimeClientState>>::new();
        for row in bundle.runtime_state_metadata.client_states {
            let RuntimeClientState { graph, state, .. } = row;
            if !valid_runtime_class_name(&row.class) {
                return Err(invalid(format!(
                    "invalid runtime client state class in graph {graph} at slot {state}"
                )));
            }
            if client_states
                .entry(graph)
                .or_default()
                .insert(state, row)
                .is_some()
            {
                return Err(invalid(format!(
                    "duplicate runtime client state in graph {graph} at slot {state}"
                )));
            }
        }
        let mut graphs = BTreeMap::new();
        for mut graph in bundle.graphs {
            if !authored_classes.is_empty() {
                for node in &mut graph.nodes {
                    if let Some(behavior) = authored_classes.get(&node.class) {
                        behavior.apply(graph.index, node)?;
                    }
                }
            }
            graph.apply_runtime_state_metadata(
                &plain_classes,
                client_states.remove(&graph.index).unwrap_or_default(),
            )?;
            graph.compile()?;
            let index = graph.index;
            if graphs.insert(index, graph).is_some() {
                return Err(invalid(format!(
                    "bundle defines graph {index} more than once"
                )));
            }
        }
        if let Some((graph, _)) = client_states.first_key_value() {
            return Err(invalid(format!(
                "runtime client state references unknown graph {graph}"
            )));
        }
        for graph in graphs.values() {
            for node in &graph.nodes {
                if let Some((child, _)) = node
                    .target_child_binding()
                    .filter(|(child, _)| !graphs.contains_key(child))
                {
                    return Err(invalid(format!(
                        "track entities state in graph {} at node {} references missing child_graph {child}",
                        graph.index, node.pos
                    )));
                }
            }
        }
        for (body, definition) in bundle
            .bodies
            .iter()
            .chain(&bundle.entities)
            .chain(&bundle.junkenstein.entities)
        {
            for binding in &definition.server_graphs {
                let graph = binding.graph.ok_or_else(|| {
                    invalid(format!(
                        "body {body} declares a server root without a graph"
                    ))
                })?;
                if !graphs.contains_key(&graph) {
                    return Err(invalid(format!(
                        "body {body} declares missing server graph {graph}"
                    )));
                }
            }
        }
        let projected_message_topics = validate_projected_message_topics(
            bundle.runtime_state_metadata.projected_message_topics,
            &graphs,
        )?;
        let room_spatial_requirements = graphs
            .values()
            .flat_map(|graph| &graph.nodes)
            .filter(|node| node.runs_on_server())
            .map(|node| {
                let mut requirements = room_spatial_requirements(&node.fields);
                if node.declares_health_change_action()
                    || node.declares_room_message_action()
                    || node.declares_room_cue_action()
                {
                    requirements.needs = requirements.needs.max(RoomSpatialNeeds::Queries);
                }
                requirements
            })
            .fold(
                RoomSpatialRequirements::default(),
                RoomSpatialRequirements::merge,
            );
        let health_topic = EntryTopic::Host(HostEventKind::HealthApplied);
        let uses_host_health_events = graphs
            .values()
            .any(|graph| graph.entry_handlers.contains_key(&health_topic));
        Ok(Self {
            graphs,
            projected_message_topics,
            room_spatial_needs: room_spatial_requirements.needs,
            uses_spatial_actor_rays: room_spatial_requirements.actor_rays,
            uses_host_health_events,
            bodies: keyed_by_guid(bundle.bodies)?,
            entities: keyed_by_guid(bundle.entities)?,
            mode_variables: keyed_by_guid(bundle.mode_variables)?
                .into_iter()
                .map(|(guid, values)| {
                    let values = values
                        .into_iter()
                        .map(|(var, value)| Ok((var, mode_value(&value)?)))
                        .collect::<io::Result<_>>()?;
                    Ok((guid, values))
                })
                .collect::<io::Result<_>>()?,
            mode_scripts: keyed_by_guid(bundle.mode_scripts)?,
            mode_rulesets: ModeRulesets::compile(bundle.mode_rulesets)?,
            enemies: bundle.junkenstein.validate()?,
            missing: bundle.missing,
        })
    }

    /// Whether any server graph uses the room's living body snapshot.
    pub fn uses_room_spatial_frame(&self) -> bool {
        self.room_spatial_needs != RoomSpatialNeeds::None
    }

    /// The most detailed room data requested by any server graph.
    pub fn room_spatial_needs(&self) -> RoomSpatialNeeds {
        self.room_spatial_needs
    }

    /// Whether a server operand needs frozen actor collision shapes.
    pub fn uses_spatial_actor_rays(&self) -> bool {
        self.uses_spatial_actor_rays
    }

    /// Whether this bundle explicitly subscribes to a host health event.
    pub fn uses_host_health_events(&self) -> bool {
        self.uses_host_health_events
    }

    pub fn graph(&self, index: u32) -> Option<&Graph> {
        self.graphs.get(&index)
    }

    pub(crate) fn is_projected_message_topic(&self, id: u64) -> bool {
        self.projected_message_topics.contains(&id)
    }

    /// Inspect local graph structure on demand. Unresolved null links and links
    /// to declared containers are valid and do not appear in this report.
    pub fn dangling_links(&self) -> impl Iterator<Item = DanglingLink> + '_ {
        self.graphs.values().flat_map(Graph::dangling_links)
    }

    pub fn graph_indexes(&self) -> impl Iterator<Item = u32> + '_ {
        self.graphs.keys().copied()
    }

    pub fn bodies(&self) -> &BTreeMap<u64, Body> {
        &self.bodies
    }

    pub fn entity_body(&self, guid: u64) -> Option<&Body> {
        self.entities.get(&guid)
    }

    pub fn enemy_pair(&self, hero: u64) -> Option<EnemyPair> {
        self.enemies
            .pairs
            .get(&format!("0x{hero:016X}"))
            .map(|pair| EnemyPair {
                body: parse_guid(&pair.body).expect("validated enemy body"),
                controller: parse_guid(&pair.controller).expect("validated enemy controller"),
            })
    }

    pub fn enemy_body(&self, definition: u64) -> Option<&Body> {
        self.enemies.entities.get(&format!("0x{definition:016X}"))
    }

    pub fn mode_variables(&self, mode: u64) -> Option<&BTreeMap<u32, ModeValue>> {
        self.mode_variables.get(&mode)
    }

    /// Authored per-body overrides take precedence over mode defaults.
    pub fn mode_ruleset_value(
        &self,
        mode: u64,
        body: u64,
        id: u32,
    ) -> Option<crate::script_wire::Value> {
        self.mode_rulesets.value(mode, body, id)
    }

    pub fn mode_scripts(&self, mode: u64) -> Option<&ModeScripts> {
        self.mode_scripts.get(&mode)
    }

    pub fn missing(&self) -> &[u32] {
        &self.missing
    }
}

pub fn asset_guid(value: &Value) -> Option<u64> {
    let text = value.as_str()?;
    let hex = text.strip_prefix("0x")?;
    if hex.len() == 16
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'A'..=b'F'))
    {
        parse_guid(text)
    } else {
        None
    }
}

pub(crate) fn message_parameter_id(config: &Value) -> u32 {
    config
        .get("m_B5051BCE")
        .and_then(asset_guid)
        .map_or(0, |guid| u32::from(guid as u16))
}

pub fn target_tag_guid(value: &Value) -> Option<u64> {
    asset_guid(value).filter(|guid| (guid >> 48) & 0xfff == TARGET_TAG_TYPE)
}

fn body_target_tags<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<Option<BTreeSet<u64>>, D::Error> {
    Option::<Vec<Value>>::deserialize(decoder)?
        .map(|tags| {
            tags.iter()
                .map(|tag| {
                    target_tag_guid(tag)
                        .or_else(|| {
                            tag.as_u64()
                                .filter(|guid| (guid >> 48) & 0xfff == TARGET_TAG_TYPE)
                        })
                        .ok_or_else(|| {
                            serde::de::Error::custom("body target tag must be a packed .06C GUID")
                        })
                })
                .collect()
        })
        .transpose()
}

pub fn field_at<'a>(fields: &'a Value, path: &str) -> Option<&'a Value> {
    let mut value = fields;
    for part in path.split('.') {
        let (name, mut indexes) = part.split_at(part.find('[').unwrap_or(part.len()));
        if name.is_empty() || !name.chars().all(|ch| ch == '_' || ch.is_alphanumeric()) {
            return None;
        }
        value = value.get(name)?;
        while !indexes.is_empty() {
            let (index, tail) = indexes.strip_prefix('[')?.split_once(']')?;
            if index.is_empty() || !index.bytes().all(|ch| ch.is_ascii_digit()) {
                return None;
            }
            value = value.get(index.parse::<usize>().ok()?)?;
            indexes = tail;
        }
    }
    Some(value)
}

fn collect_plugs<'a>(value: &'a Value, path: &str, out: &mut BTreeMap<String, &'a Value>) {
    match value {
        Value::Object(fields) if fields.contains_key("$") && fields.contains_key("links") => {
            out.insert(path.to_owned(), value);
        }
        Value::Object(fields) => {
            for (key, item) in fields {
                if key != "$" {
                    let name = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    collect_plugs(item, &name, out);
                }
            }
        }
        Value::Array(values) => {
            for (index, item) in values.iter().enumerate() {
                collect_plugs(item, &format!("{path}[{index}]"), out);
            }
        }
        _ => {}
    }
}

fn collect_graphs(value: &Value, out: &mut BTreeSet<u64>) {
    match value {
        Value::Object(fields) => {
            for field in fields.values() {
                collect_graphs(field, out);
            }
        }
        Value::Array(values) => {
            for item in values {
                collect_graphs(item, out);
            }
        }
        _ => {
            if let Some(guid) = asset_guid(value).filter(|guid| guid >> 48 == GRAPH_TYPE) {
                out.insert(guid & 0x0000_ffff_ffff_ffff);
            }
        }
    }
}

fn keyed_by_guid<T>(records: BTreeMap<String, T>) -> io::Result<BTreeMap<u64, T>> {
    records
        .into_iter()
        .map(|(key, value)| {
            let hex = key.strip_prefix("0x").unwrap_or(&key);
            let guid = u64::from_str_radix(hex, 16).map_err(invalid)?;
            Ok((guid, value))
        })
        .collect()
}

fn mode_value(value: &Value) -> io::Result<ModeValue> {
    if let Some(value) = value.as_bool() {
        return Ok(ModeValue::Bool(value));
    }
    if let Some(value) = value.as_i64() {
        return Ok(ModeValue::Integer(value));
    }
    asset_guid(value)
        .map(ModeValue::Asset)
        .ok_or_else(|| invalid("unsupported mode variable"))
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64() != Some(0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

fn node_flag<'de, D: serde::Deserializer<'de>>(decoder: D) -> Result<bool, D::Error> {
    Value::deserialize(decoder).map(|value| truthy(&value))
}

fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        let node = |pos, rid, client_only, fields: Value| {
            json!({
                "pos": pos, "uid": 0, "rid": rid, "remote": 0, "state": null,
                "class": "Node", "client_only": client_only, "server_only": 0, "fields": fields
            })
        };
        let body = json!({
            "hero": null, "name": "Fixture", "manager": null, "weapons": [null, 42],
            "graphs": [{"graph": 42, "m_1EB5A024": null, "m_75408640": 0}]
        });
        json!({
            "version": 1,
            "graphs": [{
                "index": 42,
                "header": {"m_nodesBitCount": 4, "m_statesBitCount": 3,
                    "m_remoteSyncNodesBitCount": 3, "m_syncVarsBitCount": 3},
                "fields": {"ref": "0x058000000000001A", "ignored": "0x058000000000001b"},
                "items": 5, "containers": {"3": "Opaque"}, "unknown_nodes": [8],
                "nodes": [
                    node(0, 3, false, json!({
                        "m_2BBEEAB8": true,
                        "entries": [{"trigger": {"$": "Plug", "links": [[2, "input"], [null, "unknown"]]} }],
                        "related": ["0x058000000000002B", "0x040000000000002A"]
                    })),
                    node(1, 5, true, json!({"m_2BBEEAB8": true})),
                    node(2, 6, false, json!({"m_ADEB6E05": true}))
                ],
                "states": [0, 1, null, 0],
                "entries": [0, 3, 2, null],
                "remote": [2, null, 1, 0],
                "sync_vars": [[null, 0, 0], [12, 1, 0], [3, 0, 1], [42, 0, 0]]
            }],
            "bodies": {"0x0400000000000001": body.clone()},
            "entities": {"0x0400000000000002": body},
            "mode_variables": {"0x0230000000000018": {
                "1": false, "2": 3, "3": "0x0D8000000000195F"
            }},
            "missing": [43]
        })
    }

    #[test]
    fn runtime_state_metadata_checks_node_identity_and_preserves_wire_scope() {
        let mut data = fixture();
        let graph = &mut data["graphs"][0];
        graph["nodes"][0]["state"] = json!(0);
        graph["nodes"][1]["state"] = json!(1);
        graph["nodes"][1]["client_only"] = json!(false);
        let load = |value: &Value| GraphStore::from_json(&serde_json::to_vec(value).unwrap());
        let baseline = load(&data).unwrap();
        let baseline = baseline.graph(42).unwrap();

        let row = json!({"graph": 42, "state": 1, "rid": 5, "class": "Node"});
        data["runtime_state_metadata"] = json!({
            "plain_classes": ["Node"],
            "client_states": [row.clone()],
        });
        let store = load(&data).unwrap();
        let graph = store.graph(42).unwrap();
        assert!(graph.node(0).unwrap().declares_runtime_plain_state());
        let client = graph.node(1).unwrap();
        assert!(!client.declares_runtime_plain_state() && !client.runs_on_server());
        assert!(client.is_networked() && !client.client_only && !client.server_only);
        assert_eq!(client.class, "Node");
        assert!(!graph.node(2).unwrap().declares_runtime_plain_state());
        assert_eq!(graph.owner_positions(), baseline.owner_positions());
        assert_eq!(graph.remote_positions(), baseline.remote_positions());

        for (field, value) in [
            ("rid", json!(3)),
            ("class", json!("OtherState")),
            ("state", json!(2)),
            ("graph", json!(43)),
            ("unexpected", json!(true)),
        ] {
            let mut invalid = data.clone();
            invalid["runtime_state_metadata"]["client_states"][0][field] = value;
            assert_eq!(
                load(&invalid).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
        for (field, value) in [
            ("client_states", json!([row.clone(), row.clone()])),
            ("plain_classes", json!(["Node", "Node"])),
            ("plain_classes", json!(["authored.state"])),
            ("plain_classes", json!(["N".repeat(129)])),
        ] {
            let mut invalid = data.clone();
            invalid["runtime_state_metadata"][field] = value;
            assert_eq!(
                load(&invalid).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
        for (field, value) in [
            ("server_only", json!(true)),
            ("state_behavior", json!("plain")),
            ("fields", json!({})),
        ] {
            let mut invalid = data.clone();
            invalid["graphs"][0]["nodes"][1][field] = value;
            assert_eq!(
                load(&invalid).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
    }

    #[test]
    fn body_server_graphs_preserve_public_roots_and_bind_variables_in_order() {
        use crate::{
            script_init::{Root, body_roots, player_roots},
            script_wire::Value as ScriptValue,
        };

        const BODY: &str = "0x0400000000000001";
        let mut data = fixture();
        let body = &mut data["bodies"][BODY];
        body["manager"] = json!(42);
        body["graphs"].as_array_mut().unwrap().extend([
            json!({"graph":null,"m_75408640":0}),
            json!({"graph":42,"m_75408640":0}),
        ]);
        body["server_graphs"] = json!([
            {"graph":42,"m_75408640":0,"m_1EB5A024":[{"m_0D09D2D9":"0x0000000000000017","m_value":11}]},
            {"graph":42,"m_75408640":0,"m_1EB5A024":[{"m_0D09D2D9":"0x0000000000000017","m_value":12}]}
        ]);
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let store = load(&data).unwrap();
        let body = &store.bodies()[&0x0400_0000_0000_0001];
        let ids = |roots: &[Root]| roots.iter().map(|root| root.id).collect::<Vec<_>>();
        let body = body_roots(body).unwrap();
        assert_eq!(ids(&body), [1, 3, 0, 0]);
        let player = player_roots(&store.bodies()[&0x0400_0000_0000_0001], &store).unwrap();
        assert_eq!(ids(&player), [1, 3, 4, 5, 0, 0]);
        assert_eq!(player[3].variables[&28], ScriptValue::Integer(1));
        for roots in [&body, &player] {
            let seeded = roots
                .iter()
                .filter(|root| root.is_server_only())
                .map(|root| &root.variables[&23])
                .collect::<Vec<_>>();
            assert_eq!(
                seeded,
                [&ScriptValue::Integer(11), &ScriptValue::Integer(12)]
            );
        }
        for invalid in [Value::Null, json!(43)] {
            data["bodies"][BODY]["server_graphs"][0]["graph"] = invalid;
            assert_eq!(load(&data).unwrap_err().kind(), io::ErrorKind::InvalidData);
        }
    }

    #[test]
    fn invalid_or_duplicate_mode_rulesets_fail_loading() {
        let mut data = fixture();
        let mode = "0x00000000000000AA";
        let body = "0x00000000000000BA";
        let valid = json!({"mode": mode, "rules": [{"id": 7, "value": false}]});
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        assert_eq!(load(&data).unwrap().mode_ruleset_value(0xaa, 0xba, 7), None);
        for invalid_table in [
            json!([valid.clone(), valid.clone()]),
            json!([{"mode": "0x0000000000000000", "rules": []}]),
            json!([{"mode": "0x00000000000000aa", "rules": []}]),
            json!([{"mode": mode, "rules": [{"id": 7, "body": null, "value": 12}]}]),
            json!([{"mode": mode, "rules": [{"id": 7, "body": "BA", "value": 12}]}]),
            json!([{"mode": mode, "rules": [{"id": 7, "body": "0x0000000000000000", "value": 12}]}]),
            json!([{"mode": mode, "rules": [{"id": 7, "value": [1]}]}]),
            json!([{"mode": mode, "rules": [{"id": 7, "value": u64::MAX}]}]),
            json!([{"mode": mode, "rules": [{"id": 7}]}]),
            json!([{"mode": mode, "rules": [{"id": 7, "value": 1, "boddy": body}]}]),
            json!([{"mode": mode, "rules": [{"id": 7, "value": 1}, {"id": 7, "value": 2}]}]),
            json!([{"mode": mode, "rules": [
                {"id": 7, "body": body, "value": 1}, {"id": 7, "body": body, "value": 2}
            ]}]),
        ] {
            data["mode_rulesets"] = invalid_table;
            assert_eq!(
                load(&data).unwrap_err().kind(),
                io::ErrorKind::InvalidData,
                "{}",
                data["mode_rulesets"]
            );
        }
    }

    #[test]
    fn rule_integer_tokens_do_not_round_into_floats() {
        let mut data = fixture();
        data["mode_rulesets"] = json!([
            {"mode": "0x00000000000000AA", "rules": [{"id": 7, "value": "replace-numeric-token"}]}
        ]);
        let data = serde_json::to_string(&data).unwrap();
        for token in ["-9223372036854775809", "18446744073709551616", "1e309"] {
            let invalid = data.replace("\"replace-numeric-token\"", token);
            let result = GraphStore::from_json(invalid.as_bytes());
            assert_eq!(
                result.unwrap_err().kind(),
                io::ErrorKind::InvalidData,
                "{token}"
            );
        }
        let key = "\"replace-numeric-token\"";
        let integer =
            GraphStore::from_json(data.replace(key, "-9223372036854775808").as_bytes()).unwrap();
        let float = GraphStore::from_json(data.replace(key, "7e0").as_bytes()).unwrap();
        assert_eq!(
            integer.mode_ruleset_value(0xaa, 0, 7),
            Some(crate::script_wire::Value::Integer(i64::MIN))
        );
        assert_eq!(
            float.mode_ruleset_value(0xaa, 0, 7),
            Some(crate::script_wire::Value::Float(7.0))
        );
    }

    #[test]
    fn body_target_tags_distinguish_unknown_empty_and_invalid_data() {
        let mut data = fixture();
        let body = "0x0400000000000001";
        let entity = "0x0400000000000002";
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());

        assert!(
            load(&data).unwrap().bodies()[&0x0400_0000_0000_0001]
                .target_tags
                .is_none()
        );
        data["bodies"][body]["target_tags"] = json!([]);
        data["entities"][entity]["target_tags"] = json!([
            "0x0D60000000000159",
            "0x0D60000000000109",
            "0x0D60000000000159"
        ]);
        let parsed = load(&data).unwrap();
        assert_eq!(
            parsed.bodies()[&0x0400_0000_0000_0001].target_tags,
            Some(BTreeSet::new())
        );
        assert_eq!(
            parsed
                .entity_body(0x0400_0000_0000_0002)
                .unwrap()
                .target_tags,
            Some(BTreeSet::from([
                0x0d60_0000_0000_0109,
                0x0d60_0000_0000_0159
            ]))
        );
        data["bodies"][body]["target_tags"] = json!(["0x0400000000000109"]);
        assert_eq!(load(&data).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn enemy_pairs_keep_encounter_definitions_separate_and_validate_associations() {
        let hero = "0x02E0000000000175";
        let body = "0x040000000000000a";
        let controller = "0x040000000000000b";
        let mut data = fixture();
        let mut actor = data["bodies"]["0x0400000000000001"].clone();
        actor["hero"] = json!(hero);
        actor["ai_association"] = json!(0x0400_0000_0000_000bu64);
        actor["target_tags"] = json!([0x0d60_0000_0000_0109u64, "0x0D60000000000109"]);
        let control = data["entities"]["0x0400000000000002"].clone();
        data["junkenstein"] = json!({
            "pairs":{hero:{"body":body,"controller":controller}},
            "entities":{body:actor,controller:control}
        });
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let parsed = load(&data).unwrap();
        assert_eq!(
            parsed.enemy_pair(0x02e0_0000_0000_0175),
            Some(EnemyPair {
                body: 0x0400_0000_0000_000a,
                controller: 0x0400_0000_0000_000b,
            })
        );
        assert!(parsed.entity_body(0x0400_0000_0000_000a).is_none());
        assert_eq!(
            parsed
                .enemy_body(0x0400_0000_0000_000a)
                .unwrap()
                .target_tags,
            Some(BTreeSet::from([0x0d60_0000_0000_0109]))
        );
        for (field, value) in [
            ("ai_association", json!(0x0400_0000_0000_000cu64)),
            ("hero", json!("0x02E0000000000176")),
            ("target_tags", json!([0x0400_0000_0000_0109u64])),
        ] {
            let mut bad = data.clone();
            bad["junkenstein"]["entities"][body][field] = value;
            assert!(load(&bad).is_err());
        }
        let mut missing = data.clone();
        missing["junkenstein"]["entities"]
            .as_object_mut()
            .unwrap()
            .remove(controller);
        assert!(load(&missing).is_err());
        let mut duplicated = data.clone();
        duplicated["junkenstein"]["entities"]["0x040000000000000A"] =
            duplicated["junkenstein"]["entities"][body].clone();
        assert!(load(&duplicated).is_err());
        // Playable boss definitions use the explicit encounter pair instead of an AI association.
        data["junkenstein"]["entities"][body]["ai_association"] = Value::Null;
        assert!(load(&data).is_ok());
    }

    #[test]
    fn duplicate_graph_identities_are_rejected() {
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());

        let mut duplicate_position = fixture();
        duplicate_position["graphs"][0]["nodes"][2]["pos"] = json!(0);
        let error = load(&duplicate_position).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("node position 0"));

        let mut duplicate_graph = fixture();
        let duplicate = duplicate_graph["graphs"][0].clone();
        duplicate_graph["graphs"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        let error = load(&duplicate_graph).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("graph 42"));
    }

    #[test]
    fn authored_class_catalog_rejects_ambiguous_declarations() {
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let mut data = fixture();
        assert_eq!(
            load(&data).unwrap().graph(42).unwrap().owner_positions(),
            [0, 0]
        );

        for (declarations, message) in [
            (
                json!([{"class":"authored.latch","behavior":"plain_state"},
                       {"class":"authored.latch","behavior":"client_only"}]),
                "duplicate authored class",
            ),
            (
                json!([{"class":"STUStatescriptState","behavior":"client_only"}]),
                "must use an authored.",
            ),
            (
                json!([{"class":"authored.","behavior":"client_only"}]),
                "must use an authored.",
            ),
            (
                json!([{"class":"authored.latch","behavior":"unknown"}]),
                "unknown variant",
            ),
            (
                json!([{"class":"authored.latch","behavior":"plain_state","typo":true}]),
                "unknown field",
            ),
        ] {
            data["authored_classes"] = declarations;
            let error = load(&data).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert!(error.to_string().contains(message), "{error}");
        }
    }

    #[test]
    fn authored_class_catalog_reuses_node_validation() {
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let mut data = fixture();
        data["authored_classes"] = json!([
            {"class":"authored.latch","behavior":"plain_state"},
            {"class":"authored.overlay","behavior":"client_only"},
            {"class":"authored.forward","behavior":"forward_action"}
        ]);
        let nodes = &mut data["graphs"][0]["nodes"];
        nodes[0]["class"] = json!("authored.latch");
        nodes[0]["state"] = json!(0);
        nodes[0]["state_behavior"] = json!("plain");
        nodes[0]["server_only"] = json!(true);
        nodes[1]["class"] = json!("authored.overlay");
        nodes[2]["class"] = json!("authored.forward");
        nodes[2]["fields"]["m_outPlug"] = json!({"$":"Plug","links":[]});
        let error = load(&data).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("occupy its state slot exactly once")
        );
        data["graphs"][0]["states"][3] = Value::Null;

        let store = load(&data).unwrap();
        let graph = store.graph(42).unwrap();
        assert!(graph.owner_positions().is_empty());
        assert!(graph.remote_positions().is_empty());
        assert!(graph.node(1).unwrap().client_only);

        for (position, field, value, message) in [
            (0, "client_only", json!(true), "graph 42 at node 0"),
            (0, "action_behavior", json!("forward"), "graph 42 at node 0"),
            (0, "state", json!(1), "occupy its state slot"),
            (1, "server_only", json!(true), "graph 42 at node 1"),
            (1, "state_behavior", json!("plain"), "graph 42 at node 1"),
            (2, "fields", json!({}), "output plug"),
        ] {
            let mut malformed = data.clone();
            malformed["graphs"][0]["nodes"][position][field] = value;
            let error = load(&malformed).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert!(error.to_string().contains(message), "{error}");
        }

        data["graphs"][0]["states"][2] = json!(2);
        let error = load(&data).unwrap_err();
        assert!(error.to_string().contains("no state slot"), "{error}");
    }

    #[test]
    fn local_message_action_rejects_invalid_routes_and_parameter_slots() {
        let mut data = fixture();
        let node = &mut data["graphs"][0]["nodes"][2];
        node["server_only"] = json!(true);
        node["action_behavior"] = json!("local_message");
        node["fields"] = json!({
            "m_gameMessage": {"m_gameMessage":"0xF100000000000001"},
            "m_params": [{"m_B5051BCE":"0xF200000000000007", "m_value":12}],
            "m_outPlug": {"$":"Plug", "links":[]}
        });
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let valid = load(&data).unwrap();
        assert_eq!(
            valid.graph(42).unwrap().node(2).unwrap().game_message_id(),
            Some(0xf100_0000_0000_0001)
        );
        for (field, invalid_value) in [
            (
                "m_gameMessage",
                json!({"m_gameMessage":"0x0000000000000000"}),
            ),
            ("m_gameMessage", json!({})),
            ("m_target", json!(12)),
            ("m_targets", json!([])),
            ("m_params", json!(false)),
            ("m_params", json!([{"m_B5051BCE":"0xF200000000000007"}])),
            (
                "m_params",
                json!([{"m_B5051BCE":"0xF200000000000000","m_value":12}]),
            ),
            (
                "m_params",
                json!([
                    {"m_B5051BCE":"0xF100000000000007","m_value":12},
                    {"m_B5051BCE":"0xF200000000000007","m_value":13}
                ]),
            ),
        ] {
            let mut malformed = data.clone();
            malformed["graphs"][0]["nodes"][2]["fields"][field] = invalid_value;
            let error = load(&malformed).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert!(
                error
                    .to_string()
                    .contains("local message action in graph 42 at node 2")
            );
        }
        data["graphs"][0]["nodes"][2]["client_only"] = json!(true);
        assert!(
            load(&data)
                .unwrap_err()
                .to_string()
                .contains("must be server-only")
        );
    }

    #[test]
    fn forward_action_declarations_require_a_server_action_and_valid_output() {
        let mut data = fixture();
        let node = &mut data["graphs"][0]["nodes"][2];
        node["action_behavior"] = json!("forward");
        node["server_only"] = json!(true);
        node["fields"] = json!({"m_outPlug": {"$": "Plug", "links": [[null, "input"]]}});
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let store = load(&data).unwrap();
        let forward = store.graph(42).unwrap().node(2).unwrap();
        assert!(forward.is_action());
        assert!(!forward.is_state());
        assert!(!forward.is_networked());

        for (field, value) in [
            ("server_only", json!(false)),
            ("client_only", json!(true)),
            ("state", json!(0)),
            ("state_behavior", json!("plain")),
            ("action_behavior", json!("unknown")),
            ("fields", json!({"m_outPlug": {"links": []}})),
            (
                "fields",
                json!({"m_outPlug": {"$": "Plug", "links": [[3, false]]}}),
            ),
        ] {
            let mut malformed = data.clone();
            malformed["graphs"][0]["nodes"][2][field] = value;
            assert_eq!(
                load(&malformed).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
        data["graphs"][0]["states"][2] = json!(2);
        assert_eq!(load(&data).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn room_spatial_service_is_requested_by_server_graph_operands() {
        let mut data = fixture();
        let load =
            |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap()).unwrap();
        assert!(!load(&data).uses_room_spatial_frame());
        assert!(load(&data).room_spatial_needs() < RoomSpatialNeeds::EntityFacts);

        data["graphs"][0]["nodes"][0]["fields"]["nested"] =
            json!([{"value":{"$":"EntitiesInRadius","center":null,"radius":1}}]);
        assert!(load(&data).uses_room_spatial_frame());
        assert!(load(&data).room_spatial_needs() < RoomSpatialNeeds::EntityFacts);

        let value = &mut data["graphs"][0]["nodes"][0]["fields"]["nested"][0]["value"];
        value["filter"] = json!({"relation":"ally","source":null});
        assert!(load(&data).room_spatial_needs() < RoomSpatialNeeds::EntityFacts);

        for (predicate, needs) in [
            (
                json!({"$":"EntityPosition","entity":null}),
                RoomSpatialNeeds::Queries,
            ),
            (
                json!({"$":"EntityDirection","entity":null}),
                RoomSpatialNeeds::Queries,
            ),
            (
                json!({"$":"SegmentClear","origin":null,"target":null}),
                RoomSpatialNeeds::Queries,
            ),
            (
                json!({"$":"RayCast","origin":null,"direction":null,"max_distance":1}),
                RoomSpatialNeeds::Queries,
            ),
            (
                json!({"$":"EntityRayCast","origin":null,"direction":null,"max_distance":1}),
                RoomSpatialNeeds::Queries,
            ),
            (
                json!({"$":"EntityRayCast","origin":null,"direction":null,"max_distance":1,"filter":{"alive":true}}),
                RoomSpatialNeeds::EntityFacts,
            ),
            (
                json!({"$":"EntityRayCast","origin":null,"direction":null,"max_distance":1,"filter":{"tags":{}}}),
                RoomSpatialNeeds::EntityTags,
            ),
            (
                json!({"$":"EntitiesInRadius","center":null,"radius":1,"filter":{"alive":true}}),
                RoomSpatialNeeds::EntityFacts,
            ),
            (
                json!({"$":"EntitiesInRadius","center":null,"radius":1,"filter":{"tags":{}}}),
                RoomSpatialNeeds::EntityTags,
            ),
            (
                json!({"$":"EntityMatches","entity":null,"filter":{"alive":true}}),
                RoomSpatialNeeds::EntityFacts,
            ),
            (
                json!({"$":"EntityMatches","entity":null,"filter":{"tags":{}}}),
                RoomSpatialNeeds::EntityTags,
            ),
        ] {
            let actor_rays = predicate.get("$").and_then(Value::as_str) == Some("EntityRayCast");
            data["graphs"][0]["nodes"][0]["fields"]["nested"][0]["value"] = predicate;
            let parsed = load(&data);
            assert!(parsed.uses_room_spatial_frame());
            assert_eq!(parsed.room_spatial_needs(), needs);
            assert_eq!(parsed.uses_spatial_actor_rays(), actor_rays);
        }

        let wrapper = json!({
            "$":"ClientOnly",
            "value":{"$":"EntityMatches","entity":null,"filter":{"tags":{}}}
        });
        data["graphs"][0]["nodes"][0]["fields"]["nested"][0]["value"] = wrapper;
        assert_eq!(load(&data).room_spatial_needs(), RoomSpatialNeeds::None);
        data["graphs"][0]["nodes"][0]["fields"]["nested"][0]["value"]["server_fallback"] =
            json!({"$":"SegmentClear","origin":null,"target":null});
        assert_eq!(load(&data).room_spatial_needs(), RoomSpatialNeeds::Queries);

        data["graphs"][0]["nodes"][0]["fields"]["nested"][0]["value"]["server_fallback"] =
            json!({"$":"RayCast","origin":null,"direction":null,"max_distance":1});
        assert_eq!(load(&data).room_spatial_needs(), RoomSpatialNeeds::Queries);

        data["graphs"][0]["nodes"][0]["fields"]["nested"][0]["value"] = json!({
            "$":"ClientOnly",
            "value":{"$":"EntityRayCast","origin":null,"direction":null,"max_distance":1}
        });
        assert!(!load(&data).uses_spatial_actor_rays());
        assert_eq!(load(&data).room_spatial_needs(), RoomSpatialNeeds::None);
        data["graphs"][0]["nodes"][0]["fields"]["nested"][0]["value"]["server_fallback"] =
            json!({"$":"EntityRayCast","origin":null,"direction":null,"max_distance":1});
        assert!(load(&data).uses_spatial_actor_rays());
        assert_eq!(load(&data).room_spatial_needs(), RoomSpatialNeeds::Queries);

        data["graphs"][0]["nodes"][0]["client_only"] = json!(true);
        assert!(!load(&data).uses_spatial_actor_rays());
        assert!(!load(&data).uses_room_spatial_frame());
        assert!(load(&data).room_spatial_needs() < RoomSpatialNeeds::EntityFacts);
    }

    #[test]
    fn branch_action_declarations_require_a_condition_and_both_outputs() {
        let mut data = fixture();
        let node = &mut data["graphs"][0]["nodes"][2];
        node["action_behavior"] = json!("branch");
        node["server_only"] = json!(true);
        node["fields"] = json!({
            "m_condition": false,
            "m_truePlug": {"$": "Plug", "links": [[null, "input"]]},
            "m_falsePlug": {"$": "Plug", "links": []},
        });
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let store = load(&data).unwrap();
        let branch = store.graph(42).unwrap().node(2).unwrap();
        assert!(branch.declares_branch_action() && branch.is_action() && !branch.is_networked());
        assert!(!branch.is_state());
        for field in ["m_condition", "m_truePlug", "m_falsePlug"] {
            let mut invalid = data.clone();
            invalid["graphs"][0]["nodes"][2]["fields"][field] = Value::Null;
            let result = load(&invalid).unwrap_err();
            assert_eq!(result.kind(), io::ErrorKind::InvalidData);
            assert!(
                result
                    .to_string()
                    .contains("branch action in graph 42 at node 2")
            );
            assert!(result.to_string().contains(field));
        }
    }

    #[test]
    fn health_change_action_requires_server_scope_and_operands() {
        let mut data = fixture();
        let node = &mut data["graphs"][0]["nodes"][2];
        node["action_behavior"] = json!("health_change");
        node["server_only"] = json!(true);
        node["fields"] = json!({
            "target": {"$": "STUConfigVarDynamic", "m_identifier": 1, "m_60DB8F99": false},
            "delta": -1,
            "m_outPlug": {"$": "Plug", "links": []},
        });
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let store = load(&data).unwrap();
        assert!(store.uses_room_spatial_frame());
        let action = store.graph(42).unwrap().node(2).unwrap();
        assert!(action.declares_health_change_action() && action.is_action());
        assert!(!action.is_networked() && !action.is_state());
        assert_eq!(action.class, "ActionHealthChange");

        for field in ["target", "delta", "m_outPlug"] {
            let mut invalid = data.clone();
            invalid["graphs"][0]["nodes"][2]["fields"][field] = Value::Null;
            let error = load(&invalid).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert!(
                error
                    .to_string()
                    .contains("health change action in graph 42 at node 2")
            );
            assert!(error.to_string().contains(field));
        }
        data["graphs"][0]["nodes"][2]["server_only"] = json!(false);
        let error = load(&data).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(
            error
                .to_string()
                .contains("must be server-only with no state slot")
        );
    }

    #[test]
    fn radial_health_action_requires_a_server_radius_and_delta() {
        let mut data = fixture();
        let node = &mut data["graphs"][0]["nodes"][2];
        node["action_behavior"] = json!("radial_health");
        node["server_only"] = json!(true);
        node["fields"] = json!({
            "area": {"$":"EntitiesInRadius","center":{"$":"EntityPosition","entity":null},"radius":4},
            "delta": -5,
            "m_outPlug": {"$":"Plug","links":[]},
        });

        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let store = load(&data).unwrap();
        assert_eq!(store.room_spatial_needs(), RoomSpatialNeeds::Queries);
        let action = store.graph(42).unwrap().node(2).unwrap();
        assert!(action.declares_radial_health_action() && action.is_action());
        assert!(!action.is_networked() && !action.is_state());

        for field in ["area", "delta", "edge_scale", "m_outPlug"] {
            let mut invalid = data.clone();
            invalid["graphs"][0]["nodes"][2]["fields"][field] = Value::Null;
            let error = load(&invalid).unwrap_err().to_string();
            assert!(error.contains("radial health action in graph 42 at node 2"));
        }
        data["graphs"][0]["nodes"][2]["server_only"] = json!(false);
        let error = load(&data).unwrap_err().to_string();
        assert!(error.contains("must be server-only with no state slot"));
    }

    #[test]
    fn room_cue_requires_an_authored_server_action_audience_and_assets() {
        let mut data = fixture();
        let node = &mut data["graphs"][0]["nodes"][2];
        node["action_behavior"] = json!("room_cue");
        node["server_only"] = json!(true);
        node["fields"] = json!({
            "audience": [17],
            "stimulus": "0x00000000000000A1",
            "line": "0x00000000000000B2",
            "m_outPlug": {"$": "Plug", "links": []},
        });

        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let store = load(&data).unwrap();
        let action = store.graph(42).unwrap().node(2).unwrap();
        assert_eq!(action.class, "ActionRoomCue");
        assert!(action.is_action() && !action.is_networked() && !action.is_state());
        assert_eq!(action.room_cue_assets(), Some((0xA1, 0xB2)));
        assert_eq!(store.room_spatial_needs(), RoomSpatialNeeds::Queries);

        for (field, value, diagnostic) in [
            ("audience", Value::Null, "audience expression"),
            ("stimulus", json!("0x0000000000000000"), "asset GUIDs"),
            ("stimulus", json!("0x00000000000000a1"), "asset GUIDs"),
            ("line", json!(178), "asset GUIDs"),
            ("line", json!("0xB2"), "asset GUIDs"),
            ("m_outPlug", Value::Null, "m_outPlug output plug"),
        ] {
            let mut malformed = data.clone();
            malformed["graphs"][0]["nodes"][2]["fields"][field] = value;
            let error = load(&malformed).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            let error = error.to_string();
            assert!(error.contains("room cue action in graph 42 at node 2"));
            assert!(error.contains(diagnostic), "{error}");
        }

        data["graphs"][0]["nodes"][2]["server_only"] = json!(false);
        let error = load(&data).unwrap_err().to_string();
        assert!(error.contains("must be server-only with no state slot"));

        let node = &mut data["graphs"][0]["nodes"][2];
        node["action_behavior"] = Value::Null;
        node["class"] = json!("ActionRoomCue");
        let store = load(&data).unwrap();
        let action = store.graph(42).unwrap().node(2).unwrap();
        assert!(!action.declares_room_cue_action());
        assert_eq!(action.room_cue_assets(), None);
        assert_eq!(store.room_spatial_needs(), RoomSpatialNeeds::None);

        let node = &mut data["graphs"][0]["nodes"][2];
        node["class"] = json!("authored.cue");
        node["fields"]["audience"] = json!({
            "$": "EntitiesInRadius",
            "center": {"$": "EntityPosition", "entity": null},
            "radius": 10,
            "filter": {"tags": {"all": ["0xF100000000000007"]}},
        });
        data["authored_classes"] =
            json!([{"class": "authored.cue", "behavior": "room_cue_action"}]);
        let store = load(&data).unwrap();
        let action = store.graph(42).unwrap().node(2).unwrap();
        assert_eq!(action.class, "ActionRoomCue");
        assert!(action.server_only && action.declares_room_cue_action());
        assert_eq!(action.room_cue_assets(), Some((0xA1, 0xB2)));
        assert_eq!(store.room_spatial_needs(), RoomSpatialNeeds::EntityTags);
    }

    #[test]
    fn plain_state_declarations_require_a_unique_server_state_slot() {
        let mut data = fixture();
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let legacy = load(&data).unwrap();
        let graph = legacy.graph(42).unwrap();
        assert_eq!(graph.state(0).unwrap().pos, graph.state(3).unwrap().pos);

        let node = &mut data["graphs"][0]["nodes"][0];
        node["state"] = json!(0);
        node["server_only"] = json!(true);
        node["state_behavior"] = json!("plain");
        let error = load(&data).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("graph 42 at node 0"));
        assert!(
            error
                .to_string()
                .contains("occupy its state slot exactly once")
        );
        data["graphs"][0]["states"][3] = Value::Null;

        let store = load(&data).unwrap();
        let graph = store.graph(42).unwrap();
        let plain = graph.state(0).unwrap();
        assert!(plain.is_state());
        assert!(!plain.is_networked());
        assert!(graph.owner_positions().is_empty());

        for (field, value) in [
            ("server_only", json!(false)),
            ("client_only", json!(true)),
            ("state", json!(1)),
            ("state", Value::Null),
            ("state_behavior", json!("unrecognized")),
        ] {
            let mut malformed = data.clone();
            malformed["graphs"][0]["nodes"][0][field] = value;
            assert_eq!(
                load(&malformed).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
        data["graphs"][0]["nodes"][0]["fields"]["m_ADEB6E05"] = json!(true);
        assert_eq!(load(&data).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn plain_state_declarations_validate_optional_lifecycle_plugs() {
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let mut data = fixture();
        data["graphs"][0]["states"][3] = Value::Null;
        let node = &mut data["graphs"][0]["nodes"][0];
        node["state"] = json!(0);
        node["server_only"] = json!(true);
        node["state_behavior"] = json!("plain");

        for path in [
            "m_onBeginPlug",
            "m_onEndPlug",
            "m_onAbortPlug",
            "m_onFinishedPlug",
        ] {
            let mut sample = data.clone();
            sample["graphs"][0]["nodes"][0]["fields"][path] =
                json!({"$": "Plug", "links": [[2, "input"], [null, "unresolved"]]});
            assert!(load(&sample).is_ok());

            for malformed in [
                json!({"$": "Plug"}),
                json!({"$": 1, "links": []}),
                json!({"$": "Plug", "links": [[2, 3]]}),
            ] {
                sample["graphs"][0]["nodes"][0]["fields"][path] = malformed;
                let error = load(&sample).unwrap_err();
                assert_eq!(error.kind(), io::ErrorKind::InvalidData);
                assert!(error.to_string().contains("graph 42 at node 0"));
                assert!(error.to_string().contains(path));

                sample["graphs"][0]["nodes"][0]["state_behavior"] = Value::Null;
                assert!(load(&sample).is_ok());
                sample["graphs"][0]["nodes"][0]["state_behavior"] = json!("plain");
            }
            sample["graphs"][0]["nodes"][0]["fields"][path] = Value::Null;
            assert!(load(&sample).is_ok());
        }
    }

    #[test]
    fn observe_state_declarations_require_a_writable_output_and_valid_change_plug() {
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let mut data = fixture();
        data["graphs"][0]["states"][3] = Value::Null;
        let node = &mut data["graphs"][0]["nodes"][0];
        node["state"] = json!(0);
        node["server_only"] = json!(true);
        node["state_behavior"] = json!("observe");
        node["fields"] = json!({
            "value": false,
            "output": {"$": "STUConfigVarDynamic", "m_identifier": 7, "m_60DB8F99": true},
            "on_change": {"$": "Plug", "links": [[2, "input"]]},
        });

        for (field, invalid, message) in [
            ("value", Value::Null, "value expression"),
            ("output", json!(3), "writable output"),
            (
                "on_change",
                json!({"$": "Plug", "links": [[2, 3]]}),
                "on_change",
            ),
        ] {
            let mut sample = data.clone();
            sample["graphs"][0]["nodes"][0]["fields"][field] = invalid;
            let error = load(&sample).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert!(error.to_string().contains("graph 42 at node 0"));
            assert!(error.to_string().contains(message), "{error}");
        }

        let graph = load(&data).unwrap();
        let observe = graph.graph(42).unwrap().state(0).unwrap();
        assert!(observe.declares_observe_state() && observe.is_state() && !observe.is_networked());
        assert_eq!(observe.class, "StateObserve");

        data["authored_classes"] = json!([
            {"class": "authored.sample", "behavior": "observe_state"}
        ]);
        let node = &mut data["graphs"][0]["nodes"][0];
        node["class"] = json!("authored.sample");
        node["state_behavior"] = Value::Null;
        node["server_only"] = json!(false);
        node["fields"]["on_change"] = Value::Null;
        assert!(load(&data).is_ok());
    }

    #[test]
    fn interval_state_declarations_require_scope_period_and_output() {
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let rejects = |data: &Value, expected: &str| {
            let error = load(data).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            let message = error.to_string();
            assert!(message.contains("graph 42 at node 0"), "{message}");
            assert!(message.contains(expected), "{message}");
        };
        let mut data = fixture();
        data["graphs"][0]["states"][3] = Value::Null;
        let node = &mut data["graphs"][0]["nodes"][0];
        node["state"] = json!(0);
        node["server_only"] = json!(true);
        node["state_behavior"] = json!("interval");
        node["fields"] = json!({
            "interval_frames": 1,
            "on_interval": {"$": "Plug", "links": []},
        });

        let graph = load(&data).unwrap();
        let interval = graph.graph(42).unwrap().state(0).unwrap();
        assert_eq!(interval.class, "StateInterval");
        assert!(interval.is_state() && !interval.is_networked());
        assert_eq!(interval.interval_frames().map(NonZeroU32::get), Some(1));

        for period in [
            Value::Null,
            json!(0),
            json!(-1),
            json!(1.0),
            json!(true),
            json!("1"),
            json!(u64::from(u32::MAX) + 1),
        ] {
            let mut bad = data.clone();
            bad["graphs"][0]["nodes"][0]["fields"]["interval_frames"] = period;
            rejects(&bad, "interval_frames");
        }
        for plug in [
            Value::Null,
            json!({"$": "Plug"}),
            json!({"$": "Plug", "links": [[2, 3]]}),
        ] {
            let mut bad = data.clone();
            bad["graphs"][0]["nodes"][0]["fields"]["on_interval"] = plug;
            rejects(&bad, "on_interval");
        }
        let mut missing_plug = data.clone();
        missing_plug["graphs"][0]["nodes"][0]["fields"]
            .as_object_mut()
            .unwrap()
            .remove("on_interval");
        rejects(&missing_plug, "on_interval");
        for (field, invalid) in [
            ("server_only", json!(false)),
            ("client_only", json!(true)),
            ("state", Value::Null),
        ] {
            let mut bad = data.clone();
            bad["graphs"][0]["nodes"][0][field] = invalid;
            rejects(&bad, "server-only and occupy its state slot exactly once");
        }
        let mut bad_lifecycle = data.clone();
        bad_lifecycle["graphs"][0]["nodes"][0]["fields"]["m_onBeginPlug"] = json!(3);
        rejects(&bad_lifecycle, "m_onBeginPlug");

        let node = &mut data["graphs"][0]["nodes"][0];
        node["state_behavior"] = Value::Null;
        node["class"] = json!("StateInterval");
        let graph = load(&data).unwrap();
        assert!(
            graph
                .graph(42)
                .unwrap()
                .state(0)
                .unwrap()
                .interval_frames()
                .is_none()
        );

        data["authored_classes"] =
            json!([{"class": "authored.timer", "behavior": "interval_state"}]);
        let node = &mut data["graphs"][0]["nodes"][0];
        node["class"] = json!("authored.timer");
        node["server_only"] = json!(false);
        node["fields"]["interval_frames"] = json!(u32::MAX);
        let graph = load(&data).unwrap();
        let interval = graph.graph(42).unwrap().state(0).unwrap();
        assert_eq!(interval.class, "StateInterval");
        assert!(interval.server_only && !interval.client_only);
        assert_eq!(
            interval.interval_frames().map(NonZeroU32::get),
            Some(u32::MAX)
        );
    }

    #[test]
    fn authored_button_lock_requires_server_scope_and_valid_config() {
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let mut data = fixture();
        data["graphs"][0]["states"][3] = Value::Null;
        let node = &mut data["graphs"][0]["nodes"][0];
        node["state"] = json!(0);
        node["server_only"] = json!(true);
        node["state_behavior"] = json!("button_lock");
        node["fields"] = json!({"button": 255});

        let graphs = load(&data).unwrap();
        let lock = graphs.graph(42).unwrap().state(0).unwrap();
        assert_eq!(lock.class, "StateButtonLock");
        assert_eq!(lock.button_lock(), Some((255, None)));
        assert!(lock.is_state() && !lock.is_networked());

        for (field, invalid) in [
            ("button", json!(0)),
            ("button", json!(256)),
            ("button", json!(-1)),
            ("button", json!(1.0)),
            ("duration_frames", Value::Null),
            ("duration_frames", json!(0)),
            ("duration_frames", json!(1.0)),
            ("duration_frames", json!(u64::from(u32::MAX) + 1)),
        ] {
            let mut bad = data.clone();
            bad["graphs"][0]["nodes"][0]["fields"][field] = invalid;
            let error = load(&bad).unwrap_err().to_string();
            assert!(
                error.contains("button lock state in graph 42 at node 0"),
                "{error}"
            );
            assert!(error.contains(field), "{error}");
        }
        for (field, invalid) in [("server_only", false), ("client_only", true)] {
            let mut bad = data.clone();
            bad["graphs"][0]["nodes"][0][field] = json!(invalid);
            assert!(load(&bad).unwrap_err().to_string().contains("server-only"));
        }

        data["authored_classes"] =
            json!([{"class": "authored.lock", "behavior": "button_lock_state"}]);
        let node = &mut data["graphs"][0]["nodes"][0];
        node["class"] = json!("authored.lock");
        node["state_behavior"] = Value::Null;
        node["server_only"] = json!(false);
        node["fields"] = json!({"button": 1, "duration_frames": u32::MAX});
        let graphs = load(&data).unwrap();
        let lock = graphs.graph(42).unwrap().state(0).unwrap();
        assert_eq!(lock.class, "StateButtonLock");
        assert_eq!(lock.button_lock(), Some((1, NonZeroU32::new(u32::MAX))));
        assert!(lock.server_only && !lock.client_only);
    }

    #[test]
    fn authored_movement_scale_validates_scope_factor_duration_and_alias() {
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let mut data = fixture();
        data["graphs"][0]["states"][3] = Value::Null;
        let node = &mut data["graphs"][0]["nodes"][0];
        node["state"] = json!(0);
        node["server_only"] = json!(true);
        node["state_behavior"] = json!("movement_scale");
        node["fields"] = json!({"factor": 0});

        let graphs = load(&data).unwrap();
        let scale = graphs.graph(42).unwrap().state(0).unwrap();
        assert_eq!(scale.class, "StateMovementScale");
        assert_eq!(scale.movement_scale(), Some((0.0, None)));
        assert!(scale.is_state() && !scale.is_networked());

        for fields in [
            json!({}),
            json!({"factor":-0.01}),
            json!({"factor":1.01}),
            json!({"factor":"0.5"}),
            json!({"factor":true}),
            json!({"factor":null}),
            json!({"factor":0.5,"duration_frames":null}),
            json!({"factor":0.5,"duration_frames":0}),
            json!({"factor":0.5,"duration_frames":1.0}),
            json!({"factor":0.5,"duration_frames":u64::from(u32::MAX) + 1}),
        ] {
            let mut bad = data.clone();
            bad["graphs"][0]["nodes"][0]["fields"] = fields;
            let error = load(&bad).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert!(
                error
                    .to_string()
                    .contains("movement scale state in graph 42 at node 0"),
                "{error}"
            );
        }
        let mut bad = data.clone();
        bad["graphs"][0]["nodes"][0]["server_only"] = json!(false);
        assert!(load(&bad).unwrap_err().to_string().contains("server-only"));

        data["authored_classes"] =
            json!([{"class": "authored.scale", "behavior": "movement_scale_state"}]);
        let node = &mut data["graphs"][0]["nodes"][0];
        node["class"] = json!("authored.scale");
        node["state_behavior"] = Value::Null;
        node["server_only"] = json!(false);
        node["fields"] = json!({"factor": 1.0, "duration_frames": u32::MAX});
        let graphs = load(&data).unwrap();
        let scale = graphs.graph(42).unwrap().state(0).unwrap();
        assert_eq!(scale.class, "StateMovementScale");
        assert_eq!(
            scale.movement_scale(),
            Some((1.0, NonZeroU32::new(u32::MAX)))
        );
        assert!(scale.server_only && !scale.client_only);
    }

    #[test]
    fn authored_stack_claim_validates_binding_and_keeps_null_as_a_value() {
        use crate::script_expr::Scope;

        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let mut data = fixture();
        data["graphs"][0]["states"][3] = Value::Null;
        let node = &mut data["graphs"][0]["nodes"][0];
        node["state"] = json!(0);
        node["server_only"] = json!(true);
        node["state_behavior"] = json!("stack_claim");
        node["fields"] = json!({
            "variable":{"scope":"entity","id":9},
            "priority":{"level":-3,"above_equals":true},
            "value":null,
            "duration_frames":3,
            "m_topSubgraphPlug":{"$":"Plug","links":[]},
            "m_underSubgraphPlug":null
        });
        let graphs = load(&data).unwrap();
        let claim = graphs.graph(42).unwrap().state(0).unwrap();
        assert_eq!(claim.class, "StateStackClaim");
        assert!(claim.is_state() && !claim.is_networked());
        assert_eq!(claim.stack_value(), Some(&Value::Null));
        assert_eq!(
            claim.stack_claim(),
            Some((
                Variable {
                    scope: Scope::Entity,
                    id: 9
                },
                StackPriority {
                    level: -3,
                    above_equals: true
                },
                NonZeroU32::new(3),
            ))
        );
        for (field, bad_value) in [
            ("variable", json!({"scope":"entity","id":0})),
            ("priority", json!({"level":128,"above_equals":true})),
            ("duration_frames", json!(0)),
            ("m_topSubgraphPlug", json!({"$":"Plug","links":[[0,false]]})),
        ] {
            let mut bad = data.clone();
            bad["graphs"][0]["nodes"][0]["fields"][field] = bad_value;
            let error = load(&bad).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("stack claim state in graph 42 at node 0")
            );
        }
        let mut missing = data.clone();
        missing["graphs"][0]["nodes"][0]["fields"]
            .as_object_mut()
            .unwrap()
            .remove("value");
        assert_eq!(
            load(&missing).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );

        data["authored_classes"] =
            json!([{"class":"authored.claim","behavior":"stack_claim_state"}]);
        let node = &mut data["graphs"][0]["nodes"][0];
        node["class"] = json!("authored.claim");
        node["server_only"] = json!(false);
        node["state_behavior"] = Value::Null;
        node["fields"]["variable"]["scope"] = json!("instance");
        let graphs = load(&data).unwrap();
        let claim = graphs.graph(42).unwrap().state(0).unwrap();
        assert_eq!(claim.class, "StateStackClaim");
        assert!(claim.server_only && !claim.client_only);
        assert_eq!(claim.stack_claim().unwrap().0.scope, Scope::Instance);
    }

    #[test]
    fn track_entities_state_validates_child_bindings_after_loading_all_graphs() {
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let rejects = |data: &Value, expected: &str| {
            let error = load(data).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            let message = error.to_string();
            assert!(message.contains("graph 42 at node 0"), "{message}");
            assert!(message.contains(expected), "{message}");
        };

        let mut data = fixture();
        let mut child = data["graphs"][0].clone();
        child["index"] = json!(0);
        data["graphs"][0]["states"][3] = Value::Null;
        let node = &mut data["graphs"][0]["nodes"][0];
        node["state"] = json!(0);
        node["server_only"] = json!(true);
        node["state_behavior"] = json!("track_entities");
        node["fields"] = json!({
            "targets": {"$": "STUConfigVarDynamic", "m_identifier": 3, "m_60DB8F99": false},
            "child_graph": 0,
            "target_variable": u32::MAX,
        });
        rejects(&data, "references missing child_graph 0");
        data["graphs"].as_array_mut().unwrap().push(child);

        let graphs = load(&data).unwrap();
        let node = graphs.graph(42).unwrap().state(0).unwrap();
        assert_eq!(node.class, "StateTrackEntities");
        assert_eq!(node.target_child_binding(), Some((0, u32::MAX)));
        assert!(node.is_state() && !node.is_networked());

        for (field, invalid) in [
            ("targets", Value::Null),
            ("child_graph", json!(-1)),
            ("child_graph", json!(0.0)),
            ("child_graph", json!(u64::from(u32::MAX) + 1)),
            ("target_variable", json!(0)),
            ("target_variable", json!(true)),
            ("target_variable", json!(u64::from(u32::MAX) + 1)),
        ] {
            let mut bad = data.clone();
            bad["graphs"][0]["nodes"][0]["fields"][field] = invalid;
            rejects(&bad, field);
        }
        let mut bad_scope = data.clone();
        bad_scope["graphs"][0]["nodes"][0]["client_only"] = json!(true);
        rejects(
            &bad_scope,
            "server-only and occupy its state slot exactly once",
        );

        let node = &mut data["graphs"][0]["nodes"][0];
        node["state_behavior"] = Value::Null;
        node["class"] = json!("StateTrackEntities");
        let graphs = load(&data).unwrap();
        assert!(
            graphs
                .graph(42)
                .unwrap()
                .state(0)
                .unwrap()
                .target_child_binding()
                .is_none()
        );

        data["authored_classes"] =
            json!([{"class": "authored.members", "behavior": "track_entities_state"}]);
        let node = &mut data["graphs"][0]["nodes"][0];
        node["class"] = json!("authored.members");
        node["server_only"] = json!(false);
        let graphs = load(&data).unwrap();
        let node = graphs.graph(42).unwrap().state(0).unwrap();
        assert_eq!(node.class, "StateTrackEntities");
        assert_eq!(node.target_child_binding(), Some((0, u32::MAX)));
        assert!(node.server_only && !node.client_only);
    }

    #[test]
    fn dangling_link_report_preserves_unresolved_links_and_data_order() {
        let mut data = fixture();
        let links = &mut data["graphs"][0]["nodes"][0]["fields"]["entries"][0]["trigger"]["links"];
        *links = json!([
            [2, "input"],
            [null, "unresolved"],
            [3, "container"],
            [4, "empty slot"],
            [9, "outside table"],
        ]);
        let store = GraphStore::from_json(&serde_json::to_vec(&data).unwrap()).unwrap();
        let plug = "entries[0].trigger";
        assert_eq!(
            store.dangling_links().collect::<Vec<_>>(),
            [(3, 4), (4, 9)].map(|(link_index, target)| DanglingLink {
                graph: 42,
                source: 0,
                plug: plug.into(),
                link_index,
                target,
            })
        );
        let graph = store.graph(42).unwrap();
        assert_eq!(
            graph.node(0).unwrap().links(plug).collect::<Vec<_>>(),
            [
                (Some(2), "input"),
                (None, "unresolved"),
                (Some(3), "container"),
                (Some(4), "empty slot"),
                (Some(9), "outside table"),
            ]
        );
        assert_eq!(graph.owner_positions(), [0, 0]);
    }

    #[test]
    fn builds_wire_slots_and_keeps_data_order_through_holes() {
        let store = GraphStore::from_json(&serde_json::to_vec(&fixture()).unwrap()).unwrap();
        let graph = store.graph(42).unwrap();
        assert_eq!(graph.owner_positions(), [0, 0]);
        assert_eq!(graph.remote_positions(), [0]);
        assert_eq!(graph.owner_slot(0), Some(0));
        assert_eq!(graph.owner_slot(3), Some(0));
        assert_eq!(graph.owner_slot(1), None);
        assert_eq!(graph.remote_slot(0), None);
        assert_eq!(graph.remote_slot(3), Some(0));
        assert_eq!(graph.presence_bit(42), Some(1));
        assert_eq!(graph.presence_bit(12), None);
        assert_eq!(graph.graph_refs(), BTreeSet::from([26, 43]));
        assert_eq!(graph.by_rid(6).unwrap().pos, 2);
        assert_eq!(graph.node_ref(&json!({"node": 2})).unwrap().rid, 6);
        assert!(graph.node(3).is_none());

        let node = graph.node(0).unwrap();
        let plug_path = "entries[0].trigger";
        assert_eq!(node.plugs().keys().next().unwrap(), plug_path);
        assert_eq!(
            node.links(plug_path).collect::<Vec<_>>(),
            [(Some(2), "input"), (None, "unknown")]
        );
        for invalid in [
            "entries[-1].trigger",
            "entries[0]x",
            ".entries[0]",
            "entries[1]",
        ] {
            assert_eq!(node.field(invalid), None);
        }
        assert_eq!(store.missing(), [43]);
        assert_eq!(
            store.bodies()[&0x0400_0000_0000_0001].graphs[0].graph,
            Some(42)
        );
        assert_eq!(
            store.entity_body(0x0400_0000_0000_0002).unwrap().name,
            "Fixture"
        );
        assert_eq!(
            store.mode_variables(0x0230_0000_0000_0018).unwrap(),
            &BTreeMap::from([
                (1, ModeValue::Bool(false)),
                (2, ModeValue::Integer(3)),
                (3, ModeValue::Asset(0x0d80_0000_0000_195f)),
            ])
        );
    }

    #[test]
    fn reads_gzip_and_rejects_truncated_or_inconsistent_data() {
        use flate2::{Compression, write::GzEncoder};
        use std::io::Write;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder
            .write_all(&serde_json::to_vec(&fixture()).unwrap())
            .unwrap();
        let bytes = encoder.finish().unwrap();
        assert!(
            GraphStore::from_gzip(bytes.as_slice())
                .unwrap()
                .graph(42)
                .is_some()
        );
        assert!(GraphStore::from_gzip(&bytes[..bytes.len() - 4]).is_err());
        let mut bad = fixture();
        bad["graphs"][0]["nodes"][0]["pos"] = json!(6);
        assert!(GraphStore::from_json(&serde_json::to_vec(&bad).unwrap()).is_err());
        bad["version"] = json!(2);
        assert!(GraphStore::from_json(&serde_json::to_vec(&bad).unwrap()).is_err());
    }

    #[test]
    #[ignore = "requires a prepared 104319 state graph bundle"]
    fn whole_retail_wire_index_matches_the_python_reference() {
        use sha2::{Digest, Sha256};
        let root = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
        let store = GraphStore::load(root.join("data/game_graphs_174.json.gz")).unwrap();
        let mut digest = Sha256::new();
        fn sequence(digest: &mut Sha256, items: impl IntoIterator<Item = u64>) {
            let items: Vec<_> = items.into_iter().collect();
            digest.update((items.len() as u64).to_le_bytes());
            for item in items {
                digest.update(item.to_le_bytes());
            }
        }
        for index in store.graph_indexes() {
            let graph = store.graph(index).unwrap();
            sequence(
                &mut digest,
                [
                    u64::from(index),
                    u64::from(graph.items),
                    u64::from(graph.header.nodes_bits),
                    u64::from(graph.header.states_bits),
                    u64::from(graph.header.remote_bits),
                    u64::from(graph.header.sync_vars_bits),
                    graph.nodes.len() as u64,
                ],
            );
            sequence(
                &mut digest,
                graph.owner_positions().iter().copied().map(u64::from),
            );
            sequence(
                &mut digest,
                graph.remote_positions().iter().copied().map(u64::from),
            );
            sequence(
                &mut digest,
                graph.presence_vars().flat_map(|(index, var)| {
                    [
                        index as u64,
                        u64::from(var.0.unwrap()),
                        u64::from(var.1),
                        u64::from(var.2),
                    ]
                }),
            );
            sequence(&mut digest, graph.graph_refs());
            sequence(
                &mut digest,
                graph.unknown_rids.iter().copied().map(u64::from),
            );
        }
        assert_eq!(store.bodies().len(), 32);
        let expected = match store.graphs.len() {
            373 => "f54fb85e91d33b669c73239ddc77fb0f8f7fa6e6d0f04b83532bc1f953a4ac7c",
            405 => {
                assert_eq!(store.entities.len(), 8);
                assert_eq!(store.mode_variables.len(), 25);
                "eab09343964a53d44a53baa8d990ff9c2a3eea8922d9eb1927136fb7b661fc94"
            }
            699 => {
                assert_eq!(store.entities.len(), 6);
                assert_eq!(store.mode_variables.len(), 25);
                "585ab3b69f4eb0966dd755171b33b227f8eac7de595f6f5ec7392939c92229df"
            }
            700 => {
                assert_eq!(store.entities.len(), 6);
                assert_eq!(store.mode_variables.len(), 25);
                assert!(store.graph(0x0b92).is_some());
                assert_eq!(store.enemies.pairs.len(), 8);
                assert_eq!(store.enemies.entities.len(), 19);
                assert_eq!(
                    store.enemy_pair(0x02e0_0000_0000_0175),
                    Some(EnemyPair {
                        body: 0x0400_0000_0000_16be,
                        controller: 0x0400_0000_0000_16c0,
                    })
                );
                for (definition, expected) in [
                    (0x0400_0000_0000_16be, vec![0x0be2, 0x0c89, 0x0cb8, 0x0cbf]),
                    (0x0400_0000_0000_16c0, vec![0x0c87, 0x0cf3]),
                ] {
                    let roots =
                        crate::script_init::body_roots(store.enemy_body(definition).unwrap())
                            .unwrap();
                    assert_eq!(
                        roots.iter().map(|root| root.graph).collect::<Vec<_>>(),
                        expected
                    );
                    assert!(roots.iter().all(|root| store.graph(root.graph).is_some()));
                }
                "8834709ac62f67287938b1eb747a0039131abc73f6fd6568b008d8689c2d9c22"
            }
            count => panic!("no Python reference vector for the {count}-graph bundle"),
        };
        assert_eq!(format!("{:x}", digest.finalize()), expected);
    }
}
