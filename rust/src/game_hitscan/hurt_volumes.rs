use std::{collections::BTreeMap, fs, io, path::Path};

use serde::Deserialize;

use super::{HEAD_BELOW_EYE, TargetPose};
use crate::{
    accounts::parse_guid,
    static_ray::{Vec3, capsule_entry_distance, norm, segment_separation},
};

/// Sampled movement pose plus the optional local, standing combat profile.
#[derive(Clone, Copy)]
pub(crate) struct HurtPose {
    movement: TargetPose,
    calibrated: Option<HurtProfile>,
}

impl From<TargetPose> for HurtPose {
    fn from(movement: TargetPose) -> Self {
        Self {
            movement,
            calibrated: None,
        }
    }
}

impl HurtPose {
    fn capsules(self) -> impl Iterator<Item = (Capsule, bool)> {
        let [x, y, z] = self.movement.feet;
        let world = |shape: LocalCapsule, head| {
            (
                Capsule {
                    ends: [[x, y + shape.lower, z], [x, y + shape.upper, z]],
                    radius: shape.radius,
                },
                head,
            )
        };
        let regions = match self.calibrated {
            Some(profile) => [
                Some(world(profile.body, false)),
                profile.head.map(|shape| world(shape, true)),
            ],
            None => [
                Some((
                    Capsule {
                        ends: self.movement.segment(),
                        radius: self.movement.radius,
                    },
                    false,
                )),
                None,
            ],
        };
        regions.into_iter().flatten()
    }

    pub(crate) fn contact(
        self,
        origin: Vec3,
        direction: Vec3,
        reach: f64,
        padding: f64,
    ) -> Option<(f64, bool)> {
        let (distance, head) = self
            .capsules()
            .filter_map(|(capsule, head)| {
                let [a, b] = capsule.ends;
                capsule_entry_distance(origin, direction, reach, a, b, capsule.radius + padding)
                    .map(|distance| (distance, head))
            })
            .min_by(|(a, _), (b, _)| a.total_cmp(b))?;
        let head = if self.calibrated.is_some() {
            head
        } else {
            // Compatibility for bodies that have no separately measured combat profile.
            let impact_y = origin[1] + direction[1] * distance;
            impact_y >= self.movement.feet[1] + self.movement.eye - HEAD_BELOW_EYE
        };
        Some((distance, head))
    }

    pub(crate) fn nearest_surface(self, point: Vec3, origin: Vec3) -> Option<Surface> {
        let (distance, capsule) = self
            .capsules()
            .map(|(capsule, _)| {
                let [a, b] = capsule.ends;
                let distance = (norm(segment_separation(point, a, b)) - capsule.radius).max(0.0);
                (distance, capsule)
            })
            .min_by(|(a, _), (b, _)| a.total_cmp(b))?;
        let [a, b] = capsule.ends;
        let from_axis = segment_separation(origin, a, b);
        let axis_distance = norm(from_axis);
        Some(Surface {
            distance,
            axis_distance,
            from_axis,
            sight_reach: axis_distance - capsule.radius,
        })
    }
}

pub(crate) struct Surface {
    pub distance: f64,
    pub axis_distance: f64,
    pub from_axis: Vec3,
    pub sight_reach: f64,
}

