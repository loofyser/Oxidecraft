//! The creative inventory screen: the tab strip with its icons, the paged
//! item grid, the scrollbar, the search field, the hotbar row, the delete
//! slot, the pick behaviours and the C10 wire.
//!
//! The port reads values and names from the source's creative screen
//! (`GuiContainerCreative.java`), its container (`ContainerCreative`:852-941),
//! the tab registry (`creativetab/CreativeTabs.java`) and the
//! creative wire (`PlayerControllerMP.java`:534-574,
//! `C10PacketCreativeInventoryAction.java`, `CreativeCrafting.java`,
//! `NetHandlerPlayServer.java`:1074-1132); no source text is copied.
//!
//! The pinned rules, one per row of `refs/m5-homework/task-21-values.md`:
//! - Twelve tabs, indices 0–11: search is 5, the survival inventory 11.
//!   Two rows of six; the strip's column math, hitboxes and sheet slices are
//!   the source's own (`getTabColumn`, `func_147049_a`, `func_147051_a`).
//! - The frame is 195×136. The grid is a fixed 9×5 page at (9, 18); the
//!   player hotbar row rides at y 112; the delete slot is a plain 16×16
//!   cell at (173, 112) sharing the display's tmp index 0.
//! - The paging model is `scrollTo`: the ceiling row count, the
//!   round-to-nearest start row, trailing nulls (no clipping exists), the
//!   truncating wheel divisor with its zero quirk at sizes 46–53, and the
//!   scrollbar drag with no containment re-test while armed.
//! - The search field auto-focuses on entering the search tab (no
//!   click-to-focus path); every mouse click arms a first-keystroke clear;
//!   max length 15; the filter is the port's name-only [`search_matches`]
//!   over the pinned [`SEARCH_ITEMS`] (the recorded divergence: the source
//!   filters whole tooltips and appends enchanted-book NBT stacks, neither
//!   of which the port's tables carry).
//! - The click ladder: plain takes copy the cell AS-IS (usually size 1)
//!   and send nothing; shift upgrades to max; right merges ±1; number keys
//!   hotbar-swap a max-size copy; middle picks a max copy with no packet;
//!   Q drops a copy. NO C0E is ever sent from this screen: grid takes send
//!   nothing, hotbar-row clicks and inventory-tab mutations echo as C10,
//!   bin shift-clear sends the C10 nulls, drops send C10(−1).
//! - `selected_tab` is session-scoped client UI state (default 0,
//!   preserved across opens within the session): it lives in
//!   [`Screens`](super::Screens), never in `oxide-world`'s inventory.
//!
//! Recorded carries (not gaps in the pins):
//! - The inventory tab's shift-click (mode 1), drag (mode 5) and
//!   double-click gather (mode 6) are follow-ups: the `Container.slotClick`
//!   tails they need are Task 6's machine, which this task consumes but
//!   never invents. Modes 0/2/3/4 run here; anything else is a no-op.
//! - Number keys over the delete slot are a no-op (recorded): the bin is no
//!   player-container slot, so no swap target exists for it.
//! - The server's mid-session gamemode change (`ChangeGameState` reason 3)
//!   reports no client event, so the E-key redirect follows the Join/Respawn
//!   gamemode only; a `/gamemode` switch mid-session re-routes on the next
//!   login. The world pick-block path (middle-click in the world while
//!   creative) is out of scope: the port carries no gameplay pick-block
//!   routing yet.
//! - Titles and tab hovers draw the untranslated `itemGroup.<label>` key
//!   and the bin shows `inventory.binSlot`: key resolution is a
//!   locale-table concern the port does not carry.
//! - The close carries C0D with window id 0 like every other close: the
//!   creative screen stands on the player's own inventory window.
//!
//! [`search_matches`]: crate::items::search_matches
//! [`SEARCH_ITEMS`]: crate::items::creative_tab_items

use oxide_game::container::{
    CLICK_MODE_CREATIVE_PICK, CLICK_MODE_DRAG, CLICK_MODE_DROP, CLICK_MODE_GATHER,
    CLICK_MODE_PICKUP, CLICK_MODE_QUICK_MOVE, LocalContainer, StackCaps, drag_button,
    max_stack_size,
};
use oxide_game::input::{InputEvent, Key};
use oxide_proto_v47::entity::MetadataItem;

use super::container::{CURSOR_OFFSET, ClickButton, DOUBLE_CLICK_MS, ScreenKey};
use crate::items::{
    CreativeTab, ItemTable, TabEntry, creative_tab_items, search_matches, stack_name,
};

/// The frame width (`GuiContainerCreative.java`:65 — `xSize = 195`).
pub const FRAME_W: i32 = 195;
/// The frame height (`:66` — `ySize = 136`).
pub const FRAME_H: i32 = 136;

/// The grid's left edge: 45 display slots at (9+j·18, 18+i·18)
/// (`ContainerCreative`:860-866).
pub const GRID_LEFT: i32 = 9;
/// The grid's top edge.
pub const GRID_TOP: i32 = 18;
/// The grid's columns: a fixed 9×5 page.
pub const GRID_COLS: i32 = 9;
/// The grid's rows.
pub const GRID_ROWS: i32 = 5;
/// One grid cell's count: the page holds 45 display stacks.
pub const PAGE_CELLS: usize = 45;
/// One cell's step, in panel units.
pub const CELL_STEP: i32 = 18;

/// The hotbar row's top: nine player slots at (9+k·18, 112) (`:868-871`).
pub const HOTBAR_TOP: i32 = 112;
/// The hotbar row's count.
pub const HOTBAR_CELLS: usize = 9;

/// The tab strip's columns: two rows of six (`getTabColumn`, `:194-197`).
pub const TAB_COLS: u8 = 6;
/// One tab's width in the hitbox and on the sheet (`func_147049_a`,
/// `func_147051_a` — 28 wide × 32 high).
pub const TAB_W: i32 = 28;
/// One tab's height.
pub const TAB_H: i32 = 32;
/// The search tab's index (`tabAllSearch`, `CreativeTabs.java`:55-61).
pub const SEARCH_TAB: u8 = 5;
/// The survival-inventory tab's index (`tabInventory`, `:97-103`).
pub const INVENTORY_TAB: u8 = 11;
/// The tab count (`creativeTabArray = new CreativeTabs[12]`, `:15`).
pub const TAB_COUNT: u8 = 12;

/// The first row's top, in panel units: 32 above the frame (`y = −32`,
/// `func_147049_a`:714-739).
pub const TAB_TOP_DY: i32 = -32;
/// The second row's top: the frame height below the origin (`+ySize`).
/// The draw reads `guiTop + (ySize − 4)` for the sprite (`func_147051_a`);
/// the hitbox reads `+ySize`.
pub const TAB_BOTTOM_HIT_DY: i32 = FRAME_H;
/// The draw's second-row top (`i1 = guiTop + (ySize − 4)`, `:780-829`).
pub const TAB_BOTTOM_DRAW_DY: i32 = FRAME_H - 4;
/// The last column's hitbox nudge (`xSize − 28 + 2` when column == 5,
/// `:714-739`).
pub const TAB_LAST_HIT_DX: i32 = FRAME_W - TAB_W + 2;
/// The last column's draw nudge (`guiLeft + xSize − 28` when column == 5).
pub const TAB_LAST_DRAW_DX: i32 = FRAME_W - TAB_W;
/// The selected tab's sheet row (`v + 32` when selected, `:780-829`).
pub const TAB_SELECTED_DV: i32 = 32;
/// The second row's sheet row (`v + 64` when in the second row).
pub const TAB_ROW_DV: i32 = 64;
/// The tab icon's inset from the tab sprite's left (`l + 6`, `:817-828`).
pub const TAB_ICON_DX: i32 = 6;
/// The tab icon's drop on the first row (`i1 + 8 + 1`).
pub const TAB_ICON_TOP_DY: i32 = 9;
/// The tab icon's drop on the second row (`i1 + 8 − 1`).
pub const TAB_ICON_BOTTOM_DY: i32 = 7;

/// The scrollbar track's left (`guiLeft + 175`, `drawScreen`:574-601).
pub const TRACK_DX: i32 = 175;
/// The track's width (`i1 = k + 14`).
pub const TRACK_W: i32 = 14;
/// The track's top (`guiTop + 18`, `l = j + 18`).
pub const TRACK_DY: i32 = 18;
/// The track's height (`j1 = l + 112`).
pub const TRACK_H: i32 = 112;
/// The thumb's width and height (12×15, `:696-704`).
pub const THUMB_W: i32 = 12;
/// See [`THUMB_W`].
pub const THUMB_H: i32 = 15;
/// The thumb's left: centred in the 14-wide track.
pub const THUMB_DX: i32 = TRACK_DX + 1;
/// The thumb travel: `(k − j − 17)` = 112 − 17 = 95 (`:696-704`).
pub const THUMB_SPAN: i32 = TRACK_H - 17;
/// The thumb's sheet x while the list scrolls (`needsScrollBars`, 232).
pub const THUMB_SHEET_X: i32 = 232;
/// The thumb's sheet x while the list fits (244 — the disabled slice).
pub const THUMB_IDLE_SHEET_X: i32 = 244;
/// The thumb's sheet y (0).
pub const THUMB_SHEET_Y: i32 = 0;
/// The drag's pointer offset (`mouseY − l − 7.5`, `:574-601`).
pub const SCROLL_DRAG_OFFSET: f32 = 7.5;
/// The drag's divisor (`(j1 − l) − 15` = 112 − 15 = 97).
pub const SCROLL_DRAG_SPAN: f32 = 97.0;

/// The search field's left (`guiLeft + 82`, `initGui`:263-280).
pub const SEARCH_DX: i32 = 82;
/// The search field's top (`guiTop + 6`).
pub const SEARCH_DY: i32 = 6;
/// The search field's width (89).
pub const SEARCH_W: i32 = 89;
/// The search field's max length (15, `:271`).
pub const SEARCH_MAX: usize = 15;
/// The search text's packed white (16777215, `:274`).
pub const SEARCH_COLOUR: u32 = 16_777_215;

/// The title's left and top (`(8, 6)`, `:392-400`).
pub const TITLE_DX: i32 = 8;
/// See [`TITLE_DX`].
pub const TITLE_DY: i32 = 6;
/// The title's packed grey (4210752 = 0x404040).
pub const TITLE_GREY: u32 = 4_210_752;

/// The delete slot's left (173, `setCurrentCreativeTab`:512-513).
pub const BIN_DX: i32 = 173;
/// The delete slot's top (112).
pub const BIN_DY: i32 = HOTBAR_TOP;
/// The delete slot's side (a plain 16×16 slot).
pub const BIN_SIZE: i32 = 16;
/// The bin shift-clear's wire span: the source loops `0..size` over the
/// 46-slot player container list, and the server honours only 1–44 — wire
/// slots 0 and 45 are silent no-ops (`:117-123`, `:1074-1132`). The port
/// sends exactly what the source sends.
pub const BIN_CLEAR_WIRES: i16 = 46;

