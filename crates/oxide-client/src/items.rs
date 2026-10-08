//! The item registry table: every id the source's `Item.registerItems` registers, one
//! row each, with the display name, the model resolution, the stack rules and the
//! tooltip numbers the screens read.
//!
//! The row data is the shipping client's own. Names are the en_US display strings at
//! damage 0, composed the way the source's item classes compose them (the language
//! file itself is never committed; the table carries the strings the tooltips, the
//! popup and the creative search read). Resolutions are Task 7's model classes in a
//! committed const form — [`ItemModel`] mirrors [`ItemModelSource`]'s four classes as
//! borrowed data so the table can be a `static` the completeness suite reads on a
//! starved store; [`ItemModel::source`] maps each row into the runtime class. The
//! stack caps and use counts are the source's own constructor and registration-site
//! settings.
//!
//! The rows live in the [`table`] submodule, one per registration, ascending by id.
//! `resolve` — the object set's rendering path — reads the same rows: the former
//! sprite slice (`SPRITES`) is subsumed by the table, and the ids it named resolve to
//! the same sheets through their rows.
//!
//! Task 9's read sits beside them: [`sub_items`] answers each variant registration's
//! damage stacks with the strings they compose, [`creative_tab_items`] answers every
//! creative tab's ordered stacks (the search tab's list and [`search_matches`]'s
//! filter rule included; the source's search method also appends one enchanted-book
//! stack per typed enchantment, `GuiContainerCreative.java`:354-360 — those stacks
//! carry enchantment NBT and [`TabEntry`] carries none, so they are not
//! represented), [`CreativeTab`] carries each tab's own index, icon and sheet, and
//! [`potion_name`] the potion registry's names. The data lives in the [`creative`]
//! submodule, derived from the same reference tree and pinned by the same suite.

use oxide_assets::model::{BuiltinItem, ItemModelSource};
use oxide_game::container::{BASE_MAX_STACK_SIZE, StackCaps};
use oxide_proto_v47::entity::MetadataItem;

/// How an item stack resolves for the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemResolution {
    /// A block item: the block's own state, drawn through the baked block models.
    Block(u16),
    /// An item sprite: the generated-item shape over the given sheet path.
    Sprite(&'static str),
    /// Nothing draws the id: the atlas's missing sprite stands in.
    Missing,
}

/// How one registration's model resolves, in the committed table's const form: the
/// same four classes as Task 7's [`ItemModelSource`], carried as borrowed data.
///
/// `ItemModelSource`'s own `String`s and `Vec`s cannot sit in a `static`, and the
/// completeness suite must read the table without the store; the borrowed form keeps
/// the table self-contained and [`ItemModel::source`] maps a row into the runtime
/// class one-to-one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemModel {
    /// A block item: the block model resource name its chain resolves to.
    Block(&'static str),
    /// A generated item: its `layer0…4` sheet paths in order, stopping at the first
    /// layer that does not resolve (`ItemModelGenerator.makeItemModel`).
    Generated(&'static [&'static str]),
    /// One of the folded chest trio, drawn through the chest model.
    Builtin(BuiltinItem),
    /// Nothing resolves.
    Missing,
}

impl ItemModel {
    /// The runtime class Task 7 resolves against the loaded tree.
    pub fn source(self) -> ItemModelSource {
        match self {
            ItemModel::Block(name) => ItemModelSource::Block(name.to_string()),
            ItemModel::Generated(layers) => ItemModelSource::Generated(
                layers.iter().map(|layer| (*layer).to_string()).collect(),
            ),
            ItemModel::Builtin(item) => ItemModelSource::Builtin(item),
            ItemModel::Missing => ItemModelSource::Missing,
        }
    }
}

/// The tooltip attribute numbers one registration carries.
///
/// The values derive per class from the source's own constructor blocks: the melee
/// damage from `ItemSword`/`ItemTool` (`item/ItemSword.java`:138-143,
/// `item/ItemTool.java`:100-105) and the armour value from `ItemArmor`
/// (`item/ItemArmor.java`:82). Those classes state the values on their modifiers; the
/// armour value 1.8 carries on the item instead (`EntityLivingBase.getTotalArmorValue`
/// reads it, `entity/EntityLivingBase.java`:1192-1200), and 1.8's attribute set is five
/// attributes — attack damage is the only one a registration's item states
/// (`entity/SharedMonsterAttributes.java`:16-22). The attack-speed and armour-toughness
/// slots therefore stay `None` for every row; the armour value is carried on the
/// twenty armour rows and composes no attribute line (1.8's armour states no item
/// modifier). [`ItemAttributes::tooltip_inputs`] composes the damage line alone.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ItemAttributes {
    /// The main-hand attack-damage modifier's amount: the class's base plus the
    /// material's own damage.
    pub attack_damage: Option<f32>,
    /// The attack-speed modifier's amount. No 1.8 item states one.
    pub attack_speed: Option<f32>,
    /// The armour value the piece wears. 1.8 composes no attribute line from it.
    pub armour_points: Option<f32>,
    /// The armour-toughness modifier's amount. 1.8 has no toughness concept.
    pub armour_toughness: Option<f32>,
}

