//! The book reader: the screen `MC|BOpen` stands on the held written book,
//! and the read-only view an editable book opens on its stored pages.
//!
//! The port reads values and names from the source's reader
//! (`client/gui/GuiScreenBook.java`), its open path
//! (`NetHandlerPlayClient.handleCustomPayload:1855-1863` reaching the reader
//! with the held stack) and the send
//! (`EntityPlayerMP.displayGUIBook:840-848`, an empty `MC|BOpen` buffer); no
//! source text is copied.
//!
//! The reading branch draws the page indicator plus the page text only — no
//! author/title line lives here (`drawScreen:444-515`; the author is the
//! signing view's `book.byAuthor` and the tooltip's, `:417-443` and
//! `ItemEditableBook:71-85`).

use oxide_assets::font::Font;
use oxide_proto_v47::entity::MetadataItem;
use oxide_proto_v47::nbt::{NbtValue, parse};
use oxide_render::hud::{HudDraw, HudTexture, ScaledResolution};
use oxide_render::text::string_width;

/// The reader's sheet: `textures/gui/book.png`
/// (`GuiScreenBook.java:33`, `bookGuiTextures`), under the extraction tree's
/// key — the tree keys sheets by the path below `textures/` without the
/// extension, so the draw names `gui/book`.
pub const BOOK_SHEET: &str = "gui/book";
/// The sheet's frame: `bookImageWidth = bookImageHeight = 192`
/// (`GuiScreenBook.java:52-53`), blitted whole at `i, j`
/// (`drawScreen:411-415`).
pub const BOOK_IMAGE_WIDTH: f32 = 192.0;
/// The sheet frame's height (see [`BOOK_IMAGE_WIDTH`]).
pub const BOOK_IMAGE_HEIGHT: f32 = 192.0;
/// The frame's top edge in GUI pixels: `j = 2` (`initGui`, `drawScreen`).
pub const BOOK_SHEET_Y: f32 = 2.0;
/// The page text's left inset from the frame: `i + 36` (`drawScreen`, both
/// the split path and the cached-lines path).
pub const BOOK_TEXT_X_OFFSET: f32 = 36.0;
/// The page text's top inset from the frame's top: `j + 16 + 16`
/// (`drawScreen`) — the indicator's row plus one row.
pub const BOOK_TEXT_Y_OFFSET: f32 = 32.0;
/// The page text's wrap width in font pixels: 116 (`drawScreen`'s
/// `drawSplitString` width and `splitText` width alike). The unsigned edit
/// gate's 118 is the editor's, not the reader's.
pub const BOOK_TEXT_WIDTH: i32 = 116;
/// How many wrapped lines of one page draw: `min(128 / FONT_HEIGHT, size)`
/// (`drawScreen`, the signed cached-lines path) with `FONT_HEIGHT = 9`
/// (`FontRenderer.java:35`) — fourteen lines. The unsigned split path draws
/// unclamped; see [`BookScreen::visible_lines`].
pub const BOOK_MAX_VISIBLE_LINES: usize = 14;
/// The indicator's top inset from the frame's top: `j + 16` (`drawScreen`).
pub const BOOK_INDICATOR_Y_OFFSET: f32 = 16.0;
/// The indicator's right inset from the frame's right: drawn right-aligned
/// at `i - width + bookImageWidth - 44` (`drawScreen`).
pub const BOOK_INDICATOR_RIGHT_INSET: f32 = 44.0;
/// The Done button's label: `I18n "gui.done"` (`initGui`), English
/// `gui.done=Done`.
pub const BOOK_DONE_TEXT: &str = "Done";
/// The Done button's width: the signed reader's full-width 200
/// (`initGui`, the `!bookIsUnsigned` arm). The read-only port draws this
/// rect for the editable book too — no Sign button stands beside it
/// (a port choice, recorded).
pub const BOOK_DONE_WIDTH: f32 = 200.0;
/// The Done button's height (see [`BOOK_DONE_WIDTH`]).
pub const BOOK_DONE_HEIGHT: f32 = 20.0;
/// The Done button's top inset below the frame: `4 + bookImageHeight`
/// (`initGui`).
pub const BOOK_DONE_Y_OFFSET: f32 = 4.0;
/// The Done label's idle colour: `GuiButton`'s enabled text, 14737632
/// (0xE0E0E0).
pub const BOOK_DONE_COLOUR: [f32; 4] = [224.0 / 255.0, 224.0 / 255.0, 224.0 / 255.0, 1.0];
/// The Done label's hovered colour: `GuiButton`'s hovered text, 16777120
/// (0xFFFFA0).
pub const BOOK_DONE_COLOUR_HOVER: [f32; 4] = [1.0, 1.0, 160.0 / 255.0, 1.0];
/// The widgets sheet the Done button blits (`GuiButton`'s own sheet).
pub const BOOK_WIDGETS_SHEET: &str = "gui/widgets";
/// The Done strip's idle row: `GuiButton` blits `46 + 1 * 20` off-hover.
pub const BOOK_BUTTON_V_IDLE: f32 = 66.0;
/// The Done strip's hovered row: `46 + 2 * 20` on-hover.
pub const BOOK_BUTTON_V_HOVER: f32 = 86.0;
/// A page-turn arrow's size: `NextPageButton`'s 23x13 (its constructor).
pub const BOOK_ARROW_WIDTH: f32 = 23.0;
/// A page-turn arrow's height (see [`BOOK_ARROW_WIDTH`]).
pub const BOOK_ARROW_HEIGHT: f32 = 13.0;
/// The next arrow's left inset from the frame: `i + 120` (`initGui`).
pub const BOOK_NEXT_X_OFFSET: f32 = 120.0;
/// The previous arrow's left inset from the frame: `i + 38` (`initGui`).
pub const BOOK_PREV_X_OFFSET: f32 = 38.0;
/// Both arrows' top inset from the frame's top: `j + 154` (`initGui`).
pub const BOOK_ARROW_Y_OFFSET: f32 = 154.0;
/// The arrows' idle column: `u = 0` off-hover (`NextPageButton.drawButton`).
pub const BOOK_ARROW_U_IDLE: f32 = 0.0;
/// The arrows' hovered column: `u = 23` on-hover.
pub const BOOK_ARROW_U_HOVER: f32 = 23.0;
/// The next arrow's row: `v = 192` (`drawButton`).
pub const BOOK_ARROW_V_NEXT: f32 = 192.0;
/// The previous arrow's row: `v = 192 + 13` (`drawButton`'s `j += 13`).
pub const BOOK_ARROW_V_PREV: f32 = 205.0;
/// The signed book's missing/malformed-`pages` line:
/// `EnumChatFormatting.DARK_RED + "* Invalid book tag *"` (`drawScreen`),
/// DARK_RED being the `4` colour code — the shared text path paints it.
pub const INVALID_BOOK_TAG: &str = "§4* Invalid book tag *";
/// The written book's item id: 387 (`ItemEditableBook:146-149`, the item
/// table). The open guard tests this identity — `== Items.written_book`,
/// never `instanceof` (`EntityPlayerMP:844`, `NetHandlerPlayClient:1859`).
pub const WRITTEN_BOOK_ID: i16 = 387;
/// The editable book's item id: 386 (the item table, `Book and Quill`).
pub const WRITABLE_BOOK_ID: i16 = 386;
/// The title tag a valid signed book carries (`validBookTagContents`).
pub const BOOK_TITLE_KEY: &str = "title";
/// The author tag a valid signed book carries (`validBookTagContents`).
pub const BOOK_AUTHOR_KEY: &str = "author";
/// The pages list tag both book kinds carry (`getTagList("pages", 8)`).
pub const BOOK_PAGES_KEY: &str = "pages";

