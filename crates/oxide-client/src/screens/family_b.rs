//! The container family B: the beacon, enchanting table, villager, horse and
//! anvil layouts with their interactions.
//!
//! The port reads values and names from the source's containers
//! (`ContainerBeacon`, `ContainerEnchantment`, `ContainerMerchant`,
//! `ContainerHorseInventory`, `ContainerRepair`) and their GUIs (`GuiBeacon`,
//! `GuiEnchantment`, `GuiMerchant`, `GuiScreenHorseInventory`, `GuiRepair`);
//! no source text is copied. Three corrections the homework scout re-verified
//! first-hand shape this port: the villager is a single-recipe pager (no
//! seven-row list, no scroll, no per-row clicks), the anvil renames through a
//! per-keystroke `MC|ItemName` send (not the output click), and the glyphs are
//! one reseeded word list (no cloud lists, no glyph walk — the ticked motion
//! is the book's).
//!
//! The three C17 confirms ride one generic
//! [`InputEvent::CustomPayload`](oxide_game::input::InputEvent::CustomPayload)
//! with the three encoders at the call sites ([`beacon_data`],
//! [`trsel_data`], [`item_name_data`]): the beacon's two big-endian i32s, the
//! villager's one big-endian i32, and the anvil's varint-prefixed UTF-8 string
//! (the landed `write_plugin_message`'s string rule).
//!
//! The villager's out-of-stock lock derives from the landed offer fields:
//! `MerchantRecipe.isRecipeDisabled` reads `uses >= maxUses`, which is the bit
//! the packet's disabled flag carries, so no proto change was needed (see
//! [`MerchantOffer::is_disabled`](oxide_proto_v47::window::MerchantOffer::is_disabled)).
//!
//! Recorded reductions: the enchanting book's 3D model draw becomes cover and
//! page rects driven by the ported tick floats; the glyph name draws in the
//! normal font (the SGA sheet is an asset reference only); the anvil's cost
//! line draws its number (the port carries no I18n sentences); the horse's
//! live preview is Task 20's `drawEntityOnScreen`, so the port stores the
//! window's entity id and exposes the preview anchor; the creative bypasses
//! read survival always (no gamemode feed reaches the screens); widget clicks
//! answer the left press (the source's button arm takes any press).

use oxide_game::input::{InputEvent, Key};
use oxide_proto::varint::write_varint;
use oxide_proto_v47::entity::MetadataItem;
use oxide_proto_v47::window::{MerchantOffer, WindowKind};

use super::container::{
    BackgroundKind, ClickButton, ContainerLayout, ContainerScreen, GENERIC_LAYOUT, SlotBlock,
    SlotPos, TitleKind, TitleSource,
};
use crate::enchants::enchant_line;
use crate::tooltip::display_name;

/// The beacon confirm's channel (`GuiBeacon.actionPerformed`:137-144).
pub const BEACON_CHANNEL: &str = "MC|Beacon";
/// The villager page select's channel (`GuiMerchant.actionPerformed`:126-132).
pub const TRSEL_CHANNEL: &str = "MC|TrSel";
/// The anvil rename's channel (`GuiRepair.renameItem`:136-148).
pub const ITEM_NAME_CHANNEL: &str = "MC|ItemName";

/// The zeroed cell the table builders overwrite.
const FILLER: SlotPos = SlotPos {
    index: 0,
    x: 0,
    y: 0,
    block: SlotBlock::Container,
};

/// Writes the standard 27+9 player block at wire `base` into `out` from
/// `start` (`ContainerPlayer.java`:36-67 — main at `(8 + j·18, 84 + l·18)`,
/// hotbar at `(8 + i·18, 142)`).
const fn player_block(out: &mut [SlotPos], base: i16, start: usize) {
    let mut row: i32 = 0;
    while row < 3 {
        let mut col: i32 = 0;
        while col < 9 {
            out[start + (row * 9 + col) as usize] = SlotPos {
                index: base + (row * 9 + col) as i16,
                x: 8 + col * 18,
                y: 84 + row * 18,
                block: SlotBlock::Player,
            };
            col += 1;
        }
        row += 1;
    }
    let mut col: i32 = 0;
    while col < 9 {
        out[start + (27 + col) as usize] = SlotPos {
            index: base + (27 + col) as i16,
            x: 8 + col * 18,
            y: 142,
            block: SlotBlock::Player,
        };
        col += 1;
    }
}

/// The beacon's payment slot plus the 27+9 player block at base 1: the
/// payment `BeaconSlot` at (136, 110) (`ContainerBeacon.java`:19), the main
/// rows at `(36 + l·18, 137 + k·18)` (`:20-29`) and the hotbar at y 195
/// (`58 + 137`, `:31-34`).
const fn beacon_slots() -> [SlotPos; 37] {
    let mut out = [FILLER; 37];
    out[0] = SlotPos {
        index: 0,
        x: 136,
        y: 110,
        block: SlotBlock::Container,
    };
    let mut row: i32 = 0;
    while row < 3 {
        let mut col: i32 = 0;
        while col < 9 {
            out[(1 + row * 9 + col) as usize] = SlotPos {
                index: 1 + (row * 9 + col) as i16,
                x: 36 + col * 18,
                y: 137 + row * 18,
                block: SlotBlock::Player,
            };
            col += 1;
        }
        row += 1;
    }
    let mut col: i32 = 0;
    while col < 9 {
        out[(28 + col) as usize] = SlotPos {
            index: (28 + col) as i16,
            x: 36 + col * 18,
            y: 195,
            block: SlotBlock::Player,
        };
        col += 1;
    }
    out
}

static BEACON_SLOTS: [SlotPos; 37] = beacon_slots();

/// The beacon's layout: the 230×219 panel (`GuiBeacon.java`:36-37) on the
/// `beacon` sheet, with the tile-named centred pair.
pub static BEACON: ContainerLayout = ContainerLayout {
    x_size: 230,
    y_size: 219,
    sheet: "gui/container/beacon",
    slots: &BEACON_SLOTS,
    title: TitleKind::Beacon {
        primary: TitleSource::Fixed("Primary Effect"),
        secondary: TitleSource::Fixed("Secondary Effect"),
    },
    background: BackgroundKind::Full,
};

/// The enchanting table's two slots — the item at (15, 47), the lapis at
/// (35, 47) (`ContainerEnchantment.java`:57-74) — then the standard player
/// block at base 2 (`:76-87`).
const fn enchanting_slots() -> [SlotPos; 38] {
    let mut out = [FILLER; 38];
    out[0] = SlotPos {
        index: 0,
        x: 15,
        y: 47,
        block: SlotBlock::Container,
    };
    out[1] = SlotPos {
        index: 1,
        x: 35,
        y: 47,
        block: SlotBlock::Container,
    };
    player_block(&mut out, 2, 2);
    out
}

static ENCHANTING_SLOTS: [SlotPos; 38] = enchanting_slots();

/// The enchanting table's layout: the standard 176×166 panel on the
/// `enchanting_table` sheet, the table's own title on top.
pub static ENCHANTING: ContainerLayout = ContainerLayout {
    x_size: 176,
    y_size: 166,
    sheet: "gui/container/enchanting_table",
    slots: &ENCHANTING_SLOTS,
    title: TitleKind::Enchanting {
        upper: TitleSource::WindowTitle,
    },
    background: BackgroundKind::Full,
};

/// The villager's three — the buys at (36, 53) and (62, 53), the
/// `SlotMerchantResult` at (120, 53) (`ContainerMerchant.java`:23-25) — then
/// the standard player block at base 3 (`:27-38`).
const fn villager_slots() -> [SlotPos; 39] {
    let mut out = [FILLER; 39];
    out[0] = SlotPos {
        index: 0,
        x: 36,
        y: 53,
        block: SlotBlock::Container,
    };
    out[1] = SlotPos {
        index: 1,
        x: 62,
        y: 53,
        block: SlotBlock::Container,
    };
    out[2] = SlotPos {
        index: 2,
        x: 120,
        y: 53,
        block: SlotBlock::Container,
    };
    player_block(&mut out, 3, 3);
    out
}

static VILLAGER_SLOTS: [SlotPos; 39] = villager_slots();

/// The villager's layout: the standard panel on the `villager` sheet, the
/// merchant's centred name on top.
pub static VILLAGER: ContainerLayout = ContainerLayout {
    x_size: 176,
    y_size: 166,
    sheet: "gui/container/villager",
    slots: &VILLAGER_SLOTS,
    title: TitleKind::Centred {
        lower: TitleSource::Fixed("Inventory"),
    },
    background: BackgroundKind::Full,
};

/// The horse's two — the saddle at (8, 18), the armour at (8, 36)
/// (`ContainerHorseInventory.java`:20-37) — then the standard player block at
/// base 2 (`:50-61`, rows at 84/102/120 with `j = −18`, hotbar at 142).
const fn horse_slots() -> [SlotPos; 38] {
    let mut out = [FILLER; 38];
    out[0] = SlotPos {
        index: 0,
        x: 8,
        y: 18,
        block: SlotBlock::Container,
    };
    out[1] = SlotPos {
        index: 1,
        x: 8,
        y: 36,
        block: SlotBlock::Container,
    };
    player_block(&mut out, 2, 2);
    out
}

static HORSE_SLOTS: [SlotPos; 38] = horse_slots();

/// The horse's layout: the standard panel on the `horse` sheet for a horse
/// without a chest.
pub static HORSE: ContainerLayout = ContainerLayout {
    x_size: 176,
    y_size: 166,
    sheet: "gui/container/horse",
    slots: &HORSE_SLOTS,
    title: TitleKind::Chest {
        lower: TitleSource::Fixed("Inventory"),
    },
    background: BackgroundKind::Full,
};

/// The chested horse's fifteen — `2 + l + k·5` at `(80 + l·18, 18 + k·18)`
/// (`:39-48`) — between the saddle/armour pair and the player block at base
/// 17.
const fn horse_chested_slots() -> [SlotPos; 53] {
    let mut out = [FILLER; 53];
    out[0] = SlotPos {
        index: 0,
        x: 8,
        y: 18,
        block: SlotBlock::Container,
    };
    out[1] = SlotPos {
        index: 1,
        x: 8,
        y: 36,
        block: SlotBlock::Container,
    };
    let mut row: i32 = 0;
    while row < 3 {
        let mut col: i32 = 0;
        while col < 5 {
            out[(2 + row * 5 + col) as usize] = SlotPos {
                index: 2 + (row * 5 + col) as i16,
                x: 80 + col * 18,
                y: 18 + row * 18,
                block: SlotBlock::Container,
            };
            col += 1;
        }
        row += 1;
    }
    player_block(&mut out, 17, 17);
    out
}

static HORSE_CHESTED_TABLE: [SlotPos; 53] = horse_chested_slots();

/// The chested horse's layout: the same sheet with the fifteen chest slots.
pub static HORSE_CHESTED: ContainerLayout = ContainerLayout {
    x_size: 176,
    y_size: 166,
    sheet: "gui/container/horse",
    slots: &HORSE_CHESTED_TABLE,
    title: TitleKind::Chest {
        lower: TitleSource::Fixed("Inventory"),
    },
    background: BackgroundKind::Full,
};

