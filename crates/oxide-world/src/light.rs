//! The light engine: vanilla's sky-light and block-light rules over the stored
//! nibbles, the recomputation a change needs, and the query the mesher uses.
//!
//! # The model
//!
//! A cell holds one 0..15 level per kind. Its level is the greatest of its own
//! source and, over the six neighbours, the neighbour's level minus the cell's
//! own attenuation (`World.getRawLight`, the decompiled 1.8.9 client's
//! `world/World.java:2783-2834`):
//!
//! * The attenuation is `max(1, light_opacity)`, from the behaviour table's
//!   `light_opacity` column. A cell whose opacity is 15 or more takes no inflow
//!   at all and holds its own source; a cell that is both opaque and luminous
//!   is the one exception, at an attenuation of 1 (`World.java:2795-2798`,
//!   `:2800-2808`).
//! * Block light's source is the block's `light_emission`
//!   (`Block.getLightValue`, `block/Block.java:225-227`): a torch cell holds
//!   14, and each step away loses the receiving cell's attenuation.
//! * Sky light has no emission term (`World.java:2792`) and adds a direct
//!   source: a cell at or above its column's height map holds 15
//!   (`World.java:2785-2788`). The height map is one above the topmost cell of
//!   the column whose opacity is not zero, so the 15 runs straight down an open
//!   column and stops at the first cell that is not transparent
//!   (`Chunk.canSeeSky`, `world/chunk/Chunk.java:904-910`, filled in by
//!   `Chunk.generateSkylightMap`, `:246-311`).
//!
//! The three sky rules the design spec states are that one rule seen from three
//! sides: full strength runs down through transparent cells without loss
//! (`getRawLight` returns 15 above the height map, and the column fill takes
//! nothing off the running level while it is still 15, `Chunk.java:281-286`);
//! every other step — sideways, upward, or any step of a level below 15 — pays
//! the receiving cell's attenuation; an opaque cell takes nothing.
//!
//! The attenuation is the receiving block's own opacity, not one. Water and ice
//! carry opacity 3 (`Block.java:1261-1262`, `:1337`), leaves and cobwebs
//! opacity 1 (`BlockLeaves.java:33`, `Block.java:1284`), and `getLightOpacity`
//! has no overrides anywhere in the block package. A shaft of water cells under
//! open sky therefore reads 12, 9, 6, 3, 0 from its top down, and a leaf cell
//! under open sky reads 14. The design spec's section 9 and the protocol
//! research report's lighting section both say a light-filtering block reduces
//! sky light by exactly one; the source is the model here, and the tests pin
//! the source's numbers.
//!
//! # The region a recomputation owns
//!
//! [`recompute`] owns the changed chunk column together with its eight
//! neighbours, full height, as a closed system: it reads no light outside the
//! region and writes none, so the values at the region's boundary are the ones
//! the region's own cells give. The size covers a change's reach — no light
//! travels more than 15 cells, and vanilla bounds its own engine at a manhattan
//! distance of 17 (`World.checkLightFor`, `World.java:2836-3000`, the bounds at
//! `:2881` and `:2931`) — so a world no larger than the region recomputes
//! exactly, which the tests pin.
//!
//! [`recompute_column`] owns one chunk column and reads the facing cells of the
//! four neighbouring columns as sources it never writes, which is what leaves a
//! column whose light is already correct unchanged.
//!
//! Both passes compute from scratch: the stored nibbles are overwritten, not
//! consulted, so a result does not depend on the light the store happened to
//! hold. Neither call does anything when the world does not hold the column.
//!
//! # Writing back
//!
//! The store reaches a column only through [`World::apply_column`], so a pass
//! stores its levels by rebuilding each of the region's columns from the stored
//! blocks plus the recomputed nibbles and applying it as a section update. A
//! section the column does not hold is stored as the air the pass read for it:
//! the store cannot carry an absent section through an apply.

use std::collections::VecDeque;

use oxide_proto_v47::column::{ColumnData, SectionData, block_index};

use crate::behaviour::behaviour;
use crate::chunk::{SECTION_COUNT, SECTION_SIZE, pack_nibble};
use crate::world::World;

/// The highest level a cell holds.
const MAX_LEVEL: u8 = 15;

/// Cells of a column's height: y = 0..256.
const HEIGHT: usize = SECTION_COUNT * SECTION_SIZE;

