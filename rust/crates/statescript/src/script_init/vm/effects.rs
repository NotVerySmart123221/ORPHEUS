//! StateScript continuous state effects, health pools, AI overrides, stacks, and weapon volleys.

use std::{collections::BTreeMap, sync::Arc};

use orpheus_runtime::format::parse_guid;
use serde_json::Value as Json;

use crate::{
    script_expr::{self, truthy},
    script_graph::{Node, asset_guid},
    script_modifiers::{Modifier, ModifierKey, Operation, SPEED_SLOT, narrow},
    script_runtime::{FRAME_MS, GAME_MODE_ENTITY, MAX_MOVEMENT_AXIS, PoolKind},
    script_wire::{Value, Variables},
};

use super::super::{
    ABILITY_PUSHED, ABILITY_RUNNING, HUD_HEALTH, HUD_MAX_HEALTH, LINK_PRIORITY, MAX_PENDING_SHOTS,
    MAX_PLUG_DEPTH, OWNER_HEALTH, SCRIPT_SOURCE,
    machine::{Event, LinkValue, StateKey, VarKey, VariableLink, Volley},
    shapes::{
        array_field, chase_lerp, chase_step, chase_value, configured, field, pool_kind,
        pool_number, same, shot_delay, source_key, stored,
    },
    types::{ChasePayload, InitFault, Issue, ShotEvent, StateKind},
    world::{
        AiBehaviorRequest, AiControlOverride, AiFacing, AiWeaponOverride, Site, TargetTagEffect,
        World,
    },
};
use super::Vm;

impl<'graph, 'world, 'machine, W: World> Vm<'graph, 'world, 'machine, W> {
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
        let targets = targets
            .into_iter()
            .map(|value| match value {
                Value::Entity(entity) => Some(entity),
                _ => None,
            })
            .collect::<Option<Vec<_>>>();
        let source = self.evaluate(key.instance, node, field(&node.fields, "m_63CB0D07"), None);
        let resource = self.evaluate(
            key.instance,
            node,
            field(&node.fields, "m_contactSet"),
            None,
        );
        let expected = 0x0120_0000_0000_0015;
        if node.remote < 0
            || source != Some(Value::Entity(site.entity))
            || resource != Some(Value::Asset(expected))
            || !targets.is_some_and(|targets| {
                self.world
                    .emit_contact_effect(site, site.entity, expected, &targets)
            })
        {
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
        if (site.graph, node.pos) == (0x0cbc, 69) {
            let source =
                self.creation_scalar(key.instance, node, field(config, "m_724CBA57"), Value::Null)?;
            let amount = self
                .creation_scalar(key.instance, node, field(config, "m_amount"), Value::Null)?
                .and_then(pool_number);
            if target != site.entity
                || source != Some(Value::Entity(site.entity))
                || amount != Some(10_000.0)
                || field(config, "m_93CDFD2F").as_u64() != Some(1)
                || !field(config, "m_instigator").is_null()
                || field(&node.fields, "m_8B707686").as_u64() != Some(1)
            {
                self.report(site, Issue::UnsupportedField("Junkenstein cleanup damage"));
                return Ok(());
            }
            if !self.machine.owner_dead {
                self.apply_npc_self_damage(site, 10_000.0)?;
            }
            return Ok(());
        }
        let self_hit = node.pos == 46 && target == site.entity;
        let other_hit =
            node.pos == 38 && target != site.entity && script_expr::valid_entity(target);
        if !(self_hit || other_hit)
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
            .filter(|amount| {
                (0.0..=f64::from(f32::MAX)).contains(amount) && (!self_hit || *amount > 0.0)
            })
        else {
            self.report(site, Issue::UnsupportedField("NPC damage amount"));
            return Ok(());
        };
        let amount = f64::from(amount as f32);
        if other_hit {
            if self.world.explosion_damage(site, target, amount) {
                self.post(key.instance, Event::Finish(key.raw))?;
            } else {
                self.report(
                    site,
                    Issue::UnsupportedField("NPC explosion damage service"),
                );
            }
            return Ok(());
        }
        self.apply_npc_self_damage(site, amount)
    }

