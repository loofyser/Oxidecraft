//! The player's own inventory screen: the 45-slot player container, the
//! pointer-facing player preview and the active-effect list.
//!
//! The port reads values and names from the source's player container
//! (`ContainerPlayer.java`), its inventory screen (`GuiInventory.java`: the
//! foreground label at :65-68, the preview call at :73-91, the
//! `drawEntityOnScreen` body at :96-134, the creative swap at :34-42), the
//! effects overlay (`client/renderer/InventoryEffectRenderer.java`: the frame
//! shift at :30-42, the rows at :60-109) and the open path
//! (`Minecraft.java`:2090-2103, send at :2100, display at :2101); no source
//! text is copied.
//!
//! The screen stands on window 0 through the same container path as every
//! other screen: its clicks carry window id 0 and its close sends C0D with
//! window id 0 like every other close. The layout reuses
//! [`ContainerScreen`](super::container::ContainerScreen) with the table
//! below, so slots, hover, drag, cursor and tooltips read the shared code.
//!
//! Recorded carries (not gaps in the pins):
//! - The preview is a GUI projection through the screen pass — the derived
//!   anchor/scale, the per-frame facing angles from the pointer and the own
//!   player's skin face sampled flat — not the entity pass: the source
//!   disables world lighting for the preview (`enableStandardItemLighting`
//!   plus the restore at :96-134), which the flat sample honours by
//!   construction. The posed 3D-model projection is future work; the facing
//!   math below carries the source's exact gains so it can drive one.
//! - The effect rows iterate ascending by id. The source iterates a `HashMap`
//!   (`EntityLivingBase.java`:60), which pins no order; the port's
//!   deterministic order (Task 5's store) is the recorded divergence.
//! - The source's `**:**` max-duration flag is unreachable: the port's
//!   [`StatusEffect`](oxide_game::session::StatusEffect) carries no
//!   max-duration field, so the duration always formats mm:ss.
//! - The armour ghost sprites draw nothing: the ghosts are atlas sprites with
//!   no item id, and no seam exists for non-item sprite draws. The per-slot
//!   ghost names and the validity rule below are pinned for it.
//! - The click path enforces no armour validity: the base reads every slot
//!   valid (recorded). [`armour_fits`] pins the source's rule for the seam
//!   that will.
//! - The riding branch of the open path is out of scope (recorded): the port
//!   carries no riding state, so every open sends the C16.
//! - The creative redirect is a carry (recorded): the client carries no
//!   gamemode state, so the E key always opens this screen. When the creative
//!   screen (Task 21) lands, the open path swaps there in creative mode per
//!   `GuiInventory.updateScreen`:34-42.

use oxide_game::input::InputEvent;

use super::container::{
    BackgroundKind, ContainerLayout, SlotBlock, SlotPos, TitleKind, TitleSource,
};

/// The background sheet, under the port's named-texture key
/// (`GuiInventory.java`:85-89 binds `inventoryBackground`).
pub const INVENTORY_SHEET: &str = "gui/container/inventory";

/// The preview's anchor, right and down from the frame origin
/// (`GuiInventory.java`:73-91 — `i + 51, j + 75`).
pub const PREVIEW_ANCHOR_DX: i32 = 51;
/// See [`PREVIEW_ANCHOR_DX`].
pub const PREVIEW_ANCHOR_DY: i32 = 75;
/// The preview's scale (`GuiInventory.java`:73-91 — the `30`).
pub const PREVIEW_SCALE: i32 = 30;
/// The pointer arm's vertical offset: the arm is measured from
/// (anchor.x, anchor.y − 50) (`(j + 75 - 50) - oldMouseY` at :73-91).
pub const PREVIEW_ARM_DY: i32 = 50;
/// The pointer-to-angle divisor: both arms divide by 40 (`mouseX / 40.0F`,
/// `mouseY / 40.0F` at :96-134).
pub const PREVIEW_POINTER_DIVISOR: f32 = 40.0;
/// The entity yaw's gain: `atan(mx/40) * 40` (`rotationYaw` at :96-134).
pub const PREVIEW_YAW_GAIN: f32 = 40.0;
/// The render-offset yaw's gain: `atan(mx/40) * 20` (`renderYawOffset`).
pub const PREVIEW_OFFSET_GAIN: f32 = 20.0;
/// The pitch gain's magnitude: `-atan(my/40) * 20` (`rotationPitch`).
pub const PREVIEW_PITCH_GAIN: f32 = 20.0;

