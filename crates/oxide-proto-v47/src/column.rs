//! The chunk-column payload: block, block-light, sky-light and biome arrays,
//! shared by Chunk Data (0x21) and Map Chunk Bulk (0x26).

use crate::PacketError;

/// The number of blocks in one section.
const BLOCKS_PER_SECTION: usize = 4096;

/// The bytes one section's block array occupies: 4096 little-endian shorts.
const BLOCK_ARRAY_BYTES: usize = BLOCKS_PER_SECTION * 2;

/// The bytes one section's light array occupies: 2048 nibble pairs.
const LIGHT_ARRAY_BYTES: usize = 2048;

/// The bytes of the column-wide biome array.
const BIOME_ARRAY_BYTES: usize = 256;

/// One section of a column, exactly as the wire delivers it.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionData {
    /// 4096 blocks, `(id << 4) | meta`, little-endian on the wire.
    pub blocks: Box<[u16; 4096]>,
    /// 2048 nibbles of block light.
    pub block_light: Box<[u8; 2048]>,
    /// 2048 nibbles of sky light, absent for dimensions without sky.
    pub sky_light: Option<Box<[u8; 2048]>>,
}

/// A decoded chunk column: the sections the packet carried, by section index.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnData {
    /// The primary bitmask the packet declared.
    pub mask: u16,
    /// Sections indexed by `y >> 4`; `None` for sections outside the mask.
    pub sections: [Option<SectionData>; 16],
    /// The biome array, present when the packet carried one.
    pub biomes: Option<[u8; 256]>,
}

impl ColumnData {
    /// An empty column: mask zero, every slot `None`, no biomes.
    ///
    /// This is the unload shape: a ground-up packet whose mask is zero.
    pub fn empty() -> Self {
        Self {
            mask: 0,
            sections: std::array::from_fn(|_| None),
            biomes: None,
        }
    }
}

/// The linear index of a block inside its section: x varies fastest, then z,
/// then y.
///
/// Each coordinate is `0..16`; the index is `(y << 8) | (z << 4) | x`, the
/// order vanilla lays a section out in.
pub fn block_index(x: usize, y: usize, z: usize) -> usize {
    (y << 8) | (z << 4) | x
}

/// Reads one nibble out of a light array.
///
/// The index is the section-linear index from [`block_index`], `0..4096`:
/// vanilla stores an even index in the low nibble of a byte and an odd index
/// in the high one.
///
/// # Panics
///
/// Panics when `index` is 4096 or greater: no block slot exists there.
pub fn unpack_nibble(array: &[u8; 2048], index: usize) -> u8 {
    let byte = array[index >> 1];
    if index & 1 == 0 {
        byte & 0x0F
    } else {
        byte >> 4
    }
}

/// Reads a block value's `(id, meta)` pair by coordinate.
///
/// The id is the full 12-bit field the wire carries, not an eight-bit id.
///
/// # Panics
///
/// Panics when the index computed from `x`, `y` and `z` is 4096 or greater:
/// no block slot exists there. Each coordinate is `0..16`, and a `y` of 16 or
/// above is the case that reaches past the array. The column decoder never
/// calls it out of range — a section it hands out holds all 4096 slots.
pub fn unpack_block(blocks: &[u16; 4096], x: usize, y: usize, z: usize) -> (u16, u8) {
    let value = blocks[block_index(x, y, z)];
    (value >> 4, (value & 0x0F) as u8)
}

/// The size in bytes of one column payload.
///
/// `mask` selects the sections; `sky` adds a sky-light array for each of them
/// (the Nether and the End carry none); `biomes_present` adds the 256-byte
/// biome array, which only a ground-up packet carries.
pub fn column_size(mask: u16, sky: bool, biomes_present: bool) -> usize {
    let sections = mask.count_ones() as usize;
    let mut size = sections * (BLOCK_ARRAY_BYTES + LIGHT_ARRAY_BYTES);
    if sky {
        size += sections * LIGHT_ARRAY_BYTES;
    }
    if biomes_present {
        size += BIOME_ARRAY_BYTES;
    }
    size
}

