//! The item registry table's completeness and pin suite, and Task 9's sub-item and
//! creative-list suites.
//!
//! The table's row count is the source's own registration count, derived by
//! counting the registrations of `Item.registerItems` (`item/Item.java`:511-953)
//! — 150 block items and 187 explicit registrations (the derivation and its log
//! sit in `refs/m5-task-8/`). The pins are literal values from the source, so a
//! shifted, drifted or partly dropped table fails loudly.
//!
//! The sub-item suite pins the damage stacks each class populates (wool's sixteen
//! colours, the potion set's boundaries, the spawn eggs against the M4 roster) and
//! the strings they compose; the tab suite pins every tab's own index, icon and
//! sheet, two tabs' literal first ten entries, the search list and the search
//! filter's rule (the derivation and its log sit in `refs/m5-task-9/`).

use oxide_assets::model::{BuiltinItem, ItemModelSource};
use oxide_client::items::{
    CreativeTab, ItemAttributes, ItemEntry, ItemModel, ItemTable, TabEntry, creative_tab_items,
    item_entry, potion_name, registry, search_matches, stack_name, sub_items,
};
use oxide_game::container::{BASE_MAX_STACK_SIZE, BaseStackCaps, StackCaps, max_stack_size};
use oxide_proto_v47::entity::{MetadataItem, MobType};

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

// ------------------------------------------------------------- the sub-items

/// The sub-items of a row, panicking when the row carries none.
fn subs(id: i16) -> &'static [oxide_client::items::SubItem] {
    let items = sub_items(id);
    assert!(!items.is_empty(), "id {id} must populate sub-items");
    items
}

/// The (damage, name) pairs of a row.
fn pairs(id: i16) -> Vec<(i16, &'static str)> {
    subs(id)
        .iter()
        .map(|item| (item.damage, item.name))
        .collect()
}

#[test]
fn wool_carries_its_sixteen_damage_names() {
    assert_eq!(
        pairs(35),
        vec![
            (0, "Wool"),
            (1, "Orange Wool"),
            (2, "Magenta Wool"),
            (3, "Light Blue Wool"),
            (4, "Yellow Wool"),
            (5, "Lime Wool"),
            (6, "Pink Wool"),
            (7, "Gray Wool"),
            (8, "Light Gray Wool"),
            (9, "Cyan Wool"),
            (10, "Purple Wool"),
            (11, "Blue Wool"),
            (12, "Brown Wool"),
            (13, "Green Wool"),
            (14, "Red Wool"),
            (15, "Black Wool"),
        ],
        "wool's own sixteen damage names"
    );
    for item in subs(35) {
        assert_eq!(item.tab, CreativeTab::BuildingBlocks, "wool's tab");
    }
}

#[test]
fn the_potion_damage_set_matches_the_sources_own_population() {
    let items = subs(373);
    assert_eq!(
        items.len(),
        63,
        "the water bottle plus the source's 62 potions"
    );
    assert_eq!(
        (items[0].damage, items[0].name),
        (0, "Water Bottle"),
        "the base stack the class files first"
    );
    assert_eq!(
        (items[1].damage, items[1].name),
        (8193, "Potion of Regeneration"),
        "the set's first damage, by literal"
    );
    let last = items[items.len() - 1];
    assert_eq!(
        (last.damage, last.name),
        (16462, "Splash Potion of Invisibility"),
        "the set's last damage, by literal"
    );

    // The 1.8 damage encoding: bit 13 regular, bit 14 splash, bits 5/6 the
    // extended and upgraded variants (`PotionHelper.java`:386-450). The water
    // bottle carries neither bit.
    let regular = items[1..]
        .iter()
        .filter(|item| item.damage & 16384 == 0)
        .count();
    let splash = items.iter().filter(|item| item.damage & 16384 != 0).count();
    assert_eq!(regular, 31, "the regular half");
    assert_eq!(splash, 31, "the splash half");
    for item in &items[1..] {
        assert!(
            item.damage & 8192 != 0 || item.damage & 16384 != 0,
            "every damage carries its potion bit: {}",
            item.damage
        );
        assert_eq!(item.tab, CreativeTab::Brewing, "potions sit in brewing");
    }
    for (damage, name) in [
        (8194, "Potion of Swiftness"),
        (8227, "Potion of Fire Resistance"),
        (8196, "Potion of Poison"),
        (8261, "Potion of Healing"),
        (8201, "Potion of Strength"),
        (8234, "Potion of Slowness"),
        (8225, "Potion of Regeneration"),
        (8257, "Potion of Regeneration"),
        (8265, "Potion of Strength"),
        (16385, "Splash Potion of Regeneration"),
        (16417, "Splash Potion of Regeneration"),
        (16449, "Splash Potion of Regeneration"),
    ] {
        assert_eq!(
            stack_name(373, damage),
            Some(name),
            "damage {damage}'s composed name"
        );
    }
    // The upgraded and extended variants compose the same display name as the base
    // one: the amplifier and the duration live in the tooltip, not the name.
    assert_eq!(stack_name(373, 8193), stack_name(373, 8225));
    assert_eq!(stack_name(373, 8193), stack_name(373, 8257));
    assert_eq!(stack_name(373, 8201), stack_name(373, 8233));
    assert_eq!(stack_name(373, 8201), stack_name(373, 8265));
}

