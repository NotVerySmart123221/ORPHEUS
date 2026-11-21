//! Minimal generic body StateScript host for non-encounter room bots.
//!
//! Used by Practice Range stock bots when the runtime pack carries authored
//! entity definitions (such as `0x0C18` -> `0x0D24` nameplates).

use std::{
    collections::{BTreeMap, HashMap},
    f64::consts::TAU,
    sync::Arc,
};

use serde_json::Value as Json;

use crate::{
    game_bots::Bot,
    mover::sample_movement_flags,
    script_codecs::CodecTable,
    script_expr::{EngineSetting, EntityRelation, is_zeroed_owner_selector, native_shape},
    script_graph::GraphStore,
    script_init::{
        self, AiControlOverride, AiWeaponOverride, Component, HUD_MAX_HEALTH, InitFault, Issue,
        MAX_INIT_WORK, MovementSample, OWNER_HEALTH, Site, TargetTagEffect, TargetTagEffects,
        World,
    },
    script_runtime::FRAME_MS,
    script_snapshot_wire::{SceneProjection, SnapshotEncoder},
    script_wire::{Value, Variables},
    vitals::{Death, PoolKind, Vitals},
    world::GAME_MODE_ENTITY,
};

#[cfg(test)]
mod tests;

const HUD_HEALTH: u32 = 1732;
const DIED_MESSAGE: u64 = 0x0240_0000_0000_003e;
const YAW_UNITS: f64 = 65_536.0;

#[derive(Clone)]
pub(crate) struct StockBotCatalog {
    pub(crate) graphs: Arc<GraphStore>,
    pub(crate) codecs: Arc<CodecTable>,
    pub(crate) mode: u64,
}

pub(super) struct StockBotWorld {
    graphs: Arc<GraphStore>,
    mode: u64,
    entity: u32,
    hero: u64,
    body: u64,
    team: u8,
    dead: bool,
    position: [f64; 3],
    forward: [f64; 3],
    movement: Option<MovementSample>,
    vitals: Vitals,
    target_tag_effects: TargetTagEffects,
    display_names: HashMap<Site, u64>,
    ai_control_overrides: BTreeMap<(u16, u16), AiControlOverride>,
}

impl StockBotWorld {
    fn filter_flags(&self) -> u64 {
        1 | (0x0080_0000 << self.team) | if self.dead { 0x10 } else { 0 }
    }
}

impl World for StockBotWorld {
    fn entity_variable(&mut self, entity: u32, id: u32) -> Option<Value> {
        if entity == GAME_MODE_ENTITY {
            return self
                .graphs
                .mode_variables(self.mode)
                .map(|variables| variables.get(&id).copied().map_or(Value::Null, Value::from));
        }
        if entity != self.entity {
            return None;
        }
        match id {
            OWNER_HEALTH | HUD_HEALTH => Some(Value::Float(self.vitals.total())),
            HUD_MAX_HEALTH => Some(Value::Float(self.vitals.maximum())),
            _ => None,
        }
    }

    fn known_position(&self, entity: u32) -> Option<[f64; 3]> {
        (entity == self.entity).then_some(self.position)
    }

    fn position(&mut self, entity: u32) -> Option<[f64; 3]> {
        (entity == self.entity).then_some(self.position)
    }

    fn forward(&self, entity: u32) -> Option<[f64; 3]> {
        (entity == self.entity).then_some(self.forward)
    }

    fn authored_position(&mut self, entity: u32) -> Option<[f64; 3]> {
        self.position(entity)
    }

    fn authored_forward(&self, entity: u32) -> Option<[f64; 3]> {
        self.forward(entity)
    }

    fn movement_sample(&self, site: Site) -> Option<MovementSample> {
        (site.entity == self.entity)
            .then_some(self.movement)
            .flatten()
    }

    fn entity_exists(&self, entity: u32) -> Option<bool> {
        if entity == GAME_MODE_ENTITY {
            return Some(self.graphs.mode_variables(self.mode).is_some());
        }
        (entity == self.entity).then_some(true)
    }

    fn entity_hero(&self, _site: Site, entity: u32) -> Option<u64> {
        (entity == self.entity).then_some(self.hero)
    }

    fn entity_definition(&self, _site: Site, entity: u32) -> Option<u64> {
        (entity == self.entity).then_some(self.body)
    }

    fn entity_team(&self, _site: Site, entity: u32) -> Option<i8> {
        i8::try_from(self.team)
            .ok()
            .filter(|_| entity == self.entity)
    }

