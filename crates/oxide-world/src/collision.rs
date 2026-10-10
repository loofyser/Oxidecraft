//! The collision boxes the movement model resolves against.
//!
//! One [`CollisionBox`] is one axis-aligned box of world space. The source
//! calls this an `AxisAlignedBB` (`refs/_src/MCP-919/src/minecraft/net/minecraft/util/AxisAlignedBB.java`);
//! a block hands out its box through `Block.getCollisionBoundingBox`, and
//! `World.getCollidingBoundingBoxes` (`World.java:1262-1307`) collects the
//! boxes of every block a query box touches. The behaviour table's rows carry
//! the boxes each block class declares; the movement model (and the
//! interaction raycast with it) resolves against the boxes, not the blocks.
//!
//! Every box is stored as the source stores it — absolute world coordinates,
//! `min` on the low side and `max` on the high side — so `full()` at block
//! `(x, y, z)` is the block's own unit cube and the block classes' own
//! partial boxes are built with [`CollisionBox::of`] and placed with
//! [`CollisionBox::offset`].
//!
//! Unlike the source's `AxisAlignedBB`, which carries a growth vector and a
//! re-centring rule for entity boxes, this is a plain pair of corners: the
//! movement model's own arithmetic (`addCoord`, `expand`, `intersectsWith`)
//! lives beside the rules that use it.
//!
//! # The per-state shape functions
//!
//! The source's blocks produce their boxes in two steps: a class's
//! `addCollisionBoxesToList`/`getCollisionBoundingBox` reads the block state
//! (and, for a few classes, the neighbouring blocks) and turns it into one or
//! more `AxisAlignedBB`s. The functions in this module are that second step —
//! pure, block-local boxes with no world storage — and the behaviour table's
//! `collision` column names which one a block id resolves through. Decoding a
//! metadata nibble into the property a function reads has its own small
//! helpers here ([`slab_half`], [`stair_half`], [`stair_facing`],
//! [`front_facing`], [`door_facing`]).
//!
//! Two classes read their neighbours: a stair's step and corner boxes are
//! shaped by the stairs around it ([`stairs_boxes`] takes the neighbour
//! lookup), and a fence, wall or pane's arms are resolved from what its
//! neighbours connect to ([`Connections`], filled by the caller, whose
//! predicates live beside the block classes' `canConnectTo` rules — see
//! `oxide-game`'s `WorldView`).

/// An axis-aligned collision box in world coordinates.
///
/// `min[i] <= max[i]` for the three axes; the source never normalises a box,
/// so neither does this type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollisionBox {
    /// The low corner.
    pub min: [f64; 3],
    /// The high corner.
    pub max: [f64; 3],
}

impl CollisionBox {
    /// The full-block box, `[0, 1]` on every axis.
    ///
    /// The box a default full cube reports: `Block.getCollisionBoundingBox`
    /// returns the block's `minX..maxZ` bounds, which a full cube leaves at
    /// their defaults (`block/Block.java:499-502`).
    pub const fn full() -> CollisionBox {
        CollisionBox {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 1.0, 1.0],
        }
    }

    /// The box from two explicit corners, in block-local or world space.
    ///
    /// The partial boxes the block classes declare: a bottom slab is
    /// `of([0, 0, 0], [1, 0.5, 1])` (`BlockSlab.java:34`'s
    /// `setBlockBounds(0.0F, 0.0F, 0.0F, 1.0F, 0.5F, 1.0F)`), a ladder a thin
    /// plate on its attached face (`BlockLadder.setBlockBoundsBasedOnState`).
    pub const fn of(min: [f64; 3], max: [f64; 3]) -> CollisionBox {
        CollisionBox { min, max }
    }

    /// The box moved by the offset — the source's `AxisAlignedBB.offset`
    /// (`AxisAlignedBB.java:117-125`).
    pub const fn offset(self, x: f64, y: f64, z: f64) -> CollisionBox {
        CollisionBox {
            min: [self.min[0] + x, self.min[1] + y, self.min[2] + z],
            max: [self.max[0] + x, self.max[1] + y, self.max[2] + z],
        }
    }
}

/// A horizontal direction, as the block-state facing properties carry it.
///
/// The source's `EnumFacing` has a vertical pair too; every shape in this
/// module reads the horizontal four.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facing {
    /// −Z.
    North,
    /// +Z.
    South,
    /// −X.
    West,
    /// +X.
    East,
}

impl Facing {
    /// The cell offset the direction points at.
    pub const fn offset(self) -> (i32, i32) {
        match self {
            Facing::North => (0, -1),
            Facing::South => (0, 1),
            Facing::West => (-1, 0),
            Facing::East => (1, 0),
        }
    }
}

/// Which half of its cell a slab or stair occupies.
///
/// `BlockSlab.EnumBlockHalf`: `BOTTOM` holds the low half, `TOP` the high
/// one (`BlockSlab.java:45-67`, `BlockStairs.java:80-90`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlabHalf {
    /// The low half, `[0, 0.5]` on y.
    Bottom,
    /// The high half, `[0.5, 1]` on y.
    Top,
}

/// The horizontal connections of a fence, a wall or a pane.
///
/// The four flags are the source's `NORTH`/`EAST`/`WEST`/`SOUTH` connection
/// properties, resolved from the neighbour blocks by the caller: a fence
/// through `BlockFence.canConnectTo` (`BlockFence.java:161`), a wall through
/// `BlockWall.canConnectTo` (`BlockWall.java:122`), a pane through
/// `BlockPane.canPaneConnectToBlock` (`BlockPane.java:177`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Connections {
    /// Whether the block connects to its neighbour to the north.
    pub north: bool,
    /// Whether the block connects to its neighbour to the east.
    pub east: bool,
    /// Whether the block connects to its neighbour to the west.
    pub west: bool,
    /// Whether the block connects to its neighbour to the south.
    pub south: bool,
}

