//! The click machine: `Container`'s slot-click arithmetic, the drag machine's
//! shared state and the creative screen's own container model.
//!
//! The module carries the parts of the source's container layer that a screen
//! reaches before a packet exists: the button-bit tables the drag sends are
//! spelled with (`Container.getDragEvent`/`extractDragMode`/`func_94534_d`,
//! `Container.java:687-703`), the mode table a drag validates against
//! (`isValidDragMode`, `:705-708`), the merge and split arithmetic of a drag
//! (`canAddItemToSlot`, `:722-732`; `computeStackSize`, `:738-755`), and the
//! creative screen's own slot vector with the source's click behaviour over
//! it. Every constant and branch is traceable to the cited MCP-919 source and
//! pinned by a test below.

use crate::windows::HOTBAR_SIZE;
use oxide_proto_v47::entity::MetadataItem;

/// The base stack cap: one stack of any item holds sixty-four
/// (`Item.maxStackSize`, `item/Item.java:58`, which `ItemStack.getMaxStackSize`
/// answers through `Item.getItemStackLimit`, `:165-167`).
pub const BASE_MAX_STACK_SIZE: i32 = 64;

/// The creative pane's slot count: five rows of nine over the tab's display
/// area (`GuiContainerCreative.java:860-866`).
pub const CREATIVE_PANE_SLOTS: usize = 45;

/// The creative container's slot count: the pane's forty-five followed by the
/// player's own hotbar band of nine (`GuiContainerCreative.java:868-871`).
pub const CREATIVE_SLOTS: usize = CREATIVE_PANE_SLOTS + HOTBAR_SIZE;

/// The pickup and place mode (`Container.slotClick`'s `mode == 0` branches,
/// `Container.java:232-388`).
pub const CLICK_MODE_PICKUP: i8 = 0;
/// The shift quick-move mode (`Container.java:255-289`).
pub const CLICK_MODE_QUICK_MOVE: i8 = 1;
/// The number-key swap mode (`Container.java:390-434`).
pub const CLICK_MODE_SWAP: i8 = 2;
/// The creative pick mode (`Container.java:435-445`).
pub const CLICK_MODE_CREATIVE_PICK: i8 = 3;
/// The drop mode (`Container.java:446-456`).
pub const CLICK_MODE_DROP: i8 = 4;
/// The drag mode (`Container.java:145-227`).
pub const CLICK_MODE_DRAG: i8 = 5;
/// The double-click gather mode (`Container.java:457-491`).
pub const CLICK_MODE_GATHER: i8 = 6;

/// The clicked buttons the number-key swap answers: the player's own nine
/// (`Container.java:390`'s `clickedButton >= 0 && clickedButton < 9`).
const SWAP_BUTTONS: i32 = HOTBAR_SIZE as i32;

/// The per-item stack cap the container arithmetic reads: the source's
/// `ItemStack.getMaxStackSize` (`ItemStack.java:234-236` reaching
/// `Item.getItemStackLimit`, `Item.java:165-167`).
///
/// This is the seam Task 8's item registry implements over its per-id
/// override list; the base rule alone is [`BASE_MAX_STACK_SIZE`].
pub trait StackCaps {
    /// The largest stack of `item`.
    fn max_stack_size(&self, item: &MetadataItem) -> i32;
}

/// The base rule alone: every item caps at [`BASE_MAX_STACK_SIZE`]
/// (`Item.maxStackSize`, `Item.java:58`) — what a cap table that knows no
/// overrides answers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BaseStackCaps;

impl StackCaps for BaseStackCaps {
    fn max_stack_size(&self, _item: &MetadataItem) -> i32 {
        BASE_MAX_STACK_SIZE
    }
}

/// The cap of one item from a cap table — the plan's named read over the
/// [`StackCaps`] seam.
pub fn max_stack_size(item: &MetadataItem, table: &impl StackCaps) -> i32 {
    table.max_stack_size(item)
}

/// The drag event packed in a drag button's low two bits
/// (`Container.getDragEvent`, `Container.java:695-698`): 0 the start, 1 a slot
/// joining the set, 2 the end.
pub fn get_drag_event(button: i32) -> i32 {
    button & 3
}

/// The drag mode packed in a drag button's high bits (`Container.extractDragMode`,
/// `Container.java:687-690`): 0 an even split, 1 a single item a slot, 2 the
/// whole stack.
pub fn extract_drag_mode(button: i32) -> i32 {
    (button >> 2) & 3
}

/// The drag button a drag event and mode travel as (`Container.func_94534_d`,
/// `Container.java:700-703`).
pub fn drag_button(event: i32, mode: i32) -> i32 {
    (event & 3) | ((mode & 3) << 2)
}

/// Whether a drag mode may start (`Container.isValidDragMode`,
/// `Container.java:705-708`): the even split and the single item always, the
/// whole-stack mode only under creative.
pub fn is_valid_drag_mode(mode: i32, creative: bool) -> bool {
    match mode {
        0 | 1 => true,
        2 => creative,
        _ => false,
    }
}

/// Whether the dragged stack may join a slot (`Container.canAddItemToSlot`,
/// `Container.java:722-732`, with the source's own `stackSizeMatters` true as
/// both drag call sites pass it, `:176`, `:190`): an empty slot always takes;
/// a stack of another item, damage or tag never; a stack of the same item
/// takes while the slot's own count stays within `max`.
pub fn can_add_item_to_slot(slot: &Option<MetadataItem>, stack: &MetadataItem, max: i32) -> bool {
    let Some(held) = slot else {
        return true;
    };
    same_stack(held, stack) && i32::from(held.count) <= max
}

/// The same-stack test a merge reads (`Container.java:726` and the merge
/// branch's `:338`: the same item, the same damage, equal tags — the count is
/// not part of it).
fn same_stack(one: &MetadataItem, other: &MetadataItem) -> bool {
    one.id == other.id && one.damage == other.damage && one.nbt == other.nbt
}

/// A count narrowed into the wire's own byte: every move the container works
/// out lands far inside it, and a count only a hostile write could push past
/// the byte stops at its top rather than wrapping.
fn count_of(count: i32) -> u8 {
    u8::try_from(count).unwrap_or(u8::MAX)
}