impl ItemAttributes {
    /// No attributes at all: every plain registration's row.
    pub const NONE: Self = Self {
        attack_damage: None,
        attack_speed: None,
        armour_points: None,
        armour_toughness: None,
    };

    /// The input of the attribute line the source's tooltip composes for these
    /// numbers, walked the way `ItemStack.getTooltip` walks the item's modifier map
    /// (`item/ItemStack.java`:747-783): the attribute the line names and the
    /// modifier's amount. Attack damage is the one attribute a 1.8 item states, so
    /// this is that line or nothing.
    pub fn tooltip_inputs(self) -> Option<(&'static str, f32)> {
        self.attack_damage
            .map(|amount| ("generic.attackDamage", amount))
    }
}

/// One registration of the source's item registry (`Item.registerItems`,
/// `item/Item.java`:511-953).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemEntry {
    /// The registration's id (`Item.getIdFromItem`).
    pub id: i16,
    /// The en_US display string at damage 0, as the class composes it
    /// (`Item.getItemStackDisplayName` over the class's unlocalized name).
    pub name: &'static str,
    /// The model resolution, in its committed const form.
    pub resolution: ItemModel,
    /// The stack cap the source sets (`Item.getItemStackLimit`, `item/Item.java`:165-167):
    /// the base 64 for most rows, the class's own cap for tools, potions and friends,
    /// the registration-site override where the source states one.
    pub max_stack: u8,
    /// The use count before the item breaks (`Item.getMaxDamage`).
    pub max_damage: i16,
    /// The tooltip attribute numbers.
    pub attributes: ItemAttributes,
    /// Whether the registration's class populates damage sub-items: the entries the
    /// sub-item work expands (`Item.getSubItems` overrides, and the block classes
    /// whose `getSubBlocks` adds more than its base stack).
    pub variants: bool,
}

mod creative;
mod table;

/// The registry's row for a wire id, or `None` when the id is no registration's.
pub fn item_entry(id: i16) -> Option<&'static ItemEntry> {
    table::TABLE
        .binary_search_by_key(&id, |entry| entry.id)
        .ok()
        .map(|index| &table::TABLE[index])
}

/// The whole table, ascending by id — the completeness suite's own read, and the
/// registry's row order. (A creative list's order is the source's registration and
/// sub-item order, not this one.)
pub fn registry() -> &'static [ItemEntry] {
    &table::TABLE
}

/// The table-backed [`StackCaps`] provider: the container arithmetic's per-item cap
/// read over the registry's own rows (the seam `oxide-game`'s container module names).
/// An id outside the registry keeps the base rule.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ItemTable;

impl StackCaps for ItemTable {
    fn max_stack_size(&self, item: &MetadataItem) -> i32 {
        match item_entry(item.id) {
            Some(entry) => i32::from(entry.max_stack),
            None => BASE_MAX_STACK_SIZE,
        }
    }
}

/// Resolves an item stack for the renderer: the block ids draw through the baked
/// block models, the table's generated rows through the generated-item shape over
/// their top sheet, everything else as the missing sprite. The damage folds into the
/// block state's metadata at the renderer, not here.
pub fn resolve(id: i16, damage: i16) -> ItemResolution {
    let _ = damage;
    if (1..256).contains(&id) {
        return ItemResolution::Block(id as u16);
    }
    match item_entry(id).map(|entry| entry.resolution) {
        Some(ItemModel::Generated(layers)) => {
            layers.last().map_or(ItemResolution::Missing, |sheet| {
                ItemResolution::Sprite(sheet)
            })
        }
        _ => ItemResolution::Missing,
    }
}

/// One damage sub-item a registration's class populates: a stack the creative lists
/// expand for a variant row (`Item.getSubItems`'s overrides and the block classes'
/// `getSubBlocks`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubItem {
    /// The damage (metadata) value the stack carries.
    pub damage: i16,
    /// The en_US display string the stack composes at this damage.
    pub name: &'static str,
    /// The creative tab the owning registration sits in.
    pub tab: CreativeTab,
}

