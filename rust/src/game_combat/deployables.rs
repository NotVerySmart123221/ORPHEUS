//! Generic room-authoritative pets (`STUStatescriptStatePet`) and projectile
//! impact deployables (`ImpactSpawn` / `m_49CBA3BC`) driven directly by retail
//! 1.74 entity definitions and StateScript graphs.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use bevy_ecs::entity::Entity;
use serde_json::Value as Json;

use super::{RoomScriptRequest, RoomScriptRequests, RoomSimulation, RoomSpatialFrame, Stun, Vec3};
use crate::{
    game_body::yaw_rotation,
    game_projectiles::ImpactSpawn,
    match_scene::bot_filter_component,
    script_expr::{self, EmptyContext, EntityRelation, SpatialCandidate, valid_entity},
    script_graph::{GraphStore, asset_guid, graph_asset_index},
    script_init::{
        Component, MAX_INIT_WORK, PlayScript, RoomMessage, Root, Site, World as ScriptWorld,
    },
    script_wire::{Value, Variables},
    vitals::{Death, Vitals},
    world::{Create, EntityUpdate, GAME_MODE_ENTITY, Movement, Operation},
};

const FIRST_PET_WIRE: u32 = 0xc008_0000;
const PET_WIRE_SPAN: u32 = 0x0008_0000;
const FIRST_DEPLOYABLE_WIRE: u32 = 0xc010_0000;
const DEPLOYABLE_WIRE_SPAN: u32 = 0x0008_0000;
const FIRST_SERVER_PROJECTILE_WIRE: u32 = 0xc018_0000;
const SERVER_PROJECTILE_WIRE_SPAN: u32 = 0x0008_0000;
const KEEP_MOVEMENT_ROTATION: u32 = 0x0100_0000;
const MAX_DEPLOYABLES_PER_OWNER: usize = 16;
const SCRIPTLESS_DEPLOYABLE_TICKS: u32 = 62 * 15; // 15s default lifetime when no script expires it
const SCRIPTED_DEPLOYABLE_MAX_TICKS: u32 = 62 * 30; // 30s safety cap for scripted deployables
const SHARED_STUN_GRAPH: u32 = 0x224e;
const STUN_DURATION_VAR: u64 = 0x0d80_0000_0000_00d7;

static NEXT_PET_WIRE: AtomicU32 = AtomicU32::new(0);
static NEXT_DEPLOYABLE_WIRE: AtomicU32 = AtomicU32::new(0);
static NEXT_SERVER_PROJECTILE_WIRE: AtomicU32 = AtomicU32::new(0);

pub(crate) fn next_pet_wire_id() -> u32 {
    let seq = NEXT_PET_WIRE.fetch_add(1, Ordering::Relaxed) % PET_WIRE_SPAN;
    FIRST_PET_WIRE | seq
}

fn next_deployable_wire_id() -> u32 {
    let seq = NEXT_DEPLOYABLE_WIRE.fetch_add(1, Ordering::Relaxed) % DEPLOYABLE_WIRE_SPAN;
    FIRST_DEPLOYABLE_WIRE | seq
}

pub(super) fn next_server_projectile_wire_id() -> u32 {
    let seq =
        NEXT_SERVER_PROJECTILE_WIRE.fetch_add(1, Ordering::Relaxed) % SERVER_PROJECTILE_WIRE_SPAN;
    FIRST_SERVER_PROJECTILE_WIRE | seq
}

/// Extract an authored stun duration in simulation ticks from a `PlayScript`
/// invocation targeting the retail shared stun control graph (`0x224E`).
pub(crate) fn extract_play_script_stun_ticks(
    graphs: &GraphStore,
    request: &PlayScript,
) -> Option<u32> {
    if request.graph != SHARED_STUN_GRAPH {
        return None;
    }
    let override_seconds = request
        .overrides
        .iter()
        .find(|(id, _)| *id == STUN_DURATION_VAR)
        .and_then(|(_, val)| match *val {
            Value::Float(v) => Some(v),
            Value::Integer(v) => Some(v as f64),
            _ => None,
        });
    let seconds = override_seconds.or_else(|| {
        let entries = graphs
            .graph(SHARED_STUN_GRAPH)?
            .fields
            .get("m_publicSchema")?
            .get("m_entries")?
            .as_array()?;
        let row = entries
            .iter()
            .find(|row| row.get("m_0D09D2D9").and_then(asset_guid) == Some(STUN_DURATION_VAR))?;
        let val = script_expr::evaluate(row.get("m_value")?, &mut EmptyContext).ok()?;
        match val {
            Value::Float(v) => Some(v),
            Value::Integer(v) => Some(v as f64),
            _ => None,
        }
    })?;
    Stun::from_seconds(seconds).map(|stun| stun.ticks)
}

pub(super) struct RoomPet {
    pub(super) owner: u32,
    pub(super) ecs_entity: Entity,
    pub(super) team: u8,
    #[allow(dead_code)]
    pub(super) definition: u64,
    pub(super) attach: Option<u32>,
    pub(super) position: Vec3,
    pub(super) yaw: i32,
    pub(super) vitals: Option<Vitals>,
    pub(super) component: Option<Component>,
    pub(super) world: Option<DeployableWorld>,
}

