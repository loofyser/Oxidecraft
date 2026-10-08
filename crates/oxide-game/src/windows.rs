//! The session's window state: window 0 — the player's own, held from Join
//! Game — and the single window a server may open, with the wire's apply rules
//! for both.
//!
//! The shape is the client's own container state (`NetHandlerPlayClient`'s
//! `entityplayer.inventoryContainer` and `entityplayer.openContainer`, and the
//! `handle_*` paths from `handleSetSlot:1133` through `handleCloseWindow:1311`):
//! the window the server opened, with its slots, properties and entity id; the
//! player's own 45-slot window, whose crafting result and matrix sit beside the
//! [`Inventory`] model; the cursor stack the wire addresses as window id −1;
//! and the action number the click path counts up (`Container.getNextTransactionID`,
//! `Container.java:561-565`).
//!
//! Every apply function is a port of one `handle_*` path, and every one of them
//! treats the wire as hostile (spec S2): a window id the state does not hold, a
//! slot index a window cannot carry, and a property index outside
//! [`MAX_WINDOW_PROPERTIES`] all land in [`Windows::ignored`] — the counter the
//! tests read — rather than in a panic or in an allocation sized by the wire.

use oxide_proto_v47::entity::MetadataItem;
use oxide_proto_v47::window::WindowKind;
use oxide_world::inventory::Inventory;

/// The hotbar's width: `InventoryPlayer.getHotbarSize`'s own nine
/// (`InventoryPlayer.java:58-61`).
pub const HOTBAR_SIZE: usize = 9;

/// Window 0's slot count: the crafting result and the 2×2 matrix at 0–4, the
/// armour band at 5–8, the main slots at 9–35 and the hotbar at 36–44 — the
/// registration order `ContainerPlayer` writes (`ContainerPlayer.java:26-67`).
pub const WINDOW0_SLOTS: usize = 45;

/// The window id the cursor stack rides on the wire (`handleSetSlot`'s −1
/// branch, `NetHandlerPlayClient.java:1139-1141`).
pub const CURSOR_WINDOW_ID: i8 = -1;

/// The largest property index a window may carry, one past the last index any
/// 1.8.9 container writes: the enchanting table's seven updates are 0–6
/// (`ContainerEnchantment.java:110-118`) and every other container writes
/// fewer (a furnace's four, `ContainerFurnace.java:75-80`). A write past it is
/// refused, so a hostile index cannot size the vector.
pub const MAX_WINDOW_PROPERTIES: usize = 7;

/// The pop a hotbar stack takes when a write grew it: the source's
/// `animationsToGo = 5` (`NetHandlerPlayClient.java:1158`), read by the HUD's
/// pop math (`GuiIngame.java:1043-1053`).
pub const HOTBAR_POP_TICKS: u8 = 5;

/// Window 0's crafting slots: the result and the 2×2 matrix at 0–4
/// (`ContainerPlayer`'s first five registrations, `ContainerPlayer.java:26-35`).
const CRAFTING_SLOTS: usize = 5;

/// The window-0 index the hotbar band starts at: the crafting five, the armour
/// four and the main twenty-seven ahead of it (`ContainerPlayer.java:36-67`).
const HOTBAR_BAND_START: i16 = 36;

/// The one window a server has opened, with the state the wire has written.
///
/// The field set is the plan's own (Decision 4): the slots the server declared
/// at open — sized to the packet's slot count, the way the source's
/// `ContainerLocalMenu`/`InventoryBasic` are built (`NetHandlerPlayClient.handleOpenWindow:1092-1131`)
/// — its properties, and the entity id only a horse window carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenWindowState {
    /// The window's id, 1–100 as the server assigns it.
    pub window_id: u8,
    /// The kind the type string named.
    pub kind: WindowKind,
    /// The title as sent — chat JSON.
    pub title: String,
    /// The window's slots, in its own layout; `None` is an empty slot.
    pub slots: Vec<Option<MetadataItem>>,
    /// The window's properties, resized as writes arrive.
    pub properties: Vec<i16>,
    /// The entity id a horse window carries; `None` for every other kind.
    pub entity_id: Option<i32>,
}

