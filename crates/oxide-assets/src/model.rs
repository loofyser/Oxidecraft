//! Blockstates and models: the jar's JSON with 1.8 semantics, resolved down
//! the `parent` chains, and baked into the quads a mesher draws.
//!
//! [`ModelSource::open`] reads the tree the extractor wrote — the
//! `assets/minecraft/blockstates/` and `assets/minecraft/models/` directories
//! under it — and parses every `.json` in both, in sorted order, so the tree
//! is whole or it is an error naming the file and the reason. The formats are
//! the 1.8 ones: a blockstate maps a variant key to one variant or an array
//! of them (`model`, `x`, `y`, `uvlock`, `weight`), and a model holds a
//! `parent`, a `textures` map, `elements` of cuboids with six named faces
//! each, `ambientocclusion` (default true) and `display`.
//!
//! [`ModelSource::bake_variant`] resolves the variant's model through its
//! parent chain — `builtin/*` ends a chain — merges the texture variables
//! the way the client does, and bakes one [`BakedQuad`] per present face. The
//! corner order, the uv order, the face and element rotations, `uvlock` and
//! the box normalisation for a `from`/`to` pair that runs backwards follow
//! the 1.8 client's own `FaceBakery`/`BlockFaceUV`/`BlockPart`/`ModelRotation`
//! semantics, derived value by value from those files, never copied.
//!
//! [`ModelSource::texture_paths`] answers the atlas's question: every texture
//! path the tree's blockstate files resolve, plus the locations the client
//! stitches beyond the variant scan (`ModelBakery.LOCATIONS_BUILTIN_TEXTURES`:
//! the four liquid flow/still sprites, the ten destroy stages and the four
//! empty armour-slot sprites).
//!
//! [`ModelSource::item_model`] and [`ModelSource::bake_item`] resolve the
//! item tree the way the client's item pass does (`ModelBakery`'s item walk,
//! `ItemModelGenerator`, `ModelChest`, `ItemCameraTransforms`): a generated
//! item's `layer0…4` layers through the `builtin/generated` bake, a block
//! item's block model, the folded chest trio's box model, and the display
//! transforms every class carries, completed from the source's own defaults.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The blockstate tree inside an extraction root, with the case the jar uses.
const BLOCKSTATES_DIR: &str = "assets/minecraft/blockstates";

/// The model tree inside an extraction root, with the case the jar uses.
const MODELS_DIR: &str = "assets/minecraft/models";

/// The extension both trees' files carry.
const JSON_EXTENSION: &str = "json";

/// The namespace every name in the tree belongs to.
const NAMESPACE: &str = "minecraft";

/// What every blockstate variant's model name is relative to.
const BLOCK_MODEL_PREFIX: &str = "block/";

/// What an item's model name is relative to: the client resolves an item
/// registration's name to `item/<name>` (`ModelBakery.getItemLocation`).
const ITEM_MODEL_PREFIX: &str = "item/";

/// The three model names the folded chest trio registers under, as
/// `models/item/<name>.json` writes them.
const CHEST_ITEM_MODEL: &str = "item/chest";

/// The trapped chest's model name.
const TRAPPED_CHEST_ITEM_MODEL: &str = "item/trapped_chest";

/// The ender chest's model name.
const ENDER_CHEST_ITEM_MODEL: &str = "item/ender_chest";

/// What marks a parent as one the client builds itself.
const BUILTIN_PREFIX: &str = "builtin/";

/// What marks a face texture as a variable reference.
const VARIABLE_MARK: char = '#';

/// The client's name for the missing sprite, resolved by `builtin/missing`.
const MISSING_SPRITE: &str = "missingno";

/// The element rotation angles the client allows, in degrees.
const ROTATION_ANGLES: [f32; 5] = [0.0, 22.5, -22.5, 45.0, -45.0];

/// The quarter turns a face or variant rotation may state, in degrees.
const QUARTER_TURNS: [u16; 4] = [0, 90, 180, 270];

/// The smallest coordinate an element bound may state, in 1/16 units.
const MIN_BOUND: f32 = -16.0;

/// The largest coordinate an element bound may state, in 1/16 units.
const MAX_BOUND: f32 = 32.0;

/// The epsilon the client's `applyFacing` matches corners with.
const POSITION_EPSILON: f32 = 0.001;

/// The locations the client adds to the atlas beyond the variant scan
/// (`ModelBakery.LOCATIONS_BUILTIN_TEXTURES`): the liquid and
/// destroy-stage sheets, the armour-slot icons — and the three entity
/// sheets the built-in block models sample (`TileEntityChestRenderer`
/// and `TileEntitySignRenderer` bind them outside the block atlas, but
/// this port's chest and sign boards mesh through the terrain atlas, so
/// they stitch here or the boards read the fallback).
const BUILTIN_TEXTURE_LOCATIONS: [&str; 21] = [
    "blocks/water_flow",
    "blocks/water_still",
    "blocks/lava_flow",
    "blocks/lava_still",
    "blocks/destroy_stage_0",
    "blocks/destroy_stage_1",
    "blocks/destroy_stage_2",
    "blocks/destroy_stage_3",
    "blocks/destroy_stage_4",
    "blocks/destroy_stage_5",
    "blocks/destroy_stage_6",
    "blocks/destroy_stage_7",
    "blocks/destroy_stage_8",
    "blocks/destroy_stage_9",
    "items/empty_armor_slot_helmet",
    "items/empty_armor_slot_chestplate",
    "items/empty_armor_slot_leggings",
    "items/empty_armor_slot_boots",
    "entity/chest/normal",
    "entity/chest/normal_double",
    "entity/sign",
];

/// The item layers of a `builtin/generated` model, in the client's order.
const LAYERS: [&str; 5] = ["layer0", "layer1", "layer2", "layer3", "layer4"];

/// The block-local plane `builtin/generated` draws its layers on: the client's
/// `ItemModelGenerator` plane, `from` z 7.5 to `to` z 8.5 of 16.
const GENERATED_PLANE: f32 = 8.5 / 16.0;

/// One of the six directions a block face can point, in the order the 1.8
/// client enumerates them (`EnumFacing`: down, up, north, south, west, east).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FaceDir {
    /// The face pointing down.
    Down,
    /// The face pointing up.
    Up,
    /// The face pointing north (negative z).
    North,
    /// The face pointing south (positive z).
    South,
    /// The face pointing west (negative x).
    West,
    /// The face pointing east (positive x).
    East,
}

impl FaceDir {
    /// The six directions in the client's order.
    const ALL: [FaceDir; 6] = [
        FaceDir::Down,
        FaceDir::Up,
        FaceDir::North,
        FaceDir::South,
        FaceDir::West,
        FaceDir::East,
    ];

    /// The direction a name states, if it states one. Names are matched the
    /// way the client matches them: case-folded.
    fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "down" => Some(FaceDir::Down),
            "up" => Some(FaceDir::Up),
            "north" => Some(FaceDir::North),
            "south" => Some(FaceDir::South),
            "west" => Some(FaceDir::West),
            "east" => Some(FaceDir::East),
            _ => None,
        }
    }

    /// The direction's unit vector, in the client's axes (x east, y up, z
    /// south).
    fn vector(self) -> [f32; 3] {
        match self {
            FaceDir::Down => [0.0, -1.0, 0.0],
            FaceDir::Up => [0.0, 1.0, 0.0],
            FaceDir::North => [0.0, 0.0, -1.0],
            FaceDir::South => [0.0, 0.0, 1.0],
            FaceDir::West => [-1.0, 0.0, 0.0],
            FaceDir::East => [1.0, 0.0, 0.0],
        }
    }

    /// The axis the direction lies on.
    fn axis(self) -> Axis {
        match self {
            FaceDir::Down | FaceDir::Up => Axis::Y,
            FaceDir::North | FaceDir::South => Axis::Z,
            FaceDir::West | FaceDir::East => Axis::X,
        }
    }

    /// One quarter turn of the client's `EnumFacing.rotateAround`: clockwise
    /// seen from the axis' positive end. A direction on the axis is returned
    /// unchanged.
    fn rotate_around(self, axis: Axis) -> Self {
        match axis {
            Axis::X => match self {
                FaceDir::North => FaceDir::Down,
                FaceDir::Down => FaceDir::South,
                FaceDir::South => FaceDir::Up,
                FaceDir::Up => FaceDir::North,
                other => other,
            },
            Axis::Y => match self {
                FaceDir::North => FaceDir::East,
                FaceDir::East => FaceDir::South,
                FaceDir::South => FaceDir::West,
                FaceDir::West => FaceDir::North,
                other => other,
            },
            Axis::Z => match self {
                FaceDir::East => FaceDir::Down,
                FaceDir::Down => FaceDir::West,
                FaceDir::West => FaceDir::Up,
                FaceDir::Up => FaceDir::East,
                other => other,
            },
        }
    }
}

/// One of the three axes a rotation can turn about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// The x axis.
    X,
    /// The y axis.
    Y,
    /// The z axis.
    Z,
}

impl Axis {
    /// The axis a name states, if it states one, case-folded like the client.
    fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "x" => Some(Axis::X),
            "y" => Some(Axis::Y),
            "z" => Some(Axis::Z),
            _ => None,
        }
    }
}