/// The preview silhouette's size in GUI pixels: the skin face centred on the
/// anchor, so the anchor pixel samples the face.
pub const PREVIEW_SILHOUETTE: f32 = 16.0;

/// The effects frame's left shift while effects are non-empty
/// (`InventoryEffectRenderer.java`:30-42 —
/// `160 + (width - xSize - 200) / 2`).
pub const EFFECTS_SHIFT_BASE: i32 = 160;
/// See [`EFFECTS_SHIFT_BASE`]: the shift's own subtrahend.
pub const EFFECTS_SHIFT_SPAN: i32 = 200;
/// The rows' left edge, left of the frame (`guiLeft - 124` at :60-83).
pub const EFFECT_ROW_DX: i32 = -124;
/// The row sprite on the sheet (`(0, 166, 140, 32)` at :60-83).
pub const EFFECT_ROW_RECT: [i32; 4] = [0, 166, 140, 32];
/// The row step with five or fewer effects (`l = 33` at :60-83).
pub const EFFECT_ROW_STEP: i32 = 33;
/// The compressed step's own span: `132 / (size - 1)` past five rows.
pub const EFFECT_ROW_COMPRESSED_SPAN: i32 = 132;
/// The compressed step's own threshold: past five rows.
pub const EFFECT_ROW_COMPRESSED_AT: usize = 5;
/// The icon's cell, 18×18 at (+6, +7) of the row (`:85-89`).
pub const EFFECT_ICON_DX: i32 = 6;
/// See [`EFFECT_ICON_DX`].
pub const EFFECT_ICON_DY: i32 = 7;
/// See [`EFFECT_ICON_DX`]: the cell's side.
pub const EFFECT_ICON_SIZE: i32 = 18;
/// The icon sheet rows' own top (`198 + (idx/8) * 18` at :85-89).
pub const EFFECT_ICON_TOP: i32 = 198;
/// The name pen, right of the icon (`i + 10 + 18`, `j + 6` at :91-106).
pub const EFFECT_NAME_DX: i32 = 28;
/// See [`EFFECT_NAME_DX`].
pub const EFFECT_NAME_DY: i32 = 6;
/// The name's own packed white (16777215 at :91-106).
pub const EFFECT_NAME_COLOUR: u32 = 16_777_215;
/// The duration pen (`j + 6 + 10` at :107-108).
pub const EFFECT_DURATION_DY: i32 = 16;
/// The duration's own packed grey (8355711 = 0x7F7F7F at :107-108).
pub const EFFECT_DURATION_COLOUR: u32 = 8_355_711;

/// The result slot: `SlotCrafting` at (144, 36) (`ContainerPlayer.java`:26).
pub const RESULT_POS: [i32; 2] = [144, 36];
/// The 2×2 grid's own left (`88 + j * 18` at :28-34).
pub const GRID_LEFT: i32 = 88;
/// The 2×2 grid's own top (`26 + i * 18` at :28-34).
pub const GRID_TOP: i32 = 26;
/// The armour column's own left (`8` at :36-54).
pub const ARMOUR_LEFT: i32 = 8;
/// The armour column's own top (`8 + k * 18` at :36-54).
pub const ARMOUR_TOP: i32 = 8;
/// The main block's own top (`84 + l * 18` at :56-62).
pub const MAIN_TOP: i32 = 84;
/// The hotbar's own top (`142` at :64-67).
pub const HOTBAR_TOP: i32 = 142;
/// One cell's own step (18 throughout :26-67).
pub const SLOT_STEP: i32 = 18;
/// The screen's own slot count (0 result + 4 grid + 4 armour + 27 main + 9
/// hotbar = 45).
pub const SLOT_COUNT: usize = 45;

