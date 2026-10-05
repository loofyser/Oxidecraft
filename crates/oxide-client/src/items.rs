//! The wire item ids the object set draws: the stack an object entity carries names an
//! item id, and the renderer needs to know which of the two model families draws it —
//! a block's baked state or an item's sprite sheet.
//!
//! The id space is the source's own: ids under 256 are blocks (the block registry's
//! ids), the ones above are the item registry's entries (`Item.registerItems`' explicit
//! `(id, name)` pairs, `Item.java`:769-911). The table below is the object set's
//! minimal slice of the item registry — the entries a stack in the world plausibly
//! carries today; the full registry rides the later item work. Each sprite entry names
//! the sheet path the store's own `models/item` file layers (`layer0`).

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

/// The sprite table: each entry a literal `registry id -> sheet path` pair. The ids are
/// the source's registrations (`Item.java`:769-911, names and numbers alike); the paths
/// are the store's own item model layers (`models/item/*.json`'s `layer0`).
const SPRITES: [(i16, &str); 17] = [
    (260, "items/apple"),
    (261, "items/bow_standby"),
    (262, "items/arrow"),
    (263, "items/coal"),
    (264, "items/diamond"),
    (265, "items/iron_ingot"),
    (266, "items/gold_ingot"),
    (267, "items/iron_sword"),
    (276, "items/diamond_sword"),
    (280, "items/stick"),
    (332, "items/snowball"),
    (344, "items/egg"),
    (368, "items/ender_pearl"),
    (373, "items/potion_bottle_drinkable"),
    (381, "items/ender_eye"),
    (384, "items/experience_bottle"),
    (401, "items/fireworks"),
];

/// Resolves an item stack for the renderer: the block ids draw through the baked block
/// models, the table's sprite entries through the generated-item shape, everything else
/// as the missing sprite. The damage folds into the block state's metadata at the
/// renderer, not here.
pub fn resolve(id: i16, damage: i16) -> ItemResolution {
    let _ = damage;
    if (1..256).contains(&id) {
        return ItemResolution::Block(id as u16);
    }
    let mut index = 0;
    while index < SPRITES.len() {
        if SPRITES[index].0 == id {
            return ItemResolution::Sprite(SPRITES[index].1);
        }
        index += 1;
    }
    ItemResolution::Missing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_entry_resolves_to_its_own_sheet() {
        for (id, path) in SPRITES {
            assert_eq!(
                resolve(id, 0),
                ItemResolution::Sprite(path),
                "id {id} must resolve to {path}"
            );
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
        assert_eq!(resolve(259, 0), ItemResolution::Missing);
        assert_eq!(resolve(9999, 0), ItemResolution::Missing);
        assert_eq!(resolve(256, 0), ItemResolution::Missing);
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
}
