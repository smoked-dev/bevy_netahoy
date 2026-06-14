use aeronet::io::{
    Session, SessionEndpoint,
    connection::{DisconnectReason, Disconnected},
};
use aeronet_replicon::client::{AeronetRepliconClient, AeronetRepliconClientPlugin};
use aeronet_websocket::client::{ClientConfig, WebSocketClient, WebSocketClientPlugin};
use avian3d::prelude::*;
use bevy::{
    app::{RunFixedMainLoop, RunFixedMainLoopSystems},
    input::{common_conditions::input_just_pressed, mouse::AccumulatedMouseMotion},
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PresentMode},
    winit::{UpdateMode::Continuous, WinitSettings},
};
use bevy_ahoy::{CharacterLook, prelude::*};
use bevy_enhanced_input::prelude::EnhancedInputPlugin;
use bevy_netahoy::*;
use bevy_replicon::prelude::*;

mod shared;
use shared::*;

const CAMERA_DISTANCE: f32 = 5.2;
const CAMERA_HEIGHT: f32 = 0.85;
const CAMERA_SHOULDER_OFFSET: f32 = 1.25;
const CAMERA_AIM_RIGHT_OFFSET: f32 = 0.85;
const HIT_MARKER_SECONDS: f32 = 0.75;
const AUTO_FIRE_INTERVAL_SECONDS: f32 = 0.10;

fn main() -> AppExit {
    let poor_network = poor_network_from_args();
    let time_scale = debug_time_scale_from_args();
    let remote_ghost_debug = remote_ghost_debug_from_args();

    let mut app = App::new();
    app.insert_resource(ClientLook::default())
        .insert_resource(ClientShotState::default())
        .insert_resource(ShotFeedback::default())
        .insert_resource(remote_ghost_debug)
        .insert_resource(time_scale)
        .insert_resource(WinitSettings {
            focused_mode: Continuous,
            unfocused_mode: Continuous,
        });

    if poor_network {
        app.add_plugins(AeronetNetworkConditionerPlugin::poor_condition());
    }

    app.add_plugins((
        DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "NetAhoy client".to_string(),
                resolution: (1280, 720).into(),
                present_mode: PresentMode::AutoVsync,
                ..default()
            }),
            ..default()
        }),
        PhysicsPlugins::default(),
        EnhancedInputPlugin,
        AhoyPlugins::new(NetAhoyKccSchedule),
        SharedNetAhoyPlugin,
        ClientNetAhoyPlugin,
        ClientPlugin,
    ))
    .run()
}

fn remote_ghost_debug_from_args() -> RemoteGhostDebug {
    RemoteGhostDebug {
        visible: std::env::args()
            .any(|arg| arg == "--show-ghosts" || arg == "--show-remote-ghosts"),
    }
}

#[derive(Resource, Default)]
struct ClientLook {
    yaw: f32,
    pitch: f32,
}

#[derive(Resource, Default)]
struct ClientShotState {
    next_shot_id: u32,
    seconds_until_next_shot: f32,
}

#[derive(Resource, Default)]
struct ShotFeedback {
    predicted: String,
    acknowledged: String,
}

#[derive(Resource, Default)]
struct RemoteGhostDebug {
    visible: bool,
}

#[derive(Component)]
struct SpeedText;

#[derive(Component)]
struct StatusText;

#[derive(Component)]
struct PredictionText;

#[derive(Component)]
struct ShotText;

#[derive(Component)]
struct HitMarker {
    timer: Timer,
}

type CameraRigFilter = (
    With<Camera3d>,
    Without<ClientPredictionKcc>,
    Without<ServerTruthGhost>,
    Without<LocalPresentationPlayer>,
);

struct ClientPlugin;

impl Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((WebSocketClientPlugin, AeronetRepliconClientPlugin))
            .add_observer(use_replicon_for_session)
            .add_observer(set_window_title_on_join)
            .add_observer(receive_shot_ack)
            .add_observer(log_connected)
            .add_observer(log_disconnected)
            .add_systems(Startup, (setup_client, setup_scene, setup_hud))
            .add_systems(
                Update,
                (
                    capture_cursor.run_if(input_just_pressed(MouseButton::Left)),
                    release_cursor.run_if(input_just_pressed(KeyCode::Escape)),
                    update_client_look,
                )
                    .chain(),
            )
            .add_systems(
                RunFixedMainLoop,
                gather_client_input.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
            )
            .add_systems(
                Update,
                (
                    attach_player_meshes,
                    spawn_client_prediction_kcc,
                    spawn_remote_player_visuals,
                    toggle_remote_ghost_debug,
                    update_hit_markers,
                    update_camera_from_client_prediction_kcc,
                    automatic_fire,
                    update_speed_text,
                    update_status_text,
                    update_prediction_text,
                    update_shot_text,
                )
                    .chain()
                    .after(ClientNetAhoySystems::Interpolate),
            );
    }
}