/// The player's own window 0: the crafting slots the model does not hold, and
/// the inventory it does.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Window0 {
    /// The crafting result and the 2×2 matrix, window slots 0–4
    /// (`ContainerPlayer`'s first five registrations, `ContainerPlayer.java:26-35`).
    pub crafting: [Option<MetadataItem>; 5],
    /// The player's inventory: the main slots, the armour and the cursor.
    pub inventory: Inventory,
}

/// The session's whole window state.
///
/// Window 0 exists for the whole session (the server never closes it — it is
/// the player's own container), the server opens at most one other window at a
/// time, and the cursor rides the wire as window −1.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Windows {
    /// The window the server opened, or `None`.
    pub open: Option<OpenWindowState>,
    /// The player's own window 0.
    pub player: Window0,
    /// The carried stack, window −1's target. The model's own copy
    /// (`Inventory::cursor`, `InventoryPlayer.itemStack:34`) is written with
    /// it, so the two never disagree.
    pub cursor: Option<MetadataItem>,
    /// The action number the click path counts up, as `Container.transactionID`
    /// (`Container.java:561-565`): the first transaction is 1 and the counter
    /// wraps through `i16`'s range.
    pub action_number: i16,
    /// The hotbar's pop counters, one per hotbar slot: the source's
    /// `animationsToGo` per stack, which only the hotbar's own writes set
    /// (`NetHandlerPlayClient.java:1152-1161`) and the tick decrements
    /// (`InventoryPlayer.decrementAnimations:352-362`).
    pub hotbar_pop: [u8; HOTBAR_SIZE],
    /// How many writes the state ignored: a window id it does not hold, a slot
    /// index a window cannot carry, a property index outside the cap, a close
    /// that named nothing. Hostile input is counted here, never fatal.
    pub ignored: u64,
}

/// One window's whole state, as the session's `WindowSnapshot` event carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSnapshot {
    /// The window the snapshot belongs to.
    pub window_id: u8,
    /// Every slot in the window's own layout: window 0's 45-slot projection,
    /// or the open window's own vector.
    pub slots: Vec<Option<MetadataItem>>,
    /// The carried stack.
    pub cursor: Option<MetadataItem>,
    /// The window's properties.
    pub properties: Vec<i16>,
    /// The hotbar's pop counters.
    pub hotbar_pop: [u8; HOTBAR_SIZE],
}

impl Windows {
    /// The fresh state: window 0 empty, nothing open, the action number at
    /// zero — the field's own default (`Container.transactionID` starts at 0,
    /// `Container.java:561-565`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies one Set Slot (0x2F).
    ///
    /// The source's own branches (`NetHandlerPlayClient.handleSetSlot:1133-1168`):
    /// window −1 writes the cursor (`:1139-1141`); window 0 writes its own
    /// layout, with the hotbar's pop set when the write grew the stack
    /// (`:1152-1161`); the live open window takes the write into its vector
    /// (`:1163-1166`). Anything else — an id the state does not hold, a slot
    /// index past the window's layout — is ignored and counted. Returns
    /// whether the write landed.
    pub fn apply_set_slot(&mut self, window_id: i8, slot: i16, item: Option<MetadataItem>) -> bool {
        if window_id == CURSOR_WINDOW_ID {
            self.set_cursor(item);
            return true;
        }
        if window_id == 0 {
            // The hotbar's pop, read against the stack the slot holds now
            // (`NetHandlerPlayClient.java:1152-1161`): the incoming stack's own
            // field is zero unless the write grew the stack, so a write that
            // did not grow clears the counter with the stack it replaces.
            if (HOTBAR_BAND_START..HOTBAR_BAND_START + HOTBAR_SIZE as i16).contains(&slot) {
                let grew = match (&item, self.player.inventory.window_slot(slot as usize)) {
                    (Some(incoming), Some(old)) => {
                        old.as_ref().is_none_or(|old| old.count < incoming.count)
                    }
                    _ => false,
                };
                self.hotbar_pop[slot as usize - HOTBAR_BAND_START as usize] =
                    if grew { HOTBAR_POP_TICKS } else { 0 };
            }
            let landed = self.write_window0_slot(slot, item);
            if !landed {
                self.ignored += 1;
            }
            return landed;
        }
        // The live open window: the source's own id test, the wire's byte
        // against the window's id (`handleSetSlot:1163-1166`).
        let Some(open) = self
            .open
            .as_mut()
            .filter(|open| i16::from(open.window_id) == i16::from(window_id))
        else {
            self.ignored += 1;
            return false;
        };
        let landed = match usize::try_from(slot)
            .ok()
            .and_then(|index| open.slots.get_mut(index))
        {
            Some(target) => {
                *target = item;
                true
            }
            None => false,
        };
        if !landed {
            self.ignored += 1;
        }
        landed
    }