#[test]
fn a_duplicate_effect_list_emits_its_last_damage() {
    // Sixteen pairs of damages share one effect list (l=0 against l=1/l=2 within
    // one pattern and kind; `PotionEffect.equals` compares id, amplifier, duration,
    // splash and ambient). The source's cache is a `LinkedHashMap`: a repeated key
    // replaces the value in place, so each pair's later damage is the one the
    // source emits.
    for (earlier, later, name) in [
        (8195, 8227, "Potion of Fire Resistance"),
        (16387, 16419, "Splash Potion of Fire Resistance"),
        (8197, 8261, "Potion of Healing"),
        (16389, 16453, "Splash Potion of Healing"),
        (8198, 8230, "Potion of Night Vision"),
        (16390, 16422, "Splash Potion of Night Vision"),
        (8200, 8232, "Potion of Weakness"),
        (16392, 16424, "Splash Potion of Weakness"),
        (8202, 8234, "Potion of Slowness"),
        (16394, 16426, "Splash Potion of Slowness"),
        (8204, 8268, "Potion of Harming"),
        (16396, 16460, "Splash Potion of Harming"),
        (8205, 8237, "Potion of Water Breathing"),
        (16397, 16429, "Splash Potion of Water Breathing"),
        (8206, 8238, "Potion of Invisibility"),
        (16398, 16430, "Splash Potion of Invisibility"),
    ] {
        assert_eq!(
            stack_name(373, later),
            Some(name),
            "the pair's later damage {later}"
        );
        assert_eq!(
            stack_name(373, earlier),
            None,
            "the pair's earlier damage {earlier} is replaced"
        );
    }
}

#[test]
fn the_new_log_and_leaf_carry_their_own_damages() {
    // `BlockNewLeaf.getSubBlocks` adds damages 0 and 1; `BlockNewLog` adds the two
    // planks enums' metadata minus 4 (ACACIA 4 -> 0, DARK_OAK 5 -> 1).
    assert_eq!(
        pairs(161),
        vec![(0, "Acacia Leaves"), (1, "Dark Oak Leaves")],
        "leaves2's own damages"
    );
    assert_eq!(
        pairs(162),
        vec![(0, "Acacia Wood"), (1, "Dark Oak Wood")],
        "log2's own damages"
    );
    // The planks enum's metadata (4 and 5) is not a damage either class populates.
    assert_eq!(stack_name(161, 4), None);
    assert_eq!(stack_name(162, 5), None);
}

#[test]
fn spawn_eggs_count_against_the_m4_roster() {
    let items = subs(383);
    assert_eq!(items.len(), 27, "the source's own egg count");
    let damages: Vec<i16> = items.iter().map(|item| item.damage).collect();
    assert_eq!(
        damages,
        vec![
            50, 51, 52, 54, 55, 56, 57, 58, 59, 60, 61, 62, 65, 66, 67, 68, 90, 91, 92, 93, 94, 95,
            96, 98, 100, 101, 120
        ],
        "the eggs' own damage order"
    );
    for item in items {
        let mob = MobType::from_id(item.damage as u8);
        assert!(mob.is_some(), "egg {} names a roster mob", item.damage);
        assert_eq!(item.tab, CreativeTab::Misc, "eggs sit in misc");
        assert!(
            item.name.starts_with("Spawn "),
            "egg {}'s name: {}",
            item.damage,
            item.name
        );
    }
    for (damage, name) in [
        (50, "Spawn Creeper"),
        (57, "Spawn Zombie Pigman"),
        (62, "Spawn Magma Cube"),
        (98, "Spawn Ocelot"),
        (100, "Spawn Horse"),
        (120, "Spawn Villager"),
    ] {
        assert_eq!(stack_name(383, damage), Some(name), "egg {damage}");
    }
    // The roster's eggless five: spawnable, yet no egg carries them.
    for id in [53u8, 63, 64, 97, 99] {
        assert!(MobType::from_id(id).is_some(), "id {id} is in the roster");
        assert!(
            !items.iter().any(|item| item.damage == i16::from(id)),
            "no egg carries roster id {id}"
        );
    }
}

