//! The music-and-sounds screen: the nine category sliders with the master
//! full-width on top, then the eight pairs, then Done
//! (`GuiScreenOptionsSounds.java`).

use oxide_assets::font::Font;
use oxide_game::audio::SoundCategory;
use oxide_game::chat::LanguageTable;
use oxide_game::settings::GameSettings;
use oxide_render::hud::{HudDraw, ScaledResolution};
use oxide_render::text::string_width;

use super::options::SaveKind;
use super::widgets::{Button, OptionSlider};

/// The Done button id (`GuiScreenOptionsSounds.java:50`).
pub const SOUND_DONE_ID: i32 = 200;
/// The title's y (`drawScreen`:74 centres at 15).
const SOUND_TITLE_Y: f32 = 15.0;
/// The master's full width (`Button`:93 passes 310 for master).
const MASTER_WIDTH: f32 = 310.0;

/// The sound screen's labels: the title, the Off string and the nine
/// category names (`soundCategory.<name>`).
#[derive(Debug, Clone)]
pub struct SoundLabels {
    /// The centred title (`options.sounds.title`).
    pub title: String,
    /// The zero-level string (`options.off`).
    pub off: String,
    /// Done (`gui.done`).
    pub done: String,
    /// The nine slider names in category order.
    pub categories: [String; 9],
}

/// Resolves the sound screen's labels: the title, the Off string and the
/// nine `soundCategory.<name>` names, each falling back to the store's
/// English (`GuiScreenOptionsSounds.java:36-38,95`).
pub fn sound_labels(language: Option<&LanguageTable>) -> SoundLabels {
    let text = |key: &str, fallback: &str| {
        language
            .and_then(|table| table.get(key))
            .unwrap_or(fallback)
            .to_string()
    };
    let category = |name: &str, fallback: &str| text(&format!("soundCategory.{name}"), fallback);
    SoundLabels {
        title: text("options.sounds.title", "Music & Sound Options"),
        off: text("options.off", "Off"),
        done: text("gui.done", "Done"),
        categories: [
            category("master", "Master Volume"),
            category("music", "Music"),
            category("record", "Jukebox/Note Blocks"),
            category("weather", "Weather"),
            category("block", "Blocks"),
            category("hostile", "Hostile Creatures"),
            category("neutral", "Friendly Creatures"),
            category("player", "Players"),
            category("ambient", "Ambient/Environment"),
        ],
    }
}

/// One sound slider's label: the category name, then the Off string at
/// zero or `(int)(level*100)%` (`getSoundVolume`:78-82).
pub fn sound_label_value(name: &str, level: f32, off: &str) -> String {
    if level == 0.0 {
        format!("{name}: {off}")
    } else {
        format!("{name}: {}%", (level * 100.0) as i32)
    }
}

/// One sound press: the category that wrote, or Done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundAction {
    /// Slider `i` wrote its category's level.
    Level(usize),
    /// Done saves and answers the hub.
    Done,
}

impl SoundAction {
    /// Both arms save: every press and drag writes the level through the
    /// store's save path, and Done saves on its way out
    /// (`mousePressed`:130-131, `mouseDragged`:113-114, Done `:62`).
    pub fn save_kind(self) -> SaveKind {
        match self {
            SoundAction::Level(_) | SoundAction::Done => SaveKind::Immediate,
        }
    }
}

/// The music-and-sounds screen: the nine sliders in category order plus
/// Done.
#[derive(Debug, Clone)]
pub struct SoundScreen {
    /// The nine category sliders, master first.
    sliders: [OptionSlider; 9],
    /// Done 200x20 at `w/2-100, h/6+168`.
    done: Button,
    /// The labels the sliders recompose after a write.
    labels: SoundLabels,
}

