//! Server-side movement for an authored 104319 character capsule.

use std::{collections::BTreeMap, sync::Arc};

use crate::{
    collision_index::TriangleIndex,
    commands::Command,
    contact_body::CharacterBody,
    contact_cache::CapsulePose,
    contact_gjk::PointSetSize,
    contact_hull::HullContactError,
    contact_solver::clip_velocity,
    contact_world::ContactWorld,
    mover_ground::{GroundProbe, ProbePose, pogo, standing_body},
    mover_kinematics::{Ground, MotionInput, Poses, VelocityStep, poses},
    mover_modifiers::{Modifier, ModifierKey, ModifierTimeline},
    mover_posture::{
        CROUCHED, IN_AIR, JUMPED, NOT_GROUND_UNDER, Posture, PostureStep, TOUCHING, snap,
    },
    mover_settings::MoverSettings,
    script_init::{Relocation, ScriptedMotion, SweptDisplacement},
    world::{Movement, NO_INPUT},
};

const ABYSS: f64 = 100.0;
const EPSILON: f64 = 1.1754943508222875e-36;

/// Authored velocity edits, applied once before the next collision-aware pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VelocityChange {
    Delta([f64; 3]),
    HorizontalCap(f64),
    VerticalImpulse { vertical: f64, delta: [f64; 3] },
}

impl VelocityChange {
    /// Apply this edit and report a positive launch that should leave the ground.
    pub(crate) fn apply(self, velocity: &mut [f64; 3]) -> bool {
        match self {
            Self::Delta(delta) => {
                for (axis, delta) in velocity.iter_mut().zip(delta) {
                    *axis = f64::from((*axis as f32) + delta as f32);
                }
            }
            Self::HorizontalCap(cap) => {
                let speed = velocity[0].hypot(velocity[2]);
                if speed > cap && speed > 0.0 {
                    for axis in [0, 2] {
                        velocity[axis] = f64::from((velocity[axis] * cap / speed) as f32);
                    }
                }
            }
            Self::VerticalImpulse { vertical, delta } => {
                let mut next = *velocity;
                next[1] = vertical;
                Self::Delta(delta).apply(&mut next);
                if next.iter().all(|axis| axis.is_finite()) {
                    *velocity = next;
                    return next[1] > 0.0;
                }
            }
        }
        false
    }

    pub(crate) fn valid(self) -> bool {
        match self {
            Self::Delta(delta) => delta.iter().all(|value| (*value as f32).is_finite()),
            Self::HorizontalCap(cap) => (cap as f32).is_finite() && cap >= 0.0,
            Self::VerticalImpulse { vertical, delta } => {
                (vertical as f32).is_finite()
                    && Self::Delta(delta).valid()
                    && (vertical as f32 + delta[1] as f32).is_finite()
            }
        }
    }
}

/// Project the owner's current movement record into the authored 13-bit sample.
pub(crate) fn sample_movement_flags(pose: &Movement) -> Option<u16> {
    // These modes require nested native movement records. Regular Rust poses
    // cannot enter them or enqueue the scheduled transition represented by bit 10.
    if pose.flags & (0x20 | 0x80) != 0 {
        return None;
    }
    let [x, y, z] = pose.velocity.map(|value| value as f32);
    if ![x, y, z].iter().all(|value| value.is_finite()) {
        return None;
    }
    let airborne = pose.flags & IN_AIR != 0;
    let vertical_threshold = 100.0 * f32::EPSILON;
    let moving = (x * x + y * y) + z * z > 10.0 * f32::EPSILON;
    Some(
        u16::from(airborne)
            | u16::from(!airborne) << 1
            | u16::from(y > vertical_threshold) << 2
            | u16::from(y < -vertical_threshold) << 3
            | u16::from(moving) << 4
            | u16::from(pose.throttles != [0; 2]) << 5
            | u16::from(pose.flags & CROUCHED != 0) << 6
            | u16::from(pose.flags & NOT_GROUND_UNDER != 0) << 9
            | u16::from(pose.flags & JUMPED != 0) << 11,
    )
}

pub struct Mover {
    base: MoverSettings,
    category: u16,
    probe: GroundProbe,
    body: CharacterBody,
    posture: Posture,
    modifiers: ModifierTimeline,
    cinematic_aim: Option<[i32; 2]>,
    authored_facing: Option<[i32; 2]>,
    player_input_blocked: bool,
    velocity_changes: Vec<(u32, VelocityChange)>,
    scripted_motion: Option<(u32, ScriptedMotion)>,
    scripted_motion_frame: Option<u32>,
    last_command: Option<Command>,
    last_input: Option<u32>,
    pose: Movement,
    #[cfg(any(test, feature = "inspector"))]
    last_ground_probe: Option<crate::game_debug::DebugGroundProbe>,
    #[cfg(any(test, feature = "inspector"))]
    last_contact_normals: Vec<crate::game_debug::DebugContactNormal>,
}

impl Mover {
    pub fn new(
        base: MoverSettings,
        world: Arc<ContactWorld>,
        position: [f64; 3],
        yaw: i32,
        category: u16,
    ) -> Result<Self, PointSetSize> {
        let position = position.map(snap);
        Ok(Self {
            base,
            category,
            probe: GroundProbe::new(&base)?,
            body: standing_body(&base, world, position, category),
            posture: Posture::new(&base),
            modifiers: ModifierTimeline::default(),
            cinematic_aim: None,
            authored_facing: None,
            player_input_blocked: false,
            velocity_changes: Vec::new(),
            scripted_motion: None,
            scripted_motion_frame: None,
            last_command: None,
            last_input: None,
            pose: Movement {
                position,
                yaw,
                gravity: base.gravity_scale,
                ..Movement::default()
            },
            #[cfg(any(test, feature = "inspector"))]
            last_ground_probe: None,
            #[cfg(any(test, feature = "inspector"))]
            last_contact_normals: Vec::new(),
        })
    }

    pub fn pose(&self) -> Movement {
        self.pose
    }

    pub(crate) fn contact_capsule(&self) -> CapsulePose {
        self.body.capsule_pose()
    }

    #[cfg(any(test, feature = "inspector"))]
    pub(crate) fn debug_ground_probe(
        &self,
        entity: u32,
    ) -> Option<crate::game_debug::DebugGroundProbe> {
        self.last_ground_probe.map(|mut probe| {
            probe.entity = entity;
            probe
        })
    }

