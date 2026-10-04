//! Window, event loop, wiring between game, renderer, and network thread.
//!
//! The client opens a window, clears it every redraw through
//! [`oxide_render::renderer::Renderer`], and shows the live frame rate and the chosen adapter
//! in the title. Closing the window exits; Escape exits while the pointer is free and
//! releases it while grabbed. A bounded smoke run sets `OXIDECRAFT_MAX_FRAMES` to a frame
//! count; the client then exits cleanly once that many frames have been presented.
//!
//! With `--server host:port` a session thread joins the server and reports through a channel:
//! every frame the client drains it into the renderer and the F3 debug overlay, and the
//! session's clock and sky reports become the frame's fog and sky parameters. When the
//! session ends the client exits — a server that closed the connection cleanly is a normal
//! exit, not an error.
//!
//! With a session the window drives it: a click grabs the pointer, the grabbed pointer's
//! keys, buttons and motion become `InputEvent`s on the session's channel, Escape releases
//! capture (a second Escape exits), and a window that loses focus clears the held keys. The
//! grabbed key table is [`keymap`]'s. `--input-script <path>` replays a tick-indexed script
//! through the same channel and writes one CSV line per session tick to `<path>.log`; the
//! directive grammar and the log's shape are [`ScriptDriver`]'s.
//!
//! With `--server` the client also loads the asset store's extraction tree before the window
//! opens ([`assets::ClientAssets`]) and hands the atlas, the font and the sky textures to the
//! renderer once it exists. Without a server nothing is loaded and the M0 smoke path stands;
//! the overlay then has no sheet and draws nothing. `--no-overlay` suppresses the overlay at
//! startup and `--render-distance` sets the far plane, the fog distance and the view distance
//! the client reports.

mod assets;
mod keymap;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use assets::ClientAssets;
use clap::Parser;
use crossbeam_channel::{Receiver, Sender, unbounded};
use oxide_game::hud::{HudState, debug_lines};
use oxide_game::input::{InputEvent, Key, MouseButton};
use oxide_game::interaction::Aim;
use oxide_game::player::MAX_HURT_TIME;
use oxide_game::session::{ClientEvent, MeshAssets, Session, SessionConfig};
use oxide_proto_v47::serverbound::ClientSettings;
use oxide_render::camera::{
    Camera, CameraPose, CameraSensor, DEFAULT_FOV, FovInputs, FovSmoother, NEAR_PLANE,
    WalkDistance, bob_rotations, bob_translate, camera_effect, fov, fov_modifier, hurt_roll,
    interpolate_pose, render_eye,
};
use oxide_render::fog::{FogParams, fog_colour, linear_params};
use oxide_render::fps::FpsCounter;
use oxide_render::renderer::{Renderer, RendererError, SurfaceAction, classify_surface_error};
use oxide_render::sky::SkyParams;
use oxide_render::world_overlay::{Crack, FULL_CUBE, Outline};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{
    DeviceEvent, DeviceId, ElementState, KeyEvent, MouseButton as WinitMouseButton, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, DeviceEvents, EventLoop};
use winit::keyboard::{Key as WinitKey, NamedKey, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

/// The frame count that bounds a smoke run, read from the environment.
const MAX_FRAMES_VAR: &str = "OXIDECRAFT_MAX_FRAMES";

/// How many sections one column has.
const SECTIONS_PER_COLUMN: u8 = 16;

/// The interim death view's frame dim.
///
/// The first stop of the source's death-screen gradient:
/// `GuiGameOver.drawScreen` fills `drawGradientRect(0, 0, width, height,
/// 1615855616, -1602211792)`, and `1615855616` is `0x60500000` — alpha 96,
/// red 80, green and blue zero. The one flat quad is this milestone's
/// stand-in for the two-stop gradient; the bytes are written as the client's
/// own no-transfer-function floats.
const DEATH_DIM: [f32; 4] = [80.0 / 255.0, 0.0, 0.0, 96.0 / 255.0];

/// The interim death view's title, the source's `deathScreen.title` string.
const DEATH_TITLE: &str = "You died!";

/// The interim death view's prompt, the source's `deathScreen.respawn` string.
///
/// A click or Space is answered with one client-status respawn request, and
/// the server's clientbound 0x07 clears the view.
const DEATH_RESPAWN: &str = "Respawn";

/// The command line.
#[derive(Parser)]
#[command(name = "oxide-client", version, about = "Oxidecraft client")]
struct Cli {
    /// Server to join, for example 127.0.0.1:25565. Without it the client
    /// shows the window and connects to nothing.
    #[arg(long)]
    server: Option<String>,
    /// The offline-mode player name.
    #[arg(long, default_value = "OxideDev")]
    username: String,
    /// Stop after this many presented frames.
    #[arg(long)]
    frames: Option<u64>,
    /// Suppress the debug overlay at startup even when a session exists; F3 still toggles it.
    #[arg(long)]
    no_overlay: bool,
    /// The render distance in chunks: the camera's far plane, the fog distance and the view
    /// distance sent to the server.
    #[arg(long, default_value_t = 8)]
    render_distance: u8,
    /// Replay a tick-indexed input script (see [`ScriptDriver`]) and log one CSV line per
    /// session tick to `<path>.log`. Needs `--server`: the script is injected into the
    /// session's input channel.
    #[arg(long)]
    input_script: Option<PathBuf>,
}

/// The frame count to stop after: the flag when given, the environment variable
/// otherwise, and only when positive.
fn frame_limit(frames: Option<u64>) -> Option<u64> {
    let limit = frames.or_else(|| {
        std::env::var(MAX_FRAMES_VAR)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
    });
    limit.filter(|limit| *limit > 0)
}

/// Splits a `host:port` address into its parts, or explains why it cannot.
///
/// The split takes the last colon, so an IPv6 literal keeps its own colons in
/// the host.
fn parse_server_address(address: &str) -> anyhow::Result<(String, u16)> {
    let (host, port) = address
        .rsplit_once(':')
        .ok_or_else(|| anyhow::anyhow!("the server address must be host:port, got {address:?}"))?;
    let port = port
        .parse::<u16>()
        .map_err(|error| anyhow::anyhow!("the server port {port:?} is not a number: {error}"))?;
    anyhow::ensure!(!host.is_empty(), "the server address has no host");
    Ok((host.to_string(), port))
}

/// One input-script directive: one event, due at one session tick.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Directive {
    /// The tick the directive is applied at, once the session's count reaches it.
    tick: u64,
    /// The event injected then.
    event: InputEvent,
}

/// The `--input-script` replay: the parsed directives and the tick log.
///
/// A directive is applied when the session's tick count reaches its tick, in
/// file order, through the same channel the window's own events use — the
/// script is capture-independent, so a rig run needs no pointer.
///
/// Every tick the session reports is written to `<script path>.log` as one
/// CSV line, `tick,x,y,z,yaw,pitch,on_ground`, and nothing else — the
/// measurement record of a rig run. The log starts empty on every run. A snapped report — a server correction such as the join
/// teleport — carries the tick it lands on, which can repeat a tick already
/// logged; apart from that the count is gap-free.
struct ScriptDriver {
    /// The session's input channel, exactly what the window's own events use.
    input_tx: Sender<InputEvent>,
    /// The directives, in file order.
    directives: Vec<Directive>,
    /// How many directives have been applied.
    applied: usize,
    /// The tick log, one line per tick.
    log: File,
}

impl ScriptDriver {
    /// Parses a script and opens its tick log.
    ///
    /// The log is `<path>.log`, created (or truncated) here so a run's record
    /// is its own.
    fn load(path: &Path, input_tx: Sender<InputEvent>) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("the input script {} could not be read", path.display()))?;
        let directives = parse_script(&text)?;
        let mut log_path = path.as_os_str().to_owned();
        log_path.push(".log");
        let log_path = PathBuf::from(log_path);
        let log = File::create(&log_path)
            .with_context(|| format!("the tick log {} could not be created", log_path.display()))?;
        tracing::info!(
            script = %path.display(),
            log = %log_path.display(),
            directives = directives.len(),
            "the input script was loaded"
        );
        Ok(Self {
            input_tx,
            directives,
            applied: 0,
            log,
        })
    }

    /// Observes one session tick: writes its log line, then applies every
    /// directive the tick has reached, in file order.
    ///
    /// A directive whose tick is behind the first observed one is applied at
    /// once: the session's count starts at zero, but the window sees it only
    /// from the tick it first receives.
    ///
    /// A log write failure is returned to the caller. A send failure is not
    /// fatal: a closed channel means the session ended.
    #[allow(clippy::too_many_arguments)]
    fn observe(
        &mut self,
        tick: u64,
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
        on_ground: bool,
    ) -> std::io::Result<()> {
        writeln!(self.log, "{tick},{x},{y},{z},{yaw},{pitch},{on_ground}")?;
        while let Some(directive) = self.directives.get(self.applied) {
            if directive.tick > tick {
                break;
            }
            if self.input_tx.send(directive.event).is_err() {
                tracing::warn!("the session's input channel is closed; the script stopped");
            }
            self.applied += 1;
        }
        Ok(())
    }
}

/// Parses an input script into its directives, in file order.
///
/// The format is one directive per line — a decimal tick, then the directive
/// and its arguments: `12 key W down`, `15 mouse Left up`, `18 look 40 -5`.
/// Fields are whitespace-separated; a `#` starts a comment that runs to the
/// end of the line, and blank lines are ignored. A line the grammar does not
/// cover is refused, and the refusal names its line number.
fn parse_script(text: &str) -> anyhow::Result<Vec<Directive>> {
    let mut directives = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let code = match raw.find('#') {
            Some(comment) => &raw[..comment],
            None => raw,
        };
        let code = code.trim();
        if code.is_empty() {
            continue;
        }
        let fields: Vec<&str> = code.split_whitespace().collect();
        let directive =
            parse_directive(&fields).with_context(|| format!("input script line {}", index + 1))?;
        directives.push(directive);
    }
    Ok(directives)
}

/// Parses one line's fields: the tick, then one directive with its arguments.
fn parse_directive(fields: &[&str]) -> anyhow::Result<Directive> {
    let Some((&tick, rest)) = fields.split_first() else {
        anyhow::bail!("the line has no tick");
    };
    let tick: u64 = tick
        .parse()
        .map_err(|_| anyhow::anyhow!("the tick {tick:?} is not a whole number"))?;
    let Some((&name, arguments)) = rest.split_first() else {
        anyhow::bail!("the tick is not followed by a directive");
    };
    let event = match name {
        "key" => parse_key(arguments)?,
        "mouse" => parse_mouse(arguments)?,
        "look" => parse_look(arguments)?,
        other => anyhow::bail!("{other:?} is not one of key, mouse or look"),
    };
    Ok(Directive { tick, event })
}

