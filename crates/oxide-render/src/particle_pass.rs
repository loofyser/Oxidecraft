//! The particle quads: one camera-facing quad per engine frame.
//!
//! `EffectRenderer.renderParticles:222+` draws the live particles after the
//! entities and before the weather, in four batches — the `particles.png`
//! cells then the block atlas, each split by the depth-write bit its alpha
//! bin picks (`getAlpha() != 1.0 ? 0 : 1`, depth written only for the opaque
//! bin) — through `SRC_ALPHA/ONE_MINUS_SRC_ALPHA` with the 1/255 alpha test
//! and the frame's lightmap. This pass is that draw: the client feeds the
//! session's frames with the tick they rode in on, and the pass builds the
//! quads.
//!
//! The port's recorded deviations from the source arm:
//! * The depth-write bin reads the frame's current alpha, not the spawn
//!   alpha the source bins at add time: a quad fading down from opaque stops
//!   writing depth mid-fade. Only the batching differs, never the pixels.
//! * The sheet scroll is per tick, like the weather's: the client
//!   interpolates each quad's position between its previous and current tick
//!   at feed time (the source's `interpPos`), but the sheets themselves have
//!   no per-frame clock here.
//! * The layer-3 entity-particle batch never renders in the source either
//!   (the loop runs `i < 3` over layers 0-2 with only 0 and 1 bound), so the
//!   pass draws the cells then the atlas and nothing else.

use glam::Vec3;

use crate::camera::{Camera, FIRST_PERSON_OFFSET};
use crate::fog::{FogMode, FogParams};
use crate::lightmap::{BrightnessTable, lightmap_image};
use crate::terrain_pass::{color_target, depth_state, primitive_state};
use oxide_assets::atlas::Atlas;
use oxide_assets::texture::Texture;

use crate::atlas_texture::AtlasTexture;

/// Where a particle quad samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParticleUv {
    /// A `particles.png` cell, 0-63: the column is `cell % 16`, the row is
    /// `cell / 16`, each sampled `CELL_EXTENT` wide from its grid line — the
    /// source's own `index / 16.0, + 0.0624375` (`EntityFX.java:129-133`).
    Cell(u8),
    /// A block-atlas rect, already resolved into atlas space by the client.
    Atlas {
        /// The `(u0, v0)` corner.
        min: [f32; 2],
        /// The `(u1, v1)` corner.
        max: [f32; 2],
    },
}

/// One particle quad for the particle pass: the render-side mirror of
/// `oxide_game::particles::ParticleFrame`.
///
/// The crate graph gives this crate no edge to `oxide-game`, so the client
/// maps the session's frames into these at the frame drain, interpolating
/// the position between the frame's previous and current tick by the
/// frame's fraction (the source's `interpPos`) before it reaches this type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleDraw {
    /// The quad's centre, in blocks.
    pub pos: [f32; 3],
    /// The quad's half extent, in blocks: the source's `0.1 * scale`.
    pub size: f32,
    /// Red, green, blue and alpha.
    pub colour: [f32; 4],
    /// The sampled block light, 0-15.
    pub light: u8,
    /// Where the quad samples.
    pub tex: ParticleUv,
    /// Whether the quad writes depth: the source's opaque-alpha bin
    /// (`getAlpha() != 1.0 ? 0 : 1`), read off the current alpha here.
    pub opaque: bool,
}