/// The page indicator: `I18n "book.pageIndicator"` (`drawScreen`), English
/// `book.pageIndicator=Page %1$s of %2$s` — one-based page over the total.
pub fn page_indicator(current: usize, total: usize) -> String {
    format!("Page {current} of {total}")
}

/// Splits one page into its drawn lines at the book's width: the source's
/// `wrapFormattedStringToWidth` over `sizeStringToWidth` with the trailing
/// newlines trimmed first (`drawSplitString:786-799`), measured with the
/// reader's own font. A `\n` breaks the line and is eaten; otherwise the
/// break backtracks to the last space inside the width, and a lineless
/// overflow cuts mid-word. The active `§` formats carry onto the next line
/// (`getFormatFromString`).
pub fn wrap_lines(font: &Font, text: &str, width: i32) -> Vec<String> {
    let mut trimmed = text.to_string();
    while trimmed.ends_with('\n') {
        trimmed.pop();
    }
    let mut lines = Vec::new();
    let mut rest = trimmed;
    loop {
        let chars: Vec<char> = rest.chars().collect();
        let mut take = break_index(font, &rest, width);
        if take >= chars.len() {
            lines.push(rest);
            break;
        }
        if take == 0 {
            // A zero-width call can never fill a line; take one character
            // so the loop still makes progress (the reader always wraps at
            // 116, so this arm never runs there).
            take = 1;
        }
        let head: String = chars[..take].iter().collect();
        let mut tail: String = chars[take..].iter().collect();
        if tail.starts_with(' ') || tail.starts_with('\n') {
            tail = tail.chars().skip(1).collect();
        }
        let carried = active_format(&head);
        lines.push(head);
        rest = format!("{carried}{tail}");
    }
    lines
}

/// How many leading characters of `text` fill one line: the source's
/// `sizeStringToWidth` (`FontRenderer.java:873-921`) — `\n` breaks before
/// itself, the last space inside the width wins the backtrack, and a
/// lineless overflow cuts mid-word. Widths come from the shared
/// [`string_width`], which carries the source's own `§` and bold quirks.
fn break_index(font: &Font, text: &str, width: i32) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut last_space: Option<usize> = None;
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if c == '\n' {
            return index;
        }
        if c == ' ' {
            last_space = Some(index);
        }
        let prefix: String = chars[..=index].iter().collect();
        if string_width(font, &prefix) > width {
            break;
        }
        index += 1;
    }
    if index != chars.len() {
        if let Some(space) = last_space {
            if space < index {
                return space;
            }
        }
    }
    index
}

