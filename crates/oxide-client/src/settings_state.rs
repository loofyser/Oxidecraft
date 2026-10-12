//! The settings application path: one client-side apply pushes every
//! settings field to its consumer, the store saves on change, and startup
//! reads the file before the first frame.
//!
//! [`SettingsState`] owns the [`GameSettings`] instance, the save-on-change
//! writer and the apply path. [`SettingsState::apply`] pushes every field to
//! its consumer: the renderer (GUI scale, brightness, clouds, graphics,
//! smooth lighting, VSync, FOV), the session (view distance, the wire
//! settings, the mesh settings, the sensitivity, the particle regime), the
//! audio engine (master and category volumes), the particle engine (the
//! regime), the window (fullscreen) and the keymap (a documented
//! pass-through — the keymap has no binding store; Task 19 owns the
//! `Binding` list).
//!
//! The consumers are traits so the path stays headless-testable: the
//! production types implement them beside the fakes the suites push through.
//! The session is reached through its input channel, which the state holds
//! once the session exists; without one the pushes are skipped.
//!
//! # Source rules carried here
//!
//! Fullscreen maps to winit borderless fullscreen (the source toggles its
//! exclusive mode, `GameSettings.java:479-487`); the key that toggles it
//! stays Task 25's. Clouds push the raw store value — the `< 4 → Off` clause
//! (`GameSettings.shouldRenderClouds`, `:1256`) stays Task 28's. The
//! framerate 255 sentinel skips the limiter; the top step reads "Unlimited"
//! (`options.framerateLimit.max`, `en_US.lang:340`).
//!
//! # Known limits, recorded
//!
//! These store fields have no consumer reachable through the apply params,
//! so apply carries them nowhere: `mipmap_levels` (no renderer mipmap
//! consumer), `view_bobbing` (the camera effect builds unconditionally),
//! `alternate_blocks` and `entity_shadows` (mesher/entity-pass reads with no
//! push path from the client), `invert_mouse` and `smooth_camera` (no
//! consumer), `force_unicode` (the font owns its path), the chat geometry
//! (`chat_opacity`, `chat_scale`, `chat_width`, `chat_height` — the chat view
//! reads its own values), and `last_server` (the server list's, never
//! applied). `use_vbos` and `anaglyph` are recorded no-ops in the store
//! itself. The chat mode, chat colours, language and skin parts ride the
//! session push as wire fields.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use oxide_game::audio::{AudioEngine, SoundCategory};
use oxide_game::input::{InputEvent, SessionPush};
use oxide_game::particles::ParticleEngine;
use oxide_game::settings::{
    CATEGORY_MUSIC, FRAMERATE_RANGE, FRAMERATE_UNLIMITED, GameSettings, GraphicsSetting,
    ParticleSetting, VIEW_DISTANCE_RANGE,
};
use oxide_proto_v47::serverbound::ClientSettings;
use oxide_render::renderer::Renderer;

/// The save-on-change debounce: a trailing-edge 500 ms coalescing window.
/// Rapid changes reset the deadline, so a burst of edits writes once.
pub const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

/// The options file name below the store root (the D11 format).
pub const OPTIONS_FILE: &str = "options.toml";

/// The options file path for a store root.
pub fn options_path(store_root: &Path) -> PathBuf {
    store_root.join(OPTIONS_FILE)
}

/// A window position in the frame's GUI units: the raw position divided by
/// the GUI scale factor, floored the way the source's own integer division
/// lands (`GuiNewChat.getChatComponent:256-257` — `(int)(mouseX / f)` and
/// the same for y). A zero factor is no division at all.
///
/// A mid-run GUI-scale change keeps the frame's unit: the window converts
/// every pointer event through the live factor, so the frame's units never
/// mix scales.
pub fn cursor_to_gui(x: f64, y: f64, scale: u32) -> (f32, f32) {
    if scale == 0 {
        return (x as f32, y as f32);
    }
    let factor = f64::from(scale);
    ((x / factor).floor() as f32, (y / factor).floor() as f32)
}

