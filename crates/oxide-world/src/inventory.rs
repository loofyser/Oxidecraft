//! The player inventory: the 36 main slots (hotbar first), the 4 armour
//! slots, the cursor stack and the selected hotbar slot, with the window-0
//! slot mapping the containers project through.
//!
//! The shape is `InventoryPlayer`'s own (`entity/player/InventoryPlayer.java`):
//! `mainInventory` is the 36-slot array whose first nine entries are the
//! visible bar (`:21-24`), `armorInventory` the four worn pieces (`:27`),
//! `currentItem` the selected hotbar index (`:30`) and `itemStack` the stack
//! held on the cursor (`:34`).
//!
//! # The window-0 mapping
//!
//! `ContainerPlayer` registers its slots in a fixed order
//! (`inventory/ContainerPlayer.java:26-67`): the crafting result, the four
//! matrix slots, the four armour slots, the 27 main slots, then the nine
//! hotbar slots. A `Slot` indexes its inventory through
//! `InventoryPlayer.getStackInSlot` (`InventoryPlayer.java:639-649`), which
//! shifts indices at and beyond 36 into the armour array, and the armour loop
//! registers `getSizeInventory() - 1 - k` (`ContainerPlayer.java:36-54`),
//! where `getSizeInventory()` is 36 + 4 (`InventoryPlayer.java:631-634`).
//! Together they fix both directions:
//!
//! * window 5–8 map to `armor[3..0]`, descending: window 5 is the helmet
//!   (`armor[3]`) and window 8 the boots (`armor[0]`);
//! * window 9–35 map to `main[9..36]`, same order;
//! * window 36–44 map to `main[0..8]`, the hotbar, same order.
//!
//! Window slots 0–4 are the crafting result and the 2×2 matrix: they are not
//! inventory state, so the mapping refuses them and the session keeps them
//! beside the model. The wire's slot index is an `i16` and the model indexes
//! with `usize`; a negative index or one past 44 maps nothing, so
//! [`Inventory::set_window_slot`] returns `false` and
//! [`Inventory::window_slot`] returns `None` — neither panics (spec S2).

use oxide_proto_v47::entity::MetadataItem;

/// The empty slot a range-guarded read returns: [`Inventory::get_current_item`]
/// hands out a reference, and the source's own guard reads the empty stack
/// outside the hotbar (`InventoryPlayer.getCurrentItem:50-53`).
static EMPTY_SLOT: Option<MetadataItem> = None;

/// The player's inventory: the 36 main slots (hotbar first), the 4 armour
/// slots, the cursor stack and the selected hotbar slot.
///
/// The field names and the layout are the source's own
/// (`InventoryPlayer.java:21-34`). `armor[0]` is the boots and `armor[3]` the
/// helmet; the armour band's descent in the window layout comes from the
/// registration, not from the array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inventory {
    /// The 36 main slots: `main[0..9]` is the hotbar and `main[9..36]` the 27
    /// main slots (`InventoryPlayer.mainInventory:21-24`).
    pub main: [Option<MetadataItem>; 36],
    /// The 4 armour slots, boots-first (`InventoryPlayer.armorInventory:27`).
    pub armor: [Option<MetadataItem>; 4],
    /// The stack held on the cursor — window id −1 on the wire
    /// (`InventoryPlayer.itemStack:34`).
    pub cursor: Option<MetadataItem>,
    /// The selected hotbar index, 0–8 (`InventoryPlayer.currentItem:30`).
    pub selected: i16,
}

impl Default for Inventory {
    /// The fresh inventory: every slot empty, no cursor, the first hotbar
    /// slot selected.
    fn default() -> Self {
        Self {
            main: std::array::from_fn(|_| None),
            armor: std::array::from_fn(|_| None),
            cursor: None,
            selected: 0,
        }
    }
}

impl Inventory {
    /// Maps one window-0 slot index to the model and writes `item` there.
    ///
    /// `window_id_0_slot` is the slot index as window 0's packets carry it, an
    /// `i16`. The bands are the registration order's
    /// (`ContainerPlayer.java:36-67`): 5–8 are the armour band, descending
    /// from the helmet at 5 to the boots at 8; 9–35 are the main slots in
    /// their own order; 36–44 are the hotbar. Returns whether the index
    /// mapped. The crafting slots 0–4 and anything outside 5–44 are not
    /// inventory state: they return `false` without touching the model.
    pub fn set_window_slot(&mut self, window_id_0_slot: i16, item: Option<MetadataItem>) -> bool {
        match window_id_0_slot {
            5..=8 => {
                self.armor[(8 - window_id_0_slot) as usize] = item;
                true
            }
            9..=35 => {
                self.main[window_id_0_slot as usize] = item;
                true
            }
            36..=44 => {
                self.main[(window_id_0_slot - 36) as usize] = item;
                true
            }
            _ => false,
        }
    }

