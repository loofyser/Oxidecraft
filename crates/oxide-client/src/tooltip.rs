//! The item tooltip: the line builder and the draw assembly.
//!
//! The port reads values and names from the source's builder
//! (`ItemStack.getTooltip`, `item/ItemStack.java`:644-857 — the appendix is
//! its tail at :841-854), the colour wrap (`GuiScreen.renderToolTip`
//! :158-175), the rarity table (`EnumRarity.java`:5-14 over
//! `Item.getRarity`, `item/Item.java`:424-427, plus the gold-apple, record
//! and enchanted-book overrides) and the box (`GuiScreen.drawHoveringText`
//! :189-263); no source text is copied.
//!
//! The milestone's line list ends at name / `ench` / lore / attributes /
//! advanced: the `addInformation` override lines (potion, firework, map,
//! record, banner, book) are deferred, the dyed `Dyed` / `Color:` lines, the
//! `Unbreakable` line and the `CanDestroy` / `CanPlaceOn` blocks are not
//! ported, and `RepairCost` is omitted — the source's builder never
//! references it. What the port leaves out is recorded in the report.

use oxide_assets::font::Font;
use oxide_proto_v47::entity::MetadataItem;
use oxide_proto_v47::nbt::{NbtValue, parse};
use oxide_render::hud::HudDraw;
use oxide_render::text::{colour_code, string_width};

use crate::enchants::enchant_line;
use crate::items::{item_entry, stack_name};

/// One tooltip line: the legacy `§`-coded text and the base RGBA the wrap
/// colours it with — the rarity colour for the name line, grey below
/// (`renderToolTip`:158-175 prepends the rarity to line 0 and grey to every
/// later line; an embedded code later in the text wins at render).
#[derive(Debug, Clone, PartialEq)]
pub struct TooltipLine {
    /// The line's text, `§`-coded where the source codes it.
    pub text: String,
    /// The wrap's base colour for the line.
    pub colour: [f32; 4],
}

/// One stack's rarity (`EnumRarity.java`:5-14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rarity {
    /// White (`§f`).
    Common,
    /// Yellow (`§e`).
    Uncommon,
    /// Aqua (`§b`).
    Rare,
    /// Light purple (`§d`).
    Epic,
}

/// The gold apple's id (`Item.registerItems`:831).
const GOLDEN_APPLE_ID: i16 = 322;
/// The filled map's id (`Item.registerItems`:868).
const FILLED_MAP_ID: i16 = 358;
/// The enchanted book's id (`Item.registerItems`:913).
const ENCHANTED_BOOK_ID: i16 = 403;
/// The record ids' span (`ItemRecord` registrations, `Item.java`:941-952).
const RECORD_MIN_ID: i16 = 2256;
/// The record ids' span (`ItemRecord` registrations, `Item.java`:941-952).
const RECORD_MAX_ID: i16 = 2267;
/// Sharpness's effect id: the only damage enchantment that folds into the
/// tooltip's weapon line for an undefined target
/// (`EnchantmentDamage.calcDamageByCreature`:72-74 — smite and bane read
/// zero there).
const SHARPNESS_ID: u16 = 16;
/// Sharpness's per-level fold for an undefined target: `level × 1.25`
/// (`EnchantmentDamage.calcDamageByCreature`:72-74).
const SHARPNESS_FOLD: f64 = 1.25;
/// The weapon/tool modifier's UUID both classes state
/// (`Item.itemModifierUUID`, `Item.java`:51): the fold's line.
const ITEM_MODIFIER_MOST: i64 =
    i64::from_be_bytes([0xCB, 0x3F, 0x55, 0xD3, 0x64, 0x5C, 0x4F, 0x38]);
/// The weapon/tool modifier's UUID both classes state
/// (`Item.itemModifierUUID`, `Item.java`:51): the fold's line.
const ITEM_MODIFIER_LEAST: i64 =
    i64::from_be_bytes([0xA4, 0x97, 0x9C, 0x13, 0xA3, 0x3D, 0xB5, 0xCF]);

/// One stack's rarity: enchanted reads rare, else common
/// (`Item.getRarity`:424-427), with the three overrides — gold apple meta 0
/// rare, meta above epic (`ItemAppleGold.java`:26-29), records always rare
/// (`ItemRecord.java`:76-79), and an enchanted book with stored enchantments
/// uncommon, else the base rule (`ItemEnchantedBook.java`:32-35).
pub fn rarity_of(stack: &MetadataItem) -> Rarity {
    let root = compound_of(stack);
    if stack.id == GOLDEN_APPLE_ID {
        return if stack.damage == 0 {
            Rarity::Rare
        } else {
            Rarity::Epic
        };
    }
    if (RECORD_MIN_ID..=RECORD_MAX_ID).contains(&stack.id) {
        return Rarity::Rare;
    }
    if stack.id == ENCHANTED_BOOK_ID
        && let Some(root) = root.as_ref()
        && let Some(NbtValue::List(stored)) = child(root, "StoredEnchantments")
        && !stored.is_empty()
    {
        return Rarity::Uncommon;
    }
    let enchanted = root
        .as_ref()
        .is_some_and(|root| matches!(child(root, "ench"), Some(NbtValue::List(_))));
    if enchanted {
        Rarity::Rare
    } else {
        Rarity::Common
    }
}

/// The rarity's `§` colour as RGBA, through the shared classic table.
pub fn rarity_colour(rarity: Rarity) -> [f32; 4] {
    let code = match rarity {
        Rarity::Common => 15,
        Rarity::Uncommon => 14,
        Rarity::Rare => 11,
        Rarity::Epic => 13,
    };
    let [red, green, blue] = colour_code(code);
    [red, green, blue, 1.0]
}

/// The wrap's grey for the lines below the name (`§7`).
fn grey_colour() -> [f32; 4] {
    let [red, green, blue] = colour_code(7);
    [red, green, blue, 1.0]
}

/// One stack's full tooltip: the builder's lines wrapped in the rarity and
/// grey (`renderToolTip`:158-175), with the advanced appendix when the F3+H
/// flag is set.
pub fn tooltip_lines(stack: &MetadataItem, advanced: bool) -> Vec<TooltipLine> {
    let root = compound_of(stack);
    let root = root.as_ref();
    let grey = grey_colour();
    let mut lines = Vec::new();
    lines.push(TooltipLine {
        text: name_line(stack, root, advanced),
        colour: rarity_colour(rarity_of(stack)),
    });
    let ench = root.map(|root| ench_entries(root)).unwrap_or_default();
    let hide = root.map(|root| hide_flags(root)).unwrap_or(0);
    if hide & 1 == 0 {
        for entry in &ench {
            if let Some(line) = enchant_line(entry.id, entry.level) {
                lines.push(TooltipLine {
                    text: line,
                    colour: grey,
                });
            }
        }
    }
    if let Some(root) = root {
        for lore in lore_lines(root) {
            lines.push(TooltipLine {
                text: format!("§5§o{lore}"),
                colour: grey,
            });
        }
    }
    let (has_modifiers, attributes) = attribute_section(stack, root, &ench);
    if has_modifiers && hide & 2 == 0 {
        lines.push(TooltipLine {
            text: String::new(),
            colour: grey,
        });
        for line in attributes {
            lines.push(TooltipLine {
                text: line,
                colour: grey,
            });
        }
    }
    if advanced {
        lines.extend(advanced_tooltip_lines(stack));
    }
    lines
}