/// Whether a frame may present under the framerate cap: true when no frame
/// has presented yet, when the cap is the unlimited sentinel, or when a full
/// cap period has passed since the last presented frame.
///
/// The 255 sentinel skips the gate; the runtime never branches on the
/// source's 260 top step. A zero cap clamps to the range floor rather than
/// dividing by zero.
pub fn framerate_gate(now: Instant, last: Option<Instant>, limit: u8) -> bool {
    if limit == FRAMERATE_UNLIMITED {
        return true;
    }
    let Some(last) = last else {
        return true;
    };
    let limit = limit.clamp(FRAMERATE_RANGE.0, FRAMERATE_RANGE.1);
    now.saturating_duration_since(last) >= Duration::from_secs_f64(1.0 / f64::from(limit))
}

/// Reads the startup settings for an options path and the CLI's
/// `--render-distance`: a missing file seeds from the flag (its absence
/// seeds the store default), a present file loads and the flag — when given
/// — overrides the view distance in memory only, never writing.
///
/// Returns the settings and whether the file was missing (the caller saves
/// the seed so it persists). Reading never fails: an unreadable file reads
/// as the store default.
pub fn resolve_startup(path: &Path, cli_render_distance: Option<u8>) -> (GameSettings, bool) {
    if !path.exists() {
        let mut settings = GameSettings::default();
        if let Some(chunks) = cli_render_distance {
            settings.render_distance = chunks;
        }
        return (settings, true);
    }
    let mut settings = GameSettings::load(path);
    if let Some(chunks) = cli_render_distance {
        settings.render_distance = chunks;
    }
    (settings, false)
}

/// The renderer's settings consumer: every video field the apply path owns.
pub trait VideoSink {
    /// Takes the GUI-scale setting.
    fn set_gui_scale(&mut self, scale: u8);
    /// Takes the brightness setting.
    fn set_brightness(&mut self, brightness: u8);
    /// Takes the raw cloud setting (no `< 4 → Off` resolution here).
    fn set_clouds(&mut self, clouds: u8);
    /// Takes whether the graphics path is Fast.
    fn set_graphics_fast(&mut self, fast: bool);
    /// Takes the smooth-lighting setting.
    fn set_smooth_lighting(&mut self, mode: u8);
    /// Takes the VSync switch.
    fn set_vsync(&mut self, on: bool);
    /// Takes the field-of-view setting in degrees.
    fn set_fov(&mut self, degrees: u8);
}

impl VideoSink for Renderer {
    fn set_gui_scale(&mut self, scale: u8) {
        self.set_gui_scale(scale);
    }

    fn set_brightness(&mut self, brightness: u8) {
        self.set_brightness(brightness);
    }

    fn set_clouds(&mut self, clouds: u8) {
        self.set_clouds(clouds);
    }

    fn set_graphics_fast(&mut self, fast: bool) {
        self.set_graphics(fast);
    }

    fn set_smooth_lighting(&mut self, mode: u8) {
        self.set_smooth_lighting(mode);
    }

    fn set_vsync(&mut self, on: bool) {
        self.set_vsync(on);
    }

    fn set_fov(&mut self, degrees: u8) {
        self.set_fov(degrees);
    }
}

/// The audio engine's settings consumer: the master and category volumes.
pub trait AudioSink {
    /// Takes the master volume.
    fn set_master_volume(&mut self, volume: f32);
    /// Takes one category's volume.
    fn set_category_volume(&mut self, category: SoundCategory, volume: f32);
}

impl AudioSink for AudioEngine {
    fn set_master_volume(&mut self, volume: f32) {
        self.set_master_volume(volume);
    }

    fn set_category_volume(&mut self, category: SoundCategory, volume: f32) {
        self.set_category_volume(category, volume);
    }
}

/// No audio engine: without a session nothing plays, so the volumes land
/// nowhere.
pub struct NoAudio;

impl AudioSink for NoAudio {
    fn set_master_volume(&mut self, _volume: f32) {}

    fn set_category_volume(&mut self, _category: SoundCategory, _volume: f32) {}
}

/// The particle engine's settings consumer: the spawn regime.
pub trait ParticleSink {
    /// Takes the particle regime.
    fn set_limit(&mut self, setting: ParticleSetting);
}