    fn entity_filter_flags(&self, _site: Site, entity: u32) -> Option<u64> {
        (entity == self.entity).then(|| self.filter_flags())
    }

    fn has_target_tag(&self, _site: Site, entity: u32, tag: u64) -> Option<bool> {
        if entity != self.entity {
            return None;
        }
        if let Some(effect) = self.target_tag_effects.presence(tag) {
            return Some(effect);
        }
        let body = self.graphs.entity_body(self.body)?;
        Some(body.target_tags.as_ref()?.contains(&tag))
    }

    fn set_target_tag_effect(&mut self, site: Site, effect: TargetTagEffect, active: bool) -> bool {
        site.entity == self.entity && self.target_tag_effects.apply(effect, active)
    }

    fn set_display_name(&mut self, site: Site, entity: u32, name: u64, active: bool) -> bool {
        if site.entity != self.entity || entity != self.entity {
            return false;
        }
        if active {
            self.display_names.insert(site, name);
        } else {
            self.display_names.remove(&site);
        }
        true
    }

    fn engine_setting(&self, _site: Site, key: u32) -> EngineSetting {
        crate::body_ruleset::engine_setting(&self.graphs, self.mode, key)
    }

    fn local_command_present(&self, site: Site) -> Option<bool> {
        (site.entity == self.entity).then_some(false)
    }

    fn local_combat_context_available(&self, site: Site) -> Option<bool> {
        (site.entity == self.entity).then_some(false)
    }

    fn has_dynamic_map_rigid_bodies(&self, site: Site) -> Option<bool> {
        (site.entity == self.entity).then_some(false)
    }

    fn for_code_transport(&self, _site: Site) -> Option<bool> {
        Some(false)
    }

    fn voice_context(&mut self, site: Site) -> Option<bool> {
        (site.entity == self.entity).then_some(false)
    }

    fn player_stats_available(&self, site: Site) -> Option<bool> {
        (site.entity == self.entity).then_some(false)
    }

    fn player_stats_service_available(&self, site: Site) -> Option<bool> {
        (site.entity == self.entity).then_some(false)
    }

    fn spatial_binding_owner_available(&self, site: Site) -> Option<bool> {
        (site.entity == self.entity).then_some(false)
    }

    fn outline_context_available(&self, site: Site) -> Option<bool> {
        (site.entity == self.entity).then_some(false)
    }

    fn body_active(&self, site: Site) -> Option<bool> {
        (site.entity == self.entity).then_some(true)
    }

    fn body_health(&self, site: Site, kind: Option<PoolKind>, maximum: bool) -> Option<f64> {
        (site.entity == self.entity).then(|| match kind {
            None if maximum => self.vitals.maximum(),
            None => self.vitals.total(),
            Some(_) => self.vitals.amount(kind, maximum),
        })
    }

    fn add_health_pool(
        &mut self,
        site: Site,
        kind: PoolKind,
        maximum: f64,
        current: f64,
    ) -> Option<u32> {
        (site.entity == self.entity)
            .then(|| self.vitals.add_pool(kind, maximum, current))
            .flatten()
    }

    fn remove_health_pool(&mut self, site: Site, pool: u32) -> Option<f64> {
        (site.entity == self.entity)
            .then(|| self.vitals.remove_pool(pool))
            .flatten()
    }

    fn health_pool_current(&self, site: Site, pool: u32) -> Option<f64> {
        (site.entity == self.entity)
            .then(|| self.vitals.pool_current(pool))
            .flatten()
    }

    fn add_temporary_shield(
        &mut self,
        site: Site,
        identifier: u64,
        amount: f64,
        stacked: bool,
    ) -> Option<u32> {
        (site.entity == self.entity)
            .then(|| {
                self.vitals
                    .add_temporary_shield(identifier, amount, stacked)
            })
            .flatten()
    }

    fn temporary_shield_current(&self, site: Site, identifier: u64) -> Option<f64> {
        (site.entity == self.entity).then(|| self.vitals.temporary_shield_current(identifier))
    }

    fn drain_temporary_shield(&mut self, site: Site, identifier: u64, amount: f64) -> Option<f64> {
        (site.entity == self.entity).then(|| self.vitals.drain_temporary_shield(identifier, amount))
    }

