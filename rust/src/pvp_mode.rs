//! Data-driven PvP objective, custom mode rule, placeable filtering, and match audio engine
//! composed from the five generic `room_primitives`.

use std::collections::{BTreeMap, BTreeSet};

use crate::room_primitives::{
    ActorSample, BarrierKind, DoorState, DynamicBarrierRegistry, MAP_ENTITY_BIT, ObbZone,
    SPECTATOR_TEAM, SplineMover, SplineSpeedCurve, SplineTrack, VoiceCue, rotate_vec,
};

pub const FIRST_DEFENDING_TEAM: u8 = 0;
pub const FIRST_ATTACKING_TEAM: u8 = 1;

pub const MODE_ESCORT: u64 = 0x0230_0000_0000_0015;
pub const MODE_ASSAULT: u64 = 0x0230_0000_0000_0010;
pub const MODE_HYBRID: u64 = 0x0230_0000_0000_0016;
pub const MODE_CONTROL: u64 = 0x0230_0000_0000_0014;

pub const SETTING_START_MODE: u32 = 9672;
pub const START_IMMEDIATELY: u32 = 9673;
pub const START_WHEN_FULL: u32 = 9674;
pub const START_MANUALLY: u32 = 9675;

pub const SETTING_CAPTURE_SPEED: u32 = 9518;
pub const SETTING_CONTROL_SCORE_SPEED: u32 = 9519;
pub const SETTING_PAYLOAD_SPEED: u32 = 9520;
pub const SETTING_CONTROL_SCORE_TO_WIN: u32 = 8853;
pub const SETTING_CONTROL_VALID_POINTS: u32 = 9545;

pub const SETUP_SECONDS: f64 = 45.0;
pub const CONTROL_SETUP_SECONDS: f64 = 30.0;
pub const CONTROL_UNLOCK_SECONDS: f64 = 30.0;
pub const CONTROL_ROUND_SUMMARY_DELAY: f64 = 7.0;
pub const CONTROL_INTERMISSION_SECONDS: f64 = 8.0;
pub const CONTROL_ROUND_END_MOVEMENT_SCALE: f64 = 0.375;
pub const OVERTIME_SECONDS: f64 = 5.0;
pub const FINISH_WARNING_DISTANCE: f64 = 15.0;