/// The unchested horse window's slot count: saddle, armour and the 36 player
/// slots (`ContainerHorseInventory.java`:39-48 adds the fifteen only when
/// `isChested`).
pub const HORSE_PLAIN_SLOTS: u8 = 38;
/// The chested horse window's slot count: 2 + 15 + 36.
pub const HORSE_CHESTED_SLOTS: u8 = 53;

/// Whether the horse window's slot count names the chested table: anything
/// past the plain pair carries chest slots.
pub fn horse_chested(slot_count: u8) -> bool {
    slot_count > HORSE_PLAIN_SLOTS
}

/// The anvil's three — the inputs at (27, 47) and (76, 47), the output at
/// (134, 47) (`ContainerRepair.java`:63-65) — then the standard player block
/// at base 3 (`:129-140`, main at y 84, hotbar at 142).
const fn anvil_slots() -> [SlotPos; 39] {
    let mut out = [FILLER; 39];
    out[0] = SlotPos {
        index: 0,
        x: 27,
        y: 47,
        block: SlotBlock::Container,
    };
    out[1] = SlotPos {
        index: 1,
        x: 76,
        y: 47,
        block: SlotBlock::Container,
    };
    out[2] = SlotPos {
        index: 2,
        x: 134,
        y: 47,
        block: SlotBlock::Container,
    };
    player_block(&mut out, 3, 3);
    out
}

static ANVIL_SLOTS: [SlotPos; 39] = anvil_slots();

/// The anvil's layout: the standard panel on the `anvil` sheet (no
/// `xSize`/`ySize` override — `GuiRepair.java` keeps 176×166), the fixed
/// repair label on top.
pub static ANVIL: ContainerLayout = ContainerLayout {
    x_size: 176,
    y_size: 166,
    sheet: "gui/container/anvil",
    slots: &ANVIL_SLOTS,
    title: TitleKind::Anvil {
        top: TitleSource::Fixed("Repair"),
        lower: TitleSource::Fixed("Inventory"),
    },
    background: BackgroundKind::Full,
};

/// Picks the family-B layout for an opened window: the horse's chest table
/// rides the window's slot count; every other family-B kind has one table.
/// Kinds outside the family fall back to the generic frame (the dispatch in
/// `super` only calls this for the five).
pub fn layout_for_kind(kind: WindowKind, slot_count: u8) -> &'static ContainerLayout {
    match kind {
        WindowKind::Beacon => &BEACON,
        WindowKind::EnchantingTable => &ENCHANTING,
        WindowKind::Villager => &VILLAGER,
        WindowKind::Anvil => &ANVIL,
        WindowKind::EntityHorse if horse_chested(slot_count) => &HORSE_CHESTED,
        WindowKind::EntityHorse => &HORSE,
        _ => &GENERIC_LAYOUT,
    }
}

/// The beacon's primary effect rows: `TileEntityBeacon.effectsList`'s first
/// three rows — speed/haste, resistance/jump-boost, strength
/// (`TileEntityBeacon.java`:33) as potion ids.
static BEACON_ROW_0: [i32; 2] = [1, 3];
static BEACON_ROW_1: [i32; 2] = [11, 8];
static BEACON_ROW_2: [i32; 1] = [5];
/// The three primary rows as one table, tier by tier.
pub const BEACON_PRIMARY: [&[i32]; 3] = [&BEACON_ROW_0, &BEACON_ROW_1, &BEACON_ROW_2];
/// The beacon's secondary row: regeneration alone (`:33`).
pub const BEACON_SECONDARY: &[i32] = &[10];

/// The confirm button's panel rect (`GuiBeacon.initGui`:47-48): id −1 at
/// (164, 107), 22×22.
pub const BEACON_CONFIRM_POS: (i32, i32) = (164, 107);
/// The cancel button's panel rect (`:48`): id −2 at (190, 107), 22×22.
pub const BEACON_CANCEL_POS: (i32, i32) = (190, 107);
/// The buttons' side (`Button`:220's 22×22).
pub const BEACON_BUTTON_SIDE: i32 = 22;
/// The button state strip's sheet row (`drawButton`:233-249 draws at v 219).
pub const BEACON_STRIP_V: i32 = 219;
/// The confirm icon's sheet origin (90, 220, `ConfirmButton`:285).
pub const BEACON_CONFIRM_UV: (i32, i32) = (90, 220);
/// The cancel icon's sheet origin (112, 220, `CancelButton`:273).
pub const BEACON_CANCEL_UV: (i32, i32) = (112, 220);

/// One beacon effect button: the source's `PowerButton` id (`tier << 8 |
/// effect`), its panel rect and its live state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BeaconRow {
    /// The button id the click decodes (`l << 8 | l1`, `:74`).
    pub id: i32,
    /// The potion effect id the button stands for.
    pub effect: i32,
    /// The effect row's tier (0-2 primary, 3 secondary).
    pub tier: i32,
    /// The button's left edge in panel units.
    pub x: i32,
    /// The button's top edge in panel units.
    pub y: i32,
    /// Whether the pyramid's levels unlock the row (`l >= levels` disables,
    /// `:78-81`).
    pub enabled: bool,
    /// Whether the row is the chosen effect (`l1 == field`, `:82-85`).
    pub selected: bool,
}

/// The beacon's effect rows for the pyramid's `levels` and the chosen
/// `primary`/`secondary` ids (`GuiBeacon.updateScreen`:63-124): the three
/// primary rows centred on x 76 at y `22 + l·25`, the secondary row at y 47
/// centred on x 167, and the primary-echo button when `primary > 0`
/// (`:109-122`).
pub fn beacon_rows(levels: i32, primary: i32, secondary: i32) -> Vec<BeaconRow> {
    let mut rows = Vec::new();
    for (row, effects) in BEACON_PRIMARY.iter().enumerate() {
        let tier = row as i32;
        let count = effects.len() as i32;
        let width = count * 22 + (count - 1) * 2;
        for (slot, effect) in effects.iter().enumerate() {
            rows.push(BeaconRow {
                id: (tier << 8) | effect,
                effect: *effect,
                tier,
                x: 76 + slot as i32 * 24 - width / 2,
                y: 22 + tier * 25,
                enabled: tier < levels,
                selected: *effect == primary,
            });
        }
    }
    let tier = 3;
    let count = BEACON_SECONDARY.len() as i32 + 1;
    let width = count * 22 + (count - 1) * 2;
    for (slot, effect) in BEACON_SECONDARY.iter().enumerate() {
        rows.push(BeaconRow {
            id: (tier << 8) | effect,
            effect: *effect,
            tier,
            x: 167 + slot as i32 * 24 - width / 2,
            y: 47,
            enabled: tier < levels,
            selected: *effect == secondary,
        });
    }
    if primary > 0 {
        rows.push(BeaconRow {
            id: (tier << 8) | primary,
            effect: primary,
            tier,
            x: 167 + (count - 1) * 24 - width / 2,
            y: 47,
            enabled: tier < levels,
            selected: primary == secondary,
        });
    }
    rows
}

/// The beacon button under the panel point, if any: the 22×22 hitbox each
/// `Button` draws (`drawButton`:231 tests the 22×22 rect).
pub fn beacon_hit(rows: &[BeaconRow], x: i32, y: i32) -> Option<i32> {
    rows.iter()
        .find(|row| {
            x >= row.x
                && x < row.x + BEACON_BUTTON_SIDE
                && y >= row.y
                && y < row.y + BEACON_BUTTON_SIDE
        })
        .map(|row| row.id)
}

/// Folds one power-button click into the local selection
/// (`actionPerformed`:145-168): tiers 0-2 name the primary, tier 3 the
/// secondary — a local `setField`, no send. An already-selected button is a
/// no-op (`func_146141_c`, `:147-150`); the button list rebuilds after.
pub fn beacon_select(selection: &mut BeaconSelection, rows: &[BeaconRow], id: i32) {
    let Some(row) = rows.iter().find(|row| row.id == id) else {
        return;
    };
    if row.selected {
        return;
    }
    if row.tier < 3 {
        selection.primary = row.effect;
    } else {
        selection.secondary = row.effect;
    }
}

/// Whether the confirm button is enabled: the payment slot holds a stack and
/// a primary stands chosen (`updateScreen`:125).
pub fn beacon_confirm_enabled(payment_present: bool, primary: i32) -> bool {
    payment_present && primary > 0
}

/// The button state strip's sheet x: 0 enabled, +width selected, +2·width
/// disabled, +3·width hovered (`drawButton`:233-249).
pub fn beacon_button_u(enabled: bool, selected: bool, hovered: bool) -> i32 {
    if !enabled {
        BEACON_BUTTON_SIDE * 2
    } else if selected {
        BEACON_BUTTON_SIDE
    } else if hovered {
        BEACON_BUTTON_SIDE * 3
    } else {
        0
    }
}

/// Frames the beacon confirm's body: the two chosen fields as big-endian
/// i32s (`PacketBuffer.writeInt`, `actionPerformed`:139-141).
pub fn beacon_data(primary: i32, secondary: i32) -> Vec<u8> {
    let mut data = Vec::with_capacity(8);
    data.extend_from_slice(&primary.to_be_bytes());
    data.extend_from_slice(&secondary.to_be_bytes());
    data
}

/// The beacon confirm's send on [`BEACON_CHANNEL`].
pub fn beacon_event(primary: i32, secondary: i32) -> InputEvent {
    InputEvent::CustomPayload {
        channel: String::from(BEACON_CHANNEL),
        data: beacon_data(primary, secondary),
    }
}

/// The beacon's local selection: the two chosen effect ids, seeded from the
/// window's properties 1/2 (`updateScreen`:59-61 reads fields 0-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BeaconSelection {
    /// The chosen primary effect id (field 1).
    pub primary: i32,
    /// The chosen secondary effect id (field 2).
    pub secondary: i32,
}

/// The enchanting offer rows' left edge (`mouseClicked`:91-99 tests
/// `mouseX − (i + 60)`).
pub const ENCHANT_ROW_X: i32 = 60;
/// The rows' width and height: the 108×19 hitbox (`:94`).
pub const ENCHANT_ROW_W: i32 = 108;
/// The rows' width and height, paired with [`ENCHANT_ROW_W`].
pub const ENCHANT_ROW_H: i32 = 19;

/// One offer row's top edge: `guiTop + 14 + 19·k` (`:93`).
pub fn enchant_row_y(k: i32) -> i32 {
    14 + 19 * k
}

/// The empty row's sprite row: `(0, 185, 108×19)` when the cost is 0
/// (`drawGuiContainerBackgroundLayer`:186-188).
pub const ENCHANT_EMPTY_V: i32 = 185;
/// The affordable row's sprite row: `(0, 166)` (`:205-221`).
pub const ENCHANT_IDLE_V: i32 = 166;
/// The hovered row's sprite row: `(0, 204)` (`:205-211`).
pub const ENCHANT_HOVER_V: i32 = 204;
/// The clasp's sheet x for offer `l`: `16·l` (`:193`, `:218`).
pub fn enchant_clasp_u(l: i32) -> i32 {
    16 * l
}
/// The affordable clasp's sprite row: `(16·l, 223)` (`:218`).
pub const ENCHANT_CLASP_V: i32 = 223;
/// The unaffordable clasp's sprite row: `(16·l, 239)` (`:193`).
pub const ENCHANT_CLASP_DIM_V: i32 = 239;
/// The clasp's panel rect: `(i1 + 1, j + 15 + 19·l)`, 16×16 (`:193`, `:218`).
pub fn enchant_clasp_pos(l: i32) -> (i32, i32) {
    (ENCHANT_ROW_X + 1, 15 + 19 * l)
}