/// The ghost sprite an empty helmet slot shows (`EMPTY_SLOT_NAMES[0]`,
/// `Item.java`:807-810).
pub const GHOST_HELMET: &str = "empty_armor_slot_helmet";
/// The ghost sprite an empty chestplate slot shows (`EMPTY_SLOT_NAMES[1]`).
pub const GHOST_CHESTPLATE: &str = "empty_armor_slot_chestplate";
/// The ghost sprite an empty legging slot shows (`EMPTY_SLOT_NAMES[2]`).
pub const GHOST_LEGGINGS: &str = "empty_armor_slot_leggings";
/// The ghost sprite an empty boot slot shows (`EMPTY_SLOT_NAMES[3]`).
pub const GHOST_BOOTS: &str = "empty_armor_slot_boots";

/// The pumpkin's item id: the helmet slot's extra valid stack
/// (`isItemValid`'s `k == 0` arm at :36-54).
pub const PUMPKIN_ID: i16 = 86;
/// The skull's item id: the helmet slot's other extra valid stack.
pub const SKULL_ID: i16 = 397;
/// The armour registrations' own span: five materials of four pieces
/// (leather 298, chain 302, iron 306, diamond 310, gold 314 —
/// `armorType == k` with 0 = helmet per `ItemArmor.java`:24).
pub const ARMOUR_ID_BASES: [i16; 5] = [298, 302, 306, 310, 314];

/// One filler cell while the const table builds.
const FILLER: SlotPos = SlotPos {
    index: 0,
    x: 0,
    y: 0,
    block: SlotBlock::Container,
};

/// The 45-slot table in wire order (`ContainerPlayer.java`:26-67): the
/// result (144, 36), the 2×2 grid (`88 + j*18`, `26 + i*18`, index
/// `j + i*2`), the armour column (top to bottom helmet/chest/legs/boots at
/// (8, `8 + k*18`)), the main block (9 across × 3 at (`8 + 18j`,
/// 84/102/120)) and the hotbar ((`8 + 18i`, 142)).
const fn inventory_slots() -> [SlotPos; SLOT_COUNT] {
    let mut out = [FILLER; SLOT_COUNT];
    out[0] = SlotPos {
        index: 0,
        x: RESULT_POS[0],
        y: RESULT_POS[1],
        block: SlotBlock::Container,
    };
    let mut row: i32 = 0;
    while row < 2 {
        let mut col: i32 = 0;
        while col < 2 {
            out[(1 + col + row * 2) as usize] = SlotPos {
                index: 1 + (col + row * 2) as i16,
                x: GRID_LEFT + col * SLOT_STEP,
                y: GRID_TOP + row * SLOT_STEP,
                block: SlotBlock::Container,
            };
            col += 1;
        }
        row += 1;
    }
    let mut armour: i32 = 0;
    while armour < 4 {
        out[(5 + armour) as usize] = SlotPos {
            index: 5 + armour as i16,
            x: ARMOUR_LEFT,
            y: ARMOUR_TOP + armour * SLOT_STEP,
            block: SlotBlock::Player,
        };
        armour += 1;
    }
    let mut main: i32 = 0;
    while main < 27 {
        out[(9 + main) as usize] = SlotPos {
            index: 9 + main as i16,
            x: ARMOUR_LEFT + (main % 9) * SLOT_STEP,
            y: MAIN_TOP + (main / 9) * SLOT_STEP,
            block: SlotBlock::Player,
        };
        main += 1;
    }
    let mut hotbar: i32 = 0;
    while hotbar < 9 {
        out[(36 + hotbar) as usize] = SlotPos {
            index: 36 + hotbar as i16,
            x: ARMOUR_LEFT + hotbar * SLOT_STEP,
            y: HOTBAR_TOP,
            block: SlotBlock::Player,
        };
        hotbar += 1;
    }
    out
}

