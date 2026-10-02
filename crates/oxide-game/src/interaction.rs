//! The block raycast and the aimed-block state.
//!
//! The interaction ray is the reference client's mouse-over: the frame path
//! takes the block reach from the controller, runs the look vector from the
//! pose eye and hands the segment to the world's own trace
//! (`EntityRenderer.getMouseOver`, `client/renderer/EntityRenderer.java:409-420`;
//! `Entity.rayTrace`, `entity/Entity.java:1500-1506`:
//! `this.worldObj.rayTraceBlocks(vec3, vec32, false, false, true)`).
//! [`raycast`] walks that trace: the segment's cells in order, stopping at the
//! first collision box that meets it, and answers the block, the face the ray
//! entered through and the point on that face ([`Aim`]).
//!
//! The predicate over blocks is the collision shapes the behaviour table
//! carries: a cell stops the ray when one of its collision boxes meets it.
//! Liquids and the cross plants answer no box (`block/BlockLiquid.java:121-124`,
//! `block/BlockBush.java:76-79`) and do not stop the ray; a solid block does.
//! The source's own walk stops on any block whose `canCollideCheck` is true and
//! traces its *selection* bounds (`block/Block.java:512-515`, `:681-794`), which
//! the cross plants carry too (`BlockBush.java:31`) — this client's
//! interaction ray is defined over the collision shapes, the surface the aim,
//! the outline and the break and place paths consume.
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
    if gamemode & !HARDCORE_BIT == CREATIVE_ID {
        CREATIVE_REACH
    } else {
        SURVIVAL_REACH
    }
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
/// enters, up to the source's two-hundred step cap. A cell stops the ray when
/// one of its collision boxes meets the segment; the answer is the box's
/// nearest entry, with the face the ray crosses there and the point on it
/// (`Block.collisionRayTrace`'s own faces, `block/Block.java:763-791`). A
/// non-finite eye or end answers `None`, where the source refuses a NaN one
/// (`:890-893`).
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

/// The aim one cell answers for the segment `from..to`, when one of its
/// collision boxes meets it: the box's nearest entry, on the cell's own block.
fn cell_aim(
    view: &WorldView<'_>,
    boxes: &mut Vec<CollisionBox>,
    from: [f64; 3],
    to: [f64; 3],
    cell: [i32; 3],
) -> Option<Aim> {
    boxes.clear();
    view.collision_boxes(cell[0], cell[1], cell[2], boxes);
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

#[cfg(test)]
mod tests {
    //! The raycast against synthetic worlds, built through the light tests'
    //! column fixture path (`oxide-world/tests/light.rs`): one column of
    //! sixteen sections, applied at chunk (0, 0).

    use oxide_proto_v47::column::{ColumnData, SectionData, block_index};
    use oxide_world::chunk::{SECTION_COUNT, SECTION_SIZE};
    use oxide_world::world::World;

    use super::{Aim, CREATIVE_REACH, Face, SURVIVAL_REACH, look_vector, raycast, reach};
    use crate::player::Player;
    use crate::world_view::WorldView;

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
    /// The double stone slab, id 43 — the one covered slab id, whose collision
    /// box is the full cell (`CollisionShape::Slab { double: true }`).
    const SLAB: u16 = 43 << 4;
    /// A lone fence, id 85: its post is 0.375..0.625 across and 1.5 tall
    /// (`BlockFence.java:50-107`).
    const FENCE: u16 = 85 << 4;

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
    fn plants_and_liquids_do_not_stop_the_ray_and_solids_do() {
        // Tall grass, water and lava in the ray's path, with stone behind
        // them: the ray passes through the three and meets the stone.
        let layered = world_of(|x, y, z| match (x, y, z) {
            (0, 65, 2) => TALL_GRASS,
            (0, 65, 3) => WATER,
            (0, 65, 4) => LAVA,
            (0, 65, 5) => STONE,
            _ => AIR,
        });
        let aim = aim_in(&layered, [0.5, 64.0, 0.5], 0.0, 0.0, SURVIVAL_REACH)
            .expect("the stone behind them is hit");
        assert_eq!(
            (aim.x, aim.y, aim.z, aim.face),
            (0, 65, 5, Face::North),
            "the ray reached the stone"
        );

        // With no solid behind them the ray meets nothing at all.
        let soft = world_of(|x, y, z| match (x, y, z) {
            (0, 65, 2) => TALL_GRASS,
            (0, 65, 3) => WATER,
            (0, 65, 4) => LAVA,
            _ => AIR,
        });
        assert_eq!(
            aim_in(&soft, [0.5, 64.0, 0.5], 0.0, 0.0, SURVIVAL_REACH),
            None,
            "the grass, the water and the lava do not stop the ray"
        );

        // A single solid block does stop it: the same cell as the grass, as
        // stone.
        let solid = world_of(|x, y, z| if (x, y, z) == (0, 65, 2) { STONE } else { AIR });
        let aim = aim_in(&solid, [0.5, 64.0, 0.5], 0.0, 0.0, SURVIVAL_REACH)
            .expect("the stone stops the ray");
        assert_eq!((aim.x, aim.y, aim.z, aim.face), (0, 65, 2, Face::North));
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
}
