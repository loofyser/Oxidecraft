//! The block raycast, the aimed-block state and the dig machine.
//!
//! The interaction ray is the reference client's mouse-over: the frame path
//! takes the block reach from the controller, runs the look vector from the
//! pose eye and hands the segment to the world's own trace
//! (`EntityRenderer.getMouseOver`, `client/renderer/EntityRenderer.java:409-420`;
//! `Entity.rayTrace`, `entity/Entity.java:1500-1506`:
//! `this.worldObj.rayTraceBlocks(vec3, vec32, false, false, true)`).
//! [`raycast`] walks that trace: the segment's cells in order, testing each
//! cell whose block passes the source's own check, and answers the block the
//! ray stops at, the face it entered through and the point on that face
//! ([`Aim`]).
//!
//! The predicate over blocks is the source's own: an entered cell is tested
//! when its block passes `canCollideCheck(state, stopOnLiquid)`
//! (`world/World.java:904`, `:1037`), and with the frame path's flags the
//! collision-box clause is skipped and `stopOnLiquid` is false, so the test is
//! `canCollideCheck(state, false)` — `Block.isCollidable()`
//! (`block/Block.java:512-515`, `:520-523`), true for every covered block but
//! the four liquids (`block/BlockLiquid.java:82-85`). Cells outside the
//! covered set — air and fire included, non-collidable at their classes
//! (`BlockAir.java:37-40`, `BlockFire.java:353-356`) — carry no row and never
//! stop the ray.
//!
//! A tested cell is traced against the bounds its block reports to
//! `Block.collisionRayTrace` (`block/Block.java:681-794`): the collision boxes
//! the behaviour table carries, and — for the non-cube blocks that answer no
//! collision box — the selection bounds their own classes set, the cross
//! plants, the torch, the pressure plate, the reeds, the crops and the double
//! plant (`block/BlockBush.java:30-31`, `block/BlockTallGrass.java:32-33`,
//! `block/BlockDeadBush.java:22-23`, `block/BlockMushroom.java:15-16`,
//! `block/BlockCrops.java:25-26`, `block/BlockReed.java:26-28`,
//! `block/BlockTorch.java:185-208`, `block/BlockBasePressurePlate.java:34-46`,
//! `block/BlockDoublePlant.java:41-43`).
//!
//! The reach is the controller's own: `5.0F` in creative, `4.5F` otherwise
//! (`client/multiplayer/PlayerControllerMP.java:344-346`), read from the
//! gamemode byte Join Game carried (`S01PacketJoinGame.java:44-47`).
//!
//! The origin is the pose eye — the feet plus `EntityPlayer.getEyeHeight`'s
//! `1.62F` (`entity/player/EntityPlayer.java:2326-2330`) — with no displacement. The
//! render camera's tenth-of-a-block offset along the view axis
//! (`EntityRenderer.orientCamera`'s first-person branch, `EntityRenderer.java:720`)
//! never reaches the ray.
//!
//! The digging machine ([`DigState`]) is the reference controller's own: the
//! start on a left press with an aim (`PlayerControllerMP.clickBlock`,
//! `:198-266`), the progress accumulated while held and aimed at the same
//! block (`onPlayerDamageBlock`, `:285-339`), the cancel on an aim change or a
//! release (`:240`, `:274-283`), the finish at completion (`:324-325`) and the
//! creative instant branch (`:230-235`, `:294-300`). The hand's rate is
//! [`hand_rate`] — the source's own split — and the destroy stages the
//! progress names are [`BreakStages`]'s, keyed by position until M4's entity
//! work.

use std::collections::HashMap;

use oxide_world::behaviour::{CollisionShape, Material, behaviour};
use oxide_world::chunk::{SECTION_COUNT, SECTION_SIZE};
use oxide_world::collision::CollisionBox;

use crate::physics::CollisionView;
use crate::world_view::WorldView;

/// The hardcore bit of Join Game's gamemode byte
/// (`S01PacketJoinGame.java:44-47`).
const HARDCORE_BIT: u8 = 0x08;

/// `WorldSettings.GameType.CREATIVE`'s id (`world/WorldSettings.java:139`).
const CREATIVE_ID: u8 = 1;

/// The block reach in creative: the source's `5.0F`
/// (`client/multiplayer/PlayerControllerMP.java:344-346`).
pub const CREATIVE_REACH: f64 = 5.0;

/// The block reach outside creative — survival, adventure and spectator all
/// take it: the source's `4.5F` (`PlayerControllerMP.java:344-346`).
pub const SURVIVAL_REACH: f64 = 4.5;

/// The reach the gamemode byte asks for.
///
/// `PlayerControllerMP.getBlockReachDistance` returns
/// `this.currentGameType.isCreative() ? 5.0F : 4.5F` (`:344-346`). The byte is
/// Join Game's, which packs the gamemode in its low three bits and hardcore in
/// the fourth (`S01PacketJoinGame.java:44-47`), so the hardcore bit is masked
/// before the comparison.
pub fn reach(gamemode: u8) -> f64 {
    if creative(gamemode) {
        CREATIVE_REACH
    } else {
        SURVIVAL_REACH
    }
}

/// Whether the gamemode byte names creative, its hardcore bit masked
/// (`S01PacketJoinGame.java:44-47`).
///
/// The byte is the one `getBlockReachDistance`'s comparison and the dig
/// machine's instant branch both read (`PlayerControllerMP.java:230`, `:294`).
pub fn creative(gamemode: u8) -> bool {
    gamemode & !HARDCORE_BIT == CREATIVE_ID
}

/// The face of a block the interaction ray met.
///
/// The variants and their order are the source's `EnumFacing`
/// (`util/EnumFacing.java:12-17`: DOWN, UP, NORTH, SOUTH, WEST, EAST), and the
/// wire byte is `getIndex()`'s ordinal (`:53-58`) — the value the digging and
/// placement packets carry (`C07PacketPlayerDigging.java:46`;
/// `C08PacketPlayerBlockPlacement.java:58`, from
/// `PlayerControllerMP.java:424`'s `side.getIndex()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// The block's `-Y` face.
    Down,
    /// The block's `+Y` face.
    Up,
    /// The block's `-Z` face.
    North,
    /// The block's `+Z` face.
    South,
    /// The block's `-X` face.
    West,
    /// The block's `+X` face.
    East,
}

impl Face {
    /// The face as the packets carry it: `EnumFacing.getIndex()`'s ordinal
    /// (`util/EnumFacing.java:53-58`).
    pub const fn wire(self) -> u8 {
        match self {
            Face::Down => 0,
            Face::Up => 1,
            Face::North => 2,
            Face::South => 3,
            Face::West => 4,
            Face::East => 5,
        }
    }

    /// The cell offset along this face's normal: DOWN `(0, -1, 0)`, UP
    /// `(0, 1, 0)`, NORTH `(0, 0, -1)`, SOUTH `(0, 0, 1)`, WEST `(-1, 0, 0)`,
    /// EAST `(1, 0, 0)`.
    ///
    /// `EnumFacing`'s front offsets (`util/EnumFacing.java:222-240`), the
    /// vector `BlockPos.offset(facing)` adds (`util/BlockPos.java:176-181`)
    /// — the step `ItemBlock.onItemUse` spends on a block that is not
    /// replaceable (`item/ItemBlock.java:43-45`).
    pub const fn offset(self) -> (i32, i32, i32) {
        match self {
            Face::Down => (0, -1, 0),
            Face::Up => (0, 1, 0),
            Face::North => (0, 0, -1),
            Face::South => (0, 0, 1),
            Face::West => (-1, 0, 0),
            Face::East => (1, 0, 0),
        }
    }
}

/// The block the interaction ray met.
///
/// The shape the source's `MovingObjectPosition` carries for a block hit
/// (`util/MovingObjectPosition.java:19-22`): the block's coordinates, the face
/// the ray entered through and the point on it, in world coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aim {
    /// The block's x.
    pub x: i32,
    /// The block's y.
    pub y: i32,
    /// The block's z.
    pub z: i32,
    /// The face of the block the ray entered through.
    pub face: Face,
    /// The point the ray met the block at, on [`Aim::face`]'s plane.
    pub hit: [f64; 3],
}

/// The look direction for a yaw and a pitch, in degrees.
///
/// `Entity.getVectorForRotation` (`entity/Entity.java:1476-1483`), which
/// `getLook` hands the current pitch and yaw at a full partial tick
/// (`:1459-1463`):
///
/// ```text
/// f  = cos(-yaw   * (pi / 180) - pi)
/// f1 = sin(-yaw   * (pi / 180) - pi)
/// f2 = -cos(-pitch * (pi / 180))
/// f3 = sin(-pitch * (pi / 180))
///     -> (f1 * f2, f3, f * f2)
/// ```
///
/// so yaw 0 with pitch 0 looks south along `+z`, yaw 90 west along `-x`, and a
/// positive pitch straight down. The source's `MathHelper.cos`/`sin` are table
/// lookups quantised to a 65536th of a turn; the direct functions here differ
/// below a ten-thousandth of a degree, far under the ray's resolution.
pub fn look_vector(yaw: f32, pitch: f32) -> [f64; 3] {
    const DEGREE: f64 = std::f64::consts::PI / 180.0;
    let yaw = f64::from(yaw) * DEGREE;
    let pitch = f64::from(pitch) * DEGREE;
    let f = (-yaw - std::f64::consts::PI).cos();
    let f1 = (-yaw - std::f64::consts::PI).sin();
    let f2 = -(-pitch).cos();
    let f3 = (-pitch).sin();
    [f1 * f2, f3, f * f2]
}

/// The block the ray from `eye` along `dir` meets within `reach`, or `None`.
///
/// The ray runs from `eye` — the pose eye — along `dir`, a unit look vector,
/// for `reach` blocks. The walk is `World.rayTraceBlocks`' own
/// (`world/World.java:888-1066`): the cell the eye stands in is tested first,
/// then the ray steps from cell boundary to boundary — x first, then y, then z
/// when two boundaries are crossed at the same distance — testing each cell it
/// enters, up to the source's two-hundred step cap. A cell is tested when its
/// block passes the source's own check ([`ray_bounds`], `world/World.java:904`'s
/// predicate) and stops the ray when one of the bounds that block reports meets
/// the segment; the answer is the nearest entry, with the face the ray crosses
/// there and the point on it (`Block.collisionRayTrace`'s own faces,
/// `block/Block.java:763-791`). A non-finite eye or end answers `None`, where
/// the source refuses a NaN one (`:890-893`).
pub fn raycast(view: &WorldView<'_>, eye: [f64; 3], dir: [f64; 3], reach: f64) -> Option<Aim> {
    let end = [
        eye[0] + dir[0] * reach,
        eye[1] + dir[1] * reach,
        eye[2] + dir[2] * reach,
    ];
    if !eye.iter().chain(end.iter()).all(|value| value.is_finite()) {
        return None;
    }
    let [mut x, mut y, mut z] = cell_of(eye);
    let [end_x, end_y, end_z] = cell_of(end);
    let mut boxes = Vec::new();
    if let Some(aim) = cell_aim(view, &mut boxes, eye, end, [x, y, z]) {
        return Some(aim);
    }
    // The walk proper, under the source's own step cap (`World.java:915-917`).
    let mut remaining: i32 = 200;
    let mut at = eye;
    while remaining >= 0 {
        remaining -= 1;
        if [x, y, z] == [end_x, end_y, end_z] {
            return None;
        }
        // The next boundary on each axis, and whether the axis moves at all
        // (`:930-989`).
        let (mut bound_x, mut bound_y, mut bound_z) = (999.0, 999.0, 999.0);
        let (mut moves_x, mut moves_y, mut moves_z) = (false, false, false);
        if end_x > x {
            bound_x = f64::from(x) + 1.0;
            moves_x = true;
        } else if end_x < x {
            bound_x = f64::from(x);
            moves_x = true;
        }
        if end_y > y {
            bound_y = f64::from(y) + 1.0;
            moves_y = true;
        } else if end_y < y {
            bound_y = f64::from(y);
            moves_y = true;
        }
        if end_z > z {
            bound_z = f64::from(z) + 1.0;
            moves_z = true;
        } else if end_z < z {
            bound_z = f64::from(z);
            moves_z = true;
        }
        let span_x = end[0] - at[0];
        let span_y = end[1] - at[1];
        let span_z = end[2] - at[2];
        let (mut cross_x, mut cross_y, mut cross_z) = (999.0, 999.0, 999.0);
        if moves_x {
            cross_x = (bound_x - at[0]) / span_x;
        }
        if moves_y {
            cross_y = (bound_y - at[1]) / span_y;
        }
        if moves_z {
            cross_z = (bound_z - at[2]) / span_z;
        }
        if cross_x == -0.0 {
            cross_x = -1.0e-4;
        }
        if cross_y == -0.0 {
            cross_y = -1.0e-4;
        }
        if cross_z == -0.0 {
            cross_z = -1.0e-4;
        }
        // The first boundary the ray crosses takes it into the next cell, with
        // the source's own tie order (`:1002-1048`).
        if cross_x < cross_y && cross_x < cross_z {
            at = [bound_x, at[1] + span_y * cross_x, at[2] + span_z * cross_x];
            x += if end_x > x { 1 } else { -1 };
        } else if cross_y < cross_z {
            at = [at[0] + span_x * cross_y, bound_y, at[2] + span_z * cross_y];
            y += if end_y > y { 1 } else { -1 };
        } else {
            at = [at[0] + span_x * cross_z, at[1] + span_y * cross_z, bound_z];
            z += if end_z > z { 1 } else { -1 };
        }
        if let Some(aim) = cell_aim(view, &mut boxes, eye, end, [x, y, z]) {
            return Some(aim);
        }
    }
    None
}