/// One neighbouring stair's state, as the stair shape rules read it.
///
/// The source reads a neighbour's `FACING` and `HALF` and compares them with
/// the queried stair's own (`BlockStairs.isSameStair`, `BlockStairs.java:103-108`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StairState {
    /// The neighbour's facing.
    pub facing: Facing,
    /// The neighbour's half.
    pub half: SlabHalf,
}

/// The half a slab's metadata names: bit 3, high when set.
///
/// `BlockStoneSlab.getStateFromMeta`: `HALF` is `TOP` when `meta & 8` is set
/// (`BlockStoneSlab.java:94-108`, the bit at `:104`), and double slabs carry no half.
pub const fn slab_half(meta: u8) -> SlabHalf {
    if meta & 8 != 0 {
        SlabHalf::Top
    } else {
        SlabHalf::Bottom
    }
}

/// The half a stair's metadata names: bit 2, high when set.
///
/// `BlockStairs.getStateFromMeta` reads
/// `(meta & 4) > 0 ? TOP : BOTTOM` (`BlockStairs.java:728`), and
/// `getMetaFromState` writes `i |= 4` for the top half (`:736-742`) — not the
/// slab's bit 3.
pub const fn stair_half(meta: u8) -> SlabHalf {
    if meta & 4 != 0 {
        SlabHalf::Top
    } else {
        SlabHalf::Bottom
    }
}

/// The facing a stair's metadata names.
///
/// `BlockStairs.getStateFromMeta` reads `EnumFacing.getFront(5 - (meta & 3))`
/// (`BlockStairs.java:728-729`), whose value is [`Facing::East`] at metadata
/// 0, west at 1, south at 2 and north at 3.
pub const fn stair_facing(meta: u8) -> Facing {
    match meta & 3 {
        0 => Facing::East,
        1 => Facing::West,
        2 => Facing::South,
        _ => Facing::North,
    }
}

/// The horizontal front a facing block's metadata names.
///
/// `BlockFurnace`, `BlockChest` and `BlockLadder`'s `getStateFromMeta`
/// (`BlockLadder.java:140-150`) read
/// `EnumFacing.getFront(meta)` and fold the vertical faces to north; the
/// source's front list wraps the index modulo six
/// (`EnumFacing.getFront`), which puts north at 0, 1, 2 and 8, south at 3
/// and 9, west at 4 and 10, and east at 5 and 11.
pub const fn front_facing(meta: u8) -> Facing {
    match meta % 6 {
        3 => Facing::South,
        4 => Facing::West,
        5 => Facing::East,
        _ => Facing::North,
    }
}

/// The facing a door's combined metadata names.
///
/// `BlockDoor.getFacing` runs `EnumFacing.getHorizontal(meta & 3).rotateYCCW()`
/// (`BlockDoor.java:421-424`), which is east at 0, south at 1, west at 2 and
/// north at 3 — the order the behaviour table's door facings pin.
pub const fn door_facing(combined_meta: u8) -> Facing {
    match combined_meta & 3 {
        0 => Facing::East,
        1 => Facing::South,
        2 => Facing::West,
        _ => Facing::North,
    }
}

/// A slab's box: the full cube for the double variant, the named half
/// otherwise.
///
/// `BlockSlab`'s constructor and `setBlockBoundsBasedOnState`
/// (`BlockSlab.java:28-35`, `:45-67`): the double slab fills its cell, the
/// single one is `[0, 0.5]` or `[0.5, 1]` on y.
pub fn slab_boxes(double: bool, half: SlabHalf) -> [CollisionBox; 1] {
    if double {
        return [CollisionBox::full()];
    }
    match half {
        SlabHalf::Bottom => [CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.5, 1.0])],
        SlabHalf::Top => [CollisionBox::of([0.0, 0.5, 0.0], [1.0, 1.0, 1.0])],
    }
}

