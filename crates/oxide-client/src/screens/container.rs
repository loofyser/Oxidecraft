//! The container screen base: the layout seam, the hit test, the click
//! derivation, the drag machine's view side, the cursor stack and the shared
//! player-section layout.
//!
//! The port reads values and names from the source's container screen
//! (`GuiContainer.java`) and its container (`Container.java`); no source text
//! is copied. The per-kind slot tables land in Tasks 18-20 — the base
//! consumes whatever table it is given through [`ContainerLayout::slots`],
//! and the suites below use their own local tables until then.
//!
//! The click derivation below transcribes the source's handlers:
//! `mouseClicked`:359-460 (the press table), `mouseClickMove`:466-510 (the
//! drag's add condition), `mouseReleased`:515-652 (the release table) and
//! `keyTyped`'s slot keys with `checkHotbarKeys`:692-733. Every send is one
//! [`InputEvent::ClickWindow`](oxide_game::input::InputEvent::ClickWindow);
//! the drag sends nothing on press or move — its three mode-5 phases go out
//! together at release (`:615-625`), 1+n+1 packets.
//!
//! What the port leaves out, recorded: the touchscreen branches
//! (`:385-389`, `:471-504`, `:572-613` — the port has no touchscreen, so the
//! `returningStack` drag-return at :172-187 never runs either), the slot
//! kinds behind `isItemValid`/`canDragIntoSlot`/`canTakeStack`/`getHasStack`
//! (the base answers true; an empty view cell is the only false), and the
//! per-slot stack limit behind `getItemStackLimit` (the base reads 64).

use oxide_game::container::BASE_MAX_STACK_SIZE;
use oxide_game::container::{
    CLICK_MODE_CREATIVE_PICK, CLICK_MODE_DRAG, CLICK_MODE_DROP, CLICK_MODE_GATHER,
    CLICK_MODE_PICKUP, CLICK_MODE_QUICK_MOVE, CLICK_MODE_SWAP, DragState, StackCaps,
    can_add_item_to_slot, compute_stack_size, drag_button, max_stack_size,
};
use oxide_game::input::InputEvent;
use oxide_proto_v47::entity::MetadataItem;
use oxide_proto_v47::window::WindowKind;

use super::family_b::FamilyState;

/// One slot's panel-local position: the wire index, the cell's top-left in
/// panel units (`Slot.xDisplayPosition/yDisplayPosition`), and the inventory
/// block the slot belongs to — the shift-double-click fan-out's
/// same-inventory gate (`slot.inventory == ...`, Task 18's T16-F2 rider).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotPos {
    /// The wire slot number the click names.
    pub index: i16,
    /// The cell's left edge in panel units.
    pub x: i32,
    /// The cell's top edge in panel units.
    pub y: i32,
    /// Which inventory the slot reads from: the tile's own block or the
    /// player's 27+9.
    pub block: SlotBlock,
}

/// Which inventory a slot belongs to: the tile's own slots (the chest rows,
/// the hopper five, the furnace three, the brewing four, the crafting ten)
/// or the player's 27 main plus 9 hotbar (one `InventoryPlayer` either way).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotBlock {
    /// The container side: the tile's own slots.
    Container,
    /// The player side: main and hotbar share the block.
    Player,
}

/// Where a screen's title lines come from: the window's sent title or a fixed
/// label (`GuiChest` draws the lower inventory's name and the upper window's
/// at :36-40; `GuiInventory` the fixed crafting label at :65-68).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleSource {
    /// The window's title, exactly as the server sent it.
    WindowTitle,
    /// A fixed label (the inventory's name, the crafting label).
    Fixed(&'static str),
}

/// Which title lines a layout draws, with the source's own positions (pinned
/// by the draws, not here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleKind {
    /// The chest's and hopper's pair: the window's own title (the container
    /// inventory's name, as the server sent it) at `(8, 6)` and the lower
    /// inventory's name at `(8, ySize − 96 + 2)` (`GuiChest.java`:36-40,
    /// `GuiHopper.java`:36-40 — the port corrected the order in Task 18:
    /// the top line is the server title, never the player label).
    Chest {
        /// The lower (player) inventory's display name.
        lower: TitleSource,
    },
    /// The dispenser's, furnace's and brewing stand's pair: the window's own
    /// title CENTRED at `(xSize / 2 − width / 2, 6)` and the player name at
    /// `(8, ySize − 96 + 2)` (`GuiDispenser.java`:31-35,
    /// `GuiFurnace.java`:31-35, `GuiBrewingStand.java`:35-39). The centred
    /// pen needs a width measure, so [`ContainerScreen::title_lines`] takes
    /// one.
    Centred {
        /// The lower (player) inventory's display name.
        lower: TitleSource,
    },
    /// The crafting table's fixed pair: the `container.crafting` label at
    /// `(28, 6)` and the `container.inventory` label at `(8, ySize − 96 + 2)`
    /// (`GuiCrafting.java`:32-36) — the family's only fixed top label.
    Crafting {
        /// The top label's source.
        top: TitleSource,
        /// The bottom label's source.
        lower: TitleSource,
    },
    /// The inventory's single crafting label at `(86, 16)`
    /// (`GuiInventory.java`:65-68).
    Inventory {
        /// The crafting label's source.
        label: TitleSource,
    },
    /// The beacon's centred pair: `tile.beacon.primary` at x 62 and
    /// `tile.beacon.secondary` at x 169, both at y 10 in the light grey
    /// (`GuiBeacon.java`:178-179). The window title draws nowhere — the tile
    /// names the screen.
    Beacon {
        /// The primary label's source.
        primary: TitleSource,
        /// The secondary label's source.
        secondary: TitleSource,
    },
    /// The enchanting table's pair: the table's display name at `(12, 5)` and
    /// the player name at `(8, ySize − 96 + 2)` (`GuiEnchantment.java`:67-71).
    Enchanting {
        /// The table name's source: the window's title.
        upper: TitleSource,
    },
    /// The anvil's pair: `container.repair` at `(60, 6)` and the player name
    /// at `(8, ySize − 96 + 2)` (`GuiRepair.java`:73).
    Anvil {
        /// The top label's source.
        top: TitleSource,
        /// The bottom label's source.
        lower: TitleSource,
    },
    /// The generic frame's single window title at `(8, 6)`.
    Generic,
}

/// How a layout's background blits its sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundKind {
    /// One full-panel blit at the panel's top-left.
    Full,
    /// The chest's split pair (`GuiChest.java`:45-53): the sheet's upper
    /// slice `rows × 18 + 17` tall at the panel's top-left, then the 96-row
    /// bottom blit from sheet row 126 at `y = rows × 18 + 17`.
    ChestSplit {
        /// The chest's row count: `rows = slot count / 9`.
        rows: i32,
    },
}

