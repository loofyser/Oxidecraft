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
//! The three layers differ only where the client's own block-layer state differs. Opaque
//! draws solid geometry: no blending, back faces culled, the depth test and write on. Cutout
//! is the same with the client's alpha test — a fragment whose alpha is below
//! [`CUTOUT_ALPHA`] is discarded (`GlStateManager.alphaFunc(516, 0.1F)`, `Minecraft.java:542`,
//! set for the block layers at `EntityRenderer.java:1393`) — which punches leaves-like holes
//! through the texture. Translucent carries the client's state as it stands at the translucent
//! draw (`EntityRenderer.java:1467`): the same alpha test (`:1460`), blending
//! `src_alpha / one_minus_src_alpha` with the separate alpha pair `(1, 0)` (`:1459`), back
//! faces culled (`GlStateManager.enableCull()`, `:1458`) and depth writes off
//! (`GlStateManager.depthMask(false)`, `:1463`, restored at `:1469`). It draws last.
//!
//! Depth: the camera's projection maps the near plane to 0 and the far plane to 1, which is
//! the convention [`DEPTH_FORMAT`] with [`wgpu::CompareFunction::Less`] expects. Opaque and
//! cutout test and write depth, so the nearest surface wins whatever order their meshes draw
//! in. Translucent tests depth and, like the client, writes none: a translucent fragment never
//! rejects a later one, so the per-section back-to-front order below decides which surface
//! blends over which, and every face the client would blend reaches the target.
//!
//! The winding is load-bearing. The mesher emits every face counter-clockwise seen from
//! outside the block, so every layer declares counter-clockwise front faces and culls back
//! faces: a face wound the other way, or a pipeline that culls the wrong side,
//! disappears, and the headless pipeline test's read-back fails on it. The same test fails if
//! the depth test, the depth write or the depth attachment stops working.
//!
//! Draw order: the frame is culled per section — a section whose box lies fully outside the
//! frame's frustum is skipped before its draw — and the translucent layer draws last, sorted
//! back to front by the distance from the eye to each section's centre, so a blended surface
//! lands on top of what stands behind it. That mirrors the client, which walks its render
//! infos in reverse for the translucent layer (`RenderGlobal.java:1055-1063`) on top of the
//! per-section back-to-front vertex sort the mesher already made.
//!
//! Lighting and fog are the client's own arithmetic too. The fragment stage samples the 16x16
//! lightmap — built from the Overworld's brightness table, the sun at its noon brightness and
//! the default gamma ([`crate::lightmap::lightmap_image`]) — at the vertex's packed light pair
//! over 256, the coordinate `enableLightmap`'s texture matrix installs
//! (`EntityRenderer.java:892-909`: scale `1/256`, translate eight), and multiplies that texel
//! into the atlas texel and the vertex colour: the three factors the client's block layers
//! carry, in its non-sRGB colour space (`docs/DIVERGENCES.md` records the policy). The same
//! stage then fades the result towards the frame's fog colour
//! ([`crate::fog::FogParams`]) over the distance from the eye:
//! `clamp((end - depth) / (end - start), 0, 1)` mixed in, with `depth` the fragment's
//! eye-space depth. That distance is the fixed-function fog's planar default; the source asks
//! the driver for radial distance when `GL_NV_fog_distance` is present
//! (`EntityRenderer.java:2018-2021`), which the acceptance rig checks rather than this pass.
//! A frame that has set no fog draws unfogged: the pass starts with a range that does not run
//! forwards, and such a range cannot fog anything.
//!
//! Nothing draws until both a camera and an atlas have been set: the pipelines bind the atlas
//! at group 1 and sample it in every layer, so a draw without one would be a validation error
//! and a picture of nothing. The shipping client sets the atlas from its bootstrap once the
//! asset store is open, and the asset-less session simply keeps the clear colour.

use std::collections::HashMap;

use glam::{Mat4, Vec3};