/// The F3+H appendix alone (`ItemStack.getTooltip`'s tail, :841-854): the
/// `Durability` line for a damaged stack, the dark-grey registry name, and
/// the dark-grey `NBT: n tag(s)` line when the stack carries a compound.
pub fn advanced_tooltip_lines(stack: &MetadataItem) -> Vec<TooltipLine> {
    let grey = grey_colour();
    let mut lines = Vec::new();
    let root = compound_of(stack);
    if let Some(entry) = item_entry(stack.id)
        && entry.max_damage > 0
        && stack.damage > 0
        && !unbreakable(root.as_ref())
    {
        lines.push(TooltipLine {
            text: format!(
                "Durability: {} / {}",
                entry.max_damage - stack.damage,
                entry.max_damage
            ),
            colour: grey,
        });
    }
    lines.push(TooltipLine {
        text: format!(
            "§8minecraft:{}",
            registry_name(stack.id).unwrap_or("unknown")
        ),
        colour: grey,
    });
    if let Some(root) = root.as_ref() {
        lines.push(TooltipLine {
            text: format!("§8NBT: {} tag(s)", root.len()),
            colour: grey,
        });
    }
    lines
}

/// The name line: the custom name italicised, else the sub-item's composed
/// string, else the damage-0 table string (`getDisplayName`:578-592 over
/// `getItemStackDisplayName`), always closed with `RESET` (:646-654) — then
/// the advanced `(#id/meta)` with the zero-padded id and the meta only for
/// subtyped rows (:656-675), or the filled map's ` #damage` quirk without a
/// custom name outside advanced (:677-680).
fn name_line(
    stack: &MetadataItem,
    root: Option<&Vec<(String, NbtValue)>>,
    advanced: bool,
) -> String {
    let custom = root.and_then(|root| custom_name(root)).map(str::to_string);
    let base = match custom.clone() {
        Some(name) => format!("§o{name}"),
        None => stack_name(stack.id, stack.damage)
            .or_else(|| item_entry(stack.id).map(|entry| entry.name))
            .unwrap_or("Unknown")
            .to_string(),
    };
    let mut line = format!("{base}§r");
    if advanced {
        line.push_str(&format!(" (#{:04}", stack.id));
        if has_subtypes(stack.id) {
            line.push_str(&format!("/{}", stack.damage));
        }
        line.push(')');
    } else if custom.is_none() && stack.id == FILLED_MAP_ID {
        line.push_str(&format!(" #{}", stack.damage));
    }
    line
}

/// The stack's parsed root compound, or `None` without a tail or past a
/// hostile one — Task 1's bounded reader refuses those by name.
fn compound_of(stack: &MetadataItem) -> Option<Vec<(String, NbtValue)>> {
    let tail = stack.nbt.as_deref()?;
    match parse(tail) {
        Ok(NbtValue::Compound(children)) => Some(children),
        _ => None,
    }
}

/// One named child of a compound.
fn child<'a>(compound: &'a [(String, NbtValue)], key: &str) -> Option<&'a NbtValue> {
    compound
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

/// A tag's integer read: the source's `getShort`/`getInteger` cast the
/// numeric shape down, anything else reads zero.
fn as_short(value: &NbtValue) -> i16 {
    match value {
        NbtValue::Byte(v) => i16::from(*v),
        NbtValue::Short(v) => *v,
        NbtValue::Int(v) => *v as i16,
        NbtValue::Long(v) => *v as i16,
        _ => 0,
    }
}

/// A tag's integer read: the source's `getInteger` casts the numeric shape
/// down, anything else reads zero.
fn as_int(value: &NbtValue) -> i32 {
    match value {
        NbtValue::Byte(v) => i32::from(*v),
        NbtValue::Short(v) => i32::from(*v),
        NbtValue::Int(v) => *v,
        NbtValue::Long(v) => *v as i32,
        _ => 0,
    }
}

/// A tag's long read, for the modifier UUID halves.
fn as_long(value: &NbtValue) -> i64 {
    match value {
        NbtValue::Byte(v) => i64::from(*v),
        NbtValue::Short(v) => i64::from(*v),
        NbtValue::Int(v) => i64::from(*v),
        NbtValue::Long(v) => *v,
        _ => 0,
    }
}

/// A tag's decimal read: the source's `getDouble` widens the numeric shape,
/// anything else reads zero.
fn as_double(value: &NbtValue) -> f64 {
    match value {
        NbtValue::Byte(v) => f64::from(*v),
        NbtValue::Short(v) => f64::from(*v),
        NbtValue::Int(v) => f64::from(*v),
        NbtValue::Long(v) => *v as f64,
        NbtValue::Float(v) => f64::from(*v),
        NbtValue::Double(v) => *v,
        _ => 0.0,
    }
}

/// One `ench`-list entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EnchEntry {
    /// The effect id (`id`).
    id: u16,
    /// The level (`lvl`).
    level: i16,
}

/// The `ench` list's entries in wire order (`ItemStack.java`:562-565,
/// :697-714).
fn ench_entries(root: &[(String, NbtValue)]) -> Vec<EnchEntry> {
    let Some(NbtValue::List(entries)) = child(root, "ench") else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let NbtValue::Compound(fields) = entry else {
                return None;
            };
            Some(EnchEntry {
                id: child(fields, "id").map(as_short).unwrap_or(0) as u16,
                level: child(fields, "lvl").map(as_short).unwrap_or(0),
            })
        })
        .collect()
}

/// The custom name: `display.Name` under a `display` compound
/// (`getDisplayName`:578-592).
fn custom_name(root: &[(String, NbtValue)]) -> Option<&str> {
    match child(root, "display") {
        Some(NbtValue::Compound(display)) => match child(display, "Name") {
            Some(NbtValue::String(name)) => Some(name),
            _ => None,
        },
        _ => None,
    }
}

/// The lore lines: the `display.Lore` string list, in order (:732-743). A
/// mistyped entry reads empty, as the source's `getStringTagAt` does.
fn lore_lines(root: &[(String, NbtValue)]) -> Vec<&str> {
    let Some(NbtValue::Compound(display)) = child(root, "display") else {
        return Vec::new();
    };
    let Some(NbtValue::List(lore)) = child(display, "Lore") else {
        return Vec::new();
    };
    lore.iter()
        .map(|line| match line {
            NbtValue::String(text) => text.as_str(),
            _ => "",
        })
        .collect()
}

