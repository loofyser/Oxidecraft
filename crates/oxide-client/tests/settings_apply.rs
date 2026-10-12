//! The settings application path: one apply pushes every field to its
//! consumer, the cursor conversion holds scales 1-4, the save debounces, the
//! CLI/file precedence holds both arms, and a change round-trips through the
//! file with a mid-session rebuild keeping the values.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use oxide_client::settings_state::{
    AudioSink, KeymapPass, KeymapSink, NoAudio, ParticleSink, SAVE_DEBOUNCE, SettingsState,
    VideoSink, WindowSink, cursor_to_gui, framerate_gate, options_path, resolve_startup,
};
use oxide_game::audio::SoundCategory;
use oxide_game::input::InputEvent;
use oxide_game::particles::ParticleEngine;
use oxide_game::settings::{
    CATEGORY_AMBIENT, CATEGORY_MASTER, CATEGORY_MUSIC, CloudSetting, FRAMERATE_UNLIMITED,
    GameSettings, GraphicsSetting, ParticleSetting,
};

/// A recording video consumer: every field the apply path pushes.
#[derive(Default)]
struct FakeVideo {
    gui_scale: Option<u8>,
    brightness: Option<u8>,
    clouds: Option<u8>,
    graphics_fast: Option<bool>,
    smooth_lighting: Option<u8>,
    vsync: Option<bool>,
    fov: Option<u8>,
}

impl VideoSink for FakeVideo {
    fn set_gui_scale(&mut self, scale: u8) {
        self.gui_scale = Some(scale);
    }

    fn set_brightness(&mut self, brightness: u8) {
        self.brightness = Some(brightness);
    }

    fn set_clouds(&mut self, clouds: u8) {
        self.clouds = Some(clouds);
    }

    fn set_graphics_fast(&mut self, fast: bool) {
        self.graphics_fast = Some(fast);
    }

    fn set_smooth_lighting(&mut self, mode: u8) {
        self.smooth_lighting = Some(mode);
    }

    fn set_vsync(&mut self, on: bool) {
        self.vsync = Some(on);
    }

    fn set_fov(&mut self, degrees: u8) {
        self.fov = Some(degrees);
    }
}

/// A recording audio consumer: the master and category volumes.
struct FakeAudio {
    master: Option<f32>,
    categories: [Option<f32>; 9],
}

impl FakeAudio {
    fn new() -> Self {
        Self {
            master: None,
            categories: [None; 9],
        }
    }
}

impl AudioSink for FakeAudio {
    fn set_master_volume(&mut self, volume: f32) {
        self.master = Some(volume);
    }

    fn set_category_volume(&mut self, category: SoundCategory, volume: f32) {
        self.categories[category.as_index()] = Some(volume);
    }
}

/// A recording particle consumer: the regime.
#[derive(Default)]
struct FakeParticles {
    limit: Option<ParticleSetting>,
}

impl ParticleSink for FakeParticles {
    fn set_limit(&mut self, setting: ParticleSetting) {
        self.limit = Some(setting);
    }
}

/// A recording window: the fullscreen switch.
#[derive(Default)]
struct FakeWindow {
    fullscreen: Option<bool>,
}

impl WindowSink for FakeWindow {
    fn set_fullscreen(&mut self, fullscreen: bool) {
        self.fullscreen = Some(fullscreen);
    }
}

/// A recording keymap: the documented pass-through note.
#[derive(Default)]
struct FakeKeymap {
    noted: Option<(f32, bool)>,
}

impl KeymapSink for FakeKeymap {
    fn note_settings(&mut self, sensitivity: f32, invert_mouse: bool) {
        self.noted = Some((sensitivity, invert_mouse));
    }
}

static SCRATCH_ID: AtomicU64 = AtomicU64::new(0);

/// A fresh scratch options path: no store, no file, no other test's bytes.
fn scratch_options() -> PathBuf {
    let id = SCRATCH_ID.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "oxide-settings-apply-{}-{id}.toml",
        std::process::id()
    ))
}

