//! Live `RoomSimulation` bridge for the bounded Lua 5.4 Mod / Workshop host (`--features lua-mods`).
//!
//! All hook invocations are fault-isolated: Lua runtime errors or budget overflows
//! log once per mod generation and never panic or retire the room.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::{
    commands::{CROUCH, FIRE, JUMP, SECONDARY},
    game_clock::TICK_PERIOD,
    game_combat::{BotState, PlayerBodyState, RoomSimulation, bot_health_update},
    game_replication::BodyId,
    mod_script::{
        ModAction, ModDamageEvent, ModHost, ModKillEvent, ModPlayerSnapshot, ModRoomContext,
        ModRules,
    },
    mover::VelocityChange,
    room_primitives::INTERACT_BUTTON_BIT,
    script_init::{Relocation, ScriptedMotion},
    vitals::Death,
    world::{EntityUpdate, Operation},
};

const MAX_BROADCAST_LOG: usize = 64;

const MOD_BUTTON_MAP: &[(u32, &str)] = &[
    (FIRE, "primary"),
    (SECONDARY, "secondary"),
    (0x0004, "ability1"),
    (0x0008, "ability2"),
    (0x0010, "ultimate"),
    (JUMP, "jump"),
    (CROUCH, "crouch"),
    (0x0100, "reload"),
    (0x0800, "melee"),
    (INTERACT_BUTTON_BIT, "interact"),
];

#[derive(Default)]
pub(super) struct RoomModState {
    pub(super) map_guid: u64,
    pub(super) mode_guid: u64,
    pub(super) last_generation: u64,
    pub(super) active_rules: ModRules,
    pub(super) player_gravity_overrides: HashMap<u32, f64>,
    pub(super) player_speed_overrides: HashMap<u32, f64>,
    pub(super) broadcast_log: Vec<String>,
    pub(super) fault_logged_gen: u64,
}

impl RoomSimulation {
    pub(crate) fn attach_mod_host(
        &mut self,
        host: Arc<Mutex<ModHost>>,
        map_guid: u64,
        mode_guid: u64,
    ) {
        let initial_rules = host
            .lock()
            .map(|guard| guard.script().rules())
            .unwrap_or_default();
        self.mod_host = Some(host);
        self.mod_state.map_guid = map_guid;
        self.mod_state.mode_guid = mode_guid;
        self.mod_state.last_generation = 0;
        self.mod_state.active_rules = initial_rules;
        self.mod_state.player_gravity_overrides.clear();
        self.mod_state.player_speed_overrides.clear();
        self.mod_state.fault_logged_gen = 0;
        self.sync_player_mover_mod_scales();
    }

    pub(super) fn mod_friendly_fire(&self) -> bool {
        self.mod_host.is_some() && self.mod_state.active_rules.friendly_fire
    }

    pub(super) fn mod_healing_scale(&self) -> f64 {
        if self.mod_host.is_some() {
            self.mod_state.active_rules.healing_scale
        } else {
            1.0
        }
    }

    #[cfg(test)]
    pub(crate) fn mod_active_rules(&self) -> ModRules {
        self.mod_state.active_rules
    }

    #[cfg(test)]
    pub(crate) fn mod_broadcast_log(&self) -> &[String] {
        &self.mod_state.broadcast_log
    }

    fn mod_context(&self, tick: u32, now: Duration) -> ModRoomContext {
        ModRoomContext {
            tick,
            elapsed_secs: now.as_secs_f64(),
            map_guid: self.mod_state.map_guid,
            mode_guid: self.mod_state.mode_guid,
            player_count: self.players_by_wire.len(),
        }
    }

    fn snapshot_player_for_mod(&self, wire: u32) -> Option<ModPlayerSnapshot> {
        let &entity = self.players_by_wire.get(&wire)?;
        let state = self.world.get::<PlayerBodyState>(entity)?;
        let (health, max_health) = state
            .vitals()
            .map_or((0.0, 0.0), |v| (v.total(), v.maximum()));
        Some(ModPlayerSnapshot {
            entity: state.id.entity,
            name: format!("player_{:08x}", state.owner),
            team: state.team,
            hero_guid: state.hero,
            health,
            max_health,
            position: state.mover.pose().position,
            alive: state.alive(),
        })
    }