use crate::atlas_texture::AtlasTexture;
use crate::camera::Camera;
use crate::fog::FogParams;
use crate::frustum::{Aabb3, Frustum};
use crate::lightmap::{BrightnessTable, lightmap_image};
use crate::terrain::{ChunkMesh, Layer, LayerMesh, SectionKey, VERTEX_BYTES, vertex_bytes};
use oxide_assets::atlas::Atlas;

/// The terrain shader source: sample the atlas at the vertex's uv, multiply by its colour and
/// by the lightmap's texel, then fade towards the frame's fog colour.
///
/// The mesh's colours are already shaded — the mesher multiplies the atlas entry by the face's
/// brightness and the biome tint and packs the light with the vertex — so the fragment stage's
/// work is the three samples and the fog mix. Both fragment entries run the same [`shade`]
/// helper; the two differ only in the alpha test, which [`FRAGMENT_CUTOUT`] performs on the
/// alpha the helper hands back. The light is scaled by `1/256` — the scale factor the client's
/// own lightmap matrix carries — so the pair's `+ 8` puts every sample on a texel centre.
///
/// The discard threshold is written from [`CUTOUT_ALPHA`], so the shader's literal and the
/// constant the tests pin cannot disagree.
fn shader_source() -> String {
    format!(
        r#"
struct Camera {{
    view_projection: mat4x4<f32>,
}};

// The frame's fog: the colour the terrain fades to and the range it fades over — the distance
// the fade starts at and the distance it reaches full strength at. The far plane rides along in
// the third component as the frame's own record of the range; the mix below reads the other two.
struct Fog {{
    colour: vec4<f32>,
    params: vec4<f32>,
}};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<uniform> fog: Fog;
@group(1) @binding(0) var atlas: texture_2d<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;
@group(2) @binding(0) var lightmap: texture_2d<f32>;
@group(2) @binding(1) var lightmap_sampler: sampler;

// The alpha below which every layer with the client's alpha test discards a fragment: the
// client's own tenth (`GlStateManager.alphaFunc(516, 0.1F)` in `Minecraft.java`).
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
    @location(3) depth: f32,
}};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {{
    var output: VertexOutput;
    output.clip_position = camera.view_projection * vec4<f32>(input.position, 1.0);
    // The perspective divide's denominator is the eye-space depth of the vertex, so the
    // interpolated varying is the fragment's own depth: the distance the fog is measured over.
    output.depth = output.clip_position.w;
    output.uv = input.uv;
    output.light = vec2<f32>(f32(input.light.x), f32(input.light.y));
    output.colour = input.colour;
    return output;
}}

// The client's block fragment: the atlas texel times the vertex colour times the lightmap's
// texel, then the fog mix. The lightmap's own alpha is one, so the fragment's alpha comes from
// the atlas and the vertex colour alone.
fn shade(uv: vec2<f32>, light: vec2<f32>, colour: vec4<f32>, depth: f32) -> vec4<f32> {{
    let texel = textureSample(atlas, atlas_sampler, uv)
        * colour
        * textureSample(lightmap, lightmap_sampler, light / 256.0);
    return fogged(texel, depth);
}}

// The linear fog: the factor is one at the fade's start and zero at its end, and the colour is
// mixed towards the fog colour as the source's fixed-function fog does — the alpha is left as
// the fragment wrote it. A range that does not run forwards leaves the colour alone: that is
// the state a frame with no fog set draws in.
fn fogged(colour: vec4<f32>, depth: f32) -> vec4<f32> {{
    let span = fog.params.y - fog.params.x;
    if (span <= 0.0) {{
        return colour;
    }}
    let factor = clamp((fog.params.y - depth) / span, 0.0, 1.0);
    return vec4<f32>(mix(fog.colour.rgb, colour.rgb, factor), colour.a);
}}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {{
    return shade(input.uv, input.light, input.colour, input.depth);
}}