fn setup_client(mut commands: Commands) {
    commands
        .spawn(Name::new("Client"))
        .queue(WebSocketClient::connect(
            client_config(),
            DEFAULT_SERVER_URL,
        ));

    info!("websocket client connecting to {DEFAULT_SERVER_URL}");
}

fn set_window_title_on_join(accepted: On<JoinAccepted>, mut windows: Query<&mut Window>) {
    for mut window in &mut windows {
        window.title = format!("NetAhoy client {}", accepted.player_id);
    }
}

fn use_replicon_for_session(session: On<Add, SessionEndpoint>, mut commands: Commands) {
    commands
        .entity(session.event_target())
        .insert(AeronetRepliconClient);
}

fn log_connected(session: On<Add, Session>) {
    info!("websocket client connected: {}", session.event_target());
}

fn log_disconnected(disconnected: On<Disconnected>) {
    match &disconnected.reason {
        DisconnectReason::ByUser(reason) => info!("websocket client disconnected: {reason}"),
        DisconnectReason::ByPeer(reason) => {
            warn!("websocket client disconnected by server: {reason}")
        }
        DisconnectReason::ByError(err) => warn!("websocket client disconnected: {err:#}"),
    }
}

#[cfg(target_family = "wasm")]
fn client_config() -> ClientConfig {
    ClientConfig::default()
}

#[cfg(not(target_family = "wasm"))]
fn client_config() -> ClientConfig {
    ClientConfig::builder().with_no_encryption()
}

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        DirectionalLight {
            illuminance: 18_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(-6.0, 12.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    spawn_world_render(&mut commands, &mut meshes, &mut materials);
    spawn_world_colliders(&mut commands);

    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 80.0_f32.to_radians(),
            ..default()
        }),
        Transform::from_translation(SPAWN_POINT + Vec3::new(0.0, 3.0, 7.0))
            .looking_at(SPAWN_POINT, Vec3::Y),
    ));
}

fn setup_hud(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(16.0),
            left: px(16.0),
            ..default()
        },
        Text::new("connecting"),
        TextColor(Color::WHITE.with_alpha(0.65)),
        StatusText,
    ));

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(40.0),
            left: px(16.0),
            ..default()
        },
        Text::new("prediction: waiting"),
        TextColor(Color::WHITE.with_alpha(0.55)),
        PredictionText,
    ));

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(64.0),
            left: px(16.0),
            ..default()
        },
        Text::new("shots: predicted none | server none"),
        TextColor(Color::WHITE.with_alpha(0.55)),
        ShotText,
    ));

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            bottom: px(56.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        Text::new("0.000"),
        TextColor(Color::WHITE.with_alpha(0.55)),
        SpeedText,
    ));

    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|parent| {
            parent.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: px(14.0),
                    height: px(2.0),
                    ..default()
                },
                BackgroundColor(Color::WHITE.with_alpha(0.35)),
            ));
            parent.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: px(2.0),
                    height: px(14.0),
                    ..default()
                },
                BackgroundColor(Color::WHITE.with_alpha(0.35)),
            ));
        });
}

fn player_display_color(player_id: PlayerId) -> Color {
    if player_id.0 == FLYING_TARGET_PLAYER_ID {
        Color::srgb(1.0, 0.25, 0.68)
    } else {
        player_color(player_id.0)
    }
}

fn attach_player_meshes(
    mut commands: Commands,
    local: Res<LocalPlayerId>,
    debug_ghosts: Res<RemoteGhostDebug>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    players: Query<(Entity, &PlayerId), Added<NetworkedPlayer>>,
) {
    for (entity, player_id) in &players {
        let is_local_server_truth = local.is_assigned_to(player_id.0);
        let (base_color, alpha_mode, visibility) = if is_local_server_truth {
            (
                Color::srgba(0.25, 0.65, 1.0, 0.28),
                AlphaMode::Blend,
                Visibility::Visible,
            )
        } else {
            (
                player_display_color(*player_id).with_alpha(0.18),
                AlphaMode::Blend,
                if debug_ghosts.visible {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                },
            )
        };

        commands.entity(entity).insert((
            Mesh3d(meshes.add(Capsule3d::new(PLAYER_CAPSULE_RADIUS, 1.5))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color,
                alpha_mode,
                perceptual_roughness: 0.8,
                ..default()
            })),
            visibility,
        ));
    }
}

