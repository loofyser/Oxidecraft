//! The item model resolution's tests: the generated layers, the block-item
//! fallback, the folded chest trio and the display transforms.
//!
//! The synthetic tree is hand-written JSON in a temp directory, laid out the
//! way the extractor lays it out (`assets/minecraft/models/item/`,
//! `assets/minecraft/models/block/`): no file from the game is involved and
//! no fixture carries a Mojang byte. The expected values are derived from the
//! 1.8 client's own files (`ItemModelGenerator`, `ModelChest`, `ModelBox`,
//! `ModelRenderer`, `ItemCameraTransforms`, `ItemTransformVec3f`, `ModelBlock`,
//! `TileEntityChestRenderer`), not from this crate's implementation.

use std::fs;
use std::path::{Path, PathBuf};

use oxide_assets::model::{
    BuiltinItem, CHEST_MODEL, CHEST_MODEL_SCALE, Display, ItemModelSet, ItemModelSource,
    ModelError, ModelSource, Transform, TransformType,
};

/// `models/item/three_layer.json`: a generated item of three layers.
const THREE_LAYER: &[u8] = br##"{
  "parent": "builtin/generated",
  "textures": {
    "layer0": "items/thing_a",
    "layer1": "items/thing_b",
    "layer2": "items/thing_c"
  }
}"##;

/// `models/item/plain.json`: a generated item of one layer, no display.
const PLAIN: &[u8] = br##"{
  "parent": "builtin/generated",
  "textures": { "layer0": "items/plain" }
}"##;

/// `models/item/gui_override.json`: an explicit gui transform beside a
/// generated layer.
const GUI_OVERRIDE: &[u8] = br##"{
  "parent": "builtin/generated",
  "textures": { "layer0": "items/gui_thing" },
  "display": {
    "gui": {
      "rotation": [ 30, 45, 60 ],
      "translation": [ 1, 2, 3 ],
      "scale": [ 2, 2, 2 ]
    }
  }
}"##;

/// `models/item/child_display.json`: a child that states the identity gui —
/// which the source's own `func_181687_c` reads as unstated — over a parent
/// that states a real one.
const CHILD_DISPLAY: &[u8] = br##"{
  "parent": "item/parent_display",
  "display": {
    "gui": {
      "rotation": [ 0, 0, 0 ],
      "translation": [ 0, 0, 0 ],
      "scale": [ 1, 1, 1 ]
    }
  }
}"##;

/// The parent above: a real gui transform on a generated layer.
const PARENT_DISPLAY: &[u8] = br##"{
  "parent": "builtin/generated",
  "textures": { "layer0": "items/parent_thing" },
  "display": {
    "gui": {
      "rotation": [ 0, 0, 0 ],
      "translation": [ 4, 5, 6 ],
      "scale": [ 3, 3, 3 ]
    }
  }
}"##;

/// `models/item/cube_item.json`: a block item with its own third-person
/// display, as the real tree's block items carry.
const CUBE_ITEM: &[u8] = br##"{
  "parent": "block/cube_item",
  "display": {
    "thirdperson": {
      "rotation": [ 10, 20, 30 ],
      "translation": [ 1, 1.5, -2 ],
      "scale": [ 0.4, 0.4, 0.4 ]
    }
  }
}"##;

/// `models/block/cube_item.json`: the block model the item draws through.
const CUBE_ITEM_BLOCK: &[u8] = br##"{
  "parent": "block/cube_all",
  "textures": { "all": "blocks/cube_item" }
}"##;

/// `models/block/above_block.json`: a second block model two item files parent.
const ABOVE_BLOCK: &[u8] = br##"{
  "parent": "block/cube_all",
  "textures": { "all": "blocks/above_block" }
}"##;

/// `models/item/above_block.json`: the item file above [`ABOVE_BLOCK`], with a
/// gui slot of its own — the shape the real tree's stairs and fences carry.
const ABOVE_ITEM: &[u8] = br##"{
  "parent": "block/above_block",
  "display": {
    "gui": {
      "rotation": [ 0, 180, 0 ]
    }
  }
}"##;

/// `models/item/above_block_egg.json`: a second file parented to the same block
/// model, namespaced and longer — the tie the lookup resolves past.
const ABOVE_ITEM_EGG: &[u8] = br##"{
  "parent": "minecraft:block/above_block"
}"##;