fn resolve_entity_vitals(
    graphs: Option<&GraphStore>,
    definition: u64,
    max_health: Option<f64>,
) -> Option<Vitals> {
    if let Some(pools) = graphs
        .and_then(|g| g.entity_body(definition))
        .and_then(|body| {
            crate::game_pools::authored_pools(graphs?, body)
                .ok()
                .flatten()
        })
        .filter(|pools| pools.total() > 0.0)
    {
        return Some(pools.vitals());
    }
    max_health
        .filter(|hp| hp.is_finite() && *hp > 0.0)
        .map(|hp| Vitals::new(hp, 0.0, 0.0))
}

#[allow(clippy::too_many_arguments)]
fn start_entity_vm(
    graphs: Arc<GraphStore>,
    mode: u64,
    entity: u32,
    owner: u32,
    team: u8,
    definition: u64,
    position: Vec3,
    tick: u32,
    spatial_frame: Option<Arc<RoomSpatialFrame>>,
    extra_graph: Option<u64>,
) -> (Option<Component>, Option<DeployableWorld>) {
    let mut graph_ids = Vec::new();
    if let Some(body) = graphs.entity_body(definition) {
        for binding in &body.graphs {
            if let Some(gid) = binding.graph {
                if graphs.graph(gid).is_some() && !graph_ids.contains(&gid) {
                    graph_ids.push(gid);
                }
            }
        }
    }
    if let Some(extra) = extra_graph.and_then(graph_asset_index) {
        if graphs.graph(extra).is_some() && !graph_ids.contains(&extra) {
            graph_ids.push(extra);
        }
    }
    if graph_ids.is_empty() {
        return (None, None);
    }
    let roots: Vec<Root> = graph_ids
        .into_iter()
        .enumerate()
        .map(|(idx, gid)| Root::new((idx + 1) as u16, gid))
        .collect();
    let mut world = DeployableWorld {
        graphs: Arc::clone(&graphs),
        mode,
        entity,
        owner,
        team,
        definition,
        position,
        tick,
        spatial_frame,
        script_requests: RoomScriptRequests::default(),
    };
    match Component::start_with_public_schema(
        graphs,
        entity,
        Variables::new(),
        roots,
        tick,
        &mut world,
        MAX_INIT_WORK,
    ) {
        Ok(started) => (Some(started), Some(world)),
        Err(_) => (None, None),
    }
}

#[allow(clippy::too_many_arguments)]
fn entity_create_update(
    wire: u32,
    team: u8,
    definition: u64,
    attach: Option<u32>,
    position: Vec3,
    yaw: i32,
    local_offset: Option<Vec3>,
    vitals: Option<&Vitals>,
) -> Option<EntityUpdate> {
    let team_comp = bot_filter_component(team, false).ok()?;
    let mut components = vec![team_comp];
    if let Some(vitals) = vitals {
        components.push(vitals.component());
    }
    let mut create = Create::new(definition, components);
    if let Some(parent) = attach {
        create.movement_parent = Some(parent);
        create.movement = Some(Movement {
            flags: KEEP_MOVEMENT_ROTATION,
            position: local_offset.unwrap_or([0.0; 3]),
            ..Movement::default()
        });
    } else {
        create.position = position.map(|v| v as f32);
        create.rotation = yaw_rotation(f64::from(yaw));
    }
    Some(EntityUpdate {
        entity: wire,
        operation: Some(Operation::Create(Box::new(create))),
        ..EntityUpdate::default()
    })
}

pub(super) fn server_projectile_create_update(
    wire: u32,
    team: u8,
    definition: u64,
    position: Vec3,
    velocity: Vec3,
) -> Option<EntityUpdate> {
    let team_comp = bot_filter_component(team, false).ok()?;
    let mut create = Create::new(definition, vec![team_comp]);
    let yaw = velocity[0].atan2(velocity[2]).to_degrees();
    let yaw_i32 = (yaw * 65536.0 / 360.0).round_ties_even() as i32;
    create.position = position.map(|v| v as f32);
    create.rotation = yaw_rotation(f64::from(yaw_i32));
    create.movement = Some(Movement {
        position,
        velocity,
        yaw: yaw_i32,
        ..Movement::default()
    });
    Some(EntityUpdate {
        entity: wire,
        operation: Some(Operation::Create(Box::new(create))),
        ..EntityUpdate::default()
    })
}

pub(super) struct DeployableWorld {
    graphs: Arc<GraphStore>,
    mode: u64,
    entity: u32,
    owner: u32,
    team: u8,
    definition: u64,
    position: Vec3,
    tick: u32,
    spatial_frame: Option<Arc<RoomSpatialFrame>>,
    script_requests: RoomScriptRequests,
}

impl ScriptWorld for DeployableWorld {
    fn admits_generic_player_runtime(&self, site: Site) -> bool {
        site.entity == self.entity
    }

    fn accepts_client_self_message(&self, site: Site, topic: u64) -> bool {
        site.entity == self.entity && !self.graphs.has_server_message_receiver(topic)
    }

