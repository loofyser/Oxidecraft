//! The resource-packs screen's recorded no-op surface
//! (`GuiScreenResourcePacks.java`).
//!
//! The v1 rule is the spec's §16: the button exists and selecting does
//! nothing — real packs are post-v1. The surface keeps the title, the two
//! 200-wide lists (the selected list holding the default entry only), the
//! grey folder hint and both buttons, while omitting the move/markChanged
//! arm and the whole Done-apply arm (`:167-201`) exactly: entry clicks are
//! inert, `changed` is never set, and Done returns to the parent without
//! `setRepositories`/`saveOptions`/`refreshResources`. Open Folder has no
//! port-side opener contract, so it records its press and opens nothing.

use oxide_assets::font::Font;
use oxide_game::chat::LanguageTable;
use oxide_game::settings::GameSettings;
use oxide_render::hud::{HudDraw, ScaledResolution};
use oxide_render::text::string_width;

use super::options::SaveKind;
use super::widgets::Button;

/// Open Folder's button id (`initGui` builds it at 2).
pub const PACKS_OPEN_FOLDER_ID: i32 = 2;
/// Done's button id (the confirm button at 1).
pub const PACKS_DONE_ID: i32 = 1;
/// The title's y (`drawScreen` centres it at 16).
pub const PACKS_TITLE_Y: f32 = 16.0;
/// One list's width (both lists run 200 wide, `initGui`:69-74).
pub const PACKS_LIST_WIDTH: f32 = 200.0;
/// The lists' top: below the title row.
pub const PACKS_LIST_TOP: f32 = 32.0;
/// The lists' bottom inset (`height - 51`: `GuiResourcePackList.java:16`
/// opens both lists at top 32, bottom `height - 55 + 4`).
pub const PACKS_LIST_BOTTOM_INSET: f32 = 51.0;
/// The folder hint's left offset (`width / 2 - 77`, `:232`).
pub const PACKS_HINT_OFFSET: f32 = 77.0;
/// The folder hint's bottom inset (`height - 26`, `:232`).
pub const PACKS_HINT_INSET: f32 = 26.0;
/// The button row's bottom inset (`height - 48`, `:44-45`).
const PACKS_BUTTON_INSET: f32 = 48.0;

/// The default entry the selected list holds alone (the source appends its
/// `ResourcePackListEntryDefault` past the found entries, `:66`).
pub const PACKS_DEFAULT_ENTRY: &str = "Default";

/// The packs screen's labels: the title, the folder hint and the buttons.
#[derive(Debug, Clone)]
pub struct PacksLabels {
    /// The centred title (`resourcePack.title`).
    pub title: String,
    /// The grey folder hint (`resourcePack.folderInfo`).
    pub folder_info: String,
    /// Open Folder (`resourcePack.openFolder`).
    pub open_folder: String,
    /// Done (`gui.done`).
    pub done: String,
}

/// Resolves the packs screen's labels, each falling back to the store's
/// English (`drawScreen`, `initGui`).
pub fn packs_labels(language: Option<&LanguageTable>) -> PacksLabels {
    let text = |key: &str, fallback: &str| {
        language
            .and_then(|table| table.get(key))
            .unwrap_or(fallback)
            .to_string()
    };
    PacksLabels {
        title: text("resourcePack.title", "Select Resource Packs"),
        folder_info: text(
            "resourcePack.folderInfo",
            "(Place resource pack files here)",
        ),
        open_folder: text("resourcePack.openFolder", "Open resource pack folder"),
        done: text("gui.done", "Done"),
    }
}

/// One packs press: Open Folder records its press, Done answers the hub.
/// List entries answer nothing — the move/markChanged arm is omitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacksAction {
    /// Open Folder pressed: recorded, opens nothing (no opener contract
    /// port-side).
    OpenFolder,
    /// Done answers the hub without applying (`:200`'s bare return — the
    /// `:167-201` apply arm is absent by the §16 rule).
    Done,
}

impl PacksAction {
    /// Nothing the screen answers writes the store — the omitted apply arm
    /// is exactly the `saveOptions` the port never calls — so both arms
    /// ride the debounced mark, which persists identical bytes.
    pub fn save_kind(self) -> SaveKind {
        SaveKind::Deferred
    }
}

/// The resource-packs screen: the title, the available list (empty in v1 —
/// no packs load) and the selected list holding the default entry only,
/// the folder hint, and Open Folder + Done. Entry clicks are inert and
/// Done never applies.
#[derive(Debug, Clone)]
pub struct PacksScreen {
    /// Open Folder 2 at `w/2-154, h-48`.
    open_folder: Button,
    /// Done 1 at `w/2+4, h-48`.
    done: Button,
    /// The labels the screen draws.
    labels: PacksLabels,
    /// The scaled size the screen lays out at.
    width: u32,
    /// The scaled height the lists clip against.
    height: u32,
    /// Open Folder presses, recorded and never acted on.
    open_folder_presses: u32,
}

impl PacksScreen {
    /// Stands the packs screen at `scaled` with `labels`: the available
    /// list at `w/2-4-200`, the selected list at `w/2+4`, both 200 wide
    /// (`initGui`:69-74); Open Folder 2 and Done 1, 150-wide at `h-48`
    /// (`:44-45`); the title at y 16 and the grey hint at `w/2-77, h-26`
    /// (`drawScreen`:231-232).
    pub fn new(scaled: &ScaledResolution, labels: PacksLabels) -> Self {
        let half = scaled.width / 2;
        let row = (scaled.height - PACKS_BUTTON_INSET as u32) as f32;
        Self {
            open_folder: Button::new(
                PACKS_OPEN_FOLDER_ID,
                (half - 154) as f32,
                row,
                150.0,
                20.0,
                labels.open_folder.clone(),
            ),
            done: Button::new(
                PACKS_DONE_ID,
                (half + 4) as f32,
                row,
                150.0,
                20.0,
                labels.done.clone(),
            ),
            labels,
            width: scaled.width,
            height: scaled.height,
            open_folder_presses: 0,
        }
    }