/// The table the layout lays out.
static INVENTORY_SLOTS: [SlotPos; SLOT_COUNT] = inventory_slots();

/// The inventory's layout: the default 176×166 panel on the inventory sheet,
/// the 45-slot table, the single crafting label at (86, 16) in 4210752
/// (`GuiInventory.java`:65-68 — the screen's ONLY title; no "Inventory"
/// line exists).
pub static INVENTORY_LAYOUT: ContainerLayout = ContainerLayout {
    x_size: 176,
    y_size: 166,
    sheet: INVENTORY_SHEET,
    slots: &INVENTORY_SLOTS,
    title: TitleKind::Inventory {
        label: TitleSource::Fixed("Crafting"),
    },
    background: BackgroundKind::Full,
};

/// The preview's anchor in screen units: the frame origin plus (51, 75).
pub fn preview_anchor(gx: i32, gy: i32) -> (i32, i32) {
    (gx + PREVIEW_ANCHOR_DX, gy + PREVIEW_ANCHOR_DY)
}

/// The preview's facing from the pointer, in degrees: the entity yaw, the
/// render-offset yaw and the pitch (`GuiInventory.java`:96-134 —
/// `atan(mx/40) * 40`, `atan(mx/40) * 20`, `-atan(my/40) * 20`, where `mx`
/// is the anchor's x minus the pointer and `my` the arm's y (anchor.y − 50)
/// minus the pointer). Pointer-relative every frame: no drag state, no step,
/// no clamp — `GuiInventory` defines no mouse handling at all, and
/// `GuiContainer.mouseClickMove` is the touchscreen drag-splitter.
pub fn preview_facing(
    pointer_x: f32,
    pointer_y: f32,
    anchor_x: f32,
    anchor_y: f32,
) -> (f32, f32, f32) {
    let arm_x = (anchor_x - pointer_x) / PREVIEW_POINTER_DIVISOR;
    let arm_y = ((anchor_y - PREVIEW_ARM_DY as f32) - pointer_y) / PREVIEW_POINTER_DIVISOR;
    (
        arm_x.atan() * PREVIEW_YAW_GAIN,
        arm_x.atan() * PREVIEW_OFFSET_GAIN,
        -arm_y.atan() * PREVIEW_PITCH_GAIN,
    )
}

/// The shifted frame's left while effects are non-empty
/// (`InventoryEffectRenderer.java`:30-42 —
/// `160 + (width - xSize - 200) / 2`).
pub fn effects_frame_left(width: i32, x_size: i32) -> i32 {
    EFFECTS_SHIFT_BASE + (width - x_size - EFFECTS_SHIFT_SPAN) / 2
}

/// The rows' step: 33, or `132 / (size - 1)` past five rows
/// (`InventoryEffectRenderer.java`:60-83).
pub fn effect_step(size: usize) -> i32 {
    if size > EFFECT_ROW_COMPRESSED_AT {
        EFFECT_ROW_COMPRESSED_SPAN / (size as i32 - 1)
    } else {
        EFFECT_ROW_STEP
    }
}

/// The status icon's sheet cell for an effect id: `(col, row)` with
/// `u = col * 18`, `v = 198 + row * 18` (`:85-89`,
/// `statusIconIndex = col + row * 8` in `Potion.java`:136-140). Ids 6, 7
/// and 23 carry no icon (`Potion.java`:26-74 states no `setIconIndex` for
/// them) — the row still draws its name and duration with no icon.
pub fn potion_icon_uv(effect_id: u8) -> Option<(i32, i32)> {
    match effect_id {
        1 => Some((0, 0)),
        2 => Some((1, 0)),
        3 => Some((2, 0)),
        4 => Some((3, 0)),
        5 => Some((4, 0)),
        8 => Some((2, 1)),
        9 => Some((3, 1)),
        10 => Some((7, 0)),
        11 => Some((6, 1)),
        12 => Some((7, 1)),
        13 => Some((0, 2)),
        14 => Some((0, 1)),
        15 => Some((5, 1)),
        16 => Some((4, 1)),
        17 => Some((1, 1)),
        18 => Some((5, 0)),
        19 => Some((6, 0)),
        20 => Some((1, 2)),
        21 | 22 => Some((2, 2)),
        _ => None,
    }
}