/// Errors from reading, parsing or baking the blockstate and model trees.
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    /// A filesystem failure while walking or reading the tree.
    #[error("filesystem error at {path}: {reason}")]
    Io {
        /// The path involved.
        path: PathBuf,
        /// The operating system's reason.
        reason: String,
    },
    /// A document that is not JSON.
    #[error("not valid JSON: {source}")]
    Json {
        /// The parser's own error, with its line and column.
        source: serde_json::Error,
    },
    /// A value the 1.8 formats do not allow.
    #[error("{reason}")]
    Value {
        /// What is wrong, with the element or face it is wrong in.
        reason: String,
    },
    /// A file whose content failed to parse, named by its path.
    #[error("{path}: {source}")]
    File {
        /// The file.
        path: PathBuf,
        /// What the parser said about it.
        source: Box<ModelError>,
    },
    /// A blockstate file a lookup names that is not in the tree.
    #[error("the blockstate file {path} does not exist")]
    MissingBlockState {
        /// The path the lookup resolved to.
        path: PathBuf,
    },
    /// A model file a parent chain names that is not in the tree.
    #[error("the model file {path} does not exist")]
    MissingModel {
        /// The path the chain resolved to.
        path: PathBuf,
    },
    /// A parent chain that never ends.
    #[error("the parent chain cycles: {chain}")]
    ParentCycle {
        /// The chain, ` -> ` between each model, ending on the repeat.
        chain: String,
    },
    /// A texture variable that does not resolve to a path.
    #[error("{path}: the texture variable `#{variable}` does not resolve")]
    TextureVariable {
        /// The model whose face or particle named it.
        path: PathBuf,
        /// The variable's name, without its `#`.
        variable: String,
    },
    /// A `builtin/*` parent the client does not know.
    #[error("{path}: the parent `{parent}` names no builtin the client knows")]
    UnknownBuiltin {
        /// The model that named it.
        path: PathBuf,
        /// The parent as written.
        parent: String,
    },
    /// A model or parent name outside the `minecraft` namespace.
    #[error("the model `{name}` is outside the minecraft namespace")]
    ForeignNamespace {
        /// The name as written.
        name: String,
    },
    /// An item model the baker has no geometry for: the missing marker, the
    /// animated compass or clock, or a `builtin/entity` id outside the folded
    /// chest trio (their icons draw the missing sprite; recorded).
    #[error("the item model `{name}` bakes no geometry")]
    NoItemGeometry {
        /// The item model's name, as written.
        name: String,
    },
}

/// One variant of one blockstate: the model to bake and how to rotate it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    /// The model, as the blockstate writes it (usually a bare name under
    /// `models/block/`).
    pub model: String,
    /// The rotation about the x axis, one of 0, 90, 180 or 270 degrees.
    pub x: u16,
    /// The rotation about the y axis, one of 0, 90, 180 or 270 degrees.
    pub y: u16,
    /// Whether the uv stays locked to the block face while the geometry
    /// rotates.
    pub uvlock: bool,
    /// The variant's weight in its array, 1 when the file omits it. The
    /// mesher owns the position-based choice.
    pub weight: u32,
}

/// One block's blockstate: every variant key the file lists, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockStates {
    /// The variant keys (`` is a block with no properties), each with its
    /// one variant or its array.
    pub variants: BTreeMap<String, Vec<Variant>>,
}

/// One face of one element.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    /// The face's uv in 1/16 units, `[x1, y1, x2, y2]`; `None` takes the
    /// client's default from the element's bounds.
    pub uv: Option<[f32; 4]>,
    /// The texture variable, with its leading `#`, as the file writes it.
    pub texture: String,
    /// The face this face is culled against.
    pub cullface: Option<FaceDir>,
    /// The face's own quarter turns, 0 to 270 degrees; `down` turns
    /// counter-clockwise, the others clockwise.
    pub rotation: u16,
    /// The tint index, when the face states one other than -1.
    pub tintindex: Option<u8>,
}

/// One element's rotation about its own origin.
#[derive(Debug, Clone, PartialEq)]
pub struct ElementRotation {
    /// The rotation's origin, block-local (the client scales the file's 1/16
    /// units by 1/16 as it reads them).
    pub origin: [f32; 3],
    /// The axis turned about.
    pub axis: Axis,
    /// The angle in degrees; the client allows 0, ±22.5 and ±45.
    pub angle: f32,
    /// Whether the rotated axes are stretched back out by `1/cos(angle)`.
    pub rescale: bool,
}

/// One cuboid of a model, in 1/16 units.
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    /// The corner the client reads `WEST`, `DOWN` and `NORTH` from.
    pub from: [f32; 3],
    /// The corner the client reads `EAST`, `UP` and `SOUTH` from.
    pub to: [f32; 3],
    /// The element's own rotation, when it states one.
    pub rotation: Option<ElementRotation>,
    /// Whether the element takes the face shading; false for cross and torch
    /// models.
    pub shade: bool,
    /// The element's faces, keyed by direction.
    pub faces: BTreeMap<FaceDir, Face>,
}

/// One of a model's display transforms, as the file writes the numbers.
///
/// The applier's own rules stay with the applier: the client scales a stated
/// translation by 1/16 and clamps it to ±1.5, and clamps a stated scale to ±4
/// (`ItemTransformVec3f.Deserializer`), and a draw applies the transform as
/// translate, then the rotations y, x and z in that order, then scale
/// (`ItemCameraTransforms.applyTransform`); the composition is pinned with the
/// draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// The rotation in degrees, x, y, z.
    pub rotation: [f32; 3],
    /// The translation in the file's units (the applier scales it by 1/16).
    pub translation: [f32; 3],
    /// The scale.
    pub scale: [f32; 3],
}

impl Transform {
    /// The source's own default transform: rotation (0, 0, 0), translation
    /// (0, 0, 0), scale (1, 1, 1) (`ItemTransformVec3f.DEFAULT`).
    pub const DEFAULT: Transform = Transform {
        rotation: [0.0, 0.0, 0.0],
        translation: [0.0, 0.0, 0.0],
        scale: [1.0, 1.0, 1.0],
    };
}

/// One of the seven camera transform slots a model can carry
/// (`ItemCameraTransforms.TransformType`), in the source's own order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransformType {
    /// The fallback slot (`ItemCameraTransforms.getTransform`'s default arm).
    None,
    /// The third-person slot.
    ThirdPerson,
    /// The first-person slot.
    FirstPerson,
    /// The head slot.
    Head,
    /// The gui slot.
    Gui,
    /// The ground slot.
    Ground,
    /// The fixed (item frame) slot.
    Fixed,
}

/// A model's display section, completed: the seven camera transform slots,
/// every absent one filled from the source's own default.
///
/// The source fills an absent key as it deserializes
/// (`ItemCameraTransforms.func_181683_a` returns `ItemTransformVec3f.DEFAULT`),
/// and a stated transform that equals the default reads as unstated when a
/// chain resolves (`func_181687_c`). `none` is never a JSON key; it is the
/// source's own fallback slot (`getTransform`'s default arm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Display {
    /// The third-person transform.
    pub third_person: Transform,
    /// The first-person transform.
    pub first_person: Transform,
    /// The head transform.
    pub head: Transform,
    /// The gui transform.
    pub gui: Transform,
    /// The ground transform.
    pub ground: Transform,
    /// The fixed (item frame) transform.
    pub fixed: Transform,
    /// The none (fallback) transform, always the source's default.
    pub none: Transform,
}

impl Display {
    /// The source's own default set: every slot the identity transform
    /// (`ItemCameraTransforms.DEFAULT`, `ItemTransformVec3f.DEFAULT`).
    pub const DEFAULT: Display = Display {
        third_person: Transform::DEFAULT,
        first_person: Transform::DEFAULT,
        head: Transform::DEFAULT,
        gui: Transform::DEFAULT,
        ground: Transform::DEFAULT,
        fixed: Transform::DEFAULT,
        none: Transform::DEFAULT,
    };

    /// The transform a camera type's slot carries.
    pub fn get(&self, slot: TransformType) -> Transform {
        match slot {
            TransformType::None => self.none,
            TransformType::ThirdPerson => self.third_person,
            TransformType::FirstPerson => self.first_person,
            TransformType::Head => self.head,
            TransformType::Gui => self.gui,
            TransformType::Ground => self.ground,
            TransformType::Fixed => self.fixed,
        }
    }
}

/// One model file, as parsed.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelJson {
    /// The parent, as written, when the model states one.
    pub parent: Option<String>,
    /// The texture variables, each a path or a `#variable` reference.
    pub textures: BTreeMap<String, String>,
    /// The model's elements; empty for a model that only inherits.
    pub elements: Vec<Element>,
    /// Whether ambient occlusion applies; the client's default is true.
    pub ambient_occlusion: bool,
    /// The display section, completed — an absent slot carries the source's
    /// default — when the file states one.
    pub display: Option<Display>,
}

/// One baked face: four corners and their sprite coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct BakedQuad {
    /// The corners in block-local units (0..1), counter-clockwise seen from
    /// outside the block.
    pub corners: [[f32; 3]; 4],
    /// The sprite coordinates, corner for corner with [`BakedQuad::corners`],
    /// in the sprite's 0..1 space from its top-left corner.
    pub uv: [[f32; 2]; 4],
    /// The resolved texture path, like `blocks/stone`.
    pub texture: String,
    /// The direction the face is culled against, rotated with the variant.
    pub cullface: Option<FaceDir>,
    /// The tint index the face carries.
    pub tintindex: Option<u8>,
    /// Whether the element takes the face shading.
    pub shade: bool,
}

/// One model's quads, baked for one variant.
#[derive(Debug, Clone, PartialEq)]
pub struct BakedModel {
    /// The quads, in element order then the client's face order.
    pub quads: Vec<BakedQuad>,
    /// The resolved `ambientocclusion`.
    pub ambient_occlusion: bool,
    /// The resolved particle texture, when the model's chain resolves one.
    pub particle: Option<String>,
    /// True only for a model whose chain ends at `builtin/missing`.
    pub missing: bool,
}

/// The folded `builtin/entity` trio: the chest items whose icons draw through
/// the chest model rather than a baked model of their own
/// (`TileEntityItemStackRenderer.renderByItem`'s chest branches).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinItem {
    /// The plain chest.
    Chest,
    /// The trapped chest.
    TrappedChest,
    /// The ender chest.
    EnderChest,
}

impl BuiltinItem {
    /// The trio member a model resource names, when it names one: the three
    /// `models/item/<name>.json` files whose parent is `builtin/entity`.
    fn from_model_name(resource: &str) -> Option<Self> {
        match resource {
            CHEST_ITEM_MODEL => Some(BuiltinItem::Chest),
            TRAPPED_CHEST_ITEM_MODEL => Some(BuiltinItem::TrappedChest),
            ENDER_CHEST_ITEM_MODEL => Some(BuiltinItem::EnderChest),
            _ => None,
        }
    }