/// A state with every pushed field off its default, over a scratch path and
/// a live session channel.
fn exercised_state(path: PathBuf) -> (SettingsState, crossbeam_channel::Receiver<InputEvent>) {
    let settings = GameSettings {
        render_distance: 10,
        brightness: 50,
        particles: ParticleSetting::Minimal,
        gui_scale: 2,
        fov: 90,
        max_framerate: 60,
        vsync: false,
        clouds: CloudSetting::Fast,
        graphics: GraphicsSetting::Fast,
        smooth_lighting: 0,
        fullscreen: true,
        sound_volume: 0.5,
        music_volume: 0.5,
        category_volumes: [0.25; 9],
        mouse_sensitivity: 0.8,
        invert_mouse: true,
        chat_mode: 1,
        chat_colors: false,
        language: "de_DE".to_string(),
        skin_parts: 0x01,
        ..Default::default()
    };
    let (mut state, seeded) = SettingsState::load(path, None);
    assert!(seeded, "the scratch path starts absent");
    let (sender, receiver) = crossbeam_channel::unbounded();
    state.set_session(sender);
    *state.settings_mut() = settings;
    (state, receiver)
}

#[test]
fn apply_pushes_every_renderer_field() {
    let (mut state, _session) = exercised_state(scratch_options());
    let mut video = FakeVideo::default();
    let mut audio = NoAudio;
    let mut particles = FakeParticles::default();
    let mut window = FakeWindow::default();
    let mut keymap = KeymapPass;
    state.apply(
        &mut video,
        &mut audio,
        &mut particles,
        &mut window,
        &mut keymap,
    );
    assert_eq!(
        video.gui_scale,
        Some(2),
        "the GUI scale reaches the renderer"
    );
    assert_eq!(
        video.brightness,
        Some(50),
        "the brightness reaches the renderer"
    );
    assert_eq!(
        video.clouds,
        Some(1),
        "the raw clouds value reaches set_clouds"
    );
    assert_eq!(
        video.graphics_fast,
        Some(true),
        "Fast reaches the graphics path"
    );
    assert_eq!(
        video.smooth_lighting,
        Some(0),
        "the lighting mode reaches the renderer"
    );
    assert_eq!(
        video.vsync,
        Some(false),
        "the VSync switch reaches the surface"
    );
    assert_eq!(video.fov, Some(90), "the FOV reaches the projection base");
}

#[test]
fn apply_pushes_volumes_with_the_music_fold() {
    let (mut state, _session) = exercised_state(scratch_options());
    let mut video = FakeVideo::default();
    let mut audio = FakeAudio::new();
    let mut particles = FakeParticles::default();
    let mut window = FakeWindow::default();
    let mut keymap = KeymapPass;
    state.apply(
        &mut video,
        &mut audio,
        &mut particles,
        &mut window,
        &mut keymap,
    );
    assert_eq!(
        audio.master,
        Some(0.5),
        "the master gain is the sound volume"
    );
    assert_eq!(
        audio.categories[CATEGORY_MUSIC],
        Some(0.125),
        "the music category carries the music fold over its level"
    );
    assert_eq!(
        audio.categories[CATEGORY_MASTER],
        Some(0.25),
        "the other categories pass their levels straight through"
    );
    assert_eq!(
        audio.categories[CATEGORY_AMBIENT],
        Some(0.25),
        "the last category is pushed too"
    );
    for (index, pushed) in audio.categories.iter().enumerate() {
        assert!(pushed.is_some(), "category {index} is pushed");
    }
}

#[test]
fn apply_pushes_the_particle_regime() {
    let (mut state, _session) = exercised_state(scratch_options());
    let mut video = FakeVideo::default();
    let mut audio = NoAudio;
    let mut particles = FakeParticles::default();
    let mut window = FakeWindow::default();
    let mut keymap = KeymapPass;
    state.apply(
        &mut video,
        &mut audio,
        &mut particles,
        &mut window,
        &mut keymap,
    );
    assert_eq!(particles.limit, Some(ParticleSetting::Minimal));
}

#[test]
fn apply_pushes_fullscreen_and_notes_the_keymap() {
    let (mut state, _session) = exercised_state(scratch_options());
    let mut video = FakeVideo::default();
    let mut audio = NoAudio;
    let mut particles = FakeParticles::default();
    let mut window = FakeWindow::default();
    let mut keymap = FakeKeymap::default();
    state.apply(
        &mut video,
        &mut audio,
        &mut particles,
        &mut window,
        &mut keymap,
    );
    assert_eq!(
        window.fullscreen,
        Some(true),
        "fullscreen reaches the window"
    );
    assert_eq!(
        keymap.noted,
        Some((0.8, true)),
        "the keymap pass-through names the sensitivity and the invert flag"
    );
}

