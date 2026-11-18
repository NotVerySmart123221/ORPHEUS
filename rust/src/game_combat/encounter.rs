//! Room-owned Junkenstein gate and encounter state inside `RoomSimulation`'s ECS world.

use std::{collections::BTreeSet, io, sync::Arc};

use bevy_ecs::{component::Component, entity::Entity};

use crate::{
    bits::BitWriter,
    collision_scene::CollisionScene,
    game_bots::BotWalk,
    game_combat::{CombatError, RoomSimulation},
    game_heroes::HeroCatalog,
    game_service::player_entity,
    junkenstein_messages::{DefenderMessage, POST_MATCH_PHASE},
    junkenstein_spawn::SpawnSearches,
    mover_settings::{MoverCatalog, MoverSettings},
    scene_assets::{JunkensteinGate, SpawnAnchors},
    script_codecs::CodecTable,
    script_expr::{EntityRelation, native_shape},
    script_graph::GraphStore,
    script_init::{self, InitFault, Site, World},
    script_snapshot_wire::{
        COUNTDOWN_STATES, COUNTDOWN_VAR, CountdownFrame, ProjectedSnapshot, SceneProjection,
        SnapshotEncoder, encounter_countdown_delta, encounter_message_delta, mode_action_delta,
    },
    script_wire::{Value, Variables},
    world::{Component as WireComponent, ComponentValue, GAME_MODE_ENTITY},
};