/// The six directions a level spreads over.
const NEIGHBOURS: [(i32, i32, i32); 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

/// The light the mesher shades a cell with: the greater of the two kinds.
///
/// 0 for a y outside 0..256 and 0 outside the loaded columns.
pub fn light_at(world: &World, x: i32, y: i32, z: i32) -> u8 {
    if !(0..HEIGHT as i32).contains(&y) {
        return 0;
    }
    let cx = x.div_euclid(SECTION_SIZE as i32);
    let cz = z.div_euclid(SECTION_SIZE as i32);
    let Some(chunk) = world.chunk(cx, cz) else {
        return 0;
    };
    let lx = x.rem_euclid(SECTION_SIZE as i32) as usize;
    let lz = z.rem_euclid(SECTION_SIZE as i32) as usize;
    let y = y as usize;
    chunk
        .sky_light_at(lx, y, lz)
        .max(chunk.block_light_at(lx, y, lz))
}

/// The light level a render-view position's brightness is read from: the level
/// `World.getLightBrightness` (`World.java:845-848`) looks up in the provider's table.
///
/// The level is `World.getLightFromNeighbors`'s (`:621-624`), the combined light of
/// `World.getLight` (`:626-679`). The neighbour-brightness term (`:630-657`) fires only for a
/// block whose `getUseNeighborBrightness` is true, which nothing in the decompiled tree
/// registers; the direct rule is the whole rule here: a y below zero answers zero (`:658-661`),
/// a y at or above the build height reads the topmost cell (`:662-670`), and the cell's own
/// value is the sky kind's nibble against the block kind's, greater one wins
/// (`Chunk.getLightSubtracted`, `Chunk.java:818-841`, with the day-night sky subtraction at
/// zero for the noon clock the acceptance captured).
///
/// A position no column holds answers the sky kind's default of 15
/// (`World.getLightFor`, `World.java:789-803`, `EnumSkyBlock.SKY` at `EnumSkyBlock.java:5-12`),
/// which is also what a held column answers where it carries no section
/// (`Chunk.getLightSubtracted`'s absent-array arm, `Chunk.java:825-828`) — our store keeps the
/// two apart and both answer 15 at noon.
pub fn view_light_level(world: &World, x: i32, y: i32, z: i32) -> u8 {
    let y = if y >= HEIGHT as i32 {
        HEIGHT as i32 - 1
    } else {
        y
    };
    if y < 0 {
        return 0;
    }
    let cx = x.div_euclid(SECTION_SIZE as i32);
    let cz = z.div_euclid(SECTION_SIZE as i32);
    let Some(chunk) = world.chunk(cx, cz) else {
        return 15;
    };
    let lx = x.rem_euclid(SECTION_SIZE as i32) as usize;
    let lz = z.rem_euclid(SECTION_SIZE as i32) as usize;
    let y = y as usize;
    chunk
        .sky_light_at(lx, y, lz)
        .max(chunk.block_light_at(lx, y, lz))
}

/// Recomputes both light kinds from scratch over the changed chunk column and
/// its eight neighbours, full height.
///
/// This is the pass a block change needs. The region is a closed system (see
/// the module docs): propagation stops at its boundary, and the light outside
/// it is left as it was. A call whose column the world does not hold is a
/// no-op. The changed cell's `y` does not narrow the region, so it is not read.
pub fn recompute(world: &mut World, x: i32, _y: i32, z: i32) {
    let cx = x.div_euclid(SECTION_SIZE as i32);
    let cz = z.div_euclid(SECTION_SIZE as i32);
    if world.chunk(cx, cz).is_none() {
        return;
    }
    let region = region_of(world, cx - 1, cz - 1, 3, 3);
    let block = levels(world, &region, Kind::Block, false);
    let sky = world
        .has_sky()
        .then(|| levels(world, &region, Kind::Sky, false));
    store(world, &region, sky.as_ref(), &block);
}

/// Recomputes both light kinds from scratch over one whole chunk column: sky
/// light from the column's own blocks, block light from its emitters.
///
/// The one-cell border of the four neighbouring columns is read as a source —
/// never written — so the pass accounts for the light that crosses into the
/// column, and a column whose light is already correct is left as it is. A call
/// whose column the world does not hold is a no-op.
pub fn recompute_column(world: &mut World, cx: i32, cz: i32) {
    if world.chunk(cx, cz).is_none() {
        return;
    }
    let region = region_of(world, cx, cz, 1, 1);
    let block = levels(world, &region, Kind::Block, true);
    let sky = world
        .has_sky()
        .then(|| levels(world, &region, Kind::Sky, true));
    store(world, &region, sky.as_ref(), &block);
}

/// One of the two light kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Sky light: the direct-exposure term and no emission.
    Sky,
    /// Block light: emission and no direct exposure.
    Block,
}

