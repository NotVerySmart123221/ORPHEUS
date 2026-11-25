//! Bounded, sandboxed Lua 5.4 host for ORPHEUS Workshop / Mod scripts.
//!
//! Mod scripts run in an isolated Lua 5.4 VM (`StdLib::NONE`, 256 KiB memory cap,
//! bounded instruction hooks, and zero filesystem/OS/network access).

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use mlua::{ChunkMode, Function, HookTriggers, Lua, LuaOptions, StdLib, Table, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Version of the bounded Lua Mod / Workshop ABI.
pub const MOD_ABI_VERSION: u32 = 1;

/// Bounds per-room Lua VM heap to 256 KiB so faulty mods cannot exhaust worker memory.
pub const MOD_MEMORY_LIMIT_BYTES: usize = 256 * 1024;

/// Samples the Lua instruction hook every 64 VM instructions to bound overshoot with low overhead.
pub const MOD_INSTRUCTION_HOOK_INTERVAL: u32 = 64;

/// Caps chunk initialization at 64 * 64 = 4,096 instructions.
pub const MOD_INIT_HOOK_BUDGET: u32 = 64;

/// Caps each mod event hook invocation at 64 * 64 = 4,096 instructions.
pub const MOD_HOOK_BUDGET: u32 = 64;

/// Caps mod script source text at 64 KiB.
pub const MOD_MAX_SOURCE_BYTES: usize = 64 * 1024;

/// Caps actions returned by a single mod hook call.
pub const MOD_MAX_ACTIONS_PER_EVENT: usize = 16;

/// Caps chat broadcast length emitted by a mod hook.
pub const MOD_MAX_CHAT_LEN: usize = 200;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModInfo {
    pub name: String,
    pub version: String,
    pub author: String,
}

impl Default for ModInfo {
    fn default() -> Self {
        Self {
            name: String::from("Unnamed Mod"),
            version: String::from("0.1.0"),
            author: String::from("Unknown"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModRules {
    pub damage_scale: f64,
    pub healing_scale: f64,
    pub gravity_scale: f64,
    pub speed_scale: f64,
    pub ult_charge_scale: f64,
    pub cooldown_scale: f64,
    pub friendly_fire: bool,
}

impl Default for ModRules {
    fn default() -> Self {
        Self {
            damage_scale: 1.0,
            healing_scale: 1.0,
            gravity_scale: 1.0,
            speed_scale: 1.0,
            ult_charge_scale: 1.0,
            cooldown_scale: 1.0,
            friendly_fire: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModRoomContext {
    pub tick: u32,
    pub elapsed_secs: f64,
    pub map_guid: u64,
    pub mode_guid: u64,
    pub player_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModPlayerSnapshot {
    pub entity: u32,
    pub name: String,
    pub team: u8,
    pub hero_guid: u64,
    pub health: f64,
    pub max_health: f64,
    pub position: [f64; 3],
    pub alive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModDamageEvent {
    pub attacker: u32,
    pub victim: u32,
    pub damage: f64,
    pub headshot: bool,
    pub melee: bool,
    pub ability_slot: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModDamageOutcome {
    pub damage: f64,
    pub actions: Vec<ModAction>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModKillEvent {
    pub killer: u32,
    pub victim: u32,
    pub headshot: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ModAction {
    SetHealth { entity: u32, health: f64 },
    Damage { entity: u32, amount: f64 },
    Heal { entity: u32, amount: f64 },
    ApplyImpulse { entity: u32, velocity: [f64; 3] },
    Teleport { entity: u32, position: [f64; 3] },
    SetSpeedScale { entity: u32, scale: f64 },
    SetGravityScale { entity: u32, scale: f64 },
    PlayVoiceCue { entity: u32, stimulus: u64 },
    BroadcastChat { text: String },
    ResetInteractiveProp { prop_id: u32 },
    SetRoomRule { key: String, value: f64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModStatus {
    pub name: String,
    pub version: String,
    pub author: String,
    pub abi_version: u32,
    pub generation: u64,
    pub digest: String,
    pub changed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModExecutionPhase {
    Init,
    Hook,
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum ModScriptError {
    #[error("mod script syntax error: {0}")]
    Syntax(String),
    #[error("mod script source exceeded size limit ({bytes} > {max} bytes)")]
    SourceTooLarge { bytes: usize, max: usize },
    #[error("mod script exceeded memory limit ({MOD_MEMORY_LIMIT_BYTES} bytes)")]
    MemoryLimitExceeded,
    #[error("mod script exceeded {phase:?} instruction budget")]
    InstructionBudgetExceeded { phase: ModExecutionPhase },
    #[error("mod script ABI version {found} does not match expected {expected}")]
    UnsupportedAbi { found: i64, expected: u32 },
    #[error("invalid mod script ABI: {0}")]
    InvalidAbi(String),
    #[error("mod script returned too many actions ({count} > {max})")]
    ActionBudgetExceeded { count: usize, max: usize },
    #[error("invalid mod action: {0}")]
    InvalidAction(String),
    #[error("mod script runtime error: {0}")]
    Runtime(String),
}

impl From<ModScriptError> for io::Error {
    fn from(error: ModScriptError) -> Self {
        Self::new(io::ErrorKind::InvalidData, error)
    }
}

pub fn validate_mod_source_bounds(source: &str) -> Result<(), ModScriptError> {
    if source.len() > MOD_MAX_SOURCE_BYTES {
        return Err(ModScriptError::SourceTooLarge {
            bytes: source.len(),
            max: MOD_MAX_SOURCE_BYTES,
        });
    }
    if source.trim().is_empty() {
        return Err(ModScriptError::InvalidAbi(String::from(
            "mod script source must not be empty",
        )));
    }
    Ok(())
}

pub struct ModScript {
    lua: Lua,
    info: ModInfo,
    rules: ModRules,
    on_room_start: Option<Function>,
    on_player_spawn: Option<Function>,
    on_damage: Option<Function>,
    on_kill: Option<Function>,
    on_button: Option<Function>,
    on_chat: Option<Function>,
    on_tick: Option<Function>,
    remaining_hooks: Arc<AtomicU32>,
    budget_exhausted: Arc<AtomicBool>,
}

impl ModScript {
    pub fn from_source(chunk_name: &str, source: &str) -> Result<Self, ModScriptError> {
        validate_mod_source_bounds(source)?;
        let lua = Lua::new_with(StdLib::NONE, LuaOptions::default()).map_err(map_mlua_error)?;
        lua.set_memory_limit(MOD_MEMORY_LIMIT_BYTES)
            .map_err(map_mlua_error)?;

        let remaining_hooks = Arc::new(AtomicU32::new(MOD_INIT_HOOK_BUDGET));
        let budget_exhausted = Arc::new(AtomicBool::new(false));
        let hooks = Arc::clone(&remaining_hooks);
        let exhausted = Arc::clone(&budget_exhausted);
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(MOD_INSTRUCTION_HOOK_INTERVAL),
            move |_, _| {
                let prev = hooks.load(Ordering::Relaxed);
                let left = prev.saturating_sub(1);
                hooks.store(left, Ordering::Relaxed);
                if left == 0 {
                    exhausted.store(true, Ordering::Relaxed);
                    return Err(mlua::Error::runtime("mod instruction budget exhausted"));
                }
                Ok(mlua::VmState::Continue)
            },
        )
        .map_err(map_mlua_error)?;

        let env = build_sandbox_env(&lua).map_err(map_mlua_error)?;
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
                    return Err(ModScriptError::InstructionBudgetExceeded {
                        phase: ModExecutionPhase::Init,
                    });
                }
                return Err(map_mlua_error(error));
            }
        };

        let Value::Table(table) = module else {
            return Err(ModScriptError::InvalidAbi(String::from(
                "mod script must return a table { abi = 1, ... }",
            )));
        };

        let abi = table.raw_get::<Value>("abi").map_err(map_mlua_error)?;
        let Value::Integer(version) = abi else {
            return Err(ModScriptError::InvalidAbi(String::from(
                "mod script table must contain integer field `abi`",
            )));
        };
        if version != i64::from(MOD_ABI_VERSION) {
            return Err(ModScriptError::UnsupportedAbi {
                found: version,
                expected: MOD_ABI_VERSION,
            });
        }

        let info = parse_info(&table)?;
        let rules = parse_rules(&table)?;

        Ok(Self {
            on_room_start: optional_fn(&table, "on_room_start")?,
            on_player_spawn: optional_fn(&table, "on_player_spawn")?,
            on_damage: optional_fn(&table, "on_damage")?,
            on_kill: optional_fn(&table, "on_kill")?,
            on_button: optional_fn(&table, "on_button")?,
            on_chat: optional_fn(&table, "on_chat")?,
            on_tick: optional_fn(&table, "on_tick")?,
            lua,
            info,
            rules,
            remaining_hooks,
            budget_exhausted,
        })
    }

    pub fn info(&self) -> &ModInfo {
        &self.info
    }

    pub fn rules(&self) -> ModRules {
        self.rules
    }

    pub fn on_room_start(&self, ctx: &ModRoomContext) -> Result<Vec<ModAction>, ModScriptError> {
        let Some(hook) = &self.on_room_start else {
            return Ok(Vec::new());
        };
        let ctx_tbl = marshal_context(&self.lua, ctx).map_err(map_mlua_error)?;
        let value = self.call_hook(hook, ctx_tbl)?;
        parse_actions_or_nil(value)
    }

    pub fn on_player_spawn(
        &self,
        ctx: &ModRoomContext,
        player: &ModPlayerSnapshot,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        let Some(hook) = &self.on_player_spawn else {
            return Ok(Vec::new());
        };
        let ctx_tbl = marshal_context(&self.lua, ctx).map_err(map_mlua_error)?;
        let player_tbl = marshal_player(&self.lua, player).map_err(map_mlua_error)?;
        let value = self.call_hook(hook, (ctx_tbl, player_tbl))?;
        parse_actions_or_nil(value)
    }

    pub fn on_damage(
        &self,
        ctx: &ModRoomContext,
        event: &ModDamageEvent,
    ) -> Result<ModDamageOutcome, ModScriptError> {
        let Some(hook) = &self.on_damage else {
            return Ok(ModDamageOutcome {
                damage: event.damage,
                actions: Vec::new(),
            });
        };
        let ctx_tbl = marshal_context(&self.lua, ctx).map_err(map_mlua_error)?;
        let ev_tbl = self.lua.create_table().map_err(map_mlua_error)?;
        ev_tbl
            .raw_set("attacker", i64::from(event.attacker))
            .map_err(map_mlua_error)?;
        ev_tbl
            .raw_set("victim", i64::from(event.victim))
            .map_err(map_mlua_error)?;
        ev_tbl
            .raw_set("damage", event.damage)
            .map_err(map_mlua_error)?;
        ev_tbl
            .raw_set("headshot", event.headshot)
            .map_err(map_mlua_error)?;
        ev_tbl
            .raw_set("melee", event.melee)
            .map_err(map_mlua_error)?;
        ev_tbl
            .raw_set("ability_slot", i64::from(event.ability_slot))
            .map_err(map_mlua_error)?;

        let value = self.call_hook(hook, (ctx_tbl, ev_tbl))?;
        match value {
            Value::Nil => Ok(ModDamageOutcome {
                damage: event.damage,
                actions: Vec::new(),
            }),
            Value::Number(damage) => {
                if !damage.is_finite() || damage < 0.0 {
                    return Err(ModScriptError::InvalidAction(format!(
                        "on_damage returned invalid damage {damage}"
                    )));
                }
                Ok(ModDamageOutcome {
                    damage,
                    actions: Vec::new(),
                })
            }
            Value::Integer(damage) => {
                if damage < 0 {
                    return Err(ModScriptError::InvalidAction(format!(
                        "on_damage returned negative damage {damage}"
                    )));
                }
                Ok(ModDamageOutcome {
                    damage: damage as f64,
                    actions: Vec::new(),
                })
            }
            Value::Table(tbl) => {
                let damage_val = tbl.raw_get::<Value>("damage").map_err(map_mlua_error)?;
                let damage = match damage_val {
                    Value::Nil => event.damage,
                    _ => parse_non_negative_f64_value(damage_val, "damage")?,
                };
                let actions_val = tbl.raw_get::<Value>("actions").map_err(map_mlua_error)?;
                let actions = parse_actions_or_nil(actions_val)?;
                Ok(ModDamageOutcome { damage, actions })
            }
            _ => Err(ModScriptError::InvalidAction(String::from(
                "on_damage must return nil, number, or { damage, actions }",
            ))),
        }
    }

    pub fn on_kill(
        &self,
        ctx: &ModRoomContext,
        event: &ModKillEvent,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        let Some(hook) = &self.on_kill else {
            return Ok(Vec::new());
        };
        let ctx_tbl = marshal_context(&self.lua, ctx).map_err(map_mlua_error)?;
        let ev_tbl = self.lua.create_table().map_err(map_mlua_error)?;
        ev_tbl
            .raw_set("killer", i64::from(event.killer))
            .map_err(map_mlua_error)?;
        ev_tbl
            .raw_set("victim", i64::from(event.victim))
            .map_err(map_mlua_error)?;
        ev_tbl
            .raw_set("headshot", event.headshot)
            .map_err(map_mlua_error)?;
        let value = self.call_hook(hook, (ctx_tbl, ev_tbl))?;
        parse_actions_or_nil(value)
    }

    pub fn on_button(
        &self,
        ctx: &ModRoomContext,
        player: &ModPlayerSnapshot,
        button: &str,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        let Some(hook) = &self.on_button else {
            return Ok(Vec::new());
        };
        let ctx_tbl = marshal_context(&self.lua, ctx).map_err(map_mlua_error)?;
        let player_tbl = marshal_player(&self.lua, player).map_err(map_mlua_error)?;
        let value = self.call_hook(hook, (ctx_tbl, player_tbl, button))?;
        parse_actions_or_nil(value)
    }

    pub fn on_chat(
        &self,
        ctx: &ModRoomContext,
        sender: &str,
        text: &str,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        let Some(hook) = &self.on_chat else {
            return Ok(Vec::new());
        };
        let ctx_tbl = marshal_context(&self.lua, ctx).map_err(map_mlua_error)?;
        let value = self.call_hook(hook, (ctx_tbl, sender, text))?;
        parse_actions_or_nil(value)
    }

    pub fn on_tick(
        &self,
        ctx: &ModRoomContext,
        dt_secs: f64,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        let Some(hook) = &self.on_tick else {
            return Ok(Vec::new());
        };
        let ctx_tbl = marshal_context(&self.lua, ctx).map_err(map_mlua_error)?;
        let value = self.call_hook(hook, (ctx_tbl, dt_secs))?;
        parse_actions_or_nil(value)
    }

    fn call_hook<A: mlua::IntoLuaMulti>(
        &self,
        hook: &Function,
        args: A,
    ) -> Result<Value, ModScriptError> {
        self.remaining_hooks
            .store(MOD_HOOK_BUDGET, Ordering::Relaxed);
        self.budget_exhausted.store(false, Ordering::Relaxed);
        match hook.call::<Value>(args) {
            Ok(value) => Ok(value),
            Err(error) => {
                if self.budget_exhausted.load(Ordering::Relaxed) {
                    return Err(ModScriptError::InstructionBudgetExceeded {
                        phase: ModExecutionPhase::Hook,
                    });
                }
                Err(map_mlua_error(error))
            }
        }
    }
}

pub struct ModHost {
    active: ModScript,
    generation: u64,
    digest: [u8; 32],
    path: Option<PathBuf>,
}

impl ModHost {
    pub fn from_source(chunk_name: &str, source: &str) -> Result<Self, ModScriptError> {
        let active = ModScript::from_source(chunk_name, source)?;
        let digest = sha256_bytes(source.as_bytes());
        Ok(Self {
            active,
            generation: 1,
            digest,
            path: None,
        })
    }

    pub fn from_file(path: &Path) -> io::Result<Self> {
        let source = read_bounded_source(path)?;
        let name = format!("={}", path.display());
        let mut host = Self::from_source(&name, &source).map_err(io::Error::from)?;
        host.path = Some(path.to_path_buf());
        Ok(host)
    }

    pub fn reload_source(
        &mut self,
        chunk_name: &str,
        source: &str,
    ) -> Result<ModStatus, ModScriptError> {
        let new_digest = sha256_bytes(source.as_bytes());
        if new_digest == self.digest {
            return Ok(self.status(false));
        }
        let next = ModScript::from_source(chunk_name, source)?;
        self.active = next;
        self.digest = new_digest;
        self.generation = self.generation.saturating_add(1);
        Ok(self.status(true))
    }

    pub fn reload_file(&mut self) -> io::Result<ModStatus> {
        let Some(path) = self.path.clone() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "no mod script file path configured",
            ));
        };
        let source = read_bounded_source(&path)?;
        let name = format!("={}", path.display());
        self.reload_source(&name, &source).map_err(io::Error::from)
    }

    pub fn status(&self, changed: bool) -> ModStatus {
        let info = self.active.info();
        ModStatus {
            name: info.name.clone(),
            version: info.version.clone(),
            author: info.author.clone(),
            abi_version: MOD_ABI_VERSION,
            generation: self.generation,
            digest: hex_digest(&self.digest),
            changed,
        }
    }

    pub fn script(&self) -> &ModScript {
        &self.active
    }
}

fn read_bounded_source(path: &Path) -> io::Result<String> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > MOD_MAX_SOURCE_BYTES as u64 {
        let bytes = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
        return Err(ModScriptError::SourceTooLarge {
            bytes,
            max: MOD_MAX_SOURCE_BYTES,
        }
        .into());
    }
    fs::read_to_string(path)
}

fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn hex_digest(digest: &[u8; 32]) -> String {
    let mut out = String::with_capacity(16);
    for byte in &digest[..8] {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn build_sandbox_env(lua: &Lua) -> mlua::Result<Table> {
    let env = lua.create_table()?;
    let math = lua.create_table()?;
    math.raw_set("abs", lua.create_function(|_, x: f64| Ok(x.abs()))?)?;
    math.raw_set("floor", lua.create_function(|_, x: f64| Ok(x.floor()))?)?;
    math.raw_set("ceil", lua.create_function(|_, x: f64| Ok(x.ceil()))?)?;
    math.raw_set("sqrt", lua.create_function(|_, x: f64| Ok(x.sqrt()))?)?;
    math.raw_set(
        "min",
        lua.create_function(|_, (a, b): (f64, f64)| Ok(a.min(b)))?,
    )?;
    math.raw_set(
        "max",
        lua.create_function(|_, (a, b): (f64, f64)| Ok(a.max(b)))?,
    )?;
    math.raw_set(
        "clamp",
        lua.create_function(|_, (x, lo, hi): (f64, f64, f64)| Ok(x.clamp(lo, hi)))?,
    )?;
    env.raw_set("math", math)?;
    env.raw_set("tostring", lua.globals().raw_get::<Value>("tostring")?)?;
    env.raw_set("tonumber", lua.globals().raw_get::<Value>("tonumber")?)?;
    env.raw_set("pairs", lua.globals().raw_get::<Value>("pairs")?)?;
    env.raw_set("ipairs", lua.globals().raw_get::<Value>("ipairs")?)?;
    Ok(env)
}

fn optional_fn(table: &Table, name: &str) -> Result<Option<Function>, ModScriptError> {
    let value = table.raw_get::<Value>(name).map_err(map_mlua_error)?;
    match value {
        Value::Nil => Ok(None),
        Value::Function(func) => Ok(Some(func)),
        _ => Err(ModScriptError::InvalidAbi(format!(
            "mod script field `{name}` must be a function or nil"
        ))),
    }
}

fn parse_info(table: &Table) -> Result<ModInfo, ModScriptError> {
    let mut info = ModInfo::default();
    let val = table.raw_get::<Value>("info").map_err(map_mlua_error)?;
    let Value::Table(tbl) = val else {
        return Ok(info);
    };
    if let Some(name) = optional_string(&tbl, "name")? {
        info.name = name;
    }
    if let Some(version) = optional_string(&tbl, "version")? {
        info.version = version;
    }
    if let Some(author) = optional_string(&tbl, "author")? {
        info.author = author;
    }
    Ok(info)
}

fn parse_rules(table: &Table) -> Result<ModRules, ModScriptError> {
    let mut rules = ModRules::default();
    let val = table.raw_get::<Value>("rules").map_err(map_mlua_error)?;
    let Value::Table(tbl) = val else {
        return Ok(rules);
    };
    if let Some(scale) = optional_scale(&tbl, "damage_scale")? {
        rules.damage_scale = scale;
    }
    if let Some(scale) = optional_scale(&tbl, "healing_scale")? {
        rules.healing_scale = scale;
    }
    if let Some(scale) = optional_scale(&tbl, "gravity_scale")? {
        rules.gravity_scale = scale;
    }
    if let Some(scale) = optional_scale(&tbl, "speed_scale")? {
        rules.speed_scale = scale;
    }
    if let Some(scale) = optional_scale(&tbl, "ult_charge_scale")? {
        rules.ult_charge_scale = scale;
    }
    if let Some(scale) = optional_scale(&tbl, "cooldown_scale")? {
        rules.cooldown_scale = scale;
    }
    let ff = tbl
        .raw_get::<Value>("friendly_fire")
        .map_err(map_mlua_error)?;
    if let Value::Boolean(enabled) = ff {
        rules.friendly_fire = enabled;
    }
    Ok(rules)
}

fn optional_string(table: &Table, key: &str) -> Result<Option<String>, ModScriptError> {
    let value = table.raw_get::<Value>(key).map_err(map_mlua_error)?;
    match value {
        Value::Nil => Ok(None),
        Value::String(text) => {
            let s = text
                .to_str()
                .map_err(|err| ModScriptError::InvalidAbi(err.to_string()))?;
            Ok(Some(s.trim().to_owned()))
        }
        _ => Err(ModScriptError::InvalidAbi(format!(
            "field `{key}` must be a string"
        ))),
    }
}

fn optional_scale(table: &Table, key: &str) -> Result<Option<f64>, ModScriptError> {
    let value = table.raw_get::<Value>(key).map_err(map_mlua_error)?;
    if matches!(value, Value::Nil) {
        return Ok(None);
    }
    let scale = parse_non_negative_f64_value(value, key)?;
    if scale > 10.0 {
        return Err(ModScriptError::InvalidAbi(format!(
            "rule `{key}` ({scale}) must be within [0.0, 10.0]"
        )));
    }
    Ok(Some(scale))
}

fn marshal_context(lua: &Lua, ctx: &ModRoomContext) -> mlua::Result<Table> {
    let tbl = lua.create_table()?;
    tbl.raw_set("abi_version", i64::from(MOD_ABI_VERSION))?;
    tbl.raw_set("tick", i64::from(ctx.tick))?;
    tbl.raw_set("elapsed_secs", ctx.elapsed_secs)?;
    tbl.raw_set("map_guid", ctx.map_guid as i64)?;
    tbl.raw_set("mode_guid", ctx.mode_guid as i64)?;
    tbl.raw_set("player_count", i64::try_from(ctx.player_count).unwrap_or(0))?;
    Ok(tbl)
}

fn marshal_player(lua: &Lua, player: &ModPlayerSnapshot) -> mlua::Result<Table> {
    let tbl = lua.create_table()?;
    tbl.raw_set("entity", i64::from(player.entity))?;
    tbl.raw_set("name", player.name.as_str())?;
    tbl.raw_set("team", i64::from(player.team))?;
    tbl.raw_set("hero_guid", player.hero_guid as i64)?;
    tbl.raw_set("health", player.health)?;
    tbl.raw_set("max_health", player.max_health)?;
    tbl.raw_set("alive", player.alive)?;
    let pos = lua.create_table()?;
    pos.raw_set(1, player.position[0])?;
    pos.raw_set(2, player.position[1])?;
    pos.raw_set(3, player.position[2])?;
    tbl.raw_set("position", pos)?;
    Ok(tbl)
}

fn parse_actions_or_nil(value: Value) -> Result<Vec<ModAction>, ModScriptError> {
    if matches!(value, Value::Nil) {
        return Ok(Vec::new());
    }
    let Value::Table(table) = value else {
        return Err(ModScriptError::InvalidAction(String::from(
            "hook must return nil or an action array table",
        )));
    };
    let raw_len = table.raw_len();
    if raw_len > MOD_MAX_ACTIONS_PER_EVENT {
        return Err(ModScriptError::ActionBudgetExceeded {
            count: raw_len,
            max: MOD_MAX_ACTIONS_PER_EVENT,
        });
    }
    let mut actions = Vec::with_capacity(raw_len);
    for idx in 1..=raw_len {
        let entry = table.raw_get::<Value>(idx).map_err(map_mlua_error)?;
        let Value::Table(action_tbl) = entry else {
            return Err(ModScriptError::InvalidAction(format!(
                "action at index {idx} must be a table"
            )));
        };
        actions.push(parse_single_action(&action_tbl)?);
    }
    Ok(actions)
}

fn parse_single_action(table: &Table) -> Result<ModAction, ModScriptError> {
    let kind_val = table.raw_get::<Value>("kind").map_err(map_mlua_error)?;
    let Value::String(kind_str) = kind_val else {
        return Err(ModScriptError::InvalidAction(String::from(
            "action must contain string field `kind`",
        )));
    };
    let kind = kind_str
        .to_str()
        .map_err(|err| ModScriptError::InvalidAction(format!("invalid action kind: {err}")))?;

    match &*kind {
        "set_health" => Ok(ModAction::SetHealth {
            entity: parse_u32_field(table, "entity")?,
            health: parse_non_negative_f64_field(table, "health")?,
        }),
        "damage" => Ok(ModAction::Damage {
            entity: parse_u32_field(table, "entity")?,
            amount: parse_non_negative_f64_field(table, "amount")?,
        }),
        "heal" => Ok(ModAction::Heal {
            entity: parse_u32_field(table, "entity")?,
            amount: parse_non_negative_f64_field(table, "amount")?,
        }),
        "apply_impulse" => Ok(ModAction::ApplyImpulse {
            entity: parse_u32_field(table, "entity")?,
            velocity: parse_vec3_field(table, "velocity")?,
        }),
        "teleport" => Ok(ModAction::Teleport {
            entity: parse_u32_field(table, "entity")?,
            position: parse_vec3_field(table, "position")?,
        }),
        "set_speed_scale" => Ok(ModAction::SetSpeedScale {
            entity: parse_u32_field(table, "entity")?,
            scale: parse_bounded_scale_field(table, "scale")?,
        }),
        "set_gravity_scale" => Ok(ModAction::SetGravityScale {
            entity: parse_u32_field(table, "entity")?,
            scale: parse_bounded_scale_field(table, "scale")?,
        }),
        "play_voice_cue" => Ok(ModAction::PlayVoiceCue {
            entity: parse_u32_field(table, "entity")?,
            stimulus: parse_u64_field(table, "stimulus")?,
        }),
        "broadcast_chat" => {
            let text_val = table.raw_get::<Value>("text").map_err(map_mlua_error)?;
            let Value::String(text_str) = text_val else {
                return Err(ModScriptError::InvalidAction(String::from(
                    "broadcast_chat `text` must be a string",
                )));
            };
            let text = text_str
                .to_str()
                .map_err(|err| ModScriptError::InvalidAction(err.to_string()))?;
            let clipped: String = text.chars().take(MOD_MAX_CHAT_LEN).collect();
            Ok(ModAction::BroadcastChat { text: clipped })
        }
        "reset_interactive_prop" => Ok(ModAction::ResetInteractiveProp {
            prop_id: parse_u32_field(table, "prop_id")?,
        }),
        "set_room_rule" => {
            let key_val = table.raw_get::<Value>("key").map_err(map_mlua_error)?;
            let Value::String(key_str) = key_val else {
                return Err(ModScriptError::InvalidAction(String::from(
                    "set_room_rule `key` must be a string",
                )));
            };
            let key = key_str
                .to_str()
                .map_err(|err| ModScriptError::InvalidAction(err.to_string()))?
                .to_owned();
            let value = parse_bounded_scale_field(table, "value")?;
            Ok(ModAction::SetRoomRule { key, value })
        }
        other => Err(ModScriptError::InvalidAction(format!(
            "unknown mod action kind `{other}`"
        ))),
    }
}

fn parse_u32_field(table: &Table, key: &'static str) -> Result<u32, ModScriptError> {
    let value = table.raw_get::<Value>(key).map_err(map_mlua_error)?;
    let Value::Integer(raw) = value else {
        return Err(ModScriptError::InvalidAction(format!(
            "action field `{key}` must be an integer"
        )));
    };
    u32::try_from(raw).map_err(|_| {
        ModScriptError::InvalidAction(format!("action field `{key}` ({raw}) is out of u32 range"))
    })
}

fn parse_u64_field(table: &Table, key: &'static str) -> Result<u64, ModScriptError> {
    let value = table.raw_get::<Value>(key).map_err(map_mlua_error)?;
    let Value::Integer(raw) = value else {
        return Err(ModScriptError::InvalidAction(format!(
            "action field `{key}` must be an integer"
        )));
    };
    u64::try_from(raw).map_err(|_| {
        ModScriptError::InvalidAction(format!("action field `{key}` ({raw}) must be non-negative"))
    })
}

fn parse_non_negative_f64_value(value: Value, key: &str) -> Result<f64, ModScriptError> {
    let num = match value {
        Value::Number(v) => v,
        Value::Integer(v) => v as f64,
        _ => {
            return Err(ModScriptError::InvalidAction(format!(
                "field `{key}` must be a number"
            )));
        }
    };
    if !num.is_finite() || num < 0.0 {
        return Err(ModScriptError::InvalidAction(format!(
            "field `{key}` ({num}) must be finite and non-negative"
        )));
    }
    Ok(num)
}

fn parse_non_negative_f64_field(table: &Table, key: &'static str) -> Result<f64, ModScriptError> {
    let value = table.raw_get::<Value>(key).map_err(map_mlua_error)?;
    parse_non_negative_f64_value(value, key)
}

fn parse_bounded_scale_field(table: &Table, key: &'static str) -> Result<f64, ModScriptError> {
    let scale = parse_non_negative_f64_field(table, key)?;
    if scale > 10.0 {
        return Err(ModScriptError::InvalidAction(format!(
            "field `{key}` ({scale}) must be within [0.0, 10.0]"
        )));
    }
    Ok(scale)
}

fn parse_vec3_field(table: &Table, key: &'static str) -> Result<[f64; 3], ModScriptError> {
    let value = table.raw_get::<Value>(key).map_err(map_mlua_error)?;
    let Value::Table(vec_tbl) = value else {
        return Err(ModScriptError::InvalidAction(format!(
            "action field `{key}` must be a 3-element array"
        )));
    };
    let mut out = [0.0; 3];
    for (idx, slot) in out.iter_mut().enumerate() {
        let entry = vec_tbl.raw_get::<Value>(idx + 1).map_err(map_mlua_error)?;
        let num = match entry {
            Value::Number(v) => v,
            Value::Integer(v) => v as f64,
            _ => {
                return Err(ModScriptError::InvalidAction(format!(
                    "action field `{key}[{}]` must be a number",
                    idx + 1
                )));
            }
        };
        if !num.is_finite() {
            return Err(ModScriptError::InvalidAction(format!(
                "action field `{key}[{}]` must be finite",
                idx + 1
            )));
        }
        *slot = num;
    }
    Ok(out)
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

fn map_mlua_error(error: mlua::Error) -> ModScriptError {
    if is_memory_error(&error) {
        return ModScriptError::MemoryLimitExceeded;
    }
    match error {
        mlua::Error::SyntaxError {
            message,
            incomplete_input: _,
        } => ModScriptError::Syntax(message),
        other => ModScriptError::Runtime(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ctx() -> ModRoomContext {
        ModRoomContext {
            tick: 120,
            elapsed_secs: 1.92,
            map_guid: 0x0800_0000_0000_0688,
            mode_guid: 0x0B00_0000_0000_0001,
            player_count: 1,
        }
    }

    fn sample_player() -> ModPlayerSnapshot {
        ModPlayerSnapshot {
            entity: 1,
            name: String::from("Researcher"),
            team: 0,
            hero_guid: 0x02E0_0000_0000_006E,
            health: 200.0,
            max_health: 200.0,
            position: [1.0, 2.0, 3.0],
            alive: true,
        }
    }

    #[test]
    fn mod_sandbox_blocks_os_io_and_enforces_abi_and_budgets() {
        let probe = ModScript::from_source(
            "=sandbox.lua",
            r#"
            if os ~= nil or io ~= nil or require ~= nil or debug ~= nil then
                error("sandbox leak")
            end
            return { abi = 1, info = { name = "Sandbox OK" } }
            "#,
        )
        .unwrap();
        assert_eq!(probe.info().name, "Sandbox OK");

        let bad_abi = ModScript::from_source("=bad_abi.lua", "return { abi = 99 }");
        assert!(matches!(
            bad_abi,
            Err(ModScriptError::UnsupportedAbi {
                found: 99,
                expected: 1
            })
        ));

        let oom = ModScript::from_source(
            "=oom.lua",
            r#"
            local s = "0123456789abcdef"
            for i = 1, 16 do s = s .. s end
            return { abi = 1 }
            "#,
        );
        assert!(matches!(oom, Err(ModScriptError::MemoryLimitExceeded)));

        let inf_loop = ModScript::from_source("=loop.lua", "while true do end");
        assert!(matches!(
            inf_loop,
            Err(ModScriptError::InstructionBudgetExceeded {
                phase: ModExecutionPhase::Init
            })
        ));
    }

    #[test]
    fn mod_host_hot_reload_rolls_back_on_error() {
        let mut host = ModHost::from_source(
            "=v1.lua",
            r#"
            return {
                abi = 1,
                info = { name = "V1", version = "1.0" },
                rules = { gravity_scale = 0.5 },
            }
            "#,
        )
        .unwrap();
        assert_eq!(host.script().rules().gravity_scale, 0.5);

        let err = host
            .reload_source("=broken.lua", "return { abi = 1, info = ")
            .unwrap_err();
        assert!(matches!(err, ModScriptError::Syntax(_)));
        assert_eq!(host.script().info().name, "V1");
        assert_eq!(host.status(false).generation, 1);

        let status = host
            .reload_source(
                "=v2.lua",
                r#"
                return {
                    abi = 1,
                    info = { name = "V2", version = "2.0" },
                    rules = { gravity_scale = 0.75 },
                }
                "#,
            )
            .unwrap();
        assert!(status.changed);
        assert_eq!(status.generation, 2);
        assert_eq!(host.script().info().name, "V2");
        assert_eq!(host.script().rules().gravity_scale, 0.75);
    }

    #[test]
    fn example_mods_compile_and_execute_all_hooks() {
        let ctx = sample_ctx();
        let player = sample_player();

        let low_grav = ModScript::from_source(
            "=low_gravity_arena.lua",
            include_str!("../scripts/mods/low_gravity_arena.lua"),
        )
        .unwrap();
        assert_eq!(low_grav.info().name, "Low-Gravity Arena");
        assert!((low_grav.rules().gravity_scale - 0.45).abs() < 1e-6);
        assert_eq!(
            low_grav.on_button(&ctx, &player, "jump").unwrap(),
            vec![ModAction::ApplyImpulse {
                entity: 1,
                velocity: [0.0, 4.5, 0.0],
            }]
        );
        let chat_actions = low_grav.on_chat(&ctx, "Researcher", "!grav").unwrap();
        assert!(matches!(
            chat_actions.first(),
            Some(ModAction::SetRoomRule { key, value }) if key == "gravity_scale" && (*value - 0.45).abs() < 1e-6
        ));

        let hs = ModScript::from_source(
            "=headshot_only_practice.lua",
            include_str!("../scripts/mods/headshot_only_practice.lua"),
        )
        .unwrap();
        assert_eq!(hs.info().name, "Headshot-Only Practice");

        let body_shot = hs
            .on_damage(
                &ctx,
                &ModDamageEvent {
                    attacker: 1,
                    victim: 2,
                    damage: 40.0,
                    headshot: false,
                    melee: false,
                    ability_slot: 0,
                },
            )
            .unwrap();
        assert_eq!(body_shot.damage, 0.0);
        assert!(body_shot.actions.is_empty());

        let crit_shot = hs
            .on_damage(
                &ctx,
                &ModDamageEvent {
                    attacker: 1,
                    victim: 2,
                    damage: 80.0,
                    headshot: true,
                    melee: false,
                    ability_slot: 0,
                },
            )
            .unwrap();
        assert_eq!(crit_shot.damage, 80.0);
        assert_eq!(
            crit_shot.actions,
            vec![ModAction::Heal {
                entity: 1,
                amount: 25.0,
            }]
        );

        let kill_actions = hs
            .on_kill(
                &ctx,
                &ModKillEvent {
                    killer: 1,
                    victim: 2,
                    headshot: true,
                },
            )
            .unwrap();
        assert_eq!(kill_actions.len(), 1);
        assert!(matches!(
            hs.on_chat(&ctx, "Researcher", "!hs").unwrap().as_slice(),
            [ModAction::BroadcastChat { text }] if text.contains('1')
        ));
    }
}
