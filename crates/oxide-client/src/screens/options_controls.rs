//! The controls screen: invert + sensitivity + touchscreen rows, the
//! sortable binding list with its rebind machine, Reset-All and Done
//! (`GuiControls.java`, `GuiKeyBindingList.java`).

use oxide_assets::font::Font;
use oxide_game::chat::LanguageTable;
use oxide_game::settings::GameSettings;
use oxide_render::hud::{HudDraw, ScaledResolution};
use oxide_render::text::string_width;
use winit::keyboard::KeyCode;

use super::options::SaveKind;
use super::widgets::{Button, OptionButton, OptionSlider};
use crate::keymap::{
    KEY_ESCAPE, binding_label, conflict_codes, display_code, lwjgl_code, rebind_button, rebind_key,
    reset_binding, sorted_binding_names,
};

/// Done's button id (`GuiControls.java:41`).
pub const CONTROLS_DONE_ID: i32 = 200;
/// Reset-All's button id (`:42`).
pub const CONTROLS_RESET_ALL_ID: i32 = 201;
/// The option rows' top (`initGui`:50-54 build at `18 + 24*(i>>1)`).
pub const CONTROLS_ROW_TOP: f32 = 18.0;
/// The title's y (`drawScreen`:161 centres at 8).
const CONTROLS_TITLE_Y: f32 = 8.0;
/// The binding list's top (`GuiKeyBindingList`:20 builds `63..h-32`).
pub const CONTROLS_LIST_TOP: f32 = 63.0;
/// The binding list's bottom inset (`:20`).
pub const CONTROLS_LIST_BOTTOM_INSET: f32 = 32.0;
/// One list slot's height (`:20`).
pub const CONTROLS_SLOT_HEIGHT: f32 = 20.0;
/// The rows' half-width: the 240-wide rows centre on the screen.
pub const CONTROLS_ROW_HALF_WIDTH: f32 = 120.0;
/// The change button's offset in its row (`KeyEntry.drawEntry`: the
/// 75-wide button at `x+105`).
pub const CHANGE_X_OFFSET: f32 = 105.0;
/// The change button's width (`:108`).
pub const CHANGE_WIDTH: f32 = 75.0;
/// The per-row reset button's offset (`:109-112`: the 50-wide button at
/// `x+190`).
pub const RESET_X_OFFSET: f32 = 190.0;
/// The per-row reset button's width (`:111`).
pub const RESET_WIDTH: f32 = 50.0;
/// The red conflict tint (`EnumChatFormatting.RED`).
const CONFLICT_COLOUR: [f32; 4] = [1.0, 0.25, 0.25, 1.0];

/// The controls screen's labels: the title, the three option names, the
/// sensitivity ends, the per-row reset, Reset-All and Done.
#[derive(Debug, Clone)]
pub struct ControlsLabels {
    /// The centred title (`controls.title`).
    pub title: String,
    /// The invert toggle's name (`options.invertMouse`).
    pub invert: String,
    /// The sensitivity slider's name (`options.sensitivity`).
    pub sensitivity: String,
    /// The sensitivity floor (`options.sensitivity.min`).
    pub sensitivity_min: String,
    /// The sensitivity ceiling (`options.sensitivity.max`).
    pub sensitivity_max: String,
    /// The touchscreen toggle's name (`options.touchscreen`).
    pub touchscreen: String,
    /// The per-row reset (`controls.reset`).
    pub reset: String,
    /// Reset-All (`controls.resetAll`).
    pub reset_all: String,
    /// Done (`gui.done`).
    pub done: String,
    /// The on string (`options.on`).
    pub on: String,
    /// The off string (`options.off`).
    pub off: String,
}

/// Resolves the controls screen's labels through the handed-over table,
/// falling back to the store's English.
pub fn controls_labels(language: Option<&LanguageTable>) -> ControlsLabels {
    let text = |key: &str, fallback: &str| {
        language
            .and_then(|table| table.get(key))
            .unwrap_or(fallback)
            .to_string()
    };
    ControlsLabels {
        title: text("controls.title", "Controls"),
        invert: text("options.invertMouse", "Invert Mouse"),
        sensitivity: text("options.sensitivity", "Sensitivity"),
        sensitivity_min: text("options.sensitivity.min", "*yawn*"),
        sensitivity_max: text("options.sensitivity.max", "HYPERSPEED!!!"),
        touchscreen: text("options.touchscreen", "Touchscreen Mode"),
        reset: text("controls.reset", "Reset"),
        reset_all: text("controls.resetAll", "Reset Keys"),
        done: text("gui.done", "Done"),
        on: text("options.on", "On"),
        off: text("options.off", "Off"),
    }
}