fn spawn_client_prediction_kcc(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    local_players: Query<(Entity, &Transform, Option<&AhoySnapshot>), Added<ServerTruthGhost>>,
    predictions: Query<Entity, With<ClientPredictionKcc>>,
) {
    if predictions.iter().next().is_some() {
        return;
    }

    for (server_entity, transform, authoritative_state) in &local_players {
        let position = authoritative_state
            .map(|state| state.position)
            .unwrap_or(transform.translation);
        let look = authoritative_state
            .map(|state| state.look)
            .unwrap_or(Vec2::ZERO);

        let prediction_entity = commands
            .spawn((
                Name::new("client prediction kcc"),
                ClientPredictionKcc { server_entity },
                CharacterLook {
                    yaw: look.x,
                    pitch: look.y,
                },
                player_controller(),
                Collider::cylinder(PLAYER_CAPSULE_RADIUS, 1.5),
                player_collision_layers(),
                Position::new(position),
                Rotation::IDENTITY,
                LinearVelocity::ZERO,
                Mesh3d(meshes.add(Capsule3d::new(PLAYER_CAPSULE_RADIUS, 1.5))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgba(0.1, 1.0, 0.45, 0.38),
                    alpha_mode: AlphaMode::Blend,
                    perceptual_roughness: 0.65,
                    ..default()
                })),
                Transform::from_translation(position),
                Visibility::Visible,
            ))
            .id();

        commands.spawn((
            Name::new("local presentation player"),
            LocalPresentationPlayer { prediction_entity },
            Mesh3d(meshes.add(Capsule3d::new(PLAYER_CAPSULE_RADIUS, 1.5))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 0.83, 0.22),
                perceptual_roughness: 0.7,
                ..default()
            })),
            Transform::from_translation(position),
            Visibility::default(),
        ));
    }
}

fn spawn_remote_player_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    remotes: Query<
        (Entity, &PlayerId, &Transform, Option<&AhoySnapshot>),
        Added<RemoteInterpolationBuffer>,
    >,
    visuals: Query<&RemotePlayerVisual>,
) {
    for (server_entity, player_id, transform, snapshot) in &remotes {
        if visuals
            .iter()
            .any(|visual| visual.server_entity == server_entity)
        {
            continue;
        }

        let snapshot = snapshot.filter(|snapshot| snapshot.server_tick != 0);
        let position = snapshot
            .map(|snapshot| snapshot.position)
            .unwrap_or(transform.translation);
        let look = snapshot.map(|snapshot| snapshot.look).unwrap_or(Vec2::ZERO);

        commands.spawn((
            Name::new(format!("remote player {} visual", player_id.0)),
            RemotePlayerVisual {
                server_entity,
                player_id: *player_id,
            },
            CharacterLook {
                yaw: look.x,
                pitch: look.y,
            },
            Mesh3d(meshes.add(Capsule3d::new(PLAYER_CAPSULE_RADIUS, 1.5))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: player_display_color(*player_id),
                perceptual_roughness: 0.75,
                ..default()
            })),
            Transform::from_translation(position),
            Visibility::default(),
        ));
    }
}

fn toggle_remote_ghost_debug(
    keys: Res<ButtonInput<KeyCode>>,
    mut debug_ghosts: ResMut<RemoteGhostDebug>,
    mut ghosts: Query<
        &mut Visibility,
        (With<RemoteInterpolationBuffer>, Without<ServerTruthGhost>),
    >,
) {
    if !keys.just_pressed(KeyCode::F3) {
        return;
    }

    debug_ghosts.visible = !debug_ghosts.visible;
    let visibility = if debug_ghosts.visible {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };

    for mut ghost_visibility in &mut ghosts {
        *ghost_visibility = visibility;
    }
}

fn capture_cursor(mut cursor: Single<&mut CursorOptions>) {
    cursor.grab_mode = CursorGrabMode::Locked;
    cursor.visible = false;
}

fn release_cursor(mut cursor: Single<&mut CursorOptions>) {
    cursor.visible = true;
    cursor.grab_mode = CursorGrabMode::None;
}

fn update_client_look(
    cursor: Single<&CursorOptions>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mut look: ResMut<ClientLook>,
) {
    if cursor.grab_mode != CursorGrabMode::Locked {
        return;
    }

    const MOUSE_SENSITIVITY: f32 = 0.07_f32.to_radians();
    look.yaw -= mouse_motion.delta.x * MOUSE_SENSITIVITY;
    look.pitch = (look.pitch - mouse_motion.delta.y * MOUSE_SENSITIVITY).clamp(-1.5, 1.5);
}

