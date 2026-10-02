//! The world store: columns by chunk coordinate, and the rules that turn a
//! decoded column into stored world data.

use std::collections::HashMap;

use oxide_proto_v47::clientbound::{ChunkData, MapChunkBulk};
use oxide_proto_v47::column::ColumnData;

use crate::chunk::{Chunk, SECTION_COUNT, SECTION_SIZE};

/// The client's chunk store.
#[derive(Debug)]
pub struct World {
    /// Whether the dimension has sky light.
    has_sky: bool,
    /// The loaded columns, keyed by chunk coordinate.
    chunks: HashMap<(i32, i32), Chunk>,
}

impl World {
    /// A world for a dimension with or without sky light.
    pub fn new(has_sky: bool) -> Self {
        Self {
            has_sky,
            chunks: HashMap::new(),
        }
    }

    /// Whether the dimension has sky light.
    pub fn has_sky(&self) -> bool {
        self.has_sky
    }

    /// Applies a decoded column to its chunk, creating the chunk when it is new.
    ///
    /// The unload shape — a ground-up column that selects no sections — removes
    /// the column and reports `false`; anything else reports `true`. The slots
    /// the column carries decide: a ground-up column that still carries
    /// sections is applied, whatever its mask says. This is the entry point the
    /// packet-facing calls below share.
    pub fn apply_column(&mut self, cx: i32, cz: i32, data: &ColumnData, ground_up: bool) -> bool {
        if ground_up && data.mask == 0 && data.sections.iter().all(Option::is_none) {
            self.unload(cx, cz);
            return false;
        }
        let has_sky = self.has_sky;
        let chunk = self
            .chunks
            .entry((cx, cz))
            .or_insert_with(|| Chunk::new(cx, cz, has_sky));
        chunk.apply(data, ground_up, has_sky);
        true
    }

    /// Applies Chunk Data. The unload shape (ground-up, empty mask) removes the
    /// column and reports `false`; anything else reports `true`.
    pub fn apply_chunk_data(&mut self, packet: &ChunkData) -> bool {
        self.apply_column(
            packet.chunk_x,
            packet.chunk_z,
            &packet.column,
            packet.ground_up,
        )
    }

    /// Applies Map Chunk Bulk; returns how many columns it carried.
    ///
    /// A bulk column is a ground-up full send, so each one replaces its chunk.
    pub fn apply_bulk(&mut self, packet: &MapChunkBulk) -> usize {
        for column in &packet.columns {
            self.apply_column(column.chunk_x, column.chunk_z, &column.column, true);
        }
        packet.columns.len()
    }

    /// The column at those chunk coordinates, if it is loaded.
    pub fn chunk(&self, cx: i32, cz: i32) -> Option<&Chunk> {
        self.chunks.get(&(cx, cz))
    }

    /// The packed block at world coordinates; air (0) when the column is not
    /// loaded or y is outside 0..256.
    pub fn block(&self, x: i32, y: i32, z: i32) -> u16 {
        if !(0..(SECTION_COUNT * SECTION_SIZE) as i32).contains(&y) {
            return 0;
        }
        let cx = x.div_euclid(SECTION_SIZE as i32);
        let cz = z.div_euclid(SECTION_SIZE as i32);
        let local_x = x.rem_euclid(SECTION_SIZE as i32) as usize;
        let local_z = z.rem_euclid(SECTION_SIZE as i32) as usize;
        match self.chunks.get(&(cx, cz)) {
            Some(chunk) => chunk.block(local_x, y as usize, local_z),
            None => 0,
        }
    }