pub(crate) const ENCOUNTER_INSTANCE: u16 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GateDamageStage {
    Intact,
    Stage1,
    Stage2,
    Stage3,
    Destroyed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GateHealth {
    pub(crate) remaining: f64,
    pub(crate) consumed: f64,
    pub(crate) capacity: Option<f64>,
    pub(crate) stage: GateDamageStage,
    pub(crate) active_reaction: bool,
}

impl GateHealth {
    pub(crate) fn from_snapshot(
        snapshot: &script_init::Snapshot,
        graphs: &GraphStore,
        gate_graph: u32,
    ) -> Self {
        let remaining = snapshot
            .variables
            .get(&8571)
            .map_or(1.0, crate::script_expr::to_float);
        let consumed = snapshot
            .variables
            .get(&8569)
            .map_or(0.0, crate::script_expr::to_float);
        let capacity = snapshot
            .resolved_bindings
            .get(&(0, 7786))
            .map(crate::script_expr::to_float)
            .filter(|value| value.is_finite() && *value > 0.0);
        let instance = snapshot.instances.get(&1);
        let active_reaction = instance
            .and_then(|inst| inst.states.get(&19))
            .is_some_and(|state| state.active);
        let destroyed = instance
            .and_then(|inst| inst.states.get(&2))
            .is_some_and(|state| {
                state.active && state.kind == script_init::StateKind::BooleanSwitch(false)
            })
            || !remaining.is_finite()
            || remaining <= 0.0;
        let stage = if destroyed {
            GateDamageStage::Destroyed
        } else if let Some(stage) = instance.and_then(|inst| {
            let graph = graphs.graph(gate_graph)?;
            [
                (22, GateDamageStage::Stage3),
                (21, GateDamageStage::Stage2),
                (20, GateDamageStage::Stage1),
                (10, GateDamageStage::Intact),
            ]
            .into_iter()
            .find_map(|(pos, stage)| {
                let raw = graph.node(pos)?.state?;
                inst.states
                    .get(&raw)
                    .is_some_and(|state| state.active)
                    .then_some(stage)
            })
        }) {
            stage
        } else if remaining > 0.75 {
            GateDamageStage::Intact
        } else if remaining > 0.50 {
            GateDamageStage::Stage1
        } else if remaining > 0.25 {
            GateDamageStage::Stage2
        } else {
            GateDamageStage::Stage3
        };
        Self {
            remaining,
            consumed,
            capacity,
            stage,
            active_reaction,
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct GateState {
    pub(crate) team: Option<i8>,
    pub(crate) messages: Vec<crate::script_enemy::SceneMessage>,
    pub(crate) voice_requests: Vec<crate::junkenstein_voice::VoiceRequest>,
    pub(crate) voice_variation: u64,
}

#[derive(Clone)]
pub(crate) struct GateRuntime {
    pub(crate) graphs: Arc<GraphStore>,
    pub(crate) codecs: Option<Arc<CodecTable>>,
    pub(crate) script: script_init::Component,
    pub(crate) state: GateState,
    pub(crate) health: GateHealth,
}

#[derive(Component)]
pub(crate) struct GateComponent {
    pub(crate) definition: JunkensteinGate,
    pub(crate) anchors: Arc<SpawnAnchors>,
    pub(crate) runtime: Option<GateRuntime>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GateHandle {
    pub(crate) entity: Entity,
    pub(crate) wire: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EncounterPhase {
    PendingStart,
    Countdown,
    Active,
    PostMatch,
    Complete,
}

impl EncounterPhase {
    pub(crate) fn from_state(
        first_tick: Option<u32>,
        snapshot: Option<&script_init::Snapshot>,
        result_action: Option<Site>,
        screens_complete: bool,
    ) -> Self {
        if screens_complete {
            Self::Complete
        } else if result_action.is_some() {
            Self::PostMatch
        } else if first_tick.is_none() {
            Self::PendingStart
        } else if snapshot
            .and_then(|s| s.instances.get(&ENCOUNTER_INSTANCE))
            .is_some_and(|inst| {
                inst.states
                    .get(&COUNTDOWN_STATES[0])
                    .is_some_and(|s| s.active)
                    && inst
                        .states
                        .get(&COUNTDOWN_STATES[1])
                        .is_none_or(|s| s.activation == 0)
            })
        {
            Self::Countdown
        } else {
            Self::Active
        }
    }
}

#[derive(Component)]
pub(crate) struct EncounterComponent {
    pub(crate) voice_set: u64,
    pub(crate) graphs: Arc<GraphStore>,
    pub(crate) npcs: Option<Arc<crate::npc_catalog::NpcCatalog>>,
    pub(crate) heroes: Arc<HeroCatalog>,
    pub(crate) movers: Arc<MoverCatalog>,
    pub(crate) mode: u64,
    pub(crate) script: script_init::Component,
    pub(crate) codecs: Option<Arc<CodecTable>>,
    pub(crate) searches: SpawnSearches,
    pub(crate) team: u8,
    pub(crate) difficulty: u64,
    pub(crate) ready_message: u64,
    pub(crate) ready_parameter: u32,
    pub(crate) first_tick: Option<u32>,
    pub(crate) ready_hero: Option<u64>,
    pub(crate) result_action: Option<Site>,
    pub(crate) screens_complete: bool,
    pub(crate) phase: EncounterPhase,
    pub(crate) boss_pause: bool,
}

pub(crate) struct EncounterRegistration {
    pub(crate) voice_set: u64,
    pub(crate) graphs: Arc<GraphStore>,
    pub(crate) npcs: Option<Arc<crate::npc_catalog::NpcCatalog>>,
    pub(crate) heroes: Arc<HeroCatalog>,
    pub(crate) movers: Arc<MoverCatalog>,
    pub(crate) mode: u64,
    pub(crate) script: script_init::Component,
    pub(crate) codecs: Option<Arc<CodecTable>>,
    pub(crate) gate_script: script_init::Component,
    pub(crate) gate_state: GateState,
    pub(crate) team: u8,
    pub(crate) difficulty: u64,
    pub(crate) ready_message: u64,
    pub(crate) ready_parameter: u32,
    pub(crate) result_action: Option<Site>,
    pub(crate) screens_complete: bool,
}

pub(crate) struct EncounterWorld<'a> {
    pub(crate) gate: &'a JunkensteinGate,
    pub(crate) anchors: &'a SpawnAnchors,
    pub(crate) gate_variables: &'a Variables,
    pub(crate) bodies: &'a mut RoomSimulation,
    pub(crate) creator: Option<EnemyCreator<'a>>,
    pub(crate) result_action: &'a mut Option<Site>,
    pub(crate) screens_complete: &'a mut bool,
    pub(crate) team: u8,
    pub(crate) difficulty: u64,
}

pub(crate) struct EnemyCreator<'a> {
    pub(crate) graphs: &'a Arc<GraphStore>,
    pub(crate) codecs: &'a Arc<CodecTable>,
    pub(crate) catalog: Option<&'a crate::npc_catalog::NpcCatalog>,
    pub(crate) heroes: &'a HeroCatalog,
    pub(crate) movers: &'a MoverCatalog,
    pub(crate) mode: u64,
    pub(crate) mode_variables: Arc<Variables>,
    pub(crate) tick: u32,
}

impl World for EncounterWorld<'_> {
    fn engine_setting(&self, _site: Site, key: u32) -> crate::script_expr::EngineSetting {
        crate::junkenstein_host::engine_setting(key)
    }

    fn is_victory_voice(&self, node: &crate::script_graph::Node) -> bool {
        crate::junkenstein_voice::victory_cue(node)
    }

    fn play_victory_voice(&mut self, site: Site, actor: &Value) -> Option<u64> {
        use crate::junkenstein_voice::{VICTORY_CUE, VoiceRequest, victory_line};

        if (site.entity, site.graph, site.node) != (GAME_MODE_ENTITY, 0x0c8c, Some(221)) {
            return None;
        }
        let hero = match *actor {
            Value::Entity(entity) => self.bodies.encounter_enemy_hero(entity),
            Value::Asset(hero) => Some(hero),
            _ => None,
        };
        let (line, duration) = victory_line(hero);
        self.bodies.queue_mode_voice(VoiceRequest {
            anchor: GAME_MODE_ENTITY,
            speaker: GAME_MODE_ENTITY,
            stimulus: VICTORY_CUE,
            line,
        });
        Some(duration)
    }

    fn queue_mode_action(&mut self, site: Site) -> bool {
        if (site.entity, site.instance, site.graph)
            != (GAME_MODE_ENTITY, ENCOUNTER_INSTANCE, 0x0c8c)
        {
            return false;
        }
        self.result_action.get_or_insert(site);
        true
    }

    fn send_message(
        &mut self,
        _site: Site,
        target: u32,
        message: u64,
        parameters: &Variables,
    ) -> bool {
        if target != self.gate.entity {
            return false;
        }
        self.bodies
            .queue_scene_message(crate::script_enemy::SceneMessage {
                target,
                message,
                parameters: parameters.clone(),
            });
        true
    }

    fn route_game_message(&mut self, site: Site, message: u64, parameters: &Variables) -> bool {
        use crate::junkenstein_messages::{ENCOUNTER_RESULT, POST_MATCH_COMPLETE};

        if let Some(request) = DefenderMessage::victory_view(site, message, parameters) {
            return self.bodies.queue_defender_message(request);
        }

        if (site.entity, site.graph, site.node, message)
            == (GAME_MODE_ENTITY, 0x180a, Some(28), POST_MATCH_COMPLETE)
            && parameters.is_empty()
        {
            *self.screens_complete = true;
            return true;
        }

        if (site.entity, site.graph, site.node, message)
            == (GAME_MODE_ENTITY, 0x0c8c, Some(84), ENCOUNTER_RESULT)
        {
            if parameters.len() != 4
                || ![0x3ae, 0x3ad, 0x3b4, 0x3b5]
                    .into_iter()
                    .all(|id| parameters.contains_key(&id))
                || !self.bodies.queue_defender_message(DefenderMessage {
                    target: None,
                    source: site.entity,
                    message,
                    parameters: parameters.clone(),
                })
            {
                return false;
            }
            self.bodies
                .queue_scene_message(crate::script_enemy::SceneMessage {
                    target: self.gate.entity,
                    message,
                    parameters: parameters.clone(),
                });
            self.bodies.queue_enemy_result(parameters);
            return true;
        }
        site.entity == GAME_MODE_ENTITY
            && parameters.is_empty()
            && DefenderMessage::authored(site, message)
                .is_some_and(|message| self.bodies.queue_defender_message(message))
    }

    fn related_entity(&mut self, site: Site, relation: EntityRelation, entity: u32) -> Option<u32> {
        if (site.entity, site.graph, site.node, relation)
            != (
                GAME_MODE_ENTITY,
                0x0cba,
                Some(9),
                EntityRelation::CurrentPossessor,
            )
        {
            return None;
        }
        self.bodies.encounter_possessor(entity)
    }

    fn play_script(&mut self, site: Site, target: u32, request: script_init::PlayScript) -> bool {
        self.bodies.queue_encounter_wave(site, target, request)
    }

    fn create_entity(
        &mut self,
        site: Site,
        request: &script_init::EntityCreation,
    ) -> Result<Option<u32>, String> {
        if site.entity != GAME_MODE_ENTITY {
            return Err("unsupported scene entity creation site".into());
        }
        let creator = self
            .creator
            .as_ref()
            .ok_or("missing prepared enemy creation services")?;
        let script_init::CreationKind::Hero { hero, .. } = &request.kind else {
            return Err("encounter creation requires an authored hero".into());
        };
        let definition = crate::npc_catalog::encounter_definition(
            creator.catalog,
            creator.graphs,
            creator.heroes,
            *hero,
        )
        .map_err(|error| error.to_string())?;
        let mover = creator.movers.for_body(Some(definition.body));
        self.bodies
            .create_enemy(crate::script_enemy::EnemySpawn {
                graphs: Arc::clone(creator.graphs),
                codecs: Arc::clone(creator.codecs),
                definition: &definition,
                request,
                mode: creator.mode,
                mode_variables: Arc::clone(&creator.mode_variables),
                tick: creator.tick,
                walk: BotWalk::from_mover(
                    mover.run_forward,
                    mover.accel_ground,
                    mover.decel_ground_base,
                ),
                gate: Some(crate::script_enemy::EnemyGate {
                    entity: self.gate.entity,
                    position: self.gate.position,
                }),
            })
            .map(Some)
    }

    fn creation_position(&mut self, entity: u32) -> Option<[f64; 3]> {
        self.anchors.position(entity)
    }

    fn entity_variable(&mut self, entity: u32, id: u32) -> Option<Value> {
        if entity == self.gate.entity {
            return Some(self.gate_variables.get(&id).cloned().unwrap_or(Value::Null));
        }
        None
    }

    fn static_target(&self, tag: u64) -> Option<u32> {
        match self.anchors.targets(tag)? {
            &[entity] => Some(entity),
            _ => None,
        }
    }

    fn position(&mut self, entity: u32) -> Option<[f64; 3]> {
        self.anchors.position(entity)
    }

    fn forward(&self, entity: u32) -> Option<[f64; 3]> {
        self.anchors.forward(entity)
    }

    fn team_members(&self, team: u8) -> Option<u32> {
        (team < 5).then_some(u32::from(team == self.team))
    }

    fn encounter_body_alive(&self, entity: u32) -> Option<bool> {
        Some(self.bodies.encounter_body_alive(entity))
    }

    fn has_target_tag(&self, site: Site, entity: u32, tag: u64) -> Option<bool> {
        if site.entity != GAME_MODE_ENTITY
            || !matches!(
                (site.graph, site.node, tag),
                (0x0c8c, Some(59), 0x0d60_0000_0000_011e)
                    | (0x0c8c, Some(61), 0x0d60_0000_0000_00f6)
                    | (0x0c8c, Some(58), 0x0d60_0000_0000_0120)
                    | (0x0c8c, Some(57), 0x0d60_0000_0000_00fa)
            )
        {
            return None;
        }
        self.bodies.encounter_body_has_target_tag(entity, tag)
    }

    fn participant_targets(&self, site: Site, filter: &serde_json::Value) -> Option<Vec<u32>> {
        let filter = filter.as_object()?;
        let player_query = (site.graph, site.node) == (0x0c8c, Some(231));
        let enemy_query = (site.graph, site.node) == (0x0cba, Some(56));
        let masks = [
            ("m_84065FD1", if player_query { 0x40000 } else { 1 }),
            (
                "m_1E14CCA8",
                if player_query { 0x800000 } else { 0x1000000 },
            ),
            ("m_4EA686DB", 0x10),
        ];
        if site.entity != GAME_MODE_ENTITY
            || !(player_query || enemy_query)
            || self.team != 0
            || filter
                .get("m_B486603E")
                .is_some_and(|value| !value.is_null())
            || !masks.iter().all(|(key, value)| {
                filter.get(*key).and_then(serde_json::Value::as_u64) == Some(*value)
            })
            || filter.iter().any(|(key, value)| {
                key != "$"
                    && !masks.iter().any(|(mask, _)| key == mask)
                    && !value.is_null()
                    && value.as_u64() != Some(0)
            })
        {
            return None;
        }
        Some(if player_query {
            vec![player_entity(0)]
        } else {
            self.bodies
                .bodies()
                .ok()?
                .filter(|bot| bot.spawn.team == 1 && !bot.dead)
                .map(|bot| bot.entity)
                .collect()
        })
    }

    fn native(&mut self, site: Site, config: &serde_json::Value) -> Option<Value> {
        if native_shape(config, "STU_A3DEF368", None) {
            return Some(Value::Entity(GAME_MODE_ENTITY));
        }
        if site.entity == GAME_MODE_ENTITY
            && site.graph == 0x0cc0
            && config["$"] == "STU_B3E67A36"
            && crate::script_expr::native_fields(
                config,
                &["m_filter", "m_9C7BD8B8", "m_B486603E", "m_8B2A4DF3"],
            )
            && ["m_filter", "m_9C7BD8B8", "m_8B2A4DF3"]
                .iter()
                .all(|key| config[key].is_null())
        {
            let tag = crate::script_expr::target_tag_config(&config["m_B486603E"])?;
            if matches!(
                tag,
                0x0d60_0000_0000_00fa
                    | 0x0d60_0000_0000_00ff
                    | 0x0d60_0000_0000_011e
                    | 0x0d60_0000_0000_0120
            ) {
                let entity = self.anchors.targets(tag)?.first().copied().or_else(|| {
                    self.bodies
                        .bodies()
                        .ok()?
                        .map(|bot| bot.entity)
                        .find(|&body| {
                            self.bodies.encounter_body_has_target_tag(body, tag) == Some(true)
                        })
                });
                return Some(Value::Entity(entity.unwrap_or(0)));
            }
        }
        // Bind the queue input at the original difficulty assignment.
        (site.entity == GAME_MODE_ENTITY
            && site.graph == 0x0c8c
            && site.node == Some(263)
            && native_shape(config, "STU_7BD2CA19", Some("m_resourceKey"))
            && config
                .get("m_resourceKey")
                .is_some_and(serde_json::Value::is_null))
        .then_some(Value::Asset(self.difficulty))
    }
}

pub(crate) struct GateWorld<'a> {
    pub(crate) entity: u32,
    pub(crate) state: &'a mut GateState,
    pub(crate) bodies: &'a RoomSimulation,
    pub(crate) mode_variables: &'a Variables,
}

impl World for GateWorld<'_> {
    fn engine_setting(&self, _site: Site, key: u32) -> crate::script_expr::EngineSetting {
        crate::junkenstein_host::engine_setting(key)
    }

    fn native(&mut self, _site: Site, config: &serde_json::Value) -> Option<Value> {
        native_shape(config, "STU_A3DEF368", None).then_some(Value::Entity(GAME_MODE_ENTITY))
    }

    fn entity_variable(&mut self, entity: u32, id: u32) -> Option<Value> {
        if entity == GAME_MODE_ENTITY {
            Some(self.mode_variables.get(&id).cloned().unwrap_or(Value::Null))
        } else {
            None
        }
    }

    fn position(&mut self, _entity: u32) -> Option<[f64; 3]> {
        None
    }

    fn send_message(
        &mut self,
        _site: Site,
        target: u32,
        message: u64,
        parameters: &Variables,
    ) -> bool {
        if target != GAME_MODE_ENTITY {
            return false;
        }
        self.state.messages.push(crate::script_enemy::SceneMessage {
            target,
            message,
            parameters: parameters.clone(),
        });
        true
    }

    fn assign_team(&mut self, site: Site, entity: u32, team: i8) -> bool {
        if site.entity != self.entity || entity != self.entity {
            return false;
        }
        self.state.team = Some(team);
        true
    }

    fn gate_voice_cue(&self, node: &crate::script_graph::Node) -> Option<u64> {
        crate::junkenstein_voice::gate_cue(node)
    }

    fn play_gate_voice(&mut self, site: Site, stimulus: u64, attacker: Option<u32>) -> Option<u64> {
        use crate::junkenstein_voice::{VoiceRequest, gate_lines, select_line};

        if site.entity != self.entity
            || site.graph != 0x0ca7
            || self.state.voice_requests.len() >= script_init::MAX_INIT_WORK
        {
            return None;
        }
        let attacker_hero = attacker.and_then(|entity| self.bodies.encounter_enemy_hero(entity));
        let (line, duration_ms) = select_line(
            gate_lines(stimulus, attacker_hero),
            self.entity,
            &mut self.state.voice_variation,
        )?;
        self.state.voice_requests.push(VoiceRequest {
            anchor: self.entity,
            speaker: GAME_MODE_ENTITY,
            stimulus,
            line,
        });
        Some(duration_ms)
    }
}

impl RoomSimulation {
    pub(crate) fn register_junkenstein_gate(
        &mut self,
        gate: JunkensteinGate,
        anchors: Arc<SpawnAnchors>,
    ) -> Result<GateHandle, CombatError> {
        if self.gate_handle.is_some() {
            return Err(CombatError::GateAlreadyRegistered(gate.entity));
        }
        if gate.entity == 0 || gate.position.iter().any(|axis| !axis.is_finite()) {
            return Err(CombatError::InvalidGate(
                "gate entity or position is invalid",
            ));
        }
        let wire = gate.entity;
        let entity = self
            .world
            .spawn(GateComponent {
                definition: gate,
                anchors: Arc::clone(&anchors),
                runtime: None,
            })
            .id();
        let handle = GateHandle { entity, wire };
        self.gate_handle = Some(handle);
        self.spawn_anchors = Some(anchors);
        self.debug_assert_encounter_invariants();
        Ok(handle)
    }

    pub(crate) fn register_junkenstein_encounter(
        &mut self,
        registration: EncounterRegistration,
    ) -> Result<Entity, CombatError> {
        if self.encounter_entity.is_some() {
            return Err(CombatError::EncounterAlreadyRegistered);
        }
        let Some(handle) = self.gate_handle else {
            return Err(CombatError::MissingEncounterGate);
        };
        let gate_graph = {
            let gate =
                self.world
                    .get::<GateComponent>(handle.entity)
                    .ok_or(CombatError::MissingGate {
                        wire: handle.wire,
                        entity: handle.entity,
                    })?;
            if gate.definition.entity != handle.wire {
                return Err(CombatError::GateIdentity {
                    wire: handle.wire,
                    actual: gate.definition.entity,
                });
            }
            if gate.runtime.is_some() {
                return Err(CombatError::EncounterAlreadyRegistered);
            }
            gate.definition.graph
        };
        if registration.gate_state.team != Some(2) {
            return Err(CombatError::InvalidEncounter(
                "Junkenstein gate script did not initialize team 2",
            ));
        }
        let (health, phase, boss_pause) = {
            let gate_snapshot = registration
                .gate_script
                .snapshot()
                .filter(|snapshot| {
                    snapshot.initialization_complete() && snapshot.variables.contains_key(&8571)
                })
                .ok_or(CombatError::InvalidEncounter(
                    "Junkenstein gate script did not initialize its health",
                ))?;
            let mode_snapshot = registration
                .script
                .snapshot()
                .filter(|snapshot| snapshot.initialization_complete())
                .ok_or(CombatError::InvalidEncounter(
                    "Junkenstein encounter script did not complete initialization",
                ))?;
            (
                GateHealth::from_snapshot(gate_snapshot, &registration.graphs, gate_graph),
                EncounterPhase::from_state(
                    None,
                    Some(mode_snapshot),
                    registration.result_action,
                    registration.screens_complete,
                ),
                mode_snapshot.boss_pause_active(),
            )
        };
        let voice_set = registration.voice_set;
        let gate_runtime = GateRuntime {
            graphs: Arc::clone(&registration.graphs),
            codecs: registration.codecs.clone(),
            script: registration.gate_script,
            state: registration.gate_state,
            health,
        };
        let encounter = EncounterComponent {
            voice_set,
            graphs: registration.graphs,
            npcs: registration.npcs,
            heroes: registration.heroes,
            movers: registration.movers,
            mode: registration.mode,
            script: registration.script,
            codecs: registration.codecs,
            searches: SpawnSearches::default(),
            team: registration.team,
            difficulty: registration.difficulty,
            ready_message: registration.ready_message,
            ready_parameter: registration.ready_parameter,
            first_tick: None,
            ready_hero: None,
            result_action: registration.result_action,
            screens_complete: registration.screens_complete,
            phase,
            boss_pause,
        };
        self.world
            .get_mut::<GateComponent>(handle.entity)
            .expect("validated gate entity")
            .runtime = Some(gate_runtime);
        let entity = self.world.spawn(encounter).id();
        self.encounter_entity = Some(entity);
        self.set_mode_voice_set(voice_set);
        self.mode_boss_pause = boss_pause;
        self.debug_assert_encounter_invariants();
        Ok(entity)
    }

    pub(crate) fn validate_gate_and_encounter_handles(&self) -> Result<(), CombatError> {
        if let Some(handle) = self.gate_handle {
            let gate =
                self.world
                    .get::<GateComponent>(handle.entity)
                    .ok_or(CombatError::MissingGate {
                        wire: handle.wire,
                        entity: handle.entity,
                    })?;
            if gate.definition.entity != handle.wire {
                return Err(CombatError::GateIdentity {
                    wire: handle.wire,
                    actual: gate.definition.entity,
                });
            }
            if self.encounter_entity.is_some() && gate.runtime.is_none() {
                return Err(CombatError::MissingGate {
                    wire: handle.wire,
                    entity: handle.entity,
                });
            }
        }
        if let Some(entity) = self.encounter_entity {
            if self.gate_handle.is_none() {
                return Err(CombatError::MissingEncounterGate);
            }
            if self.world.get::<EncounterComponent>(entity).is_none() {
                return Err(CombatError::MissingEncounter { entity });
            }
        }
        Ok(())
    }

    pub(crate) fn debug_assert_encounter_invariants(&self) {
        if !cfg!(debug_assertions) {
            return;
        }
        if let Some(handle) = self.gate_handle {
            let gate = self
                .world
                .get::<GateComponent>(handle.entity)
                .expect("gate_handle references missing GateComponent");
            debug_assert_eq!(gate.definition.entity, handle.wire);
            debug_assert!(self.spawn_anchors.is_some());
            debug_assert_eq!(gate.runtime.is_some(), self.encounter_entity.is_some());
        } else {
            debug_assert!(self.encounter_entity.is_none());
        }
        if let Some(entity) = self.encounter_entity {
            let encounter = self
                .world
                .get::<EncounterComponent>(entity)
                .expect("encounter_entity references missing EncounterComponent");
            debug_assert_eq!(self.mode_voice_set, Some(encounter.voice_set));
            debug_assert_eq!(encounter.boss_pause, self.mode_boss_pause);
        }
    }

    fn gate_component(&self) -> Option<&GateComponent> {
        let handle = self.gate_handle?;
        let gate = self.world.get::<GateComponent>(handle.entity)?;
        (gate.definition.entity == handle.wire).then_some(gate)
    }

    fn encounter_component(&self) -> Option<&EncounterComponent> {
        let entity = self.encounter_entity?;
        self.world.get::<EncounterComponent>(entity)
    }

    pub(crate) fn junkenstein_gate(&self) -> Option<&JunkensteinGate> {
        self.gate_component().map(|gate| &gate.definition)
    }

    #[cfg(test)]
    pub(crate) fn gate_health(&self) -> Option<GateHealth> {
        self.gate_component()
            .and_then(|gate| gate.runtime.as_ref())
            .map(|runtime| runtime.health)
    }

    #[cfg(test)]
    pub(crate) fn gate_damage_stage(&self) -> Option<GateDamageStage> {
        self.gate_health().map(|health| health.stage)
    }

    pub(crate) fn gate_projection(
        &self,
    ) -> Option<crate::script_snapshot_wire::SceneProjection<'_>> {
        let runtime = self.gate_component()?.runtime.as_ref()?;
        Some(SceneProjection {
            snapshot: runtime.script.snapshot()?,
            encoder: SnapshotEncoder::new(&runtime.graphs, runtime.codecs.as_deref()?),
        })
    }

    pub(crate) fn set_gate_capacity(&mut self, capacity: f64) -> io::Result<()> {
        let capacity = f64::from(capacity as f32);
        if !capacity.is_finite() || capacity <= 0.0 {
            return Err(invalid("gate capacity must be positive and finite"));
        }
        self.validate_gate_and_encounter_handles()
            .map_err(io::Error::other)?;
        let Some(handle) = self.gate_handle else {
            return Err(invalid("missing Junkenstein gate"));
        };
        let Some(mode_variables) = self
            .encounter_snapshot()
            .map(|snapshot| snapshot.variables.clone())
        else {
            return Err(invalid("missing Junkenstein encounter"));
        };
        let (gate_graph, original_runtime) = {
            let mut gate = self
                .world
                .get_mut::<GateComponent>(handle.entity)
                .ok_or_else(|| invalid("missing Junkenstein gate component"))?;
            let runtime = gate
                .runtime
                .take()
                .ok_or_else(|| invalid("missing Junkenstein gate runtime"))?;
            (gate.definition.graph, runtime)
        };
        let mut runtime = original_runtime.clone();
        let result = {
            let mut world = GateWorld {
                entity: handle.wire,
                state: &mut runtime.state,
                bodies: self,
                mode_variables: &mode_variables,
            };
            runtime
                .script
                .set_instance_variable(1, 8567, Value::Float(capacity), &mut world)
                .map_err(invalid)
                .and_then(|()| runtime.script.run_now(&mut world).map_err(invalid))
                .and_then(|()| {
                    if runtime.script.resolved_variable(
                        0,
                        crate::script_expr::Variable {
                            scope: crate::script_expr::Scope::Entity,
                            id: 7786,
                        },
                    ) != Some(Value::Float(capacity))
                    {
                        Err(invalid(
                            "gate capacity override did not reach its authored binding",
                        ))
                    } else {
                        Ok(())
                    }
                })
        };
        let final_runtime = if result.is_ok() {
            if let Some(snapshot) = runtime.script.snapshot() {
                runtime.health = GateHealth::from_snapshot(snapshot, &runtime.graphs, gate_graph);
            }
            runtime
        } else {
            original_runtime
        };
        self.world
            .get_mut::<GateComponent>(handle.entity)
            .expect("validated gate component")
            .runtime = Some(final_runtime);
        result
    }

    pub(crate) fn take_gate_voice_requests(
        &mut self,
    ) -> Vec<crate::junkenstein_voice::VoiceRequest> {
        let Some(handle) = self.gate_handle else {
            return Vec::new();
        };
        let Some(mut gate) = self.world.get_mut::<GateComponent>(handle.entity) else {
            return Vec::new();
        };
        gate.runtime.as_mut().map_or_else(Vec::new, |runtime| {
            std::mem::take(&mut runtime.state.voice_requests)
        })
    }

    pub(crate) fn encounter_snapshot(&self) -> Option<&script_init::Snapshot> {
        self.encounter_component()?.script.snapshot()
    }

    #[cfg(test)]
    pub(crate) fn encounter_phase(&self) -> Option<EncounterPhase> {
        self.encounter_component().map(|encounter| encounter.phase)
    }

    pub(crate) fn encounter_boss_pause(&self) -> bool {
        self.encounter_component()
            .map_or(self.mode_boss_pause, |encounter| encounter.boss_pause)
    }

    pub(crate) fn encounter_countdown(&self) -> Option<CountdownFrame> {
        let snapshot = self.encounter_snapshot()?;
        let instance = snapshot.instances.get(&ENCOUNTER_INSTANCE)?;
        if instance.states.get(&COUNTDOWN_STATES[0])?.activation == 0 {
            return None;
        }
        let value = snapshot
            .resolved_bindings
            .get(&(0, COUNTDOWN_VAR))
            .or_else(|| snapshot.variables.get(&COUNTDOWN_VAR))?
            .clone();
        Some(CountdownFrame {
            value,
            states: COUNTDOWN_STATES.map(|raw| {
                instance
                    .states
                    .get(&raw)
                    .filter(|state| state.active)?
                    .chase
                    .clone()
            }),
        })
    }

    pub(crate) fn encounter_countdown_wire(
        &self,
        countdown: &CountdownFrame,
    ) -> io::Result<Option<BitWriter>> {
        let Some(encounter) = self.encounter_component() else {
            return Ok(None);
        };
        let Some(codecs) = &encounter.codecs else {
            return Ok(None);
        };
        let instance = encounter
            .script
            .snapshot()
            .and_then(|snapshot| snapshot.instances.get(&ENCOUNTER_INSTANCE))
            .ok_or_else(|| invalid("missing Junkenstein encounter instance"))?;
        let graph = encounter
            .script
            .graph_store()
            .graph(instance.graph)
            .ok_or_else(|| invalid("missing Junkenstein encounter graph"))?;
        encounter_countdown_delta(graph, codecs, ENCOUNTER_INSTANCE, countdown)
            .map(Some)
            .map_err(invalid)
    }

    pub(crate) fn encounter_screens_complete(&self) -> bool {
        self.encounter_component()
            .is_some_and(|encounter| encounter.screens_complete)
    }

    pub(crate) fn encounter_screen_delta(
        &self,
        previous: Option<&ProjectedSnapshot>,
    ) -> io::Result<Option<(ProjectedSnapshot, BitWriter)>> {
        let Some(encounter) = self.encounter_component() else {
            return Ok(None);
        };
        let (Some(snapshot), Some(codecs)) =
            (encounter.script.snapshot(), encounter.codecs.as_ref())
        else {
            return Ok(None);
        };
        let encoder = SnapshotEncoder::new(&encounter.graphs, codecs);
        let Some(current) = encoder.post_match_screens(snapshot).map_err(invalid)? else {
            return Ok(None);
        };
        let Some(payload) = encoder
            .post_match_delta(previous, &current)
            .map_err(invalid)?
        else {
            return Ok(None);
        };
        Ok(Some((current, payload)))
    }

    pub(crate) fn encounter_notification_delta(
        &self,
        previous: Option<&ProjectedSnapshot>,
    ) -> io::Result<Option<(ProjectedSnapshot, BitWriter)>> {
        let Some(encounter) = self.encounter_component() else {
            return Ok(None);
        };
        let (Some(snapshot), Some(codecs)) =
            (encounter.script.snapshot(), encounter.codecs.as_ref())
        else {
            return Ok(None);
        };
        let encoder = SnapshotEncoder::new(&encounter.graphs, codecs);
        let Some(current) = encoder.mission_notifications(snapshot).map_err(invalid)? else {
            return Ok(None);
        };
        let Some(payload) = encoder.mounted_delta(previous, &current).map_err(invalid)? else {
            return Ok(None);
        };
        Ok(Some((current, payload)))
    }

    pub(crate) fn encounter_phase_wire(
        &self,
        delivered: &std::collections::BTreeMap<u16, u16>,
    ) -> io::Result<Option<(u16, u16, BitWriter)>> {
        let Some(encounter) = self.encounter_component() else {
            return Ok(None);
        };
        let (Some(snapshot), Some(codecs)) =
            (encounter.script.snapshot(), encounter.codecs.as_ref())
        else {
            return Ok(None);
        };
        let Some(instance) = snapshot.instances.get(&ENCOUNTER_INSTANCE) else {
            return Ok(None);
        };
        let graph = encounter
            .graphs
            .graph(instance.graph)
            .ok_or_else(|| invalid("missing Junkenstein encounter graph"))?;
        // These are authored persistent sends, not the notification graph's
        // server-side receivers. The client-only HUD listens to the sends.
        for (&raw, state) in &instance.states {
            if !state.active
                || !matches!(
                    state.kind,
                    script_init::StateKind::PersistentSend(
                        0x0240_0000_0000_0694 | 0x0240_0000_0000_06a8
                    )
                )
                || delivered.get(&raw) == Some(&state.activation)
            {
                continue;
            }
            let payload = encounter_message_delta(
                graph,
                codecs,
                (ENCOUNTER_INSTANCE, None),
                raw,
                &Variables::new(),
                Variables::new(),
            )
            .map_err(invalid)?;
            return Ok(Some((raw, state.activation, payload)));
        }
        Ok(None)
    }

    pub(crate) fn encounter_active(&self) -> bool {
        self.encounter_component()
            .is_some_and(|encounter| encounter.result_action.is_none())
    }

    pub(crate) fn encounter_match_result(&self) -> Option<WireComponent> {
        let encounter = self.encounter_component()?;
        encounter.result_action?;
        let snapshot = encounter.script.snapshot()?;
        let instance = snapshot.instances.get(&ENCOUNTER_INSTANCE)?;
        let Value::Integer(winner) = instance.variables.get(&7525)? else {
            return None;
        };
        let winner = i32::try_from(*winner).ok()?;
        // Retail MECGameMode: the winner at +168 is compared with the local
        // team; the native MatchTime binding reads the float at +170.
        let mut fields = vec![None; 10];
        fields[7] = Some(ComponentValue::I32(winner));
        fields[9] = Some(ComponentValue::F32(
            encounter.script.now_ms() as f32 / 1000.0,
        ));
        Some(WireComponent { index: 114, fields })
    }

    pub(crate) fn encounter_post_match_wire(&self) -> io::Result<Option<BitWriter>> {
        let Some(encounter) = self.encounter_component() else {
            return Ok(None);
        };
        let (Some(snapshot), Some(codecs)) =
            (encounter.script.snapshot(), encounter.codecs.as_ref())
        else {
            return Ok(None);
        };
        let Some(instance) = snapshot.instances.get(&ENCOUNTER_INSTANCE) else {
            return Ok(None);
        };
        let raw = 8;
        if !instance.states.get(&raw).is_some_and(|state| {
            state.active && state.kind == script_init::StateKind::PersistentSend(POST_MATCH_PHASE)
        }) {
            return Ok(None);
        }
        let graph = encounter
            .graphs
            .graph(instance.graph)
            .ok_or_else(|| invalid("missing Junkenstein encounter graph"))?;
        let entity_variables = snapshot
            .variables
            .get(&8619)
            .map(|value| (8619, value.clone()))
            .into_iter()
            .collect();
        let instance_variables = instance
            .variables
            .get(&7525)
            .map(|value| (7525, value.clone()))
            .into_iter()
            .collect();
        encounter_message_delta(
            graph,
            codecs,
            (ENCOUNTER_INSTANCE, None),
            raw,
            &entity_variables,
            instance_variables,
        )
        .map(Some)
        .map_err(invalid)
    }

    pub(crate) fn encounter_result_wire(&self) -> io::Result<Option<BitWriter>> {
        use crate::junkenstein_messages::RESULT_SUMMARY;
        use crate::script_init::StateKind;

        let Some(encounter) = self.encounter_component() else {
            return Ok(None);
        };
        let (Some(snapshot), Some(codecs)) =
            (encounter.script.snapshot(), encounter.codecs.as_ref())
        else {
            return Ok(None);
        };
        let parent = (ENCOUNTER_INSTANCE, 13);
        let Some((&id, instance)) = snapshot
            .instances
            .iter()
            .find(|(_, instance)| instance.graph == 0x0cc7 && instance.parent == Some(parent))
        else {
            return Ok(None);
        };
        let Some(raw) = [2, 11].into_iter().find(|raw| {
            instance.states.get(raw).is_some_and(|state| {
                state.active && state.kind == StateKind::PersistentSend(RESULT_SUMMARY)
            })
        }) else {
            return Ok(None);
        };
        let graph = encounter
            .graphs
            .graph(instance.graph)
            .ok_or_else(|| invalid("missing Junkenstein results graph"))?;
        let select = |variables: &Variables, ids: &[u32]| -> Variables {
            ids.iter()
                .filter_map(|&id| variables.get(&id).map(|value| (id, value.clone())))
                .collect()
        };
        let (entity_ids, instance_ids): (&[u32], &[u32]) = if raw == 2 {
            (&[8525, 8642, 8643], &[])
        } else {
            (&[1303], &[176, 11817])
        };
        encounter_message_delta(
            graph,
            codecs,
            (id, Some(parent)),
            raw,
            &select(&snapshot.variables, entity_ids),
            select(&instance.variables, instance_ids),
        )
        .map(Some)
        .map_err(invalid)
    }

    pub(crate) fn encounter_outcome_wire(&self, stamp: u32) -> io::Result<Option<BitWriter>> {
        let Some(encounter) = self.encounter_component() else {
            return Ok(None);
        };
        let Some(site) = encounter.result_action else {
            return Ok(None);
        };
        let graph = encounter
            .graphs
            .graph(site.graph)
            .ok_or_else(|| invalid("missing encounter outcome graph"))?;
        let position = site
            .node
            .ok_or_else(|| invalid("missing encounter outcome node"))?;
        mode_action_delta(graph, site.instance, position, stamp)
            .map(Some)
            .map_err(invalid)
    }

    #[cfg(any(test, feature = "inspector"))]
    pub(crate) fn encounter_debug(&self) -> Option<crate::game_debug::Encounter> {
        let encounter = self.encounter_component()?;
        encounter.first_tick?;
        let snapshot = encounter.script.snapshot()?;
        let instance = snapshot.instances.get(&ENCOUNTER_INSTANCE)?;
        Some(crate::game_debug::Encounter {
            elapsed_ms: encounter.script.now_ms(),
            countdown_started: instance
                .states
                .get(&52)
                .is_some_and(|state| state.activation != 0),
        })
    }

    #[cfg(any(test, feature = "inspector"))]
    pub(super) fn push_encounter_debug_spawn_candidates(
        &self,
        out: &mut Vec<crate::game_debug::DebugSpawnCandidate>,
        truncated: &mut bool,
    ) {
        let Some(encounter) = self.encounter_component() else {
            return;
        };
        let (candidates, search_truncated) = encounter.searches.debug_candidates();
        *truncated |= search_truncated;
        let remaining = crate::game_debug::MAX_DEBUG_SPAWN_CANDIDATES.saturating_sub(out.len());
        if candidates.len() > remaining {
            *truncated = true;
        }
        out.extend(candidates.iter().copied().take(remaining));
    }

    #[cfg(any(test, feature = "inspector"))]
    pub(super) fn push_encounter_debug_scripts(
        &self,
        out: &mut Vec<crate::game_debug::EntityScriptSnapshot>,
    ) {
        if let Some(encounter) = self.encounter_component() {
            if let Some(snapshot) = encounter.script.snapshot() {
                out.push(crate::game_debug::summarize_script_snapshot(
                    GAME_MODE_ENTITY,
                    "encounter",
                    snapshot,
                ));
            }
        }
        if let Some(gate) = self.gate_component() {
            if let Some(runtime) = gate.runtime.as_ref() {
                if let Some(snapshot) = runtime.script.snapshot() {
                    out.push(crate::game_debug::summarize_script_snapshot(
                        gate.definition.entity,
                        "gate",
                        snapshot,
                    ));
                }
            }
        }
    }

    pub(crate) fn step_encounter(
        &mut self,
        collision: &CollisionScene,
        spawn_mover: &MoverSettings,
        tick: u32,
        hero: Option<u64>,
    ) -> Result<(), CombatError> {
        self.validate_gate_and_encounter_handles()?;
        let (Some(gate_handle), Some(encounter_entity)) = (self.gate_handle, self.encounter_entity)
        else {
            return Ok(());
        };
        let mut encounter = self
            .world
            .get_entity_mut(encounter_entity)
            .map_err(|_| CombatError::MissingEncounter {
                entity: encounter_entity,
            })?
            .take::<EncounterComponent>()
            .ok_or(CombatError::MissingEncounter {
                entity: encounter_entity,
            })?;
        let (gate, anchors, mut gate_runtime) = {
            let mut gate_component = self
                .world
                .get_mut::<GateComponent>(gate_handle.entity)
                .ok_or(CombatError::MissingGate {
                    wire: gate_handle.wire,
                    entity: gate_handle.entity,
                })?;
            let runtime = gate_component
                .runtime
                .take()
                .ok_or(CombatError::MissingGate {
                    wire: gate_handle.wire,
                    entity: gate_handle.entity,
                })?;
            (
                gate_component.definition.clone(),
                Arc::clone(&gate_component.anchors),
                runtime,
            )
        };
        match self.step_encounter_components(
            &gate,
            &anchors,
            &mut gate_runtime,
            &mut encounter,
            collision,
            spawn_mover,
            tick,
            hero,
        ) {
            Ok(()) => {
                if let Ok(mut gate_entity) = self.world.get_entity_mut(gate_handle.entity) {
                    if let Some(mut gate_component) = gate_entity.get_mut::<GateComponent>() {
                        gate_component.runtime = Some(gate_runtime);
                    }
                }
                if let Ok(mut entity_mut) = self.world.get_entity_mut(encounter_entity) {
                    entity_mut.insert(encounter);
                }
                self.debug_assert_encounter_invariants();
            }
            Err(fault) => {
                eprintln!("Junkenstein encounter halted: {fault}");
                self.set_mode_boss_pause(false);
                self.world.despawn(encounter_entity);
                self.encounter_entity = None;
                self.debug_assert_encounter_invariants();
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn step_encounter_components(
        &mut self,
        gate: &JunkensteinGate,
        anchors: &SpawnAnchors,
        gate_runtime: &mut GateRuntime,
        encounter: &mut EncounterComponent,
        collision: &CollisionScene,
        spawn_mover: &MoverSettings,
        tick: u32,
        hero: Option<u64>,
    ) -> Result<(), InitFault> {
        // E1: advance room-relative encounter clock.
        let first = *encounter.first_tick.get_or_insert(tick);
        let frame = tick.saturating_sub(first);
        // E2: drain combat scene messages into gate and mode scripts.
        for message in self.take_scene_messages() {
            if message.target == gate.entity {
                gate_runtime.script.queue_entity_frame_message(
                    frame,
                    message.message,
                    message.parameters,
                )?;
            } else if message.target == GAME_MODE_ENTITY {
                encounter.script.queue_entity_frame_message(
                    frame,
                    message.message,
                    message.parameters,
                )?;
            }
        }
        // E3: forward boss intro starts from live enemy scripts.
        for (body, parameters) in self.take_boss_intro_starts() {
            encounter.script.queue_frame_message_from(
                frame,
                body,
                crate::junkenstein_messages::BOSS_INTRO_STARTED,
                parameters,
            )?;
        }
        // E4: advance collision-backed spawn searches.
        encounter
            .searches
            .step(frame, &mut encounter.script, collision, spawn_mover)?;
        // E5: advance gate script against current mode variables.
        let previous_gate_variables = gate_runtime
            .script
            .snapshot()
            .ok_or(InitFault::Halted)?
            .variables
            .clone();
        {
            let mode_variables = &encounter
                .script
                .snapshot()
                .ok_or(InitFault::Halted)?
                .variables;
            let mut gate_world = GateWorld {
                entity: gate.entity,
                state: &mut gate_runtime.state,
                bodies: self,
                mode_variables,
            };
            gate_runtime.script.advance_frame(frame, &mut gate_world)?;
        }
        // E6: notify mode script of changed gate entity variables.
        let gate_variables = &gate_runtime
            .script
            .snapshot()
            .ok_or(InitFault::Halted)?
            .variables;
        for id in previous_gate_variables
            .keys()
            .chain(gate_variables.keys())
            .copied()
            .collect::<BTreeSet<_>>()
        {
            if previous_gate_variables.get(&id) != gate_variables.get(&id) {
                encounter
                    .script
                    .queue_foreign_variable_change(frame, gate.entity, id)?;
            }
        }
        // E7: deliver queued gate -> mode messages before advancing mode.
        for message in std::mem::take(&mut gate_runtime.state.messages) {
            encounter.script.queue_entity_frame_message(
                frame,
                message.message,
                message.parameters,
            )?;
        }
        // E8: notify encounter instance when a player hero becomes ready.
        if let Some(hero) = hero.filter(|&hero| encounter.ready_hero != Some(hero)) {
            // The persistent 0C8C receiver aggregates its 0C90 sender as an array.
            let parameters = Variables::from([(
                encounter.ready_parameter,
                Value::Array(vec![Value::Asset(hero)]),
            )]);
            if encounter.script.queue_frame_message(
                frame,
                ENCOUNTER_INSTANCE,
                encounter.ready_message,
                parameters,
            )? != 0
            {
                encounter.ready_hero = Some(hero);
            }
        }
        // E9: advance encounter mode script.
        let gate_variables = &gate_runtime
            .script
            .snapshot()
            .ok_or(InitFault::Halted)?
            .variables;
        let mode_variables = Arc::new(
            encounter
                .script
                .snapshot()
                .ok_or(InitFault::Halted)?
                .variables
                .clone(),
        );
        let mut world = EncounterWorld {
            gate,
            anchors,
            gate_variables,
            bodies: self,
            creator: encounter.codecs.as_ref().map(|codecs| EnemyCreator {
                graphs: &encounter.graphs,
                codecs,
                catalog: encounter.npcs.as_deref(),
                heroes: &encounter.heroes,
                movers: &encounter.movers,
                mode: encounter.mode,
                tick,
                mode_variables,
            }),
            result_action: &mut encounter.result_action,
            screens_complete: &mut encounter.screens_complete,
            team: encounter.team,
            difficulty: encounter.difficulty,
        };
        encounter.script.advance_frame(frame, &mut world)?;
        // E10: publish mode variables, boss pause, and derived ECS projections.
        let mode = encounter.script.snapshot().ok_or(InitFault::Halted)?;
        self.set_mode_variables(Arc::new(mode.variables.clone()));
        let boss_pause = mode.boss_pause_active();
        self.set_mode_boss_pause(boss_pause);
        encounter.boss_pause = boss_pause;
        if let Some(gate_snapshot) = gate_runtime.script.snapshot() {
            gate_runtime.health =
                GateHealth::from_snapshot(gate_snapshot, &gate_runtime.graphs, gate.graph);
        }
        encounter.phase = EncounterPhase::from_state(
            encounter.first_tick,
            Some(mode),
            encounter.result_action,
            encounter.screens_complete,
        );
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn gate_snapshot(&self) -> Option<&script_init::Snapshot> {
        self.gate_component()?.runtime.as_ref()?.script.snapshot()
    }

    #[cfg(test)]
    pub(crate) fn gate_resolved_variable(
        &self,
        instance: u16,
        variable: crate::script_expr::Variable,
    ) -> Option<Value> {
        self.gate_component()?
            .runtime
            .as_ref()?
            .script
            .resolved_variable(instance, variable)
    }

    #[cfg(test)]
    pub(crate) fn gate_team(&self) -> Option<i8> {
        self.gate_component()?
            .runtime
            .as_ref()
            .and_then(|runtime| runtime.state.team)
    }

    #[cfg(test)]
    pub(crate) fn step_test_gate_frame(
        &mut self,
        frame: u32,
        message: u64,
        parameters: Variables,
    ) -> Result<(), InitFault> {
        let handle = self.gate_handle.expect("registered Junkenstein gate");
        let mode_variables = self
            .encounter_snapshot()
            .expect("live encounter")
            .variables
            .clone();
        let (gate_graph, mut runtime) = {
            let mut gate = self
                .world
                .get_mut::<GateComponent>(handle.entity)
                .expect("live gate component");
            (
                gate.definition.graph,
                gate.runtime.take().expect("live gate runtime"),
            )
        };
        let result = {
            let mut world = GateWorld {
                entity: handle.wire,
                state: &mut runtime.state,
                bodies: self,
                mode_variables: &mode_variables,
            };
            runtime
                .script
                .queue_entity_frame_message(frame, message, parameters)
                .and_then(|_| runtime.script.advance_frame(frame, &mut world))
        };
        if result.is_ok() {
            if let Some(snapshot) = runtime.script.snapshot() {
                runtime.health = GateHealth::from_snapshot(snapshot, &runtime.graphs, gate_graph);
            }
        }
        self.world
            .get_mut::<GateComponent>(handle.entity)
            .expect("live gate component")
            .runtime = Some(runtime);
        result
    }

    #[cfg(test)]
    pub(crate) fn queue_test_encounter_entity_message(
        &mut self,
        message: u64,
        parameters: Variables,
    ) -> Result<usize, InitFault> {
        let entity = self.encounter_entity.expect("registered encounter");
        let mut encounter = self
            .world
            .get_mut::<EncounterComponent>(entity)
            .expect("live encounter component");
        encounter.script.queue_entity_message(message, parameters)
    }

    #[cfg(test)]
    pub(crate) fn queue_test_encounter_frame_message(
        &mut self,
        frame: u32,
        instance: u16,
        message: u64,
        parameters: Variables,
    ) -> Result<usize, InitFault> {
        let entity = self.encounter_entity.expect("registered encounter");
        let mut encounter = self
            .world
            .get_mut::<EncounterComponent>(entity)
            .expect("live encounter component");
        encounter
            .script
            .queue_frame_message(frame, instance, message, parameters)
    }

    #[cfg(test)]
    pub(crate) fn queue_test_spawn_result(
        &mut self,
        frame: u32,
        key: script_init::SpawnSearchKey,
        positions: Vec<[f64; 3]>,
    ) -> Result<(), InitFault> {
        let entity = self.encounter_entity.expect("registered encounter");
        let mut encounter = self
            .world
            .get_mut::<EncounterComponent>(entity)
            .expect("live encounter component");
        encounter.script.queue_spawn_result(frame, key, positions)
    }

    #[cfg(test)]
    pub(crate) fn encounter_spawn_search_keys(
        &self,
    ) -> Vec<(script_init::SpawnSearchKey, script_init::SpawnSearch)> {
        self.encounter_component()
            .map_or_else(Vec::new, |encounter| {
                encounter.script.spawn_searches().collect()
            })
    }

    #[cfg(test)]
    pub(crate) fn invalidate_gate_component(&mut self) -> bool {
        let Some(handle) = self.gate_handle else {
            return false;
        };
        let Ok(mut entity) = self.world.get_entity_mut(handle.entity) else {
            return false;
        };
        entity.take::<GateComponent>().is_some()
    }

    #[cfg(test)]
    pub(crate) fn corrupt_gate_identity(&mut self, wire: u32) -> bool {
        let Some(handle) = self.gate_handle else {
            return false;
        };
        let Some(mut gate) = self.world.get_mut::<GateComponent>(handle.entity) else {
            return false;
        };
        gate.definition.entity = wire;
        true
    }

    #[cfg(test)]
    pub(crate) fn invalidate_encounter_component(&mut self) -> bool {
        let Some(entity) = self.encounter_entity else {
            return false;
        };
        let Ok(mut entity_mut) = self.world.get_entity_mut(entity) else {
            return false;
        };
        entity_mut.take::<EncounterComponent>().is_some()
    }
}

fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
