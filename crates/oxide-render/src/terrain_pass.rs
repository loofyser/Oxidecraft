//! The textured terrain: one pipeline per render layer, the atlas bind group, the per-layer
//! section uploads and the culled, ordered draws.
//!
//! The pipelines read the byte stream [`vertex_bytes`] produces — a position, a uv, the packed
//! light pair and a colour, 28 bytes a vertex — and transform the positions with the camera's
//! view-projection matrix. The fragment stage samples the atlas at the vertex's uv and
//! multiplies the texel by the vertex colour, exactly as the client's block shader does: the
//! mesher bakes the face's brightness and the biome tint into the vertex colour, and the atlas
//! supplies the texture. The vertex stage hands the uv, the colour and the light pair (as two
//! floats) to the fragment stage; the light is what the M2 light ramp will read next.
//!
//! The three layers differ only where this task pins them apart. Opaque draws solid geometry
//! with the depth test and no blending, culling back faces. Cutout is the same with the
//! client's alpha test: a fragment whose alpha is below [`CUTOUT_ALPHA`] is discarded
//! (`GlStateManager.alphaFunc(516, 0.1F)`, `Minecraft.java:542`, set for the block layers at
//! `EntityRenderer.java:1393`), which punches leaves-like holes through the texture.
//! Translucent blends `src_alpha / one_minus_src_alpha` over what is already in the target
//! (`EntityRenderer.java:1459`), drawing last and culling nothing.
//!
//! All three layers test and write depth. Writing depth for the translucent layer is this
//! task's pinned choice (all three pipelines share one depth state), where the client switches
//! depth writes off for its translucent block layer (`GlStateManager.depthMask(false)`,
//! `EntityRenderer.java:1463`); with our per-section back-to-front order the farthest blended
//! surface lands first and each nearer one passes the test and blends over it, so the picture
//! matches while the depth buffer stays valid for the passes that follow the terrain.
//!
//! Culling the back faces of the translucent layer is likewise this task's pin — the client
//! renders that layer with culling enabled (`GlStateManager.enableCull()`,
//! `EntityRenderer.java:1458`, before the layer draws at `:1467`) and culls faces in the mesh
//! build instead — and it can only draw geometry the mesher emitted on purpose, never hide a
//! face: a glass or water face seen from either side draws.
//!
//! The winding is load-bearing. The mesher emits every face counter-clockwise seen from
//! outside the block, so the opaque and cutout layers declare counter-clockwise front faces
//! and cull back faces: a face wound the other way, or a pipeline that culls the wrong side,
//! disappears, and the headless pipeline test's read-back fails on it. The same test fails if
//! the depth test, the depth write or the depth attachment stops working.
//!
//! Depth: the camera's projection maps the near plane to 0 and the far plane to 1, which is
//! the convention [`DEPTH_FORMAT`] with [`wgpu::CompareFunction::Less`] expects. All three
//! pipelines test and write depth, so the nearest surface wins whatever order the meshes draw
//! in.
//!
//! Draw order: the frame is culled per section — a section whose box lies fully outside the
//! frame's frustum is skipped before its draw — and the translucent layer draws last, sorted
//! back to front by the distance from the eye to each section's centre, so a blended surface
//! lands on top of what stands behind it. That mirrors the client, which walks its render
//! infos in reverse for the translucent layer (`RenderGlobal.java:1055-1063`) on top of the
//! per-section back-to-front vertex sort the mesher already made.
//!
//! Nothing draws until both a camera and an atlas have been set: the pipelines bind the atlas
//! at group 1 and sample it in every layer, so a draw without one would be a validation error
//! and a picture of nothing. The shipping client sets the atlas from its bootstrap once the
//! asset store is open, and the asset-less session simply keeps the clear colour.

use std::collections::HashMap;

use glam::{Mat4, Vec3};

