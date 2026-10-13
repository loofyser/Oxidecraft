//! The main menu: the logo, the splash, the version lines and the button
//! set over the panorama.
//!
//! The port reads values and names from the source's main menu
//! (`client/gui/GuiMainMenu.java`); no source text is copied.
//!
//! Layout (`initGui`, `addSingleplayerMultiplayerButtons`, with `j =
//! height/4 + 48`, spacing 24): singleplayer (1) at `w/2-100, j` 200x20,
//! multiplayer (2) at `j+24`, options (0) at `w/2-100, j+84` 98x20, quit
//! (4) at `w/2+2, j+84` 98x20. Realms (14) and the language button (5)
//! are OMITTED (recorded: the language screen lands in Task 26, realms
//! never ships) — and so are the OpenGL-warning and
//! realms-notification overlays.
//!
//! Singleplayer draws enabled and records inert on press (Decision 7):
//! the world code it would open does not exist, so the press answers the
//! action and opens nothing — the LAN-inert precedent (Task 17).
//!
//! The panorama is a port simplification (recorded): the source renders
//! the six faces as a 3D cube (FOV 120, base rotate 180 X + 90 Z) through
//! an 8x8 blur grid x7 into a 256 viewport, then stretches the result
//! over the screen (`drawPanorama`, `rotateAndBlurSkybox`, `renderSkybox`)
//! — which has no equivalent on this renderer's ortho screen pass — so
//! the port draws the yaw-facing side face's full-face blit beneath the
//! source's own two gradient rects. The load-bearing part is exact: the
//! yaw/pitch laws with the source's own rates (yaw `-(timer+partial)*0.1`
//! at 20 Hz = 2 deg/s, a 180 s period; pitch
//! `sin((timer+partial)/400)*25+20`), the six 256x256 faces from the
//! store, and the gradient pair on top.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use oxide_assets::font::Font;
use oxide_game::chat::LanguageTable;
use oxide_render::hud::{HudDraw, HudTexture, ScaledResolution};
use oxide_render::text::string_width;

use super::widgets::Button;

/// The compatibility line beside the logo: the exact source string
/// (`drawScreen`:568 — `mc.isDemo()` never holds here, so no ` Demo`
/// suffix).
pub const MENU_VERSION: &str = "Minecraft 1.8.9";
/// The copyright line: the exact source string (`drawScreen`:576).
pub const MENU_COPYRIGHT: &str = "Copyright Mojang AB. Do not distribute!";
/// The splash before the file reads: `"missingno"` (the constructor's
/// default, kept when the read fails).
pub const SPLASH_DEFAULT: &str = "missingno";
/// The splash re-roll rule, verbatim: while the pick's `hashCode` equals
/// this, roll again (`GuiMainMenu.java`:119). The splash it names is
/// [`SPLASH_EXCLUDED_TEXT`].
pub const SPLASH_EXCLUDED_HASH: i32 = 125780783;
/// The splash the re-roll rule excludes: the one line of the store's
/// `texts/splashes.txt` whose Java `hashCode` is
/// [`SPLASH_EXCLUDED_HASH`].
pub const SPLASH_EXCLUDED_TEXT: &str =
    "This message will never appear on the splash screen, isn't that weird?";
