//! Match-authoritative direct damage and training bot lifetimes.
//!
//! The projectile and body scripts supply shot events. The match supplies
//! the command and pose at consumption, and owns identity-gated delivery.
//! Hit selection, damage, and respawn order follow ow174/game/combat.py.

use std::{
    collections::{BTreeMap, HashMap},
    io,
    sync::Arc,
    time::Duration,
};

use bevy_ecs::{component::Component, entity::Entity, world::World};
use indexmap::IndexMap;

use crate::{
    collision_index::IndexError,
    collision_scene::CollisionScene,
    commands::Command,
    game_bots::{BOT_ENTITY, BOT_RECORD_DELAY, BOT_RESPAWN_TIME, Bot},
    game_combat_messages::CombatNotice,
    game_heroes::SOLDIER,
    game_hitscan::{HitDefinition, HitscanShot, Impact, TargetHistory, TargetPose, ViewFrame},
    game_projectiles::ProjectileToken,
    game_replication::BodyId,
    junkenstein_messages::DefenderMessage,
    junkenstein_navigation::{
        GateApproach, JUNKRAT, REAPER, ROADHOG, ReaperExit, SUMMONER, WITCH, ZOMBARDIER,
    },
    match_scene::bot_create,
    mover::Mover,
    projectile_flight::{ProjectileBallistics, ProjectileFlight},
    scene_assets::{BotQueries, MOVER_CATEGORY},
    script_expr::{RoomEntityFacts, SpatialCandidate},
    script_graph::RoomSpatialNeeds,
    script_init::{HealthTargetFacts, ShotEvent},
    static_ray::{Vec3, capsule_entry_hit, norm, segment_separation},
    vitals::{Death, NamedPoolLoss},
    world::{EntityUpdate, Operation},
};

#[cfg(any(test, feature = "inspector"))]
use crate::game_debug::{Actor, ActorKind};

