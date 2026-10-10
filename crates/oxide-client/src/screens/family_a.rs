//! The container family A: the chest, hopper, dispenser, furnace, brewing
//! stand and crafting table layouts with their property draws.
//!
//! The port reads values and names from the source's containers
//! (`ContainerChest`, `ContainerHopper`, `ContainerDispenser`,
//! `ContainerFurnace`, `ContainerBrewingStand`, `ContainerWorkbench`) and
//! their GUIs (`GuiChest`, `GuiHopper`, `GuiDispenser`, `GuiFurnace`,
//! `GuiBrewingStand`, `GuiCrafting`); no source text is copied. Two source
//! traps are recorded: `GuiHopper` lives in `client/gui/`, not
//! `client/gui/inventory/` like the other five, and `GuiChest`'s ctor param
//! names are inverted vs physical position (`upperInv` is the PLAYER
//! inventory, `lowerChestInventory` the CHEST one whose name draws on top).
//!
//! Brewing carries FOUR slots (three `Potion` + one `Ingredient`) and ONE
//! property (`brewTime`, id 0): both the fill bar and the seven-frame bubble
//! animation derive from it. The dropper shares the dispenser's layout. The
//! furnace arrow's `l + 1` blit leaves a 1-px remnant at `l == 0`; the port
//! guards it and records the divergence.

use oxide_proto_v47::window::WindowKind;

use super::container::{
    BackgroundKind, ContainerLayout, GENERIC_LAYOUT, SlotBlock, SlotPos, TitleKind, TitleSource,
};

/// One sheet-space blit: the panel-dest rect and the sheet-src origin, all
/// in texels. The frame divides the source rect by the sheet's 256-texel
/// side into the draw's uv.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SheetBlit {
    /// The dest rect's left edge in panel units.
    pub dx: i32,
    /// The dest rect's top edge in panel units.
    pub dy: i32,
    /// The rect's width in texels.
    pub w: i32,
    /// The rect's height in texels.
    pub h: i32,
    /// The source rect's left edge in sheet texels.
    pub sx: i32,
    /// The source rect's top edge in sheet texels.
    pub sy: i32,
}