/// The particle shader source: sample the bound sheet at the vertex's uv,
/// multiply by the lightmap's texel and the vertex's colour, then fade
/// towards the frame's fog colour.
///
/// The groups copy the weather pass's layout — group 0 the frame's
/// uniforms, group 1 the sheet, group 2 the lightmap. The discard threshold
/// is the source's own 1/255, in force at the particle draw.
fn shader_source() -> String {
    r#"
struct Camera {
    view_projection: mat4x4<f32>,
    eye: vec4<f32>,
};

struct Fog {
    colour: vec4<f32>,
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<uniform> fog: Fog;
@group(1) @binding(0) var sheet: texture_2d<f32>;
@group(1) @binding(1) var sheet_sampler: sampler;
@group(2) @binding(0) var lightmap: texture_2d<f32>;
@group(2) @binding(1) var lightmap_sampler: sampler;

const PARTICLE_ALPHA_TEST: f32 = 0.003921569;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) light: vec2<u32>,
    @location(3) colour: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) light: vec2<f32>,
    @location(2) colour: vec4<f32>,
    @location(3) distance: f32,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = camera.view_projection * vec4<f32>(input.position, 1.0);
    output.distance = length(input.position - camera.eye.xyz);
    output.uv = input.uv;
    output.light = vec2<f32>(f32(input.light.x), f32(input.light.y));
    output.colour = input.colour;
    return output;
}

fn fogged(colour: vec4<f32>, distance: f32) -> vec4<f32> {
    if (fog.params.w > 0.0) {
        let d = fog.params.w * distance;
        let factor = exp(-d * d);
        return vec4<f32>(mix(fog.colour.rgb, colour.rgb, factor), colour.a);
    }
    let span = fog.params.y - fog.params.x;
    if (span <= 0.0) {
        return colour;
    }
    let factor = clamp((fog.params.y - distance) / span, 0.0, 1.0);
    return vec4<f32>(mix(fog.colour.rgb, colour.rgb, factor), colour.a);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(sheet, sheet_sampler, input.uv)
        * textureSample(lightmap, lightmap_sampler, input.light / 256.0);
    let colour = fogged(
        vec4<f32>(texel.rgb * input.colour.rgb, texel.a * input.colour.a),
        input.distance,
    );
    if (colour.a < PARTICLE_ALPHA_TEST) {
        discard;
    }
    return colour;
}
"#
    .to_string()
}

/// The particle vertex attributes: a position at offset 0, the uv at 12,
/// the packed light pair at 20 and the colour at 24.
static ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x2,
    2 => Uint16x2,
    3 => Float32x4
];

/// The stride of one particle vertex in bytes: 12 + 8 + 4 + 16.
const VERTEX_BYTES: usize = 40;

/// The size of the camera uniform in bytes: one `mat4x4<f32>` and the eye's
/// `vec4<f32>`, the weather pass's own layout.
const CAMERA_BYTES: usize = 64 + 16;

/// The size of the fog uniform in bytes: two `vec4<f32>`, the weather
/// pass's own layout.
const FOG_BYTES: usize = 32;

/// The lightmap's size in texels on each axis, the weather pass's own.
const LIGHTMAP_SIZE: u32 = 16;

/// The fog a frame draws with until one is set: a range that does not run
/// forwards, which the shader's mix reads as "leave the fragment as it is".
const NO_FOG: FogParams = FogParams {
    colour: [0.0; 3],
    start: 0.0,
    end: 0.0,
    far_plane: 0.0,
    mode: FogMode::Linear,
    density: 0.0,
};

/// The particle quads' pass: two pipelines (depth writing or not), the
/// frame's uniforms, the cells sheet and the block atlas they draw through,
/// and the quads the last upload built.
pub struct ParticlePass {
    /// The pipelines the batches draw through, indexed by the depth-write
    /// bit: the opaque bin writes, the faded bin does not.
    pipelines: [wgpu::RenderPipeline; 2],
    /// The uniform buffer holding the frame's view-projection matrix.
    camera_buffer: wgpu::Buffer,
    /// The uniform buffer holding the frame's fog.
    fog_buffer: wgpu::Buffer,
    /// The bind group the pipelines read the frame's uniforms through.
    frame_bind_group: wgpu::BindGroup,
    /// The lightmap's texture and sampler, and the group-2 bind group they
    /// are bound through; built with the pass and holding the image a fresh
    /// client holds.
    lightmap_bind_group: wgpu::BindGroup,
    /// The lightmap's texture handle, kept so
    /// [`ParticlePass::set_lightmap`] can rewrite its bytes when the sky's
    /// brightness changes.
    lightmap_texture: wgpu::Texture,
    /// The cells sheet, once one has been set.
    cells: Option<CellSheet>,
    /// The block atlas, once one has been set: the pass's own upload, beside
    /// the terrain pass's — the two passes never share a texture handle.
    atlas: Option<AtlasSheet>,
    /// The camera's screen-right at the last [`ParticlePass::set_camera`]:
    /// the axis the next upload billboards around.
    right: Vec3,
    /// The camera's screen-up at the last [`ParticlePass::set_camera`].
    up: Vec3,
    /// The four batches the last upload built, in the source's draw order:
    /// the cells without the depth write, the cells with it, the atlas
    /// without it, the atlas with it. A batch whose sheet is missing, or
    /// that the upload left empty, draws nothing.
    batches: [Option<BatchMesh>; 4],
}

