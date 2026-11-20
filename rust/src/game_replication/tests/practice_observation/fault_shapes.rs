//! Static operand shapes for the opt-in prepared actor input report.

use crate::{
    script_expr::{Scope, lvalue},
    script_graph::GraphStore,
    script_init::Diagnostic,
};
use serde_json::{Map, Value as Json, json};

const MAX_DEPTH: usize = 5;
const MAX_MEMBERS: usize = 24;
const MAX_ARRAY_ITEMS: usize = 8;
const MAX_NODES: usize = 160;

pub(super) fn diagnostic_record(diagnostic: &Diagnostic, graphs: Option<&GraphStore>) -> Json {
    let site = diagnostic.site;
    let mut record = json!({
        "graph": site.graph,
        "node": site.node,
        "issue": format!("{:?}", diagnostic.issue),
    });
    if let Some(graphs) = graphs {
        record["registration_rejected"] = json!(diagnostic.registration_rejected);
        record["diagnostic_server_only"] = json!(diagnostic.server_only);
        record["node_shape"] = graph_node_shape(graphs, site.graph, site.node);
    }
    record
}

fn graph_node_shape(graphs: &GraphStore, graph: u32, node: Option<u16>) -> Json {
    let Some(node) = node.and_then(|pos| graphs.graph(graph)?.node(pos)) else {
        return Json::Null;
    };
    let mut remaining = MAX_NODES;
    json!({
        "class": class_label(&node.class),
        "is_action": node.is_action(),
        "is_state": node.is_state(),
        "client_only": node.client_only,
        "server_only": node.server_only,
        "has_remote_slot": node.remote >= 0,
        "has_state_slot": node.state.is_some(),
        "operands": operand_shape(&node.fields, 0, &mut remaining),
    })
}

fn schema_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 96
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn class_label(label: &str) -> Option<&str> {
    ((label.starts_with("STU") || label == "Plug") && schema_label(label)).then_some(label)
}

fn field_label(label: &str) -> bool {
    matches!(
        label,
        "item" | "links" | "origin" | "direction" | "max_distance" | "filter" | "target"
    ) || label.starts_with("m_") && schema_label(label)
}

