//! The game settings store: the vanilla option set with source-derived defaults,
//! ranges, the per-field wire/local split, and the `options.toml` load/save pair.
//!
//! Field defaults follow the field initialisers in
//! `client/settings/GameSettings.java`:65-182 with the constructor rules in
//! `GameSettings.java`:184-206. The file format here is this port's own
//! (`options.toml`, sorted keys, one per line); vanilla's `options.txt` is
//! never read or written.
//!
//! # Wire versus local
//!
//! Wire fields re-send client settings (`C15PacketClientSettings`, built by
//! [`GameSettings::to_client_settings`]): the locale, the view distance, the
//! chat mode, chat colours, and the skin-parts byte. Everything else applies
//! locally (video, audio, chat box geometry) and never leaves the client.
//!
//! # Hostile input
//!
//! [`GameSettings::load`] is total: a missing or unreadable file is defaults,
//! unknown keys are ignored, malformed lines are skipped, wrong-typed values
//! keep their default, and out-of-range values clamp. Loading never panics.
//!
//! # Out of scope, recorded
//!
//! Difficulty, resource packs, the stream options, snooper, touchscreen,
//! the override sizes and held-item tooltips have no machine in this client
//! and are not carried. `autoJump` and locked difficulty are absent from
//! the 1.8.9 source itself. `use_vbos` persists but is a recorded no-op
//! (the renderer always uses VBOs), as is `anaglyph`.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use oxide_proto_v47::serverbound::ClientSettings;

/// The view-distance slider range, in chunks.
///
/// The `RENDER_DISTANCE` enum base is 2-16 (`GameSettings.java`:1274) and the
/// 64-bit constructor raises the max to 32 when the heap is at least 1 GB
/// (`GameSettings.java`:195-202). This port is 64-bit-only, so the raised max
/// governs; the 8-chunk 32-bit default branch is recorded but never taken.
/// The rig confirmation of the shown slider value is carried separately.
pub const VIEW_DISTANCE_RANGE: (u8, u8) = (2, 32);

/// The sound-category key names in `SoundCategory`'s id order
/// (`SoundCategory.java`:8-16).
///
/// Index `i` of [`GameSettings::category_volumes`] is the level of
/// `CATEGORY_NAMES[i]`.
pub const CATEGORY_NAMES: [&str; 9] = [
    "master", "music", "record", "weather", "block", "hostile", "neutral", "player", "ambient",
];

/// The index of the master level in [`GameSettings::category_volumes`].
pub const CATEGORY_MASTER: usize = 0;

/// The index of the music level in [`GameSettings::category_volumes`].
pub const CATEGORY_MUSIC: usize = 1;

/// The index of the record level in [`GameSettings::category_volumes`].
pub const CATEGORY_RECORD: usize = 2;

/// The index of the weather level in [`GameSettings::category_volumes`].
pub const CATEGORY_WEATHER: usize = 3;

/// The index of the block level in [`GameSettings::category_volumes`].
pub const CATEGORY_BLOCK: usize = 4;

/// The index of the hostile level in [`GameSettings::category_volumes`].
pub const CATEGORY_HOSTILE: usize = 5;

/// The index of the neutral level in [`GameSettings::category_volumes`].
pub const CATEGORY_NEUTRAL: usize = 6;

/// The index of the player level in [`GameSettings::category_volumes`].
pub const CATEGORY_PLAYER: usize = 7;

/// The index of the ambient level in [`GameSettings::category_volumes`].
pub const CATEGORY_AMBIENT: usize = 8;

/// The `max_framerate` value meaning unlimited.
///
/// The source's `FRAMERATE_LIMIT` slider spans 10-260 in steps of 10 with the
/// top step (260) displaying `options.framerateLimit.max` = Unlimited and
/// skipping the frame sync (`GameSettings.java`:71, `:623`, `en_US.lang`:340,
/// `Minecraft.java`:1209-1213); 260 does not fit in a `u8`, so the numeric
/// range stops at the step-aligned 250 and the top folds into this sentinel.
pub const FRAMERATE_UNLIMITED: u8 = 255;

/// The numeric `max_framerate` range; [`FRAMERATE_UNLIMITED`] sits above it.
pub const FRAMERATE_RANGE: (u8, u8) = (10, 250);

/// The brightness range, in percent of the source's gamma slider.
///
/// `gammaSetting` has no initialiser (Java default 0.0, the Moody end), so the
/// default brightness is 0; the value maps to the gamma slider as-is over 100.
pub const BRIGHTNESS_RANGE: (u8, u8) = (0, 100);

/// The GUI scale range: 0 is auto, then the three fixed scales
/// (`GameSettings.java`:57 lists the four labels).
pub const GUI_SCALE_RANGE: (u8, u8) = (0, 3);

/// The field-of-view range (`FOV`, `GameSettings.java`: `30.0F, 110.0F`).
pub const FOV_RANGE: (u8, u8) = (30, 110);

/// The mipmap-level range (`MIPMAP_LEVELS`, `0.0F`-`4.0F`).
pub const MIPMAP_RANGE: (u8, u8) = (0, 4);

/// The smooth-lighting range: off, minimum, maximum
/// (`AMBIENT_OCCLUSIONS`, `GameSettings.java`:59).
pub const SMOOTH_LIGHTING_RANGE: (u8, u8) = (0, 2);

/// The chat-mode range: full, commands only, hidden
/// (`EnumChatVisibility`, matching `ClientSettings.chat_mode`).
pub const CHAT_MODE_RANGE: (u8, u8) = (0, 2);

/// The particle amount option, in the source's option order
/// (`PARTICLES`, `GameSettings.java`:58: all, decreased, minimal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticleSetting {
    /// Every particle spawns.
    All,
    /// Roughly one in three spawns is degraded away.
    Decreased,
    /// Only minimal spawns survive.
    Minimal,
}

impl ParticleSetting {
    /// Maps a stored index, clamping past-the-end values to [`Self::Minimal`].
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::All,
            1 => Self::Decreased,
            _ => Self::Minimal,
        }
    }

    /// The stored index, in the source's option order.
    pub fn as_u8(self) -> u8 {
        match self {
            Self::All => 0,
            Self::Decreased => 1,
            Self::Minimal => 2,
        }
    }
}

/// The cloud render mode, in the source's option order
/// (`CLOUDS_TYPES`, `GameSettings.java`:64: off, fast, fancy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudSetting {
    /// No clouds draw.
    Off,
    /// The flat fast layer draws.
    Fast,
    /// The fancy layer draws.
    Fancy,
}

impl CloudSetting {
    /// Maps a stored index, clamping past-the-end values to [`Self::Fancy`].
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Off,
            1 => Self::Fast,
            _ => Self::Fancy,
        }
    }

    /// The stored index, in the source's option order.
    pub fn as_u8(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Fast => 1,
            Self::Fancy => 2,
        }
    }
}

/// The graphics mode: fast or fancy (`fancyGraphics`, `GameSettings.java`:75).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsSetting {
    /// Fast graphics (the mesher's `graphics_fast` path).
    Fast,
    /// Fancy graphics.
    Fancy,
}