/// The glyph run's panel x: `i1 + 20` (`:178-180`).
pub const ENCHANT_GLYPH_X: i32 = 80;
/// The glyph run's width: 86 (`:178-180`).
pub const ENCHANT_GLYPH_W: i32 = 86;

/// One glyph run's panel y: `j + 16 + 19·l` (`:193-224`).
pub fn enchant_glyph_y(l: i32) -> i32 {
    16 + 19 * l
}

/// The cost number's right edge: `j1 + 86` (`drawStringWithShadow` at `:224`).
pub const ENCHANT_COST_RIGHT: i32 = 166;

/// The cost number's panel x for its measured `width`: right-aligned at
/// `j1 + 86 − width` (`:224`).
pub fn enchant_cost_x(width: i32) -> i32 {
    ENCHANT_COST_RIGHT - width
}

/// The cost number's panel y: `j + 16 + 19·l + 7` (`:224`).
pub fn enchant_cost_y(l: i32) -> i32 {
    23 + 19 * l
}

/// The affordable glyph and cost colour: 8453920 (`:221`, `:205-221`).
pub const ENCHANT_AFFORD: u32 = 8_453_920;
/// The hovered glyph and cost colour: 16777088 (`:205-211`).
pub const ENCHANT_HOVER: u32 = 16_777_088;
/// The unaffordable glyph colour: `(6839882 & 16711422) >> 1` (`:200`).
pub const ENCHANT_DIM_GLYPH: u32 = (6_839_882 & 16_711_422) >> 1;
/// The unaffordable cost colour: 4226832 (`:201`, carried as `i2`).
pub const ENCHANT_DIM_COST: u32 = 4_226_832;

/// One offer row's face: the background and clasp sprites and the two text
/// colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfferFace {
    /// The 108×19 background's sheet row (185 empty, 166 idle, 204 hovered).
    pub bg_v: i32,
    /// The 16×16 clasp's sheet row (223 affordable, 239 unaffordable): `None`
    /// on the empty row, which draws no clasp (`:186-188`).
    pub clasp_v: Option<i32>,
    /// The glyph run's colour.
    pub glyph: u32,
    /// The cost number's colour.
    pub cost: u32,
}

/// One offer row's face for the zero-based `index` with `cost`, the lapis in
/// the slot and the player's `level` (`drawGuiContainerBackgroundLayer`:
/// 183-227). A zero cost draws the empty row; otherwise the row is
/// unaffordable while `lapis < index + 1 || level < cost` outside creative
/// (`:196`) — the port reads survival always (no gamemode feed reaches the
/// screens), recorded above.
pub fn enchant_face(index: usize, cost: i32, lapis: i32, level: i32, hovered: bool) -> OfferFace {
    if cost == 0 {
        return OfferFace {
            bg_v: ENCHANT_EMPTY_V,
            clasp_v: None,
            glyph: ENCHANT_AFFORD,
            cost: ENCHANT_AFFORD,
        };
    }
    let need = index as i32 + 1;
    if lapis < need || level < cost {
        OfferFace {
            bg_v: ENCHANT_EMPTY_V,
            clasp_v: Some(ENCHANT_CLASP_DIM_V),
            glyph: ENCHANT_DIM_GLYPH,
            cost: ENCHANT_DIM_COST,
        }
    } else if hovered {
        OfferFace {
            bg_v: ENCHANT_HOVER_V,
            clasp_v: Some(ENCHANT_CLASP_V),
            glyph: ENCHANT_HOVER,
            // The cost stays 8453920 on the affordable row even hovered: the
            // source reassigns `i2 = 8453920` after the hover branch (`:224`),
            // so only the glyph takes the yellow.
            cost: ENCHANT_AFFORD,
        }
    } else {
        OfferFace {
            bg_v: ENCHANT_IDLE_V,
            clasp_v: Some(ENCHANT_CLASP_V),
            glyph: ENCHANT_AFFORD,
            cost: ENCHANT_AFFORD,
        }
    }
}

/// The offer click's gate (`ContainerEnchantment.enchantItem`:243-253): the
/// lapis covers `index + 1`, the cost is positive, the item slot holds a
/// stack, and the level covers both `index + 1` and the cost — outside
/// creative, which the port reads as survival always (recorded above).
pub fn enchant_gate(index: usize, cost: i32, has_item: bool, lapis: i32, level: i32) -> bool {
    let need = index as i32 + 1;
    cost > 0 && has_item && lapis >= need && level >= need && level >= cost
}

/// The cost number's text: the bare cost (`:190` draws `"" + l1`).
pub fn enchant_cost_text(cost: i32) -> String {
    format!("{cost}")
}

/// The white-italic clue line for the offered enchantment's composed `name`
/// (`drawScreen`:244-252 wraps the translated name in WHITE + ITALIC; the
/// port carries no I18n sentence, so the `container.enchant.clue` wrapper is
/// the name alone — recorded above).
pub fn enchant_clue_line(name: &str) -> String {
    format!("§f§o{name}")
}

/// The offered enchantment's composed clue name, if the offer names a known
/// enchantment (`Enchantment.getEnchantmentById(l & 255)`, `:247`): the id's
/// low byte names it, the bits above 8 its level.
pub fn enchant_clue_name(id: i32) -> Option<String> {
    let level = ((id & 0xFF00) >> 8) as i16;
    enchant_line((id & 0xFF) as u16, level)
}

/// One offer's hover lines (`drawScreen`:244-299): the clue when the offer
/// names a known enchantment, then — outside creative, which the port reads
/// as survival always — the blank separator, the red level requirement while
/// the level falls short, else the lapis line (grey when the lapis covers
/// `index + 1`, red while short) and the grey level line. Both count lines
/// name `index + 1` (`lapis.one|many`, `level.one|many` over `i1 = j + 1`,
/// `:267-296`); the port's English follows the title convention — `1 Lapis
/// Lazuli` / `{n} Lapis Lazuli`, `1 Level` / `{n} Levels` — recorded above.
pub fn enchant_tooltip(
    clue: Option<&str>,
    id_known: bool,
    index: usize,
    cost: i32,
    lapis: i32,
    level: i32,
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(clue) = clue {
        lines.push(enchant_clue_line(clue));
    }
    let need = index as i32 + 1;
    // The blank separator follows the clue id, not the clue line: the source
    // adds it whenever `l >= 0` (`:255-258`), even for an id no registry
    // entry names (no clue line, but the separator and count lines still
    // draw).
    if id_known {
        lines.push(String::new());
    }
    if level < cost {
        lines.push(format!("§cLevel Requirement: {cost}"));
    } else {
        let lapis_line = if need == 1 {
            String::from("1 Lapis Lazuli")
        } else {
            format!("{need} Lapis Lazuli")
        };
        lines.push(format!(
            "{}{lapis_line}",
            if lapis >= need { "§7" } else { "§c" }
        ));
        let level_line = if need == 1 {
            String::from("1 Level")
        } else {
            format!("{need} Levels")
        };
        lines.push(format!("§7{level_line}"));
    }
    lines
}

/// The single glyph word list (`EnchantmentNameParts.java`: the `~70`-token
/// array split on the space — 55 words once the joints split).
pub const ENCHANT_WORDS: &str = "the elder scrolls klaatu berata niktu xyzzy bless curse light darkness fire air earth water hot dry cold wet ignite snuff embiggen twist shorten stretch fiddle destroy imbue galvanize enchant free limited range of towards inside sphere cube self other ball mental physical grow shrink demon elemental spirit animal creature beast humanoid undead fresh stale";

/// The glyph list as words, split on the space.
pub fn enchant_words() -> Vec<&'static str> {
    ENCHANT_WORDS
        .split(' ')
        .filter(|word| !word.is_empty())
        .collect()
}

/// `java.util.Random`'s 48-bit LCG (`Random.java`: the `0x5DEECE66D`
/// multiplier): the glyph names reseed it per frame from the table's xpSeed
/// (`GuiEnchantment.java`:172), so the port carries the generator rather
/// than a lookalike.
struct JavaRand(u64);

/// The LCG multiplier.
const JAVA_MULT: u64 = 0x5DEE_CE66D;
/// The 48-bit state mask.
const JAVA_MASK: u64 = (1 << 48) - 1;

impl JavaRand {
    /// Seeds the generator the way `Random.setSeed` does.
    fn seeded(seed: i64) -> Self {
        Self((seed as u64 ^ JAVA_MULT) & JAVA_MASK)
    }

    /// The next `bits` random bits.
    fn next(&mut self, bits: u32) -> i32 {
        self.0 = (self.0.wrapping_mul(JAVA_MULT).wrapping_add(0xB)) & JAVA_MASK;
        (self.0 >> (48 - bits)) as i32
    }

