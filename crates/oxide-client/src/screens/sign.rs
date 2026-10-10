//! The sign editor: the screen `SignEditorOpen` (0x36) stands on the block.
//!
//! The port reads values and names from the source's editor
//! (`client/gui/inventory/GuiEditSign.java`), its tile state
//! (`tileentity/TileEntitySign.java`) and the close path; no source text is
//! copied.
//!
//! The editor opens on line 0 with its counter at 0 (the constructor,
//! `GuiEditSign.java`:32-35, stores the tile alone; `editLine` and
//! `updateCounter` are field defaults). The lines come from the session's
//! sign map — the last `SignTextChanged` for the position — or four empty
//! lines when the map holds none (the tile's own default, four empty
//! `ChatComponentText`, `TileEntitySign.java`:24-31).
//!
//! The close sends the update on EVERY close: `onGuiClosed` (`:52-63`)
//! queues the `C12PacketUpdateSign` (the send at `:59`) and Escape reaches
//! the same close (`keyTyped`:118-121 runs the Done button's action,
//! `:76-86`, which closes the screen) — so Done and Escape send
//! identically. The port's close answers
//! [`InputEvent::UpdateSign`](oxide_game::input::InputEvent::UpdateSign)
//! instead of the container close.
//!
//! The blink is the editor's own counter (`updateScreen`:68-71 ticks it
//! once): the editing markers show while `updateCounter / 6 % 2 == 0`
//! (`drawScreen`:169-175) — a 12-tick period, six on and six off.
//!
//! The navigation wraps both directions through `& 3` (`keyTyped`:94-102):
//! up steps one back, down, Enter and the keypad Enter one on. Backspace
//! drops the last character when the line is non-empty, and a typed
//! character appends only when `ChatAllowedCharacters.isAllowedCharacter`
//! takes it AND the width test holds — `getStringWidth(line + char) <= 90`
//! (`:104-116`). There is NO character cap: width alone gates.
//!
//! The editing line's display wrap is `"> " + line + " <"` — the editor's
//! own markers, not a colour (`TileEntitySignRenderer.java`:86-111 draws
//! the wrapped line while `lineBeingEdited` is set, which the blink phase
//! alone sets). The in-world draw never shows it.
//!
//! The flat draw is a port choice (recorded): the source renders the 3D
//! sign through the tile dispatcher under
//! `translate(width / 2, 0, 50)` + `scale(-93.75, -93.75, -93.75)` +
//! `rotate(180 Y)` (`GuiEditSign.drawScreen`:133-136, `:174`), which has no HUD-space
//! equivalent — so the editor draws the title, the four lines over a flat
//! board-coloured backing, and the Done button (`GuiButton`'s own 200x20 at
//! `width / 2 - 100, height / 4 + 120`, `GuiEditSign.initGui`:41-47).

use oxide_assets::font::Font;
use oxide_game::input::{InputEvent, Key};
use oxide_render::hud::{HudDraw, HudTexture, ScaledResolution};
use oxide_render::text::string_width;