    /// Writes one window-0 slot: the crafting slots at 0–4 into `crafting`, the
    /// rest through the inventory's own mapping
    /// ([`Inventory::set_window_slot`]). Returns whether the index is window-0
    /// state at all.
    fn write_window0_slot(&mut self, slot: i16, item: Option<MetadataItem>) -> bool {
        if (0..CRAFTING_SLOTS as i16).contains(&slot) {
            self.player.crafting[slot as usize] = item;
            return true;
        }
        self.player.inventory.set_window_slot(slot, item)
    }

    /// Writes the carried stack, in both of its stores: the state's own field
    /// and the model's copy (`InventoryPlayer.setItemStack:788-791` behind the
    /// wire's window −1, `NetHandlerPlayClient.java:1139-1141`), so the two
    /// never disagree.
    fn set_cursor(&mut self, item: Option<MetadataItem>) {
        self.player.inventory.cursor = item.clone();
        self.cursor = item;
    }

    /// Applies one Window Items (0x30).
    ///
    /// The source writes the packet's stacks over the container's slots, index
    /// by index (`handleWindowItems:1198-1211` reaching
    /// `Container.putStacksInSlots:546-552`): window 0's indices map through its
    /// own layout, the open window's land in its vector, and an index a window
    /// cannot carry — or a window the state does not hold — is ignored and
    /// counted. Returns whether a slot landed.
    pub fn apply_window_items(&mut self, window_id: u8, slots: Vec<Option<MetadataItem>>) -> bool {
        if window_id == 0 {
            let mut landed = false;
            for (index, item) in slots.into_iter().enumerate() {
                let Ok(slot) = i16::try_from(index) else {
                    self.ignored += 1;
                    continue;
                };
                if self.write_window0_slot(slot, item) {
                    landed = true;
                    if (HOTBAR_BAND_START..HOTBAR_BAND_START + HOTBAR_SIZE as i16).contains(&slot) {
                        // The packet's own stacks carry no pop (`ItemStack`'s
                        // field starts at zero), so the whole-set write clears
                        // the counter with the stack it replaces.
                        self.hotbar_pop[slot as usize - HOTBAR_BAND_START as usize] = 0;
                    }
                } else {
                    self.ignored += 1;
                }
            }
            return landed;
        }
        let Some(open) = self
            .open
            .as_mut()
            .filter(|open| open.window_id == window_id)
        else {
            self.ignored += 1;
            return false;
        };
        let mut landed = false;
        for (index, item) in slots.into_iter().enumerate() {
            match open.slots.get_mut(index) {
                Some(target) => {
                    *target = item;
                    landed = true;
                }
                None => self.ignored += 1,
            }
        }
        landed
    }

    /// Applies one Window Property (0x31) to the open window.
    ///
    /// The source routes the write to the open container's `updateProgressBar`
    /// when the id matches and drops it otherwise
    /// (`handleWindowProperty:1286-1294`); window 0's container has no
    /// properties at all (`Container.updateProgressBar:554-556` is empty). The
    /// vector grows to hold the index, up to [`MAX_WINDOW_PROPERTIES`]; a write
    /// that lands nowhere is ignored and counted. Returns whether the write
    /// landed.
    pub fn apply_property(&mut self, window_id: u8, property: i16, value: i16) -> bool {
        let Some(index) = usize::try_from(property)
            .ok()
            .filter(|index| *index < MAX_WINDOW_PROPERTIES)
        else {
            self.ignored += 1;
            return false;
        };
        let Some(open) = self
            .open
            .as_mut()
            .filter(|open| open.window_id == window_id)
        else {
            self.ignored += 1;
            return false;
        };
        if open.properties.len() <= index {
            open.properties.resize(index + 1, 0);
        }
        open.properties[index] = value;
        true
    }

