//! The end-state screens: death, disconnect, terrain-loading and the
//! open-link confirm, plus the sleep cover's shared pieces.
//!
//! The port reads values and names from the source's game-over screen
//! (`client/gui/GuiGameOver.java`), its disconnect screen
//! (`client/gui/GuiDisconnected.java`), its terrain screen
//! (`client/gui/GuiDownloadTerrain.java`), its open-link confirm
//! (`client/gui/GuiConfirmOpenLink.java` over `client/gui/GuiYesNo.java`),
//! the sleep overlay (`client/gui/GuiIngame.java:194-212`), the sleeping
//! camera branch (`client/renderer/EntityRenderer.java:644-665`), the sleep
//! keys (`client/gui/GuiSleepMP.java`) and the English names
//! (`assets/minecraft/lang/en_US.lang`); no source text is copied.
//!
//! Layout (`GuiGameOver.initGui`): Respawn (0) and Title screen (1) as the
//! 200x20 pair at `w/2-100, h/4+72` and `h/4+96`. Both buttons stand
//! disabled for 20 ticks (`setButtonDelay(20)`); the title button stands
//! the quit-confirm (`GuiYesNo` carrying
//! `deathScreen.quit.confirm`/`deathScreen.titleScreen`/`deathScreen.respawn`),
//! whose confirm quits to the title and whose cancel respawns
//! (`confirmClicked:86-99`). The title draws at 2x scale at GUI y 60
//! (`drawScreen:105-111`), the score at y 100 (`:120`).
//!
//! Layout (`GuiDisconnected.initGui`): the reason grey at
//! `h/2 - msgH/2 - 2*FONT_HEIGHT`, the body wrapped at `width - 50`
//! stepping `FONT_HEIGHT`, the Back button (0) 200x20 at
//! `h/2 + msgH/2 + FONT_HEIGHT` returning to the parent screen.
//!
//! Layout (`GuiDownloadTerrain.drawScreen:48-52`): the dirt background and
//! the centred `multiplayer.downloadingTerrain` line at `h/2 - 50`. Its
//! 20-tick keepalive (`updateScreen:38-45`) needs no port: the session
//! echoes the server's keepalives regardless of the standing screen.
//!
//! Layout (`GuiConfirmOpenLink.initGui:33-38` over `GuiYesNo.drawScreen`):
//! the title at y 70, the link wrapped at `width - 50` from y 90 stepping
//! `FONT_HEIGHT`, the warning at y 110 in 16764108, and the three 100x20
//! buttons — confirm (0), copy (2), cancel (1) — at `w/2-50-105`,
//! `w/2-50`, `w/2-50+105`, `h/6+96`. The copy falls through to
//! `confirmClicked(false)` (`actionPerformed:44-50`): it copies, then
//! returns without opening.
//!
//! Sleep (`GuiIngame.java:197-212`): the full-screen rect
//! `220 * sleepTimer/100 << 24 | 1052704`, mirroring past 100 as
//! `1.0 - (timer - 100)/10.0` over the 100→110 fade. The timer ticks 0→100
//! while asleep and 100→110→0 after the wake (`EntityPlayer.java:304-329`).
//! The camera rises by eye +1.0 and 0.3 up with the bed-facing yaw
//! (`EntityRenderer.java:644-665`); the Leave Bed button (1) stands
//! 200x20 at `w/2-100, h-40` (`GuiSleepMP.initGui:18-22`), and Escape and
//! the button both send C0B carrying action 2 (`GuiSleepMP.keyTyped:51-67`).
//!
//! The screens draw as [`HudDraw`](oxide_render::hud::HudDraw) records —
//! rects, text and the widget buttons — so no `HudDraw` growth was needed
//! for them (the T15 ruling carries).

use oxide_assets::font::Font;
use oxide_game::chat::{self, LanguageTable};
use oxide_render::hud::{HudDraw, ScaledResolution};
use oxide_render::text::string_width;

use super::widgets::Button;

/// The death gradient's top stop: 1615855616
/// (`GuiGameOver.drawScreen:103`).
pub const DEATH_GRADIENT_TOP: [f32; 4] = [80.0 / 255.0, 0.0 / 255.0, 0.0 / 255.0, 96.0 / 255.0];
/// The death gradient's bottom stop: -1602211792 (`:103`).
pub const DEATH_GRADIENT_BOTTOM: [f32; 4] =
    [128.0 / 255.0, 48.0 / 255.0, 48.0 / 255.0, 160.0 / 255.0];