/// The inventory tab's armour block: 2×2 at (`9 + (j−5)/2·54`,
/// `6 + (j−5)%2·27`) for j = 5..8 (`:481-509`).
pub const ARMOUR_LEFT: i32 = 9;
/// See [`ARMOUR_LEFT`].
pub const ARMOUR_TOP: i32 = 6;
/// The armour block's column step (54).
pub const ARMOUR_DX: i32 = 54;
/// The armour block's row step (27).
pub const ARMOUR_DY: i32 = 27;
/// The inventory tab's main block top (`54 + (j−9)/9·18` for j = 9..35).
pub const INV_MAIN_TOP: i32 = 54;
/// The parked crafting slots' coordinate (−2000, `:481-509`).
pub const OFFSCREEN_SLOT: i32 = -2000;
/// The player copies' slot count on the inventory tab (0..44 verbatim).
pub const PLAYER_SLOTS: usize = 45;

/// The tabs strip sheet, under the port's named-texture key
/// (`…/creative_inventory/tabs.png`, `:38`, `:684`, `:699`).
pub const TABS_SHEET: &str = "gui/container/creative_inventory/tabs";

/// The panel sheet for a tab, under the port's named-texture key
/// (`…/creative_inventory/tab_<name>`, `:692-693`).
pub fn panel_sheet(tab: CreativeTab) -> &'static str {
    match tab {
        CreativeTab::BuildingBlocks => "gui/container/creative_inventory/tab_buildingBlocks",
        CreativeTab::Decorations => "gui/container/creative_inventory/tab_decorations",
        CreativeTab::Redstone => "gui/container/creative_inventory/tab_redstone",
        CreativeTab::Transportation => "gui/container/creative_inventory/tab_transportation",
        CreativeTab::Misc => "gui/container/creative_inventory/tab_misc",
        CreativeTab::Search => "gui/container/creative_inventory/tab_search",
        CreativeTab::Food => "gui/container/creative_inventory/tab_food",
        CreativeTab::Tools => "gui/container/creative_inventory/tab_tools",
        CreativeTab::Combat => "gui/container/creative_inventory/tab_combat",
        CreativeTab::Brewing => "gui/container/creative_inventory/tab_brewing",
        CreativeTab::Materials => "gui/container/creative_inventory/tab_materials",
        CreativeTab::Inventory => "gui/container/creative_inventory/tab_inventory",
    }
}

/// A tab's column (`getTabColumn = tabIndex % 6`, `:194-197`).
pub fn tab_column(index: u8) -> u8 {
    index % TAB_COLS
}

/// Whether the tab sits in the first row (`tabIndex < 6`, `:202-205`).
pub fn tab_first_row(index: u8) -> bool {
    index < TAB_COLS
}

/// A tab's hitbox, panel-relative: left, top, 28×32 (`func_147049_a`).
pub fn tab_hit(index: u8) -> (i32, i32) {
    let col = tab_column(index);
    let x = if col == TAB_COLS - 1 {
        TAB_LAST_HIT_DX
    } else if col > 0 {
        TAB_W * i32::from(col) + i32::from(col)
    } else {
        0
    };
    let y = if tab_first_row(index) {
        TAB_TOP_DY
    } else {
        TAB_BOTTOM_HIT_DY
    };
    (x, y)
}

/// A tab's sprite's top-left, panel-relative (`func_147051_a`).
pub fn tab_sprite(index: u8) -> (i32, i32) {
    let col = tab_column(index);
    let x = if col == TAB_COLS - 1 {
        TAB_LAST_DRAW_DX
    } else if col > 0 {
        TAB_W * i32::from(col) + i32::from(col)
    } else {
        0
    };
    let y = if tab_first_row(index) {
        TAB_TOP_DY + 4
    } else {
        TAB_BOTTOM_DRAW_DY
    };
    (x, y)
}

/// A tab's sheet slice: (u, v) at 28×32 (`u = column·28`, `v = 0/+32/+64`).
pub fn tab_uv(index: u8, selected: bool) -> (i32, i32) {
    let u = TAB_W * i32::from(tab_column(index));
    let mut v = if tab_first_row(index) { 0 } else { TAB_ROW_DV };
    if selected {
        v += TAB_SELECTED_DV;
    }
    (u, v)
}

/// A tab's icon draw position, panel-relative (`l + 6`, `i1 + 8 ± 1`).
pub fn tab_icon_pos(index: u8) -> (i32, i32) {
    let (x, y) = tab_sprite(index);
    let dy = if tab_first_row(index) {
        TAB_ICON_TOP_DY
    } else {
        TAB_ICON_BOTTOM_DY
    };
    (x + TAB_ICON_DX, y + dy)
}

/// Whether the tab hides the scrollbar and the title: only the inventory
/// tab does (`setNoScrollbar`/`setNoTitle`, `:103`; `hasScrollbar`,
/// `drawInForegroundOfTab`).
pub fn tab_hides_chrome(index: u8) -> bool {
    index == INVENTORY_TAB
}

/// The title line a tab draws, or `None` on the inventory tab
/// (`itemGroup.<label>` at (8, 6), `:392-400`). The key is untranslated:
/// resolution is a locale-table concern the port does not carry.
pub fn tab_title(index: u8) -> Option<String> {
    if tab_hides_chrome(index) {
        return None;
    }
    CreativeTab::from_index(index).map(|tab| format!("itemGroup.{}", tab.label()))
}

/// The scrolled row count: `(size + 8)/9 − 5`, the ceiling form
/// (`scrollTo`:881-907).
pub fn page_rows(size: usize) -> i32 {
    (size as i32 + 8) / GRID_COLS - GRID_ROWS
}

/// The first displayed row: `(int)(p·rows + 0.5)`, floored at 0 with no
/// upper clamp (the callers clamp `p` to 0..1).
pub fn start_row(scroll: f32, rows: i32) -> i32 {
    ((scroll * rows as f32 + 0.5) as i32).max(0)
}

/// One wheel step: ignored unless the list scrolls; the divisor is the
/// TRUNCATING `size/9 − 5` — zero at sizes 46–53, so ±Infinity slams the
/// clamp to an end (`handleMouseInput`:546-569). The notch clamps to ±1.
pub fn wheel_scroll(scroll: f32, size: usize, notch: f32) -> f32 {
    if size <= PAGE_CELLS {
        return scroll;
    }
    let divisor = (size / GRID_COLS as usize) as f32 - GRID_ROWS as f32;
    (scroll - notch.clamp(-1.0, 1.0) / divisor).clamp(0.0, 1.0)
}

/// The drag's scroll for a panel-local pointer y: `(y − 18 − 7.5)/97`,
/// clamped 0..1 (`drawScreen`:574-601).
pub fn drag_scroll(pointer_y: f32) -> f32 {
    ((pointer_y - TRACK_DY as f32 - SCROLL_DRAG_OFFSET) / SCROLL_DRAG_SPAN).clamp(0.0, 1.0)
}

/// The thumb's top-left, panel-relative, or `None` on the inventory tab:
/// `(176, 18 + 95·scroll)`, 12×15 (`:696-704`).
pub fn thumb_rect(scroll: f32, index: u8) -> Option<(i32, i32)> {
    if tab_hides_chrome(index) {
        return None;
    }
    Some((
        THUMB_DX,
        TRACK_DY + (THUMB_SPAN as f32 * scroll.clamp(0.0, 1.0)) as i32,
    ))
}

/// The thumb's sheet x: 232 while the list scrolls, 244 while it fits.
pub fn thumb_sheet_x(size: usize, index: u8) -> i32 {
    if !tab_hides_chrome(index) && size > PAGE_CELLS {
        THUMB_SHEET_X
    } else {
        THUMB_IDLE_SHEET_X
    }
}

/// The inventory tab's slot position for a player-container index:
/// crafting 0–4 parked offscreen, armour 5–8 in the 2×2 block, main 9–35,
/// hotbar 36–44 at y 112 (`setCurrentCreativeTab`:481-509).
pub fn inventory_slot_pos(slot: i16) -> (i32, i32) {
    if (5..=8).contains(&slot) {
        let armour = slot - 5;
        (
            ARMOUR_LEFT + (armour / 2) as i32 * ARMOUR_DX,
            ARMOUR_TOP + (armour % 2) as i32 * ARMOUR_DY,
        )
    } else if (9..=35).contains(&slot) {
        let main = slot - 9;
        (
            ARMOUR_LEFT + (main % GRID_COLS as i16) as i32 * CELL_STEP,
            INV_MAIN_TOP + (main / GRID_COLS as i16) as i32 * CELL_STEP,
        )
    } else if (36..=44).contains(&slot) {
        (GRID_LEFT + (slot - 36) as i32 * CELL_STEP, HOTBAR_TOP)
    } else {
        (OFFSCREEN_SLOT, OFFSCREEN_SLOT)
    }
}

/// One creative list entry's display stack: count 1, the entry's damage,
/// no tag (the `getSubItems` sizes the RED step pins as 1).
pub fn entry_stack(entry: TabEntry) -> MetadataItem {
    MetadataItem {
        id: entry.id,
        count: 1,
        damage: entry.damage,
        nbt: None,
    }
}

/// Which region of the screen the panel-local point hovers, in hit-test
/// order: the tab strip first (it swallows the click, `:406-423`), then
/// the bin, the grid, the hotbar row or the inventory tab's player slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hover {
    /// A tab-strip tab, by tab index.
    Tab(u8),
    /// The scrollbar track.
    Track,
    /// A grid display cell, 0..45.
    Grid(usize),
    /// A hotbar-row cell, 0..9.
    Hotbar(usize),
    /// A player-container slot on the inventory tab, by wire index.
    Player(i16),
    /// The delete slot.
    Bin,
    /// Inside the panel over no slot: swallowed, never a drop.
    Panel,
    /// Nothing clickable.
    None,
}

/// Whether the point sits in the panel-local rect.
fn in_rect(x: f32, y: f32, rx: i32, ry: i32, w: i32, h: i32) -> bool {
    x >= rx as f32 && y >= ry as f32 && x < (rx + w) as f32 && y < (ry + h) as f32
}