    /// Applies an Open Window (0x2D): the server's window replaces whatever
    /// stood, the server opening at most one at a time.
    ///
    /// The source builds a fresh container for every open
    /// (`handleOpenWindow:1092-1131`), so the slots, properties and entity id
    /// come from the packet and nothing of the old window survives.
    pub fn apply_open(&mut self, open: OpenWindowState) {
        self.open = Some(open);
    }

    /// Applies a Close Window (0x2E): the window it names leaves the state and
    /// the carried stack drops with it.
    ///
    /// The source closes the current screen whatever id the packet carries and
    /// drops the cursor with it (`handleCloseWindow:1311-1315` reaching
    /// `EntityPlayerSP.closeScreenAndDropStack:336-341`). This port keeps the
    /// id policed: only a close naming the live open window changes anything —
    /// window 0 is the player's own and never leaves — and a close that lands
    /// nowhere is ignored and counted. Returns whether a window was closed.
    pub fn apply_close(&mut self, window_id: u8) -> bool {
        if window_id == 0
            || !self
                .open
                .as_ref()
                .is_some_and(|open| open.window_id == window_id)
        {
            self.ignored += 1;
            return false;
        }
        self.open = None;
        self.set_cursor(None);
        true
    }

    /// The next action number: the counter advances first, so the first
    /// transaction is 1, and it wraps through `i16`'s range the way the
    /// source's Java `short` does (`Container.getNextTransactionID:561-565`).
    pub fn next_action_number(&mut self) -> i16 {
        self.action_number = self.action_number.wrapping_add(1);
        self.action_number
    }

    /// Whether a window id names a container the state holds: window 0, which
    /// is always held, or the live open window. The confirm loop's own guard
    /// (`NetHandlerPlayClient.handleConfirmTransaction:1176-1195`).
    pub fn holds_window(&self, window_id: i8) -> bool {
        if window_id == 0 {
            return true;
        }
        self.open
            .as_ref()
            .is_some_and(|open| i16::from(open.window_id) == i16::from(window_id))
    }

    /// Advances the hotbar's pop counters one tick.
    ///
    /// The source's living update decrements every main-inventory stack's
    /// `animationsToGo` once per tick (`EntityPlayer.java:617` reaching
    /// `InventoryPlayer.decrementAnimations:352-362` and
    /// `ItemStack.updateAnimation:486-491`). Returns whether a counter moved,
    /// which is what a tick republishes window 0's snapshot for.
    pub fn tick_hotbar_pop(&mut self) -> bool {
        let mut moved = false;
        for pop in &mut self.hotbar_pop {
            if *pop > 0 {
                *pop -= 1;
                moved = true;
            }
        }
        moved
    }

    /// Window 0's slots in the wire's own 45-slot layout: the crafting result
    /// and the matrix at 0–4, then the armour, main and hotbar bands through
    /// the inventory's own mapping (`Inventory::window_slot`).
    pub fn window0_slots(&self) -> Vec<Option<MetadataItem>> {
        let mut slots = Vec::with_capacity(WINDOW0_SLOTS);
        slots.extend(self.player.crafting.iter().cloned());
        for index in CRAFTING_SLOTS..WINDOW0_SLOTS {
            slots.push(self.player.inventory.window_slot(index).cloned().flatten());
        }
        slots
    }

    /// The snapshot of one window, or `None` when the state does not hold it:
    /// window 0 projects the crafting slots and the inventory into the 45-slot
    /// layout; the open window carries its own vector.
    pub fn snapshot(&self, window_id: u8) -> Option<WindowSnapshot> {
        if window_id == 0 {
            return Some(WindowSnapshot {
                window_id: 0,
                slots: self.window0_slots(),
                cursor: self.cursor.clone(),
                properties: Vec::new(),
                hotbar_pop: self.hotbar_pop,
            });
        }
        let open = self
            .open
            .as_ref()
            .filter(|open| open.window_id == window_id)?;
        Some(WindowSnapshot {
            window_id,
            slots: open.slots.clone(),
            cursor: self.cursor.clone(),
            properties: open.properties.clone(),
            hotbar_pop: self.hotbar_pop,
        })
    }
}