/// The death title's scale: the 2x matrix around the title draw (`:105`).
pub const DEATH_TITLE_SCALE: f32 = 2.0;
/// The death title's GUI y: 30 in 2x space (`:110`).
pub const DEATH_TITLE_Y: f32 = 60.0;
/// The score line's GUI y (`:120`).
pub const DEATH_SCORE_Y: f32 = 100.0;
/// Respawn's button id (`initGui:38`).
pub const DEATH_RESPAWN_ID: i32 = 0;
/// The title button's id (`initGui:39`).
pub const DEATH_TITLE_ID: i32 = 1;
/// The button gate in ticks: both buttons enable after 20
/// (`initGui:23` reaching `setButtonDelay(20)`).
pub const DEATH_ENABLE_TICKS: u32 = 20;
/// The disconnect Back button's id (`GuiDisconnected.initGui:43`).
pub const DISCONNECT_BUTTON_ID: i32 = 0;
/// The disconnect reason's grey: 11184810 (`GuiDisconnected.drawScreen:56`).
pub const DISCONNECT_TITLE_COLOUR: [f32; 4] = [170.0 / 255.0, 170.0 / 255.0, 170.0 / 255.0, 1.0];
/// The disconnect body's wrap margin: `width - 50` (`initGui:40`).
pub const DISCONNECT_WRAP_MARGIN: u32 = 50;
/// The confirm title's GUI y (`GuiYesNo.drawScreen:71`).
pub const CONFIRM_TITLE_Y: f32 = 70.0;
/// The confirm warning's GUI y (`GuiConfirmOpenLink.drawScreen:69`).
pub const CONFIRM_WARNING_Y: f32 = 110.0;
/// The confirm warning's red: 16764108 (`:69`).
pub const CONFIRM_WARNING_COLOUR: [f32; 4] = [1.0, 204.0 / 255.0, 204.0 / 255.0, 1.0];
/// The terrain line's offset above centre (`GuiDownloadTerrain.drawScreen:51`).
pub const TERRAIN_LINE_Y_OFFSET: f32 = 50.0;
/// The bed block's id (`EntityRenderer.java:650` reads the block at the
/// entity and turns only for the bed).
pub const BED_BLOCK_ID: u16 = 26;

/// The death screen's answer: respawn at once, the quit-confirm, and the
/// confirm's own two answers (`GuiGameOver.actionPerformed:60-84` and
/// `confirmClicked:86-99`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathAction {
    /// Respawn now: C16 carrying 0, then the screen drops
    /// (`actionPerformed:69-72`).
    Respawn,
    /// The title button: stands the quit-confirm; the screen answers this
    /// once, then the confirm's own answers take over (`:74-83`).
    QuitToTitle,
    /// The confirm's Yes: quit the world to the title (`confirmClicked`
    /// with true, `:86-92`).
    ConfirmQuit,
    /// The confirm's cancel: respawn (`confirmClicked` with false, `:93-98`).
    ConfirmRespawn,
}

/// The disconnect screen's answer: the Back button returns to the parent
/// (`GuiDisconnected.actionPerformed:50-56`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisconnectAction {
    /// Show the parent: the live server list, else a fresh multiplayer
    /// screen over the main menu.
    ToParent,
}

/// The open-link confirm's answer: open, copy, or cancel
/// (`GuiConfirmOpenLink.actionPerformed:44-50`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmAction {
    /// Open the link: the standing opener, then the screen drops
    /// (`confirmClicked` with true).
    Confirm,
    /// Copy the link, then return without opening: the copy falls through
    /// to `confirmClicked(false)` (`:46-50`).
    Copy,
    /// Return to the chat without opening (`confirmClicked` with false).
    Cancel,
}

/// The death screen's labels: the title, the buttons and the score line.
#[derive(Debug, Clone)]
pub struct DeathLabels {
    /// `deathScreen.title` ("You died!").
    pub title: String,
    /// `deathScreen.respawn` ("Respawn").
    pub respawn: String,
    /// `deathScreen.score` ("Score").
    pub score: String,
    /// `deathScreen.quit.confirm` ("Are you sure you want to quit?").
    pub quit_confirm: String,
    /// `deathScreen.titleScreen` ("Title screen").
    pub title_screen: String,
}