/// The cell a point stands in: the floor of each coordinate, as the source's
/// own `MathHelper.floor_double` (`World.java:894-899`).
fn cell_of(point: [f64; 3]) -> [i32; 3] {
    [
        point[0].floor() as i32,
        point[1].floor() as i32,
        point[2].floor() as i32,
    ]
}

/// The aim one cell answers for the segment `from..to`, when one of the bounds
/// its block reports meets it: the nearest entry, on the cell's own block.
fn cell_aim(
    view: &WorldView<'_>,
    boxes: &mut Vec<CollisionBox>,
    from: [f64; 3],
    to: [f64; 3],
    cell: [i32; 3],
) -> Option<Aim> {
    boxes.clear();
    ray_bounds(view, cell, boxes);
    let mut nearest: Option<(f64, Face)> = None;
    for box_ in boxes.iter() {
        let Some((t, face)) = box_entry(box_, from, to) else {
            continue;
        };
        if nearest.is_none_or(|(best, _)| t < best) {
            nearest = Some((t, face));
        }
    }
    let (t, face) = nearest?;
    Some(Aim {
        x: cell[0],
        y: cell[1],
        z: cell[2],
        face,
        hit: [
            from[0] + (to[0] - from[0]) * t,
            from[1] + (to[1] - from[1]) * t,
            from[2] + (to[2] - from[2]) * t,
        ],
    })
}

/// The bounds the reference's per-cell trace reads, pushed into `out`.
///
/// The cell's stop check is the source's own: `World.rayTraceBlocks` tests
/// `canCollideCheck(state, stopOnLiquid)` on every entered cell
/// (`world/World.java:904`, `:1037`), and the frame path's flags leave the
/// collision-box clause out and pass `stopOnLiquid = false`
/// (`entity/Entity.java:1505`), so the check is `canCollideCheck(state, false)`
/// — `Block.isCollidable()` (`block/Block.java:512-515`, `:520-523`), false
/// only for the liquids here (`block/BlockLiquid.java:82-85`). A cell outside
/// the covered set (air, fire and the rest) or a liquid pushes nothing and
/// cannot stop the ray. A stopping cell's trace is `Block.collisionRayTrace`
/// (`:681-794`): the collision boxes the behaviour table carries, or, for a
/// non-cube block that answers no collision box, the selection bounds its own
/// class sets ([`selection_bounds`]).
fn ray_bounds(view: &WorldView<'_>, cell: [i32; 3], out: &mut Vec<CollisionBox>) {
    let value = view.0.block(cell[0], cell[1], cell[2]);
    let Some(row) = behaviour(value >> 4) else {
        return;
    };
    if row.liquid.is_some() {
        return;
    }
    if matches!(row.collision, CollisionShape::None) {
        selection_bounds(value >> 4, (value & 0xF) as u8, cell, out);
    } else {
        view.collision_boxes(cell[0], cell[1], cell[2], out);
    }
}

/// The selection bounds the source's classes set for the collision-less
/// non-cube blocks, pushed into `out` in world coordinates.
///
/// `Block.collisionRayTrace` traces the bounds `setBlockBoundsBasedOnState`
/// leaves behind (`block/Block.java:683`), and these blocks answer no
/// collision box (`BlockBush.getCollisionBoundingBox`, `block/BlockBush.java:76-79`,
/// bypassed by the frame path's false `ignoreBlockWithoutBoundingBox`), so the
/// trace runs on the bounds their own classes set:
///
/// * the bush default, `[0.3, 0, 0.3]..[0.7, 0.6, 0.7]` (`block/BlockBush.java:30-31`,
///   inherited by the two flowers, which do not override it);
/// * the tall grass and the dead bush, `[0.1, 0, 0.1]..[0.9, 0.8, 0.9]`
///   (`block/BlockTallGrass.java:32-33`, `block/BlockDeadBush.java:22-23`);
/// * the mushrooms, `[0.3, 0, 0.3]..[0.7, 0.4, 0.7]` (`block/BlockMushroom.java:15-16`);
/// * the crops and their carrot and potato subclasses,
///   `[0, 0, 0]..[1, 0.25, 1]` (`block/BlockCrops.java:25-26`, with
///   `BlockCarrot.java:6` and `BlockPotato.java:10` extending it);
/// * the reeds, `[0.125, 0, 0.125]..[0.875, 1, 0.875]` (`block/BlockReed.java:26-28`);
/// * the double plant, the full cube (`block/BlockDoublePlant.java:41-43`);
/// * the torch, per facing (`BlockTorch.collisionRayTrace`, `block/BlockTorch.java:185-208`,
///   over the metadata of `getStateFromMeta`, `:244-269`: 1 east, 2 west,
///   3 south, 4 north, anything else standing);
/// * the pressure plate, `[1/16, 0, 1/16]..[15/16, h, 15/16]` with `h` 1/32
///   powered and 1/16 otherwise (`block/BlockBasePressurePlate.java:34-46`,
///   over `BlockPressurePlate.getStateFromMeta`, `:73-76`: metadata 1 is
///   powered).
fn selection_bounds(id: u16, meta: u8, cell: [i32; 3], out: &mut Vec<CollisionBox>) {
    let (min, max): ([f64; 3], [f64; 3]) = match id {
        31 | 32 => ([0.1, 0.0, 0.1], [0.9, 0.8, 0.9]),
        37 | 38 => ([0.3, 0.0, 0.3], [0.7, 0.6, 0.7]),
        39 | 40 => ([0.3, 0.0, 0.3], [0.7, 0.4, 0.7]),
        59 | 141 | 142 => ([0.0, 0.0, 0.0], [1.0, 0.25, 1.0]),
        83 => ([0.125, 0.0, 0.125], [0.875, 1.0, 0.875]),
        175 => ([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]),
        50 => match meta {
            1 => ([0.0, 0.2, 0.35], [0.3, 0.8, 0.65]),
            2 => ([0.7, 0.2, 0.35], [1.0, 0.8, 0.65]),
            3 => ([0.35, 0.2, 0.0], [0.65, 0.8, 0.3]),
            4 => ([0.35, 0.2, 0.7], [0.65, 0.8, 1.0]),
            _ => ([0.4, 0.0, 0.4], [0.6, 0.6, 0.6]),
        },
        72 => {
            if meta == 1 {
                ([0.0625, 0.0, 0.0625], [0.9375, 0.03125, 0.9375])
            } else {
                ([0.0625, 0.0, 0.0625], [0.9375, 0.0625, 0.9375])
            }
        }
        _ => return,
    };
    out.push(CollisionBox::of(min, max).offset(
        f64::from(cell[0]),
        f64::from(cell[1]),
        f64::from(cell[2]),
    ));
}

/// The segment's entry into a box: the smallest `t` in `0..=1` at which the
/// segment is on the box's surface, with the face it crosses there.
///
/// This is `Block.collisionRayTrace`'s answer (`block/Block.java:681-794`)
/// reduced to its result: the nearest of the box's six planes that the segment
/// meets within the box's own bounds, with the plane's face (`:763-791`). A
/// segment that starts inside the box leaves through the nearest plane ahead,
/// as the source's does; a box the segment only passes beside, or one wholly
/// behind it, answers `None`.
fn box_entry(box_: &CollisionBox, from: [f64; 3], to: [f64; 3]) -> Option<(f64, Face)> {
    let mut entry = f64::NEG_INFINITY;
    let mut exit = f64::INFINITY;
    let mut entry_face = Face::Down;
    let mut exit_face = Face::Down;
    for axis in 0..3 {
        let (low_face, high_face) = match axis {
            0 => (Face::West, Face::East),
            1 => (Face::Down, Face::Up),
            _ => (Face::North, Face::South),
        };
        let span = to[axis] - from[axis];
        if span == 0.0 {
            if from[axis] < box_.min[axis] || from[axis] > box_.max[axis] {
                return None;
            }
            continue;
        }
        let (mut t_low, mut t_high) = (
            (box_.min[axis] - from[axis]) / span,
            (box_.max[axis] - from[axis]) / span,
        );
        let (mut low_face, mut high_face) = (low_face, high_face);
        if t_low > t_high {
            std::mem::swap(&mut t_low, &mut t_high);
            std::mem::swap(&mut low_face, &mut high_face);
        }
        if t_low > entry {
            entry = t_low;
            entry_face = low_face;
        }
        if t_high < exit {
            exit = t_high;
            exit_face = high_face;
        }
        if entry > exit {
            return None;
        }
    }
    if exit < 0.0 || entry > 1.0 {
        return None;
    }
    if entry >= 0.0 {
        return Some((entry, entry_face));
    }
    if exit.is_finite() {
        return Some((exit, exit_face));
    }
    // A box with no extent on any axis the segment moves along: the segment's
    // start lies on it, as the source's first candidate does.
    Some((0.0, entry_face))
}

/// Whether a block value is replaceable by a placement — the source's own
/// overwrite rule, read at both checks a right click makes.
///
/// `ItemBlock.onItemUse` keeps the aimed cell when its block answers
/// `Block.isReplaceable` and steps one cell along the face otherwise
/// (`item/ItemBlock.java:43-45`), and the landing cell must pass
/// `World.canBlockBePlaced`, whose material clause is
/// `blockMaterial.isReplaceable()` (`world/World.java:3153-3157`). The
/// material rule is set at the material singletons: `MaterialTransparent` —
/// air and fire (`Material.java:5,19`; `MaterialTransparent.java:8`) —
/// `MaterialLiquid` — water and lava (`Material.java:9-10`;
/// `MaterialLiquid.java:8`) — and `Material.vine` (`Material.java:16`),
/// which the covered plants carry: the tall grass (`BlockTallGrass.java:30`),
/// the dead bush (`BlockDeadBush.java:21`) and the double plant
/// (`BlockDoublePlant.java:34`). `Material.isReplaceable()` answers the flag
/// (`Material.java:162-165`).
///
/// Over that material set the source's block-level overrides narrow exactly
/// one covered block: `Block.isReplaceable` defaults to false
/// (`block/Block.java:387-390`) and `BlockDoublePlant` overrides it to its
/// grass and fern variants alone (`:69-81`), so the double plant is gated on
/// its meta here.
///
/// The four liquids can never be aimed — the interaction ray passes them —
/// but they are replaceable as landing cells. Values outside the covered set
/// (the snow layer's, the vine block's, fire) carry no table row, and this
/// client does not replace what it cannot vouch for.
pub fn replaceable(value: u16) -> bool {
    match value >> 4 {
        0 | 8 | 9 | 10 | 11 | 31 | 32 => true,
        175 => matches!(value & 0xF, 2 | 3),
        _ => false,
    }
}

