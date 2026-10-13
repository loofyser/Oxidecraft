//! The chat-options screen: the ten rows in source order, the five float
//! sliders 0-1 step 0, and Done (`ScreenChatOptions.java`).

use oxide_assets::font::Font;
use oxide_game::chat::LanguageTable;
use oxide_game::settings::GameSettings;
use oxide_render::hud::{HudDraw, ScaledResolution};
use oxide_render::text::string_width;

use super::options::SaveKind;
use super::widgets::{Button, OptionButton, OptionSlider};

/// Done's button id (`ScreenChatOptions.java:61`).
pub const CHAT_DONE_ID: i32 = 200;

/// The row ids are the source `Options` enum ordinals
/// (`GameSettings.java:1274-1314`).
pub const CHAT_ROW_IDS: [i32; 10] = [15, 16, 17, 18, 19, 25, 27, 28, 26, 42];

/// The ten row titles in source order with their Done.
#[derive(Debug, Clone)]
pub struct ChatLabels {
    /// The centred title (`options.chat.title`).
    pub title: String,
    /// The ten row titles in source order.
    pub rows: [String; 10],
    /// The chat-mode strings (`options.chat.visibility.*`).
    pub visibility_full: String,
    /// The chat-mode strings (`options.chat.visibility.*`).
    pub visibility_system: String,
    /// The chat-mode strings (`options.chat.visibility.*`).
    pub visibility_hidden: String,
    /// Done (`gui.done`).
    pub done: String,
}

/// The handed language's strings with the store's English beneath.
pub fn chat_labels(language: Option<&LanguageTable>) -> ChatLabels {
    let text = |key: &str, fallback: &str| {
        language
            .and_then(|table| table.get(key))
            .unwrap_or(fallback)
            .to_string()
    };
    ChatLabels {
        title: text("options.chat.title", "Chat Settings"),
        rows: [
            text("options.chat.visibility", "Chat"),
            text("options.chat.color", "Colors"),
            text("options.chat.links", "Web Links"),
            text("options.chat.opacity", "Opacity"),
            text("options.chat.links.prompt", "Prompt on Links"),
            text("options.chat.scale", "Scale"),
            text("options.chat.height.focused", "Focused Height"),
            text("options.chat.height.unfocused", "Unfocused Height"),
            text("options.chat.width", "Width"),
            text("options.reducedDebugInfo", "Reduced Debug Info"),
        ],
        visibility_full: text("options.chat.visibility.full", "Shown"),
        visibility_system: text("options.chat.visibility.system", "Commands Only"),
        visibility_hidden: text("options.chat.visibility.hidden", "Hidden"),
        done: text("gui.done", "Done"),
    }
}

/// The mode row's label (`GameSettings.java:615-617`).
pub fn chat_visibility_label(
    mode: u8,
    base: &str,
    full: &str,
    system: &str,
    hidden: &str,
) -> String {
    let value = match mode {
        1 => system,
        2 => hidden,
        _ => full,
    };
    format!("{base}: {value}")
}

/// The opacity slider's label (`GameSettings.java:618-619`): `(int)(f*90+10)%`.
pub fn chat_opacity_label(opacity: f32, base: &str) -> String {
    format!("{base}: {}%", (opacity * 90.0 + 10.0) as i32)
}

/// The width slider's label (`GameSettings.java:627-628`): `floor(w*280+40)` px.
pub fn chat_width_label(width: f32, base: &str) -> String {
    format!("{base}: {}px", (width * 280.0 + 40.0).floor() as i32)
}

/// The height sliders' labels (`GameSettings.java:629-630`): `floor(h*160+20)` px.
pub fn chat_height_label(height: f32, base: &str) -> String {
    format!("{base}: {}px", (height * 160.0 + 20.0).floor() as i32)
}

/// A chat-options row: the five float rows are sliders, the rest buttons.
/// The float arm carries its source id beside the slider (the widget is
/// option-agnostic and carries no id of its own).
#[derive(Debug, Clone)]
pub enum ChatRow {
    /// A toggle button.
    Toggle(OptionButton),
    /// A 0-1 step-0 float slider with its source id.
    Float {
        /// The row's source `Options` ordinal.
        id: i32,
        /// The option-agnostic slider.
        slider: OptionSlider,
    },
}