/// The effect name with the amplifier's numeral: numerals append ONLY at
/// amplifiers 1/2/3 (II/III/IV); 0 or ≥4 draws the bare name
/// (`InventoryEffectRenderer.java`:91-106).
pub fn effect_name(base: &str, amplifier: u8) -> String {
    match amplifier {
        1 => format!("{base} II"),
        2 => format!("{base} III"),
        3 => format!("{base} IV"),
        _ => String::from(base),
    }
}

/// The duration text: `Potion.getDurationString` without the unreachable
/// max-duration flag — `ticksToElapsedTime`: `s = ticks/20`, `m = s/60`,
/// `s %= 60`, `m:ss` with the seconds zero-padded only
/// (`StringUtils.java`:12-18). Minutes stay unpadded past 59.
pub fn effect_duration(ticks: i32) -> String {
    let seconds = ticks / 20;
    let minutes = seconds / 60;
    let rest = seconds % 60;
    if rest < 10 {
        format!("{minutes}:0{rest}")
    } else {
        format!("{minutes}:{rest}")
    }
}

/// The ghost sprite an empty armour slot shows, by wire index: the
/// `EMPTY_SLOT_NAMES[k]` ghosts (`ContainerPlayer.java`:36-54). Any other
/// slot shows none.
pub fn armour_ghost(slot: i16) -> Option<&'static str> {
    match slot {
        5 => Some(GHOST_HELMET),
        6 => Some(GHOST_CHESTPLATE),
        7 => Some(GHOST_LEGGINGS),
        8 => Some(GHOST_BOOTS),
        _ => None,
    }
}

/// Whether the stack fits the armour slot: valid iff its piece's
/// `armorType == k` — plus the pumpkin and the skull iff `k == 0`
/// (`ContainerPlayer.java`:36-54 with `ItemArmor.java`:24). The five
/// materials register helmet/chest/legs/boots in order (leather 298, chain
/// 302, iron 306, diamond 310, gold 314).
pub fn armour_fits(slot: i16, item_id: i16) -> bool {
    if !(5..=8).contains(&slot) {
        return false;
    }
    let want = slot - 5;
    if (item_id == PUMPKIN_ID || item_id == SKULL_ID) && want == 0 {
        return true;
    }
    ARMOUR_ID_BASES
        .iter()
        .any(|base| (0..4).any(|piece| item_id == base + piece && want == piece))
}

/// One E-key open's sends: exactly one [`InputEvent::OpenInventory`] — the
/// C16 client status 2 (`Minecraft.java`:2090-2103, send at :2100). No
/// guard exists, so two opens send two (the caller opens the screen beside
/// it, at :2101).
pub fn open_sends() -> Vec<InputEvent> {
    vec![InputEvent::OpenInventory]
}

#[cfg(test)]
mod tests {
    //! The inventory screen's pins: the slot table, the preview's facing
    //! gains and anchor/scale, the effects overlay's order and format
    //! literals, and the one-C16-per-open rule.

    use super::*;

    #[test]
    fn the_table_holds_all_forty_five_slots() {
        assert_eq!(INVENTORY_SLOTS.len(), SLOT_COUNT);
        assert_eq!(INVENTORY_LAYOUT.x_size, 176);
        assert_eq!(INVENTORY_LAYOUT.y_size, 166);
        assert_eq!(INVENTORY_LAYOUT.sheet, "gui/container/inventory");
    }

