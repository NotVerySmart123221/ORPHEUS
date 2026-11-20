//! Initialization and supported frame events of authored statescript graphs.
//!
//! This covers Entry, typed variable writes, conditional links and static
//! SubScript instances. Diagnostics make a partial snapshot unsuitable for
//! gameplay replication. The component also advances static waits and dispatches
//! scoped message entries. Input, combat and other authored actions still need
//! their corresponding runtime.

use std::{collections::BTreeMap, sync::Arc};

use orpheus_wire::commands::Command;
use serde_json::Value as Json;

use crate::{
    script_expr::{self, EmptyContext, EvalError, StackPriority, Variable},
    script_graph::{Body, GraphBinding, GraphStore, Node, asset_guid},
    script_modifiers::{Modifier, ModifierKey},
    script_runtime::{FRAME_MS, frame_start},
    script_wire::{Value, Variables},
};

pub const MAX_INIT_WORK: usize = 100_000;
const MAX_PLUG_DEPTH: usize = 64;
const MAX_EVENTS_PER_UPDATE: usize = 1000;
const MAX_PENDING_EVENTS: usize = 16_384;
const MAX_PENDING_SHOTS: usize = 256;
const MAX_PLAY_INSTANCES: usize = 1500;
const SCRIPT_SOURCE: u32 = 0x1b;
const ABILITY_RUNNING: u8 = 1;
const ABILITY_PUSHED: u8 = 2;
const ABILITY_HELD: u8 = 4;
const SWITCH_TO_TRUE: u16 = 1;
const SWITCH_TO_FALSE: u16 = 2;
const LINK_PRIORITY: StackPriority = StackPriority {
    level: 0,
    above_equals: true,
};
#[doc(hidden)]
pub const ABORT_PLUG: &str = "m_F198FD3A";
const SUBGRAPH_STATES: &str = "m_FF3DAF1E";
const MOVEMENT_BOOLEAN_OUTPUTS: [&str; 12] = [
    "m_105DF1D6",
    "m_49F4B33C",
    "m_56AC7B36",
    "m_16BC4759",
    "m_F620FC2A",
    "m_6306AA29",
    "m_103A8581",
    "m_958FF5C0",
    "m_4C55BB03",
    "m_2EB48682",
    "m_45EB431F",
    "m_BF152E27",
];

#[derive(Clone, Debug)]
pub struct Root {
    pub id: u16,
    pub graph: u32,
    pub variables: Variables,
    run_startup: bool,
    initial_state: Option<u16>,
    server_only: bool,
}

impl Root {
    pub fn new(id: u16, graph: u32) -> Self {
        Self {
            id,
            graph,
            variables: Variables::new(),
            run_startup: true,
            initial_state: None,
            server_only: false,
        }
    }

    /// Mount a private root. The VM assigns its instance ID separately.
    pub fn server_only(graph: u32) -> Self {
        Self {
            server_only: true,
            ..Self::new(0, graph)
        }
    }

    pub fn is_server_only(&self) -> bool {
        self.server_only
    }

    #[doc(hidden)]
    pub fn listener(id: u16, graph: u32, initial_state: Option<u16>) -> Self {
        Self {
            run_startup: false,
            initial_state,
            ..Self::new(id, graph)
        }
    }

    fn with_binding(mut self, binding: &GraphBinding) -> Result<Self, InitFault> {
        for field in binding.sync.iter().flatten() {
            let key = field
                .get("m_0D09D2D9")
                .and_then(asset_guid)
                .map_or(0, |guid| guid as u16);
            let value = script_expr::evaluate(
                field.get("m_value").unwrap_or(&Json::Null),
                &mut EmptyContext,
            )
            .map_err(|error| InitFault::RootBinding {
                id: self.id,
                graph: self.graph,
                error,
            })?;
            self.variables.insert(u32::from(key), value);
        }
        Ok(self)
    }
}

fn public_body_roots(body: &Body) -> Result<Vec<Root>, InitFault> {
    let mut roots = Vec::new();
    for (offset, binding) in body.graphs.iter().enumerate() {
        if let Some(graph) = binding.graph {
            let id = u16::try_from(offset + 1).map_err(|_| InitFault::InstanceIdExhausted)?;
            roots.push(Root::new(id, graph).with_binding(binding)?);
        }
    }
    Ok(roots)
}