/// The creative screen's own state: the selected tab, the scroll offset,
/// the search field, the view's cursor and slot copies, and the pointer.
#[derive(Debug, Clone)]
pub struct CreativeScreen {
    /// The selected tab, 0..12 (the source's static, default 0).
    selected_tab: u8,
    /// The scroll offset, 0..1 (`currentScroll`).
    scroll: f32,
    /// The search query.
    search: String,
    /// The search field's cursor byte index, always on a boundary.
    search_cursor: usize,
    /// Whether the search field shows (on the search tab only).
    search_visible: bool,
    /// Whether the search field holds focus (auto-set on entering).
    search_focused: bool,
    /// Whether the next keystroke clears the field first (armed by every
    /// mouse click, `field_147057_D`).
    clear_armed: bool,
    /// Whether a scrollbar drag is armed (`isScrolling`).
    scrolling: bool,
    /// Whether the left button was down on the last frame (`wasClicking`).
    was_clicking: bool,
    /// The view's carried stack (takes mutate it with no packet).
    cursor: Option<MetadataItem>,
    /// The 45 grid display cells.
    grid: [Option<MetadataItem>; PAGE_CELLS],
    /// The hotbar row's nine player copies.
    hotbar: [Option<MetadataItem>; HOTBAR_CELLS],
    /// The inventory tab's player-container copies, wire order 0..44.
    player: [Option<MetadataItem>; PLAYER_SLOTS],
    /// The current ordered list (the tab's list, filtered on search).
    list: Vec<TabEntry>,
    /// The pointer in panel-local units.
    mouse: (f32, f32),
    /// The panel's top-left in screen units.
    origin: (i32, i32),
    /// The pending tab a left press swallowed, switched on release.
    pending_tab: Option<u8>,
    /// A press-with-cursor drag's covered hotbar container indices
    /// (45..54), or `None` while no drag is armed.
    drag_covered: Option<Vec<i16>>,
    /// The drag's raw button, for the different-button cancel.
    drag_button: i8,
    /// Whether the last press was swallowed (tab, track or resolved
    /// click): the release drops it.
    ignore_release: bool,
    /// Whether the last press qualifies for the double-click gather.
    double_click: bool,
    /// The last press's region, raw button and time, for the 250 ms rule.
    last: Option<(Hover, i8, u64)>,
}

impl CreativeScreen {
    /// Opens the screen on the tab: scroll reset, grid filled, the search
    /// field auto-focused on the search tab only (`setCurrentCreativeTab`).
    pub fn new(selected_tab: u8) -> Self {
        let mut screen = Self {
            selected_tab: if selected_tab < TAB_COUNT {
                selected_tab
            } else {
                0
            },
            scroll: 0.0,
            search: String::new(),
            search_cursor: 0,
            search_visible: false,
            search_focused: false,
            clear_armed: false,
            scrolling: false,
            was_clicking: false,
            cursor: None,
            grid: [const { None }; PAGE_CELLS],
            hotbar: [const { None }; HOTBAR_CELLS],
            player: [const { None }; PLAYER_SLOTS],
            list: Vec::new(),
            mouse: (0.0, 0.0),
            origin: (0, 0),
            pending_tab: None,
            drag_covered: None,
            drag_button: 0,
            ignore_release: false,
            double_click: false,
            last: None,
        };
        let tab = screen.selected_tab;
        screen.search_visible = tab == SEARCH_TAB;
        screen.search_focused = tab == SEARCH_TAB;
        screen.rebuild_list();
        screen.refresh_grid();
        screen
    }

    /// The selected tab.
    pub fn selected_tab(&self) -> u8 {
        self.selected_tab
    }

    /// The scroll offset, 0..1.
    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    /// The search query.
    pub fn search_text(&self) -> &str {
        &self.search
    }

    /// Whether the search field shows.
    pub fn search_visible(&self) -> bool {
        self.search_visible
    }

    /// Whether the search field holds focus.
    pub fn search_focused(&self) -> bool {
        self.search_focused
    }

    /// The view's carried stack.
    pub fn cursor(&self) -> Option<&MetadataItem> {
        self.cursor.as_ref()
    }

    /// The grid display cells.
    pub fn grid(&self) -> &[Option<MetadataItem>; PAGE_CELLS] {
        &self.grid
    }

    /// The hotbar row's copies.
    pub fn hotbar(&self) -> &[Option<MetadataItem>; HOTBAR_CELLS] {
        &self.hotbar
    }

    /// One player-container copy, by wire index.
    pub fn player_slot(&self, slot: i16) -> Option<&Option<MetadataItem>> {
        usize::try_from(slot).ok().and_then(|i| self.player.get(i))
    }

    /// The panel's top-left in screen units.
    pub fn origin(&self) -> (i32, i32) {
        self.origin
    }

    /// Centres the panel (`guiLeft/guiTop`).
    pub fn set_screen_size(&mut self, width: i32, height: i32) {
        self.origin = ((width - FRAME_W) / 2, (height - FRAME_H) / 2);
    }

    /// Whether the tab's list scrolls (`needsScrollBars`:451-454 — off the
    /// inventory tab with strictly more than one page, `func_148328_e`).
    pub fn needs_scrollbars(&self) -> bool {
        !tab_hides_chrome(self.selected_tab) && self.list.len() > PAGE_CELLS
    }

    /// The tab's registry entry.
    fn tab(&self) -> CreativeTab {
        CreativeTab::from_index(self.selected_tab).unwrap_or(CreativeTab::BuildingBlocks)
    }

    /// Rebuilds the ordered list: the tab's pinned list, filtered by the
    /// query on the search tab (`updateCreativeSearch`:341-387, through the
    /// port's name-only [`search_matches`]).
    fn rebuild_list(&mut self) {
        if self.selected_tab == SEARCH_TAB {
            let query = self.search.clone();
            self.list = creative_tab_items(CreativeTab::Search)
                .iter()
                .copied()
                .filter(|entry| {
                    let name = stack_name(entry.id, entry.damage)
                        .or_else(|| crate::items::item_entry(entry.id).map(|row| row.name))
                        .unwrap_or("");
                    search_matches(name, &query)
                })
                .collect();
        } else {
            self.list = creative_tab_items(self.tab()).to_vec();
        }
    }

    /// Refills the 45 display cells from the list at the scroll offset:
    /// cell (l, k) shows `list[l + (k + start)·9]`, null past the end
    /// (`scrollTo`:881-907 — no clipping exists).
    fn refresh_grid(&mut self) {
        let start = start_row(self.scroll, page_rows(self.list.len())) as usize;
        for i in 0..PAGE_CELLS {
            let (l, k) = (i % GRID_COLS as usize, i / GRID_COLS as usize);
            self.grid[i] = self
                .list
                .get(l + (k + start) * GRID_COLS as usize)
                .map(|entry| entry_stack(*entry));
        }
    }

    /// The shared tab-switch body: scroll reset, grid refill, the search
    /// field's auto-focus/clear rules (`setCurrentCreativeTab`:521-540).
    fn switch_to(&mut self, index: u8) {
        self.selected_tab = index;
        self.scroll = 0.0;
        if index == SEARCH_TAB {
            self.search.clear();
            self.search_cursor = 0;
            self.search_visible = true;
            self.search_focused = true;
        } else {
            self.search_visible = false;
            self.search_focused = false;
        }
        self.scrolling = false;
        self.pending_tab = None;
        self.drag_covered = None;
        self.ignore_release = false;
        self.double_click = false;
        self.rebuild_list();
        self.refresh_grid();
    }

    /// Switches tabs: scroll reset, grid refill, the search field's
    /// auto-focus/clear rules (`setCurrentCreativeTab`:521-540).
    pub fn set_tab(&mut self, index: u8) {
        if index < TAB_COUNT {
            self.switch_to(index);
        }
    }

    /// All regions under the panel-local point, in hit order: the tab
    /// strip first (it swallows the click, `:406-423`), then the tab's
    /// own regions with the bin last (it is appended last, `:512-513`).
    fn hit_all(&self, x: f32, y: f32) -> Vec<Hover> {
        let mut out = Vec::new();
        for index in 0..TAB_COUNT {
            let (hx, hy) = tab_hit(index);
            if in_rect(x, y, hx, hy, TAB_W, TAB_H) {
                out.push(Hover::Tab(index));
                break;
            }
        }
        if self.selected_tab == INVENTORY_TAB {
            for slot in 5..=44 {
                let (sx, sy) = inventory_slot_pos(slot);
                if in_rect(x, y, sx, sy, CELL_STEP - 2, CELL_STEP - 2) {
                    out.push(Hover::Player(slot));
                }
            }
            if in_rect(x, y, BIN_DX, BIN_DY, BIN_SIZE, BIN_SIZE) {
                out.push(Hover::Bin);
            }
        } else {
            if y >= GRID_TOP as f32
                && y < (GRID_TOP + GRID_ROWS * CELL_STEP) as f32
                && x >= GRID_LEFT as f32
                && x < (GRID_LEFT + GRID_COLS * CELL_STEP) as f32
            {
                let col = ((x - GRID_LEFT as f32) / CELL_STEP as f32) as usize;
                let row = ((y - GRID_TOP as f32) / CELL_STEP as f32) as usize;
                let cell = row * GRID_COLS as usize + col;
                let (cx, cy) = (
                    GRID_LEFT + col as i32 * CELL_STEP,
                    GRID_TOP + row as i32 * CELL_STEP,
                );
                if in_rect(x, y, cx, cy, CELL_STEP - 2, CELL_STEP - 2) {
                    out.push(Hover::Grid(cell));
                } else {
                    out.push(Hover::Panel);
                }
            }
            if y >= HOTBAR_TOP as f32
                && y < (HOTBAR_TOP + CELL_STEP) as f32
                && x >= GRID_LEFT as f32
                && x < (GRID_LEFT + GRID_COLS * CELL_STEP) as f32
            {
                let col = ((x - GRID_LEFT as f32) / CELL_STEP as f32) as usize;
                let (cx, cy) = (GRID_LEFT + col as i32 * CELL_STEP, HOTBAR_TOP);
                if in_rect(x, y, cx, cy, CELL_STEP - 2, CELL_STEP - 2) {
                    out.push(Hover::Hotbar(col));
                } else {
                    out.push(Hover::Panel);
                }
            }
            if in_rect(x, y, BIN_DX, BIN_DY, BIN_SIZE, BIN_SIZE) {
                // The quirk's own rect: the bin shares tmp index 0 with
                // grid cell (0, 0) (`:512`), so a plain click here runs the
                // take ladder on the first displayed item.
                out.push(Hover::Bin);
            }
            if x >= TRACK_DX as f32
                && x < (TRACK_DX + TRACK_W) as f32
                && y >= TRACK_DY as f32
                && y < (TRACK_DY + TRACK_H) as f32
            {
                out.push(Hover::Track);
            }
        }
        if out.is_empty() {
            if x >= 0.0 && y >= 0.0 && x < FRAME_W as f32 && y < FRAME_H as f32 {
                out.push(Hover::Panel);
            } else {
                out.push(Hover::None);
            }
        }
        out
    }

    /// Hit-tests the panel-local point, first match (the click's slot).
    pub fn click_at(&self, x: f32, y: f32) -> Hover {
        self.hit_all(x, y).into_iter().next().unwrap_or(Hover::None)
    }

    /// Hit-tests the panel-local point.
    pub fn hover_at(&self, x: f32, y: f32) -> Hover {
        self.hit_all(x, y).into_iter().next().unwrap_or(Hover::None)
    }

    /// The hover the draw loop's highlight reads: the last match.
    fn hover_last(&self, x: f32, y: f32) -> Hover {
        self.hit_all(x, y).into_iter().last().unwrap_or(Hover::None)
    }

