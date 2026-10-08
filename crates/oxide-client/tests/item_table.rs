//! The item registry table's completeness and pin suite.
//!
//! The table's row count is the source's own registration count, derived by
//! counting the registrations of `Item.registerItems` (`item/Item.java`:511-953)
//! — 150 block items and 187 explicit registrations (the derivation and its log
//! sit in `refs/m5-task-8/`). The pins are literal values from the source, so a
//! shifted, drifted or partly dropped table fails loudly.

use oxide_assets::model::{BuiltinItem, ItemModelSource};
use oxide_client::items::{ItemAttributes, ItemEntry, ItemModel, ItemTable, item_entry, registry};
use oxide_game::container::{BASE_MAX_STACK_SIZE, BaseStackCaps, StackCaps, max_stack_size};
use oxide_proto_v47::entity::MetadataItem;

/// The derived registration count: 150 block items + 187 explicit items.
const REGISTRATION_COUNT: usize = 337;

fn entry(id: i16) -> &'static ItemEntry {
    item_entry(id).unwrap_or_else(|| panic!("id {id} must be in the registry table"))
}

fn stack(id: i16) -> MetadataItem {
    MetadataItem {
        id,
        count: 1,
        damage: 0,
        nbt: None,
    }
}

#[test]
fn the_table_carries_every_registration_once() {
    let rows = registry();
    assert_eq!(rows.len(), REGISTRATION_COUNT, "the registry's row count");
    for pair in rows.windows(2) {
        assert!(
            pair[0].id < pair[1].id,
            "ids ascend strictly: {} then {}",
            pair[0].id,
            pair[1].id
        );
    }
    for row in rows {
        assert_eq!(item_entry(row.id).map(|entry| entry.id), Some(row.id));
    }
    for id in [-1, 0, 255, 500, 1000, 2255, 2268, 30_000] {
        assert_eq!(item_entry(id), None, "id {id} is no registration");
    }
}

#[test]
fn the_pins_name_the_sources_own_strings() {
    for (id, name) in [
        (1, "Stone"),
        (5, "Oak Wood Planks"),
        (35, "Wool"),
        (54, "Chest"),
        (257, "Iron Pickaxe"),
        (260, "Apple"),
        (263, "Coal"),
        (267, "Iron Sword"),
        (276, "Diamond Sword"),
        (290, "Wooden Hoe"),
        (297, "Bread"),
        (298, "Leather Cap"),
        (311, "Diamond Chestplate"),
        (325, "Bucket"),
        (326, "Water Bucket"),
        (331, "Redstone"),
        (345, "Compass"),
        (349, "Raw Fish"),
        (354, "Cake"),
        (355, "Bed"),
        (373, "Water Bottle"),
        (383, "Spawn"),
        (397, "Skeleton Skull"),
        (404, "Redstone Comparator"),
        (425, "Black Banner"),
        (2267, "Music Disc"),
    ] {
        assert_eq!(entry(id).name, name, "id {id}'s display name");
    }
}

#[test]
fn the_stack_caps_are_the_sources_own() {
    for (id, cap) in [
        (1, 64),   // a block item keeps the base rule
        (261, 1),  // bow
        (267, 1),  // iron sword
        (283, 1),  // golden sword
        (290, 1),  // wooden hoe
        (293, 1),  // diamond hoe
        (322, 64), // golden apple
        (325, 16), // bucket, the registration-site cap
        (326, 1),  // water bucket
        (332, 16), // snowball
        (344, 16), // egg
        (354, 1),  // cake, the site override over the base rule
        (355, 1),  // bed, the site override
        (368, 16), // ender pearl
        (373, 1),  // potion
        (387, 16), // written book, the site override over the class's 1
        (403, 1),  // enchanted book
        (416, 16), // armor stand, the site override
        (417, 1),  // iron horse armor
        (425, 16), // banner
    ] {
        assert_eq!(entry(id).max_stack, cap, "id {id}'s stack cap");
    }
}

#[test]
fn the_use_counts_are_the_sources_own() {
    for (id, uses) in [
        (1, 0),      // a block item never wears
        (259, 64),   // flint and steel
        (260, 0),    // an apple never wears either
        (261, 384),  // bow
        (267, 250),  // iron sword
        (268, 59),   // wooden sword
        (272, 131),  // stone sword
        (276, 1561), // diamond sword
        (283, 32),   // golden sword
        (290, 59),   // wooden hoe
        (298, 55),   // leather cap (5 × 11)
        (311, 528),  // diamond chestplate (33 × 16)
        (317, 91),   // golden boots (7 × 13)
        (346, 64),   // fishing rod
        (359, 238),  // shears
        (398, 25),   // carrot on a stick
    ] {
        assert_eq!(entry(id).max_damage, uses, "id {id}'s use count");
    }
}

#[test]
fn the_sword_damage_literals_are_the_sources_own() {
    for (id, damage) in [(268, 4.0), (272, 5.0), (267, 6.0), (276, 7.0), (283, 4.0)] {
        let attributes = entry(id).attributes;
        assert_eq!(
            attributes.attack_damage,
            Some(damage),
            "id {id}'s attack damage"
        );
        assert_eq!(
            attributes.attack_speed, None,
            "1.8 states no speed: id {id}"
        );
        assert_eq!(attributes.armour_points, None, "id {id} is no armour");
        assert_eq!(attributes.armour_toughness, None);
        assert_eq!(
            attributes.tooltip_inputs(),
            Some(("generic.attackDamage", damage)),
            "id {id}'s composed tooltip line"
        );
    }
}