fn append_server_body_roots(body: &Body, roots: &mut Vec<Root>) -> Result<(), InitFault> {
    for binding in &body.server_graphs {
        if let Some(graph) = binding.graph {
            roots.push(Root::server_only(graph).with_binding(binding)?);
        }
    }
    Ok(())
}

/// Initial body bindings use their asset order. Private roots follow those bindings.
pub fn body_roots(body: &Body) -> Result<Vec<Root>, InitFault> {
    let mut roots = public_body_roots(body)?;
    append_server_body_roots(body, &mut roots)?;
    Ok(roots)
}

/// Body bindings, manager and equipped weapon slots in asset order, then private roots.
pub fn player_roots(body: &Body, graphs: &GraphStore) -> Result<Vec<Root>, InitFault> {
    let mut roots = public_body_roots(body)?;
    let mut id = roots.last().map_or(0, |root| root.id);
    let mut append = |graph, slot: Option<usize>| {
        id = id.checked_add(1).ok_or(InitFault::InstanceIdExhausted)?;
        let mut root = Root::new(id, graph);
        if let Some(slot) = slot {
            root.variables.insert(28, Value::Integer(slot as i64));
        }
        roots.push(root);
        Ok::<_, InitFault>(())
    };
    if let Some(manager) = body.manager {
        append(manager, None)?;
    }
    for (slot, graph) in body.weapons.iter().enumerate() {
        if let Some(graph) = graph {
            append(*graph, Some(slot))?;
        }
    }
    append_server_body_roots(body, &mut roots)?;
    if let Some(root) = roots.iter().find(|root| graphs.graph(root.graph).is_none()) {
        return Err(InitFault::MissingPlayerGraph(root.graph));
    }
    Ok(roots)
}

#[doc(hidden)]
pub const OWNER_HEALTH: u32 = 253;
const HUD_HEALTH: u32 = 1732;
#[doc(hidden)]
pub const HUD_MAX_HEALTH: u32 = 1737;

pub fn player_entity_variables(body: &Body, health: f64, maximum_health: f64) -> Variables {
    let slots = body
        .weapons
        .iter()
        .map(|weapon| Value::Bool(weapon.is_some()))
        .collect();
    let first = body.weapons.iter().position(Option::is_some).unwrap_or(0);
    Variables::from([
        (29, Value::Array(slots)),
        (16_515, Value::Integer(first as i64)),
        (OWNER_HEALTH, Value::Float(health)),
        (HUD_HEALTH, Value::Float(health)),
        (HUD_MAX_HEALTH, Value::Float(maximum_health)),
        (342, Value::Float(0.0)),
    ])
}

mod machine;
mod shapes;
#[cfg(test)]
mod tests;
mod types;
mod vm;
mod voice;
mod world;

pub use types::{
    AbilityCooldown, ChasePayload, Diagnostic, InitFault, Instance, Issue, NodeObservations,
    PendingTimer, RandomCheckpoint, ShotEvent, Snapshot, SpawnSearch, SpawnSearchKey, StackBinding,
    State, StateKind,
};
pub use world::{
    AiBehaviorRequest, AiControlOverride, AiFacing, AiWeaponOverride, BeamContact, CreationKind,
    EmptyWorld, EntityCreation, HealthTargetFacts, HitTags, HostEvent, LandingSelection,
    MovementSample, PlayScript, RoomCue, RoomEntityStatus, RoomMessage, Site, TargetTagEffect,
    TargetTagEffects, TargetTagSnapshot, World,
};

use machine::{Event, Machine, Startup, StateKey, VarKey};
use shapes::source_key;
use vm::Vm;
use voice::InstanceContext;

/// A single body using the same authored graph engine as one-shot initialization.
#[derive(Clone)]
pub struct Component {
    graphs: Arc<GraphStore>,
    machine: Machine,
    halted: bool,
}

