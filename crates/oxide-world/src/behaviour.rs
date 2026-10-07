//! The block behaviour table: one row per block id this client renders, carrying
//! the name, the metadata layout, the light columns, the two cube predicates
//! (**`full_cube` from `isFullCube()` and `occludes` from `isOpaqueCube()`**,
//! which the cull rule reads and which are not the same column), the material,
//! the render layer, the tint kind, the render path and the movement columns
//! (`collision`, `hardness`, `climbable`, `slipperiness`) the rest of the
//! milestone consumes.
//!
//! # Scope
//!
//! `covered_ids()` is the M1 palette's id set unioned with the non-air ids the
//! M1 acceptance world scan reported (`refs/rig/evidence/m1/task12-world-id-scan.txt`);
//! the scan added no id the palette did not already carry, so the union is those
//! 73 ids, sorted, plus the two the M4 acceptance's own frames added on their
//! evidence — the snow layer (78) and the barrier (166). The sample wall of the
//! acceptance scene is generated from the same list. A block outside the set has
//! no row: its renderer draws the M1 magenta and logs once per id (the M2
//! plan's Decision 8).
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
//!   `BlockBarrier.java`, `BlockMycelium.java`, `BlockTallGrass.java`, `BlockDeadBush.java`,
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
//! * `block/Block.java` and the block classes — the movement columns: the
//!   registration lines' `setHardness`/`setBlockUnbreakable` and the classes'
//!   own constructors for the hardness, `EntityLivingBase.isOnLadder`'s test
//!   for the climbable flag, the classes' `slipperiness` assignments, and
//!   each class's `addCollisionBoxesToList`/`getCollisionBoundingBox` for the
//!   collision class (the boxes are the pure shape functions in
//!   `crate::collision`).
//! * `client/renderer/BlockModelShapes.java` — the state mapper that picks the
//!   blockstate file and the variant key per state, and the built-in blocks that
//!   have neither.
//! * `client/renderer/BlockFluidRenderer.java` and `client/renderer/BlockModelRenderer.java`
//!   — the liquid surface height and the tint-index rule.
//!
//! Only names and values are carried over; no source text is reproduced.
//!
//! # Notes
//!
//! The interaction reach is per gamemode, not per block, so it has no row of
//! its own; the design's interaction section asks for it to be recorded here.
//! `PlayerControllerMP.getBlockReachDistance` returns
//! `this.currentGameType.isCreative() ? 5.0F : 4.5F`
//! (`client/multiplayer/PlayerControllerMP.java:344-346`): `5.0` in creative
//! and `4.5` in every other gamemode. The gamemode the client reads is Join
//! Game's byte with the hardcore bit masked (`S01PacketJoinGame.java:44-47`);
//! `oxide-game`'s `interaction::reach` applies the rule and its tests pin both
//! literals.

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
/// `CraftedSnow`, `Gourd` and `Barrier`, taken from the source's constructors,
/// and the listed `Stone`, `Piston`, `Portal`, `Web` and `RedstoneLight` are
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
    /// `Material.barrier`, the barrier: requires a tool and immovable
    /// (`Material.java:49`).
    Barrier,
}

impl Material {
    /// `Material.blocksLight()`: whether this material blocks light, the
    /// property the source's `Block.translucent` field inverts
    /// (`block/Block.java:291-297`, read back by `isTranslucent()` at
    /// `:215-223`).
    ///
    /// False for the transparent, logic and portal materials — air, fire,
    /// plants, vine, circuits, carpet, snow and portal
    /// (`block/material/Material.java:5,15,16,19,21,22,29,37`,
    /// `MaterialLogic.java:22-25`, `MaterialTransparent.java:22-25`,
    /// `MaterialPortal.java:21-24`) — and true for every other material. The
    /// six that call `setTranslucent()` (leaves, glass, tnt, ice, snow, cactus;
    /// `Material.java:14,23,25,27,29,33`) do not move it: `setTranslucent()`
    /// sets only `Material.isTranslucent`, which only `isOpaque()` reads
    /// (`:118-121`, `:170-173`), so a leaf blocks light as a stone does and
    /// snow is false through its own logic class, not through that call.
    ///
    /// The ambient-occlusion corner substitution is the reader
    /// (`BlockModelRenderer.java:377-405`).
    pub fn blocks_light(self) -> bool {
        !matches!(
            self,
            Material::Plant
                | Material::Snow
                | Material::Circuit
                | Material::Portal
                | Material::Vine
        )
    }

    /// `Material.blocksMovement()`: whether a block of this material is solid.
    ///
    /// False for the same three material classes as
    /// [`Material::blocks_light`] (`MaterialLogic.java:30-33`,
    /// `MaterialTransparent.java:30-33`, `MaterialPortal.java:29-32`) and for
    /// the two liquids (`MaterialLiquid.java:23-26`), true otherwise — those
    /// are the only overrides the source's material classes carry. The
    /// ambient-occlusion light value reads it through `isBlockNormalCube()`
    /// (`block/Block.java:347-350`, `:1099-1102`).
    pub fn blocks_movement(self) -> bool {
        self.blocks_light() && !matches!(self, Material::Liquid)
    }

