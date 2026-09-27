//! The block behaviour table: one row per block id this client renders, carrying
//! the name, the metadata layout, the light columns, the material, the render
//! layer, the tint kind and the render path the rest of the milestone consumes.
//!
//! # Scope
//!
//! `covered_ids()` is the M1 palette's id set unioned with the non-air ids the
//! M1 acceptance world scan reported (`refs/rig/evidence/m1/task12-world-id-scan.txt`);
//! the scan added no id the palette did not already carry, so the union is those
//! 73 ids, sorted. The sample wall of the acceptance scene is generated from the
//! same list. A block outside the set has no row: its renderer draws the M1
//! magenta and logs once per id (the M2 plan's Decision 8).
//!
//! # What the rows are taken from
//!
//! Every constant traces to the decompiled 1.8.9 client under
//! `refs/_src/MCP-919/src/minecraft/net/minecraft/`:
//!
//! * `block/Block.java` — the id table's `registerBlock` lines (the registration
//!   name, and the hardness, light and opacity arguments they set), the
//!   `getLightOpacity`/`getLightValue`/`getBlockLayer`/`isOpaqueCube`/`isFullCube`
//!   defaults, and the constructor rule `lightOpacity = isOpaqueCube() ? 255 : 0`.
//! * the block classes themselves (`block/BlockStone.java`, `BlockGrass.java`,
//!   `BlockDirt.java`, `BlockSand.java`, `BlockPlanks.java`, `BlockOldLog.java`,
//!   `BlockNewLog.java`, `BlockOldLeaf.java`, `BlockNewLeaf.java`, `BlockLeaves.java`,
//!   `BlockSandStone.java`, `BlockStoneBrick.java`, `BlockColored.java`,
//!   `BlockSlab.java`, `BlockStoneSlab.java`, `BlockStairs.java`, `BlockTorch.java`,
//!   `BlockChest.java`, `BlockCrops.java`, `BlockFarmland.java`, `BlockFurnace.java`,
//!   `BlockDoor.java`, `BlockLadder.java`, `BlockPressurePlate.java`, `BlockIce.java`,
//!   `BlockSnowBlock.java`, `BlockCactus.java`, `BlockReed.java`, `BlockFence.java`,
//!   `BlockPumpkin.java`, `BlockHugeMushroom.java`, `BlockPane.java`,
//!   `BlockMycelium.java`, `BlockTallGrass.java`, `BlockDeadBush.java`,
//!   `BlockFlower.java`, `BlockMushroom.java`, `BlockBush.java`, `BlockQuartz.java`,
//!   `BlockDoublePlant.java`, `BlockLiquid.java`, `BlockStaticLiquid.java`,
//!   `BlockDynamicLiquid.java`, `BlockFalling.java`) for the property
//!   declarations, the metadata packing, the light overrides, the materials, the
//!   render layers and the tint functions.
//! * `block/state/BlockState.java` — the constructor sorts the declared properties
//!   by name, so a state's property string is alphabetical; `variant_key` sorts
//!   the same way.
//! * `util/EnumFacing.java` — the facing order (`down`, `up`, `north`, `south`,
//!   `west`, `east`) and the horizontal order (`south`, `west`, `north`, `east`)
//!   the facing properties are packed through.
//! * `init/Blocks.java` — the registration names the client resolves blocks by.
//! * `client/renderer/BlockModelShapes.java` — the state mapper that picks the
//!   blockstate file and the variant key per state, and the built-in blocks that
//!   have neither.
//! * `client/renderer/BlockFluidRenderer.java` and `client/renderer/BlockModelRenderer.java`
//!   — the liquid surface height and the tint-index rule.
//!
//! Only names and values are carried over; no source text is reproduced.

/// Where a block's texture colours take their tint from.
///
/// The client applies a block's colour only to quads whose model carries a tint
/// index (`BlockModelRenderer`, `bakedquad.hasTintIndex()`), so a block with no
/// tinted quads is [`TintKind::None`] whatever its colour functions say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TintKind {
    /// Untinted.
    None,
    /// The grass colour at the position (`BiomeColorHelper.getGrassColorAtPos`).
    Grass,
    /// The foliage colour at the position (`BiomeColorHelper.getFoliageColorAtPos`).
    Foliage,
    /// The biome's water colour (`BiomeColorHelper.getWaterColorAtPos`).
    Water,
    /// The grass block's side-overlay faces: the grass colour.
    ///
    /// The 1.8 model `block/grass` gives both the top face and the side-overlay
    /// element `tintindex: 0`, and `BlockGrass.colorMultiplier` ignores the pass,
    /// so this kind and [`TintKind::Grass`] resolve to the same colour; the kind
    /// names the overlay element for a mesher that labels faces.
    GrassSideOverlay,
}

/// The render layer a block's geometry is drawn in.
///
/// The values are the client's `EnumWorldBlockLayer` constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderLayer {
    /// Drawn with no transparency.
    Solid,
    /// Cutout with mipmapped minification; leaves under Fancy graphics.
    CutoutMipped,
    /// Cutout: alpha below the cutout threshold discards.
    Cutout,
    /// Alpha-blended; water and ice.
    Translucent,
}

/// The block's material, the subset of the source's `Material` values the
/// covered ids need.
///
/// The list is the M2 plan's; the covered ids also need `Cactus`, `Clay`,
/// `CraftedSnow` and `Gourd`, taken from the source's constructors, and the
/// listed `Stone`, `Snow`, `Piston`, `Portal`, `Web` and `RedstoneLight` are
/// declared for the ids later milestones add (no covered id uses them yet).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Material {
    /// `Material.rock`, for the stone-and-ore family.
    Stone,
    /// `Material.wood`.
    Wood,
    /// `Material.grass`.
    Grass,
    /// `Material.leaves`.
    Leaves,
    /// `Material.glass`.
    Glass,
    /// `Material.cloth`, wool.
    Cloth,
    /// `Material.water` and `Material.lava`: the two liquids.
    Liquid,
    /// `Material.plants`, the flowers, crops and crops blocks.
    Plant,
    /// `Material.iron`, the metal blocks.
    Metal,
    /// `Material.ground`, dirt and farmland.
    Ground,
    /// `Material.sand`, sand, gravel and soul sand.
    Sand,
    /// `Material.rock`, the stone family's own material.
    Rock,
    /// `Material.ice`.
    Ice,
    /// `Material.snow`.
    Snow,
    /// `Material.tnt`.
    Tnt,
    /// `Material.piston`.
    Piston,
    /// `Material.circuits`, torches and ladders.
    Circuit,
    /// `Material.portal`.
    Portal,
    /// `Material.web`.
    Web,
    /// `Material.redstoneLight`.
    RedstoneLight,
    /// `Material.vine`, the grass and bush plants.
    Vine,
    /// `Material.cactus`.
    Cactus,
    /// `Material.clay`.
    Clay,
    /// `Material.craftedSnow`, the full snow block.
    CraftedSnow,
    /// `Material.gourd`, the pumpkin.
    Gourd,
}

