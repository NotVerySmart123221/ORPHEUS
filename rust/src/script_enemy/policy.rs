use super::*;

pub(super) fn flashbang_entry_admits(
    graphs: &GraphStore,
    mut entity_value: impl FnMut(crate::script_expr::Variable) -> Option<Value>,
) -> Result<bool, String> {
    use crate::script_expr::{Context, Scope, Variable};
    struct Inputs(Vec<(Variable, Value)>);
    impl Context for Inputs {
        fn variable(&mut self, variable: Variable) -> Value {
            self.0
                .iter()
                .find(|(key, _)| *key == variable)
                .map_or(Value::Null, |(_, value)| value.clone())
        }
    }
    let graph = graphs
        .graph(0x224e)
        .ok_or("Flashbang effect graph unavailable")?;
    let condition = graph
        .node(4)
        .and_then(|node| node.field("m_condition"))
        .ok_or("Flashbang entry predicate unavailable")?;
    let variables = condition
        .get("m_expression")
        .and_then(|v| v.get("m_dynamicVars"))
        .and_then(Json::as_array)
        .ok_or("Flashbang entry variables unavailable")?;
    let schema = graph
        .fields
        .get("m_publicSchema")
        .and_then(|v| v.get("m_entries"))
        .and_then(Json::as_array)
        .ok_or("Flashbang entry defaults unavailable")?;
    let overrides = graphs
        .graph(0x0228)
        .and_then(|g| g.node(7))
        .and_then(|n| n.field("m_69F4B52C.m_6525E98B.m_1EB5A024"))
        .and_then(Json::as_array)
        .ok_or("Flashbang wrapper overrides unavailable")?;
    let mut inputs = Inputs(Vec::new());
    for config in variables {
        let variable = crate::script_expr::dynamic(config)
            .map_err(|e| e.to_string())?
            .ok_or("Flashbang entry variable unavailable")?;
        let value = match variable.scope {
            Scope::Entity => entity_value(variable).ok_or("Flashbang target component halted")?,
            Scope::Instance => {
                let identifier = 0x0d80_0000_0000_0000 | u64::from(variable.id);
                // The admitted 0228 path uses schema defaults for this guard.
                // A changed wrapper must not silently bypass an override.
                if overrides
                    .iter()
                    .any(|row| row.get("m_0D09D2D9").and_then(asset_guid) == Some(identifier))
                {
                    return Err("Flashbang entry override requires evaluation".into());
                }
                let default = schema
                    .iter()
                    .find(|row| row.get("m_0D09D2D9").and_then(asset_guid) == Some(identifier))
                    .and_then(|row| row.get("m_value"))
                    .ok_or("Flashbang entry default unavailable")?;
                crate::script_expr::evaluate(default, &mut inputs).map_err(|e| e.to_string())?
            }
        };
        inputs.0.push((variable, value));
    }
    crate::script_expr::evaluate(condition, &mut inputs)
        .map(|value| crate::script_expr::truthy(&value))
        .map_err(|e| e.to_string())
}

pub(super) fn flashbang_stack_admits(
    body: &Component,
    graphs: &GraphStore,
) -> Result<bool, String> {
    let config = graphs
        .graph(0x224e)
        .and_then(|graph| graph.node(122))
        .and_then(|node| node.field("m_condition"))
        .filter(|config| config["$"] == "STU_3ACE35FB")
        .ok_or("Flashbang control-stack predicate unavailable")?;
    let variable = crate::script_expr::dynamic(&config["m_E50CF556"])
        .map_err(|error| error.to_string())?
        .filter(|variable| variable.scope == crate::script_expr::Scope::Entity)
        .ok_or("Flashbang control-stack target unavailable")?;
    let priority = crate::script_expr::stack_priority(&config["m_012B7AF7"])
        .map_err(|error| error.to_string())?;
    body.permits_entity_stack(variable.id, priority)
        .ok_or_else(|| "Flashbang target component halted".into())
}

pub(super) fn flashbang_exclusions_admit(
    filter: &Json,
    presence: impl FnMut(u64) -> Option<bool>,
) -> Result<bool, String> {
    if filter["$"] != "STUConfigVarFilter" {
        return Err("Flashbang exclusion filter unavailable".into());
    }
    // Only this independent tag clause is checked here. Native actor category
    // initialization remains separate from the room timed-control policy.
    crate::script_expr::excluded_tags_admit(&filter["m_8B2A4DF3"], presence)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Flashbang target tags unavailable".into())
}

impl EnemyScripts {
    pub(crate) fn flashbang_control_allowed(
        &self,
        graphs: &GraphStore,
        binding: &EnemyBinding,
    ) -> Result<bool, String> {
        if !flashbang_entry_admits(graphs, |variable| self.body.resolved_variable(0, variable))? {
            return Ok(false);
        }
        if !flashbang_stack_admits(&self.body, graphs)? {
            return Ok(false);
        }
        let filter = graphs
            .graph(0x224e)
            .and_then(|graph| graph.node(9))
            .and_then(|node| node.field("m_condition.m_filter"))
            .ok_or("Flashbang exclusion filter unavailable")?;
        flashbang_exclusions_admit(filter, |tag| self.body_has_target_tag(graphs, binding, tag))
    }

    /// Queue the admitted Flashbang wrapper before its hit entry. Normal pair
    /// delivery owns VM execution, schema overrides and observer state lifetime.
    pub(crate) fn queue_flashbang_effect(&mut self, binding: &EnemyBinding, caller: Site) {
        if !is_junkenstein_mode(binding.mode) || (caller.graph, caller.node) != (0x01e9, Some(7)) {
            return;
        }
        self.messages.push_back(PairDelivery {
            target: binding.body,
            input: PairInput::PlayScript {
                caller,
                request: PlayScript {
                    policy: 2,
                    overrides: vec![],
                    graph: 0x0228,
                    source: caller.entity,
                    parameters: vec![],
                    auto_retire: false,
                },
            },
        });
        self.messages.push_back(PairDelivery {
            target: binding.body,
            input: PairInput::Message {
                sender: caller.entity,
                message: 0x0240_0000_0000_000f,
                parameters: Variables::new(),
                required_graph: Some(0x0228),
            },
        });
    }

    pub(crate) fn set_control_blocked(&mut self, blocked: bool) {
        self.control_records.control_blocked = blocked;
    }