    /// The model resource the trio member registers under, as
    /// `models/item/<name>.json` writes it — the name [`ModelSource::bake_item`]
    /// resolves the member's chain by.
    pub fn model_name(self) -> &'static str {
        match self {
            BuiltinItem::Chest => CHEST_ITEM_MODEL,
            BuiltinItem::TrappedChest => TRAPPED_CHEST_ITEM_MODEL,
            BuiltinItem::EnderChest => ENDER_CHEST_ITEM_MODEL,
        }
    }

    /// The icon sheet the trio's draw binds, as the texture tree keys it.
    ///
    /// The names are the TESRs' own `ResourceLocation`s with the `.png` the
    /// tree's keys drop (`TileEntityChestRenderer`'s `textureNormal` and
    /// `textureTrapped`, `TileEntityEnderChestRenderer`'s
    /// `ENDER_CHEST_TEXTURE`). The christmas sheets the chest renderer picks
    /// for December 24–26 are not folded (recorded).
    pub fn icon_sheet(self) -> &'static str {
        match self {
            BuiltinItem::Chest => "entity/chest/normal",
            BuiltinItem::TrappedChest => "entity/chest/trapped",
            BuiltinItem::EnderChest => "entity/chest/ender",
        }
    }
}

/// One box of the chest model: the corners `ModelBox` builds, in 1/16 units,
/// and the rotation point its renderer places it by.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChestBox {
    /// The box's first corner (`ModelBox.posX1/posY1/posZ1`).
    pub from: [f32; 3],
    /// The box's second corner, the first plus the box's size
    /// (`ModelBox.posX2/posY2/posZ2`).
    pub to: [f32; 3],
    /// The rotation point the renderer translates the box by before drawing
    /// it (`ModelRenderer.render`).
    pub origin: [f32; 3],
}

/// The chest model's own scale: the renderers draw every box at 1/16
/// (`ModelChest.renderAll`'s argument).
pub const CHEST_MODEL_SCALE: f32 = 0.0625;

/// The chest trio's small model: the boxes `ModelChest` builds, in its own
/// render order — the lid, the knob, the base.
///
/// Each box is the `addBox` call's corners and the renderer's rotation point,
/// in 1/16 units, before [`CHEST_MODEL_SCALE`]; the draw composes the two.
/// The lid's rotation (the open angle) is draw state and stays with the draw.
pub const CHEST_MODEL: [ChestBox; 3] = [
    // The lid: addBox(0, -5, -14, 14, 5, 14), rotation point (1, 7, 15).
    ChestBox {
        from: [0.0, -5.0, -14.0],
        to: [14.0, 0.0, 0.0],
        origin: [1.0, 7.0, 15.0],
    },
    // The knob: addBox(-1, -2, -15, 2, 4, 1), rotation point (8, 7, 15).
    ChestBox {
        from: [-1.0, -2.0, -15.0],
        to: [1.0, 2.0, -14.0],
        origin: [8.0, 7.0, 15.0],
    },
    // The base: addBox(0, 0, 0, 14, 10, 14), rotation point (1, 6, 1).
    ChestBox {
        from: [0.0, 0.0, 0.0],
        to: [14.0, 10.0, 14.0],
        origin: [1.0, 6.0, 1.0],
    },
];

/// Where one item's model comes from, resolved against the tree.
///
/// The classes are the client's item pass's own (`ModelBakery.bakeItemModels`
/// splits the item locations three ways: the generated items go through
/// `ItemModelGenerator`, the `builtin/entity` ids become builtin models, and
/// the rest bake as their own model).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemModelSource {
    /// A block item: its chain resolves to a block model's geometry. Carries
    /// the block model's resource name.
    Block(String),
    /// A generated item: its `layer0…4` texture paths in order, stopping at
    /// the first layer that does not resolve
    /// (`ItemModelGenerator.makeItemModel`).
    Generated(Vec<String>),
    /// One of the folded chest trio, drawn through the chest model.
    Builtin(BuiltinItem),
    /// Nothing resolves.
    Missing,
}

/// The item model sources one name list resolves to, in the list's order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ItemModelSet {
    /// One source per name, in the input order.
    pub sources: Vec<ItemModelSource>,
    /// How many of them resolved to [`ItemModelSource::Missing`].
    pub missing: usize,
}

/// One item's baked model: its geometry, its resolved textures and its
/// completed display transforms.
#[derive(Debug, Clone, PartialEq)]
pub struct BakedItem {
    /// The geometry quads: a generated item's layer planes or a block item's
    /// baked faces. Empty for the chest trio, whose geometry is
    /// [`BakedItem::boxes`].
    pub quads: Vec<BakedQuad>,
    /// The chest trio's box model ([`CHEST_MODEL`]); empty for the other
    /// classes.
    pub boxes: Vec<ChestBox>,
    /// The resolved texture paths the item's draw samples, the particle first
    /// when the chain resolves one, then the geometry's own textures, each
    /// once, in first-seen order (`ModelBakery.getItemsTextureLocations` adds
    /// the particle and then the layers or the element faces).
    pub textures: Vec<String>,
    /// The display transforms, completed: every slot resolved through the
    /// chain, an absent slot filled from the source's default.
    pub display: Display,
}

/// Where a parent chain ends, and what the end carries.
#[derive(Debug, Clone)]
enum ChainEnd {
    /// At a model file: the elements and occlusion that apply.
    File {
        /// The end model's elements.
        elements: Vec<Element>,
        /// The end model's `ambientocclusion`.
        ambient_occlusion: bool,
    },
    /// At a `builtin/*` parent the client builds itself.
    Builtin(Builtin),
}

/// The `builtin/*` parents the client knows.
#[derive(Debug, Clone, Copy)]
enum Builtin {
    /// `builtin/missing`: the missing-model marker.
    Missing,
    /// `builtin/generated`: the generated item layers.
    Generated,
    /// `builtin/compass`: the animated compass.
    Compass,
    /// `builtin/clock`: the animated clock.
    Clock,
    /// `builtin/entity`: a block-entity renderer.
    Entity,
}

impl Builtin {
    /// The builtin a parent's name states, if the client knows it.
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "missing" => Some(Builtin::Missing),
            "generated" => Some(Builtin::Generated),
            "compass" => Some(Builtin::Compass),
            "clock" => Some(Builtin::Clock),
            "entity" => Some(Builtin::Entity),
            _ => None,
        }
    }

    /// The client's marker models state no `ambientocclusion` of their own,
    /// so it stays true; the generated item model is built with it off.
    fn ambient_occlusion(self) -> bool {
        !matches!(self, Builtin::Generated)
    }
}

/// A model and its ancestors, leaf first.
#[derive(Debug, Clone)]
struct Chain {
    /// The resource paths, leaf first, ending on the chain's end.
    paths: Vec<String>,
    /// The models, in the same order.
    models: Vec<ModelJson>,
    /// Where the chain ends.
    end: ChainEnd,
}

impl Chain {
    /// The client's texture-variable resolution: the first model in the
    /// chain that defines `variable` supplies the value, a value that is
    /// itself a reference is resolved from the leaf again, and a reference
    /// that comes back around to the model it was found in does not resolve.
    fn texture(&self, variable: &str) -> Option<String> {
        self.resolve(0, variable, &mut None)
    }

    /// One step of [`Chain::texture`], from `model` towards the root.
    fn resolve(&self, model: usize, variable: &str, found: &mut Option<usize>) -> Option<String> {
        if *found == Some(model) {
            return None;
        }
        let mut value = self.models.get(model)?.textures.get(variable).cloned();
        if value.is_none() && model + 1 < self.models.len() {
            value = self.resolve(model + 1, variable, found);
        }
        *found = Some(model);
        match value {
            Some(value) if value.starts_with(VARIABLE_MARK) => {
                self.resolve(0, value.trim_start_matches(VARIABLE_MARK), found)
            }
            other => other,
        }
    }

    /// The display transforms of this chain, resolved the way the source
    /// resolves them (`ModelBlock.getTransform`): each slot is the first
    /// model's own, walking the chain from the leaf; a slot no model states —
    /// and a stated transform that equals the default, which
    /// `ItemCameraTransforms.func_181687_c` reads as unstated — stays the
    /// source's default.
    fn display(&self) -> Display {
        Display {
            third_person: self.display_slot(TransformType::ThirdPerson),
            first_person: self.display_slot(TransformType::FirstPerson),
            head: self.display_slot(TransformType::Head),
            gui: self.display_slot(TransformType::Gui),
            ground: self.display_slot(TransformType::Ground),
            fixed: self.display_slot(TransformType::Fixed),
            none: Transform::DEFAULT,
        }
    }

    /// One slot of [`Chain::display`].
    fn display_slot(&self, slot: TransformType) -> Transform {
        for model in &self.models {
            let stated = model
                .display
                .map(|display| display.get(slot))
                .unwrap_or(Transform::DEFAULT);
            if stated != Transform::DEFAULT {
                return stated;
            }
        }
        Transform::DEFAULT
    }
}

/// Every blockstate and model an extraction tree holds, with its caches.
///
/// The tree is read whole by [`ModelSource::open`]: every blockstate file and
/// every model file under the two directories parses before the source
/// exists, and a file that does not is an error naming it. Parent chains and
/// bakes are resolved on first use and kept.
#[derive(Debug)]
pub struct ModelSource {
    /// The blockstates directory, for error paths.
    blockstates_dir: PathBuf,
    /// The models directory, for error paths.
    models_dir: PathBuf,
    /// The blockstates, keyed by their file name without the extension.
    blockstates: BTreeMap<String, BlockStates>,
    /// The models, keyed by their path below `models/` without the extension
    /// (`block/cube`).
    models: BTreeMap<String, ModelJson>,
    /// The resolved parent chains, keyed like `models`.
    chains: RefCell<BTreeMap<String, Chain>>,
    /// The bakes, keyed by (model, x, y, uvlock).
    bakes: RefCell<BTreeMap<(String, u16, u16, bool), BakedModel>>,
    /// The texture paths, computed once.
    texture_paths: RefCell<Option<BTreeSet<String>>>,
}

