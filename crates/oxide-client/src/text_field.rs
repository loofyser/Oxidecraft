//! The shared text-field engine: the one machine behind the chat field,
//! the anvil's name field and the creative search field.
//!
//! The shape is the source's `GuiTextField` (`GuiTextField.java`): the text,
//! the cursor, the selection anchor, the scroll offset, a per-consumer length
//! cap, a character filter and a responder hook. The book corner holds no
//! field — its title and pages are plain strings the source edits by hand
//! (`GuiScreenBook.java`:338-368) — and the sign editor stays out the same
//! way (`GuiEditSign.java`:92-119 edits lines under a width cap, never
//! through `GuiTextField`); both are recorded out, not absorbed.
//!
//! The responder hook is the mutation answers: every editing entry that
//! changes the text answers `true` and the caller fires its own responder
//! send (the anvil's `MC|ItemName` re-fire, the creative rebuild, the chat's
//! none — `GuiTextField.writeText`:164-167 notifies, `setText`:86-101 never
//! does). [`TextField::set_text`] therefore answers nothing: the slot-sync
//! paths depend on the silence.

use std::process::{Command, Stdio};

use oxide_game::input::Key;
use winit::keyboard::KeyCode;

/// Whether the field accepts a character: `ChatAllowedCharacters`
/// refuses the format code, everything below the space and DEL
/// (`ChatAllowedCharacters.java`:10-13), and `writeText`:132 filters every
/// append through it.
pub fn field_allowed(c: char) -> bool {
    c != '§' && c >= ' ' && c != '\u{7f}'
}

/// The frame's border grey: `-6250336` (`drawTextBox`:527-530).
pub const FRAME_BORDER: u32 = 0xFFA0A0A0;
/// The frame's inner black: `-16777216` (`drawTextBox`:531-534).
pub const FRAME_INNER: u32 = 0xFF000000;
/// The enabled text grey: `14737632` (`GuiTextField.java`:52).
pub const TEXT_ENABLED: u32 = 14737632;
/// The disabled text grey: `7368816` (`GuiTextField.java`:53).
pub const TEXT_DISABLED: u32 = 7368816;
/// The blink phase divisor: the caret draws while `counter / 6 % 2 == 0`
/// (`GuiTextField.java`:540).
pub const BLINK_DIV: u64 = 6;

/// What one field key did: the character path's next step, the field's own,
/// or a key the field never touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldOutcome {
    /// The key is the character path's: the caller runs
    /// [`TextField::write_text`] with the key's text.
    Character,
    /// The key was the field's own and is done.
    Consumed,
    /// The key is not the field's: the field holds no focus, so nothing ran.
    Ignored,
}

/// The caret the draw reads: the bar straddling the pen, the underscore at
/// it, or nothing while the blink is down (`drawTextBox`:556-583).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caret {
    /// The cursor sits inside the text — or the field is full, the source's
    /// own extra condition — so the caret is the bar.
    Bar,
    /// The cursor sits past the text's end: the underscore at the pen.
    Underscore,
    /// The blink is down: no caret this frame.
    Hidden,
}

/// One clipboard chord the window edge reads from the physical key while
/// Ctrl is held (`textboxKeyTyped`:343-373 reads the same chords through the
/// keyboard state).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardOp {
    /// Ctrl+A: select the whole text.
    SelectAll,
    /// Ctrl+C: copy the selection through the clipboard.
    Copy,
    /// Ctrl+X: copy the selection, then delete it.
    Cut,
    /// Ctrl+V: type the clipboard's text through the character path.
    Paste,
}

/// What one clipboard chord did: whether the text changed (the responder
/// fires exactly then) and the text a copy or cut carried, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardEdit {
    /// Whether the text changed: Cut and landing Pastes re-fire, SelectAll
    /// and Copy never do.
    pub changed: bool,
    /// The selected text a Copy or Cut carried; `None` when nothing was
    /// selected, so nothing ran.
    pub copied: Option<String>,
}

/// The clipboard chord the physical key carries while Ctrl is held, or
/// `None` when the key is no chord. The window edge runs
/// [`TextField::apply_clipboard`] with the answer.
pub fn clipboard_op(ctrl: bool, code: KeyCode) -> Option<ClipboardOp> {
    if !ctrl {
        return None;
    }
    match code {
        KeyCode::KeyA => Some(ClipboardOp::SelectAll),
        KeyCode::KeyC => Some(ClipboardOp::Copy),
        KeyCode::KeyX => Some(ClipboardOp::Cut),
        KeyCode::KeyV => Some(ClipboardOp::Paste),
        _ => None,
    }
}