@fragment
fn fs_cutout(input: VertexOutput) -> @location(0) vec4<f32> {{
    let colour = shade(input.uv, input.light, input.colour, input.depth);
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

/// The fragment entry point of the opaque layer: the sample times the colour.
const FRAGMENT_MAIN: &str = "fs_main";

/// The fragment entry point of the cutout and translucent layers: like [`FRAGMENT_MAIN`],
/// discarding below [`CUTOUT_ALPHA`].
const FRAGMENT_CUTOUT: &str = "fs_cutout";

/// The alpha below which a layer with the client's alpha test discards a fragment: the
/// client's own 0.1 (`Minecraft.java:542`, set for the block layers at
/// `EntityRenderer.java:1393` and in force at the translucent draw, `:1460`).
const CUTOUT_ALPHA: f32 = 0.1;

/// The depth format the pipeline tests and writes against.
///
/// The renderer builds its depth texture with this format, and the depth state of every
/// layer's pipeline declares it; the projection maps the near plane to depth 0 and the far
/// plane to depth 1, as [`wgpu::CompareFunction::Less`] over a 0..1 range expects.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The size of the camera uniform in bytes: one `mat4x4<f32>`.
const CAMERA_BYTES: usize = 64;

/// The binding the fog uniform occupies in group 0, next to the camera as the frame's other
/// uniform.
const FOG_BINDING: u32 = 1;

/// The size of the fog uniform in bytes: two `vec4<f32>` — the colour, then the start, the end
/// and the far plane.
const FOG_BYTES: usize = 32;

/// The binding the lightmap texture occupies in group 2.
const LIGHTMAP_BINDING: u32 = 0;

/// The binding the lightmap sampler occupies in group 2.
const LIGHTMAP_SAMPLER_BINDING: u32 = 1;

/// The lightmap's size in texels on each axis: one texel per light level
/// (`DynamicTexture(16, 16)`, `EntityRenderer.java:190`).
const LIGHTMAP_SIZE: u32 = 16;

/// The sun's brightness the lightmap is built with: `World.getSunBrightness` at the noon
/// celestial angle of zero, where `1 - (cos(0) * 2 + 0.2)` clamps to zero and the value is
/// `1 * 0.8 + 0.2` (`World.java:1418-1427`). A live clock's per-frame value is M6's.
const SUN_BRIGHTNESS_NOON: f32 = 1.0;

/// The gamma the lightmap is built with: `GameSettings.gammaSetting`'s default, the float
/// field's own zero (`GameSettings.java:171`, whose constructor leaves it and whose only
/// writers are the options file and the slider, `:718`, `:270`).
const GAMMA_DEFAULT: f32 = 0.0;

/// The fog a frame draws with until one is set: a range that does not run forwards, which the
/// shader's mix reads as "leave the fragment as it is".
const NO_FOG: FogParams = FogParams {
    colour: [0.0; 3],
    start: 0.0,
    end: 0.0,
    far_plane: 0.0,
};

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
/// blend state, the cull mode and the depth write — are pure values the unit tests pin without
/// a device; the vertex layout and the depth test are shared by all three layers and take no
/// part here.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LayerPlan {
    /// The fragment entry point the layer's pipeline runs.
    fragment: &'static str,
    /// The colour blend state; `None` writes the fragment's colour as it is.
    blend: Option<wgpu::BlendState>,
    /// The faces to cull: back faces for every layer.
    cull: Option<wgpu::Face>,
    /// Whether the layer writes the depth buffer; the translucent layer does not.
    depth_write: bool,
}

/// The plan for one layer.
fn layer_plan(layer: Layer) -> LayerPlan {
    match layer {
        Layer::Opaque => LayerPlan {
            fragment: FRAGMENT_MAIN,
            blend: None,
            cull: Some(wgpu::Face::Back),
            depth_write: true,
        },
        Layer::Cutout => LayerPlan {
            fragment: FRAGMENT_CUTOUT,
            blend: None,
            cull: Some(wgpu::Face::Back),
            depth_write: true,
        },
        Layer::Translucent => LayerPlan {
            fragment: FRAGMENT_CUTOUT,
            blend: Some(translucent_blend()),
            cull: Some(wgpu::Face::Back),
            depth_write: false,
        },
    }
}

/// The translucent layer's blend: `src_alpha` over `one_minus_src_alpha`, with the target's
/// alpha left as it stands.
///
/// Both components are the client's own for the translucent block layer
/// (`EntityRenderer.java:1459`, `GlStateManager.tryBlendFuncSeparate(770, 771, 1, 0)`, in
/// force when the layer draws at `EntityRenderer.java:1467`): the colour pair is
/// `GL_SRC_ALPHA` / `GL_ONE_MINUS_SRC_ALPHA`, and the separate alpha pair `(1, 0)`
/// multiplies the destination's alpha byte by zero, so it keeps its value where the colour
/// pair would have scaled it.
fn translucent_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::Zero,
            operation: wgpu::BlendOperation::Add,
        },
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

