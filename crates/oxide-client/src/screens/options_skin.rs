//! The skin-customization screen: one toggle per player-model part plus
//! Done (`GuiCustomizeSkin.java`).

use oxide_assets::font::Font;
use oxide_game::chat::LanguageTable;
use oxide_game::settings::GameSettings;
use oxide_render::hud::{HudDraw, ScaledResolution};
use oxide_render::text::string_width;

use super::options::SaveKind;
use super::widgets::Button;

/// Done's button id (`initGui` builds it at 200).
pub const SKIN_DONE_ID: i32 = 200;
/// The title's y (`drawScreen` centres it at 20).
const SKIN_TITLE_Y: f32 = 20.0;

/// The seven model parts in enum order (`EnumPlayerModelParts.java:8-33`):
/// cape included as part 0 — there is no separate cape switch.
pub const SKIN_PARTS: [&str; 7] = [
    "cape",
    "jacket",
    "left_sleeve",
    "right_sleeve",
    "left_pants_leg",
    "right_pants_leg",
    "hat",
];

/// The English part names the buttons compose (`options.modelPart.*`
/// rows), in [`SKIN_PARTS`] order.
const SKIN_PART_NAMES: [&str; 7] = [
    "Cape",
    "Jacket",
    "Left Sleeve",
    "Right Sleeve",
    "Left Pants Leg",
    "Right Pants Leg",
    "Hat",
];

/// The label keys in [`SKIN_PARTS`] order (`options.modelPart.*`,
/// `en_US.lang`:389-395 — not the `skinCustomisation` title key).
const SKIN_PART_KEYS: [&str; 7] = [
    "options.modelPart.cape",
    "options.modelPart.jacket",
    "options.modelPart.left_sleeve",
    "options.modelPart.right_sleeve",
    "options.modelPart.left_pants_leg",
    "options.modelPart.right_pants_leg",
    "options.modelPart.hat",
];

/// The skin screen's labels: the title, the seven part names and Done.
#[derive(Debug, Clone)]
pub struct SkinLabels {
    /// The centred title (`options.skinCustomisation.title`).
    pub title: String,
    /// The seven part names in [`SKIN_PARTS`] order.
    pub parts: [String; 7],
    /// The on string (`options.on`).
    pub on: String,
    /// The off string (`options.off`).
    pub off: String,
    /// Done (`gui.done`).
    pub done: String,
}

/// Resolves the skin screen's labels through the handed-over table,
/// falling back to the store's English.
pub fn skin_labels(language: Option<&LanguageTable>) -> SkinLabels {
    let text = |key: &str, fallback: &str| {
        language
            .and_then(|table| table.get(key))
            .unwrap_or(fallback)
            .to_string()
    };
    SkinLabels {
        title: text("options.skinCustomisation.title", "Skin Customization"),
        parts: core::array::from_fn(|index| text(SKIN_PART_KEYS[index], SKIN_PART_NAMES[index])),
        on: text("options.on", "On"),
        off: text("options.off", "Off"),
        done: text("gui.done", "Done"),
    }
}

/// One part button's label: the part name, then its On/Off state.
pub fn skin_part_label(name: &str, enabled: bool, on: &str, off: &str) -> String {
    if enabled {
        format!("{name}: {on}")
    } else {
        format!("{name}: {off}")
    }
}

/// One skin press: the part that flipped, or Done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkinAction {
    /// Part `i` flipped its bit (`actionPerformed`'s `ButtonPart` arm).
    Part(usize),
    /// Done saves and answers the hub (the 200 arm).
    Done,
}

impl SkinAction {
    /// Both arms write the store at once: the flip rides
    /// `switchModelPartEnabled`'s save, Done saves on its way out.
    pub fn save_kind(self) -> SaveKind {
        SaveKind::Immediate
    }
}

/// The skin-customization screen: the seven part toggles in enum order
/// plus Done. Each press flips the part's bit in the skin-parts byte and
/// relabels; the caller pushes the byte to the session (the C15 re-send
/// rides the standing session path) and saves.
#[derive(Debug, Clone)]
pub struct SkinScreen {
    /// The seven part buttons, ids = part ids.
    parts: [Button; 7],
    /// Done 200x20 two slots below the odd seventh.
    done: Button,
    /// The labels the buttons recompose after a write.
    labels: SkinLabels,
}

impl SkinScreen {
    /// Stands the skin screen at `scaled` on `settings` with `labels`:
    /// the seven 150x20 pairs at `w/2-155`/`w/2+5`, rows
    /// `h/6+24*(i>>1)`, Done 200x20 two slots below (`initGui`:24-41).
    pub fn new(scaled: &ScaledResolution, settings: &GameSettings, labels: SkinLabels) -> Self {
        let half = scaled.width / 2;
        let base = scaled.height / 6;
        let row = |index: usize| {
            (
                (half - 155 + ((index % 2) as u32) * 160) as f32,
                (base + 24 * ((index >> 1) as u32)) as f32,
            )
        };
        let mut screen = Self {
            parts: core::array::from_fn(|index| {
                let (x, y) = row(index);
                Button::new(index as i32, x, y, 150.0, 20.0, String::new())
            }),
            done: Button::new(
                SKIN_DONE_ID,
                (half - 100) as f32,
                (base + 96) as f32,
                200.0,
                20.0,
                labels.done.clone(),
            ),
            labels,
        };
        screen.relabel(settings);
        screen
    }

    /// Whether part `i`'s bit stands in the byte (`isWearing`'s read).
    fn part_on(settings: &GameSettings, part: usize) -> bool {
        settings.skin_parts & (1 << part) != 0
    }

    /// Rewrites every button's On/Off label from the byte.
    fn relabel(&mut self, settings: &GameSettings) {
        for (index, button) in self.parts.iter_mut().enumerate() {
            button.label = skin_part_label(
                &self.labels.parts[index],
                Self::part_on(settings, index),
                &self.labels.on,
                &self.labels.off,
            );
        }
    }

    /// The part buttons.
    pub fn part_count(&self) -> usize {
        self.parts.len()
    }

    /// Part `i`'s top-left.
    pub fn part_pos(&self, index: usize) -> (f32, f32) {
        (self.parts[index].x, self.parts[index].y)
    }

    /// Part `i`'s composed label.
    pub fn part_label(&self, index: usize) -> &str {
        &self.parts[index].label
    }

    /// Done's top-left.
    pub fn done_pos(&self) -> (f32, f32) {
        (self.done.x, self.done.y)
    }

    /// One press at the GUI-space point: a part button flips the part's
    /// bit (`switchModelPartEnabled`) and relabels, Done answers the hub.
    /// Empty space answers nothing.
    pub fn click_at(
        &mut self,
        point: (f32, f32),
        settings: &mut GameSettings,
    ) -> Option<SkinAction> {
        if self.done.pressed(point) {
            return Some(SkinAction::Done);
        }
        for (index, button) in self.parts.iter().enumerate() {
            if button.pressed(point) {
                settings.skin_parts ^= 1 << index;
                self.relabel(settings);
                return Some(SkinAction::Part(index));
            }
        }
        None
    }

    /// The screen's content draws: the centred title at y 20, the seven
    /// part buttons, then Done (`drawScreen`, then `super.drawScreen`).
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
            y: SKIN_TITLE_Y,
            scale: 1.0,
            colour: [1.0, 1.0, 1.0, 1.0],
            shadow: true,
            blend: true,
        }];
        for button in &self.parts {
            draws.extend(button.draws(font, mouse));
        }
        draws.extend(self.done.draws(font, mouse));
        draws
    }
}