    /// `Random.nextInt(bound)`: the power-of-two fast path and the rejection
    /// loop that keeps the draw unbiased.
    fn next_int(&mut self, bound: i32) -> i32 {
        if bound <= 0 {
            return 0;
        }
        if (bound & bound.wrapping_neg()) == bound {
            return (((bound as i64) * (self.next(31) as i64)) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let value = bits % bound;
            if bits - value + (bound - 1) >= 0 {
                return value;
            }
        }
    }
}

/// The frame's glyph name: `generateNewRandomName` — 3-4 words from the single
/// list — over the generator reseeded from the table's `xpSeed`
/// (`EnchantmentNameParts.java`:20-32, `GuiEnchantment.java`:172).
///
/// The source generates one name per offer row inside the frame loop with a
/// single advancing RNG (the reseed runs once, `generateNewRandomName` runs
/// per row), so row `index` reads the RNG's (`index` + 1)-th name. Row 0
/// always reads the first word because every frame reseeds.
pub fn glyph_name(xp_seed: i32) -> String {
    glyph_word_at(xp_seed, 0)
}

/// Draws the frame's (`index` + 1)-th glyph name: row `index`'s galactic
/// text for the per-row loop.
pub fn glyph_word_at(xp_seed: i32, index: usize) -> String {
    let mut rng = JavaRand::seeded(i64::from(xp_seed));
    let words: Vec<&str> = ENCHANT_WORDS
        .split(' ')
        .filter(|word| !word.is_empty())
        .collect();
    let mut name = String::new();
    for _ in 0..=index {
        name.clear();
        let count = rng.next_int(2) + 3;
        for slot in 0..count {
            if slot > 0 {
                name.push(' ');
            }
            let pick = rng.next_int(words.len() as i32);
            name.push_str(words[pick as usize]);
        }
    }
    name
}

/// The book's cover rect on the panel: the port's 2D stand-in for the 3D
/// model draw (`drawGuiContainerBackgroundLayer`:113-169 projects the model
/// over the table) — the cover centred over the offer rows with the two page
/// rects opening from its spine by the ported open float.
pub const BOOK_RECT: (i32, i32, i32, i32) = (62, 2, 52, 10);
/// The book cover's flat colour and the pages' flat colour: the stand-in has
/// no sheet art, so the cover and the two page rects draw flat — dark leather
/// brown for the cover, pale vellum for the pages (recorded reduction).
pub const BOOK_COLOUR: u32 = 0x6B_4A35;
/// The pages' flat colour, paired with [`BOOK_COLOUR`].
pub const BOOK_PAGE_COLOUR: u32 = 0xD8_CFA8;
/// The page pair's full half-width at fully open.
pub const BOOK_PAGE_W: i32 = 24;

/// The book's ticked animation fields (`GuiEnchantment.java`:46-53): the tick
/// counter, the current and previous rotation, the target rotation, the
/// velocity and the current and previous open amounts.
#[derive(Debug, Clone, PartialEq)]
pub struct BookAnim {
    /// Ticks since open (`field_147073_u`).
    pub tick: u64,
    /// The current rotation (`field_147071_v`).
    pub rot: f32,
    /// The previous rotation (`field_147069_w`).
    pub prev_rot: f32,
    /// The target rotation (`field_147082_x`).
    pub target: f32,
    /// The smoothed velocity (`field_147081_y`).
    pub vel: f32,
    /// The current open amount (`field_147080_z`).
    pub open: f32,
    /// The previous open amount (`field_147076_A`).
    pub prev_open: f32,
    /// The last item the table held, for the change retarget
    /// (`field_147077_B` compares with `areItemStacksEqual`).
    pub last_item: Option<MetadataItem>,
    /// The retarget draw (`GuiEnchantment.random`, unseeded wall-clock in the
    /// source): the port seeds it at zero so the motion is deterministic —
    /// recorded above.
    pub rand_state: u64,
}

impl Default for BookAnim {
    fn default() -> Self {
        Self {
            tick: 0,
            rot: 0.0,
            prev_rot: 0.0,
            target: 0.0,
            vel: 0.0,
            open: 0.0,
            prev_open: 0.0,
            last_item: None,
            rand_state: 0,
        }
    }
}

impl BookAnim {
    /// One screen tick (`func_147068_g`:306-353): an item change retargets the
    /// rotation until it stands more than 1 away (`:308-323`); the open amount
    /// steps ±0.2 toward (any level positive) clamped to 0-1 (`:330-347`);
    /// the velocity chases the clamped gap chase times 0.9 and the rotation
    /// advances by it (`:348-352`).
    pub fn step(&mut self, levels: &[i16; 3], slot0: Option<&MetadataItem>) {
        if slot0 != self.last_item.as_ref() {
            self.last_item = slot0.cloned();
            let mut rng = JavaRand(self.rand_state);
            loop {
                self.target += (rng.next_int(4) - rng.next_int(4)) as f32;
                if self.rot > self.target + 1.0 || self.rot < self.target - 1.0 {
                    break;
                }
            }
            self.rand_state = rng.0;
        }
        self.tick += 1;
        self.prev_rot = self.rot;
        self.prev_open = self.open;
        if levels.iter().any(|level| *level != 0) {
            self.open += 0.2;
        } else {
            self.open -= 0.2;
        }
        self.open = self.open.clamp(0.0, 1.0);
        let chase = ((self.target - self.rot) * 0.4).clamp(-0.2, 0.2);
        self.vel += (chase - self.vel) * 0.9;
        self.rot += self.vel;
    }

    /// The frame's draw floats at `partial` ticks (`:138-160`): the open
    /// lerp `f2` and the two page flips `f3`/`f4` — the fractional rotation
    /// offset by 0.25/0.75, times 1.6 minus 0.3, clamped to 0-1.
    pub fn frame(&self, partial: f32) -> (f32, f32, f32) {
        let open = self.prev_open + (self.open - self.prev_open) * partial;
        let rot = self.prev_rot + (self.rot - self.prev_rot) * partial;
        (open, page_flip(rot + 0.25), page_flip(rot + 0.75))
    }
}

/// One page flip: the fractional part times 1.6 minus 0.3, clamped
/// (`:141-157`).
fn page_flip(value: f32) -> f32 {
    ((value - value.trunc()) * 1.6 - 0.3).clamp(0.0, 1.0)
}

/// The villager pager buttons (`GuiMerchant.initGui`:60-66): the next button
/// (id 1, forward) at `(120 + 27, 24 − 1)`, the previous (id 2) at
/// `(36 − 19, 24 − 1)`, each 12×19.
pub const VILLAGER_NEXT_POS: (i32, i32) = (147, 23);
/// The previous pager's panel origin, paired with [`VILLAGER_NEXT_POS`].
pub const VILLAGER_PREV_POS: (i32, i32) = (17, 23);
/// The pager buttons' size: 12×19 (`MerchantButton`:240's 12×19).
pub const VILLAGER_BUTTON_W: i32 = 12;
/// The pager buttons' height, paired with [`VILLAGER_BUTTON_W`].
pub const VILLAGER_BUTTON_H: i32 = 19;

/// The displayed recipe's icon origins (`GuiMerchant.drawScreen`:193-203):
/// the buy at (36, 24), the second buy at (62, 24) when present, the sell at
/// (120, 24).
pub const VILLAGER_BUY_POS: (i32, i32) = (36, 24);
/// The second buy's icon origin, paired with [`VILLAGER_BUY_POS`].
pub const VILLAGER_SECOND_POS: (i32, i32) = (62, 24);
/// The sell's icon origin, paired with [`VILLAGER_BUY_POS`].
pub const VILLAGER_SELL_POS: (i32, i32) = (120, 24);

/// The out-of-stock red X's sheet rect: `(212, 0, 28×21)` drawn at (83, 21)
/// and (83, 51) while the recipe is disabled (`drawGuiContainerBackgroundLayer`:
/// 158-165).
pub const VILLAGER_RED_X_UV: (i32, i32, i32, i32) = (212, 0, 28, 21);
/// The red X's first panel origin, paired with [`VILLAGER_RED_X_UV`].
pub const VILLAGER_RED_X_A: (i32, i32) = (83, 21);
/// The red X's second panel origin, paired with [`VILLAGER_RED_X_A`].
pub const VILLAGER_RED_X_B: (i32, i32) = (83, 51);

/// One pager button's sheet origin (`MerchantButton.drawButton`:247-266):
/// `sx = 176 + (disabled ? 24 : hovered ? 12 : 0)`, `sy = 0` forward else the
/// button height 19.
pub fn merchant_button_uv(enabled: bool, hovered: bool, forward: bool) -> (i32, i32) {
    let sx = 176
        + if !enabled {
            VILLAGER_BUTTON_W * 2
        } else if hovered {
            VILLAGER_BUTTON_W
        } else {
            0
        };
    let sy = if forward { 0 } else { VILLAGER_BUTTON_H };
    (sx, sy)
}

/// Whether the pager button is enabled: next while the selection stands
/// before the last offer, previous while past the first
/// (`updateScreen`:86-93).
pub fn pager_enabled(selected: usize, count: usize, forward: bool) -> bool {
    if count == 0 {
        return false;
    }
    if forward {
        selected < count - 1
    } else {
        selected > 0
    }
}

/// Frames the villager page select's body: the selected index as one
/// big-endian i32 (`PacketBuffer.writeInt`, `actionPerformed`:129-130).
pub fn trsel_data(index: i32) -> Vec<u8> {
    index.to_be_bytes().to_vec()
}

/// The villager page select's send on [`TRSEL_CHANNEL`].
pub fn trsel_event(index: i32) -> InputEvent {
    InputEvent::CustomPayload {
        channel: String::from(TRSEL_CHANNEL),
        data: trsel_data(index),
    }
}

/// The villager's pager state: the selected recipe and the trade list the
/// `MC|TrList` payload carried (`ClientEvent::MerchantOffers`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VillagerSelection {
    /// The selected recipe's index into `offers`.
    pub selected: usize,
    /// The trade list, in wire order.
    pub offers: Vec<MerchantOffer>,
}

impl VillagerSelection {
    /// Folds a fresh trade list: the selection clamps into it (the source
    /// returns early past the end, `drawGuiContainerBackgroundLayer`:152-156,
    /// so a clamped index is the port's standing rule).
    pub fn set_offers(&mut self, offers: Vec<MerchantOffer>) {
        self.offers = offers;
        if !self.offers.is_empty() {
            self.selected = self.selected.min(self.offers.len() - 1);
        }
    }

    /// Steps the pager (`actionPerformed`:102-124): next clamps at the last
    /// offer, previous at the first. Answers whether the page moved — only a
    /// move re-fills the merchant inventory and sends `MC|TrSel`.
    pub fn step(&mut self, forward: bool) -> bool {
        if self.offers.is_empty() {
            return false;
        }
        let next = if forward {
            (self.selected + 1).min(self.offers.len() - 1)
        } else {
            self.selected.saturating_sub(1)
        };
        if next == self.selected {
            return false;
        }
        self.selected = next;
        true
    }

    /// The displayed offer, if the selection names one.
    pub fn current(&self) -> Option<&MerchantOffer> {
        self.offers.get(self.selected)
    }
}

/// Whether the panel point hits the pager button at `pos`: the 12×19 rect.
pub fn pager_hit(pos: (i32, i32), x: i32, y: i32) -> bool {
    x >= pos.0 && x < pos.0 + VILLAGER_BUTTON_W && y >= pos.1 && y < pos.1 + VILLAGER_BUTTON_H
}

/// The chested panel's dest rect: `(79, 17, 90×54)`
/// (`GuiScreenHorseInventory.java`:58-61).
pub const HORSE_CHEST_RECT: (i32, i32, i32, i32) = (79, 17, 90, 54);
/// The chested panel's sheet origin: `(0, ySize)` (`:60`).
pub const HORSE_CHEST_UV: (i32, i32) = (0, 166);
/// The armour slot frame's dest rect: `(7, 35, 18×18)` (`:63-66`).
pub const HORSE_ARMOUR_RECT: (i32, i32, i32, i32) = (7, 35, 18, 18);
/// The armour frame's sheet origin: `(0, ySize + 54)` (`:65`).
pub const HORSE_ARMOUR_UV: (i32, i32) = (0, 220);
/// The horse preview's anchor and scale: `drawEntityOnScreen(i + 51, j + 60,
/// 17, ...)` (`:68`) — Task 20 owns the entity draw; the port exposes the
/// anchor so its case pins the layout around it.
pub const HORSE_PREVIEW: (i32, i32, i32) = (51, 60, 17);

/// The horse screen's live state: the window's entity id resolves the horse
/// for the preview and the chested/armour flags (`handleOpenWindow`:1107-1115
/// looks the horse up by `packetIn.getEntityId`); the title stays the packet
/// string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HorseState {
    /// The horse's entity id; `None` for no horse window.
    pub entity_id: Option<i32>,
    /// Whether the armour frame draws: `canWearArmor` in the source, which
    /// the client cannot resolve without the live horse — the port stands it
    /// up true (the common horse) behind this setter, recorded above.
    pub armoured: bool,
}

