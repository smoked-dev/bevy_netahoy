use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_netahoy::{AhoyButtons, AhoyUserCmd, MoveView, MovementEffects};

use crate::shared::JUMP_PAD_COLLISION_LAYER;

pub const JUMP_PAD_TRANSLATION: Vec3 = Vec3::new(8.0, 0.15, 7.0);
pub const JUMP_PAD_SIZE: Vec3 = Vec3::new(3.0, 0.3, 3.0);

const PLAYER_CAPSULE_HALF_HEIGHT: f32 = 0.75;
const JUMP_PAD_VERTICAL_SPEED: f32 = 50.0;
const JUMP_PAD_TRIGGER_HALF_EXTENTS: Vec3 =
    Vec3::new(JUMP_PAD_SIZE.x * 0.5, 0.3, JUMP_PAD_SIZE.z * 0.5);

/// Spawn the invisible trigger volume for a jump pad: a static `Sensor` box
/// centered where a standing player's center sits, on the jump-pad layer. It's a
/// sensor, so the KCC ignores it for movement; the [`jump_pad`] effect detects it
/// with an explicit point query. The pad's *visual* comes from the world render.
pub fn spawn_jump_pad_trigger(commands: &mut Commands, base: &Transform) {
    let center =
        base.translation + Vec3::Y * (JUMP_PAD_SIZE.y * 0.5 + PLAYER_CAPSULE_HALF_HEIGHT);
    let full = JUMP_PAD_TRIGGER_HALF_EXTENTS * 2.0;
    commands.spawn((
        Name::new("jump pad trigger"),
        Transform::from_translation(center),
        RigidBody::Static,
        Collider::cuboid(full.x, full.y, full.z),
        Sensor,
        CollisionLayers::new(JUMP_PAD_COLLISION_LAYER, LayerMask::NONE),
    ));
}

pub fn register_jump_pad_effect(mut effects: ResMut<MovementEffects>) {
    effects.0.push(jump_pad);
}

/// While the player's center is inside a jump-pad trigger, set vertical velocity.
/// Set (not add) is idempotent across contact ticks and re-derives from position
/// on replay, so it needs no edge detection. Detection rides the static jump-pad
/// collider layer, so it's deterministic.
fn jump_pad(
    view: MoveView,
    _command: &AhoyUserCmd,
    _previous_buttons: AhoyButtons,
    world: &SpatialQuery,
    velocity: &mut Vec3,
) {
    let filter = SpatialQueryFilter::from_mask(JUMP_PAD_COLLISION_LAYER);
    let mut on_pad = false;
    world.point_intersections_callback(view.position, &filter, |_| {
        on_pad = true;
        false // first hit is enough
    });
    if on_pad {
        velocity.y = JUMP_PAD_VERTICAL_SPEED;
    }
}