/// Which liquid a block is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiquidKind {
    /// Water (`Material.water`).
    Water,
    /// Lava (`Material.lava`).
    Lava,
}

/// How a block's geometry is produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderKind {
    /// From the blockstate-variant model.
    Model,
    /// From the liquid surface rule; the client's `BlockLiquid` overrides the
    /// model renderer with the fluid renderer, and the mesher routes the
    /// liquids here.
    Liquid,
    /// A cross-quad plant. The mesher identifies cross quads from the model
    /// itself (their elements carry `"shade": false`); this value is the
    /// table's own bookkeeping of which blocks are those plants.
    Cross,
}

/// The offset of a property that the metadata does not carry.
///
/// `PropertyKind::Bool` at this offset is always `false` and `PropertyKind::Enum`
/// at this offset takes its first listed value: the meta-derived state leaves
/// the property at the block's default, which is what the source's
/// `getStateFromMeta` produces for properties it does not set (grass's and
/// dirt's `snowy`, the stairs' world-contextual `shape`).
pub const NOT_IN_METADATA: u8 = u8::MAX;

/// How one property's value is read from a metadata nibble.
///
/// The three kinds are the 1.8 layout: a property takes the metadata bits from
/// `offset`, and the bits below it belong to the properties a state declares
/// before it. The read itself mirrors the source's own lookups: an index past
/// the last listed value falls back to the first one, the same choice
/// `byMetadata` makes for out-of-range metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyKind {
    /// A flag: `true` when bit `offset` is set. At [`NOT_IN_METADATA`] it is
    /// always `false`.
    Bool {
        /// The bit the flag reads.
        offset: u8,
    },
    /// A number taken from `bits` metadata bits at `offset`: the masked value,
    /// which must stay below `values`.
    Int {
        /// The first bit the number reads.
        offset: u8,
        /// How many bits the number occupies.
        bits: u8,
        /// The exclusive upper bound of the value.
        values: u8,
    },
    /// One of `values`, indexed by `bits` metadata bits at `offset`. At
    /// [`NOT_IN_METADATA`] the first value is taken.
    ///
    /// Where the source's own lookup is not a plain masked index — the stairs'
    /// and doors' facing, the furnace's and chest's front, the huge mushroom's
    /// slab — the list is the value the metadata selects for each of the sixteen
    /// metadata values, so the read stays exact over the whole nibble.
    Enum {
        /// The first bit the index reads.
        offset: u8,
        /// How many bits the index occupies.
        bits: u8,
        /// The values, in metadata-index order.
        values: &'static [&'static str],
    },
}

/// One property of a block's state, in the order `createBlockState` declares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropertyDef {
    /// The property's name, as the blockstate key spells it.
    pub name: &'static str,
    /// How the metadata carries the property.
    pub kind: PropertyKind,
}

/// One block's row in the behaviour table.
#[derive(Debug, Clone, Copy)]
pub struct BlockBehaviour {
    /// The block's numeric id, the first field of a wire block value's `id`.
    pub id: u16,
    /// The registration name (`Blocks.<field>`): the blockstate file's name for
    /// every block the client's state mapper does not name after a property
    /// value, and this project's name for the block either way.
    pub name: &'static str,
    /// The state's properties, in `createBlockState` declaration order (the
    /// packing order; the key itself is alphabetical, see [`variant_key`]).
    pub properties: &'static [PropertyDef],
    /// `getLightOpacity()`: the light the block removes, 0..=255.
    pub light_opacity: u8,
    /// The same column as the light engine's 0..=15 attenuation
    /// (`light_opacity.min(15)`).
    pub light_filter: u8,
    /// `getLightValue()`: 0..=15, the light the block emits.
    pub light_emission: u8,
    /// `isFullCube()`: whether the block fills its cell.
    pub full_cube: bool,
    /// The block's material.
    pub material: Material,
    /// The render layer. Leaves carry the layer M2 renders (`SOLID` under Fast
    /// graphics); their Fancy alternative is `CUTOUT_MIPPED`.
    pub render_layer: RenderLayer,
    /// The tint the block's tinted quads take.
    pub tint: TintKind,
    /// The liquid, for the four liquid ids.
    pub liquid: Option<LiquidKind>,
    /// How the block's geometry is produced.
    pub render: RenderKind,
}

/// The ids the table covers, sorted.
pub fn covered_ids() -> &'static [u16] {
    &COVERED
}

/// The table's row for a block id, or `None` for an id outside the covered set.
pub fn behaviour(id: u16) -> Option<&'static BlockBehaviour> {
    TABLE
        .binary_search_by_key(&id, |entry| entry.id)
        .ok()
        .map(|index| &TABLE[index])
}

/// The blockstate key a block's meta-derived state spells: its properties as
/// `name=value` pairs joined by commas, in name order, and the empty string for
/// a block with no properties.
///
/// The order is the client's own: `BlockState`'s constructor sorts the declared
/// properties by name before the state's map is built, and the client's
/// `StateMapperBase.getPropertyString` writes that map out. When the string is
/// empty the client's key is the literal `normal`; the blockstate files use that
/// spelling, and the loaders keep it, so an empty string from here resolves
/// against a file by the file's own `normal` key.
///
/// The values are the ones the source's `getStateFromMeta` produces for the
/// metadata. A metadata value outside a property's own value space — a log2
/// variant index that names no wood, a mushroom variant the lookup leaves
/// unset — takes the property's first value, the fallback the source's own
/// `byMetadata` lookups use; no loaded world carries such a value.
pub fn variant_key(block: &BlockBehaviour, meta: u8) -> String {
    let mut properties: Vec<&PropertyDef> = block.properties.iter().collect();
    properties.sort_by_key(|property| property.name);
    let mut key = String::new();
    for property in properties {
        if !key.is_empty() {
            key.push(',');
        }
        key.push_str(property.name);
        key.push('=');
        push_value(&mut key, property.kind, meta);
    }
    key
}

