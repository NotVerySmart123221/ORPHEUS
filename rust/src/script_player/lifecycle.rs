use std::{collections::BTreeMap, io, sync::Arc};

use serde_json::Value as Json;

use super::{
    unsupported,
    world::{BodyWorld, DeadeyeScene},
};
use crate::{
    commands::Command,
    game_combat::{
        BIOTIC_CANISTER, PosedBioticField, PosedHitscan, PosedProjectile, PosedShot, Stun,
        eye_height, script_eye_position,
    },
    game_content::is_junkenstein_mode,
    game_heroes::{CASSIDY, SOLDIER},
    game_hitscan::{HitDefinition, aim_direction},
    game_instance::TeamMemberCounts,
    game_projectiles::{ProjectileLeases, authored_number, local_prediction, predicted_sync},
    game_replication::{BodyStep, OwnerBodyRuntime, OwnerRoomActivity},
    junkenstein_messages::{DefenderMessage, DefenderSighting, SIGHTING},
    mover::Mover,
    mover_modifiers::{Modifier, ModifierKey},
    projectile_flight::{ProjectileBallistics, authored_gravity, lifted_direction},
    script_codecs::CodecTable,
    script_control::LEGACY_PROJECTILE_CONTROL,
    script_expr::to_float,
    script_init::{
        Component, HostEvent, HostHealthContext, InitFault, RoomMessage, ScriptedMotion, Snapshot,
    },
    script_runtime::FRAME_MS,
    script_snapshot_wire::SnapshotEncoder,
    script_wire::Value,
    vitals::{Damage, Death, Vitals},
    world::Component as EntityComponent,
};

pub(crate) struct PlayerBodyRuntime {
    pub(super) component: Component,
    pub(super) world: BodyWorld,
    pub(super) codecs: Arc<CodecTable>,
    pub(super) modifiers: Option<BTreeMap<ModifierKey, Modifier>>,
    pub(super) leases: ProjectileLeases,
    pub(super) frame: u32,
    pub(super) charge_binding: Option<super::charge::ChargeBinding>,
}

pub(super) const DEADEYE_GRAPH: u32 = 0x022f;
pub(super) const DEADEYE_TARGET_EFFECT: u32 = 0x01ee;

impl OwnerBodyRuntime for PlayerBodyRuntime {
    fn set_room_activity(&mut self, activity: OwnerRoomActivity) {
        self.world.room_activity = Some(activity);
    }

    fn set_encounter_active(&mut self, active: bool) -> io::Result<()> {
        if self
            .charge_binding
            .is_some_and(|binding| binding.is_configured())
            && self.world.room_activity == Some(OwnerRoomActivity::Regular)
        {
            // Regular rooms report no encounter and still admit configured charge.
            return self.sync_charge_admission(true);
        }
        // The room normally supplies this phase before synchronizing charge.
        // Keep direct calls usable by supplying it here when necessary.
        if self.charge_binding.is_some()
            && self
                .world
                .room_activity
                .map(OwnerRoomActivity::encounter_active)
                != Some(active)
        {
            self.world.room_activity = Some(OwnerRoomActivity::Encounter { active });
        }
        self.sync_charge_admission(active)
    }

    fn receive_applied_damage(
        &mut self,
        damage: crate::game_combat::AppliedDamage,
    ) -> io::Result<()> {
        self.credit_applied_damage(damage)
    }

    fn target_tags(&self) -> crate::script_init::TargetTagSnapshot {
        self.world
            .target_tag_effects
            .snapshot(self.world.target_tags.as_ref())
    }

    fn target_filter_flags(&self) -> Option<u64> {
        Some(self.world.own_filter_flags())
    }

    fn has_deadeye(&self) -> bool {
        self.world.has_deadeye()
    }

    fn set_deadeye_targets(&mut self, scene: crate::script_player::DeadeyeScene) {
        self.world.deadeye_scene = scene;
    }

    fn take_script_requests(&mut self) -> Vec<crate::game_combat::RoomScriptRequest> {
        self.world.script_requests.take()
    }

    fn receive_room_message(
        &mut self,
        sender: u32,
        message: RoomMessage,
    ) -> Result<usize, InitFault> {
        self.component.queue_room_message_from(sender, message)
    }

    fn set_spatial_frame(&mut self, frame: Arc<crate::game_combat::RoomSpatialFrame>) {
        self.world.spatial_frame = Some(frame);
    }

