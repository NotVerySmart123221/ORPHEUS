//! Observer reports coupled to authoritative projectile lifetimes.

use std::{collections::HashMap, sync::Arc};

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
}

impl ObservedShot {
    pub(super) fn new(
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
        }
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
        frame: u32,
    },
}

#[derive(Component)]
struct ObservedBatch(u64);

#[derive(Default)]
pub(super) struct RoomProjectileReports {
    next_id: u64,
    pending: HashMap<u64, u16>,
    events: Vec<ObservedProjectileEvent>,
}

impl RoomSimulation {
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
                self.projectile_reports.pending.insert(id, count);
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
                .extend(flights.into_iter().map(|flight| {
                    let mut entity = self.world.spawn(flight);
                    if let Some(id) = id {
                        entity.insert(ObservedBatch(id));
                    }
                    entity.id()
                }));
        }
    }

    pub(super) fn complete_observed_projectile(&mut self, entity: Entity, frame: u32) {
        let Some(&ObservedBatch(id)) = self.world.get::<ObservedBatch>(entity) else {
            return;
        };
        let Some(remaining) = self.projectile_reports.pending.get_mut(&id) else {
            return;
        };
        *remaining -= 1;
        if *remaining == 0 {
            self.projectile_reports.pending.remove(&id);
            self.projectile_reports
                .events
                .push(ObservedProjectileEvent::Completed { id, frame });
        }
    }

    pub(crate) fn take_projectile_reports(&mut self) -> Vec<ObservedProjectileEvent> {
        std::mem::take(&mut self.projectile_reports.events)
    }
}
