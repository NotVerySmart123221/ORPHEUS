//! Serialize embedded state-group (subgraph box) entries from DataTool's
//! local `.01B` graph XML exports into the prepared graph bundle.
//!
//! The reference packer records per-graph `containers` as bare class tags,
//! which carries no routing: an authored `{"item": N}` reference reaching
//! such a box reports the missing launch loudly (the 104319 hook-graph
//! export genuinely has no group edges, so the tag is faithful there).
//! Where the export does carry them, the group's edges are restored without
//! guessing: each recorded container keeps its class (validated against the
//! export), and gains the begin edges the export shows leaving the group
//! (its own transition plugs). The enriched form is what
//! `orpheus_statescript::script_graph::Container` parses.

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read},
    path::Path,
};

use roxmltree::{Document, Node};
use serde_json::{Value, json};

use super::{RuntimeError, at};

const DRAGONML: &str = "https://yretenai.com/dragonml/v1";
const MAX_XML_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EDGES_PER_GRAPH: usize = 1_000_000;

pub(super) fn apply(input: &Path, xml_dir: &Path, graphs: &mut Value) -> Result<(), RuntimeError> {
    if !at(xml_dir, fs::metadata(xml_dir))?.is_dir() {
        return at(
            xml_dir,
            Err(invalid("expected a DataTool .01B XML directory")),
        );
    }
    let records = at(
        input,
        graphs
            .as_array_mut()
            .ok_or_else(|| invalid("expected an array of state graph records")),
    )?;
    for record in records {
        let index = at(
            input,
            record
                .get("index")
                .and_then(Value::as_u64)
                .ok_or_else(|| invalid("state graph record has no index")),
        )?;
        let Some(containers) = record.get_mut("containers") else {
            continue;
        };
        if containers.as_object().is_none_or(serde_json::Map::is_empty) {
            continue;
        }
        let path = xml_dir.join(format!("{index:012X}.01B.xml"));
        let Some(export) = at(&path, read_export(&path))? else {
            continue;
        };
        export.enrich(input, &path, containers)?;
    }
    Ok(())
}

/// One exported graph: the item table and every resolved plug edge.
struct GraphExport {
    /// Item-table ordinal to element class name.
    classes: BTreeMap<u16, String>,
    /// Edges as (source item, target item, target plug field name).
    edges: Vec<(u16, u16, String)>,
}

fn read_export(path: &Path) -> io::Result<Option<GraphExport>> {
    let input = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut xml = String::new();
    input.take(MAX_XML_BYTES + 1).read_to_string(&mut xml)?;
    if xml.len() as u64 > MAX_XML_BYTES {
        return Err(invalid("graph XML exceeds the size limit"));
    }
    let document = Document::parse(&xml).map_err(invalid)?;
    Ok(Some(GraphExport::collect(&document)?))
}

fn named<'a, 'input: 'a>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|child| child.attribute((DRAGONML, "name")) == Some(name))
}

fn item_id<'a, 'input: 'a>(node: Node<'a, 'input>) -> Option<&'a str> {
    node.attribute((DRAGONML, "id"))
}

