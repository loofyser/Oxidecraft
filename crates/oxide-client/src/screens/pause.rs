//! The pause menu and the placeholder statistics, achievements and
//! snooper screens.
//!
//! The port reads values and names from the source's pause menu
//! (`client/gui/GuiIngameMenu.java`), its statistics screen
//! (`client/gui/achievement/GuiStats.java`), its achievements screen
//! (`client/gui/achievement/GuiAchievements.java`), its snooper screen
//! (`client/gui/GuiSnooper.java`) and the screen base's background
//! (`client/gui/GuiScreen.java`); no source text is copied.
//!
//! Layout (`GuiIngameMenu.initGui`, `i = -16`): Back to Game (4) 200x20 at
//! `w/2-100, h/4+8`; Achievements (5) + Statistics (6) as the 98x20 pair
//! at `h/4+32`; Options (0) + Open to LAN (7) as the 98x20 pair at
//! `h/4+80`; Disconnect (1) 200x20 at `h/4+104`. Widths halve by integer
//! division like the source's `width / 2`.
//!
//! Open to LAN is always DISABLED (R1): no integrated server exists, so
//! the `:38` enable rule is permanently false, and a press on it answers
//! nothing. Disconnect always reads "Disconnect" (R2): the `:27-30`
//! re-label never fires. The Realms arm is omitted, recorded (R3).
//!
//! The placeholders skip the C16 statistics request and the downloading
//! spinner (R4): no statistics writer exists in v1, so each stands its
//! frame, title, Done button and recorded placeholder body at once. The
//! statistics tab buttons and the snooper toggle are omitted, recorded
//! (R8): the tabs belong to the post-v1 grids, and the toggle would fork
//! the settings apply path. The snooper lands unreachable (R10): its only
//! opener is the options hub's button 104, so this round declares its
//! parent slot and the hub wires the Done return.
//!
//! The screens draw as [`HudDraw`](oxide_render::hud::HudDraw) records —
//! dirt tiles, text and the widget buttons — so no `HudDraw` growth was
//! needed for them (the T15 ruling carries).

use oxide_assets::font::Font;
use oxide_game::chat::LanguageTable;
use oxide_render::hud::{HudDraw, HudTexture, ScaledResolution};
use oxide_render::text::string_width;

use super::widgets::Button;

/// Back to Game's id (`initGui`'s second button).
pub const PAUSE_BACK_ID: i32 = 4;
/// Achievements' id (the upper 98-wide pair's left button).
pub const PAUSE_ACHIEVEMENTS_ID: i32 = 5;
/// Statistics' id (the upper 98-wide pair's right button).
pub const PAUSE_STATS_ID: i32 = 6;
/// Options' id (the lower 98-wide pair's left button).
pub const PAUSE_OPTIONS_ID: i32 = 0;
/// Open to LAN's id (the lower 98-wide pair's right button, always
/// disabled).
pub const PAUSE_LAN_ID: i32 = 7;
/// Disconnect's id (the 200-wide bottom button).
pub const PAUSE_DISCONNECT_ID: i32 = 1;
/// The statistics Done button's id (`GuiStats.createButtons`).
pub const STATS_DONE_ID: i32 = 0;
/// The achievements Done button's id (`GuiAchievements.initGui`).
pub const ACHIEVEMENTS_DONE_ID: i32 = 1;
/// The snooper Done button's id (`GuiSnooper.initGui`).
pub const SNOOPER_DONE_ID: i32 = 2;
/// The dirt tile's key: the extraction tree's `gui/options_background.png`,
/// the sheet `drawBackground` binds.
pub const DIRT_SHEET: &str = "gui/options_background";
/// The dirt tile's span in GUI pixels: `drawBackground` tiles 32 px
/// (`GuiScreen.java`:688-703).
pub const DIRT_SPAN: f32 = 32.0;
/// The dirt tile's tint: the `(64, 64, 64)` colour `drawBackground` inks
/// every vertex with.
pub const DIRT_TINT: [f32; 4] = [64.0 / 255.0, 64.0 / 255.0, 64.0 / 255.0, 1.0];
/// The pause title's height: `drawScreen` centres "Game menu" at `(w/2,
/// 40)`.
pub const PAUSE_TITLE_Y: f32 = 40.0;
/// The statistics title's height: the loaded draw centres the title at y
/// 20 (`GuiStats.drawScreen`).
pub const STATS_TITLE_Y: f32 = 20.0;
/// The achievements placeholder title's height: the loaded panel's own
/// title rides the panel, so the placeholder centres at the statistics
/// title's height (recorded).
pub const ACHIEVEMENTS_TITLE_Y: f32 = 20.0;
/// The snooper title's height: the draw centres the title at y 8
/// (`GuiSnooper.drawScreen`:103-117).
pub const SNOOPER_TITLE_Y: f32 = 8.0;
/// The title white: 16777215 (every title's `drawCenteredString` colour).
pub const TITLE_COLOUR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// The placeholder body grey: 8421504 (the snooper description's
/// `drawCenteredString` colour).
pub const BODY_COLOUR: [f32; 4] = [128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 1.0];
/// The statistics placeholder body (R11): an honest stand-in — no
/// statistics are tracked, and the full grids land after v1.
pub const STATS_BODY: &str = "Statistics are not tracked yet — the full grids land after v1.";
/// The achievements placeholder body (R11): an honest stand-in — no
/// achievements are tracked, and the full panel lands after v1.
pub const ACHIEVEMENTS_BODY: &str =
    "Achievements are not tracked yet — the full panel lands after v1.";