/// A stair's collision boxes, in the order the source adds them.
///
/// `BlockStairs.addCollisionBoxesToList` (`BlockStairs.java:533-546`) adds
/// three boxes: the base half (`setBaseCollisionBounds`, `:80-90`), the step
/// box its facing and any matching neighbour shape
/// (`func_176306_h`, `:292-406`) and — when the step box reported an open
/// corner — the corner box a turning pair of stairs opens up
/// (`func_176304_i`, `:408-528`). `neighbour` answers the stair state at the
/// cell one step in a direction, or `None` when that cell is not a stair.
pub fn stairs_boxes(
    facing: Facing,
    half: SlabHalf,
    neighbour: impl Fn(Facing) -> Option<StairState>,
) -> Vec<CollisionBox> {
    let state = StairState { facing, half };
    let is_same_stair = |dir: Facing| neighbour(dir) == Some(state);

    let mut boxes = Vec::with_capacity(3);

    // The base half (`setBaseCollisionBounds`).
    boxes.push(match half {
        SlabHalf::Bottom => CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.5, 1.0]),
        SlabHalf::Top => CollisionBox::of([0.0, 0.5, 0.0], [1.0, 1.0, 1.0]),
    });

    // The step box (`func_176306_h`): the upper (lower for a top half)
    // quadrant the stair rises into, shrunk where the neighbouring stairs
    // continue the run.
    let top = half == SlabHalf::Top;
    let (y0, y1) = if top { (0.0, 0.5) } else { (0.5, 1.0) };
    let mut x0 = 0.0;
    let mut x1 = 1.0;
    let mut z0 = 0.0;
    let mut z1 = 0.5;
    let mut open = true;
    match facing {
        Facing::East => {
            x0 = 0.5;
            z1 = 1.0;
            if let Some(other) = neighbour(Facing::East) {
                if other.half == half {
                    if other.facing == Facing::North && !is_same_stair(Facing::South) {
                        z1 = 0.5;
                        open = false;
                    } else if other.facing == Facing::South && !is_same_stair(Facing::North) {
                        z0 = 0.5;
                        open = false;
                    }
                }
            }
        }
        Facing::West => {
            x1 = 0.5;
            z1 = 1.0;
            if let Some(other) = neighbour(Facing::West) {
                if other.half == half {
                    if other.facing == Facing::North && !is_same_stair(Facing::South) {
                        z1 = 0.5;
                        open = false;
                    } else if other.facing == Facing::South && !is_same_stair(Facing::North) {
                        z0 = 0.5;
                        open = false;
                    }
                }
            }
        }
        Facing::South => {
            z0 = 0.5;
            z1 = 1.0;
            if let Some(other) = neighbour(Facing::South) {
                if other.half == half {
                    if other.facing == Facing::West && !is_same_stair(Facing::East) {
                        x1 = 0.5;
                        open = false;
                    } else if other.facing == Facing::East && !is_same_stair(Facing::West) {
                        x0 = 0.5;
                        open = false;
                    }
                }
            }
        }
        Facing::North => {
            if let Some(other) = neighbour(Facing::North) {
                if other.half == half {
                    if other.facing == Facing::West && !is_same_stair(Facing::East) {
                        x1 = 0.5;
                        open = false;
                    } else if other.facing == Facing::East && !is_same_stair(Facing::West) {
                        x0 = 0.5;
                        open = false;
                    }
                }
            }
        }
    }
    boxes.push(CollisionBox::of([x0, y0, z0], [x1, y1, z1]));

    // The corner box (`func_176304_i`): a pair of stairs turned through each
    // other leaves an open quadrant, and one box fills it.
    if open {
        let (mut x0, mut x1, mut z0, mut z1) = (0.0, 0.5, 0.5, 1.0);
        let mut corners = false;
        match facing {
            Facing::East => {
                if let Some(other) = neighbour(Facing::West) {
                    if other.half == half {
                        if other.facing == Facing::North && !is_same_stair(Facing::North) {
                            z0 = 0.0;
                            z1 = 0.5;
                            corners = true;
                        } else if other.facing == Facing::South && !is_same_stair(Facing::South) {
                            corners = true;
                        }
                    }
                }
            }
            Facing::West => {
                if let Some(other) = neighbour(Facing::East) {
                    if other.half == half {
                        x0 = 0.5;
                        x1 = 1.0;
                        if other.facing == Facing::North && !is_same_stair(Facing::North) {
                            z0 = 0.0;
                            z1 = 0.5;
                            corners = true;
                        } else if other.facing == Facing::South && !is_same_stair(Facing::South) {
                            corners = true;
                        }
                    }
                }
            }
            Facing::South => {
                if let Some(other) = neighbour(Facing::North) {
                    if other.half == half {
                        z0 = 0.0;
                        z1 = 0.5;
                        if other.facing == Facing::West && !is_same_stair(Facing::West) {
                            corners = true;
                        } else if other.facing == Facing::East && !is_same_stair(Facing::East) {
                            x0 = 0.5;
                            x1 = 1.0;
                            corners = true;
                        }
                    }
                }
            }
            Facing::North => {
                if let Some(other) = neighbour(Facing::South) {
                    if other.half == half {
                        if other.facing == Facing::West && !is_same_stair(Facing::West) {
                            corners = true;
                        } else if other.facing == Facing::East && !is_same_stair(Facing::East) {
                            x0 = 0.5;
                            x1 = 1.0;
                            corners = true;
                        }
                    }
                }
            }
        }
        if corners {
            boxes.push(CollisionBox::of([x0, y0, z0], [x1, y1, z1]));
        }
    }

    boxes
}

/// A snow layer's box: the full footprint, the height `(layers - 1) / 8`.
///
/// `BlockSnow.getCollisionBoundingBox` (`BlockSnow.java:43-48`): the `LAYERS`
/// property runs 1..=8 and the top stands `0.125` per layer short of the
/// block's own top, a one-layer drift having no collision box at all. The
/// count is clamped to that range, so an out-of-range value still answers the
/// base or the full layer.
pub fn snow_layer_box(layers: u8) -> CollisionBox {
    let layers = layers.clamp(1, 8);
    let height = f64::from(u32::from(layers - 1)) * 0.125;
    CollisionBox::of([0.0, 0.0, 0.0], [1.0, height, 1.0])
}

/// A cactus's box: 1/16 short of the cell on three sides.
///
/// `BlockCactus.getCollisionBoundingBox` (`BlockCactus.java:63-68`): the
/// box's sides are `x + 1/16` to `x + 1 − 1/16`, its top `y + 1 − 1/16` and
/// its bottom the block's own base.
pub const fn cactus_box() -> CollisionBox {
    CollisionBox::of([0.0625, 0.0, 0.0625], [0.9375, 0.9375, 0.9375])
}

/// A fence's boxes: its post and the two arms the connections open.
///
/// `BlockFence.addCollisionBoxesToList` (`BlockFence.java:50-107`): the
/// 0.375..0.625 post spans the north–south axis when either end connects,
/// the west–east axis when either of those connects or when neither
/// north–south does, and both arms reach out to the full cell on the
/// connected sides. Every box spans `y` 0..1.5.
pub fn fence_boxes(connections: Connections) -> Vec<CollisionBox> {
    let mut boxes = Vec::with_capacity(2);
    if connections.north || connections.south {
        boxes.push(CollisionBox::of(
            [0.375, 0.0, if connections.north { 0.0 } else { 0.375 }],
            [0.625, 1.5, if connections.south { 1.0 } else { 0.625 }],
        ));
    }
    if connections.west || connections.east || !(connections.north || connections.south) {
        boxes.push(CollisionBox::of(
            [if connections.west { 0.0 } else { 0.375 }, 0.0, 0.375],
            [if connections.east { 1.0 } else { 0.625 }, 1.5, 0.625],
        ));
    }
    boxes
}