#[cfg(test)]
mod tests {
    //! The state's own pins: the 45-slot projection, the cursor's two stores,
    //! the hostile-input counters, the pop counters and the property cap.

    use super::*;
    use oxide_proto_v47::window::WindowKind;

    /// One stack from a literal id and count.
    fn item(id: i16, count: u8) -> Option<MetadataItem> {
        Some(MetadataItem {
            id,
            count,
            damage: 0,
            nbt: None,
        })
    }

    /// An opened window with `slot_count` empty slots.
    fn opened(window_id: u8, slot_count: u8) -> OpenWindowState {
        OpenWindowState {
            window_id,
            kind: WindowKind::Chest,
            title: "Chest".into(),
            slots: vec![None; usize::from(slot_count)],
            properties: Vec::new(),
            entity_id: None,
        }
    }

    /// The id at one projected index, or `None` when the slot is empty.
    fn id_at(slots: &[Option<MetadataItem>], index: usize) -> Option<i16> {
        slots[index].as_ref().map(|item| item.id)
    }

    #[test]
    fn a_fresh_state_holds_window_zero_and_nothing_else() {
        let windows = Windows::new();
        assert!(windows.open.is_none());
        assert!(windows.cursor.is_none());
        assert_eq!(windows.action_number, 0);
        assert_eq!(windows.hotbar_pop, [0; HOTBAR_SIZE]);
        assert_eq!(windows.ignored, 0);
        assert!(windows.player.crafting.iter().all(Option::is_none));
        let slots = windows.window0_slots();
        assert_eq!(
            slots.len(),
            WINDOW0_SLOTS,
            "window 0's layout is the full container"
        );
        assert!(slots.iter().all(Option::is_none));
        assert_eq!(
            windows.snapshot(0).map(|snapshot| snapshot.window_id),
            Some(0)
        );
        assert!(windows.snapshot(1).is_none(), "no window 1 exists yet");
    }

    #[test]
    fn a_set_slot_lands_in_every_window_zero_region() {
        let mut windows = Windows::new();
        // The crafting result and the 2×2 matrix: 0–4.
        for slot in 0..5 {
            windows.apply_set_slot(0, slot, item(100 + slot, 1));
        }
        // The armour band 5–8, the main slots 9–35, the hotbar 36–44.
        for slot in 5..45 {
            windows.apply_set_slot(0, slot, item(200 + slot, 1));
        }
        let slots = windows.window0_slots();
        assert_eq!(slots.len(), WINDOW0_SLOTS);
        for slot in 0..5 {
            assert_eq!(id_at(&slots, slot as usize), Some(100 + slot));
            assert_eq!(
                windows.player.crafting[slot as usize]
                    .as_ref()
                    .map(|item| item.id),
                Some(100 + slot)
            );
        }
        // The armour band descends from the helmet at window 5 to the boots at
        // window 8 (`ContainerPlayer.java:36-54`): window 5 lands in `armor[3]`.
        assert_eq!(id_at(&slots, 5), Some(205));
        assert_eq!(
            windows.player.inventory.armor[3]
                .as_ref()
                .map(|item| item.id),
            Some(205)
        );
        assert_eq!(id_at(&slots, 8), Some(208));
        assert_eq!(
            windows.player.inventory.armor[0]
                .as_ref()
                .map(|item| item.id),
            Some(208)
        );
        assert_eq!(id_at(&slots, 9), Some(209));
        assert_eq!(
            windows.player.inventory.main[9]
                .as_ref()
                .map(|item| item.id),
            Some(209)
        );
        assert_eq!(id_at(&slots, 36), Some(236));
        assert_eq!(
            windows.player.inventory.main[0]
                .as_ref()
                .map(|item| item.id),
            Some(236)
        );
        assert_eq!(id_at(&slots, 44), Some(244));
        assert_eq!(
            windows.player.inventory.main[8]
                .as_ref()
                .map(|item| item.id),
            Some(244)
        );
        assert_eq!(windows.ignored, 0);
    }