    pub(crate) fn take_velocity_changes(&mut self) -> Vec<(u32, crate::mover::VelocityChange)> {
        std::mem::take(&mut self.control_records.velocity_changes)
    }

    pub(crate) fn movement_speed_scale(&self, frame: u32) -> f64 {
        self.movement_modifiers.at(frame).intent(0, 1.0).max(0.0)
    }

    fn refresh_movement_modifiers(
        &mut self,
        graphs: &GraphStore,
        binding: &EnemyBinding,
        vitals: &mut Vitals,
        frame: u32,
    ) -> Result<(), String> {
        let empty = Variables::new();
        let mut world = EnemyWorld {
            retirement_requested: &mut self.retirement_requested,
            messages: &mut self.messages,
            target_tag_effects: &mut self.target_tag_effects,
            behavior_records: &mut self.behavior_records,
            control_records: &mut self.control_records,
            binding,
            graphs,
            vitals,
            body_variables: &empty,
            controller_variables: &self
                .controller
                .snapshot()
                .ok_or("enemy controller halted")?
                .variables,
        };
        let modifiers = self
            .body
            .movement_modifiers(&mut world)
            .map_err(|error| error.to_string())?
            .ok_or("enemy movement modifier projection is unsupported")?;
        self.movement_modifiers.track(frame, &modifiers);
        Ok(())
    }
    pub(crate) fn teardown(
        &mut self,
        graphs: &GraphStore,
        binding: &EnemyBinding,
        vitals: &mut Vitals,
    ) -> Result<(), String> {
        let empty = Variables::new();
        {
            let other = &self
                .controller
                .snapshot()
                .ok_or("controller teardown halted")?
                .variables;
            let mut world = EnemyWorld {
                retirement_requested: &mut self.retirement_requested,
                messages: &mut self.messages,
                target_tag_effects: &mut self.target_tag_effects,
                behavior_records: &mut self.behavior_records,
                control_records: &mut self.control_records,
                binding,
                graphs,
                vitals: &mut *vitals,
                body_variables: &empty,
                controller_variables: other,
            };
            self.body
                .teardown(&mut world)
                .map_err(|error| error.to_string())?;
        }
        let other = &self
            .body
            .snapshot()
            .ok_or("body teardown halted")?
            .variables;
        let mut world = EnemyWorld {
            retirement_requested: &mut self.retirement_requested,
            messages: &mut self.messages,
            target_tag_effects: &mut self.target_tag_effects,
            behavior_records: &mut self.behavior_records,
            control_records: &mut self.control_records,
            binding,
            graphs,
            vitals: &mut *vitals,
            body_variables: other,
            controller_variables: &empty,
        };
        self.controller
            .teardown(&mut world)
            .map_err(|error| error.to_string())?;
        self.deliver_messages(graphs, binding, vitals)
    }
    pub(crate) fn take_scene_messages(&mut self) -> Vec<SceneMessage> {
        std::mem::take(&mut self.scene_messages)
    }

    #[cfg(test)]
    pub(crate) fn ai_control_setting(&self) -> Option<(f32, bool)> {
        self.control_records.ai_control_setting
    }

    pub(crate) fn take_script_requests(&mut self) -> Vec<crate::game_combat::RoomScriptRequest> {
        self.control_records.script_requests.take()
    }

    pub(crate) fn receive_room_message(
        &mut self,
        sender: u32,
        message: RoomMessage,
    ) -> Result<usize, InitFault> {
        self.body.queue_room_message_from(sender, message)
    }

    pub(crate) fn take_scripted_damage(&mut self) -> Vec<EnemyDamage> {
        std::mem::take(&mut self.control_records.scripted_damage)
    }

    pub(crate) fn set_objective_route(&mut self, active: bool) {
        self.control_records.objective_route_active = Some(active);
    }
    pub(crate) fn boss_pause_active(&self) -> bool {
        self.body
            .snapshot()
            .is_some_and(script_init::Snapshot::boss_pause_active)
    }

    pub(crate) fn set_room_boss_pause(&mut self, active: bool) {
        self.control_records.room_boss_pause = active;
    }

    pub(crate) fn boss_intro_finished(&self) -> bool {
        self.body.snapshot().is_some_and(|body| {
            body.instances.values().any(|instance| {
                // The 0CE0:4 wait sends 06AC as it ends.
                instance.graph == 0x0ce0
                    && instance
                        .states
                        .get(&3)
                        .is_some_and(|wait| wait.activation != 0 && !wait.active)
            })
        })
    }

    pub(crate) fn boss_combat_handoff(&self, binding: &EnemyBinding) -> bool {
        if self.boss_intro_finished()
            || self.controller.snapshot().is_some_and(|controller| {
                controller.instances.values().any(|instance| {
                    // 0C95:5 is the controller's authored facing state.
                    instance.graph == 0x0c95
                        && instance
                            .states
                            .get(&2)
                            .is_some_and(|state| state.activation != 0 && !state.active)
                })
            })
        {
            return true;
        }
        // The boss-specific branches at 0CBC:31/32/33 bypass the intro
        // when the mode has already introduced this boss and enables skipping.
        let (seen, wave_state) = match binding.hero {
            REAPER => (8520, 6),
            ROADHOG => (8521, 4),
            JUNKRAT => (8522, 5),
            _ => return false,
        };
        [seen, 8566].into_iter().all(|id| {
            binding
                .mode_variables
                .get(&id)
                .is_some_and(crate::script_expr::truthy)
        }) && self.body.snapshot().is_some_and(|body| {
            !body
                .instances
                .values()
                .any(|instance| instance.graph == 0x0ce0)
                && body.instances.values().any(|instance| {
                    instance.graph == 0x0cbc
                        && instance
                            .states
                            .get(&wave_state)
                            .is_some_and(|state| state.active)
                })
        })
    }

    pub(crate) fn set_weapon_target(&mut self, target: Option<BossTarget>) {
        self.control_records.weapon_target = target;
    }

    pub(crate) fn caster_ready(&self) -> bool {
        !self.behavior_records.is_empty()
            && !self.boss_pause_active()
            && self.cinematic_aim().is_none()
            && self.facing_direction().is_none()
    }

    pub(crate) fn set_witch_target_distance(&mut self, distance: Option<f64>) {
        self.control_records.witch_target_distance = distance;
    }