/// `models/block/cube.json`: one element, six faces, every face culled.
const CUBE: &[u8] = br##"{
  "elements": [
    {
      "from": [0, 0, 0],
      "to": [16, 16, 16],
      "faces": {
        "down":  { "uv": [0, 0, 16, 16], "texture": "#down",  "cullface": "down" },
        "up":    { "uv": [0, 0, 16, 16], "texture": "#up",    "cullface": "up" },
        "north": { "uv": [0, 0, 16, 16], "texture": "#north", "cullface": "north" },
        "south": { "uv": [0, 0, 16, 16], "texture": "#south", "cullface": "south" },
        "west":  { "uv": [0, 0, 16, 16], "texture": "#west",  "cullface": "west" },
        "east":  { "uv": [0, 0, 16, 16], "texture": "#east",  "cullface": "east" }
      }
    }
  ]
}"##;

/// `models/block/cube_all.json`: the pure-inheritance helper.
const CUBE_ALL: &[u8] = br##"{
  "parent": "block/cube",
  "textures": {
    "particle": "#all",
    "down": "#all", "up": "#all", "north": "#all",
    "south": "#all", "west": "#all", "east": "#all"
  }
}"##;

/// `models/item/chest.json` and its trapped and ender siblings: the folded
/// `builtin/entity` trio, the shape the real tree writes.
const CHEST: &[u8] = br##"{ "parent": "builtin/entity" }"##;
const TRAPPED_CHEST: &[u8] = br##"{ "parent": "builtin/entity" }"##;
const ENDER_CHEST: &[u8] = br##"{ "parent": "builtin/entity" }"##;

/// A `builtin/entity` id outside the folded trio.
const BANNER: &[u8] = br##"{ "parent": "builtin/entity" }"##;

/// The other builtin ends, all outside the folded set.
const COMPASS: &[u8] = br##"{ "parent": "builtin/compass" }"##;
const MISSING_MARKER: &[u8] = br##"{ "parent": "builtin/missing" }"##;
const UNKNOWN_BUILTIN: &[u8] = br##"{ "parent": "builtin/gizmo" }"##;

/// A parent that names no file in the tree.
const ORPHAN: &[u8] = br##"{ "parent": "block/no_such_model" }"##;

/// A parent outside the minecraft namespace.
const FOREIGN: &[u8] = br##"{ "parent": "othermod:block/cube_item" }"##;

/// Two models whose parents name each other.
const CYCLE_A: &[u8] = br##"{ "parent": "item/cycle_b" }"##;
const CYCLE_B: &[u8] = br##"{ "parent": "item/cycle_a" }"##;

#[test]
fn a_three_layer_item_resolves_its_layers_and_bakes_their_planes() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");

    assert_eq!(
        source.item_model("item/three_layer"),
        ItemModelSource::Generated(vec![
            "items/thing_a".to_string(),
            "items/thing_b".to_string(),
            "items/thing_c".to_string(),
        ]),
        "the three-layer item carries its layers in order"
    );

    let baked = source
        .bake_item("item/three_layer")
        .expect("the generated item bakes");
    assert_eq!(baked.quads.len(), 3, "one plane per resolved layer");
    assert_eq!(
        baked.textures,
        vec![
            "items/thing_a".to_string(),
            "items/thing_b".to_string(),
            "items/thing_c".to_string(),
        ],
        "the bake's resolved textures are the layers"
    );
    let layers = ["items/thing_a", "items/thing_b", "items/thing_c"];
    for (index, quad) in baked.quads.iter().enumerate() {
        assert_eq!(quad.texture, layers[index]);
        assert_eq!(
            quad.tintindex,
            Some(index as u8),
            "a layer's tintindex is its own index"
        );
        assert_close(
            &quad.corners,
            [
                [0.0, 1.0, 0.53125],
                [0.0, 0.0, 0.53125],
                [1.0, 0.0, 0.53125],
                [1.0, 1.0, 0.53125],
            ],
            "the layer plane sits at z = 8.5/16",
        );
    }
    assert!(baked.boxes.is_empty(), "a generated item carries no boxes");
    assert_eq!(
        baked.display,
        Display::DEFAULT,
        "a model with no display section uses the source's defaults"
    );
}