/// The `§` formats still active at a line's end, carried onto the next
/// line: the source's `getFormatFromString` — a colour code replaces the
/// carry, a style code appends to it.
fn active_format(line: &str) -> String {
    let mut carried = String::new();
    let chars: Vec<char> = line.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '§' && index + 1 < chars.len() {
            let code = chars[index + 1];
            if is_format_colour(code) {
                carried = format!("§{code}");
            } else if is_format_special(code) {
                carried = format!("{carried}§{code}");
            }
            index += 1;
        }
        index += 1;
    }
    carried
}

/// Whether the code selects a colour (`FontRenderer.isFormatColor`).
fn is_format_colour(code: char) -> bool {
    code.is_ascii_hexdigit()
}

/// Whether the code selects a style or reset
/// (`FontRenderer.isFormatSpecial`).
fn is_format_special(code: char) -> bool {
    matches!(code, 'k'..='o' | 'K'..='O' | 'r' | 'R')
}

/// One JSON-encoded signed page's plain text: the `{"text": ...}` shape the
/// signing send writes (`sendBookToServer` JSON-encodes each page), read
/// without command-component processing (a port choice — the resolve step
/// is out of scope). `None` when the page is not JSON text, and the reader
/// draws the raw string then (the source's parse-failure arm, which leaves
/// the cached lines null so the raw `drawSplitString` runs).
pub fn json_text(page: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(page).ok()?;
    Some(json_string(&value))
}

/// One JSON value's plain text: strings read as-is, objects read their
/// `text` plus every `extra` entry in order, arrays concatenate — the
/// shapes the signing send writes.
fn json_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(items) => items.iter().map(json_string).collect(),
        serde_json::Value::Object(map) => {
            let mut out = String::new();
            if let Some(text) = map.get("text") {
                out.push_str(&json_string(text));
            }
            if let Some(extra) = map.get("extra") {
                out.push_str(&json_string(extra));
            }
            out
        }
        _ => String::new(),
    }
}

/// One named child of a compound.
fn child<'a>(compound: &'a [(String, NbtValue)], key: &str) -> Option<&'a NbtValue> {
    compound
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

/// The stack's `pages` strings through Task 1's reader: the root compound's
/// `pages` list of strings (`getTagList("pages", 8)`'s copy). The fallbacks
/// split by kind — unsigned + missing/malformed list reads one empty page
/// (the fresh `[""]`), signed + missing/malformed reads the single
/// dark-red invalid-tag line (`drawScreen:469-490`, both totals ending at
/// one). There is no page-count cap on the read — the NBT and collection
/// caps bound the wire — a port choice, recorded.
fn read_pages(stack: &MetadataItem, unsigned: bool) -> Vec<String> {
    let root = stack.nbt.as_deref().and_then(|tail| match parse(tail) {
        Ok(NbtValue::Compound(children)) => Some(children),
        _ => None,
    });
    let list = root
        .as_ref()
        .and_then(|root| match child(root, BOOK_PAGES_KEY) {
            Some(NbtValue::List(items)) => Some(items.clone()),
            _ => None,
        });
    if unsigned {
        match list {
            Some(items) => {
                let mut pages = Vec::with_capacity(items.len());
                for item in &items {
                    match item {
                        NbtValue::String(text) => pages.push(text.clone()),
                        _ => return vec![String::new()],
                    }
                }
                if pages.is_empty() {
                    pages.push(String::new());
                }
                pages
            }
            None => vec![String::new()],
        }
    } else {
        let valid = match (&root, &list) {
            (Some(root), Some(items)) => items
                .iter()
                .all(|item| matches!(item, NbtValue::String(_)))
                && child(root, BOOK_TITLE_KEY).is_some_and(
                    |title| matches!(title, NbtValue::String(text) if text.chars().count() <= 32),
                )
                && child(root, BOOK_AUTHOR_KEY)
                    .is_some_and(|author| matches!(author, NbtValue::String(_))),
            _ => false,
        };
        if !valid {
            return vec![INVALID_BOOK_TAG.to_string()];
        }
        list.map(|items| {
            items
                .iter()
                .map(|item| match item {
                    NbtValue::String(page) => json_text(page).unwrap_or_else(|| page.clone()),
                    _ => String::new(),
                })
                .collect()
        })
        .unwrap_or_else(|| vec![INVALID_BOOK_TAG.to_string()])
    }
}

/// The reader over one book stack: the held stack the open snapshotted,
/// its pages, and the page standing open (`currPage`, a field default of 0
/// — the reader opens on the first page).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookScreen {
    /// The book stack being read.
    stack: MetadataItem,
    /// The pages as read: the NBT `pages` strings (signed pages
    /// JSON-decoded), or the fallback — one empty page for the editable
    /// book, the invalid-tag line for the written one.
    pages: Vec<String>,
    /// The open page, 0-based (`currPage`).
    page: usize,
}

