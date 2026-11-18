//! Immutable-per-room prepared scene state and simulation accessors.

use super::*;

pub struct PreparedScene {
    pub(super) junkenstein_validation: Option<crate::junkenstein_validation::JunkensteinValidation>,
    pub(super) map: GameMap,
    #[cfg(any(test, feature = "inspector"))]
    pub(super) movement_cache: String,
    pub(super) ground: Arc<CollisionScene>,
    pub(super) contacts: Option<Arc<ContactWorld>>,
    pub(super) controller: Controller,
    pub(super) mode_ui: BitWriter,
    pub(super) junkenstein_gate: Option<JunkensteinGate>,
    pub(super) junkenstein_anchors: Arc<SpawnAnchors>,
    pub(super) encounter: Option<JunkensteinEncounter>,
    pub(super) initial_board: Component,
    pub(super) simulation: RoomSimulation,
    pub(super) scripted_bots: usize,
    pub(super) movers: Arc<MoverCatalog>,
    pub(super) heroes: Arc<HeroCatalog>,
    pub(super) pools: Arc<PoolCatalog>,
    pub(crate) owner_body_factory: Option<Arc<OwnerBodyFactory>>,
}

impl PreparedScene {
    #[cfg(test)]
    pub(crate) fn fixture_for_bots(bots: Vec<Bot>) -> Self {
        use crate::collision_cache::{CollisionCache, Mesh, Triangle};

        let cache = CollisionCache {
            header: serde_json::from_str(
                r#"{"meshes":[[3,1]],"hulls":0,"models":0,"placements":0,"identifiers":[]}"#,
            )
            .unwrap(),
            meshes: vec![Mesh {
                vertices: vec![[-1.0, 0.0, 0.0], [-1.0, 2.0, 0.0], [-1.0, 0.0, 1.0]],
                triangles: vec![Triangle {
                    corners: [0, 1, 2],
                    wings: [-1; 3],
                    mask: 0x18,
                    surface: 0,
                }],
            }],
            hulls: vec![],
            models: vec![],
            placements: vec![],
        };
        let ground = Arc::new(CollisionScene::movement_probe(&cache, &BTreeSet::new()).unwrap());
        Self {
            junkenstein_validation: None,
            map: GameMap::practice_range(),
            #[cfg(any(test, feature = "inspector"))]
            movement_cache: String::new(),
            ground,
            contacts: None,
            controller: Controller::fixture_for_test(),
            mode_ui: BitWriter::new(),
            junkenstein_gate: None,
            junkenstein_anchors: Arc::new(SpawnAnchors::default()),
            encounter: None,
            initial_board: Component {
                index: 0,
                fields: Vec::new(),
            },
            simulation: RoomSimulation::new(bots).unwrap(),
            scripted_bots: 0,
            movers: Arc::new(MoverCatalog::default()),
            heroes: Arc::new(HeroCatalog::default()),
            pools: Arc::new(PoolCatalog::default()),
            owner_body_factory: None,
        }
    }

    pub(crate) fn simulation(&self) -> &RoomSimulation {
        &self.simulation
    }

    pub(crate) fn simulation_mut(&mut self) -> &mut RoomSimulation {
        &mut self.simulation
    }

    pub(crate) fn simulation_and_rays(&mut self) -> (&mut RoomSimulation, &TriangleIndex) {
        (&mut self.simulation, self.ground.triangles())
    }