/// Copies text to the clipboard through `wl-copy` (both present; the house
/// external-command pattern). A spawn or write failure is a recorded
/// no-op, never a panic. Answers whether the bytes left.
pub fn clipboard_store(text: &str) -> bool {
    // The daemon outlives the copy: its stdout/stderr are nulled so a
    // forked holder never inherits the caller's pipes.
    let mut child = match Command::new("wl-copy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    let wrote = match child.stdin.as_mut() {
        Some(stdin) => std::io::Write::write_all(stdin, text.as_bytes()).is_ok(),
        None => false,
    };
    drop(child.stdin.take());
    if !wrote {
        return false;
    }
    match child.wait() {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

/// Reads the clipboard through `wl-paste`. A missing daemon, a failed read
/// or a non-UTF-8 payload is a recorded no-op (`None`), never a panic.
pub fn clipboard_load() -> Option<String> {
    let output = match Command::new("wl-paste").stderr(Stdio::null()).output() {
        Ok(output) => output,
        Err(_) => return None,
    };
    if !output.status.success() {
        return None;
    }
    match String::from_utf8(output.stdout) {
        Ok(text) if !text.is_empty() => Some(text),
        _ => None,
    }
}

/// The shared field: text, cursor, selection anchor, scroll offset, cap,
/// filter, focus and blink — `GuiTextField.java`'s own state.
#[derive(Debug, Clone)]
pub struct TextField {
    /// The text, edited in place.
    text: String,
    /// The cursor's byte index into `text`, always on a character boundary
    /// (the source's cursor is a character index — `setCursorPosition`:310-316
    /// clamps it — and the byte index is the port's form of the same position).
    cursor: usize,
    /// The selection's anchor byte index (`selectionEnd`): equal to the
    /// cursor while nothing is selected.
    select_end: usize,
    /// The drawn window's left edge in characters (`lineScrollOffset`): the
    /// draw trims this many characters and the scroll maintenance keeps the
    /// cursor inside the visible run.
    scroll: usize,
    /// The length cap in characters (`maxStringLength`, default 32 in the
    /// source; every consumer here sets its own).
    max_len: usize,
    /// The character filter `writeText` runs (`ChatAllowedCharacters` for
    /// all three consumers here).
    validator: fn(char) -> bool,
    /// Whether the field holds focus (`setFocused` from `mouseClicked`).
    focused: bool,
    /// Whether a click outside drops focus (`canLoseFocus`; the chat field
    /// never loses it, the screens' fields do).
    can_lose_focus: bool,
    /// Whether the field answers keys and draws lit (`isEnabled`).
    enabled: bool,
    /// The blink counter: steps once per session tick, and the caret draws
    /// while `blink / 6 % 2 == 0` (`updateCursorCounter`:78-81, `:540`).
    blink: u64,
}

impl TextField {
    /// A field capped at `max_len` characters, unfocused, enabled, allowed
    /// through [`field_allowed`].
    pub fn new(max_len: usize) -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            select_end: 0,
            scroll: 0,
            max_len,
            validator: field_allowed,
            focused: false,
            can_lose_focus: true,
            enabled: true,
            blink: 0,
        }
    }

    /// The field's text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The cursor's byte index into the text.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The selection anchor's byte index.
    pub fn selection_anchor(&self) -> usize {
        self.select_end
    }

    /// Whether any selection stands.
    pub fn has_selection(&self) -> bool {
        self.cursor != self.select_end
    }

    /// The selection's byte range, cursor side sorted out.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        if self.cursor == self.select_end {
            None
        } else {
            Some(self.sorted_ends())
        }
    }

    /// The selected text, or `None` while nothing is selected.
    pub fn selected_text(&self) -> Option<String> {
        self.selection_range()
            .map(|(lo, hi)| self.text[lo..hi].to_owned())
    }

    /// The length cap in characters.
    pub fn max_len(&self) -> usize {
        self.max_len
    }

    /// Whether the field holds focus.
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Whether the field draws lit and answers keys.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// The text grey the draw reads: enabled or disabled (`:52-53`).
    pub fn text_grey(&self) -> u32 {
        if self.enabled {
            TEXT_ENABLED
        } else {
            TEXT_DISABLED
        }
    }

    /// Parks focus and the lose-focus rule together.
    pub fn set_focus(&mut self, focused: bool, can_lose_focus: bool) {
        self.focused = focused;
        self.can_lose_focus = can_lose_focus;
    }

    /// Lights or dims the field.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Replaces the validator `write_text` filters through.
    pub fn set_validator(&mut self, validator: fn(char) -> bool) {
        self.validator = validator;
    }

    /// Replaces the text with the cursor at its end (`setText`:86-101):
    /// the cap truncates, the selection collapses — and nothing notifies.
    /// The slot-sync paths depend on the silence, so this answers nothing.
    pub fn set_text(&mut self, text: &str) {
        self.text = text.chars().take(self.max_len).collect();
        self.cursor = self.text.len();
        self.select_end = self.cursor;
    }

    /// Clears the text through [`TextField::set_text`]: silent too.
    pub fn clear(&mut self) {
        self.set_text("");
    }

    /// One session tick of the blink counter.
    pub fn tick(&mut self) {
        self.blink = self.blink.wrapping_add(1);
    }

    /// Restarts the blink count, as taking focus does (`setFocused`:698-705).
    pub fn reset_blink(&mut self) {
        self.blink = 0;
    }

    /// Whether the caret is in its lit phase (`GuiTextField.java`:540).
    pub fn cursor_visible(&self) -> bool {
        (self.blink / BLINK_DIV) % 2 == 0
    }

    /// The caret the draw reads for the blink and the cursor's seat.
    pub fn caret(&self) -> Caret {
        if !self.cursor_visible() {
            Caret::Hidden
        } else if self.cursor < self.text.len() || self.char_count() >= self.max_len {
            Caret::Bar
        } else {
            Caret::Underscore
        }
    }

    /// Appends text at the cursor — the character path `writeText`
    /// (`:129-169`): the filter drops refused characters, a standing
    /// selection is replaced, the append stops at the cap, the cursor lands
    /// past the insert and the selection collapses. Answers whether the
    /// text changed: the caller fires its responder send exactly then (a
    /// fully refused append past the filter answers false, the source's
    /// own false past it).
    pub fn write_text(&mut self, text: &str) -> bool {
        let validator = self.validator;
        let kept: String = text.chars().filter(|c| validator(*c)).collect();
        let (lo, hi) = self.sorted_ends();
        let selected = self.text[lo..hi].chars().count();
        let room = self
            .max_len
            .saturating_sub(self.char_count().saturating_sub(selected));
        let insert: String = kept.chars().take(room).collect();
        let mut next = String::with_capacity(lo + insert.len() + self.text.len() - hi);
        next.push_str(&self.text[..lo]);
        next.push_str(&insert);
        next.push_str(&self.text[hi..]);
        let changed = next != self.text;
        self.text = next;
        self.cursor = lo + insert.len();
        self.select_end = self.cursor;
        changed
    }

    /// Deletes `amount` characters from the cursor — `deleteFromCursor`
    /// (`:193-234`): a standing selection collapses through `writeText("")`
    /// either way, a negative amount eats backwards, a positive one
    /// forwards. Answers whether the text changed.
    pub fn delete_from_cursor(&mut self, amount: i32) -> bool {
        if self.cursor != self.select_end {
            return self.write_text("");
        }
        if amount == 0 || self.text.is_empty() {
            return false;
        }
        let count = self.char_count();
        let current = self.char_of_byte(self.cursor);
        let (lo, hi) = if amount < 0 {
            (
                current.saturating_sub(amount.unsigned_abs() as usize),
                current,
            )
        } else {
            (current, count.min(current + amount as usize))
        };
        if lo == hi {
            return false;
        }
        self.text
            .drain(self.byte_of_char(lo)..self.byte_of_char(hi));
        self.cursor = self.byte_of_char(lo);
        self.select_end = self.cursor;
        true
    }

    /// Deletes the byte range `[start, end)`: the word-cycle's own splice
    /// (the source deletes back to the word start at `:229` before writing
    /// the candidate). Collapses the selection and answers whether the
    /// text changed.
    pub fn delete_range(&mut self, start: usize, end: usize) -> bool {
        let len = self.text.len();
        let (mut lo, mut hi) = if start <= end {
            (start.min(len), end.min(len))
        } else {
            (end.min(len), start.min(len))
        };
        while lo > 0 && !self.text.is_char_boundary(lo) {
            lo -= 1;
        }
        while hi > 0 && !self.text.is_char_boundary(hi) {
            hi -= 1;
        }
        if lo == hi {
            return false;
        }
        self.text.drain(lo..hi);
        self.cursor = lo;
        self.select_end = lo;
        true
    }

    /// The word jump `func_146197_a` (`:257-297`) from the byte index `from`:
    /// `steps` words back (negative) or on (positive), splitting on the
    /// space alone, skipping past spaces only when `skip_spaces` asks —
    /// the arrows pass true with Ctrl, the completion's word split passes
    /// false going back once. Answers the byte index, clamped to the text.
    pub fn word_jump(&self, from: usize, steps: i32, skip_spaces: bool) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let count = chars.len();
        let mut index = self.char_of_byte(from).min(count);
        if steps < 0 {
            for _ in 0..steps.unsigned_abs() {
                if skip_spaces {
                    while index > 0 && chars[index - 1] == ' ' {
                        index -= 1;
                    }
                }
                while index > 0 && chars[index - 1] != ' ' {
                    index -= 1;
                }
            }
        } else {
            for _ in 0..steps as usize {
                while index < count && chars[index] != ' ' {
                    index += 1;
                }
                if skip_spaces {
                    while index < count && chars[index] == ' ' {
                        index += 1;
                    }
                }
            }
        }
        self.byte_of_char(index)
    }

    /// Parks the cursor at the byte index, collapsing the selection unless
    /// `select` extends it (the arrows' Shift form).
    pub fn place_cursor(&mut self, at: usize, select: bool) {
        let mut at = at.min(self.text.len());
        while at > 0 && !self.text.is_char_boundary(at) {
            at -= 1;
        }
        self.cursor = at;
        if !select {
            self.select_end = at;
        }
    }

    /// One editing key, as `textboxKeyTyped`:337-494 reads it: Backspace and
    /// Delete with their Ctrl-word forms (`:378-391`, `:463-476`); Home and
    /// End (`:393-404`); the arrows with their Shift/Ctrl forms (`:405-449`).
    /// Every other key is the character path's. An unfocused field answers
    /// [`FieldOutcome::Ignored`] and touches nothing.
    pub fn key(&mut self, key: Key, ctrl: bool, shift: bool) -> FieldOutcome {
        if !self.focused {
            return FieldOutcome::Ignored;
        }
        match key {
            Key::Backspace => {
                if self.has_selection() {
                    self.write_text("");
                } else if ctrl {
                    let from = self.word_jump(self.cursor, -1, true);
                    self.delete_range(from, self.cursor);
                } else {
                    self.delete_from_cursor(-1);
                }
                FieldOutcome::Consumed
            }
            Key::Delete => {
                if self.has_selection() {
                    self.write_text("");
                } else if ctrl {
                    let to = self.word_jump(self.cursor, 1, true);
                    self.delete_range(self.cursor, to);
                } else {
                    self.delete_from_cursor(1);
                }
                FieldOutcome::Consumed
            }
            Key::Home => {
                self.place_cursor(0, shift);
                FieldOutcome::Consumed
            }
            Key::End => {
                let end = self.text.len();
                self.place_cursor(end, shift);
                FieldOutcome::Consumed
            }
            Key::ArrowLeft => {
                let to = if ctrl {
                    self.word_jump(self.cursor, -1, true)
                } else {
                    match self.text[..self.cursor].chars().next_back() {
                        Some(prev) => self.cursor - prev.len_utf8(),
                        None => self.cursor,
                    }
                };
                self.place_cursor(to, shift);
                FieldOutcome::Consumed
            }
            Key::ArrowRight => {
                let to = if ctrl {
                    self.word_jump(self.cursor, 1, true)
                } else {
                    match self.text[self.cursor..].chars().next() {
                        Some(next) => self.cursor + next.len_utf8(),
                        None => self.cursor,
                    }
                };
                self.place_cursor(to, shift);
                FieldOutcome::Consumed
            }
            _ => FieldOutcome::Character,
        }
    }

    /// One click: focus follows `inside` exactly while the field can lose
    /// it (`mouseClicked`:499-520 over `canLoseFocus`), and a focused click
    /// parks the cursor at the caller-measured index — the width walk
    /// (minus 4 with a background) stays with the caller, which owns the font.
    pub fn click(&mut self, inside: bool, cursor: Option<usize>) {
        if self.can_lose_focus {
            self.focused = inside;
        }
        if self.focused {
            if let Some(at) = cursor {
                self.place_cursor(at, false);
            }
        }
    }

    /// Selects the whole text: the cursor to the end, the anchor to the
    /// start (`textboxKeyTyped`:343-345).
    pub fn select_all(&mut self) {
        self.cursor = self.text.len();
        self.select_end = 0;
    }

    /// Copies the selection through the clipboard (`:346-355`): best-effort,
    /// and answers the selected text so the caller need not re-read it.
    pub fn copy_selection(&self) -> Option<String> {
        let selected = self.selected_text()?;
        clipboard_store(&selected);
        Some(selected)
    }

    /// Cuts the selection: copies, then deletes it (`:356-364`). Answers
    /// the carried text; the text changed exactly then.
    pub fn cut_selection(&mut self) -> Option<String> {
        let selected = self.selected_text()?;
        clipboard_store(&selected);
        self.write_text("");
        Some(selected)
    }

    /// Pastes the clipboard through the character path (`:365-373`).
    /// Answers whether the text changed.
    pub fn paste_clipboard(&mut self) -> bool {
        match clipboard_load() {
            Some(text) => self.write_text(&text),
            None => false,
        }
    }

    /// Runs one window-edge clipboard chord and answers what it did.
    pub fn apply_clipboard(&mut self, op: ClipboardOp) -> ClipboardEdit {
        match op {
            ClipboardOp::SelectAll => {
                self.select_all();
                ClipboardEdit {
                    changed: false,
                    copied: None,
                }
            }
            ClipboardOp::Copy => ClipboardEdit {
                changed: false,
                copied: self.copy_selection(),
            },
            ClipboardOp::Cut => {
                let copied = self.cut_selection();
                ClipboardEdit {
                    changed: copied.is_some(),
                    copied,
                }
            }
            ClipboardOp::Paste => ClipboardEdit {
                changed: self.paste_clipboard(),
                copied: None,
            },
        }
    }

    /// Keeps the cursor inside the drawn run (`drawTextBox`'s own scroll
    /// maintenance): `width_of` measures a run, `visible` is the run's
    /// budget. The draw then reads [`TextField::visible_text`].
    pub fn scroll_to_visible(&mut self, width_of: impl Fn(&str) -> i32, visible: i32) {
        if self.scroll > self.char_count() {
            self.scroll = self.char_count();
        }
        loop {
            let cursor_char = self.char_of_byte(self.cursor);
            if self.scroll > cursor_char {
                self.scroll -= 1;
                continue;
            }
            if self.scroll == cursor_char {
                break;
            }
            let start = self.byte_of_char(self.scroll);
            if width_of(&self.text[start..self.cursor]) > visible {
                self.scroll += 1;
            } else {
                break;
            }
        }
    }

    /// The drawn run: the text past the scroll offset.
    pub fn visible_text(&self) -> &str {
        &self.text[self.byte_of_char(self.scroll.min(self.char_count()))..]
    }

    /// The cursor's index inside [`TextField::visible_text`].
    pub fn relative_cursor(&self) -> usize {
        let start = self.byte_of_char(self.scroll.min(self.char_count()));
        self.cursor.saturating_sub(start)
    }

    /// The standing selection inside [`TextField::visible_text`]: the sorted
    /// ends shifted past the window's start and clamped to the drawn run —
    /// the frame draws the source's XOR rect (`drawTextBox`:573-575) over
    /// this run. `None` while no selection stands.
    pub fn relative_selection(&self) -> Option<(usize, usize)> {
        let (lo, hi) = self.selection_range()?;
        let start = self.byte_of_char(self.scroll.min(self.char_count()));
        let end = start + self.visible_text().len();
        let lo = lo.max(start).min(end) - start;
        let hi = hi.max(start).min(end) - start;
        if lo < hi { Some((lo, hi)) } else { None }
    }

    /// The text's length in characters: the cap's own unit.
    fn char_count(&self) -> usize {
        self.text.chars().count()
    }

    /// The byte index the character index opens.
    fn byte_of_char(&self, index: usize) -> usize {
        if index == 0 {
            return 0;
        }
        self.text
            .char_indices()
            .nth(index)
            .map(|(byte, _)| byte)
            .unwrap_or(self.text.len())
    }

    /// The character index the byte index sits at.
    fn char_of_byte(&self, byte: usize) -> usize {
        self.text[..byte.min(self.text.len())].chars().count()
    }

    /// The cursor and anchor sorted: the splice's own ends.
    fn sorted_ends(&self) -> (usize, usize) {
        if self.cursor <= self.select_end {
            (self.cursor, self.select_end)
        } else {
            (self.select_end, self.cursor)
        }
    }
}