    /// The projection read: one window-0 slot index to the model's stack, or
    /// `None` when the index is not inventory state (the crafting slots 0–4
    /// and anything past 44).
    pub fn window_slot(&self, window_index: usize) -> Option<&Option<MetadataItem>> {
        match window_index {
            5..=8 => Some(&self.armor[8 - window_index]),
            9..=35 => Some(&self.main[window_index]),
            36..=44 => Some(&self.main[window_index - 36]),
            _ => None,
        }
    }

    /// The item stack currently held by the player, from the selected hotbar
    /// slot — `InventoryPlayer.getCurrentItem` (`:50-53`) including its
    /// guard: outside 0–8 the held item is the empty stack.
    pub fn get_current_item(&self) -> &Option<MetadataItem> {
        if (0..9).contains(&self.selected) {
            &self.main[self.selected as usize]
        } else {
            &EMPTY_SLOT
        }
    }

    /// Selects the hotbar slot, stored verbatim: the source's `currentItem` is
    /// a plain field (`InventoryPlayer.currentItem:30`) and the range guard
    /// lives in the read (`:52`), which this model mirrors in
    /// [`Self::get_current_item`].
    pub fn set_selected(&mut self, slot: i16) {
        self.selected = slot;
    }

    /// Steps the selected hotbar slot one position: the source's
    /// `changeCurrentItem` (`InventoryPlayer.java:165-186`) clamps the
    /// direction's magnitude to one step, subtracts it from the selection
    /// (`+1` steps back, `−1` forward, `:162-163`) and wraps the result into
    /// 0–8.
    ///
    /// Returns whether the selection moved: a zero direction has no effect
    /// (`:163`), though the wrap still normalises a selection that was
    /// already outside 0–8, exactly as the source's loops do.
    pub fn change_current_item(&mut self, direction: i32) -> bool {
        let step = direction.signum();
        self.selected = (i32::from(self.selected) - step).rem_euclid(9) as i16;
        step != 0
    }

    /// The respawn edge's immediate clear: takes the cursor stack out and
    /// returns it so the caller can record it, leaving the main and armour
    /// arrays alone.
    ///
    /// The source's Respawned edge replaces the client player outright
    /// (`NetHandlerPlayClient.handleRespawn:1056-1073` →
    /// `Minecraft.setDimensionAndSpawnPlayer:2430-2459` →
    /// `PlayerControllerMP.func_178892_a:487-490` builds a new
    /// `EntityPlayerSP`, whose `EntityPlayer.inventory:82` is a fresh
    /// `InventoryPlayer`), so the cursor — `InventoryPlayer.itemStack:34`,
    /// which the server only touches through window id −1 — is dropped, not
    /// carried; the server's window-0 resend refills the arrays.
    /// `EntityPlayerSP.closeScreenAndDropStack:336-341` drops the cursor the
    /// same way at a screen edge.
    pub fn clear_for_respawn(&mut self) -> Option<MetadataItem> {
        self.cursor.take()
    }
}

#[cfg(test)]
mod tests {
    //! The model's own pins: the fresh state and the wrap literals. The full
    //! mapping suite lives in `tests/inventory.rs`.

    use super::Inventory;
    use oxide_proto_v47::entity::MetadataItem;

    /// One stack from a literal id.
    fn item(id: i16) -> Option<MetadataItem> {
        Some(MetadataItem {
            id,
            count: 1,
            damage: 0,
            nbt: None,
        })
    }

    #[test]
    fn a_fresh_inventory_holds_nothing_and_selects_zero() {
        let inventory = Inventory::default();
        assert!(inventory.main.iter().all(Option::is_none));
        assert!(inventory.armor.iter().all(Option::is_none));
        assert!(inventory.cursor.is_none());
        assert_eq!(inventory.selected, 0);
        assert!(inventory.get_current_item().is_none());
    }

    #[test]
    fn change_current_item_wraps_at_both_ends() {
        let mut inventory = Inventory::default();
        inventory.main[0] = item(5);
        inventory.main[8] = item(7);
        inventory.set_selected(0);
        assert!(inventory.change_current_item(1));
        assert_eq!(inventory.selected, 8);
        assert_eq!(inventory.get_current_item(), &item(7));
        assert!(inventory.change_current_item(-1));
        assert_eq!(inventory.selected, 0);
        assert_eq!(inventory.get_current_item(), &item(5));
        // A zero direction does not move the selection.
        assert!(!inventory.change_current_item(0));
        assert_eq!(inventory.selected, 0);
    }
}
