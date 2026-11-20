//! Per-peer world delivery, including the owning player's body lifetime.

use std::{
    borrow::Borrow,
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    io,
    ops::Range,
    sync::Arc,
    time::Duration,
};

use crate::{
    commands::{Command, CommandQueue},
    game_body::{BodyCreate, BodyView, body_create, owner_pose},
    game_bots::BOT_RECORD_DELAY,
    game_clock::{MAX_LEASE_FRAME, SEND_EVERY, TICK_PERIOD},
    game_combat::{
        AdmittedFlight, BIOTIC_CANISTER, BioticField, BotReplacement, CombatError, FlightOutcome,
        HealingPulse, PlayerBodyState, PosedBioticField, PosedHitscan, PosedProjectile, PosedShot,
        RoomSimulation,
    },
    game_controller::ControllerFrame,
    game_egress::{DeliveryReport, FrameRecords, GameEgress, RecordSource, SendError, SentFrame},
    game_heroes::{CASSIDY, Hero, SOLDIER},
    game_messages::Message,
    game_parameters::{ClientGameMessage, HeroSelectRequest},
    game_projectiles::{OwnerProjectiles, ProjectileDelivery, ProjectileLeases, ProjectileToken},
    match_scene::{PlayerAppearance, Possession, team_bit},
    mover::Mover,
    mover_correction::CorrectionHistory,
    mover_kinematics::Poses,
    mover_modifiers::{Modifier, ModifierKey},
    projectile_flight::ProjectileFlight,
    scene_assets::PreparedScene,
    script_init::{HostEvent, InitFault, RoomMessage, Snapshot},
    script_player::OwnerBodySpawn,
    script_snapshot_wire::CountdownFrame,
    script_snapshot_wire::{ProjectedSnapshot, SnapshotEncoder, View},
    script_stream::{BodyStream, ChangeKey, DeltaKind, ScriptChunk, Stream},
    vitals::{Damage, Death, Vitals},
    world::{
        Component as WorldComponent, Create, EntityUpdate, GAME_MODE_ENTITY, MAX_PAYLOAD, Movement,
        Operation, SkinTheme, frame,
    },
};

const BODY_REUSE_DELAY: Duration = Duration::from_secs(30);
// Resource bound for this worker. The reference keeps 64 predicted states and
// 32 commands. A farther-ahead command can be retried once the clock catches up.
const MAX_INPUT_LEAD: u32 = 96;
// Several retransmit windows fit before a peer exhausts this queue. One final
// leave marker may follow the accepted prefix when the cap is reached.
const MAX_PENDING_INPUT: usize = 256;
// BodyScript.command retains this many intermediate VM frames before its
// requested frame. The first neutral release runs even across a larger gap.
const MAX_SCRIPT_TAIL: u32 = 256;
const FIRST_BIOTIC_VIEW: u32 = 0xa008_0000;
const END_BIOTIC_VIEW: u32 = 0xa00c_0000;
const BIOTIC_FIELD_ENTITY: u64 = 0x0400_0000_0000_03dc;
const KEEP_MOVEMENT_ROTATION: u32 = 0x0100_0000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BodyId {
    pub entity: u32,
    pub generation: u64,
}

#[derive(Clone, Copy)]
struct Body {
    id: BodyId,
    hero: u64,
    skin: SkinTheme,
}

/// A running owner VM and the authored data used to encode its snapshots.
/// A supported modifier projection is required before the mover can correct
/// the owner's prediction, even if the current set of modifiers is empty.
pub(crate) trait OwnerBodyRuntime: Send + Sync {
    fn set_encounter_active(&mut self, _active: bool) -> io::Result<()> {
        Ok(())
    }
    fn receive_applied_damage(
        &mut self,
        _damage: crate::game_combat::AppliedDamage,
    ) -> io::Result<()> {
        Ok(())
    }
    fn take_velocity_changes(&mut self) -> Vec<(u32, crate::mover::VelocityChange)> {
        Vec::new()
    }
    fn take_script_requests(&mut self) -> Vec<crate::game_combat::RoomScriptRequest> {
        Vec::new()
    }
    fn receive_room_message(
        &mut self,
        _sender: u32,
        _message: RoomMessage,
    ) -> Result<usize, InitFault> {
        Ok(0)
    }
    fn receive_host_event(&mut self, _event: HostEvent) -> Result<usize, InitFault> {
        Ok(0)
    }
    fn set_spatial_frame(&mut self, _frame: Arc<crate::game_combat::RoomSpatialFrame>) {}
    fn set_defenders(&mut self, _defenders: Arc<[u32]>) {}
    fn set_team_member_counts(&mut self, _counts: crate::game_instance::TeamMemberCounts) {}
    fn take_sightings(&mut self) -> Vec<crate::junkenstein_messages::DefenderSighting> {
        Vec::new()
    }
    fn receive_sighting(
        &mut self,
        _message: crate::junkenstein_messages::DefenderSighting,
    ) -> io::Result<()> {
        Ok(())
    }
    fn receive_defender_message(
        &mut self,
        _message: crate::junkenstein_messages::DefenderMessage,
    ) -> io::Result<usize> {
        Ok(0)
    }
    fn cinematic_aim(&self) -> Option<[i32; 2]> {
        None
    }