/// The completion word's start: the byte past the last space before the
/// cursor — `func_146197_a(-1, cursor, false)`'s shape (`:216-221` takes the
/// word this way before the request). `before_cursor` is the field's text
/// up to the cursor.
pub fn completion_word_start(before_cursor: &str) -> usize {
    match before_cursor.rfind(' ') {
        Some(index) => index + 1,
        None => 0,
    }
}

/// The candidates for the word: the names it opens, matched
/// case-insensitively (`CommandBase.doesStringStartWith`:670-673, the
/// ignore-case region match the server answers with) in the list's own
/// order — the cycle preserves it.
pub fn completion_matches<'a>(word: &str, names: &[&'a str]) -> Vec<&'a str> {
    let folded = word.to_lowercase();
    names
        .iter()
        .copied()
        .filter(|name| name.to_lowercase().starts_with(&folded))
        .collect()
}

/// The shared head of the candidates: `StringUtils.getCommonPrefix`'s own
/// (`onAutocompleteResponse`:315-344 replaces the word with it when it is
/// non-empty and differs from the word).
pub fn common_prefix(candidates: &[&str]) -> String {
    let mut prefix = String::new();
    if candidates.is_empty() {
        return prefix;
    }
    for (index, c) in candidates[0].chars().enumerate() {
        if candidates[1..]
            .iter()
            .all(|name| name.chars().nth(index) == Some(c))
        {
            prefix.push(c);
        } else {
            break;
        }
    }
    prefix
}