use crate::atlas_texture::AtlasTexture;
use crate::camera::Camera;
use crate::frustum::{Aabb3, Frustum};
use crate::terrain::{ChunkMesh, Layer, LayerMesh, SectionKey, VERTEX_BYTES, vertex_bytes};
use oxide_assets::atlas::Atlas;

/// The terrain shader source: sample the atlas at the vertex's uv, multiply by its colour.
///
/// The mesh's colours are already lit — the mesher multiplies the atlas entry by the face's
/// brightness and the biome tint and packs the light with the vertex — so the fragment stage's
/// work is the sample and the multiply. The two fragment entries are the layer difference:
/// [`FRAGMENT_MAIN`] hands the colour on, [`FRAGMENT_CUTOUT`] discards a fragment the atlas
/// made (nearly) transparent.
///
/// The cutout threshold is written from [`CUTOUT_ALPHA`], so the shader's literal and the
/// constant the tests pin cannot disagree.
fn shader_source() -> String {
    format!(
        r#"
struct Camera {{
    view_projection: mat4x4<f32>,
}};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var atlas: texture_2d<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

// The alpha below which the cutout layer discards a fragment: the client's own tenth
// (`GlStateManager.alphaFunc(516, 0.1F)` in `Minecraft.java`).
const CUTOUT_ALPHA: f32 = {CUTOUT_ALPHA};

struct VertexInput {{
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) light: vec2<u32>,
    @location(3) colour: vec4<f32>,
}};

struct VertexOutput {{
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) light: vec2<f32>,
    @location(2) colour: vec4<f32>,
}};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {{
    var output: VertexOutput;
    output.clip_position = camera.view_projection * vec4<f32>(input.position, 1.0);
    output.uv = input.uv;
    output.light = vec2<f32>(f32(input.light.x), f32(input.light.y));
    output.colour = input.colour;
    return output;
}}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {{
    return textureSample(atlas, atlas_sampler, input.uv) * input.colour;
}}

@fragment
fn fs_cutout(input: VertexOutput) -> @location(0) vec4<f32> {{
    let colour = textureSample(atlas, atlas_sampler, input.uv) * input.colour;
    if (colour.a < CUTOUT_ALPHA) {{
        discard;
    }}
    return colour;
}}
"#
    )
}

/// The vertex entry point every layer's pipeline uses.
const VS_ENTRY: &str = "vs_main";

/// The fragment entry point of the opaque and translucent layers: the sample times the colour.
const FRAGMENT_MAIN: &str = "fs_main";

/// The fragment entry point of the cutout layer: like [`FRAGMENT_MAIN`], discarding below
/// [`CUTOUT_ALPHA`].
const FRAGMENT_CUTOUT: &str = "fs_cutout";

/// The alpha below which the cutout layer discards a fragment: the client's own 0.1
/// (`Minecraft.java:542`, `EntityRenderer.java:1393`).
const CUTOUT_ALPHA: f32 = 0.1;

/// The depth format the pipeline tests and writes against.
///
/// The renderer builds its depth texture with this format, and the depth state of every
/// layer's pipeline declares it; the projection maps the near plane to depth 0 and the far
/// plane to depth 1, as [`wgpu::CompareFunction::Less`] over a 0..1 range expects.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The size of the camera uniform in bytes: one `mat4x4<f32>`.
const CAMERA_BYTES: usize = 64;

/// The terrain vertex attributes: a position at offset 0, the uv at offset 12,
/// the packed light pair at offset 20 and the colour at offset 24.
static ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x2,
    2 => Uint16x2,
    3 => Unorm8x4
];

/// One layer's pipeline choices.
///
/// Kept apart from the pipeline itself so the per-layer differences — the fragment entry, the
/// blend state and the cull mode — are pure values the unit tests pin without a device; the
/// vertex layout and the depth state are shared by all three layers and take no part here.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LayerPlan {
    /// The fragment entry point the layer's pipeline runs.
    fragment: &'static str,
    /// The colour blend state; `None` writes the fragment's colour as it is.
    blend: Option<wgpu::BlendState>,
    /// The faces to cull: back faces for the opaque layers, none for the translucent one.
    cull: Option<wgpu::Face>,
}