/// The death screen's labels from the table, English when the table is
/// absent or a key is missing.
pub fn death_labels(table: Option<&LanguageTable>) -> DeathLabels {
    let get = |key: &str, english: &str| {
        table
            .and_then(|table| table.get(key))
            .unwrap_or(english)
            .to_string()
    };
    DeathLabels {
        title: get("deathScreen.title", "You died!"),
        respawn: get("deathScreen.respawn", "Respawn"),
        score: get("deathScreen.score", "Score"),
        quit_confirm: get("deathScreen.quit.confirm", "Are you sure you want to quit?"),
        title_screen: get("deathScreen.titleScreen", "Title screen"),
    }
}

/// The score line: `Score` + ": " + the yellow score
/// (`GuiGameOver.drawScreen:120`).
pub fn death_score_line(score_label: &str, score: i32) -> String {
    format!("{score_label}: §e{score}")
}

/// The death screen: the gradient, the title, the score and the gated
/// button pair, then the quit-confirm on the title button.
#[derive(Debug, Clone)]
pub struct DeathScreen {
    buttons: [Button; 2],
    ticks: u32,
    confirming: bool,
    score: i32,
    labels: DeathLabels,
}

/// The death button pair: Respawn (0) and Title (1) at `w/2-100, h/4+72`
/// and `h/4+96`, standing disabled (`GuiGameOver.initGui:23-39`).
fn death_buttons(scaled: &ScaledResolution, top: &str, bottom: &str) -> [Button; 2] {
    let half = scaled.width / 2;
    let mut respawn = Button::new(
        DEATH_RESPAWN_ID,
        half as f32 - 100.0,
        scaled.height as f32 / 4.0 + 72.0,
        200.0,
        20.0,
        top.to_string(),
    );
    let mut title = Button::new(
        DEATH_TITLE_ID,
        half as f32 - 100.0,
        scaled.height as f32 / 4.0 + 96.0,
        200.0,
        20.0,
        bottom.to_string(),
    );
    respawn.enabled = false;
    title.enabled = false;
    [respawn, title]
}

impl DeathScreen {
    /// Stands the death screen: the gated button pair, no ticks run yet.
    pub fn new(scaled: &ScaledResolution, labels: &DeathLabels, score: i32) -> Self {
        Self {
            buttons: death_buttons(scaled, &labels.respawn, &labels.title_screen),
            ticks: 0,
            confirming: false,
            score,
            labels: DeathLabels {
                title: labels.title.clone(),
                respawn: labels.respawn.clone(),
                score: labels.score.clone(),
                quit_confirm: labels.quit_confirm.clone(),
                title_screen: labels.title_screen.clone(),
            },
        }
    }

    /// The two buttons, Respawn then Title — the confirm's Yes then No
    /// once the title button stands the quit-confirm.
    pub fn buttons(&self) -> &[Button; 2] {
        &self.buttons
    }

    /// Whether the quit-confirm stands: the title button stood it and no
    /// answer has landed yet.
    pub fn confirming(&self) -> bool {
        self.confirming
    }

    /// One client tick on the death screen: the gate counts up, then both
    /// buttons enable (`GuiYesNo.updateScreen:93-102`, armed by
    /// `setButtonDelay(20)`).
    pub fn tick(&mut self) {
        self.ticks = self.ticks.saturating_add(1);
        if self.ticks >= DEATH_ENABLE_TICKS {
            self.buttons[0].enabled = true;
            self.buttons[1].enabled = true;
        }
    }

    /// The click at the GUI-space point: Respawn, the title button's
    /// quit-confirm, or the confirm's own answers. A gated or missed click
    /// answers nothing.
    pub fn click_at(&mut self, point: (f32, f32)) -> Option<DeathAction> {
        let id = self
            .buttons
            .iter()
            .find(|button| button.pressed(point))
            .map(|button| button.id)?;
        if self.confirming {
            return match id {
                DEATH_RESPAWN_ID => Some(DeathAction::ConfirmQuit),
                _ => Some(DeathAction::ConfirmRespawn),
            };
        }
        match id {
            DEATH_RESPAWN_ID => Some(DeathAction::Respawn),
            _ => {
                self.confirming = true;
                self.ticks = 0;
                for button in &mut self.buttons {
                    button.enabled = false;
                }
                self.buttons[0].label = self.labels.title_screen.clone();
                self.buttons[1].label = self.labels.respawn.clone();
                Some(DeathAction::QuitToTitle)
            }
        }
    }

