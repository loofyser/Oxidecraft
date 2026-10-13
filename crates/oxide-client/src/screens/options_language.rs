//! The language screen: the locale list, the Force Unicode toggle and
//! Done (`GuiLanguage.java`).

use oxide_assets::font::Font;
use oxide_assets::lang::LanguageInfo;
use oxide_game::chat::LanguageTable;
use oxide_game::settings::GameSettings;
use oxide_render::hud::{HudDraw, ScaledResolution};
use oxide_render::text::string_width;

use super::options::SaveKind;
use super::widgets::{Button, OptionButton};

/// Force Unicode's button id (`initGui` builds it at 100).
pub const LANGUAGE_UNICODE_ID: i32 = 100;
/// Done's button id (the confirm button at 6).
pub const LANGUAGE_DONE_ID: i32 = 6;
/// The title's y (`drawScreen` centres it at 16).
const LANGUAGE_TITLE_Y: f32 = 16.0;
/// The list's top (`GuiSlot` built at 32).
pub const LANGUAGE_LIST_TOP: f32 = 32.0;
/// The list's bottom inset (`height - 65 + 4`).
pub const LANGUAGE_LIST_BOTTOM_INSET: f32 = 61.0;
/// One list row's height (the slot's 18).
pub const LANGUAGE_SLOT_HEIGHT: f32 = 18.0;
/// The grey warning's inset from the bottom (`height - 56`).
const LANGUAGE_WARNING_INSET: f32 = 56.0;

/// The language screen's labels: the title, the warning and Done.
#[derive(Debug, Clone)]
pub struct LanguageLabels {
    /// The centred title (`options.language`).
    pub title: String,
    /// The grey warning (`options.languageWarning`, parenthesised).
    pub warning: String,
    /// Force Unicode's name (`options.forceUnicodeFont`).
    pub unicode: String,
    /// The on string (`options.on`).
    pub on: String,
    /// The off string (`options.off`).
    pub off: String,
    /// Done (`gui.done`).
    pub done: String,
}

/// Resolves the language screen's labels: the title, the warning and the
/// toggle, each falling back to the store's English (`drawScreen`,
/// `initGui`).
pub fn language_labels(language: Option<&LanguageTable>) -> LanguageLabels {
    let text = |key: &str, fallback: &str| {
        language
            .and_then(|table| table.get(key))
            .unwrap_or(fallback)
            .to_string()
    };
    LanguageLabels {
        title: text("options.language", "Language"),
        warning: format!(
            "({})",
            text("options.languageWarning", "Use narrow characters")
        ),
        unicode: text("options.forceUnicodeFont", "Force Unicode Font"),
        on: text("options.on", "On"),
        off: text("options.off", "Off"),
        done: text("gui.done", "Done"),
    }
}

/// One language press: the toggle flipped, a row selected, or Done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageAction {
    /// Force Unicode flipped (`actionPerformed`'s case 100).
    ForceUnicode,
    /// Row `i`'s locale took (`List.elementClicked`'s set + save).
    Selected(usize),
    /// Done answers the hub (case 6).
    Done,
}

impl LanguageAction {
    /// Every arm writes the store at once: the toggle `setOptionValue`s
    /// and the row click saves (`:79-90`, `:133-145`).
    pub fn save_kind(self) -> SaveKind {
        SaveKind::Immediate
    }

    /// Whether the press re-runs the layout: the toggle re-runs
    /// `setWorldAndResolution` (`:79-90`); a row click only relabels.
    /// The scale re-derive the layout runs is Task 28's reader — the
    /// toggle persists now and the carry rides there (recorded).
    pub fn relayout(self) -> bool {
        matches!(self, LanguageAction::ForceUnicode)
    }
}

/// The language screen: the locale list over the loader's languages plus
/// the mechanism — rows read `name (region)` sorted by code (`Language`
/// :28-31, `:43-46`) — with Force Unicode and Done beneath.
#[derive(Debug, Clone)]
pub struct LanguageScreen {
    /// The locale codes in list order.
    locales: Vec<String>,
    /// The row strings the list draws (`name (region)` per locale).
    rows: Vec<String>,
    /// The selected row: the store's language when it stands in the list,
    /// else the head.
    selected: usize,
    /// Force Unicode 100 at `w/2-155, h-38`.
    unicode: OptionButton,
    /// Done 6 at `w/2+5, h-38`.
    done: Button,
    /// The list's scroll offset in GUI pixels.
    scroll: f32,
    /// The labels the toggle recomposes after a write.
    labels: LanguageLabels,
    /// The scaled size the screen lays out at.
    width: u32,
    /// The scaled height the list clips against.
    height: u32,
}

impl LanguageScreen {
    /// Stands the language screen at `scaled` on `settings` with `labels`
    /// over the loader's `locales` (sorted by code): the list spans
    /// `32..h-61` in 18-pixel rows reading `name (region)`, Force Unicode
    /// 100 and Done 6 sit 150-wide at `h-38` (`initGui`:46-52, `List`:116).
    pub fn new(
        scaled: &ScaledResolution,
        settings: &GameSettings,
        labels: LanguageLabels,
        locales: &[LanguageInfo],
    ) -> Self {
        let half = scaled.width / 2;
        let left = (half - 155) as f32;
        let right = (half + 5) as f32;
        let bottom = (scaled.height - 38) as f32;
        let on = if settings.force_unicode {
            labels.on.clone()
        } else {
            labels.off.clone()
        };
        let selected = locales
            .iter()
            .position(|locale| locale.code == settings.language)
            .unwrap_or(0);
        Self {
            locales: locales.iter().map(|locale| locale.code.clone()).collect(),
            rows: locales.iter().map(LanguageInfo::display).collect(),
            selected,
            unicode: OptionButton::new(
                LANGUAGE_UNICODE_ID,
                left,
                bottom,
                150.0,
                20.0,
                labels.unicode.clone(),
                on,
            ),
            done: Button::new(
                LANGUAGE_DONE_ID,
                right,
                bottom,
                150.0,
                20.0,
                labels.done.clone(),
            ),
            scroll: 0.0,
            labels,
            width: scaled.width,
            height: scaled.height,
        }
    }