/// The anvil name field's panel rect: `GuiTextField(0, font, i + 62, j + 24,
/// 103×12)` (`GuiRepair.java`:47).
pub const NAME_FIELD_RECT: (i32, i32, i32, i32) = (62, 24, 103, 12);
/// The name cursor's bar: 1px wide, `FONT_HEIGHT` tall, a pixel below the
/// field's top and 4 (the inset) + 1 past the text before it
/// (`GuiTextField.drawTextBox` draws the bar at `x + 1`, `y + 1`,
/// `FONT_HEIGHT` tall, in −3092272 = 0xFFD0D0D0).
pub const NAME_CURSOR_W: i32 = 1;
/// The cursor bar's height, paired with [`NAME_CURSOR_W`].
pub const NAME_CURSOR_H: i32 = 9;
/// The cursor bar's inset past the text, paired with [`NAME_CURSOR_W`].
pub const NAME_CURSOR_DX: i32 = 5;
/// The cursor bar's drop below the field's top, paired with [`NAME_CURSOR_W`].
pub const NAME_CURSOR_DY: i32 = 1;
/// The cursor bar's grey, paired with [`NAME_CURSOR_W`].
pub const NAME_CURSOR_COLOUR: [f32; 4] = [208.0 / 255.0, 208.0 / 255.0, 208.0 / 255.0, 1.0];
/// The name field's max length: 30 (`:51`).
pub const NAME_FIELD_MAX: usize = 30;

/// Whether a character may be typed: `ChatAllowedCharacters:10-13` refuses
/// the format code, everything below the space and DEL, and
/// `GuiTextField.writeText`:132 filters every append through it — the same
/// rule the M4 chat field carries.
fn name_allowed(c: char) -> bool {
    c != '§' && c >= ' ' && c != '\u{7f}'
}

/// What one field key did: the character path's next step, the field's own,
/// or a key the field never touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKey {
    /// The key is the character path's: the caller runs [`NameField::type_text`].
    Character,
    /// The key was the field's own and is done; the keystroke still re-fires
    /// the rename send below (the source's `textboxKeyTyped` answers true for
    /// handled keys — arrows included — and every true answer reaches
    /// `renameItem`).
    Consumed,
    /// The key is not the field's: Tab, Enter and the recall-less arrows fall
    /// through to the container (`GuiRepair.keyTyped`:117-134 reaches
    /// `super.keyTyped` when `textboxKeyTyped` answers false).
    Ignored,
}

/// The anvil's name field: the M4 chat field engine's subset — text, cursor,
/// Backspace and the arrows plus the character path (`GuiTextField`'s full
/// word ops and selection never run: `renameItem` only calls get/setText,
/// `keyTyped` only `textboxKeyTyped`, plus `mouseClicked` focus and the draw).
/// No recall and no history: the rename is a single line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NameField {
    /// The text, edited in place.
    text: String,
    /// The cursor's byte index into `text`, always on a character boundary.
    cursor: usize,
    /// Whether the field holds focus (`setFocused` from `mouseClicked`).
    focused: bool,
    /// Whether the input slot held a stack at the last sync: the slot-0 sync
    /// below only re-fires on a presence change.
    bound: bool,
    /// The blink counter: steps once per session tick while open, and the
    /// cursor draws while `blink / 6 % 2 == 0` (`GuiTextField.java`:540).
    blink: u64,
}

impl NameField {
    /// The field's text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The cursor's byte index into the text.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Whether the field holds focus.
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Replaces the text with the cursor at its end (`setText`:86-101 — which,
    /// unlike the character path, applies no length cap).
    pub fn set_text(&mut self, text: &str) {
        self.text.clear();
        self.text.push_str(text);
        self.cursor = self.text.len();
    }

    /// One session tick of the blink counter.
    pub fn tick(&mut self) {
        self.blink = self.blink.wrapping_add(1);
    }

    /// Whether the cursor is in its lit phase (`GuiTextField.java`:540).
    pub fn cursor_visible(&self) -> bool {
        (self.blink / 6) % 2 == 0
    }

    /// One click: focus lands exactly on the field's rect
    /// (`mouseClicked` → `GuiTextField.mouseClicked` → `setFocused`).
    pub fn click(&mut self, x: i32, y: i32) {
        let (rx, ry, w, h) = NAME_FIELD_RECT;
        self.focused = x >= rx && x < rx + w && y >= ry && y < ry + h;
    }

    /// Appends text at the cursor — the character path `writeText` (`:129-169`)
    /// runs for keys the field does not own: refused characters drop, and the
    /// append stops at the field's 30-character cap. Answers whether any
    /// character landed: a fully refused append sends nothing (the source's
    /// `textboxKeyTyped` answers false past the filter). A cap-full append is
    /// the recorded divergence — the source answers true and re-fires, the
    /// port answers false and stays silent.
    pub fn type_text(&mut self, text: &str) -> bool {
        let kept: Vec<char> = text.chars().filter(|&c| name_allowed(c)).collect();
        let room = NAME_FIELD_MAX.saturating_sub(self.text.chars().count());
        let insert: String = kept.into_iter().take(room).collect();
        if insert.is_empty() {
            return false;
        }
        self.text.insert_str(self.cursor, &insert);
        self.cursor += insert.len();
        true
    }

    /// Backspace: removes the character before the cursor, if any
    /// (`textboxKeyTyped`:378-391's delete branch).
    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = self.text[..self.cursor]
            .chars()
            .next_back()
            .expect("a non-empty prefix has a last character");
        self.text.drain(self.cursor - prev.len_utf8()..self.cursor);
        self.cursor -= prev.len_utf8();
    }

    /// The left arrow: one character back (`:405-426`).
    fn left(&mut self) {
        if let Some(prev) = self.text[..self.cursor].chars().next_back() {
            self.cursor -= prev.len_utf8();
        }
    }

    /// The right arrow: one character on (`:428-449`).
    fn right(&mut self) {
        if let Some(next) = self.text[self.cursor..].chars().next() {
            self.cursor += next.len_utf8();
        }
    }

    /// One editing key, as `GuiRepair.keyTyped`:117-134 reads it against
    /// `textboxKeyTyped`:337-494: Backspace and the sideways arrows edit and
    /// consume; every other key is the character path's or the container's.
    pub fn key(&mut self, key: Key) -> FieldKey {
        match key {
            Key::Backspace => {
                self.backspace();
                FieldKey::Consumed
            }
            Key::ArrowLeft => {
                self.left();
                FieldKey::Consumed
            }
            Key::ArrowRight => {
                self.right();
                FieldKey::Consumed
            }
            _ => FieldKey::Character,
        }
    }
}

/// Whether the name field is enabled: the slot-0 sync enables it exactly
/// while the input slot holds a stack (`sendSlotContents`:200-212).
pub fn name_enabled(slot0_present: bool) -> bool {
    slot0_present
}

/// Frames the anvil rename's body: the raw string, varint-length-prefixed
/// UTF-8 — `PacketBuffer.writeString`'s rule, which is the landed
/// `write_plugin_message`'s channel-string rule.
pub fn item_name_data(name: &str) -> Vec<u8> {
    let mut data = Vec::with_capacity(name.len() + 5);
    write_varint(&mut data, name.len() as i32).expect("a Vec never refuses bytes");
    data.extend_from_slice(name.as_bytes());
    data
}

/// The anvil rename's send on [`ITEM_NAME_CHANNEL`].
pub fn item_name_event(name: &str) -> InputEvent {
    InputEvent::CustomPayload {
        channel: String::from(ITEM_NAME_CHANNEL),
        data: item_name_data(name),
    }
}

/// Folds the slot-0 sync into the field (`sendSlotContents`:200-212): a
/// presence change resets the text to the stack's display name (blank when
/// the slot cleared) and the enable gate with it; a newly filled slot
/// re-fires `renameItem` — the text as the `MC|ItemName` send. An unchanged
/// presence answers `None`: ordinary snapshots never resend.
pub fn anvil_sync(field: &mut NameField, slot0: Option<&MetadataItem>) -> Option<InputEvent> {
    let present = slot0.is_some();
    if present == field.bound {
        return None;
    }
    field.bound = present;
    field.set_text(slot0.map(display_name).as_deref().unwrap_or(""));
    if present {
        Some(item_name_event(field.text()))
    } else {
        None
    }
}

/// The anvil cost's default colour: 8453920 (`GuiRepair.java`:77).
pub const ANVIL_COST_COLOUR: u32 = 8_453_920;
/// The anvil cost's red: 16736352 — past the 40-level cap outside creative
/// and past an untakeable output (`:81-93`).
pub const ANVIL_COST_RED: u32 = 16_736_352;
/// The cap past which the cost reads expensive: 40 (`:81`).
pub const ANVIL_EXPENSIVE_AT: i32 = 40;

/// The cost line's panel y: 67 (`:97-113` draws at `l = 67`).
pub const ANVIL_COST_Y: i32 = 67;

/// The cost line's panel x for its measured `width`: `xSize − 8 − width`
/// (`:96`).
pub fn anvil_cost_x(width: i32) -> i32 {
    ANVIL.x_size - 8 - width
}

/// The anvil's cost line: the number the property names, its colour and
/// whether it reads expensive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnvilCost {
    /// The `maximumCost` property value.
    pub value: i32,
    /// Whether the line reads the expensive text (red past the cap).
    pub expensive: bool,
    /// The line's colour.
    pub colour: u32,
}

/// The anvil's cost line (`drawGuiContainerForegroundLayer`:75-99): hidden
/// while `maximumCost` is 0 (`:75`) and while inputs stand without an output
/// (`:86-89`); the expensive red past 40 outside creative (`:81-85`, the port
/// reads survival always); the untakeable red past an output the level cannot
/// take (`:90-93`); else the default (`:77`). The port draws the number alone
/// — no I18n sentences travel — recorded above.
pub fn anvil_cost(
    maximum_cost: i32,
    creative: bool,
    has_output: bool,
    can_take: bool,
) -> Option<AnvilCost> {
    if maximum_cost <= 0 {
        return None;
    }
    if maximum_cost >= ANVIL_EXPENSIVE_AT && !creative {
        return Some(AnvilCost {
            value: maximum_cost,
            expensive: true,
            colour: ANVIL_COST_RED,
        });
    }
    if !has_output {
        return None;
    }
    if !can_take {
        return Some(AnvilCost {
            value: maximum_cost,
            expensive: false,
            colour: ANVIL_COST_RED,
        });
    }
    Some(AnvilCost {
        value: maximum_cost,
        expensive: false,
        colour: ANVIL_COST_COLOUR,
    })
}

/// The name field's backdrop strip: `(59, 20, 110×16)`, sheet row `ySize`
/// with a stack in slot 0 else `ySize + 16` (`drawGuiContainerBackgroundLayer`:
/// 180).
pub const ANVIL_STRIP_RECT: (i32, i32, i32, i32) = (59, 20, 110, 16);
/// The strip's sheet row with (`ySize = 166`) and without (`ySize + 16) a
/// slot-0 stack, paired with [`ANVIL_STRIP_RECT`].
pub const ANVIL_STRIP_V_FULL: i32 = 166;
/// The strip's sheet row without a stack, paired with [`ANVIL_STRIP_V_FULL`].
pub const ANVIL_STRIP_V_EMPTY: i32 = 182;
/// The sheet row the strip samples from: the panel's own `ySize`.
pub fn anvil_strip_v(slot0_present: bool) -> i32 {
    if slot0_present {
        ANVIL_STRIP_V_FULL
    } else {
        ANVIL_STRIP_V_EMPTY
    }
}