/// The sensitivity slider's label: the min string at 0, the max string at
/// 1, `(int)(sensitivity*200)%` otherwise (`GameSettings.java:623`).
pub fn sensitivity_label_value(base: &str, sensitivity: f32, min: &str, max: &str) -> String {
    if sensitivity == 0.0 {
        format!("{base}: {min}")
    } else if sensitivity == 1.0 {
        format!("{base}: {max}")
    } else {
        format!("{base}: {}%", (sensitivity * 200.0) as i32)
    }
}

/// One controls press: the option or list write it ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlsAction {
    /// The invert toggle cycled.
    Invert,
    /// The sensitivity slider wrote.
    Sensitivity,
    /// A change press armed row `i`.
    Armed(usize),
    /// The armed row `i` bound its key.
    Rebound(usize),
    /// The armed row `i` unbound (Escape).
    Unbound(usize),
    /// Row `i`'s reset restored its default.
    ResetRow(usize),
    /// Reset-All restored every default.
    ResetAll,
    /// Done answers the hub (the owner saves first).
    Done,
}

impl ControlsAction {
    /// The press's save rule: the sensitivity slider defers to Done like
    /// the hub's FOV slider; every other arm saves at once (each
    /// `setOptionKeyBinding` route and Done save first).
    pub fn save_kind(self) -> SaveKind {
        match self {
            ControlsAction::Sensitivity => SaveKind::Deferred,
            _ => SaveKind::Immediate,
        }
    }
}

/// One binding's list row: the description, the 75-wide change button and
/// the 50-wide per-row reset (`KeyEntry`'s pair at `x+105` / `x+190`).
#[derive(Debug, Clone)]
pub struct BindingRow {
    /// The binding's store name (`key.<name>` minus the prefix).
    name: &'static str,
    /// The description the row draws (`key.<name>`).
    desc: String,
    /// The change button.
    change: Button,
    /// The per-row reset button (`controls.reset`).
    reset: Button,
    /// Whether another row shares the code (the red-only conflict).
    conflicted: bool,
}

impl BindingRow {
    /// The binding's store name.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The change button's left edge.
    pub fn change_x(&self) -> f32 {
        self.change.x
    }

    /// The row's top edge.
    pub fn y(&self) -> f32 {
        self.change.y
    }

    /// The change button's label (the display code, or `> code <` armed).
    pub fn change_label(&self) -> &str {
        &self.change.label
    }

    /// Whether the row shares its code (red-only, never blocked).
    pub fn conflicted(&self) -> bool {
        self.conflicted
    }
}

/// One list entry: a category header or a binding row (the source
/// interleaves `CategoryEntry` headers over the sorted bindings).
#[derive(Debug, Clone)]
enum Entry {
    /// A centred category header (never presses).
    Header(String),
    /// A binding row.
    Key(BindingRow),
}

/// The controls screen: the three option rows, the binding list, Reset-All
/// and Done.
#[derive(Debug, Clone)]
pub struct ControlsScreen {
    /// The invert toggle row 0 left.
    invert: OptionButton,
    /// The sensitivity slider row 0 right.
    sensitivity: OptionSlider,
    /// The touchscreen toggle row 1 left (inert: the store carries no
    /// touchscreen flag, so the press records nothing).
    touchscreen: OptionButton,
    /// The list entries: category headers over the sorted binding rows.
    entries: Vec<Entry>,
    /// The binding names in list order (the sort runs once; writes only
    /// reseat codes and gates).
    order: Vec<&'static str>,
    /// The table the rows resolve through, if one was handed over.
    language: Option<LanguageTable>,
    /// The armed row's key-entry index: the next key or click binds it.
    armed: Option<usize>,
    /// The list's scroll offset in GUI pixels.
    scroll: f32,
    /// Done 200 + Reset-All 201 at `h-29`.
    done: Button,
    /// Reset-All, gated on any row differing from its default.
    reset_all: Button,
    /// The labels the controls recompose after a write.
    labels: ControlsLabels,
    /// The scaled size the screen lays out at.
    width: u32,
    /// The scaled height the list clips against.
    height: u32,
}

