//! Authored hazards and lifecycle policy, independent of collision safety bounds.

use std::{collections::HashMap, fs, io, path::Path, time::Duration};

use serde::Deserialize;

use crate::accounts::parse_guid;

const MAX_LETHAL_VOLUMES: usize = 64;

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum LethalVolume {
    Aabb(AabbVolume),
    Obb(ObbVolume),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AabbVolume {
    min: [f64; 3],
    max: [f64; 3],
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObbVolume {
    center: [f64; 3],
    half_extents: [f64; 3],
    /// Unit quaternion `[x, y, z, w]` rotating local box axes into world space.
    rotation: [f64; 4],
}

impl LethalVolume {
    fn valid(&self) -> bool {
        match self {
            Self::Aabb(b) => (0..3).all(|axis| {
                b.min[axis].is_finite() && b.max[axis].is_finite() && b.min[axis] < b.max[axis]
            }),
            Self::Obb(o) => {
                let q_norm_sq = o.rotation.iter().map(|v| v * v).sum::<f64>();
                o.center.into_iter().all(f64::is_finite)
                    && o.half_extents.into_iter().all(|h| h.is_finite() && h > 0.0)
                    && o.rotation.into_iter().all(f64::is_finite)
                    && q_norm_sq.is_finite()
                    && q_norm_sq > 1e-12
            }
        }
    }

    fn contains(&self, feet: [f64; 3]) -> bool {
        match self {
            Self::Aabb(b) => {
                (0..3).all(|axis| b.min[axis] <= feet[axis] && feet[axis] <= b.max[axis])
            }
            Self::Obb(o) => {
                let dx = feet[0] - o.center[0];
                let dy = feet[1] - o.center[1];
                let dz = feet[2] - o.center[2];
                let norm = o.rotation.iter().map(|v| v * v).sum::<f64>().sqrt();
                if !norm.is_finite() || norm <= 1e-12 {
                    return false;
                }
                // Rotate delta by conjugate of unit quaternion to transform into local box space.
                let qx = -o.rotation[0] / norm;
                let qy = -o.rotation[1] / norm;
                let qz = -o.rotation[2] / norm;
                let qw = o.rotation[3] / norm;
                let tx = 2.0 * (qy * dz - qz * dy);
                let ty = 2.0 * (qz * dx - qx * dz);
                let tz = 2.0 * (qx * dy - qy * dx);
                let lx = dx + qw * tx + (qy * tz - qz * ty);
                let ly = dy + qw * ty + (qz * tx - qx * tz);
                let lz = dz + qw * tz + (qx * ty - qy * tx);
                lx.abs() <= o.half_extents[0]
                    && ly.abs() <= o.half_extents[1]
                    && lz.abs() <= o.half_extents[2]
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MovingPlatformSpec {
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub half_extents: [f64; 3],
    pub period_secs: f64,
}

impl MovingPlatformSpec {
    pub(crate) fn valid(&self) -> bool {
        self.start.into_iter().all(f64::is_finite)
            && self.end.into_iter().all(f64::is_finite)
            && self
                .half_extents
                .into_iter()
                .all(|h| h.is_finite() && h > 0.0)
            && self.period_secs.is_finite()
            && self.period_secs > 0.0
    }

    pub(crate) fn position_at(&self, time_secs: f64) -> [f64; 3] {
        let phase = (std::f64::consts::TAU * time_secs / self.period_secs).cos();
        let t = 0.5 - 0.5 * phase;
        [
            self.start[0] + (self.end[0] - self.start[0]) * t,
            self.start[1] + (self.end[1] - self.start[1]) * t,
            self.start[2] + (self.end[2] - self.start[2]) * t,
        ]
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HealthPackSpec {
    pub position: [f64; 3],
    #[serde(default)]
    pub mega: bool,
}

impl HealthPackSpec {
    pub(crate) fn valid(&self) -> bool {
        self.position.into_iter().all(f64::is_finite)
    }

    pub(crate) fn heal_amount(&self) -> f64 {
        if self.mega { 250.0 } else { 75.0 }
    }

    pub(crate) fn respawn_ticks(&self, hacked: bool) -> u32 {
        let base = if self.mega { 62 * 15 } else { 62 * 10 };
        if hacked { base / 4 } else { base }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JumpPadSpec {
    pub position: [f64; 3],
    #[serde(default = "default_pad_radius")]
    pub radius: f64,
    pub velocity: [f64; 3],
}

fn default_pad_radius() -> f64 {
    1.6
}

impl JumpPadSpec {
    pub(crate) fn valid(&self) -> bool {
        self.position.into_iter().all(f64::is_finite)
            && self.velocity.into_iter().all(f64::is_finite)
            && self.radius.is_finite()
            && self.radius > 0.0
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct MapEnvironment {
    lethal_volumes: Vec<LethalVolume>,
    player_respawns: HashMap<u64, Duration>,
    health_packs: Vec<HealthPackSpec>,
    jump_pads: Vec<JumpPadSpec>,
    moving_platforms: Vec<MovingPlatformSpec>,
}

impl MapEnvironment {
    /// Whether there are no lethal volumes; lifecycle rules may still be configured.
    pub(crate) fn is_empty(&self) -> bool {
        self.lethal_volumes.is_empty()
    }

    pub(crate) fn player_respawn_delay(&self, mode: u64) -> Option<Duration> {
        self.player_respawns.get(&mode).copied()
    }

    pub(crate) fn health_packs(&self) -> &[HealthPackSpec] {
        &self.health_packs
    }

    pub(crate) fn jump_pads(&self) -> &[JumpPadSpec] {
        &self.jump_pads
    }

    pub(crate) fn moving_platforms(&self) -> &[MovingPlatformSpec] {
        &self.moving_platforms
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
    #[serde(default)]
    health_packs: Vec<HealthPackSpec>,
    #[serde(default)]
    jump_pads: Vec<JumpPadSpec>,
    #[serde(default)]
    moving_platforms: Vec<MovingPlatformSpec>,
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
    /// Ship the known lifecycle rules and let local maps extend or override them.
    pub(crate) fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        let mut defaults = Self::from_json(include_str!("map_environment_defaults.json"))?;
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(defaults),
            Err(error) => return Err(error),
        };
        for (map, mut configured) in Self::from_json(&text)?.maps {
            if let Some(shipped) = defaults.maps.get(&map) {
                for (&mode, &delay) in &shipped.player_respawns {
                    configured.player_respawns.entry(mode).or_insert(delay);
                }
            }
            defaults.maps.insert(map, configured);
        }
        Ok(defaults)
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
            if data.health_packs.iter().any(|pack| !pack.valid()) {
                return Err(invalid(format!(
                    "map {key:?} contains invalid health pack coordinates"
                )));
            }
            if data.jump_pads.iter().any(|pad| !pad.valid()) {
                return Err(invalid(format!(
                    "map {key:?} contains invalid jump pad spec"
                )));
            }
            if data.moving_platforms.iter().any(|plat| !plat.valid()) {
                return Err(invalid(format!(
                    "map {key:?} contains invalid moving platform spec"
                )));
            }
            let environment = MapEnvironment {
                lethal_volumes: data.lethal_volumes,
                player_respawns,
                health_packs: data.health_packs,
                jump_pads: data.jump_pads,
                moving_platforms: data.moving_platforms,
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
    fn practice_respawn_default_survives_missing_data_and_accepts_mode_override() {
        let practice = crate::game_content::GameMap::practice_range();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("environment.json");
        let load = || {
            EnvironmentCatalog::load(&path)
                .unwrap()
                .for_map(practice.map_guid)
        };
        assert_eq!(
            load().player_respawn_delay(practice.mode_guid),
            Some(Duration::from_secs(3))
        );
        assert_eq!(load().player_respawn_delay(42), None);
        fs::write(
            &path,
            json!({"version":1,"maps":{
                (format!("0x{:016X}", practice.map_guid)):{
                    "lethal_volumes":[volume()],
                    "player_respawns":[{
                        "mode":format!("0x{:016X}", practice.mode_guid), "delay_ms":750
                    }]
                }
            }})
            .to_string(),
        )
        .unwrap();
        let configured = load();
        assert_eq!(
            configured.player_respawn_delay(practice.mode_guid),
            Some(Duration::from_millis(750))
        );
        assert!(configured.lethal_at([0.0; 3]));
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

    #[test]
    fn rotated_obb_lethal_volume_and_moving_platform_cycle() {
        // 45-degree Y-axis rotation: sin(22.5 deg), cos(22.5 deg)
        let angle = std::f64::consts::FRAC_PI_4;
        let qy = (angle * 0.5).sin();
        let qw = (angle * 0.5).cos();
        let data = json!({
            "version": 1,
            "maps": {
                (format!("0x{MAP:016X}")): {
                    "lethal_volumes": [{
                        "center": [10.0, -5.0, 0.0],
                        "half_extents": [2.0, 1.0, 0.5],
                        "rotation": [0.0, qy, 0.0, qw]
                    }],
                    "moving_platforms": [{
                        "start": [0.0, 2.0, 0.0],
                        "end": [10.0, 2.0, 0.0],
                        "half_extents": [2.0, 0.3, 2.0],
                        "period_secs": 8.0
                    }]
                }
            }
        });
        let env = EnvironmentCatalog::from_json(&data.to_string())
            .unwrap()
            .for_map(MAP);
        // Point along rotated local X axis (+1.5m along 45 deg diagonal) is inside.
        let diag = 1.5 * std::f64::consts::FRAC_1_SQRT_2;
        assert!(env.lethal_at([10.0 + diag, -5.0, -diag]));
        // Point perpendicular to rotated box (+1.5m along local Z) is outside half_extents.z=0.5.
        assert!(!env.lethal_at([10.0 + diag, -5.0, diag]));
        assert_eq!(env.moving_platforms().len(), 1);
        let plat = env.moving_platforms()[0];
        assert!((plat.position_at(0.0)[0] - 0.0).abs() < 1e-9);
        assert!((plat.position_at(4.0)[0] - 10.0).abs() < 1e-9);
        assert!((plat.position_at(8.0)[0] - 0.0).abs() < 1e-9);
    }
}
