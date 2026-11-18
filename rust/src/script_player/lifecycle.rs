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
        eye_height,
    },
    game_content::is_junkenstein_mode,
    game_heroes::{CASSIDY, SOLDIER},
    game_hitscan::{HitDefinition, aim_direction},
    game_projectiles::{ProjectileLeases, authored_number, local_prediction, predicted_sync},
    game_replication::{BodyStep, OwnerBodyRuntime},
    junkenstein_messages::{DefenderMessage, DefenderSighting, SIGHTING},
    mover::Mover,
    mover_modifiers::{Modifier, ModifierKey},
    projectile_flight::{ProjectileBallistics, authored_gravity, lifted_direction},
    script_codecs::CodecTable,
    script_expr::to_float,
    script_graph::GraphStore,
    script_init::{Component, Snapshot},
    script_runtime::FRAME_MS,
    script_snapshot_wire::SnapshotEncoder,
    script_wire::{Value, Variables},
    vitals::{Death, Vitals},
    world::Component as EntityComponent,
};

pub(crate) struct PlayerBodyRuntime {
    pub(super) encounter_active: bool,
    pub(super) component: Component,
    pub(super) world: BodyWorld,
    pub(super) codecs: Arc<CodecTable>,
    pub(super) modifiers: Option<BTreeMap<ModifierKey, Modifier>>,
    pub(super) leases: ProjectileLeases,
    pub(super) frame: u32,
}

pub(super) fn flashbang_duration(graphs: &GraphStore) -> Option<&Json> {
    graphs
        .graph(0x0228)?
        .node(7)?
        .field("m_69F4B52C.m_6525E98B.m_1EB5A024")?
        .as_array()?
        .iter()
        .find(|row| {
            row.get("m_0D09D2D9")
                .and_then(crate::script_graph::asset_guid)
                == Some(0x0d80_0000_0000_00d7)
        })?
        .get("m_value")
}

pub(super) const DEADEYE_GRAPH: u32 = 0x022f;
pub(super) const DEADEYE_TARGET_EFFECT: u32 = 0x01ee;

impl PlayerBodyRuntime {
    pub(super) fn sync_charge_admission(&mut self, active: bool) -> io::Result<()> {
        if !self.world.has_deadeye() {
            return Ok(());
        }
        self.encounter_active = active;
        let enabled = active && self.world.vitals.death().is_none();
        let variable = crate::script_expr::Variable {
            scope: crate::script_expr::Scope::Entity,
            id: 2426,
        };
        if self.component.resolved_variable(0, variable) != Some(Value::Bool(enabled)) {
            // The room owns this live defender capability. The authored 0B8B
            // graph owns the passive wait, amount, multipliers, damage gates
            // and cap; the former 60-second host fill is intentionally removed.
            self.component
                .set_entity_variable(2426, Value::Bool(enabled), &mut self.world)
                .map_err(unsupported)?;
            self.component
                .run_now(&mut self.world)
                .map_err(unsupported)?;
        }
        Ok(())
    }
}

impl OwnerBodyRuntime for PlayerBodyRuntime {
    fn set_encounter_active(&mut self, active: bool) -> io::Result<()> {
        self.sync_charge_admission(active)
    }

    fn receive_applied_damage(
        &mut self,
        damage: crate::game_combat::AppliedDamage,
    ) -> io::Result<()> {
        if !self.world.has_deadeye()
            || !self.encounter_active
            || damage.source != self.world.entity
            || self.world.vitals.death().is_some()
            || !damage.amount.is_finite()
            || damage.amount <= 0.0
        {
            return Ok(());
        }
        if self.component.snapshot().is_some_and(|snapshot| {
            snapshot.instances.values().any(|instance| {
                instance.graph == DEADEYE_GRAPH
                    && instance
                        .states
                        .get(&0)
                        .is_some_and(crate::script_init::State::ability_running)
            })
        }) {
            return Ok(());
        }
        let instance = self
            .component
            .snapshot()
            .and_then(|snapshot| {
                snapshot
                    .instances
                    .iter()
                    .find(|(_, instance)| instance.graph == 0x0b8b)
                    .map(|(&id, _)| id)
            })
            .ok_or_else(|| {
                unsupported("damage credit requires the authored common charge graph")
            })?;
        let parameters = Variables::from([
            (0x13, Value::Float(f64::from(damage.amount as f32))),
            (0x15, Value::Entity(damage.target)),
            (
                0x3d,
                Value::Array(
                    damage
                        .tags
                        .as_slice()
                        .iter()
                        .copied()
                        .map(Value::Asset)
                        .collect(),
                ),
            ),
            // AppliedDamage is net pool loss; no gross mitigation contributions
            // remain to subtract in the authored detail fold.
            (0x624, Value::Array(Vec::new())),
        ]);
        self.component
            .queue_message(instance, 0x0240_0000_0000_0012, parameters)
            .map_err(unsupported)?;
        self.world.damage_credit = Some(damage);
        let result = self.component.run_now(&mut self.world).map_err(unsupported);
        self.world.damage_credit = None;
        result
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

    fn vitals(&self) -> Option<&Vitals> {
        Some(&self.world.vitals)
    }

    fn heal(&mut self, amount: f64) {
        self.world.vitals.heal(amount);
    }

    fn receive_damage(&mut self, amount: f64, source: Death) -> bool {
        let result = self.world.vitals.damage(amount, source);
        if result.killed {
            self.component.set_owner_dead(true);
            if let Err(error) = self.sync_charge_admission(self.encounter_active) {
                eprintln!("Owner death charge cleanup failed: {error}");
            }
        }
        result.killed
    }

    fn take_health_update(&mut self) -> Option<EntityComponent> {
        let vitals = &mut self.world.vitals;
        if !vitals.has_changed() {
            return None;
        }
        vitals.clear_changed();
        Some(vitals.cumulative_component())
    }

    fn snapshot(&self) -> Option<&Snapshot> {
        self.component.snapshot()
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
        let pose = mover.pose();
        self.world.position = pose.position;
        // Command gaps and timer advances retain the last movement record.
        if step == BodyStep::Posed {
            self.world.movement = pose;
        }
        if step == BodyStep::UnposedTimer {
            self.component
                .advance_frame(command.frame, &mut self.world)
                .map_err(unsupported)?;
        } else {
            self.world.throttles = [command.right, command.forward];
            self.component
                .advance_command(command, &mut self.world)
                .map_err(unsupported)?;
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

    fn movement_modifiers(&self) -> Option<&BTreeMap<ModifierKey, Modifier>> {
        self.modifiers.as_ref()
    }

    fn projectile_leases(&self) -> Option<&ProjectileLeases> {
        (!self.leases.blocks().is_empty()).then_some(&self.leases)
    }

    fn take_shots(&mut self, command: Option<Command>, mover: &Mover) -> Vec<PosedShot> {
        let shots = self.component.take_shots().unwrap_or_default();
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
                // 0228/7 supplies the duration override for the Flashbang effect,
                // including the mode-dependent branch. Keep policy in authored data.
                let stun_duration = (hero == CASSIDY && (shot.graph, node.pos) == (0x01e9, 7))
                    .then(|| flashbang_duration(&self.world.graphs).cloned())
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
                    let flashbang = hero == CASSIDY && (shot.graph, node.pos) == (0x01e9, 7);
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
                        if evaluate(effect).ok()? != Value::Asset(0x04a0_0000_0000_0079) {
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
