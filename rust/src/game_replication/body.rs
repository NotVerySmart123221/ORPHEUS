use super::{delivery::*, *};
use crate::script_control::LEGACY_PROJECTILE_CONTROL;

pub(super) struct StepBuffers<'a> {
    pub(super) frame: &'a mut u32,
    pub(super) fields: &'a mut Vec<(ProjectileToken, BioticField)>,
    pub(super) launches: &'a mut Vec<AdmittedFlight>,
    pub(super) hitscan: &'a mut Vec<PosedHitscan>,
}

pub(super) struct BodyScript {
    pub(super) id: BodyId,
    pub(super) stream: BodyStream<ProjectedSnapshot>,
    pub(super) frame: u32,
    pub(super) unsent_delta: Option<(u32, DeltaKind)>,
    pub(super) corrections: Option<CorrectionHistory>,
    pub(super) projectiles: Option<OwnerProjectiles>,
}

impl BodyScript {
    pub(super) fn new(
        body: Body,
        frame: u32,
        processed_at_spawn: Option<u32>,
        runtime: &dyn OwnerBodyRuntime,
    ) -> io::Result<Self> {
        let snapshot = runtime
            .snapshot()
            .ok_or_else(|| invalid("body VM is unavailable"))?;
        if snapshot.entity != body.id.entity {
            return Err(invalid("body VM has the wrong entity"));
        }
        let encoder = runtime.encoder();
        let projected = encoder.project(snapshot, View::Owner).map_err(invalid)?;
        encoder.full(&projected).map_err(invalid)?;
        let corrections = (matches!(body.hero, SOLDIER | CASSIDY)
            && runtime.movement_modifiers().is_some())
        .then(|| {
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
            projectiles,
        })
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
            runtime.advance(command, mover, step)?;
            *buffers.frame = command.frame;
            self.frame = command.frame;
        }
        match runtime.snapshot() {
            Some(snapshot) if snapshot.initialization_complete() => {}
            Some(snapshot) => {
                let sites: Vec<String> = snapshot
                    .diagnostics
                    .iter()
                    .map(|diagnostic| match diagnostic.site.node {
                        Some(node) => {
                            format!(
                                "{:04X}:{node}:{:?}",
                                diagnostic.site.graph, diagnostic.issue
                            )
                        }
                        None => format!("{:04X}:-:{:?}", diagnostic.site.graph, diagnostic.issue),
                    })
                    .collect();
                return Err(invalid(format!(
                    "body VM reached an unsupported state: {}",
                    sites.join(", ")
                )));
            }
            None => return Err(invalid("body VM has no snapshot")),
        }
        mover.set_cinematic_aim(runtime.cinematic_aim());
        for (frame, change) in runtime.take_velocity_changes() {
            mover.queue_velocity_change(frame, change);
        }
        if step == BodyStep::UnposedTimer {
            // Combat discards the neutral interval's reports at its boundary.
            // Drain interior reports early so a long gap cannot fill the VM queue.
            runtime.take_shots(None, mover);
            return Ok(());
        }
        if let Some(modifiers) = runtime.movement_modifiers() {
            mover.track_modifiers(command.frame, modifiers);
        } else if self.corrections.is_some() {
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
            return;
        };
        let bound = (projectile.ballistics.lifetime / TICK_PERIOD.as_secs_f64()).ceil() as u32;
        let Some(flight) = ProjectileFlight::new(
            projectile.ballistics,
            projectile.origin,
            projectile.direction,
            None,
        ) else {
            return;
        };
        let flight = if let Some(gravity) = projectile.gravity {
            flight.with_gravity(gravity)
        } else {
            flight
        };
        let flight = flight.with_expiry_blast(projectile.expiry_blast);
        if let Some(token) = projectiles.admit(projectile.shot, definition, bound) {
            launches.push(AdmittedFlight {
                token,
                flight,
                stun: projectile.stun,
                hit_effect: projectile
                    .stun
                    .and_then(|_| LEGACY_PROJECTILE_CONTROL.flight_site(&projectile.shot)),
            });
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
        let packet = frame(u32::MAX, u32::MAX, &[], &[], &[&chunk.update]).map_err(invalid)?;
        if packet.len() > MAX_PAYLOAD {
            return Err(invalid(
                "body script chunk exceeds the game datagram budget",
            ));
        }
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
        if let Some((report, projectiles)) = self.stream.sent(last).zip(self.projectiles.as_mut()) {
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