impl GraphicsSetting {
    /// Maps the source's boolean: set means fancy.
    pub fn from_bool(fancy: bool) -> Self {
        if fancy { Self::Fancy } else { Self::Fast }
    }

    /// The source's boolean: fancy is set.
    pub fn as_bool(self) -> bool {
        matches!(self, Self::Fancy)
    }
}

/// One named key binding: the source's `KeyBinding` description key minus
/// the `key.` prefix, with its LWJGL code — negative for a mouse button
/// (`-100` left, `-99` right, `-98` middle), `0` for unbound
/// (`GameSettings.java`:127-151, `GuiControls.java`:127-152).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Binding {
    /// The description key minus its `key.` prefix (`forward`, `hotbar.1`).
    pub name: &'static str,
    /// The LWJGL code the binding holds.
    pub code: i32,
}

/// How many bindings the store carries: the source's 24 named fields plus
/// the 9-hotbar array (`GameSettings.java`:127-151).
pub const BINDING_COUNT: usize = 33;

/// The lowest storable code: the left button (`-100 + 0`,
/// `GuiKeyBindingList.java`:98-110).
pub const BINDING_CODE_MIN: i32 = -100;

/// The highest storable code: past every LWJGL key code.
pub const BINDING_CODE_MAX: i32 = 255;

/// The default table in the source's array order (`GameSettings.java`'s
/// constructor: attack, use, forward, left, back, right, jump, sneak,
/// sprint, drop, inventory, chat, playerlist, pick block, command,
/// screenshot, perspective, smooth camera, the four stream keys, fullscreen,
/// spectator outlines, then hotbar 1-9): `(name, category, code)`.
pub const BINDING_TABLE: [(&str, &str, i32); BINDING_COUNT] = [
    ("attack", "key.categories.gameplay", -100),
    ("use", "key.categories.gameplay", -99),
    ("forward", "key.categories.movement", 17),
    ("left", "key.categories.movement", 30),
    ("back", "key.categories.movement", 31),
    ("right", "key.categories.movement", 32),
    ("jump", "key.categories.movement", 57),
    ("sneak", "key.categories.movement", 42),
    ("sprint", "key.categories.movement", 29),
    ("drop", "key.categories.gameplay", 16),
    ("inventory", "key.categories.inventory", 18),
    ("chat", "key.categories.multiplayer", 20),
    ("playerlist", "key.categories.multiplayer", 15),
    ("pickItem", "key.categories.gameplay", -98),
    ("command", "key.categories.multiplayer", 53),
    ("screenshot", "key.categories.misc", 60),
    ("togglePerspective", "key.categories.misc", 63),
    ("smoothCamera", "key.categories.misc", 0),
    ("streamStartStop", "key.categories.stream", 64),
    ("streamPauseUnpause", "key.categories.stream", 65),
    ("streamCommercial", "key.categories.stream", 0),
    ("streamToggleMic", "key.categories.stream", 0),
    ("fullscreen", "key.categories.misc", 87),
    ("spectatorOutlines", "key.categories.misc", 0),
    ("hotbar.1", "key.categories.inventory", 2),
    ("hotbar.2", "key.categories.inventory", 3),
    ("hotbar.3", "key.categories.inventory", 4),
    ("hotbar.4", "key.categories.inventory", 5),
    ("hotbar.5", "key.categories.inventory", 6),
    ("hotbar.6", "key.categories.inventory", 7),
    ("hotbar.7", "key.categories.inventory", 8),
    ("hotbar.8", "key.categories.inventory", 9),
    ("hotbar.9", "key.categories.inventory", 10),
];

/// The binding list's category order: the source sorts by the localized
/// category (`KeyBinding.compareTo`, `KeyBinding.java`:143-151), which in
/// English reads Gameplay, Inventory, Miscellaneous, Movement, Multiplayer,
/// Streaming (`en_US.lang`: `key.categories.*`).
pub const BINDING_CATEGORY_ORDER: [&str; 6] = [
    "key.categories.gameplay",
    "key.categories.inventory",
    "key.categories.misc",
    "key.categories.movement",
    "key.categories.multiplayer",
    "key.categories.stream",
];

/// The default bindings as [`Binding`] rows in [`BINDING_TABLE`] order.
pub fn default_bindings() -> [Binding; BINDING_COUNT] {
    std::array::from_fn(|index| {
        let (name, _, code) = BINDING_TABLE[index];
        Binding { name, code }
    })
}

/// Clamps a raw code into the storable range.
fn clamp_binding(code: i32) -> i32 {
    code.clamp(BINDING_CODE_MIN, BINDING_CODE_MAX)
}