#[test]
fn a_single_layer_item_resolves_one_layer() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");

    assert_eq!(
        source.item_model("item/plain"),
        ItemModelSource::Generated(vec!["items/plain".to_string()])
    );
    let baked = source.bake_item("item/plain").expect("the item bakes");
    assert_eq!(baked.quads.len(), 1);
    assert_eq!(baked.textures, vec!["items/plain".to_string()]);
}

#[test]
fn a_bare_name_names_the_item_tree() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");

    assert_eq!(
        source.item_model("plain"),
        source.item_model("item/plain"),
        "a bare name is the item tree's own (ModelBakery.getItemLocation)"
    );
    assert_eq!(
        source.item_model("minecraft:plain"),
        source.item_model("item/plain"),
        "the minecraft namespace is accepted the way the model tree accepts it"
    );
}

#[test]
fn a_block_item_resolves_to_its_block_model() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");

    assert_eq!(
        source.item_model("item/cube_item"),
        ItemModelSource::Block("block/cube_item".to_string()),
        "a block item resolves to the block model it draws through"
    );

    let baked = source
        .bake_item("item/cube_item")
        .expect("the block item bakes");
    assert_eq!(baked.quads.len(), 6, "the cube is six quads");
    assert_eq!(
        baked.textures,
        vec!["blocks/cube_item".to_string()],
        "the resolved texture is the block's own"
    );
    assert!(baked.boxes.is_empty());
    let third_person = baked.display.get(TransformType::ThirdPerson);
    assert_eq!(
        third_person,
        Transform {
            rotation: [10.0, 20.0, 30.0],
            translation: [1.0, 1.5, -2.0],
            scale: [0.4, 0.4, 0.4],
        },
        "the item model's own third-person transform applies"
    );
}

#[test]
fn the_item_file_above_a_block_member_is_found_and_carries_the_slots() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");

    // The tie: two item files parent `block/above_block`, one namespaced; the
    // shortest path wins, so the item's own file is found.
    assert_eq!(
        source.item_model_above("block/above_block").as_deref(),
        Some("item/above_block"),
        "the shortest item file parented to the member"
    );
    let baked = source
        .bake_item("item/above_block")
        .expect("the item file's chain bakes");
    assert_eq!(
        baked.quads.len(),
        6,
        "the block model's cube, through the item file"
    );
    assert_eq!(
        baked.display.get(TransformType::Gui),
        Transform {
            rotation: [0.0, 180.0, 0.0],
            translation: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        },
        "the item file's own gui slot"
    );

    assert_eq!(
        source.item_model_above("block/cube_item").as_deref(),
        Some("item/cube_item"),
        "the block item's own file"
    );
    assert_eq!(
        source.item_model_above("block/cube"),
        None,
        "no item file parents the helper"
    );
}

#[test]
fn the_chest_trio_resolves_to_the_folded_builtin() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");

    for (name, item, sheet) in [
        ("item/chest", BuiltinItem::Chest, "entity/chest/normal"),
        (
            "item/trapped_chest",
            BuiltinItem::TrappedChest,
            "entity/chest/trapped",
        ),
        (
            "item/ender_chest",
            BuiltinItem::EnderChest,
            "entity/chest/ender",
        ),
    ] {
        assert_eq!(source.item_model(name), ItemModelSource::Builtin(item));
        assert_eq!(
            item.icon_sheet(),
            sheet,
            "the trio's icon sheets are the TESR's own names"
        );
        let baked = source.bake_item(name).expect("the trio member bakes");
        assert!(baked.quads.is_empty(), "the trio's geometry is its boxes");
        assert_eq!(baked.boxes, CHEST_MODEL.to_vec());
        assert_eq!(baked.textures, vec![sheet.to_string()]);
        assert_eq!(baked.display, Display::DEFAULT);
    }

    assert_eq!(
        CHEST_MODEL_SCALE, 0.0625,
        "the chest model's own render scale"
    );
}

