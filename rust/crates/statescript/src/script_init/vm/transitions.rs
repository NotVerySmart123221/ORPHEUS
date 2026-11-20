//! StateScript state ticking, timers, watches, switches, registration, and teardown.

use std::num::NonZeroU32;

use serde_json::Value as Json;

use crate::{
    script_expr::{self, Scope, equal, truthy},
    script_graph::{Node, asset_guid},
    script_runtime::frame_start,
    script_wire::{Value, Variables},
};

use super::super::{
    ABILITY_HELD, MAX_PLUG_DEPTH, SUBGRAPH_STATES, SWITCH_TO_FALSE, SWITCH_TO_TRUE,
    machine::{Event, RegisteredClass, Startup, StateKey, VarKey},
    shapes::{
        array_field, branch_path, case_path, configured, field, is_unlinked_plug, pool_number,
        raw_indices, registered_state, source_key,
    },
    types::{InitFault, Issue, StateKind},
    world::{Site, World},
};
use super::Vm;

impl<'graph, 'world, 'machine, W: World> Vm<'graph, 'world, 'machine, W> {
    pub(in crate::script_init) fn tick_states(&mut self, id: u16) -> Result<(), InitFault> {
        let now = self.machine.now_ms;
        let due: Vec<_> = self
            .machine
            .ticking
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|(&raw, &last)| (last != now).then_some(raw))
            .collect();
        for raw in due {
            self.spend("polled state tick")?;
            if let Some(last) = self
                .machine
                .ticking
                .get_mut(&id)
                .and_then(|states| states.get_mut(&raw))
            {
                let elapsed_ms = now.saturating_sub(*last);
                *last = now;
                let key = StateKey { instance: id, raw };
                match self
                    .state(key)
                    .filter(|state| state.active)
                    .map(|state| state.kind)
                {
                    Some(StateKind::BooleanSwitch(_)) => {
                        self.check_switch(key, 0)?;
                    }
                    Some(StateKind::DelayedSwitch { .. }) => {
                        let node = self
                            .graph(id)
                            .state(usize::from(raw))
                            .expect("delayed switch");
                        self.recheck_delayed_switch(key, node, 0)?;
                    }
                    Some(StateKind::HealthPool { pool: Some(pool) }) => {
                        let node = self.graph(id).state(usize::from(raw)).expect("health pool");
                        if let Some(current) =
                            self.world.health_pool_current(self.site(id, node), pool)
                        {
                            self.publish_health_pool(key, node, current)?;
                        }
                    }
                    Some(StateKind::ShieldRecharge) => {
                        self.tick_shield_recharge(key, elapsed_ms)?
                    }
                    Some(StateKind::WitchStaffHealing) => {
                        self.tick_witch_staff_healing(key, elapsed_ms)?
                    }
                    Some(StateKind::SummonerBeamRay) => {
                        let node = self
                            .graph(id)
                            .state(usize::from(raw))
                            .expect("Summoner beam ray");
                        self.refresh_summoner_beam_ray(key, node)?;
                    }
                    Some(StateKind::SummonerBeamDamage) => {
                        self.tick_summoner_beam_damage(key, elapsed_ms)?
                    }
                    Some(StateKind::WitchStaffEffect) => {
                        let node = self
                            .graph(id)
                            .state(usize::from(raw))
                            .expect("Witch staff effect");
                        self.refresh_witch_staff_effect(key, node)?;
                    }
                    Some(StateKind::MovementMod) => {
                        let node = self
                            .graph(id)
                            .state(usize::from(raw))
                            .expect("movement modifier");
                        self.refresh_movement_modifier(key, node)?;
                    }
                    Some(StateKind::MovementSample(previous)) => {
                        let node = self
                            .graph(id)
                            .state(usize::from(raw))
                            .expect("movement sample");
                        self.update_movement_sample(key, node, previous)?;
                    }
                    Some(StateKind::Switch { .. }) => self.post(id, Event::Reevaluate(raw))?,
                    Some(StateKind::Shockwave { .. }) => self.tick_shockwave(key)?,
                    Some(StateKind::ModifyHealth) => {
                        let node = self
                            .graph(id)
                            .state(usize::from(raw))
                            .expect("modify health state");
                        self.world.modify_health(self.site(id, node), elapsed_ms);
                    }
                    Some(StateKind::PlacementSelector { .. }) => {
                        self.tick_placement_selector(key)?
                    }
                    Some(StateKind::ForCode) => {
                        let node = self
                            .graph(id)
                            .state(usize::from(raw))
                            .expect("for-code state");
                        self.publish_for_code(key, node)?;
                    }
                    Some(StateKind::TrackedTargets) => {
                        let node = self
                            .graph(id)
                            .state(usize::from(raw))
                            .expect("tracked target state");
                        self.refresh_tracked_targets(key, node)?;
                    }
                    Some(StateKind::DeadeyeEffects) => {
                        let node = self
                            .graph(id)
                            .state(usize::from(raw))
                            .expect("Deadeye effect state");
                        self.refresh_deadeye_effects(key, node)?;
                    }
                    Some(StateKind::ExplosionContact) => {
                        let node = self
                            .graph(id)
                            .state(usize::from(raw))
                            .expect("contact state");
                        self.refresh_contact(key, node)?;
                    }
                    Some(StateKind::AiBehavior { record: Some(_) }) => {
                        let node = self
                            .graph(id)
                            .state(usize::from(raw))
                            .expect("AI behavior state");
                        self.update_ai_behavior(key, node)?;
                    }
                    Some(StateKind::Ability { .. }) => self.activate_ability(key, 0)?,
                    Some(StateKind::ChaseVar) => self.tick_chase(key)?,
                    Some(StateKind::Watch) => self.check_watch(key)?,
                    Some(StateKind::Observe { sampled_frame })
                        if sampled_frame < self.machine.frame =>
                    {
                        self.sample_observed_value(key)?;
                    }
                    Some(StateKind::TrackEntities { sampled_frame })
                        if sampled_frame < self.machine.frame =>
                    {
                        self.sample_target_children(key)?;
                    }
                    Some(StateKind::LivingBodyArray) => self.refresh_living_body_array(key)?,
                    Some(StateKind::AiFacing(Some(record))) => {
                        let node = self
                            .graph(id)
                            .state(usize::from(raw))
                            .expect("AI facing state");
                        self.update_ai_facing(key, node, Some(record))?;
                    }
                    Some(StateKind::WaitFrames { target }) if target <= self.machine.frame => {
                        self.post(id, Event::Finish(raw))?;
                    }
                    _ => (),
                }
            }
        }
        Ok(())
    }

    pub(in crate::script_init) fn clear_tick(&mut self, key: StateKey) {
        if let Some(states) = self.machine.ticking.get_mut(&key.instance) {
            states.shift_remove(&key.raw);
            if states.is_empty() {
                self.machine.ticking.remove(&key.instance);
            }
        }
    }

    pub(in crate::script_init) fn arm_wait(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let id = key.instance;
        let Some(seconds) = self.evaluate(id, node, field(&node.fields, "m_timeout"), Some(key))
        else {
            return Ok(());
        };
        let milliseconds = ((script_expr::to_float(&seconds) * 1000.0) as f32).round();
        if !milliseconds.is_finite() {
            self.report(self.site(id, node), Issue::UnsupportedField("Wait timeout"));
            return Ok(());
        }
        if milliseconds <= 0.0 {
            self.post(id, Event::Finish(key.raw))
        } else {
            self.post_after(id, milliseconds as u64, Event::Timer(key.raw, 1))
        }
    }

    pub(in crate::script_init) fn arm_animation(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let id = key.instance;
        let duration = field(&node.fields, "m_86701C3E");
        if duration.is_null() {
            let client_removes_zombardier_animation = (self.graph(id).index, node.pos)
                == (0x0ca1, 58)
                && field(&node.fields, "m_154DBA91")["$"] == "STUConfigVarBool"
                && field(&node.fields, "m_154DBA91")["m_value"].as_u64() == Some(1)
                && is_unlinked_plug(&node.fields, "m_onEndPlug", "STUStatescriptOutputPlug");
            // Without m_playOverDuration the length is the animation asset's,
            // which the reference does not know either: it arms no timer and
            // no finish, so the state stays on until a transition or its
            // parent ends it. Accept the untimed idle shape and Zombardier's
            // client-removed firing animation when they have no timer links.
            if (!field(&node.fields, "m_154DBA91").is_null()
                && !client_removes_zombardier_animation)
                || !is_unlinked_plug(&node.fields, "m_A3B86BB0", "STUStatescriptOutputPlug")
                || !is_unlinked_plug(&node.fields, "m_onFinishedPlug", "STUStatescriptOutputPlug")
            {
                self.report(
                    self.site(id, node),
                    Issue::UnsupportedField("animation asset duration"),
                );
            }
            return Ok(());
        }
        let Some(seconds) = self.evaluate(id, node, duration, None) else {
            return Ok(());
        };
        let delay = ((script_expr::to_float(&seconds) * 1000.0) as f32).round();
        if !delay.is_finite() {
            self.report(
                self.site(id, node),
                Issue::UnsupportedField("animation duration"),
            );
            return Ok(());
        }
        let removes_self = field(&node.fields, "m_154DBA91");
        let client_ends = if removes_self.is_null() {
            false
        } else {
            let Some(value) = self.evaluate(id, node, removes_self, None) else {
                return Ok(());
            };
            truthy(&value)
        };
        let delay_ms = delay.max(0.0) as u64;
        self.post_after(id, delay_ms, Event::Timer(key.raw, 1))?;
        if !client_ends {
            self.post_after(id, delay_ms, Event::Finish(key.raw))?;
        }
        Ok(())
    }

    pub(in crate::script_init) fn arm_frame_wait(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let id = key.instance;
        let Some(value) = self.evaluate(id, node, field(&node.fields, "m_3016B9A1"), None) else {
            return Ok(());
        };
        let count = script_expr::to_int(&value, 0) as u32;
        let target = self.machine.frame.wrapping_add(count);
        self.state_mut(key).expect("active frame wait").kind = StateKind::WaitFrames { target };
        self.register_tick(key, self.machine.now_ms);
        if target <= self.machine.frame {
            self.post(id, Event::Finish(key.raw))
        } else {
            Ok(())
        }
    }

    pub(in crate::script_init) fn stamp_cinematic_time_scale(
        &mut self,
        key: StateKey,
        node: &Node,
    ) {
        let Some(delay) =
            self.evaluate(key.instance, node, field(&node.fields, "m_17D12F6B"), None)
        else {
            return;
        };
        let delay = script_expr::to_int(&delay, 0).max(0) as u32;
        let boundary_frame = self.machine.frame.wrapping_add(delay);
        self.state_mut(key)
            .expect("cinematic time scale state")
            .kind = StateKind::CinematicTimeScale { boundary_frame };
    }

    pub(in crate::script_init) fn arm_phase_transition(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let mut durations = [0; 3];
        for (duration, name) in durations
            .iter_mut()
            .zip(["m_AD8C28BA", "m_C69C2A73", "m_D7ED647E"])
        {
            let Some(value) = self
                .evaluate(key.instance, node, field(&node.fields, name), None)
                .and_then(pool_number)
                .filter(|value| *value >= 0.0 && *value < 2_147_483.0)
            else {
                self.report(
                    self.site(key.instance, node),
                    Issue::UnsupportedField("phase transition duration"),
                );
                return Ok(());
            };
            *duration = (value * 1000.0).round() as u64;
        }
        let [enter, hold, exit] = durations;
        self.post_after(key.instance, enter, Event::Timer(key.raw, 1))?;
        self.post_after(key.instance, enter + hold + exit, Event::Abort(key.raw))
    }

    pub(in crate::script_init) fn timer(
        &mut self,
        key: StateKey,
        parameter: u16,
    ) -> Result<(), InitFault> {
        let Some(state) = self.state(key).filter(|state| state.active) else {
            return Ok(());
        };
        let kind = state.kind;
        if let StateKind::Interval {
            period_frames,
            next_frame: Some(next),
        } = kind
        {
            if parameter == 1 && next <= self.machine.frame {
                self.fire_interval(key, period_frames, next)?;
            }
            return Ok(());
        }
        if kind == StateKind::Wait && parameter == 1 {
            self.post(key.instance, Event::Finish(key.raw))?;
        }
        let node = self
            .graph(key.instance)
            .state(usize::from(key.raw))
            .expect("active timer state");
        if kind == StateKind::VoiceStimulus {
            return self.voice_timer(key, node, parameter);
        }
        if kind == StateKind::Defer && parameter == 1 {
            self.follow(key.instance, node, "m_FD0C0646", Some(key), 0)?;
            if self.state(key).is_some_and(|state| state.active) {
                self.follow(key.instance, node, "m_8E484B2E", Some(key), 0)?;
            }
            return Ok(());
        }
        if matches!(kind, StateKind::LogicalButton { .. })
            && parameter == 1
            && !self.button_held(key, node)
        {
            self.set_button_down(key, node, false, 0)?;
        }
        if kind == StateKind::PhaseTransition && parameter == 1 {
            self.follow(key.instance, node, "m_82A8A67E", Some(key), 0)?;
        }
        if kind == StateKind::Anim && parameter == 1 {
            self.follow(key.instance, node, "m_A3B86BB0", Some(key), 0)?;
        }
        if let StateKind::BossIntroVoice(cue) = kind {
            if parameter == 1 {
                let site = self.site(key.instance, node);
                if let Some(duration_ms) = self.world.play_boss_intro_voice(site, cue) {
                    self.post_after(key.instance, duration_ms, Event::Finish(key.raw))?;
                } else {
                    self.report(site, Issue::UnsupportedField("Boss intro voice service"));
                }
            }
        }
        if kind == StateKind::VictoryVoice && parameter == 1 {
            let site = self.site(key.instance, node);
            if let Some(actor) =
                self.evaluate(key.instance, node, field(&node.fields, "m_8C8C5285"), None)
            {
                if let Some(duration_ms) = self.world.play_victory_voice(site, &actor) {
                    self.post_after(key.instance, duration_ms, Event::Finish(key.raw))?;
                } else {
                    self.report(site, Issue::UnsupportedField("victory voice service"));
                }
            }
        }
        if let StateKind::GateVoice(stimulus) = kind {
            if parameter == 1 {
                let attacker = field(&node.fields, "m_8C8C5285");
                let attacker = if attacker.is_null() {
                    None
                } else {
                    match self.evaluate(key.instance, node, attacker, None) {
                        Some(Value::Entity(entity)) => Some(entity),
                        _ => None,
                    }
                };
                let site = self.site(key.instance, node);
                if let Some(duration_ms) = self.world.play_gate_voice(site, stimulus, attacker) {
                    self.post_after(key.instance, duration_ms, Event::Finish(key.raw))?;
                } else {
                    self.report(site, Issue::UnsupportedField("gate voice service"));
                }
            }
        }
        if let StateKind::DelayedSwitch { current, undecided } = kind {
            let branch = match parameter {
                SWITCH_TO_TRUE => Some(true),
                SWITCH_TO_FALSE => Some(false),
                _ => None,
            };
            if let Some(branch) = branch {
                self.state_mut(key)
                    .expect("active delayed switch")
                    .delayed_timers[usize::from(branch)] = false;
                if undecided || current != branch {
                    self.set_delayed_branch(key, node, branch, 0)?;
                }
            }
        }
        if matches!(kind, StateKind::Ability { .. }) && matches!(parameter, 1 | 2) {
            self.activate_ability(key, 0)?;
        }
        if matches!(kind, StateKind::WeaponVolley { .. }) {
            match parameter {
                1 => {
                    self.follow(key.instance, node, "m_onMinShotsPlug", Some(key), 0)?;
                }
                2 => self.post(key.instance, Event::Finish(key.raw))?,
                _ => (),
            }
        }
        self.try_complete(key, node, 0)
    }

    pub(in crate::script_init) fn spawn_child(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<Option<u16>, InitFault> {
        let id = key.instance;
        let site = self.site(id, node);
        let Some(guid) = node
            .field("m_36F7B64C.m_36F7B64C.m_C974E369.m_graph")
            .and_then(asset_guid)
        else {
            self.report(site, Issue::InvalidSubScript);
            return Ok(None);
        };
        let graph = u32::from(guid as u16);
        if self.graphs.graph(graph).is_none() {
            self.report(site, Issue::MissingGraph(graph));
            return Ok(None);
        }
        let mut vars = Variables::new();
        let mut entity_vars = Variables::new();
        if (site.graph, node.pos, graph) == (0x0c95, 7, 0x0ce2) {
            let schema = node.field("m_36F7B64C.m_36F7B64C").unwrap_or(&Json::Null);
            if field(schema, "$").as_str() != Some("STU_29C4C486")
                || !field(schema, "m_CB2634D7").is_null()
                || field(schema, "m_75408640").as_u64() != Some(0)
                || !field(schema, "m_17BB822A").is_array()
            {
                self.report(site, Issue::UnsupportedField("subscript schema override"));
                return Ok(None);
            }
            for row in array_field(schema, "m_17BB822A") {
                self.spend("subscript schema override")?;
                let identifier = asset_guid(field(row, "m_0D09D2D9")).filter(|id| {
                    matches!(id, 0x0d80_0000_0000_01af | 0x0d80_0000_0000_01f5)
                        && field(row, "$").as_str() == Some("STU_6364CD4F")
                });
                let Some(identifier) = identifier else {
                    self.report(site, Issue::UnsupportedField("subscript schema override"));
                    return Ok(None);
                };
                let Some(value) = self.evaluate(id, node, field(row, "m_value"), None) else {
                    return Ok(None);
                };
                vars.insert(u32::from(identifier as u16), value);
            }
        }
        for item in array_field(&node.fields, "m_89402D74") {
            if !item.is_object() {
                continue;
            }
            let target = match script_expr::lvalue(field(item, "m_out_Var")) {
                Ok(Some(target)) => target,
                Ok(None) => continue,
                Err(error) => {
                    self.report(site, Issue::Expression(error));
                    return Ok(None);
                }
            };
            let config = field(item, "m_value");
            let value = if script_expr::native_shape(config, "STU_06610EBC", Some("m_configVars")) {
                self.assignment_group(id, node, config)?
            } else {
                self.evaluate(id, node, config, None)
            };
            let Some(value) = value else {
                return Ok(None);
            };
            match target.scope {
                Scope::Instance => vars.insert(target.id, value),
                Scope::Entity => entity_vars.insert(target.id, value),
            };
        }
        let child =
            u16::try_from(self.machine.next_id).map_err(|_| InitFault::InstanceIdExhausted)?;
        if self.create(
            child,
            graph,
            Some((id, key.raw)),
            vars,
            Startup::Ordinary,
            false,
        )? {
            // The reference sets child binding bases directly before Entry.
            self.machine.snapshot.variables.extend(entity_vars);
            return Ok(Some(child));
        }
        Ok(None)
    }

    pub(in crate::script_init) fn try_complete(
        &mut self,
        key: StateKey,
        node: &Node,
        depth: usize,
    ) -> Result<(), InitFault> {
        for (index, _) in array_field(&node.fields, "m_transitionPlug")
            .iter()
            .enumerate()
        {
            if self.follow(
                key.instance,
                node,
                &format!("m_transitionPlug[{index}]"),
                Some(key),
                depth,
            )? {
                self.post(key.instance, Event::Abort(key.raw))?;
                break;
            }
        }
        Ok(())
    }

    pub(in crate::script_init) fn changed(&mut self, key: StateKey) -> Result<(), InitFault> {
        if let Some(state) = self.state_mut(key) {
            state.pending_change = false;
            if state.active && matches!(state.kind, StateKind::Switch { .. }) {
                self.post(key.instance, Event::Reevaluate(key.raw))?;
            } else if state.active
                && !state.pending_timer
                && matches!(
                    state.kind,
                    StateKind::BooleanSwitch(_)
                        | StateKind::DelayedSwitch { .. }
                        | StateKind::AiBehavior { record: Some(_) }
                )
            {
                state.pending_timer = true;
                self.post(key.instance, Event::Reevaluate(key.raw))?;
            } else if state.active && matches!(state.kind, StateKind::Stack { .. }) {
                self.update_stack(key)?;
            } else if state.active && state.kind == StateKind::AiControlSetting {
                let node = self
                    .graph(key.instance)
                    .state(usize::from(key.raw))
                    .expect("AI control setting state");
                self.update_ai_control_setting(key, node);
            } else if state.active && state.kind == StateKind::MovementMod {
                let node = self
                    .graph(key.instance)
                    .state(usize::from(key.raw))
                    .expect("movement modifier");
                self.refresh_movement_modifier(key, node)?;
            } else if state.active && state.kind == StateKind::ExplosionContact {
                let node = self
                    .graph(key.instance)
                    .state(usize::from(key.raw))
                    .expect("contact state");
                self.refresh_contact(key, node)?;
            } else if state.active && state.kind == StateKind::PhaseScriptList {
                let node = self
                    .graph(key.instance)
                    .state(usize::from(key.raw))
                    .expect("phase script list");
                self.refresh_phase_scripts(key, node)?;
            } else if state.active && state.kind == StateKind::Watch {
                self.check_watch(key)?;
            } else if state.active && state.kind == StateKind::LivingBodyArray {
                self.refresh_living_body_array(key)?;
            } else if state.active && matches!(state.kind, StateKind::HealthPool { pool: Some(_) })
            {
                let node = self
                    .graph(key.instance)
                    .state(usize::from(key.raw))
                    .expect("active health pool");
                self.report(
                    self.site(key.instance, node),
                    Issue::UnsupportedField("changing health pool capacity"),
                );
            }
        }
        if self.state(key).is_some_and(|state| state.active) {
            let graph = self.graph(key.instance);
            if graph.index == 0x0c8c {
                if let Some(node) = graph.state(usize::from(key.raw)) {
                    if matches!(node.pos, 33 | 275) {
                        self.try_complete(key, node, 0)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub(in crate::script_init) fn schedule_interval(
        &mut self,
        key: StateKey,
        next_frame: Option<u32>,
    ) -> Result<(), InitFault> {
        if let Some(next) = next_frame {
            let delay = frame_start(next).saturating_sub(self.machine.now_ms);
            self.post_after(key.instance, delay, Event::Timer(key.raw, 1))?;
        }
        Ok(())
    }

    fn fire_interval(
        &mut self,
        key: StateKey,
        period_frames: NonZeroU32,
        scheduled_frame: u32,
    ) -> Result<(), InitFault> {
        let period = period_frames.get();
        let frame = self.machine.frame;
        // Drop missed pulses while keeping the original cadence.
        let next_frame = frame.checked_add(period - (frame - scheduled_frame) % period);
        self.state_mut(key).expect("active interval").kind = StateKind::Interval {
            period_frames,
            next_frame,
        };
        self.schedule_interval(key, next_frame)?;

        let node = self
            .graph(key.instance)
            .state(usize::from(key.raw))
            .expect("interval state");
        self.follow(key.instance, node, "on_interval", Some(key), 0)?;
        if self
            .state(key)
            .is_some_and(|state| state.active && !state.ending)
        {
            self.try_complete(key, node, 0)?;
        }
        Ok(())
    }

    fn sample_observed_value(&mut self, key: StateKey) -> Result<(), InitFault> {
        let id = key.instance;
        let frame = self.machine.frame;
        let Some(node) = self.graph(id).state(usize::from(key.raw)) else {
            return Ok(());
        };
        let Some(state) = self.state_mut(key) else {
            return Ok(());
        };
        state.kind = StateKind::Observe {
            sampled_frame: frame,
        };
        let site = self.site(id, node);
        let Some(value) = self.observation_value(site, field(&node.fields, "value")) else {
            return Ok(());
        };
        let Some(state) = self
            .state_mut(key)
            .filter(|state| state.active && !state.ending)
        else {
            return Ok(());
        };
        if state.watch_seen.as_ref() == Some(&value) {
            return Ok(());
        }
        let target = match script_expr::lvalue(field(&node.fields, "output")) {
            Ok(Some(target)) => target,
            Ok(None) => {
                self.report(
                    self.site(id, node),
                    Issue::UnsupportedField("observe output"),
                );
                return Ok(());
            }
            Err(error) => {
                self.report(self.site(id, node), Issue::Expression(error));
                return Ok(());
            }
        };
        state.watch_seen = Some(value.clone());
        self.write(source_key(id, target), value)?;
        self.follow(id, node, "on_change", Some(key), 0)?;
        if self
            .state(key)
            .is_some_and(|state| state.active && !state.ending)
        {
            self.try_complete(key, node, 0)?;
        }
        Ok(())
    }

    pub(super) fn observation_value(&mut self, site: Site, value: &Json) -> Option<Value> {
        match self.evaluate_with_context(site, value, None, Value::Null, false) {
            Ok(value) => Some(value),
            Err(
                script_expr::EvalError::UnavailableEntityPosition(_)
                | script_expr::EvalError::UnavailableEntityDirection(_)
                | script_expr::EvalError::UnavailableEntityVariable { .. }
                | script_expr::EvalError::UnavailableSpatialCandidates
                | script_expr::EvalError::UnavailableSpatialVisibility
                | script_expr::EvalError::UnavailableRoomEntityFacts,
            ) => {
                // A later sample can succeed as the scene updates.
                self.report(site, Issue::ObservationUnavailable);
                None
            }
            Err(error) => {
                self.report(site, Issue::Expression(error));
                None
            }
        }
    }

    pub(in crate::script_init) fn check_watch(&mut self, key: StateKey) -> Result<(), InitFault> {
        let Some(seen) = self
            .state(key)
            .filter(|state| state.active)
            .and_then(|state| state.watch_seen.clone())
        else {
            return Ok(());
        };
        let id = key.instance;
        let node = self
            .graph(id)
            .state(usize::from(key.raw))
            .expect("Watch state");
        if self
            .evaluate(id, node, field(&node.fields, "m_B7A3CF78"), Some(key))
            .is_some_and(|value| !equal(&value, &seen))
        {
            self.post(id, Event::WatchChanged(key.raw))?;
        }
        Ok(())
    }

    pub(in crate::script_init) fn deliver_watch(&mut self, key: StateKey) -> Result<(), InitFault> {
        let Some(seen) = self
            .state(key)
            .filter(|state| state.active)
            .and_then(|state| state.watch_seen.clone())
        else {
            return Ok(());
        };
        let id = key.instance;
        let node = self
            .graph(id)
            .state(usize::from(key.raw))
            .expect("Watch state");
        match script_expr::lvalue(field(&node.fields, "m_48A8CDEC")) {
            Ok(Some(target)) => self.write(source_key(id, target), seen)?,
            Ok(None) => (),
            Err(error) => {
                self.report(self.site(id, node), Issue::Expression(error));
                return Ok(());
            }
        }
        self.follow(id, node, "m_A70DB860", Some(key), 0)?;
        let seen = self.evaluate(id, node, field(&node.fields, "m_B7A3CF78"), Some(key));
        if let Some(state) = self.state_mut(key) {
            state.watch_seen = seen
        }
        self.try_complete(key, node, 0)
    }

    pub(in crate::script_init) fn reevaluate(
        &mut self,
        key: StateKey,
        depth: usize,
    ) -> Result<(), InitFault> {
        let Some(state) = self.state_mut(key).filter(|state| state.active) else {
            return Ok(());
        };
        state.pending_timer = false;
        let kind = state.kind;
        let node = self
            .graph(key.instance)
            .state(usize::from(key.raw))
            .expect("switch state");
        let evaluated = if matches!(kind, StateKind::DelayedSwitch { .. }) {
            self.recheck_delayed_switch(key, node, depth)?;
            true
        } else if matches!(kind, StateKind::AiBehavior { record: Some(_) }) {
            self.update_ai_behavior(key, node)?;
            true
        } else if let StateKind::Switch { case } = kind {
            let Some(next) = self.switch_case(key, node) else {
                return Ok(());
            };
            if next != case {
                self.exit_subgraph(key.instance, node, &case_path(case), depth + 1)?;
                self.state_mut(key).expect("active switch").kind = StateKind::Switch { case: next };
                self.follow(key.instance, node, &case_path(next), Some(key), depth)?;
            }
            true
        } else {
            self.check_switch(key, depth)?
        };
        if evaluated {
            self.try_complete(key, node, depth)?;
        }
        Ok(())
    }

    pub(in crate::script_init) fn check_switch(
        &mut self,
        key: StateKey,
        depth: usize,
    ) -> Result<bool, InitFault> {
        let Some(StateKind::BooleanSwitch(current)) = self
            .state(key)
            .filter(|state| state.active)
            .map(|state| state.kind)
        else {
            return Ok(false);
        };
        let id = key.instance;
        let node = self
            .graph(id)
            .state(usize::from(key.raw))
            .expect("active state node");
        if let Some(value) = self.evaluate(id, node, field(&node.fields, "m_condition"), Some(key))
        {
            let next = truthy(&value);
            if next != current {
                self.exit_subgraph(id, node, branch_path(current), depth + 1)?;
                self.state_mut(key).expect("active switch").kind = StateKind::BooleanSwitch(next);
                self.enter_branch(key, node, next, depth)?;
            }
            return Ok(true);
        }
        Ok(false)
    }

    pub(in crate::script_init) fn begin_delayed_switch(
        &mut self,
        key: StateKey,
        node: &Node,
        depth: usize,
    ) -> Result<(), InitFault> {
        let id = key.instance;
        let Some(undecided) = self.evaluate(id, node, field(&node.fields, "m_308B8A80"), None)
        else {
            return Ok(());
        };
        let undecided = truthy(&undecided);
        let current = if undecided {
            false
        } else {
            let initial = field(&node.fields, "m_418D9D9D");
            let config = if initial.is_null() {
                field(&node.fields, "m_BD83C8E6")
            } else {
                initial
            };
            let Some(initial) = self.evaluate(id, node, config, Some(key)) else {
                return Ok(());
            };
            truthy(&initial)
        };
        self.state_mut(key).expect("active delayed switch").kind =
            StateKind::DelayedSwitch { current, undecided };
        self.register_tick(key, self.machine.now_ms);
        if !undecided {
            self.enter_branch(key, node, current, depth)?;
        }
        self.recheck_delayed_switch(key, node, depth)
    }

    pub(in crate::script_init) fn recheck_delayed_switch(
        &mut self,
        key: StateKey,
        node: &Node,
        depth: usize,
    ) -> Result<(), InitFault> {
        let Some(StateKind::DelayedSwitch { current, undecided }) = self
            .state(key)
            .filter(|state| state.active)
            .map(|state| state.kind)
        else {
            return Ok(());
        };
        let id = key.instance;
        let condition = field(&node.fields, "m_BD83C8E6");
        let false_override = field(&node.fields, "m_E8FC060D");
        let requested = if current {
            let Some(holds_false) =
                self.delayed_false_condition(key, node, condition, false_override)
            else {
                return Ok(());
            };
            holds_false.then_some(false)
        } else {
            let Some(holds_true) = self.evaluate(id, node, condition, Some(key)) else {
                return Ok(());
            };
            if truthy(&holds_true) {
                Some(true)
            } else if undecided {
                let Some(holds_false) =
                    self.delayed_false_condition(key, node, condition, false_override)
                else {
                    return Ok(());
                };
                holds_false.then_some(false)
            } else {
                None
            }
        };
        let Some(branch) = requested else {
            if !undecided {
                self.cancel_delayed_timer(key, !current);
            }
            return Ok(());
        };
        let delay_field = if branch { "m_7130456B" } else { "m_ED60694A" };
        let Some(delay) = self.evaluate(id, node, field(&node.fields, delay_field), None) else {
            return Ok(());
        };
        let seconds = script_expr::to_float(&delay);
        let milliseconds = ((seconds as f32) * 1000.0).trunc();
        if !milliseconds.is_finite() {
            self.report(
                self.site(id, node),
                Issue::UnsupportedField("delayed switch timeout"),
            );
            return Ok(());
        }
        if seconds > 0.0 {
            self.arm_delayed_timer(key, branch, milliseconds as u64)?;
        }
        if undecided {
            self.cancel_delayed_timer(key, !branch);
        }
        if seconds <= 0.0 {
            self.set_delayed_branch(key, node, branch, depth)?;
        }
        Ok(())
    }

    pub(in crate::script_init) fn delayed_false_condition(
        &mut self,
        key: StateKey,
        node: &Node,
        condition: &Json,
        override_config: &Json,
    ) -> Option<bool> {
        let invert = override_config.is_null();
        let config = if invert { condition } else { override_config };
        self.evaluate(key.instance, node, config, Some(key))
            .map(|value| truthy(&value) != invert)
    }

    pub(in crate::script_init) fn arm_delayed_timer(
        &mut self,
        key: StateKey,
        branch: bool,
        delay_ms: u64,
    ) -> Result<(), InitFault> {
        let index = usize::from(branch);
        if !self
            .state(key)
            .expect("active delayed switch")
            .delayed_timers[index]
        {
            let parameter = if branch {
                SWITCH_TO_TRUE
            } else {
                SWITCH_TO_FALSE
            };
            self.post_after(key.instance, delay_ms, Event::Timer(key.raw, parameter))?;
            self.state_mut(key)
                .expect("active delayed switch")
                .delayed_timers[index] = true;
        }
        Ok(())
    }

    pub(in crate::script_init) fn cancel_delayed_timer(&mut self, key: StateKey, branch: bool) {
        let pending = &mut self
            .state_mut(key)
            .expect("active delayed switch")
            .delayed_timers[usize::from(branch)];
        if std::mem::take(pending) {
            let parameter = if branch {
                SWITCH_TO_TRUE
            } else {
                SWITCH_TO_FALSE
            };
            self.machine.queues.retain(key.instance, |_, event| {
                !matches!(event, Event::Timer(raw, param) if *raw == key.raw && *param == parameter)
            });
        }
    }

    pub(in crate::script_init) fn set_delayed_branch(
        &mut self,
        key: StateKey,
        node: &Node,
        next: bool,
        depth: usize,
    ) -> Result<(), InitFault> {
        if let StateKind::DelayedSwitch {
            current,
            undecided: false,
        } = self.state(key).expect("active delayed switch").kind
        {
            self.exit_subgraph(key.instance, node, branch_path(current), depth + 1)?;
        }
        self.state_mut(key).expect("active delayed switch").kind = StateKind::DelayedSwitch {
            current: next,
            undecided: false,
        };
        self.enter_branch(key, node, next, depth)
    }

    pub(in crate::script_init) fn switch_case(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Option<Option<usize>> {
        let value = self.evaluate(
            key.instance,
            node,
            field(&node.fields, "m_262AE982"),
            Some(key),
        )?;
        for (index, case) in array_field(&node.fields, "m_05706D1C").iter().enumerate() {
            if case.is_object() {
                let candidate = self.evaluate(key.instance, node, field(case, "m_value"), None)?;
                if equal(&value, &candidate) {
                    return Some(Some(index));
                }
            }
        }
        Some(None)
    }

    pub(in crate::script_init) fn enter_branch(
        &mut self,
        key: StateKey,
        node: &Node,
        branch: bool,
        depth: usize,
    ) -> Result<(), InitFault> {
        let output = if branch {
            "m_onTruePlug"
        } else {
            "m_onFalsePlug"
        };
        self.follow(key.instance, node, output, Some(key), depth)?;
        if self.state(key).is_some_and(|state| state.active) {
            self.follow(key.instance, node, branch_path(branch), Some(key), depth)?;
        }
        Ok(())
    }

    pub(in crate::script_init) fn register_state(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        let entry = registered_state(site.graph, node).expect("validated linked state");
        if entry.suppresses_model && !self.world.set_encounter_model_enabled(site, false) {
            self.report(site, Issue::UnsupportedField("encounter model control"));
            return Ok(());
        }
        if entry.class == RegisteredClass::CinematicAim {
            let accepted = if (site.graph, site.node) == (0x224e, Some(34)) {
                let mut degrees = [0.0; 2];
                for (out, name) in degrees.iter_mut().zip(["m_A53161C8", "m_AB37A892"]) {
                    let Some(value) = self.evaluate_at_with_initial(
                        site,
                        field(&node.fields, name),
                        None,
                        Value::Float(0.0),
                    ) else {
                        return Ok(());
                    };
                    *out = script_expr::to_float(&value);
                }
                self.world.set_captured_aim_lock(site, Some(degrees))
            } else {
                self.world.set_cinematic_aim_lock(site, true)
            };
            if !accepted {
                self.report(site, Issue::UnsupportedField("cinematic aim lock pose"));
                return Ok(());
            }
        }
        self.machine.registered.insert(key, entry);
        self.update_state_links(entry.class)
    }

    pub(in crate::script_init) fn unregister_state(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let Some(entry) = self.machine.registered.shift_remove(&key) else {
            return Ok(());
        };
        if entry.class == RegisteredClass::CinematicAim {
            let site = self.site(key.instance, node);
            if (site.graph, site.node) == (0x224e, Some(34)) {
                self.world.set_captured_aim_lock(site, None);
            } else {
                self.world.set_cinematic_aim_lock(site, false);
            }
        }
        if entry.suppresses_model
            && !self
                .machine
                .registered
                .values()
                .any(|state| state.suppresses_model)
        {
            let site = self.site(key.instance, node);
            if !self.world.set_encounter_model_enabled(site, true) {
                self.report(site, Issue::UnsupportedField("encounter model control"));
            }
        }
        self.state_mut(key).expect("registered state").kind = StateKind::Registered { next: None };
        self.update_state_links(entry.class)
    }

    pub(in crate::script_init) fn update_state_links(
        &mut self,
        class: RegisteredClass,
    ) -> Result<(), InitFault> {
        let mut next = None;
        for (&key, entry) in self.machine.registered.iter().rev() {
            if entry.class != class {
                continue;
            }
            self.machine.work_left =
                self.machine
                    .work_left
                    .checked_sub(1)
                    .ok_or(InitFault::WorkBudget {
                        limit: self.machine.max_work,
                        operation: "linked state registry",
                    })?;
            self.machine.snapshot.instances[&key.instance].states[&key.raw].kind =
                StateKind::Registered { next };
            if !entry.server_only {
                next = Some((key.instance, key.raw));
            }
        }
        Ok(())
    }

    pub(in crate::script_init) fn end(
        &mut self,
        key: StateKey,
        finished: bool,
        depth: usize,
    ) -> Result<(), InitFault> {
        if depth >= MAX_PLUG_DEPTH {
            return Err(InitFault::PlugDepth);
        }
        let Some(state) = self
            .state_mut(key)
            .filter(|state| state.active && !state.ending)
        else {
            return Ok(());
        };
        state.ending = true;
        let kind = state.kind;
        let id = key.instance;
        let node = self
            .graph(id)
            .state(usize::from(key.raw))
            .expect("active state node");
        self.machine
            .queues
            .retain(id, |_, event| event.state() != Some(key.raw));
        self.clear_tick(key);
        self.machine
            .voice_poll
            .retain(|registered| *registered != key);
        self.exit_subgraph(id, node, "m_subgraphPlug", depth + 1)?;
        match kind {
            StateKind::VoiceStimulus => {
                self.world.voice_context(self.site(id, node));
            }
            StateKind::ForCode => {
                self.unregister_for_code(key);
                self.publish_for_code(key, node)?;
            }
            StateKind::Registered { .. } => self.unregister_state(key, node)?,
            StateKind::CinematicTimeScale { .. } => self.stamp_cinematic_time_scale(key, node),
            StateKind::SameTeamContact { registered: true } => {
                self.world
                    .set_same_team_contact_suppressed(self.site(id, node), false);
            }
            StateKind::OutgoingDamageFilter { registered: true } => {
                self.world
                    .set_outgoing_damage_filter(self.site(id, node), None);
            }
            StateKind::AiBehavior {
                record: Some(record),
            } => {
                self.world
                    .ai_behavior_request(self.site(id, node), record, false);
            }
            StateKind::AiControlSetting => {
                self.world
                    .set_ai_control_setting(self.site(id, node), 5.0, false);
            }
            StateKind::AiControlOverride {
                record: Some(record),
            } => {
                self.world
                    .ai_control_override(self.site(id, node), record, false);
            }
            StateKind::AiFacing(Some(record)) => {
                self.world.ai_facing(self.site(id, node), record, false);
            }
            StateKind::AiWeaponOverride {
                record: Some(record),
            } => {
                self.world
                    .ai_weapon_override(self.site(id, node), record, false);
            }
            StateKind::DisplayName {
                binding: Some((entity, name)),
            } => {
                self.world
                    .set_display_name(self.site(id, node), entity, name, false);
            }
            StateKind::Defer => {
                if let Some(pending) = self.machine.deferred.get_mut(&id) {
                    pending.retain(|(_, raw)| *raw != key.raw);
                }
                self.exit_subgraph(id, node, "m_8E484B2E", depth + 1)?;
            }
            StateKind::MovementMod => {
                self.machine
                    .movement_modifiers
                    .retain(|entry, _| entry.instance != key.instance || entry.state != key.raw);
            }
            StateKind::MovementSample(previous) => {
                self.update_movement_sample(key, node, previous)?;
            }
            StateKind::Shockwave { .. } => {
                if let Some(state) = self.state_mut(key) {
                    state.kind = StateKind::Shockwave { managed: false };
                }
            }
            StateKind::PlacementSelector { .. } => {
                if let Some(state) = self.state_mut(key) {
                    state.kind = StateKind::PlacementSelector {
                        confirmed: false,
                        sampled: false,
                    };
                }
            }
            StateKind::MoveTo { .. } => {
                if let Some(state) = self.state_mut(key) {
                    state.kind = StateKind::MoveTo { movement: None };
                }
            }
            StateKind::TargetTag {
                effect: Some(effect),
            } => {
                self.world
                    .set_target_tag_effect(self.site(id, node), effect, false);
            }
            StateKind::HealthPool { pool: Some(pool) } => {
                if let Some(current) = self.world.remove_health_pool(self.site(id, node), pool) {
                    self.publish_health_pool(key, node, current)?;
                }
            }
            StateKind::BooleanSwitch(branch) => {
                self.exit_subgraph(id, node, branch_path(branch), depth + 1)?
            }
            StateKind::DelayedSwitch {
                current,
                undecided: false,
            } => self.exit_subgraph(id, node, branch_path(current), depth + 1)?,
            StateKind::Switch { case } => {
                self.exit_subgraph(id, node, &case_path(case), depth + 1)?
            }
            StateKind::PhaseScriptList | StateKind::PhaseScript => {
                for child in self.scoped_children(key) {
                    self.destroy(child)?;
                }
            }
            StateKind::WitchStaffEffect => {
                self.world.set_witch_staff_effect(self.site(id, node), None);
            }
            StateKind::DeadeyeEffects | StateKind::TrackEntities { .. } => {
                self.retain_target_children(key, &[])?;
            }
            StateKind::SubScript { child: Some(child) } => self.destroy(child)?,
            StateKind::MessageIdle | StateKind::BossPauseReceiver { .. } => {
                self.exit_subgraph(id, node, "m_33DA5163", depth + 1)?;
                self.exit_subgraph(id, node, "m_452862E6", depth + 1)?;
            }
            StateKind::AiCandidate { selected: true } => {
                self.exit_subgraph(id, node, "m_6F92F4BD", depth + 1)?;
            }
            StateKind::ButtonBlock(Some(button)) => {
                self.state_mut(key).expect("ending button block").kind =
                    StateKind::ButtonBlock(None);
                self.release_button_block(button, depth + 1)?;
            }
            StateKind::LogicalButton { .. } => {
                self.exit_subgraph(id, node, "m_80AF45FB", depth + 1)?;
                self.exit_subgraph(id, node, "m_1C9EF058", depth + 1)?;
            }
            StateKind::Stack { .. } => {
                self.exit_subgraph(id, node, "m_topSubgraphPlug", depth + 1)?;
                self.exit_subgraph(id, node, "m_underSubgraphPlug", depth + 1)?;
                self.unlink_single_output(key, depth + 1)?;
            }
            StateKind::CosmeticEntity => self.unlink_single_output(key, depth + 1)?,
            StateKind::ChaseVar => self.finish_chase(key, depth + 1)?,
            StateKind::WeaponVolley { .. } => self.finish_volley(key, depth + 1)?,
            StateKind::Ability { flags } => {
                self.set_ability_flags(key, flags & !ABILITY_HELD);
                self.stop_ability(key, node, depth + 1)?;
                self.finish_cooldown(key)?;
            }
            _ => (),
        }
        let state = self.state_mut(key).expect("ending state");
        if matches!(
            kind,
            StateKind::TrackedTargets | StateKind::TrackEntities { .. }
        ) {
            state.tracked_targets.clear();
        }
        state.active = false;
        state.ending = false;
        state.pending_change = false;
        state.pending_timer = false;
        state.delayed_timers = [false; 2];
        if matches!(kind, StateKind::SubScript { .. }) {
            state.kind = StateKind::SubScript { child: None };
        }
        if matches!(kind, StateKind::HealthPool { .. }) {
            state.kind = StateKind::HealthPool { pool: None };
        }
        if matches!(kind, StateKind::DisplayName { .. }) {
            state.kind = StateKind::DisplayName { binding: None };
        }
        if matches!(kind, StateKind::TargetTag { .. }) {
            state.kind = StateKind::TargetTag { effect: None };
        }
        if matches!(kind, StateKind::AiWeaponOverride { .. }) {
            state.kind = StateKind::AiWeaponOverride { record: None };
        }
        if matches!(kind, StateKind::AiControlOverride { .. }) {
            state.kind = StateKind::AiControlOverride { record: None };
        }
        if matches!(kind, StateKind::AiFacing(_)) {
            state.kind = StateKind::AiFacing(None);
        }
        if matches!(kind, StateKind::SameTeamContact { .. }) {
            state.kind = StateKind::SameTeamContact { registered: false };
        }
        if matches!(kind, StateKind::OutgoingDamageFilter { .. }) {
            state.kind = StateKind::OutgoingDamageFilter { registered: false };
        }
        if matches!(kind, StateKind::AiBehavior { .. }) {
            state.kind = StateKind::AiBehavior { record: None };
        }
        if matches!(kind, StateKind::LogicalButton { .. }) {
            state.kind = StateKind::LogicalButton { down: false };
        }
        if matches!(kind, StateKind::Stack { .. }) {
            state.kind = StateKind::Stack {
                top: false,
                under: false,
            };
        }
        self.machine.watchers.retain(|_, watchers| {
            watchers.retain(|watcher| *watcher != key);
            !watchers.is_empty()
        });
        self.machine.foreign_watchers.retain(|_, watchers| {
            watchers.retain(|watcher| *watcher != key);
            !watchers.is_empty()
        });
        let ending = if finished {
            "m_onFinishedPlug"
        } else {
            "m_onAbortPlug"
        };
        if kind != StateKind::VoiceStimulus {
            self.follow(id, node, ending, Some(key), depth)?;
        }
        self.follow(id, node, "m_onEndPlug", Some(key), depth)?;
        Ok(())
    }

    pub(in crate::script_init) fn exit_subgraph(
        &mut self,
        id: u16,
        node: &Node,
        path: &str,
        depth: usize,
    ) -> Result<(), InitFault> {
        let Some(plug) = node.field(path) else {
            return Ok(());
        };
        for raw in raw_indices(array_field(plug, SUBGRAPH_STATES)) {
            self.end(StateKey { instance: id, raw }, false, depth)?;
            self.drop_begin(id, raw);
        }
        Ok(())
    }

    pub(in crate::script_init) fn drop_begin(&mut self, id: u16, raw: u16) {
        let now = self.machine.now_ms;
        self.machine.queues.retain(id, |due, event| {
            due != now || !matches!(event, Event::Begin(state) if *state == raw)
        });
    }

    pub(in crate::script_init) fn exit_instance(&mut self, id: u16) -> Result<(), InitFault> {
        let parent = self.machine.snapshot.instances[&id].parent;
        self.destroy(id)?;
        let Some((instance, raw)) = parent else {
            return Ok(());
        };
        let key = StateKey { instance, raw };
        let Some(state) = self.state_mut(key).filter(|state| {
            state.active && !state.ending && state.kind == StateKind::SubScript { child: Some(id) }
        }) else {
            return Ok(());
        };
        state.kind = StateKind::SubScript { child: None };
        let node = self
            .graph(instance)
            .state(usize::from(raw))
            .expect("active subscript");
        if !configured(node, "m_24AB04BB") {
            self.post(instance, Event::Finish(raw))?;
        }
        Ok(())
    }

    pub(in crate::script_init) fn destroy(&mut self, id: u16) -> Result<(), InitFault> {
        let mut pending = vec![(id, false)];
        while let Some((id, children_done)) = pending.pop() {
            if !self.machine.snapshot.instances.contains_key(&id) {
                continue;
            }
            if !children_done {
                self.spend("instance teardown")?;
                let mut effects = Vec::new();
                for state in self.machine.snapshot.instances[&id].states.values_mut() {
                    state.ending = true;
                    effects.extend(std::mem::take(&mut state.target_children).into_values());
                }
                pending.push((id, true));
                pending.extend(
                    effects
                        .into_iter()
                        .filter(|child| self.machine.snapshot.instances.contains_key(child))
                        .map(|child| (child, false)),
                );
                pending.extend(self.machine.snapshot.instances.iter().rev().filter_map(
                    |(&child, instance)| {
                        instance
                            .parent
                            .is_some_and(|(parent, _)| parent == id)
                            .then_some((child, false))
                    },
                ));
                continue;
            }
            let outputs: Vec<_> = self.machine.snapshot.instances[&id]
                .states
                .iter()
                .filter_map(|(&raw, state)| {
                    (state.linked_variable.is_some()
                        || state.ability_stack.is_some()
                        || (state.active
                            && matches!(
                                state.kind,
                                StateKind::ForCode | StateKind::AiControlSetting
                            ))
                        || matches!(
                            state.kind,
                            StateKind::Registered { .. }
                                | StateKind::SameTeamContact { registered: true }
                                | StateKind::OutgoingDamageFilter { registered: true }
                                | StateKind::WeaponVolley { .. }
                                | StateKind::ButtonBlock(Some(_))
                                | StateKind::HealthPool { pool: Some(_) }
                                | StateKind::DisplayName { binding: Some(_) }
                                | StateKind::TargetTag { effect: Some(_) }
                                | StateKind::AiWeaponOverride { record: Some(_) }
                                | StateKind::AiControlOverride { record: Some(_) }
                                | StateKind::AiFacing(Some(_))
                                | StateKind::AiBehavior { record: Some(_) }
                                | StateKind::VoiceStimulus
                        ))
                    .then_some((raw, state.kind))
                })
                .collect();
            for (raw, kind) in outputs {
                let key = StateKey { instance: id, raw };
                if kind == StateKind::ForCode {
                    let node = self
                        .graph(id)
                        .state(usize::from(raw))
                        .expect("for-code state");
                    self.unregister_for_code(key);
                    self.publish_for_code(key, node)?;
                } else if matches!(kind, StateKind::Registered { .. }) {
                    let node = self
                        .graph(id)
                        .state(usize::from(raw))
                        .expect("linked state");
                    self.unregister_state(key, node)?;
                } else if kind == StateKind::VoiceStimulus {
                    let node = self.graph(id).state(usize::from(raw)).expect("voice state");
                    if self.state(key).is_some_and(|state| state.active) {
                        self.world.voice_context(self.site(id, node));
                    }
                    self.world.voice_context(self.site(id, node));
                } else if let StateKind::SameTeamContact { registered: true } = kind {
                    let node = self
                        .graph(id)
                        .state(usize::from(raw))
                        .expect("team contact state");
                    self.world
                        .set_same_team_contact_suppressed(self.site(id, node), false);
                } else if let StateKind::OutgoingDamageFilter { registered: true } = kind {
                    let node = self
                        .graph(id)
                        .state(usize::from(raw))
                        .expect("outgoing damage filter state");
                    self.world
                        .set_outgoing_damage_filter(self.site(id, node), None);
                } else if let StateKind::AiBehavior {
                    record: Some(record),
                } = kind
                {
                    let node = self
                        .graph(id)
                        .state(usize::from(raw))
                        .expect("AI behavior state");
                    self.world
                        .ai_behavior_request(self.site(id, node), record, false);
                } else if kind == StateKind::AiControlSetting {
                    let node = self
                        .graph(id)
                        .state(usize::from(raw))
                        .expect("AI control setting state");
                    self.world
                        .set_ai_control_setting(self.site(id, node), 5.0, false);
                } else if let StateKind::AiControlOverride {
                    record: Some(record),
                } = kind
                {
                    let node = self
                        .graph(id)
                        .state(usize::from(raw))
                        .expect("AI control state");
                    self.world
                        .ai_control_override(self.site(id, node), record, false);
                } else if let StateKind::AiFacing(Some(record)) = kind {
                    let node = self
                        .graph(id)
                        .state(usize::from(raw))
                        .expect("AI facing state");
                    self.world.ai_facing(self.site(id, node), record, false);
                } else if let StateKind::AiWeaponOverride {
                    record: Some(record),
                } = kind
                {
                    let node = self
                        .graph(id)
                        .state(usize::from(raw))
                        .expect("AI weapon state");
                    self.world
                        .ai_weapon_override(self.site(id, node), record, false);
                } else if let StateKind::DisplayName {
                    binding: Some((entity, name)),
                } = kind
                {
                    let node = self
                        .graph(id)
                        .state(usize::from(raw))
                        .expect("display name");
                    self.world
                        .set_display_name(self.site(id, node), entity, name, false);
                } else if let StateKind::TargetTag {
                    effect: Some(effect),
                } = kind
                {
                    let node = self.graph(id).state(usize::from(raw)).expect("target tag");
                    self.world
                        .set_target_tag_effect(self.site(id, node), effect, false);
                } else if let StateKind::ButtonBlock(Some(button)) = kind {
                    self.state_mut(key).expect("destroyed button block").kind =
                        StateKind::ButtonBlock(None);
                    self.release_button_block(button, 0)?;
                } else if let StateKind::HealthPool { pool: Some(pool) } = kind {
                    let node = self.graph(id).state(usize::from(raw)).expect("health pool");
                    let site = self.site(id, node);
                    self.world.remove_health_pool(site, pool);
                    self.publish_health_totals(site)?;
                } else if matches!(kind, StateKind::WeaponVolley { .. }) {
                    self.finish_volley(key, 0)?;
                } else if kind == StateKind::ChaseVar {
                    self.finish_chase(key, 0)?;
                } else if matches!(kind, StateKind::Ability { .. }) {
                    self.unlink_ability_stack(key, 0)?;
                    self.finish_cooldown(key)?;
                } else {
                    self.unlink_single_output(key, 0)?;
                }
            }
            self.machine.snapshot.instances.shift_remove(&id);
            self.machine
                .disabled_start_entries
                .retain(|(instance, _)| *instance != id);
            self.machine.queues.remove(id);
            self.machine.ticking.remove(&id);
            self.machine.deferred.remove(&id);
            self.machine.voice_poll.retain(|key| key.instance != id);
            self.machine
                .ability_presses
                .retain(|key, _| key.instance != id);
            self.machine.watchers.retain(|key, watchers| {
                if matches!(key, VarKey::Instance(instance, _) if *instance == id) {
                    return false;
                }
                watchers.retain(|watcher| watcher.instance != id);
                !watchers.is_empty()
            });
            self.machine.foreign_watchers.retain(|_, watchers| {
                watchers.retain(|watcher| watcher.instance != id);
                !watchers.is_empty()
            });
        }
        Ok(())
    }
}