    /// Sets the packed block at world coordinates, answering the previous
    /// value.
    ///
    /// The write lands in the column's section store; a section the column
    /// does not hold is materialised first as its air defaults (block light 0,
    /// sky light 15 when the dimension has sky light — the store's own
    /// materialisation rule, the one the column pass writes back with).
    ///
    /// `None` when the column is not loaded or y is outside 0..256: there is
    /// no slot to write and nothing changes. The previous value is what the
    /// cell read before the write — air (0) for a cell of a materialised
    /// section.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, value: u16) -> Option<u16> {
        if !(0..(SECTION_COUNT * SECTION_SIZE) as i32).contains(&y) {
            return None;
        }
        let cx = x.div_euclid(SECTION_SIZE as i32);
        let cz = z.div_euclid(SECTION_SIZE as i32);
        let local_x = x.rem_euclid(SECTION_SIZE as i32) as usize;
        let local_z = z.rem_euclid(SECTION_SIZE as i32) as usize;
        let chunk = self.chunks.get_mut(&(cx, cz))?;
        chunk.set_block(local_x, y as usize, local_z, value)
    }

    /// The block-light nibble at world coordinates; 0 when the column is not
    /// loaded or y is outside 0..256.
    ///
    /// The point read the column snapshot's border strips and any caller
    /// outside the mesher use; a stored section the column does not hold
    /// answers 0.
    pub fn block_light(&self, x: i32, y: i32, z: i32) -> u8 {
        if !(0..(SECTION_COUNT * SECTION_SIZE) as i32).contains(&y) {
            return 0;
        }
        let cx = x.div_euclid(SECTION_SIZE as i32);
        let cz = z.div_euclid(SECTION_SIZE as i32);
        let local_x = x.rem_euclid(SECTION_SIZE as i32) as usize;
        let local_z = z.rem_euclid(SECTION_SIZE as i32) as usize;
        match self.chunks.get(&(cx, cz)) {
            Some(chunk) => chunk.block_light_at(local_x, y as usize, local_z),
            None => 0,
        }
    }

    /// The sky-light nibble at world coordinates; 0 when the column is not
    /// loaded or y is outside 0..256.
    ///
    /// A loaded column answers its store's own value, which is 15 for an
    /// absent section of a dimension with sky — the store's documented
    /// default, not a second rule here.
    pub fn sky_light(&self, x: i32, y: i32, z: i32) -> u8 {
        if !(0..(SECTION_COUNT * SECTION_SIZE) as i32).contains(&y) {
            return 0;
        }
        let cx = x.div_euclid(SECTION_SIZE as i32);
        let cz = z.div_euclid(SECTION_SIZE as i32);
        let local_x = x.rem_euclid(SECTION_SIZE as i32) as usize;
        let local_z = z.rem_euclid(SECTION_SIZE as i32) as usize;
        match self.chunks.get(&(cx, cz)) {
            Some(chunk) => chunk.sky_light_at(local_x, y as usize, local_z),
            None => 0,
        }
    }

    /// Removes a column, reporting whether one was present.
    pub fn unload(&mut self, cx: i32, cz: i32) -> bool {
        self.chunks.remove(&(cx, cz)).is_some()
    }

    /// The loaded chunk coordinates, for tests and diagnostics, in ascending
    /// order.
    pub fn loaded(&self) -> Vec<(i32, i32)> {
        let mut coords: Vec<(i32, i32)> = self.chunks.keys().copied().collect();
        coords.sort_unstable();
        coords
    }
}

#[cfg(test)]
mod tests {
    //! The store's block write: the round trip, the sparse materialisation
    //! rule, the chunk border and the build-height guard.

    use oxide_proto_v47::column::ColumnData;

    use super::World;

    /// Stone, id 1, no metadata.
    const STONE: u16 = 1 << 4;
    /// Glowstone, id 89, no metadata.
    const GLOWSTONE: u16 = 89 << 4;

    /// A world with one loaded but section-less column at (0, 0): the sparse
    /// shape a partial send leaves, where every read answers the absent
    /// section's own default.
    fn sparse_world(has_sky: bool) -> World {
        let mut world = World::new(has_sky);
        world.apply_column(0, 0, &ColumnData::empty(), false);
        world
    }