/// The size a dragged-into slot takes when a drag ends
/// (`Container.computeStackSize`, `Container.java:738-755`, with the item cap
/// the source applies straight after the call folded in, `:196-198`): the
/// mode's base — an even split of the dragged stack across the dragged set,
/// one item, or the dragged item's own cap — plus the slot's own count, capped
/// at the dragged item's limit.
///
/// `drag` is the stack being carried (the source's `itemstack1`, a copy of the
/// cursor), `slot` the slot's current stack (the source's `k`) and `others`
/// the dragged-into set, whose length divides the split. The slot's own
/// inventory limit (`:201-204`) reads a slot kind this vector does not carry;
/// it belongs to the caller that knows the slot.
pub fn compute_stack_size(
    drag_mode: i32,
    drag: &MetadataItem,
    slot: Option<&MetadataItem>,
    others: &[Option<MetadataItem>],
    caps: &impl StackCaps,
) -> i32 {
    let held = slot.map_or(0, |stack| i32::from(stack.count));
    let base = match drag_mode {
        0 => match others.len() {
            // The split's divisor is the set's own size; the end pass only
            // runs with a set, so the empty read is a guard alone
            // (`Container.java:183`).
            0 => return held,
            len => i32::from(drag.count) / i32::try_from(len).unwrap_or(i32::MAX),
        },
        1 => 1,
        2 => max_stack_size(drag, caps),
        // The source's switch has no default (`:740-752`).
        _ => i32::from(drag.count),
    };
    (base + held).min(max_stack_size(drag, caps))
}

/// The view's drag machine: the screen-side state a drag keeps while it is
/// open, which the source spreads over `GuiContainer`'s `dragSplitting`,
/// `dragSplittingLimit`, `dragSplittingSlots` and `draggedStack` fields, for
/// the container clicks the drag drives.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DragState {
    /// The drag's mode, `extract_drag_mode`'s read of the start button
    /// (`Container.java:687-690`).
    pub mode: i32,
    /// The raw drag button the last drag event carried, the source's
    /// `func_94534_d` composition of the event and the mode
    /// (`Container.java:700-703`).
    pub button: i32,
    /// The slot indices the drag covers, the view's `dragSplittingSlots`.
    pub slots: Vec<i16>,
    /// The stack left on the cursor when the drag ends, the view's preview of
    /// `draggedStack`.
    pub remnant: Option<MetadataItem>,
}

/// The creative screen's own container: the tab pane's forty-five slots
/// followed by the player's hotbar band (`GuiContainerCreative.java:860-871`),
/// with the source's click behaviour for the modes the pane sends
/// (`Container.slotClick`, `Container.java:140-494`, under the creative
/// container's own overrides, `GuiContainerCreative.java:914-941`).
///
/// The shell hooks the source runs beside a click — `onSlotChanged`,
/// `onPickupFromSlot`, `detectAndSendChanges`, a world drop — belong to the
/// view that owns the container; nothing here touches a world, a listener or
/// the wire. A click's own return, the carrier its packet would echo
/// (`PlayerControllerMP.windowClick:534-540`), is [`LocalContainer::slot_click`]'s
/// return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalContainer {
    /// The slots: the pane's forty-five (`GuiContainerCreative.java:860-866`)
    /// then the hotbar band's nine (`:868-871`); `None` is an empty slot.
    pub slots: Vec<Option<MetadataItem>>,
    /// The carried stack (the source's `InventoryPlayer.itemStack:34`, which
    /// the creative container's own slots share).
    pub cursor: Option<MetadataItem>,
    /// The drag's mode, the source's `Container.dragMode:24` — `-1` before any
    /// drag, then `extract_drag_mode`'s read of a start button.
    pub drag_mode: i32,
    /// The drag's event, the source's `Container.dragEvent:27`: 0 idle, 1 the
    /// set is filling, 2 the end.
    pub drag_event: i32,
    /// The slots the drag covers, the source's `Container.dragSlots:28` — a
    /// set there, so a slot joins once however often the pointer crosses it.
    pub drag_slots: Vec<i16>,
}

impl Default for LocalContainer {
    fn default() -> Self {
        Self {
            slots: vec![None; CREATIVE_SLOTS],
            cursor: None,
            drag_mode: -1,
            drag_event: 0,
            drag_slots: Vec::new(),
        }
    }
}

impl LocalContainer {
    /// An empty container: every slot empty, nothing carried, no drag.
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs one slot click over the container's own vector — the source's
    /// `Container.slotClick` (`Container.java:140-494`) as the creative
    /// screen's container reaches it, under the creative container's own
    /// overrides: the shift transfer that deletes a hotbar slot's stack and
    /// answers nothing (`ContainerCreative.transferStackInSlot:918-931`) and
    /// the merge and drag bounds that hold only the hotbar band
    /// (`canMergeSlot:933-936`, `canDragIntoSlot:938-941`).
    ///
    /// Two of the source's own conditions are answered by this container's
    /// shape rather than read again: `Slot.isItemValid` is the base's true
    /// (`Slot.java:73-76`) and `canTakeStack` the base's true
    /// (`Slot.java:150-153`) for every slot of the vector, and the pick
    /// branch's `capabilities.isCreativeMode` guard (`Container.java:435`)
    /// holds for every screen that owns this container. The slots'
    /// inventory limit, sixty-four everywhere here (`Slot.getItemStackLimit`
    /// reaching the inventories' own `getInventoryStackLimit`), is folded into
    /// the arithmetic where the source reads it. The shell hooks the click
    /// runs beside itself — `onSlotChanged`, `onPickupFromSlot`,
    /// `detectAndSendChanges`, a world drop — belong to the view.
    ///
    /// The carried stack's own drop outside every slot (`Container.java:234-254`'s
    /// −999 branch) is the screen's to route: the creative screen swallows a
    /// click that meets no slot on its filled tabs itself (`GuiContainerCreative.java:91-119`),
    /// so it does not reach this container, and an index the vector cannot
    /// carry is no slot at all (the source's own list would throw on it).
    ///
    /// `caps` is the item cap table, consulted wherever the source reads
    /// `ItemStack.getMaxStackSize`.
    ///
    /// Returns the source's own return for the branch the click takes: the
    /// slot's pre-click stack for a pickup on a filled slot
    /// (`Container.java:291-296`), and nothing for every other branch — the
    /// shift click and its retry (`:266-279`), the place (`:299-319`), the
    /// swap (`:390-434`), the pick (`:435-445`), the drop (`:446-456`) and the
    /// gather (`:457-491`) all leave the source's own carrier null.
    pub fn slot_click(
        &mut self,
        slot: i16,
        button: i8,
        mode: i8,
        caps: &impl StackCaps,
    ) -> Option<MetadataItem> {
        let button = i32::from(button);
        let mode = i32::from(mode);
        let index = usize::try_from(slot)
            .ok()
            .filter(|index| *index < self.slots.len());

        if mode == i32::from(CLICK_MODE_DRAG) {
            self.drag(slot, index, button, caps);
            return None;
        }

        if self.drag_event != 0 {
            // A click of any other mode with a drag open drops the drag and
            // lands nowhere (`Container.java:228-231`).
            self.reset_drag();
            return None;
        }

        if (mode == i32::from(CLICK_MODE_PICKUP) || mode == i32::from(CLICK_MODE_QUICK_MOVE))
            && (button == 0 || button == 1)
        {
            if mode == i32::from(CLICK_MODE_QUICK_MOVE) {
                // The source answers a shift click with its transfer's own
                // return (`:266-271`), which the creative override leaves null.
                return index.and_then(|index| self.transfer_stack_in_slot(index));
            }
            return index.and_then(|index| self.pickup_click(index, button, caps));
        }

        if mode == i32::from(CLICK_MODE_SWAP) && (0..SWAP_BUTTONS).contains(&button) {
            if let Some(index) = index {
                self.swap_with_hotbar(index, button);
            }
        } else if mode == i32::from(CLICK_MODE_CREATIVE_PICK) && self.cursor.is_none() {
            if let Some(index) = index {
                self.creative_pick(index, caps);
            }
        } else if mode == i32::from(CLICK_MODE_DROP) && self.cursor.is_none() {
            if let Some(index) = index {
                self.drop_click(index, button);
            }
        } else if mode == i32::from(CLICK_MODE_GATHER) {
            self.gather(index, button, caps);
        }

        None
    }