// ---------------------------------------------------------------------------
// 1. Custom Mode Rules
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingRow {
    pub id: u32,
    pub value_type: u32,
    pub raw: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ControlRuleSettings {
    pub capture_scale_bits: Option<u32>,
    pub score_scale_bits: Option<u32>,
    pub fixed_stage: Option<usize>,
    pub points_to_win: Option<u32>,
}

impl ControlRuleSettings {
    pub fn capture_scale(&self) -> f64 {
        self.capture_scale_bits
            .map_or(1.0, |b| f64::from(f32::from_bits(b)))
    }
    pub fn score_scale(&self) -> f64 {
        self.score_scale_bits
            .map_or(1.0, |b| f64::from(f32::from_bits(b)))
    }
}

#[derive(Clone, Debug, Default)]
pub struct CustomModeRules {
    pub map_overrides: Vec<SettingRow>,
    pub mode_overrides: Vec<SettingRow>,
}

impl CustomModeRules {
    fn rows(&self) -> impl Iterator<Item = &SettingRow> {
        self.map_overrides.iter().chain(self.mode_overrides.iter())
    }

    pub fn start_mode(&self) -> u32 {
        for row in self.rows() {
            if row.id == SETTING_START_MODE
                && row.value_type == 3
                && matches!(
                    row.raw,
                    START_IMMEDIATELY | START_WHEN_FULL | START_MANUALLY
                )
            {
                return row.raw;
            }
        }
        START_WHEN_FULL
    }

    pub fn objective_rate(&self, key: u32) -> f64 {
        for row in self.rows() {
            if row.id == key && matches!(row.value_type, 1 | 2) {
                let rate = f32::from_bits(row.raw);
                if (0.1..=5.0).contains(&rate) {
                    return f64::from(rate);
                }
            }
        }
        1.0
    }

    pub fn control_settings(&self) -> ControlRuleSettings {
        let mut out = ControlRuleSettings::default();
        for row in self.rows() {
            match row.id {
                SETTING_CAPTURE_SPEED if matches!(row.value_type, 1 | 2) => {
                    let rate = f32::from_bits(row.raw);
                    if (0.1..=5.0).contains(&rate) && out.capture_scale_bits.is_none() {
                        out.capture_scale_bits = Some(row.raw);
                    }
                }
                SETTING_CONTROL_SCORE_SPEED if matches!(row.value_type, 1 | 2) => {
                    let rate = f32::from_bits(row.raw);
                    if (0.1..=5.0).contains(&rate) && out.score_scale_bits.is_none() {
                        out.score_scale_bits = Some(row.raw);
                    }
                }
                SETTING_CONTROL_VALID_POINTS
                    if row.value_type == 3 && out.fixed_stage.is_none() =>
                {
                    out.fixed_stage = match row.raw {
                        0x2577 => Some(0),
                        0x2578 => Some(1),
                        0x2579 => Some(2),
                        _ => None,
                    };
                }
                SETTING_CONTROL_SCORE_TO_WIN
                    if row.value_type == 1
                        && (1..=3).contains(&row.raw)
                        && out.points_to_win.is_none() =>
                {
                    out.points_to_win = Some(row.raw);
                }
                _ => {}
            }
        }
        out
    }

    pub fn can_manual_start(custom_start: Option<u32>, already_assembled: bool) -> bool {
        !already_assembled && matches!(custom_start, Some(START_WHEN_FULL | START_MANUALLY))
    }

    pub const fn spectators_fill_playing_slots() -> bool {
        false
    }

    pub const fn practice_start_automatically() -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// 2. Objective Placeables & Panel Fitting
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct ModePlaceablesCatalog {
    pub base_placeables: Vec<u32>,
    pub placeables_by_mode: BTreeMap<u64, Vec<u32>>,
    pub mirrored: BTreeSet<u32>,
    pub objective_placeables: Vec<u32>,
    pub health_pack_placeables: Vec<u32>,
    pub scene_placeables: Vec<u32>,
}

impl ModePlaceablesCatalog {
    pub fn resolve(&self, mode: u64, include_mode_extras: bool) -> Vec<u32> {
        let mut out: BTreeSet<u32> = self
            .placeables_by_mode
            .get(&mode)
            .unwrap_or(&self.base_placeables)
            .iter()
            .copied()
            .collect();
        if include_mode_extras {
            for &p in &self.objective_placeables {
                if self.mirrored.contains(&p) {
                    out.insert(p);
                }
            }
            for &p in &self.health_pack_placeables {
                if self.mirrored.contains(&p) {
                    out.insert(p);
                }
            }
            for &p in &self.scene_placeables {
                if self.mirrored.contains(&p) {
                    out.insert(p);
                }
            }
        }
        out.into_iter().collect()
    }
}

pub fn active_spawn_candidates(spawns: &[(u8, Option<u32>)]) -> [Vec<u32>; 2] {
    let mut out: [Vec<u32>; 2] = [Vec::new(), Vec::new()];
    for &(team, placeable) in spawns {
        if let (0 | 1, Some(p)) = (team, placeable) {
            out[team as usize].push(MAP_ENTITY_BIT | p);
        }
    }
    out
}

pub fn fit_control_room_panel(
    panel_pos: [f64; 3],
    panel_scale: [f64; 3],
    rim_pos: [f64; 3],
    rim_rot: [f64; 4],
    rim_scale: [f64; 3],
    rim_size: [f64; 2],
    panel_base_size: f64,
) -> ([f32; 3], [f32; 3]) {
    let normal = rotate_vec(rim_rot, [0.0, 0.0, 1.0]);
    let delta = [
        panel_pos[0] - rim_pos[0],
        panel_pos[1] - rim_pos[1],
        panel_pos[2] - rim_pos[2],
    ];
    let dot = normal[0] * delta[0] + normal[1] * delta[1] + normal[2] * delta[2];
    let depth = dot.min(-0.02);
    let pos = [
        (rim_pos[0] + normal[0] * depth) as f32,
        (rim_pos[1] + normal[1] * depth) as f32,
        (rim_pos[2] + normal[2] * depth) as f32,
    ];
    let scale = [
        (rim_size[0] * rim_scale[0] / panel_base_size) as f32,
        (rim_size[1] * rim_scale[1] / panel_base_size) as f32,
        panel_scale[2] as f32,
    ];
    (pos, scale)
}

// ---------------------------------------------------------------------------
// 3. Shared Match Phase & Clock Quantization
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchPhase {
    Waiting = 0,
    Assemble = 1,
    Setup = 2,
    Active = 3,
    Unlock = 4,
    Complete = 5,
    RoundEnd = 6,
}

impl MatchPhase {
    pub fn name(self) -> &'static str {
        match self {
            Self::Waiting => "WAITING",
            Self::Assemble => "ASSEMBLE",
            Self::Setup => "SETUP",
            Self::Active => "ACTIVE",
            Self::Unlock => "UNLOCK",
            Self::Complete => "COMPLETE",
            Self::RoundEnd => "ROUND_END",
        }
    }
}

pub fn hud_clock_seconds(remaining: f64) -> f64 {
    let rem = remaining.max(0.0);
    let precision = if rem <= 10.0 { 10.0 } else { 4.0 };
    (rem * precision - 1e-7).ceil().max(0.0) / precision
}

// ---------------------------------------------------------------------------
// 4. Composable CapturePointStage (Assault + Hybrid Point A + Control)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct CapturePointSpec {
    pub placeable: u32,
    pub position: [f64; 3],
    pub zone: ObbZone,
    pub round_seconds: f64,
    pub capture_seconds: f64,
    pub segments: usize,
}

#[derive(Clone, Debug)]
pub struct CapturePointStage {
    pub spec: CapturePointSpec,
    pub progress: f64,
    pub peak: f64,
    pub contested: bool,
    pub capture_scale: f64,
}

impl CapturePointStage {
    pub fn new(spec: CapturePointSpec, capture_scale: f64) -> Self {
        Self {
            spec,
            progress: 0.0,
            peak: 0.0,
            contested: false,
            capture_scale,
        }
    }

    pub fn milestone_floor(&self) -> f64 {
        let seg = self.spec.segments.max(1) as f64;
        (self.peak * seg).floor() / seg
    }