/// Parses a `key <name> down|up` directive.
fn parse_key(arguments: &[&str]) -> anyhow::Result<InputEvent> {
    let [name, edge] = arguments else {
        anyhow::bail!("key takes a key name and down or up, got {arguments:?}");
    };
    let key = match *name {
        "W" => Key::W,
        "A" => Key::A,
        "S" => Key::S,
        "D" => Key::D,
        "Space" => Key::Space,
        "ShiftLeft" => Key::ShiftLeft,
        "ControlLeft" => Key::ControlLeft,
        other => anyhow::bail!("{other:?} is not a bound key"),
    };
    Ok(InputEvent::Key {
        key,
        pressed: parse_edge(edge)?,
    })
}

/// Parses a `mouse Left|Right down|up` directive.
fn parse_mouse(arguments: &[&str]) -> anyhow::Result<InputEvent> {
    let [name, edge] = arguments else {
        anyhow::bail!("mouse takes a button and down or up, got {arguments:?}");
    };
    let button = match *name {
        "Left" => MouseButton::Left,
        "Right" => MouseButton::Right,
        other => anyhow::bail!("{other:?} is not a bound mouse button"),
    };
    Ok(InputEvent::MouseButton {
        button,
        pressed: parse_edge(edge)?,
    })
}

/// Parses a `look <dx> <dy>` directive: one mouse delta in screen pixels.
fn parse_look(arguments: &[&str]) -> anyhow::Result<InputEvent> {
    let [dx, dy] = arguments else {
        anyhow::bail!("look takes a dx and a dy, got {arguments:?}");
    };
    let dx: f64 = dx
        .parse()
        .map_err(|_| anyhow::anyhow!("the look dx {dx:?} is not a number"))?;
    let dy: f64 = dy
        .parse()
        .map_err(|_| anyhow::anyhow!("the look dy {dy:?} is not a number"))?;
    Ok(InputEvent::MouseDelta { dx, dy })
}

/// Parses the `down` or `up` edge a key or mouse directive carries.
fn parse_edge(edge: &str) -> anyhow::Result<bool> {
    match edge {
        "down" => Ok(true),
        "up" => Ok(false),
        other => anyhow::bail!("{other:?} is neither down nor up"),
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // The client is built before the window: a bad --server is refused before
    // anything is opened.
    let mut app = ClientApp::new(Cli::parse())?;
    let event_loop = EventLoop::new()?;
    event_loop.run_app(&mut app)?;
    anyhow::ensure!(
        !app.stopped_on_error,
        "the client stopped after a window or GPU error"
    );
    Ok(())
}

/// The application handler: owns the window, the renderer and the session link,
/// and presents one frame per redraw.
struct ClientApp {
    /// The window, once the event loop has resumed and it has been created.
    window: Option<Arc<Window>>,
    /// The renderer for that window.
    renderer: Option<Renderer>,
    /// The frame-rate accounting shown in the title and the overlay.
    fps: FpsCounter,
    /// Frames presented since start.
    frames: u64,
    /// The frame count to stop after, when the smoke-run variable asks for one.
    max_frames: Option<u64>,
    /// Whether the client must report a failure once the event loop returns.
    stopped_on_error: bool,
    /// The session thread and the events it reports, when `--server` was given.
    session: Option<SessionLink>,
    /// The client's assets, when `--server` was given: the mesh inputs the session shares,
    /// the font and the sky textures the renderer uploads.
    assets: Option<ClientAssets>,
    /// The render distance in chunks: the camera's far plane, the fog's far plane and the
    /// view distance sent to the server.
    render_distance: u8,
    /// What the debug overlay reports, updated from the session's events.
    hud: HudState,
    /// The clock and the sky the session last reported.
    sky: SkyState,
    /// The latest pose the session reported, the pose before it, and the session's tick.
    ///
    /// A regular `PlayerTick` slides the current pose into the previous one, so a later
    /// frame can interpolate between them; a snapped one — a server correction — collapses
    /// the two, so nothing interpolates across the jump. The tick is the session's own
    /// 20 Hz clock, and it is what the cloud offset advances from.
    player: PlayerState,
    /// The camera's own per-tick state: the smoother the FOV base blends through, the walk
    /// distance and the two damped sensors the view bob reads, the hurt flash, and the
    /// death and arrival clocks the frame's terms and fraction use.
    camera: CameraState,
    /// Whether F3 has the overlay showing.
    overlay_visible: bool,
    /// Whether the session last reported the player dead.
    ///
    /// While it holds, the frame draws the interim death view — the dim quad
    /// and the two lines — instead of the debug overlay, and the session
    /// answers clicks and Space with respawn requests.
    dead: bool,
    /// The overlay pass's own inputs: the latest aim the session reported and
    /// the live destroy stages.
    ///
    /// The window keeps them so a frame can hand the outline the aim's cell
    /// and the crack every stage within the render distance, and the click
    /// paths can act on the aim.
    world_overlay: WorldOverlayState,
    /// The pointer-capture rules.
    capture: Capture,
    /// The `--input-script` replay, when the flag was given.
    script: Option<ScriptDriver>,
}

/// The world-overlay state the session's events keep for the frame.
#[derive(Debug, Default)]
struct WorldOverlayState {
    /// The latest aim the session reported, or `None` when the interaction ray
    /// meets no block: the outline draws its cell, `None` stops it.
    aim: Option<Aim>,
    /// The live destroy stages the crack draws: one entry per breaking block,
    /// keyed by the block's cell, holding the sprite stage `0..=9`.
    ///
    /// The session reports each landing and clearing as `BreakStage` and
    /// `BreakCleared`; the window keeps the map so a frame can hand the pass
    /// every stage within the render distance.
    break_stages: BTreeMap<[i32; 3], u8>,
}

/// The clock and the sky the session last reported: what each frame's sky parameters and fog
/// are built from.
#[derive(Debug, Default)]
struct SkyState {
    /// The world's time of day in ticks, from the last Time Update.
    time_of_day: Option<i64>,
    /// The sky the last Time Update or teleport produced.
    sky: Option<SkyValues>,
    /// The void-fog factor the last Join Game's level type asks for.
    void_y_factor: f32,
}

/// The world-derived sky values the session's `Sky` event carries.
#[derive(Debug, Clone, Copy)]
struct SkyValues {
    /// The celestial angle in `0..1`.
    celestial_angle: f32,
    /// The sky's colour at the view block.
    colour: [f32; 3],
    /// The sun's brightness.
    sun_brightness: f32,
    /// The stars' brightness.
    star_brightness: f32,
    /// The clouds' tint.
    cloud_colour: [f32; 3],
    /// The moon's phase in `0..8`.
    moon_phase: u8,
    /// The light level at the view block, `0..15`: the chain's brightness factor reads it
    /// (`EntityRenderer.java:362`).
    light_level: u8,
}

/// The pose the session last reported and the pose before it, plus the session's tick count.
///
/// A regular tick slides current into previous so a later frame can interpolate between
/// them; a snapped tick — a server correction — collapses the pair so nothing interpolates
/// across the jump.
#[derive(Debug, Default)]
struct PlayerState {
    /// The pose the previous `PlayerTick` reported.
    previous: Pose,
    /// The pose the latest `PlayerTick` reported.
    current: Pose,
    /// The tick the latest report carried: the session's 20 Hz clock.
    tick: u64,
}

/// One `PlayerTick`'s pose: the player's feet and where they look.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Pose {
    /// The feet's x, y and z in blocks.
    position: [f64; 3],
    /// The yaw in degrees.
    yaw: f32,
    /// The pitch in degrees.
    pitch: f32,
}

impl PlayerState {
    /// Folds one `PlayerTick` into the state.
    ///
    /// The pose it carries becomes current and the old current slides into previous —
    /// unless the report is snapped, which sets previous to current as well so nothing
    /// interpolates across a correction. The tick always advances.
    fn observe(&mut self, tick: u64, position: [f64; 3], yaw: f32, pitch: f32, snapped: bool) {
        self.previous = self.current;
        self.current = Pose {
            position,
            yaw,
            pitch,
        };
        if snapped {
            self.previous = self.current;
        }
        self.tick = tick;
    }
}

/// The camera's per-tick state: the FOV smoother, the walk distance and the damped camera
/// sensors the view bob reads, the hurt flash, and the death and arrival clocks.
///
/// The smoother steps once per tick toward `AbstractClientPlayer.getFovModifier`'s value —
/// what `EntityRenderer.updateFovModifierHand` blends (`EntityRenderer.java:527-530`), which
/// carries the flying factor and the sprint attribute but not the water or death terms. The
/// walk distance and the two sensors mirror `Entity.moveEntity`'s distance increment
/// (`Entity.java:872`), `Entity.onEntityUpdate`'s previous-tick copies (`:420`) and
/// `EntityPlayer.onLivingUpdate`'s damping (`EntityPlayer.java:653-654`). The tick's
/// displacement between the two reported positions stands in for the source's motion
/// vector, which the tick events do not carry.
#[derive(Debug, Default)]
struct CameraState {
    /// The FOV smoother the frame's base blends through.
    smoother: FovSmoother,
    /// The walk-distance accumulator and its previous tick's copy.
    walk: WalkDistance,
    /// The damped yaw sensor.
    camera_yaw: CameraSensor,
    /// The damped pitch sensor.
    camera_pitch: CameraSensor,
    /// The latest tick's hurt flash (`hurtTime`).
    hurt_time: u32,
    /// The yaw the last hurt came from (`attackedAtYaw`); zero until M4 tracks attackers.
    attacked_at_yaw: f32,
    /// Whether the latest tick reported the player in water, for the FOV's water term.
    in_water: bool,
    /// Ticks the death state has run (`deathTime`).
    death_time: u32,
    /// The feet position the latest tick reported, for the next tick's displacement.
    last_position: Option<[f64; 3]>,
    /// When the latest tick arrived, for the frame fraction.
    last_tick_arrival: Option<Instant>,
}

/// One `PlayerTick`'s camera-relevant state: the position the displacement is measured from,
/// the flags the smoother and the accumulators read, the hurt flash and the death state.
#[derive(Debug, Clone, Copy)]
struct CameraTick {
    /// The feet position the tick reported.
    position: [f64; 3],
    /// Whether the player stands on something.
    on_ground: bool,
    /// Whether the player is sprinting.
    sprinting: bool,
    /// Whether the player is sneaking.
    sneaking: bool,
    /// Whether the player is flying.
    flying: bool,
    /// Whether the player is in water.
    in_water: bool,
    /// Ticks left of the hurt flash.
    hurt_time: u32,
    /// The yaw the last hurt came from.
    attacked_at_yaw: f32,
    /// Whether a server correction produced this tick.
    snapped: bool,
    /// Whether the player is dead.
    dead: bool,
}