impl ParticlePass {
    /// Builds the particle pipelines for colour attachments in `format`.
    ///
    /// The vertex layout is the byte stream the upload produces: a
    /// `Float32x3` position at offset 0, a `Float32x2` uv at 12, a `Uint16x2`
    /// light pair at 20 and a `Float32x4` colour at 24. Both pipelines blend
    /// `src_alpha` over `one_minus_src_alpha` with the source's 1/255 alpha
    /// test and cull nothing; they differ in the depth write alone — the
    /// opaque bin's writes, the faded bin's does not — and both test the
    /// depth buffer in the frame's format. Group 0 is the frame's uniforms,
    /// group 1 the sheet the batch binds per arm, group 2 the lightmap,
    /// built here with the image a fresh client holds. The fog starts
    /// unfogged until [`ParticlePass::set_fog`] gives one.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oxide particle shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source().into()),
        });
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide particle frame layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let sheet_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide particle sheet layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let lightmap_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide particle lightmap layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("oxide particle pipeline layout"),
            bind_group_layouts: &[&frame_layout, &sheet_layout, &lightmap_layout],
            push_constant_ranges: &[],
        });
        // The two pipelines differ in the depth write alone: the faded bin
        // tests depth without writing it, while the opaque bin writes it —
        // the source's `getAlpha() != 1.0 ? 0 : 1` binning
        // (`EffectRenderer.java:222+`).
        let pipeline_for = |write: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if write {
                    "oxide particle opaque pipeline"
                } else {
                    "oxide particle faded pipeline"
                }),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: VERTEX_BYTES as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &ATTRIBUTES,
                    }],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[color_target(format, Some(particle_blend()))],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: primitive_state(None),
                depth_stencil: Some(depth_state(write)),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let pipelines = [pipeline_for(false), pipeline_for(true)];
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide particle camera"),
            size: CAMERA_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let fog_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide particle fog"),
            size: FOG_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&fog_buffer, 0, &fog_bytes(NO_FOG));
        let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide particle frame bind group"),
            layout: &frame_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: fog_buffer.as_entire_binding(),
                },
            ],
        });
        let lightmap_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("oxide particle lightmap"),
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
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &lightmap_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &lightmap_image(&BrightnessTable::overworld(), 1.0, 0.0),
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
        let lightmap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("oxide particle lightmap sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let lightmap_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide particle lightmap bind group"),
            layout: &lightmap_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&lightmap_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&lightmap_sampler),
                },
            ],
        });
        let (right, up) = screen_axes(Vec3::NEG_Z);
        Self {
            pipelines,
            camera_buffer,
            fog_buffer,
            frame_bind_group,
            lightmap_bind_group,
            lightmap_texture,
            cells: None,
            atlas: None,
            right,
            up,
            batches: [None, None, None, None],
        }
    }

    /// Writes the camera's view-projection matrix and keeps its screen axes
    /// for the next upload's billboards.
    ///
    /// The renderer calls this once per frame with the surface's current
    /// aspect ratio, so a resize cannot leave a stale projection behind.
    pub fn set_camera(&mut self, queue: &wgpu::Queue, camera: Camera, aspect: f32) {
        let view_projection = camera.view_projection(aspect);
        let eye = camera.eye() - FIRST_PERSON_OFFSET * camera.forward();
        queue.write_buffer(&self.camera_buffer, 0, &camera_bytes(view_projection, eye));
        (self.right, self.up) = screen_axes(camera.forward());
    }

    /// Uploads the cells sheet the cell quads draw through.
    ///
    /// Until this has been called the cell batches draw nothing; a re-upload
    /// replaces the sheet whole.
    pub fn set_cells(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, cells: &Texture) {
        let layout = self.sheet_layout(device);
        self.cells = Some(CellSheet::upload(device, queue, &layout, cells));
    }

    /// Uploads the block atlas the atlas quads draw through.
    ///
    /// The pass keeps its own upload beside the terrain pass's — the two
    /// passes never share a texture handle — with the atlas's own mip chain.
    /// Until this has been called the atlas batches draw nothing; a
    /// re-upload replaces the atlas whole.
    pub fn set_atlas(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, atlas: &Atlas) {
        let layout = self.sheet_layout(device);
        let uploaded = AtlasTexture::upload(device, queue, atlas);
        let bind_group = uploaded.bind_group(device, &layout);
        self.atlas = Some(AtlasSheet {
            bind_group,
            atlas: uploaded,
        });
    }

    /// Writes the frame's fog for the frames that follow.
    pub fn set_fog(&mut self, queue: &wgpu::Queue, params: FogParams) {
        queue.write_buffer(&self.fog_buffer, 0, &fog_bytes(params));
    }

    /// Rewrites the lightmap for a sky brightness and a gamma, the blend
    /// `EntityRenderer.updateLightmap` performs.
    pub fn set_lightmap(&mut self, queue: &wgpu::Queue, sun_brightness: f32, gamma: f32) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.lightmap_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &lightmap_image(&BrightnessTable::overworld(), sun_brightness, gamma),
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
    }

    /// Replaces the quads the next draw builds.
    ///
    /// Each draw becomes one camera-facing quad billboarded around the
    /// screen axes the last [`ParticlePass::set_camera`] kept: a cell draw
    /// samples its [`cell_uv`] rect of the cells sheet, an atlas draw its
    /// own rect of the block atlas. The bin follows the draw's current
    /// alpha — opaque at exactly `1.0`, faded below — and an empty slice
    /// clears every batch, so a settled tick draws nothing until the next
    /// upload. The single light level rides both light axes: the engine
    /// keeps the block-and-sky maximum, and the lightmap is read on its
    /// diagonal.
    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, draws: &[ParticleDraw]) {
        let mut vertices: [Vec<ParticleVertex>; 4] =
            [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        for draw in draws {
            if draw.size <= 0.0 {
                continue;
            }
            let uv = match draw.tex {
                ParticleUv::Cell(cell) => cell_uv(cell),
                ParticleUv::Atlas { min, max } => [min, max],
            };
            let batch = match draw.tex {
                ParticleUv::Cell(_) => 0,
                ParticleUv::Atlas { .. } => 2,
            } + usize::from(draw.opaque);
            vertices[batch].extend_from_slice(&quad_vertices(draw, self.right, self.up, uv));
        }
        self.batches = [
            upload_batch(device, queue, &vertices[0], "oxide particle cells faded"),
            upload_batch(device, queue, &vertices[1], "oxide particle cells opaque"),
            upload_batch(device, queue, &vertices[2], "oxide particle atlas faded"),
            upload_batch(device, queue, &vertices[3], "oxide particle atlas opaque"),
        ];
    }

    /// Draws the four batches in the source's order: the cells without the
    /// depth write, the cells with it, the atlas without it, the atlas with
    /// it. An arm whose sheet or mesh is missing draws nothing.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(2, &self.lightmap_bind_group, &[]);
        let sheets = [
            self.cells.as_ref().map(|sheet| &sheet.bind_group),
            self.cells.as_ref().map(|sheet| &sheet.bind_group),
            self.atlas.as_ref().map(|sheet| &sheet.bind_group),
            self.atlas.as_ref().map(|sheet| &sheet.bind_group),
        ];
        for (index, mesh) in self.batches.iter().enumerate() {
            let (Some(sheet), Some(mesh)) = (sheets[index], mesh.as_ref()) else {
                continue;
            };
            // Batches 0 and 2 are the faded bins, 1 and 3 the opaque ones.
            pass.set_pipeline(&self.pipelines[index % 2]);
            pass.set_bind_group(1, sheet, &[]);
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.index_count, 0, 0..1);
        }
    }

    /// The sheet bind-group layout: rebuilt per upload call because the pass
    /// keeps no layout handle.
    fn sheet_layout(&self, device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide particle sheet layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        })
    }
}