/// Appends one property's meta-derived value to `key`.
fn push_value(key: &mut String, kind: PropertyKind, meta: u8) {
    match kind {
        PropertyKind::Bool { offset } => {
            let value = offset != NOT_IN_METADATA && (meta >> offset) & 1 == 1;
            key.push_str(if value { "true" } else { "false" });
        }
        PropertyKind::Int {
            offset,
            bits,
            values,
        } => {
            let index = index_from(offset, bits, meta);
            let value = if index >= usize::from(values) {
                0
            } else {
                index
            };
            push_u8(key, value as u8);
        }
        PropertyKind::Enum {
            offset,
            bits,
            values,
        } => {
            let index = if offset == NOT_IN_METADATA {
                0
            } else {
                index_from(offset, bits, meta)
            };
            let value = values.get(index).unwrap_or(&values[0]);
            key.push_str(value);
        }
    }
}

/// The index a property reads from the metadata: `bits` bits from `offset`,
/// clamped to the first value when the offset is [`NOT_IN_METADATA`].
fn index_from(offset: u8, bits: u8, meta: u8) -> usize {
    if offset == NOT_IN_METADATA {
        return 0;
    }
    let mask = if bits >= 8 {
        u8::MAX
    } else {
        (1u8 << bits) - 1
    };
    usize::from((meta >> offset) & mask)
}

/// Appends a `u8` as decimal text.
fn push_u8(key: &mut String, value: u8) {
    if value >= 100 {
        key.push(char::from(b'0' + value / 100));
    }
    if value >= 10 {
        key.push(char::from(b'0' + value / 10 % 10));
    }
    key.push(char::from(b'0' + value % 10));
}

/// The fraction of a liquid cell's height its surface stands at, for a liquid
/// level 0..=15.
///
/// The source's rule (`BlockLiquid.getLiquidHeightPercent`) is the *air* gap
/// `(level + 1) / 9` for levels 0..=7, with levels 8..=15 — a falling column —
/// taking the source's own height; the mesher draws the complement, which is
/// what this returns. A source (level 0) stands at 8/9, the thinnest flowing
/// level (7) at 1/9. A cell with liquid above it is drawn full height by the
/// geometry rule, not by this function.
pub fn liquid_height_percent(level: u8) -> f32 {
    let level = if level >= 8 { 0 } else { level };
    1.0 - (f32::from(level) + 1.0) / 9.0
}

/// The liquid a block id is, if it is one.
pub fn liquid_kind(id: u16) -> Option<LiquidKind> {
    behaviour(id).and_then(|block| block.liquid)
}

/// The wood types `BlockPlanks.EnumType` names, in its metadata order.
const WOODS: [&str; 6] = ["oak", "spruce", "birch", "jungle", "acacia", "dark_oak"];

/// The first four wood types: the old log's and the old leaves' value space.
const WOODS_FIRST_FOUR: [&str; 4] = ["oak", "spruce", "birch", "jungle"];

/// The wood types the new log and the new leaves carry.
const WOODS_LAST_TWO: [&str; 2] = ["acacia", "dark_oak"];

/// The log axis values `BlockLog.EnumAxis` names, indexed by the metadata's
/// high two bits: `getStateFromMeta` maps 0 to y, 4 to x, 8 to z and 12 to none.
const LOG_AXES: [&str; 4] = ["y", "x", "z", "none"];

/// `EnumDyeColor`'s names in metadata order, 0 white through 15 black.
const DYES: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "silver",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];

/// `BlockStone.EnumType`'s names in metadata order.
const STONE_VARIANTS: [&str; 7] = [
    "stone",
    "granite",
    "smooth_granite",
    "diorite",
    "smooth_diorite",
    "andesite",
    "smooth_andesite",
];

/// `BlockDirt.DirtType`'s names in metadata order.
const DIRT_VARIANTS: [&str; 3] = ["dirt", "coarse_dirt", "podzol"];

/// `BlockSand.EnumType`'s names in metadata order.
const SAND_VARIANTS: [&str; 2] = ["sand", "red_sand"];

/// `BlockSandStone.EnumType`'s names in metadata order.
const SANDSTONE_TYPES: [&str; 3] = ["sandstone", "chiseled_sandstone", "smooth_sandstone"];

/// `BlockStoneBrick.EnumType`'s names in metadata order.
const STONE_BRICK_VARIANTS: [&str; 4] = [
    "stonebrick",
    "mossy_stonebrick",
    "cracked_stonebrick",
    "chiseled_stonebrick",
];

/// `BlockStoneSlab.EnumType`'s names in metadata order.
const STONE_SLABS: [&str; 8] = [
    "stone",
    "sandstone",
    "wood_old",
    "cobblestone",
    "brick",
    "stone_brick",
    "nether_brick",
    "quartz",
];

/// `BlockTallGrass.EnumType`'s names in metadata order.
const TALL_GRASS_TYPES: [&str; 3] = ["dead_bush", "tall_grass", "fern"];

/// `BlockFlower.EnumFlowerType`'s names for the red flower, in metadata order.
const RED_FLOWERS: [&str; 9] = [
    "poppy",
    "blue_orchid",
    "allium",
    "houstonia",
    "red_tulip",
    "orange_tulip",
    "white_tulip",
    "pink_tulip",
    "oxeye_daisy",
];

/// `BlockHugeMushroom.EnumType`'s values per metadata value.
///
/// The source's lookup has sixteen slots and leaves 11, 12 and 13 unset (the
/// enum names no value for them); those slots repeat the first value, the same
/// fallback the lookup makes for out-of-range metadata.
const MUSHROOM_VARIANTS: [&str; 16] = [
    "all_inside",
    "north_west",
    "north",
    "north_east",
    "west",
    "center",
    "east",
    "south_west",
    "south",
    "south_east",
    "stem",
    "all_inside",
    "all_inside",
    "all_inside",
    "all_outside",
    "all_stem",
];