    /// The death draws: the two-stop gradient over the full frame, the 2x
    /// title, the score line and the buttons — the quit-confirm's message
    /// and its own pair once the title button stands it.
    pub fn draws(
        &self,
        font: &Font,
        scaled: &ScaledResolution,
        mouse: Option<(f32, f32)>,
    ) -> Vec<HudDraw> {
        let width = scaled.width as f32;
        let height = scaled.height as f32;
        let mut draws = vec![
            HudDraw::Rect {
                x: 0.0,
                y: 0.0,
                width,
                height: height / 2.0,
                colour: DEATH_GRADIENT_TOP,
            },
            HudDraw::Rect {
                x: 0.0,
                y: height / 2.0,
                width,
                height: height - height / 2.0,
                colour: DEATH_GRADIENT_BOTTOM,
            },
        ];
        if self.confirming {
            draws.push(centred(
                font,
                scaled,
                &self.labels.quit_confirm,
                CONFIRM_TITLE_Y,
            ));
        } else {
            let title_width = string_width(font, &self.labels.title) as f32 * DEATH_TITLE_SCALE;
            draws.push(HudDraw::Text {
                text: self.labels.title.clone(),
                x: width / 2.0 - title_width / 2.0,
                y: DEATH_TITLE_Y,
                scale: DEATH_TITLE_SCALE,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: true,
                blend: true,
            });
            draws.push(centred(
                font,
                scaled,
                &death_score_line(&self.labels.score, self.score),
                DEATH_SCORE_Y,
            ));
        }
        for button in &self.buttons {
            draws.extend(button.draws(font, mouse));
        }
        draws
    }
}

/// The disconnect button's GUI y: `h/2 + msgH/2 + FONT_HEIGHT` where the
/// message block is `body_lines * font_h` tall
/// (`GuiDisconnected.initGui:43`).
pub fn disconnect_button_y(screen_h: u32, body_lines: usize, font_h: u32) -> f32 {
    screen_h as f32 / 2.0 + (body_lines as u32 * font_h) as f32 / 2.0 + font_h as f32
}

/// The disconnect screen: the grey reason, the wrapped white body and the
/// Back button.
#[derive(Debug, Clone)]
pub struct DisconnectedScreen {
    title: String,
    lines: Vec<String>,
    button: Button,
}

impl DisconnectedScreen {
    /// Stands the disconnect screen: the body wraps at `width - 50`, the
    /// Back button stands under the block
    /// (`GuiDisconnected.initGui:38-43`).
    pub fn new(
        scaled: &ScaledResolution,
        title: String,
        body: String,
        font: &Font,
        button_label: String,
    ) -> Self {
        let width = scaled.width as i32 - DISCONNECT_WRAP_MARGIN as i32;
        let runs = chat::flatten(&chat::parse_json(&body));
        let lines = chat::wrap(&runs, width, font)
            .iter()
            .map(|line| runs_text(line))
            .collect::<Vec<_>>();
        let button = Button::new(
            DISCONNECT_BUTTON_ID,
            scaled.width as f32 / 2.0 - 100.0,
            disconnect_button_y(scaled.height, lines.len().max(1), font.height()),
            200.0,
            20.0,
            button_label,
        );
        Self {
            title,
            lines,
            button,
        }
    }

    /// The grey reason line.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The wrapped body lines.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// The click at the GUI-space point: the Back button returns to the
    /// parent; anywhere else answers nothing.
    pub fn click_at(&self, point: (f32, f32)) -> Option<DisconnectAction> {
        if self.button.pressed(point) {
            Some(DisconnectAction::ToParent)
        } else {
            None
        }
    }