#[test]
fn a_damaged_entry_composes_its_own_name() {
    for (id, damage, name) in [
        (1, 6, "Polished Andesite"),
        (5, 4, "Acacia Wood Planks"),
        (6, 5, "Dark Oak Sapling"),
        (24, 2, "Smooth Sandstone"),
        (35, 1, "Orange Wool"),
        (44, 7, "Quartz Slab"),
        (95, 15, "Black Stained Glass"),
        (155, 2, "Pillar Quartz Block"),
        (263, 1, "Charcoal"),
        (322, 1, "Golden Apple"),
        (349, 3, "Pufferfish"),
        (350, 1, "Cooked Salmon"),
        (351, 15, "Bone Meal"),
        (373, 8193, "Potion of Regeneration"),
        (383, 50, "Spawn Creeper"),
        (397, 3, "Head"),
        (425, 15, "White Banner"),
    ] {
        assert_eq!(stack_name(id, damage), Some(name), "id {id} at {damage}");
    }
    // A row the class files as a single stack names the same string at any damage;
    // a damage a variant class does not populate has no name.
    assert_eq!(stack_name(260, 0), Some("Apple"));
    assert_eq!(stack_name(260, 5), Some("Apple"));
    assert_eq!(stack_name(35, 16), None);
    assert_eq!(stack_name(9999, 0), None);
    assert_eq!(stack_name(0, 0), None);
}

#[test]
fn the_potion_registry_names_its_effects() {
    let names: Vec<u8> = (0..=24)
        .filter_map(|id| potion_name(id).map(|row| row.id))
        .collect();
    assert_eq!(
        names,
        (1..=23).collect::<Vec<u8>>(),
        "the registry's own ids"
    );
    for (id, key, name, postfix) in [
        (1, "potion.moveSpeed", "Speed", "Potion of Swiftness"),
        (2, "potion.moveSlowdown", "Slowness", "Potion of Slowness"),
        (
            4,
            "potion.digSlowDown",
            "Mining Fatigue",
            "Potion of Dullness",
        ),
        (6, "potion.heal", "Instant Health", "Potion of Healing"),
        (7, "potion.harm", "Instant Damage", "Potion of Harming"),
        (
            10,
            "potion.regeneration",
            "Regeneration",
            "Potion of Regeneration",
        ),
        (
            11,
            "potion.resistance",
            "Resistance",
            "Potion of Resistance",
        ),
        (19, "potion.poison", "Poison", "Potion of Poison"),
        (20, "potion.wither", "Wither", "Potion of Decay"),
        (
            23,
            "potion.saturation",
            "Saturation",
            "Potion of Saturation",
        ),
    ] {
        let row = potion_name(id).unwrap_or_else(|| panic!("potion {id} must be in the table"));
        assert_eq!(
            (row.key, row.name, row.postfix),
            (key, name, postfix),
            "potion {id}"
        );
    }
    assert_eq!(potion_name(0), None);
    assert_eq!(potion_name(24), None);
}

#[test]
fn every_variant_row_answers_its_own_sub_items() {
    let mut populated = 0;
    let mut flagged = 0;
    for row in registry() {
        let items = sub_items(row.id);
        assert_eq!(
            items.len() > 1,
            row.variants,
            "id {}'s variant flag marks a class that populates more than its base stack",
            row.id
        );
        if row.variants {
            flagged += 1;
        }
        if !items.is_empty() {
            populated += 1;
            let mut damages: Vec<i16> = items.iter().map(|item| item.damage).collect();
            let count = damages.len();
            damages.sort_unstable();
            damages.dedup();
            assert_eq!(damages.len(), count, "id {}'s damages are unique", row.id);
            for item in items {
                assert!(!item.name.is_empty(), "id {}'s name is present", row.id);
                assert!(
                    creative_tab_items(item.tab).contains(&TabEntry {
                        id: row.id,
                        damage: item.damage
                    }),
                    "id {} at {} is listed in its own tab",
                    row.id,
                    item.damage
                );
            }
        }
    }
    assert_eq!(flagged, 37, "the rows the table flags as variants");
    // Two classes populate exactly their base stack, so the table does not flag
    // them: the dandelion (`BlockYellowFlower`) and the red sandstone slab.
    assert_eq!(populated, 39, "the rows that answer a sub-item list");
    for id in [37, 182] {
        assert_eq!(sub_items(id).len(), 1, "id {id} populates its base stack");
        assert!(!item_entry(id).expect("a row").variants);
    }
}