/// One entry of a creative list: an item id at a damage value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabEntry {
    /// The registration's id.
    pub id: i16,
    /// The damage (metadata) value the stack carries.
    pub damage: i16,
}

/// One creative tab's own data, as `CreativeTabs`'s anonymous subclasses state it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabMeta {
    /// The tab's slot in `creativeTabArray` (`CreativeTabs.getTabIndex`,
    /// `creativetab/CreativeTabs.java`:123-126).
    pub index: u8,
    /// The tab's own label (`CreativeTabs.getTabLabel`, `:128-131`).
    pub label: &'static str,
    /// The tab's icon stack (`getTabIconItem` at `getIconItemDamage`, `:151-156`).
    pub icon: TabEntry,
    /// The sheet the tab draws from (`getBackgroundImageName`, `:158-161`).
    pub sheet: &'static str,
}

/// One potion of the source's registry (`Potion.potionTypes`), ascending by id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PotionName {
    /// The potion's id (`Potion.getId`).
    pub id: u8,
    /// The registry's own name (`Potion.setPotionName`).
    pub key: &'static str,
    /// The en_US effect name the key carries.
    pub name: &'static str,
    /// The en_US postfix a stack's display name composes from this effect
    /// (`ItemPotion.getItemStackDisplayName`, `item/ItemPotion.java`:211-240).
    pub postfix: &'static str,
}

/// A creative tab: one variant per tab of the source's `creativeTabArray`, in its own
/// `getTabIndex` order (`creativetab/CreativeTabs.java`:17-103).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CreativeTab {
    /// The building blocks tab (`tabBlock`, index 0).
    BuildingBlocks,
    /// The decorations tab (`tabDecorations`, index 1).
    Decorations,
    /// The redstone tab (`tabRedstone`, index 2).
    Redstone,
    /// The transportation tab (`tabTransport`, index 3).
    Transportation,
    /// The miscellaneous tab (`tabMisc`, index 4).
    Misc,
    /// The search tab (`tabAllSearch`, index 5).
    Search,
    /// The food tab (`tabFood`, index 6).
    Food,
    /// The tools tab (`tabTools`, index 7).
    Tools,
    /// The combat tab (`tabCombat`, index 8).
    Combat,
    /// The brewing tab (`tabBrewing`, index 9).
    Brewing,
    /// The materials tab (`tabMaterials`, index 10).
    Materials,
    /// The survival inventory tab (`tabInventory`, index 11).
    Inventory,
}

impl CreativeTab {
    /// Every tab, in `creativeTabArray` order.
    pub const ALL: [CreativeTab; 12] = [
        CreativeTab::BuildingBlocks,
        CreativeTab::Decorations,
        CreativeTab::Redstone,
        CreativeTab::Transportation,
        CreativeTab::Misc,
        CreativeTab::Search,
        CreativeTab::Food,
        CreativeTab::Tools,
        CreativeTab::Combat,
        CreativeTab::Brewing,
        CreativeTab::Materials,
        CreativeTab::Inventory,
    ];

    /// The tab's slot in `creativeTabArray` (`CreativeTabs.getTabIndex`): the
    /// constructor's first argument, which the variants are declared in.
    pub const fn index(self) -> u8 {
        self as u8
    }

    /// The tab's own label (`CreativeTabs.getTabLabel`).
    pub fn label(self) -> &'static str {
        self.meta().label
    }

    /// The tab's icon stack: `getTabIconItem`'s item at `getIconItemDamage`'s damage.
    pub fn icon(self) -> TabEntry {
        self.meta().icon
    }

    /// The sheet the tab draws from (`CreativeTabs.getBackgroundImageName`).
    pub fn sheet(self) -> &'static str {
        self.meta().sheet
    }

    /// The tab's row of the source's own data.
    pub fn meta(self) -> &'static TabMeta {
        &creative::TAB_META[self as usize]
    }

    /// The tab at a `creativeTabArray` index.
    pub fn from_index(index: u8) -> Option<CreativeTab> {
        CreativeTab::ALL
            .into_iter()
            .find(|tab| tab.index() == index)
    }
}

/// The damage sub-items a registration's class populates, in its own populate order
/// (`Item.getSubItems` and the block classes' `getSubBlocks`): the stacks the
/// creative lists expand for the row. Empty for a row the class files as a single
/// stack, and for an id outside the registry.
pub fn sub_items(id: i16) -> &'static [SubItem] {
    match creative::SUB_ITEM_RANGES.binary_search_by_key(&id, |&(row, _, _)| row) {
        Ok(index) => {
            let (_, start, count) = creative::SUB_ITEM_RANGES[index];
            &creative::SUB_ITEMS[start..start + count]
        }
        Err(_) => &[],
    }
}