    /// The locale rows.
    pub fn row_count(&self) -> usize {
        self.locales.len()
    }

    /// Row `i`'s locale code.
    pub fn row(&self, index: usize) -> &str {
        &self.locales[index]
    }

    /// Row `i`'s drawn string (`name (region)`).
    pub fn row_text(&self, index: usize) -> &str {
        &self.rows[index]
    }

    /// The selected row, if the list stands non-empty.
    pub fn selected(&self) -> Option<usize> {
        if self.locales.is_empty() {
            None
        } else {
            Some(self.selected)
        }
    }

    /// Force Unicode's top-left (`w/2-155, h-38`).
    pub fn unicode_pos(&self) -> (f32, f32) {
        (self.unicode.x(), self.unicode.y())
    }

    /// Done's top-left (`w/2+5, h-38`).
    pub fn done_pos(&self) -> (f32, f32) {
        (self.done.x, self.done.y)
    }

    /// One press at the GUI-space point: the toggle flips Force Unicode
    /// and re-lays out, a list row sets the language and saves, Done
    /// answers the hub. Empty space answers nothing.
    pub fn click_at(
        &mut self,
        point: (f32, f32),
        settings: &mut GameSettings,
    ) -> Option<LanguageAction> {
        if self.unicode.press(point) {
            settings.force_unicode = !settings.force_unicode;
            let on = if settings.force_unicode {
                self.labels.on.clone()
            } else {
                self.labels.off.clone()
            };
            self.unicode.set_value(on);
            return Some(LanguageAction::ForceUnicode);
        }
        if self.done.pressed(point) {
            return Some(LanguageAction::Done);
        }
        for (slot, index) in (0..self.locales.len()).enumerate() {
            let y = LANGUAGE_LIST_TOP + slot as f32 * LANGUAGE_SLOT_HEIGHT - self.scroll;
            if y < LANGUAGE_LIST_TOP - LANGUAGE_SLOT_HEIGHT
                || y >= self.height as f32 - LANGUAGE_LIST_BOTTOM_INSET
            {
                continue;
            }
            if point.0 >= 0.0
                && point.0 < self.width as f32
                && point.1 >= y
                && point.1 < y + LANGUAGE_SLOT_HEIGHT
            {
                settings.language = self.locales[index].clone();
                self.selected = index;
                return Some(LanguageAction::Selected(index));
            }
        }
        None
    }

    /// One wheel motion: the locale list scrolls by whole rows, clamped to
    /// its overflow (`GuiSlot`'s scroll).
    pub fn wheel(&mut self, lines: i32) {
        let span = self.height as f32 - LANGUAGE_LIST_BOTTOM_INSET - LANGUAGE_LIST_TOP;
        let overflow = (self.locales.len() as f32 * LANGUAGE_SLOT_HEIGHT - span).max(0.0);
        self.scroll = (self.scroll + lines as f32 * LANGUAGE_SLOT_HEIGHT).clamp(0.0, overflow);
    }

    /// The screen's content draws: the visible locale rows (the selected
    /// row highlighted), the centred title at y 16, the grey warning at
    /// `h-56`, then Force Unicode and Done (`drawScreen`, then
    /// `super.drawScreen`).
    pub fn draws(
        &self,
        font: &Font,
        scaled: &ScaledResolution,
        mouse: Option<(f32, f32)>,
    ) -> Vec<HudDraw> {
        let mut draws = Vec::new();
        let bottom = self.height as f32 - LANGUAGE_LIST_BOTTOM_INSET;
        for (slot, (index, row)) in self.rows.iter().enumerate().enumerate() {
            let y = LANGUAGE_LIST_TOP + slot as f32 * LANGUAGE_SLOT_HEIGHT - self.scroll;
            if y < LANGUAGE_LIST_TOP - LANGUAGE_SLOT_HEIGHT || y >= bottom {
                continue;
            }
            if index == self.selected {
                draws.push(HudDraw::Rect {
                    x: 0.0,
                    y,
                    width: scaled.width as f32,
                    height: LANGUAGE_SLOT_HEIGHT,
                    colour: [1.0, 1.0, 1.0, 80.0 / 255.0],
                });
            }
            let width = string_width(font, row);
            draws.push(HudDraw::Text {
                text: row.clone(),
                x: scaled.width as f32 / 2.0 - width as f32 / 2.0,
                y: y + (LANGUAGE_SLOT_HEIGHT - 8.0) / 2.0,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: true,
                blend: true,
            });
        }
        let width = string_width(font, &self.labels.title);
        draws.push(HudDraw::Text {
            text: self.labels.title.clone(),
            x: scaled.width as f32 / 2.0 - width as f32 / 2.0,
            y: LANGUAGE_TITLE_Y,
            scale: 1.0,
            colour: [1.0, 1.0, 1.0, 1.0],
            shadow: true,
            blend: true,
        });
        let width = string_width(font, &self.labels.warning);
        draws.push(HudDraw::Text {
            text: self.labels.warning.clone(),
            x: scaled.width as f32 / 2.0 - width as f32 / 2.0,
            y: scaled.height as f32 - LANGUAGE_WARNING_INSET,
            scale: 1.0,
            colour: [128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 1.0],
            shadow: true,
            blend: true,
        });
        draws.extend(self.unicode.draws(font, mouse));
        draws.extend(self.done.draws(font, mouse));
        draws
    }
}