    pub(crate) fn witch_staff_effect(&self) -> Option<(u32, f64)> {
        self.control_records
            .witch_staff_effect
            .filter(|&(target, _)| {
                self.control_records.witch_ally == Some(target)
                    && self.control_records.witch_target_distance.is_some()
            })
    }

    pub(crate) fn record_witch_staff_source(&mut self, body: u32, source: WitchStaffSource) {
        self.control_records
            .witch_staff_sources
            .insert(body, source);
    }

    pub(crate) fn sync_witch_staff(
        &mut self,
        graphs: &GraphStore,
        binding: &EnemyBinding,
        vitals: &mut Vitals,
        detach: Option<(u32, u16)>,
        attach: Option<(u32, f64)>,
    ) -> Result<Option<u16>, String> {
        let mut world = EnemyWorld {
            retirement_requested: &mut self.retirement_requested,
            messages: &mut self.messages,
            target_tag_effects: &mut self.target_tag_effects,
            behavior_records: &mut self.behavior_records,
            control_records: &mut self.control_records,
            binding,
            graphs,
            vitals,
            body_variables: &Variables::new(),
            controller_variables: &self
                .controller
                .snapshot()
                .ok_or("enemy controller halted")?
                .variables,
        };
        if let Some((source, instance)) = detach {
            self.body
                .detach_witch_staff(instance, &mut world)
                .map_err(|error| error.to_string())?;
            world
                .control_records
                .witch_staff_sources
                .shift_remove(&source);
        }
        let instance = attach
            .map(|(source, amount)| self.body.attach_witch_staff(source, amount, &mut world))
            .transpose()
            .map_err(|error| error.to_string())?;
        let body = self.body.snapshot().ok_or("enemy body halted")?;
        if !body.initialization_complete() {
            return Err(format!("enemy body: {:?}", body.diagnostics));
        }
        self.deliver_messages(graphs, binding, vitals)?;
        Ok(instance)
    }

    pub(crate) fn publish_witch_targets(
        &mut self,
        frame: u32,
        enemy: Option<u32>,
        ally: Option<u32>,
    ) -> Result<(), script_init::InitFault> {
        self.control_records.weapon_target = enemy.map(|entity| BossTarget {
            entity,
            secondary: false,
        });
        self.control_records.witch_ally = ally;
        self.publish_weapon_targets(frame, 0x0cd2, [(0, 2299, enemy), (1, 7126, ally)])
    }

    pub(crate) fn summoner_reaches(&self) -> Option<[f64; 2]> {
        let controller = self
            .controller
            .snapshot()?
            .instances
            .values()
            .find(|i| i.graph == 0x185d)?;
        let [primary, secondary] = [7198, 4074].map(|id| {
            controller
                .variables
                .get(&id)
                .map(crate::script_expr::to_float)
        });
        let [Some(primary), Some(secondary)] = [primary, secondary] else {
            return None;
        };
        (primary.is_finite() && primary > 0.0 && secondary.is_finite() && secondary > 0.0)
            .then_some([primary, secondary])
    }

    pub(crate) fn summoner_beam_reach(&self) -> Option<f64> {
        let (&id, _) = self
            .body
            .snapshot()?
            .instances
            .iter()
            .find(|(_, instance)| instance.graph == 0x157c)?;
        let variable = crate::script_expr::Variable {
            scope: crate::script_expr::Scope::Instance,
            id: 1648,
        };
        let reach = crate::script_expr::to_float(&self.body.resolved_variable(id, variable)?);
        (reach.is_finite() && reach > 0.0).then_some(reach)
    }

    pub(crate) fn set_summoner_rays(&mut self, rays: [BeamContact; 2]) {
        self.control_records.summoner_rays = rays;
    }

    pub(crate) fn publish_summoner_targets(
        &mut self,
        frame: u32,
        [primary, secondary]: [Option<u32>; 2],
        target: Option<BossTarget>,
    ) -> Result<(), script_init::InitFault> {
        self.control_records.summoner_targets = [primary, secondary];
        self.control_records.weapon_target = target;
        self.publish_weapon_targets(frame, 0x185d, [(0, 8066, primary), (1, 11652, secondary)])
    }

    fn publish_weapon_targets(
        &mut self,
        frame: u32,
        graph: u32,
        targets: [(i64, u32, Option<u32>); 2],
    ) -> Result<(), script_init::InitFault> {
        let Some((id, updates)) = self.controller.snapshot().and_then(|snapshot| {
            let (&id, controller) = snapshot.instances.iter().find(|(_, i)| i.graph == graph)?;
            let updates = targets.map(|(slot, variable, target)| {
                let current = match controller.variables.get(&variable) {
                    Some(Value::Entity(entity)) if crate::script_expr::valid_entity(*entity) => {
                        Some(*entity)
                    }
                    _ => None,
                };
                (current != target).then_some((slot, target.unwrap_or(0)))
            });
            Some((id, updates))
        }) else {
            return Ok(());
        };
        for (slot, entity) in updates.into_iter().flatten() {
            self.controller.queue_frame_message(
                frame,
                id,
                0x0240_0000_0000_0079,
                Variables::from([(0x57, Value::Integer(slot)), (0x59, Value::Entity(entity))]),
            )?;
        }
        Ok(())
    }

    pub(crate) fn grenade_arc(
        &self,
        graphs: &GraphStore,
        binding: &EnemyBinding,
    ) -> Option<GrenadeArc> {
        let (graph, volley) = match binding.hero {
            JUNKRAT
                if self.ordinary_boss_weapon(binding)
                    && self
                        .controller
                        .snapshot()?
                        .instances
                        .values()
                        .any(|instance| {
                            instance.graph == 0x0cb5
                                && instance.states.get(&11).is_some_and(|state| state.active)
                        }) =>
            {
                (0x022b, 31)
            }
            ZOMBARDIER => (0x0ca1, 14),
            _ => return None,
        };
        let body = self.body.snapshot()?;
        let (&instance, _) = body
            .instances
            .iter()
            .find(|(_, instance)| instance.graph == graph)?;
        let node = graphs.graph(graph)?.node(volley)?;
        let resolve = |config: &Json| match crate::script_expr::dynamic(config)? {
            Some(variable) => self
                .body
                .resolved_variable(instance, variable)
                .ok_or(ShotConfigError::MalformedConfig("grenade setting")),
            None => crate::script_expr::evaluate(config, &mut crate::script_expr::EmptyContext),
        };
        let speed = crate::script_expr::to_float(&resolve(node.field("m_projectileSpeed")?).ok()?);
        let range = crate::script_expr::to_float(&resolve(node.field("m_FEC435C6")?).ok()?);
        let gravity = authored_gravity(&node.fields, resolve)?;
        (speed.is_finite() && speed > 0.0 && range.is_finite() && range > 0.0).then_some(
            GrenadeArc {
                speed,
                gravity,
                range,
            },
        )
    }

