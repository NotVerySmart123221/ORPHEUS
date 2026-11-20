//! Deferred notifications for room-authoritative health changes.

use std::{collections::VecDeque, sync::Arc};

use bevy_ecs::{entity::Entity, world::World};
use indexmap::IndexMap;

use crate::{
    script_expr::valid_entity,
    script_init::{HealthTargetFacts, HitTags, HostEvent},
};

use super::{AppliedDamage, BotState, PlayerBodyState, RoomSimulation};

const MAX_ROOM_HEALTH_EVENTS: usize = 512;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct SampledBody {
    pub(super) wire: u32,
    pub(super) entity: Entity,
}

impl SampledBody {
    fn lookup(
        wire: u32,
        bots: &IndexMap<u32, Entity>,
        players: &IndexMap<u32, Entity>,
    ) -> Option<Self> {
        if !valid_entity(wire) {
            return None;
        }
        let &entity = bots.get(&wire).or_else(|| players.get(&wire))?;
        Some(Self { wire, entity })
    }

    fn health_target_facts(self, world: &World) -> Option<HealthTargetFacts> {
        if let Some(state) = world.get::<BotState>(self.entity) {
            return (state.bot.entity == self.wire).then(|| state.health_target_facts());
        }
        let state = world.get::<PlayerBodyState>(self.entity)?;
        (state.id.entity == self.wire).then(|| state.health_target_facts())
    }
}