/// The snooper placeholder body (R11): an honest stand-in — no machine
/// usage is collected in this version.
pub const SNOOPER_BODY: &str = "Machine usage reporting is not collected in this version.";
/// The 98-wide pair's width: the two-column rows' buttons
/// (`initGui` builds them 98x20).
pub const PAUSE_PAIR_WIDTH: f32 = 98.0;

/// One action a pause press answers: the button's id routed through the
/// source's `actionPerformed` arms. LAN never answers — the disabled
/// button never presses (`GuiButton.mousePressed` gates it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseAction {
    /// Back to Game: the screen closes and the pointer recaptures
    /// (`displayGuiScreen(null)` + `setIngameFocus`).
    BackToGame,
    /// Achievements: the achievements screen stands over the pause.
    Achievements,
    /// Statistics: the statistics screen stands over the pause.
    Statistics,
    /// Options: the options hub lands later; the press records the action
    /// and opens nothing yet.
    Options,
    /// Disconnect: the standing disconnect teardown runs — the session
    /// ends and the main menu stands — and the press pins this id only.
    Disconnect,
}

/// One action a statistics press answers: Done returns to the pause
/// (`actionPerformed` id 0 → the parent screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsAction {
    /// Done: the pause parent stands again.
    Done,
}

/// One action an achievements press answers: Done returns to the pause
/// (`actionPerformed` id 1 → the parent screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AchievementsAction {
    /// Done: the pause parent stands again.
    Done,
}

/// One action a snooper press answers: Done returns to the declared
/// parent — the options hub, which wires the return when it lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnooperAction {
    /// Done: the declared parent stands again.
    Done,
}

/// The snooper's declared parent (R10): the screen its Done button
/// returns to. The only opener is the options hub's button 104
/// (`GuiOptions.java`:191-194), so the hub wires the return when it
/// lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnooperParent {
    /// The options hub (Task 18 owns it).
    Options,
}