    /// Moves the pointer: hover refresh, the armed scrollbar drag (no
    /// containment re-test) and the drag-split cover over hotbar slots.
    pub fn mouse_moved(&mut self, x: f32, y: f32) {
        self.mouse = (x, y);
        if self.scrolling {
            if self.needs_scrollbars() {
                self.scroll = drag_scroll(y);
                self.refresh_grid();
            } else {
                self.scrolling = false;
            }
        }
        let hover = self.hover_last(x, y);
        if let Some(covered) = self.drag_covered.as_mut() {
            if let Hover::Hotbar(k) = hover {
                let index = PAGE_CELLS as i16 + k as i16;
                if !covered.contains(&index) {
                    covered.push(index);
                }
            }
        }
    }

    /// Presses a button: the tab swallow, the track arm, the outside drop,
    /// the grid takes and the hotbar prediction (`mouseClicked`:359-460
    /// through `handleMouseClick`:85-246).
    pub fn press(
        &mut self,
        button: ClickButton,
        shift: bool,
        caps: &impl StackCaps,
        now_ms: u64,
    ) -> Vec<InputEvent> {
        // Every mouse click arms the search's first-keystroke clear
        // (`field_147057_D`, `handleMouseClick`:87).
        self.clear_armed = true;
        let hover = self.click_at(self.mouse.0, self.mouse.1);
        self.double_click = self.last.is_some_and(|(region, raw, time)| {
            region == hover && raw == button.raw() && now_ms.wrapping_sub(time) < DOUBLE_CLICK_MS
        });
        self.last = Some((hover, button.raw(), now_ms));
        match hover {
            Hover::Tab(index) => {
                if button == ClickButton::Left {
                    self.pending_tab = Some(index);
                    self.was_clicking = true;
                }
                self.ignore_release = true;
                Vec::new()
            }
            Hover::Track => {
                if button == ClickButton::Left {
                    self.was_clicking = true;
                    if self.needs_scrollbars() {
                        self.scrolling = true;
                        self.scroll = drag_scroll(self.mouse.1);
                        self.refresh_grid();
                    }
                }
                self.ignore_release = true;
                Vec::new()
            }
            Hover::Panel => {
                self.ignore_release = true;
                Vec::new()
            }
            Hover::None => {
                let mut out = Vec::new();
                if self.cursor.is_some() && button != ClickButton::Pick {
                    out.extend(self.outside_drop(button));
                }
                self.ignore_release = true;
                out
            }
            Hover::Bin => {
                let out = self.click_bin(button, shift, caps);
                self.ignore_release = true;
                out
            }
            Hover::Grid(cell) => {
                if button == ClickButton::Pick {
                    if self.cursor.is_none() {
                        if let Some(stack) = self.grid[cell].clone() {
                            self.cursor = Some(max_copy(&stack, caps));
                        }
                    } else {
                        self.arm_drag(button);
                        return Vec::new();
                    }
                } else {
                    self.take(cell, shift, button, caps);
                }
                self.ignore_release = true;
                Vec::new()
            }
            Hover::Hotbar(k) => {
                if self.cursor.is_some() {
                    self.arm_drag(button);
                    return Vec::new();
                }
                let mode = if button == ClickButton::Pick {
                    CLICK_MODE_CREATIVE_PICK
                } else if shift {
                    CLICK_MODE_QUICK_MOVE
                } else {
                    CLICK_MODE_PICKUP
                };
                let out = self.run_hotbar_click(k, button.raw(), mode, caps);
                self.ignore_release = true;
                out
            }
            Hover::Player(slot) => {
                // Modes 1/5/6 on the player container are follow-ups (Task
                // 6's tails); modes 0/2/3 run through the shared pickup,
                // swap and pick branches with the diff echo.
                if shift {
                    self.ignore_release = true;
                    return Vec::new();
                }
                let out = self.click_player(slot, button, caps);
                self.ignore_release = true;
                out
            }
        }
    }

    /// Releases a button: the pending tab switch, the drag batch echo and
    /// the swallowed-press drop (`mouseReleased`:515-652).
    pub fn release(
        &mut self,
        button: ClickButton,
        shift: bool,
        caps: &impl StackCaps,
        now_ms: u64,
    ) -> Vec<InputEvent> {
        let _ = now_ms;
        let hover = self.click_at(self.mouse.0, self.mouse.1);
        if button == ClickButton::Left {
            self.scrolling = false;
            self.was_clicking = false;
        }
        if let Some(index) = self.pending_tab.take() {
            if button == ClickButton::Left && hover == Hover::Tab(index) {
                self.set_tab(index);
            }
            return Vec::new();
        }
        if self.ignore_release {
            self.ignore_release = false;
            return Vec::new();
        }
        let Some(covered) = self.drag_covered.take() else {
            self.double_click = false;
            if self.cursor.is_none() {
                self.last = None;
            }
            return Vec::new();
        };
        if button.raw() != self.drag_button {
            // A release of another button than the drag's cancels it.
            self.double_click = false;
            return Vec::new();
        }
        if !covered.is_empty() {
            self.double_click = false;
            let out = self.finish_hotbar_drag(&covered, i32::from(self.drag_button), caps);
            if self.cursor.is_none() {
                self.last = None;
            }
            return out;
        }
        if self.double_click && button == ClickButton::Left {
            self.double_click = false;
            if let Hover::Hotbar(k) = hover {
                // The double-click gather merges from the hotbar band only
                // (`canMergeSlot`:933-936); grid cells fall through the
                // plain ladder (FLAG-M1).
                let before = self.hotbar.clone();
                let mut container = self.scratch_grid();
                container.slot_click(
                    PAGE_CELLS as i16 + k as i16,
                    button.raw(),
                    CLICK_MODE_GATHER,
                    &ItemTable,
                );
                self.write_back_grid(&container);
                let out = diff_band(&before, &self.hotbar, 36);
                if self.cursor.is_none() {
                    self.last = None;
                }
                return out;
            }
        }
        self.double_click = false;
        let out = self.replay_single(hover, button, shift, caps);
        if self.cursor.is_none() {
            self.last = None;
        }
        out
    }

    /// Types a screen key: number-key swaps (before text), the drop key
    /// and the pick key (`keyTyped`:692-712, `checkHotbarKeys`:718-733).
    pub fn screen_key(&mut self, key: ScreenKey, ctrl: bool) -> Vec<InputEvent> {
        match key {
            ScreenKey::Number(index) => self.number_key(index, &ItemTable).unwrap_or_default(),
            ScreenKey::Drop => {
                let hover = self.hover_last(self.mouse.0, self.mouse.1);
                self.drop_key(hover, ctrl, &ItemTable)
            }
            ScreenKey::Pick => {
                let hover = self.hover_last(self.mouse.0, self.mouse.1);
                self.pick_key(hover, &ItemTable)
            }
        }
    }

    /// The chat key off the search tab jumps straight there (`keyTyped`
    /// :308-313). Answers whether it jumped.
    pub fn chat_key(&mut self) -> bool {
        if self.selected_tab == SEARCH_TAB {
            return false;
        }
        self.set_tab(SEARCH_TAB);
        true
    }

    /// One editing key for the search field. Answers whether the field
    /// owned it.
    pub fn field_key(&mut self, key: Key) -> bool {
        if !(self.search_visible && self.search_focused) {
            return false;
        }
        match key {
            Key::Backspace => {
                if self.search_cursor == 0 {
                    return true;
                }
                let prev = self.search[..self.search_cursor]
                    .chars()
                    .next_back()
                    .expect("a non-empty prefix has a last character");
                self.search
                    .drain(self.search_cursor - prev.len_utf8()..self.search_cursor);
                self.search_cursor -= prev.len_utf8();
                self.rebuild_list();
                self.scroll = 0.0;
                self.refresh_grid();
                true
            }
            Key::ArrowLeft => {
                if let Some(prev) = self.search[..self.search_cursor].chars().next_back() {
                    self.search_cursor -= prev.len_utf8();
                }
                true
            }
            Key::ArrowRight => {
                if let Some(next) = self.search[self.search_cursor..].chars().next() {
                    self.search_cursor += next.len_utf8();
                }
                true
            }
            _ => false,
        }
    }

    /// Types characters into the search field: the armed first-keystroke
    /// clear, the 15-character cap, then the re-filter with its scroll
    /// reset (`keyTyped`:321-332, `updateCreativeSearch`:341-387).
    pub fn type_text(&mut self, text: &str) {
        if !(self.search_visible && self.search_focused) {
            return;
        }
        if self.clear_armed {
            self.search.clear();
            self.search_cursor = 0;
            self.clear_armed = false;
        }
        let room = SEARCH_MAX.saturating_sub(self.search.chars().count());
        let insert: String = text
            .chars()
            .filter(|&c| c != '§' && c >= ' ' && c != '\u{7f}')
            .take(room)
            .collect();
        if insert.is_empty() {
            return;
        }
        self.search.insert_str(self.search_cursor, &insert);
        self.search_cursor += insert.len();
        self.rebuild_list();
        self.scroll = 0.0;
        self.refresh_grid();
    }

    /// Rolls the wheel: ignored unless the list scrolls (`handleMouseInput`
    /// :546-569).
    pub fn wheel(&mut self, notch: f32) {
        if self.selected_tab == INVENTORY_TAB || !self.needs_scrollbars() {
            return;
        }
        self.scroll = wheel_scroll(self.scroll, self.list.len(), notch);
        self.refresh_grid();
    }

    /// Folds one window-0 snapshot into the player copies: slots 36–44
    /// always land; every other slot is suppressed while the open screen is
    /// creative on a non-inventory tab (`handleSetSlot`:1135-1165).
    pub fn apply_snapshot(&mut self, slots: Vec<Option<MetadataItem>>) {
        for i in 0..HOTBAR_CELLS {
            let stack = slots.get(36 + i).cloned().unwrap_or(None);
            self.hotbar[i] = stack.clone();
            self.player[36 + i] = stack;
        }
        if self.selected_tab == INVENTORY_TAB {
            for i in 0..36 {
                self.player[i] = slots.get(i).cloned().unwrap_or(None);
            }
        }
    }

    /// The cursor draw the frame reads: the carried stack at the pointer
    /// minus 8 (`drawScreen`:144-170).
    pub fn cursor_draw(&self, mouse: (f32, f32)) -> Option<(MetadataItem, f32, f32)> {
        self.cursor
            .clone()
            .map(|stack| (stack, mouse.0 - CURSOR_OFFSET, mouse.1 - CURSOR_OFFSET))
    }

    /// The current ordered list's length: the thumb's enabled slice reads
    /// it (`thumb_sheet_x`).
    pub fn list_len(&self) -> usize {
        self.list.len()
    }

    /// Whether a scrollbar drag is armed (test scaffolding).
    #[cfg(test)]
    pub fn is_scrolling_for_test(&self) -> bool {
        self.scrolling
    }

    /// The current ordered list's length (test scaffolding).
    #[cfg(test)]
    pub fn list_len_for_test(&self) -> usize {
        self.list.len()
    }