/// The editor's per-line width cap in font pixels: a typed character
/// appends only while `getStringWidth(line + char) <= 90`
/// (`GuiEditSign.java`:111). Width alone gates — there is no character cap.
pub const SIGN_LINE_CAP_PX: i32 = 90;
/// The blink clock's divisor: the markers show while
/// `updateCounter / 6 % 2 == 0` (`GuiEditSign.java`:169-175) — a 12-tick
/// period, six ticks on and six off.
pub const SIGN_BLINK_DIVISOR: u32 = 6;
/// The editor's title: `I18n "sign.edit"` (`drawScreen`:130), English
/// `sign.edit=Edit sign message`, drawn centred at `(width / 2, 40)` in
/// white (16777215).
pub const SIGN_EDIT_TITLE: &str = "Edit sign message";
/// The title's top edge in GUI pixels (`drawScreen`:130).
pub const SIGN_TITLE_Y: f32 = 40.0;
/// The title's colour: 16777215, opaque white.
pub const SIGN_TITLE_COLOUR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// The Done button's label: `I18n "gui.done"` (`GuiEditSign.initGui:45`), English
/// `gui.done=Done`.
pub const SIGN_DONE_TEXT: &str = "Done";
/// The Done button's size: `GuiButton(0, width / 2 - 100, height / 4 + 120,
/// ...)` is the source's 200x20 button (`GuiEditSign.initGui:45`).
pub const SIGN_DONE_WIDTH: f32 = 200.0;
/// The Done button's height (see [`SIGN_DONE_WIDTH`]).
pub const SIGN_DONE_HEIGHT: f32 = 20.0;
/// The Done label's idle colour: `GuiButton`'s enabled text, 14737632
/// (0xE0E0E0) — answered while the pointer stands off the button.
pub const SIGN_DONE_COLOUR: [f32; 4] = [224.0 / 255.0, 224.0 / 255.0, 224.0 / 255.0, 1.0];
/// The Done label's hovered colour: `GuiButton`'s hovered text, 16777120
/// (0xFFFFA0) — answered while the pointer stands on the button
/// (`drawButton`'s hovered arm).
pub const SIGN_DONE_COLOUR_HOVER: [f32; 4] = [1.0, 1.0, 160.0 / 255.0, 1.0];
/// The widgets sheet the button blits: the extraction tree's
/// `gui/widgets.png`, the sheet `GuiButton` draws from.
pub const SIGN_WIDGETS_SHEET: &str = "gui/widgets";
/// The button sprite's idle row: `GuiButton` blits `46 + i * 20` with
/// `i = 1` while the pointer stands off the button (`drawButton`) — the
/// enabled strip at v 66, 20 tall.
pub const SIGN_BUTTON_V_IDLE: f32 = 66.0;
/// The button sprite's hovered row: `i = 2` while the pointer stands on
/// the button — the hot strip at v 86, 20 tall.
pub const SIGN_BUTTON_V_HOVER: f32 = 86.0;
/// The first line's top edge in GUI pixels: the four rows run at the
/// board's own 10-pixel pitch under the title (a port choice — the source
/// lays the lines on the 3D board, which has no HUD-space position).
pub const SIGN_LINES_TOP: f32 = 80.0;
/// The rows' pitch in GUI pixels (see [`SIGN_LINES_TOP`]).
pub const SIGN_LINE_PITCH: f32 = 10.0;
/// The backing rect's colour: the flat stand-in for the 3D board the
/// source renders behind its black text (a port choice — the sheet pixel
/// cannot be sampled, so black text needs a light field to read on).
pub const SIGN_BOARD_COLOUR: [f32; 4] = [0.55, 0.42, 0.28, 1.0];
/// The backing rect's half width in GUI pixels: the 90-pixel cap plus an
/// 8-pixel margin each side.
pub const SIGN_BOARD_HALF_WIDTH: f32 = 53.0;
/// The backing rect's top edge: four rows above the first line's top.
pub const SIGN_BOARD_TOP: f32 = 76.0;
/// The backing rect's height: the four rows' glyphs (4 x 10) plus the
/// margins.
pub const SIGN_BOARD_HEIGHT: f32 = 46.0;
/// The lines' colour: the literal black `0x000000`
/// (`TileEntitySignRenderer.java`:86 — `int i = 0`, never changed).
pub const SIGN_TEXT_COLOUR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// Whether a character may be typed: `ChatAllowedCharacters:10-13`
/// refuses the format code `§` (167), everything below the space, and DEL
/// (127) — the same law the chat field and the anvil name field carry.
pub fn sign_allowed(c: char) -> bool {
    c != '§' && c >= ' ' && c != '\u{7f}'
}

/// The sign editor: the block, its four lines, the editing line and the
/// blink counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignScreen {
    /// The sign's world x.
    x: i32,
    /// The sign's world y.
    y: i32,
    /// The sign's world z.
    z: i32,
    /// The four lines, as the session's map held them at the open (empty
    /// four lines when it held none).
    lines: [String; 4],
    /// The line being edited, 0..4 (`editLine`, `GuiEditSign.java`:27).
    edit_line: u8,
    /// The blink counter (`updateCounter`, `:24`), ticked once per tick.
    counter: u32,
}

/// One sign-text map entry: the block the store held at the position, its
/// metadata nibble, and the last four lines the session reported. The
/// editor reads the lines; the world draw needs the board's kind and
/// facing too.
#[derive(Debug, Clone, PartialEq)]
pub struct SignMapEntry {
    /// The block id at the position (63 standing, 68 wall).
    pub block_id: u16,
    /// The metadata nibble: the standing sign's rotation, the wall sign's
    /// facing.
    pub metadata: u8,
    /// The last four lines, as sent.
    pub lines: [String; 4],
}