impl ControlsScreen {
    /// Stands the controls screen at `scaled` on `settings` with `labels`:
    /// invert/sensitivity at y 18, touchscreen at y 42, the binding list
    /// `63..h-32` in `(category, description)` sort with headers, Done +
    /// Reset-All at `h-29` (`initGui`:38-59, `GuiKeyBindingList`:20-43).
    /// The rows resolve through `language` when one is handed over.
    pub fn new(
        scaled: &ScaledResolution,
        settings: &GameSettings,
        labels: ControlsLabels,
        language: Option<&LanguageTable>,
    ) -> Self {
        let order = sorted_binding_names(language);
        let mut screen = Self {
            invert: OptionButton::new(0, 0.0, 0.0, 150.0, 20.0, String::new(), String::new()),
            sensitivity: OptionSlider::new(
                0.0,
                0.0,
                150.0,
                20.0,
                0.0,
                1.0,
                0.0,
                0.5,
                String::new(),
            ),
            touchscreen: OptionButton::new(24, 0.0, 0.0, 150.0, 20.0, String::new(), String::new()),
            entries: Vec::new(),
            order,
            language: language.cloned(),
            armed: None,
            scroll: 0.0,
            done: Button::new(200, 0.0, 0.0, 150.0, 20.0, String::new()),
            reset_all: Button::new(201, 0.0, 0.0, 150.0, 20.0, String::new()),
            labels,
            width: scaled.width,
            height: scaled.height,
        };
        screen.rebuild(settings, None);
        screen
    }

    /// The On/Off string for a toggle.
    fn on_off(on: bool, labels: &ControlsLabels) -> String {
        if on {
            labels.on.clone()
        } else {
            labels.off.clone()
        }
    }

    /// The category header's label through the handed table (the source
    /// formats the `key.categories.*` key).
    fn category_label(&self, category: &str) -> String {
        const FALLBACKS: [(&str, &str); 6] = [
            ("key.categories.gameplay", "Gameplay"),
            ("key.categories.inventory", "Inventory"),
            ("key.categories.misc", "Miscellaneous"),
            ("key.categories.movement", "Movement"),
            ("key.categories.multiplayer", "Multiplayer"),
            ("key.categories.stream", "Streaming"),
        ];
        let fallback = FALLBACKS
            .iter()
            .find(|(known, _)| *known == category)
            .map(|(_, label)| *label)
            .unwrap_or(category);
        self.language
            .as_ref()
            .and_then(|table| table.get(category))
            .unwrap_or(fallback)
            .to_string()
    }

    /// The row's stored code, 0 for an unknown name.
    fn row_code(settings: &GameSettings, name: &str) -> i32 {
        settings
            .bindings
            .iter()
            .find(|binding| binding.name == name)
            .map(|binding| binding.code)
            .unwrap_or(0)
    }

