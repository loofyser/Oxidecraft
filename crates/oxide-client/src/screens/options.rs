//! The options hub and the video settings screen.
//!
//! The port reads values and names from the source's hub
//! (`client/gui/GuiOptions.java`), its video screen
//! (`client/gui/GuiVideoSettings.java`), its options-row list
//! (`client/gui/GuiOptionsRowList.java`), the option slider and button
//! (`GuiOptionSlider.java`, `GuiOptionButton.java`) and the settings they
//! write (`client/settings/GameSettings.java`); no source text is copied.
//!
//! The hub is the source's BUTTON GRID (R1 — no tab strip exists in
//! 1.8.9): the FOV slider row 0, then — with no world in v1 — the
//! REALMS_NOTIFICATIONS toggle beside it (`:81`); fixed 150x20 pairs at
//! `w/2-155` / `w/2+5`, rows `h/6+48k-6`: skin (110), sounds (106) +
//! stream (107, OMITTED — no Twitch in v1, recorded), video (101) +
//! controls (100 — the controls screen lands in Task 19; the button
//! answers its action and opens nothing yet, recorded), language
//! (102 — Task 26; same placeholder shape, recorded) + chat (103),
//! resource packs (105 — the packs no-op is Task 26's, recorded) +
//! snooper (104 — wired to Task 17's declared slot), Done (200).
//! Super-secret (8675309) is OMITTED, recorded. No open saves: the
//! writes save — toggles and cycles persist on the press, sliders ride
//! the debounced mark, Done saves and returns to the parent; the title
//! reads "Options" at y 15.
//!
//! The video screen carries exactly the source's sixteen rows in order
//! (R2): GRAPHICS, RENDER_DISTANCE, AMBIENT_OCCLUSION, FRAMERATE_LIMIT,
//! ANAGLYPH, VIEW_BOBBING, GUI_SCALE, GAMMA, RENDER_CLOUDS, PARTICLES,
//! USE_FULLSCREEN, ENABLE_VSYNC, MIPMAP_LEVELS, BLOCK_ALTERNATIVES,
//! USE_VBO, ENTITY_SHADOWS — eight rows of two 150x20 (`w/2-155`,
//! `+160`), slot height 25, list top 32 / bottom h-32, width 400; Done
//! (200) at `h-27` saves and returns. Without VBO support the list
//! truncates BEFORE USE_VBO — the copy loop `break`s, so ENTITY_SHADOWS
//! drops too (`:34-51`; dead in practice — the renderer always uses
//! VBOs — recorded). The scrollbar thumb is not drawn (recorded): the
//! wheel scrolls the at-most-one-slot overflow instead.
//!
//! # Apply and save rules (R3, R4)
//!
//! The screens call NO renderer: every widget writes into the handed-over
//! [`GameSettings`](oxide_game::settings::GameSettings), and the owner
//! pushes it through the live [`SettingsState::apply`](crate::settings_state::SettingsState::apply)
//! (GUI scale included — apply owns the re-layout; the four-scale table
//! stays Task 28's). Each answered press carries its [`SaveKind`]: the
//! source's `setOptionValue:522` ends in `saveOptions`, so every TOGGLE
//! and cycle persists at once ([`SaveKind::Immediate`]); the slider
//! writes (`setOptionFloatValue:256-360`) never save, so sliders persist
//! at the next Done/screen-switch save ([`SaveKind::Deferred`]) —
//! carried here as the debounced writer's dirty mark, flushed by the
//! owner's Done-time save. The C15 re-send rides Done with a player
//! present (`sendSettingsToServer:1203-1215`): port-side the owner's
//! Done-time apply pushes `SetViewDistance` + `PushSettings` (the C15
//! fields) only while a session channel is held — verified against
//! [`SettingsState::apply`](crate::settings_state::SettingsState::apply)'s
//! session arm, recorded.

use oxide_assets::font::Font;
use oxide_game::chat::LanguageTable;
use oxide_game::settings::{CloudSetting, GameSettings, GraphicsSetting, ParticleSetting};
use oxide_render::hud::{HudDraw, ScaledResolution};
use oxide_render::text::string_width;

use super::widgets::{Button, OptionButton, OptionSlider};
pub use super::widgets::{denormalize_option, normalize_option, snap_to_step};

/// The controls button's id (`initGui`'s `options.controls` button).
pub const OPTIONS_CONTROLS_ID: i32 = 100;
/// The video button's id (the hub's route to the video screen).
pub const OPTIONS_VIDEO_ID: i32 = 101;
/// The language button's id (Task 26 owns its screen).
pub const OPTIONS_LANGUAGE_ID: i32 = 102;
/// The chat button's id (the chat options land with the chat rows).
pub const OPTIONS_CHAT_ID: i32 = 103;
/// The snooper button's id (the route to Task 17's declared slot).
pub const OPTIONS_SNOOPER_ID: i32 = 104;
/// The resource-packs button's id (the packs no-op is Task 26's).
pub const OPTIONS_RESOURCEPACK_ID: i32 = 105;
/// The sounds button's id (the sound screen lands with the audio rows).
pub const OPTIONS_SOUNDS_ID: i32 = 106;
/// The skin-customization button's id.
pub const OPTIONS_SKIN_ID: i32 = 110;
/// The Done button's id (saves and returns to the parent).
pub const OPTIONS_DONE_ID: i32 = 200;
/// The realms toggle's id: the REALMS_NOTIFICATIONS option ordinal (the
/// 45th `Options` entry, so 44) — the no-world row-0 toggle carries the
/// option's own id like every `GuiOptionButton`.
pub const OPTIONS_REALMS_ID: i32 = 44;
/// The hub title's height: the draw centres "Options" at `(w/2, 15)`.
pub const OPTIONS_TITLE_Y: f32 = 15.0;
/// The video title's height: the draw centres it at `(w/2, 5)`.
pub const VIDEO_TITLE_Y: f32 = 5.0;
/// The row list's top edge in GUI pixels (`initGui`'s 32).
pub const VIDEO_LIST_TOP: f32 = 32.0;
/// The row list's bottom inset in GUI pixels (the list ends at h-32).
pub const VIDEO_LIST_BOTTOM_INSET: f32 = 32.0;
/// The row list's width in GUI pixels (`getListWidth`).
pub const VIDEO_LIST_WIDTH: f32 = 400.0;
/// One row's height in GUI pixels (the slot height 25).
pub const VIDEO_ROW_HEIGHT: f32 = 25.0;
/// The Done button's bottom inset in GUI pixels (Done stands at h-27).
pub const VIDEO_DONE_INSET: f32 = 27.0;
/// The FOV slider's floor (`FOV`, `30.0F`).
pub const FOV_MIN: f32 = 30.0;
/// The FOV slider's ceiling (`110.0F`).
pub const FOV_MAX: f32 = 110.0;
/// The FOV slider's step (`1.0F`).
pub const FOV_STEP: f32 = 1.0;
/// The default FOV: the Normal end of the label (`fovSetting = 70.0F`).
pub const FOV_NORMAL: u8 = 70;
/// The top FOV: the Quake Pro end of the label (`110.0F`).
pub const FOV_QUAKE_PRO: u8 = 110;
/// The render-distance slider's floor (`RENDER_DISTANCE`, `2.0F`).
pub const RENDER_DISTANCE_MIN: f32 = 2.0;
/// The render-distance slider's ceiling: the 64-bit constructor raises
/// the enum base 16 to 32 (`:195-204`; the rig leg confirms the shown
/// 32 — never re-derived here).
pub const RENDER_DISTANCE_MAX: f32 = 32.0;
/// The render-distance slider's step (`1.0F`).
pub const RENDER_DISTANCE_STEP: f32 = 1.0;
/// The framerate slider's floor (`FRAMERATE_LIMIT`, `10.0F`).
pub const FRAMERATE_MIN: f32 = 10.0;
/// The framerate slider's ceiling (`260.0F` — the top step reads
/// "Unlimited" and runs uncapped).
pub const FRAMERATE_MAX: f32 = 260.0;
/// The framerate slider's step (`10.0F`).
pub const FRAMERATE_STEP: f32 = 10.0;
/// The gamma slider's floor (continuous from 0).
pub const GAMMA_MIN: f32 = 0.0;
/// The gamma slider's ceiling (continuous to 1).
pub const GAMMA_MAX: f32 = 1.0;
/// The gamma slider's step: 0 is the continuous arm (`snapToStep`
/// passes the value through).
pub const GAMMA_STEP: f32 = 0.0;
/// The mipmap slider's floor (`MIPMAP_LEVELS`, `0.0F` — reads Off).
pub const MIPMAP_MIN: f32 = 0.0;
/// The mipmap slider's ceiling (`4.0F`).
pub const MIPMAP_MAX: f32 = 4.0;
/// The mipmap slider's step (`1.0F`).
pub const MIPMAP_STEP: f32 = 1.0;