/// One container panel: the size, the sheet and the slot table it lays out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerLayout {
    /// The panel width (`xSize`; the default 176 at `GuiContainer.java`:28).
    pub x_size: i32,
    /// The panel height (`ySize`; the default 166 at `:29`).
    pub y_size: i32,
    /// The sheet the background blits, under the port's named-texture key.
    pub sheet: &'static str,
    /// The slot table, in slot order: the hit test's first match and the
    /// highlight's last match both read this order.
    pub slots: &'static [SlotPos],
    /// Which title lines draw over the panel.
    pub title: TitleKind,
    /// How the background blits the sheet: the full panel, or the chest's
    /// split pair.
    pub background: BackgroundKind,
}

/// The generic frame's layout: the default 176×166 panel with no slot table.
/// An unknown window kind opens on the chest sheet's generic container
/// (`NetHandlerPlayClient`:1117-1126 opens a generic container for an
/// unlisted type, which `displayGUIChest` shows on `generic_54`), and —
/// until Tasks 18-20 land the per-kind tables — every container screen draws
/// it (recorded).
pub static GENERIC_LAYOUT: ContainerLayout = ContainerLayout {
    x_size: 176,
    y_size: 166,
    sheet: "gui/container/generic_54",
    slots: &[],
    title: TitleKind::Generic,
    background: BackgroundKind::Full,
};

/// The default panel width (`GuiContainer.java`:28).
pub const PANEL_W: i32 = 176;
/// The default panel height (`GuiContainer.java`:29).
pub const PANEL_H: i32 = 166;
/// One slot cell's side (`isMouseOverSlot`'s 16×16 at :657-659).
pub const SLOT_SIZE: i32 = 16;
/// The hit test's pad: `isPointInRegion` tests the ±1-padded 18×18 region
/// (`GuiContainer.java`:666-672), not the bare cell.
pub const HOVER_PAD: i32 = 1;
/// The cursor draw's offset: the carried stack's top-left sits 8 left and 8
/// above the pointer (`GuiContainer.java`:149, :169 — `k2` is 16 while a
/// touchscreen drag shows, which never runs here).
pub const CURSOR_OFFSET: f32 = 8.0;
/// The hover highlight's colour: the semi-transparent white rect at :134
/// (`drawGradientRect(..., −2130706433, −2130706433)` = 0x80FFFFFF).
pub const HOVER_COLOUR: [f32; 4] = [1.0, 1.0, 1.0, 128.0 / 255.0];
/// The title lines' colour (4210752 = 0x404040).
pub const TITLE_COLOUR: [f32; 4] = [64.0 / 255.0, 64.0 / 255.0, 64.0 / 255.0, 1.0];
/// The title lines' packed grey (4210752 = 0x404040): every container title
/// but the beacon's pair.
pub const TITLE_GREY: u32 = 4_210_752;
/// The beacon titles' packed light grey (14737632 = 0xE0E0E0,
/// `GuiBeacon.java`:178-179).
pub const BEACON_TITLE_GREY: u32 = 14_737_632;

/// Unpacks a title line's RGB int into the draw's straight RGBA.
pub fn title_rgba(colour: u32) -> [f32; 4] {
    [
        ((colour >> 16) & 0xFF) as f32 / 255.0,
        ((colour >> 8) & 0xFF) as f32 / 255.0,
        (colour & 0xFF) as f32 / 255.0,
        1.0,
    ]
}
/// The double-click window: a press on the same slot within 250 ms of the
/// last, with the same button, gathers (`mouseClicked`:359-365).
pub const DOUBLE_CLICK_MS: u64 = 250;
/// The pick-block binding's mouse encoding: the source tests
/// `mouseButton == keyBindPickBlock.getKeyCode() + 100` (:362/:410/:448) —
/// the binding, not a hard-coded middle click. The default binding is the
/// middle button, whose LWJGL code is 2.
pub const PICK_BUTTON_OFFSET: i32 = 100;
/// The raw button a pick press and release carry: the default binding's
/// middle code.
pub const PICK_BUTTON_CODE: i8 = 2;
/// The player section's left edge: every container's player block starts at
/// x 8 (`ContainerPlayer.java`:36-67, `ContainerChest.java`:16-37).
pub const SLOT_LEFT: i32 = 8;
/// The player section's cell step: 18 per row and column.
pub const SLOT_STEP: i32 = 18;
/// The standard main block's top: rows at 84/102/120 (`ContainerPlayer`:57).
pub const MAIN_TOP: i32 = 84;
/// The standard hotbar's top: 142 (`ContainerPlayer`:64-67).
pub const HOTBAR_TOP: i32 = 142;
/// The chest's rows-dependent step: the player block shifts
/// `(rows − 4) × 18` (`ContainerChest.java`:34-37).
pub const CHEST_SHIFT_STEP: i32 = 18;
/// The chest's main block top before the shift: 103 (`ContainerChest`:34).
pub const CHEST_MAIN_TOP: i32 = 103;
/// The chest's hotbar top before the shift: 161 (`ContainerChest`:37).
pub const CHEST_HOTBAR_TOP: i32 = 161;

/// One mouse button the screen binds: left, right, and the pick-block binding
/// (`mouseClicked`'s gate at :369 admits buttons 0, 1 and the binding only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickButton {
    /// Button 0.
    Left,
    /// Button 1.
    Right,
    /// The pick-block binding (`keyBindPickBlock.getKeyCode() + 100`).
    Pick,
}

impl ClickButton {
    /// The raw button the click carries on the wire.
    pub fn raw(self) -> i8 {
        match self {
            ClickButton::Left => 0,
            ClickButton::Right => 1,
            ClickButton::Pick => PICK_BUTTON_CODE,
        }
    }

    /// The drag limit a press with a carried stack arms: 0 left, 1 right, 2
    /// pick (`mouseClicked`:436-452).
    pub fn drag_limit(self) -> i32 {
        match self {
            ClickButton::Left => 0,
            ClickButton::Right => 1,
            ClickButton::Pick => 2,
        }
    }
}

/// One screen key the container answers: the number keys, the drop key and
/// the pick key (`keyTyped`:692-712 with `checkHotbarKeys`:718-733).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenKey {
    /// A number key 1-9, as the zero-based hotbar index (`checkHotbarKeys`'s
    /// `i` in 0..8, sent as mode 2's button).
    Number(u8),
    /// The drop key (Q): mode 4 over a hovered stack.
    Drop,
    /// The pick-block key: mode 3 over a hovered stack.
    Pick,
}

/// Whether the panel-local point sits in the slot's padded cell
/// (`isPointInRegion`:666-672 — the ±1 18×18 region).
pub fn point_in_slot(slot_x: i32, slot_y: i32, px: i32, py: i32) -> bool {
    px >= slot_x - HOVER_PAD
        && px < slot_x + SLOT_SIZE + HOVER_PAD
        && py >= slot_y - HOVER_PAD
        && py < slot_y + SLOT_SIZE + HOVER_PAD
}