impl ModelSource {
    /// Reads every blockstate and model file under `extraction_root`.
    ///
    /// `extraction_root` is what
    /// [`Extractor::root`](crate::extract::Extractor::root) returns —
    /// `<store root>/extracted/<version>`. Both
    /// `assets/minecraft/blockstates/` and `assets/minecraft/models/` must
    /// exist below it; a wrong root is an error naming the directory. Every
    /// `.json` under both is parsed in sorted order, so the same tree fails on
    /// the same file on every host.
    pub fn open(extraction_root: &Path) -> Result<Self, ModelError> {
        let blockstates_dir = extraction_root.join(BLOCKSTATES_DIR);
        let models_dir = extraction_root.join(MODELS_DIR);
        for dir in [&blockstates_dir, &models_dir] {
            if !dir.is_dir() {
                return Err(ModelError::Io {
                    path: dir.clone(),
                    reason: "the directory is not in the extraction tree".to_string(),
                });
            }
        }

        let mut blockstates = BTreeMap::new();
        for relative in collect_json(&blockstates_dir)? {
            let path = blockstates_dir.join(&relative);
            let name = path_stem(&relative)?;
            let text = read_text(&path)?;
            let states = BlockStates::parse(&text).map_err(|source| ModelError::File {
                path: path.clone(),
                source: Box::new(source),
            })?;
            blockstates.insert(name, states);
        }

        let mut models = BTreeMap::new();
        for relative in collect_json(&models_dir)? {
            let path = models_dir.join(&relative);
            let key = resource_path(&relative)?;
            let text = read_text(&path)?;
            let model = ModelJson::parse(&text).map_err(|source| ModelError::File {
                path: path.clone(),
                source: Box::new(source),
            })?;
            models.insert(key, model);
        }

        Ok(Self {
            blockstates_dir,
            models_dir,
            blockstates,
            models,
            chains: RefCell::new(BTreeMap::new()),
            bakes: RefCell::new(BTreeMap::new()),
            texture_paths: RefCell::new(None),
        })
    }

    /// The blockstate file `block_name` names, parsed and cached.
    ///
    /// `block_name` is the file's name without its `.json` extension — the
    /// name the client's own blockstate lookup uses (`stone`, `torch`,
    /// `oak_fence`).
    pub fn blockstates(&self, block_name: &str) -> Result<&BlockStates, ModelError> {
        self.blockstates
            .get(block_name)
            .ok_or_else(|| ModelError::MissingBlockState {
                path: self
                    .blockstates_dir
                    .join(format!("{block_name}.{JSON_EXTENSION}")),
            })
    }

    /// Bakes one variant's model into its quads, cached per variant shape.
    ///
    /// The variant's rotation moves the geometry about the block's centre
    /// (0.5, 0.5, 0.5), x first and then y, following the client's
    /// `ModelRotation`; with `uvlock` the uv is re-projected onto the face the
    /// quad ends up on, so the texture stays locked to the block face while
    /// the geometry turns. The variant's `x` and `y` must be quarter turns —
    /// multiples of 90, the only values the 1.8 formats produce; a non-quarter
    /// value is floored to its quarter rather than refused. Weighted arrays
    /// stay arrays: this bakes one entry,
    /// and the mesher owns the position-based choice between them.
    pub fn bake_variant(&self, variant: &Variant) -> Result<BakedModel, ModelError> {
        let key = (variant.model.clone(), variant.x, variant.y, variant.uvlock);
        if let Some(baked) = self.bakes.borrow().get(&key) {
            return Ok(baked.clone());
        }
        let chain = self.chain_for(&variant.model)?;
        let baked = self.bake(&chain, variant)?;
        self.bakes.borrow_mut().insert(key, baked.clone());
        Ok(baked)
    }

    /// Resolves one item model name into its source.
    ///
    /// `name` is the name the item registry states: a bare name is the item
    /// tree's own (`ModelBakery.getItemLocation` maps an item registration's
    /// name to `item/<name>`), a path that already names one is taken as
    /// written. Anything that does not resolve — an absent file, a broken or
    /// foreign chain, a builtin outside the folded set — degrades to
    /// [`ItemModelSource::Missing`] rather than failing: the source's own item
    /// walk skips an item that does not bake with a warning
    /// (`ModelBakery.loadItemModels:283-286`; the block-side twin warns at
    /// `bakeBlockModels:411`), and the client's item pass never fails
    /// its load over one.
    pub fn item_model(&self, name: &str) -> ItemModelSource {
        let Ok(resource) = item_model_resource(name) else {
            return ItemModelSource::Missing;
        };
        let Ok(chain) = self.chain_for_resource(&resource) else {
            return ItemModelSource::Missing;
        };
        match &chain.end {
            ChainEnd::Builtin(Builtin::Generated) => {
                ItemModelSource::Generated(chain_layers(&chain))
            }
            ChainEnd::Builtin(Builtin::Entity) => match BuiltinItem::from_model_name(&resource) {
                Some(item) => ItemModelSource::Builtin(item),
                None => ItemModelSource::Missing,
            },
            ChainEnd::Builtin(_) => ItemModelSource::Missing,
            ChainEnd::File { .. } => ItemModelSource::Block(block_member(&chain, &resource)),
        }
    }

    /// Resolves a list of item model names, counting the misses.
    pub fn item_models(&self, names: &[&str]) -> ItemModelSet {
        let sources: Vec<ItemModelSource> =
            names.iter().map(|name| self.item_model(name)).collect();
        let missing = sources
            .iter()
            .filter(|source| matches!(source, ItemModelSource::Missing))
            .count();
        ItemModelSet { sources, missing }
    }

    /// The item file whose chain's first `block/` hop is `member`.
    ///
    /// The source's item bake starts at `models/item/<name>.json`
    /// (`ModelBakery.getItemLocation`:350-353) and walks down into the block
    /// model, and a block item's display slots live on that item file — the
    /// chain's transform lookup walks up from the item's own model
    /// (`ModelBlock.getTransform`:177-179). [`ItemModelSource::Block`] carries
    /// the block member alone, so this answers the file above it: the shortest
    /// `item/` path whose own parent resolves to `member`, lexicographically
    /// first on a tie. A tie means several items parent one block model
    /// (`block/stone`'s monster-egg file beside the item's own); the shipping
    /// tree's tied files state the same display slots.
    pub fn item_model_above(&self, member: &str) -> Option<String> {
        self.models
            .iter()
            .filter(|(path, model)| {
                path.starts_with(ITEM_MODEL_PREFIX)
                    && model
                        .parent
                        .as_deref()
                        .and_then(|parent| path_of(parent).ok())
                        == Some(member)
            })
            .map(|(path, _)| path)
            .min_by_key(|path| (path.len(), path.as_str()))
            .cloned()
    }

    /// The item file a generated row's own layer list regenerates.
    ///
    /// The row's layers are the `layer0…4` paths its chain resolves — the
    /// same list [`ItemModelSource::Generated`] carries — and the file that
    /// regenerates exactly that list is the row's model file: the generator
    /// writes the item's file from the item's own sprites
    /// (`ItemModelGenerator`'s naming rule pairs the sprite `items/<name>`
    /// with the file `item/<name>`), so the full layer list — not one
    /// sprite's basename — names the file. The shortest `item/` path wins a
    /// tie, lexicographically first, matching [`ModelSource::item_model_above`];
    /// a tie means two files regenerate one list (the shipping tree's ties
    /// state the same display slots).
    pub fn generated_item_file(&self, layers: &[&str]) -> Option<String> {
        self.models
            .iter()
            .filter(|(path, _)| path.starts_with(ITEM_MODEL_PREFIX))
            .filter(|(path, _)| {
                self.chain_for_resource(path)
                    .map(|chain| {
                        chain_layers(&chain)
                            .iter()
                            .map(String::as_str)
                            .eq(layers.iter().copied())
                    })
                    .unwrap_or(false)
            })
            .map(|(path, _)| path)
            .min_by_key(|path| (path.len(), path.as_str()))
            .cloned()
    }

    /// Bakes one item model by name.
    ///
    /// The classes bake the way the source's item pass bakes them
    /// (`ModelBakery.bakeItemModels`): a generated item bakes one layer plane
    /// per resolved `layer0…4` through the same path
    /// [`ModelSource::bake_variant`] uses for `builtin/generated`, a block
    /// item bakes its chain's elements, and the folded chest trio bakes its
    /// box list ([`CHEST_MODEL`]). Every class's display transforms complete
    /// through its chain.
    ///
    /// A name that resolves to no geometry errors: a name the chain cannot
    /// resolve answers the chain's own error, and the builtin ends outside
    /// the trio — the missing marker, the animated compass or clock, a
    /// `builtin/entity` id that is not one of the trio — answer
    /// [`ModelError::NoItemGeometry`]. The resolution
    /// ([`ModelSource::item_model`]) is where a miss degrades to
    /// [`ItemModelSource::Missing`] instead.
    pub fn bake_item(&self, name: &str) -> Result<BakedItem, ModelError> {
        let resource = item_model_resource(name)?;
        let chain = self.chain_for_resource(&resource)?;
        let display = chain.display();
        match &chain.end {
            ChainEnd::Builtin(Builtin::Generated) => {
                let quads = generated_quads(&chain);
                let mut textures = Vec::new();
                if let Some(particle) = chain.texture("particle") {
                    push_texture(&mut textures, &particle);
                }
                for quad in &quads {
                    push_texture(&mut textures, &quad.texture);
                }
                Ok(BakedItem {
                    quads,
                    boxes: Vec::new(),
                    textures,
                    display,
                })
            }
            ChainEnd::Builtin(Builtin::Entity) => match BuiltinItem::from_model_name(&resource) {
                Some(item) => Ok(BakedItem {
                    quads: Vec::new(),
                    boxes: CHEST_MODEL.to_vec(),
                    textures: vec![item.icon_sheet().to_string()],
                    display,
                }),
                None => Err(ModelError::NoItemGeometry {
                    name: name.to_string(),
                }),
            },
            ChainEnd::Builtin(_) => Err(ModelError::NoItemGeometry {
                name: name.to_string(),
            }),
            ChainEnd::File { .. } => {
                let variant = Variant {
                    model: resource.clone(),
                    x: 0,
                    y: 0,
                    uvlock: false,
                    weight: 1,
                };
                let baked = self.bake(&chain, &variant)?;
                let mut textures = Vec::new();
                if let Some(particle) = &baked.particle {
                    push_texture(&mut textures, particle);
                }
                for quad in &baked.quads {
                    push_texture(&mut textures, &quad.texture);
                }
                Ok(BakedItem {
                    quads: baked.quads,
                    boxes: Vec::new(),
                    textures,
                    display,
                })
            }
        }
    }