/// The vanilla option set with source-derived defaults.
///
/// The per-field wire/local split is documented at the module level; the
/// ranges live in the `*_RANGE` constants beside each field's docs.
#[derive(Debug, Clone, PartialEq)]
pub struct GameSettings {
    /// View distance in chunks; default 12, the 64-bit constructor branch
    /// (`GameSettings.java`:204). Range [`VIEW_DISTANCE_RANGE`].
    pub render_distance: u8,
    /// Brightness in percent of the gamma slider; default 0 (Moody).
    /// Range [`BRIGHTNESS_RANGE`].
    pub brightness: u8,
    /// The particle amount; default all (`particleSetting` has no initialiser).
    pub particles: ParticleSetting,
    /// GUI scale; 0 is auto, the default (`guiScale` has no initialiser).
    /// Range [`GUI_SCALE_RANGE`].
    pub gui_scale: u8,
    /// Field of view; default 70 (`GameSettings.java`:189).
    /// Range [`FOV_RANGE`].
    pub fov: u8,
    /// Framerate cap; default 120 (`GameSettings.java`:71), numeric range
    /// [`FRAMERATE_RANGE`], [`FRAMERATE_UNLIMITED`] for no cap.
    pub max_framerate: u8,
    /// Mipmap levels; default 4 (`GameSettings.java`:111).
    /// Range [`MIPMAP_RANGE`].
    pub mipmap_levels: u8,
    /// Vertical sync; default true (`GameSettings.java`:88).
    pub vsync: bool,
    /// View bobbing; default true (`GameSettings.java`:68).
    pub view_bobbing: bool,
    /// The VBO toggle; default false (`GameSettings.java`:89). Recorded
    /// no-op: the renderer always uses VBOs, the value only persists.
    pub use_vbos: bool,
    /// Alternate blocks; default true (`GameSettings.java`:90). The mesher
    /// reads it at its next touch.
    pub alternate_blocks: bool,
    /// Entity shadows; default true (`GameSettings.java`:125). The entity
    /// pass's shadow draws read it.
    pub entity_shadows: bool,
    /// Cloud mode; default fancy (`clouds = 2`, `GameSettings.java`:74).
    pub clouds: CloudSetting,
    /// Graphics mode; default fancy (`GameSettings.java`:75).
    pub graphics: GraphicsSetting,
    /// Smooth lighting; default 2, maximum (`GameSettings.java`:78), matching
    /// the session's `SMOOTH_LIGHTING`. Range [`SMOOTH_LIGHTING_RANGE`].
    pub smooth_lighting: u8,
    /// Anaglyph 3D; default false (no initialiser). Recorded no-op.
    pub anaglyph: bool,
    /// Fullscreen; default false (no initialiser).
    pub fullscreen: bool,
    /// Realms notifications; default true (`GameSettings.java`:126). The
    /// options hub's no-world row-0 toggle writes it.
    pub realms_notifications: bool,
    /// Music gain; default 1.0. The engine multiplies it over the music
    /// category level.
    pub music_volume: f32,
    /// Master gain; default 1.0. The engine multiplies it over every
    /// category level.
    pub sound_volume: f32,
    /// Per-category levels in [`CATEGORY_NAMES`] order; each defaults to 1.0
    /// (`getSoundLevel`, `GameSettings.java`:1168-1171).
    pub category_volumes: [f32; 9],
    /// Mouse sensitivity; default 0.5 (`GameSettings.java`:65), range 0-1.
    pub mouse_sensitivity: f32,
    /// Inverted mouse; default false (no initialiser).
    pub invert_mouse: bool,
    /// Smooth camera; default false (no initialiser).
    pub smooth_camera: bool,
    /// Chat opacity; default 1.0 (`GameSettings.java`:85), range 0-1.
    pub chat_opacity: f32,
    /// Chat scale; default 1.0 (`GameSettings.java`:106), range 0-1.
    pub chat_scale: f32,
    /// Chat width; default 1.0 (`GameSettings.java`:107), range 0-1.
    pub chat_width: f32,
    /// Chat (focused) height; default 1.0 (`GameSettings.java`:109), range
    /// 0-1.
    pub chat_height: f32,
    /// Chat unfocused height; default 0.44366196 (`GameSettings.java`:108),
    /// range 0-1.
    pub chat_height_unfocused: f32,
    /// Web links in chat; default true (`GameSettings.java`:83).
    pub chat_links: bool,
    /// The confirm prompt before opening a chat link; default true
    /// (`GameSettings.java`:84).
    pub chat_links_prompt: bool,
    /// Chat colours; default true (`GameSettings.java`:82). Wire field.
    pub chat_colors: bool,
    /// Chat visibility mode; default 0, full. Range [`CHAT_MODE_RANGE`].
    /// Wire field.
    pub chat_mode: u8,
    /// Displayed skin parts bitmask; default 0x7F, every one of the seven
    /// model parts set (`GameSettings.java`:101 with `1 << partId` masks).
    /// Wire field.
    pub skin_parts: u8,
    /// Locale; default `en_US` (`GameSettings.java`:190). Wire field.
    pub language: String,
    /// Forced unicode font; default false (`GameSettings.java`:191).
    pub force_unicode: bool,
    /// Reduced debug info; default false (`GameSettings.java`: the
    /// `reducedDebugInfo` flag, no initialiser). It rides the chat screen
    /// in the source (`ScreenChatOptions.java`:9).
    pub reduced_debug_info: bool,
    /// The 33 rebindable keys in [`BINDING_TABLE`] order
    /// (`GameSettings.java`:127-151).
    pub bindings: [Binding; BINDING_COUNT],
    /// The last server joined; the source default is empty
    /// (`GameSettings.java`:188), which this port reads as absent.
    pub last_server: Option<String>,
}

impl Default for GameSettings {
    /// The source-derived defaults documented per field above.
    fn default() -> Self {
        Self {
            render_distance: 12,
            brightness: 0,
            particles: ParticleSetting::All,
            gui_scale: 0,
            fov: 70,
            max_framerate: 120,
            mipmap_levels: 4,
            vsync: true,
            view_bobbing: true,
            use_vbos: false,
            alternate_blocks: true,
            entity_shadows: true,
            clouds: CloudSetting::Fancy,
            graphics: GraphicsSetting::Fancy,
            smooth_lighting: 2,
            anaglyph: false,
            fullscreen: false,
            realms_notifications: true,
            music_volume: 1.0,
            sound_volume: 1.0,
            category_volumes: [1.0; 9],
            mouse_sensitivity: 0.5,
            invert_mouse: false,
            smooth_camera: false,
            chat_opacity: 1.0,
            chat_scale: 1.0,
            chat_width: 1.0,
            chat_height: 1.0,
            chat_height_unfocused: 0.44366196,
            chat_links: true,
            chat_links_prompt: true,
            chat_colors: true,
            chat_mode: 0,
            skin_parts: 0x7F,
            language: "en_US".to_string(),
            force_unicode: false,
            reduced_debug_info: false,
            bindings: default_bindings(),
            last_server: None,
        }
    }
}

/// Parses an integer value; anything else keeps the field's default.
fn parse_int(text: &str) -> Option<i64> {
    text.trim().parse::<i64>().ok()
}

/// Parses a float value; non-finite values keep the field's default.
///
/// Like the source's `parseFloat` (`GameSettings.java`:1060-1063), the words
/// `true` and `false` read as 1.0 and 0.0.
fn parse_float(text: &str) -> Option<f32> {
    let trimmed = text.trim();
    if trimmed.eq_ignore_ascii_case("true") {
        return Some(1.0);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Some(0.0);
    }
    match trimmed.parse::<f32>() {
        Ok(value) if value.is_finite() => Some(value),
        _ => None,
    }
}

/// Parses a boolean value; anything else keeps the field's default.
fn parse_bool(text: &str) -> Option<bool> {
    if text.trim().eq_ignore_ascii_case("true") {
        Some(true)
    } else if text.trim().eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// Clamps a parsed integer into a `u8` range.
fn clamp_u8(raw: i64, range: (u8, u8)) -> u8 {
    raw.clamp(i64::from(range.0), i64::from(range.1)) as u8
}

/// Clamps a parsed float into range.
fn clamp_f32(value: f32, low: f32, high: f32) -> f32 {
    value.clamp(low, high)
}

/// Parses a particle setting from an index or an option name.
fn parse_particles(text: &str) -> Option<ParticleSetting> {
    if let Some(raw) = parse_int(text) {
        return Some(ParticleSetting::from_u8(clamp_u8(raw, (0, 2))));
    }
    if text.trim().eq_ignore_ascii_case("all") {
        Some(ParticleSetting::All)
    } else if text.trim().eq_ignore_ascii_case("decreased") {
        Some(ParticleSetting::Decreased)
    } else if text.trim().eq_ignore_ascii_case("minimal") {
        Some(ParticleSetting::Minimal)
    } else {
        None
    }
}

/// Parses a cloud setting from an index, an option name, or a boolean
/// (the vanilla file writes `true`/`false`/`fast` for the three modes).
fn parse_clouds(text: &str) -> Option<CloudSetting> {
    if let Some(raw) = parse_int(text) {
        return Some(CloudSetting::from_u8(clamp_u8(raw, (0, 2))));
    }
    if text.trim().eq_ignore_ascii_case("off") {
        Some(CloudSetting::Off)
    } else if text.trim().eq_ignore_ascii_case("fast") {
        Some(CloudSetting::Fast)
    } else if text.trim().eq_ignore_ascii_case("fancy") {
        Some(CloudSetting::Fancy)
    } else {
        parse_bool(text).map(|flag| CloudSetting::from_u8(if flag { 2 } else { 0 }))
    }
}

/// Parses a graphics setting from a boolean, a name, or an index.
fn parse_graphics(text: &str) -> Option<GraphicsSetting> {
    if let Some(flag) = parse_bool(text) {
        return Some(GraphicsSetting::from_bool(flag));
    }
    if text.trim().eq_ignore_ascii_case("fast") {
        Some(GraphicsSetting::Fast)
    } else if text.trim().eq_ignore_ascii_case("fancy") {
        Some(GraphicsSetting::Fancy)
    } else {
        parse_int(text).map(|raw| GraphicsSetting::from_bool(raw != 0))
    }
}

/// Unquotes a saved string; anything malformed keeps the field's default.
fn unquote(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.len() < 2 || !trimmed.starts_with('"') || !trimmed.ends_with('"') {
        return None;
    }
    let body = &trimmed[1..trimmed.len() - 1];
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(char) = chars.next() {
        if char == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => return None,
            }
        } else {
            out.push(char);
        }
    }
    Some(out)
}