/// The click's slot: the FIRST match in slot order
/// (`getSlotAtPosition`:341-354). Coordinates are panel-local.
pub fn slot_at_first(slots: &[SlotPos], x: f32, y: f32) -> Option<i16> {
    slots
        .iter()
        .find(|slot| point_in_slot(slot.x, slot.y, x as i32, y as i32))
        .map(|slot| slot.index)
}

/// The highlight's slot: the LAST match in slot order (the draw loop at
/// :121-139 sets `theSlot` per match, so the last one stands). Identical to
/// the click's while slot rects do not overlap.
pub fn hovered_last(slots: &[SlotPos], x: f32, y: f32) -> Option<i16> {
    slots
        .iter()
        .rev()
        .find(|slot| point_in_slot(slot.x, slot.y, x as i32, y as i32))
        .map(|slot| slot.index)
}

/// The standard player block: 27 main slots then the 9 hotbar, starting at
/// wire index `base` (`ContainerPlayer.java`:36-67 — main at
/// `(8 + j·18, 84 + l·18)`, hotbar at `(8 + i·18, 142)`).
pub fn player_section(base: i16) -> Vec<SlotPos> {
    let mut slots = Vec::with_capacity(36);
    for row in 0..3i32 {
        for col in 0..9i32 {
            slots.push(SlotPos {
                index: base + row as i16 * 9 + col as i16,
                x: SLOT_LEFT + col * SLOT_STEP,
                y: MAIN_TOP + row * SLOT_STEP,
                block: SlotBlock::Player,
            });
        }
    }
    for col in 0..9i32 {
        slots.push(SlotPos {
            index: base + 27 + col as i16,
            x: SLOT_LEFT + col * SLOT_STEP,
            y: HOTBAR_TOP,
            block: SlotBlock::Player,
        });
    }
    slots
}

/// The chest's rows-dependent shift: `(rows − 4) × 18` (`ContainerChest`:34).
pub fn chest_row_shift(rows: i32) -> i32 {
    (rows - 4) * CHEST_SHIFT_STEP
}

/// The chest's player block: the standard 27+9 shifted by
/// [`chest_row_shift`] (`ContainerChest.java`:34-37 — main at
/// `(8 + j·18, 103 + l·18 + i)`, hotbar at `(8 + i·18, 161 + i)`).
pub fn chest_player_section(base: i16, rows: i32) -> Vec<SlotPos> {
    let shift = chest_row_shift(rows);
    let mut slots = Vec::with_capacity(36);
    for row in 0..3i32 {
        for col in 0..9i32 {
            slots.push(SlotPos {
                index: base + row as i16 * 9 + col as i16,
                x: SLOT_LEFT + col * SLOT_STEP,
                y: CHEST_MAIN_TOP + row * SLOT_STEP + shift,
                block: SlotBlock::Player,
            });
        }
    }
    for col in 0..9i32 {
        slots.push(SlotPos {
            index: base + 27 + col as i16,
            x: SLOT_LEFT + col * SLOT_STEP,
            y: CHEST_HOTBAR_TOP + shift,
            block: SlotBlock::Player,
        });
    }
    slots
}

/// The cursor draw the frame reads: the carried stack, where its top-left
/// lands, and the overlay text (`drawScreen`'s cursor draw at :144-170 with
/// `drawItemStack`'s z-200 cell at :205-214).
#[derive(Debug, Clone, PartialEq)]
pub struct CursorDraw {
    /// The stack to draw, carrying the preview count while a multi-slot drag
    /// runs.
    pub stack: MetadataItem,
    /// The cell's left edge in screen units (pointer − 8).
    pub x: f32,
    /// The cell's top edge in screen units (pointer − 8).
    pub y: f32,
    /// The overlay text: the yellow zero while the drag's remnant is empty
    /// (`§e0` at :164-167 — the source's altText, not the count).
    pub alt_text: Option<String>,
}

/// One title line the frame reads: the text, the panel-local pen and the
/// packed RGB the source's `drawString` takes (4210752 = 0x404040 for the
/// container greys, 14737632 = 0xE0E0E0 for the beacon's pair).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleLine {
    /// The line's text: the window title as sent, or the fixed label.
    pub text: String,
    /// The pen's x in panel units.
    pub x: i32,
    /// The pen's y in panel units.
    pub y: i32,
    /// The packed RGB int (no alpha; the draws are opaque).
    pub colour: u32,
}

/// The last press the double-click rule reads (`lastClickSlot/Time/Button`
/// at :359-365, recorded at :457-459).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LastClick {
    /// The pressed slot's wire index, or `None` outside every slot.
    slot: Option<i16>,
    /// The pressed button's raw code.
    button: i8,
    /// The press's millisecond clock (`Minecraft.getSystemTime`).
    time: u64,
}

/// One covered slot's preview: the wire index, the drawn count, and whether
/// the draw caps it — past the cap the count draws yellow (`drawSlot`'s
/// capped branch at `GuiContainer.java`:253-264, which states `s` as the
/// yellow cap while clamping the stack).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewSlot {
    /// The covered wire index, in cover order.
    pub index: i16,
    /// The drawn count, clamped to the cap.
    pub count: i32,
    /// Whether the raw split ran past the cap: the yellow text's own read.
    pub capped: bool,
}

/// The drag's view side: Task 6's [`DragState`] (mode, packed button, covered
/// slots, remnant cursor) plus the raw pressed button the release compares
/// (`dragSplittingButton` at :558-564) and the per-slot preview counts
/// (`updateDragSplitting`:309-336).
#[derive(Debug, Clone, PartialEq, Eq)]
struct DragRun {
    /// The Task 6 machine: the limit as mode, the packed button, the covered
    /// wire indices (join-once — a slot joins however often crossed, R38) and
    /// the remnant cursor.
    state: DragState,
    /// The raw pressed button, for the release's cancel compare.
    press: ClickButton,
    /// The drag's preview: the wire index, the drawn count and the capped
    /// read, in cover order.
    preview: Vec<PreviewSlot>,
}

/// The drag mode's own base share before any cap: the even split over the
/// set's size, one item, or the dragged item's own cap (`Container.java`'s
/// `computeStackSize` arms at :740-752, without the item cap that call folds
/// in after). The capped read needs the pre-cap base, so it reads these arms
/// directly rather than reusing that call.
fn drag_base(mode: i32, cursor: &MetadataItem, set_len: usize, caps: &impl StackCaps) -> i32 {
    match mode {
        0 => match i32::try_from(set_len) {
            Ok(0) | Err(_) => 0,
            Ok(len) => i32::from(cursor.count) / len,
        },
        1 => 1,
        2 => max_stack_size(cursor, caps),
        _ => i32::from(cursor.count),
    }
}

