//! The inventory model's suite: the window-0 slot mapping in both directions,
//! the accessors and the respawn clear. Every expected value is hand
//! arithmetic on the registration order `ContainerPlayer` fixes and
//! `InventoryPlayer`'s own reads, never rebuilt with the code under test.

use oxide_proto_v47::entity::MetadataItem;
use oxide_world::inventory::Inventory;

/// One stack from a literal id and count.
fn item(id: i16, count: u8) -> Option<MetadataItem> {
    Some(MetadataItem {
        id,
        count,
        damage: 0,
        nbt: None,
    })
}

/// The window-0 layout as `ContainerPlayer`'s registration order fixes it
/// (`ContainerPlayer.java:26-67`): the result and the four matrix slots at
/// 0–4, the armour band 5–8 descending from the helmet, the 27 main slots at
/// 9–35 in their own order, and the hotbar at 36–44. Each pair is the window
/// index and the id of the marker the model slot it maps to carries.
const WINDOW_0_TABLE: [(usize, i16); 40] = [
    (5, 203),
    (6, 202),
    (7, 201),
    (8, 200),
    (9, 109),
    (10, 110),
    (11, 111),
    (12, 112),
    (13, 113),
    (14, 114),
    (15, 115),
    (16, 116),
    (17, 117),
    (18, 118),
    (19, 119),
    (20, 120),
    (21, 121),
    (22, 122),
    (23, 123),
    (24, 124),
    (25, 125),
    (26, 126),
    (27, 127),
    (28, 128),
    (29, 129),
    (30, 130),
    (31, 131),
    (32, 132),
    (33, 133),
    (34, 134),
    (35, 135),
    (36, 100),
    (37, 101),
    (38, 102),
    (39, 103),
    (40, 104),
    (41, 105),
    (42, 106),
    (43, 107),
    (44, 108),
];

#[test]
fn every_window_index_maps_as_the_registration_order_fixes_it() {
    let mut inventory = Inventory::default();
    for (index, slot) in inventory.main.iter_mut().enumerate() {
        *slot = item(100 + index as i16, 1);
    }
    for (index, slot) in inventory.armor.iter_mut().enumerate() {
        *slot = item(200 + index as i16, 1);
    }

    // The crafting result and the four matrix slots are not inventory state.
    for index in 0..=4usize {
        assert_eq!(inventory.window_slot(index), None, "window {index}");
    }
    // Every mapped index 5–44 sees the marker of the slot it names.
    for (index, id) in WINDOW_0_TABLE {
        let seen = inventory.window_slot(index).and_then(|slot| slot.as_ref());
        assert_eq!(seen.map(|stack| stack.id), Some(id), "window {index}");
    }
    // Past 44 there is no slot.
    for index in [45usize, 46, 90, usize::MAX] {
        assert_eq!(inventory.window_slot(index), None, "window {index}");
    }
}

#[test]
fn set_window_slot_writes_both_directions_and_refuses_the_unmapped_band() {
    let mut inventory = Inventory::default();

    // The two named edges: window 44 is the last hotbar slot and window 9 the
    // first main slot.
    assert!(inventory.set_window_slot(44, item(1, 1)));
    assert_eq!(inventory.main[8], item(1, 1));
    assert!(inventory.set_window_slot(9, item(2, 1)));
    assert_eq!(inventory.main[9], item(2, 1));
    // The armour band descends: 5 is the helmet (armour[3]) and 8 the boots.
    assert!(inventory.set_window_slot(5, item(3, 1)));
    assert_eq!(inventory.armor[3], item(3, 1));
    assert!(inventory.set_window_slot(8, item(4, 1)));
    assert_eq!(inventory.armor[0], item(4, 1));
    // Window 36 is the first hotbar slot.
    assert!(inventory.set_window_slot(36, item(5, 1)));
    assert_eq!(inventory.main[0], item(5, 1));

    // The crafting band and every index outside 5–44 refuse without touching
    // the model; the wire's slot index is an i16 and a hostile one indexes
    // nothing.
    for slot in [0i16, 1, 2, 3, 4, 45, 46, 100, -1, -100, i16::MIN, i16::MAX] {
        let before = inventory.clone();
        assert!(!inventory.set_window_slot(slot, item(9, 1)), "slot {slot}");
        assert_eq!(inventory, before, "slot {slot}");
    }

    // An empty write clears the slot it maps to.
    assert!(inventory.set_window_slot(44, None));
    assert_eq!(inventory.main[8], None);
}

