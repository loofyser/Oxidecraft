//! The column snapshot: the mesher's world-free input.
//!
//! The mesh core never touches the world. Everything it reads comes from a
//! [`ColumnSnapshot`]: the column's own blocks, light and biomes copied out of
//! the store, plus a one-cell collar — the four [`Border`] strips — holding the
//! blocks, light and biomes of the neighbouring cells through each of the
//! column's four faces.
//!
//! The copy happens on the thread that owns the store; the snapshot owns its
//! bytes, so it is `Send` and a worker can build quads from it without a lock
//! and without the store outliving the mesh.
//!
//! Coordinates are local to the column: `x` and `z` are 0..16 for the column
//! itself and -1 or 16 for the collar one cell out, `y` is 0..256 over the
//! whole world height. Reads outside those bounds — `y` past 255, or `x`/`z`
//! past the collar — are not an error: they answer air and both light kinds 0,
//! which is what the mesher needs for the faces of a block at the world's top
//! or bottom.

use oxide_world::chunk::{SECTION_COUNT, SECTION_SIZE, Section};
use oxide_world::world::World;

/// Cells along one side of a border strip: the column's 16, plus one at each
/// end of the strip's across axis.
const BORDER_SIDE: i32 = SECTION_SIZE as i32 + 2;

/// Cells in one border strip: 18 x 18.
const BORDER_ROW: usize = (BORDER_SIDE * BORDER_SIDE) as usize;

/// Blocks in one border strip: every y of the 18 x 18 row.
const BORDER_BLOCKS: usize = BORDER_ROW * SECTION_COUNT * SECTION_SIZE;

/// Blocks in the column's own store: 16 x 16 x 256.
const COLUMN_BLOCKS: usize = SECTION_SIZE * SECTION_SIZE * SECTION_COUNT * SECTION_SIZE;

/// The four sides of the collar, in the order [`ColumnSnapshot::borders`]
/// indexes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    /// The neighbour through `-z` (north).
    North = 0,
    /// The neighbour through `+z` (south).
    South = 1,
    /// The neighbour through `-x` (west).
    West = 2,
    /// The neighbour through `+x` (east).
    East = 3,
}

/// One side of the collar: the blocks and light of the 18 x 18 cells that ring
/// the column on that side, and the biome of the 16 cells along the shared
/// edge.
///
/// The strip's across axis runs -1..=16 the way the column's own axis does — a
/// north border's cells are `z = -1`, `x = -1..=16` — and its along axis is the
/// world height, `y = 0..256`.
#[derive(Debug, Clone)]
pub struct Border {
    /// Packed blocks, `(y << 9) | (across + 1) * 18 + (along + 1)`.
    blocks: Vec<u16>,
    /// Block light, one byte a cell.
    block_light: Vec<u8>,
    /// Sky light, one byte a cell.
    sky_light: Vec<u8>,
    /// The biomes along the shared edge, indexed by the strip's along axis
    /// (`x` for the north and south borders, `z` for the west and east ones).
    biomes: [u8; SECTION_SIZE],
}

/// The linear index of a border cell: the strip's across axis varies fastest,
/// then the along axis, then `y`.
fn border_index(across: i32, along: i32, y: i32) -> usize {
    y as usize * BORDER_ROW + ((across + 1) as usize * BORDER_SIDE as usize + (along + 1) as usize)
}

/// The linear index of a column cell: `x` varies fastest, then `z`, then `y`.
fn column_index(x: i32, z: i32, y: i32) -> usize {
    (((y as usize) << 8) | ((z as usize) << 4)) | x as usize
}

/// One column's blocks, light and biomes, and the collar around it.
///
/// [`ColumnSnapshot::from_world`] is its only constructor: it copies the whole
/// column and the four strips out of the store, so the snapshot reproduces
/// every read the mesher makes of the world.
#[derive(Debug, Clone)]
pub struct ColumnSnapshot {
    /// The column's chunk x.
    pub cx: i32,
    /// The column's chunk z.
    pub cz: i32,
    /// Whether the dimension carries sky light, as the store was built.
    pub has_sky: bool,
    /// The biome ids of the column's 16 x 16 cells, indexed `z * 16 + x`: the
    /// store's own `Chunk::biome` array.
    pub biome: [u8; SECTION_SIZE * SECTION_SIZE],
    /// The column's packed blocks, `(id << 4) | meta`.
    blocks: Vec<u16>,
    /// The column's block-light levels, one byte a cell.
    block_light: Vec<u8>,
    /// The column's sky-light levels, one byte a cell.
    sky_light: Vec<u8>,
    /// The collar, indexed by [`Side`].
    borders: [Border; 4],
}