    fn set_team_member_counts(&mut self, counts: TeamMemberCounts) {
        self.world.team_member_counts = Some(counts);
    }

    fn set_defenders(&mut self, defenders: Arc<[u32]>) {
        self.world.defenders = defenders;
    }

    fn take_sightings(&mut self) -> Vec<DefenderSighting> {
        std::mem::take(&mut self.world.sightings)
    }

    fn receive_sighting(&mut self, message: DefenderSighting) -> io::Result<()> {
        if !is_junkenstein_mode(self.world.mode)
            || message.target != self.world.entity
            || self.world.vitals.death().is_some()
        {
            return Ok(());
        }
        self.component
            .queue_entity_message_from(message.source.entity, SIGHTING, message.parameters)
            .map_err(unsupported)?;
        self.world.sighting_source = Some(message.source);
        let result = self.component.run_now(&mut self.world).map_err(unsupported);
        self.world.sighting_source = None;
        result
    }

    fn receive_defender_message(&mut self, message: DefenderMessage) -> io::Result<usize> {
        if !is_junkenstein_mode(self.world.mode) || !message.accepts_body(self.world.entity) {
            return Ok(0);
        }
        let queued = self
            .component
            .queue_entity_message_from(message.source, message.message, message.parameters)
            .map_err(unsupported)?;
        self.component
            .run_now(&mut self.world)
            .map_err(unsupported)?;
        Ok(queued)
    }

    fn cinematic_aim(&self) -> Option<[i32; 2]> {
        self.world.cinematic_aim.map(|(_, aim)| aim)
    }

    fn authored_facing(&self) -> Option<[i32; 2]> {
        if self.world.vitals.death().is_some() {
            return None;
        }
        let (_, record) = crate::facing::winner(self.world.facing_claims.iter())?;
        crate::facing::angles(record.direction)
    }

    fn vitals(&self) -> Option<&Vitals> {
        Some(&self.world.vitals)
    }

    fn take_vitals(&mut self) -> Option<Vitals> {
        Some(std::mem::replace(&mut self.world.vitals, Vitals::empty()))
    }

    fn receive_host_event(
        &mut self,
        event: HostEvent,
        context: HostHealthContext,
    ) -> Result<usize, InitFault> {
        self.component.queue_host_event_with_context(event, context)
    }

    fn heal(&mut self, amount: f64) -> f64 {
        self.world.vitals.heal(amount)
    }

    fn receive_damage(&mut self, amount: f64, source: Death) -> Damage {
        let result = self.world.vitals.damage(amount, source);
        if result.killed {
            self.world.scripted_motion = None;
            self.world.velocity_changes.clear();
            self.component.set_owner_dead(true);
            if let Err(error) = self.sync_charge_admission(false) {
                eprintln!("Owner death charge cleanup failed: {error}");
            }
        }
        result
    }

    fn take_health_update(&mut self) -> Option<EntityComponent> {
        self.world.vitals.take_cumulative_update()
    }

    fn snapshot(&self) -> Option<&Snapshot> {
        self.component.snapshot()
    }

    #[cfg(test)]
    fn random_checkpoint(&self) -> Option<crate::script_init::RandomCheckpoint> {
        self.component
            .snapshot()
            .map(|_| self.component.random_checkpoint())
    }