/// The splash's angle about Z: the source's own −20 degrees
/// (`drawScreen`:562 — the plan's "45" is corrected here).
pub const SPLASH_ANGLE_DEG: f32 = -20.0;
/// The logo's width: 274 (`drawScreen`:537-540).
pub const LOGO_WIDTH: f32 = 274.0;
/// The logo's top edge: 30 (see [`LOGO_WIDTH`]).
pub const LOGO_TOP: f32 = 30.0;
/// The logo sheet: the extraction tree's `gui/title/minecraft.png`.
pub const LOGO_SHEET: &str = "gui/title/minecraft";
/// The panorama faces: the extraction tree's six 256x256
/// `gui/title/background/panorama_N.png`, extensionless store keys.
pub const PANORAMA_SHEETS: [&str; 6] = [
    "gui/title/background/panorama_0",
    "gui/title/background/panorama_1",
    "gui/title/background/panorama_2",
    "gui/title/background/panorama_3",
    "gui/title/background/panorama_4",
    "gui/title/background/panorama_5",
];
/// The singleplayer button's id (`addSingleplayerMultiplayerButtons`).
pub const MENU_SINGLEPLAYER_ID: i32 = 1;
/// The multiplayer button's id (see [`MENU_SINGLEPLAYER_ID`]).
pub const MENU_MULTIPLAYER_ID: i32 = 2;
/// The options button's id (`initGui`).
pub const MENU_OPTIONS_ID: i32 = 0;
/// The quit button's id (see [`MENU_OPTIONS_ID`]).
pub const MENU_QUIT_ID: i32 = 4;
/// The splash file in the extraction tree.
pub const SPLASH_FILE: &str = "assets/minecraft/texts/splashes.txt";
/// The main-menu gradient pair's top colour (`drawScreen`:541-542 —
/// `-2130706433`, 0x81010101).
pub const MENU_GRADIENT_TOP: [f32; 4] = [1.0 / 255.0, 1.0 / 255.0, 1.0 / 255.0, 129.0 / 255.0];
/// The main-menu gradient pair's bottom colour (`:541-542` — `16777215`,
/// 0x00FFFFFF).
pub const MENU_GRADIENT_BOTTOM: [f32; 4] = [1.0, 1.0, 1.0, 0.0];
/// The main-menu gradient pair's second top colour (`:542` — `0`).
pub const MENU_GRADIENT2_TOP: [f32; 4] = [0.0, 0.0, 0.0, 0.0];
/// The main-menu gradient pair's second bottom colour (`:542` —
/// `Integer.MIN_VALUE`, 0x80000000).
pub const MENU_GRADIENT2_BOTTOM: [f32; 4] = [0.0, 0.0, 0.0, 128.0 / 255.0];
/// The splash's yellow: `-256` (`drawScreen`:566, 0xFFFFFF00).
pub const SPLASH_COLOUR: [f32; 4] = [1.0, 1.0, 0.0, 1.0];
/// The version lines' white: `-1` (`drawScreen`:573/:577, 0xFFFFFFFF).
pub const VERSION_COLOUR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// One action a menu press answers: the button's id routed through the
/// source's `actionPerformed` arms. Singleplayer records inert (Decision
/// 7); multiplayer and options await their screens (Tasks 16/25 carry
/// them); quit closes the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    /// Singleplayer: drawn enabled, opens nothing.
    Singleplayer,
    /// Multiplayer: its screen lands later; the press records the action.
    Multiplayer,
    /// Options: its screen lands later; the press records the action.
    Options,
    /// Quit: `mc.shutdown()` — the window closes.
    Quit,
}

/// Java's `String.hashCode`: the UTF-16 code units folded as
/// `s[0]*31^(n-1) + ...` in i32-wrapping arithmetic. The splash re-roll
/// rule reads it, so the port carries the exact law rather than a
/// lookalike.
pub fn java_hash(text: &str) -> i32 {
    let mut hash: i32 = 0;
    for unit in text.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    hash
}

/// Java's `Random`: the 48-bit LCG behind the launch-seeded splash pick
/// and the logo seed (`Random.java`: the `(seed ^ 0x5DEECE66D) &
/// 0xFFFFFFFFFFFF` scramble, `seed = (seed * 0x5DEECE66D + 0xB) &
/// 0xFFFFFFFFFFFF` per draw, the top 31 bits per `next(31)`).
struct JavaRandom {
    /// The 48-bit state.
    state: u64,
}

impl JavaRandom {
    /// The multiplier (`0x5DEECE66D`).
    const MULTIPLIER: u64 = 0x5DEECE66D;
    /// The addend (`0xB`).
    const ADDEND: u64 = 0xB;
    /// The 48-bit mask.
    const MASK: u64 = (1 << 48) - 1;

    /// Stands the stream on the launch seed (the constructor's scramble).
    fn new(seed: u64) -> Self {
        Self {
            state: (seed ^ Self::MULTIPLIER) & Self::MASK,
        }
    }

    /// The next `bits` pseudo-random bits (the `next` step).
    fn next(&mut self, bits: u32) -> u32 {
        self.state = (self
            .state
            .wrapping_mul(Self::MULTIPLIER)
            .wrapping_add(Self::ADDEND))
            & Self::MASK;
        (self.state >> (48 - bits)) as u32
    }

