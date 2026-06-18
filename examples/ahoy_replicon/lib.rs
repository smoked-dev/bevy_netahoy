use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Event, Serialize, Deserialize, Clone, Copy, Debug)]
pub struct HitScanShot {
    pub shot_id: u32,
    pub client_sample_tick: u64,
    pub client_sample_alpha: f32,
    pub origin: Vec3,
    pub direction: Vec3,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct HitScanHit {
    pub player_id: u64,
    pub position: Vec3,
    pub distance: f32,
}

#[derive(Event, Serialize, Deserialize, Clone, Copy, Debug)]
pub struct HitScanAck {
    pub shot_id: u32,
    pub server_tick: u64,
    pub client_sample_tick: u64,
    pub client_sample_alpha: f32,
    pub hit: Option<HitScanHit>,
}