    /// Reseats every control at the stored size on `settings`, keeping the
    /// armed row and the scroll (every binding write rebuilds the conflict
    /// map and the labels like the source's per-draw rebuild).
    fn rebuild(&mut self, settings: &GameSettings, armed: Option<usize>) {
        let half = self.width / 2;
        let left = (half - 155) as f32;
        let right = (half + 5) as f32;
        self.invert = OptionButton::new(
            0,
            left,
            CONTROLS_ROW_TOP,
            150.0,
            20.0,
            self.labels.invert.clone(),
            Self::on_off(settings.invert_mouse, &self.labels),
        );
        self.sensitivity = OptionSlider::new(
            right,
            CONTROLS_ROW_TOP,
            150.0,
            20.0,
            0.0,
            1.0,
            0.0,
            settings.mouse_sensitivity.clamp(0.0, 1.0),
            sensitivity_label_value(
                &self.labels.sensitivity,
                settings.mouse_sensitivity,
                &self.labels.sensitivity_min,
                &self.labels.sensitivity_max,
            ),
        );
        self.touchscreen = OptionButton::new(
            24,
            left,
            CONTROLS_ROW_TOP + 24.0,
            150.0,
            20.0,
            self.labels.touchscreen.clone(),
            self.labels.off.clone(),
        );
        let row_left = half as f32 - CONTROLS_ROW_HALF_WIDTH;
        let conflicts = conflict_codes(&settings.bindings);
        let mut entries = Vec::new();
        let mut category: Option<&str> = None;
        for name in &self.order {
            let row_category = GameSettings::binding_category(name).unwrap_or("");
            if category != Some(row_category) {
                category = Some(row_category);
                entries.push(Entry::Header(self.category_label(row_category)));
            }
            let code = Self::row_code(settings, name);
            entries.push(Entry::Key(BindingRow {
                name,
                desc: binding_label(name, self.language.as_ref()),
                change: Button::new(
                    0,
                    row_left + CHANGE_X_OFFSET,
                    0.0,
                    CHANGE_WIDTH,
                    20.0,
                    display_code(code),
                ),
                reset: Button::new(
                    0,
                    row_left + RESET_X_OFFSET,
                    0.0,
                    RESET_WIDTH,
                    20.0,
                    self.labels.reset.clone(),
                ),
                conflicted: conflicts.contains_key(&code),
            }));
        }
        self.entries = entries;
        self.armed = armed;
        self.done = Button::new(
            CONTROLS_DONE_ID,
            left,
            (self.height - 29) as f32,
            150.0,
            20.0,
            self.labels.done.clone(),
        );
        self.reset_all = Button::new(
            CONTROLS_RESET_ALL_ID,
            right,
            (self.height - 29) as f32,
            150.0,
            20.0,
            self.labels.reset_all.clone(),
        );
        self.lay_rows();
        self.refresh_gates(settings);
        self.refresh_armed_label(settings);
    }

    /// Seats every row's buttons at the scrolled list positions (the list
    /// rebuild reseats on scroll like the source's per-draw layout).
    fn lay_rows(&mut self) {
        for (slot, entry) in self.entries.iter_mut().enumerate() {
            let y = CONTROLS_LIST_TOP + slot as f32 * CONTROLS_SLOT_HEIGHT - self.scroll;
            if let Entry::Key(row) = entry {
                row.change.y = y;
                row.reset.y = y;
            }
        }
    }

    /// Refreshes the Reset-All gate and the per-row reset gates: Reset-All
    /// lights only while a row differs from its default
    /// (`drawScreen`:162-173); a per-row reset lights on the same rule
    /// (`drawEntry`'s enabled arm).
    fn refresh_gates(&mut self, settings: &GameSettings) {
        let mut any = false;
        for entry in &mut self.entries {
            if let Entry::Key(row) = entry {
                let changed = Self::row_code(settings, row.name)
                    != GameSettings::default_binding(row.name).unwrap_or(0);
                row.reset.enabled = changed;
                any = any || changed;
            }
        }
        self.reset_all.enabled = any;
    }

    /// Rewrites the armed row's change label to `> code <` (the source's
    /// armed dressing, without formatting codes).
    fn refresh_armed_label(&mut self, settings: &GameSettings) {
        let mut key_index = 0;
        for entry in &mut self.entries {
            if let Entry::Key(row) = entry {
                let code = Self::row_code(settings, row.name);
                row.change.label = if Some(key_index) == self.armed {
                    format!("> {} <", display_code(code))
                } else {
                    display_code(code)
                };
                key_index += 1;
            }
        }
    }

    /// The invert toggle's top-left.
    pub fn invert_pos(&self) -> (f32, f32) {
        (self.invert.x(), self.invert.y())
    }

    /// The sensitivity slider's top-left.
    pub fn sensitivity_pos(&self) -> (f32, f32) {
        (self.sensitivity.x(), self.sensitivity.y())
    }

    /// The touchscreen toggle's top-left.
    pub fn touchscreen_pos(&self) -> (f32, f32) {
        (self.touchscreen.x(), self.touchscreen.y())
    }

    /// Done's top-left.
    pub fn done_pos(&self) -> (f32, f32) {
        (self.done.x, self.done.y)
    }

    /// Reset-All's top-left.
    pub fn reset_all_pos(&self) -> (f32, f32) {
        (self.reset_all.x, self.reset_all.y)
    }

