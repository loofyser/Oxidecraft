//! The world through the movement model's eyes: [`WorldView`] adapts the
//! chunk store to `physics::CollisionView`.
//!
//! Every answer goes through the behaviour table: the cell's packed block
//! value selects a row, and the row's movement columns are what the movement
//! rules read. The classes whose shapes depend on their neighbours resolve
//! them here, through predicates derived from the source's own connection
//! rules:
//!
//! * a fence reads `BlockFence.canConnectTo` (`BlockFence.java:161-165`),
//! * a cobblestone wall `BlockWall.canConnectTo` (`BlockWall.java:122-126`),
//! * a pane `BlockPane.canPaneConnectToBlock` (`BlockPane.java:177-180`),
//! * a stair `BlockStairs.isSameStair` (`BlockStairs.java:103-108`),
//! * a chest `BlockChest.setBlockBoundsBasedOnState`'s same-block test
//!   (`BlockChest.java:66-88`),
//! * a door `BlockDoor.combineMetadata` (`BlockDoor.java:295-308`).
//!
//! A cell outside the covered set — air included — has no row: it answers the
//! movement defaults (no box, no fluid, no climb, the `0.6` slipperiness),
//! the same magenta-and-pass treatment the mesher gives it.

use oxide_world::behaviour::{BlockBehaviour, CollisionShape, LiquidKind, Material, behaviour};
use oxide_world::collision::{
    CollisionBox, Connections, Facing, StairState, cactus_box, chest_box, door_box, door_facing,
    fence_boxes, front_facing, ladder_box, pane_boxes, slab_boxes, slab_half, snow_layer_box,
    soul_sand_box, stair_facing, stair_half, stairs_boxes, wall_box,
};
use oxide_world::world::World;

use crate::physics::{CollisionView, Fluid, FluidKind};

/// The slipperiness of a cell with no row: `Block.slipperiness`' `0.6F`
/// default (`block/Block.java:291`), which air carries.
const DEFAULT_SLIPPERINESS: f32 = 0.6;

/// The world as the movement model reads it.
///
/// The lifetime borrows the store for one tick's worth of queries; the cell
/// reads go through [`World::block`], so an unloaded column answers air.
pub struct WorldView<'a>(
    /// The chunk store the queries read.
    pub &'a World,
);

impl CollisionView for WorldView<'_> {
    fn slipperiness(&self, x: i32, y: i32, z: i32) -> f32 {
        row_of(self.0, x, y, z).map_or(DEFAULT_SLIPPERINESS, |row| row.slipperiness)
    }

    fn collision_boxes(&self, x: i32, y: i32, z: i32, out: &mut Vec<CollisionBox>) {
        let Some(row) = row_of(self.0, x, y, z) else {
            return;
        };
        let meta = meta_of(self.0, x, y, z);
        let (ox, oy, oz) = (f64::from(x), f64::from(y), f64::from(z));
        match row.collision {
            CollisionShape::None => {}
            CollisionShape::Full => out.push(CollisionBox::full().offset(ox, oy, oz)),
            CollisionShape::Slab { double } => {
                out.push(slab_boxes(double, slab_half(meta))[0].offset(ox, oy, oz));
            }
            CollisionShape::Stairs => {
                let boxes = stairs_boxes(stair_facing(meta), stair_half(meta), |dir| {
                    self.stair_state(x, y, z, dir)
                });
                for box_ in boxes {
                    out.push(box_.offset(ox, oy, oz));
                }
            }
            CollisionShape::SnowLayers => {
                // `LAYERS` is `(meta & 7) + 1` (`BlockSnow.java:153`).
                let layers = (meta & 7) + 1;
                out.push(snow_layer_box(layers).offset(ox, oy, oz));
            }
            CollisionShape::Cactus => out.push(cactus_box().offset(ox, oy, oz)),
            CollisionShape::Fence => {
                let connections = Connections {
                    north: self.neighbour_connects_fence(row, x, y, z - 1),
                    south: self.neighbour_connects_fence(row, x, y, z + 1),
                    west: self.neighbour_connects_fence(row, x - 1, y, z),
                    east: self.neighbour_connects_fence(row, x + 1, y, z),
                };
                for box_ in fence_boxes(connections) {
                    out.push(box_.offset(ox, oy, oz));
                }
            }
            CollisionShape::Wall => {
                let connections = Connections {
                    north: self.neighbour_connects_wall(x, y, z - 1),
                    south: self.neighbour_connects_wall(x, y, z + 1),
                    west: self.neighbour_connects_wall(x - 1, y, z),
                    east: self.neighbour_connects_wall(x + 1, y, z),
                };
                out.push(wall_box(connections).offset(ox, oy, oz));
            }
            CollisionShape::Pane => {
                let connections = Connections {
                    north: self.neighbour_connects_pane(x, y, z - 1),
                    south: self.neighbour_connects_pane(x, y, z + 1),
                    west: self.neighbour_connects_pane(x - 1, y, z),
                    east: self.neighbour_connects_pane(x + 1, y, z),
                };
                for box_ in pane_boxes(connections) {
                    out.push(box_.offset(ox, oy, oz));
                }
            }
            CollisionShape::Chest => {
                let same = |dx: i32, dz: i32| {
                    row_of(self.0, x + dx, y, z + dz).is_some_and(|other| other.id == row.id)
                };
                out.push(
                    chest_box(same(0, -1), same(0, 1), same(-1, 0), same(1, 0)).offset(ox, oy, oz),
                );
            }
            CollisionShape::Door => {
                let (facing, open, hinge_left) = self.door_state(x, y, z, meta);
                out.push(door_box(facing, open, hinge_left).offset(ox, oy, oz));
            }
            CollisionShape::Ladder => {
                out.push(ladder_box(front_facing(meta)).offset(ox, oy, oz));
            }
            CollisionShape::SoulSand => out.push(soul_sand_box().offset(ox, oy, oz)),
        }
    }

    fn fluid(&self, x: i32, y: i32, z: i32) -> Option<Fluid> {
        let row = row_of(self.0, x, y, z)?;
        let kind = match row.liquid? {
            LiquidKind::Water => FluidKind::Water,
            LiquidKind::Lava => FluidKind::Lava,
        };
        Some(Fluid {
            kind,
            level: meta_of(self.0, x, y, z),
        })
    }

    fn climbable(&self, x: i32, y: i32, z: i32) -> bool {
        row_of(self.0, x, y, z).is_some_and(|row| row.climbable)
    }
}