impl CameraState {
    /// Folds one `PlayerTick` into the state.
    ///
    /// The previous tick's copies slide in first, the smoother steps toward the tick's own
    /// modifier, and the walk distance and the two sensors advance. A snapped tick carries
    /// a server correction, so its displacement is neither walking nor motion.
    fn observe(&mut self, tick: CameraTick) {
        self.walk.previous = self.walk.distance;
        self.camera_yaw.previous = self.camera_yaw.value;
        self.camera_pitch.previous = self.camera_pitch.value;
        self.smoother.step(fov_modifier(&FovInputs {
            sprinting: tick.sprinting,
            flying: tick.flying,
            // The bow's counter arrives with M5; the term is the identity at zero.
            item_use_ticks: 0,
        }));
        self.hurt_time = tick.hurt_time;
        self.attacked_at_yaw = tick.attacked_at_yaw;
        self.in_water = tick.in_water;
        if tick.dead {
            self.death_time += 1;
        } else {
            self.death_time = 0;
        }
        let delta = if tick.snapped {
            [0.0, 0.0, 0.0]
        } else {
            match self.last_position {
                Some(previous) => [
                    tick.position[0] - previous[0],
                    tick.position[1] - previous[1],
                    tick.position[2] - previous[2],
                ],
                None => [0.0, 0.0, 0.0],
            }
        };
        self.last_position = Some(tick.position);
        // The walk-distance increment (`Entity.java:872`): the horizontal displacement
        // times 0.6, skipped while flying (`EntityPlayer.canTriggerWalking`,
        // `:2205-2208`), on the sneak glide on the ground (`Entity.java:626`) and on a
        // correction.
        if !tick.snapped && !tick.flying && !(tick.on_ground && tick.sneaking) {
            self.walk.distance += ((delta[0] * delta[0] + delta[2] * delta[2]).sqrt() * 0.6) as f32;
        }
        // The damped camera sensors (`EntityPlayer.java:635-654`): the yaw follows the
        // horizontal speed clamped to 0.1 and zeroed off the ground or dead, the pitch
        // follows the vertical motion's lean and is zeroed on the ground or dead.
        let mut speed = ((delta[0] * delta[0] + delta[2] * delta[2]).sqrt()).min(0.1) as f32;
        if !tick.on_ground || tick.dead {
            speed = 0.0;
        }
        let mut lean = ((-delta[1] * f64::from(0.2_f32)).atan() * 15.0) as f32;
        if tick.on_ground || tick.dead {
            lean = 0.0;
        }
        self.camera_yaw.value += (speed - self.camera_yaw.value) * 0.4;
        self.camera_pitch.value += (lean - self.camera_pitch.value) * 0.8;
    }

    /// The frame fraction: the time since the latest `PlayerTick` arrived over the 50 ms
    /// tick, clamped to `0..=1`; a frame before the first tick is at zero.
    fn partial(&self, now: Instant) -> f32 {
        match self.last_tick_arrival {
            Some(arrival) => (now.duration_since(arrival).as_secs_f32() / 0.05).clamp(0.0, 1.0),
            None => 0.0,
        }
    }
}

/// The camera pose of one reported tick pose.
fn camera_pose(pose: Pose) -> CameraPose {
    CameraPose {
        position: pose.position,
        yaw: pose.yaw,
        pitch: pose.pitch,
    }
}

/// The fog and the sky one frame draws with, from the session's clock and sky and the client's
/// own frame state.
///
/// The session owns the world, so the six world-derived values and the moon's phase come from
/// its `Sky` event; the fog colour, the far plane and the cloud counter are the client's own.
/// The far plane is the render distance in blocks — the source's `farPlaneDistance`, which the
/// sky pass doubles and the cloud pass quadruples — and the fog's range comes from
/// [`linear_params`] of it. The render distance's chunk count also feeds the fog colour's sky
/// mix and brightness factor (`EntityRenderer.java:1767-1768`, `:363-364`), so it travels
/// alongside the far plane it derives rather than being recovered from it.
fn frame_params(
    time_of_day: i64,
    dimension: i8,
    eye_y: f64,
    void_y_factor: f32,
    render_distance: u8,
    values: SkyValues,
    cloud_ticks: i64,
) -> (FogParams, SkyParams) {
    let far_plane = f32::from(render_distance) * 16.0;
    let (start, end) = linear_params(far_plane);
    let colour = fog_colour(
        dimension,
        time_of_day as f32,
        eye_y,
        void_y_factor,
        values.colour,
        render_distance,
        values.light_level,
    );
    let fog = FogParams {
        colour,
        start,
        end,
        far_plane,
    };
    let sky = SkyParams {
        celestial_angle: values.celestial_angle,
        sky_colour: values.colour,
        sun_brightness: values.sun_brightness,
        star_brightness: values.star_brightness,
        fog_colour: colour,
        far_plane,
        cloud_offset_ticks: cloud_ticks,
        cloud_colour: values.cloud_colour,
        moon_phase: values.moon_phase,
    };
    (fog, sky)
}

/// The void-fog factor a level type asks for: `WorldProvider.getVoidFogYFactor`
/// (`WorldProvider.java:231-234`), one in a flat world and 0.03125 otherwise.
fn void_y_factor(level_type: &str) -> f32 {
    if level_type == "flat" { 1.0 } else { 0.03125 }
}

impl ClientApp {
    /// Builds the handler, reads the smoke-run frame limit, loads the assets when a session
    /// was asked for, and opens the session.
    fn new(cli: Cli) -> anyhow::Result<Self> {
        let max_frames = frame_limit(cli.frames);
        if let Some(limit) = max_frames {
            tracing::info!(frames = limit, "a frame limit is set, exiting after it");
        }
        // With a server the assets load before anything opens, so a store that is missing or
        // malformed fails fast; the smoke path without one loads nothing.
        let mut assets = None;
        let session = match cli.server {
            Some(address) => {
                let (host, port) = parse_server_address(&address)?;
                tracing::info!(server = %address, username = %cli.username, "joining the server");
                let loaded = ClientAssets::load(None)?;
                let session = spawn_session(
                    host,
                    port,
                    cli.username,
                    address,
                    cli.render_distance,
                    Arc::clone(&loaded.mesh),
                );
                assets = Some(loaded);
                Some(session)
            }
            None => None,
        };
        // The script replay needs the session's own input channel: a script
        // with nowhere to inject is refused before the window opens, the same
        // way a bad `--server` is.
        let script = match (&cli.input_script, session.as_ref()) {
            (Some(path), Some(link)) => Some(ScriptDriver::load(path, link.input_tx.clone())?),
            (Some(_), None) => {
                anyhow::bail!("--input-script needs --server: there is no session to replay into")
            }
            (None, _) => None,
        };
        // The overlay starts visible when a session exists and `--no-overlay` did not
        // suppress it; the smoke path keeps it hidden until F3, and without a sheet it
        // draws nothing.
        let overlay_visible = session.is_some() && !cli.no_overlay;
        let server = session
            .as_ref()
            .map(|link| link.server.clone())
            .unwrap_or_default();
        Ok(Self {
            window: None,
            renderer: None,
            fps: FpsCounter::new(Duration::from_secs(1)),
            frames: 0,
            max_frames,
            stopped_on_error: false,
            session,
            assets,
            render_distance: cli.render_distance,
            hud: HudState {
                fps: 0.0,
                position: [0.0; 3],
                yaw: 0.0,
                pitch: 0.0,
                dimension: 0,
                server,
                entity_id: 0,
            },
            sky: SkyState::default(),
            player: PlayerState::default(),
            camera: CameraState::default(),
            world_overlay: WorldOverlayState::default(),
            overlay_visible,
            dead: false,
            capture: Capture::default(),
            script,
        })
    }

    /// Presents one frame, updates the title, and stops once the limit is reached.
    ///
    /// The session's events are drained first, so the meshes and the pose they
    /// carry are what this frame draws. A tick the script replay observes is
    /// logged and its due directives injected before the frame draws. A frame
    /// the surface is not ready for is dropped, and a surface that went stale
    /// is reconfigured before the frame is retried once. Any other failure
    /// stops the client.
    fn draw(&mut self, event_loop: &ActiveEventLoop) {
        // Drained into a list first, so the rest of the frame works from owned
        // events and holds no borrow of the session.
        let events: Vec<ClientEvent> = match self.session.as_ref() {
            Some(session) => session.events.try_iter().collect(),
            None => Vec::new(),
        };
        let (Some(window), Some(renderer)) = (self.window.as_ref(), self.renderer.as_mut()) else {
            return;
        };
        let mut session_ended = false;
        for event in events {
            if let ClientEvent::PlayerTick {
                tick,
                x,
                y,
                z,
                yaw,
                pitch,
                on_ground,
                sprinting,
                sneaking,
                flying,
                in_water,
                snapped,
                hurt_time,
                attacked_at_yaw,
                ..
            } = &event
            {
                if let Some(script) = self.script.as_mut() {
                    if let Err(error) = script.observe(*tick, *x, *y, *z, *yaw, *pitch, *on_ground)
                    {
                        // The record of the run is broken; stop rather than
                        // pretend the measurement is whole.
                        tracing::error!(%error, "the tick log could not be written");
                        self.stopped_on_error = true;
                        event_loop.exit();
                        return;
                    }
                }
                self.camera.observe(CameraTick {
                    position: [*x, *y, *z],
                    on_ground: *on_ground,
                    sprinting: *sprinting,
                    sneaking: *sneaking,
                    flying: *flying,
                    in_water: *in_water,
                    hurt_time: *hurt_time,
                    attacked_at_yaw: *attacked_at_yaw,
                    snapped: *snapped,
                    dead: self.dead,
                });
                self.camera.last_tick_arrival = Some(Instant::now());
            }
            session_ended |= apply_session_event(
                renderer,
                &mut self.hud,
                &mut self.sky,
                &mut self.player,
                &mut self.world_overlay,
                &mut self.dead,
                event,
            );
        }
        if session_ended {
            tracing::info!("the session ended, exiting");
            event_loop.exit();
            return;
        }
        // The camera follows the pose the session last reported. The smoke path
        // without a session stays as M0 left it: no camera, so the frame is the
        // sky clear.
        if self.session.is_some() {
            let partial = self.camera.partial(Instant::now());
            let pose = interpolate_pose(
                camera_pose(self.player.previous),
                camera_pose(self.player.current),
                partial,
            );
            let walk = self.camera.walk;
            let yaw_sensor = self.camera.camera_yaw;
            let pitch_sensor = self.camera.camera_pitch;
            // The frame's FOV chain and view effect: the smoothed setting with the water and
            // death terms, and the hurt roll over the view bob (`EntityRenderer.java:551-629`,
            // `:585-609`, `:615-629`), all at this frame's fraction.
            let frame_fov = fov(
                DEFAULT_FOV,
                &self.camera.smoother,
                partial,
                self.camera.in_water,
                self.dead,
                self.camera.death_time,
            );
            let bob_offset = bob_translate(walk, yaw_sensor, partial);
            let bob_turns = bob_rotations(walk, yaw_sensor, pitch_sensor, partial);
            let hurt_turns = hurt_roll(
                self.camera.hurt_time,
                MAX_HURT_TIME,
                partial,
                self.camera.attacked_at_yaw,
                self.dead,
                self.camera.death_time,
            );
            // The frame's camera record: one line per frame, the values a rig run
            // reconciles the live behaviour against (the smoke runs keep this level on).
            tracing::debug!(
                ?bob_offset,
                ?bob_turns,
                ?hurt_turns,
                eye = ?render_eye(&pose),
                yaw = pose.yaw,
                pitch = pose.pitch,
                fov = frame_fov,
                partial,
                "the frame camera"
            );
            renderer.set_camera(Camera {
                pose,
                fov_degrees: frame_fov,
                near: NEAR_PLANE,
                far_chunks: self.render_distance as f32,
                view_effect: camera_effect(bob_offset, &bob_turns, &hurt_turns),
            });
            // The world overlay's own inputs: the aim's block for the outline, and every
            // destroy stage the render distance can show for the crack.
            renderer.set_outline(self.world_overlay.aim.map(aim_outline));
            renderer.set_cracks(cracks_in_view(
                &self.world_overlay.break_stages,
                self.player.current.position,
                self.render_distance,
            ));
            if let (Some(time_of_day), Some(values)) = (self.sky.time_of_day, self.sky.sky) {
                let (fog, sky) = frame_params(
                    time_of_day,
                    self.hud.dimension,
                    self.player.current.position[1],
                    self.sky.void_y_factor,
                    self.render_distance,
                    values,
                    self.player.tick as i64,
                );
                renderer.set_fog(fog);
                renderer.set_sky(sky);
            }
        }
        // The death view replaces the debug overlay while the player is dead:
        // the dim quad over the scene and the two lines where the overlay's
        // text goes. Both are cleared when the respawn arrives.
        if self.dead {
            renderer.set_dim(Some(DEATH_DIM));
            renderer.set_overlay_lines(vec![DEATH_TITLE.to_string(), DEATH_RESPAWN.to_string()]);
        } else {
            renderer.set_dim(None);
            renderer.set_overlay_lines(if self.overlay_visible {
                debug_lines(&self.hud)
            } else {
                Vec::new()
            });
        }
        match present_frame(renderer) {
            Ok(PresentOutcome::Presented) => {}
            Ok(PresentOutcome::Skipped) => return,
            Err(error) => {
                tracing::error!(error = ?error, "the frame could not be presented");
                self.stopped_on_error = true;
                event_loop.exit();
                return;
            }
        }
        // Counted only now, once a frame has actually been presented: a frame
        // the surface was not ready for is not counted, and a frame the retry
        // presented counts once.
        self.fps.record_frame(Instant::now());
        self.frames += 1;
        let fps = self.fps.fps();
        // The overlay reports the rate the counter last measured, so it shows
        // this value from the next frame on.
        self.hud.fps = fps;
        let title = match self.session.as_ref() {
            Some(session) => format!(
                "Oxidecraft — {fps:.0} fps — {} — {}",
                renderer.adapter_name(),
                session.server
            ),
            None => format!("Oxidecraft — {fps:.0} fps — {}", renderer.adapter_name()),
        };
        window.set_title(title.as_str());
        if self.frames % 60 == 0 {
            tracing::info!(
                frames = self.frames,
                fps = format!("{fps:.1}"),
                "frame rate"
            );
        }
        if let Some(limit) = self.max_frames {
            if self.frames >= limit {
                tracing::info!(frames = self.frames, "frame limit reached, exiting");
                event_loop.exit();
            }
        }
    }