    /// The current ordered list (test scaffolding).
    #[cfg(test)]
    pub fn list_for_test(&self) -> &[TabEntry] {
        &self.list
    }

    /// Sets the scroll offset directly (test scaffolding).
    #[cfg(test)]
    pub fn set_scroll_for_test(&mut self, scroll: f32) {
        self.scroll = scroll.clamp(0.0, 1.0);
        self.refresh_grid();
    }

    /// Sets the cursor directly (test scaffolding).
    #[cfg(test)]
    pub fn set_cursor_for_test(&mut self, cursor: Option<MetadataItem>) {
        self.cursor = cursor;
    }

    /// Sets a hotbar copy directly (test scaffolding).
    #[cfg(test)]
    pub fn set_hotbar_for_test(&mut self, index: usize, stack: Option<MetadataItem>) {
        if index < HOTBAR_CELLS {
            self.hotbar[index] = stack;
        }
    }

    /// Sets a player copy directly (test scaffolding).
    #[cfg(test)]
    pub fn set_player_for_test(&mut self, slot: i16, stack: Option<MetadataItem>) {
        if let Ok(i) = usize::try_from(slot) {
            if i < PLAYER_SLOTS {
                self.player[i] = stack;
            }
        }
    }

    /// Sets the search query bypassing the tab gate, then re-filters
    /// (test scaffolding for the no-clip spill pin).
    #[cfg(test)]
    pub fn type_text_for_test(&mut self, text: &str) {
        self.search.clear();
        self.search.push_str(text);
        self.search_cursor = self.search.len();
        self.rebuild_list();
        self.scroll = 0.0;
        self.refresh_grid();
    }

    /// Presses a button at a hovered region directly (test scaffolding).
    #[cfg(test)]
    pub fn press_at_for_test(
        &mut self,
        hover: Hover,
        button: ClickButton,
        shift: bool,
    ) -> Vec<InputEvent> {
        let (x, y) = hover_center(hover);
        self.mouse = (x, y);
        self.press(button, shift, &ItemTable, 1_000)
    }

    /// Runs the take ladder at a grid cell directly (test scaffolding).
    #[cfg(test)]
    pub fn take_at_for_test(&mut self, cell: usize, shift: bool, button: ClickButton) {
        self.take(cell, shift, button, &ItemTable);
    }

    /// Types a screen key at a hovered region directly (test scaffolding).
    #[cfg(test)]
    pub fn screen_key_at_for_test(
        &mut self,
        hover: Hover,
        key: ScreenKey,
        ctrl: bool,
    ) -> Vec<InputEvent> {
        let (x, y) = hover_center(hover);
        self.mouse = (x, y);
        self.screen_key(key, ctrl)
    }

    /// Arms a press-with-cursor drag over the hotbar band.
    fn arm_drag(&mut self, button: ClickButton) {
        self.drag_covered = Some(Vec::new());
        self.drag_button = button.raw();
        self.ignore_release = false;
    }

    /// The grid take ladder (`:148-228`): plain left-click copies the cell
    /// AS-IS, shift upgrades to max, right merges ±1 around the cursor, and
    /// anything else clears it. The cursor alone moves — takes send
    /// nothing.
    fn take(&mut self, cell: usize, shift: bool, button: ClickButton, caps: &impl StackCaps) {
        let held = self.grid.get(cell).cloned().flatten();
        match (self.cursor.clone(), held) {
            (None, None) => {}
            (None, Some(stack)) => {
                self.cursor = Some(if shift { max_copy(&stack, caps) } else { stack });
            }
            (Some(_), None) => {
                self.cursor = None;
            }
            (Some(cursor), Some(stack)) => {
                if !same_stack(&cursor, &stack) {
                    self.cursor = None;
                } else if shift {
                    self.cursor = Some(max_copy(&stack, caps));
                } else if button == ClickButton::Left {
                    let cap = max_stack_size(&stack, caps);
                    self.cursor = Some(MetadataItem {
                        count: count_byte((i32::from(cursor.count) + 1).min(cap)),
                        ..cursor
                    });
                } else if button == ClickButton::Right {
                    if cursor.count <= 1 {
                        self.cursor = None;
                    } else {
                        self.cursor = Some(MetadataItem {
                            count: cursor.count - 1,
                            ..cursor
                        });
                    }
                }
            }
        }
    }

    /// The outside-click drop: the whole cursor on the left, one item on
    /// the right, both with C10(−1) (`:91-116`).
    fn outside_drop(&mut self, button: ClickButton) -> Vec<InputEvent> {
        let Some(cursor) = self.cursor.clone() else {
            return Vec::new();
        };
        if button == ClickButton::Left {
            self.cursor = None;
            vec![creative_action(-1, Some(cursor))]
        } else {
            let dropped = MetadataItem {
                count: 1,
                ..cursor.clone()
            };
            if cursor.count <= 1 {
                self.cursor = None;
            } else {
                self.cursor = Some(MetadataItem {
                    count: cursor.count - 1,
                    ..cursor
                });
            }
            vec![creative_action(-1, Some(dropped))]
        }
    }

    /// The delete slot: shift-click sends the C10 nulls across the player
    /// container with no local clear (`:117-123`); a plain click clears
    /// the cursor on the inventory tab (`:124-147`) and runs the take
    /// ladder on the first displayed item anywhere else (the tmp-index-0
    /// quirk, `:512`).
    fn click_bin(
        &mut self,
        button: ClickButton,
        shift: bool,
        caps: &impl StackCaps,
    ) -> Vec<InputEvent> {
        if shift && button != ClickButton::Pick {
            return (0..BIN_CLEAR_WIRES)
                .map(|wire| creative_action(wire, None))
                .collect();
        }
        if self.selected_tab == INVENTORY_TAB {
            self.cursor = None;
            Vec::new()
        } else if button == ClickButton::Pick {
            if self.cursor.is_none() {
                if let Some(first) = self.grid[0].clone() {
                    self.cursor = Some(max_copy(&first, caps));
                }
            }
            Vec::new()
        } else {
            self.take(0, shift, button, caps);
            Vec::new()
        }
    }

    /// The 54-slot creative container around the grid and the hotbar: the
    /// prediction device for hotbar-row clicks (`:229-245`, through
    /// [`LocalContainer::slot_click`]).
    fn scratch_grid(&self) -> LocalContainer {
        let mut slots = Vec::with_capacity(PAGE_CELLS + HOTBAR_CELLS);
        slots.extend(self.grid.iter().cloned());
        slots.extend(self.hotbar.iter().cloned());
        LocalContainer {
            slots,
            cursor: self.cursor.clone(),
            drag_mode: -1,
            drag_event: 0,
            drag_slots: Vec::new(),
        }
    }

    /// Writes a grid scratch back into the view's copies.
    fn write_back_grid(&mut self, container: &LocalContainer) {
        for (i, stack) in container.slots.iter().take(PAGE_CELLS).enumerate() {
            self.grid[i] = stack.clone();
        }
        for (i, stack) in container
            .slots
            .iter()
            .skip(PAGE_CELLS)
            .take(HOTBAR_CELLS)
            .enumerate()
        {
            self.hotbar[i] = stack.clone();
        }
        self.cursor = container.cursor.clone();
    }

    /// The player container around the inventory tab's copies: the
    /// prediction device for its modes 0/3 (`:124-147`, through the shared
    /// pickup and pick branches — modes 1/5/6 never reach it).
    fn scratch_player(&self) -> LocalContainer {
        LocalContainer {
            slots: self.player.to_vec(),
            cursor: self.cursor.clone(),
            drag_mode: -1,
            drag_event: 0,
            drag_slots: Vec::new(),
        }
    }

    /// Writes a player scratch back, echoing every changed slot as C10 —
    /// the port's `CreativeCrafting` echo without the crafter loop: the
    /// local click applies to the player container and each changed wire
    /// slot carries its new stack.
    fn write_back_player(&mut self, container: &LocalContainer) -> Vec<InputEvent> {
        let mut out = Vec::new();
        for (i, stack) in container.slots.iter().take(PLAYER_SLOTS).enumerate() {
            if self.player[i] != *stack {
                self.player[i] = stack.clone();
                out.push(creative_action(i as i16, stack.clone()));
            }
        }
        self.cursor = container.cursor.clone();
        out
    }

    /// One hotbar-row click through the creative container with the R34
    /// echo: the clicked wire slot always, plus any other changed hotbar
    /// slot (the swap and gather fan-out).
    fn run_hotbar_click(
        &mut self,
        k: usize,
        raw: i8,
        mode: i8,
        caps: &impl StackCaps,
    ) -> Vec<InputEvent> {
        let before = self.hotbar.clone();
        let mut container = self.scratch_grid();
        container.slot_click(PAGE_CELLS as i16 + k as i16, raw, mode, caps);
        self.write_back_grid(&container);
        let mut out = vec![creative_action(36 + k as i16, self.hotbar[k].clone())];
        for (i, (old, new)) in before.iter().zip(self.hotbar.iter()).enumerate() {
            if i != k && old != new {
                out.push(creative_action(36 + i as i16, new.clone()));
            }
        }
        out
    }

    /// One inventory-tab slot click: mode 3 picks a max copy with no
    /// packet, mode 0 predicts through the shared pickup branch with the
    /// diff echo.
    fn click_player(
        &mut self,
        slot: i16,
        button: ClickButton,
        caps: &impl StackCaps,
    ) -> Vec<InputEvent> {
        let mut container = self.scratch_player();
        let mode = if button == ClickButton::Pick {
            CLICK_MODE_CREATIVE_PICK
        } else {
            CLICK_MODE_PICKUP
        };
        container.slot_click(slot, button.raw(), mode, caps);
        self.write_back_player(&container)
    }

    /// Replays a drag-armed press as the single click its release names
    /// (the drag covered nothing).
    fn replay_single(
        &mut self,
        hover: Hover,
        button: ClickButton,
        shift: bool,
        caps: &impl StackCaps,
    ) -> Vec<InputEvent> {
        match hover {
            Hover::Grid(cell) => {
                if button == ClickButton::Pick {
                    Vec::new()
                } else {
                    self.take(cell, shift, button, caps);
                    Vec::new()
                }
            }
            Hover::Hotbar(k) => {
                let mode = if button == ClickButton::Pick {
                    CLICK_MODE_CREATIVE_PICK
                } else if shift {
                    CLICK_MODE_QUICK_MOVE
                } else {
                    CLICK_MODE_PICKUP
                };
                self.run_hotbar_click(k, button.raw(), mode, caps)
            }
            Hover::Player(slot) => {
                if shift {
                    Vec::new()
                } else {
                    self.click_player(slot, button, caps)
                }
            }
            Hover::Bin => self.click_bin(button, shift, caps),
            Hover::None => {
                if self.cursor.is_some() && button != ClickButton::Pick {
                    self.outside_drop(button)
                } else {
                    Vec::new()
                }
            }
            Hover::Tab(_) | Hover::Track | Hover::Panel => Vec::new(),
        }
    }