impl ColumnSnapshot {
    /// Copies the column at `(cx, cz)` and its collar out of `world`.
    ///
    /// An unloaded — or loaded but empty — column yields an all-air snapshot
    /// whose light is dark and whose biomes are 0, so a session still reports
    /// sections for an unloaded neighbour and the mesher still runs. A section
    /// the column does not hold reads as air and as the store's own light
    /// default for an absent section: sky light 15 in a dimension with sky,
    /// block light 0.
    ///
    /// A collar cell that no loaded column covers answers air and both light
    /// kinds 0, and the biome of the strip's nearest edge cell — the collar's
    /// four diagonal corners have no column of their own to read from, and no
    /// face's tint looks through them in ordinary meshing.
    pub fn from_world(world: &World, cx: i32, cz: i32) -> ColumnSnapshot {
        let has_sky = world.has_sky();
        let mut snapshot = ColumnSnapshot {
            cx,
            cz,
            has_sky,
            biome: [0; SECTION_SIZE * SECTION_SIZE],
            blocks: vec![0; COLUMN_BLOCKS],
            block_light: vec![0; COLUMN_BLOCKS],
            sky_light: vec![0; COLUMN_BLOCKS],
            borders: std::array::from_fn(|_| Border {
                blocks: vec![0; BORDER_BLOCKS],
                block_light: vec![0; BORDER_BLOCKS],
                sky_light: vec![0; BORDER_BLOCKS],
                biomes: [0; SECTION_SIZE],
            }),
        };

        if let Some(chunk) = world.chunk(cx, cz) {
            for z in 0..SECTION_SIZE as i32 {
                for x in 0..SECTION_SIZE as i32 {
                    snapshot.biome[(z * SECTION_SIZE as i32 + x) as usize] =
                        chunk.biome(x as usize, z as usize);
                }
            }
            for sy in 0..SECTION_COUNT {
                let base = sy * SECTION_SIZE;
                match chunk.section(sy) {
                    Some(section) => copy_section(&mut snapshot, base, section),
                    None if has_sky => {
                        // The store's own default for an absent section: sky 15,
                        // block 0 (`Chunk::sky_light_at`).
                        for y in base..base + SECTION_SIZE {
                            for z in 0..SECTION_SIZE as i32 {
                                for x in 0..SECTION_SIZE as i32 {
                                    let index = column_index(x, z, y as i32);
                                    snapshot.sky_light[index] = 15;
                                }
                            }
                        }
                    }
                    None => {}
                }
            }
        }

        for side in [Side::North, Side::South, Side::West, Side::East] {
            snapshot.copy_border(world, side);
        }
        snapshot
    }

    /// The block at a column-local position.
    ///
    /// `x` and `z` are 0..16 for the column itself and may reach one cell out
    /// into the collar; `y` is 0..256. Anything outside answers air.
    pub fn block(&self, x: i32, y: i32, z: i32) -> u16 {
        if !in_collar(x, z) || !in_height(y) {
            return 0;
        }
        if in_column(x, z) {
            return self.blocks[column_index(x, z, y)];
        }
        let side = side_of(x, z);
        let border = &self.borders[side as usize];
        let (across, along) = across_along(side, x, z);
        border.blocks[border_index(across, along, y)]
    }

    /// The light at a column-local position, as `(sky, block)` levels.
    ///
    /// The order matches the packed pair a vertex carries: sky first, then
    /// block. `x` and `z` may reach one cell out into the collar; `y` is
    /// 0..256. Anything outside answers `(0, 0)`.
    pub fn light(&self, x: i32, y: i32, z: i32) -> (u8, u8) {
        if !in_collar(x, z) || !in_height(y) {
            return (0, 0);
        }
        if in_column(x, z) {
            let index = column_index(x, z, y);
            return (self.sky_light[index], self.block_light[index]);
        }
        let side = side_of(x, z);
        let border = &self.borders[side as usize];
        let (across, along) = across_along(side, x, z);
        let index = border_index(across, along, y);
        (border.sky_light[index], border.block_light[index])
    }

    /// The biome id at a column-local position: `x` and `z` are 0..16 for the
    /// column's own cells and may reach one cell into the collar.
    ///
    /// Anything outside the collar answers 0, the id an unloaded column's
    /// fresh biome array holds.
    pub fn biome_at(&self, x: i32, z: i32) -> u8 {
        if in_column(x, z) {
            return self.biome[(z * SECTION_SIZE as i32 + x) as usize];
        }
        if !in_collar(x, z) {
            return 0;
        }
        let side = side_of(x, z);
        let border = &self.borders[side as usize];
        let along = if matches!(side, Side::West | Side::East) {
            z
        } else {
            x
        };
        // The collar's diagonal corners have no cell of their own along any
        // shared edge; they take the nearest edge cell of their strip.
        border.biomes[along.clamp(0, SECTION_SIZE as i32 - 1) as usize]
    }

