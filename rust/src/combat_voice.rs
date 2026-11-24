//! Data-driven combat announcer cues loaded from the runtime pack.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::Path,
};

use serde::Deserialize;

use crate::{
    accounts::parse_guid,
    script_expr::target_tag_config,
    script_graph::{Graph, GraphStore, Node},
    script_init::TargetTagSnapshot,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct VoiceCueEntry {
    stimulus: u64,
    lines: Vec<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct HurtVoiceTable {
    minor: Option<VoiceCueEntry>,
    severe: Option<VoiceCueEntry>,
    death: Option<VoiceCueEntry>,
    bot_death: Option<VoiceCueEntry>,
}

impl HurtVoiceTable {
    fn is_empty(&self) -> bool {
        self.minor.is_none()
            && self.severe.is_none()
            && self.death.is_none()
            && self.bot_death.is_none()
    }
}

/// Optional multikill announcer and actor hurt/death voice tables loaded from the runtime pack.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CombatVoiceCatalog {
    multikill_window_ticks: u32,
    multikills: BTreeMap<u32, VoiceCueEntry>,
    multikill_exclusions: BTreeSet<u64>,
    hurt_cooldown_ticks: u32,
    hurt: HurtVoiceTable,
    hurt_by_body: BTreeMap<u64, HurtVoiceTable>,
}

impl CombatVoiceCatalog {
    /// Load an optional combat voice catalog, returning an empty catalog when absent.
    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => Self::from_json(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    /// Parse and validate a combat voice catalog from JSON.
    pub fn from_json(text: &str) -> io::Result<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCatalog {
            #[serde(default)]
            multikill_window_ticks: u32,
            #[serde(default)]
            multikills: BTreeMap<u32, RawCue>,
            #[serde(default)]
            hurt_cooldown_ticks: u32,
            #[serde(default)]
            hurt: Option<RawHurtTable>,
            #[serde(default)]
            hurt_by_body: BTreeMap<String, RawHurtTable>,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawHurtTable {
            #[serde(default)]
            minor: Option<RawCue>,
            #[serde(default)]
            severe: Option<RawCue>,
            #[serde(default)]
            death: Option<RawCue>,
            #[serde(default)]
            bot_death: Option<RawCue>,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCue {
            stimulus: String,
            lines: Vec<String>,
        }

        fn parse_cue(raw: RawCue, context: &str) -> io::Result<VoiceCueEntry> {
            let stimulus = parse_nonzero_guid(&raw.stimulus, "stimulus")?;
            if raw.lines.is_empty() {
                return Err(invalid(format!(
                    "{context} requires at least one line GUID"
                )));
            }
            let lines = raw
                .lines
                .iter()
                .map(|line| parse_nonzero_guid(line, "line"))
                .collect::<io::Result<Vec<_>>>()?;
            Ok(VoiceCueEntry { stimulus, lines })
        }

        let raw: RawCatalog = serde_json::from_str(text).map_err(invalid)?;
        if !raw.multikills.is_empty() && raw.multikill_window_ticks == 0 {
            return Err(invalid(
                "multikill_window_ticks must be positive when multikills are configured",
            ));
        }
        let mut multikills = BTreeMap::new();
        for (count, cue) in raw.multikills {
            multikills.insert(count, parse_cue(cue, &format!("multikill {count}"))?);
        }
        let parse_hurt = |table: RawHurtTable| -> io::Result<HurtVoiceTable> {
            Ok(HurtVoiceTable {
                minor: table
                    .minor
                    .map(|cue| parse_cue(cue, "hurt.minor"))
                    .transpose()?,
                severe: table
                    .severe
                    .map(|cue| parse_cue(cue, "hurt.severe"))
                    .transpose()?,
                death: table
                    .death
                    .map(|cue| parse_cue(cue, "hurt.death"))
                    .transpose()?,
                bot_death: table
                    .bot_death
                    .map(|cue| parse_cue(cue, "hurt.bot_death"))
                    .transpose()?,
            })
        };
        let hurt = raw.hurt.map(parse_hurt).transpose()?.unwrap_or_default();
        let mut hurt_by_body = BTreeMap::new();
        for (key, table) in raw.hurt_by_body {
            let body = parse_nonzero_guid(&key, "hurt body")?;
            if body >> 48 != 0x0400 {
                return Err(invalid("hurt_by_body requires a body definition GUID"));
            }
            let table = parse_hurt(table)?;
            if table.is_empty() || hurt_by_body.insert(body, table).is_some() {
                return Err(invalid("hurt_by_body entry is empty or duplicates a body"));
            }
        }
        if (!hurt.is_empty() || !hurt_by_body.is_empty()) && raw.hurt_cooldown_ticks == 0 {
            return Err(invalid(
                "hurt_cooldown_ticks must be positive when hurt cues are configured",
            ));
        }
        Ok(Self {
            multikill_window_ticks: raw.multikill_window_ticks,
            multikills,
            multikill_exclusions: BTreeSet::new(),
            hurt_cooldown_ticks: raw.hurt_cooldown_ticks,
            hurt,
            hurt_by_body,
        })
    }

    /// Whether the catalog defines no announcer or hurt cues.
    pub fn is_empty(&self) -> bool {
        self.multikills.is_empty() && !self.has_hurt_cues()
    }

    /// Whether any actor hurt or death voice cues are configured.
    pub fn has_hurt_cues(&self) -> bool {
        !self.hurt.is_empty() || !self.hurt_by_body.is_empty()
    }

    /// Minimum tick gap between non-lethal hurt voice cues for the same speaker.
    pub fn hurt_cooldown_ticks(&self) -> u32 {
        self.hurt_cooldown_ticks
    }

    /// Maximum tick gap between consecutive eliminations in the same streak.
    pub fn multikill_window_ticks(&self) -> u32 {
        self.multikill_window_ticks
    }

    pub(crate) fn bind_multikill_exclusions(&mut self, graphs: &GraphStore) {
        if self.multikills.is_empty() {
            return;
        }
        let cues = &self.multikills;
        let mut resolved = graphs
            .graph_indexes()
            .filter_map(|index| graphs.graph(index))
            .flat_map(|graph| {
                graph
                    .nodes
                    .iter()
                    .filter_map(move |node| multikill_exclusions(graph, node, cues))
            });
        let Some((tags, window_ticks)) = resolved.next() else {
            return;
        };
        if resolved
            .all(|(other_tags, other_window)| other_tags == tags && other_window == window_ticks)
        {
            self.multikill_exclusions = tags;
            if let Some(ticks) = window_ticks.filter(|&ticks| ticks > 0) {
                self.multikill_window_ticks = ticks;
            }
        }
    }

    pub(crate) fn has_multikill_exclusions(&self) -> bool {
        !self.multikill_exclusions.is_empty()
    }

    pub(crate) fn multikill_target_allowed(&self, tags: &TargetTagSnapshot) -> bool {
        self.multikill_exclusions
            .iter()
            .all(|&tag| tags.presence(tag) == Some(false))
    }

    /// Resolve `(stimulus, line)` for a streak count, if configured.
    pub fn multikill_cue(&self, count: u32, pick: usize) -> Option<(u64, u64)> {
        let entry = self.multikills.get(&count)?;
        let line = *entry.lines.get(pick % entry.lines.len())?;
        Some((entry.stimulus, line))
    }

    /// Resolve `(stimulus, line)` for an actor hurt or death cue, if configured.
    pub fn hurt_cue(
        &self,
        body: Option<u64>,
        is_bot: bool,
        critical: bool,
        killed: bool,
        pick: usize,
    ) -> Option<(u64, u64)> {
        let table = body
            .and_then(|body| self.hurt_by_body.get(&body))
            .unwrap_or(&self.hurt);
        let entry = if killed {
            if is_bot {
                table.bot_death.as_ref().or(table.death.as_ref())
            } else {
                table.death.as_ref()
            }
        } else if critical {
            table.severe.as_ref().or(table.minor.as_ref())
        } else {
            table.minor.as_ref()
        }?;
        let line = *entry.lines.get(pick % entry.lines.len())?;
        Some((entry.stimulus, line))
    }
}

fn multikill_exclusions(
    graph: &Graph,
    node: &Node,
    cues: &BTreeMap<u32, VoiceCueEntry>,
) -> Option<(BTreeSet<u64>, Option<u32>)> {
    let condition = node.field("m_condition")?;
    if node.class != "STU_3387AB5D"
        || condition["$"] != "STU_AC829876"
        || condition["m_filter"]["$"] != "STUConfigVarFilter"
    {
        return None;
    }
    let tags = node.field("m_condition.m_filter.m_8B2A4DF3")?;
    if tags.is_null() {
        return None;
    }
    let tags = if tags["$"] == "STU_06610EBC" {
        tags["m_configVars"].as_array()?.as_slice()
    } else {
        std::slice::from_ref(tags)
    };
    let mut tags = tags
        .iter()
        .map(target_tag_config)
        .collect::<Option<BTreeSet<_>>>()?;
    tags.remove(&0);
    let mut frontier: Vec<_> = node
        .links("m_truePlug")
        .filter_map(|(pos, _)| pos)
        .collect();
    let mut visited = BTreeSet::new();
    let mut matched_switch = false;
    let mut window_ticks = None;
    while let Some(pos) = frontier.pop() {
        let Some(branch) = graph.node(pos).filter(|_| visited.insert(pos)) else {
            continue;
        };
        let paths: &[&str] = match branch.class.as_str() {
            "STUStatescriptActionSetVar" => &["m_outPlug"],
            "STU_3387AB5D" => &["m_truePlug", "m_falsePlug"],
            "STUStatescriptActionSwitch" if matches_cues(graph, branch, cues) => {
                matched_switch = true;
                continue;
            }
            "STUStatescriptStateWait"
                if branch.field("m_0DBAFD7F").and_then(|v| v.as_u64()) == Some(1) =>
            {
                if window_ticks.is_none() {
                    window_ticks = resolve_wait_ticks(graph, branch);
                }
                continue;
            }
            _ => continue,
        };
        frontier.extend(
            paths
                .iter()
                .flat_map(|path| branch.links(path))
                .filter_map(|(pos, _)| pos),
        );
    }
    matched_switch.then_some((tags, window_ticks))
}

fn resolve_wait_ticks(graph: &Graph, wait: &Node) -> Option<u32> {
    let timeout = wait.field("m_timeout")?;
    let seconds = match timeout["$"].as_str()? {
        "STUConfigVarInt" | "STUConfigVarFloat" => timeout["m_value"].as_f64(),
        "STUConfigVarDynamic" => {
            let var_id = timeout["m_identifier"].as_u64()?;
            let mut found = graph.nodes.iter().filter_map(|n| {
                if n.class == "STUStatescriptActionSetVar"
                    && n.field("m_out_Var.m_identifier").and_then(|v| v.as_u64()) == Some(var_id)
                {
                    let val = n.field("m_value")?;
                    if matches!(
                        val["$"].as_str(),
                        Some("STUConfigVarInt" | "STUConfigVarFloat")
                    ) {
                        return val["m_value"].as_f64();
                    }
                }
                None
            });
            let first = found.next()?;
            found.all(|other| other == first).then_some(first)
        }
        _ => None,
    }?;
    (seconds.is_finite() && seconds > 0.0).then_some(((seconds * 1000.0) / 16.0).round() as u32)
}

fn matches_cues(graph: &Graph, node: &Node, cues: &BTreeMap<u32, VoiceCueEntry>) -> bool {
    let Some(cases) = node.field("m_FDD34C25").and_then(|value| value.as_array()) else {
        return false;
    };
    cues.iter().all(|(&count, cue)| {
        cases.iter().any(|case| {
            case["m_value"]["m_value"].as_u64() == Some(u64::from(count))
                && case["links"].as_array().into_iter().flatten().any(|link| {
                    link[0]
                        .as_u64()
                        .and_then(|pos| u16::try_from(pos).ok())
                        .and_then(|pos| graph.node(pos))
                        .filter(|action| action.class == "STUStatescriptActionSetVar")
                        .and_then(|action| action.field("m_value.m_voiceStimulus"))
                        .and_then(|value| value.as_str())
                        .and_then(parse_guid)
                        == Some(cue.stimulus)
                })
        })
    })
}

fn parse_nonzero_guid(text: &str, field: &str) -> io::Result<u64> {
    parse_guid(text)
        .filter(|&guid| guid != 0)
        .ok_or_else(|| invalid(format!("invalid {field} GUID {text:?}")))
}

fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multikill_cues_and_selects_lines() {
        let catalog = CombatVoiceCatalog::from_json(
            r#"{
                "multikill_window_ticks": 250,
                "multikills": {
                    "2": { "stimulus": "0x0EE0000000000102", "lines": ["0x0760000000000202"] },
                    "3": { "stimulus": "0x0EE0000000000103", "lines": ["0x0760000000000203", "0x0760000000000204"] }
                }
            }"#,
        )
        .unwrap();
        assert!(!catalog.is_empty());
        assert_eq!(catalog.multikill_window_ticks(), 250);
        assert_eq!(catalog.multikill_cue(1, 0), None);
        assert_eq!(
            catalog.multikill_cue(2, 0),
            Some((0x0EE0_0000_0000_0102, 0x0760_0000_0000_0202))
        );
        assert_eq!(
            catalog.multikill_cue(3, 1),
            Some((0x0EE0_0000_0000_0103, 0x0760_0000_0000_0204))
        );
        assert_eq!(
            catalog.multikill_cue(3, 2),
            Some((0x0EE0_0000_0000_0103, 0x0760_0000_0000_0203))
        );
    }

    #[test]
    fn missing_file_loads_empty_default() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = CombatVoiceCatalog::load(dir.path().join("missing.json")).unwrap();
        assert!(catalog.is_empty());
        assert_eq!(catalog.multikill_cue(2, 0), None);
    }

    #[test]
    fn rejects_invalid_zero_empty_or_malformed_entries() {
        for invalid_json in [
            r#"{"multikill_window_ticks": 0, "multikills": {"2": {"stimulus": "0x1", "lines": ["0x2"]}}}"#,
            r#"{"multikill_window_ticks": 250, "multikills": {"2": {"stimulus": "0x0", "lines": ["0x2"]}}}"#,
            r#"{"multikill_window_ticks": 250, "multikills": {"2": {"stimulus": "bad", "lines": ["0x2"]}}}"#,
            r#"{"multikill_window_ticks": 250, "multikills": {"2": {"stimulus": "0x1", "lines": []}}}"#,
            r#"{"multikill_window_ticks": 250, "multikills": {"2": {"stimulus": "0x1", "lines": ["0x0"]}}}"#,
            r#"{"hurt_cooldown_ticks": 0, "hurt": {"minor": {"stimulus": "0x1", "lines": ["0x2"]}}}"#,
            r#"{"hurt_cooldown_ticks": 12, "hurt": {"minor": {"stimulus": "0x1", "lines": []}}}"#,
            r#"{"hurt_cooldown_ticks": 12, "hurt": {"death": {"stimulus": "0x0", "lines": ["0x2"]}}}"#,
        ] {
            let error = CombatVoiceCatalog::from_json(invalid_json).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        }
    }

    #[test]
    fn parses_hurt_cues_with_fallbacks_and_backward_compatibility() {
        let legacy = CombatVoiceCatalog::from_json(
            r#"{
                "multikill_window_ticks": 250,
                "multikills": {
                    "2": { "stimulus": "0x0EE0000000000102", "lines": ["0x0760000000000202"] }
                }
            }"#,
        )
        .unwrap();
        assert!(!legacy.is_empty());
        assert!(!legacy.has_hurt_cues());
        assert_eq!(legacy.hurt_cue(None, true, false, false, 0), None);

        let hurt_only = CombatVoiceCatalog::from_json(
            r#"{
                "hurt_cooldown_ticks": 18,
                "hurt": {
                    "minor": { "stimulus": "0x0EE0000000000003", "lines": ["0x0760000000000006", "0x0760000000000007"] },
                    "death": { "stimulus": "0x0EE0000000000001", "lines": ["0x076000000000001C"] }
                }
            }"#,
        )
        .unwrap();
        assert!(!hurt_only.is_empty());
        assert!(hurt_only.has_hurt_cues());
        assert_eq!(hurt_only.hurt_cooldown_ticks(), 18);
        assert_eq!(hurt_only.multikill_cue(2, 0), None);
        assert_eq!(
            hurt_only.hurt_cue(None, true, false, false, 1),
            Some((0x0EE0_0000_0000_0003, 0x0760_0000_0000_0007))
        );
        // Severe falls back to minor; bot_death falls back to death.
        assert_eq!(
            hurt_only.hurt_cue(None, false, true, false, 0),
            Some((0x0EE0_0000_0000_0003, 0x0760_0000_0000_0006))
        );
        assert_eq!(
            hurt_only.hurt_cue(None, true, false, true, 0),
            Some((0x0EE0_0000_0000_0001, 0x0760_0000_0000_001C))
        );

        let full = CombatVoiceCatalog::from_json(
            r#"{
                "hurt_cooldown_ticks": 12,
                "hurt": {
                    "minor": { "stimulus": "0x0EE0000000000003", "lines": ["0x0760000000000006"] },
                    "severe": { "stimulus": "0x0EE0000000000004", "lines": ["0x076000000000000B"] },
                    "death": { "stimulus": "0x0EE0000000000001", "lines": ["0x076000000000001C"] },
                    "bot_death": { "stimulus": "0x0EE0000000000001", "lines": ["0x076000000000819E"] }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(
            full.hurt_cue(None, true, true, false, 0),
            Some((0x0EE0_0000_0000_0004, 0x0760_0000_0000_000B))
        );
        assert_eq!(
            full.hurt_cue(None, false, false, true, 0),
            Some((0x0EE0_0000_0000_0001, 0x0760_0000_0000_001C))
        );
        assert_eq!(
            full.hurt_cue(None, true, false, true, 0),
            Some((0x0EE0_0000_0000_0001, 0x0760_0000_0000_819E))
        );
    }

    #[test]
    fn bind_multikill_exclusions_extracts_authored_restartable_wait_timeout() {
        let mut voice = CombatVoiceCatalog::from_json(
            r#"{
                "multikill_window_ticks": 999,
                "multikills": {
                    "2": { "stimulus": "0x0EE000000000058A", "lines": ["0x0760000000007F74"] }
                }
            }"#,
        )
        .unwrap();
        let make_node = |pos: u16, class: &str, fields: serde_json::Value| {
            serde_json::json!({
                "pos": pos, "class": class, "uid": pos, "rid": pos,
                "remote": -1, "state": null, "client_only": false, "server_only": false,
                "fields": fields,
            })
        };
        let bundle = serde_json::json!({
            "version": 1,
            "bodies": {},
            "missing": [],
            "graphs": [{
                "index": 533,
                "items": 5,
                "states": [],
                "entries": [],
                "remote": [],
                "sync_vars": [],
                "containers": {},
                "unknown_nodes": [],
                "fields": {},
                "header": {
                    "m_nodesBitCount": 4, "m_statesBitCount": 4,
                    "m_remoteSyncNodesBitCount": 4, "m_syncVarsBitCount": 4
                },
                "nodes": [
                    make_node(0, "STU_3387AB5D", serde_json::json!({
                        "m_condition": {
                            "$": "STU_AC829876",
                            "m_filter": {
                                "$": "STUConfigVarFilter",
                                "m_8B2A4DF3": { "$": "STU_357DABBB", "m_7D7C86A1": "0x0D600000000006AD" }
                            }
                        },
                        "m_truePlug": { "$": "STUStatescriptOutputPlug", "links": [[1, "m_inPlug"], [2, "m_inPlug"]] }
                    })),
                    make_node(1, "STUStatescriptActionSetVar", serde_json::json!({
                        "m_out_Var": { "$": "STUConfigVarDynamic", "m_identifier": 2036 },
                        "m_value": { "$": "STUConfigVarInt", "m_value": 2 },
                        "m_outPlug": { "$": "STUStatescriptOutputPlug", "links": [[3, "m_beginPlug"]] }
                    })),
                    make_node(2, "STUStatescriptActionSwitch", serde_json::json!({
                        "m_FDD34C25": [{
                            "m_value": { "m_value": 2 },
                            "links": [[4, "m_inPlug"]]
                        }]
                    })),
                    make_node(3, "STUStatescriptStateWait", serde_json::json!({
                        "m_0DBAFD7F": 1,
                        "m_timeout": { "$": "STUConfigVarDynamic", "m_identifier": 2036 }
                    })),
                    make_node(4, "STUStatescriptActionSetVar", serde_json::json!({
                        "m_value": { "m_voiceStimulus": "0x0EE000000000058A" }
                    }))
                ]
            }]
        });
        let graphs = GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap();
        voice.bind_multikill_exclusions(&graphs);
        assert_eq!(voice.multikill_window_ticks(), 125);
        assert!(voice.has_multikill_exclusions());
    }
}
