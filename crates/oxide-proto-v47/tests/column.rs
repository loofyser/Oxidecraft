//! Tests for the column decoder: the size formula, nibble order, index order,
//! both packet shapes, and the hostile paths.

use oxide_proto_v47::PacketError;
use oxide_proto_v47::clientbound::{ChunkData, MapChunkBulk};
use oxide_proto_v47::column::{block_index, column_size, parse_column, unpack_nibble};

/// Builds one section's payload: blocks first, then block light, then sky light.
///
/// This is a whole column payload only for a single-section column, where the
/// section's three arrays are the column's three arrays.
fn build_section(block: u16, at: (usize, usize, usize)) -> Vec<u8> {
    let mut out = vec![0u8; 8192];
    let index = block_index(at.0, at.1, at.2);
    out[index * 2..index * 2 + 2].copy_from_slice(&block.to_le_bytes());
    out.extend_from_slice(&[0u8; 2048]); // block light
    out.extend_from_slice(&[0xFFu8; 2048]); // sky light: every nibble 15
    out
}

/// Builds one column's payload in the order the wire carries it: every
/// included section's block array, then their block-light arrays, then their
/// sky-light arrays, then the biomes.
///
/// `first_blocks` gives each section's block value at index 0; the rest of
/// every block array is air, block light is zero and, when `sky` is set, sky
/// light is 0xFF in every nibble.
fn build_column(first_blocks: &[u16], sky: bool, biomes: Option<u8>) -> Vec<u8> {
    let sections = first_blocks.len();
    let mut data = Vec::with_capacity(sections * 12_288 + 256);
    for value in first_blocks {
        let mut blocks = [0u8; 8192];
        blocks[0..2].copy_from_slice(&value.to_le_bytes());
        data.extend_from_slice(&blocks);
    }
    data.resize(data.len() + 2048 * sections, 0); // block light
    if sky {
        data.resize(data.len() + 2048 * sections, 0xFF); // sky light: every nibble 15
    }
    if let Some(value) = biomes {
        data.resize(data.len() + 256, value);
    }
    data
}

#[test]
fn the_size_formula_matches_the_spec_fixture() {
    // Mask 0x0001, overworld, ground-up: 12,544 bytes.
    assert_eq!(column_size(0x0001, true, true), 12_544);
    // Nether: no sky light array.
    assert_eq!(column_size(0x0001, false, true), 10_496);
    // A section update carries no biomes.
    assert_eq!(column_size(0x0001, true, false), 12_288);
    // The unload shape.
    assert_eq!(column_size(0x0000, true, true), 256);
}

#[test]
fn even_indices_are_the_low_nibble() {
    let mut array = [0u8; 2048];
    array[0] = 0xF3; // index 0 low = 3, index 1 high = 15
    assert_eq!(unpack_nibble(&array, 0), 3);
    assert_eq!(unpack_nibble(&array, 1), 15);
}

#[test]
fn the_index_order_is_x_then_z_then_y() {
    // (y << 8) | (z << 4) | x, x varying fastest.
    assert_eq!(block_index(0, 0, 0), 0);
    assert_eq!(block_index(15, 0, 0), 15);
    assert_eq!(block_index(0, 0, 1), 16);
    assert_eq!(block_index(0, 1, 0), 256);
    assert_eq!(block_index(15, 15, 15), 4095);
}

#[test]
fn a_single_section_decodes_into_the_right_slot() {
    let mut data = build_section(0x0017, (1, 2, 3)); // id 1, meta 7, at (1,2,3)
    data.extend_from_slice(&[0u8; 256]); // biomes
    let column = parse_column(&data, 0x0001, true, true).expect("decode");
    let section = column.sections[0].as_ref().expect("section 0 present");
    let value = section.blocks[block_index(1, 2, 3)];
    assert_eq!(value, 0x0017);
    assert_eq!(value >> 4, 1);
    assert_eq!(value & 0x0F, 7);
    assert_eq!(section.sky_light.as_ref().expect("sky")[0], 0xFF);
    assert!(
        column.sections[1].is_none(),
        "sections outside the mask stay absent"
    );
    assert!(column.biomes.is_some());
}

#[test]
fn sections_are_assigned_by_bit_index() {
    // Two sections, each with its block array in the column's wire order.
    let data = build_column(&[0x0001, 0x0002], true, Some(3));
    let column = parse_column(&data, 0x0003, true, true).expect("decode");
    assert!(column.sections[0].is_some());
    assert!(column.sections[1].is_some());
    assert_eq!(column.sections[0].as_ref().unwrap().blocks[0], 0x0001);
    assert_eq!(column.sections[1].as_ref().unwrap().blocks[0], 0x0002);
}

#[test]
fn a_nether_column_has_no_sky_light() {
    let mut data = build_section(0x0001, (0, 0, 0));
    data.truncate(8192 + 2048); // drop the sky-light half
    data.extend_from_slice(&[0u8; 256]);
    let column = parse_column(&data, 0x0001, false, true).expect("decode");
    assert!(column.sections[0].as_ref().unwrap().sky_light.is_none());
}

#[test]
fn a_size_that_does_not_match_the_mask_is_refused() {
    let data = vec![0u8; 100];
    let error = parse_column(&data, 0x0001, true, true).expect_err("100 bytes cannot be a column");
    let _ = error;
}

#[test]
fn a_truncated_column_is_refused() {
    let mut data = build_section(0x0001, (0, 0, 0));
    data.truncate(4000);
    assert!(parse_column(&data, 0x0001, true, true).is_err());
}