/// The pause menu's button set at the scaled size: `i = -16`, Back to
/// Game (4) 200x20 at `h/4+8`, Achievements (5) + Statistics (6) as the
/// 98x20 pair at `h/4+32`, Options (0) + Open to LAN (7) as the 98x20
/// pair at `h/4+80`, Disconnect (1) 200x20 at `h/4+104`. `labels` runs
/// back, achievements, statistics, options, LAN, disconnect. LAN builds
/// disabled (R1); the rest build enabled and visible.
pub fn pause_buttons(scaled: &ScaledResolution, labels: [String; 6]) -> Vec<Button> {
    let half = scaled.width / 2;
    let quarter = scaled.height / 4;
    let [back, achievements, statistics, options, lan, disconnect] = labels;
    let mut lan_button = Button::new(
        PAUSE_LAN_ID,
        (half + 2) as f32,
        (quarter + 80) as f32,
        PAUSE_PAIR_WIDTH,
        20.0,
        lan,
    );
    lan_button.enabled = false;
    vec![
        Button::new(
            PAUSE_DISCONNECT_ID,
            (half - 100) as f32,
            (quarter + 104) as f32,
            200.0,
            20.0,
            disconnect,
        ),
        Button::new(
            PAUSE_BACK_ID,
            (half - 100) as f32,
            (quarter + 8) as f32,
            200.0,
            20.0,
            back,
        ),
        Button::new(
            PAUSE_OPTIONS_ID,
            (half - 100) as f32,
            (quarter + 80) as f32,
            PAUSE_PAIR_WIDTH,
            20.0,
            options,
        ),
        lan_button,
        Button::new(
            PAUSE_ACHIEVEMENTS_ID,
            (half - 100) as f32,
            (quarter + 32) as f32,
            PAUSE_PAIR_WIDTH,
            20.0,
            achievements,
        ),
        Button::new(
            PAUSE_STATS_ID,
            (half + 2) as f32,
            (quarter + 32) as f32,
            PAUSE_PAIR_WIDTH,
            20.0,
            statistics,
        ),
    ]
}

/// The pause menu's labels in button order — back, achievements,
/// statistics, options, LAN, disconnect — through the handed-over
/// language table: the source's `I18n` lookup with its missing-key rule.
/// With no table handed over, the English the store's `en_US.lang`
/// carries.
pub fn pause_labels(language: Option<&LanguageTable>) -> [String; 6] {
    const KEYS: [&str; 6] = [
        "menu.returnToGame",
        "gui.achievements",
        "gui.stats",
        "menu.options",
        "menu.shareToLan",
        "menu.disconnect",
    ];
    const ENGLISH: [&str; 6] = [
        "Back to Game",
        "Achievements",
        "Statistics",
        "Options...",
        "Open to LAN",
        "Disconnect",
    ];
    let Some(table) = language else {
        return ENGLISH.map(str::to_string);
    };
    KEYS.map(|key| table.get(key).unwrap_or(key).to_string())
}

/// One title through the handed-over language table, or the store's
/// English with no table: the pause menu's `menu.game`.
pub fn pause_title(language: Option<&LanguageTable>) -> String {
    title_lookup(language, "menu.game", "Game menu")
}

/// The statistics title: `gui.stats`.
pub fn stats_title(language: Option<&LanguageTable>) -> String {
    title_lookup(language, "gui.stats", "Statistics")
}

/// The achievements title: `gui.achievements`.
pub fn achievements_title(language: Option<&LanguageTable>) -> String {
    title_lookup(language, "gui.achievements", "Achievements")
}

/// The snooper title: `options.snooper.title`.
pub fn snooper_title(language: Option<&LanguageTable>) -> String {
    title_lookup(
        language,
        "options.snooper.title",
        "Machine Specs Collection",
    )
}

/// The Done label the placeholders stand on: `gui.done`.
pub fn done_label(language: Option<&LanguageTable>) -> String {
    title_lookup(language, "gui.done", "Done")
}

/// One language-table lookup with the missing-key rule (the key itself)
/// and the store's English fallback.
fn title_lookup(language: Option<&LanguageTable>, key: &str, english: &str) -> String {
    match language {
        None => english.to_string(),
        Some(table) => table.get(key).unwrap_or(key).to_string(),
    }
}

