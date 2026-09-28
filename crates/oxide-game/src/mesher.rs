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
//! `:488-504`, or — for a quad that does not span its face's whole extent, on
//! a north, south, west or east face — the occlusion-weighted branch of
//! `:454-488`. The same four cells' `getAmbientOcclusionLightValue()`
//! (`0.2` for a normal cube, `1.0` otherwise; `block/Block.java:1099-1102`)
//! multiply the vertex's colour, so a concave corner darkens its vertex
//! (`:451`, `:491-502`, `:154-165`). Every other quad — the standard path of
//! `:98-101` and `:245-296` — gives all four vertices the light of one cell:
//! the neighbour the face looks into for a quad with a `cullface`, the block's
//! own cell for one without (a cross model's quads; this project's baked quad
//! does not keep the element face key, which is known limit 12).
//!
//! `SmoothLighting::Minimum` and `SmoothLighting::Maximum` are the same path in
//! this version: the renderer reads only `Minecraft.isAmbientOcclusionEnabled()`
//! (`Minecraft.java:2491-2493`), which is the setting not being `Off`.
//!
//! # The layers
//!
//! A quad's layer is its block's render layer (`Block.getBlockLayer`,
//! `block/Block.java:516-519`), the behaviour table's `render_layer` column:
//! `Solid` to the opaque layer, `CutoutMipped` and `Cutout` to the cutout
//! layer, `Translucent` to the translucent one. Air, and an id outside the
//! table, are opaque. Leaves are the one graphics-level override — the table
//! stores their Fast row, and [`MeshContext::graphics_fast`] false names the
//! other column (`BlockLeaves.getBlockLayer`, `block/BlockLeaves.java:293-296`),
//! the cutout layer — while the same flag makes their `occludes` read false
//! (`isOpaqueCube`, `:278-281`), so the cull rule and the layer cannot
//! disagree.
//!
//! The translucent layer's quads are sorted back to front before its indices
//! are emitted: descending distance from the section's centre to each quad's
//! centre, the mean of its four vertex positions, as a stable sort over quads.
//! The order is static and per section — the renderer draws translucent
//! sections back to front by camera distance, which is the ordering this
//! milestone needs; per-frame per-quad sorting is the transparent-terrain pass
//! a later milestone's overlays revisit.
//!
//! # Liquids
//!
//! A block whose `render` column is `Liquid` takes no model: the client builds
//! the liquid blocks in and never bakes them, so their geometry is the fluid
//! renderer's own rules, ported in [`liquid`] from
//! `client/renderer/BlockFluidRenderer.java` and `block/BlockLiquid.java`.
//!
//! # Known interim state
//!
//! The client's remaining built-in blocks still draw the magenta fallback cube
//! from the atlas's own fallback sprite, as do ids outside the behaviour table
//! and states the model set could not resolve.

mod liquid;
mod models;
mod snapshot;

pub use models::{BlockModelSet, ModelChoice, blockstate_target};
pub use snapshot::{Border, ColumnSnapshot};

use oxide_assets::atlas::Atlas;
use oxide_assets::model::{BakedModel, BakedQuad, FaceDir};
use oxide_render::terrain::{ChunkMesh, Layer, LayerMesh, Vertex};
use oxide_world::behaviour::{
    BlockBehaviour, Material, RenderKind, RenderLayer, TintKind, behaviour,
};
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
    /// Whether the graphics setting is Fast.
    ///
    /// The leaves' rule reads it, from the one flag, in both places it moves:
    /// their layer and their occlusion (`layer_of`, `occludes`).
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
    let base = section * SECTION_SIZE;
    let mut builder = SectionBuilder::new(base as i32);
    for y in base..base + SECTION_SIZE {
        for z in 0..SECTION_SIZE {
            for x in 0..SECTION_SIZE {
                append_block(&mut builder, snapshot, (x as i32, y as i32, z as i32), ctx);
            }
        }
    }
    let mesh = builder.finish();
    if mesh.is_empty() { None } else { Some(mesh) }
}

