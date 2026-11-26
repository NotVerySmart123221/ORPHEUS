//! StateScript continuous state effects, health pools, AI overrides, stacks, and weapon volleys.

use std::{collections::BTreeMap, sync::Arc};

use orpheus_runtime::format::parse_guid;
use serde_json::Value as Json;

use crate::{
    script_expr::{self, Scope, Variable, truthy},
    script_graph::{Node, asset_guid},
    script_modifiers::{Modifier, ModifierKey, Operation, SPEED_SLOT, narrow},
    script_runtime::{FRAME_MS, GAME_MODE_ENTITY, MAX_MOVEMENT_AXIS, PoolKind},
    script_wire::{Value, Variables},
};

use super::super::{
    ABILITY_PUSHED, ABILITY_RUNNING, HUD_HEALTH, HUD_MAX_HEALTH, LINK_PRIORITY, MAX_PENDING_SHOTS,
    MAX_PLUG_DEPTH, OWNER_HEALTH, SCRIPT_SOURCE, SUBGRAPH_STATES,
    machine::{Event, LinkValue, StateKey, VarKey, VariableLink, Volley},
    shapes::{
        array_field, chase_lerp, chase_step, chase_value, configured, field, immediate_self_damage,
        named_temporary_shield, pool_kind, pool_number, raw_indices, same, shot_delay, source_key,
        stored,
    },
    types::{ChasePayload, InitFault, Issue, ReportedShot, ShotEvent, StateKind},
    world::{
        AiBehaviorRequest, AiControlOverride, AiFacing, AiWeaponOverride, PetSpawn, Site,
        TargetTagEffect, VelocityAxis, VelocityOp, World,
    },
};
use super::Vm;

impl<'graph, 'world, 'machine, W: World> Vm<'graph, 'world, 'machine, W> {
    pub(in crate::script_init) fn place_spray(
        &mut self,
        key: StateKey,
        node: &Node,
        depth: usize,
    ) -> Result<(), InitFault> {
        let unlock = self.evaluate(key.instance, node, field(&node.fields, "m_E441DC25"), None);
        let reach = self
            .evaluate(key.instance, node, field(&node.fields, "m_7F47658A"), None)
            .as_ref()
            .map_or(0.0, script_expr::to_float);
        let placement = match unlock {
            Some(Value::Asset(asset))
                if asset >> 48 == 0x0250
                    && asset & 0x0000_ffff_ffff_ffff != 0
                    && reach.is_finite()
                    && reach > 0.0 =>
            {
                match self.world.spray_placement(reach, self.command_frame) {
                    Some(hit) => hit.map(|hit| (asset, hit)),
                    None => {
                        self.report(self.site(key.instance, node), Issue::ObservationUnavailable);
                        None
                    }
                }
            }
            _ => None,
        };
        if let Some(state) = self.state_mut(key) {
            state.kind = StateKind::SprayPlacement(placement);
        }
        let plug = if placement.is_some() {
            "m_A39D2005"
        } else {
            "m_9254FB21"
        };
        self.follow(key.instance, node, plug, Some(key), depth)?;
        if placement.is_none() {
            self.post(key.instance, Event::Abort(key.raw))?;
        }
        Ok(())
    }

    pub(in crate::script_init) fn begin_target_tag(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        let add = field(&node.fields, "m_targetTags");
        let suppress = field(&node.fields, "m_44D1BE94");
        let (literal, present) = match (add.is_null(), suppress.is_null()) {
            (false, true) => (add, true),
            (true, false) => (suppress, false),
            _ => {
                self.report(site, Issue::UnsupportedField("target tag operands"));
                return Ok(());
            }
        };
        let Some(tag) = script_expr::target_tag_config(literal).filter(|&tag| tag != 0) else {
            self.report(site, Issue::UnsupportedField("target tag literal"));
            return Ok(());
        };
        let Some(Value::Entity(entity)) =
            self.evaluate(key.instance, node, field(&node.fields, "m_31714AFC"), None)
        else {
            self.report(site, Issue::UnsupportedField("target tag entity"));
            return Ok(());
        };
        let effect = TargetTagEffect {
            entity,
            tag,
            present,
        };
        if !self.world.set_target_tag_effect(site, effect, true) {
            self.report(site, Issue::UnsupportedField("target tag effect"));
            return Ok(());
        }
        self.state_mut(key).expect("active target tag").kind = StateKind::TargetTag {
            effect: Some(effect),
        };
        Ok(())
    }

    pub(in crate::script_init) fn ai_behavior_value(
        &mut self,
        key: StateKey,
        node: &Node,
        name: &str,
        default: f64,
    ) -> Result<Option<Value>, InitFault> {
        let mut config = field(&node.fields, name);
        for _ in 0..MAX_PLUG_DEPTH {
            self.spend("AI behavior scalar")?;
            if config.is_null() {
                return Ok(Some(Value::Float(default)));
            }
            if script_expr::native_shape(config, "STU_06610EBC", Some("m_configVars")) {
                config = config
                    .get("m_configVars")
                    .and_then(Json::as_array)
                    .and_then(|items| items.first())
                    .unwrap_or(&Json::Null);
            } else {
                return Ok(self
                    .evaluate(key.instance, node, config, Some(key))
                    .map(|value| {
                        if value == Value::Null {
                            Value::Float(default)
                        } else {
                            value
                        }
                    }));
            }
        }
        self.report(
            self.site(key.instance, node),
            Issue::UnsupportedField("AI behavior config depth"),
        );
        Ok(None)
    }

    pub(in crate::script_init) fn update_ai_behavior(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        let previous = match self.state(key).map(|state| state.kind) {
            Some(StateKind::AiBehavior { record }) => record,
            _ => return Ok(()),
        };
        let (priority, prefer_later) = if let Some(previous) = previous {
            (previous.priority, previous.prefer_later)
        } else {
            let cfg = field(&node.fields, "m_priority");
            if cfg.is_null() {
                (0.0, false)
            } else {
                if !script_expr::native_fields(cfg, &["m_priority", "m_resourceKey"])
                    || field(cfg, "$").as_str() != Some("STU_8A491388")
                    || !field(cfg, "m_resourceKey").is_null()
                    || !field(cfg, "m_priority")
                        .as_str()
                        .and_then(parse_guid)
                        .is_some_and(|guid| guid >> 48 == 0x0c40)
                {
                    self.report(
                        site,
                        Issue::UnsupportedField("AI behavior priority resource"),
                    );
                    return Ok(());
                }
                match script_expr::stack_priority(cfg) {
                    Ok(priority) => (f64::from(priority.level), priority.above_equals),
                    Err(error) => {
                        self.report(site, Issue::Expression(error));
                        return Ok(());
                    }
                }
            }
        };
        let Some(flag) = self.ai_behavior_value(key, node, "m_88881C38", 0.0)? else {
            return Ok(());
        };
        let mut shared = [0.0; 2];
        for (number, name) in shared.iter_mut().zip(["m_36998F8A", "m_44064F45"]) {
            let Some(value) = self.ai_behavior_value(key, node, name, 0.0)? else {
                return Ok(());
            };
            *number = script_expr::to_float(&value);
        }
        let (class, fields): (&'static str, &[(&str, f64)]) = match node.class.as_str() {
            "STU_B61E6681" => ("STU_B61E6681", &[("m_203D6B00", 0.0), ("m_weight", 0.0)]),
            "STU_CFA338CB" => (
                "STU_CFA338CB",
                &[
                    ("m_DC8A3A89", 0.0),
                    ("m_503D6044", 0.0),
                    ("m_157D72D1", 0.0),
                ],
            ),
            "STU_600E3C59" => (
                "STU_600E3C59",
                &[
                    ("m_D655C99A", 2.0),
                    ("m_width", 2.0),
                    ("m_weight", 0.0),
                    ("", 0.0),
                    ("m_B802CBBA", 1.0),
                ],
            ),
            _ => unreachable!("AI behavior class"),
        };
        let mut inputs = [0.0; 5];
        for (index, (name, default)) in fields.iter().enumerate() {
            if name.is_empty() {
                continue;
            }
            let Some(value) = self.ai_behavior_value(key, node, name, *default)? else {
                return Ok(());
            };
            inputs[index] = script_expr::to_float(&value);
        }
        if class == "STU_CFA338CB" {
            inputs[2] =
                f64::from((inputs[2] * f64::from((std::f64::consts::PI / 180.0) as f32)) as f32);
        }
        if class == "STU_600E3C59" {
            inputs[3] = f64::from(match field(&node.fields, "m_DD792FF0").as_u64() {
                Some(0) => 1.0f32 / 60.0,
                Some(1) => 0.05,
                Some(2) => 0.1,
                Some(4) => 0.5,
                _ => 0.2,
            });
        }
        if shared
            .iter()
            .chain(inputs.iter())
            .any(|value| !value.is_finite())
        {
            self.report(site, Issue::UnsupportedField("AI behavior finite inputs"));
            return Ok(());
        }
        let record = AiBehaviorRequest {
            class,
            priority,
            prefer_later,
            flag: truthy(&flag),
            shared,
            inputs,
        };
        match self.world.ai_behavior_request(site, record, true) {
            Some(true) => {
                self.state_mut(key).expect("active AI behavior").kind = StateKind::AiBehavior {
                    record: Some(record),
                }
            }
            Some(false) => (),
            None => self.report(site, Issue::UnsupportedField("AI behavior component")),
        }
        Ok(())
    }

    pub(in crate::script_init) fn refresh_contact(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        let Some(value) = self.evaluate(
            key.instance,
            node,
            field(&node.fields, "m_53FCEDBF"),
            Some(key),
        ) else {
            return Ok(());
        };
        let targets = match value {
            Value::Array(items) => items,
            Value::Null => Vec::new(),
            value => vec![value],
        };
        let mut entities = Vec::with_capacity(targets.len());
        let generic_player = self.world.admits_generic_player_runtime(site);
        for value in targets {
            self.spend("contact effect target")?;
            let Value::Entity(entity) = value else {
                if generic_player {
                    continue;
                }
                self.report(site, Issue::UnsupportedField("contact effect target"));
                return Ok(());
            };
            if !script_expr::valid_entity(entity) {
                if generic_player {
                    continue;
                }
                self.report(site, Issue::UnsupportedField("contact effect target"));
                return Ok(());
            }
            entities.push(entity);
        }
        // The state admission already checked the immutable resource and sync binding.
        let resource = node.field("m_contactSet.m_contactSet").and_then(asset_guid);
        if !resource.is_some_and(|resource| {
            self.world
                .emit_contact_effect(site, site.entity, resource, &entities)
        }) {
            self.report(site, Issue::UnsupportedField("contact effect service"));
        }
        Ok(())
    }

    pub(in crate::script_init) fn begin_npc_health_change(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        if self.world.modify_health(site, 0) {
            self.register_tick(key, self.machine.now_ms);
            return Ok(());
        }
        if let Some(damage) = immediate_self_damage(node, key.raw) {
            if !damage.skip_dead_owner || !self.machine.owner_dead {
                return self.apply_npc_self_damage(site, damage.amount);
            }
            return Ok(());
        }
        let Some(target) = self.creation_scalar(
            key.instance,
            node,
            field(&node.fields, "m_FA3ECC43"),
            Value::Null,
        )?
        else {
            return Ok(());
        };
        if node.pos == 48 && target == Value::Array(Vec::new()) {
            return Ok(());
        }
        let config = field(&node.fields, "m_modifyHealth");
        let Value::Entity(target) = target else {
            self.report(site, Issue::UnsupportedField("NPC damage target"));
            return Ok(());
        };
        if node.pos != 38
            || target == site.entity
            || !script_expr::valid_entity(target)
            || field(config, "m_93CDFD2F").as_u64() != Some(1)
            || !field(&node.fields, "m_95F1FB0B").is_null()
        {
            self.report(site, Issue::UnsupportedField("NPC health service"));
            return Ok(());
        }
        for name in ["m_instigator", "m_724CBA57"] {
            if self.creation_scalar(key.instance, node, field(config, name), Value::Null)?
                != Some(Value::Entity(site.entity))
            {
                self.report(site, Issue::UnsupportedField("NPC damage source"));
                return Ok(());
            }
        }
        let Some(amount) = self
            .creation_scalar(key.instance, node, field(config, "m_amount"), Value::Null)?
            .and_then(pool_number)
            .filter(|amount| (0.0..=f64::from(f32::MAX)).contains(amount))
        else {
            self.report(site, Issue::UnsupportedField("NPC damage amount"));
            return Ok(());
        };
        let amount = f64::from(amount as f32);
        if self.world.explosion_damage(site, target, amount) {
            self.post(key.instance, Event::Finish(key.raw))?;
        } else {
            self.report(
                site,
                Issue::UnsupportedField("NPC explosion damage service"),
            );
        }
        Ok(())
    }

    fn apply_npc_self_damage(&mut self, site: Site, amount: f64) -> Result<(), InitFault> {
        if !self.world.self_damage(site, amount) {
            self.report(site, Issue::UnsupportedField("NPC self damage service"));
        }
        self.publish_health_totals(site)
    }

