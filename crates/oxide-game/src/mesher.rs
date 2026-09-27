//! The model-driven mesh core: a column's blocks to one mesh per section.
//!
//! A build reads one [`ColumnSnapshot`] and a [`MeshContext`] and nothing else —
//! no world, no lock — so a worker can own the pair and a pool can run it off
//! the session's thread. The snapshot is the store's own answer for the column
//! and its collar; the context carries the baked models, the atlas, the colour
//! maps and the two graphics settings.
//!
//! For every cell with a block, the packed id and metadata pick a model
//! ([`BlockModelSet::model`], position-dependent for a weighted variant array).
//! Each of the model's quads is drawn unless its declared `cullface` meets a
//! neighbour whose `occludes` column is true — the source's
//! `Block.shouldSideBeRendered` base rule, `block/Block.java:468-471` — and its
//! four vertices take the quad's atlas uv, the face's shade times the biome
//! tint where the face carries a tint index, and the light of the cells the
//! face looks into.
//!
//! # The light
//!
//! `BlockModelRenderer.java:32` picks between two paths. The ambient-occlusion
//! path runs when the smooth-lighting setting is not `Off`, the block emits no
//! light and the model's `ambientocclusion` is true; it samples cells around
//! the face per vertex and averages them, with the `0 → fourth` substitution
//! `getAoBrightness` performs (`:506-524`) and the plain four-sample branch at
//! `:488-504`. Every other quad — the standard path of `:98-101` and `:245-296`
//! — gives all four vertices the light of one cell: the neighbour the face looks
//! into for a quad with a `cullface`, the block's own cell for one without (a
//! cross model's quads; this project's baked quad does not keep the element face
//! key, which is known limit 12).
//!
//! `SmoothLighting::Minimum` and `SmoothLighting::Maximum` are the same path in
//! this version: the renderer reads only `Minecraft.isAmbientOcclusionEnabled()`
//! (`Minecraft.java:2491-2493`), which is the setting not being `Off`.
//!
//! # Known interim state
//!
//! Liquids and special render layers are Task 9's: this core draws whichever
//! quads a block's resolved model has, in the single [`ChunkMesh`] buffer M1
//! used. A state with no model — an id outside the behaviour table, a state the
//! model set could not resolve, the client's built-in blocks — draws the magenta
//! fallback cube from the atlas's own fallback sprite.

mod models;
mod snapshot;

pub use models::{BlockModelSet, ModelChoice, blockstate_target};
pub use snapshot::{Border, ColumnSnapshot};

use oxide_assets::atlas::Atlas;
use oxide_assets::model::{BakedModel, BakedQuad, FaceDir};
use oxide_render::terrain::{ChunkMesh, Vertex};
use oxide_world::behaviour::{BlockBehaviour, TintKind, behaviour};
use oxide_world::biome::{TintMaps, tint_at_9_biome};
use oxide_world::chunk::{SECTION_COUNT, SECTION_SIZE};

use crate::palette::Face;

/// Which light path a quad's vertices take.
///
/// The client's `ambientOcclusion` setting. `Off` takes the standard path;
/// `Minimum` and `Maximum` both take the ambient-occlusion path, because 1.8.9's
/// renderer never tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmoothLighting {
    /// Ambient occlusion off.
    Off,
    /// Ambient occlusion, the setting's first step.
    Minimum,
    /// Ambient occlusion, the setting's second step.
    Maximum,
}

/// Everything a build reads besides the snapshot.
#[derive(Debug)]
pub struct MeshContext<'a> {
    /// The block states' baked models.
    pub models: &'a BlockModelSet,
    /// The atlas the quads' sprite names resolve against.
    pub atlas: &'a Atlas,
    /// The grass and foliage colour maps.
    pub tint_maps: &'a TintMaps,
    /// Whether the graphics setting is Fast. Task 9's leaves rule reads it; this
    /// core carries it without using it.
    pub graphics_fast: bool,
    /// Which light path the build takes.
    pub smooth_lighting: SmoothLighting,
}

/// Builds every section of a column: one `(section index, mesh)` per section,
/// the mesh `None` where the section drew no quads.
pub fn build_column_meshes(
    snapshot: &ColumnSnapshot,
    ctx: &MeshContext<'_>,
) -> Vec<(usize, Option<ChunkMesh>)> {
    (0..SECTION_COUNT)
        .map(|section| (section, build_section_mesh(snapshot, section, ctx)))
        .collect()
}