/// `BlockQuartz.EnumType`'s names in metadata order.
const QUARTZ_VARIANTS: [&str; 5] = ["default", "chiseled", "lines_y", "lines_x", "lines_z"];

/// `BlockDoublePlant.EnumPlantType`'s names in metadata order.
const DOUBLE_PLANT_VARIANTS: [&str; 6] = [
    "sunflower",
    "syringa",
    "double_grass",
    "double_fern",
    "double_rose",
    "paeonia",
];

/// The torch's facings per metadata value: `BlockTorch.getStateFromMeta` maps 1
/// to east, 2 to west, 3 to south and 4 to north, and every other value — 0 and
/// 5 through 15 — to up.
const TORCH_FACINGS: [&str; 16] = [
    "up", "east", "west", "south", "north", "up", "up", "up", "up", "up", "up", "up", "up", "up",
    "up", "up",
];

/// The horizontal-facing blocks' front per metadata value: the state is
/// `EnumFacing.getFront(meta)` with the Y faces folded to north
/// (`BlockFurnace.getStateFromMeta`, `BlockChest.getStateFromMeta` and
/// `BlockLadder.getStateFromMeta`), and `getFront` wraps the index modulo six.
const FRONT_FACINGS: [&str; 16] = [
    "north", "north", "north", "south", "west", "east", "north", "north", "north", "south", "west",
    "east", "north", "north", "north", "south",
];

/// The door's facing per metadata value: the lower half is
/// `EnumFacing.getHorizontal(meta & 3).rotateYCCW()`, and the upper half keeps
/// the default `BlockDoor.getStateFromMeta` leaves it at.
const DOOR_FACINGS: [&str; 16] = [
    "east", "south", "west", "north", "east", "south", "west", "north", "north", "north", "north",
    "north", "north", "north", "north", "north",
];

/// The stairs' facings per metadata value: `getFront(5 - (meta & 3))`.
const STAIR_FACINGS: [&str; 4] = ["east", "west", "south", "north"];

/// `BlockStairs.EnumShape`'s names; the meta-derived state always holds the
/// first one, because the shape is world-contextual.
const STAIR_SHAPES: [&str; 5] = [
    "straight",
    "inner_left",
    "inner_right",
    "outer_left",
    "outer_right",
];

/// The pumpkin's facing per metadata value: `getHorizontal(meta)`, which wraps
/// modulo four, and `meta & 3` is that wrap.
const PUMPKIN_FACINGS: [&str; 4] = ["south", "west", "north", "east"];

/// The fence's and the pane's connection flags: metadata does not carry them,
/// so the meta-derived state holds the default `false` for all four.
const CONNECTIONS: [PropertyDef; 4] = [
    PropertyDef {
        name: "north",
        kind: PropertyKind::Bool {
            offset: NOT_IN_METADATA,
        },
    },
    PropertyDef {
        name: "east",
        kind: PropertyKind::Bool {
            offset: NOT_IN_METADATA,
        },
    },
    PropertyDef {
        name: "west",
        kind: PropertyKind::Bool {
            offset: NOT_IN_METADATA,
        },
    },
    PropertyDef {
        name: "south",
        kind: PropertyKind::Bool {
            offset: NOT_IN_METADATA,
        },
    },
];

/// The liquid level property: `PropertyInteger.create("level", 0, 15)`, packed
/// whole in the metadata.
const LEVEL: PropertyDef = PropertyDef {
    name: "level",
    kind: PropertyKind::Int {
        offset: 0,
        bits: 4,
        values: 16,
    },
};

/// The snow flag grass, dirt's non-podzol states and mycelium declare but the
/// metadata does not carry.
const SNOWY: PropertyDef = PropertyDef {
    name: "snowy",
    kind: PropertyKind::Bool {
        offset: NOT_IN_METADATA,
    },
};

/// The hit flag TNT declares and the metadata's low bit carries.
const EXPLODE: PropertyDef = PropertyDef {
    name: "explode",
    kind: PropertyKind::Bool { offset: 0 },
};