    /// Every texture path the tree's blockstate files resolve, for the atlas.
    ///
    /// The set is the resolved texture of every face of every element of every
    /// model the blockstate files name (following each chain's own `textures`
    /// maps, including the particle), plus the locations of
    /// `ModelBakery.LOCATIONS_BUILTIN_TEXTURES` — the liquid, destroy-stage
    /// and armour-slot locations the client stitches beyond the variant scan.
    /// The client's own missing sprite (`missingno`, the atlas's fallback) is
    /// not a path in the set: the atlas builds it itself.
    ///
    /// A blockstate or chain that does not load contributes nothing rather
    /// than failing the set; the tree's parse happens in [`ModelSource::open`],
    /// and the real tree resolves whole. The answer is computed once and
    /// cached.
    pub fn texture_paths(&self) -> BTreeSet<String> {
        if let Some(paths) = self.texture_paths.borrow().as_ref() {
            return paths.clone();
        }
        let mut paths: BTreeSet<String> = BUILTIN_TEXTURE_LOCATIONS
            .iter()
            .map(|path| path.to_string())
            .collect();
        for states in self.blockstates.values() {
            for variant in states.variants.values().flatten() {
                let Ok(chain) = self.chain_for(&variant.model) else {
                    continue;
                };
                if let ChainEnd::File { elements, .. } = &chain.end {
                    for element in elements {
                        for face in element.faces.values() {
                            let variable = face.texture.trim_start_matches(VARIABLE_MARK);
                            collect_texture(&chain, variable, &mut paths);
                        }
                    }
                }
                collect_texture(&chain, "particle", &mut paths);
            }
        }
        *self.texture_paths.borrow_mut() = Some(paths.clone());
        paths
    }

    /// The chain `model` names, resolved and cached.
    ///
    /// `model` is the name a variant states: a `minecraft:` prefix is
    /// accepted, the path is taken relative to `models/block/`, and the chain
    /// walks the `parent` fields from there. `builtin/*` ends a chain.
    fn chain_for(&self, model: &str) -> Result<Chain, ModelError> {
        self.chain_for_resource(&block_model_resource(model)?)
    }

    /// The chain a model resource names, resolved and cached.
    ///
    /// `resource` is a path below `models/` (`block/stone`, `item/chest`);
    /// the chain walks the `parent` fields from there. `builtin/*` ends a
    /// chain.
    fn chain_for_resource(&self, resource: &str) -> Result<Chain, ModelError> {
        if let Some(chain) = self.chains.borrow().get(resource) {
            return Ok(chain.clone());
        }

        let mut paths = Vec::new();
        let mut models = Vec::new();
        let mut current = resource.to_string();
        let end = loop {
            if let Some(position) = paths.iter().position(|path| path == &current) {
                let mut cycle = paths[position..].to_vec();
                cycle.push(current);
                return Err(ModelError::ParentCycle {
                    chain: cycle.join(" -> "),
                });
            }
            let path_here = self.model_file_path(&current);
            let Some(loaded) = self.models.get(&current) else {
                return Err(ModelError::MissingModel { path: path_here });
            };
            paths.push(current);
            models.push(loaded.clone());
            match loaded.parent.clone() {
                None => {
                    break ChainEnd::File {
                        elements: loaded.elements.clone(),
                        ambient_occlusion: loaded.ambient_occlusion,
                    };
                }
                Some(parent) if parent.starts_with(BUILTIN_PREFIX) => {
                    let name = &parent[BUILTIN_PREFIX.len()..];
                    let Some(builtin) = Builtin::from_name(name) else {
                        return Err(ModelError::UnknownBuiltin {
                            path: path_here,
                            parent,
                        });
                    };
                    break ChainEnd::Builtin(builtin);
                }
                Some(parent) => current = resource_of(&parent)?,
            }
        };

        let chain = Chain { paths, models, end };
        self.chains
            .borrow_mut()
            .insert(resource.to_string(), chain.clone());
        Ok(chain)
    }

    /// The path of a model resource, for error messages.
    fn model_file_path(&self, resource: &str) -> PathBuf {
        self.models_dir.join(format!("{resource}.{JSON_EXTENSION}"))
    }

    /// Bakes a resolved chain for one variant.
    fn bake(&self, chain: &Chain, variant: &Variant) -> Result<BakedModel, ModelError> {
        let path = self.model_file_path(chain.paths.first().map_or("", String::as_str));
        let elements = match &chain.end {
            ChainEnd::Builtin(builtin) => {
                return Ok(BakedModel {
                    quads: match builtin {
                        Builtin::Generated => generated_quads(chain),
                        _ => Vec::new(),
                    },
                    ambient_occlusion: builtin.ambient_occlusion(),
                    particle: match builtin {
                        Builtin::Missing => Some(MISSING_SPRITE.to_string()),
                        Builtin::Generated => chain
                            .texture("particle")
                            .or_else(|| chain.texture(LAYERS[0])),
                        Builtin::Compass | Builtin::Clock | Builtin::Entity => {
                            chain.texture("particle")
                        }
                    },
                    missing: matches!(builtin, Builtin::Missing),
                });
            }
            ChainEnd::File { elements, .. } => elements,
        };

        // Floors a non-quarter value to its quarter (`bake_variant`'s precondition note).
        let rotation = VariantRotation {
            quarters_x: (u32::from(variant.x) / 90) % 4,
            quarters_y: (u32::from(variant.y) / 90) % 4,
            uvlock: variant.uvlock,
        };
        let mut quads = Vec::new();
        for element in elements {
            for (face, part) in &element.faces {
                let variable = part.texture.trim_start_matches(VARIABLE_MARK);
                let Some(texture) = chain.texture(variable) else {
                    return Err(ModelError::TextureVariable {
                        path: path.clone(),
                        variable: variable.to_string(),
                    });
                };
                quads.push(bake_face(element, *face, part, &texture, rotation));
            }
        }
        Ok(BakedModel {
            quads,
            ambient_occlusion: chain.end.ambient_occlusion(),
            particle: chain.texture("particle"),
            missing: false,
        })
    }
}

impl ChainEnd {
    /// The ambient occlusion a chain end applies.
    fn ambient_occlusion(&self) -> bool {
        match self {
            ChainEnd::File {
                ambient_occlusion, ..
            } => *ambient_occlusion,
            ChainEnd::Builtin(builtin) => builtin.ambient_occlusion(),
        }
    }
}

/// The variant rotation a bake applies: quarter turns about x then y, about
/// the block's centre, with or without the uv lock.
#[derive(Debug, Clone, Copy)]
struct VariantRotation {
    /// Quarter turns about the x axis (0..4).
    quarters_x: u32,
    /// Quarter turns about the y axis (0..4).
    quarters_y: u32,
    /// Whether the uv stays locked to the block face.
    uvlock: bool,
}

impl VariantRotation {
    /// True when the rotation leaves everything where it is.
    fn is_identity(&self) -> bool {
        self.quarters_x == 0 && self.quarters_y == 0
    }

    /// One rotated corner.
    fn corner(&self, corner: [f32; 3]) -> [f32; 3] {
        let mut corner = corner;
        for _ in 0..self.quarters_x {
            corner = step_x(corner);
        }
        for _ in 0..self.quarters_y {
            corner = step_y(corner);
        }
        corner
    }

    /// The slot a corner's data ends up in after the rotation reorders the
    /// quad's vertices (the client's `ModelRotation.rotateVertex`).
    fn vertex_slot(&self, face: FaceDir, vertex: usize) -> usize {
        let mut slot = vertex;
        if face.axis() == Axis::X {
            slot = (vertex + self.quarters_x as usize) % 4;
        }
        let mut rotated = face;
        for _ in 0..self.quarters_x {
            rotated = rotated.rotate_around(Axis::X);
        }
        if rotated.axis() == Axis::Y {
            slot = (slot + self.quarters_y as usize) % 4;
        }
        slot
    }

    /// The direction a face ends up on (the client's
    /// `ModelRotation.rotateFace`).
    fn face(&self, face: FaceDir) -> FaceDir {
        let mut rotated = face;
        for _ in 0..self.quarters_x {
            rotated = rotated.rotate_around(Axis::X);
        }
        if rotated.axis() != Axis::Y {
            for _ in 0..self.quarters_y {
                rotated = rotated.rotate_around(Axis::Y);
            }
        }
        rotated
    }
}

/// One quarter turn about the x axis, the client's `rotateX` step: north to
/// down, about the block's centre.
fn step_x(corner: [f32; 3]) -> [f32; 3] {
    let x = corner[0] - 0.5;
    let y = corner[1] - 0.5;
    let z = corner[2] - 0.5;
    [x + 0.5, z + 0.5, -y + 0.5]
}

/// One quarter turn about the y axis, the client's `rotateY` step: north to
/// east, about the block's centre.
fn step_y(corner: [f32; 3]) -> [f32; 3] {
    let x = corner[0] - 0.5;
    let y = corner[1] - 0.5;
    let z = corner[2] - 0.5;
    [-z + 0.5, y + 0.5, x + 0.5]
}

/// The client's `FaceBakery.getPositionsDiv16` array: an element's bounds in
/// the order `EnumFacing` enumerates its indices (0 from.y, 1 to.y, 2 from.z,
/// 3 to.z, 4 from.x, 5 to.x), in block-local units.
fn slot_values(from: [f32; 3], to: [f32; 3]) -> [f32; 6] {
    hull_slots(
        [from[0] / 16.0, from[1] / 16.0, from[2] / 16.0],
        [to[0] / 16.0, to[1] / 16.0, to[2] / 16.0],
    )
}

/// The same six slots for a hull that is already in block-local units.
fn hull_slots(from: [f32; 3], to: [f32; 3]) -> [f32; 6] {
    [from[1], to[1], from[2], to[2], from[0], to[0]]
}

/// The client's `EnumFaceDirection` vertex order: per face, the four corners
/// as triples of the slot indices [`slot_values`] builds.
const FACE_VERTICES: [[[u8; 3]; 4]; 6] = [
    [[4, 0, 3], [4, 0, 2], [5, 0, 2], [5, 0, 3]],
    [[4, 1, 2], [4, 1, 3], [5, 1, 3], [5, 1, 2]],
    [[5, 1, 2], [5, 0, 2], [4, 0, 2], [4, 1, 2]],
    [[4, 1, 3], [4, 0, 3], [5, 0, 3], [5, 1, 3]],
    [[4, 1, 2], [4, 0, 2], [4, 0, 3], [4, 1, 3]],
    [[5, 1, 3], [5, 0, 3], [5, 0, 2], [5, 1, 2]],
];