/// The plan for one layer.
fn layer_plan(layer: Layer) -> LayerPlan {
    match layer {
        Layer::Opaque => LayerPlan {
            fragment: FRAGMENT_MAIN,
            blend: None,
            cull: Some(wgpu::Face::Back),
        },
        Layer::Cutout => LayerPlan {
            fragment: FRAGMENT_CUTOUT,
            blend: None,
            cull: Some(wgpu::Face::Back),
        },
        Layer::Translucent => LayerPlan {
            fragment: FRAGMENT_MAIN,
            blend: Some(translucent_blend()),
            cull: None,
        },
    }
}

/// The translucent layer's blend: `src_alpha` over `one_minus_src_alpha`, added.
///
/// The colour component is the client's own pair for the translucent block layer
/// (`EntityRenderer.java:1459`, `GlStateManager.tryBlendFuncSeparate(770, 771, 1, 0)` —
/// `GL_SRC_ALPHA` and `GL_ONE_MINUS_SRC_ALPHA` — set just before the layer draws at
/// `EntityRenderer.java:1467`). The alpha component is the same pair, as this task pins; the
/// client's separate alpha pair is `(1, 0)`, which only differs in the target's own alpha
/// byte, and every read-back in this milestone is compared on colour.
fn translucent_blend() -> wgpu::BlendState {
    let component = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::SrcAlpha,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    };
    wgpu::BlendState {
        color: component,
        alpha: component,
    }
}

/// The layer's name, used to label its GPU objects.
fn layer_name(layer: Layer) -> &'static str {
    match layer {
        Layer::Opaque => "opaque",
        Layer::Cutout => "cutout",
        Layer::Translucent => "translucent",
    }
}

/// The label one of a layer's GPU objects carries: `oxide terrain <layer> <what>`.
fn layer_label(layer: Layer, what: &str) -> String {
    format!("oxide terrain {} {what}", layer_name(layer))
}

/// A section's mesh on the GPU: its two buffers and how many indices to draw.
struct GpuMesh {
    /// The vertex buffer, filled with [`vertex_bytes`] output.
    vertex_buffer: wgpu::Buffer,
    /// The index buffer, `u32` indices as little-endian bytes.
    index_buffer: wgpu::Buffer,
    /// The number of indices in the index buffer.
    index_count: u32,
}

/// One section's upload: one [`GpuMesh`] per non-empty layer.
#[derive(Default)]
struct SectionMesh {
    /// The three layers, indexed by [`Layer::index`]; a layer the section draws nothing in
    /// holds no buffers.
    layers: [Option<GpuMesh>; 3],
}

/// The frame state [`TerrainPass::set_camera`] derives for the next draw: where the boxes are
/// tested and what the translucent layer is ordered by.
struct FrameState {
    /// The frame's frustum, from the matrix the camera uploaded.
    frustum: Frustum,
    /// The eye position, the point the translucent order measures from.
    eye: Vec3,
}

/// The three terrain pipelines, the atlas they sample and the meshes they draw.
pub struct TerrainPass {
    /// One pipeline per layer, indexed by [`Layer::index`], built in the draw order.
    pipelines: [wgpu::RenderPipeline; 3],
    /// The layout group 1 binds the atlas through: built once, and the same object the atlas's
    /// bind groups are created with, so the two cannot drift apart.
    atlas_layout: wgpu::BindGroupLayout,
    /// The uniform buffer holding the frame's view-projection matrix.
    camera_buffer: wgpu::Buffer,
    /// The bind group the pipelines read the uniform through.
    camera_bind_group: wgpu::BindGroup,
    /// The atlas's texture and sampler, once one has been set; holding the texture keeps the
    /// view it is sampled through valid.
    atlas: Option<AtlasTexture>,
    /// The bind group the pipelines read the atlas through; set with the atlas and replaced
    /// with it.
    atlas_bind_group: Option<wgpu::BindGroup>,
    /// Every section's mesh, keyed by section.
    meshes: HashMap<SectionKey, SectionMesh>,
    /// The frame the next draw culls with, once a camera has been set.
    frame: Option<FrameState>,
}