/// The cost of entering a cell, or 0 for a cell that takes no inflow.
///
/// `World.getRawLight` (`World.java:2793-2808`) reads the block's own opacity,
/// lifts it to 1 for a cell that is both opaque and luminous, takes at least 1
/// off everything else, and lets a cell of opacity 15 or more take nothing at
/// all.
fn attenuation(opacity: u8, emission: u8) -> u8 {
    if opacity >= MAX_LEVEL {
        if emission > 0 { 1 } else { 0 }
    } else {
        opacity.max(1)
    }
}

/// A block id's row in the behaviour table, as the two light columns.
///
/// An id outside the covered set has no row and is air to the model, which is
/// what the source's own lookup gives for an id no block is registered at:
/// opacity 0 and no emission.
fn light_columns(id: u16) -> (u8, u8) {
    behaviour(id).map_or((0, 0), |row| (row.light_opacity, row.light_emission))
}

/// A pass's cell set: a rectangle of chunk columns, full height, together with
/// the blocks of every slot the world holds.
struct Region {
    /// The chunk coordinates of the rectangle's low corner.
    base_x: i32,
    base_z: i32,
    /// The rectangle's size in chunk columns.
    chunks_x: usize,
    chunks_z: usize,
    /// Whether each slot is loaded, indexed `z * chunks_x + x`.
    loaded: Vec<bool>,
    /// Each loaded slot's blocks, `section * 4096 + block_index` inside it.
    blocks: Vec<Vec<u16>>,
}

impl Region {
    /// Cells along x and along z.
    fn width(&self) -> usize {
        self.chunks_x * SECTION_SIZE
    }

    fn depth(&self) -> usize {
        self.chunks_z * SECTION_SIZE
    }

    /// The slot a cell of the rectangle falls in, when the world holds it.
    fn slot(&self, lx: usize, lz: usize) -> Option<usize> {
        let slot = (lz / SECTION_SIZE) * self.chunks_x + lx / SECTION_SIZE;
        self.loaded[slot].then_some(slot)
    }

    /// The flat index of a loaded slot's cell, from the rectangle's corner.
    fn index_at(&self, slot: usize, lx: usize, y: usize, lz: usize) -> usize {
        let x = (slot % self.chunks_x) * SECTION_SIZE + lx;
        let z = (slot / self.chunks_x) * SECTION_SIZE + lz;
        (z * self.width() + x) * HEIGHT + y
    }

    /// The flat index of a world position, or `None` outside the rectangle,
    /// outside y 0..256, or in a slot the world does not hold.
    fn index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        if !(0..HEIGHT as i32).contains(&y) {
            return None;
        }
        let lx = x - self.base_x * SECTION_SIZE as i32;
        let lz = z - self.base_z * SECTION_SIZE as i32;
        if lx < 0 || lz < 0 || lx as usize >= self.width() || lz as usize >= self.depth() {
            return None;
        }
        let (lx, lz) = (lx as usize, lz as usize);
        self.slot(lx, lz)?;
        Some((lz * self.width() + lx) * HEIGHT + y as usize)
    }

    /// The world position of a cell index.
    fn cell_of(&self, index: usize) -> (i32, i32, i32) {
        let y = (index % HEIGHT) as i32;
        let column = index / HEIGHT;
        let lz = column / self.width();
        let lx = column % self.width();
        (
            self.base_x * SECTION_SIZE as i32 + lx as i32,
            y,
            self.base_z * SECTION_SIZE as i32 + lz as i32,
        )
    }

    /// One cell's stored block value.
    fn block_at(&self, slot: usize, lx: usize, y: usize, lz: usize) -> u16 {
        self.blocks[slot][(y / SECTION_SIZE) * 4096 + block_index(lx, y % SECTION_SIZE, lz)]
    }

    /// The slot's chunk coordinates.
    fn chunk_of(&self, slot: usize) -> (i32, i32) {
        (
            self.base_x + (slot % self.chunks_x) as i32,
            self.base_z + (slot / self.chunks_x) as i32,
        )
    }
}

