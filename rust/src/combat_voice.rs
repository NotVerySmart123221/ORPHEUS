//! Data-driven combat announcer cues loaded from the runtime pack.

use std::{collections::BTreeMap, fs, io, path::Path};

use serde::Deserialize;

use crate::accounts::parse_guid;

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
    hurt_cooldown_ticks: u32,
    hurt: HurtVoiceTable,
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
        let hurt = match raw.hurt {
            Some(table) => HurtVoiceTable {
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
            },
            None => HurtVoiceTable::default(),
        };
        if !hurt.is_empty() && raw.hurt_cooldown_ticks == 0 {
            return Err(invalid(
                "hurt_cooldown_ticks must be positive when hurt cues are configured",
            ));
        }
        Ok(Self {
            multikill_window_ticks: raw.multikill_window_ticks,
            multikills,
            hurt_cooldown_ticks: raw.hurt_cooldown_ticks,
            hurt,
        })
    }

    /// Whether the catalog defines no announcer or hurt cues.
    pub fn is_empty(&self) -> bool {
        self.multikills.is_empty() && !self.has_hurt_cues()
    }

    /// Whether any actor hurt or death voice cues are configured.
    pub fn has_hurt_cues(&self) -> bool {
        !self.hurt.is_empty()
    }

    /// Minimum tick gap between non-lethal hurt voice cues for the same speaker.
    pub fn hurt_cooldown_ticks(&self) -> u32 {
        self.hurt_cooldown_ticks
    }

    /// Maximum tick gap between consecutive eliminations in the same streak.
    pub fn multikill_window_ticks(&self) -> u32 {
        self.multikill_window_ticks
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
        is_bot: bool,
        critical: bool,
        killed: bool,
        pick: usize,
    ) -> Option<(u64, u64)> {
        let entry = if killed {
            if is_bot {
                self.hurt.bot_death.as_ref().or(self.hurt.death.as_ref())
            } else {
                self.hurt.death.as_ref()
            }
        } else if critical {
            self.hurt.severe.as_ref().or(self.hurt.minor.as_ref())
        } else {
            self.hurt.minor.as_ref()
        }?;
        let line = *entry.lines.get(pick % entry.lines.len())?;
        Some((entry.stimulus, line))
    }
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
        assert_eq!(legacy.hurt_cue(true, false, false, 0), None);

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
            hurt_only.hurt_cue(true, false, false, 1),
            Some((0x0EE0_0000_0000_0003, 0x0760_0000_0000_0007))
        );
        // Severe falls back to minor; bot_death falls back to death.
        assert_eq!(
            hurt_only.hurt_cue(false, true, false, 0),
            Some((0x0EE0_0000_0000_0003, 0x0760_0000_0000_0006))
        );
        assert_eq!(
            hurt_only.hurt_cue(true, false, true, 0),
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
            full.hurt_cue(true, true, false, 0),
            Some((0x0EE0_0000_0000_0004, 0x0760_0000_0000_000B))
        );
        assert_eq!(
            full.hurt_cue(false, false, true, 0),
            Some((0x0EE0_0000_0000_0001, 0x0760_0000_0000_001C))
        );
        assert_eq!(
            full.hurt_cue(true, false, true, 0),
            Some((0x0EE0_0000_0000_0001, 0x0760_0000_0000_819E))
        );
    }
}