#[test]
fn the_chest_model_pins_the_source_boxes() {
    // The three boxes `ModelChest` builds, in its own render order
    // (lid, knob, base): each corner pair as `addBox` states it and each
    // rotation point as the constructor sets it.
    let lid = CHEST_MODEL[0];
    assert_eq!(lid.from, [0.0, -5.0, -14.0]);
    assert_eq!(lid.to, [14.0, 0.0, 0.0]);
    assert_eq!(lid.origin, [1.0, 7.0, 15.0]);

    let knob = CHEST_MODEL[1];
    assert_eq!(knob.from, [-1.0, -2.0, -15.0]);
    assert_eq!(knob.to, [1.0, 2.0, -14.0]);
    assert_eq!(knob.origin, [8.0, 7.0, 15.0]);

    let base = CHEST_MODEL[2];
    assert_eq!(base.from, [0.0, 0.0, 0.0]);
    assert_eq!(base.to, [14.0, 10.0, 14.0]);
    assert_eq!(base.origin, [1.0, 6.0, 1.0]);
}

#[test]
fn an_unknown_name_resolves_missing_with_a_counter() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");

    let set = source.item_models(&[
        "item/three_layer",
        "item/no_such_item",
        "item/chest",
        "item/plain",
        "item/also_absent",
    ]);
    assert_eq!(
        set,
        ItemModelSet {
            sources: vec![
                ItemModelSource::Generated(vec![
                    "items/thing_a".to_string(),
                    "items/thing_b".to_string(),
                    "items/thing_c".to_string(),
                ]),
                ItemModelSource::Missing,
                ItemModelSource::Builtin(BuiltinItem::Chest),
                ItemModelSource::Generated(vec!["items/plain".to_string()]),
                ItemModelSource::Missing,
            ],
            missing: 2,
        },
        "the list resolves in order and counts its misses"
    );
}

#[test]
fn every_unresolvable_shape_degrades_to_missing() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");

    for name in [
        "item/no_such_item",
        "item/banner",
        "item/compass",
        "item/missing_marker",
        "item/unknown_builtin",
        "item/orphan",
        "item/foreign",
        "item/cycle_a",
    ] {
        assert_eq!(
            source.item_model(name),
            ItemModelSource::Missing,
            "{name} degrades to Missing rather than failing the load"
        );
        assert!(source.bake_item(name).is_err(), "{name} bakes no geometry");
    }
}

#[test]
fn the_builtin_ends_without_geometry_say_so() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");

    // The missing marker, the animated compass and a `builtin/entity` id
    // outside the trio resolve to a builtin end the baker has no item shape
    // for; their icons draw the missing sprite (recorded).
    for name in ["item/banner", "item/compass", "item/missing_marker"] {
        assert!(
            matches!(
                source.bake_item(name),
                Err(ModelError::NoItemGeometry { .. })
            ),
            "{name} answers its own no-geometry error"
        );
    }
}

#[test]
fn the_display_defaults_pin_every_field_by_literal() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");
    let baked = source.bake_item("item/plain").expect("the item bakes");

    // The source's own default set (`ItemCameraTransforms.DEFAULT`:13/:32,
    // `ItemTransformVec3f.DEFAULT`:16): the identity for every type.
    for slot in [
        TransformType::None,
        TransformType::ThirdPerson,
        TransformType::FirstPerson,
        TransformType::Head,
        TransformType::Gui,
        TransformType::Ground,
        TransformType::Fixed,
    ] {
        let transform = baked.display.get(slot);
        assert_eq!(transform.rotation, [0.0, 0.0, 0.0], "{slot:?} rotation");
        assert_eq!(
            transform.translation,
            [0.0, 0.0, 0.0],
            "{slot:?} translation"
        );
        assert_eq!(transform.scale, [1.0, 1.0, 1.0], "{slot:?} scale");
    }
}

#[test]
fn an_explicit_gui_overrides_the_default_for_that_type_only() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");
    let baked = source
        .bake_item("item/gui_override")
        .expect("the item bakes");

    assert_eq!(
        baked.display.gui,
        Transform {
            rotation: [30.0, 45.0, 60.0],
            translation: [1.0, 2.0, 3.0],
            scale: [2.0, 2.0, 2.0],
        },
        "the file's own gui numbers land unchanged"
    );
    for slot in [
        TransformType::None,
        TransformType::ThirdPerson,
        TransformType::FirstPerson,
        TransformType::Head,
        TransformType::Ground,
        TransformType::Fixed,
    ] {
        assert_eq!(
            baked.display.get(slot),
            Transform::DEFAULT,
            "{slot:?} stays the source's default"
        );
    }
}

