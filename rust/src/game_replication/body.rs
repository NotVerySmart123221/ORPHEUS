use super::{delivery::*, *};
use crate::{script_control::LEGACY_PROJECTILE_CONTROL, script_snapshot_wire::SnapshotError};

pub(super) struct StepBuffers<'a> {
    pub(super) frame: &'a mut u32,
    pub(super) fields: &'a mut Vec<(ProjectileToken, BioticField)>,
    pub(super) launches: &'a mut Vec<AdmittedFlight>,
    pub(super) hitscan: &'a mut Vec<PosedHitscan>,
}

pub(super) fn stage_scripted_motion(runtime: &mut dyn OwnerBodyRuntime, mover: &mut Mover) {
    if mover.has_pending_script_motion() {
        return;
    }
    if let Some((frame, motion)) = runtime.take_scripted_motion() {
        mover.queue_scripted_motion(frame, motion);
    }
}

pub(super) struct BodyScript {
    pub(super) id: BodyId,
    pub(super) stream: BodyStream<ProjectedSnapshot>,
    pub(super) frame: u32,
    pub(super) unsent_delta: Option<(u32, DeltaKind)>,
    pub(super) corrections: Option<CorrectionHistory>,
    continuous_corrections: bool,
    processed_at_spawn: Option<u32>,
    pub(super) projectiles: Option<OwnerProjectiles>,
}

fn startup_snapshot(runtime: &dyn OwnerBodyRuntime, entity: u32) -> io::Result<&Snapshot> {
    let snapshot = runtime
        .snapshot()
        .ok_or_else(|| invalid("body VM is unavailable"))?;
    if snapshot.entity != entity {
        return Err(invalid("body VM has the wrong entity"));
    }
    Ok(snapshot)
}

pub(crate) fn validate_body_baselines(
    runtime: &dyn OwnerBodyRuntime,
    entity: u32,
) -> io::Result<()> {
    let snapshot = startup_snapshot(runtime, entity)?;
    let encoder = runtime.encoder();
    for (view, budget_error) in [
        (
            View::Owner,
            "owner script baseline exceeds the datagram budget",
        ),
        (
            View::Observer,
            "observer script baseline exceeds the datagram budget",
        ),
    ] {
        let baseline_error = |error| invalid(format!("{view:?} script baseline: {error}"));
        let projected = encoder.project(snapshot, view).map_err(baseline_error)?;
        let chunk = encoder
            .queue_full(&mut BodyStream::new(entity), &projected)
            .map_err(baseline_error)?;
        ensure_script_chunk_fits(&chunk.update, budget_error)?;
    }
    Ok(())
}

impl BodyScript {
    pub(super) fn new(
        body: Body,
        frame: u32,
        processed_at_spawn: Option<u32>,
        runtime: &dyn OwnerBodyRuntime,
    ) -> io::Result<Self> {
        startup_snapshot(runtime, body.id.entity)?;
        let continuous_corrections =
            matches!(body.hero, SOLDIER | CASSIDY) && runtime.movement_modifiers().is_some();
        let corrections = continuous_corrections.then(|| {
            CorrectionHistory::new(body.id.entity, body.id.generation)
                .with_processed_at_spawn(processed_at_spawn)
        });
        let projectiles = runtime
            .projectile_leases()
            .filter(|leases| !leases.blocks().is_empty())
            .map(|leases| OwnerProjectiles::new(body.id.entity, leases.clone()));
        Ok(Self {
            id: body.id,
            stream: BodyStream::new(body.id.entity),
            frame,
            unsent_delta: None,
            corrections,
            continuous_corrections,
            processed_at_spawn,
            projectiles,
        })
    }

    pub(super) fn record_scripted_movement(
        &mut self,
        mover: &Mover,
        frame: u32,
        frame_us: u32,
        previously_processed: Option<u32>,
    ) {
        if frame_us == 0 || !mover.scripted_motion_at(frame) {
            return;
        }
        let history = self.corrections.get_or_insert_with(|| {
            CorrectionHistory::new(self.id.entity, self.id.generation)
                .with_processed_at_spawn(self.processed_at_spawn.max(previously_processed))
        });
        history.swept_motion(frame, frame_us);
        history.record(mover.pose());
    }