/// The corner a face's `vertex` reads its components from.
fn corner_of(face: FaceDir, vertex: usize, slots: &[f32; 6]) -> [f32; 3] {
    let indices = FACE_VERTICES[face as usize][vertex];
    [
        slots[indices[0] as usize],
        slots[indices[1] as usize],
        slots[indices[2] as usize],
    ]
}

/// The client's default uv for a face with no `uv`: the element's bounds
/// projected per direction, in 1/16 units.
fn default_uv(face: FaceDir, from: [f32; 3], to: [f32; 3]) -> [f32; 4] {
    match face {
        FaceDir::Down | FaceDir::Up => [from[0], from[2], to[0], to[2]],
        FaceDir::North | FaceDir::South => [from[0], 16.0 - to[1], to[0], 16.0 - from[1]],
        FaceDir::West | FaceDir::East => [from[2], 16.0 - to[1], to[2], 16.0 - from[1]],
    }
}

/// The uv corner the client's `BlockFaceUV` reads for a vertex index, in
/// 1/16 units: the face's own quarter turns step the corner through the
/// cycle (u0,v0), (u0,v1), (u1,v1), (u1,v0).
fn uv_corner(rotation: u16, uv: [f32; 4], vertex: usize) -> [f32; 2] {
    let step = (vertex + (rotation as usize / 90)) % 4;
    let u = if step == 2 || step == 3 { uv[2] } else { uv[0] };
    let v = if step == 1 || step == 2 { uv[3] } else { uv[1] };
    [u, v]
}

/// The element rotation's rescale factors: the two axes across the rotation
/// axis stretch by `1/cos(angle)`, the axis itself does not.
fn part_scale(rotation: &ElementRotation) -> [f32; 3] {
    if !rotation.rescale {
        return [1.0, 1.0, 1.0];
    }
    let factor = if rotation.angle.abs() == 22.5 {
        1.0 / (22.5_f32).to_radians().cos() - 1.0
    } else {
        1.0 / std::f32::consts::FRAC_PI_4.cos() - 1.0
    };
    match rotation.axis {
        Axis::X => [1.0, 1.0 + factor, 1.0 + factor],
        Axis::Y => [1.0 + factor, 1.0, 1.0 + factor],
        Axis::Z => [1.0 + factor, 1.0 + factor, 1.0],
    }
}

/// One corner turned by the element's own rotation, about its origin.
fn rotate_part(corner: [f32; 3], rotation: &ElementRotation) -> [f32; 3] {
    let angle = rotation.angle.to_radians();
    let (sin, cos) = angle.sin_cos();
    let x = corner[0] - rotation.origin[0];
    let y = corner[1] - rotation.origin[1];
    let z = corner[2] - rotation.origin[2];
    let turned = match rotation.axis {
        Axis::X => [x, y * cos - z * sin, y * sin + z * cos],
        Axis::Y => [x * cos + z * sin, y, -x * sin + z * cos],
        Axis::Z => [x * cos - y * sin, x * sin + y * cos, z],
    };
    let scale = part_scale(rotation);
    [
        turned[0] * scale[0] + rotation.origin[0],
        turned[1] * scale[1] + rotation.origin[1],
        turned[2] * scale[2] + rotation.origin[2],
    ]
}

/// The direction a quad's corners wind about (the client's
/// `FaceBakery.getFacingFromVertexData`): the first direction with a
/// non-negative best dot, up when nothing matches.
fn facing_from(corners: &[[f32; 3]; 4]) -> FaceDir {
    let a = [
        corners[0][0] - corners[1][0],
        corners[0][1] - corners[1][1],
        corners[0][2] - corners[1][2],
    ];
    let b = [
        corners[2][0] - corners[1][0],
        corners[2][1] - corners[1][1],
        corners[2][2] - corners[1][2],
    ];
    let normal = [
        b[1] * a[2] - b[2] * a[1],
        b[2] * a[0] - b[0] * a[2],
        b[0] * a[1] - b[1] * a[0],
    ];
    let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if length == 0.0 {
        return FaceDir::Up;
    }
    let normal = [normal[0] / length, normal[1] / length, normal[2] / length];
    let mut best = FaceDir::Up;
    let mut best_dot = 0.0;
    for face in FaceDir::ALL {
        let vector = face.vector();
        let dot = normal[0] * vector[0] + normal[1] * vector[1] + normal[2] * vector[2];
        if dot >= 0.0 && dot > best_dot {
            best_dot = dot;
            best = face;
        }
    }
    best
}

/// Re-projects each corner's uv onto the face the quad ended up on, the
/// client's `FaceBakery.lockUv`: the texture stays where the block face puts
/// it while the geometry turns.
fn lock_uv(corners: &[[f32; 3]; 4], uv: &mut [[f32; 2]; 4], face: FaceDir, rotation: u16) {
    let step = (rotation as usize / 90) % 4;
    for (index, corner) in corners.iter().enumerate() {
        let mut x = corner[0];
        let mut y = corner[1];
        let mut z = corner[2];
        for value in [&mut x, &mut y, &mut z] {
            if *value < -0.1 || *value >= 1.1 {
                *value -= value.floor();
            }
        }
        let (u, v) = match face {
            FaceDir::Down => (x * 16.0, (1.0 - z) * 16.0),
            FaceDir::Up => (x * 16.0, z * 16.0),
            FaceDir::North => ((1.0 - x) * 16.0, (1.0 - y) * 16.0),
            FaceDir::South => (x * 16.0, (1.0 - y) * 16.0),
            FaceDir::West => (z * 16.0, (1.0 - y) * 16.0),
            FaceDir::East => ((1.0 - z) * 16.0, (1.0 - y) * 16.0),
        };
        uv[(index + 4 - step) % 4] = [u, v];
    }
}

/// Snaps a quad's corners onto the element box's min/max hull, the client's
/// `FaceBakery.applyFacing`, keeping each uv on the corner it was baked for.
/// It runs whenever the element has no rotation of its own; for a box whose
/// `from` sits past its `to` on an axis it is what reads that axis as its
/// hull, which is why the real tree's six fire models bake at all.
fn apply_facing(face: FaceDir, corners: &mut [[f32; 3]; 4], uv: &mut [[f32; 2]; 4]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for corner in corners.iter() {
        for (axis, value) in corner.iter().enumerate() {
            min[axis] = min[axis].min(*value);
            max[axis] = max[axis].max(*value);
        }
    }
    let old_corners = *corners;
    let old_uv = *uv;
    let slots = hull_slots(min, max);
    for vertex in 0..4 {
        let position = corner_of(face, vertex, &slots);
        corners[vertex] = position;
        for (other, corner) in old_corners.iter().enumerate() {
            if (position[0] - corner[0]).abs() < POSITION_EPSILON
                && (position[1] - corner[1]).abs() < POSITION_EPSILON
                && (position[2] - corner[2]).abs() < POSITION_EPSILON
            {
                uv[vertex] = old_uv[other];
            }
        }
    }
}

/// Bakes one face of one element into a quad.
fn bake_face(
    element: &Element,
    face: FaceDir,
    part: &Face,
    texture: &str,
    rotation: VariantRotation,
) -> BakedQuad {
    let slots = slot_values(element.from, element.to);
    let uv = part
        .uv
        .unwrap_or_else(|| default_uv(face, element.from, element.to));

    let mut corners = [[0.0f32; 3]; 4];
    let mut uvs = [[0.0f32; 2]; 4];
    for vertex in 0..4 {
        let mut corner = corner_of(face, vertex, &slots);
        if let Some(part_rotation) = &element.rotation {
            corner = rotate_part(corner, part_rotation);
        }
        let slot = if rotation.is_identity() {
            vertex
        } else {
            rotation.vertex_slot(face, vertex)
        };
        corners[slot] = rotation.corner(corner);
        uvs[slot] = uv_corner(part.rotation, uv, vertex);
    }

    let quad_face = facing_from(&corners);
    if rotation.uvlock {
        lock_uv(&corners, &mut uvs, quad_face, part.rotation);
    }
    if element.rotation.is_none() {
        apply_facing(quad_face, &mut corners, &mut uvs);
    }

    BakedQuad {
        corners,
        uv: uvs.map(|sprite| [sprite[0] / 16.0, sprite[1] / 16.0]),
        texture: texture.to_string(),
        cullface: part.cullface.map(|cullface| rotation.face(cullface)),
        tintindex: part.tintindex,
        shade: element.shade,
    }
}

/// The quads `builtin/generated` draws: one flat plane per layer the model's
/// chain defines, front face only, in the client's `ItemModelGenerator`
/// shape. The builtin bake and the item bake
/// ([`ModelSource::bake_item`]) both consume it.
fn generated_quads(chain: &Chain) -> Vec<BakedQuad> {
    let south_uv = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];
    let mut quads = Vec::new();
    for (index, layer) in LAYERS.iter().enumerate() {
        let Some(texture) = chain.texture(layer) else {
            break;
        };
        quads.push(BakedQuad {
            corners: [
                [0.0, 1.0, GENERATED_PLANE],
                [0.0, 0.0, GENERATED_PLANE],
                [1.0, 0.0, GENERATED_PLANE],
                [1.0, 1.0, GENERATED_PLANE],
            ],
            uv: south_uv,
            texture,
            cullface: None,
            tintindex: Some(index as u8),
            shade: true,
        });
    }
    quads
}

/// A generated chain's `layer0…4` texture paths, in order, stopping at the
/// first layer that does not resolve (`ItemModelGenerator.makeItemModel`
/// reads the layers in order and stops where one is absent).
fn chain_layers(chain: &Chain) -> Vec<String> {
    let mut layers = Vec::new();
    for variable in LAYERS {
        let Some(texture) = chain.texture(variable) else {
            break;
        };
        layers.push(texture);
    }
    layers
}