struct Capsule {
    ends: [Vec3; 2],
    radius: f64,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalCapsule {
    /// Endpoints of a vertical capsule axis, in metres above the feet.
    lower: f64,
    upper: f64,
    radius: f64,
}

impl LocalCapsule {
    fn valid(self) -> bool {
        self.lower.is_finite()
            && self.upper.is_finite()
            && self.lower <= self.upper
            && self.radius.is_finite()
            && self.radius > 0.0
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct HurtProfile {
    body: LocalCapsule,
    head: Option<LocalCapsule>,
}

#[derive(Default)]
pub(crate) struct HurtVolumeCatalog {
    by_body: BTreeMap<u64, HurtProfile>,
}

impl HurtVolumeCatalog {
    pub(crate) fn load(path: &Path) -> io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => Self::from_json(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn from_json(text: &str) -> io::Result<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Data {
            version: u32,
            bodies: BTreeMap<String, HurtProfile>,
        }

        let invalid = |message: &str| io::Error::new(io::ErrorKind::InvalidData, message);
        let data: Data = serde_json::from_str(text)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        if data.version != 1 {
            return Err(invalid("unsupported combat hurt volume catalog version"));
        }
        let mut by_body = BTreeMap::new();
        for (key, profile) in data.bodies {
            let body = parse_guid(&key)
                .filter(|body| *body != 0)
                .ok_or_else(|| invalid("invalid combat hurt volume body GUID"))?;
            if !profile.body.valid() || profile.head.is_some_and(|head| !head.valid()) {
                return Err(invalid("invalid combat hurt volume capsule"));
            }
            if by_body.insert(body, profile).is_some() {
                return Err(invalid("duplicate combat hurt volume body GUID"));
            }
        }
        Ok(Self { by_body })
    }

    pub(crate) fn pose(&self, body: u64, movement: TargetPose) -> HurtPose {
        HurtPose {
            movement,
            calibrated: self.by_body.get(&body).copied(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::{
        game_hitscan::{DamageTags, HitDefinition, HitscanShot},
        projectile_flight::{ProjectileBallistics, ProjectileFlight},
    };

    #[test]
    fn calibrated_combat_volumes_distinguish_head_body_and_empty_space() {
        let contract: Value = serde_json::from_str(include_str!(
            "../../../fixtures/behavior/combat-hurt-volumes-v1.json"
        ))
        .unwrap();
        assert_eq!(contract["case_id"], "combat-hurt-volumes.synthetic.v1");
        let input = &contract["input"];
        let body = parse_guid(input["body"].as_str().unwrap()).unwrap();
        let mover = &input["movement"];
        let movement = TargetPose {
            feet: serde_json::from_value(mover["feet"].clone()).unwrap(),
            height: mover["height"].as_f64().unwrap(),
            radius: mover["radius"].as_f64().unwrap(),
            eye: mover["eye"].as_f64().unwrap(),
        };
        let profile = HurtVolumeCatalog::from_json(&input["catalog"].to_string()).unwrap();
        let target = profile.pose(body, movement);
        let compatibility = profile.pose(body + 1, movement);
        let direction = [0.0, 0.0, 1.0];
        let hit = HitDefinition {
            tags: DamageTags::EMPTY,
            amount: 20.0,
            loadout: None,
            critical: 2.0,
            falloff: None,
        };
        for case in contract["cases"].as_array().unwrap() {
            let origin = serde_json::from_value(case["origin"].clone()).unwrap();
            let critical = case["critical"].as_bool();
            let shot = HitscanShot::with_direction(origin, direction, 10.0, hit);
            let resolve = |target| {
                shot.resolve_combat([(1, target)], |_, _, _| Ok(None))
                    .unwrap()
            };
            assert_eq!(
                resolve(target).map(|impact| (impact.critical, impact.damage)),
                critical.map(|head| (head, if head { 40.0 } else { 20.0 })),
                "{}",
                case["name"]
            );
            match case["name"].as_str().unwrap() {
                "head-above-mover" | "side-outside-mover" => {
                    assert!(resolve(compatibility).is_none());
                }
                "chest-below-head" => assert!(resolve(compatibility).unwrap().critical),
                _ => {}
            }
            assert!(
                shot.resolve_combat([(1, target)], |_, _, _| Ok(Some(1.0)))
                    .unwrap()
                    .is_none()
            );
            let mut flight = ProjectileFlight::new(
                ProjectileBallistics {
                    speed: 600.0,
                    lifetime: 0.5,
                    radius: 0.0,
                    definition: None,
                    direct: Some(hit),
                    blast: None,
                },
                origin,
                direction,
                None,
            )
            .unwrap();
            let impact = flight
                .advance_combat(1, &[(1, target)], None, |_, _, _| Ok(None))
                .unwrap()
                .map(|end| end.impacts[0]);
            assert_eq!(impact.is_some(), critical.is_some(), "{}", case["name"]);
        }
    }

    #[test]
    #[ignore = "requires a calibrated 104319 Practice Range pack and private ray replay"]
    fn prepared_live_rays_hit_the_calibrated_bot_regions() {
        use std::{path::PathBuf, time::Duration};

        use crate::{
            commands::Command,
            game_combat::{PosedHitscan, ShotContext},
            game_combat_messages::CombatNotice,
            game_content::GameMap,
            game_instance::RETAIL_QUANTA,
            scene_assets::SceneAssets,
        };

        #[derive(Deserialize)]
        struct Replay {
            target: u32,
            samples: Vec<Ray>,
        }

        #[derive(Deserialize)]
        struct Ray {
            frame: u32,
            origin: Vec3,
            direction: Vec3,
            critical: Option<bool>,
        }

        let root = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
        let replay = PathBuf::from(std::env::var_os("ORPHEUS_HURT_REPLAY").unwrap());
        let replay: Replay = serde_json::from_slice(&fs::read(replay).unwrap()).unwrap();
        assert!(!replay.samples.is_empty());
        let mut scene = SceneAssets::load(&root)
            .unwrap()
            .prepare(GameMap::practice_range())
            .unwrap();
        let (room, queries) = scene.bot_simulation();
        let target = room
            .bodies()
            .unwrap()
            .find(|bot| bot.entity == replay.target)
            .unwrap();
        assert!(
            queries
                .hurt_pose(target.spawn.body, queries.target(target))
                .calibrated
                .is_some()
        );
        for sample in replay.samples {
            let result = room
                .hitscan(
                    &queries,
                    PosedHitscan {
                        source: 0xa000_0101,
                        origin: sample.origin,
                        direction: sample.direction,
                        command: Command {
                            frame: sample.frame,
                            ..Command::default()
                        },
                        shot_frame: sample.frame,
                        reach: 30.0,
                        hit: HitDefinition {
                            tags: DamageTags::EMPTY,
                            amount: 2.0,
                            loadout: None,
                            critical: 2.0,
                            falloff: None,
                        },
                    },
                    ShotContext {
                        team: 0,
                        quanta: RETAIL_QUANTA,
                        tick: sample.frame,
                        now: Duration::ZERO,
                    },
                    |entity| entity == replay.target,
                )
                .unwrap();
            let observed = result.map(|hit| match hit.notices[0] {
                Some(CombatNotice::Hit {
                    target, critical, ..
                }) => {
                    assert_eq!(target, replay.target);
                    critical
                }
                _ => panic!("missing ordinary damage notice"),
            });
            assert_eq!(observed, sample.critical, "frame {}", sample.frame);
        }
    }
}
