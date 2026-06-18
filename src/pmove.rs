//! The Source-style movement context: one user command in, one KCC step out.
//!
//! [`NetAhoyStepper`] is the single movement code path. Client prediction,
//! client replay after a misprediction, and server command consumption all go
//! through [`NetAhoyStepper::pmove`], so they cannot drift apart.

use avian3d::prelude::*;
use bevy::{
    ecs::{query::QueryData, schedule::ScheduleLabel, system::SystemParam},
    prelude::*,
    time::Stopwatch,
};
use bevy_ahoy::{CharacterLook, input::AccumulatedInput, prelude::*};

use crate::protocol::{AhoyButtons, AhoySnapshot, AhoyUserCmd, NetAhoyMoveState};

/// A static, axis-aligned region that *sets* a player's vertical velocity while
/// the player's center is inside it. Field effects (jump pads, bounce pads) are
/// pure data — the lib applies them in the [`NetAhoyStepper::pmove`] tail so they
/// re-derive identically during client replay and server simulation. Set (not
/// add) keeps multi-tick contact idempotent.
#[derive(Clone, Copy, Debug)]
pub struct LaunchZone {
    pub center: Vec3,
    pub half_extents: Vec3,
    pub launch_speed: f32,
}

#[derive(Resource, Default)]
pub struct LaunchZones(pub Vec<LaunchZone>);

/// The deterministic slice of a player's movement state handed to a
/// [`MovementAbility`]. By value so the ability never holds a live borrow of the
/// player query, freeing the read-only [`SpatialQuery`] to be borrowed alongside.
#[derive(Clone, Copy, Debug)]
pub struct MoveView {
    pub position: Vec3,
    /// `x` = yaw, `y` = pitch (radians).
    pub look: Vec2,
    pub velocity: Vec3,
}

/// A game ability evaluated inside the per-step path. It may read only inputs
/// that replay reproduces — the player's own state, this command, and the
/// *static* world via the read-only [`SpatialQuery`] — and returns a velocity
/// delta. This signature is the guardrail: an ability physically cannot read
/// another player or dynamic entity, so it cannot desync on replay.
pub type MovementAbility =
    fn(view: MoveView, command: &AhoyUserCmd, previous_buttons: AhoyButtons, world: &SpatialQuery) -> Option<Vec3>;

#[derive(Resource, Default)]
pub struct MovementAbilities(pub Vec<MovementAbility>);

/// Schedule that Ahoy's own per-tick systems are parked in. Netcode steps the
/// KCC manually through [`NetAhoyStepper`]; add [`NetAhoyKccRunnerPlugin`] only
/// if you want Ahoy to also run automatically every fixed tick.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NetAhoyKccSchedule;

pub struct NetAhoyKccRunnerPlugin;

impl Plugin for NetAhoyKccRunnerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedPostUpdate,
            run_net_ahoy_kcc_schedule.before(PhysicsSystems::First),
        );
    }
}

fn run_net_ahoy_kcc_schedule(world: &mut World) {
    world.run_schedule(NetAhoyKccSchedule);
}

/// Everything the client needs to rewind to (and resimulate from) one
/// predicted command.
#[derive(Clone, Debug)]
pub struct AhoyPredictionFrame {
    pub command: AhoyUserCmd,
    pub position: Vec3,
    pub velocity: Vec3,
    pub look: Vec2,
    pub state: NetAhoyMoveState,
    pub controller_state: CharacterControllerState,
    pub accumulated_input: AccumulatedInput,
}

#[derive(QueryData)]
#[query_data(mutable)]
pub struct PmoveParts {
    input: &'static mut AccumulatedInput,
    look: &'static mut CharacterLook,
    transform: &'static mut Transform,
    position: &'static mut Position,
    velocity: &'static mut LinearVelocity,
    state: &'static mut CharacterControllerState,
}

/// The movement context. The Bevy dependencies of a KCC step live in here so
/// callers read like Source's `pmove`:
///
/// ```ignore
/// for command in commands {
///     stepper.pmove(entity, command, previous_buttons)?;
///     previous_buttons = command.buttons;
/// }
/// ```
#[derive(SystemParam)]
pub struct NetAhoyStepper<'w, 's> {
    // ParamSet because the Ahoy stepper's internal query also writes
    // Transform/Position/LinearVelocity/AccumulatedInput.
    set: ParamSet<
        'w,
        's,
        (
            CharacterControllerStepper<'w, 's>,
            Query<'w, 's, PmoveParts>,
            SpatialQuery<'w, 's>,
        ),
    >,
    zones: Res<'w, LaunchZones>,
    abilities: Res<'w, MovementAbilities>,
    fixed_time: Res<'w, Time<Fixed>>,
}

impl NetAhoyStepper<'_, '_> {
    /// Run one user command through one KCC movement step for `entity`.
    pub fn pmove(
        &mut self,
        entity: Entity,
        command: AhoyUserCmd,
        previous_buttons: AhoyButtons,
    ) -> Result<()> {
        let fixed_delta = self.fixed_time.timestep();

        {
            let mut players = self.set.p1();
            let mut parts = players.get_mut(entity)?;
            tick_input_timers(&mut parts.input, fixed_delta);
            clear_transient_input(&mut parts.input);
            apply_usercmd(&mut parts.input, &mut parts.look, command, previous_buttons);
        }

        self.set.p0().step_entity(entity, fixed_delta)?;

        // The step writes Transform; Position is what the next step reads.
        let view = {
            let mut players = self.set.p1();
            let mut parts = players.get_mut(entity)?;
            parts.position.0 = parts.transform.translation;
            MoveView {
                position: parts.transform.translation,
                look: Vec2::new(parts.look.yaw, parts.look.pitch),
                velocity: parts.velocity.0,
            }
        };

        self.apply_movement_effects(entity, view, command, previous_buttons)?;
        Ok(())
    }

