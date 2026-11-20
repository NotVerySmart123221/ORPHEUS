//! Optional authored hazards and lifecycle policy, independent of collision safety bounds.

use std::{collections::HashMap, fs, io, path::Path, time::Duration};

use serde::Deserialize;

use crate::accounts::parse_guid;

const MAX_LETHAL_VOLUMES: usize = 64;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LethalVolume {
    min: [f64; 3],
    max: [f64; 3],
}

impl LethalVolume {
    fn valid(&self) -> bool {
        (0..3).all(|axis| {
            self.min[axis].is_finite()
                && self.max[axis].is_finite()
                && self.min[axis] < self.max[axis]
        })
    }

    fn contains(&self, feet: [f64; 3]) -> bool {
        (0..3).all(|axis| self.min[axis] <= feet[axis] && feet[axis] <= self.max[axis])
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct MapEnvironment {
    lethal_volumes: Vec<LethalVolume>,
    player_respawns: HashMap<u64, Duration>,
}

impl MapEnvironment {
    /// Whether there are no lethal volumes; lifecycle rules may still be configured.
    pub(crate) fn is_empty(&self) -> bool {
        self.lethal_volumes.is_empty()
    }

    pub(crate) fn player_respawn_delay(&self, mode: u64) -> Option<Duration> {
        self.player_respawns.get(&mode).copied()
    }

    /// Box faces count as inside. Invalid poses do not imply a gameplay death.
    pub(crate) fn lethal_at(&self, feet: [f64; 3]) -> bool {
        feet.into_iter().all(f64::is_finite)
            && self
                .lethal_volumes
                .iter()
                .any(|volume| volume.contains(feet))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogData {
    version: u32,
    maps: HashMap<String, MapEnvironmentData>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MapEnvironmentData {
    lethal_volumes: Vec<LethalVolume>,
    #[serde(default)]
    player_respawns: Vec<PlayerRespawnData>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerRespawnData {
    mode: String,
    delay_ms: u32,
}

#[derive(Debug, Default)]
pub(crate) struct EnvironmentCatalog {
    maps: HashMap<u64, MapEnvironment>,
}

impl EnvironmentCatalog {
    /// An omitted opt-in file leaves all maps without environmental hazards.
    pub(crate) fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => Self::from_json(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn from_json(text: &str) -> io::Result<Self> {
        let data: CatalogData =
            serde_json::from_str(text).map_err(|error| invalid(error.to_string()))?;
        if data.version != 1 {
            return Err(invalid("unsupported map environment version"));
        }
        let mut maps = HashMap::with_capacity(data.maps.len());
        for (key, data) in data.maps {
            let map = parse_guid(&key)
                .ok_or_else(|| invalid(format!("invalid environment map GUID {key:?}")))?;
            if data.lethal_volumes.len() > MAX_LETHAL_VOLUMES {
                return Err(invalid(format!(
                    "map {key:?} exceeds the {MAX_LETHAL_VOLUMES} lethal volume limit"
                )));
            }
            if data.lethal_volumes.iter().any(|volume| !volume.valid()) {
                return Err(invalid(format!(
                    "map {key:?} requires finite lethal volume bounds with min < max on every axis"
                )));
            }
            let mut player_respawns = HashMap::with_capacity(data.player_respawns.len());
            for rule in data.player_respawns {
                let mode = parse_guid(&rule.mode).ok_or_else(|| {
                    invalid(format!("invalid player respawn mode GUID {:?}", rule.mode))
                })?;
                if rule.delay_ms == 0 {
                    return Err(invalid("player respawn delay_ms must be positive"));
                }
                let delay = Duration::from_millis(u64::from(rule.delay_ms));
                if player_respawns.insert(mode, delay).is_some() {
                    return Err(invalid(format!(
                        "duplicate player respawn mode GUID {:?} for map {key:?}",
                        rule.mode
                    )));
                }
            }
            let environment = MapEnvironment {
                lethal_volumes: data.lethal_volumes,
                player_respawns,
            };
            if maps.insert(map, environment).is_some() {
                return Err(invalid(format!("duplicate environment map GUID {key:?}")));
            }
        }
        Ok(Self { maps })
    }

    pub(crate) fn for_map(&self, map: u64) -> MapEnvironment {
        self.maps.get(&map).cloned().unwrap_or_default()
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const MAP: u64 = 0x0800_0000_0052_0701;

    fn source(volumes: Vec<Value>) -> Value {
        json!({"version":1,"maps":{
            (format!("0x{MAP:016X}")):{"lethal_volumes":volumes}
        }})
    }

    fn volume() -> Value {
        json!({"min":[-2,-4,-6],"max":[2,4,6]})
    }

    #[test]
    fn authored_boxes_include_faces_and_keep_unconfigured_maps_safe() {
        let catalog = EnvironmentCatalog::from_json(
            &source(vec![volume(), json!({"min":[10,10,10],"max":[12,12,12]})]).to_string(),
        )
        .unwrap();
        let environment = catalog.for_map(MAP);
        assert!(!environment.is_empty());
        assert_eq!(environment.player_respawn_delay(42), None);
        for feet in [
            [0.0; 3],
            [-2.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [0.0, -4.0, 0.0],
            [0.0, 4.0, 0.0],
            [0.0, 0.0, -6.0],
            [0.0, 0.0, 6.0],
            [-2.0, -4.0, -6.0],
            [2.0, 4.0, 6.0],
            [11.0; 3],
        ] {
            assert!(environment.lethal_at(feet), "{feet:?}");
        }
        for feet in [
            [-2.000_001, 0.0, 0.0],
            [2.000_001, 0.0, 0.0],
            [0.0, -4.000_001, 0.0],
            [0.0, 4.000_001, 0.0],
            [0.0, 0.0, -6.000_001],
            [0.0, 0.0, 6.000_001],
            [7.0; 3],
            [f64::NAN, 0.0, 0.0],
            [0.0, f64::INFINITY, 0.0],
        ] {
            assert!(!environment.lethal_at(feet), "{feet:?}");
        }
        assert!(catalog.for_map(MAP + 1).is_empty());
        assert!(!catalog.for_map(MAP + 1).lethal_at([0.0; 3]));
        let mut detached = catalog.for_map(MAP);
        detached.lethal_volumes.clear();
        assert!(catalog.for_map(MAP).lethal_at([0.0; 3]));
    }

    #[test]
    fn player_respawn_rules_select_only_the_authored_map_and_mode() {
        let mut data = source(vec![volume()]);
        let map = format!("0x{MAP:016X}");
        data["maps"][&map]["player_respawns"] = json!([
            {"mode":"0x2A","delay_ms":500},
            {"mode":"0x2B","delay_ms":1200}
        ]);
        let catalog = EnvironmentCatalog::from_json(&data.to_string()).unwrap();
        let environment = catalog.for_map(MAP);
        assert_eq!(
            environment.player_respawn_delay(42),
            Some(Duration::from_millis(500))
        );
        assert_eq!(
            environment.player_respawn_delay(43),
            Some(Duration::from_millis(1200))
        );
        assert_eq!(environment.player_respawn_delay(44), None);
        assert_eq!(catalog.for_map(MAP + 1).player_respawn_delay(42), None);
        assert!(environment.lethal_at([0.0; 3]));

        // A lifecycle-only map does not enable the lethal-volume sampling path.
        data["maps"][&map]["lethal_volumes"] = json!([]);
        data["maps"][&map]["player_respawns"] = json!([
            {"mode":"0x2A","delay_ms":u32::MAX}
        ]);
        let environment = EnvironmentCatalog::from_json(&data.to_string())
            .unwrap()
            .for_map(MAP);
        assert!(environment.is_empty());
        assert_eq!(
            environment.player_respawn_delay(42),
            Some(Duration::from_millis(u64::from(u32::MAX)))
        );
    }

    #[test]
    fn player_respawn_rules_reject_invalid_delays_modes_and_unknown_fields() {
        for rules in [
            json!([{"mode":"0x2A","delay_ms":0}]),
            json!([{"mode":"0x2A","delay_ms":-1}]),
            json!([{"mode":"0x2A","delay_ms":0.5}]),
            json!([{"mode":"0x2A","delay_ms":true}]),
            json!([{"mode":"0x2A","delay_ms":4294967296_u64}]),
            json!([{"mode":"0x2A"}]),
            json!([{"mode":"invalid","delay_ms":1}]),
            json!([{"mode":"0x2A","delay_ms":1,"hero":7}]),
            json!([{"mode":"0x2A","delay_ms":1},{"mode":"42","delay_ms":2}]),
        ] {
            let mut data = source(vec![]);
            data["maps"][format!("0x{MAP:016X}")]["player_respawns"] = rules;
            assert_eq!(
                EnvironmentCatalog::from_json(&data.to_string())
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidData
            );
        }
    }

    #[test]
    fn catalog_rejects_unknown_fields_invalid_bounds_and_map_aliases() {
        for text in [
            r#"{"version":2,"maps":{}}"#,
            r#"{"version":1,"maps":{},"guessed_floor":-100}"#,
            r#"{"version":1,"maps":{"0x01":{"lethal_volumes":[],"extra":true}}}"#,
            r#"{"version":1,"maps":{"0x01":{"lethal_volumes":[{"min":[0,0,0],"max":[1,1,1],"damage":5}]}}}"#,
            r#"{"version":1,"maps":{"not-a-guid":{"lethal_volumes":[]}}}"#,
            r#"{"version":1,"maps":{"0x01":{"lethal_volumes":[]},"1":{"lethal_volumes":[]}}}"#,
            r#"{"version":1,"maps":{"0x01":{}}}"#,
            r#"{"version":1,"maps":{"0x01":{"lethal_volumes":[{"min":[0,0,0],"max":[1e999,1,1]}]}}}"#,
        ] {
            assert_eq!(
                EnvironmentCatalog::from_json(text).unwrap_err().kind(),
                io::ErrorKind::InvalidData,
                "{text}"
            );
        }
        for axis in 0..3 {
            for reversed in [false, true] {
                let mut invalid_volume = volume();
                let lower = invalid_volume["min"][axis].as_f64().unwrap();
                invalid_volume["max"][axis] = json!(if reversed { lower - 1.0 } else { lower });
                assert!(
                    EnvironmentCatalog::from_json(&source(vec![invalid_volume]).to_string())
                        .is_err()
                );
            }
        }
    }

    #[test]
    fn volume_budget_is_bounded_and_missing_configuration_is_empty() {
        let maximum = source(vec![volume(); MAX_LETHAL_VOLUMES]);
        assert!(EnvironmentCatalog::from_json(&maximum.to_string()).is_ok());
        let oversized = source(vec![volume(); MAX_LETHAL_VOLUMES + 1]);
        assert!(EnvironmentCatalog::from_json(&oversized.to_string()).is_err());
        let empty = EnvironmentCatalog::from_json(r#"{"version":1,"maps":{}}"#).unwrap();
        assert!(empty.for_map(MAP).is_empty());
        let path = std::env::temp_dir().join(format!(
            "orpheus-missing-environment-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(!path.exists());
        assert!(
            EnvironmentCatalog::load(path)
                .unwrap()
                .for_map(MAP)
                .is_empty()
        );
    }
}