/// The `particles.png` sheet's grid: 16 cells across, like the source's
/// `index / 16.0` divisor (`EntityFX.java:129-133`).
const CELLS_PER_ROW: f32 = 16.0;

/// The sampled cell's extent in sheet space: the source's own `+ 0.0624375`
/// inset (`EntityFX.java:129-133`), which keeps the sample off the
/// neighbour cell without shrinking the quad.
pub const CELL_EXTENT: f32 = 0.0624375;

/// The sheet-space uv rect of a `particles.png` cell: the column is
/// `cell % 16`, the row is `cell / 16`, each [`CELL_EXTENT`] wide from its
/// grid line — the source's own arithmetic, over the full 16×16 grid, so an
/// out-of-range cell wraps through the sheet's repeat sampler exactly as the
/// source's coordinates wrap through its texture.
pub fn cell_uv(cell: u8) -> [[f32; 2]; 2] {
    let column = f32::from(cell % 16);
    let row = f32::from(cell / 16);
    let u0 = column / CELLS_PER_ROW;
    let v0 = row / CELLS_PER_ROW;
    [[u0, v0], [u0 + CELL_EXTENT, v0 + CELL_EXTENT]]
}

/// One particle vertex, in the [`ATTRIBUTES`] order.
#[derive(Debug, Clone, Copy)]
struct ParticleVertex {
    /// The world-space position.
    position: [f32; 3],
    /// The sheet uv.
    uv: [f32; 2],
    /// The packed light pair, each level shifted four bits with eight
    /// added — the client's own packing, as the terrain vertices carry it.
    light: [u16; 2],
    /// Red, green, blue and alpha.
    colour: [f32; 4],
}