    fn entity_variable(&mut self, entity: u32, id: u32) -> Option<Value> {
        if entity == GAME_MODE_ENTITY {
            return self
                .graphs
                .mode_variables(self.mode)
                .map(|variables| variables.get(&id).copied().map_or(Value::Null, Value::from));
        }
        None
    }

    fn known_position(&self, entity: u32) -> Option<[f64; 3]> {
        (entity == self.entity).then_some(self.position)
    }

    fn position(&mut self, entity: u32) -> Option<[f64; 3]> {
        if entity == self.entity {
            return Some(self.position);
        }
        self.spatial_frame
            .as_deref()?
            .actor_pose(entity)
            .map(|(pos, _)| pos)
    }

    fn entity_exists(&self, entity: u32) -> Option<bool> {
        if entity == self.entity {
            return Some(true);
        }
        if entity == GAME_MODE_ENTITY {
            return Some(self.graphs.mode_variables(self.mode).is_some());
        }
        if valid_entity(entity)
            && self
                .spatial_frame
                .as_deref()
                .is_some_and(|frame| frame.candidates().iter().any(|c| c.entity == entity))
        {
            return Some(true);
        }
        None
    }

    fn entity_alive(&self, site: Site, entity: u32) -> bool {
        if site.entity != self.entity {
            return false;
        }
        entity == self.entity
            || self
                .spatial_frame
                .as_deref()
                .is_some_and(|frame| frame.candidates().iter().any(|c| c.entity == entity))
    }

    fn entity_team(&self, site: Site, entity: u32) -> Option<i8> {
        if site.entity != self.entity {
            return None;
        }
        if entity == self.entity {
            return i8::try_from(self.team).ok();
        }
        self.spatial_frame
            .as_deref()?
            .candidates()
            .iter()
            .find(|c| c.entity == entity)
            .and_then(|c| c.team)
            .and_then(|team| i8::try_from(team).ok())
    }

    fn entity_definition(&self, site: Site, entity: u32) -> Option<u64> {
        (site.entity == self.entity && entity == self.entity).then_some(self.definition)
    }

    fn related_entity(&mut self, site: Site, relation: EntityRelation, entity: u32) -> Option<u32> {
        if site.entity != self.entity || entity != self.entity {
            return None;
        }
        match relation {
            EntityRelation::CreatorBody | EntityRelation::OwningController => Some(self.owner),
            _ => None,
        }
    }

    fn spatial_candidates(&self) -> Option<&[SpatialCandidate]> {
        self.spatial_frame
            .as_deref()
            .map(RoomSpatialFrame::candidates)
    }

    fn spatial_line_clear(&self, origin: [f64; 3], target: [f64; 3]) -> Option<bool> {
        self.spatial_frame.as_deref()?.line_clear(origin, target)
    }

    fn scene_filter_policy(
        &self,
        site: Site,
        filter: &Json,
    ) -> Option<crate::script_expr::SceneFilterPolicy> {
        use crate::script_expr::{FilterPredicate, FilterRelation, SceneFilterPolicy};
        if site.entity != self.entity || self.spatial_frame.is_none() {
            return None;
        }
        let team_mode = filter.get("m_66747E7D").and_then(Json::as_u64).unwrap_or(0);
        let hostile_flag = filter.get("m_89831602").and_then(Json::as_u64).unwrap_or(0);
        let relation = match (team_mode, hostile_flag) {
            (1, 0) => Some(FilterRelation::Friendly),
            (2, 0) | (0, 1) => Some(FilterRelation::Hostile),
            (0, 0) => Some(FilterRelation::Any),
            _ => None,
        }?;
        let predicate = if filter.get("m_condition").is_none_or(Json::is_null) {
            FilterPredicate::None
        } else {
            FilterPredicate::Target
        };
        Some(SceneFilterPolicy {
            relation,
            predicate,
        })
    }

    fn tracked_targets(
        &mut self,
        site: Site,
        origin: u32,
        offset: [f64; 3],
        radius: f64,
        filter: &Json,
    ) -> Option<Vec<u32>> {
        if site.entity != self.entity || !radius.is_finite() || radius < 0.0 {
            return None;
        }
        let frame = self.spatial_frame.as_deref()?;
        let base = if origin == self.entity {
            self.position
        } else {
            frame
                .actor_pose(origin)
                .map(|(p, _)| p)
                .unwrap_or(self.position)
        };
        let center = [
            base[0] + offset[0],
            base[1] + offset[1],
            base[2] + offset[2],
        ];
        let team_mode = filter.get("m_66747E7D").and_then(Json::as_u64).unwrap_or(0);
        let hostile_flag = filter.get("m_89831602").and_then(Json::as_u64).unwrap_or(0);
        let relation = match (team_mode, hostile_flag) {
            (1, 0) => 1u64,
            (2, 0) | (0, 1) => 2u64,
            _ => 0u64,
        };
        let mut hits = Vec::new();
        for candidate in frame.candidates() {
            let team_ok = match relation {
                1 => candidate.team == Some(self.team),
                2 => candidate.team != Some(self.team),
                _ => true,
            };
            if !team_ok {
                continue;
            }
            let cap_r = 0.45;
            let cap_a = [
                candidate.position[0],
                candidate.position[1] + cap_r,
                candidate.position[2],
            ];
            let cap_b = [
                candidate.position[0],
                candidate.position[1] + 1.45,
                candidate.position[2],
            ];
            if crate::static_ray::segment_distance(center, cap_a, cap_b) <= radius + cap_r {
                hits.push(candidate.entity);
            }
        }
        Some(hits)
    }