    pub(crate) fn outgoing_damage(&self, amount: f64, tags: &[u64]) -> f64 {
        let adjustment = self
            .control_records
            .outgoing_damage_filters
            .values()
            .filter(|filter| filter.excluded_tags.iter().all(|tag| !tags.contains(tag)))
            .map(|filter| filter.adjustment)
            .sum::<f64>();
        if adjustment == 0.0 {
            amount
        } else {
            let scale = ((1.0 + adjustment) as f32).max(0.0);
            f64::from((amount * f64::from(scale)) as f32)
        }
    }

    pub(crate) fn take_weapon_shots(
        &mut self,
        graphs: &GraphStore,
        binding: &EnemyBinding,
        vitals: &mut Vitals,
        frame: u32,
    ) -> EnemyWeaponShots {
        let shots = self.body.take_shots().unwrap_or_default();
        if self.control_records.control_blocked {
            return Default::default();
        }
        let mut weapons = EnemyWeaponShots::default();
        if shots.is_empty() || vitals.death().is_some() {
            return weapons;
        }
        let Some(controller) = self.controller.snapshot() else {
            return weapons;
        };
        let empty = Variables::new();
        let mut world = EnemyWorld {
            retirement_requested: &mut self.retirement_requested,
            messages: &mut self.messages,
            target_tag_effects: &mut self.target_tag_effects,
            behavior_records: &mut self.behavior_records,
            control_records: &mut self.control_records,
            binding,
            graphs,
            vitals,
            body_variables: &empty,
            controller_variables: &controller.variables,
        };
        let [x, y, z] = binding.pose.position;
        let origin = [
            x,
            y + crate::game_combat::eye_height(binding.hero, false),
            z,
        ];
        let forward = aim_direction(&Command {
            yaw: binding.pose.yaw,
            pitch: binding.pose.pitch,
            ..Command::default()
        });
        for shot in shots {
            let Some(node) = self.body.shot_node(&shot) else {
                continue;
            };
            let mut evaluate =
                |config: &Json| self.body.evaluate_shot_config(&shot, config, &mut world);
            match (binding.hero, shot.graph, node.pos) {
                (REAPER, 0x0019, 10) => {
                    let Some(reach) = node
                        .field("m_FEC435C6")
                        .and_then(|v| evaluate(v).ok())
                        .map(|v| crate::script_expr::to_float(&v))
                        .filter(|v| v.is_finite() && *v > 0.0)
                    else {
                        continue;
                    };
                    let Some(direct) = node.field("m_6396149F") else {
                        continue;
                    };
                    let Ok(Some(hit)) = HitDefinition::from_authored(direct, &mut evaluate) else {
                        continue;
                    };
                    let Some(directions) =
                        shotgun_directions(&node.fields, forward, &shot, node.pos, &mut evaluate)
                    else {
                        continue;
                    };
                    weapons.hitscan.push(EnemyHitscanVolley {
                        damage_tags: damage_tags(direct, &mut evaluate).collect(),
                        pellets: directions
                            .into_iter()
                            .map(|direction| HitscanShot {
                                origin,
                                direction,
                                reach,
                                hit,
                            })
                            .collect(),
                    });
                }
                (ROADHOG, 0x01c6, 26 | 29)
                | (JUNKRAT, 0x022b, 31)
                | (ZOMBARDIER, 0x0ca1, 14)
                | (WITCH, 0x006d, 10)
                | (SUMMONER, 0x157c, 85) => {
                    let Some(mut pattern) =
                        projectile_pattern(&node.fields, forward, &shot, node.pos, &mut evaluate)
                    else {
                        continue;
                    };
                    if matches!(binding.hero, JUNKRAT | ZOMBARDIER) {
                        let Some(gravity) = authored_gravity(&node.fields, &mut evaluate) else {
                            continue;
                        };
                        pattern.gravity = Some(gravity);
                    }
                    let mut projectiles = pattern.launch(binding.body, binding.team, origin, frame);
                    if (binding.hero, node.pos) == (ROADHOG, 29) {
                        let Some(mut projectile) = projectiles.next() else {
                            continue;
                        };
                        if let Some(transition) = node
                            .field("m_747ECB51")
                            .filter(|v| v["$"] == "STU_DB5FDC79")
                        {
                            projectile.split = projectile_pattern(
                                &transition["m_volley"],
                                forward,
                                &shot,
                                node.pos,
                                &mut evaluate,
                            );
                        }
                        weapons.projectiles.push(projectile);
                    } else {
                        weapons.projectiles.extend(projectiles);
                    }
                }
                _ => {}
            }
        }
        weapons
    }

    fn weapon_command(&self, binding: &EnemyBinding, frame: u32) -> Command {
        let secondary = self
            .control_records
            .weapon_target
            .is_some_and(|target| target.secondary);
        let (graph, button_state, button) = match (binding.hero, secondary) {
            (REAPER, _) => (0x0019, 11, FIRE),
            (ROADHOG, false) => (0x01c6, 8, FIRE),
            (ROADHOG, true) => (0x01c6, 11, SECONDARY),
            (JUNKRAT, _) => (0x022b, 8, FIRE),
            (ZOMBARDIER, _) => (0x0ca1, 10, SECONDARY),
            _ => unreachable!("hero has no authored weapon input"),
        };
        let listening = self.body.snapshot().is_some_and(|body| {
            body.instances.values().any(|instance| {
                instance.graph == graph
                    && instance
                        .states
                        .get(&button_state)
                        .is_some_and(|state| state.active)
            })
        });
        Command {
            frame,
            yaw: binding.pose.yaw,
            pitch: binding.pose.pitch,
            buttons: if self.control_records.weapon_target.is_some()
                && (binding.hero == ZOMBARDIER || self.ordinary_boss_weapon(binding))
                && (listening || self.body.command_buttons() & button != 0)
            {
                button
            } else {
                0
            },
            ..Command::default()
        }
    }