/// One open container screen: the window it stands on, the view's copies of
/// the slots and the cursor, and the click/drag/hover state.
#[derive(Debug, Clone)]
pub struct ContainerScreen {
    /// The window the screen stands on.
    window_id: u8,
    /// The window's kind.
    kind: WindowKind,
    /// The window's title, exactly as sent.
    title: String,
    /// The panel the screen lays out.
    layout: &'static ContainerLayout,
    /// The view's copy of the window's slots, in wire order.
    slots: Vec<Option<MetadataItem>>,
    /// The view's copy of the window's properties by index (a furnace's burn
    /// and cook state, a brewing stand's brew time): the property draws read
    /// these, an absent index reading 0.
    properties: Vec<i16>,
    /// The view's copy of the carried stack.
    cursor: Option<MetadataItem>,
    /// The pointer in panel-local units.
    mouse: (f32, f32),
    /// The panel's top-left in screen units (centred by default).
    origin: (i32, i32),
    /// The highlight's slot: the draw loop's last match (`theSlot` at :115).
    hovered: Option<i16>,
    /// The drag's view side while one runs (`dragSplitting` at :434-452).
    drag: Option<DragRun>,
    /// The swallowed release (`ignoreMouseUp`, set at :432, cleared at
    /// :566-570).
    ignore_mouse_up: bool,
    /// The last press, for the double-click rule.
    last: Option<LastClick>,
    /// Whether the next left release gathers (`doubleClick`, set at :364).
    double_click: bool,
    /// The shift-clicked stack the double-click's shift arm merges
    /// (`shiftClickedSlot`, recorded at :417 and :633-636).
    shift_clicked: Option<MetadataItem>,
    /// The family-B widget state when the screen stands on one of the five
    /// (`family_b::FamilyState`, stood up by `Screens::open_container` from
    /// the window's kind and entity id; `None` otherwise).
    family: FamilyState,
}

impl ContainerScreen {
    /// Opens the screen on the window: the view starts with no slots and an
    /// empty cursor until the first snapshot lands.
    pub fn new(
        window_id: u8,
        kind: WindowKind,
        title: String,
        layout: &'static ContainerLayout,
    ) -> Self {
        Self {
            window_id,
            kind,
            title,
            layout,
            slots: Vec::new(),
            properties: Vec::new(),
            cursor: None,
            mouse: (0.0, 0.0),
            origin: (0, 0),
            hovered: None,
            drag: None,
            ignore_mouse_up: false,
            last: None,
            double_click: false,
            shift_clicked: None,
            family: FamilyState::None,
        }
    }

    /// The window the screen stands on.
    pub fn window_id(&self) -> u8 {
        self.window_id
    }

    /// The window's kind.
    pub fn kind(&self) -> WindowKind {
        self.kind
    }

    /// Whether the screen draws the generic frame: an unlisted window type
    /// opens a generic container (`NetHandlerPlayClient`:1117-1126).
    pub fn generic_frame(&self) -> bool {
        self.kind == WindowKind::Unknown
    }

