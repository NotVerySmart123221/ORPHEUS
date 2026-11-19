//! State graph metadata for retail 1.74 and its owner/remote wire tables.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{self, BufReader, Read},
    path::Path,
};

use flate2::bufread::MultiGzDecoder;
use orpheus_runtime::format::{MAX_GRAPH_BYTES, TARGET_TAG_TYPE, parse_guid};
use serde::Deserialize;
use serde_json::{Map, Value};

const GRAPH_TYPE: u64 = 0x0580;

#[derive(Debug, Deserialize)]
pub struct Header {
    #[serde(rename = "m_nodesBitCount")]
    pub nodes_bits: u8,
    #[serde(rename = "m_statesBitCount")]
    pub states_bits: u8,
    #[serde(rename = "m_remoteSyncNodesBitCount")]
    pub remote_bits: u8,
    #[serde(rename = "m_syncVarsBitCount")]
    pub sync_vars_bits: u8,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum StateBehavior {
    Plain,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ActionBehavior {
    Forward,
}

#[derive(Debug, Deserialize)]
pub struct Node {
    pub pos: u16,
    pub class: String,
    pub uid: u32,
    pub rid: u16,
    pub remote: i32,
    pub state: Option<u16>,
    state_behavior: Option<StateBehavior>,
    action_behavior: Option<ActionBehavior>,
    #[serde(deserialize_with = "node_flag")]
    pub client_only: bool,
    #[serde(deserialize_with = "node_flag")]
    pub server_only: bool,
    pub fields: Value,
}

impl Node {
    /// Whether the node's declared execution scope includes the server.
    pub(crate) fn runs_on_server(&self) -> bool {
        !self.client_only
    }

    pub fn is_networked(&self) -> bool {
        !self.client_only && !self.server_only
    }

    pub fn is_state(&self) -> bool {
        self.declares_plain_state() || self.fields.get("m_2BBEEAB8").is_some_and(truthy)
    }

    fn declares_plain_state(&self) -> bool {
        matches!(self.state_behavior, Some(StateBehavior::Plain))
    }

    pub fn is_action(&self) -> bool {
        self.declares_forward_action() || self.fields.get("m_ADEB6E05").is_some_and(truthy)
    }

    pub(crate) fn declares_forward_action(&self) -> bool {
        matches!(self.action_behavior, Some(ActionBehavior::Forward))
    }

    pub fn field(&self, path: &str) -> Option<&Value> {
        field_at(&self.fields, path)
    }

    /// The plug's targets in data order, including unresolved targets.
    pub fn links(&self, path: &str) -> impl Iterator<Item = (Option<u16>, &str)> {
        self.field(path)
            .into_iter()
            .flat_map(raw_plug_links)
            .filter_map(decode_link)
    }

    pub fn plugs(&self) -> BTreeMap<String, &Value> {
        let mut found = BTreeMap::new();
        collect_plugs(&self.fields, "", &mut found);
        found
    }
}

fn raw_plug_links(field: &Value) -> impl Iterator<Item = &Value> {
    field
        .get("$")
        .and_then(|_| field.get("links")?.as_array())
        .into_iter()
        .flatten()
}

fn decode_link(link: &Value) -> Option<(Option<u16>, &str)> {
    let pair = link.as_array()?;
    let position = match pair.first()? {
        Value::Null => None,
        value => Some(u16::try_from(value.as_u64()?).ok()?),
    };
    Some((position, pair.get(1)?.as_str()?))
}

/// A decoded link to a position absent from both the node and container tables.
#[derive(Debug, Eq, PartialEq)]
pub struct DanglingLink {
    pub graph: u32,
    pub source: u16,
    pub plug: String,
    /// Zero-based position in the plug's original link list.
    pub link_index: usize,
    pub target: u16,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub struct SyncVar(pub Option<u32>, pub u8, pub u8);

impl SyncVar {
    pub fn has_presence_bit(self) -> bool {
        self.0.is_some() && self.1 == 0
    }
}

#[derive(Debug, Default)]
struct WireSlots {
    by_list_index: Vec<Option<usize>>,
    positions: Vec<u16>,
}

impl WireSlots {
    fn build(order: &[Option<u16>], graph: &Graph, require_state: bool) -> Self {
        let mut positions = Vec::new();
        let mut first_slot = vec![None; graph.by_pos.len()];
        let by_list_index = order
            .iter()
            .map(|&position| {
                let node = graph.node(position?)?;
                if !node.is_networked() || (require_state && !node.is_state()) {
                    return None;
                }
                let slot = *first_slot[usize::from(node.pos)].get_or_insert(positions.len());
                positions.push(node.pos);
                Some(slot)
            })
            .collect();
        Self {
            by_list_index,
            positions,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Graph {
    pub index: u32,
    pub header: Header,
    pub fields: Value,
    pub items: u16,
    pub containers: BTreeMap<u16, String>,
    pub nodes: Vec<Node>,
    #[serde(rename = "unknown_nodes")]
    pub unknown_rids: Vec<u16>,
    pub states: Vec<Option<u16>>,
    pub entries: Vec<Option<u16>>,
    #[serde(rename = "remote")]
    pub remote_nodes: Vec<Option<u16>>,
    pub sync_vars: Vec<SyncVar>,
    #[serde(skip)]
    by_pos: Vec<Option<usize>>,
    #[serde(skip)]
    by_rid: Vec<Option<usize>>,
    #[serde(skip)]
    owner: WireSlots,
    #[serde(skip)]
    remote: WireSlots,
    #[serde(skip)]
    presence: Vec<usize>,
}

impl Graph {
    fn compile(&mut self) -> io::Result<()> {
        self.by_pos = vec![None; usize::from(self.items)];
        if let Some(max) = self.nodes.iter().map(|node| node.rid).max() {
            self.by_rid = vec![None; usize::from(max) + 1];
        }
        let authored_state_positions =
            self.nodes
                .iter()
                .any(Node::declares_forward_action)
                .then(|| {
                    self.states
                        .iter()
                        .copied()
                        .flatten()
                        .collect::<BTreeSet<_>>()
                });
        for (index, node) in self.nodes.iter_mut().enumerate() {
            if node.declares_plain_state() {
                let occupies_state_slot = node
                    .state
                    .and_then(|slot| self.states.get(usize::from(slot)))
                    .is_some_and(|&position| position == Some(node.pos));
                if !node.server_only
                    || !node.runs_on_server()
                    || node.is_action()
                    || !occupies_state_slot
                {
                    return Err(invalid(format!(
                        "plain state in graph {} at node {} must be server-only and occupy its state slot",
                        self.index, node.pos
                    )));
                }
                // Ordinary authored states use the one plain lifecycle, including
                // at class checks outside the main dispatch.
                node.class = "STUStatescriptState".into();
            }
            if node.declares_forward_action() {
                let state_slot = authored_state_positions
                    .as_ref()
                    .is_some_and(|positions| positions.contains(&node.pos));
                let has_valid_output = node.field("m_outPlug").is_some_and(|plug| {
                    plug.get("$").and_then(Value::as_str).is_some()
                        && plug
                            .get("links")
                            .and_then(Value::as_array)
                            .is_some_and(|links| {
                                links.iter().all(|link| decode_link(link).is_some())
                            })
                });
                if !node.server_only
                    || !node.runs_on_server()
                    || node.is_state()
                    || node.state.is_some()
                    || state_slot
                    || !has_valid_output
                {
                    return Err(invalid(format!(
                        "forward action in graph {} at node {} must be server-only with an output plug and no state slot",
                        self.index, node.pos
                    )));
                }
                // The declaration supplies dispatch. An authored name that
                // matches an entry or another special class cannot activate it.
                node.class = "ActionForward".into();
            }
            let position = self
                .by_pos
                .get_mut(usize::from(node.pos))
                .ok_or_else(|| invalid("graph node outside the item table"))?;
            if position.replace(index).is_some() {
                return Err(invalid(format!(
                    "graph {} defines node position {} more than once",
                    self.index, node.pos
                )));
            }
            self.by_rid[usize::from(node.rid)] = Some(index);
        }
        self.owner = WireSlots::build(&self.states, self, false);
        self.remote = WireSlots::build(&self.remote_nodes, self, true);
        self.presence = self
            .sync_vars
            .iter()
            .enumerate()
            .filter_map(|(index, var)| var.has_presence_bit().then_some(index))
            .collect();
        Ok(())
    }

    pub fn node(&self, pos: u16) -> Option<&Node> {
        self.nodes.get((*self.by_pos.get(usize::from(pos))?)?)
    }

    pub fn by_rid(&self, rid: u16) -> Option<&Node> {
        self.nodes.get((*self.by_rid.get(usize::from(rid))?)?)
    }

    pub fn state(&self, index: usize) -> Option<&Node> {
        self.node((*self.states.get(index)?)?)
    }

    pub fn remote_node(&self, index: usize) -> Option<&Node> {
        self.node((*self.remote_nodes.get(index)?)?)
    }

    pub fn node_ref(&self, value: &Value) -> Option<&Node> {
        self.node(u16::try_from(value.get("node")?.as_u64()?).ok()?)
    }

    pub fn owner_positions(&self) -> &[u16] {
        &self.owner.positions
    }

    pub fn remote_positions(&self) -> &[u16] {
        &self.remote.positions
    }

    pub fn owner_slot(&self, index: usize) -> Option<usize> {
        *self.owner.by_list_index.get(index)?
    }

    pub fn remote_slot(&self, index: usize) -> Option<usize> {
        *self.remote.by_list_index.get(index)?
    }

    pub fn presence_vars(&self) -> impl Iterator<Item = (usize, SyncVar)> + '_ {
        self.presence
            .iter()
            .map(|&index| (index, self.sync_vars[index]))
    }

    pub fn presence_bit(&self, var: u32) -> Option<usize> {
        self.presence_vars()
            .position(|(_, entry)| entry.0 == Some(var))
    }

    fn dangling_links(&self) -> impl Iterator<Item = DanglingLink> + '_ {
        self.nodes.iter().flat_map(move |node| {
            node.plugs().into_iter().flat_map(move |(path, plug)| {
                raw_plug_links(plug)
                    .enumerate()
                    .filter_map(move |(link_index, link)| {
                        let (target, _) = decode_link(link)?;
                        let target = target?;
                        if self.node(target).is_some() || self.containers.contains_key(&target) {
                            return None;
                        }
                        Some(DanglingLink {
                            graph: self.index,
                            source: node.pos,
                            plug: path.clone(),
                            link_index,
                            target,
                        })
                    })
            })
        })
    }

    pub fn graph_refs(&self) -> BTreeSet<u64> {
        let mut found = BTreeSet::new();
        collect_graphs(&self.fields, &mut found);
        for node in &self.nodes {
            collect_graphs(&node.fields, &mut found);
        }
        found
    }
}

#[derive(Debug, Deserialize)]
pub struct GraphBinding {
    pub graph: Option<u32>,
    #[serde(rename = "m_1EB5A024")]
    pub sync: Option<Vec<Value>>,
    #[serde(rename = "m_75408640")]
    pub flags: i64,
}

#[derive(Debug, Deserialize)]
pub struct Body {
    pub hero: Option<String>,
    pub name: String,
    pub graphs: Vec<GraphBinding>,
    pub manager: Option<u32>,
    pub weapons: Vec<Option<u32>>,
    #[serde(default)]
    pub ai_weapon_buttons: Map<String, Value>,
    #[serde(default)]
    pub ai_association: Option<u64>,
    #[serde(default)]
    pub ai_weapon_identifiers: Map<String, Value>,
    /// None means the local entity definition was unavailable.
    #[serde(default)]
    pub component92_present: Option<bool>,
    /// An omitted list means the source definition was unavailable.
    #[serde(default, deserialize_with = "body_target_tags")]
    pub target_tags: Option<BTreeSet<u64>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModeValue {
    Bool(bool),
    Integer(i64),
    Asset(u64),
}

#[derive(Debug, Deserialize)]
pub struct ModeScripts {
    pub pve: bool,
    pub scripted_loading: bool,
    #[serde(default, deserialize_with = "optional_graph_index")]
    pub graph: Option<u32>,
    #[serde(default, deserialize_with = "optional_graph_index")]
    pub shared_graph: Option<u32>,
    pub teams: BTreeMap<u8, TeamScripts>,
}

#[derive(Debug, Deserialize)]
pub struct TeamScripts {
    #[serde(default, deserialize_with = "optional_graph_index")]
    pub controller: Option<u32>,
    #[serde(default, deserialize_with = "optional_graph_index")]
    pub body: Option<u32>,
}

fn optional_graph_index<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u32>, D::Error> {
    Option::<Value>::deserialize(deserializer)?
        .map(|value| {
            asset_guid(&value)
                .filter(|guid| guid >> 48 == GRAPH_TYPE)
                .and_then(|guid| u32::try_from(guid & 0x0000_ffff_ffff_ffff).ok())
                .ok_or_else(|| serde::de::Error::custom("invalid mode script graph GUID"))
        })
        .transpose()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnemyPair {
    pub body: u64,
    pub controller: u64,
}

#[derive(Debug, Deserialize)]
struct RawEnemyPair {
    body: String,
    controller: String,
}

#[derive(Debug, Default, Deserialize)]
struct EnemyCatalog {
    #[serde(default)]
    pairs: BTreeMap<String, RawEnemyPair>,
    #[serde(default)]
    entities: BTreeMap<String, Body>,
}

impl EnemyCatalog {
    fn validate(mut self) -> io::Result<Self> {
        let asset = |text: &str, kind| {
            parse_guid(text).filter(|guid| guid >> 48 == kind && guid & 0x0000_ffff_ffff_ffff != 0)
        };
        for (key, body) in std::mem::take(&mut self.entities) {
            let guid = asset(&key, 0x0400)
                .ok_or_else(|| invalid("invalid Junkenstein entity definition"))?;
            if self
                .entities
                .insert(format!("0x{guid:016X}"), body)
                .is_some()
            {
                return Err(invalid("duplicate Junkenstein entity definition"));
            }
        }
        for (hero, mut pair) in std::mem::take(&mut self.pairs) {
            if asset(&hero, 0x02e0).is_none()
                || asset(&pair.body, 0x0400).is_none()
                || asset(&pair.controller, 0x0400).is_none()
                || parse_guid(&pair.body) == parse_guid(&pair.controller)
            {
                return Err(invalid("invalid Junkenstein body/controller pair"));
            }
            pair.body = format!("0x{:016X}", parse_guid(&pair.body).unwrap());
            pair.controller = format!("0x{:016X}", parse_guid(&pair.controller).unwrap());
            let body = self
                .entities
                .get(&pair.body)
                .ok_or_else(|| invalid("missing Junkenstein enemy body"))?;
            let controller = self
                .entities
                .get(&pair.controller)
                .ok_or_else(|| invalid("missing Junkenstein enemy controller"))?;
            if body.hero.as_deref().and_then(parse_guid) != parse_guid(&hero)
                || body
                    .ai_association
                    .is_some_and(|association| Some(association) != parse_guid(&pair.controller))
                || body.graphs.is_empty()
                || controller.graphs.is_empty()
            {
                return Err(invalid("inconsistent Junkenstein body/controller binding"));
            }
            if self
                .pairs
                .insert(format!("0x{:016X}", parse_guid(&hero).unwrap()), pair)
                .is_some()
            {
                return Err(invalid("duplicate Junkenstein hero pair"));
            }
        }
        Ok(self)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthoredClass {
    class: String,
    behavior: AuthoredClassBehavior,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AuthoredClassBehavior {
    PlainState,
    ForwardAction,
    ClientOnly,
}

impl AuthoredClassBehavior {
    fn apply(self, graph: u32, node: &mut Node) -> io::Result<()> {
        let (client_only, state, action) = match self {
            Self::PlainState => (false, Some(StateBehavior::Plain), None),
            Self::ForwardAction => (false, None, Some(ActionBehavior::Forward)),
            Self::ClientOnly => (true, None, None),
        };
        if (node.client_only && !client_only)
            || (node.server_only && client_only)
            || (node.state_behavior.is_some() && node.state_behavior != state)
            || (node.action_behavior.is_some() && node.action_behavior != action)
        {
            return Err(invalid(format!(
                "authored class {} conflicts with scope or behavior in graph {graph} at node {}",
                node.class, node.pos
            )));
        }
        node.client_only = client_only;
        node.server_only = !client_only;
        node.state_behavior = state;
        node.action_behavior = action;
        Ok(())
    }
}

#[derive(Deserialize)]
struct Bundle {
    version: u32,
    #[serde(default)]
    authored_classes: Vec<AuthoredClass>,
    graphs: Vec<Graph>,
    bodies: BTreeMap<String, Body>,
    #[serde(default)]
    entities: BTreeMap<String, Body>,
    #[serde(default)]
    mode_variables: BTreeMap<String, BTreeMap<u32, Value>>,
    #[serde(default)]
    mode_scripts: BTreeMap<String, ModeScripts>,
    #[serde(default)]
    junkenstein: EnemyCatalog,
    missing: Vec<u32>,
}

#[derive(Debug)]
pub struct GraphStore {
    graphs: BTreeMap<u32, Graph>,
    bodies: BTreeMap<u64, Body>,
    entities: BTreeMap<u64, Body>,
    mode_variables: BTreeMap<u64, BTreeMap<u32, ModeValue>>,
    mode_scripts: BTreeMap<u64, ModeScripts>,
    enemies: EnemyCatalog,
    missing: Vec<u32>,
}

impl GraphStore {
    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        let input = BufReader::new(File::open(path)?);
        Self::from_gzip(input)
    }

    pub fn from_gzip(input: impl io::BufRead) -> io::Result<Self> {
        let mut data = Vec::new();
        MultiGzDecoder::new(input)
            .take(MAX_GRAPH_BYTES + 1)
            .read_to_end(&mut data)?;
        if data.len() as u64 > MAX_GRAPH_BYTES {
            return Err(invalid("state graph bundle exceeds the size limit"));
        }
        Self::from_json(&data)
    }

    pub fn from_json(data: &[u8]) -> io::Result<Self> {
        let bundle: Bundle = serde_json::from_slice(data).map_err(invalid)?;
        if bundle.version != 1 {
            return Err(invalid("unsupported state graph bundle version"));
        }
        let mut authored_classes = BTreeMap::new();
        for AuthoredClass { class, behavior } in bundle.authored_classes {
            let valid_name = class.strip_prefix("authored.").is_some_and(|name| {
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            });
            if !valid_name {
                return Err(invalid(format!(
                    "class {class:?} must use an authored. name with letters, digits, underscores or hyphens"
                )));
            }
            if authored_classes.insert(class.clone(), behavior).is_some() {
                return Err(invalid(format!("duplicate authored class {class}")));
            }
        }
        let mut graphs = BTreeMap::new();
        for mut graph in bundle.graphs {
            if !authored_classes.is_empty() {
                for node in &mut graph.nodes {
                    if let Some(behavior) = authored_classes.get(&node.class) {
                        behavior.apply(graph.index, node)?;
                    }
                }
            }
            graph.compile()?;
            let index = graph.index;
            if graphs.insert(index, graph).is_some() {
                return Err(invalid(format!(
                    "bundle defines graph {index} more than once"
                )));
            }
        }
        Ok(Self {
            graphs,
            bodies: keyed_by_guid(bundle.bodies)?,
            entities: keyed_by_guid(bundle.entities)?,
            mode_variables: keyed_by_guid(bundle.mode_variables)?
                .into_iter()
                .map(|(guid, values)| {
                    let values = values
                        .into_iter()
                        .map(|(var, value)| Ok((var, mode_value(&value)?)))
                        .collect::<io::Result<_>>()?;
                    Ok((guid, values))
                })
                .collect::<io::Result<_>>()?,
            mode_scripts: keyed_by_guid(bundle.mode_scripts)?,
            enemies: bundle.junkenstein.validate()?,
            missing: bundle.missing,
        })
    }

    pub fn graph(&self, index: u32) -> Option<&Graph> {
        self.graphs.get(&index)
    }

    /// Inspect local graph structure on demand. Unresolved null links and links
    /// to declared containers are valid and do not appear in this report.
    pub fn dangling_links(&self) -> impl Iterator<Item = DanglingLink> + '_ {
        self.graphs.values().flat_map(Graph::dangling_links)
    }

    pub fn graph_indexes(&self) -> impl Iterator<Item = u32> + '_ {
        self.graphs.keys().copied()
    }

    pub fn bodies(&self) -> &BTreeMap<u64, Body> {
        &self.bodies
    }

    pub fn entity_body(&self, guid: u64) -> Option<&Body> {
        self.entities.get(&guid)
    }

    pub fn enemy_pair(&self, hero: u64) -> Option<EnemyPair> {
        self.enemies
            .pairs
            .get(&format!("0x{hero:016X}"))
            .map(|pair| EnemyPair {
                body: parse_guid(&pair.body).expect("validated enemy body"),
                controller: parse_guid(&pair.controller).expect("validated enemy controller"),
            })
    }

    pub fn enemy_body(&self, definition: u64) -> Option<&Body> {
        self.enemies.entities.get(&format!("0x{definition:016X}"))
    }

    pub fn mode_variables(&self, mode: u64) -> Option<&BTreeMap<u32, ModeValue>> {
        self.mode_variables.get(&mode)
    }

    pub fn mode_scripts(&self, mode: u64) -> Option<&ModeScripts> {
        self.mode_scripts.get(&mode)
    }

    pub fn missing(&self) -> &[u32] {
        &self.missing
    }
}

pub fn asset_guid(value: &Value) -> Option<u64> {
    let text = value.as_str()?;
    let hex = text.strip_prefix("0x")?;
    if hex.len() == 16
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'A'..=b'F'))
    {
        parse_guid(text)
    } else {
        None
    }
}

pub fn target_tag_guid(value: &Value) -> Option<u64> {
    asset_guid(value).filter(|guid| (guid >> 48) & 0xfff == TARGET_TAG_TYPE)
}

fn body_target_tags<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<Option<BTreeSet<u64>>, D::Error> {
    Option::<Vec<Value>>::deserialize(decoder)?
        .map(|tags| {
            tags.iter()
                .map(|tag| {
                    target_tag_guid(tag)
                        .or_else(|| {
                            tag.as_u64()
                                .filter(|guid| (guid >> 48) & 0xfff == TARGET_TAG_TYPE)
                        })
                        .ok_or_else(|| {
                            serde::de::Error::custom("body target tag must be a packed .06C GUID")
                        })
                })
                .collect()
        })
        .transpose()
}

pub fn field_at<'a>(fields: &'a Value, path: &str) -> Option<&'a Value> {
    let mut value = fields;
    for part in path.split('.') {
        let (name, mut indexes) = part.split_at(part.find('[').unwrap_or(part.len()));
        if name.is_empty() || !name.chars().all(|ch| ch == '_' || ch.is_alphanumeric()) {
            return None;
        }
        value = value.get(name)?;
        while !indexes.is_empty() {
            let (index, tail) = indexes.strip_prefix('[')?.split_once(']')?;
            if index.is_empty() || !index.bytes().all(|ch| ch.is_ascii_digit()) {
                return None;
            }
            value = value.get(index.parse::<usize>().ok()?)?;
            indexes = tail;
        }
    }
    Some(value)
}

fn collect_plugs<'a>(value: &'a Value, path: &str, out: &mut BTreeMap<String, &'a Value>) {
    match value {
        Value::Object(fields) if fields.contains_key("$") && fields.contains_key("links") => {
            out.insert(path.to_owned(), value);
        }
        Value::Object(fields) => {
            for (key, item) in fields {
                if key != "$" {
                    let name = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    collect_plugs(item, &name, out);
                }
            }
        }
        Value::Array(values) => {
            for (index, item) in values.iter().enumerate() {
                collect_plugs(item, &format!("{path}[{index}]"), out);
            }
        }
        _ => {}
    }
}

fn collect_graphs(value: &Value, out: &mut BTreeSet<u64>) {
    match value {
        Value::Object(fields) => {
            for field in fields.values() {
                collect_graphs(field, out);
            }
        }
        Value::Array(values) => {
            for item in values {
                collect_graphs(item, out);
            }
        }
        _ => {
            if let Some(guid) = asset_guid(value).filter(|guid| guid >> 48 == GRAPH_TYPE) {
                out.insert(guid & 0x0000_ffff_ffff_ffff);
            }
        }
    }
}

fn keyed_by_guid<T>(records: BTreeMap<String, T>) -> io::Result<BTreeMap<u64, T>> {
    records
        .into_iter()
        .map(|(key, value)| {
            let hex = key.strip_prefix("0x").unwrap_or(&key);
            let guid = u64::from_str_radix(hex, 16).map_err(invalid)?;
            Ok((guid, value))
        })
        .collect()
}

fn mode_value(value: &Value) -> io::Result<ModeValue> {
    if let Some(value) = value.as_bool() {
        return Ok(ModeValue::Bool(value));
    }
    if let Some(value) = value.as_i64() {
        return Ok(ModeValue::Integer(value));
    }
    asset_guid(value)
        .map(ModeValue::Asset)
        .ok_or_else(|| invalid("unsupported mode variable"))
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64() != Some(0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

fn node_flag<'de, D: serde::Deserializer<'de>>(decoder: D) -> Result<bool, D::Error> {
    Value::deserialize(decoder).map(|value| truthy(&value))
}

fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        let node = |pos, rid, client_only, fields: Value| {
            json!({
                "pos": pos, "uid": 0, "rid": rid, "remote": 0, "state": null,
                "class": "Node", "client_only": client_only, "server_only": 0, "fields": fields
            })
        };
        let body = json!({
            "hero": null, "name": "Fixture", "manager": null, "weapons": [null, 42],
            "graphs": [{"graph": 42, "m_1EB5A024": null, "m_75408640": 0}]
        });
        json!({
            "version": 1,
            "graphs": [{
                "index": 42,
                "header": {"m_nodesBitCount": 4, "m_statesBitCount": 3,
                    "m_remoteSyncNodesBitCount": 3, "m_syncVarsBitCount": 3},
                "fields": {"ref": "0x058000000000001A", "ignored": "0x058000000000001b"},
                "items": 5, "containers": {"3": "Opaque"}, "unknown_nodes": [8],
                "nodes": [
                    node(0, 3, false, json!({
                        "m_2BBEEAB8": true,
                        "entries": [{"trigger": {"$": "Plug", "links": [[2, "input"], [null, "unknown"]]} }],
                        "related": ["0x058000000000002B", "0x040000000000002A"]
                    })),
                    node(1, 5, true, json!({"m_2BBEEAB8": true})),
                    node(2, 6, false, json!({"m_ADEB6E05": true}))
                ],
                "states": [0, 1, null, 0],
                "entries": [0, 3, 2, null],
                "remote": [2, null, 1, 0],
                "sync_vars": [[null, 0, 0], [12, 1, 0], [3, 0, 1], [42, 0, 0]]
            }],
            "bodies": {"0x0400000000000001": body.clone()},
            "entities": {"0x0400000000000002": body},
            "mode_variables": {"0x0230000000000018": {
                "1": false, "2": 3, "3": "0x0D8000000000195F"
            }},
            "missing": [43]
        })
    }

    #[test]
    fn body_target_tags_distinguish_unknown_empty_and_invalid_data() {
        let mut data = fixture();
        let body = "0x0400000000000001";
        let entity = "0x0400000000000002";
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());

        assert!(
            load(&data).unwrap().bodies()[&0x0400_0000_0000_0001]
                .target_tags
                .is_none()
        );
        data["bodies"][body]["target_tags"] = json!([]);
        data["entities"][entity]["target_tags"] = json!([
            "0x0D60000000000159",
            "0x0D60000000000109",
            "0x0D60000000000159"
        ]);
        let parsed = load(&data).unwrap();
        assert_eq!(
            parsed.bodies()[&0x0400_0000_0000_0001].target_tags,
            Some(BTreeSet::new())
        );
        assert_eq!(
            parsed
                .entity_body(0x0400_0000_0000_0002)
                .unwrap()
                .target_tags,
            Some(BTreeSet::from([
                0x0d60_0000_0000_0109,
                0x0d60_0000_0000_0159
            ]))
        );
        data["bodies"][body]["target_tags"] = json!(["0x0400000000000109"]);
        assert_eq!(load(&data).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn enemy_pairs_keep_encounter_definitions_separate_and_validate_associations() {
        let hero = "0x02E0000000000175";
        let body = "0x040000000000000a";
        let controller = "0x040000000000000b";
        let mut data = fixture();
        let mut actor = data["bodies"]["0x0400000000000001"].clone();
        actor["hero"] = json!(hero);
        actor["ai_association"] = json!(0x0400_0000_0000_000bu64);
        actor["target_tags"] = json!([0x0d60_0000_0000_0109u64, "0x0D60000000000109"]);
        let control = data["entities"]["0x0400000000000002"].clone();
        data["junkenstein"] = json!({
            "pairs":{hero:{"body":body,"controller":controller}},
            "entities":{body:actor,controller:control}
        });
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let parsed = load(&data).unwrap();
        assert_eq!(
            parsed.enemy_pair(0x02e0_0000_0000_0175),
            Some(EnemyPair {
                body: 0x0400_0000_0000_000a,
                controller: 0x0400_0000_0000_000b,
            })
        );
        assert!(parsed.entity_body(0x0400_0000_0000_000a).is_none());
        assert_eq!(
            parsed
                .enemy_body(0x0400_0000_0000_000a)
                .unwrap()
                .target_tags,
            Some(BTreeSet::from([0x0d60_0000_0000_0109]))
        );
        for (field, value) in [
            ("ai_association", json!(0x0400_0000_0000_000cu64)),
            ("hero", json!("0x02E0000000000176")),
            ("target_tags", json!([0x0400_0000_0000_0109u64])),
        ] {
            let mut bad = data.clone();
            bad["junkenstein"]["entities"][body][field] = value;
            assert!(load(&bad).is_err());
        }
        let mut missing = data.clone();
        missing["junkenstein"]["entities"]
            .as_object_mut()
            .unwrap()
            .remove(controller);
        assert!(load(&missing).is_err());
        let mut duplicated = data.clone();
        duplicated["junkenstein"]["entities"]["0x040000000000000A"] =
            duplicated["junkenstein"]["entities"][body].clone();
        assert!(load(&duplicated).is_err());
        // Playable boss definitions use the explicit encounter pair instead of an AI association.
        data["junkenstein"]["entities"][body]["ai_association"] = Value::Null;
        assert!(load(&data).is_ok());
    }

    #[test]
    fn duplicate_graph_identities_are_rejected() {
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());

        let mut duplicate_position = fixture();
        duplicate_position["graphs"][0]["nodes"][2]["pos"] = json!(0);
        let error = load(&duplicate_position).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("node position 0"));

        let mut duplicate_graph = fixture();
        let duplicate = duplicate_graph["graphs"][0].clone();
        duplicate_graph["graphs"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        let error = load(&duplicate_graph).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("graph 42"));
    }