/// A cobblestone wall's box: its post and arms, 1.5 high.
///
/// `BlockWall.setBlockBoundsBasedOnState` (`BlockWall.java:67-113`) builds
/// the 0.25..0.75 footprint, reaching the cell's edge on each connected
/// side, and narrows it to 0.3125..0.6875 across the run when only one axis
/// connects. `getCollisionBoundingBox` (`:115-120`) then raises the box's
/// top to 1.5 — the wall's collision stands taller than the block it sets
/// its render bounds at.
pub fn wall_box(connections: Connections) -> CollisionBox {
    let mut x0 = 0.25;
    let mut x1 = 0.75;
    let mut z0 = 0.25;
    let mut z1 = 0.75;
    if connections.north {
        z0 = 0.0;
    }
    if connections.south {
        z1 = 1.0;
    }
    if connections.west {
        x0 = 0.0;
    }
    if connections.east {
        x1 = 1.0;
    }
    if connections.north && connections.south && !connections.west && !connections.east {
        x0 = 0.3125;
        x1 = 0.6875;
    } else if !connections.north && !connections.south && connections.west && connections.east {
        z0 = 0.3125;
        z1 = 0.6875;
    }
    CollisionBox::of([x0, 0.0, z0], [x1, 1.5, z1])
}

/// A pane's boxes: up to two plates, one per axis.
///
/// `BlockPane.addCollisionBoxesToList` (`BlockPane.java:75-122`): each plate
/// is 2/16 thick and reaches half a cell toward a single connected side, the
/// full cell when both sides of its axis connect — or when nothing connects
/// at all, which is the cross a lone pane stands as.
pub fn pane_boxes(connections: Connections) -> Vec<CollisionBox> {
    let mut boxes = Vec::with_capacity(2);
    let any = connections.north || connections.south || connections.west || connections.east;
    if (!connections.west || !connections.east) && any {
        if connections.west {
            boxes.push(CollisionBox::of([0.0, 0.0, 0.4375], [0.5, 1.0, 0.5625]));
        } else if connections.east {
            boxes.push(CollisionBox::of([0.5, 0.0, 0.4375], [1.0, 1.0, 0.5625]));
        }
    } else {
        boxes.push(CollisionBox::of([0.0, 0.0, 0.4375], [1.0, 1.0, 0.5625]));
    }
    if (!connections.north || !connections.south) && any {
        if connections.north {
            boxes.push(CollisionBox::of([0.4375, 0.0, 0.0], [0.5625, 1.0, 0.5]));
        } else if connections.south {
            boxes.push(CollisionBox::of([0.4375, 0.0, 0.5], [0.5625, 1.0, 1.0]));
        }
    } else {
        boxes.push(CollisionBox::of([0.4375, 0.0, 0.0], [0.5625, 1.0, 1.0]));
    }
    boxes
}

/// A chest's box: 14/16 across, its top at 7/8.
///
/// `BlockChest`'s constructor and `setBlockBoundsBasedOnState`
/// (`BlockChest.java:42`, `:66-88`): `[0.0625, 0, 0.0625]` to
/// `[0.9375, 0.875, 0.9375]`, reaching the cell's edge on the side a chest of
/// the same block sits on — the two boxes of a double chest meet in the
/// middle instead of leaving the seam gap.
pub fn chest_box(north: bool, south: bool, west: bool, east: bool) -> CollisionBox {
    if north {
        CollisionBox::of([0.0625, 0.0, 0.0], [0.9375, 0.875, 0.9375])
    } else if south {
        CollisionBox::of([0.0625, 0.0, 0.0625], [0.9375, 0.875, 1.0])
    } else if west {
        CollisionBox::of([0.0, 0.0, 0.0625], [0.9375, 0.875, 0.9375])
    } else if east {
        CollisionBox::of([0.0625, 0.0, 0.0625], [1.0, 0.875, 0.9375])
    } else {
        CollisionBox::of([0.0625, 0.0, 0.0625], [0.9375, 0.875, 0.9375])
    }
}

/// A soul sand block's box: the full footprint, its top at 7/8.
///
/// `BlockSoulSand.getCollisionBoundingBox` (`BlockSoulSand.java:20-24`):
/// `[0, 0, 0]` to `[1, 0.875, 1]` — the surface sits an eighth of a block
/// below the cell's top, the box the entity drags its feet through
/// (`onEntityCollidedWithBlock` beside it keeps the slowing itself, `:30-33`).
pub const fn soul_sand_box() -> CollisionBox {
    CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.875, 1.0])
}

/// A ladder's box: a 2/16 plate on its attached face.
///
/// `BlockLadder.getCollisionBoundingBox` and `setBlockBoundsBasedOnState`
/// (`BlockLadder.java:28-32`, `:40-66`): a facing north ladder lies at
/// `z` 0.875..1, south at `z` 0..0.125, west at `x` 0.875..1 and east at
/// `x` 0..0.125, full height.
pub const fn ladder_box(facing: Facing) -> CollisionBox {
    match facing {
        Facing::North => CollisionBox::of([0.0, 0.0, 0.875], [1.0, 1.0, 1.0]),
        Facing::South => CollisionBox::of([0.0, 0.0, 0.0], [1.0, 1.0, 0.125]),
        Facing::West => CollisionBox::of([0.875, 0.0, 0.0], [1.0, 1.0, 1.0]),
        Facing::East => CollisionBox::of([0.0, 0.0, 0.0], [0.125, 1.0, 1.0]),
    }
}

/// The horizontal facing a metadata nibble names.
///
/// `EnumFacing.getHorizontal(meta & 3)` (`util/EnumFacing.java:273-276`,
/// over the `Plane.HORIZONTAL` order): south at 0, west at 1, north at 2
/// and east at 3 — the order the behaviour table's pumpkin facings pin.
pub const fn horizontal_facing(meta: u8) -> Facing {
    match meta & 3 {
        0 => Facing::South,
        1 => Facing::West,
        2 => Facing::North,
        _ => Facing::East,
    }
}

/// An enchanting table's box: the full footprint, its top at 3/4.
///
/// `BlockEnchantmentTable`'s constructor (`BlockEnchantmentTable.java:24`):
/// `setBlockBounds(0.0F, 0.0F, 0.0F, 1.0F, 0.75F, 1.0F)`, with no
/// `setBlockBoundsBasedOnState` override, so the ray traces this box.
pub const fn enchanting_table_box() -> CollisionBox {
    CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.75, 1.0])
}