    fn has_deadeye(&self) -> bool {
        false
    }

    fn set_deadeye_targets(&mut self, _scene: crate::script_player::DeadeyeScene) {}

    #[cfg(test)]
    fn set_deadeye_scene(&mut self, scene: &PreparedScene, team: u8) -> io::Result<()> {
        if self.has_deadeye() {
            self.set_deadeye_targets(scene.deadeye_scene(team).map_err(io::Error::other)?);
        }
        Ok(())
    }

    fn snapshot(&self) -> Option<&Snapshot>;
    #[cfg(test)]
    fn random_checkpoint(&self) -> Option<crate::script_init::RandomCheckpoint> {
        None
    }
    fn encoder(&self) -> SnapshotEncoder<'_>;
    fn advance(&mut self, command: Command, mover: &Mover, step: BodyStep) -> io::Result<()>;
    fn frame_ms(&self) -> u32;

    fn target_tags(&self) -> crate::script_init::TargetTagSnapshot {
        Default::default()
    }
    fn target_filter_flags(&self) -> Option<u64> {
        None
    }

    fn vitals(&self) -> Option<&Vitals> {
        None
    }

    fn heal(&mut self, _amount: f64) -> f64 {
        0.0
    }

    fn receive_damage(&mut self, _amount: f64, _source: Death) -> Damage {
        Damage::default()
    }

    /// New health revisions supersede older ones, including any slot clears.
    fn take_health_update(&mut self) -> Option<WorldComponent> {
        None
    }

    fn projectile_leases(&self) -> Option<&ProjectileLeases> {
        None
    }

    fn movement_modifiers(&self) -> Option<&BTreeMap<ModifierKey, Modifier>> {
        None
    }

    fn take_shots(&mut self, _command: Option<Command>, _mover: &Mover) -> Vec<PosedShot> {
        Vec::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BodyStep {
    Posed,
    UnposedBoundary,
    UnposedTimer,
}

pub(crate) struct Record {
    update: EntityUpdate,
    delivery: Delivery,
}

impl Record {
    pub(crate) fn baseline(update: EntityUpdate) -> Self {
        Self {
            update,
            delivery: Delivery::Baseline,
        }
    }

    fn script(chunk: ScriptChunk, close: bool) -> Self {
        Self {
            update: chunk.update,
            delivery: Delivery::Script {
                last: chunk.last,
                close,
                resend: chunk.resend,
            },
        }
    }
}

pub(crate) struct Ready<'a> {
    index: usize,
    record: &'a Arc<Record>,
}

impl Ready<'_> {
    pub(crate) fn tracked(&self) -> Arc<Record> {
        Arc::clone(self.record)
    }
}

impl Borrow<EntityUpdate> for Ready<'_> {
    fn borrow(&self) -> &EntityUpdate {
        &self.record.update
    }
}

pub(crate) struct WorldDelivery {
    owner: u32,
    queued: Vec<Arc<Record>>,
    created: HashSet<u32>,
    bots: HashMap<u32, BotLifetime>,
    health_updates: HashMap<u32, u64>,
    health_revision: u64,
    // These barriers belong to the peer's reliable stream and survive world clears.
    entity_notice_ordinals: HashMap<u32, u32>,
    started: Option<u32>,
    pub(crate) presentation_due: bool,
    controller: Stream,
    mode: Stream,
    countdown: Option<CountdownFrame>,
    encounter_post_match_queued: bool,
    encounter_screens: Option<ProjectedSnapshot>,
    encounter_notifications: Option<ProjectedSnapshot>,
    encounter_notification_turn: bool,
    encounter_phases: BTreeMap<u16, u16>,
    encounter_result_queued: bool,
    encounter_outcome_queued: bool,
    select_open: bool,
    body: Option<Body>,
    bound: Option<BodyId>,
    generation: u64,
    bodies: HashMap<u32, (u64, Lifetime)>,
    remote_players: HashMap<u32, (BodyId, BotLifetime)>,
    input: InputQueue,
    body_script: Option<BodyScript>,
    projectile_destroys: HashMap<u32, (BodyId, ProjectileToken)>,
    biotic_views: BTreeMap<u32, BioticView>,
    scene_views: BTreeMap<u32, BodyStream<ProjectedSnapshot>>,
    gate: Option<u32>,
    next_biotic_view: u32,
}

mod body;
mod delivery;

pub(crate) use body::validate_body_baselines;
use body::{BioticView, BodyScript, InputQueue};
use delivery::{BotLifetime, Delivery, Lifetime};

#[cfg(test)]
pub(crate) mod tests;