impl ParticleSink for ParticleEngine {
    fn set_limit(&mut self, setting: ParticleSetting) {
        self.set_limit(setting);
    }
}

/// The window's settings consumer: the fullscreen switch.
pub trait WindowSink {
    /// Takes the fullscreen switch (borderless when set).
    fn set_fullscreen(&mut self, fullscreen: bool);
}

/// The keymap's settings consumer: a documented pass-through.
///
/// The keymap has no binding store — Task 19 owns the `Binding` list — so
/// the note lands nowhere. It exists so the apply path names the consumer
/// and the binding task has the call site to take over.
pub trait KeymapSink {
    /// Notes the sensitivity and invert-mouse settings.
    fn note_settings(&mut self, sensitivity: f32, invert_mouse: bool);
}

/// The keymap pass-through: [`KeymapSink`] with nowhere to push to.
pub struct KeymapPass;

impl KeymapSink for KeymapPass {
    fn note_settings(&mut self, _sensitivity: f32, _invert_mouse: bool) {}
}

/// The client's settings owner: the [`GameSettings`] instance, the
/// save-on-change writer and the apply path.
///
/// The CLI's `--render-distance` never enters the owned settings when the
/// file is present: it rides [`SettingsState::run_render_distance`] for the
/// run, so the writer cannot persist it. A missing file seeds from the flag
/// instead, and the seed persists like any stored value.
pub struct SettingsState {
    /// The owned settings.
    settings: GameSettings,
    /// The options file the writer saves to.
    path: PathBuf,
    /// The session's input channel, once a session exists. The session-held
    /// values (view distance, wire settings, mesh settings, sensitivity and
    /// the particle regime) push through it.
    session: Option<Sender<InputEvent>>,
    /// When the debounced save is due, set by [`SettingsState::mark_changed`].
    pending_save: Option<Instant>,
    /// Whether the owned settings differ from the file: only a dirty state
    /// writes, so an unchanged run — including a flag-overridden one — never
    /// touches the file.
    dirty: bool,
    /// The CLI's `--render-distance` for this run, held only when the file
    /// was present: the run reads it, the file keeps its value.
    run_render_distance: Option<u8>,
}

impl SettingsState {
    /// Loads the startup state for an options path and the CLI's
    /// `--render-distance` (see [`resolve_startup`]), holding the session
    /// pushes until [`SettingsState::set_session`] names the channel.
    ///
    /// Returns the state and whether the file was missing (the caller saves
    /// the seed so it persists).
    pub fn load(path: PathBuf, cli_render_distance: Option<u8>) -> (Self, bool) {
        let (settings, seeded) = resolve_startup(&path, cli_render_distance);
        // A seed is a change the caller saves; an override is not owned, so
        // it rides beside the settings and the state starts clean.
        let run_render_distance = if seeded { None } else { cli_render_distance };
        (
            Self {
                settings,
                path,
                session: None,
                pending_save: None,
                dirty: seeded,
                run_render_distance,
            },
            seeded,
        )
    }

    /// Names the session's input channel: later applies push the
    /// session-held values through it.
    pub fn set_session(&mut self, sender: Sender<InputEvent>) {
        self.session = Some(sender);
    }

    /// The owned settings.
    pub fn settings(&self) -> &GameSettings {
        &self.settings
    }

    /// The owned settings, mutably. Callers mark the change with
    /// [`SettingsState::mark_changed`] and push it with
    /// [`SettingsState::apply`].
    pub fn settings_mut(&mut self) -> &mut GameSettings {
        &mut self.settings
    }

    /// The effective view distance the frame reads: the run override when
    /// the flag was given over a present file, else the stored value,
    /// clamped into range.
    pub fn view_distance(&self) -> u8 {
        self.run_render_distance
            .unwrap_or(self.settings.render_distance)
            .clamp(VIEW_DISTANCE_RANGE.0, VIEW_DISTANCE_RANGE.1)
    }

    /// The wire settings the session handshakes with: the store's values
    /// with the effective view distance.
    pub fn wire_settings(&self) -> ClientSettings {
        let mut wire = self.settings.to_client_settings();
        wire.view_distance = self.view_distance();
        wire
    }