/// The dirt background's draws at the scaled size: the `options_background`
/// tile in 32 px spans under the (64, 64, 64) tint (`drawBackground`).
/// The hud sampler clamps, so the source's one REPEAT quad becomes one
/// full-uv tile per 32 px cell — the edge row and column clip to the
/// frame, sampling their share of the tile.
pub fn dirt_draws(scaled: &ScaledResolution) -> Vec<HudDraw> {
    let width = scaled.width as f32;
    let height = scaled.height as f32;
    let mut draws = Vec::new();
    let mut top = 0.0;
    while top < height {
        let span_y = (DIRT_SPAN).min(height - top);
        let mut left = 0.0;
        while left < width {
            let span_x = (DIRT_SPAN).min(width - left);
            draws.push(HudDraw::TexturedRect {
                texture: HudTexture::Named(DIRT_SHEET),
                x: left,
                y: top,
                width: span_x,
                height: span_y,
                uv: [0.0, 0.0, span_x / DIRT_SPAN, span_y / DIRT_SPAN],
                colour: DIRT_TINT,
            });
            left += DIRT_SPAN;
        }
        top += DIRT_SPAN;
    }
    draws
}

/// One centred line's draws: the text at `(w/2 - width/2, y)` in the
/// colour, with the source's shadow.
fn centred_line(
    font: &Font,
    scaled: &ScaledResolution,
    text: &str,
    y: f32,
    colour: [f32; 4],
) -> HudDraw {
    let width = string_width(font, text);
    HudDraw::Text {
        text: text.to_string(),
        x: scaled.width as f32 / 2.0 - width as f32 / 2.0,
        y,
        scale: 1.0,
        colour,
        shadow: true,
        blend: true,
    }
}

/// The pause menu's state: the six-button set.
#[derive(Debug, Clone)]
pub struct PauseScreen {
    /// The button set, laid out at the last `set_screen_size`.
    buttons: Vec<Button>,
}

impl PauseScreen {
    /// Stands the pause menu: the six buttons at `scaled` on `labels`
    /// (back, achievements, statistics, options, LAN, disconnect).
    pub fn new(scaled: &ScaledResolution, labels: [String; 6]) -> Self {
        Self {
            buttons: pause_buttons(scaled, labels),
        }
    }

    /// The button set.
    pub fn buttons(&self) -> &[Button] {
        &self.buttons
    }

    /// Re-lays the buttons at the new scaled size (`initGui` re-runs on
    /// resize: the `buttonList` is cleared and rebuilt).
    pub fn set_screen_size(&mut self, scaled: &ScaledResolution, labels: [String; 6]) {
        self.buttons = pause_buttons(scaled, labels);
    }

    /// One press at the GUI-space point: the button under it answers its
    /// id's action (`mouseClicked` fires on press; only button 0 reaches
    /// here — the caller gates it). The disabled LAN never presses, so it
    /// answers nothing.
    pub fn click_at(&self, point: (f32, f32)) -> Option<PauseAction> {
        let button = self.buttons.iter().find(|button| button.pressed(point))?;
        match button.id {
            PAUSE_BACK_ID => Some(PauseAction::BackToGame),
            PAUSE_ACHIEVEMENTS_ID => Some(PauseAction::Achievements),
            PAUSE_STATS_ID => Some(PauseAction::Statistics),
            PAUSE_OPTIONS_ID => Some(PauseAction::Options),
            PAUSE_DISCONNECT_ID => Some(PauseAction::Disconnect),
            _ => None,
        }
    }

    /// The pause menu's content draws in painter's order: the centred
    /// "Game menu" title at `(w/2, 40)`, then the six buttons
    /// (`drawScreen`:108-113, then `super.drawScreen`). The dirt tiles
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
            "Game menu",
            PAUSE_TITLE_Y,
            TITLE_COLOUR,
        )];
        for button in &self.buttons {
            draws.extend(button.draws(font, mouse));
        }
        draws
    }
}

/// The statistics Done button's rect at the scaled size: 150x20 at
/// `w/2+4, h-28` (`createButtons`).
fn stats_done_rect(scaled: &ScaledResolution) -> (f32, f32, f32, f32) {
    (
        scaled.width as f32 / 2.0 + 4.0,
        scaled.height as f32 - 28.0,
        150.0,
        20.0,
    )
}

/// The achievements Done button's rect at the scaled size: 80x20 at
/// `w/2+24, h/2+74` (`initGui`).
fn achievements_done_rect(scaled: &ScaledResolution) -> (f32, f32, f32, f32) {
    (
        scaled.width as f32 / 2.0 + 24.0,
        scaled.height as f32 / 2.0 + 74.0,
        80.0,
        20.0,
    )
}

