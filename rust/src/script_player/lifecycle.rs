use std::{collections::BTreeMap, io, sync::Arc};

use serde_json::Value as Json;

use super::{
    unsupported,
    world::{BodyWorld, DeadeyeScene},
};
use crate::{
    commands::Command,
    game_combat::{
        BIOTIC_CANISTER, BIOTIC_FIELD_ENTITY, PosedBioticField, PosedHitscan, PosedProjectile,
        PosedShot, Stun, eye_height, script_eye_position,
    },
    game_content::is_junkenstein_mode,
    game_heroes::CASSIDY,
    game_hitscan::{HitDefinition, aim_direction},
    game_instance::TeamMemberCounts,
    game_parameters::{ClientWheelAction, ClientWheelUse},
    game_projectiles::{
        ImpactSpawn, ProjectileLeases, authored_number, local_prediction, predicted_sync,
    },
    game_replication::{BodyStep, OwnerBodyRuntime, OwnerRoomActivity},
    junkenstein_messages::{DefenderMessage, DefenderSighting, SIGHTING},
    mover::Mover,
    mover_modifiers::{Modifier, ModifierKey},
    projectile_flight::{ProjectileBallistics, authored_gravity, posed_projectile_direction},
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

    fn set_wheel_cosmetics(&mut self, cosmetics: crate::match_scene::WheelCosmetics) {
        self.world.wheel_cosmetics = cosmetics;
    }

    fn wheel_actions_available(&self) -> bool {
        self.world.graphs.has_asset_category_data() && self.component.snapshot().is_some()
    }

    fn queue_wheel_selection(&mut self, action: ClientWheelAction) -> Result<usize, InitFault> {
        if !self.world.graphs.has_asset_category_entry(action.asset()) {
            return Ok(0);
        }
        let selection = action.into_local_message();
        self.component.queue_entity_message_from(
            self.world.entity,
            selection.id,
            selection.parameters,
        )
    }

    fn queue_wheel_use(&mut self, report: ClientWheelUse) -> Result<usize, InitFault> {
        use crate::script_init::StateKind;
        if report.entity != self.world.entity {
            return Ok(0);
        }
        let Some(instance) = self
            .component
            .snapshot()
            .and_then(|snapshot| snapshot.instances.get(&report.instance))
        else {
            return Ok(0);
        };
        let states = || instance.states.values().filter(|state| state.active);
        if !states().any(|state| matches!(state.kind, StateKind::SprayPlacement(Some((unlock, _))) if unlock == report.unlock))
            || !states().any(|state| matches!(state.kind, StateKind::ClientPulse { .. }))
        {
            return Ok(0);
        }
        self.component
            .queue_local_instance_message(report.instance, report.into_local_message())
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

    fn receive_authored_hit_feedback(
        &mut self,
        event: HostEvent,
        context: &HostHealthContext,
        variant: f32,
    ) -> Result<usize, InitFault> {
        let result = self
            .component
            .queue_authored_hit_feedback(event, context, variant);
        #[cfg(feature = "inspector")]
        if crate::owner_trace::playtest_enabled() && !matches!(result, Ok(0)) {
            let HostEvent::HealthApplied { target, .. } = event;
            crate::owner_trace::playtest(format_args!(
                "combat hit presentation body={:08X} target={target:08X} game_frame={:?} vm_frame={} critical={:?} queued={result:?}",
                self.world.entity, context.damage_frame, self.frame, context.critical_hit
            ));
        }
        result
    }

    fn receive_authored_elimination_feedback(
        &mut self,
        target: Arc<crate::script_init::HealthTargetFacts>,
    ) -> Result<usize, InitFault> {
        #[cfg(feature = "inspector")]
        let victim = target.target;
        let result = self
            .component
            .queue_authored_elimination_feedback(target, true);
        #[cfg(feature = "inspector")]
        if crate::owner_trace::playtest_enabled() {
            crate::owner_trace::playtest(format_args!(
                "combat elimination presentation body={:08X} target={victim:08X} vm_frame={} queued={result:?}",
                self.world.entity, self.frame
            ));
        }
        result
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

    fn publish_death(
        &mut self,
        killer: u32,
        critical: bool,
        loadout: Option<u64>,
        source: Option<crate::game_combat::DeathSourceFacts>,
    ) -> Result<(), InitFault> {
        // A dead owner may stop sending commands. Execute the event at the
        // current VM clock so the observer projection can publish it immediately.
        if self
            .component
            .queue_body_death(killer, critical, loadout, None)?
            == 0
        {
            return Ok(());
        }
        self.world.death_source = source;
        let result = self.component.run_now(&mut self.world);
        self.world.death_source = None;
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
        if step != BodyStep::UnposedTimer && crate::owner_trace::enabled() {
            let buttons = self.component.command_buttons();
            let throttle = [command.right, command.forward];
            if step == BodyStep::UnposedBoundary
                || command.buttons != buttons
                || command.action != 0
                || (command.buttons != 0 && throttle != self.world.throttles)
            {
                crate::owner_trace::record(format_args!(
                    "owner-volley input body={:08x} previous={} frame={} step={step:?} buttons={buttons:#x}->{:#x} action={} right={} forward={}",
                    self.world.entity,
                    self.frame,
                    command.frame,
                    command.buttons,
                    command.action,
                    command.right,
                    command.forward
                ));
            }
        }
        let pose = mover.pose();
        #[cfg(feature = "inspector")]
        let before_effects = crate::owner_trace::playtest_enabled().then(|| {
            self.component
                .snapshot()
                .and_then(|snapshot| snapshot.frame_actions.back())
                .map_or(0, |action| action.callout.serial)
        });
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
        #[cfg(feature = "inspector")]
        if let Some((before, snapshot)) = before_effects.zip(self.component.snapshot()) {
            for action in snapshot
                .frame_actions
                .iter()
                .rev()
                .take_while(|action| action.callout.serial > before)
            {
                let callout = action.callout;
                let Some(instance) = snapshot.instances.get(&callout.instance) else {
                    continue;
                };
                let effect = self
                    .world
                    .graphs
                    .graph(instance.graph)
                    .and_then(|graph| graph.node(callout.position))
                    .filter(|node| node.class == "STUStatescriptActionEffect")
                    .and_then(|node| {
                        crate::script_graph::asset_guid(&node.fields["m_effect"]["m_effect"])
                    });
                if let Some(effect) = effect {
                    crate::owner_trace::playtest(format_args!(
                        "combat effect emitted body={:08X} vm_frame={} serial={} effect={effect:016X}",
                        snapshot.entity, self.frame, callout.serial
                    ));
                }
            }
        }
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
        let shots = self.component.take_reported_shots().unwrap_or_default();
        #[cfg(feature = "inspector")]
        if !shots.is_empty() && crate::owner_trace::enabled() {
            let pose_frame = command.map(|command| command.frame);
            for report in &shots {
                let shot = report.event;
                crate::owner_trace::record(format_args!(
                    "owner-volley vm-shot body={:08x} shot_frame={} pose_frame={pose_frame:?} instance={} state={} counter={} start_ms={} ordinal={}",
                    shot.entity,
                    shot.frame,
                    shot.instance,
                    shot.raw_state,
                    shot.activation,
                    shot.activation_ms,
                    shot.ordinal
                ));
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
            .filter_map(|report| {
                let shot = report.event;
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
                if ImpactSpawn::from_volley(node, &mut evaluate).ok().flatten()
                    == Some(ImpactSpawn {
                        definition: BIOTIC_FIELD_ENTITY,
                        on_spawn_graph: None,
                    })
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
                    let gravity = authored_gravity(&node.fields, &mut evaluate).ok()?;
                    if !node.is_networked()
                        || !local_prediction(node)
                        || !predicted_sync(node, &mut evaluate).ok()?
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
                    if ballistics.direct.is_some_and(|hit| hit.amount < 0.0)
                        || ballistics
                            .blast
                            .as_ref()
                            .is_some_and(|blast| blast.hit.amount < 0.0)
                    {
                        return None;
                    }
                    if !ballistics.direct.is_some_and(|hit| hit.amount > 0.0)
                        && !ballistics.blast.as_ref().is_some_and(|blast| {
                            blast.hit.amount > 0.0 || blast.knockback.is_some()
                        })
                    {
                        return None;
                    }
                    let direction =
                        posed_projectile_direction(node, aim_direction(&command), &mut evaluate)?;
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
                    crate::weapon_spread::authored_hitscan_direction(
                        node,
                        &report,
                        aim_direction(&command),
                        &mut evaluate,
                    )
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