    #[test]
    fn the_cursor_rides_window_minus_one() {
        let mut windows = Windows::new();
        windows.apply_set_slot(CURSOR_WINDOW_ID, 0, item(64, 3));
        assert_eq!(windows.cursor.as_ref().map(|item| item.id), Some(64));
        assert_eq!(
            windows.player.inventory.cursor.as_ref().map(|item| item.id),
            Some(64),
            "the model's own copy is kept in step"
        );
        assert!(
            windows.window0_slots().iter().all(Option::is_none),
            "the cursor is not one of window 0's slots"
        );
        assert_eq!(
            windows
                .snapshot(0)
                .unwrap()
                .cursor
                .as_ref()
                .map(|item| item.id),
            Some(64)
        );
        // The empty slot clears it, in both stores.
        windows.apply_set_slot(CURSOR_WINDOW_ID, 0, None);
        assert!(windows.cursor.is_none());
        assert!(windows.player.inventory.cursor.is_none());
    }

    #[test]
    fn a_set_slot_for_a_window_the_state_does_not_hold_is_ignored() {
        let mut windows = Windows::new();
        windows.apply_set_slot(5, 0, item(1, 1));
        assert_eq!(windows.ignored, 1, "nothing is open");
        windows.apply_set_slot(0, 99, item(1, 1));
        windows.apply_set_slot(0, -1, item(1, 1));
        assert_eq!(windows.ignored, 3, "past window 0's layout, and negative");
        windows.apply_open(opened(5, 27));
        windows.apply_set_slot(5, 27, item(1, 1));
        windows.apply_set_slot(6, 0, item(1, 1));
        assert_eq!(
            windows.ignored, 5,
            "past the open vector, and another window"
        );
        assert!(windows.snapshot(6).is_none());
        assert!(windows.window0_slots().iter().all(Option::is_none));
        assert!(
            windows
                .open
                .as_ref()
                .unwrap()
                .slots
                .iter()
                .all(Option::is_none),
            "nothing landed"
        );
    }

    #[test]
    fn opening_twice_replaces_and_a_close_clears_and_drops_the_cursor() {
        let mut windows = Windows::new();
        windows.apply_set_slot(CURSOR_WINDOW_ID, 0, item(1, 1));
        windows.apply_open(opened(5, 27));
        windows.apply_open(opened(7, 9));
        assert_eq!(
            windows.open.as_ref().map(|open| open.window_id),
            Some(7),
            "the new window replaces the old"
        );
        // A close naming another window leaves the state alone.
        assert!(!windows.apply_close(5));
        assert_eq!(windows.open.as_ref().map(|open| open.window_id), Some(7));
        assert!(
            windows.cursor.is_some(),
            "a close that landed nowhere drops nothing"
        );
        // The close that names the open window clears it and drops the cursor.
        assert!(windows.apply_close(7));
        assert!(windows.open.is_none());
        assert!(windows.cursor.is_none());
        assert!(windows.player.inventory.cursor.is_none());
        // A second close changes nothing.
        assert!(!windows.apply_close(7));
        assert!(windows.open.is_none());
        // Window 0 is not the server's to close.
        assert!(!windows.apply_close(0));
        assert_eq!(
            windows.snapshot(0).map(|snapshot| snapshot.window_id),
            Some(0),
            "window 0 is held for the whole session"
        );
    }

    #[test]
    fn the_action_number_counts_from_one_and_wraps_at_the_maximum() {
        let mut windows = Windows::new();
        assert_eq!(windows.next_action_number(), 1);
        assert_eq!(windows.next_action_number(), 2);
        windows.action_number = i16::MAX;
        assert_eq!(
            windows.next_action_number(),
            i16::MIN,
            "the Java short wraps in two's complement"
        );
    }

    #[test]
    fn properties_resize_per_window_within_the_cap() {
        let mut windows = Windows::new();
        // Window 0 has no properties: the write lands nowhere.
        windows.apply_property(0, 0, 40);
        assert_eq!(windows.ignored, 1);
        windows.apply_open(opened(5, 3));
        windows.apply_property(5, 2, 40);
        {
            let open = windows.open.as_ref().unwrap();
            assert_eq!(
                open.properties.len(),
                3,
                "the vector grows to hold the index"
            );
            assert_eq!(open.properties, vec![0, 0, 40]);
        }
        windows.apply_property(5, 0, 7);
        assert_eq!(
            windows.open.as_ref().unwrap().properties,
            vec![7, 0, 40],
            "an earlier index keeps the grown length"
        );
        // Past the largest index any 1.8.9 container writes, and negative.
        windows.apply_property(5, 7, 1);
        windows.apply_property(5, -1, 1);
        assert_eq!(windows.ignored, 3);
        assert_eq!(windows.open.as_ref().unwrap().properties.len(), 3);
    }