/// Adds one resolved texture path to a baked item's list, once, skipping the
/// client's missing sprite, which the atlas builds itself.
fn push_texture(textures: &mut Vec<String>, path: &str) {
    if path != MISSING_SPRITE && !textures.iter().any(|texture| texture == path) {
        textures.push(path.to_string());
    }
}

/// The block model an item's chain draws through: the first model in the
/// chain under `block/`, or the item's own model when the chain names none.
fn block_member(chain: &Chain, resource: &str) -> String {
    chain
        .paths
        .iter()
        .find(|path| path.starts_with(BLOCK_MODEL_PREFIX))
        .map(String::to_string)
        .unwrap_or_else(|| resource.to_string())
}

/// Adds one resolved texture path to the set, skipping the client's missing
/// sprite, which the atlas builds itself.
fn collect_texture(chain: &Chain, variable: &str, paths: &mut BTreeSet<String>) {
    if let Some(texture) = chain.texture(variable) {
        if texture != MISSING_SPRITE {
            paths.insert(texture);
        }
    }
}

/// The resource path a block variant's model name states, `block/` prefixed
/// the way the client prefixes it.
fn block_model_resource(model: &str) -> Result<String, ModelError> {
    Ok(format!("{BLOCK_MODEL_PREFIX}{}", path_of(model)?))
}

/// The resource path an item model name states: a bare name is the item
/// tree's own, `item/` prefixed the way the client prefixes it; a path that
/// already names one is taken as written.
fn item_model_resource(name: &str) -> Result<String, ModelError> {
    let path = path_of(name)?;
    if path.contains('/') {
        Ok(path.to_string())
    } else {
        Ok(format!("{ITEM_MODEL_PREFIX}{path}"))
    }
}

/// The resource path a parent name states.
fn resource_of(parent: &str) -> Result<String, ModelError> {
    path_of(parent).map(str::to_string)
}

/// A name's path part, checking its namespace.
fn path_of(name: &str) -> Result<&str, ModelError> {
    match name.split_once(':') {
        None => Ok(name),
        Some((NAMESPACE, path)) => Ok(path),
        Some(_) => Err(ModelError::ForeignNamespace {
            name: name.to_string(),
        }),
    }
}

impl BlockStates {
    /// Parses one blockstate document.
    ///
    /// The document is the client's `variants` object: every key maps to one
    /// variant or a non-empty array of them. A variant states `model` and
    /// optionally `x`, `y`, `uvlock` and `weight`. `x` and `y` are turned to
    /// 0, 90, 180 or 270 first, the way the client turns them, and any other
    /// angle is an error; `weight` defaults to 1.
    pub fn parse(json: &str) -> Result<Self, ModelError> {
        let raw: RawBlockStates =
            serde_json::from_str(json).map_err(|source| ModelError::Json { source })?;
        let mut variants: BTreeMap<String, Vec<Variant>> = BTreeMap::new();
        for (key, entry) in raw.variants {
            let entries = match entry {
                RawVariantEntry::One(variant) => vec![variant],
                RawVariantEntry::Many(variants) => {
                    if variants.is_empty() {
                        return Err(ModelError::Value {
                            reason: format!("the variant `{key}` carries an empty array"),
                        });
                    }
                    variants
                }
            };
            let mut parsed = Vec::with_capacity(entries.len());
            for variant in entries {
                parsed.push(variant.into_variant(&key)?);
            }
            variants.insert(key, parsed);
        }
        Ok(Self { variants })
    }
}

impl RawVariant {
    /// Validates one variant entry.
    fn into_variant(self, key: &str) -> Result<Variant, ModelError> {
        Ok(Variant {
            model: self.model,
            x: quarter_turns(self.x, "x", key)?,
            y: quarter_turns(self.y, "y", key)?,
            uvlock: self.uvlock,
            weight: u32::try_from(self.weight).map_err(|_| ModelError::Value {
                reason: format!("the variant `{key}` states the weight {}", self.weight),
            })?,
        })
    }
}

/// One variant's rotation, normalized the way the client normalizes it and
/// checked against the four quarter turns.
fn quarter_turns(value: i64, axis: &str, key: &str) -> Result<u16, ModelError> {
    let normalized = value.rem_euclid(360);
    let turns = u16::try_from(normalized).unwrap_or(0);
    if QUARTER_TURNS.contains(&turns) {
        Ok(turns)
    } else {
        Err(ModelError::Value {
            reason: format!(
                "the variant `{key}` states an {axis} rotation of {value} degrees; only 0, 90, 180 and 270 are allowed"
            ),
        })
    }
}

impl ModelJson {
    /// Parses one model document.
    ///
    /// The fields are the client's: `parent`, `textures`, `elements`,
    /// `ambientocclusion` (default true) and `display`. A model states either
    /// a parent or elements, not both and not neither. Every bound sits
    /// within -16 to 32 in 1/16 units, every face key is one of the six
    /// names, every face texture is a `#variable`, every face rotation is a
    /// quarter turn, and an element's rotation axis is x, y or z with one of
    /// the five angles the client allows. Each of those, when violated, is an
    /// error, never a default.
    pub fn parse(json: &str) -> Result<Self, ModelError> {
        let raw: RawModel =
            serde_json::from_str(json).map_err(|source| ModelError::Json { source })?;
        let parent = raw.parent.filter(|parent| !parent.is_empty());
        let elements = raw.elements.unwrap_or_default();
        let has_parent = parent.is_some();
        let has_elements = !elements.is_empty();
        if has_parent && has_elements {
            return Err(ModelError::Value {
                reason:
                    "the model names both a parent and elements; the client takes one or the other"
                        .to_string(),
            });
        }
        if !has_parent && !has_elements {
            return Err(ModelError::Value {
                reason: "the model names neither a parent nor elements".to_string(),
            });
        }

        let mut parsed = Vec::with_capacity(elements.len());
        for (index, element) in elements.into_iter().enumerate() {
            parsed.push(element.into_element(index)?);
        }
        Ok(Self {
            parent,
            textures: raw.textures,
            elements: parsed,
            ambient_occlusion: raw.ambientocclusion.unwrap_or(true),
            display: raw.display.map(Display::from_raw),
        })
    }
}

impl RawElement {
    /// Validates and converts one element.
    fn into_element(self, index: usize) -> Result<Element, ModelError> {
        for (key, bound) in [("from", self.from), ("to", self.to)] {
            for value in bound {
                if !(MIN_BOUND..=MAX_BOUND).contains(&value) {
                    return Err(ModelError::Value {
                        reason: format!(
                            "element {index}: the {key} position [{}, {}, {}] exceeds the allowed boundaries (-16 to 32)",
                            bound[0], bound[1], bound[2]
                        ),
                    });
                }
            }
        }
        if self.faces.is_empty() {
            return Err(ModelError::Value {
                reason: format!("element {index} carries no faces"),
            });
        }
        let mut faces = BTreeMap::new();
        for (name, face) in self.faces {
            let Some(direction) = FaceDir::from_name(&name) else {
                return Err(ModelError::Value {
                    reason: format!(
                        "element {index}: `{name}` is not one of the six face names (down, up, north, south, west, east)"
                    ),
                });
            };
            faces.insert(direction, face.into_face(index, &name)?);
        }
        let rotation = match self.rotation {
            None => None,
            Some(rotation) => Some(rotation.into_rotation(index)?),
        };
        Ok(Element {
            from: self.from,
            to: self.to,
            rotation,
            shade: self.shade,
            faces,
        })
    }
}

impl RawFace {
    /// Validates and converts one face.
    fn into_face(self, index: usize, name: &str) -> Result<Face, ModelError> {
        if !self.texture.starts_with(VARIABLE_MARK) {
            return Err(ModelError::Value {
                reason: format!(
                    "element {index}, face `{name}`: the texture `{}` must name a variable with a leading `#`",
                    self.texture
                ),
            });
        }
        let rotation = match self.rotation {
            None => 0,
            Some(rotation) => match u16::try_from(rotation) {
                Ok(quarter) if QUARTER_TURNS.contains(&quarter) => quarter,
                _ => {
                    return Err(ModelError::Value {
                        reason: format!(
                            "element {index}, face `{name}`: the rotation {rotation} is outside the four quarter turns (0, 90, 180, 270)"
                        ),
                    });
                }
            },
        };
        let cullface = match self.cullface.as_deref() {
            None | Some("") => None,
            Some(cullface) => match FaceDir::from_name(cullface) {
                Some(direction) => Some(direction),
                None => {
                    return Err(ModelError::Value {
                        reason: format!(
                            "element {index}, face `{name}`: the cullface `{cullface}` is not one of the six face names"
                        ),
                    });
                }
            },
        };
        let tintindex = match self.tintindex {
            None | Some(-1) => None,
            Some(value) => Some(u8::try_from(value).map_err(|_| ModelError::Value {
                reason: format!(
                    "element {index}, face `{name}`: the tintindex {value} is outside 0..=255"
                ),
            })?),
        };
        Ok(Face {
            uv: self.uv,
            texture: self.texture,
            cullface,
            rotation,
            tintindex,
        })
    }
}

impl RawElementRotation {
    /// Validates and converts one element rotation.
    fn into_rotation(self, index: usize) -> Result<ElementRotation, ModelError> {
        let Some(axis) = Axis::from_name(&self.axis) else {
            return Err(ModelError::Value {
                reason: format!(
                    "element {index}: the rotation axis `{}` is not x, y or z",
                    self.axis
                ),
            });
        };
        if !ROTATION_ANGLES.contains(&self.angle) {
            return Err(ModelError::Value {
                reason: format!(
                    "element {index}: the rotation angle {} is not one of 0, 22.5, -22.5, 45 or -45",
                    self.angle
                ),
            });
        }
        Ok(ElementRotation {
            origin: [
                self.origin[0] / 16.0,
                self.origin[1] / 16.0,
                self.origin[2] / 16.0,
            ],
            axis,
            angle: self.angle,
            rescale: self.rescale,
        })
    }
}