/// When an answered press persists: toggles and cycles at once, sliders
/// at the next Done/screen-switch save (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveKind {
    /// The write persists at once (`setOptionValue`'s trailing save).
    Immediate,
    /// The write rides the debounced mark; the Done/screen-switch save
    /// flushes it (the slider writes never save).
    Deferred,
}

/// One action a hub press answers: the button's id routed through the
/// source's `actionPerformed` arms. Stream (107) and super-secret
/// (8675309) never answer — their buttons are not built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsAction {
    /// The FOV slider's press or drag motion wrote the store.
    FovChanged,
    /// The realms toggle cycled.
    RealmsToggled,
    /// Skin customization (the screen lands later; the press records the
    /// action and the owner saves first).
    Skin,
    /// The sound options (same placeholder shape).
    Sounds,
    /// The video screen: the owner saves first and stands it.
    Video,
    /// The controls screen (Task 19; same placeholder shape).
    Controls,
    /// The language screen (Task 26; same placeholder shape).
    Language,
    /// The chat options (same placeholder shape).
    Chat,
    /// The resource packs (the packs no-op is Task 26's).
    ResourcePacks,
    /// The snooper: the owner saves first and stands Task 17's screen.
    Snooper,
    /// Done: the owner saves and returns to the parent.
    Done,
}

impl OptionsAction {
    /// The press's save rule (see [`SaveKind`]): the FOV slider defers
    /// to Done; every other arm saves at once (each `displayGuiScreen`
    /// route and Done save first).
    pub fn save_kind(self) -> SaveKind {
        match self {
            OptionsAction::FovChanged => SaveKind::Deferred,
            _ => SaveKind::Immediate,
        }
    }
}

/// The hub's labels in build order — skin, sounds, video, controls,
/// language, chat, resource packs, snooper, Done — plus the title and
/// the two row-0 option names, through the handed-over language table:
/// the source's `I18n` lookup with its missing-key rule.
#[derive(Debug, Clone)]
pub struct OptionsLabels {
    /// The hub title (`options.title`).
    pub title: String,
    /// The skin button (`options.skinCustomisation`).
    pub skin: String,
    /// The sounds button (`options.sounds`).
    pub sounds: String,
    /// The video button (`options.video`).
    pub video: String,
    /// The controls button (`options.controls`).
    pub controls: String,
    /// The language button (`options.language`).
    pub language: String,
    /// The chat button (`options.chat.title`).
    pub chat: String,
    /// The resource-packs button (`options.resourcepack`).
    pub resourcepack: String,
    /// The snooper button (`options.snooper.view`).
    pub snooper: String,
    /// Done (`gui.done`).
    pub done: String,
    /// The FOV slider's name (`options.fov`).
    pub fov: String,
    /// The realms toggle's name (`options.realmsNotifications`).
    pub realms: String,
}

/// Resolves the hub's labels: with no table handed over, the English the
/// store's `en_US.lang` carries.
pub fn options_labels(language: Option<&LanguageTable>) -> OptionsLabels {
    const KEYS: [(&str, &str); 12] = [
        ("options.title", "Options"),
        ("options.skinCustomisation", "Skin Customization..."),
        ("options.sounds", "Music & Sounds..."),
        ("options.video", "Video Settings..."),
        ("options.controls", "Controls..."),
        ("options.language", "Language..."),
        ("options.chat.title", "Chat Settings..."),
        ("options.resourcepack", "Resource Packs..."),
        ("options.snooper.view", "Snooper Settings..."),
        ("gui.done", "Done"),
        ("options.fov", "FOV"),
        ("options.realmsNotifications", "Realms Notifications"),
    ];
    let look = |key: &str, english: &str| match language {
        None => english.to_string(),
        Some(table) => table.get(key).unwrap_or(key).to_string(),
    };
    OptionsLabels {
        title: look(KEYS[0].0, KEYS[0].1),
        skin: look(KEYS[1].0, KEYS[1].1),
        sounds: look(KEYS[2].0, KEYS[2].1),
        video: look(KEYS[3].0, KEYS[3].1),
        controls: look(KEYS[4].0, KEYS[4].1),
        language: look(KEYS[5].0, KEYS[5].1),
        chat: look(KEYS[6].0, KEYS[6].1),
        resourcepack: look(KEYS[7].0, KEYS[7].1),
        snooper: look(KEYS[8].0, KEYS[8].1),
        done: look(KEYS[9].0, KEYS[9].1),
        fov: look(KEYS[10].0, KEYS[10].1),
        realms: look(KEYS[11].0, KEYS[11].1),
    }
}

/// One centred line's draws: the text at `(w/2 - width/2, y)` in white
/// with the source's shadow.
fn centred_line(font: &Font, scaled: &ScaledResolution, text: &str, y: f32) -> HudDraw {
    let width = string_width(font, text);
    HudDraw::Text {
        text: text.to_string(),
        x: scaled.width as f32 / 2.0 - width as f32 / 2.0,
        y,
        scale: 1.0,
        colour: [1.0, 1.0, 1.0, 1.0],
        shadow: true,
        blend: true,
    }
}

/// The options hub: the FOV slider and the realms toggle row 0, then the
/// fixed navigation pairs and Done.
#[derive(Debug, Clone)]
pub struct OptionsScreen {
    /// The FOV slider row 0 left (`field_146440_f`'s loop).
    fov: OptionSlider,
    /// The no-world realms toggle row 0 right (`:81`).
    realms: OptionButton,
    /// The navigation pairs and Done, left column top-to-bottom, Done,
    /// then the right column.
    buttons: Vec<Button>,
    /// The labels the controls recompose after a write.
    labels: OptionsLabels,
}

impl OptionsScreen {
    /// Stands the hub at `scaled` on `settings` with `labels`: the FOV
    /// slider, the realms toggle, the navigation pairs and Done
    /// (`initGui`'s no-world build).
    pub fn new(scaled: &ScaledResolution, settings: &GameSettings, labels: OptionsLabels) -> Self {
        let half = scaled.width / 2;
        let row0 = scaled.height / 6 - 12;
        let fov = OptionSlider::new(
            (half - 155) as f32,
            row0 as f32,
            150.0,
            20.0,
            FOV_MIN,
            FOV_MAX,
            FOV_STEP,
            f32::from(settings.fov),
            fov_label(&labels.fov, settings.fov),
        );
        let realms = OptionButton::new(
            OPTIONS_REALMS_ID,
            (half + 5) as f32,
            row0 as f32,
            150.0,
            20.0,
            labels.realms.clone(),
            on_off(settings.realms_notifications),
        );
        let mut screen = Self {
            fov,
            realms,
            buttons: Vec::new(),
            labels,
        };
        screen.lay_buttons(scaled);
        screen
    }

