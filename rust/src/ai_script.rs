//! Bounded, sandboxed Lua 5.4 host for room-local AI `think()` policies.

use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use mlua::{ChunkMode, Function, HookTriggers, Lua, LuaOptions, StdLib, Table, Value};
use thiserror::Error;

use crate::{
    game_clock::TICK_PERIOD,
    game_combat::{PlayerBodyState, RoomSimulation},
    game_replication::BodyId,
    patrol_script::{AiActorSnapshot, AiPatrolSnapshot},
};

/// Version of the bounded Lua AI `think(snapshot, events) -> commands` ABI.
pub(crate) const AI_ABI_VERSION: u32 = 2;

/// Bounds per-room Lua VM heap to 256 KiB so faulty scripts cannot exhaust worker memory.
pub(crate) const AI_MEMORY_LIMIT_BYTES: usize = 256 * 1024;

/// Samples the Lua instruction hook every 64 VM instructions to bound overshoot with low overhead.
pub(crate) const AI_INSTRUCTION_HOOK_INTERVAL: u32 = 64;

/// Caps chunk initialization at 32 * 64 = 2,048 instructions so loading/reload cannot stall.
pub(crate) const AI_INIT_HOOK_BUDGET: u32 = 32;

/// Caps each `think()` call at 64 * 64 = 4,096 instructions to fit inside the 16 ms tick budget.
pub(crate) const AI_THINK_HOOK_BUDGET: u32 = 64;

/// Caps candidate targets marshaled into `snapshot.targets` per tick to bound table overhead.
pub(crate) const AI_MAX_SNAPSHOT_TARGETS: usize = 32;

/// Caps event entries passed per `think()` call to bound host-to-Lua table marshaling.
pub(crate) const AI_MAX_EVENTS_PER_TICK: usize = 16;

/// Caps commands returned by `think()` per tick to prevent command flooding and unbounded loops.
pub(crate) const AI_MAX_COMMANDS_PER_TICK: usize = 8;

/// Preserves the existing 60-second upper bound on bot wait timers.
pub(crate) const AI_MAX_WAIT_SECONDS: f64 = 60.0;