impl SignScreen {
    /// Stands the editor on the block with the map's lines: line 0 edited,
    /// the counter at 0 (`GuiEditSign.java`:32-35 plus the field defaults).
    pub fn new(x: i32, y: i32, z: i32, lines: [String; 4]) -> Self {
        Self {
            x,
            y,
            z,
            lines,
            edit_line: 0,
            counter: 0,
        }
    }

    /// The sign's world position.
    pub fn position(&self) -> (i32, i32, i32) {
        (self.x, self.y, self.z)
    }

    /// The four lines, as edited.
    pub fn lines(&self) -> &[String; 4] {
        &self.lines
    }

    /// The line being edited, 0..4.
    pub fn edit_line(&self) -> u8 {
        self.edit_line
    }

    /// The blink counter, for the pins.
    #[cfg(test)]
    pub fn counter(&self) -> u32 {
        self.counter
    }

    /// One session tick: the blink counter steps once
    /// (`updateScreen`:68-71).
    pub fn tick(&mut self) {
        self.counter = self.counter.wrapping_add(1);
    }

    /// Whether the editing markers show: `updateCounter / 6 % 2 == 0`
    /// (`drawScreen`:169-175).
    pub fn blink_visible(&self) -> bool {
        self.counter / SIGN_BLINK_DIVISOR % 2 == 0
    }

    /// The four display lines: the editing line wrapped as
    /// `"> " + line + " <"` while the blink shows it, every other line
    /// (and the editing line in the dark phase) plain.
    pub fn display_lines(&self) -> [String; 4] {
        let visible = self.blink_visible();
        [0, 1, 2, 3].map(|index| {
            if index == self.edit_line && visible {
                format!("> {} <", self.lines[index as usize])
            } else {
                self.lines[index as usize].clone()
            }
        })
    }

    /// The up arrow: `editLine - 1 & 3` (`keyTyped`:94-102).
    pub fn move_up(&mut self) {
        self.edit_line = self.edit_line.wrapping_sub(1) & 3;
    }

    /// Down, Enter and the keypad Enter alike: `editLine + 1 & 3`.
    pub fn move_down(&mut self) {
        self.edit_line = self.edit_line.wrapping_add(1) & 3;
    }

    /// Backspace: drops the last character when the line is non-empty
    /// (`keyTyped`:106-109). Answers whether a character left.
    pub fn backspace(&mut self) -> bool {
        self.lines[self.edit_line as usize].pop().is_some()
    }

    /// One typed character: appends only when the character is allowed AND
    /// `getStringWidth(line + char) <= 90` (`keyTyped`:111-114), measured
    /// with the editor's font. Answers whether the line grew.
    pub fn type_char(&mut self, c: char, font: &Font) -> bool {
        if !sign_allowed(c) {
            return false;
        }
        let mut candidate = self.lines[self.edit_line as usize].clone();
        candidate.push(c);
        if string_width(font, &candidate) > SIGN_LINE_CAP_PX {
            return false;
        }
        self.lines[self.edit_line as usize] = candidate;
        true
    }

    /// Types a string's characters in order (`GuiTextField.writeText`'s
    /// per-character filter). Answers how many landed.
    pub fn type_text(&mut self, text: &str, font: &Font) -> usize {
        text.chars().filter(|c| self.type_char(*c, font)).count()
    }

    /// One editing key: up steps back, down and Enter step on, Backspace
    /// drops the last character (`GuiEditSign.keyTyped`:94-109). Answers whether the
    /// editor changed. Any other key is not the editor's.
    pub fn key(&mut self, key: Key) -> bool {
        match key {
            Key::ArrowUp => {
                self.move_up();
                true
            }
            Key::ArrowDown | Key::Enter => {
                self.move_down();
                true
            }
            Key::Backspace => self.backspace(),
            _ => false,
        }
    }

    /// The close's send: `InputEvent::UpdateSign` with the position and
    /// the four lines as edited — on EVERY close, Done and Escape alike
    /// (`onGuiClosed`:52-63, the send at `:59`).
    pub fn close_event(&self) -> InputEvent {
        InputEvent::UpdateSign {
            x: self.x,
            y: self.y,
            z: self.z,
            lines: self.lines.clone(),
        }
    }