impl Display {
    /// Converts the parsed display section, filling every absent slot with
    /// the source's own default (`ItemCameraTransforms.func_181683_a`).
    fn from_raw(raw: RawDisplay) -> Self {
        Self {
            third_person: raw
                .thirdperson
                .map(Transform::from_raw)
                .unwrap_or(Transform::DEFAULT),
            first_person: raw
                .firstperson
                .map(Transform::from_raw)
                .unwrap_or(Transform::DEFAULT),
            head: raw
                .head
                .map(Transform::from_raw)
                .unwrap_or(Transform::DEFAULT),
            gui: raw
                .gui
                .map(Transform::from_raw)
                .unwrap_or(Transform::DEFAULT),
            ground: raw
                .ground
                .map(Transform::from_raw)
                .unwrap_or(Transform::DEFAULT),
            fixed: raw
                .fixed
                .map(Transform::from_raw)
                .unwrap_or(Transform::DEFAULT),
            none: Transform::DEFAULT,
        }
    }
}

impl Transform {
    /// Converts one transform, keeping the file's numbers.
    fn from_raw(raw: RawTransform) -> Self {
        Self {
            rotation: raw.rotation,
            translation: raw.translation,
            scale: raw.scale,
        }
    }
}

/// Every `.json` file under `dir`, recursively, as paths relative to `dir`,
/// sorted so the walk order cannot leak into which file a failure reports.
fn collect_json(dir: &Path) -> Result<Vec<PathBuf>, ModelError> {
    let mut files = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        let path = dir.join(&relative);
        let entries = fs::read_dir(&path).map_err(|source| ModelError::Io {
            path: path.clone(),
            reason: source.to_string(),
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| ModelError::Io {
                path: path.clone(),
                reason: source.to_string(),
            })?;
            let child = relative.join(entry.file_name());
            let kind = entry.file_type().map_err(|source| ModelError::Io {
                path: dir.join(&child),
                reason: source.to_string(),
            })?;
            if kind.is_dir() {
                pending.push(child);
            } else if child.extension().and_then(|extension| extension.to_str())
                == Some(JSON_EXTENSION)
            {
                files.push(child);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// A relative file path's stem as a blockstate name.
fn path_stem(relative: &Path) -> Result<String, ModelError> {
    relative
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_string)
        .ok_or_else(|| ModelError::Io {
            path: relative.to_path_buf(),
            reason: "the file's name is not UTF-8".to_string(),
        })
}

/// A relative file path as a resource path: the path below `models/` without
/// the extension.
fn resource_path(relative: &Path) -> Result<String, ModelError> {
    let without_extension = relative.with_extension("");
    without_extension
        .to_str()
        .map(|path| path.replace(std::path::MAIN_SEPARATOR, "/"))
        .ok_or_else(|| ModelError::Io {
            path: relative.to_path_buf(),
            reason: "the file's name is not UTF-8".to_string(),
        })
}

/// Reads a file as text, naming it when it fails.
fn read_text(path: &Path) -> Result<String, ModelError> {
    fs::read_to_string(path).map_err(|source| ModelError::Io {
        path: path.to_path_buf(),
        reason: source.to_string(),
    })
}

/// A blockstate document, straight off the JSON.
#[derive(Deserialize)]
struct RawBlockStates {
    /// The variant keys each map to one variant or an array of them.
    variants: BTreeMap<String, RawVariantEntry>,
}

/// One variant key's entry: one variant or an array.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawVariantEntry {
    /// A single variant object.
    One(RawVariant),
    /// An array of variants.
    Many(Vec<RawVariant>),
}

/// One variant, as the file writes it.
#[derive(Deserialize)]
struct RawVariant {
    /// The model's name.
    model: String,
    /// The x rotation in degrees.
    #[serde(default)]
    x: i64,
    /// The y rotation in degrees.
    #[serde(default)]
    y: i64,
    /// Whether the uv stays locked to the block face.
    #[serde(default)]
    uvlock: bool,
    /// The weight in the variant's array.
    #[serde(default = "one")]
    weight: i64,
}

/// The default weight.
fn one() -> i64 {
    1
}

/// A model document, straight off the JSON.
#[derive(Deserialize)]
struct RawModel {
    /// The parent's name.
    #[serde(default)]
    parent: Option<String>,
    /// The texture variables.
    #[serde(default)]
    textures: BTreeMap<String, String>,
    /// The elements.
    #[serde(default)]
    elements: Option<Vec<RawElement>>,
    /// Whether ambient occlusion applies.
    #[serde(default)]
    ambientocclusion: Option<bool>,
    /// The display section.
    #[serde(default)]
    display: Option<RawDisplay>,
}

/// One element, as the file writes it.
#[derive(Deserialize)]
struct RawElement {
    /// The first bound.
    from: [f32; 3],
    /// The second bound.
    to: [f32; 3],
    /// The element's own rotation.
    #[serde(default)]
    rotation: Option<RawElementRotation>,
    /// Whether the element takes the face shading.
    #[serde(default = "yes")]
    shade: bool,
    /// The faces, keyed by name as written.
    #[serde(default)]
    faces: BTreeMap<String, RawFace>,
}

/// The default shade.
fn yes() -> bool {
    true
}

/// One element rotation, as the file writes it.
#[derive(Deserialize)]
struct RawElementRotation {
    /// The rotation's origin.
    origin: [f32; 3],
    /// The axis' name.
    axis: String,
    /// The angle in degrees.
    angle: f32,
    /// Whether the rotated axes stretch back out.
    #[serde(default)]
    rescale: bool,
}

/// One face, as the file writes it.
#[derive(Deserialize)]
struct RawFace {
    /// The uv in 1/16 units.
    #[serde(default)]
    uv: Option<[f32; 4]>,
    /// The texture variable.
    texture: String,
    /// The cull direction's name.
    #[serde(default)]
    cullface: Option<String>,
    /// The face's own rotation in degrees.
    #[serde(default)]
    rotation: Option<i64>,
    /// The tint index.
    #[serde(default)]
    tintindex: Option<i64>,
}

/// A display section, as the file writes it.
#[derive(Deserialize)]
struct RawDisplay {
    /// The third-person transform.
    #[serde(default)]
    thirdperson: Option<RawTransform>,
    /// The first-person transform.
    #[serde(default)]
    firstperson: Option<RawTransform>,
    /// The head transform.
    #[serde(default)]
    head: Option<RawTransform>,
    /// The gui transform.
    #[serde(default)]
    gui: Option<RawTransform>,
    /// The ground transform.
    #[serde(default)]
    ground: Option<RawTransform>,
    /// The fixed transform.
    #[serde(default)]
    fixed: Option<RawTransform>,
}

/// One transform, as the file writes it.
#[derive(Deserialize)]
struct RawTransform {
    /// The rotation in degrees.
    #[serde(default)]
    rotation: [f32; 3],
    /// The translation.
    #[serde(default)]
    translation: [f32; 3],
    /// The scale.
    #[serde(default = "ones")]
    scale: [f32; 3],
}

/// The default scale.
fn ones() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_transform_is_the_sources_identity() {
        // `ItemTransformVec3f.DEFAULT` and the three default arrays its
        // deserializer fills absent fields from.
        assert_eq!(Transform::DEFAULT.rotation, [0.0, 0.0, 0.0]);
        assert_eq!(Transform::DEFAULT.translation, [0.0, 0.0, 0.0]);
        assert_eq!(Transform::DEFAULT.scale, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn every_camera_type_defaults_to_the_identity() {
        // `ItemCameraTransforms.DEFAULT` fills all seven slots with
        // `ItemTransformVec3f.DEFAULT`, and the enum is the source's own
        // order (`TransformType`).
        for slot in [
            TransformType::None,
            TransformType::ThirdPerson,
            TransformType::FirstPerson,
            TransformType::Head,
            TransformType::Gui,
            TransformType::Ground,
            TransformType::Fixed,
        ] {
            assert_eq!(Display::DEFAULT.get(slot), Transform::DEFAULT, "{slot:?}");
        }
    }

    #[test]
    fn the_chest_model_pins_the_sources_boxes() {
        // `ModelChest`'s three `addBox` calls and their rotation points.
        assert_eq!(CHEST_MODEL.len(), 3);
        assert_eq!(CHEST_MODEL[0].from, [0.0, -5.0, -14.0]);
        assert_eq!(CHEST_MODEL[0].to, [14.0, 0.0, 0.0]);
        assert_eq!(CHEST_MODEL[0].origin, [1.0, 7.0, 15.0]);
        assert_eq!(CHEST_MODEL[1].from, [-1.0, -2.0, -15.0]);
        assert_eq!(CHEST_MODEL[1].to, [1.0, 2.0, -14.0]);
        assert_eq!(CHEST_MODEL[1].origin, [8.0, 7.0, 15.0]);
        assert_eq!(CHEST_MODEL[2].from, [0.0, 0.0, 0.0]);
        assert_eq!(CHEST_MODEL[2].to, [14.0, 10.0, 14.0]);
        assert_eq!(CHEST_MODEL[2].origin, [1.0, 6.0, 1.0]);
        assert_eq!(CHEST_MODEL_SCALE, 0.0625);
    }

    #[test]
    fn the_item_tree_prefixes_bare_names_and_keeps_item_paths() {
        assert_eq!(item_model_resource("stone").expect("a name"), "item/stone");
        assert_eq!(
            item_model_resource("item/stone").expect("a name"),
            "item/stone"
        );
        assert_eq!(
            item_model_resource("minecraft:stone").expect("a name"),
            "item/stone"
        );
        assert_eq!(
            item_model_resource("minecraft:item/stone").expect("a name"),
            "item/stone"
        );
        assert!(matches!(
            item_model_resource("othermod:stone"),
            Err(ModelError::ForeignNamespace { .. })
        ));
    }

    #[test]
    fn the_trio_pins_the_sources_sheets() {
        assert_eq!(BuiltinItem::Chest.icon_sheet(), "entity/chest/normal");
        assert_eq!(
            BuiltinItem::TrappedChest.icon_sheet(),
            "entity/chest/trapped"
        );
        assert_eq!(BuiltinItem::EnderChest.icon_sheet(), "entity/chest/ender");
        assert_eq!(
            BuiltinItem::from_model_name("item/chest"),
            Some(BuiltinItem::Chest)
        );
        assert_eq!(
            BuiltinItem::from_model_name("item/trapped_chest"),
            Some(BuiltinItem::TrappedChest)
        );
        assert_eq!(
            BuiltinItem::from_model_name("item/ender_chest"),
            Some(BuiltinItem::EnderChest)
        );
        assert_eq!(BuiltinItem::from_model_name("item/banner"), None);
    }
}