/// Reads a rectangle of chunk columns out of the world.
///
/// The slots the world does not hold stay out of the pass.
fn region_of(world: &World, base_x: i32, base_z: i32, chunks_x: usize, chunks_z: usize) -> Region {
    let mut loaded = vec![false; chunks_x * chunks_z];
    let mut blocks = vec![Vec::new(); chunks_x * chunks_z];
    for slot in 0..loaded.len() {
        let cx = base_x + (slot % chunks_x) as i32;
        let cz = base_z + (slot / chunks_x) as i32;
        let Some(chunk) = world.chunk(cx, cz) else {
            continue;
        };
        loaded[slot] = true;
        let mut stored = vec![0u16; 4096 * SECTION_COUNT];
        for section in 0..SECTION_COUNT {
            for ly in 0..SECTION_SIZE {
                for lz in 0..SECTION_SIZE {
                    for lx in 0..SECTION_SIZE {
                        stored[section * 4096 + block_index(lx, ly, lz)] =
                            chunk.block(lx, section * SECTION_SIZE + ly, lz);
                    }
                }
            }
        }
        blocks[slot] = stored;
    }
    Region {
        base_x,
        base_z,
        chunks_x,
        chunks_z,
        loaded,
        blocks,
    }
}

/// One kind's levels over a region, with the queue the spread walks.
struct Levels {
    /// The level of every cell, indexed as [`Region::index_at`].
    level: Vec<u8>,
    /// The attenuation of every cell: 0 for a cell that takes no inflow.
    attenuation: Vec<u8>,
    /// The cells to visit, by flat index; a cell is enqueued when its level
    /// rises, which is the visit rule `World.checkLightFor` settles on
    /// (`World.java:2920-2964`).
    queue: VecDeque<usize>,
}

/// Runs one kind's spread over the region and returns the levels.
///
/// `border` adds the one-cell read outside the rectangle that
/// [`recompute_column`] may make: the facing cells of the four neighbouring
/// columns are read as sources, never written.
fn levels(world: &World, region: &Region, kind: Kind, border: bool) -> Levels {
    let cells = region.width() * region.depth() * HEIGHT;
    let mut levels = Levels {
        level: vec![0; cells],
        attenuation: vec![0; cells],
        queue: VecDeque::new(),
    };
    let mut emission = vec![0u8; cells];
    for slot in 0..region.loaded.len() {
        if !region.loaded[slot] {
            continue;
        }
        for section in 0..SECTION_COUNT {
            for ly in 0..SECTION_SIZE {
                for lz in 0..SECTION_SIZE {
                    for lx in 0..SECTION_SIZE {
                        let y = section * SECTION_SIZE + ly;
                        let index = region.index_at(slot, lx, y, lz);
                        let (opacity, emits) = light_columns(region.block_at(slot, lx, y, lz) >> 4);
                        levels.attenuation[index] = attenuation(opacity, emits);
                        emission[index] = emits;
                    }
                }
            }
        }
    }
    match kind {
        Kind::Sky => seed_sky(region, &mut levels),
        Kind::Block => seed_block(&mut levels, &emission),
    }
    if border {
        seed_border(world, region, kind, &mut levels);
    }
    spread(region, &mut levels);
    levels
}

/// Seeds the sky pass: every cell at or above its column's height map is at 15.
///
/// The height map is one above the topmost cell of the column whose opacity is
/// not zero, so an entirely transparent column is 15 from bottom to top
/// (`Chunk.canSeeSky`, `Chunk.java:904-910`).
fn seed_sky(region: &Region, levels: &mut Levels) {
    for slot in 0..region.loaded.len() {
        if !region.loaded[slot] {
            continue;
        }
        for lz in 0..SECTION_SIZE {
            for lx in 0..SECTION_SIZE {
                let mut exposed = 0;
                for y in (0..HEIGHT).rev() {
                    let (opacity, _) = light_columns(region.block_at(slot, lx, y, lz) >> 4);
                    if opacity != 0 {
                        exposed = y + 1;
                        break;
                    }
                }
                for y in exposed..HEIGHT {
                    let index = region.index_at(slot, lx, y, lz);
                    levels.level[index] = MAX_LEVEL;
                    levels.queue.push_back(index);
                }
            }
        }
    }
}

/// Seeds the block pass: every emitting cell starts at its emission.
fn seed_block(levels: &mut Levels, emission: &[u8]) {
    for (index, &emits) in emission.iter().enumerate() {
        if emits > 0 {
            levels.level[index] = emits;
            levels.queue.push_back(index);
        }
    }
}

