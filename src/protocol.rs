//! Wire types, replicated components, and the protocol plugin both peers add.

use std::{
    cmp::Ordering,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_ahoy::{MantleState, prelude::*};
use bevy_replicon::prelude::*;
use serde::{Deserialize, Serialize};

pub const DEFAULT_PORT: u16 = 5000;
pub const FIXED_TIMESTEP_HZ: f64 = 20.0;
pub const DEFAULT_SERVER_ADDR: SocketAddr =
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DEFAULT_PORT);
pub const DEFAULT_SERVER_URL: &str = "ws://127.0.0.1:5000";
pub const PLAYER_CAPSULE_RADIUS: f32 = 0.45;
pub const PLAYER_CAPSULE_HALF_HEIGHT: f32 = 0.75;

#[derive(Default)]
pub struct NetAhoyProtocolPlugin;

impl Plugin for NetAhoyProtocolPlugin {
    fn build(&self, app: &mut App) {
        app.replicate::<NetworkedPlayer>()
            .replicate::<PlayerId>()
            .replicate::<AhoySnapshot>()
            .replicate_filtered::<Transform, Without<AhoySnapshot>>()
            .replicate_filtered::<LinearVelocity, Without<AhoySnapshot>>()
            .add_client_event::<JoinRequest>(Channel::Ordered)
            .add_server_event::<JoinAccepted>(Channel::Ordered)
            .add_client_event::<AhoyUserCmdPacket>(Channel::Unreliable);
    }
}

#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct NetworkedPlayer;

#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PlayerId(pub u64);

#[derive(Event, Serialize, Deserialize, Clone, Copy, Debug)]
pub struct JoinRequest;

#[derive(Event, Serialize, Deserialize, Clone, Copy, Debug)]
pub struct JoinAccepted {
    pub player_id: u64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct AhoyButtons {
    pub jump: bool,
    pub crouch: bool,
    pub tac: bool,
    pub mantle: bool,
    pub crane: bool,
    pub climbdown: bool,
    pub swim_up: bool,
    /// Pragmatic seam for the demo's rocket-jump ability. Generalize to a small
    /// button bitset if more game abilities ever need their own command bits.
    pub fire_rocket: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct AhoyUserCmd {
    pub sequence: u32,
    pub movement: Vec2,
    pub look: Vec2,
    pub buttons: AhoyButtons,
}

#[derive(Event, Serialize, Deserialize, Clone, Debug, Default)]
pub struct AhoyUserCmdPacket {
    pub commands: Vec<AhoyUserCmd>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct NetAhoyMoveState {
    pub grounded: bool,
    pub crouching: bool,
    pub mantle_height_left: Option<f32>,
    pub crane_height_left: Option<f32>,
}

impl NetAhoyMoveState {
    pub fn from_controller_state(state: &CharacterControllerState) -> Self {
        Self {
            grounded: state.grounded.is_some(),
            crouching: state.crouching,
            mantle_height_left: state.mantle.as_ref().map(|mantle| mantle.height_left),
            crane_height_left: state.crane_height_left,
        }
    }

    pub fn apply_to_controller_state(&self, state: &mut CharacterControllerState) {
        state.crouching = self.crouching;
        state.crane_height_left = self.crane_height_left;
        state.mantle = self
            .mantle_height_left
            .map(|height_left| MantleState { height_left });

        if self.grounded {
            state.last_ground.reset();
        } else {
            state.grounded = None;
        }
    }
}

#[derive(Component, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct AhoySnapshot {
    pub server_tick: u64,
    pub last_processed_sequence: u32,
    pub last_processed_buttons: AhoyButtons,
    pub position: Vec3,
    pub velocity: Vec3,
    pub look: Vec2,
    pub state: NetAhoyMoveState,
}

pub fn sequence_is_newer(incoming: u32, current: u32) -> bool {
    incoming != current && incoming.wrapping_sub(current) < (u32::MAX / 2)
}

pub fn sequence_cmp(a: u32, b: u32) -> Ordering {
    if a == b {
        Ordering::Equal
    } else if sequence_is_newer(a, b) {
        Ordering::Greater
    } else {
        Ordering::Less
    }
}