impl TerrainPass {
    /// Builds the three layer pipelines for colour attachments in `format`.
    ///
    /// The vertex layout is the byte stream [`vertex_bytes`] produces: a `Float32x3` position
    /// at offset 0, a `Float32x2` uv at 12, a `Uint16x2` light pair at 20 and a `Unorm8x4`
    /// colour at 24, with a stride of [`VERTEX_BYTES`]. Every layer culls back faces except
    /// the translucent one and every layer tests and writes the depth buffer in
    /// [`DEPTH_FORMAT`], so a pass that draws with them needs a depth attachment of that
    /// format and a colour attachment in `format`. Group 0 is the camera uniform; group 1 is
    /// the atlas, whose layout this builds once for the pipelines and for
    /// [`TerrainPass::set_atlas`]'s bind groups.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oxide terrain shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source().into()),
        });
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide terrain camera layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(CAMERA_BYTES as u64),
                },
                count: None,
            }],
        });
        let atlas_layout = AtlasTexture::bind_group_layout(device);
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide terrain camera"),
            size: CAMERA_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide terrain camera bind group"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("oxide terrain pipeline layout"),
            bind_group_layouts: &[&camera_layout, &atlas_layout],
            push_constant_ranges: &[],
        });
        let pipelines = Layer::ALL.map(|layer| {
            let plan = layer_plan(layer);
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(&layer_label(layer, "pipeline")),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(VS_ENTRY),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[vertex_layout()],
                },
                primitive: primitive_state(plan.cull),
                depth_stencil: Some(depth_state()),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(plan.fragment),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[color_target(format, plan.blend)],
                }),
                multiview: None,
                cache: None,
            })
        });
        Self {
            pipelines,
            atlas_layout,
            camera_buffer,
            camera_bind_group,
            atlas: None,
            atlas_bind_group: None,
            meshes: HashMap::new(),
            frame: None,
        }
    }

    /// Uploads `atlas` and binds it for the frames that follow, replacing any earlier atlas.
    ///
    /// The upload copies the atlas's mip chain as it is ([`AtlasTexture::upload`]) and the old
    /// texture, its view and the old bind group are dropped with the replacement. Until an
    /// atlas is set [`TerrainPass::draw`] issues no draw calls: every layer samples the atlas,
    /// so a draw without one can only be wrong. M6 re-uploads here whenever an animated
    /// sprite's frame changes; M2 calls it once when the client's bootstrap has an atlas.
    pub fn set_atlas(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, atlas: &Atlas) {
        let texture = AtlasTexture::upload(device, queue, atlas);
        let bind_group = texture.bind_group(device, &self.atlas_layout);
        self.atlas = Some(texture);
        self.atlas_bind_group = Some(bind_group);
    }

    /// Writes the camera's view-projection matrix and derives the frame's cull state.
    ///
    /// The matrix reaches the shader as it is; nothing is transformed on the CPU. The renderer
    /// calls this once per frame with the surface's current aspect ratio, so a resize cannot
    /// leave a stale projection behind, and the frustum and the eye position derived here are
    /// what the next [`TerrainPass::draw`] culls and orders with.
    pub fn set_camera(&mut self, queue: &wgpu::Queue, camera: Camera, aspect: f32) {
        let view_projection = camera.view_projection(aspect);
        queue.write_buffer(&self.camera_buffer, 0, &matrix_bytes(view_projection));
        self.frame = Some(FrameState {
            frustum: Frustum::from_view_projection(view_projection),
            eye: camera.eye(),
        });
    }

    /// Adds or replaces a section's mesh on the GPU.
    ///
    /// Each non-empty layer gets its own buffer pair, uploaded on its own, because the layers
    /// draw in separate passes with separate pipelines; a layer the mesh draws nothing in
    /// holds no buffers, so a section whose water vanished frees its translucent buffers when
    /// the replacement lands. An empty mesh is a removal, because a zero-sized buffer is not a
    /// buffer; the mesher already returns `None` for a section that draws nothing. The buffers
    /// are sized exactly to the mesh, so a caller uploads a section only when its mesh changed.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: SectionKey,
        mesh: &ChunkMesh,
    ) {
        if mesh.is_empty() {
            self.remove(key);
            return;
        }
        let mut section = SectionMesh::default();
        for layer in Layer::ALL {
            let geometry = mesh.layer(layer);
            // A layer with no indices draws nothing, and one with no vertices has nothing to
            // draw from: both leaves its slot empty, and a zero-sized buffer is never created.
            if geometry.indices.is_empty() || geometry.vertices.is_empty() {
                continue;
            }
            section.layers[layer.index()] = Some(upload_layer(device, queue, layer, geometry));
        }
        self.meshes.insert(key, section);
    }

    /// Removes a section's mesh, freeing its buffers.
    ///
    /// Removing a section that holds no mesh does nothing.
    pub fn remove(&mut self, key: SectionKey) {
        self.meshes.remove(&key);
    }

    /// Draws every mesh the frame keeps, layer by layer, in the layers' own order.
    ///
    /// The opaque and cutout layers draw in the table's order — the table is a hash map, and
    /// every surface there is opaque and depth-tested, so the order cannot change the picture
    /// — and the translucent layer draws last, sorted back to front by the distance from the
    /// eye to the section's centre. Sections whose box lies fully outside the frame's frustum
    /// are skipped, both for their draw and for the translucent order.
    ///
    /// Nothing draws until a camera and an atlas have both been set: with no atlas the layers
    /// would sample an unbound texture, so the whole draw is skipped, and with no camera there
    /// is no frame to cull or order with.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let (Some(atlas_bind_group), Some(frame)) =
            (self.atlas_bind_group.as_ref(), self.frame.as_ref())
        else {
            return;
        };
        for layer in Layer::ALL {
            let draws = self.draw_list(layer, frame);
            if draws.is_empty() {
                continue;
            }
            pass.set_pipeline(&self.pipelines[layer.index()]);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.set_bind_group(1, atlas_bind_group, &[]);
            for (_, mesh) in draws {
                pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, 0..1);
            }
        }
    }

    /// One layer's draw list: the meshes that survive the cull, with the eye distance the
    /// translucent layer is ordered by.
    fn draw_list(&self, layer: Layer, frame: &FrameState) -> Vec<(f32, &GpuMesh)> {
        let mut draws = Vec::new();
        for (key, section) in &self.meshes {
            let Some(mesh) = section.layers[layer.index()].as_ref() else {
                continue;
            };
            let aabb = Aabb3::section(key.0, key.2, key.1);
            if !frame.frustum.intersects(&aabb) {
                continue;
            }
            let centre = aabb.centre();
            let distance = frame
                .eye
                .distance(Vec3::new(centre[0], centre[1], centre[2]));
            draws.push((distance, mesh));
        }
        if layer == Layer::Translucent {
            // Farthest first, so a blended surface lands over what stands behind it. The
            // comparison is total, so a NaN distance could never panic the sort.
            draws.sort_by(|near, far| far.0.total_cmp(&near.0));
        }
        draws
    }
}