/// The chest's row counts the port lays out: `rows = slot count / 9`,
/// clamped to 1..=6 (`GuiChest.java`:23-30 generalises; only 3- and 6-row
/// windows occur from vanilla tiles).
pub const CHEST_MIN_ROWS: usize = 1;
/// The chest's row counts the port lays out (see [`CHEST_MIN_ROWS`]).
pub const CHEST_MAX_ROWS: usize = 6;

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
const fn standard_player_block(out: &mut [SlotPos], base: i16, start: usize) {
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

macro_rules! chest_table {
    ($build:ident, $slots:ident, $layout:ident, $rows:expr) => {
        const fn $build() -> [SlotPos; $rows * 9 + 36] {
            let mut out = [FILLER; $rows * 9 + 36];
            // The chest block: rows × 9 at `(8 + k·18, 18 + j·18)`
            // (`ContainerChest.java`:16-33).
            let mut row: i32 = 0;
            while row < $rows {
                let mut col: i32 = 0;
                while col < 9 {
                    out[(row * 9 + col) as usize] = SlotPos {
                        index: (row * 9 + col) as i16,
                        x: 8 + col * 18,
                        y: 18 + row * 18,
                        block: SlotBlock::Container,
                    };
                    col += 1;
                }
                row += 1;
            }
            // The player block, shifted `(rows − 4) × 18` (`:34-37`).
            let shift = ($rows - 4) * 18;
            let mut prow: i32 = 0;
            while prow < 3 {
                let mut col: i32 = 0;
                while col < 9 {
                    out[($rows * 9 + prow * 9 + col) as usize] = SlotPos {
                        index: ($rows * 9 + prow * 9 + col) as i16,
                        x: 8 + col * 18,
                        y: 103 + prow * 18 + shift,
                        block: SlotBlock::Player,
                    };
                    col += 1;
                }
                prow += 1;
            }
            let mut col: i32 = 0;
            while col < 9 {
                out[($rows * 9 + 27 + col) as usize] = SlotPos {
                    index: ($rows * 9 + 27 + col) as i16,
                    x: 8 + col * 18,
                    y: 161 + shift,
                    block: SlotBlock::Player,
                };
                col += 1;
            }
            out
        }
        static $slots: [SlotPos; $rows * 9 + 36] = $build();
        /// The chest layout for its row count: `176 × (114 + rows × 18)` on
        /// the `generic_54` sheet, the window's own title on top.
        pub static $layout: ContainerLayout = ContainerLayout {
            x_size: 176,
            y_size: 114 + $rows * 18,
            sheet: "gui/container/generic_54",
            slots: &$slots,
            title: TitleKind::Chest {
                lower: TitleSource::Fixed("Inventory"),
            },
            background: BackgroundKind::ChestSplit { rows: $rows },
        };
    };
}

chest_table!(build_chest_1, CHEST_9_SLOTS, CHEST_9, 1);
chest_table!(build_chest_2, CHEST_18_SLOTS, CHEST_18, 2);
chest_table!(build_chest_3, CHEST_27_SLOTS, CHEST_27, 3);
chest_table!(build_chest_4, CHEST_36_SLOTS, CHEST_36, 4);
chest_table!(build_chest_5, CHEST_45_SLOTS, CHEST_45, 5);
chest_table!(build_chest_6, CHEST_90_SLOTS, CHEST_90, 6);

/// Every chest layout by row count: index `rows − 1`.
pub static CHEST_LAYOUTS: [&ContainerLayout; 6] = [
    &CHEST_9, &CHEST_18, &CHEST_27, &CHEST_36, &CHEST_45, &CHEST_90,
];

/// The hopper's five at `(44 + j·18, 20)` (`ContainerHopper.java`:12-33),
/// then the player block at base 5 (`:44-66` — main rows 51/69/87, hotbar
/// 109).
const fn hopper_slots() -> [SlotPos; 41] {
    let mut out = [FILLER; 41];
    let mut col: i32 = 0;
    while col < 5 {
        out[col as usize] = SlotPos {
            index: col as i16,
            x: 44 + col * 18,
            y: 20,
            block: SlotBlock::Container,
        };
        col += 1;
    }
    // The hopper's own player block at base 5: main rows 51/69/87, hotbar
    // 109 (`ContainerHopper.java`:44-66 — `int i = 51`, rows at `l·18 + i`,
    // hotbar at `58 + i`).
    let mut row: i32 = 0;
    while row < 3 {
        let mut pcol: i32 = 0;
        while pcol < 9 {
            out[(5 + row * 9 + pcol) as usize] = SlotPos {
                index: 5 + (row * 9 + pcol) as i16,
                x: 8 + pcol * 18,
                y: 51 + row * 18,
                block: SlotBlock::Player,
            };
            pcol += 1;
        }
        row += 1;
    }
    let mut hcol: i32 = 0;
    while hcol < 9 {
        out[(32 + hcol) as usize] = SlotPos {
            index: (32 + hcol) as i16,
            x: 8 + hcol * 18,
            y: 109,
            block: SlotBlock::Player,
        };
        hcol += 1;
    }
    out
}

static HOPPER_SLOTS: [SlotPos; 41] = hopper_slots();

/// The hopper's layout: 5 slots on the `hopper` sheet, 176×133.
pub static HOPPER: ContainerLayout = ContainerLayout {
    x_size: 176,
    y_size: 133,
    sheet: "gui/container/hopper",
    slots: &HOPPER_SLOTS,
    title: TitleKind::Chest {
        lower: TitleSource::Fixed("Inventory"),
    },
    background: BackgroundKind::Full,
};

/// The dispenser's 3×3 at `(62 + j·18, 17 + i·18)`
/// (`ContainerDispenser.java`:12-33), then the standard player block at base
/// 9 (`:34-56`).
const fn dispenser_slots() -> [SlotPos; 45] {
    let mut out = [FILLER; 45];
    let mut row: i32 = 0;
    while row < 3 {
        let mut col: i32 = 0;
        while col < 3 {
            out[(row * 3 + col) as usize] = SlotPos {
                index: (row * 3 + col) as i16,
                x: 62 + col * 18,
                y: 17 + row * 18,
                block: SlotBlock::Container,
            };
            col += 1;
        }
        row += 1;
    }
    standard_player_block(&mut out, 9, 9);
    out
}

static DISPENSER_SLOTS: [SlotPos; 45] = dispenser_slots();

/// The dispenser's layout: the 3×3 grid on the `dispenser` sheet, shared by
/// the dropper.
pub static DISPENSER: ContainerLayout = ContainerLayout {
    x_size: 176,
    y_size: 166,
    sheet: "gui/container/dispenser",
    slots: &DISPENSER_SLOTS,
    title: TitleKind::Centred {
        lower: TitleSource::Fixed("Inventory"),
    },
    background: BackgroundKind::Full,
};

/// The furnace's three — input `(56, 17)`, fuel `(56, 53)`, output `(116,
/// 35)` (`ContainerFurnace.java`:20-37) — then the standard player block at
/// base 3 (`:38-60`).
const fn furnace_slots() -> [SlotPos; 39] {
    let mut out = [FILLER; 39];
    out[0] = SlotPos {
        index: 0,
        x: 56,
        y: 17,
        block: SlotBlock::Container,
    };
    out[1] = SlotPos {
        index: 1,
        x: 56,
        y: 53,
        block: SlotBlock::Container,
    };
    out[2] = SlotPos {
        index: 2,
        x: 116,
        y: 35,
        block: SlotBlock::Container,
    };
    standard_player_block(&mut out, 3, 3);
    out
}

static FURNACE_SLOTS: [SlotPos; 39] = furnace_slots();

/// The furnace's layout: input, fuel and output on the `furnace` sheet.
pub static FURNACE: ContainerLayout = ContainerLayout {
    x_size: 176,
    y_size: 166,
    sheet: "gui/container/furnace",
    slots: &FURNACE_SLOTS,
    title: TitleKind::Centred {
        lower: TitleSource::Fixed("Inventory"),
    },
    background: BackgroundKind::Full,
};

/// The brewing stand's four — potions `(56, 46)`, `(79, 53)`, `(102, 46)`
/// and the ingredient `(79, 17)` (`ContainerBrewingStand.java`:21-40) — then
/// the standard player block at base 4 (`:41-63`).
const fn brewing_slots() -> [SlotPos; 40] {
    let mut out = [FILLER; 40];
    out[0] = SlotPos {
        index: 0,
        x: 56,
        y: 46,
        block: SlotBlock::Container,
    };
    out[1] = SlotPos {
        index: 1,
        x: 79,
        y: 53,
        block: SlotBlock::Container,
    };
    out[2] = SlotPos {
        index: 2,
        x: 102,
        y: 46,
        block: SlotBlock::Container,
    };
    out[3] = SlotPos {
        index: 3,
        x: 79,
        y: 17,
        block: SlotBlock::Container,
    };
    standard_player_block(&mut out, 4, 4);
    out
}

static BREWING_SLOTS: [SlotPos; 40] = brewing_slots();

/// The brewing stand's layout: three potion slots and the ingredient slot on
/// the `brewing_stand` sheet.
pub static BREWING: ContainerLayout = ContainerLayout {
    x_size: 176,
    y_size: 166,
    sheet: "gui/container/brewing_stand",
    slots: &BREWING_SLOTS,
    title: TitleKind::Centred {
        lower: TitleSource::Fixed("Inventory"),
    },
    background: BackgroundKind::Full,
};

/// The workbench's ten — the result `(124, 35)` and the grid `j + i·3` at
/// `(30 + j·18, 17 + i·18)` (`ContainerWorkbench.java`:24-46) — then the
/// standard player block at base 10 (`:47-69`).
const fn crafting_slots() -> [SlotPos; 46] {
    let mut out = [FILLER; 46];
    out[0] = SlotPos {
        index: 0,
        x: 124,
        y: 35,
        block: SlotBlock::Container,
    };
    let mut row: i32 = 0;
    while row < 3 {
        let mut col: i32 = 0;
        while col < 3 {
            out[(1 + row * 3 + col) as usize] = SlotPos {
                index: 1 + (row * 3 + col) as i16,
                x: 30 + col * 18,
                y: 17 + row * 18,
                block: SlotBlock::Container,
            };
            col += 1;
        }
        row += 1;
    }
    standard_player_block(&mut out, 10, 10);
    out
}

static CRAFTING_SLOTS: [SlotPos; 46] = crafting_slots();

/// The crafting table's layout: the result slot and the 3×3 grid on the
/// `crafting_table` sheet — the family's only fixed top label.
pub static CRAFTING: ContainerLayout = ContainerLayout {
    x_size: 176,
    y_size: 166,
    sheet: "gui/container/crafting_table",
    slots: &CRAFTING_SLOTS,
    title: TitleKind::Crafting {
        top: TitleSource::Fixed("Crafting"),
        lower: TitleSource::Fixed("Inventory"),
    },
    background: BackgroundKind::Full,
};

/// Picks the layout for an opened window: the chest's row count rides the
/// window's slot count (`slot count / 9`, clamped 1..=6); the dropper shares
/// the dispenser's table; the plain `minecraft:container` opens on the
/// chest's pair like the source's else branch; the unlanded kinds keep the
/// generic frame until their tasks (recorded).
pub fn layout_for_kind(kind: WindowKind, slot_count: u8) -> &'static ContainerLayout {
    match kind {
        WindowKind::Chest | WindowKind::Container => {
            let rows = (usize::from(slot_count) / 9).clamp(CHEST_MIN_ROWS, CHEST_MAX_ROWS);
            CHEST_LAYOUTS[rows - 1]
        }
        WindowKind::Hopper => &HOPPER,
        WindowKind::Dispenser | WindowKind::Dropper => &DISPENSER,
        WindowKind::Furnace => &FURNACE,
        WindowKind::BrewingStand => &BREWING,
        WindowKind::CraftingTable => &CRAFTING,
        WindowKind::EnchantingTable
        | WindowKind::Villager
        | WindowKind::Beacon
        | WindowKind::Anvil
        | WindowKind::EntityHorse
        | WindowKind::Unknown => &GENERIC_LAYOUT,
    }
}

