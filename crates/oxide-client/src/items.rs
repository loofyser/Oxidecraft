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
/// damage from `ItemSword`/`ItemTool` (`item/ItemSword.java`:138-146,
/// `item/ItemTool.java`:100-108) and the armour value from `ItemArmor`
/// (`item/ItemArmor.java`:82). Those classes state the values on their modifiers; the
/// armour value 1.8 carries on the item instead (`EntityLivingBase.getTotalArmorValue`
/// reads it, `entity/EntityLivingBase.java`:1192-1200), and 1.8's attribute set is five
/// attributes — attack damage is the only one a registration's item states
/// (`entity/SharedMonsterAttributes.java`:16-22). The attack-speed, armour and
/// toughness slots therefore stay `None` for every row, and
/// [`ItemAttributes::tooltip_inputs`] composes the damage line alone.
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
}