/// The placement a right press at an [`Aim`] would make: the packet's own
/// facts and the cell the block lands in.
///
/// The source builds its packet from exactly these — the aimed position and
/// the face (`PlayerControllerMP.onPlayerRightClick` spends the frame's
/// `ObjectMouseOver` at `:395-396` and sends at `:424`), and the three hit
/// fractions (`:395-397`'s `f`, `f1`, `f2`, scaled at
/// `C08PacketPlayerBlockPlacement.writePacketData:60-62`) — and predicts the
/// block at the cell its `onItemUse` call writes (`:436`, `:443`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// The aimed block's x — the packet's Location Position.
    pub x: i32,
    /// The aimed block's y.
    pub y: i32,
    /// The aimed block's z.
    pub z: i32,
    /// The face the ray entered through — the packet's face byte.
    pub face: Face,
    /// The cursor's three bytes, 0 through 16.
    pub cursor: [u8; 3],
    /// The cell the block lands in.
    pub target: [i32; 3],
}

/// The cursor's three bytes for a hit in `aim`'s cell: the source's
/// `(int)(facing * 16.0F)` per axis
/// (`C08PacketPlayerBlockPlacement.writePacketData`, `:60-62`), where each
/// fraction is the hit point minus the cell's own coordinate on that axis
/// (`PlayerControllerMP.onPlayerRightClick`, `:395-397`).
///
/// The cast truncates toward zero and is not clamped: a hit on the cell's
/// far plane carries a fraction of exactly 1 and writes 16.
pub fn placement_cursor(aim: &Aim) -> [u8; 3] {
    let mut cursor = [0u8; 3];
    for (axis, byte) in cursor.iter_mut().enumerate() {
        let cell = match axis {
            0 => aim.x,
            1 => aim.y,
            _ => aim.z,
        };
        let fraction = (aim.hit[axis] - f64::from(cell)) as f32;
        *byte = (fraction * 16.0) as u8;
    }
    cursor
}

/// The placement a right press with `aim` makes, or `None` when the
/// source's own client-side checks refuse the press.
///
/// The target is the aimed cell when its block is [`replaceable`] and the
/// next cell along the face's normal otherwise — `ItemBlock.onItemUse`'s
/// rule, applied before it spends the stack (`item/ItemBlock.java:43-45`).
/// The target must then accept the block: the block there must itself be
/// [`replaceable`], `World.canBlockBePlaced`'s material clause
/// (`world/World.java:3153-3157`; with the null entity `ItemBlock` passes at
/// `:56` its bounding-box clause never refuses), and the target must lie
/// inside the world's build range, 0 through 255 — the range the world's own
/// write path takes (`World::block` answers air outside it, `World::set_block`
/// refuses it).
///
/// The source checks the world border on the aimed position instead
/// (`PlayerControllerMP.java:398-401`: outside `getWorldBorder().contains`
/// it refuses, and the border's default span cannot be met within a reach of
/// five), and leaves collision and support validation to the server; until
/// M5's inventory this client assumes a full cube is held, whose support rule
/// is the material check above.
pub fn placement(view: &WorldView<'_>, aim: &Aim) -> Option<Placement> {
    let aimed = view.0.block(aim.x, aim.y, aim.z);
    let target = if replaceable(aimed) {
        [aim.x, aim.y, aim.z]
    } else {
        let (dx, dy, dz) = aim.face.offset();
        [aim.x + dx, aim.y + dy, aim.z + dz]
    };
    let in_bounds = (0..(SECTION_COUNT * SECTION_SIZE) as i32).contains(&target[1]);
    let accepts = replaceable(view.0.block(target[0], target[1], target[2]));
    if !in_bounds || !accepts {
        return None;
    }
    Some(Placement {
        x: aim.x,
        y: aim.y,
        z: aim.z,
        face: aim.face,
        cursor: placement_cursor(aim),
        target,
    })
}

/// The hand's progress per tick on a block with no tool held: the source's
/// own split, quoted — `Block.getPlayerRelativeBlockHardness`
/// (`block/Block.java:590-594`):
///
/// ```text
/// f < 0.0F ? 0.0F : (!playerIn.canHarvestBlock(this)
///     ? playerIn.getToolDigEfficiency(this) / f / 100.0F
///     : playerIn.getToolDigEfficiency(this) / f / 30.0F)
/// ```
///
/// With no held item `getToolDigEfficiency` is the inventory's own `1.0F`
/// (`EntityPlayer.getToolDigEfficiency`, `:900-902`, over
/// `InventoryPlayer.getStrVsBlock`'s empty-slot branch, `:552-562`) and
/// `canHarvestBlock` is the material's `isToolNotRequired`
/// (`InventoryPlayer.canHeldItemHarvest`, `:684-695`), so the split is
/// `1/hardness/100` per tick when the material requires a tool and
/// `1/hardness/30` when it does not. A negative hardness — an unbreakable
/// block — is zero: no tick ever completes it.
pub fn hand_rate(hardness: f32, tool_not_required: bool) -> f32 {
    if hardness < 0.0 {
        return 0.0;
    }
    if tool_not_required {
        1.0 / hardness / 30.0
    } else {
        1.0 / hardness / 100.0
    }
}

/// Whether a material can be harvested without a tool:
/// `Material.isToolNotRequired` (`block/material/Material.java:174-179`),
/// which answers `requiresNoTool` — true by default (`:69`) and cleared by
/// `setRequiresTool` (`:127`) for rock, iron, anvil, snow, crafted snow, web
/// and barrier (`:9-11`, `:29-32`, `:39-45`, `:49`).
///
/// In this table's vocabulary those are [`Material::Rock`] and
/// [`Material::Stone`] (both `Material.rock`), [`Material::Metal`],
/// [`Material::Snow`], [`Material::CraftedSnow`] and [`Material::Web`]; the
/// anvil and barrier materials are not covered ids yet.
pub fn tool_not_required(material: Material) -> bool {
    !matches!(
        material,
        Material::Rock
            | Material::Stone
            | Material::Metal
            | Material::Snow
            | Material::CraftedSnow
            | Material::Web
    )
}

/// The block facts one dig step reads: where the aim met a block, through
/// which face, and the hand's rate there ([`hand_rate`] over the behaviour
/// table's row).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DigAim {
    /// The block's x.
    pub x: i32,
    /// The block's y.
    pub y: i32,
    /// The block's z.
    pub z: i32,
    /// The face of the block the ray entered through.
    pub face: Face,
    /// The hand's progress per tick at the block.
    pub rate: f32,
}

/// One instruction the dig machine leaves for the session, in the order the
/// source produces them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DigAction {
    /// Send Player Digging with status 0 for this block and face
    /// (`PlayerControllerMP.clickBlock`, `:232`, `:243`).
    Start {
        /// The block's x.
        x: i32,
        /// The block's y.
        y: i32,
        /// The block's z.
        z: i32,
        /// The face the start carries.
        face: Face,
    },
    /// Send Player Digging with status 1: the aim left the running block
    /// (`:240`) or the dig was reset (`:278`).
    Abort {
        /// The block's x.
        x: i32,
        /// The block's y.
        y: i32,
        /// The block's z.
        z: i32,
        /// The face the abort carries.
        face: Face,
    },
    /// Send Player Digging with status 2 — the dig completed (`:324`).
    Finish {
        /// The block's x.
        x: i32,
        /// The block's y.
        y: i32,
        /// The block's z.
        z: i32,
        /// The face the finish carries.
        face: Face,
    },
    /// Send Animation — one swing (`EntityPlayerSP.swingItem`, `:304-308`).
    Swing,
    /// Remove the block locally: the completion's prediction
    /// (`onPlayerDestroyBlock`, `:123-193`, through the session's
    /// block-change pipeline).
    Destroy {
        /// The block's x.
        x: i32,
        /// The block's y.
        y: i32,
        /// The block's z.
        z: i32,
    },
    /// Land a destroy-stage update at a position. `index` is
    /// `(int)(progress * 10.0F) - 1`; a negative index removes the entry
    /// (`RenderGlobal.sendBlockBreakProgress`, `:2362-2381`).
    Stage {
        /// The block's x.
        x: i32,
        /// The block's y.
        y: i32,
        /// The block's z.
        z: i32,
        /// The stage index; negative means remove.
        index: i32,
    },
}

/// The destroy-stage index a progress value names:
/// `(int)(progress * 10.0F) - 1` (`PlayerControllerMP.java:263`, `:331`).
///
/// A negative index is the removal the stage map reads. The completing tick
/// sends the index its reset progress produces — `-1` — so the indices the
/// client's own digs send are `-1..=8`; `9` would need a progress in
/// `1.0..2.0`, which the completion branch consumes first (`:321-326`).
fn stage_index(progress: f32) -> i32 {
    (progress * 10.0) as i32 - 1
}

/// The digging state machine: `PlayerControllerMP`'s own dig fields and
/// paths.
///
/// [`DigState::click`] is `clickMouse`'s click path (`Minecraft.java:1522-1563`
/// over `clickBlock`, `PlayerControllerMP.java:198-266`), run once per left
/// press; [`DigState::on_player_damage_block`] is
/// `sendClickBlockToController`'s held path (`Minecraft.java:1496-1520` over
/// `onPlayerDamageBlock`, `:285-339`), run once per tick while the button is
/// held; and [`DigState::reset_block_removing`] is `resetBlockRemoving`
/// (`:274-283`), taken when the button is released or the aim leaves every
/// block. The completion's local removal is the session's, through
/// [`DigAction::Destroy`].
///
/// The fields are the controller's own: `currentBlock` (`:46-56`), the
/// progress `curBlockDamageMP` (`:43`), the `blockHitDelay` that skips
/// progress after a break (`:53`) and the `isHittingBlock` flag (`:56`). Two
/// details of the source's click path are not carried: `onPlayerDamageBlock`'s
/// air guard (`:305-309`), which this client's ray cannot reach — the change
/// that empties a block also empties the aim, and that resets the dig — and
/// `leftClickCounter` (`Minecraft.java:1500-1503`, `:1524`), the miss
/// cooldown, which this client has no attack-miss path to throttle. The
/// swing rule is the plan's: one per press (`clickMouse:1526`) and one on the
/// completing tick (`:1509-1512`); the source's per-held-tick swing is not
/// carried.
#[derive(Debug, Clone, PartialEq)]
pub struct DigState {
    /// `currentBlock`: the block a running dig is hitting.
    target: Option<(i32, i32, i32)>,
    /// `isHittingBlock`.
    hitting: bool,
    /// `curBlockDamageMP`: the accumulated progress, from 0.0 to the 1.0 that
    /// completes the dig.
    progress: f32,
    /// `blockHitDelay`: ticks that skip progress after a block is gone.
    hit_delay: i32,
}

impl Default for DigState {
    fn default() -> Self {
        Self::new()
    }
}

impl DigState {
    /// A machine that is hitting nothing.
    pub fn new() -> Self {
        Self {
            target: None,
            hitting: false,
            progress: 0.0,
            hit_delay: 0,
        }
    }

    /// Whether a dig is running (`isHittingBlock`).
    pub fn hitting(&self) -> bool {
        self.hitting
    }

    /// The accumulated progress, 0.0 up to the 1.0 that completes the dig.
    pub fn progress(&self) -> f32 {
        self.progress
    }

