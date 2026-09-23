//! Window, event loop, wiring between game, renderer, and network thread.
//!
//! The client opens a window, clears it every redraw through
//! [`oxide_render::renderer::Renderer`], and shows the live frame rate and the chosen adapter
//! in the title. Escape or closing the window exits. A bounded smoke run sets
//! `OXIDECRAFT_MAX_FRAMES` to a frame count; the client then exits cleanly once that many
//! frames have been presented.

use std::sync::Arc;
use std::time::{Duration, Instant};

use oxide_render::fps::FpsCounter;
use oxide_render::renderer::{Renderer, RendererError, SurfaceAction, classify_surface_error};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

/// The frame count that bounds a smoke run, read from the environment.
const MAX_FRAMES_VAR: &str = "OXIDECRAFT_MAX_FRAMES";

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let event_loop = EventLoop::new()?;
    let mut app = ClientApp::new();
    event_loop.run_app(&mut app)?;
    anyhow::ensure!(
        !app.stopped_on_error,
        "the client stopped after a window or GPU error"
    );
    Ok(())
}

/// The application handler: owns the window and the renderer and presents one frame per redraw.
struct ClientApp {
    /// The window, once the event loop has resumed and it has been created.
    window: Option<Arc<Window>>,
    /// The renderer for that window.
    renderer: Option<Renderer>,
    /// The frame-rate accounting shown in the title.
    fps: FpsCounter,
    /// Frames presented since start.
    frames: u64,
    /// The frame count to stop after, when the smoke-run variable asks for one.
    max_frames: Option<u64>,
    /// Whether the client must report a failure once the event loop returns.
    stopped_on_error: bool,
}

impl ClientApp {
    /// Builds the handler and reads the smoke-run frame limit.
    fn new() -> Self {
        let max_frames = std::env::var(MAX_FRAMES_VAR)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|limit| *limit > 0);
        if let Some(limit) = max_frames {
            tracing::info!(frames = limit, "a frame limit is set, exiting after it");
        }
        Self {
            window: None,
            renderer: None,
            fps: FpsCounter::new(Duration::from_secs(1)),
            frames: 0,
            max_frames,
            stopped_on_error: false,
        }
    }

    /// Presents one frame, updates the title, and stops once the limit is reached.
    ///
    /// A frame the surface is not ready for is dropped, and a surface that went stale is
    /// reconfigured before the frame is retried once. Any other failure stops the client.
    fn draw(&mut self, event_loop: &ActiveEventLoop) {
        let (Some(window), Some(renderer)) = (self.window.as_ref(), self.renderer.as_mut()) else {
            return;
        };
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
        let title = format!("Oxidecraft — {fps:.0} fps — {}", renderer.adapter_name());
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

#[cfg(test)]
mod tests {
    //! Key-routing tests for the exit shortcut.

    use super::is_escape_press;
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
}