    /// The panel the screen lays out.
    pub fn layout(&self) -> &'static ContainerLayout {
        self.layout
    }

    /// Folds one window snapshot into the view's copies.
    pub fn apply_snapshot(
        &mut self,
        slots: Vec<Option<MetadataItem>>,
        cursor: Option<MetadataItem>,
        properties: Vec<i16>,
    ) {
        self.slots = slots;
        self.cursor = cursor;
        self.properties = properties;
    }

    /// Moves the pointer and refreshes the hover and the drag's add rule.
    /// Coordinates are panel-local; the hover is the draw loop's last match.
    pub fn mouse_moved(&mut self, x: f32, y: f32, caps: &impl StackCaps) {
        self.mouse = (x, y);
        self.hovered = hovered_last(self.layout.slots, x, y);
        // The drag's add condition (`mouseClickMove`:505-509): a slot under
        // the pointer, a carried stack bigger than the covered set, the slot
        // taking the stack, valid and draggable — the last three read true
        // for every slot in the base (recorded).
        if self.drag.is_some() {
            let cursor = self.cursor.clone();
            let Some(cursor) = cursor else {
                return;
            };
            let Some(slot) = slot_at_first(self.layout.slots, x, y) else {
                return;
            };
            let stack = self.slot_stack(slot).cloned().flatten();
            let covered = self.drag.as_ref().map_or(0, |drag| drag.state.slots.len());
            if cursor.count as usize <= covered {
                return;
            }
            if !can_add_item_to_slot(&stack, &cursor, max_stack_size(&cursor, caps)) {
                return;
            }
            let joined = {
                let drag = self.drag.as_mut().expect("the drag is armed");
                if !drag.state.slots.contains(&slot) {
                    drag.state.slots.push(slot);
                }
                true
            };
            if joined {
                self.update_preview(caps);
            }
        }
    }

    /// Sets the screen size the panel centres in (`guiLeft/guiTop` at
    /// `GuiContainer.java`:92-93).
    pub fn set_screen_size(&mut self, width: i32, height: i32) {
        self.origin = (
            (width - self.layout.x_size) / 2,
            (height - self.layout.y_size) / 2,
        );
    }

    /// The panel's top-left in screen units.
    pub fn origin(&self) -> (i32, i32) {
        self.origin
    }

    /// The highlight's slot: the draw loop's last match.
    pub fn hovered(&self) -> Option<i16> {
        self.hovered
    }

    /// The click's slot: the first match in slot order.
    pub fn click_slot(&self) -> Option<i16> {
        slot_at_first(self.layout.slots, self.mouse.0, self.mouse.1)
    }

    /// Whether the pointer sits outside the panel (`flag1` at :372).
    pub fn outside(&self) -> bool {
        self.mouse.0 < 0.0
            || self.mouse.1 < 0.0
            || self.mouse.0 >= self.layout.x_size as f32
            || self.mouse.1 >= self.layout.y_size as f32
    }

    /// Whether a drag is armed.
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The drag's covered wire indices, in cover order.
    pub fn drag_slots(&self) -> &[i16] {
        self.drag
            .as_ref()
            .map_or(&[], |drag| drag.state.slots.as_slice())
    }

    /// The drag's preview remnant: the count left on the cursor.
    pub fn remnant_count(&self) -> Option<u8> {
        self.drag
            .as_ref()
            .and_then(|drag| drag.state.remnant.as_ref())
            .map(|stack| stack.count)
    }

    /// The drag's per-slot preview: the wire index, the drawn count and the
    /// capped read, in cover order.
    pub fn preview(&self) -> &[PreviewSlot] {
        self.drag
            .as_ref()
            .map_or(&[], |drag| drag.preview.as_slice())
    }

    /// The view's cursor copy.
    pub fn cursor(&self) -> Option<&MetadataItem> {
        self.cursor.as_ref()
    }

    /// The view's copy of the wire slot's stack.
    pub fn slot_stack(&self, index: i16) -> Option<&Option<MetadataItem>> {
        usize::try_from(index).ok().and_then(|i| self.slots.get(i))
    }

    /// The view's copy of the window's properties by index.
    pub fn properties(&self) -> &[i16] {
        &self.properties
    }

    /// Stands the family-B widget state up (or clears it): the kind dispatch
    /// in `Screens::open_container` owns this.
    pub fn set_family(&mut self, family: FamilyState) {
        self.family = family;
    }

    /// The family-B widget state.
    pub fn family(&self) -> &FamilyState {
        &self.family
    }

    /// The family-B widget state, mutably: the click/key/tick folds own it.
    pub fn family_mut(&mut self) -> &mut FamilyState {
        &mut self.family
    }

    /// The pointer in panel-local units: the widget hit tests read it.
    pub fn panel_mouse(&self) -> (f32, f32) {
        self.mouse
    }

    /// Recomputes the drag's preview into the run (`updateDragSplitting`
    /// :309-336).
    fn update_preview(&mut self, caps: &impl StackCaps) {
        let Some(cursor) = self.cursor.clone() else {
            return;
        };
        let Some(slots) = self.drag.as_ref().map(|drag| drag.state.slots.clone()) else {
            return;
        };
        let Some(mode) = self.drag.as_ref().map(|drag| drag.state.mode) else {
            return;
        };
        let stack_of = |index: i16| self.slot_stack(index).cloned().flatten();
        let others: Vec<Option<MetadataItem>> = slots.iter().map(|slot| stack_of(*slot)).collect();
        let mut remnant = i32::from(cursor.count);
        let mut preview = Vec::with_capacity(slots.len());
        for slot in &slots {
            let held = stack_of(*slot).map_or(0, |stack| i32::from(stack.count));
            let raw = compute_stack_size(mode, &cursor, stack_of(*slot).as_ref(), &others, caps);
            let cap = max_stack_size(&cursor, caps).min(BASE_MAX_STACK_SIZE);
            let size = raw.min(cap);
            remnant -= size - held;
            preview.push(PreviewSlot {
                index: *slot,
                count: size,
                // The capped read compares the mode's own base plus the
                // held count against the caps (`drawSlot`:253-264 states
                // the yellow cap past either one). `compute_stack_size`
                // already folds the item cap in, so the base is re-read
                // from the mode's own arms here.
                capped: drag_base(mode, &cursor, others.len(), caps) + held > cap,
            });
        }
        let mut remnant_stack = cursor;
        remnant_stack.count = remnant.clamp(0, 255) as u8;
        if let Some(drag) = self.drag.as_mut() {
            drag.state.remnant = Some(remnant_stack);
            drag.preview = preview;
        }
    }

    /// Presses a bound button (`mouseClicked`:359-460): the double-click
    /// timing records, then the empty-cursor press clicks (pick → mode 3,
    /// shift over a slot → mode 1, outside → mode 4, else mode 0) while the
    /// carried-stack press arms the local drag and sends nothing. Presses on
    /// no slot (`l == −1`) send nothing. `now_ms` is the source's
    /// `Minecraft.getSystemTime` wall clock.
    pub fn press(&mut self, button: ClickButton, shift: bool, now_ms: u64) -> Vec<InputEvent> {
        let mut events = Vec::new();
        let slot = self.click_slot();
        let slot_id = match slot {
            Some(index) => index,
            None if self.outside() => -999,
            None => -1,
        };
        // The double-click timing (`:359-365`): the same slot, under 250 ms,
        // the same button.
        self.double_click = self.last.is_some_and(|last| {
            last.slot == slot
                && now_ms.wrapping_sub(last.time) < DOUBLE_CLICK_MS
                && last.button == button.raw()
        });
        self.ignore_mouse_up = false;
        if slot_id != -1 && self.drag.is_none() {
            if self.cursor.is_none() {
                if button == ClickButton::Pick {
                    events.push(self.click(slot_id, button.raw(), CLICK_MODE_CREATIVE_PICK));
                } else {
                    // Shift over a real slot quick-moves; shift outside still
                    // throws (`flag2 = l != −999 && shift` at :413).
                    let quick = slot_id != -999 && shift;
                    let mode = if quick {
                        self.shift_clicked =
                            slot.and_then(|index| self.slot_stack(index).cloned().flatten());
                        CLICK_MODE_QUICK_MOVE
                    } else if slot_id == -999 {
                        CLICK_MODE_DROP
                    } else {
                        CLICK_MODE_PICKUP
                    };
                    events.push(self.click(slot_id, button.raw(), mode));
                }
                self.ignore_mouse_up = true;
            } else {
                // The carried-stack press starts the LOCAL drag (`:434-452`):
                // nothing is sent here.
                self.drag = Some(DragRun {
                    state: DragState {
                        mode: button.drag_limit(),
                        button: drag_button(0, button.drag_limit()),
                        slots: Vec::new(),
                        remnant: self.cursor.clone(),
                    },
                    press: button,
                    preview: Vec::new(),
                });
            }
        }
        self.last = Some(LastClick {
            slot,
            button: button.raw(),
            time: now_ms,
        });
        events
    }

    /// Releases a button (`mouseReleased`:515-652): the double-click gather
    /// (shift fans mode 1 out over every matching slot, else mode 6), the
    /// different-button cancel, the swallowed press release, the drag's
    /// 1+n+1 mode-5 batch, and otherwise the carried-stack click (pick →
    /// mode 3, shift → mode 1, else mode 0).
    pub fn release(
        &mut self,
        button: ClickButton,
        shift: bool,
        now_ms: u64,
        caps: &impl StackCaps,
    ) -> Vec<InputEvent> {
        let _ = caps;
        let _ = now_ms;
        let mut events = Vec::new();
        let slot = self.click_slot();
        let slot_id = match slot {
            Some(index) => index,
            None if self.outside() => -999,
            None => -1,
        };
        // The double-click arm (`:536-556`): a flagged press released with
        // the left button over a slot. The base's `canMergeSlot` answers
        // true (`Container.java`:500-503).
        if self.double_click && slot.is_some() && button == ClickButton::Left {
            if shift {
                if let Some(want) = self.shift_clicked.clone() {
                    // The shift arm fans mode 1 out over the matching slots
                    // (`:536-556`): the source's own inventory block, a stack
                    // to take (`canTakeStack` + `getHasStack`), and room for
                    // the wanted stack (`canAddItemToSlot`).
                    let source = slot
                        .and_then(|index| self.layout.slots.iter().find(|pos| pos.index == index));
                    let cap = max_stack_size(&want, caps);
                    for pos in self.layout.slots {
                        if source.is_some_and(|from| pos.block != from.block) {
                            continue;
                        }
                        let held = self.slot_stack(pos.index).cloned().flatten();
                        let Some(held) = held else { continue };
                        if !can_add_item_to_slot(&Some(held), &want, cap) {
                            continue;
                        }
                        events.push(self.click(pos.index, button.raw(), CLICK_MODE_QUICK_MOVE));
                    }
                }
            } else {
                events.push(self.click(slot_id, button.raw(), CLICK_MODE_GATHER));
            }
            self.double_click = false;
            self.last = None;
        } else {
            // A release of another button than the drag's cancels it
            // (`:558-564`): no send, and the next release swallows.
            if self.drag.as_ref().is_some_and(|drag| drag.press != button) {
                self.drag = None;
                self.ignore_mouse_up = true;
                return events;
            }
            if self.ignore_mouse_up {
                self.ignore_mouse_up = false;
                return events;
            }
            // The drag's batch (`:615-625`): start, one per covered slot,
            // end — 1+n+1 packets, all at release.
            if self
                .drag
                .as_ref()
                .is_some_and(|drag| !drag.state.slots.is_empty())
            {
                let drag = self.drag.as_ref().expect("the drag is armed");
                let limit = drag.state.mode;
                events.push(self.click(-999, drag_button(0, limit) as i8, CLICK_MODE_DRAG));
                for slot in drag.state.slots.clone() {
                    events.push(self.click(slot, drag_button(1, limit) as i8, CLICK_MODE_DRAG));
                }
                events.push(self.click(-999, drag_button(2, limit) as i8, CLICK_MODE_DRAG));
            } else if self.cursor.is_some() {
                if button == ClickButton::Pick {
                    events.push(self.click(slot_id, button.raw(), CLICK_MODE_CREATIVE_PICK));
                } else {
                    let quick = slot_id != -999 && shift;
                    if quick {
                        self.shift_clicked =
                            slot.and_then(|index| self.slot_stack(index).cloned().flatten());
                    }
                    events.push(self.click(
                        slot_id,
                        button.raw(),
                        if quick {
                            CLICK_MODE_QUICK_MOVE
                        } else {
                            CLICK_MODE_PICKUP
                        },
                    ));
                }
            }
        }
        if self.cursor.is_none() {
            self.last = None;
        }
        self.drag = None;
        events
    }

    /// Types a screen key (`keyTyped`:692-712): the number keys swap with
    /// mode 2 while the cursor is empty (`checkHotbarKeys`:718-733's gate),
    /// and the drop/pick keys answer over a hovered stack only (mode 4 with
    /// Ctrl for the whole stack, mode 3 with button 0).
    pub fn screen_key(&mut self, key: ScreenKey, ctrl: bool) -> Vec<InputEvent> {
        let mut events = Vec::new();
        match key {
            ScreenKey::Number(index) if index < 9 => {
                if self.cursor.is_none() {
                    if let Some(slot) = self.hovered {
                        events.push(self.click(slot, index as i8, CLICK_MODE_SWAP));
                    }
                }
            }
            ScreenKey::Drop => {
                if let Some(slot) = self.hovered_stack() {
                    events.push(self.click(slot, i8::from(ctrl), CLICK_MODE_DROP));
                }
            }
            ScreenKey::Pick => {
                if let Some(slot) = self.hovered_stack() {
                    events.push(self.click(slot, 0, CLICK_MODE_CREATIVE_PICK));
                }
            }
            ScreenKey::Number(_) => {}
        }
        events
    }

    /// The hovered slot when it carries a stack (`theSlot.getHasStack`).
    fn hovered_stack(&self) -> Option<i16> {
        self.hovered
            .filter(|slot| self.slot_stack(*slot).cloned().flatten().is_some())
    }

    /// One click on the screen's window.
    fn click(&self, slot: i16, button: i8, mode: i8) -> InputEvent {
        InputEvent::ClickWindow {
            window_id: i32::from(self.window_id),
            slot,
            button,
            mode,
        }
    }

    /// The cursor draw the frame reads (`drawScreen`:144-170): the carried
    /// stack at the pointer minus 8, drawing the remnant while a multi-slot
    /// drag runs, with the yellow zero when the remnant is empty.
    pub fn cursor_draw(&self, mouse: (f32, f32)) -> Option<CursorDraw> {
        let cursor = self.cursor.clone()?;
        let multi = self
            .drag
            .as_ref()
            .is_some_and(|drag| drag.state.slots.len() > 1);
        if multi {
            let remnant = self.remnant_count().unwrap_or(cursor.count);
            let mut stack = cursor;
            stack.count = remnant;
            let alt_text = (remnant == 0).then(|| String::from("§e0"));
            Some(CursorDraw {
                stack,
                x: mouse.0 - CURSOR_OFFSET,
                y: mouse.1 - CURSOR_OFFSET,
                alt_text,
            })
        } else {
            Some(CursorDraw {
                stack: cursor,
                x: mouse.0 - CURSOR_OFFSET,
                y: mouse.1 - CURSOR_OFFSET,
                alt_text: None,
            })
        }
    }

    /// The title lines the frame reads, in panel-local units. The centred top
    /// line measures its display text through `measure` (`xSize / 2 −
    /// width / 2`, `GuiDispenser.java`:31-35 and its kin); every other pen is
    /// pinned.
    pub fn title_lines(&self, measure: impl Fn(&str) -> i32) -> Vec<TitleLine> {
        match self.layout.title {
            TitleKind::Chest { lower } => vec![
                TitleLine {
                    text: self.title.clone(),
                    x: 8,
                    y: 6,
                    colour: TITLE_GREY,
                },
                TitleLine {
                    text: match lower {
                        TitleSource::WindowTitle => self.title.clone(),
                        TitleSource::Fixed(label) => String::from(label),
                    },
                    x: 8,
                    y: self.layout.y_size - 96 + 2,
                    colour: TITLE_GREY,
                },
            ],
            TitleKind::Centred { lower } => vec![
                TitleLine {
                    text: self.title.clone(),
                    x: self.layout.x_size / 2 - measure(self.title.as_str()) / 2,
                    y: 6,
                    colour: TITLE_GREY,
                },
                TitleLine {
                    text: match lower {
                        TitleSource::WindowTitle => self.title.clone(),
                        TitleSource::Fixed(label) => String::from(label),
                    },
                    x: 8,
                    y: self.layout.y_size - 96 + 2,
                    colour: TITLE_GREY,
                },
            ],
            TitleKind::Crafting { top, lower } => vec![
                TitleLine {
                    text: match top {
                        TitleSource::WindowTitle => self.title.clone(),
                        TitleSource::Fixed(label) => String::from(label),
                    },
                    x: 28,
                    y: 6,
                    colour: TITLE_GREY,
                },
                TitleLine {
                    text: match lower {
                        TitleSource::WindowTitle => self.title.clone(),
                        TitleSource::Fixed(label) => String::from(label),
                    },
                    x: 8,
                    y: self.layout.y_size - 96 + 2,
                    colour: TITLE_GREY,
                },
            ],
            TitleKind::Inventory { label } => vec![TitleLine {
                text: match label {
                    TitleSource::WindowTitle => self.title.clone(),
                    TitleSource::Fixed(text) => String::from(text),
                },
                x: 86,
                y: 16,
                colour: TITLE_GREY,
            }],
            TitleKind::Beacon { primary, secondary } => {
                let prime = match primary {
                    TitleSource::WindowTitle => self.title.clone(),
                    TitleSource::Fixed(label) => String::from(label),
                };
                let second = match secondary {
                    TitleSource::WindowTitle => self.title.clone(),
                    TitleSource::Fixed(label) => String::from(label),
                };
                vec![
                    TitleLine {
                        x: 62 - measure(prime.as_str()) / 2,
                        y: 10,
                        text: prime,
                        colour: BEACON_TITLE_GREY,
                    },
                    TitleLine {
                        x: 169 - measure(second.as_str()) / 2,
                        y: 10,
                        text: second,
                        colour: BEACON_TITLE_GREY,
                    },
                ]
            }
            TitleKind::Enchanting { upper } => vec![
                TitleLine {
                    text: match upper {
                        TitleSource::WindowTitle => self.title.clone(),
                        TitleSource::Fixed(label) => String::from(label),
                    },
                    x: 12,
                    y: 5,
                    colour: TITLE_GREY,
                },
                TitleLine {
                    text: String::from("Inventory"),
                    x: 8,
                    y: self.layout.y_size - 96 + 2,
                    colour: TITLE_GREY,
                },
            ],
            TitleKind::Anvil { top, lower } => vec![
                TitleLine {
                    text: match top {
                        TitleSource::WindowTitle => self.title.clone(),
                        TitleSource::Fixed(label) => String::from(label),
                    },
                    x: 60,
                    y: 6,
                    colour: TITLE_GREY,
                },
                TitleLine {
                    text: match lower {
                        TitleSource::WindowTitle => self.title.clone(),
                        TitleSource::Fixed(label) => String::from(label),
                    },
                    x: 8,
                    y: self.layout.y_size - 96 + 2,
                    colour: TITLE_GREY,
                },
            ],
            TitleKind::Generic => vec![TitleLine {
                text: self.title.clone(),
                x: 8,
                y: 6,
                colour: TITLE_GREY,
            }],
        }
    }
}