    pub(in crate::script_init) fn update_named_shield_decay(
        &mut self,
        key: StateKey,
        node: &Node,
        elapsed_ms: u64,
    ) -> Result<bool, InitFault> {
        let site = self.site(key.instance, node);
        if self.machine.owner_dead {
            self.post(key.instance, Event::Finish(key.raw))?;
            return Ok(false);
        }
        let identifier = script_expr::named_shield_identifier(field(&node.fields, "m_7BE02F89"))
            .expect("validated named shield identifier");
        let effect = field(&node.fields, "m_modifyHealth");
        let Some(rate) = self
            .creation_scalar(key.instance, node, field(effect, "m_amount"), Value::Null)?
            .and_then(pool_number)
            .filter(|rate| *rate > 0.0 && *rate <= f64::from(f32::MAX))
        else {
            self.report(site, Issue::UnsupportedField("named shield decay rate"));
            return Ok(false);
        };
        let amount = rate * (elapsed_ms as f64 / 1_000.0);
        let Some(remaining) = self.world.drain_temporary_shield(site, identifier, amount) else {
            self.report(site, Issue::UnsupportedField("named shield decay service"));
            return Ok(false);
        };
        if elapsed_ms != 0 {
            self.publish_health_totals(site)?;
        }
        if remaining <= 0.0 {
            self.post(key.instance, Event::Finish(key.raw))?;
            return Ok(false);
        }
        Ok(true)
    }

    pub(in crate::script_init) fn begin_shield_recharge(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        if !script_expr::valid_entity(site.entity) || !self.world.recharge_shields(site, 0.0) {
            self.report(site, Issue::UnsupportedField("shield recharge service"));
            return Ok(());
        }
        self.register_tick(key, self.machine.now_ms);
        Ok(())
    }

    pub(in crate::script_init) fn tick_shield_recharge(
        &mut self,
        key: StateKey,
        elapsed_ms: u64,
    ) -> Result<(), InitFault> {
        if self.machine.owner_dead || elapsed_ms == 0 {
            return Ok(());
        }
        let node = self
            .graph(key.instance)
            .state(usize::from(key.raw))
            .expect("shield recharge");
        let site = self.site(key.instance, node);
        let config = field(&node.fields, "m_modifyHealth");
        let rate = self
            .creation_scalar(key.instance, node, field(config, "m_amount"), Value::Null)?
            .and_then(pool_number)
            .filter(|rate| (0.0..=f64::from(f32::MAX)).contains(rate));
        let Some(rate) = rate else {
            self.report(site, Issue::UnsupportedField("shield recharge rate"));
            return Ok(());
        };
        if !self
            .world
            .recharge_shields(site, rate * elapsed_ms as f64 / 1000.0)
        {
            self.report(site, Issue::UnsupportedField("shield recharge service"));
        }
        self.publish_health_totals(site)
    }

    pub(in crate::script_init) fn refresh_ground_ray(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        let fields = &node.fields;
        let segment = &fields["m_F5732403"];
        let origin = self.evaluate_at(site, &fields["m_3BC31C57"], None);
        let direction = self.evaluate_at(site, &segment["m_direction"], None);
        let length = self
            .evaluate_at(site, &segment["m_6A79BD98"], None)
            .and_then(pool_number);
        let (Some(Value::Vector(origin)), Some(Value::Vector(direction)), Some(length)) =
            (origin, direction, length)
        else {
            self.report(site, Issue::UnsupportedField("ground ray geometry"));
            return Ok(());
        };
        if !length.is_finite() || length <= 0.0 || direction != [0.0, -1.0, 0.0] {
            self.report(site, Issue::UnsupportedField("ground ray segment"));
            return Ok(());
        }
        let Some(hit) =
            self.world
                .ability_ground_ray(site, self.machine.frame, origin, direction, length)
        else {
            self.report(site, Issue::UnsupportedField("ground ray collision scene"));
            return Ok(());
        };
        let output = |field: &str| {
            let variable = script_expr::lvalue(&fields["m_target"][field])
                .ok()
                .flatten()
                .expect("ground ray output");
            source_key(key.instance, variable)
        };
        if let Some(contact) = hit.as_ref() {
            self.write(output("m_5ED803CF"), Value::Vector(contact.point))?;
        }
        self.write(output("m_7BA44A69"), Value::Bool(hit.is_some()))?;
        self.register_tick(key, self.machine.now_ms);
        Ok(())
    }

    pub(in crate::script_init) fn refresh_summoner_beam_ray(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        let segment = field(&node.fields, "m_F5732403");
        let origin = self
            .evaluate(key.instance, node, field(&node.fields, "m_3BC31C57"), None)
            .and_then(|v| match v {
                Value::Vector(pos) if pos.iter().all(|a| a.is_finite()) => Some(pos),
                _ => None,
            });
        let (direction, length) = if segment.get("$").and_then(Json::as_str) == Some("STU_E5D454BA")
        {
            let end_pos =
                match self.evaluate(key.instance, node, field(segment, "m_BD631144"), None) {
                    Some(Value::Vector(pos)) if pos.iter().all(|a| a.is_finite()) => Some(pos),
                    _ => {
                        match self.evaluate(key.instance, node, field(segment, "m_entity"), None) {
                            Some(Value::Entity(entity)) if script_expr::valid_entity(entity) => {
                                self.world.position(entity)
                            }
                            _ => None,
                        }
                    }
                };
            match (origin.or_else(|| self.world.position(site.entity)), end_pos) {
                (Some(start), Some(end)) => {
                    let delta = [end[0] - start[0], end[1] - start[1], end[2] - start[2]];
                    let dist = delta[0].hypot(delta[1]).hypot(delta[2]);
                    if dist.is_finite() && dist > 1e-6 {
                        (Some(delta.map(|a| a / dist)), Some(dist))
                    } else {
                        (None, None)
                    }
                }
                _ => (None, None),
            }
        } else {
            let direction = self
                .evaluate(key.instance, node, field(segment, "m_direction"), None)
                .and_then(|v| match v {
                    Value::Vector(dir) if dir.iter().all(|a| a.is_finite()) => Some(dir),
                    _ => None,
                });
            let length = self
                .evaluate(key.instance, node, field(segment, "m_6A79BD98"), None)
                .and_then(pool_number)
                .filter(|len| len.is_finite() && *len > 0.0);
            (direction, length)
        };
        let radius = self
            .evaluate(key.instance, node, field(&node.fields, "m_57DAE1B5"), None)
            .and_then(pool_number)
            .filter(|r| r.is_finite() && *r > 0.0)
            .unwrap_or(0.0);
        let filter = field(&node.fields, "m_filter");
        let generic = self
            .world
            .generic_beam_contact(site, origin, direction, length, radius, filter);
        let (entity, hit, point, normal, distance) = if let Some(contact) = generic {
            (
                contact.entity,
                contact.hit,
                contact.point,
                contact.normal,
                contact.distance,
            )
        } else if let Some(contact) = self.world.summoner_beam_contact(site) {
            (contact.entity, contact.hit, None, None, None)
        } else {
            self.report(site, Issue::UnsupportedField("Summoner beam ray service"));
            return Ok(());
        };
        let outputs = field(&node.fields, "m_target");
        for (name, value) in [
            ("m_692DFF72", entity.map_or(Value::Null, Value::Entity)),
            ("m_750377AA", Value::Bool(hit)),
            ("m_7BA44A69", Value::Bool(hit)),
            ("m_0CBF2A15", Value::Bool(hit)),
        ] {
            if let Some(output) = script_expr::lvalue(field(outputs, name))
                .map_err(|_| InitFault::InvalidRoot(key.instance))?
            {
                self.write(source_key(key.instance, output), value)?;
            }
        }
        if let Some(point) = point {
            if let Some(output) = script_expr::lvalue(field(outputs, "m_5ED803CF"))
                .map_err(|_| InitFault::InvalidRoot(key.instance))?
            {
                self.write(source_key(key.instance, output), Value::Vector(point))?;
            }
        }
        if let Some(normal) = normal {
            if let Some(output) = script_expr::lvalue(field(outputs, "m_F2F8606D"))
                .map_err(|_| InitFault::InvalidRoot(key.instance))?
            {
                self.write(source_key(key.instance, output), Value::Vector(normal))?;
            }
        }
        if let Some(distance) = distance {
            if let Some(output) = script_expr::lvalue(field(outputs, "m_98DC2D67"))
                .map_err(|_| InitFault::InvalidRoot(key.instance))?
            {
                self.write(source_key(key.instance, output), Value::Float(distance))?;
            }
        }
        self.register_tick(key, self.machine.now_ms);
        Ok(())
    }

    pub(in crate::script_init) fn apply_generic_modify_health(
        &mut self,
        key: StateKey,
        node: &Node,
        elapsed_ms: Option<u64>,
    ) -> Result<(), InitFault> {
        if self.machine.owner_dead || elapsed_ms == Some(0) {
            return Ok(());
        }
        let id = key.instance;
        let site = self.site(id, node);
        let effect = field(&node.fields, "m_modifyHealth");
        let mut amount = self
            .evaluate(id, node, field(effect, "m_amount"), None)
            .and_then(pool_number)
            .filter(|amount| amount.is_finite() && *amount > 0.0)
            .unwrap_or(0.0);
        if let Some(ms) = elapsed_ms {
            amount = f64::from((amount * (ms as f64 / 1000.0)) as f32);
            if !amount.is_finite() || amount <= 0.0 {
                return Ok(());
            }
        }
        let knockback = if elapsed_ms.is_none() {
            crate::script_init::Knockback::from_authored(effect, &mut |cfg| {
                self.evaluate(id, node, cfg, None).ok_or_else(|| {
                    script_expr::EvalError::UnsupportedConfig("modify health knockback".into())
                })
            })
        } else {
            None
        };
        if amount <= 0.0 && knockback.is_none() {
            return Ok(());
        }
        let target_field = field(&node.fields, "m_FA3ECC43");
        let target_value = if target_field.is_null() {
            Value::Entity(site.entity)
        } else {
            match self.evaluate(id, node, target_field, None) {
                Some(value) => value,
                None => return Ok(()),
            }
        };
        let damage = field(effect, "m_93CDFD2F").as_u64() == Some(1);
        let delta = if amount > 0.0 {
            if damage { -amount } else { amount }
        } else {
            0.0
        };
        let kb_origin = if knockback.is_some() {
            let origin_field = field(effect, "m_C0139FA6");
            if origin_field.is_null() {
                self.world.position(site.entity)
            } else {
                match self.evaluate(id, node, origin_field, None) {
                    Some(Value::Vector(pos)) if pos.iter().all(|a| a.is_finite()) => Some(pos),
                    Some(Value::Entity(entity)) if script_expr::valid_entity(entity) => {
                        self.world.position(entity)
                    }
                    _ => self.world.position(site.entity),
                }
            }
        } else {
            None
        };
        let kb_mode = if knockback.is_some() {
            self.evaluate(id, node, field(effect, "m_583C208B"), None)
                .and_then(|v| match v {
                    Value::Integer(n) => Some(n),
                    Value::Float(f) => Some(f as i64),
                    _ => None,
                })
        } else {
            None
        };
        let caster_forward = if knockback.is_some() {
            self.world.forward(site.entity)
        } else {
            None
        };
        let targets = match target_value {
            Value::Array(items) => items,
            Value::Null => Vec::new(),
            other => vec![other],
        };
        for item in targets {
            if let Value::Entity(target) = item {
                if script_expr::valid_entity(target) {
                    let impulse = knockback.as_ref().and_then(|kb| {
                        let target_pos = self.world.position(target)?;
                        let origin = kb_origin.unwrap_or(target_pos);
                        let dx = target_pos[0] - origin[0];
                        let dy = target_pos[1] - origin[1];
                        let dz = target_pos[2] - origin[2];
                        let dist = dx.hypot(dy).hypot(dz);
                        if kb_mode == Some(6) || dx.hypot(dz) < 1e-4 {
                            let fwd = caster_forward.unwrap_or([dx, 0.0, dz]);
                            kb.radial_delta([fwd[0], fwd[2]], dist, true, 1.0)
                        } else {
                            kb.delta(origin, target_pos, dist, true, 1.0)
                        }
                    });
                    if delta != 0.0 || impulse.is_some() {
                        let _ = self.world.queue_modify_health(site, target, delta, impulse);
                    }
                }
            }
        }
        Ok(())
    }