/// Packs one light level the way the client's vertices carry it: the level
/// shifted four bits with eight added, so the shader's `/ 256` lands on the
/// texel's centre.
fn pack_light(level: u8) -> u16 {
    u16::from(level.min(15)) * 16 + 8
}

/// One draw's quad vertices: the centre billboarded around the camera's
/// screen axes at `± size`, carrying `uv` and the frame's colour and light.
fn quad_vertices(
    draw: &ParticleDraw,
    right: Vec3,
    up: Vec3,
    uv: [[f32; 2]; 2],
) -> [ParticleVertex; 4] {
    let centre = Vec3::from_array(draw.pos);
    let light = [pack_light(draw.light), pack_light(draw.light)];
    let corner = |side: f32, lift: f32, u: f32, v: f32| ParticleVertex {
        position: (centre + right * (side * draw.size) + up * (lift * draw.size)).to_array(),
        uv: [u, v],
        light,
        colour: draw.colour,
    };
    let [[u0, v0], [u1, v1]] = uv;
    [
        corner(-1.0, -1.0, u0, v1),
        corner(1.0, -1.0, u1, v1),
        corner(1.0, 1.0, u1, v0),
        corner(-1.0, 1.0, u0, v0),
    ]
}
/// A sheet's uploaded texture, with the bind group the pass reads it
/// through.
struct CellSheet {
    /// The bind group the pass draws the sheet's quads through.
    bind_group: wgpu::BindGroup,
    /// The texture handle, kept so the view above stays valid.
    #[allow(dead_code)]
    texture: wgpu::Texture,
}