    pub(crate) fn enemy_projections(
        &self,
    ) -> Result<impl Iterator<Item = crate::script_snapshot_wire::SceneProjection<'_>>, CombatError>
    {
        self.simulation.enemy_projections()
    }
    pub(crate) fn gate_projection(
        &self,
    ) -> Option<crate::script_snapshot_wire::SceneProjection<'_>> {
        let encounter = self.encounter.as_ref()?;
        Some(crate::script_snapshot_wire::SceneProjection {
            snapshot: encounter.gate_script.snapshot()?,
            encoder: crate::script_snapshot_wire::SnapshotEncoder::new(
                &encounter.graphs,
                encounter.codecs.as_deref()?,
            ),
        })
    }
    pub(crate) fn take_enemy_creates(&mut self) -> Vec<EntityUpdate> {
        self.simulation.take_created()
    }
    pub(crate) fn take_encounter_voice_requests(
        &mut self,
    ) -> Vec<crate::junkenstein_voice::VoiceRequest> {
        let mut requests = self.encounter.as_mut().map_or_else(Vec::new, |encounter| {
            std::mem::take(&mut encounter.gate_state.voice_requests)
        });
        requests.extend(self.simulation.take_encounter_voice_requests());
        requests
    }
    pub fn map(&self) -> &GameMap {
        &self.map
    }

    pub fn scripted_bots(&self) -> usize {
        self.scripted_bots
    }

    pub(crate) fn movement_ready(&self) -> bool {
        (is_practice_range(&self.map) || self.map.is_junkenstein())
            && self.scripted_bots == 0
            && self.contacts.is_some()
    }

    #[cfg(test)]
    pub(crate) fn movement_index(&self) -> &TriangleIndex {
        self.ground.triangles()
    }

    #[cfg(any(test, feature = "inspector"))]
    pub(crate) fn debug_geometry(&self) -> DebugGeometry {
        DebugGeometry::new(
            &self.map,
            self.movement_cache.clone(),
            MOVER_CATEGORY,
            Arc::clone(&self.ground),
            self.contacts.clone(),
        )
    }

    pub(crate) fn player_spawn_mover(&self, body: u64, team: u8) -> io::Result<Mover> {
        let (position, yaw) = self
            .map
            .spawn_point(team, 0)
            .ok_or_else(|| invalid("no spawn for the selected hero"))?;
        let yaw = (yaw * 65536.0 / 360.0).round_ties_even() as i32;
        let contacts = self.contacts.as_ref().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "the scene has no native player collision",
            )
        })?;
        Mover::new(
            *self.movers.for_body(Some(body)),
            Arc::clone(contacts),
            position,
            yaw,
            mover_category(&self.map, team),
        )
        .map_err(invalid)
    }

    pub(crate) fn owner_body_runtime(
        &self,
        hero: &Hero,
        spawn: OwnerBodySpawn,
    ) -> io::Result<Option<Box<dyn OwnerBodyRuntime>>> {
        self.owner_body_factory
            .as_ref()
            .map_or(Ok(None), |factory| factory(hero, spawn))
    }

    pub fn heroes(&self) -> &HeroCatalog {
        &self.heroes
    }

    pub fn pools(&self) -> &PoolCatalog {
        &self.pools
    }

    pub fn controller(&self) -> Controller {
        self.controller
    }

    pub fn mode_ui(&self) -> &BitWriter {
        &self.mode_ui
    }

    pub fn junkenstein_gate(&self) -> Option<&JunkensteinGate> {
        self.junkenstein_gate.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn gate_snapshot(&self) -> Option<&script_init::Snapshot> {
        self.encounter.as_ref()?.gate_script.snapshot()
    }

    #[cfg(test)]
    pub(crate) fn set_test_gate_capacity(&mut self, capacity: f64) {
        self.set_gate_capacity(capacity).unwrap();
    }

    pub(super) fn set_gate_capacity(&mut self, capacity: f64) -> io::Result<()> {
        let capacity = f64::from(capacity as f32);
        if !capacity.is_finite() || capacity <= 0.0 {
            return Err(invalid("gate capacity must be positive and finite"));
        }
        let gate = self.junkenstein_gate.as_ref().unwrap();
        let encounter = self.encounter.as_mut().unwrap();
        let mut world = GateWorld {
            entity: gate.entity,
            state: &mut encounter.gate_state,
            bodies: &self.simulation,
            mode_variables: &encounter.script.snapshot().unwrap().variables,
        };
        encounter
            .gate_script
            .set_instance_variable(1, 8567, Value::Float(capacity), &mut world)
            .map_err(invalid)?;
        encounter.gate_script.run_now(&mut world).map_err(invalid)?;
        if encounter.gate_script.resolved_variable(
            0,
            crate::script_expr::Variable {
                scope: crate::script_expr::Scope::Entity,
                id: 7786,
            },
        ) != Some(Value::Float(capacity))
        {
            return Err(invalid(
                "gate capacity override did not reach its authored binding",
            ));
        }
        Ok(())
    }

    pub(crate) fn protects_cassidy(&self, hero: Option<u64>) -> bool {
        self.junkenstein_validation
            .is_some_and(|settings| settings.protects(hero))
    }

    pub(crate) fn encounter_snapshot(&self) -> Option<&script_init::Snapshot> {
        self.encounter.as_ref()?.script.snapshot()
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
        let Some(encounter) = &self.encounter else {
            return Ok(None);
        };
        let Some(codecs) = &encounter.codecs else {
            return Ok(None);
        };
        let instance = self
            .encounter_snapshot()
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
        self.encounter
            .as_ref()
            .is_some_and(|encounter| encounter.screens_complete)
    }

    pub(crate) fn encounter_screen_delta(
        &self,
        previous: Option<&ProjectedSnapshot>,
    ) -> io::Result<Option<(ProjectedSnapshot, BitWriter)>> {
        let Some(encounter) = &self.encounter else {
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
        let Some(encounter) = &self.encounter else {
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
        let Some(encounter) = &self.encounter else {
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
        self.encounter
            .as_ref()
            .is_some_and(|encounter| encounter.result_action.is_none())
    }

    pub(crate) fn encounter_match_result(&self) -> Option<Component> {
        let encounter = self.encounter.as_ref()?;
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
        Some(Component { index: 114, fields })
    }

    pub(crate) fn encounter_post_match_wire(&self) -> io::Result<Option<BitWriter>> {
        let Some(encounter) = &self.encounter else {
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

        let Some(encounter) = &self.encounter else {
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
        let Some(encounter) = &self.encounter else {
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
        let encounter = self.encounter.as_ref()?;
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

    pub(crate) fn step_encounter(&mut self, tick: u32, hero: Option<u64>) {
        let Some(encounter) = &mut self.encounter else {
            return;
        };
        let gate = self
            .junkenstein_gate
            .as_ref()
            .expect("initialized Junkenstein gate");
        let spawn_mover = self.movers.for_body(Some(0x0400_0000_0000_16be));
        if let Err(error) = encounter.step(
            (gate, &self.junkenstein_anchors, &mut self.simulation),
            (&self.ground, spawn_mover),
            tick,
            hero,
        ) {
            eprintln!("Junkenstein encounter halted: {error}");
            self.simulation.set_mode_boss_pause(false);
            self.encounter = None;
        }
    }

    pub fn bots(&self) -> Result<impl ExactSizeIterator<Item = &Bot>, CombatError> {
        self.simulation.bodies()
    }

    #[cfg(any(test, feature = "inspector"))]
    pub(crate) fn bot_actors(&self) -> Result<impl ExactSizeIterator<Item = Actor>, CombatError> {
        self.simulation.debug_actors()
    }

    pub(crate) fn deadeye_scene(
        &self,
        team: u8,
    ) -> Result<crate::script_player::DeadeyeScene, CombatError> {
        let queries = BotQueries::new(
            self.map.free_for_all,
            &self.movers,
            &self.ground,
            self.contacts.as_ref(),
        );
        let targets = self
            .simulation
            .living_enemies(team)?
            .map(|(bot, health)| crate::script_player::DeadeyeTarget {
                velocity: Some(bot.velocity),
                script_state: self.simulation.body_script_state(bot.entity),
                entity: bot.entity,
                pose: queries.target(bot),
                health,
            })
            .collect();
        Ok(crate::script_player::DeadeyeScene {
            health: Some(self.simulation.body_healths()?),
            targets,
            collision: Some(self.ground.clone()),
            view_subjects: self.simulation.boss_view_subjects()?,
        })
    }

    #[cfg(test)]
    pub(crate) fn invalidate_bot_component(&mut self) -> bool {
        self.simulation.invalidate_bot_component()
    }

    pub(crate) fn bot_simulation(&mut self) -> (&mut RoomSimulation, BotQueries<'_>) {
        if let Some(encounter) = &self.encounter {
            self.simulation.set_mode_voice_set(encounter.voice_set);
        }
        self.simulation
            .set_cassidy_protected(self.protects_cassidy(Some(crate::game_heroes::CASSIDY)));
        (
            &mut self.simulation,
            BotQueries::new(
                self.map.free_for_all,
                &self.movers,
                &self.ground,
                self.contacts.as_ref(),
            ),
        )
    }

    pub(crate) fn retire_projectiles(&mut self, connection: u32) {
        self.simulation.retire_projectiles(connection);
    }

    #[cfg(test)]
    pub(crate) fn set_junkenstein_defenders(&mut self, defenders: Arc<[u32]>) {
        self.simulation.set_junkenstein_defenders(defenders);
    }

    pub(crate) fn clear_projectiles(&mut self) {
        self.simulation.clear_projectiles();
    }

    pub(crate) fn step_bots(
        &mut self,
        tick: u32,
        patrol: &PatrolScript,
    ) -> Result<(), CombatError> {
        let floor = self.ground.triangles();
        let map = &self.map;
        self.simulation.for_each_body_mut(|bot| {
            if let Err(error) = bot.step(
                tick,
                patrol,
                ground_height(floor, mover_category(map, bot.spawn.team)),
            ) {
                eprintln!(
                    "Patrol for bot {:08x} stopped at frame {tick}: {error}",
                    bot.entity
                );
            }
        })
    }

    pub fn initial_world(&self, viewer: &PlayerAppearance<'_>) -> io::Result<Vec<EntityUpdate>> {
        let mut updates = initial_world(
            &self.map,
            viewer,
            self.initial_board.clone(),
            std::iter::empty(),
        )?;
        updates.extend(self.simulation.initial_creates()?);
        Ok(updates)
    }
}