    /// One drag event (`Container.java:145-227`): the button's bits carry the
    /// event and the mode, the set fills while the drag is open, and the end
    /// pass splits the cursor across it.
    fn drag(&mut self, slot: i16, index: Option<usize>, button: i32, caps: &impl StackCaps) {
        let previous = self.drag_event;
        self.drag_event = get_drag_event(button);
        // The source's own two reset branches (`Container.java:150-157`) — the
        // event that left the machine's life, and the cursor already down when
        // the news arrives — fold into one here.
        if ((previous != 1 || self.drag_event != 2) && previous != self.drag_event)
            || self.cursor.is_none()
        {
            self.reset_drag();
        } else if self.drag_event == 0 {
            self.drag_mode = extract_drag_mode(button);
            if is_valid_drag_mode(self.drag_mode, true) {
                self.drag_event = 1;
                self.drag_slots.clear();
            } else {
                self.reset_drag();
            }
        } else if self.drag_event == 1 {
            let Some(index) = index else {
                return;
            };
            let joins = {
                let Some(cursor) = self.cursor.as_ref() else {
                    return;
                };
                let cap = max_stack_size(cursor, caps);
                let covered = i32::try_from(self.drag_slots.len()).unwrap_or(i32::MAX);
                can_add_item_to_slot(&self.slots[index], cursor, cap)
                    && i32::from(cursor.count) > covered
                    && self.can_drag_into_slot(index)
            };
            if joins && !self.drag_slots.contains(&slot) {
                // The set holds a slot once however often the pointer crosses
                // it (`Container.java:28`).
                self.drag_slots.push(slot);
            }
        } else if self.drag_event == 2 {
            self.finish_drag(caps);
        } else {
            // The fourth event pair is no event: the drag drops
            // (`Container.java:223-226`).
            self.reset_drag();
        }
    }

    /// The drag's end pass (`Container.java:181-222`): each covered slot takes
    /// the size the mode computes for it, the remainder rides back on the
    /// cursor, and the drag closes either way.
    fn finish_drag(&mut self, caps: &impl StackCaps) {
        if !self.drag_slots.is_empty() {
            let Some(cursor) = self.cursor.clone() else {
                self.reset_drag();
                return;
            };
            let cap = max_stack_size(&cursor, caps);
            let covered = i32::try_from(self.drag_slots.len()).unwrap_or(i32::MAX);
            let set: Vec<Option<MetadataItem>> = self
                .drag_slots
                .iter()
                .filter_map(|slot| usize::try_from(*slot).ok())
                .map(|index| self.slots.get(index).cloned().flatten())
                .collect();
            let mut left = i32::from(cursor.count);
            for position in 0..self.drag_slots.len() {
                let Ok(index) = usize::try_from(self.drag_slots[position]) else {
                    continue;
                };
                if index >= self.slots.len() {
                    continue;
                }
                // The source's per-slot guard (`:190`): the slot must take the
                // dragged stack, the cursor must still cover the set, and the
                // creative bound must hold.
                if !can_add_item_to_slot(&self.slots[index], &cursor, cap)
                    || i32::from(cursor.count) < covered
                    || !self.can_drag_into_slot(index)
                {
                    continue;
                }
                let held = self.slots[index].clone();
                let size = compute_stack_size(self.drag_mode, &cursor, held.as_ref(), &set, caps)
                    .min(BASE_MAX_STACK_SIZE);
                left -= size - held.as_ref().map_or(0, |stack| i32::from(stack.count));
                self.slots[index] = Some(MetadataItem {
                    count: count_of(size),
                    ..cursor.clone()
                });
            }
            self.cursor = if left > 0 {
                Some(MetadataItem {
                    count: count_of(left),
                    ..cursor
                })
            } else {
                None
            };
        }
        self.reset_drag();
    }

    /// Clears the drag's state (`Container.resetDrag`, `Container.java:713-717`).
    fn reset_drag(&mut self) {
        self.drag_event = 0;
        self.drag_mode = -1;
        self.drag_slots.clear();
    }

    /// Whether a slot index is one of the player's own nine — the last band of
    /// the container, which wraps the player's inventory
    /// (`GuiContainerCreative.java:868-871`).
    pub fn is_player_slot(&self, index: usize) -> bool {
        index >= CREATIVE_PANE_SLOTS
    }

    /// The creative container's drag bound (`ContainerCreative.canDragIntoSlot`,
    /// `GuiContainerCreative.java:938-941`): a slot in the player's own
    /// inventory accepts a drag across it, so the hotbar band does and the
    /// pane does not.
    pub fn can_drag_into_slot(&self, index: usize) -> bool {
        self.is_player_slot(index)
    }