    /// The block a running dig is hitting, when one is.
    pub fn target(&self) -> Option<(i32, i32, i32)> {
        self.target
    }

    /// One left press: the swing, then `clickBlock` on the aimed block.
    ///
    /// The source swings before it looks at the mouse-over, so a press with
    /// no aim still swings and digs nothing (`Minecraft.java:1526-1528`).
    pub fn click(&mut self, aim: Option<DigAim>, creative: bool) -> Vec<DigAction> {
        let mut actions = vec![DigAction::Swing];
        if let Some(aim) = aim {
            self.click_block(aim, creative, &mut actions);
        }
        actions
    }

    /// One held tick against the aim: `onPlayerDamageBlock`, or the reset the
    /// caller's else branch takes when the aim left every block
    /// (`Minecraft.java:1515-1518`).
    pub fn on_player_damage_block(
        &mut self,
        aim: Option<DigAim>,
        creative: bool,
    ) -> Vec<DigAction> {
        let mut actions = Vec::new();
        match aim {
            Some(aim) => self.damage_block(aim, creative, &mut actions),
            None => self.reset(&mut actions),
        }
        actions
    }

    /// The release, or the aim leaving every block: `resetBlockRemoving`
    /// (`PlayerControllerMP.java:274-283`).
    pub fn reset_block_removing(&mut self) -> Vec<DigAction> {
        let mut actions = Vec::new();
        self.reset(&mut actions);
        actions
    }

    /// `clickBlock` (`PlayerControllerMP.java:198-266`): the creative
    /// instant branch, the cancel of a different running block, the start,
    /// and the running-dig state a rate under 1.0 leaves.
    fn click_block(&mut self, aim: DigAim, creative: bool, actions: &mut Vec<DigAction>) {
        if creative {
            // The creative branch (`:230-235`): the start, the instant
            // destroy and the five-tick hit delay; no dig state is kept.
            actions.push(DigAction::Start {
                x: aim.x,
                y: aim.y,
                z: aim.z,
                face: aim.face,
            });
            actions.push(DigAction::Destroy {
                x: aim.x,
                y: aim.y,
                z: aim.z,
            });
            self.hit_delay = 5;
            return;
        }
        if self.hitting && self.target == Some((aim.x, aim.y, aim.z)) {
            return;
        }
        if self.hitting {
            // The previous block's cancel carries the incoming face
            // (`:238-241`): the source passes its own parameter through.
            let current = self.target.expect("a running dig has a block");
            actions.push(DigAction::Abort {
                x: current.0,
                y: current.1,
                z: current.2,
                face: aim.face,
            });
        }
        actions.push(DigAction::Start {
            x: aim.x,
            y: aim.y,
            z: aim.z,
            face: aim.face,
        });
        if aim.rate >= 1.0 {
            // The instant branch (`:252-255`): a block the hand completes in
            // one step is destroyed without a running dig.
            actions.push(DigAction::Destroy {
                x: aim.x,
                y: aim.y,
                z: aim.z,
            });
        } else {
            self.hitting = true;
            self.target = Some((aim.x, aim.y, aim.z));
            self.progress = 0.0;
            actions.push(DigAction::Stage {
                x: aim.x,
                y: aim.y,
                z: aim.z,
                index: stage_index(0.0),
            });
        }
    }

    /// `onPlayerDamageBlock` (`PlayerControllerMP.java:285-339`): the hit
    /// delay, the creative branch, one progress step with its stage, the
    /// completion, or a fresh click for a block the dig is not hitting.
    fn damage_block(&mut self, aim: DigAim, creative: bool, actions: &mut Vec<DigAction>) {
        if self.hit_delay > 0 {
            // The delay's ticks skip progress (`:289-293`).
            self.hit_delay -= 1;
            return;
        }
        if creative {
            // The creative branch (`:294-300`): a start and an instant
            // destroy every five ticks.
            self.hit_delay = 5;
            actions.push(DigAction::Start {
                x: aim.x,
                y: aim.y,
                z: aim.z,
                face: aim.face,
            });
            actions.push(DigAction::Destroy {
                x: aim.x,
                y: aim.y,
                z: aim.z,
            });
            return;
        }
        if !self.hitting || self.target != Some((aim.x, aim.y, aim.z)) {
            // Not hitting this block: the source's own fallback is a fresh
            // click (`:337`).
            self.click_block(aim, creative, actions);
            return;
        }
        self.progress += aim.rate;
        if self.progress >= 1.0 {
            // The completion (`:321-329`): the stop goes out, the block is
            // removed locally, the progress resets, and the stage that
            // follows carries that reset. The caller's swing follows the
            // return (`Minecraft.java:1509-1512`).
            self.hitting = false;
            self.target = None;
            actions.push(DigAction::Finish {
                x: aim.x,
                y: aim.y,
                z: aim.z,
                face: aim.face,
            });
            actions.push(DigAction::Destroy {
                x: aim.x,
                y: aim.y,
                z: aim.z,
            });
            self.progress = 0.0;
            self.hit_delay = 5;
            actions.push(DigAction::Stage {
                x: aim.x,
                y: aim.y,
                z: aim.z,
                index: stage_index(self.progress),
            });
            actions.push(DigAction::Swing);
            return;
        }
        actions.push(DigAction::Stage {
            x: aim.x,
            y: aim.y,
            z: aim.z,
            index: stage_index(self.progress),
        });
    }

    /// `resetBlockRemoving` (`PlayerControllerMP.java:274-283`): the abort
    /// with the DOWN face, the cleared state and the stage's literal `-1`.
    fn reset(&mut self, actions: &mut Vec<DigAction>) {
        if !self.hitting {
            return;
        }
        let current = self.target.expect("a running dig has a block");
        actions.push(DigAction::Abort {
            x: current.0,
            y: current.1,
            z: current.2,
            face: Face::Down,
        });
        self.hitting = false;
        self.target = None;
        self.progress = 0.0;
        actions.push(DigAction::Stage {
            x: current.0,
            y: current.1,
            z: current.2,
            index: -1,
        });
    }
}

/// The destroy stages the crack overlay draws from, keyed by block position
/// until M4's entity work.
///
/// The source keys its map by the breaking player's entity id
/// (`RenderGlobal.damagedBlocks`, `client/renderer/RenderGlobal.java:126-127`),
/// one `DestroyBlockProgress` per breaker — so two breakers on one block share
/// one entry only while the position matches (`:2366-2372`). This client has
/// no entity filtering yet, so the map is keyed by position: a 0x25 from any
/// entity lands on the block's one entry.
///
/// A stage lands through [`BreakStages::set`] (0..=9) and is removed by
/// [`BreakStages::clear`]; [`BreakStages::tick`] advances the counter and,
/// every twentieth tick, sweeps entries whose last update is more than 400
/// ticks old (`RenderGlobal.updateClouds`, `:1138-1146`, over
/// `cleanupDamagedBlocks`'s `cloudTickCounter - i > 400`, `:1131`), so a
/// removal lands 401 to 420 ticks after the last update.
#[derive(Debug, Clone)]
pub struct BreakStages {
    /// The source's `cloudTickCounter`, advanced once per tick.
    counter: u32,
    /// The live entries by block position.
    entries: HashMap<(i32, i32, i32), StageEntry>,
}

/// One stage entry: the stage and the counter's value at its last update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StageEntry {
    /// The stage the crack overlay draws, 0..=9.
    stage: u8,
    /// The counter when the entry was last updated
    /// (`DestroyBlockProgress.createdAtCloudUpdateTick`, `:57-60`).
    last: u32,
}

impl Default for BreakStages {
    fn default() -> Self {
        Self::new()
    }
}

impl BreakStages {
    /// An empty map at counter zero.
    pub fn new() -> Self {
        Self {
            counter: 0,
            entries: HashMap::new(),
        }
    }

    /// The stage a position holds, when it holds one.
    pub fn stage(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        self.entries.get(&(x, y, z)).map(|entry| entry.stage)
    }

    /// How many entries are live.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map holds no entry.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Lands a stage on a position, answering whether the stored value
    /// changed.
    ///
    /// The source's set (`sendBlockBreakProgress:2364-2375`): an entry is
    /// created when none holds the position, the stage is stored, and the
    /// last-update tick is refreshed either way.
    pub fn set(&mut self, x: i32, y: i32, z: i32, stage: u8) -> bool {
        let counter = self.counter;
        match self.entries.get_mut(&(x, y, z)) {
            Some(entry) => {
                let changed = entry.stage != stage;
                entry.stage = stage;
                entry.last = counter;
                changed
            }
            None => {
                self.entries.insert(
                    (x, y, z),
                    StageEntry {
                        stage,
                        last: counter,
                    },
                );
                true
            }
        }
    }

    /// Removes a position's entry, answering whether one was held
    /// (`sendBlockBreakProgress`'s else branch, `:2377-2380`).
    pub fn clear(&mut self, x: i32, y: i32, z: i32) -> bool {
        self.entries.remove(&(x, y, z)).is_some()
    }