/// Builds one section's mesh, or `None` when it has no quads.
///
/// `section` is the section index, 0..16; its cells are the column's cells at
/// `y` 16 × `section` .. 16 × `section + 15`.
pub fn build_section_mesh(
    snapshot: &ColumnSnapshot,
    section: usize,
    ctx: &MeshContext<'_>,
) -> Option<ChunkMesh> {
    let mut mesh = ChunkMesh::default();
    let base = section * SECTION_SIZE;
    for y in base..base + SECTION_SIZE {
        for z in 0..SECTION_SIZE {
            for x in 0..SECTION_SIZE {
                append_block(&mut mesh, snapshot, (x as i32, y as i32, z as i32), ctx);
            }
        }
    }
    if mesh.is_empty() { None } else { Some(mesh) }
}

/// Appends one cell's geometry to the section's mesh.
fn append_block(
    mesh: &mut ChunkMesh,
    snapshot: &ColumnSnapshot,
    position: (i32, i32, i32),
    ctx: &MeshContext<'_>,
) {
    let value = snapshot.block(position.0, position.1, position.2);
    let id = value >> 4;
    if id == 0 {
        return;
    }
    let meta = (value & 0x0F) as u8;
    let block = behaviour(id);
    match ctx
        .models
        .model(id, meta, position.0, position.1, position.2)
    {
        ModelChoice::Model(model) => append_model(mesh, snapshot, position, block, model, ctx),
        ModelChoice::Missing => append_fallback(mesh, snapshot, position, ctx),
    }
}

/// Appends every quad of a resolved model, in the model's own order.
fn append_model(
    mesh: &mut ChunkMesh,
    snapshot: &ColumnSnapshot,
    position: (i32, i32, i32),
    block: Option<&BlockBehaviour>,
    model: &BakedModel,
    ctx: &MeshContext<'_>,
) {
    for quad in &model.quads {
        if let Some(cull) = quad.cullface {
            let (dx, dy, dz) = offset(cull);
            let neighbour = snapshot.block(position.0 + dx, position.1 + dy, position.2 + dz) >> 4;
            if behaviour(neighbour).is_some_and(|entry| entry.occludes) {
                continue;
            }
        }
        append_quad(mesh, snapshot, position, block, quad, model, ctx);
    }
}