    #[test]
    fn a_write_round_trips_and_answers_the_previous_value() {
        let mut world = sparse_world(true);
        assert_eq!(
            world.set_block(0, 70, 0, STONE),
            Some(0),
            "the previous value is air"
        );
        assert_eq!(world.block(0, 70, 0), STONE, "the write reads back");
        assert_eq!(
            world.set_block(0, 70, 0, GLOWSTONE),
            Some(STONE),
            "the next write answers the value it replaced"
        );
        assert_eq!(world.block(0, 70, 0), GLOWSTONE, "and lands");
    }

    #[test]
    fn a_write_crosses_a_chunk_border() {
        let mut world = sparse_world(true);
        world.apply_column(1, 0, &ColumnData::empty(), false);
        world.apply_column(-1, 0, &ColumnData::empty(), false);

        assert_eq!(
            world.set_block(15, 70, 0, STONE),
            Some(0),
            "the chunk's last local x"
        );
        assert_eq!(
            world.set_block(16, 70, 0, GLOWSTONE),
            Some(0),
            "the next chunk's first local x"
        );
        assert_eq!(
            world.set_block(-1, 70, 0, GLOWSTONE),
            Some(0),
            "and the negative side's last local x"
        );
        assert_eq!(world.block(15, 70, 0), STONE);
        assert_eq!(world.block(16, 70, 0), GLOWSTONE);
        assert_eq!(world.block(-1, 70, 0), GLOWSTONE);
    }

    #[test]
    fn a_write_materialises_the_absent_section_as_its_air_defaults() {
        let mut world = sparse_world(true);
        assert_eq!(
            world.sky_light(0, 40, 0),
            15,
            "an absent section reads its sky default"
        );
        assert_eq!(world.block_light(0, 40, 0), 0, "and its block light");
        assert_eq!(world.block(0, 40, 0), 0, "and air");
        let neighbour_before = (
            world.block(1, 40, 0),
            world.sky_light(1, 40, 0),
            world.block_light(1, 40, 0),
        );

        assert_eq!(
            world.set_block(0, 40, 0, STONE),
            Some(0),
            "the write lands on the absent section's air"
        );

        assert_eq!(world.block(0, 40, 0), STONE, "the change");
        assert_eq!(
            (
                world.block(1, 40, 0),
                world.sky_light(1, 40, 0),
                world.block_light(1, 40, 0)
            ),
            neighbour_before,
            "its neighbour reads exactly as the air-with-light defaults did"
        );
        assert_eq!(
            world.sky_light(1, 40, 0),
            15,
            "which is sky 15 in a dimension with sky"
        );
        assert_eq!(world.block_light(1, 40, 0), 0, "and block light 0");
        assert_eq!(world.block(1, 40, 0), 0, "and air");
    }

    #[test]
    fn a_write_outside_the_build_range_is_rejected() {
        let mut world = sparse_world(true);
        assert_eq!(world.set_block(0, 256, 0, STONE), None, "above the world");
        assert_eq!(world.set_block(0, -1, 0, STONE), None, "below the world");
        assert_eq!(world.block(0, 255, 0), 0, "nothing was written");
        assert_eq!(world.block(0, 0, 0), 0, "and nothing below");
    }

    #[test]
    fn a_write_into_an_unloaded_column_is_rejected() {
        let mut world = sparse_world(true);
        assert_eq!(
            world.set_block(64, 70, 0, STONE),
            None,
            "there is no column to write into"
        );
        assert_eq!(world.block(64, 70, 0), 0, "and nothing changed");
    }

    #[test]
    fn a_write_into_a_dimension_without_sky_keeps_the_sky_dark() {
        let mut world = sparse_world(false);
        assert_eq!(
            world.set_block(0, 40, 0, GLOWSTONE),
            Some(0),
            "the write lands"
        );
        assert_eq!(world.block(0, 40, 0), GLOWSTONE);
        assert_eq!(
            world.sky_light(1, 40, 0),
            0,
            "the materialised section carries no sky store"
        );
        assert_eq!(world.block_light(1, 40, 0), 0, "and no block light");
    }
}