    #[test]
    fn the_result_and_grid_come_from_the_player_container() {
        // Slot 0 = SlotCrafting result at (144, 36); slots 1-4 the 2x2 grid
        // (88/106 x 26/44, index j + i*2).
        assert_eq!(INVENTORY_SLOTS[0], slot(0, 144, 36));
        assert_eq!(INVENTORY_SLOTS[1], slot(1, 88, 26));
        assert_eq!(INVENTORY_SLOTS[2], slot(2, 106, 26));
        assert_eq!(INVENTORY_SLOTS[3], slot(3, 88, 44));
        assert_eq!(INVENTORY_SLOTS[4], slot(4, 106, 44));
    }

    #[test]
    fn the_armour_column_runs_helmet_to_boots() {
        // Slots 5-8 top to bottom at (8, 8/26/44/62).
        assert_eq!(INVENTORY_SLOTS[5], player_slot(5, 8, 8));
        assert_eq!(INVENTORY_SLOTS[6], player_slot(6, 8, 26));
        assert_eq!(INVENTORY_SLOTS[7], player_slot(7, 8, 44));
        assert_eq!(INVENTORY_SLOTS[8], player_slot(8, 8, 62));
    }

    #[test]
    fn the_main_and_hotbar_blocks_match_the_player_sections() {
        // Slots 9-35: 9 across x 3 at (8+18j, 84/102/120); slots 36-44 the
        // hotbar at (8+18i, 142).
        assert_eq!(INVENTORY_SLOTS[9], player_slot(9, 8, 84));
        assert_eq!(INVENTORY_SLOTS[17], player_slot(17, 152, 84));
        assert_eq!(INVENTORY_SLOTS[18], player_slot(18, 8, 102));
        assert_eq!(INVENTORY_SLOTS[35], player_slot(35, 152, 120));
        assert_eq!(INVENTORY_SLOTS[36], player_slot(36, 8, 142));
        assert_eq!(INVENTORY_SLOTS[44], player_slot(44, 152, 142));
    }

    #[test]
    fn the_preview_anchor_and_scale_come_from_the_call_site() {
        // (i+51, j+75), scale 30; the pointer arm measured from
        // (anchor.x, anchor.y-50).
        assert_eq!(preview_anchor(100, 40), (151, 115));
        assert_eq!(PREVIEW_SCALE, 30);
        assert_eq!(PREVIEW_ARM_DY, 50);
    }

    #[test]
    fn the_preview_faces_the_pointer_with_the_source_gains() {
        // atan(pointer/40) x40 yaw / x20 offset / x20 pitch — no step, no
        // clamp: a dead-centre pointer faces forward, a 40px arm turns
        // atan(1) x gain, and a 400px arm still follows atan (unclamped).
        let (yaw, offset, pitch) = preview_facing(151.0, 65.0, 151.0, 115.0);
        assert_eq!((yaw, offset, pitch), (0.0, 0.0, -0.0));
        let (yaw, offset, pitch) = preview_facing(111.0, 25.0, 151.0, 115.0);
        let arm = 1.0_f32.atan();
        assert!((yaw - arm * 40.0).abs() < 1e-4, "yaw {yaw}");
        assert!((offset - arm * 20.0).abs() < 1e-4, "offset {offset}");
        assert!((pitch + arm * 20.0).abs() < 1e-4, "pitch {pitch}");
        let (far_yaw, _, _) = preview_facing(-249.0, 65.0, 151.0, 115.0);
        let far = 10.0_f32.atan() * 40.0;
        assert!((far_yaw - far).abs() < 1e-3, "unclamped {far_yaw} vs {far}");
        assert!(far_yaw < 90.0, "atan never reaches a clamp {far_yaw}");
    }

    #[test]
    fn the_effects_frame_shifts_while_non_empty() {
        // 160 + (width - xSize - 200)/2: at 448 wide the frame sits at 196,
        // sixty right of the centred 136.
        assert_eq!(effects_frame_left(448, 176), 196);
    }

    #[test]
    fn the_effect_step_compresses_past_five_rows() {
        assert_eq!(effect_step(1), 33);
        assert_eq!(effect_step(5), 33);
        assert_eq!(effect_step(6), 26);
        assert_eq!(effect_step(10), 14);
    }