    /// `nextInt(bound)`: the power-of-two fast path and the rejection
    /// loop that keeps the distribution uniform (`Random.java`:267-305).
    fn next_int(&mut self, bound: i32) -> i32 {
        let bound = bound as u32;
        if bound.is_power_of_two() {
            return ((bound as u64 * u64::from(self.next(31))) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let value = bits % bound;
            if bits.wrapping_sub(value).wrapping_add(bound - 1) < 1 << 31 {
                return value as i32;
            }
        }
    }

    /// `nextFloat()`: `next(24) / 2^24` (`Random.java`:307-324) — the
    /// logo seed's law (`updateCounter = RANDOM.nextFloat()`, :145).
    fn next_float(&mut self) -> f32 {
        (self.next(24) as f32) / ((1 << 24) as f32)
    }
}

/// Java's `Random.nextInt(bound)` from the launch seed: one draw of the
/// stream [`pick_splash`] reads repeatedly.
pub fn java_pick(seed: u64, bound: i32) -> i32 {
    JavaRandom::new(seed).next_int(bound)
}

/// Picks the splash from the file's lines: trim, skip empties, default
/// [`SPLASH_DEFAULT`] when nothing survives, and re-roll while the pick's
/// hash is [`SPLASH_EXCLUDED_HASH`] (the constructor's loop, verbatim).
pub fn pick_splash(lines: &[String], seed: u64) -> String {
    let kept: Vec<String> = lines
        .iter()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    if kept.is_empty() {
        return SPLASH_DEFAULT.to_string();
    }
    let mut random = JavaRandom::new(seed);
    loop {
        let pick = kept[random.next_int(kept.len() as i32) as usize].clone();
        if java_hash(&pick) != SPLASH_EXCLUDED_HASH {
            return pick;
        }
    }
}

/// The calendar override, or `None` outside the three windows
/// (`initGui`: the December-24, January-1 and October-31 arms).
pub fn calendar_override(month: u32, day: u32) -> Option<&'static str> {
    if month == 12 && day == 24 {
        Some("Merry X-mas!")
    } else if month == 1 && day == 1 {
        Some("Happy new year!")
    } else if month == 10 && day == 31 {
        Some("OOoooOOOoooo! Spooky!")
    } else {
        None
    }
}

/// The civil month and day for days since the Unix epoch (Howard
/// Hinnant's algorithm): the calendar override's inputs from the wall
/// clock, with no date dependency.
pub fn civil_month_day(days: i64) -> (u32, u32) {
    let days = days + 719468;
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let ordinal = (days - era * 146097) as u32;
    let year_of_era = (ordinal - ordinal / 1460 + ordinal / 36524 - ordinal / 146096) / 365;
    let day_of_year = ordinal - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    };
    (month, day)
}

/// Today's civil month and day from the wall clock, or `None` before the
/// epoch (the calendar override stays out then).
fn today_month_day() -> Option<(u32, u32)> {
    let days = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as i64 / 86400;
    Some(civil_month_day(days))
}

/// The splash's scale: `(1.8 - |sin(2*pi*(ms%1000)/1000)| * 0.1) *
/// 100/(width+32)` (`drawScreen`:563-564 — a 1 s period, ±0.1/1.8
/// amplitude). `text_width` is the font's `getStringWidth` for the
/// splash.
pub fn splash_scale(now_ms: u64, text_width: i32) -> f32 {
    let phase = (now_ms % 1000) as f32 / 1000.0 * std::f32::consts::TAU;
    let bounce = 1.8 - phase.sin().abs() * 0.1;
    bounce * 100.0 / (text_width + 32) as f32
}

/// The panorama's yaw in degrees: `-(timer+partial) * 0.1`
/// (`drawPanorama`:403) — at the 20 Hz `updateScreen` tick
/// (`++panoramaTimer`, :164-166) that is 2 deg/s, a 180 s period.
pub fn panorama_yaw(timer: i64, partial: f32) -> f32 {
    -(timer as f32 + partial) * 0.1
}

/// The panorama's pitch in degrees: `sin((timer+partial)/400) * 25 + 20`
/// (`drawPanorama`:402) — a ~1257 s period.
pub fn panorama_pitch(timer: i64, partial: f32) -> f32 {
    ((timer as f32 + partial) / 400.0).sin() * 25.0 + 20.0
}