    pub(in crate::script_init) fn refresh_generic_tracked_targets(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let id = key.instance;
        let site = self.site(id, node);
        let origin_field = field(&node.fields, "m_8DCA77B6");
        let offset_field = field(&node.fields, "m_05BE62DD");
        // In retail 1.74, when `m_8DCA77B6` (origin entity) is null and `m_05BE62DD` is
        // present (e.g. Brigitte Flail, Mercy Staff, Moira Grasp, D.Va Call Mech),
        // `m_05BE62DD` is an absolute world-space position rather than a local offset.
        // Pass `origin = 0` so `World::tracked_targets` uses `offset` directly as centre.
        let origin = if origin_field.is_null() {
            if offset_field.is_null() {
                site.entity
            } else {
                0
            }
        } else {
            match self.evaluate(id, node, origin_field, None) {
                Some(Value::Entity(entity)) if script_expr::valid_entity(entity) => entity,
                _ => return self.publish_tracked_targets(key, node, Vec::new()),
            }
        };
        let offset = if offset_field.is_null() {
            [0.0; 3]
        } else {
            match self.evaluate(id, node, offset_field, None) {
                Some(Value::Vector(v)) if v.iter().all(|a| a.is_finite()) => v,
                _ => [0.0; 3],
            }
        };
        let radius = self
            .evaluate(id, node, field(&node.fields, "m_radius"), None)
            .and_then(pool_number)
            .filter(|r| r.is_finite() && *r > 0.0)
            .unwrap_or(0.0);
        if radius <= 0.0 {
            return self.publish_tracked_targets(key, node, Vec::new());
        }
        let filter = field(&node.fields, "m_filter");
        let angle_field = field(&node.fields, "m_A30B383E");
        let candidates = if !angle_field.is_null() {
            let angle = self
                .evaluate(id, node, angle_field, None)
                .and_then(pool_number)
                .filter(|a| (0.0..=360.0).contains(a))
                .unwrap_or(360.0);
            let visible = field(&node.fields, "m_32AFE9AA").as_u64() == Some(1);
            let Some(candidates) = self
                .world
                .cone_targets(site, origin, offset, radius, angle, visible, filter)
            else {
                return self.publish_tracked_targets(key, node, Vec::new());
            };
            candidates
        } else {
            let Some(candidates) = self
                .world
                .tracked_targets(site, origin, offset, radius, filter)
            else {
                return self.publish_tracked_targets(key, node, Vec::new());
            };
            candidates
        };
        let mut targets = Vec::with_capacity(candidates.len());
        let condition = field(filter, "m_condition");
        for candidate in candidates {
            self.spend("generic tracked target condition")?;
            if !script_expr::valid_entity(candidate) {
                continue;
            }
            let accepted = if condition.is_null() {
                true
            } else {
                let previous = self
                    .parameters
                    .replace(Arc::new(Variables::from([(8, Value::Entity(candidate))])));
                let value = self.evaluate(id, node, condition, None);
                self.parameters = previous;
                value.is_some_and(|v| truthy(&v))
            };
            if accepted {
                targets.push(candidate);
            }
        }
        self.publish_tracked_targets(key, node, targets)
    }

    pub(in crate::script_init) fn tick_summoner_beam_damage(
        &mut self,
        key: StateKey,
        elapsed_ms: u64,
    ) -> Result<(), InitFault> {
        if self.machine.owner_dead || elapsed_ms == 0 {
            return Ok(());
        }
        let node = self
            .graph(key.instance)
            .state(usize::from(key.raw))
            .expect("Summoner beam damage");
        let site = self.site(key.instance, node);
        let target = self.creation_scalar(
            key.instance,
            node,
            field(&node.fields, "m_FA3ECC43"),
            Value::Null,
        )?;
        let Some(Value::Entity(target)) = target.filter(
            |value| matches!(value, Value::Entity(entity) if script_expr::valid_entity(*entity)),
        ) else {
            return Ok(());
        };
        let rate = self
            .creation_scalar(
                key.instance,
                node,
                &node.fields["m_modifyHealth"]["m_amount"],
                Value::Null,
            )?
            .and_then(pool_number)
            .filter(|amount| amount.is_finite() && *amount >= 0.0);
        if !rate.is_some_and(|rate| {
            self.world.summoner_beam_damage(
                site,
                target,
                f64::from((rate * elapsed_ms as f64 / 1000.0) as f32),
            )
        }) {
            self.report(
                site,
                Issue::UnsupportedField("Summoner beam damage service"),
            );
        }
        Ok(())
    }

    pub(in crate::script_init) fn refresh_witch_staff_effect(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        let params = array_field(&node.fields, "m_params");
        if asset_guid(&node.fields["m_A76A8CF6"]["m_A76A8CF6"]) != Some(0x04a0_0000_0000_0019)
            || params.len() != 1
            || asset_guid(field(&params[0], "m_B5051BCE")) != Some(0x0a40_0000_0000_002a)
        {
            self.report(
                site,
                Issue::UnsupportedField("Witch staff effect resource or parameters"),
            );
            return Ok(());
        }
        let target = self.creation_scalar(
            key.instance,
            node,
            field(&node.fields, "m_targets"),
            Value::Null,
        )?;
        let effect =
            if let Some(Value::Entity(target)) = target.filter(|_| !self.machine.owner_dead) {
                let amount = self
                    .creation_scalar(
                        key.instance,
                        node,
                        field(&params[0], "m_value"),
                        Value::Null,
                    )?
                    .and_then(pool_number)
                    .filter(|amount| amount.is_finite() && *amount >= 0.0);
                amount.map(|amount| (target, amount))
            } else {
                None
            };
        if !self.world.set_witch_staff_effect(site, effect) {
            self.report(site, Issue::UnsupportedField("Witch staff effect service"));
        }
        Ok(())
    }

    pub(in crate::script_init) fn tick_witch_staff_healing(
        &mut self,
        key: StateKey,
        elapsed_ms: u64,
    ) -> Result<(), InitFault> {
        if self.machine.owner_dead || elapsed_ms == 0 {
            return Ok(());
        }
        let node = self
            .graph(key.instance)
            .state(usize::from(key.raw))
            .expect("Witch staff healing");
        let site = self.site(key.instance, node);
        let Value::Entity(source) = self
            .machine
            .value(VarKey::Instance(key.instance, SCRIPT_SOURCE), None)
        else {
            self.report(site, Issue::UnsupportedField("Witch staff healing source"));
            return Ok(());
        };
        let amount = self
            .creation_scalar(
                key.instance,
                node,
                &node.fields["m_modifyHealth"]["m_amount"],
                Value::Null,
            )?
            .and_then(pool_number)
            .filter(|amount| amount.is_finite() && *amount >= 0.0);
        if !amount.is_some_and(|amount| {
            self.world.heal_from_witch_staff(
                site,
                source,
                f64::from((amount * elapsed_ms as f64 / 1000.0) as f32),
            )
        }) {
            self.report(site, Issue::UnsupportedField("Witch staff healing service"));
        }
        self.publish_health_totals(site)
    }

    pub(in crate::script_init) fn refresh_tracked_targets(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        if matches!((site.graph, site.node), (0x006b, Some(6 | 7 | 121))) {
            let radius = self
                .creation_scalar(
                    key.instance,
                    node,
                    field(&node.fields, "m_radius"),
                    Value::Null,
                )?
                .and_then(pool_number)
                .filter(|radius| *radius > 0.0 && radius.is_finite());
            if let Some(targets) =
                radius.and_then(|radius| self.world.witch_staff_targets(site, radius))
            {
                return self.publish_tracked_targets(key, node, targets);
            }
        }
        let Some(Value::Entity(origin)) = self.creation_scalar(
            key.instance,
            node,
            field(&node.fields, "m_8DCA77B6"),
            Value::Null,
        )?
        else {
            self.report(site, Issue::UnsupportedField("tracked target origin"));
            return Ok(());
        };
        let Some(Value::Vector(offset)) = self.creation_scalar(
            key.instance,
            node,
            field(&node.fields, "m_05BE62DD"),
            Value::Vector([0.0; 3]),
        )?
        else {
            self.report(site, Issue::UnsupportedField("tracked target offset"));
            return Ok(());
        };
        let Some(radius) = self
            .creation_scalar(
                key.instance,
                node,
                field(&node.fields, "m_radius"),
                Value::Null,
            )?
            .and_then(pool_number)
            .filter(|radius| *radius >= 0.0 && radius.is_finite())
        else {
            self.report(site, Issue::UnsupportedField("tracked target radius"));
            return Ok(());
        };
        let filter = field(&node.fields, "m_filter");
        let angle_field = field(&node.fields, "m_A30B383E");
        if !angle_field.is_null() {
            let Some(angle) = self
                .creation_scalar(key.instance, node, angle_field, Value::Null)?
                .and_then(pool_number)
                .filter(|angle| (0.0..=360.0).contains(angle))
            else {
                self.report(site, Issue::UnsupportedField("tracked target angle"));
                return Ok(());
            };
            let visible = field(&node.fields, "m_32AFE9AA").as_u64() == Some(1);
            let Some(targets) = self
                .world
                .cone_targets(site, origin, offset, radius, angle, visible, filter)
            else {
                self.report(site, Issue::UnsupportedField("tracked target cone service"));
                return Ok(());
            };
            return self.publish_tracked_targets(key, node, targets);
        }
        let Some(candidates) = self
            .world
            .tracked_targets(site, origin, offset, radius, filter)
        else {
            self.report(site, Issue::UnsupportedField("tracked target service"));
            return Ok(());
        };
        let mut targets = Vec::new();
        for candidate in candidates {
            self.spend("tracked target condition")?;
            if !script_expr::valid_entity(candidate) {
                self.report(site, Issue::UnsupportedField("tracked target identity"));
                return Ok(());
            }
            let condition = field(filter, "m_condition");
            let accepted = if condition.is_null() {
                true
            } else {
                let previous = self
                    .parameters
                    .replace(Arc::new(Variables::from([(8, Value::Entity(candidate))])));
                let value = self.evaluate(key.instance, node, condition, None);
                self.parameters = previous;
                let Some(value) = value else {
                    return Ok(());
                };
                truthy(&value)
            };
            if accepted {
                targets.push(candidate);
            }
        }
        self.publish_tracked_targets(key, node, targets)
    }

    pub(in crate::script_init) fn publish_tracked_targets(
        &mut self,
        key: StateKey,
        node: &Node,
        targets: Vec<u32>,
    ) -> Result<(), InitFault> {
        if let Some(output) = script_expr::lvalue(field(&node.fields, "m_8A2905A0"))
            .map_err(|_| InitFault::InvalidRoot(key.instance))?
        {
            self.write(
                source_key(key.instance, output),
                Value::Array(
                    targets
                        .iter()
                        .map(|target| Value::Entity(*target))
                        .collect(),
                ),
            )?;
        }
        self.state_mut(key)
            .expect("tracked target state")
            .tracked_targets = targets;
        self.register_tick(key, self.machine.now_ms);
        Ok(())
    }

