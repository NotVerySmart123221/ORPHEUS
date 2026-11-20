//! Authored ruleset values and engine settings scoped to a mode.

use std::{collections::BTreeMap, io};

use serde::{Deserialize, Deserializer};
use serde_json::{Value as Json, value::RawValue};

use super::{asset_guid, invalid};
use crate::{script_expr::EngineSetting, script_wire::Value};

type RuleValues = BTreeMap<(Option<u64>, u32), Value>;

#[derive(Debug)]
pub(super) struct ModeRulesets(BTreeMap<u64, ModeProfile>);

#[derive(Debug)]
struct ModeProfile {
    rules: RuleValues,
    engine_settings: Option<BTreeMap<u32, Value>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModeRulesetDefinition {
    mode: Json,
    #[serde(default)]
    rules: Vec<RuleDefinition>,
    #[serde(default, deserialize_with = "setting_table")]
    engine_settings: Option<Vec<EngineSettingDefinition>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleDefinition {
    id: u32,
    #[serde(default, deserialize_with = "body_scope")]
    body: Option<u64>,
    value: Box<RawValue>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineSettingDefinition {
    id: u32,
    value: Box<RawValue>,
}

impl ModeRulesets {
    pub(super) fn compile(definitions: Vec<ModeRulesetDefinition>) -> io::Result<Self> {
        let mut modes = BTreeMap::new();
        for ModeRulesetDefinition {
            mode,
            rules,
            engine_settings,
        } in definitions
        {
            let mode = nonzero_guid(&mode)
                .ok_or_else(|| invalid("mode ruleset requires a nonzero canonical mode GUID"))?;
            let mut values = RuleValues::new();
            for RuleDefinition {
                id,
                body: scope,
                value,
            } in rules
            {
                let value = scalar(&value).ok_or_else(|| {
                    invalid(format!(
                        "ruleset {mode:016X}:{id} requires null, a Boolean, a signed integer or a finite float"
                    ))
                })?;
                if values.insert((scope, id), value).is_some() {
                    return Err(invalid(format!(
                        "duplicate rule {id} in mode {mode:016X} for body {scope:X?}"
                    )));
                }
            }
            let profile = ModeProfile {
                rules: values,
                engine_settings: engine_settings
                    .map(|settings| compile_engine_settings(mode, settings))
                    .transpose()?,
            };
            if modes.insert(mode, profile).is_some() {
                return Err(invalid(format!("duplicate mode ruleset {mode:016X}")));
            }
        }
        Ok(Self(modes))
    }

    pub(super) fn value(&self, mode: u64, body: u64, id: u32) -> Option<Value> {
        let values = &self.0.get(&mode)?.rules;
        values
            .get(&(Some(body), id))
            .or_else(|| values.get(&(None, id)))
            .cloned()
    }

    pub(super) fn engine_setting(&self, mode: u64, key: u32) -> Option<EngineSetting> {
        let table = self.0.get(&mode)?.engine_settings.as_ref()?;
        Some(
            table
                .get(&key)
                .cloned()
                .map_or(EngineSetting::Missing, EngineSetting::Present),
        )
    }
}

fn compile_engine_settings(
    mode: u64,
    settings: Vec<EngineSettingDefinition>,
) -> io::Result<BTreeMap<u32, Value>> {
    let mut table = BTreeMap::new();
    for EngineSettingDefinition { id, value } in settings {
        let value = scalar(&value).ok_or_else(|| {
            invalid(format!(
                "engine setting {mode:016X}:{id} requires null, a Boolean, a signed integer or a finite float"
            ))
        })?;
        if table.insert(id, value).is_some() {
            return Err(invalid(format!(
                "duplicate engine setting {id} in mode {mode:016X}"
            )));
        }
    }
    Ok(table)
}

fn setting_table<'de, D: Deserializer<'de>>(
    decoder: D,
) -> Result<Option<Vec<EngineSettingDefinition>>, D::Error> {
    Vec::deserialize(decoder).map(Some)
}

fn body_scope<'de, D: Deserializer<'de>>(decoder: D) -> Result<Option<u64>, D::Error> {
    let guid = Json::deserialize(decoder)?;
    nonzero_guid(&guid)
        .map(Some)
        .ok_or_else(|| serde::de::Error::custom("rule body requires a nonzero canonical GUID"))
}

fn nonzero_guid(value: &Json) -> Option<u64> {
    asset_guid(value).filter(|&guid| guid != 0)
}

fn scalar(value: &RawValue) -> Option<Value> {
    let value = value.get();
    match value {
        "null" => Some(Value::Null),
        "true" => Some(Value::Bool(true)),
        "false" => Some(Value::Bool(false)),
        _ if value.contains(['.', 'e', 'E']) => value
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(Value::Float),
        _ => value.parse::<i64>().ok().map(Value::Integer),
    }
}