/// A section's quads while it is built: one list per layer, in visit order.
///
/// The section's centre is the sort key's origin; the translucent layer's quads
/// are reordered against it in [`SectionBuilder::finish`], once every quad of
/// the section is in.
struct SectionBuilder {
    /// The layers' quads, indexed by [`Layer::index`], four vertices each.
    quads: [Vec<[Vertex; 4]>; 3],
    /// The section's centre, in the column's own coordinates:
    /// `(8, base + 8, 8)`, the middle of the section.
    centre: [f32; 3],
}

impl SectionBuilder {
    /// A builder for the section whose first cell is at `base` on the y axis.
    fn new(base: i32) -> SectionBuilder {
        SectionBuilder {
            quads: std::array::from_fn(|_| Vec::new()),
            centre: [8.0, base as f32 + 8.0, 8.0],
        }
    }

    /// Appends one quad to one layer.
    fn push(&mut self, layer: Layer, quad: [Vertex; 4]) {
        self.quads[layer.index()].push(quad);
    }

    /// The section's mesh: each layer's quads in visit order, the translucent
    /// layer sorted back to front, each quad's two triangles over its four
    /// vertices.
    ///
    /// The sort runs on quads, before any index is emitted, so it is a stable
    /// one: two quads the same distance from the centre keep the visit order.
    fn finish(self) -> ChunkMesh {
        let centre = self.centre;
        let mut quads = self.quads;
        quads[Layer::Translucent.index()]
            .sort_by(|a, b| quad_distance(b, centre).total_cmp(&quad_distance(a, centre)));
        ChunkMesh {
            layers: std::array::from_fn(|index| {
                let mut layer = LayerMesh::default();
                for quad in &quads[index] {
                    let base = layer.vertices.len() as u32;
                    layer.vertices.extend_from_slice(quad);
                    layer.indices.extend_from_slice(&[
                        base,
                        base + 1,
                        base + 2,
                        base,
                        base + 2,
                        base + 3,
                    ]);
                }
                layer
            }),
        }
    }
}

/// The distance from a section's centre to a quad's centre, the mean of the
/// quad's four vertex positions.
fn quad_distance(quad: &[Vertex; 4], centre: [f32; 3]) -> f32 {
    let mut sum = [0.0f32; 3];
    for vertex in quad {
        for (axis, total) in sum.iter_mut().enumerate() {
            *total += vertex.position[axis];
        }
    }
    let dx = sum[0] / 4.0 - centre[0];
    let dy = sum[1] / 4.0 - centre[1];
    let dz = sum[2] / 4.0 - centre[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Appends one cell's geometry to the section's builder.
fn append_block(
    builder: &mut SectionBuilder,
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
    let layer = layer_of(block, ctx.graphics_fast);
    if block.is_some_and(|entry| entry.render == RenderKind::Liquid) {
        // Every liquid row carries its kind, and the table's own tests pin it;
        // a row without one is no liquid the fluid renderer could draw, so it
        // takes the fallback cube rather than a guessed surface.
        match block.and_then(|entry| entry.liquid) {
            Some(kind) => liquid::append(builder, snapshot, position, kind, block, layer, ctx),
            None => append_fallback(builder, snapshot, position, layer, ctx),
        }
        return;
    }
    match ctx
        .models
        .model(id, meta, position.0, position.1, position.2)
    {
        ModelChoice::Model(model) => append_model(builder, snapshot, position, block, model, ctx),
        ModelChoice::Missing => append_fallback(builder, snapshot, position, layer, ctx),
    }
}

/// The layer a block's quads land in.
///
/// The block's render layer, the client's `getBlockLayer`
/// (`block/Block.java:516-519`) as the table carries it, with the leaves'
/// graphics-level exception: the table stores their Fast row, and Fancy
/// graphics name the other layer — `CUTOUT_MIPPED`, this project's cutout
/// bucket (`BlockLeaves.getBlockLayer`, `block/BlockLeaves.java:293-296`).
/// Air, and an id outside the table, draw in the opaque layer, as does the
/// fallback cube for an id with no row.
fn layer_of(block: Option<&BlockBehaviour>, graphics_fast: bool) -> Layer {
    match block {
        Some(entry) if entry.material == Material::Leaves && !graphics_fast => Layer::Cutout,
        Some(entry) => bucket(entry.render_layer),
        None => Layer::Opaque,
    }
}

/// One render layer's bucket: `CUTOUT` and `CUTOUT_MIPPED` share the cutout
/// pass, and the other two stand alone.
fn bucket(layer: RenderLayer) -> Layer {
    match layer {
        RenderLayer::Solid => Layer::Opaque,
        RenderLayer::CutoutMipped | RenderLayer::Cutout => Layer::Cutout,
        RenderLayer::Translucent => Layer::Translucent,
    }
}

/// Whether the block at a cell hides a neighbour's face against it: the
/// table's `occludes` column, the source's `Block.isOpaqueCube()`.
///
/// The leaves are the one graphics-level override, and it is the same flag
/// that moves their layer: `BlockLeaves.isOpaqueCube` answers
/// `!fancyGraphics` (`block/BlockLeaves.java:278-281`), so the table's Fast
/// values stand only while `graphics_fast` does. Air, and an id outside the
/// table, occlude nothing.
fn occludes(id: u16, graphics_fast: bool) -> bool {
    match behaviour(id) {
        Some(entry) if entry.material == Material::Leaves => graphics_fast,
        Some(entry) => entry.occludes,
        None => false,
    }
}

/// Appends every quad of a resolved model, in the model's own order.
fn append_model(
    builder: &mut SectionBuilder,
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
            if occludes(neighbour, ctx.graphics_fast) {
                continue;
            }
        }
        append_quad(builder, snapshot, position, block, quad, model, ctx);
    }
}