impl ChatRow {
    /// The row's source id.
    pub fn id(&self) -> i32 {
        match self {
            Self::Toggle(button) => button.button.id,
            Self::Float { id, .. } => *id,
        }
    }

    /// The row's top-left.
    pub fn pos(&self) -> (f32, f32) {
        match self {
            Self::Toggle(button) => (button.x(), button.y()),
            Self::Float { slider, .. } => (slider.x(), slider.y()),
        }
    }

    /// Whether the row is one of the five float sliders (`getEnumFloat`'s
    /// own arm; the rest are cycle buttons).
    pub fn is_slider(&self) -> bool {
        matches!(self, Self::Float { .. })
    }

    /// One press at the point: the slider folds the pointer into its value
    /// and latches; the button answers nothing (the caller hit-tests first).
    pub fn press(&mut self, point: (f32, f32)) -> Option<f32> {
        match self {
            Self::Toggle(_) => None,
            Self::Float { slider, .. } => slider.press(point),
        }
    }

    /// One drag motion at the pointer's x: the latched slider writes again
    /// per event.
    pub fn drag_to(&mut self, x: f32) -> Option<f32> {
        match self {
            Self::Toggle(_) => None,
            Self::Float { slider, .. } => slider.drag_to(x),
        }
    }

    /// One release anywhere: the slider latch clears.
    pub fn release(&mut self) {
        if let Self::Float { slider, .. } = self {
            slider.release();
        }
    }

    /// The row's draws: the button's or the slider's own.
    pub fn draws(&self, font: &Font, mouse: Option<(f32, f32)>) -> Vec<HudDraw> {
        match self {
            Self::Toggle(button) => button.draws(font, mouse),
            Self::Float { slider, .. } => slider.draws(font, mouse),
        }
    }
}

/// The writes a row press can land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatAction {
    /// The mode button cycled 0-1-2.
    Visibility,
    /// Colours flipped.
    Colors,
    /// Links flipped.
    Links,
    /// The opacity slider moved.
    Opacity,
    /// Link prompts flipped.
    LinksPrompt,
    /// The scale slider moved.
    Scale,
    /// The focused-height slider moved.
    HeightFocused,
    /// The unfocused-height slider moved.
    HeightUnfocused,
    /// The width slider moved.
    Width,
    /// Reduced debug flipped.
    ReducedDebug,
    /// Done: the store writes at once.
    Done,
}

impl ChatAction {
    /// Every arm writes the store at once.
    pub fn save_kind(&self) -> SaveKind {
        SaveKind::Immediate
    }
}

/// The chat-options screen.
#[derive(Debug, Clone)]
pub struct ChatScreen {
    /// The ten rows in source order.
    rows: [ChatRow; 10],
    /// Done 200x20 at `w/2-100, h/6+120`.
    done: Button,
    /// The scaled size the screen lays out at.
    width: u32,
    /// The scaled height Done hangs from.
    height: u32,
    /// The labels the rows recompose after a write.
    labels: ChatLabels,
}

impl ChatScreen {
    /// Lay the ten rows out at `w/2-155+(i%2)*160, h/6+24*(i>>1)`.
    pub fn new(scaled: &ScaledResolution, settings: &GameSettings, labels: ChatLabels) -> Self {
        let mut screen = Self {
            rows: core::array::from_fn(|_| {
                ChatRow::Toggle(OptionButton::new(
                    0,
                    0.0,
                    0.0,
                    150.0,
                    20.0,
                    String::new(),
                    String::new(),
                ))
            }),
            done: Button::new(CHAT_DONE_ID, 0.0, 0.0, 200.0, 20.0, labels.done.clone()),
            width: scaled.width,
            height: scaled.height,
            labels,
        };
        screen.rebuild(settings);
        screen
    }