#[cfg(test)]
mod unit {
    //! The base's own pins: the constants, the cursor draw's offset, the
    //! title lines' pens and the panel's centring.

    use super::*;
    use oxide_game::container::BaseStackCaps;

    static PINS: &[SlotPos] = &[SlotPos {
        index: 0,
        x: 8,
        y: 18,
        block: SlotBlock::Container,
    }];
    static PIN_LAYOUT: ContainerLayout = ContainerLayout {
        x_size: 176,
        y_size: 166,
        sheet: "unit/panel",
        slots: PINS,
        title: TitleKind::Generic,
        background: BackgroundKind::Full,
    };

    #[test]
    fn the_generic_frame_is_the_default_panel() {
        assert_eq!((GENERIC_LAYOUT.x_size, GENERIC_LAYOUT.y_size), (176, 166));
        assert!(
            GENERIC_LAYOUT.slots.is_empty(),
            "no slot table until Tasks 18-20"
        );
    }

    #[test]
    fn the_button_codes_match_the_source() {
        assert_eq!(ClickButton::Left.raw(), 0);
        assert_eq!(ClickButton::Right.raw(), 1);
        assert_eq!(ClickButton::Pick.raw(), PICK_BUTTON_CODE);
        assert_eq!(PICK_BUTTON_OFFSET, 100, "the binding test adds 100");
        assert_eq!(
            (
                ClickButton::Left.drag_limit(),
                ClickButton::Right.drag_limit(),
                ClickButton::Pick.drag_limit()
            ),
            (0, 1, 2)
        );
        assert_eq!(DOUBLE_CLICK_MS, 250);
    }