    /// The creative container's merge bound (`ContainerCreative.canMergeSlot`,
    /// `GuiContainerCreative.java:933-936`, its `yDisplayPosition > 90`): only
    /// the hotbar band stands under that line, so only it merges.
    pub fn can_merge_slot(&self, index: usize) -> bool {
        self.is_player_slot(index)
    }

    /// The first free slot of the player's own band — the source's
    /// `InventoryPlayer.getFirstEmptyStack` (`Container.java:402`), whose own
    /// search walks the whole of the player's inventory; this container
    /// carries the hotbar band's nine alone.
    fn first_empty_player_slot(&self) -> Option<usize> {
        (CREATIVE_PANE_SLOTS..self.slots.len()).find(|index| self.slots[*index].is_none())
    }

    /// The creative container's own shift transfer
    /// (`ContainerCreative.transferStackInSlot:918-931`): a clicked hotbar
    /// slot's stack is cleared outright — the creative screen deletes it — and
    /// the pane's slots are left alone. Its answer is the source's own null,
    /// so the retry a carried stack would trigger (`Container.java:268-276`)
    /// never runs.
    fn transfer_stack_in_slot(&mut self, index: usize) -> Option<MetadataItem> {
        if self.is_player_slot(index) {
            self.slots[index] = None;
        }
        None
    }

    /// The pickup and place branch (`Container.java:280-388`): the carried
    /// stack places into an empty slot, a filled slot's stack picks up (whole
    /// on the left, half on the right), a same-stack pair merges up to the
    /// caps, and another item swaps. Returns the branch's own carrier — the
    /// slot's pre-click stack (`:291-296`).
    fn pickup_click(
        &mut self,
        index: usize,
        button: i32,
        caps: &impl StackCaps,
    ) -> Option<MetadataItem> {
        let held = self.slots[index].clone();
        let echo = held.clone();
        let cursor = self.cursor.clone();
        match (held, cursor) {
            // Place (`:299-319`).
            (None, Some(cursor)) => {
                let wanted = if button == 0 {
                    i32::from(cursor.count)
                } else {
                    1
                };
                let wanted = wanted.min(BASE_MAX_STACK_SIZE);
                if i32::from(cursor.count) >= wanted {
                    self.slots[index] = Some(MetadataItem {
                        count: count_of(wanted),
                        ..cursor.clone()
                    });
                    let left = i32::from(cursor.count) - wanted;
                    self.cursor = if left > 0 {
                        Some(MetadataItem {
                            count: count_of(left),
                            ..cursor
                        })
                    } else {
                        None
                    };
                }
            }
            // Pick (`:323-334`).
            (Some(held), None) => {
                let wanted = if button == 0 {
                    i32::from(held.count)
                } else {
                    (i32::from(held.count) + 1) / 2
                };
                self.cursor = Some(MetadataItem {
                    count: count_of(wanted),
                    ..held.clone()
                });
                let left = i32::from(held.count) - wanted;
                self.slots[index] = if left > 0 {
                    Some(MetadataItem {
                        count: count_of(left),
                        ..held
                    })
                } else {
                    None
                };
            }
            // Merge or swap (`:336-365`).
            (Some(held), Some(cursor)) => {
                if same_stack(&held, &cursor) {
                    let mut moved = if button == 0 {
                        i32::from(cursor.count)
                    } else {
                        1
                    };
                    moved = moved.min(BASE_MAX_STACK_SIZE - i32::from(held.count));
                    moved = moved.min(max_stack_size(&cursor, caps) - i32::from(held.count));
                    // A slot a hostile write pushed past the caps would run
                    // the source's own subtraction negative; zero it here.
                    let moved = moved.max(0);
                    let left = i32::from(cursor.count) - moved;
                    self.cursor = if left > 0 {
                        Some(MetadataItem {
                            count: count_of(left),
                            ..cursor
                        })
                    } else {
                        None
                    };
                    if moved > 0 {
                        self.slots[index] = Some(MetadataItem {
                            count: count_of(i32::from(held.count) + moved),
                            ..held
                        });
                    }
                } else if i32::from(cursor.count) <= BASE_MAX_STACK_SIZE {
                    // Swap: the two stacks exchange when the cursor fits the
                    // slot's own limit (`:361-365`).
                    self.slots[index] = Some(cursor);
                    self.cursor = Some(held);
                }
            }
            // An empty slot under an empty cursor is no click at all
            // (`:299`'s own guard pair).
            (None, None) => {}
        }
        echo
    }

    /// The number-key branch (`Container.java:390-434`): the clicked slot and
    /// the player's hotbar slot the number names exchange through the carried
    /// stack.
    fn swap_with_hotbar(&mut self, index: usize, button: i32) {
        let hotbar = CREATIVE_PANE_SLOTS + usize::try_from(button).unwrap_or(0);
        let swap = self.slots.get(hotbar).cloned().flatten();
        let player_slot = self.is_player_slot(index);
        let flag = swap.is_none() || player_slot;
        let free = if flag {
            None
        } else {
            self.first_empty_player_slot()
        };
        let flag = flag || free.is_some();
        let held = self.slots[index].clone();
        if let Some(held) = held {
            if flag {
                // The hotbar slot takes a copy of the clicked stack (`:409`).
                self.slots[hotbar] = Some(held);
                if !player_slot && swap.is_some() {
                    // The displaced stack takes the first free player slot and
                    // the clicked slot empties (`:411-419`).
                    if let Some(free) = free {
                        self.slots[free] = swap;
                        self.slots[index] = None;
                    }
                } else {
                    // The clicked slot takes the hotbar's old stack, empty or
                    // not (`:421-426`).
                    self.slots[index] = swap;
                }
            }
        } else if let Some(swap) = swap {
            // An empty slot takes the hotbar's stack and the hotbar clears
            // (`:428-432`).
            self.slots[hotbar] = None;
            self.slots[index] = Some(swap);
        }
    }

    /// The creative pick (`Container.java:435-445`): a filled slot's stack
    /// rides out, whole to its cap.
    fn creative_pick(&mut self, index: usize, caps: &impl StackCaps) {
        if let Some(held) = self.slots[index].clone() {
            self.cursor = Some(MetadataItem {
                count: count_of(max_stack_size(&held, caps)),
                ..held
            });
        }
    }