impl Component {
    /// Release host effects and queued work, including after a fatal VM fault.
    pub fn teardown(&mut self, world: &mut impl World) -> Result<(), InitFault> {
        let notify_observers = !self.halted;
        self.run_with_budget(world, |vm| {
            let roots = vm
                .machine
                .snapshot
                .instances
                .iter()
                .filter_map(|(&id, instance)| instance.parent.is_none().then_some(id))
                .collect::<Vec<_>>();
            for root in roots {
                vm.destroy(root)?;
            }
            if notify_observers {
                vm.notify_code(1)?;
            }
            Ok(())
        })
    }
    pub fn register_code_observer(&mut self, token: u64, identifier: Option<u64>) {
        if !self.machine.code_observers.contains(&(token, identifier)) {
            self.machine.code_observers.push((token, identifier));
        }
    }

    pub fn unregister_code_observer(&mut self, token: u64) {
        self.machine
            .code_observers
            .retain(|(registered, _)| *registered != token);
    }

    pub fn code_value(
        &mut self,
        identifier: u64,
        initial: Value,
        world: &mut impl World,
    ) -> Result<(bool, Value), InitFault> {
        let mut result = None;
        self.run(world, |vm| {
            result = Some(vm.code_value(identifier, initial)?);
            Ok(())
        })?;
        Ok(result.expect("completed code value operation"))
    }
    pub fn start(
        graphs: Arc<GraphStore>,
        entity: u32,
        entity_variables: Variables,
        roots: Vec<Root>,
        frame: u32,
        world: &mut impl World,
        max_work: usize,
    ) -> Result<Self, InitFault> {
        Self::start_with_options(
            graphs,
            entity,
            entity_variables,
            roots,
            frame,
            world,
            (max_work, false),
        )
    }

    pub fn start_with_public_schema(
        graphs: Arc<GraphStore>,
        entity: u32,
        entity_variables: Variables,
        roots: Vec<Root>,
        frame: u32,
        world: &mut impl World,
        max_work: usize,
    ) -> Result<Self, InitFault> {
        Self::start_with_options(
            graphs,
            entity,
            entity_variables,
            roots,
            frame,
            world,
            (max_work, true),
        )
    }

    fn start_with_options(
        graphs: Arc<GraphStore>,
        entity: u32,
        entity_variables: Variables,
        roots: Vec<Root>,
        frame: u32,
        world: &mut impl World,
        (max_work, public_schemas): (usize, bool),
    ) -> Result<Self, InitFault> {
        let mut machine = Machine::new(entity, entity_variables, frame, max_work)?;
        machine.public_schemas = public_schemas;
        Vm::new(&graphs, world, &mut machine).start(roots)?;
        machine.capture_outputs();
        Ok(Self {
            graphs,
            machine,
            halted: false,
        })
    }

    pub fn snapshot(&self) -> Option<&Snapshot> {
        (!self.halted).then_some(&self.machine.snapshot)
    }

    /// Inspect the component's random stream without drawing from it.
    pub fn random_checkpoint(&self) -> RandomCheckpoint {
        self.machine.random_checkpoint()
    }

    pub fn graph_store(&self) -> &GraphStore {
        &self.graphs
    }

    pub fn now_ms(&self) -> u64 {
        self.machine.now_ms
    }

