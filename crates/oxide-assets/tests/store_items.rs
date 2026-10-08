//! The ignored real-store pass for the item model resolution: a bounded item
//! set resolves and bakes from the user's own extraction tree.
//!
//! Ignored by default because it needs the user's own store, exactly like
//! `tests/resources.rs`'s `the_real_extraction_tree_loads`: `OXIDECRAFT_STORE`
//! must name the store root that holds `extracted/`.

use std::path::Path;

use oxide_assets::model::{BuiltinItem, ItemModelSource, ModelSource, Transform, TransformType};
use oxide_assets::resources::TextureSet;

/// The bounded list: a sword, a tool, a block item, a chest and a potion —
/// one per resolution class.
const ITEMS: [&str; 5] = [
    "item/diamond_sword",
    "item/stone_pickaxe",
    "item/acacia_planks",
    "item/chest",
    "item/bottle_drinkable",
];

#[test]
#[ignore = "reads the real extraction tree; run it with OXIDECRAFT_STORE set and --ignored"]
fn the_real_item_set_resolves_and_bakes() {
    let store = std::env::var("OXIDECRAFT_STORE").expect(
        "OXIDECRAFT_STORE must name the store root that holds extracted/ (for example \
         ~/.local/share/oxidecraft); this test does not pass without a store",
    );
    let root = Path::new(&store).join("extracted").join("1.8.9");
    let models = ModelSource::open(&root).expect("the real model tree opens");
    let textures = TextureSet::load(&root).expect("the real texture tree loads");

    let set = models.item_models(&ITEMS);
    assert_eq!(
        set.missing, 0,
        "every bounded item resolves in the real tree: {:?}",
        set.sources
    );
    assert!(
        matches!(set.sources[0], ItemModelSource::Generated(_)),
        "the sword is a generated item: {:?}",
        set.sources[0]
    );
    assert!(
        matches!(set.sources[1], ItemModelSource::Generated(_)),
        "the tool is a generated item: {:?}",
        set.sources[1]
    );
    assert!(
        matches!(set.sources[2], ItemModelSource::Block(_)),
        "the block item draws through its block model: {:?}",
        set.sources[2]
    );
    assert_eq!(
        set.sources[3],
        ItemModelSource::Builtin(BuiltinItem::Chest),
        "the chest is the folded trio's own"
    );
    assert!(
        matches!(set.sources[4], ItemModelSource::Generated(_)),
        "the potion is a generated item: {:?}",
        set.sources[4]
    );

    for (name, source) in ITEMS.iter().zip(&set.sources) {
        let baked = models.bake_item(name).expect("the item bakes");
        let geometry = baked.quads.len() + baked.boxes.len();
        assert!(geometry > 0, "{name} bakes non-empty geometry");
        assert!(!baked.textures.is_empty(), "{name} resolves textures");
        for texture in &baked.textures {
            assert!(
                textures.item_texture(texture).is_some(),
                "{name}: the texture {texture} is not in the store's tree"
            );
        }
        println!(
            "{name}: {source:?} -> {} quads, {} boxes, textures {:?}",
            baked.quads.len(),
            baked.boxes.len(),
            baked.textures
        );
    }

    // The trio's icon sheet is the TESR's own `.png` name, and the naming
    // path reads it with or without the suffix.
    for sheet in [
        "entity/chest/normal",
        "entity/chest/trapped",
        "entity/chest/ender",
    ] {
        assert!(
            textures.item_texture(sheet).is_some(),
            "the chest sheet {sheet} is in the store"
        );
        assert!(
            textures.item_texture(&format!("{sheet}.png")).is_some(),
            "the chest sheet {sheet}.png reads through the naming path"
        );
    }

    // The store's own item models state their transforms; the completion
    // carries them (a reading, not a literal).
    let sword = models
        .bake_item("item/diamond_sword")
        .expect("the sword bakes");
    let third_person = sword.display.get(TransformType::ThirdPerson);
    assert_ne!(
        third_person,
        Transform::DEFAULT,
        "the sword's model states a third-person transform"
    );
    assert_eq!(
        sword.display.get(TransformType::Ground),
        Transform::DEFAULT,
        "a type the model does not state stays the source's default"
    );
    println!("diamond_sword third-person: {third_person:?}");
}
