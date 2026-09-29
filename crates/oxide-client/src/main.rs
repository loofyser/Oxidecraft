//! Window, event loop, wiring between game, renderer, and network thread.
//!
//! The client opens a window, clears it every redraw through
//! [`oxide_render::renderer::Renderer`], and shows the live frame rate and the chosen adapter
//! in the title. Escape or closing the window exits. A bounded smoke run sets
//! `OXIDECRAFT_MAX_FRAMES` to a frame count; the client then exits cleanly once that many
//! frames have been presented.
//!
//! With `--server host:port` a session thread joins the server and reports through a channel:
//! every frame the client drains it into the renderer and the F3 debug overlay, and the
//! session's clock and sky reports become the frame's fog and sky parameters. When the
//! session ends the client exits — a server that closed the connection cleanly is a normal
//! exit, not an error.

use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;
use crossbeam_channel::{Receiver, unbounded};
use oxide_game::hud::{HudState, debug_lines};
use oxide_game::session::{ClientEvent, Session, SessionConfig};
use oxide_proto_v47::serverbound::ClientSettings;
use oxide_render::camera::{Camera, CameraPose, DEFAULT_FOV, NEAR_PLANE};
use oxide_render::fog::{FogParams, fog_colour, linear_params};
use oxide_render::fps::FpsCounter;
use oxide_render::renderer::{Renderer, RendererError, SurfaceAction, classify_surface_error};
use oxide_render::sky::SkyParams;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

/// The frame count that bounds a smoke run, read from the environment.
const MAX_FRAMES_VAR: &str = "OXIDECRAFT_MAX_FRAMES";

/// The projection's far plane, in chunks; the plane itself is `far_chunks * 16 * √2`.
const FAR_CHUNKS: f32 = 8.0;

/// How many sections one column has.
const SECTIONS_PER_COLUMN: u8 = 16;

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
    /// What the debug overlay reports, updated from the session's events.
    hud: HudState,
    /// The clock and the sky the session last reported.
    sky: SkyState,
    /// The cloud counter M2 advances once per rendered frame.
    ///
    /// The source's counter is client-local and advances once per client tick
    /// (`RenderGlobal.updateClouds`, `RenderGlobal.java:1138-1142`, from `Minecraft.runTick`,
    /// `Minecraft.java:2193-2196`); M2 has no tick loop until M3, so the frame stands in for
    /// the tick and the two clients' cloud phases are independent.
    cloud_ticks: i64,
    /// Whether F3 has the overlay showing.
    overlay_visible: bool,
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
}

/// The fog and the sky one frame draws with, from the session's clock and sky and the client's
/// own frame state.
///
/// The session owns the world, so the five world-derived values and the moon's phase come from
/// its `Sky` event; the fog colour, the far plane and the cloud counter are the client's own.
/// The far plane is the render distance in blocks — the source's `farPlaneDistance`, which the
/// sky pass doubles and the cloud pass quadruples — and the fog's range comes from
/// [`linear_params`] of it.
fn frame_params(
    time_of_day: i64,
    dimension: i8,
    eye_y: f64,
    void_y_factor: f32,
    values: SkyValues,
    cloud_ticks: i64,
) -> (FogParams, SkyParams) {
    let far_plane = FAR_CHUNKS * 16.0;
    let (start, end) = linear_params(far_plane);
    let colour = fog_colour(dimension, time_of_day as f32, eye_y, void_y_factor);
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
    /// Builds the handler, reads the smoke-run frame limit, and opens the
    /// session when `--server` was given.
    fn new(cli: Cli) -> anyhow::Result<Self> {
        let max_frames = frame_limit(cli.frames);
        if let Some(limit) = max_frames {
            tracing::info!(frames = limit, "a frame limit is set, exiting after it");
        }
        let session = match cli.server {
            Some(address) => {
                let (host, port) = parse_server_address(&address)?;
                tracing::info!(server = %address, username = %cli.username, "joining the server");
                Some(spawn_session(host, port, cli.username, address))
            }
            None => None,
        };
        // The overlay starts visible when a session exists, because there is
        // something to report; the smoke path keeps it hidden until F3.
        let overlay_visible = session.is_some();
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
            cloud_ticks: 0,
            overlay_visible,
        })
    }

    /// Presents one frame, updates the title, and stops once the limit is reached.
    ///
    /// The session's events are drained first, so the meshes and the pose they
    /// carry are what this frame draws. A frame the surface is not ready for is
    /// dropped, and a surface that went stale is reconfigured before the frame
    /// is retried once. Any other failure stops the client.
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
            session_ended |= apply_session_event(renderer, &mut self.hud, &mut self.sky, event);
        }
        if session_ended {
            tracing::info!("the session ended, exiting");
            event_loop.exit();
            return;
        }
        // The camera follows the pose the server last reported. The smoke path
        // without a session stays as M0 left it: no camera, so the frame is the
        // sky clear.
        if self.session.is_some() {
            renderer.set_camera(Camera {
                pose: CameraPose {
                    position: self.hud.position,
                    yaw: self.hud.yaw,
                    pitch: self.hud.pitch,
                },
                fov_degrees: DEFAULT_FOV,
                near: NEAR_PLANE,
                far_chunks: FAR_CHUNKS,
            });
            // The counter M3's tick loop will advance once per tick; see the field's own note.
            self.cloud_ticks += 1;
            if let (Some(time_of_day), Some(values)) = (self.sky.time_of_day, self.sky.sky) {
                let (fog, sky) = frame_params(
                    time_of_day,
                    self.hud.dimension,
                    self.hud.position[1],
                    self.sky.void_y_factor,
                    values,
                    self.cloud_ticks,
                );
                renderer.set_fog(fog);
                renderer.set_sky(sky);
            }
        }
        renderer.set_overlay_lines(if self.overlay_visible {
            debug_lines(&self.hud)
        } else {
            Vec::new()
        });
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
}

