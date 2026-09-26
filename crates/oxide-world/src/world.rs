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