#[test]
fn the_pickaxe_damage_literals_are_the_sources_own() {
    for (id, damage) in [(270, 2.0), (274, 3.0), (257, 4.0), (278, 5.0), (285, 2.0)] {
        let attributes = entry(id).attributes;
        assert_eq!(
            attributes.attack_damage,
            Some(damage),
            "id {id}'s attack damage"
        );
        assert_eq!(
            attributes.tooltip_inputs(),
            Some(("generic.attackDamage", damage)),
            "id {id}'s composed tooltip line"
        );
    }
}

#[test]
fn the_armour_values_are_the_sources_own() {
    for (id, points) in [(298, 1.0), (303, 5.0), (307, 6.0), (311, 8.0), (317, 1.0)] {
        let attributes = entry(id).attributes;
        assert_eq!(
            attributes.armour_points,
            Some(points),
            "id {id}'s armour value"
        );
        assert_eq!(attributes.armour_toughness, None, "1.8 has no toughness");
        assert_eq!(attributes.attack_damage, None, "armour states no damage");
        assert_eq!(
            attributes.tooltip_inputs(),
            None,
            "1.8 composes no attribute line from armour: id {id}"
        );
    }
}

#[test]
fn a_plain_item_carries_no_attributes() {
    let attributes = entry(260).attributes;
    assert_eq!(attributes, ItemAttributes::NONE);
    assert_eq!(attributes.tooltip_inputs(), None);
}

#[test]
fn the_variant_flags_mark_the_sub_item_classes() {
    for id in [
        1, 5, 31, 35, 38, 44, 95, 97, 98, 126, 145, 155, 161, 162, 175, 263, 322, 349, 350, 351,
        373, 383, 397, 425,
    ] {
        assert!(entry(id).variants, "id {id} populates sub-items");
    }
    for id in [
        2, 4, 20, 50, 54, 78, 151, 182, 260, 262, 267, 276, 280, 297, 325,
    ] {
        assert!(!entry(id).variants, "id {id} has a single stack");
    }
    assert_eq!(
        registry().iter().filter(|row| row.variants).count(),
        37,
        "the derived variant count"
    );
}

#[test]
fn the_resolution_classes_pin_the_sources_own() {
    let rows = registry();
    let count =
        |kind: fn(&ItemModel) -> bool| rows.iter().filter(|row| kind(&row.resolution)).count();
    assert_eq!(count(|model| matches!(model, ItemModel::Block(_))), 122);
    assert_eq!(count(|model| matches!(model, ItemModel::Generated(_))), 208);
    assert_eq!(count(|model| matches!(model, ItemModel::Builtin(_))), 3);
    assert_eq!(count(|model| matches!(model, ItemModel::Missing)), 4);
    for row in rows {
        if let ItemModel::Generated(layers) = row.resolution {
            assert!(!layers.is_empty(), "id {} has layers", row.id);
        }
    }
}

#[test]
fn the_resolutions_map_into_task_sevens_classes() {
    assert_eq!(entry(1).resolution, ItemModel::Block("block/stone"));
    assert_eq!(entry(5).resolution, ItemModel::Block("block/oak_planks"));
    assert_eq!(entry(54).resolution, ItemModel::Builtin(BuiltinItem::Chest));
    assert_eq!(
        entry(130).resolution,
        ItemModel::Builtin(BuiltinItem::EnderChest)
    );
    assert_eq!(
        entry(146).resolution,
        ItemModel::Builtin(BuiltinItem::TrappedChest)
    );
    assert_eq!(
        entry(260).resolution,
        ItemModel::Generated(&["items/apple"])
    );
    assert_eq!(
        entry(261).resolution,
        ItemModel::Generated(&["items/bow_standby"])
    );
    assert_eq!(
        entry(298).resolution,
        ItemModel::Generated(&["items/leather_helmet", "items/leather_helmet_overlay"])
    );
    assert_eq!(
        entry(373).resolution,
        ItemModel::Generated(&["items/potion_overlay", "items/potion_bottle_drinkable"])
    );
    assert_eq!(entry(345).resolution, ItemModel::Missing);
    assert_eq!(
        entry(1).resolution.source(),
        ItemModelSource::Block("block/stone".to_string())
    );
    assert_eq!(
        entry(54).resolution.source(),
        ItemModelSource::Builtin(BuiltinItem::Chest)
    );
    assert_eq!(
        entry(373).resolution.source(),
        ItemModelSource::Generated(vec![
            "items/potion_overlay".to_string(),
            "items/potion_bottle_drinkable".to_string(),
        ])
    );
    assert_eq!(entry(345).resolution.source(), ItemModelSource::Missing);
}

#[test]
fn the_cap_bridge_reads_the_table() {
    assert_eq!(max_stack_size(&stack(267), &ItemTable), 1);
    assert_eq!(max_stack_size(&stack(373), &ItemTable), 1);
    assert_eq!(max_stack_size(&stack(325), &ItemTable), 16);
    assert_eq!(max_stack_size(&stack(260), &ItemTable), 64);
    assert_eq!(max_stack_size(&stack(255), &ItemTable), BASE_MAX_STACK_SIZE);
    assert_eq!(
        max_stack_size(&stack(260), &BaseStackCaps),
        BASE_MAX_STACK_SIZE
    );
    assert_eq!(ItemTable.max_stack_size(&stack(276)), 1);
}