/// The multi-candidate print: the `", "`-joined list, one chat line
/// (`autocompletePlayerNames`:232-247).
pub fn join_candidates(candidates: &[&str]) -> String {
    candidates.join(", ")
}

/// The match text of a command opening: a `/`-leading input strips the one
/// slash before the word split (`NetHandlerPlayServer.processTabComplete`
/// :1215-1227 hands the slash-stripped input to the ladder). With no client
/// command table the names below are the only candidates — recorded.
pub fn strip_command_slash(text: &str) -> &str {
    text.strip_prefix('/').unwrap_or(text)
}

/// Whether two words read the same ignoring case: the prefix-against-word
/// differ check (`:330-337` replaces only when they differ).
pub fn words_equal_fold(left: &str, right: &str) -> bool {
    left.to_lowercase() == right.to_lowercase()
}

#[cfg(test)]
mod field_tests {
    //! The field laws: the write/delete/setText rules, the key subset with
    //! its word forms, the clipboard chords and the completion helpers.

    use super::*;

    /// A field capped at ten, focused, for the law pins.
    fn open_field() -> TextField {
        let mut field = TextField::new(10);
        field.set_focus(true, true);
        field
    }

    #[test]
    fn write_text_filters_caps_and_notifies() {
        // `writeText` (`:129-169`): the filter drops refused characters, the
        // append stops at the cap, the cursor lands past the insert, and a
        // changed text answers true for the responder.
        let mut field = open_field();
        assert!(field.write_text("ab\u{a7}c\u{1}d"));
        assert_eq!((field.text(), field.cursor()), ("abcd", 4));
        assert!(field.write_text("efghij"));
        assert_eq!(field.text(), "abcdefghij");
        assert!(!field.write_text("kl"), "a full field refuses");
        assert_eq!(field.cursor(), 10, "and the cursor stays put");
        assert!(!field.write_text(""), "an empty append changes nothing");
    }

