//! Retail 104319 animation durations for `STUStatescriptStateAnim`.

use std::{collections::HashMap, fs, io, path::Path};

use crate::accounts::parse_guid;

const ANY_CATEGORY: u64 = u64::MAX;

type RawDurationMap = HashMap<String, HashMap<String, HashMap<String, f64>>>;

#[derive(Debug, Default)]
pub(crate) struct AnimationTiming {
    emote_durations: HashMap<(u64, u64, u64), f64>,
    greeting_durations: HashMap<(u64, u64, u64), f64>,
}

fn read_optional_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> io::Result<T> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error),
    }
}

fn parse_hex_key(text: &str) -> io::Result<u64> {
    parse_guid(text)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("invalid GUID {text:?}")))
}

impl AnimationTiming {
    pub(crate) fn load(data_dir: &Path) -> io::Result<Option<Self>> {
        let raw_emote_durs: RawDurationMap =
            read_optional_json(&data_dir.join("emote_durations_174.json"))?;
        let raw_greeting_durs: RawDurationMap =
            read_optional_json(&data_dir.join("greeting_durations_174.json"))?;
        if raw_emote_durs.is_empty() && raw_greeting_durs.is_empty() {
            return Ok(None);
        }

        let mut emote_durations = HashMap::new();
        for (owner, aliases) in raw_emote_durs {
            let owner = parse_hex_key(&owner)?;
            for (alias, categories) in aliases {
                let alias = parse_hex_key(&alias)?;
                for (category, seconds) in categories {
                    let category = parse_hex_key(&category)?;
                    if seconds.is_finite() && seconds > 0.0 {
                        emote_durations.insert((owner, alias, category), seconds);
                    }
                }
            }
        }

        let mut greeting_durations = HashMap::new();
        for (owner, aliases) in raw_greeting_durs {
            let owner = parse_hex_key(&owner)?;
            for (alias, categories) in aliases {
                let alias = parse_hex_key(&alias)?;
                for (category, seconds) in categories {
                    let category = parse_hex_key(&category)?;
                    if seconds.is_finite() && seconds > 0.0 {
                        greeting_durations.insert((owner, alias, category), seconds);
                    }
                }
            }
        }

        Ok(Some(Self {
            emote_durations,
            greeting_durations,
        }))
    }

    pub(crate) fn duration(
        &self,
        body: u64,
        blend_tree: u64,
        alias: u64,
        category: u64,
    ) -> Option<f64> {
        if alias == 0 {
            return Some(0.0);
        }
        let (table, owner) = if blend_tree != 0 {
            (&self.emote_durations, blend_tree)
        } else {
            (&self.greeting_durations, body)
        };
        table
            .get(&(owner, alias, category))
            .or_else(|| table.get(&(owner, alias, ANY_CATEGORY)))
            .copied()
    }
}