    fn witch_weapon_command(&mut self, binding: &EnemyBinding, frame: u32) -> Command {
        let mut command = Command {
            frame,
            yaw: binding.pose.yaw,
            pitch: binding.pose.pitch,
            ..Command::default()
        };
        if !self.caster_ready() || self.control_records.room_boss_pause {
            return command;
        }
        let (raw, action, graph, trigger, weapon_override) =
            if self.control_records.witch_ally.is_some() {
                (29, 1, 0x006b, 50, 9)
            } else {
                (31, 2, 0x006d, 5, 12)
            };
        let activation = self.controller.snapshot().and_then(|snapshot| {
            snapshot
                .instances
                .values()
                .find(|instance| instance.graph == 0x0cd2)?
                .states
                .get(&raw)
                .filter(|state| state.active)
                .map(|state| state.activation)
        });
        if let Some(activation) = activation {
            let request = (raw, activation);
            if self.control_records.witch_switch != Some(request) {
                self.control_records.witch_switch = Some(request);
                command.action = action;
            }
        }
        if command.action == 0
            && self.control_records.witch_target_distance.is_some()
            && self.control_records.weapons.keys().any(|site| {
                (site.entity, site.graph, site.node)
                    == (binding.controller, 0x0cd2, Some(weapon_override))
            })
            && self.body.snapshot().is_some_and(|body| {
                body.instances.values().any(|instance| {
                    instance.graph == graph
                        && (instance
                            .states
                            .get(&trigger)
                            .is_some_and(|state| state.active)
                            || self.body.command_buttons() & FIRE != 0)
                })
            })
        {
            command.buttons = FIRE;
        }
        command
    }

    fn summoner_weapon_command(&self, binding: &EnemyBinding, frame: u32) -> Command {
        let buttons = self
            .control_records
            .weapon_target
            .filter(|_| self.caster_ready() && !self.control_records.room_boss_pause)
            .map_or(0, |target| {
                let (node, button) = if target.secondary {
                    (21, SECONDARY)
                } else {
                    (18, FIRE)
                };
                if self.control_records.weapons.keys().any(|site| {
                    (site.entity, site.graph, site.node) == (binding.controller, 0x185d, Some(node))
                }) {
                    button
                } else {
                    0
                }
            });
        Command {
            frame,
            yaw: binding.pose.yaw,
            pitch: binding.pose.pitch,
            buttons,
            ..Command::default()
        }
    }

    fn ordinary_boss_weapon(&self, binding: &EnemyBinding) -> bool {
        let mut body_record = false;
        let mut grenade_record = false;
        for (site, record) in self
            .control_records
            .weapons
            .iter()
            .filter(|(_, record)| record.slot == 0)
        {
            if record.target != binding.body
                || record.identifier != 0x0d80_0000_0000_023a
                || record.enabled
            {
                return false;
            }
            match (site.entity, site.graph, site.node) {
                (entity, 0x08b6, Some(3)) if entity == binding.body && !body_record => {
                    body_record = true
                }
                (entity, 0x0cb5, Some(17)) if entity == binding.controller && !grenade_record => {
                    grenade_record = true
                }
                _ => return false,
            }
        }
        body_record && grenade_record == (binding.hero == JUNKRAT)
    }

    pub(crate) fn cinematic_aim(&self) -> Option<[i32; 2]> {
        self.control_records
            .cinematic_aim
            .last()
            .map(|(_, &aim)| aim)
    }

    pub(crate) fn facing_direction(&self) -> Option<[f64; 3]> {
        self.control_records
            .facing
            .values()
            .fold(None, |winner: Option<&script_init::AiFacing>, record| {
                if winner.is_none_or(|winner| {
                    record.priority.level > winner.priority.level
                        || (record.priority.level == winner.priority.level
                            && record.priority.above_equals)
                }) {
                    Some(record)
                } else {
                    winner
                }
            })
            .map(|record| record.direction)
    }

    pub(crate) fn queue_wave_script(
        &mut self,
        binding: &EnemyBinding,
        caller: Site,
        request: PlayScript,
    ) -> bool {
        if (caller.entity, caller.graph, caller.node, request.graph)
            != (GAME_MODE_ENTITY, 0x0cba, Some(9), 0x0c95)
            || !is_junkenstein_mode(binding.mode)
            || self.messages.len() >= MAX_INIT_WORK
        {
            return false;
        }
        self.messages.push_back(PairDelivery {
            target: binding.controller,
            input: PairInput::PlayScript { caller, request },
        });
        true
    }

    #[cfg(test)]
    pub(crate) fn controller_display_name(&self) -> Option<u64> {
        self.control_records
            .display_names
            .last()
            .map(|(_, &name)| name)
    }

    #[cfg(test)]
    pub(crate) fn behavior_requests(
        &self,
    ) -> impl Iterator<Item = &script_init::AiBehaviorRequest> {
        self.behavior_records.values()
    }
    pub(crate) fn queue_common_corpse_retirement(
        &mut self,
        binding: &EnemyBinding,
        vitals: &Vitals,
        frame: u32,
    ) -> Result<(), String> {
        if !is_junkenstein_mode(binding.mode)
            || !crate::junkenstein_host::common_corpse_due(
                binding.hero,
                vitals.death().map(|death| death.frame),
                frame,
            )
        {
            return Ok(());
        }
        let body = self
            .body
            .snapshot()
            .ok_or("common corpse body unavailable")?;
        let controller = self
            .controller
            .snapshot()
            .ok_or("common corpse controller unavailable")?;
        if body.entity != binding.body
            || controller.entity != binding.controller
            || body.entity == controller.entity
            || !body.initialization_complete()
            || !controller.initialization_complete()
        {
            return Err("common corpse pair identity or script state is unavailable".into());
        }
        self.retirement_requested = true;
        Ok(())
    }

