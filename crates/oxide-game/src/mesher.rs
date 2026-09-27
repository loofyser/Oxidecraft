//! Turning world sections into terrain geometry: one quad per visible face.
//!
//! A face is emitted when the neighbouring block is air, so faces are culled
//! across section borders and across chunk borders alike: the neighbour is read
//! through [`World::block`], which spans the whole loaded world. The 4096 block
//! positions of a section are visited in the section-linear order that
//! `oxide_proto_v47::column::block_index` lays out (x fastest, then z, then y),
//! so the mesher does no index arithmetic of its own.
//!
//! Each face's corners wind counter-clockwise seen from outside the block, and
//! its colour is the palette entry with the face's brightness already
//! multiplied in. M1 has one pass and no light: a mesh carries geometry and
//! colours only.

use oxide_render::terrain::{ChunkMesh, Vertex};
use oxide_world::chunk::{SECTION_COUNT, SECTION_SIZE};
use oxide_world::world::World;

use crate::palette::{Face, block_color};

/// The four corners of one face, counter-clockwise seen from outside the block.
///
/// The winding is load-bearing: the pipeline culls back faces, so a face wound
/// the other way disappears. The test suite asserts every face's cross product
/// points along its normal.
pub fn face_corners(face: Face) -> [[f32; 3]; 4] {
    match face {
        Face::Top => [
            [0.0, 1.0, 0.0],
            [0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 0.0],
        ],
        Face::Bottom => [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
        ],
        Face::North => [
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
        ],
        Face::South => [
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ],
        Face::West => [
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
        ],
        Face::East => [
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 1.0, 1.0],
            [1.0, 0.0, 1.0],
        ],
    }
}

/// Builds one section's mesh, or `None` when the section draws nothing.
///
/// `cx` and `cz` are chunk coordinates and `sy` the section index, 0..16; a
/// section index outside that range has no block slots and builds nothing.
/// Positions in the mesh are world coordinates.
pub fn build_section_mesh(world: &World, cx: i32, cz: i32, sy: usize) -> Option<ChunkMesh> {
    if sy >= SECTION_COUNT {
        return None;
    }
    let origin_x = cx * SECTION_SIZE as i32;
    let origin_y = (sy * SECTION_SIZE) as i32;
    let origin_z = cz * SECTION_SIZE as i32;
    let mut mesh = ChunkMesh::default();
    for y in 0..SECTION_SIZE {
        for z in 0..SECTION_SIZE {
            for x in 0..SECTION_SIZE {
                let block_x = origin_x + x as i32;
                let block_y = origin_y + y as i32;
                let block_z = origin_z + z as i32;
                let value = world.block(block_x, block_y, block_z);
                let id = value >> 4;
                if id == 0 {
                    continue;
                }
                let meta = (value & 0x0F) as u8;
                for face in Face::ALL {
                    let (dx, dy, dz) = face.offset();
                    if world.block(block_x + dx, block_y + dy, block_z + dz) >> 4 != 0 {
                        continue;
                    }
                    let base = mesh.vertices.len() as u32;
                    let brightness = face.brightness();
                    let color = block_color(id, meta, face);
                    for corner in face_corners(face) {
                        mesh.vertices.push(Vertex {
                            position: [
                                block_x as f32 + corner[0],
                                block_y as f32 + corner[1],
                                block_z as f32 + corner[2],
                            ],
                            color: [
                                color[0] * brightness,
                                color[1] * brightness,
                                color[2] * brightness,
                            ],
                        });
                    }
                    mesh.indices.extend_from_slice(&[
                        base,
                        base + 1,
                        base + 2,
                        base,
                        base + 2,
                        base + 3,
                    ]);
                }
            }
        }
    }
    if mesh.is_empty() { None } else { Some(mesh) }
}

/// Builds every section of a column: index `sy`, `None` for a section that
/// draws nothing, so the caller can remove a mesh that is now empty.
pub fn build_column_meshes(world: &World, cx: i32, cz: i32) -> Vec<(usize, Option<ChunkMesh>)> {
    (0..SECTION_COUNT)
        .map(|sy| (sy, build_section_mesh(world, cx, cz, sy)))
        .collect()
}