/// The side face the yaw looks through: the yaw (negative-growing)
/// folded into `[0, 360)` and split into 60-degree sextants over faces
/// 0..5, so the six stored faces all sample in one revolution. A port
/// simplification (recorded): the source turns the whole cube under a
/// fixed camera, so no sextant maps one face exactly — the pin is that
/// the six sextants sample the six faces in order.
pub fn panorama_face(yaw_deg: f32) -> usize {
    (((-yaw_deg).rem_euclid(360.0) / 60.0).floor() as usize) % 6
}

/// The menu's button set at the scaled size: `j = h/4+48`, spacing 24 —
/// singleplayer (1) `w/2-100, j` 200x20, multiplayer (2) `j+24`,
/// options (0) `w/2-100, j+84` 98x20, quit (4) `w/2+2, j+84` 98x20.
/// Realms (14) and language (5) are omitted (recorded). Widths halve by
/// integer division like the source's `width / 2`.
pub fn menu_buttons(
    scaled: &ScaledResolution,
    single: String,
    multi: String,
    options: String,
    quit: String,
) -> Vec<Button> {
    let half = scaled.width / 2;
    let j = scaled.height / 4 + 48;
    vec![
        Button::new(
            MENU_SINGLEPLAYER_ID,
            (half - 100) as f32,
            j as f32,
            200.0,
            20.0,
            single,
        ),
        Button::new(
            MENU_MULTIPLAYER_ID,
            (half - 100) as f32,
            (j + 24) as f32,
            200.0,
            20.0,
            multi,
        ),
        Button::new(
            MENU_OPTIONS_ID,
            (half - 100) as f32,
            (j + 84) as f32,
            98.0,
            20.0,
            options,
        ),
        Button::new(
            MENU_QUIT_ID,
            (half + 2) as f32,
            (j + 84) as f32,
            98.0,
            20.0,
            quit,
        ),
    ]
}

/// Whether the menu stands at startup: it does when no `--server` was
/// given; the `--server` route joins the session and bypasses it.
pub fn stands_at_startup(server: Option<&str>) -> bool {
    server.is_none()
}

/// The main menu's state: the button set, the splash, the panorama clock
/// and the logo seed.
#[derive(Debug, Clone)]
pub struct MainMenuScreen {
    /// The button set, laid out at the last `set_screen_size`.
    buttons: Vec<Button>,
    /// The splash text: the pick, unless a calendar window overrides it.
    splash: String,
    /// The panorama clock: `++` per 20 Hz tick (`updateScreen`:164-166).
    panorama_timer: i64,
    /// The logo seed: `RANDOM.nextFloat()` at build (`initGui`'s caller
    /// :145); the rare branch draws while it sits below 1e-4 (:546).
    update_counter: f32,
    /// The last wall-clock 20 Hz tick, for the sessionless tick source.
    last_tick: Option<Instant>,
}

impl MainMenuScreen {
    /// Stands the menu: the buttons at `scaled`, the splash picked from
    /// the file's lines under `seed`, the panorama clock at 0 and the
    /// logo seed drawn from the launch. The splash pick and the logo
    /// seed read one launch stream in the source's order (the
    /// constructor's pick first, `updateCounter` at :145 after), and the
    /// calendar window overrides the pick (`initGui`'s arms).
    pub fn new(
        scaled: &ScaledResolution,
        splash_lines: &[String],
        seed: u64,
        labels: [String; 4],
    ) -> Self {
        let kept: Vec<String> = splash_lines
            .iter()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect();
        let mut random = JavaRandom::new(seed);
        let mut splash = if kept.is_empty() {
            SPLASH_DEFAULT.to_string()
        } else {
            loop {
                let pick = kept[random.next_int(kept.len() as i32) as usize].clone();
                if java_hash(&pick) != SPLASH_EXCLUDED_HASH {
                    break pick;
                }
            }
        };
        let update_counter = random.next_float();
        if let Some((month, day)) = today_month_day() {
            if let Some(holiday) = calendar_override(month, day) {
                splash = holiday.to_string();
            }
        }
        let [single, multi, options, quit] = labels;
        Self {
            buttons: menu_buttons(scaled, single, multi, options, quit),
            splash,
            panorama_timer: 0,
            update_counter,
            last_tick: None,
        }
    }

    /// The button set.
    pub fn buttons(&self) -> &[Button] {
        &self.buttons
    }

    /// The splash text.
    pub fn splash(&self) -> &str {
        &self.splash
    }

    /// The panorama clock.
    pub fn timer(&self) -> i64 {
        self.panorama_timer
    }