    /// The framerate cap the frame gate reads.
    pub fn max_framerate(&self) -> u8 {
        self.settings.max_framerate
    }

    /// Marks the settings changed: the debounced save goes due
    /// [`SAVE_DEBOUNCE`] after `now`, and the state turns dirty so the
    /// writer has something to persist. Rapid marks reset the deadline, so
    /// a burst of edits writes once.
    pub fn mark_changed(&mut self, now: Instant) {
        self.pending_save = Some(now + SAVE_DEBOUNCE);
        self.dirty = true;
    }

    /// Saves now when a debounced save is due, reporting whether a write
    /// happened. A clean state never writes; a failed write leaves the
    /// deadline armed for the next poll.
    pub fn poll_save(&mut self, now: Instant) -> io::Result<bool> {
        if self.path.as_os_str().is_empty() {
            self.pending_save = None;
            return Ok(false);
        }
        let Some(due) = self.pending_save else {
            return Ok(false);
        };
        if now < due {
            return Ok(false);
        }
        self.pending_save = None;
        if !self.dirty {
            return Ok(false);
        }
        self.settings.save(&self.path)?;
        self.dirty = false;
        Ok(true)
    }

    /// Saves the settings whole-file at once when dirty, clearing any
    /// pending save. A clean state is a no-op: an unchanged run never
    /// touches the file, flag override or not.
    pub fn save_now(&mut self) -> io::Result<()> {
        if self.path.as_os_str().is_empty() {
            self.pending_save = None;
            return Ok(());
        }
        self.pending_save = None;
        if !self.dirty {
            return Ok(());
        }
        self.settings.save(&self.path)?;
        self.dirty = false;
        Ok(())
    }

    /// Pushes every field to its consumer: the renderer, the audio engine,
    /// the particle engine, the window, the keymap pass-through and — when
    /// a session is held — the session's input channel.
    ///
    /// The music category carries the music fold over its level (the music
    /// volume times the category level); every other category passes its
    /// level straight through. The clouds push the raw store value — the
    /// `< 4 → Off` clause stays Task 28's. A closed session channel drops
    /// the pushes silently: the session is gone, so there is nothing to
    /// tell.
    pub fn apply<R, E, P, W, K>(
        &mut self,
        renderer: &mut R,
        engine: &mut E,
        particles: &mut P,
        window: &mut W,
        keymap: &mut K,
    ) where
        R: VideoSink,
        E: AudioSink,
        P: ParticleSink,
        W: WindowSink,
        K: KeymapSink,
    {
        let settings = &self.settings;
        renderer.set_gui_scale(settings.gui_scale);
        renderer.set_brightness(settings.brightness);
        renderer.set_clouds(settings.clouds.as_u8());
        renderer.set_graphics_fast(matches!(settings.graphics, GraphicsSetting::Fast));
        renderer.set_smooth_lighting(settings.smooth_lighting);
        renderer.set_vsync(settings.vsync);
        renderer.set_fov(settings.fov);
        engine.set_master_volume(settings.sound_volume);
        for (index, level) in settings.category_volumes.iter().enumerate() {
            let level = if index == CATEGORY_MUSIC {
                settings.music_volume * *level
            } else {
                *level
            };
            engine.set_category_volume(SoundCategory::ORDER[index], level);
        }
        particles.set_limit(settings.particles);
        window.set_fullscreen(settings.fullscreen);
        keymap.note_settings(settings.mouse_sensitivity, settings.invert_mouse);
        if let Some(session) = self.session.as_ref() {
            let chunks = self.view_distance();
            let push = SessionPush {
                settings: self.wire_settings(),
                graphics_fast: matches!(settings.graphics, GraphicsSetting::Fast),
                smooth_lighting: settings.smooth_lighting,
                sensitivity: settings.mouse_sensitivity,
                particles: settings.particles,
            };
            let _ = session.send(InputEvent::SetViewDistance { chunks });
            let _ = session.send(InputEvent::PushSettings(push));
        }
    }
}