/// Uploads one layer's geometry into its own buffer pair.
fn upload_layer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layer: Layer,
    geometry: &LayerMesh,
) -> GpuMesh {
    let vertices = vertex_bytes(&geometry.vertices);
    let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(&layer_label(layer, "vertices")),
        size: vertices.len() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&vertex_buffer, 0, &vertices);
    let indices = index_bytes(&geometry.indices);
    let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(&layer_label(layer, "indices")),
        size: indices.len() as u64,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&index_buffer, 0, &indices);
    GpuMesh {
        vertex_buffer,
        index_buffer,
        index_count: geometry.indices.len() as u32,
    }
}

/// The vertex buffer layout the pipelines read, tied to [`VERTEX_BYTES`] by the unit tests.
fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: VERTEX_BYTES as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    }
}

/// The primitive state for a cull mode: triangles, counter-clockwise front faces.
///
/// A face is front-facing exactly when the camera sees its outside, because the mesher winds
/// every face counter-clockwise seen from outside; culling back faces then drops the faces
/// the camera cannot see, and the translucent layer culls nothing so both sides of a glass or
/// water surface draw.
fn primitive_state(cull: Option<wgpu::Face>) -> wgpu::PrimitiveState {
    wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        front_face: wgpu::FrontFace::Ccw,
        cull_mode: cull,
        ..Default::default()
    }
}

