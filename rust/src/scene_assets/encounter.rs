//! Junkenstein castle gate, spawn anchor, and encounter script world adapters.

use super::*;

/// Authored server placement and script for the castle gate.
#[derive(Debug, PartialEq)]
pub struct JunkensteinGate {
    pub entity: u32,
    pub position: [f64; 3],
    pub graph: u32,
}

impl JunkensteinGate {
    pub const TAG: u64 = 0x0d60_0000_0000_030e;
}

pub(super) struct SpawnAnchor {
    pub(super) position: [f64; 3],
    pub(super) forward: [f64; 3],
}

#[derive(Default)]
pub(crate) struct SpawnAnchors {
    pub(super) transforms: BTreeMap<u32, SpawnAnchor>,
    pub(super) tagged: BTreeMap<u64, Option<Vec<u32>>>,
}

impl SpawnAnchors {
    pub(crate) fn targets(&self, tag: u64) -> Option<&[u32]> {
        match self.tagged.get(&tag) {
            Some(targets) => targets.as_deref(),
            None => Some(&[]),
        }
    }

    pub(crate) fn position(&self, entity: u32) -> Option<[f64; 3]> {
        self.transforms.get(&entity).map(|anchor| anchor.position)
    }

    pub(crate) fn forward(&self, entity: u32) -> Option<[f64; 3]> {
        self.transforms.get(&entity).map(|anchor| anchor.forward)
    }
}

pub(super) struct EncounterWorld<'a> {
    pub(super) gate: &'a JunkensteinGate,
    pub(super) anchors: &'a SpawnAnchors,
    pub(super) gate_variables: &'a Variables,
    pub(super) bodies: &'a mut RoomSimulation,
    pub(super) creator: Option<EnemyCreator<'a>>,
    pub(super) result_action: &'a mut Option<Site>,
    pub(super) screens_complete: &'a mut bool,
    pub(super) team: u8,
    pub(super) difficulty: u64,
}

pub(super) struct EnemyCreator<'a> {
    pub(super) graphs: &'a Arc<GraphStore>,
    pub(super) codecs: &'a Arc<CodecTable>,
    pub(super) catalog: Option<&'a crate::npc_catalog::NpcCatalog>,
    pub(super) heroes: &'a HeroCatalog,
    pub(super) movers: &'a MoverCatalog,
    pub(super) mode: u64,
    pub(super) mode_variables: Arc<Variables>,
    pub(super) tick: u32,
}

