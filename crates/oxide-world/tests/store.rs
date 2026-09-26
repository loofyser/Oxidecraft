//! Tests for the chunk store: ground-up replacement, section updates, the
//! unload shape, light defaults, and coordinate handling.

use oxide_proto_v47::clientbound::{BulkColumn, ChunkData, MapChunkBulk};
use oxide_proto_v47::column::{ColumnData, SectionData, block_index};
use oxide_world::chunk::{Chunk, SECTION_COUNT, SECTION_SIZE, Section};
use oxide_world::world::World;

/// One section's wire data: `block` (a packed value) sits at the local origin,
/// the block-light nibble at index 0 is `block_light`, and every sky-light
/// nibble is `sky_light` when an array is given at all.
fn section_with(block: u16, block_light: u8, sky_light: Option<u8>) -> SectionData {
    let mut blocks = Box::new([0u16; 4096]);
    blocks[block_index(0, 0, 0)] = block;
    let mut light = Box::new([0u8; 2048]);
    light[0] = block_light & 0x0F;
    SectionData {
        blocks,
        block_light: light,
        sky_light: sky_light.map(|value| {
            let nibble = value & 0x0F;
            Box::new([(nibble << 4) | nibble; 2048])
        }),
    }
}

/// A column whose listed sections each carry one block: `(section index, id)`
/// puts the packed value `id << 4` at that section's local origin, with block
/// light zero and every sky-light nibble 15. The mask matches the listed
/// sections; the column carries no biomes.
fn column_with_sections(entries: &[(usize, u16)]) -> ColumnData {
    let mut column = ColumnData::empty();
    for &(index, id) in entries {
        column.sections[index] = Some(section_with(id << 4, 0, Some(15)));
        column.mask |= 1u16 << index;
    }
    column
}

#[test]
fn a_ground_up_column_turns_unlisted_sections_into_air() {
    let mut world = World::new(true);
    let first = column_with_sections(&[(0, 1), (3, 1)]); // id 1 in sections 0 and 3
    world.apply_column(0, 0, &first, true);
    assert_eq!(world.block(0, 0, 0) >> 4, 1);
    assert_eq!(world.block(0, (SECTION_SIZE * 3) as i32, 0) >> 4, 1);

    let second = column_with_sections(&[(5, 2)]);
    world.apply_column(0, 0, &second, true);
    assert_eq!(world.block(0, 0, 0), 0, "section 0 is air now");
    assert_eq!(
        world.block(0, (SECTION_SIZE * 3) as i32, 0),
        0,
        "section 3 is gone too"
    );
    assert_eq!(
        world.block(0, (SECTION_SIZE * 5) as i32, 0) >> 4,
        2,
        "section 5 carries the stone"
    );
    let chunk = world.chunk(0, 0).expect("loaded");
    assert_eq!(
        chunk.sky_light_at(0, 0, 0),
        15,
        "air keeps the sky-light default"
    );
    assert_eq!(chunk.sky_light_at(0, SECTION_SIZE * 3, 0), 15);
    assert_eq!(chunk.block_light_at(0, SECTION_SIZE * 3, 0), 0);
}

#[test]
fn a_section_update_replaces_only_the_listed_sections() {
    let mut world = World::new(true);
    let mut first = ColumnData::empty();
    first.sections[0] = Some(section_with(0x0010, 7, Some(12)));
    first.mask = 0x0001;
    world.apply_column(0, 0, &first, true);

    let mut update = ColumnData::empty();
    update.sections[5] = Some(section_with(0x0020, 0, Some(15)));
    update.mask = 1u16 << 5;
    world.apply_column(0, 0, &update, false);

    let chunk = world.chunk(0, 0).expect("loaded");
    assert_eq!(world.block(0, 0, 0) >> 4, 1, "section 0 keeps its block");
    assert_eq!(chunk.block_light_at(0, 0, 0), 7, "and its block light");
    assert_eq!(chunk.sky_light_at(0, 0, 0), 12, "and its sky light");
    assert_eq!(
        world.block(0, (SECTION_SIZE * 5) as i32, 0) >> 4,
        2,
        "section 5 is new"
    );
}

#[test]
fn sections_outside_the_mask_start_at_the_light_defaults() {
    let mut world = World::new(true);
    world.apply_column(0, 0, &column_with_sections(&[(5, 1)]), true);
    let chunk = world.chunk(0, 0).expect("loaded");
    assert_eq!(world.block(0, 0, 0), 0, "no blocks outside the mask");
    assert_eq!(
        chunk.block_light_at(0, 0, 0),
        0,
        "block light starts at zero"
    );
    assert_eq!(
        chunk.sky_light_at(0, 0, 0),
        15,
        "sky light starts at full strength"
    );
    assert_eq!(chunk.block_light_at(0, SECTION_SIZE * 15, 0), 0);
    assert_eq!(chunk.sky_light_at(0, SECTION_SIZE * 15, 0), 15);
}