impl BookScreen {
    /// Stands the reader on the stack: page 0, the pages read through
    /// Task 1's reader (`getTagList("pages", 8)`'s copy,
    /// `GuiScreenBook:69-98`).
    pub fn new(stack: MetadataItem) -> Self {
        let unsigned = stack.id == WRITABLE_BOOK_ID;
        let pages = read_pages(&stack, unsigned);
        Self {
            stack,
            pages,
            page: 0,
        }
    }

    /// The book stack being read.
    pub fn stack(&self) -> &MetadataItem {
        &self.stack
    }

    /// Whether the book is the editable kind (386): its missing list falls
    /// back to one empty page, and the written kind (387) to the
    /// invalid-tag line.
    pub fn is_unsigned(&self) -> bool {
        self.stack.id == WRITABLE_BOOK_ID
    }

    /// The pages as read.
    pub fn pages(&self) -> &[String] {
        &self.pages
    }

    /// The page count the indicator totals: the list's length floored to
    /// one (`if (bookTotalPages < 1) bookTotalPages = 1`).
    pub fn total_pages(&self) -> usize {
        self.pages.len().max(1)
    }

    /// The open page, 0-based.
    pub fn current_page(&self) -> usize {
        self.page
    }

    /// The open page's stored text: the list's entry, or the empty string
    /// past its end (the source reads `""` when the index is out of range).
    pub fn current_text(&self) -> &str {
        self.pages.get(self.page).map(String::as_str).unwrap_or("")
    }

    /// The open page's wrapped lines at the book's width.
    pub fn page_lines(&self, font: &Font) -> Vec<String> {
        wrap_lines(font, self.current_text(), BOOK_TEXT_WIDTH)
    }

    /// The open page's drawn lines: the signed path keeps the source's
    /// fourteen-line clamp (`min(128 / FONT_HEIGHT, size)`); the unsigned
    /// split path draws every wrapped line.
    pub fn visible_lines(&self, font: &Font) -> Vec<String> {
        let mut lines = self.page_lines(font);
        if !self.is_unsigned() {
            lines.truncate(BOOK_MAX_VISIBLE_LINES);
        }
        lines
    }

    /// Turns one page forward when a later page stands open
    /// (`actionPerformed` id 1, the read-only arm — the unsigned add-page
    /// is the editor's, not the reader's). Answers whether the page moved.
    pub fn next_page(&mut self) -> bool {
        if self.page + 1 < self.total_pages() {
            self.page += 1;
            true
        } else {
            false
        }
    }

    /// Turns one page back when a page stands behind (`actionPerformed` id
    /// 2). Answers whether the page moved.
    pub fn prev_page(&mut self) -> bool {
        if self.page > 0 {
            self.page -= 1;
            true
        } else {
            false
        }
    }

    /// Whether the next arrow shows: a later page stands open
    /// (`updateButtons`, the read-only arm).
    pub fn next_visible(&self) -> bool {
        self.page + 1 < self.total_pages()
    }

    /// Whether the previous arrow shows: the page is past the first
    /// (`updateButtons`).
    pub fn prev_visible(&self) -> bool {
        self.page > 0
    }

    /// The frame's left edge in GUI pixels: `(width - 192) / 2`.
    pub fn frame_x(scaled: &ScaledResolution) -> f32 {
        scaled.width as f32 / 2.0 - BOOK_IMAGE_WIDTH / 2.0
    }

    /// The next arrow's rect in GUI pixels.
    pub fn next_bounds(scaled: &ScaledResolution) -> (f32, f32, f32, f32) {
        (
            Self::frame_x(scaled) + BOOK_NEXT_X_OFFSET,
            BOOK_SHEET_Y + BOOK_ARROW_Y_OFFSET,
            BOOK_ARROW_WIDTH,
            BOOK_ARROW_HEIGHT,
        )
    }

    /// The previous arrow's rect in GUI pixels.
    pub fn prev_bounds(scaled: &ScaledResolution) -> (f32, f32, f32, f32) {
        (
            Self::frame_x(scaled) + BOOK_PREV_X_OFFSET,
            BOOK_SHEET_Y + BOOK_ARROW_Y_OFFSET,
            BOOK_ARROW_WIDTH,
            BOOK_ARROW_HEIGHT,
        )
    }

    /// The Done button's rect in GUI pixels: the signed reader's centred
    /// full-width 200x20 at `4 + 192`.
    pub fn done_bounds(scaled: &ScaledResolution) -> (f32, f32, f32, f32) {
        (
            scaled.width as f32 / 2.0 - BOOK_DONE_WIDTH / 2.0,
            BOOK_DONE_Y_OFFSET + BOOK_IMAGE_HEIGHT,
            BOOK_DONE_WIDTH,
            BOOK_DONE_HEIGHT,
        )
    }

    /// Whether the GUI-space point stands on a rect.
    fn on_rect(bounds: (f32, f32, f32, f32), point: (f32, f32)) -> bool {
        let (x, y, w, h) = bounds;
        point.0 >= x && point.0 < x + w && point.1 >= y && point.1 < y + h
    }