/// Reads one window property by index: an absent index reads 0 (Task 5's
/// `apply_property` zero-fills the vec).
pub fn window_property(properties: &[i16], index: usize) -> i16 {
    properties.get(index).copied().unwrap_or(0)
}

/// The furnace flame's height `k` (`getBurnLeftScaled(13)` = `field0 × 13 /
/// field1`, with the `field1 == 0 → 200` fallback): drawn only while
/// `field0 > 0`, so a cold or missing burn reads `None`.
pub fn furnace_flame(properties: &[i16]) -> Option<i32> {
    let burn = i32::from(window_property(properties, 0));
    if burn <= 0 {
        return None;
    }
    let total = i32::from(window_property(properties, 1));
    let total = if total == 0 { 200 } else { total };
    Some(burn * 13 / total)
}

/// The furnace arrow's width `l` (`getCookProgressScaled(24)` = `field2 × 24
/// / field3`, `0` unless both are non-zero): the blit draws `l + 1` wide, so
/// the port guards `l == 0 → None` rather than keeping the source's 1-px
/// remnant (recorded divergence).
pub fn furnace_arrow(properties: &[i16]) -> Option<i32> {
    let cook = i32::from(window_property(properties, 2));
    let total = i32::from(window_property(properties, 3));
    let width = if cook != 0 && total != 0 {
        cook * 24 / total
    } else {
        0
    };
    if width == 0 { None } else { Some(width) }
}