/// Lets the cells outside the rectangle feed the ones at its edge.
///
/// A source outside the region never changes during the pass, so one pass over
/// the rectangle's edge cells takes everything it has to give. A neighbour the
/// world does not hold contributes nothing here, although the source's own
/// `getLightFor` answers the sky kind's default of 15 for a position no
/// column holds (`World.java:789-803`): vanilla refuses to relight around an
/// unloaded neighbour too (`World.checkLightFor`'s loaded-area bound,
/// `World.java:2838`), so the engine reads it as dark.
fn seed_border(world: &World, region: &Region, kind: Kind, levels: &mut Levels) {
    for index in 0..levels.level.len() {
        let (x, y, z) = region.cell_of(index);
        let attenuation = levels.attenuation[index];
        if attenuation == 0 {
            continue;
        }
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            if region.index(x + dx, y, z + dz).is_some() {
                continue;
            }
            let Some(level) = source(world, kind, x + dx, y, z + dz) else {
                continue;
            };
            let candidate = level as i32 - attenuation as i32;
            if candidate > levels.level[index] as i32 {
                levels.level[index] = candidate as u8;
                levels.queue.push_back(index);
            }
        }
    }
}

/// One cell outside the region, read from the store as a source.
fn source(world: &World, kind: Kind, x: i32, y: i32, z: i32) -> Option<u8> {
    let chunk = world.chunk(
        x.div_euclid(SECTION_SIZE as i32),
        z.div_euclid(SECTION_SIZE as i32),
    )?;
    let lx = x.rem_euclid(SECTION_SIZE as i32) as usize;
    let lz = z.rem_euclid(SECTION_SIZE as i32) as usize;
    let y = y as usize;
    Some(match kind {
        Kind::Sky => chunk.sky_light_at(lx, y, lz),
        Kind::Block => chunk.block_light_at(lx, y, lz),
    })
}

/// Spreads the levels: pop a cell, relax its six neighbours, and enqueue a
/// neighbour when its level rises.
fn spread(region: &Region, levels: &mut Levels) {
    while let Some(index) = levels.queue.pop_front() {
        let (x, y, z) = region.cell_of(index);
        let level = levels.level[index];
        // A cell at 1 or below can raise nothing: every receivable neighbour
        // attenuates by at least 1, so the candidate it offers is at most 0,
        // which never passes the strict rise test below. The exit is a
        // work-saver only; values and termination are the same without it.
        if level <= 1 {
            continue;
        }
        for (dx, dy, dz) in NEIGHBOURS {
            let Some(neighbour) = region.index(x + dx, y + dy, z + dz) else {
                continue;
            };
            let attenuation = levels.attenuation[neighbour];
            if attenuation == 0 {
                continue;
            }
            let candidate = level as i32 - attenuation as i32;
            if candidate > levels.level[neighbour] as i32 {
                levels.level[neighbour] = candidate as u8;
                levels.queue.push_back(neighbour);
            }
        }
    }
}

/// Writes a pass's levels back into the world.
///
/// Each loaded slot is rebuilt from its stored blocks with the recomputed
/// nibbles and applied to its column as a section update, which is the only
/// route the store offers the engine (see the module docs). A dimension without
/// sky carries no sky-light array.
fn store(world: &mut World, region: &Region, sky: Option<&Levels>, block: &Levels) {
    for slot in 0..region.loaded.len() {
        if !region.loaded[slot] {
            continue;
        }
        let mut data = ColumnData::empty();
        for section in 0..SECTION_COUNT {
            let mut blocks = Box::new([0u16; 4096]);
            blocks.copy_from_slice(&region.blocks[slot][section * 4096..(section + 1) * 4096]);
            let mut block_light = Box::new([0u8; 2048]);
            let mut sky_light = sky.map(|_| Box::new([0u8; 2048]));
            for ly in 0..SECTION_SIZE {
                for lz in 0..SECTION_SIZE {
                    for lx in 0..SECTION_SIZE {
                        let y = section * SECTION_SIZE + ly;
                        let index = region.index_at(slot, lx, y, lz);
                        let cell = block_index(lx, ly, lz);
                        pack_nibble(&mut block_light, cell, block.level[index]);
                        if let (Some(levels), Some(array)) = (sky, sky_light.as_mut()) {
                            pack_nibble(array, cell, levels.level[index]);
                        }
                    }
                }
            }
            data.sections[section] = Some(SectionData {
                blocks,
                block_light,
                sky_light,
            });
            data.mask |= 1u16 << section;
        }
        let (cx, cz) = region.chunk_of(slot);
        world.apply_column(cx, cz, &data, false);
    }
}
