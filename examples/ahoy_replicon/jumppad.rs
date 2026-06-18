use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_ahoy::CharacterController;

pub const JUMP_PAD_TRANSLATION: Vec3 = Vec3::new(8.0, 0.15, 7.0);
pub const JUMP_PAD_SIZE: Vec3 = Vec3::new(3.0, 0.3, 3.0);

const PLAYER_CAPSULE_HALF_HEIGHT: f32 = 0.75;
const JUMP_PAD_VERTICAL_SPEED: f32 = 50.0;
const JUMP_PAD_TRIGGER_HALF_EXTENTS: Vec3 =
    Vec3::new(JUMP_PAD_SIZE.x * 0.5, 0.3, JUMP_PAD_SIZE.z * 0.5);

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct JumpPad;

pub fn apply_jump_pads(
    pads: Query<&Transform, With<JumpPad>>,
    mut players: Query<
        (&Transform, &mut LinearVelocity),
        (With<CharacterController>, Without<JumpPad>),
    >,
) {
    for (player_transform, mut velocity) in &mut players {
        if pads
            .iter()
            .any(|pad_transform| contains_player(pad_transform, player_transform.translation))
        {
            velocity.y = JUMP_PAD_VERTICAL_SPEED;
        }
    }
}

fn contains_player(pad_transform: &Transform, player_position: Vec3) -> bool {
    let center =
        pad_transform.translation + Vec3::Y * (JUMP_PAD_SIZE.y * 0.5 + PLAYER_CAPSULE_HALF_HEIGHT);

    (player_position - center)
        .abs()
        .cmple(JUMP_PAD_TRIGGER_HALF_EXTENTS)
        .all()
}