    /// `Material.isOpaque()`: whether the material reads opaque to the
    /// connection rules a fence, wall or pane resolves.
    ///
    /// `isTranslucent ? false : blocksMovement()` (`block/material/Material.java:170-173`);
    /// the six materials that call `setTranslucent()` — leaves, glass, tnt,
    /// ice, snow, cactus (`Material.java:14,23,25,27,29,33`) — answer false
    /// whatever their movement, and the rest read [`Material::blocks_movement`].
    /// The predicate is `BlockFence.canConnectTo`'s (`BlockFence.java:161`)
    /// and `BlockWall.canConnectTo`'s (`BlockWall.java:122`).
    pub fn is_opaque(self) -> bool {
        !matches!(
            self,
            Material::Leaves
                | Material::Glass
                | Material::Tnt
                | Material::Ice
                | Material::Snow
                | Material::Cactus
        ) && self.blocks_movement()
    }

    /// `Material.isSolid()`: whether a block of this material counts as solid
    /// ground, the flag the fluid-height predicate reads.
    ///
    /// True by default (`block/material/Material.java:94-97`) and false for
    /// exactly four material classes — `MaterialTransparent`
    /// (`MaterialTransparent.java:14-17`), `MaterialLogic`
    /// (`MaterialLogic.java:14-17`), `MaterialLiquid`
    /// (`MaterialLiquid.java:31-34`) and `MaterialPortal`
    /// (`MaterialPortal.java:13-16`) — which in this table's vocabulary is
    /// [`Material::Plant`], [`Material::Vine`], [`Material::Circuit`],
    /// [`Material::Snow`], [`Material::Liquid`] and [`Material::Portal`].
    /// Nothing else moves it: the web's `blocksMovement` override
    /// (`Material.java:39-45`) is the recorded latent note and is not this
    /// flag.
    ///
    /// The fluid-height predicate reads it (`client/renderer/BlockFluidRenderer.java:272-278`).
    pub fn is_solid(self) -> bool {
        !matches!(
            self,
            Material::Plant
                | Material::Vine
                | Material::Circuit
                | Material::Snow
                | Material::Liquid
                | Material::Portal
        )
    }
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
    /// No geometry at all: the block draws nothing. `BlockBarrier.getRenderType()`
    /// answers `-1` — the render dispatcher's "no render"
    /// (`BlockBarrier.java:19-25`) — so the mesher emits no quads for it: no
    /// model, and no fallback cube either.
    Invisible,
}

/// The collision shape class a block's id resolves through.
///
/// Every class is the box or boxes the source's block class answers from its
/// `addCollisionBoxesToList`/`getCollisionBoundingBox` overrides; the shapes
/// are the pure functions in [`crate::collision`], and the classes that read
/// their neighbours are resolved by the view (`oxide-game`'s `WorldView`).
/// A class with no covered id today (the cobblestone wall)
/// is declared all the same: later milestones add the ids, and the movement
/// model should not learn a new vocabulary then.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionShape {
    /// No collision box: the liquids (`BlockLiquid.java:121-124`), the cross
    /// plants (`BlockBush.java:76-79`, inherited by the flowers, mushrooms,
    /// crops, double plant, tall grass and dead bush; `BlockTorch.java:40-43`;
    /// `BlockReed.java:116-119`) and the pressure plate
    /// (`BlockBasePressurePlate.java:57-60`) answer a null box.
    None,
    /// The block's own unit cube: `Block.getCollisionBoundingBox`'s default
    /// (`block/Block.java:499-502`).
    Full,
    /// A slab: the named half, or the full cube for the double variant
    /// ([`crate::collision::slab_boxes`]; `BlockSlab.java:28-35`, `:45-67`).
    Slab {
        /// Whether the double-slab variant fills its cell
        /// (`BlockSlab.java:28-31`); the one covered id is 43, whose id names
        /// the double slab alone.
        double: bool,
    },
    /// A stair: the base half plus the step and corner boxes
    /// ([`crate::collision::stairs_boxes`]; `BlockStairs.java:533-546`).
    Stairs,
    /// A snow layer: the full footprint at its counted height
    /// ([`crate::collision::snow_layer_box`]; `BlockSnow.java:43-48`).
    SnowLayers,
    /// A cactus: the 1/16 inset ([`crate::collision::cactus_box`];
    /// `BlockCactus.java:63-68`).
    Cactus,
    /// A fence: its post and arms ([`crate::collision::fence_boxes`];
    /// `BlockFence.java:50-107`).
    Fence,
    /// A cobblestone wall: its post and arms, 1.5 high
    /// ([`crate::collision::wall_box`]; `BlockWall.java:115-120`).
    Wall,
    /// A pane: the two axis plates ([`crate::collision::pane_boxes`];
    /// `BlockPane.java:75-122`).
    Pane,
    /// A chest: the 14/16 box ([`crate::collision::chest_box`];
    /// `BlockChest.java:42`, `:66-88`).
    Chest,
    /// A door: the 3/16 plate ([`crate::collision::door_box`];
    /// `BlockDoor.java:72-76`, `:83-154`).
    Door,
    /// A ladder: the 1/8 plate on its attached face
    /// ([`crate::collision::ladder_box`]; `BlockLadder.java:28-32`, `:40-66`).
    Ladder,
    /// A soul sand block: the full footprint, its top at 7/8
    /// ([`crate::collision::soul_sand_box`]; `BlockSoulSand.java:20-24`).
    SoulSand,
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
/// before it. The read mirrors the source's own lookups for the metadata those
/// lookups can build; an index past the last listed value falls back to the
/// first *listed* value — this project's own documented choice for the values
/// the source's filtered lookups cannot build (see [`variant_key`]).
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
    /// `isOpaqueCube()`: whether the block hides the face of a neighbour that
    /// culls against it.
    ///
    /// This is the predicate `Block.shouldSideBeRendered`'s base rule reads
    /// (`block/Block.java:468-471`), and it is not `isFullCube()`: the mob
    /// spawner and ice are full cubes that do not hide a neighbour's face
    /// (`BlockMobSpawner.java`, `BlockBreakable.java`, inherited by
    /// `BlockIce.java`), and the two leaf ids answer `!fancyGraphics`
    /// (`BlockLeaves.java:278-281`), which under M2's Fast graphics is true.
    /// Everything else carries its `full_cube` value, `Block.isOpaqueCube()`'s
    /// own default being `true`.
    pub occludes: bool,
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
    /// The collision shape class, resolved per state (see [`CollisionShape`]).
    pub collision: CollisionShape,
    /// `Block.getBlockHardness()`: the source's `blockHardness`
    /// (`block/Block.java:115`, set by `setHardness` at `:395-398` and by
    /// `setBlockUnbreakable` at `:407-409`, which is `-1.0`).
    pub hardness: f32,
    /// `EntityLivingBase.isOnLadder()`: whether the movement model climbs a
    /// block of this id. The source tests the block itself —
    /// `block == Blocks.ladder || block == Blocks.vine`
    /// (`entity/EntityLivingBase.java:1134-1141`) — and of the covered ids
    /// only the ladder (65) is admitted.
    pub climbable: bool,
    /// `Block.slipperiness`: the friction multiplier (`block/Block.java:291`
    /// sets the `0.6` default; `BlockIce.java:23` the one `0.98`).
    pub slipperiness: f32,
}