    fn queue_health_change(&mut self, site: Site, target: u32, delta: f64) -> bool {
        self.queue_modify_health(site, target, delta, None)
    }

    fn queue_health_changes(&mut self, site: Site, changes: &[(u32, f64)]) -> bool {
        if site.entity != self.entity {
            return false;
        }
        let mut any = false;
        for &(target, delta) in changes {
            any |= self.queue_modify_health(site, target, delta, None);
        }
        any
    }

    fn queue_modify_health(
        &mut self,
        site: Site,
        target: u32,
        delta: f64,
        impulse: Option<[f64; 3]>,
    ) -> bool {
        if site.entity != self.entity || !delta.is_finite() {
            return false;
        }
        let Some(frame) = self.spatial_frame.as_deref() else {
            return false;
        };
        if delta < 0.0 || (delta == 0.0 && impulse.is_some()) {
            self.script_requests.queue_contact_damage(
                frame,
                self.tick,
                (-delta).max(0.0),
                &[(target, impulse)],
            )
        } else if delta > 0.0 {
            self.script_requests.queue_health(frame, target, delta)
        } else {
            false
        }
    }

    fn queue_room_message(&mut self, site: Site, target: u32, message: RoomMessage) -> bool {
        site.entity == self.entity
            && self
                .spatial_frame
                .as_deref()
                .is_some_and(|frame| self.script_requests.queue_message(frame, target, message))
    }

    fn play_script(&mut self, site: Site, target: u32, request: PlayScript) -> bool {
        if site.entity != self.entity {
            return false;
        }
        if target != self.entity && valid_entity(target) {
            if let Some(ticks) = extract_play_script_stun_ticks(&self.graphs, &request) {
                if let Some(frame) = self.spatial_frame.as_deref() {
                    let _ = self.script_requests.queue_stun(frame, target, ticks);
                }
            }
        }
        self.graphs.is_client_only_graph(request.graph)
            || self.graphs.graph(request.graph).is_some()
    }

    fn native(&mut self, site: Site, config: &Json) -> Option<Value> {
        if site.entity != self.entity {
            return None;
        }
        match config.get("$")?.as_str()? {
            "STU_1FF67CA6" => Some(Value::Entity(self.entity)),
            "STU_37F20D24" => Some(Value::Asset(self.mode)),
            "STU_A3DEF368" if self.graphs.mode_variables(self.mode).is_some() => {
                Some(Value::Entity(GAME_MODE_ENTITY))
            }
            "STU_4A101787" | "STU_A77AC2AC" | "STU_D3EB8D87" => Some(Value::Bool(false)),
            _ => None,
        }
    }
}

pub(super) struct RoomDeployable {
    pub(super) owner: u32,
    pub(super) ecs_entity: Entity,
    pub(super) team: u8,
    #[allow(dead_code)]
    pub(super) definition: u64,
    pub(super) position: Vec3,
    pub(super) yaw: i32,
    pub(super) vitals: Option<Vitals>,
    spawned_tick: u32,
    max_ticks: u32,
    pub(super) component: Option<Component>,
    pub(super) world: Option<DeployableWorld>,
}