    /// One press's answer: the Done button, then the visible arrows —
    /// nothing else on the reader answers the pointer (the `CHANGE_PAGE`
    /// link clicks ride the component model the read-only port does not
    /// keep — a port choice, recorded).
    pub fn click_at(&self, scaled: &ScaledResolution, point: (f32, f32)) -> BookClick {
        if Self::on_rect(Self::done_bounds(scaled), point) {
            return BookClick::Done;
        }
        if self.next_visible() && Self::on_rect(Self::next_bounds(scaled), point) {
            return BookClick::Next;
        }
        if self.prev_visible() && Self::on_rect(Self::prev_bounds(scaled), point) {
            return BookClick::Prev;
        }
        BookClick::None
    }

    /// The reader's draws over the background: the 192x192 sheet, the
    /// right-aligned page indicator, the wrapped page text in black, the
    /// visible arrows from the sheet's two-state sprites, and the Done
    /// button with its label. No author/title line draws in the reading
    /// view. The arrows' hovered column and the Done hovered strip follow
    /// the free pointer — `None` (before the first move) draws idle.
    pub fn draws(
        &self,
        font: &Font,
        scaled: &ScaledResolution,
        mouse: Option<(f32, f32)>,
    ) -> Vec<HudDraw> {
        let mut draws = Vec::new();
        let frame_x = Self::frame_x(scaled);
        draws.push(HudDraw::TexturedRect {
            texture: HudTexture::Named(BOOK_SHEET),
            x: frame_x,
            y: BOOK_SHEET_Y,
            width: BOOK_IMAGE_WIDTH,
            height: BOOK_IMAGE_HEIGHT,
            uv: [
                0.0,
                0.0,
                BOOK_IMAGE_WIDTH / 256.0,
                BOOK_IMAGE_HEIGHT / 256.0,
            ],
            colour: [1.0, 1.0, 1.0, 1.0],
        });
        let indicator = page_indicator(self.page + 1, self.total_pages());
        let indicator_width = string_width(font, &indicator);
        draws.push(HudDraw::Text {
            text: indicator,
            x: frame_x - indicator_width as f32 + BOOK_IMAGE_WIDTH - BOOK_INDICATOR_RIGHT_INSET,
            y: BOOK_SHEET_Y + BOOK_INDICATOR_Y_OFFSET,
            scale: 1.0,
            colour: [0.0, 0.0, 0.0, 1.0],
            shadow: false,
            blend: true,
        });
        let line_pitch = font.height() as f32;
        for (row, line) in self.visible_lines(font).iter().enumerate() {
            draws.push(HudDraw::Text {
                text: line.clone(),
                x: frame_x + BOOK_TEXT_X_OFFSET,
                y: BOOK_SHEET_Y + BOOK_TEXT_Y_OFFSET + row as f32 * line_pitch,
                scale: 1.0,
                colour: [0.0, 0.0, 0.0, 1.0],
                shadow: false,
                blend: true,
            });
        }
        if self.next_visible() {
            let bounds = Self::next_bounds(scaled);
            let hovered = mouse.is_some_and(|point| Self::on_rect(bounds, point));
            draws.push(arrow_draw(bounds, hovered, true));
        }
        if self.prev_visible() {
            let bounds = Self::prev_bounds(scaled);
            let hovered = mouse.is_some_and(|point| Self::on_rect(bounds, point));
            draws.push(arrow_draw(bounds, hovered, false));
        }
        let (bx, by, bw, bh) = Self::done_bounds(scaled);
        let hovered = mouse.is_some_and(|point| Self::on_rect(Self::done_bounds(scaled), point));
        let button_v = if hovered {
            BOOK_BUTTON_V_HOVER
        } else {
            BOOK_BUTTON_V_IDLE
        };
        let label_colour = if hovered {
            BOOK_DONE_COLOUR_HOVER
        } else {
            BOOK_DONE_COLOUR
        };
        draws.push(HudDraw::TexturedRect {
            texture: HudTexture::Named(BOOK_WIDGETS_SHEET),
            x: bx,
            y: by,
            width: bw,
            height: bh,
            uv: [
                0.0,
                button_v / 256.0,
                BOOK_DONE_WIDTH / 256.0,
                (button_v + BOOK_DONE_HEIGHT) / 256.0,
            ],
            colour: [1.0, 1.0, 1.0, 1.0],
        });
        let label_width = string_width(font, BOOK_DONE_TEXT);
        draws.push(HudDraw::Text {
            text: BOOK_DONE_TEXT.to_string(),
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

/// One page-turn arrow's draw from the sheet's two-state sprites: `u = 23`
/// on-hover else `0`, `v = 192` for next else `205`
/// (`NextPageButton.drawButton`).
fn arrow_draw(bounds: (f32, f32, f32, f32), hovered: bool, next: bool) -> HudDraw {
    let (x, y, w, h) = bounds;
    let u = if hovered {
        BOOK_ARROW_U_HOVER
    } else {
        BOOK_ARROW_U_IDLE
    };
    let v = if next {
        BOOK_ARROW_V_NEXT
    } else {
        BOOK_ARROW_V_PREV
    };
    HudDraw::TexturedRect {
        texture: HudTexture::Named(BOOK_SHEET),
        x,
        y,
        width: w,
        height: h,
        uv: [
            u / 256.0,
            v / 256.0,
            (u + BOOK_ARROW_WIDTH) / 256.0,
            (v + BOOK_ARROW_HEIGHT) / 256.0,
        ],
        colour: [1.0, 1.0, 1.0, 1.0],
    }
}

/// One press on the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookClick {
    /// The Done button: the reader closes (the close sends nothing — the
    /// unsigned `MC|BEdit` send is the editor's, not the reader's).
    Done,
    /// The next arrow: the page turns forward.
    Next,
    /// The previous arrow: the page turns back.
    Prev,
    /// Anywhere else: the reader swallows it.
    None,
}

#[cfg(test)]
mod tests {
    //! The reader's own behaviour: the open defaults, the page turns and
    //! their bounds, the wrap rule at the book's width, both `pages`
    //! fallbacks, and the indicator's text.

    use super::*;

    /// The suite's synthetic font: every printable cell carries a one-texel
    /// left column, so each glyph advances two and the space four — widths
    /// stay exact without the store.
    fn reader_font() -> Font {
        const SIDE: u32 = 128;
        const CELL: u32 = 8;
        let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
        for code in 33..=126u32 {
            let cell_x = (code % 16) * CELL;
            let cell_y = (code / 16) * CELL;
            for row in 0..CELL {
                let offset = (((cell_y + row) * SIDE + cell_x) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
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
        .expect("the synthetic sheet is a 16x16 grid")
    }

    /// One NBT string's bytes: the length, then the UTF-8.
    fn nbt_string(text: &str) -> Vec<u8> {
        let mut out = (text.len() as u16).to_be_bytes().to_vec();
        out.extend(text.as_bytes());
        out
    }

    /// One named tag's bytes: the id, the name, then the payload.
    fn tag(id: u8, name: &str, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![id];
        out.extend(nbt_string(name));
        out.extend(payload);
        out
    }

    /// A compound payload's bytes: the children, then the end tag.
    fn compound(children: Vec<u8>) -> Vec<u8> {
        let mut out = children;
        out.push(0);
        out
    }

    /// A list payload's bytes: the element type, the count, then the raw
    /// elements.
    fn list(element: u8, elements: Vec<Vec<u8>>) -> Vec<u8> {
        let mut out = vec![element];
        out.extend((elements.len() as i32).to_be_bytes());
        for element in elements {
            out.extend(element);
        }
        out
    }

    /// A slot's raw NBT tail: the root compound with an empty name.
    fn root(children: Vec<u8>) -> Vec<u8> {
        let mut out = vec![10];
        out.extend(nbt_string(""));
        out.extend(compound(children));
        out
    }

    /// One unsigned page entry's bytes.
    fn page_entry(text: &str) -> Vec<u8> {
        nbt_string(text)
    }

    fn book_stack(id: i16, nbt: Option<Vec<u8>>) -> MetadataItem {
        MetadataItem {
            id,
            count: 1,
            damage: 0,
            nbt,
        }
    }

    /// A written book's tail: the title, the author and the JSON pages a
    /// valid signed book carries.
    fn signed_tail(pages: &[&str]) -> Vec<u8> {
        let entries = pages
            .iter()
            .map(|page| page_entry(&format!("{{\"text\":\"{page}\"}}")))
            .collect();
        let mut children = tag(8, "title", &nbt_string("Tales"));
        children.extend(tag(8, "author", &nbt_string("Ada")));
        children.extend(tag(9, "pages", &list(8, entries)));
        root(children)
    }

    /// An editable book's tail: the plain `pages` strings.
    fn unsigned_tail(pages: &[&str]) -> Vec<u8> {
        let entries = pages.iter().map(|page| page_entry(page)).collect();
        root(tag(9, "pages", &list(8, entries)))
    }

    fn scaled_448() -> ScaledResolution {
        ScaledResolution {
            width: 448,
            height: 240,
            scale_factor: 2,
        }
    }

    #[test]
    fn the_reader_opens_on_the_first_page_of_its_pages() {
        // The constructor copies the `pages` list (`GuiScreenBook:69-98`);
        // `currPage` is the field default 0.
        let screen = BookScreen::new(book_stack(
            WRITTEN_BOOK_ID,
            Some(signed_tail(&["one", "two", "three"])),
        ));
        assert_eq!(screen.current_page(), 0);
        assert_eq!(screen.total_pages(), 3);
        assert_eq!(screen.current_text(), "one");
        assert!(!screen.is_unsigned(), "387 is the written kind");
    }

    #[test]
    fn the_page_turn_stops_at_both_ends() {
        // `actionPerformed` ids 1/2 with `updateButtons` after: forward
        // while a later page stands open, back while past the first.
        let mut screen = BookScreen::new(book_stack(
            WRITTEN_BOOK_ID,
            Some(signed_tail(&["one", "two", "three"])),
        ));
        assert!(!screen.prev_page(), "the first page turns no further back");
        assert!(!screen.prev_visible(), "no previous arrow on page one");
        assert!(screen.next_visible());
        assert!(screen.next_page());
        assert_eq!(screen.current_page(), 1);
        assert!(screen.next_visible() && screen.prev_visible());
        assert!(screen.next_page());
        assert_eq!(screen.current_text(), "three");
        assert!(!screen.next_page(), "the last page turns no further on");
        assert!(!screen.next_visible(), "no next arrow on the last page");
        assert!(screen.prev_page());
        assert_eq!(screen.current_page(), 1);
    }

    #[test]
    fn the_wrap_breaks_at_the_books_width_with_space_backtrack() {
        // `sizeStringToWidth` at 116 with 2-pixel advances: 58 glyphs fit,
        // so a 70-glyph wordless run cuts mid-word at 58; a spaced run
        // backtracks to the last space inside the width.
        let font = reader_font();
        let run = "A".repeat(70);
        let lines = wrap_lines(&font, &run, BOOK_TEXT_WIDTH);
        assert_eq!(lines.len(), 2, "one overflow line: {lines:?}");
        assert_eq!(lines[0].chars().count(), 58);
        assert_eq!(lines[1].chars().count(), 12);
        let spaced = format!("{} {}", "B".repeat(50), "C".repeat(20));
        let lines = wrap_lines(&font, &spaced, BOOK_TEXT_WIDTH);
        assert_eq!(lines.len(), 2, "the space breaks the line: {lines:?}");
        assert_eq!(lines[0], "B".repeat(50));
        assert_eq!(lines[1], "C".repeat(20));
        let broken = "first\nsecond";
        assert_eq!(
            wrap_lines(&font, broken, BOOK_TEXT_WIDTH),
            vec!["first", "second"]
        );
    }

    #[test]
    fn the_signed_page_keeps_fourteen_lines() {
        // `min(128 / FONT_HEIGHT, size)` with `FONT_HEIGHT = 9`: fourteen
        // lines draw; the unsigned split path draws every wrapped line.
        let font = reader_font();
        let long = "A".repeat(58 * 20);
        let signed = BookScreen::new(book_stack(
            WRITTEN_BOOK_ID,
            Some(signed_tail(&[&*Box::leak(long.clone().into_boxed_str())])),
        ));
        let wrapped = wrap_lines(&font, &long, BOOK_TEXT_WIDTH);
        assert_eq!(wrapped.len(), 20, "the page wraps to twenty lines");
        assert_eq!(
            signed.visible_lines(&font).len(),
            BOOK_MAX_VISIBLE_LINES,
            "the signed clamp keeps fourteen"
        );
        let plain = BookScreen::new(book_stack(
            WRITABLE_BOOK_ID,
            Some(unsigned_tail(&[&*Box::leak(long.into_boxed_str())])),
        ));
        assert_eq!(
            plain.visible_lines(&font).len(),
            20,
            "the unsigned split path draws every line"
        );
    }

    #[test]
    fn an_unsigned_book_without_pages_opens_one_empty_page() {
        // `bookPages == null && isUnsigned`: a fresh `[""]`, total 1
        // (`GuiScreenBook:93-98`).
        let screen = BookScreen::new(book_stack(WRITABLE_BOOK_ID, None));
        assert!(screen.is_unsigned(), "386 is the editable kind");
        assert_eq!(screen.pages(), &["".to_string()]);
        assert_eq!(screen.total_pages(), 1);
        assert_eq!(screen.current_text(), "");
    }

    #[test]
    fn a_signed_book_without_pages_opens_the_invalid_line() {
        // Signed + missing list: `bookPages` stays null with total 1, and
        // the tag check fails, so the one dark-red line draws
        // (`drawScreen:469-490`).
        let screen = BookScreen::new(book_stack(WRITTEN_BOOK_ID, None));
        assert_eq!(screen.pages(), &[INVALID_BOOK_TAG.to_string()]);
        assert_eq!(screen.total_pages(), 1);
    }

    #[test]
    fn a_signed_book_with_a_malformed_list_opens_the_invalid_line() {
        // Malformed (an int list where strings belong, and a title/author
        // pair that cannot rescue it) draws the same single line.
        let children = tag(9, "pages", &list(3, vec![1i32.to_be_bytes().to_vec()]));
        let mut full = tag(8, "title", &nbt_string("Tales"));
        full.extend(tag(8, "author", &nbt_string("Ada")));
        full.extend(children);
        let screen = BookScreen::new(book_stack(WRITTEN_BOOK_ID, Some(root(full))));
        assert_eq!(screen.pages(), &[INVALID_BOOK_TAG.to_string()]);
        assert_eq!(screen.total_pages(), 1);
    }

    #[test]
    fn the_indicator_counts_pages_from_one() {
        // `I18n "book.pageIndicator"`, English `Page %1$s of %2$s`.
        assert_eq!(page_indicator(1, 3), "Page 1 of 3");
        assert_eq!(page_indicator(3, 3), "Page 3 of 3");
        let mut screen = BookScreen::new(book_stack(
            WRITTEN_BOOK_ID,
            Some(signed_tail(&["one", "two", "three"])),
        ));
        screen.next_page();
        assert_eq!(
            page_indicator(screen.current_page() + 1, screen.total_pages()),
            "Page 2 of 3"
        );
    }

    #[test]
    fn the_arrows_stand_on_the_sources_rects() {
        // `initGui`: next `(i + 120, j + 154)`, prev `(i + 38, j + 154)`
        // with `i = (width - 192) / 2`, `j = 2` — 23x13 both.
        let scaled = scaled_448();
        assert_eq!(BookScreen::frame_x(&scaled), 128.0);
        assert_eq!(BookScreen::next_bounds(&scaled), (248.0, 156.0, 23.0, 13.0));
        assert_eq!(BookScreen::prev_bounds(&scaled), (166.0, 156.0, 23.0, 13.0));
        assert_eq!(
            BookScreen::done_bounds(&scaled),
            (124.0, 196.0, 200.0, 20.0)
        );
    }

    #[test]
    fn the_press_answers_done_then_the_visible_arrows() {
        let scaled = scaled_448();
        let mut screen = BookScreen::new(book_stack(
            WRITTEN_BOOK_ID,
            Some(signed_tail(&["one", "two", "three"])),
        ));
        assert_eq!(
            screen.click_at(&scaled, (200.0, 200.0)),
            BookClick::Done,
            "inside the Done rect"
        );
        assert_eq!(
            screen.click_at(&scaled, (250.0, 160.0)),
            BookClick::Next,
            "the next arrow shows on page one"
        );
        assert_eq!(
            screen.click_at(&scaled, (170.0, 160.0)),
            BookClick::None,
            "the previous arrow hides on page one"
        );
        screen.next_page();
        screen.next_page();
        assert_eq!(
            screen.click_at(&scaled, (250.0, 160.0)),
            BookClick::None,
            "the next arrow hides on the last page"
        );
        assert_eq!(
            screen.click_at(&scaled, (170.0, 160.0)),
            BookClick::Prev,
            "the previous arrow shows past page one"
        );
        assert_eq!(screen.click_at(&scaled, (10.0, 10.0)), BookClick::None);
    }

    #[test]
    fn the_hovered_arrow_draws_the_hot_column() {
        // `u = 23` on-hover else `0` (`NextPageButton.drawButton`); the
        // previous arrow keeps its `205` row either way.
        let font = reader_font();
        let scaled = scaled_448();
        let mut screen = BookScreen::new(book_stack(
            WRITTEN_BOOK_ID,
            Some(signed_tail(&["one", "two", "three"])),
        ));
        screen.next_page();
        let idle = screen.draws(&font, &scaled, Some((0.0, 0.0)));
        let hot = screen.draws(&font, &scaled, Some((250.0, 160.0)));
        fn arrow_uv(draws: &[HudDraw], x: f32) -> [f32; 4] {
            draws
                .iter()
                .find_map(|draw| match draw {
                    HudDraw::TexturedRect {
                        texture: HudTexture::Named("gui/book"),
                        x: dx,
                        uv,
                        width,
                        height,
                        ..
                    } if *dx == x && *width == 23.0 && *height == 13.0 => Some(*uv),
                    _ => None,
                })
                .expect("the arrow draws")
        }
        assert_eq!(
            arrow_uv(&idle, 248.0)[0],
            0.0,
            "the next arrow idles at u 0"
        );
        assert_eq!(
            arrow_uv(&hot, 248.0)[0],
            23.0 / 256.0,
            "the hovered next arrow heats to u 23"
        );
        assert_eq!(
            arrow_uv(&hot, 248.0)[1],
            192.0 / 256.0,
            "the next row stays 192"
        );
        assert_eq!(
            arrow_uv(&hot, 166.0)[1],
            205.0 / 256.0,
            "the previous row stays 205"
        );
    }

    #[test]
    fn the_draws_carry_no_author_or_title_line() {
        // The reading branch draws the indicator plus the page text only.
        let font = reader_font();
        let scaled = scaled_448();
        let screen = BookScreen::new(book_stack(
            WRITTEN_BOOK_ID,
            Some(signed_tail(&["hello world"])),
        ));
        let draws = screen.draws(&font, &scaled, Some((0.0, 0.0)));
        let texts: Vec<&str> = draws
            .iter()
            .filter_map(|draw| match draw {
                HudDraw::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            texts.iter().any(|text| text.contains("Page 1 of 1")),
            "the indicator draws: {texts:?}"
        );
        assert!(
            texts.iter().any(|text| text.contains("hello world")),
            "the page text draws: {texts:?}"
        );
        assert!(
            !texts
                .iter()
                .any(|text| text.contains("Ada") || text.contains("Tales")),
            "no author/title line draws: {texts:?}"
        );
    }
}