    pub(super) fn scripted_motion_history(&mut self) -> Option<&mut CorrectionHistory> {
        self.corrections
            .as_mut()
            .filter(|history| history.impulse_correction().is_some())
    }

    pub(super) fn correction_sent(&mut self, correction: &crate::mover_correction::Correction) {
        if let Some(history) = &mut self.corrections {
            if history.sent(correction)
                && !self.continuous_corrections
                && history.impulse_correction().is_none()
            {
                self.corrections = None;
            }
        }
    }

    pub(super) fn advance_step(
        &mut self,
        runtime: &mut dyn OwnerBodyRuntime,
        mover: &mut Mover,
        buffers: &mut StepBuffers<'_>,
        command: Command,
        step: BodyStep,
    ) -> io::Result<()> {
        if command.frame > *buffers.frame {
            if let Err(error) = runtime.advance(command, mover, step) {
                mover.set_authored_facing(None);
                return Err(error);
            }
            *buffers.frame = command.frame;
            self.frame = command.frame;
        }
        match runtime.snapshot() {
            Some(snapshot) if snapshot.initialization_complete() => {}
            Some(snapshot) => {
                mover.set_authored_facing(None);
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    SnapshotError::Incomplete {
                        entity: snapshot.entity,
                        diagnostics: snapshot.diagnostics.clone(),
                    },
                ));
            }
            None => {
                mover.set_authored_facing(None);
                return Err(invalid("body VM has no snapshot"));
            }
        }
        mover.set_cinematic_aim(runtime.cinematic_aim());
        mover.set_authored_facing(runtime.authored_facing());
        let velocity_changes = runtime.take_velocity_changes();
        if runtime
            .vitals()
            .is_some_and(|vitals| vitals.death().is_some())
        {
            mover.clear_pending_motion();
        } else {
            for (frame, change) in velocity_changes {
                mover.queue_velocity_change(frame, change);
            }
        }
        if step == BodyStep::UnposedTimer {
            // Combat discards the neutral interval's reports at its boundary.
            // Drain interior reports early so a long gap cannot fill the VM queue.
            runtime.take_shots(None, mover);
            return Ok(());
        }
        if let Some(modifiers) = runtime.movement_modifiers() {
            mover.track_modifiers(command.frame, modifiers);
        } else if self.continuous_corrections {
            return Err(invalid("body VM lost its movement projection"));
        }
        if let Some(history) = self
            .corrections
            .as_mut()
            .filter(|_| step == BodyStep::Posed)
        {
            history.record(mover.pose());
        }
        for shot in runtime.take_shots((step == BodyStep::Posed).then_some(command), mover) {
            match shot {
                PosedShot::Hitscan(shot) => buffers.hitscan.push(*shot),
                PosedShot::Projectile(shot) => self.launch_projectile(*shot, buffers.launches),
                PosedShot::BioticField(shot) => self.launch_biotic_field(shot, buffers.fields),
            }
        }
        if step == BodyStep::Posed {
            stage_scripted_motion(runtime, mover);
        }
        Ok(())
    }

    fn launch_biotic_field(
        &mut self,
        field: PosedBioticField,
        fields: &mut Vec<(ProjectileToken, BioticField)>,
    ) {
        if let Some(token) = self.projectiles.as_mut().and_then(|projectiles| {
            projectiles.admit(field.shot, BIOTIC_CANISTER, BioticField::BOUND)
        }) {
            fields.push((token, BioticField::new(field.feet)));
        }
    }

    pub(super) fn launch_projectile(
        &mut self,
        projectile: PosedProjectile,
        launches: &mut Vec<AdmittedFlight>,
    ) {
        let Some((projectiles, definition)) = self
            .projectiles
            .as_mut()
            .zip(projectile.ballistics.definition)
        else {
            #[cfg(feature = "inspector")]
            crate::owner_trace::playtest(format_args!(
                "projectile event=launch-rejected shot={:?} reason=no-definition-or-tracker",
                projectile.shot
            ));
            return;
        };
        let bound = (projectile.ballistics.lifetime / TICK_PERIOD.as_secs_f64()).ceil() as u32;
        #[cfg(feature = "inspector")]
        let radius = projectile.ballistics.radius;
        let Some(flight) = ProjectileFlight::new(
            projectile.ballistics,
            projectile.origin,
            projectile.direction,
            None,
        ) else {
            #[cfg(feature = "inspector")]
            crate::owner_trace::playtest(format_args!(
                "projectile event=launch-rejected shot={:?} reason=invalid-flight",
                projectile.shot
            ));
            return;
        };
        let flight = if let Some(gravity) = projectile.gravity {
            flight.with_gravity(gravity)
        } else {
            flight
        };
        let flight = flight.with_expiry_blast(projectile.expiry_blast);
        if let Some(token) = projectiles.admit(projectile.shot, definition, bound) {
            #[cfg(feature = "inspector")]
            crate::owner_trace::playtest(format_args!(
                "projectile event=launched token={token:?} shot={:?} definition={definition:016x} origin={:?} aim={:?} radius={radius}",
                projectile.shot, projectile.origin, projectile.direction
            ));
            launches.push(AdmittedFlight {
                token,
                flight,
                stun: projectile.stun,
                hit_effect: projectile
                    .stun
                    .and_then(|_| LEGACY_PROJECTILE_CONTROL.flight_site(&projectile.shot)),
            });
        } else {
            #[cfg(feature = "inspector")]
            crate::owner_trace::playtest(format_args!(
                "projectile event=launch-rejected shot={:?} definition={definition:016x} reason=not-admitted",
                projectile.shot
            ));
        }
    }

    pub(super) fn next_record(
        &mut self,
        runtime: &dyn OwnerBodyRuntime,
        created: bool,
    ) -> io::Result<Option<(Record, Option<u32>)>> {
        if !created {
            return Ok(None);
        }
        let encoder = runtime.encoder();
        let snapshot = runtime
            .snapshot()
            .ok_or_else(|| invalid("body VM is unavailable"))?;
        let (chunk, kind) = if self.stream.last() == 0 {
            let snapshot = encoder.project(snapshot, View::Owner).map_err(invalid)?;
            (
                encoder
                    .queue_full(&mut self.stream, &snapshot)
                    .map_err(invalid)?,
                None,
            )
        } else {
            let replace_initial =
                matches!(self.unsent_delta, Some((_, DeltaKind::InitialOwner(_))));
            let Some(kind) = self
                .stream
                .owner_kind(self.frame, false, replace_initial)
                .map_err(invalid)?
            else {
                return Ok(None);
            };
            let (_, base) = self
                .stream
                .base()
                .expect("owner delta requires the baseline");
            let snapshot = encoder.project_next(snapshot, base).map_err(invalid)?;
            let chunk = encoder
                .queue_delta(&mut self.stream, &snapshot, kind, runtime.frame_ms())
                .map_err(invalid)?
                .expect("owner deltas include a command frame");
            (chunk, Some(kind))
        };
        // An individual chunk that cannot fit alone needs fragmentation.
        // Fail this VM closed so it cannot hold up other entity records.
        ensure_script_chunk_fits(
            &chunk.update,
            "body script chunk exceeds the game datagram budget",
        )?;
        let last = chunk.last;
        let replaced = kind.and_then(|kind| {
            self.unsent_delta.replace((last, kind)).map(|(last, _)| {
                self.stream.discard_unsent(last);
                last
            })
        });
        let record = Record {
            update: chunk.update,
            delivery: Delivery::BodyScript {
                #[cfg(test)]
                rifle_ammo_written: chunk.owner_rifle_ammo_written,
                id: self.id,
                last,
                delta: kind.is_some(),
            },
        };
        Ok(Some((record, replaced)))
    }

    pub(super) fn sent(&mut self, last: u32) {
        if let Some(report) = self.stream.sent(last) {
            #[cfg(feature = "inspector")]
            if crate::owner_trace::enabled() {
                report.snapshot.trace_owner_volley_delta(
                    last,
                    report.command_frame,
                    report.changes,
                );
            }
            if let Some(projectiles) = self.projectiles.as_mut() {
                projectiles.owner_sent(
                    last,
                    report.command_frame,
                    |instance, raw| {
                        report
                            .changes
                            .contains(&ChangeKey::State(instance, raw))
                            .then(|| report.snapshot.volley_identity(instance, raw))
                            .flatten()
                    },
                    |instance, raw| {
                        report
                            .changes
                            .contains(&ChangeKey::VolleyIntroduction(instance, raw))
                    },
                );
            }
        }
        if self.unsent_delta.is_some_and(|(queued, _)| queued == last) {
            self.unsent_delta = None;
        }
    }
}