    /// The title's y (`drawScreen`:231 centers `resourcePack.title` at 16).
    pub fn title_y(&self) -> f32 {
        PACKS_TITLE_Y
    }

    /// The folder hint's y (`drawScreen`:232 draws `resourcePack.folderInfo`
    /// at `h-26`).
    pub fn hint_y(&self) -> f32 {
        self.height as f32 - PACKS_HINT_INSET
    }

    /// The lists' top (`initGui`:69-74 opens both at 32).
    pub fn list_top(&self) -> f32 {
        PACKS_LIST_TOP
    }

    /// The lists' bottom (`GuiResourcePackList.java:16` closes both at
    /// `height - 55 + 4`).
    pub fn list_bottom(&self) -> f32 {
        self.height as f32 - PACKS_LIST_BOTTOM_INSET
    }

    /// The selected entries' count: the default entry only (no packs load
    /// in v1 — the source appends its default entry past the found ones,
    /// `:66`).
    pub fn selected_count(&self) -> usize {
        self.selected_entries().len()
    }

    /// Open Folder presses so far, recorded and never acted on (`:132-137`
    /// opens the folder port-side).
    pub fn open_folder_presses(&self) -> u32 {
        self.open_folder_presses
    }

    /// The available list's left (`width / 2 - 4 - 200`: the source halves
    /// in integers, so the half truncates before the subtraction).
    pub fn available_left(&self) -> f32 {
        (self.width / 2) as f32 - 4.0 - PACKS_LIST_WIDTH
    }

    /// The selected list's left (`width / 2 + 4`, integer half).
    pub fn selected_left(&self) -> f32 {
        (self.width / 2) as f32 + 4.0
    }

    /// The selected entries: the default entry only (no packs load in v1).
    pub fn selected_entries(&self) -> [&str; 1] {
        [PACKS_DEFAULT_ENTRY]
    }

    /// Open Folder's top-left (`w/2-154, h-48`).
    pub fn open_folder_pos(&self) -> (f32, f32) {
        (self.open_folder.x, self.open_folder.y)
    }

    /// Done's top-left (`w/2+4, h-48`).
    pub fn done_pos(&self) -> (f32, f32) {
        (self.done.x, self.done.y)
    }

    /// One press at the GUI-space point: Open Folder records its press,
    /// Done answers the hub. List entries are inert and empty space
    /// answers nothing. The press never touches `settings` — the apply
    /// arm's absence is the point.
    pub fn click_at(
        &mut self,
        point: (f32, f32),
        _settings: &mut GameSettings,
    ) -> Option<PacksAction> {
        if self.open_folder.pressed(point) {
            self.open_folder_presses += 1;
            return Some(PacksAction::OpenFolder);
        }
        if self.done.pressed(point) {
            return Some(PacksAction::Done);
        }
        None
    }

    /// The screen's content draws: the two lists (available empty, selected
    /// holding the default entry), the centred title at y 16, the grey
    /// folder hint at `w/2-77, h-26`, then Open Folder and Done
    /// (`drawScreen`, then `super.drawScreen`).
    pub fn draws(
        &self,
        font: &Font,
        scaled: &ScaledResolution,
        mouse: Option<(f32, f32)>,
    ) -> Vec<HudDraw> {
        let mut draws = Vec::new();
        let top = PACKS_LIST_TOP;
        let bottom = self.height as f32 - PACKS_LIST_BOTTOM_INSET;
        for left in [self.available_left(), self.selected_left()] {
            draws.push(HudDraw::Rect {
                x: left,
                y: top,
                width: PACKS_LIST_WIDTH,
                height: (bottom - top).max(0.0),
                colour: [0.0, 0.0, 0.0, 0.5],
            });
        }
        let width = string_width(font, PACKS_DEFAULT_ENTRY);
        draws.push(HudDraw::Text {
            text: PACKS_DEFAULT_ENTRY.to_string(),
            x: self.selected_left() + PACKS_LIST_WIDTH / 2.0 - width as f32 / 2.0,
            y: top + 4.0,
            scale: 1.0,
            colour: [1.0, 1.0, 1.0, 1.0],
            shadow: true,
            blend: true,
        });
        let width = string_width(font, &self.labels.title);
        draws.push(HudDraw::Text {
            text: self.labels.title.clone(),
            x: scaled.width as f32 / 2.0 - width as f32 / 2.0,
            y: PACKS_TITLE_Y,
            scale: 1.0,
            colour: [1.0, 1.0, 1.0, 1.0],
            shadow: true,
            blend: true,
        });
        draws.push(HudDraw::Text {
            text: self.labels.folder_info.clone(),
            x: scaled.width as f32 / 2.0 - PACKS_HINT_OFFSET,
            y: scaled.height as f32 - PACKS_HINT_INSET,
            scale: 1.0,
            colour: [128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 1.0],
            shadow: true,
            blend: true,
        });
        draws.extend(self.open_folder.draws(font, mouse));
        draws.extend(self.done.draws(font, mouse));
        draws
    }
}