impl RoomDeployable {
    fn is_expired(&self, tick: u32) -> bool {
        if self.vitals.as_ref().is_some_and(|v| v.death().is_some()) {
            return true;
        }
        if tick.saturating_sub(self.spawned_tick) >= self.max_ticks {
            return true;
        }
        if let Some(component) = &self.component {
            match component.snapshot() {
                None => return true,
                Some(snapshot) => {
                    let any_active = snapshot
                        .instances
                        .values()
                        .any(|inst| inst.states.values().any(|state| state.active));
                    if !any_active && tick > self.spawned_tick.saturating_add(4) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

impl RoomSimulation {
    fn deployable_graphs(&self) -> Option<(Arc<GraphStore>, u64)> {
        self.script_graphs
            .as_ref()
            .map(|(g, m)| (Arc::clone(g), *m))
            .or_else(|| {
                self.stock_bots
                    .as_ref()
                    .map(|c| (Arc::clone(&c.graphs), c.mode))
            })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn spawn_room_pet(
        &mut self,
        owner: u32,
        team: u8,
        pet_id: u32,
        definition: u64,
        attach: Option<u32>,
        position: Vec3,
        yaw: i32,
        max_health: Option<f64>,
        local_offset: Option<Vec3>,
        tick: u32,
    ) {
        if self.room_pets.contains_key(&pet_id) {
            return;
        }
        let graphs_and_mode = self.deployable_graphs();
        let vitals = resolve_entity_vitals(
            graphs_and_mode.as_ref().map(|(g, _)| g.as_ref()),
            definition,
            max_health,
        );
        let Some(update) = entity_create_update(
            pet_id,
            team,
            definition,
            attach,
            position,
            yaw,
            local_offset,
            vitals.as_ref(),
        ) else {
            return;
        };
        let (component, world_opt) = if let Some((graphs, mode)) = graphs_and_mode {
            start_entity_vm(
                graphs,
                mode,
                pet_id,
                owner,
                team,
                definition,
                position,
                tick,
                self.spatial_frame.as_ref().map(|(_, f)| Arc::clone(f)),
                None,
            )
        } else {
            (None, None)
        };
        let ecs_entity = self.world.spawn_empty().id();
        self.room_pets.insert(
            pet_id,
            RoomPet {
                owner,
                ecs_entity,
                team,
                definition,
                attach,
                position,
                yaw,
                vitals,
                component,
                world: world_opt,
            },
        );
        self.created.push(update);
    }

    pub(super) fn despawn_room_pet(&mut self, pet_id: u32) {
        if let Some(pet) = self.room_pets.shift_remove(&pet_id) {
            let _ = self.world.despawn(pet.ecs_entity);
            self.retired_enemies.push(pet_id);
        }
    }

    pub(super) fn retire_owner_pets_and_deployables(&mut self, owner: u32) {
        let dead_pets: Vec<u32> = self
            .room_pets
            .iter()
            .filter(|(_, pet)| pet.owner == owner)
            .map(|(&id, _)| id)
            .collect();
        for pet_id in dead_pets {
            self.despawn_room_pet(pet_id);
        }
        let dead_deployables: Vec<u32> = self
            .room_deployables
            .iter()
            .filter(|(_, d)| d.owner == owner)
            .map(|(&id, _)| id)
            .collect();
        for id in dead_deployables {
            if let Some(dep) = self.room_deployables.shift_remove(&id) {
                let _ = self.world.despawn(dep.ecs_entity);
                self.retired_enemies.push(id);
            }
        }
    }

    pub(super) fn spawn_impact_deployable(
        &mut self,
        owner: u32,
        team: u8,
        spawn: ImpactSpawn,
        position: Vec3,
        yaw: i32,
        tick: u32,
    ) {
        // Bound active deployables per owner by retiring the oldest if needed.
        let owner_ids: Vec<u32> = self
            .room_deployables
            .iter()
            .filter(|(_, d)| d.owner == owner)
            .map(|(&id, _)| id)
            .collect();
        if owner_ids.len() >= MAX_DEPLOYABLES_PER_OWNER {
            if let Some(&oldest) = owner_ids.first() {
                if let Some(dep) = self.room_deployables.shift_remove(&oldest) {
                    let _ = self.world.despawn(dep.ecs_entity);
                    self.retired_enemies.push(oldest);
                }
            }
        }
        let entity = next_deployable_wire_id();
        let graphs_and_mode = self.deployable_graphs();
        let vitals = resolve_entity_vitals(
            graphs_and_mode.as_ref().map(|(g, _)| g.as_ref()),
            spawn.definition,
            None,
        );
        let Some(update) = entity_create_update(
            entity,
            team,
            spawn.definition,
            None,
            position,
            yaw,
            None,
            vitals.as_ref(),
        ) else {
            return;
        };
        let (component, world_opt) = if let Some((graphs, mode)) = graphs_and_mode {
            start_entity_vm(
                graphs,
                mode,
                entity,
                owner,
                team,
                spawn.definition,
                position,
                tick,
                self.spatial_frame.as_ref().map(|(_, f)| Arc::clone(f)),
                spawn.on_spawn_graph,
            )
        } else {
            (None, None)
        };
        let max_ticks = if component.is_some() {
            SCRIPTED_DEPLOYABLE_MAX_TICKS
        } else {
            SCRIPTLESS_DEPLOYABLE_TICKS
        };
        let ecs_entity = self.world.spawn_empty().id();
        self.room_deployables.insert(
            entity,
            RoomDeployable {
                owner,
                ecs_entity,
                team,
                definition: spawn.definition,
                position,
                yaw,
                vitals,
                spawned_tick: tick,
                max_ticks,
                component,
                world: world_opt,
            },
        );
        self.created.push(update);
    }

    pub(super) fn intercept_barrier_ray(
        &mut self,
        attacker_team: Option<u8>,
        ffa: bool,
        origin: Vec3,
        direction: Vec3,
        max_dist: f64,
        damage: f64,
        source: u32,
        tick: u32,
        now: Duration,
    ) -> Option<(u32, f64)> {
        if max_dist <= 0.0 || !max_dist.is_finite() {
            return None;
        }
        let mut best: Option<(u32, bool, f64)> = None;
        for (&wire, pet) in &self.room_pets {
            let Some(vitals) = pet.vitals.as_ref() else {
                continue;
            };
            if vitals.death().is_some() {
                continue;
            }
            if !ffa && attacker_team == Some(pet.team) {
                continue;
            }
            let (radius, height): (f64, f64) = if vitals.maximum() >= 250.0 {
                (1.65, 2.4)
            } else {
                (0.65, 1.4)
            };
            let bottom = [
                pet.position[0],
                pet.position[1] + radius.min(0.6),
                pet.position[2],
            ];
            let top = [pet.position[0], pet.position[1] + height, pet.position[2]];
            if let Some(dist) = crate::static_ray::capsule_entry_distance(
                origin, direction, max_dist, bottom, top, radius,
            ) {
                if best.is_none_or(|(_, _, prev)| dist < prev) {
                    best = Some((wire, true, dist));
                }
            }
        }
        for (&wire, dep) in &self.room_deployables {
            let Some(vitals) = dep.vitals.as_ref() else {
                continue;
            };
            if vitals.death().is_some() {
                continue;
            }
            if !ffa && attacker_team == Some(dep.team) {
                continue;
            }
            let (radius, height) = if vitals.maximum() >= 250.0 {
                (1.4, 2.2)
            } else {
                (0.55, 1.1)
            };
            let bottom = [dep.position[0], dep.position[1] + 0.3, dep.position[2]];
            let top = [dep.position[0], dep.position[1] + height, dep.position[2]];
            if let Some(dist) = crate::static_ray::capsule_entry_distance(
                origin, direction, max_dist, bottom, top, radius,
            ) {
                if best.is_none_or(|(_, _, prev)| dist < prev) {
                    best = Some((wire, false, dist));
                }
            }
        }
        let (wire, is_pet, dist) = best?;
        if damage > 0.0 {
            let death = Death {
                time: now.as_secs_f64(),
                frame: tick,
                killer: source,
            };
            if is_pet {
                if let Some(pet) = self.room_pets.get_mut(&wire) {
                    if let Some(vitals) = pet.vitals.as_mut() {
                        vitals.damage(damage, death);
                        self.scripted_health.push(EntityUpdate {
                            entity: wire,
                            operation: Some(Operation::Update(vec![vitals.component()])),
                            ..EntityUpdate::default()
                        });
                    }
                }
            } else if let Some(dep) = self.room_deployables.get_mut(&wire) {
                if let Some(vitals) = dep.vitals.as_mut() {
                    vitals.damage(damage, death);
                    self.scripted_health.push(EntityUpdate {
                        entity: wire,
                        operation: Some(Operation::Update(vec![vitals.component()])),
                        ..EntityUpdate::default()
                    });
                }
            }
        }
        Some((wire, dist))
    }

    pub(super) fn advance_deployables(&mut self, tick: u32) {
        self.step_status_effects(tick);
        let frame = self.spatial_frame.as_ref().map(|(_, f)| Arc::clone(f));

        // Step active room pets (barriers, turrets, walls, teleporters).
        if !self.room_pets.is_empty() {
            let mut dead_pets = Vec::new();
            let attached_poses: Vec<(u32, Vec3, i32)> = self
                .room_pets
                .iter()
                .filter_map(|(&id, pet)| {
                    let parent = pet.attach?;
                    let (pos, angles) = frame.as_deref()?.actor_pose(parent)?;
                    Some((id, pos, angles[0]))
                })
                .collect();
            for (id, pos, yaw) in attached_poses {
                if let Some(pet) = self.room_pets.get_mut(&id) {
                    pet.position = pos;
                    pet.yaw = yaw;
                }
            }
            for (&id, pet) in &mut self.room_pets {
                if let (Some(component), Some(world)) = (pet.component.as_mut(), pet.world.as_mut())
                {
                    world.tick = tick;
                    world.position = pet.position;
                    world.spatial_frame = frame.clone();
                    if component.advance_unposed(tick, false, world).is_err() {
                        pet.component = None;
                    }
                }
                if let Some(vitals) = pet.vitals.as_mut() {
                    if vitals.has_changed() {
                        vitals.clear_changed();
                        self.scripted_health.push(EntityUpdate {
                            entity: id,
                            operation: Some(Operation::Update(vec![vitals.component()])),
                            ..EntityUpdate::default()
                        });
                    }
                    if vitals.death().is_some() {
                        dead_pets.push(id);
                    }
                }
            }
            for id in dead_pets {
                self.despawn_room_pet(id);
            }
        }

        if self.room_deployables.is_empty() {
            return;
        }
        let mut expired = Vec::new();
        for (&id, deployable) in &mut self.room_deployables {
            if let (Some(component), Some(world)) =
                (deployable.component.as_mut(), deployable.world.as_mut())
            {
                world.tick = tick;
                world.spatial_frame = frame.clone();
                if component.advance_unposed(tick, false, world).is_err() {
                    deployable.component = None;
                }
            }
            if let Some(vitals) = deployable.vitals.as_mut() {
                if vitals.has_changed() {
                    vitals.clear_changed();
                    self.scripted_health.push(EntityUpdate {
                        entity: id,
                        operation: Some(Operation::Update(vec![vitals.component()])),
                        ..EntityUpdate::default()
                    });
                }
            }
            if deployable.is_expired(tick) {
                expired.push(id);
            }
        }
        for id in expired {
            if let Some(dep) = self.room_deployables.shift_remove(&id) {
                let _ = self.world.despawn(dep.ecs_entity);
                self.retired_enemies.push(id);
            }
        }
    }

    pub(super) fn drain_deployable_script_requests(
        &mut self,
    ) -> Vec<(u32, Vec<RoomScriptRequest>)> {
        let mut out = Vec::new();
        for pet in self.room_pets.values_mut() {
            if let Some(world) = pet.world.as_mut() {
                let reqs = world.script_requests.take();
                if !reqs.is_empty() {
                    out.push((pet.owner, reqs));
                }
            }
        }
        for deployable in self.room_deployables.values_mut() {
            if let Some(world) = deployable.world.as_mut() {
                let reqs = world.script_requests.take();
                if !reqs.is_empty() {
                    out.push((deployable.owner, reqs));
                }
            }
        }
        out
    }

    #[cfg(test)]
    pub(crate) fn active_room_pets(&self) -> usize {
        self.room_pets.len()
    }

    #[cfg(test)]
    pub(crate) fn active_room_deployables(&self) -> usize {
        self.room_deployables.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mover::VelocityChange,
        mover_modifiers::{Operation as ModOp, SPEED_SLOT},
        script_init::{PetSpawn, VelocityOp},
    };
    use serde_json::json;

    #[derive(Default)]
    struct StubWorld {
        owner: u32,
        spawned: Vec<(u32, PetSpawn)>,
        despawned: Vec<u32>,
        velocity_ops: Vec<(u32, Vec<VelocityOp>)>,
    }

    impl ScriptWorld for StubWorld {
        fn admits_generic_player_runtime(&self, site: Site) -> bool {
            site.entity == self.owner
        }

        fn entity_variable(&mut self, _entity: u32, _id: u32) -> Option<Value> {
            None
        }

        fn position(&mut self, entity: u32) -> Option<[f64; 3]> {
            (entity == self.owner).then_some([1.0, 2.0, 3.0])
        }

        fn spawn_pet(&mut self, site: Site, spawn: PetSpawn) -> Option<u32> {
            if site.entity != self.owner {
                return None;
            }
            let id = 0xc008_0042;
            self.spawned.push((id, spawn));
            Some(id)
        }

        fn despawn_pet(&mut self, site: Site, entity: u32) -> bool {
            if site.entity != self.owner {
                return false;
            }
            self.despawned.push(entity);
            true
        }

        fn apply_velocity_ops(&mut self, site: Site, frame: u32, ops: &[VelocityOp]) -> bool {
            if site.entity != self.owner {
                return false;
            }
            self.velocity_ops.push((frame, ops.to_vec()));
            true
        }
    }

    fn single_state_store(state_node: Json) -> Arc<GraphStore> {
        let bundle = json!({
            "version": 1,
            "bodies": {},
            "missing": [],
            "graphs": [{
                "index": 1,
                "items": 2,
                "states": [1],
                "entries": [0],
                "remote": [],
                "sync_vars": [],
                "containers": {},
                "unknown_nodes": [],
                "fields": {},
                "header": {
                    "m_nodesBitCount": 4,
                    "m_statesBitCount": 4,
                    "m_remoteSyncNodesBitCount": 4,
                    "m_syncVarsBitCount": 4
                },
                "nodes": [
                    {
                        "pos": 0, "uid": 0, "rid": 0, "remote": -1,
                        "class": "STUStatescriptEntry",
                        "state": null, "server_only": false, "client_only": false,
                        "fields": {
                            "m_outPlug": {
                                "$": "STUStatescriptOutputPlug",
                                "links": [[1, "m_beginPlug"]]
                            }
                        }
                    },
                    state_node
                ]
            }]
        });
        Arc::new(GraphStore::from_json(bundle.to_string().as_bytes()).unwrap())
    }

    #[test]
    fn pet_state_spawns_writes_output_variable_and_despawns_on_end() {
        let graphs = single_state_store(json!({
            "pos": 1, "uid": 1, "rid": 1, "remote": -1,
            "class": "STUStatescriptStatePet",
            "state": 0, "server_only": false, "client_only": false,
            "fields": {
                "m_2BBEEAB8": 1,
                "m_beginPlug": { "$": "STUStatescriptInputPlug", "links": [] },
                "m_entityDefinition": { "$": "STU_8556841E", "m_entityDef": "0x040000000000011A" },
                "m_AA26E7DD": { "$": "STU_B5A0CAF0" },
                "m_B3019338": { "$": "STUConfigVarDynamic", "m_60DB8F99": 0, "m_identifier": 38 }
            }
        }));
        let owner = 0xa000_0101;
        let mut world = StubWorld {
            owner,
            ..Default::default()
        };
        let mut component = Component::start(
            graphs,
            owner,
            Variables::new(),
            vec![Root::new(1, 1)],
            1,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        assert_eq!(world.spawned.len(), 1);
        assert_eq!(world.spawned[0].0, 0xc008_0042);
        assert_eq!(world.spawned[0].1.definition, 0x0400_0000_0000_011a);
        assert_eq!(world.spawned[0].1.attach, Some(owner));
        let snap = component.snapshot().unwrap();
        assert_eq!(
            snap.instances[&1].variables.get(&38),
            Some(&Value::Entity(0xc008_0042))
        );

        component.debug_end_state(1, 0, &mut world).unwrap();
        assert_eq!(world.despawned, vec![0xc008_0042]);
        let snap = component.snapshot().unwrap();
        assert_eq!(snap.instances[&1].variables.get(&38), Some(&Value::Null));
    }

    #[test]
    fn room_pet_and_impact_deployable_replicate_and_expire() {
        let mut sim = RoomSimulation::new(Vec::new()).unwrap();
        let owner = 0xa000_0101;
        let pet_id = next_pet_wire_id();
        sim.spawn_room_pet(
            owner,
            0,
            pet_id,
            0x0400_0000_0000_0223,
            None,
            [3.0, 1.0, -4.0],
            0,
            None,
            None,
            10,
        );
        assert_eq!(sim.active_room_pets(), 1);
        let created = sim.take_created();
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].entity, pet_id);

        sim.spawn_impact_deployable(
            owner,
            0,
            ImpactSpawn {
                definition: 0x0400_0000_0000_14a1,
                on_spawn_graph: None,
            },
            [5.0, 0.0, 2.0],
            0,
            10,
        );
        assert_eq!(sim.active_room_deployables(), 1);
        let dep_creates = sim.take_created();
        assert_eq!(dep_creates.len(), 1);
        let dep_wire = dep_creates[0].entity;

        // Advancing past scriptless TTL retires the deployable automatically.
        sim.advance_deployables(10 + SCRIPTLESS_DEPLOYABLE_TICKS);
        assert_eq!(sim.active_room_deployables(), 0);
        assert!(sim.take_retired_enemies().contains(&dep_wire));

        // Retiring the owner cleans up active pets cleanly.
        sim.retire_owner_pets_and_deployables(owner);
        assert_eq!(sim.active_room_pets(), 0);
        assert!(sim.take_retired_enemies().contains(&pet_id));
    }

    #[test]
    fn movement_mod_handles_forced_velocity_and_inline_mover_component() {
        let graphs = single_state_store(json!({
            "pos": 1, "uid": 1, "rid": 1, "remote": -1,
            "class": "STU_316CFEF2",
            "state": 0, "server_only": false, "client_only": false,
            "fields": {
                "m_2BBEEAB8": 1,
                "m_beginPlug": { "$": "STUStatescriptInputPlug", "links": [] },
                "m_BC5E91CF": [
                    {
                        "$": "STU_09F9B1FE",
                        "m_CFEC5F76": 1,
                        "m_9B7A63EA": {
                            "$": "STUConfigVarDynamic",
                            "m_60DB8F99": 1,
                            "m_identifier": 100
                        }
                    },
                    {
                        "$": "STU_ED2B22BD",
                        "m_9B7A63EA": {
                            "$": "STUCharacterMoverComponent",
                            "m_19452F15": 11.0,
                            "m_E631BA84": 7.0,
                            "m_1FF06061": 0
                        }
                    }
                ]
            }
        }));
        let owner = 0xa000_0101;
        let mut world = StubWorld {
            owner,
            ..Default::default()
        };
        let mut component = Component::start(
            graphs,
            owner,
            Variables::from([(100, Value::Vector([12.0, 99.0, -4.0]))]),
            vec![Root::new(1, 1)],
            1,
            &mut world,
            MAX_INIT_WORK,
        )
        .unwrap();
        assert_eq!(world.velocity_ops.len(), 1);
        assert_eq!(
            world.velocity_ops[0].1,
            vec![VelocityOp::SetHorizontalVector([12.0, -4.0])]
        );
        // Verify horizontal velocity change preserves Y component.
        let mut vel = [0.0, -7.5, 0.0];
        VelocityChange::SetHorizontalVelocity([12.0, -4.0]).apply(&mut vel);
        assert_eq!(vel, [12.0, -7.5, -4.0]);

        // Verify inline STUCharacterMoverComponent modifiers were published.
        let mods = component.movement_modifiers(&mut world).unwrap().unwrap();
        assert!(mods.values().any(|m| {
            m.index == SPEED_SLOT && m.operation == ModOp::Multiply && (m.value - 2.0).abs() < 1e-4
        }));
        assert!(mods.values().any(|m| {
            m.index == 2 && m.operation == ModOp::Multiply && (m.value - 2.0).abs() < 1e-4
        }));
        assert!(
            mods.values()
                .any(|m| m.index == 22 && m.operation == ModOp::Set && m.value == 0.0)
        );

        // Verify next tick re-applies continuous forced velocity automatically.
        component.advance_unposed(2, false, &mut world).unwrap();
        assert!(world.velocity_ops.len() >= 2);
    }

    #[test]
    fn play_script_stun_extracts_authored_duration_from_shared_control_graph() {
        let graphs =
            GraphStore::from_json(br#"{"version":1,"graphs":[],"bodies":{},"missing":[]}"#)
                .unwrap();
        let req = PlayScript {
            policy: 0,
            overrides: vec![(STUN_DURATION_VAR, Value::Float(0.8))],
            graph: SHARED_STUN_GRAPH,
            source: 0xa000_0101,
            parameters: vec![],
            auto_retire: false,
        };
        assert_eq!(extract_play_script_stun_ticks(&graphs, &req), Some(50));

        let other = PlayScript {
            graph: 0x1234,
            ..req
        };
        assert_eq!(extract_play_script_stun_ticks(&graphs, &other), None);
    }
}