    #[test]
    fn authored_class_catalog_rejects_ambiguous_declarations() {
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let mut data = fixture();
        assert_eq!(
            load(&data).unwrap().graph(42).unwrap().owner_positions(),
            [0, 0]
        );

        for (declarations, message) in [
            (
                json!([{"class":"authored.latch","behavior":"plain_state"},
                       {"class":"authored.latch","behavior":"client_only"}]),
                "duplicate authored class",
            ),
            (
                json!([{"class":"STUStatescriptState","behavior":"client_only"}]),
                "must use an authored.",
            ),
            (
                json!([{"class":"authored.","behavior":"client_only"}]),
                "must use an authored.",
            ),
            (
                json!([{"class":"authored.latch","behavior":"unknown"}]),
                "unknown variant",
            ),
            (
                json!([{"class":"authored.latch","behavior":"plain_state","typo":true}]),
                "unknown field",
            ),
        ] {
            data["authored_classes"] = declarations;
            let error = load(&data).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert!(error.to_string().contains(message), "{error}");
        }
    }

    #[test]
    fn authored_class_catalog_reuses_node_validation() {
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let mut data = fixture();
        data["authored_classes"] = json!([
            {"class":"authored.latch","behavior":"plain_state"},
            {"class":"authored.overlay","behavior":"client_only"},
            {"class":"authored.forward","behavior":"forward_action"}
        ]);
        let nodes = &mut data["graphs"][0]["nodes"];
        nodes[0]["class"] = json!("authored.latch");
        nodes[0]["state"] = json!(0);
        nodes[0]["state_behavior"] = json!("plain");
        nodes[0]["server_only"] = json!(true);
        nodes[1]["class"] = json!("authored.overlay");
        nodes[2]["class"] = json!("authored.forward");
        nodes[2]["fields"]["m_outPlug"] = json!({"$":"Plug","links":[]});

        let store = load(&data).unwrap();
        let graph = store.graph(42).unwrap();
        assert!(graph.owner_positions().is_empty());
        assert!(graph.remote_positions().is_empty());
        assert!(graph.node(1).unwrap().client_only);

        for (position, field, value, message) in [
            (0, "client_only", json!(true), "graph 42 at node 0"),
            (0, "action_behavior", json!("forward"), "graph 42 at node 0"),
            (0, "state", json!(1), "occupy its state slot"),
            (1, "server_only", json!(true), "graph 42 at node 1"),
            (1, "state_behavior", json!("plain"), "graph 42 at node 1"),
            (2, "fields", json!({}), "output plug"),
        ] {
            let mut malformed = data.clone();
            malformed["graphs"][0]["nodes"][position][field] = value;
            let error = load(&malformed).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert!(error.to_string().contains(message), "{error}");
        }

        data["graphs"][0]["states"][2] = json!(2);
        let error = load(&data).unwrap_err();
        assert!(error.to_string().contains("no state slot"), "{error}");
    }