    /// Sends one input event to the session, when there is one.
    fn send_input(&self, event: InputEvent) {
        if let Some(session) = self.session.as_ref() {
            if session.input_tx.send(event).is_err() {
                tracing::warn!("the session's input channel is closed");
            }
        }
    }

    /// Routes one keyboard event: the window's shortcuts, then the gameplay keys.
    ///
    /// The shortcuts always work; gameplay keys are translated from the
    /// physical key and flow only while the pointer is grabbed — while it is
    /// free there is no game to steer, and the click that grabs arrives first.
    fn on_key(&mut self, event_loop: &ActiveEventLoop, event: KeyEvent) {
        if is_escape_press(event.state, event.repeat, &event.logical_key) {
            let step = self.capture.escape();
            self.apply_capture(event_loop, step);
            return;
        }
        if is_f3_press(event.state, &event.logical_key) {
            self.overlay_visible = !self.overlay_visible;
            tracing::info!(
                visible = self.overlay_visible,
                "the debug overlay was toggled"
            );
            return;
        }
        if let Some(input) = gameplay_key(self.capture.grabbed(), event.state, event.physical_key) {
            self.send_input(input);
        }
    }

    /// Routes one mouse button event: the click that grabs, or the gameplay
    /// button.
    ///
    /// The grabbing click is consumed — the source's own first press goes to
    /// `setIngameFocus` (`Minecraft.java:1887-1891`), not to the game — and
    /// with no session there is nothing to grab for.
    fn on_mouse_button(
        &mut self,
        event_loop: &ActiveEventLoop,
        state: ElementState,
        button: WinitMouseButton,
    ) {
        if !self.capture.grabbed() {
            if state == ElementState::Pressed && self.session.is_some() {
                let step = self.capture.click();
                self.apply_capture(event_loop, step);
            }
            return;
        }
        if let Some(button) = bound_mouse_button(button) {
            self.send_input(InputEvent::MouseButton {
                button,
                pressed: state == ElementState::Pressed,
            });
        }
    }

    /// Applies one capture decision to the window and the session.
    ///
    /// A grab hides the pointer and locks it: `Locked` is asked for first and
    /// `Confined` is the fallback — X11 implements only the latter, Wayland
    /// both. A release restores the pointer and sends
    /// [`InputEvent::FocusLost`], the key-clearing event; an exit ends the
    /// client.
    fn apply_capture(&mut self, event_loop: &ActiveEventLoop, step: CaptureStep) {
        match step {
            CaptureStep::Consumed => {}
            CaptureStep::Grab => {
                let Some(window) = self.window.as_ref() else {
                    return;
                };
                let grab = window
                    .set_cursor_grab(CursorGrabMode::Locked)
                    .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
                match grab {
                    Ok(()) => {
                        window.set_cursor_visible(false);
                        tracing::info!("the pointer was grabbed");
                    }
                    Err(error) => {
                        // The pointer never left the window manager's
                        // control; the capture state must not claim it did.
                        self.capture = Capture::default();
                        tracing::warn!(%error, "the pointer could not be grabbed");
                    }
                }
            }
            CaptureStep::Release => {
                if let Some(window) = self.window.as_ref() {
                    if let Err(error) = window.set_cursor_grab(CursorGrabMode::None) {
                        tracing::warn!(%error, "the pointer could not be released");
                    }
                    window.set_cursor_visible(true);
                }
                if let Some(input) = step.input() {
                    self.send_input(input);
                }
                tracing::info!("the pointer was released and the held keys were cleared");
            }
            CaptureStep::Exit => {
                tracing::info!("escape was pressed, exiting");
                event_loop.exit();
            }
        }
    }
}

/// The session thread's end of the wiring: where it came from and what it has
/// reported so far.
struct SessionLink {
    /// The address as the command line gave it, shown on the overlay.
    server: String,
    /// The events the session thread reports, drained once per frame.
    events: Receiver<ClientEvent>,
    /// The window's end of the input channel; the session thread drains the other
    /// end.
    ///
    /// The window's own events and the `--input-script` replay both write here;
    /// the link owns the sender so the channel lives as long as the session does.
    input_tx: Sender<InputEvent>,
}

/// Opens a session on its own thread and returns the link the window drains.
///
/// The thread owns the connection, because the session blocks on reads and the
/// window must keep drawing. It runs to completion, reports the failure in the
/// log if it has one, and then reports [`ClientEvent::Disconnected`] whatever
/// the outcome was, so the window can stop. A session that returned cleanly is
/// a normal exit: the server closed the connection. The client settings carry the
/// command line's view distance, and the `mesh` assets are the bootstrap's. The
/// input channel is opened here too: the thread drains the receiver, the link
/// keeps the sender.
fn spawn_session(
    host: String,
    port: u16,
    username: String,
    server: String,
    render_distance: u8,
    mesh: Arc<MeshAssets>,
) -> SessionLink {
    let (sender, receiver) = unbounded();
    let (input_tx, input_rx) = unbounded();
    std::thread::spawn(move || {
        let config = SessionConfig {
            host,
            port,
            username,
            settings: ClientSettings {
                view_distance: render_distance,
                ..Default::default()
            },
            mesh: Some(mesh),
        };
        match Session::connect(&config) {
            Ok(session) => {
                if let Err(error) = session.run(&sender, input_rx) {
                    tracing::error!(error = ?error, "the session ended with an error");
                }
            }
            Err(error) => tracing::error!(error = ?error, "the connection could not be opened"),
        }
        let _ = sender.send(ClientEvent::Disconnected {
            reason: "the session thread ended".into(),
        });
    });
    SessionLink {
        server,
        events: receiver,
        input_tx,
    }
}

/// Stores the aim a session report carries, and answers whether it moved.
///
/// The window's outline and crack passes draw from the stored aim and the
/// click paths act on it; a report that clears the aim is stored as `None`, so
/// a look that leaves every block stops the outline.
fn store_aim(aim: &mut Option<Aim>, report: Option<Aim>) -> bool {
    let moved = *aim != report;
    *aim = report;
    moved
}

/// The outline the aim draws: the aimed block's cell and its box.
///
/// The session publishes no block state with the aim, so the box is [`FULL_CUBE`] — the box
/// every full-block class's selection bounds report (`RenderGlobal.java:1895`). A block whose
/// behaviour-table shape is partial (stairs, slabs, fences, panes, walls, doors) is a recorded
/// class for the milestone that carries block state into the frame.
fn aim_outline(aim: Aim) -> Outline {
    Outline {
        block: [aim.x, aim.y, aim.z],
        shape: FULL_CUBE,
    }
}

/// The crack entries a frame draws: the stage map's entries within `render_distance` chunks of
/// `eye`, in cell order.
///
/// The overlay itself drops entries beyond the source's 32-block reach
/// (`RenderGlobal.java:1843-1848`); the window's own filter keeps a stage left behind by a
/// walked-away block out of the pass at all.
fn cracks_in_view(
    stages: &BTreeMap<[i32; 3], u8>,
    eye: [f64; 3],
    render_distance: u8,
) -> Vec<Crack> {
    let limit = f64::from(render_distance) * 16.0;
    stages
        .iter()
        .filter(|(block, _)| {
            let dx = f64::from(block[0]) + 0.5 - eye[0];
            let dy = f64::from(block[1]) + 0.5 - eye[1];
            let dz = f64::from(block[2]) + 0.5 - eye[2];
            dx * dx + dy * dy + dz * dz <= limit * limit
        })
        .map(|(&block, &stage)| Crack { block, stage })
        .collect()
}

/// Stores a destroy stage the session reported, and answers whether the map moved.
///
/// The reports land in the source's `destroyBlockIcons` map, keyed by block position
/// (`RenderGlobal.java:1789-1794`); a repeat of the stage already stored is no move, so the
/// caller's log line fires once per change.
fn store_break_stage(
    stages: &mut BTreeMap<[i32; 3], u8>,
    x: i32,
    y: i32,
    z: i32,
    stage: u8,
) -> bool {
    stages.insert([x, y, z], stage) != Some(stage)
}

