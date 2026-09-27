//! Section and column storage: the typed blocks and light the decoded columns
//! become.

use oxide_proto_v47::column::{ColumnData, SectionData, block_index, unpack_nibble};

/// Blocks per axis in a section.
pub const SECTION_SIZE: usize = 16;

/// Sections in a column: y = 0..256.
pub const SECTION_COUNT: usize = 16;

/// Block slots in one section: 16 x 16 x 16.
const BLOCKS_PER_SECTION: usize = SECTION_SIZE * SECTION_SIZE * SECTION_SIZE;

/// Bytes in one light array: two nibbles to a byte.
const LIGHT_ARRAY_BYTES: usize = BLOCKS_PER_SECTION / 2;

/// The highest level a light nibble holds.
const MAX_LEVEL: u8 = 15;

/// Biome ids in one column: one per column position.
const BIOME_COUNT: usize = SECTION_SIZE * SECTION_SIZE;

/// One 16x16x16 section of block ids and light.
///
/// Every accessor takes coordinates that are each 0..16; a larger one has no
/// slot in the section.
#[derive(Debug, Clone)]
pub struct Section {
    /// 4096 packed block values, `(id << 4) | meta`.
    blocks: Box<[u16; BLOCKS_PER_SECTION]>,
    /// 2048 block-light nibbles.
    block_light: Box<[u8; LIGHT_ARRAY_BYTES]>,
    /// 2048 sky-light nibbles; `None` in a dimension without sky.
    sky_light: Option<Box<[u8; LIGHT_ARRAY_BYTES]>>,
}

impl Section {
    /// A section of air: block light 0, sky light 15 when the dimension has sky.
    pub fn air(has_sky: bool) -> Self {
        Self {
            blocks: Box::new([0u16; BLOCKS_PER_SECTION]),
            block_light: Box::new([0u8; LIGHT_ARRAY_BYTES]),
            sky_light: if has_sky {
                Some(Box::new([0xFFu8; LIGHT_ARRAY_BYTES]))
            } else {
                None
            },
        }
    }

    /// The packed block value, `(id << 4) | meta`.
    pub fn block(&self, x: usize, y: usize, z: usize) -> u16 {
        self.blocks[block_index(x, y, z)]
    }

    /// Sets the packed block value. Indices must be 0..16.
    pub fn set_block(&mut self, x: usize, y: usize, z: usize, value: u16) {
        self.blocks[block_index(x, y, z)] = value;
    }

    /// The block-light nibble, 0..15.
    pub fn block_light(&self, x: usize, y: usize, z: usize) -> u8 {
        unpack_nibble(&self.block_light, block_index(x, y, z))
    }

    /// The sky-light nibble, 0..15; 0 when the dimension has no sky.
    pub fn sky_light(&self, x: usize, y: usize, z: usize) -> u8 {
        match &self.sky_light {
            Some(array) => unpack_nibble(array, block_index(x, y, z)),
            None => 0,
        }
    }

    /// Sets the block-light nibble. Indices must be 0..16.
    ///
    /// # Panics
    ///
    /// Panics when `level` is above 15: a nibble holds 0..15, and a larger
    /// value is a programming error.
    pub fn set_block_light(&mut self, x: usize, y: usize, z: usize, level: u8) {
        let level = light_level(level);
        pack_nibble(&mut self.block_light, block_index(x, y, z), level);
    }

    /// Sets the sky-light nibble. Indices must be 0..16.
    ///
    /// A write into a section without a sky store — a dimension without sky —
    /// is a no-op: there is no array to store it in, and the section keeps
    /// reading 0 there.
    ///
    /// # Panics
    ///
    /// Panics when `level` is above 15.
    pub fn set_sky_light(&mut self, x: usize, y: usize, z: usize, level: u8) {
        let level = light_level(level);
        if let Some(array) = &mut self.sky_light {
            pack_nibble(array, block_index(x, y, z), level);
        }
    }

    /// The section's packed block values, in `block_index` order.
    ///
    /// The slice is the raw store the column snapshot copies from; it is 4096
    /// long and holds `(id << 4) | meta` per cell.
    pub fn blocks(&self) -> &[u16] {
        &self.blocks[..]
    }

