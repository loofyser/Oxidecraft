//! Tests for the mesher: face counts, culling across section and chunk borders,
//! winding, and the palette.

use oxide_game::mesher::{build_column_meshes, build_section_mesh};
use oxide_game::palette::{Face, UNKNOWN_COLOR, block_color};
use oxide_world::chunk::Section;
use oxide_world::world::World;

/// A world with one column whose section 0 carries `blocks`, each entry a
/// `(x, y, z, id << 4 | meta)` tuple.
fn world_with(blocks: &[(usize, usize, usize, u16)]) -> World {
    let mut world = World::new(true);
    let mut column = oxide_proto_v47::column::ColumnData::empty();
    let mut section = Section::air(true);
    for (x, y, z, value) in blocks {
        section.set_block(*x, *y, *z, *value);
    }
    column.sections[0] = Some(oxide_world::chunk::data_from_section(&section));
    world.apply_column(0, 0, &column, true);
    world
}

#[test]
fn a_single_block_has_six_faces() {
    let world = world_with(&[(0, 0, 0, 0x0010)]); // stone at the section origin
    let mesh = build_section_mesh(&world, 0, 0, 0).expect("a mesh");
    assert_eq!(mesh.vertices.len(), 24, "six faces of four vertices");
    assert_eq!(mesh.indices.len(), 36, "six faces of two triangles");
    assert_eq!(&mesh.indices[..6], &[0, 1, 2, 0, 2, 3]);
}

#[test]
fn two_stacked_blocks_share_no_face() {
    let world = world_with(&[(0, 0, 0, 0x0010), (0, 1, 0, 0x0010)]);
    let mesh = build_section_mesh(&world, 0, 0, 0).expect("a mesh");
    // Twelve faces less the two that touch: ten.
    assert_eq!(mesh.vertices.len(), 10 * 4);
}

#[test]
fn a_neighbour_across_a_section_border_culls_the_face() {
    // Section 1, at the matching position, sees the section-0 block below.
    let mut column = oxide_proto_v47::column::ColumnData::empty();
    let mut section = Section::air(true);
    section.set_block(0, 0, 0, 0x0010);
    column.sections[1] = Some(oxide_world::chunk::data_from_section(&section));
    let mut world2 = world_with(&[(0, 15, 0, 0x0010)]);
    world2.apply_column(0, 0, &column, false);
    let mesh = build_section_mesh(&world2, 0, 0, 1).expect("a mesh");
    assert_eq!(mesh.vertices.len(), 5 * 4, "the bottom face is culled");
}

#[test]
fn a_neighbour_across_a_chunk_border_culls_the_face() {
    let mut world = World::new(true);
    let mut left = oxide_proto_v47::column::ColumnData::empty();
    let mut section = Section::air(true);
    section.set_block(15, 0, 0, 0x0010);
    left.sections[0] = Some(oxide_world::chunk::data_from_section(&section));
    world.apply_column(0, 0, &left, true);
    let mut right = oxide_proto_v47::column::ColumnData::empty();
    let mut section = Section::air(true);
    section.set_block(0, 0, 0, 0x0010);
    right.sections[0] = Some(oxide_world::chunk::data_from_section(&section));
    world.apply_column(1, 0, &right, true);

    let mesh = build_section_mesh(&world, 0, 0, 0).expect("a mesh");
    assert_eq!(
        mesh.vertices.len(),
        5 * 4,
        "the east face meets its neighbour"
    );
}

#[test]
fn an_empty_section_builds_nothing() {
    let world = World::new(true);
    assert!(build_section_mesh(&world, 0, 0, 0).is_none());
}

#[test]
fn a_column_reports_every_section_slot() {
    let world = world_with(&[(0, 0, 0, 0x0010)]);
    let meshes = build_column_meshes(&world, 0, 0);
    assert_eq!(meshes.len(), 16);
    assert!(meshes[0].1.is_some(), "section 0 draws");
    assert!(meshes[1].1.is_none(), "section 1 is empty");
}