/// The snooper Done button's rect at the scaled size: 150x20 at `w/2+2,
/// h-30` (`initGui`; the toggle beside it is omitted, recorded).
fn snooper_done_rect(scaled: &ScaledResolution) -> (f32, f32, f32, f32) {
    (
        scaled.width as f32 / 2.0 + 2.0,
        scaled.height as f32 - 30.0,
        150.0,
        20.0,
    )
}

/// The statistics placeholder's state: the Done button.
#[derive(Debug, Clone)]
pub struct StatsScreen {
    /// The Done button, laid out at the last `set_screen_size`.
    done: Button,
}

impl StatsScreen {
    /// Stands the statistics placeholder: the Done button at `scaled` on
    /// the label.
    pub fn new(scaled: &ScaledResolution, done: String) -> Self {
        let (x, y, w, h) = stats_done_rect(scaled);
        Self {
            done: Button::new(STATS_DONE_ID, x, y, w, h, done),
        }
    }

    /// The Done button.
    pub fn done_button(&self) -> &Button {
        &self.done
    }

    /// Re-lays the Done button at the new scaled size.
    pub fn set_screen_size(&mut self, scaled: &ScaledResolution, done: String) {
        let (x, y, w, h) = stats_done_rect(scaled);
        self.done = Button::new(STATS_DONE_ID, x, y, w, h, done);
    }

    /// One press at the GUI-space point: Done answers its return to the
    /// pause; anything else answers nothing.
    pub fn click_at(&self, point: (f32, f32)) -> Option<StatsAction> {
        if self.done.pressed(point) {
            Some(StatsAction::Done)
        } else {
            None
        }
    }

    /// The placeholder's content draws: the centred title at y 20, the
    /// recorded body, then Done. The dirt tiles and the gradient ride the
    /// frame's own assembly beneath these.
    pub fn draws(
        &self,
        font: &Font,
        scaled: &ScaledResolution,
        mouse: Option<(f32, f32)>,
    ) -> Vec<HudDraw> {
        let mut draws = vec![
            centred_line(font, scaled, "Statistics", STATS_TITLE_Y, TITLE_COLOUR),
            centred_line(
                font,
                scaled,
                STATS_BODY,
                scaled.height as f32 / 2.0,
                BODY_COLOUR,
            ),
        ];
        draws.extend(self.done.draws(font, mouse));
        draws
    }
}

/// The achievements placeholder's state: the Done button.
#[derive(Debug, Clone)]
pub struct AchievementsScreen {
    /// The Done button, laid out at the last `set_screen_size`.
    done: Button,
}

impl AchievementsScreen {
    /// Stands the achievements placeholder: the Done button at `scaled`
    /// on the label.
    pub fn new(scaled: &ScaledResolution, done: String) -> Self {
        let (x, y, w, h) = achievements_done_rect(scaled);
        Self {
            done: Button::new(ACHIEVEMENTS_DONE_ID, x, y, w, h, done),
        }
    }

    /// The Done button.
    pub fn done_button(&self) -> &Button {
        &self.done
    }

    /// Re-lays the Done button at the new scaled size.
    pub fn set_screen_size(&mut self, scaled: &ScaledResolution, done: String) {
        let (x, y, w, h) = achievements_done_rect(scaled);
        self.done = Button::new(ACHIEVEMENTS_DONE_ID, x, y, w, h, done);
    }

    /// One press at the GUI-space point: Done answers its return to the
    /// pause; anything else answers nothing.
    pub fn click_at(&self, point: (f32, f32)) -> Option<AchievementsAction> {
        if self.done.pressed(point) {
            Some(AchievementsAction::Done)
        } else {
            None
        }
    }

    /// The placeholder's content draws: the centred title at y 20, the
    /// recorded body, then Done. The dirt tiles and the gradient ride the
    /// frame's own assembly beneath these.
    pub fn draws(
        &self,
        font: &Font,
        scaled: &ScaledResolution,
        mouse: Option<(f32, f32)>,
    ) -> Vec<HudDraw> {
        let mut draws = vec![
            centred_line(
                font,
                scaled,
                "Achievements",
                ACHIEVEMENTS_TITLE_Y,
                TITLE_COLOUR,
            ),
            centred_line(
                font,
                scaled,
                ACHIEVEMENTS_BODY,
                scaled.height as f32 / 2.0,
                BODY_COLOUR,
            ),
        ];
        draws.extend(self.done.draws(font, mouse));
        draws
    }
}