/// The ordered list of a creative tab: the source's own creation order — the
/// registration order of `Item.registerItems` filtered by the tab, each entry's
/// sub-items in the class's populate order. `Item.itemRegistry` iterates its id list
/// (`util/ObjectIntIdentityMap.java`:38-41), so the order ascends by id.
///
/// [`CreativeTab::Search`] is the search tab's own list
/// (`GuiContainerCreative.updateCreativeSearch`, `:341-387`): every tabbed
/// registration's stacks, in the same order. [`CreativeTab::Inventory`] holds the
/// player's own items and is empty.
pub fn creative_tab_items(tab: CreativeTab) -> &'static [TabEntry] {
    match tab {
        CreativeTab::BuildingBlocks => &creative::BUILDING_BLOCKS_ITEMS,
        CreativeTab::Decorations => &creative::DECORATIONS_ITEMS,
        CreativeTab::Redstone => &creative::REDSTONE_ITEMS,
        CreativeTab::Transportation => &creative::TRANSPORTATION_ITEMS,
        CreativeTab::Misc => &creative::MISC_ITEMS,
        CreativeTab::Search => &creative::SEARCH_ITEMS,
        CreativeTab::Food => &creative::FOOD_ITEMS,
        CreativeTab::Tools => &creative::TOOLS_ITEMS,
        CreativeTab::Combat => &creative::COMBAT_ITEMS,
        CreativeTab::Brewing => &creative::BREWING_ITEMS,
        CreativeTab::Materials => &creative::MATERIALS_ITEMS,
        CreativeTab::Inventory => &creative::INVENTORY_ITEMS,
    }
}

/// The display name a creative list entry carries: the sub-item's own composed name
/// where the class populates one, else the registration's own name. `None` for an id
/// outside the registry, and for a damage the class does not populate.
pub fn stack_name(id: i16, damage: i16) -> Option<&'static str> {
    let entry = item_entry(id)?;
    let items = sub_items(id);
    if items.is_empty() {
        return Some(entry.name);
    }
    items
        .iter()
        .find(|item| item.damage == damage)
        .map(|item| item.name)
}

/// The search tab's filter rule: the source's own comparison
/// (`GuiContainerCreative.java`:363, `:372`) — both sides lower-cased, then
/// `contains`. The source filters over a stack's whole tooltip with formatting codes
/// stripped; the names this table carries have no codes, so the display name is what
/// this compares.
pub fn search_matches(name: &str, query: &str) -> bool {
    name.to_lowercase().contains(&query.to_lowercase())
}