    fn related_entity(&mut self, site: Site, relation: EntityRelation, entity: u32) -> Option<u32> {
        if site.entity != self.entity {
            return None;
        }
        match relation {
            EntityRelation::OwningController
            | EntityRelation::CurrentPossessor
            | EntityRelation::AiControlEntity
                if entity == self.entity =>
            {
                Some(0)
            }
            EntityRelation::PossessedBody | EntityRelation::ControllerBodyField if entity == 0 => {
                Some(0)
            }
            _ => None,
        }
    }

    fn ai_control_override(
        &mut self,
        site: Site,
        record: AiControlOverride,
        active: bool,
    ) -> Option<bool> {
        if site.entity != self.entity || record.target != self.entity {
            return None;
        }
        let key = (site.instance, site.node?);
        self.ai_control_overrides.remove(&key);
        if active {
            self.ai_control_overrides.insert(key, record);
        }
        Some(true)
    }

    fn ai_weapon_override(
        &mut self,
        site: Site,
        record: AiWeaponOverride,
        _active: bool,
    ) -> Option<bool> {
        (site.entity == self.entity && record.target == self.entity && site.node.is_some())
            .then_some(false)
    }

    fn native(&mut self, site: Site, config: &Json) -> Option<Value> {
        if site.entity != self.entity {
            return None;
        }
        let class = config.get("$").and_then(Json::as_str)?;
        match class {
            "STU_37F20D24"
                if native_shape(config, class, Some("m_resourceKey"))
                    && config.get("m_resourceKey").is_none_or(Json::is_null) =>
            {
                Some(Value::Asset(self.mode))
            }
            "STU_A3DEF368"
                if native_shape(config, class, None)
                    && self.entity_exists(GAME_MODE_ENTITY) == Some(true) =>
            {
                Some(Value::Entity(GAME_MODE_ENTITY))
            }
            "STU_E72F6348"
                if self.entity_exists(GAME_MODE_ENTITY) == Some(false)
                    && native_shape(config, class, Some("m_entity"))
                    && config
                        .get("m_entity")
                        .is_some_and(|mode| native_shape(mode, "STU_A3DEF368", None)) =>
            {
                Some(Value::Bool(false))
            }
            "STU_4A101787" | "STU_A77AC2AC" | "STU_D3EB8D87"
                if native_shape(config, class, None) =>
            {
                Some(Value::Bool(false))
            }
            "STU_295D117D" => Some(Value::Null),
            "STU_ED45C5E8" if is_zeroed_owner_selector(config) => Some(Value::Null),
            _ => None,
        }
    }
}

pub(crate) struct StockBotScript {
    component: Component,
    world: StockBotWorld,
    graphs: Arc<GraphStore>,
    codecs: Arc<CodecTable>,
    frame: u32,
    faulted: bool,
}

impl StockBotScript {
    pub(super) fn start(catalog: &StockBotCatalog, bot: &Bot, tick: u32) -> Option<Self> {
        let entity_body = catalog.graphs.entity_body(bot.spawn.body)?;
        let roots = match script_init::body_roots(entity_body) {
            Ok(roots) => roots,
            Err(error) => {
                eprintln!("stock bot {:08X} roots unavailable: {error}", bot.entity);
                return None;
            }
        };
        let has_body_graph = roots
            .iter()
            .any(|root| !root.is_server_only() && root.graph == crate::game_pools::BODY_GRAPH);
        let initial = Variables::from([
            (OWNER_HEALTH, Value::Float(bot.spawn.health)),
            (HUD_HEALTH, Value::Float(bot.spawn.health)),
            (HUD_MAX_HEALTH, Value::Float(bot.spawn.health)),
        ]);
        let mut world = StockBotWorld {
            graphs: Arc::clone(&catalog.graphs),
            mode: catalog.mode,
            entity: bot.entity,
            hero: bot.spawn.hero,
            body: bot.spawn.body,
            team: bot.spawn.team,
            dead: bot.dead,
            position: bot.position,
            forward: forward_from_yaw(bot.yaw),
            movement: sample_bot_movement(bot, tick),
            vitals: if has_body_graph {
                Vitals::empty()
            } else {
                Vitals::new(bot.spawn.health, 0.0, 0.0)
            },
            target_tag_effects: TargetTagEffects::default(),
            display_names: HashMap::new(),
            ai_control_overrides: BTreeMap::new(),
        };
        let component = match Component::start_with_public_schema(
            Arc::clone(&catalog.graphs),
            bot.entity,
            initial,
            roots,
            tick,
            &mut world,
            MAX_INIT_WORK,
        ) {
            Ok(component) => component,
            Err(error) => {
                eprintln!("stock bot {:08X} startup stopped: {error}", bot.entity);
                return None;
            }
        };
        let mut script = Self {
            component,
            world,
            graphs: Arc::clone(&catalog.graphs),
            codecs: Arc::clone(&catalog.codecs),
            frame: tick,
            faulted: false,
        };
        script.check_projection(Ok(()));
        Some(script)
    }

