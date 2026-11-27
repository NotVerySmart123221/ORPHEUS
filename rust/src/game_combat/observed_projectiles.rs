//! Observer reports coupled to authoritative projectile lifetimes.

use std::sync::Arc;

use bevy_ecs::{component::Component, entity::Entity};

use crate::{
    game_clock::TICK_PERIOD, script_enemy::EnemyProjectile, script_snapshot_wire::FrozenVolley,
    static_ray::Vec3,
};

use super::RoomSimulation;

#[derive(Debug)]
pub(crate) struct ObservedShot {
    pub(crate) volley: FrozenVolley,
    pub(crate) definition: u64,
    pub(crate) origin: [f32; 3],
    pub(crate) direction: Vec3,
    pub(crate) launched: u32,
    pub(crate) lifetime_ticks: u32,
    pub(crate) skip_owner: bool,
}

impl ObservedShot {
    pub(crate) fn new(
        volley: FrozenVolley,
        definition: u64,
        origin: Vec3,
        direction: Vec3,
        launched: u32,
        lifetime: f64,
    ) -> Self {
        Self {
            volley,
            definition,
            origin: origin.map(|value| value as f32),
            direction,
            launched,
            lifetime_ticks: (lifetime / TICK_PERIOD.as_secs_f64()).ceil() as u32,
            skip_owner: false,
        }
    }

    pub(crate) fn with_skip_owner(mut self, skip_owner: bool) -> Self {
        self.skip_owner = skip_owner;
        self
    }
}

pub(super) struct StockShotBatch {
    pub(super) flights: Vec<EnemyProjectile>,
    pub(super) presentation: Option<ObservedShot>,
}

#[derive(Clone)]
pub(crate) enum ObservedProjectileEvent {
    Launched {
        id: u64,
        presentation: Arc<ObservedShot>,
        count: u16,
    },
    Completed {
        id: u64,
        ordinal: usize,
        frame: u32,
        impact: bool,
        point: Option<[f32; 3]>,
    },
}

#[derive(Component)]
struct ObservedFlight {
    batch: u64,
    ordinal: usize,
}

#[derive(Default)]
pub(super) struct RoomProjectileReports {
    next_id: u64,
    events: Vec<ObservedProjectileEvent>,
}

impl RoomSimulation {
    pub(super) fn spawn_admitted_launches(
        &mut self,
        launches: Vec<super::AdmittedFlight>,
        order: &mut Vec<Entity>,
    ) {
        let mut iter = launches.into_iter().peekable();
        while let Some(first) = iter.next() {
            let Some(presentation) = first.presentation.clone() else {
                order.push(self.world.spawn(first).id());
                continue;
            };
            let event_key = presentation.volley.event;
            let mut group = vec![first];
            while let Some(next) = iter.peek() {
                if next
                    .presentation
                    .as_ref()
                    .is_some_and(|p| p.volley.event == event_key)
                {
                    group.push(iter.next().expect("peeked launch"));
                } else {
                    break;
                }
            }
            let batch_id = u16::try_from(group.len())
                .ok()
                .filter(|&count| count > 0)
                .and_then(|count| {
                    let id = self.projectile_reports.next_id;
                    self.projectile_reports.next_id = id.checked_add(1)?;
                    self.projectile_reports
                        .events
                        .push(ObservedProjectileEvent::Launched {
                            id,
                            presentation,
                            count,
                        });
                    Some(id)
                });
            for (ordinal, launch) in group.into_iter().enumerate() {
                let mut entity = self.world.spawn(launch);
                if let Some(batch) = batch_id {
                    entity.insert(ObservedFlight { batch, ordinal });
                }
                order.push(entity.id());
            }
        }
    }

    pub(super) fn spawn_stock_shots(&mut self, batches: Vec<StockShotBatch>) {
        for StockShotBatch {
            flights,
            presentation,
        } in batches
        {
            let id = presentation.and_then(|presentation| {
                let count = u16::try_from(flights.len())
                    .ok()
                    .filter(|&count| count > 0)?;
                let id = self.projectile_reports.next_id;
                self.projectile_reports.next_id = id.checked_add(1)?;
                self.projectile_reports
                    .events
                    .push(ObservedProjectileEvent::Launched {
                        id,
                        presentation: Arc::new(presentation),
                        count,
                    });
                Some(id)
            });
            self.enemy_projectiles
                .extend(flights.into_iter().enumerate().map(|(ordinal, flight)| {
                    let mut entity = self.world.spawn(flight);
                    if let Some(id) = id {
                        entity.insert(ObservedFlight { batch: id, ordinal });
                    }
                    entity.id()
                }));
        }
    }

    pub(super) fn complete_observed_projectile(
        &mut self,
        entity: Entity,
        frame: u32,
        impact: bool,
        point: Option<Vec3>,
    ) {
        let Some(&ObservedFlight { batch, ordinal }) = self.world.get::<ObservedFlight>(entity)
        else {
            return;
        };
        self.projectile_reports
            .events
            .push(ObservedProjectileEvent::Completed {
                id: batch,
                ordinal,
                frame,
                impact,
                point: point.map(|p| p.map(|v| v as f32)),
            });
    }

    pub(crate) fn take_projectile_reports(&mut self) -> Vec<ObservedProjectileEvent> {
        std::mem::take(&mut self.projectile_reports.events)
    }
}