    fn encoder(&self) -> SnapshotEncoder<'_> {
        SnapshotEncoder::new(&self.world.graphs, &self.codecs)
    }

    fn advance(&mut self, command: Command, mover: &Mover, step: BodyStep) -> io::Result<()> {
        if command.frame <= self.frame
            || (step != BodyStep::UnposedTimer && command.frame != self.frame.saturating_add(1))
        {
            return Err(unsupported(
                "the owner VM frame is outside the command sequence",
            ));
        }
        #[cfg(feature = "inspector")]
        if step != BodyStep::UnposedTimer
            && (step == BodyStep::UnposedBoundary
                || command.buttons != self.component.command_buttons()
                || command.action != 0)
            && crate::game_replication::owner_volley_trace_enabled()
        {
            eprintln!(
                "owner-volley input body={:08x} previous={} frame={} step={step:?} buttons={:#x}->{:#x} action={}",
                self.world.entity,
                self.frame,
                command.frame,
                self.component.command_buttons(),
                command.buttons,
                command.action
            );
        }
        let pose = mover.pose();
        self.world.position = pose.position;
        self.world.eye_position = script_eye_position(self.world.hero, mover);
        // Command gaps and timer advances retain the last movement record.
        if step == BodyStep::Posed {
            self.world.movement = pose;
        }
        if step == BodyStep::Posed {
            self.world.throttles = [command.right, command.forward];
            self.component
                .advance_command(command, &mut self.world)
                .map_err(unsupported)?;
        } else {
            let boundary = step == BodyStep::UnposedBoundary;
            if boundary {
                self.world.throttles = [0; 2];
            }
            self.component
                .advance_unposed(command.frame, boundary, &mut self.world)
                .map_err(unsupported)?;
        }
        if step != BodyStep::UnposedTimer {
            self.modifiers = self
                .component
                .movement_modifiers(&mut self.world)
                .map_err(unsupported)?;
        }
        self.frame = command.frame;
        Ok(())
    }

    fn frame_ms(&self) -> u32 {
        FRAME_MS as u32
    }

    fn take_velocity_changes(&mut self) -> Vec<(u32, crate::mover::VelocityChange)> {
        if self.world.vitals.death().is_some() {
            self.world.velocity_changes.clear();
        }
        std::mem::take(&mut self.world.velocity_changes)
    }

    fn take_scripted_motion(&mut self) -> Option<(u32, ScriptedMotion)> {
        self.world
            .scripted_motion
            .take()
            .filter(|_| self.world.vitals.death().is_none())
    }

    fn movement_modifiers(&self) -> Option<&BTreeMap<ModifierKey, Modifier>> {
        self.modifiers.as_ref()
    }

    fn projectile_leases(&self) -> Option<&ProjectileLeases> {
        (!self.leases.blocks().is_empty()).then_some(&self.leases)
    }

    fn take_shots(&mut self, command: Option<Command>, mover: &Mover) -> Vec<PosedShot> {
        let shots = self.component.take_shots().unwrap_or_default();
        #[cfg(feature = "inspector")]
        if !shots.is_empty() && crate::game_replication::owner_volley_trace_enabled() {
            let pose_frame = command.map(|command| command.frame);
            for shot in &shots {
                eprintln!(
                    "owner-volley vm-shot body={:08x} shot_frame={} pose_frame={pose_frame:?} instance={} state={} counter={} start_ms={} ordinal={}",
                    shot.entity,
                    shot.frame,
                    shot.instance,
                    shot.raw_state,
                    shot.activation,
                    shot.activation_ms,
                    shot.ordinal
                );
            }
        }
        let Some(command) = command else {
            return Vec::new();
        };
        let [x, y, z] = mover.pose().position;
        let hero = self.world.hero;
        let origin = [x, y + eye_height(hero, mover.crouched()), z];
        shots
            .into_iter()
            .filter_map(|shot| {
                let node = self.component.shot_node(&shot)?;
                let flashbang = LEGACY_PROJECTILE_CONTROL.matches_shot(hero, shot.graph, node.pos);
                let stun_duration = flashbang
                    .then(|| {
                        LEGACY_PROJECTILE_CONTROL
                            .duration(&self.world.graphs)
                            .cloned()
                    })
                    .flatten();
                // Python e1128509 declares this primary-only implicit policy;
                // it is not a recovered global native critical default.
                let default_critical = if (hero, shot.graph, node.pos) == (CASSIDY, 0x01d2, 31) {
                    2.0
                } else {
                    1.0
                };
                let mut evaluate = |config: &Json| {
                    self.component
                        .evaluate_shot_config(&shot, config, &mut self.world)
                };
                if shot.graph == 0x0257
                    && node.pos == 4
                    && hero == SOLDIER
                    && node
                        .field("m_49CBA3BC.m_entityDefinition")
                        .and_then(|config| evaluate(config).ok())
                        == Some(Value::Asset(0x0400_0000_0000_03dc))
                    && node
                        .field("m_projectileEntity.m_entityDef")
                        .and_then(|config| evaluate(config).ok())
                        == Some(Value::Asset(BIOTIC_CANISTER))
                    && self.leases.entity(BIOTIC_CANISTER, shot.frame, 0).is_some()
                {
                    return Some(PosedShot::BioticField(PosedBioticField {
                        shot,
                        feet: [x, y, z],
                    }));
                }
                let speed = node.field("m_projectileSpeed").unwrap_or(&Json::Null);
                if !speed.is_null() && to_float(&evaluate(speed).ok()?) != 0.0 {
                    // Admit only the known unconditional gravity path for Flashbang;
                    // other projectile motion/aim configurations remain unsupported.
                    let gravity = flashbang
                        .then(|| authored_gravity(&node.fields, &mut evaluate))
                        .flatten();
                    if !node.is_networked()
                        || !local_prediction(node)
                        || !predicted_sync(node, &mut evaluate).ok()?
                        || node
                            .field("m_projectileMotions")
                            .is_some_and(|v| !v.is_null() && gravity.is_none())
                        || node.field("m_aimID").is_some_and(|v| !v.is_null())
                        || authored_number(
                            node.field("m_numProjectilesPerShot"),
                            1.0,
                            &mut evaluate,
                        )
                        .ok()?
                            != 1.0
                    {
                        return None;
                    }
                    let ballistics =
                        ProjectileBallistics::from_authored(node, &mut evaluate).ok()??;
                    let definition = ballistics.definition?;
                    self.leases.entity(definition, shot.frame, 0)?;
                    if !ballistics.direct.is_some_and(|hit| hit.amount > 0.0)
                        && !ballistics
                            .blast
                            .as_ref()
                            .is_some_and(|blast| blast.hit.amount > 0.0)
                    {
                        return None;
                    }
                    // Preserve the existing rejection of unsupported mixed
                    // payloads outside the verified Flashbang zero-direct case.
                    if !flashbang
                        && (ballistics.direct.is_some_and(|hit| hit.amount <= 0.0)
                            || ballistics
                                .blast
                                .as_ref()
                                .is_some_and(|blast| blast.hit.amount <= 0.0))
                    {
                        return None;
                    }
                    let mut direction = aim_direction(&command);
                    if flashbang {
                        let lift = authored_number(
                            node.field("m_5EC1EB38.m_22CE7F30"),
                            0.0,
                            &mut evaluate,
                        )
                        .ok()?;
                        direction = lifted_direction(direction, lift)?;
                    }
                    let stun = if flashbang {
                        let effect = node.field("m_D2D11CE9.m_modifyHealth.m_277A4ECA")?;
                        if !LEGACY_PROJECTILE_CONTROL.has_effect_marker(&evaluate(effect).ok()?) {
                            return None;
                        }
                        Some(Stun::from_seconds(to_float(
                            &evaluate(stun_duration.as_ref()?).ok()?,
                        ))?)
                    } else {
                        None
                    };
                    return Some(PosedShot::Projectile(Box::new(PosedProjectile {
                        stun,
                        shot,
                        ballistics,
                        origin,
                        direction,
                        gravity,
                        // Scoped reconstruction shared with Python e1128509.
                        expiry_blast: flashbang && gravity.is_some(),
                    })));
                }
                let hit = HitDefinition::from_authored_with_critical(
                    node.field("m_6396149F")?,
                    default_critical,
                    &mut evaluate,
                )
                .ok()??;
                if hit.amount <= 0.0 {
                    return None;
                }
                let reach = node.field("m_FEC435C6").unwrap_or(&Json::Null);
                let reach = if reach.is_null() {
                    0.0
                } else {
                    to_float(&evaluate(reach).ok()?)
                };
                let direction = if (shot.graph, node.pos) == (DEADEYE_TARGET_EFFECT, 14) {
                    let snapshot = self.component.snapshot()?;
                    let Value::Entity(entity) = *snapshot
                        .instances
                        .get(&shot.instance)?
                        .variables
                        .get(&536)?
                    else {
                        return None;
                    };
                    let Value::Array(visible) = snapshot.variables.get(&5874)? else {
                        return None;
                    };
                    if !visible.contains(&Value::Entity(entity)) {
                        return None;
                    }
                    DeadeyeScene::shot_direction(&self.world.deadeye_scene, origin, entity)?
                } else {
                    aim_direction(&command)
                };
                Some(PosedShot::Hitscan(Box::new(PosedHitscan {
                    source: shot.entity,
                    origin,
                    direction,
                    command,
                    shot_frame: shot.frame,
                    reach,
                    hit,
                })))
            })
            .collect()
    }
}