/// The `HideFlags` integer (`getTooltip`:685-693); bit 1 gates the `ench`
/// lines, bit 2 the attribute block. The milestone ports no line behind the
/// other bits, so they read as nothing.
fn hide_flags(root: &[(String, NbtValue)]) -> i32 {
    child(root, "HideFlags").map(as_int).unwrap_or(0)
}

/// Whether the stack opts out of damage (`Unbreakable`, :786-789): without
/// it the milestone composes no line, but it still hides the `Durability`
/// appendix through `isItemStackDamageable`.
fn unbreakable(root: Option<&Vec<(String, NbtValue)>>) -> bool {
    root.and_then(|root| child(root, "Unbreakable"))
        .is_some_and(|flag| as_int(flag) != 0)
}

/// One attribute modifier: the NBT entry's name, amount and operation, with
/// the zero-UUID entries dropped (`getAttributeModifiers`:967-991).
#[derive(Debug, Clone, PartialEq)]
struct AttrModifier {
    /// The attribute's key (`AttributeName`).
    key: String,
    /// The modifier's amount (`Amount`).
    amount: f64,
    /// The modifier's operation (`Operation`).
    operation: i32,
    /// Whether the modifier carries the weapon/tool UUID: the Sharpness
    /// fold's line (`ItemStack.java`:767-769).
    weapon_uuid: bool,
}

/// The NBT modifier list when the stack states one (`getAttributeModifiers`
/// prefers it over the item's own); `None` falls back to the item.
fn nbt_modifiers(root: &[(String, NbtValue)]) -> Option<Vec<AttrModifier>> {
    let Some(NbtValue::List(entries)) = child(root, "AttributeModifiers") else {
        return None;
    };
    let mut modifiers = Vec::new();
    for entry in entries {
        let NbtValue::Compound(fields) = entry else {
            continue;
        };
        let most = child(fields, "UUIDMost").map(as_long).unwrap_or(0);
        let least = child(fields, "UUIDLeast").map(as_long).unwrap_or(0);
        if most == 0 && least == 0 {
            continue;
        }
        let key = match child(fields, "AttributeName") {
            Some(NbtValue::String(key)) => key.clone(),
            _ => String::new(),
        };
        modifiers.push(AttrModifier {
            key,
            amount: child(fields, "Amount").map(as_double).unwrap_or(0.0),
            operation: child(fields, "Operation").map(as_int).unwrap_or(0),
            weapon_uuid: most == ITEM_MODIFIER_MOST && least == ITEM_MODIFIER_LEAST,
        });
    }
    Some(modifiers)
}

/// The attribute section: whether the blank separator draws (the modifier
/// set is non-empty) and the bare plus/take lines. The NBT list wins when
/// present; else the item's own attack-damage modifier with the Sharpness
/// fold on its weapon UUID (`ItemStack.java`:747-784).
fn attribute_section(
    stack: &MetadataItem,
    root: Option<&Vec<(String, NbtValue)>>,
    ench: &[EnchEntry],
) -> (bool, Vec<String>) {
    let fold: f64 = ench
        .iter()
        .filter(|entry| entry.id == SHARPNESS_ID)
        .map(|entry| f64::from(entry.level) * SHARPNESS_FOLD)
        .sum();
    if let Some(root) = root
        && let Some(modifiers) = nbt_modifiers(root)
    {
        let lines = modifiers
            .iter()
            .filter_map(|modifier| {
                let amount = modifier.amount + if modifier.weapon_uuid { fold } else { 0.0 };
                attribute_line(&modifier.key, amount, modifier.operation)
            })
            .collect();
        return (!modifiers.is_empty(), lines);
    }
    match item_entry(stack.id).and_then(|entry| entry.attributes.tooltip_inputs()) {
        Some((key, amount)) => {
            let lines = attribute_line(key, f64::from(amount) + fold, 0)
                .into_iter()
                .collect();
            (true, lines)
        }
        None => (false, Vec::new()),
    }
}

/// One attribute line: blue `+N name` / red `−N name` for operation 0, the
/// percent shapes for operations 1-2, the number in `DECIMALFORMAT`'s
/// `#.###` (`ItemStack.java`:38, :771-783). A zero amount composes nothing.
fn attribute_line(key: &str, amount: f64, operation: i32) -> Option<String> {
    let scaled = if operation == 1 || operation == 2 {
        amount * 100.0
    } else {
        amount
    };
    let percent = if operation == 1 || operation == 2 {
        "%"
    } else {
        ""
    };
    let display = attribute_display(key);
    if amount > 0.0 {
        Some(format!(
            "§9+{n}{percent} {display}",
            n = decimal_format(scaled)
        ))
    } else if amount < 0.0 {
        Some(format!(
            "§c-{n}{percent} {display}",
            n = decimal_format(-scaled)
        ))
    } else {
        None
    }
}

/// An attribute key's display string (`attribute.name.<key>`,
/// `en_US.lang`:1946-1959); past the table the key passes through, as the
/// source's `StatCollector` returns it.
fn attribute_display(key: &str) -> String {
    match key {
        "generic.maxHealth" => String::from("Max Health"),
        "generic.followRange" => String::from("Mob Follow Range"),
        "generic.knockbackResistance" => String::from("Knockback Resistance"),
        "generic.movementSpeed" => String::from("Speed"),
        "generic.attackDamage" => String::from("Attack Damage"),
        _ => format!("attribute.name.{key}"),
    }
}