    /// The row count the suite pins.
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// The row's source id.
    pub fn row_id(&self, index: usize) -> i32 {
        self.rows[index].id()
    }

    /// The row's top-left.
    pub fn row_pos(&self, index: usize) -> (f32, f32) {
        self.rows[index].pos()
    }

    /// Whether row `index` is one of the five float sliders.
    pub fn row_is_slider(&self, index: usize) -> bool {
        self.rows[index].is_slider()
    }

    /// Done's top-left (`w/2-100, h/6+120`).
    pub fn done_pos(&self) -> (f32, f32) {
        (self.done.x, self.done.y)
    }

    /// Rebuild every row from the store (fresh labels after a write).
    fn rebuild(&mut self, settings: &GameSettings) {
        let half = f64::from(self.width / 2);
        let base = f64::from(self.height / 6);
        let labels = &self.labels;
        let row = |index: usize| {
            (
                (half - 155.0 + f64::from((index % 2) as u32) * 160.0) as f32,
                (base + f64::from((index >> 1) as u32) * 24.0) as f32,
            )
        };
        let on = |flag: bool| {
            if flag {
                String::from("On")
            } else {
                String::from("Off")
            }
        };
        let mut built: [ChatRow; 10] = core::array::from_fn(|_| {
            ChatRow::Toggle(OptionButton::new(
                0,
                0.0,
                0.0,
                150.0,
                20.0,
                String::new(),
                String::new(),
            ))
        });
        for (index, slot) in built.iter_mut().enumerate() {
            let (x, y) = row(index);
            let id = CHAT_ROW_IDS[index];
            let name = labels.rows[index].clone();
            *slot = match index {
                0 => ChatRow::Toggle(OptionButton::new(
                    id,
                    x,
                    y,
                    150.0,
                    20.0,
                    name,
                    match settings.chat_mode {
                        1 => labels.visibility_system.clone(),
                        2 => labels.visibility_hidden.clone(),
                        _ => labels.visibility_full.clone(),
                    },
                )),
                1 => ChatRow::Toggle(OptionButton::new(
                    id,
                    x,
                    y,
                    150.0,
                    20.0,
                    name,
                    on(settings.chat_colors),
                )),
                2 => ChatRow::Toggle(OptionButton::new(
                    id,
                    x,
                    y,
                    150.0,
                    20.0,
                    name,
                    on(settings.chat_links),
                )),
                3 => ChatRow::Float {
                    id,
                    slider: OptionSlider::new(
                        x,
                        y,
                        150.0,
                        20.0,
                        0.0,
                        1.0,
                        0.0,
                        settings.chat_opacity,
                        chat_opacity_label(settings.chat_opacity, &name),
                    ),
                },
                4 => ChatRow::Toggle(OptionButton::new(
                    id,
                    x,
                    y,
                    150.0,
                    20.0,
                    name,
                    on(settings.chat_links_prompt),
                )),
                5 => ChatRow::Float {
                    id,
                    slider: OptionSlider::new(
                        x,
                        y,
                        150.0,
                        20.0,
                        0.0,
                        1.0,
                        0.0,
                        settings.chat_scale,
                        format!("{}: {}%", name, (settings.chat_scale * 100.0) as i32),
                    ),
                },
                6 => ChatRow::Float {
                    id,
                    slider: OptionSlider::new(
                        x,
                        y,
                        150.0,
                        20.0,
                        0.0,
                        1.0,
                        0.0,
                        settings.chat_height,
                        chat_height_label(settings.chat_height, &name),
                    ),
                },
                7 => ChatRow::Float {
                    id,
                    slider: OptionSlider::new(
                        x,
                        y,
                        150.0,
                        20.0,
                        0.0,
                        1.0,
                        0.0,
                        settings.chat_height_unfocused,
                        chat_height_label(settings.chat_height_unfocused, &name),
                    ),
                },
                8 => ChatRow::Float {
                    id,
                    slider: OptionSlider::new(
                        x,
                        y,
                        150.0,
                        20.0,
                        0.0,
                        1.0,
                        0.0,
                        settings.chat_width,
                        chat_width_label(settings.chat_width, &name),
                    ),
                },
                _ => ChatRow::Toggle(OptionButton::new(
                    id,
                    x,
                    y,
                    150.0,
                    20.0,
                    name,
                    on(settings.reduced_debug_info),
                )),
            };
        }
        self.rows = built;
        self.done.x = (half - 100.0) as f32;
        self.done.y = (base + 120.0) as f32;
    }