    /// Whether the rare logo branch draws: `updateCounter < 1e-4`
    /// (`drawScreen`:546).
    pub fn uses_rare_logo(&self) -> bool {
        f64::from(self.update_counter) < 1e-4
    }

    /// Steps the panorama clock once: the session tick's fold (the
    /// `updateScreen` 20 Hz source).
    pub fn tick(&mut self) {
        self.panorama_timer += 1;
    }

    /// Sets the panorama clock outright: the render tests' seam for
    /// pinning the yaw-facing face without waiting out the wall clock.
    /// The live menu never calls it — only `tick` and `tick_wall` move
    /// the clock there.
    pub fn set_panorama_timer(&mut self, timer: i64) {
        self.panorama_timer = timer;
    }

    /// Steps the sessionless clock: every 50 ms of wall time since the
    /// last tick (or the open) advances the panorama clock once, so the
    /// menu turns at the source's 20 Hz with no session behind it.
    /// Answers how many ticks elapsed.
    pub fn tick_wall(&mut self, now: Instant) -> u32 {
        let Some(last) = self.last_tick else {
            self.last_tick = Some(now);
            return 0;
        };
        let elapsed = now.saturating_duration_since(last);
        let ticks = (elapsed.as_millis() / 50) as u32;
        if ticks > 0 {
            self.panorama_timer += i64::from(ticks);
            self.last_tick = Some(last + Duration::from_millis(u64::from(ticks) * 50));
        }
        ticks
    }

    /// Re-lays the buttons at the new scaled size (`initGui` re-runs on
    /// resize: the `buttonList` is cleared and rebuilt).
    pub fn set_screen_size(&mut self, scaled: &ScaledResolution, labels: [String; 4]) {
        let [single, multi, options, quit] = labels;
        self.buttons = menu_buttons(scaled, single, multi, options, quit);
    }

    /// One press at the GUI-space point: the button under it answers its
    /// id's action (`mouseClicked` fires on press; only button 0 reaches
    /// here — the caller gates it).
    pub fn click_at(&self, point: (f32, f32)) -> Option<MenuAction> {
        let button = self.buttons.iter().find(|button| button.pressed(point))?;
        match button.id {
            MENU_SINGLEPLAYER_ID => Some(MenuAction::Singleplayer),
            MENU_MULTIPLAYER_ID => Some(MenuAction::Multiplayer),
            MENU_OPTIONS_ID => Some(MenuAction::Options),
            MENU_QUIT_ID => Some(MenuAction::Quit),
            _ => None,
        }
    }