impl BlockBehaviour {
    /// `Block.isFullBlock()`: whether the block fills its cell for the
    /// connection rules a pane reads (`BlockPane.java:177`).
    ///
    /// The source sets `fullBlock` once, at construction, from
    /// `isOpaqueCube()` (`block/Block.java:295`) — this table's `occludes`
    /// column — and only the double slab moves it afterwards
    /// (`BlockSlab.java:28-31`). The leaves' `isOpaqueCube()` is a runtime
    /// graphics read (`BlockLeaves.java:278-281`); the column carries M2's
    /// Fast-graphics value, as it does everywhere else in the row.
    pub fn is_full_block(&self) -> bool {
        self.occludes || matches!(self.collision, CollisionShape::Slab { double: true })
    }
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
/// unset — takes the property's first listed value. Where the list was filtered
/// down from a wider source enum (the log2 and leaves2 variants), that fallback
/// is this project's own documented choice for a state the source cannot build:
/// the source's lookup would return the whole enum's first value, which the
/// filtered property does not allow. No loaded world carries such a value.
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

/// The wooden pressure plate's `powered` answer for each metadata value: the
/// source reads the flag as `meta == 1` (`BlockPressurePlate.getStateFromMeta`,
/// BlockPressurePlate.java:73-76), so only metadata 1 is pressed and every
/// other value — including the odd values above 1 — is unpressed.
const PRESSURE_PLATE_POWERED: [&str; 16] = [
    "false", "true", "false", "false", "false", "false", "false", "false", "false", "false",
    "false", "false", "false", "false", "false", "false",
];

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
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 1.5,
        climbable: false,
        slipperiness: 0.60,
    },
    // BlockGrass: SNOWY (BlockGrass.java:21); the metadata packs the flag away
    // (getMetaFromState returns 0), so the meta-derived state is never snowy.
    // Its layer is the cutout-mipped queue for every state (getBlockLayer,
    // BlockGrass.java:157-160): the side overlay is an alpha-cutout texture.
    BlockBehaviour {
        id: 2,
        name: "grass",
        properties: &[SNOWY],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        occludes: true,
        material: Material::Grass,
        render_layer: RenderLayer::CutoutMipped,
        tint: TintKind::GrassSideOverlay,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.6,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Ground,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.5,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 2.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 2.0,
        climbable: false,
        slipperiness: 0.60,
    },
    BlockBehaviour {
        id: 7,
        name: "bedrock",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: -1.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Liquid,
        render_layer: RenderLayer::Translucent,
        tint: TintKind::Water,
        liquid: Some(LiquidKind::Water),
        render: RenderKind::Liquid,
        collision: CollisionShape::None,
        hardness: 100.0,
        climbable: false,
        slipperiness: 0.60,
    },
    BlockBehaviour {
        id: 9,
        name: "water",
        properties: &[LEVEL],
        light_opacity: 3,
        light_filter: 3,
        light_emission: 0,
        full_cube: false,
        occludes: false,
        material: Material::Liquid,
        render_layer: RenderLayer::Translucent,
        tint: TintKind::Water,
        liquid: Some(LiquidKind::Water),
        render: RenderKind::Liquid,
        collision: CollisionShape::None,
        hardness: 100.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Liquid,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: Some(LiquidKind::Lava),
        render: RenderKind::Liquid,
        collision: CollisionShape::None,
        hardness: 100.0,
        climbable: false,
        slipperiness: 0.60,
    },
    BlockBehaviour {
        id: 11,
        name: "lava",
        properties: &[LEVEL],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 15,
        full_cube: false,
        occludes: false,
        material: Material::Liquid,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: Some(LiquidKind::Lava),
        render: RenderKind::Liquid,
        collision: CollisionShape::None,
        hardness: 100.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Sand,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.5,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Sand,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.6,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 3.0,
        climbable: false,
        slipperiness: 0.60,
    },
    BlockBehaviour {
        id: 15,
        name: "iron_ore",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 3.0,
        climbable: false,
        slipperiness: 0.60,
    },
    BlockBehaviour {
        id: 16,
        name: "coal_ore",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 3.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 2.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Leaves,
        render_layer: RenderLayer::Solid,
        tint: TintKind::Foliage,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.2,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Glass,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.3,
        climbable: false,
        slipperiness: 0.60,
    },
    BlockBehaviour {
        id: 21,
        name: "lapis_ore",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 3.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.8,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Vine,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::Grass,
        liquid: None,
        render: RenderKind::Cross,
        collision: CollisionShape::None,
        hardness: 0.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Vine,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Cross,
        collision: CollisionShape::None,
        hardness: 0.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Cloth,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.8,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Plant,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Cross,
        collision: CollisionShape::None,
        hardness: 0.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Plant,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Cross,
        collision: CollisionShape::None,
        hardness: 0.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Plant,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Cross,
        collision: CollisionShape::None,
        hardness: 0.0,
        climbable: false,
        slipperiness: 0.60,
    },
    BlockBehaviour {
        id: 40,
        name: "red_mushroom",
        properties: &[],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        occludes: false,
        material: Material::Plant,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Cross,
        collision: CollisionShape::None,
        hardness: 0.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Metal,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 3.0,
        climbable: false,
        slipperiness: 0.60,
    },
    BlockBehaviour {
        id: 42,
        name: "iron_block",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        occludes: true,
        material: Material::Metal,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 5.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Slab { double: true },
        hardness: 2.0,
        climbable: false,
        slipperiness: 0.60,
    },
    BlockBehaviour {
        id: 45,
        name: "brick_block",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 2.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Tnt,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.0,
        climbable: false,
        slipperiness: 0.60,
    },
    BlockBehaviour {
        id: 47,
        name: "bookshelf",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        occludes: true,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 1.5,
        climbable: false,
        slipperiness: 0.60,
    },
    BlockBehaviour {
        id: 48,
        name: "mossy_cobblestone",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 2.0,
        climbable: false,
        slipperiness: 0.60,
    },
    BlockBehaviour {
        id: 49,
        name: "obsidian",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 50.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Circuit,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::None,
        hardness: 0.0,
        climbable: false,
        slipperiness: 0.60,
    },
    // BlockMobSpawner: no properties, non-opaque (BlockMobSpawner.java:57-60
    // overrides only isOpaqueCube), still a full cube — the default it keeps
    // (Block.java:366-369) — cutout, and the client registers no light
    // emission for it.
    BlockBehaviour {
        id: 52,
        name: "mob_spawner",
        properties: &[],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: true,
        occludes: false,
        material: Material::Rock,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 5.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Stairs,
        hardness: 2.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Chest,
        hardness: 2.5,
        climbable: false,
        slipperiness: 0.60,
    },
    BlockBehaviour {
        id: 56,
        name: "diamond_ore",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 3.0,
        climbable: false,
        slipperiness: 0.60,
    },
    BlockBehaviour {
        id: 57,
        name: "diamond_block",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        occludes: true,
        material: Material::Metal,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 5.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 2.5,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Plant,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Cross,
        collision: CollisionShape::None,
        hardness: 0.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Ground,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.6,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 3.5,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 3.5,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Wood,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Door,
        hardness: 3.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Circuit,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Ladder,
        hardness: 0.4,
        climbable: true,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Stairs,
        hardness: 2.0,
        climbable: false,
        slipperiness: 0.60,
    },
    // BlockPressurePlate with Material.wood: POWERED (BlockPressurePlate.java:17,
    // 73-89), the pressure plate geometry non-opaque; only metadata 1 reads as
    // pressed (getStateFromMeta, BlockPressurePlate.java:73-76), so the value
    // list is the source's own per-metadata answer.
    BlockBehaviour {
        id: 72,
        name: "wooden_pressure_plate",
        properties: &[PropertyDef {
            name: "powered",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 4,
                values: &PRESSURE_PLATE_POWERED,
            },
        }],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        occludes: false,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::None,
        hardness: 0.5,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 3.0,
        climbable: false,
        slipperiness: 0.60,
    },
    // BlockSnow: LAYERS, a PropertyInteger 1..=8 (BlockSnow.java:26) read from
    // the metadata as (meta & 7) + 1 (:151-153); the registration sets hardness
    // 0.1 and light opacity 0 (Block.java:1336), and the class answers
    // non-opaque (:53-56) and non-full-cube (:58-61), keeps the base SOLID
    // layer (Block.java:828) and carries Material.snow (:30) with the layer
    // collision box (:43-48).
    BlockBehaviour {
        id: 78,
        name: "snow_layer",
        properties: &[PropertyDef {
            name: "layers",
            kind: PropertyKind::Enum {
                offset: 0,
                bits: 3,
                values: &["1", "2", "3", "4", "5", "6", "7", "8"],
            },
        }],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        occludes: false,
        material: Material::Snow,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::SnowLayers,
        hardness: 0.1,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Ice,
        render_layer: RenderLayer::Translucent,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.5,
        climbable: false,
        slipperiness: 0.98,
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
        occludes: true,
        material: Material::CraftedSnow,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.2,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Cactus,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Cactus,
        hardness: 0.4,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Clay,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.6,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Plant,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::Grass,
        liquid: None,
        render: RenderKind::Cross,
        collision: CollisionShape::None,
        hardness: 0.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Fence,
        hardness: 2.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Gourd,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 1.0,
        climbable: false,
        slipperiness: 0.60,
    },
    BlockBehaviour {
        id: 87,
        name: "netherrack",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.4,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Sand,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::SoulSand,
        hardness: 0.5,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Glass,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.3,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 1.5,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.2,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.2,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Glass,
        render_layer: RenderLayer::CutoutMipped,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Pane,
        hardness: 0.3,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Grass,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.6,
        climbable: false,
        slipperiness: 0.60,
    },
    BlockBehaviour {
        id: 129,
        name: "emerald_ore",
        properties: &[],
        light_opacity: 255,
        light_filter: 15,
        light_emission: 0,
        full_cube: true,
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 3.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Plant,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Cross,
        collision: CollisionShape::None,
        hardness: 0.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: false,
        material: Material::Plant,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Cross,
        collision: CollisionShape::None,
        hardness: 0.0,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Rock,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.8,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Leaves,
        render_layer: RenderLayer::Solid,
        tint: TintKind::Foliage,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 0.2,
        climbable: false,
        slipperiness: 0.60,
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
        occludes: true,
        material: Material::Wood,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Model,
        collision: CollisionShape::Full,
        hardness: 2.0,
        climbable: false,
        slipperiness: 0.60,
    },
    // BlockBarrier: no properties, Material.barrier (Material.java:49), and the
    // registration's `setBlockUnbreakable` — hardness -1 (Blocks.java:433,
    // Block.java:407-409). `getRenderType()` answers -1, the dispatcher's "no
    // render" (BlockBarrier.java:19-25): the row's kind draws nothing — no
    // model, no fallback cube. `isOpaqueCube()` false (:27-33) and
    // `getAmbientOcclusionLightValue()` 1.0F (:35-41). No blockstate file:
    // `registerBuiltInBlocks` names it (BlockModelShapes.java:161). The row's
    // full_cube is false: the source's `isFullCube()` default is true, but the
    // column's readers are the ambient-occlusion light value and the fence and
    // wall connection rules, whose own barrier clauses answer false — false
    // keeps both readers on the source's answers. `render_layer` is inert for
    // the kind: no bucket is ever chosen.
    BlockBehaviour {
        id: 166,
        name: "barrier",
        properties: &[],
        light_opacity: 0,
        light_filter: 0,
        light_emission: 0,
        full_cube: false,
        occludes: false,
        material: Material::Barrier,
        render_layer: RenderLayer::Solid,
        tint: TintKind::None,
        liquid: None,
        render: RenderKind::Invisible,
        collision: CollisionShape::Full,
        hardness: -1.0,
        climbable: false,
        slipperiness: 0.60,
    },
    // BlockDoublePlant: HALF + VARIANT + FACING (BlockDoublePlant.java:28-39,
    // 281-316), Material.vine. The source tints the grass and fern variants with
    // the grass colour and leaves the rest white (colorMultiplier,
    // BlockDoublePlant.java:149-153); those two variants' double_grass and
    // double_fern models inherit block/tallgrass's tint index, while the
    // flowering variants' models are block/cross, which carries none, so this
    // value reaches no quad of theirs.
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
        occludes: false,
        material: Material::Vine,
        render_layer: RenderLayer::Cutout,
        tint: TintKind::Grass,
        liquid: None,
        render: RenderKind::Cross,
        collision: CollisionShape::None,
        hardness: 0.0,
        climbable: false,
        slipperiness: 0.60,
    },
];