    pub(crate) fn queue_death(
        &mut self,
        source: crate::vitals::Death,
        critical: bool,
        loadout: Option<u64>,
    ) -> Result<(), String> {
        self.body.set_owner_dead(true);
        let parameters = Variables::from([
            (0x12, Value::Entity(source.killer)),
            (0x95, Value::Bool(critical)),
            (0x3e6, loadout.map_or(Value::Null, Value::Asset)),
            (0x3e7, Value::Array(vec![])),
        ]);
        const DIED: u64 = 0x0240_0000_0000_003e;
        if u64::from(source.frame) * crate::script_runtime::FRAME_MS >= self.body.now_ms() {
            self.body
                .queue_frame_message_from(source.frame, source.killer, DIED, parameters)
        } else {
            // Damage may land after this body's same-frame script update.
            self.body
                .queue_entity_message_from(source.killer, DIED, parameters)
        }
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

    pub fn advance(
        &mut self,
        graphs: &GraphStore,
        binding: &EnemyBinding,
        vitals: &mut Vitals,
        frame: u32,
    ) -> Result<(), String> {
        self.body.set_owner_dead(vitals.death().is_some());
        let command = match binding.hero {
            WITCH => Some(self.witch_weapon_command(binding, frame)),
            SUMMONER => Some(self.summoner_weapon_command(binding, frame)),
            REAPER | ROADHOG | JUNKRAT | ZOMBARDIER => Some(self.weapon_command(binding, frame)),
            _ => None,
        };
        let room_pause = self.control_records.room_boss_pause;
        let blocked = self.control_records.control_blocked;
        let empty = Variables::new();
        {
            let mut world = EnemyWorld {
                retirement_requested: &mut self.retirement_requested,
                messages: &mut self.messages,
                target_tag_effects: &mut self.target_tag_effects,
                behavior_records: &mut self.behavior_records,
                control_records: &mut self.control_records,
                binding,
                graphs,
                vitals: &mut *vitals,
                body_variables: &empty,
                controller_variables: &self
                    .controller
                    .snapshot()
                    .ok_or("enemy controller halted")?
                    .variables,
            };
            self.body
                .set_control_blocked(blocked, &mut world)
                .map_err(|error| error.to_string())?;
            match command {
                Some(command) => self.body.advance_command(command, &mut world),
                None => self.body.advance_frame(frame, &mut world),
            }
            .map_err(|error| error.to_string())?;
            self.body
                .refresh_boss_pause(&mut world, room_pause)
                .map_err(|error| error.to_string())?;
        }
        let body = self.body.snapshot().ok_or("enemy body halted")?;
        if !body.initialization_complete() {
            return Err(format!("enemy body: {:?}", body.diagnostics));
        }
        // Health services may have killed this body during its own update.
        self.body.set_owner_dead(vitals.death().is_some());
        let body = self.body.snapshot().ok_or("enemy body halted")?;
        let mut world = EnemyWorld {
            retirement_requested: &mut self.retirement_requested,
            messages: &mut self.messages,
            target_tag_effects: &mut self.target_tag_effects,
            behavior_records: &mut self.behavior_records,
            control_records: &mut self.control_records,
            binding,
            graphs,
            vitals: &mut *vitals,
            body_variables: &body.variables,
            controller_variables: &empty,
        };
        self.controller
            .advance_frame(frame, &mut world)
            .map_err(|error| error.to_string())?;
        let controller = self
            .controller
            .snapshot()
            .ok_or("enemy controller halted")?;
        if !controller.initialization_complete() {
            return Err(format!("enemy controller: {:?}", controller.diagnostics));
        }
        self.deliver_messages(graphs, binding, vitals)?;
        self.refresh_movement_modifiers(graphs, binding, vitals, frame)
    }

    pub(crate) fn has_polled_voice(&self) -> bool {
        self.body.has_polled_voice() || self.controller.has_polled_voice()
    }

    /// Called by the room only after every actor's ordinary component updates.
    pub(crate) fn poll_voice(
        &mut self,
        graphs: &GraphStore,
        binding: &EnemyBinding,
        vitals: &mut Vitals,
    ) -> Result<(), String> {
        let empty = Variables::new();
        for body_phase in [true, false] {
            let (receiver, other) = if body_phase {
                (&mut self.body, &self.controller)
            } else {
                (&mut self.controller, &self.body)
            };
            if !receiver.has_polled_voice() {
                continue;
            }
            let other = &other.snapshot().ok_or("enemy voice peer halted")?.variables;
            let mut world = EnemyWorld {
                retirement_requested: &mut self.retirement_requested,
                messages: &mut self.messages,
                target_tag_effects: &mut self.target_tag_effects,
                behavior_records: &mut self.behavior_records,
                control_records: &mut self.control_records,
                binding,
                graphs,
                vitals: &mut *vitals,
                body_variables: if body_phase { &empty } else { other },
                controller_variables: if body_phase { other } else { &empty },
            };
            receiver
                .poll_voice(&mut world)
                .map_err(|error| error.to_string())?;
            let snapshot = receiver.snapshot().ok_or("enemy voice component halted")?;
            if !snapshot.initialization_complete() {
                return Err(format!("enemy voice: {:?}", snapshot.diagnostics));
            }
        }
        Ok(())
    }

    pub(super) fn deliver_messages(
        &mut self,
        graphs: &GraphStore,
        binding: &EnemyBinding,
        vitals: &mut Vitals,
    ) -> Result<(), String> {
        let empty = Variables::new();
        let mut delivered = 0;
        while let Some(message) = self.messages.pop_front() {
            delivered += 1;
            if delivered > MAX_INIT_WORK {
                return Err("enemy cross-component message budget exhausted".into());
            }
            if message.target == GAME_MODE_ENTITY
                || binding
                    .gate
                    .is_some_and(|gate| gate.entity == message.target)
            {
                let PairInput::Message {
                    message: id,
                    parameters,
                    ..
                } = message.input
                else {
                    return Err("PlayScript cannot target the scene message queue".into());
                };
                self.scene_messages.push(SceneMessage {
                    target: message.target,
                    message: id,
                    parameters,
                });
                continue;
            }
            let (receiver, other) = if message.target == binding.body {
                (&mut self.body, &self.controller)
            } else {
                (&mut self.controller, &self.body)
            };
            let other = &other
                .snapshot()
                .ok_or("enemy message peer halted")?
                .variables;
            let mut world = EnemyWorld {
                retirement_requested: &mut self.retirement_requested,
                messages: &mut self.messages,
                target_tag_effects: &mut self.target_tag_effects,
                behavior_records: &mut self.behavior_records,
                control_records: &mut self.control_records,
                binding,
                graphs,
                vitals: &mut *vitals,
                body_variables: if message.target == binding.body {
                    &empty
                } else {
                    other
                },
                controller_variables: if message.target == binding.controller {
                    &empty
                } else {
                    other
                },
            };
            match message.input {
                PairInput::Message {
                    sender,
                    message,
                    parameters,
                    required_graph,
                } => {
                    if required_graph.is_some_and(|graph| {
                        !receiver.snapshot().is_some_and(|snapshot| {
                            snapshot.initialization_complete()
                                && snapshot
                                    .instances
                                    .values()
                                    .any(|instance| instance.graph == graph)
                        })
                    }) {
                        continue;
                    }
                    receiver
                        .queue_entity_message_from(sender, message, parameters)
                        .map_err(|error| error.to_string())?;
                    receiver.run_now(&mut world)
                }
                PairInput::PlayScript { caller, request } => {
                    receiver.receive_play_script(caller, &request, &mut world)
                }
            }
            .map_err(|error| error.to_string())?;
        }
        // A PlayScript start can already have submitted the peer's work
        // before encountering an unsupported branch on the sender.
        for (kind, receiver) in [("body", &self.body), ("controller", &self.controller)] {
            let snapshot = receiver.snapshot().ok_or("enemy script receiver halted")?;
            if !snapshot.initialization_complete() {
                return Err(format!("enemy {kind} delivery: {:?}", snapshot.diagnostics));
            }
        }
        Ok(())
    }
    /// Nothing is registered in the room until both components have started.
    pub fn start(
        graphs: Arc<GraphStore>,
        definition: &NpcDefinition,
        binding: &EnemyBinding,
        frame: u32,
        vitals: &mut Vitals,
    ) -> Result<Self, String> {
        if definition.hero != binding.hero || definition.body != binding.pair.body {
            return Err("enemy definition differs from the authored pair".into());
        }
        let record = graphs
            .enemy_body(binding.pair.body)
            .ok_or("missing enemy body record")?;
        let control = graphs
            .enemy_body(binding.pair.controller)
            .ok_or("missing enemy controller record")?;
        let initial =
            script_init::player_entity_variables(record, vitals.total(), vitals.maximum());
        let roots = if matches!(binding.hero, REAPER | ROADHOG | JUNKRAT | WITCH | SUMMONER) {
            script_init::player_roots(record, &graphs)
        } else {
            script_init::body_roots(record)
        }
        .map_err(|error| error.to_string())?;
        // The common body graph owns its pools. Keep the profile's initial
        // bindings, then let its live states register each pool exactly once.
        // NPCs using 0C89's capacity binding retain their catalog-owned pools.
        if roots
            .iter()
            .any(|root| root.graph == crate::game_pools::BODY_GRAPH)
        {
            *vitals = Vitals::empty();
        }
        let empty = Variables::new();
        let mut control_records = AiControlRecords::default();
        let mut behavior_records = indexmap::IndexMap::new();
        let mut messages = VecDeque::new();
        let mut retirement_requested = false;
        let mut target_tag_effects = TargetTagEffects::default();
        let mut world = EnemyWorld {
            retirement_requested: &mut retirement_requested,
            messages: &mut messages,
            target_tag_effects: &mut target_tag_effects,
            behavior_records: &mut behavior_records,
            control_records: &mut control_records,
            binding,
            graphs: &graphs,
            vitals,
            body_variables: &initial,
            controller_variables: &empty,
        };
        let body = Component::start_with_public_schema(
            Arc::clone(&graphs),
            binding.body,
            initial.clone(),
            roots,
            frame,
            &mut world,
            MAX_INIT_WORK,
        )
        .map_err(|error| error.to_string())?;
        let snapshot = body.snapshot().ok_or("enemy body startup halted")?;
        if !snapshot.initialization_complete() {
            return Err(format!("enemy body startup: {:?}", snapshot.diagnostics));
        }
        world.body_variables = &snapshot.variables;
        let controller = Component::start_with_public_schema(
            Arc::clone(&graphs),
            binding.controller,
            Variables::new(),
            script_init::body_roots(control).map_err(|error| error.to_string())?,
            frame,
            &mut world,
            MAX_INIT_WORK,
        )
        .map_err(|error| error.to_string())?;
        let snapshot = controller
            .snapshot()
            .ok_or("enemy controller startup halted")?;
        if !snapshot.initialization_complete() {
            return Err(format!(
                "enemy controller startup: {:?}",
                snapshot.diagnostics
            ));
        }
        let mut scripts = Self {
            body,
            controller,
            control_records,
            behavior_records,
            target_tag_effects,
            messages,
            scene_messages: Vec::new(),
            retirement_requested,
            movement_modifiers: Default::default(),
        };
        scripts.deliver_messages(&graphs, binding, vitals)?;
        scripts.refresh_movement_modifiers(&graphs, binding, vitals, frame)?;
        Ok(scripts)
    }
}

type ShotConfigError = crate::script_expr::EvalError;

fn damage_tags<'a>(
    direct: &'a Json,
    evaluate: &'a mut impl FnMut(&Json) -> Result<Value, ShotConfigError>,
) -> impl Iterator<Item = u64> + 'a {
    direct["m_modifyHealth"]["m_148F3152"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| match evaluate(&entry["m_EE3055BD"]).ok()? {
            Value::Asset(tag) => Some(tag),
            _ => None,
        })
}