impl WorldView<'_> {
    /// The stair state one step in a direction, or `None` when that cell is
    /// not a stair: what the source's `func_176306_h`/`func_176304_i` read
    /// through `isSameStair` (`BlockStairs.java:103-108`).
    fn stair_state(&self, x: i32, y: i32, z: i32, dir: Facing) -> Option<StairState> {
        let (dx, dz) = dir.offset();
        let other = row_of(self.0, x + dx, y, z + dz)?;
        if !matches!(other.collision, CollisionShape::Stairs) {
            return None;
        }
        let meta = meta_of(self.0, x + dx, y, z + dz);
        Some(StairState {
            facing: stair_facing(meta),
            half: stair_half(meta),
        })
    }

    /// The door's state, from the combined metadata `BlockDoor.combineMetadata`
    /// builds (`BlockDoor.java:295-308`): the lower half's nibble, whether the
    /// queried cell is the upper half, and the upper half's hinge and powered
    /// bits. The facing is `getFacing(combined & 3)` (`:421-424`), the open
    /// flag bit 2 (`:426-429`) and the hinge flag bit 4 (`:436-439`), which
    /// `combineMetadata` fills from the upper half's `HINGE == RIGHT`
    /// (`:383-386`) — the "left" of `isHingeLeft` is the source's own naming.
    fn door_state(&self, x: i32, y: i32, z: i32, meta: u8) -> (Facing, bool, bool) {
        let top = meta & 8 != 0;
        let lower = if top {
            meta_of(self.0, x, y - 1, z)
        } else {
            meta
        };
        let upper = if top {
            meta
        } else {
            meta_of(self.0, x, y + 1, z)
        };
        let combined = (lower & 7)
            | if top { 8 } else { 0 }
            | if upper & 1 != 0 { 16 } else { 0 }
            | if upper & 2 != 0 { 32 } else { 0 };
        (door_facing(combined), combined & 4 != 0, combined & 16 != 0)
    }

    /// `BlockFence.canConnectTo` (`BlockFence.java:161-165`) for the fence at
    /// a neighbour cell: a fence of the same material, or an opaque full-cube
    /// block other than the gourd. The source's barrier and fence-gate clauses
    /// (`:163-164`) cover ids outside the covered set, which answer `false`.
    fn neighbour_connects_fence(&self, row: &BlockBehaviour, x: i32, y: i32, z: i32) -> bool {
        let Some(other) = row_of(self.0, x, y, z) else {
            return false;
        };
        let same_material_fence =
            matches!(other.collision, CollisionShape::Fence) && other.material == row.material;
        same_material_fence
            || (other.material.is_opaque() && other.full_cube && other.material != Material::Gourd)
    }

    /// `BlockWall.canConnectTo` (`BlockWall.java:122-126`): the wall itself,
    /// or an opaque full-cube block other than the gourd. The barrier and
    /// fence-gate clauses cover ids outside the covered set (the barrier
    /// answering `false` here as it does there, the gate `true`).
    fn neighbour_connects_wall(&self, x: i32, y: i32, z: i32) -> bool {
        let Some(other) = row_of(self.0, x, y, z) else {
            return false;
        };
        matches!(other.collision, CollisionShape::Wall)
            || (other.material.is_opaque() && other.full_cube && other.material != Material::Gourd)
    }

    /// `BlockPane.canPaneConnectToBlock` (`BlockPane.java:177-180`): a full
    /// block, a pane, the glass block. The two stained ids the source also
    /// names are outside the covered set.
    fn neighbour_connects_pane(&self, x: i32, y: i32, z: i32) -> bool {
        let Some(other) = row_of(self.0, x, y, z) else {
            return false;
        };
        other.is_full_block() || matches!(other.collision, CollisionShape::Pane) || other.id == 20
    }
}