    #[test]
    fn a_standing_selection_is_replaced_and_deletes_collapse_it() {
        // `writeText` replaces the selection (`:129-169` splices between the
        // sorted anchor and cursor); `deleteFromCursor` collapses one
        // through `writeText("")` (`:193-234`).
        let mut field = open_field();
        field.write_text("abcd");
        field.place_cursor(1, false);
        field.place_cursor(3, true);
        assert_eq!(field.selected_text().as_deref(), Some("bc"));
        assert!(field.write_text("X"));
        assert_eq!((field.text(), field.cursor()), ("aXd", 2));
        assert!(!field.has_selection(), "the insert collapses the anchor");
        field.place_cursor(0, false);
        field.place_cursor(2, true);
        assert!(
            field.delete_from_cursor(-1),
            "the delete eats the selection"
        );
        assert_eq!(field.text(), "d");
    }

    #[test]
    fn set_text_truncates_parks_and_stays_silent() {
        // `setText`:86-101 validates, truncates to the cap, parks the cursor
        // at the end — and never notifies, which the slot-sync paths depend
        // on. The silence is the unit return: no responder answer leaves.
        let mut field = open_field();
        field.write_text("abc");
        let answered: () = field.set_text("abcdefghijklm");
        assert_eq!(answered, ());
        assert_eq!((field.text(), field.cursor()), ("abcdefghij", 10));
        assert!(!field.has_selection());
    }