/// A brewing stand's box: the centre column, 1/8 across, 7/8 high.
///
/// `BlockBrewingStand.setBlockBoundsBasedOnState`
/// (`BlockBrewingStand.java:78-84`): `setBlockBounds(0.4375F, 0.0F,
/// 0.4375F, 0.5625F, 0.875F, 0.5625F)` — the box the ray traces, not the
/// item-render plate beside it (`:90-93`).
pub const fn brewing_stand_box() -> CollisionBox {
    CollisionBox::of([0.4375, 0.0, 0.4375], [0.5625, 0.875, 0.5625])
}

/// An anvil's box: the 3/4-wide plate, full height, on the facing's axis.
///
/// `BlockAnvil.setBlockBoundsBasedOnState` (`BlockAnvil.java:83-95`): a
/// facing on the x axis stands at `x` full and `z` 0.125..0.875, a facing on
/// the z axis at `x` 0.125..0.875 and `z` full.
pub const fn anvil_box(facing: Facing) -> CollisionBox {
    match facing {
        Facing::East | Facing::West => CollisionBox::of([0.0, 0.0, 0.125], [1.0, 1.0, 0.875]),
        Facing::North | Facing::South => CollisionBox::of([0.125, 0.0, 0.0], [0.875, 1.0, 1.0]),
    }
}

/// A door's box: a 3/16 plate, hinged open or closed.
///
/// `BlockDoor.setBoundBasedOnMeta` (`BlockDoor.java:83-154`): the closed
/// plate stands against the facing's side; an open one swings to the
/// perpendicular axis, to the hinge's side. The face is `float f = 0.1875F`
/// (`:85`).
pub const fn door_box(facing: Facing, open: bool, hinge_left: bool) -> CollisionBox {
    let f = 0.1875;
    if open {
        match facing {
            Facing::North => {
                if hinge_left {
                    CollisionBox::of([1.0 - f, 0.0, 0.0], [1.0, 1.0, 1.0])
                } else {
                    CollisionBox::of([0.0, 0.0, 0.0], [f, 1.0, 1.0])
                }
            }
            Facing::South => {
                if hinge_left {
                    CollisionBox::of([0.0, 0.0, 0.0], [f, 1.0, 1.0])
                } else {
                    CollisionBox::of([1.0 - f, 0.0, 0.0], [1.0, 1.0, 1.0])
                }
            }
            Facing::West => {
                if hinge_left {
                    CollisionBox::of([0.0, 0.0, 0.0], [1.0, 1.0, f])
                } else {
                    CollisionBox::of([0.0, 0.0, 1.0 - f], [1.0, 1.0, 1.0])
                }
            }
            Facing::East => {
                if hinge_left {
                    CollisionBox::of([0.0, 0.0, 1.0 - f], [1.0, 1.0, 1.0])
                } else {
                    CollisionBox::of([0.0, 0.0, 0.0], [1.0, 1.0, f])
                }
            }
        }
    } else {
        match facing {
            Facing::North => CollisionBox::of([0.0, 0.0, 1.0 - f], [1.0, 1.0, 1.0]),
            Facing::South => CollisionBox::of([0.0, 0.0, 0.0], [1.0, 1.0, f]),
            Facing::West => CollisionBox::of([1.0 - f, 0.0, 0.0], [1.0, 1.0, 1.0]),
            Facing::East => CollisionBox::of([0.0, 0.0, 0.0], [f, 1.0, 1.0]),
        }
    }
}

#[cfg(test)]
mod tests {
    //! The pinned shapes the behaviour table and the movement model consume.

    use super::{
        CollisionBox, Connections, Facing, SlabHalf::Bottom, SlabHalf::Top, StairState, chest_box,
        door_box, fence_boxes, front_facing, ladder_box, pane_boxes, slab_boxes, snow_layer_box,
        soul_sand_box, stair_half, stairs_boxes, wall_box,
    };

    #[test]
    fn the_full_box_is_the_sources_unit_cube() {
        let full = CollisionBox::full();
        assert_eq!(full.min, [0.0, 0.0, 0.0]);
        assert_eq!(full.max, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn of_keeps_the_corners_it_is_given() {
        let slab = CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.5, 1.0]);
        assert_eq!(slab.min, [0.0, 0.0, 0.0]);
        assert_eq!(slab.max, [1.0, 0.5, 1.0]);
    }

    #[test]
    fn offset_moves_both_corners() {
        let at_origin = CollisionBox::full().offset(2.0, -1.0, 4.0);
        assert_eq!(at_origin.min, [2.0, -1.0, 4.0]);
        assert_eq!(at_origin.max, [3.0, 0.0, 5.0]);
    }