/// The brew time a full bar starts from (`GuiBrewingStand.java`:58 reads
/// `k / 400.0F`).
const BREW_TIME_MAX: f32 = 400.0;
/// The fill bar's full height (`:58` scales 28 px).
const FILL_HEIGHT: f32 = 28.0;

/// The brewing fill bar's height: `28 × (1 − k / 400)` for the brew time `k`
/// (property 0), drawn only while `k > 0` and `l > 0`.
pub fn brewing_fill(properties: &[i16]) -> Option<i32> {
    let brew = i32::from(window_property(properties, 0));
    if brew <= 0 {
        return None;
    }
    let fill = (FILL_HEIGHT * (1.0 - brew as f32 / BREW_TIME_MAX)) as i32;
    if fill <= 0 { None } else { Some(fill) }
}

/// The bubble animation's seven frame heights for `(k / 2) % 7 == 0..6`
/// (`GuiBrewingStand.java`:61-79).
const BUBBLE_FRAMES: [i32; 7] = [29, 24, 20, 16, 11, 6, 0];

/// The brewing bubble frame's height: `(k / 2) % 7` maps onto `29 / 24 / 20
/// / 16 / 11 / 6 / 0`, drawn only while `k > 0` and the frame is non-zero.
pub fn brewing_bubble(properties: &[i16]) -> Option<i32> {
    let brew = i32::from(window_property(properties, 0));
    if brew <= 0 {
        return None;
    }
    let height = BUBBLE_FRAMES[(brew / 2 % 7) as usize];
    if height <= 0 { None } else { Some(height) }
}

/// The background's sheet blits for a layout: the full panel, or the
/// chest's split pair (the upper slice, then the 96-row bottom blit from
/// sheet row 126).
pub fn background_blits(layout: &ContainerLayout) -> Vec<SheetBlit> {
    match layout.background {
        BackgroundKind::Full => vec![SheetBlit {
            dx: 0,
            dy: 0,
            w: layout.x_size,
            h: layout.y_size,
            sx: 0,
            sy: 0,
        }],
        // `GuiChest.java`:45-53: the upper slice `rows × 18 + 17` tall at
        // the panel's top-left, then the 96-row bottom blit from sheet row
        // 126 at `y = rows × 18 + 17`.
        BackgroundKind::ChestSplit { rows } => {
            let split = rows * 18 + 17;
            vec![
                SheetBlit {
                    dx: 0,
                    dy: 0,
                    w: layout.x_size,
                    h: split,
                    sx: 0,
                    sy: 0,
                },
                SheetBlit {
                    dx: 0,
                    dy: split,
                    w: layout.x_size,
                    h: 96,
                    sx: 0,
                    sy: 126,
                },
            ]
        }
    }
}