/// `DECIMALFORMAT`'s `#.###` (`ItemStack.java`:38): at most three decimals,
/// no trailing zeros.
fn decimal_format(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded == 0.0 {
        return String::from("0");
    }
    let text = format!("{rounded:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The registry name behind one item id, without the `minecraft:`
/// namespace the appendix formats (`Item.itemRegistry`'s own names), or
/// `None` past the table.
pub fn registry_name(id: i16) -> Option<&'static str> {
    REGISTRY_NAMES
        .binary_search_by_key(&id, |(row, _)| *row)
        .ok()
        .map(|index| REGISTRY_NAMES[index].1)
}

/// Whether the id's class states damage sub-items (`Item.getHasSubtypes`):
/// the table's variant rows, plus the filled map, whose unbounded damage is
/// the map id rather than a populated sub-item (`ItemMap.java`:28 sets it).
pub fn has_subtypes(id: i16) -> bool {
    item_entry(id).is_some_and(|entry| entry.variants) || id == FILLED_MAP_ID
}

/// One registry name per row of the item table, ascending by id:
/// `Item.itemRegistry`'s own names (`Item.registerItems` for the item
/// ids, `Block.registerBlocks` for the block ids), without the
/// `minecraft:` namespace the appendix formats at the call site.
/// Derived by `refs/m5-task-17/gen_registry.py` (names only).
const REGISTRY_NAMES: &[(i16, &str)] = &[
    (1, "stone"),
    (2, "grass"),
    (3, "dirt"),
    (4, "cobblestone"),
    (5, "planks"),
    (6, "sapling"),
    (7, "bedrock"),
    (12, "sand"),
    (13, "gravel"),
    (14, "gold_ore"),
    (15, "iron_ore"),
    (16, "coal_ore"),
    (17, "log"),
    (18, "leaves"),
    (19, "sponge"),
    (20, "glass"),
    (21, "lapis_ore"),
    (22, "lapis_block"),
    (23, "dispenser"),
    (24, "sandstone"),
    (25, "noteblock"),
    (27, "golden_rail"),
    (28, "detector_rail"),
    (29, "sticky_piston"),
    (30, "web"),
    (31, "tallgrass"),
    (32, "deadbush"),
    (33, "piston"),
    (35, "wool"),
    (37, "yellow_flower"),
    (38, "red_flower"),
    (39, "brown_mushroom"),
    (40, "red_mushroom"),
    (41, "gold_block"),
    (42, "iron_block"),
    (44, "stone_slab"),
    (45, "brick_block"),
    (46, "tnt"),
    (47, "bookshelf"),
    (48, "mossy_cobblestone"),
    (49, "obsidian"),
    (50, "torch"),
    (52, "mob_spawner"),
    (53, "oak_stairs"),
    (54, "chest"),
    (56, "diamond_ore"),
    (57, "diamond_block"),
    (58, "crafting_table"),
    (60, "farmland"),
    (61, "furnace"),
    (62, "lit_furnace"),
    (65, "ladder"),
    (66, "rail"),
    (67, "stone_stairs"),
    (69, "lever"),
    (70, "stone_pressure_plate"),
    (72, "wooden_pressure_plate"),
    (73, "redstone_ore"),
    (76, "redstone_torch"),
    (77, "stone_button"),
    (78, "snow_layer"),
    (79, "ice"),
    (80, "snow"),
    (81, "cactus"),
    (82, "clay"),
    (84, "jukebox"),
    (85, "fence"),
    (86, "pumpkin"),
    (87, "netherrack"),
    (88, "soul_sand"),
    (89, "glowstone"),
    (91, "lit_pumpkin"),
    (95, "stained_glass"),
    (96, "trapdoor"),
    (97, "monster_egg"),
    (98, "stonebrick"),
    (99, "brown_mushroom_block"),
    (100, "red_mushroom_block"),
    (101, "iron_bars"),
    (102, "glass_pane"),
    (103, "melon_block"),
    (106, "vine"),
    (107, "fence_gate"),
    (108, "brick_stairs"),
    (109, "stone_brick_stairs"),
    (110, "mycelium"),
    (111, "waterlily"),
    (112, "nether_brick"),
    (113, "nether_brick_fence"),
    (114, "nether_brick_stairs"),
    (116, "enchanting_table"),
    (120, "end_portal_frame"),
    (121, "end_stone"),
    (122, "dragon_egg"),
    (123, "redstone_lamp"),
    (126, "wooden_slab"),
    (128, "sandstone_stairs"),
    (129, "emerald_ore"),
    (130, "ender_chest"),
    (131, "tripwire_hook"),
    (133, "emerald_block"),
    (134, "spruce_stairs"),
    (135, "birch_stairs"),
    (136, "jungle_stairs"),
    (137, "command_block"),
    (138, "beacon"),
    (139, "cobblestone_wall"),
    (143, "wooden_button"),
    (145, "anvil"),
    (146, "trapped_chest"),
    (147, "light_weighted_pressure_plate"),
    (148, "heavy_weighted_pressure_plate"),
    (151, "daylight_detector"),
    (152, "redstone_block"),
    (153, "quartz_ore"),
    (154, "hopper"),
    (155, "quartz_block"),
    (156, "quartz_stairs"),
    (157, "activator_rail"),
    (158, "dropper"),
    (159, "stained_hardened_clay"),
    (160, "stained_glass_pane"),
    (161, "leaves2"),
    (162, "log2"),
    (163, "acacia_stairs"),
    (164, "dark_oak_stairs"),
    (165, "slime"),
    (166, "barrier"),
    (167, "iron_trapdoor"),
    (168, "prismarine"),
    (169, "sea_lantern"),
    (170, "hay_block"),
    (171, "carpet"),
    (172, "hardened_clay"),
    (173, "coal_block"),
    (174, "packed_ice"),
    (175, "double_plant"),
    (179, "red_sandstone"),
    (180, "red_sandstone_stairs"),
    (182, "stone_slab2"),
    (183, "spruce_fence_gate"),
    (184, "birch_fence_gate"),
    (185, "jungle_fence_gate"),
    (186, "dark_oak_fence_gate"),
    (187, "acacia_fence_gate"),
    (188, "spruce_fence"),
    (189, "birch_fence"),
    (190, "jungle_fence"),
    (191, "dark_oak_fence"),
    (192, "acacia_fence"),
    (256, "iron_shovel"),
    (257, "iron_pickaxe"),
    (258, "iron_axe"),
    (259, "flint_and_steel"),
    (260, "apple"),
    (261, "bow"),
    (262, "arrow"),
    (263, "coal"),
    (264, "diamond"),
    (265, "iron_ingot"),
    (266, "gold_ingot"),
    (267, "iron_sword"),
    (268, "wooden_sword"),
    (269, "wooden_shovel"),
    (270, "wooden_pickaxe"),
    (271, "wooden_axe"),
    (272, "stone_sword"),
    (273, "stone_shovel"),
    (274, "stone_pickaxe"),
    (275, "stone_axe"),
    (276, "diamond_sword"),
    (277, "diamond_shovel"),
    (278, "diamond_pickaxe"),
    (279, "diamond_axe"),
    (280, "stick"),
    (281, "bowl"),
    (282, "mushroom_stew"),
    (283, "golden_sword"),
    (284, "golden_shovel"),
    (285, "golden_pickaxe"),
    (286, "golden_axe"),
    (287, "string"),
    (288, "feather"),
    (289, "gunpowder"),
    (290, "wooden_hoe"),
    (291, "stone_hoe"),
    (292, "iron_hoe"),
    (293, "diamond_hoe"),
    (294, "golden_hoe"),
    (295, "wheat_seeds"),
    (296, "wheat"),
    (297, "bread"),
    (298, "leather_helmet"),
    (299, "leather_chestplate"),
    (300, "leather_leggings"),
    (301, "leather_boots"),
    (302, "chainmail_helmet"),
    (303, "chainmail_chestplate"),
    (304, "chainmail_leggings"),
    (305, "chainmail_boots"),
    (306, "iron_helmet"),
    (307, "iron_chestplate"),
    (308, "iron_leggings"),
    (309, "iron_boots"),
    (310, "diamond_helmet"),
    (311, "diamond_chestplate"),
    (312, "diamond_leggings"),
    (313, "diamond_boots"),
    (314, "golden_helmet"),
    (315, "golden_chestplate"),
    (316, "golden_leggings"),
    (317, "golden_boots"),
    (318, "flint"),
    (319, "porkchop"),
    (320, "cooked_porkchop"),
    (321, "painting"),
    (322, "golden_apple"),
    (323, "sign"),
    (324, "wooden_door"),
    (325, "bucket"),
    (326, "water_bucket"),
    (327, "lava_bucket"),
    (328, "minecart"),
    (329, "saddle"),
    (330, "iron_door"),
    (331, "redstone"),
    (332, "snowball"),
    (333, "boat"),
    (334, "leather"),
    (335, "milk_bucket"),
    (336, "brick"),
    (337, "clay_ball"),
    (338, "reeds"),
    (339, "paper"),
    (340, "book"),
    (341, "slime_ball"),
    (342, "chest_minecart"),
    (343, "furnace_minecart"),
    (344, "egg"),
    (345, "compass"),
    (346, "fishing_rod"),
    (347, "clock"),
    (348, "glowstone_dust"),
    (349, "fish"),
    (350, "cooked_fish"),
    (351, "dye"),
    (352, "bone"),
    (353, "sugar"),
    (354, "cake"),
    (355, "bed"),
    (356, "repeater"),
    (357, "cookie"),
    (358, "filled_map"),
    (359, "shears"),
    (360, "melon"),
    (361, "pumpkin_seeds"),
    (362, "melon_seeds"),
    (363, "beef"),
    (364, "cooked_beef"),
    (365, "chicken"),
    (366, "cooked_chicken"),
    (367, "rotten_flesh"),
    (368, "ender_pearl"),
    (369, "blaze_rod"),
    (370, "ghast_tear"),
    (371, "gold_nugget"),
    (372, "nether_wart"),
    (373, "potion"),
    (374, "glass_bottle"),
    (375, "spider_eye"),
    (376, "fermented_spider_eye"),
    (377, "blaze_powder"),
    (378, "magma_cream"),
    (379, "brewing_stand"),
    (380, "cauldron"),
    (381, "ender_eye"),
    (382, "speckled_melon"),
    (383, "spawn_egg"),
    (384, "experience_bottle"),
    (385, "fire_charge"),
    (386, "writable_book"),
    (387, "written_book"),
    (388, "emerald"),
    (389, "item_frame"),
    (390, "flower_pot"),
    (391, "carrot"),
    (392, "potato"),
    (393, "baked_potato"),
    (394, "poisonous_potato"),
    (395, "map"),
    (396, "golden_carrot"),
    (397, "skull"),
    (398, "carrot_on_a_stick"),
    (399, "nether_star"),
    (400, "pumpkin_pie"),
    (401, "fireworks"),
    (402, "firework_charge"),
    (403, "enchanted_book"),
    (404, "comparator"),
    (405, "netherbrick"),
    (406, "quartz"),
    (407, "tnt_minecart"),
    (408, "hopper_minecart"),
    (409, "prismarine_shard"),
    (410, "prismarine_crystals"),
    (411, "rabbit"),
    (412, "cooked_rabbit"),
    (413, "rabbit_stew"),
    (414, "rabbit_foot"),
    (415, "rabbit_hide"),
    (416, "armor_stand"),
    (417, "iron_horse_armor"),
    (418, "golden_horse_armor"),
    (419, "diamond_horse_armor"),
    (420, "lead"),
    (421, "name_tag"),
    (422, "command_block_minecart"),
    (423, "mutton"),
    (424, "cooked_mutton"),
    (425, "banner"),
    (427, "spruce_door"),
    (428, "birch_door"),
    (429, "jungle_door"),
    (430, "acacia_door"),
    (431, "dark_oak_door"),
    (2256, "record_13"),
    (2257, "record_cat"),
    (2258, "record_blocks"),
    (2259, "record_chirp"),
    (2260, "record_far"),
    (2261, "record_mall"),
    (2262, "record_mellohi"),
    (2263, "record_stal"),
    (2264, "record_strad"),
    (2265, "record_ward"),
    (2266, "record_11"),
    (2267, "record_wait"),
];

/// The tooltip fill's colour: `l = −267386864` = `0xF0100010`
/// (`drawHoveringText`:230), an ARGB int — alpha `F0`, red `10`, green `00`,
/// blue `10`.
pub const TOOLTIP_FILL: [f32; 4] = [16.0 / 255.0, 0.0, 16.0 / 255.0, 240.0 / 255.0];
/// The border gradient's top colour: `i1 = 1347420415` = `0x505000FF`
/// (:236), an ARGB int — alpha `50`, red `50`, green `00`, blue `FF`.
pub const TOOLTIP_BORDER_TOP: [f32; 4] = [80.0 / 255.0, 0.0, 1.0, 80.0 / 255.0];
/// The border gradient's bottom colour: `(i1 & 0xFEFEFE) >> 1 | (i1 &
/// `0xFF000000)` = `0x5028007F` (:236), an ARGB int — alpha `50`, red `28`,
/// green `00`, blue `7F`.
pub const TOOLTIP_BORDER_BOTTOM: [f32; 4] = [40.0 / 255.0, 0.0, 127.0 / 255.0, 80.0 / 255.0];
/// The frame's pad: the text sits 3 from the fill's inner edge (:230-241).
pub const TOOLTIP_PAD: f32 = 3.0;
/// The per-line advance: 10 px, with 2 extra after the title (:248-253).
pub const TOOLTIP_PITCH: f32 = 10.0;
/// The extra advance after the title line (:248-253).
pub const TOOLTIP_TITLE_GAP: f32 = 2.0;

/// The tooltip box: the text origin (`l1`, `i2` at :209-226) with the
/// measured width (`i`) and the line height (`k`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TooltipBox {
    /// The text's left edge: the cursor plus 12, flipped past the edge.
    pub x: f32,
    /// The text's top: the cursor minus 12, clamped past the bottom.
    pub y: f32,
    /// The longest line's width.
    pub width: f32,
    /// 8 alone, else 8 + 2 + (lines − 1) × 10.
    pub height: f32,
}