    /// The disconnect draws: the reason grey at `h/2 - msgH/2 - 2*FONT`,
    /// the white body lines from `h/2 - msgH/2`, and the Back button
    /// (`GuiDisconnected.drawScreen:54-68`). The dirt tiles ride the
    /// frame's own assembly beneath these.
    pub fn draws(
        &self,
        font: &Font,
        scaled: &ScaledResolution,
        mouse: Option<(f32, f32)>,
    ) -> Vec<HudDraw> {
        let step = font.height() as f32;
        let block = self.lines.len().max(1) as f32 * step;
        let mut draws = vec![HudDraw::Text {
            text: self.title.clone(),
            x: scaled.width as f32 / 2.0 - string_width(font, &self.title) as f32 / 2.0,
            y: scaled.height as f32 / 2.0 - block / 2.0 - step * 2.0,
            scale: 1.0,
            colour: DISCONNECT_TITLE_COLOUR,
            shadow: true,
            blend: true,
        }];
        let mut y = scaled.height as f32 / 2.0 - block / 2.0;
        for line in &self.lines {
            draws.push(HudDraw::Text {
                text: line.clone(),
                x: scaled.width as f32 / 2.0 - string_width(font, line) as f32 / 2.0,
                y,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: true,
                blend: true,
            });
            y += step;
        }
        draws.extend(self.button.draws(font, mouse));
        draws
    }
}

/// The terrain screen: the standing `Downloading terrain` line. The dirt
/// tiles ride the frame's own assembly beneath it.
#[derive(Debug, Clone)]
pub struct TerrainScreen {
    line: String,
}

impl TerrainScreen {
    /// Stands the terrain screen on the line the window hands over
    /// (`multiplayer.downloadingTerrain`).
    pub fn new(line: String) -> Self {
        Self { line }
    }

    /// The standing line.
    pub fn line(&self) -> &str {
        &self.line
    }

    /// The terrain draws: the centred line at `h/2 - 50`
    /// (`GuiDownloadTerrain.drawScreen:48-52`).
    pub fn draws(&self, font: &Font, scaled: &ScaledResolution) -> Vec<HudDraw> {
        vec![centred(
            font,
            scaled,
            &self.line,
            scaled.height as f32 / 2.0 - TERRAIN_LINE_Y_OFFSET,
        )]
    }
}

/// The open-link confirm's labels: the title and the button pair, which
/// read differently for trusted links (`GuiConfirmOpenLink`'s constructor).
pub struct ConfirmLabels {
    /// The title for an untrusted link: `chat.link.confirm`.
    pub title: String,
    /// The title for a trusted link: `chat.link.confirmTrusted`.
    pub trusted_title: String,
    /// The confirm button for an untrusted link: `gui.yes`.
    pub confirm_untrusted: String,
    /// The cancel button for an untrusted link: `gui.no`.
    pub cancel_untrusted: String,
    /// The confirm button for a trusted link: `chat.link.open`.
    pub confirm_trusted: String,
    /// The cancel button for a trusted link: `gui.cancel`.
    pub cancel_trusted: String,
    /// The copy button, either way: `chat.copy`.
    pub copy: String,
    /// The red warning under the link: `chat.link.warning`.
    pub warning: String,
}

/// The confirm's labels from the table, English when the table is absent
/// or a key is missing.
pub fn confirm_labels(table: Option<&LanguageTable>) -> ConfirmLabels {
    let get = |key: &str, english: &str| {
        table
            .and_then(|table| table.get(key))
            .unwrap_or(english)
            .to_string()
    };
    ConfirmLabels {
        title: get(
            "chat.link.confirm",
            "Are you sure you want to open the following website?",
        ),
        trusted_title: get(
            "chat.link.confirmTrusted",
            "Do you want to open this link or copy it to your clipboard?",
        ),
        confirm_untrusted: get("gui.yes", "Yes"),
        cancel_untrusted: get("gui.no", "No"),
        confirm_trusted: get("chat.link.open", "Open in browser"),
        cancel_trusted: get("gui.cancel", "Cancel"),
        copy: get("chat.copy", "Copy to Clipboard"),
        warning: get(
            "chat.link.warning",
            "Never open links from people that you don't trust!",
        ),
    }
}

/// The open-link confirm: the title, the wrapped link, the warning and
/// the confirm/copy/cancel row.
#[derive(Debug, Clone)]
pub struct ConfirmScreen {
    link: String,
    title: String,
    lines: Vec<String>,
    warning: String,
    buttons: [Button; 3],
}

