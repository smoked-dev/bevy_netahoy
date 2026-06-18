//! Interpolation timing and capsule-cast math shared by client interpolation
//! and server lag compensation.

use std::collections::VecDeque;

use avian3d::prelude::{Collider, Position, Rotation};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::protocol::{AhoySnapshot, NetAhoyMoveState};

pub const REMOTE_INTERPOLATION_DISCONTINUITY_TICKS: u64 = 20;
pub const REMOTE_INTERPOLATION_TELEPORT_DISTANCE: f32 = 8.0;

/// A point on the server timeline: whole tick plus a fraction into the next.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RemoteRenderTime {
    pub tick: u64,
    pub alpha: f32,
}

impl RemoteRenderTime {
    pub fn new(tick: u64, alpha: f32) -> Self {
        Self::from_ticks_f64((tick as f64 + alpha as f64).max(0.0))
    }

    pub fn from_seconds(seconds: f64, tick_hz: f64) -> Self {
        Self::from_ticks_f64((seconds.max(0.0) * tick_hz).max(0.0))
    }

    pub fn from_ticks_f64(ticks: f64) -> Self {
        let ticks = ticks.max(0.0);
        let tick = ticks.floor() as u64;
        let alpha = (ticks - tick as f64) as f32;
        Self { tick, alpha }
    }

    pub fn as_ticks_f64(self) -> f64 {
        self.tick as f64 + self.alpha as f64
    }

    pub fn clamp_ticks(self, min_tick: u64, max_tick: u64) -> Self {
        let min_ticks = min_tick as f64;
        let max_ticks = max_tick as f64;
        Self::from_ticks_f64(self.as_ticks_f64().clamp(min_ticks, max_ticks))
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct RemoteSnapshotSample {
    pub server_tick: u64,
    pub position: Vec3,
    pub velocity: Vec3,
    pub look: Vec2,
    pub state: NetAhoyMoveState,
}

impl RemoteSnapshotSample {
    pub fn from_snapshot(snapshot: &AhoySnapshot) -> Self {
        Self {
            server_tick: snapshot.server_tick,
            position: snapshot.position,
            velocity: snapshot.velocity,
            look: snapshot.look,
            state: snapshot.state,
        }
    }

    pub(crate) fn starts_new_motion_segment_after(self, previous: Self) -> bool {
        let tick_gap = self.server_tick.saturating_sub(previous.server_tick);
        tick_gap > REMOTE_INTERPOLATION_DISCONTINUITY_TICKS
            || self.position.distance(previous.position) > REMOTE_INTERPOLATION_TELEPORT_DISTANCE
    }

    pub fn interpolate_at(self, other: Self, render_time: RemoteRenderTime) -> Self {
        let span = other.server_tick.saturating_sub(self.server_tick).max(1) as f64;
        let alpha =
            ((render_time.as_ticks_f64() - self.server_tick as f64) / span).clamp(0.0, 1.0) as f32;
        self.interpolate_fraction(other, alpha, render_time)
    }

    pub fn interpolate_fraction(
        self,
        other: Self,
        alpha: f32,
        render_time: RemoteRenderTime,
    ) -> Self {
        let alpha = alpha.clamp(0.0, 1.0);
        Self {
            server_tick: render_time.tick,
            position: self.position.lerp(other.position, alpha),
            velocity: self.velocity.lerp(other.velocity, alpha),
            look: Vec2::new(
                lerp_radians(self.look.x, other.look.x, alpha),
                self.look.y.lerp(other.look.y, alpha),
            ),
            state: if alpha < 0.5 { self.state } else { other.state },
        }
    }
}

pub(crate) fn sample_buffer_at(
    samples: &VecDeque<RemoteSnapshotSample>,
    render_time: RemoteRenderTime,
) -> Option<RemoteSnapshotSample> {
    let first = samples.front().copied()?;
    let last = samples.back().copied()?;
    let target_ticks = render_time.as_ticks_f64();

    if target_ticks <= first.server_tick as f64 {
        return Some(first);
    }
    if target_ticks >= last.server_tick as f64 {
        return Some(last);
    }

    let mut previous = first;
    for sample in samples.iter().copied().skip(1) {
        if target_ticks <= sample.server_tick as f64 {
            return Some(previous.interpolate_at(sample, render_time));
        }
        previous = sample;
    }

    Some(last)
}

fn lerp_radians(from: f32, to: f32, alpha: f32) -> f32 {
    let delta =
        (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    from + delta * alpha
}

pub fn ray_capsule_distance(
    origin: Vec3,
    direction: Vec3,
    max_distance: f32,
    capsule_center: Vec3,
    radius: f32,
    half_height: f32,
) -> Option<f32> {
    let direction = direction.try_normalize()?;
    ray_segment_capsule_distance(
        origin,
        direction,
        max_distance,
        capsule_center - Vec3::Y * half_height,
        capsule_center + Vec3::Y * half_height,
        radius,
    )
}

pub(crate) fn ray_segment_capsule_distance(
    origin: Vec3,
    direction: Vec3,
    max_distance: f32,
    segment_a: Vec3,
    segment_b: Vec3,
    radius: f32,
) -> Option<f32> {
    let direction = direction.try_normalize()?;
    let capsule_center = segment_a.midpoint(segment_b);
    let capsule = Collider::capsule_endpoints(
        radius,
        segment_a - capsule_center,
        segment_b - capsule_center,
    );
    let (distance, _) = capsule.cast_ray(
        Position::new(capsule_center),
        Rotation::IDENTITY,
        origin,
        direction,
        max_distance,
        false,
    )?;
    Some(distance)
}