    #[cfg(any(test, feature = "inspector"))]
    pub(crate) fn debug_contact_normals(
        &self,
        entity: u32,
    ) -> impl Iterator<Item = crate::game_debug::DebugContactNormal> + '_ {
        self.last_contact_normals
            .iter()
            .copied()
            .map(move |mut contact| {
                contact.entity = entity;
                contact
            })
    }

    pub(crate) fn capsule(&self) -> (f64, f64) {
        if self.crouched() {
            (self.base.crouch_height, self.base.crouch_radius)
        } else {
            (self.base.stand_height, self.base.stand_radius)
        }
    }

    pub(crate) fn crouched(&self) -> bool {
        self.posture.crouched()
    }

    pub(crate) fn spring_offset(&self) -> f32 {
        self.posture.spring_offset()
    }

    pub fn track_modifiers(&mut self, frame: u32, active: &BTreeMap<ModifierKey, Modifier>) {
        self.modifiers.track(frame, active);
    }

    pub(crate) fn character_collision_for_snapshot(&self, admitted_at: u32) -> bool {
        // Room ticks and input frames can diverge. Use the next frame on the
        // capsule's own clock to include newly started or retired intervals.
        let frame = self.pose.frame.unwrap_or(admitted_at).wrapping_add(1);
        self.modifiers.at(frame).character_collision(&self.base)
    }

    pub(crate) fn set_cinematic_aim(&mut self, aim: Option<[i32; 2]>) {
        self.cinematic_aim = aim;
    }

    pub(crate) fn set_authored_facing(&mut self, facing: Option<[i32; 2]>) {
        self.authored_facing = facing;
    }

    pub(crate) fn queue_velocity_change(&mut self, frame: u32, change: VelocityChange) {
        if change.valid() {
            self.velocity_changes.push((frame, change));
        }
    }

    /// Keep the first request until its first actual pose at or after `frame`.
    pub(crate) fn queue_scripted_motion(&mut self, frame: u32, motion: ScriptedMotion) -> bool {
        if self.scripted_motion.is_some() {
            return false;
        }
        self.scripted_motion = Some((frame, motion));
        true
    }

    #[cfg(test)]
    pub(crate) fn queue_swept_displacement(
        &mut self,
        frame: u32,
        displacement: SweptDisplacement,
    ) -> bool {
        self.queue_scripted_motion(frame, ScriptedMotion::Sweep(displacement))
    }

    /// Drop script work that has not reached a pose on this body.
    pub(crate) fn clear_pending_script_motion(&mut self) {
        self.velocity_changes.clear();
        self.scripted_motion = None;
    }

    pub(crate) fn has_pending_script_motion(&self) -> bool {
        self.scripted_motion.is_some()
    }

    /// Whether this successfully completed pose moved through a script request.
    pub(crate) fn scripted_motion_at(&self, frame: u32) -> bool {
        self.scripted_motion_frame == Some(frame)
    }

    /// Simulate the newest command and any reconstructible poses before it.
    /// The callback can advance the body's statescript at each posed frame.
    pub fn step(
        &mut self,
        rays: &TriangleIndex,
        newest: Command,
        dt: f64,
        after_pose: impl FnMut(&mut Self, Command),
    ) -> Result<(), HullContactError> {
        self.step_against(rays, newest, dt, &[], after_pose)
    }

    /// Use one frozen actor-contact slice for this command's recovered poses.
    pub fn step_against(
        &mut self,
        rays: &TriangleIndex,
        newest: Command,
        dt: f64,
        blockers: &[CapsulePose],
        after_pose: impl FnMut(&mut Self, Command),
    ) -> Result<(), HullContactError> {
        let mut pending = self.pending_poses(newest);
        self.step_pending(rays, &mut pending, dt, usize::MAX, || blockers, after_pose)?;
        Ok(())
    }

    pub(crate) fn pending_poses(&self, newest: Command) -> Poses {
        poses(self.last_command, newest)
    }

    pub(crate) fn block_player_input(&mut self, blocked: bool) {
        self.player_input_blocked = blocked;
    }

    pub(crate) fn step_pending<B: AsRef<[CapsulePose]>>(
        &mut self,
        rays: &TriangleIndex,
        pending: &mut Poses,
        dt: f64,
        limit: usize,
        mut actor_capsules: impl FnMut() -> B,
        mut after_pose: impl FnMut(&mut Self, Command),
    ) -> Result<usize, HullContactError> {
        let blockers = if limit != 0 && pending.len() != 0 {
            Some(actor_capsules())
        } else {
            None
        };
        let blockers = blockers.as_ref().map_or(&[][..], AsRef::as_ref);
        let mut stepped = 0;
        for mut command in pending.by_ref().take(limit) {
            if self.player_input_blocked {
                command = Command {
                    frame: command.frame,
                    yaw: command.yaw,
                    pitch: command.pitch,
                    ..Command::default()
                };
            }
            if let Some(aim) = self.cinematic_aim.or(self.authored_facing) {
                [command.yaw, command.pitch] = aim;
            }
            self.step_pose(rays, command, dt, blockers)?;
            after_pose(self, command);
            stepped += 1;
        }
        if stepped != 0 && pending.len() == 0 {
            self.last_command = Some(pending.newest());
        }
        Ok(stepped)
    }

    fn step_pose(
        &mut self,
        rays: &TriangleIndex,
        command: Command,
        dt: f64,
        blockers: &[CapsulePose],
    ) -> Result<(), HullContactError> {
        let active = self.modifiers.at(command.frame);
        let settings = active.settings(&self.base);
        let blockers = if settings.character_collision != 0 {
            blockers
        } else {
            &[]
        };
        self.scripted_motion_frame = None;
        let mut scripted_motion_changed = false;
        if self
            .scripted_motion
            .is_some_and(|(frame, _)| frame <= command.frame)
        {
            // Consume once even if the solver rejects the attempt. Stage on a
            // fresh capsule so an error cannot separate the body from its feet.
            if let Some((_, motion)) = self.scripted_motion.take() {
                let before = self.pose.position;
                match motion {
                    ScriptedMotion::Sweep(displacement) => {
                        self.apply_swept_displacement(displacement, dt, blockers)?;
                    }
                    ScriptedMotion::Relocate(relocation) => {
                        self.apply_relocation(relocation, blockers)?;
                    }
                }
                scripted_motion_changed = self.pose.position != before;
            }
        }
        let mut launched = false;
        self.velocity_changes.retain(|&(frame, change)| {
            if frame <= command.frame {
                launched |= change.apply(&mut self.pose.velocity);
                false
            } else {
                true
            }
        });
        if launched && self.pose.velocity[1] > 0.0 {
            self.posture.begin_velocity_launch();
        }
        let mut input = MotionInput::from(&command).with_modifiers(&active);
        // The authored direction has already been packed by the server. Keep
        // its pitch, including straight up or down, when it supplies the pose.
        if let (None, Some(_), Ok(pitch)) = (
            self.cinematic_aim,
            self.authored_facing,
            i16::try_from(command.pitch),
        ) {
            input.pitch = pitch;
            input.pitch_held = false;
        }
        let probe = self.probe.probe(
            rays,
            self.body.world(),
            &settings,
            ProbePose {
                feet: self.pose.position,
                velocity: self.pose.velocity,
                spring: self.posture.spring_offset(),
                crouched: self.posture.crouched(),
                airborne: self.posture.ground() != Ground::Standing,
            },
            dt,
            self.category,
        )?;
        #[cfg(any(test, feature = "inspector"))]
        {
            let origin = self.pose.position;
            self.last_ground_probe = Some(crate::game_debug::DebugGroundProbe {
                tick: command.frame,
                entity: 0,
                origin,
                hit: probe.ground.map(|hit| [origin[0], hit.y, origin[2]]),
                normal: probe.ground.map(|hit| hit.normal),
                grounded: probe.ground.is_some(),
                non_ground_under: probe.non_ground_under,
            });
        }
        self.posture.advance(
            PostureStep {
                command: &command,
                base: &self.base,
                values: &settings,
                mods: &active,
                probe,
                dt,
                pitch_held: input.pitch_held,
            },
            &mut self.pose.position,
            &mut self.pose.velocity,
            |radius, top| self.body.try_resize(radius, top),
        )?;
        let ground = self.posture.ground();
        let velocity = VelocityStep {
            base: &self.base,
            settings: &settings,
            input,
            ground,
            crouched: self.posture.crouched(),
            dt,
        }
        .advance(self.pose.velocity);
        if ground != Ground::Standing {
            self.posture.apply_fall(settings.gravity_scale, dt);
        }
        let normal = probe.ground.map_or([0.0, 1.0, 0.0], |hit| hit.normal);
        let vertical = self.move_body(rays, velocity, normal, dt, command.frame, blockers)?;
        self.posture.finish_fall(settings.gravity_scale, vertical);

        if input.throttles != [0; 2] {
            self.last_input = Some(command.frame);
            self.pose.input_throttles = input.throttles;
        }
        self.pose.frame = Some(command.frame);
        self.pose.input_frame = match self.last_input {
            Some(frame) if frame == command.frame => None,
            frame => Some(frame.unwrap_or(NO_INPUT)),
        };
        self.pose.throttles = input.throttles;
        self.pose.yaw = input.yaw;
        self.pose.pitch = i32::from(input.pitch);
        self.pose.flags = self.posture.flags;
        self.pose.air_ticks = self.posture.air_ticks;
        self.pose.crouch_frame = self.posture.crouch_end;
        self.pose.gravity = settings.gravity_scale;
        self.pose.spring = self.posture.spring_steps().map(i32::from);
        self.pose.spring_speed = f64::from(self.posture.spring_speed());
        self.pose.fall = self.posture.fall;
        self.scripted_motion_frame = scripted_motion_changed.then_some(command.frame);
        Ok(())
    }

    fn apply_swept_displacement(
        &mut self,
        displacement: SweptDisplacement,
        dt: f64,
        actor_capsules: &[CapsulePose],
    ) -> Result<(), HullContactError> {
        let start = self.body.position();
        let mut staged = self.body.fresh_at(start);
        let end = staged
            .move_capsule_against(displacement.delta(), dt as f32, actor_capsules)?
            .position;
        let feet = std::array::from_fn(|axis| {
            self.pose.position[axis] + f64::from(end[axis]) - f64::from(start[axis])
        })
        .map(snap);
        staged.teleport(self.capsule_origin(feet));
        self.body = staged;
        self.pose.position = feet;
        Ok(())
    }

    fn apply_relocation(
        &mut self,
        relocation: Relocation,
        blockers: &[CapsulePose],
    ) -> Result<(), HullContactError> {
        let Some(feet) = relocation
            .destination_from(self.pose.position)
            .map(|feet| feet.map(snap))
        else {
            return Ok(());
        };
        let delta = std::array::from_fn(|axis| feet[axis] - self.pose.position[axis]);
        if SweptDisplacement::new(delta).is_none() {
            return Ok(());
        }
        let origin = self.capsule_origin(feet);
        if self.body.relocate_if_clear(origin, blockers)? {
            self.pose.position = feet;
        }
        Ok(())
    }

    fn capsule_origin(&self, [x, y, z]: [f64; 3]) -> [f32; 3] {
        let lift = pogo(&self.base, self.posture.crouched()) as f32 + self.posture.spring_offset();
        [x as f32, (y + f64::from(lift)) as f32, z as f32]
    }

    fn move_body(
        &mut self,
        rays: &TriangleIndex,
        velocity: [f64; 3],
        normal: [f64; 3],
        dt: f64,
        _frame: u32,
        blockers: &[CapsulePose],
    ) -> Result<f64, HullContactError> {
        let grounded = self.posture.ground() == Ground::Standing;
        let [x, y, z] = velocity;
        let velocity = if grounded && (x * x + y * y) + z * z >= EPSILON {
            along_ground([x, z], normal)
        } else {
            velocity
        };
        let [x, y, z] = velocity.map(|axis| axis as f32);
        let dt = dt as f32;
        let rise = self.posture.spring_speed();
        let displacement = [
            ((0.0 * rise + x) * dt) + 0.0,
            ((rise + y) * dt) + 0.0,
            ((0.0 * rise + z) * dt) + 0.0,
        ];
        let movement = self.body.move_capsule_against(displacement, dt, blockers)?;
        let [mut x, mut y, mut z] = clip_velocity([x, y, z], movement.planes).map(f64::from);
        if grounded {
            let squared = (x * x + y * y) + z * z;
            let flat = x * x + z * z;
            if squared >= EPSILON && flat >= EPSILON {
                let factor = (squared / flat).sqrt();
                (x, y, z) = (x * factor, 0.0, z * factor);
            }
        }
        self.posture.flags =
            (self.posture.flags & !TOUCHING) | (u32::from(movement.touching) * TOUCHING);
        let lift = pogo(&self.base, self.posture.crouched()) as f32 + self.posture.spring_offset();
        let [px, py, pz] = movement.position;
        let feet = f64::from(py - lift);
        let floor = rays.lowest() - ABYSS;
        self.pose.position = [f64::from(px), feet.max(floor), f64::from(pz)].map(snap);
        #[cfg(any(test, feature = "inspector"))]
        {
            let point = self.pose.position;
            self.last_contact_normals.clear();
            self.last_contact_normals.extend(
                movement
                    .planes
                    .iter()
                    .take(crate::game_debug::MAX_DEBUG_MOVER_NORMALS)
                    .map(|plane| crate::game_debug::DebugContactNormal {
                        tick: _frame,
                        entity: 0,
                        point,
                        normal: plane.normal.map(f64::from),
                    }),
            );
        }
        let [px, py, pz] = self.pose.position;
        self.body
            .teleport([px as f32, (py + f64::from(lift)) as f32, pz as f32]);
        if feet < floor {
            y = 0.0;
        }
        self.pose.velocity = [x, y, z].map(snap);
        Ok(y)
    }
}