/// The block atlas on the GPU, with the bind group the pass draws the atlas
/// quads through.
struct AtlasSheet {
    /// The bind group the pass draws the atlas quads through.
    bind_group: wgpu::BindGroup,
    /// The atlas handle, kept so the view above stays valid.
    #[allow(dead_code)]
    atlas: AtlasTexture,
}

/// One batch's quads on the GPU: the vertex buffer and how many indices the
/// batch's draw issues.
struct BatchMesh {
    /// The vertex buffer, filled with [`vertex_bytes`] output.
    vertex_buffer: wgpu::Buffer,
    /// The index buffer, `u32` indices as little-endian bytes.
    index_buffer: wgpu::Buffer,
    /// The number of indices in the index buffer.
    index_count: u32,
}

impl CellSheet {
    /// Uploads `cells` as an `Rgba8Unorm` image, refusing a byte count that
    /// does not match its declared size with a one-texel stand-in.
    fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        cells: &Texture,
    ) -> Self {
        let (width, height, bytes) = if cells.rgba.len()
            == (cells.width * cells.height * 4) as usize
        {
            (
                cells.width.max(1),
                cells.height.max(1),
                cells.rgba.as_slice(),
            )
        } else {
            tracing::warn!(
                width = cells.width,
                height = cells.height,
                bytes = cells.rgba.len(),
                "the particle cells sheet does not match its declared size; a one-texel stand-in is used"
            );
            (1, 1, &[255u8, 255u8, 255u8, 255u8] as &[u8])
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("oxide particle cells"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("oxide particle cells sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide particle cells bind group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        Self {
            bind_group,
            texture,
        }
    }
}

/// The camera's screen axes for a view along `forward`: the right is the
/// view crossed with the world's up — the weather ribbon's own axis —
/// falling back to +x when the view is straight up or down, and the up
/// completes the frame.
fn screen_axes(forward: Vec3) -> (Vec3, Vec3) {
    let right = Vec3::new(-forward.z, 0.0, forward.x);
    let right = if right.length_squared() < 1e-6 {
        Vec3::X
    } else {
        right.normalize()
    };
    (right, right.cross(forward).normalize())
}

/// The particle blend: `src_alpha` over `one_minus_src_alpha`, with the
/// target's alpha left as it stands — the translucent layer's own blend.
fn particle_blend() -> wgpu::BlendState {
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

/// The camera uniform's bytes: the matrix's columns, then the eye.
fn camera_bytes(matrix: glam::Mat4, eye: Vec3) -> [u8; CAMERA_BYTES] {
    let mut bytes = [0u8; CAMERA_BYTES];
    for (index, component) in matrix.to_cols_array().iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&component.to_le_bytes());
    }
    for (index, component) in [eye.x, eye.y, eye.z, 0.0].iter().enumerate() {
        let offset = 64 + index * 4;
        bytes[offset..offset + 4].copy_from_slice(&component.to_le_bytes());
    }
    bytes
}

/// The fog uniform's bytes: the colour, then the start, the end, the far
/// plane and the density.
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
        params.density,
    ];
    for (index, value) in values.iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// One vertex's bytes in the [`ATTRIBUTES`] order.
fn vertex_bytes(vertex: &ParticleVertex) -> [u8; VERTEX_BYTES] {
    let mut bytes = [0u8; VERTEX_BYTES];
    for (index, component) in vertex.position.iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&component.to_le_bytes());
    }
    for (index, component) in vertex.uv.iter().enumerate() {
        let offset = 12 + index * 4;
        bytes[offset..offset + 4].copy_from_slice(&component.to_le_bytes());
    }
    for (index, level) in vertex.light.iter().enumerate() {
        let offset = 20 + index * 2;
        bytes[offset..offset + 2].copy_from_slice(&level.to_le_bytes());
    }
    for (index, component) in vertex.colour.iter().enumerate() {
        let offset = 24 + index * 4;
        bytes[offset..offset + 4].copy_from_slice(&component.to_le_bytes());
    }
    bytes
}