pub(super) enum HealthEventContext<'a> {
    Unattributed,
    Impact(&'a AppliedDamage),
    Healing(&'a World),
}

struct HealthApplied {
    source: Option<SampledBody>,
    target: SampledBody,
    delta: f64,
    killed: bool,
    hit_tags: Option<HitTags>,
    target_facts: Option<Arc<HealthTargetFacts>>,
}

impl HealthApplied {
    fn resolve(&self, room: &RoomSimulation) -> Option<(HostEvent, [Option<SampledBody>; 2])> {
        let target = room.sample_health_body(self.target.wire);
        if target.is_some_and(|body| body != self.target) {
            // A replacement must not inherit a stale notification or its target ID.
            return None;
        }
        let source = self
            .source
            .filter(|&body| room.sample_health_body(body.wire) == Some(body));
        let event = HostEvent::HealthApplied {
            source: source.map(|body| body.wire),
            target: self.target.wire,
            delta: self.delta,
            killed: self.killed,
            hit_tags: self.hit_tags,
        };
        Some((event, [source.filter(|&body| body != self.target), target]))
    }
}

#[derive(Default)]
pub(super) struct RoomHealthEvents {
    enabled: bool,
    pending: VecDeque<HealthApplied>,
}

impl RoomHealthEvents {
    pub(super) fn stage(
        &mut self,
        source: Option<SampledBody>,
        target: SampledBody,
        delta: f64,
        killed: bool,
        context: HealthEventContext<'_>,
    ) {
        if !self.enabled
            || self.pending.len() >= MAX_ROOM_HEALTH_EVENTS
            || !delta.is_finite()
            || delta == 0.0
            || (killed && delta > 0.0)
        {
            return;
        }
        let (hit_tags, target_facts) = match context {
            HealthEventContext::Impact(impact) if impact.target_facts.target == target.wire => {
                (Some(impact.tags), Some(impact.target_facts.clone()))
            }
            // A self-heal or an unavailable source has no foreign observer
            // of the target in this notification.
            HealthEventContext::Healing(world)
                if delta > 0.0 && source.is_some_and(|source| source != target) =>
            {
                (None, target.health_target_facts(world))
            }
            _ => (None, None),
        };
        self.pending.push_back(HealthApplied {
            source,
            target,
            delta,
            killed,
            hit_tags,
            target_facts: target_facts.map(Arc::new),
        });
    }
}

impl RoomSimulation {
    pub(crate) fn enable_host_health_events(&mut self) {
        self.health_events.enabled = true;
    }

    pub(super) fn record_health_applied(
        &mut self,
        source: u32,
        target: u32,
        delta: f64,
        killed: bool,
        impact: Option<&AppliedDamage>,
    ) {
        if !self.health_events.enabled {
            return;
        }
        let Some(target) = self.sample_health_body(target) else {
            return;
        };
        let source = self.sample_health_body(source);
        let context = match impact {
            Some(impact) => HealthEventContext::Impact(impact),
            None if delta > 0.0 => HealthEventContext::Healing(&self.world),
            None => HealthEventContext::Unattributed,
        };
        self.health_events
            .stage(source, target, delta, killed, context);
    }

    /// Queue each event for the recipients' next VM frames, in settlement order.
    pub(super) fn flush_health_events(&mut self) {
        while let Some(change) = self.health_events.pending.pop_front() {
            if let Some((event, receivers)) = change.resolve(self) {
                for body in receivers.into_iter().flatten() {
                    self.queue_health_event(body.entity, event, change.target_facts.clone());
                }
            }
        }
    }

    pub(super) fn sample_health_body(&self, wire: u32) -> Option<SampledBody> {
        SampledBody::lookup(wire, &self.by_wire, &self.players_by_wire)
    }

    /// Collect already applied VM-local mutations before processing retirement.
    pub(super) fn collect_local_health_events(&mut self) {
        if !self.health_events.enabled {
            return;
        }
        for (&wire, &entity) in &self.by_wire {
            let Some(mut state) = self.world.get_mut::<BotState>(entity) else {
                continue;
            };
            let Some(enemy) = state.enemy.as_mut() else {
                continue;
            };
            let changes = enemy.scripts.take_local_health_changes();
            let target = SampledBody { wire, entity };
            for change in changes {
                let source =
                    SampledBody::lookup(change.source, &self.by_wire, &self.players_by_wire);
                // The VM has finished its local mutation. Use this committed body
                // snapshot when publishing the heal to a distinct room source.
                let context = if change.delta > 0.0 {
                    HealthEventContext::Healing(&self.world)
                } else {
                    HealthEventContext::Unattributed
                };
                self.health_events
                    .stage(source, target, change.delta, change.killed, context);
            }
        }
    }

    fn queue_health_event(
        &mut self,
        entity: Entity,
        event: HostEvent,
        target_facts: Option<Arc<HealthTargetFacts>>,
    ) {
        // A full or halted VM only drops its own notification.
        if let Some(mut state) = self.world.get_mut::<PlayerBodyState>(entity) {
            if state.alive() {
                if let Some(runtime) = &mut state.runtime {
                    let _ = runtime.receive_host_event(event, target_facts);
                }
            }
        } else if let Some(mut state) = self.world.get_mut::<BotState>(entity) {
            if !state.bot.dead && state.vitals.death().is_none() {
                if let Some(enemy) = state
                    .enemy
                    .as_mut()
                    .filter(|enemy| enemy.fault.is_none() && !enemy.scripts.retirement_requested)
                {
                    let _ = enemy
                        .scripts
                        .body
                        .queue_host_event_with_target_facts(event, target_facts);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "health_events/tests.rs"]
mod settled_hit_tests;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{
        game_combat::{HealthEffect, tests::fixture_bots},
        vitals::Death,
    };

    #[test]
    fn records_actual_settled_damage_and_healing() {
        let bots = fixture_bots();
        let source = bots[0].entity;
        let target = bots[1].entity;
        let mut room = RoomSimulation::new(bots).unwrap();
        let now = Duration::from_millis(48);
        let dealer = Death {
            killer: source,
            frame: 3,
            time: now.as_secs_f64(),
        };
        room.settle_health(target, HealthEffect::Damage(1.0), dealer, now)
            .unwrap();
        assert!(room.health_events.pending.is_empty());
        room.enable_host_health_events();

        for (effect, expected) in [
            (HealthEffect::Damage(24.0), Some((-24.0, false))),
            (HealthEffect::Heal(50.0), Some((25.0, false))),
            (HealthEffect::Heal(1.0), None),
            (HealthEffect::Damage(200.0), Some((-100.0, true))),
            (HealthEffect::Damage(1.0), None),
            (HealthEffect::Heal(1.0), None),
        ] {
            assert!(room.settle_health(target, effect, dealer, now).unwrap());
            let actual = room.health_events.pending.pop_front().map(|entry| {
                assert_eq!(entry.source.map(|body| body.wire), Some(source));
                assert_eq!(entry.target.wire, target);
                assert_eq!(entry.hit_tags, None);
                assert_eq!(entry.target_facts.is_some(), entry.delta > 0.0);
                (entry.delta, entry.killed)
            });
            assert_eq!(actual, expected);
            assert!(room.health_events.pending.is_empty());
        }
    }

    #[test]
    fn local_npc_healing_publishes_committed_target_facts() {
        use crate::{
            game_content::JUNKENSTEIN_REVENGE,
            game_hitscan::TargetPose,
            game_pools::HealthPools,
            npc_catalog::NpcDefinition,
            script_codecs::CodecTable,
            script_enemy::{EnemySpawn, EnemyTarget, WitchStaffSource},
            script_graph::GraphStore,
            script_init::{CreationKind, EntityCreation, OWNER_HEALTH},
            script_wire::Value,
        };
        use serde_json::{Value as Json, json};

        let fixture: Json = serde_json::from_str(include_str!(
            "../../../fixtures/behavior/script-local-health-outcomes-v1.json"
        ))
        .unwrap();
        let input = &fixture["input"]["local_heal"];
        let expected = &fixture["expected"]["local_heal"];
        let effect = json!({
            "index":121,"items":2,"fields":{},"containers":{},"unknown_nodes":[],
            "header":{"m_nodesBitCount":2,"m_statesBitCount":2,
                "m_remoteSyncNodesBitCount":2,"m_syncVarsBitCount":2},
            "states":[0],"entries":[1],"remote":[],"sync_vars":[],
            "nodes":[
                {"pos":0,"rid":0,"uid":0,"remote":-1,"state":0,"client_only":false,
                    "server_only":true,"class":"STUStatescriptStateModifyHealth",
                    "fields":{"m_2BBEEAB8":1,"m_ADEB6E05":0,
                        "m_modifyHealth":{"m_amount":input["requested_per_second"]}}},
                {"pos":1,"rid":1,"uid":1,"remote":-1,"state":null,"client_only":false,
                    "server_only":true,"class":"STUStatescriptEntry",
                    "fields":{"m_outPlug":{"$":"Plug","links":[[0,"m_beginPlug"]]}}}
            ]
        });
        let bundle = json!({
            "version":1,"graphs":[fixture["input"]["ai"]["listener_graph"],effect],
            "bodies":{},"missing":[],
            "junkenstein":{
                "pairs":{"0x02E0000000000901":{"body":"0x0400000000000902",
                    "controller":"0x0400000000000903"}},
                "entities":{
                    "0x0400000000000902":{"hero":"0x02E0000000000901","name":"Target",
                        "graphs":[{"graph":908,"m_75408640":0}],"weapons":[],
                        "target_tags":[input["target_tag"]]},
                    "0x0400000000000903":{"name":"Control","weapons":[],
                        "graphs":[{"graph":908,"m_75408640":0}]}
                }
            }
        });
        let graphs =
            Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap());
        let definition = NpcDefinition {
            hero: 0x02e0_0000_0000_0901,
            body: 0x0400_0000_0000_0902,
            name: "Target".into(),
            pools: HealthPools {
                health: 100.0,
                armour: 0.0,
                shields: 0.0,
            },
        };
        let bots = fixture_bots();
        let source = bots[0].entity;
        let mut room = RoomSimulation::new(bots).unwrap();
        room.enable_host_health_events();
        let target = room
            .create_enemy(EnemySpawn {
                graphs: Arc::clone(&graphs),
                codecs: Arc::new(CodecTable::from_json(r#"{"classes":{}}"#).unwrap()),
                definition: &definition,
                request: &EntityCreation {
                    position: [0.0; 3],
                    kind: CreationKind::Hero {
                        hero: definition.hero,
                        team: 1,
                        yaw: 0.0,
                        flags: [Value::Bool(false), Value::Bool(false)],
                        controller: None,
                        skin: None,
                    },
                },
                mode: JUNKENSTEIN_REVENGE,
                mode_variables: Arc::default(),
                tick: 3,
                walk: Default::default(),
                gate: None,
            })
            .unwrap();

        let state = room.state_mut(target).unwrap().unwrap();
        state.vitals.damage(
            input["wound"].as_f64().unwrap(),
            Death {
                killer: target,
                frame: 3,
                time: 0.048,
            },
        );
        let enemy = state.enemy.as_mut().unwrap();
        enemy.binding.roster = Some(Arc::new(vec![EnemyTarget {
            hero: None,
            filter_flags: None,
            tags: Default::default(),
            health: None,
            velocity: None,
            script_state: Default::default(),
            entity: source,
            team: 1,
            pose: TargetPose {
                feet: [0.0; 3],
                height: 1.0,
                radius: 0.25,
                eye: 0.75,
            },
            alive: true,
        }]));
        enemy.scripts.record_witch_staff_source(
            source,
            WitchStaffSource {
                controller: source,
                name: "Source".into(),
                health: [100.0, 0.0],
                capacity: [100.0, 0.0],
            },
        );
        enemy
            .scripts
            .sync_witch_staff(
                &graphs,
                &enemy.binding,
                &mut state.vitals,
                None,
                Some((source, 8.0)),
            )
            .unwrap();
        enemy
            .scripts
            .advance(&graphs, &enemy.binding, &mut state.vitals, 4)
            .unwrap();
        let committed_health = expected["committed_health"].as_f64().unwrap();
        assert_eq!(state.vitals.total(), committed_health);

        room.collect_local_health_events();
        let change = room.health_events.pending.pop_front().unwrap();
        let (event, _) = change.resolve(&room).unwrap();
        assert_eq!(
            event,
            HostEvent::HealthApplied {
                source: Some(source),
                target,
                delta: expected["actual_delta"].as_f64().unwrap(),
                killed: false,
                hit_tags: None,
            }
        );
        let facts = change.target_facts.unwrap();
        let tag = crate::accounts::parse_guid(input["target_tag"].as_str().unwrap()).unwrap();
        assert_eq!(facts.target, target);
        assert_eq!(facts.filter_flags, expected["target_filter_flags"].as_u64());
        assert_eq!(facts.tags.presence(tag), Some(true));
        assert_eq!(
            facts.script_state.read(OWNER_HEALTH),
            Some(Value::Float(committed_health))
        );
        room.collect_local_health_events();
        assert!(room.health_events.pending.is_empty());
    }

    #[test]
    fn field_pulses_record_one_capped_change_with_checked_source_per_body() {
        use crate::{
            contact_world::ContactWorld,
            game_combat::{BioticField, BotQueries, FIELD_DELAY, tests::fixture_ground},
            game_replication::BodyId,
            mover::Mover,
            mover_settings::{MoverCatalog, MoverSettings},
        };
        use std::sync::Arc;

        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/behavior/script-local-health-outcomes-v1.json"
        ))
        .unwrap();
        assert_eq!(
            fixture["case_id"],
            "script-local-health-outcomes.synthetic.v1"
        );
        let field = &fixture["input"]["field"];
        let wounds: [f64; 2] =
            serde_json::from_value(field["initial_two_actor_wounds"].clone()).unwrap();
        let deltas: [f64; 2] =
            serde_json::from_value(fixture["expected"]["field"]["actual_deltas"].clone()).unwrap();
        let elapsed = u32::try_from(field["pulse_ticks_elapsed"].as_u64().unwrap()).unwrap();

        let bots = fixture_bots();
        let targets = [bots[0].entity, bots[1].entity];
        let mut room = RoomSimulation::new(bots).unwrap();
        let owner = BodyId {
            entity: 702,
            generation: 1,
        };
        let owner_body = |id| {
            let mover = Mover::new(
                MoverSettings::default(),
                Arc::new(ContactWorld::new(Vec::new()).unwrap()),
                [0.0; 3],
                0,
                0x08,
            )
            .unwrap();
            PlayerBodyState::new(id, 701, 0, 1, 703, mover, None, 100, None)
        };
        room.spawn_player_body(owner_body(owner)).unwrap();
        room.enable_host_health_events();
        let mut field = BioticField::new([0.0; 3]);
        assert!(field.advance(100).0.is_none());
        let pulse = field.advance(100 + FIELD_DELAY + elapsed).0.unwrap();
        assert!(pulse.amount < wounds[1]);
        let pulses = [pulse, pulse];
        let ground = fixture_ground();
        let movers = MoverCatalog::default();
        let queries = BotQueries::new(false, &movers, &ground, None);

        let wound = |room: &mut RoomSimulation, target, amount| {
            room.state_mut(target).unwrap().unwrap().vitals.damage(
                amount,
                Death {
                    killer: owner.entity,
                    frame: 100,
                    time: 1.6,
                },
            );
        };
        for (target, amount) in targets.into_iter().zip(wounds) {
            wound(&mut room, target, amount);
        }
        let updates = room
            .heal_allies(&queries, 1, Some(owner.entity), Some(owner), &pulses)
            .unwrap();
        assert_eq!(
            updates
                .iter()
                .map(|update| update.entity)
                .collect::<Vec<_>>(),
            targets
        );
        for (wire, delta) in targets.into_iter().zip(deltas) {
            let event = room.health_events.pending.pop_front().unwrap();
            assert_eq!(event.source.map(|body| body.wire), Some(owner.entity));
            assert_eq!(
                (event.target.wire, event.delta, event.killed),
                (wire, delta, false)
            );
            assert_eq!(event.hit_tags, None);
            let facts = event.target_facts.as_ref().unwrap();
            assert_eq!(facts.target, wire);
            assert_eq!(
                facts.filter_flags,
                Some(1 | crate::match_scene::team_bit(1).unwrap())
            );
        }
        assert!(room.health_events.pending.is_empty());
        assert!(
            room.heal_allies(&queries, 1, Some(owner.entity), Some(owner), &pulses)
                .unwrap()
                .is_empty()
        );
        assert!(room.health_events.pending.is_empty());

        room.spawn_player_body(owner_body(BodyId {
            generation: 2,
            ..owner
        }))
        .unwrap();
        assert!(
            room.advance_player_fields(owner, 100 + FIELD_DELAY + elapsed)
                .is_none()
        );
        wound(&mut room, targets[0], wounds[0]);
        room.heal_allies(&queries, 1, Some(owner.entity), Some(owner), &pulses)
            .unwrap();
        let event = room.health_events.pending.pop_front().unwrap();
        assert!(event.source.is_none());
        assert!(event.target_facts.is_none());
        assert_eq!((event.target.wire, event.delta), (targets[0], deltas[0]));
        assert!(room.health_events.pending.is_empty());
    }

    #[test]
    fn bounds_pending_events_and_resolves_each_body_generation() {
        let bots = fixture_bots();
        let source = bots[0].entity;
        let target = bots[1].entity;
        let mut room = RoomSimulation::new(bots).unwrap();
        room.enable_host_health_events();

        for (delta, killed) in [
            (f64::NAN, false),
            (f64::INFINITY, false),
            (0.0, false),
            (-0.0, false),
            (1.0, true),
        ] {
            room.record_health_applied(source, target, delta, killed, None);
        }
        room.record_health_applied(source, 0, -1.0, false, None);
        assert!(room.health_events.pending.is_empty());

        room.record_health_applied(source, source, -1.0, false, None);
        let self_damage = room.health_events.pending.pop_front().unwrap();
        let (_, receivers) = self_damage.resolve(&room).unwrap();
        assert_eq!(receivers.into_iter().flatten().count(), 1);

        for amount in 1..=MAX_ROOM_HEALTH_EVENTS + 1 {
            room.record_health_applied(source, target, -(amount as f64), false, None);
        }
        assert_eq!(room.health_events.pending.len(), MAX_ROOM_HEALTH_EVENTS);
        let retired = room.by_wire.shift_remove(&target).unwrap();
        assert!(room.world.despawn(retired));
        let first = room.health_events.pending.pop_front().unwrap();
        let (event, receivers) = first.resolve(&room).unwrap();
        assert_eq!(
            event,
            HostEvent::HealthApplied {
                source: Some(source),
                target,
                delta: -1.0,
                killed: false,
                hit_tags: None,
            }
        );
        assert_eq!(
            receivers.map(|body| body.map(|body| body.wire)),
            [Some(source), None]
        );

        let replacement = room.world.spawn_empty().id();
        assert_ne!(retired, replacement);
        room.by_wire.insert(target, replacement);
        let last = room.health_events.pending.pop_back().unwrap();
        assert_eq!(last.delta, -(MAX_ROOM_HEALTH_EVENTS as f64));
        assert!(last.resolve(&room).is_none());
        room.flush_health_events();
        assert!(room.health_events.pending.is_empty());
    }
}