/// The session thread's end of the wiring: where it came from and what it has
/// reported so far.
struct SessionLink {
    /// The address as the command line gave it, shown on the overlay.
    server: String,
    /// The events the session thread reports, drained once per frame.
    events: Receiver<ClientEvent>,
}

/// Opens a session on its own thread and returns the link the window drains.
///
/// The thread owns the connection, because the session blocks on reads and the
/// window must keep drawing. It runs to completion, reports the failure in the
/// log if it has one, and then reports [`ClientEvent::Disconnected`] whatever
/// the outcome was, so the window can stop. A session that returned cleanly is
/// a normal exit: the server closed the connection.
fn spawn_session(host: String, port: u16, username: String, server: String) -> SessionLink {
    let (sender, receiver) = unbounded();
    std::thread::spawn(move || {
        let config = SessionConfig {
            host,
            port,
            username,
            settings: ClientSettings::default(),
            // The assets are the bootstrap's to hand in (Task 14); until then
            // the session meshes every block as the atlas's fallback sprite.
            mesh: None,
        };
        match Session::connect(&config) {
            Ok(session) => {
                if let Err(error) = session.run(&sender) {
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
    }
}

/// Applies one event the session reported: meshes go to the renderer, the pose
/// and the join parameters to the overlay state, and the clock and the sky to
/// the frame's parameters.
///
/// Returns whether the session ended, which stops the client. The session
/// returns `Ok(())` when the server closed the connection, so its end is a
/// normal exit, not an error.
fn apply_session_event(
    renderer: &mut Renderer,
    hud: &mut HudState,
    sky: &mut SkyState,
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
        ClientEvent::PlayerPosition {
            x,
            y,
            z,
            yaw,
            pitch,
        } => {
            hud.position = [x, y, z];
            hud.yaw = yaw;
            hud.pitch = pitch;
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
        } => {
            sky.sky = Some(SkyValues {
                celestial_angle,
                colour,
                sun_brightness,
                star_brightness,
                cloud_colour,
                moon_phase,
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

impl ApplicationHandler for ClientApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
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
            WindowEvent::KeyboardInput { event, .. }
                if is_escape_press(event.state, &event.logical_key) =>
            {
                tracing::info!("escape was pressed, exiting");
                event_loop.exit();
            }
            WindowEvent::KeyboardInput { event, .. }
                if is_f3_press(event.state, &event.logical_key) =>
            {
                self.overlay_visible = !self.overlay_visible;
                tracing::info!(
                    visible = self.overlay_visible,
                    "the debug overlay was toggled"
                );
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

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        tracing::info!(frames = self.frames, "client exiting");
    }
}

/// Whether a key event is a press of Escape.
///
/// Kept out of the event match so the shortcut rule can be pinned without an event loop.
fn is_escape_press(state: ElementState, key: &Key) -> bool {
    state == ElementState::Pressed && *key == Key::Named(NamedKey::Escape)
}

/// Whether a key event is a press of F3.
///
/// Kept out of the event match so the overlay toggle can be pinned without an event loop.
fn is_f3_press(state: ElementState, key: &Key) -> bool {
    state == ElementState::Pressed && *key == Key::Named(NamedKey::F3)
}

#[cfg(test)]
mod tests {
    //! Key-routing and command-line tests.

    use super::{
        Cli, ClientApp, SkyValues, frame_params, is_escape_press, is_f3_press,
        parse_server_address, void_y_factor,
    };
    use clap::Parser;
    use winit::event::ElementState;
    use winit::keyboard::{Key, NamedKey};

    #[test]
    fn an_escape_press_exits() {
        assert!(is_escape_press(
            ElementState::Pressed,
            &Key::Named(NamedKey::Escape)
        ));
    }

    #[test]
    fn an_escape_release_and_other_keys_do_not_exit() {
        assert!(!is_escape_press(
            ElementState::Released,
            &Key::Named(NamedKey::Escape)
        ));
        assert!(!is_escape_press(
            ElementState::Pressed,
            &Key::Character("w".into())
        ));
    }

    #[test]
    fn only_a_press_of_f3_toggles() {
        assert!(is_f3_press(
            ElementState::Pressed,
            &Key::Named(NamedKey::F3)
        ));
        assert!(!is_f3_press(
            ElementState::Released,
            &Key::Named(NamedKey::F3)
        ));
        assert!(!is_f3_press(
            ElementState::Pressed,
            &Key::Character("f3".into())
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
    fn one_frames_parameters_carry_the_clock_the_sky_and_the_fog() {
        let values = SkyValues {
            celestial_angle: 0.25,
            colour: [0.5, 0.6, 0.7],
            sun_brightness: 0.9,
            star_brightness: 0.1,
            cloud_colour: [1.0, 0.5, 0.25],
            moon_phase: 5,
        };
        let (fog, sky) = frame_params(6000, 0, 64.0, 0.03125, values, 7);
        // The Overworld's noon fog at the eye on the ground is the provider's base itself
        // (`WorldProvider.getFogColor`, `WorldProvider.java:181-183`), and the range is the
        // terrain's own for the eight-chunk far plane (`EntityRenderer.java:2014-2015`).
        assert_eq!(fog.colour, [0.7529412, 0.84705883, 1.0]);
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
}