    /// The Done button's rect in GUI pixels: `(width / 2 - 100,
    /// height / 4 + 120)`, 200x20 (`GuiEditSign.initGui:45`).
    pub fn done_bounds(scaled: &ScaledResolution) -> (f32, f32, f32, f32) {
        let width = scaled.width as f32;
        let height = scaled.height as f32;
        (
            width / 2.0 - 100.0,
            height / 4.0 + 120.0,
            SIGN_DONE_WIDTH,
            SIGN_DONE_HEIGHT,
        )
    }

    /// Whether the GUI-space point stands on the Done button.
    pub fn done_pressed(scaled: &ScaledResolution, point: (f32, f32)) -> bool {
        let (x, y, w, h) = Self::done_bounds(scaled);
        point.0 >= x && point.0 < x + w && point.1 >= y && point.1 < y + h
    }

    /// The editor's draws: the title, the four lines over the board
    /// backing, and the Done button with its label. The button's strip and
    /// tint follow the free pointer's scaled position — the hot row and
    /// tint on the button, the idle row and tint off it — like
    /// `GuiButton.drawButton`'s hovered arm; `None` (before the first move)
    /// draws idle.
    pub fn draws(
        &self,
        font: &Font,
        scaled: &ScaledResolution,
        mouse: Option<(f32, f32)>,
    ) -> Vec<HudDraw> {
        let width = scaled.width as f32;
        let mut draws = Vec::new();
        draws.push(HudDraw::Rect {
            x: width / 2.0 - SIGN_BOARD_HALF_WIDTH,
            y: SIGN_BOARD_TOP,
            width: SIGN_BOARD_HALF_WIDTH * 2.0,
            height: SIGN_BOARD_HEIGHT,
            colour: SIGN_BOARD_COLOUR,
        });
        let title_width = string_width(font, SIGN_EDIT_TITLE);
        draws.push(HudDraw::Text {
            text: SIGN_EDIT_TITLE.to_string(),
            x: width / 2.0 - title_width as f32 / 2.0,
            y: SIGN_TITLE_Y,
            scale: 1.0,
            colour: SIGN_TITLE_COLOUR,
            shadow: true,
            blend: true,
        });
        for (index, line) in self.display_lines().iter().enumerate() {
            let line_width = string_width(font, line);
            draws.push(HudDraw::Text {
                text: line.clone(),
                x: width / 2.0 - line_width as f32 / 2.0,
                y: SIGN_LINES_TOP + index as f32 * SIGN_LINE_PITCH,
                scale: 1.0,
                colour: SIGN_TEXT_COLOUR,
                shadow: false,
                blend: true,
            });
        }
        let (bx, by, bw, bh) = Self::done_bounds(scaled);
        let hovered = mouse.is_some_and(|point| Self::done_pressed(scaled, point));
        let button_v = if hovered {
            SIGN_BUTTON_V_HOVER
        } else {
            SIGN_BUTTON_V_IDLE
        };
        let label_colour = if hovered {
            SIGN_DONE_COLOUR_HOVER
        } else {
            SIGN_DONE_COLOUR
        };
        draws.push(HudDraw::TexturedRect {
            texture: HudTexture::Named(SIGN_WIDGETS_SHEET),
            x: bx,
            y: by,
            width: bw,
            height: bh,
            uv: [
                0.0,
                button_v / 256.0,
                200.0 / 256.0,
                (button_v + 20.0) / 256.0,
            ],
            colour: [1.0, 1.0, 1.0, 1.0],
        });
        let label_width = string_width(font, SIGN_DONE_TEXT);
        draws.push(HudDraw::Text {
            text: SIGN_DONE_TEXT.to_string(),
            x: bx + bw / 2.0 - label_width as f32 / 2.0,
            y: by + (bh - 8.0) / 2.0,
            scale: 1.0,
            colour: label_colour,
            shadow: true,
            blend: true,
        });
        draws
    }
}

#[cfg(test)]
mod tests {
    //! The editor's own behaviour: the open defaults, the blink phase, the
    //! navigation wrap, the width cap, and the close that always sends.

    use super::*;