    /// The drag's end batch: the mode-5 start/add/end sequence through the
    /// creative container, then C10 across the nine hotbar wire slots
    /// (`:229-245`'s drag-event-2 arm).
    fn finish_hotbar_drag(
        &mut self,
        covered: &[i16],
        limit: i32,
        caps: &impl StackCaps,
    ) -> Vec<InputEvent> {
        let mut container = self.scratch_grid();
        container.slot_click(-999, drag_button(0, limit) as i8, CLICK_MODE_DRAG, caps);
        for slot in covered {
            container.slot_click(*slot, drag_button(1, limit) as i8, CLICK_MODE_DRAG, caps);
        }
        container.slot_click(-999, drag_button(2, limit) as i8, CLICK_MODE_DRAG, caps);
        self.write_back_grid(&container);
        (0..HOTBAR_CELLS as i16)
            .map(|i| creative_action(36 + i, self.hotbar[i as usize].clone()))
            .collect()
    }

    /// A number key: the hotbar swap, tried before text (`checkHotbarKeys`
    /// :718-733, gated on an empty cursor and a hovered slot). Answers
    /// `None` when the key falls through to the search field's text path —
    /// the window's key router reads the answer before typing.
    pub fn number_key(&mut self, index: u8, caps: &impl StackCaps) -> Option<Vec<InputEvent>> {
        if index >= HOTBAR_CELLS as u8 || self.cursor.is_some() {
            return None;
        }
        let i = index as usize;
        match self.hover_last(self.mouse.0, self.mouse.1) {
            Hover::Grid(cell) => {
                let held = self.grid[cell].clone()?;
                let copy = max_copy(&held, caps);
                self.hotbar[i] = Some(copy.clone());
                Some(vec![creative_action(36 + index as i16, Some(copy))])
            }
            Hover::Hotbar(k) => {
                if k == i {
                    return Some(Vec::new());
                }
                let before = self.hotbar.clone();
                self.hotbar.swap(k, i);
                Some(diff_band(&before, &self.hotbar, 36))
            }
            Hover::Player(slot) => {
                let hot = 36 + index as i16;
                if slot == hot {
                    return Some(Vec::new());
                }
                let mut out = Vec::new();
                self.player.swap(slot as usize, hot as usize);
                for (n, stack) in self.player.iter().enumerate() {
                    if n == slot as usize || n == hot as usize {
                        out.push(creative_action(n as i16, stack.clone()));
                    }
                }
                Some(out)
            }
            Hover::Tab(_) | Hover::Track | Hover::Bin | Hover::Panel | Hover::None => None,
        }
    }

    /// The drop key over a hover: grid cells drop a copy with C10(−1) and
    /// keep the cursor (`:148-228`'s mode-4 arm); hotbar cells predict
    /// through the container with the single-slot echo (`:229-245`); player
    /// slots drop 1 (or max with Ctrl) with C10(−1) only (`:124-147`).
    fn drop_key(&mut self, hover: Hover, ctrl: bool, caps: &impl StackCaps) -> Vec<InputEvent> {
        match hover {
            Hover::Grid(cell) => {
                let Some(held) = self.grid[cell].clone() else {
                    return Vec::new();
                };
                let cap = max_stack_size(&held, caps);
                let dropped = MetadataItem {
                    count: count_byte(if ctrl { cap } else { 1 }),
                    ..held
                };
                vec![creative_action(-1, Some(dropped))]
            }
            Hover::Hotbar(k) => {
                let raw = if ctrl { 1 } else { 0 };
                self.run_hotbar_click(k, raw, CLICK_MODE_DROP, caps)
            }
            Hover::Player(slot) => {
                if let Some(held) = self.player[slot as usize].clone() {
                    let cap = max_stack_size(&held, caps);
                    let want = (if ctrl { cap } else { 1 }).min(i32::from(held.count));
                    let left = i32::from(held.count) - want;
                    self.player[slot as usize] = if left > 0 {
                        Some(MetadataItem {
                            count: count_byte(left),
                            ..held.clone()
                        })
                    } else {
                        None
                    };
                    let dropped = MetadataItem {
                        count: count_byte(want),
                        ..held
                    };
                    vec![creative_action(-1, Some(dropped))]
                } else if let Some(cursor) = self.cursor.clone() {
                    self.cursor = None;
                    vec![creative_action(-1, Some(cursor))]
                } else {
                    Vec::new()
                }
            }
            Hover::Tab(_) | Hover::Track | Hover::Bin | Hover::Panel | Hover::None => Vec::new(),
        }
    }

    /// The pick key over a hovered stack: a max copy with no packet on
    /// grid cells and player slots; the single-slot echo on the hotbar
    /// row (`:359-368`, `:407-412`, `:692-696`).
    fn pick_key(&mut self, hover: Hover, caps: &impl StackCaps) -> Vec<InputEvent> {
        match hover {
            Hover::Grid(cell) => {
                if self.cursor.is_none() {
                    if let Some(held) = self.grid[cell].clone() {
                        self.cursor = Some(max_copy(&held, caps));
                    }
                }
                Vec::new()
            }
            Hover::Hotbar(k) => {
                if self.cursor.is_none() && self.hotbar[k].is_some() {
                    self.run_hotbar_click(
                        k,
                        ClickButton::Pick.raw(),
                        CLICK_MODE_CREATIVE_PICK,
                        caps,
                    )
                } else {
                    Vec::new()
                }
            }
            Hover::Player(slot) => {
                if self.cursor.is_none() {
                    if let Some(held) = self.player[slot as usize].clone() {
                        self.cursor = Some(max_copy(&held, caps));
                    }
                }
                Vec::new()
            }
            Hover::Bin if self.selected_tab != INVENTORY_TAB => {
                if self.cursor.is_none() {
                    if let Some(first) = self.grid[0].clone() {
                        self.cursor = Some(max_copy(&first, caps));
                    }
                }
                Vec::new()
            }
            Hover::Bin | Hover::Tab(_) | Hover::Track | Hover::Panel | Hover::None => Vec::new(),
        }
    }
}

/// One C10 creative write: `sendSlotPacket`'s packet and
/// `sendPacketDropItem`'s −1 (`PlayerControllerMP.java`:557-574).
fn creative_action(slot: i16, item: Option<MetadataItem>) -> InputEvent {
    InputEvent::CreativeAction { slot, item }
}

/// The same-stack test a merge reads: the same item, the same damage,
/// equal tags — the count is not part of it (`Container.java`:726).
fn same_stack(one: &MetadataItem, other: &MetadataItem) -> bool {
    one.id == other.id && one.damage == other.damage && one.nbt == other.nbt
}

/// A count narrowed into the wire's own byte, stopping at its top.
fn count_byte(count: i32) -> u8 {
    u8::try_from(count).unwrap_or(u8::MAX)
}

/// A max-size copy of the stack under the caps.
fn max_copy(stack: &MetadataItem, caps: &impl StackCaps) -> MetadataItem {
    MetadataItem {
        count: count_byte(max_stack_size(stack, caps).max(1)),
        ..stack.clone()
    }
}

/// The C10 echo across a changed nine-slot band at the wire base.
fn diff_band(
    before: &[Option<MetadataItem>; HOTBAR_CELLS],
    after: &[Option<MetadataItem>; HOTBAR_CELLS],
    base: i16,
) -> Vec<InputEvent> {
    before
        .iter()
        .zip(after.iter())
        .enumerate()
        .filter(|(_, (old, new))| old != new)
        .map(|(i, (_, new))| creative_action(base + i as i16, new.clone()))
        .collect()
}

/// A hovered region's centre in panel units (shared by the press/release
/// test scaffolding).
#[cfg(test)]
fn hover_center(hover: Hover) -> (f32, f32) {
    match hover {
        Hover::Grid(cell) => (
            (GRID_LEFT + (cell % GRID_COLS as usize) as i32 * CELL_STEP + 8) as f32,
            (GRID_TOP + (cell / GRID_COLS as usize) as i32 * CELL_STEP + 8) as f32,
        ),
        Hover::Hotbar(k) => ((GRID_LEFT + k as i32 * CELL_STEP + 8) as f32, 120.0),
        Hover::Bin => ((BIN_DX + 8) as f32, (BIN_DY + 8) as f32),
        Hover::Track => ((TRACK_DX + 7) as f32, (TRACK_DY + 50) as f32),
        Hover::Tab(index) => {
            let (hx, hy) = tab_hit(index);
            ((hx + 14) as f32, (hy + 16) as f32)
        }
        Hover::Player(slot) => {
            let (sx, sy) = inventory_slot_pos(slot);
            ((sx + 8) as f32, (sy + 8) as f32)
        }
        Hover::Panel => (2.0, 100.0),
        Hover::None => (300.0, 300.0),
    }
}

#[cfg(test)]
mod tests {
    //! The creative screen's pins: the tab strip, the paging model with the
    //! wheel quirk, the search rules, the delete slot with its quirk, and
    //! the wire table (grid takes send nothing, hotbar and inventory-tab
    //! mutations echo C10, drops send C10(−1), C0E never leaves).

    use super::*;
    use crate::items::ItemTable;

    /// One stack from a literal id and count.
    fn stack(id: i16, count: u8) -> Option<MetadataItem> {
        Some(MetadataItem {
            id,
            count,
            damage: 0,
            nbt: None,
        })
    }

    /// A screen on the tab with the test caps.
    fn screen_on(tab: u8) -> CreativeScreen {
        CreativeScreen::new(tab)
    }

    #[test]
    fn the_frame_is_wider_than_the_standard_panel() {
        // 195×136 to fit the scrollbar (`GuiContainerCreative.java`:65-66).
        assert_eq!((FRAME_W, FRAME_H), (195, 136));
    }

    #[test]
    fn the_search_tab_is_five_and_the_inventory_tab_is_eleven() {
        // The plan's 10/11 for search is wrong: 10 is materials, 5 is
        // search, 11 the survival inventory (`CreativeTabs.java`:55-103).
        assert_eq!(SEARCH_TAB, 5);
        assert_eq!(INVENTORY_TAB, 11);
        assert_eq!(CreativeTab::Search.index(), 5);
        assert_eq!(CreativeTab::Inventory.index(), 11);
        assert_eq!(CreativeTab::Materials.index(), 10);
    }

    #[test]
    fn the_strip_is_two_rows_of_six() {
        for index in 0..TAB_COUNT {
            assert_eq!(tab_column(index), index % 6);
            assert_eq!(tab_first_row(index), index < 6);
        }
    }

    #[test]
    fn the_tab_hitboxes_follow_the_source_column_math() {
        // x = 28·col (+col past 0; xSize−28+2 at col 5); y = −32 up top,
        // +ySize below; 28×32 (`func_147049_a`:714-739).
        assert_eq!(tab_hit(0), (0, -32));
        assert_eq!(tab_hit(1), (29, -32));
        assert_eq!(tab_hit(SEARCH_TAB), (FRAME_W - TAB_W + 2, -32));
        assert_eq!(tab_hit(6), (0, FRAME_H));
        assert_eq!(tab_hit(INVENTORY_TAB), (FRAME_W - TAB_W + 2, FRAME_H));
        assert_eq!((TAB_W, TAB_H), (28, 32));
    }