#[test]
fn the_unload_shape_removes_the_column() {
    let mut world = World::new(true);
    world.apply_column(0, 0, &column_with_sections(&[(0, 1)]), true);
    assert_eq!(world.block(0, 0, 0) >> 4, 1);

    let removed = world.apply_column(0, 0, &ColumnData::empty(), true);
    assert!(!removed, "the unload shape reports false");
    assert!(world.chunk(0, 0).is_none(), "the column is gone");
    assert_eq!(world.block(0, 0, 0), 0, "and reads as air afterwards");
    assert!(world.loaded().is_empty());
}

#[test]
fn chunk_data_applies_and_the_unload_shape_removes() {
    let mut world = World::new(true);
    let packet = ChunkData {
        chunk_x: 0,
        chunk_z: 0,
        ground_up: true,
        mask: 0x0001,
        column: column_with_sections(&[(0, 1)]),
    };
    assert!(world.apply_chunk_data(&packet));
    assert_eq!(world.block(0, 0, 0) >> 4, 1);

    let unload = ChunkData {
        chunk_x: 0,
        chunk_z: 0,
        ground_up: true,
        mask: 0,
        column: ColumnData::empty(),
    };
    assert!(
        !world.apply_chunk_data(&unload),
        "the unload shape reports false"
    );
    assert!(world.chunk(0, 0).is_none());
    assert_eq!(world.block(0, 0, 0), 0);
}

#[test]
fn a_block_outside_the_loaded_area_is_air() {
    let mut world = World::new(true);
    world.apply_column(0, 0, &column_with_sections(&[(0, 1)]), true);
    assert_eq!(
        world.block(SECTION_SIZE as i32, 0, 0),
        0,
        "the next column is not loaded"
    );
    assert_eq!(
        world.block(0, (SECTION_COUNT * SECTION_SIZE) as i32, 0),
        0,
        "above the world"
    );
    assert_eq!(world.block(0, -1, 0), 0, "below the world");
    assert_eq!(
        world.block(0, (SECTION_COUNT * SECTION_SIZE) as i32 - 1, 0),
        0,
        "the top section is air"
    );
    assert_eq!(
        world.block(0, 0, -(SECTION_SIZE as i32)),
        0,
        "and the next column in z"
    );
}

#[test]
fn packed_values_round_trip() {
    let mut section = Section::air(true);
    section.set_block(1, 2, 3, 0x1234);
    assert_eq!(section.block(1, 2, 3), 0x1234);
    assert_eq!(section.block(1, 2, 3) >> 4, 0x123, "the full twelve-bit id");
    assert_eq!(section.block(1, 2, 3) & 0x0F, 4, "the metadata nibble");
    assert_eq!(section.block(1, 2, 4), 0, "and nothing beside it moved");
    section.set_block(15, 15, 15, 0xFFFF);
    assert_eq!(section.block(15, 15, 15), 0xFFFF);

    // The same packing survives the trip through the store.
    let mut world = World::new(true);
    world.apply_column(0, 0, &column_with_sections(&[(0, 0x123)]), true);
    assert_eq!(world.block(0, 0, 0), 0x1230);
    assert_eq!(world.block(0, 0, 0) >> 4, 0x123);
    assert_eq!(
        world.block(0, 0, 0) & 0x0F,
        0,
        "this value carries no metadata"
    );
}

#[test]
fn a_nether_column_has_no_sky_light() {
    let mut world = World::new(false);
    let mut column = ColumnData::empty();
    column.sections[0] = Some(section_with(0x0010, 4, None)); // no sky-light array
    column.mask = 0x0001;
    world.apply_column(0, 0, &column, true);

    assert!(!world.has_sky());
    let chunk = world.chunk(0, 0).expect("loaded");
    assert_eq!(
        chunk.sky_light_at(0, 0, 0),
        0,
        "a section with no array reads zero"
    );
    assert_eq!(
        chunk.sky_light_at(0, SECTION_SIZE * 7, 0),
        0,
        "so does an absent section"
    );
    assert_eq!(
        chunk.block_light_at(0, 0, 0),
        4,
        "block light still arrives"
    );

    // Even a column that carries a sky-light array answers zero: the dimension
    // has no sky.
    let mut second = ColumnData::empty();
    second.sections[0] = Some(section_with(0x0010, 0, Some(15)));
    second.mask = 0x0001;
    world.apply_column(0, 1, &second, true);
    let chunk = world.chunk(0, 1).expect("loaded");
    assert_eq!(
        chunk.sky_light_at(0, 0, 0),
        0,
        "sky light is dropped without sky"
    );
}