    #[test]
    fn the_icon_table_maps_ids_to_sheet_cells() {
        assert_eq!(potion_icon_uv(1), Some((0, 0)));
        assert_eq!(potion_icon_uv(5), Some((4, 0)));
        assert_eq!(potion_icon_uv(10), Some((7, 0)));
        assert_eq!(potion_icon_uv(13), Some((0, 2)));
        assert_eq!(potion_icon_uv(21), Some((2, 2)));
        assert_eq!(potion_icon_uv(22), Some((2, 2)));
        // Ids 6, 7 and 23 carry no icon — the row still draws text-only.
        assert_eq!(potion_icon_uv(6), None);
        assert_eq!(potion_icon_uv(7), None);
        assert_eq!(potion_icon_uv(23), None);
        assert_eq!(potion_icon_uv(0), None);
        assert_eq!(potion_icon_uv(24), None);
    }

    #[test]
    fn the_name_numerals_cap_at_four() {
        assert_eq!(effect_name("Speed", 0), "Speed");
        assert_eq!(effect_name("Speed", 1), "Speed II");
        assert_eq!(effect_name("Strength", 2), "Strength III");
        assert_eq!(effect_name("Haste", 3), "Haste IV");
        assert_eq!(effect_name("Speed", 4), "Speed");
    }

    #[test]
    fn the_duration_formats_minutes_unpadded() {
        assert_eq!(effect_duration(0), "0:00");
        assert_eq!(effect_duration(20), "0:01");
        assert_eq!(effect_duration(199), "0:09");
        assert_eq!(effect_duration(200), "0:10");
        assert_eq!(effect_duration(1200), "1:00");
        assert_eq!(effect_duration(3600), "3:00");
        assert_eq!(effect_duration(78000), "65:00");
    }

    #[test]
    fn the_ghosts_pair_with_the_armour_slots() {
        assert_eq!(armour_ghost(5), Some("empty_armor_slot_helmet"));
        assert_eq!(armour_ghost(6), Some("empty_armor_slot_chestplate"));
        assert_eq!(armour_ghost(7), Some("empty_armor_slot_leggings"));
        assert_eq!(armour_ghost(8), Some("empty_armor_slot_boots"));
        assert_eq!(armour_ghost(0), None);
        assert_eq!(armour_ghost(9), None);
    }

    #[test]
    fn the_armour_validity_follows_the_piece_type() {
        // armorType == k: diamond helmet (310) fits slot 5 only, chestplate
        // (311) slot 6, leggings (312) slot 7, boots (313) slot 8.
        assert!(armour_fits(5, 310));
        assert!(!armour_fits(6, 310));
        assert!(armour_fits(6, 311));
        assert!(armour_fits(7, 312));
        assert!(armour_fits(8, 313));
        // Leather (298) and gold boots (317) read the same rule.
        assert!(armour_fits(5, 298));
        assert!(armour_fits(8, 317));
        assert!(!armour_fits(5, 317));
        // The pumpkin and the skull fit the helmet slot only.
        assert!(armour_fits(5, 86));
        assert!(armour_fits(5, 397));
        assert!(!armour_fits(8, 86));
        // A stick fits nowhere, and no armour fits outside 5-8.
        assert!(!armour_fits(5, 280));
        assert!(!armour_fits(9, 310));
        assert!(!armour_fits(0, 310));
    }

    #[test]
    fn one_open_sends_one_c16() {
        assert_eq!(open_sends(), vec![InputEvent::OpenInventory]);
    }

    /// One craft-side slot's pinned shape.
    fn slot(index: i16, x: i32, y: i32) -> SlotPos {
        SlotPos {
            index,
            x,
            y,
            block: SlotBlock::Container,
        }
    }

    /// One player-side slot's pinned shape.
    fn player_slot(index: i16, x: i32, y: i32) -> SlotPos {
        SlotPos {
            index,
            x,
            y,
            block: SlotBlock::Player,
        }
    }
}
