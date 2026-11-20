//! Internal StateScript machine storage, queues, stacks, and variable links.

use std::{
    collections::{BTreeMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use indexmap::IndexMap;

use crate::{
    script_expr::StackPriority,
    script_graph::{EntryTopic, GraphStore, HostEventKind},
    script_modifiers::{Modifier, ModifierKey},
    script_runtime::{ButtonEdge, Timeline, frame_start},
    script_wire::{Value, Variables},
};

use super::{
    HostHealthContext, MAX_PENDING_EVENTS, RoomMessage,
    shapes::stored,
    types::{
        InitFault, PendingTimer, RandomCheckpoint, ShotEvent, Snapshot, SpawnSearchKey,
        StackBinding, StateKind,
    },
};

#[derive(Clone, Copy)]
enum MessageDestination {
    Ordinary(Option<u16>, u64),
    Room(u64),
    Host(HostEventKind),
}

impl MessageDestination {
    fn topic(self) -> EntryTopic {
        match self {
            Self::Ordinary(_, message) | Self::Room(message) => EntryTopic::Message(message),
            Self::Host(kind) => EntryTopic::Host(kind),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum VarKey {
    Entity(u32),
    Instance(u16, u32),
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct StateKey {
    pub(super) instance: u16,
    pub(super) raw: u16,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum RegisteredClass {
    CinematicAim,
    EntityControl,
    State9028E035,
    State43994F1B,
    State93B8758C,
}

#[derive(Clone, Copy)]
pub(super) struct RegisteredState {
    pub(super) class: RegisteredClass,
    pub(super) server_only: bool,
    pub(super) suppresses_model: bool,
}

#[derive(Clone)]
pub(super) enum LinkValue {
    Fixed(Value),
    Volley(u8),
    Chase,
    Cooldown,
    AbilityStack,
}

#[derive(Clone)]
pub(super) struct VariableLink {
    pub(super) state: StateKey,
    pub(super) priority: StackPriority,
    pub(super) output: LinkValue,
    pub(super) stack: bool,
}

pub(super) struct ArrayInsertion {
    pub(super) target: VarKey,
    pub(super) items: Vec<Value>,
    pub(super) index: Option<usize>,
    pub(super) unique: bool,
}

#[derive(Clone)]
pub(super) struct Volley {
    pub(super) start_ms: u64,
    pub(super) ammo0: i64,
    pub(super) per_shot: i64,
    pub(super) rate: f64,
    pub(super) spread: f64,
    pub(super) planned: u32,
    pub(super) reported: u32,
    pub(super) outputs: [Option<VarKey>; 3],
}

impl Volley {
    pub(super) fn ammo_at(&self, now_ms: u64) -> i64 {
        let Some(elapsed) = now_ms.checked_sub(self.start_ms) else {
            return self.ammo0;
        };
        let seconds = f64::from(elapsed as f32 * 0.001_f32);
        let shots = ((seconds * self.rate) as f32).floor() as i64;
        self.ammo0
            .saturating_sub(self.per_shot.saturating_mul(shots.saturating_add(1)))
            .max(0)
    }

    pub(super) fn output(&self, slot: u8, now_ms: u64) -> Value {
        match slot {
            0 => Value::Integer(self.ammo_at(now_ms)),
            1 => Value::Float(self.rate),
            2 => Value::Float(self.spread),
            _ => unreachable!("weapon volley output slot"),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct AbilityPress {
    pub(super) button: u8,
    pub(super) time_ms: u64,
    pub(super) activation: u16,
}

#[derive(Clone)]
pub(super) enum Event {
    Entry(u16),
    Assign(VarKey, Value),
    ExitInstance,
    Begin(u16),
    Finish(u16),
    Abort(u16),
    Changed(u16),
    Reevaluate(u16),
    Timer(u16, u16),
    Button(ButtonEdge),
    AbilityCheck(u16),
    CheckTransitions(u16),
    WatchChanged(u16),
    SpawnResult {
        key: SpawnSearchKey,
        positions: Vec<[f64; 3]>,
    },
    Message {
        position: u16,
        sender: Option<u32>,
        parameters: Arc<Variables>,
        health_context: HostHealthContext,
    },
}

pub(super) enum Startup {
    Passive,
    Ordinary,
    Played { auto_retire: bool },
}

impl Event {
    pub(super) fn state(&self) -> Option<u16> {
        match *self {
            Self::SpawnResult { key, .. } => Some(key.raw),
            Self::Entry(_)
            | Self::Assign(..)
            | Self::ExitInstance
            | Self::Message { .. }
            | Self::Button(_) => None,
            Self::Begin(state)
            | Self::Finish(state)
            | Self::Abort(state)
            | Self::Changed(state)
            | Self::Reevaluate(state)
            | Self::AbilityCheck(state)
            | Self::CheckTransitions(state)
            | Self::WatchChanged(state)
            | Self::Timer(state, _) => Some(state),
        }
    }
}

const SPLITMIX_STEP: u64 = 0x9e37_79b9_7f4a_7c15;
const SPLITMIX_STEP_INVERSE: u64 = 0xf1de_83e1_9937_733d;
const _: () = assert!(SPLITMIX_STEP.wrapping_mul(SPLITMIX_STEP_INVERSE) == 1);

pub(super) struct Machine {
    pub(super) public_schemas: bool,
    pub(super) code_states: Vec<StateKey>,
    pub(super) code_pending: [Vec<u64>; 2],
    pub(super) code_observers: Vec<(u64, Option<u64>)>,
    pub(super) snapshot: Snapshot,
    pub(super) registered: IndexMap<StateKey, RegisteredState>,
    pub(super) queues: Timeline<Event>,
    pub(super) watchers: BTreeMap<VarKey, Vec<StateKey>>,
    pub(super) foreign_watchers: BTreeMap<(u32, u32), Vec<StateKey>>,
    pub(super) ticking: BTreeMap<u16, IndexMap<u16, u64>>,
    pub(super) deferred: BTreeMap<u16, Vec<(f64, u16)>>,
    pub(super) voice_poll: Vec<StateKey>,
    pub(super) links: BTreeMap<VarKey, Vec<VariableLink>>,
    pub(super) volleys: BTreeMap<StateKey, Volley>,
    pub(super) movement_modifiers: BTreeMap<ModifierKey, Modifier>,
    pub(super) shots: Vec<ShotEvent>,
    pub(super) ability_presses: BTreeMap<StateKey, Vec<AbilityPress>>,
    next_public_id: u32,
    next_private_id: u32,
    pub(super) work_left: usize,
    pub(super) max_work: usize,
    pub(super) frame: u32,
    pub(super) now_ms: u64,
    random_initial_seed: u64,
    pub(super) random_state: AtomicU64,
    pub(super) command_buttons: u32,
    pub(super) held_buttons: Vec<u8>,
    pub(super) button_blocks: [usize; 256],
    pub(super) owner_dead: bool,
    pub(super) control_blocked: bool,
    pub(super) disabled_start_entries: HashSet<(u16, u16)>,
}

impl Clone for Machine {
    fn clone(&self) -> Self {
        Self {
            public_schemas: self.public_schemas,
            code_states: self.code_states.clone(),
            code_pending: self.code_pending.clone(),
            code_observers: self.code_observers.clone(),
            snapshot: self.snapshot.clone(),
            registered: self.registered.clone(),
            queues: self.queues.clone(),
            watchers: self.watchers.clone(),
            foreign_watchers: self.foreign_watchers.clone(),
            ticking: self.ticking.clone(),
            deferred: self.deferred.clone(),
            voice_poll: self.voice_poll.clone(),
            links: self.links.clone(),
            volleys: self.volleys.clone(),
            movement_modifiers: self.movement_modifiers.clone(),
            shots: self.shots.clone(),
            ability_presses: self.ability_presses.clone(),
            next_public_id: self.next_public_id,
            next_private_id: self.next_private_id,
            work_left: self.work_left,
            max_work: self.max_work,
            frame: self.frame,
            now_ms: self.now_ms,
            random_initial_seed: self.random_initial_seed,
            random_state: AtomicU64::new(self.random_state.load(Ordering::Relaxed)),
            command_buttons: self.command_buttons,
            held_buttons: self.held_buttons.clone(),
            button_blocks: self.button_blocks,
            owner_dead: self.owner_dead,
            control_blocked: self.control_blocked,
            disabled_start_entries: self.disabled_start_entries.clone(),
        }
    }
}

impl Machine {
    pub(super) fn random_u64(&self) -> u64 {
        // SplitMix64 keeps the component's stream reproducible across graph calls.
        let state = self
            .random_state
            .fetch_add(SPLITMIX_STEP, Ordering::Relaxed)
            .wrapping_add(SPLITMIX_STEP);
        let mixed = (state ^ (state >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        let mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        mixed ^ (mixed >> 31)
    }

    pub(super) fn random_checkpoint(&self) -> RandomCheckpoint {
        // The stream advances by a fixed odd step. Invert that step in
        // wrapping arithmetic to recover the count without a second counter.
        let state = self.random_state.load(Ordering::Relaxed);
        RandomCheckpoint {
            model: "splitmix64",
            initial_seed: self.random_initial_seed,
            draws: state
                .wrapping_sub(self.random_initial_seed)
                .wrapping_mul(SPLITMIX_STEP_INVERSE),
        }
    }

    pub(super) fn new(
        entity: u32,
        variables: Variables,
        frame: u32,
        max_work: usize,
    ) -> Result<Self, InitFault> {
        if max_work == 0 {
            return Err(InitFault::InvalidBudget);
        }
        let random_initial_seed = (u64::from(entity) << 32) | u64::from(frame);
        Ok(Self {
            code_states: Vec::new(),
            code_pending: Default::default(),
            code_observers: Vec::new(),
            snapshot: Snapshot {
                entity,
                now_ms: frame_start(frame),
                variables,
                instances: IndexMap::new(),
                pending_timers: IndexMap::new(),
                bindings: IndexMap::new(),
                resolved_bindings: IndexMap::new(),
                diagnostics: Vec::new(),
                #[cfg(feature = "node-observations")]
                node_observations: Default::default(),
            },
            queues: Timeline::new(MAX_PENDING_EVENTS),
            registered: IndexMap::new(),
            watchers: BTreeMap::new(),
            foreign_watchers: BTreeMap::new(),
            ticking: BTreeMap::new(),
            deferred: BTreeMap::new(),
            voice_poll: Vec::new(),
            links: BTreeMap::new(),
            volleys: BTreeMap::new(),
            movement_modifiers: BTreeMap::new(),
            shots: Vec::new(),
            ability_presses: BTreeMap::new(),
            next_public_id: 1,
            next_private_id: u32::from(u16::MAX),
            work_left: max_work,
            max_work,
            frame,
            now_ms: frame_start(frame),
            random_initial_seed,
            random_state: AtomicU64::new(random_initial_seed),
            command_buttons: 0,
            held_buttons: Vec::new(),
            button_blocks: [0; 256],
            owner_dead: false,
            control_blocked: false,
            disabled_start_entries: HashSet::new(),
            public_schemas: false,
        })
    }

    pub(super) fn next_instance_id(&self, isolated: bool) -> Result<u16, InitFault> {
        if self.next_public_id > self.next_private_id {
            return Err(InitFault::InstanceIdExhausted);
        }
        let id = if isolated {
            self.next_private_id
        } else {
            self.next_public_id
        };
        u16::try_from(id).map_err(|_| InitFault::InstanceIdExhausted)
    }

    pub(super) fn claim_instance_id(&mut self, id: u16, isolated: bool) -> Result<(), InitFault> {
        if id == 0 || self.snapshot.instances.contains_key(&id) {
            return Err(InitFault::InstanceIdExhausted);
        }
        let id = u32::from(id);
        if isolated {
            if id < self.next_public_id || id != self.next_private_id {
                return Err(InitFault::InstanceIdExhausted);
            }
            self.next_private_id = id - 1;
        } else {
            if id > self.next_private_id {
                return Err(InitFault::InstanceIdExhausted);
            }
            self.next_public_id = self.next_public_id.max(id + 1);
        }
        Ok(())
    }

    pub(super) fn capture_outputs(&mut self) {
        self.snapshot.now_ms = self.now_ms;
        self.snapshot.pending_timers.clear();
        for &id in self.snapshot.instances.keys() {
            let timers: Vec<_> = self
                .queues
                .iter(id)
                .filter_map(|(due_ms, event)| {
                    let (raw_state, finish, parameter) = match event {
                        Event::Timer(raw, param) => (*raw, false, *param),
                        Event::Finish(raw) => (*raw, true, 0),
                        _ => return None,
                    };
                    Some(PendingTimer {
                        due_ms,
                        raw_state,
                        finish,
                        parameter,
                    })
                })
                .collect();
            if !timers.is_empty() {
                self.snapshot.pending_timers.insert(id, timers);
            }
        }
        self.snapshot.bindings.clear();
        self.snapshot.resolved_bindings.clear();
        for (&variable, entries) in &self.links {
            let key = match variable {
                VarKey::Entity(id) => (0, id),
                VarKey::Instance(instance, id) => (instance, id),
            };
            let Some(top) = entries.last() else { continue };
            let bindings: Vec<_> = entries
                .iter()
                .filter(|entry| entry.stack)
                .map(|entry| StackBinding {
                    instance: entry.state.instance,
                    raw_state: entry.state.raw,
                    priority: entry.priority,
                    slot: u8::from(matches!(entry.output, LinkValue::AbilityStack)),
                })
                .collect();
            if !bindings.is_empty() {
                self.snapshot.bindings.insert(key, bindings);
            }
            let resolved = self.link_value(top);
            self.snapshot.resolved_bindings.insert(key, resolved);
        }
    }

    pub(super) fn queue_entries(
        &mut self,
        graphs: &GraphStore,
        recipient: Option<u16>,
        message: u64,
        parameters: Variables,
        due_ms: u64,
    ) -> Result<usize, InitFault> {
        self.queue_entries_from(graphs, recipient, None, message, parameters, due_ms)
    }

    pub(super) fn queue_entries_from(
        &mut self,
        graphs: &GraphStore,
        recipient: Option<u16>,
        sender: Option<u32>,
        message: u64,
        parameters: Variables,
        due_ms: u64,
    ) -> Result<usize, InitFault> {
        self.queue_message_entries(
            graphs,
            MessageDestination::Ordinary(recipient, message),
            sender,
            parameters,
            due_ms,
            HostHealthContext::default(),
        )
    }

    pub(super) fn queue_local_game_message(
        &mut self,
        graphs: &GraphStore,
        message: u64,
        parameters: Variables,
        health_context: HostHealthContext,
    ) -> Result<usize, InitFault> {
        self.queue_message_entries(
            graphs,
            MessageDestination::Ordinary(None, message),
            Some(self.snapshot.entity),
            parameters,
            self.now_ms,
            health_context,
        )
    }

    pub(super) fn queue_room_entries_from(
        &mut self,
        graphs: &GraphStore,
        sender: u32,
        message: RoomMessage,
        due_ms: u64,
    ) -> Result<usize, InitFault> {
        self.queue_message_entries(
            graphs,
            MessageDestination::Room(message.id),
            Some(sender),
            message.parameters,
            due_ms,
            HostHealthContext::default(),
        )
    }

    pub(super) fn queue_host_entries(
        &mut self,
        graphs: &GraphStore,
        kind: HostEventKind,
        parameters: Variables,
        due_ms: u64,
        health_context: HostHealthContext,
    ) -> Result<usize, InitFault> {
        self.queue_message_entries(
            graphs,
            MessageDestination::Host(kind),
            None,
            parameters,
            due_ms,
            health_context,
        )
    }

    fn queue_message_entries(
        &mut self,
        graphs: &GraphStore,
        destination: MessageDestination,
        sender: Option<u32>,
        parameters: Variables,
        due_ms: u64,
        health_context: HostHealthContext,
    ) -> Result<usize, InitFault> {
        let topic = destination.topic();
        if matches!(topic, EntryTopic::Message(id) if graphs.is_projected_message_topic(id)) {
            return Ok(0);
        }
        let mut entries = Vec::new();
        for (&id, item) in &self.snapshot.instances {
            if matches!(destination, MessageDestination::Ordinary(Some(recipient), _) if recipient != id)
            {
                continue;
            }
            let graph = graphs.graph(item.graph).expect("registered graph");
            for node in graph
                .entry_handlers(topic)
                .filter(|node| match destination {
                    MessageDestination::Host(_) => true,
                    MessageDestination::Room(_) => {
                        node.declares_room_message_entry()
                            || node.accepts_directed_numeric_message(&parameters)
                    }
                    MessageDestination::Ordinary(_, _) => {
                        !node.declares_room_message_entry()
                            && (node.is_message_entry()
                                || node.state.is_some_and(|raw| {
                                    item.states.get(&raw).is_some_and(|state| {
                                        state.active && state.kind == StateKind::MessageIdle
                                    })
                                }))
                    }
                })
            {
                // Bound the temporary batch as well as the VM queue. No
                // receiver gets a partial delivery on a capacity failure.
                self.queues.reserve(entries.len() + 1)?;
                entries.push((id, node.pos));
            }
        }
        let parameters = Arc::new(parameters);
        for &(instance, position) in &entries {
            self.queues.push(
                instance,
                due_ms,
                Event::Message {
                    position,
                    sender,
                    parameters: Arc::clone(&parameters),
                    health_context: health_context.clone(),
                },
            )?;
        }
        Ok(entries.len())
    }

    pub(super) fn value(&self, key: VarKey, below: Option<StateKey>) -> Value {
        self.links
            .get(&key)
            .and_then(|entries| {
                entries
                    .iter()
                    .take_while(|entry| Some(entry.state) != below)
                    .last()
            })
            .map_or_else(
                || stored(&self.snapshot, key),
                |entry| self.link_value(entry),
            )
    }

    pub(super) fn link_value(&self, entry: &VariableLink) -> Value {
        match &entry.output {
            LinkValue::Fixed(value) => value.clone(),
            LinkValue::Volley(slot) => self.volleys[&entry.state].output(*slot, self.now_ms),
            LinkValue::Chase => self.snapshot.instances[&entry.state.instance].states
                [&entry.state.raw]
                .chase
                .as_ref()
                .expect("linked chase state")
                .current
                .clone(),
            LinkValue::Cooldown => Value::Float(
                self.snapshot.instances[&entry.state.instance].states[&entry.state.raw]
                    .ability_cooldown
                    .expect("linked cooldown state")
                    .remaining(self.now_ms),
            ),
            LinkValue::AbilityStack => Value::Bool(true),
        }
    }

    pub(super) fn allows_stack(
        &self,
        key: VarKey,
        priority: StackPriority,
        exclude: Option<StateKey>,
    ) -> bool {
        self.links.get(&key).is_none_or(|entries| {
            entries
                .iter()
                .filter(|entry| Some(entry.state) != exclude)
                .all(|entry| {
                    entry.priority.level < priority.level
                        || (entry.priority.level == priority.level && priority.above_equals)
                })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_and_projected_instance_ids_meet_without_reuse() {
        let mut machine = Machine::new(1, Variables::new(), 0, 100).unwrap();
        assert_eq!(machine.next_instance_id(false), Ok(1));
        assert_eq!(machine.next_instance_id(true), Ok(u16::MAX));

        machine.claim_instance_id(u16::MAX - 2, false).unwrap();
        machine.claim_instance_id(u16::MAX, true).unwrap();

        let remaining = u16::MAX - 1;
        assert_eq!(machine.next_instance_id(false), Ok(remaining));
        assert_eq!(machine.next_instance_id(true), Ok(remaining));
        machine.claim_instance_id(remaining, false).unwrap();

        assert_eq!(
            machine.next_instance_id(false),
            Err(InitFault::InstanceIdExhausted)
        );
        assert_eq!(
            machine.next_instance_id(true),
            Err(InitFault::InstanceIdExhausted)
        );
        assert!(
            machine.snapshot.instances.is_empty(),
            "retirement must not make an allocated ID available"
        );
        assert_eq!(
            machine.clone().next_instance_id(true),
            Err(InitFault::InstanceIdExhausted)
        );
    }
}
