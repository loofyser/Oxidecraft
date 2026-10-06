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
//! The chat rides the same stream: T (or `/`) opens the field while the pointer is grabbed,
//! its characters arrive as text and its editing keys through [`keymap`], and Enter sends
//! one [`InputEvent::SendChat`]. While it is open the pointer frees, the look stills and the
//! wheel scrolls the log; Escape closes it and touches nothing else. The field and its
//! routing are [`ChatInput`]'s and [`ClientApp::on_key`]'s.
//!
//! With `--server` the client also loads the asset store's extraction tree before the window
//! opens ([`assets::ClientAssets`]) and hands the atlas, the font and the sky textures to the
//! renderer once it exists. Without a server nothing is loaded and the M0 smoke path stands;
//! the overlay then has no sheet and draws nothing. `--no-overlay` suppresses the overlay at
//! startup and `--render-distance` sets the far plane, the fog distance and the view distance
//! the client reports.

mod assets;
mod items;
mod keymap;
mod skin_worker;
mod view;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use assets::ClientAssets;
use clap::Parser;
use crossbeam_channel::{Receiver, Sender, unbounded};
use oxide_assets::font::Font;
use oxide_assets::skins::SkinCache;
use oxide_assets::store::Store;
use oxide_game::chat::{ClickAction, ClickEvent};
use oxide_game::entity_view::PlayerListRecord;
use oxide_game::hud::{HudState, debug_lines};
use oxide_game::input::{InputEvent, Key, MouseButton};
use oxide_game::interaction::Aim;
use oxide_game::player::MAX_HURT_TIME;
use oxide_game::scoreboard::Scoreboard;
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
use skin_worker::{SkinRequest, SkinUpdate};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{
    DeviceEvent, DeviceId, ElementState, KeyEvent, MouseButton as WinitMouseButton,
    MouseScrollDelta, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, DeviceEvents, EventLoop};
use winit::keyboard::{Key as WinitKey, KeyCode, NamedKey, PhysicalKey};
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

/// One input-script directive: one action, due at one session tick.
#[derive(Debug, Clone, PartialEq)]
struct Directive {
    /// The tick the directive is applied at, once the session's count reaches it.
    tick: u64,
    /// What the directive does then.
    action: DirectiveAction,
}