/// The broken-arrow strip for inputs without an output: `(99, 45, 28×21)`
/// from sheet `(xSize, 0)` (`:182-185`).
pub const ANVIL_ARROW_RECT: (i32, i32, i32, i32) = (99, 45, 28, 21);
/// The broken arrow's sheet origin, paired with [`ANVIL_ARROW_RECT`].
pub const ANVIL_ARROW_UV: (i32, i32) = (176, 0);

/// Whether the broken-arrow strip draws: either input holds a stack while the
/// output stands empty (`:182`).
pub fn anvil_arrow_broken(has_input0: bool, has_input1: bool, has_output: bool) -> bool {
    (has_input0 || has_input1) && !has_output
}

/// One family-B screen's interactive state, standing on the window's kind.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum FamilyState {
    /// No family-B screen (or no screen at all).
    #[default]
    None,
    /// The beacon's local selection, seeded from properties 1/2.
    Beacon(BeaconSelection),
    /// The enchanting book's animation.
    Enchanting(BookAnim),
    /// The villager's pager over the trade list.
    Villager(VillagerSelection),
    /// The horse's live state.
    Horse(HorseState),
    /// The anvil's name field.
    Anvil(NameField),
}

impl FamilyState {
    /// Stands the state up for an opened window of `kind`.
    pub fn for_kind(kind: WindowKind, entity_id: Option<i32>) -> Self {
        match kind {
            WindowKind::Beacon => FamilyState::Beacon(BeaconSelection::default()),
            WindowKind::EnchantingTable => FamilyState::Enchanting(BookAnim::default()),
            WindowKind::Villager => FamilyState::Villager(VillagerSelection::default()),
            WindowKind::EntityHorse => FamilyState::Horse(HorseState {
                entity_id,
                armoured: true,
            }),
            WindowKind::Anvil => FamilyState::Anvil(NameField::default()),
            _ => FamilyState::None,
        }
    }
}

#[cfg(test)]
mod tables {
    //! The family-B pins: every layout's size, sheet, slot count and title
    //! rule, three coordinates per table, and the horse's chest pick.

    use super::*;

