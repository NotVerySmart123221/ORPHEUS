//! Compatibility binding for the existing native timed projectile control.

use serde_json::Value as Json;

use crate::{
    game_content::is_junkenstein_mode,
    game_heroes::CASSIDY,
    script_expr::{self, Context, Scope, Variable},
    script_graph::{GraphStore, asset_guid},
    script_init::{Component, PlayScript, ShotEvent, Site},
    script_wire::Value,
};

pub(crate) const LEGACY_PROJECTILE_CONTROL: LegacyProjectileControl = LegacyProjectileControl {
    source_hero: CASSIDY,
    source_graph: 0x01e9,
    source_node: 7,
    effect_marker: 0x04a0_0000_0000_0079,
    wrapper_graph: 0x0228,
    wrapper_node: 7,
    duration_variable: 0x0d80_0000_0000_00d7,
    control_graph: 0x224e,
    entry_node: 4,
    stack_node: 122,
    exclusion_node: 9,
    hit_message: 0x0240_0000_0000_000f,
};

pub(crate) struct LegacyProjectileControl {
    source_hero: u64,
    source_graph: u32,
    source_node: u16,
    effect_marker: u64,
    wrapper_graph: u32,
    wrapper_node: u16,
    duration_variable: u64,
    control_graph: u32,
    entry_node: u16,
    stack_node: u16,
    exclusion_node: u16,
    hit_message: u64,
}

impl LegacyProjectileControl {
    pub(crate) fn matches_shot(&self, hero: u64, graph: u32, node: u16) -> bool {
        (hero, graph, node) == (self.source_hero, self.source_graph, self.source_node)
    }

    pub(crate) fn has_effect_marker(&self, value: &Value) -> bool {
        *value == Value::Asset(self.effect_marker)
    }

    /// The admitted flight already carries a native control duration. Its
    /// source graph identifies the compatibility reaction at this boundary.
    pub(crate) fn flight_site(&self, shot: &ShotEvent) -> Option<Site> {
        (shot.graph == self.source_graph).then_some(Site {
            entity: shot.entity,
            instance: shot.instance,
            graph: shot.graph,
            node: Some(self.source_node),
        })
    }

    pub(crate) fn matches_recipient_mode(&self, mode: u64) -> bool {
        is_junkenstein_mode(mode)
    }

    fn wrapper_overrides<'a>(&self, graphs: &'a GraphStore) -> Option<&'a [Json]> {
        graphs
            .graph(self.wrapper_graph)?
            .node(self.wrapper_node)?
            .field("m_69F4B52C.m_6525E98B.m_1EB5A024")?
            .as_array()
            .map(Vec::as_slice)
    }

    pub(crate) fn duration<'a>(&self, graphs: &'a GraphStore) -> Option<&'a Json> {
        variable_row(self.wrapper_overrides(graphs)?, self.duration_variable)?.get("m_value")
    }

    pub(crate) fn entry_admits(
        &self,
        graphs: &GraphStore,
        mut entity_value: impl FnMut(Variable) -> Option<Value>,
    ) -> Result<bool, String> {
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
            .graph(self.control_graph)
            .ok_or("Flashbang effect graph unavailable")?;
        let condition = graph
            .node(self.entry_node)
            .and_then(|node| node.field("m_condition"))
            .ok_or("Flashbang entry predicate unavailable")?;
        let variables = condition
            .get("m_expression")
            .and_then(|value| value.get("m_dynamicVars"))
            .and_then(Json::as_array)
            .ok_or("Flashbang entry variables unavailable")?;
        let schema = graph
            .fields
            .get("m_publicSchema")
            .and_then(|value| value.get("m_entries"))
            .and_then(Json::as_array)
            .ok_or("Flashbang entry defaults unavailable")?;
        let overrides = self
            .wrapper_overrides(graphs)
            .ok_or("Flashbang wrapper overrides unavailable")?;

        let mut inputs = Inputs(Vec::new());
        for config in variables {
            let variable = script_expr::dynamic(config)
                .map_err(|error| error.to_string())?
                .ok_or("Flashbang entry variable unavailable")?;
            let value = match variable.scope {
                Scope::Entity => {
                    entity_value(variable).ok_or("Flashbang target component halted")?
                }
                Scope::Instance => {
                    let identifier = 0x0d80_0000_0000_0000 | u64::from(variable.id);
                    // This entry uses schema defaults. An override must not
                    // silently change the predicate evaluated by the host.
                    if variable_row(overrides, identifier).is_some() {
                        return Err("Flashbang entry override requires evaluation".into());
                    }
                    let default = variable_row(schema, identifier)
                        .and_then(|row| row.get("m_value"))
                        .ok_or("Flashbang entry default unavailable")?;
                    script_expr::evaluate(default, &mut inputs)
                        .map_err(|error| error.to_string())?
                }
            };
            inputs.0.push((variable, value));
        }
        script_expr::evaluate(condition, &mut inputs)
            .map(|value| script_expr::truthy(&value))
            .map_err(|error| error.to_string())
    }

    pub(crate) fn stack_admits(
        &self,
        body: &Component,
        graphs: &GraphStore,
    ) -> Result<bool, String> {
        let config = graphs
            .graph(self.control_graph)
            .and_then(|graph| graph.node(self.stack_node))
            .and_then(|node| node.field("m_condition"))
            .filter(|config| config["$"] == "STU_3ACE35FB")
            .ok_or("Flashbang control-stack predicate unavailable")?;
        let variable = script_expr::dynamic(&config["m_E50CF556"])
            .map_err(|error| error.to_string())?
            .filter(|variable| variable.scope == Scope::Entity)
            .ok_or("Flashbang control-stack target unavailable")?;
        let priority = script_expr::stack_priority(&config["m_012B7AF7"])
            .map_err(|error| error.to_string())?;
        body.permits_entity_stack(variable.id, priority)
            .ok_or_else(|| "Flashbang target component halted".into())
    }

    pub(crate) fn exclusion_filter<'a>(&self, graphs: &'a GraphStore) -> Result<&'a Json, String> {
        graphs
            .graph(self.control_graph)
            .and_then(|graph| graph.node(self.exclusion_node))
            .and_then(|node| node.field("m_condition.m_filter"))
            .ok_or_else(|| "Flashbang exclusion filter unavailable".into())
    }

    pub(crate) fn exclusions_admit(
        &self,
        filter: &Json,
        presence: impl FnMut(u64) -> Option<bool>,
    ) -> Result<bool, String> {
        if filter["$"] != "STUConfigVarFilter" {
            return Err("Flashbang exclusion filter unavailable".into());
        }
        // The host checks the independent tag clause. Actor category
        // initialization follows its existing script path.
        script_expr::excluded_tags_admit(&filter["m_8B2A4DF3"], presence)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Flashbang target tags unavailable".into())
    }

    pub(crate) fn reaction(&self, mode: u64, caller: Site) -> Option<(PlayScript, u64)> {
        if !self.matches_recipient_mode(mode)
            || (caller.graph, caller.node) != (self.source_graph, Some(self.source_node))
        {
            return None;
        }
        Some((
            PlayScript {
                policy: 2,
                overrides: vec![],
                graph: self.wrapper_graph,
                source: caller.entity,
                parameters: vec![],
                auto_retire: false,
            },
            self.hit_message,
        ))
    }
}

fn variable_row(rows: &[Json], identifier: u64) -> Option<&Json> {
    rows.iter()
        .find(|row| row.get("m_0D09D2D9").and_then(asset_guid) == Some(identifier))
}