    pub fn captured_segments(&self) -> usize {
        let seg = self.spec.segments.max(1);
        let p = (self.progress as f32) * (seg as f32);
        (p.floor() as usize).min(seg.saturating_sub(1))
    }

    /// Advance capture progress for Attack/Defend (team1 attacking, team0 defending).
    /// Returns `true` if the point reached 100% capture on this step.
    pub fn step_attack_defend(&mut self, dt: f64, actors: &[ActorSample]) -> bool {
        let occ = self.spec.zone.occupants(actors);
        let attackers = occ.team1.len();
        let defenders = occ.team0.len();
        self.contested = attackers > 0 && defenders > 0;
        if self.contested || dt <= 0.0 {
            return false;
        }
        if attackers > 0 {
            let mult = match attackers {
                1 => 1.0,
                2 => 1.25,
                _ => 1.5,
            };
            let gain = (dt * mult * self.capture_scale) / self.spec.capture_seconds.max(0.01);
            self.progress = (self.progress + gain).min(1.0);
            self.peak = self.peak.max(self.progress);
            return self.progress >= 1.0 - 1e-9;
        }
        // Decay toward last milestone floor
        let floor = self.milestone_floor();
        if self.progress > floor {
            let decay = dt / self.spec.capture_seconds.max(0.01) / 3.0;
            self.progress = (self.progress - decay).max(floor);
        }
        false
    }
}

#[derive(Clone, Debug)]
pub struct AssaultRound {
    pub points: Vec<CapturePointStage>,
    pub phase: MatchPhase,
    pub stage: usize,
    pub captured: usize,
    pub deadline: f64,
    pub last_update: f64,
    pub last_attacker: f64,
    pub overtime: bool,
    pub winner: Option<u8>,
}

impl AssaultRound {
    pub fn new(specs: Vec<CapturePointSpec>, capture_scale: f64) -> Self {
        let points = specs
            .into_iter()
            .map(|s| CapturePointStage::new(s, capture_scale))
            .collect();
        Self {
            points,
            phase: MatchPhase::Assemble,
            stage: 0,
            captured: 0,
            deadline: 0.0,
            last_update: 0.0,
            last_attacker: 0.0,
            overtime: false,
            winner: None,
        }
    }

    pub fn start_setup(&mut self, assembly_ends: f64) {
        self.phase = MatchPhase::Setup;
        self.deadline = assembly_ends + SETUP_SECONDS;
        self.last_update = assembly_ends;
    }

    pub fn current(&self) -> &CapturePointStage {
        &self.points[self.stage.min(self.points.len() - 1)]
    }