// ----------------------------------------------------------------- the tabs

#[test]
fn every_tab_carries_its_index_label_icon_and_sheet() {
    for (index, tab, label, icon, sheet) in [
        (
            0,
            CreativeTab::BuildingBlocks,
            "buildingBlocks",
            (45, 0),
            "items.png",
        ),
        (
            1,
            CreativeTab::Decorations,
            "decorations",
            (175, 5),
            "items.png",
        ),
        (2, CreativeTab::Redstone, "redstone", (331, 0), "items.png"),
        (
            3,
            CreativeTab::Transportation,
            "transportation",
            (27, 0),
            "items.png",
        ),
        (4, CreativeTab::Misc, "misc", (327, 0), "items.png"),
        (
            5,
            CreativeTab::Search,
            "search",
            (345, 0),
            "item_search.png",
        ),
        (6, CreativeTab::Food, "food", (260, 0), "items.png"),
        (7, CreativeTab::Tools, "tools", (258, 0), "items.png"),
        (8, CreativeTab::Combat, "combat", (283, 0), "items.png"),
        (9, CreativeTab::Brewing, "brewing", (373, 0), "items.png"),
        (
            10,
            CreativeTab::Materials,
            "materials",
            (280, 0),
            "items.png",
        ),
        (
            11,
            CreativeTab::Inventory,
            "inventory",
            (54, 0),
            "inventory.png",
        ),
    ] {
        assert_eq!(tab.index(), index, "the tab's own index");
        assert_eq!(tab.meta().index, index, "the tab's own row");
        assert_eq!(tab.label(), label, "tab {index}'s own label");
        assert_eq!(
            (tab.icon().id, tab.icon().damage),
            icon,
            "tab {index}'s icon"
        );
        assert_eq!(tab.sheet(), sheet, "tab {index}'s sheet");
        assert_eq!(CreativeTab::from_index(index), Some(tab), "index {index}");
        assert_eq!(CreativeTab::ALL[index as usize], tab, "the array's order");
    }
    assert_eq!(CreativeTab::ALL.len(), 12);
    assert_eq!(CreativeTab::from_index(12), None);
    assert_eq!(CreativeTab::from_index(200), None);
}

#[test]
fn two_tabs_pin_their_first_ten_entries() {
    let head = |tab, count: usize| -> Vec<(i16, i16)> {
        creative_tab_items(tab)
            .iter()
            .take(count)
            .map(|entry| (entry.id, entry.damage))
            .collect()
    };
    assert_eq!(
        head(CreativeTab::BuildingBlocks, 10),
        vec![
            (1, 0),
            (1, 1),
            (1, 2),
            (1, 3),
            (1, 4),
            (1, 5),
            (1, 6),
            (2, 0),
            (3, 0),
            (3, 1)
        ],
        "the building blocks tab's first ten, by literal"
    );
    assert_eq!(
        head(CreativeTab::Brewing, 10),
        vec![
            (370, 0),
            (373, 0),
            (373, 8193),
            (373, 8225),
            (373, 8257),
            (373, 16385),
            (373, 16417),
            (373, 16449),
            (373, 8194),
            (373, 8226)
        ],
        "the brewing tab's first ten, by literal"
    );
}