/// Quotes a string for the file.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for char in text.chars() {
        match char {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(char),
        }
    }
    out.push('"');
    out
}

/// Formats a float so `save`/`load` round-trips exactly and stably.
fn fmt_f32(value: f32) -> String {
    value.to_string()
}

impl GameSettings {
    /// Loads the settings file; total: a missing or unreadable file is
    /// defaults, unknown keys are ignored, malformed lines are skipped,
    /// wrong-typed values keep their default, and out-of-range values clamp.
    /// Never panics, whatever the file holds.
    pub fn load(path: impl AsRef<Path>) -> Self {
        let text = fs::read_to_string(path.as_ref()).unwrap_or_default();
        let mut settings = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                settings.apply_entry(key.trim(), value.trim());
            }
        }
        settings
    }

    /// Applies one `key = value` line; unknown keys and wrong-typed values
    /// leave the settings untouched.
    fn apply_entry(&mut self, key: &str, value: &str) {
        if key.starts_with("category_") {
            if let Some(name) = key.strip_prefix("category_") {
                if let Some(index) = CATEGORY_NAMES.iter().position(|known| *known == name) {
                    if let Some(level) = parse_float(value) {
                        self.category_volumes[index] = clamp_f32(level, 0.0, 1.0);
                    }
                }
            }
            return;
        }
        if key.starts_with("key_") {
            if let Some(name) = key.strip_prefix("key_") {
                if BINDING_TABLE.iter().any(|(known, _, _)| *known == name) {
                    if let Some(raw) = parse_int(value) {
                        let clamped = raw
                            .clamp(i64::from(BINDING_CODE_MIN), i64::from(BINDING_CODE_MAX))
                            as i32;
                        self.set_binding(name, clamped);
                    }
                }
            }
            return;
        }
        match key {
            "render_distance" => {
                if let Some(raw) = parse_int(value) {
                    self.render_distance = clamp_u8(raw, VIEW_DISTANCE_RANGE);
                }
            }
            "brightness" => {
                if let Some(raw) = parse_int(value) {
                    self.brightness = clamp_u8(raw, BRIGHTNESS_RANGE);
                }
            }
            "particles" => {
                if let Some(setting) = parse_particles(value) {
                    self.particles = setting;
                }
            }
            "gui_scale" => {
                if let Some(raw) = parse_int(value) {
                    self.gui_scale = clamp_u8(raw, GUI_SCALE_RANGE);
                }
            }
            "fov" => {
                if let Some(raw) = parse_int(value) {
                    self.fov = clamp_u8(raw, FOV_RANGE);
                }
            }
            "max_framerate" => {
                if let Some(raw) = parse_int(value) {
                    self.max_framerate = if raw > i64::from(FRAMERATE_RANGE.1) {
                        FRAMERATE_UNLIMITED
                    } else {
                        clamp_u8(raw, FRAMERATE_RANGE)
                    };
                }
            }
            "mipmap_levels" => {
                if let Some(raw) = parse_int(value) {
                    self.mipmap_levels = clamp_u8(raw, MIPMAP_RANGE);
                }
            }
            "vsync" => {
                if let Some(flag) = parse_bool(value) {
                    self.vsync = flag;
                }
            }
            "view_bobbing" => {
                if let Some(flag) = parse_bool(value) {
                    self.view_bobbing = flag;
                }
            }
            "use_vbos" => {
                if let Some(flag) = parse_bool(value) {
                    self.use_vbos = flag;
                }
            }
            "alternate_blocks" => {
                if let Some(flag) = parse_bool(value) {
                    self.alternate_blocks = flag;
                }
            }
            "entity_shadows" => {
                if let Some(flag) = parse_bool(value) {
                    self.entity_shadows = flag;
                }
            }
            "clouds" => {
                if let Some(setting) = parse_clouds(value) {
                    self.clouds = setting;
                }
            }
            "graphics" => {
                if let Some(setting) = parse_graphics(value) {
                    self.graphics = setting;
                }
            }
            "smooth_lighting" => {
                if let Some(raw) = parse_int(value) {
                    self.smooth_lighting = clamp_u8(raw, SMOOTH_LIGHTING_RANGE);
                }
            }
            "anaglyph" => {
                if let Some(flag) = parse_bool(value) {
                    self.anaglyph = flag;
                }
            }
            "fullscreen" => {
                if let Some(flag) = parse_bool(value) {
                    self.fullscreen = flag;
                }
            }
            "realms_notifications" => {
                if let Some(flag) = parse_bool(value) {
                    self.realms_notifications = flag;
                }
            }
            "music_volume" => {
                if let Some(level) = parse_float(value) {
                    self.music_volume = clamp_f32(level, 0.0, 1.0);
                }
            }
            "sound_volume" => {
                if let Some(level) = parse_float(value) {
                    self.sound_volume = clamp_f32(level, 0.0, 1.0);
                }
            }
            "mouse_sensitivity" => {
                if let Some(level) = parse_float(value) {
                    self.mouse_sensitivity = clamp_f32(level, 0.0, 1.0);
                }
            }
            "invert_mouse" => {
                if let Some(flag) = parse_bool(value) {
                    self.invert_mouse = flag;
                }
            }
            "smooth_camera" => {
                if let Some(flag) = parse_bool(value) {
                    self.smooth_camera = flag;
                }
            }
            "chat_opacity" => {
                if let Some(level) = parse_float(value) {
                    self.chat_opacity = clamp_f32(level, 0.0, 1.0);
                }
            }
            "chat_scale" => {
                if let Some(level) = parse_float(value) {
                    self.chat_scale = clamp_f32(level, 0.0, 1.0);
                }
            }
            "chat_width" => {
                if let Some(level) = parse_float(value) {
                    self.chat_width = clamp_f32(level, 0.0, 1.0);
                }
            }
            "chat_height" => {
                if let Some(level) = parse_float(value) {
                    self.chat_height = clamp_f32(level, 0.0, 1.0);
                }
            }
            "chat_height_unfocused" => {
                if let Some(level) = parse_float(value) {
                    self.chat_height_unfocused = clamp_f32(level, 0.0, 1.0);
                }
            }
            "chat_links" => {
                if let Some(flag) = parse_bool(value) {
                    self.chat_links = flag;
                }
            }
            "chat_links_prompt" => {
                if let Some(flag) = parse_bool(value) {
                    self.chat_links_prompt = flag;
                }
            }
            "chat_colors" => {
                if let Some(flag) = parse_bool(value) {
                    self.chat_colors = flag;
                }
            }
            "chat_mode" => {
                if let Some(raw) = parse_int(value) {
                    self.chat_mode = clamp_u8(raw, CHAT_MODE_RANGE);
                }
            }
            "skin_parts" => {
                if let Some(raw) = parse_int(value) {
                    self.skin_parts = clamp_u8(raw, (0, 255));
                }
            }
            "language" => {
                if let Some(language) = unquote(value) {
                    if !language.is_empty() {
                        self.language = language;
                    }
                }
            }
            "force_unicode" => {
                if let Some(flag) = parse_bool(value) {
                    self.force_unicode = flag;
                }
            }
            "reduced_debug_info" => {
                if let Some(flag) = parse_bool(value) {
                    self.reduced_debug_info = flag;
                }
            }
            "last_server" => {
                if let Some(server) = unquote(value) {
                    if server.is_empty() {
                        self.last_server = None;
                    } else {
                        self.last_server = Some(server);
                    }
                }
            }
            _ => {}
        }
    }

    /// Collects the file entries; a `BTreeMap` keeps the keys sorted.
    fn entries(&self) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        map.insert(
            "alternate_blocks".to_string(),
            self.alternate_blocks.to_string(),
        );
        map.insert("anaglyph".to_string(), self.anaglyph.to_string());
        map.insert("brightness".to_string(), self.brightness.to_string());
        for (index, name) in CATEGORY_NAMES.iter().enumerate() {
            map.insert(
                format!("category_{name}"),
                fmt_f32(self.category_volumes[index]),
            );
        }
        map.insert("chat_colors".to_string(), self.chat_colors.to_string());
        map.insert("chat_height".to_string(), fmt_f32(self.chat_height));
        map.insert(
            "chat_height_unfocused".to_string(),
            fmt_f32(self.chat_height_unfocused),
        );
        map.insert("chat_links".to_string(), self.chat_links.to_string());
        map.insert(
            "chat_links_prompt".to_string(),
            self.chat_links_prompt.to_string(),
        );
        map.insert("chat_mode".to_string(), self.chat_mode.to_string());
        map.insert("chat_opacity".to_string(), fmt_f32(self.chat_opacity));
        map.insert("chat_scale".to_string(), fmt_f32(self.chat_scale));
        map.insert("chat_width".to_string(), fmt_f32(self.chat_width));
        map.insert("clouds".to_string(), self.clouds.as_u8().to_string());
        map.insert(
            "entity_shadows".to_string(),
            self.entity_shadows.to_string(),
        );
        map.insert("force_unicode".to_string(), self.force_unicode.to_string());
        map.insert("fov".to_string(), self.fov.to_string());
        map.insert("fullscreen".to_string(), self.fullscreen.to_string());
        map.insert("graphics".to_string(), self.graphics.as_bool().to_string());
        map.insert("gui_scale".to_string(), self.gui_scale.to_string());
        map.insert("invert_mouse".to_string(), self.invert_mouse.to_string());
        for binding in &self.bindings {
            map.insert(format!("key_{}", binding.name), binding.code.to_string());
        }
        map.insert("language".to_string(), quote(&self.language));
        if let Some(server) = &self.last_server {
            map.insert("last_server".to_string(), quote(server));
        }
        map.insert("max_framerate".to_string(), self.max_framerate.to_string());
        map.insert("mipmap_levels".to_string(), self.mipmap_levels.to_string());
        map.insert(
            "mouse_sensitivity".to_string(),
            fmt_f32(self.mouse_sensitivity),
        );
        map.insert("music_volume".to_string(), fmt_f32(self.music_volume));
        map.insert("particles".to_string(), self.particles.as_u8().to_string());
        map.insert(
            "render_distance".to_string(),
            self.render_distance.to_string(),
        );
        map.insert(
            "realms_notifications".to_string(),
            self.realms_notifications.to_string(),
        );
        map.insert(
            "reduced_debug_info".to_string(),
            self.reduced_debug_info.to_string(),
        );
        map.insert("skin_parts".to_string(), self.skin_parts.to_string());
        map.insert("smooth_camera".to_string(), self.smooth_camera.to_string());
        map.insert(
            "smooth_lighting".to_string(),
            self.smooth_lighting.to_string(),
        );
        map.insert("sound_volume".to_string(), fmt_f32(self.sound_volume));
        map.insert("use_vbos".to_string(), self.use_vbos.to_string());
        map.insert("view_bobbing".to_string(), self.view_bobbing.to_string());
        map.insert("vsync".to_string(), self.vsync.to_string());
        map
    }

    /// Saves the settings in the file's own format: sorted keys, one per
    /// line. Writing the same settings twice yields identical bytes.
    ///
    /// The write is atomic: the bytes land in a sibling temporary file first
    /// and the rename publishes them, so a crash never leaves a half-written
    /// file behind.
    pub fn save(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let mut text = String::new();
        for (key, value) in self.entries() {
            text.push_str(&key);
            text.push_str(" = ");
            text.push_str(&value);
            text.push('\n');
        }
        let staged = path.with_extension("tmp");
        fs::write(&staged, text)?;
        if let Err(error) = fs::rename(&staged, path) {
            let _ = fs::remove_file(&staged);
            return Err(error);
        }
        Ok(())
    }

    /// The effective view distance: the stored value clamped into
    /// [`VIEW_DISTANCE_RANGE`].
    pub fn view_distance_chunks(&self) -> u8 {
        self.render_distance
            .clamp(VIEW_DISTANCE_RANGE.0, VIEW_DISTANCE_RANGE.1)
    }

    /// The camera far plane for the effective view distance: chunks times
    /// sixteen (`EntityRenderer.java`:750).
    pub fn far_plane(&self) -> f32 {
        f32::from(self.view_distance_chunks()) * 16.0
    }

    /// Sets the GUI scale option index, clamping past-the-end values to the
    /// largest scale.
    pub fn set_gui_scale_index(&mut self, index: u8) {
        self.gui_scale = index.min(GUI_SCALE_RANGE.1);
    }

    /// The default code for `name`, or `None` for an unknown binding.
    pub fn default_binding(name: &str) -> Option<i32> {
        BINDING_TABLE
            .iter()
            .find(|(known, _, _)| *known == name)
            .map(|(_, _, code)| *code)
    }

    /// The binding's category key, or `None` for an unknown binding.
    pub fn binding_category(name: &str) -> Option<&'static str> {
        BINDING_TABLE
            .iter()
            .find(|(known, _, _)| *known == name)
            .map(|(_, category, _)| *category)
    }

    /// The code `name` holds now, falling back to its default when the row
    /// is absent (an unknown name reads `0`, unbound).
    pub fn binding(&self, name: &str) -> i32 {
        self.bindings
            .iter()
            .find(|binding| binding.name == name)
            .map(|binding| binding.code)
            .or_else(|| Self::default_binding(name))
            .unwrap_or(0)
    }

    /// Writes `code` into `name`'s row, clamped into the storable range;
    /// an unknown name writes nothing and answers false
    /// (`setOptionKeyBinding`, `GameSettings.java`:244-248).
    pub fn set_binding(&mut self, name: &str, code: i32) -> bool {
        let Some(row) = self.bindings.iter_mut().find(|row| row.name == name) else {
            return false;
        };
        row.code = clamp_binding(code);
        true
    }

    /// Restores every row's default (`GuiControls.java`:79-87).
    pub fn reset_bindings(&mut self) {
        self.bindings = default_bindings();
    }

    /// Whether any row differs from its default: the Reset-All button's own
    /// enablement (`GuiControls.java`:162-173).
    pub fn bindings_differ(&self) -> bool {
        self.bindings != default_bindings()
    }

    /// Builds the wire client-settings payload: the locale, the effective
    /// view distance, the chat mode, chat colours, and the skin-parts byte
    /// (`sendSettingsToServer`, `GameSettings.java`:1183-1195).
    pub fn to_client_settings(&self) -> ClientSettings {
        ClientSettings {
            locale: self.language.clone(),
            view_distance: self.view_distance_chunks(),
            chat_mode: self.chat_mode.min(CHAT_MODE_RANGE.1),
            chat_colors: self.chat_colors,
            skin_parts: self.skin_parts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxide_proto_v47::serverbound::write_client_settings;

    fn scoped_file(name: &str, body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("test scratch dir");
        let path = dir.path().join(name);
        fs::write(&path, body).expect("test fixture");
        (dir, path)
    }

    fn with_render_distance(value: u8) -> GameSettings {
        GameSettings {
            render_distance: value,
            ..Default::default()
        }
    }

    fn non_default() -> GameSettings {
        GameSettings {
            render_distance: 16,
            brightness: 50,
            particles: ParticleSetting::Decreased,
            gui_scale: 2,
            fov: 90,
            max_framerate: 60,
            mipmap_levels: 2,
            vsync: false,
            view_bobbing: false,
            use_vbos: true,
            alternate_blocks: false,
            entity_shadows: false,
            clouds: CloudSetting::Fast,
            graphics: GraphicsSetting::Fast,
            smooth_lighting: 1,
            anaglyph: true,
            fullscreen: true,
            realms_notifications: false,
            music_volume: 0.25,
            sound_volume: 0.75,
            category_volumes: [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9],
            mouse_sensitivity: 0.75,
            invert_mouse: true,
            smooth_camera: true,
            chat_opacity: 0.5,
            chat_scale: 0.75,
            chat_width: 0.5,
            chat_height: 0.8,
            chat_height_unfocused: 0.6,
            chat_links: false,
            chat_links_prompt: false,
            chat_colors: false,
            chat_mode: 1,
            skin_parts: 0x40,
            language: "de_DE".to_string(),
            force_unicode: true,
            reduced_debug_info: true,
            bindings: default_bindings(),
            last_server: Some("play.example.test:25565".to_string()),
        }
    }

    #[test]
    fn defaults_match_source() {
        let s = GameSettings::default();
        assert_eq!(s.render_distance, 12);
        assert_eq!(s.brightness, 0);
        assert_eq!(s.particles, ParticleSetting::All);
        assert_eq!(s.gui_scale, 0);
        assert_eq!(s.fov, 70);
        assert_eq!(s.max_framerate, 120);
        assert_eq!(s.mipmap_levels, 4);
        assert!(s.vsync);
        assert!(s.view_bobbing);
        assert!(!s.use_vbos);
        assert!(s.alternate_blocks);
        assert!(s.entity_shadows);
        assert_eq!(s.clouds, CloudSetting::Fancy);
        assert_eq!(s.graphics, GraphicsSetting::Fancy);
        assert_eq!(s.smooth_lighting, 2);
        assert!(!s.anaglyph);
        assert!(!s.fullscreen);
        assert!(s.realms_notifications);
        assert_eq!(s.music_volume, 1.0);
        assert_eq!(s.sound_volume, 1.0);
        assert_eq!(s.category_volumes, [1.0; 9]);
        assert_eq!(s.mouse_sensitivity, 0.5);
        assert!(!s.invert_mouse);
        assert!(!s.smooth_camera);
        assert_eq!(s.chat_opacity, 1.0);
        assert_eq!(s.chat_scale, 1.0);
        assert_eq!(s.chat_width, 1.0);
        assert_eq!(s.chat_height, 1.0);
        assert_eq!(s.chat_height_unfocused, 0.44366196);
        assert!(s.chat_links);
        assert!(s.chat_links_prompt);
        assert!(s.chat_colors);
        assert_eq!(s.chat_mode, 0);
        assert_eq!(s.skin_parts, 0x7F);
        assert_eq!(s.language, "en_US");
        assert!(!s.force_unicode);
        assert!(!s.reduced_debug_info);
        assert_eq!(s.bindings, default_bindings());
        assert_eq!(s.last_server, None);
    }

    #[test]
    fn view_distance_range_is_source_derived() {
        assert_eq!(VIEW_DISTANCE_RANGE, (2, 32));
    }

    #[test]
    fn category_names_follow_sound_category_order() {
        assert_eq!(
            CATEGORY_NAMES,
            [
                "master", "music", "record", "weather", "block", "hostile", "neutral", "player",
                "ambient"
            ]
        );
    }

    #[test]
    fn framerate_unlimited_sentinel() {
        assert_eq!(FRAMERATE_UNLIMITED, 255);
        assert_ne!(GameSettings::default().max_framerate, FRAMERATE_UNLIMITED);
    }

    #[test]
    fn view_distance_clamps_at_range_ends() {
        assert_eq!(with_render_distance(2).view_distance_chunks(), 2);
        assert_eq!(with_render_distance(32).view_distance_chunks(), 32);
        assert_eq!(with_render_distance(1).view_distance_chunks(), 2);
        assert_eq!(with_render_distance(33).view_distance_chunks(), 32);
    }

    #[test]
    fn far_plane_is_chunks_times_sixteen() {
        assert_eq!(GameSettings::default().far_plane(), 192.0);
        assert_eq!(with_render_distance(100).far_plane(), 512.0);
    }

    #[test]
    fn gui_scale_index_clamps() {
        let mut s = GameSettings::default();
        s.set_gui_scale_index(2);
        assert_eq!(s.gui_scale, 2);
        s.set_gui_scale_index(0);
        assert_eq!(s.gui_scale, 0);
        s.set_gui_scale_index(3);
        assert_eq!(s.gui_scale, 3);
        s.set_gui_scale_index(4);
        assert_eq!(s.gui_scale, 3);
        s.set_gui_scale_index(255);
        assert_eq!(s.gui_scale, 3);
    }

    #[test]
    fn wire_mapping_pins_c15_fields() {
        let s = non_default();
        let wire = s.to_client_settings();
        assert_eq!(wire.locale, "de_DE");
        assert_eq!(wire.view_distance, 16);
        assert_eq!(wire.chat_mode, 1);
        assert!(!wire.chat_colors);
        assert_eq!(wire.skin_parts, 0x40);
        let mut bytes = Vec::new();
        write_client_settings(&mut bytes, &wire).expect("writer takes the mapping");
        assert_eq!(&bytes[0..1], &[0x15]);
    }

    #[test]
    fn default_wire_mapping_carries_64bit_view_distance() {
        let wire = GameSettings::default().to_client_settings();
        assert_eq!(wire.locale, "en_US");
        assert_eq!(wire.view_distance, 12);
        assert_eq!(wire.chat_mode, 0);
        assert!(wire.chat_colors);
        assert_eq!(wire.skin_parts, 0x7F);
    }

    #[test]
    fn round_trip_non_default_instance() {
        let dir = tempfile::tempdir().expect("test scratch dir");
        let path = dir.path().join("options.toml");
        let before = non_default();
        before.save(&path).expect("save works");
        let after = GameSettings::load(&path);
        assert_eq!(before, after);
    }

    #[test]
    fn save_format_is_sorted_and_stable() {
        let dir = tempfile::tempdir().expect("test scratch dir");
        let path = dir.path().join("options.toml");
        let s = non_default();
        s.save(&path).expect("first save");
        let first = fs::read_to_string(&path).expect("read back");
        s.save(&path).expect("second save");
        let second = fs::read_to_string(&path).expect("read back");
        assert_eq!(first, second);
        let keys: Vec<&str> = first
            .lines()
            .map(|line| line.split('=').next().unwrap_or("").trim())
            .collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted);
        assert!(keys.len() >= 40, "every field persists, got {}", keys.len());
        for name in CATEGORY_NAMES {
            assert!(
                first.contains(&format!("category_{name} = ")),
                "missing category key for {name}"
            );
        }
    }

    #[test]
    fn missing_file_is_defaults() {
        let dir = tempfile::tempdir().expect("test scratch dir");
        let loaded = GameSettings::load(dir.path().join("no-such-file.toml"));
        assert_eq!(loaded, GameSettings::default());
    }

    #[test]
    fn save_publishes_atomically_with_no_staging_left_behind() {
        let dir = tempfile::tempdir().expect("test scratch dir");
        let path = dir.path().join("options.toml");
        let before = non_default();
        before.save(&path).expect("save works");
        assert!(
            !dir.path().join("options.tmp").exists(),
            "the staging file renames away"
        );
        let after = GameSettings::load(&path);
        assert_eq!(before, after);
    }

    #[test]
    fn empty_file_is_defaults() {
        let (_dir, path) = scoped_file("empty.toml", "");
        assert_eq!(GameSettings::load(&path), GameSettings::default());
    }

    #[test]
    fn garbage_lines_are_skipped() {
        let body = "this is not a setting\n:::===\n= \n   \n# comment\nrender_distance = 16\n";
        let (_dir, path) = scoped_file("garbage.toml", body);
        let loaded = GameSettings::load(&path);
        assert_eq!(loaded.render_distance, 16);
        assert_eq!(loaded.fov, 70);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let body = "fov = 90\nresource_packs = [\"x\"]\nstreamKbps = 3\n";
        let (_dir, path) = scoped_file("unknown.toml", body);
        let loaded = GameSettings::load(&path);
        assert_eq!(loaded.fov, 90);
        assert_eq!(
            loaded,
            GameSettings {
                fov: 90,
                ..Default::default()
            }
        );
    }

    #[test]
    fn wrong_types_keep_defaults() {
        let body = "fov = abc\nvsync = maybe\nmouse_sensitivity = lots\nchat_mode = full\nskin_parts = 0x7F\n";
        let (_dir, path) = scoped_file("types.toml", body);
        let loaded = GameSettings::load(&path);
        assert_eq!(loaded, GameSettings::default());
    }

    #[test]
    fn out_of_range_values_clamp() {
        let body = [
            "render_distance = 99",
            "brightness = 200",
            "fov = 5",
            "mipmap_levels = 9",
            "smooth_lighting = 9",
            "chat_mode = 7",
            "particles = 9",
            "clouds = 9",
            "max_framerate = 260",
            "sound_volume = 2.5",
            "music_volume = -1.0",
            "mouse_sensitivity = 9.0",
            "chat_opacity = -0.5",
            "skin_parts = 300",
            "category_master = 7.0",
        ]
        .join("\n");
        let (_dir, path) = scoped_file("ranges.toml", &body);
        let loaded = GameSettings::load(&path);
        assert_eq!(loaded.render_distance, 32);
        assert_eq!(loaded.view_distance_chunks(), 32);
        assert_eq!(loaded.brightness, 100);
        assert_eq!(loaded.fov, 30);
        assert_eq!(loaded.mipmap_levels, 4);
        assert_eq!(loaded.smooth_lighting, 2);
        assert_eq!(loaded.chat_mode, 2);
        assert_eq!(loaded.particles, ParticleSetting::Minimal);
        assert_eq!(loaded.clouds, CloudSetting::Fancy);
        assert_eq!(loaded.max_framerate, FRAMERATE_UNLIMITED);
        assert_eq!(loaded.sound_volume, 1.0);
        assert_eq!(loaded.music_volume, 0.0);
        assert_eq!(loaded.mouse_sensitivity, 1.0);
        assert_eq!(loaded.chat_opacity, 0.0);
        assert_eq!(loaded.skin_parts, 255);
        assert_eq!(loaded.category_volumes[0], 1.0);
    }

    #[test]
    fn bool_and_enum_names_parse() {
        let body = [
            "graphics = fast",
            "clouds = off",
            "particles = minimal",
            "vsync = false",
            "graphics = fast",
        ]
        .join("\n");
        let (_dir, path) = scoped_file("names.toml", &body);
        let loaded = GameSettings::load(&path);
        assert_eq!(loaded.graphics, GraphicsSetting::Fast);
        assert_eq!(loaded.clouds, CloudSetting::Off);
        assert_eq!(loaded.particles, ParticleSetting::Minimal);
        assert!(!loaded.vsync);
        let body = "graphics = fancy\nclouds = fast\nparticles = decreased\n";
        let (_dir, path) = scoped_file("names2.toml", body);
        let loaded = GameSettings::load(&path);
        assert_eq!(loaded.graphics, GraphicsSetting::Fancy);
        assert_eq!(loaded.clouds, CloudSetting::Fast);
        assert_eq!(loaded.particles, ParticleSetting::Decreased);
    }

    #[test]
    fn language_and_last_server() {
        let body = "language = \"fr_FR\"\nlast_server = \"mc.example:25566\"\n";
        let (_dir, path) = scoped_file("locale.toml", body);
        let loaded = GameSettings::load(&path);
        assert_eq!(loaded.language, "fr_FR");
        assert_eq!(loaded.last_server, Some("mc.example:25566".to_string()));
        let body = "last_server = \"\"\n";
        let (_dir, path) = scoped_file("empty-server.toml", body);
        assert_eq!(GameSettings::load(&path).last_server, None);
    }

    #[test]
    fn the_default_binding_table_pins_thirty_three_source_rows() {
        // `GameSettings.java`:127-151: 24 named fields plus the 9-hotbar
        // array, in the constructor's array order.
        let bindings = default_bindings();
        assert_eq!(bindings.len(), BINDING_COUNT);
        assert_eq!(
            bindings[0],
            Binding {
                name: "attack",
                code: -100
            }
        );
        assert_eq!(
            bindings[1],
            Binding {
                name: "use",
                code: -99
            }
        );
        assert_eq!(
            bindings[2],
            Binding {
                name: "forward",
                code: 17
            }
        );
        assert_eq!(
            bindings[3],
            Binding {
                name: "left",
                code: 30
            }
        );
        assert_eq!(
            bindings[4],
            Binding {
                name: "back",
                code: 31
            }
        );
        assert_eq!(
            bindings[5],
            Binding {
                name: "right",
                code: 32
            }
        );
        assert_eq!(
            bindings[6],
            Binding {
                name: "jump",
                code: 57
            }
        );
        assert_eq!(
            bindings[7],
            Binding {
                name: "sneak",
                code: 42
            }
        );
        assert_eq!(
            bindings[8],
            Binding {
                name: "sprint",
                code: 29
            }
        );
        assert_eq!(
            bindings[9],
            Binding {
                name: "drop",
                code: 16
            }
        );
        assert_eq!(
            bindings[10],
            Binding {
                name: "inventory",
                code: 18
            }
        );
        assert_eq!(
            bindings[11],
            Binding {
                name: "chat",
                code: 20
            }
        );
        assert_eq!(
            bindings[12],
            Binding {
                name: "playerlist",
                code: 15
            }
        );
        assert_eq!(
            bindings[13],
            Binding {
                name: "pickItem",
                code: -98
            }
        );
        assert_eq!(
            bindings[14],
            Binding {
                name: "command",
                code: 53
            }
        );
        assert_eq!(
            bindings[15],
            Binding {
                name: "screenshot",
                code: 60
            }
        );
        assert_eq!(
            bindings[16],
            Binding {
                name: "togglePerspective",
                code: 63
            }
        );
        assert_eq!(
            bindings[17],
            Binding {
                name: "smoothCamera",
                code: 0
            }
        );
        assert_eq!(
            bindings[18],
            Binding {
                name: "streamStartStop",
                code: 64
            }
        );
        assert_eq!(
            bindings[19],
            Binding {
                name: "streamPauseUnpause",
                code: 65
            }
        );
        assert_eq!(
            bindings[20],
            Binding {
                name: "streamCommercial",
                code: 0
            }
        );
        assert_eq!(
            bindings[21],
            Binding {
                name: "streamToggleMic",
                code: 0
            }
        );
        assert_eq!(
            bindings[22],
            Binding {
                name: "fullscreen",
                code: 87
            }
        );
        assert_eq!(
            bindings[23],
            Binding {
                name: "spectatorOutlines",
                code: 0
            }
        );
        for (slot, binding) in bindings[24..].iter().enumerate() {
            assert_eq!(binding.name, BINDING_TABLE[24 + slot].0);
            assert_eq!(
                binding.code,
                2 + slot as i32,
                "hotbar slot {} reads code {}",
                slot + 1,
                2 + slot as i32
            );
        }
        let categories: Vec<&str> = BINDING_TABLE
            .iter()
            .map(|(_, category, _)| *category)
            .collect();
        assert!(categories.contains(&"key.categories.movement"));
        assert!(categories.contains(&"key.categories.gameplay"));
        assert!(categories.contains(&"key.categories.inventory"));
        assert!(categories.contains(&"key.categories.multiplayer"));
        assert!(categories.contains(&"key.categories.misc"));
        assert!(categories.contains(&"key.categories.stream"));
    }

    #[test]
    fn binding_writes_clamp_and_reset_restores() {
        let mut settings = GameSettings::default();
        assert!(!settings.bindings_differ());
        assert!(settings.set_binding("forward", 33));
        assert_eq!(settings.binding("forward"), 33);
        assert!(settings.bindings_differ());
        assert!(settings.set_binding("forward", 9999));
        assert_eq!(settings.binding("forward"), BINDING_CODE_MAX);
        assert!(settings.set_binding("attack", -9999));
        assert_eq!(settings.binding("attack"), BINDING_CODE_MIN);
        assert!(!settings.set_binding("no_such_key", 5));
        assert_eq!(settings.binding("no_such_key"), 0);
        settings.reset_bindings();
        assert!(!settings.bindings_differ());
        assert_eq!(settings.binding("forward"), 17);
    }

    #[test]
    fn bindings_persist_per_row_through_the_same_file() {
        let dir = tempfile::tempdir().expect("test scratch dir");
        let path = dir.path().join("options.toml");
        let mut before = GameSettings::default();
        assert!(before.set_binding("forward", 33));
        assert!(before.set_binding("sprint", 0));
        before.chat_height_unfocused = 0.6;
        before.chat_links = false;
        before.reduced_debug_info = true;
        before.save(&path).expect("save works");
        let text = fs::read_to_string(&path).expect("read back");
        assert!(
            text.contains("key_forward = 33\n"),
            "one row per binding:\n{text}"
        );
        assert!(
            text.contains("chat_height_unfocused = 0.6\n"),
            "text:\n{text}"
        );
        let after = GameSettings::load(&path);
        assert_eq!(before, after);
        assert_eq!(after.binding("forward"), 33);
        assert_eq!(after.binding("sprint"), 0);
        assert!(after.bindings_differ());
    }

    #[test]
    fn binding_rows_clamp_and_ignore_unknown_names() {
        let body =
            "key_forward = 9999\nkey_attack = -5000\nkey_nope = 12\nkey_forward = notanumber\n";
        let (_dir, path) = scoped_file("keys.toml", body);
        let loaded = GameSettings::load(&path);
        assert_eq!(loaded.binding("forward"), BINDING_CODE_MAX);
        assert_eq!(loaded.binding("attack"), BINDING_CODE_MIN);
        assert_eq!(loaded.binding("sprint"), 29);
    }
}