    /// The menu's draws in painter's order: the yaw-facing panorama
    /// face's full-screen blit, the two full-screen gradient rects
    /// (`drawScreen`:541-542), the logo (normal two-piece or the rare
    /// five-piece branch), the splash at −20 degrees about
    /// `(w/2+90, 70)` scaled by the bounce law and centred at `(0, -8)`
    /// in yellow, the version lines (`:568-577`), then the buttons.
    /// `now_ms` is the frame's system time: the splash bounce's
    /// millisecond clock and the panorama's partial fraction ride it.
    pub fn draws(
        &self,
        font: &Font,
        scaled: &ScaledResolution,
        mouse: Option<(f32, f32)>,
        now_ms: u64,
    ) -> Vec<HudDraw> {
        let width = scaled.width as f32;
        let height = scaled.height as f32;
        let mut draws = Vec::new();
        // The panorama beneath everything: the yaw-facing side face's
        // full-screen blit (the port's recorded simplification of the
        // cube + blur chain); the partial fraction rides the millisecond
        // clock's own 50 ms tick.
        let partial = (now_ms % 50) as f32 / 50.0;
        let face = panorama_face(panorama_yaw(self.panorama_timer, partial));
        draws.push(HudDraw::TexturedRect {
            texture: HudTexture::Named(PANORAMA_SHEETS[face]),
            x: 0.0,
            y: 0.0,
            width,
            height,
            uv: [0.0, 0.0, 1.0, 1.0],
            colour: [1.0, 1.0, 1.0, 1.0],
        });
        // The two full-screen gradient rects (`drawScreen`:541-542): each
        // a vertical blend, halved into its top and bottom colours like
        // the screen background's own pair.
        for (top, bottom) in [
            (MENU_GRADIENT_TOP, MENU_GRADIENT_BOTTOM),
            (MENU_GRADIENT2_TOP, MENU_GRADIENT2_BOTTOM),
        ] {
            draws.push(HudDraw::Rect {
                x: 0.0,
                y: 0.0,
                width,
                height: height / 2.0,
                colour: top,
            });
            draws.push(HudDraw::Rect {
                x: 0.0,
                y: height / 2.0,
                width,
                height: height - height / 2.0,
                colour: bottom,
            });
        }
        // The logo at `(w/2-137, 30)`: the rare five-piece branch while
        // `updateCounter < 1e-4`, else the normal two-piece pair.
        let left = width / 2.0 - LOGO_WIDTH / 2.0;
        if self.uses_rare_logo() {
            for (dx, sx, w) in [
                (0.0, 0.0, 99.0),
                (99.0, 129.0, 27.0),
                (125.0, 126.0, 3.0),
                (128.0, 99.0, 26.0),
                (155.0, 0.0, 155.0),
            ] {
                let sy = if dx < 155.0 { 0.0 } else { 45.0 };
                draws.push(HudDraw::TexturedRect {
                    texture: HudTexture::Named(LOGO_SHEET),
                    x: left + dx,
                    y: LOGO_TOP,
                    width: w,
                    height: 44.0,
                    uv: [
                        sx / 256.0,
                        sy / 256.0,
                        (sx + w) / 256.0,
                        (sy + 44.0) / 256.0,
                    ],
                    colour: [1.0, 1.0, 1.0, 1.0],
                });
            }
        } else {
            for (dx, sy) in [(0.0, 0.0), (155.0, 45.0)] {
                draws.push(HudDraw::TexturedRect {
                    texture: HudTexture::Named(LOGO_SHEET),
                    x: left + dx,
                    y: LOGO_TOP,
                    width: 155.0,
                    height: 44.0,
                    uv: [0.0, sy / 256.0, 155.0 / 256.0, (sy + 44.0) / 256.0],
                    colour: [1.0, 1.0, 1.0, 1.0],
                });
            }
        }
        // The splash: translated to `(w/2+90, 70)`, rotated −20 about Z,
        // scaled by the bounce law, centred at `(0, -8)` in yellow
        // (`drawScreen`:560-567).
        let splash_width = string_width(font, &self.splash);
        let scale = splash_scale(now_ms, splash_width);
        let centre = (width / 2.0 + 90.0, 70.0);
        draws.push(HudDraw::RotatedText {
            text: self.splash.clone(),
            x: centre.0 - splash_width as f32 * scale / 2.0,
            y: centre.1 - 8.0 * scale,
            scale,
            colour: SPLASH_COLOUR,
            shadow: true,
            blend: true,
            angle_deg: SPLASH_ANGLE_DEG,
            cx: centre.0,
            cy: centre.1,
        });
        // The version lines: `Minecraft 1.8.9` bottom-left, the copyright
        // bottom-right (`drawScreen`:568-577).
        draws.push(HudDraw::Text {
            text: MENU_VERSION.to_string(),
            x: 2.0,
            y: height - 10.0,
            scale: 1.0,
            colour: VERSION_COLOUR,
            shadow: true,
            blend: true,
        });
        let copyright_width = string_width(font, MENU_COPYRIGHT);
        draws.push(HudDraw::Text {
            text: MENU_COPYRIGHT.to_string(),
            x: width - copyright_width as f32 - 2.0,
            y: height - 10.0,
            scale: 1.0,
            colour: VERSION_COLOUR,
            shadow: true,
            blend: true,
        });
        for button in &self.buttons {
            draws.extend(button.draws(font, mouse));
        }
        draws
    }
}

/// One menu label key through the handed-over language table: the
/// source's `I18n` lookup with its missing-key rule (the key itself,
/// `StringTranslate.tryTranslateKey`:110-114). With no table handed
/// over, the English the store's `en_US.lang` carries — the labels the
/// buttons stand on.
pub fn menu_labels(language: Option<&LanguageTable>) -> [String; 4] {
    const KEYS: [&str; 4] = [
        "menu.singleplayer",
        "menu.multiplayer",
        "menu.options",
        "menu.quit",
    ];
    const ENGLISH: [&str; 4] = ["Singleplayer", "Multiplayer", "Options...", "Quit Game"];
    let Some(table) = language else {
        return ENGLISH.map(str::to_string);
    };
    KEYS.map(|key| table.get(key).unwrap_or(key).to_string())
}