fn gather_client_input(
    keys: Res<ButtonInput<KeyCode>>,
    look: Res<ClientLook>,
    mut input: ResMut<ClientInput>,
) {
    let mut movement = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        movement.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        movement.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        movement.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        movement.x += 1.0;
    }

    input.movement = movement;
    input.look = Vec2::new(look.yaw, look.pitch);
    input.buttons = AhoyButtons {
        jump: keys.pressed(KeyCode::Space),
        crouch: keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::KeyC),
        tac: keys.pressed(KeyCode::ShiftLeft),
        mantle: keys.pressed(KeyCode::KeyE),
        crane: keys.pressed(KeyCode::KeyQ),
        climbdown: keys.pressed(KeyCode::KeyZ),
        swim_up: keys.pressed(KeyCode::Space),
    };
}

fn automatic_fire(
    mut commands: Commands,
    cursor: Single<&CursorOptions>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    clock: Res<ClientServerClock>,
    local: Res<LocalPlayerId>,
    mut shot_state: ResMut<ClientShotState>,
    mut feedback: ResMut<ShotFeedback>,
    camera: Single<&Transform, CameraRigFilter>,
    targets: Query<(&RemotePlayerVisual, &Transform)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !mouse_buttons.pressed(MouseButton::Left) {
        shot_state.seconds_until_next_shot = 0.0;
        return;
    }
    if cursor.grab_mode != CursorGrabMode::Locked {
        return;
    }

    shot_state.seconds_until_next_shot -= time.delta_secs();
    if shot_state.seconds_until_next_shot > 0.0 {
        return;
    }
    shot_state.seconds_until_next_shot = AUTO_FIRE_INTERVAL_SECONDS;

    let shot_id = shot_state.next_shot_id.wrapping_add(1);
    shot_state.next_shot_id = shot_id;

    let origin = camera.translation;
    let direction = camera.rotation * Vec3::NEG_Z;
    let predicted_hit = predicted_hit_scan(origin, direction, local.0, &targets);
    let sample_time = clock.target_time().unwrap_or_default();

    feedback.predicted = predicted_hit
        .map(|hit| format_hit("client", hit))
        .unwrap_or_else(|| format!("client #{shot_id}: miss"));

    if let Some(hit) = predicted_hit {
        spawn_hit_marker(
            &mut commands,
            &mut meshes,
            &mut materials,
            hit.position,
            Color::srgb(0.2, 1.0, 0.35),
            "client predicted hit",
        );
    }

    commands.client_trigger(HitScanShot {
        shot_id,
        client_sample_tick: sample_time.tick,
        client_sample_alpha: sample_time.alpha,
        origin,
        direction,
    });
}

fn predicted_hit_scan(
    origin: Vec3,
    direction: Vec3,
    local_player_id: Option<u64>,
    targets: &Query<(&RemotePlayerVisual, &Transform)>,
) -> Option<HitScanHit> {
    targets
        .iter()
        .filter(|(visual, _)| Some(visual.player_id.0) != local_player_id)
        .filter_map(|(visual, transform)| {
            let distance = ray_capsule_distance(
                origin,
                direction,
                HITSCAN_MAX_DISTANCE,
                transform.translation,
                PLAYER_CAPSULE_RADIUS,
                PLAYER_CAPSULE_HALF_HEIGHT,
            )?;
            Some(HitScanHit {
                player_id: visual.player_id,
                position: origin + direction.normalize_or_zero() * distance,
                distance,
            })
        })
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
}

fn receive_shot_ack(
    ack: On<HitScanAck>,
    mut commands: Commands,
    mut feedback: ResMut<ShotFeedback>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    feedback.acknowledged = ack
        .hit
        .map(|hit| format_hit("server", hit))
        .unwrap_or_else(|| format!("server #{}: miss", ack.shot_id));

    if let Some(hit) = ack.hit {
        spawn_hit_marker(
            &mut commands,
            &mut meshes,
            &mut materials,
            hit.position,
            Color::srgb(1.0, 0.12, 0.1),
            "server ack hit",
        );
    }
}

fn spawn_hit_marker(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    position: Vec3,
    color: Color,
    name: &'static str,
) {
    commands.spawn((
        Name::new(name),
        HitMarker {
            timer: Timer::from_seconds(HIT_MARKER_SECONDS, TimerMode::Once),
        },
        Mesh3d(meshes.add(Cuboid::new(0.18, 0.18, 0.18))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: color,
            ..default()
        })),
        Transform::from_translation(position),
    ));
}