/// The tooltip box for `lines` at the cursor in a screen: right-and-below
/// the pointer, flipped left past the right edge (`l1 -= 28 + i`) and
/// clamped up past the bottom (`i2 = height − k − 6`), with no top or left
/// guards (`drawHoveringText`:209-226).
pub fn tooltip_box(
    lines: &[TooltipLine],
    font: &Font,
    mouse: (f32, f32),
    screen: (f32, f32),
) -> TooltipBox {
    let width = lines
        .iter()
        .map(|line| string_width(font, &line.text))
        .max()
        .unwrap_or(0) as f32;
    let height = if lines.len() > 1 {
        8.0 + TOOLTIP_TITLE_GAP + (lines.len() as f32 - 1.0) * TOOLTIP_PITCH
    } else {
        8.0
    };
    let mut x = mouse.0 + 12.0;
    let mut y = mouse.1 - 12.0;
    if x + width > screen.0 {
        x -= 28.0 + width;
    }
    if y + height + 6.0 > screen.1 {
        y = screen.1 - height - 6.0;
    }
    TooltipBox {
        x,
        y,
        width,
        height,
    }
}

/// The tooltip's draws, last in the screen's list (over the cursor): the
/// five fill rects, the border — the vertical sides in half-height steps of
/// the top and bottom colours, the top edge in the top colour and the bottom
/// edge in the bottom colour (no gradient-rect kind exists in the HUD list)
/// — then every line shadowed in its own colour (`:228-253`).
pub fn tooltip_draws(
    lines: &[TooltipLine],
    font: &Font,
    mouse: (f32, f32),
    screen: (f32, f32),
) -> Vec<HudDraw> {
    let placed = tooltip_box(lines, font, mouse, screen);
    let (x, y, width, height) = (placed.x, placed.y, placed.width, placed.height);
    let mut draws = Vec::with_capacity(11 + lines.len());
    let fill = |x: f32, y: f32, width: f32, height: f32| HudDraw::Rect {
        x,
        y,
        width,
        height,
        colour: TOOLTIP_FILL,
    };
    draws.push(fill(x - 3.0, y - 4.0, width + 6.0, 1.0));
    draws.push(fill(x - 3.0, y + height + 3.0, width + 6.0, 1.0));
    draws.push(fill(x - 3.0, y - 3.0, width + 6.0, height + 6.0));
    draws.push(fill(x - 4.0, y - 3.0, 1.0, height + 6.0));
    draws.push(fill(x + width + 3.0, y - 3.0, 1.0, height + 6.0));
    let border = |x: f32, y: f32, width: f32, height: f32, colour: [f32; 4]| HudDraw::Rect {
        x,
        y,
        width,
        height,
        colour,
    };
    let middle = y + height / 2.0;
    draws.push(border(
        x - 3.0,
        y - 2.0,
        1.0,
        middle - (y - 2.0),
        TOOLTIP_BORDER_TOP,
    ));
    draws.push(border(
        x - 3.0,
        middle,
        1.0,
        (y + height + 2.0) - middle,
        TOOLTIP_BORDER_BOTTOM,
    ));
    draws.push(border(
        x + width + 2.0,
        y - 2.0,
        1.0,
        middle - (y - 2.0),
        TOOLTIP_BORDER_TOP,
    ));
    draws.push(border(
        x + width + 2.0,
        middle,
        1.0,
        (y + height + 2.0) - middle,
        TOOLTIP_BORDER_BOTTOM,
    ));
    draws.push(border(
        x - 3.0,
        y - 3.0,
        width + 6.0,
        1.0,
        TOOLTIP_BORDER_TOP,
    ));
    draws.push(border(
        x - 3.0,
        y + height + 2.0,
        width + 6.0,
        1.0,
        TOOLTIP_BORDER_BOTTOM,
    ));
    let mut pen = y;
    for (index, line) in lines.iter().enumerate() {
        draws.push(HudDraw::Text {
            text: line.text.clone(),
            x,
            y: pen,
            scale: 1.0,
            colour: line.colour,
            shadow: true,
            blend: false,
        });
        pen += TOOLTIP_PITCH + if index == 0 { TOOLTIP_TITLE_GAP } else { 0.0 };
    }
    draws
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxide_assets::texture::Texture;

    /// The suite's synthetic font: every printable cell carries a one-texel
    /// left column, so each glyph advances two and the space four — widths
    /// stay exact without the store.
    fn test_font() -> Font {
        const SIDE: u32 = 128;
        const CELL: u32 = 8;
        let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
        for code in 33..=126u32 {
            let cell_x = (code % 16) * CELL;
            let cell_y = (code / 16) * CELL;
            for row in 0..CELL {
                let offset = (((cell_y + row) * SIDE + cell_x) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        Font::load(
            &Texture {
                width: SIDE,
                height: SIDE,
                rgba,
            },
            None,
        )
        .expect("the synthetic sheet is a 16x16 grid")
    }

    /// One NBT string's bytes: the length, then the UTF-8.
    fn nbt_string(text: &str) -> Vec<u8> {
        let mut out = (text.len() as u16).to_be_bytes().to_vec();
        out.extend(text.as_bytes());
        out
    }

    /// One named tag's bytes: the id, the name, then the payload.
    fn tag(id: u8, name: &str, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![id];
        out.extend(nbt_string(name));
        out.extend(payload);
        out
    }

    /// A compound payload's bytes: the children, then the end tag.
    fn compound(children: Vec<u8>) -> Vec<u8> {
        let mut out = children;
        out.push(0);
        out
    }

    /// A list payload's bytes: the element type, the count, then the raw
    /// elements.
    fn list(element: u8, elements: Vec<Vec<u8>>) -> Vec<u8> {
        let mut out = vec![element];
        out.extend((elements.len() as i32).to_be_bytes());
        for element in elements {
            out.extend(element);
        }
        out
    }

    /// One `ench`-list entry: the id and level shorts, unnamed.
    fn ench_entry(id: i16, level: i16) -> Vec<u8> {
        let mut out = tag(2, "id", &id.to_be_bytes());
        out.extend(tag(2, "lvl", &level.to_be_bytes()));
        out.push(0);
        out
    }

    /// A slot's raw NBT tail: the root compound with an empty name.
    fn root(children: Vec<u8>) -> Vec<u8> {
        let mut out = vec![10];
        out.extend(nbt_string(""));
        out.extend(compound(children));
        out
    }

    fn stack(id: i16, damage: i16, nbt: Option<Vec<u8>>) -> MetadataItem {
        MetadataItem {
            id,
            count: 1,
            damage,
            nbt,
        }
    }

    fn texts(lines: &[TooltipLine]) -> Vec<&str> {
        lines.iter().map(|line| line.text.as_str()).collect()
    }

    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

    fn grey() -> [f32; 4] {
        let [r, g, b] = colour_code(7);
        [r, g, b, 1.0]
    }

    fn aqua() -> [f32; 4] {
        let [r, g, b] = colour_code(11);
        [r, g, b, 1.0]
    }

    fn light_purple() -> [f32; 4] {
        let [r, g, b] = colour_code(13);
        [r, g, b, 1.0]
    }

    #[test]
    fn a_plain_item_is_its_name_alone() {
        let lines = tooltip_lines(&stack(265, 0, None), false);
        assert_eq!(lines.len(), 1, "no NBT, no appendix: {lines:?}");
        assert_eq!(lines[0].text, "Iron Ingot§r");
        assert_eq!(lines[0].colour, WHITE, "common reads white");
    }

    #[test]
    fn a_custom_name_is_italic() {
        let name = tag(8, "Name", &nbt_string("My Ingot"));
        let display = tag(10, "display", &compound(name));
        let lines = tooltip_lines(&stack(265, 0, Some(root(display))), false);
        assert_eq!(texts(&lines), ["§oMy Ingot§r"]);
        assert_eq!(lines[0].colour, WHITE);
    }

    #[test]
    fn the_rarity_tiers_colour_the_name() {
        assert_eq!(rarity_of(&stack(265, 0, None)), Rarity::Common);
        assert_eq!(rarity_of(&stack(322, 0, None)), Rarity::Rare);
        assert_eq!(rarity_of(&stack(322, 1, None)), Rarity::Epic);
        assert_eq!(rarity_of(&stack(2256, 0, None)), Rarity::Rare);
        assert_eq!(rarity_colour(Rarity::Rare), aqua());
        assert_eq!(rarity_colour(Rarity::Epic), light_purple());
        let lines = tooltip_lines(&stack(322, 0, None), false);
        assert_eq!(lines[0].text, "Golden Apple§r");
        assert_eq!(lines[0].colour, aqua(), "meta 0 reads rare");
        let lines = tooltip_lines(&stack(322, 1, None), false);
        assert_eq!(lines[0].colour, light_purple(), "meta 1 reads epic");
    }

    #[test]
    fn the_enchanted_book_override_reads_stored_enchantments() {
        let stored = tag(9, "StoredEnchantments", &list(10, vec![ench_entry(16, 1)]));
        let enchanted = stack(403, 0, Some(root(stored)));
        assert_eq!(rarity_of(&enchanted), Rarity::Uncommon);
        let bare = stack(403, 0, None);
        assert_eq!(rarity_of(&bare), Rarity::Common);
    }

    #[test]
    fn an_enchanted_sword_lists_two_roman_lines() {
        let ench = tag(
            9,
            "ench",
            &list(10, vec![ench_entry(16, 3), ench_entry(34, 2)]),
        );
        let lines = tooltip_lines(&stack(276, 0, Some(root(ench))), false);
        // Sharpness III folds 3 × 1.25 into the weapon line: 7 + 3.75.
        assert_eq!(
            texts(&lines),
            [
                "Diamond Sword§r",
                "Sharpness III",
                "Unbreaking II",
                "",
                "§9+10.75 Attack Damage",
            ]
        );
        assert_eq!(lines[0].colour, aqua(), "the ench tag rares the name");
        for line in &lines[1..] {
            assert_eq!(line.colour, grey(), "later lines read grey");
        }
    }

    #[test]
    fn unknown_enchant_ids_are_skipped() {
        let ench = tag(
            9,
            "ench",
            &list(10, vec![ench_entry(9, 1), ench_entry(16, 1)]),
        );
        let lines = tooltip_lines(&stack(276, 0, Some(root(ench))), false);
        assert_eq!(
            texts(&lines),
            [
                "Diamond Sword§r",
                "Sharpness I",
                "",
                "§9+8.25 Attack Damage",
            ]
        );
    }

    #[test]
    fn lore_lines_are_purple_italic() {
        let lore = tag(
            9,
            "Lore",
            &list(8, vec![nbt_string("Line one"), nbt_string("Line two")]),
        );
        let display = tag(10, "display", &compound(lore));
        let lines = tooltip_lines(&stack(265, 0, Some(root(display))), false);
        assert_eq!(
            texts(&lines),
            ["Iron Ingot§r", "§5§oLine one", "§5§oLine two",]
        );
        assert_eq!(lines[1].colour, grey());
    }

    #[test]
    fn a_diamond_sword_states_its_attack_damage() {
        let lines = tooltip_lines(&stack(276, 0, None), false);
        assert_eq!(texts(&lines), ["Diamond Sword§r", "", "§9+7 Attack Damage"]);
    }

    #[test]
    fn attribute_modifiers_come_from_nbt_first() {
        fn modifier(name: &str, amount: &[u8], operation: i32) -> Vec<u8> {
            let mut out = tag(8, "AttributeName", &nbt_string(name));
            out.extend(tag(6, "Amount", amount));
            out.extend(tag(3, "Operation", &operation.to_be_bytes()));
            out.extend(tag(4, "UUIDMost", &1i64.to_be_bytes()));
            out.extend(tag(4, "UUIDLeast", &1i64.to_be_bytes()));
            out.push(0);
            out
        }
        let entries = vec![
            modifier("generic.attackDamage", &5.0f64.to_be_bytes(), 0),
            modifier("generic.maxHealth", &(-2.0f64).to_be_bytes(), 0),
            modifier("generic.movementSpeed", &0.2f64.to_be_bytes(), 1),
        ];
        let list = tag(9, "AttributeModifiers", &list(10, entries));
        let lines = tooltip_lines(&stack(265, 0, Some(root(list))), false);
        assert_eq!(
            texts(&lines),
            [
                "Iron Ingot§r",
                "",
                "§9+5 Attack Damage",
                "§c-2 Max Health",
                "§9+20% Speed",
            ]
        );
    }

    #[test]
    fn zero_uuid_modifiers_are_dropped() {
        let mut entry = tag(8, "AttributeName", &nbt_string("generic.attackDamage"));
        entry.extend(tag(6, "Amount", &5.0f64.to_be_bytes()));
        entry.extend(tag(3, "Operation", &0i32.to_be_bytes()));
        entry.extend(tag(4, "UUIDMost", &0i64.to_be_bytes()));
        entry.extend(tag(4, "UUIDLeast", &0i64.to_be_bytes()));
        entry.push(0);
        let list = tag(9, "AttributeModifiers", &list(10, vec![entry]));
        let lines = tooltip_lines(&stack(265, 0, Some(root(list))), false);
        assert_eq!(texts(&lines), ["Iron Ingot§r"]);
    }

    #[test]
    fn hide_flags_gate_enchantments_and_attributes() {
        let mut nbt = tag(9, "ench", &list(10, vec![ench_entry(16, 1)]));
        nbt.extend(tag(3, "HideFlags", &1i32.to_be_bytes()));
        let lines = tooltip_lines(&stack(276, 0, Some(root(nbt))), false);
        assert_eq!(
            texts(&lines),
            ["Diamond Sword§r", "", "§9+8.25 Attack Damage",]
        );
        let mut nbt = tag(9, "ench", &list(10, vec![ench_entry(16, 1)]));
        nbt.extend(tag(3, "HideFlags", &3i32.to_be_bytes()));
        let lines = tooltip_lines(&stack(276, 0, Some(root(nbt))), false);
        assert_eq!(texts(&lines), ["Diamond Sword§r"]);
    }

    #[test]
    fn the_repair_cost_composes_no_line() {
        let nbt = tag(3, "RepairCost", &5i32.to_be_bytes());
        let lines = tooltip_lines(&stack(276, 0, Some(root(nbt))), false);
        assert_eq!(texts(&lines), ["Diamond Sword§r", "", "§9+7 Attack Damage"]);
    }

    #[test]
    fn a_filled_map_appends_its_damage() {
        let lines = tooltip_lines(&stack(358, 7, None), false);
        assert_eq!(texts(&lines), ["Map§r #7"]);
    }

    #[test]
    fn the_advanced_appendix_pins_durability_registry_and_nbt() {
        let lines = tooltip_lines(&stack(276, 50, None), true);
        assert_eq!(
            texts(&lines),
            [
                "Diamond Sword§r (#0276)",
                "",
                "§9+7 Attack Damage",
                "Durability: 1511 / 1561",
                "§8minecraft:diamond_sword",
            ]
        );
        let name = tag(8, "Name", &nbt_string("X"));
        let display = tag(10, "display", &compound(name));
        let lines = tooltip_lines(&stack(276, 50, Some(root(display))), true);
        assert_eq!(
            texts(&lines),
            [
                "§oX§r (#0276)",
                "",
                "§9+7 Attack Damage",
                "Durability: 1511 / 1561",
                "§8minecraft:diamond_sword",
                "§8NBT: 1 tag(s)",
            ]
        );
    }

    #[test]
    fn the_advanced_id_carries_the_meta_for_subtypes() {
        let lines = tooltip_lines(&stack(322, 1, None), true);
        assert_eq!(texts(&lines)[0], "Golden Apple§r (#0322/1)");
    }

    #[test]
    fn the_registry_names_the_sources_own() {
        assert_eq!(registry_name(276), Some("diamond_sword"));
        assert_eq!(registry_name(1), Some("stone"));
        assert_eq!(registry_name(2256), Some("record_13"));
        assert_eq!(registry_name(9_999), None);
        assert!(has_subtypes(322), "the gold apple states subtypes");
        assert!(has_subtypes(358), "the filled map states subtypes");
        assert!(!has_subtypes(276), "a sword states none");
    }

    #[test]
    fn the_box_sits_right_and_below_the_cursor() {
        let font = test_font();
        let lines = vec![TooltipLine {
            text: String::from("§fAB§r"),
            colour: WHITE,
        }];
        let placed = tooltip_box(&lines, &font, (10.0, 20.0), (200.0, 200.0));
        assert_eq!(
            placed,
            TooltipBox {
                x: 22.0,
                y: 8.0,
                width: 4.0,
                height: 8.0
            },
            "cursor + 12, cursor − 12"
        );
    }

    #[test]
    fn the_box_flips_past_the_right_edge() {
        let font = test_font();
        let lines = vec![TooltipLine {
            text: String::from("§fAB§r"),
            colour: WHITE,
        }];
        let placed = tooltip_box(&lines, &font, (190.0, 20.0), (200.0, 200.0));
        assert_eq!(placed.x, 170.0, "cursor + 12 − 28 − width");
        assert_eq!(placed.y, 8.0);
    }

    #[test]
    fn the_box_clamps_past_the_bottom_edge() {
        let font = test_font();
        let lines = vec![
            TooltipLine {
                text: String::from("§fAB§r"),
                colour: WHITE,
            },
            TooltipLine {
                text: String::from("CD"),
                colour: grey(),
            },
        ];
        let placed = tooltip_box(&lines, &font, (10.0, 195.0), (200.0, 200.0));
        assert_eq!(placed.height, 20.0, "8 + 2 + one pitch");
        assert_eq!(placed.y, 174.0, "screen − height − 6");
    }

    #[test]
    fn the_draws_fill_border_then_shadowed_text() {
        use oxide_render::hud::HudDraw;

        let font = test_font();
        let lines = vec![TooltipLine {
            text: String::from("§fAB§r"),
            colour: WHITE,
        }];
        let draws = tooltip_draws(&lines, &font, (10.0, 20.0), (200.0, 200.0));
        assert_eq!(draws.len(), 5 + 6 + 1, "fills, border halves, one line");
        for draw in &draws[..5] {
            assert!(
                matches!(draw, HudDraw::Rect { colour, .. } if *colour == TOOLTIP_FILL),
                "the fill first: {draw:?}"
            );
        }
        for draw in &draws[5..11] {
            let HudDraw::Rect { colour, .. } = draw else {
                panic!("the border is solid rects: {draw:?}");
            };
            assert!(
                *colour == TOOLTIP_BORDER_TOP || *colour == TOOLTIP_BORDER_BOTTOM,
                "border halves only: {draw:?}"
            );
        }
        let HudDraw::Rect { colour, y, .. } = &draws[5] else {
            panic!("the first border rect draws: {:?}", draws[5]);
        };
        assert_eq!(*colour, TOOLTIP_BORDER_TOP);
        assert_eq!(
            *y, 6.0,
            "the left side's top half starts below the top edge"
        );
        let HudDraw::Rect { colour, y, .. } = &draws[9] else {
            panic!("the top edge draws: {:?}", draws[9]);
        };
        assert_eq!(*colour, TOOLTIP_BORDER_TOP);
        assert_eq!(*y, 5.0, "the top edge's row");
        let HudDraw::Rect { colour, y, .. } = &draws[10] else {
            panic!("the bottom edge draws: {:?}", draws[10]);
        };
        assert_eq!(*colour, TOOLTIP_BORDER_BOTTOM);
        assert_eq!(*y, 18.0, "the bottom edge's row");
        let HudDraw::Text {
            text,
            colour,
            shadow,
            blend,
            scale,
            x,
            y,
        } = draws.last().expect("the line draws last")
        else {
            panic!("the line is text: {:?}", draws.last());
        };
        assert_eq!(text, "§fAB§r");
        assert_eq!((*x, *y), (22.0, 8.0));
        assert_eq!(*colour, WHITE);
        assert_eq!(*scale, 1.0);
        assert!(shadow, "every line takes the shadow path");
        assert!(!blend);
    }

    #[test]
    fn the_second_line_pitches_past_the_title_gap() {
        use oxide_render::hud::HudDraw;

        let font = test_font();
        let lines = vec![
            TooltipLine {
                text: String::from("§fAB§r"),
                colour: WHITE,
            },
            TooltipLine {
                text: String::from("CD"),
                colour: grey(),
            },
        ];
        let draws = tooltip_draws(&lines, &font, (10.0, 20.0), (200.0, 200.0));
        let pens: Vec<(f32, f32)> = draws
            .iter()
            .filter_map(|draw| match draw {
                HudDraw::Text { x, y, .. } => Some((*x, *y)),
                _ => None,
            })
            .collect();
        assert_eq!(pens, [(22.0, 8.0), (22.0, 20.0)], "+2 after line 0, +10");
    }

    #[test]
    fn the_roman_helper_spells_to_ten() {
        assert_eq!(crate::enchants::roman_numeral(4), "IV");
        assert_eq!(crate::enchants::roman_numeral(9), "IX");
    }

    #[test]
    fn the_box_colours_pin_the_sources_own_literals() {
        fn bytes(colour: [f32; 4]) -> [u8; 4] {
            [
                (colour[0] * 255.0).round() as u8,
                (colour[1] * 255.0).round() as u8,
                (colour[2] * 255.0).round() as u8,
                (colour[3] * 255.0).round() as u8,
            ]
        }
        assert_eq!(bytes(TOOLTIP_FILL), [16, 0, 16, 240], "0xF0100010");
        assert_eq!(bytes(TOOLTIP_BORDER_TOP), [80, 0, 255, 80], "0x505000FF");
        assert_eq!(bytes(TOOLTIP_BORDER_BOTTOM), [40, 0, 127, 80], "0x5028007F");
    }
}