#[test]
fn a_fully_populated_inventory_projects_into_window_0_and_sets_back() {
    let mut inventory = Inventory::default();
    for (index, slot) in inventory.main.iter_mut().enumerate() {
        *slot = item(100 + index as i16, index as u8 + 1);
    }
    for (index, slot) in inventory.armor.iter_mut().enumerate() {
        *slot = item(200 + index as i16, index as u8 + 1);
    }
    inventory.cursor = item(300, 5);
    inventory.selected = 3;

    // The window-0 vector, hand-written: the result and the matrix empty, the
    // armour band descending, the main band in order, the hotbar last.
    let mut expected: Vec<Option<MetadataItem>> = vec![None; 5];
    expected.push(item(203, 4));
    expected.push(item(202, 3));
    expected.push(item(201, 2));
    expected.push(item(200, 1));
    for index in 9..36usize {
        expected.push(item(100 + index as i16, index as u8 + 1));
    }
    for index in 0..9usize {
        expected.push(item(100 + index as i16, index as u8 + 1));
    }
    assert_eq!(expected.len(), 45);

    let projected: Vec<Option<MetadataItem>> = (0..45usize)
        .map(|index| inventory.window_slot(index).cloned().unwrap_or(None))
        .collect();
    assert_eq!(projected, expected);

    // The same vector written back through the mapping rebuilds the model.
    // The cursor and the selection are not in the window mapping and stay as
    // the fresh model has them.
    let mut rebuilt = Inventory::default();
    for (index, stack) in projected.iter().enumerate() {
        assert_eq!(
            rebuilt.set_window_slot(index as i16, stack.clone()),
            index >= 5,
            "window {index}"
        );
    }
    assert_eq!(rebuilt.main, inventory.main);
    assert_eq!(rebuilt.armor, inventory.armor);
    assert_eq!(rebuilt.cursor, None);
    assert_eq!(rebuilt.selected, 0);
}

#[test]
fn a_fresh_inventory_is_empty_with_the_first_hotbar_slot_selected() {
    // The crate root re-exports the model beside its module.
    let inventory: oxide_world::Inventory = Inventory::default();
    assert!(inventory.main.iter().all(Option::is_none));
    assert!(inventory.armor.iter().all(Option::is_none));
    assert_eq!(inventory.cursor, None);
    assert_eq!(inventory.selected, 0);
    assert!(inventory.get_current_item().is_none());
}

#[test]
fn get_current_item_reads_the_selected_hotbar_slot_and_guards_the_range() {
    let mut inventory = Inventory::default();
    for (index, slot) in inventory.main.iter_mut().enumerate() {
        *slot = item(100 + index as i16, 1);
    }

    // Each selection 0–8 reads its own hotbar slot.
    for selected in 0..9i16 {
        inventory.set_selected(selected);
        assert_eq!(
            inventory.get_current_item(),
            &item(100 + selected, 1),
            "selected {selected}"
        );
    }

    // The source's read guard: outside 0–8 the held item is the empty stack
    // (`InventoryPlayer.getCurrentItem:50-53`).
    for selected in [-1i16, 9, 100, i16::MIN, i16::MAX] {
        inventory.set_selected(selected);
        assert_eq!(
            inventory.get_current_item(),
            &None::<MetadataItem>,
            "selected {selected}"
        );
    }
}

#[test]
fn change_current_item_steps_one_slot_per_event_and_wraps_mod_nine() {
    let mut inventory = Inventory::default();

    // The source's argument convention (`InventoryPlayer.java:162-163`): a
    // positive direction steps back one slot, a negative one forward.
    inventory.set_selected(0);
    assert!(inventory.change_current_item(1));
    assert_eq!(inventory.selected, 8);
    assert!(inventory.change_current_item(-1));
    assert_eq!(inventory.selected, 0);

    // The magnitude clamps to one step: ±N moves exactly one slot
    // (`InventoryPlayer.java:167-175`).
    assert!(inventory.change_current_item(5));
    assert_eq!(inventory.selected, 8);
    assert!(inventory.change_current_item(-100));
    assert_eq!(inventory.selected, 0);
    assert!(inventory.change_current_item(i32::MIN));
    assert_eq!(inventory.selected, 1);
    assert!(inventory.change_current_item(i32::MAX));
    assert_eq!(inventory.selected, 0);

    // Nine steps walk the whole bar and land back where they started.
    inventory.set_selected(0);
    let mut walked = Vec::new();
    for _ in 0..9 {
        inventory.change_current_item(1);
        walked.push(inventory.selected);
    }
    assert_eq!(walked, vec![8, 7, 6, 5, 4, 3, 2, 1, 0]);

    // A zero direction has no effect (`InventoryPlayer.java:163`).
    inventory.set_selected(3);
    assert!(!inventory.change_current_item(0));
    assert_eq!(inventory.selected, 3);
}

#[test]
fn clear_for_respawn_drops_the_cursor_and_keeps_the_arrays() {
    let mut inventory = Inventory::default();
    inventory.main[0] = item(1, 1);
    inventory.main[35] = item(2, 2);
    inventory.armor[3] = item(3, 1);
    inventory.selected = 4;
    inventory.cursor = item(4, 64);

    // The source's Respawned edge replaces the client player and its whole
    // inventory, so the cursor is dropped; the server re-sends window 0, so
    // the arrays stay until it lands.
    assert_eq!(inventory.clear_for_respawn(), item(4, 64));
    assert_eq!(inventory.cursor, None);
    assert_eq!(inventory.main[0], item(1, 1));
    assert_eq!(inventory.main[35], item(2, 2));
    assert_eq!(inventory.armor[3], item(3, 1));
    assert_eq!(inventory.selected, 4);

    // Nothing on the cursor: nothing to return.
    assert_eq!(inventory.clear_for_respawn(), None);
}