impl ConfirmScreen {
    /// Stands the confirm on the link: the link wraps at `width - 50`,
    /// the three buttons stand at `w/2-50-105`, `w/2-50`, `w/2-50+105`,
    /// `h/6+96` (`GuiConfirmOpenLink.initGui:33-38`).
    pub fn new(
        scaled: &ScaledResolution,
        link: String,
        font: &Font,
        labels: &ConfirmLabels,
        trusted: bool,
    ) -> Self {
        let (title, confirm, cancel) = if trusted {
            (
                labels.trusted_title.clone(),
                &labels.confirm_trusted,
                &labels.cancel_trusted,
            )
        } else {
            (
                labels.title.clone(),
                &labels.confirm_untrusted,
                &labels.cancel_untrusted,
            )
        };
        let width = scaled.width as i32 - DISCONNECT_WRAP_MARGIN as i32;
        let runs = [chat::StyledRun {
            text: link.clone(),
            colour: None,
            styles: 0,
            click: None,
            hover: None,
        }];
        let lines = chat::wrap(&runs, width, font)
            .iter()
            .map(|line| runs_text(line))
            .collect::<Vec<_>>();
        let half = scaled.width / 2;
        let y = scaled.height / 6 + 96;
        let buttons = [
            Button::new(
                0,
                half as f32 - 155.0,
                y as f32,
                100.0,
                20.0,
                confirm.clone(),
            ),
            Button::new(
                2,
                half as f32 - 50.0,
                y as f32,
                100.0,
                20.0,
                labels.copy.clone(),
            ),
            Button::new(1, half as f32 + 55.0, y as f32, 100.0, 20.0, cancel.clone()),
        ];
        Self {
            link,
            title,
            lines,
            warning: labels.warning.clone(),
            buttons,
        }
    }

    /// The link the confirm asks about: the opener and the clipboard copy
    /// read it.
    pub fn link(&self) -> &str {
        &self.link
    }

    /// The three buttons: confirm, copy, cancel.
    pub fn buttons(&self) -> &[Button; 3] {
        &self.buttons
    }

    /// The click at the GUI-space point: confirm, copy, or cancel. A
    /// missed click answers nothing.
    pub fn click_at(&self, point: (f32, f32)) -> Option<ConfirmAction> {
        let id = self
            .buttons
            .iter()
            .find(|button| button.pressed(point))
            .map(|button| button.id)?;
        match id {
            0 => Some(ConfirmAction::Confirm),
            2 => Some(ConfirmAction::Copy),
            _ => Some(ConfirmAction::Cancel),
        }
    }

    /// The confirm draws: the title at y 70, the wrapped link from y 90,
    /// the warning at y 110 in 16764108, and the button row
    /// (`GuiYesNo.drawScreen:69-83` and
    /// `GuiConfirmOpenLink.drawScreen:63-72`). The dirt tiles ride the
    /// frame's own assembly beneath these.
    pub fn draws(
        &self,
        font: &Font,
        scaled: &ScaledResolution,
        mouse: Option<(f32, f32)>,
    ) -> Vec<HudDraw> {
        let mut draws = vec![centred(font, scaled, &self.title, CONFIRM_TITLE_Y)];
        let mut y = CONFIRM_TITLE_Y + 20.0;
        for line in &self.lines {
            draws.push(centred(font, scaled, line, y));
            y += font.height() as f32;
        }
        draws.push(HudDraw::Text {
            text: self.warning.clone(),
            x: scaled.width as f32 / 2.0 - string_width(font, &self.warning) as f32 / 2.0,
            y: CONFIRM_WARNING_Y,
            scale: 1.0,
            colour: CONFIRM_WARNING_COLOUR,
            shadow: true,
            blend: true,
        });
        for button in &self.buttons {
            draws.extend(button.draws(font, mouse));
        }
        draws
    }
}