    /// Lays the navigation pairs and Done at the scaled size: rows
    /// `h/6+48k-6` for the five left pairs, Done 200x20 at
    /// `w/2-100, h/6+168`, the right column's three pairs beside the
    /// middle rows.
    fn lay_buttons(&mut self, scaled: &ScaledResolution) {
        let half = scaled.width / 2;
        let base = scaled.height / 6;
        let left = (half - 155) as f32;
        let right = (half + 5) as f32;
        let row = |over: u32| (base + over - 6) as f32;
        self.buttons = vec![
            Button::new(
                OPTIONS_SKIN_ID,
                left,
                row(48),
                150.0,
                20.0,
                self.labels.skin.clone(),
            ),
            Button::new(
                OPTIONS_SOUNDS_ID,
                left,
                row(72),
                150.0,
                20.0,
                self.labels.sounds.clone(),
            ),
            Button::new(
                OPTIONS_VIDEO_ID,
                left,
                row(96),
                150.0,
                20.0,
                self.labels.video.clone(),
            ),
            Button::new(
                OPTIONS_LANGUAGE_ID,
                left,
                row(120),
                150.0,
                20.0,
                self.labels.language.clone(),
            ),
            Button::new(
                OPTIONS_RESOURCEPACK_ID,
                left,
                row(144),
                150.0,
                20.0,
                self.labels.resourcepack.clone(),
            ),
            Button::new(
                OPTIONS_DONE_ID,
                (half - 100) as f32,
                (base + 168) as f32,
                200.0,
                20.0,
                self.labels.done.clone(),
            ),
            Button::new(
                OPTIONS_CONTROLS_ID,
                right,
                row(96),
                150.0,
                20.0,
                self.labels.controls.clone(),
            ),
            Button::new(
                OPTIONS_CHAT_ID,
                right,
                row(120),
                150.0,
                20.0,
                self.labels.chat.clone(),
            ),
            Button::new(
                OPTIONS_SNOOPER_ID,
                right,
                row(144),
                150.0,
                20.0,
                self.labels.snooper.clone(),
            ),
        ];
    }

    /// The FOV slider.
    pub fn fov_slider(&self) -> &OptionSlider {
        &self.fov
    }

    /// The realms toggle.
    pub fn realms_button(&self) -> &OptionButton {
        &self.realms
    }

    /// The navigation pairs and Done.
    pub fn buttons(&self) -> &[Button] {
        &self.buttons
    }

    /// Re-lays the hub at the new scaled size (`initGui` re-runs on
    /// resize: the button list is cleared and rebuilt); the option
    /// controls reseat at the stored values.
    pub fn set_screen_size(&mut self, scaled: &ScaledResolution, settings: &GameSettings) {
        let half = scaled.width / 2;
        let row0 = scaled.height / 6 - 12;
        self.fov = OptionSlider::new(
            (half - 155) as f32,
            row0 as f32,
            150.0,
            20.0,
            FOV_MIN,
            FOV_MAX,
            FOV_STEP,
            f32::from(settings.fov),
            fov_label(&self.labels.fov, settings.fov),
        );
        self.realms = OptionButton::new(
            OPTIONS_REALMS_ID,
            (half + 5) as f32,
            row0 as f32,
            150.0,
            20.0,
            self.labels.realms.clone(),
            on_off(settings.realms_notifications),
        );
        self.lay_buttons(scaled);
    }

    /// One press at the GUI-space point: the FOV slider folds the pointer
    /// into the store, the realms toggle cycles, a navigation button
    /// answers its action (`mouseClicked` fires on press). Empty space
    /// answers nothing.
    pub fn click_at(
        &mut self,
        point: (f32, f32),
        settings: &mut GameSettings,
    ) -> Option<OptionsAction> {
        if self.fov.press(point).is_some() {
            settings.fov = self.fov.value().clamp(FOV_MIN, FOV_MAX) as u8;
            self.fov
                .set_label(fov_label(&self.labels.fov, settings.fov));
            return Some(OptionsAction::FovChanged);
        }
        if self.realms.press(point) {
            settings.realms_notifications = !settings.realms_notifications;
            self.realms.set_value(on_off(settings.realms_notifications));
            return Some(OptionsAction::RealmsToggled);
        }
        let button = self.buttons.iter().find(|button| button.pressed(point))?;
        match button.id {
            OPTIONS_SKIN_ID => Some(OptionsAction::Skin),
            OPTIONS_SOUNDS_ID => Some(OptionsAction::Sounds),
            OPTIONS_VIDEO_ID => Some(OptionsAction::Video),
            OPTIONS_CONTROLS_ID => Some(OptionsAction::Controls),
            OPTIONS_LANGUAGE_ID => Some(OptionsAction::Language),
            OPTIONS_CHAT_ID => Some(OptionsAction::Chat),
            OPTIONS_RESOURCEPACK_ID => Some(OptionsAction::ResourcePacks),
            OPTIONS_SNOOPER_ID => Some(OptionsAction::Snooper),
            OPTIONS_DONE_ID => Some(OptionsAction::Done),
            _ => None,
        }
    }

    /// One drag motion at the pointer's x: the latched FOV slider writes
    /// again per event (`mouseDragged` writes through
    /// `setOptionFloatValue` on every motion).
    pub fn drag_to(&mut self, mouse_x: f32, settings: &mut GameSettings) -> Option<OptionsAction> {
        if self.fov.drag_to(mouse_x).is_some() {
            settings.fov = self.fov.value().clamp(FOV_MIN, FOV_MAX) as u8;
            self.fov
                .set_label(fov_label(&self.labels.fov, settings.fov));
            return Some(OptionsAction::FovChanged);
        }
        None
    }

    /// One release anywhere: the slider latch clears (`mouseReleased`).
    pub fn release(&mut self) {
        self.fov.release();
    }

    /// The hub's content draws in painter's order: the centred "Options"
    /// title at `(w/2, 15)`, the FOV slider, the realms toggle, then the
    /// navigation pairs and Done (`drawScreen`, then `super.drawScreen`).
    /// The dirt tiles and the world-present gradient ride the frame's own
    /// assembly beneath these.
    pub fn draws(
        &self,
        font: &Font,
        scaled: &ScaledResolution,
        mouse: Option<(f32, f32)>,
    ) -> Vec<HudDraw> {
        let mut draws = vec![centred_line(
            font,
            scaled,
            &self.labels.title,
            OPTIONS_TITLE_Y,
        )];
        draws.extend(self.fov.draws(font, mouse));
        draws.extend(self.realms.draws(font, mouse));
        for button in &self.buttons {
            draws.extend(button.draws(font, mouse));
        }
        draws
    }
}

/// One video row option, in the source's fixed order
/// (`GuiVideoSettings.java:16`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoOption {
    /// The graphics cycle (Fancy/Fast).
    Graphics,
    /// The render-distance slider (2–32, "N chunks").
    RenderDistance,
    /// The smooth-lighting cycle (OFF/Minimum/Maximum).
    AmbientOcclusion,
    /// The framerate slider (10–260, "N fps"/"Unlimited").
    FramerateLimit,
    /// The anaglyph toggle.
    Anaglyph,
    /// The view-bobbing toggle.
    ViewBobbing,
    /// The GUI-scale cycle (Auto/Small/Normal/Large).
    GuiScale,
    /// The brightness slider (Moody/+N%/Bright).
    Gamma,
    /// The clouds cycle (OFF/Fast/Fancy).
    RenderClouds,
    /// The particles cycle (All/Decreased/Minimal).
    Particles,
    /// The fullscreen toggle.
    UseFullscreen,
    /// The VSync toggle.
    EnableVsync,
    /// The mipmap slider (OFF/N).
    MipmapLevels,
    /// The alternate-blocks toggle.
    BlockAlternatives,
    /// The VBO toggle (drops without VBO support).
    UseVbo,
    /// The entity-shadows toggle (drops with it).
    EntityShadows,
}