/// Removes a block's destroy stage, and answers whether the map moved.
///
/// The session reports the removal when a dig stops or completes; the answer tells the caller
/// whether the log line is worth writing.
fn clear_break_stage(stages: &mut BTreeMap<[i32; 3], u8>, x: i32, y: i32, z: i32) -> bool {
    stages.remove(&[x, y, z]).is_some()
}

/// Applies one event the session reported: meshes go to the renderer, the pose
/// to the view state and the overlay, the join parameters to the overlay state,
/// the clock and the sky to the frame's parameters, and the aim to the
/// window's own copy.
///
/// Returns whether the session ended, which stops the client. The session
/// returns `Ok(())` when the server closed the connection, so its end is a
/// normal exit, not an error.
fn apply_session_event(
    renderer: &mut Renderer,
    hud: &mut HudState,
    sky: &mut SkyState,
    player: &mut PlayerState,
    world_overlay: &mut WorldOverlayState,
    dead: &mut bool,
    event: ClientEvent,
) -> bool {
    match event {
        ClientEvent::LoggedIn { uuid, username } => {
            tracing::info!(%uuid, %username, "logged in");
            false
        }
        ClientEvent::Joined {
            entity_id,
            gamemode,
            dimension,
            difficulty,
            max_players,
            level_type,
        } => {
            tracing::info!(
                entity_id,
                gamemode,
                dimension,
                difficulty,
                max_players,
                %level_type,
                "joined the world"
            );
            hud.entity_id = entity_id;
            hud.dimension = dimension;
            sky.void_y_factor = void_y_factor(&level_type);
            false
        }
        ClientEvent::PlayerTick {
            x,
            y,
            z,
            yaw,
            pitch,
            tick,
            snapped,
            ..
        } => {
            hud.position = [x, y, z];
            hud.yaw = yaw;
            hud.pitch = pitch;
            player.observe(tick, [x, y, z], yaw, pitch, snapped);
            false
        }
        ClientEvent::Aim { aim: report } => {
            if store_aim(&mut world_overlay.aim, report) {
                tracing::debug!(?report, "the aim moved");
            }
            false
        }
        ClientEvent::BreakStage { x, y, z, stage } => {
            // The crack overlay's own input: the stage lands in the map a frame
            // hands the pass, and the log line fires once per change.
            if store_break_stage(&mut world_overlay.break_stages, x, y, z, stage) {
                tracing::debug!(x, y, z, stage, "a destroy stage landed");
            }
            false
        }
        ClientEvent::BreakCleared { x, y, z } => {
            if clear_break_stage(&mut world_overlay.break_stages, x, y, z) {
                tracing::debug!(x, y, z, "a destroy stage left");
            }
            false
        }
        ClientEvent::Time {
            world_age,
            time_of_day,
        } => {
            tracing::debug!(world_age, time_of_day, "the clock was set");
            sky.time_of_day = Some(time_of_day);
            false
        }
        ClientEvent::Sky {
            celestial_angle,
            colour,
            sun_brightness,
            star_brightness,
            cloud_colour,
            moon_phase,
            light_level,
        } => {
            // The terrain lightmap follows the clock's own sun brightness.
            renderer.set_lightmap(sun_brightness);
            sky.sky = Some(SkyValues {
                celestial_angle,
                colour,
                sun_brightness,
                star_brightness,
                cloud_colour,
                moon_phase,
                light_level,
            });
            false
        }
        ClientEvent::ChunkUpdated { cx, cz, sections } => {
            for (section, mesh) in &sections {
                renderer.set_section_mesh((cx, cz, *section as u8), mesh.as_ref());
            }
            false
        }
        ClientEvent::ChunkUnloaded { cx, cz } => {
            for section in 0..SECTIONS_PER_COLUMN {
                renderer.set_section_mesh((cx, cz, section), None);
            }
            false
        }
        ClientEvent::Health {
            health,
            food,
            saturation,
        } => {
            tracing::debug!(health, food, saturation, "the health was set");
            false
        }
        ClientEvent::Died => {
            // The death view goes up: the dim quad and the two lines, drawn
            // from the next frame until the respawn clears them. The session
            // has already gated the input and cancelled the dig.
            tracing::info!("the player died; the death view is up");
            *dead = true;
            false
        }
        ClientEvent::Respawned {
            dimension,
            gamemode,
        } => {
            tracing::info!(dimension, gamemode, "the player respawned");
            *dead = false;
            hud.dimension = dimension;
            false
        }
        ClientEvent::WorldCleared => {
            // The world those meshes were built from is gone; the session's
            // fresh columns replace them as they arrive.
            tracing::info!("the world was rebuilt; the section meshes are dropped");
            renderer.clear_section_meshes();
            false
        }
        ClientEvent::KeepAlive { id } => {
            tracing::debug!(id, "keepalive answered");
            false
        }
        ClientEvent::Disconnected { reason } => {
            tracing::info!(%reason, "the session ended");
            true
        }
    }
}

/// Whether a redraw presented a frame or the surface was not ready for one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentOutcome {
    /// The frame was cleared and presented.
    Presented,
    /// No frame was presented; the surface was not ready and the next redraw tries again.
    Skipped,
}

/// Renders and presents one frame, recovering a stale surface once.
///
/// `Outdated` and `Lost` mean the surface went stale: it is reconfigured from its stored
/// configuration and the frame retried a single time. `Timeout` drops the frame and the next
/// redraw tries again. Every other failure is returned for the caller to treat as fatal.
fn present_frame(renderer: &mut Renderer) -> Result<PresentOutcome, RendererError> {
    match renderer.render() {
        Ok(()) => Ok(PresentOutcome::Presented),
        Err(RendererError::Frame(error)) => match classify_surface_error(&error) {
            SurfaceAction::Reconfigure => {
                tracing::warn!(
                    ?error,
                    "the surface went stale, reconfiguring it and retrying the frame"
                );
                renderer.reconfigure();
                renderer.render().map(|()| PresentOutcome::Presented)
            }
            SurfaceAction::SkipFrame => {
                tracing::debug!(?error, "the surface was not ready, skipping this frame");
                Ok(PresentOutcome::Skipped)
            }
            SurfaceAction::Fatal => Err(RendererError::Frame(error)),
        },
        Err(error) => Err(error),
    }
}

/// The pointer-capture rules.
///
/// The pointer starts free: gameplay input is suppressed until the first
/// click grabs it. Escape releases capture while it is grabbed — the source's
/// own focus loss is the model, `Minecraft.setIngameNotInFocus`
/// (`Minecraft.java:1469-1478`), which unpresses every held key and ungrabs
/// the cursor — and exits while free. Losing focus drops capture the same
/// way; nothing recaptures on focus gain: the next click does.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Capture {
    /// Whether the pointer is grabbed.
    grabbed: bool,
}

/// What one capture event asks the window to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureStep {
    /// The event is consumed by the capture rules; nothing is sent or changed.
    Consumed,
    /// Grab the pointer; the press that asked for it is consumed.
    Grab,
    /// Release the pointer and send [`InputEvent::FocusLost`], the
    /// key-clearing event.
    Release,
    /// The client should exit.
    Exit,
}

impl Capture {
    /// Whether the pointer is grabbed: gameplay input flows only while it is.
    fn grabbed(self) -> bool {
        self.grabbed
    }

    /// One mouse button press: the first press while free grabs the pointer.
    ///
    /// A press that arrives while the pointer is already grabbed is not the
    /// capture rules' to consume — it is the caller's gameplay button and
    /// travels on.
    fn click(&mut self) -> CaptureStep {
        if self.grabbed {
            CaptureStep::Consumed
        } else {
            self.grabbed = true;
            CaptureStep::Grab
        }
    }

    /// One Escape press: releases while grabbed, asks to exit while free.
    ///
    /// The release is what makes the second Escape the way out: the first
    /// ends capture, the next one — with the pointer free — exits.
    fn escape(&mut self) -> CaptureStep {
        if self.grabbed {
            self.grabbed = false;
            CaptureStep::Release
        } else {
            CaptureStep::Exit
        }
    }

    /// The window lost focus: capture is dropped and the keys are cleared.
    ///
    /// The release carries [`InputEvent::FocusLost`] even when the pointer
    /// was already free, so a session's held keys cannot survive the window
    /// losing them.
    fn focus_lost(&mut self) -> CaptureStep {
        self.grabbed = false;
        CaptureStep::Release
    }
}

impl CaptureStep {
    /// The input event this step sends, if any.
    ///
    /// The release carries the key-clearing event, so what capture loses is
    /// what the session releases.
    fn input(self) -> Option<InputEvent> {
        match self {
            CaptureStep::Release => Some(InputEvent::FocusLost),
            CaptureStep::Consumed | CaptureStep::Grab | CaptureStep::Exit => None,
        }
    }
}

/// Translates one key edge into the gameplay input it is, while grabbed.
///
/// Returns `None` while the pointer is free — gameplay input is suppressed
/// until the click that grabs arrives — and for a key the client does not
/// bind; a bound key's edge travels as [`InputEvent::Key`].
fn gameplay_key(
    grabbed: bool,
    state: ElementState,
    physical_key: PhysicalKey,
) -> Option<InputEvent> {
    if !grabbed {
        return None;
    }
    let PhysicalKey::Code(code) = physical_key else {
        return None;
    };
    keymap::translate(code).map(|key| InputEvent::Key {
        key,
        pressed: state == ElementState::Pressed,
    })
}

/// The bound mouse button a window button maps to, or `None` when unbound.
fn bound_mouse_button(button: WinitMouseButton) -> Option<MouseButton> {
    match button {
        WinitMouseButton::Left => Some(MouseButton::Left),
        WinitMouseButton::Right => Some(MouseButton::Right),
        _ => None,
    }
}