    #[test]
    fn word_jumps_split_on_the_space_alone() {
        // `func_146197_a`:257-297 splits on the space (32) with the
        // skip-spaces flag.
        let mut field = open_field();
        field.write_text("ab cd  ef");
        assert_eq!(field.word_jump(9, -1, false), 7, "back one word, no skip");
        assert_eq!(field.word_jump(9, -1, true), 7);
        assert_eq!(field.word_jump(5, -1, true), 3, "past the spaces");
        assert_eq!(field.word_jump(0, 1, false), 2, "on lands on the space");
        assert_eq!(field.word_jump(0, 1, true), 3, "skip lands past it");
        assert_eq!(field.word_jump(9, -2, true), 3);
    }

    #[test]
    fn the_key_subset_edits_and_the_shift_forms_extend() {
        // `textboxKeyTyped`:337-494 — Backspace/Delete with Ctrl-word forms,
        // Home/End, the arrows with Shift/Ctrl forms.
        let mut field = open_field();
        field.write_text("ab cd");
        assert_eq!(field.key(Key::Home, false, false), FieldOutcome::Consumed);
        assert_eq!(field.cursor(), 0);
        assert_eq!(field.key(Key::End, false, false), FieldOutcome::Consumed);
        assert_eq!(field.cursor(), 5);
        assert_eq!(
            field.key(Key::ArrowLeft, false, true),
            FieldOutcome::Consumed
        );
        assert_eq!(field.selected_text().as_deref(), Some("d"));
        assert_eq!(
            field.key(Key::ArrowLeft, true, false),
            FieldOutcome::Consumed,
            "Ctrl+Left jumps the word"
        );
        assert_eq!(field.cursor(), 3);
        assert_eq!(
            field.key(Key::Backspace, false, false),
            FieldOutcome::Consumed
        );
        assert_eq!(field.text(), "abcd", "the space before the cursor goes");
        let mut field = open_field();
        field.write_text("ab cd");
        field.key(Key::End, false, false);
        assert_eq!(
            field.key(Key::Backspace, true, false),
            FieldOutcome::Consumed
        );
        assert_eq!((field.text(), field.cursor()), ("ab ", 3));
        let mut field = open_field();
        field.write_text("ab cd");
        field.key(Key::Home, false, false);
        assert_eq!(field.key(Key::Delete, true, false), FieldOutcome::Consumed);
        assert_eq!(
            (field.text(), field.cursor()),
            ("cd", 0),
            "Ctrl+Delete eats the word and its trailing space"
        );
    }