    #[test]
    fn the_tab_sprites_follow_the_source_draw_math() {
        // l = gx + 28·col (+col past 0; +xSize−28 at col 5); i1 = gy−28 up
        // top, gy+(ySize−4) below; u = col·28, v = 0/+32/+64.
        assert_eq!(tab_sprite(0), (0, -28));
        assert_eq!(tab_sprite(1), (29, -28));
        assert_eq!(tab_sprite(SEARCH_TAB), (FRAME_W - TAB_W, -28));
        assert_eq!(tab_sprite(6), (0, FRAME_H - 4));
        assert_eq!(tab_sprite(INVENTORY_TAB), (FRAME_W - TAB_W, FRAME_H - 4));
        assert_eq!(tab_uv(0, false), (0, 0));
        assert_eq!(tab_uv(0, true), (0, 32));
        assert_eq!(tab_uv(6, false), (0, 64));
        assert_eq!(tab_uv(6, true), (0, 96));
        assert_eq!(tab_uv(SEARCH_TAB, true), (5 * 28, 32));
        assert_eq!(tab_icon_pos(0), (6, -28 + 9));
        assert_eq!(tab_icon_pos(6), (6, FRAME_H - 4 + 7));
    }

    #[test]
    fn only_the_inventory_tab_hides_the_scrollbar_and_the_title() {
        for index in 0..TAB_COUNT {
            assert_eq!(tab_hides_chrome(index), index == INVENTORY_TAB);
            assert_eq!(tab_title(index).is_none(), index == INVENTORY_TAB);
        }
        assert_eq!(tab_title(0).as_deref(), Some("itemGroup.buildingBlocks"));
    }

    #[test]
    fn the_page_rows_ceil_and_the_start_row_rounds() {
        // (size+8)/9−5; (int)(p·rows+0.5) floored at 0 (`scrollTo`:881-907).
        assert_eq!(page_rows(600), 62);
        assert_eq!(page_rows(46), 1);
        assert_eq!(page_rows(45), 0);
        assert_eq!(start_row(0.0, 62), 0);
        assert_eq!(start_row(1.0, 62), 62);
        assert_eq!(start_row(0.5, 62), 31);
        assert_eq!(start_row(0.25, 62), 16);
    }

    #[test]
    fn past_the_end_cells_are_empty_with_no_clipping() {
        // No clipping draw exists: past-the-end cells write null and render
        // as ordinary empty slots (`scrollTo`:881-907). 158 building-blocks
        // entries at scroll 1: rows (158+8)/9−5 = 13, start 13, so cells
        // show list 117..161 — the last four cells (158..161) are empty.
        let mut screen = CreativeScreen::new(CreativeTab::BuildingBlocks.index());
        assert_eq!(
            screen.list_len_for_test(),
            creative_tab_items(CreativeTab::BuildingBlocks).len()
        );
        screen.set_scroll_for_test(1.0);
        let filled = screen.grid().iter().filter(|cell| cell.is_some()).count();
        assert_eq!(filled, 41);
        assert!(screen.grid()[41..].iter().all(|cell| cell.is_none()));
    }

    #[test]
    fn the_wheel_divisor_quirk_slams_to_an_end_at_size_46() {
        // size/9−5 TRUNCATES: 46/9 = 5, 5−5 = 0, so ±Infinity clamps to an
        // end (`handleMouseInput`:546-569). Match it, do not fix it.
        assert_eq!(wheel_scroll(0.5, 46, 1.0), 0.0);
        assert_eq!(wheel_scroll(0.5, 46, -1.0), 1.0);
        assert_eq!(wheel_scroll(0.5, 53, 1.0), 0.0);
        let stepped = wheel_scroll(0.5, 600, 1.0);
        assert!((stepped - (0.5 - 1.0 / 61.0)).abs() < 1e-6, "{stepped}");
    }

    #[test]
    fn entering_the_search_tab_autofocuses_and_clears() {
        let mut screen = screen_on(0);
        assert!(!screen.search_visible());
        assert!(!screen.search_focused());
        screen.set_tab(SEARCH_TAB);
        assert!(screen.search_visible());
        assert!(screen.search_focused());
        assert_eq!(screen.search_text(), "");
        screen.type_text("stone");
        assert_eq!(screen.search_text(), "stone");
        screen.set_tab(0);
        assert!(!screen.search_visible());
        assert!(!screen.search_focused());
        screen.set_tab(SEARCH_TAB);
        assert_eq!(screen.search_text(), "", "re-entering clears");
        assert!(screen.search_focused());
    }

    #[test]
    fn every_click_arms_the_first_keystroke_clear() {
        let mut screen = screen_on(SEARCH_TAB);
        screen.type_text("stone");
        screen.press(ClickButton::Left, false, &ItemTable, 1_000);
        screen.type_text("x");
        assert_eq!(screen.search_text(), "x");
    }

    #[test]
    fn the_search_caps_at_fifteen_characters() {
        let mut screen = screen_on(SEARCH_TAB);
        screen.type_text("0123456789abcdef");
        assert_eq!(screen.search_text(), "0123456789abcde");
        assert_eq!(screen.search_text().chars().count(), SEARCH_MAX);
    }

    #[test]
    fn the_search_filters_the_pinned_list_by_name() {
        let mut screen = screen_on(SEARCH_TAB);
        assert_eq!(screen.list_len_for_test(), 600);
        screen.type_text("stone");
        assert!(screen.list_len_for_test() > 0);
        assert!(screen.list_len_for_test() < 600);
        for entry in screen.list_for_test() {
            let name = stack_name(entry.id, entry.damage).unwrap_or("");
            assert!(search_matches(name, "stone"), "{name} matched stone");
        }
        screen.set_tab(SEARCH_TAB);
        assert_eq!(screen.search_text(), "");
        assert_eq!(screen.list_len_for_test(), 600);
    }

    #[test]
    fn a_plain_take_copies_as_is_and_sends_nothing() {
        // Plain left-click copies the cell stack AS-IS (usually size 1),
        // NOT a full stack — and sends nothing (`:148-228`).
        let mut screen = screen_on(0);
        let cell = screen.grid()[0].clone().expect("tab 0 opens non-empty");
        let events = screen.press_at_for_test(Hover::Grid(0), ClickButton::Left, false);
        assert!(events.is_empty(), "takes send nothing");
        assert_eq!(screen.cursor(), Some(&cell));
    }

    #[test]
    fn a_shift_take_upgrades_to_max_and_sends_nothing() {
        let mut screen = screen_on(0);
        let events = screen.press_at_for_test(Hover::Grid(0), ClickButton::Left, true);
        assert!(events.is_empty());
        let cursor = screen.cursor().expect("shift takes");
        assert_eq!(cursor.count, 64, "stone stacks to 64");
    }

    #[test]
    fn a_right_take_merges_down_and_clears_at_zero() {
        let mut screen = screen_on(0);
        screen.press_at_for_test(Hover::Grid(0), ClickButton::Left, false);
        let first = screen.cursor().expect("took");
        assert_eq!(first.count, 1);
        screen.press_at_for_test(Hover::Grid(0), ClickButton::Left, false);
        assert_eq!(screen.cursor().expect("grew").count, 2);
        screen.press_at_for_test(Hover::Grid(0), ClickButton::Right, false);
        assert_eq!(screen.cursor().expect("shrank").count, 1);
        screen.press_at_for_test(Hover::Grid(0), ClickButton::Right, false);
        assert_eq!(screen.cursor(), None);
    }

    #[test]
    fn a_mismatched_take_clears_the_cursor() {
        let mut screen = screen_on(0);
        screen.press_at_for_test(Hover::Grid(0), ClickButton::Left, false);
        assert!(screen.cursor().is_some());
        screen.press_at_for_test(Hover::Grid(1), ClickButton::Left, false);
        // Cell 1 may hold the same item; force a mismatch through the
        // ladder directly.
        screen.set_cursor_for_test(stack(1, 5));
        screen.take_at_for_test(3, false, ClickButton::Left);
        let cursor = screen.cursor();
        let cell = screen.grid()[3].clone();
        match (cursor, cell) {
            (Some(cursor), Some(cell))
                if cursor.id == cell.id
                    && cursor.damage == cell.damage
                    && cursor.nbt == cell.nbt => {}
            (None, _) => {}
            (cursor, cell) => panic!("unexpected ladder rest {cursor:?} {cell:?}"),
        }
    }

    #[test]
    fn a_middle_take_copies_max_with_no_packet() {
        let mut screen = screen_on(0);
        let events = screen.press_at_for_test(Hover::Grid(0), ClickButton::Pick, false);
        assert!(events.is_empty());
        assert_eq!(screen.cursor().expect("picked").count, 64);
    }

    #[test]
    fn a_grid_drop_sends_c10_minus_one_and_keeps_the_cursor() {
        let mut screen = screen_on(0);
        let events = screen.screen_key_at_for_test(Hover::Grid(0), ScreenKey::Drop, false);
        assert_eq!(events.len(), 1);
        match &events[0] {
            InputEvent::CreativeAction { slot, item } => {
                assert_eq!(*slot, -1);
                assert_eq!(item.clone().expect("dropped").count, 1);
            }
            other => panic!("drops send C10(-1), sent {other:?}"),
        }
        assert_eq!(screen.cursor(), None, "Q changes no cursor");
    }

    #[test]
    fn a_number_key_swaps_a_max_copy_with_a_c10_echo() {
        let mut screen = screen_on(0);
        let cell = screen.grid()[0].clone().expect("tab 0 opens non-empty");
        let events = screen.screen_key_at_for_test(Hover::Grid(0), ScreenKey::Number(3), false);
        assert_eq!(events.len(), 1);
        match &events[0] {
            InputEvent::CreativeAction { slot, item } => {
                assert_eq!(*slot, 36 + 3);
                let written = item.clone().expect("swapped");
                assert_eq!((written.id, written.damage), (cell.id, cell.damage));
                assert_eq!(written.count, 64);
            }
            other => panic!("swaps echo C10, sent {other:?}"),
        }
        assert_eq!(screen.hotbar()[3].clone().expect("hotbar").count, 64);
    }

    #[test]
    fn a_hotbar_pickup_echoes_the_wire_slot() {
        let mut screen = screen_on(0);
        screen.set_hotbar_for_test(0, stack(264, 5));
        let events = screen.press_at_for_test(Hover::Hotbar(0), ClickButton::Left, false);
        assert_eq!(screen.cursor(), stack(264, 5).as_ref());
        assert_eq!(screen.hotbar()[0], None);
        assert_eq!(events.len(), 1);
        match &events[0] {
            InputEvent::CreativeAction { slot, item } => {
                assert_eq!(*slot, 36);
                assert_eq!(item, &None);
            }
            other => panic!("hotbar clicks echo C10, sent {other:?}"),
        }
    }