    #[test]
    fn a_hotbar_write_that_grew_the_stack_sets_the_pop_and_a_replace_clears_it() {
        let mut windows = Windows::new();
        // A stack into an empty slot grows it: the pop is set.
        windows.apply_set_slot(0, 36, item(1, 1));
        assert_eq!(windows.hotbar_pop[0], HOTBAR_POP_TICKS);
        // A bigger count grows it again.
        windows.apply_set_slot(0, 36, item(1, 4));
        assert_eq!(windows.hotbar_pop[0], HOTBAR_POP_TICKS);
        // The same count replaces the stack without a pop: the incoming stack
        // carries its own zero.
        windows.apply_set_slot(0, 36, item(1, 4));
        assert_eq!(windows.hotbar_pop[0], 0);
        // A smaller count replaces it too.
        windows.apply_set_slot(0, 44, item(2, 9));
        assert_eq!(windows.hotbar_pop[8], HOTBAR_POP_TICKS);
        windows.apply_set_slot(0, 44, item(2, 1));
        assert_eq!(windows.hotbar_pop[8], 0);
        // A write elsewhere in window 0 never pops.
        windows.apply_set_slot(0, 9, item(3, 64));
        windows.apply_set_slot(0, 0, item(4, 64));
        assert!(windows.hotbar_pop.iter().all(|pop| *pop == 0));
        // The window's whole slot set replaces the stacks: the packet's own
        // stacks carry no pop.
        windows.apply_set_slot(0, 36, item(1, 8));
        assert_eq!(windows.hotbar_pop[0], HOTBAR_POP_TICKS);
        windows.apply_window_items(0, vec![None; WINDOW0_SLOTS]);
        assert!(windows.hotbar_pop.iter().all(|pop| *pop == 0));
    }

    #[test]
    fn the_pop_counters_tick_down_once_per_tick() {
        let mut windows = Windows::new();
        windows.apply_set_slot(0, 36, item(1, 1));
        assert_eq!(windows.hotbar_pop[0], HOTBAR_POP_TICKS);
        assert!(windows.tick_hotbar_pop(), "the tick moved a live counter");
        assert_eq!(windows.hotbar_pop[0], HOTBAR_POP_TICKS - 1);
        for expected in [3, 2, 1, 0] {
            assert!(windows.tick_hotbar_pop());
            assert_eq!(windows.hotbar_pop[0], expected);
        }
        assert!(
            !windows.tick_hotbar_pop(),
            "a spent counter is not a change"
        );
        assert_eq!(windows.hotbar_pop[0], 0);
    }

    #[test]
    fn every_apply_reports_whether_it_landed() {
        let mut windows = Windows::new();
        // The cursor and window 0's own slots always land.
        assert!(windows.apply_set_slot(CURSOR_WINDOW_ID, 0, item(1, 1)));
        assert!(windows.apply_set_slot(0, 36, item(2, 1)));
        // A window the state does not hold, and a slot past window 0's layout,
        // land nowhere.
        assert!(!windows.apply_set_slot(5, 0, item(3, 1)));
        assert!(!windows.apply_set_slot(0, 45, item(3, 1)));
        // A whole-set write lands when a slot of it did, and an empty packet
        // changes nothing.
        assert!(windows.apply_window_items(0, vec![item(4, 1)]));
        assert!(!windows.apply_window_items(0, Vec::new()));
        // The open window's own vector, and the ids it does not answer to.
        windows.apply_open(opened(5, 27));
        assert!(windows.apply_window_items(5, vec![item(5, 1)]));
        assert!(!windows.apply_window_items(6, vec![item(5, 1)]));
        assert!(!windows.apply_property(6, 0, 1));
        assert!(windows.apply_property(5, 0, 1));
        assert!(!windows.apply_property(5, 7, 1));
        assert_eq!(
            windows.ignored, 5,
            "each write that landed nowhere counted once"
        );
    }