/// The behaviour row of a cell, or `None` when its id is outside the covered
/// set (air included).
fn row_of(world: &World, x: i32, y: i32, z: i32) -> Option<&'static BlockBehaviour> {
    behaviour(world.block(x, y, z) >> 4)
}

/// The metadata nibble of a cell.
fn meta_of(world: &World, x: i32, y: i32, z: i32) -> u8 {
    (world.block(x, y, z) & 0xF) as u8
}

#[cfg(test)]
mod tests {
    //! The adapter against real worlds, built through the light tests' column
    //! fixture path (`oxide-world/tests/light.rs`): one column of sixteen
    //! sections, applied at chunk (0, 0).

    use oxide_proto_v47::column::{ColumnData, SectionData, block_index};
    use oxide_world::behaviour::liquid_height_percent;
    use oxide_world::chunk::{SECTION_COUNT, SECTION_SIZE};
    use oxide_world::collision::CollisionBox;
    use oxide_world::world::World;

    use super::WorldView;
    use crate::physics::{CollisionView, Fluid, FluidKind};

    /// Air.
    const AIR: u16 = 0;
    /// Stone, id 1.
    const STONE: u16 = 1 << 4;
    /// An oak log, id 17: opaque, a full cube, wood.
    const LOG: u16 = 17 << 4;
    /// A pumpkin, id 86: opaque and a full cube, but the gourd material.
    const PUMPKIN: u16 = 86 << 4;
    /// A fence, id 85.
    const FENCE: u16 = 85 << 4;
    /// A glass pane, id 102.
    const PANE: u16 = 102 << 4;
    /// The glass block, id 20.
    const GLASS: u16 = 20 << 4;
    /// Ice, id 79: the `0.98` slipperiness.
    const ICE: u16 = 79 << 4;
    /// A ladder, id 65, facing north (metadata 2).
    const LADDER_NORTH: u16 = 65 << 4 | 2;
    /// A ladder facing south (metadata 3): the plate on the cell's north edge.
    const LADDER_SOUTH: u16 = 65 << 4 | 3;
    /// An oak stair, id 53, facing east, bottom half (metadata 0).
    const STAIR_EAST: u16 = 53 << 4;
    /// An oak stair facing north, bottom half (metadata 3).
    const STAIR_NORTH: u16 = 53 << 4 | 3;
    /// An oak stair facing east, top half (metadata 4).
    const STAIR_TOP_EAST: u16 = 53 << 4 | 4;
    /// An oak stair facing north, top half (metadata 4 | 3).
    const STAIR_TOP_NORTH: u16 = 53 << 4 | 7;
    /// A chest, id 54.
    const CHEST: u16 = 54 << 4;
    /// A wooden door, id 64, lower half: facing east, closed (metadata 0),
    /// and its upper half with the hinge bit set (metadata 8 | 1).
    const DOOR_LOWER_CLOSED: u16 = 64 << 4;
    /// A wooden door: lower half open (metadata 4) under a hinged upper half.
    const DOOR_LOWER_OPEN: u16 = 64 << 4 | 4;
    /// The door's upper half with `HINGE == RIGHT` (metadata 8 | 1).
    const DOOR_UPPER_HINGE: u16 = 64 << 4 | 8 | 1;
    /// Water, id 9, with the given metadata.
    const fn water(meta: u16) -> u16 {
        9 << 4 | meta
    }
    /// Lava, id 11.
    const LAVA: u16 = 11 << 4;

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