    /// The write row `index`'s press lands, in source order.
    fn action_for(index: usize) -> ChatAction {
        match index {
            0 => ChatAction::Visibility,
            1 => ChatAction::Colors,
            2 => ChatAction::Links,
            3 => ChatAction::Opacity,
            4 => ChatAction::LinksPrompt,
            5 => ChatAction::Scale,
            6 => ChatAction::HeightFocused,
            7 => ChatAction::HeightUnfocused,
            8 => ChatAction::Width,
            _ => ChatAction::ReducedDebug,
        }
    }

    /// Folds row `index`'s press into the store: toggles cycle, sliders
    /// fold the pointer. Answers the write it landed.
    fn write_row(
        &mut self,
        index: usize,
        point: (f32, f32),
        settings: &mut GameSettings,
    ) -> ChatAction {
        let action = Self::action_for(index);
        match index {
            0 => settings.chat_mode = (settings.chat_mode + 1) % 3,
            1 => settings.chat_colors = !settings.chat_colors,
            2 => settings.chat_links = !settings.chat_links,
            4 => settings.chat_links_prompt = !settings.chat_links_prompt,
            9 => settings.reduced_debug_info = !settings.reduced_debug_info,
            _ => {
                if let Some(value) = self.rows[index].press(point) {
                    match index {
                        3 => settings.chat_opacity = value,
                        5 => settings.chat_scale = value,
                        6 => settings.chat_height = value,
                        7 => settings.chat_height_unfocused = value,
                        _ => settings.chat_width = value,
                    }
                }
            }
        }
        action
    }

    /// Press a row or Done. Empty space answers nothing. The caller saves
    /// (every arm is immediate) and feeds the new widths into the frame's
    /// chat mirror — the screen owns the store write, the frame owns the
    /// log, and the log keeps split lines, so a width write takes effect
    /// on the next messages while the geometry updates at once.
    pub fn click_at(
        &mut self,
        point: (f32, f32),
        settings: &mut GameSettings,
    ) -> Option<ChatAction> {
        if self.done.pressed(point) {
            return Some(ChatAction::Done);
        }
        for index in 0..self.rows.len() {
            let (x, y) = self.rows[index].pos();
            if point.0 >= x && point.0 < x + 150.0 && point.1 >= y && point.1 < y + 20.0 {
                let action = self.write_row(index, point, settings);
                self.rebuild(settings);
                return Some(action);
            }
        }
        None
    }

    /// Drag a float row; the write lands the same way a press lands it.
    pub fn drag_to(&mut self, x: f32, settings: &mut GameSettings) -> Option<ChatAction> {
        for index in 0..self.rows.len() {
            if let Some(value) = self.rows[index].drag_to(x) {
                match index {
                    3 => settings.chat_opacity = value,
                    5 => settings.chat_scale = value,
                    6 => settings.chat_height = value,
                    7 => settings.chat_height_unfocused = value,
                    8 => settings.chat_width = value,
                    _ => {}
                }
                let action = Self::action_for(index);
                self.rebuild(settings);
                return Some(action);
            }
        }
        None
    }

    /// Release the drag.
    pub fn release(&mut self) {
        for row in &mut self.rows {
            row.release();
        }
    }

    /// The title plus every row, then Done.
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
            y: 20.0,
            scale: 1.0,
            colour: [1.0, 1.0, 1.0, 1.0],
            shadow: true,
            blend: true,
        }];
        for row in &self.rows {
            draws.extend(row.draws(font, mouse));
        }
        draws.extend(self.done.draws(font, mouse));
        draws
    }
}