    #[test]
    fn a_slab_is_the_half_its_state_names() {
        assert_eq!(
            slab_boxes(false, Bottom)[0],
            CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.5, 1.0])
        );
        assert_eq!(
            slab_boxes(false, Top)[0],
            CollisionBox::of([0.0, 0.5, 0.0], [1.0, 1.0, 1.0])
        );
        assert_eq!(slab_boxes(true, Bottom)[0], CollisionBox::full());
    }

    #[test]
    fn a_lone_stair_is_its_half_and_its_step() {
        let bottom = stairs_boxes(Facing::East, Bottom, |_| None);
        assert_eq!(
            bottom,
            vec![
                CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.5, 1.0]),
                CollisionBox::of([0.5, 0.5, 0.0], [1.0, 1.0, 1.0]),
            ]
        );
        let top = stairs_boxes(Facing::East, Top, |_| None);
        assert_eq!(
            top,
            vec![
                CollisionBox::of([0.0, 0.5, 0.0], [1.0, 1.0, 1.0]),
                CollisionBox::of([0.5, 0.0, 0.0], [1.0, 0.5, 1.0]),
            ]
        );
    }

    #[test]
    fn a_stair_turning_half_a_run_gains_the_corner_box() {
        // The queried stair faces east, bottom half; its west neighbour faces
        // north, and the cell north of the queried stair is not that stair —
        // the outer-turn case `func_176304_i` fills (`BlockStairs.java:429-450`).
        let west = super::StairState {
            facing: Facing::North,
            half: Bottom,
        };
        let boxes = stairs_boxes(Facing::East, Bottom, |dir| {
            if dir == Facing::West {
                Some(west)
            } else {
                None
            }
        });
        assert_eq!(
            boxes,
            vec![
                CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.5, 1.0]),
                CollisionBox::of([0.5, 0.5, 0.0], [1.0, 1.0, 1.0]),
                CollisionBox::of([0.0, 0.5, 0.0], [0.5, 1.0, 0.5]),
            ]
        );
    }

    /// A neighbour lookup answering `state` at `dir`, `None` everywhere else:
    /// the stair-state probe's shape without a world.
    fn turned(dir: Facing, state: StairState) -> impl Fn(Facing) -> Option<StairState> {
        move |query| if query == dir { Some(state) } else { None }
    }

    /// The bottom stair's base box, common to the turning-run cases.
    const BASE: CollisionBox = CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.5, 1.0]);

    #[test]
    fn the_stair_half_decoder_reads_the_stair_bit() {
        // Stair metadata bit 2 (`BlockStairs.java:728`, written back at
        // `:736-742`) — not the slab's bit 3.
        assert_eq!(stair_half(0), Bottom);
        assert_eq!(stair_half(3), Bottom);
        assert_eq!(stair_half(4), Top);
        assert_eq!(stair_half(7), Top);
    }

    #[test]
    fn the_front_facing_decoder_folds_the_vertical_faces_to_north() {
        // `EnumFacing.getFront` wraps the index modulo six
        // (`BlockLadder.java:140-150`): north at 2, south at 3 and 9, west at
        // 4 and 10, east at 5.
        assert_eq!(front_facing(2), Facing::North);
        assert_eq!(front_facing(3), Facing::South);
        assert_eq!(front_facing(9), Facing::South);
        assert_eq!(front_facing(4), Facing::West);
        assert_eq!(front_facing(10), Facing::West);
        assert_eq!(front_facing(5), Facing::East);
    }

    #[test]
    fn the_step_box_shrinks_against_a_turning_run_on_every_facing() {
        // `func_176306_h` (`BlockStairs.java:292-406`): where the stair ahead
        // turns and the continuation behind it is not the same stair, the step
        // box is cut back and the returned flag keeps the corner box closed.
        // Bottom half throughout — the half only moves the y span.
        let cases = [
            (
                Facing::East,
                Facing::North,
                CollisionBox::of([0.5, 0.5, 0.0], [1.0, 1.0, 0.5]),
            ),
            (
                Facing::East,
                Facing::South,
                CollisionBox::of([0.5, 0.5, 0.5], [1.0, 1.0, 1.0]),
            ),
            (
                Facing::West,
                Facing::North,
                CollisionBox::of([0.0, 0.5, 0.0], [0.5, 1.0, 0.5]),
            ),
            (
                Facing::West,
                Facing::South,
                CollisionBox::of([0.0, 0.5, 0.5], [0.5, 1.0, 1.0]),
            ),
            (
                Facing::South,
                Facing::West,
                CollisionBox::of([0.0, 0.5, 0.5], [0.5, 1.0, 1.0]),
            ),
            (
                Facing::South,
                Facing::East,
                CollisionBox::of([0.5, 0.5, 0.5], [1.0, 1.0, 1.0]),
            ),
            (
                Facing::North,
                Facing::West,
                CollisionBox::of([0.0, 0.5, 0.0], [0.5, 1.0, 0.5]),
            ),
            (
                Facing::North,
                Facing::East,
                CollisionBox::of([0.5, 0.5, 0.0], [1.0, 1.0, 0.5]),
            ),
        ];
        for (facing, turn, step) in cases {
            let state = StairState {
                facing: turn,
                half: Bottom,
            };
            assert_eq!(
                stairs_boxes(facing, Bottom, turned(facing, state)),
                vec![BASE, step],
                "a {facing:?} stair meeting a {turn:?} turn"
            );
        }

        // The gates: a neighbour continuing the run, one of another half and a
        // run whose far side is the queried stair itself all keep the step
        // full.
        let full_step = CollisionBox::of([0.5, 0.5, 0.0], [1.0, 1.0, 1.0]);
        let continuing = StairState {
            facing: Facing::East,
            half: Bottom,
        };
        assert_eq!(
            stairs_boxes(Facing::East, Bottom, turned(Facing::East, continuing)),
            vec![BASE, full_step],
            "a straight run"
        );
        let other_half = StairState {
            facing: Facing::North,
            half: Top,
        };
        assert_eq!(
            stairs_boxes(Facing::East, Bottom, turned(Facing::East, other_half)),
            vec![BASE, full_step],
            "a neighbour of the other half"
        );
        let state = continuing;
        assert_eq!(
            stairs_boxes(Facing::East, Bottom, |dir| match dir {
                Facing::East => Some(StairState {
                    facing: Facing::North,
                    half: Bottom,
                }),
                Facing::South => Some(state),
                _ => None,
            }),
            vec![BASE, full_step],
            "the queried stair continues past the turn"
        );
    }

    #[test]
    fn a_south_stair_turning_through_its_north_neighbour_gains_the_corner_box() {
        // `func_176304_i`'s south arm sets both z bounds of the corner box
        // and adjusts only x per the neighbour's facing
        // (`BlockStairs.java:482-497`).
        let west = StairState {
            facing: Facing::West,
            half: Bottom,
        };
        assert_eq!(
            stairs_boxes(Facing::South, Bottom, turned(Facing::North, west)),
            vec![
                BASE,
                CollisionBox::of([0.0, 0.5, 0.5], [1.0, 1.0, 1.0]),
                CollisionBox::of([0.0, 0.5, 0.0], [0.5, 1.0, 0.5]),
            ]
        );
        let east = StairState {
            facing: Facing::East,
            half: Bottom,
        };
        assert_eq!(
            stairs_boxes(Facing::South, Bottom, turned(Facing::North, east)),
            vec![
                BASE,
                CollisionBox::of([0.0, 0.5, 0.5], [1.0, 1.0, 1.0]),
                CollisionBox::of([0.5, 0.5, 0.0], [1.0, 1.0, 0.5]),
            ]
        );
    }

    #[test]
    fn a_west_or_north_stair_gains_its_turning_corner_box() {
        // The corner function's west arm (`BlockStairs.java:450-476`) and
        // north arm (`:500-528`), both sub-cases each.
        let north = StairState {
            facing: Facing::North,
            half: Bottom,
        };
        let south = StairState {
            facing: Facing::South,
            half: Bottom,
        };
        assert_eq!(
            stairs_boxes(Facing::West, Bottom, turned(Facing::East, north)),
            vec![
                BASE,
                CollisionBox::of([0.0, 0.5, 0.0], [0.5, 1.0, 1.0]),
                CollisionBox::of([0.5, 0.5, 0.0], [1.0, 1.0, 0.5]),
            ]
        );
        assert_eq!(
            stairs_boxes(Facing::West, Bottom, turned(Facing::East, south)),
            vec![
                BASE,
                CollisionBox::of([0.0, 0.5, 0.0], [0.5, 1.0, 1.0]),
                CollisionBox::of([0.5, 0.5, 0.5], [1.0, 1.0, 1.0]),
            ]
        );
        let west = StairState {
            facing: Facing::West,
            half: Bottom,
        };
        let east = StairState {
            facing: Facing::East,
            half: Bottom,
        };
        assert_eq!(
            stairs_boxes(Facing::North, Bottom, turned(Facing::South, west)),
            vec![
                BASE,
                CollisionBox::of([0.0, 0.5, 0.0], [1.0, 1.0, 0.5]),
                CollisionBox::of([0.0, 0.5, 0.5], [0.5, 1.0, 1.0]),
            ]
        );
        assert_eq!(
            stairs_boxes(Facing::North, Bottom, turned(Facing::South, east)),
            vec![
                BASE,
                CollisionBox::of([0.0, 0.5, 0.0], [1.0, 1.0, 0.5]),
                CollisionBox::of([0.5, 0.5, 0.5], [1.0, 1.0, 1.0]),
            ]
        );
    }

    #[test]
    fn a_fence_post_grows_arms_toward_its_connections() {
        // A lone fence: the west–east axis' post alone (`BlockFence.java:90-94`).
        assert_eq!(
            fence_boxes(Connections::default()),
            vec![CollisionBox::of([0.375, 0.0, 0.375], [0.625, 1.5, 0.625])]
        );
        // North and south arms, and a west–east arm reaching east: the
        // junction's two boxes (`:60-71`, `:84-95`).
        let junction = Connections {
            north: true,
            south: true,
            east: true,
            west: false,
        };
        assert_eq!(
            fence_boxes(junction),
            vec![
                CollisionBox::of([0.375, 0.0, 0.0], [0.625, 1.5, 1.0]),
                CollisionBox::of([0.375, 0.0, 0.375], [1.0, 1.5, 0.625]),
            ]
        );
        // One connection: the arm reaches its side and the post's other end
        // falls back to the 0.375..0.625 span (`BlockFence.java:56-94`).
        for (connections, box_) in [
            (
                Connections {
                    north: true,
                    ..Connections::default()
                },
                CollisionBox::of([0.375, 0.0, 0.0], [0.625, 1.5, 0.625]),
            ),
            (
                Connections {
                    south: true,
                    ..Connections::default()
                },
                CollisionBox::of([0.375, 0.0, 0.375], [0.625, 1.5, 1.0]),
            ),
            (
                Connections {
                    west: true,
                    ..Connections::default()
                },
                CollisionBox::of([0.0, 0.0, 0.375], [0.625, 1.5, 0.625]),
            ),
            (
                Connections {
                    east: true,
                    ..Connections::default()
                },
                CollisionBox::of([0.375, 0.0, 0.375], [1.0, 1.5, 0.625]),
            ),
        ] {
            assert_eq!(fence_boxes(connections), vec![box_], "{connections:?}");
        }
    }

    #[test]
    fn a_pane_is_a_cross_alone_and_a_plate_toward_a_connection() {
        assert_eq!(
            pane_boxes(Connections::default()),
            vec![
                CollisionBox::of([0.0, 0.0, 0.4375], [1.0, 1.0, 0.5625]),
                CollisionBox::of([0.4375, 0.0, 0.0], [0.5625, 1.0, 1.0]),
            ]
        );
        // One connection on an axis emits that axis' reaching plate alone; the
        // branch pushes nothing when neither side of its axis connects, even
        // held open by the other axis (`BlockPane.java:82-99`, `:101-118`).
        let west = Connections {
            west: true,
            ..Connections::default()
        };
        assert_eq!(
            pane_boxes(west),
            vec![CollisionBox::of([0.0, 0.0, 0.4375], [0.5, 1.0, 0.5625])]
        );
        let north_south = Connections {
            north: true,
            south: true,
            ..Connections::default()
        };
        assert_eq!(
            pane_boxes(north_south),
            vec![CollisionBox::of([0.4375, 0.0, 0.0], [0.5625, 1.0, 1.0])]
        );
        // A single north or south connection emits that side's half plate
        // alone (`:101-118`).
        let north = Connections {
            north: true,
            ..Connections::default()
        };
        assert_eq!(
            pane_boxes(north),
            vec![CollisionBox::of([0.4375, 0.0, 0.0], [0.5625, 1.0, 0.5])]
        );
        let south = Connections {
            south: true,
            ..Connections::default()
        };
        assert_eq!(
            pane_boxes(south),
            vec![CollisionBox::of([0.4375, 0.0, 0.5], [0.5625, 1.0, 1.0])]
        );
    }

    #[test]
    fn a_chest_merges_toward_each_of_its_four_sides() {
        // `BlockChest.setBlockBoundsBasedOnState` (`BlockChest.java:66-88`):
        // the 14/16 box reaches the cell's edge on the side its twin sits at.
        assert_eq!(
            chest_box(false, false, false, false),
            CollisionBox::of([0.0625, 0.0, 0.0625], [0.9375, 0.875, 0.9375])
        );
        assert_eq!(
            chest_box(true, false, false, false),
            CollisionBox::of([0.0625, 0.0, 0.0], [0.9375, 0.875, 0.9375])
        );
        assert_eq!(
            chest_box(false, true, false, false),
            CollisionBox::of([0.0625, 0.0, 0.0625], [0.9375, 0.875, 1.0])
        );
        assert_eq!(
            chest_box(false, false, true, false),
            CollisionBox::of([0.0, 0.0, 0.0625], [0.9375, 0.875, 0.9375])
        );
        assert_eq!(
            chest_box(false, false, false, true),
            CollisionBox::of([0.0625, 0.0, 0.0625], [1.0, 0.875, 0.9375])
        );
    }

    #[test]
    fn a_wall_narrows_across_a_single_axis_run() {
        // `BlockWall.setBlockBoundsBasedOnState` (`BlockWall.java:67-113`)
        // builds the 0.25..0.75 post, the arms and the 0.3125..0.6875
        // narrowing when only one axis connects.
        assert_eq!(
            wall_box(Connections::default()),
            CollisionBox::of([0.25, 0.0, 0.25], [0.75, 1.5, 0.75])
        );
        assert_eq!(
            wall_box(Connections {
                north: true,
                south: true,
                ..Connections::default()
            }),
            CollisionBox::of([0.3125, 0.0, 0.0], [0.6875, 1.5, 1.0])
        );
        assert_eq!(
            wall_box(Connections {
                west: true,
                east: true,
                ..Connections::default()
            }),
            CollisionBox::of([0.0, 0.0, 0.3125], [1.0, 1.5, 0.6875])
        );
        // A single connected side reaches the cell's edge without narrowing.
        assert_eq!(
            wall_box(Connections {
                north: true,
                ..Connections::default()
            }),
            CollisionBox::of([0.25, 0.0, 0.0], [0.75, 1.5, 0.75])
        );
    }

    #[test]
    fn a_ladder_is_a_plate_on_its_attached_face() {
        assert_eq!(
            ladder_box(Facing::North),
            CollisionBox::of([0.0, 0.0, 0.875], [1.0, 1.0, 1.0])
        );
        assert_eq!(
            ladder_box(Facing::South),
            CollisionBox::of([0.0, 0.0, 0.0], [1.0, 1.0, 0.125])
        );
        assert_eq!(
            ladder_box(Facing::West),
            CollisionBox::of([0.875, 0.0, 0.0], [1.0, 1.0, 1.0])
        );
        assert_eq!(
            ladder_box(Facing::East),
            CollisionBox::of([0.0, 0.0, 0.0], [0.125, 1.0, 1.0])
        );
    }

    #[test]
    fn a_closed_door_is_a_plate_on_the_facings_side() {
        assert_eq!(
            door_box(Facing::North, false, false),
            CollisionBox::of([0.0, 0.0, 0.8125], [1.0, 1.0, 1.0])
        );
        assert_eq!(
            door_box(Facing::East, false, false),
            CollisionBox::of([0.0, 0.0, 0.0], [0.1875, 1.0, 1.0])
        );
        // An open door swings to the hinge's side (`BlockDoor.java:93-136`).
        assert_eq!(
            door_box(Facing::East, true, false),
            CollisionBox::of([0.0, 0.0, 0.0], [1.0, 1.0, 0.1875])
        );
        assert_eq!(
            door_box(Facing::East, true, true),
            CollisionBox::of([0.0, 0.0, 0.8125], [1.0, 1.0, 1.0])
        );
        // Closed south and west (`BlockDoor.java:138-153`).
        assert_eq!(
            door_box(Facing::South, false, false),
            CollisionBox::of([0.0, 0.0, 0.0], [1.0, 1.0, 0.1875])
        );
        assert_eq!(
            door_box(Facing::West, false, false),
            CollisionBox::of([0.8125, 0.0, 0.0], [1.0, 1.0, 1.0])
        );
        // Open south and west swing to the hinge's side (`BlockDoor.java:91-136`).
        assert_eq!(
            door_box(Facing::South, true, false),
            CollisionBox::of([0.8125, 0.0, 0.0], [1.0, 1.0, 1.0])
        );
        assert_eq!(
            door_box(Facing::South, true, true),
            CollisionBox::of([0.0, 0.0, 0.0], [0.1875, 1.0, 1.0])
        );
        assert_eq!(
            door_box(Facing::West, true, false),
            CollisionBox::of([0.0, 0.0, 0.8125], [1.0, 1.0, 1.0])
        );
        assert_eq!(
            door_box(Facing::West, true, true),
            CollisionBox::of([0.0, 0.0, 0.0], [1.0, 1.0, 0.1875])
        );
    }

    #[test]
    fn soul_sand_stands_an_eighth_below_its_cell() {
        assert_eq!(
            soul_sand_box(),
            CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.875, 1.0])
        );
    }

    #[test]
    fn a_snow_layers_height_is_its_count_short_of_the_top() {
        assert_eq!(
            snow_layer_box(1),
            CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.0, 1.0])
        );
        assert_eq!(
            snow_layer_box(4),
            CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.375, 1.0])
        );
        assert_eq!(
            snow_layer_box(8),
            CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.875, 1.0])
        );
        // The count clamps to the property's 1..=8 range: below it the base
        // layer, above it the full top.
        assert_eq!(
            snow_layer_box(0),
            CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.0, 1.0])
        );
        assert_eq!(
            snow_layer_box(9),
            CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.875, 1.0])
        );
    }
}