impl World for EncounterWorld<'_> {
    fn engine_setting(&self, _site: Site, key: u32) -> crate::script_expr::EngineSetting {
        crate::junkenstein_host::engine_setting(key)
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
        self.anchors
            .transforms
            .get(&entity)
            .map(|anchor| anchor.position)
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
        self.anchors
            .transforms
            .get(&entity)
            .map(|anchor| anchor.position)
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

#[derive(Default)]
pub(super) struct GateState {
    pub(super) team: Option<i8>,
    pub(super) messages: Vec<crate::script_enemy::SceneMessage>,
    pub(super) voice_requests: Vec<crate::junkenstein_voice::VoiceRequest>,
    pub(super) voice_variation: u64,
}

pub(super) struct GateWorld<'a> {
    pub(super) entity: u32,
    pub(super) state: &'a mut GateState,
    pub(super) bodies: &'a RoomSimulation,
    pub(super) mode_variables: &'a Variables,
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

pub(super) struct JunkensteinEncounter {
    pub(super) voice_set: u64,
    pub(super) graphs: Arc<GraphStore>,
    pub(super) npcs: Option<Arc<crate::npc_catalog::NpcCatalog>>,
    pub(super) heroes: Arc<HeroCatalog>,
    pub(super) movers: Arc<MoverCatalog>,
    pub(super) mode: u64,
    pub(super) script: script_init::Component,
    pub(super) codecs: Option<Arc<CodecTable>>,
    pub(super) searches: SpawnSearches,
    pub(super) gate_script: script_init::Component,
    pub(super) gate_state: GateState,
    pub(super) team: u8,
    pub(super) difficulty: u64,
    pub(super) ready_message: u64,
    pub(super) ready_parameter: u32,
    pub(super) first_tick: Option<u32>,
    pub(super) ready_hero: Option<u64>,
    pub(super) result_action: Option<Site>,
    pub(super) screens_complete: bool,
}

impl JunkensteinEncounter {
    pub(super) fn step(
        &mut self,
        (gate, anchors, bodies): (&JunkensteinGate, &SpawnAnchors, &mut RoomSimulation),
        (collision, spawn_mover): (&CollisionScene, &MoverSettings),
        tick: u32,
        hero: Option<u64>,
    ) -> Result<(), script_init::InitFault> {
        let first = *self.first_tick.get_or_insert(tick);
        let frame = tick.saturating_sub(first);
        for message in bodies.take_scene_messages() {
            if message.target == gate.entity {
                self.gate_script.queue_entity_frame_message(
                    frame,
                    message.message,
                    message.parameters,
                )?;
            } else if message.target == GAME_MODE_ENTITY {
                self.script.queue_entity_frame_message(
                    frame,
                    message.message,
                    message.parameters,
                )?;
            }
        }
        for (body, parameters) in bodies.take_boss_intro_starts() {
            self.script.queue_frame_message_from(
                frame,
                body,
                crate::junkenstein_messages::BOSS_INTRO_STARTED,
                parameters,
            )?;
        }
        self.searches
            .step(frame, &mut self.script, collision, spawn_mover)?;
        let mut gate_world = GateWorld {
            entity: gate.entity,
            state: &mut self.gate_state,
            bodies,
            mode_variables: &self.script.snapshot().expect("live encounter").variables,
        };
        let previous_gate_variables = self
            .gate_script
            .snapshot()
            .expect("live gate")
            .variables
            .clone();
        self.gate_script.advance_frame(frame, &mut gate_world)?;
        let gate_variables = &self
            .gate_script
            .snapshot()
            .expect("advanced gate")
            .variables;
        for id in previous_gate_variables
            .keys()
            .chain(gate_variables.keys())
            .copied()
            .collect::<BTreeSet<_>>()
        {
            if previous_gate_variables.get(&id) != gate_variables.get(&id) {
                self.script
                    .queue_foreign_variable_change(frame, gate.entity, id)?;
            }
        }
        for message in std::mem::take(&mut self.gate_state.messages) {
            self.script
                .queue_entity_frame_message(frame, message.message, message.parameters)?;
        }
        let gate_variables = &self
            .gate_script
            .snapshot()
            .expect("advanced gate script")
            .variables;
        let mut world = EncounterWorld {
            gate,
            anchors,
            gate_variables,
            bodies,
            creator: self.codecs.as_ref().map(|codecs| EnemyCreator {
                graphs: &self.graphs,
                codecs,
                catalog: self.npcs.as_deref(),
                heroes: &self.heroes,
                movers: &self.movers,
                mode: self.mode,
                tick,
                mode_variables: Arc::new(
                    self.script
                        .snapshot()
                        .expect("live encounter")
                        .variables
                        .clone(),
                ),
            }),
            result_action: &mut self.result_action,
            screens_complete: &mut self.screens_complete,
            team: self.team,
            difficulty: self.difficulty,
        };
        if let Some(hero) = hero.filter(|&hero| self.ready_hero != Some(hero)) {
            // The persistent 0C8C receiver aggregates its 0C90 sender as an array.
            let parameters =
                Variables::from([(self.ready_parameter, Value::Array(vec![Value::Asset(hero)]))]);
            if self.script.queue_frame_message(
                frame,
                ENCOUNTER_INSTANCE,
                self.ready_message,
                parameters,
            )? != 0
            {
                self.ready_hero = Some(hero);
            }
        }
        self.script.advance_frame(frame, &mut world)?;
        let mode = self.script.snapshot().expect("live encounter");
        world
            .bodies
            .set_mode_variables(Arc::new(mode.variables.clone()));
        world.bodies.set_mode_boss_pause(mode.boss_pause_active());
        Ok(())
    }
}
