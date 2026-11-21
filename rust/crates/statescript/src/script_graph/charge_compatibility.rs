//! Authored compatibility bindings for the stock damage charge receiver.

use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};

use orpheus_runtime::format::parse_guid;
use serde::Deserialize;
use serde_json::Value;

use crate::script_expr::{Scope, dynamic, native_fields};

use super::{
    Body, EntryTopic, GRAPH_TYPE, Graph, Node, asset_guid, invalid, message_parameter_id,
    optional_graph_index, valid_output_plug,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ChargeCompatibility {
    #[serde(deserialize_with = "required_graph_index")]
    pub receiver_graph: u32,
    #[serde(deserialize_with = "charge_compatibility_modes")]
    pub modes: BTreeSet<u64>,
    pub admission_variable: u32,
    #[serde(deserialize_with = "charge_compatibility_message")]
    pub damage_message: u64,
    /// A public root's SubScript state for a receiver mounted as its child.
    pub parent: Option<ChargeStateBinding>,
    pub inhibit: Option<ChargeStateBinding>,
}

/// A raw server-running state slot in an authored graph.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ChargeStateBinding {
    #[serde(deserialize_with = "required_graph_index")]
    pub graph: u32,
    pub state: u16,
}

fn required_graph_index<'de, D: serde::Deserializer<'de>>(decoder: D) -> Result<u32, D::Error> {
    optional_graph_index(decoder)?
        .ok_or_else(|| serde::de::Error::custom("missing charge compatibility graph GUID"))
}

fn charge_compatibility_modes<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<BTreeSet<u64>, D::Error> {
    let values = Vec::<Value>::deserialize(decoder)?;
    let mut modes = BTreeSet::new();
    for value in values {
        let mode = asset_guid(&value)
            .filter(|mode| mode >> 48 == 0x0230)
            .ok_or_else(|| {
                <D::Error as serde::de::Error>::custom("invalid charge compatibility mode GUID")
            })?;
        if !modes.insert(mode) {
            return Err(serde::de::Error::custom(
                "duplicate charge compatibility mode",
            ));
        }
    }
    if modes.is_empty() {
        return Err(serde::de::Error::custom(
            "charge compatibility requires at least one mode",
        ));
    }
    Ok(modes)
}

fn charge_compatibility_message<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<u64, D::Error> {
    let value = Value::deserialize(decoder)?;
    asset_guid(&value)
        .filter(|message| message >> 48 == 0x0240 && *message & 0x0000_ffff_ffff_ffff != 0)
        .ok_or_else(|| serde::de::Error::custom("invalid charge compatibility message GUID"))
}

fn valid_charge_damage_receiver(node: &Node) -> bool {
    if !node.runs_on_server()
        || !node.is_message_entry()
        || node.is_action()
        || node.is_state()
        || node.state.is_some()
        || node.remote != -1
        || node.declares_room_message_entry()
        || node.host_event_kind().is_some()
        || node.literal_game_message_id().is_none()
        || !node.field("m_outPlug").is_some_and(valid_output_plug)
    {
        return false;
    }

    let Some(parameters) = node.field("m_params").and_then(Value::as_array) else {
        return false;
    };
    let mut slots = BTreeSet::new();
    let mut destinations = BTreeSet::new();
    parameters.len() == 4
        && parameters.iter().all(|parameter| {
            let slot = message_parameter_id(parameter);
            let parameter_guid = parameter.get("m_B5051BCE").and_then(asset_guid);
            let destination = parameter
                .get("m_out_Var")
                .and_then(|value| crate::script_expr::lvalue(value).ok().flatten());
            matches!(slot, 0x13 | 0x15 | 0x3d | 0x624)
                && parameter_guid == Some(0x0a40_0000_0000_0000 | u64::from(slot))
                && native_fields(parameter, &["m_B5051BCE", "m_out_Var"])
                && parameter
                    .get("$")
                    .is_none_or(|class| class.as_str() == Some("STU_EF930FD7"))
                && slots.insert(slot)
                && destination.is_some_and(|variable| {
                    variable.id != 0
                        && destinations.insert((variable.scope == Scope::Entity, variable.id))
                })
        })
}