    /// The drop (`Container.java:446-456`): one item on the left, the slot's
    /// whole stack on the right. The stack the source hands to the world
    /// (`:454`) is the view's to drop — this container holds no world.
    fn drop_click(&mut self, index: usize, button: i32) {
        if let Some(held) = self.slots[index].clone() {
            let wanted = if button == 0 {
                1
            } else {
                i32::from(held.count)
            };
            let left = i32::from(held.count) - wanted;
            self.slots[index] = if left > 0 {
                Some(MetadataItem {
                    count: count_of(left),
                    ..held
                })
            } else {
                None
            };
        }
    }

    /// The double-click gather (`Container.java:457-491`): with a stack
    /// carried and the double-clicked slot empty, the cursor tops up from
    /// every slot that merges into it, twice over — the first pass honouring
    /// only slots that are not already full, the second taking from those too
    /// — walking out from the side the button names. Only the hotbar band
    /// merges into it (`canMergeSlot:933-936`).
    fn gather(&mut self, index: Option<usize>, button: i32, caps: &impl StackCaps) {
        let Some(mut cursor) = self.cursor.clone() else {
            return;
        };
        if index.is_some_and(|index| self.slots[index].is_some()) {
            // The source's own guard (`:462`): a filled slot under the double
            // click takes the click instead.
            return;
        }
        let cap = max_stack_size(&cursor, caps);
        let count = self.slots.len();
        let walk: Vec<usize> = if button == 0 {
            (0..count).collect()
        } else {
            (0..count).rev().collect()
        };
        for pass in 0..2 {
            for index in walk.iter().copied() {
                if i32::from(cursor.count) >= cap {
                    // The source's own loop condition (`:469`).
                    break;
                }
                let Some(held) = self.slots[index].clone() else {
                    continue;
                };
                let full = i32::from(held.count) == max_stack_size(&held, caps);
                if !can_add_item_to_slot(&Some(held.clone()), &cursor, cap)
                    || !self.can_merge_slot(index)
                    || (pass == 0 && full)
                {
                    continue;
                }
                let take = (cap - i32::from(cursor.count)).min(i32::from(held.count));
                cursor.count = count_of(i32::from(cursor.count) + take);
                let left = i32::from(held.count) - take;
                self.slots[index] = if left > 0 {
                    Some(MetadataItem {
                        count: count_of(left),
                        ..held
                    })
                } else {
                    None
                };
            }
        }
        self.cursor = Some(cursor);
    }
}

#[cfg(test)]
mod tests {
    //! The arithmetic's own pins: the drag button's bits, the mode table, the
    //! merge and split rules, and the creative container's click behaviour.

    use super::*;

    /// One stack from a literal id and count, with no damage and no tag.
    fn item(id: i16, count: u8) -> MetadataItem {
        MetadataItem {
            id,
            count,
            damage: 0,
            nbt: None,
        }
    }

    /// One stack with a damage value and a single-byte tag tail.
    fn tailed(id: i16, count: u8, damage: i16, tag: u8) -> MetadataItem {
        MetadataItem {
            id,
            count,
            damage,
            nbt: Some(vec![tag]),
        }
    }

    /// The test cap table: the base sixty-four for every item, with the listed
    /// ids capped at one — the shape Task 8's registry will carry as real
    /// overrides.
    struct TestCaps {
        ones: Vec<i16>,
    }

    impl StackCaps for TestCaps {
        fn max_stack_size(&self, item: &MetadataItem) -> i32 {
            if self.ones.contains(&item.id) {
                1
            } else {
                BASE_MAX_STACK_SIZE
            }
        }
    }

    /// A cap table with the listed ids capped at one.
    fn caps_of(ones: &[i16]) -> TestCaps {
        TestCaps {
            ones: ones.to_vec(),
        }
    }

    /// An empty dragged-into set of `n` slots, as the arithmetic takes it.
    fn empties(n: usize) -> Vec<Option<MetadataItem>> {
        vec![None; n]
    }

    /// A container with the listed stacks in place.
    fn local(placed: &[(usize, MetadataItem)]) -> LocalContainer {
        let mut container = LocalContainer::new();
        for (index, stack) in placed {
            container.slots[*index] = Some(stack.clone());
        }
        container
    }

    #[test]
    fn the_module_constants_carry_the_source_literals() {
        assert_eq!(BASE_MAX_STACK_SIZE, 64, "Item.maxStackSize, Item.java:58");
        assert_eq!(CREATIVE_PANE_SLOTS, 45, "five rows of nine");
        assert_eq!(CREATIVE_SLOTS, 54, "the pane and the hotbar band");
        assert_eq!(
            [
                CLICK_MODE_PICKUP,
                CLICK_MODE_QUICK_MOVE,
                CLICK_MODE_SWAP,
                CLICK_MODE_CREATIVE_PICK,
                CLICK_MODE_DROP,
                CLICK_MODE_DRAG,
                CLICK_MODE_GATHER,
            ],
            [0, 1, 2, 3, 4, 5, 6],
            "the source's clickType table, Container.slotClick:140-494"
        );
    }

    #[test]
    fn the_drag_button_bits_read_the_source_tables() {
        // `getDragEvent` keeps the low two bits (`Container.java:695-698`).
        assert_eq!(get_drag_event(0), 0);
        assert_eq!(get_drag_event(3), 3);
        assert_eq!(get_drag_event(4), 0);
        assert_eq!(get_drag_event(7), 3);
        assert_eq!(get_drag_event(12), 0);
        assert_eq!(get_drag_event(-1), 3, "the mask reads the low bits alone");
        // `extractDragMode` shifts them out (`Container.java:687-690`).
        assert_eq!(extract_drag_mode(0), 0);
        assert_eq!(extract_drag_mode(4), 1);
        assert_eq!(extract_drag_mode(8), 2);
        assert_eq!(extract_drag_mode(12), 3);
        assert_eq!(extract_drag_mode(3), 0);
        // `func_94534_d` packs an event and a mode back together
        // (`Container.java:700-703`).
        assert_eq!(drag_button(0, 0), 0);
        assert_eq!(drag_button(1, 1), 5);
        assert_eq!(drag_button(2, 2), 10);
        for event in [0i32, 1, 2] {
            for mode in [0i32, 1, 2, 3] {
                let packed = drag_button(event, mode);
                assert_eq!(get_drag_event(packed), event, "event {event}, mode {mode}");
                assert_eq!(
                    extract_drag_mode(packed),
                    mode,
                    "event {event}, mode {mode}"
                );
            }
        }
    }