    fn apply_npc_self_damage(&mut self, site: Site, amount: f64) -> Result<(), InitFault> {
        if !self.world.self_damage(site, amount) {
            self.report(site, Issue::UnsupportedField("NPC self damage service"));
        }
        self.publish_health_totals(site)
    }

    pub(in crate::script_init) fn begin_shield_recharge(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        let config = field(&node.fields, "m_modifyHealth");
        if field(&node.fields, "m_DD792FF0").as_u64() != Some(3)
            || field(&node.fields, "m_7B2B3F5C").as_u64() != Some(1)
            || field(config, "m_93CDFD2F").as_u64() != Some(0)
        {
            self.report(
                site,
                Issue::UnsupportedField("shield recharge configuration"),
            );
            return Ok(());
        }
        for target in [
            field(&node.fields, "m_FA3ECC43"),
            field(config, "m_724CBA57"),
        ] {
            if self.creation_scalar(key.instance, node, target, Value::Null)?
                != Some(Value::Entity(site.entity))
            {
                self.report(site, Issue::UnsupportedField("shield recharge target"));
                return Ok(());
            }
        }
        let interval = self
            .creation_scalar(
                key.instance,
                node,
                field(&node.fields, "m_95F1FB0B"),
                Value::Null,
            )?
            .and_then(pool_number);
        if interval != Some(1.0) || !self.world.recharge_shields(site, 0.0) {
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
        if self.machine.owner_dead {
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

    pub(in crate::script_init) fn refresh_summoner_beam_ray(
        &mut self,
        key: StateKey,
        node: &Node,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        let Some(contact) = self.world.summoner_beam_contact(site) else {
            self.report(site, Issue::UnsupportedField("Summoner beam ray service"));
            return Ok(());
        };
        let outputs = field(&node.fields, "m_target");
        for (name, value) in [
            (
                "m_692DFF72",
                contact.entity.map_or(Value::Null, Value::Entity),
            ),
            ("m_750377AA", Value::Bool(contact.hit)),
        ] {
            if let Some(output) = script_expr::lvalue(field(outputs, name))
                .map_err(|_| InitFault::InvalidRoot(key.instance))?
            {
                self.write(source_key(key.instance, output), value)?;
            }
        }
        self.register_tick(key, self.machine.now_ms);
        Ok(())
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
        if matches!((site.graph, site.node), (0x022f, Some(26 | 86))) {
            let Some(angle) = self
                .creation_scalar(
                    key.instance,
                    node,
                    field(&node.fields, "m_A30B383E"),
                    Value::Null,
                )?
                .and_then(pool_number)
                .filter(|angle| (0.0..=360.0).contains(angle))
            else {
                self.report(site, Issue::UnsupportedField("Deadeye target angle"));
                return Ok(());
            };
            let targets = (origin == site.entity && offset == [0.0; 3])
                .then(|| self.world.deadeye_targets(site, radius, angle))
                .flatten();
            let Some(targets) = targets else {
                self.report(site, Issue::UnsupportedField("Deadeye target service"));
                return Ok(());
            };
            return self.publish_tracked_targets(key, node, targets);
        }
        let filter = field(&node.fields, "m_filter");
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
                    Value::Entity(entity) if self.world.deadeye_target_alive(site, entity) => {
                        Some(entity)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        self.retain_target_children(key, &selected)?;
        for entity in selected {
            let Some(child) = self.ensure_target_child(key, entity, TARGET_GRAPH, false, || {
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

    pub(in crate::script_init) fn update_ai_facing(
        &mut self,
        key: StateKey,
        node: &Node,
        previous: Option<AiFacing>,
    ) -> Result<(), InitFault> {
        let site = self.site(key.instance, node);
        let (target, priority) = if let Some(previous) = previous {
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
        let Some(direction) =
            self.evaluate(key.instance, node, field(&node.fields, "m_7CCD8B4B"), None)
        else {
            return Ok(());
        };
        let direction = match direction {
            Value::Vector(direction) if direction.iter().all(|axis| axis.is_finite()) => direction,
            _ => {
                self.report(site, Issue::UnsupportedField("AI facing direction"));
                return Ok(());
            }
        };
        let record = AiFacing {
            target,
            direction,
            priority,
        };
        match self.world.ai_facing(site, record, true) {
            Some(true) => {
                self.state_mut(key).expect("active AI facing").kind =
                    StateKind::AiFacing(Some(record));
                self.register_tick(key, self.machine.now_ms);
            }
            Some(false) => self.post(key.instance, Event::Abort(key.raw))?,
            None => self.report(site, Issue::UnsupportedField("AI facing component")),
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
        if site.graph == 0x0cbc {
            return self.begin_wave_health(key, node, site);
        }
        let Some(maximum) = self
            .evaluate(id, node, field(&node.fields, "m_amount"), Some(key))
            .and_then(pool_number)
        else {
            self.report(site, Issue::UnsupportedField("health pool capacity"));
            return Ok(());
        };
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

    pub(in crate::script_init) fn begin_stack(
        &mut self,
        key: StateKey,
        node: &Node,
        depth: usize,
    ) -> Result<(), InitFault> {
        let id = key.instance;
        let Some(value) = self.evaluate(id, node, field(&node.fields, "m_value"), Some(key)) else {
            return Ok(());
        };
        let target = match script_expr::lvalue(field(&node.fields, "m_out_Var")) {
            Ok(Some(target)) => source_key(id, target),
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
                planned,
                reported: 0,
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
        while volley.reported < volley.planned {
            let time_ms = volley
                .start_ms
                .saturating_add(shot_delay(volley.reported, volley.rate));
            if time_ms > self.machine.now_ms {
                break;
            }
            if self.machine.shots.len() == MAX_PENDING_SHOTS {
                return Err(InitFault::PendingShots);
            }
            self.machine.shots.push(ShotEvent {
                entity: self.machine.snapshot.entity,
                instance: key.instance,
                graph: instance.graph,
                raw_state: key.raw,
                frame: (time_ms / FRAME_MS) as u32,
                time_ms,
                activation,
                activation_ms: volley.start_ms,
                ordinal: volley.reported,
            });
            volley.reported += 1;
        }
        Ok(())
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
        let values = [
            volley.output(0, at),
            volley.output(1, at),
            volley.output(2, at),
        ];
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
        let value = field(&node.fields, "m_value");
        if value.is_null() {
            return Ok(());
        }
        let Some(value) = self.evaluate(id, node, value, Some(key)) else {
            return Ok(());
        };
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
        let Some(bodies) = self.world.shockwave_targets(site, reach) else {
            return Ok(false);
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
        let speed_config = field(&node.fields, "m_speed");
        if field(&node.fields, "m_B060F688").as_u64() != Some(0)
            || speed_config.is_null()
            || !field(&node.fields, "m_destinationBone").is_null()
            || !field(&node.fields, "m_5BEFF040").is_null()
        {
            return Ok(());
        }
        let entity_config = field(&node.fields, "m_destinationEntity");
        if !entity_config.is_null()
            && self.evaluate(key.instance, node, entity_config, None) != Some(Value::Entity(0))
        {
            return Ok(());
        }
        let Some(Value::Vector(destination)) =
            self.evaluate(key.instance, node, field(&node.fields, "m_865F3D01"), None)
        else {
            return Ok(());
        };
        let speed = match self.evaluate(key.instance, node, speed_config, None) {
            Some(Value::Integer(n)) => n as f64,
            Some(Value::Float(f)) => f,
            _ => return Ok(()),
        };
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
        Ok(())
    }
}