/// The video screen's sixteen rows in source order.
pub const VIDEO_OPTIONS: [VideoOption; 16] = [
    VideoOption::Graphics,
    VideoOption::RenderDistance,
    VideoOption::AmbientOcclusion,
    VideoOption::FramerateLimit,
    VideoOption::Anaglyph,
    VideoOption::ViewBobbing,
    VideoOption::GuiScale,
    VideoOption::Gamma,
    VideoOption::RenderClouds,
    VideoOption::Particles,
    VideoOption::UseFullscreen,
    VideoOption::EnableVsync,
    VideoOption::MipmapLevels,
    VideoOption::BlockAlternatives,
    VideoOption::UseVbo,
    VideoOption::EntityShadows,
];

/// The row options at the VBO support: without it the copy loop breaks
/// BEFORE USE_VBO (`:34-51`), so USE_VBO and ENTITY_SHADOWS drop with it.
/// (The source pads the truncated array with one trailing null row; the
/// port builds no null buttons, so the empty row drops too — recorded.)
pub fn video_row_options(vbo_supported: bool) -> Vec<VideoOption> {
    if vbo_supported {
        VIDEO_OPTIONS.to_vec()
    } else {
        VIDEO_OPTIONS
            .iter()
            .take_while(|option| **option != VideoOption::UseVbo)
            .copied()
            .collect()
    }
}

/// Whether the option rides a slider (`getEnumFloat`): render distance,
/// framerate, gamma and mipmaps. Every other row is a cycle button (the
/// row's `func_148182_a` builds `GuiOptionSlider` for float options,
/// `GuiOptionButton` otherwise).
pub fn video_option_is_slider(option: VideoOption) -> bool {
    matches!(
        option,
        VideoOption::RenderDistance
            | VideoOption::FramerateLimit
            | VideoOption::Gamma
            | VideoOption::MipmapLevels
    )
}

/// One action a video press answers: the row's control routed through
/// the row list's `mousePressed` arms (cycles and toggles write through
/// `setOptionValue`, sliders through `setOptionFloatValue`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoAction {
    /// The graphics press cycled Fancy/Fast.
    GraphicsCycled,
    /// A render-distance press or drag motion wrote the store.
    RenderDistanceChanged,
    /// The smooth-lighting press cycled OFF/Minimum/Maximum.
    AmbientOcclusionCycled,
    /// A framerate press or drag motion wrote the store.
    FramerateChanged,
    /// The anaglyph press toggled.
    AnaglyphToggled,
    /// The view-bobbing press toggled.
    ViewBobbingToggled,
    /// The GUI-scale press cycled: the owner re-lays the screen after
    /// the apply (see [`VideoAction::relayout`]).
    GuiScaleCycled,
    /// A brightness press or drag motion wrote the store.
    GammaChanged,
    /// The clouds press cycled OFF/Fast/Fancy.
    CloudsCycled,
    /// The particles press cycled All/Decreased/Minimal.
    ParticlesCycled,
    /// The fullscreen press toggled.
    FullscreenToggled,
    /// The VSync press toggled.
    VsyncToggled,
    /// A mipmap press or drag motion wrote the store.
    MipmapChanged,
    /// The alternate-blocks press toggled.
    BlockAlternativesToggled,
    /// The VBO press toggled.
    VboToggled,
    /// The entity-shadows press toggled.
    EntityShadowsToggled,
    /// Done: the owner saves and returns to the hub.
    Done,
}

impl VideoAction {
    /// The press's save rule (see [`SaveKind`]): the four sliders defer
    /// to Done; every cycle, toggle and Done saves at once.
    pub fn save_kind(self) -> SaveKind {
        match self {
            VideoAction::RenderDistanceChanged
            | VideoAction::FramerateChanged
            | VideoAction::GammaChanged
            | VideoAction::MipmapChanged => SaveKind::Deferred,
            _ => SaveKind::Immediate,
        }
    }

    /// Whether the press re-lays the screen: only the GUI-scale cycle
    /// re-runs `setWorldAndResolution` after click AND release
    /// (`:85-116`) — apply owns the re-layout, so the action carries
    /// the notice and the owner rebuilds the rows after the apply.
    pub fn relayout(self) -> bool {
        matches!(self, VideoAction::GuiScaleCycled)
    }
}

/// The video screen's labels — the sixteen option names plus the title
/// and Done — through the handed-over language table.
#[derive(Debug, Clone)]
pub struct VideoLabels {
    /// The screen title (`options.videoTitle`).
    pub title: String,
    /// Done (`gui.done`).
    pub done: String,
    /// The graphics name (`options.graphics`).
    pub graphics: String,
    /// The render-distance name (`options.renderDistance`).
    pub render_distance: String,
    /// The smooth-lighting name (`options.ao`).
    pub ambient_occlusion: String,
    /// The framerate name (`options.framerateLimit`).
    pub framerate_limit: String,
    /// The anaglyph name (`options.anaglyph`).
    pub anaglyph: String,
    /// The view-bobbing name (`options.viewBobbing`).
    pub view_bobbing: String,
    /// The GUI-scale name (`options.guiScale`).
    pub gui_scale: String,
    /// The brightness name (`options.gamma`).
    pub gamma: String,
    /// The clouds name (`options.renderClouds`).
    pub render_clouds: String,
    /// The particles name (`options.particles`).
    pub particles: String,
    /// The fullscreen name (`options.fullscreen`).
    pub fullscreen: String,
    /// The VSync name (`options.vsync`).
    pub vsync: String,
    /// The mipmap name (`options.mipmapLevels`).
    pub mipmap_levels: String,
    /// The alternate-blocks name (`options.blockAlternatives`).
    pub block_alternatives: String,
    /// The VBO name (`options.vbo`).
    pub use_vbo: String,
    /// The entity-shadows name (`options.entityShadows`).
    pub entity_shadows: String,
}

/// Resolves the video screen's labels: with no table handed over, the
/// English the store's `en_US.lang` carries.
pub fn video_labels(language: Option<&LanguageTable>) -> VideoLabels {
    let look = |key: &str, english: &str| match language {
        None => english.to_string(),
        Some(table) => table.get(key).unwrap_or(key).to_string(),
    };
    VideoLabels {
        title: look("options.videoTitle", "Video Settings"),
        done: look("gui.done", "Done"),
        graphics: look("options.graphics", "Graphics"),
        render_distance: look("options.renderDistance", "Render Distance"),
        ambient_occlusion: look("options.ao", "Smooth Lighting"),
        framerate_limit: look("options.framerateLimit", "Max Framerate"),
        anaglyph: look("options.anaglyph", "3D Anaglyph"),
        view_bobbing: look("options.viewBobbing", "View Bobbing"),
        gui_scale: look("options.guiScale", "GUI Scale"),
        gamma: look("options.gamma", "Brightness"),
        render_clouds: look("options.renderClouds", "Clouds"),
        particles: look("options.particles", "Particles"),
        fullscreen: look("options.fullscreen", "Fullscreen"),
        vsync: look("options.vsync", "Use VSync"),
        mipmap_levels: look("options.mipmapLevels", "Mipmap Levels"),
        block_alternatives: look("options.blockAlternatives", "Alternate Blocks"),
        use_vbo: look("options.vbo", "Use VBOs"),
        entity_shadows: look("options.entityShadows", "Entity Shadows"),
    }
}