/// Uploads one batch's quads, or nothing for an empty vertex list: a
/// zero-sized buffer is not a buffer.
fn upload_batch(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    vertices: &[ParticleVertex],
    label: &str,
) -> Option<BatchMesh> {
    if vertices.is_empty() {
        return None;
    }
    let mut bytes = Vec::with_capacity(vertices.len() * VERTEX_BYTES);
    for vertex in vertices {
        bytes.extend_from_slice(&vertex_bytes(vertex));
    }
    let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes.len() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&vertex_buffer, 0, &bytes);
    let quads = vertices.len() / 4;
    let mut indices = Vec::with_capacity(quads * 6);
    for quad in 0..quads {
        let base = (quad * 4) as u32;
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let mut index_bytes = Vec::with_capacity(indices.len() * 4);
    for index in &indices {
        index_bytes.extend_from_slice(&index.to_le_bytes());
    }
    let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: index_bytes.len() as u64,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&index_buffer, 0, &index_bytes);
    Some(BatchMesh {
        vertex_buffer,
        index_buffer,
        index_count: indices.len() as u32,
    })
}

#[cfg(test)]
mod tests {
    //! The cell uvs, the screen axes and the quad builder, without a GPU.

    use super::*;

    #[test]
    fn the_cell_uv_matches_the_source_inset() {
        // Cell 0 sits at the sheet's corner, one extent wide.
        assert_eq!(cell_uv(0), [[0.0, 0.0], [CELL_EXTENT, CELL_EXTENT]]);
        // Cell 19 — the rain drops' first draw — is column 3, row 1.
        let [[u0, v0], [u1, v1]] = cell_uv(19);
        assert!((u0 - 3.0 / 16.0).abs() < 1e-7, "column 3: {u0}");
        assert!((v0 - 1.0 / 16.0).abs() < 1e-7, "row 1: {v0}");
        assert!((u1 - u0 - CELL_EXTENT).abs() < 1e-7);
        assert!((v1 - v0 - CELL_EXTENT).abs() < 1e-7);
    }

    #[test]
    fn the_screen_axes_face_the_camera() {
        // Facing +z (south): the right is -x, the up is +y.
        let (right, up) = screen_axes(Vec3::Z);
        assert!((right + Vec3::X).length() < 1e-6, "right: {right:?}");
        assert!((up - Vec3::Y).length() < 1e-6, "up: {up:?}");
        // Straight down falls back to +x without degenerating.
        let (right, up) = screen_axes(Vec3::NEG_Y);
        assert_eq!(right, Vec3::X);
        assert!(up.is_finite(), "up: {up:?}");
    }

    #[test]
    fn the_quad_billboards_around_the_screen_axes() {
        let draw = ParticleDraw {
            pos: [0.0, 0.0, 0.0],
            size: 0.5,
            colour: [1.0, 1.0, 1.0, 1.0],
            light: 15,
            tex: ParticleUv::Cell(0),
            opaque: true,
        };
        let corners = quad_vertices(&draw, Vec3::X, Vec3::Y, cell_uv(0));
        assert_eq!(
            corners.map(|corner| corner.position),
            [
                [-0.5, -0.5, 0.0],
                [0.5, -0.5, 0.0],
                [0.5, 0.5, 0.0],
                [-0.5, 0.5, 0.0],
            ]
        );
        assert_eq!(corners[0].light, [248, 248]);
        assert_eq!(corners[0].colour, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn the_light_packing_lands_on_texel_centres() {
        assert_eq!(pack_light(15), 248);
        assert_eq!(pack_light(0), 8);
        assert_eq!(pack_light(99), 248, "levels above 15 clamp");
    }
}