    /// The binding rows (headers excluded).
    pub fn binding_row_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| matches!(entry, Entry::Key(_)))
            .count()
    }

    /// Key-entry `i`'s row.
    pub fn binding_row(&self, index: usize) -> &BindingRow {
        self.entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Key(row) => Some(row),
                Entry::Header(_) => None,
            })
            .nth(index)
            .expect("the binding row stands")
    }

    /// The armed row's key-entry index, if a change press armed one.
    pub fn armed(&self) -> Option<usize> {
        self.armed
    }

    /// Whether Reset-All lights (any row off its default).
    pub fn reset_enabled(&self) -> bool {
        self.reset_all.enabled
    }

    /// One press at the GUI-space point with the mouse button: while a row
    /// stands armed the click binds the negative button (`mouseClicked`
    /// arms first at `:100-110`); otherwise the option rows write, a
    /// change press arms, a per-row reset restores its default, Reset-All
    /// restores every default, Done answers the hub. Empty space answers
    /// nothing.
    pub fn click_button(
        &mut self,
        button: u8,
        point: (f32, f32),
        settings: &mut GameSettings,
    ) -> Option<ControlsAction> {
        if let Some(index) = self.armed {
            let name = self.binding_row(index).name;
            rebind_button(settings, name, button);
            self.rebuild(settings, None);
            return Some(ControlsAction::Rebound(index));
        }
        if self.invert.press(point) {
            settings.invert_mouse = !settings.invert_mouse;
            self.invert
                .set_value(Self::on_off(settings.invert_mouse, &self.labels));
            return Some(ControlsAction::Invert);
        }
        if self.sensitivity.press(point).is_some() {
            settings.mouse_sensitivity = self.sensitivity.value().clamp(0.0, 1.0);
            self.sensitivity.set_label(sensitivity_label_value(
                &self.labels.sensitivity,
                settings.mouse_sensitivity,
                &self.labels.sensitivity_min,
                &self.labels.sensitivity_max,
            ));
            return Some(ControlsAction::Sensitivity);
        }
        if self.touchscreen.press(point) {
            return None;
        }
        if self.done.pressed(point) {
            return Some(ControlsAction::Done);
        }
        if self.reset_all.pressed(point) {
            settings.reset_bindings();
            self.rebuild(settings, None);
            return Some(ControlsAction::ResetAll);
        }
        let mut hit: Option<(usize, bool)> = None;
        let mut key_index = 0;
        for entry in &self.entries {
            if let Entry::Key(row) = entry {
                if row.change.hit(point) {
                    hit = Some((key_index, false));
                    break;
                }
                if row.reset.enabled && row.reset.hit(point) {
                    hit = Some((key_index, true));
                    break;
                }
                key_index += 1;
            }
        }
        match hit {
            Some((index, false)) => {
                self.armed = Some(index);
                self.refresh_armed_label(settings);
                Some(ControlsAction::Armed(index))
            }
            Some((index, true)) => {
                let name = self.binding_row(index).name;
                reset_binding(settings, name);
                self.rebuild(settings, None);
                Some(ControlsAction::ResetRow(index))
            }
            None => None,
        }
    }

    /// One left press at the GUI-space point (`mouseClicked`'s button-0
    /// path through [`ControlsScreen::click_button`]).
    pub fn click_at(
        &mut self,
        point: (f32, f32),
        settings: &mut GameSettings,
    ) -> Option<ControlsAction> {
        self.click_button(0, point, settings)
    }

    /// One key press while armed: Escape unbinds to 0, any mapped key
    /// binds, an unmapped key only disarms (`keyTyped`:127-152). With no
    /// armed row the screen consumes nothing.
    pub fn key(&mut self, code: KeyCode, settings: &mut GameSettings) -> Option<ControlsAction> {
        let index = self.armed?;
        match lwjgl_code(code) {
            Some(lwjgl) => {
                let name = self.binding_row(index).name;
                rebind_key(settings, name, lwjgl);
                let unbound = lwjgl == KEY_ESCAPE;
                self.rebuild(settings, None);
                if unbound {
                    Some(ControlsAction::Unbound(index))
                } else {
                    Some(ControlsAction::Rebound(index))
                }
            }
            None => {
                self.rebuild(settings, None);
                None
            }
        }
    }

    /// One drag motion at the pointer's x: the latched sensitivity slider
    /// writes again per event.
    pub fn drag_to(&mut self, mouse_x: f32, settings: &mut GameSettings) -> Option<ControlsAction> {
        if self.sensitivity.drag_to(mouse_x).is_some() {
            settings.mouse_sensitivity = self.sensitivity.value().clamp(0.0, 1.0);
            self.sensitivity.set_label(sensitivity_label_value(
                &self.labels.sensitivity,
                settings.mouse_sensitivity,
                &self.labels.sensitivity_min,
                &self.labels.sensitivity_max,
            ));
            return Some(ControlsAction::Sensitivity);
        }
        None
    }

    /// One release anywhere: the slider latch clears.
    pub fn release(&mut self) {
        self.sensitivity.release();
    }

    /// One wheel motion: the binding list scrolls by whole slots, clamped
    /// to its overflow (`GuiKeyBindingList` rides `GuiSlot`'s scroll).
    pub fn wheel(&mut self, lines: i32) {
        let span = self.height as f32 - CONTROLS_LIST_BOTTOM_INSET - CONTROLS_LIST_TOP;
        let overflow = (self.entries.len() as f32 * CONTROLS_SLOT_HEIGHT - span).max(0.0);
        self.scroll = (self.scroll + lines as f32 * CONTROLS_SLOT_HEIGHT).clamp(0.0, overflow);
        self.lay_rows();
    }

    /// The screen's content draws: the centred title at `(w/2, 8)`, the
    /// three option rows, the visible list slots (headers centred, rows
    /// with description + change + reset, conflicts red-only), then Done +
    /// Reset-All (`drawScreen`, then `super.drawScreen`).
    pub fn draws(
        &self,
        font: &Font,
        scaled: &ScaledResolution,
        mouse: Option<(f32, f32)>,
    ) -> Vec<HudDraw> {
        let width = string_width(font, &self.labels.title);
        let mut draws = vec![HudDraw::Text {
            text: self.labels.title.clone(),
            x: scaled.width as f32 / 2.0 - width as f32 / 2.0,
            y: CONTROLS_TITLE_Y,
            scale: 1.0,
            colour: [1.0, 1.0, 1.0, 1.0],
            shadow: true,
            blend: true,
        }];
        draws.extend(self.invert.draws(font, mouse));
        draws.extend(self.sensitivity.draws(font, mouse));
        draws.extend(self.touchscreen.draws(font, mouse));
        let bottom = self.height as f32 - CONTROLS_LIST_BOTTOM_INSET;
        for (slot, entry) in self.entries.iter().enumerate() {
            let y = CONTROLS_LIST_TOP + slot as f32 * CONTROLS_SLOT_HEIGHT - self.scroll;
            if y < CONTROLS_LIST_TOP - CONTROLS_SLOT_HEIGHT || y >= bottom {
                continue;
            }
            match entry {
                Entry::Header(label) => {
                    let width = string_width(font, label);
                    draws.push(HudDraw::Text {
                        text: label.clone(),
                        x: scaled.width as f32 / 2.0 - width as f32 / 2.0,
                        y: y + CONTROLS_SLOT_HEIGHT - 9.0,
                        scale: 1.0,
                        colour: [1.0, 1.0, 1.0, 1.0],
                        shadow: true,
                        blend: true,
                    });
                }
                Entry::Key(row) => {
                    let desc_width = string_width(font, &row.desc);
                    draws.push(HudDraw::Text {
                        text: row.desc.clone(),
                        x: row.change.x - 5.0 - desc_width as f32,
                        y: y + 6.0,
                        scale: 1.0,
                        colour: [1.0, 1.0, 1.0, 1.0],
                        shadow: true,
                        blend: true,
                    });
                    draws.extend(row.change.draws(font, mouse));
                    if row.conflicted {
                        let label_width = string_width(font, &row.change.label);
                        draws.push(HudDraw::Text {
                            text: row.change.label.clone(),
                            x: row.change.x + (CHANGE_WIDTH - label_width as f32) / 2.0,
                            y: row.change.y + 6.0,
                            scale: 1.0,
                            colour: CONFLICT_COLOUR,
                            shadow: true,
                            blend: true,
                        });
                    }
                    draws.extend(row.reset.draws(font, mouse));
                }
            }
        }
        draws.extend(self.done.draws(font, mouse));
        draws.extend(self.reset_all.draws(font, mouse));
        draws
    }
}