/// Appends one quad: four vertices, then the two triangles over them, in the
/// block's own render layer.
fn append_quad(
    builder: &mut SectionBuilder,
    snapshot: &ColumnSnapshot,
    position: (i32, i32, i32),
    block: Option<&BlockBehaviour>,
    quad: &BakedQuad,
    model: &BakedModel,
    ctx: &MeshContext<'_>,
) {
    let layer = layer_of(block, ctx.graphics_fast);
    let face = quad_face(quad);
    // The cell the standard path's single sample comes from: the neighbour the
    // face looks into, or the block's own cell for a quad with no face to look
    // through.
    let sample = match quad.cullface {
        Some(cull) => {
            let (dx, dy, dz) = offset(cull);
            (position.0 + dx, position.1 + dy, position.2 + dz)
        }
        None => position,
    };
    let centre = cell_pair(snapshot, sample, block);
    let emission = block.map_or(0, |entry| entry.light_emission);
    let (pairs, multipliers) = if take_ambient_occlusion(ctx, emission, model) {
        let corners = ambient_corners(
            snapshot,
            position,
            quad,
            face,
            block.is_some_and(|entry| entry.full_cube),
            ctx.graphics_fast,
        );
        (corners.pairs, Some(corners.multipliers))
    } else {
        ([centre; 4], None)
    };
    let uvs = atlas_uv(ctx, quad);
    let tint = vertex_tint(block, quad, snapshot, position, ctx);
    let shade = shade_byte(quad, face);
    let colour = |multiplier: f32| -> [u8; 4] {
        [
            shaded_channel(shade, tint[0], multiplier),
            shaded_channel(shade, tint[1], multiplier),
            shaded_channel(shade, tint[2], multiplier),
            255,
        ]
    };
    let colours: [[u8; 4]; 4] = match multipliers {
        Some(multipliers) => std::array::from_fn(|index| colour(multipliers[index])),
        None => [colour(1.0); 4],
    };
    let vertices = std::array::from_fn(|index| Vertex {
        position: [
            position.0 as f32 + quad.corners[index][0],
            position.1 as f32 + quad.corners[index][1],
            position.2 as f32 + quad.corners[index][2],
        ],
        uv: uvs[index],
        light: light_attribute(pairs[index]),
        colour: colours[index],
    });
    builder.push(layer, vertices);
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

/// The tint the quad's colour is multiplied by: the block's tint at the
/// block's own position, or white.
///
/// The tint is sampled once per tinted face, at the block's own position, from
/// the nine-sample average the source's tint path consumes — `BlockModelRenderer`
/// gets the colour per quad from `BlockColors` (`BiomeColorHelper.getGrassColorAtPos`
/// and its siblings average the nine cells around the position, `world/BiomeColorHelper.java`),
/// and `oxide-world`'s [`tint_at_9_biome`] is that average over the snapshot's
/// biomes. A face with no tint index takes no tint at all — the client tints
/// nothing else — and a block whose kind is `None` is white.
fn vertex_tint(
    block: Option<&BlockBehaviour>,
    quad: &BakedQuad,
    snapshot: &ColumnSnapshot,
    position: (i32, i32, i32),
    ctx: &MeshContext<'_>,
) -> [u8; 3] {
    match block {
        Some(entry) if quad.tintindex.is_some() && entry.tint != TintKind::None => tint_at_9_biome(
            ctx.tint_maps,
            position.0,
            position.1,
            position.2,
            entry.tint,
            |x, z| snapshot.biome_at(x, z),
        ),
        _ => [255, 255, 255],
    }
}

/// The face's own shade as the byte the source bakes into the vertex record.
///
/// `FaceBakery.getFaceShadeColor` quantises the face's brightness with
/// `clamp((int)(shade * 255), 0, 255)` (`client/renderer/block/model/FaceBakery.java:48-53`),
/// so the six faces carry 127, 255, 204, 204, 153 and 153 — every later
/// multiply is a byte-times-byte one. The fluid renderer's own shades are the
/// same six values (`BlockFluidRenderer.java:53-56`), so the liquid path
/// composes its colours through this byte too.
fn face_shade(face: Face) -> u8 {
    (face.brightness() * 255.0) as u8
}

/// The shade byte of a model quad's face: the face's own shade, or the
/// builder's own colour when the model face turned shading off, which is the
/// white word `-1` (`FaceBakery.java:90-93`).
fn shade_byte(quad: &BakedQuad, face: Face) -> u8 {
    if quad.shade { face_shade(face) } else { 255 }
}

/// One colour channel as the source multiplies it: the baked shade byte times
/// the ambient-occlusion path's per-vertex multiplier and the tint byte, each
/// over 255, truncating.
///
/// `renderModelAmbientOcclusionQuads` hands `vertexColorMultiplier[i] * tint`
/// to `WorldRenderer.putColorMultiplier` (`client/renderer/BlockModelRenderer.java:151-165`),
/// which multiplies the byte already in the vertex record and keeps alpha
/// (`client/renderer/WorldRenderer.java:297-320`: `k = (int)((j & 255) * red)`).
/// The standard path is the same expression with the tint alone — it calls the
/// same method without a multiplier (`:287-293`) — and an untinted quad's tint
/// is `[255, 255, 255]`, which leaves the shade byte exactly as baked.
fn shaded_channel(shade: u8, tint: u8, multiplier: f32) -> u8 {
    let tint = f32::from(tint) / 255.0;
    (f32::from(shade) * (multiplier * tint)) as u8
}

/// The fallback cube: the six faces of a block drawn from the atlas's fallback
/// sprite.
///
/// The cull rule is the one the models get, and the vertices take the face's
/// shade and the light of the neighbour the face looks into — the form M1's
/// palette mesher gave an id it could not colour, with the magenta coming from
/// the sprite now rather than from the vertex.
fn append_fallback(
    builder: &mut SectionBuilder,
    snapshot: &ColumnSnapshot,
    position: (i32, i32, i32),
    layer: Layer,
    ctx: &MeshContext<'_>,
) {
    let rect = ctx.atlas.uv(&ctx.atlas.missing);
    for face in Face::ALL {
        let (dx, dy, dz) = face.offset();
        let neighbour = snapshot.block(position.0 + dx, position.1 + dy, position.2 + dz) >> 4;
        if occludes(neighbour, ctx.graphics_fast) {
            continue;
        }
        let pair = light_attribute(cell_pair(
            snapshot,
            (position.0 + dx, position.1 + dy, position.2 + dz),
            None,
        ));
        let shade = face_shade(face);
        let vertices = std::array::from_fn(|index| {
            let corner = FALLBACK_CORNERS[face as usize][index];
            let uv = SPRITE_CORNERS[index];
            Vertex {
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
                colour: [shade, shade, shade, 255],
            }
        });
        builder.push(layer, vertices);
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

/// One ambient-occlusion quad's per-vertex output.
struct AmbientCorners {
    /// The packed light pair of each quad vertex, in vertex order.
    pairs: [i32; 4],
    /// The colour multiplier of each quad vertex, in vertex order
    /// (`AmbientOcclusionFace.vertexColorMultiplier`).
    multipliers: [f32; 4],
}

/// The four cells each vertex of an ambient-occlusion quad averages, and the
/// two results they give it: its packed light pair and its colour multiplier.
///
/// `AmbientOcclusionFace.updateVertexBrightness` (`BlockModelRenderer.java:364-504`):
/// the base is the cell the quad's face looks into when `fillQuadBounds`'s
/// flag 0 holds — the quad is the block's face on the face's own axis — and
/// the block's own cell otherwise; the four tangent neighbours of the base are
/// read first, then the four corner cells, each corner taking the tangent's
/// value only when *neither* of the two behind-cells it spans is translucent
/// (`:377-442`, the source's `Block.translucent`, i.e. `!Material.blocksLight()`).
/// The centre is the neighbour's cell when flag 0 holds or the neighbour is not
/// opaque, and the block's own cell otherwise (`:444-449`) — "opaque" read
/// through [`occludes`], the leaves' graphics-level clause included, the same
/// reading the cull rule takes.
///
/// The four cells' `getAmbientOcclusionLightValue()` values answer the vertex's
/// colour multiplier — their mean in the plain branch (`:491-502`) or the
/// quad-bounds-weighted mix of the four means in the occlusion-weighted branch
/// (`:454-488`) — and their packed brightnesses feed the same branch's light
/// pairs through `getAoBrightness` (`:506-524`).
fn ambient_corners(
    snapshot: &ColumnSnapshot,
    position: (i32, i32, i32),
    quad: &BakedQuad,
    face: Face,
    full_cube: bool,
    graphics_fast: bool,
) -> AmbientCorners {
    let bounds = quad_bounds(face, &quad.corners, full_cube);
    let (dx, dy, dz) = offset(direction(face));
    let base = if bounds.face_plane {
        (position.0 + dx, position.1 + dy, position.2 + dz)
    } else {
        position
    };
    let tangents = TANGENTS[face as usize];
    let mut side = [0i32; 4];
    let mut light = [1.0f32; 4];
    let mut translucent = [false; 4];
    let mut cells = [[0i32; 3]; 4];
    for (index, tangent) in tangents.iter().enumerate() {
        let (tx, ty, tz) = offset(*tangent);
        cells[index] = [base.0 + tx, base.1 + ty, base.2 + tz];
        let (x, y, z) = (cells[index][0], cells[index][1], cells[index][2]);
        let value = snapshot.block(x, y, z);
        side[index] = cell_pair(snapshot, (x, y, z), behaviour(value >> 4));
        light[index] = ao_light_value(value >> 4);
        translucent[index] = !blocks_light(snapshot.block(x + dx, y + dy, z + dz) >> 4);
    }
    // The source's `i1`, `j1`, `k1` and `l1`: the first tangent of each pair,
    // pushed one cell along the second. Each one answers the first tangent's
    // own value when neither behind-cell is translucent (`:387-442`).
    let corner = |a: usize, b: usize| -> (i32, f32) {
        if !translucent[a] && !translucent[b] {
            return (side[a], light[a]);
        }
        let (bx, by, bz) = offset(tangents[b]);
        let (x, y, z) = (cells[a][0] + bx, cells[a][1] + by, cells[a][2] + bz);
        let value = snapshot.block(x, y, z);
        (
            cell_pair(snapshot, (x, y, z), behaviour(value >> 4)),
            ao_light_value(value >> 4),
        )
    };
    let (side_i, light_i) = corner(0, 2);
    let (side_j, light_j) = corner(0, 3);
    let (side_k, light_k) = corner(1, 2);
    let (side_l, light_l) = corner(1, 3);
    let (own_x, own_y, own_z) = position;
    let neighbour = (own_x + dx, own_y + dy, own_z + dz);
    let neighbour_value = snapshot.block(neighbour.0, neighbour.1, neighbour.2);
    let centre = if bounds.face_plane || !occludes(neighbour_value >> 4, graphics_fast) {
        cell_pair(snapshot, neighbour, behaviour(neighbour_value >> 4))
    } else {
        cell_pair(
            snapshot,
            position,
            behaviour(snapshot.block(own_x, own_y, own_z) >> 4),
        )
    };
    let base_light = {
        let (x, y, z) = base;
        ao_light_value(snapshot.block(x, y, z) >> 4)
    };
    // The source's plain branch, slot for slot (`:491-502`): each vertex's
    // light is `getAoBrightness` over two tangents, the corner between them and
    // the centre, and its multiplier is the mean of the same four cells'
    // ambient-occlusion light values.
    let plain_pair = [
        ao_brightness(side[3], side[0], side_j, centre),
        ao_brightness(side[2], side[0], side_i, centre),
        ao_brightness(side[2], side[1], side_k, centre),
        ao_brightness(side[3], side[1], side_l, centre),
    ];
    let plain_multiplier = [
        (light[3] + light[0] + light_j + base_light) * 0.25,
        (light[2] + light[0] + light_i + base_light) * 0.25,
        (light[2] + light[1] + light_k + base_light) * 0.25,
        (light[3] + light[1] + light_l + base_light) * 0.25,
    ];
    let (slot_pair, slot_multiplier) = match (bounds.partial, ORIENTATIONS[face as usize]) {
        (true, Some(arrays)) => {
            // The occlusion-weighted branch (`:454-488`): a quad that does not
            // span its face's whole extent on a north, south, west or east face
            // mixes the four plain values through the per-slot quad-bounds
            // products, which for a partial quad are the two fractions of the
            // face's other axes each vertex's corner sees.
            let quad_bounds = bounds.oriented();
            let products = |array: &[u8; 8]| -> [f32; 4] {
                [
                    quad_bounds[array[0] as usize] * quad_bounds[array[1] as usize],
                    quad_bounds[array[2] as usize] * quad_bounds[array[3] as usize],
                    quad_bounds[array[4] as usize] * quad_bounds[array[5] as usize],
                    quad_bounds[array[6] as usize] * quad_bounds[array[7] as usize],
                ]
            };
            let weighted: [[f32; 4]; 4] = std::array::from_fn(|slot| products(&arrays[slot]));
            let mixed_pair = std::array::from_fn(|slot| weighted_pair(plain_pair, &weighted[slot]));
            let mixed_multiplier = std::array::from_fn(|slot| {
                plain_multiplier[0] * weighted[slot][0]
                    + plain_multiplier[1] * weighted[slot][1]
                    + plain_multiplier[2] * weighted[slot][2]
                    + plain_multiplier[3] * weighted[slot][3]
            });
            (mixed_pair, mixed_multiplier)
        }
        _ => (plain_pair, plain_multiplier),
    };
    // `VertexTranslations` names the quad vertex each slot belongs to.
    let slots = VERTEX_SLOTS[face as usize];
    let mut pairs = [0i32; 4];
    let mut multipliers = [0f32; 4];
    for slot in 0..4 {
        pairs[slots[slot]] = slot_pair[slot];
        multipliers[slots[slot]] = slot_multiplier[slot];
    }
    AmbientCorners { pairs, multipliers }
}

/// `getVertexBrightness`: the four plain vertex pairs mixed by one slot's
/// quad-bounds products, per 16-bit light field, truncating
/// (`BlockModelRenderer.java:526-531`).
fn weighted_pair(plain: [i32; 4], weights: &[f32; 4]) -> i32 {
    let field = |shift: u32| -> i32 {
        let mut sum = 0.0f32;
        for (index, weight) in weights.iter().enumerate() {
            sum += ((plain[index] >> shift) & 0xFF) as f32 * weight;
        }
        (sum as i32) & 0xFF
    };
    (field(16) << 16) | field(0)
}

/// `Block.getAmbientOcclusionLightValue()`: `0.2` for a normal cube and `1.0`
/// for everything else (`block/Block.java:1099-1102`), where a normal cube is
/// the material blocking movement and the block being a full cube
/// (`isBlockNormalCube`, `:347-350`). Air, and an id outside the table, are not
/// normal cubes.
fn ao_light_value(id: u16) -> f32 {
    match behaviour(id) {
        Some(entry) if entry.material.blocks_movement() && entry.full_cube => 0.2,
        _ => 1.0,
    }
}

/// `Material.blocksLight()` — the source's `Block.translucent` field inverts it
/// (`block/Block.java:291-297`, read by `isTranslucent()` at `:215-223`):
/// whether the block's material blocks light. Air, and an id outside the table,
/// block nothing — the same non-occluding default the cull rule gives them.
fn blocks_light(id: u16) -> bool {
    behaviour(id).is_some_and(|entry| entry.material.blocks_light())
}

/// `getAoBrightness`: the two channels' four-sample average, with a zero sample
/// replaced by the fourth first (`BlockModelRenderer.java:506-524`).
fn ao_brightness(first: i32, second: i32, third: i32, fourth: i32) -> i32 {
    let filled = |value: i32| if value == 0 { fourth } else { value };
    (filled(first) + filled(second) + filled(third) + fourth) >> 2 & 0x00FF_00FF
}

/// The light attribute pair of a packed value: the block field first, then the
/// sky field, each with the sampler's eight added.
///
/// The order is the client's own: the packed int holds the block level at bits
/// 4..8 and the sky level at bits 20..24 (`World.getCombinedLight`,
/// `World.java:832-842`), and `ItemRenderer.setLightMapFromPlayer`
/// (`:113-116`) hands the low half — the block field — to
/// `glMultiTexCoord2f` first, as a vertex's pair of shorts carries it. The
/// first component is therefore the block level, which is the lightmap's own
/// first axis: its column is `i % 16` and its row `i / 16`
/// (`EntityRenderer.java:934-937`).
fn light_attribute(packed: i32) -> [u16; 2] {
    let field = |shift: u32| ((((packed as u32) >> shift) & 0xFF) + 8) as u16;
    [field(0), field(16)]
}

/// A quad's bounds and the two flags `fillQuadBounds` sets.
struct QuadBounds {
    /// The quad's bounds in the source's `quadBounds` order: the minimum y,
    /// maximum y, minimum z, maximum z, minimum x and maximum x
    /// (`BlockModelRenderer.java:193-207`, indexed by `EnumFacing`'s order).
    bounds: [f32; 6],
    /// The source's flag 0: the quad is the block's face on the face's own
    /// axis, so the ambient-occlusion base cell is the neighbour behind it.
    face_plane: bool,
    /// The source's flag 1: the quad does not span the whole face.
    partial: bool,
}

impl QuadBounds {
    /// The twelve values the orientation table indexes: the six bounds, then
    /// each one's distance from 1 (`BlockModelRenderer.java:195-206`).
    fn oriented(&self) -> [f32; 12] {
        std::array::from_fn(|index| {
            if index < 6 {
                self.bounds[index]
            } else {
                1.0 - self.bounds[index - 6]
            }
        })
    }
}

/// `fillQuadBounds` (`BlockModelRenderer.java:171-243`): the quad's own
/// extents, and the two flags the ambient-occlusion path branches on.
///
/// Flag 0 is `(the face's own plane is the block's plane || the block is a
/// full cube) && the quad is flat in the face's axis` — the `isFullCube()`
/// clause is the source's, so a *partial* element's face on one of the block's
/// own planes satisfies it while a plain "covers the whole face" test does
/// not. Flag 1 is true when either of the face's other two axes fails to reach
/// the block's edge, and only a north, south, west or east quad can then reach
/// the weighted branch (`EnumNeighborInfo.field_178289_i`, `:454`, `:536-541`).
fn quad_bounds(face: Face, corners: &[[f32; 3]; 4], full_cube: bool) -> QuadBounds {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for corner in corners {
        for axis in 0..3 {
            min[axis] = min[axis].min(corner[axis]);
            max[axis] = max[axis].max(corner[axis]);
        }
    }
    let (min_x, max_x) = (min[0], max[0]);
    let (min_y, max_y) = (min[1], max[1]);
    let (min_z, max_z) = (min[2], max[2]);
    let edge = 1.0e-4f32;
    let almost = 0.9999f32;
    let (face_plane, partial) = match face {
        Face::Bottom => (
            (min_y < edge || full_cube) && min_y == max_y,
            min_x >= edge || min_z >= edge || max_x <= almost || max_z <= almost,
        ),
        Face::Top => (
            (max_y > almost || full_cube) && min_y == max_y,
            min_x >= edge || min_z >= edge || max_x <= almost || max_z <= almost,
        ),
        Face::North => (
            (min_z < edge || full_cube) && min_z == max_z,
            min_x >= edge || min_y >= edge || max_x <= almost || max_y <= almost,
        ),
        Face::South => (
            (max_z > almost || full_cube) && min_z == max_z,
            min_x >= edge || min_y >= edge || max_x <= almost || max_y <= almost,
        ),
        Face::West => (
            (min_x < edge || full_cube) && min_x == max_x,
            min_y >= edge || min_z >= edge || max_y <= almost || max_z <= almost,
        ),
        Face::East => (
            (max_x > almost || full_cube) && min_x == max_x,
            min_y >= edge || min_z >= edge || max_y <= almost || max_z <= almost,
        ),
    };
    QuadBounds {
        bounds: [min_y, max_y, min_z, max_z, min_x, max_x],
        face_plane,
        partial,
    }
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

/// `EnumNeighborInfo`'s four orientation arrays per face
/// (`BlockModelRenderer.java:536-541`), as the quad-bound indexes they name:
/// `Orientation`'s index is `EnumFacing`'s order — down 0, up 1, north 2,
/// south 3, west 4, east 5 — plus six for the flipped form (`:578-599`), and
/// `quadBounds` is indexed the same way (`:195-206`).
///
/// The outer index is [`Face`]'s declaration order and the inner one the
/// source's field order: `field_178286_j` for the first vertex,
/// `field_178287_k`, `field_178284_l` and `field_178285_m` for the rest. `Top`
/// and `Bottom` carry no arrays — the source's `field_178289_i` is false for
/// them, so the weighted branch never runs on a vertical face (`:454`).
const ORIENTATIONS: [Option<[[u8; 8]; 4]>; 6] = [
    None,
    None,
    // NORTH: UP, FLIP_WEST, UP, WEST, FLIP_UP, WEST, FLIP_UP, FLIP_WEST and
    // the three sibling arrays.
    Some([
        [1, 10, 1, 4, 7, 4, 7, 10],
        [1, 11, 1, 5, 7, 5, 7, 11],
        [0, 11, 0, 5, 6, 5, 6, 11],
        [0, 10, 0, 4, 6, 4, 6, 10],
    ]),
    // SOUTH: UP, FLIP_WEST, FLIP_UP, FLIP_WEST, FLIP_UP, WEST, UP, WEST and
    // the three sibling arrays.
    Some([
        [1, 10, 7, 10, 7, 4, 1, 4],
        [0, 10, 6, 10, 6, 4, 0, 4],
        [0, 11, 6, 11, 6, 5, 0, 5],
        [1, 11, 7, 11, 7, 5, 1, 5],
    ]),
    // WEST: UP, SOUTH, UP, FLIP_SOUTH, FLIP_UP, FLIP_SOUTH, FLIP_UP, SOUTH and
    // the three sibling arrays.
    Some([
        [1, 3, 1, 9, 7, 9, 7, 3],
        [1, 2, 1, 8, 7, 8, 7, 2],
        [0, 2, 0, 8, 6, 8, 6, 2],
        [0, 3, 0, 9, 6, 9, 6, 3],
    ]),
    // EAST: FLIP_DOWN, SOUTH, FLIP_DOWN, FLIP_SOUTH, DOWN, FLIP_SOUTH, DOWN,
    // SOUTH and the three sibling arrays.
    Some([
        [6, 3, 6, 9, 0, 9, 0, 3],
        [6, 2, 6, 8, 0, 8, 0, 2],
        [7, 2, 7, 8, 1, 8, 1, 2],
        [7, 3, 7, 9, 1, 9, 1, 3],
    ]),
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