/// The ids [`TABLE`] covers, sorted; the table's ids are this list in order.
const COVERED: [u16; 75] = [
    1, 2, 3, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 24, 31, 32, 35, 37, 38, 39,
    40, 41, 42, 43, 45, 46, 47, 48, 49, 50, 52, 53, 54, 56, 57, 58, 59, 60, 61, 62, 64, 65, 67, 72,
    73, 78, 79, 80, 81, 82, 83, 85, 86, 87, 88, 89, 98, 99, 100, 102, 110, 129, 141, 142, 155, 161,
    162, 166, 175,
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

    use crate::collision::{
        CollisionBox, Connections, Facing, SlabHalf, cactus_box, chest_box, door_box, fence_boxes,
        ladder_box, pane_boxes, slab_boxes, snow_layer_box, soul_sand_box, stairs_boxes, wall_box,
    };

    /// The row for a covered id.
    fn block(id: u16) -> &'static BlockBehaviour {
        behaviour(id).unwrap_or_else(|| panic!("id {id} is covered"))
    }

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
        // wood, and the table falls back to the first of its listed woods —
        // this project's own choice for a state the source cannot build (its
        // lookup would name a wood the filtered property does not allow).
        let log2 = behaviour(162).expect("log2");
        assert_eq!(variant_key(log2, 2), "axis=y,variant=acacia");
        assert_eq!(variant_key(log2, 3), "axis=y,variant=acacia");
        assert_eq!(variant_key(log2, 6), "axis=x,variant=acacia");
        // The huge mushroom's lookup leaves three slots unset.
        let mushroom = behaviour(99).expect("mushroom");
        assert_eq!(variant_key(mushroom, 11), "variant=all_inside");
        assert_eq!(variant_key(mushroom, 15), "variant=all_stem");
    }

    #[test]
    fn the_movement_columns_name_the_sources_classes() {
        // Every covered id's collision class, from its source block class'
        // overrides. The ids that are not the full cube:
        let classes: [(u16, CollisionShape); 28] = [
            (8, CollisionShape::None),
            (9, CollisionShape::None),
            (10, CollisionShape::None),
            (11, CollisionShape::None),
            (31, CollisionShape::None),
            (32, CollisionShape::None),
            (37, CollisionShape::None),
            (38, CollisionShape::None),
            (39, CollisionShape::None),
            (40, CollisionShape::None),
            (43, CollisionShape::Slab { double: true }),
            (50, CollisionShape::None),
            (53, CollisionShape::Stairs),
            (54, CollisionShape::Chest),
            (59, CollisionShape::None),
            (64, CollisionShape::Door),
            (65, CollisionShape::Ladder),
            (67, CollisionShape::Stairs),
            (72, CollisionShape::None),
            (78, CollisionShape::SnowLayers),
            (81, CollisionShape::Cactus),
            (83, CollisionShape::None),
            (85, CollisionShape::Fence),
            (88, CollisionShape::SoulSand),
            (102, CollisionShape::Pane),
            (141, CollisionShape::None),
            (142, CollisionShape::None),
            (175, CollisionShape::None),
        ];
        for &id in covered_ids() {
            let entry = behaviour(id).expect("covered");
            let expected = classes
                .iter()
                .find(|(class_id, _)| *class_id == id)
                .map_or(CollisionShape::Full, |(_, shape)| *shape);
            assert_eq!(entry.collision, expected, "id {id}'s collision class");
        }
    }

    #[test]
    fn the_shape_functions_carry_the_sources_boxes() {
        // A slab's halves and the double variant's cube (`BlockSlab.java:34`,
        // `:49`, `:57-64`).
        assert_eq!(
            slab_boxes(false, SlabHalf::Bottom)[0],
            CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.5, 1.0])
        );
        assert_eq!(
            slab_boxes(false, SlabHalf::Top)[0],
            CollisionBox::of([0.0, 0.5, 0.0], [1.0, 1.0, 1.0])
        );
        assert_eq!(slab_boxes(true, SlabHalf::Bottom)[0], CollisionBox::full());

        // A lone stair's base half and step box per facing
        // (`BlockStairs.java:80-90`, `:292-406`).
        let base_bottom = CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.5, 1.0]);
        let base_top = CollisionBox::of([0.0, 0.5, 0.0], [1.0, 1.0, 1.0]);
        for (facing, step, top_step) in [
            (
                Facing::North,
                CollisionBox::of([0.0, 0.5, 0.0], [1.0, 1.0, 0.5]),
                CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.5, 0.5]),
            ),
            (
                Facing::South,
                CollisionBox::of([0.0, 0.5, 0.5], [1.0, 1.0, 1.0]),
                CollisionBox::of([0.0, 0.0, 0.5], [1.0, 0.5, 1.0]),
            ),
            (
                Facing::West,
                CollisionBox::of([0.0, 0.5, 0.0], [0.5, 1.0, 1.0]),
                CollisionBox::of([0.0, 0.0, 0.0], [0.5, 0.5, 1.0]),
            ),
            (
                Facing::East,
                CollisionBox::of([0.5, 0.5, 0.0], [1.0, 1.0, 1.0]),
                CollisionBox::of([0.5, 0.0, 0.0], [1.0, 0.5, 1.0]),
            ),
        ] {
            let lower = stairs_boxes(facing, SlabHalf::Bottom, |_| None);
            assert_eq!(lower, vec![base_bottom, step], "a bottom stair, {facing:?}");
            let top = stairs_boxes(facing, SlabHalf::Top, |_| None);
            assert_eq!(top, vec![base_top, top_step], "a top stair, {facing:?}");
        }

        // The fence's post and arms (`BlockFence.java:50-107`).
        assert_eq!(
            fence_boxes(Connections::default()),
            vec![CollisionBox::of([0.375, 0.0, 0.375], [0.625, 1.5, 0.625])]
        );
        assert_eq!(
            fence_boxes(Connections {
                north: true,
                south: true,
                west: true,
                east: true,
            }),
            vec![
                CollisionBox::of([0.375, 0.0, 0.0], [0.625, 1.5, 1.0]),
                CollisionBox::of([0.0, 0.0, 0.375], [1.0, 1.5, 0.625]),
            ]
        );

        // The cactus' inset (`BlockCactus.java:63-68`).
        assert_eq!(
            cactus_box(),
            CollisionBox::of([0.0625, 0.0, 0.0625], [0.9375, 0.9375, 0.9375])
        );

        // The snow layer heights, one per layer: the property runs 1..=8 and
        // the top stands (layers - 1)/8 above the block's base
        // (`BlockSnow.java:43-48`, `:71-80`).
        let heights = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875];
        for (index, height) in heights.iter().enumerate() {
            let layers = index as u8 + 1;
            assert_eq!(
                snow_layer_box(layers),
                CollisionBox::of([0.0, 0.0, 0.0], [1.0, *height, 1.0]),
                "{layers} layers"
            );
        }

        // The wall's post, arms and 1.5 collision height
        // (`BlockWall.java:67-120`).
        assert_eq!(
            wall_box(Connections {
                north: true,
                east: true,
                ..Connections::default()
            }),
            CollisionBox::of([0.25, 0.0, 0.0], [1.0, 1.5, 0.75])
        );

        // The ladder's plate. The task brief pinned this class as `NONE`; the
        // source's `getCollisionBoundingBox` answers a 1/8 plate on the
        // attached face (`BlockLadder.java:28-32`, `:40-66`), so the pin is
        // refuted and the plate is pinned here.
        assert_eq!(
            ladder_box(Facing::North),
            CollisionBox::of([0.0, 0.0, 0.875], [1.0, 1.0, 1.0])
        );

        // Soul sand's top (`BlockSoulSand.java:20-24`).
        assert_eq!(
            soul_sand_box(),
            CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.875, 1.0])
        );

        // The chest box and its merge toward a same-block neighbour
        // (`BlockChest.java:42`, `:66-88`).
        assert_eq!(
            chest_box(false, false, false, false),
            CollisionBox::of([0.0625, 0.0, 0.0625], [0.9375, 0.875, 0.9375])
        );
        assert_eq!(chest_box(true, false, false, false).min[2], 0.0);
        assert_eq!(chest_box(false, false, false, true).max[0], 1.0);

        // The door's closed plate and its open swing (`BlockDoor.java:83-154`).
        assert_eq!(
            door_box(Facing::North, false, false),
            CollisionBox::of([0.0, 0.0, 0.8125], [1.0, 1.0, 1.0])
        );
        assert_eq!(
            door_box(Facing::North, true, false),
            CollisionBox::of([0.0, 0.0, 0.0], [0.1875, 1.0, 1.0])
        );

        // The pane's cross alone, and the single plate a one-sided connection
        // yields (`BlockPane.java:75-122`).
        assert_eq!(pane_boxes(Connections::default()).len(), 2);
        assert_eq!(
            pane_boxes(Connections {
                west: true,
                ..Connections::default()
            })
            .len(),
            1
        );
    }

    #[test]
    fn the_hardness_column_carries_the_sources_values() {
        // The registration lines (`block/Block.java`), the classes' own
        // constructors (logs and leaves through `BlockLog.java:21` and
        // `BlockLeaves.java:32`, the crops through `BlockCrops.java:28`, the
        // double plant through `BlockDoublePlant.java:36`) and the stairs'
        // model copy (`BlockStairs.java:45`).
        assert_eq!(block(1).hardness, 1.5); // stone, Block.java:1252
        assert_eq!(block(3).hardness, 0.5); // dirt, :1254
        assert_eq!(block(7).hardness, -1.0); // bedrock's setBlockUnbreakable, :1260
        assert_eq!(block(9).hardness, 100.0); // water, :1262
        assert_eq!(block(17).hardness, 2.0); // log, BlockLog.java:21
        assert_eq!(block(18).hardness, 0.2); // leaves, BlockLeaves.java:32
        assert_eq!(block(20).hardness, 0.3); // glass, :1273
        assert_eq!(block(35).hardness, 0.8); // wool, :1289
        assert_eq!(block(49).hardness, 50.0); // obsidian, :1306
        assert_eq!(block(53).hardness, 2.0); // oak stairs' planks model, :1310 and :1258
        assert_eq!(block(59).hardness, 0.0); // wheat, BlockCrops.java:28
        assert_eq!(block(67).hardness, 2.0); // stone stairs' model, :1325 and :1256
        assert_eq!(block(88).hardness, 0.5); // soul sand, :1347
        assert_eq!(block(102).hardness, 0.3); // glass pane, :1362
        assert_eq!(block(141).hardness, 0.0); // carrots, BlockCrops.java:28
        assert_eq!(block(142).hardness, 0.0); // potatoes, BlockCrops.java:28
        assert_eq!(block(161).hardness, 0.2); // leaves2, BlockLeaves.java:32
        assert_eq!(block(162).hardness, 2.0); // log2, BlockLog.java:21
        assert_eq!(block(175).hardness, 0.0); // double plant, BlockDoublePlant.java:36

        // The rest of the covered column, each value from its registration
        // line in `block/Block.java`.
        assert_eq!(block(2).hardness, 0.6); // grass, :1253
        assert_eq!(block(4).hardness, 2.0); // cobblestone, :1255
        assert_eq!(block(5).hardness, 2.0); // planks, :1257
        assert_eq!(block(8).hardness, 100.0); // flowing_water, :1261
        assert_eq!(block(10).hardness, 100.0); // flowing_lava, :1263
        assert_eq!(block(11).hardness, 100.0); // lava, :1264
        assert_eq!(block(12).hardness, 0.5); // sand, :1265
        assert_eq!(block(13).hardness, 0.6); // gravel, :1266
        assert_eq!(block(14).hardness, 3.0); // gold_ore, :1267
        assert_eq!(block(15).hardness, 3.0); // iron_ore, :1268
        assert_eq!(block(16).hardness, 3.0); // coal_ore, :1269
        assert_eq!(block(21).hardness, 3.0); // lapis_ore, :1274
        assert_eq!(block(24).hardness, 0.8); // sandstone, :1277
        assert_eq!(block(31).hardness, 0.0); // tallgrass, :1285
        assert_eq!(block(32).hardness, 0.0); // deadbush, :1286
        assert_eq!(block(37).hardness, 0.0); // yellow_flower, :1291
        assert_eq!(block(38).hardness, 0.0); // red_flower, :1292
        assert_eq!(block(39).hardness, 0.0); // brown_mushroom, :1293
        assert_eq!(block(40).hardness, 0.0); // red_mushroom, :1295
        assert_eq!(block(41).hardness, 3.0); // gold_block, :1297
        assert_eq!(block(42).hardness, 5.0); // iron_block, :1298
        assert_eq!(block(43).hardness, 2.0); // double_stone_slab, :1299
        assert_eq!(block(45).hardness, 2.0); // brick_block, :1301
        assert_eq!(block(46).hardness, 0.0); // tnt, :1303
        assert_eq!(block(47).hardness, 1.5); // bookshelf, :1304
        assert_eq!(block(48).hardness, 2.0); // mossy_cobblestone, :1305
        assert_eq!(block(50).hardness, 0.0); // torch, :1307
        assert_eq!(block(52).hardness, 5.0); // mob_spawner, :1309
        assert_eq!(block(54).hardness, 2.5); // chest, :1311
        assert_eq!(block(56).hardness, 3.0); // diamond_ore, :1313
        assert_eq!(block(57).hardness, 5.0); // diamond_block, :1314
        assert_eq!(block(58).hardness, 2.5); // crafting_table, :1315
        assert_eq!(block(60).hardness, 0.6); // farmland, :1317
        assert_eq!(block(61).hardness, 3.5); // furnace, :1319
        assert_eq!(block(62).hardness, 3.5); // lit_furnace, :1320
        assert_eq!(block(64).hardness, 3.0); // wooden_door, :1322
        assert_eq!(block(65).hardness, 0.4); // ladder, :1323
        assert_eq!(block(72).hardness, 0.5); // wooden_pressure_plate, :1330
        assert_eq!(block(73).hardness, 3.0); // redstone_ore, :1331
        assert_eq!(block(79).hardness, 0.5); // ice, :1337
        assert_eq!(block(80).hardness, 0.2); // snow, :1338
        assert_eq!(block(81).hardness, 0.4); // cactus, :1339
        assert_eq!(block(82).hardness, 0.6); // clay, :1340
        assert_eq!(block(83).hardness, 0.0); // reeds, :1341
        assert_eq!(block(85).hardness, 2.0); // fence, :1343
        assert_eq!(block(86).hardness, 1.0); // pumpkin, :1344
        assert_eq!(block(87).hardness, 0.4); // netherrack, :1346
        assert_eq!(block(89).hardness, 0.3); // glowstone, :1348
        assert_eq!(block(98).hardness, 1.5); // stonebrick, :1357
        assert_eq!(block(99).hardness, 0.2); // brown_mushroom_block, :1359
        assert_eq!(block(100).hardness, 0.2); // red_mushroom_block, :1360
        assert_eq!(block(110).hardness, 0.6); // mycelium, :1371
        assert_eq!(block(129).hardness, 3.0); // emerald_ore, :1391
        assert_eq!(block(155).hardness, 0.8); // quartz_block, :1417
    }

    #[test]
    fn the_movement_metadata_is_the_source_default_everywhere_but_its_overrides() {
        for &id in covered_ids() {
            let entry = behaviour(id).expect("covered");
            assert_eq!(
                entry.climbable,
                id == 65,
                "id {id}: only the ladder passes isOnLadder's test"
            );
            assert_eq!(
                entry.slipperiness,
                if id == 79 { 0.98 } else { 0.6 },
                "id {id}: Block.java:291's default, BlockIce.java:23's ice"
            );
            assert!(
                entry.hardness >= -1.0 && entry.hardness.is_finite(),
                "id {id}: a hardness above the unbreakable sentinel"
            );
        }
    }
}