    /// Advances the counter one tick and sweeps expired entries every
    /// twentieth tick, answering the positions it removed.
    ///
    /// The sweep removes an entry when `counter - last > 400`
    /// (`cleanupDamagedBlocks:1131`); with the sweep every twenty ticks a
    /// removal lands 401 to 420 ticks after the last update. The positions are
    /// sorted, so the events a session reports are deterministic where the
    /// source's map iteration is not.
    pub fn tick(&mut self) -> Vec<(i32, i32, i32)> {
        self.counter = self.counter.wrapping_add(1);
        if self.counter % 20 != 0 {
            return Vec::new();
        }
        let counter = self.counter;
        let mut removed: Vec<(i32, i32, i32)> = self
            .entries
            .iter()
            .filter(|(_, entry)| counter.wrapping_sub(entry.last) > 400)
            .map(|(position, _)| *position)
            .collect();
        removed.sort_unstable();
        for position in &removed {
            self.entries.remove(position);
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    //! The raycast against synthetic worlds, built through the light tests'
    //! column fixture path (`oxide-world/tests/light.rs`): one column of
    //! sixteen sections, applied at chunk (0, 0).

    use oxide_proto_v47::column::{ColumnData, SectionData, block_index};
    use oxide_world::chunk::{SECTION_COUNT, SECTION_SIZE};
    use oxide_world::world::World;

    use super::{
        Aim, BreakStages, CREATIVE_REACH, DigAction, DigAim, DigState, Face, Placement,
        SURVIVAL_REACH, hand_rate, look_vector, placement, placement_cursor, raycast, reach,
        replaceable, tool_not_required,
    };
    use crate::player::Player;
    use crate::world_view::WorldView;
    use oxide_world::behaviour::Material;

    /// Air.
    const AIR: u16 = 0;
    /// Stone, id 1.
    const STONE: u16 = 1 << 4;
    /// Tall grass, id 31.
    const TALL_GRASS: u16 = 31 << 4;
    /// Water, id 9, the still level.
    const WATER: u16 = 9 << 4;
    /// Lava, id 11.
    const LAVA: u16 = 11 << 4;
    /// Fire, id 51: outside the covered set, as `BlockFire.isCollidable`
    /// answers false (`block/BlockFire.java:353-356`).
    const FIRE: u16 = 51 << 4;
    /// The double stone slab, id 43 — the one covered slab id, whose collision
    /// box is the full cell (`CollisionShape::Slab { double: true }`).
    const SLAB: u16 = 43 << 4;
    /// A lone fence, id 85: its post is 0.375..0.625 across and 1.5 tall
    /// (`BlockFence.java:50-107`).
    const FENCE: u16 = 85 << 4;
    /// The dead bush, id 32, a `Material.vine` plant like the tall grass.
    const DEAD_BUSH: u16 = 32 << 4;
    /// The poppy, id 38: a plant of `Material.plants`, which does not carry
    /// the replaceable flag (`Material.java:15`).
    const FLOWER: u16 = 38 << 4;
    /// The double plant, id 175, its grass variant (meta 2) — the replaceable
    /// half of the block's own override (`BlockDoublePlant.java:69-81`).
    const DOUBLE_PLANT_GRASS: u16 = (175 << 4) | 2;
    /// The double plant's fern variant (meta 3), replaceable like the grass.
    const DOUBLE_PLANT_FERN: u16 = (175 << 4) | 3;
    /// The double plant's rose variant (meta 4): the override refuses it, so
    /// a placement steps beside it.
    const DOUBLE_PLANT_ROSE: u16 = (175 << 4) | 4;
    /// The snow layer, id 78: outside the covered set, as the ray tests note
    /// the covered non-cube blocks.
    const SNOW_LAYER: u16 = 78 << 4;

    /// One section: every cell's value from `block_at(local x, local y, local
    /// z)`, all light zero.
    fn section_of(block_at: impl Fn(usize, usize, usize) -> u16) -> SectionData {
        let mut blocks = Box::new([0u16; 4096]);
        for ly in 0..SECTION_SIZE {
            for lz in 0..SECTION_SIZE {
                for lx in 0..SECTION_SIZE {
                    blocks[block_index(lx, ly, lz)] = block_at(lx, ly, lz);
                }
            }
        }
        SectionData {
            blocks,
            block_light: Box::new([0; 2048]),
            sky_light: Some(Box::new([0; 2048])),
        }
    }

    /// A world of one column at chunk (0, 0), every cell's block value from
    /// `block_at(x, y, z)` in world coordinates.
    fn world_of(block_at: impl Fn(i32, i32, i32) -> u16) -> World {
        let mut data = ColumnData::empty();
        for sy in 0..SECTION_COUNT {
            data.sections[sy] = Some(section_of(|lx, ly, lz| {
                block_at(lx as i32, (sy * SECTION_SIZE + ly) as i32, lz as i32)
            }));
            data.mask |= 1u16 << sy;
        }
        let mut world = World::new(true);
        world.apply_column(0, 0, &data, true);
        world
    }

    /// The eye a player standing at `feet` looks from: the pose eye, the feet
    /// plus `Player::eye_height()`'s `1.62`.
    fn pose_eye(feet: [f64; 3]) -> [f64; 3] {
        [feet[0], feet[1] + 1.62, feet[2]]
    }

    /// The aim a player at `feet`, looking at `yaw` and `pitch`, finds in
    /// `world` with the given reach.
    fn aim_in(world: &World, feet: [f64; 3], yaw: f32, pitch: f32, reach: f64) -> Option<Aim> {
        raycast(
            &WorldView(world),
            pose_eye(feet),
            look_vector(yaw, pitch),
            reach,
        )
    }

    /// Asserts a hit point equals `expected` within the float noise of the
    /// trig the look vector runs through.
    fn assert_hit(hit: [f64; 3], expected: [f64; 3]) {
        for axis in 0..3 {
            assert!(
                (hit[axis] - expected[axis]).abs() < 1e-9,
                "the hit point {hit:?} is not {expected:?}"
            );
        }
    }

    #[test]
    fn the_placement_target_is_the_aimed_cell_or_its_face_neighbour() {
        // `ItemBlock.onItemUse` keeps the aimed pos when its block answers
        // `Block.isReplaceable` and steps one cell along the face otherwise
        // (`item/ItemBlock.java:38-45`: `pos = pos.offset(side)`), and
        // `BlockPos.offset` adds `EnumFacing`'s front offsets
        // (`util/BlockPos.java:176-181`, `util/EnumFacing.java:222-240`).
        // A stone aim takes every one of the six neighbours.
        let world = world_of(|x, y, z| {
            if x == 0 && y == 65 && z == 2 {
                STONE
            } else {
                AIR
            }
        });
        for (face, target) in [
            (Face::Down, [0, 64, 2]),
            (Face::Up, [0, 66, 2]),
            (Face::North, [0, 65, 1]),
            (Face::South, [0, 65, 3]),
            (Face::West, [-1, 65, 2]),
            (Face::East, [1, 65, 2]),
        ] {
            let aim = Aim {
                x: 0,
                y: 65,
                z: 2,
                face,
                hit: [0.5, 65.5, 2.0],
            };
            assert_eq!(
                placement(&WorldView(&world), &aim).map(|placed| placed.target),
                Some(target),
                "the {face:?} neighbour"
            );
        }
    }

    #[test]
    fn a_replaceable_aimed_block_is_replaced_in_place() {
        // The other half of the same rule (`item/ItemBlock.java:43-45`): tall
        // grass answers `isReplaceable` true (`BlockTallGrass.java:49-52`,
        // material `Material.vine`, `Material.java:16`) and water carries
        // `MaterialLiquid`'s replaceable flag (`MaterialLiquid.java:8`), so
        // the block lands in the aimed cell itself — which the landing check
        // also passes, its material being replaceable.
        let aim = Aim {
            x: 0,
            y: 65,
            z: 2,
            face: Face::North,
            hit: [0.5, 65.5, 2.0],
        };
        for value in [TALL_GRASS, WATER] {
            let world = world_of(|x, y, z| {
                if x == 0 && y == 65 && z == 2 {
                    value
                } else {
                    AIR
                }
            });
            assert_eq!(
                placement(&WorldView(&world), &aim).map(|placed| placed.target),
                Some([0, 65, 2]),
                "the aimed cell itself for {value:#06x}"
            );
        }
    }

    #[test]
    fn the_replaceable_set_is_the_materials_own() {
        // The landing check `World.canBlockBePlaced` runs is the material's
        // `isReplaceable` (`world/World.java:3153-3157`; the field is set by
        // `Material.setReplaceable`, `Material.java:153-157`): true for
        // `MaterialTransparent` — air and fire (`Material.java:5,19`;
        // `MaterialTransparent.java:8`) — for `MaterialLiquid` — water and
        // lava (`Material.java:9-10`; `MaterialLiquid.java:8`) — for
        // `Material.vine` (`Material.java:16`), which the covered plants
        // carry (`BlockTallGrass.java:30`, `BlockDeadBush.java:21`,
        // `BlockDoublePlant.java:34`), and for the four liquid ids the ray
        // passes but a placement can overwrite.
        for value in [
            AIR,
            8 << 4,
            WATER,
            10 << 4,
            LAVA,
            TALL_GRASS,
            DEAD_BUSH,
            DOUBLE_PLANT_GRASS,
            DOUBLE_PLANT_FERN,
        ] {
            assert!(replaceable(value), "{value:#06x} is replaceable");
        }
        // False for `Material.plants` — the flower and crop family, which
        // does not set the flag (`Material.java:15`) — for the rock and wood
        // families, and for the double plant's rose half, which
        // `BlockDoublePlant` overrides away (`:69-81`).
        for value in [
            STONE,
            3 << 4,
            FLOWER,
            39 << 4,
            59 << 4,
            83 << 4,
            SLAB,
            FENCE,
            50 << 4,
            DOUBLE_PLANT_ROSE,
        ] {
            assert!(!replaceable(value), "{value:#06x} is not replaceable");
        }
        // The values outside the covered set carry no table row, so this
        // client refuses to replace them: the snow layer's material and the
        // vine block's are replaceable in the source, fire's too, but nothing
        // the table cannot vouch for is overwritten blind.
        for value in [SNOW_LAYER, 106 << 4, FIRE] {
            assert!(
                !replaceable(value),
                "{value:#06x} is outside the covered set"
            );
        }
    }

    #[test]
    fn the_cursor_bytes_scale_each_hit_fraction_through_sixteen() {
        // `C08PacketPlayerBlockPlacement.writePacketData` writes
        // `(int)(facing * 16.0F)` per axis, each fraction taken from the hit
        // point minus the aimed cell's coordinate (`PlayerControllerMP.java`
        // `:395-397`'s `f`, `f1`, `f2`; `:60-62`'s casts, truncating toward
        // zero).
        let north = Aim {
            x: 0,
            y: 65,
            z: 2,
            face: Face::North,
            hit: [0.5, 65.62, 2.0],
        };
        assert_eq!(
            placement_cursor(&north),
            [8, 9, 0],
            "the fractions (0.5, 0.62, 0.0)"
        );
        // A hit on the cell's far plane carries a fraction of exactly 1 on
        // that axis and writes 16: unclamped, and the source's own value.
        let far_plane = Aim {
            x: 0,
            y: 65,
            z: 2,
            face: Face::East,
            hit: [1.0, 65.62, 2.5],
        };
        assert_eq!(placement_cursor(&far_plane), [16, 9, 8], "a face-exact hit");
        // The floor hit a downward aim meets: the top face's hit at y 64.0
        // over the cell at y 63 has the fraction 1.0 on y.
        let floor = Aim {
            x: 0,
            y: 63,
            z: 2,
            face: Face::Up,
            hit: [0.5, 64.0, 2.1200000000000045],
        };
        assert_eq!(placement_cursor(&floor), [8, 16, 1], "the floor's top face");
    }

    #[test]
    fn a_target_that_cannot_accept_the_block_refuses_the_placement() {
        // The aimed stone's north neighbour is stone too: the landing check
        // (`World.canBlockBePlaced`'s material clause, `:3153-3157`) refuses,
        // and the client keeps the packet (`PlayerControllerMP.java:417-421`
        // returns false before it).
        let blocked = world_of(|x, y, z| {
            if x == 0 && y == 65 && (z == 1 || z == 2) {
                STONE
            } else {
                AIR
            }
        });
        let aim = Aim {
            x: 0,
            y: 65,
            z: 2,
            face: Face::North,
            hit: [0.5, 65.5, 2.0],
        };
        assert_eq!(
            placement(&WorldView(&blocked), &aim),
            None,
            "the occupied neighbour"
        );
        // The target must lie inside the world's build range, 0 through 255:
        // the highest cell's up neighbour is y 256 and the lowest cell's down
        // neighbour is -1.
        let top = world_of(|x, y, z| {
            if x == 0 && y == 255 && z == 2 {
                STONE
            } else {
                AIR
            }
        });
        let up = Aim {
            x: 0,
            y: 255,
            z: 2,
            face: Face::Up,
            hit: [0.5, 255.5, 2.0],
        };
        assert_eq!(
            placement(&WorldView(&top), &up),
            None,
            "y 256 leaves the range"
        );
        let bottom = world_of(|x, y, z| {
            if x == 0 && y == 0 && z == 2 {
                STONE
            } else {
                AIR
            }
        });
        let down = Aim {
            x: 0,
            y: 0,
            z: 2,
            face: Face::Down,
            hit: [0.5, 0.5, 2.0],
        };
        assert_eq!(
            placement(&WorldView(&bottom), &down),
            None,
            "y -1 leaves the range"
        );
    }

    #[test]
    fn a_placement_carries_the_packets_own_facts() {
        // The source builds the packet from the frame's hit result — the
        // aimed position and the side (`PlayerControllerMP.java:424`'s
        // `hitPos` and `side.getIndex()`) and the three fractions
        // (`:395-397`) — and predicts the block at the cell `onItemUse`
        // writes.
        let world = world_of(|x, y, z| {
            if x == 0 && y == 65 && z == 2 {
                STONE
            } else {
                AIR
            }
        });
        let aim = Aim {
            x: 0,
            y: 65,
            z: 2,
            face: Face::North,
            hit: [0.5, 65.62, 2.0],
        };
        assert_eq!(
            placement(&WorldView(&world), &aim),
            Some(Placement {
                x: 0,
                y: 65,
                z: 2,
                face: Face::North,
                cursor: [8, 9, 0],
                target: [0, 65, 1],
            })
        );
    }

    #[test]
    fn the_face_wire_bytes_are_the_sources_facings() {
        // `EnumFacing`'s order is D-U-N-S-W-E (`util/EnumFacing.java:12-17`)
        // and `getIndex` is the ordinal (`:53-58`): the byte the digging and
        // placement packets write.
        assert_eq!(Face::Down.wire(), 0);
        assert_eq!(Face::Up.wire(), 1);
        assert_eq!(Face::North.wire(), 2);
        assert_eq!(Face::South.wire(), 3);
        assert_eq!(Face::West.wire(), 4);
        assert_eq!(Face::East.wire(), 5);
    }

    #[test]
    fn the_look_vector_is_the_sources_rotation_vector() {
        // `Entity.getVectorForRotation` (`Entity.java:1476-1483`):
        // f = cos(-yaw - pi), f1 = sin(-yaw - pi), f2 = -cos(-pitch),
        // f3 = sin(-pitch), giving (f1 * f2, f3, f * f2) — yaw 0 and pitch 0
        // look south along +z, yaw 90 west along -x, pitch 90 straight down.
        let close = |v: [f64; 3], expected: [f64; 3]| {
            (0..3).all(|axis| (v[axis] - expected[axis]).abs() < 1e-12)
        };
        assert!(
            close(look_vector(0.0, 0.0), [0.0, 0.0, 1.0]),
            "yaw 0, pitch 0 is south"
        );
        assert!(
            close(look_vector(90.0, 0.0), [-1.0, 0.0, 0.0]),
            "yaw 90 is west"
        );
        assert!(
            close(look_vector(180.0, 0.0), [0.0, 0.0, -1.0]),
            "yaw 180 is north"
        );
        assert!(
            close(look_vector(-90.0, 0.0), [1.0, 0.0, 0.0]),
            "yaw -90 is east"
        );
        assert!(
            close(look_vector(0.0, 90.0), [0.0, -1.0, 0.0]),
            "pitch 90 looks straight down"
        );
        assert!(
            close(look_vector(0.0, -90.0), [0.0, 1.0, 0.0]),
            "pitch -90 looks straight up"
        );
        // A diagonal: yaw 45 turns half way toward west, pitch 45 half way
        // down.
        let half = std::f64::consts::FRAC_1_SQRT_2;
        assert!(
            close(look_vector(45.0, 0.0), [-half, 0.0, half]),
            "yaw 45 is the south-west diagonal"
        );
        assert!(
            close(look_vector(0.0, 45.0), [0.0, -half, half]),
            "pitch 45 is the down-south diagonal"
        );
    }

    #[test]
    fn the_reach_is_the_gamemodes() {
        // `PlayerControllerMP.getBlockReachDistance` (`:344-346`):
        // `this.currentGameType.isCreative() ? 5.0F : 4.5F`. The gamemode is
        // Join Game's byte with the hardcore bit masked
        // (`S01PacketJoinGame.java:44-47`), so 9 — hardcore creative — is
        // creative.
        assert_eq!(CREATIVE_REACH, 5.0, "the source's creative literal");
        assert_eq!(SURVIVAL_REACH, 4.5, "the source's other literal");
        assert_eq!(reach(1), 5.0, "creative");
        assert_eq!(reach(0x08 | 1), 5.0, "hardcore creative");
        assert_eq!(reach(0), 4.5, "survival");
        assert_eq!(reach(0x08), 4.5, "hardcore survival");
        assert_eq!(reach(2), 4.5, "adventure");
        assert_eq!(reach(3), 4.5, "spectator");
    }

    #[test]
    fn a_block_straight_ahead_is_hit_with_its_face_and_a_point_on_it() {
        // A player at (0.5, 64, 0.5) faces south; a stone block sits two cells
        // south at (0, 65, 2), in the eye's row. The ray meets its north face
        // at z = 2.0, at the eye's own height.
        let world = world_of(|x, y, z| if (x, y, z) == (0, 65, 2) { STONE } else { AIR });
        let aim =
            aim_in(&world, [0.5, 64.0, 0.5], 0.0, 0.0, SURVIVAL_REACH).expect("the block is hit");
        assert_eq!(
            (aim.x, aim.y, aim.z, aim.face),
            (0, 65, 2, Face::North),
            "the block and the face the ray entered through"
        );
        assert_hit(aim.hit, [0.5, 65.62, 2.0]);

        // The nearer of two solid blocks stops the ray.
        let two = world_of(|x, y, z| {
            if (x, y, z) == (0, 65, 2) || (x, y, z) == (0, 65, 4) {
                STONE
            } else {
                AIR
            }
        });
        let near = aim_in(&two, [0.5, 64.0, 0.5], 0.0, 0.0, SURVIVAL_REACH).expect("a block");
        assert_eq!(
            (near.x, near.y, near.z),
            (0, 65, 2),
            "the nearer block stops it"
        );
    }

    #[test]
    fn a_block_below_is_hit_on_its_top_face() {
        // The player stands on stone at (0, 63, 0) and looks straight down:
        // the ray meets the floor's top face at y = 64.0.
        let world = world_of(|x, y, z| if (x, y, z) == (0, 63, 0) { STONE } else { AIR });
        let aim =
            aim_in(&world, [0.5, 64.0, 0.5], 0.0, 90.0, SURVIVAL_REACH).expect("the floor is hit");
        assert_eq!((aim.x, aim.y, aim.z, aim.face), (0, 63, 0, Face::Up));
        assert_hit(aim.hit, [0.5, 64.0, 0.5]);
    }

    #[test]
    fn a_partial_shape_is_hit_on_its_own_face() {
        // A lone fence at (2, 65, 0) is its post: x 0.375..0.625, z
        // 0.375..0.625, y 65..66.5. A player looking east meets the post's
        // west face at x = 2.375 — the box's face, not the cell's.
        let world = world_of(|x, y, z| if (x, y, z) == (2, 65, 0) { FENCE } else { AIR });
        let aim =
            aim_in(&world, [0.5, 64.0, 0.5], -90.0, 0.0, SURVIVAL_REACH).expect("the post is hit");
        assert_eq!((aim.x, aim.y, aim.z, aim.face), (2, 65, 0, Face::West));
        assert_hit(aim.hit, [2.375, 65.62, 0.5]);
    }

    #[test]
    fn beyond_reach_misses() {
        // A stone block whose north face is 4.55 from the eye: out of the
        // survival reach, inside the creative one.
        let world = world_of(|x, y, z| if (x, y, z) == (0, 65, 5) { STONE } else { AIR });
        let feet = [0.5, 64.0, 0.45];
        assert_eq!(
            aim_in(&world, feet, 0.0, 0.0, SURVIVAL_REACH),
            None,
            "4.55 is beyond the 4.5 reach"
        );
        assert!(
            aim_in(&world, feet, 0.0, 0.0, CREATIVE_REACH).is_some(),
            "4.55 is inside the 5.0 reach"
        );
    }

    #[test]
    fn plants_stop_the_ray_while_liquids_air_and_fire_pass() {
        // The frame path's flags (`entity/Entity.java:1505`) leave the
        // collision-box clause of the cell test out and pass
        // `stopOnLiquid = false` (`world/World.java:904`, `:1037`), so the
        // stop check is `canCollideCheck(state, false)` — `Block.isCollidable()`
        // (`block/Block.java:512-515`). A plant passes that check, so it stops
        // the ray on its own selection bounds (`block/BlockBush.java:31`), not
        // the cell's.
        let behind = world_of(|x, y, z| match (x, y, z) {
            (0, 65, 2) => TALL_GRASS,
            (0, 65, 5) => STONE,
            _ => AIR,
        });
        let aim = aim_in(&behind, [0.5, 64.0, 0.5], 0.0, 0.0, SURVIVAL_REACH)
            .expect("the grass stops the ray");
        assert_eq!(
            (aim.x, aim.y, aim.z, aim.face),
            (0, 65, 2, Face::North),
            "the tall grass stops the ray before the stone behind it"
        );
        assert_hit(aim.hit, [0.5, 65.62, 2.1]);

        // Liquids do not: `BlockLiquid.canCollideCheck` answers false at the
        // frame path's `stopOnLiquid = false` (`block/BlockLiquid.java:82-85`).
        // With only water and lava in the path the ray meets nothing; with
        // stone behind them it meets the stone.
        let soft = world_of(|x, y, z| match (x, y, z) {
            (0, 65, 2) => WATER,
            (0, 65, 3) => LAVA,
            _ => AIR,
        });
        assert_eq!(
            aim_in(&soft, [0.5, 64.0, 0.5], 0.0, 0.0, SURVIVAL_REACH),
            None,
            "water and lava do not stop the ray"
        );
        let soggy = world_of(|x, y, z| match (x, y, z) {
            (0, 65, 2) => WATER,
            (0, 65, 3) => LAVA,
            (0, 65, 5) => STONE,
            _ => AIR,
        });
        let aim = aim_in(&soggy, [0.5, 64.0, 0.5], 0.0, 0.0, SURVIVAL_REACH)
            .expect("the stone behind the liquids is hit");
        assert_eq!(
            (aim.x, aim.y, aim.z, aim.face),
            (0, 65, 5, Face::North),
            "the ray reaches the stone behind the liquids"
        );

        // Fire answers false too (`block/BlockFire.java:353-356`) and is
        // outside the covered set: the ray passes it as it passes air.
        let burning = world_of(|x, y, z| match (x, y, z) {
            (0, 65, 2) => FIRE,
            (0, 65, 5) => STONE,
            _ => AIR,
        });
        let aim = aim_in(&burning, [0.5, 64.0, 0.5], 0.0, 0.0, SURVIVAL_REACH)
            .expect("the stone behind the fire is hit");
        assert_eq!(
            (aim.x, aim.y, aim.z, aim.face),
            (0, 65, 5, Face::North),
            "the fire does not stop the ray"
        );
    }

    /// Asserts the ray from `feet`, at `yaw` and `pitch`, stops at the single
    /// block in `world`'s cell (0, 65, 2) with `face` and the hit point `hit`.
    fn assert_stops_at(
        block: u16,
        feet: [f64; 3],
        yaw: f32,
        pitch: f32,
        face: Face,
        hit: [f64; 3],
        message: &str,
    ) {
        let world = world_of(|x, y, z| if (x, y, z) == (0, 65, 2) { block } else { AIR });
        let aim = aim_in(&world, feet, yaw, pitch, SURVIVAL_REACH);
        assert_eq!(
            aim.map(|aim| (aim.x, aim.y, aim.z, aim.face)),
            Some((0, 65, 2, face)),
            "{message}"
        );
        assert_hit(aim.expect("the block stops the ray").hit, hit);
    }

    #[test]
    fn the_collidable_non_cube_blocks_stop_the_ray_on_their_selection_bounds() {
        // The covered ids that answer no collision box but pass the source's
        // stop check — the cross plants, the torch, the pressure plate, the
        // reeds, the crops and the double plant — stop the ray on the
        // selection bounds their own classes set (see `selection_bounds`).
        // Each world holds one block at (0, 65, 2), in the eye's own row.
        let row_eye = [0.5, 64.0, 0.5]; // eye y 65.62, inside the 0.8-high boxes
        let low_eye = [0.5, 63.5, 0.5]; // eye y 65.12, inside the 0.25-high crops
        let bush_eye = [0.5, 63.9, 0.5]; // eye y 65.52, inside the 0.6-high bush
        let torch_eye = [0.5, 63.78, 2.5]; // eye y 65.4, inside a wall torch's band

        // The tall grass (`block/BlockTallGrass.java:32-33`) and the dead bush
        // (`block/BlockDeadBush.java:22-23`): x/z 0.1..0.9, y 0..0.8.
        assert_stops_at(
            31 << 4,
            row_eye,
            0.0,
            0.0,
            Face::North,
            [0.5, 65.62, 2.1],
            "the tall grass stops the ray",
        );
        assert_stops_at(
            32 << 4,
            row_eye,
            0.0,
            0.0,
            Face::North,
            [0.5, 65.62, 2.1],
            "the dead bush stops the ray",
        );
        // The flowers carry the bush default (`block/BlockBush.java:30-31`).
        assert_stops_at(
            37 << 4,
            bush_eye,
            0.0,
            0.0,
            Face::North,
            [0.5, 65.52, 2.3],
            "the yellow flower stops the ray",
        );
        assert_stops_at(
            38 << 4,
            bush_eye,
            0.0,
            0.0,
            Face::North,
            [0.5, 65.52, 2.3],
            "the red flower stops the ray",
        );
        // The mushrooms (`block/BlockMushroom.java:15-16`): y 0..0.4.
        assert_stops_at(
            39 << 4,
            low_eye,
            0.0,
            0.0,
            Face::North,
            [0.5, 65.12, 2.3],
            "the brown mushroom stops the ray",
        );
        assert_stops_at(
            40 << 4,
            low_eye,
            0.0,
            0.0,
            Face::North,
            [0.5, 65.12, 2.3],
            "the red mushroom stops the ray",
        );
        // The crops and their carrot and potato subclasses
        // (`block/BlockCrops.java:25-26`, `BlockCarrot.java:6`,
        // `BlockPotato.java:10`): the full footprint, y 0..0.25.
        assert_stops_at(
            59 << 4,
            low_eye,
            0.0,
            0.0,
            Face::North,
            [0.5, 65.12, 2.0],
            "the wheat stops the ray",
        );
        assert_stops_at(
            141 << 4,
            low_eye,
            0.0,
            0.0,
            Face::North,
            [0.5, 65.12, 2.0],
            "the carrots stop the ray",
        );
        assert_stops_at(
            142 << 4,
            low_eye,
            0.0,
            0.0,
            Face::North,
            [0.5, 65.12, 2.0],
            "the potatoes stop the ray",
        );
        // The reeds (`block/BlockReed.java:26-28`).
        assert_stops_at(
            83 << 4,
            row_eye,
            0.0,
            0.0,
            Face::North,
            [0.5, 65.62, 2.125],
            "the reeds stop the ray",
        );
        // The double plant: the full cube (`block/BlockDoublePlant.java:41-43`).
        assert_stops_at(
            175 << 4,
            row_eye,
            0.0,
            0.0,
            Face::North,
            [0.5, 65.62, 2.0],
            "the double plant stops the ray",
        );
        // The torch (`BlockTorch.collisionRayTrace`, `:185-208`): standing
        // (metadata 0 and the wire's own 5, `:244-269`) and the four wall
        // facings, each met on its own box.
        assert_stops_at(
            50 << 4,
            bush_eye,
            0.0,
            0.0,
            Face::North,
            [0.5, 65.52, 2.4],
            "the standing torch stops the ray",
        );
        assert_stops_at(
            50 << 4 | 5,
            bush_eye,
            0.0,
            0.0,
            Face::North,
            [0.5, 65.52, 2.4],
            "the wire's standing torch stops the ray",
        );
        assert_stops_at(
            50 << 4 | 1,
            torch_eye,
            90.0,
            0.0,
            Face::East,
            [0.3, 65.4, 2.5],
            "the east-facing torch stops the ray on its west edge",
        );
        assert_stops_at(
            50 << 4 | 2,
            torch_eye,
            -90.0,
            0.0,
            Face::West,
            [0.7, 65.4, 2.5],
            "the west-facing torch stops the ray on its east edge",
        );
        assert_stops_at(
            50 << 4 | 3,
            [0.5, 63.78, 0.5],
            0.0,
            0.0,
            Face::North,
            [0.5, 65.4, 2.0],
            "the south-facing torch stops the ray",
        );
        assert_stops_at(
            50 << 4 | 4,
            [0.5, 63.78, 0.5],
            0.0,
            0.0,
            Face::North,
            [0.5, 65.4, 2.7],
            "the north-facing torch stops the ray",
        );
        // The pressure plate, met from above (`block/BlockBasePressurePlate.java:34-46`):
        // the unpowered 1/16 and the powered 1/32 top (metadata 1 is powered,
        // `BlockPressurePlate.getStateFromMeta`, `:73-76`).
        assert_stops_at(
            72 << 4,
            [0.5, 65.38, 2.5],
            0.0,
            90.0,
            Face::Up,
            [0.5, 65.0625, 2.5],
            "the unpowered pressure plate stops the ray",
        );
        assert_stops_at(
            72 << 4 | 1,
            [0.5, 65.38, 2.5],
            0.0,
            90.0,
            Face::Up,
            [0.5, 65.03125, 2.5],
            "the powered pressure plate stops the ray",
        );

        // The trace runs on those bounds, not the whole cell: rays beside the
        // tall grass box (x 0.05 < 0.1) and beside the standing torch box
        // (x 0.2 < 0.4) pass them and find nothing.
        let grass = world_of(|x, y, z| {
            if (x, y, z) == (0, 65, 2) {
                TALL_GRASS
            } else {
                AIR
            }
        });
        assert_eq!(
            aim_in(&grass, [0.05, 64.0, 0.5], 0.0, 0.0, SURVIVAL_REACH),
            None,
            "beside the grass bounds the ray passes"
        );
        let torch = world_of(|x, y, z| {
            if (x, y, z) == (0, 65, 2) {
                50 << 4
            } else {
                AIR
            }
        });
        assert_eq!(
            aim_in(&torch, [0.2, 63.9, 0.5], 0.0, 0.0, SURVIVAL_REACH),
            None,
            "beside the torch bounds the ray passes"
        );
    }

    #[test]
    fn the_origin_is_the_pose_eye_and_not_the_render_eye() {
        // The render camera sits a tenth of a block behind the pose eye on the
        // view axis (`oxide-render`'s `FIRST_PERSON_OFFSET`,
        // `EntityRenderer.java:720`); the interaction ray starts at the pose
        // eye — the feet plus `eye_height()`, no displacement.
        //
        // A slab (id 43) whose north face is 4.95 from the pose eye is reached
        // there, and missed from the displaced eye, 5.05 away. The origin is
        // composed the way the session composes it, through the player's own
        // eye height.
        let mut player = Player::new();
        player.position = [0.5, 64.0, 0.05];
        let eye = [
            player.position[0],
            player.position[1] + player.eye_height(),
            player.position[2],
        ];
        let dir = look_vector(player.yaw, player.pitch);
        let world = world_of(|x, y, z| if (x, y, z) == (0, 65, 5) { SLAB } else { AIR });
        let aim = raycast(&WorldView(&world), eye, dir, CREATIVE_REACH)
            .expect("the pose eye reaches the slab");
        assert_eq!((aim.x, aim.y, aim.z, aim.face), (0, 65, 5, Face::North));
        assert_hit(aim.hit, [0.5, 65.62, 5.0]);

        let displaced = [
            eye[0] - dir[0] * 0.1,
            eye[1] - dir[1] * 0.1,
            eye[2] - dir[2] * 0.1,
        ];
        assert_eq!(
            raycast(&WorldView(&world), displaced, dir, CREATIVE_REACH),
            None,
            "the render eye, a tenth of a block behind, is beyond the reach"
        );

        // And the other way: a slab whose face is 5.05 from the pose eye is
        // missed there and reached by an eye displaced a tenth along the view
        // axis, so the test fails whichever origin a displaced ray would use.
        let far = world_of(|x, y, z| if (x, y, z) == (0, 65, 6) { SLAB } else { AIR });
        let feet = [0.5, 64.0, 0.95];
        assert_eq!(
            aim_in(&far, feet, 0.0, 0.0, CREATIVE_REACH),
            None,
            "5.05 is beyond the 5.0 reach"
        );
        let eye = pose_eye(feet);
        let forward = [
            eye[0] + dir[0] * 0.1,
            eye[1] + dir[1] * 0.1,
            eye[2] + dir[2] * 0.1,
        ];
        let aim = raycast(&WorldView(&far), forward, dir, CREATIVE_REACH)
            .expect("a forward-displaced eye reaches it");
        assert_eq!((aim.x, aim.y, aim.z), (0, 65, 6));
    }

    /// A dig aim at one block with the given rate.
    fn dig_aim(rate: f32) -> DigAim {
        DigAim {
            x: 0,
            y: 65,
            z: 2,
            face: Face::North,
            rate,
        }
    }

    /// The damage ticks a survival dig of one block takes: the press, then
    /// held ticks until the finish, answering the number of held ticks that
    /// ran before it.
    fn damage_ticks_to_complete(rate: f32) -> usize {
        let mut dig = DigState::new();
        let aim = dig_aim(rate);
        dig.click(Some(aim), false);
        for tick in 1..=1000 {
            let actions = dig.on_player_damage_block(Some(aim), false);
            if actions
                .iter()
                .any(|action| matches!(action, DigAction::Finish { .. }))
            {
                return tick;
            }
        }
        panic!("the dig never completed");
    }

    #[test]
    fn the_hand_rate_is_the_sources_split_and_the_material_rule_is_the_sources_own() {
        // `Block.getPlayerRelativeBlockHardness` (`block/Block.java:590-594`)
        // with no held item: `getToolDigEfficiency` is the inventory's own
        // `1.0F` (`EntityPlayer.java:900-902`; `InventoryPlayer.getStrVsBlock`
        // `:552-562`) and `canHarvestBlock` is the material's own
        // `isToolNotRequired` (`InventoryPlayer.canHeldItemHarvest:684-695`),
        // so the split is 1/hardness/100 when a tool is required and
        // 1/hardness/30 when it is not; a negative hardness is zero.
        let dirt = hand_rate(0.5, true);
        assert!(
            (dirt - 1.0 / 0.5 / 30.0).abs() < 1e-7,
            "dirt's rate: {dirt}"
        );
        let stone = hand_rate(1.5, false);
        assert!(
            (stone - 1.0 / 1.5 / 100.0).abs() < 1e-7,
            "stone's rate: {stone}"
        );
        assert_eq!(hand_rate(-1.0, true), 0.0, "bedrock: unbreakable");
        assert_eq!(hand_rate(-1.0, false), 0.0, "unbreakable either way");

        // The material rule: `Material.isToolNotRequired`
        // (`block/material/Material.java:174-179`) answers `requiresNoTool` —
        // true by default (`:69`) and cleared by `setRequiresTool` (`:127`)
        // for rock, iron, anvil, snow, crafted snow, web and barrier
        // (`:9-11`, `:29-32`, `:39-45`, `:49`).
        assert!(tool_not_required(Material::Ground), "dirt");
        assert!(tool_not_required(Material::Grass), "grass");
        assert!(tool_not_required(Material::Wood), "wood");
        assert!(!tool_not_required(Material::Rock), "stone's own material");
        assert!(
            !tool_not_required(Material::Stone),
            "the same Material.rock"
        );
        assert!(!tool_not_required(Material::Metal), "iron");
        assert!(!tool_not_required(Material::Snow), "the snow layer");
        assert!(!tool_not_required(Material::CraftedSnow), "the snow block");
        assert!(!tool_not_required(Material::Web), "web");
    }

    #[test]
    fn the_hand_completes_dirt_in_fifteen_ticks_and_stone_in_a_hundred_and_fifty_one() {
        // Dirt's rate is 1/0.5/30 = 1/15 per damage tick, stone's is
        // 1/1.5/100. Fifteen damage ticks complete dirt. Stone's accumulated
        // sum lands at 0.9999992 after 150 additions in `f32` — the source's
        // own float arithmetic (`PlayerControllerMP.java:312` adds the
        // `float`s) — and crosses on the 151st, so the test pins the 151st,
        // not the real-arithmetic 150th.
        assert_eq!(damage_ticks_to_complete(hand_rate(0.5, true)), 15, "dirt");
        assert_eq!(
            damage_ticks_to_complete(hand_rate(1.5, false)),
            151,
            "stone"
        );
    }

    #[test]
    fn the_destroy_stages_step_the_sources_index() {
        // The index is `(int)(progress * 10.0F) - 1`
        // (`PlayerControllerMP.java:263`, `:331`). A dirt dig's fifteen damage
        // ticks step -1, 0, 1, 1, 2, 3, 3, 4, 5, 5, 6, 7, 7, 8 — the
        // two-thirds steps the rate gives — and the completing tick's stage
        // is the reset progress's -1, so 9 is never sent.
        let mut dig = DigState::new();
        let aim = dig_aim(hand_rate(0.5, true));
        dig.click(Some(aim), false);
        let mut stages = Vec::new();
        for _ in 0..15 {
            for action in dig.on_player_damage_block(Some(aim), false) {
                if let DigAction::Stage { index, .. } = action {
                    stages.push(index);
                }
            }
        }
        assert_eq!(
            stages,
            vec![-1, 0, 1, 1, 2, 3, 3, 4, 5, 5, 6, 7, 7, 8, -1],
            "the stepped stages, the completion's removal last"
        );
    }

    #[test]
    fn creative_completes_on_the_first_click() {
        // `clickBlock`'s creative branch (`PlayerControllerMP.java:230-235`):
        // the start and the instant destroy, then the five-tick hit delay
        // (`:234`). The press's swing leads them (`Minecraft.java:1526`).
        let mut dig = DigState::new();
        let actions = dig.click(Some(dig_aim(0.0)), true);
        assert_eq!(
            actions,
            vec![
                DigAction::Swing,
                DigAction::Start {
                    x: 0,
                    y: 65,
                    z: 2,
                    face: Face::North
                },
                DigAction::Destroy { x: 0, y: 65, z: 2 },
            ],
            "the creative click starts, destroys and swings"
        );
        assert!(!dig.hitting(), "no dig runs in creative");

        // The delay's five ticks only count down (`:289-293`); the sixth held
        // tick starts and destroys again (`:294-300`).
        for tick in 1..=5 {
            assert!(
                dig.on_player_damage_block(Some(dig_aim(0.0)), true)
                    .is_empty(),
                "delay tick {tick} sends nothing"
            );
        }
        assert_eq!(
            dig.on_player_damage_block(Some(dig_aim(0.0)), true),
            vec![
                DigAction::Start {
                    x: 0,
                    y: 65,
                    z: 2,
                    face: Face::North
                },
                DigAction::Destroy { x: 0, y: 65, z: 2 },
            ],
            "the five-tick creative break"
        );
    }

    #[test]
    fn bedrock_never_completes() {
        // A negative hardness answers rate 0 (`block/Block.java:590-594`), so
        // no number of held ticks completes it: the dig stays running and
        // every tick's stage is the no-crack -1.
        let mut dig = DigState::new();
        let aim = dig_aim(hand_rate(-1.0, false));
        assert_eq!(aim.rate, 0.0, "bedrock's rate");
        let actions = dig.click(Some(aim), false);
        assert!(
            actions.contains(&DigAction::Start {
                x: 0,
                y: 65,
                z: 2,
                face: Face::North
            }),
            "the dig starts: {actions:?}"
        );
        assert!(dig.hitting(), "the dig runs");
        for tick in 0..600 {
            let actions = dig.on_player_damage_block(Some(aim), false);
            assert!(
                !actions.iter().any(|action| matches!(
                    action,
                    DigAction::Finish { .. } | DigAction::Destroy { .. }
                )),
                "tick {tick} breaks nothing: {actions:?}"
            );
        }
        assert_eq!(dig.progress(), 0.0, "no progress accumulated");
    }

    #[test]
    fn an_aim_change_aborts_the_running_block_and_starts_the_new_one() {
        // `onPlayerDamageBlock`'s else branch runs `clickBlock` (`:337`),
        // whose cancel carries the old block and the *incoming* face
        // (`:238-241` — the source passes its own parameter through), and the
        // start carries the new block with the stage of the reset progress
        // (`:243`, `:263`).
        let mut dig = DigState::new();
        let first = dig_aim(hand_rate(0.5, true));
        dig.click(Some(first), false);
        let second = DigAim {
            x: 1,
            y: 65,
            z: 2,
            face: Face::West,
            rate: hand_rate(1.5, false),
        };
        assert_eq!(
            dig.on_player_damage_block(Some(second), false),
            vec![
                DigAction::Abort {
                    x: 0,
                    y: 65,
                    z: 2,
                    face: Face::West
                },
                DigAction::Start {
                    x: 1,
                    y: 65,
                    z: 2,
                    face: Face::West
                },
                DigAction::Stage {
                    x: 1,
                    y: 65,
                    z: 2,
                    index: -1
                },
            ],
            "the cancel and the new start"
        );
    }

    #[test]
    fn a_release_aborts_with_the_down_face() {
        // `resetBlockRemoving` (`PlayerControllerMP.java:274-283`): the abort
        // carries DOWN (`:278`) and the stage is the literal -1 (`:281`). A
        // second release is a no-op.
        let mut dig = DigState::new();
        let aim = dig_aim(hand_rate(0.5, true));
        dig.click(Some(aim), false);
        assert_eq!(
            dig.reset_block_removing(),
            vec![
                DigAction::Abort {
                    x: 0,
                    y: 65,
                    z: 2,
                    face: Face::Down
                },
                DigAction::Stage {
                    x: 0,
                    y: 65,
                    z: 2,
                    index: -1
                },
            ],
            "the abort and the stage's removal"
        );
        assert!(!dig.hitting(), "the dig stopped");
        assert!(
            dig.reset_block_removing().is_empty(),
            "a second release is a no-op"
        );
    }

    #[test]
    fn the_completion_finishes_before_it_removes() {
        // The source's order (`PlayerControllerMP.java:321-331`): the STOP
        // goes out (`:324`), then `onPlayerDestroyBlock`'s local removal
        // (`:325`), then the stage of the reset progress (`:326`, `:331`) —
        // and the caller's swing follows the return (`Minecraft.java:1509-1512`).
        let mut dig = DigState::new();
        let aim = dig_aim(hand_rate(0.5, true));
        dig.click(Some(aim), false);
        let mut last = Vec::new();
        for _ in 0..15 {
            last = dig.on_player_damage_block(Some(aim), false);
        }
        assert_eq!(
            last,
            vec![
                DigAction::Finish {
                    x: 0,
                    y: 65,
                    z: 2,
                    face: Face::North
                },
                DigAction::Destroy { x: 0, y: 65, z: 2 },
                DigAction::Stage {
                    x: 0,
                    y: 65,
                    z: 2,
                    index: -1
                },
                DigAction::Swing,
            ],
            "finish, then the removal, then the stage, then the swing"
        );
        assert!(!dig.hitting(), "the dig completed");
    }

    #[test]
    fn the_hit_delay_skips_five_ticks_after_a_break() {
        // `blockHitDelay = 5` on the completion (`:328`) and its ticks only
        // count down (`:289-293`): the next block takes no progress — and no
        // start — for five held ticks.
        let mut dig = DigState::new();
        let aim = dig_aim(hand_rate(0.5, true));
        dig.click(Some(aim), false);
        for _ in 0..15 {
            dig.on_player_damage_block(Some(aim), false);
        }
        let next = DigAim {
            x: 1,
            y: 65,
            z: 2,
            face: Face::West,
            rate: hand_rate(0.5, true),
        };
        for tick in 1..=5 {
            assert!(
                dig.on_player_damage_block(Some(next), false).is_empty(),
                "delay tick {tick} sends nothing"
            );
        }
        let started = dig.on_player_damage_block(Some(next), false);
        assert!(
            started.contains(&DigAction::Start {
                x: 1,
                y: 65,
                z: 2,
                face: Face::West
            }),
            "the sixth tick starts the next block: {started:?}"
        );
    }

    #[test]
    fn stages_set_step_and_clear() {
        // The map's own contract: a new entry changed it, a repeated stage
        // does not, a step does, and a clear answers whether one was held.
        let mut stages = BreakStages::new();
        assert!(stages.is_empty(), "a new map is empty");
        assert!(stages.set(3, 65, 2, 0), "a new entry changed the map");
        assert_eq!(stages.stage(3, 65, 2), Some(0), "the stage landed");
        assert!(!stages.set(3, 65, 2, 0), "the same stage is no change");
        assert!(stages.set(3, 65, 2, 4), "a step changed the map");
        assert_eq!(stages.stage(3, 65, 2), Some(4), "the step landed");
        assert_eq!(stages.len(), 1, "one entry");
        assert!(stages.clear(3, 65, 2), "the entry was held");
        assert_eq!(stages.stage(3, 65, 2), None, "the entry is gone");
        assert!(!stages.clear(3, 65, 2), "a second clear is a no-op");
        assert!(stages.is_empty(), "the map is empty again");
    }

    #[test]
    fn an_entry_expires_at_the_first_sweep_past_four_hundred_ticks() {
        // The sweep runs every twentieth tick (`RenderGlobal.updateClouds`,
        // `RenderGlobal.java:1138-1146`) and removes when
        // `cloudTickCounter - i > 400` (`cleanupDamagedBlocks:1131`): an entry
        // set at counter 1 is removed at counter 420 — 419 ticks after its
        // last update, the first sweep past 400.
        let mut stages = BreakStages::new();
        stages.tick(); // the counter is 1
        stages.set(2, 65, 2, 5);
        let mut removals = Vec::new();
        for counter in 2..=440 {
            for position in stages.tick() {
                removals.push((counter, position));
            }
        }
        assert_eq!(
            removals,
            vec![(420, (2, 65, 2))],
            "removed at counter 420, 419 ticks after the last update"
        );
    }

    #[test]
    fn an_age_of_exactly_four_hundred_ticks_survives_its_sweep() {
        // The comparison is strict (`cloudTickCounter - i > 400`,
        // `cleanupDamagedBlocks:1131`), so an entry whose age is exactly 400
        // at a sweep survives it: one set at counter 20 is swept at 420 with
        // an age of 400 — kept — and removed at the next sweep, 440.
        let mut stages = BreakStages::new();
        for _ in 0..20 {
            stages.tick(); // the counter is 20, and its sweep found nothing
        }
        stages.set(2, 65, 2, 5);
        let mut removed = None;
        for counter in 21..=460 {
            if stages.tick().contains(&(2, 65, 2)) {
                removed = Some(counter);
            }
        }
        assert_eq!(
            removed,
            Some(440),
            "an age of exactly 400 at counter 420 is kept; 420 removes it"
        );
    }

    #[test]
    fn an_update_refreshes_an_entrys_age() {
        // The source refreshes the entry's tick on every set
        // (`setCloudUpdateTick`, `RenderGlobal.java:2375`), so a 0x25 update
        // extends its life: an entry refreshed at counter 401 is removed at
        // counter 820, not 420.
        let mut stages = BreakStages::new();
        stages.tick(); // the counter is 1
        stages.set(2, 65, 2, 5);
        for _ in 0..400 {
            stages.tick(); // the counter is 401
        }
        stages.set(2, 65, 2, 6);
        let mut removed = None;
        for counter in 402..=840 {
            if stages.tick().contains(&(2, 65, 2)) {
                removed = Some(counter);
            }
        }
        assert_eq!(
            removed,
            Some(820),
            "removed at counter 820, 419 ticks after the refresh"
        );
    }

    #[test]
    fn two_updates_on_one_position_share_one_entry() {
        // The source keys by breaker id, so two breakers on one block share
        // one `DestroyBlockProgress` only while the position matches
        // (`RenderGlobal.java:2366-2372`); this client's map is keyed by
        // position until M4, so any update lands on the one entry.
        let mut stages = BreakStages::new();
        assert!(stages.set(0, 64, 0, 2), "the first update");
        assert!(stages.set(0, 64, 0, 7), "the second changed the stage");
        assert_eq!(stages.len(), 1, "one entry for the position");
        assert_eq!(stages.stage(0, 64, 0), Some(7), "the latest stage");
    }
}