    #[test]
    fn an_unfocused_field_ignores_every_key() {
        // `textboxKeyTyped` answers false unfocused: the field is not its.
        let mut field = TextField::new(10);
        assert!(!field.is_focused());
        assert_eq!(
            field.key(Key::Backspace, false, false),
            FieldOutcome::Ignored
        );
        assert_eq!(field.key(Key::T, false, false), FieldOutcome::Ignored);
    }

    #[test]
    fn select_all_copy_cut_and_paste() {
        // `textboxKeyTyped`:343-373 — Ctrl+A selects, C copies, X cuts, V
        // pastes through the character path. The copy answer carries the
        // selection whatever the daemon does; failures stay silent.
        let mut field = open_field();
        field.write_text("hello");
        assert_eq!(field.copy_selection(), None, "no selection, no copy");
        field.select_all();
        assert_eq!(field.selected_text().as_deref(), Some("hello"));
        assert_eq!(field.copy_selection(), Some("hello".to_owned()));
        assert_eq!(field.text(), "hello", "copy keeps the text");
        assert_eq!(field.cut_selection(), Some("hello".to_owned()));
        assert_eq!((field.text(), field.cursor()), ("", 0));
        let landed = field.paste_clipboard();
        let _ = landed;
    }

    #[test]
    fn the_window_edge_reads_the_clipboard_chords() {
        // The Ctrl+A/C/X/V chords the window edge maps from the physical
        // key; anything else — or no Ctrl — is no chord.
        assert_eq!(
            clipboard_op(true, KeyCode::KeyA),
            Some(ClipboardOp::SelectAll)
        );
        assert_eq!(clipboard_op(true, KeyCode::KeyC), Some(ClipboardOp::Copy));
        assert_eq!(clipboard_op(true, KeyCode::KeyX), Some(ClipboardOp::Cut));
        assert_eq!(clipboard_op(true, KeyCode::KeyV), Some(ClipboardOp::Paste));
        assert_eq!(clipboard_op(false, KeyCode::KeyA), None);
        assert_eq!(clipboard_op(true, KeyCode::KeyB), None);
        let mut field = open_field();
        field.write_text("hello");
        let edit = field.apply_clipboard(ClipboardOp::SelectAll);
        assert_eq!(
            edit,
            ClipboardEdit {
                changed: false,
                copied: None,
            }
        );
        assert_eq!(field.selected_text().as_deref(), Some("hello"));
    }