    #[test]
    fn drag_modes_validate_per_the_source_table() {
        // `isValidDragMode` (`Container.java:705-708`): the even split and the
        // single item are always valid; the whole-stack mode needs creative.
        assert!(is_valid_drag_mode(0, false));
        assert!(is_valid_drag_mode(0, true));
        assert!(is_valid_drag_mode(1, false));
        assert!(is_valid_drag_mode(1, true));
        assert!(
            !is_valid_drag_mode(2, false),
            "the full mode needs creative"
        );
        assert!(is_valid_drag_mode(2, true));
        assert!(
            !is_valid_drag_mode(3, true),
            "the fourth pair is not a mode"
        );
        assert!(!is_valid_drag_mode(-1, true));
    }

    #[test]
    fn can_add_item_to_slot_follows_the_stack_rules() {
        let stone = item(1, 63);
        assert!(
            can_add_item_to_slot(&None, &stone, 64),
            "an empty slot takes"
        );
        assert!(can_add_item_to_slot(&Some(item(1, 40)), &stone, 64));
        assert!(
            can_add_item_to_slot(&Some(item(1, 64)), &stone, 64),
            "the source tests the slot's own count, not the sum (`:728`)"
        );
        assert!(
            !can_add_item_to_slot(&Some(item(1, 65)), &stone, 64),
            "a count past the cap refuses"
        );
        assert!(
            !can_add_item_to_slot(&Some(item(2, 1)), &stone, 64),
            "another item refuses"
        );
        assert!(
            !can_add_item_to_slot(
                &Some(MetadataItem {
                    id: 1,
                    count: 1,
                    damage: 3,
                    nbt: None,
                }),
                &stone,
                64
            ),
            "another damage refuses"
        );
        assert!(
            !can_add_item_to_slot(&Some(tailed(1, 1, 0, 7)), &stone, 64),
            "an unequal tag refuses"
        );
        assert!(can_add_item_to_slot(
            &Some(tailed(1, 1, 0, 7)),
            &tailed(1, 1, 0, 7),
            64
        ));
        let single = item(276, 1);
        assert!(can_add_item_to_slot(&Some(item(276, 1)), &single, 1));
        assert!(!can_add_item_to_slot(&Some(item(276, 2)), &single, 1));
    }

    #[test]
    fn a_drag_splits_one_item_or_the_whole_stack_by_its_mode() {
        let caps = caps_of(&[]);
        let drag = item(1, 64);
        // Mode 0: an even split of the dragged stack across the set, floored
        // (`Container.java:742-744`).
        assert_eq!(compute_stack_size(0, &drag, None, &empties(3), &caps), 21);
        assert_eq!(
            compute_stack_size(0, &drag, Some(&item(1, 10)), &empties(3), &caps),
            31
        );
        assert_eq!(compute_stack_size(0, &drag, None, &empties(5), &caps), 12);
        assert_eq!(
            compute_stack_size(0, &item(1, 1), None, &empties(3), &caps),
            0,
            "a stack shorter than the set splits to nothing a slot (`:744`)"
        );
        assert_eq!(
            compute_stack_size(0, &drag, Some(&item(1, 64)), &empties(3), &caps),
            64,
            "a full slot stays at its cap"
        );
        // Mode 1: one item a slot (`:746-748`).
        assert_eq!(compute_stack_size(1, &drag, None, &empties(3), &caps), 1);
        assert_eq!(
            compute_stack_size(1, &drag, Some(&item(1, 3)), &empties(3), &caps),
            4
        );
        // Mode 2: the whole stack, capped (`:750-751`, `:196-198`).
        assert_eq!(compute_stack_size(2, &drag, None, &empties(3), &caps), 64);
        assert_eq!(
            compute_stack_size(2, &drag, Some(&item(1, 5)), &empties(3), &caps),
            64
        );
        // The dragged item's own cap binds (`ItemStack.getMaxStackSize:234-236`).
        let sized = caps_of(&[2]);
        assert_eq!(
            compute_stack_size(2, &item(2, 64), None, &empties(1), &sized),
            1
        );
        assert_eq!(
            compute_stack_size(1, &item(2, 64), None, &empties(1), &sized),
            1
        );
        assert_eq!(
            compute_stack_size(2, &item(2, 64), Some(&item(2, 5)), &empties(1), &sized),
            1,
            "the cap replaces a stack past it, as the source's own cap does"
        );
        // An unknown mode leaves the dragged stack its own size: the source's
        // switch has no default (`:740-752`).
        assert_eq!(
            compute_stack_size(7, &item(1, 3), Some(&item(1, 2)), &empties(1), &caps),
            5
        );
        assert_eq!(
            compute_stack_size(0, &drag, Some(&item(1, 2)), &[], &caps),
            2,
            "an empty set never reaches the split; the slot keeps its count"
        );
    }

    #[test]
    fn a_drag_over_the_container_splits_evenly_and_leaves_nothing() {
        let caps = caps_of(&[]);
        let mut container = LocalContainer::new();
        container.cursor = Some(item(1, 8));
        // The start event names no slot: the source sends slotId −999
        // (`GuiContainer.mouseReleased:617`).
        container.slot_click(-999, 0, 5, &caps);
        assert_eq!(container.drag_event, 1, "the drag is open");
        assert_eq!(container.drag_mode, 0, "the left button's even split");
        container.slot_click(45, 1, 5, &caps);
        container.slot_click(46, 1, 5, &caps);
        assert_eq!(container.drag_slots, vec![45, 46]);
        container.slot_click(45, 1, 5, &caps);
        assert_eq!(
            container.drag_slots,
            vec![45, 46],
            "a slot joins once (`:28`)"
        );
        // A click of another mode mid-drag resets the drag and lands nowhere
        // (`Container.java:228-231`).
        let between = container.slot_click(3, 0, 0, &caps);
        assert!(between.is_none());
        assert_eq!(container.drag_event, 0, "the drag was dropped");
        assert!(
            container.slots[3].is_none(),
            "the reset click landed nowhere"
        );
        // A do-over: two slots, an even split of eight, nothing left over.
        container.slot_click(-999, 0, 5, &caps);
        container.slot_click(45, 1, 5, &caps);
        container.slot_click(46, 1, 5, &caps);
        container.slot_click(-999, 2, 5, &caps);
        assert_eq!(container.slots[45].as_ref().map(|s| s.count), Some(4));
        assert_eq!(container.slots[46].as_ref().map(|s| s.count), Some(4));
        assert!(container.cursor.is_none(), "eight over two leaves nothing");
        assert_eq!(container.drag_event, 0, "the end closes the drag");
    }