impl SoundScreen {
    /// Stands the sound screen at `scaled` on `settings` with `labels`:
    /// the master 310-wide at `w/2-155, h/6-12`, then the eight pairs at
    /// `w/2-155`/`w/2+5`, rows `h/6-12+24*(i>>1)`, Done 200x20 at
    /// `w/2-100, h/6+168` (`initGui`:38-50).
    pub fn new(scaled: &ScaledResolution, settings: &GameSettings, labels: SoundLabels) -> Self {
        let half = scaled.width / 2;
        let base = scaled.height / 6 - 12;
        let mut i = 0;
        let sliders: [OptionSlider; 9] = SoundCategory::ORDER.map(|category| {
            let index = category.as_index();
            let wide = category == SoundCategory::Master;
            let x = half - 155 + (i % 2) * 160;
            let y = base + 24 * (i >> 1);
            i += if wide { 2 } else { 1 };
            let width = if wide { MASTER_WIDTH } else { 150.0 };
            let level = settings.category_volumes[index].clamp(0.0, 1.0);
            OptionSlider::new(
                x as f32,
                y as f32,
                width,
                20.0,
                0.0,
                1.0,
                0.0,
                level,
                sound_label_value(&labels.categories[index], level, &labels.off),
            )
        });
        Self {
            sliders,
            done: Button::new(
                SOUND_DONE_ID,
                (half - 100) as f32,
                (base + 180) as f32,
                200.0,
                20.0,
                labels.done.clone(),
            ),
            labels,
        }
    }

    /// The nine sliders.
    pub fn slider_count(&self) -> usize {
        self.sliders.len()
    }

    /// Slider `i` in category order.
    pub fn slider(&self, index: usize) -> &OptionSlider {
        &self.sliders[index]
    }

    /// Done's top-left (`w/2-100, h/6+168`).
    pub fn done_pos(&self) -> (f32, f32) {
        (self.done.x, self.done.y)
    }

    /// One press at the GUI-space point: a slider folds the pointer into
    /// its category's level (`mousePressed`:124-135), Done answers Done.
    /// Empty space answers nothing.
    pub fn click_at(
        &mut self,
        point: (f32, f32),
        settings: &mut GameSettings,
    ) -> Option<SoundAction> {
        if self.done.pressed(point) {
            return Some(SoundAction::Done);
        }
        for (index, slider) in self.sliders.iter_mut().enumerate() {
            if slider.press(point).is_some() {
                let level = slider.value().clamp(0.0, 1.0);
                settings.category_volumes[index] = level;
                slider.set_label(sound_label_value(
                    &self.labels.categories[index],
                    level,
                    &self.labels.off,
                ));
                return Some(SoundAction::Level(index));
            }
        }
        None
    }

    /// One drag motion at the pointer's x: the latched slider writes again
    /// per event (`mouseDragged`:105-122).
    pub fn drag_to(&mut self, mouse_x: f32, settings: &mut GameSettings) -> Option<SoundAction> {
        for (index, slider) in self.sliders.iter_mut().enumerate() {
            if slider.drag_to(mouse_x).is_some() {
                let level = slider.value().clamp(0.0, 1.0);
                settings.category_volumes[index] = level;
                slider.set_label(sound_label_value(
                    &self.labels.categories[index],
                    level,
                    &self.labels.off,
                ));
                return Some(SoundAction::Level(index));
            }
        }
        None
    }

    /// One release anywhere: the slider latch clears (`mouseReleased`).
    /// Answers whether a latch stood: the caller plays `gui.button.press`
    /// on one — the source plays it on every dragging release, the master
    /// arm's level read beside it being a dead local (`:146-165`).
    pub fn release(&mut self) -> bool {
        let held = self.sliders.iter().any(|slider| slider.dragging());
        for slider in &mut self.sliders {
            slider.release();
        }
        held
    }

    /// The screen's content draws: the centred title at `(w/2, 15)`, the
    /// nine sliders, then Done (`drawScreen`, then `super.drawScreen`).
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
            y: SOUND_TITLE_Y,
            scale: 1.0,
            colour: [1.0, 1.0, 1.0, 1.0],
            shadow: true,
            blend: true,
        }];
        for slider in &self.sliders {
            draws.extend(slider.draws(font, mouse));
        }
        draws.extend(self.done.draws(font, mouse));
        draws
    }
}