    pub(super) fn on_damage(&mut self, remaining_health: f64, source: Death) {
        if self.faulted {
            return;
        }
        let current = self.world.vitals.total();
        if current > remaining_health {
            self.world.vitals.damage(current - remaining_health, source);
        }
        let health = Value::Float(remaining_health);
        let result = self
            .component
            .set_entity_variable(OWNER_HEALTH, health.clone(), &mut self.world)
            .and_then(|()| {
                self.component
                    .set_entity_variable(HUD_HEALTH, health, &mut self.world)
            });
        self.check_projection(result);
    }

    pub(super) fn on_death(&mut self, source: Death, critical: bool, loadout: Option<u64>) {
        if self.faulted || self.world.dead {
            return;
        }
        self.world.dead = true;
        self.component.set_owner_dead(true);
        let next_frame = source.frame.max(self.frame + 1);
        if let Err(error) = self.component.advance_frame(next_frame, &mut self.world) {
            self.stop_script(error);
            return;
        }
        self.frame = next_frame;
        let death_health = self
            .component
            .set_entity_variable(OWNER_HEALTH, Value::Float(0.0), &mut self.world)
            .and_then(|()| {
                self.component
                    .set_entity_variable(HUD_HEALTH, Value::Float(0.0), &mut self.world)
            });
        if let Err(error) = death_health {
            self.stop_script(error);
            return;
        }
        let parameters = Variables::from([
            (0x12, Value::Entity(source.killer)),
            (0x95, Value::Bool(critical)),
            (0x3e6, loadout.map_or(Value::Null, Value::Asset)),
            (0x3e7, Value::Array(vec![])),
        ]);
        let queued = if u64::from(source.frame) * FRAME_MS >= self.component.now_ms() {
            self.component.queue_frame_message_from(
                source.frame,
                source.killer,
                DIED_MESSAGE,
                parameters,
            )
        } else {
            self.component
                .queue_entity_message_from(source.killer, DIED_MESSAGE, parameters)
        };
        let result = queued.and_then(|_| self.component.run_now(&mut self.world));
        self.check_projection(result);
    }

    pub(super) fn advance(&mut self, tick: u32, bot: &Bot) {
        self.world.position = bot.position;
        self.world.forward = forward_from_yaw(bot.yaw);
        self.world.movement = sample_bot_movement(bot, tick);
        if self.faulted || tick <= self.frame {
            return;
        }
        self.frame = tick;
        let result = self.component.advance_frame(tick, &mut self.world);
        self.check_projection(result);
    }

    fn check_projection(&mut self, result: Result<(), InitFault>) {
        if let Err(error) = result {
            self.stop_script(error);
            return;
        }
        let diagnostic = self.component.snapshot().and_then(|snapshot| {
            snapshot.diagnostics.iter().find(|diagnostic| {
                !diagnostic.server_only && diagnostic.issue != Issue::ObservationUnavailable
            })
        });
        if let Some(diagnostic) = diagnostic {
            self.stop_script(format!(
                "{:04X}:{:?}: {:?}",
                diagnostic.site.graph, diagnostic.site.node, diagnostic.issue
            ));
        }
    }

    fn stop_script(&mut self, reason: impl std::fmt::Display) {
        eprintln!(
            "stock bot {:08X} script stopped: {reason}",
            self.world.entity
        );
        self.faulted = true;
    }

    pub(super) fn projection(&self) -> Option<SceneProjection<'_>> {
        if self.faulted {
            return None;
        }
        Some(SceneProjection {
            snapshot: self.component.snapshot()?,
            encoder: SnapshotEncoder::new(&self.graphs, &self.codecs),
        })
    }
}

fn sample_bot_movement(bot: &Bot, tick: u32) -> Option<MovementSample> {
    let pose = bot.movement(tick);
    Some(MovementSample {
        flags: sample_movement_flags(&pose)?,
        air_ticks: pose.air_ticks,
        pose_frame: pose.frame,
    })
}

fn forward_from_yaw(yaw: f64) -> [f64; 3] {
    let (sin, cos) = (yaw * TAU / YAW_UNITS).sin_cos();
    [sin, 0.0, cos]
}