#[test]
fn apply_sends_the_session_pushes() {
    let (mut state, session) = exercised_state(scratch_options());
    let mut video = FakeVideo::default();
    let mut audio = NoAudio;
    let mut particles = FakeParticles::default();
    let mut window = FakeWindow::default();
    let mut keymap = KeymapPass;
    state.apply(
        &mut video,
        &mut audio,
        &mut particles,
        &mut window,
        &mut keymap,
    );
    let events: Vec<InputEvent> = session.try_iter().collect();
    assert_eq!(
        events.len(),
        2,
        "one view-distance push and one settings push"
    );
    assert!(
        matches!(events[0], InputEvent::SetViewDistance { chunks: 10 }),
        "the view distance rides its own push: {events:?}"
    );
    match &events[1] {
        InputEvent::PushSettings(push) => {
            assert_eq!(push.settings.view_distance, 10);
            assert_eq!(push.settings.locale, "de_DE");
            assert_eq!(push.settings.chat_mode, 1);
            assert!(!push.settings.chat_colors);
            assert_eq!(push.settings.skin_parts, 0x01);
            assert!(push.graphics_fast, "the Fast path rides the push");
            assert_eq!(push.smooth_lighting, 0);
            assert_eq!(push.sensitivity, 0.8);
            assert_eq!(push.particles, ParticleSetting::Minimal);
        }
        other => panic!("the second push carries the whole store: {other:?}"),
    }
}

#[test]
fn apply_pushes_raw_clouds_without_resolution() {
    // The `< 4 → Off` clause stays Task 28's: a short view distance never
    // resolves the pushed clouds value here.
    let path = scratch_options();
    let (mut state, _session) = SettingsState::load(path, None);
    state.settings_mut().render_distance = 2;
    state.settings_mut().clouds = CloudSetting::Fancy;
    let mut video = FakeVideo::default();
    let mut audio = NoAudio;
    let mut particles = FakeParticles::default();
    let mut window = FakeWindow::default();
    let mut keymap = KeymapPass;
    state.apply(
        &mut video,
        &mut audio,
        &mut particles,
        &mut window,
        &mut keymap,
    );
    assert_eq!(video.clouds, Some(2), "the raw Fancy value is pushed as-is");
}

#[test]
fn the_cursor_conversion_holds_scales_one_to_four() {
    // Window px → frame units by the scale factor, floored the source's
    // integer-division way; scale 0 is no division at all.
    assert_eq!(cursor_to_gui(7.9, 3.1, 1), (7.0, 3.0));
    assert_eq!(cursor_to_gui(8.0, 4.0, 2), (4.0, 2.0));
    assert_eq!(cursor_to_gui(639.0, 719.0, 3), (213.0, 239.0));
    assert_eq!(cursor_to_gui(10.0, 20.0, 4), (2.0, 5.0));
    assert_eq!(cursor_to_gui(639.9, 0.5, 3), (213.0, 0.0));
    assert_eq!(cursor_to_gui(10.5, 20.5, 0), (10.5, 20.5));
}

#[test]
fn the_save_debounce_coalesces_a_burst_into_one_write() {
    assert_eq!(SAVE_DEBOUNCE, Duration::from_millis(500));
    let path = scratch_options();
    let (mut state, _seeded) = SettingsState::load(path.clone(), None);
    let start = Instant::now();
    for step in 0..5u64 {
        state.settings_mut().brightness = 10 + step as u8;
        state.mark_changed(start + Duration::from_millis(step * 10));
    }
    assert!(
        !state
            .poll_save(start + Duration::from_millis(400))
            .expect("the poll reads"),
        "no write lands inside the window"
    );
    assert!(
        state
            .poll_save(start + Duration::from_millis(600))
            .expect("the poll writes"),
        "one write lands past the trailing edge"
    );
    assert!(
        !state
            .poll_save(start + Duration::from_millis(700))
            .expect("the poll re-reads"),
        "at most one write per burst"
    );
    let reread = GameSettings::load(&path);
    assert_eq!(
        reread.brightness, 14,
        "the burst's last value is what persists"
    );
}

#[test]
fn the_missing_file_seeds_from_the_flag_and_never_writes() {
    let path = scratch_options();
    let (settings, seeded) = resolve_startup(&path, Some(6));
    assert!(seeded, "a missing file reports the seed");
    assert_eq!(
        settings.render_distance, 6,
        "the flag seeds the view distance"
    );
    assert!(
        !path.exists(),
        "resolving never writes; the caller saves the seed"
    );
}