    #[test]
    fn a_single_drag_leaves_the_remnant_on_the_cursor() {
        let caps = caps_of(&[]);
        let mut container = LocalContainer::new();
        container.cursor = Some(item(1, 3));
        container.slot_click(-999, 4, 5, &caps);
        assert_eq!(container.drag_mode, 1, "the right button's single mode");
        container.slot_click(45, 5, 5, &caps);
        container.slot_click(46, 5, 5, &caps);
        container.slot_click(-999, 6, 5, &caps);
        assert_eq!(container.slots[45].as_ref().map(|s| s.count), Some(1));
        assert_eq!(container.slots[46].as_ref().map(|s| s.count), Some(1));
        assert_eq!(
            container.cursor.as_ref().map(|s| s.count),
            Some(1),
            "three over two leaves one"
        );
    }

    #[test]
    fn a_drag_refuses_a_slot_the_cursor_cannot_cover() {
        let caps = caps_of(&[]);
        let mut container = LocalContainer::new();
        container.cursor = Some(item(1, 1));
        container.slot_click(-999, 4, 5, &caps);
        container.slot_click(45, 5, 5, &caps);
        container.slot_click(46, 5, 5, &caps);
        assert_eq!(
            container.drag_slots,
            vec![45],
            "the set may not outgrow the stack (`Container.java:176`)"
        );
    }

    #[test]
    fn a_drag_over_a_pane_slot_never_joins() {
        let caps = caps_of(&[]);
        let mut container = LocalContainer::new();
        container.cursor = Some(item(1, 5));
        container.slot_click(-999, 0, 5, &caps);
        container.slot_click(3, 1, 5, &caps);
        assert!(
            container.drag_slots.is_empty(),
            "the creative drag bound holds the hotbar band (`:938-941`)"
        );
    }

    #[test]
    fn a_full_drag_fills_a_slot_to_the_cap_and_keeps_the_rest() {
        let caps = caps_of(&[]);
        let mut container = local(&[(45, item(1, 5))]);
        container.cursor = Some(item(1, 64));
        container.slot_click(-999, 8, 5, &caps);
        assert_eq!(container.drag_mode, 2, "the middle button's full mode");
        container.slot_click(45, 9, 5, &caps);
        container.slot_click(-999, 10, 5, &caps);
        assert_eq!(
            container.slots[45].as_ref().map(|s| s.count),
            Some(64),
            "the cap folds into the slot's size"
        );
        assert_eq!(
            container.cursor.as_ref().map(|s| s.count),
            Some(5),
            "the slot's own five ride back onto the cursor"
        );
    }

    #[test]
    fn the_gather_tops_the_cursor_up_from_the_hotbar_alone() {
        let caps = caps_of(&[]);
        let mut container = local(&[
            (3, item(1, 5)),
            (45, item(1, 5)),
            (46, item(1, 64)),
            (47, item(2, 3)),
        ]);
        container.cursor = Some(item(1, 10));
        let echo = container.slot_click(0, 0, 6, &caps);
        assert!(
            echo.is_none(),
            "the gather leaves the carrier null (`:457-491`)"
        );
        assert!(
            container.slots[3].is_some(),
            "a pane slot is no merge target (`canMergeSlot:933-936`)"
        );
        assert!(container.slots[45].is_none(), "the first stack is spent");
        assert_eq!(
            container.slots[46].as_ref().map(|s| s.count),
            Some(15),
            "the full slot gives its share on the second pass (`:467-473`)"
        );
        assert_eq!(
            container.cursor.as_ref().map(|s| s.count),
            Some(64),
            "the cursor tops out at its cap"
        );
        assert_eq!(container.slots[47].as_ref().map(|s| s.id), Some(2));
    }

    #[test]
    fn the_gather_walks_from_the_side_the_button_names() {
        let caps = caps_of(&[]);
        // Left: the walk starts at the first slot.
        let mut container = local(&[(45, item(1, 5)), (46, item(1, 5))]);
        container.cursor = Some(item(1, 60));
        container.slot_click(0, 0, 6, &caps);
        assert_eq!(container.slots[45].as_ref().map(|s| s.count), Some(1));
        assert_eq!(container.slots[46].as_ref().map(|s| s.count), Some(5));
        assert_eq!(container.cursor.as_ref().map(|s| s.count), Some(64));
        // Right: the walk starts at the last one.
        let mut container = local(&[(45, item(1, 5)), (46, item(1, 5))]);
        container.cursor = Some(item(1, 60));
        container.slot_click(0, 1, 6, &caps);
        assert_eq!(container.slots[45].as_ref().map(|s| s.count), Some(5));
        assert_eq!(container.slots[46].as_ref().map(|s| s.count), Some(1));
        assert_eq!(container.cursor.as_ref().map(|s| s.count), Some(64));
        // A filled slot under the double click gathers nothing (`:462`).
        let mut container = local(&[(45, item(1, 5))]);
        container.cursor = Some(item(1, 10));
        container.slot_click(45, 0, 6, &caps);
        assert_eq!(container.cursor.as_ref().map(|s| s.count), Some(10));
        assert_eq!(container.slots[45].as_ref().map(|s| s.count), Some(5));
    }