    /// The boxes the view reports for one cell.
    fn boxes_at(world: &World, x: i32, y: i32, z: i32) -> Vec<CollisionBox> {
        let mut boxes = Vec::new();
        WorldView(world).collision_boxes(x, y, z, &mut boxes);
        boxes
    }

    #[test]
    fn a_solid_block_answers_the_full_cube_and_the_source_defaults() {
        let world = world_of(|_, y, _| if y == 64 { STONE } else { AIR });
        let view = WorldView(&world);
        assert_eq!(
            boxes_at(&world, 0, 64, 0),
            vec![CollisionBox::full().offset(0.0, 64.0, 0.0)]
        );
        assert_eq!(view.slipperiness(0, 64, 0), 0.6);
        assert!(!view.climbable(0, 64, 0));
        assert_eq!(view.fluid(0, 64, 0), None);
    }

    #[test]
    fn air_answers_nothing() {
        let world = world_of(|_, _, _| AIR);
        let view = WorldView(&world);
        assert_eq!(boxes_at(&world, 0, 64, 0), vec![]);
        assert_eq!(view.slipperiness(0, 64, 0), 0.6);
        assert!(!view.climbable(0, 64, 0));
        assert_eq!(view.fluid(0, 64, 0), None);
        // An unloaded column answers air too.
        assert_eq!(boxes_at(&world, 400, 64, 400), vec![]);
    }

    #[test]
    fn ice_carries_the_source_slipperiness() {
        let world = world_of(|_, y, _| if y == 64 { ICE } else { AIR });
        assert_eq!(WorldView(&world).slipperiness(0, 64, 0), 0.98);
    }

    #[test]
    fn a_liquid_reports_its_kind_and_level_and_the_level_derives_the_height() {
        let world = world_of(|_, y, _| match y {
            60 => water(0),
            61 => water(5),
            62 => water(12),
            63 => LAVA,
            _ => AIR,
        });
        let view = WorldView(&world);
        assert_eq!(
            view.fluid(0, 60, 0),
            Some(Fluid {
                kind: FluidKind::Water,
                level: 0
            })
        );
        assert_eq!(
            view.fluid(0, 63, 0),
            Some(Fluid {
                kind: FluidKind::Lava,
                level: 0
            })
        );
        // The level the view reads is the metadata nibble, and the source's
        // height rule reads it: `1 - (level + 1) / 9` for the flow levels and
        // the source's own height for a falling column of 8 or more
        // (`BlockLiquid.getLiquidHeightPercent`, `block/BlockLiquid.java:48-56`).
        let level = view.fluid(0, 61, 0).expect("water").level;
        assert_eq!(level, 5);
        assert_eq!(liquid_height_percent(level), 1.0 - 6.0 / 9.0);
        assert_eq!(liquid_height_percent(0), 1.0 - 1.0 / 9.0);
        let falling = view.fluid(0, 62, 0).expect("water").level;
        assert_eq!(falling, 12);
        assert_eq!(liquid_height_percent(falling), liquid_height_percent(0));
    }

    #[test]
    fn a_lone_fence_post_is_one_box() {
        let world = world_of(|x, y, z| if (x, y, z) == (8, 64, 8) { FENCE } else { AIR });
        assert_eq!(
            boxes_at(&world, 8, 64, 8),
            vec![CollisionBox::of([8.375, 64.0, 8.375], [8.625, 65.5, 8.625])]
        );
    }

    #[test]
    fn a_fence_junction_resolves_the_post_and_its_arms() {
        // The queried fence at (8, 64, 8) — the middle of the loaded column, so
        // its neighbours are loaded too: fences north and south of it, stone
        // east. The post spans north–south and the west–east arm reaches into
        // the stone (`BlockFence.java:60-95`).
        let world = world_of(|x, y, z| {
            if y != 64 {
                return AIR;
            }
            match (x, z) {
                (8, 8) | (8, 7) | (8, 9) => FENCE,
                (9, 8) => STONE,
                _ => AIR,
            }
        });
        assert_eq!(
            boxes_at(&world, 8, 64, 8),
            vec![
                CollisionBox::of([8.375, 64.0, 8.0], [8.625, 65.5, 9.0]),
                CollisionBox::of([8.375, 64.0, 8.375], [9.0, 65.5, 8.625]),
            ]
        );
    }