#[test]
fn a_bulk_applies_every_column() {
    let mut world = World::new(true);
    let packet = MapChunkBulk {
        sky_light: true,
        columns: vec![
            BulkColumn {
                chunk_x: 2,
                chunk_z: 3,
                mask: 0x0001,
                column: column_with_sections(&[(0, 1)]),
            },
            BulkColumn {
                chunk_x: 2,
                chunk_z: 4,
                mask: 0x0001,
                column: column_with_sections(&[(0, 2)]),
            },
        ],
    };
    assert_eq!(world.apply_bulk(&packet), 2);
    assert_eq!(
        world.block(2 * SECTION_SIZE as i32, 0, 3 * SECTION_SIZE as i32) >> 4,
        1
    );
    assert_eq!(
        world.block(2 * SECTION_SIZE as i32, 0, 4 * SECTION_SIZE as i32) >> 4,
        2
    );
    assert_eq!(world.loaded(), vec![(2, 3), (2, 4)]);
    let chunk = world.chunk(2, 4).expect("loaded");
    assert_eq!(
        (chunk.x(), chunk.z()),
        (2, 4),
        "the column knows where it is"
    );
}

#[test]
fn negative_coordinates_land_in_the_right_column() {
    let mut world = World::new(true);
    world.apply_column(-1, -1, &column_with_sections(&[(0, 1)]), true);
    assert_eq!(
        world.block(-(SECTION_SIZE as i32), 0, -(SECTION_SIZE as i32)) >> 4,
        1,
        "x = -16 is local 0 of chunk -1"
    );
    assert_eq!(
        world.block(-1, 0, -1),
        0,
        "the far corner of that column is air"
    );
    assert_eq!(world.block(-17, 0, -17), 0, "chunk -2 is not loaded");
    assert!(world.chunk(-1, -1).is_some());
    assert_eq!(world.loaded(), vec![(-1, -1)]);
}

#[test]
fn unloading_reports_whether_a_column_was_present() {
    let mut world = World::new(true);
    assert!(!world.unload(0, 0), "nothing is loaded yet");
    world.apply_column(0, 0, &column_with_sections(&[(0, 1)]), true);
    assert!(world.unload(0, 0), "the column was there");
    assert!(!world.unload(0, 0), "it is gone now");
}

#[test]
fn the_biome_array_is_replaced_only_when_the_packet_carries_one() {
    let mut world = World::new(true);
    let mut first = column_with_sections(&[(0, 1)]);
    first.biomes = Some([3u8; 256]);
    world.apply_column(0, 0, &first, true);
    assert_eq!(world.chunk(0, 0).expect("loaded").biome(1, 2), 3);

    // A section update carries no biome array; the old one stays.
    let update = column_with_sections(&[(5, 2)]);
    assert!(update.biomes.is_none());
    world.apply_column(0, 0, &update, false);
    assert_eq!(
        world.chunk(0, 0).expect("loaded").biome(1, 2),
        3,
        "left alone"
    );

    // A ground-up column that carries one replaces the whole array, indexed
    // (z << 4) | x like the wire.
    let mut second = ColumnData::empty();
    second.sections[0] = Some(section_with(0x0010, 0, Some(15)));
    second.mask = 0x0001;
    let mut biomes = [0u8; 256];
    biomes[block_index(1, 0, 2)] = 9;
    second.biomes = Some(biomes);
    world.apply_column(0, 0, &second, true);
    let chunk = world.chunk(0, 0).expect("loaded");
    assert_eq!(chunk.biome(1, 2), 9, "the biome index is (z << 4) | x");
    assert_eq!(chunk.biome(2, 1), 0, "and not the other way round");
}

#[test]
fn a_ground_up_column_that_carries_sections_is_applied_even_with_a_zero_mask() {
    // The store reads the slots the column carries, not the mask, so a column
    // built with sections and no mask still lands.
    let mut world = World::new(true);
    let mut column = ColumnData::empty();
    column.sections[0] = Some(section_with(0x0030, 0, Some(15)));
    assert_eq!(column.mask, 0);
    world.apply_column(0, 0, &column, true);
    assert_eq!(world.block(0, 0, 0) >> 4, 3);
}

#[test]
fn a_new_column_is_empty_and_light_does_not_fill_it() {
    let fresh = Chunk::new(5, -3, true);
    assert!(fresh.is_empty());
    assert_eq!((fresh.x(), fresh.z()), (5, -3));
    assert_eq!(fresh.biome(0, 0), 0, "a new column starts at biome zero");

    let mut world = World::new(true);
    let mut column = ColumnData::empty();
    column.sections[0] = Some(section_with(0, 7, Some(15))); // present, all air
    column.mask = 0x0001;
    world.apply_column(0, 0, &column, true);
    assert!(
        world.chunk(0, 0).expect("loaded").is_empty(),
        "a section of air is not a block"
    );

    world.apply_column(0, 0, &column_with_sections(&[(0, 1)]), true);
    assert!(!world.chunk(0, 0).expect("loaded").is_empty());
}