/// The three terrain pipelines, the frame's uniforms they read, the atlas and the lightmap
/// they sample and the meshes they draw.
pub struct TerrainPass {
    /// One pipeline per layer, indexed by [`Layer::index`], built in the draw order.
    pipelines: [wgpu::RenderPipeline; 3],
    /// The layout group 1 binds the atlas through: built once, and the same object the atlas's
    /// bind groups are created with, so the two cannot drift apart.
    atlas_layout: wgpu::BindGroupLayout,
    /// The uniform buffer holding the frame's view-projection matrix.
    camera_buffer: wgpu::Buffer,
    /// The uniform buffer holding the frame's fog.
    fog_buffer: wgpu::Buffer,
    /// The bind group the pipelines read the frame's uniforms through: the camera at
    /// [`CAMERA_BYTES`]' binding and the fog at [`FOG_BINDING`].
    frame_bind_group: wgpu::BindGroup,
    /// The lightmap's texture and sampler, and the group-2 bind group they are bound through;
    /// built with the pass and holding the image the client starts with.
    lightmap_bind_group: wgpu::BindGroup,
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
    /// Builds the three layer pipelines for colour attachments in `format`, and the lightmap
    /// they sample.
    ///
    /// The vertex layout is the byte stream [`vertex_bytes`] produces: a `Float32x3` position
    /// at offset 0, a `Float32x2` uv at 12, a `Uint16x2` light pair at 20 and a `Unorm8x4`
    /// colour at 24, with a stride of [`VERTEX_BYTES`]. Every layer culls back faces and
    /// tests the depth buffer in [`DEPTH_FORMAT`]; the translucent layer's depth writes are
    /// off, so a pass that draws with them needs a depth attachment of that format and a
    /// colour attachment in `format`. Group 0 is the frame's uniforms — the camera matrix and
    /// the fog; group 1 is the atlas, whose layout this builds once for the pipelines and for
    /// [`TerrainPass::set_atlas`]'s bind groups; group 2 is the lightmap, built here with the
    /// image a fresh client holds (the Overworld's table, the noon sun and the default gamma).
    /// The fog starts at [`NO_FOG`], so the first frame a caller draws is unfogged until
    /// [`TerrainPass::set_fog`] gives one.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oxide terrain shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source().into()),
        });
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide terrain frame layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(CAMERA_BYTES as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: FOG_BINDING,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(FOG_BYTES as u64),
                    },
                    count: None,
                },
            ],
        });
        let atlas_layout = AtlasTexture::bind_group_layout(device);
        let lightmap_layout = lightmap_bind_group_layout(device);
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide terrain camera"),
            size: CAMERA_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let fog_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide terrain fog"),
            size: FOG_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide terrain frame bind group"),
            layout: &frame_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: FOG_BINDING,
                    resource: fog_buffer.as_entire_binding(),
                },
            ],
        });
        let lightmap_texture = device.create_texture(&lightmap_texture_descriptor());
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &lightmap_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &lightmap_image(
                &BrightnessTable::overworld(),
                SUN_BRIGHTNESS_NOON,
                GAMMA_DEFAULT,
            ),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(LIGHTMAP_SIZE * 4),
                rows_per_image: Some(LIGHTMAP_SIZE),
            },
            wgpu::Extent3d {
                width: LIGHTMAP_SIZE,
                height: LIGHTMAP_SIZE,
                depth_or_array_layers: 1,
            },
        );
        let lightmap_view = lightmap_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let lightmap_sampler = device.create_sampler(&lightmap_sampler_descriptor());
        let lightmap_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide terrain lightmap bind group"),
            layout: &lightmap_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: LIGHTMAP_BINDING,
                    resource: wgpu::BindingResource::TextureView(&lightmap_view),
                },
                wgpu::BindGroupEntry {
                    binding: LIGHTMAP_SAMPLER_BINDING,
                    resource: wgpu::BindingResource::Sampler(&lightmap_sampler),
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("oxide terrain pipeline layout"),
            bind_group_layouts: &[&frame_layout, &atlas_layout, &lightmap_layout],
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
                depth_stencil: Some(depth_state(plan.depth_write)),
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
        let mut pass = Self {
            pipelines,
            atlas_layout,
            camera_buffer,
            fog_buffer,
            frame_bind_group,
            lightmap_bind_group,
            atlas: None,
            atlas_bind_group: None,
            meshes: HashMap::new(),
            frame: None,
        };
        pass.set_fog(queue, NO_FOG);
        pass
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

    /// Writes the frame's fog for the frames that follow.
    ///
    /// The renderer calls this once per frame, before the draw, with the frame's own colour and
    /// range; a pass that has never been given one keeps [`NO_FOG`] and draws unfogged.
    pub fn set_fog(&mut self, queue: &wgpu::Queue, params: FogParams) {
        queue.write_buffer(&self.fog_buffer, 0, &fog_bytes(params));
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
            pass.set_bind_group(0, &self.frame_bind_group, &[]);
            pass.set_bind_group(1, atlas_bind_group, &[]);
            pass.set_bind_group(2, &self.lightmap_bind_group, &[]);
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
/// the camera cannot see. Every layer culls, as the client's cull state is enabled at all
/// three of its block-layer draws.
fn primitive_state(cull: Option<wgpu::Face>) -> wgpu::PrimitiveState {
    wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        front_face: wgpu::FrontFace::Ccw,
        cull_mode: cull,
        ..Default::default()
    }
}

/// The depth state every layer shares, with the write on or off.
///
/// The format and the comparison are the same for all three layers — the projection maps the
/// near plane to 0 and the far plane to 1, as [`wgpu::CompareFunction::Less`] expects, and a
/// nearer fragment wins — and `write` is the client's depth mask: on for its solid layers,
/// off for the translucent one (`GlStateManager.depthMask(false)`, `EntityRenderer.java:1463`).
fn depth_state(write: bool) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: write,
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

/// Packs a frame's fog into the 32 little-endian bytes of the shader's two `vec4`s.
///
/// The first `vec4` holds the colour, the second the fade's start, its end and the far plane,
/// with the components the mix does not read left zero. WGSL lays a uniform struct's `vec4`
/// fields out in declaration order, so the bytes go out in that order field by field.
fn fog_bytes(params: FogParams) -> [u8; FOG_BYTES] {
    let mut bytes = [0u8; FOG_BYTES];
    let values = [
        params.colour[0],
        params.colour[1],
        params.colour[2],
        0.0,
        params.start,
        params.end,
        params.far_plane,
        0.0,
    ];
    for (index, value) in values.iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// The group-2 layout: the lightmap texture and the sampler the fragment stage reads it
/// through. Filterable and single-sampled, because the lightmap is a plain 2D texture the
/// fragment stage samples with its own filtering.
fn lightmap_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("oxide terrain lightmap layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: LIGHTMAP_BINDING,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: LIGHTMAP_SAMPLER_BINDING,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

/// The lightmap's texture descriptor: [`LIGHTMAP_SIZE`] texels on each axis, `Rgba8Unorm`
/// because the client's lightmap image is plain bytes the fixed pipeline blended without any
/// transfer function, and uploadable because the pass fills it with `write_texture`.
fn lightmap_texture_descriptor() -> wgpu::TextureDescriptor<'static> {
    wgpu::TextureDescriptor {
        label: Some("oxide terrain lightmap"),
        size: wgpu::Extent3d {
            width: LIGHTMAP_SIZE,
            height: LIGHTMAP_SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    }
}

/// The lightmap's sampler descriptor: linear filtering on both axes and clamping on every
/// axis, the parameters the client installs for its lightmap texture
/// (`EntityRenderer.enableLightmap`, `:902-905`).
///
/// Linear filtering is what makes the light levels blend from texel to texel — the client
/// installs it here, not `GL_NEAREST` — and the clamp is what keeps the coordinate's `+ 8`
/// offset from wrapping the brightest level into the dimmest.
fn lightmap_sampler_descriptor() -> wgpu::SamplerDescriptor<'static> {
    wgpu::SamplerDescriptor {
        label: Some("oxide terrain lightmap sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use wgpu::{
        BlendFactor, BlendOperation, ColorWrites, CompareFunction, Face, FrontFace,
        PrimitiveTopology, TextureFormat, VertexFormat,
    };

    use super::{
        CUTOUT_ALPHA, DEPTH_FORMAT, FOG_BINDING, FOG_BYTES, FRAGMENT_CUTOUT, FRAGMENT_MAIN,
        GAMMA_DEFAULT, LIGHTMAP_BINDING, LIGHTMAP_SAMPLER_BINDING, LIGHTMAP_SIZE, NO_FOG,
        SUN_BRIGHTNESS_NOON, VS_ENTRY, color_target, depth_state, fog_bytes, layer_plan,
        lightmap_sampler_descriptor, lightmap_texture_descriptor, primitive_state, shader_source,
        vertex_layout,
    };
    use crate::atlas_texture::{ATLAS_BINDING, SAMPLER_BINDING};
    use crate::fog::FogParams;
    use crate::lightmap::{BrightnessTable, lightmap_image};
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
    fn the_depth_state_tests_every_layer_and_only_the_solid_layers_write() {
        assert_eq!(DEPTH_FORMAT, TextureFormat::Depth32Float);
        let solid = depth_state(layer_plan(Layer::Opaque).depth_write);
        assert_eq!(solid.format, DEPTH_FORMAT);
        assert!(solid.depth_write_enabled, "the terrain writes depth");
        assert_eq!(solid.depth_compare, CompareFunction::Less, "nearer wins");
        let cutout = depth_state(layer_plan(Layer::Cutout).depth_write);
        assert_eq!(cutout.format, solid.format);
        assert!(
            cutout.depth_write_enabled,
            "the cutout layer writes depth too"
        );
        assert_eq!(cutout.depth_compare, solid.depth_compare);
        // The translucent layer's own state: the same format and test, the client's depth
        // mask off.
        let translucent = depth_state(layer_plan(Layer::Translucent).depth_write);
        assert_eq!(translucent.format, DEPTH_FORMAT);
        assert_eq!(translucent.depth_compare, CompareFunction::Less);
        assert!(
            !translucent.depth_write_enabled,
            "the translucent layer writes no depth"
        );
    }

    #[test]
    fn every_layer_culls_the_back_faces() {
        for (layer, what) in [
            (Layer::Opaque, "the opaque layer"),
            (Layer::Cutout, "the cutout layer"),
            (Layer::Translucent, "the translucent layer"),
        ] {
            let primitive = primitive_state(layer_plan(layer).cull);
            assert_eq!(primitive.topology, PrimitiveTopology::TriangleList);
            assert_eq!(primitive.front_face, FrontFace::Ccw, "{what}");
            assert_eq!(
                primitive.cull_mode,
                Some(Face::Back),
                "{what} culls its back faces"
            );
        }
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
        // The separate alpha pair `(1, 0)`: the destination's alpha keeps its value.
        assert_eq!(blend.alpha.src_factor, BlendFactor::One);
        assert_eq!(blend.alpha.dst_factor, BlendFactor::Zero);
        assert_eq!(blend.alpha.operation, BlendOperation::Add);
        assert_eq!(target.write_mask, ColorWrites::ALL);
    }

    #[test]
    fn the_cutout_and_translucent_layers_run_the_discarding_fragment_entry() {
        assert_eq!(layer_plan(Layer::Opaque).fragment, FRAGMENT_MAIN);
        assert_eq!(layer_plan(Layer::Cutout).fragment, FRAGMENT_CUTOUT);
        assert_eq!(layer_plan(Layer::Translucent).fragment, FRAGMENT_CUTOUT);
        // The entries the pipelines name are the entries the shader declares, and the
        // discarding one discards at the client's own threshold.
        let shader = shader_source();
        for entry in [VS_ENTRY, FRAGMENT_MAIN, FRAGMENT_CUTOUT] {
            assert!(
                shader.contains(&format!("fn {entry}(")),
                "the shader declares {entry}"
            );
        }
        assert!(shader.contains("discard"), "the discarding entry discards");
        assert!(
            shader.contains(&format!("const CUTOUT_ALPHA: f32 = {CUTOUT_ALPHA};")),
            "the shader's own threshold is the client's tenth"
        );
        assert_eq!(CUTOUT_ALPHA, 0.1);
    }

    #[test]
    fn the_shader_binds_the_frames_uniforms_at_group_zero_the_atlas_at_one_and_the_lightmap_at_two()
    {
        let shader = shader_source();
        assert!(shader.contains("@group(0) @binding(0) var<uniform> camera: Camera"));
        assert!(shader.contains(&format!(
            "@group(0) @binding({FOG_BINDING}) var<uniform> fog: Fog"
        )));
        assert!(shader.contains(&format!(
            "@group(1) @binding({ATLAS_BINDING}) var atlas: texture_2d<f32>"
        )));
        assert!(shader.contains(&format!(
            "@group(1) @binding({SAMPLER_BINDING}) var atlas_sampler: sampler"
        )));
        assert!(shader.contains(&format!(
            "@group(2) @binding({LIGHTMAP_BINDING}) var lightmap: texture_2d<f32>"
        )));
        assert!(shader.contains(&format!(
            "@group(2) @binding({LIGHTMAP_SAMPLER_BINDING}) var lightmap_sampler: sampler"
        )));
        // The lightmap's coordinate is the packed light over 256, the scale the client's own
        // texture matrix installs, so a level's `+ 8` lands on the texel's centre.
        assert!(
            shader.contains("light / 256.0"),
            "the packed light is scaled by 256"
        );
        // Both fragment entries shade through the one helper — the atlas texel, the vertex
        // colour and the lightmap texel — and the discarding one tests the alpha it returns.
        assert_eq!(
            shader
                .matches("shade(input.uv, input.light, input.colour, input.depth)")
                .count(),
            2,
            "both entries run the shared shade"
        );
        assert!(
            shader.contains("let colour = shade("),
            "the cutout entry shades and then tests"
        );
        assert!(
            shader.contains("textureSample(lightmap, lightmap_sampler"),
            "the shade samples the lightmap"
        );
    }

    #[test]
    fn the_fog_uniform_packs_the_colour_the_start_the_end_and_the_far_plane() {
        let params = FogParams {
            colour: [0.25, 0.5, 0.75],
            start: 96.0,
            end: 128.0,
            far_plane: 128.0,
        };
        let bytes = fog_bytes(params);
        assert_eq!(bytes.len(), FOG_BYTES, "two `vec4<f32>`");
        for (index, expected) in [
            0.25f32, 0.5, 0.75,
            0.0, // the first vec4: the colour, its unused fourth left zero
            96.0, 128.0, 128.0,
            0.0, // the second: the fade's start and end, the far plane, zero
        ]
        .iter()
        .enumerate()
        {
            let value = f32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap());
            assert_eq!(value, *expected, "component {index}");
        }
    }

    #[test]
    fn a_frame_with_no_fog_set_cannot_fog_anything() {
        // The span the shader computes for a fresh pass cannot come out positive: the range
        // the pass holds does not run forwards. The check is a compile-time one, so the
        // constant cannot drift away from the shader's own guard unproven.
        const { assert!(NO_FOG.end - NO_FOG.start <= 0.0) };
        let shader = shader_source();
        assert!(
            shader.contains("let span = fog.params.y - fog.params.x;"),
            "the shader's span is the uniform's own end minus its start"
        );
        assert!(
            shader.contains("if (span <= 0.0)"),
            "and a span that does not run forwards skips the mix"
        );
    }

    #[test]
    fn the_lightmap_is_a_filtered_clamped_sixteen_texel_texture() {
        let texture = lightmap_texture_descriptor();
        assert_eq!(texture.size.width, LIGHTMAP_SIZE);
        assert_eq!(texture.size.height, LIGHTMAP_SIZE);
        assert_eq!(texture.size.depth_or_array_layers, 1);
        assert_eq!(
            texture.mip_level_count, 1,
            "one mip: the image has no chain"
        );
        assert_eq!(texture.sample_count, 1);
        assert_eq!(
            texture.format,
            TextureFormat::Rgba8Unorm,
            "the image is plain bytes, sampled without a transfer function"
        );
        assert!(
            texture.usage.contains(wgpu::TextureUsages::COPY_DST),
            "the pass writes the image into it"
        );
        assert!(texture.usage.contains(wgpu::TextureUsages::TEXTURE_BINDING));
        assert!(
            texture.view_formats.is_empty(),
            "no sRGB view: the lightmap's bytes are used as the client uses them"
        );
        let sampler = lightmap_sampler_descriptor();
        assert_eq!(sampler.mag_filter, wgpu::FilterMode::Linear);
        assert_eq!(sampler.min_filter, wgpu::FilterMode::Linear);
        assert_eq!(sampler.address_mode_u, wgpu::AddressMode::ClampToEdge);
        assert_eq!(sampler.address_mode_v, wgpu::AddressMode::ClampToEdge);
        assert_eq!(sampler.address_mode_w, wgpu::AddressMode::ClampToEdge);
    }

    #[test]
    fn the_passes_lightmap_is_the_clients_own_starting_image() {
        // The triple the pass builds its texture with: `getSunBrightness` at the noon angle —
        // `1 - (cos(0) * 2 + 0.2)` clamps away to zero, leaving `1 * 0.8 + 0.2`
        // (`World.java:1418-1427`) — and `gammaSetting`'s default zero
        // (`GameSettings.java:171`).
        let sun = SUN_BRIGHTNESS_NOON;
        let gamma = GAMMA_DEFAULT;
        assert_eq!(sun, 1.0, "the noon sun's brightness");
        assert_eq!(gamma, 0.0, "the default gamma");
        assert_eq!(
            lightmap_image(&BrightnessTable::overworld(), sun, gamma),
            lightmap_image(&BrightnessTable::overworld(), 1.0, 0.0),
            "the pass builds the image the client starts the game with"
        );
    }
}