    #[test]
    fn a_hotbar_shift_click_deletes_with_a_c10_echo() {
        // The creative transfer clears the hotbar slot outright
        // (`transferStackInSlot`:918-931).
        let mut screen = screen_on(0);
        screen.set_hotbar_for_test(2, stack(264, 5));
        let events = screen.press_at_for_test(Hover::Hotbar(2), ClickButton::Left, true);
        assert_eq!(screen.hotbar()[2], None);
        assert_eq!(screen.cursor(), None);
        assert_eq!(events.len(), 1);
        match &events[0] {
            InputEvent::CreativeAction { slot, item } => {
                assert_eq!(*slot, 38);
                assert_eq!(item, &None);
            }
            other => panic!("shift-clear echoes C10, sent {other:?}"),
        }
    }

    #[test]
    fn an_outside_drop_sends_c10_minus_one() {
        let mut screen = screen_on(0);
        screen.set_cursor_for_test(stack(264, 5));
        let events = screen.press_at_for_test(Hover::None, ClickButton::Left, false);
        assert_eq!(screen.cursor(), None);
        assert_eq!(events.len(), 1);
        match &events[0] {
            InputEvent::CreativeAction { slot, item } => {
                assert_eq!(*slot, -1);
                assert_eq!(item, &stack(264, 5));
            }
            other => panic!("outside drops send C10(-1), sent {other:?}"),
        }
        screen.set_cursor_for_test(stack(264, 5));
        let events = screen.press_at_for_test(Hover::None, ClickButton::Right, false);
        assert_eq!(screen.cursor(), stack(264, 4).as_ref());
        match &events[0] {
            InputEvent::CreativeAction { slot, item } => {
                assert_eq!(*slot, -1);
                assert_eq!(item, &stack(264, 1));
            }
            other => panic!("right drops split one, sent {other:?}"),
        }
    }

    #[test]
    fn the_bin_shift_clear_sends_forty_six_nulls() {
        // The source loops 0..size over the 46-slot player list with no
        // local clear; wire slots 0/45 are server no-ops (`:117-123`).
        for tab in [0, INVENTORY_TAB] {
            let mut screen = screen_on(tab);
            screen.set_player_for_test(9, stack(264, 5));
            let events = screen.press_at_for_test(Hover::Bin, ClickButton::Left, true);
            assert_eq!(events.len(), BIN_CLEAR_WIRES as usize);
            for (wire, event) in events.iter().enumerate() {
                match event {
                    InputEvent::CreativeAction { slot, item } => {
                        assert_eq!(*slot, wire as i16);
                        assert_eq!(item, &None);
                    }
                    other => panic!("bin clear sends C10 nulls, sent {other:?}"),
                }
            }
            assert_eq!(
                screen.player_slot(9),
                Some(&stack(264, 5)),
                "no local clear — sync arrives from the server"
            );
        }
    }

    #[test]
    fn the_bin_plain_click_clears_the_cursor_on_the_inventory_tab() {
        let mut screen = screen_on(INVENTORY_TAB);
        screen.set_cursor_for_test(stack(264, 5));
        let events = screen.press_at_for_test(Hover::Bin, ClickButton::Left, false);
        assert!(events.is_empty());
        assert_eq!(screen.cursor(), None);
    }

    #[test]
    fn the_bin_plain_click_takes_cell_zero_on_grid_tabs() {
        // The quirk: the bin shares tmp index 0 with grid cell (0, 0)
        // (`:512`), so a plain bin click on a grid tab runs the take
        // ladder on the first displayed item instead of clearing.
        let mut screen = screen_on(0);
        let cell = screen.grid()[0].clone().expect("tab 0 opens non-empty");
        let events = screen.press_at_for_test(Hover::Bin, ClickButton::Left, false);
        assert!(events.is_empty());
        assert_eq!(screen.cursor(), Some(&cell));
    }

    #[test]
    fn inventory_tab_pickup_echoes_changed_slots() {
        let mut screen = screen_on(INVENTORY_TAB);
        screen.set_player_for_test(9, stack(264, 5));
        let events = screen.press_at_for_test(Hover::Player(9), ClickButton::Left, false);
        assert_eq!(screen.cursor(), stack(264, 5).as_ref());
        assert_eq!(screen.player_slot(9), Some(&None));
        assert_eq!(events.len(), 1);
        match &events[0] {
            InputEvent::CreativeAction { slot, item } => {
                assert_eq!(*slot, 9);
                assert_eq!(item, &None);
            }
            other => panic!("inventory-tab mutations echo C10, sent {other:?}"),
        }
    }

    #[test]
    fn inventory_tab_number_keys_swap_with_an_echo_pair() {
        let mut screen = screen_on(INVENTORY_TAB);
        screen.set_player_for_test(9, stack(264, 1));
        screen.set_player_for_test(36, stack(265, 1));
        let events = screen.screen_key_at_for_test(Hover::Player(9), ScreenKey::Number(0), false);
        assert_eq!(screen.player_slot(9), Some(&stack(265, 1)));
        assert_eq!(screen.player_slot(36), Some(&stack(264, 1)));
        assert_eq!(events.len(), 2, "the echo names both swapped slots");
    }

    #[test]
    fn inventory_tab_drops_send_c10_minus_one() {
        let mut screen = screen_on(INVENTORY_TAB);
        screen.set_player_for_test(9, stack(264, 5));
        let events = screen.screen_key_at_for_test(Hover::Player(9), ScreenKey::Drop, false);
        assert_eq!(events.len(), 1);
        match &events[0] {
            InputEvent::CreativeAction { slot, item } => {
                assert_eq!(*slot, -1);
                assert_eq!(item, &stack(264, 1));
            }
            other => panic!("inventory-tab drops send C10(-1), sent {other:?}"),
        }
        assert_eq!(screen.player_slot(9), Some(&stack(264, 4)));
    }

    #[test]
    fn nothing_from_this_screen_is_a_click_window() {
        // NO C0E is ever sent here: the override shadows the base send and
        // `windowClick` is never called (`PlayerControllerMP.java`:534-540).
        let mut screen = screen_on(0);
        screen.set_hotbar_for_test(0, stack(264, 5));
        screen.set_cursor_for_test(stack(265, 2));
        let mut all = Vec::new();
        all.extend(screen.press_at_for_test(Hover::Grid(0), ClickButton::Left, false));
        all.extend(screen.press_at_for_test(Hover::Hotbar(0), ClickButton::Left, false));
        all.extend(screen.screen_key_at_for_test(Hover::Grid(0), ScreenKey::Drop, false));
        all.extend(screen.screen_key_at_for_test(Hover::Grid(0), ScreenKey::Number(1), false));
        all.extend(screen.press_at_for_test(Hover::None, ClickButton::Left, false));
        for event in &all {
            assert!(
                matches!(event, InputEvent::CreativeAction { .. }),
                "only C10 leaves the creative screen, sent {event:?}"
            );
        }
        assert!(!all.is_empty());
    }

    #[test]
    fn the_thumb_parks_by_scroll_with_the_source_slices() {
        // 12×15 at (176, 18 + 95·scroll); sheet x 232 scrolling, 244 idle.
        assert_eq!(thumb_rect(0.0, 0), Some((THUMB_DX, TRACK_DY)));
        assert_eq!(thumb_rect(1.0, 0), Some((THUMB_DX, TRACK_DY + THUMB_SPAN)));
        assert_eq!((THUMB_W, THUMB_H), (12, 15));
        assert_eq!(THUMB_DX, TRACK_DX + 1);
        assert_eq!(thumb_rect(0.0, INVENTORY_TAB), None);
        assert_eq!(thumb_sheet_x(600, 0), THUMB_SHEET_X);
        assert_eq!(thumb_sheet_x(600, 0), 232);
        assert_eq!(thumb_sheet_x(10, 0), THUMB_IDLE_SHEET_X);
        assert_eq!(thumb_sheet_x(10, 0), 244);
    }

    #[test]
    fn the_track_press_arms_the_drag_with_no_containment_retest() {
        let mut screen = screen_on(SEARCH_TAB);
        assert!(screen.needs_scrollbars(), "600 search entries scroll");
        screen.press_at_for_test(Hover::Track, ClickButton::Left, false);
        assert!(screen.is_scrolling_for_test());
        // While armed the drag answers anywhere on the frame.
        screen.mouse_moved(10.0, 200.0);
        assert!(screen.scroll() > 0.9, "scroll {}", screen.scroll());
        screen.release(ClickButton::Left, false, &ItemTable, 2_000);
        assert!(!screen.is_scrolling_for_test());
    }

    #[test]
    fn the_search_field_sits_where_init_gui_puts_it() {
        assert_eq!((SEARCH_DX, SEARCH_DY, SEARCH_W), (82, 6, 89));
    }

    #[test]
    fn the_inventory_tab_lays_out_the_survival_slots() {
        // Armour 2×2, main 27, hotbar 9 at y 112, bin bottom-right.
        assert_eq!(inventory_slot_pos(5), (9, 6));
        assert_eq!(inventory_slot_pos(6), (9, 33));
        assert_eq!(inventory_slot_pos(7), (63, 6));
        assert_eq!(inventory_slot_pos(8), (63, 33));
        assert_eq!(inventory_slot_pos(9), (9, 54));
        assert_eq!(inventory_slot_pos(35), (9 + 8 * 18, 54 + 2 * 18));
        assert_eq!(inventory_slot_pos(36), (9, 112));
        assert_eq!(inventory_slot_pos(44), (9 + 8 * 18, 112));
        assert_eq!(inventory_slot_pos(0), (OFFSCREEN_SLOT, OFFSCREEN_SLOT));
        assert_eq!((BIN_DX, BIN_DY, BIN_SIZE), (173, 112, 16));
    }

    #[test]
    fn window_zero_snapshots_suppress_outside_the_hotbar_off_the_inventory_tab() {
        // `handleSetSlot`:1135-1165: window-0 slots 36–44 always apply;
        // the rest are suppressed on non-inventory tabs.
        let mut screen = screen_on(0);
        let mut slots = vec![None; PLAYER_SLOTS];
        slots[9] = stack(264, 1);
        slots[36] = stack(265, 1);
        screen.apply_snapshot(slots);
        assert_eq!(screen.player_slot(9), Some(&None));
        assert_eq!(screen.player_slot(36), Some(&stack(265, 1)));
        let mut screen = screen_on(INVENTORY_TAB);
        let mut slots = vec![None; PLAYER_SLOTS];
        slots[9] = stack(264, 1);
        screen.apply_snapshot(slots);
        assert_eq!(screen.player_slot(9), Some(&stack(264, 1)));
    }

    #[test]
    fn the_default_tab_is_zero() {
        assert_eq!(CreativeScreen::new(0).selected_tab(), 0);
    }
}