/// Caps AI script source text at 64 KiB so parser and bytecode fit inside the room Lua heap.
pub(crate) const AI_MAX_SOURCE_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TargetRole {
    Enemy,
    Ally,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
pub(crate) struct AiTargetSnapshot {
    pub(crate) body: Option<BodyId>,
    pub(crate) entity: u32,
    pub(crate) team: u8,
    pub(crate) alive: bool,
    pub(crate) health: f64,
    pub(crate) max_health: f64,
    pub(crate) distance: f64,
    pub(crate) favored: bool,
    pub(crate) visible: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AiSnapshot {
    pub(crate) tick: u32,
    pub(crate) dt_secs: f64,
    pub(crate) actor: AiActorSnapshot,
    pub(crate) targets: Vec<AiTargetSnapshot>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) enum AiEvent {
    Spawn { tick: u32 },
    Release { tick: u32 },
    Think { tick: u32 },
    Damaged { tick: u32, source: u32 },
    TargetRetired { tick: u32, target: BodyId },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum AiCommand {
    Wait { seconds: f64 },
    MovePatrolSlot { slot: usize },
    SelectTarget { target: BodyId, role: TargetRole },
    ClearTarget { role: TargetRole },
    Hold,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AiExecutionPhase {
    Init,
    Think,
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub(crate) enum AiScriptError {
    #[error("AI script syntax error: {0}")]
    Syntax(String),
    #[error("AI script source exceeded size limit ({bytes} > {max} bytes)")]
    SourceTooLarge { bytes: usize, max: usize },
    #[error("AI script exceeded memory limit ({AI_MEMORY_LIMIT_BYTES} bytes)")]
    MemoryLimitExceeded,
    #[error("AI script exceeded {phase:?} instruction budget")]
    InstructionBudgetExceeded { phase: AiExecutionPhase },
    #[error("AI script ABI version {found} does not match expected {expected}")]
    UnsupportedAbi { found: i64, expected: u32 },
    #[error("invalid AI script ABI: {0}")]
    InvalidAbi(String),
    #[error("AI snapshot exceeded cap: {0}")]
    SnapshotBudgetExceeded(&'static str),
    #[error("AI event batch exceeded cap ({count} > {max})")]
    EventBudgetExceeded { count: usize, max: usize },
    #[error("AI script returned too many commands ({count} > {max})")]
    CommandBudgetExceeded { count: usize, max: usize },
    #[error("invalid AI snapshot: {0}")]
    InvalidSnapshot(&'static str),
    #[error("invalid AI command: {0}")]
    InvalidCommand(String),
    #[error("stale or retired player body handle {entity:08x}@gen{generation}", entity = .0.entity, generation = .0.generation)]
    StaleBodyHandle(BodyId),
    #[error("unknown or non-player target handle {entity:08x}@gen{generation}", entity = .0.entity, generation = .0.generation)]
    UnknownTargetHandle(BodyId),
    #[error("AI script runtime error: {0}")]
    Runtime(String),
}

impl From<AiScriptError> for io::Error {
    fn from(error: AiScriptError) -> Self {
        Self::new(io::ErrorKind::InvalidData, error)
    }
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) enum HandleQueryResult<'a> {
    LivePlayer(&'a PlayerBodyState),
    StalePlayer(BodyId),
    UnknownHandle(BodyId),
}

/// Validates a player `BodyId` handle read-only against `RoomSimulation`'s ECS world.
#[allow(dead_code)]
pub(crate) fn query_body_handle<'a>(sim: &'a RoomSimulation, id: BodyId) -> HandleQueryResult<'a> {
    if !sim.player_body_generation_current(id) {
        return HandleQueryResult::StalePlayer(id);
    }
    if id.generation != 0 {
        if let Some(state) = sim.player_body(id) {
            return HandleQueryResult::LivePlayer(state);
        }
    }
    HandleQueryResult::UnknownHandle(id)
}

#[allow(dead_code)]
pub(crate) fn validate_player_body_handle(
    sim: &RoomSimulation,
    id: BodyId,
) -> Result<&PlayerBodyState, AiScriptError> {
    match query_body_handle(sim, id) {
        HandleQueryResult::LivePlayer(state) => Ok(state),
        HandleQueryResult::StalePlayer(stale) => Err(AiScriptError::StaleBodyHandle(stale)),
        HandleQueryResult::UnknownHandle(unknown) => {
            Err(AiScriptError::UnknownTargetHandle(unknown))
        }
    }
}

#[allow(dead_code)]
pub(crate) fn validate_commands_against_simulation(
    commands: &[AiCommand],
    simulation: &RoomSimulation,
) -> Result<(), AiScriptError> {
    for command in commands {
        if let AiCommand::SelectTarget { target, role: _ } = command {
            validate_player_body_handle(simulation, *target)?;
        }
    }
    Ok(())
}

pub(crate) fn validate_ai_source_bounds(source: &str) -> Result<(), AiScriptError> {
    if source.len() > AI_MAX_SOURCE_BYTES {
        return Err(AiScriptError::SourceTooLarge {
            bytes: source.len(),
            max: AI_MAX_SOURCE_BYTES,
        });
    }
    if source.trim().is_empty() {
        return Err(AiScriptError::InvalidAbi(String::from(
            "AI script source must not be empty",
        )));
    }
    Ok(())
}

pub(crate) struct AiScript {
    think: Function,
    lua: Lua,
    remaining_hooks: Arc<AtomicU32>,
    budget_exhausted: Arc<AtomicBool>,
}

impl AiScript {
    pub(crate) fn from_source(chunk_name: &str, source: &str) -> Result<Self, AiScriptError> {
        validate_ai_source_bounds(source)?;
        let lua = Lua::new_with(StdLib::NONE, LuaOptions::default()).map_err(map_mlua_error)?;
        lua.set_memory_limit(AI_MEMORY_LIMIT_BYTES)
            .map_err(map_mlua_error)?;
        let remaining_hooks = Arc::new(AtomicU32::new(AI_INIT_HOOK_BUDGET));
        let budget_exhausted = Arc::new(AtomicBool::new(false));
        let hooks = Arc::clone(&remaining_hooks);
        let exhausted = Arc::clone(&budget_exhausted);
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(AI_INSTRUCTION_HOOK_INTERVAL),
            move |_, _| {
                let left = hooks.load(Ordering::Relaxed).saturating_sub(1);
                hooks.store(left, Ordering::Relaxed);
                if left == 0 {
                    exhausted.store(true, Ordering::Relaxed);
                    return Err(mlua::Error::runtime("AI instruction budget exhausted"));
                }
                Ok(mlua::VmState::Continue)
            },
        )
        .map_err(map_mlua_error)?;

        let env = lua.create_table().map_err(map_mlua_error)?;
        let module = match lua
            .load(source)
            .set_name(chunk_name)
            .set_mode(ChunkMode::Text)
            .set_environment(env)
            .call::<Value>(())
        {
            Ok(value) => value,
            Err(error) => {
                if budget_exhausted.load(Ordering::Relaxed) {
                    return Err(AiScriptError::InstructionBudgetExceeded {
                        phase: AiExecutionPhase::Init,
                    });
                }
                return Err(map_mlua_error(error));
            }
        };

        let Value::Table(table) = module else {
            return Err(AiScriptError::InvalidAbi(String::from(
                "AI script chunk must return a table { abi = 2, think = function }",
            )));
        };

        let abi = table.raw_get::<Value>("abi").map_err(map_mlua_error)?;
        let Value::Integer(version) = abi else {
            return Err(AiScriptError::InvalidAbi(String::from(
                "AI script table must contain integer field `abi`",
            )));
        };
        if version != i64::from(AI_ABI_VERSION) {
            return Err(AiScriptError::UnsupportedAbi {
                found: version,
                expected: AI_ABI_VERSION,
            });
        }

        let think_value = table.raw_get::<Value>("think").map_err(map_mlua_error)?;
        let Value::Function(think) = think_value else {
            return Err(AiScriptError::InvalidAbi(String::from(
                "AI script table must contain function field `think`",
            )));
        };

        Ok(Self {
            think,
            lua,
            remaining_hooks,
            budget_exhausted,
        })
    }

    pub(crate) fn think(
        &self,
        snapshot: &AiSnapshot,
        events: &[AiEvent],
    ) -> Result<Vec<AiCommand>, AiScriptError> {
        validate_snapshot_inputs(snapshot, events)?;

        self.remaining_hooks
            .store(AI_THINK_HOOK_BUDGET, Ordering::Relaxed);
        self.budget_exhausted.store(false, Ordering::Relaxed);

        let snapshot_table = marshal_snapshot(&self.lua, snapshot).map_err(map_mlua_error)?;
        let events_table = marshal_events(&self.lua, events).map_err(map_mlua_error)?;

        let result = match self.think.call::<Value>((snapshot_table, events_table)) {
            Ok(value) => value,
            Err(error) => {
                if self.budget_exhausted.load(Ordering::Relaxed) {
                    return Err(AiScriptError::InstructionBudgetExceeded {
                        phase: AiExecutionPhase::Think,
                    });
                }
                return Err(map_mlua_error(error));
            }
        };

        parse_commands(result, snapshot.actor.patrol)
    }

    #[cfg(test)]
    pub(crate) fn used_memory(&self) -> usize {
        self.lua.used_memory()
    }

    #[cfg(test)]
    pub(crate) fn set_memory_limit_for_test(&self, limit: usize) -> Result<usize, AiScriptError> {
        self.lua.set_memory_limit(limit).map_err(map_mlua_error)
    }
}

fn validate_snapshot_inputs(
    snapshot: &AiSnapshot,
    events: &[AiEvent],
) -> Result<(), AiScriptError> {
    if snapshot.targets.len() > AI_MAX_SNAPSHOT_TARGETS {
        return Err(AiScriptError::SnapshotBudgetExceeded("targets"));
    }
    if events.len() > AI_MAX_EVENTS_PER_TICK {
        return Err(AiScriptError::EventBudgetExceeded {
            count: events.len(),
            max: AI_MAX_EVENTS_PER_TICK,
        });
    }
    if !snapshot.dt_secs.is_finite() {
        return Err(AiScriptError::InvalidSnapshot("dt_secs must be finite"));
    }
    let actor = &snapshot.actor;
    if actor.position.iter().any(|axis| !axis.is_finite()) {
        return Err(AiScriptError::InvalidSnapshot(
            "actor position must be finite",
        ));
    }
    if !actor.yaw.is_finite() || !actor.health.is_finite() || !actor.max_health.is_finite() {
        return Err(AiScriptError::InvalidSnapshot(
            "actor yaw and health must be finite",
        ));
    }
    if let Some(generation) = actor.generation {
        if i64::try_from(generation).is_err() {
            return Err(AiScriptError::InvalidSnapshot(
                "actor generation exceeds Lua integer range",
            ));
        }
    }
    if let Some(patrol) = actor.patrol {
        if !patrol.wait_seconds.is_finite() {
            return Err(AiScriptError::InvalidSnapshot(
                "patrol wait_seconds must be finite",
            ));
        }
        if i64::try_from(patrol.route_index).is_err() || i64::try_from(patrol.route_count).is_err()
        {
            return Err(AiScriptError::InvalidSnapshot(
                "patrol route indices exceed Lua integer range",
            ));
        }
    }
    for target in &snapshot.targets {
        if !target.health.is_finite()
            || !target.max_health.is_finite()
            || !target.distance.is_finite()
        {
            return Err(AiScriptError::InvalidSnapshot(
                "target health and distance must be finite",
            ));
        }
        if let Some(body) = target.body {
            if i64::try_from(body.generation).is_err() {
                return Err(AiScriptError::InvalidSnapshot(
                    "target generation exceeds Lua integer range",
                ));
            }
        }
    }
    for event in events {
        if let AiEvent::TargetRetired { target, tick: _ } = event {
            if i64::try_from(target.generation).is_err() {
                return Err(AiScriptError::InvalidSnapshot(
                    "retired target generation exceeds Lua integer range",
                ));
            }
        }
    }
    Ok(())
}

fn marshal_snapshot(lua: &Lua, snapshot: &AiSnapshot) -> mlua::Result<Table> {
    let root = lua.create_table()?;
    root.raw_set("abi_version", i64::from(AI_ABI_VERSION))?;
    root.raw_set("tick", i64::from(snapshot.tick))?;
    root.raw_set("dt_secs", snapshot.dt_secs)?;

    let actor_tbl = lua.create_table()?;
    let actor = &snapshot.actor;
    actor_tbl.raw_set("entity", i64::from(actor.entity))?;
    let actor_gen = actor
        .generation
        .map(|generation| i64::try_from(generation).unwrap_or(0))
        .unwrap_or(0);
    actor_tbl.raw_set("generation", actor_gen)?;
    actor_tbl.raw_set("team", i64::from(actor.team))?;

    let pos_tbl = lua.create_table()?;
    pos_tbl.raw_set(1, actor.position[0])?;
    pos_tbl.raw_set(2, actor.position[1])?;
    pos_tbl.raw_set(3, actor.position[2])?;
    actor_tbl.raw_set("position", pos_tbl)?;

    actor_tbl.raw_set("yaw", actor.yaw)?;
    actor_tbl.raw_set("alive", actor.alive)?;
    actor_tbl.raw_set("health", actor.health)?;
    actor_tbl.raw_set("max_health", actor.max_health)?;
    actor_tbl.raw_set("stunned", actor.stunned)?;

    if let Some(patrol) = actor.patrol {
        let patrol_tbl = lua.create_table()?;
        patrol_tbl.raw_set(
            "route_index",
            i64::try_from(patrol.route_index).unwrap_or(0),
        )?;
        patrol_tbl.raw_set(
            "route_count",
            i64::try_from(patrol.route_count).unwrap_or(0),
        )?;
        patrol_tbl.raw_set("wait_seconds", patrol.wait_seconds)?;
        patrol_tbl.raw_set("walking", patrol.walking)?;
        actor_tbl.raw_set("patrol", patrol_tbl)?;
    } else {
        actor_tbl.raw_set("patrol", Value::Nil)?;
    }
    root.raw_set("actor", actor_tbl)?;

    let targets_tbl = lua.create_table()?;
    for (idx, target) in snapshot.targets.iter().enumerate() {
        let entry = lua.create_table()?;
        let (is_player, generation) = match target.body {
            Some(body) => (true, i64::try_from(body.generation).unwrap_or(0)),
            None => (false, 0),
        };
        entry.raw_set("entity", i64::from(target.entity))?;
        entry.raw_set("generation", generation)?;
        entry.raw_set("is_player", is_player)?;
        entry.raw_set("team", i64::from(target.team))?;
        entry.raw_set("alive", target.alive)?;
        entry.raw_set("health", target.health)?;
        entry.raw_set("max_health", target.max_health)?;
        entry.raw_set("distance", target.distance)?;
        entry.raw_set("favored", target.favored)?;
        entry.raw_set("visible", target.visible)?;
        targets_tbl.raw_set(idx + 1, entry)?;
    }
    root.raw_set("targets", targets_tbl)?;
    Ok(root)
}

fn marshal_events(lua: &Lua, events: &[AiEvent]) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for (idx, event) in events.iter().enumerate() {
        let entry = lua.create_table()?;
        match *event {
            AiEvent::Spawn { tick } => {
                entry.raw_set("kind", "spawn")?;
                entry.raw_set("tick", i64::from(tick))?;
            }
            AiEvent::Release { tick } => {
                entry.raw_set("kind", "release")?;
                entry.raw_set("tick", i64::from(tick))?;
            }
            AiEvent::Think { tick } => {
                entry.raw_set("kind", "think")?;
                entry.raw_set("tick", i64::from(tick))?;
            }
            AiEvent::Damaged { tick, source } => {
                entry.raw_set("kind", "damaged")?;
                entry.raw_set("tick", i64::from(tick))?;
                entry.raw_set("source", i64::from(source))?;
            }
            AiEvent::TargetRetired { tick, target } => {
                entry.raw_set("kind", "target_retired")?;
                entry.raw_set("tick", i64::from(tick))?;
                entry.raw_set("entity", i64::from(target.entity))?;
                entry.raw_set("generation", i64::try_from(target.generation).unwrap_or(0))?;
            }
        }
        table.raw_set(idx + 1, entry)?;
    }
    Ok(table)
}

fn parse_commands(
    value: Value,
    patrol_context: Option<AiPatrolSnapshot>,
) -> Result<Vec<AiCommand>, AiScriptError> {
    let Value::Table(table) = value else {
        return Err(AiScriptError::InvalidCommand(String::from(
            "think() must return a command array table",
        )));
    };
    let raw_len = table.raw_len();
    if raw_len > AI_MAX_COMMANDS_PER_TICK {
        return Err(AiScriptError::CommandBudgetExceeded {
            count: raw_len,
            max: AI_MAX_COMMANDS_PER_TICK,
        });
    }

    let mut seen = 0_usize;
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair.map_err(map_mlua_error)?;
        seen += 1;
        if seen > AI_MAX_COMMANDS_PER_TICK {
            return Err(AiScriptError::CommandBudgetExceeded {
                count: seen,
                max: AI_MAX_COMMANDS_PER_TICK,
            });
        }
        let Value::Integer(index) = key else {
            return Err(AiScriptError::InvalidCommand(String::from(
                "command array must contain integer indices 1..=N only",
            )));
        };
        let valid_index = usize::try_from(index)
            .ok()
            .is_some_and(|idx| idx >= 1 && idx <= raw_len);
        if !valid_index {
            return Err(AiScriptError::InvalidCommand(format!(
                "command array contains out-of-range or sparse key {index}"
            )));
        }
    }
    if seen != raw_len {
        return Err(AiScriptError::InvalidCommand(String::from(
            "command array must be a dense 1-based sequence without holes",
        )));
    }

    let mut commands = Vec::with_capacity(raw_len);
    for idx in 1..=raw_len {
        let entry = table.raw_get::<Value>(idx).map_err(map_mlua_error)?;
        let Value::Table(cmd_table) = entry else {
            return Err(AiScriptError::InvalidCommand(format!(
                "command at index {idx} must be a table"
            )));
        };
        commands.push(parse_single_command(&cmd_table, patrol_context)?);
    }
    Ok(commands)
}

fn parse_single_command(
    table: &Table,
    patrol_context: Option<AiPatrolSnapshot>,
) -> Result<AiCommand, AiScriptError> {
    let kind_val = table.raw_get::<Value>("kind").map_err(map_mlua_error)?;
    let Value::String(kind_str) = kind_val else {
        return Err(AiScriptError::InvalidCommand(String::from(
            "command must contain string field `kind`",
        )));
    };
    let kind = kind_str
        .to_str()
        .map_err(|error| AiScriptError::InvalidCommand(format!("invalid command kind: {error}")))?;

    match &*kind {
        "wait" => {
            let seconds_val = table.raw_get::<Value>("seconds").map_err(map_mlua_error)?;
            let seconds = match seconds_val {
                Value::Number(value) => value,
                Value::Integer(value) => value as f64,
                _ => {
                    return Err(AiScriptError::InvalidCommand(String::from(
                        "wait command `seconds` must be a number",
                    )));
                }
            };
            let min_wait = TICK_PERIOD.as_secs_f64();
            if !seconds.is_finite() || !(min_wait..=AI_MAX_WAIT_SECONDS).contains(&seconds) {
                return Err(AiScriptError::InvalidCommand(format!(
                    "wait `seconds` ({seconds}) must be finite and within [{min_wait}, {AI_MAX_WAIT_SECONDS}]"
                )));
            }
            Ok(AiCommand::Wait { seconds })
        }
        "patrol_slot" => {
            let slot_val = table.raw_get::<Value>("slot").map_err(map_mlua_error)?;
            let Value::Integer(raw_slot) = slot_val else {
                return Err(AiScriptError::InvalidCommand(String::from(
                    "patrol_slot command `slot` must be an integer",
                )));
            };
            let slot = usize::try_from(raw_slot).map_err(|_| {
                AiScriptError::InvalidCommand(format!(
                    "patrol_slot `slot` ({raw_slot}) must be non-negative"
                ))
            })?;
            let Some(patrol) = patrol_context else {
                return Err(AiScriptError::InvalidCommand(String::from(
                    "patrol_slot command requires patrol snapshot context",
                )));
            };
            if slot >= patrol.route_count {
                return Err(AiScriptError::InvalidCommand(format!(
                    "patrol_slot `slot` ({slot}) is outside route_count ({})",
                    patrol.route_count
                )));
            }
            Ok(AiCommand::MovePatrolSlot { slot })
        }
        "select_target" => {
            let role = parse_target_role(table)?;
            let entity = parse_u32_field(table, "entity")?;
            let generation = parse_u64_field(table, "generation")?;
            Ok(AiCommand::SelectTarget {
                target: BodyId { entity, generation },
                role,
            })
        }
        "clear_target" => {
            let role = parse_target_role(table)?;
            Ok(AiCommand::ClearTarget { role })
        }
        "hold" => Ok(AiCommand::Hold),
        other => Err(AiScriptError::InvalidCommand(format!(
            "unknown AI command kind `{other}`"
        ))),
    }
}

fn parse_target_role(table: &Table) -> Result<TargetRole, AiScriptError> {
    let role_val = table.raw_get::<Value>("role").map_err(map_mlua_error)?;
    let Value::String(role_str) = role_val else {
        return Err(AiScriptError::InvalidCommand(String::from(
            "target command `role` must be a string",
        )));
    };
    let role = role_str
        .to_str()
        .map_err(|error| AiScriptError::InvalidCommand(format!("invalid role string: {error}")))?;
    match &*role {
        "enemy" => Ok(TargetRole::Enemy),
        "ally" => Ok(TargetRole::Ally),
        other => Err(AiScriptError::InvalidCommand(format!(
            "unknown target role `{other}`"
        ))),
    }
}

fn parse_u32_field(table: &Table, key: &'static str) -> Result<u32, AiScriptError> {
    let value = table.raw_get::<Value>(key).map_err(map_mlua_error)?;
    let Value::Integer(raw) = value else {
        return Err(AiScriptError::InvalidCommand(format!(
            "command field `{key}` must be an integer"
        )));
    };
    u32::try_from(raw).map_err(|_| {
        AiScriptError::InvalidCommand(format!("command field `{key}` ({raw}) is out of u32 range"))
    })
}

fn parse_u64_field(table: &Table, key: &'static str) -> Result<u64, AiScriptError> {
    let value = table.raw_get::<Value>(key).map_err(map_mlua_error)?;
    let Value::Integer(raw) = value else {
        return Err(AiScriptError::InvalidCommand(format!(
            "command field `{key}` must be an integer"
        )));
    };
    u64::try_from(raw).map_err(|_| {
        AiScriptError::InvalidCommand(format!(
            "command field `{key}` ({raw}) must be non-negative"
        ))
    })
}

fn is_memory_error(error: &mlua::Error) -> bool {
    match error {
        mlua::Error::MemoryError(_) => true,
        mlua::Error::CallbackError {
            cause,
            traceback: _,
        }
        | mlua::Error::WithContext { cause, context: _ }
        | mlua::Error::BadArgument {
            cause,
            to: _,
            pos: _,
            name: _,
        } => is_memory_error(cause),
        _ => false,
    }
}

fn map_mlua_error(error: mlua::Error) -> AiScriptError {
    if is_memory_error(&error) {
        return AiScriptError::MemoryLimitExceeded;
    }
    match error {
        mlua::Error::SyntaxError {
            message,
            incomplete_input: _,
        } => AiScriptError::Syntax(message),
        other => AiScriptError::Runtime(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        contact_world::ContactWorld, game_heroes::SOLDIER, mover::Mover,
        mover_settings::MoverSettings,
    };

    fn patrol_snapshot(tick: u32, route_index: usize, route_count: usize) -> AiSnapshot {
        AiSnapshot {
            tick,
            dt_secs: TICK_PERIOD.as_secs_f64(),
            actor: AiActorSnapshot::patrol_fixture(route_index, route_count),
            targets: Vec::new(),
        }
    }

    #[test]
    fn ai_memory_budget_exceeded_returns_deterministic_error_without_panic() {
        let init_oom = AiScript::from_source(
            "=init_oom.lua",
            r#"
            local s = "0123456789abcdef"
            for i = 1, 16 do
                s = s .. s
            end
            return { abi = 2, think = function() return {} end }
            "#,
        );
        assert!(matches!(init_oom, Err(AiScriptError::MemoryLimitExceeded)));

        let think_oom = AiScript::from_source(
            "=think_oom.lua",
            r#"
            return {
                abi = 2,
                think = function()
                    local s = "0123456789abcdef"
                    for i = 1, 16 do
                        s = s .. s
                    end
                    return {}
                end,
            }
            "#,
        )
        .unwrap();
        let err = think_oom
            .think(&patrol_snapshot(1, 0, 2), &[AiEvent::Think { tick: 1 }])
            .unwrap_err();
        assert_eq!(err, AiScriptError::MemoryLimitExceeded);

        // A separate healthy VM remains unaffected after OOM in another VM.
        let healthy = AiScript::from_source(
            "=healthy.lua",
            r#"
            return {
                abi = 2,
                think = function()
                    return { { kind = "hold" } }
                end,
            }
            "#,
        )
        .unwrap();
        assert_eq!(
            healthy
                .think(&patrol_snapshot(1, 0, 2), &[AiEvent::Think { tick: 1 }])
                .unwrap(),
            vec![AiCommand::Hold]
        );
    }

    #[test]
    fn ai_init_instruction_budget_exceeded_returns_deterministic_error() {
        let err = match AiScript::from_source("=init_loop.lua", "while true do end") {
            Ok(_) => panic!("expected init loop to exhaust instruction budget"),
            Err(error) => error,
        };
        assert_eq!(
            err,
            AiScriptError::InstructionBudgetExceeded {
                phase: AiExecutionPhase::Init,
            }
        );
    }

    #[test]
    fn ai_think_instruction_budget_exceeded_returns_deterministic_error() {
        let script = AiScript::from_source(
            "=think_loop.lua",
            r#"
            return {
                abi = 2,
                think = function(snapshot, events)
                    if snapshot.tick == 1 then
                        while true do end
                    end
                    return { { kind = "hold" } }
                end,
            }
            "#,
        )
        .unwrap();
        let err = script
            .think(&patrol_snapshot(1, 0, 2), &[AiEvent::Think { tick: 1 }])
            .unwrap_err();
        assert_eq!(
            err,
            AiScriptError::InstructionBudgetExceeded {
                phase: AiExecutionPhase::Think,
            }
        );

        // Hook budget resets cleanly before every subsequent think() call.
        for tick in 2..=501 {
            let commands = script
                .think(&patrol_snapshot(tick, 0, 2), &[AiEvent::Think { tick }])
                .unwrap();
            assert_eq!(commands, vec![AiCommand::Hold]);
        }
    }

    #[test]
    fn ai_snapshot_target_and_event_caps_return_deterministic_errors() {
        let script = AiScript::from_source(
            "=noop.lua",
            "return { abi = 2, think = function() return {} end }",
        )
        .unwrap();

        let mut too_many_targets = patrol_snapshot(1, 0, 2);
        too_many_targets.targets = (0..=AI_MAX_SNAPSHOT_TARGETS)
            .map(|idx| AiTargetSnapshot {
                body: None,
                entity: 0xc000_0000 + u32::try_from(idx).unwrap(),
                team: 0,
                alive: true,
                health: 200.0,
                max_health: 200.0,
                distance: 1.0,
                favored: false,
                visible: true,
            })
            .collect();
        assert_eq!(
            script
                .think(&too_many_targets, &[AiEvent::Think { tick: 1 }])
                .unwrap_err(),
            AiScriptError::SnapshotBudgetExceeded("targets")
        );

        let too_many_events = vec![AiEvent::Think { tick: 1 }; AI_MAX_EVENTS_PER_TICK + 1];
        assert_eq!(
            script
                .think(&patrol_snapshot(1, 0, 2), &too_many_events)
                .unwrap_err(),
            AiScriptError::EventBudgetExceeded {
                count: AI_MAX_EVENTS_PER_TICK + 1,
                max: AI_MAX_EVENTS_PER_TICK,
            }
        );

        let mut nan_snapshot = patrol_snapshot(1, 0, 2);
        nan_snapshot.dt_secs = f64::NAN;
        assert!(matches!(
            script.think(&nan_snapshot, &[]).unwrap_err(),
            AiScriptError::InvalidSnapshot(_)
        ));

        let mut huge_gen_snapshot = patrol_snapshot(1, 0, 2);
        huge_gen_snapshot.actor.generation = Some(u64::MAX);
        assert!(matches!(
            script.think(&huge_gen_snapshot, &[]).unwrap_err(),
            AiScriptError::InvalidSnapshot(_)
        ));
    }

    #[test]
    fn ai_command_budget_and_validation_return_deterministic_errors() {
        let too_many_cmds = AiScript::from_source(
            "=too_many.lua",
            r#"
            return {
                abi = 2,
                think = function()
                    local out = {}
                    for i = 1, 9 do
                        out[i] = { kind = "hold" }
                    end
                    return out
                end,
            }
            "#,
        )
        .unwrap();
        assert_eq!(
            too_many_cmds
                .think(&patrol_snapshot(1, 0, 2), &[])
                .unwrap_err(),
            AiScriptError::CommandBudgetExceeded {
                count: 9,
                max: AI_MAX_COMMANDS_PER_TICK,
            }
        );

        let large_hash = AiScript::from_source(
            "=hash_flood.lua",
            r#"
            return {
                abi = 2,
                think = function()
                    return { a=1, b=2, c=3, d=4, e=5, f=6, g=7, h=8, i=9 }
                end,
            }
            "#,
        )
        .unwrap();
        assert!(matches!(
            large_hash
                .think(&patrol_snapshot(1, 0, 2), &[])
                .unwrap_err(),
            AiScriptError::InvalidCommand(_)
        ));

        for invalid_return in [
            "return { abi = 2, think = function() return nil end }",
            "return { abi = 2, think = function() return 'hold' end }",
            "return { abi = 2, think = function() return { kind = 'hold' } end }",
            "return { abi = 2, think = function() return { [0] = { kind = 'hold' } } end }",
            "return { abi = 2, think = function() return { [1] = { kind = 'hold' }, [3] = { kind = 'hold' } } end }",
            "return { abi = 2, think = function() return { { kind = 'wait', seconds = 0/0 } } end }",
            "return { abi = 2, think = function() return { { kind = 'wait', seconds = 1/0 } } end }",
            "return { abi = 2, think = function() return { { kind = 'wait', seconds = -1/0 } } end }",
            "return { abi = 2, think = function() return { { kind = 'wait', seconds = 0 } } end }",
            "return { abi = 2, think = function() return { { kind = 'wait', seconds = 0.005 } } end }",
            "return { abi = 2, think = function() return { { kind = 'wait', seconds = 61 } } end }",
            "return { abi = 2, think = function() return { { kind = 'wait', seconds = '3' } } end }",
            "return { abi = 2, think = function() return { { kind = 'patrol_slot', slot = -1 } } end }",
            "return { abi = 2, think = function() return { { kind = 'patrol_slot', slot = 3 } } end }",
            "return { abi = 2, think = function() return { { kind = 'patrol_slot', slot = 1.5 } } end }",
            "return { abi = 2, think = function() return { { kind = 'patrol_slot', slot = '0' } } end }",
            "return { abi = 2, think = function() return { { kind = 'select_target', role = 'boss', entity = 1, generation = 1 } } end }",
            "return { abi = 2, think = function() return { { kind = 'unknown' } } end }",
        ] {
            let script = AiScript::from_source("=invalid_cmd.lua", invalid_return).unwrap();
            assert!(
                matches!(
                    script.think(&patrol_snapshot(1, 0, 3), &[]).unwrap_err(),
                    AiScriptError::InvalidCommand(_)
                ),
                "expected InvalidCommand for `{invalid_return}`"
            );
        }

        // ABI rejections including legacy v1 script.
        assert!(matches!(
            AiScript::from_source("=v1.lua", "return 1, function() return 3 end"),
            Err(AiScriptError::InvalidAbi(_))
        ));
        assert!(matches!(
            AiScript::from_source(
                "=v1_table.lua",
                "return { abi = 1, think = function() return {} end }"
            ),
            Err(AiScriptError::UnsupportedAbi {
                found: 1,
                expected: 2
            })
        ));
        assert!(matches!(
            AiScript::from_source("=syntax.lua", "return {"),
            Err(AiScriptError::Syntax(_))
        ));
    }

    #[test]
    fn ai_command_rejects_stale_zero_and_unknown_body_id_handles() {
        let mut simulation = RoomSimulation::new(Vec::new()).unwrap();
        let owner = 0xa000_0100;
        let wire = owner | 1;
        let contact_world = Arc::new(ContactWorld::new(Vec::new()).unwrap());
        let make_state = |id: BodyId, pos: [f64; 3]| {
            let mover = Mover::new(
                MoverSettings::default(),
                Arc::clone(&contact_world),
                pos,
                0,
                0x08,
            )
            .unwrap();
            PlayerBodyState::new(id, owner, 0, 1, SOLDIER, mover, None, 100, None)
        };

        let gen1 = simulation.next_player_generation(owner, 0).unwrap();
        let id1 = BodyId {
            entity: wire,
            generation: gen1,
        };
        simulation
            .spawn_player_body(make_state(id1, [0.0; 3]))
            .unwrap();

        let gen2 = simulation.next_player_generation(owner, 0).unwrap();
        let id2 = BodyId {
            entity: wire,
            generation: gen2,
        };
        simulation
            .spawn_player_body(make_state(id2, [5.0, 0.0, 0.0]))
            .unwrap();

        let script = AiScript::from_source(
            "=target_selector.lua",
            r#"
            return {
                abi = 2,
                think = function(snapshot)
                    local target = snapshot.targets[1]
                    return {
                        {
                            kind = "select_target",
                            role = "enemy",
                            entity = target.entity,
                            generation = target.generation,
                        }
                    }
                end,
            }
            "#,
        )
        .unwrap();

        let run_select = |body: BodyId| {
            let snapshot = AiSnapshot {
                tick: 101,
                dt_secs: TICK_PERIOD.as_secs_f64(),
                actor: AiActorSnapshot::patrol_fixture(0, 1),
                targets: vec![AiTargetSnapshot {
                    body: Some(body),
                    entity: body.entity,
                    team: 1,
                    alive: true,
                    health: 200.0,
                    max_health: 200.0,
                    distance: 5.0,
                    favored: false,
                    visible: true,
                }],
            };
            let commands = script
                .think(&snapshot, &[AiEvent::Think { tick: 101 }])
                .unwrap();
            validate_commands_against_simulation(&commands, &simulation)
        };

        // 1. Retired generation id1 must be rejected as stale, leaving live id2 untouched.
        assert_eq!(
            run_select(id1).unwrap_err(),
            AiScriptError::StaleBodyHandle(id1)
        );
        assert_eq!(
            simulation.player_body(id2).unwrap().mover.pose().position,
            [5.0, 0.0, 0.0]
        );

        // 2. Zero generation on known player wire must be rejected cleanly.
        let zero_player = BodyId {
            entity: wire,
            generation: 0,
        };
        assert_eq!(
            run_select(zero_player).unwrap_err(),
            AiScriptError::StaleBodyHandle(zero_player)
        );

        // 3. Never-spawned wire ID must be rejected as unknown (catching player_body_generation_current's true return on unrecorded wires).
        let never_spawned = BodyId {
            entity: 0xDEAD_BEEF,
            generation: 1,
        };
        assert_eq!(
            run_select(never_spawned).unwrap_err(),
            AiScriptError::UnknownTargetHandle(never_spawned)
        );
        let zero_unknown = BodyId {
            entity: 0xDEAD_BEEF,
            generation: 0,
        };
        assert_eq!(
            run_select(zero_unknown).unwrap_err(),
            AiScriptError::UnknownTargetHandle(zero_unknown)
        );

        // 4. Live replacement handle id2 succeeds and resolves to live PlayerBodyState.
        assert!(run_select(id2).is_ok());
        assert_eq!(
            validate_player_body_handle(&simulation, id2).unwrap().id,
            id2
        );
    }

    #[test]
    fn ai_events_and_clear_target_round_trip_through_think_abi() {
        let script = AiScript::from_source(
            "=events_roundtrip.lua",
            r#"
            return {
                abi = 2,
                think = function(snapshot, events)
                    if events[1].kind == "damaged"
                        and events[1].source == 123
                        and events[2].kind == "target_retired"
                        and events[2].entity == 456
                        and events[2].generation == 7
                        and snapshot.targets[1].is_player == false
                    then
                        return {
                            { kind = "clear_target", role = "enemy" },
                            { kind = "clear_target", role = "ally" },
                            { kind = "hold" },
                        }
                    end
                    return {}
                end,
            }
            "#,
        )
        .unwrap();
        let mut snapshot = patrol_snapshot(50, 0, 1);
        snapshot.targets.push(AiTargetSnapshot {
            body: None,
            entity: 0xc000_1001,
            team: 1,
            alive: true,
            health: 150.0,
            max_health: 200.0,
            distance: 8.5,
            favored: true,
            visible: true,
        });
        let commands = script
            .think(
                &snapshot,
                &[
                    AiEvent::Damaged {
                        tick: 50,
                        source: 123,
                    },
                    AiEvent::TargetRetired {
                        tick: 50,
                        target: BodyId {
                            entity: 456,
                            generation: 7,
                        },
                    },
                ],
            )
            .unwrap();
        assert_eq!(
            commands,
            vec![
                AiCommand::ClearTarget {
                    role: TargetRole::Enemy,
                },
                AiCommand::ClearTarget {
                    role: TargetRole::Ally,
                },
                AiCommand::Hold,
            ]
        );
    }
}