/// The ON/OFF word the boolean rows read (`options.on`, `options.off`).
fn on_off(on: bool) -> String {
    if on {
        "ON".to_string()
    } else {
        "OFF".to_string()
    }
}

/// The `"name: value"` composition the rows draw (the `getKeyBinding`
/// prefix rule).
fn named(name: &str, value: &str) -> String {
    format!("{name}: {value}")
}

/// The FOV label: 70 reads Normal (`options.fov.min`), 110 reads Quake
/// Pro (`options.fov.max`), else the degrees (`getKeyBinding`'s FOV arm).
pub fn fov_label(name: &str, fov: u8) -> String {
    if fov == FOV_NORMAL {
        named(name, "Normal")
    } else if fov == FOV_QUAKE_PRO {
        named(name, "Quake Pro")
    } else {
        named(name, &fov.to_string())
    }
}

/// The render-distance label: `"N chunks"` (`getKeyBinding`'s
/// RENDER_DISTANCE arm).
pub fn render_distance_label(name: &str, chunks: u8) -> String {
    named(name, &format!("{chunks} chunks"))
}

/// The framerate label: the sentinel reads "Unlimited"
/// (`options.framerateLimit.max`), else `"N fps"`.
pub fn framerate_label(name: &str, cap: u8) -> String {
    if cap == oxide_game::settings::FRAMERATE_UNLIMITED {
        named(name, "Unlimited")
    } else {
        named(name, &format!("{cap} fps"))
    }
}

/// Normalizes the stored framerate cap into the 0..1 slider position:
/// the sentinel sits at the top; every other cap runs the option law
/// over 10–260 step 10.
pub fn framerate_normalize(cap: u8) -> f32 {
    if cap == oxide_game::settings::FRAMERATE_UNLIMITED {
        1.0
    } else {
        normalize_option(f32::from(cap), FRAMERATE_MIN, FRAMERATE_MAX, FRAMERATE_STEP)
    }
}

/// Denormalizes the 0..1 slider position into the stored framerate cap:
/// the top step folds into the sentinel (260 exceeds `u8`, so the store
/// carries numeric 10–250 plus 255 — the settings-parity pin).
pub fn framerate_denormalize(position: f32) -> u8 {
    let raw = denormalize_option(position, FRAMERATE_MIN, FRAMERATE_MAX, FRAMERATE_STEP);
    if raw >= FRAMERATE_MAX {
        oxide_game::settings::FRAMERATE_UNLIMITED
    } else {
        raw as u8
    }
}

/// The brightness label: 0 reads Moody (`options.gamma.min`), 100 reads
/// Bright (`options.gamma.max`), else `"+N%"` (the GAMMA arm reads the
/// normalized `f`, so the percent is the store's own).
pub fn gamma_label(name: &str, brightness: u8) -> String {
    if brightness == 0 {
        named(name, "Moody")
    } else if brightness == 100 {
        named(name, "Bright")
    } else {
        named(name, &format!("+{brightness}%"))
    }
}

/// Normalizes the stored brightness (percent) into the continuous 0..1
/// gamma position.
pub fn gamma_normalize(brightness: u8) -> f32 {
    f32::from(brightness) / 100.0
}

/// Denormalizes the 0..1 gamma position into the stored brightness
/// percent.
pub fn gamma_denormalize(position: f32) -> u8 {
    (position.clamp(0.0, 1.0) * 100.0).round() as u8
}

/// The mipmap label: 0 reads "OFF" (`options.off`), else the level.
pub fn mipmap_label(name: &str, levels: u8) -> String {
    if levels == 0 {
        named(name, "OFF")
    } else {
        named(name, &levels.to_string())
    }
}

/// The graphics label: Fancy/Fast (`options.graphics.fancy/fast`).
pub fn graphics_label(name: &str, fancy: bool) -> String {
    named(name, if fancy { "Fancy" } else { "Fast" })
}

/// The smooth-lighting label: OFF/Minimum/Maximum (`options.ao.*`).
pub fn ambient_occlusion_label(name: &str, mode: u8) -> String {
    named(
        name,
        match mode {
            1 => "Minimum",
            2 => "Maximum",
            _ => "OFF",
        },
    )
}

/// The GUI-scale label: Auto/Small/Normal/Large (`options.guiScale.*`).
pub fn gui_scale_label(name: &str, scale: u8) -> String {
    named(
        name,
        match scale {
            1 => "Small",
            2 => "Normal",
            3 => "Large",
            _ => "Auto",
        },
    )
}

/// The clouds label: OFF/Fast/Fancy (`CLOUDS_TYPES` reads `options.off`,
/// `options.graphics.fast`, `options.graphics.fancy`).
pub fn clouds_label(name: &str, clouds: CloudSetting) -> String {
    named(
        name,
        match clouds {
            CloudSetting::Off => "OFF",
            CloudSetting::Fast => "Fast",
            CloudSetting::Fancy => "Fancy",
        },
    )
}

/// The particles label: All/Decreased/Minimal (`options.particles.*`).
pub fn particles_label(name: &str, particles: ParticleSetting) -> String {
    named(
        name,
        match particles {
            ParticleSetting::All => "All",
            ParticleSetting::Decreased => "Decreased",
            ParticleSetting::Minimal => "Minimal",
        },
    )
}

/// The boolean label: ON/OFF (`options.on/off`).
pub fn toggle_label(name: &str, on: bool) -> String {
    named(name, &on_off(on))
}

/// Cycles the GUI scale: `guiScale + v & 3` — the source's own
/// precedence (addition binds tighter than the mask), so +1 wraps 3 to
/// 0 (`setOptionValue`'s GUI_SCALE arm).
pub fn cycle_gui_scale(scale: u8) -> u8 {
    (scale + 1) & 3
}

/// Cycles the clouds: `+v % 3` in OFF/Fast/Fancy order.
pub fn cycle_clouds(clouds: CloudSetting) -> CloudSetting {
    match clouds {
        CloudSetting::Off => CloudSetting::Fast,
        CloudSetting::Fast => CloudSetting::Fancy,
        CloudSetting::Fancy => CloudSetting::Off,
    }
}

/// Cycles the particles: `+v % 3` in All/Decreased/Minimal order.
pub fn cycle_particles(particles: ParticleSetting) -> ParticleSetting {
    match particles {
        ParticleSetting::All => ParticleSetting::Decreased,
        ParticleSetting::Decreased => ParticleSetting::Minimal,
        ParticleSetting::Minimal => ParticleSetting::All,
    }
}

/// Cycles the smooth lighting: `+v % 3` in OFF/Minimum/Maximum order.
pub fn cycle_ambient_occlusion(mode: u8) -> u8 {
    (mode + 1) % 3
}

/// One video row's control: the slider for the four float options, the
/// cycle button for the rest (the row's `func_148182_a` builds by
/// `getEnumFloat`).
#[derive(Debug, Clone)]
pub enum VideoControl {
    /// A slider control with its option.
    Slider(VideoOption, OptionSlider),
    /// A cycle/toggle button with its option.
    Button(VideoOption, OptionButton),
}

impl VideoControl {
    /// The row's option.
    pub fn option(&self) -> VideoOption {
        match self {
            VideoControl::Slider(option, _) | VideoControl::Button(option, _) => *option,
        }
    }

    /// The left edge in GUI pixels.
    pub fn x(&self) -> f32 {
        match self {
            VideoControl::Slider(_, slider) => slider.x(),
            VideoControl::Button(_, button) => button.x(),
        }
    }

    /// The top edge in GUI pixels.
    pub fn y(&self) -> f32 {
        match self {
            VideoControl::Slider(_, slider) => slider.y(),
            VideoControl::Button(_, button) => button.y(),
        }
    }