    /// Copies one side of the collar out of the world.
    fn copy_border(&mut self, world: &World, side: Side) {
        let along_is_x = matches!(side, Side::West | Side::East);
        let across = match side {
            Side::North => -1,
            Side::South => SECTION_SIZE as i32,
            Side::West => -1,
            Side::East => SECTION_SIZE as i32,
        };
        let border = &mut self.borders[side as usize];
        for along in -1..=SECTION_SIZE as i32 {
            let (x, z) = if along_is_x {
                (across, along)
            } else {
                (along, across)
            };
            if (0..SECTION_SIZE as i32).contains(&along) {
                let world_x = self.cx * SECTION_SIZE as i32 + x;
                let world_z = self.cz * SECTION_SIZE as i32 + z;
                let neighbour = match side {
                    Side::North => (self.cx, self.cz - 1),
                    Side::South => (self.cx, self.cz + 1),
                    Side::West => (self.cx - 1, self.cz),
                    Side::East => (self.cx + 1, self.cz),
                };
                // The neighbour's own edge cell: its x is 15 for a west
                // neighbour and 0 for an east one, its z 15 for a north
                // neighbour and 0 for a south one.
                let edge = match side {
                    Side::North | Side::West => SECTION_SIZE - 1,
                    Side::South | Side::East => 0,
                };
                border.biomes[along as usize] = match world.chunk(neighbour.0, neighbour.1) {
                    Some(chunk) => {
                        if along_is_x {
                            chunk.biome(edge, local(world_z))
                        } else {
                            chunk.biome(local(world_x), edge)
                        }
                    }
                    None => 0,
                };
            }
            for y in 0..(SECTION_COUNT * SECTION_SIZE) as i32 {
                let world_x = self.cx * SECTION_SIZE as i32 + x;
                let world_z = self.cz * SECTION_SIZE as i32 + z;
                let index = border_index(across, along, y);
                border.blocks[index] = world.block(world_x, y, world_z);
                border.block_light[index] = world.block_light(world_x, y, world_z);
                border.sky_light[index] = world.sky_light(world_x, y, world_z);
            }
        }
    }
}

/// Copies one stored section's blocks and light into the snapshot's column
/// store. The section's own index layout is the column store's, so the copy is
/// a slice move for the blocks and a nibble-by-nibble unpack for the light.
fn copy_section(snapshot: &mut ColumnSnapshot, base: usize, section: &Section) {
    let offset = base << 8;
    let cells = SECTION_SIZE * SECTION_SIZE * SECTION_SIZE;
    snapshot.blocks[offset..offset + cells].copy_from_slice(section.blocks());
    for index in 0..cells {
        let cell = offset + index;
        snapshot.block_light[cell] = unpack_light(section.block_light_bytes(), index);
        snapshot.sky_light[cell] = unpack_light(section.sky_light_bytes(), index);
    }
}

/// Reads one nibble out of a packed light array; 0 when the array is absent —
/// a section without a sky store.
fn unpack_light(array: &[u8], index: usize) -> u8 {
    let Some(byte) = array.get(index >> 1) else {
        return 0;
    };
    if index & 1 == 0 {
        byte & 0x0F
    } else {
        byte >> 4
    }
}

/// Whether a position is one of the column's own cells.
fn in_column(x: i32, z: i32) -> bool {
    (0..SECTION_SIZE as i32).contains(&x) && (0..SECTION_SIZE as i32).contains(&z)
}

/// Whether a position is inside the collar: the column's cells or one cell out
/// on exactly one axis.
fn in_collar(x: i32, z: i32) -> bool {
    let x_ok = (-1..=SECTION_SIZE as i32).contains(&x);
    let z_ok = (-1..=SECTION_SIZE as i32).contains(&z);
    x_ok && z_ok && !(x_out(x) && z_out(z))
}

/// Whether `x` is one cell outside the column.
fn x_out(x: i32) -> bool {
    x == -1 || x == SECTION_SIZE as i32
}

/// Whether `z` is one cell outside the column.
fn z_out(z: i32) -> bool {
    z == -1 || z == SECTION_SIZE as i32
}

/// Whether a world height is inside the column's own range.
fn in_height(y: i32) -> bool {
    (0..(SECTION_COUNT * SECTION_SIZE) as i32).contains(&y)
}

/// The side of the collar a position outside the column sits on. The caller
/// has already checked that exactly one axis is out.
fn side_of(x: i32, z: i32) -> Side {
    if x_out(x) {
        if x == -1 { Side::West } else { Side::East }
    } else if z == -1 {
        Side::North
    } else {
        Side::South
    }
}

/// A collar position's two border axes: the off-column coordinate (`across`,
/// -1 or 16) and the in-column one (`along`, -1..=16).
fn across_along(side: Side, x: i32, z: i32) -> (i32, i32) {
    if matches!(side, Side::West | Side::East) {
        (x, z)
    } else {
        (z, x)
    }
}

/// The column-local coordinate of a world coordinate.
fn local(world: i32) -> usize {
    world.rem_euclid(SECTION_SIZE as i32) as usize
}