/// The depth state: test and write depth, a nearer fragment winning.
fn depth_state() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: true,
        depth_compare: wgpu::CompareFunction::Less,
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

/// The colour target for one attachment in `format`: every channel written, blended when the
/// layer asks for it.
fn color_target(
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState {
        format,
        blend,
        write_mask: wgpu::ColorWrites::ALL,
    })
}

/// Packs a matrix into the 64 little-endian bytes of a WGSL `mat4x4<f32>`.
///
/// WGSL lays a uniform matrix out as four columns of four `f32`, which is the order
/// [`Mat4::to_cols_array`] returns, so the components go out as they are.
fn matrix_bytes(matrix: Mat4) -> [u8; CAMERA_BYTES] {
    let mut bytes = [0u8; CAMERA_BYTES];
    for (index, component) in matrix.to_cols_array().iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&component.to_le_bytes());
    }
    bytes
}

/// Packs `u32` indices into the little-endian bytes the index buffer holds.
fn index_bytes(indices: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(indices));
    for index in indices {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use wgpu::{
        BlendFactor, BlendOperation, ColorWrites, CompareFunction, Face, FrontFace,
        PrimitiveTopology, TextureFormat, VertexFormat,
    };

    use super::{
        CUTOUT_ALPHA, DEPTH_FORMAT, FRAGMENT_CUTOUT, FRAGMENT_MAIN, VS_ENTRY, color_target,
        depth_state, layer_plan, primitive_state, shader_source, vertex_layout,
    };
    use crate::atlas_texture::{ATLAS_BINDING, SAMPLER_BINDING};
    use crate::terrain::{Layer, VERTEX_BYTES};

    #[test]
    fn the_vertex_layout_matches_the_byte_stream() {
        let layout = vertex_layout();
        assert_eq!(layout.array_stride, VERTEX_BYTES as u64);
        assert_eq!(layout.step_mode, wgpu::VertexStepMode::Vertex);
        let [position, uv, light, colour] = layout.attributes else {
            panic!("four attributes: a position, a uv, a light pair and a colour");
        };
        assert_eq!(position.shader_location, 0);
        assert_eq!(position.format, VertexFormat::Float32x3);
        assert_eq!(position.offset, 0);
        assert_eq!(uv.shader_location, 1);
        assert_eq!(uv.format, VertexFormat::Float32x2);
        assert_eq!(uv.offset, 12);
        assert_eq!(light.shader_location, 2);
        assert_eq!(light.format, VertexFormat::Uint16x2);
        assert_eq!(light.offset, 20);
        assert_eq!(colour.shader_location, 3);
        assert_eq!(colour.format, VertexFormat::Unorm8x4);
        assert_eq!(colour.offset, 24);
        // The last attribute ends exactly at the stride, so no byte of a vertex is unread.
        assert_eq!(colour.offset + 4, layout.array_stride);
    }

    #[test]
    fn the_depth_state_tests_and_writes_the_depth_buffer() {
        let depth = depth_state();
        assert_eq!(DEPTH_FORMAT, TextureFormat::Depth32Float);
        assert_eq!(depth.format, DEPTH_FORMAT);
        assert!(depth.depth_write_enabled, "the terrain writes depth");
        assert_eq!(depth.depth_compare, CompareFunction::Less, "nearer wins");
    }

    #[test]
    fn the_opaque_and_cutout_layers_cull_the_back_faces_and_show_two_sided() {
        let opaque = primitive_state(layer_plan(Layer::Opaque).cull);
        let cutout = primitive_state(layer_plan(Layer::Cutout).cull);
        let translucent = primitive_state(layer_plan(Layer::Translucent).cull);
        for (primitive, what) in [
            (opaque, "the opaque layer"),
            (cutout, "the cutout layer"),
            (translucent, "the translucent layer"),
        ] {
            assert_eq!(primitive.topology, PrimitiveTopology::TriangleList);
            assert_eq!(primitive.front_face, FrontFace::Ccw, "{what}");
        }
        assert_eq!(opaque.cull_mode, Some(Face::Back), "the opaque layer");
        assert_eq!(cutout.cull_mode, Some(Face::Back), "the cutout layer");
        assert_eq!(
            translucent.cull_mode, None,
            "the translucent layer draws both sides of a surface"
        );
    }

    #[test]
    fn the_opaque_and_cutout_layers_replace_and_the_translucent_one_blends() {
        for layer in [Layer::Opaque, Layer::Cutout] {
            let target = color_target(TextureFormat::Rgba8Unorm, layer_plan(layer).blend)
                .expect("a colour target");
            assert_eq!(target.format, TextureFormat::Rgba8Unorm);
            assert_eq!(target.blend, None, "the layer writes its colour as it is");
            assert_eq!(target.write_mask, ColorWrites::ALL);
        }
        let target = color_target(
            TextureFormat::Rgba8Unorm,
            layer_plan(Layer::Translucent).blend,
        )
        .expect("a colour target");
        let blend = target.blend.expect("the translucent layer blends");
        assert_eq!(blend.color.src_factor, BlendFactor::SrcAlpha);
        assert_eq!(blend.color.dst_factor, BlendFactor::OneMinusSrcAlpha);
        assert_eq!(blend.color.operation, BlendOperation::Add);
        assert_eq!(
            blend.alpha, blend.color,
            "both components are the same pair"
        );
        assert_eq!(target.write_mask, ColorWrites::ALL);
    }

    #[test]
    fn the_cutout_layer_runs_the_discarding_fragment_entry() {
        assert_eq!(layer_plan(Layer::Opaque).fragment, FRAGMENT_MAIN);
        assert_eq!(layer_plan(Layer::Translucent).fragment, FRAGMENT_MAIN);
        assert_eq!(layer_plan(Layer::Cutout).fragment, FRAGMENT_CUTOUT);
        // The entries the pipelines name are the entries the shader declares, and the cutout
        // one discards at the client's own threshold.
        let shader = shader_source();
        for entry in [VS_ENTRY, FRAGMENT_MAIN, FRAGMENT_CUTOUT] {
            assert!(
                shader.contains(&format!("fn {entry}(")),
                "the shader declares {entry}"
            );
        }
        assert!(shader.contains("discard"), "the cutout entry discards");
        assert!(
            shader.contains(&format!("const CUTOUT_ALPHA: f32 = {CUTOUT_ALPHA};")),
            "the shader's own threshold is the client's tenth"
        );
        assert_eq!(CUTOUT_ALPHA, 0.1);
    }

    #[test]
    fn the_shader_binds_the_camera_at_group_zero_and_the_atlas_at_group_one() {
        let shader = shader_source();
        assert!(shader.contains("@group(0) @binding(0) var<uniform> camera: Camera"));
        assert!(shader.contains(&format!(
            "@group(1) @binding({ATLAS_BINDING}) var atlas: texture_2d<f32>"
        )));
        assert!(shader.contains(&format!(
            "@group(1) @binding({SAMPLER_BINDING}) var atlas_sampler: sampler"
        )));
    }
}