    pub fn spawn_searches(&self) -> impl Iterator<Item = (SpawnSearchKey, SpawnSearch)> + '_ {
        self.snapshot()
            .into_iter()
            .flat_map(|snapshot| &snapshot.instances)
            .flat_map(|(&instance, state)| {
                state.states.iter().filter_map(move |(&raw, state)| {
                    let search = state.spawn_search.filter(|_| state.active)?;
                    Some((
                        SpawnSearchKey {
                            instance,
                            raw,
                            activation: state.activation,
                            start_ms: state.start_ms,
                        },
                        search,
                    ))
                })
            })
    }

    pub fn queue_spawn_result(
        &mut self,
        frame: u32,
        key: SpawnSearchKey,
        positions: Vec<[f64; 3]>,
    ) -> Result<(), InitFault> {
        if self.halted {
            return Err(InitFault::Halted);
        }
        let active = self
            .machine
            .snapshot
            .instances
            .get(&key.instance)
            .and_then(|instance| instance.states.get(&key.raw))
            .is_some_and(|state| {
                state.active
                    && state.activation == key.activation
                    && state.start_ms == key.start_ms
                    && state.spawn_search.is_some()
            });
        if !active {
            return Ok(());
        }
        let due_ms = frame_start(frame);
        if due_ms < self.machine.now_ms {
            return Err(InitFault::FrameOrder {
                frame,
                now_ms: self.machine.now_ms,
            });
        }
        self.machine
            .queues
            .push(key.instance, due_ms, Event::SpawnResult { key, positions })?;
        Ok(())
    }

    /// Consume reports only after the body graph path is ready for authority.
    pub fn take_shots(&mut self) -> Option<Vec<ShotEvent>> {
        let shots = std::mem::take(&mut self.machine.shots);
        self.snapshot()?.initialization_complete().then_some(shots)
    }

    pub fn shot_node(&self, shot: &ShotEvent) -> Option<&Node> {
        let snapshot = self.snapshot()?;
        let instance = snapshot.instances.get(&shot.instance)?;
        if snapshot.entity != shot.entity || instance.graph != shot.graph {
            return None;
        }
        self.graphs
            .graph(shot.graph)?
            .state(usize::from(shot.raw_state))
            .filter(|node| node.class == "STUStatescriptStateWeaponVolley")
    }

    pub fn evaluate_shot_config(
        &self,
        shot: &ShotEvent,
        config: &Json,
        world: &mut impl World,
    ) -> Result<Value, EvalError> {
        self.shot_node(shot)
            .ok_or(EvalError::MalformedConfig("shot source"))?;
        self.evaluate_state_config(shot.instance, shot.raw_state, config, world)
    }

    pub fn evaluate_state_config(
        &self,
        instance: u16,
        raw_state: u16,
        config: &Json,
        world: &mut impl World,
    ) -> Result<Value, EvalError> {
        let snapshot = self
            .snapshot()
            .ok_or(EvalError::MalformedConfig("state source"))?;
        let graph = snapshot
            .instances
            .get(&instance)
            .ok_or(EvalError::MalformedConfig("state source"))?
            .graph;
        let node = self
            .graphs
            .graph(graph)
            .and_then(|graph| graph.state(usize::from(raw_state)))
            .ok_or(EvalError::MalformedConfig("state source"))?;
        let mut context = InstanceContext {
            machine: &self.machine,
            world,
            site: Site {
                entity: snapshot.entity,
                instance,
                graph,
                node: Some(node.pos),
            },
            reads: Vec::new(),
            foreign_reads: Vec::new(),
            parameters: None,
            health_target_facts: None,
            watcher: None,
            read_native: false,
            untrusted_native: false,
        };
        script_expr::evaluate(config, &mut context)
    }

    pub fn resolved_variable(&self, instance: u16, variable: Variable) -> Option<Value> {
        (!self.halted).then(|| self.machine.value(source_key(instance, variable), None))
    }

    pub fn permits_entity_stack(&self, id: u32, priority: StackPriority) -> Option<bool> {
        (!self.halted).then(|| {
            self.machine
                .allows_stack(VarKey::Entity(id), priority, None)
        })
    }

    pub fn set_entity_variable(
        &mut self,
        id: u32,
        value: Value,
        world: &mut impl World,
    ) -> Result<(), InitFault> {
        self.run(world, |vm| {
            let key = VarKey::Entity(id);
            let old = vm.machine.value(key, None);
            vm.set_base(key, value);
            vm.notify_value(key, &old, None)
        })
    }

    #[doc(hidden)]
    pub fn set_instance_variable(
        &mut self,
        instance: u16,
        id: u32,
        value: Value,
        world: &mut impl World,
    ) -> Result<(), InitFault> {
        self.run(world, |vm| {
            let key = VarKey::Instance(instance, id);
            let old = vm.machine.value(key, None);
            vm.set_base(key, value);
            vm.notify_value(key, &old, None)
        })
    }

    #[doc(hidden)]
    pub fn begin_state(
        &mut self,
        instance: u16,
        raw: u16,
        world: &mut impl World,
    ) -> Result<(), InitFault> {
        self.run(world, |vm| vm.begin(machine::StateKey { instance, raw }, 0))
    }

    /// Read the rows published by state lifecycle and dependency events.
    /// Authoritative modifiers require a fully supported runtime path.
    pub fn movement_modifiers(
        &mut self,
        _world: &mut impl World,
    ) -> Result<Option<BTreeMap<ModifierKey, Modifier>>, InitFault> {
        if self.halted {
            return Err(InitFault::Halted);
        }
        Ok(self
            .machine
            .snapshot
            .initialization_complete()
            .then(|| self.machine.movement_modifiers.clone()))
    }

    /// Queue the authored entries of one instance at the current clock.
    pub fn queue_message(
        &mut self,
        instance: u16,
        message: u64,
        parameters: Variables,
    ) -> Result<usize, InitFault> {
        self.queue_message_at(self.machine.now_ms, instance, message, parameters)
    }

    pub fn queue_frame_message(
        &mut self,
        frame: u32,
        instance: u16,
        message: u64,
        parameters: Variables,
    ) -> Result<usize, InitFault> {
        let due_ms = frame_start(frame);
        if due_ms < self.machine.now_ms {
            return Err(InitFault::FrameOrder {
                frame,
                now_ms: self.machine.now_ms,
            });
        }
        self.queue_message_at(due_ms, instance, message, parameters)
    }

    /// Publish a real foreign component slot change at the room frame boundary.
    pub fn queue_foreign_variable_change(
        &mut self,
        frame: u32,
        entity: u32,
        variable: u32,
    ) -> Result<(), InitFault> {
        let due = frame_start(frame);
        if due < self.machine.now_ms {
            return Err(InitFault::FrameOrder {
                frame,
                now_ms: self.machine.now_ms,
            });
        }
        if self.halted {
            return Err(InitFault::Halted);
        }
        let watchers = self
            .machine
            .foreign_watchers
            .get(&(entity, variable))
            .cloned()
            .unwrap_or_default();
        for key in watchers {
            let Some(state) = self
                .machine
                .snapshot
                .instances
                .get_mut(&key.instance)
                .and_then(|instance| instance.states.get_mut(&key.raw))
                .filter(|state| state.active && !state.ending && !state.pending_change)
            else {
                continue;
            };
            self.machine
                .queues
                .push(key.instance, due, Event::Changed(key.raw))?;
            state.pending_change = true;
        }
        Ok(())
    }

    pub fn queue_frame_message_from(
        &mut self,
        frame: u32,
        sender: u32,
        message: u64,
        parameters: Variables,
    ) -> Result<usize, InitFault> {
        let due_ms = frame_start(frame);
        if due_ms < self.machine.now_ms {
            return Err(InitFault::FrameOrder {
                frame,
                now_ms: self.machine.now_ms,
            });
        }
        if self.halted {
            return Err(InitFault::Halted);
        }
        self.machine.queue_entries_from(
            &self.graphs,
            None,
            Some(sender),
            message,
            parameters,
            due_ms,
        )
    }

    /// Schedule an authored room message at the start of the next VM frame.
    pub fn queue_room_message_from(
        &mut self,
        sender: u32,
        message: RoomMessage,
    ) -> Result<usize, InitFault> {
        if self.halted {
            return Err(InitFault::Halted);
        }
        if !script_expr::valid_entity(sender)
            || sender == self.machine.snapshot.entity
            || !message.is_valid()
        {
            return Ok(0);
        }
        let Some(due_ms) = self.next_vm_frame_ms() else {
            return Ok(0);
        };
        self.machine
            .queue_room_entries_from(&self.graphs, sender, message, due_ms)
    }

    /// Notify explicitly subscribed server graphs at the next receiver VM frame.
    pub fn queue_host_event(&mut self, event: HostEvent) -> Result<usize, InitFault> {
        self.queue_host_event_with_target_facts(event, None)
    }

    /// Deliver facts for the event's original target with its server notification.
    /// Facts are available only to immediate foreign queries during delivery.
    pub fn queue_host_event_with_target_facts(
        &mut self,
        event: HostEvent,
        target_facts: Option<Arc<HealthTargetFacts>>,
    ) -> Result<usize, InitFault> {
        if self.halted {
            return Err(InitFault::Halted);
        }
        if target_facts
            .as_ref()
            .is_some_and(|facts| facts.target != event.target())
        {
            return Ok(0);
        }
        let Some(due_ms) = self.next_vm_frame_ms() else {
            return Ok(0);
        };
        let Some((kind, parameters)) = event.bindings_for(self.machine.snapshot.entity) else {
            return Ok(0);
        };
        self.machine
            .queue_host_entries(&self.graphs, kind, parameters, due_ms, target_facts)
    }

    fn next_vm_frame_ms(&self) -> Option<u64> {
        u32::try_from(self.machine.now_ms / FRAME_MS + 1)
            .ok()
            .map(frame_start)
    }

    fn queue_message_at(
        &mut self,
        due_ms: u64,
        instance: u16,
        message: u64,
        parameters: Variables,
    ) -> Result<usize, InitFault> {
        if self.halted {
            return Err(InitFault::Halted);
        }
        self.machine
            .queue_entries(&self.graphs, Some(instance), message, parameters, due_ms)
    }

    pub fn run_now(&mut self, world: &mut impl World) -> Result<(), InitFault> {
        self.run(world, |vm| vm.run())
    }

    pub fn refresh_boss_pause(
        &mut self,
        world: &mut impl World,
        room_pause: bool,
    ) -> Result<(), InitFault> {
        let snapshot = &self.machine.snapshot;
        let pause = room_pause || snapshot.boss_pause_active();
        let receivers: Vec<_> = snapshot.instances.iter().filter_map(|(&id, instance)| {
            let raw = 56;
            let state = instance.states.get(&raw)?;
            (instance.graph == 0x0cbc
                && state.active
                && matches!(state.kind, StateKind::BossPauseReceiver { receiving } if receiving != pause))
                .then_some(StateKey { instance: id, raw })
        }).collect();
        if receivers.is_empty() {
            return Ok(());
        }
        self.run(world, |vm| {
            for key in receivers {
                vm.spend("Boss pause receiver")?;
                let node = vm
                    .graph(key.instance)
                    .state(usize::from(key.raw))
                    .expect("Boss pause receiver");
                let (exit, enter) = if pause {
                    ("m_33DA5163", "m_452862E6")
                } else {
                    ("m_452862E6", "m_33DA5163")
                };
                vm.exit_subgraph(key.instance, node, exit, 1)?;
                vm.state_mut(key).expect("active Boss pause receiver").kind =
                    StateKind::BossPauseReceiver { receiving: pause };
                vm.follow(key.instance, node, enter, Some(key), 0)?;
            }
            vm.run()
        })
    }

    pub fn receive_play_script(
        &mut self,
        caller: Site,
        request: &PlayScript,
        world: &mut impl World,
    ) -> Result<(), InitFault> {
        self.run(world, |vm| {
            vm.start_loaded_script(caller, request)?;
            vm.run()
        })
    }

    pub fn attach_witch_staff(
        &mut self,
        source: u32,
        amount: f64,
        world: &mut impl World,
    ) -> Result<u16, InitFault> {
        let id = self.machine.next_instance_id(false)?;
        self.run(world, |vm| {
            let variables = Variables::from([(SCRIPT_SOURCE, Value::Entity(source))]);
            vm.create(
                id,
                0x0079,
                None,
                variables,
                Startup::Ordinary,
                types::InstanceScope::Projected,
            )?;
            vm.machine.queue_entries_from(
                vm.graphs,
                Some(id),
                Some(source),
                0x0240_0000_0000_000f,
                Variables::from([(42, Value::Float(amount))]),
                vm.machine.now_ms,
            )?;
            vm.run()
        })?;
        Ok(id)
    }

    pub fn detach_witch_staff(&mut self, id: u16, world: &mut impl World) -> Result<(), InitFault> {
        self.run(world, |vm| {
            if vm
                .machine
                .snapshot
                .instances
                .get(&id)
                .is_some_and(|instance| instance.graph == 0x0079)
            {
                vm.destroy(id)?;
            }
            vm.run()
        })
    }

    pub fn queue_entity_message(
        &mut self,
        message: u64,
        parameters: Variables,
    ) -> Result<usize, InitFault> {
        self.queue_entity_message_at(None, message, parameters)
    }

    pub fn queue_entity_message_from(
        &mut self,
        sender: u32,
        message: u64,
        parameters: Variables,
    ) -> Result<usize, InitFault> {
        if !script_expr::valid_entity(sender) {
            return self.queue_entity_message(message, parameters);
        }
        self.queue_entity_message_at(Some(sender), message, parameters)
    }

    fn queue_entity_message_at(
        &mut self,
        sender: Option<u32>,
        message: u64,
        parameters: Variables,
    ) -> Result<usize, InitFault> {
        if self.halted {
            return Err(InitFault::Halted);
        }
        self.machine.queue_entries_from(
            &self.graphs,
            None,
            sender,
            message,
            parameters,
            self.machine.now_ms,
        )
    }

    pub fn queue_entity_frame_message(
        &mut self,
        frame: u32,
        message: u64,
        parameters: Variables,
    ) -> Result<usize, InitFault> {
        let due_ms = frame_start(frame);
        if self.halted {
            return Err(InitFault::Halted);
        }
        if due_ms < self.machine.now_ms {
            return Err(InitFault::FrameOrder {
                frame,
                now_ms: self.machine.now_ms,
            });
        }
        self.machine
            .queue_entries(&self.graphs, None, message, parameters, due_ms)
    }

    pub fn gate_approach_instance(&self) -> Option<u16> {
        let snapshot = self.snapshot()?;
        if !snapshot.initialization_complete() {
            return None;
        }
        snapshot.instances.iter().find_map(|(&id, instance)| {
            (instance.graph == 0x0c87 && instance.retire_after_frame.is_none()).then_some(id)
        })
    }

    pub fn queue_gate_arrival(&mut self, frame: u32, instance: u16) -> Result<(), InitFault> {
        let due_ms = frame_start(frame);
        if due_ms < self.machine.now_ms {
            return Err(InitFault::FrameOrder {
                frame,
                now_ms: self.machine.now_ms,
            });
        }
        self.machine
            .queues
            .push(instance, due_ms, Event::Entry(20))?;
        Ok(())
    }

    pub fn has_polled_voice(&self) -> bool {
        !self.machine.voice_poll.is_empty()
    }

    /// Explicit +80 phase, separate from ordinary graph ticks and deferred callbacks.
    pub fn poll_voice(&mut self, world: &mut impl World) -> Result<(), InitFault> {
        self.run(world, |vm| vm.poll_voice())
    }

    pub fn notify_voice_failure(
        &mut self,
        instance: u16,
        raw: u16,
        world: &mut impl World,
    ) -> Result<bool, InitFault> {
        let mut notified = false;
        self.run(world, |vm| {
            notified = vm.notify_voice_failure(StateKey { instance, raw })?;
            Ok(())
        })?;
        Ok(notified)
    }

    pub fn advance_frame(&mut self, frame: u32, world: &mut impl World) -> Result<(), InitFault> {
        self.advance(frame, None, world)
    }

    pub fn advance_command(
        &mut self,
        command: Command,
        world: &mut impl World,
    ) -> Result<(), InitFault> {
        self.advance(command.frame, Some(command), world)
    }

    /// Interrupt through authored ability exit paths without treating a living
    /// controlled body as dead. Timers and passive script lifecycles keep running.
    pub fn set_control_blocked(
        &mut self,
        blocked: bool,
        world: &mut impl World,
    ) -> Result<(), InitFault> {
        if self.machine.control_blocked == blocked {
            return Ok(());
        }
        if !blocked {
            self.machine.control_blocked = false;
            return Ok(());
        }
        self.run(world, |vm| {
            vm.queue_buttons(Command {
                frame: vm.machine.frame,
                ..Default::default()
            })?;
            vm.run()?;
            vm.machine.control_blocked = true;
            vm.machine.held_buttons.clear();
            vm.machine.ability_presses.clear();
            let abilities: Vec<_> = vm
                .machine
                .snapshot
                .instances
                .iter()
                .flat_map(|(&instance, data)| {
                    data.states.iter().filter_map(move |(&raw, state)| {
                        (state.active && matches!(state.kind, StateKind::Ability { .. }))
                            .then_some(StateKey { instance, raw })
                    })
                })
                .collect();
            for key in abilities {
                let node = vm
                    .graph(key.instance)
                    .state(usize::from(key.raw))
                    .expect("active ability");
                vm.stop_ability(key, node, 0)?;
            }
            for data in vm.machine.snapshot.instances.values_mut() {
                for state in data.states.values_mut() {
                    state.buffered_press = None;
                }
            }
            vm.run()?;
            vm.machine.shots.clear();
            Ok(())
        })
    }

    pub fn set_owner_dead(&mut self, dead: bool) {
        self.machine.owner_dead = dead;
    }

    pub fn command_buttons(&self) -> u32 {
        self.machine.command_buttons
    }

    fn advance(
        &mut self,
        frame: u32,
        command: Option<Command>,
        world: &mut impl World,
    ) -> Result<(), InitFault> {
        if frame_start(frame) < self.machine.now_ms {
            return Err(InitFault::FrameOrder {
                frame,
                now_ms: self.machine.now_ms,
            });
        }
        self.run(world, |vm| vm.advance_frame(frame, command))
    }

    #[doc(hidden)]
    pub fn debug_exit_instance(
        &mut self,
        instance: u16,
        world: &mut impl World,
    ) -> Result<(), InitFault> {
        self.run(world, |vm| vm.exit_instance(instance))
    }

    #[doc(hidden)]
    pub fn debug_begin_isolated_wave_health_pool(
        &mut self,
        instance: u16,
        raw: u16,
        world: &mut impl World,
    ) -> Result<(), InitFault> {
        self.run(world, |vm| {
            let node = vm.graph(instance).state(usize::from(raw)).unwrap();
            assert!(shapes::is_wave_health_pool(node));
            vm.machine.snapshot.instances[&instance].states.insert(
                raw,
                State {
                    active: true,
                    ..State::dormant(StateKind::HealthPool { pool: None })
                },
            );
            vm.begin_health_pool(StateKey { instance, raw }, node, true)
        })
    }

    #[doc(hidden)]
    pub fn debug_end_state(
        &mut self,
        instance: u16,
        raw: u16,
        world: &mut impl World,
    ) -> Result<(), InitFault> {
        self.run(world, |vm| vm.end(StateKey { instance, raw }, false, 0))
    }

    fn run<W: World>(
        &mut self,
        world: &mut W,
        operation: impl FnOnce(&mut Vm<'_, '_, '_, W>) -> Result<(), InitFault>,
    ) -> Result<(), InitFault> {
        if self.halted {
            return Err(InitFault::Halted);
        }
        self.run_with_budget(world, operation)
    }

    fn run_with_budget<W: World>(
        &mut self,
        world: &mut W,
        operation: impl FnOnce(&mut Vm<'_, '_, '_, W>) -> Result<(), InitFault>,
    ) -> Result<(), InitFault> {
        self.machine.work_left = self.machine.max_work;
        let result = operation(&mut Vm::new(&self.graphs, world, &mut self.machine));
        self.halted |= result.is_err();
        if !self.halted {
            self.machine.capture_outputs();
        }
        result
    }
}

pub fn initialize(
    graphs: &GraphStore,
    entity: u32,
    entity_variables: Variables,
    roots: Vec<Root>,
    world: &mut impl World,
    max_work: usize,
) -> Result<Snapshot, InitFault> {
    let mut machine = Machine::new(entity, entity_variables, 0, max_work)?;
    Vm::new(graphs, world, &mut machine).start(roots)?;
    machine.capture_outputs();
    Ok(machine.snapshot)
}