fn along_ground([x, z]: [f64; 2], [nx, ny, nz]: [f64; 3]) -> [f64; 3] {
    let speed = (x * x + z * z).sqrt();
    let along = x * nx + z * nz;
    let projected = [x - nx * along, -ny * along, z - nz * along];
    let (px, pz) = (-z, x);
    let length = (px * px + pz * pz).sqrt();
    if length * length < EPSILON || speed <= 0.0 {
        return projected;
    }
    let (px, pz) = (px / length, pz / length);
    let direction = [-pz * ny, pz * nx - px * nz, px * ny];
    let [dx, dy, dz] = direction;
    let norm = (dx * dx + dy * dy + dz * dz).sqrt();
    if norm <= 0.0 {
        return projected;
    }
    let sign = if (dx * projected[0] + dy * projected[1]) + dz * projected[2] < 0.0 {
        -norm
    } else {
        norm
    };
    direction.map(|axis| axis / sign * speed)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        fs::{self, File},
        path::PathBuf,
    };

    use sha2::{Digest, Sha256};

    use super::*;
    use crate::{
        collision_cache::read_movement_cache,
        collision_scene::CollisionScene,
        commands::{CROUCH, JUMP},
        mover_modifiers::Operation,
        mover_settings::MoverCatalog,
        world::movement_record,
    };

    fn swept_fixture() -> serde_json::Value {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../fixtures/behavior/native-swept-displacement-v1.json"
        ))
        .unwrap();
        assert_eq!(fixture["schema_version"], 1);
        assert_eq!(fixture["case_id"], "native-swept-displacement.synthetic.v1");
        assert_eq!(fixture["evidence"], "synthetic-native");
        fixture
    }

    #[test]
    fn swept_displacement_waits_for_one_actual_pose_and_first_pending_request_wins() {
        let fixture = swept_fixture();
        let input = &fixture["input"];
        let due: u32 = input["due_frame"].as_u64().unwrap().try_into().unwrap();
        let delta: [f64; 3] = serde_json::from_value(input["displacement"].clone()).unwrap();
        let displacement = SweptDisplacement::new(delta).unwrap();
        let second = SweptDisplacement::new(
            serde_json::from_value(input["rejected_second_displacement"].clone()).unwrap(),
        )
        .unwrap();
        let rays = TriangleIndex::new([]).unwrap();
        let world = Arc::new(ContactWorld::new(Vec::new()).unwrap());
        let make = || {
            Mover::new(
                MoverSettings {
                    gravity_scale: 0.0,
                    ..MoverSettings::default()
                },
                Arc::clone(&world),
                [0.0; 3],
                0,
                0x18,
            )
            .unwrap()
        };
        let mut moved = make();
        let mut reference = make();
        let velocity = serde_json::from_value(input["velocity"].clone()).unwrap();
        moved.pose.velocity = velocity;
        reference.pose.velocity = velocity;
        assert!(moved.queue_swept_displacement(due, displacement));
        assert!(!moved.queue_swept_displacement(due, second));
        assert!(moved.has_pending_script_motion());
        for frame in 1..=due + 2 {
            let command = Command {
                frame,
                ..Command::default()
            };
            if frame == due {
                let mut pending = moved.pending_poses(command);
                assert_eq!(
                    moved
                        .step_pending(&rays, &mut pending, 0.016, 0, Vec::new, |_, _| {})
                        .unwrap(),
                    0
                );
                assert!(moved.has_pending_script_motion());
                assert!(!moved.scripted_motion_at(frame));
            }
            if frame == due + 1 {
                assert!(moved.queue_swept_displacement(frame, second));
                moved.clear_pending_script_motion();
                assert!(!moved.has_pending_script_motion());
                assert!(moved.scripted_motion_at(due));
                assert!(!moved.scripted_motion_at(frame));
            }
            moved.step(&rays, command, 0.016, |_, _| {}).unwrap();
            reference.step(&rays, command, 0.016, |_, _| {}).unwrap();
            let expected = if frame >= due { delta } else { [0.0; 3] };
            assert_eq!(
                moved.pose.position,
                std::array::from_fn(|axis| reference.pose.position[axis] + expected[axis])
            );
            assert_eq!(moved.pose.velocity, reference.pose.velocity);
            assert_eq!(moved.pose.flags, reference.pose.flags);
            assert_eq!(moved.pose.frame, reference.pose.frame);
            assert_eq!(moved.has_pending_script_motion(), frame < due);
            assert_eq!(moved.scripted_motion_at(frame), frame == due);
        }
    }

    #[test]
    fn swept_displacement_marker_excludes_rounded_and_canceled_requests() {
        let rays = TriangleIndex::new([]).unwrap();
        let mut mover = Mover::new(
            MoverSettings {
                gravity_scale: 0.0,
                ..MoverSettings::default()
            },
            Arc::new(ContactWorld::new(Vec::new()).unwrap()),
            [0.0; 3],
            0,
            0x18,
        )
        .unwrap();
        let tiny = SweptDisplacement::new([0.000_001, 0.0, 0.0]).unwrap();
        assert!(mover.queue_swept_displacement(1, tiny));
        mover
            .step(
                &rays,
                Command {
                    frame: 1,
                    ..Command::default()
                },
                0.016,
                |_, _| {},
            )
            .unwrap();
        assert_eq!(mover.pose.position[0], 0.0);
        assert!(!mover.has_pending_script_motion());
        assert!(!mover.scripted_motion_at(1));
        assert!(
            mover.queue_swept_displacement(2, SweptDisplacement::new([2.0, 0.0, 0.0]).unwrap())
        );
        mover.clear_pending_script_motion();
        mover
            .step(
                &rays,
                Command {
                    frame: 2,
                    ..Command::default()
                },
                0.016,
                |_, _| {},
            )
            .unwrap();
        assert_eq!(mover.pose.position[0], 0.0);
        assert!(!mover.has_pending_script_motion());
        assert!(!mover.scripted_motion_at(2));
    }

    #[test]
    fn posed_batches_sample_actor_capsules_without_empty_or_zero_budget_work() {
        let fixture = swept_fixture();
        let input = &fixture["input"];
        let due: u32 = input["due_frame"].as_u64().unwrap().try_into().unwrap();
        let settings = MoverSettings {
            gravity_scale: 0.0,
            stand_height: input["stand_height"].as_f64().unwrap(),
            stand_radius: input["stand_radius"].as_f64().unwrap(),
            ..MoverSettings::default()
        };
        let blocker = crate::mover_ground::standing_capsule(
            &settings,
            serde_json::from_value(input["actor_feet"].clone()).unwrap(),
        );
        let displacement = SweptDisplacement::new(
            serde_json::from_value(input["wall_displacement"].clone()).unwrap(),
        )
        .unwrap();
        let rays = TriangleIndex::new([]).unwrap();
        let mut mover = Mover::new(
            settings,
            Arc::new(ContactWorld::new(Vec::new()).unwrap()),
            [0.0; 3],
            0,
            0x18,
        )
        .unwrap();
        assert!(mover.queue_swept_displacement(due, displacement));
        let mut samples = 0;
        for frame in 1..=due + 1 {
            let command = Command {
                frame,
                ..Command::default()
            };
            let mut pending = mover.pending_poses(command);
            if frame == due {
                mover
                    .step_pending(
                        &rays,
                        &mut pending,
                        0.016,
                        0,
                        || {
                            samples += 1;
                            vec![blocker]
                        },
                        |_, _| {},
                    )
                    .unwrap();
                assert_eq!(samples, frame - 1);
            }
            mover
                .step_pending(
                    &rays,
                    &mut pending,
                    0.016,
                    usize::MAX,
                    || {
                        samples += 1;
                        vec![blocker]
                    },
                    |_, _| {},
                )
                .unwrap();
            assert_eq!(samples, frame);
            assert_eq!(mover.scripted_motion_at(frame), frame == due);
        }
        assert!(mover.pose.position[2] > fixture["expected"]["wall_min_z"].as_f64().unwrap());
        assert!(mover.pose.position[2] < fixture["expected"]["wall_max_z"].as_f64().unwrap());
        assert_eq!(mover.contact_capsule(), mover.body.capsule_pose());
        assert!(!mover.has_pending_script_motion());
        let before = mover.pose.position;
        assert!(mover.queue_swept_displacement(due + 2, displacement));
        mover.clear_pending_script_motion();
        let mut pending = mover.pending_poses(Command {
            frame: due + 2,
            ..Command::default()
        });
        mover
            .step_pending(
                &rays,
                &mut pending,
                0.016,
                usize::MAX,
                || {
                    samples += 1;
                    vec![blocker]
                },
                |_, _| {},
            )
            .unwrap();
        assert_eq!(mover.pose.position, before);
        assert_eq!(samples, due + 2);
        mover
            .step_pending(
                &rays,
                &mut pending,
                0.016,
                usize::MAX,
                || -> Vec<CapsulePose> { panic!("exhausted poses must not sample actor capsules") },
                |_, _| {},
            )
            .unwrap();
    }

    #[test]
    fn regular_momentum_uses_one_actor_snapshot_for_all_recovered_poses() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../fixtures/behavior/native-regular-actor-contact-v1.json"
        ))
        .unwrap();
        assert_eq!(fixture["schema_version"], 1);
        assert_eq!(
            fixture["case_id"],
            "native-regular-actor-contact.synthetic.v1"
        );
        assert_eq!(fixture["evidence"], "synthetic-native");
        let input = &fixture["input"];
        let warmup_frame: u32 = input["warmup_frame"].as_u64().unwrap().try_into().unwrap();
        let final_frame: u32 = input["final_frame"].as_u64().unwrap().try_into().unwrap();
        let dt = input["dt"].as_f64().unwrap();
        let settings = MoverSettings {
            gravity_scale: 0.0,
            decel_air_per_speed: 0.0,
            stand_height: input["stand_height"].as_f64().unwrap(),
            stand_radius: input["stand_radius"].as_f64().unwrap(),
            ..MoverSettings::default()
        };
        let blocker = crate::mover_ground::standing_capsule(
            &settings,
            serde_json::from_value(input["actor_feet"].clone()).unwrap(),
        );
        let rays = TriangleIndex::new([]).unwrap();
        let world = Arc::new(ContactWorld::new(Vec::new()).unwrap());
        let make = || Mover::new(settings, Arc::clone(&world), [0.0; 3], 0, 0x18).unwrap();
        let mut blocked = make();
        let mut free = make();
        for mover in [&mut blocked, &mut free] {
            mover
                .step(
                    &rays,
                    Command {
                        frame: warmup_frame,
                        ..Command::default()
                    },
                    dt,
                    |_, _| {},
                )
                .unwrap();
            mover.pose.velocity = serde_json::from_value(input["velocity"].clone()).unwrap();
        }
        let command = Command {
            frame: final_frame,
            ..Command::default()
        };
        let mut pending = blocked.pending_poses(command);
        assert_eq!(
            pending.len(),
            fixture["expected"]["posed_frames"].as_u64().unwrap() as usize
        );
        blocked
            .step_pending(
                &rays,
                &mut pending,
                dt,
                0,
                || -> Vec<CapsulePose> { panic!("zero pose budget must not sample actors") },
                |_, _| {},
            )
            .unwrap();
        let mut samples = 0;
        let mut posed = 0;
        blocked
            .step_pending(
                &rays,
                &mut pending,
                dt,
                usize::MAX,
                || {
                    samples += 1;
                    vec![blocker]
                },
                |mover, command| {
                    posed += 1;
                    assert!(
                        mover.pose.position[2] < fixture["expected"]["maximum_z"].as_f64().unwrap()
                    );
                    assert!(!mover.scripted_motion_at(command.frame));
                },
            )
            .unwrap();
        free.step(&rays, command, dt, |_, _| {}).unwrap();
        assert_eq!(
            samples,
            fixture["expected"]["sample_count"].as_u64().unwrap()
        );
        assert_eq!(posed, fixture["expected"]["posed_frames"].as_u64().unwrap());
        assert!(blocked.pose.position[2] > fixture["expected"]["minimum_z"].as_f64().unwrap());
        assert!(free.pose.position[2] > fixture["expected"]["maximum_z"].as_f64().unwrap());
        assert!(!blocked.has_pending_script_motion());
    }

    fn swept_wall_world(input: &serde_json::Value, category: u16) -> Arc<ContactWorld> {
        use crate::{
            collision_cache::{Mesh, Triangle},
            contact_mesh::ContactMesh,
            contact_world::{StaticGeometry, StaticShape},
        };

        let wall = Mesh {
            vertices: serde_json::from_value(input["wall_vertices"].clone()).unwrap(),
            triangles: input["wall_triangles"]
                .as_array()
                .unwrap()
                .iter()
                .map(|corners| Triangle {
                    corners: serde_json::from_value(corners.clone()).unwrap(),
                    wings: [-1; 3],
                    mask: category,
                    surface: 0,
                })
                .collect(),
        };
        let mesh = ContactMesh::new(wall, [1.0; 3], u16::MAX).unwrap();
        let shape = StaticShape::new(StaticGeometry::Mesh(Arc::new(mesh)), None).unwrap();
        Arc::new(ContactWorld::new(vec![shape]).unwrap())
    }

    #[test]
    fn relocation_shares_deadline_and_marks_only_a_clear_destination() {
        let fixture = swept_fixture();
        let input = &fixture["input"];
        let category = u16::try_from(input["category"].as_u64().unwrap()).unwrap();
        let settings = MoverSettings {
            gravity_scale: 0.0,
            stand_height: input["stand_height"].as_f64().unwrap(),
            stand_radius: input["stand_radius"].as_f64().unwrap(),
            ..MoverSettings::default()
        };
        let mut mover = Mover::new(
            settings,
            swept_wall_world(input, category),
            [0.0; 3],
            0,
            category,
        )
        .unwrap();
        let destination = serde_json::from_value(input["wall_displacement"].clone()).unwrap();
        let blocker = crate::mover_ground::standing_capsule(&settings, destination);
        let motion = ScriptedMotion::Relocate(Relocation::new(destination).unwrap());
        let rays = TriangleIndex::new([]).unwrap();
        assert!(mover.queue_scripted_motion(2, motion));
        assert!(!mover.queue_swept_displacement(2, SweptDisplacement::new(destination).unwrap()));
        for frame in 1..=4 {
            if frame == 3 {
                assert!(mover.queue_scripted_motion(frame, motion));
            }
            let blockers = if frame < 3 {
                std::slice::from_ref(&blocker)
            } else {
                &[]
            };
            mover
                .step_against(
                    &rays,
                    Command {
                        frame,
                        ..Command::default()
                    },
                    0.016,
                    blockers,
                    |_, _| {},
                )
                .unwrap();
            assert_eq!(mover.has_pending_script_motion(), frame == 1);
            assert_eq!(mover.scripted_motion_at(frame), frame == 3);
            assert_eq!(
                mover.pose.position,
                if frame < 3 { [0.0; 3] } else { destination }
            );
        }
    }

    #[test]
    fn effective_character_collision_controls_sweeps_and_regular_motion() {
        let fixture = swept_fixture();
        let input = &fixture["input"];
        let category: u16 = input["category"].as_u64().unwrap().try_into().unwrap();
        let settings = MoverSettings {
            gravity_scale: 0.0,
            decel_air_base: 0.0,
            decel_air_per_speed: 0.0,
            stand_height: input["stand_height"].as_f64().unwrap(),
            stand_radius: input["stand_radius"].as_f64().unwrap(),
            ..MoverSettings::default()
        };
        let world = swept_wall_world(input, category);
        let rays = TriangleIndex::new([]).unwrap();
        let wall_z = fixture["expected"]["wall_max_z"].as_f64().unwrap();
        let actor_z = wall_z / 2.0;
        let blocker = crate::mover_ground::standing_capsule(&settings, [0.0, 0.0, actor_z]);
        let delta = [0.0, 0.0, wall_z * 2.0];
        let dt = 0.25;
        let collision_disabled = BTreeMap::from([(
            ModifierKey {
                instance: 1,
                state: 1,
                slot: 0,
            },
            Modifier {
                index: crate::mover_modifiers::COLLISION,
                operation: Operation::Set,
                value: 0.0,
                priority: 0.0,
            },
        )]);

        for swept in [false, true] {
            let mut mover =
                Mover::new(settings, Arc::clone(&world), [0.0; 3], 0, category).unwrap();
            let start = mover.body.position();
            for (frame, collides) in [(1, true), (2, false), (3, true)] {
                // Reset position to exercise each interval against the same
                // blockers while keeping the real command and modifier clocks.
                mover.pose.position = [0.0; 3];
                mover.body = mover.body.fresh_at(start);
                mover.pose.velocity = [0.0; 3];
                if swept {
                    assert!(
                        mover.queue_swept_displacement(
                            frame,
                            SweptDisplacement::new(delta).unwrap()
                        )
                    );
                } else {
                    mover.queue_velocity_change(
                        frame,
                        VelocityChange::Delta(delta.map(|axis| axis / dt)),
                    );
                }
                mover
                    .step_against(
                        &rays,
                        Command {
                            frame,
                            ..Command::default()
                        },
                        dt,
                        &[blocker],
                        |_, _| {},
                    )
                    .unwrap();
                let z = mover.pose.position[2];
                let range = if collides {
                    0.0..actor_z
                } else {
                    actor_z..wall_z
                };
                assert!(
                    range.contains(&z) && z > range.start,
                    "frame {frame}, swept {swept}, z {z}"
                );
                assert_eq!(mover.scripted_motion_at(frame), swept);
                if frame == 1 {
                    mover.track_modifiers(frame, &collision_disabled);
                } else if frame == 2 {
                    mover.track_modifiers(frame, &BTreeMap::new());
                }
            }
        }
    }

    #[test]
    fn swept_displacement_uses_the_existing_capsule_without_clipping_velocity() {
        let fixture = swept_fixture();
        let input = &fixture["input"];
        let category: u16 = input["category"].as_u64().unwrap().try_into().unwrap();
        let mut mover = Mover::new(
            MoverSettings {
                stand_height: input["stand_height"].as_f64().unwrap(),
                stand_radius: input["stand_radius"].as_f64().unwrap(),
                ..MoverSettings::default()
            },
            swept_wall_world(input, category),
            [0.0; 3],
            8192,
            category,
        )
        .unwrap();
        mover.pose.velocity = serde_json::from_value(input["velocity"].clone()).unwrap();
        mover.pose.pitch = -2048;
        let before = mover.pose();
        let displacement = SweptDisplacement::new(
            serde_json::from_value(input["wall_displacement"].clone()).unwrap(),
        )
        .unwrap();
        mover
            .apply_swept_displacement(displacement, 0.016, &[])
            .unwrap();
        let after = mover.pose();
        assert!(after.position[2] > fixture["expected"]["wall_min_z"].as_f64().unwrap());
        assert!(after.position[2] < fixture["expected"]["wall_max_z"].as_f64().unwrap());
        assert_eq!(after.velocity, before.velocity);
        assert_eq!(
            (after.yaw, after.pitch, after.flags),
            (before.yaw, before.pitch, before.flags)
        );
        let lift =
            pogo(&mover.base, mover.posture.crouched()) as f32 + mover.posture.spring_offset();
        assert_eq!(
            mover.body.position(),
            [
                after.position[0] as f32,
                (after.position[1] + f64::from(lift)) as f32,
                after.position[2] as f32,
            ]
        );
        mover
            .apply_swept_displacement(displacement, 0.016, &[])
            .unwrap();
        let tolerance = fixture["expected"]["repeated_block_tolerance"]
            .as_f64()
            .unwrap();
        assert!((mover.pose.position[2] - after.position[2]).abs() <= tolerance);
        assert_eq!(mover.pose.velocity, before.velocity);
    }

    #[test]
    fn velocity_changes_are_ordered_once_and_wait_for_their_pose() {
        let rays = TriangleIndex::new([]).unwrap();
        let mut mover = Mover::new(
            MoverSettings::default(),
            Arc::new(ContactWorld::new(Vec::new()).unwrap()),
            [0.0; 3],
            0,
            0x18,
        )
        .unwrap();
        let mut reference = Mover::new(
            MoverSettings::default(),
            Arc::new(ContactWorld::new(Vec::new()).unwrap()),
            [0.0; 3],
            0,
            0x18,
        )
        .unwrap();
        for frame in 1..=4 {
            let command = Command {
                frame,
                ..Default::default()
            };
            if frame == 3 {
                reference.pose.velocity[0] = 3.0;
                reference.pose.velocity[2] = 4.0;
            }
            mover
                .step(&rays, command, 0.016, |mover, command| {
                    if command.frame == 1 {
                        mover.queue_velocity_change(3, VelocityChange::Delta([6.0, 0.0, 8.0]));
                        mover.queue_velocity_change(3, VelocityChange::HorizontalCap(5.0));
                        mover.queue_velocity_change(3, VelocityChange::Delta([f64::NAN, 0.0, 0.0]));
                    }
                })
                .unwrap();
            reference.step(&rays, command, 0.016, |_, _| {}).unwrap();
            assert_eq!(mover.pose(), reference.pose());
        }
        assert!(mover.velocity_changes.is_empty());
        let mut velocity = [6.0, 7.0, 8.0];
        VelocityChange::HorizontalCap(5.0).apply(&mut velocity);
        assert_eq!(velocity, [3.0, 7.0, 4.0]);
        assert!(!VelocityChange::HorizontalCap(-1.0).valid());
    }

    #[test]
    fn vertical_impulse_leaves_supported_ground_at_the_due_pose() {
        use crate::collision_index::SceneTriangle;

        let contract: serde_json::Value = serde_json::from_str(include_str!(
            "../../fixtures/behavior/script-vertical-velocity-impulse-v1.json"
        ))
        .unwrap();
        let input = &contract["input"];
        let expected = &contract["expected"];
        let change = VelocityChange::VerticalImpulse {
            vertical: input["vertical_reset"].as_f64().unwrap(),
            delta: serde_json::from_value(input["delta"].clone()).unwrap(),
        };
        let mut velocity: [f64; 3] =
            serde_json::from_value(input["prior_velocity"].clone()).unwrap();
        let before_physics: [f64; 3] =
            serde_json::from_value(expected["velocity_before_ground_and_gravity"].clone()).unwrap();
        assert!(change.valid());
        assert!(change.apply(&mut velocity));
        assert_eq!(velocity, before_physics);

        let extreme = f64::from(f32::MAX);
        let overflow = VelocityChange::VerticalImpulse {
            vertical: 2.0,
            delta: [extreme, 8.0, 1.0],
        };
        velocity = [extreme, -4.0, -1.5];
        assert!(overflow.valid());
        assert!(!overflow.apply(&mut velocity));
        assert_eq!(velocity, [extreme, -4.0, -1.5]);

        let rays = TriangleIndex::new([SceneTriangle {
            vertices: [[-10.0, 0.0, -10.0], [0.0, 0.0, 20.0], [10.0, 0.0, -10.0]],
            mask: 0x18,
            surface: 0,
        }])
        .unwrap();
        let mut mover = Mover::new(
            MoverSettings::default(),
            Arc::new(ContactWorld::new(Vec::new()).unwrap()),
            [0.0; 3],
            0,
            0x18,
        )
        .unwrap();
        let source = input["frame"].as_u64().unwrap() as u32;
        let due = expected["apply_frame"].as_u64().unwrap() as u32;
        mover.queue_velocity_change(due, change);
        let mut step = |frame| {
            mover
                .step(
                    &rays,
                    Command {
                        frame,
                        ..Default::default()
                    },
                    0.016,
                    |_, _| {},
                )
                .unwrap();
            mover.pose()
        };
        let grounded = step(source);
        assert_eq!(grounded.flags & IN_AIR, 0);
        let lifted = step(due);
        assert_ne!(lifted.flags & IN_AIR, 0);
        assert!(lifted.position[1] > grounded.position[1]);
        assert!(lifted.velocity[1] > 0.0);
        let next = step(due + 1);
        assert!(next.position[1] > lifted.position[1]);
        assert!(next.velocity[1] < lifted.velocity[1]);
        assert!(mover.velocity_changes.is_empty());
    }

    #[test]
    fn directional_modifier_changes_live_motion_without_changing_input_records() {
        let rays = TriangleIndex::new([]).unwrap();
        let world = Arc::new(ContactWorld::new(Vec::new()).unwrap());
        let make_mover = || {
            Mover::new(
                MoverSettings::default(),
                Arc::clone(&world),
                [0.0; 3],
                0,
                0x18,
            )
            .unwrap()
        };
        let mut modified = make_mover();
        let mut reference = make_mover();
        modified.track_modifiers(
            0,
            &BTreeMap::from([(
                ModifierKey {
                    instance: 1,
                    state: 1,
                    slot: 0,
                },
                Modifier {
                    index: 17,
                    operation: Operation::Multiply,
                    value: 0.5,
                    priority: 0.0,
                },
            )]),
        );
        for frame in 1..=20 {
            let mut command = Command {
                frame,
                forward: 126,
                ..Command::default()
            };
            modified.step(&rays, command, 0.016, |_, _| {}).unwrap();
            command.forward = 63;
            reference.step(&rays, command, 0.016, |_, _| {}).unwrap();
            assert_eq!(modified.pose().position, reference.pose().position);
            assert_eq!(modified.pose().velocity, reference.pose().velocity);
            assert_eq!(modified.pose().throttles, [0, 126]);
            assert_eq!(modified.pose().input_throttles, [0, 126]);
        }
        assert!(modified.pose().position[2] > 0.0);
    }

    #[test]
    fn owner_movement_sample_covers_fresh_and_live_poses() {
        let mut pose = Movement::default();
        assert_eq!(sample_movement_flags(&pose), Some(1 << 1));

        pose.frame = Some(20);
        assert_eq!(sample_movement_flags(&pose), Some(1 << 1));
        pose.flags = CROUCHED | NOT_GROUND_UNDER;
        assert_eq!(
            sample_movement_flags(&pose),
            Some((1 << 1) | (1 << 6) | (1 << 9))
        );
        pose.flags = JUMPED;
        pose.velocity = [0.0, 2.0, 0.0];
        pose.throttles = [0, -127];
        assert_eq!(
            sample_movement_flags(&pose),
            Some(1 | (1 << 2) | (1 << 4) | (1 << 5) | (1 << 11))
        );
        pose.flags = 0x800000;
        pose.velocity[1] = -2.0;
        pose.throttles = [0; 2];
        assert_eq!(sample_movement_flags(&pose), Some(1 | (1 << 3) | (1 << 4)));
        pose.flags |= 0x80;
        assert_eq!(sample_movement_flags(&pose), None);
    }

    #[test]
    fn ground_probe_and_contact_normals_are_recorded_read_only() {
        use crate::{
            collision_cache::{Mesh, Triangle},
            collision_index::SceneTriangle,
            contact_mesh::ContactMesh,
            contact_world::{StaticGeometry, StaticShape},
        };

        let floor_tris = [
            SceneTriangle {
                vertices: [[-10.0, 0.0, -10.0], [-10.0, 0.0, 10.0], [10.0, 0.0, 10.0]],
                mask: 0x18,
                surface: 0,
            },
            SceneTriangle {
                vertices: [[10.0, 0.0, 10.0], [10.0, 0.0, -10.0], [-10.0, 0.0, -10.0]],
                mask: 0x18,
                surface: 0,
            },
        ];
        let rays = TriangleIndex::new(floor_tris).unwrap();
        let wall = Mesh {
            vertices: vec![
                [-5.0, 0.0, 1.0],
                [-5.0, 4.0, 1.0],
                [5.0, 0.0, 1.0],
                [5.0, 4.0, 1.0],
            ],
            triangles: vec![
                Triangle {
                    corners: [0, 1, 2],
                    wings: [-1, 3, -1],
                    mask: 0x18,
                    surface: 0,
                },
                Triangle {
                    corners: [3, 2, 1],
                    wings: [-1, 0, -1],
                    mask: 0x18,
                    surface: 0,
                },
            ],
        };
        let mesh = ContactMesh::new(wall, [1.0; 3], u16::MAX).unwrap();
        let shape = StaticShape::new(StaticGeometry::Mesh(Arc::new(mesh)), None).unwrap();
        let world = Arc::new(ContactWorld::new(vec![shape]).unwrap());
        let mut mover =
            Mover::new(MoverSettings::default(), world, [0.0, 0.0, 0.0], 0, 0x18).unwrap();
        assert!(mover.debug_ground_probe(42).is_none());

        let mut hit_wall = false;
        let mut last_frame = 0;
        for frame in 1..=30 {
            last_frame = frame;
            let command = Command {
                frame,
                forward: 127,
                ..Command::default()
            };
            mover.step(&rays, command, 0.016, |_, _| {}).unwrap();
            let probe = mover.debug_ground_probe(42).unwrap();
            assert_eq!(probe.entity, 42);
            assert_eq!(probe.tick, frame);
            assert!(probe.grounded);
            assert_eq!(probe.normal, Some([0.0, 1.0, 0.0]));
            let normals: Vec<_> = mover.debug_contact_normals(42).collect();
            assert!(normals.len() <= crate::game_debug::MAX_DEBUG_MOVER_NORMALS);
            if normals.iter().any(|contact| {
                contact.entity == 42 && contact.tick == frame && contact.normal[2] < -0.5
            }) {
                hit_wall = true;
            }
        }
        assert!(hit_wall, "expected wall contact normal against Z=1 wall");
        assert!(last_frame == 30);
    }

    #[test]
    #[ignore = "requires a prepared 104319 Practice Range movement cache"]
    fn practice_range_player_movement_records_match_python() {
        let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
        let cache =
            read_movement_cache(File::open(runtime.join("collision/0688.col")).unwrap()).unwrap();
        let ids = BTreeSet::new();
        let scene = CollisionScene::movement_probe(&cache, &ids).unwrap();
        let world = Arc::new(ContactWorld::from_movement_cache(cache, &ids).unwrap());
        let catalog_path = runtime.join("data/game_movers_174.json");
        let settings = MoverCatalog::load(&catalog_path).unwrap();
        let records: BTreeMap<String, serde_json::Value> =
            serde_json::from_slice(&fs::read(catalog_path).unwrap()).unwrap();
        let expected = match records.len() {
            // Combined Python cc620be includes normalized spring bounds.
            38 => "1db24f3d6b8bbda247fcf7db88134d4ad88c5b27b05453d3f7a777f97c4ee9fd",
            43 => "289ba18c0a1af9e621c4cc72d5e51a8acf45d1fb7a5806af7e866313c26c9e8e",
            count => panic!("no Python reference vector for the {count}-mover replay"),
        };
        let sprint = BTreeMap::from([(
            ModifierKey {
                instance: 1,
                state: 1,
                slot: 0,
            },
            Modifier {
                index: 0,
                operation: Operation::Multiply,
                value: 1.5,
                priority: 0.0,
            },
        )]);
        let released = BTreeMap::new();
        let mut combined = Sha256::new();
        for (i, guid) in records.keys().enumerate() {
            let body = crate::accounts::parse_guid(guid).unwrap();
            let spawn = if i % 3 == 2 {
                [45.5, 2.5, 34.75]
            } else {
                [54.56507, 1.0, 42.14788]
            };
            let category = [0x08, 0x10, 0x18][i % 3];
            let mut mover = Mover::new(
                *settings.for_body(Some(body)),
                Arc::clone(&world),
                spawn,
                -31010,
                category,
            )
            .unwrap();
            let first = 4000 + i as u32 * 500;
            let mut digest = Sha256::new();
            let mut visited = 0;
            for n in 0..304 {
                if (20..24).contains(&n) || (80..118).contains(&n) || (210..278).contains(&n) {
                    continue;
                }
                let jump = (24..29).contains(&n) || (138..153).contains(&n);
                let crouch = (45..75).contains(&n) || (121..136).contains(&n);
                let command = Command {
                    frame: first + n,
                    forward: [127, -127, 58, 0][(n as usize / 16) % 4],
                    right: [0, 0, -127, 111][(n as usize / 20) % 4],
                    yaw: (n as i32 / 12) * 4096 - 31010,
                    pitch: (n as i32 % 28) * 2500 - 32000,
                    buttons: (u32::from(jump) * JUMP) | (u32::from(crouch) * CROUCH),
                    ..Command::default()
                };
                mover
                    .step(scene.triangles(), command, 0.016, |mover, pose| {
                        visited += 1;
                        let record = movement_record(&mover.pose(), Some(pose.frame + 2)).unwrap();
                        digest.update((record.bit_len() as u32).to_le_bytes());
                        digest.update(record.as_bytes());
                        let active = if (12..35).contains(&((pose.frame - first) % 72)) {
                            &sprint
                        } else {
                            &released
                        };
                        mover.track_modifiers(pose.frame, active);
                    })
                    .unwrap();
            }
            assert_eq!(visited, 236);
            combined.update(body.to_le_bytes());
            combined.update(digest.finalize());
        }
        assert_eq!(format!("{:x}", combined.finalize()), expected);
    }
}