pub(super) enum InputWork {
    Message {
        message: Message,
        tick: u32,
        received: Duration,
    },
    Skin {
        hero: u64,
        tick: u32,
        received: Duration,
    },
    Command {
        command: Command,
        pending: Option<PendingCommand>,
    },
    Leave,
}

#[derive(Default)]
pub(super) struct ScriptGap {
    release: Option<u32>,
    tail: Range<u32>,
}

impl ScriptGap {
    pub(super) fn before(script_frame: u32, first_pose: u32) -> Self {
        let first_missing = script_frame.saturating_add(1);
        if first_missing >= first_pose {
            return Self::default();
        }
        let last_missing = first_pose - 1;
        Self {
            release: Some(first_missing),
            tail: (first_missing + 1).max(last_missing.saturating_sub(MAX_SCRIPT_TAIL))..first_pose,
        }
    }
}

impl Iterator for ScriptGap {
    type Item = (u32, BodyStep);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(frame) = self.release.take() {
            return Some((frame, BodyStep::UnposedBoundary));
        }
        let frame = self.tail.next()?;
        let step = if self.tail.is_empty() {
            BodyStep::UnposedBoundary
        } else {
            BodyStep::UnposedTimer
        };
        Some((frame, step))
    }
}

pub(super) struct PendingCommand {
    pub(super) poses: Poses,
    pub(super) gap: ScriptGap,
}