    fn sync_player_mover_mod_scales(&mut self) {
        if self.mod_host.is_none() {
            return;
        }
        let base_gravity = self.mod_state.active_rules.gravity_scale;
        let base_speed = self.mod_state.active_rules.speed_scale;
        let entities: Vec<(u32, bevy_ecs::entity::Entity)> = self
            .players_by_wire
            .iter()
            .map(|(&wire, &entity)| (wire, entity))
            .collect();
        for (wire, entity) in entities {
            let gravity = self
                .mod_state
                .player_gravity_overrides
                .get(&wire)
                .copied()
                .unwrap_or(base_gravity);
            let speed = self
                .mod_state
                .player_speed_overrides
                .get(&wire)
                .copied()
                .unwrap_or(base_speed);
            if let Some(mut state) = self.world.get_mut::<PlayerBodyState>(entity) {
                state.mover.set_mod_scales(gravity, speed);
            }
        }
    }

    fn log_mod_fault(&mut self, generation: u64, hook: &str, error: impl std::fmt::Display) {
        if self.mod_state.fault_logged_gen != generation {
            self.mod_state.fault_logged_gen = generation;
            eprintln!("[mod] hook `{hook}` fault (gen {generation}): {error}");
        }
    }

    pub(super) fn mod_on_player_spawn(&mut self, id: BodyId, tick: u32) {
        let Some(host) = self.mod_host.as_ref().map(Arc::clone) else {
            return;
        };
        self.sync_player_mover_mod_scales();
        let Some(snapshot) = self.snapshot_player_for_mod(id.entity) else {
            return;
        };
        let now = Duration::from_secs_f64(f64::from(tick) * TICK_PERIOD.as_secs_f64());
        let ctx = self.mod_context(tick, now);
        let outcome = {
            let Ok(guard) = host.lock() else {
                return;
            };
            let gen_id = guard.generation();
            (gen_id, guard.script().on_player_spawn(&ctx, &snapshot))
        };
        match outcome {
            (_, Ok(actions)) => self.apply_mod_actions(actions, tick, now),
            (gen_id, Err(error)) => self.log_mod_fault(gen_id, "on_player_spawn", error),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn mod_evaluate_damage(
        &mut self,
        attacker: u32,
        victim: u32,
        raw_damage: f64,
        headshot: bool,
        melee: bool,
        ability_slot: i32,
        tick: u32,
        now: Duration,
    ) -> f64 {
        let Some(host) = self.mod_host.as_ref().map(Arc::clone) else {
            return raw_damage;
        };
        let scaled = (raw_damage * self.mod_state.active_rules.damage_scale).max(0.0);
        let ctx = self.mod_context(tick, now);
        let event = ModDamageEvent {
            attacker,
            victim,
            damage: scaled,
            headshot,
            melee,
            ability_slot,
        };
        let outcome = {
            let Ok(guard) = host.lock() else {
                return scaled;
            };
            let gen_id = guard.generation();
            (gen_id, guard.script().on_damage(&ctx, &event))
        };
        match outcome {
            (_, Ok(result)) => {
                if !result.actions.is_empty() {
                    self.apply_mod_actions(result.actions, tick, now);
                }
                result.damage
            }
            (gen_id, Err(error)) => {
                self.log_mod_fault(gen_id, "on_damage", error);
                scaled
            }
        }
    }

    pub(super) fn mod_on_kill(
        &mut self,
        killer: u32,
        victim: u32,
        headshot: bool,
        tick: u32,
        now: Duration,
    ) {
        let Some(host) = self.mod_host.as_ref().map(Arc::clone) else {
            return;
        };
        let ctx = self.mod_context(tick, now);
        let event = ModKillEvent {
            killer,
            victim,
            headshot,
        };
        let outcome = {
            let Ok(guard) = host.lock() else {
                return;
            };
            let gen_id = guard.generation();
            (gen_id, guard.script().on_kill(&ctx, &event))
        };
        match outcome {
            (_, Ok(actions)) => self.apply_mod_actions(actions, tick, now),
            (gen_id, Err(error)) => self.log_mod_fault(gen_id, "on_kill", error),
        }
    }

    #[cfg(test)]
    pub(crate) fn mod_dispatch_chat(&mut self, sender: &str, text: &str, tick: u32, now: Duration) {
        let Some(host) = self.mod_host.as_ref().map(Arc::clone) else {
            return;
        };
        let ctx = self.mod_context(tick, now);
        let outcome = {
            let Ok(guard) = host.lock() else {
                return;
            };
            let gen_id = guard.generation();
            (gen_id, guard.script().on_chat(&ctx, sender, text))
        };
        match outcome {
            (_, Ok(actions)) => self.apply_mod_actions(actions, tick, now),
            (gen_id, Err(error)) => self.log_mod_fault(gen_id, "on_chat", error),
        }
    }

    pub(super) fn step_mod_host(
        &mut self,
        pressed_players: &[(u32, u32)],
        tick: u32,
        now: Duration,
    ) {
        let Some(host) = self.mod_host.as_ref().map(Arc::clone) else {
            return;
        };
        let ctx = self.mod_context(tick, now);
        // 1. Detect initial attach or live hot-reload generation bump.
        let reload_result = {
            let Ok(guard) = host.lock() else {
                return;
            };
            let gen_id = guard.generation();
            if gen_id != self.mod_state.last_generation {
                let rules = guard.script().rules();
                let start_actions = guard.script().on_room_start(&ctx);
                Some((gen_id, rules, start_actions))
            } else {
                None
            }
        };
        if let Some((gen_id, rules, start_actions)) = reload_result {
            self.mod_state.last_generation = gen_id;
            self.mod_state.active_rules = rules;
            self.mod_state.player_gravity_overrides.clear();
            self.mod_state.player_speed_overrides.clear();
            self.mod_state.fault_logged_gen = 0;
            self.sync_player_mover_mod_scales();
            match start_actions {
                Ok(actions) => self.apply_mod_actions(actions, tick, now),
                Err(error) => self.log_mod_fault(gen_id, "on_room_start", error),
            }
        } else {
            self.sync_player_mover_mod_scales();
        }

        // 2. Dispatch rising button edges to `on_button`.
        for &(wire, mask) in pressed_players {
            if mask == 0 {
                continue;
            }
            let Some(snapshot) = self.snapshot_player_for_mod(wire) else {
                continue;
            };
            for &(bit, name) in MOD_BUTTON_MAP {
                if mask & bit == 0 {
                    continue;
                }
                let outcome = {
                    let Ok(guard) = host.lock() else {
                        return;
                    };
                    let gen_id = guard.generation();
                    (gen_id, guard.script().on_button(&ctx, &snapshot, name))
                };
                match outcome {
                    (_, Ok(actions)) => self.apply_mod_actions(actions, tick, now),
                    (gen_id, Err(error)) => self.log_mod_fault(gen_id, "on_button", error),
                }
            }
        }

        // 3. Dispatch per-tick hook `on_tick`.
        let tick_outcome = {
            let Ok(guard) = host.lock() else {
                return;
            };
            let gen_id = guard.generation();
            (
                gen_id,
                guard.script().on_tick(&ctx, TICK_PERIOD.as_secs_f64()),
            )
        };
        match tick_outcome {
            (_, Ok(actions)) => self.apply_mod_actions(actions, tick, now),
            (gen_id, Err(error)) => self.log_mod_fault(gen_id, "on_tick", error),
        }
    }

    fn apply_mod_actions(&mut self, actions: Vec<ModAction>, tick: u32, now: Duration) {
        for action in actions {
            match action {
                ModAction::Heal { entity, amount } => {
                    if amount > 0.0 && amount.is_finite() {
                        let scaled = amount * self.mod_state.active_rules.healing_scale;
                        self.apply_mod_heal(entity, scaled);
                    }
                }
                ModAction::Damage { entity, amount } => {
                    if amount > 0.0 && amount.is_finite() {
                        self.apply_mod_direct_damage(entity, amount, tick, now);
                    }
                }
                ModAction::SetHealth { entity, health } => {
                    if health >= 0.0 && health.is_finite() {
                        let current = self.actor_current_health(entity);
                        if let Some(current) = current {
                            let delta = health - current;
                            if delta > 1e-6 {
                                self.apply_mod_heal(entity, delta);
                            } else if delta < -1e-6 {
                                self.apply_mod_direct_damage(entity, -delta, tick, now);
                            }
                        }
                    }
                }
                ModAction::ApplyImpulse { entity, velocity } => {
                    let change = VelocityChange::Impulse(velocity);
                    if !change.valid() {
                        continue;
                    }
                    if let Some(&ecs_entity) = self.players_by_wire.get(&entity) {
                        if let Some(mut state) = self.world.get_mut::<PlayerBodyState>(ecs_entity) {
                            state.mover.queue_velocity_change(tick, change);
                        }
                    } else if let Some(&ecs_entity) = self.by_wire.get(&entity) {
                        if let Some(mut state) = self.world.get_mut::<BotState>(ecs_entity) {
                            state.bot.queue_velocity_changes(vec![(tick, change)]);
                        }
                    }
                }
                ModAction::Teleport { entity, position } => {
                    let Some(reloc) = Relocation::new(position) else {
                        continue;
                    };
                    if let Some(&ecs_entity) = self.players_by_wire.get(&entity) {
                        if let Some(mut state) = self.world.get_mut::<PlayerBodyState>(ecs_entity) {
                            state
                                .mover
                                .queue_scripted_motion(tick, ScriptedMotion::Relocate(reloc));
                        }
                    } else if let Some(&ecs_entity) = self.by_wire.get(&entity) {
                        if let Some(mut state) = self.world.get_mut::<BotState>(ecs_entity) {
                            state.bot.position = position;
                        }
                    }
                }
                ModAction::SetSpeedScale { entity, scale } => {
                    if scale.is_finite() && scale > 0.0 {
                        self.mod_state.player_speed_overrides.insert(entity, scale);
                        self.sync_player_mover_mod_scales();
                    }
                }
                ModAction::SetGravityScale { entity, scale } => {
                    if scale.is_finite() && scale > 0.0 {
                        self.mod_state
                            .player_gravity_overrides
                            .insert(entity, scale);
                        self.sync_player_mover_mod_scales();
                    }
                }
                ModAction::ResetInteractiveProp { prop_id } => {
                    self.interactive_props_mut().reset_prop(prop_id);
                }
                ModAction::SetRoomRule { key, value } => {
                    if !value.is_finite() {
                        continue;
                    }
                    match key.as_str() {
                        "damage_scale" if value >= 0.0 => {
                            self.mod_state.active_rules.damage_scale = value;
                        }
                        "healing_scale" if value >= 0.0 => {
                            self.mod_state.active_rules.healing_scale = value;
                        }
                        "gravity_scale" if value > 0.0 => {
                            self.mod_state.active_rules.gravity_scale = value;
                            self.sync_player_mover_mod_scales();
                        }
                        "speed_scale" if value > 0.0 => {
                            self.mod_state.active_rules.speed_scale = value;
                            self.sync_player_mover_mod_scales();
                        }
                        "ult_charge_scale" if value >= 0.0 => {
                            self.mod_state.active_rules.ult_charge_scale = value;
                        }
                        "cooldown_scale" if value > 0.0 => {
                            self.mod_state.active_rules.cooldown_scale = value;
                        }
                        "friendly_fire" => {
                            self.mod_state.active_rules.friendly_fire = value != 0.0;
                        }
                        _ => {}
                    }
                }
                ModAction::BroadcastChat { text } => {
                    eprintln!("[mod] {text}");
                    if self.mod_state.broadcast_log.len() >= MAX_BROADCAST_LOG {
                        self.mod_state.broadcast_log.remove(0);
                    }
                    self.mod_state.broadcast_log.push(text);
                }
                ModAction::PlayVoiceCue { .. } => {}
            }
        }
    }

    fn actor_current_health(&self, wire: u32) -> Option<f64> {
        if let Some(&entity) = self.players_by_wire.get(&wire) {
            return self
                .world
                .get::<PlayerBodyState>(entity)
                .and_then(|state| state.vitals())
                .map(crate::vitals::Vitals::total);
        }
        if let Some(&entity) = self.by_wire.get(&wire) {
            return self
                .world
                .get::<BotState>(entity)
                .map(|state| state.vitals.total());
        }
        None
    }

    fn apply_mod_heal(&mut self, wire: u32, amount: f64) {
        if let Some(&entity) = self.players_by_wire.get(&wire) {
            if let Some(state) = self.world.get_mut::<PlayerBodyState>(entity) {
                let state = state.into_inner();
                if state.alive() && state.vitals().is_some() {
                    let healed = state.heal(amount);
                    if healed > 0.0 {
                        if let Some(component) = state.take_health_update() {
                            self.scripted_health.push(EntityUpdate {
                                entity: wire,
                                operation: Some(Operation::Update(vec![component])),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
            return;
        }
        if let Some(&entity) = self.by_wire.get(&wire) {
            if let Some(state) = self.world.get_mut::<BotState>(entity) {
                let state = state.into_inner();
                if !state.bot.dead {
                    let healed = state.vitals.heal(amount);
                    if healed > 0.0 {
                        if let Some(script) = &mut state.stock_script {
                            script.on_heal(state.vitals.total());
                        }
                        self.scripted_health.push(bot_health_update(state, false));
                    }
                }
            }
        }
    }

    fn apply_mod_direct_damage(&mut self, wire: u32, amount: f64, tick: u32, now: Duration) {
        let death = Death {
            time: now.as_secs_f64(),
            frame: tick,
            killer: 0,
        };
        if let Some(id) = self.player_handle_for_wire(wire) {
            if let Some((_, _, health)) = self.damage_player_body_by_wire(id, amount, death, None) {
                self.scripted_health.extend(health);
            }
            return;
        }
        if let Some(&entity) = self.by_wire.get(&wire) {
            if let Some(state) = self.world.get_mut::<BotState>(entity) {
                let state = state.into_inner();
                if let Some(result) =
                    Self::damage_bot_with_hit(state, amount, death, now, false, None)
                {
                    let update = bot_health_update(state, result.killed);
                    self.scripted_health.push(update);
                }
            }
        }
    }
}