    #[test]
    fn window_items_writes_both_window_layouts() {
        let mut windows = Windows::new();
        let mut slots: Vec<Option<MetadataItem>> = vec![None; WINDOW0_SLOTS];
        slots[0] = item(1, 1); // the crafting result
        slots[5] = item(2, 1); // the helmet
        slots[9] = item(3, 1); // a main slot
        slots[36] = item(4, 1); // a hotbar slot
        windows.apply_window_items(0, slots);
        assert_eq!(
            windows.player.crafting[0].as_ref().map(|item| item.id),
            Some(1)
        );
        assert_eq!(
            windows.player.inventory.armor[3]
                .as_ref()
                .map(|item| item.id),
            Some(2)
        );
        assert_eq!(
            windows.player.inventory.main[9]
                .as_ref()
                .map(|item| item.id),
            Some(3)
        );
        assert_eq!(
            windows.player.inventory.main[0]
                .as_ref()
                .map(|item| item.id),
            Some(4)
        );
        assert_eq!(windows.ignored, 0);
        // A window-0 packet with more slots than the layout holds: the extras
        // land nowhere.
        windows.apply_window_items(0, vec![None; WINDOW0_SLOTS + 1]);
        assert_eq!(windows.ignored, 1);
        // The open window's vector takes the packet index by index.
        windows.apply_open(opened(5, 27));
        let mut chest: Vec<Option<MetadataItem>> = vec![None; 27];
        chest[26] = item(9, 2);
        windows.apply_window_items(5, chest);
        assert_eq!(
            windows.open.as_ref().unwrap().slots[26]
                .as_ref()
                .map(|item| item.id),
            Some(9)
        );
        // A shorter packet leaves the tail standing (`putStacksInSlots:546-552`
        // writes the packet's own indices alone).
        windows.apply_window_items(5, vec![item(8, 1)]);
        assert_eq!(
            windows.open.as_ref().unwrap().slots[0]
                .as_ref()
                .map(|item| item.id),
            Some(8)
        );
        assert_eq!(
            windows.open.as_ref().unwrap().slots[26]
                .as_ref()
                .map(|item| item.id),
            Some(9)
        );
        // Past the vector, and for a window the state does not hold.
        windows.apply_window_items(5, vec![None; 28]);
        windows.apply_window_items(6, vec![None; 1]);
        assert_eq!(windows.ignored, 3);
    }

    #[test]
    fn a_confirm_names_window_zero_or_the_live_open_window() {
        let mut windows = Windows::new();
        assert!(
            windows.holds_window(0),
            "window 0 is held for the whole session"
        );
        assert!(!windows.holds_window(5));
        assert!(
            !windows.holds_window(-1),
            "the wire's 0xFF names no container"
        );
        windows.apply_open(opened(5, 27));
        assert!(windows.holds_window(5));
        assert!(!windows.holds_window(6));
        windows.apply_close(5);
        assert!(!windows.holds_window(5), "a closed window holds nothing");
    }

    #[test]
    fn the_snapshot_carries_each_window_layout() {
        let mut windows = Windows::new();
        windows.apply_set_slot(CURSOR_WINDOW_ID, 0, item(1, 2));
        windows.apply_set_slot(0, 36, item(2, 1));
        let snapshot = windows.snapshot(0).expect("window 0 is always held");
        assert_eq!(snapshot.window_id, 0);
        assert_eq!(snapshot.slots.len(), WINDOW0_SLOTS);
        assert_eq!(snapshot.slots[36].as_ref().map(|item| item.id), Some(2));
        assert_eq!(snapshot.cursor.as_ref().map(|item| item.id), Some(1));
        assert!(
            snapshot.properties.is_empty(),
            "window 0's container has no properties"
        );
        assert_eq!(snapshot.hotbar_pop[0], HOTBAR_POP_TICKS);
        windows.apply_open(opened(5, 3));
        windows.apply_property(5, 0, 4);
        let snapshot = windows.snapshot(5).expect("the open window is held");
        assert_eq!(snapshot.slots.len(), 3);
        assert_eq!(snapshot.properties, vec![4]);
        assert_eq!(
            snapshot.cursor.as_ref().map(|item| item.id),
            Some(1),
            "the carried stack rides every window"
        );
        assert!(windows.snapshot(6).is_none());
    }
}
