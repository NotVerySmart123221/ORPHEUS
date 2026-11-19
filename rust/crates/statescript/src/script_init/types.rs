//! StateScript runtime snapshots, state payloads, diagnostics, and fault types.

use std::{
    collections::{BTreeMap, HashSet},
    error::Error,
    fmt,
};

use indexmap::IndexMap;

use crate::{
    script_expr::{EvalError, StackPriority, Variable},
    script_runtime::{BOSS_PAUSE, QueueFull},
    script_voice::IntroCue,
    script_wire::{Value, Variables},
};

use super::{
    ABILITY_RUNNING,
    machine::VarKey,
    world::{
        AiBehaviorRequest, AiControlOverride, AiFacing, AiWeaponOverride, Site, TargetTagEffect,
    },
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StateKind {
    RollMovementParameter,
    ExplosionContact,
    TrackedTargets,
    DeadeyeEffects,
    WitchStaffEffect,
    WitchStaffHealing,
    SummonerBeamRay,
    SummonerBeamDamage,
    ModifyHealth,
    ShieldRecharge,
    ForCode,
    AiBehavior {
        record: Option<AiBehaviorRequest>,
    },
    AiControlSetting,
    AiControlOverride {
        record: Option<AiControlOverride>,
    },
    AiFacing(Option<AiFacing>),
    Plain,
    HealthPool {
        pool: Option<u32>,
    },
    HealthCapacity,
    AiWeaponOverride {
        record: Option<AiWeaponOverride>,
    },
    TargetTag {
        effect: Option<TargetTagEffect>,
    },
    DisplayName {
        binding: Option<(u32, u64)>,
    },
    BooleanSwitch(bool),
    DelayedSwitch {
        current: bool,
        undecided: bool,
    },
    Switch {
        case: Option<usize>,
    },
    ActiveQuery,
    GamePadVibration,
    SubScript {
        child: Option<u16>,
    },
    PhaseScriptList,
    PhaseScript,
    PhaseTransition,
    MessageIdle,
    BossPauseReceiver {
        receiving: bool,
    },
    Registered {
        next: Option<(u16, u16)>,
    },
    Wait,
    WaitFrames {
        target: u32,
    },
    CinematicTimeScale {
        boundary_frame: u32,
    },
    AiCandidate {
        selected: bool,
    },
    LogicalButton {
        down: bool,
    },
    ButtonBlock(Option<u8>),
    Stack {
        top: bool,
        under: bool,
    },
    Ability {
        flags: u8,
    },
    MovementMod,
    Defer,
    VoiceStimulus,
    BossIntroVoice(IntroCue),
    GateVoice(u64),
    VictoryVoice,
    SameTeamContact {
        registered: bool,
    },
    OutgoingDamageFilter {
        registered: bool,
    },
    MovementSample(u16),
    Anim,
    Effect,
    ClientPulse {
        count: u8,
        fresh_at: Option<u64>,
    },
    EmptyTargets,
    SendMessage,
    PersistentSend(u64),
    CosmeticEntity,
    ChaseVar,
    Watch,
    SpawnSearch,
    RecentInput,
    LivingBodyArray,
    Shockwave {
        managed: bool,
    },
    PlacementSelector {
        confirmed: bool,
        sampled: bool,
    },
    MoveTo {
        movement: Option<([f64; 3], f64)>,
    },
    WeaponVolley {
        volleys: u8,
        offset_ms: u64,
        ammo0: i64,
        ammo_var: Option<Variable>,
    },
}

#[derive(Clone, Copy, Debug)]
pub(super) struct BufferedPress {
    pub(super) time_ms: u64,
    pub(super) activation: u16,
}

#[derive(Clone, Debug)]
pub struct State {
    pub tracked_targets: Vec<u32>,
    pub(super) deadeye_children: BTreeMap<u32, u16>,
    pub active: bool,
    pub activation: u16,
    pub start_ms: u64,
    pub kind: StateKind,
    pub(crate) voice: Option<Box<crate::script_voice::State>>,
    pub chase: Option<ChasePayload>,
    pub ability_cooldown: Option<AbilityCooldown>,
    pub(super) watch_seen: Option<Value>,
    pub(super) spawn_search: Option<SpawnSearch>,
    pub(super) recent_inputs: Vec<(usize, u64)>,
    pub(super) ending: bool,
    pub(super) pending_change: bool,
    pub(super) pending_timer: bool,
    pub(super) delayed_timers: [bool; 2],
    pub(super) buffered_press: Option<BufferedPress>,
    pub(super) linked_variable: Option<VarKey>,
    pub(super) ability_stack: Option<VarKey>,
    pub(super) untrusted_native: bool,
}

impl State {
    pub fn ability_running(&self) -> bool {
        self.active
            && matches!(self.kind, StateKind::Ability { flags } if flags & ABILITY_RUNNING != 0)
    }

    /// Whether a watched evaluation for this state has ever read a native
    /// outside the reference's client-agreed set.
    pub fn reads_untrusted_native(&self) -> bool {
        self.untrusted_native
    }

    pub fn move_request(&self) -> Option<([f64; 3], f64)> {
        match self.kind {
            StateKind::MoveTo { movement } if self.active => movement,
            _ => None,
        }
    }

    pub(super) fn dormant(kind: StateKind) -> Self {
        Self {
            tracked_targets: Vec::new(),
            deadeye_children: BTreeMap::new(),
            active: false,
            activation: 0,
            start_ms: 0,
            kind,
            voice: None,
            chase: None,
            ability_cooldown: None,
            watch_seen: None,
            spawn_search: None,
            recent_inputs: Vec::new(),
            ending: false,
            pending_change: false,
            pending_timer: false,
            delayed_timers: [false; 2],
            buffered_press: None,
            linked_variable: None,
            ability_stack: None,
            untrusted_native: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SpawnSearchKey {
    pub(super) instance: u16,
    pub(super) raw: u16,
    pub(super) activation: u16,
    pub(super) start_ms: u64,
}

impl SpawnSearchKey {
    pub const fn instance(self) -> u16 {
        self.instance
    }

    pub const fn raw(self) -> u16 {
        self.raw
    }

    pub const fn activation(self) -> u16 {
        self.activation
    }

    pub const fn start_ms(self) -> u64 {
        self.start_ms
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnSearch {
    pub position: [f64; 3],
    pub direction: [f64; 3],
    pub radius: f64,
    pub half_angle: f64,
    pub minimum: usize,
    pub maximum: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AbilityCooldown {
    pub current: f64,
    pub rate: f64,
    pub last_ms: u64,
}

impl AbilityCooldown {
    pub(super) fn remaining(self, now_ms: u64) -> f64 {
        let elapsed = (now_ms.saturating_sub(self.last_ms) as f64 * -f64::from(0.001_f32)) as f32;
        let consumed = (f64::from(elapsed) * self.rate) as f32;
        f64::from(((f64::from(consumed) + self.current) as f32).max(0.0))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChasePayload {
    pub current: Value,
    pub reached: bool,
    pub remaining_ms: Option<i64>,
    pub last_ms: u64,
}

#[derive(Clone, Debug)]
pub struct Instance {
    pub graph: u32,
    pub(super) start_ms: u64,
    pub(super) retire_after_frame: Option<u32>,
    /// Parent instance and raw state index, as carried on the wire.
    pub parent: Option<(u16, u16)>,
    pub variables: Variables,
    pub states: IndexMap<u16, State>,
    pub(super) join_inputs: BTreeMap<u16, HashSet<(u16, String)>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Issue {
    MissingGraph(u32),
    MissingNode(u16),
    MissingState(u16),
    UnsupportedAction(String),
    UnsupportedState(String),
    UnsupportedField(&'static str),
    EntityCreation(String),
    InvalidSubScript,
    Expression(EvalError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub site: Site,
    pub issue: Issue,
    /// An existing placement or registration guard rejected this node.
    pub registration_rejected: bool,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub entity: u32,
    pub now_ms: u64,
    pub variables: Variables,
    pub instances: IndexMap<u16, Instance>,
    /// Pending state timers and finishes in their per-instance execution order.
    pub pending_timers: IndexMap<u16, Vec<PendingTimer>>,
    /// Stack links by (instance or zero for the entity, variable).
    pub bindings: IndexMap<(u16, u32), Vec<StackBinding>>,
    /// Effective linked values, separate from their stored bases.
    pub resolved_bindings: IndexMap<(u16, u32), Value>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Snapshot {
    pub fn boss_pause_active(&self) -> bool {
        self.instances.values().any(|instance| {
            let senders: &[u16] = match instance.graph {
                0x0ce0 => &[8],
                0x0c8c => &[25, 31],
                _ => return false,
            };
            senders.iter().any(|raw| {
                instance.states.get(raw).is_some_and(|state| {
                    state.active && state.kind == StateKind::PersistentSend(BOSS_PAUSE)
                })
            })
        })
    }

    /// The reached initialization path was supported. This does not check
    /// dormant paths, later input, timers, messages, or gameplay.
    pub fn initialization_complete(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum InitFault {
    InvalidBudget,
    InvalidRoot(u16),
    InstanceIdExhausted,
    MissingPlayerGraph(u32),
    RootBinding {
        id: u16,
        graph: u32,
        error: EvalError,
    },
    WorkBudget {
        limit: usize,
        operation: &'static str,
    },
    EventBudget {
        instance: u16,
    },
    PendingEvents {
        limit: usize,
    },
    PendingShots,
    FrameOrder {
        frame: u32,
        now_ms: u64,
    },
    Halted,
    PlugDepth,
}

impl fmt::Display for InitFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBudget => {
                f.write_str("statescript initialization budget must be positive")
            }
            Self::InvalidRoot(id) => write!(f, "invalid or duplicate statescript root {id}"),
            Self::InstanceIdExhausted => {
                f.write_str("statescript initialization instance IDs exhausted")
            }
            Self::MissingPlayerGraph(graph) => {
                write!(f, "player statescript graph {graph:04X} is missing")
            }
            Self::RootBinding { id, graph, error } => {
                write!(
                    f,
                    "statescript root {id} graph {graph:04X} binding failed: {error}"
                )
            }
            Self::WorkBudget { limit, operation } => {
                write!(
                    f,
                    "statescript initialization work budget {limit} exhausted during {operation}"
                )
            }
            Self::EventBudget { instance } => {
                write!(
                    f,
                    "statescript initialization event budget exhausted in instance {instance}"
                )
            }
            Self::PendingEvents { limit } => {
                write!(f, "statescript pending event limit {limit} exceeded")
            }
            Self::PendingShots => f.write_str("statescript shot queue limit exceeded"),
            Self::FrameOrder { frame, now_ms } => {
                write!(f, "statescript frame {frame} starts before {now_ms} ms")
            }
            Self::Halted => f.write_str("statescript component halted after a failed operation"),
            Self::PlugDepth => f.write_str("statescript initialization plug depth exceeded"),
        }
    }
}

impl Error for InitFault {}

impl From<QueueFull> for InitFault {
    fn from(error: QueueFull) -> Self {
        Self::PendingEvents { limit: error.limit }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingTimer {
    pub due_ms: u64,
    pub raw_state: u16,
    pub finish: bool,
    pub parameter: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StackBinding {
    pub instance: u16,
    pub raw_state: u16,
    pub priority: StackPriority,
    pub slot: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShotEvent {
    pub entity: u32,
    pub instance: u16,
    pub graph: u32,
    pub raw_state: u16,
    pub frame: u32,
    pub time_ms: u64,
    pub activation: u16,
    pub activation_ms: u64,
    pub ordinal: u32,
}