fn projectile_pattern(
    fields: &Json,
    forward: [f64; 3],
    shot: &script_init::ShotEvent,
    position: u16,
    evaluate: &mut impl FnMut(&Json) -> Result<Value, ShotConfigError>,
) -> Option<ProjectilePattern> {
    let ballistics = ProjectileBallistics::from_fields(fields, &mut *evaluate).ok()??;
    let directions = match fields["m_5EC1EB38"]["$"].as_str()? {
        "STU_613BDDB0" => shotgun_directions(fields, forward, shot, position, evaluate)?,
        "STU_239B571E" => vec![forward],
        _ => return None,
    };
    Some(ProjectilePattern {
        ballistics,
        gravity: None,
        directions,
        damage_tags: damage_tags(&fields["m_6396149F"], evaluate).collect(),
    })
}

fn shotgun_directions(
    fields: &Json,
    forward: [f64; 3],
    shot: &script_init::ShotEvent,
    position: u16,
    evaluate: &mut impl FnMut(&Json) -> Result<Value, ShotConfigError>,
) -> Option<Vec<[f64; 3]>> {
    use crate::script_expr::{to_float, to_int};

    let launch = &fields["m_5EC1EB38"];
    if launch["$"] != "STU_613BDDB0" {
        return None;
    }
    let cone = to_float(&evaluate(&launch["m_spread"]).ok()?);
    let count = to_int(&evaluate(&fields["m_numProjectilesPerShot"]).ok()?, 0);
    if !(0.0..90.0).contains(&cone) || !(1..=32).contains(&count) {
        return None;
    }
    let rings: Vec<_> = launch["m_0D30E40C"]
        .as_array()?
        .iter()
        .map(|row| {
            let radius = to_float(&evaluate(&row["m_6425FFB9"]).ok()?);
            let size = to_int(&evaluate(&row["m_B19BD35D"]).ok()?, 0);
            ((0.0..=1.0).contains(&radius) && (1..=count).contains(&size))
                .then_some((radius, size as usize))
        })
        .collect::<Option<_>>()?;
    (rings.iter().map(|(_, size)| size).sum::<usize>() == count as usize).then(|| {
        crate::weapon_spread::ring_directions(forward, cone.to_radians(), &rings, shot, position)
            .collect()
    })
}