    /// The synthetic sheet's measured font: the `A` cell inks columns
    /// 0..=4, so `A` advances six font pixels (the text suite's own
    /// pattern).
    fn editor_font() -> Font {
        const SIDE: u32 = 128;
        const CELL: u32 = 8;
        let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
        for code in ['A', 'B', 'C'] {
            let code = code as u32;
            let cell_x = (code % 16) * CELL;
            let cell_y = (code / 16) * CELL;
            for row in 0..CELL {
                for column in 0..=4 {
                    let offset = (((cell_y + row) * SIDE + cell_x + column) * 4) as usize;
                    rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
        Font::load(
            &oxide_assets::texture::Texture {
                width: SIDE,
                height: SIDE,
                rgba,
            },
            None,
        )
        .expect("the synthetic sheet loads")
    }

    /// Four empty lines: the map's default for a missing entry.
    fn empty_lines() -> [String; 4] {
        [String::new(), String::new(), String::new(), String::new()]
    }

    #[test]
    fn the_editor_opens_on_line_zero_with_its_counter_at_zero() {
        // The constructor stores the tile alone (`GuiEditSign.java`:32-35):
        // `editLine` and `updateCounter` are field defaults.
        let screen = SignScreen::new(
            4,
            65,
            -9,
            [
                "first".to_string(),
                String::new(),
                "third".to_string(),
                String::new(),
            ],
        );
        assert_eq!(screen.position(), (4, 65, -9));
        assert_eq!(screen.edit_line(), 0);
        assert!(screen.blink_visible(), "counter 0 shows the markers");
        assert_eq!(
            screen.lines(),
            &[
                "first".to_string(),
                String::new(),
                "third".to_string(),
                String::new()
            ]
        );
    }

    #[test]
    fn the_blink_runs_a_twelve_tick_period_six_on_six_off() {
        // `updateCounter / 6 % 2 == 0` (`drawScreen`:169-175): ticks 0-5
        // show, 6-11 hide, 12 shows again.
        let mut screen = SignScreen::new(0, 64, 0, empty_lines());
        for _ in 0..6 {
            assert!(screen.blink_visible(), "tick {}", screen.counter);
            screen.tick();
        }
        assert_eq!(screen.counter, 6);
        for _ in 6..12 {
            assert!(!screen.blink_visible(), "tick {}", screen.counter);
            screen.tick();
        }
        assert!(screen.blink_visible(), "tick 12 shows again");
    }

    #[test]
    fn the_arrows_and_enter_wrap_all_four_lines() {
        // `editLine - 1 & 3` up, `editLine + 1 & 3` down/Enter
        // (`keyTyped`:94-102).
        let mut screen = SignScreen::new(0, 64, 0, empty_lines());
        screen.move_up();
        assert_eq!(screen.edit_line(), 3, "up wraps 0 to 3");
        screen.move_up();
        assert_eq!(screen.edit_line(), 2);
        screen.move_down();
        assert_eq!(screen.edit_line(), 3);
        screen.move_down();
        assert_eq!(screen.edit_line(), 0, "down wraps 3 to 0");
        for _ in 0..5 {
            screen.move_down();
        }
        assert_eq!(screen.edit_line(), 1, "Enter walks on from 0");
    }

    #[test]
    fn backspace_drops_the_last_character_only_when_non_empty() {
        // Key 14 drops the last char when non-empty (`keyTyped`:106-109).
        let mut screen = SignScreen::new(
            0,
            64,
            0,
            [
                "AB".to_string(),
                String::new(),
                String::new(),
                String::new(),
            ],
        );
        assert!(screen.backspace());
        assert_eq!(screen.lines()[0], "A");
        assert!(screen.backspace());
        assert_eq!(screen.lines()[0], "");
        assert!(!screen.backspace(), "empty stays empty");
        screen.move_down();
        assert!(!screen.backspace(), "the move went to line 1, still empty");
    }

    #[test]
    fn the_line_cap_is_the_ninety_pixel_width_test_alone() {
        // A typed char appends only under the width test
        // (`keyTyped`:111-114): `A` advances six, so fifteen land (90) and
        // the sixteenth refuses. There is NO character cap.
        let font = editor_font();
        let mut screen = SignScreen::new(0, 64, 0, empty_lines());
        for index in 0..15 {
            assert!(
                screen.type_char('A', &font),
                "character {index} lands inside 90 pixels"
            );
        }
        assert_eq!(string_width(&font, &screen.lines()[0]), 90);
        assert!(
            !screen.type_char('A', &font),
            "the sixteenth A would weigh 96"
        );
        assert_eq!(screen.lines()[0].chars().count(), 15);
        // The cap is per line: the other three still take text.
        screen.move_down();
        assert!(screen.type_char('B', &font));
    }

    #[test]
    fn disallowed_characters_never_land_whatever_the_width() {
        // `ChatAllowedCharacters.isAllowedCharacter` gates first: `§`,
        // control characters and DEL refuse.
        let font = editor_font();
        let mut screen = SignScreen::new(0, 64, 0, empty_lines());
        for c in ['§', '\n', '\u{7f}', '\t'] {
            assert!(!screen.type_char(c, &font), "{c:?} refuses");
        }
        assert_eq!(screen.lines()[0], "");
    }

    #[test]
    fn the_editing_line_wears_the_wrap_only_in_the_visible_phase() {
        // `"> " + line + " <"` while the blink shows it; plain otherwise —
        // and never on the other three lines.
        let mut screen = SignScreen::new(
            0,
            64,
            0,
            [
                "AB".to_string(),
                "C".to_string(),
                String::new(),
                String::new(),
            ],
        );
        assert_eq!(
            screen.display_lines(),
            [
                "> AB <".to_string(),
                "C".to_string(),
                String::new(),
                String::new()
            ]
        );
        for _ in 0..6 {
            screen.tick();
        }
        assert_eq!(
            screen.display_lines()[0],
            "AB",
            "the dark phase drops the wrap"
        );
        screen.move_down();
        assert_eq!(screen.display_lines()[1], "C", "line 1 is edited but dark");
        for _ in 0..6 {
            screen.tick();
        }
        assert_eq!(
            screen.display_lines(),
            [
                "AB".to_string(),
                "> C <".to_string(),
                String::new(),
                String::new()
            ]
        );
    }

    #[test]
    fn every_close_sends_the_update_with_the_position_and_lines() {
        // `onGuiClosed` (`:52-63`) queues the update on every close — Done
        // and Escape alike — so the close event carries all four lines.
        let mut screen = SignScreen::new(4, 65, -9, empty_lines());
        let font = editor_font();
        screen.type_text("AB", &font);
        screen.move_down();
        screen.type_text("C", &font);
        assert_eq!(
            screen.close_event(),
            InputEvent::UpdateSign {
                x: 4,
                y: 65,
                z: -9,
                lines: [
                    "AB".to_string(),
                    "C".to_string(),
                    String::new(),
                    String::new()
                ],
            }
        );
    }

    #[test]
    fn the_done_button_stands_at_the_sources_rect() {
        // `GuiButton(0, width / 2 - 100, height / 4 + 120, ...)`, 200x20
        // (`initGui`:44): at 427x240 the rect is (113.5, 180) to (313.5,
        // 200).
        let scaled = oxide_render::hud::scaled_resolution(1280, 720, 0);
        assert_eq!(scaled.width, 427);
        assert_eq!(scaled.height, 240);
        let (x, y, w, h) = SignScreen::done_bounds(&scaled);
        assert_eq!((x, y, w, h), (113.5, 180.0, 200.0, 20.0));
        assert!(SignScreen::done_pressed(&scaled, (200.0, 190.0)));
        assert!(!SignScreen::done_pressed(&scaled, (100.0, 190.0)));
        assert!(!SignScreen::done_pressed(&scaled, (200.0, 179.0)));
    }

    #[test]
    fn the_done_button_rests_on_the_idle_row() {
        // `GuiButton.drawButton` blits `46 + i * 20` with `i = 1` idle — the
        // enabled strip at v 66; v 46 is the disabled row the port
        // hardcodes today.
        let font = editor_font();
        let scaled = oxide_render::hud::scaled_resolution(1280, 720, 0);
        let screen = SignScreen::new(0, 64, 0, empty_lines());
        let blit = screen
            .draws(&font, &scaled, None)
            .into_iter()
            .find_map(|draw| match draw {
                HudDraw::TexturedRect {
                    texture: HudTexture::Named(SIGN_WIDGETS_SHEET),
                    uv,
                    ..
                } => Some(uv),
                _ => None,
            })
            .expect("the Done button blits the widgets sheet");
        assert_eq!(
            blit,
            [0.0, 66.0 / 256.0, 200.0 / 256.0, 86.0 / 256.0],
            "the idle strip"
        );
    }

    #[test]
    fn the_done_button_stays_idle_while_the_pointer_stands_off_it() {
        // Off the button — and before the first move — the strip stays v 66
        // and the label 14737632 (0xE0E0E0).
        let font = editor_font();
        let scaled = oxide_render::hud::scaled_resolution(1280, 720, 0);
        let screen = SignScreen::new(0, 64, 0, empty_lines());
        for mouse in [None, Some((100.0, 50.0))] {
            let draws = screen.draws(&font, &scaled, mouse);
            let blit = draws
                .iter()
                .find_map(|draw| match draw {
                    HudDraw::TexturedRect {
                        texture: HudTexture::Named(SIGN_WIDGETS_SHEET),
                        uv,
                        ..
                    } => Some(*uv),
                    _ => None,
                })
                .expect("the Done button blits the widgets sheet");
            assert_eq!(
                blit,
                [0.0, 66.0 / 256.0, 200.0 / 256.0, 86.0 / 256.0],
                "the idle strip for {mouse:?}"
            );
            let label = draws
                .iter()
                .find_map(|draw| match draw {
                    HudDraw::Text { text, colour, .. } if text == SIGN_DONE_TEXT => Some(*colour),
                    _ => None,
                })
                .expect("the Done label draws");
            assert_eq!(
                label,
                [224.0 / 255.0, 224.0 / 255.0, 224.0 / 255.0, 1.0],
                "the idle tint for {mouse:?}"
            );
        }
    }

    #[test]
    fn the_hovered_done_button_takes_the_hot_row_and_tint() {
        // `i = 2` hovered: the hot strip at v 86, the label 16777120
        // (0xFFFFA0). At 427x240 the Done rect is (113.5, 180, 200, 20),
        // so (213.5, 190.0) stands on it.
        let font = editor_font();
        let scaled = oxide_render::hud::scaled_resolution(1280, 720, 0);
        let screen = SignScreen::new(0, 64, 0, empty_lines());
        assert!(SignScreen::done_pressed(&scaled, (213.5, 190.0)));
        let draws = screen.draws(&font, &scaled, Some((213.5, 190.0)));
        let blit = draws
            .iter()
            .find_map(|draw| match draw {
                HudDraw::TexturedRect {
                    texture: HudTexture::Named(SIGN_WIDGETS_SHEET),
                    uv,
                    ..
                } => Some(*uv),
                _ => None,
            })
            .expect("the Done button blits the widgets sheet");
        assert_eq!(
            blit,
            [0.0, 86.0 / 256.0, 200.0 / 256.0, 106.0 / 256.0],
            "the hot strip"
        );
        let label = draws
            .iter()
            .find_map(|draw| match draw {
                HudDraw::Text { text, colour, .. } if text == SIGN_DONE_TEXT => Some(*colour),
                _ => None,
            })
            .expect("the Done label draws");
        assert_eq!(label, [1.0, 1.0, 160.0 / 255.0, 1.0], "the hovered tint");
    }

    #[test]
    fn the_draws_carry_the_title_lines_and_done_button() {
        // The title, the four display lines over the backing, the button
        // blit and its label: ten draws over the frame's own background.
        let font = editor_font();
        let scaled = oxide_render::hud::scaled_resolution(1280, 720, 0);
        let screen = SignScreen::new(
            0,
            64,
            0,
            [
                "AB".to_string(),
                String::new(),
                String::new(),
                String::new(),
            ],
        );
        let draws = screen.draws(&font, &scaled, None);
        let texts: Vec<&str> = draws
            .iter()
            .filter_map(|draw| match draw {
                HudDraw::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            texts.contains(&"Edit sign message"),
            "the title draws: {texts:?}"
        );
        assert!(texts.contains(&"> AB <"), "the wrapped line draws");
        assert!(texts.contains(&"Done"), "the button label draws");
        assert_eq!(texts.len(), 6, "title, four lines, label: {texts:?}");
        assert_eq!(draws.len(), 8, "backing, title, four lines, blit, label");
    }
}