fn invalid(reason: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

impl GraphExport {
    fn collect<'a, 'input: 'a>(document: &'a Document<'input>) -> io::Result<Self> {
        // A dragon:ref entry points at the element carrying that dragon:id;
        // the first definition wins, matching the reader's inline order.
        let mut by_id = BTreeMap::new();
        for node in std::iter::once(document.root()).chain(document.root().descendants()) {
            if let Some(id) = node.attribute((DRAGONML, "id")) {
                by_id.entry(id).or_insert(node);
            }
        }
        let resolve = |node: Node<'a, 'input>| {
            if node.tag_name().name() == "ref" {
                node.attribute((DRAGONML, "id"))
                    .and_then(|id| by_id.get(id).copied())
            } else {
                Some(node)
            }
        };
        let items = document
            .root()
            .descendants()
            .find(|node| {
                node.tag_name().name() == "array"
                    && node.attribute((DRAGONML, "name")) == Some("m_items")
            })
            .ok_or_else(|| invalid("graph XML has no item array"))?;
        let mut classes = BTreeMap::new();
        let mut ordinal_of = BTreeMap::new();
        // Inline node copies carry no dragon:id; the export ties them to
        // their item through the authored m_uniqueID (the same identity the
        // packer records as each node's `uid`).
        let mut ordinal_of_uid = BTreeMap::new();
        for (index, child) in items
            .children()
            .filter(roxmltree::Node::is_element)
            .enumerate()
        {
            let ordinal = u16::try_from(index)
                .map_err(|_| invalid("graph XML item table exceeds u16 ordinals"))?;
            let Some(target) = resolve(child) else {
                return Err(invalid("graph XML item reference does not resolve"));
            };
            classes.insert(ordinal, target.tag_name().name().to_owned());
            if let Some(id) = item_id(target) {
                ordinal_of.insert(id, ordinal);
            }
            if let Some(uid) = target.attribute("m_uniqueID") {
                if let Ok(uid) = uid.parse::<u64>() {
                    ordinal_of_uid.insert(uid, ordinal);
                }
            }
        }
        let item_ordinal = |node: Node<'a, 'input>| -> Option<u16> {
            item_id(node)
                .and_then(|id| ordinal_of.get(id).copied())
                .or_else(|| {
                    node.attribute("m_uniqueID")
                        .and_then(|uid| uid.parse::<u64>().ok())
                        .and_then(|uid| ordinal_of_uid.get(&uid).copied())
                })
        };
        // A link's own element lives under its output side; an item that is
        // serialized inline (a m_parentNode copy) is another item's subtree
        // and its edges belong to that item's walk, not this one.
        let mut edges = Vec::new();
        let mut collect_link = |link: Node<'a, 'input>, source: u16| -> io::Result<()> {
            if link
                .ancestors()
                .find_map(&item_ordinal)
                .is_some_and(|owner| owner != source)
            {
                return Ok(());
            }
            let Some(wrapper) = named(link, "m_inputPlug") else {
                return Err(invalid("graph XML plug link has no input wrapper"));
            };
            // An output plug serialized without an input side is an
            // unconnected authored edge; it carries no bundle link.
            let Some(parent) = named(wrapper, "m_parentNode") else {
                return Ok(());
            };
            // A target that is not an item of this export (a remote node)
            // carries no edge for the bundle.
            let Some(parent) = resolve(parent) else {
                return Ok(());
            };
            let Some(target) = item_ordinal(parent) else {
                return Ok(());
            };
            let field_name = parent.children().find_map(|child| {
                let name = child.attribute((DRAGONML, "name"))?;
                (child.attribute((DRAGONML, "id")) == wrapper.attribute((DRAGONML, "id"))
                    || resolve(child).is_some_and(|resolved| {
                        item_id(resolved) == item_id(wrapper) && name != "m_parentNode"
                    }))
                .then(|| name.to_owned())
            });
            // The link element may be serialized inside the target's inline
            // copy without a matching back-reference on the item element; a
            // link whose target field cannot be named is not restorable and
            // contributes no edge (the packer's own drop remains loud).
            let Some(field_name) = field_name else {
                return Ok(());
            };
            edges.push((source, target, field_name));
            Ok(())
        };
        for (source, element) in ordinal_of
            .iter()
            .filter_map(|(id, ordinal)| Some((*ordinal, *by_id.get(*id)?)))
        {
            for field in element
                .children()
                .filter(roxmltree::Node::is_element)
                .filter_map(|child| resolve(child).or(Some(child)))
            {
                for link in field
                    .descendants()
                    .filter(|node| node.tag_name().name() == "STUGraphLink")
                {
                    collect_link(link, source)?;
                }
            }
        }
        if edges.len() > MAX_EDGES_PER_GRAPH {
            return Err(invalid("graph XML has more plug edges than the limit"));
        }
        edges.sort_unstable();
        edges.dedup();
        Ok(Self { classes, edges })
    }

    fn enrich(
        &self,
        input: &Path,
        export: &Path,
        containers: &mut Value,
    ) -> Result<(), RuntimeError> {
        let entries = at(
            input,
            containers
                .as_object_mut()
                .ok_or_else(|| invalid("expected container records")),
        )?;
        for (key, recorded) in entries {
            let ordinal = at(
                input,
                key.parse::<u32>()
                    .map(u16::try_from)
                    .map_err(invalid)
                    .and_then(|ordinal| ordinal.map_err(|_| invalid("container ordinal range"))),
            )?;
            let recorded_class = match recorded {
                Value::String(class) => class.clone(),
                Value::Object(fields) => at(
                    input,
                    fields
                        .get("class")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .ok_or_else(|| invalid("container record has no class")),
                )?,
                _ => return at(input, Err(invalid("container record has no class"))),
            };
            let export_class = at(
                export,
                self.classes
                    .get(&ordinal)
                    .filter(|class| **class == recorded_class)
                    .ok_or_else(|| {
                        invalid(format!(
                            "container item {ordinal} is not the recorded class {recorded_class}"
                        ))
                    }),
            )?;
            // A routed container forwards along its own begin edges: links
            // whose output side is a field of the group (its transition
            // plugs), in the same [target, field] encoding as any plug.
            let links: Vec<Value> = self
                .edges
                .iter()
                .filter(|(source, _, _)| *source == ordinal)
                .map(|(_, target, field)| json!([*target, field.as_str()]))
                .collect();
            *recorded = json!({"class": export_class, "links": links});
        }
        Ok(())
    }
}