/// Appends one quad: four vertices, then the two triangles over them.
fn append_quad(
    mesh: &mut ChunkMesh,
    snapshot: &ColumnSnapshot,
    position: (i32, i32, i32),
    block: Option<&BlockBehaviour>,
    quad: &BakedQuad,
    model: &BakedModel,
    ctx: &MeshContext<'_>,
) {
    let face = quad_face(quad);
    // The cell the quad's light comes from: the neighbour the face looks into,
    // or the block's own cell for a quad with no face to look through.
    let sample = match quad.cullface {
        Some(cull) => {
            let (dx, dy, dz) = offset(cull);
            (position.0 + dx, position.1 + dy, position.2 + dz)
        }
        None => position,
    };
    let centre = cell_pair(snapshot, sample, block);
    let emission = block.map_or(0, |entry| entry.light_emission);
    let pairs = if take_ambient_occlusion(ctx, emission, model) {
        ambient_pairs(snapshot, position, quad, face, centre)
    } else {
        [centre; 4]
    };
    let uvs = atlas_uv(ctx, quad);
    let colour = vertex_colour(block, quad, face, snapshot, position, ctx);
    let base = mesh.vertices.len() as u32;
    for (index, corner) in quad.corners.iter().enumerate() {
        mesh.vertices.push(Vertex {
            position: [
                position.0 as f32 + corner[0],
                position.1 as f32 + corner[1],
                position.2 as f32 + corner[2],
            ],
            uv: uvs[index],
            light: light_attribute(pairs[index]),
            colour,
        });
    }
    mesh.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// Whether a quad's vertices take the ambient-occlusion path.
///
/// `BlockModelRenderer.java:32`'s condition: the setting is not `Off`, the block
/// emits no light of its own, and the model asks for ambient occlusion.
fn take_ambient_occlusion(ctx: &MeshContext<'_>, emission: u8, model: &BakedModel) -> bool {
    ctx.smooth_lighting != SmoothLighting::Off && emission == 0 && model.ambient_occlusion
}

/// The atlas uv of a quad's four corners: the model's own sprite coordinates
/// mapped into the sprite's content rect.
///
/// The rect is [`Atlas::uv`]'s answer, already inside the level-0 image and
/// inside the one-texel padding; a texture the tree never stitched falls back to
/// the fallback sprite's rect, exactly as the missing model does.
fn atlas_uv(ctx: &MeshContext<'_>, quad: &BakedQuad) -> [[f32; 2]; 4] {
    let [min, max] = match ctx.atlas.sprites.get(&quad.texture) {
        Some(sprite) => ctx.atlas.uv(sprite),
        None => ctx.atlas.uv(&ctx.atlas.missing),
    };
    std::array::from_fn(|corner| {
        [
            min[0] + quad.uv[corner][0] * (max[0] - min[0]),
            min[1] + quad.uv[corner][1] * (max[1] - min[1]),
        ]
    })
}

/// The vertex colour: the block's tint, or white, times the face's shade.
///
/// The tint is sampled once per tinted face, at the block's own position, from
/// the nine-sample average the source's tint path consumes — `BlockModelRenderer`
/// gets the colour per quad from `BlockColors` (`BiomeColorHelper.getGrassColorAtPos`
/// and its siblings average the nine cells around the position, `world/BiomeColorHelper.java`),
/// and `oxide-world`'s [`tint_at_9_biome`] is that average over the snapshot's
/// biomes. A face with no tint index takes no tint at all — the client tints
/// nothing else — and a block whose kind is `None` is white.
///
/// The shade is the face's own brightness, or none at all for a model face that
/// turned shading off (`cross`'s quads). The bytes are the source's truncating
/// conversion, `(int)(channel * 255)`.
fn vertex_colour(
    block: Option<&BlockBehaviour>,
    quad: &BakedQuad,
    face: Face,
    snapshot: &ColumnSnapshot,
    position: (i32, i32, i32),
    ctx: &MeshContext<'_>,
) -> [u8; 4] {
    let tint = match block {
        Some(entry) if quad.tintindex.is_some() && entry.tint != TintKind::None => tint_at_9_biome(
            ctx.tint_maps,
            position.0,
            position.1,
            position.2,
            entry.tint,
            |x, z| snapshot.biome_at(x, z),
        ),
        _ => [255, 255, 255],
    };
    let brightness = if quad.shade { face.brightness() } else { 1.0 };
    let channel = |index: usize| (f32::from(tint[index]) * brightness) as u8;
    [channel(0), channel(1), channel(2), 255]
}

/// The fallback cube: the six faces of a block drawn from the atlas's fallback
/// sprite.
///
/// The cull rule is the one the models get, and the vertices take the face's
/// shade and the light of the neighbour the face looks into — the form M1's
/// palette mesher gave an id it could not colour, with the magenta coming from
/// the sprite now rather than from the vertex.
fn append_fallback(
    mesh: &mut ChunkMesh,
    snapshot: &ColumnSnapshot,
    position: (i32, i32, i32),
    ctx: &MeshContext<'_>,
) {
    let rect = ctx.atlas.uv(&ctx.atlas.missing);
    for face in Face::ALL {
        let (dx, dy, dz) = face.offset();
        let neighbour = snapshot.block(position.0 + dx, position.1 + dy, position.2 + dz) >> 4;
        if behaviour(neighbour).is_some_and(|entry| entry.occludes) {
            continue;
        }
        let pair = light_attribute(cell_pair(
            snapshot,
            (position.0 + dx, position.1 + dy, position.2 + dz),
            None,
        ));
        let shade = [(face.brightness() * 255.0) as u8; 3];
        let base = mesh.vertices.len() as u32;
        for (index, corner) in FALLBACK_CORNERS[face as usize].iter().enumerate() {
            let uv = SPRITE_CORNERS[index];
            mesh.vertices.push(Vertex {
                position: [
                    position.0 as f32 + corner[0],
                    position.1 as f32 + corner[1],
                    position.2 as f32 + corner[2],
                ],
                uv: [
                    rect[0][0] + uv[0] * (rect[1][0] - rect[0][0]),
                    rect[0][1] + uv[1] * (rect[1][1] - rect[0][1]),
                ],
                light: pair,
                colour: [shade[0], shade[1], shade[2], 255],
            });
        }
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// The fallback cube's corners per face, counter-clockwise seen from outside,
/// in the client's `EnumFaceDirection` order — the order the model baker emits —
/// so the sprite corners pair with them corner for corner.
///
/// Indexed by [`Face`]'s declaration order.
const FALLBACK_CORNERS: [[[f32; 3]; 4]; 6] = [
    [
        [0.0, 1.0, 0.0],
        [0.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
        [1.0, 1.0, 0.0],
    ],
    [
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 1.0],
    ],
    [
        [1.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
    ],
    [
        [0.0, 1.0, 1.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
    ],
    [
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
    ],
    [
        [1.0, 1.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
    ],
];

/// The sprite coordinates of a full-face quad's four corners: the sprite's
/// top-left, bottom-left, bottom-right and top-right, which is the order
/// `BlockFaceUV` walks a face with no rotation.
const SPRITE_CORNERS: [[f32; 2]; 4] = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];

/// The face a quad belongs to, from its own winding: the axis its normal is
/// longest on.
///
/// The source reads the element's face key, which this project's baked quad does
/// not keep; an axis-aligned quad — every face of a cube element, rotated or not
/// — answers the same face the element declared.
fn quad_face(quad: &BakedQuad) -> Face {
    let normal = quad_normal(&quad.corners);
    let axes = [normal[0].abs(), normal[1].abs(), normal[2].abs()];
    let axis = if axes[0] >= axes[1] && axes[0] >= axes[2] {
        0
    } else if axes[1] >= axes[2] {
        1
    } else {
        2
    };
    let positive = normal[axis] >= 0.0;
    match (axis, positive) {
        (1, true) => Face::Top,
        (1, false) => Face::Bottom,
        (2, true) => Face::South,
        (2, false) => Face::North,
        (0, true) => Face::East,
        _ => Face::West,
    }
}

/// The normal of a quad's first three corners.
fn quad_normal(corners: &[[f32; 3]; 4]) -> [f32; 3] {
    let edge = |from: [f32; 3], to: [f32; 3]| [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let a = edge(corners[0], corners[1]);
    let b = edge(corners[1], corners[2]);
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A model direction's outward offset: the neighbour a face looks into.
fn offset(dir: FaceDir) -> (i32, i32, i32) {
    match dir {
        FaceDir::Up => (0, 1, 0),
        FaceDir::Down => (0, -1, 0),
        FaceDir::North => (0, 0, -1),
        FaceDir::South => (0, 0, 1),
        FaceDir::West => (-1, 0, 0),
        FaceDir::East => (1, 0, 0),
    }
}

/// A palette face as the model's own direction.
fn direction(face: Face) -> FaceDir {
    match face {
        Face::Top => FaceDir::Up,
        Face::Bottom => FaceDir::Down,
        Face::North => FaceDir::North,
        Face::South => FaceDir::South,
        Face::West => FaceDir::West,
        Face::East => FaceDir::East,
    }
}

/// The light of one cell, as the source's packed pair: the sky field in bits
/// 16..24 and the block field in bits 0..8, each the 0..15 level shifted left
/// four bits.
///
/// The store's own answer supplies the two levels; a block that emits light
/// answers its own emission in the block field, this project's face-local
/// stand-in for `World.getCombinedLight`'s world-wide maximum.
fn cell_pair(
    snapshot: &ColumnSnapshot,
    cell: (i32, i32, i32),
    block: Option<&BlockBehaviour>,
) -> i32 {
    let (sky, level) = snapshot.light(cell.0, cell.1, cell.2);
    let level = level.max(block.map_or(0, |entry| entry.light_emission));
    ((u32::from(sky) << 4) << 16 | (u32::from(level) << 4)) as i32
}

/// The four cells each vertex of an ambient-occlusion quad averages.
///
/// `AmbientOcclusionFace.updateVertexBrightness` (`BlockModelRenderer.java:364-504`):
/// the base is the cell the quad's face looks into for a quad that covers the
/// whole face and the block's own cell otherwise; the four tangent neighbours of
/// the base are read first, then the four corner cells, each corner falling back
/// to the tangent neighbour that shares its side when that side's block, or the
/// tangent block itself, is translucent. The centre is the standard path's own
/// sample.
fn ambient_pairs(
    snapshot: &ColumnSnapshot,
    position: (i32, i32, i32),
    quad: &BakedQuad,
    face: Face,
    centre: i32,
) -> [i32; 4] {
    let (dx, dy, dz) = offset(direction(face));
    let base = if covers_full_face(&quad.corners) {
        (position.0 + dx, position.1 + dy, position.2 + dz)
    } else {
        position
    };
    let tangents = TANGENTS[face as usize];
    let mut side = [0i32; 4];
    let mut translucent = [false; 4];
    let mut cells = [[0i32; 3]; 4];
    for (index, tangent) in tangents.iter().enumerate() {
        let (tx, ty, tz) = offset(*tangent);
        cells[index] = [base.0 + tx, base.1 + ty, base.2 + tz];
        let behind = snapshot.block(
            cells[index][0] + dx,
            cells[index][1] + dy,
            cells[index][2] + dz,
        ) >> 4;
        translucent[index] = behaviour(behind).is_some_and(|entry| entry.material.is_translucent());
        let cell = (cells[index][0], cells[index][1], cells[index][2]);
        side[index] = cell_pair(
            snapshot,
            cell,
            behaviour(snapshot.block(cell.0, cell.1, cell.2) >> 4),
        );
    }
    let corner = |a: usize, b: usize| -> i32 {
        if translucent[a] || translucent[b] {
            return side[a];
        }
        let (bx, by, bz) = offset(tangents[b]);
        let cell = (cells[a][0] + bx, cells[a][1] + by, cells[a][2] + bz);
        cell_pair(
            snapshot,
            cell,
            behaviour(snapshot.block(cell.0, cell.1, cell.2) >> 4),
        )
    };
    let corner_i = corner(0, 2);
    let corner_j = corner(0, 3);
    let corner_k = corner(1, 2);
    let corner_l = corner(1, 3);
    // `VertexTranslations` names the quad vertex each slot belongs to, and the
    // four groups the source's plain branch averages into each slot.
    let slots = VERTEX_SLOTS[face as usize];
    let mut pairs = [0i32; 4];
    pairs[slots[0]] = ao_brightness(side[3], side[0], corner_j, centre);
    pairs[slots[1]] = ao_brightness(side[2], side[0], corner_i, centre);
    pairs[slots[2]] = ao_brightness(side[2], side[1], corner_k, centre);
    pairs[slots[3]] = ao_brightness(side[3], side[1], corner_l, centre);
    pairs
}

/// `getAoBrightness`: the two channels' four-sample average, with a zero sample
/// replaced by the fourth first (`BlockModelRenderer.java:506-524`).
fn ao_brightness(first: i32, second: i32, third: i32, fourth: i32) -> i32 {
    let filled = |value: i32| if value == 0 { fourth } else { value };
    (filled(first) + filled(second) + filled(third) + fourth) >> 2 & 0x00FF_00FF
}

/// The light attribute pair of a packed value: each combined field with the
/// sampler's eight added, sky first.
fn light_attribute(packed: i32) -> [u16; 2] {
    let field = |shift: u32| ((((packed as u32) >> shift) & 0xFF) + 8) as u16;
    [field(16), field(0)]
}

/// Whether a quad's four corners cover a block face: every coordinate at 0 or
/// 1, with one axis constant and the other two reaching both ends.
///
/// The source's own flag 0 in `fillQuadBounds` asks whether the quad is the
/// block's full face; a full cube's face is, a partial element's — a grass
/// overlay, a stairs step — is not.
fn covers_full_face(corners: &[[f32; 3]; 4]) -> bool {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for corner in corners {
        for axis in 0..3 {
            if corner[axis] != 0.0 && corner[axis] != 1.0 {
                return false;
            }
            min[axis] = min[axis].min(corner[axis]);
            max[axis] = max[axis].max(corner[axis]);
        }
    }
    (0..3).filter(|axis| min[*axis] == max[*axis]).count() == 1
}

/// The four tangent neighbours of each face, in the source's own order:
/// `EnumNeighborInfo.field_178276_g` (`BlockModelRenderer.java:534-541`).
///
/// Indexed by [`Face`]'s declaration order: top, bottom, north, south, west,
/// east.
const TANGENTS: [[FaceDir; 4]; 6] = [
    [FaceDir::East, FaceDir::West, FaceDir::North, FaceDir::South],
    [FaceDir::West, FaceDir::East, FaceDir::North, FaceDir::South],
    [FaceDir::Up, FaceDir::Down, FaceDir::East, FaceDir::West],
    [FaceDir::West, FaceDir::East, FaceDir::Down, FaceDir::Up],
    [FaceDir::Up, FaceDir::Down, FaceDir::North, FaceDir::South],
    [FaceDir::Down, FaceDir::Up, FaceDir::North, FaceDir::South],
];

/// `VertexTranslations`: the quad vertex each face's four ambient-occlusion
/// slots belong to (`BlockModelRenderer.java:601-616`), in [`Face`]'s
/// declaration order.
const VERTEX_SLOTS: [[usize; 4]; 6] = [
    [2, 3, 0, 1],
    [0, 1, 2, 3],
    [3, 0, 1, 2],
    [0, 1, 2, 3],
    [3, 0, 1, 2],
    [1, 2, 3, 0],
];
