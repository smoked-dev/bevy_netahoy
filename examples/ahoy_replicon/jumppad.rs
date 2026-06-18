use bevy::prelude::*;
use bevy_netahoy::{LaunchZone, LaunchZones};

pub const JUMP_PAD_TRANSLATION: Vec3 = Vec3::new(8.0, 0.15, 7.0);
pub const JUMP_PAD_SIZE: Vec3 = Vec3::new(3.0, 0.3, 3.0);

const PLAYER_CAPSULE_HALF_HEIGHT: f32 = 0.75;
const JUMP_PAD_VERTICAL_SPEED: f32 = 50.0;
const JUMP_PAD_TRIGGER_HALF_EXTENTS: Vec3 =
    Vec3::new(JUMP_PAD_SIZE.x * 0.5, 0.3, JUMP_PAD_SIZE.z * 0.5);

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct JumpPad;

/// Register each jump pad as a static [`LaunchZone`] the netcode applies inside
/// the movement step, so it re-derives identically during prediction, replay,
/// and on the server. Pads are static, so this runs once after they spawn.
pub fn register_jump_pad_zones(pads: Query<&Transform, With<JumpPad>>, mut zones: ResMut<LaunchZones>) {
    for pad in &pads {
        zones.0.push(LaunchZone {
            center: pad.translation
                + Vec3::Y * (JUMP_PAD_SIZE.y * 0.5 + PLAYER_CAPSULE_HALF_HEIGHT),
            half_extents: JUMP_PAD_TRIGGER_HALF_EXTENTS,
            launch_speed: JUMP_PAD_VERTICAL_SPEED,
        });
    }
}