    #[test]
    fn forward_action_declarations_require_a_server_action_and_valid_output() {
        let mut data = fixture();
        let node = &mut data["graphs"][0]["nodes"][2];
        node["action_behavior"] = json!("forward");
        node["server_only"] = json!(true);
        node["fields"] = json!({"m_outPlug": {"$": "Plug", "links": [[null, "input"]]}});
        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let store = load(&data).unwrap();
        let forward = store.graph(42).unwrap().node(2).unwrap();
        assert!(forward.is_action());
        assert!(!forward.is_state());
        assert!(!forward.is_networked());

        for (field, value) in [
            ("server_only", json!(false)),
            ("client_only", json!(true)),
            ("state", json!(0)),
            ("state_behavior", json!("plain")),
            ("action_behavior", json!("unknown")),
            ("fields", json!({"m_outPlug": {"links": []}})),
            (
                "fields",
                json!({"m_outPlug": {"$": "Plug", "links": [[3, false]]}}),
            ),
        ] {
            let mut malformed = data.clone();
            malformed["graphs"][0]["nodes"][2][field] = value;
            assert_eq!(
                load(&malformed).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
        data["graphs"][0]["states"][2] = json!(2);
        assert_eq!(load(&data).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn plain_state_declarations_require_a_server_state_slot() {
        let mut data = fixture();
        let node = &mut data["graphs"][0]["nodes"][0];
        node["state"] = json!(0);
        node["server_only"] = json!(true);
        node["state_behavior"] = json!("plain");

        let load = |data: &Value| GraphStore::from_json(&serde_json::to_vec(data).unwrap());
        let store = load(&data).unwrap();
        let graph = store.graph(42).unwrap();
        let plain = graph.state(0).unwrap();
        assert!(plain.is_state());
        assert!(!plain.is_networked());
        assert!(graph.owner_positions().is_empty());

        for (field, value) in [
            ("server_only", json!(false)),
            ("client_only", json!(true)),
            ("state", json!(1)),
            ("state", Value::Null),
            ("state_behavior", json!("unrecognized")),
        ] {
            let mut malformed = data.clone();
            malformed["graphs"][0]["nodes"][0][field] = value;
            assert_eq!(
                load(&malformed).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
        data["graphs"][0]["nodes"][0]["fields"]["m_ADEB6E05"] = json!(true);
        assert_eq!(load(&data).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn dangling_link_report_preserves_unresolved_links_and_data_order() {
        let mut data = fixture();
        let links = &mut data["graphs"][0]["nodes"][0]["fields"]["entries"][0]["trigger"]["links"];
        *links = json!([
            [2, "input"],
            [null, "unresolved"],
            [3, "container"],
            [4, "empty slot"],
            [9, "outside table"],
        ]);
        let store = GraphStore::from_json(&serde_json::to_vec(&data).unwrap()).unwrap();
        let plug = "entries[0].trigger";
        assert_eq!(
            store.dangling_links().collect::<Vec<_>>(),
            [(3, 4), (4, 9)].map(|(link_index, target)| DanglingLink {
                graph: 42,
                source: 0,
                plug: plug.into(),
                link_index,
                target,
            })
        );
        let graph = store.graph(42).unwrap();
        assert_eq!(
            graph.node(0).unwrap().links(plug).collect::<Vec<_>>(),
            [
                (Some(2), "input"),
                (None, "unresolved"),
                (Some(3), "container"),
                (Some(4), "empty slot"),
                (Some(9), "outside table"),
            ]
        );
        assert_eq!(graph.owner_positions(), [0, 0]);
    }

    #[test]
    fn builds_wire_slots_and_keeps_data_order_through_holes() {
        let store = GraphStore::from_json(&serde_json::to_vec(&fixture()).unwrap()).unwrap();
        let graph = store.graph(42).unwrap();
        assert_eq!(graph.owner_positions(), [0, 0]);
        assert_eq!(graph.remote_positions(), [0]);
        assert_eq!(graph.owner_slot(0), Some(0));
        assert_eq!(graph.owner_slot(3), Some(0));
        assert_eq!(graph.owner_slot(1), None);
        assert_eq!(graph.remote_slot(0), None);
        assert_eq!(graph.remote_slot(3), Some(0));
        assert_eq!(graph.presence_bit(42), Some(1));
        assert_eq!(graph.presence_bit(12), None);
        assert_eq!(graph.graph_refs(), BTreeSet::from([26, 43]));
        assert_eq!(graph.by_rid(6).unwrap().pos, 2);
        assert_eq!(graph.node_ref(&json!({"node": 2})).unwrap().rid, 6);
        assert!(graph.node(3).is_none());

        let node = graph.node(0).unwrap();
        let plug_path = "entries[0].trigger";
        assert_eq!(node.plugs().keys().next().unwrap(), plug_path);
        assert_eq!(
            node.links(plug_path).collect::<Vec<_>>(),
            [(Some(2), "input"), (None, "unknown")]
        );
        for invalid in [
            "entries[-1].trigger",
            "entries[0]x",
            ".entries[0]",
            "entries[1]",
        ] {
            assert_eq!(node.field(invalid), None);
        }
        assert_eq!(store.missing(), [43]);
        assert_eq!(
            store.bodies()[&0x0400_0000_0000_0001].graphs[0].graph,
            Some(42)
        );
        assert_eq!(
            store.entity_body(0x0400_0000_0000_0002).unwrap().name,
            "Fixture"
        );
        assert_eq!(
            store.mode_variables(0x0230_0000_0000_0018).unwrap(),
            &BTreeMap::from([
                (1, ModeValue::Bool(false)),
                (2, ModeValue::Integer(3)),
                (3, ModeValue::Asset(0x0d80_0000_0000_195f)),
            ])
        );
    }

    #[test]
    fn reads_gzip_and_rejects_truncated_or_inconsistent_data() {
        use flate2::{Compression, write::GzEncoder};
        use std::io::Write;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder
            .write_all(&serde_json::to_vec(&fixture()).unwrap())
            .unwrap();
        let bytes = encoder.finish().unwrap();
        assert!(
            GraphStore::from_gzip(bytes.as_slice())
                .unwrap()
                .graph(42)
                .is_some()
        );
        assert!(GraphStore::from_gzip(&bytes[..bytes.len() - 4]).is_err());
        let mut bad = fixture();
        bad["graphs"][0]["nodes"][0]["pos"] = json!(6);
        assert!(GraphStore::from_json(&serde_json::to_vec(&bad).unwrap()).is_err());
        bad["version"] = json!(2);
        assert!(GraphStore::from_json(&serde_json::to_vec(&bad).unwrap()).is_err());
    }

    #[test]
    #[ignore = "requires a prepared 104319 state graph bundle"]
    fn whole_retail_wire_index_matches_the_python_reference() {
        use sha2::{Digest, Sha256};
        let root = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
        let store = GraphStore::load(root.join("data/game_graphs_174.json.gz")).unwrap();
        let mut digest = Sha256::new();
        fn sequence(digest: &mut Sha256, items: impl IntoIterator<Item = u64>) {
            let items: Vec<_> = items.into_iter().collect();
            digest.update((items.len() as u64).to_le_bytes());
            for item in items {
                digest.update(item.to_le_bytes());
            }
        }
        for index in store.graph_indexes() {
            let graph = store.graph(index).unwrap();
            sequence(
                &mut digest,
                [
                    u64::from(index),
                    u64::from(graph.items),
                    u64::from(graph.header.nodes_bits),
                    u64::from(graph.header.states_bits),
                    u64::from(graph.header.remote_bits),
                    u64::from(graph.header.sync_vars_bits),
                    graph.nodes.len() as u64,
                ],
            );
            sequence(
                &mut digest,
                graph.owner_positions().iter().copied().map(u64::from),
            );
            sequence(
                &mut digest,
                graph.remote_positions().iter().copied().map(u64::from),
            );
            sequence(
                &mut digest,
                graph.presence_vars().flat_map(|(index, var)| {
                    [
                        index as u64,
                        u64::from(var.0.unwrap()),
                        u64::from(var.1),
                        u64::from(var.2),
                    ]
                }),
            );
            sequence(&mut digest, graph.graph_refs());
            sequence(
                &mut digest,
                graph.unknown_rids.iter().copied().map(u64::from),
            );
        }
        assert_eq!(store.bodies().len(), 32);
        let expected = match store.graphs.len() {
            373 => "f54fb85e91d33b669c73239ddc77fb0f8f7fa6e6d0f04b83532bc1f953a4ac7c",
            405 => {
                assert_eq!(store.entities.len(), 8);
                assert_eq!(store.mode_variables.len(), 25);
                "eab09343964a53d44a53baa8d990ff9c2a3eea8922d9eb1927136fb7b661fc94"
            }
            699 => {
                assert_eq!(store.entities.len(), 6);
                assert_eq!(store.mode_variables.len(), 25);
                "585ab3b69f4eb0966dd755171b33b227f8eac7de595f6f5ec7392939c92229df"
            }
            700 => {
                assert_eq!(store.entities.len(), 6);
                assert_eq!(store.mode_variables.len(), 25);
                assert!(store.graph(0x0b92).is_some());
                assert_eq!(store.enemies.pairs.len(), 8);
                assert_eq!(store.enemies.entities.len(), 19);
                assert_eq!(
                    store.enemy_pair(0x02e0_0000_0000_0175),
                    Some(EnemyPair {
                        body: 0x0400_0000_0000_16be,
                        controller: 0x0400_0000_0000_16c0,
                    })
                );
                for (definition, expected) in [
                    (0x0400_0000_0000_16be, vec![0x0be2, 0x0c89, 0x0cb8, 0x0cbf]),
                    (0x0400_0000_0000_16c0, vec![0x0c87, 0x0cf3]),
                ] {
                    let roots =
                        crate::script_init::body_roots(store.enemy_body(definition).unwrap())
                            .unwrap();
                    assert_eq!(
                        roots.iter().map(|root| root.graph).collect::<Vec<_>>(),
                        expected
                    );
                    assert!(roots.iter().all(|root| store.graph(root.graph).is_some()));
                }
                "8834709ac62f67287938b1eb747a0039131abc73f6fd6568b008d8689c2d9c22"
            }
            count => panic!("no Python reference vector for the {count}-graph bundle"),
        };
        assert_eq!(format!("{:x}", digest.finalize()), expected);
    }
}