fn operand_shape(value: &Json, depth: usize, remaining: &mut usize) -> Json {
    *remaining -= 1;
    match value {
        Json::Null => json!("null"),
        Json::Bool(_) => json!("boolean"),
        Json::Number(number) if number.is_i64() || number.is_u64() => json!("integer"),
        Json::Number(_) => json!("float"),
        Json::String(_) => json!("string"),
        Json::Array(items) => {
            let mut shapes = Vec::new();
            if depth < MAX_DEPTH {
                for item in items.iter().take(MAX_ARRAY_ITEMS) {
                    if *remaining == 0 {
                        break;
                    }
                    shapes.push(operand_shape(item, depth + 1, remaining));
                }
            }
            json!({"kind": "array", "count": items.len(), "items": shapes})
        }
        Json::Object(fields) => {
            let class = fields.get("$").and_then(Json::as_str).and_then(class_label);
            let mut shapes = Map::new();
            if depth < MAX_DEPTH {
                for (name, value) in fields
                    .iter()
                    .filter(|(name, _)| field_label(name))
                    .take(MAX_MEMBERS)
                {
                    if *remaining == 0 {
                        break;
                    }
                    shapes.insert(name.clone(), operand_shape(value, depth + 1, remaining));
                }
            }
            let omitted = fields.len() - shapes.len() - usize::from(fields.contains_key("$"));
            let mut shape =
                json!({"kind": "object", "class": class, "fields": shapes, "omitted": omitted});
            if let Ok(Some(variable)) = lvalue(value) {
                shape["variable_scope"] = json!(match variable.scope {
                    Scope::Instance => "instance",
                    Scope::Entity => "entity",
                });
            }
            shape
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script_init::{Issue, Site};

    #[test]
    fn actor_fault_shapes_are_opt_in_and_only_expose_schema() {
        let contract: Json = serde_json::from_str(include_str!(
            "../../../../../fixtures/behavior/actor-input-node-shape-v1.json"
        ))
        .unwrap();
        assert_eq!(contract["schema_version"], 1);
        assert_eq!(contract["case_id"], "actor-input-node-shape-v1");
        let input = &contract["input"];
        let diagnostic = Diagnostic {
            site: Site {
                entity: 7,
                instance: 1,
                graph: input["graph"].as_u64().unwrap() as u32,
                node: Some(input["node"].as_u64().unwrap() as u16),
            },
            issue: Issue::UnsupportedAction(input["class"].as_str().unwrap().into()),
            registration_rejected: true,
            server_only: false,
        };
        let data = json!({
            "version": 1, "bodies": {}, "missing": [],
            "graphs": [{
                "index": input["graph"], "items": 1, "states": [], "entries": [],
                "remote": [], "sync_vars": [], "containers": {}, "unknown_nodes": [], "fields": {},
                "header": {"m_nodesBitCount": 4, "m_statesBitCount": 4,
                    "m_remoteSyncNodesBitCount": 4, "m_syncVarsBitCount": 4},
                "nodes": [{
                    "class": input["class"], "pos": input["node"], "uid": 0, "rid": 0,
                    "remote": -1, "state": null, "client_only": false, "server_only": false,
                    "fields": input["fields"],
                }]
            }]
        });
        let graphs = GraphStore::from_json(&serde_json::to_vec(&data).unwrap()).unwrap();
        let default = diagnostic_record(&diagnostic, None);
        assert_eq!(default.as_object().unwrap().len(), 3);
        assert_eq!(default["graph"], input["graph"]);
        assert_eq!(default["node"], input["node"]);
        assert!(default.get("node_shape").is_none());

        let record = diagnostic_record(&diagnostic, Some(&graphs));
        assert_eq!(record["issue"], default["issue"]);
        assert_eq!(record["registration_rejected"], true);
        let shape = &record["node_shape"];
        assert_eq!(shape["class"], input["class"]);
        assert_eq!(shape["is_action"], true);
        assert_eq!(shape["has_remote_slot"], false);
        let fields = &shape["operands"]["fields"];
        assert_eq!(fields["m_instigator"], "null");
        assert_eq!(fields["m_target"]["class"], "STUConfigVarDynamic");
        assert_eq!(fields["m_target"]["fields"]["m_identifier"], "integer");
        assert_eq!(fields["m_target"]["variable_scope"], "instance");
        assert_eq!(fields["m_out_Value"]["variable_scope"], "entity");
        assert_eq!(fields["m_filters"]["count"], 2);
        assert_eq!(fields["m_filters"]["items"][1]["class"], Json::Null);
        assert_eq!(fields["m_amount"], "float");
        let serialized = record.to_string();
        for value in contract["expected"]["suppressed_values"]
            .as_array()
            .unwrap()
        {
            let value = value.as_str().unwrap();
            assert!(!serialized.contains(value), "unexpected disclosure {value}");
        }
        assert_eq!(
            graph_node_shape(&graphs, diagnostic.site.graph, Some(5)),
            Json::Null
        );
    }

    #[test]
    fn actor_fault_shape_bounds_nested_and_wide_operands() {
        let array = Json::Array(vec![json!("hidden"); 1_000]);
        let mut remaining = MAX_NODES;
        let shape = operand_shape(&array, 0, &mut remaining);
        assert_eq!(shape["count"], 1_000);
        assert_eq!(shape["items"].as_array().unwrap().len(), MAX_ARRAY_ITEMS);

        let value = json!({"m_child": array});
        let shape = operand_shape(&value, MAX_DEPTH, &mut remaining);
        assert_eq!(shape["fields"], json!({}));
        assert_eq!(shape["omitted"], 1);

        let mut remaining = 2;
        let shape = operand_shape(&value, 0, &mut remaining);
        assert_eq!(remaining, 0);
        assert_eq!(shape["fields"]["m_child"]["items"], json!([]));

        let wide = Json::Object(
            (0..50)
                .map(|index| (format!("m_operand_{index}"), Json::Null))
                .collect(),
        );
        let mut remaining = MAX_NODES;
        let shape = operand_shape(&wide, 0, &mut remaining);
        assert_eq!(shape["fields"].as_object().unwrap().len(), MAX_MEMBERS);
        assert_eq!(shape["omitted"], 50 - MAX_MEMBERS);
    }
}