    /// The width in GUI pixels.
    pub fn w(&self) -> f32 {
        match self {
            VideoControl::Slider(_, slider) => slider.w(),
            VideoControl::Button(_, button) => button.w(),
        }
    }
}

/// The video settings screen: the row pairs over the option list, the
/// scroll offset and Done.
#[derive(Debug, Clone)]
pub struct VideoScreen {
    /// The row pairs, two controls per row in source order.
    rows: Vec<(VideoControl, VideoControl)>,
    /// The list's scroll offset in GUI pixels, `0..=max_scroll`.
    scroll: f32,
    /// Done 200x20 (`initGui`'s button).
    done: Button,
    /// The labels the rows recompose after a write.
    labels: VideoLabels,
    /// Whether VBOs are supported (the truncation arm).
    vbo_supported: bool,
    /// The scaled height the list clips against.
    height: u32,
}

impl VideoScreen {
    /// Stands the video screen at `scaled` on `settings` with `labels`:
    /// the row pairs at the list top, the scroll home, Done at `h-27`
    /// (`initGui`).
    pub fn new(
        scaled: &ScaledResolution,
        settings: &GameSettings,
        labels: VideoLabels,
        vbo_supported: bool,
    ) -> Self {
        let mut screen = Self {
            rows: Vec::new(),
            scroll: 0.0,
            done: Button::new(
                OPTIONS_DONE_ID,
                (scaled.width / 2 - 100) as f32,
                (scaled.height - 27) as f32,
                200.0,
                20.0,
                labels.done.clone(),
            ),
            labels,
            vbo_supported,
            height: scaled.height,
        };
        screen.lay_rows(scaled, settings);
        screen
    }

    /// Builds one row's control at `(x, y)` on the stored value.
    fn build_control(
        labels: &VideoLabels,
        option: VideoOption,
        settings: &GameSettings,
        x: f32,
        y: f32,
    ) -> VideoControl {
        match option {
            VideoOption::RenderDistance => VideoControl::Slider(
                option,
                OptionSlider::new(
                    x,
                    y,
                    150.0,
                    20.0,
                    RENDER_DISTANCE_MIN,
                    RENDER_DISTANCE_MAX,
                    RENDER_DISTANCE_STEP,
                    f32::from(settings.render_distance),
                    render_distance_label(&labels.render_distance, settings.render_distance),
                ),
            ),
            VideoOption::FramerateLimit => VideoControl::Slider(
                option,
                OptionSlider::new(
                    x,
                    y,
                    150.0,
                    20.0,
                    FRAMERATE_MIN,
                    FRAMERATE_MAX,
                    FRAMERATE_STEP,
                    framerate_denormalize(framerate_normalize(settings.max_framerate)) as f32,
                    framerate_label(&labels.framerate_limit, settings.max_framerate),
                ),
            ),
            VideoOption::Gamma => VideoControl::Slider(
                option,
                OptionSlider::new(
                    x,
                    y,
                    150.0,
                    20.0,
                    GAMMA_MIN,
                    GAMMA_MAX,
                    GAMMA_STEP,
                    gamma_normalize(settings.brightness),
                    gamma_label(&labels.gamma, settings.brightness),
                ),
            ),
            VideoOption::MipmapLevels => VideoControl::Slider(
                option,
                OptionSlider::new(
                    x,
                    y,
                    150.0,
                    20.0,
                    MIPMAP_MIN,
                    MIPMAP_MAX,
                    MIPMAP_STEP,
                    f32::from(settings.mipmap_levels),
                    mipmap_label(&labels.mipmap_levels, settings.mipmap_levels),
                ),
            ),
            _ => VideoControl::Button(
                option,
                OptionButton::new(
                    option_button_id(option),
                    x,
                    y,
                    150.0,
                    20.0,
                    option_name(labels, option),
                    option_value(option, settings),
                ),
            ),
        }
    }

    /// Lays the row pairs at the scaled size: two 150x20 per 25 px slot
    /// from the list top (`GuiOptionsRowList`'s `w/2-155`, `+160`
    /// columns); the scroll resets home like a fresh `initGui`.
    fn lay_rows(&mut self, scaled: &ScaledResolution, settings: &GameSettings) {
        let half = scaled.width / 2;
        let left = (half - 155) as f32;
        let right = (half - 155 + 160) as f32;
        self.height = scaled.height;
        self.rows = video_row_options(self.vbo_supported)
            .chunks(2)
            .enumerate()
            .map(|(row, pair)| {
                let y = VIDEO_LIST_TOP + row as f32 * VIDEO_ROW_HEIGHT;
                let right_option = if pair.len() > 1 { pair[1] } else { pair[0] };
                (
                    Self::build_control(&self.labels, pair[0], settings, left, y),
                    Self::build_control(&self.labels, right_option, settings, right, y),
                )
            })
            .collect();
        self.scroll = 0.0;
        self.done = Button::new(
            OPTIONS_DONE_ID,
            (scaled.width / 2 - 100) as f32,
            (scaled.height - 27) as f32,
            200.0,
            20.0,
            self.labels.done.clone(),
        );
    }

    /// The row pairs.
    pub fn rows(&self) -> &[(VideoControl, VideoControl)] {
        &self.rows
    }

    /// The row count: sixteen rows pair into eight.
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// One row's pair (left, right).
    pub fn row_pair(&self, row: usize) -> (&VideoControl, &VideoControl) {
        let pair = &self.rows[row];
        (&pair.0, &pair.1)
    }

    /// The list's top edge (see [`VIDEO_LIST_TOP`]).
    pub fn list_top(&self) -> f32 {
        VIDEO_LIST_TOP
    }

    /// The list's bottom edge at the scaled size (h-32).
    pub fn list_bottom(&self, scaled: &ScaledResolution) -> f32 {
        scaled.height as f32 - VIDEO_LIST_BOTTOM_INSET
    }

    /// The list's width (see [`VIDEO_LIST_WIDTH`]).
    pub fn list_width(&self) -> f32 {
        VIDEO_LIST_WIDTH
    }

    /// One row's height (see [`VIDEO_ROW_HEIGHT`]).
    pub fn row_height(&self) -> f32 {
        VIDEO_ROW_HEIGHT
    }

    /// Done's rect `(x, y, w, h)` at the scaled size.
    pub fn done_rect(&self, scaled: &ScaledResolution) -> (f32, f32, f32, f32) {
        (
            (scaled.width / 2 - 100) as f32,
            (scaled.height - 27) as f32,
            200.0,
            20.0,
        )
    }

    /// The list's scroll offset in GUI pixels.
    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    /// The scroll ceiling at the scaled size: the content past the
    /// window, floored at zero (a fitting list never scrolls).
    pub fn max_scroll(&self, scaled: &ScaledResolution) -> f32 {
        (self.rows.len() as f32 * VIDEO_ROW_HEIGHT - (self.list_bottom(scaled) - VIDEO_LIST_TOP))
            .max(0.0)
    }

    /// One wheel notch: one slot per notch, clamped into the window (the
    /// list's own scroll; the source scrolls by slot the same way). Every
    /// control re-seats at its scrolled row, so the hit-tests and the
    /// draws read live rects.
    pub fn wheel(&mut self, scaled: &ScaledResolution, notch: f32) {
        let ceiling = self.max_scroll(scaled);
        self.scroll = (self.scroll - notch.clamp(-1.0, 1.0) * VIDEO_ROW_HEIGHT).clamp(0.0, ceiling);
        for (row, pair) in self.rows.iter_mut().enumerate() {
            let y = VIDEO_LIST_TOP + row as f32 * VIDEO_ROW_HEIGHT - self.scroll;
            for control in [&mut pair.0, &mut pair.1] {
                match control {
                    VideoControl::Slider(_, slider) => slider.set_pos(slider.x(), y),
                    VideoControl::Button(_, button) => button.set_pos(button.x(), y),
                }
            }
        }
    }