/// The connection flags fence and glass pane declare and the metadata does not
/// carry, in `createBlockState` order.
const TABLE: &[BlockBehaviour] = &[
    // BlockStone: VARIANT (BlockStone.java:20), material rock, opaque.
    BlockBehaviour {
        id: 1,
        name: "stone",
        properties: &[PropertyDef {
            name: "variant",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 4,
                values: &STONE_VARIANTS,
            },
        }],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockGrass: SNOWY (BlockGrass.java:21); the metadata packs the flag away
    // (getMetaFromState returns 0), so the meta-derived state is never snowy.
    BlockBehaviour {
        id: 2,
        name: "grass",
        properties: &[SNOWY],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Grass,
        render_layer: RenderLayer::Solid,
        tint: TintKind::GrassSideOverlay,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockDirt: VARIANT + SNOWY (BlockDirt.java:22-23); only the variant is
    // packed.
    BlockBehaviour {
        id: 3,
        name: "dirt",
        properties: &[
            PropertyDef {
                name: "variant",
                kind: PropertyKind::Enum {
                    offset: 0,
                    bits: 4,
                    values: &DIRT_VARIANTS,
                },
            },
            SNOWY,
        ],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Ground,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // Plain rock and wood blocks with no properties: Block.java's registerBlock
    // lines 4, 7, 13-16, 21, 41, 42, 45, 47-49, 56-58, 73, 82, 87, 88, 129.
    BlockBehaviour {
        id: 4,
        name: "cobblestone",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockPlanks: VARIANT (BlockPlanks.java:16-17), all six wood types.
    BlockBehaviour {
        id: 5,
        name: "planks",
        properties: &[PropertyDef {
            name: "variant",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 4,
                values: &WOODS,
            },
        }],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    BlockBehaviour {
        id: 7,
        name: "bedrock",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockDynamicLiquid / BlockStaticLiquid with Material.water
    // (Block.java:1261-1262): registration opacity 3, translucent layer.
    BlockBehaviour {
        id: 8,
        name: "flowing_water",
        properties: &[LEVEL],
        light_opacity: 3,
        light_filter: 3,
        light_emission: 0,
        full_cube: false,
        material: Material::Liquid,
        render_layer: RenderLayer::Translucent,
        tint: TintKind::Water,
        liquid: Some(LiquidKind::Water),
        render: RenderKind::Liquid,
    },
    BlockBehaviour {
        id: 9,
        name: "water",
        properties: &[LEVEL],
        light_opacity: 3,
        light_filter: 3,
        light_emission: 0,
        full_cube: false,
        material: Material::Liquid,
        render_layer: RenderLayer::Translucent,
        tint: TintKind::Water,
        liquid: Some(LiquidKind::Water),
        render: RenderKind::Liquid,
    },
    // BlockDynamicLiquid / BlockStaticLiquid with Material.lava
    // (Block.java:1263-1264): no opacity is set, the ctor rule gives 0, and the
    // registration's light level is the full 15.
    BlockBehaviour {
        id: 10,
        name: "flowing_lava",
        properties: &[LEVEL],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 15,
        full_cube: false,
        material: Material::Liquid,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: Some(LiquidKind::Lava),
        render: RenderKind::Liquid,
    },
    BlockBehaviour {
        id: 11,
        name: "lava",
        properties: &[LEVEL],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 15,
        full_cube: false,
        material: Material::Liquid,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: Some(LiquidKind::Lava),
        render: RenderKind::Liquid,
    },
    // BlockSand: VARIANT (BlockSand.java:16-17), sand and red sand.
    BlockBehaviour {
        id: 12,
        name: "sand",
        properties: &[PropertyDef {
            name: "variant",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 4,
                values: &SAND_VARIANTS,
            },
        }],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Sand,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockGravel falls through BlockFalling to Material.sand (BlockFalling.java:16-19).
    BlockBehaviour {
        id: 13,
        name: "gravel",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Sand,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockOre, material rock: gold, iron, coal, lapis, diamond, emerald and quartz ore.
    BlockBehaviour {
        id: 14,
        name: "gold_ore",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    BlockBehaviour {
        id: 15,
        name: "iron_ore",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    BlockBehaviour {
        id: 16,
        name: "coal_ore",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockOldLog: VARIANT + LOG_AXIS (BlockOldLog.java:16-22, 77-100, 107-132);
    // the variant's low two bits and the axis's high two bits, y first.
    BlockBehaviour {
        id: 17,
        name: "log",
        properties: &[
            PropertyDef {
                name: "variant",
                kind: PropertyKind::Enum {
                    offset: 0,
                    bits: 2,
                    values: &WOODS_FIRST_FOUR,
                },
            },
            PropertyDef {
                name: "axis",
                kind: PropertyKind::Enum {
                    offset: 2,
                    bits: 2,
                    values: &LOG_AXES,
                },
            },
        ],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockOldLeaf: VARIANT + CHECK_DECAY + DECAYABLE (BlockOldLeaf.java:103-137);
    // leaves carry opacity 1 (BlockLeaves.java:33) and the Fast layer (SOLID).
    BlockBehaviour {
        id: 18,
        name: "leaves",
        properties: &[
            PropertyDef {
                name: "variant",
                kind: PropertyKind::Enum {
                    offset: 0,
                    bits: 2,
                    values: &WOODS_FIRST_FOUR,
                },
            },
            PropertyDef {
                name: "check_decay",
                kind: PropertyKind::Bool { offset: 3 },
            },
            PropertyDef {
                name: "decayable",
                kind: PropertyKind::Enum {
                    offset: 2,
                    bits: 1,
                    values: &["true", "false"],
                },
            },
        ],
        light_opacity: 1,
        light_filter: 1,
        light_emission: 0,
        full_cube: true,
        material: Material::Leaves,
        render_layer: RenderLayer::Solid,
        tint: TintKind::Foliage,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockGlass (Material.glass): non-opaque, cutout, full-cube false.
    BlockBehaviour {
        id: 20,
        name: "glass",
        properties: &[],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        material: Material::Glass,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    BlockBehaviour {
        id: 21,
        name: "lapis_ore",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockSandStone: TYPE (BlockSandStone.java:17), three values packed whole.
    BlockBehaviour {
        id: 24,
        name: "sandstone",
        properties: &[PropertyDef {
            name: "type",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 4,
                values: &SANDSTONE_TYPES,
            },
        }],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockTallGrass: TYPE (BlockTallGrass.java:26, 152-168), Material.vine; the
    // grass and fern models carry a tint index (BlockModelRenderer.java:142-144),
    // and the source tints them with the grass colour.
    BlockBehaviour {
        id: 31,
        name: "tallgrass",
        properties: &[PropertyDef {
            name: "type",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 4,
                values: &TALL_GRASS_TYPES,
            },
        }],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        material: Material::Vine,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::Grass,
        liquid: None,
        render: RenderKind::Cross,
    },
    // BlockDeadBush: no properties (BlockDeadBush.java:19-24); its cross model
    // carries no tint index.
    BlockBehaviour {
        id: 32,
        name: "deadbush",
        properties: &[],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        material: Material::Vine,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Cross,
    },
    // BlockColored: COLOR (BlockColored.java:17, 57-73), the sixteen dye colours.
    BlockBehaviour {
        id: 35,
        name: "wool",
        properties: &[PropertyDef {
            name: "color",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 4,
                values: &DYES,
            },
        }],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Cloth,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockYellowFlower / BlockRedFlower through BlockFlower: TYPE
    // (BlockFlower.java:50-87), Material.plants; the cross model is untinted.
    BlockBehaviour {
        id: 37,
        name: "yellow_flower",
        properties: &[PropertyDef {
            name: "type",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 4,
                values: &["dandelion"],
            },
        }],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        material: Material::Plant,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Cross,
    },
    BlockBehaviour {
        id: 38,
        name: "red_flower",
        properties: &[PropertyDef {
            name: "type",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 4,
                values: &RED_FLOWERS,
            },
        }],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        material: Material::Plant,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Cross,
    },
    // BlockMushroom through BlockBush: no properties; the brown mushroom's
    // registration sets the light level 0.125, which is the emission 1
    // (Block.java:1293).
    BlockBehaviour {
        id: 39,
        name: "brown_mushroom",
        properties: &[],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 1,
        full_cube: false,
        material: Material::Plant,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Cross,
    },
    BlockBehaviour {
        id: 40,
        name: "red_mushroom",
        properties: &[],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        material: Material::Plant,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Cross,
    },
    // The metal blocks, Material.iron.
    BlockBehaviour {
        id: 41,
        name: "gold_block",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Metal,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    BlockBehaviour {
        id: 42,
        name: "iron_block",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Metal,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockStoneSlab's states are meta-derived for the double slab only: id 43
    // carries SEAMLESS + VARIANT (BlockStoneSlab.java:94-136), and the short
    // slab (id 44) is not a covered id — the M1 palette and the acceptance world
    // scan carry 43, not 44.
    BlockBehaviour {
        id: 43,
        name: "double_stone_slab",
        properties: &[
            PropertyDef {
                name: "seamless",
                kind: PropertyKind::Bool { offset: 3 },
            },
            PropertyDef {
                name: "variant",
                kind: PropertyKind::Enum {
                    offset: 0,
                    bits: 3,
                    values: &STONE_SLABS,
                },
            },
        ],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    BlockBehaviour {
        id: 45,
        name: "brick_block",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockTNT: EXPLODE (BlockTNT.java:23, 144-160), Material.tnt.
    BlockBehaviour {
        id: 46,
        name: "tnt",
        properties: &[EXPLODE],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Tnt,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    BlockBehaviour {
        id: 47,
        name: "bookshelf",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    BlockBehaviour {
        id: 48,
        name: "mossy_cobblestone",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    BlockBehaviour {
        id: 49,
        name: "obsidian",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockTorch: FACING (BlockTorch.java:24-30, 244-311), Material.circuits,
    // registration light level 0.9375 which is the emission 14 (Block.java:1307).
    BlockBehaviour {
        id: 50,
        name: "torch",
        properties: &[PropertyDef {
            name: "facing",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 4,
                values: &TORCH_FACINGS,
            },
        }],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 14,
        full_cube: false,
        material: Material::Circuit,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockMobSpawner: no properties, non-opaque, cutout, and the client
    // registers no light emission for it.
    BlockBehaviour {
        id: 52,
        name: "mob_spawner",
        properties: &[],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        material: Material::Rock,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockStairs: FACING + HALF + SHAPE (BlockStairs.java:30-32, 726-747,
    // 791-794); the light opacity 255 is set in the constructor
    // (BlockStairs.java:48), the model is the stairs block (wood or stone), and
    // the shape is world-contextual so the meta-derived state holds `straight`.
    BlockBehaviour {
        id: 53,
        name: "oak_stairs",
        properties: &[
            PropertyDef {
                name: "facing",
                kind: PropertyKind::Enum {
                    offset: 0,
                    bits: 2,
                    values: &STAIR_FACINGS,
                },
            },
            PropertyDef {
                name: "half",
                kind: PropertyKind::Enum {
                    offset: 2,
                    bits: 1,
                    values: &["bottom", "top"],
                },
            },
            PropertyDef {
                name: "shape",
                kind: PropertyKind::Enum {
                    offset: NOT_IN_METADATA,
                    bits: 0,
                    values: &STAIR_SHAPES,
                },
            },
        ],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: false,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockChest: FACING (BlockChest.java:31, 585-608), Material.wood, and a
    // built-in block: the client's registerBuiltInBlocks names it, so it has no
    // blockstate file and its geometry comes from the block-entity renderer,
    // which M2 has not implemented yet, so this table routes it down the model
    // path and the mesher draws the fallback sprite.
    BlockBehaviour {
        id: 54,
        name: "chest",
        properties: &[PropertyDef {
            name: "facing",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 4,
                values: &FRONT_FACINGS,
            },
        }],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    BlockBehaviour {
        id: 56,
        name: "diamond_ore",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    BlockBehaviour {
        id: 57,
        name: "diamond_block",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Metal,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockWorkbench: Material.wood, a plain full cube.
    BlockBehaviour {
        id: 58,
        name: "crafting_table",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockCrops: AGE, a PropertyInteger 0..7 (BlockCrops.java:19, 203-219); the
    // crop model carries no tint index, so the block is untinted.
    BlockBehaviour {
        id: 59,
        name: "wheat",
        properties: &[PropertyDef {
            name: "age",
            kind: PropertyKind::Int {
                offset: 0,
                bits: 3,
                values: 8,
            },
        }],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        material: Material::Plant,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Cross,
    },
    // BlockFarmland: MOISTURE 0..7 (BlockFarmland.java:22, 160-176); the
    // constructor sets the light opacity 255 (BlockFarmland.java:30).
    BlockBehaviour {
        id: 60,
        name: "farmland",
        properties: &[PropertyDef {
            name: "moisture",
            kind: PropertyKind::Int {
                offset: 0,
                bits: 3,
                values: 8,
            },
        }],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: false,
        material: Material::Ground,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockFurnace: FACING (BlockFurnace.java:26, 248-271); the lit variant's
    // registration sets the light level 0.875, the emission 13 (Block.java:1320).
    BlockBehaviour {
        id: 61,
        name: "furnace",
        properties: &[PropertyDef {
            name: "facing",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 4,
                values: &FRONT_FACINGS,
            },
        }],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    BlockBehaviour {
        id: 62,
        name: "lit_furnace",
        properties: &[PropertyDef {
            name: "facing",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 4,
                values: &FRONT_FACINGS,
            },
        }],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 13,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockDoor with Material.wood: HALF + FACING + OPEN + HINGE + POWERED
    // (BlockDoor.java:28-38, 367-444), the cutout layer (BlockDoor.java:331).
    BlockBehaviour {
        id: 64,
        name: "wooden_door",
        properties: &[
            PropertyDef {
                name: "half",
                kind: PropertyKind::Enum {
                    offset: 3,
                    bits: 1,
                    values: &["lower", "upper"],
                },
            },
            PropertyDef {
                name: "facing",
                kind: PropertyKind::Enum {
                    offset: 0,
                    bits: 4,
                    values: &DOOR_FACINGS,
                },
            },
            PropertyDef {
                name: "open",
                kind: PropertyKind::Bool { offset: 2 },
            },
            PropertyDef {
                name: "hinge",
                kind: PropertyKind::Enum {
                    offset: 0,
                    bits: 1,
                    values: &["left", "right"],
                },
            },
            PropertyDef {
                name: "powered",
                kind: PropertyKind::Bool { offset: 1 },
            },
        ],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        material: Material::Wood,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockLadder: FACING (BlockLadder.java:140-163), Material.circuits, cutout.
    BlockBehaviour {
        id: 65,
        name: "ladder",
        properties: &[PropertyDef {
            name: "facing",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 4,
                values: &FRONT_FACINGS,
            },
        }],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        material: Material::Circuit,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    BlockBehaviour {
        id: 67,
        name: "stone_stairs",
        properties: &[
            PropertyDef {
                name: "facing",
                kind: PropertyKind::Enum {
                    offset: 0,
                    bits: 2,
                    values: &STAIR_FACINGS,
                },
            },
            PropertyDef {
                name: "half",
                kind: PropertyKind::Enum {
                    offset: 2,
                    bits: 1,
                    values: &["bottom", "top"],
                },
            },
            PropertyDef {
                name: "shape",
                kind: PropertyKind::Enum {
                    offset: NOT_IN_METADATA,
                    bits: 0,
                    values: &STAIR_SHAPES,
                },
            },
        ],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: false,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockPressurePlate with Material.wood: POWERED (BlockPressurePlate.java:17,
    // 73-89), the pressure plate geometry non-opaque.
    BlockBehaviour {
        id: 72,
        name: "wooden_pressure_plate",
        properties: &[PropertyDef {
            name: "powered",
            kind: PropertyKind::Bool { offset: 0 },
        }],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockRedstoneOre: no properties; the unlit registration sets no light and
    // the lit one is a different id outside the covered set.
    BlockBehaviour {
        id: 73,
        name: "redstone_ore",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockIce: translucent with the registration's opacity 3 (Block.java:1337).
    BlockBehaviour {
        id: 79,
        name: "ice",
        properties: &[],
        light_opacity: 3,
        light_filter: 3,
        light_emission: 0,
        full_cube: true,
        material: Material::Ice,
        render_layer: RenderLayer::Translucent,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockSnowBlock: no properties, Material.craftedSnow.
    BlockBehaviour {
        id: 80,
        name: "snow",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::CraftedSnow,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockCactus: AGE, a PropertyInteger 0..15 (BlockCactus.java:21, 134-151),
    // Material.cactus, non-opaque, cutout.
    BlockBehaviour {
        id: 81,
        name: "cactus",
        properties: &[PropertyDef {
            name: "age",
            kind: PropertyKind::Int {
                offset: 0,
                bits: 4,
                values: 16,
            },
        }],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        material: Material::Cactus,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockClay: Material.clay, a plain full cube.
    BlockBehaviour {
        id: 82,
        name: "clay",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Clay,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockReed: AGE 0..15 (BlockReed.java:21, 160-176), Material.plants; the
    // reeds model inherits `block/tallgrass`, whose faces carry a tint index,
    // and the source tints them with the grass colour.
    BlockBehaviour {
        id: 83,
        name: "reeds",
        properties: &[PropertyDef {
            name: "age",
            kind: PropertyKind::Int {
                offset: 0,
                bits: 4,
                values: 16,
            },
        }],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        material: Material::Plant,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::Grass,
        liquid: None,
        render: RenderKind::Cross,
    },
    // BlockFence with Material.wood: NORTH + EAST + WEST + SOUTH
    // (BlockFence.java:24-33, 180-197); the connections are world-contextual.
    BlockBehaviour {
        id: 85,
        name: "fence",
        properties: &CONNECTIONS,
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockPumpkin: FACING (BlockPumpkin.java:133-149), Material.gourd.
    BlockBehaviour {
        id: 86,
        name: "pumpkin",
        properties: &[PropertyDef {
            name: "facing",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 2,
                values: &PUMPKIN_FACINGS,
            },
        }],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Gourd,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    BlockBehaviour {
        id: 87,
        name: "netherrack",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockSoulSand: Material.sand.
    BlockBehaviour {
        id: 88,
        name: "soul_sand",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Sand,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockGlowstone with Material.glass: opaque cube, registration light level
    // 1.0, the emission 15 (Block.java:1348).
    BlockBehaviour {
        id: 89,
        name: "glowstone",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 15,
        full_cube: true,
        material: Material::Glass,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockStoneBrick: VARIANT (BlockStoneBrick.java:16, 52-68).
    BlockBehaviour {
        id: 98,
        name: "stonebrick",
        properties: &[PropertyDef {
            name: "variant",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 4,
                values: &STONE_BRICK_VARIANTS,
            },
        }],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockHugeMushroom with Material.wood: VARIANT (BlockHugeMushroom.java:19,
    // 83-99); its lookup has sixteen slots, three of them unset.
    BlockBehaviour {
        id: 99,
        name: "brown_mushroom_block",
        properties: &[PropertyDef {
            name: "variant",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 4,
                values: &MUSHROOM_VARIANTS,
            },
        }],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    BlockBehaviour {
        id: 100,
        name: "red_mushroom_block",
        properties: &[PropertyDef {
            name: "variant",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 4,
                values: &MUSHROOM_VARIANTS,
            },
        }],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockPane with Material.glass: NORTH + EAST + WEST + SOUTH
    // (BlockPane.java:23-35, 195-203), non-opaque, the cutout-mipped layer.
    BlockBehaviour {
        id: 102,
        name: "glass_pane",
        properties: &CONNECTIONS,
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        material: Material::Glass,
        render_layer: RenderLayer::CutoutMipped,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockMycelium: SNOWY, Material.grass (BlockMycelium.java:20-28, 89-97).
    BlockBehaviour {
        id: 110,
        name: "mycelium",
        properties: &[SNOWY],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Grass,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    BlockBehaviour {
        id: 129,
        name: "emerald_ore",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockCarrot and BlockPotato through BlockCrops: AGE 0..7.
    BlockBehaviour {
        id: 141,
        name: "carrots",
        properties: &[PropertyDef {
            name: "age",
            kind: PropertyKind::Int {
                offset: 0,
                bits: 3,
                values: 8,
            },
        }],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        material: Material::Plant,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Cross,
    },
    BlockBehaviour {
        id: 142,
        name: "potatoes",
        properties: &[PropertyDef {
            name: "age",
            kind: PropertyKind::Int {
                offset: 0,
                bits: 3,
                values: 8,
            },
        }],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        material: Material::Plant,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Cross,
    },
    // BlockQuartz: VARIANT (BlockQuartz.java:21, 94-110).
    BlockBehaviour {
        id: 155,
        name: "quartz_block",
        properties: &[PropertyDef {
            name: "variant",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 4,
                values: &QUARTZ_VARIANTS,
            },
        }],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockNewLeaf: VARIANT + CHECK_DECAY + DECAYABLE (BlockNewLeaf.java:77-111),
    // the last two wood types.
    BlockBehaviour {
        id: 161,
        name: "leaves2",
        properties: &[
            PropertyDef {
                name: "variant",
                kind: PropertyKind::Enum {
                    offset: 0,
                    bits: 2,
                    values: &WOODS_LAST_TWO,
                },
            },
            PropertyDef {
                name: "check_decay",
                kind: PropertyKind::Bool { offset: 3 },
            },
            PropertyDef {
                name: "decayable",
                kind: PropertyKind::Enum {
                    offset: 2,
                    bits: 1,
                    values: &["true", "false"],
                },
            },
        ],
        light_opacity: 1,
        light_filter: 1,
        light_emission: 0,
        full_cube: true,
        material: Material::Leaves,
        render_layer: RenderLayer::Solid,
        tint: TintKind::Foliage,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockNewLog: VARIANT + LOG_AXIS (BlockNewLog.java:69-124).
    BlockBehaviour {
        id: 162,
        name: "log2",
        properties: &[
            PropertyDef {
                name: "variant",
                kind: PropertyKind::Enum {
                    offset: 0,
                    bits: 2,
                    values: &WOODS_LAST_TWO,
                },
            },
            PropertyDef {
                name: "axis",
                kind: PropertyKind::Enum {
                    offset: 2,
                    bits: 2,
                    values: &LOG_AXES,
                },
            },
        ],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
    },
    // BlockDoublePlant: HALF + VARIANT + FACING (BlockDoublePlant.java:28-39,
    // 281-316), Material.vine; the plant models carry no tint index, so the
    // block is untinted however its colour function answers.
    BlockBehaviour {
        id: 175,
        name: "double_plant",
        properties: &[
            PropertyDef {
                name: "half",
                kind: PropertyKind::Enum {
                    offset: 3,
                    bits: 1,
                    values: &["lower", "upper"],
                },
            },
            PropertyDef {
                name: "variant",
                kind: PropertyKind::Enum {
                    offset: 0,
                    bits: 4,
                    values: &DOUBLE_PLANT_VARIANTS,
                },
            },
            PropertyDef {
                name: "facing",
                kind: PropertyKind::Enum {
                    offset: NOT_IN_METADATA,
                    bits: 0,
                    values: &["north"],
                },
            },
        ],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        material: Material::Vine,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Cross,
    },
];

/// The ids [`TABLE`] covers, sorted; the table's ids are this list in order.
const COVERED: [u16; 73] = [
    1, 2, 3, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 24, 31, 32, 35, 37, 38, 39,
    40, 41, 42, 43, 45, 46, 47, 48, 49, 50, 52, 53, 54, 56, 57, 58, 59, 60, 61, 62, 64, 65, 67, 72,
    73, 79, 80, 81, 82, 83, 85, 86, 87, 88, 89, 98, 99, 100, 102, 110, 129, 141, 142, 155, 161,
    162, 175,
];

/// The table answers for exactly the covered ids, in order, and every row's
/// light columns agree (the engine's filter is the source opacity clamped).
const _: () = {
    assert!(TABLE.len() == COVERED.len());
    let mut index = 0;
    while index < COVERED.len() {
        assert!(TABLE[index].id == COVERED[index]);
        let opacity = TABLE[index].light_opacity;
        let filter = if opacity < 15 { opacity } else { 15 };
        assert!(TABLE[index].light_filter == filter);
        index += 1;
    }
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_sorted_and_unique() {
        let ids = covered_ids();
        assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn every_row_answers_for_a_covered_id_and_nothing_else() {
        for &id in covered_ids() {
            let entry = behaviour(id).expect("covered");
            assert_eq!(entry.id, id);
        }
        assert!(behaviour(0).is_none());
        assert!(behaviour(5000).is_none());
        assert_eq!(liquid_kind(9), Some(LiquidKind::Water));
        assert_eq!(liquid_kind(54), None);
    }

    #[test]
    fn a_not_in_metadata_property_takes_its_default() {
        let stairs = behaviour(53).expect("stairs");
        assert_eq!(
            variant_key(stairs, 0),
            "facing=east,half=bottom,shape=straight"
        );
        assert_eq!(
            variant_key(stairs, 15),
            "facing=north,half=top,shape=straight"
        );
        let fence = behaviour(85).expect("fence");
        assert_eq!(
            variant_key(fence, 9),
            "east=false,north=false,south=false,west=false"
        );
    }

    #[test]
    fn an_out_of_range_index_falls_back_to_the_first_value() {
        // log2's variant index covers two woods; the other two indices name no
        // wood, and the source's own lookup falls back to the first value.
        let log2 = behaviour(162).expect("log2");
        assert_eq!(variant_key(log2, 2), "axis=y,variant=acacia");
        assert_eq!(variant_key(log2, 3), "axis=y,variant=acacia");
        assert_eq!(variant_key(log2, 6), "axis=x,variant=acacia");
        // The huge mushroom's lookup leaves three slots unset.
        let mushroom = behaviour(99).expect("mushroom");
        assert_eq!(variant_key(mushroom, 11), "variant=all_inside");
        assert_eq!(variant_key(mushroom, 15), "variant=all_stem");
    }
}