#[derive(Debug, thiserror::Error)]
pub enum CombatError {
    #[error("bot wire ID {0:08X} is already in use")]
    BotIdInUse(u32),
    #[error("bot wire IDs exhausted")]
    BotIdsExhausted,
    #[error("bot {wire:08X} references missing ECS body {entity:?}")]
    MissingBot { wire: u32, entity: Entity },
    #[error("bot index {wire:08X} references body with wire ID {actual:08X}")]
    BotIdentity { wire: u32, actual: u32 },
    #[error("player {wire:08X} references missing ECS body {entity:?}")]
    MissingPlayerBody { wire: u32, entity: Entity },
    #[error("player index {wire:08X} references body with wire ID {actual:08X}")]
    PlayerBodyIdentity { wire: u32, actual: u32 },
    #[error("projectile batch {connection} references missing ECS flight {entity:?}")]
    MissingFlight { connection: u32, entity: Entity },
    #[error(
        "projectile batch {connection} generation {generation} belongs to {expected:08X}, not {actual:08X}"
    )]
    FlightOwnerIdentity {
        connection: u32,
        generation: u64,
        expected: u32,
        actual: u32,
    },
    #[error("bot index slot {0} is unavailable")]
    MissingBotSlot(usize),
    #[error("cannot encode respawn for bot {wire:08X}: {source}")]
    RespawnCreate {
        wire: u32,
        #[source]
        source: io::Error,
    },
    #[error("combat collision query failed: {0}")]
    Collision(#[from] IndexError),
    #[error("gate {wire:08X} references missing ECS component {entity:?}")]
    MissingGate { wire: u32, entity: Entity },
    #[error("gate index {wire:08X} references gate with wire ID {actual:08X}")]
    GateIdentity { wire: u32, actual: u32 },
    #[error("gate {0:08X} is already registered")]
    GateAlreadyRegistered(u32),
    #[error("encounter references missing ECS component {entity:?}")]
    MissingEncounter { entity: Entity },
    #[error("encounter is already registered")]
    EncounterAlreadyRegistered,
    #[error("encounter requires a registered Junkenstein gate")]
    MissingEncounterGate,
    #[error("invalid Junkenstein gate definition: {0}")]
    InvalidGate(&'static str),
    #[error("invalid Junkenstein encounter state: {0}")]
    InvalidEncounter(&'static str),
}

pub(crate) fn eye_height(hero: u64, crouched: bool) -> f64 {
    match (crouched, hero) {
        (false, _) => 1.55,
        (true, SOLDIER) => 1.085,
        (true, _) => 1.15,
    }
}

pub(crate) fn script_eye_position(hero: u64, mover: &Mover) -> Option<[f64; 3]> {
    let mut position = mover.pose().position;
    position[1] += eye_height(hero, mover.crouched()) + f64::from(mover.spring_offset());
    position.into_iter().all(f64::is_finite).then_some(position)
}

#[derive(Clone, Copy)]
pub(crate) struct PosedHitscan {
    pub source: u32,
    pub origin: [f64; 3],
    pub direction: [f64; 3],
    pub command: Command,
    pub shot_frame: u32,
    pub reach: f64,
    pub hit: HitDefinition,
}

pub(crate) enum PosedShot {
    Hitscan(Box<PosedHitscan>),
    Projectile(Box<PosedProjectile>),
    BioticField(PosedBioticField),
}

pub(crate) const BIOTIC_CANISTER: u64 = 0x0400_0000_0000_072d;
const FIELD_DELAY: u32 = 12;
const FIELD_TICKS: u32 = 313; // ceil(5 seconds / 16 ms).
const FIELD_FADE_TICKS: u32 = 282; // ceil(4.5 seconds / 16 ms).
const HEAL_PER_TICK: f64 = 35.0 * 0.016;

pub(crate) struct PosedBioticField {
    pub shot: ShotEvent,
    pub feet: Vec3,
}

pub(crate) struct BioticField {
    centre: Vec3,
    clock: Option<(u32, u32)>,
}

#[derive(Clone, Copy)]
pub(crate) struct HealingPulse {
    centre: Vec3,
    pub amount: f64,
}

impl HealingPulse {
    pub(crate) fn reaches(self, target: TargetPose) -> bool {
        let [a, b] = target.segment();
        norm(segment_separation(self.centre, a, b)) - target.radius <= 5.0
    }

    fn total_at(pulses: &[Self], target: TargetPose) -> f64 {
        pulses
            .iter()
            .filter(|pulse| pulse.reaches(target))
            .map(|pulse| pulse.amount)
            .sum()
    }
}

impl BioticField {
    pub(crate) const BOUND: u32 = FIELD_DELAY + FIELD_TICKS;

    pub(crate) fn new([x, y, z]: Vec3) -> Self {
        Self {
            centre: [x, y + 1.0, z],
            clock: None,
        }
    }

    pub(crate) fn presentation(&self, tick: u32) -> Option<bool> {
        let (start, _) = self.clock?;
        let age = tick.checked_sub(start)?;
        (age < FIELD_TICKS).then_some(age >= FIELD_FADE_TICKS)
    }

    pub(crate) fn advance(&mut self, tick: u32) -> (Option<HealingPulse>, bool) {
        let (start, last) = self.clock.get_or_insert_with(|| {
            let start = tick.saturating_add(FIELD_DELAY);
            (start, start)
        });
        let end = start.saturating_add(FIELD_TICKS);
        let through = tick.min(end - 1);
        let steps = through.saturating_sub(*last);
        *last = through.max(*last);
        let pulse = (steps != 0).then_some(HealingPulse {
            centre: self.centre,
            amount: HEAL_PER_TICK * f64::from(steps),
        });
        (pulse, tick >= end)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Stun {
    pub ticks: u32,
}

impl Stun {
    pub(crate) fn from_seconds(seconds: f64) -> Option<Self> {
        let ticks = (seconds as f32 / crate::game_clock::TICK_PERIOD.as_secs_f32()).ceil();
        (seconds.is_finite()
            && seconds > 0.0
            && ticks > 0.0
            && f64::from(ticks) < f64::from(u32::MAX))
        .then_some(Self {
            ticks: ticks as u32,
        })
    }
}

pub(crate) struct PosedProjectile {
    pub stun: Option<Stun>,
    pub gravity: Option<f32>,
    pub expiry_blast: bool,
    pub shot: ShotEvent,
    pub ballistics: ProjectileBallistics,
    pub origin: [f64; 3],
    pub direction: [f64; 3],
}

#[derive(Clone, Copy)]
pub(crate) struct ShotContext {
    pub team: u8,
    pub quanta: u32,
    pub tick: u32,
    pub now: Duration,
}

pub(crate) struct BotHit {
    pub damage: AppliedDamage,
    pub notices: [Option<CombatNotice>; 2],
    pub health: EntityUpdate,
}

#[derive(Clone)]
pub(crate) struct AppliedDamage {
    pub source: u32,
    /// Actual pool loss, including armour reduction and remaining health limits.
    pub amount: f64,
    pub named_pool_losses: Vec<NamedPoolLoss>,
    pub tags: crate::game_hitscan::DamageTags,
    pub target_facts: HealthTargetFacts,
}

#[derive(Clone)]
pub(crate) struct BotReplacement {
    pub retired: u32,
    pub created: EntityUpdate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SeatedBotInfo {
    pub slot: u8,
    pub controller: u32,
    pub body: u32,
    pub hero: u64,
    pub team: u8,
    pub name: String,
}

mod encounter;
mod enemy;
mod environment;
mod health;
mod health_events;
mod movement;
mod observed_projectiles;
mod player;
mod script_requests;
mod stock_bot;
pub(crate) use encounter::{
    ENCOUNTER_INSTANCE, EncounterRegistration, EncounterWorld, GateState, GateWorld,
};
use encounter::{EncounterComponent, GateHandle};
#[cfg(test)]
pub(crate) use encounter::{EncounterPhase, GateDamageStage, GateHealth};
use enemy::{BotState, EnemyState};
use health::HealthEffect;
use health_events::{HealthDamageEvidence, HealthEventContext, SampledBody};
pub(crate) use observed_projectiles::{ObservedProjectileEvent, ObservedShot};
pub(crate) use player::{PlayerBodyEvent, PlayerBodyHandle, PlayerBodyState, PlayerDeath};
use script_requests::{PlayerRoomCue, RoomScriptTarget};
pub(crate) use script_requests::{RoomScriptRequest, RoomScriptRequests};

#[derive(Component)]
pub(crate) struct AdmittedFlight {
    pub stun: Option<Stun>,
    pub hit_effect: Option<crate::script_init::Site>,
    pub token: ProjectileToken,
    pub flight: ProjectileFlight,
}

pub(crate) enum FlightOutcome {
    Ended {
        token: ProjectileToken,
        travelled: f64,
    },
    Failed(ProjectileToken),
}

struct FlightBatch {
    owner: BodyId,
    order: Vec<Entity>,
}

/// One room frame's living candidates, optional body facts and static collision view.
#[derive(Clone)]
pub(crate) struct RoomSpatialFrame {
    candidates: Vec<SpatialCandidate>,
    actor_capsules: Option<BTreeMap<u32, TargetPose>>,
    entity_facts: Option<BTreeMap<u32, RoomEntityFacts>>,
    script_targets: HashMap<u32, RoomScriptTarget>,
    player_wires: HashMap<u32, u32>,
    ground: Arc<CollisionScene>,
    #[cfg(any(test, feature = "inspector"))]
    captured_tick: u32,
    #[cfg(any(test, feature = "inspector"))]
    debug_rays: Arc<crate::game_debug::DebugRayHistory>,
}

impl RoomSpatialFrame {
    pub(crate) fn body_generation(&self, wire: u32) -> Option<u64> {
        Some(self.script_targets.get(&wire)?.entity.to_bits())
    }

    pub(crate) fn candidates(&self) -> &[SpatialCandidate] {
        &self.candidates
    }

    pub(crate) fn actor_pose(&self, entity: u32) -> Option<(Vec3, [i32; 2])> {
        let (candidate, target) = self.actor_sample(entity)?;
        Some((candidate.position, target.angles))
    }

    pub(crate) fn actor_eye_position(&self, entity: u32) -> Option<Vec3> {
        self.actor_sample(entity)?.1.eye_position
    }

    /// Opaque room-local identity from the same frozen actor admission as queries.
    pub(crate) fn actor_identity(&self, entity: u32) -> Option<u64> {
        Some(self.actor_sample(entity)?.1.entity.to_bits())
    }

    fn actor_sample(&self, entity: u32) -> Option<(&SpatialCandidate, &RoomScriptTarget)> {
        let target = self.script_targets.get(&entity)?;
        let candidate = self.candidates.get(target.candidate_index)?;
        (candidate.entity == entity).then_some((candidate, target))
    }

    pub(crate) fn entity_facts(&self) -> Option<&BTreeMap<u32, RoomEntityFacts>> {
        self.entity_facts.as_ref()
    }

    pub(crate) fn line_clear(&self, origin: Vec3, target: Vec3) -> Option<bool> {
        self.ground.segment_clear(origin, target, MOVER_CATEGORY)
    }

    pub(crate) fn ray_cast(
        &self,
        site: crate::script_init::Site,
        vm_frame: u32,
        origin: Vec3,
        direction: Vec3,
        max_distance: f64,
    ) -> Option<Option<crate::script_expr::SpatialRayHit>> {
        #[cfg(not(any(test, feature = "inspector")))]
        let _ = (site, vm_frame);
        let result = self
            .ground
            .checked_cast_ray(origin, direction, max_distance, MOVER_CATEGORY)
            .map(|hit| {
                hit.map(|hit| crate::script_expr::SpatialRayHit {
                    distance: hit.distance,
                    point: hit.point,
                    normal: hit.normal,
                })
            });
        #[cfg(any(test, feature = "inspector"))]
        {
            let (hit, end) = match result {
                Some(Some(hit)) => ("world", hit.point),
                Some(None) => (
                    "none",
                    crate::static_ray::along(origin, direction, max_distance),
                ),
                None => (
                    "unavailable",
                    crate::static_ray::along(origin, direction, max_distance),
                ),
            };
            self.record_script_ray(site, vm_frame, "script_static", [origin, end], (hit, None));
        }
        result
    }

    pub(crate) fn actor_ray_cast(
        &self,
        site: crate::script_init::Site,
        vm_frame: u32,
        origin: Vec3,
        direction: Vec3,
        max_distance: f64,
        targets: &[u32],
    ) -> Option<Option<crate::script_expr::SpatialActorRayHit>> {
        #[cfg(not(any(test, feature = "inspector")))]
        let _ = (site, vm_frame);
        #[cfg(any(test, feature = "inspector"))]
        let mut occluding_point = None;
        // Keep availability and actor admission atomic: a partially checked
        // candidate list still returns unavailable, including in diagnostics.
        let result = (|| {
            let capsules = self.actor_capsules.as_ref()?;
            let wall =
                self.ground
                    .checked_cast_ray(origin, direction, max_distance, MOVER_CATEGORY)?;
            let mut nearest: Option<crate::script_expr::SpatialActorRayHit> = None;
            for &entity in targets {
                let target = self.script_targets.get(&entity)?;
                let candidate = self.candidates.get(target.candidate_index)?;
                let pose = capsules.get(&entity)?;
                if candidate.entity != entity
                    || candidate.position != pose.feet
                    || !pose.feet.into_iter().all(f64::is_finite)
                    || !pose.height.is_finite()
                    || pose.height < 0.0
                    || !pose.radius.is_finite()
                    || pose.radius <= 0.0
                {
                    return None;
                }
                let [bottom, top] = pose.segment();
                if !bottom.into_iter().chain(top).all(f64::is_finite) {
                    return None;
                }
                let Some(hit) =
                    capsule_entry_hit(origin, direction, max_distance, bottom, top, pose.radius)
                else {
                    continue;
                };
                let distance = hit.distance;
                if wall.is_some_and(|wall| wall.distance < distance) {
                    #[cfg(any(test, feature = "inspector"))]
                    {
                        occluding_point = wall.map(|wall| wall.point);
                    }
                    continue;
                }
                if nearest.as_ref().is_none_or(|hit| {
                    distance < hit.distance || (distance == hit.distance && entity < hit.entity)
                }) {
                    nearest = Some(crate::script_expr::SpatialActorRayHit {
                        entity,
                        distance,
                        normal: hit.normal,
                    });
                }
            }
            Some(nearest)
        })();
        #[cfg(any(test, feature = "inspector"))]
        {
            let (hit, end, target) = match result {
                Some(Some(hit)) => (
                    "actor",
                    crate::static_ray::along(origin, direction, hit.distance),
                    Some(hit.entity),
                ),
                Some(None) => match occluding_point {
                    Some(point) => ("world", point, None),
                    None => (
                        "none",
                        crate::static_ray::along(origin, direction, max_distance),
                        None,
                    ),
                },
                None => (
                    "unavailable",
                    crate::static_ray::along(origin, direction, max_distance),
                    None,
                ),
            };
            self.record_script_ray(site, vm_frame, "script_actor", [origin, end], (hit, target));
        }
        result
    }

    #[cfg(any(test, feature = "inspector"))]
    fn record_script_ray(
        &self,
        site: crate::script_init::Site,
        vm_frame: u32,
        kind: &'static str,
        [origin, end]: [Vec3; 2],
        (hit, target): (&'static str, Option<u32>),
    ) {
        if !origin.into_iter().chain(end).all(f64::is_finite) {
            return;
        }
        self.debug_rays.push(crate::game_debug::DebugRay {
            tick: self.captured_tick,
            source: site.entity,
            kind,
            origin,
            end,
            hit,
            target,
            critical: false,
            script: Some(crate::game_debug::DebugScriptRay {
                graph: site.graph,
                instance: site.instance,
                node: site.node,
                vm_frame,
            }),
        });
    }
}

struct RoomSpatialQueries {
    free_for_all: bool,
    ground: Arc<CollisionScene>,
    needs: RoomSpatialNeeds,
    actor_movers: Option<Arc<crate::mover_settings::MoverCatalog>>,
}

/// Room-authoritative bot bodies, player bodies, and in-flight projectiles.
pub(crate) struct RoomSimulation {
    world: World,
    // Wire lookup also preserves the authored spawner order across respawns.
    by_wire: IndexMap<u32, Entity>,
    players_by_wire: IndexMap<u32, Entity>,
    players_by_owner: IndexMap<u32, PlayerBodyHandle>,
    team_member_counts: Option<crate::game_instance::TeamMemberCounts>,
    spatial_queries: Option<RoomSpatialQueries>,
    spatial_frame: Option<(u32, Arc<RoomSpatialFrame>)>,
    movement_contacts: Option<movement::MovementContacts>,
    environment: crate::map_environment::MapEnvironment,
    health_events: health_events::RoomHealthEvents,
    // Generation maps live for the room's lifetime so reconnects cannot reset history.
    player_generations: HashMap<u32, u64>,
    latest_wire_generations: HashMap<u32, u64>,
    next_bot: u32,
    combat_active: bool,
    player_targets_active: bool,
    cassidy_protected: bool,
    created: Vec<EntityUpdate>,
    player_events: Vec<PlayerBodyEvent>,
    player_deaths: Vec<player::PlayerDeath>,
    observer_player_health: Vec<(u32, EntityUpdate)>,
    scene_messages: Vec<crate::script_enemy::SceneMessage>,
    external_targets: Option<Vec<crate::script_enemy::EnemyTarget>>,
    external_enemy_damage: Vec<crate::script_enemy::EnemyDamage>,
    scripted_health: Vec<EntityUpdate>,
    retired_enemies: Vec<u32>,
    retired_entity_ids: Arc<std::collections::BTreeSet<u32>>,
    defender_messages: Vec<DefenderMessage>,
    mode_voice_requests: Vec<crate::junkenstein_voice::VoiceRequest>,
    player_cues: Vec<PlayerRoomCue>,
    mode_voice_set: Option<u64>,
    mode_boss_pause: bool,
    spawn_anchors: Option<Arc<crate::scene_assets::SpawnAnchors>>,
    stock_bots: Option<stock_bot::StockBotCatalog>,
    gate_handle: Option<GateHandle>,
    encounter_entity: Option<Entity>,
    // Handle lists preserve launch order within the worker's ordered peer turns.
    projectiles: HashMap<u32, FlightBatch>,
    enemy_projectiles: Vec<Entity>,
    projectile_reports: observed_projectiles::RoomProjectileReports,
    witch_staff_links: IndexMap<u32, (u32, u16)>,
    #[cfg(any(test, feature = "inspector"))]
    debug_rays: Arc<crate::game_debug::DebugRayHistory>,
}

impl RoomSimulation {
    pub(crate) fn create_enemy(
        &mut self,
        spawn: crate::script_enemy::EnemySpawn<'_>,
    ) -> Result<u32, String> {
        use crate::{
            script_enemy::{EnemyBinding, EnemyScripts},
            script_init::CreationKind,
        };
        let crate::script_enemy::EnemySpawn {
            graphs,
            codecs,
            definition,
            request,
            mode,
            mode_variables,
            tick,
            walk,
            gate,
        } = spawn;
        let CreationKind::Hero {
            hero,
            team,
            yaw,
            controller,
            skin,
            ..
        } = &request.kind
        else {
            return Err("scene enemy factory needs an authored hero request".into());
        };
        let pair = graphs
            .enemy_pair(*hero)
            .ok_or("missing authored enemy pair")?;
        if ((crate::npc_catalog::is_junkenstein_boss(*hero) || controller.is_some())
            && *controller != Some(pair.controller))
            || definition.body != pair.body
            || definition.hero != *hero
            || *team > 1
            || !yaw.is_finite()
            || definition.pools.health <= 0.0
            || [
                definition.pools.health,
                definition.pools.armour,
                definition.pools.shields,
                definition.pools.total(),
            ]
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0 || *value > f64::from(f32::MAX))
            || request
                .position
                .iter()
                .any(|value| !value.is_finite() || value.abs() > f64::from(f32::MAX))
        {
            return Err("scene enemy creation differs from its validated definition".into());
        }
        let body = self.next_bot;
        let control = body.checked_add(1).ok_or("enemy IDs exhausted")?;
        if !(BOT_ENTITY..0xd000_0000).contains(&body)
            || control >= 0xd000_0000
            || self.by_wire.contains_key(&body)
            || self.by_wire.contains_key(&control)
            || self
                .checked_states()
                .map_err(|error| error.to_string())?
                .into_iter()
                .filter_map(|state| state.enemy.as_ref())
                .any(|enemy| [body, control].contains(&enemy.binding.controller))
        {
            return Err("enemy entity allocator exhausted or collided".into());
        }
        let bot = Bot::scene_enemy(body, definition, *team, request.position, *yaw, walk, tick);
        let binding = EnemyBinding {
            mode_voice_set: self.mode_voice_set,
            body,
            controller: control,
            hero: *hero,
            pair,
            mode,
            mode_variables,
            team: *team,
            skin: *skin,
            pose: bot.movement(tick),
            gate,
            roster: None,
            spatial_frame: self
                .spatial_frame
                .as_ref()
                .map(|(_, frame)| Arc::clone(frame)),
            retired_entities: Arc::clone(&self.retired_entity_ids),
            frame: tick,
            now_secs: 0.0,
            spawn_anchors: self.spawn_anchors.clone(),
        };
        let mut vitals = definition.pools.vitals();
        let scripts =
            EnemyScripts::start(Arc::clone(&graphs), definition, &binding, tick, &mut vitals)?;
        let encoder = crate::script_snapshot_wire::SnapshotEncoder::new(&graphs, &codecs);
        let snapshot = encoder
            .project(
                scripts.body.snapshot().expect("started body"),
                crate::script_snapshot_wire::View::Observer,
            )
            .map_err(|error| error.to_string())?;
        let payload = encoder.full(&snapshot).map_err(|error| error.to_string())?;
        let projection = crate::world::EntityUpdate {
            entity: body,
            chunk: Some(
                crate::script_wire::chunk(&payload, 0, 1).map_err(|error| error.to_string())?,
            ),
            ..Default::default()
        };
        if crate::world::frame(u32::MAX, u32::MAX, &[], &[], &[&projection])
            .map_err(|error| error.to_string())?
            .len()
            > crate::world::MAX_PAYLOAD
        {
            return Err("enemy observer baseline exceeds the datagram budget".into());
        }
        let mut created = bot_create(&bot, Some(&vitals)).map_err(|error| error.to_string())?;
        if let Some(skin) = skin {
            let Some(Operation::Create(definition)) = &mut created.operation else {
                unreachable!()
            };
            definition.skin.guid = *skin;
        }
        let reaper_exit = ReaperExit::at_spawn(&bot);
        let state = BotState {
            bot,
            vitals,
            history: TargetHistory::new(None),
            respawn_at: None,
            stock_script: None,
            enemy: Some(EnemyState {
                scripts,
                binding,
                graphs,
                codecs,
                fault: None,
                gate_approach: gate.and_then(|gate| GateApproach::new(*hero, gate.position)),
                objective_mover_attached: false,
                reaper_exit,
                boss_pursuit: matches!(*hero, REAPER | ROADHOG | JUNKRAT | WITCH | SUMMONER)
                    .then(Box::default),
                boss_handoff_complete: false,
            }),
        };
        // All fallible preparation precedes the identity and wire publication boundary.
        let entity = self.world.spawn(state).id();
        self.by_wire.insert(body, entity);
        self.next_bot = control + 1;
        self.created.push(created);
        self.combat_active = true;
        Ok(body)
    }

    pub(crate) fn take_created(&mut self) -> Vec<EntityUpdate> {
        std::mem::take(&mut self.created)
    }

    pub(crate) fn queue_scene_message(&mut self, message: crate::script_enemy::SceneMessage) {
        self.scene_messages.push(message);
    }
    #[cfg(test)]
    pub(crate) fn set_external_targets(
        &mut self,
        mut targets: Vec<crate::script_enemy::EnemyTarget>,
    ) {
        targets.sort_unstable_by_key(|target| target.entity);
        self.external_targets = Some(targets);
    }
    #[cfg(test)]
    pub(crate) fn take_external_enemy_damage(&mut self) -> Vec<crate::script_enemy::EnemyDamage> {
        std::mem::take(&mut self.external_enemy_damage)
    }

    /// Synthetic post-AI ingress for testing the real room/owner damage boundary.
    #[cfg(test)]
    pub(crate) fn queue_test_external_enemy_damage(
        &mut self,
        hit: crate::script_enemy::EnemyDamage,
    ) {
        self.external_enemy_damage.push(hit);
    }
    pub(crate) fn take_scripted_health(&mut self) -> Vec<EntityUpdate> {
        std::mem::take(&mut self.scripted_health)
    }
    pub(crate) fn take_retired_enemies(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.retired_enemies)
    }
    pub(crate) fn take_scene_messages(&mut self) -> Vec<crate::script_enemy::SceneMessage> {
        std::mem::take(&mut self.scene_messages)
    }

    pub(crate) fn initial_creates(&self) -> io::Result<Vec<EntityUpdate>> {
        self.states()
            .map(|state| {
                let state = state.map_err(io::Error::other)?;
                let mut update = bot_create(&state.bot, Some(&state.vitals))?;
                if let Some(skin) = state.enemy.as_ref().and_then(|enemy| enemy.binding.skin) {
                    if let Some(Operation::Create(definition)) = &mut update.operation {
                        definition.skin.guid = skin;
                    }
                }
                Ok(update)
            })
            .collect()
    }

    pub(crate) fn enable_stock_bot_scripts(
        &mut self,
        graphs: Arc<crate::script_graph::GraphStore>,
        codecs: Arc<crate::script_codecs::CodecTable>,
        mode: u64,
    ) {
        let catalog = stock_bot::StockBotCatalog {
            graphs,
            codecs,
            mode,
        };
        let entities: Vec<_> = self.by_wire.values().copied().collect();
        for entity in entities {
            let mut state = self
                .world
                .get_mut::<BotState>(entity)
                .expect("live bot handle");
            if state.enemy.is_none() && state.stock_script.is_none() {
                state.stock_script = stock_bot::StockBotScript::start(
                    &catalog,
                    &state.bot,
                    crate::game_clock::FIRST_TICK,
                    stock_bot::BodyEntry::Initial,
                );
            }
        }
        self.stock_bots = Some(catalog);
    }

    pub(crate) fn seat_bots(&mut self, map: &crate::game_content::GameMap, player_count: usize) {
        if map.free_for_all {
            return;
        }
        let first = map
            .team_sizes
            .iter()
            .map(|&count| usize::from(count))
            .sum::<usize>()
            .max(player_count);
        let entities: Vec<_> = self.by_wire.values().copied().collect();
        let mut offset = 0usize;
        for entity in entities {
            let state = self
                .world
                .get_mut::<BotState>(entity)
                .expect("live bot handle")
                .into_inner();
            if state.enemy.is_some() {
                continue;
            }
            let seat = first.saturating_add(offset);
            offset += 1;
            state.bot.slot = (seat < crate::game_instance::PARTICIPANTS)
                .then(|| u8::try_from(seat).expect("participant slot fits in u8"));
        }
    }

    pub(crate) fn seated_bots(&self) -> Vec<SeatedBotInfo> {
        self.by_wire
            .values()
            .filter_map(|&entity| {
                let state = self.world.get::<BotState>(entity)?;
                if state.enemy.is_some() {
                    return None;
                }
                let slot = state.bot.slot?;
                let controller = state.bot.controller()?;
                Some(SeatedBotInfo {
                    slot,
                    controller,
                    body: state.bot.entity,
                    hero: state.bot.spawn.hero,
                    team: state.bot.spawn.team,
                    name: state.bot.spawn.name.clone(),
                })
            })
            .collect()
    }

    pub(crate) fn is_bot_dead(&self, wire: u32) -> bool {
        self.by_wire
            .get(&wire)
            .and_then(|&entity| self.world.get::<BotState>(entity))
            .is_some_and(|state| state.bot.dead)
    }

    fn advance_stock_bots(&mut self, tick: u32) -> Vec<observed_projectiles::StockShotBatch> {
        let mut launches = Vec::new();
        let entities: Vec<_> = self.by_wire.values().copied().collect();
        for entity in entities {
            let state = self
                .world
                .get_mut::<BotState>(entity)
                .expect("live bot handle")
                .into_inner();
            if let Some(script) = state.stock_script.as_mut() {
                script.advance(tick, &state.bot);
                launches.extend(script.take_projectiles(tick, &state.bot));
            }
        }
        launches
    }

    pub(crate) fn enemy_projections(
        &self,
    ) -> Result<impl Iterator<Item = crate::script_snapshot_wire::SceneProjection<'_>>, CombatError>
    {
        Ok(self.checked_states()?.into_iter().filter_map(|state| {
            if let Some(enemy) = state.enemy.as_ref().filter(|enemy| enemy.fault.is_none()) {
                return Some(crate::script_snapshot_wire::SceneProjection {
                    snapshot: enemy.scripts.body.snapshot()?,
                    encoder: crate::script_snapshot_wire::SnapshotEncoder::new(
                        &enemy.graphs,
                        &enemy.codecs,
                    ),
                });
            }
            state
                .stock_script
                .as_ref()
                .and_then(stock_bot::StockBotScript::projection)
        }))
    }

    pub(crate) fn set_mode_voice_set(&mut self, definition: u64) {
        if self.mode_voice_set == Some(definition) {
            return;
        }
        self.mode_voice_set = Some(definition);
        for &entity in self.by_wire.values() {
            let mut state = self
                .world
                .get_mut::<BotState>(entity)
                .expect("live bot handle");
            if let Some(enemy) = state.enemy.as_mut() {
                enemy.binding.mode_voice_set = Some(definition);
            }
        }
    }

    pub(crate) fn set_mode_variables(&mut self, variables: Arc<crate::script_wire::Variables>) {
        for &entity in self.by_wire.values() {
            let mut state = self
                .world
                .get_mut::<BotState>(entity)
                .expect("live bot handle");
            if let Some(enemy) = state.enemy.as_mut() {
                enemy.binding.mode_variables = Arc::clone(&variables);
            }
        }
    }
    pub(crate) fn set_mode_boss_pause(&mut self, active: bool) {
        self.mode_boss_pause = active;
        if let Some(entity) = self.encounter_entity {
            if let Some(mut encounter) = self.world.get_mut::<EncounterComponent>(entity) {
                encounter.boss_pause = active;
            }
        }
    }
    pub(crate) fn new(bots: Vec<Bot>) -> Result<Self, CombatError> {
        let mut world = World::new();
        let mut next_bot = BOT_ENTITY;
        let mut by_wire = IndexMap::with_capacity(bots.len());
        for bot in bots {
            if by_wire.contains_key(&bot.entity) {
                return Err(CombatError::BotIdInUse(bot.entity));
            }
            next_bot = next_bot.max(
                bot.entity
                    .checked_add(1)
                    .ok_or(CombatError::BotIdsExhausted)?,
            );
            by_wire.insert(bot.entity, world.spawn(BotState::new(bot, None)).id());
        }
        Ok(Self {
            world,
            by_wire,
            players_by_wire: IndexMap::new(),
            players_by_owner: IndexMap::new(),
            team_member_counts: None,
            spatial_queries: None,
            spatial_frame: None,
            movement_contacts: None,
            environment: Default::default(),
            health_events: health_events::RoomHealthEvents::default(),
            player_generations: HashMap::new(),
            latest_wire_generations: HashMap::new(),
            next_bot,
            combat_active: false,
            player_targets_active: false,
            cassidy_protected: false,
            created: Vec::new(),
            player_events: Vec::new(),
            player_deaths: Vec::new(),
            observer_player_health: Vec::new(),
            scene_messages: Vec::new(),
            external_targets: None,
            external_enemy_damage: Vec::new(),
            scripted_health: Vec::new(),
            retired_enemies: Vec::new(),
            retired_entity_ids: Arc::default(),
            defender_messages: Vec::new(),
            mode_voice_requests: Vec::new(),
            player_cues: Vec::new(),
            mode_voice_set: None,
            mode_boss_pause: false,
            spawn_anchors: None,
            stock_bots: None,
            gate_handle: None,
            encounter_entity: None,
            projectiles: HashMap::new(),
            enemy_projectiles: Vec::new(),
            projectile_reports: Default::default(),
            witch_staff_links: IndexMap::new(),
            #[cfg(any(test, feature = "inspector"))]
            debug_rays: Arc::default(),
        })
    }

    #[cfg(test)]
    pub(crate) fn set_spawn_anchors(&mut self, anchors: Arc<crate::scene_assets::SpawnAnchors>) {
        self.spawn_anchors = Some(anchors);
    }

    pub(crate) fn spawn_anchors(&self) -> Option<&Arc<crate::scene_assets::SpawnAnchors>> {
        self.spawn_anchors.as_ref()
    }

    pub(crate) fn enable_spatial_queries(
        &mut self,
        free_for_all: bool,
        ground: Arc<CollisionScene>,
        needs: RoomSpatialNeeds,
    ) {
        self.spatial_queries = Some(RoomSpatialQueries {
            free_for_all,
            ground,
            needs,
            actor_movers: None,
        });
    }

    pub(crate) fn enable_spatial_actor_rays(
        &mut self,
        movers: Arc<crate::mover_settings::MoverCatalog>,
    ) {
        if let Some(queries) = &mut self.spatial_queries {
            queries.actor_movers = Some(movers);
        }
    }

    /// Capture room state without changing the room's published frame.
    pub(crate) fn capture_spatial_frame(
        &self,
        frame: u32,
    ) -> Result<Option<Arc<RoomSpatialFrame>>, CombatError> {
        let Some(queries) = &self.spatial_queries else {
            return Ok(None);
        };
        let capacity = self.players_by_wire.len() + self.by_wire.len();
        let mut candidates = Vec::with_capacity(capacity);
        let actor_queries = queries
            .actor_movers
            .as_ref()
            .map(|movers| BotQueries::new(queries.free_for_all, movers, &queries.ground, None));
        let mut actor_capsules = actor_queries.as_ref().map(|_| BTreeMap::new());
        let mut entity_facts = (queries.needs >= RoomSpatialNeeds::EntityFacts).then(BTreeMap::new);
        let include_tags = queries.needs == RoomSpatialNeeds::EntityTags;
        let mut script_targets = HashMap::with_capacity(capacity);
        let mut player_wires = HashMap::with_capacity(self.players_by_wire.len());
        for (state, &entity) in self.states().zip(self.by_wire.values()) {
            let state = state?;
            let bot = &state.bot;
            let alive = !bot.dead && state.vitals.death().is_none();
            let team = (!queries.free_for_all).then_some(bot.spawn.team);
            if let Some(facts) = &mut entity_facts {
                let tags = if include_tags {
                    state.enemy.as_ref().map_or_else(Default::default, |enemy| {
                        enemy.scripts.target_tags(&enemy.graphs, &enemy.binding)
                    })
                } else {
                    Default::default()
                };
                facts.insert(bot.entity, RoomEntityFacts { team, alive, tags });
            }
            if alive {
                let pose = bot.movement(frame);
                if let Some((capsules, actor_queries)) =
                    actor_capsules.as_mut().zip(actor_queries.as_ref())
                {
                    // Use the same collision proxy dimensions as other room
                    // queries, including their catalog fallback policy.
                    let mut capsule = actor_queries.target(bot);
                    capsule.feet = pose.position;
                    capsules.insert(bot.entity, capsule);
                }
                script_targets.insert(
                    bot.entity,
                    RoomScriptTarget {
                        entity,
                        candidate_index: candidates.len(),
                        angles: [pose.yaw, pose.pitch],
                        eye_position: None,
                        health: true,
                        messages: state.enemy.as_ref().is_some_and(|enemy| {
                            enemy.fault.is_none() && !enemy.scripts.retirement_requested
                        }),
                        cue_listener: false,
                    },
                );
                candidates.push(SpatialCandidate {
                    entity: bot.entity,
                    position: pose.position,
                    team,
                });
            }
        }
        for (&wire, &entity) in &self.players_by_wire {
            let state = self
                .world
                .get::<PlayerBodyState>(entity)
                .ok_or(CombatError::MissingPlayerBody { wire, entity })?;
            if state.id.entity != wire {
                return Err(CombatError::PlayerBodyIdentity {
                    wire,
                    actual: state.id.entity,
                });
            }
            player_wires.insert(state.owner, wire);
            let alive = state.alive();
            let team = (!queries.free_for_all).then_some(state.team);
            if let Some(facts) = &mut entity_facts {
                let tags = if include_tags {
                    state
                        .runtime
                        .as_ref()
                        .map_or_else(Default::default, |runtime| runtime.target_tags())
                } else {
                    Default::default()
                };
                facts.insert(wire, RoomEntityFacts { team, alive, tags });
            }
            if alive {
                let pose = state.mover.pose();
                if let Some(capsules) = &mut actor_capsules {
                    capsules.insert(wire, state.target_pose());
                }
                script_targets.insert(
                    wire,
                    RoomScriptTarget {
                        entity,
                        candidate_index: candidates.len(),
                        angles: [pose.yaw, pose.pitch],
                        eye_position: state.script_eye_position(),
                        health: state.vitals().is_some(),
                        messages: state.runtime.is_some(),
                        cue_listener: true,
                    },
                );
                candidates.push(SpatialCandidate {
                    entity: wire,
                    position: pose.position,
                    team,
                });
            }
        }

        Ok(Some(Arc::new(RoomSpatialFrame {
            candidates,
            actor_capsules,
            entity_facts,
            script_targets,
            player_wires,
            ground: Arc::clone(&queries.ground),
            #[cfg(any(test, feature = "inspector"))]
            captured_tick: frame,
            #[cfg(any(test, feature = "inspector"))]
            debug_rays: Arc::clone(&self.debug_rays),
        })))
    }

    /// Read the frozen room frame without the owner's captured body generation.
    pub(crate) fn owner_startup_spatial_frame(&self, owner: u32) -> Option<Arc<RoomSpatialFrame>> {
        let mut frame = Arc::clone(&self.spatial_frame.as_ref()?.1);
        let Some(&wire) = frame.player_wires.get(&owner) else {
            return Some(frame);
        };
        let view = Arc::make_mut(&mut frame);
        view.player_wires.remove(&owner);
        if let Some(capsules) = &mut view.actor_capsules {
            capsules.remove(&wire);
        }
        if let Some(facts) = &mut view.entity_facts {
            facts.remove(&wire);
        }
        if let Some(removed) = view.script_targets.remove(&wire) {
            let index = removed.candidate_index;
            view.candidates.remove(index);
            for target in view.script_targets.values_mut() {
                if target.candidate_index > index {
                    target.candidate_index -= 1;
                }
            }
        }
        Some(frame)
    }

    /// Freeze room queries at the first use of this room frame.
    pub(crate) fn prepare_spatial_frame(&mut self, frame: u32) -> Result<(), CombatError> {
        if self
            .spatial_frame
            .as_ref()
            .is_some_and(|(previous, _)| *previous == frame)
        {
            return Ok(());
        }
        let Some(spatial) = self.capture_spatial_frame(frame)? else {
            return Ok(());
        };
        for &entity in self.players_by_wire.values() {
            if let Some(mut state) = self.world.get_mut::<PlayerBodyState>(entity) {
                if let Some(runtime) = state.runtime.as_mut() {
                    runtime.set_spatial_frame(Arc::clone(&spatial));
                }
            }
        }
        for &entity in self.by_wire.values() {
            if let Some(mut state) = self.world.get_mut::<BotState>(entity) {
                if let Some(enemy) = state.enemy.as_mut() {
                    enemy.binding.spatial_frame = Some(Arc::clone(&spatial));
                }
            }
        }
        self.spatial_frame = Some((frame, spatial));
        Ok(())
    }

    fn states(&self) -> impl ExactSizeIterator<Item = Result<&BotState, CombatError>> {
        self.by_wire.iter().map(|(&wire, &entity)| {
            let state = self
                .world
                .get::<BotState>(entity)
                .ok_or(CombatError::MissingBot { wire, entity })?;
            check_bot_identity(wire, state)?;
            Ok(state)
        })
    }

    #[cfg(test)]
    pub(crate) fn observed_bot_death(
        &self,
        wire: u32,
    ) -> Result<Option<(crate::vitals::Death, Duration)>, CombatError> {
        Ok(self
            .states()
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .find(|state| state.bot.entity == wire)
            .and_then(|state| state.vitals.death().zip(state.respawn_at)))
    }

    fn checked_states(&self) -> Result<Vec<&BotState>, CombatError> {
        self.states().collect()
    }

    fn state_mut(&mut self, wire: u32) -> Result<Option<&mut BotState>, CombatError> {
        let Some(&entity) = self.by_wire.get(&wire) else {
            return Ok(None);
        };
        let state = self
            .world
            .get_mut::<BotState>(entity)
            .ok_or(CombatError::MissingBot { wire, entity })?
            .into_inner();
        check_bot_identity(wire, state)?;
        Ok(Some(state))
    }

    pub(crate) fn bodies(&self) -> Result<impl ExactSizeIterator<Item = &Bot>, CombatError> {
        let bodies = self
            .states()
            .map(|state| state.map(|state| &state.bot))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(bodies.into_iter())
    }

    pub(crate) fn body_script_state(
        &self,
        body: u32,
    ) -> crate::script_entity_state::EntityScriptState {
        self.by_wire
            .get(&body)
            .and_then(|entity| self.world.get::<BotState>(*entity))
            .map_or_else(Default::default, |state| state.script_state())
    }

    pub(crate) fn body_healths(&self) -> Result<std::collections::BTreeMap<u32, f64>, CombatError> {
        self.states()
            .map(|state| state.map(|state| (state.bot.entity, state.vitals.total())))
            .collect()
    }

    pub(crate) fn boss_view_subjects(
        &self,
    ) -> Result<std::collections::BTreeMap<u32, crate::script_player::ViewSubject>, CombatError>
    {
        Ok(self
            .checked_states()?
            .into_iter()
            .filter_map(|state| {
                if !crate::npc_catalog::is_junkenstein_boss(state.bot.spawn.hero) {
                    return None;
                }
                let enemy = state.enemy.as_ref().filter(|enemy| enemy.fault.is_none())?;
                let forward = crate::game_hitscan::aim_direction(&Command {
                    yaw: enemy.binding.pose.yaw,
                    pitch: 0,
                    ..Default::default()
                });
                Some((
                    state.bot.entity,
                    crate::script_player::ViewSubject {
                        position: state.bot.position,
                        forward,
                        tags: enemy.scripts.target_tags(&enemy.graphs, &enemy.binding),
                    },
                ))
            })
            .collect())
    }

    pub(crate) fn living_enemies(
        &self,
        team: u8,
    ) -> Result<impl Iterator<Item = (&Bot, f64)>, CombatError> {
        Ok(self.checked_states()?.into_iter().filter_map(move |state| {
            let bot = &state.bot;
            (bot.spawn.team != team && !bot.dead && state.vitals.death().is_none())
                .then(|| (bot, state.vitals.total()))
        }))
    }

    pub(crate) fn encounter_body_alive(&self, wire_id: u32) -> bool {
        self.by_wire
            .get(&wire_id)
            .and_then(|&entity| self.world.get::<BotState>(entity))
            .is_some_and(|state| !state.bot.dead && state.vitals.death().is_none())
    }

    pub(crate) fn encounter_enemy_hero(&self, wire_id: u32) -> Option<u64> {
        let state = self.world.get::<BotState>(*self.by_wire.get(&wire_id)?)?;
        Some(state.enemy.as_ref()?.binding.hero)
    }

    pub(crate) fn encounter_possessor(&self, body: u32) -> Option<u32> {
        let state = self.world.get::<BotState>(*self.by_wire.get(&body)?)?;
        let enemy = state.enemy.as_ref()?;
        (!state.bot.dead && state.vitals.death().is_none() && enemy.fault.is_none())
            .then_some(enemy.binding.controller)
    }

    pub(crate) fn encounter_body_has_target_tag(&self, body: u32, tag: u64) -> Option<bool> {
        let state = self.world.get::<BotState>(*self.by_wire.get(&body)?)?;
        let enemy = state.enemy.as_ref()?;
        enemy
            .scripts
            .body_has_target_tag(&enemy.graphs, &enemy.binding, tag)
    }

    pub(crate) fn take_boss_intro_starts(&mut self) -> Vec<(u32, crate::script_wire::Variables)> {
        let mut starts = Vec::new();
        for (&body, &handle) in &self.by_wire {
            let state = self
                .world
                .get_mut::<BotState>(handle)
                .expect("live bot handle");
            if let Some(enemy) = &mut state.into_inner().enemy {
                starts.extend(
                    enemy
                        .scripts
                        .take_boss_intro_starts()
                        .into_iter()
                        .map(|parameters| (body, parameters)),
                );
            }
        }
        starts
    }

    pub(crate) fn queue_encounter_wave(
        &mut self,
        caller: crate::script_init::Site,
        controller: u32,
        request: crate::script_init::PlayScript,
    ) -> bool {
        // create_enemy allocates the controller immediately after its body.
        let Some(body) = controller.checked_sub(1) else {
            return false;
        };
        let Some(state) = self.state_mut(body).ok().flatten() else {
            return false;
        };
        let Some(enemy) = state.enemy.as_mut().filter(|enemy| {
            enemy.binding.controller == controller
                && enemy.fault.is_none()
                && !state.bot.dead
                && state.vitals.death().is_none()
        }) else {
            return false;
        };
        enemy
            .scripts
            .queue_wave_script(&enemy.binding, caller, request)
    }

    pub(crate) fn for_each_body_mut(
        &mut self,
        mut step: impl FnMut(&mut Bot),
    ) -> Result<(), CombatError> {
        for (&wire, &entity) in &self.by_wire {
            let mut state = self
                .world
                .get_mut::<BotState>(entity)
                .ok_or(CombatError::MissingBot { wire, entity })?;
            check_bot_identity(wire, &state)?;
            step(&mut state.bot);
            check_bot_identity(wire, &state)?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn body_mut(&mut self, wire_id: u32) -> Option<&mut Bot> {
        Some(&mut self.state_mut(wire_id).unwrap()?.bot)
    }

    #[cfg(any(test, feature = "inspector"))]
    fn record_debug_ray(&self, ray: crate::game_debug::DebugRay) {
        self.debug_rays.push(ray);
    }

    #[cfg(any(test, feature = "inspector"))]
    pub(crate) fn debug_physics(
        &self,
    ) -> Result<Option<crate::game_debug::PhysicsOverlay>, CombatError> {
        use crate::game_debug::{
            MAX_DEBUG_CONTACT_NORMALS, MAX_DEBUG_GROUND_PROBES, PhysicsOverlay, PhysicsTruncation,
        };

        let (rays, rays_truncated) = self.debug_rays.snapshot();
        let mut contact_normals = Vec::new();
        let mut ground_probes = Vec::new();
        let mut truncated = PhysicsTruncation {
            rays: rays_truncated,
            contact_normals: false,
            ground_probes: false,
        };

        self.push_player_debug_physics(
            &mut contact_normals,
            &mut truncated.contact_normals,
            &mut ground_probes,
            &mut truncated.ground_probes,
        );

        for state in self.states() {
            let state = state?;
            for contact in state.bot.debug_contact_normals() {
                if contact_normals.len() == MAX_DEBUG_CONTACT_NORMALS {
                    truncated.contact_normals = true;
                    break;
                }
                contact_normals.push(contact);
            }
            if let Some(mover) = state
                .enemy
                .as_ref()
                .and_then(|enemy| enemy.reaper_exit.as_ref())
                .and_then(|exit| exit.debug_mover())
            {
                if let Some(probe) = mover.debug_ground_probe(state.bot.entity) {
                    if ground_probes.len() == MAX_DEBUG_GROUND_PROBES {
                        truncated.ground_probes = true;
                    } else {
                        ground_probes.push(probe);
                    }
                }
                for contact in mover.debug_contact_normals(state.bot.entity) {
                    if contact_normals.len() == MAX_DEBUG_CONTACT_NORMALS {
                        truncated.contact_normals = true;
                        break;
                    }
                    contact_normals.push(contact);
                }
            }
        }

        if rays.is_empty()
            && contact_normals.is_empty()
            && ground_probes.is_empty()
            && truncated == PhysicsTruncation::default()
        {
            return Ok(None);
        }

        Ok(Some(PhysicsOverlay {
            rays,
            contact_normals,
            ground_probes,
            truncated,
        }))
    }

    #[cfg(any(test, feature = "inspector"))]
    pub(crate) fn debug_navigation(
        &self,
    ) -> Result<Option<crate::game_debug::NavigationOverlay>, CombatError> {
        use crate::game_debug::{MAX_DEBUG_ROUTES, NavigationOverlay, NavigationTruncation};

        let mut routes = Vec::new();
        let mut truncated = NavigationTruncation::default();
        let push_route =
            |routes: &mut Vec<_>,
             truncated: &mut bool,
             route: Option<crate::game_debug::DebugBotRoute>| {
                if let Some(route) = route {
                    if routes.len() == MAX_DEBUG_ROUTES {
                        *truncated = true;
                    } else {
                        routes.push(route);
                    }
                }
            };

        for state in self.states() {
            let state = state?;
            push_route(&mut routes, &mut truncated.routes, state.bot.debug_route());
            if let Some(enemy) = state.enemy.as_ref() {
                push_route(
                    &mut routes,
                    &mut truncated.routes,
                    enemy
                        .gate_approach
                        .as_ref()
                        .and_then(|approach| approach.debug_route(state.bot.entity)),
                );
                push_route(
                    &mut routes,
                    &mut truncated.routes,
                    enemy
                        .boss_pursuit
                        .as_ref()
                        .and_then(|pursuit| pursuit.debug_route(state.bot.entity)),
                );
            }
        }

        let mut spawn_candidates = Vec::new();
        self.push_encounter_debug_spawn_candidates(
            &mut spawn_candidates,
            &mut truncated.spawn_candidates,
        );

        if routes.is_empty()
            && spawn_candidates.is_empty()
            && truncated == NavigationTruncation::default()
        {
            return Ok(None);
        }

        Ok(Some(NavigationOverlay {
            routes,
            spawn_candidates,
            truncated,
        }))
    }

    #[cfg(any(test, feature = "inspector"))]
    pub(crate) fn debug_scripts(
        &self,
    ) -> Result<(Vec<crate::game_debug::EntityScriptSnapshot>, bool), CombatError> {
        use crate::game_debug::{MAX_DEBUG_VM_ENTITIES, summarize_script_snapshot};

        let mut scripts = Vec::new();
        self.push_player_debug_scripts(&mut scripts);
        for state in self.states() {
            let state = state?;
            if let Some(enemy) = state.enemy.as_ref() {
                if let Some(snapshot) = enemy.scripts.body.snapshot() {
                    scripts.push(summarize_script_snapshot(
                        enemy.binding.body,
                        "enemy_body",
                        snapshot,
                    ));
                }
                if let Some(snapshot) = enemy.scripts.controller.snapshot() {
                    scripts.push(summarize_script_snapshot(
                        enemy.binding.controller,
                        "enemy_controller",
                        snapshot,
                    ));
                }
            }
        }
        self.push_encounter_debug_scripts(&mut scripts);
        scripts.sort_unstable_by(|a, b| a.entity.cmp(&b.entity).then_with(|| a.kind.cmp(b.kind)));
        let truncated = scripts.len() > MAX_DEBUG_VM_ENTITIES;
        scripts.truncate(MAX_DEBUG_VM_ENTITIES);
        Ok((scripts, truncated))
    }

    #[cfg(any(test, feature = "inspector"))]
    pub(crate) fn debug_actors(&self) -> Result<impl ExactSizeIterator<Item = Actor>, CombatError> {
        let states = self.states().collect::<Result<Vec<_>, _>>()?;
        let players = self.player_debug_actors();
        Ok(states
            .into_iter()
            .map(|state| {
                let bot = &state.bot;
                Actor {
                    entity: bot.entity,
                    generation: None,
                    kind: ActorKind::Bot,
                    team: bot.spawn.team,
                    position: bot.position,
                    velocity: bot.velocity,
                    alive: !bot.dead,
                    health: Some(if self.combat_active {
                        state.vitals.total()
                    } else {
                        bot.spawn.health
                    }),
                    script_active: bot.patrol_active(),
                    last_contact_clip: bot.last_contact_clip(),
                }
            })
            .chain(players)
            .collect::<Vec<_>>()
            .into_iter())
    }

    pub(crate) fn set_junkenstein_defenders(&mut self, defenders: Arc<[u32]>) {
        for &handle in self.by_wire.values() {
            let mut state = self
                .world
                .get_mut::<BotState>(handle)
                .expect("live bot handle");
            if let Some(enemy) = &mut state.enemy {
                enemy.scripts.set_defenders(Arc::clone(&defenders));
            }
        }
    }

    pub(crate) fn take_defender_sightings(
        &mut self,
    ) -> Vec<crate::junkenstein_messages::DefenderSighting> {
        self.by_wire
            .values()
            .filter_map(|&handle| {
                self.world
                    .get_mut::<BotState>(handle)
                    .expect("live bot handle")
                    .into_inner()
                    .enemy
                    .as_mut()
                    .map(|enemy| enemy.scripts.take_sightings())
            })
            .flatten()
            .collect()
    }

    pub(crate) fn queue_defender_message(&mut self, message: DefenderMessage) -> bool {
        if self.defender_messages.len() >= crate::script_init::MAX_INIT_WORK {
            return false;
        }
        self.defender_messages.push(message);
        true
    }

    pub(crate) fn queue_enemy_result(&mut self, parameters: &crate::script_wire::Variables) {
        for &handle in self.by_wire.values() {
            let state = self
                .world
                .get_mut::<BotState>(handle)
                .expect("live bot handle")
                .into_inner();
            let Some(enemy) = state.enemy.as_mut().filter(|enemy| enemy.fault.is_none()) else {
                continue;
            };
            let body = &mut enemy.scripts.body;
            if body.snapshot().is_some_and(|snapshot| {
                snapshot
                    .instances
                    .values()
                    .any(|instance| instance.graph == 0x0cbc)
            }) {
                if let Err(error) = body.queue_entity_message_from(
                    crate::world::GAME_MODE_ENTITY,
                    crate::junkenstein_messages::ENCOUNTER_RESULT,
                    parameters.clone(),
                ) {
                    eprintln!(
                        "enemy {} result delivery failed: {error}",
                        enemy.binding.body
                    );
                    enemy.halt(&mut state.bot, error.to_string());
                }
            }
        }
    }

    pub(crate) fn take_defender_messages(&mut self) -> Vec<DefenderMessage> {
        let mut messages = std::mem::take(&mut self.defender_messages);
        for &handle in self.by_wire.values() {
            let state = self
                .world
                .get_mut::<BotState>(handle)
                .expect("live bot handle");
            if let Some(enemy) = &mut state.into_inner().enemy {
                messages.extend(enemy.scripts.take_defender_messages());
            }
        }
        messages
    }

    pub(crate) fn queue_mode_voice(&mut self, request: crate::junkenstein_voice::VoiceRequest) {
        self.mode_voice_requests.push(request);
    }

    pub(crate) fn take_encounter_voice_requests(
        &mut self,
    ) -> Vec<crate::junkenstein_voice::VoiceRequest> {
        let mut requests = self.take_gate_voice_requests();
        requests.extend(std::mem::take(&mut self.mode_voice_requests));
        requests.extend(
            self.by_wire
                .values()
                .filter_map(|&handle| {
                    self.world
                        .get_mut::<BotState>(handle)
                        .expect("live bot handle")
                        .into_inner()
                        .enemy
                        .as_mut()
                        .map(|enemy| enemy.scripts.take_voice_requests())
                })
                .flatten(),
        );
        requests
    }

    pub(crate) fn activate_combat(&mut self) {
        self.combat_active = true;
    }

    fn record_poses(&mut self, queries: &BotQueries<'_>, tick: u32) -> Result<(), CombatError> {
        for (&wire, &entity) in &self.by_wire {
            let state = self
                .world
                .get_mut::<BotState>(entity)
                .ok_or(CombatError::MissingBot { wire, entity })?;
            check_bot_identity(wire, &state)?;
            if !state.bot.dead {
                let state = state.into_inner();
                state.history.record(tick, queries.target(&state.bot));
            }
        }
        self.record_player_poses(tick);
        Ok(())
    }

    pub(crate) fn hitscan(
        &mut self,
        queries: &BotQueries<'_>,
        shot: PosedHitscan,
        context: ShotContext,
        visible: impl Fn(u32) -> bool,
    ) -> Result<Option<BotHit>, CombatError> {
        let view = ViewFrame::from_command(&shot.command, context.quanta);
        let ray = HitscanShot::with_direction(shot.origin, shot.direction, shot.reach, shot.hit);
        // Scene order breaks exact ray ties.
        let mut candidates = Vec::with_capacity(self.by_wire.len() + self.players_by_wire.len());
        for state in self.states() {
            let state = state?;
            let bot = &state.bot;
            if !eligible_bot(bot, shot.source, context.team, queries.free_for_all())
                || !visible(bot.entity)
            {
                continue;
            }
            if let Some(pose) = state.history.at(view, queries.target(bot)) {
                candidates.push((bot.entity, queries.hurt_pose(bot.spawn.body, pose)));
            }
        }
        self.push_player_hitscan_candidates(
            shot.source,
            context.team,
            queries.free_for_all(),
            view,
            &visible,
            &mut candidates,
        );
        #[cfg(any(test, feature = "inspector"))]
        let mut blocking_wall = None;
        let Some(impact) = ray.resolve_combat(candidates, |origin, direction, reach| {
            let wall = queries.hitscan_wall(origin, direction, reach)?;
            #[cfg(any(test, feature = "inspector"))]
            {
                blocking_wall = wall.filter(|distance| *distance < reach);
            }
            Ok(wall)
        })?
        else {
            #[cfg(any(test, feature = "inspector"))]
            {
                // An occluder already found before the actor is the first
                // wall along the full ray as well.
                let wall = blocking_wall.or_else(|| {
                    queries
                        .hitscan_wall(ray.origin, ray.direction, ray.reach)
                        .ok()
                        .flatten()
                });
                let (hit, end) = match wall {
                    Some(distance) => (
                        "world",
                        crate::static_ray::along(ray.origin, ray.direction, distance),
                    ),
                    None => (
                        "none",
                        crate::static_ray::along(ray.origin, ray.direction, ray.reach),
                    ),
                };
                self.record_debug_ray(crate::game_debug::DebugRay {
                    tick: context.tick,
                    source: shot.source,
                    kind: "hitscan",
                    origin: ray.origin,
                    end,
                    hit,
                    target: None,
                    critical: false,
                    script: None,
                });
            }
            return Ok(None);
        };
        #[cfg(any(test, feature = "inspector"))]
        self.record_debug_ray(crate::game_debug::DebugRay {
            tick: context.tick,
            source: shot.source,
            kind: "hitscan",
            origin: ray.origin,
            end: crate::static_ray::along(ray.origin, ray.direction, impact.distance),
            hit: "actor",
            target: Some(impact.entity),
            critical: impact.critical,
            script: None,
        });
        if self.players_by_wire.contains_key(&impact.entity) {
            return self.apply_player_impact(shot.source, shot.shot_frame, context, &impact);
        }
        self.apply_bot_impact(shot.source, shot.shot_frame, context, &impact)
    }

    #[cfg(test)]
    pub(crate) fn heal_bots(
        &mut self,
        queries: &BotQueries<'_>,
        team: u8,
        pulses: &[HealingPulse],
    ) -> Result<Vec<EntityUpdate>, CombatError> {
        self.heal_allies(queries, team, None, None, pulses)
    }

    pub(crate) fn heal_allies(
        &mut self,
        queries: &BotQueries<'_>,
        team: u8,
        skip_player_wire: Option<u32>,
        field_owner: Option<BodyId>,
        pulses: &[HealingPulse],
    ) -> Result<Vec<EntityUpdate>, CombatError> {
        if queries.free_for_all() || pulses.is_empty() {
            return Ok(Vec::new());
        }
        self.activate_combat();
        let source = field_owner
            .filter(|&id| self.player_body(id).is_some())
            .and_then(|id| self.sample_health_body(id.entity));
        let mut updates = Vec::new();
        for (&wire, &entity) in &self.by_wire {
            let state = self
                .world
                .get_mut::<BotState>(entity)
                .ok_or(CombatError::MissingBot { wire, entity })?
                .into_inner();
            check_bot_identity(wire, state)?;
            let bot = &state.bot;
            if bot.dead || bot.spawn.team != team {
                continue;
            }
            let amount = HealingPulse::total_at(pulses, queries.target(bot));
            let healed = state.vitals.heal(amount);
            if healed <= 0.0 {
                continue;
            }
            let component = state.vitals.component();
            state.vitals.clear_changed();
            updates.push(EntityUpdate {
                entity: bot.entity,
                operation: Some(Operation::Update(vec![component])),
                ..EntityUpdate::default()
            });
            let target = SampledBody { wire, entity };
            let room_status = self.sample_health_target_room_status(target);
            self.health_events.stage(
                source,
                target,
                healed,
                false,
                HealthEventContext {
                    world: &self.world,
                    damage: None,
                },
                room_status,
            );
        }
        self.heal_player_bodies(team, skip_player_wire, source, pulses, &mut updates);
        Ok(updates)
    }

    pub(crate) fn advance_projectiles(
        &mut self,
        queries: &BotQueries<'_>,
        connection: u32,
        owner: BodyId,
        launches: Vec<AdmittedFlight>,
        context: ShotContext,
    ) -> Result<(Vec<FlightOutcome>, Vec<BotHit>), CombatError> {
        if !self.player_body_generation_current(owner) {
            return Ok((Vec::new(), Vec::new()));
        }
        if let Some(batch) = self.projectiles.get(&connection) {
            // Body generations are monotonic within a live connection. Late
            // work cannot inspect, advance or retire the replacement's batch.
            if owner.generation < batch.owner.generation {
                return Ok((Vec::new(), Vec::new()));
            }
            if owner.generation == batch.owner.generation && owner.entity != batch.owner.entity {
                return Err(CombatError::FlightOwnerIdentity {
                    connection,
                    generation: owner.generation,
                    expected: batch.owner.entity,
                    actual: owner.entity,
                });
            }
            for &entity in &batch.order {
                if self.world.get::<AdmittedFlight>(entity).is_none() {
                    return Err(CombatError::MissingFlight { connection, entity });
                }
            }
        }
        if self
            .projectiles
            .get(&connection)
            .is_some_and(|batch| batch.owner != owner)
        {
            self.retire_projectiles(connection);
        }
        let mut order = self
            .projectiles
            .remove(&connection)
            .map_or_else(Vec::new, |batch| batch.order);
        order.extend(
            launches
                .into_iter()
                .map(|launch| self.world.spawn(launch).id()),
        );
        let mut outcomes = Vec::new();
        let mut hits = Vec::new();
        if order.is_empty() {
            return Ok((outcomes, hits));
        }

        self.activate_combat();
        let mut continuing = Vec::with_capacity(order.len());
        for entity in order {
            // An earlier flight may have killed a target in this same tick.
            let mut candidates =
                Vec::with_capacity(self.by_wire.len() + self.players_by_wire.len());
            for state in self.states() {
                let bot = &state?.bot;
                if eligible_bot(bot, owner.entity, context.team, queries.free_for_all()) {
                    let pose = queries.hurt_pose(bot.spawn.body, queries.target(bot));
                    candidates.push((bot.entity, pose));
                }
            }
            self.push_player_projectile_candidates(
                owner.entity,
                context.team,
                queries.free_for_all(),
                &mut candidates,
            );
            let (token, stun, hit_effect, result) = {
                let mut active = self
                    .world
                    .get_mut::<AdmittedFlight>(entity)
                    .ok_or(CombatError::MissingFlight { connection, entity })?;
                let result = active.flight.advance_combat(
                    context.tick,
                    &candidates,
                    |origin, direction, reach| queries.hitscan_wall(origin, direction, reach),
                );
                (active.token, active.stun, active.hit_effect, result)
            };
            match result {
                Ok(None) => {
                    continuing.push(entity);
                    continue;
                }
                Ok(Some(end)) => {
                    #[cfg(feature = "inspector")]
                    {
                        let surface = if end.expired {
                            "expiry"
                        } else if end.contact.is_some() {
                            "actor"
                        } else {
                            "world"
                        };
                        crate::owner_trace::playtest(format_args!(
                            "projectile event=ended peer={connection} owner={:08x} token={token:?} tick={} surface={surface} contact={:?} point={:?} travelled_s={} impacts={}",
                            owner.entity,
                            context.tick,
                            end.contact,
                            end.point,
                            end.travelled,
                            end.impacts.len()
                        ));
                    }
                    for impact in &end.impacts {
                        #[cfg(feature = "inspector")]
                        crate::owner_trace::playtest(format_args!(
                            "projectile event=impact peer={connection} owner={:08x} token={token:?} tick={} target={:08x} raw_damage={} distance={}",
                            owner.entity,
                            context.tick,
                            impact.entity,
                            impact.damage,
                            impact.distance
                        ));
                        if self.players_by_wire.contains_key(&impact.entity) {
                            if let Some(hit) = self.apply_player_impact(
                                owner.entity,
                                context.tick,
                                context,
                                impact,
                            )? {
                                hits.push(hit);
                            }
                            continue;
                        }
                        if let Some(hit) =
                            self.apply_bot_impact(owner.entity, context.tick, context, impact)?
                        {
                            if let Some(stun) = stun {
                                if let Some(state) = self.state_mut(impact.entity)? {
                                    let mut control_allowed = true;
                                    let reaction =
                                        hit_effect.filter(|_| !state.bot.dead).and_then(|caller| {
                                            state
                                                .enemy
                                                .as_mut()
                                                .filter(|enemy| enemy.fault.is_none())
                                                .map(|enemy| (caller, enemy))
                                        });
                                    if let Some((caller, enemy)) = reaction {
                                        match enemy.scripts.apply_legacy_projectile_control(
                                            &enemy.graphs,
                                            &enemy.binding,
                                            caller,
                                        ) {
                                            Ok(allowed) => control_allowed = allowed,
                                            Err(error) => {
                                                control_allowed = false;
                                                enemy.halt(&mut state.bot, error);
                                            }
                                        }
                                    }
                                    if control_allowed {
                                        state.bot.stun_for(context.tick, stun.ticks);
                                    }
                                }
                            }
                            hits.push(hit);
                        }
                    }
                    outcomes.push(FlightOutcome::Ended {
                        token,
                        travelled: end.travelled,
                    });
                }
                // A query exceeding the bounded ray budget cancels only this
                // flight. Broken geometry and ECS ownership are room faults.
                Err(error) => {
                    #[cfg(feature = "inspector")]
                    crate::owner_trace::playtest(format_args!(
                        "projectile event=failed peer={connection} owner={:08x} token={token:?} tick={} reason={error:?}",
                        owner.entity, context.tick
                    ));
                    if matches!(error, IndexError::RayTraversalLimit) {
                        outcomes.push(FlightOutcome::Failed(token));
                    } else {
                        return Err(error.into());
                    }
                }
            }
            if !self.world.despawn(entity) {
                return Err(CombatError::MissingFlight { connection, entity });
            }
        }
        if !continuing.is_empty() {
            self.projectiles.insert(
                connection,
                FlightBatch {
                    owner,
                    order: continuing,
                },
            );
        }
        Ok((outcomes, hits))
    }

    pub(crate) fn retire_projectiles(&mut self, connection: u32) {
        if let Some(batch) = self.projectiles.remove(&connection) {
            for entity in batch.order {
                // Cleanup must remain usable while retiring a damaged room.
                self.world.despawn(entity);
            }
        }
    }

    pub(crate) fn clear_projectiles(&mut self) {
        for entity in self.projectiles.drain().flat_map(|(_, batch)| batch.order) {
            self.world.despawn(entity);
        }
    }

    fn apply_bot_impact(
        &mut self,
        source: u32,
        shot_frame: u32,
        context: ShotContext,
        impact: &Impact,
    ) -> Result<Option<BotHit>, CombatError> {
        let Some(state) = self.state_mut(impact.entity)? else {
            // An impact carrying a retired wire identity must not damage its
            // replacement body.
            return Ok(None);
        };
        let Some(result) = Self::damage_bot_with_hit(
            state,
            impact.damage,
            Death {
                time: context.now.as_secs_f64(),
                frame: context.tick,
                killer: source,
            },
            context.now,
            impact.critical,
            impact.loadout,
        ) else {
            return Ok(None);
        };
        let entity = state.bot.entity;
        let health = bot_health_update(state, result.killed);
        let hit = BotHit {
            damage: AppliedDamage {
                source,
                amount: result.dealt,
                named_pool_losses: result.named_pool_losses,
                tags: impact.tags,
                target_facts: state.health_target_facts(),
            },
            notices: [
                Some(CombatNotice::Hit {
                    target: entity,
                    dealt: result.dealt,
                    critical: impact.critical,
                    killed: result.killed,
                    shot_frame,
                }),
                result.killed.then_some(CombatNotice::Elimination {
                    killer: source,
                    victim: entity,
                }),
            ],
            health,
        };
        self.record_health_applied(
            source,
            entity,
            -result.dealt,
            result.killed,
            Some(HealthDamageEvidence {
                impact: Some((hit.damage.tags, &hit.damage.target_facts)),
                named_pool_losses: &hit.damage.named_pool_losses,
            }),
        );
        Ok(Some(hit))
    }

    fn damage_bot(
        state: &mut BotState,
        amount: f64,
        source: Death,
        now: Duration,
    ) -> Option<crate::vitals::Damage> {
        Self::damage_bot_with_hit(state, amount, source, now, false, None)
    }

    #[cfg(test)]
    pub(crate) fn kill_bot_for_test(&mut self, entity: u32, tick: u32, now: Duration) {
        self.combat_active = true;
        let state = self
            .state_mut(entity)
            .expect("test bot state lookup")
            .expect("test bot entity");
        Self::damage_bot(
            state,
            10_000.0,
            Death {
                killer: 0xa000_0100,
                frame: tick,
                time: now.as_secs_f64(),
            },
            now,
        );
    }

    fn damage_bot_with_hit(
        state: &mut BotState,
        amount: f64,
        source: Death,
        now: Duration,
        critical: bool,
        loadout: Option<u64>,
    ) -> Option<crate::vitals::Damage> {
        if state.bot.dead {
            return None;
        }
        let result = state.vitals.damage(amount, source);
        if let Some(script) = &mut state.stock_script {
            script.on_damage(state.vitals.total(), source);
        }
        if result.killed {
            state.bot.died(source.frame);
            if let Some(script) = &mut state.stock_script {
                script.on_death(source, critical, loadout);
            }
            if let Some(enemy) = &mut state.enemy {
                if let Err(error) = enemy.scripts.queue_death(source, critical, loadout) {
                    enemy.halt(&mut state.bot, format!("enemy death input: {error}"));
                }
            }
            state.respawn_at = state
                .enemy
                .is_none()
                .then(|| now.saturating_add(BOT_RESPAWN_TIME));
        }
        (result.dealt > 0.0).then_some(result)
    }

    pub(crate) fn finish_tick(
        &mut self,
        queries: &BotQueries<'_>,
        tick: u32,
        now: Duration,
    ) -> Result<Vec<BotReplacement>, CombatError> {
        self.prepare_movement_frame(tick)?;
        self.prepare_spatial_frame(tick)?;
        let motion_frame = self.movement_contact_frame();
        self.states().try_for_each(|state| state.map(|_| ()))?;
        self.validate_gate_and_encounter_handles()?;
        if !self.players_by_wire.is_empty() {
            let defenders: Arc<[u32]> = Arc::from(self.collect_junkenstein_defenders());
            self.set_junkenstein_defenders(Arc::clone(&defenders));
            self.set_player_defenders(defenders);
        }
        let stock_launches = self.advance_stock_bots(tick);
        if !stock_launches.is_empty() {
            self.activate_combat();
        }
        if !self.combat_active {
            self.collect_local_health_events();
            self.settle_authored_requests(tick, now)?;
            self.settle_environment(tick, now)?;
            self.flush_health_events();
            return self.respawn_due(tick, now);
        }
        let external_and_players = self.collect_player_enemy_targets();
        let has_roster = self.player_targets_active
            || !self.players_by_wire.is_empty()
            || self.external_targets.is_some()
            || !stock_launches.is_empty()
            || !self.enemy_projectiles.is_empty();
        let states = self.checked_states()?;
        let roster =
            has_roster.then(|| {
                Arc::new(
                    external_and_players
                        .iter()
                        .cloned()
                        .chain(states.iter().copied().map(|state| {
                            crate::script_enemy::EnemyTarget {
                                hero: Some(state.bot.spawn.hero),
                                filter_flags: Some(
                                    1 | (0x0080_0000 << state.bot.spawn.team)
                                        | if state.bot.dead { 0x10 } else { 0 },
                                ),
                                tags: state.enemy.as_ref().map_or_else(Default::default, |enemy| {
                                    enemy.scripts.target_tags(&enemy.graphs, &enemy.binding)
                                }),
                                health: Some(state.vitals.total()),
                                velocity: Some(state.bot.velocity),
                                script_state: state.script_state(),
                                entity: state.bot.entity,
                                team: state.bot.spawn.team,
                                pose: queries.target(&state.bot),
                                alive: !state.bot.dead,
                            }
                        }))
                        .collect(),
                )
            });
        let mut retired = Vec::new();
        let mut launched = Vec::new();
        let mut enemy_damage = Vec::new();
        #[cfg(any(test, feature = "inspector"))]
        let mut recorded_rays = Vec::new();
        let witch_present = states
            .iter()
            .copied()
            .any(|state| state.bot.spawn.hero == WITCH && !state.bot.dead);
        let witch_allies = if witch_present {
            states
                .iter()
                .copied()
                .filter_map(|state| {
                    let enemy = state.enemy.as_ref()?;
                    let current = state.vitals.total();
                    let maximum = state.vitals.maximum();
                    (!state.bot.dead
                        && state.vitals.death().is_none()
                        && enemy.fault.is_none()
                        && current < maximum)
                        .then(|| {
                            let favored = enemy
                                .scripts
                                .body_has_target_tag(
                                    &enemy.graphs,
                                    &enemy.binding,
                                    0x0d60_0000_0000_011e,
                                )
                                .unwrap_or(false);
                            (state.bot.entity, (favored, current / maximum))
                        })
                })
                .collect::<HashMap<_, _>>()
        } else {
            HashMap::new()
        };
        let pause_senders = states
            .iter()
            .copied()
            .filter(|state| {
                state
                    .enemy
                    .as_ref()
                    .is_some_and(|enemy| enemy.fault.is_none() && enemy.scripts.boss_pause_active())
            })
            .count();
        let mode_boss_pause = self.encounter_boss_pause();
        for &entity in self.by_wire.values() {
            let mut state = self
                .world
                .get_mut::<BotState>(entity)
                .expect("live bot handle");
            let BotState {
                bot, vitals, enemy, ..
            } = &mut *state;
            if let Some(enemy) = enemy.as_mut().filter(|enemy| enemy.fault.is_none()) {
                if let Some(frame) = &motion_frame {
                    frame.stage_bot(bot, tick);
                }
                if bot.dead || vitals.death().is_some() {
                    enemy.scripts.clear_pending_script_motion();
                }
                if let Some(motion) = enemy
                    .scripts
                    .take_scripted_motion(tick)
                    .filter(|_| !bot.dead && vitals.death().is_none())
                {
                    if !enemy.objective_mover_attached {
                        enemy.objective_mover_attached = queries.attach_objective_mover(bot);
                    }
                    if enemy.objective_mover_attached {
                        bot.apply_scripted_motion(tick, motion);
                    }
                }
                enemy.binding.frame = tick;
                enemy.binding.now_secs = now.as_secs_f64();
                enemy.binding.retired_entities = Arc::clone(&self.retired_entity_ids);
                bot.queue_velocity_changes(enemy.scripts.take_velocity_changes());
                let stunned = bot.is_stunned(tick);
                enemy.scripts.set_control_blocked(stunned);
                let other_pause = pause_senders > usize::from(enemy.scripts.boss_pause_active());
                let room_pause = mode_boss_pause || other_pause;
                enemy.scripts.set_room_boss_pause(room_pause);
                if enemy.binding.hero == WITCH {
                    let targets = roster.as_deref().map(Vec::as_slice).unwrap_or_default();
                    if let Err(error) = enemy.select_witch_targets(
                        bot,
                        tick,
                        queries,
                        if room_pause || stunned || vitals.death().is_some() {
                            &[]
                        } else {
                            targets
                        },
                        &witch_allies,
                    ) {
                        enemy.halt(bot, error.to_string());
                        continue;
                    }
                }
                if enemy.binding.hero == SUMMONER {
                    let targets = roster.as_deref().map(Vec::as_slice).unwrap_or_default();
                    if let Err(error) = enemy.select_summoner_targets(
                        bot,
                        tick,
                        queries,
                        if room_pause || stunned || vitals.death().is_some() {
                            &[]
                        } else {
                            targets
                        },
                    ) {
                        enemy.halt(bot, error.to_string());
                        continue;
                    }
                }
                if enemy.binding.hero == ZOMBARDIER {
                    let targets = roster.as_deref().map(Vec::as_slice).unwrap_or_default();
                    enemy.aim_zombardier(
                        bot,
                        tick,
                        queries,
                        if room_pause || stunned || vitals.death().is_some() {
                            &[]
                        } else {
                            targets
                        },
                    );
                }
                if stunned {
                    bot.stop_objective(tick);
                } else if let Err(error) = enemy.approach_gate(bot, tick, queries) {
                    bot.stop_objective(tick);
                    enemy.halt(bot, error.to_string());
                    continue;
                }
                if let Err(error) = enemy.advance_boss_weapon(
                    bot,
                    tick,
                    queries,
                    room_pause || stunned || vitals.death().is_some(),
                    &external_and_players,
                ) {
                    bot.stop_objective(tick);
                    enemy.halt(bot, error.to_string());
                    continue;
                }
                let navigation_pose = bot.movement(tick);
                enemy.binding.pose = navigation_pose;
                enemy.binding.roster = roster.clone();
                // Sample the facing already held at entry without overriding the
                // bot's navigation pose if the script releases it this update.
                if let Some([yaw, pitch]) = enemy
                    .scripts
                    .authored_facing(enemy.binding.body)
                    .filter(|_| {
                        !bot.dead
                            && vitals.death().is_none()
                            && enemy.scripts.cinematic_aim().is_none()
                    })
                {
                    enemy.binding.pose.yaw = yaw;
                    enemy.binding.pose.pitch = pitch;
                }
                let advanced = enemy
                    .scripts
                    .advance(&enemy.graphs, &enemy.binding, vitals, tick);
                enemy.binding.pose = navigation_pose;
                if let Err(error) = advanced {
                    eprintln!("enemy {} halted: {error}", enemy.binding.body);
                    bot.stop_objective(tick);
                    enemy.halt(bot, error);
                } else if let Some(aim) = enemy.scripts.cinematic_aim() {
                    bot.apply_encounter_aim(aim, tick);
                } else if let Some(aim) = enemy.scripts.authored_facing(enemy.binding.body) {
                    bot.apply_encounter_aim(aim, tick);
                } else if crate::npc_catalog::is_junkenstein_boss(enemy.binding.hero) {
                    if let Some(direction) = enemy.scripts.facing_direction() {
                        bot.apply_encounter_facing(direction, tick);
                    }
                }
                if enemy.fault.is_none()
                    && matches!(
                        enemy.binding.hero,
                        REAPER | ROADHOG | JUNKRAT | ZOMBARDIER | WITCH | SUMMONER
                    )
                {
                    let shots = enemy.scripts.take_weapon_shots(
                        &enemy.graphs,
                        &enemy.binding,
                        vitals,
                        tick,
                    );
                    launched.extend(shots.projectiles);
                    for volley in shots.hitscan {
                        for pellet in volley.pellets {
                            let targets = roster
                                .iter()
                                .flat_map(|targets| targets.iter())
                                .filter(|target| target.alive && target.team != enemy.binding.team)
                                .map(|target| (target.entity, target.pose));
                            match pellet.resolve(targets, |origin, direction, reach| {
                                queries.hitscan_wall(origin, direction, reach)
                            }) {
                                Ok(Some(impact)) => {
                                    #[cfg(any(test, feature = "inspector"))]
                                    recorded_rays.push(crate::game_debug::DebugRay {
                                        tick,
                                        source: bot.entity,
                                        kind: "hitscan",
                                        origin: pellet.origin,
                                        end: crate::static_ray::along(
                                            pellet.origin,
                                            pellet.direction,
                                            impact.distance,
                                        ),
                                        hit: "actor",
                                        target: Some(impact.entity),
                                        critical: impact.critical,
                                        script: None,
                                    });
                                    enemy_damage.push(crate::script_enemy::EnemyDamage {
                                        source: bot.entity,
                                        target: impact.entity,
                                        amount: enemy
                                            .scripts
                                            .outgoing_damage(impact.damage, &volley.damage_tags),
                                    });
                                }
                                Ok(None) => {
                                    #[cfg(any(test, feature = "inspector"))]
                                    {
                                        let (hit, end) = match queries
                                            .hitscan_wall(
                                                pellet.origin,
                                                pellet.direction,
                                                pellet.reach,
                                            )
                                            .ok()
                                            .flatten()
                                        {
                                            Some(distance) => (
                                                "world",
                                                crate::static_ray::along(
                                                    pellet.origin,
                                                    pellet.direction,
                                                    distance,
                                                ),
                                            ),
                                            None => (
                                                "none",
                                                crate::static_ray::along(
                                                    pellet.origin,
                                                    pellet.direction,
                                                    pellet.reach,
                                                ),
                                            ),
                                        };
                                        recorded_rays.push(crate::game_debug::DebugRay {
                                            tick,
                                            source: bot.entity,
                                            kind: "hitscan",
                                            origin: pellet.origin,
                                            end,
                                            hit,
                                            target: None,
                                            critical: false,
                                            script: None,
                                        });
                                    }
                                }
                                Err(_) => {}
                            }
                        }
                    }
                }
                self.scene_messages
                    .extend(enemy.scripts.take_scene_messages());
                enemy_damage.extend(enemy.scripts.take_scripted_damage());
                if vitals.death().is_some() && !bot.dead {
                    bot.died(tick);
                    enemy.scripts.clear_pending_script_motion();
                }
                if let Some(health) = vitals.changed_component() {
                    self.scripted_health.push(EntityUpdate {
                        entity: bot.entity,
                        operation: Some(Operation::Update(vec![health])),
                        ..Default::default()
                    });
                    vitals.clear_changed();
                }
                if enemy.fault.is_none() {
                    if let Err(error) =
                        enemy
                            .scripts
                            .queue_common_corpse_retirement(&enemy.binding, vitals, tick)
                    {
                        enemy.halt(bot, error);
                    }
                }
                if enemy.fault.is_none() && enemy.scripts.retirement_requested {
                    if let Err(error) =
                        enemy
                            .scripts
                            .teardown(&enemy.graphs, &enemy.binding, vitals)
                    {
                        enemy.halt(bot, error);
                    } else {
                        retired.push(bot.entity);
                        self.scene_messages
                            .extend(enemy.scripts.take_scene_messages());
                    }
                }
            }
        }
        #[cfg(any(test, feature = "inspector"))]
        for ray in recorded_rays {
            self.record_debug_ray(ray);
        }
        self.collect_local_health_events();
        // +80 voice callbacks are a separate phase after all ordinary actor updates.
        // Select through read-only facts before taking a mutable actor reference.
        let voiced =
            self.states()
                .try_fold(Vec::new(), |mut bodies, state| -> Result<_, CombatError> {
                    let state = state?;
                    if let Some(enemy) = state.enemy.as_ref() {
                        if enemy.fault.is_none() && enemy.scripts.has_polled_voice() {
                            bodies.push(state.bot.entity);
                        }
                    }
                    Ok(bodies)
                })?;
        for body in voiced {
            let BotState {
                bot, vitals, enemy, ..
            } = self.state_mut(body)?.expect("checked voiced body");
            if let Some(enemy) = enemy.as_mut().filter(|enemy| enemy.fault.is_none()) {
                if let Err(error) = enemy
                    .scripts
                    .poll_voice(&enemy.graphs, &enemy.binding, vitals)
                {
                    bot.stop_objective(tick);
                    enemy.halt(bot, error);
                }
            }
        }
        self.collect_local_health_events();
        self.advance_enemy_projectiles(
            queries,
            roster.as_deref().map_or(&[], Vec::as_slice),
            tick,
            &mut enemy_damage,
        );
        self.spawn_stock_shots(stock_launches);
        self.enemy_projectiles.extend(
            launched
                .into_iter()
                .map(|projectile| self.world.spawn(projectile).id()),
        );
        for hit in enemy_damage {
            if !self.settle_health(
                hit.target,
                HealthEffect::Damage(hit.amount),
                Death {
                    time: now.as_secs_f64(),
                    frame: tick,
                    killer: hit.source,
                },
                now,
            )? {
                self.external_enemy_damage.push(hit);
            }
        }
        self.settle_authored_requests(tick, now)?;
        for wire in retired {
            let entity = self.by_wire.shift_remove(&wire).expect("retired NPC body");
            let enemy = self
                .world
                .get::<BotState>(entity)
                .unwrap()
                .enemy
                .as_ref()
                .unwrap();
            let identities = [enemy.binding.body, enemy.binding.controller];
            Arc::make_mut(&mut self.retired_entity_ids).extend(identities);
            self.world.despawn(entity);
            self.retired_enemies.push(wire);
        }
        if witch_present || !self.witch_staff_links.is_empty() {
            self.sync_witch_staff_links()?;
            self.collect_local_health_events();
        }
        self.settle_environment(tick, now)?;
        let replacements = self.respawn_due(tick, now);
        self.record_poses(queries, tick)?;
        let replacements = replacements?;
        self.flush_health_events();
        Ok(replacements)
    }

    fn sync_witch_staff_links(&mut self) -> Result<(), CombatError> {
        use crate::{script_enemy::WitchStaffSource, vitals::PoolKind};
        let pools = [Some(PoolKind::Health), Some(PoolKind::Armour)];
        let desired = self
            .checked_states()?
            .into_iter()
            .filter_map(|state| {
                let enemy = state.enemy.as_ref()?;
                if state.bot.dead || enemy.fault.is_some() || enemy.binding.hero != WITCH {
                    return None;
                }
                let (target, amount) = enemy.scripts.witch_staff_effect()?;
                let source = WitchStaffSource {
                    controller: enemy.binding.controller,
                    name: state.bot.spawn.name.clone(),
                    health: pools.map(|kind| state.vitals.amount(kind, false)),
                    capacity: pools.map(|kind| state.vitals.amount(kind, true)),
                };
                Some((state.bot.entity, (target, source, amount)))
            })
            .collect::<IndexMap<_, _>>();
        let mut links = std::mem::take(&mut self.witch_staff_links);
        let stale = links
            .extract_if(.., |source, (target, _)| {
                desired
                    .get(source)
                    .is_none_or(|(current, _, _)| current != target)
            })
            .collect::<Vec<_>>();
        for (source, (target, instance)) in stale {
            if let Some(state) = self.state_mut(target)? {
                if let Some(enemy) = state.enemy.as_mut().filter(|enemy| enemy.fault.is_none()) {
                    if let Err(error) = enemy.scripts.sync_witch_staff(
                        &enemy.graphs,
                        &enemy.binding,
                        &mut state.vitals,
                        Some((source, instance)),
                        None,
                    ) {
                        enemy.halt(&mut state.bot, error);
                    }
                }
            }
        }
        for (source, (target, caster, amount)) in desired {
            if let Some(state) = self.state_mut(target)?.filter(|state| !state.bot.dead) {
                if let Some(enemy) = state.enemy.as_mut().filter(|enemy| enemy.fault.is_none()) {
                    enemy.scripts.record_witch_staff_source(source, caster);
                    if links.contains_key(&source) {
                        continue;
                    }
                    match enemy.scripts.sync_witch_staff(
                        &enemy.graphs,
                        &enemy.binding,
                        &mut state.vitals,
                        None,
                        Some((source, amount)),
                    ) {
                        Ok(Some(instance)) => {
                            links.insert(source, (target, instance));
                        }
                        Ok(None) => (),
                        Err(error) => {
                            enemy.halt(&mut state.bot, error);
                        }
                    }
                }
            }
        }
        self.witch_staff_links = links;
        Ok(())
    }

    fn advance_enemy_projectiles(
        &mut self,
        queries: &BotQueries<'_>,
        roster: &[crate::script_enemy::EnemyTarget],
        tick: u32,
        damage: &mut Vec<crate::script_enemy::EnemyDamage>,
    ) {
        use crate::script_enemy::{EnemyDamage, EnemyProjectile};

        let mut children = Vec::new();
        for handle in std::mem::take(&mut self.enemy_projectiles) {
            let mut projectile = self
                .world
                .get_mut::<EnemyProjectile>(handle)
                .expect("live enemy projectile");
            let targets: Vec<_> = roster
                .iter()
                .filter(|target| target.alive && target.team != projectile.team)
                .map(|target| (target.entity, target.pose))
                .collect();
            match projectile
                .flight
                .advance(tick, &targets, |origin, direction, reach| {
                    queries.hitscan_wall(origin, direction, reach)
                }) {
                Ok(None) => {
                    self.enemy_projectiles.push(handle);
                    continue;
                }
                Ok(Some(end)) => {
                    let source = projectile.source;
                    let team = projectile.team;
                    if end.expired {
                        if let Some(split) = projectile.split.take() {
                            children.extend(split.launch(source, team, end.point, tick));
                        }
                    }
                    let tags = projectile.damage_tags.clone();
                    let shooter = self
                        .by_wire
                        .get(&source)
                        .and_then(|&body| self.world.get::<BotState>(body))
                        .and_then(|state| state.enemy.as_ref());
                    damage.extend(end.impacts.into_iter().map(|impact| EnemyDamage {
                        source,
                        target: impact.entity,
                        amount: shooter.map_or(impact.damage, |enemy| {
                            enemy.scripts.outgoing_damage(impact.damage, &tags)
                        }),
                    }));
                }
                Err(_) => {}
            }
            self.complete_observed_projectile(handle, tick);
            self.world.despawn(handle);
        }
        self.enemy_projectiles.extend(
            children
                .into_iter()
                .map(|projectile| self.world.spawn(projectile).id()),
        );
    }

    fn respawn_due(
        &mut self,
        tick: u32,
        now: Duration,
    ) -> Result<Vec<BotReplacement>, CombatError> {
        // Validate the entire batch before retiring any entity handles.
        let mut prepared = Vec::new();
        let mut next_bot = self.next_bot;
        for (slot, (&wire, &previous)) in self.by_wire.iter().enumerate() {
            let state = self
                .world
                .get::<BotState>(previous)
                .ok_or(CombatError::MissingBot {
                    wire,
                    entity: previous,
                })?;
            check_bot_identity(wire, state)?;
            if !state.respawn_at.is_some_and(|deadline| deadline <= now) {
                continue;
            }
            if self.by_wire.contains_key(&next_bot) {
                return Err(CombatError::BotIdInUse(next_bot));
            }
            let fresh = state
                .bot
                .respawned(next_bot, tick.saturating_add(BOT_RECORD_DELAY));
            next_bot = next_bot
                .checked_add(1)
                .ok_or(CombatError::BotIdsExhausted)?;
            let created =
                bot_create(&fresh, None).map_err(|source| CombatError::RespawnCreate {
                    wire: fresh.entity,
                    source,
                })?;
            prepared.push((
                slot,
                previous,
                fresh,
                BotReplacement {
                    retired: state.bot.entity,
                    created,
                },
            ));
        }
        let mut replacements = Vec::with_capacity(prepared.len());
        for (slot, previous, fresh, replacement) in prepared {
            let fresh_wire = fresh.entity;
            if !self.world.despawn(previous) {
                return Err(CombatError::MissingBot {
                    wire: replacement.retired,
                    entity: previous,
                });
            }
            let mut next_state = BotState::new(fresh, Some(tick));
            if let Some(catalog) = &self.stock_bots {
                next_state.stock_script = stock_bot::StockBotScript::start(
                    catalog,
                    &next_state.bot,
                    tick,
                    stock_bot::BodyEntry::Respawn,
                );
            }
            let entity = self.world.spawn(next_state).id();
            self.by_wire
                .replace_index(slot, fresh_wire)
                .map_err(|_| CombatError::BotIdInUse(fresh_wire))?;
            *self
                .by_wire
                .get_index_mut(slot)
                .ok_or(CombatError::MissingBotSlot(slot))?
                .1 = entity;
            #[cfg(feature = "inspector")]
            crate::owner_trace::playtest(format_args!(
                "bot-respawn event=created tick={tick} old={:08x} new={fresh_wire:08x}",
                replacement.retired
            ));
            replacements.push(replacement);
        }
        self.next_bot = next_bot;
        Ok(replacements)
    }

    #[cfg(test)]
    pub(crate) fn invalidate_bot_component(&mut self) -> bool {
        let Some(&entity) = self.by_wire.values().next() else {
            return false;
        };
        let Ok(mut entity) = self.world.get_entity_mut(entity) else {
            return false;
        };
        entity.take::<BotState>().is_some()
    }

    #[cfg(test)]
    pub(crate) fn reset_test_bodies(&mut self) {
        for entity in self.by_wire.drain(..).map(|(_, entity)| entity) {
            self.world.despawn(entity);
        }
        self.clear_players();
        self.clear_projectiles();
        for entity in self.enemy_projectiles.drain(..) {
            self.world.despawn(entity);
        }
        self.projectile_reports = Default::default();
        self.player_generations.clear();
        self.latest_wire_generations.clear();
        self.next_bot = BOT_ENTITY;
        self.combat_active = false;
        self.player_targets_active = false;
        self.cassidy_protected = false;
        self.created.clear();
        self.player_events.clear();
        self.player_deaths.clear();
        self.observer_player_health.clear();
        self.scene_messages.clear();
        self.external_targets = None;
        self.external_enemy_damage.clear();
        self.scripted_health.clear();
        self.retired_enemies.clear();
        self.retired_entity_ids = Arc::default();
        self.defender_messages.clear();
        self.mode_voice_requests.clear();
        self.player_cues.clear();
        self.witch_staff_links.clear();
        #[cfg(any(test, feature = "inspector"))]
        {
            self.debug_rays.clear();
        }
        self.debug_assert_encounter_invariants();
    }
}

fn bot_health_update(state: &mut BotState, killed: bool) -> EntityUpdate {
    let mut components = vec![state.vitals.component()];
    state.vitals.clear_changed();
    if killed {
        if let Ok(filter) = crate::match_scene::bot_filter_component(state.bot.spawn.team, true) {
            components.push(filter);
        }
    }
    EntityUpdate {
        entity: state.bot.entity,
        operation: Some(Operation::Update(components)),
        ..EntityUpdate::default()
    }
}

fn check_bot_identity(wire: u32, state: &BotState) -> Result<(), CombatError> {
    if state.bot.entity != wire {
        return Err(CombatError::BotIdentity {
            wire,
            actual: state.bot.entity,
        });
    }
    Ok(())
}

fn eligible_target(
    entity: u32,
    alive: bool,
    target_team: u8,
    source: u32,
    source_team: u8,
    free_for_all: bool,
) -> bool {
    alive && entity != source && (free_for_all || target_team != source_team)
}

fn eligible_bot(bot: &Bot, source: u32, team: u8, free_for_all: bool) -> bool {
    eligible_target(
        bot.entity,
        !bot.dead,
        bot.spawn.team,
        source,
        team,
        free_for_all,
    )
}

#[cfg(test)]
mod tests;