    pub(in crate::script_init) fn refresh_deadeye_effects(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        const TARGET_GRAPH: u32 = 0x01ee;
        const REAPPLY: u64 = 0x0240_0000_0000_000f;
        let site = self.site(key.instance, node);
        let selected = match stored(&self.machine.snapshot, VarKey::Entity(2551)) {
            Value::Array(values) if self.graphs.graph(TARGET_GRAPH).is_some() => values
                .into_iter()
                .filter_map(|value| match value {
                    Value::Entity(entity) if self.world.entity_alive(site, entity) => Some(entity),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        self.retain_target_children(key, &selected)?;
        for entity in selected {
            let Some(child) =
                self.ensure_target_child(key, entity, TARGET_GRAPH, false, None, || {
                    Variables::from([
                        (536, Value::Entity(entity)),
                        (SCRIPT_SOURCE, Value::Entity(site.entity)),
                    ])
                })?
            else {
                continue;
            };
            self.machine.queue_entries_from(
                self.graphs,
                Some(child),
                Some(site.entity),
                REAPPLY,
                Variables::from([(92, Value::Entity(entity))]),
                self.machine.now_ms,
            )?;
        }
        self.register_tick(key, self.machine.now_ms);
        Ok(())
    }

    pub(in crate::script_init) fn begin_for_code(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        if !matches!(field(&node.fields, "m_DC897C87"), Json::Null)
            && !matches!(field(&node.fields, "m_DC897C87").as_u64(), Some(0 | 1))
        {
            self.report(
                self.site(key.instance, node),
                Issue::UnsupportedField("for-code notification channel"),
            );
            return Ok(());
        }
        if !field(&node.fields, "m_values").is_null()
            && !field(&node.fields, "m_values")
                .as_array()
                .is_some_and(|rows| rows.iter().all(Json::is_object))
        {
            self.report(
                self.site(key.instance, node),
                Issue::UnsupportedField("for-code value rows"),
            );
            return Ok(());
        }
        self.machine.code_states.push(key);
        self.register_tick(key, self.machine.now_ms);
        self.publish_for_code(key, node)
    }

    pub(in crate::script_init) fn publish_for_code(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let channel = field(&node.fields, "m_DC897C87").as_u64().unwrap_or(1) as usize;
        if channel > 1 {
            return Ok(());
        }
        for row in array_field(&node.fields, "m_values") {
            self.spend("native code value update")?;
            let config = field(row, "m_value");
            if !config.is_null() {
                self.evaluate(key.instance, node, config, None);
            }
            if let Some(identifier) = field(row, "m_identifier")
                .as_str()
                .and_then(parse_guid)
                .filter(|id| *id != 0)
            {
                self.machine.code_pending[channel].push(identifier);
            }
        }
        if channel == 0 {
            self.notify_code(0)?;
        }
        Ok(())
    }

    pub(in crate::script_init) fn notify_code(&mut self, channel: usize) -> Result<(), InitFault> {
        let pending = std::mem::take(&mut self.machine.code_pending[channel]);
        let mut selected = Vec::new();
        for identifier in pending {
            for (token, filter) in self.machine.code_observers.clone() {
                self.spend("native code observer selection")?;
                if filter.is_none_or(|filter| filter == identifier) && !selected.contains(&token) {
                    selected.push(token);
                }
            }
        }
        for token in selected {
            self.spend("native code notification")?;
            if !self.world.code_changed(self.machine.snapshot.entity, token) {
                self.report(
                    Site {
                        entity: self.machine.snapshot.entity,
                        instance: 0,
                        graph: 0,
                        node: None,
                    },
                    Issue::UnsupportedField("native code observer"),
                );
            }
        }
        Ok(())
    }

    pub(in crate::script_init) fn unregister_for_code(&mut self, key: StateKey) {
        if let Some(index) = self
            .machine
            .code_states
            .iter()
            .position(|registered| *registered == key)
        {
            self.machine.code_states.swap_remove(index);
        }
    }

    pub(in crate::script_init) fn code_value(
        &mut self,
        identifier: u64,
        mut initial: Value,
    ) -> Result<(bool, Value), InitFault> {
        for key in self.machine.code_states.clone() {
            let node = self
                .graph(key.instance)
                .state(usize::from(key.raw))
                .expect("registered code state");
            for row in array_field(&node.fields, "m_values") {
                self.spend("native code value lookup")?;
                if field(row, "m_identifier").as_str().and_then(parse_guid) == Some(identifier) {
                    let config = field(row, "m_value");
                    let failed_base = field(config, "$").as_str() == Some("STU_B40ACFBA")
                        && script_expr::native_fields(config, &["m_52EA7AB9"]);
                    if !config.is_null() && !failed_base {
                        if let Some(value) = self.evaluate(key.instance, node, config, None) {
                            initial = value;
                        }
                    }
                    return Ok((true, initial));
                }
            }
            initial = Value::Null;
        }
        Ok((false, initial))
    }

    pub(in crate::script_init) fn begin_ai_control_override(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        let Some(Value::Entity(target)) = self.creation_scalar(
            key.instance,
            node,
            field(&node.fields, "m_00D49978"),
            Value::Null,
        )?
        else {
            self.report(site, Issue::UnsupportedField("AI control target"));
            return Ok(());
        };
        let priority = field(&node.fields, "m_priority");
        let (priority, prefer_later) = if priority.is_null() {
            (9999.0, false)
        } else {
            match script_expr::stack_priority(priority) {
                Ok(value) => (f64::from(value.level), value.above_equals),
                Err(error) => {
                    self.report(site, Issue::Expression(error));
                    return Ok(());
                }
            }
        };
        let limits = if node.class == "STU_473549CA" {
            let mut limits = [f64::from(std::f32::consts::PI); 2];
            for (index, name) in ["m_49EEBB65", "m_FD4D6C19"].into_iter().enumerate() {
                if !field(&node.fields, name).is_null() {
                    let Some(value) = self.creation_scalar(
                        key.instance,
                        node,
                        field(&node.fields, name),
                        Value::Float(limits[index]),
                    )?
                    else {
                        return Ok(());
                    };
                    limits[index] = script_expr::to_float(&value);
                }
            }
            if limits.iter().any(|value| !value.is_finite()) {
                self.report(site, Issue::UnsupportedField("AI control limits"));
                return Ok(());
            }
            Some(limits)
        } else {
            None
        };
        let record = AiControlOverride {
            target,
            limits,
            priority,
            prefer_later,
        };
        match self.world.ai_control_override(site, record, true) {
            Some(true) => {
                self.state_mut(key).expect("active AI control state").kind =
                    StateKind::AiControlOverride {
                        record: Some(record),
                    }
            }
            Some(false) => (),
            None => self.report(site, Issue::UnsupportedField("AI control component")),
        }
        Ok(())
    }

    pub(in crate::script_init) fn update_ai_control_setting(&mut self, key: StateKey, node: &Node) {
        let id = key.instance;
        let Some(scalar) = self.evaluate(id, node, field(&node.fields, "m_0468C6D4"), Some(key))
        else {
            return;
        };
        let Some(flag) = self.evaluate(id, node, field(&node.fields, "m_D6E83CD4"), Some(key))
        else {
            return;
        };
        let site = self.site(id, node);
        if !self.world.set_ai_control_setting(
            site,
            (script_expr::to_float(&scalar) as f32).max(0.1),
            truthy(&flag),
        ) {
            self.report(
                site,
                Issue::UnsupportedField("AI control setting component"),
            );
        }
    }

    pub(in crate::script_init) fn publish_facing_record(
        &mut self,
        node: &Node,
        site: Site,
        record: AiFacing,
        active: bool,
    ) -> Option<bool> {
        if node.declares_facing_claim() {
            self.world.set_facing_claim(site, record, active)
        } else {
            self.world.ai_facing(site, record, active)
        }
    }

    pub(in crate::script_init) fn update_ai_facing(
        &mut self,
        key: StateKey,
        node: &Node,
        previous: Option<AiFacing>,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        let authored = node.declares_facing_claim();
        let (target, priority) = if authored {
            (
                site.entity,
                node.facing_claim_priority()
                    .expect("validated facing claim priority"),
            )
        } else if let Some(previous) = previous {
            (previous.target, previous.priority)
        } else {
            let priority = field(&node.fields, "m_priority");
            if !field(&node.fields, "m_baseEntity").is_null()
                || asset_guid(field(priority, "m_priority")) != Some(0x0c40_0000_0000_0003)
                || !field(priority, "m_resourceKey").is_null()
            {
                self.report(site, Issue::UnsupportedField("AI facing options"));
                return Ok(());
            }
            let Some(Value::Entity(target)) =
                self.evaluate(key.instance, node, field(&node.fields, "m_00D49978"), None)
            else {
                self.report(site, Issue::UnsupportedField("AI facing target"));
                return Ok(());
            };
            (
                target,
                script_expr::stack_priority(priority).expect("validated priority asset"),
            )
        };
        let operand = if authored { "direction" } else { "m_7CCD8B4B" };
        let Some(direction) = self.evaluate(key.instance, node, field(&node.fields, operand), None)
        else {
            if authored {
                self.post(key.instance, Event::Abort(key.raw))?;
            }
            return Ok(());
        };
        let direction = match direction {
            Value::Vector(direction) if direction.iter().all(|axis| axis.is_finite()) => direction,
            _ => {
                self.report(
                    site,
                    Issue::UnsupportedField(if authored {
                        "facing claim direction"
                    } else {
                        "AI facing direction"
                    }),
                );
                if authored {
                    self.post(key.instance, Event::Abort(key.raw))?;
                }
                return Ok(());
            }
        };
        let direction = if authored {
            let scale = direction.into_iter().map(f64::abs).fold(0.0, f64::max);
            if scale == 0.0 {
                self.report(site, Issue::UnsupportedField("facing claim direction"));
                self.post(key.instance, Event::Abort(key.raw))?;
                return Ok(());
            }
            let scaled = direction.map(|axis| axis / scale);
            let magnitude = scaled.iter().map(|axis| axis * axis).sum::<f64>().sqrt();
            scaled.map(|axis| axis / magnitude)
        } else {
            direction
        };
        let record = AiFacing {
            target,
            direction,
            priority,
        };
        match self.publish_facing_record(node, site, record, true) {
            Some(true) => {
                self.state_mut(key).expect("active AI facing").kind =
                    StateKind::AiFacing(Some(record));
                self.register_tick(key, self.machine.now_ms);
            }
            Some(false) => self.post(key.instance, Event::Abort(key.raw))?,
            None => {
                self.report(
                    site,
                    Issue::UnsupportedField(if authored {
                        "facing claim component"
                    } else {
                        "AI facing component"
                    }),
                );
                if authored {
                    self.post(key.instance, Event::Abort(key.raw))?;
                }
            }
        }
        Ok(())
    }

    pub(in crate::script_init) fn begin_display_name(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        let target = field(&node.fields, "m_14EDDDA7");
        let (name, shape) = match (site.graph, node.pos) {
            (0x0cf3, 21..=25) => (
                0x7105 + u64::from(node.pos - 21),
                script_expr::native_shape(target, "STU_B5A0CAF0", None),
            ),
            (0x0cbc, 20 | 21 | 22 | 27 | 109) | (0x0d1d, 6 | 11 | 73) => {
                let name = match (site.graph, node.pos) {
                    (0x0cbc, 20) => 0x4ccc,
                    (0x0cbc, 21) => 0x4ccd,
                    (0x0cbc, 22) => 0x4cce,
                    (0x0cbc, 27) => 0x4cd3,
                    (0x0cbc, 109) => 0x6abf,
                    (0x0d1d, 6) => 0x5768,
                    (0x0d1d, 11) => 0x5769,
                    _ => 0x5e06,
                };
                (
                    name,
                    script_expr::native_shape(target, "STU_DB742151", Some("m_entity")),
                )
            }
            _ => (0, false),
        };
        let name = 0x0de0_0000_0000_0000 | name;
        if !shape || asset_guid(field(&node.fields, "m_overrideName")) != Some(name) {
            self.report(
                site,
                Issue::UnsupportedField("display name override options"),
            );
            return Ok(());
        }
        let Some(Value::Entity(entity)) = self.evaluate(key.instance, node, target, None) else {
            self.report(
                site,
                Issue::UnsupportedField("display name override target"),
            );
            return Ok(());
        };
        if !script_expr::valid_entity(entity)
            || self.world.entity_component92(site, entity) == Some(false)
        {
            return Ok(());
        }
        if !self.world.set_display_name(site, entity, name, true) {
            self.report(
                site,
                Issue::UnsupportedField("display name override target"),
            );
            return Ok(());
        }
        self.state_mut(key).expect("active display name").kind = StateKind::DisplayName {
            binding: Some((entity, name)),
        };
        Ok(())
    }

    pub(in crate::script_init) fn begin_ai_weapon_override(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        let Some(Value::Entity(target)) = self.creation_scalar(
            key.instance,
            node,
            field(&node.fields, "m_00D49978"),
            Value::Null,
        )?
        else {
            self.report(site, Issue::UnsupportedField("AI weapon target"));
            return Ok(());
        };
        let Some(slot) = self.creation_scalar(
            key.instance,
            node,
            field(&node.fields, "m_D66E7D68"),
            Value::Integer(9999),
        )?
        else {
            return Ok(());
        };
        let Some(Value::Asset(identifier)) = self.creation_scalar(
            key.instance,
            node,
            field(&node.fields, "m_F2810377"),
            Value::Asset(0),
        )?
        else {
            self.report(site, Issue::UnsupportedField("AI weapon identifier"));
            return Ok(());
        };
        let Some(enabled) = self.creation_scalar(
            key.instance,
            node,
            field(&node.fields, "m_F8585ABF"),
            Value::Null,
        )?
        else {
            return Ok(());
        };
        let priority = field(&node.fields, "m_priority");
        let (priority, prefer_later) = if priority.is_null() {
            (9999.0, false)
        } else {
            let priority = match script_expr::stack_priority(priority) {
                Ok(priority) => priority,
                Err(error) => {
                    self.report(site, Issue::Expression(error));
                    return Ok(());
                }
            };
            (f64::from(priority.level), priority.above_equals)
        };
        let record = AiWeaponOverride {
            target,
            slot: script_expr::to_int(&slot, 9999),
            identifier,
            enabled: truthy(&enabled),
            priority,
            prefer_later,
        };
        match self.world.ai_weapon_override(site, record, true) {
            Some(true) => {
                self.state_mut(key).expect("active AI weapon state").kind =
                    StateKind::AiWeaponOverride {
                        record: Some(record),
                    }
            }
            Some(false) => (),
            None => self.report(site, Issue::UnsupportedField("AI weapon control component")),
        }
        Ok(())
    }

    pub(in crate::script_init) fn check_health_capacity(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        let target = self.evaluate(key.instance, node, field(&node.fields, "m_FA3ECC43"), None);
        let amount = self
            .evaluate(key.instance, node, field(&node.fields, "m_amount"), None)
            .and_then(pool_number);
        if target != Some(Value::Entity(site.entity))
            || field(&node.fields, "m_7E8B666C").as_u64() != Some(0)
            || !amount.is_some_and(|amount| {
                amount > 0.0
                    && self.world.body_health(site, Some(PoolKind::Health), true) == Some(amount)
            })
        {
            self.report(
                site,
                Issue::UnsupportedField("authored NPC health capacity"),
            );
        }
        Ok(())
    }

    pub(in crate::script_init) fn begin_health_pool(
        &mut self,
        key: StateKey,
        node: &Node,
        wave_bonus: bool,
    ) -> Result<(), InitFault> {
        let id = key.instance;
        let site = self.site(id, node);
        if !matches!(
            self.evaluate(id, node, field(&node.fields, "m_FA3ECC43"), None),
            Some(Value::Entity(target)) if target == site.entity
        ) {
            self.report(site, Issue::UnsupportedField("health pool target"));
            return Ok(());
        }
        if wave_bonus {
            return self.begin_wave_health(key, node, site);
        }
        let Some(maximum) = self
            .evaluate(id, node, field(&node.fields, "m_amount"), Some(key))
            .and_then(pool_number)
        else {
            self.report(site, Issue::UnsupportedField("health pool capacity"));
            return Ok(());
        };
        if let Some(identifier) = named_temporary_shield(node, key.raw) {
            let instance = &self.machine.snapshot.instances[&id];
            let stacked = instance.parent.is_none() && instance.retire_after_frame.is_some();
            if maximum == 0.0 {
                if stacked {
                    self.post(id, Event::Finish(key.raw))?;
                }
                return Ok(());
            }
            let Some(pool) = self
                .world
                .add_temporary_shield(site, identifier, maximum, stacked)
            else {
                self.report(
                    site,
                    Issue::UnsupportedField("temporary shield registration"),
                );
                return Ok(());
            };
            self.state_mut(key).expect("active temporary shield").kind =
                StateKind::HealthPool { pool: Some(pool) };
            if stacked {
                self.register_tick(key, self.machine.now_ms);
            }
            return self.publish_health_totals(site);
        }
        let initial = field(&node.fields, "m_BCFDD2E9");
        let current = if initial.is_null() {
            maximum
        } else {
            let Some(current) = self.evaluate(id, node, initial, None).and_then(pool_number) else {
                self.report(site, Issue::UnsupportedField("health pool initial amount"));
                return Ok(());
            };
            current
        };
        let kind = pool_kind(field(&node.fields, "m_7E8B666C")).expect("validated pool kind");
        let Some(pool) = self.world.add_health_pool(site, kind, maximum, current) else {
            self.report(site, Issue::UnsupportedField("health pool registration"));
            return Ok(());
        };
        self.state_mut(key).expect("active health pool").kind =
            StateKind::HealthPool { pool: Some(pool) };
        self.register_tick(key, self.machine.now_ms);
        self.publish_health_pool(key, node, current)
    }

    pub(in crate::script_init) fn begin_wave_health(
        &mut self,
        key: StateKey,
        node: &Node,
        site: Site,
    ) -> Result<(), InitFault> {
        const DIFFICULTY: u32 = 9948;
        let amount = field(&node.fields, "m_amount");
        let actual = self.world.entity_variable(GAME_MODE_ENTITY, DIFFICULTY);
        let local = self
            .machine
            .value(VarKey::Instance(key.instance, DIFFICULTY), None);
        let Some(Value::Integer(difficulty @ 0..=3)) = actual else {
            self.report(site, Issue::UnsupportedField("wave health difficulty"));
            return Ok(());
        };
        if Some(local) != actual
            || self
                .world
                .body_health(site, Some(PoolKind::Health), true)
                .is_none_or(|health| health <= 0.0)
        {
            self.report(
                site,
                Issue::UnsupportedField("wave health difficulty or base health"),
            );
            return Ok(());
        }
        let table = array_field(field(amount, "m_814F8EC3"), "m_configVars");
        let Some(amount) = self
            .evaluate(key.instance, node, &table[difficulty as usize], None)
            .and_then(pool_number)
        else {
            self.report(site, Issue::UnsupportedField("wave health amount"));
            return Ok(());
        };
        if amount == 0.0 && node.pos == 43 && difficulty == 0 {
            return Ok(());
        }
        if amount <= 0.0 {
            self.report(site, Issue::UnsupportedField("wave health amount"));
            return Ok(());
        }
        let Some(pool) = self.world.add_wave_health(site, amount) else {
            self.report(site, Issue::UnsupportedField("wave health registration"));
            return Ok(());
        };
        if let Some(pool) = pool {
            self.state_mut(key).expect("active wave health pool").kind =
                StateKind::HealthPool { pool: Some(pool) };
            self.publish_health_totals(site)?;
        }
        Ok(())
    }

    pub(in crate::script_init) fn publish_health_pool(
        &mut self,
        key: StateKey,
        node: &Node,
        current: f64,
    ) -> Result<(), InitFault> {
        if !field(&node.fields, "m_7BE02F89").is_null() {
            // The reference exposes the named value through the pool query.
            return self.publish_health_totals(self.site(key.instance, node));
        }
        let output = script_expr::lvalue(field(&node.fields, "m_CEAB19E8"))
            .expect("validated health pool output");
        if let Some(output) = output {
            let output = source_key(key.instance, output);
            let current = Value::Float(current);
            if !same(&self.machine.value(output, None), &current) {
                self.write(output, current)?;
            }
        }
        self.publish_health_totals(self.site(key.instance, node))
    }

    pub(in crate::script_init) fn publish_health_totals(
        &mut self,
        site: Site,
    ) -> Result<(), InitFault> {
        for (variable, maximum) in [
            (OWNER_HEALTH, false),
            (HUD_HEALTH, false),
            (HUD_MAX_HEALTH, true),
        ] {
            if let Some(amount) = self.world.body_health(site, None, maximum) {
                let key = VarKey::Entity(variable);
                let value = Value::Float(amount);
                if !same(&self.machine.value(key, None), &value) {
                    self.write(key, value)?;
                }
            }
        }
        Ok(())
    }

    fn rejects_stack_value(&mut self, key: StateKey, node: &Node, value: &Value) -> bool {
        if node.declares_stack_claim() && matches!(value, Value::Map(_) | Value::IdentifierMap(_)) {
            self.report(
                self.site(key.instance, node),
                Issue::UnsupportedField("map-valued stack claim"),
            );
            true
        } else {
            false
        }
    }

    pub(in crate::script_init) fn begin_stack(
        &mut self,
        key: StateKey,
        node: &Node,
        depth: usize,
    ) -> Result<(), InitFault> {
        let id = key.instance;
        let Some(value) = self.evaluate(
            id,
            node,
            node.stack_value().unwrap_or(&Json::Null),
            Some(key),
        ) else {
            return Ok(());
        };
        if self.rejects_stack_value(key, node, &value) {
            return Ok(());
        }
        let (variable, priority) = if let Some((variable, priority, duration)) = node.stack_claim()
        {
            if let Some(frames) = duration {
                // The top subgraph can synchronously end its owner.
                self.post_after(
                    id,
                    u64::from(frames.get()) * FRAME_MS,
                    Event::Finish(key.raw),
                )?;
            }
            (variable, priority)
        } else {
            let variable = match script_expr::lvalue(field(&node.fields, "m_out_Var")) {
                Ok(Some(variable)) => variable,
                Ok(None) => return Ok(()),
                Err(error) => {
                    self.report(self.site(id, node), Issue::Expression(error));
                    return Ok(());
                }
            };
            let priority = match script_expr::stack_priority(field(&node.fields, "m_priority")) {
                Ok(priority) => priority,
                Err(error) => {
                    self.report(self.site(id, node), Issue::Expression(error));
                    return Ok(());
                }
            };
            (variable, priority)
        };
        let target = source_key(id, variable);
        self.state_mut(key).expect("active stack").linked_variable = Some(target);
        self.insert_link(
            target,
            VariableLink {
                state: key,
                priority,
                output: LinkValue::Fixed(value),
                stack: true,
            },
            depth,
        )
    }

    pub(in crate::script_init) fn begin_cosmetic(
        &mut self,
        key: StateKey,
        node: &Node,
        depth: usize,
    ) -> Result<(), InitFault> {
        let target = match script_expr::lvalue(field(&node.fields, "m_1DEF59FA")) {
            Ok(Some(target)) => source_key(key.instance, target),
            Ok(None) => return Ok(()),
            Err(error) => {
                self.report(self.site(key.instance, node), Issue::Expression(error));
                return Ok(());
            }
        };
        self.state_mut(key)
            .expect("active cosmetic")
            .linked_variable = Some(target);
        self.insert_link(
            target,
            VariableLink {
                state: key,
                priority: LINK_PRIORITY,
                stack: false,
                output: LinkValue::Fixed(Value::Handle {
                    kind: 1,
                    instance: u32::from(key.instance),
                    state: key.raw,
                    extra: 0,
                }),
            },
            depth,
        )
    }

    pub(in crate::script_init) fn begin_chase(
        &mut self,
        key: StateKey,
        node: &Node,
        depth: usize,
    ) -> Result<(), InitFault> {
        let id = key.instance;
        let target = match script_expr::lvalue(field(&node.fields, "m_out_Var")) {
            Ok(target) => target.map(|target| source_key(id, target)),
            Err(error) => {
                self.report(self.site(id, node), Issue::Expression(error));
                return Ok(());
            }
        };
        let initial = field(&node.fields, "m_EB0FC261");
        let current = if initial.is_null() {
            target.map_or(Value::Null, |target| self.machine.value(target, None))
        } else {
            let Some(value) = self.evaluate(id, node, initial, None) else {
                return Ok(());
            };
            value
        };
        let current = chase_value(&current);
        let remaining_ms = if configured(node, "m_5BEFF040") {
            let Some(value) = self.evaluate(id, node, field(&node.fields, "m_5BEFF040"), None)
            else {
                return Ok(());
            };
            Some((script_expr::to_float(&value) * 1000.0) as i64)
        } else {
            None
        };
        let now = self.machine.now_ms;
        let state = self.state_mut(key).expect("active chase");
        state.chase = Some(ChasePayload {
            current: current.clone(),
            reached: false,
            remaining_ms,
            last_ms: now,
        });
        state.linked_variable = target;
        if let Some(target) = target {
            let old = self.machine.value(target, None);
            self.set_base(target, current);
            self.insert_link(
                target,
                VariableLink {
                    state: key,
                    priority: LINK_PRIORITY,
                    output: LinkValue::Chase,
                    stack: false,
                },
                depth,
            )?;
            self.notify_value(target, &old, None)?;
        }
        self.register_tick(key, now);
        Ok(())
    }

    pub(in crate::script_init) fn tick_chase(&mut self, key: StateKey) -> Result<(), InitFault> {
        let id = key.instance;
        let now = self.machine.now_ms;
        let Some(state) = self.state_mut(key) else {
            return Ok(());
        };
        let target = state.linked_variable;
        let Some(chase) = state.chase.as_mut() else {
            return Ok(());
        };
        let elapsed = (now - chase.last_ms) as i64;
        chase.last_ms = now;
        if elapsed == 0 || chase.reached {
            return Ok(());
        }
        let mut next = chase.clone();
        let before = target.map(|target| self.machine.value(target, None));
        let node = self
            .graph(id)
            .state(usize::from(key.raw))
            .expect("chase state");
        let Some(destination) =
            self.evaluate(id, node, field(&node.fields, "m_destination"), Some(key))
        else {
            return Ok(());
        };
        let destination = chase_value(&destination);
        if let Some(remaining) = next.remaining_ms {
            if remaining <= elapsed {
                next.current = destination;
                next.reached = true;
            } else {
                let share = f64::from((elapsed as f64 / remaining as f64) as f32);
                next.current = chase_lerp(next.current, destination, share);
                next.remaining_ms = Some(remaining - elapsed);
            }
        } else {
            let rate = field(&node.fields, "m_9B0385B4");
            let rate = if rate.is_null() {
                1.0
            } else {
                let Some(value) = self.evaluate(id, node, rate, Some(key)) else {
                    return Ok(());
                };
                script_expr::to_float(&value)
            };
            if rate > 0.0 {
                let per_ms = (rate * f64::from(0.001_f32)) as f32;
                let distance = (f64::from(per_ms) * elapsed as f64) as f32;
                let step = f64::from((f64::from(distance) + script_expr::EPSILON) as f32);
                (next.current, next.reached) = chase_step(next.current, destination, step);
            }
        }
        let reached = next.reached;
        self.state_mut(key).expect("active chase").chase = Some(next);
        if let (Some(target), Some(before)) = (target, before) {
            self.notify_value(target, &before, None)?
        }
        if reached {
            self.post(id, Event::CheckTransitions(key.raw))?;
            if !configured(node, "m_CB671004") {
                self.post(id, Event::Finish(key.raw))?
            }
        }
        Ok(())
    }

    pub(in crate::script_init) fn chase_write(
        &mut self,
        key: StateKey,
        target: VarKey,
    ) -> Result<(), InitFault> {
        let id = key.instance;
        let node = self
            .graph(id)
            .state(usize::from(key.raw))
            .expect("chase state");
        match field(&node.fields, "m_1E10B2B1").as_i64().unwrap_or(0) {
            0 => self.unlink_single_output(key, self.output_write_depth)?,
            1 => {
                let value = chase_value(&stored(&self.machine.snapshot, target));
                if let Some(chase) = self.state_mut(key).and_then(|state| state.chase.as_mut()) {
                    chase.current = value;
                }
            }
            _ => (),
        }
        Ok(())
    }

    pub(in crate::script_init) fn finish_chase(
        &mut self,
        key: StateKey,
        depth: usize,
    ) -> Result<(), InitFault> {
        let Some(state) = self.state_mut(key) else {
            return Ok(());
        };
        let Some(target) = state.linked_variable.take() else {
            return Ok(());
        };
        let current = state.chase.as_ref().map(|chase| chase.current.clone());
        self.remove_link(key, target, current, depth)
    }

    pub(in crate::script_init) fn volley_config(
        &mut self,
        id: u16,
        node: &Node,
        name: &str,
        default: Value,
    ) -> Option<Value> {
        let config = field(&node.fields, name);
        if config.is_null() {
            Some(default)
        } else {
            self.evaluate(id, node, config, None)
        }
    }

    pub(in crate::script_init) fn begin_volley(
        &mut self,
        key: StateKey,
        node: &Node,
        depth: usize,
    ) -> Result<(), InitFault> {
        let id = key.instance;
        let names = [
            "m_A17BE89B",
            "m_A55BC904",
            "m_5F40D9D3",
            "m_numShotsPerSecond",
        ];
        let mut config = [
            Value::Integer(1),
            Value::Integer(1),
            Value::Integer(1),
            Value::Float(1.0),
        ];
        for (value, name) in config.iter_mut().zip(names) {
            let Some(found) = self.volley_config(id, node, name, value.clone()) else {
                return Ok(());
            };
            *value = found;
        }
        let [max, min, per, rate] = config;
        let (Ok(maximum), Ok(minimum)) = (
            u32::try_from(script_expr::to_int(&max, 0).max(0)),
            u32::try_from(script_expr::to_int(&min, 0).max(0)),
        ) else {
            self.report(
                self.site(id, node),
                Issue::UnsupportedField("weapon volley shot count"),
            );
            return Ok(());
        };
        let per_shot = script_expr::to_int(&per, 0).max(0);
        let rate = if rate == Value::Null {
            1.0
        } else {
            script_expr::to_float(&rate).max(1e-5)
        };
        if !rate.is_finite() {
            self.report(
                self.site(id, node),
                Issue::UnsupportedField("weapon volley rate"),
            );
            return Ok(());
        }
        let mut outputs = [None; 3];
        for (output, name) in outputs
            .iter_mut()
            .zip(["m_24E351A4", "m_17770E7E", "m_3FFC9EB9"])
        {
            let cfg = field(&node.fields, name);
            if cfg.is_null() {
                continue;
            }
            *output = match script_expr::lvalue(cfg) {
                Ok(Some(var)) => Some(var),
                Ok(None) => {
                    self.report(
                        self.site(id, node),
                        Issue::UnsupportedField("weapon volley output"),
                    );
                    return Ok(());
                }
                Err(error) => {
                    self.report(self.site(id, node), Issue::Expression(error));
                    return Ok(());
                }
            };
        }
        let [ammo_var, _, _] = outputs;
        let outputs = outputs.map(|output| output.map(|var| source_key(id, var)));
        for (index, output) in outputs.iter().enumerate() {
            if output.is_some() && outputs[..index].contains(output) {
                self.report(
                    self.site(id, node),
                    Issue::UnsupportedField("duplicate weapon volley output"),
                );
                return Ok(());
            }
        }
        let ammo0 = outputs[0].map_or_else(
            || i64::from(maximum).saturating_mul(per_shot),
            |var| script_expr::to_int(&self.machine.value(var, None), 0).max(0),
        );
        let spread = outputs[2].map_or(0.0, |var| {
            script_expr::to_float(&self.machine.value(var, None)).clamp(0.0, 1.0)
        });
        if !spread.is_finite() {
            self.report(
                self.site(id, node),
                Issue::UnsupportedField("weapon volley spread"),
            );
            return Ok(());
        }
        let planned = if per_shot == 0 {
            maximum
        } else {
            (ammo0 / per_shot).min(i64::from(maximum)) as u32
        };
        let launch = field(&node.fields, "m_5EC1EB38");
        let (spread_ramp, spread_delay_ms) = if launch["$"] == "STU_239B571E" {
            let delay = field(launch, "m_23C72C36");
            let delay_ms = if delay.is_null() {
                Some(0)
            } else {
                self.evaluate(id, node, delay, None)
                    .map(|value| script_expr::to_float(&value))
                    .filter(|secs| secs.is_finite() && *secs >= 0.0)
                    .map(|secs| (secs * 1000.0).round() as u64)
            };
            let config = field(launch, "m_AC20B1C1");
            let ramp = if config.is_null() {
                Some(0.0)
            } else {
                self.evaluate(id, node, config, None)
                    .map(|value| script_expr::to_float(&value) as f32)
                    .filter(|ramp| ramp.is_finite() && *ramp >= 0.0)
            };
            match (ramp, delay_ms) {
                (Some(ramp), Some(delay_ms)) => (Some(ramp), delay_ms),
                _ => (None, 0),
            }
        } else {
            (None, 0)
        };
        let start_ms = self.machine.now_ms;
        self.state_mut(key).expect("active volley").kind = StateKind::WeaponVolley {
            volleys: 1,
            offset_ms: 0,
            ammo0,
            ammo_var,
        };
        self.machine.volleys.insert(
            key,
            Volley {
                start_ms,
                ammo0,
                per_shot,
                rate,
                spread,
                spread_ramp,
                spread_delay_ms,
                planned,
                reported: 0,
                unposed_rounds: 0,
                outputs,
            },
        );
        for (slot, (target, base)) in outputs
            .into_iter()
            .zip([
                Value::Integer(ammo0),
                Value::Float(rate),
                Value::Float(spread),
            ])
            .enumerate()
        {
            let Some(target) = target else { continue };
            let before = self.machine.value(target, None);
            self.set_base(target, base);
            self.insert_link(
                target,
                VariableLink {
                    state: key,
                    priority: LINK_PRIORITY,
                    stack: false,
                    output: LinkValue::Volley(slot as u8),
                },
                depth,
            )?;
            self.notify_value(target, &before, None)?;
        }
        if per_shot == 0 || i64::from(minimum) < ammo0 / per_shot {
            self.post_after(
                id,
                shot_delay(minimum.saturating_sub(1), rate),
                Event::Timer(key.raw, 1),
            )?;
        }
        self.post_after(
            id,
            shot_delay(planned.saturating_sub(1), rate),
            Event::Timer(key.raw, 2),
        )
    }

    pub(in crate::script_init) fn report_active_volleys(&mut self) -> Result<(), InitFault> {
        let active: Vec<_> = self
            .machine
            .snapshot
            .instances
            .iter()
            .flat_map(|(&id, instance)| {
                instance.states.iter().filter_map(move |(&raw, state)| {
                    (state.active && matches!(state.kind, StateKind::WeaponVolley { .. }))
                        .then_some(StateKey { instance: id, raw })
                })
            })
            .collect();
        for key in active {
            self.report_volley(key)?
        }
        Ok(())
    }

    pub(in crate::script_init) fn report_volley(&mut self, key: StateKey) -> Result<(), InitFault> {
        if self.machine.owner_dead {
            return Ok(());
        }
        let Some(activation) = self.state(key).map(|state| state.activation) else {
            return Ok(());
        };
        let instance = &self.machine.snapshot.instances[&key.instance];
        let Some(volley) = self.machine.volleys.get_mut(&key) else {
            return Ok(());
        };
        if !self.machine.permit_weapon_shots {
            // Locate the first round past the new clock without walking an
            // arbitrarily long, unposed interval one shot at a time.
            let mut first = volley.reported;
            let mut end = volley.planned;
            while first < end {
                let middle = first + (end - first) / 2;
                let due = volley
                    .start_ms
                    .saturating_add(shot_delay(middle, volley.rate));
                if due <= self.machine.now_ms {
                    first = middle + 1;
                } else {
                    end = middle;
                }
            }
            volley.unposed_rounds += first - volley.reported;
            volley.reported = first;
            return Ok(());
        }
        let start_ms = volley.start_ms;
        let rate = volley.rate;
        let graph = instance.graph;
        let mut ordinal = volley.reported;
        let planned = volley.planned;
        let mut presentation = None;
        let spread = (volley.spread, volley.spread_ramp, volley.spread_delay_ms);
        while ordinal < planned {
            let time_ms = start_ms.saturating_add(shot_delay(ordinal, rate));
            if time_ms > self.machine.now_ms {
                break;
            }
            if self.machine.shots.len() == MAX_PENDING_SHOTS {
                return Err(InitFault::PendingShots);
            }
            let (presentation, presentation_states) =
                presentation.get_or_insert_with(|| self.volley_presentation(key));
            self.machine.shots.push(ReportedShot {
                event: ShotEvent {
                    entity: self.machine.snapshot.entity,
                    instance: key.instance,
                    graph,
                    raw_state: key.raw,
                    frame: (time_ms / FRAME_MS) as u32,
                    time_ms,
                    activation,
                    activation_ms: start_ms,
                    ordinal,
                },
                presentation: presentation.clone(),
                presentation_states: presentation_states.clone(),
                normalized_spread: Volley::normalized_shot_spread(
                    spread.0,
                    spread.1,
                    spread.2,
                    time_ms - start_ms,
                    rate,
                ),
            });
            ordinal += 1;
        }
        self.machine
            .volleys
            .get_mut(&key)
            .expect("reported volley")
            .reported = ordinal;
        Ok(())
    }

    fn volley_presentation(&self, key: StateKey) -> (Vec<(Variable, Value)>, Vec<u16>) {
        let instance = &self.machine.snapshot.instances[&key.instance];
        if instance.is_server_only() {
            return (Vec::new(), Vec::new());
        }
        let graph = self.graph(key.instance);
        let Some(volley_node) = graph.state(usize::from(key.raw)) else {
            return (Vec::new(), Vec::new());
        };
        let is_synced = |variable: &Variable| {
            let scope = u8::from(variable.scope == Scope::Entity);
            graph
                .sync_vars
                .iter()
                .any(|sync| sync.0 == Some(variable.id) && sync.1 == scope)
                && (variable.scope == Scope::Entity
                    || graph
                        .owner_only_instance_vars()
                        .binary_search(&variable.id)
                        .is_err())
                && !graph.server_only_variables().any(|id| id == variable.id)
        };
        let mut presented = Vec::new();
        let mut presented_states = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut push_var = |variable: Variable, value: Value| {
            if seen.insert((matches!(variable.scope, Scope::Entity), variable.id)) {
                presented.push((variable, value));
            }
        };
        if let Some(plug) = volley_node.field("m_subgraphPlug") {
            let children = array_field(plug, SUBGRAPH_STATES);
            if !children.is_empty() {
                for (&raw, state) in &instance.states {
                    if !state.active {
                        continue;
                    }
                    let Some(node) = graph.state(usize::from(raw)) else {
                        continue;
                    };
                    if raw_indices(children).any(|child| child == raw)
                        && node.is_networked()
                        && graph.remote_positions().contains(&node.pos)
                        && matches!(
                            node.class.as_str(),
                            "STUStatescriptStateStack" | "STUStatescriptStateDataFlowMapping"
                        )
                    {
                        presented_states.push(raw);
                    }
                    let var_field = match node.class.as_str() {
                        "STUStatescriptStateDataFlowMapping"
                            if node.is_networked()
                                && graph.remote_positions().contains(&node.pos) =>
                        {
                            "m_answer"
                        }
                        "STUStatescriptStateStack"
                            if node.is_networked()
                                && graph.remote_positions().contains(&node.pos) =>
                        {
                            "m_out_Var"
                        }
                        _ => continue,
                    };
                    let Ok(Some(variable)) = script_expr::lvalue(field(&node.fields, var_field))
                    else {
                        continue;
                    };
                    if !is_synced(&variable) {
                        continue;
                    }
                    let source = source_key(key.instance, variable);
                    let Some(top) = self.machine.links.get(&source).and_then(|list| list.last())
                    else {
                        continue;
                    };
                    if top.state.instance != key.instance
                        || !raw_indices(children).any(|child| child == top.state.raw)
                    {
                        continue;
                    }
                    push_var(variable, self.machine.value(source, None));
                }
            }
        }
        if let Some(sync_vars) = volley_node.frame_action_variables() {
            for variable in sync_vars {
                if !is_synced(&variable) {
                    continue;
                }
                let value = self.machine.value(source_key(key.instance, variable), None);
                if !matches!(value, Value::Null) {
                    push_var(variable, value);
                }
            }
        }
        (presented, presented_states)
    }

    pub(in crate::script_init) fn finish_volley(
        &mut self,
        key: StateKey,
        depth: usize,
    ) -> Result<(), InitFault> {
        self.report_volley(key)?;
        let Some(volley) = self.machine.volleys.get(&key) else {
            return Ok(());
        };
        let outputs = volley.outputs;
        let at = self.machine.now_ms.saturating_add(1);
        let values =
            [0, 1, 2].map(|slot| volley.output(slot, at, self.machine.permit_weapon_shots));
        for (target, value) in outputs.into_iter().zip(values) {
            if let Some(target) = target {
                self.remove_link(key, target, Some(value), depth)?
            }
        }
        self.machine.volleys.remove(&key);
        Ok(())
    }

    pub(in crate::script_init) fn insert_link(
        &mut self,
        target: VarKey,
        link: VariableLink,
        depth: usize,
    ) -> Result<(), InitFault> {
        let old = self.machine.value(target, None);
        match target {
            VarKey::Entity(id) => self
                .machine
                .snapshot
                .variables
                .entry(id)
                .or_insert(Value::Null),
            VarKey::Instance(instance, id) => self.machine.snapshot.instances[&instance]
                .variables
                .entry(id)
                .or_insert(Value::Null),
        };
        let entries = self.machine.links.entry(target).or_default();
        entries.retain(|entry| entry.state != link.state);
        let at = entries
            .iter()
            .position(|entry| {
                entry.priority.level > link.priority.level
                    || (entry.priority.level == link.priority.level && !link.priority.above_equals)
            })
            .unwrap_or(entries.len());
        entries.insert(at, link);
        self.notify_value(target, &old, None)?;
        self.sync_stack(target, depth)
    }

    pub(in crate::script_init) fn sync_stack(
        &mut self,
        variable: VarKey,
        depth: usize,
    ) -> Result<(), InitFault> {
        if depth >= MAX_PLUG_DEPTH {
            return Err(InitFault::PlugDepth);
        }
        let Some(entries) = self.machine.links.get(&variable) else {
            return Ok(());
        };
        let states: Vec<_> = entries
            .iter()
            .filter(|entry| entry.stack)
            .map(|entry| entry.state)
            .collect();
        for key in states {
            self.spend("stack position change")?;
            let Some(state) = self
                .state(key)
                .filter(|state| state.active && !state.ending)
            else {
                continue;
            };
            let top = self
                .machine
                .links
                .get(&variable)
                .and_then(|entries| entries.last());
            if let StateKind::Ability { flags } = state.kind {
                if flags & ABILITY_PUSHED != 0 && state.ability_stack == Some(variable) {
                    let node = self
                        .graph(key.instance)
                        .state(usize::from(key.raw))
                        .expect("ability state");
                    if top.is_some_and(|entry| {
                        entry.state == key && matches!(entry.output, LinkValue::AbilityStack)
                    }) {
                        if flags & ABILITY_RUNNING == 0 {
                            self.start_ability(key, node, depth + 1)?;
                        }
                    } else {
                        self.stop_ability(key, node, depth + 1)?;
                    }
                }
                continue;
            }
            if state.linked_variable != Some(variable) {
                continue;
            }
            let top = top.is_some_and(|entry| entry.state == key);
            let kind = StateKind::Stack { top, under: !top };
            if state.kind == kind {
                continue;
            }
            self.state_mut(key).expect("active stack").kind = kind;
            let node = self
                .graph(key.instance)
                .state(usize::from(key.raw))
                .expect("stack state");
            let (old, next) = if top {
                ("m_underSubgraphPlug", "m_topSubgraphPlug")
            } else {
                ("m_topSubgraphPlug", "m_underSubgraphPlug")
            };
            self.exit_subgraph(key.instance, node, old, depth + 1)?;
            self.follow(key.instance, node, next, Some(key), depth)?;
        }
        Ok(())
    }

    pub(in crate::script_init) fn unlink_single_output(
        &mut self,
        key: StateKey,
        depth: usize,
    ) -> Result<(), InitFault> {
        let Some(target) = self
            .state_mut(key)
            .and_then(|state| state.linked_variable.take())
        else {
            return Ok(());
        };
        self.remove_link(key, target, None, depth)
    }

    pub(in crate::script_init) fn remove_link(
        &mut self,
        key: StateKey,
        target: VarKey,
        bake: Option<Value>,
        depth: usize,
    ) -> Result<(), InitFault> {
        self.unlink_output(key, target, bake)?;
        self.sync_stack(target, depth)
    }

    pub(in crate::script_init) fn unlink_output(
        &mut self,
        key: StateKey,
        target: VarKey,
        bake: Option<Value>,
    ) -> Result<(), InitFault> {
        let old = self.machine.value(target, None);
        if let Some(value) = bake {
            self.set_base(target, value);
        }
        if let Some(entries) = self.machine.links.get_mut(&target) {
            entries.retain(|entry| entry.state != key);
            if entries.is_empty() {
                self.machine.links.remove(&target);
            }
        }
        self.notify_value(target, &old, None)
    }

    pub(in crate::script_init) fn update_stack(&mut self, key: StateKey) -> Result<(), InitFault> {
        let Some(target) = self.state(key).and_then(|state| state.linked_variable) else {
            return Ok(());
        };
        let id = key.instance;
        let node = self
            .graph(id)
            .state(usize::from(key.raw))
            .expect("stack state");
        let Some(value) = node.stack_value().filter(|config| !config.is_null()) else {
            return Ok(());
        };
        let Some(value) = self.evaluate(id, node, value, Some(key)) else {
            return Ok(());
        };
        if self.rejects_stack_value(key, node, &value) {
            return Ok(());
        }
        let old = self.machine.value(target, None);
        if let Some(LinkValue::Fixed(current)) = self
            .machine
            .links
            .get_mut(&target)
            .and_then(|entries| entries.iter_mut().find(|entry| entry.state == key))
            .map(|entry| &mut entry.output)
        {
            if !same(current, &value) {
                *current = value;
                self.notify_value(target, &old, Some(key))?;
            }
        }
        Ok(())
    }

    #[inline(never)]
    pub(in crate::script_init) fn begin_pet(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let id = key.instance;
        let site = self.site(id, node);
        let Some(Value::Asset(definition)) = self.evaluate(
            id,
            node,
            field(&node.fields, "m_entityDefinition"),
            Some(key),
        ) else {
            return Ok(());
        };
        if definition >> 48 != 0x0400 || definition & 0x0000_ffff_ffff_ffff == 0 {
            return Ok(());
        }
        let attach = match self.evaluate(id, node, field(&node.fields, "m_AA26E7DD"), Some(key)) {
            Some(Value::Entity(entity)) if script_expr::valid_entity(entity) => Some(entity),
            _ => None,
        };
        let position = match self.evaluate(id, node, field(&node.fields, "m_position"), Some(key)) {
            Some(Value::Vector(pos)) if pos.iter().all(|c| c.is_finite()) => Some(pos),
            Some(Value::Entity(entity)) if script_expr::valid_entity(entity) => {
                self.world.position(entity)
            }
            _ => None,
        };
        let Some(pet_id) = self.world.spawn_pet(
            site,
            PetSpawn {
                definition,
                attach,
                position,
            },
        ) else {
            return Ok(());
        };
        if let Some(state) = self.state_mut(key) {
            state.kind = StateKind::Pet {
                entity: Some(pet_id),
            };
        }
        if let Ok(Some(target)) = script_expr::lvalue(field(&node.fields, "m_B3019338")) {
            self.write(source_key(id, target), Value::Entity(pet_id))?;
        }
        Ok(())
    }

    #[inline(never)]
    pub(in crate::script_init) fn end_pet(
        &mut self,
        key: StateKey,
        node: &Node,
        entity: u32,
    ) -> Result<(), InitFault> {
        let id = key.instance;
        let site = self.site(id, node);
        self.world.despawn_pet(site, entity);
        if let Ok(Some(target)) = script_expr::lvalue(field(&node.fields, "m_B3019338")) {
            let var_key = source_key(id, target);
            if same(&self.machine.value(var_key, None), &Value::Entity(entity)) {
                self.write(var_key, Value::Null)?;
            }
        }
        Ok(())
    }

    // Publish on activation, dependency changes and explicit polling, rather
    // than rerolling random-valued rows whenever a mover reads its properties.
    pub(in crate::script_init) fn refresh_movement_modifier(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let id = key.instance;
        if let Some((factor, _)) = node.movement_scale() {
            self.spend("movement modifier")?;
            self.machine.movement_modifiers.insert(
                ModifierKey {
                    instance: id,
                    state: key.raw,
                    slot: 0,
                },
                Modifier {
                    index: SPEED_SLOT,
                    operation: Operation::Multiply,
                    value: narrow(factor),
                    priority: 0.0,
                },
            );
            return Ok(());
        }
        let mut modifiers = BTreeMap::new();
        for (slot, config) in array_field(&node.fields, "m_BC5E91CF").iter().enumerate() {
            self.spend("movement modifier")?;
            let shape = config.get("$").and_then(Json::as_str);
            let (operation, default) = match shape {
                Some("STU_4CB0950F") => (Operation::Add, 0.0),
                Some("STU_D8CC6B10") => (Operation::Multiply, 1.0),
                Some("STU_56DEE1BB") => (Operation::Set, 0.0),
                Some("STU_C61711C8") => (Operation::Minimum, f64::from(f32::MAX)),
                Some("STU_F197CFBF") => (Operation::Maximum, f64::from(f32::MIN_POSITIVE)),
                Some("STU_09F9B1FE")
                    if self
                        .world
                        .admits_generic_player_runtime(self.site(id, node)) =>
                {
                    if let Some(value) =
                        self.evaluate(id, node, field(config, "m_9B7A63EA"), Some(key))
                    {
                        let mode = config.get("m_CFEC5F76").and_then(Json::as_u64).unwrap_or(0);
                        let op = match (mode, value) {
                            (0, Value::Vector(v)) if v.iter().all(|c| c.is_finite()) => {
                                Some(VelocityOp::SetVector(v))
                            }
                            (0, val @ (Value::Float(_) | Value::Integer(_))) => {
                                let speed = script_expr::to_float(&val);
                                speed.is_finite().then(|| {
                                    if speed == 0.0 {
                                        VelocityOp::SetVector([0.0; 3])
                                    } else {
                                        VelocityOp::SetSpeed {
                                            axis: VelocityAxis::All,
                                            value: speed,
                                        }
                                    }
                                })
                            }
                            (1, Value::Vector([vx, _, vz])) if vx.is_finite() && vz.is_finite() => {
                                Some(VelocityOp::SetHorizontalVector([vx, vz]))
                            }
                            (1, val @ (Value::Float(_) | Value::Integer(_))) => {
                                let speed = script_expr::to_float(&val);
                                speed.is_finite().then(|| {
                                    if speed == 0.0 {
                                        VelocityOp::SetHorizontalVector([0.0, 0.0])
                                    } else {
                                        VelocityOp::SetSpeed {
                                            axis: VelocityAxis::Horizontal,
                                            value: speed,
                                        }
                                    }
                                })
                            }
                            (2, Value::Vector([_, vy, _])) if vy.is_finite() => {
                                Some(VelocityOp::SetSpeed {
                                    axis: VelocityAxis::Vertical,
                                    value: vy,
                                })
                            }
                            (2, val @ (Value::Float(_) | Value::Integer(_))) => {
                                let speed = script_expr::to_float(&val);
                                speed.is_finite().then_some(VelocityOp::SetSpeed {
                                    axis: VelocityAxis::Vertical,
                                    value: speed,
                                })
                            }
                            _ => None,
                        };
                        if let Some(op) = op {
                            self.world.apply_velocity_ops(
                                self.site(id, node),
                                self.machine.frame,
                                &[op],
                            );
                        }
                    }
                    continue;
                }
                Some("STU_ED2B22BD")
                    if self
                        .world
                        .admits_generic_player_runtime(self.site(id, node)) =>
                {
                    if let Some(component) = config.get("m_9B7A63EA").filter(|c| {
                        c.get("$").and_then(Json::as_str) == Some("STUCharacterMoverComponent")
                    }) {
                        let priority = self
                            .movement_number(key, node, field(config, "m_priority"), 0.0)
                            .unwrap_or(0.0);
                        let base_slot = 1000usize.saturating_add(slot.saturating_mul(8));
                        let mut push = |sub: usize, index: u8, operation: Operation, value: f64| {
                            if value.is_finite() {
                                modifiers.insert(
                                    ModifierKey {
                                        instance: id,
                                        state: key.raw,
                                        slot: base_slot + sub,
                                    },
                                    Modifier {
                                        index,
                                        operation,
                                        value: narrow(value),
                                        priority,
                                    },
                                );
                            }
                        };
                        if let Some(v) = component.get("m_19452F15").and_then(Json::as_f64) {
                            if v.is_finite() && v > 0.0 && (v - 5.5).abs() > 1e-4 {
                                push(0, SPEED_SLOT, Operation::Multiply, v / 5.5);
                            }
                        }
                        if let Some(v) = component.get("m_DAA0618B").and_then(Json::as_f64) {
                            if v.is_finite() && v >= 0.0 && (v - 35.0).abs() > 1e-4 {
                                push(1, 1, Operation::Multiply, v / 35.0);
                            }
                        }
                        if let Some(v) = component.get("m_E631BA84").and_then(Json::as_f64) {
                            if v.is_finite() && v >= 0.0 && (v - 3.5).abs() > 1e-4 {
                                push(2, 2, Operation::Multiply, v / 3.5);
                            }
                        }
                        if let Some(v) = component.get("m_EC289987").and_then(Json::as_f64) {
                            if v.is_finite() && v >= 0.0 && (v - 6.0).abs() > 1e-4 {
                                push(3, 10, Operation::Set, v);
                            }
                        }
                        if let Some(v) = component.get("m_C2C40969").and_then(Json::as_f64) {
                            if v.is_finite() && (v - 1.75).abs() > 1e-4 {
                                push(4, 11, Operation::Multiply, v / 1.75);
                            }
                        }
                        if let Some(v) = component.get("m_1FF06061").and_then(Json::as_f64) {
                            if v == 0.0 {
                                push(5, 22, Operation::Set, 0.0);
                            }
                        }
                    }
                    continue;
                }
                _ => {
                    self.report(
                        self.site(id, node),
                        Issue::UnsupportedField("movement modifier operation"),
                    );
                    continue;
                }
            };
            let Some(index) = config
                .get("m_A9561CA0")
                .and_then(Json::as_u64)
                .and_then(|value| u8::try_from(value).ok())
            else {
                self.report(
                    self.site(id, node),
                    Issue::UnsupportedField("movement modifier index"),
                );
                continue;
            };
            let Some(value) = self.movement_number(key, node, field(config, "m_9B7A63EA"), default)
            else {
                continue;
            };
            let Some(priority) = self.movement_number(key, node, field(config, "m_priority"), 0.0)
            else {
                continue;
            };
            modifiers.insert(
                ModifierKey {
                    instance: id,
                    state: key.raw,
                    slot,
                },
                Modifier {
                    index,
                    operation,
                    value,
                    priority,
                },
            );
        }
        self.machine
            .movement_modifiers
            .retain(|entry, _| entry.instance != id || entry.state != key.raw);
        self.machine.movement_modifiers.extend(modifiers);
        Ok(())
    }

    pub(in crate::script_init) fn movement_number(
        &mut self,
        key: StateKey,
        node: &Node,
        config: &Json,
        default: f64,
    ) -> Option<f64> {
        if config.is_null() {
            return Some(default);
        }
        let value = self.evaluate(key.instance, node, config, Some(key))?;
        Some(match value {
            Value::Null => 0.0,
            Value::Bool(_) | Value::Integer(_) | Value::Float(_) => {
                f64::from(script_expr::to_float(&value) as f32)
            }
            _ => default,
        })
    }

    pub(in crate::script_init) fn register_tick(&mut self, key: StateKey, start_ms: u64) {
        self.machine
            .ticking
            .entry(key.instance)
            .or_default()
            .entry(key.raw)
            .or_insert(start_ms);
    }

    pub(in crate::script_init) fn begin_shockwave(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        let immediate = self
            .evaluate(key.instance, node, field(&node.fields, "m_3418CEA4"), None)
            .is_some_and(|value| truthy(&value));
        let has_target = script_expr::lvalue(field(&node.fields, "m_8A2905A0"))
            .ok()
            .flatten()
            .is_some();
        if has_target
            && !immediate
            && node
                .fields
                .get("m_04F3240D")
                .and_then(|v| v.get("m_value"))
                .and_then(|v| v.as_i64())
                == Some(1)
            && !field(&node.fields, "m_05BE62DD").is_null()
            && !field(&node.fields, "m_D59188D5").is_null()
        {
            return self.begin_ground_wave(key, node);
        }
        if (site.graph, node.pos) == (0x0B27, 95)
            && has_target
            && field(&node.fields, "m_04F3240D").is_null()
            && immediate
            && self.collect_shockwave(key, node, 0.0)?
        {
            if let Some(state) = self.state_mut(key) {
                state.kind = StateKind::Shockwave { managed: true };
            }
            self.register_tick(key, self.machine.now_ms);
        } else {
            self.report(site, Issue::UnsupportedField("shockwave service"));
        }
        Ok(())
    }

    fn begin_ground_wave(&mut self, key: StateKey, node: &Node) -> Result<(), InitFault> {
        use crate::script_expr::EvalError;
        use crate::script_init::GroundWave;

        let site = self.site(key.instance, node);
        let fields = &node.fields;
        let get_profile = |field: &str| {
            fields[field]["m_C1903771"]
                .get("m_C1903771")
                .and_then(asset_guid)
        };
        if fields["m_04F3240D"]["m_value"] != 1
            || get_profile("m_3D667CE3") != Some(0x0690_0000_0000_002d)
            || get_profile("m_9A8CA980") != Some(0x0690_0000_0000_012d)
            || fields["m_filter"]["m_4EA686DB"] != 20
            || fields["m_filter"]["m_84065FD1"] != 64
            || fields["m_filter"]["m_89831602"] != 1
            || fields["m_filter"]["m_condition"].is_null()
        {
            self.report(
                site,
                Issue::UnsupportedField("ground wave collision profile"),
            );
            return Ok(());
        }
        let wave = self.with_instance_context(site, None, |context| {
            let mut vector = |name| match script_expr::evaluate(&fields[name], context)? {
                Value::Vector(xyz) if xyz.into_iter().all(f64::is_finite) => Ok(xyz),
                _ => Err(EvalError::MalformedConfig("ground wave position")),
            };
            let centre = vector("m_05BE62DD")?;
            let sight_origin = vector("m_D59188D5")?;
            let mut scalar = |name| {
                pool_number(script_expr::evaluate(&fields[name], context)?)
                    .filter(|v| v.is_finite())
                    .ok_or(EvalError::MalformedConfig("ground wave scalar"))
            };
            let wave = GroundWave {
                centre,
                sight_origin,
                yaw: scalar("m_D45E22FA")?,
                aperture: scalar("m_A30B383E")?,
                distance: scalar("m_6A79BD98")?,
                speed: scalar("m_speed")?,
                max_rise: scalar("m_E81B2985")?,
                max_drop: scalar("m_FF3B7DF2")?,
                ground_range: scalar("m_525E5A40")?,
            };
            if wave.speed <= 0.0
                || wave.distance <= 0.0
                || !(0.0..=360.0).contains(&wave.aperture)
                || [wave.max_rise, wave.max_drop, wave.ground_range]
                    .into_iter()
                    .any(|v| v < 0.0)
            {
                return Err(EvalError::MalformedConfig("ground wave bounds"));
            }
            Ok(wave)
        });
        match wave {
            Ok(wave) => self.state_mut(key).expect("wave state").ground_wave = Some(Box::new(wave)),
            Err(error) => {
                self.report(site, Issue::Expression(error));
                return Ok(());
            }
        }
        if self.collect_shockwave(key, node, 0.0)? {
            self.state_mut(key).expect("wave state").kind = StateKind::Shockwave { managed: true };
            self.register_tick(key, self.machine.now_ms);
        } else {
            self.report(site, Issue::UnsupportedField("ground wave room sample"));
        }
        Ok(())
    }

    fn collect_shockwave(
        &mut self,
        key: StateKey,
        node: &Node,
        reach: f64,
    ) -> Result<bool, InitFault> {
        let site = self.site(key.instance, node);
        let Some(target) = script_expr::lvalue(field(&node.fields, "m_8A2905A0"))
            .ok()
            .flatten()
        else {
            return Ok(false);
        };
        let wave = self
            .state(key)
            .and_then(|state| state.ground_wave.as_deref())
            .copied();
        let bodies = if let Some(wave) = wave {
            let Some(candidates) = self.world.ground_wave_targets(site, &wave, reach) else {
                return Ok(false);
            };
            let mut accepted = Vec::new();
            for candidate in candidates {
                self.spend("ground wave candidate")?;
                let previous = self
                    .parameters
                    .replace(Arc::new(Variables::from([(8, Value::Entity(candidate))])));
                let condition = self.evaluate(
                    key.instance,
                    node,
                    &node.fields["m_filter"]["m_condition"],
                    None,
                );
                self.parameters = previous;
                let Some(condition) = condition else {
                    return Ok(false);
                };
                if truthy(&condition) {
                    accepted.push(candidate);
                }
            }
            accepted
        } else {
            let Some(candidates) = self.world.shockwave_targets(site, reach) else {
                return Ok(false);
            };
            candidates
        };
        let var_key = source_key(key.instance, target);
        let mut items = match self.machine.value(var_key, None) {
            Value::Array(items) => items,
            _ => Vec::new(),
        };
        let mut seen: std::collections::HashSet<u32> = items
            .iter()
            .filter_map(|item| match *item {
                Value::Entity(id) if id != 0 => Some(id),
                _ => None,
            })
            .collect();
        let before = items.len();
        for body in bodies {
            if body != 0 && seen.insert(body) {
                items.push(Value::Entity(body));
            }
        }
        if items.len() != before {
            self.write(var_key, Value::Array(items))?;
        }
        Ok(true)
    }

    pub(in crate::script_init) fn tick_shockwave(
        &mut self,
        key: StateKey,
    ) -> Result<(), InitFault> {
        let node = self
            .graph(key.instance)
            .state(usize::from(key.raw))
            .expect("shockwave state");
        if let Some(state) = self.state(key) {
            if let Some(wave) = state.ground_wave.as_deref() {
                let elapsed = self.machine.now_ms.saturating_sub(state.start_ms);
                let reach = wave.reach(elapsed);
                let finished = reach >= wave.distance;
                if !self.collect_shockwave(key, node, reach)? {
                    self.report(
                        self.site(key.instance, node),
                        Issue::UnsupportedField("ground wave room sample"),
                    );
                    return Ok(());
                }
                if finished {
                    self.post(key.instance, Event::Finish(key.raw))?;
                }
                return Ok(());
            }
        }
        let reach = self
            .evaluate(key.instance, node, field(&node.fields, "m_6A79BD98"), None)
            .and_then(|value| match value {
                Value::Bool(_) | Value::Integer(_) | Value::Float(_) => {
                    let f = script_expr::to_float(&value);
                    Some(if f.is_finite() { f.max(0.0) } else { 0.0 })
                }
                _ => None,
            })
            .unwrap_or(0.0);
        if !self.collect_shockwave(key, node, reach)? {
            let site = self.site(key.instance, node);
            self.report(site, Issue::UnsupportedField("shockwave service"));
        }
        self.post(key.instance, Event::Finish(key.raw))
    }

    pub(in crate::script_init) fn begin_placement_selector(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        if (site.graph, node.pos) != (0x0B27, 15) {
            if self.world.admits_generic_player_runtime(site) {
                return Ok(());
            }
            self.report(site, Issue::UnsupportedState(node.class.clone()));
            return Ok(());
        }
        let enabled = !field(&node.fields, "m_B02B1FD7").is_null()
            && self
                .evaluate(key.instance, node, field(&node.fields, "m_A6BB0869"), None)
                .is_some_and(|value| truthy(&value));
        if enabled {
            self.register_tick(key, self.machine.now_ms);
        }
        Ok(())
    }

    pub(in crate::script_init) fn tick_placement_selector(
        &mut self,
        key: StateKey,
    ) -> Result<(), InitFault> {
        let node = self
            .graph(key.instance)
            .state(usize::from(key.raw))
            .expect("placement selector state");
        let site = self.site(key.instance, node);
        if (site.graph, node.pos) != (0x0B27, 15) {
            return Ok(());
        }
        let sampled = matches!(
            self.state(key).map(|state| state.kind),
            Some(StateKind::PlacementSelector { sampled: true, .. })
        );
        let frame_ms = crate::script_runtime::FRAME_MS;
        let at_frame_end = self.machine.now_ms % frame_ms == frame_ms - 1;
        if (sampled && !at_frame_end)
            || !self
                .evaluate(key.instance, node, field(&node.fields, "m_B02B1FD7"), None)
                .is_some_and(|value| truthy(&value))
        {
            return Ok(());
        }
        if let Some(StateKind::PlacementSelector { sampled, .. }) =
            self.state_mut(key).map(|state| &mut state.kind)
        {
            *sampled = true;
        }
        let point = match self.world.select_landing(site) {
            super::super::LandingSelection::Unhandled => {
                self.report(site, Issue::UnsupportedField("landing selection"));
                return Ok(());
            }
            super::super::LandingSelection::Invalid => None,
            super::super::LandingSelection::Point(point) => Some(point),
        };
        if let Some(point) = point {
            for field_name in ["m_2FECD1E3", "m_1549F0F9"] {
                if let Some(target) = script_expr::lvalue(field(&node.fields, field_name))
                    .ok()
                    .flatten()
                {
                    self.write(source_key(key.instance, target), Value::Entity(0))?;
                }
            }
            if let Some(target) = script_expr::lvalue(field(&node.fields, "m_30710B76"))
                .ok()
                .flatten()
            {
                self.write(source_key(key.instance, target), Value::Vector(point))?;
            }
        }
        if let Some(target) = script_expr::lvalue(field(&node.fields, "m_C3240FFC"))
            .ok()
            .flatten()
        {
            self.write(
                source_key(key.instance, target),
                Value::Bool(point.is_some()),
            )?;
        }
        if point.is_some() {
            self.post(key.instance, Event::CheckTransitions(key.raw))?;
        }
        Ok(())
    }

    pub(in crate::script_init) fn begin_move_to(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        if node.class == "STU_194D744E" {
            let site = self.site(key.instance, node);
            let destination =
                self.evaluate(key.instance, node, field(&node.fields, "m_BD2A95EE"), None);
            if let (Some(Value::Vector(destination)), Some(speed)) =
                (destination, self.world.ai_walk_speed(site))
            {
                self.set_move_request(key, destination, speed);
            }
            return Ok(());
        }
        let speed_config = field(&node.fields, "m_speed");
        if field(&node.fields, "m_B060F688").as_u64() != Some(0)
            || speed_config.is_null()
            || !field(&node.fields, "m_destinationBone").is_null()
            || !field(&node.fields, "m_5BEFF040").is_null()
        {
            return Ok(());
        }
        let entity_config = field(&node.fields, "m_destinationEntity");
        let entity_dest = if entity_config.is_null() {
            None
        } else {
            match self.evaluate(key.instance, node, entity_config, None) {
                Some(Value::Entity(0)) => None,
                Some(Value::Entity(entity))
                    if script_expr::valid_entity(entity)
                        && self
                            .world
                            .admits_generic_player_runtime(self.site(key.instance, node)) =>
                {
                    self.world.authored_position(entity)
                }
                _ => return Ok(()),
            }
        };
        let destination =
            match self.evaluate(key.instance, node, field(&node.fields, "m_865F3D01"), None) {
                Some(Value::Vector(destination)) => destination,
                _ => match entity_dest {
                    Some(destination) => destination,
                    None => return Ok(()),
                },
            };
        let speed = match self.evaluate(key.instance, node, speed_config, None) {
            Some(Value::Integer(n)) => n as f64,
            Some(Value::Float(f)) => f,
            _ => return Ok(()),
        };
        self.set_move_request(key, destination, speed);
        Ok(())
    }

    fn set_move_request(&mut self, key: StateKey, destination: [f64; 3], speed: f64) {
        let max_axis = MAX_MOVEMENT_AXIS;
        if speed > 0.0
            && speed <= max_axis
            && destination
                .iter()
                .all(|&axis| (-max_axis..=max_axis).contains(&axis))
        {
            if let Some(state) = self.state_mut(key) {
                state.kind = StateKind::MoveTo {
                    movement: Some((destination, speed)),
                };
            }
        }
    }
}