/// The live property blits for an opened kind: the furnace's flame and
/// arrow, the brewing stand's fill and bubbles; every other kind draws
/// none.
pub fn property_blits(kind: WindowKind, properties: &[i16]) -> Vec<SheetBlit> {
    match kind {
        WindowKind::Furnace => {
            let mut blits = Vec::new();
            // `GuiFurnace.java`:48-52: the flame grows upward from base
            // y + 48 — dest `(+56, +36 + 12 − k)`, src `(176, 12 − k)`,
            // size `(14, k + 1)`.
            if let Some(k) = furnace_flame(properties) {
                blits.push(SheetBlit {
                    dx: 56,
                    dy: 48 - k,
                    w: 14,
                    h: k + 1,
                    sx: 176,
                    sy: 12 - k,
                });
            }
            // `GuiFurnace.java`:54-55: the arrow grows rightward — dest
            // `(+79, +34)`, src `(176, 14)`, size `(l + 1, 16)`.
            if let Some(l) = furnace_arrow(properties) {
                blits.push(SheetBlit {
                    dx: 79,
                    dy: 34,
                    w: l + 1,
                    h: 16,
                    sx: 176,
                    sy: 14,
                });
            }
            blits
        }
        WindowKind::BrewingStand => {
            let mut blits = Vec::new();
            // `GuiBrewingStand.java`:56-59: the fill grows downward from
            // y + 16 — dest `(+97, +16)`, src `(176, 0)`, size `(9, l)`.
            if let Some(l) = brewing_fill(properties) {
                blits.push(SheetBlit {
                    dx: 97,
                    dy: 16,
                    w: 9,
                    h: l,
                    sx: 176,
                    sy: 0,
                });
            }
            // `GuiBrewingStand.java`:61-79: the bubble frame rises — dest
            // `(+65, +14 + 29 − l)`, src `(185, 29 − l)`, size `(12, l)`.
            if let Some(l) = brewing_bubble(properties) {
                blits.push(SheetBlit {
                    dx: 65,
                    dy: 43 - l,
                    w: 12,
                    h: l,
                    sx: 185,
                    sy: 29 - l,
                });
            }
            blits
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tables {
    //! The family-A pins: every layout's size, sheet, slot count and title
    //! rule, three coordinates per table, and the chest's row-count pick.

    use super::*;

    #[test]
    fn the_six_row_chest_lays_out_ninety_slots() {
        assert_eq!((CHEST_90.x_size, CHEST_90.y_size), (176, 222));
        assert_eq!(CHEST_90.sheet, "gui/container/generic_54");
        assert_eq!(
            CHEST_90.slots.len(),
            90,
            "54 chest slots plus the 36 player"
        );
        assert_eq!(
            CHEST_90.slots[0],
            SlotPos {
                index: 0,
                x: 8,
                y: 18,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            CHEST_90.slots[8],
            SlotPos {
                index: 8,
                x: 152,
                y: 18,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            CHEST_90.slots[53],
            SlotPos {
                index: 53,
                x: 152,
                y: 108,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            CHEST_90.slots[54],
            SlotPos {
                index: 54,
                x: 8,
                y: 139,
                block: SlotBlock::Player,
            }
        );
        assert_eq!(
            CHEST_90.slots[89],
            SlotPos {
                index: 89,
                x: 152,
                y: 197,
                block: SlotBlock::Player,
            }
        );
        assert!(
            matches!(
                CHEST_90.title,
                TitleKind::Chest {
                    lower: TitleSource::Fixed("Inventory"),
                }
            ),
            "the chest draws the window's own title on top"
        );
        assert_eq!(
            CHEST_90.background,
            BackgroundKind::ChestSplit { rows: 6 },
            "the same sheet's upper slice plus the bottom blit"
        );
    }

    #[test]
    fn the_three_row_chest_lays_out_sixty_three_slots() {
        assert_eq!((CHEST_27.x_size, CHEST_27.y_size), (176, 168));
        assert_eq!(CHEST_27.sheet, "gui/container/generic_54");
        assert_eq!(
            CHEST_27.slots.len(),
            63,
            "27 chest slots plus the 36 player"
        );
        assert_eq!(
            CHEST_27.slots[0],
            SlotPos {
                index: 0,
                x: 8,
                y: 18,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            CHEST_27.slots[26],
            SlotPos {
                index: 26,
                x: 152,
                y: 54,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            CHEST_27.slots[17],
            SlotPos {
                index: 17,
                x: 152,
                y: 36,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            CHEST_27.slots[27],
            SlotPos {
                index: 27,
                x: 8,
                y: 85,
                block: SlotBlock::Player,
            }
        );
        assert_eq!(
            CHEST_27.slots[62],
            SlotPos {
                index: 62,
                x: 152,
                y: 143,
                block: SlotBlock::Player,
            }
        );
        assert_eq!(
            CHEST_27.background,
            BackgroundKind::ChestSplit { rows: 3 },
            "168 = 114 + 3 × 18"
        );
    }

    #[test]
    fn the_chest_rows_ride_the_window_slot_count() {
        assert_eq!(
            layout_for_kind(WindowKind::Chest, 27).y_size,
            168,
            "a 27-slot window opens the 3-row chest"
        );
        assert_eq!(
            layout_for_kind(WindowKind::Chest, 54).y_size,
            222,
            "a 54-slot window opens the 6-row chest"
        );
        assert_eq!(
            layout_for_kind(WindowKind::Chest, 27).slots.len(),
            63,
            "the 3-row table carries the window's 27 plus the player 36"
        );
        assert_eq!(
            layout_for_kind(WindowKind::Chest, 54).slots.len(),
            90,
            "the 6-row table carries the window's 54 plus the player 36"
        );
    }

    #[test]
    fn the_hopper_lays_out_five_slots() {
        assert_eq!((HOPPER.x_size, HOPPER.y_size), (176, 133));
        assert_eq!(HOPPER.sheet, "gui/container/hopper");
        assert_eq!(HOPPER.slots.len(), 41, "5 hopper slots plus the 36 player");
        assert_eq!(
            HOPPER.slots[0],
            SlotPos {
                index: 0,
                x: 44,
                y: 20,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            HOPPER.slots[2],
            SlotPos {
                index: 2,
                x: 80,
                y: 20,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            HOPPER.slots[4],
            SlotPos {
                index: 4,
                x: 116,
                y: 20,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            HOPPER.slots[5],
            SlotPos {
                index: 5,
                x: 8,
                y: 51,
                block: SlotBlock::Player,
            }
        );
        assert_eq!(
            HOPPER.slots[40],
            SlotPos {
                index: 40,
                x: 152,
                y: 109,
                block: SlotBlock::Player,
            }
        );
        assert!(
            matches!(HOPPER.title, TitleKind::Chest { .. }),
            "the hopper draws the window's own title like the chest"
        );
    }

    #[test]
    fn the_dispenser_lays_out_the_three_by_three() {
        assert_eq!((DISPENSER.x_size, DISPENSER.y_size), (176, 166));
        assert_eq!(DISPENSER.sheet, "gui/container/dispenser");
        assert_eq!(DISPENSER.slots.len(), 45, "9 grid slots plus the 36 player");
        assert_eq!(
            DISPENSER.slots[0],
            SlotPos {
                index: 0,
                x: 62,
                y: 17,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            DISPENSER.slots[4],
            SlotPos {
                index: 4,
                x: 80,
                y: 35,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            DISPENSER.slots[8],
            SlotPos {
                index: 8,
                x: 98,
                y: 53,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            DISPENSER.slots[9],
            SlotPos {
                index: 9,
                x: 8,
                y: 84,
                block: SlotBlock::Player,
            }
        );
        assert_eq!(
            DISPENSER.slots[44],
            SlotPos {
                index: 44,
                x: 152,
                y: 142,
                block: SlotBlock::Player,
            }
        );
        assert!(
            matches!(DISPENSER.title, TitleKind::Centred { .. }),
            "the dispenser centres the window's own title"
        );
        assert_eq!(
            layout_for_kind(WindowKind::Dropper, 9).sheet,
            "gui/container/dispenser",
            "the dropper shares the dispenser's table"
        );
    }

    #[test]
    fn the_furnace_lays_out_input_fuel_and_output() {
        assert_eq!((FURNACE.x_size, FURNACE.y_size), (176, 166));
        assert_eq!(FURNACE.sheet, "gui/container/furnace");
        assert_eq!(
            FURNACE.slots.len(),
            39,
            "3 furnace slots plus the 36 player"
        );
        assert_eq!(
            FURNACE.slots[0],
            SlotPos {
                index: 0,
                x: 56,
                y: 17,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            FURNACE.slots[1],
            SlotPos {
                index: 1,
                x: 56,
                y: 53,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            FURNACE.slots[2],
            SlotPos {
                index: 2,
                x: 116,
                y: 35,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            FURNACE.slots[3],
            SlotPos {
                index: 3,
                x: 8,
                y: 84,
                block: SlotBlock::Player,
            }
        );
        assert_eq!(
            FURNACE.slots[38],
            SlotPos {
                index: 38,
                x: 152,
                y: 142,
                block: SlotBlock::Player,
            }
        );
        assert!(matches!(FURNACE.title, TitleKind::Centred { .. }));
    }

    #[test]
    fn the_brewing_stand_lays_out_four_slots() {
        assert_eq!((BREWING.x_size, BREWING.y_size), (176, 166));
        assert_eq!(BREWING.sheet, "gui/container/brewing_stand");
        assert_eq!(
            BREWING.slots.len(),
            40,
            "4 brewing slots plus the 36 player"
        );
        assert_eq!(
            BREWING.slots[0],
            SlotPos {
                index: 0,
                x: 56,
                y: 46,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            BREWING.slots[1],
            SlotPos {
                index: 1,
                x: 79,
                y: 53,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            BREWING.slots[2],
            SlotPos {
                index: 2,
                x: 102,
                y: 46,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            BREWING.slots[3],
            SlotPos {
                index: 3,
                x: 79,
                y: 17,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            BREWING.slots[4],
            SlotPos {
                index: 4,
                x: 8,
                y: 84,
                block: SlotBlock::Player,
            }
        );
        assert!(matches!(BREWING.title, TitleKind::Centred { .. }));
    }

    #[test]
    fn the_crafting_table_lays_out_result_and_grid() {
        assert_eq!((CRAFTING.x_size, CRAFTING.y_size), (176, 166));
        assert_eq!(CRAFTING.sheet, "gui/container/crafting_table");
        assert_eq!(
            CRAFTING.slots.len(),
            46,
            "10 crafting slots plus the 36 player"
        );
        assert_eq!(
            CRAFTING.slots[0],
            SlotPos {
                index: 0,
                x: 124,
                y: 35,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            CRAFTING.slots[1],
            SlotPos {
                index: 1,
                x: 30,
                y: 17,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            CRAFTING.slots[5],
            SlotPos {
                index: 5,
                x: 48,
                y: 35,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            CRAFTING.slots[9],
            SlotPos {
                index: 9,
                x: 66,
                y: 53,
                block: SlotBlock::Container,
            }
        );
        assert_eq!(
            CRAFTING.slots[10],
            SlotPos {
                index: 10,
                x: 8,
                y: 84,
                block: SlotBlock::Player,
            }
        );
        assert!(
            matches!(
                CRAFTING.title,
                TitleKind::Crafting {
                    top: TitleSource::Fixed("Crafting"),
                    ..
                }
            ),
            "crafting's fixed top label is the family's only one"
        );
    }

    #[test]
    fn every_family_kind_resolves_its_own_layout() {
        assert_eq!(layout_for_kind(WindowKind::Hopper, 5).sheet, HOPPER.sheet);
        assert_eq!(
            layout_for_kind(WindowKind::Dispenser, 9).sheet,
            DISPENSER.sheet
        );
        assert_eq!(layout_for_kind(WindowKind::Furnace, 3).sheet, FURNACE.sheet);
        assert_eq!(
            layout_for_kind(WindowKind::BrewingStand, 4).sheet,
            BREWING.sheet
        );
        assert_eq!(
            layout_for_kind(WindowKind::CraftingTable, 10).sheet,
            CRAFTING.sheet
        );
    }
}

#[cfg(test)]
mod properties {
    //! The property-draw pins: the furnace flame and arrow scales, the
    //! brewing fill and bubble frames, and the missing-property fallbacks.

    use super::*;

    #[test]
    fn the_flame_scales_burn_over_total() {
        // Field 1 rides the fixture explicitly — 200 is only the field1 == 0
        // fallback, never a constant.
        assert_eq!(furnace_flame(&[0, 200, 0, 0]), None, "cold draws nothing");
        assert_eq!(furnace_flame(&[100, 200, 0, 0]), Some(6));
        assert_eq!(furnace_flame(&[200, 200, 0, 0]), Some(13));
        assert_eq!(
            furnace_flame(&[100, 0, 0, 0]),
            Some(6),
            "a missing total falls back to 200"
        );
        assert_eq!(furnace_flame(&[]), None, "no burn field draws nothing");
    }

    #[test]
    fn the_arrow_scales_cook_over_total() {
        assert_eq!(
            furnace_arrow(&[0, 0, 0, 200]),
            None,
            "the guard skips l == 0"
        );
        assert_eq!(furnace_arrow(&[0, 0, 100, 200]), Some(12));
        assert_eq!(furnace_arrow(&[0, 0, 200, 200]), Some(24));
        assert_eq!(
            furnace_arrow(&[0, 0, 100, 0]),
            None,
            "a missing total cooks nothing"
        );
        assert_eq!(furnace_arrow(&[]), None, "no cook fields draw nothing");
    }

    #[test]
    fn the_brewing_fill_empties_as_the_brew_runs() {
        assert_eq!(brewing_fill(&[400]), None, "a fresh brew fills nothing");
        assert_eq!(brewing_fill(&[200]), Some(14));
        assert_eq!(brewing_fill(&[1]), Some(27));
        assert_eq!(brewing_fill(&[0]), None, "a done brew draws nothing");
        assert_eq!(brewing_fill(&[]), None, "no brew time draws nothing");
    }

    #[test]
    fn the_brewing_bubbles_step_through_seven_frames() {
        // (k / 2) % 7 selects 29 / 24 / 20 / 16 / 11 / 6 / 0.
        assert_eq!(brewing_bubble(&[400]), Some(11), "200 % 7 is frame 4");
        assert_eq!(brewing_bubble(&[200]), Some(20), "100 % 7 is frame 2");
        let frames: Vec<Option<i32>> = [0, 14, 2, 4, 6, 8, 10]
            .iter()
            .map(|k| brewing_bubble(&[*k]))
            .collect();
        assert_eq!(
            frames,
            vec![
                None,
                Some(29),
                Some(24),
                Some(20),
                Some(16),
                Some(11),
                Some(6),
            ],
            "k = 0 draws nothing, then the frames run 29 down to 6"
        );
        assert_eq!(
            brewing_bubble(&[12]),
            None,
            "(12 / 2) % 7 is frame 6, whose height is 0 — nothing draws"
        );
    }
}

#[cfg(test)]
mod blits {
    //! The sheet-slice pins: the chest's split pair, the furnace flame and
    //! arrow rects, the brewing fill and bubble rects.

    use super::*;

    #[test]
    fn the_chest_background_splits_at_the_row_slice() {
        // 3 rows: the upper slice 3 × 18 + 17 = 71 tall, the 96-row bottom
        // from sheet row 126 at y 71.
        assert_eq!(
            background_blits(&CHEST_27),
            vec![
                SheetBlit {
                    dx: 0,
                    dy: 0,
                    w: 176,
                    h: 71,
                    sx: 0,
                    sy: 0,
                },
                SheetBlit {
                    dx: 0,
                    dy: 71,
                    w: 176,
                    h: 96,
                    sx: 0,
                    sy: 126,
                },
            ]
        );
        // 6 rows: the upper slice 6 × 18 + 17 = 125 tall.
        assert_eq!(
            background_blits(&CHEST_90),
            vec![
                SheetBlit {
                    dx: 0,
                    dy: 0,
                    w: 176,
                    h: 125,
                    sx: 0,
                    sy: 0,
                },
                SheetBlit {
                    dx: 0,
                    dy: 125,
                    w: 176,
                    h: 96,
                    sx: 0,
                    sy: 126,
                },
            ]
        );
    }

    #[test]
    fn the_full_panel_blits_once() {
        assert_eq!(
            background_blits(&FURNACE),
            vec![SheetBlit {
                dx: 0,
                dy: 0,
                w: 176,
                h: 166,
                sx: 0,
                sy: 0,
            }]
        );
    }

    #[test]
    fn the_mid_burn_furnace_blits_flame_and_arrow() {
        // burn 100/200 → k = 6; cook 100/200 → l = 12, drawn 13 wide.
        assert_eq!(
            property_blits(WindowKind::Furnace, &[100, 200, 100, 200]),
            vec![
                SheetBlit {
                    dx: 56,
                    dy: 42,
                    w: 14,
                    h: 7,
                    sx: 176,
                    sy: 6,
                },
                SheetBlit {
                    dx: 79,
                    dy: 34,
                    w: 13,
                    h: 16,
                    sx: 176,
                    sy: 14,
                },
            ]
        );
    }

    #[test]
    fn the_cold_furnace_blits_nothing() {
        assert_eq!(
            property_blits(WindowKind::Furnace, &[0, 200, 0, 200]),
            vec![]
        );
        assert_eq!(property_blits(WindowKind::Furnace, &[]), vec![]);
    }

    #[test]
    fn the_mid_brew_stand_blits_fill_and_bubbles() {
        // k = 200 → fill 14; (200 / 2) % 7 = 2 → frame 20.
        assert_eq!(
            property_blits(WindowKind::BrewingStand, &[200]),
            vec![
                SheetBlit {
                    dx: 97,
                    dy: 16,
                    w: 9,
                    h: 14,
                    sx: 176,
                    sy: 0,
                },
                SheetBlit {
                    dx: 65,
                    dy: 23,
                    w: 12,
                    h: 20,
                    sx: 185,
                    sy: 9,
                },
            ]
        );
    }

    #[test]
    fn the_done_brew_blits_nothing() {
        assert_eq!(property_blits(WindowKind::BrewingStand, &[0]), vec![]);
        assert_eq!(property_blits(WindowKind::BrewingStand, &[]), vec![]);
    }

    #[test]
    fn the_plain_kinds_blit_no_properties() {
        for kind in [
            WindowKind::Chest,
            WindowKind::Hopper,
            WindowKind::Dispenser,
            WindowKind::Dropper,
            WindowKind::CraftingTable,
            WindowKind::Unknown,
        ] {
            assert_eq!(
                property_blits(kind, &[100, 200, 100, 200]),
                vec![],
                "{kind:?} draws no property blits"
            );
        }
    }
}