    /// Field effects (data zones) then ability effects (game fns), applied to
    /// velocity inside the step so client replay and the server reproduce them.
    /// All inputs are replay-reproducible: position, this command, static world.
    fn apply_movement_effects(
        &mut self,
        entity: Entity,
        view: MoveView,
        command: AhoyUserCmd,
        previous_buttons: AhoyButtons,
    ) -> Result<()> {
        // Field zones: set vertical velocity while inside. Set (not +=) so
        // multi-tick contact is idempotent and re-derives cleanly on replay.
        let launch_y = self.zones.0.iter().find_map(|zone| {
            ((view.position - zone.center).abs().cmple(zone.half_extents).all())
                .then_some(zone.launch_speed)
        });

        // Abilities: each reads only `view` + this command + the read-only
        // SpatialQuery, and returns a Δvelocity. `view` is by value, so p1 is
        // not borrowed while p2 (SpatialQuery) is.
        let mut impulse = Vec3::ZERO;
        if !self.abilities.0.is_empty() {
            // Collect first to release the Res borrow before touching the ParamSet.
            let abilities = self.abilities.0.clone();
            let world = self.set.p2();
            for ability in &abilities {
                if let Some(delta) = ability(view, &command, previous_buttons, &world) {
                    impulse += delta;
                }
            }
        }

        if launch_y.is_some() || impulse != Vec3::ZERO {
            let mut players = self.set.p1();
            let mut parts = players.get_mut(entity)?;
            parts.velocity.0 += impulse;
            if let Some(y) = launch_y {
                parts.velocity.0.y = y;
            }
        }
        Ok(())
    }

    /// Record the post-step state for `command` so it can be restored later.
    pub fn capture_frame(&mut self, entity: Entity, command: AhoyUserCmd) -> Option<AhoyPredictionFrame> {
        let mut players = self.set.p1();
        let parts = players.get_mut(entity).ok()?;
        Some(AhoyPredictionFrame {
            command,
            position: parts.transform.translation,
            velocity: parts.velocity.0,
            look: Vec2::new(parts.look.yaw, parts.look.pitch),
            state: NetAhoyMoveState::from_controller_state(&parts.state),
            controller_state: parts.state.clone(),
            accumulated_input: parts.input.clone(),
        })
    }

    /// Rewind `entity` to an authoritative snapshot, reusing the locally
    /// recorded controller/input state for that tick when available.
    pub fn restore(
        &mut self,
        entity: Entity,
        snapshot: &AhoySnapshot,
        local_state: Option<(&CharacterControllerState, &AccumulatedInput)>,
    ) {
        let mut players = self.set.p1();
        let Ok(mut parts) = players.get_mut(entity) else {
            return;
        };

        parts.transform.translation = snapshot.position;
        parts.position.0 = snapshot.position;
        parts.velocity.0 = snapshot.velocity;
        parts.look.yaw = snapshot.look.x;
        parts.look.pitch = snapshot.look.y;

        if let Some((stored_state, stored_input)) = local_state {
            *parts.state = stored_state.clone();
            *parts.input = stored_input.clone();
        } else {
            *parts.state = CharacterControllerState::default();
            *parts.input = AccumulatedInput::default();
        }

        snapshot.state.apply_to_controller_state(&mut parts.state);
    }

    pub fn position(&mut self, entity: Entity) -> Option<Vec3> {
        let players = self.set.p1();
        players
            .get(entity)
            .ok()
            .map(|parts| parts.transform.translation)
    }

    /// Drop held movement input without stepping, for ticks with no command.
    pub fn clear_transient(&mut self, entity: Entity) {
        let mut players = self.set.p1();
        if let Ok(mut parts) = players.get_mut(entity) {
            clear_transient_input(&mut parts.input);
        }
    }
}

fn tick_input_timers(input: &mut AccumulatedInput, delta: std::time::Duration) {
    if let Some(timer) = input.jumped.as_mut() {
        timer.tick(delta);
    }
    if let Some(timer) = input.tac.as_mut() {
        timer.tick(delta);
    }
    if let Some(timer) = input.craned.as_mut() {
        timer.tick(delta);
    }
    if let Some(timer) = input.mantled.as_mut() {
        timer.tick(delta);
    }
    if let Some(timer) = input.climbdown.as_mut() {
        timer.tick(delta);
    }
}

fn clear_transient_input(input: &mut AccumulatedInput) {
    input.last_movement = None;
    input.swim_up = false;
    input.crouched = false;
}

fn apply_usercmd(
    input: &mut AccumulatedInput,
    look: &mut CharacterLook,
    command: AhoyUserCmd,
    previous_buttons: AhoyButtons,
) {
    input.last_movement = Some(command.movement.clamp_length_max(1.0));
    input.swim_up = command.buttons.swim_up;
    input.crouched = command.buttons.crouch;

    if command.buttons.jump && !previous_buttons.jump {
        input.jumped = Some(Stopwatch::new());
    }
    if command.buttons.tac && !previous_buttons.tac {
        input.tac = Some(Stopwatch::new());
    }
    if command.buttons.crane && !previous_buttons.crane {
        input.craned = Some(Stopwatch::new());
    }
    if command.buttons.mantle && !previous_buttons.mantle {
        input.mantled = Some(Stopwatch::new());
    }
    if command.buttons.climbdown && !previous_buttons.climbdown {
        input.climbdown = Some(Stopwatch::new());
    }

    look.yaw = command.look.x;
    look.pitch = command.look.y.clamp(-1.5, 1.5);
}