/// What one script directive does.
#[derive(Debug, Clone, PartialEq)]
enum DirectiveAction {
    /// One input event, injected into the session's channel exactly as the
    /// window's own events are.
    Input(InputEvent),
    /// The `chat` macro's message: the field opens, the text goes through
    /// the character path and the send leaves on the line's own tick — the
    /// same field machine the window's T and typing drive, not a shortcut.
    Chat(String),
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
    /// A `chat` directive drives the window's own field: it opens, the text
    /// goes through the character path and Enter's send leaves for the
    /// session on this tick; a line that arrives while the field is already
    /// open is refused and logged. A `look` directive's delta is not
    /// forwarded while the field is open — the mouse then moves the window's
    /// cursor, the source's screen rule, not the camera. The field's owner
    /// runs the window's own follow-ups (the pointer rules are the window's;
    /// a rig run has no pointer to free).
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
        chat: &mut ChatInput,
    ) -> std::io::Result<()> {
        writeln!(self.log, "{tick},{x},{y},{z},{yaw},{pitch},{on_ground}")?;
        while let Some(directive) = self.directives.get(self.applied) {
            if directive.tick > tick {
                break;
            }
            match &directive.action {
                DirectiveAction::Input(event) => {
                    if chat.open && matches!(event, InputEvent::MouseDelta { .. }) {
                        tracing::debug!(
                            "a look while the chat is open moves the window's cursor, not the camera"
                        );
                    } else if self.input_tx.send(event.clone()).is_err() {
                        tracing::warn!("the session's input channel is closed; the script stopped");
                    }
                }
                DirectiveAction::Chat(text) => {
                    if chat.open {
                        tracing::warn!(
                            text = %text,
                            "the chat is already open; the chat directive was refused"
                        );
                    } else {
                        // The same field machine the window's T, typing and
                        // Enter drive — the filter and the cap run as they
                        // would for a player — and the send leaves on this
                        // line's own tick.
                        chat.open("");
                        chat.type_text(text);
                        if let ChatKey::Send(send) = chat.key(Key::Enter) {
                            if self.input_tx.send(send).is_err() {
                                tracing::warn!(
                                    "the session's input channel is closed; the script stopped"
                                );
                            }
                        }
                    }
                }
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
/// A `chat` line is the one whose argument is free text: `<tick> chat <text>`
/// opens the chat field, types `text` through the character path and sends it
/// on the tick — the same field machine the window's T, typing and Enter
/// drive — and a `chat` line while the chat is already open is refused. While
/// the chat is open a `look` directive moves the window's cursor rather than
/// the camera, the same rule the window's own mouse follows, so its delta is
/// not forwarded while the field is open. Fields are whitespace-separated; a
/// `#` starts a comment that runs to the end of the line, and blank lines are
/// ignored. A line the grammar does not cover is refused, and the refusal
/// names its line number.
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
        let directive = parse_directive(code, &fields)
            .with_context(|| format!("input script line {}", index + 1))?;
        directives.push(directive);
    }
    Ok(directives)
}

/// Parses one line's fields: the tick, then one directive with its arguments.
///
/// `code` is the line's own text, comment stripped and trimmed: the `chat`
/// directive's message is the rest of it after the name, spaces and all.
fn parse_directive(code: &str, fields: &[&str]) -> anyhow::Result<Directive> {
    let Some((&tick, rest)) = fields.split_first() else {
        anyhow::bail!("the line has no tick");
    };
    let tick: u64 = tick
        .parse()
        .map_err(|_| anyhow::anyhow!("the tick {tick:?} is not a whole number"))?;
    let Some((&name, arguments)) = rest.split_first() else {
        anyhow::bail!("the tick is not followed by a directive");
    };
    let action = match name {
        "key" => DirectiveAction::Input(parse_key(arguments)?),
        "mouse" => DirectiveAction::Input(parse_mouse(arguments)?),
        "look" => DirectiveAction::Input(parse_look(arguments)?),
        "chat" => DirectiveAction::Chat(chat_text(code)?),
        other => anyhow::bail!("{other:?} is not one of key, mouse, look or chat"),
    };
    Ok(Directive { tick, action })
}

/// The message of a `chat` directive: everything after the name on the line,
/// leading whitespace stripped.
///
/// The message is kept as written — spaces inside it and all — because it is
/// the field that filters and trims it as the message goes out, exactly as a
/// typed one. A line with nothing after the name is refused here.
fn chat_text(code: &str) -> anyhow::Result<String> {
    let after_tick = code
        .split_once(char::is_whitespace)
        .map(|(_, rest)| rest.trim_start())
        .unwrap_or_default();
    let text = after_tick
        .strip_prefix("chat")
        .unwrap_or_default()
        .trim_start();
    anyhow::ensure!(!text.is_empty(), "the chat directive has no message");
    Ok(text.to_string())
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
    /// The skins the worker resolved, keyed by the hyphenated UUID — the map
    /// the player renderer draws from.
    skins: BTreeMap<String, SkinUpdate>,
    /// The entity feed's own state: the latest frames, their arrival and the
    /// draws a frame builds from them.
    view: view::View,
    /// The chat mirror: the session's chat messages and the frame's hud draws for them.
    chat: view::ChatView,
    /// The chat field: the text, the cursor and the open state the window's
    /// keys and the script's `chat` lines drive.
    chat_input: ChatInput,
    /// The held player list: the session's entries and header/footer pair, and
    /// the held-key state the frame's draws gate on.
    ///
    /// The key lands in [`ClientApp::on_key`], the session's events in
    /// [`ClientApp::draw`]'s drain, and the draws join the chat's in the same
    /// frame.
    tab: view::TabState,
    /// The frame's scoreboard mirror: every whole-board report replaces it, and
    /// the tab list's sort and display slots read it.
    board: Scoreboard,
    /// The window's own account name: what the sidebar reads its team through
    /// (`GuiIngame.java`:320). Taken from the command line's username before the
    /// session takes that value for its own handshake.
    own_name: String,
    /// The measured font the frame's text surfaces share: the same value the
    /// chat mirror measured against, and the one the tab list's assembly reads.
    /// `None` without a session.
    font: Option<Font>,
    /// The skin worker's request feed, when a session was opened.
    ///
    /// Dropping it — the client drops it with the app at exit — closes the
    /// channel and the worker's loop returns.
    skin_requests: Option<Sender<SkinRequest>>,
    /// The worker's updates, drained into [`ClientApp::skins`] once per frame.
    skin_updates: Option<Receiver<SkinUpdate>>,
    /// The pointer-capture rules.
    capture: Capture,
    /// The free pointer's position in the frame's GUI units while the chat is
    /// open: the hover and the box's hit-test read it, and nothing about it
    /// reaches the session (`GuiChat.drawScreen`:305-310 reads the free mouse;
    /// `GuiNewChat.getChatComponent`:256-257 scales it by the factor).
    cursor: Option<(f32, f32)>,
    /// The link opener the confirm overlay's Enter runs.
    opener: UrlOpener,
    /// The `--input-script` replay, when the flag was given.
    script: Option<ScriptDriver>,
}

/// The world-overlay state the session's events keep for the frame.
#[derive(Debug, Default)]
struct WorldOverlayState {
    /// The latest aim the session reported, or `None` when the interaction ray
    /// meets no block: the outline draws its cell, `None` stops it.
    aim: Option<Aim>,
    /// The live destroy stages the crack draws: one entry per breaking player,
    /// keyed by the breaker's entity id, holding the block and the sprite
    /// stage `0..=9`.
    ///
    /// The source keys its map the same way (`RenderGlobal.damagedBlocks`,
    /// `RenderGlobal.java`:127), one entry per breaker, so two breakers on one
    /// block are two entries and the crack pass draws both; the session
    /// reports each landing and clearing as `BreakStage` and `BreakCleared`,
    /// and the window keeps the map so a frame can hand the pass every stage
    /// within the render distance.
    break_stages: BTreeMap<i32, WindowBreakEntry>,
}

/// One live destroy stage the window holds for the crack pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowBreakEntry {
    /// The breaking block's cell.
    pos: [i32; 3],
    /// The stage the crack draws, 0..=9.
    stage: u8,
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
        let mut chat_font = None;
        let mut skin_requests_tx = None;
        let mut skin_updates_rx = None;
        // The sidebar reads the team through the window's own name; the session
        // takes the command line's username for its own handshake, so the window
        // keeps its copy first.
        let own_name = cli.username.clone();
        let session = match cli.server {
            Some(address) => {
                let (host, port) = parse_server_address(&address)?;
                tracing::info!(server = %address, username = %cli.username, "joining the server");
                let loaded = ClientAssets::load(None)?;
                // The chat mirror measures against the same sheet every other text
                // surface uses; it gets the font before the window opens.
                chat_font = Some(loaded.font.clone());
                // The skin worker opens the store the assets loaded from —
                // the same root rule — and owns the cache on its own thread.
                let store = Store::open(assets::default_store_root()?)?;
                let cache = Arc::new(SkinCache::new(&store));
                let (requests, updates) = skin_worker::spawn(move |url| cache.fetch(url));
                skin_requests_tx = Some(requests);
                skin_updates_rx = Some(updates);
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
            skins: BTreeMap::new(),
            view: view::View::new(),
            chat: match &chat_font {
                Some(font) => {
                    let mut chat = view::ChatView::new();
                    chat.set_font(font.clone());
                    chat
                }
                None => view::ChatView::new(),
            },
            chat_input: ChatInput::default(),
            tab: view::TabState::new(),
            board: Scoreboard::default(),
            own_name,
            font: chat_font,
            skin_requests: skin_requests_tx,
            skin_updates: skin_updates_rx,
            overlay_visible,
            dead: false,
            capture: Capture::default(),
            cursor: None,
            opener: Box::new(spawn_url_opener),
            script,
        })
    }

    /// Drains the skin worker's updates into [`ClientApp::skins`], and uploads each
    /// to the renderer's registry — a re-upload replaces, an absent cape clears.
    fn drain_skins(&mut self) {
        let Some(updates) = self.skin_updates.as_ref() else {
            return;
        };
        let drained: Vec<SkinUpdate> = updates.try_iter().collect();
        if let Some(renderer) = self.renderer.as_mut() {
            for update in &drained {
                renderer.set_skin(
                    &update.uuid,
                    update.texture.as_deref(),
                    update.cape.as_deref(),
                );
            }
        }
        store_skins(&mut self.skins, drained);
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
        // The worker's updates land before the frame reads the map.
        self.drain_skins();
        let (Some(window), Some(renderer)) = (self.window.as_ref(), self.renderer.as_mut()) else {
            return;
        };
        let mut session_ended = false;
        for event in events {
            self.view.apply(&event);
            // The tab list folds its own events in — the player-list entries
            // and the header and footer pair — and a scoreboard report
            // becomes the frame's mirror, the board the assembly reads.
            self.tab.observe(&event);
            if let ClientEvent::ScoreboardChanged { board } = &event {
                self.board = board.clone();
            }
            if let ClientEvent::PlayerList { entries } = &event {
                if let Some(requests) = self.skin_requests.as_ref() {
                    forward_skins(requests, entries);
                }
            }
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
                    if let Err(error) = script.observe(
                        *tick,
                        *x,
                        *y,
                        *z,
                        *yaw,
                        *pitch,
                        *on_ground,
                        &mut self.chat_input,
                    ) {
                        // The record of the run is broken; stop rather than
                        // pretend the measurement is whole.
                        tracing::error!(%error, "the tick log could not be written");
                        self.stopped_on_error = true;
                        event_loop.exit();
                        return;
                    }
                }
                // The chat field's blink steps on the session's tick — the
                // source runs its counter from `GuiChat.updateScreen`
                // (`:78-81`).
                if self.chat_input.open {
                    self.chat_input.tick();
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
                &mut self.chat,
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
            // The entity draws: the feed interpolated at this frame's fraction, in
            // the feed's own order, the window's own entity skipped.
            renderer.set_entities(
                self.view
                    .entity_draws(Instant::now(), &self.skins, &self.board),
            );
        }
        // The chat: the mirror ages to the session's tick, and the frame's draws —
        // bars, text and the record line at the scaled resolution — land in the hud
        // pass, which draws them between the dim and the debug overlay. The frame
        // re-couples the view's open state with the field's (the script drives the
        // field alone), feeds the hover from the free pointer, and hands the draws
        // the field the blink and the line read from.
        self.chat.update(self.player.tick);
        view::reconcile_chat_open(&mut self.chat, &self.chat_input);
        let scaled = renderer.scaled_resolution();
        self.chat.feed_hover(
            tooltip_point(self.chat_input.open, self.chat.confirm_open(), self.cursor),
            scaled,
        );
        // The hud list runs in the source's own overlay order: the scoreboard
        // sidebar first (`GuiIngame.java`:336), then the chat's draws
        // (`GuiIngame.java`:343-346), then the held player list
        // (`GuiIngame.java`:348-358). The sidebar and the list draw with the
        // measured font; the list draws only while its key is held.
        let mut hud_draws = match self.font.as_ref() {
            Some(font) => view::sidebar_draws(&self.board, &self.own_name, font, scaled),
            None => Vec::new(),
        };
        hud_draws.extend(self.chat.draws(scaled, &self.chat_input));
        if self.tab.open {
            if let Some(font) = self.font.as_ref() {
                hud_draws.extend(self.tab.tab_draws(
                    &self.board,
                    font,
                    scaled,
                    renderer.entity_textures(),
                ));
            }
        }
        renderer.set_hud(hud_draws);
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

    /// Routes one keyboard event: the window's shortcuts, the chat's own
    /// routing, then the gameplay keys.
    ///
    /// Escape first: while the chat is open it closes the chat and nothing
    /// else — the M3 capture rules apply only while the chat is closed.
    /// While the chat is open every other key is the field's; while it is
    /// closed the shortcuts work as before, T or `/` opens the field, and
    /// gameplay keys flow only while the pointer is grabbed.
    fn on_key(&mut self, event_loop: &ActiveEventLoop, event: KeyEvent) {
        if is_escape_press(event.state, event.repeat, &event.logical_key) {
            match escape_route(
                self.chat_input.open,
                self.chat.confirm_open(),
                &mut self.capture,
            ) {
                EscapeRoute::CancelConfirm => self.chat.cancel_confirm(),
                EscapeRoute::CloseChat => self.close_chat(event_loop),
                EscapeRoute::Capture(step) => self.apply_capture(event_loop, step),
            }
            return;
        }
        // The confirm overlay is the top screen while it stands in for the
        // chat: its own keys are the only ones it answers, and the field
        // beneath takes nothing.
        if self.chat.confirm_open() {
            self.on_confirm_key(event.state, event.physical_key);
            return;
        }
        if self.chat_input.open {
            self.on_chat_key(event_loop, event);
            return;
        }
        // The held player list: a Tab edge while no screen consumes the keys
        // — the open chat above answers first — sets the held state the
        // frame's list draws gate on (`Minecraft.java`:1904-1912 sets every
        // keyboard edge's binding; `GuiIngame.java`:350 reads the held key).
        if let Some(held) = tab_held(event.state, event.physical_key) {
            self.tab.open = held;
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
        if self.capture.grabbed() {
            if let Some(default) = chat_opener(event.state, event.repeat, event.physical_key) {
                self.open_chat(event_loop, default);
                return;
            }
        }
        if let Some(input) = gameplay_key(self.capture.grabbed(), event.state, event.physical_key) {
            self.send_input(input);
        }
    }

    /// Routes one key event to the open chat field.
    ///
    /// The field's own keys are consumed by [`ChatInput::key`]; every other
    /// key's character text (`KeyEvent.text`) goes through the field's
    /// character path, which filters and caps it, exactly as a typed
    /// character would. Enter's send leaves for the session and the field
    /// closes — and the close is what recaptures the pointer.
    fn on_chat_key(&mut self, event_loop: &ActiveEventLoop, event: KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        let consumed = match event.physical_key {
            PhysicalKey::Code(code) => match keymap::translate(code) {
                Some(key) => match self.chat_input.key(key) {
                    ChatKey::Send(send) => {
                        self.send_input(send);
                        true
                    }
                    ChatKey::Consumed => true,
                    ChatKey::Character => false,
                },
                None => false,
            },
            PhysicalKey::Unidentified(_) => false,
        };
        if !consumed {
            if let Some(text) = event.text.as_deref() {
                self.chat_input.type_text(text);
            }
        }
        if !self.chat_input.open {
            self.close_chat(event_loop);
        }
    }

    /// Opens the chat field and frees the pointer.
    ///
    /// This is the source's screen open: `displayGuiScreen` frees the cursor
    /// and the held keys (`Minecraft.java`:1010-1012 through
    /// `setIngameNotInFocus`:1470-1478), and the chat view goes open for the
    /// frame's draws.
    fn open_chat(&mut self, event_loop: &ActiveEventLoop, default: &str) {
        self.chat_input.open(default);
        self.chat.set_open(true);
        // The screen's open drops the held keys, the player list's included.
        self.tab.open = false;
        // The pointer was grabbed until this press: there is no free position
        // yet, so no hover or hit-test point until the mouse moves.
        self.cursor = None;
        let step = self.capture.chat_open();
        self.apply_capture(event_loop, step);
    }

    /// Closes the chat field and recaptures the pointer.
    ///
    /// The source's screen close: `displayGuiScreen(null)` recaptures
    /// (`Minecraft.java`:1019-1023 through `setIngameFocus`:1453-1465), the
    /// scroll resets (`GuiChat.onGuiClosed`:69-73), the chat view goes shut
    /// and the field drops its text.
    fn close_chat(&mut self, event_loop: &ActiveEventLoop) {
        self.chat_input.close();
        self.chat.set_open(false);
        self.chat.reset_scroll();
        // The cursor is captured again: the free position means nothing.
        self.cursor = None;
        let step = self.capture.chat_close();
        self.apply_capture(event_loop, step);
    }

    /// Routes one key event to the confirm overlay — the only keys it has.
    ///
    /// Enter opens the link: the overlay's URL goes through the opener exactly
    /// once, and the overlay ends, returning to the chat screen it replaced
    /// (`GuiScreen.confirmClicked`:713-719's true answer opens the link and
    /// re-displays the chat). Escape is the cancel and is routed by
    /// [`escape_route`] before this; every other key is the overlay's own to
    /// ignore.
    fn on_confirm_key(&mut self, state: ElementState, physical_key: PhysicalKey) {
        if !is_enter_press(state, physical_key) {
            return;
        }
        if let Some(url) = self.chat.take_confirm() {
            (self.opener)(&url);
        }
    }

    /// One press of the chat screen: the run under the free pointer acts —
    /// `GuiChat.mouseClicked`:172-186 (`getChatComponent` hit-tests the drawn
    /// lines, then `handleComponentClick` acts on the component's click
    /// event). A press on no run does nothing.
    fn chat_click(&mut self) {
        let Some(point) = self.cursor else {
            return;
        };
        let Some(renderer) = self.renderer.as_ref() else {
            return;
        };
        let scaled = renderer.scaled_resolution();
        let click = self
            .chat
            .run_at(point, scaled)
            .and_then(|run| run.click.clone());
        if let Some(click) = click {
            self.apply_chat_click(&click);
        }
    }

    /// Acts on one click event — `GuiScreen.handleComponentClick`:445-452 for the
    /// commands, `:403-433` for the link: a run command is sent through the
    /// session's chat path, a suggest command overwrites the field's text, and
    /// a link raises the confirm overlay that asks.
    fn apply_chat_click(&mut self, click: &ClickEvent) {
        match &click.action {
            ClickAction::RunCommand => {
                self.send_input(InputEvent::SendChat {
                    text: command_text(&click.value),
                });
            }
            ClickAction::SuggestCommand => {
                self.chat_input.set_text(&click.value);
            }
            ClickAction::OpenUrl => {
                self.chat.open_confirm(&click.value);
            }
        }
    }

    /// Routes one mouse button event: the click that grabs, or the gameplay
    /// button.
    ///
    /// The grabbing click is consumed — the source's own first press goes to
    /// `setIngameFocus` (`Minecraft.java:1887-1891`), not to the game — and
    /// with no session there is nothing to grab for. While the chat is open
    /// the screen holds the click: it can neither grab nor steer, and the
    /// screen's own hit-tests are a later round's.
    fn on_mouse_button(
        &mut self,
        event_loop: &ActiveEventLoop,
        state: ElementState,
        button: WinitMouseButton,
    ) {
        if self.chat_input.open {
            // The screen holds the click. A press with the confirm overlay up
            // belongs to the overlay, whose own keys are the keyboard's in this
            // milestone; otherwise the press hit-tests the box's runs —
            // `GuiChat.mouseClicked`:172-186 — and acts on the one under the
            // free pointer.
            if state == ElementState::Pressed && !self.chat.confirm_open() {
                self.chat_click();
            }
            return;
        }
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

/// The skin requests a player-list report produces: one per entry whose
/// properties carry a `textures` value, named by the entry's uuid.
///
/// An entry without the property produces no request — nothing is fetchable
/// for it — and the renderer falls back to the UUID default.
fn skin_requests(entries: &[PlayerListRecord]) -> Vec<SkinRequest> {
    entries
        .iter()
        .filter_map(|record| {
            let property = record
                .properties
                .iter()
                .find(|(name, _)| name == "textures")
                .map(|(_, value)| value.clone())?;
            Some(SkinRequest {
                uuid: record.uuid.clone(),
                property: Some(property),
            })
        })
        .collect()
}

/// Forwards a player-list report to the skin worker: one request per entry
/// that carries a `textures` property.
fn forward_skins(requests: &Sender<SkinRequest>, entries: &[PlayerListRecord]) {
    for request in skin_requests(entries) {
        // A closed channel means the worker is gone; the frame must not stop
        // over it.
        let _ = requests.send(request);
    }
}

/// Folds a batch of the worker's updates into the skin map, keyed by uuid:
/// a later update for one uuid replaces the earlier one.
fn store_skins(
    skins: &mut BTreeMap<String, SkinUpdate>,
    updates: impl IntoIterator<Item = SkinUpdate>,
) {
    for update in updates {
        skins.insert(update.uuid.clone(), update);
    }
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
/// `eye`, sorted by the block and then the breaker.
///
/// The overlay itself drops entries beyond the source's 32-block reach
/// (`RenderGlobal.drawBlockDamageTexture`'s squared-distance removal, `:1845`); the window's
/// own filter keeps a stage left behind by a walked-away block out of the pass at all. One
/// entry is one `Crack` and duplicates are kept: two breakers on one block hand the pass two
/// crack draws, which the multiply blend applies twice — the source's
/// `damagedBlocks.values()` iteration does exactly this (`RenderGlobal.java`:710, `:735`).
fn cracks_in_view(
    stages: &BTreeMap<i32, WindowBreakEntry>,
    eye: [f64; 3],
    render_distance: u8,
) -> Vec<Crack> {
    let limit = f64::from(render_distance) * 16.0;
    let mut entries: Vec<([i32; 3], i32, u8)> = stages
        .iter()
        .filter(|(_, entry)| {
            let dx = f64::from(entry.pos[0]) + 0.5 - eye[0];
            let dy = f64::from(entry.pos[1]) + 0.5 - eye[1];
            let dz = f64::from(entry.pos[2]) + 0.5 - eye[2];
            dx * dx + dy * dy + dz * dz <= limit * limit
        })
        .map(|(&breaker, entry)| (entry.pos, breaker, entry.stage))
        .collect();
    entries.sort_unstable();
    entries
        .into_iter()
        .map(|(block, _, stage)| Crack { block, stage })
        .collect()
}

/// Stores a destroy stage the session reported under its breaker, and answers whether the map
/// moved.
///
/// The reports land in the source's `damagedBlocks` map, one entry per breaker, keyed by the
/// breaking player's entity id (`RenderGlobal.java`:127); a same-breaker write at a new
/// position replaces the entry (`RenderGlobal.java`:2368-2372), and a repeat of the stage
/// already stored is no move, so the caller's log line fires once per change.
fn store_break_stage(
    stages: &mut BTreeMap<i32, WindowBreakEntry>,
    breaker: i32,
    x: i32,
    y: i32,
    z: i32,
    stage: u8,
) -> bool {
    match stages.get_mut(&breaker) {
        Some(entry) => {
            let moved = entry.pos != [x, y, z] || entry.stage != stage;
            entry.pos = [x, y, z];
            entry.stage = stage;
            moved
        }
        None => {
            stages.insert(
                breaker,
                WindowBreakEntry {
                    pos: [x, y, z],
                    stage,
                },
            );
            true
        }
    }
}

/// Removes a breaker's destroy stage, and answers whether the map moved.
///
/// The session reports the removal when a dig stops or completes; the source's remove branch
/// drops the entry by breaker (`RenderGlobal.sendBlockBreakProgress`:2379), so a removal
/// cannot touch another breaker's entry on the same block. The answer tells the caller whether
/// the log line is worth writing.
fn clear_break_stage(stages: &mut BTreeMap<i32, WindowBreakEntry>, breaker: i32) -> bool {
    stages.remove(&breaker).is_some()
}

/// Applies one event the world-overlay state holds, answering whether the
/// event was one of them.
///
/// The three events the overlay state carries — the aim, a destroy stage
/// landing, and a stage leaving — land here; the frame reads the state
/// exactly as the assertions do: `aim_outline` over the stored aim and
/// `cracks_in_view` over the breaker-keyed map, so a test can drive the
/// window's real wiring without a renderer.
fn apply_overlay_event(world_overlay: &mut WorldOverlayState, event: &ClientEvent) -> bool {
    match event {
        ClientEvent::Aim { aim: report } => {
            if store_aim(&mut world_overlay.aim, *report) {
                tracing::debug!(?report, "the aim moved");
            }
            true
        }
        ClientEvent::BreakStage {
            breaker,
            x,
            y,
            z,
            stage,
        } => {
            // The crack overlay's own input: the stage lands in the map a
            // frame hands the pass, and the log line fires once per change.
            if store_break_stage(
                &mut world_overlay.break_stages,
                *breaker,
                *x,
                *y,
                *z,
                *stage,
            ) {
                tracing::debug!(
                    breaker = *breaker,
                    x = *x,
                    y = *y,
                    z = *z,
                    stage = *stage,
                    "a destroy stage landed"
                );
            }
            true
        }
        ClientEvent::BreakCleared { breaker, x, y, z } => {
            if clear_break_stage(&mut world_overlay.break_stages, *breaker) {
                tracing::debug!(
                    breaker = *breaker,
                    x = *x,
                    y = *y,
                    z = *z,
                    "a destroy stage left"
                );
            }
            true
        }
        _ => false,
    }
}

/// Applies one event the session reported: meshes go to the renderer, the pose
/// to the view state and the overlay, the join parameters to the overlay state,
/// the clock and the sky to the frame's parameters, and the aim to the
/// window's own copy.
///
/// Returns whether the session ended, which stops the client. The session
/// returns `Ok(())` when the server closed the connection, so its end is a
/// normal exit, not an error.
#[allow(clippy::too_many_arguments)]
fn apply_session_event(
    renderer: &mut Renderer,
    hud: &mut HudState,
    sky: &mut SkyState,
    player: &mut PlayerState,
    chat: &mut view::ChatView,
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
        ClientEvent::EntitiesTick { .. } => {
            // The window's entity surfaces arrive with a later milestone; the
            // feed is accepted here so the session's event stream stays total.
            false
        }
        ClientEvent::PlayerList { .. } => {
            // The held player list takes its entries at the frame's drain;
            // the feed is accepted here so the session's event stream stays
            // total.
            false
        }
        ClientEvent::ScoreboardChanged { .. } => {
            // The frame's drain keeps each whole-board report as the held
            // player list's mirror; the window's sidebar surface itself
            // arrives with a later milestone, and the feed is accepted here
            // so the session's event stream stays total.
            false
        }
        ClientEvent::TabText { .. } => {
            // The held player list takes the header and footer pair at the
            // frame's drain, with the entries; the feed is accepted here so
            // the session's event stream stays total.
            false
        }
        ClientEvent::Chat { text, position } => {
            // The chat mirror: the message lands with the tick the player pose last
            // reported — the clock the log's fade and the record line's hold read.
            chat.observe(&text, position, player.tick);
            false
        }
        ClientEvent::Aim { .. }
        | ClientEvent::BreakStage { .. }
        | ClientEvent::BreakCleared { .. } => {
            // The overlay state's own events; the helper holds the stores the
            // frame's outline and crack reads consume.
            apply_overlay_event(world_overlay, &event);
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
///
/// The open chat is the second thing that frees the pointer — the screen
/// open itself, `Minecraft.displayGuiScreen`
/// (`Minecraft.java:1010-1023`) — and its close recaptures; the rules live in
/// [`Capture::chat_open`] and [`Capture::chat_close`].
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

    /// The chat's open: frees the pointer the way the source's screen open
    /// does — `displayGuiScreen` calls `setIngameNotInFocus`
    /// (`Minecraft.java`:1010-1012 through `:1470-1478`), which ungrabs the
    /// cursor — and drops what the session holds with the release. A pointer
    /// that is already free stays free, and the drop still runs.
    fn chat_open(&mut self) -> CaptureStep {
        if self.grabbed {
            self.grabbed = false;
            CaptureStep::Release
        } else {
            CaptureStep::Consumed
        }
    }

    /// The chat's close: recaptures the pointer, the source's
    /// `setIngameFocus` path (`Minecraft.java`:1019-1023 through
    /// `:1453-1465`). The pointer was freed when the screen opened, so the
    /// grab is asked for here — it is exactly a click's grab with no click.
    fn chat_close(&mut self) -> CaptureStep {
        self.grabbed = true;
        CaptureStep::Grab
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
/// bind or one that is not gameplay input at all ([`Key::is_gameplay`]: the
/// chat keys carry slots but drive no movement); a bound gameplay key's edge
/// travels as [`InputEvent::Key`].
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
    keymap::translate(code)
        .filter(|key| key.is_gameplay())
        .map(|key| InputEvent::Key {
            key,
            pressed: state == ElementState::Pressed,
        })
}

/// The chat's opener keys while it is closed: a fresh press of T opens an
/// empty field, `/` a slashed one — the source's two chat keys
/// (`Minecraft.java`:2113-2121 over the `keyBindChat` and `keyBindCommand`
/// bindings, `GameSettings.java`:139, `:141`). A repeat is not a fresh press
/// and opens nothing; a release is no open at all.
fn chat_opener(
    state: ElementState,
    repeat: bool,
    physical_key: PhysicalKey,
) -> Option<&'static str> {
    if state != ElementState::Pressed || repeat {
        return None;
    }
    let PhysicalKey::Code(code) = physical_key else {
        return None;
    };
    match keymap::translate(code) {
        Some(Key::T) => Some(""),
        Some(Key::Slash) => Some("/"),
        _ => None,
    }
}

/// The player-list key's own edge: a Tab press holds the list and a release
/// drops it, and no other key is this key at all.
///
/// The source's binding is a held key state — the keyboard loop sets every
/// edge (`Minecraft.java`:1904-1912) and the list's gate reads the held key
/// (`GuiIngame.java`:350) — and the state drops when a screen opens or the
/// focus goes, the source's own unpress of every held key
/// (`Minecraft.java`:1470-1478).
fn tab_held(state: ElementState, physical_key: PhysicalKey) -> Option<bool> {
    if physical_key == PhysicalKey::Code(KeyCode::Tab) {
        Some(state == ElementState::Pressed)
    } else {
        None
    }
}

/// The link opener the confirm overlay's Enter path runs — the source's
/// `openWebLink` (`GuiScreen.java`:727-739) in this milestone: the interim
/// hands the URL to `xdg-open`, the desktop's own opener, and leaves the
/// process to itself.
///
/// The seam is a box so the overlay's tests run with a recording opener and no
/// process at all.
type UrlOpener = Box<dyn Fn(&str)>;

/// The interim opener: `xdg-open` with the URL, spawned and left alone. A
/// spawn failure is logged, never fatal — the link is a side effect, not the
/// frame's own work.
fn spawn_url_opener(url: &str) {
    match Command::new("xdg-open").arg(url).spawn() {
        Ok(child) => tracing::info!(url, pid = child.id(), "the link opener was spawned"),
        Err(error) => tracing::warn!(url, %error, "the link opener could not be spawned"),
    }
}

/// Whether a key event is a fresh press of Enter — the confirm overlay's open
/// key.
///
/// The source's confirm screen answers with its own buttons; the interim binds
/// the true answer's open — `confirmClicked`'s `openWebLink` branch
/// (`GuiScreen.java`:713-719) — to Enter, and Escape cancels on the M3 route.
fn is_enter_press(state: ElementState, physical_key: PhysicalKey) -> bool {
    state == ElementState::Pressed && physical_key == PhysicalKey::Code(KeyCode::Enter)
}

/// The command a `run_command` click sends: the value with a leading slash
/// added when it carries none — the plan's own rule (`/the-value`).
///
/// `GuiScreen.handleComponentClick`:449-452 sends the value through
/// `sendChatMessage(value, false)`, the same session path Enter takes
/// (`GuiScreen.java`:481-493), and the server reads a leading slash as the
/// command path (`NetHandlerPlayServer`:808-811 strips exactly one); a value
/// that already carries one is sent as it stands.
fn command_text(value: &str) -> String {
    if value.starts_with('/') {
        value.to_owned()
    } else {
        format!("/{value}")
    }
}

/// A window position in the frame's GUI units: the raw position divided by the
/// GUI scale factor, floored the way the source's own integer division lands
/// (`GuiNewChat.getChatComponent`:256-257 — `(int)(mouseX / f)` and the same
/// for y). A zero factor is no division at all.
fn scaled_cursor(position: PhysicalPosition<f64>, scale: u32) -> (f32, f32) {
    if scale == 0 {
        return (position.x as f32, position.y as f32);
    }
    let factor = f64::from(scale);
    (
        (position.x / factor).floor() as f32,
        (position.y / factor).floor() as f32,
    )
}

/// The point each frame feeds the chat's hover: the free pointer's scaled
/// position while the chat screen is the open one — not while the confirm
/// overlay stands in for it — and none otherwise
/// (`GuiChat.drawScreen`:305-310 resolves the hover only as the open screen).
fn tooltip_point(
    chat_open: bool,
    confirm_open: bool,
    cursor: Option<(f32, f32)>,
) -> Option<(f32, f32)> {
    if chat_open && !confirm_open {
        cursor
    } else {
        None
    }
}

/// Where one Escape press goes: the confirm overlay cancels first — it is the
/// screen on top, and its cancel re-displays the chat
/// (`GuiScreen.confirmClicked`:713-725) — then an open chat closes and nothing
/// else; a closed chat leaves the M3 capture rules exactly as they were.
fn escape_route(chat_open: bool, confirm_open: bool, capture: &mut Capture) -> EscapeRoute {
    if confirm_open {
        EscapeRoute::CancelConfirm
    } else if chat_open {
        EscapeRoute::CloseChat
    } else {
        EscapeRoute::Capture(capture.escape())
    }
}

/// The three destinations of an Escape press. The chat's own close is what
/// makes an Escape while the chat is open close only — capture neither
/// releases nor exits — and the overlay's cancel sits above both: cancel is
/// the topmost screen's, the M3 capture rules only the free pointer's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EscapeRoute {
    /// The confirm overlay is cancelled; the chat below it stays open.
    CancelConfirm,
    /// The open chat closes and nothing else moves.
    CloseChat,
    /// The capture rules run, exactly M3's.
    Capture(CaptureStep),
}

/// The lines one wheel event scrolls the open chat: the event's delta clamps
/// to one notch of sign — a pixel delta's magnitude is not a line count —
/// and a notch is the source's seven lines (`GuiChat.handleMouseInput`:143-167
/// computes `(int) dWheel`, clamps to ±1 and multiplies by seven at `:160-163`;
/// its shift-flavoured second count needs a modifier state the window event
/// does not carry, so the seven is the one rule).
fn chat_wheel_lines(delta: MouseScrollDelta) -> i32 {
    let notches = match delta {
        MouseScrollDelta::LineDelta(_, y) => y,
        MouseScrollDelta::PixelDelta(position) => position.y as f32,
    };
    let notch = if notches > 0.0 {
        1
    } else if notches < 0.0 {
        -1
    } else {
        0
    };
    notch * 7
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
            // The entity set: the shadow sprite and the two default skins, under the
            // keys the pass's draws and the resolver name.
            for (key, texture) in &assets.entity_textures {
                renderer.set_entity_texture(key, texture);
            }
            renderer.set_default_skins(&assets.skin_wide, &assets.skin_slim);
            // The object set: the object sheets and the blocks atlas under the keys the
            // object draws name, and the item mesh source the draws' shapes, block models,
            // frame wood and icon quads resolve through.
            for (key, texture) in &assets.object_textures {
                renderer.set_entity_texture(key, texture);
            }
            renderer.set_entity_texture(assets::BLOCKS_ATLAS_TEXTURE, &assets.blocks_atlas);
            // The hud's icon sheet: the tab list's latency bars and heart
            // glyphs sample it under the name their draws carry.
            renderer.set_hud_texture(assets::HUD_ICONS, &assets.hud_icons);
            renderer.set_item_source(Arc::new(assets.item_meshes.clone()));
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
            WindowEvent::CursorMoved { position, .. } => {
                // The free pointer's position, in the frame's GUI units, while
                // the chat is open: the hover and the box's clicks read it,
                // and nothing about it reaches the session
                // (`GuiChat.drawScreen`:305-310 reads the live mouse;
                // `GuiNewChat.getChatComponent`:256-257 divides it by the
                // scale factor).
                if self.chat_input.open {
                    let scale = self
                        .renderer
                        .as_ref()
                        .map_or(1, |renderer| renderer.scaled_resolution().scale_factor);
                    self.cursor = Some(scaled_cursor(position, scale));
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // The wheel is the chat log's while the chat is open
                // (`GuiChat.handleMouseInput`:143-167); a closed chat has no
                // wheel surface yet — the hotbar's is a later task's.
                if self.chat_input.open {
                    let lines = chat_wheel_lines(delta);
                    if lines != 0 {
                        self.chat.scroll(lines);
                    }
                }
            }
            WindowEvent::Focused(false) => {
                tracing::info!("the window lost focus, dropping capture");
                let step = self.capture.focus_lost();
                self.apply_capture(event_loop, step);
                // The held list drops with the focus: the source's lost-focus
                // pause opens the game menu on the same half-second
                // (`EntityRenderer.java`:1071-1079), and a screen's open
                // unpresses every held key (`Minecraft.java`:1010-1012,
                // `Minecraft.java`:1470-1478).
                self.tab.open = false;
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
        // grabbed: a free cursor's motion never turns the player. While the
        // chat is open the motion belongs to the window's own cursor — the
        // screen freed it — so the session is not told either.
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
            if self.capture.grabbed() && !self.chat_input.open {
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

/// The chat field's character cap: `GuiChat.initGui`'s own
/// `setMaxStringLength(100)` (`GuiChat.java`:59).
///
/// The same hundred the session's guard reads (`oxide_game::session`'s
/// `CHAT_FIELD_CAP`): the field enforces, the session's drain only logs a
/// message that slipped past.
const CHAT_TEXT_CAP: usize = 100;

/// Whether the chat field accepts a character:
/// `ChatAllowedCharacters.isAllowedCharacter`:10-13 refuses the format
/// code `§` (167), everything below the space, and DEL (127), and
/// `GuiTextField.writeText`:132 filters every append through it.
fn chat_allowed(c: char) -> bool {
    c != '\u{a7}' && c >= ' ' && c != '\u{7f}'
}

/// The chat field's state: the text, the cursor, whether it is open, and the
/// blink counter — over the recall of the messages it sent.
///
/// The field is the source's `GuiChat` over its `GuiTextField`
/// (`GuiChat.java`:47-66 initialises both): the text is edited in place — a
/// character appends at the cursor, Backspace removes the one before it, the
/// arrows walk it (`GuiTextField.textboxKeyTyped`:337-494) — under the
/// 100-character cap the source sets (`GuiChat.initGui`:59). Enter sends the trimmed
/// text as one [`InputEvent::SendChat`] and closes (`GuiChat.keyTyped`:104-137);
/// Escape closes without sending (`:100-103`); the up and down arrows recall
/// the messages already sent (`getSentHistory`:272-296 over `GuiNewChat`'s
/// list, `GuiNewChat.java`:190-206), buffering the draft while they walk.
/// While the field is closed it carries no text.
#[derive(Debug, Default)]
struct ChatInput {
    /// The text, edited in place.
    text: String,
    /// The cursor's byte index into `text`, always on a character boundary
    /// (the source's cursor is a character index — `GuiTextField.setCursorPosition`:310-316
    /// clamps it to the text's length — and the byte index is the port's
    /// form of the same position).
    cursor: usize,
    /// Whether the field is open. The closed field carries no text.
    open: bool,
    /// The blink counter: steps once per session tick while open, and the
    /// cursor draws while `blink / 6 % 2 == 0` (`GuiTextField.java`:540
    /// divides its counter by six the same way).
    blink: u64,
    /// The messages sent so far, oldest first: `GuiNewChat`'s own list
    /// (`GuiNewChat.java`:190-206), which the recall walks.
    sent: Vec<String>,
    /// The recall's position, counted from the list's end —
    /// `GuiChat.initGui` sets `sentHistoryCursor = getSentMessages().size()`
    /// (`:57`) — so the end is the draft.
    recall: usize,
    /// The draft the recall started from, buffered when it leaves the end and
    /// restored when it returns (`GuiChat.historyBuffer`, `:21-27`).
    history_buffer: String,
}

/// What one field key did: the character path's next step, or the field's
/// own.
#[derive(Debug, Clone, PartialEq)]
enum ChatKey {
    /// The key is the character path's: append the event's text through
    /// [`ChatInput::type_text`].
    Character,
    /// The key was the field's own and is done; nothing further to do.
    Consumed,
    /// Enter sent the field's text; the event goes to the session.
    Send(InputEvent),
}

impl ChatInput {
    /// Opens the field on a default text — `""` from T, `"/"` from the
    /// command key (`Minecraft.java`:2113-2121) — the way
    /// `GuiTextField.setFocused`:698-705 takes focus: the cursor goes to
    /// the text's end and the blink count starts over.
    fn open(&mut self, default: &str) {
        self.open = true;
        self.recall = self.sent.len();
        self.history_buffer.clear();
        self.set_text(default);
        self.blink = 0;
    }

    /// Closes the field and drops its text — the screen's own close; what
    /// was typed and not sent goes with it (the source leaves it on the
    /// field, and the next open clears the field, `GuiChat.java`:47-66).
    fn close(&mut self) {
        self.open = false;
        self.text.clear();
        self.cursor = 0;
    }

    /// Replaces the text with the cursor at its end (`GuiTextField.setText`:86-101).
    fn set_text(&mut self, text: &str) {
        self.text.clear();
        self.text.push_str(text);
        self.cursor = self.text.len();
    }

    /// One session tick of the blink counter (`GuiTextField.updateCursorCounter`:78-81).
    fn tick(&mut self) {
        self.blink = self.blink.wrapping_add(1);
    }

    /// Whether the cursor is in its lit phase (`GuiTextField.java`:540).
    ///
    /// The frame's draw consumes this ([`view::ChatView::input_draws`]): the
    /// caret draws only while the phase is lit.
    fn cursor_visible(&self) -> bool {
        (self.blink / 6) % 2 == 0
    }

    /// Appends text at the cursor — the character path `GuiTextField.writeText`
    /// (`:129-169`) runs for any key that is not the field's own: every
    /// character the filter refuses is dropped, and the append stops at the
    /// field's cap.
    fn type_text(&mut self, text: &str) {
        let kept: Vec<char> = text.chars().filter(|&c| chat_allowed(c)).collect();
        let room = CHAT_TEXT_CAP.saturating_sub(self.text.chars().count());
        let insert: String = kept.into_iter().take(room).collect();
        if insert.is_empty() {
            return;
        }
        self.text.insert_str(self.cursor, &insert);
        self.cursor += insert.len();
    }

    /// Backspace: removes the character before the cursor, if any
    /// (`GuiTextField.textboxKeyTyped`:378-391's delete branch).
    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = self.text[..self.cursor]
            .chars()
            .next_back()
            .expect("a non-empty prefix has a last character");
        self.text.drain(self.cursor - prev.len_utf8()..self.cursor);
        self.cursor -= prev.len_utf8();
    }

    /// The left arrow: the cursor moves one character back (`:405-426`).
    fn left(&mut self) {
        if let Some(prev) = self.text[..self.cursor].chars().next_back() {
            self.cursor -= prev.len_utf8();
        }
    }

    /// The right arrow: the cursor moves one character on (`:428-449`).
    fn right(&mut self) {
        if let Some(next) = self.text[self.cursor..].chars().next() {
            self.cursor += next.len_utf8();
        }
    }

    /// The recall (`GuiChat.getSentHistory`:272-296): `msg_pos` walks the
    /// list — up is `-1`, down `+1`. Leaving the end buffers the draft and
    /// shows the newest message; returning to the end restores the draft; a
    /// step with nowhere to go holds.
    fn recall(&mut self, msg_pos: i32) {
        let next = (self.recall as i32 + msg_pos).clamp(0, self.sent.len() as i32) as usize;
        if next == self.recall {
            return;
        }
        if next == self.sent.len() {
            self.recall = next;
            let draft = self.history_buffer.clone();
            self.set_text(&draft);
        } else {
            if self.recall == self.sent.len() {
                self.history_buffer = self.text.clone();
            }
            self.recall = next;
            let message = self.sent[next].clone();
            self.set_text(&message);
        }
    }

    /// Enter: sends the trimmed text when anything remains and closes either
    /// way (`GuiChat.keyTyped`:104-137 — both branches reach
    /// `displayGuiScreen(null)`); the sent message joins the recall list as
    /// `sendChatMessage` adds it (`GuiScreen.java`:481-493 over
    /// `GuiNewChat.addToSentMessages`, `:200-206`).
    fn enter(&mut self) -> Option<InputEvent> {
        let message = self.text.trim().to_string();
        self.close();
        if message.is_empty() {
            return None;
        }
        if self.sent.last() != Some(&message) {
            self.sent.push(message.clone());
        }
        Some(InputEvent::SendChat { text: message })
    }

    /// One editing key, as `GuiChat.keyTyped`:87-138 reads it: the
    /// field's own keys are consumed here — the source's Tab completion is
    /// deferred, so Tab is consumed only — every other key is the character
    /// path's, and Enter's send leaves as the event.
    fn key(&mut self, key: Key) -> ChatKey {
        match key {
            Key::Backspace => {
                self.backspace();
                ChatKey::Consumed
            }
            Key::ArrowLeft => {
                self.left();
                ChatKey::Consumed
            }
            Key::ArrowRight => {
                self.right();
                ChatKey::Consumed
            }
            Key::ArrowUp => {
                self.recall(-1);
                ChatKey::Consumed
            }
            Key::ArrowDown => {
                self.recall(1);
                ChatKey::Consumed
            }
            Key::Tab => ChatKey::Consumed,
            Key::Enter => match self.enter() {
                Some(event) => ChatKey::Send(event),
                None => ChatKey::Consumed,
            },
            _ => ChatKey::Character,
        }
    }
}

#[cfg(test)]
mod tests {
    //! Key-routing and command-line tests.

    use super::{
        Aim, CameraState, CameraTick, Capture, CaptureStep, ChatInput, ChatKey, Cli, ClientApp,
        DEATH_DIM, DEATH_RESPAWN, DEATH_TITLE, Directive, DirectiveAction, EscapeRoute, Key,
        MouseButton, PlayerState, ScriptDriver, SessionLink, SkinRequest, SkinUpdate, SkyValues,
        UrlOpener, WindowBreakEntry, WorldOverlayState, aim_outline, apply_overlay_event,
        bound_mouse_button, chat_opener, chat_wheel_lines, clear_break_stage, command_text,
        cracks_in_view, escape_route, frame_params, gameplay_key, is_enter_press, is_escape_press,
        is_f3_press, parse_script, parse_server_address, scaled_cursor, skin_requests, store_aim,
        store_break_stage, store_skins, tab_held, tooltip_point, void_y_factor,
    };
    use clap::Parser;
    use crossbeam_channel::unbounded;
    use oxide_assets::skins::DefaultModel;
    use oxide_assets::texture::Texture;
    use oxide_game::chat::{ClickAction, ClickEvent};
    use oxide_game::entity_view::PlayerListRecord;
    use oxide_game::input::InputEvent;
    use oxide_game::interaction::Face;
    use oxide_game::session::ClientEvent;
    use oxide_render::world_overlay::{Crack, FULL_CUBE, Outline};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use winit::dpi::PhysicalPosition;
    use winit::event::{ElementState, MouseButton as WinitMouseButton, MouseScrollDelta};
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
    fn a_player_list_report_becomes_one_request_per_textured_entry() {
        let textured = PlayerListRecord {
            uuid: "00000000-0000-0000-0000-000000000001".to_owned(),
            name: "OxideDev".to_owned(),
            properties: vec![("textures".to_owned(), "eyJ4".to_owned())],
            ..PlayerListRecord::default()
        };
        let plain = PlayerListRecord {
            uuid: "00000000-0000-0000-0000-000000000002".to_owned(),
            name: "Someone".to_owned(),
            ..PlayerListRecord::default()
        };
        assert_eq!(
            skin_requests(&[textured, plain]),
            vec![SkinRequest {
                uuid: "00000000-0000-0000-0000-000000000001".to_owned(),
                property: Some("eyJ4".to_owned()),
            }],
            "only the entry carrying a textures property is fetchable"
        );
    }

    #[test]
    fn skin_updates_land_in_the_map_by_uuid() {
        let update = |uuid: &str, fill: u8| SkinUpdate {
            uuid: uuid.to_owned(),
            texture: Some(Arc::new(Texture {
                width: 64,
                height: 64,
                rgba: vec![fill; 64 * 64 * 4],
            })),
            cape: None,
            model: DefaultModel::Wide,
        };
        let mut skins = BTreeMap::new();
        store_skins(
            &mut skins,
            [
                update("00000000-0000-0000-0000-000000000001", 0x11),
                update("00000000-0000-0000-0000-000000000002", 0x22),
                update("00000000-0000-0000-0000-000000000001", 0x33),
            ],
        );
        assert_eq!(skins.len(), 2, "one entry per uuid");
        assert_eq!(
            skins["00000000-0000-0000-0000-000000000001"]
                .texture
                .as_ref()
                .map(|texture| texture.rgba[0]),
            Some(0x33),
            "a later update replaces the earlier one"
        );
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
    fn escape_while_the_chat_is_open_closes_it_and_nothing_else() {
        // The carve-out: the chat is a screen, so Escape closes it — it
        // neither releases capture (the pointer already freed when the
        // screen opened, `Minecraft.setIngameNotInFocus`:1470-1478, reached
        // from `displayGuiScreen`:1010-1012) nor asks to exit; the close is
        // what recaptures (`setIngameFocus`:1453-1465, `displayGuiScreen`:1019-1023).
        let mut capture = Capture::default();
        assert_eq!(
            capture.chat_open(),
            CaptureStep::Consumed,
            "a free pointer stays free"
        );
        assert_eq!(capture.click(), CaptureStep::Grab);
        assert_eq!(
            capture.chat_open(),
            CaptureStep::Release,
            "the open frees the pointer"
        );
        assert!(!capture.grabbed());
        assert_eq!(
            escape_route(true, false, &mut capture),
            EscapeRoute::CloseChat
        );
        assert!(!capture.grabbed(), "the escape itself touches nothing");
        assert_eq!(
            capture.chat_close(),
            CaptureStep::Grab,
            "the close recaptures"
        );
        assert!(capture.grabbed());
    }

    #[test]
    fn escape_while_the_chat_is_closed_keeps_the_m3_rules() {
        // The same physical press, with no chat open, is exactly the M3
        // rule above: while grabbed it releases, free it exits.
        let mut capture = Capture::default();
        capture.click();
        assert_eq!(
            escape_route(false, false, &mut capture),
            EscapeRoute::Capture(CaptureStep::Release),
            "escape while grabbed releases"
        );
        assert_eq!(
            escape_route(false, false, &mut capture),
            EscapeRoute::Capture(CaptureStep::Exit),
            "the second escape exits"
        );
    }

    #[test]
    fn escape_while_the_confirm_overlay_is_up_cancels_it_first() {
        // The overlay is the topmost screen: Escape cancels it and returns to
        // the chat — neither answer closes the chat
        // (`GuiScreen.confirmClicked`:713-725 re-displays `this`, the chat
        // screen, for both).
        let mut capture = Capture::default();
        capture.click();
        assert_eq!(
            escape_route(true, true, &mut capture),
            EscapeRoute::CancelConfirm,
            "the overlay's cancel comes before the chat's close"
        );
        assert!(capture.grabbed(), "the escape itself touches nothing");
        assert_eq!(
            escape_route(true, false, &mut capture),
            EscapeRoute::CloseChat,
            "with the overlay down the same press closes the chat"
        );
        assert_eq!(
            escape_route(false, false, &mut Capture::default()),
            EscapeRoute::Capture(CaptureStep::Exit),
            "a closed chat keeps the M3 rules: the untouched capture's own exit"
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
    fn the_tab_key_holds_and_releases_the_player_list() {
        // The player list's binding is a held key state: the source sets
        // every keyboard edge's binding (`Minecraft.java`:1904-1912) and the
        // list's gate reads it (`GuiIngame.java`:350). Only Tab is this key.
        let tab = PhysicalKey::Code(KeyCode::Tab);
        assert_eq!(
            tab_held(ElementState::Pressed, tab),
            Some(true),
            "a press holds the list"
        );
        assert_eq!(
            tab_held(ElementState::Released, tab),
            Some(false),
            "a release drops it"
        );
        assert_eq!(
            tab_held(ElementState::Pressed, PhysicalKey::Code(KeyCode::KeyT)),
            None,
            "no other key is the list's"
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
                    action: DirectiveAction::Input(InputEvent::Key {
                        key: Key::W,
                        pressed: true
                    })
                },
                Directive {
                    tick: 11,
                    action: DirectiveAction::Input(InputEvent::Key {
                        key: Key::A,
                        pressed: true
                    })
                },
                Directive {
                    tick: 12,
                    action: DirectiveAction::Input(InputEvent::Key {
                        key: Key::S,
                        pressed: true
                    })
                },
                Directive {
                    tick: 13,
                    action: DirectiveAction::Input(InputEvent::Key {
                        key: Key::D,
                        pressed: true
                    })
                },
                Directive {
                    tick: 14,
                    action: DirectiveAction::Input(InputEvent::Key {
                        key: Key::Space,
                        pressed: true
                    })
                },
                Directive {
                    tick: 15,
                    action: DirectiveAction::Input(InputEvent::Key {
                        key: Key::ShiftLeft,
                        pressed: true
                    })
                },
                Directive {
                    tick: 16,
                    action: DirectiveAction::Input(InputEvent::Key {
                        key: Key::ControlLeft,
                        pressed: true
                    })
                },
                Directive {
                    tick: 17,
                    action: DirectiveAction::Input(InputEvent::MouseButton {
                        button: MouseButton::Left,
                        pressed: true
                    })
                },
                Directive {
                    tick: 18,
                    action: DirectiveAction::Input(InputEvent::MouseButton {
                        button: MouseButton::Right,
                        pressed: false
                    })
                },
                Directive {
                    tick: 20,
                    action: DirectiveAction::Input(InputEvent::MouseDelta { dx: 30.0, dy: -4.0 })
                },
                Directive {
                    tick: 25,
                    action: DirectiveAction::Input(InputEvent::Key {
                        key: Key::W,
                        pressed: false
                    })
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
            directives[0].action,
            DirectiveAction::Input(InputEvent::Key {
                key: Key::A,
                pressed: true
            })
        );
        assert_eq!(directives[1].tick, 8);
        assert_eq!(
            directives[1].action,
            DirectiveAction::Input(InputEvent::Key {
                key: Key::A,
                pressed: false
            })
        );
    }

    #[test]
    fn a_malformed_line_is_refused_with_its_line_number() {
        let cases = [
            ("10 key W sideways", 1, "neither down nor up"),
            ("10 key Q down", 1, "not a bound key"),
            ("10 jump", 1, "not one of key, mouse, look or chat"),
            ("10 chat", 1, "no message"),
            ("10 chat   ", 1, "no message"),
            ("x key W down", 1, "not a whole number"),
            ("x chat hi", 1, "not a whole number"),
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
        let mut chat = ChatInput::default();
        // The first tick the window sees may already be past a directive:
        // tick 6 is past the tick-4 press and short of the tick-7 pair.
        driver
            .observe(6, 1.0, 2.0, 3.0, 0.0, 0.0, true, &mut chat)
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
            .observe(7, 1.5, 2.0, 3.5, 10.0, -2.5, true, &mut chat)
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
            .observe(9, 2.0, 2.0, 4.0, 10.0, -2.5, true, &mut chat)
            .expect("the log line writes");
        assert!(input_rx.try_recv().is_err(), "nothing is due at tick 9");
        driver
            .observe(12, 2.5, 2.0, 4.5, 40.0, -8.0, false, &mut chat)
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
        // The window feeds one crack entry per live destroy stage — one per
        // breaker — sorted by the block and then the breaker, and keeps only
        // what the render distance can show; the pass drops the rest itself
        // at the source's 32-block reach (`RenderGlobal.java`:1845). Two
        // breakers on one block hand the pass two entries, which the multiply
        // blend applies twice.
        let mut stages = BTreeMap::new();
        assert!(store_break_stage(&mut stages, 7, 0, 64, 0, 3));
        assert!(
            store_break_stage(&mut stages, 9, 0, 64, 0, 5),
            "the same block"
        );
        assert!(store_break_stage(&mut stages, 8, 7, 64, 0, 5));
        assert!(store_break_stage(&mut stages, 5, 140, 64, 0, 9), "far out");
        let eye = [0.5, 64.5, 0.5];
        assert_eq!(
            cracks_in_view(&stages, eye, 8),
            vec![
                Crack {
                    block: [0, 64, 0],
                    stage: 3,
                },
                Crack {
                    block: [0, 64, 0],
                    stage: 5,
                },
                Crack {
                    block: [7, 64, 0],
                    stage: 5,
                },
            ],
            "both breakers draw, in block order"
        );
        // The boundary cell is inside: its centre sits 128 blocks out, the plane itself.
        assert!(store_break_stage(&mut stages, 6, 128, 64, 0, 1));
        assert_eq!(
            cracks_in_view(&stages, eye, 8),
            vec![
                Crack {
                    block: [0, 64, 0],
                    stage: 3,
                },
                Crack {
                    block: [0, 64, 0],
                    stage: 5,
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
        assert_eq!(cracks_in_view(&stages, eye, 12).len(), 5);
    }

    #[test]
    fn the_break_stage_store_reports_its_moves() {
        // The session's stage reports land in a map keyed by the breaker, and
        // each report answers whether the entry it wrote changed — the same
        // stage twice is one landing, and a same-breaker write at a new block
        // replaces the entry.
        let mut stages = BTreeMap::new();
        assert!(
            store_break_stage(&mut stages, 4, 1, 2, 3, 4),
            "a first stage lands"
        );
        assert_eq!(
            stages.get(&4),
            Some(&WindowBreakEntry {
                pos: [1, 2, 3],
                stage: 4
            })
        );
        assert!(
            !store_break_stage(&mut stages, 4, 1, 2, 3, 4),
            "the same stage is no move"
        );
        assert!(
            store_break_stage(&mut stages, 4, 1, 2, 3, 7),
            "a later stage moves"
        );
        assert_eq!(
            stages.get(&4),
            Some(&WindowBreakEntry {
                pos: [1, 2, 3],
                stage: 7
            })
        );
        assert!(
            store_break_stage(&mut stages, 4, 9, 2, 3, 7),
            "a new block replaces the entry"
        );
        assert_eq!(
            stages.get(&4),
            Some(&WindowBreakEntry {
                pos: [9, 2, 3],
                stage: 7
            })
        );
        assert!(clear_break_stage(&mut stages, 4), "the clear removes it");
        assert!(
            !clear_break_stage(&mut stages, 4),
            "nothing is left to clear"
        );
        assert!(stages.is_empty());
    }

    #[test]
    fn overlay_wiring() {
        // The window's overlay state is driven through the real helper
        // `apply_session_event` delegates to, and read exactly as the frame
        // reads it: `aim_outline` over the stored aim and `cracks_in_view`
        // over the breaker-keyed map. Two breakers on one block are two
        // entries — the pass multiplies twice — and clearing one leaves the
        // other.
        let mut overlay = WorldOverlayState::default();
        let aim = Aim {
            x: -3,
            y: 64,
            z: 12,
            face: Face::Up,
            hit: [-2.5, 65.0, 12.5],
        };
        assert!(
            apply_overlay_event(&mut overlay, &ClientEvent::Aim { aim: Some(aim) }),
            "the aim is the overlay's own event"
        );
        assert_eq!(
            aim_outline(overlay.aim.expect("the aim landed")),
            Outline {
                block: [-3, 64, 12],
                shape: FULL_CUBE,
            }
        );
        // A look that leaves every block clears it.
        assert!(apply_overlay_event(
            &mut overlay,
            &ClientEvent::Aim { aim: None }
        ));
        assert!(overlay.aim.is_none(), "the aim cleared");
        // Two breakers on one block: both entries land, and both reach the
        // pass.
        assert!(apply_overlay_event(
            &mut overlay,
            &ClientEvent::BreakStage {
                breaker: 7,
                x: 0,
                y: 64,
                z: 0,
                stage: 3,
            }
        ));
        assert!(apply_overlay_event(
            &mut overlay,
            &ClientEvent::BreakStage {
                breaker: 9,
                x: 0,
                y: 64,
                z: 0,
                stage: 5,
            }
        ));
        let eye = [0.5, 64.5, 0.5];
        assert_eq!(
            cracks_in_view(&overlay.break_stages, eye, 8),
            vec![
                Crack {
                    block: [0, 64, 0],
                    stage: 3,
                },
                Crack {
                    block: [0, 64, 0],
                    stage: 5,
                },
            ],
            "both breakers draw"
        );
        // Clearing one breaker leaves the other.
        assert!(apply_overlay_event(
            &mut overlay,
            &ClientEvent::BreakCleared {
                breaker: 7,
                x: 0,
                y: 64,
                z: 0,
            }
        ));
        assert_eq!(
            cracks_in_view(&overlay.break_stages, eye, 8),
            vec![Crack {
                block: [0, 64, 0],
                stage: 5,
            }],
            "breaker 9 stands"
        );
        // A session event the overlay does not hold reports not-consumed.
        assert!(!apply_overlay_event(
            &mut overlay,
            &ClientEvent::Disconnected {
                reason: "the session thread ended".into(),
            }
        ));
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

    /// A fresh field, opened the way T opens it.
    fn open_field(default: &str) -> ChatInput {
        let mut chat = ChatInput::default();
        chat.open(default);
        chat
    }

    #[test]
    fn the_field_appends_at_the_cursor_and_the_arrows_walk_it() {
        // `GuiTextField.writeText` (`:129-169`): the characters are inserted
        // at the cursor, and the arrows move it one character at a time
        // (`textboxKeyTyped` cases 203/205, `:405-449`).
        let mut chat = open_field("");
        chat.type_text("ac");
        assert_eq!((chat.text.as_str(), chat.cursor), ("ac", 2));
        assert_eq!(chat.key(Key::ArrowLeft), ChatKey::Consumed);
        chat.type_text("b");
        assert_eq!(
            (chat.text.as_str(), chat.cursor),
            ("abc", 2),
            "the character lands at the cursor"
        );
        chat.key(Key::ArrowRight);
        assert_eq!(
            (chat.text.as_str(), chat.cursor),
            ("abc", 3),
            "the right arrow steps to the end"
        );
        chat.key(Key::ArrowLeft);
        assert_eq!(
            (chat.text.as_str(), chat.cursor),
            ("abc", 2),
            "the left arrow steps back"
        );
        chat.key(Key::Backspace);
        assert_eq!(
            (chat.text.as_str(), chat.cursor),
            ("ac", 1),
            "backspace removes the character before the cursor"
        );
    }

    #[test]
    fn the_cursor_walks_characters_and_never_lands_inside_one() {
        // The source's cursor is a character index; this port keeps a byte
        // index pinned to character boundaries, so one step across a
        // multi-byte character moves by its whole UTF-8 length.
        let mut chat = open_field("");
        chat.type_text("éa");
        assert_eq!((chat.text.as_str(), chat.cursor), ("éa", 3));
        chat.key(Key::ArrowLeft);
        assert_eq!(chat.cursor, 2, "one character left, not one byte");
        chat.key(Key::ArrowLeft);
        assert_eq!(chat.cursor, 0);
        chat.key(Key::ArrowRight);
        assert_eq!(chat.cursor, 2);
        chat.key(Key::Backspace);
        assert_eq!(
            (chat.text.as_str(), chat.cursor),
            ("a", 0),
            "backspace removes the two-byte character whole"
        );
    }

    #[test]
    fn the_cap_is_the_sources_hundred_characters() {
        // `GuiChat.initGui` sets the field's cap to 100 (`GuiChat.java`:59
        // over `GuiTextField.setMaxStringLength`:639-647), and
        // `writeText` cuts an insertion to the room left under it
        // (`GuiTextField.java`:135-152).
        let mut chat = open_field("");
        chat.type_text(&"a".repeat(100));
        assert_eq!(chat.text.chars().count(), 100);
        chat.type_text("bc");
        assert_eq!(
            chat.text,
            "a".repeat(100),
            "a full field refuses every further character"
        );
        assert_eq!(chat.cursor, 100, "and the cursor stays put");
        let mut chat = open_field("");
        chat.type_text(&"a".repeat(99));
        chat.type_text("bc");
        assert_eq!(
            chat.text,
            format!("{}b", "a".repeat(99)),
            "an insertion fits only the room left under the cap"
        );
        assert_eq!(chat.cursor, 100);
    }

    #[test]
    fn t_opens_the_field_empty_and_the_command_key_opens_it_slashed() {
        // `Minecraft.java`:2113-2121: T opens a `GuiChat` with no text, `/`
        // one prefilled with the command slash; the cursor sits after it
        // (`GuiTextField.setText`, `:86-101`).
        let chat = open_field("");
        assert!(chat.open);
        assert_eq!((chat.text.as_str(), chat.cursor), ("", 0));
        let chat = open_field("/");
        assert_eq!((chat.text.as_str(), chat.cursor), ("/", 1));
        // A closed field carries no text: the next open starts fresh.
        let mut chat = open_field("");
        chat.type_text("draft");
        chat.close();
        assert!(!chat.open);
        assert_eq!(chat.text, "");
    }

    #[test]
    fn enter_sends_the_field_text_and_closes() {
        // `GuiChat.keyTyped`:127-137: Enter takes the field's text, trims it
        // (`:129`), sends it when anything remains and closes the screen.
        let mut chat = open_field("");
        chat.type_text("say hi");
        assert_eq!(
            chat.key(Key::Enter),
            ChatKey::Send(InputEvent::SendChat {
                text: "say hi".into()
            }),
            "the send carries exactly the field's text"
        );
        assert!(!chat.open, "Enter closes the field");
        // The source trims the message before it goes (`GuiChat.java`:129).
        let mut chat = open_field("");
        chat.type_text("  spaced  ");
        assert_eq!(
            chat.key(Key::Enter),
            ChatKey::Send(InputEvent::SendChat {
                text: "spaced".into()
            })
        );
        // A message that trims to nothing sends nothing and still closes
        // (`:131-136`).
        let mut chat = open_field("");
        chat.type_text("   ");
        assert_eq!(chat.key(Key::Enter), ChatKey::Consumed);
        assert!(!chat.open);
    }

    #[test]
    fn the_arrow_history_recalls_the_sent_messages_newest_first() {
        // `GuiChat.getSentHistory` (`:272-296`) over the sent list
        // (`GuiNewChat.java`:190-206): the recall cursor starts
        // at the list's end (`GuiChat.initGui`:57), Up walks toward the
        // oldest, Down back to the newest, and a step past the end restores
        // the draft the recall started from.
        let mut chat = ChatInput::default();
        for message in ["first", "second", "third"] {
            chat.open("");
            chat.type_text(message);
            chat.key(Key::Enter);
        }
        chat.open("");
        chat.key(Key::ArrowUp);
        assert_eq!(
            (chat.text.as_str(), chat.cursor),
            ("third", 5),
            "the first Up is the newest message, cursor at its end"
        );
        chat.key(Key::ArrowUp);
        assert_eq!(chat.text, "second");
        chat.key(Key::ArrowUp);
        assert_eq!(chat.text, "first");
        chat.key(Key::ArrowUp);
        assert_eq!(chat.text, "first", "the oldest is the recall's end");
        chat.key(Key::ArrowDown);
        assert_eq!(chat.text, "second");
        chat.key(Key::ArrowDown);
        assert_eq!(chat.text, "third");
        chat.key(Key::ArrowDown);
        assert_eq!(chat.text, "", "the draft comes back");
    }

    #[test]
    fn the_recall_restores_the_draft_and_consecutive_sends_are_folded() {
        // `addToSentMessages` skips a message equal to the previous one
        // (`GuiNewChat.java`:200-206), and the draft typed before the recall
        // is buffered and restored (`GuiChat.java`:283-289).
        let mut chat = ChatInput::default();
        for message in ["dup", "dup", "next"] {
            chat.open("");
            chat.type_text(message);
            chat.key(Key::Enter);
        }
        chat.open("");
        chat.type_text("draft");
        chat.key(Key::ArrowUp);
        assert_eq!(chat.text, "next");
        chat.key(Key::ArrowUp);
        assert_eq!(chat.text, "dup");
        chat.key(Key::ArrowDown);
        assert_eq!(chat.text, "next");
        chat.key(Key::ArrowDown);
        assert_eq!(chat.text, "draft", "the draft the recall started from");
        chat.key(Key::ArrowDown);
        assert_eq!(chat.text, "draft", "and the end of the list holds it");
    }

    #[test]
    fn tab_while_open_is_swallowed_and_the_letter_keys_fall_through() {
        // Completion is deferred (Known limits): Tab is consumed and does
        // nothing (`GuiChat.keyTyped`:91-94 calls the autocomplete this
        // milestone does not build). A letter key is not the field's to
        // consume: its character text is what lands (`:122-125`).
        let mut chat = open_field("");
        chat.type_text("ab");
        assert_eq!(chat.key(Key::Tab), ChatKey::Consumed);
        assert_eq!(
            (chat.text.as_str(), chat.cursor, chat.open),
            ("ab", 2, true)
        );
        assert_eq!(chat.key(Key::W), ChatKey::Character);
        assert_eq!(chat.key(Key::T), ChatKey::Character);
        assert_eq!(chat.key(Key::Space), ChatKey::Character);
    }

    #[test]
    fn the_character_path_filters_the_control_characters() {
        // `ChatAllowedCharacters.isAllowedCharacter`:10-13: the format
        // code § (167), everything below the space and DEL (127) are refused;
        // `writeText` filters through it (`GuiTextField.java`:132).
        let mut chat = open_field("");
        chat.type_text("a\u{1}b\tc\u{7f}d\u{a7}e");
        assert_eq!(chat.text, "abcde");
        assert_eq!(chat.cursor, 5);
    }

    #[test]
    fn the_cursor_blinks_on_the_sources_six_tick_phases() {
        // `GuiTextField.java:540`: the cursor draws while
        // `cursorCounter / 6 % 2 == 0`; the counter steps once per tick from
        // the chat screen's `updateScreen` (`GuiChat.java`:78-81) and starts
        // over when the field takes focus (`GuiTextField.setFocused`:698-705).
        let mut chat = open_field("");
        assert!(chat.cursor_visible(), "the field opens with the cursor up");
        for _ in 0..5 {
            chat.tick();
        }
        assert!(chat.cursor_visible(), "five ticks in it is still up");
        chat.tick();
        assert!(!chat.cursor_visible(), "the sixth tick puts it down");
        for _ in 0..5 {
            chat.tick();
        }
        assert!(!chat.cursor_visible(), "five more keep it down");
        chat.tick();
        assert!(chat.cursor_visible(), "the twelfth tick brings it back");
        // Taking focus again restarts the count.
        for _ in 0..6 {
            chat.tick();
        }
        assert!(!chat.cursor_visible());
        chat.open("/");
        assert!(chat.cursor_visible(), "the reopen restarts the count");
    }

    #[test]
    fn the_chat_openers_are_the_sources_own_keys() {
        // T and `/`: `Minecraft.runTick` opens `new GuiChat()` / `new
        // GuiChat("/")` on the `keyBindChat` / `keyBindCommand` presses
        // (`Minecraft.runTick`:2113-2121), and the source's bindings are T (code 20) and
        // slash (code 53) (`GameSettings.java`:139, `:141`). An opener is a
        // fresh press of the physical key: a release or a repeat is no open.
        let t = PhysicalKey::Code(KeyCode::KeyT);
        let slash = PhysicalKey::Code(KeyCode::Slash);
        assert_eq!(chat_opener(ElementState::Pressed, false, t), Some(""));
        assert_eq!(chat_opener(ElementState::Pressed, false, slash), Some("/"));
        assert_eq!(chat_opener(ElementState::Released, false, t), None);
        assert_eq!(
            chat_opener(ElementState::Pressed, true, t),
            None,
            "an auto-repeat is not a fresh press"
        );
        assert_eq!(
            chat_opener(
                ElementState::Pressed,
                false,
                PhysicalKey::Code(KeyCode::KeyW)
            ),
            None
        );
    }

    #[test]
    fn the_chat_keys_stay_out_of_the_gameplay_path() {
        // The keys the chat rides carry held slots in the intent so its
        // index space stays total, but they drive no movement and never
        // travel to the session as gameplay input.
        for code in [
            KeyCode::KeyT,
            KeyCode::Slash,
            KeyCode::Tab,
            KeyCode::Enter,
            KeyCode::NumpadEnter,
            KeyCode::Backspace,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
        ] {
            assert_eq!(
                gameplay_key(true, ElementState::Pressed, PhysicalKey::Code(code)),
                None,
                "{code:?} is not gameplay input"
            );
        }
        assert_eq!(
            gameplay_key(
                true,
                ElementState::Pressed,
                PhysicalKey::Code(KeyCode::KeyW)
            ),
            Some(InputEvent::Key {
                key: Key::W,
                pressed: true
            })
        );
    }

    #[test]
    fn the_wheel_clamps_every_event_to_one_notch_of_seven_lines() {
        // `GuiChat.handleMouseInput`:143-167: the event's delta clamps to
        // ±1 notch and a notch is seven lines unless shift is down
        // (`GuiChat.handleMouseInput`:160-163); the log's own clamp (`GuiNewChat.scroll`:222-237)
        // takes it from there.
        assert_eq!(chat_wheel_lines(MouseScrollDelta::LineDelta(0.0, 1.0)), 7);
        assert_eq!(
            chat_wheel_lines(MouseScrollDelta::LineDelta(0.0, 5.0)),
            7,
            "a five-notch event still clamps to one notch"
        );
        assert_eq!(chat_wheel_lines(MouseScrollDelta::LineDelta(0.0, -1.0)), -7);
        assert_eq!(chat_wheel_lines(MouseScrollDelta::LineDelta(0.0, 0.0)), 0);
        assert_eq!(
            chat_wheel_lines(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
                0.0, -12.0
            ))),
            -7
        );
    }

    #[test]
    fn a_chat_line_carries_its_text_verbatim() {
        // The macro's line is `<tick> chat <text>`: the text is the rest of
        // the line after the name, inner spaces and all, as the field would
        // receive it; a `#` still starts a comment, so a message cannot
        // contain one.
        let script = "5 chat say hello   world\n6 chat /gamemode 1 OxideDev\n";
        let directives = parse_script(script).expect("the script parses");
        assert_eq!(
            directives,
            [
                Directive {
                    tick: 5,
                    action: DirectiveAction::Chat("say hello   world".into())
                },
                Directive {
                    tick: 6,
                    action: DirectiveAction::Chat("/gamemode 1 OxideDev".into())
                },
            ]
        );
    }

    #[test]
    fn a_chat_line_opens_types_and_sends_through_the_field() {
        // The text path, not a shortcut: the macro drives the same field the
        // window's T does — the characters filter and cap exactly as typed
        // ones do — and the send leaves on the line's own tick.
        let dir = std::env::temp_dir().join(format!("oxide-client-chat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the scratch directory is created");
        let script_path = dir.join("chat.script");
        std::fs::write(&script_path, format!("4 chat \u{1}{}\n", "x".repeat(101)))
            .expect("the script is written");
        let (input_tx, input_rx) = unbounded();
        let mut driver = ScriptDriver::load(&script_path, input_tx).expect("the script loads");
        let mut chat = ChatInput::default();
        driver
            .observe(4, 0.0, 64.0, 0.0, 0.0, 0.0, true, &mut chat)
            .expect("the log line writes");
        assert_eq!(
            input_rx.try_recv().expect("the send is due"),
            InputEvent::SendChat {
                text: "x".repeat(100),
            },
            "the control character was filtered and the cap cut at 100"
        );
        assert!(input_rx.try_recv().is_err(), "one send, exactly");
        assert!(!chat.open, "the send closed the field");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_chat_line_while_the_chat_is_already_open_is_refused() {
        // The refusal: the macro cannot steal an open field — nothing is
        // typed and nothing is sent, and the field is left as it was.
        let dir = std::env::temp_dir().join(format!("oxide-client-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the scratch directory is created");
        let script_path = dir.join("chat.script");
        std::fs::write(&script_path, "4 chat say hi\n").expect("the script is written");
        let (input_tx, input_rx) = unbounded();
        let mut driver = ScriptDriver::load(&script_path, input_tx).expect("the script loads");
        let mut chat = ChatInput::default();
        chat.open("");
        chat.type_text("mine");
        driver
            .observe(4, 0.0, 64.0, 0.0, 0.0, 0.0, true, &mut chat)
            .expect("the log line writes");
        assert!(input_rx.try_recv().is_err(), "nothing left the field");
        assert_eq!(
            (chat.text.as_str(), chat.open),
            ("mine", true),
            "the open field is untouched"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_look_line_while_the_chat_is_open_does_not_reach_the_session() {
        // The source's screen rule: while the chat is open the mouse moves
        // the window's cursor, not the camera, so a scripted look must not
        // travel as a session delta while the field is open — and must while
        // it is closed.
        let dir = std::env::temp_dir().join(format!("oxide-client-look-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the scratch directory is created");
        let script_path = dir.join("look.script");
        std::fs::write(&script_path, "4 look 30 -4\n8 look 10 0\n").expect("the script is written");
        let (input_tx, input_rx) = unbounded();
        let mut driver = ScriptDriver::load(&script_path, input_tx).expect("the script loads");
        let mut chat = ChatInput::default();
        driver
            .observe(4, 0.0, 64.0, 0.0, 0.0, 0.0, true, &mut chat)
            .expect("the log line writes");
        assert_eq!(
            input_rx
                .try_recv()
                .expect("the closed chat lets the look through"),
            InputEvent::MouseDelta { dx: 30.0, dy: -4.0 }
        );
        chat.open("");
        driver
            .observe(8, 0.0, 64.0, 0.0, 0.0, 0.0, true, &mut chat)
            .expect("the log line writes");
        assert!(
            input_rx.try_recv().is_err(),
            "the open chat took the look: it moves the window's cursor"
        );
        assert!(chat.open, "the refused look left the field open");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- the chat screen's clicks, the link confirm and the free pointer ----

    /// A link opener that records the URLs it is handed: the source's
    /// `openWebLink` (`GuiScreen.java`:727-739) replaced in every test that runs
    /// the overlay's Enter.
    fn recording_opener() -> (UrlOpener, Rc<RefCell<Vec<String>>>) {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let recorder = Rc::clone(&seen);
        let opener: UrlOpener = Box::new(move |url: &str| {
            recorder.borrow_mut().push(url.to_owned());
        });
        (opener, seen)
    }

    /// An app without a server or a window, the way the smoke tests build one.
    fn test_app() -> ClientApp {
        let cli = Cli::try_parse_from(["oxide-client"]).expect("a bare invocation parses");
        ClientApp::new(cli).expect("the client builds without a server")
    }

    #[test]
    fn the_confirm_keys_open_the_link_once_and_escape_cancels() {
        // The overlay's two keys: Enter takes the URL through the opener exactly
        // once — the second press finds the overlay gone
        // (`GuiScreen.confirmClicked`:713-725 opens and re-displays the chat) —
        // and the Escape route cancels with no key opening anything after.
        let mut app = test_app();
        let (opener, seen) = recording_opener();
        app.opener = opener;
        let enter = PhysicalKey::Code(KeyCode::Enter);
        assert!(is_enter_press(ElementState::Pressed, enter));
        assert!(!is_enter_press(
            ElementState::Pressed,
            PhysicalKey::Code(KeyCode::KeyX)
        ));
        assert!(
            !is_enter_press(ElementState::Released, enter),
            "a release is no press"
        );

        app.chat.open_confirm("https://a.example");
        app.on_confirm_key(ElementState::Pressed, enter);
        assert_eq!(seen.borrow().as_slice(), ["https://a.example".to_owned()]);
        assert!(!app.chat.confirm_open(), "the open took the overlay");
        app.on_confirm_key(ElementState::Pressed, enter);
        assert_eq!(seen.borrow().len(), 1, "nothing opens twice");

        // Escape: the route cancels first, and the Enter after it finds no
        // overlay — nothing opens in the escape's wake.
        app.chat.open_confirm("https://b.example");
        let mut capture = Capture::default();
        assert_eq!(
            escape_route(true, true, &mut capture),
            EscapeRoute::CancelConfirm
        );
        assert!(!capture.grabbed(), "the escape itself touches nothing");
        app.chat.cancel_confirm();
        assert!(!app.chat.confirm_open());
        app.on_confirm_key(ElementState::Pressed, enter);
        assert_eq!(seen.borrow().len(), 1, "zero opened by the escape path");
    }

    #[test]
    fn a_run_command_click_sends_the_command_with_one_slash() {
        // `GuiScreen.handleComponentClick`:449-452 sends the click's value
        // through `sendChatMessage(value, false)` — the same session path Enter
        // takes, not the sent-history one — and the server reads a leading slash
        // as the command path (`NetHandlerPlayServer`:808-811 strips exactly
        // one); the port adds the slash the plan names (`/the-value`) only when
        // the value carries none.
        assert_eq!(command_text("say hi"), "/say hi");
        assert_eq!(
            command_text("/say hi"),
            "/say hi",
            "a slashed value is not doubled"
        );
        let mut app = test_app();
        let (_events_tx, events_rx) = unbounded();
        let (input_tx, input_rx) = unbounded();
        app.session = Some(SessionLink {
            server: "test".into(),
            events: events_rx,
            input_tx,
        });
        app.apply_chat_click(&ClickEvent {
            action: ClickAction::RunCommand,
            value: "say hi".into(),
        });
        assert_eq!(
            input_rx
                .try_recv()
                .expect("the command left on the send path"),
            InputEvent::SendChat {
                text: "/say hi".into()
            }
        );
        assert!(input_rx.try_recv().is_err(), "one send, exactly");
    }

    #[test]
    fn a_suggest_click_replaces_the_field_text_and_lands_the_cursor_at_its_end() {
        // `GuiScreen.handleComponentClick`:445-448's `setText(value, true)` over
        // `GuiChat.setText`:191-198: the field is overwritten and the cursor
        // lands after the new text; the screen stays open.
        let mut app = test_app();
        app.chat_input.open("/");
        app.chat_input.type_text("draft");
        app.apply_chat_click(&ClickEvent {
            action: ClickAction::SuggestCommand,
            value: "say hi".into(),
        });
        assert_eq!(
            (app.chat_input.text.as_str(), app.chat_input.cursor),
            ("say hi", 6)
        );
        assert!(app.chat_input.open, "the screen stays open");
    }

    #[test]
    fn an_open_url_click_raises_the_confirm_overlay() {
        // `GuiScreen.java`:403-433: with the prompt on, the link is not opened
        // at the click — the click stores it as `clickedLinkURI` and raises the
        // screen that asks; the Enter path takes it from there.
        let mut app = test_app();
        app.apply_chat_click(&ClickEvent {
            action: ClickAction::OpenUrl,
            value: "https://a.example".into(),
        });
        assert!(app.chat.confirm_open());
        assert_eq!(
            app.chat.take_confirm().as_deref(),
            Some("https://a.example"),
            "the stored link the Enter path opens"
        );
        assert!(!app.chat.confirm_open(), "taking it clears the overlay");
    }

    #[test]
    fn the_tooltip_point_follows_the_open_field_and_steps_aside_for_the_confirm() {
        // The hover is the chat screen's own (`GuiChat.drawScreen`:305-310
        // hit-tests the free mouse every frame), so the frame feeds it only
        // while the chat is the screen on top — not while the confirm overlay
        // stands in for the replaced one.
        let point = (3.0, 4.0);
        assert_eq!(tooltip_point(true, false, Some(point)), Some(point));
        assert_eq!(tooltip_point(true, false, None), None, "no pointer yet");
        assert_eq!(tooltip_point(false, false, Some(point)), None, "closed");
        assert_eq!(
            tooltip_point(true, true, Some(point)),
            None,
            "the overlay is up"
        );
    }

    #[test]
    fn a_window_position_scales_to_the_frames_gui_units() {
        // `GuiNewChat.getChatComponent`:256-257 divides the raw mouse position
        // by the scale factor; the port's units are the frame's own, floored the
        // way the source's integer division lands.
        assert_eq!(
            scaled_cursor(PhysicalPosition::new(639.0, 719.0), 3),
            (213.0, 239.0)
        );
        assert_eq!(
            scaled_cursor(PhysicalPosition::new(639.9, 0.5), 3),
            (213.0, 0.0),
            "the division floors"
        );
        assert_eq!(
            scaled_cursor(PhysicalPosition::new(10.0, 20.0), 0),
            (10.0, 20.0),
            "a zero factor is no division"
        );
    }

    #[test]
    fn a_click_on_the_confirm_overlay_acts_on_nothing() {
        // While the overlay is up the click belongs to it, and the overlay has
        // no mouse surface in this milestone: no run is acted on, the overlay
        // stays, and above all no link opens (that is Enter's, and Enter's
        // only).
        let mut app = test_app();
        let (opener, seen) = recording_opener();
        app.opener = opener;
        app.chat.observe("\"AA\"", 1, 0);
        app.chat.open_confirm("https://a.example");
        app.chat_input.open("");
        app.apply_chat_click(&ClickEvent {
            action: ClickAction::OpenUrl,
            value: "https://b.example".into(),
        });
        assert_eq!(seen.borrow().len(), 0, "clicks never open links");
        assert!(app.chat.confirm_open(), "the overlay stays up");
    }
}