fn format_hit(source: &str, hit: HitScanHit) -> String {
    let target = if hit.player_id.0 == FLYING_TARGET_PLAYER_ID {
        "target".to_string()
    } else {
        format!("player {}", hit.player_id.0)
    };
    format!("{source}: {target} {:.1}m", hit.distance)
}

fn update_hit_markers(
    mut commands: Commands,
    time: Res<Time>,
    mut markers: Query<(Entity, &mut HitMarker, &mut Transform)>,
) {
    for (entity, mut marker, mut transform) in &mut markers {
        marker.timer.tick(time.delta());
        let remaining = marker.timer.remaining_secs() / HIT_MARKER_SECONDS;
        transform.scale = Vec3::splat(remaining.max(0.15));
        if marker.timer.is_finished() {
            commands.entity(entity).despawn();
        }
    }
}

fn update_camera_from_client_prediction_kcc(
    look: Res<ClientLook>,
    predictions: Query<&Transform, (With<ClientPredictionKcc>, Without<Camera3d>)>,
    server_players: Query<&Transform, (With<ServerTruthGhost>, Without<Camera3d>)>,
    mut camera: Single<&mut Transform, CameraRigFilter>,
) {
    let target = predictions
        .single()
        .map(|transform| transform.translation + Vec3::Y * 0.6)
        .or_else(|_| {
            server_players
                .single()
                .map(|transform| transform.translation + Vec3::Y * 0.6)
        })
        .unwrap_or(SPAWN_POINT);
    let rotation = Quat::from_euler(EulerRot::YXZ, look.yaw, look.pitch, 0.0);
    let forward = rotation * Vec3::NEG_Z;
    let right = rotation * Vec3::X;
    let camera_target = target + forward * 8.0 + right * CAMERA_AIM_RIGHT_OFFSET;

    camera.translation = target - forward * CAMERA_DISTANCE
        + right * CAMERA_SHOULDER_OFFSET
        + Vec3::Y * CAMERA_HEIGHT;
    camera.look_at(camera_target, Vec3::Y);
}

fn update_speed_text(
    mut text: Single<&mut Text, With<SpeedText>>,
    predicted_velocity: Option<Single<&LinearVelocity, With<ClientPredictionKcc>>>,
    server_velocity: Option<Single<&LinearVelocity, With<ServerTruthGhost>>>,
) {
    if let Some(velocity) = predicted_velocity {
        text.0 = format!("predicted {:.3}", velocity.xz().length());
    } else if let Some(velocity) = server_velocity {
        text.0 = format!("{:.3}", velocity.xz().length());
    }
}

fn update_status_text(
    local: Res<LocalPlayerId>,
    state: Res<State<ClientState>>,
    time_scale: Res<DebugTimeScale>,
    debug_ghosts: Res<RemoteGhostDebug>,
    mut text: Single<&mut Text, With<StatusText>>,
) {
    let mut status = format!("{} - {:?}", local.label(), state.get());
    if time_scale.is_scaled() {
        status.push_str(&format!(" - slowmo {:.2}x", time_scale.factor));
    }
    if debug_ghosts.visible {
        status.push_str(" - ghosts");
    }
    text.0 = status;
}

fn update_prediction_text(
    mut text: Single<&mut Text, With<PredictionText>>,
    input_state: Res<ClientInputState>,
    prediction: Option<Single<&PredictionCorrection, With<ClientPredictionKcc>>>,
) {
    let Some(prediction) = prediction else {
        text.0 = "prediction: waiting".to_string();
        return;
    };

    let ack_lag = input_state
        .next_sequence
        .wrapping_sub(prediction.last_ack_sequence);
    text.0 = format!(
        "prediction: {:?} | error {:.3}m | ack {} | lag {} | replay {} | offset {:.3}m",
        prediction.mode,
        prediction.last_error,
        prediction.last_ack_sequence,
        ack_lag,
        prediction.replayed_commands,
        prediction.presentation_offset.length()
    );
}

fn update_shot_text(feedback: Res<ShotFeedback>, mut text: Single<&mut Text, With<ShotText>>) {
    let predicted = if feedback.predicted.is_empty() {
        "client none"
    } else {
        feedback.predicted.as_str()
    };
    let acknowledged = if feedback.acknowledged.is_empty() {
        "server none"
    } else {
        feedback.acknowledged.as_str()
    };

    text.0 = format!("shots: {predicted} | {acknowledged}");
}