#[test]
fn a_stated_default_falls_through_to_the_parent() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");
    let baked = source
        .bake_item("item/child_display")
        .expect("the item bakes");

    // The child states the identity gui, which `func_181687_c` reads as
    // unstated, so the parent's real one wins (`ModelBlock.getTransform`).
    assert_eq!(
        baked.display.gui,
        Transform {
            rotation: [0.0, 0.0, 0.0],
            translation: [4.0, 5.0, 6.0],
            scale: [3.0, 3.0, 3.0],
        },
        "a stated default does not shadow the parent's transform"
    );
    assert_eq!(
        baked.display.get(TransformType::ThirdPerson),
        Transform::DEFAULT
    );
}

/// Asserts four corners against literals within a float epsilon.
fn assert_close(corners: &[[f32; 3]; 4], expected: [[f32; 3]; 4], label: &str) {
    for (index, (got, want)) in corners.iter().zip(expected).enumerate() {
        for axis in 0..3 {
            assert!(
                (got[axis] - want[axis]).abs() < 1e-5,
                "{label}: corner {index} axis {axis} is {:?}, want {:?}",
                got,
                want
            );
        }
    }
}

/// The synthetic tree every test above resolves from, written file by file.
fn sphere() -> Tree {
    let tree = Tree::new();
    tree.dir("assets/minecraft/blockstates");
    tree.write("assets/minecraft/models/item/three_layer.json", THREE_LAYER);
    tree.write("assets/minecraft/models/item/plain.json", PLAIN);
    tree.write(
        "assets/minecraft/models/item/gui_override.json",
        GUI_OVERRIDE,
    );
    tree.write(
        "assets/minecraft/models/item/child_display.json",
        CHILD_DISPLAY,
    );
    tree.write(
        "assets/minecraft/models/item/parent_display.json",
        PARENT_DISPLAY,
    );
    tree.write("assets/minecraft/models/item/cube_item.json", CUBE_ITEM);
    tree.write(
        "assets/minecraft/models/block/cube_item.json",
        CUBE_ITEM_BLOCK,
    );
    tree.write(
        "assets/minecraft/models/block/above_block.json",
        ABOVE_BLOCK,
    );
    tree.write("assets/minecraft/models/item/above_block.json", ABOVE_ITEM);
    tree.write(
        "assets/minecraft/models/item/above_block_egg.json",
        ABOVE_ITEM_EGG,
    );
    tree.write("assets/minecraft/models/block/cube.json", CUBE);
    tree.write("assets/minecraft/models/block/cube_all.json", CUBE_ALL);
    tree.write("assets/minecraft/models/item/chest.json", CHEST);
    tree.write(
        "assets/minecraft/models/item/trapped_chest.json",
        TRAPPED_CHEST,
    );
    tree.write("assets/minecraft/models/item/ender_chest.json", ENDER_CHEST);
    tree.write("assets/minecraft/models/item/banner.json", BANNER);
    tree.write("assets/minecraft/models/item/compass.json", COMPASS);
    tree.write(
        "assets/minecraft/models/item/missing_marker.json",
        MISSING_MARKER,
    );
    tree.write(
        "assets/minecraft/models/item/unknown_builtin.json",
        UNKNOWN_BUILTIN,
    );
    tree.write("assets/minecraft/models/item/orphan.json", ORPHAN);
    tree.write("assets/minecraft/models/item/foreign.json", FOREIGN);
    tree.write("assets/minecraft/models/item/cycle_a.json", CYCLE_A);
    tree.write("assets/minecraft/models/item/cycle_b.json", CYCLE_B);
    tree
}

/// A synthetic extraction tree in a temp directory, removed on drop.
struct Tree {
    /// Keeps the temp directory alive for the test's duration.
    _dir: tempfile::TempDir,
    /// The extraction root the tree's files live under.
    root: PathBuf,
}

impl Tree {
    /// An empty tree in a fresh temp directory.
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("a temp directory");
        let root = dir.path().to_path_buf();
        Self { _dir: dir, root }
    }

    /// Writes `bytes` at `relative` under the extraction root, creating any
    /// parent directories.
    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("the tree's directories");
        }
        fs::write(&path, bytes).expect("the tree's files");
    }

    /// Creates `relative` as a directory under the extraction root.
    fn dir(&self, relative: &str) {
        fs::create_dir_all(self.root.join(relative)).expect("the tree's directories");
    }

    /// The extraction root to hand [`ModelSource::open`].
    fn root(&self) -> &Path {
        &self.root
    }
}