#[test]
fn every_face_winds_counter_clockwise_from_outside() {
    for face in Face::ALL {
        // The corner table lives in the mesher; a face whose first triangle
        // winds the other way would be culled by the pipeline.
        let corners = oxide_game::mesher::face_corners(face);
        let [a, b, c, _] = corners;
        let first = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let second = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cross = [
            first[1] * second[2] - first[2] * second[1],
            first[2] * second[0] - first[0] * second[2],
            first[0] * second[1] - first[1] * second[0],
        ];
        let normal = face.normal();
        let dot = cross[0] * normal[0] + cross[1] * normal[1] + cross[2] * normal[2];
        assert!(dot > 0.5, "{face:?} winds the wrong way: {cross:?}");
    }
}

#[test]
fn the_palette_colours_the_common_blocks() {
    let grass_top = block_color(2, 0, Face::Top);
    assert!(
        grass_top[1] > grass_top[0] && grass_top[1] > grass_top[2],
        "grass top is green"
    );
    let grass_side = block_color(2, 0, Face::North);
    assert!(grass_side[0] > grass_side[2], "the side is earthy");
    let stone = block_color(1, 0, Face::Top);
    assert!(
        (stone[0] - stone[1]).abs() < 0.01 && (stone[1] - stone[2]).abs() < 0.01,
        "stone is grey"
    );
    assert_eq!(
        block_color(19_999, 0, Face::Top),
        UNKNOWN_COLOR,
        "an unknown id is loud"
    );
}

#[test]
fn the_palette_covers_every_id_the_rig_world_carries() {
    // Every non-air id the rig's saved world carries within the acceptance
    // area (a scan of refs/rig/server/parity/region, all chunks within 18 of
    // the spawn chunk (1, 10)). The acceptance frames must show no magenta
    // below y=128, and magenta is the missing-entry colour; air never reaches
    // the palette.
    const WORLD_IDS: [u16; 55] = [
        1, 2, 3, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 21, 24, 31, 32, 35, 37, 38, 39,
        40, 43, 48, 49, 50, 52, 53, 54, 56, 58, 59, 60, 64, 65, 67, 72, 73, 81, 82, 83, 85, 86, 99,
        100, 102, 141, 142, 161, 162, 175,
    ];
    for id in WORLD_IDS {
        for face in Face::ALL {
            assert_ne!(
                block_color(id, 0, face),
                UNKNOWN_COLOR,
                "id {id} has no palette entry"
            );
        }
    }
    // The two ids the frames lean on most after the terrain itself: the
    // acacia and dark oak leaves the scan found most of, and the flowers.
    let acacia_leaves = block_color(161, 0, Face::Top);
    assert!(
        acacia_leaves[1] > acacia_leaves[0] && acacia_leaves[1] > acacia_leaves[2],
        "acacia leaves are green"
    );
    let dandelion = block_color(37, 0, Face::Top);
    assert!(
        dandelion[0] > 0.8 && dandelion[1] > 0.8 && dandelion[2] < 0.4,
        "a dandelion is yellow"
    );
}

#[test]
fn brightness_is_baked_into_the_vertex_colour() {
    let world = world_with(&[(0, 0, 0, 0x0010)]);
    let mesh = build_section_mesh(&world, 0, 0, 0).expect("a mesh");
    let base = block_color(1, 0, Face::Top);
    let top_vertex = mesh
        .vertices
        .iter()
        .find(|vertex| vertex.position[1] == 1.0)
        .expect("a top vertex");
    assert!(
        (top_vertex.color[0] - base[0]).abs() < 1e-5,
        "top brightness is 1.0"
    );
    let bottom_vertex = mesh
        .vertices
        .iter()
        .find(|vertex| vertex.position[1] == 0.0)
        .expect("a bottom vertex");
    assert!(
        (bottom_vertex.color[0] - base[0] * 0.5).abs() < 1e-5,
        "bottom brightness is 0.5"
    );
}