    #[test]
    fn the_cursor_draws_eight_up_and_left_of_the_pointer() {
        let mut screen = ContainerScreen::new(1, WindowKind::Chest, String::new(), &PIN_LAYOUT);
        screen.apply_snapshot(vec![None], Some(stack(1, 4)), Vec::new());
        let draw = screen.cursor_draw((100.0, 50.0)).expect("a cursor draws");
        assert_eq!((draw.x, draw.y), (92.0, 42.0));
        assert_eq!(draw.stack.count, 4);
        assert_eq!(draw.alt_text, None);
    }

    #[test]
    fn a_single_slot_drag_draws_the_cursor_whole() {
        let mut screen = ContainerScreen::new(1, WindowKind::Chest, String::new(), &PIN_LAYOUT);
        screen.apply_snapshot(vec![None], Some(stack(1, 1)), Vec::new());
        screen.mouse_moved(16.0, 26.0, &BaseStackCaps);
        screen.press(ClickButton::Left, false, 1_000);
        // A lone carried item dragged over one slot: the set holds one, so
        // the cursor still draws whole.
        screen.mouse_moved(16.0, 26.0, &BaseStackCaps);
        let draw = screen.cursor_draw((100.0, 50.0)).expect("a cursor draws");
        assert_eq!(draw.stack.count, 1, "a single-slot drag draws whole");
        assert_eq!(draw.alt_text, None);
    }

    #[test]
    fn the_empty_remnant_draws_the_yellow_zero() {
        static TWO: &[SlotPos] = &[
            SlotPos {
                index: 0,
                x: 8,
                y: 18,
                block: SlotBlock::Container,
            },
            SlotPos {
                index: 1,
                x: 26,
                y: 18,
                block: SlotBlock::Container,
            },
        ];
        static TWO_LAYOUT: ContainerLayout = ContainerLayout {
            x_size: 176,
            y_size: 166,
            sheet: "unit/pair",
            slots: TWO,
            title: TitleKind::Generic,
            background: BackgroundKind::Full,
        };
        let mut screen = ContainerScreen::new(1, WindowKind::Chest, String::new(), &TWO_LAYOUT);
        screen.apply_snapshot(vec![None, None], Some(stack(1, 2)), Vec::new());
        screen.mouse_moved(16.0, 26.0, &BaseStackCaps);
        screen.press(ClickButton::Left, false, 1_000);
        screen.mouse_moved(16.0, 26.0, &BaseStackCaps);
        screen.mouse_moved(34.0, 26.0, &BaseStackCaps);
        assert_eq!(screen.remnant_count(), Some(0), "two split one each");
        let draw = screen.cursor_draw((100.0, 50.0)).expect("a cursor draws");
        assert_eq!(draw.stack.count, 0);
        assert_eq!(draw.alt_text, Some(String::from("§e0")));
    }