/// The snooper placeholder's state: the Done button and the declared
/// parent.
#[derive(Debug, Clone)]
pub struct SnooperScreen {
    /// The Done button, laid out at the last `set_screen_size`.
    done: Button,
    /// The declared parent: the screen Done returns to.
    parent: SnooperParent,
}

impl SnooperScreen {
    /// Stands the snooper placeholder: the Done button at `scaled` on the
    /// label, parented at the options hub.
    pub fn new(scaled: &ScaledResolution, done: String) -> Self {
        let (x, y, w, h) = snooper_done_rect(scaled);
        Self {
            done: Button::new(SNOOPER_DONE_ID, x, y, w, h, done),
            parent: SnooperParent::Options,
        }
    }

    /// The Done button.
    pub fn done_button(&self) -> &Button {
        &self.done
    }

    /// The declared parent: the screen Done returns to once the options
    /// hub lands.
    pub fn parent(&self) -> SnooperParent {
        self.parent
    }

    /// Re-lays the Done button at the new scaled size; the parent stands.
    pub fn set_screen_size(&mut self, scaled: &ScaledResolution, done: String) {
        let (x, y, w, h) = snooper_done_rect(scaled);
        self.done = Button::new(SNOOPER_DONE_ID, x, y, w, h, done);
    }

    /// One press at the GUI-space point: Done answers its return to the
    /// declared parent; anything else answers nothing.
    pub fn click_at(&self, point: (f32, f32)) -> Option<SnooperAction> {
        if self.done.pressed(point) {
            Some(SnooperAction::Done)
        } else {
            None
        }
    }

    /// The placeholder's content draws: the centred title at y 8, the
    /// recorded body, then Done. The dirt tiles and the gradient ride the
    /// frame's own assembly beneath these.
    pub fn draws(
        &self,
        font: &Font,
        scaled: &ScaledResolution,
        mouse: Option<(f32, f32)>,
    ) -> Vec<HudDraw> {
        let mut draws = vec![
            centred_line(
                font,
                scaled,
                "Machine Specs Collection",
                SNOOPER_TITLE_Y,
                TITLE_COLOUR,
            ),
            centred_line(
                font,
                scaled,
                SNOOPER_BODY,
                SNOOPER_TITLE_Y + 16.0,
                BODY_COLOUR,
            ),
        ];
        draws.extend(self.done.draws(font, mouse));
        draws
    }
}

#[cfg(test)]
mod tests {
    //! The pause menu's own behaviour: the LAN gate and the press routing.

    use super::*;

    /// A scaled size for the unit pins.
    fn scaled() -> ScaledResolution {
        ScaledResolution {
            width: 427,
            height: 240,
            scale_factor: 2,
        }
    }

    /// English labels in button order.
    fn labels() -> [String; 6] {
        pause_labels(None)
    }

    #[test]
    fn the_lan_button_builds_disabled_and_presses_nothing() {
        // The `:38` enable rule is permanently false with no integrated
        // server: the button draws the grey strip and never presses.
        let screen = PauseScreen::new(&scaled(), labels());
        let lan = screen
            .buttons()
            .iter()
            .find(|button| button.id == PAUSE_LAN_ID)
            .expect("LAN stands");
        assert!(!lan.enabled, "LAN is always disabled");
        assert!(
            screen.click_at((lan.x + 1.0, lan.y + 1.0)).is_none(),
            "a press on LAN answers nothing"
        );
    }

    #[test]
    fn the_snooper_declares_the_options_hub_as_its_parent() {
        // R10: the only opener is the hub's button 104, so the parent slot
        // stands declared until the hub wires it.
        let screen = SnooperScreen::new(&scaled(), String::from("Done"));
        assert_eq!(screen.parent(), SnooperParent::Options);
    }
}
