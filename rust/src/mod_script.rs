//! Bounded, sandboxed Lua 5.4 host for ORPHEUS Workshop / Mod scripts.
//!
//! Mod scripts run in isolated per-room Lua 5.4 VMs (512 KiB memory cap,
//! bounded instruction hooks, zero filesystem/OS/network/debug access) and
//! support both ABI v1 (legacy) and ABI v2 (full Workshop API: emotes/dance,
//! voicelines, sprays, hero switching, status effects, ult/cooldown control,
//! slow-motion time scale, bot spawning/AI, custom projectiles, and live
//! read-only world raycast & radius queries).

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
};

use mlua::{ChunkMode, Function, HookTriggers, Lua, LuaOptions, MultiValue, StdLib, Table, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Latest version of the bounded Lua Mod / Workshop ABI.
pub const MOD_ABI_VERSION: u32 = 2;

/// Minimum supported Lua Mod ABI version (ABI v1 scripts remain fully compatible).
pub const MOD_MIN_ABI_VERSION: u32 = 1;

/// Bounds per-room Lua VM heap to 512 KiB so faulty mods cannot exhaust worker memory.
pub const MOD_MEMORY_LIMIT_BYTES: usize = 512 * 1024;

/// Samples the Lua instruction hook every 64 VM instructions to bound overshoot with low overhead.
pub const MOD_INSTRUCTION_HOOK_INTERVAL: u32 = 64;

/// Caps chunk initialization at 256 * 64 = 16,384 instructions.
pub const MOD_INIT_HOOK_BUDGET: u32 = 256;

/// Caps each mod event hook invocation at 256 * 64 = 16,384 instructions.
pub const MOD_HOOK_BUDGET: u32 = 256;

/// Caps mod script source text at 128 KiB.
pub const MOD_MAX_SOURCE_BYTES: usize = 128 * 1024;

/// Caps actions returned by a single mod hook call.
pub const MOD_MAX_ACTIONS_PER_EVENT: usize = 64;

/// Caps chat broadcast length emitted by a mod hook.
pub const MOD_MAX_CHAT_LEN: usize = 240;

/// Caps `string.rep` output bytes in the Lua sandbox.
const MOD_MAX_STRING_REP_BYTES: usize = 4096;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModInfo {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
}

impl Default for ModInfo {
    fn default() -> Self {
        Self {
            name: String::from("Unnamed Mod"),
            version: String::from("0.1.0"),
            author: String::from("Unknown"),
            description: String::new(),
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
    pub time_scale: f64,
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
            time_scale: 1.0,
            friendly_fire: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModPlayerSnapshot {
    pub entity: u32,
    pub name: String,
    pub team: u8,
    pub hero_guid: u64,
    pub hero_name: String,
    pub health: f64,
    pub max_health: f64,
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub yaw: f64,
    pub pitch: f64,
    pub facing: [f64; 3],
    pub ult_charge: f64,
    pub alive: bool,
    pub crouching: bool,
    pub grounded: bool,
    pub emoting: bool,
    pub is_bot: bool,
}

impl Default for ModPlayerSnapshot {
    fn default() -> Self {
        Self {
            entity: 0,
            name: String::new(),
            team: 0,
            hero_guid: 0,
            hero_name: String::new(),
            health: 0.0,
            max_health: 0.0,
            position: [0.0; 3],
            velocity: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            facing: [0.0, 0.0, 1.0],
            ult_charge: 0.0,
            alive: false,
            crouching: false,
            grounded: true,
            emoting: false,
            is_bot: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModRoomContext {
    pub tick: u32,
    pub elapsed_secs: f64,
    pub map_guid: u64,
    pub mode_guid: u64,
    pub player_count: usize,
    pub time_scale: f64,
    pub rules: ModRules,
    pub sender_entity: Option<u32>,
    pub players: Vec<ModPlayerSnapshot>,
    pub bots: Vec<ModPlayerSnapshot>,
}

impl Default for ModRoomContext {
    fn default() -> Self {
        Self {
            tick: 0,
            elapsed_secs: 0.0,
            map_guid: 0,
            mode_guid: 0,
            player_count: 0,
            time_scale: 1.0,
            rules: ModRules::default(),
            sender_entity: None,
            players: Vec::new(),
            bots: Vec::new(),
        }
    }
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModHealEvent {
    pub healer: u32,
    pub target: u32,
    pub amount: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModKillEvent {
    pub killer: u32,
    pub victim: u32,
    pub headshot: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModRaycastHit {
    pub distance: f64,
    pub point: [f64; 3],
    pub normal: [f64; 3],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModHeroInfo {
    pub guid: u64,
    pub name: String,
    pub role: String,
}

/// Read-only world query interface exposed to Lua Workshop hooks as the `world` parameter.
pub trait ModWorldView {
    fn raycast(
        &self,
        _origin: [f64; 3],
        _direction: [f64; 3],
        _max_distance: f64,
    ) -> Option<ModRaycastHit> {
        None
    }

    fn players_in_radius(
        &self,
        _center: [f64; 3],
        _radius: f64,
        _team: Option<u8>,
    ) -> Vec<ModPlayerSnapshot> {
        Vec::new()
    }

    fn get_entity(&self, _entity: u32) -> Option<ModPlayerSnapshot> {
        None
    }

    fn find_hero(&self, _query: &str) -> Option<ModHeroInfo> {
        None
    }

    fn heroes(&self) -> Vec<ModHeroInfo> {
        Vec::new()
    }
}

/// Default empty world query view used when no live room geometry/catalog is attached.
#[derive(Clone, Copy, Debug, Default)]
pub struct EmptyModWorldView;

impl ModWorldView for EmptyModWorldView {}

#[derive(Clone, Debug, PartialEq)]
pub enum ModAction {
    // --- Health & Vitals ---
    SetHealth {
        entity: u32,
        health: f64,
    },
    Damage {
        entity: u32,
        amount: f64,
    },
    Heal {
        entity: u32,
        amount: f64,
    },
    Kill {
        entity: u32,
    },
    Respawn {
        entity: u32,
    },
    AddHealthPool {
        entity: u32,
        kind: String,
        amount: f64,
    },
    // --- Kinematics & Facing ---
    ApplyImpulse {
        entity: u32,
        velocity: [f64; 3],
    },
    SetVelocity {
        entity: u32,
        velocity: [f64; 3],
    },
    Teleport {
        entity: u32,
        position: [f64; 3],
    },
    SetFacing {
        entity: u32,
        yaw: f64,
        pitch: f64,
    },
    SetSpeedScale {
        entity: u32,
        scale: f64,
    },
    SetGravityScale {
        entity: u32,
        scale: f64,
    },
    // --- Per-Actor Combat Modifiers ---
    SetDamageScale {
        entity: u32,
        scale: f64,
    },
    SetDamageTakenScale {
        entity: u32,
        scale: f64,
    },
    SetHealingScale {
        entity: u32,
        scale: f64,
    },
    // --- Emotes / Dance / Voicelines / Sprays / Comms / Voice Cues ---
    PlayEmote {
        entity: u32,
        slot: u8,
    },
    PlayVoiceline {
        entity: u32,
        slot: u8,
    },
    PlaceSpray {
        entity: u32,
        slot: u8,
    },
    Communication {
        entity: u32,
        kind: String,
    },
    PlayVoiceCue {
        entity: u32,
        stimulus: u64,
    },
    // --- Hero, Status Effects, Ultimate & Cooldowns ---
    SetHero {
        entity: u32,
        hero: String,
    },
    SetStatus {
        entity: u32,
        status: String,
        duration: f64,
    },
    ClearStatus {
        entity: u32,
        status: String,
    },
    SetUltCharge {
        entity: u32,
        percent: f64,
    },
    AddUltCharge {
        entity: u32,
        amount: f64,
    },
    ResetCooldowns {
        entity: u32,
    },
    // --- Bots & Custom Projectiles ---
    SpawnBot {
        hero: String,
        team: u8,
        position: [f64; 3],
        name: Option<String>,
    },
    DespawnBot {
        entity: u32,
    },
    SetBotAi {
        entity: u32,
        mode: String,
    },
    SpawnProjectile {
        source: u32,
        origin: [f64; 3],
        direction: [f64; 3],
        speed: f64,
        damage: f64,
        radius: f64,
        gravity: f64,
    },
    // --- Room Rules, Time Scale, Props & Chat ---
    SetTimeScale {
        scale: f64,
    },
    ResetInteractiveProp {
        prop_id: u32,
    },
    ApplyPropImpulse {
        prop_id: u32,
        velocity: [f64; 3],
    },
    SetRoomRule {
        key: String,
        value: f64,
    },
    BroadcastChat {
        text: String,
    },
    SendChat {
        entity: u32,
        text: String,
    },
    Announce {
        text: String,
    },
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
    #[error(
        "mod script ABI version {found} is unsupported (expected {MOD_MIN_ABI_VERSION}..={expected})"
    )]
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
    abi_version: u32,
    info: ModInfo,
    rules: ModRules,
    on_room_start: Option<Function>,
    on_player_join: Option<Function>,
    on_player_leave: Option<Function>,
    on_player_spawn: Option<Function>,
    on_hero_switch: Option<Function>,
    on_damage: Option<Function>,
    on_heal: Option<Function>,
    on_kill: Option<Function>,
    on_button: Option<Function>,
    on_emote: Option<Function>,
    on_chat: Option<Function>,
    on_tick: Option<Function>,
    remaining_hooks: Arc<AtomicU32>,
    budget_exhausted: Arc<AtomicBool>,
}

impl ModScript {
    pub fn from_source(chunk_name: &str, source: &str) -> Result<Self, ModScriptError> {
        validate_mod_source_bounds(source)?;
        // Load ONLY pure table/string/math/utf8 libraries into the VM (never os, io, package, debug).
        // Even from these, build_sandbox_env selectively whitelists safe entries into an isolated _ENV.
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
            LuaOptions::default(),
        )
        .map_err(map_mlua_error)?;
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

        let env =
            build_sandbox_env(&lua, sha256_seed(source.as_bytes())).map_err(map_mlua_error)?;
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
                "mod script must return a table { abi = 1|2, ... }",
            )));
        };

        let abi = table.raw_get::<Value>("abi").map_err(map_mlua_error)?;
        let Value::Integer(version) = abi else {
            return Err(ModScriptError::InvalidAbi(String::from(
                "mod script table must contain integer field `abi`",
            )));
        };
        if version < i64::from(MOD_MIN_ABI_VERSION) || version > i64::from(MOD_ABI_VERSION) {
            return Err(ModScriptError::UnsupportedAbi {
                found: version,
                expected: MOD_ABI_VERSION,
            });
        }
        let abi_version = version as u32;

        let info = parse_info(&table)?;
        let rules = parse_rules(&table)?;

        Ok(Self {
            abi_version,
            on_room_start: optional_fn(&table, "on_room_start")?,
            on_player_join: optional_fn(&table, "on_player_join")?,
            on_player_leave: optional_fn(&table, "on_player_leave")?,
            on_player_spawn: optional_fn(&table, "on_player_spawn")?,
            on_hero_switch: optional_fn(&table, "on_hero_switch")?,
            on_damage: optional_fn(&table, "on_damage")?,
            on_heal: optional_fn(&table, "on_heal")?,
            on_kill: optional_fn(&table, "on_kill")?,
            on_button: optional_fn(&table, "on_button")?,
            on_emote: optional_fn(&table, "on_emote")?,
            on_chat: optional_fn(&table, "on_chat")?,
            on_tick: optional_fn(&table, "on_tick")?,
            lua,
            info,
            rules,
            remaining_hooks,
            budget_exhausted,
        })
    }

    pub fn abi_version(&self) -> u32 {
        self.abi_version
    }

    pub fn info(&self) -> &ModInfo {
        &self.info
    }

    pub fn rules(&self) -> ModRules {
        self.rules
    }

    pub fn has_on_tick(&self) -> bool {
        self.on_tick.is_some()
    }

    pub fn has_on_damage(&self) -> bool {
        self.on_damage.is_some()
    }

    pub fn has_on_heal(&self) -> bool {
        self.on_heal.is_some()
    }

    pub fn on_room_start(&self, ctx: &ModRoomContext) -> Result<Vec<ModAction>, ModScriptError> {
        self.on_room_start_with_world(ctx, &EmptyModWorldView)
    }

    pub fn on_room_start_with_world(
        &self,
        ctx: &ModRoomContext,
        world: &dyn ModWorldView,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        let Some(hook) = &self.on_room_start else {
            return Ok(Vec::new());
        };
        self.call_scoped_actions(hook, world, |lua, world_tbl| {
            let ctx_tbl = marshal_context(lua, ctx)?;
            Ok(MultiValue::from_vec(vec![
                Value::Table(ctx_tbl),
                Value::Table(world_tbl),
            ]))
        })
    }

    pub fn on_player_join_with_world(
        &self,
        ctx: &ModRoomContext,
        player: &ModPlayerSnapshot,
        world: &dyn ModWorldView,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        let Some(hook) = &self.on_player_join else {
            return Ok(Vec::new());
        };
        self.call_scoped_actions(hook, world, |lua, world_tbl| {
            let ctx_tbl = marshal_context(lua, ctx)?;
            let player_tbl = marshal_player(lua, player)?;
            Ok(MultiValue::from_vec(vec![
                Value::Table(ctx_tbl),
                Value::Table(player_tbl),
                Value::Table(world_tbl),
            ]))
        })
    }

    pub fn on_player_leave_with_world(
        &self,
        ctx: &ModRoomContext,
        player: &ModPlayerSnapshot,
        world: &dyn ModWorldView,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        let Some(hook) = &self.on_player_leave else {
            return Ok(Vec::new());
        };
        self.call_scoped_actions(hook, world, |lua, world_tbl| {
            let ctx_tbl = marshal_context(lua, ctx)?;
            let player_tbl = marshal_player(lua, player)?;
            Ok(MultiValue::from_vec(vec![
                Value::Table(ctx_tbl),
                Value::Table(player_tbl),
                Value::Table(world_tbl),
            ]))
        })
    }

    pub fn on_player_spawn(
        &self,
        ctx: &ModRoomContext,
        player: &ModPlayerSnapshot,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        self.on_player_spawn_with_world(ctx, player, &EmptyModWorldView)
    }

    pub fn on_player_spawn_with_world(
        &self,
        ctx: &ModRoomContext,
        player: &ModPlayerSnapshot,
        world: &dyn ModWorldView,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        let Some(hook) = &self.on_player_spawn else {
            return Ok(Vec::new());
        };
        self.call_scoped_actions(hook, world, |lua, world_tbl| {
            let ctx_tbl = marshal_context(lua, ctx)?;
            let player_tbl = marshal_player(lua, player)?;
            Ok(MultiValue::from_vec(vec![
                Value::Table(ctx_tbl),
                Value::Table(player_tbl),
                Value::Table(world_tbl),
            ]))
        })
    }

    pub fn on_hero_switch_with_world(
        &self,
        ctx: &ModRoomContext,
        player: &ModPlayerSnapshot,
        old_hero_guid: u64,
        new_hero_guid: u64,
        world: &dyn ModWorldView,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        let Some(hook) = &self.on_hero_switch else {
            return Ok(Vec::new());
        };
        self.call_scoped_actions(hook, world, |lua, world_tbl| {
            let ctx_tbl = marshal_context(lua, ctx)?;
            let player_tbl = marshal_player(lua, player)?;
            Ok(MultiValue::from_vec(vec![
                Value::Table(ctx_tbl),
                Value::Table(player_tbl),
                Value::Integer(old_hero_guid as i64),
                Value::Integer(new_hero_guid as i64),
                Value::Table(world_tbl),
            ]))
        })
    }

    pub fn on_damage(
        &self,
        ctx: &ModRoomContext,
        event: &ModDamageEvent,
    ) -> Result<ModDamageOutcome, ModScriptError> {
        self.on_damage_with_world(ctx, event, &EmptyModWorldView)
    }

    pub fn on_damage_with_world(
        &self,
        ctx: &ModRoomContext,
        event: &ModDamageEvent,
        world: &dyn ModWorldView,
    ) -> Result<ModDamageOutcome, ModScriptError> {
        let Some(hook) = &self.on_damage else {
            return Ok(ModDamageOutcome {
                damage: event.damage,
                actions: Vec::new(),
            });
        };
        let value = self.call_scoped_value(hook, world, |lua, world_tbl| {
            let ctx_tbl = marshal_context(lua, ctx)?;
            let ev_tbl = lua.create_table()?;
            ev_tbl.raw_set("attacker", i64::from(event.attacker))?;
            ev_tbl.raw_set("victim", i64::from(event.victim))?;
            ev_tbl.raw_set("damage", event.damage)?;
            ev_tbl.raw_set("headshot", event.headshot)?;
            ev_tbl.raw_set("melee", event.melee)?;
            ev_tbl.raw_set("ability_slot", i64::from(event.ability_slot))?;
            Ok(MultiValue::from_vec(vec![
                Value::Table(ctx_tbl),
                Value::Table(ev_tbl),
                Value::Table(world_tbl),
            ]))
        })?;
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

    pub fn on_heal_with_world(
        &self,
        ctx: &ModRoomContext,
        event: &ModHealEvent,
        world: &dyn ModWorldView,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        let Some(hook) = &self.on_heal else {
            return Ok(Vec::new());
        };
        self.call_scoped_actions(hook, world, |lua, world_tbl| {
            let ctx_tbl = marshal_context(lua, ctx)?;
            let ev_tbl = lua.create_table()?;
            ev_tbl.raw_set("healer", i64::from(event.healer))?;
            ev_tbl.raw_set("target", i64::from(event.target))?;
            ev_tbl.raw_set("amount", event.amount)?;
            Ok(MultiValue::from_vec(vec![
                Value::Table(ctx_tbl),
                Value::Table(ev_tbl),
                Value::Table(world_tbl),
            ]))
        })
    }

    pub fn on_kill(
        &self,
        ctx: &ModRoomContext,
        event: &ModKillEvent,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        self.on_kill_with_world(ctx, event, &EmptyModWorldView)
    }

    pub fn on_kill_with_world(
        &self,
        ctx: &ModRoomContext,
        event: &ModKillEvent,
        world: &dyn ModWorldView,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        let Some(hook) = &self.on_kill else {
            return Ok(Vec::new());
        };
        self.call_scoped_actions(hook, world, |lua, world_tbl| {
            let ctx_tbl = marshal_context(lua, ctx)?;
            let ev_tbl = lua.create_table()?;
            ev_tbl.raw_set("killer", i64::from(event.killer))?;
            ev_tbl.raw_set("victim", i64::from(event.victim))?;
            ev_tbl.raw_set("headshot", event.headshot)?;
            Ok(MultiValue::from_vec(vec![
                Value::Table(ctx_tbl),
                Value::Table(ev_tbl),
                Value::Table(world_tbl),
            ]))
        })
    }

    pub fn on_button(
        &self,
        ctx: &ModRoomContext,
        player: &ModPlayerSnapshot,
        button: &str,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        self.on_button_edge_with_world(ctx, player, button, true, &EmptyModWorldView)
    }

    pub fn on_button_edge_with_world(
        &self,
        ctx: &ModRoomContext,
        player: &ModPlayerSnapshot,
        button: &str,
        pressed: bool,
        world: &dyn ModWorldView,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        let Some(hook) = &self.on_button else {
            return Ok(Vec::new());
        };
        self.call_scoped_actions(hook, world, |lua, world_tbl| {
            let ctx_tbl = marshal_context(lua, ctx)?;
            let player_tbl = marshal_player(lua, player)?;
            let btn_str = lua.create_string(button)?;
            Ok(MultiValue::from_vec(vec![
                Value::Table(ctx_tbl),
                Value::Table(player_tbl),
                Value::String(btn_str),
                Value::Boolean(pressed),
                Value::Table(world_tbl),
            ]))
        })
    }

    pub fn on_emote_with_world(
        &self,
        ctx: &ModRoomContext,
        player: &ModPlayerSnapshot,
        slot: u8,
        world: &dyn ModWorldView,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        let Some(hook) = &self.on_emote else {
            return Ok(Vec::new());
        };
        self.call_scoped_actions(hook, world, |lua, world_tbl| {
            let ctx_tbl = marshal_context(lua, ctx)?;
            let player_tbl = marshal_player(lua, player)?;
            Ok(MultiValue::from_vec(vec![
                Value::Table(ctx_tbl),
                Value::Table(player_tbl),
                Value::Integer(i64::from(slot)),
                Value::Table(world_tbl),
            ]))
        })
    }

    pub fn on_chat(
        &self,
        ctx: &ModRoomContext,
        sender: &str,
        text: &str,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        self.on_chat_with_world(ctx, sender, text, &EmptyModWorldView)
    }

    pub fn on_chat_with_world(
        &self,
        ctx: &ModRoomContext,
        sender: &str,
        text: &str,
        world: &dyn ModWorldView,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        let Some(hook) = &self.on_chat else {
            return Ok(Vec::new());
        };
        self.call_scoped_actions(hook, world, |lua, world_tbl| {
            let ctx_tbl = marshal_context(lua, ctx)?;
            let sender_str = lua.create_string(sender)?;
            let text_str = lua.create_string(text)?;
            Ok(MultiValue::from_vec(vec![
                Value::Table(ctx_tbl),
                Value::String(sender_str),
                Value::String(text_str),
                Value::Table(world_tbl),
            ]))
        })
    }

    pub fn on_tick(
        &self,
        ctx: &ModRoomContext,
        dt_secs: f64,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        self.on_tick_with_world(ctx, dt_secs, &EmptyModWorldView)
    }

    pub fn on_tick_with_world(
        &self,
        ctx: &ModRoomContext,
        dt_secs: f64,
        world: &dyn ModWorldView,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        let Some(hook) = &self.on_tick else {
            return Ok(Vec::new());
        };
        self.call_scoped_actions(hook, world, |lua, world_tbl| {
            let ctx_tbl = marshal_context(lua, ctx)?;
            Ok(MultiValue::from_vec(vec![
                Value::Table(ctx_tbl),
                Value::Number(dt_secs),
                Value::Table(world_tbl),
            ]))
        })
    }

    fn call_scoped_actions(
        &self,
        hook: &Function,
        world: &dyn ModWorldView,
        build_args: impl FnOnce(&Lua, Table) -> mlua::Result<MultiValue>,
    ) -> Result<Vec<ModAction>, ModScriptError> {
        let value = self.call_scoped_value(hook, world, build_args)?;
        parse_actions_or_nil(value)
    }

    fn call_scoped_value(
        &self,
        hook: &Function,
        world: &dyn ModWorldView,
        build_args: impl FnOnce(&Lua, Table) -> mlua::Result<MultiValue>,
    ) -> Result<Value, ModScriptError> {
        self.remaining_hooks
            .store(MOD_HOOK_BUDGET, Ordering::Relaxed);
        self.budget_exhausted.store(false, Ordering::Relaxed);
        let result = self.lua.scope(|scope| {
            let world_tbl = build_world_query_table(&self.lua, scope, world)?;
            let args = build_args(&self.lua, world_tbl)?;
            hook.call::<Value>(args)
        });
        match result {
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
    chunk_name: Arc<str>,
    source: Arc<str>,
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
            chunk_name: Arc::from(chunk_name),
            source: Arc::from(source),
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
        self.chunk_name = Arc::from(chunk_name);
        self.source = Arc::from(source);
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

    /// Instantiate a completely isolated Lua 5.4 VM for a single room so concurrent
    /// rooms never share mutable Lua globals or contend on the host mutex.
    pub fn instantiate_for_room(&self) -> Result<(u64, ModScript), ModScriptError> {
        let script = ModScript::from_source(&self.chunk_name, &self.source)?;
        Ok((self.generation, script))
    }

    pub fn status(&self, changed: bool) -> ModStatus {
        let info = self.active.info();
        ModStatus {
            name: info.name.clone(),
            version: info.version.clone(),
            author: info.author.clone(),
            abi_version: self.active.abi_version(),
            generation: self.generation,
            digest: hex_digest(&self.digest),
            changed,
        }
    }

    pub fn script(&self) -> &ModScript {
        &self.active
    }

    pub fn generation(&self) -> u64 {
        self.generation
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

fn sha256_seed(bytes: &[u8]) -> u64 {
    let digest = sha256_bytes(bytes);
    let seed = u64::from_le_bytes([
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
    ]);
    if seed == 0 {
        0x9e37_79b9_7f4a_7c15
    } else {
        seed
    }
}

fn hex_digest(digest: &[u8; 32]) -> String {
    let mut out = String::with_capacity(16);
    for byte in &digest[..8] {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn build_sandbox_env(lua: &Lua, rng_seed: u64) -> mlua::Result<Table> {
    let env = lua.create_table()?;
    let globals = lua.globals();

    // 1. Safe pure base functions (never load/dofile/loadfile/collectgarbage/raw*_metatable).
    for name in [
        "tostring", "tonumber", "pairs", "ipairs", "next", "type", "select", "pcall", "xpcall",
        "error", "assert", "rawget", "rawset", "rawequal", "rawlen",
    ] {
        if let Ok(val) = globals.raw_get::<Value>(name) {
            if !val.is_nil() {
                env.raw_set(name, val)?;
            }
        }
    }

    // 2. Safe pure `table` library.
    if let Ok(Value::Table(src_table)) = globals.raw_get::<Value>("table") {
        let tbl = lua.create_table()?;
        for name in [
            "insert", "remove", "sort", "concat", "unpack", "move", "pack",
        ] {
            if let Ok(val) = src_table.raw_get::<Value>(name) {
                if !val.is_nil() {
                    tbl.raw_set(name, val)?;
                }
            }
        }
        env.raw_set("table", tbl)?;
    }

    // 3. Safe pure `string` library (omits `dump`, bounds `rep` to MOD_MAX_STRING_REP_BYTES).
    if let Ok(Value::Table(src_string)) = globals.raw_get::<Value>("string") {
        let str_tbl = lua.create_table()?;
        for name in [
            "byte", "char", "find", "format", "gmatch", "gsub", "len", "lower", "match", "reverse",
            "sub", "upper",
        ] {
            if let Ok(val) = src_string.raw_get::<Value>(name) {
                if !val.is_nil() {
                    str_tbl.raw_set(name, val)?;
                }
            }
        }
        str_tbl.raw_set(
            "rep",
            lua.create_function(|_, (s, n, sep): (String, i64, Option<String>)| {
                if n <= 0 {
                    return Ok(String::new());
                }
                let sep = sep.unwrap_or_default();
                let count = n as usize;
                let total = s
                    .len()
                    .saturating_mul(count)
                    .saturating_add(sep.len().saturating_mul(count.saturating_sub(1)));
                if total > MOD_MAX_STRING_REP_BYTES {
                    return Err(mlua::Error::runtime("string.rep exceeded sandbox limit"));
                }
                let mut out = String::with_capacity(total);
                for i in 0..count {
                    if i > 0 && !sep.is_empty() {
                        out.push_str(&sep);
                    }
                    out.push_str(&s);
                }
                Ok(out)
            })?,
        )?;
        env.raw_set("string", str_tbl)?;
    }

    // 4. Safe `utf8` library.
    if let Ok(Value::Table(src_utf8)) = globals.raw_get::<Value>("utf8") {
        let utf8_tbl = lua.create_table()?;
        for name in ["char", "codepoint", "codes", "len", "offset", "charpattern"] {
            if let Ok(val) = src_utf8.raw_get::<Value>(name) {
                if !val.is_nil() {
                    utf8_tbl.raw_set(name, val)?;
                }
            }
        }
        env.raw_set("utf8", utf8_tbl)?;
    }

    // 5. Full safe `math` library + Workshop helpers (`clamp`, `lerp`, `round`, deterministic `random`).
    let math = lua.create_table()?;
    if let Ok(Value::Table(src_math)) = globals.raw_get::<Value>("math") {
        for name in [
            "abs",
            "acos",
            "asin",
            "atan",
            "ceil",
            "cos",
            "deg",
            "exp",
            "floor",
            "fmod",
            "huge",
            "log",
            "max",
            "maxinteger",
            "min",
            "mininteger",
            "modf",
            "pi",
            "rad",
            "sin",
            "sqrt",
            "tan",
            "tointeger",
            "type",
            "ult",
        ] {
            if let Ok(val) = src_math.raw_get::<Value>(name) {
                if !val.is_nil() {
                    math.raw_set(name, val)?;
                }
            }
        }
    }
    math.raw_set(
        "clamp",
        lua.create_function(|_, (x, lo, hi): (f64, f64, f64)| {
            Ok(if lo <= hi {
                x.clamp(lo, hi)
            } else {
                x.clamp(hi, lo)
            })
        })?,
    )?;
    math.raw_set(
        "lerp",
        lua.create_function(|_, (a, b, t): (f64, f64, f64)| Ok(a + (b - a) * t))?,
    )?;
    math.raw_set("round", lua.create_function(|_, x: f64| Ok(x.round()))?)?;
    let rng = Arc::new(AtomicU64::new(rng_seed));
    let rng_ref = Arc::clone(&rng);
    math.raw_set(
        "random",
        lua.create_function(move |_, (a, b): (Option<i64>, Option<i64>)| {
            // xorshift64* deterministic PRNG isolated per room VM.
            let mut x = rng_ref.load(Ordering::Relaxed);
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            rng_ref.store(x, Ordering::Relaxed);
            let next = x.wrapping_mul(0x2545_f491_4f6c_dd1d);
            match (a, b) {
                (None, None) => {
                    let frac = ((next >> 11) as f64) * (1.0 / ((1u64 << 53) as f64));
                    Ok(Value::Number(frac))
                }
                (Some(upper), None) => {
                    if upper < 1 {
                        return Err(mlua::Error::runtime("math.random upper bound must be >= 1"));
                    }
                    let span = upper as u64;
                    Ok(Value::Integer((next % span) as i64 + 1))
                }
                (Some(low), Some(high)) => {
                    if low > high {
                        return Err(mlua::Error::runtime("math.random interval is empty"));
                    }
                    let span = (high as i128 - low as i128 + 1) as u128;
                    let offset = (u128::from(next) % span) as i128;
                    Ok(Value::Integer((low as i128 + offset) as i64))
                }
                (None, Some(_)) => Err(mlua::Error::runtime("invalid math.random arguments")),
            }
        })?,
    )?;
    env.raw_set("math", math)?;

    // 6. Built-in pure `vec3` utility library for Workshop spatial math.
    let vec3 = lua.create_table()?;
    vec3.raw_set(
        "new",
        lua.create_function(|lua, (x, y, z): (f64, f64, f64)| make_vec3(lua, [x, y, z]))?,
    )?;
    vec3.raw_set(
        "add",
        lua.create_function(|lua, (a, b): (Table, Table)| {
            let a = read_vec3_table(&a)?;
            let b = read_vec3_table(&b)?;
            make_vec3(lua, [a[0] + b[0], a[1] + b[1], a[2] + b[2]])
        })?,
    )?;
    vec3.raw_set(
        "sub",
        lua.create_function(|lua, (a, b): (Table, Table)| {
            let a = read_vec3_table(&a)?;
            let b = read_vec3_table(&b)?;
            make_vec3(lua, [a[0] - b[0], a[1] - b[1], a[2] - b[2]])
        })?,
    )?;
    vec3.raw_set(
        "scale",
        lua.create_function(|lua, (a, s): (Table, f64)| {
            let a = read_vec3_table(&a)?;
            make_vec3(lua, [a[0] * s, a[1] * s, a[2] * s])
        })?,
    )?;
    vec3.raw_set(
        "dot",
        lua.create_function(|_, (a, b): (Table, Table)| {
            let a = read_vec3_table(&a)?;
            let b = read_vec3_table(&b)?;
            Ok(a[0] * b[0] + a[1] * b[1] + a[2] * b[2])
        })?,
    )?;
    vec3.raw_set(
        "cross",
        lua.create_function(|lua, (a, b): (Table, Table)| {
            let a = read_vec3_table(&a)?;
            let b = read_vec3_table(&b)?;
            make_vec3(
                lua,
                [
                    a[1] * b[2] - a[2] * b[1],
                    a[2] * b[0] - a[0] * b[2],
                    a[0] * b[1] - a[1] * b[0],
                ],
            )
        })?,
    )?;
    vec3.raw_set(
        "len",
        lua.create_function(|_, a: Table| {
            let a = read_vec3_table(&a)?;
            Ok((a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt())
        })?,
    )?;
    vec3.raw_set(
        "dist",
        lua.create_function(|_, (a, b): (Table, Table)| {
            let a = read_vec3_table(&a)?;
            let b = read_vec3_table(&b)?;
            let dx = a[0] - b[0];
            let dy = a[1] - b[1];
            let dz = a[2] - b[2];
            Ok((dx * dx + dy * dy + dz * dz).sqrt())
        })?,
    )?;
    vec3.raw_set(
        "normalize",
        lua.create_function(|lua, a: Table| {
            let a = read_vec3_table(&a)?;
            let len = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
            if len > 1e-9 {
                make_vec3(lua, [a[0] / len, a[1] / len, a[2] / len])
            } else {
                make_vec3(lua, [0.0, 0.0, 0.0])
            }
        })?,
    )?;
    vec3.raw_set(
        "lerp",
        lua.create_function(|lua, (a, b, t): (Table, Table, f64)| {
            let a = read_vec3_table(&a)?;
            let b = read_vec3_table(&b)?;
            make_vec3(
                lua,
                [
                    a[0] + (b[0] - a[0]) * t,
                    a[1] + (b[1] - a[1]) * t,
                    a[2] + (b[2] - a[2]) * t,
                ],
            )
        })?,
    )?;
    env.raw_set("vec3", vec3)?;

    // 7. Read-only Workshop constant helpers (`OW`).
    let ow = lua.create_table()?;
    ow.raw_set("ABI_VERSION", i64::from(MOD_ABI_VERSION))?;
    let status = lua.create_table()?;
    for (k, v) in [
        ("STUN", "stun"),
        ("SLEEP", "sleep"),
        ("FREEZE", "freeze"),
        ("ANTI_HEAL", "anti_heal"),
        ("HEAL_BOOST", "heal_boost"),
        ("HACK", "hack"),
        ("INVULNERABLE", "invulnerable"),
        ("CC_IMMUNE", "cc_immune"),
    ] {
        status.raw_set(k, v)?;
    }
    ow.raw_set("STATUS", status)?;
    let bot_ai = lua.create_table()?;
    for (k, v) in [
        ("DEFAULT", "default"),
        ("IDLE", "idle"),
        ("DANCE", "dance"),
        ("FOLLOW", "follow"),
        ("PATROL", "patrol"),
        ("COMBAT", "combat"),
    ] {
        bot_ai.raw_set(k, v)?;
    }
    ow.raw_set("BOT_AI", bot_ai)?;
    let comms = lua.create_table()?;
    for (k, v) in [
        ("HELLO", "hello"),
        ("NEED_HEALING", "need_healing"),
        ("THANKS", "thanks"),
    ] {
        comms.raw_set(k, v)?;
    }
    ow.raw_set("COMMS", comms)?;
    env.raw_set("OW", ow)?;

    Ok(env)
}

fn build_world_query_table<'scope, 'env>(
    lua: &Lua,
    scope: &'scope mlua::Scope<'scope, 'env>,
    world: &'env dyn ModWorldView,
) -> mlua::Result<Table> {
    let tbl = lua.create_table()?;

    tbl.raw_set(
        "raycast",
        scope.create_function(move |lua, (origin, dir, max_dist): (Table, Table, f64)| {
            let origin = read_vec3_table(&origin)?;
            let dir = read_vec3_table(&dir)?;
            let max_dist = if max_dist.is_finite() && max_dist > 0.0 {
                max_dist.clamp(0.01, 500.0)
            } else {
                50.0
            };
            let out = lua.create_table()?;
            if let Some(hit) = world.raycast(origin, dir, max_dist) {
                out.raw_set("hit", true)?;
                out.raw_set("distance", hit.distance)?;
                out.raw_set("point", make_vec3(lua, hit.point)?)?;
                out.raw_set("normal", make_vec3(lua, hit.normal)?)?;
            } else {
                out.raw_set("hit", false)?;
            }
            Ok(out)
        })?,
    )?;

    tbl.raw_set(
        "distance",
        scope.create_function(|_, (a, b): (Table, Table)| {
            let a = read_vec3_table(&a)?;
            let b = read_vec3_table(&b)?;
            let dx = a[0] - b[0];
            let dy = a[1] - b[1];
            let dz = a[2] - b[2];
            Ok((dx * dx + dy * dy + dz * dz).sqrt())
        })?,
    )?;

    tbl.raw_set(
        "players_in_radius",
        scope.create_function(
            move |lua, (center, radius, team): (Table, f64, Option<i64>)| {
                let center = read_vec3_table(&center)?;
                let radius = if radius.is_finite() && radius >= 0.0 {
                    radius.min(500.0)
                } else {
                    0.0
                };
                let team = team.and_then(|t| u8::try_from(t).ok());
                let list = lua.create_table()?;
                for (idx, snap) in world
                    .players_in_radius(center, radius, team)
                    .iter()
                    .enumerate()
                {
                    list.raw_set(idx + 1, marshal_player(lua, snap)?)?;
                }
                Ok(list)
            },
        )?,
    )?;

    tbl.raw_set(
        "get_entity",
        scope.create_function(move |lua, entity: i64| {
            let Ok(id) = u32::try_from(entity) else {
                return Ok(Value::Nil);
            };
            match world.get_entity(id) {
                Some(snap) => Ok(Value::Table(marshal_player(lua, &snap)?)),
                None => Ok(Value::Nil),
            }
        })?,
    )?;

    tbl.raw_set(
        "find_hero",
        scope.create_function(move |lua, query: String| match world.find_hero(&query) {
            Some(hero) => Ok(Value::Table(marshal_hero(lua, &hero)?)),
            None => Ok(Value::Nil),
        })?,
    )?;

    tbl.raw_set(
        "heroes",
        scope.create_function(move |lua, ()| {
            let list = lua.create_table()?;
            for (idx, hero) in world.heroes().iter().enumerate() {
                list.raw_set(idx + 1, marshal_hero(lua, hero)?)?;
            }
            Ok(list)
        })?,
    )?;

    Ok(tbl)
}

fn make_vec3(lua: &Lua, v: [f64; 3]) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.raw_set(1, v[0])?;
    t.raw_set(2, v[1])?;
    t.raw_set(3, v[2])?;
    Ok(t)
}

fn read_vec3_table(table: &Table) -> mlua::Result<[f64; 3]> {
    let mut out = [0.0; 3];
    for (idx, slot) in out.iter_mut().enumerate() {
        let entry = table.raw_get::<Value>(idx + 1)?;
        let num = match entry {
            Value::Number(v) => v,
            Value::Integer(v) => v as f64,
            _ => return Err(mlua::Error::runtime("vec3 element must be a number")),
        };
        if !num.is_finite() {
            return Err(mlua::Error::runtime("vec3 element must be finite"));
        }
        *slot = num;
    }
    Ok(out)
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
    if let Some(description) = optional_string(&tbl, "description")? {
        info.description = description;
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
    if let Some(scale) = optional_scale(&tbl, "time_scale")? {
        rules.time_scale = scale.clamp(0.1, 3.0);
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

fn marshal_hero(lua: &Lua, hero: &ModHeroInfo) -> mlua::Result<Table> {
    let tbl = lua.create_table()?;
    tbl.raw_set("guid", hero.guid as i64)?;
    tbl.raw_set("name", hero.name.as_str())?;
    tbl.raw_set("role", hero.role.as_str())?;
    Ok(tbl)
}

fn marshal_context(lua: &Lua, ctx: &ModRoomContext) -> mlua::Result<Table> {
    let tbl = lua.create_table()?;
    tbl.raw_set("abi_version", i64::from(MOD_ABI_VERSION))?;
    tbl.raw_set("tick", i64::from(ctx.tick))?;
    tbl.raw_set("elapsed_secs", ctx.elapsed_secs)?;
    tbl.raw_set("map_guid", ctx.map_guid as i64)?;
    tbl.raw_set("mode_guid", ctx.mode_guid as i64)?;
    tbl.raw_set("player_count", i64::try_from(ctx.player_count).unwrap_or(0))?;
    tbl.raw_set("time_scale", ctx.time_scale)?;
    if let Some(sender_entity) = ctx.sender_entity {
        tbl.raw_set("sender_entity", i64::from(sender_entity))?;
    }
    let rules = lua.create_table()?;
    rules.raw_set("damage_scale", ctx.rules.damage_scale)?;
    rules.raw_set("healing_scale", ctx.rules.healing_scale)?;
    rules.raw_set("gravity_scale", ctx.rules.gravity_scale)?;
    rules.raw_set("speed_scale", ctx.rules.speed_scale)?;
    rules.raw_set("ult_charge_scale", ctx.rules.ult_charge_scale)?;
    rules.raw_set("cooldown_scale", ctx.rules.cooldown_scale)?;
    rules.raw_set("time_scale", ctx.rules.time_scale)?;
    rules.raw_set("friendly_fire", ctx.rules.friendly_fire)?;
    tbl.raw_set("rules", rules)?;

    let players = lua.create_table()?;
    for (idx, p) in ctx.players.iter().enumerate() {
        players.raw_set(idx + 1, marshal_player(lua, p)?)?;
    }
    tbl.raw_set("players", players)?;

    let bots = lua.create_table()?;
    for (idx, b) in ctx.bots.iter().enumerate() {
        bots.raw_set(idx + 1, marshal_player(lua, b)?)?;
    }
    tbl.raw_set("bots", bots)?;
    Ok(tbl)
}

fn marshal_player(lua: &Lua, player: &ModPlayerSnapshot) -> mlua::Result<Table> {
    let tbl = lua.create_table()?;
    tbl.raw_set("entity", i64::from(player.entity))?;
    tbl.raw_set("name", player.name.as_str())?;
    tbl.raw_set("team", i64::from(player.team))?;
    tbl.raw_set("hero_guid", player.hero_guid as i64)?;
    tbl.raw_set("hero_name", player.hero_name.as_str())?;
    tbl.raw_set("health", player.health)?;
    tbl.raw_set("max_health", player.max_health)?;
    tbl.raw_set("ult_charge", player.ult_charge)?;
    tbl.raw_set("yaw", player.yaw)?;
    tbl.raw_set("pitch", player.pitch)?;
    tbl.raw_set("alive", player.alive)?;
    tbl.raw_set("crouching", player.crouching)?;
    tbl.raw_set("grounded", player.grounded)?;
    tbl.raw_set("emoting", player.emoting)?;
    tbl.raw_set("is_bot", player.is_bot)?;
    tbl.raw_set("position", make_vec3(lua, player.position)?)?;
    tbl.raw_set("velocity", make_vec3(lua, player.velocity)?)?;
    tbl.raw_set("facing", make_vec3(lua, player.facing)?)?;
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
        "kill" => Ok(ModAction::Kill {
            entity: parse_u32_field(table, "entity")?,
        }),
        "respawn" => Ok(ModAction::Respawn {
            entity: parse_u32_field(table, "entity")?,
        }),
        "add_health_pool" => Ok(ModAction::AddHealthPool {
            entity: parse_u32_field(table, "entity")?,
            kind: parse_required_string_field(table, "pool")
                .or_else(|_| parse_required_string_field(table, "pool_kind"))?,
            amount: parse_non_negative_f64_field(table, "amount")?,
        }),
        "apply_impulse" => Ok(ModAction::ApplyImpulse {
            entity: parse_u32_field(table, "entity")?,
            velocity: parse_vec3_field(table, "velocity")?,
        }),
        "set_velocity" => Ok(ModAction::SetVelocity {
            entity: parse_u32_field(table, "entity")?,
            velocity: parse_vec3_field(table, "velocity")?,
        }),
        "teleport" => Ok(ModAction::Teleport {
            entity: parse_u32_field(table, "entity")?,
            position: parse_vec3_field(table, "position")?,
        }),
        "set_facing" => Ok(ModAction::SetFacing {
            entity: parse_u32_field(table, "entity")?,
            yaw: parse_finite_f64_field(table, "yaw")?,
            pitch: parse_optional_finite_f64_field(table, "pitch")?.unwrap_or(0.0),
        }),
        "set_speed_scale" => Ok(ModAction::SetSpeedScale {
            entity: parse_u32_field(table, "entity")?,
            scale: parse_bounded_scale_field(table, "scale")?,
        }),
        "set_gravity_scale" => Ok(ModAction::SetGravityScale {
            entity: parse_u32_field(table, "entity")?,
            scale: parse_bounded_scale_field(table, "scale")?,
        }),
        "set_damage_scale" => Ok(ModAction::SetDamageScale {
            entity: parse_u32_field(table, "entity")?,
            scale: parse_bounded_scale_field(table, "scale")?,
        }),
        "set_damage_taken_scale" => Ok(ModAction::SetDamageTakenScale {
            entity: parse_u32_field(table, "entity")?,
            scale: parse_bounded_scale_field(table, "scale")?,
        }),
        "set_healing_scale" => Ok(ModAction::SetHealingScale {
            entity: parse_u32_field(table, "entity")?,
            scale: parse_bounded_scale_field(table, "scale")?,
        }),
        "play_emote" => {
            let slot = parse_optional_u8_field(table, "slot")?.unwrap_or(0).min(3);
            Ok(ModAction::PlayEmote {
                entity: parse_u32_field(table, "entity")?,
                slot,
            })
        }
        "play_voiceline" => {
            let slot = parse_optional_u8_field(table, "slot")?.unwrap_or(0).min(3);
            Ok(ModAction::PlayVoiceline {
                entity: parse_u32_field(table, "entity")?,
                slot,
            })
        }
        "place_spray" => {
            let slot = parse_optional_u8_field(table, "slot")?.unwrap_or(0).min(3);
            Ok(ModAction::PlaceSpray {
                entity: parse_u32_field(table, "entity")?,
                slot,
            })
        }
        "communication" => Ok(ModAction::Communication {
            entity: parse_u32_field(table, "entity")?,
            kind: parse_required_string_field(table, "message")
                .or_else(|_| parse_required_string_field(table, "comm"))?,
        }),
        "play_voice_cue" => Ok(ModAction::PlayVoiceCue {
            entity: parse_u32_field(table, "entity")?,
            stimulus: parse_u64_field(table, "stimulus")?,
        }),
        "set_hero" => {
            let hero_val = table.raw_get::<Value>("hero").map_err(map_mlua_error)?;
            let hero = match hero_val {
                Value::String(s) => s
                    .to_str()
                    .map_err(|err| ModScriptError::InvalidAction(err.to_string()))?
                    .trim()
                    .to_owned(),
                Value::Integer(guid) => format!("0x{:016X}", guid as u64),
                _ => {
                    return Err(ModScriptError::InvalidAction(String::from(
                        "set_hero `hero` must be a hero name string or GUID integer",
                    )));
                }
            };
            Ok(ModAction::SetHero {
                entity: parse_u32_field(table, "entity")?,
                hero,
            })
        }
        "set_status" => Ok(ModAction::SetStatus {
            entity: parse_u32_field(table, "entity")?,
            status: parse_required_string_field(table, "status")?,
            duration: parse_non_negative_f64_field(table, "duration")?.clamp(0.05, 120.0),
        }),
        "clear_status" => Ok(ModAction::ClearStatus {
            entity: parse_u32_field(table, "entity")?,
            status: optional_string(table, "status")?.unwrap_or_else(|| String::from("all")),
        }),
        "set_ult_charge" => Ok(ModAction::SetUltCharge {
            entity: parse_u32_field(table, "entity")?,
            percent: parse_non_negative_f64_field(table, "percent")?.clamp(0.0, 100.0),
        }),
        "add_ult_charge" => Ok(ModAction::AddUltCharge {
            entity: parse_u32_field(table, "entity")?,
            amount: parse_finite_f64_field(table, "amount")?.clamp(-10_000.0, 10_000.0),
        }),
        "reset_cooldowns" => Ok(ModAction::ResetCooldowns {
            entity: parse_u32_field(table, "entity")?,
        }),
        "spawn_bot" => {
            let hero_val = table.raw_get::<Value>("hero").map_err(map_mlua_error)?;
            let hero = match hero_val {
                Value::String(s) => s
                    .to_str()
                    .map_err(|err| ModScriptError::InvalidAction(err.to_string()))?
                    .trim()
                    .to_owned(),
                Value::Integer(guid) => format!("0x{:016X}", guid as u64),
                _ => {
                    return Err(ModScriptError::InvalidAction(String::from(
                        "spawn_bot `hero` must be a hero name string or GUID integer",
                    )));
                }
            };
            let team = parse_optional_u8_field(table, "team")?.unwrap_or(1);
            let position = parse_vec3_field(table, "position")?;
            let name = optional_string(table, "name")?;
            Ok(ModAction::SpawnBot {
                hero,
                team,
                position,
                name,
            })
        }
        "despawn_bot" => Ok(ModAction::DespawnBot {
            entity: parse_u32_field(table, "entity")?,
        }),
        "set_bot_ai" => Ok(ModAction::SetBotAi {
            entity: parse_u32_field(table, "entity")?,
            mode: parse_required_string_field(table, "mode")?,
        }),
        "spawn_projectile" => Ok(ModAction::SpawnProjectile {
            source: parse_u32_field(table, "source")?,
            origin: parse_vec3_field(table, "origin")?,
            direction: parse_vec3_field(table, "direction")?,
            speed: parse_non_negative_f64_field(table, "speed")?.clamp(1.0, 200.0),
            damage: parse_non_negative_f64_field(table, "damage")?.clamp(0.0, 2000.0),
            radius: parse_optional_non_negative_f64_field(table, "radius")?
                .unwrap_or(0.5)
                .clamp(0.05, 20.0),
            gravity: parse_optional_finite_f64_field(table, "gravity")?
                .unwrap_or(0.0)
                .clamp(-40.0, 40.0),
        }),
        "set_time_scale" => {
            let scale = parse_non_negative_f64_field(table, "scale")?.clamp(0.1, 3.0);
            Ok(ModAction::SetTimeScale { scale })
        }
        "broadcast_chat" => Ok(ModAction::BroadcastChat {
            text: parse_clipped_chat(table, "text")?,
        }),
        "send_chat" => Ok(ModAction::SendChat {
            entity: parse_u32_field(table, "entity")?,
            text: parse_clipped_chat(table, "text")?,
        }),
        "announce" => Ok(ModAction::Announce {
            text: parse_clipped_chat(table, "text")?,
        }),
        "reset_interactive_prop" => Ok(ModAction::ResetInteractiveProp {
            prop_id: parse_u32_field(table, "prop_id")?,
        }),
        "apply_prop_impulse" => Ok(ModAction::ApplyPropImpulse {
            prop_id: parse_u32_field(table, "prop_id")?,
            velocity: parse_vec3_field(table, "velocity")?,
        }),
        "set_room_rule" => {
            let key = parse_required_string_field(table, "key")?;
            let value = parse_bounded_scale_field(table, "value")?;
            Ok(ModAction::SetRoomRule { key, value })
        }
        other => Err(ModScriptError::InvalidAction(format!(
            "unknown mod action kind `{other}`"
        ))),
    }
}

fn parse_clipped_chat(table: &Table, key: &'static str) -> Result<String, ModScriptError> {
    let text_val = table.raw_get::<Value>(key).map_err(map_mlua_error)?;
    let Value::String(text_str) = text_val else {
        return Err(ModScriptError::InvalidAction(format!(
            "action `{key}` must be a string"
        )));
    };
    let text = text_str
        .to_str()
        .map_err(|err| ModScriptError::InvalidAction(err.to_string()))?;
    Ok(text.chars().take(MOD_MAX_CHAT_LEN).collect())
}

fn parse_required_string_field(table: &Table, key: &'static str) -> Result<String, ModScriptError> {
    let val = table.raw_get::<Value>(key).map_err(map_mlua_error)?;
    let Value::String(s) = val else {
        return Err(ModScriptError::InvalidAction(format!(
            "action field `{key}` must be a string"
        )));
    };
    let text = s
        .to_str()
        .map_err(|err| ModScriptError::InvalidAction(err.to_string()))?;
    Ok(text.trim().to_owned())
}

fn parse_u32_field(table: &Table, key: &'static str) -> Result<u32, ModScriptError> {
    let value = table.raw_get::<Value>(key).map_err(map_mlua_error)?;
    let raw = match value {
        Value::Integer(raw) => raw,
        Value::Number(v)
            if v.is_finite() && v.fract() == 0.0 && v >= 0.0 && v <= f64::from(u32::MAX) =>
        {
            v as i64
        }
        _ => {
            return Err(ModScriptError::InvalidAction(format!(
                "action field `{key}` must be an integer"
            )));
        }
    };
    u32::try_from(raw).map_err(|_| {
        ModScriptError::InvalidAction(format!("action field `{key}` ({raw}) is out of u32 range"))
    })
}

fn parse_optional_u8_field(table: &Table, key: &'static str) -> Result<Option<u8>, ModScriptError> {
    let value = table.raw_get::<Value>(key).map_err(map_mlua_error)?;
    if matches!(value, Value::Nil) {
        return Ok(None);
    }
    let raw = match value {
        Value::Integer(raw) => raw,
        Value::Number(v)
            if v.is_finite() && v.fract() == 0.0 && v >= 0.0 && v <= f64::from(u8::MAX) =>
        {
            v as i64
        }
        _ => {
            return Err(ModScriptError::InvalidAction(format!(
                "action field `{key}` must be an integer"
            )));
        }
    };
    u8::try_from(raw).map(Some).map_err(|_| {
        ModScriptError::InvalidAction(format!("action field `{key}` ({raw}) is out of u8 range"))
    })
}

fn parse_u64_field(table: &Table, key: &'static str) -> Result<u64, ModScriptError> {
    let value = table.raw_get::<Value>(key).map_err(map_mlua_error)?;
    let raw = match value {
        Value::Integer(raw) => raw,
        Value::Number(v)
            if v.is_finite() && v.fract() == 0.0 && v >= 0.0 && v <= (i64::MAX as f64) =>
        {
            v as i64
        }
        _ => {
            return Err(ModScriptError::InvalidAction(format!(
                "action field `{key}` must be an integer"
            )));
        }
    };
    u64::try_from(raw).map_err(|_| {
        ModScriptError::InvalidAction(format!("action field `{key}` ({raw}) must be non-negative"))
    })
}

fn parse_finite_f64_value(value: Value, key: &str) -> Result<f64, ModScriptError> {
    let num = match value {
        Value::Number(v) => v,
        Value::Integer(v) => v as f64,
        _ => {
            return Err(ModScriptError::InvalidAction(format!(
                "field `{key}` must be a number"
            )));
        }
    };
    if !num.is_finite() {
        return Err(ModScriptError::InvalidAction(format!(
            "field `{key}` ({num}) must be finite"
        )));
    }
    Ok(num)
}

fn parse_finite_f64_field(table: &Table, key: &'static str) -> Result<f64, ModScriptError> {
    let value = table.raw_get::<Value>(key).map_err(map_mlua_error)?;
    parse_finite_f64_value(value, key)
}

fn parse_optional_finite_f64_field(
    table: &Table,
    key: &'static str,
) -> Result<Option<f64>, ModScriptError> {
    let value = table.raw_get::<Value>(key).map_err(map_mlua_error)?;
    if matches!(value, Value::Nil) {
        return Ok(None);
    }
    parse_finite_f64_value(value, key).map(Some)
}

fn parse_non_negative_f64_value(value: Value, key: &str) -> Result<f64, ModScriptError> {
    let num = parse_finite_f64_value(value, key)?;
    if num < 0.0 {
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

fn parse_optional_non_negative_f64_field(
    table: &Table,
    key: &'static str,
) -> Result<Option<f64>, ModScriptError> {
    let value = table.raw_get::<Value>(key).map_err(map_mlua_error)?;
    if matches!(value, Value::Nil) {
        return Ok(None);
    }
    parse_non_negative_f64_value(value, key).map(Some)
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
            sender_entity: Some(1),
            players: vec![sample_player()],
            bots: vec![sample_bot()],
            ..ModRoomContext::default()
        }
    }

    fn sample_player() -> ModPlayerSnapshot {
        ModPlayerSnapshot {
            entity: 1,
            name: String::from("Researcher"),
            team: 0,
            hero_guid: 0x02E0_0000_0000_006E,
            hero_name: String::from("Soldier: 76"),
            health: 200.0,
            max_health: 200.0,
            position: [1.0, 2.0, 3.0],
            facing: [0.0, 0.0, 1.0],
            alive: true,
            ..ModPlayerSnapshot::default()
        }
    }

    fn sample_bot() -> ModPlayerSnapshot {
        ModPlayerSnapshot {
            entity: 0xc000_1000,
            name: String::from("Training Bot"),
            team: 1,
            hero_guid: 0x02E0_0000_0000_016B,
            hero_name: String::from("Training Bot"),
            health: 200.0,
            max_health: 200.0,
            position: [5.0, 2.0, 3.0],
            facing: [0.0, 0.0, -1.0],
            alive: true,
            is_bot: true,
            ..ModPlayerSnapshot::default()
        }
    }

    struct StubWorldView;
    impl ModWorldView for StubWorldView {
        fn raycast(
            &self,
            origin: [f64; 3],
            direction: [f64; 3],
            max_distance: f64,
        ) -> Option<ModRaycastHit> {
            let d = max_distance.min(10.0);
            Some(ModRaycastHit {
                distance: d,
                point: [
                    origin[0] + direction[0] * d,
                    origin[1] + direction[1] * d,
                    origin[2] + direction[2] * d,
                ],
                normal: [0.0, 1.0, 0.0],
            })
        }

        fn players_in_radius(
            &self,
            _center: [f64; 3],
            _radius: f64,
            _team: Option<u8>,
        ) -> Vec<ModPlayerSnapshot> {
            vec![sample_player(), sample_bot()]
        }

        fn get_entity(&self, entity: u32) -> Option<ModPlayerSnapshot> {
            if entity == 1 {
                Some(sample_player())
            } else {
                None
            }
        }

        fn find_hero(&self, query: &str) -> Option<ModHeroInfo> {
            if query.eq_ignore_ascii_case("tracer") {
                Some(ModHeroInfo {
                    guid: 0x02E0_0000_0000_0003,
                    name: String::from("Tracer"),
                    role: String::from("Damage"),
                })
            } else {
                None
            }
        }
    }

    #[test]
    fn mod_sandbox_blocks_os_io_and_enforces_abi_and_budgets() {
        let probe = ModScript::from_source(
            "=sandbox.lua",
            r#"
            if os ~= nil or io ~= nil or require ~= nil or debug ~= nil or load ~= nil or string.dump ~= nil then
                error("sandbox leak")
            end
            assert(type(string.lower("HELLO")) == "string")
            assert(table.concat({"a", "b"}, "-") == "a-b")
            assert(math.round(2.6) == 3)
            local v = vec3.normalize(vec3.add({3, 0, 0}, {0, 4, 0}))
            assert(math.abs(vec3.len(v) - 1.0) < 1e-6)
            return { abi = 2, info = { name = "Sandbox OK" } }
            "#,
        )
        .unwrap();
        assert_eq!(probe.info().name, "Sandbox OK");
        assert_eq!(probe.abi_version(), 2);

        let bad_abi = ModScript::from_source("=bad_abi.lua", "return { abi = 99 }");
        assert!(matches!(
            bad_abi,
            Err(ModScriptError::UnsupportedAbi {
                found: 99,
                expected: MOD_ABI_VERSION
            })
        ));

        let oom = ModScript::from_source(
            "=oom.lua",
            r#"
            local s = "0123456789abcdef"
            for i = 1, 18 do s = s .. s end
            return { abi = 1 }
            "#,
        );
        assert!(matches!(oom, Err(ModScriptError::MemoryLimitExceeded)));

        let rep_abuse = ModScript::from_source(
            "=rep.lua",
            r#"local s = string.rep("x", 100000); return { abi = 2 }"#,
        );
        assert!(matches!(rep_abuse, Err(ModScriptError::Runtime(_))));

        let inf_loop = ModScript::from_source("=loop.lua", "while true do end");
        assert!(matches!(
            inf_loop,
            Err(ModScriptError::InstructionBudgetExceeded {
                phase: ModExecutionPhase::Init
            })
        ));
    }

    #[test]
    fn mod_host_hot_reload_and_per_room_vm_isolation() {
        let mut host = ModHost::from_source(
            "=v1.lua",
            r#"
            local counter = 0
            return {
                abi = 2,
                info = { name = "V1", version = "1.0" },
                rules = { gravity_scale = 0.5 },
                on_chat = function(ctx, sender, text)
                    counter = counter + 1
                    return { { kind = "broadcast_chat", text = tostring(counter) } }
                end
            }
            "#,
        )
        .unwrap();
        assert_eq!(host.script().rules().gravity_scale, 0.5);

        // Two rooms instantiated from the same ModHost MUST have completely isolated Lua state.
        let (gen_a, room_a) = host.instantiate_for_room().unwrap();
        let (gen_b, room_b) = host.instantiate_for_room().unwrap();
        assert_eq!(gen_a, 1);
        assert_eq!(gen_b, 1);
        let ctx = sample_ctx();
        assert_eq!(
            room_a.on_chat(&ctx, "A", "ping").unwrap(),
            vec![ModAction::BroadcastChat { text: "1".into() }]
        );
        assert_eq!(
            room_a.on_chat(&ctx, "A", "ping").unwrap(),
            vec![ModAction::BroadcastChat { text: "2".into() }]
        );
        // Room B still starts at 1!
        assert_eq!(
            room_b.on_chat(&ctx, "B", "ping").unwrap(),
            vec![ModAction::BroadcastChat { text: "1".into() }]
        );

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
                    abi = 2,
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
    fn abi_v2_world_queries_dance_and_workshop_actions() {
        let script = ModScript::from_source(
            "=workshop.lua",
            r#"
            return {
                abi = 2,
                info = { name = "Workshop Probe", version = "2.0" },
                on_chat = function(ctx, sender, text, world)
                    if text == "!dance" then
                        return {
                            { kind = "play_emote", entity = ctx.sender_entity, slot = 2 },
                            { kind = "set_bot_ai", entity = ctx.bots[1].entity, mode = OW.BOT_AI.DANCE },
                        }
                    elseif text == "!tp" then
                        local p = world.get_entity(ctx.sender_entity)
                        local hit = world.raycast(p.position, p.facing, 30)
                        if hit.hit then
                            return { { kind = "teleport", entity = p.entity, position = hit.point } }
                        end
                    elseif text == "!tracer" then
                        local h = world.find_hero("tracer")
                        return {
                            { kind = "set_hero", entity = ctx.sender_entity, hero = h.name },
                            { kind = "set_ult_charge", entity = ctx.sender_entity, percent = 100 },
                            { kind = "reset_cooldowns", entity = ctx.sender_entity },
                            { kind = "set_status", entity = ctx.sender_entity, status = OW.STATUS.CC_IMMUNE, duration = 5.0 },
                            { kind = "set_time_scale", scale = 0.5 },
                        }
                    end
                    return {}
                end
            }
            "#,
        )
        .unwrap();
        let ctx = sample_ctx();
        let actions = script
            .on_chat_with_world(&ctx, "Researcher", "!dance", &StubWorldView)
            .unwrap();
        assert_eq!(
            actions,
            vec![
                ModAction::PlayEmote { entity: 1, slot: 2 },
                ModAction::SetBotAi {
                    entity: 0xc000_1000,
                    mode: String::from("dance"),
                },
            ]
        );
        let tp = script
            .on_chat_with_world(&ctx, "Researcher", "!tp", &StubWorldView)
            .unwrap();
        assert_eq!(
            tp,
            vec![ModAction::Teleport {
                entity: 1,
                position: [1.0, 2.0, 13.0],
            }]
        );
        let kit = script
            .on_chat_with_world(&ctx, "Researcher", "!tracer", &StubWorldView)
            .unwrap();
        assert_eq!(kit.len(), 5);
        assert!(matches!(kit[0], ModAction::SetHero { entity: 1, ref hero } if hero == "Tracer"));
        assert!(
            matches!(kit[1], ModAction::SetUltCharge { entity: 1, percent } if percent == 100.0)
        );
        assert!(matches!(kit[2], ModAction::ResetCooldowns { entity: 1 }));
        assert!(matches!(kit[4], ModAction::SetTimeScale { scale } if (scale - 0.5).abs() < 1e-6));
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

        let dance = ModScript::from_source(
            "=dance_party_workshop.lua",
            include_str!("../scripts/mods/dance_party_workshop.lua"),
        )
        .unwrap();
        assert_eq!(dance.abi_version(), 2);
        let dance_all = dance
            .on_chat_with_world(&ctx, "Researcher", "!danceall", &StubWorldView)
            .unwrap();
        assert!(
            dance_all
                .iter()
                .any(|a| matches!(a, ModAction::PlayEmote { entity: 1, .. }))
        );
        assert!(
            dance_all
                .iter()
                .any(|a| matches!(a, ModAction::SetBotAi { mode, .. } if mode == "dance"))
        );
    }
}
