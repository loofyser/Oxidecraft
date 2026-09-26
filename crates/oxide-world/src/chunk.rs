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

    /// The wire data as a stored section.
    ///
    /// A sky-light array is kept only for a dimension with sky; a section the
    /// packet gave no sky-light array for reads as dark.
    fn from_data(data: &SectionData, has_sky: bool) -> Self {
        Self {
            blocks: data.blocks.clone(),
            block_light: data.block_light.clone(),
            sky_light: if has_sky {
                data.sky_light.clone()
            } else {
                None
            },
        }
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
                Some(section) => *slot = Some(Section::from_data(section, has_sky)),
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
}