impl ApplicationHandler for ClientApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // The look rides raw device motion, which winit reports only for the
        // focused window by default; the grab state is this client's own gate,
        // so the stream is asked for unconditionally.
        event_loop.listen_device_events(DeviceEvents::Always);
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Oxidecraft")
            .with_inner_size(LogicalSize::new(1280.0, 720.0));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                tracing::error!(%error, "the window could not be created");
                self.stopped_on_error = true;
                event_loop.exit();
                return;
            }
        };
        match Renderer::new(&window) {
            Ok(renderer) => {
                tracing::info!(adapter = renderer.adapter_name(), "renderer ready");
                self.renderer = Some(renderer);
            }
            Err(error) => {
                tracing::error!(error = ?error, "the renderer could not be created");
                self.stopped_on_error = true;
                event_loop.exit();
                return;
            }
        }
        // The bootstrap's assets land once, before the first frame: the session's mesh
        // inputs go to the session (they were handed in at startup) and the atlas, the font
        // and the sky textures to the renderer here.
        if let (Some(assets), Some(renderer)) = (&self.assets, self.renderer.as_mut()) {
            tracing::debug!(
                font_height = assets.font.height(),
                "uploading the client's atlas, font and sky textures"
            );
            renderer.set_atlas(&assets.mesh.atlas);
            if let Err(error) = renderer.set_font(&assets.sheet) {
                // Unreachable after `ClientAssets::load` measured the same sheet; a sheet
                // the overlay refuses is fatal rather than silently defaulted.
                tracing::error!(error = ?error, "the font sheet was refused");
                self.stopped_on_error = true;
                event_loop.exit();
                return;
            }
            renderer.set_sky_textures(assets.sky_textures.clone());
        }
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                tracing::info!("the window was closed, exiting");
                event_loop.exit();
            }
            WindowEvent::KeyboardInput { event, .. } => self.on_key(event_loop, event),
            WindowEvent::MouseInput { state, button, .. } => {
                self.on_mouse_button(event_loop, state, button);
            }
            WindowEvent::Focused(false) => {
                tracing::info!("the window lost focus, dropping capture");
                let step = self.capture.focus_lost();
                self.apply_capture(event_loop, step);
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size);
                }
            }
            WindowEvent::RedrawRequested => self.draw(event_loop),
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        // The look is raw device motion (`MouseHelper.mouseXYChange`,
        // `MouseHelper.java:33-37`) and flows only while the pointer is
        // grabbed: a free cursor's motion never turns the player.
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
            if self.capture.grabbed() {
                self.send_input(InputEvent::MouseDelta { dx, dy });
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        tracing::info!(frames = self.frames, "client exiting");
    }
}

/// Whether a key event is a fresh press of Escape.
///
/// A held key's auto-repeat carries `repeat` and is not a fresh press: while
/// the pointer is grabbed, holding Escape releases capture once and must not
/// walk on into the exit the next press asks for. Kept out of the event match
/// so the shortcut rule can be pinned without an event loop.
fn is_escape_press(state: ElementState, repeat: bool, key: &WinitKey) -> bool {
    state == ElementState::Pressed && !repeat && *key == WinitKey::Named(NamedKey::Escape)
}

/// Whether a key event is a press of F3.
///
/// Kept out of the event match so the overlay toggle can be pinned without an event loop.
fn is_f3_press(state: ElementState, key: &WinitKey) -> bool {
    state == ElementState::Pressed && *key == WinitKey::Named(NamedKey::F3)
}

#[cfg(test)]
mod tests {
    //! Key-routing and command-line tests.