/// One centred line's draws: the text at `(w/2 - width/2, y)` in white
/// with the source's shadow (the pause screen's own line, `pause.rs:318`).
fn centred(font: &Font, scaled: &ScaledResolution, text: &str, y: f32) -> HudDraw {
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

/// The wrapped runs' display text: the run characters with their `§`
/// colour and style codes, the draw's own `§r` reset closing each run
/// (the chat view's own line text, `view.rs:4572`).
fn runs_text(runs: &[chat::StyledRun]) -> String {
    const CODES: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::new();
    for run in runs {
        if let Some(index) = run.colour {
            text.push('§');
            text.push(char::from(CODES[usize::from(index.min(15))]));
        }
        if run.styles & chat::STYLE_BOLD != 0 {
            text.push_str("§l");
        }
        if run.styles & chat::STYLE_ITALIC != 0 {
            text.push_str("§o");
        }
        if run.styles & chat::STYLE_UNDERLINED != 0 {
            text.push_str("§n");
        }
        if run.styles & chat::STYLE_OBFUSCATED != 0 {
            text.push_str("§k");
        }
        if run.styles & chat::STYLE_STRIKETHROUGH != 0 {
            text.push_str("§m");
        }
        text.push_str(&run.text);
        text.push_str("§r");
    }
    text
}

/// The Leave Bed button: id 1, 200x20 at `w/2-100, h-40`
/// (`GuiSleepMP.initGui:18-22`).
pub fn leave_bed_button(scaled: &ScaledResolution, label: String) -> Button {
    let half = scaled.width / 2;
    Button::new(
        1,
        half as f32 - 100.0,
        scaled.height as f32 - 40.0,
        200.0,
        20.0,
        label,
    )
}

/// The Leave Bed label: `multiplayer.stopSleeping`, English without a table.
pub fn leave_bed_label(table: Option<&LanguageTable>) -> String {
    table
        .and_then(|table| table.get("multiplayer.stopSleeping"))
        .unwrap_or("Leave Bed")
        .to_string()
}

/// The terrain screen's line: `multiplayer.downloadingTerrain`, English
/// without a table.
pub fn terrain_line(table: Option<&LanguageTable>) -> String {
    table
        .and_then(|table| table.get("multiplayer.downloadingTerrain"))
        .unwrap_or("Downloading terrain")
        .to_string()
}

/// The disconnect screen's fixed title: `disconnect.lost`, English without
/// a table.
pub fn disconnect_title(table: Option<&LanguageTable>) -> String {
    table
        .and_then(|table| table.get("disconnect.lost"))
        .unwrap_or("Connection Lost")
        .to_string()
}

/// The disconnect screen's button: `gui.toMenu`, English without a table.
pub fn disconnect_button_label(table: Option<&LanguageTable>) -> String {
    table
        .and_then(|table| table.get("gui.toMenu"))
        .unwrap_or("Back to server list")
        .to_string()
}

/// The sleep darkening: the full-screen `16,16,16` rect whose alpha is
/// `220 * sleepTimer/100`, mirroring past 100 as `1.0 - (timer - 100)/10.0`
/// over the 100→110 fade (`GuiIngame.java:197-212`).
pub fn sleep_dim(timer: u32) -> [f32; 4] {
    let f = timer as f32 / 100.0;
    let f = if f > 1.0 {
        (1.0 - (timer - 100) as f32 / 10.0).max(0.0)
    } else {
        f
    };
    let alpha = (220.0 * f) as u8;
    [
        16.0 / 255.0,
        16.0 / 255.0,
        16.0 / 255.0,
        f32::from(alpha) / 255.0,
    ]
}

/// One sleep-timer tick: 0→100 while asleep, capped; 100→110→0 after the
/// wake (`EntityPlayer.java:304-329`).
pub fn sleep_tick(sleeping: bool, timer: u32) -> u32 {
    if sleeping {
        timer.saturating_add(1).min(100)
    } else if timer == 0 {
        0
    } else {
        let next = timer.saturating_add(1);
        if next >= 110 { 0 } else { next }
    }
}

/// The sleeping camera's yaw: the bed-facing quarter-turn folded into the
/// player's yaw (`EntityRenderer.java:650-657` rotates `facing * 90` about
/// +Y, then `yaw + 180` about -Y, which the port's yaw convention reads as
/// `yaw + 180 - facing * 90`).
pub fn sleep_yaw(yaw: f32, facing: u8) -> f32 {
    yaw + 180.0 - f32::from(facing.min(3)) * 90.0
}

/// The bed head's facing quarter-turn: the metadata's low two bits
/// (`BlockBed.getDirectionFromMetadata`), so head and occupied bits keep
/// the facing.
pub fn bed_facing_j(meta: u8) -> u8 {
    meta & 3
}