    /// The section's stored block-light nibbles, two per byte.
    ///
    /// The packed form [`unpack_nibble`] reads; the snapshot unpacks it while
    /// copying, one cell at a time.
    pub fn block_light_bytes(&self) -> &[u8] {
        &self.block_light[..]
    }

    /// The section's stored sky-light nibbles, two per byte.
    ///
    /// Empty for a section without a sky store — a dimension without sky, or
    /// a section whose packet carried no sky array — where every cell reads 0.
    pub fn sky_light_bytes(&self) -> &[u8] {
        match &self.sky_light {
            Some(array) => &array[..],
            None => &[],
        }
    }
}

/// Reads a light level a setter was handed, 0..=15.
///
/// # Panics
///
/// Panics when `level` is above 15: no nibble holds it, so it is a programming
/// error rather than a value to clamp.
fn light_level(level: u8) -> u8 {
    assert!(level <= MAX_LEVEL, "the light level {level} is above 15");
    level
}

/// Writes one light level into a packed nibble array: the inverse of
/// `unpack_nibble`, which stores an even index in the low nibble of a byte and
/// an odd index in the high one.
///
/// The section setters and the light engine's column write-back share it.
///
/// # Panics
///
/// Panics when `index` is 4096 or greater: no block slot exists there. `level`
/// must be 0..=15; the callers validate it.
pub(crate) fn pack_nibble(array: &mut [u8; LIGHT_ARRAY_BYTES], index: usize, level: u8) {
    let byte = &mut array[index >> 1];
    *byte = if index & 1 == 0 {
        (*byte & 0xF0) | (level & 0x0F)
    } else {
        (*byte & 0x0F) | (level << 4)
    };
}

/// The stored section for wire data: the wire-to-store bridge.
///
/// The protocol crate cannot name [`Section`] in the wire types it defines, so
/// the crossing lives here: tests and any later code that needs one build a
/// stored section from [`SectionData`] through this function.
///
/// A sky-light array is kept only for a dimension with sky; a section the
/// packet gave no sky-light array for reads as dark.
pub fn section_from_data(data: &SectionData, has_sky: bool) -> Section {
    Section {
        blocks: data.blocks.clone(),
        block_light: data.block_light.clone(),
        sky_light: if has_sky {
            data.sky_light.clone()
        } else {
            None
        },
    }
}

/// The wire data for a stored section: the store-to-wire half of the bridge.
///
/// A sky-light array is carried over only when the section holds one: a
/// section from a dimension without sky reads back without one.
pub fn data_from_section(section: &Section) -> SectionData {
    SectionData {
        blocks: section.blocks.clone(),
        block_light: section.block_light.clone(),
        sky_light: section.sky_light.clone(),
    }
}

/// A 16x16x256 column.
#[derive(Debug)]
pub struct Chunk {
    /// The chunk x this column belongs to.
    x: i32,
    /// The chunk z this column belongs to.
    z: i32,
    /// Whether the dimension has sky light.
    has_sky: bool,
    /// The sections by index; `None` for a section the column does not hold.
    sections: [Option<Section>; SECTION_COUNT],
    /// Biome ids indexed `(z << 4) | x`; zero until a packet carries an array.
    biomes: [u8; BIOME_COUNT],
}

impl Chunk {
    /// An empty column for the given dimension.
    pub fn new(x: i32, z: i32, has_sky: bool) -> Self {
        Self {
            x,
            z,
            has_sky,
            sections: std::array::from_fn(|_| None),
            biomes: [0u8; BIOME_COUNT],
        }
    }

    /// The chunk x.
    pub fn x(&self) -> i32 {
        self.x
    }

    /// The chunk z.
    pub fn z(&self) -> i32 {
        self.z
    }