    use super::{
        Aim, CameraState, CameraTick, Capture, CaptureStep, Cli, ClientApp, DEATH_DIM,
        DEATH_RESPAWN, DEATH_TITLE, Directive, Key, MouseButton, PlayerState, ScriptDriver,
        SkyValues, aim_outline, bound_mouse_button, clear_break_stage, cracks_in_view,
        frame_params, gameplay_key, is_escape_press, is_f3_press, parse_script,
        parse_server_address, store_aim, store_break_stage, void_y_factor,
    };
    use clap::Parser;
    use crossbeam_channel::unbounded;
    use oxide_game::input::InputEvent;
    use oxide_game::interaction::Face;
    use oxide_render::world_overlay::{Crack, FULL_CUBE, Outline};
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};
    use winit::event::{ElementState, MouseButton as WinitMouseButton};
    use winit::keyboard::{Key as WinitKey, KeyCode, NamedKey, PhysicalKey};

    #[test]
    fn an_escape_press_exits() {
        assert!(is_escape_press(
            ElementState::Pressed,
            false,
            &WinitKey::Named(NamedKey::Escape)
        ));
    }

    #[test]
    fn an_escape_release_and_other_keys_do_not_exit() {
        assert!(!is_escape_press(
            ElementState::Released,
            false,
            &WinitKey::Named(NamedKey::Escape)
        ));
        assert!(!is_escape_press(
            ElementState::Pressed,
            false,
            &WinitKey::Character("w".into())
        ));
    }

    #[test]
    fn only_a_fresh_escape_press_counts() {
        // A held key's auto-repeat is not a second press: it must not walk
        // the capture rules on from a release into an exit.
        assert!(!is_escape_press(
            ElementState::Pressed,
            true,
            &WinitKey::Named(NamedKey::Escape)
        ));
    }

    #[test]
    fn only_a_press_of_f3_toggles() {
        assert!(is_f3_press(
            ElementState::Pressed,
            &WinitKey::Named(NamedKey::F3)
        ));
        assert!(!is_f3_press(
            ElementState::Released,
            &WinitKey::Named(NamedKey::F3)
        ));
        assert!(!is_f3_press(
            ElementState::Pressed,
            &WinitKey::Character("f3".into())
        ));
    }

    #[test]
    fn the_frame_flag_parses() {
        let cli =
            Cli::try_parse_from(["oxide-client", "--frames", "120"]).expect("the flag parses");
        assert_eq!(cli.frames, Some(120));
    }

    #[test]
    fn the_defaults_are_an_offline_username_and_no_server() {
        let cli = Cli::try_parse_from(["oxide-client"]).expect("a bare invocation parses");
        assert_eq!(cli.username, "OxideDev");
        assert!(cli.server.is_none());
        assert_eq!(cli.frames, None);
        assert!(!cli.no_overlay);
        assert_eq!(cli.render_distance, 8);
    }

    #[test]
    fn the_comparison_flags_parse() {
        let cli = Cli::try_parse_from(["oxide-client", "--no-overlay", "--render-distance", "12"])
            .expect("the flags parse");
        assert!(cli.no_overlay);
        assert_eq!(cli.render_distance, 12);
    }

    #[test]
    fn a_server_address_splits_into_a_host_and_a_port() {
        let (host, port) = parse_server_address("localhost:25566").expect("the address parses");
        assert_eq!(host, "localhost");
        assert_eq!(port, 25566);
    }

    #[test]
    fn a_server_without_a_port_is_refused() {
        let error = parse_server_address("127.0.0.1").expect_err("the address must be refused");
        assert!(error.to_string().contains("host:port"), "{error}");
    }

    #[test]
    fn a_server_port_that_is_not_a_number_is_refused() {
        let error = parse_server_address("127.0.0.1:game").expect_err("the port must be refused");
        assert!(error.to_string().contains("port"), "{error}");
    }

    #[test]
    fn the_smoke_path_starts_without_a_session_or_the_overlay() {
        let cli = Cli::try_parse_from(["oxide-client"]).expect("a bare invocation parses");
        let app = ClientApp::new(cli).expect("the client builds without a server");
        assert!(app.session.is_none(), "no session is opened");
        assert!(!app.overlay_visible, "the overlay stays hidden");
    }

    #[test]
    fn only_a_flat_world_asks_for_the_full_void_fog_factor() {
        assert_eq!(void_y_factor("flat"), 1.0);
        assert_eq!(void_y_factor("default"), 0.03125);
    }

    #[test]
    fn the_pose_state_slides_a_regular_tick_and_collapses_a_snapped_one() {
        let mut player = PlayerState::default();
        player.observe(1, [0.0, 64.0, 0.0], 0.0, 0.0, false);
        player.observe(2, [1.5, 64.0, -2.0], 10.0, -5.0, false);
        assert_eq!(
            player.previous.position,
            [0.0, 64.0, 0.0],
            "the previous pose slides into previous"
        );
        assert_eq!(player.current.position, [1.5, 64.0, -2.0]);
        assert_eq!(player.tick, 2);
        player.observe(3, [9.0, 70.0, 9.0], 90.0, 45.0, true);
        assert_eq!(
            player.previous, player.current,
            "a snapped tick collapses the pair: nothing interpolates across a correction"
        );
        assert_eq!(player.current.position, [9.0, 70.0, 9.0]);
        assert_eq!(player.tick, 3, "the tick advances even when snapped");
    }

    #[test]
    fn the_cloud_offset_is_the_tick_the_session_last_reported() {
        // The client's cloud phase is the session's 20 Hz clock, not a frame counter:
        // the offset a frame draws with is the tick the last PlayerTick carried.
        let mut player = PlayerState::default();
        player.observe(41, [0.0, 64.0, 0.0], 0.0, 0.0, false);
        let values = SkyValues {
            celestial_angle: 0.25,
            colour: [0.5, 0.6, 0.7],
            sun_brightness: 0.9,
            star_brightness: 0.1,
            cloud_colour: [1.0, 0.5, 0.25],
            moon_phase: 5,
            light_level: 15,
        };
        let (_, sky) = frame_params(6000, 0, 64.0, 0.03125, 8, values, player.tick as i64);
        assert_eq!(sky.cloud_offset_ticks, 41);
    }

    #[test]
    fn one_frames_parameters_carry_the_clock_the_sky_and_the_fog() {
        let values = SkyValues {
            celestial_angle: 0.25,
            colour: [0.5, 0.6, 0.7],
            sun_brightness: 0.9,
            star_brightness: 0.1,
            cloud_colour: [1.0, 0.5, 0.25],
            moon_phase: 5,
            light_level: 15,
        };
        let (fog, sky) = frame_params(6000, 0, 64.0, 0.03125, 8, values, 7);
        // The Overworld's noon fog at the eye on the ground starts from the provider's base
        // (`WorldProvider.getFogColor`, `WorldProvider.java:181-183`) and takes the render
        // distance's sky mix: 0.186711... of the way to the frame's own sky colour
        // (`EntityRenderer.java:1767-1768`, `:1803-1805`), with a full-light block leaving the
        // brightness factor at one (`:363-364`). The bytes are the frame's readout.
        assert_eq!(
            fog.colour.map(|channel| (channel * 255.0).round() as u8),
            [180, 204, 241]
        );
        assert_eq!((fog.start, fog.end, fog.far_plane), (96.0, 128.0, 128.0));
        assert_eq!(sky.celestial_angle, 0.25);
        assert_eq!(sky.sky_colour, [0.5, 0.6, 0.7]);
        assert_eq!(sky.sun_brightness, 0.9);
        assert_eq!(sky.star_brightness, 0.1);
        assert_eq!(sky.fog_colour, fog.colour);
        assert_eq!(sky.far_plane, 128.0);
        assert_eq!(sky.cloud_offset_ticks, 7);
        assert_eq!(sky.cloud_colour, [1.0, 0.5, 0.25]);
        assert_eq!(sky.moon_phase, 5);
    }

    #[test]
    fn the_input_script_flag_parses() {
        let cli = Cli::try_parse_from(["oxide-client", "--input-script", "walk.script"])
            .expect("the flag parses");
        assert_eq!(cli.input_script, Some(PathBuf::from("walk.script")));
        let cli = Cli::try_parse_from(["oxide-client"]).expect("a bare invocation parses");
        assert!(cli.input_script.is_none());
    }

    #[test]
    fn an_input_script_without_a_server_is_refused() {
        let cli = Cli::try_parse_from(["oxide-client", "--input-script", "walk.script"])
            .expect("the flags parse");
        let error = ClientApp::new(cli)
            .err()
            .expect("a script with no session is refused");
        assert!(error.to_string().contains("--server"), "{error}");
    }

    #[test]
    fn a_click_grabs_and_the_first_escape_releases_without_exiting() {
        let mut capture = Capture::default();
        assert!(!capture.grabbed(), "the pointer starts free");
        assert_eq!(capture.click(), CaptureStep::Grab);
        assert!(capture.grabbed());
        assert_eq!(
            capture.click(),
            CaptureStep::Consumed,
            "a press that arrives while grabbed is the caller's gameplay button"
        );
        assert!(capture.grabbed(), "the consumed press leaves capture alone");
        assert_eq!(
            capture.escape(),
            CaptureStep::Release,
            "escape while grabbed releases capture instead of asking to exit"
        );
        assert!(!capture.grabbed());
    }

    #[test]
    fn escape_while_free_asks_to_exit_and_the_second_escape_ends_it() {
        let mut capture = Capture::default();
        assert_eq!(
            capture.escape(),
            CaptureStep::Exit,
            "escape while free exits"
        );
        assert_eq!(capture.click(), CaptureStep::Grab);
        assert_eq!(
            capture.escape(),
            CaptureStep::Release,
            "the first escape releases"
        );
        assert_eq!(
            capture.escape(),
            CaptureStep::Exit,
            "the second escape exits"
        );
    }

    #[test]
    fn a_release_carries_the_key_clearing_event() {
        // The source's focus loss unpresses every held key
        // (`Minecraft.java:1469-1478`): the release must carry the session's
        // own key-clearing event.
        assert_eq!(CaptureStep::Release.input(), Some(InputEvent::FocusLost));
        assert_eq!(CaptureStep::Grab.input(), None, "a grab sends nothing");
        assert_eq!(CaptureStep::Consumed.input(), None);
        assert_eq!(CaptureStep::Exit.input(), None);
    }

    #[test]
    fn a_focus_loss_drops_capture_and_the_held_keys() {
        let mut capture = Capture::default();
        capture.click();
        assert!(capture.grabbed());
        assert_eq!(
            capture.focus_lost(),
            CaptureStep::Release,
            "a focus loss releases capture"
        );
        assert!(!capture.grabbed(), "capture is dropped");
        assert_eq!(
            Capture::default().focus_lost(),
            CaptureStep::Release,
            "the keys are cleared even when the pointer was free"
        );
        assert_eq!(
            capture.click(),
            CaptureStep::Grab,
            "nothing recaptures on its own; the next click does"
        );
    }

    #[test]
    fn gameplay_keys_translate_only_while_grabbed() {
        let w = PhysicalKey::Code(KeyCode::KeyW);
        assert_eq!(
            gameplay_key(true, ElementState::Pressed, w),
            Some(InputEvent::Key {
                key: Key::W,
                pressed: true
            })
        );
        assert_eq!(
            gameplay_key(true, ElementState::Released, w),
            Some(InputEvent::Key {
                key: Key::W,
                pressed: false
            })
        );
        assert_eq!(
            gameplay_key(false, ElementState::Pressed, w),
            None,
            "gameplay input is suppressed while the pointer is free"
        );
        assert_eq!(
            gameplay_key(
                true,
                ElementState::Pressed,
                PhysicalKey::Code(KeyCode::KeyQ)
            ),
            None,
            "an unbound key sends nothing even while grabbed"
        );
    }

    #[test]
    fn only_the_bound_mouse_buttons_translate() {
        assert_eq!(
            bound_mouse_button(WinitMouseButton::Left),
            Some(MouseButton::Left)
        );
        assert_eq!(
            bound_mouse_button(WinitMouseButton::Right),
            Some(MouseButton::Right)
        );
        assert_eq!(bound_mouse_button(WinitMouseButton::Middle), None);
    }

    #[test]
    fn a_script_parses_into_its_directives() {
        let script = "\
# a short walk, look and stop
10 key W down
11 key A down
12 key S down
13 key D down
14 key Space down
15 key ShiftLeft down
16 key ControlLeft down
17 mouse Left down
18 mouse Right up
20 look 30 -4
25 key W up
";
        let directives = parse_script(script).expect("the script parses");
        assert_eq!(
            directives,
            [
                Directive {
                    tick: 10,
                    event: InputEvent::Key {
                        key: Key::W,
                        pressed: true
                    }
                },
                Directive {
                    tick: 11,
                    event: InputEvent::Key {
                        key: Key::A,
                        pressed: true
                    }
                },
                Directive {
                    tick: 12,
                    event: InputEvent::Key {
                        key: Key::S,
                        pressed: true
                    }
                },
                Directive {
                    tick: 13,
                    event: InputEvent::Key {
                        key: Key::D,
                        pressed: true
                    }
                },
                Directive {
                    tick: 14,
                    event: InputEvent::Key {
                        key: Key::Space,
                        pressed: true
                    }
                },
                Directive {
                    tick: 15,
                    event: InputEvent::Key {
                        key: Key::ShiftLeft,
                        pressed: true
                    }
                },
                Directive {
                    tick: 16,
                    event: InputEvent::Key {
                        key: Key::ControlLeft,
                        pressed: true
                    }
                },
                Directive {
                    tick: 17,
                    event: InputEvent::MouseButton {
                        button: MouseButton::Left,
                        pressed: true
                    }
                },
                Directive {
                    tick: 18,
                    event: InputEvent::MouseButton {
                        button: MouseButton::Right,
                        pressed: false
                    }
                },
                Directive {
                    tick: 20,
                    event: InputEvent::MouseDelta { dx: 30.0, dy: -4.0 }
                },
                Directive {
                    tick: 25,
                    event: InputEvent::Key {
                        key: Key::W,
                        pressed: false
                    }
                },
            ]
        );
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let script = "# a leading comment\n\n   \n  # an indented comment\n7 key A down # a trailing comment\n8 key A up\n";
        let directives = parse_script(script).expect("comments do not refuse the script");
        assert_eq!(directives.len(), 2, "only the two directives count");
        assert_eq!(directives[0].tick, 7);
        assert_eq!(
            directives[0].event,
            InputEvent::Key {
                key: Key::A,
                pressed: true
            }
        );
        assert_eq!(directives[1].tick, 8);
        assert_eq!(
            directives[1].event,
            InputEvent::Key {
                key: Key::A,
                pressed: false
            }
        );
    }

    #[test]
    fn a_malformed_line_is_refused_with_its_line_number() {
        let cases = [
            ("10 key W sideways", 1, "neither down nor up"),
            ("10 key Q down", 1, "not a bound key"),
            ("10 jump", 1, "not one of key, mouse or look"),
            ("x key W down", 1, "not a whole number"),
            ("10 look 3", 1, "look takes a dx and a dy"),
            ("10 look 1 two", 1, "the look dy"),
            ("10 mouse Middle down", 1, "not a bound mouse button"),
            (
                "# a comment\n\n11 key W down extra",
                3,
                "key takes a key name and down or up",
            ),
        ];
        for (script, line, needle) in cases {
            let error = parse_script(script).expect_err("the line must be refused");
            let message = format!("{error:#}");
            assert!(
                message.contains(&format!("line {line}")),
                "{script:?} must name line {line}: {message}"
            );
            assert!(
                message.contains(needle),
                "{script:?} must say {needle:?}: {message}"
            );
        }
    }

    #[test]
    fn directives_are_applied_at_their_tick_in_file_order() {
        let dir = std::env::temp_dir().join(format!("oxide-client-script-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the scratch directory is created");
        let script_path = dir.join("walk.script");
        std::fs::write(
            &script_path,
            "# the replay's own shape\n4 key W down\n7 key D down\n7 mouse Left down\n12 look 30 -4\n",
        )
        .expect("the script is written");
        let (input_tx, input_rx) = unbounded();
        let mut driver = ScriptDriver::load(&script_path, input_tx).expect("the script loads");
        // The first tick the window sees may already be past a directive:
        // tick 6 is past the tick-4 press and short of the tick-7 pair.
        driver
            .observe(6, 1.0, 2.0, 3.0, 0.0, 0.0, true)
            .expect("the log line writes");
        assert_eq!(
            input_rx.try_recv().expect("the tick-4 press is due"),
            InputEvent::Key {
                key: Key::W,
                pressed: true
            }
        );
        assert!(
            input_rx.try_recv().is_err(),
            "the tick-7 pair is not due yet"
        );
        // Tick 7 applies the two directives at tick 7, in file order.
        driver
            .observe(7, 1.5, 2.0, 3.5, 10.0, -2.5, true)
            .expect("the log line writes");
        assert_eq!(
            input_rx.try_recv().expect("D down"),
            InputEvent::Key {
                key: Key::D,
                pressed: true
            }
        );
        assert_eq!(
            input_rx.try_recv().expect("Left down"),
            InputEvent::MouseButton {
                button: MouseButton::Left,
                pressed: true
            }
        );
        assert!(input_rx.try_recv().is_err(), "the look is not due yet");
        // Tick 9 applies nothing new; tick 12 applies the look.
        driver
            .observe(9, 2.0, 2.0, 4.0, 10.0, -2.5, true)
            .expect("the log line writes");
        assert!(input_rx.try_recv().is_err(), "nothing is due at tick 9");
        driver
            .observe(12, 2.5, 2.0, 4.5, 40.0, -8.0, false)
            .expect("the log line writes");
        assert_eq!(
            input_rx.try_recv().expect("the look"),
            InputEvent::MouseDelta { dx: 30.0, dy: -4.0 }
        );
        assert!(input_rx.try_recv().is_err());
        // The log carries one line per observed tick, in the documented shape.
        let log = std::fs::read_to_string(dir.join("walk.script.log")).expect("the log reads");
        assert_eq!(
            log.lines().collect::<Vec<&str>>(),
            [
                "6,1,2,3,0,0,true",
                "7,1.5,2,3.5,10,-2.5,true",
                "9,2,2,4,10,-2.5,true",
                "12,2.5,2,4.5,40,-8,false",
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_aim_is_stored_and_cleared() {
        // The session reports the aim when it changes; the window keeps the
        // latest report for the outline and crack passes to draw from, `None`
        // included, and a repeat of the same report is no move.
        let block = Aim {
            x: 1,
            y: 64,
            z: -3,
            face: Face::North,
            hit: [1.5, 64.5, -3.0],
        };
        let mut aim = None;
        assert!(
            store_aim(&mut aim, Some(block)),
            "the first report moves it"
        );
        assert_eq!(aim, Some(block), "the block is stored");
        assert!(
            !store_aim(&mut aim, Some(block)),
            "the same block is no move"
        );
        assert_eq!(aim, Some(block), "and it is still stored");
        assert!(store_aim(&mut aim, None), "the clearing report moves it");
        assert_eq!(aim, None, "the aim is cleared");
        assert!(!store_aim(&mut aim, None), "there is nothing left to clear");
    }

    #[test]
    fn the_aim_outline_wraps_the_aims_cell_in_the_full_cube() {
        // The outline draws the ray's block — the session's `Aim` — with the full cube's box,
        // the shape every full-block class's selection bounds report
        // (`RenderGlobal.java:1895`).
        let aim = Aim {
            x: -3,
            y: 64,
            z: 12,
            face: Face::Up,
            hit: [-2.5, 65.0, 12.5],
        };
        assert_eq!(
            aim_outline(aim),
            Outline {
                block: [-3, 64, 12],
                shape: FULL_CUBE,
            }
        );
    }

    #[test]
    fn the_crack_set_is_the_stage_maps_entries_within_the_render_distance() {
        // The window feeds one crack entry per live destroy stage, in cell order, and keeps
        // only what the render distance can show; the pass drops the rest itself at the
        // source's 32-block reach (`RenderGlobal.java:1843-1848`).
        let mut stages = BTreeMap::new();
        assert!(store_break_stage(&mut stages, 0, 64, 0, 3));
        assert!(store_break_stage(&mut stages, 7, 64, 0, 5));
        assert!(store_break_stage(&mut stages, 140, 64, 0, 9), "far out");
        let eye = [0.5, 64.5, 0.5];
        assert_eq!(
            cracks_in_view(&stages, eye, 8),
            vec![
                Crack {
                    block: [0, 64, 0],
                    stage: 3,
                },
                Crack {
                    block: [7, 64, 0],
                    stage: 5,
                },
            ]
        );
        // The boundary cell is inside: its centre sits 128 blocks out, the plane itself.
        assert!(store_break_stage(&mut stages, 128, 64, 0, 1));
        assert_eq!(
            cracks_in_view(&stages, eye, 8),
            vec![
                Crack {
                    block: [0, 64, 0],
                    stage: 3,
                },
                Crack {
                    block: [7, 64, 0],
                    stage: 5,
                },
                Crack {
                    block: [128, 64, 0],
                    stage: 1,
                },
            ]
        );
        // A wider render distance takes the far one back.
        assert_eq!(cracks_in_view(&stages, eye, 12).len(), 4);
    }

    #[test]
    fn the_break_stage_store_reports_its_moves() {
        // The session's stage reports land in a map keyed by the block's cell, and each
        // report answers whether the entry it wrote changed — the same stage twice is one
        // landing.
        let mut stages = BTreeMap::new();
        assert!(
            store_break_stage(&mut stages, 1, 2, 3, 4),
            "a first stage lands"
        );
        assert_eq!(stages.get(&[1, 2, 3]), Some(&4));
        assert!(
            !store_break_stage(&mut stages, 1, 2, 3, 4),
            "the same stage is no move"
        );
        assert!(
            store_break_stage(&mut stages, 1, 2, 3, 7),
            "a later stage moves"
        );
        assert_eq!(stages.get(&[1, 2, 3]), Some(&7));
        assert!(
            clear_break_stage(&mut stages, 1, 2, 3),
            "the clear removes it"
        );
        assert!(
            !clear_break_stage(&mut stages, 1, 2, 3),
            "nothing is left to clear"
        );
        assert!(stages.is_empty());
    }

    #[test]
    fn the_death_view_constants_are_the_sources() {
        // The dim is the first stop of `GuiGameOver`'s gradient
        // (`drawGradientRect(0, 0, width, height, 1615855616, -1602211792)`):
        // `0x60500000` is alpha 96 over red 80, and the client's floats are
        // the byte values over 255, its no-transfer-function convention.
        assert_eq!(DEATH_DIM[0], 80.0 / 255.0, "red 80");
        assert_eq!(DEATH_DIM[1], 0.0, "green 0");
        assert_eq!(DEATH_DIM[2], 0.0, "blue 0");
        assert_eq!(DEATH_DIM[3], 96.0 / 255.0, "alpha 96");
        // The two lines are the source's `deathScreen.title` and
        // `deathScreen.respawn` strings.
        assert_eq!(DEATH_TITLE, "You died!");
        assert_eq!(DEATH_RESPAWN, "Respawn");
    }

    /// One tick's camera state at a position, with everything else quiet.
    fn quiet_tick(position: [f64; 3]) -> CameraTick {
        CameraTick {
            position,
            on_ground: true,
            sprinting: false,
            sneaking: false,
            flying: false,
            in_water: false,
            hurt_time: 0,
            attacked_at_yaw: 0.0,
            snapped: false,
            dead: false,
        }
    }

    #[test]
    fn the_camera_state_steps_the_smoother_toward_the_ticks_flags() {
        // The smoother's target is the flag-borne modifier only (`EntityRenderer.java:527-530`
        // reads `AbstractClientPlayer.getFovModifier`): a walk tick holds the hand at the
        // identity, a sprinting tick blends it toward the sprint attribute's 1.15 — 1.075
        // after the first tick, 1.1125 after the second (`:532-534`).
        let mut camera = CameraState::default();
        camera.observe(quiet_tick([0.0, 64.0, 0.0]));
        assert_eq!(camera.smoother.hand, 1.0, "a walk tick holds the identity");
        let mut sprinting = quiet_tick([0.0, 64.0, 0.0]);
        sprinting.sprinting = true;
        camera.observe(sprinting);
        assert!(
            (camera.smoother.hand - 1.075).abs() < 1e-6,
            "got {}",
            camera.smoother.hand
        );
        camera.observe(sprinting);
        assert!((camera.smoother.hand - 1.1125).abs() < 1e-6);
        assert!((camera.smoother.prev - 1.075).abs() < 1e-6);
    }

    #[test]
    fn the_walk_accumulator_adds_the_sources_increment_and_skips_corrections() {
        // `Entity.moveEntity` (`Entity.java:872`) adds the tick's horizontal displacement
        // times 0.6, with the previous tick's copy sliding in first (`Entity.onEntityUpdate`,
        // `:420`); the increment is skipped while flying (`canTriggerWalking`,
        // `EntityPlayer.java:2205-2208`), on the ground while sneaking (`Entity.java:626`)
        // and on a server correction.
        let mut camera = CameraState::default();
        camera.observe(quiet_tick([0.0, 64.0, 0.0]));
        assert_eq!(
            camera.walk.distance, 0.0,
            "the first tick measures across nothing"
        );
        camera.observe(quiet_tick([3.0, 64.0, 4.0]));
        assert!(
            (camera.walk.distance - 3.0).abs() < 1e-6,
            "sqrt(3² + 4²) × 0.6, got {}",
            camera.walk.distance
        );
        assert_eq!(camera.walk.previous, 0.0, "the previous tick's copy");
        let mut snapped = quiet_tick([100.0, 64.0, 0.0]);
        snapped.snapped = true;
        camera.observe(snapped);
        assert!(
            (camera.walk.distance - 3.0).abs() < 1e-6,
            "a correction is not walking"
        );
        assert!((camera.walk.previous - 3.0).abs() < 1e-6);
        let mut sneaking = quiet_tick([100.0, 64.0, 0.5]);
        sneaking.sneaking = true;
        camera.observe(sneaking);
        assert!(
            (camera.walk.distance - 3.0).abs() < 1e-6,
            "the sneak glide adds nothing"
        );
        let mut flying = quiet_tick([100.0, 64.0, 1.0]);
        flying.flying = true;
        camera.observe(flying);
        assert!(
            (camera.walk.distance - 3.0).abs() < 1e-6,
            "flight adds nothing"
        );
        camera.observe(quiet_tick([100.0, 64.0, 2.0]));
        assert!(
            (camera.walk.distance - 3.6).abs() < 1e-6,
            "one more block × 0.6"
        );
    }

    #[test]
    fn the_camera_sensors_damp_toward_the_ticks_displacement() {
        // `EntityPlayer.onLivingUpdate` (`EntityPlayer.java:635-654`): the yaw sensor follows
        // the horizontal speed clamped to 0.1 and zeroed off the ground, the pitch sensor
        // follows `atan(−motionY × 0.2) × 15` and is zeroed on the ground. The tick's
        // displacement stands in for the motion vector. A ground tick five blocks on clamps
        // to 0.1: `cameraYaw += (0.1 − 0) × 0.4 = 0.04`. Airborne, the yaw target is zero
        // and a 0.42-block rise leans the pitch by `atan(−0.42 × 0.2) × 15 ≈ −1.257°`,
        // damped by 0.8 to −1.0056.
        let mut camera = CameraState::default();
        camera.observe(quiet_tick([0.0, 64.0, 0.0]));
        camera.observe(quiet_tick([3.0, 64.0, 4.0]));
        assert!(
            (camera.camera_yaw.value - 0.04).abs() < 1e-6,
            "got {}",
            camera.camera_yaw.value
        );
        assert_eq!(camera.camera_yaw.previous, 0.0);
        assert_eq!(
            camera.camera_pitch.value, 0.0,
            "on the ground the pitch target is zero"
        );
        let mut airborne = quiet_tick([3.0, 64.42, 4.0]);
        airborne.on_ground = false;
        camera.observe(airborne);
        assert!(
            (camera.camera_yaw.value - 0.024).abs() < 1e-6,
            "0.04 damped toward zero by 0.4, got {}",
            camera.camera_yaw.value
        );
        assert!(
            (camera.camera_pitch.value - -1.0056392).abs() < 1e-4,
            "got {}",
            camera.camera_pitch.value
        );
    }

    #[test]
    fn the_frame_fraction_is_the_elapsed_fraction_of_the_fifty_millisecond_tick() {
        let start = Instant::now();
        let mut camera = CameraState::default();
        assert_eq!(
            camera.partial(start),
            0.0,
            "before the first tick the fraction is zero"
        );
        camera.last_tick_arrival = Some(start);
        assert!(
            (camera.partial(start + Duration::from_millis(12)) - 0.24).abs() < 1e-4,
            "12 ms into the tick"
        );
        assert!((camera.partial(start + Duration::from_millis(25)) - 0.5).abs() < 1e-4);
        assert!((camera.partial(start + Duration::from_millis(50)) - 1.0).abs() < 1e-4);
        assert!(
            (camera.partial(start + Duration::from_millis(500)) - 1.0).abs() < 1e-4,
            "a late frame clamps to the tick's end"
        );
    }

    #[test]
    fn the_death_clock_counts_dead_ticks_and_clears_when_alive() {
        // `EntityLivingBase.onDeathUpdate`'s `++deathTime` (`EntityLivingBase.java:400`,
        // reached from `:349`) advances once per dead tick; the client clears it once a
        // living tick arrives (the source resets it on the respawn,
        // `EntityPlayer.preparePlayerToSpawn:578-583`).
        let mut camera = CameraState::default();
        let mut dead = quiet_tick([0.0, 64.0, 0.0]);
        dead.dead = true;
        camera.observe(dead);
        assert_eq!(camera.death_time, 1);
        camera.observe(dead);
        assert_eq!(camera.death_time, 2);
        camera.observe(quiet_tick([0.0, 64.0, 0.0]));
        assert_eq!(camera.death_time, 0, "a living tick clears the clock");
    }
}