    /// One row's top edge at the scaled size, scroll applied.
    pub fn row_y(&self, row: usize) -> f32 {
        VIDEO_LIST_TOP + row as f32 * VIDEO_ROW_HEIGHT - self.scroll
    }

    /// Re-lays the rows at the new scaled size (`initGui` re-runs on
    /// resize): the pairs rebuild at the stored values and the scroll
    /// resets home. The GUI-scale cycle calls this after its apply.
    pub fn set_screen_size(&mut self, scaled: &ScaledResolution, settings: &GameSettings) {
        self.lay_rows(scaled, settings);
    }

    /// Recomposes one control's label after a write (`displayString`'s
    /// re-read) and reseats a slider at the written value (the
    /// `mouseDragged` re-read).
    fn refresh(&mut self, option: VideoOption, settings: &GameSettings) {
        for pair in &mut self.rows {
            for control in [&mut pair.0, &mut pair.1] {
                if control.option() != option {
                    continue;
                }
                match control {
                    VideoControl::Slider(_, slider) => {
                        slider.set_value(slider_value(option, settings));
                        slider.set_label(option_label(&self.labels, option, settings));
                    }
                    VideoControl::Button(_, button) => {
                        button.set_value(option_value(option, settings));
                    }
                }
            }
        }
    }

    /// Writes one slider's motion into the store: the press and each
    /// drag motion answer the row's action (`setOptionFloatValue` per
    /// event).
    fn write_slider(
        settings: &mut GameSettings,
        option: VideoOption,
        position: f32,
    ) -> VideoAction {
        match option {
            VideoOption::RenderDistance => {
                settings.render_distance = denormalize_option(
                    position,
                    RENDER_DISTANCE_MIN,
                    RENDER_DISTANCE_MAX,
                    RENDER_DISTANCE_STEP,
                ) as u8;
                VideoAction::RenderDistanceChanged
            }
            VideoOption::FramerateLimit => {
                settings.max_framerate = framerate_denormalize(position);
                VideoAction::FramerateChanged
            }
            VideoOption::Gamma => {
                settings.brightness = gamma_denormalize(position);
                VideoAction::GammaChanged
            }
            _ => {
                settings.mipmap_levels =
                    denormalize_option(position, MIPMAP_MIN, MIPMAP_MAX, MIPMAP_STEP) as u8;
                VideoAction::MipmapChanged
            }
        }
    }

    /// Cycles one button's option in the store (`setOptionValue`'s arms)
    /// and answers the row's action.
    fn write_button(settings: &mut GameSettings, option: VideoOption) -> VideoAction {
        match option {
            VideoOption::Graphics => {
                settings.graphics = if settings.graphics.as_bool() {
                    GraphicsSetting::Fast
                } else {
                    GraphicsSetting::Fancy
                };
                VideoAction::GraphicsCycled
            }
            VideoOption::AmbientOcclusion => {
                settings.smooth_lighting = cycle_ambient_occlusion(settings.smooth_lighting);
                VideoAction::AmbientOcclusionCycled
            }
            VideoOption::Anaglyph => {
                settings.anaglyph = !settings.anaglyph;
                VideoAction::AnaglyphToggled
            }
            VideoOption::ViewBobbing => {
                settings.view_bobbing = !settings.view_bobbing;
                VideoAction::ViewBobbingToggled
            }
            VideoOption::GuiScale => {
                settings.gui_scale = cycle_gui_scale(settings.gui_scale);
                VideoAction::GuiScaleCycled
            }
            VideoOption::RenderClouds => {
                settings.clouds = cycle_clouds(settings.clouds);
                VideoAction::CloudsCycled
            }
            VideoOption::Particles => {
                settings.particles = cycle_particles(settings.particles);
                VideoAction::ParticlesCycled
            }
            VideoOption::UseFullscreen => {
                settings.fullscreen = !settings.fullscreen;
                VideoAction::FullscreenToggled
            }
            VideoOption::EnableVsync => {
                settings.vsync = !settings.vsync;
                VideoAction::VsyncToggled
            }
            VideoOption::BlockAlternatives => {
                settings.alternate_blocks = !settings.alternate_blocks;
                VideoAction::BlockAlternativesToggled
            }
            VideoOption::UseVbo => {
                settings.use_vbos = !settings.use_vbos;
                VideoAction::VboToggled
            }
            _ => {
                settings.entity_shadows = !settings.entity_shadows;
                VideoAction::EntityShadowsToggled
            }
        }
    }

    /// One press at the GUI-space point: Done answers first, then the
    /// visible rows' controls top-to-bottom (`mouseClicked` fires on
    /// press; clipped rows never press). Empty space answers nothing.
    pub fn click_at(
        &mut self,
        point: (f32, f32),
        settings: &mut GameSettings,
    ) -> Option<VideoAction> {
        if self.done.pressed(point) {
            return Some(VideoAction::Done);
        }
        let bottom = self.height as f32 - VIDEO_LIST_BOTTOM_INSET;
        for row in 0..self.rows.len() {
            let y = self.row_y(row);
            if y + 20.0 <= VIDEO_LIST_TOP || y >= bottom {
                continue;
            }
            for side in 0..2 {
                let control = if side == 0 {
                    &mut self.rows[row].0
                } else {
                    &mut self.rows[row].1
                };
                let option = control.option();
                if !control_hit(&mut *control, point) {
                    continue;
                }
                if video_option_is_slider(option) {
                    let position = slider_press(control, point)?;
                    let action = Self::write_slider(&mut *settings, option, position);
                    self.refresh(option, &*settings);
                    return Some(action);
                }
                press_button(control, point);
                let action = Self::write_button(&mut *settings, option);
                self.refresh(option, &*settings);
                return Some(action);
            }
        }
        None
    }

    /// One drag motion at the pointer's x: every latched slider writes
    /// again per event and answers its row's action.
    pub fn drag_to(&mut self, mouse_x: f32, settings: &mut GameSettings) -> Option<VideoAction> {
        for row in 0..self.rows.len() {
            for side in 0..2 {
                let (option, position) = {
                    let control = if side == 0 {
                        &mut self.rows[row].0
                    } else {
                        &mut self.rows[row].1
                    };
                    let VideoControl::Slider(option, slider) = control else {
                        continue;
                    };
                    let Some(_) = slider.drag_to(mouse_x) else {
                        continue;
                    };
                    (*option, slider.position())
                };
                let action = Self::write_slider(&mut *settings, option, position);
                self.refresh(option, &*settings);
                return Some(action);
            }
        }
        None
    }

    /// One release anywhere: every slider latch clears (`mouseReleased`).
    pub fn release(&mut self) {
        for pair in &mut self.rows {
            for control in [&mut pair.0, &mut pair.1] {
                if let VideoControl::Slider(_, slider) = control {
                    slider.release();
                }
            }
        }
    }