    #[test]
    fn clicks_focus_and_park() {
        // `mouseClicked`:499-520 — a field that can lose focus follows the
        // click; one that cannot keeps it; a focused click parks the
        // caller-measured cursor.
        let mut field = open_field();
        field.write_text("hello");
        field.click(false, None);
        assert!(!field.is_focused());
        field.click(true, Some(2));
        assert_eq!((field.is_focused(), field.cursor()), (true, 2));
        field.set_focus(true, false);
        field.click(false, None);
        assert!(field.is_focused(), "the chat field never loses it");
    }

    #[test]
    fn the_blink_runs_on_the_sources_six_tick_phases() {
        // `GuiTextField.java`:540 — the caret draws while
        // `counter / 6 % 2 == 0`, restarted by focus.
        let mut field = open_field();
        assert!(field.cursor_visible());
        for _ in 0..5 {
            field.tick();
        }
        assert!(field.cursor_visible());
        field.tick();
        assert!(!field.cursor_visible());
        field.reset_blink();
        assert!(field.cursor_visible());
    }

    #[test]
    fn the_caret_bar_rides_the_text_and_the_full_field() {
        // The port's own bar rule (`input_draws` over `drawTextBox`:556-583):
        // inside the text — or past a full field — the caret is the bar, at
        // the text's end the underscore, and nothing while the blink is down.
        let mut field = open_field();
        field.write_text("abc");
        field.key(Key::Home, false, false);
        assert_eq!(field.caret(), Caret::Bar);
        field.key(Key::End, false, false);
        assert_eq!(field.caret(), Caret::Underscore);
        field.write_text("defghij");
        assert_eq!(field.caret(), Caret::Bar, "full past the cap reads bar");
        for _ in 0..6 {
            field.tick();
        }
        assert_eq!(field.caret(), Caret::Hidden);
    }

    #[test]
    fn the_scroll_keeps_the_cursor_in_the_drawn_run() {
        // `drawTextBox`'s scroll maintenance with its width trim: the drawn
        // run opens at the offset and carries the cursor.
        let mut field = TextField::new(100);
        field.set_focus(true, true);
        field.write_text(&"a".repeat(20));
        let width_of = |run: &str| run.chars().count() as i32;
        field.scroll_to_visible(width_of, 10);
        assert_eq!(field.visible_text(), "aaaaaaaaaa");
        assert_eq!(field.relative_cursor(), 10);
        field.key(Key::Home, false, false);
        field.scroll_to_visible(width_of, 10);
        assert_eq!(field.visible_text(), "aaaaaaaaaaaaaaaaaaaa");
    }

    #[test]
    fn the_completion_word_is_the_last_space_split() {
        // `func_146197_a(-1, cursor, false)`'s shape: past the last space.
        assert_eq!(completion_word_start(""), 0);
        assert_eq!(completion_word_start("ab"), 0);
        assert_eq!(completion_word_start("say ab"), 4);
        assert_eq!(
            completion_word_start("say "),
            4,
            "a trailing space opens empty"
        );
        assert_eq!(completion_word_start("/ab"), 0);
    }

    #[test]
    fn candidates_match_case_insensitively_in_list_order() {
        // `doesStringStartWith` (`CommandBase.java`:670-673): the
        // ignore-case region match, cycling in the list's own order.
        let names = ["Alice", "bob", "Alfred", "carol"];
        let refs: Vec<&str> = names.to_vec();
        assert_eq!(completion_matches("al", &refs), ["Alice", "Alfred"]);
        assert_eq!(completion_matches("AL", &refs), ["Alice", "Alfred"]);
        assert_eq!(
            completion_matches("", &refs),
            ["Alice", "bob", "Alfred", "carol"]
        );
        assert_eq!(completion_matches("z", &refs), Vec::<&str>::new());
    }

    #[test]
    fn the_common_prefix_and_the_joined_print() {
        // `StringUtils.getCommonPrefix` feeds the replace (`:330-337`); the
        // multi-candidate print joins with `", "` (`:232-247`).
        assert_eq!(common_prefix(&["Alice", "Alfred"]), "Al");
        assert_eq!(common_prefix(&["bob"]), "bob");
        assert_eq!(common_prefix(&["ab", "cd"]), "");
        assert_eq!(join_candidates(&["Alice", "Alfred"]), "Alice, Alfred");
    }

    #[test]
    fn slash_inputs_match_past_the_one_slash() {
        // `processTabComplete`:1215-1227 strips the slash before the ladder;
        // with no client command table the names are the only candidates.
        assert_eq!(strip_command_slash("/ab"), "ab");
        assert_eq!(strip_command_slash("ab"), "ab");
        assert_eq!(strip_command_slash("/"), "");
        assert!(words_equal_fold("Al", "aL"));
        assert!(!words_equal_fold("Al", "Bo"));
    }
}