fn references_charge_admission(value: &Value, admission_variable: u32) -> bool {
    if dynamic(value).is_ok_and(|variable| {
        variable.is_some_and(|variable| {
            variable.scope == Scope::Entity && variable.id == admission_variable
        })
    }) {
        return true;
    }
    match value {
        Value::Array(items) => items
            .iter()
            .any(|item| references_charge_admission(item, admission_variable)),
        Value::Object(fields) => fields.iter().any(|(name, value)| {
            name != "m_out_Var" && references_charge_admission(value, admission_variable)
        }),
        _ => false,
    }
}

fn public_root_count(definition: &Body, graph: u32) -> usize {
    definition
        .graphs
        .iter()
        .filter(|root| root.graph == Some(graph))
        .count()
        + usize::from(definition.manager == Some(graph))
        + definition
            .weapons
            .iter()
            .filter(|root| **root == Some(graph))
            .count()
}

fn native_server_state(graphs: &BTreeMap<u32, Graph>, state: ChargeStateBinding) -> Option<&Node> {
    graphs
        .get(&state.graph)?
        .state(usize::from(state.state))
        .filter(|node| {
            node.runs_on_server()
                && node.is_state()
                && !node.is_action()
                && node.state_behavior.is_none()
                && node.state == Some(state.state)
        })
}

pub(super) fn validate_charge_compatibility(
    body: &str,
    definition: &Body,
    binding: &ChargeCompatibility,
    graphs: &BTreeMap<u32, Graph>,
) -> io::Result<()> {
    if definition.hero.as_deref().and_then(parse_guid).is_none() {
        return Err(invalid(format!(
            "body {body} requires a hero to declare charge compatibility"
        )));
    }
    if definition.authored_charge_graph.is_some() {
        return Err(invalid(format!(
            "body {body} cannot declare both charge compatibility and an authored charge graph"
        )));
    }
    if binding.admission_variable == 0 {
        return Err(invalid(format!(
            "body {body} requires a nonzero charge compatibility admission variable"
        )));
    }

    let graph = binding.receiver_graph;
    let root = binding.parent.map_or(graph, |parent| parent.graph);
    if public_root_count(definition, root) != 1
        || definition
            .server_graphs
            .iter()
            .any(|binding| binding.graph == Some(root) || binding.graph == Some(graph))
        || binding.parent.is_some() && public_root_count(definition, graph) != 0
    {
        return Err(invalid(format!(
            "body {body} must mount its charge compatibility graph {root} once as a public root and leave any nested receiver unmounted"
        )));
    }
    if let Some(parent) = binding.parent {
        // Native SubScript resolves the asset's low 16 bits as the child index.
        let receiver = u16::try_from(graph).map_err(|_| {
            invalid(format!(
                "body {body} declares charge compatibility child graph {graph} outside the native SubScript index range"
            ))
        })?;
        let receiver_asset = (GRAPH_TYPE << 48) | u64::from(receiver);
        let valid = native_server_state(graphs, parent).is_some_and(|node| {
            matches!(
                node.class.as_str(),
                "STUStatescriptStateSubScript" | "STU_37D754C8"
            ) && node.subscript_graph_asset() == Some(receiver_asset)
        });
        if !valid {
            return Err(invalid(format!(
                "body {body} requires a reciprocal server-running SubScript parent targeting charge compatibility graph {graph}"
            )));
        }
    }

    let receiver = graphs.get(&graph).ok_or_else(|| {
        invalid(format!(
            "body {body} declares missing charge compatibility graph {graph}"
        ))
    })?;
    let mut ordinary_receivers = receiver
        .entry_handlers(EntryTopic::Message(binding.damage_message))
        .filter(|node| !node.declares_room_message_entry());
    if !ordinary_receivers
        .next()
        .is_some_and(valid_charge_damage_receiver)
        || ordinary_receivers.next().is_some()
    {
        return Err(invalid(format!(
            "charge compatibility graph {graph} for body {body} requires exactly one ordinary damage receiver with bindings 13, 15, 3D and 624"
        )));
    }
    if !receiver.nodes.iter().any(|node| {
        node.runs_on_server()
            && references_charge_admission(&node.fields, binding.admission_variable)
    }) {
        return Err(invalid(format!(
            "charge compatibility graph {graph} for body {body} does not reference its admission variable"
        )));
    }
    if let Some(inhibit) = binding.inhibit {
        let valid = native_server_state(graphs, inhibit)
            .is_some_and(|node| node.class == "STUStatescriptStateAbility");
        if !valid {
            return Err(invalid(format!(
                "body {body} requires a reciprocal server-running charge inhibit ability"
            )));
        }
    }
    Ok(())
}