#[test]
fn the_flag_overrides_a_present_file_in_memory_only() {
    let path = scratch_options();
    let stored = GameSettings {
        render_distance: 10,
        ..Default::default()
    };
    stored.save(&path).expect("the fixture writes");
    let (settings, seeded) = resolve_startup(&path, Some(6));
    assert!(!seeded, "a present file is no seed");
    assert_eq!(settings.render_distance, 6, "the flag wins for this run");
    let kept = GameSettings::load(&path);
    assert_eq!(kept.render_distance, 10, "the file keeps its value");
}

#[test]
fn an_unchanged_run_never_touches_the_file() {
    // The exit-save leg of the flag arm: the run reads the override while
    // the writer stays clean, so the exit save is a no-op.
    let path = scratch_options();
    let stored = GameSettings {
        render_distance: 10,
        ..Default::default()
    };
    stored.save(&path).expect("the fixture writes");
    let before = std::fs::read(&path).expect("the fixture reads");
    let (mut state, seeded) = SettingsState::load(path.clone(), Some(6));
    assert!(!seeded, "a present file is no seed");
    assert_eq!(state.view_distance(), 6, "the run reads the override");
    assert_eq!(
        state.wire_settings().view_distance,
        6,
        "the wire carries it"
    );
    state.save_now().expect("the exit save runs");
    let after = std::fs::read(&path).expect("the file still reads");
    assert_eq!(before, after, "an unchanged run never touches the file");
}

#[test]
fn without_a_flag_a_present_file_governs() {
    let path = scratch_options();
    let stored = GameSettings {
        render_distance: 10,
        ..Default::default()
    };
    stored.save(&path).expect("the fixture writes");
    let (settings, seeded) = resolve_startup(&path, None);
    assert!(!seeded);
    assert_eq!(
        settings.render_distance, 10,
        "a restart keeps the changed value"
    );
}

#[test]
fn without_a_flag_a_missing_file_seeds_the_store_default() {
    let path = scratch_options();
    let (settings, seeded) = resolve_startup(&path, None);
    assert!(seeded);
    assert_eq!(
        settings.render_distance,
        GameSettings::default().render_distance,
        "no flag means the store's own default seeds"
    );
}

#[test]
fn a_change_round_trips_and_a_rebuild_keeps_the_values() {
    let path = scratch_options();
    let (mut state, _seeded) = SettingsState::load(path.clone(), None);
    state.settings_mut().brightness = 77;
    state.settings_mut().fov = 100;
    state.mark_changed(Instant::now());
    state.save_now().expect("the change saves");
    let reread = GameSettings::load(&path);
    assert_eq!(reread, *state.settings(), "save then re-read is equal");
    // A mid-session renderer rebuild keeps the values: a fresh consumer
    // applied from the same state reads the same fields.
    let mut rebuilt = FakeVideo::default();
    let mut audio = NoAudio;
    let mut particles = FakeParticles::default();
    let mut window = FakeWindow::default();
    let mut keymap = KeymapPass;
    state.apply(
        &mut rebuilt,
        &mut audio,
        &mut particles,
        &mut window,
        &mut keymap,
    );
    assert_eq!(rebuilt.brightness, Some(77));
    assert_eq!(rebuilt.fov, Some(100));
}

#[test]
fn the_framerate_gate_blocks_until_the_period_passes() {
    let start = Instant::now();
    assert!(
        !framerate_gate(start, Some(start), 120),
        "no time passed, the frame waits"
    );
    assert!(
        framerate_gate(start + Duration::from_micros(8400), Some(start), 120),
        "past the 120 Hz period the frame runs"
    );
    assert!(
        framerate_gate(start, None, 120),
        "with no frame yet the first one runs"
    );
    assert!(
        framerate_gate(start, Some(start), FRAMERATE_UNLIMITED),
        "the 255 sentinel skips the gate"
    );
    assert!(
        !framerate_gate(start, Some(start), 10),
        "a low cap still gates a fresh frame"
    );
}

#[test]
fn the_real_particle_engine_takes_the_limit() {
    let mut engine = ParticleEngine::seeded(0x9E37_79B9_7F4A_7C15, 8);
    ParticleSink::set_limit(&mut engine, ParticleSetting::Minimal);
    assert_eq!(engine.setting(), ParticleSetting::Minimal);
}

#[test]
fn the_options_path_hangs_below_the_store_root() {
    let root = PathBuf::from("/store/root");
    assert_eq!(options_path(&root), root.join("options.toml"));
}