    #[test]
    fn a_click_places_picks_merges_and_swaps() {
        let caps = caps_of(&[]);
        // Place: the cursor empties into an empty slot, whole on the left,
        // one item on the right (`Container.java:299-319`).
        let mut container = LocalContainer::new();
        container.cursor = Some(item(1, 3));
        let echo = container.slot_click(10, 0, 0, &caps);
        assert!(echo.is_none(), "an empty slot echoes nothing (`:291-296`)");
        assert_eq!(container.slots[10].as_ref().map(|s| s.count), Some(3));
        assert!(container.cursor.is_none());
        let mut container = LocalContainer::new();
        container.cursor = Some(item(1, 3));
        container.slot_click(10, 1, 0, &caps);
        assert_eq!(container.slots[10].as_ref().map(|s| s.count), Some(1));
        assert_eq!(container.cursor.as_ref().map(|s| s.count), Some(2));

        // Pick: the slot empties onto the cursor, whole on the left, half on
        // the right (`:323-334`).
        let mut container = local(&[(10, item(1, 5))]);
        let echo = container.slot_click(10, 0, 0, &caps);
        assert_eq!(
            echo.as_ref().map(|s| (s.id, s.count)),
            Some((1, 5)),
            "the pre-click stack rides back (`:291-296`)"
        );
        assert!(container.slots[10].is_none());
        assert_eq!(container.cursor.as_ref().map(|s| s.count), Some(5));
        let mut container = local(&[(10, item(1, 5))]);
        container.slot_click(10, 1, 0, &caps);
        assert_eq!(
            container.cursor.as_ref().map(|s| s.count),
            Some(3),
            "(5 + 1) / 2 (`:326`)"
        );
        assert_eq!(container.slots[10].as_ref().map(|s| s.count), Some(2));

        // Merge: the cursor tops the slot up to the caps (`:336-360`).
        let mut container = local(&[(10, item(1, 60))]);
        container.cursor = Some(item(1, 10));
        container.slot_click(10, 0, 0, &caps);
        assert_eq!(container.slots[10].as_ref().map(|s| s.count), Some(64));
        assert_eq!(
            container.cursor.as_ref().map(|s| s.count),
            Some(6),
            "the cap keeps the rest on the cursor"
        );

        // Swap: another item exchanges, the slot's stack riding onto the
        // cursor (`:361-365`).
        let mut container = local(&[(10, item(1, 5))]);
        container.cursor = Some(item(2, 3));
        container.slot_click(10, 0, 0, &caps);
        assert_eq!(container.slots[10].as_ref().map(|s| s.id), Some(2));
        assert_eq!(
            container.cursor.as_ref().map(|s| (s.id, s.count)),
            Some((1, 5))
        );

        // A slot already past a cap-one item's limit: the merge refuses to
        // grow the cursor past nothing (the source's own subtraction would run
        // negative here; this port clamps at zero).
        let sized = caps_of(&[1]);
        let mut container = local(&[(10, item(1, 5))]);
        container.cursor = Some(item(1, 2));
        container.slot_click(10, 0, 0, &sized);
        assert_eq!(container.cursor.as_ref().map(|s| s.count), Some(2));
        assert_eq!(container.slots[10].as_ref().map(|s| s.count), Some(5));
    }

    #[test]
    fn a_number_key_swaps_the_clicked_slot_with_the_hotbar() {
        let caps = caps_of(&[]);
        // A pane slot clicked: its stack moves to the hotbar, the hotbar's old
        // stack to the first free player slot, the pane slot cleared
        // (`Container.java:406-420`).
        let mut container = local(&[(10, item(1, 3)), (45, item(2, 2))]);
        container.slot_click(10, 0, 2, &caps);
        assert_eq!(
            container.slots[45].as_ref().map(|s| (s.id, s.count)),
            Some((1, 3))
        );
        assert!(container.slots[10].is_none(), "the pane slot empties");
        assert_eq!(
            container.slots[46].as_ref().map(|s| (s.id, s.count)),
            Some((2, 2)),
            "the displaced stack takes the first free player slot"
        );

        // A hotbar slot clicked: the two stacks exchange (`:421-426`).
        let mut container = local(&[(45, item(1, 3)), (48, item(2, 2))]);
        container.slot_click(45, 3, 2, &caps);
        assert_eq!(
            container.slots[45].as_ref().map(|s| (s.id, s.count)),
            Some((2, 2))
        );
        assert_eq!(
            container.slots[48].as_ref().map(|s| (s.id, s.count)),
            Some((1, 3))
        );

        // An empty clicked slot takes the hotbar's stack, the hotbar clearing
        // (`:428-432`).
        let mut container = local(&[(45, item(2, 2))]);
        container.slot_click(10, 0, 2, &caps);
        assert_eq!(
            container.slots[10].as_ref().map(|s| (s.id, s.count)),
            Some((2, 2))
        );
        assert!(
            container.slots[45].is_none(),
            "the number key's slot empties"
        );
    }

    #[test]
    fn the_creative_pick_takes_the_whole_stack() {
        let caps = caps_of(&[]);
        let mut container = local(&[(10, item(5, 3))]);
        container.slot_click(10, 0, 3, &caps);
        assert_eq!(
            container.cursor.as_ref().map(|s| (s.id, s.count)),
            Some((5, 64)),
            "picked to the cap (`Container.java:441-443`)"
        );
        // The item's own cap binds, and nothing picks while a stack is
        // carried (`:435`).
        let sized = caps_of(&[5]);
        let mut container = local(&[(10, item(5, 3))]);
        container.slot_click(10, 0, 3, &sized);
        assert_eq!(container.cursor.as_ref().map(|s| s.count), Some(1));
        let mut container = local(&[(10, item(5, 3))]);
        container.cursor = Some(item(9, 1));
        container.slot_click(10, 0, 3, &caps);
        assert_eq!(
            container.cursor.as_ref().map(|s| s.id),
            Some(9),
            "the carried stack stays"
        );
    }

    #[test]
    fn the_drop_takes_one_item_or_the_whole_stack() {
        let caps = caps_of(&[]);
        let mut container = local(&[(10, item(1, 5))]);
        container.slot_click(10, 0, 4, &caps);
        assert_eq!(container.slots[10].as_ref().map(|s| s.count), Some(4));
        container.slot_click(10, 1, 4, &caps);
        assert!(
            container.slots[10].is_none(),
            "the right button drops the rest (`:450-452`)"
        );
        // Nothing drops while a stack is carried (`:446`).
        let mut container = local(&[(10, item(1, 5))]);
        container.cursor = Some(item(9, 1));
        container.slot_click(10, 0, 4, &caps);
        assert_eq!(container.slots[10].as_ref().map(|s| s.count), Some(5));
    }

    #[test]
    fn a_shift_click_clears_a_hotbar_slot_and_never_echoes() {
        let caps = caps_of(&[]);
        let mut container = local(&[(10, item(1, 3)), (50, item(2, 4))]);
        let echo = container.slot_click(50, 0, 1, &caps);
        assert!(
            echo.is_none(),
            "the creative transfer answers nothing (`:918-931`)"
        );
        assert!(
            container.slots[50].is_none(),
            "the hotbar slot's stack is deleted"
        );
        let echo = container.slot_click(10, 0, 1, &caps);
        assert!(echo.is_none());
        assert_eq!(
            container.slots[10].as_ref().map(|s| s.count),
            Some(3),
            "a pane slot is left alone"
        );
    }
}