    /// The video screen's content draws in painter's order: the centred
    /// title at `(w/2, 5)`, the visible rows' controls, Done
    /// (`drawScreen`, then `super.drawScreen`). Rows fully outside the
    /// list window stay undrawn (the list's own clip); the dirt tiles
    /// and the world-present gradient ride the frame's own assembly
    /// beneath these.
    pub fn draws(
        &self,
        font: &Font,
        scaled: &ScaledResolution,
        mouse: Option<(f32, f32)>,
    ) -> Vec<HudDraw> {
        let mut draws = vec![centred_line(
            font,
            scaled,
            &self.labels.title,
            VIDEO_TITLE_Y,
        )];
        let bottom = self.list_bottom(scaled);
        for (row, pair) in self.rows.iter().enumerate() {
            let y = self.row_y(row);
            if y + 20.0 <= VIDEO_LIST_TOP || y >= bottom {
                continue;
            }
            for control in [&pair.0, &pair.1] {
                match control {
                    VideoControl::Slider(_, slider) => draws.extend(slider.draws(font, mouse)),
                    VideoControl::Button(_, button) => draws.extend(button.draws(font, mouse)),
                }
            }
        }
        draws.extend(self.done.draws(font, mouse));
        draws
    }
}

/// One cycle button's id: the option's source ordinal is the button's
/// own only where the source builds one — the rows answer typed actions
/// port-side, so the id only needs to pin the control apart. The hub's
/// fixed ids stay with the hub.
fn option_button_id(option: VideoOption) -> i32 {
    match option {
        VideoOption::Graphics => 11,
        VideoOption::AmbientOcclusion => 12,
        VideoOption::GuiScale => 13,
        VideoOption::Particles => 14,
        VideoOption::RenderClouds => 10,
        VideoOption::Anaglyph => 7,
        VideoOption::ViewBobbing => 6,
        VideoOption::UseFullscreen => 21,
        VideoOption::EnableVsync => 22,
        VideoOption::UseVbo => 23,
        VideoOption::BlockAlternatives => 41,
        VideoOption::EntityShadows => 43,
        _ => -1,
    }
}

/// One cycle button's name.
fn option_name(labels: &VideoLabels, option: VideoOption) -> String {
    match option {
        VideoOption::Graphics => labels.graphics.clone(),
        VideoOption::AmbientOcclusion => labels.ambient_occlusion.clone(),
        VideoOption::Anaglyph => labels.anaglyph.clone(),
        VideoOption::ViewBobbing => labels.view_bobbing.clone(),
        VideoOption::GuiScale => labels.gui_scale.clone(),
        VideoOption::RenderClouds => labels.render_clouds.clone(),
        VideoOption::Particles => labels.particles.clone(),
        VideoOption::UseFullscreen => labels.fullscreen.clone(),
        VideoOption::EnableVsync => labels.vsync.clone(),
        VideoOption::BlockAlternatives => labels.block_alternatives.clone(),
        VideoOption::UseVbo => labels.use_vbo.clone(),
        VideoOption::EntityShadows => labels.entity_shadows.clone(),
        _ => String::new(),
    }
}

/// One cycle button's value word at the stored settings. English, like the
/// row labels below: the value words resolve from the same keys only in
/// the store's own table, so the screen draws the English the tests pin.
fn option_value(option: VideoOption, settings: &GameSettings) -> String {
    match option {
        VideoOption::Graphics => {
            if settings.graphics.as_bool() {
                "Fancy".to_string()
            } else {
                "Fast".to_string()
            }
        }
        VideoOption::AmbientOcclusion => match settings.smooth_lighting {
            1 => "Minimum".to_string(),
            2 => "Maximum".to_string(),
            _ => "OFF".to_string(),
        },
        VideoOption::Anaglyph => on_off(settings.anaglyph),
        VideoOption::ViewBobbing => on_off(settings.view_bobbing),
        VideoOption::GuiScale => match settings.gui_scale {
            1 => "Small".to_string(),
            2 => "Normal".to_string(),
            3 => "Large".to_string(),
            _ => "Auto".to_string(),
        },
        VideoOption::RenderClouds => match settings.clouds {
            CloudSetting::Off => "OFF".to_string(),
            CloudSetting::Fast => "Fast".to_string(),
            CloudSetting::Fancy => "Fancy".to_string(),
        },
        VideoOption::Particles => match settings.particles {
            ParticleSetting::All => "All".to_string(),
            ParticleSetting::Decreased => "Decreased".to_string(),
            ParticleSetting::Minimal => "Minimal".to_string(),
        },
        VideoOption::UseFullscreen => on_off(settings.fullscreen),
        VideoOption::EnableVsync => on_off(settings.vsync),
        VideoOption::BlockAlternatives => on_off(settings.alternate_blocks),
        VideoOption::UseVbo => on_off(settings.use_vbos),
        VideoOption::EntityShadows => on_off(settings.entity_shadows),
        _ => String::new(),
    }
}

/// One row's composed label after a write (sliders and buttons alike).
fn option_label(labels: &VideoLabels, option: VideoOption, settings: &GameSettings) -> String {
    match option {
        VideoOption::RenderDistance => {
            render_distance_label(&labels.render_distance, settings.render_distance)
        }
        VideoOption::FramerateLimit => {
            framerate_label(&labels.framerate_limit, settings.max_framerate)
        }
        VideoOption::Gamma => gamma_label(&labels.gamma, settings.brightness),
        VideoOption::MipmapLevels => mipmap_label(&labels.mipmap_levels, settings.mipmap_levels),
        VideoOption::Graphics => graphics_label(&labels.graphics, settings.graphics.as_bool()),
        VideoOption::AmbientOcclusion => {
            ambient_occlusion_label(&labels.ambient_occlusion, settings.smooth_lighting)
        }
        VideoOption::GuiScale => gui_scale_label(&labels.gui_scale, settings.gui_scale),
        VideoOption::RenderClouds => clouds_label(&labels.render_clouds, settings.clouds),
        VideoOption::Particles => particles_label(&labels.particles, settings.particles),
        VideoOption::Anaglyph => toggle_label(&labels.anaglyph, settings.anaglyph),
        VideoOption::ViewBobbing => toggle_label(&labels.view_bobbing, settings.view_bobbing),
        VideoOption::UseFullscreen => toggle_label(&labels.fullscreen, settings.fullscreen),
        VideoOption::EnableVsync => toggle_label(&labels.vsync, settings.vsync),
        VideoOption::BlockAlternatives => {
            toggle_label(&labels.block_alternatives, settings.alternate_blocks)
        }
        VideoOption::UseVbo => toggle_label(&labels.use_vbo, settings.use_vbos),
        VideoOption::EntityShadows => toggle_label(&labels.entity_shadows, settings.entity_shadows),
    }
}

/// One slider's reseat value at the stored settings (the `mouseDragged`
/// re-read after a write).
fn slider_value(option: VideoOption, settings: &GameSettings) -> f32 {
    match option {
        VideoOption::RenderDistance => f32::from(settings.render_distance),
        VideoOption::FramerateLimit => {
            framerate_denormalize(framerate_normalize(settings.max_framerate)) as f32
        }
        VideoOption::Gamma => gamma_normalize(settings.brightness),
        _ => f32::from(settings.mipmap_levels),
    }
}

/// Whether the point presses the control: the rect, edges inclusive on
/// the top-left and exclusive on the bottom-right.
fn control_hit(control: &mut VideoControl, point: (f32, f32)) -> bool {
    match control {
        VideoControl::Slider(_, slider) => {
            point.0 >= slider.x()
                && point.1 >= slider.y()
                && point.0 < slider.x() + slider.w()
                && point.1 < slider.y() + 20.0
        }
        VideoControl::Button(_, button) => button.press(point),
    }
}

/// One slider press at the point: folds the pointer, latches, and
/// answers the 0..1 position the store write reads.
fn slider_press(control: &mut VideoControl, point: (f32, f32)) -> Option<f32> {
    let VideoControl::Slider(_, slider) = control else {
        return None;
    };
    slider.press(point)?;
    Some(slider.position())
}

/// Presses one cycle button at the point (the row's `mousePressed` gate).
fn press_button(control: &mut VideoControl, point: (f32, f32)) {
    if let VideoControl::Button(_, button) = control {
        button.press(point);
    }
}