/// Decodes one column payload into its sections and biomes.
///
/// The payload holds, in this order: the block array of every section the
/// mask selects, ascending by section; their block-light arrays; their
/// sky-light arrays when `sky`; and finally the 256 biome bytes when
/// `biomes_present`. A payload whose length is not exactly
/// [`column_size`] of the same arguments is refused before any offset is
/// used, so the reader never reaches past the bytes it was given.
pub fn parse_column(
    data: &[u8],
    mask: u16,
    sky: bool,
    biomes_present: bool,
) -> Result<ColumnData, PacketError> {
    let expected = column_size(mask, sky, biomes_present);
    if data.len() != expected {
        return Err(PacketError::BadColumnSize {
            got: data.len(),
            expected,
        });
    }
    let sections = mask.count_ones() as usize;
    let blocks_bytes = sections * BLOCK_ARRAY_BYTES;
    let light_bytes = sections * LIGHT_ARRAY_BYTES;
    let sky_offset = blocks_bytes + light_bytes;
    let biomes_offset = sky_offset + if sky { light_bytes } else { 0 };

    let mut out: [Option<SectionData>; 16] = std::array::from_fn(|_| None);
    let mut ordinal = 0usize;
    for (bit, slot) in out.iter_mut().enumerate() {
        if mask & (1u16 << bit) == 0 {
            continue;
        }
        let sky_light = if sky {
            Some(read_array::<2048>(
                data,
                sky_offset + ordinal * LIGHT_ARRAY_BYTES,
            )?)
        } else {
            None
        };
        *slot = Some(SectionData {
            blocks: read_blocks(data, ordinal * BLOCK_ARRAY_BYTES)?,
            block_light: read_array::<2048>(data, blocks_bytes + ordinal * LIGHT_ARRAY_BYTES)?,
            sky_light,
        });
        ordinal += 1;
    }
    let biomes = if biomes_present {
        Some(*read_array::<256>(data, biomes_offset)?)
    } else {
        None
    };
    Ok(ColumnData {
        mask,
        sections: out,
        biomes,
    })
}

/// Reads one section's block array at `at`, little-endian.
fn read_blocks(data: &[u8], at: usize) -> Result<Box<[u16; 4096]>, PacketError> {
    let raw = read_bytes_at(data, at, BLOCK_ARRAY_BYTES)?;
    let mut blocks = Box::new([0u16; BLOCKS_PER_SECTION]);
    for (slot, pair) in blocks.iter_mut().zip(raw.chunks_exact(2)) {
        *slot = u16::from_le_bytes([pair[0], pair[1]]);
    }
    Ok(blocks)
}

/// Reads a fixed-size byte array at `at`.
fn read_array<const N: usize>(data: &[u8], at: usize) -> Result<Box<[u8; N]>, PacketError> {
    let raw = read_bytes_at(data, at, N)?;
    let mut out = Box::new([0u8; N]);
    out.copy_from_slice(raw);
    Ok(out)
}

/// The `len` bytes at `at`, or a size refusal when they fall outside.
fn read_bytes_at(data: &[u8], at: usize, len: usize) -> Result<&[u8], PacketError> {
    let end = at.saturating_add(len);
    data.get(at..end).ok_or(PacketError::BadColumnSize {
        got: data.len(),
        expected: end,
    })
}

#[cfg(test)]
mod tests {
    use super::{block_index, unpack_block, unpack_nibble};

    #[test]
    fn unpack_block_splits_id_and_meta() {
        let mut blocks = [0u16; 4096];
        blocks[block_index(2, 3, 4)] = 0x1234;
        let (id, meta) = unpack_block(&blocks, 2, 3, 4);
        assert_eq!(id, 0x123);
        assert_eq!(meta, 4);
    }

    #[test]
    fn unpack_nibble_reads_the_vanilla_order() {
        let mut array = [0u8; 2048];
        array[0] = 0x3F;
        assert_eq!(unpack_nibble(&array, 0), 0x0F);
        assert_eq!(unpack_nibble(&array, 1), 0x3);
        array[1] = 0x21;
        assert_eq!(unpack_nibble(&array, 2), 1);
        assert_eq!(unpack_nibble(&array, 3), 2);
    }

    #[test]
    fn unpack_block_sees_the_full_identifier_field() {
        // A value above the eight-bit id range must keep its high bits.
        let mut blocks = [0u16; 4096];
        blocks[block_index(0, 0, 1)] = 0xFF0F;
        let (id, meta) = unpack_block(&blocks, 0, 0, 1);
        assert_eq!(id, 0x0FF0);
        assert_eq!(meta, 0x0F);
    }
}