impl PendingCommand {
    pub(super) fn new(poses: Poses, script: Option<&BodyScript>) -> Self {
        let gap = script
            .zip(poses.next_frame())
            .map_or_else(ScriptGap::default, |(script, first_pose)| {
                ScriptGap::before(script.frame, first_pose)
            });
        Self { poses, gap }
    }
}

#[derive(Default)]
pub(super) struct InputQueue {
    pub(super) work: VecDeque<InputWork>,
    pub(super) first_admitted: Option<u32>,
    pub(super) admitted: Option<u32>,
    pub(super) processed: Option<u32>,
    pub(super) closing: bool,
}

impl InputQueue {
    pub(super) fn push(&mut self, work: InputWork) -> bool {
        if self.closing {
            return false;
        }
        if self.work.len() == MAX_PENDING_INPUT {
            self.close_after_prefix();
            return false;
        }
        self.work.push_back(work);
        true
    }

    pub(super) fn commands(&mut self, tick: u32, commands: &[Command], body_frame: Option<u32>) {
        if self.closing || commands.is_empty() {
            return;
        }
        let (fresh, boundary) = command_prefix(self.admitted.or(body_frame), tick, commands);
        for command in fresh {
            if !self.push(InputWork::Command {
                command,
                pending: None,
            }) {
                return;
            }
            self.first_admitted.get_or_insert(command.frame);
            self.admitted = Some(command.frame);
        }
        if boundary.is_some() {
            self.close_after_prefix();
        }
    }

    fn close_after_prefix(&mut self) {
        self.work.push_back(InputWork::Leave);
        self.closing = true;
    }

    pub(super) fn clear(&mut self) {
        self.work.clear();
        self.admitted = self.processed;
        self.closing = false;
    }
}

pub(super) struct BioticView {
    pub(super) token: ProjectileToken,
    pub(super) parent: u32,
    pub(super) created: bool,
    pub(super) faded: Option<bool>,
    pub(super) retiring: bool,
    pub(super) stream: Stream,
}
