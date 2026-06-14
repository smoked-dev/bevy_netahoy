//! Demo world, spawn points, and player cosmetics shared by the example
//! client and server. Not part of the bevy_netahoy library.
#![allow(dead_code)]

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_ahoy::prelude::*;

pub const WORLD_COLLISION_LAYER: LayerMask = LayerMask(1 << 0);
pub const PLAYER_COLLISION_LAYER: LayerMask = LayerMask(1 << 1);
pub const SPAWN_POINT: Vec3 = Vec3::new(0.0, 2.2, 8.0);
pub const FLYING_TARGET_PLAYER_ID: u64 = 9_001;

#[derive(Clone, Copy)]
pub struct WorldBox {
    pub name: &'static str,
    pub translation: Vec3,
    pub size: Vec3,
    pub rotation: Quat,
    pub color: Color,
}

pub fn world_boxes() -> [WorldBox; 6] {
    [
        WorldBox {
            name: "floor",
            translation: Vec3::new(0.0, -0.2, 0.0),
            size: Vec3::new(34.0, 0.4, 34.0),
            rotation: Quat::IDENTITY,
            color: Color::srgb(0.18, 0.20, 0.22),
        },
        WorldBox {
            name: "surf ramp",
            translation: Vec3::new(0.0, 1.0, -2.0),
            size: Vec3::new(5.0, 0.35, 14.0),
            rotation: Quat::from_rotation_x(-0.36),
            color: Color::srgb(0.42, 0.58, 0.68),
        },
        WorldBox {
            name: "left bank",
            translation: Vec3::new(-7.0, 1.8, -6.0),
            size: Vec3::new(10.0, 0.35, 8.0),
            rotation: Quat::from_rotation_z(0.55),
            color: Color::srgb(0.30, 0.45, 0.55),
        },
        WorldBox {
            name: "right bank",
            translation: Vec3::new(7.0, 1.8, -6.0),
            size: Vec3::new(10.0, 0.35, 8.0),
            rotation: Quat::from_rotation_z(-0.55),
            color: Color::srgb(0.30, 0.45, 0.55),
        },
        WorldBox {
            name: "mantle block",
            translation: Vec3::new(-4.0, 0.8, 5.0),
            size: Vec3::new(3.0, 1.6, 2.0),
            rotation: Quat::IDENTITY,
            color: Color::srgb(0.38, 0.45, 0.30),
        },
        WorldBox {
            name: "reset platform",
            translation: Vec3::new(0.0, 0.45, 12.0),
            size: Vec3::new(5.0, 0.5, 4.0),
            rotation: Quat::IDENTITY,
            color: Color::srgb(0.45, 0.38, 0.30),
        },
    ]
}

pub fn spawn_world_colliders(commands: &mut Commands) {
    for world_box in world_boxes() {
        commands.spawn((
            Name::new(world_box.name),
            Transform {
                translation: world_box.translation,
                rotation: world_box.rotation,
                ..default()
            },
            RigidBody::Static,
            Collider::cuboid(world_box.size.x, world_box.size.y, world_box.size.z),
            CollisionLayers::new(WORLD_COLLISION_LAYER, LayerMask::ALL),
        ));
    }
}

pub fn spawn_world_render(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    for world_box in world_boxes() {
        commands.spawn((
            Name::new(world_box.name),
            Mesh3d(meshes.add(Cuboid::new(
                world_box.size.x,
                world_box.size.y,
                world_box.size.z,
            ))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: world_box.color,
                perceptual_roughness: 0.85,
                ..default()
            })),
            Transform {
                translation: world_box.translation,
                rotation: world_box.rotation,
                ..default()
            },
        ));
    }
}

pub fn player_controller() -> CharacterController {
    CharacterController {
    //    filter: SpatialQueryFilter::from_mask(WORLD_COLLISION_LAYER),
        acceleration_hz: 10.0,
        air_acceleration_hz: 120.0,
        speed: 6.5,
        gravity: 23.0,
        friction_hz: 4.0,
        ..default()
    }
}

pub fn player_collision_layers() -> CollisionLayers {
    CollisionLayers::new(PLAYER_COLLISION_LAYER, LayerMask::ALL)
}

pub fn player_spawn_point(player_id: u64) -> Vec3 {
    let index = player_id.saturating_sub(1) as f32;
    SPAWN_POINT + Vec3::new(index * 1.4, 0.0, 0.0)
}

pub fn player_color(player_id: u64) -> Color {
    Color::hsv((player_id.wrapping_mul(137) % 360) as f32, 0.55, 0.9)
}