impl EnemyScripts {
    pub(crate) fn take_defender_messages(&mut self) -> Vec<DefenderMessage> {
        std::mem::take(&mut self.control_records.defender_messages)
    }

    pub(crate) fn take_voice_requests(&mut self) -> Vec<VoiceRequest> {
        std::mem::take(&mut self.control_records.voice_requests)
    }

    pub(crate) fn take_boss_intro_starts(&mut self) -> Vec<Variables> {
        std::mem::take(&mut self.control_records.boss_intro_starts)
    }

    pub(crate) fn target_tags(
        &self,
        graphs: &GraphStore,
        binding: &EnemyBinding,
    ) -> script_init::TargetTagSnapshot {
        self.target_tag_effects.snapshot(
            graphs
                .enemy_body(binding.pair.body)
                .and_then(|body| body.target_tags.as_ref()),
        )
    }

    pub(crate) fn body_has_target_tag(
        &self,
        graphs: &GraphStore,
        binding: &EnemyBinding,
        tag: u64,
    ) -> Option<bool> {
        body_has_target_tag(graphs, binding, &self.target_tag_effects, tag)
    }

    pub(crate) fn set_defenders(&mut self, defenders: Arc<[u32]>) {
        self.control_records.defenders = defenders;
    }

    pub(crate) fn take_sightings(&mut self) -> Vec<DefenderSighting> {
        std::mem::take(&mut self.control_records.sightings)
    }
}

pub(super) fn body_has_target_tag(
    graphs: &GraphStore,
    binding: &EnemyBinding,
    effects: &TargetTagEffects,
    tag: u64,
) -> Option<bool> {
    effects.presence(tag).or_else(|| {
        let tags = graphs.enemy_body(binding.pair.body)?.target_tags.as_ref()?;
        Some(tags.contains(&tag))
    })
}

#[cfg(test)]
pub(crate) fn fixture(
    fail_controller: bool,
) -> (
    Arc<GraphStore>,
    Arc<crate::script_codecs::CodecTable>,
    NpcDefinition,
    script_init::EntityCreation,
) {
    fixture_with_graphs(fail_controller, &[])
}

#[cfg(test)]
pub(super) fn fixture_with_graphs(
    fail_controller: bool,
    extra: &[Json],
) -> (
    Arc<GraphStore>,
    Arc<crate::script_codecs::CodecTable>,
    NpcDefinition,
    script_init::EntityCreation,
) {
    use serde_json::json;
    let graph = |index, fail| {
        json!({"index":index,"items":if fail {2}else{0},
        "nodes":if fail {json!([{"pos":0,"class":"STUStatescriptEntry","uid":0,"rid":0,"remote":-1,
            "state":null,"client_only":false,"server_only":false,"fields":{"m_outPlug":{"$":"Plug","links":[[1,"m_inPlug"]],"m_FF3DAF1E":[]}}},
            {"pos":1,"class":"UnsupportedFixtureAction","uid":1,"rid":1,"remote":-1,
            "state":null,"client_only":false,"server_only":false,"fields":{"m_ADEB6E05":true}}
            ])}else{json!([])},
        "states":[],"entries":if fail {json!([0])}else{json!([])},"remote":[],"sync_vars":[],"containers":{},
        "unknown_nodes":[],"fields":{},"header":{"m_nodesBitCount":4,"m_statesBitCount":4,
        "m_remoteSyncNodesBitCount":4,"m_syncVarsBitCount":4}})
    };
    let body = json!({"hero":"0x02E0000000000175","name":"Fixture","manager":null,"weapons":[],
        "graphs":[{"graph":42,"m_1EB5A024":null,"m_75408640":0}],"target_tags":[]});
    let control = json!({"hero":null,"name":"Fixture control","manager":null,"weapons":[],
        "ai_weapon_identifiers":{"0":[0x0d80_0000_0000_023au64]},
        "graphs":[{"graph":43,"m_1EB5A024":null,"m_75408640":0}],"target_tags":[]});
    let mut data = json!({"version":1,"graphs":[graph(42,false),graph(43,fail_controller)],"bodies":{},"entities":{},"missing":[],
        "junkenstein":{"pairs":{"0x02E0000000000175":{"body":"0x04000000000016BE","controller":"0x04000000000016BF"}},
        "entities":{"0x04000000000016BE":body,"0x04000000000016BF":control}}});
    data["graphs"]
        .as_array_mut()
        .unwrap()
        .extend_from_slice(extra);
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&data).unwrap()).unwrap());
    let codecs =
        Arc::new(crate::script_codecs::CodecTable::from_json(r#"{"classes":{}}"#).unwrap());
    let definition = NpcDefinition {
        hero: 0x02e0_0000_0000_0175,
        body: 0x0400_0000_0000_16be,
        name: "Fixture".into(),
        pools: crate::game_pools::HealthPools {
            health: 200.0,
            armour: 50.0,
            shields: 25.0,
        },
    };
    let request = script_init::EntityCreation {
        position: [1.0, 2.0, 3.0],
        kind: script_init::CreationKind::Hero {
            hero: definition.hero,
            team: 1,
            yaw: 90.0,
            flags: [Value::Bool(false), Value::Bool(false)],
            controller: None,
            skin: Some(0x0b20_0000_0000_0010),
        },
    };
    (graphs, codecs, definition, request)
}
