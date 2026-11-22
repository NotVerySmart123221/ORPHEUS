//! Data-driven combat announcer cues loaded from the runtime pack.

use std::{collections::BTreeMap, fs, io, path::Path};

use serde::Deserialize;

use crate::accounts::parse_guid;

#[derive(Clone, Debug, PartialEq, Eq)]
struct VoiceCueEntry {
    stimulus: u64,
    lines: Vec<u64>,
}

/// Optional multikill announcer table keyed by elimination streak length.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CombatVoiceCatalog {
    multikill_window_ticks: u32,
    multikills: BTreeMap<u32, VoiceCueEntry>,
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
            multikill_window_ticks: u32,
            multikills: BTreeMap<u32, RawCue>,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCue {
            stimulus: String,
            lines: Vec<String>,
        }

        let raw: RawCatalog = serde_json::from_str(text).map_err(invalid)?;
        if !raw.multikills.is_empty() && raw.multikill_window_ticks == 0 {
            return Err(invalid(
                "multikill_window_ticks must be positive when multikills are configured",
            ));
        }
        let mut multikills = BTreeMap::new();
        for (count, cue) in raw.multikills {
            let stimulus = parse_nonzero_guid(&cue.stimulus, "stimulus")?;
            if cue.lines.is_empty() {
                return Err(invalid(format!(
                    "multikill {count} requires at least one line GUID"
                )));
            }
            let lines = cue
                .lines
                .iter()
                .map(|line| parse_nonzero_guid(line, "line"))
                .collect::<io::Result<Vec<_>>>()?;
            multikills.insert(count, VoiceCueEntry { stimulus, lines });
        }
        Ok(Self {
            multikill_window_ticks: raw.multikill_window_ticks,
            multikills,
        })
    }

    /// Whether the catalog defines no announcer cues.
    pub fn is_empty(&self) -> bool {
        self.multikills.is_empty()
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
        ] {
            let error = CombatVoiceCatalog::from_json(invalid_json).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        }
    }
}