/// The potion registry's row for a potion id (`Potion.potionTypes`), or `None` for an
/// id outside it.
pub fn potion_name(id: u8) -> Option<&'static PotionName> {
    creative::POTION_NAMES
        .binary_search_by_key(&id, |row| row.id)
        .ok()
        .map(|index| &creative::POTION_NAMES[index])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_entry_resolves_to_its_own_sheet() {
        for row in registry() {
            if row.id < 256 {
                // Block ids draw as blocks; the table's sheets serve the item ids.
                continue;
            }
            if let ItemModel::Generated(layers) = row.resolution {
                let top = layers[layers.len() - 1];
                assert_eq!(
                    resolve(row.id, 0),
                    ItemResolution::Sprite(top),
                    "id {} must resolve to its top sheet {top}",
                    row.id
                );
            }
        }
    }

    #[test]
    fn the_table_holds_the_sources_own_ids() {
        assert_eq!(resolve(260, 0), ItemResolution::Sprite("items/apple"));
        assert_eq!(resolve(261, 0), ItemResolution::Sprite("items/bow_standby"));
        assert_eq!(resolve(262, 0), ItemResolution::Sprite("items/arrow"));
        assert_eq!(resolve(263, 0), ItemResolution::Sprite("items/coal"));
        assert_eq!(resolve(264, 0), ItemResolution::Sprite("items/diamond"));
        assert_eq!(resolve(265, 0), ItemResolution::Sprite("items/iron_ingot"));
        assert_eq!(resolve(266, 0), ItemResolution::Sprite("items/gold_ingot"));
        assert_eq!(resolve(267, 0), ItemResolution::Sprite("items/iron_sword"));
        assert_eq!(
            resolve(276, 0),
            ItemResolution::Sprite("items/diamond_sword")
        );
        assert_eq!(resolve(280, 0), ItemResolution::Sprite("items/stick"));
        assert_eq!(resolve(332, 0), ItemResolution::Sprite("items/snowball"));
        assert_eq!(resolve(344, 0), ItemResolution::Sprite("items/egg"));
        assert_eq!(resolve(368, 0), ItemResolution::Sprite("items/ender_pearl"));
        assert_eq!(
            resolve(373, 0),
            ItemResolution::Sprite("items/potion_bottle_drinkable")
        );
        assert_eq!(resolve(381, 0), ItemResolution::Sprite("items/ender_eye"));
        assert_eq!(
            resolve(384, 0),
            ItemResolution::Sprite("items/experience_bottle")
        );
        assert_eq!(resolve(401, 0), ItemResolution::Sprite("items/fireworks"));
    }

    #[test]
    fn block_ids_resolve_through_the_block_family() {
        assert_eq!(resolve(1, 0), ItemResolution::Block(1));
        assert_eq!(resolve(5, 3), ItemResolution::Block(5));
        assert_eq!(resolve(255, 0), ItemResolution::Block(255));
        assert_eq!(resolve(0, 0), ItemResolution::Missing);
    }

    #[test]
    fn unknown_ids_are_missing() {
        assert_eq!(resolve(-1, 0), ItemResolution::Missing);
        assert_eq!(resolve(1000, 0), ItemResolution::Missing);
        assert_eq!(resolve(9999, 0), ItemResolution::Missing);
        assert_eq!(resolve(2268, 0), ItemResolution::Missing);
    }

    #[test]
    fn the_damage_does_not_change_the_resolution() {
        for damage in [0, 1, 7, 15, 32000] {
            assert_eq!(
                resolve(332, damage),
                ItemResolution::Sprite("items/snowball")
            );
            assert_eq!(resolve(5, damage), ItemResolution::Block(5));
        }
    }

    #[test]
    fn the_lookup_answers_the_registrys_own_rows() {
        assert_eq!(item_entry(267).map(|entry| entry.name), Some("Iron Sword"));
        assert_eq!(
            item_entry(373).map(|entry| entry.name),
            Some("Water Bottle")
        );
        assert_eq!(item_entry(0), None);
        assert_eq!(item_entry(255), None);
        assert_eq!(item_entry(9_999), None);
    }

    #[test]
    fn the_cap_bridge_reads_the_table_over_the_base_rule() {
        let item = |id| MetadataItem {
            id,
            count: 1,
            damage: 0,
            nbt: None,
        };
        assert_eq!(ItemTable.max_stack_size(&item(267)), 1);
        assert_eq!(ItemTable.max_stack_size(&item(260)), 64);
        assert_eq!(ItemTable.max_stack_size(&item(9_999)), BASE_MAX_STACK_SIZE);
    }

    #[test]
    fn the_sub_item_and_tab_lookups_answer_the_sources_own_rows() {
        assert_eq!(
            sub_items(35).first().map(|item| (item.damage, item.name)),
            Some((0, "Wool"))
        );
        assert_eq!(
            sub_items(35).get(1).map(|item| (item.damage, item.name)),
            Some((1, "Orange Wool"))
        );
        assert!(sub_items(260).is_empty());
        assert_eq!(
            sub_items(373).last().map(|item| (item.damage, item.name)),
            Some((16462, "Splash Potion of Invisibility"))
        );
        assert_eq!(sub_items(383).len(), 27);
        assert_eq!(stack_name(35, 1), Some("Orange Wool"));
        assert_eq!(stack_name(260, 0), Some("Apple"));
        assert_eq!(stack_name(35, 16), None);
        assert_eq!(stack_name(9_999, 0), None);
        assert_eq!(
            potion_name(1).map(|row| row.postfix),
            Some("Potion of Swiftness")
        );
        assert_eq!(potion_name(0), None);
        assert!(search_matches("Potion of Regeneration", "REGEN"));
        assert!(!search_matches("Potion of Regeneration", "Swift"));
        assert_eq!(
            creative_tab_items(CreativeTab::BuildingBlocks)
                .first()
                .map(|entry| (entry.id, entry.damage)),
            Some((1, 0))
        );
        assert!(creative_tab_items(CreativeTab::Inventory).is_empty());
        assert_eq!(CreativeTab::Brewing.index(), 9);
        assert_eq!(CreativeTab::Brewing.icon().id, 373);
        assert_eq!(CreativeTab::Brewing.sheet(), "items.png");
        assert_eq!(CreativeTab::Search.sheet(), "item_search.png");
        assert_eq!(CreativeTab::from_index(11), Some(CreativeTab::Inventory));
        assert_eq!(CreativeTab::from_index(12), None);
    }
}