    #[test]
    fn the_capped_preview_marks_its_slots() {
        static CAPPED_TWO: &[SlotPos] = &[
            SlotPos {
                index: 0,
                x: 8,
                y: 18,
                block: SlotBlock::Container,
            },
            SlotPos {
                index: 1,
                x: 26,
                y: 18,
                block: SlotBlock::Container,
            },
        ];
        static CAPPED_LAYOUT: ContainerLayout = ContainerLayout {
            x_size: 176,
            y_size: 166,
            sheet: "unit/capped",
            slots: CAPPED_TWO,
            title: TitleKind::Generic,
            background: BackgroundKind::Full,
        };
        // Sixty-four shovels (cap 1) split evenly over two slots: the raw
        // thirty-two runs past the cap, so each preview draws capped at one
        // (`drawSlot`:253-264 states the yellow cap while clamping).
        let mut screen = ContainerScreen::new(1, WindowKind::Chest, String::new(), &CAPPED_LAYOUT);
        screen.apply_snapshot(vec![None, None], Some(stack(256, 64)), Vec::new());
        screen.mouse_moved(16.0, 26.0, &crate::items::ItemTable);
        screen.press(ClickButton::Left, false, 1_000);
        screen.mouse_moved(16.0, 26.0, &crate::items::ItemTable);
        screen.mouse_moved(34.0, 26.0, &crate::items::ItemTable);
        let preview = screen.preview();
        assert_eq!(preview.len(), 2, "both covered slots preview");
        for entry in preview {
            assert_eq!(entry.count, 1, "the split clamps to the cap");
            assert!(entry.capped, "thirty-two past a cap of one caps");
        }
    }

    #[test]
    fn the_panel_centres_in_the_screen() {
        let mut screen = ContainerScreen::new(1, WindowKind::Chest, String::new(), &PIN_LAYOUT);
        screen.set_screen_size(400, 300);
        assert_eq!(screen.origin(), ((400 - 176) / 2, (300 - 166) / 2));
    }

    #[test]
    fn the_chest_titles_pen_the_two_lines() {
        static CHEST_LAYOUT: ContainerLayout = ContainerLayout {
            x_size: 176,
            y_size: 168,
            sheet: "unit/chest",
            slots: PINS,
            title: TitleKind::Chest {
                lower: TitleSource::Fixed("Inventory"),
            },
            background: BackgroundKind::Full,
        };
        let screen =
            ContainerScreen::new(1, WindowKind::Chest, String::from("Chest"), &CHEST_LAYOUT);
        // The top line is the window's own title (the chest inventory's
        // name, as the server sent it); the bottom line is the player
        // inventory's name (`GuiChest.java`:36-40, order corrected in Task
        // 18 — the ctor's `upperInv`/`lowerChestInventory` names are
        // inverted vs physical position).
        assert_eq!(
            screen.title_lines(|_| 0),
            vec![
                TitleLine {
                    text: String::from("Chest"),
                    x: 8,
                    y: 6,
                    colour: TITLE_GREY,
                },
                TitleLine {
                    text: String::from("Inventory"),
                    x: 8,
                    y: 168 - 96 + 2,
                    colour: TITLE_GREY,
                },
            ]
        );
    }

    #[test]
    fn the_centred_title_halves_the_measured_width() {
        static CENTRED_LAYOUT: ContainerLayout = ContainerLayout {
            x_size: 176,
            y_size: 166,
            sheet: "unit/furnace",
            slots: PINS,
            title: TitleKind::Centred {
                lower: TitleSource::Fixed("Inventory"),
            },
            background: BackgroundKind::Full,
        };
        let screen = ContainerScreen::new(
            1,
            WindowKind::Furnace,
            String::from("Furnace"),
            &CENTRED_LAYOUT,
        );
        // A 60-wide top line centres at 176 / 2 − 60 / 2 = 58
        // (`GuiFurnace.java`:31-35); the player line stays left at (8, 72).
        assert_eq!(
            screen.title_lines(|_| 60),
            vec![
                TitleLine {
                    text: String::from("Furnace"),
                    x: 58,
                    y: 6,
                    colour: TITLE_GREY,
                },
                TitleLine {
                    text: String::from("Inventory"),
                    x: 8,
                    y: 166 - 96 + 2,
                    colour: TITLE_GREY,
                },
            ]
        );
    }

    #[test]
    fn the_crafting_titles_are_the_fixed_pair() {
        static CRAFTING_LAYOUT: ContainerLayout = ContainerLayout {
            x_size: 176,
            y_size: 166,
            sheet: "unit/crafting",
            slots: PINS,
            title: TitleKind::Crafting {
                top: TitleSource::Fixed("Crafting"),
                lower: TitleSource::Fixed("Inventory"),
            },
            background: BackgroundKind::Full,
        };
        let screen = ContainerScreen::new(
            0,
            WindowKind::CraftingTable,
            String::from("ignored server title"),
            &CRAFTING_LAYOUT,
        );
        // The family's only fixed top label: `container.crafting` at (28, 6),
        // the player label below — the server title draws nowhere
        // (`GuiCrafting.java`:32-36).
        assert_eq!(
            screen.title_lines(|_| 0),
            vec![
                TitleLine {
                    text: String::from("Crafting"),
                    x: 28,
                    y: 6,
                    colour: TITLE_GREY,
                },
                TitleLine {
                    text: String::from("Inventory"),
                    x: 8,
                    y: 166 - 96 + 2,
                    colour: TITLE_GREY,
                },
            ]
        );
    }

    #[test]
    fn the_inventory_title_pens_the_crafting_label() {
        static INV_LAYOUT: ContainerLayout = ContainerLayout {
            x_size: 176,
            y_size: 166,
            sheet: "unit/inventory",
            slots: PINS,
            title: TitleKind::Inventory {
                label: TitleSource::Fixed("Crafting"),
            },
            background: BackgroundKind::Full,
        };
        let screen = ContainerScreen::new(0, WindowKind::Container, String::new(), &INV_LAYOUT);
        assert_eq!(
            screen.title_lines(|_| 0),
            vec![TitleLine {
                text: String::from("Crafting"),
                x: 86,
                y: 16,
                colour: TITLE_GREY,
            }]
        );
    }

    fn stack(id: i16, count: u8) -> MetadataItem {
        MetadataItem {
            id,
            count,
            damage: 0,
            nbt: None,
        }
    }
}