    #[test]
    fn a_pane_reaches_toward_a_full_neighbour() {
        // A pane with stone east: the west–east plate reaches the east half,
        // and the north–south plate pushes nothing (`BlockPane.java:82-118`).
        let world = world_of(|x, y, z| {
            if y != 64 {
                return AIR;
            }
            match (x, z) {
                (8, 8) => PANE,
                (9, 8) => STONE,
                _ => AIR,
            }
        });
        assert_eq!(
            boxes_at(&world, 8, 64, 8),
            vec![CollisionBox::of([8.5, 64.0, 8.4375], [9.0, 65.0, 8.5625])]
        );
    }

    #[test]
    fn a_ladder_is_a_plate_and_climbs() {
        let world = world_of(|x, y, z| {
            if (x, y, z) == (8, 64, 8) {
                LADDER_NORTH
            } else {
                AIR
            }
        });
        let view = WorldView(&world);
        assert!(view.climbable(8, 64, 8));
        assert_eq!(
            boxes_at(&world, 8, 64, 8),
            vec![CollisionBox::of([8.0, 64.0, 8.875], [9.0, 65.0, 9.0])]
        );
    }

    #[test]
    fn a_south_ladder_is_a_plate_on_its_attached_face() {
        // Metadata 3 is the south facing (`BlockLadder.java:140-150`):
        // `EnumFacing.getFront(3)` is south, so the plate lies on the cell's
        // north edge (`BlockLadder.java:40-66`).
        let world = world_of(|x, y, z| {
            if (x, y, z) == (8, 64, 8) {
                LADDER_SOUTH
            } else {
                AIR
            }
        });
        assert_eq!(
            boxes_at(&world, 8, 64, 8),
            vec![CollisionBox::of([8.0, 64.0, 8.0], [9.0, 65.0, 8.125])]
        );
    }

    #[test]
    fn a_stair_reads_its_neighbours_step_and_corner() {
        // The queried bottom stair faces east; its west neighbour is a bottom
        // stair facing north, which opens the outer corner box
        // (`BlockStairs.java:429-450`).
        let world = world_of(|x, y, z| {
            if y != 64 {
                return AIR;
            }
            match (x, z) {
                (8, 8) => STAIR_EAST,
                (7, 8) => STAIR_NORTH,
                _ => AIR,
            }
        });
        assert_eq!(
            boxes_at(&world, 8, 64, 8),
            vec![
                CollisionBox::of([8.0, 64.0, 8.0], [9.0, 64.5, 9.0]),
                CollisionBox::of([8.5, 64.5, 8.0], [9.0, 65.0, 9.0]),
                CollisionBox::of([8.0, 64.5, 8.0], [8.5, 65.0, 8.5]),
            ]
        );
    }

    #[test]
    fn a_top_stair_is_its_top_half_and_its_low_step() {
        // Metadata 4 is the top half (`BlockStairs.java:728`): the base box
        // is the upper half and the step the lower one, facing east.
        let world = world_of(|x, y, z| {
            if (x, y, z) == (8, 64, 8) {
                STAIR_TOP_EAST
            } else {
                AIR
            }
        });
        assert_eq!(
            boxes_at(&world, 8, 64, 8),
            vec![
                CollisionBox::of([8.0, 64.5, 8.0], [9.0, 65.0, 9.0]),
                CollisionBox::of([8.5, 64.0, 8.0], [9.0, 64.5, 9.0]),
            ]
        );
    }

    #[test]
    fn a_top_stair_reads_a_top_neighbours_turn() {
        // The queried top stair faces east; its west neighbour is a top stair
        // facing north — same half, turning — so the outer corner box opens
        // with the top half's own y span (`func_176304_i`'s east arm,
        // `BlockStairs.java:429-450`).
        let world = world_of(|x, y, z| {
            if y != 64 {
                return AIR;
            }
            match (x, z) {
                (8, 8) => STAIR_TOP_EAST,
                (7, 8) => STAIR_TOP_NORTH,
                _ => AIR,
            }
        });
        assert_eq!(
            boxes_at(&world, 8, 64, 8),
            vec![
                CollisionBox::of([8.0, 64.5, 8.0], [9.0, 65.0, 9.0]),
                CollisionBox::of([8.5, 64.0, 8.0], [9.0, 64.5, 9.0]),
                CollisionBox::of([8.0, 64.0, 8.0], [8.5, 64.5, 8.5]),
            ]
        );
    }