    /// Applies a decoded column.
    ///
    /// `ground_up` replaces the whole column: sections in the mask are replaced
    /// by the packet's data, every other section becomes air, and the biome
    /// array is replaced when the packet carried one. A section update replaces
    /// only the sections in the mask; every other section keeps its blocks and
    /// its light, and the biomes are left alone.
    ///
    /// `has_sky` is the dimension's sky flag; the column adopts it, and a
    /// sky-light array is stored only when it is set.
    pub fn apply(&mut self, data: &ColumnData, ground_up: bool, has_sky: bool) {
        self.has_sky = has_sky;
        for (index, slot) in self.sections.iter_mut().enumerate() {
            match data.sections[index].as_ref() {
                Some(section) => *slot = Some(section_from_data(section, has_sky)),
                None if ground_up => *slot = None,
                None => {}
            }
        }
        if ground_up {
            self.biomes = data.biomes.unwrap_or(self.biomes);
        }
    }

    /// The packed block at world-local coordinates; y 0..256.
    pub fn block(&self, x: usize, y: usize, z: usize) -> u16 {
        match self.section_at(y) {
            Some(section) => section.block(x % SECTION_SIZE, y % SECTION_SIZE, z % SECTION_SIZE),
            None => 0,
        }
    }

    /// The block-light nibble at world-local coordinates; 0 for a section that
    /// is not present, and 0 outside 0..256.
    pub fn block_light_at(&self, x: usize, y: usize, z: usize) -> u8 {
        match self.section_at(y) {
            Some(section) => {
                section.block_light(x % SECTION_SIZE, y % SECTION_SIZE, z % SECTION_SIZE)
            }
            None => 0,
        }
    }

    /// The sky-light nibble at world-local coordinates; 15 for an absent section
    /// in a dimension with sky, 0 in one without.
    pub fn sky_light_at(&self, x: usize, y: usize, z: usize) -> u8 {
        match self.section_at(y) {
            Some(section) => {
                section.sky_light(x % SECTION_SIZE, y % SECTION_SIZE, z % SECTION_SIZE)
            }
            None if self.has_sky => 15,
            None => 0,
        }
    }

    /// Sets the block-light nibble at world-local coordinates; y 0..256.
    ///
    /// A write into a section the column does not hold is a no-op: the cell
    /// keeps whichever default its reads give (0 for block light).
    ///
    /// # Panics
    ///
    /// Panics when `level` is above 15.
    pub fn set_block_light(&mut self, x: usize, y: usize, z: usize, level: u8) {
        let level = light_level(level);
        if let Some(section) = self.section_mut(y) {
            section.set_block_light(x % SECTION_SIZE, y % SECTION_SIZE, z % SECTION_SIZE, level);
        }
    }

    /// Sets the sky-light nibble at world-local coordinates; y 0..256.
    ///
    /// A write into a section the column does not hold, or into a section
    /// without a sky store, is a no-op: the cell keeps whichever default its
    /// reads give (15 for an absent section in a dimension with sky).
    ///
    /// # Panics
    ///
    /// Panics when `level` is above 15.
    pub fn set_sky_light(&mut self, x: usize, y: usize, z: usize, level: u8) {
        let level = light_level(level);
        if let Some(section) = self.section_mut(y) {
            section.set_sky_light(x % SECTION_SIZE, y % SECTION_SIZE, z % SECTION_SIZE, level);
        }
    }

    /// Whether the column holds no blocks at all.
    pub fn is_empty(&self) -> bool {
        self.sections
            .iter()
            .flatten()
            .all(|section| section.blocks.iter().all(|&value| value == 0))
    }

    /// The biome id at a column position.
    pub fn biome(&self, x: usize, z: usize) -> u8 {
        self.biomes[block_index(x % SECTION_SIZE, 0, z % SECTION_SIZE)]
    }

    /// The section a y coordinate falls in, when the column holds it. `y` is
    /// 0..256; anything above has no section.
    fn section_at(&self, y: usize) -> Option<&Section> {
        self.sections.get(y / SECTION_SIZE)?.as_ref()
    }

    /// The stored section at a section index, when the column holds it.
    ///
    /// The whole-store half of the column snapshot's copy: `sy` is 0..16, and
    /// a section the column does not hold — an empty slot from a partial send
    /// — answers `None`.
    pub fn section(&self, sy: usize) -> Option<&Section> {
        self.sections.get(sy)?.as_ref()
    }

    /// The section a y coordinate falls in, mutably, when the column holds it.
    fn section_mut(&mut self, y: usize) -> Option<&mut Section> {
        self.sections.get_mut(y / SECTION_SIZE)?.as_mut()
    }
}
