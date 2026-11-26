use std::{collections::BTreeMap, fs, io, path::Path};

use serde::Deserialize;

use super::{HEAD_BELOW_EYE, TargetPose};
use crate::{
    accounts::parse_guid,
    static_ray::{SurfaceHit, Vec3, capsule_entry_hit, norm, segment_separation},
};

/// Live skeletal posture inputs used to articulate multi-region hurtboxes for
/// hero bodies that do not carry a separate static calibration entry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SkeletalStance {
    pub yaw: i32,
    pub speed: f64,
    pub crouched: bool,
    pub head_protected: bool,
}

/// Sampled movement pose plus an optional combat profile for that posture.
#[derive(Clone, Copy)]
pub(crate) struct HurtPose {
    movement: TargetPose,
    calibrated: Option<HurtProfile>,
    stance: Option<SkeletalStance>,
}

impl From<TargetPose> for HurtPose {
    fn from(movement: TargetPose) -> Self {
        Self {
            movement,
            calibrated: None,
            stance: None,
        }
    }
}

impl HurtPose {
    pub(crate) fn with_stance(mut self, stance: SkeletalStance) -> Self {
        self.stance = Some(stance);
        self
    }

    pub(crate) fn feet(self) -> Vec3 {
        self.movement.feet
    }

    #[cfg(test)]
    pub(crate) fn head_probe_height(self) -> Option<f64> {
        let offset = match self.calibrated {
            Some(profile) => {
                let head = profile.head?;
                head.lower.midpoint(head.upper)
            }
            None => self.movement.eye,
        };
        Some(self.movement.feet[1] + offset)
    }

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
        let regions = match (self.calibrated, self.stance) {
            (Some(profile), _) => [
                Some(world(profile.body, false)),
                profile.head.map(|shape| world(shape, true)),
                None,
            ],
            (None, Some(stance)) => {
                let h = self.movement.height.max(0.8);
                let r = self.movement.radius.max(0.25);
                let rad = f64::from(stance.yaw) * std::f64::consts::TAU / 65_536.0;
                let (sin, cos) = rad.sin_cos();
                let lean = if stance.crouched {
                    0.09
                } else {
                    (stance.speed / 8.0).clamp(0.0, 1.0) * 0.08
                };
                let legs_r = (r * 0.62).max(0.18);
                let legs_top = (h * 0.46).max(legs_r + 0.05);
                let legs = Capsule {
                    ends: [[x, y + legs_r, z], [x, y + legs_top, z]],
                    radius: legs_r,
                };
                let torso_r = (r * 0.94).max(0.24);
                let torso_low = (h * 0.46).max(torso_r);
                let torso_high = (h * 0.78).max(torso_low + 0.05);
                let torso = Capsule {
                    ends: [
                        [
                            x + sin * (lean * 0.4),
                            y + torso_low,
                            z + cos * (lean * 0.4),
                        ],
                        [x + sin * lean, y + torso_high, z + cos * lean],
                    ],
                    radius: torso_r,
                };
                let head = (!stance.head_protected).then(|| {
                    let head_r = (r * 0.42).clamp(0.18, 0.28);
                    let head_y = (self.movement.eye + 0.04).clamp(torso_high, h - head_r * 0.5);
                    let head_fwd = lean * 1.35;
                    let center = [x + sin * head_fwd, y + head_y, z + cos * head_fwd];
                    (
                        Capsule {
                            ends: [center, center],
                            radius: head_r,
                        },
                        true,
                    )
                });
                [Some((legs, false)), Some((torso, false)), head]
            }
            (None, None) => [
                Some((
                    Capsule {
                        ends: self.movement.segment(),
                        radius: self.movement.radius,
                    },
                    false,
                )),
                None,
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
    ) -> Option<HurtContact> {
        let mut best_body: Option<HurtContact> = None;
        let mut best_head: Option<HurtContact> = None;
        for (capsule, head) in self.capsules() {
            let [a, b] = capsule.ends;
            if let Some(hit) =
                capsule_entry_hit(origin, direction, reach, a, b, capsule.radius + padding)
            {
                let candidate = HurtContact { capsule, hit, head };
                let slot = if head { &mut best_head } else { &mut best_body };
                if slot
                    .as_ref()
                    .is_none_or(|prev| candidate.hit.distance < prev.hit.distance)
                {
                    *slot = Some(candidate);
                }
            }
        }
        let mut contact = match (best_head, best_body) {
            (Some(head), Some(body)) => {
                // When a ray/projectile intersects both the head sphere and a wider
                // chest capsule whose upper dome protrudes slightly further forward,
                // prioritize the headshot so projectiles and face shots are not stolen
                // by the torso volume.
                let front_margin = (body.capsule.radius - head.capsule.radius).max(0.0) + 0.15;
                if head.hit.distance <= body.hit.distance + front_margin {
                    head
                } else {
                    body
                }
            }
            (Some(head), None) => head,
            (None, Some(body)) => body,
            (None, None) => return None,
        };
        if self.calibrated.is_none() {
            // Compatibility for bodies that have no separately measured combat profile.
            let impact_y = origin[1] + direction[1] * contact.hit.distance;
            contact.head = impact_y >= self.movement.feet[1] + self.movement.eye - HEAD_BELOW_EYE;
        }
        Some(contact)
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
    axis_distance: f64,
    from_axis: Vec3,
    sight_reach: f64,
}

impl Surface {
    pub(crate) fn visible<E>(
        &self,
        cast: impl FnOnce(Vec3, f64) -> Result<Option<f64>, E>,
    ) -> Result<bool, E> {
        const EPSILON: f64 = 1e-9;
        let reach = self.sight_reach;
        if reach <= EPSILON {
            return Ok(true);
        }
        let toward = self.from_axis.map(|axis| -axis / self.axis_distance);
        Ok(cast(toward, reach)?.is_none_or(|hit| hit + EPSILON >= reach))
    }
}

pub(crate) struct HurtContact {
    capsule: Capsule,
    pub(crate) hit: SurfaceHit,
    pub(crate) head: bool,
}

impl HurtContact {
    pub(crate) fn point_on_body(&self, sweep_centre: Vec3) -> Vec3 {
        let [a, b] = self.capsule.ends;
        let separation = segment_separation(sweep_centre, a, b);
        std::array::from_fn(|axis| {
            sweep_centre[axis] - separation[axis] + self.hit.normal[axis] * self.capsule.radius
        })
    }
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

impl HurtProfile {
    fn valid(&self) -> bool {
        self.body.valid() && self.head.is_none_or(LocalCapsule::valid)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BodyProfiles {
    body: LocalCapsule,
    head: Option<LocalCapsule>,
    crouched: Option<HurtProfile>,
}

#[derive(Default)]
pub(crate) struct HurtVolumeCatalog {
    by_body: BTreeMap<u64, BodyProfiles>,
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
            bodies: BTreeMap<String, BodyProfiles>,
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
            if !profile.body.valid()
                || profile.head.is_some_and(|head| !head.valid())
                || profile.crouched.as_ref().is_some_and(|pose| !pose.valid())
            {
                return Err(invalid("invalid combat hurt volume capsule"));
            }
            if by_body.insert(body, profile).is_some() {
                return Err(invalid("duplicate combat hurt volume body GUID"));
            }
        }
        Ok(Self { by_body })
    }

    pub(crate) fn pose(&self, body: u64, movement: TargetPose, crouched: bool) -> HurtPose {
        let calibrated = self.by_body.get(&body).and_then(|profiles| {
            if crouched {
                profiles.crouched
            } else {
                Some(HurtProfile {
                    body: profiles.body,
                    head: profiles.head,
                })
            }
        });
        HurtPose {
            movement,
            calibrated,
            stance: None,
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
        scene_assets::PreparedScene,
    };

    #[test]
    fn crouched_hurt_pose_uses_matching_calibration_or_mover_shape() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../fixtures/behavior/combat-hurt-volumes-v1.json"
        ))
        .unwrap();
        let input = &fixture["input"];
        let crouched = &fixture["crouched"];
        let key = input["body"].as_str().unwrap();
        let body = parse_guid(key).unwrap();
        let pose = TargetPose {
            feet: serde_json::from_value(input["movement"]["feet"].clone()).unwrap(),
            height: crouched["height"].as_f64().unwrap(),
            radius: input["movement"]["radius"].as_f64().unwrap(),
            eye: crouched["eye"].as_f64().unwrap(),
        };
        let mut scene = PreparedScene::fixture_for_bots(Vec::new());
        let direction = [0.0, 0.0, 1.0];
        let standing_ray = serde_json::from_value(crouched["standing_head_ray"].clone()).unwrap();
        let crouched_ray = serde_json::from_value(crouched["crouched_head_ray"].clone()).unwrap();
        let mut data = input["catalog"].clone();
        for custom in [false, true] {
            if custom {
                data["bodies"][key]["crouched"] = crouched["profile"].clone();
            }
            let catalog = HurtVolumeCatalog::from_json(&data.to_string()).unwrap();
            scene.fixture_set_hurt_volumes(catalog);
            let (_, queries) = scene.bot_simulation();
            let sampled = queries.hurt_pose(body, pose);
            assert_eq!(sampled.calibrated.is_some(), custom);
            assert!(
                sampled
                    .contact(standing_ray, direction, 10.0, 0.0)
                    .is_none()
            );
            assert!(
                sampled
                    .contact(crouched_ray, direction, 10.0, 0.0)
                    .unwrap()
                    .head
            );
        }
    }

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
        let target = profile.pose(body, movement, false);
        let compatibility = profile.pose(body + 1, movement, false);
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
            for profile in input["projectile_profiles"].as_array().unwrap() {
                let multiplier = profile["critical_multiplier"].as_f64().unwrap();
                let expected = critical.map(|head| {
                    let critical = head && multiplier > 1.0;
                    (critical, if critical { 20.0 * multiplier } else { 20.0 })
                });
                let mut flight = ProjectileFlight::new(
                    ProjectileBallistics {
                        speed: 600.0,
                        lifetime: 0.5,
                        max_distance: None,
                        terrain_rebound: None,
                        terrain_radius: 0.0,
                        radius: 0.0,
                        definition: None,
                        direct: Some(HitDefinition {
                            critical: multiplier,
                            ..hit
                        }),
                        blast: None,
                    },
                    origin,
                    direction,
                    None,
                )
                .unwrap();
                let impact = flight
                    .advance_combat(1, &[(1, target)], None, |_, _, _, _| Ok(None))
                    .unwrap()
                    .map(|end| (end.impacts[0].critical, end.impacts[0].damage));
                assert_eq!(impact, expected, "{} ({multiplier})", case["name"]);
            }
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