    #[test]
    fn a_chest_merges_toward_its_twin() {
        // A chest with another chest south of it reaches the cell's edge
        // (`BlockChest.java:66-88`).
        let world = world_of(|x, y, z| {
            if y != 64 {
                return AIR;
            }
            match (x, z) {
                (8, 8) | (8, 9) => CHEST,
                _ => AIR,
            }
        });
        assert_eq!(
            boxes_at(&world, 8, 64, 8),
            vec![CollisionBox::of(
                [8.0625, 64.0, 8.0625],
                [8.9375, 64.875, 9.0]
            )]
        );
    }

    #[test]
    fn a_door_reads_both_halves_and_swings_on_its_hinge() {
        // Closed, facing east: the 3/16 plate on the east side, whatever the
        // hinge (`BlockDoor.java:138-153`).
        let closed = world_of(|x, y, z| {
            if x != 8 || z != 8 {
                return AIR;
            }
            match y {
                64 => DOOR_LOWER_CLOSED,
                65 => DOOR_UPPER_HINGE,
                _ => AIR,
            }
        });
        assert_eq!(
            boxes_at(&closed, 8, 64, 8),
            vec![CollisionBox::of([8.0, 64.0, 8.0], [8.1875, 65.0, 9.0])]
        );
        // Open with the hinge bit set: the plate swings onto the north–south
        // axis, to the hinge's side (`:91-136`).
        let open = world_of(|x, y, z| {
            if x != 8 || z != 8 {
                return AIR;
            }
            match y {
                64 => DOOR_LOWER_OPEN,
                65 => DOOR_UPPER_HINGE,
                _ => AIR,
            }
        });
        assert_eq!(
            boxes_at(&open, 8, 64, 8),
            vec![CollisionBox::of([8.0, 64.0, 8.8125], [9.0, 65.0, 9.0])]
        );
    }

    #[test]
    fn the_neighbour_predicates_follow_the_sources_can_connect_tos() {
        // One probe cell per neighbour value the predicates read, all in the
        // loaded column at y 64, z 8.
        let world = world_of(|x, y, z| {
            if y != 64 || z != 8 {
                return AIR;
            }
            match x {
                8 => STONE,
                9 => LOG,
                10 => PUMPKIN,
                11 => FENCE,
                12 => PANE,
                13 => GLASS,
                _ => AIR,
            }
        });
        let view = WorldView(&world);
        let fence = oxide_world::behaviour::behaviour(85).expect("fence");
        // Fence: a same-material fence and an opaque full cube connect; the
        // gourd, the pane and air do not (`BlockFence.java:161-165`).
        assert!(view.neighbour_connects_fence(fence, 8, 64, 8));
        assert!(view.neighbour_connects_fence(fence, 9, 64, 8));
        assert!(!view.neighbour_connects_fence(fence, 10, 64, 8));
        assert!(view.neighbour_connects_fence(fence, 11, 64, 8));
        assert!(!view.neighbour_connects_fence(fence, 12, 64, 8));
        assert!(!view.neighbour_connects_fence(fence, 14, 64, 8));
        // Wall: the same rules; nothing covered is a wall yet
        // (`BlockWall.java:124-125`).
        assert!(view.neighbour_connects_wall(8, 64, 8));
        assert!(view.neighbour_connects_wall(9, 64, 8));
        assert!(!view.neighbour_connects_wall(10, 64, 8));
        assert!(!view.neighbour_connects_wall(11, 64, 8));
        assert!(!view.neighbour_connects_wall(14, 64, 8));
        // Pane: full blocks, panes and glass connect; the fence does not
        // (`BlockPane.java:177-180`).
        assert!(view.neighbour_connects_pane(8, 64, 8));
        assert!(!view.neighbour_connects_pane(11, 64, 8));
        assert!(view.neighbour_connects_pane(12, 64, 8));
        assert!(view.neighbour_connects_pane(13, 64, 8));
        assert!(!view.neighbour_connects_pane(14, 64, 8));
    }
}
