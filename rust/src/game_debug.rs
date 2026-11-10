//! Read-only simulation facts copied by the game worker for local diagnostics.

use serde::Serialize;

use crate::game_timing::WorkerTiming;

#[derive(Serialize)]
pub(crate) struct Snapshot {
    pub tick: u32,
    pub rooms: Vec<Room>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timing: Option<WorkerTiming>,
}

#[derive(Serialize)]
pub(crate) struct Room {
    pub id: String,
    pub map: String,
    pub name: String,
    pub actors: Vec<Actor>,
}

#[derive(Serialize)]
pub(crate) struct Actor {
    pub entity: u32,
    pub generation: Option<u64>,
    pub kind: ActorKind,
    pub team: u8,
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub alive: bool,
    pub health: Option<f64>,
    pub script_active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_contact_clip: Option<ContactClip>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct ContactClip {
    pub tick: u32,
    pub proxy_id: u32,
    pub category: u16,
    pub fraction: f32,
    pub before: [f64; 3],
    pub intended: [f64; 3],
    pub after: [f64; 3],
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActorKind {
    Player,
    Bot,
}