#[test]
fn the_tab_lists_expand_variants_in_their_own_order() {
    assert_eq!(
        creative_tab_items(CreativeTab::Decorations)
            .iter()
            .take(8)
            .map(|entry| (entry.id, entry.damage))
            .collect::<Vec<_>>(),
        vec![
            (6, 0),
            (6, 1),
            (6, 2),
            (6, 3),
            (6, 4),
            (6, 5),
            (18, 0),
            (18, 1)
        ],
        "decorations opens on the sapling run then the leaves"
    );
    assert_eq!(
        creative_tab_items(CreativeTab::Food)
            .iter()
            .take(8)
            .map(|entry| (entry.id, entry.damage))
            .collect::<Vec<_>>(),
        vec![
            (260, 0),
            (282, 0),
            (297, 0),
            (319, 0),
            (320, 0),
            (322, 0),
            (322, 1),
            (349, 0)
        ],
        "food's own order"
    );
    for tab in CreativeTab::ALL {
        let entries = creative_tab_items(tab);
        for pair in entries.windows(2) {
            assert!(
                pair[0].id <= pair[1].id,
                "{:?} ascends by registration id: {:?} then {:?}",
                tab,
                pair[0],
                pair[1]
            );
        }
        for entry in entries {
            let items = sub_items(entry.id);
            if items.is_empty() {
                assert_eq!(entry.damage, 0, "{:?}: {:?} is a single stack", tab, entry);
            } else {
                assert!(
                    items.iter().any(|item| item.damage == entry.damage),
                    "{:?}: {:?} is a populated damage",
                    tab,
                    entry
                );
            }
        }
    }
}

#[test]
fn the_search_list_is_every_tabbed_stack_in_registration_order() {
    let search = creative_tab_items(CreativeTab::Search);
    assert_eq!(search.len(), 600, "the search list's own size");
    assert_eq!(
        search.iter().take(10).collect::<Vec<_>>(),
        creative_tab_items(CreativeTab::BuildingBlocks)
            .iter()
            .take(10)
            .collect::<Vec<_>>(),
        "the search list opens on the registry's own first stacks"
    );
    for pair in search.windows(2) {
        assert!(
            pair[0].id <= pair[1].id,
            "the search list ascends by id: {:?} then {:?}",
            pair[0],
            pair[1]
        );
    }
    let item_tabs = [
        CreativeTab::BuildingBlocks,
        CreativeTab::Decorations,
        CreativeTab::Redstone,
        CreativeTab::Transportation,
        CreativeTab::Misc,
        CreativeTab::Food,
        CreativeTab::Tools,
        CreativeTab::Combat,
        CreativeTab::Brewing,
        CreativeTab::Materials,
    ];
    let summed: usize = item_tabs
        .iter()
        .map(|tab| creative_tab_items(*tab).len())
        .sum();
    assert_eq!(
        summed, 600,
        "the search list is every tabbed stack exactly once"
    );
    assert!(creative_tab_items(CreativeTab::Inventory).is_empty());
}

#[test]
fn the_search_filter_matches_case_insensitively() {
    for (name, query, matches) in [
        ("Stone", "stone", true),
        ("Stone", "STONE", true),
        ("Stone", "StOnE", true),
        ("Stone", "ton", true),
        ("Stone", "", true),
        ("Orange Wool", "orange wool", true),
        ("Orange Wool", "WOOL", true),
        ("Orange Wool", "range woo", true),
        ("Splash Potion of Regeneration", "potion of regen", true),
        ("Stone", "stones", false),
        ("Stone", "granite", false),
        ("Orange Wool", "red", false),
    ] {
        assert_eq!(
            search_matches(name, query),
            matches,
            "search_matches({name:?}, {query:?})"
        );
    }

    // The rule over the search tab's own list: a fixture of the source's own names.
    let filter = |query: &str| -> Vec<(i16, i16)> {
        creative_tab_items(CreativeTab::Search)
            .iter()
            .filter(|entry| {
                let name = stack_name(entry.id, entry.damage)
                    .unwrap_or_else(|| panic!("{entry:?} must carry a name"));
                search_matches(name, query)
            })
            .map(|entry| (entry.id, entry.damage))
            .collect()
    };
    assert_eq!(
        filter("potion of regeneration"),
        vec![
            (373, 8193),
            (373, 8225),
            (373, 8257),
            (373, 16385),
            (373, 16417),
            (373, 16449)
        ],
        "the regular and splash regeneration potions"
    );
    assert_eq!(filter("wool").len(), 16, "wool's own sixteen stacks");
    assert_eq!(
        filter("wool").iter().map(|pair| pair.1).collect::<Vec<_>>(),
        (0..16).collect::<Vec<_>>()
    );
    assert_eq!(filter("spawn").len(), 27, "every egg's stack");
    assert_eq!(filter("sword").len(), 5, "the five swords");
    assert_eq!(
        filter("stone").len(),
        46,
        "the stone-family stacks and the names that carry the word"
    );
    assert!(filter("no such string").is_empty());
}