    #[test]
    fn the_beacon_lays_out_the_payment_and_the_shifted_player_block() {
        assert_eq!((BEACON.x_size, BEACON.y_size), (230, 219));
        assert_eq!(BEACON.sheet, "gui/container/beacon");
        assert_eq!(BEACON.slots.len(), 37, "the payment plus the 36 player");
        assert_eq!(
            BEACON.slots[0],
            SlotPos {
                index: 0,
                x: 136,
                y: 110,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            BEACON.slots[1],
            SlotPos {
                index: 1,
                x: 36,
                y: 137,
                block: SlotBlock::Player,
            }
        );
        assert_eq!(
            BEACON.slots[9],
            SlotPos {
                index: 9,
                x: 36 + 8 * 18,
                y: 137,
                block: SlotBlock::Player,
            }
        );
        assert_eq!(
            BEACON.slots[28],
            SlotPos {
                index: 28,
                x: 36,
                y: 195,
                block: SlotBlock::Player,
            }
        );
        assert_eq!(layout_for_kind(WindowKind::Beacon, 37), &BEACON);
    }

    #[test]
    fn the_enchanting_table_lays_out_the_item_and_the_lapis() {
        assert_eq!((ENCHANTING.x_size, ENCHANTING.y_size), (176, 166));
        assert_eq!(ENCHANTING.sheet, "gui/container/enchanting_table");
        assert_eq!(
            ENCHANTING.slots.len(),
            38,
            "two table slots plus the 36 player"
        );
        assert_eq!(
            ENCHANTING.slots[0],
            SlotPos {
                index: 0,
                x: 15,
                y: 47,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            ENCHANTING.slots[1],
            SlotPos {
                index: 1,
                x: 35,
                y: 47,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            ENCHANTING.slots[2],
            SlotPos {
                index: 2,
                x: 8,
                y: 84,
                block: SlotBlock::Player,
            }
        );
        assert_eq!(
            layout_for_kind(WindowKind::EnchantingTable, 38),
            &ENCHANTING
        );
    }

    #[test]
    fn the_villager_lays_out_the_two_buys_and_the_result() {
        assert_eq!((VILLAGER.x_size, VILLAGER.y_size), (176, 166));
        assert_eq!(VILLAGER.sheet, "gui/container/villager");
        assert_eq!(
            VILLAGER.slots.len(),
            39,
            "three merchant slots plus the 36 player"
        );
        assert_eq!(
            VILLAGER.slots[0],
            SlotPos {
                index: 0,
                x: 36,
                y: 53,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            VILLAGER.slots[1],
            SlotPos {
                index: 1,
                x: 62,
                y: 53,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            VILLAGER.slots[2],
            SlotPos {
                index: 2,
                x: 120,
                y: 53,
                block: SlotBlock::Container,
            }
        );
    }

    #[test]
    fn the_horse_picks_its_table_by_slot_count() {
        assert_eq!((HORSE.x_size, HORSE.y_size), (176, 166));
        assert_eq!(HORSE.sheet, "gui/container/horse");
        assert_eq!(HORSE.slots.len(), 38, "saddle, armour and the 36 player");
        assert_eq!(
            HORSE.slots[0],
            SlotPos {
                index: 0,
                x: 8,
                y: 18,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            HORSE.slots[1],
            SlotPos {
                index: 1,
                x: 8,
                y: 36,
                block: SlotBlock::Container,
            }
        );
        assert!(!horse_chested(38));
        assert!(horse_chested(53));
        assert_eq!(layout_for_kind(WindowKind::EntityHorse, 38), &HORSE);
        assert_eq!(layout_for_kind(WindowKind::EntityHorse, 53), &HORSE_CHESTED);
        assert_eq!(HORSE_CHESTED.slots.len(), 53, "2 + 15 chest + 36 player");
        assert_eq!(
            HORSE_CHESTED.slots[2],
            SlotPos {
                index: 2,
                x: 80,
                y: 18,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            HORSE_CHESTED.slots[16],
            SlotPos {
                index: 16,
                x: 80 + 4 * 18,
                y: 18 + 2 * 18,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            HORSE_CHESTED.slots[17],
            SlotPos {
                index: 17,
                x: 8,
                y: 84,
                block: SlotBlock::Player,
            }
        );
    }

    #[test]
    fn the_anvil_lays_out_the_inputs_and_the_output() {
        assert_eq!((ANVIL.x_size, ANVIL.y_size), (176, 166));
        assert_eq!(ANVIL.sheet, "gui/container/anvil");
        assert_eq!(
            ANVIL.slots.len(),
            39,
            "three anvil slots plus the 36 player"
        );
        assert_eq!(
            ANVIL.slots[0],
            SlotPos {
                index: 0,
                x: 27,
                y: 47,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            ANVIL.slots[1],
            SlotPos {
                index: 1,
                x: 76,
                y: 47,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            ANVIL.slots[2],
            SlotPos {
                index: 2,
                x: 134,
                y: 47,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(layout_for_kind(WindowKind::Anvil, 39), &ANVIL);
    }
}

#[cfg(test)]
mod interactions {
    //! The family-B interaction pins: the beacon's rows and confirm, the
    //! enchanting faces and gate, the villager pager and `MC|TrSel`, the
    //! offer lock, the anvil cost and the name field.

    use super::*;

    fn offer(uses: i32, max_uses: i32) -> MerchantOffer {
        MerchantOffer {
            first: None,
            second: None,
            output: None,
            uses,
            max_uses,
        }
    }

    fn stack(id: i16, count: u8) -> MetadataItem {
        MetadataItem {
            id,
            count,
            damage: 0,
            nbt: None,
        }
    }

    #[test]
    // The tier-0 encoding written out: `(tier << 8) | effect` with tier 0
    // trips `identity_op`, kept for the shape.
    #[allow(clippy::identity_op)]
    fn the_beacon_rows_come_from_the_effects_table() {
        // Levels 4 unlock every row; the chosen ids preselect.
        let rows = beacon_rows(4, 3, 10);
        // Three primary rows: 2 + 2 + 1, then regeneration, then the echo.
        assert_eq!(rows.len(), 7);
        assert_eq!(
            rows[0],
            BeaconRow {
                id: (0 << 8) | 1,
                effect: 1,
                tier: 0,
                x: 76 - (2 * 22 + 2) / 2,
                y: 22,
                enabled: true,
                selected: false,
            }
        );
        assert_eq!(rows[1].effect, 3);
        assert!(rows[1].selected, "digSpeed is the chosen primary");
        assert_eq!(rows[1].x, 76 + 24 - (2 * 22 + 2) / 2);
        assert_eq!(rows[2].effect, 11);
        assert_eq!(rows[3].effect, 8);
        assert_eq!(rows[3].y, 22 + 25);
        assert_eq!(rows[4].effect, 5);
        assert_eq!(rows[4].y, 22 + 50);
        // The secondary row at y 47 centred on 167, and the echo of the
        // primary past it.
        assert_eq!(rows[5].effect, 10);
        assert_eq!(rows[5].tier, 3);
        assert!(rows[5].selected, "regeneration is the chosen secondary");
        assert_eq!(rows[6].effect, 3);
        assert_eq!(rows[6].x, 167 + 24 - (2 * 22 + 2) / 2);
        assert_eq!(rows[6].y, 47);
    }

    #[test]
    fn the_beacon_rows_lock_below_their_tier() {
        // Levels 1 unlocks only tier 0; regeneration's row wants 4.
        let rows = beacon_rows(1, 0, 0);
        assert!(rows[0].enabled);
        assert!(!rows[2].enabled, "tier 1 locks at levels 1");
        assert!(!rows[5].enabled, "the secondary locks below levels 4");
        assert_eq!(rows.len(), 6, "five primary plus the secondary, no echo");
    }

    #[test]
    fn the_beacon_click_selects_locally_and_confirms_over_the_send() {
        let rows = beacon_rows(4, 0, 0);
        let mut selection = BeaconSelection::default();
        let hit = beacon_hit(&rows, rows[3].x + 1, rows[3].y + 1);
        assert_eq!(hit, Some((1 << 8) | 8));
        beacon_select(&mut selection, &rows, hit.expect("the jump button hits"));
        assert_eq!(selection.primary, 8);
        assert_eq!(selection.secondary, 0);
        // Re-clicking the chosen button is a no-op.
        let again = beacon_rows(4, 8, 0);
        beacon_select(&mut selection, &again, (1 << 8) | 8);
        assert_eq!(selection.primary, 8);
        // The confirm needs the payment and a primary.
        assert!(!beacon_confirm_enabled(false, 8));
        assert!(!beacon_confirm_enabled(true, 0));
        assert!(beacon_confirm_enabled(true, 8));
        assert_eq!(
            beacon_event(8, 0),
            InputEvent::CustomPayload {
                channel: String::from("MC|Beacon"),
                data: vec![0, 0, 0, 8, 0, 0, 0, 0],
            }
        );
        // An unknown id selects nothing.
        beacon_select(&mut selection, &rows, 0xFFFF);
        assert_eq!(selection.primary, 8);
    }

    #[test]
    fn the_beacon_button_strip_reads_enabled_selected_hovered() {
        assert_eq!(beacon_button_u(true, false, false), 0);
        assert_eq!(beacon_button_u(true, true, false), 22);
        assert_eq!(beacon_button_u(false, true, true), 44);
        assert_eq!(beacon_button_u(true, false, true), 66);
    }

    #[test]
    fn the_enchanting_faces_read_cost_and_level() {
        // The empty row: no clasp, default colours.
        assert_eq!(
            enchant_face(0, 0, 0, 0, false),
            OfferFace {
                bg_v: 185,
                clasp_v: None,
                glyph: 8_453_920,
                cost: 8_453_920,
            }
        );
        // Affordable and idle.
        assert_eq!(
            enchant_face(1, 12, 3, 30, false),
            OfferFace {
                bg_v: 166,
                clasp_v: Some(223),
                glyph: 8_453_920,
                cost: 8_453_920,
            }
        );
        // Hovered: the glyph takes the yellow, the cost stays 8453920 (`:224`).
        assert_eq!(enchant_face(1, 12, 3, 30, true).bg_v, 204);
        assert_eq!(enchant_face(1, 12, 3, 30, true).glyph, 16_777_088);
        assert_eq!(enchant_face(1, 12, 3, 30, true).cost, 8_453_920);
        // Short on lapis: the dim face.
        assert_eq!(
            enchant_face(1, 12, 1, 30, false),
            OfferFace {
                bg_v: 185,
                clasp_v: Some(239),
                glyph: ENCHANT_DIM_GLYPH,
                cost: 4_226_832,
            }
        );
        // Short on levels: dim too.
        assert_eq!(enchant_face(0, 6, 1, 5, false).clasp_v, Some(239));
        assert_eq!(ENCHANT_DIM_GLYPH, 3_419_941);
        // The geometry the draws read.
        assert_eq!(enchant_row_y(2), 14 + 38);
        assert_eq!(enchant_clasp_u(2), 32);
        assert_eq!(enchant_clasp_pos(1), (61, 34));
        assert_eq!(enchant_glyph_y(1), 35);
        assert_eq!(enchant_cost_x(10), 156);
        assert_eq!(enchant_cost_y(0), 23);
    }

    #[test]
    fn the_enchanting_click_needs_the_item_the_lapis_and_the_levels() {
        assert!(enchant_gate(0, 6, true, 1, 6));
        assert!(!enchant_gate(0, 0, true, 1, 6), "no cost, no send");
        assert!(!enchant_gate(0, 6, false, 1, 6), "no item, no send");
        assert!(
            !enchant_gate(1, 12, true, 1, 30),
            "the lapis must cover index + 1"
        );
        assert!(
            !enchant_gate(0, 6, true, 1, 5),
            "the level must cover the cost"
        );
        assert_eq!(enchant_cost_text(30), "30");
    }

    #[test]
    fn the_enchanting_tooltip_names_the_rows() {
        let lines = enchant_tooltip(Some("Protection IV"), true, 2, 30, 3, 30);
        assert_eq!(lines[0], "§f§oProtection IV");
        assert_eq!(lines[1], "");
        assert_eq!(lines[2], "§73 Lapis Lazuli");
        assert_eq!(lines[3], "§73 Levels");
        let short_lapis = enchant_tooltip(Some("Protection IV"), true, 2, 30, 1, 30);
        assert_eq!(short_lapis[2], "§c3 Lapis Lazuli");
        let short_level = enchant_tooltip(Some("Protection IV"), true, 2, 30, 3, 10);
        assert_eq!(
            short_level,
            vec!["§f§oProtection IV", "", "§cLevel Requirement: 30"]
        );
        let single = enchant_tooltip(None, false, 0, 4, 1, 4);
        assert_eq!(single, vec!["§71 Lapis Lazuli", "§71 Level"]);
        // An id no registry entry names still separates: the clue line stays
        // out but the blank does not (`:247` names nothing, `:255` still
        // separates).
        let unknown = enchant_tooltip(None, true, 1, 7, 2, 7);
        assert_eq!(unknown[0], "");
        assert_eq!(unknown[1], "§72 Lapis Lazuli");
        assert_eq!(enchant_clue_name(1024), Some(String::from("Protection IV")));
    }

    #[test]
    fn the_glyph_name_reseeds_from_the_table_seed() {
        let words = enchant_words();
        assert_eq!(words.len(), 55);
        // Cross-checked against an independent transcription of
        // `java.util.Random`: seed 0 names this exact name.
        assert_eq!(glyph_name(0), "galvanize ignite imbue beast");
        let first = glyph_name(0);
        assert_eq!(glyph_name(0), first, "the same seed names the same name");
        let parts: Vec<&str> = first.split(' ').collect();
        assert!(
            (3..=4).contains(&parts.len()),
            "three or four words, got {first:?}"
        );
        for part in &parts {
            assert!(words.contains(part), "{part:?} is not in the word list");
        }
        assert_ne!(glyph_name(0), glyph_name(12345), "seeds discriminate");
    }

    #[test]
    fn the_book_opens_toward_levels_and_turns_its_pages() {
        let mut book = BookAnim::default();
        book.step(&[30, 0, 0], None);
        assert_eq!(book.tick, 1);
        assert_eq!(book.open, 0.2);
        book.step(&[30, 0, 0], None);
        assert_eq!(book.open, 0.4);
        // No levels anywhere closes it again, floored at zero.
        for _ in 0..10 {
            book.step(&[0, 0, 0], None);
        }
        assert_eq!(book.open, 0.0);
        // The page flips clamp the fractional rotation (f32 arithmetic: the
        // expectation recomputes in f32 so the rounding matches exactly).
        let (open, f3, f4) = BookAnim::default().frame(1.0);
        assert_eq!(
            (open, f3, f4),
            (0.0, 0.25f32 * 1.6 - 0.3, 0.75f32 * 1.6 - 0.3)
        );
    }

    #[test]
    fn the_villager_pager_walks_one_recipe_and_sends_the_page() {
        let mut pager = VillagerSelection::default();
        assert!(!pager.step(true), "no offers, no move");
        pager.set_offers(vec![offer(0, 7), offer(7, 7), offer(3, 7)]);
        assert!(!pager_enabled(0, 3, false));
        assert!(pager_enabled(0, 3, true));
        assert!(pager.step(true));
        assert_eq!(pager.selected, 1);
        assert!(pager_enabled(1, 3, true));
        assert!(pager_enabled(1, 3, false));
        assert!(pager.step(true));
        assert!(!pager.step(true), "the last page holds");
        assert_eq!(pager.selected, 2);
        assert!(!pager_enabled(2, 3, true));
        assert!(pager.step(false));
        assert_eq!(pager.selected, 1);
        assert_eq!(
            trsel_event(1),
            InputEvent::CustomPayload {
                channel: String::from("MC|TrSel"),
                data: vec![0, 0, 0, 1],
            }
        );
        // The used-out offer locks; the pager still shows it.
        assert!(pager.current().is_some_and(|recipe| recipe.is_disabled()));
        pager.step(true);
        assert!(pager.current().is_some_and(|recipe| !recipe.is_disabled()));
        assert!(!offer(6, 7).is_disabled());
        assert!(offer(7, 7).is_disabled());
        // A shorter list clamps the selection.
        pager.set_offers(vec![offer(0, 7)]);
        assert_eq!(pager.selected, 0);
        // The pager hitboxes.
        assert!(pager_hit(VILLAGER_NEXT_POS, 148, 24));
        assert!(pager_hit(VILLAGER_PREV_POS, 18, 30));
        assert!(!pager_hit(VILLAGER_PREV_POS, 30, 30));
        assert_eq!(merchant_button_uv(true, false, true), (176, 0));
        assert_eq!(merchant_button_uv(true, true, true), (188, 0));
        assert_eq!(merchant_button_uv(false, false, false), (200, 19));
    }

    #[test]
    fn the_anvil_cost_gates_on_the_property_and_the_output() {
        assert_eq!(anvil_cost(0, false, true, true), None);
        assert_eq!(
            anvil_cost(5, false, true, true),
            Some(AnvilCost {
                value: 5,
                expensive: false,
                colour: 8_453_920,
            })
        );
        assert_eq!(
            anvil_cost(5, false, true, false)
                .expect("the untakeable output still lines")
                .colour,
            16_736_352,
            "the untakeable output reddens"
        );
        assert_eq!(
            anvil_cost(5, false, false, false),
            None,
            "inputs without an output hide the line"
        );
        let expensive = anvil_cost(40, false, true, true).expect("past the cap");
        assert!(expensive.expensive);
        assert_eq!(expensive.colour, 16_736_352);
        assert!(
            !anvil_cost(40, true, true, true)
                .expect("creative")
                .expensive
        );
        assert_eq!(anvil_cost_x(20), 148);
        assert_eq!(ANVIL_COST_Y, 67);
        assert_eq!(anvil_strip_v(true), 166);
        assert_eq!(anvil_strip_v(false), 182);
        assert!(anvil_arrow_broken(true, false, false));
        assert!(!anvil_arrow_broken(true, false, true));
        assert!(!anvil_arrow_broken(false, false, false));
    }

    #[test]
    fn the_name_field_edits_like_the_chat_subset_and_sends_per_keystroke() {
        let mut field = NameField::default();
        field.click(70, 26);
        assert!(field.is_focused());
        field.click(10, 10);
        assert!(!field.is_focused());
        field.click(70, 26);
        assert!(field.type_text("Sword"));
        assert_eq!(field.text(), "Sword");
        assert_eq!(field.cursor(), 5);
        assert_eq!(field.key(Key::ArrowLeft), FieldKey::Consumed);
        assert_eq!(field.cursor(), 4);
        assert!(field.type_text("o"));
        assert_eq!(field.text(), "Sworod");
        assert_eq!(field.key(Key::Backspace), FieldKey::Consumed);
        assert_eq!(field.text(), "Sword");
        // The cap holds at thirty characters: the over-long append lands
        // only its fitting part, and a capped field sends nothing.
        assert!(field.type_text(&"x".repeat(30)));
        assert_eq!(field.text().chars().count(), 30);
        assert!(!field.type_text("y"));
        assert_eq!(field.key(Key::Enter), FieldKey::Character);
        // The send carries the raw string, varint-prefixed.
        assert_eq!(
            item_name_event("Hi"),
            InputEvent::CustomPayload {
                channel: String::from("MC|ItemName"),
                data: vec![2, b'H', b'i'],
            }
        );
        // The slot-0 sync resets on presence changes and re-fires.
        let mut sync = NameField::default();
        assert_eq!(anvil_sync(&mut sync, None), None);
        let event = anvil_sync(&mut sync, Some(&stack(267, 1))).expect("a fill re-fires");
        assert!(matches!(event, InputEvent::CustomPayload { .. }));
        assert_eq!(anvil_sync(&mut sync, Some(&stack(267, 1))), None);
        assert_eq!(anvil_sync(&mut sync, None), None);
        assert_eq!(sync.text(), "");
        assert!(name_enabled(true));
        assert!(!name_enabled(false));
    }
}

/// One beacon effect's face: the potion id, its English name (the port
/// renders English under the title convention, as `enchants.rs` does — the
/// source localises `Potion.getName`; not re-derived from lang this pass)
/// and its `inventoryBackground` strip index (`Potion.setIconIndex(x, y)`
/// reads `x + y * 8`, `Potion.java`:26-38,138).
pub const BEACON_EFFECTS: [(i32, &str, i32); 6] = [
    (1, "Speed", 0),
    (3, "Haste", 2),
    (5, "Strength", 4),
    (10, "Regeneration", 7),
    (11, "Resistance", 14),
    (8, "Jump Boost", 10),
];

/// One beacon effect's English name, if it is one of the six.
pub fn potion_name(effect: i32) -> Option<&'static str> {
    BEACON_EFFECTS
        .iter()
        .find(|entry| entry.0 == effect)
        .map(|entry| entry.1)
}

/// One beacon effect's strip index, if it is one of the six.
pub fn potion_icon(effect: i32) -> Option<i32> {
    BEACON_EFFECTS
        .iter()
        .find(|entry| entry.0 == effect)
        .map(|entry| entry.2)
}

/// One effect icon's sheet origin on the inventory sheet: the strip formula
/// the `PowerButton` constructor carries — `(index % 8 * 18, 198 + index / 8
/// * 18)` (`GuiBeacon.java`:305).
pub fn potion_icon_uv(index: i32) -> (i32, i32) {
    ((index % 8) * 18, 198 + (index / 8) * 18)
}

/// The inventory sheet's port key (`GuiContainer.inventoryBackground`).
pub const INVENTORY_SHEET: &str = "gui/container/inventory";

/// The confirm button's hover text: `gui.done` in English.
pub const BEACON_DONE_TEXT: &str = "Done";
/// The cancel button's hover text: `gui.cancel` in English.
pub const BEACON_CANCEL_TEXT: &str = "Cancel";

/// One beacon button's hover text (`drawButtonForegroundLayer`): the confirm
/// and cancel microcopy, else the potion name with ` II` past tier 2 unless
/// it is regeneration (`GuiBeacon.java`:278-294, :309-319).
pub fn beacon_tooltip(rows: &[BeaconRow], id: i32) -> Option<String> {
    if id == -1 {
        return Some(String::from(BEACON_DONE_TEXT));
    }
    if id == -2 {
        return Some(String::from(BEACON_CANCEL_TEXT));
    }
    let row = rows.iter().find(|row| row.id == id)?;
    let mut text = String::from(potion_name(row.effect)?);
    if row.tier >= 3 && row.effect != BEACON_SECONDARY[0] {
        text.push_str(" II");
    }
    Some(text)
}

/// Reads one window property by index: an absent index reads 0 (the same
/// rule `family_a` carries).
fn prop(properties: &[i16], index: usize) -> i32 {
    properties.get(index).copied().unwrap_or(0) as i32
}

/// Folds one bound-button press into the family-B widgets: the beacon's rows
/// and confirm/cancel, the enchanting offers, the villager pager and the
/// anvil field's focus. Widget clicks answer the left press only (recorded);
/// every other button runs the base path alone. The pointer the fold reads
/// is the screen's panel-local mouse, fed before the press.
///
/// The beacon confirm sends its `MC|Beacon` pair and then closes (the source
/// sends the C17 then `closeScreen`, which carries the C0D —
/// `GuiBeacon.actionPerformed`:137-144); cancel closes alone (`:133-136`).
/// The enchanting offer sends `EnchantItem` through the container's gate
/// (`GuiEnchantment.mouseClicked`:91-99); the pager sends `MC|TrSel` only
/// when the page moved (`GuiMerchant.actionPerformed`:102-124); the anvil
/// field only focuses (`GuiRepair.mouseClicked` → `setFocused`).
pub fn family_click(
    screen: &mut ContainerScreen,
    button: ClickButton,
    level: i32,
) -> Vec<InputEvent> {
    let kind = screen.kind();
    let window_id = screen.window_id();
    let (mx, my) = screen.panel_mouse();
    let (x, y) = (mx as i32, my as i32);
    match kind {
        WindowKind::Beacon => {
            if button != ClickButton::Left {
                return Vec::new();
            }
            let properties = screen.properties().to_vec();
            let levels = prop(&properties, 0);
            let payment = screen.slot_stack(0).cloned().flatten().is_some();
            let mut selection = match screen.family() {
                FamilyState::Beacon(selection) => *selection,
                _ => BeaconSelection {
                    primary: prop(&properties, 1),
                    secondary: prop(&properties, 2),
                },
            };
            let rows = beacon_rows(levels, selection.primary, selection.secondary);
            let mut events = Vec::new();
            if let Some(id) = beacon_hit(&rows, x, y) {
                beacon_select(&mut selection, &rows, id);
            } else if x >= BEACON_CONFIRM_POS.0
                && x < BEACON_CONFIRM_POS.0 + BEACON_BUTTON_SIDE
                && y >= BEACON_CONFIRM_POS.1
                && y < BEACON_CONFIRM_POS.1 + BEACON_BUTTON_SIDE
            {
                if beacon_confirm_enabled(payment, selection.primary) {
                    events.push(beacon_event(selection.primary, selection.secondary));
                    events.push(InputEvent::CloseWindow { window_id });
                }
            } else if x >= BEACON_CANCEL_POS.0
                && x < BEACON_CANCEL_POS.0 + BEACON_BUTTON_SIDE
                && y >= BEACON_CANCEL_POS.1
                && y < BEACON_CANCEL_POS.1 + BEACON_BUTTON_SIDE
            {
                events.push(InputEvent::CloseWindow { window_id });
            }
            *screen.family_mut() = FamilyState::Beacon(selection);
            events
        }
        WindowKind::EnchantingTable => {
            if button != ClickButton::Left {
                return Vec::new();
            }
            let properties = screen.properties().to_vec();
            let costs = [
                prop(&properties, 0),
                prop(&properties, 1),
                prop(&properties, 2),
            ];
            let lapis = screen
                .slot_stack(1)
                .cloned()
                .flatten()
                .map_or(0, |stack| i32::from(stack.count));
            let has_item = screen.slot_stack(0).cloned().flatten().is_some();
            for k in 0..3 {
                if (ENCHANT_ROW_X..ENCHANT_ROW_X + ENCHANT_ROW_W).contains(&x)
                    && y >= enchant_row_y(k)
                    && y < enchant_row_y(k) + ENCHANT_ROW_H
                    && enchant_gate(k as usize, costs[k as usize], has_item, lapis, level)
                {
                    return vec![InputEvent::EnchantItem {
                        window_id,
                        index: k as i8,
                    }];
                }
            }
            Vec::new()
        }
        WindowKind::Villager => {
            if button != ClickButton::Left {
                return Vec::new();
            }
            let mut send: Option<InputEvent> = None;
            if let FamilyState::Villager(pager) = screen.family_mut() {
                if pager_hit(VILLAGER_NEXT_POS, x, y) {
                    if pager.step(true) {
                        send = Some(trsel_event(pager.selected as i32));
                    }
                } else if pager_hit(VILLAGER_PREV_POS, x, y) && pager.step(false) {
                    send = Some(trsel_event(pager.selected as i32));
                }
            }
            send.into_iter().collect()
        }
        WindowKind::Anvil => {
            if let FamilyState::Anvil(field) = screen.family_mut() {
                field.click(x, y);
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// Folds one editing key into the anvil's name field: Backspace and the
/// sideways arrows edit and re-fire `MC|ItemName`; Enter and Tab fall
/// through to the container; every other key is the character path's
/// ([`family_type`]). A disabled (slot-0-empty) or unfocused field answers
/// nothing — the source's `textboxKeyTyped` gates on focus, and the port's
/// enabled gate is the slot-0 presence.
pub fn family_key(screen: &mut ContainerScreen, key: Key) -> Vec<InputEvent> {
    if matches!(key, Key::Enter | Key::Tab) {
        return Vec::new();
    }
    if screen.kind() != WindowKind::Anvil {
        return Vec::new();
    }
    let present = screen.slot_stack(0).cloned().flatten().is_some();
    if !name_enabled(present) {
        return Vec::new();
    }
    if let FamilyState::Anvil(field) = screen.family_mut() {
        if !field.is_focused() {
            return Vec::new();
        }
        match field.key(key) {
            FieldKey::Consumed => vec![item_name_event(field.text())],
            FieldKey::Character | FieldKey::Ignored => Vec::new(),
        }
    } else {
        Vec::new()
    }
}

/// Folds typed text into the anvil's name field — the character path
/// `GuiTextField.writeText` runs for keys the field does not own: a landed
/// append re-fires `MC|ItemName` with the raw string, a fully refused one
/// sends nothing.
pub fn family_type(screen: &mut ContainerScreen, text: &str) -> Vec<InputEvent> {
    if screen.kind() != WindowKind::Anvil {
        return Vec::new();
    }
    let present = screen.slot_stack(0).cloned().flatten().is_some();
    if !name_enabled(present) {
        return Vec::new();
    }
    if let FamilyState::Anvil(field) = screen.family_mut() {
        if !field.is_focused() {
            return Vec::new();
        }
        if field.type_text(text) {
            return vec![item_name_event(field.text())];
        }
    }
    Vec::new()
}

/// Whether the anvil's name field owns the next key: the screen stands on
/// the anvil with an input in slot 0 and the field holds focus. The caller
/// routes editing keys and characters to the field and keeps the
/// container's number/drop keys out (`GuiRepair.keyTyped`:117-134 runs
/// `textboxKeyTyped` before `super.keyTyped`).
pub fn field_owns_input(screen: &ContainerScreen) -> bool {
    if screen.kind() != WindowKind::Anvil {
        return false;
    }
    let present = screen.slot_stack(0).cloned().flatten().is_some();
    if !name_enabled(present) {
        return false;
    }
    matches!(screen.family(), FamilyState::Anvil(field) if field.is_focused())
}

/// Steps the family-B widgets one session tick: the enchanting book chases
/// its levels and the anvil field blinks (`GuiEnchantment.func_147068_g`,
/// `GuiTextField.updateCursorCounter`). The beacon's selection reseeds from
/// the properties on snapshots, not here.
pub fn tick_family(screen: &mut ContainerScreen) {
    let properties = screen.properties().to_vec();
    let slot0 = screen.slot_stack(0).cloned().flatten();
    match screen.family_mut() {
        FamilyState::Enchanting(book) => {
            let levels = [
                properties.first().copied().unwrap_or(0),
                properties.get(1).copied().unwrap_or(0),
                properties.get(2).copied().unwrap_or(0),
            ];
            book.step(&levels, slot0.as_ref());
        }
        FamilyState::Anvil(field) => field.tick(),
        FamilyState::Beacon(_)
        | FamilyState::Villager(_)
        | FamilyState::Horse(_)
        | FamilyState::None => {}
    }
}