    pub fn advance(&mut self, now: f64, actors: &[ActorSample]) {
        if self.phase == MatchPhase::Setup && now >= self.deadline {
            self.phase = MatchPhase::Active;
            self.last_update = self.deadline;
            self.deadline += self.points[0].spec.round_seconds;
        }
        if self.phase != MatchPhase::Active {
            return;
        }
        let dt = (now - self.last_update).max(0.0);
        let occ = self.points[self.stage].spec.zone.occupants(actors);
        let has_attacker = !occ.team1.is_empty();
        if self.overtime {
            if has_attacker && now < self.last_attacker + OVERTIME_SECONDS {
                self.last_attacker = now;
            }
            if now >= self.last_attacker + OVERTIME_SECONDS {
                self.phase = MatchPhase::Complete;
                self.overtime = false;
                self.winner = Some(FIRST_DEFENDING_TEAM);
                self.last_update = now;
                return;
            }
        } else if now >= self.deadline {
            if has_attacker {
                self.overtime = true;
                self.last_attacker = now;
            } else {
                self.phase = MatchPhase::Complete;
                self.overtime = false;
                self.winner = Some(FIRST_DEFENDING_TEAM);
                self.last_update = now;
                return;
            }
        } else if has_attacker {
            self.last_attacker = now;
        }
        let completed = self.points[self.stage].step_attack_defend(dt, actors);
        self.last_update = now;
        if completed {
            self.captured += 1;
            self.overtime = false;
            if self.stage + 1 >= self.points.len() {
                self.phase = MatchPhase::Complete;
                self.winner = Some(FIRST_ATTACKING_TEAM);
            } else {
                self.stage += 1;
                self.deadline = self.deadline.max(now) + self.points[self.stage].spec.round_seconds;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 5. EscortRound (SplineMover + DynamicBarrierRegistry)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct EscortRound {
    pub mover: SplineMover,
    pub barriers: DynamicBarrierRegistry,
    pub checkpoints: Vec<(f64, u32)>,
    pub stage_seconds: Vec<f64>,
    pub debris_distances: BTreeMap<u32, f64>,
    pub phase: MatchPhase,
    pub stage: usize,
    pub stage_offset: usize,
    pub deadline: f64,
    pub overtime: bool,
    pub winner: Option<u8>,
    pub gate_impact_started: Option<f64>,
}

impl EscortRound {
    pub fn new(
        track: SplineTrack,
        curve: SplineSpeedCurve,
        barriers: DynamicBarrierRegistry,
        stage_seconds: Vec<f64>,
        auto_drive: bool,
    ) -> Self {
        let mut checkpoints = Vec::new();
        let mut debris_distances = BTreeMap::new();
        for (idx, node) in track.nodes.iter().enumerate() {
            if let Some(cp) = node.checkpoint {
                checkpoints.push((track.node_distances[idx], cp));
            }
        }
        for entry in &barriers.entries {
            if entry.kind == BarrierKind::GateDebris {
                if let Some(node_idx) = entry.debris_rail_node {
                    if let Some(&d) = track.node_distances.get(node_idx) {
                        debris_distances.insert(entry.entity, d);
                    }
                }
            }
        }
        let mover = SplineMover::new(track, curve, auto_drive);
        Self {
            mover,
            barriers,
            checkpoints,
            stage_seconds,
            debris_distances,
            phase: MatchPhase::Assemble,
            stage: 0,
            stage_offset: 0,
            deadline: 0.0,
            overtime: false,
            winner: None,
            gate_impact_started: None,
        }
    }

    pub fn start_setup(&mut self, assembly_ends: f64) {
        self.phase = MatchPhase::Setup;
        self.deadline = assembly_ends + SETUP_SECONDS;
        self.mover.last_update = assembly_ends;
    }

    pub fn checkpoint_fractions(&self) -> Vec<f64> {
        let len = self.mover.track.total_length.max(1e-9);
        self.checkpoints.iter().map(|(d, _)| d / len).collect()
    }

    pub fn near_finish(&self) -> bool {
        let rem = self.mover.track.total_length - self.mover.distance;
        self.phase == MatchPhase::Active
            && self.mover.distance > 0.0
            && rem > 0.0
            && rem < FINISH_WARNING_DISTANCE
    }

    pub fn advance(&mut self, now: f64, actors: &[ActorSample]) {
        if self.phase == MatchPhase::Setup && now >= self.deadline {
            self.phase = MatchPhase::Active;
            self.mover.last_attacker = self.deadline;
            self.mover.last_update = self.deadline;
            self.deadline += self.stage_seconds[0];
        }
        if self.phase != MatchPhase::Active {
            return;
        }
        let prev_dist = self.mover.distance;
        let events = self
            .mover
            .step(now, actors, FIRST_ATTACKING_TEAM, FIRST_DEFENDING_TEAM);
        for (tag, wait) in events.open_gates {
            self.barriers.request_route_gate(tag, true, now, wait);
        }
        for tag in events.close_gates {
            self.barriers.request_route_gate(tag, false, now, 0.0);
        }
        if let Some(t) = events.gate_impact_at {
            self.gate_impact_started = Some(t);
        }
        for (&ent, &trigger_d) in &self.debris_distances {
            if prev_dist < trigger_d && self.mover.distance + 1e-9 >= trigger_d {
                self.barriers.debris_started.entry(ent).or_insert(now);
            }
        }
        for _cp in events.reached_checkpoints {
            self.stage += 1;
            self.overtime = false;
            if self.stage >= self.checkpoints.len() {
                self.phase = MatchPhase::Complete;
                self.winner = Some(FIRST_ATTACKING_TEAM);
                self.mover.pushers.clear();
                self.mover.blockers.clear();
                return;
            }
            let extra = self.stage_seconds.get(self.stage).copied().unwrap_or(0.0);
            self.deadline = self.deadline.max(now) + extra;
        }

        if now >= self.deadline {
            if !self.mover.pushers.is_empty() || self.mover.scripted_movement() {
                self.overtime = true;
            } else if !self.overtime || now >= self.mover.last_attacker + OVERTIME_SECONDS {
                self.phase = MatchPhase::Complete;
                self.winner = Some(FIRST_DEFENDING_TEAM);
                self.overtime = false;
                self.mover.pushers.clear();
                self.mover.blockers.clear();
            }
        }
    }

    pub fn door_state(&self, entity: u32, now: f64, previous: Option<&DoorState>) -> DoorState {
        self.barriers.door_state(
            entity,
            now,
            self.stage + self.stage_offset,
            self.phase == MatchPhase::Active || self.phase == MatchPhase::Complete,
            previous,
        )
    }
}

// ---------------------------------------------------------------------------
// 6. HybridRound (CapturePointStage -> EscortRound)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct HybridRound {
    pub capture: CapturePointStage,
    pub escort: EscortRound,
    pub unlock_delay: f64,
    pub unlock_seconds: f64,
    pub move_at: Option<f64>,
}

impl HybridRound {
    pub fn new(
        capture: CapturePointStage,
        mut escort: EscortRound,
        unlock_delay: f64,
        unlock_seconds: f64,
    ) -> Self {
        escort.stage_offset = 1;
        Self {
            capture,
            escort,
            unlock_delay,
            unlock_seconds,
            move_at: None,
        }
    }

    pub fn start_setup(&mut self, assembly_ends: f64) {
        self.escort.phase = MatchPhase::Setup;
        self.escort.deadline = assembly_ends + SETUP_SECONDS;
        self.escort.mover.last_update = assembly_ends;
    }

    pub fn room_stage(&self) -> usize {
        if self.capture.progress < 1.0 - 1e-9 {
            0
        } else {
            1 + self.escort.stage
        }
    }

    pub fn payload_active(&self) -> bool {
        self.escort.phase == MatchPhase::Active && self.capture.progress >= 1.0 - 1e-9
    }

    pub fn advance(&mut self, now: f64, actors: &[ActorSample]) {
        if self.escort.phase == MatchPhase::Setup && now >= self.escort.deadline {
            self.escort.phase = MatchPhase::Active;
            self.escort.mover.last_update = self.escort.deadline;
            self.escort.mover.last_attacker = self.escort.deadline;
            self.escort.deadline += self.capture.spec.round_seconds;
        }
        if self.escort.phase != MatchPhase::Active {
            return;
        }
        if self.capture.progress < 1.0 - 1e-9 {
            let dt = (now - self.escort.mover.last_update).max(0.0);
            let occ = self.capture.spec.zone.occupants(actors);
            if !occ.team1.is_empty() {
                self.escort.mover.last_attacker = now;
            }
            if now >= self.escort.deadline && occ.team1.is_empty() {
                if !self.escort.overtime
                    || now >= self.escort.mover.last_attacker + OVERTIME_SECONDS
                {
                    self.escort.phase = MatchPhase::Complete;
                    self.escort.winner = Some(FIRST_DEFENDING_TEAM);
                    self.escort.mover.last_update = now;
                    return;
                }
            } else if now >= self.escort.deadline {
                self.escort.overtime = true;
            }
            let done = self.capture.step_attack_defend(dt, actors);
            self.escort.mover.last_update = now;
            if done {
                self.escort.overtime = false;
                let extra = self.escort.stage_seconds.first().copied().unwrap_or(0.0);
                self.escort.deadline = self.escort.deadline.max(now) + extra;
                self.move_at = Some(now + self.unlock_delay + self.unlock_seconds);
                self.escort.mover.last_attacker = now;
            }
            return;
        }

        if let Some(unlock_end) = self.move_at {
            if now < unlock_end {
                self.escort.mover.last_update = now;
                return;
            }
            self.escort.mover.last_update = self.escort.mover.last_update.max(unlock_end);
        }
        self.escort.advance(now, actors);
    }
}

// ---------------------------------------------------------------------------
// 7. ControlRound (Multi-Round Capture + 0..100 Scoring + Healing Rooms)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ControlStageSpec {
    pub point: CapturePointSpec,
    pub healing_rooms: [Option<ObbZone>; 2],
}

#[derive(Clone, Debug)]
pub struct ControlRound {
    pub stages: Vec<ControlStageSpec>,
    pub stage_order: Vec<usize>,
    pub round_index: usize,
    pub points_to_win: u32,
    pub capture_scale: f64,
    pub score_scale: f64,
    pub fixed_stage: Option<usize>,
    pub phase: MatchPhase,
    pub owner: i8, // -1 neutral, 0 team0, 1 team1
    pub progress: f64,
    pub scores: [f64; 2],
    pub round_wins: [u32; 2],
    pub contested: bool,
    pub overtime: bool,
    pub round_winner: Option<u8>,
    pub winner: Option<u8>,
    pub deadline: f64,
    pub last_update: f64,
    pub last_capture: f64,
    pub last_challenger: f64,
    pub barriers: DynamicBarrierRegistry,
}

impl ControlRound {
    pub fn new(
        stages: Vec<ControlStageSpec>,
        stage_order: Vec<usize>,
        points_to_win: u32,
        capture_scale: f64,
        score_scale: f64,
        fixed_stage: Option<usize>,
        barriers: DynamicBarrierRegistry,
    ) -> Self {
        Self {
            stages,
            stage_order,
            round_index: 0,
            points_to_win,
            capture_scale,
            score_scale,
            fixed_stage,
            phase: MatchPhase::Assemble,
            owner: -1,
            progress: 0.0,
            scores: [0.0, 0.0],
            round_wins: [0, 0],
            contested: false,
            overtime: false,
            round_winner: None,
            winner: None,
            deadline: 0.0,
            last_update: 0.0,
            last_capture: 0.0,
            last_challenger: 0.0,
            barriers,
        }
    }

    pub fn stage(&self) -> usize {
        if let Some(fixed) = self.fixed_stage {
            return fixed;
        }
        self.stage_order[self
            .round_index
            .min(self.stage_order.len().saturating_sub(1))]
    }

    pub fn movement_scale(&self) -> f64 {
        if self.phase == MatchPhase::RoundEnd {
            CONTROL_ROUND_END_MOVEMENT_SCALE
        } else {
            1.0
        }
    }

    pub fn start_setup(&mut self, now: f64) {
        self.phase = MatchPhase::Setup;
        self.deadline = now + CONTROL_SETUP_SECONDS;
        self.last_update = now;
    }

    pub fn in_healing_room(&self, team: u8, pos: [f64; 3]) -> bool {
        if self.phase == MatchPhase::Complete || self.phase == MatchPhase::RoundEnd {
            return false;
        }
        if team > 1 {
            return false;
        }
        self.stages[self.stage()].healing_rooms[team as usize]
            .as_ref()
            .is_some_and(|z| z.contains(pos))
    }

    pub fn next_round(&mut self, now: f64) {
        self.round_index += 1;
        self.owner = -1;
        self.progress = 0.0;
        self.scores = [0.0, 0.0];
        self.contested = false;
        self.overtime = false;
        self.round_winner = None;
        self.phase = MatchPhase::Setup;
        self.deadline = now + CONTROL_SETUP_SECONDS;
        self.last_update = now;
    }

    pub fn finish_round(&mut self, winner: u8, now: f64) {
        self.round_winner = Some(winner);
        self.round_wins[winner as usize] += 1;
        self.scores[winner as usize] = 100.0;
        self.overtime = false;
        if self.round_wins[winner as usize] >= self.points_to_win {
            self.winner = Some(winner);
            self.phase = MatchPhase::Complete;
            self.deadline = now + CONTROL_INTERMISSION_SECONDS;
        } else {
            self.phase = MatchPhase::RoundEnd;
            self.deadline = now + CONTROL_ROUND_SUMMARY_DELAY;
        }
    }

    pub fn advance(&mut self, now: f64, actors: &[ActorSample]) {
        if self.phase == MatchPhase::Setup && now >= self.deadline {
            self.phase = MatchPhase::Unlock;
            self.deadline += CONTROL_UNLOCK_SECONDS;
            self.last_update = now;
            return;
        }
        if self.phase == MatchPhase::Unlock && now >= self.deadline {
            self.phase = MatchPhase::Active;
            self.last_update = self.deadline;
        }
        if self.phase == MatchPhase::RoundEnd {
            if now >= self.deadline {
                self.phase = MatchPhase::Complete;
                self.deadline = now + CONTROL_INTERMISSION_SECONDS;
            }
            return;
        }
        if self.phase != MatchPhase::Active {
            return;
        }

        let dt = (now - self.last_update).max(0.0);
        self.last_update = now;

        let occ = self.stages[self.stage()].point.zone.occupants(actors);
        let c0 = occ.team0.len();
        let c1 = occ.team1.len();
        self.contested = c0 > 0 && c1 > 0;

        // Owner score accumulation (default 1.0% per 1.2s = 100/120 %/s, or score_scale * 100/120)
        if self.owner == 0 || self.owner == 1 {
            let ow = self.owner as usize;
            let challengers = if ow == 0 { c1 } else { c0 };
            if challengers > 0 {
                self.last_challenger = now;
            }
            let rate = (100.0 / 120.0) * self.score_scale;
            self.scores[ow] = (self.scores[ow] + dt * rate).min(100.0);
            if self.scores[ow] >= 99.99 {
                let trigger = challengers > 0 || self.contested || self.progress > 0.0;
                if !self.overtime && trigger {
                    self.overtime = true;
                    self.last_challenger = self.last_challenger.max(now);
                }
                if self.overtime {
                    if now < self.last_challenger + OVERTIME_SECONDS {
                        self.scores[ow] = 99.99;
                    } else {
                        self.finish_round(ow as u8, now);
                        return;
                    }
                } else {
                    self.finish_round(ow as u8, now);
                    return;
                }
            }
        }

        // Capture / retake progress
        if !self.contested {
            let cap_sec = self.stages[self.stage()].point.capture_seconds.max(0.01);
            let dominant: Option<usize> = if c0 > 0 && self.owner != 0 {
                Some(0)
            } else if c1 > 0 && self.owner != 1 {
                Some(1)
            } else {
                None
            };
            if let Some(team) = dominant {
                let count = if team == 0 { c0 } else { c1 };
                let mult = match count {
                    1 => 1.0,
                    2 => 1.5,
                    _ => 2.0,
                };
                self.last_capture = now;
                self.progress =
                    (self.progress + (dt * mult * self.capture_scale) / cap_sec).min(1.0);
                if self.progress >= 1.0 - 1e-9 {
                    self.owner = team as i8;
                    self.progress = 0.0;
                    self.overtime = false;
                }
            } else if self.progress > 0.0 && now > self.last_capture + 5.0 {
                let decay = dt.min(now - self.last_capture - 5.0);
                self.progress = (self.progress - decay / cap_sec).max(0.0);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 8. MatchAudioScheduler
// ---------------------------------------------------------------------------

pub const CUE_SELECT_HERO: VoiceCue = VoiceCue::new(0x1B3, 0x0646);
pub const CUE_PREPARE: [VoiceCue; 2] = [VoiceCue::new(0x168, 0x05DB), VoiceCue::new(0x167, 0x05DA)];
pub const CUE_ATTACK_STARTS: [VoiceCue; 2] =
    [VoiceCue::new(0x05D, 0x05F2), VoiceCue::new(0x06B, 0x05F1)];
pub const CUE_PAYLOAD: [VoiceCue; 2] = [VoiceCue::new(0x19B, 0x0620), VoiceCue::new(0x194, 0x0619)];
pub const CUE_OBJECTIVE_A: [VoiceCue; 2] =
    [VoiceCue::new(0x17B, 0x05F8), VoiceCue::new(0x178, 0x05F5)];
pub const CUE_OBJECTIVE_B: [VoiceCue; 2] =
    [VoiceCue::new(0x17C, 0x05FA), VoiceCue::new(0x179, 0x05F6)];
pub const CUE_POINT_A_TAKEN: [VoiceCue; 2] =
    [VoiceCue::new(0x061, 0x0613), VoiceCue::new(0x060, 0x060D)];
pub const CUE_CHECKPOINT: VoiceCue = VoiceCue::new(0x198, 0x061D);
pub const CUE_NEAR_FINISH: VoiceCue = VoiceCue::new(0x199, 0x061C);
pub const CUE_OVERTIME: VoiceCue = VoiceCue::new(0x066, 0x0625);
pub const CUE_RESULT: [VoiceCue; 2] = [VoiceCue::new(0x19F, 0x0628), VoiceCue::new(0x19E, 0x0627)];
pub const CUE_MATCH_COMPLETE: VoiceCue = VoiceCue::new(0x6C2, 0x8B45);
pub const CUE_PREPARE_FOR_BATTLE: VoiceCue = VoiceCue::new(0x70B, 0x8D9D);
pub const CUE_CONTROL_START: VoiceCue = VoiceCue::new(0x4FA, 0x7B53);
pub const CUE_CONTROL_ROUNDS: [VoiceCue; 3] = [
    CUE_CONTROL_START,
    VoiceCue::new(0x4FB, 0x7B54),
    VoiceCue::new(0x4FC, 0x7B55),
];
pub const CUE_CONTROL_UNLOCKED: VoiceCue = VoiceCue::new(0x464, 0x81F0);
pub const CUE_CONTROL_ROUND_RESULT: [[VoiceCue; 2]; 3] = [
    [VoiceCue::new(0x46A, 0x77BC), VoiceCue::new(0x469, 0x77BE)],
    [VoiceCue::new(0x47C, 0x7BEA), VoiceCue::new(0x47A, 0x7BEC)],
    [VoiceCue::new(0x5C4, 0x8244), VoiceCue::new(0x5C7, 0x8247)],
];
pub const CUE_CONTROL_SCORE: VoiceCue = VoiceCue::new(0x63B, 0x8447);
pub const CUE_CONTROL_SCORE_FIRST: [VoiceCue; 3] = [
    VoiceCue::new(0x61B, 0x8427),
    VoiceCue::new(0x5FD, 0x8408),
    VoiceCue::new(0x5FE, 0x840A),
];
pub const CUE_CONTROL_SCORE_SECOND: [VoiceCue; 3] = [
    VoiceCue::new(0x61C, 0x8431),
    VoiceCue::new(0x61D, 0x8426),
    VoiceCue::new(0x61E, 0x842A),
];

pub fn setup_clock_cue(seconds: i64, attacks: bool) -> Option<VoiceCue> {
    match seconds {
        60 => Some(if attacks {
            VoiceCue::new(0x169, 0x05DD)
        } else {
            VoiceCue::new(0x16A, 0x05DE)
        }),
        30 => Some(if attacks {
            VoiceCue::new(0x16B, 0x05DF)
        } else {
            VoiceCue::new(0x16C, 0x05E0)
        }),
        _ => None,
    }
}

pub fn round_clock_cue(seconds: i64) -> Option<VoiceCue> {
    match seconds {
        60 => Some(VoiceCue::new(0x19D, 0x0622)),
        30 => Some(VoiceCue::new(0x067, 0x0623)),
        10 => Some(VoiceCue::new(0x068, 0x0624)),
        _ => None,
    }
}

pub fn number_cue(seconds: i64) -> Option<VoiceCue> {
    match seconds {
        5 => Some(VoiceCue::new(0x057, 0x05EC)),
        4 => Some(VoiceCue::new(0x058, 0x05EB)),
        3 => Some(VoiceCue::new(0x059, 0x05EA)),
        2 => Some(VoiceCue::new(0x05B, 0x05E9)),
        1 => Some(VoiceCue::new(0x05C, 0x05E7)),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioTimeline {
    pub phase: MatchPhase,
    pub seconds: i64,
    pub stage: usize,
    pub overtime: bool,
    pub winner: Option<u8>,
    pub near_finish: bool,
    pub control_wins: Option<[u8; 2]>,
    pub round_winner: Option<u8>,
}

pub fn match_speech(
    previous: Option<&AudioTimeline>,
    current: &AudioTimeline,
    mode: u64,
    team: u8,
    single_countdown: bool,
) -> Vec<VoiceCue> {
    let attacks = team == FIRST_ATTACKING_TEAM;
    let spectator = team == SPECTATOR_TEAM;
    let att = usize::from(attacks);

    if current.phase == MatchPhase::Assemble
        && previous.is_none_or(|p| p.phase == MatchPhase::Waiting)
    {
        if spectator || current.seconds <= 2 {
            return Vec::new();
        }
        let prep = if mode == MODE_CONTROL {
            CUE_PREPARE_FOR_BATTLE
        } else {
            CUE_PREPARE[att]
        };
        return vec![prep, CUE_SELECT_HERO];
    }

    let Some(prev) = previous else {
        return Vec::new();
    };

    if mode == MODE_CONTROL && !single_countdown {
        if current.phase == MatchPhase::RoundEnd
            && prev.phase == MatchPhase::Active
            && current.stage == prev.stage
        {
            if let Some(rw) = current.round_winner {
                let won = usize::from(spectator || team == rw);
                return vec![CUE_CONTROL_ROUND_RESULT[current.stage.min(2)][won]];
            }
        }
        if current.phase == MatchPhase::Complete && prev.phase != MatchPhase::Complete {
            if let Some(w) = current.winner {
                return vec![if spectator {
                    CUE_MATCH_COMPLETE
                } else {
                    CUE_RESULT[usize::from(team == w)]
                }];
            }
            if let Some([w0, w1]) = current.control_wins {
                let (first, second) = if team == FIRST_ATTACKING_TEAM {
                    (w1 as usize, w0 as usize)
                } else {
                    (w0 as usize, w1 as usize)
                };
                return vec![
                    CUE_CONTROL_SCORE,
                    CUE_CONTROL_SCORE_FIRST[first.min(2)],
                    CUE_CONTROL_SCORE_SECOND[second.min(2)],
                ];
            }
        }
        if current.overtime && !prev.overtime {
            return vec![CUE_OVERTIME];
        }
        if current.phase == MatchPhase::Setup
            && prev.phase == MatchPhase::Setup
            && current.stage == prev.stage
            && current.seconds < prev.seconds
        {
            if let Some(c) = number_cue(current.seconds) {
                return vec![c];
            }
        }
        if current.phase == MatchPhase::Unlock
            && current.seconds > 5
            && (prev.phase != MatchPhase::Unlock || prev.stage != current.stage)
        {
            return vec![CUE_CONTROL_ROUNDS[current.stage.min(2)]];
        }
        if current.phase == MatchPhase::Active
            && prev.phase == MatchPhase::Unlock
            && current.stage == prev.stage
        {
            return vec![CUE_CONTROL_UNLOCKED];
        }
        return Vec::new();
    }

    if current.phase == MatchPhase::Complete && prev.phase != MatchPhase::Complete {
        return vec![if spectator {
            CUE_MATCH_COMPLETE
        } else {
            CUE_RESULT[usize::from(current.winner == Some(team))]
        }];
    }

    let mut finish = Vec::new();
    if current.near_finish && !prev.near_finish {
        finish.push(CUE_NEAR_FINISH);
    }

    if current.overtime && (!prev.overtime || prev.stage != current.stage) {
        let mut out = vec![CUE_OVERTIME];
        out.extend(finish);
        return out;
    }

    if current.phase == MatchPhase::Active
        && matches!(prev.phase, MatchPhase::Assemble | MatchPhase::Setup)
    {
        if spectator || current.stage > 0 {
            return Vec::new();
        }
        if mode == MODE_CONTROL {
            return vec![CUE_CONTROL_START];
        }
        let obj = if mode == MODE_ESCORT {
            CUE_PAYLOAD[att]
        } else {
            CUE_OBJECTIVE_A[att]
        };
        return vec![CUE_ATTACK_STARTS[att], obj];
    }

    if current.phase == MatchPhase::Active && prev.phase == MatchPhase::Active {
        if prev.stage < current.stage {
            if mode == MODE_HYBRID && prev.stage == 0 {
                return if spectator {
                    Vec::new()
                } else {
                    vec![CUE_POINT_A_TAKEN[att], CUE_PAYLOAD[att]]
                };
            }
            if mode == MODE_ESCORT || mode == MODE_HYBRID {
                let mut out = vec![CUE_CHECKPOINT];
                out.extend(finish);
                return out;
            }
            return if spectator {
                Vec::new()
            } else {
                vec![CUE_POINT_A_TAKEN[att], CUE_OBJECTIVE_B[att]]
            };
        }
        if !finish.is_empty() {
            return finish;
        }
        if !current.overtime && prev.stage == current.stage && current.seconds < prev.seconds {
            return round_clock_cue(current.seconds).into_iter().collect();
        }
    }

    if current.phase == prev.phase && current.seconds < prev.seconds {
        if current.phase == MatchPhase::Setup {
            if !spectator {
                if let Some(c) = setup_clock_cue(current.seconds, attacks) {
                    return vec![c];
                }
            }
            return number_cue(current.seconds).into_iter().collect();
        }
        if current.phase == MatchPhase::Assemble && single_countdown {
            return number_cue(current.seconds).into_iter().collect();
        }
    }

    Vec::new()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModeActionSnapshot {
    pub phase: MatchPhase,
    pub stage: usize,
    pub seconds: f64,
    pub overtime: bool,
}

pub fn mode_action(prev: ModeActionSnapshot, cur: ModeActionSnapshot) -> Option<u16> {
    if prev.overtime && !cur.overtime {
        return Some(473);
    }
    if cur.overtime && !prev.overtime {
        return Some(471);
    }
    if prev.phase != cur.phase || prev.stage != cur.stage {
        return None;
    }
    if !(0.0 < cur.seconds && cur.seconds < prev.seconds) {
        return None;
    }
    match cur.phase {
        MatchPhase::Assemble if cur.seconds <= 10.0 && 10.0 < prev.seconds => Some(469),
        MatchPhase::Setup => {
            let sec = cur.seconds.ceil() as i64;
            if (1..=5).contains(&sec) && sec < (prev.seconds.ceil() as i64) {
                return Some((534 - sec) as u16); // 5->529, 4->530, 3->531, 2->532, 1->533
            }
            if 5.0 < cur.seconds && cur.seconds <= 15.0 && 15.0 < prev.seconds {
                return Some(470);
            }
            None
        }
        MatchPhase::Active if cur.seconds <= 30.0 && 30.0 < prev.seconds => Some(472),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