#[test]
fn chunk_data_decodes_the_unload_shape() {
    // Ground-up, mask 0, size 0: "this column is empty".
    let mut body = vec![0x21];
    body.extend_from_slice(&0i32.to_be_bytes());
    body.extend_from_slice(&0i32.to_be_bytes());
    body.push(1); // ground-up
    body.extend_from_slice(&0u16.to_be_bytes());
    oxide_proto::varint::write_varint(&mut body, 0).expect("size");
    // The sky flag is inert for the unload shape; an overworld caller passes true.
    let decoded = ChunkData::decode(&body[1..], true).expect("decode");
    assert_eq!(decoded.mask, 0);
    assert!(decoded.ground_up);
    assert_eq!(
        decoded
            .column
            .sections
            .iter()
            .filter(|s| s.is_some())
            .count(),
        0
    );
}

#[test]
fn map_chunk_bulk_reads_each_column_at_its_computed_offset() {
    // Two overworld columns, masks 0x000F and 0x0001, biomes always present.
    let mut body = vec![0x26, 1]; // sky-light sent, 2 columns
    oxide_proto::varint::write_varint(&mut body, 2).expect("count");
    for (cx, mask) in [(0i32, 0x000Fu16), (1, 0x0001)] {
        body.extend_from_slice(&cx.to_be_bytes());
        body.extend_from_slice(&0i32.to_be_bytes());
        body.extend_from_slice(&mask.to_be_bytes());
    }
    let mut payload = Vec::new();
    for mask in [0x000Fu16, 0x0001] {
        let first_blocks: Vec<u16> = (0..mask.count_ones())
            .map(|bitten| bitten as u16 + 1)
            .collect();
        payload.extend_from_slice(&build_column(&first_blocks, true, Some(5)));
    }
    body.extend_from_slice(&payload);
    let decoded = MapChunkBulk::decode(&body[1..]).expect("decode");
    assert!(decoded.sky_light);
    assert_eq!(decoded.columns.len(), 2);
    assert_eq!(decoded.columns[0].chunk_x, 0);
    assert_eq!(decoded.columns[0].mask, 0x000F);
    assert_eq!(decoded.columns[1].mask, 0x0001);
    // The second column starts where the first one ended: the first block there
    // is the section we wrote for it.
    assert_eq!(
        decoded.columns[1].column.sections[0]
            .as_ref()
            .unwrap()
            .blocks[0],
        1
    );
}

#[test]
fn chunk_data_decodes_a_ground_up_column() {
    // The full 0x21 shape: coordinates, flags, then the sized payload.
    let mut body = vec![0x21];
    body.extend_from_slice(&3i32.to_be_bytes());
    body.extend_from_slice(&(-2i32).to_be_bytes());
    body.push(1); // ground-up
    body.extend_from_slice(&0x0001u16.to_be_bytes());
    let mut payload = build_section(0x0021, (15, 0, 15)); // id 2, meta 1, far corner
    payload.extend_from_slice(&[1u8; 256]);
    oxide_proto::varint::write_varint(&mut body, payload.len() as i32).expect("size");
    body.extend_from_slice(&payload);
    let decoded = ChunkData::decode(&body[1..], true).expect("decode");
    assert_eq!((decoded.chunk_x, decoded.chunk_z), (3, -2));
    assert!(decoded.ground_up);
    let section = decoded.column.sections[0].as_ref().expect("section 0");
    assert_eq!(section.blocks[block_index(15, 0, 15)], 0x0021);
    assert_eq!(decoded.column.biomes, Some([1u8; 256]));
}

#[test]
fn chunk_data_refuses_a_size_that_overruns_the_body() {
    // The size field claims far more bytes than the payload carries.
    let mut body = vec![0x21];
    body.extend_from_slice(&0i32.to_be_bytes());
    body.extend_from_slice(&0i32.to_be_bytes());
    body.push(1);
    body.extend_from_slice(&0x0001u16.to_be_bytes());
    oxide_proto::varint::write_varint(&mut body, 20_000).expect("size");
    let error = ChunkData::decode(&body[1..], true).expect_err("the body is far too short");
    assert!(matches!(
        error,
        PacketError::BadColumnSize {
            got: 0,
            expected: 20_000
        }
    ));
}

#[test]
fn map_chunk_bulk_refuses_a_column_count_that_lies() {
    // The count claims a thousand columns; no metadata follows it.
    let mut body = vec![0x26, 0]; // no sky light
    oxide_proto::varint::write_varint(&mut body, 1_000).expect("count");
    let error = MapChunkBulk::decode(&body[1..]).expect_err("the metadata cannot fit");
    assert!(matches!(error, PacketError::Codec(_)));
}

#[test]
fn map_chunk_bulk_refuses_a_payload_that_ends_early() {
    // One column's data is a single byte short of its computed size.
    let mut body = vec![0x26, 1]; // sky light: the column needs 12,544 bytes
    oxide_proto::varint::write_varint(&mut body, 1).expect("count");
    body.extend_from_slice(&0i32.to_be_bytes());
    body.extend_from_slice(&0i32.to_be_bytes());
    body.extend_from_slice(&0x0001u16.to_be_bytes());
    body.extend_from_slice(&[0u8; 12_543]);
    let error = MapChunkBulk::decode(&body[1..]).expect_err("the column is short");
    assert!(matches!(
        error,
        PacketError::BadColumnSize {
            got: 12_543,
            expected: 12_544
        }
    ));
}
