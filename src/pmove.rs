//! The Source-style movement context: one user command in, one KCC step out.
//!
//! [`NetAhoyStepper`] is the single movement code path. Client prediction,
//! client replay after a misprediction, and server command consumption all go
//! through [`NetAhoyStepper::step`], so they cannot drift apart.

use avian3d::prelude::*;
use bevy::{
    ecs::{query::QueryData, schedule::ScheduleLabel, system::SystemParam},
    prelude::*,
    time::Stopwatch,
};
use bevy_ahoy::{CharacterLook, input::AccumulatedInput, prelude::*};

use crate::protocol::{AhoyButtons, AhoySnapshot, AhoyUserCmd, NetAhoyMoveState};

/// The deterministic slice of a player's movement state handed to a
/// [`MovementEffect`]. By value so the effect never holds a live borrow of
/// the player query, freeing the read-only [`SpatialQuery`] to be borrowed
/// alongside. Velocity is *not* here — it's the one mutable thing, passed as
/// `&mut Vec3`.
#[derive(Clone, Copy, Debug)]
pub struct MoveView {
    pub position: Vec3,
    /// `x` = yaw, `y` = pitch (radians).
    pub look: Vec2,
}

/// A game movement effect evaluated inside the per-step path: jump pads, rocket
/// jumps, anything that nudges velocity. It may read only inputs that replay
/// reproduces — the player's own view, this command (incl. the game-defined
/// [`AhoyButtons`] bits), `previous_buttons` for edge detection, and the *static*
/// world via the read-only [`SpatialQuery`] — and writes through `&mut Vec3`
/// velocity, so it can set, add, zero, or clamp as it likes. The narrow signature
/// is the guardrail: an effect physically cannot read another player or touch
/// anything but velocity, so it cannot desync on replay.
pub type MovementEffect = fn(
    view: MoveView,
    command: &AhoyUserCmd,
    previous_buttons: AhoyButtons,
    world: &SpatialQuery,
    velocity: &mut Vec3,
);

#[derive(Resource, Default)]
pub struct MovementEffects(pub Vec<MovementEffect>);

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
///     stepper.step(entity, command, previous_buttons)?;
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
    effects: Res<'w, MovementEffects>,
    fixed_time: Res<'w, Time<Fixed>>,
}

impl NetAhoyStepper<'_, '_> {
    /// Run one user command through one KCC movement step for `entity`.
    /// `previous_buttons` is the prior command's buttons, used for rising-edge
    /// detection (library bits in the controller, game bits in movement effects).
    pub fn step(
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
        let (view, mut velocity) = {
            let mut players = self.set.p1();
            let mut parts = players.get_mut(entity)?;
            parts.position.0 = parts.transform.translation;
            (
                MoveView {
                    position: parts.transform.translation,
                    look: Vec2::new(parts.look.yaw, parts.look.pitch),
                },
                parts.velocity.0,
            )
        };

        // Movement effects (jump pads, rocket jumps, ...) run inside the step so
        // client replay and the server reproduce them. Each reads only `view` +
        // this command + `previous_buttons` + the read-only static-world
        // SpatialQuery, and mutates `velocity` in registration order. `velocity`
        // is a local copy, so p1 is not borrowed while p2 (SpatialQuery) is.
        if !self.effects.0.is_empty() {
            // Clone the (cheap, fn-pointer) list to drop the Res borrow before p2.
            let effects = self.effects.0.clone();
            let world = self.set.p2();
            for effect in &effects {
                effect(view, &command, previous_buttons, &world, &mut velocity);
            }

            let mut players = self.set.p1();
            players.get_mut(entity)?.velocity.0 = velocity;
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
    input.swim_up = command.buttons.contains(AhoyButtons::SWIM_UP);
    input.crouched = command.buttons.contains(AhoyButtons::CROUCH);

    // Bits set this command but not last = rising edges.
    let pressed = command.buttons - previous_buttons;
    if pressed.contains(AhoyButtons::JUMP) {
        input.jumped = Some(Stopwatch::new());
    }
    if pressed.contains(AhoyButtons::TAC) {
        input.tac = Some(Stopwatch::new());
    }
    if pressed.contains(AhoyButtons::CRANE) {
        input.craned = Some(Stopwatch::new());
    }
    if pressed.contains(AhoyButtons::MANTLE) {
        input.mantled = Some(Stopwatch::new());
    }
    if pressed.contains(AhoyButtons::CLIMBDOWN) {
        input.climbdown = Some(Stopwatch::new());
    }

    look.yaw = command.look.x;
    look.pitch = command.look.y.clamp(-1.5, 1.5);
}
