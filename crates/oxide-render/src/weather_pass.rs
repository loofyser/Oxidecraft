//! The rain/snow columns: one camera-facing ribbon per session column.
//!
//! `EntityRenderer.renderRainSnow:1594+` draws the weather after the
//! particles and before the translucent terrain, one vertical ribbon per
//! column of the `(2·radius+1)²` box the session computed
//! (`oxide_game::weather::weather_columns`), textured with the 64×256 rain
//! or snow sheet, scrolled per tick, faded by the column's alpha and lit by
//! its combined light. This pass is that draw: the client feeds the
//! session's columns with the tick they rode in on, and the pass builds the
//! ribbons.
//!
//! The port's recorded deviations from the source arm:
//! * The scroll advances per tick, not per frame: the sheets scroll
//!   `3` source frames' worth per tick (`SCROLL_FRAMES_PER_TICK`), so the
//!   streaks descend at the source's per-second rate while the tick is the
//!   only clock the session speaks.
//! * The rain arm's per-column phase (`hash & 31`) and speed (`3 + rand`)
//!   are the source's own hash evaluated with wrapping arithmetic; the speed
//!   factor reads the hash's next byte instead of a second `rand` draw, so a
//!   tick replays its scroll exactly.
//! * The snow arm's tick-driven sideways drift (`d9/d10`) is a static
//!   per-column offset from the same hash: the sheet scrolls but no longer
//!   sways.
//! * Thunder has no visual in the rain pass — only the fog dim (landed) and
//!   the lightning entity (recorded) — so the params carry no thunder.
//!
//! The ribbon is the source's quad extent: the column's centre ± a quarter
//! block along the camera's horizontal axis, spanning `y0..y1`, with
//! `v = y * 0.25 + scroll` down the sheet. The state around the draw is the
//! source's: `SRC_ALPHA/ONE_MINUS_SRC_ALPHA`, the 0.1 alpha test, no culling
//! and the depth test left writing.

use glam::Vec3;

use crate::camera::{Camera, FIRST_PERSON_OFFSET};
use crate::fog::{FogMode, FogParams};
use crate::lightmap::{BrightnessTable, lightmap_image};
use crate::terrain_pass::{color_target, depth_state, primitive_state};
use oxide_assets::texture::Texture;

/// One rain/snow column for the weather pass, as the session computed it.
///
/// The render-side mirror of `oxide_game::weather::WeatherColumn`: the
/// crate graph gives this crate no edge to `oxide-game`, so the client maps
/// the session's columns into these at the frame drain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeatherColumn {
    /// The column's block x.
    pub x: i32,
    /// The column's block z.
    pub z: i32,
    /// The streak's bottom block.
    pub y0: i32,
    /// The streak's top block.
    pub y1: i32,
    /// Whether the column draws the snow sheet.
    pub snow: bool,
    /// The distance-falloff alpha, rain strength included.
    pub alpha: f32,
    /// The block light at the column's light cell, 0-15.
    pub block_light: u8,
    /// The sky light at the column's light cell, 0-15.
    pub sky_light: u8,
}

/// The weather shader source: sample the bound sheet at the vertex's uv,
/// multiply by the lightmap's texel, scale the alpha by the vertex's, then
/// fade towards the frame's fog colour.
///
/// The camera, fog and lightmap groups copy the terrain pass's layout —
/// group 0 the frame's uniforms, group 1 the sheet, group 2 the lightmap —
/// so the two passes' frames read alike. The discard threshold is the
/// client's own tenth, in force at the source's weather draw.
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

const WEATHER_ALPHA_TEST: f32 = 0.1;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) light: vec2<u32>,
    @location(3) alpha: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) light: vec2<f32>,
    @location(2) alpha: f32,
    @location(3) distance: f32,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = camera.view_projection * vec4<f32>(input.position, 1.0);
    output.distance = length(input.position - camera.eye.xyz);
    output.uv = input.uv;
    output.light = vec2<f32>(f32(input.light.x), f32(input.light.y));
    output.alpha = input.alpha;
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
    let colour = fogged(vec4<f32>(texel.rgb, texel.a * input.alpha), input.distance);
    if (colour.a < WEATHER_ALPHA_TEST) {
        discard;
    }
    return colour;
}
"#
    .to_string()
}

/// The source frames one tick stands in for: the sheets scroll this many
/// source frames' worth per tick, so the streaks descend at the source's
/// per-second rate.
const SCROLL_FRAMES_PER_TICK: f32 = 3.0;

/// The ribbon's half width in blocks: the source's quad spans the column's
/// centre ± its `d3/d4` offsets, a quarter block each way.
const RIBBON_HALF_WIDTH: f32 = 0.25;

/// The sheet v one block of streak covers: the source's `y * 0.25`.
const V_PER_BLOCK: f32 = 0.25;

/// The snow scroll's period in source frames: `d8`'s `& 511`.
const SNOW_SCROLL_PERIOD: f32 = 512.0;

/// The size of the camera uniform in bytes: one `mat4x4<f32>` and the eye's
/// `vec4<f32>`, the terrain pass's own layout.
const CAMERA_BYTES: usize = 64 + 16;

/// The size of the fog uniform in bytes: two `vec4<f32>`, the terrain
/// pass's own layout.
const FOG_BYTES: usize = 32;

/// The lightmap's size in texels on each axis, the terrain pass's own.
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

/// The weather vertex attributes: a position at offset 0, the uv at 12, the
/// packed light pair at 20 and the alpha at 24.
static ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x2,
    2 => Uint16x2,
    3 => Float32
];

/// The stride of one weather vertex in bytes: 12 + 8 + 4 + 4.
const VERTEX_BYTES: usize = 28;

/// One weather vertex, in the [`ATTRIBUTES`] order.
#[derive(Debug, Clone, Copy)]
struct WeatherVertex {
    /// The world-space position.
    position: [f32; 3],
    /// The sheet uv.
    uv: [f32; 2],
    /// The packed light pair, each level shifted four bits with eight
    /// added — the client's own packing, as the terrain vertices carry it.
    light: [u16; 2],
    /// The column's alpha.
    alpha: f32,
}

/// The source's per-column hash: `l1*l1*3121 + l1*45238971 ^ k1*k1*418711 +
/// k1*13761` (`EntityRenderer.java:1594+`), in wrapping arithmetic.
fn column_hash(x: i32, z: i32) -> i32 {
    x.wrapping_mul(x)
        .wrapping_mul(3121)
        .wrapping_add(x.wrapping_mul(45_238_971))
        ^ z.wrapping_mul(z)
            .wrapping_mul(418_711)
            .wrapping_add(z.wrapping_mul(13_761))
}

/// The rain sheet's scroll at `tick` for a column of `hash`: the source's
/// `((rendererUpdateCount + hash & 31) + partial) / 32 * (3 + rand)`, with
/// the tick standing in for [`SCROLL_FRAMES_PER_TICK`] source frames and
/// the hash's next byte standing in for the second `rand` draw.
fn rain_scroll(tick: u32, hash: i32) -> f32 {
    let phase = f32::from((hash & 31) as u8);
    let speed = 3.0 + f32::from(((hash >> 5) & 0xFF) as u8) / 255.0;
    (tick as f32 * SCROLL_FRAMES_PER_TICK + phase) / 32.0 * speed
}

/// The snow sheet's scroll at `tick`: the source's `d8 =
/// ((updateCount & 511) + partial) / 512`, with the tick standing in for
/// [`SCROLL_FRAMES_PER_TICK`] source frames.
fn snow_scroll(tick: u32) -> f32 {
    ((tick as f32 * SCROLL_FRAMES_PER_TICK) % SNOW_SCROLL_PERIOD) / SNOW_SCROLL_PERIOD
}

/// The snow ribbon's static sideways offset in blocks: the source drifts
/// the snow quads per tick (`d9/d10`); this port holds the drift still at
/// the hash's own fraction, ± half a block.
fn snow_offset(hash: i32) -> f32 {
    f32::from((hash & 0xFF) as u8) / 255.0 - 0.5
}

/// Packs one light level the way the client's vertices carry it: the level
/// shifted four bits with eight added, so the shader's `/ 256` lands on the
/// texel's centre.
fn pack_light(level: u8) -> u16 {
    u16::from(level.min(15)) * 16 + 8
}

/// The ribbon's horizontal axis for a camera looking along `forward`: the
/// unit vector across the view, falling back to +x when the view is straight
/// down.
fn ribbon_axis(forward: Vec3) -> Vec3 {
    let flat = Vec3::new(forward.x, 0.0, forward.z);
    if flat.length_squared() < 1e-6 {
        Vec3::X
    } else {
        Vec3::new(-flat.z, 0.0, flat.x).normalize()
    }
}

/// One column's ribbon vertices: the centre ± [`RIBBON_HALF_WIDTH`] along
/// `axis` (plus the snow's static `offset`), spanning `y0..y1`, with `v`
/// running `y * 0.25 + scroll` down the sheet.
fn ribbon_vertices(
    column: &WeatherColumn,
    axis: Vec3,
    scroll: f32,
    offset: f32,
) -> [WeatherVertex; 4] {
    let centre = Vec3::new(
        column.x as f32 + 0.5 + offset * axis.x,
        0.0,
        column.z as f32 + 0.5 + offset * axis.z,
    );
    let half = axis * RIBBON_HALF_WIDTH;
    let light = [pack_light(column.block_light), pack_light(column.sky_light)];
    let alpha = column.alpha.clamp(0.0, 1.0);
    let y0 = column.y0 as f32;
    let y1 = column.y1 as f32;
    let corner = |side: f32, y: f32| WeatherVertex {
        position: (centre + half * side).with_y(y).to_array(),
        uv: [if side < 0.0 { 0.0 } else { 1.0 }, y * V_PER_BLOCK + scroll],
        light,
        alpha,
    };
    [
        corner(-1.0, y0),
        corner(1.0, y0),
        corner(1.0, y1),
        corner(-1.0, y1),
    ]
}

/// A sheet's uploaded texture, with the bind group the pass reads it
/// through.
struct SheetTexture {
    /// The bind group the pass draws the sheet's ribbons through.
    bind_group: wgpu::BindGroup,
    /// The texture handle, kept so the view above stays valid.
    #[allow(dead_code)]
    texture: wgpu::Texture,
}

impl SheetTexture {
    /// Uploads `sheet` as an `Rgba8Unorm` image, refusing a byte count that
    /// does not match its declared size with a one-texel stand-in.
    fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        sheet: &Texture,
        label: &str,
    ) -> Self {
        let (width, height, bytes) = if sheet.rgba.len()
            == (sheet.width * sheet.height * 4) as usize
        {
            (
                sheet.width.max(1),
                sheet.height.max(1),
                sheet.rgba.as_slice(),
            )
        } else {
            tracing::warn!(
                label,
                width = sheet.width,
                height = sheet.height,
                bytes = sheet.rgba.len(),
                "the weather sheet does not match its declared size; a one-texel stand-in is used"
            );
            (1, 1, &[255u8, 255u8, 255u8, 255u8] as &[u8])
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
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
            label: Some("oxide weather sheet sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide weather sheet bind group"),
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

/// One sheet's ribbons on the GPU: the vertex buffer and how many indices
/// the sheet's draw issues.
struct SheetMesh {
    /// The vertex buffer, filled with [`vertex_bytes`] output.
    vertex_buffer: wgpu::Buffer,
    /// The index buffer, `u32` indices as little-endian bytes.
    index_buffer: wgpu::Buffer,
    /// The number of indices in the index buffer.
    index_count: u32,
}

/// The rain/snow columns' pass: one pipeline, the frame's uniforms, the two
/// sheets and the lightmap they draw through, and the ribbons the last
/// upload built.
pub struct WeatherPass {
    /// The pipeline both sheets' ribbons draw through.
    pipeline: wgpu::RenderPipeline,
    /// The uniform buffer holding the frame's view-projection matrix.
    camera_buffer: wgpu::Buffer,
    /// The uniform buffer holding the frame's fog.
    fog_buffer: wgpu::Buffer,
    /// The bind group the pipeline reads the frame's uniforms through.
    frame_bind_group: wgpu::BindGroup,
    /// The lightmap's texture and sampler, and the group-2 bind group they
    /// are bound through; built with the pass and holding the image a fresh
    /// client holds.
    lightmap_bind_group: wgpu::BindGroup,
    /// The lightmap's texture handle, kept so
    /// [`WeatherPass::set_lightmap`] can rewrite its bytes when the sky's
    /// brightness changes.
    lightmap_texture: wgpu::Texture,
    /// The rain sheet, once one has been set.
    rain: Option<SheetTexture>,
    /// The snow sheet, once one has been set.
    snow: Option<SheetTexture>,
    /// The rain ribbons the last upload built.
    rain_mesh: Option<SheetMesh>,
    /// The snow ribbons the last upload built.
    snow_mesh: Option<SheetMesh>,
    /// The camera's forward at the last [`WeatherPass::set_camera`]: the
    /// axis the next upload billboards around.
    forward: Vec3,
}

impl WeatherPass {
    /// Builds the weather pipeline for colour attachments in `format`, and
    /// the lightmap it samples.
    ///
    /// The vertex layout is the byte stream [`vertex_bytes`] produces: a
    /// `Float32x3` position at offset 0, a `Float32x2` uv at 12, a `Uint16x2`
    /// light pair at 20 and a `Float32` alpha at 24. The pipeline blends
    /// `src_alpha` over `one_minus_src_alpha` with the source's 0.1 alpha
    /// test, culls nothing and tests and writes the depth buffer in
    /// [`DEPTH_FORMAT`]. Group 0 is the frame's uniforms — the camera matrix
    /// and the fog; group 1 is the sheet the draw binds per arm; group 2 is
    /// the lightmap, built here with the image a fresh client holds. The fog
    /// starts at [`NO_FOG`], so the first frame a caller draws is unfogged
    /// until [`WeatherPass::set_fog`] gives one.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oxide weather shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source().into()),
        });
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide weather frame layout"),
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
            label: Some("oxide weather sheet layout"),
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
            label: Some("oxide weather lightmap layout"),
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
            label: Some("oxide weather pipeline layout"),
            bind_group_layouts: &[&frame_layout, &sheet_layout, &lightmap_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("oxide weather pipeline"),
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
                targets: &[color_target(format, Some(weather_blend()))],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: primitive_state(None),
            depth_stencil: Some(depth_state(true)),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide weather camera"),
            size: CAMERA_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let fog_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide weather fog"),
            size: FOG_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&fog_buffer, 0, &fog_bytes(NO_FOG));
        let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide weather frame bind group"),
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
            label: Some("oxide weather lightmap"),
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
            label: Some("oxide weather lightmap sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let lightmap_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide weather lightmap bind group"),
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
        Self {
            pipeline,
            camera_buffer,
            fog_buffer,
            frame_bind_group,
            lightmap_bind_group,
            lightmap_texture,
            rain: None,
            snow: None,
            rain_mesh: None,
            snow_mesh: None,
            forward: Vec3::NEG_Z,
        }
    }

    /// Writes the camera's view-projection matrix and keeps its forward for
    /// the next upload's billboard axis.
    ///
    /// The renderer calls this once per frame with the surface's current
    /// aspect ratio, so a resize cannot leave a stale projection behind.
    pub fn set_camera(&mut self, queue: &wgpu::Queue, camera: Camera, aspect: f32) {
        let view_projection = camera.view_projection(aspect);
        let eye = camera.eye() - FIRST_PERSON_OFFSET * camera.forward();
        queue.write_buffer(&self.camera_buffer, 0, &camera_bytes(view_projection, eye));
        self.forward = camera.forward();
    }

    /// Uploads the rain and snow sheets the columns draw through.
    ///
    /// Until both have been set the arms whose sheet is missing draw
    /// nothing; a re-upload replaces the sheet whole.
    pub fn set_sheets(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rain: &Texture,
        snow: &Texture,
    ) {
        let layout = self.sheet_layout(device);
        self.rain = Some(SheetTexture::upload(
            device,
            queue,
            &layout,
            rain,
            "oxide weather rain",
        ));
        self.snow = Some(SheetTexture::upload(
            device,
            queue,
            &layout,
            snow,
            "oxide weather snow",
        ));
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

    /// Replaces the columns the next draw builds, scrolling the sheets by
    /// `tick`.
    ///
    /// Each column becomes one ribbon billboarded around the camera's
    /// forward at the last [`WeatherPass::set_camera`]; an empty slice
    /// clears both arms, so a settled tick draws nothing until the next
    /// upload.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        columns: &[WeatherColumn],
        tick: u32,
    ) {
        let axis = ribbon_axis(self.forward);
        let mut rain_vertices = Vec::new();
        let mut snow_vertices = Vec::new();
        for column in columns {
            if column.y1 <= column.y0 {
                continue;
            }
            let hash = column_hash(column.x, column.z);
            if column.snow {
                snow_vertices.extend_from_slice(&ribbon_vertices(
                    column,
                    axis,
                    snow_scroll(tick),
                    snow_offset(hash),
                ));
            } else {
                rain_vertices.extend_from_slice(&ribbon_vertices(
                    column,
                    axis,
                    rain_scroll(tick, hash),
                    0.0,
                ));
            }
        }
        self.rain_mesh = upload_sheet(device, queue, &rain_vertices, "oxide weather rain");
        self.snow_mesh = upload_sheet(device, queue, &snow_vertices, "oxide weather snow");
    }

    /// Draws the rain ribbons through the rain sheet and the snow ribbons
    /// through the snow sheet: the source's batch switch on the column kind.
    ///
    /// An arm whose sheet or mesh is missing draws nothing.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(2, &self.lightmap_bind_group, &[]);
        if let (Some(rain), Some(mesh)) = (self.rain.as_ref(), self.rain_mesh.as_ref()) {
            pass.set_bind_group(1, &rain.bind_group, &[]);
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.index_count, 0, 0..1);
        }
        if let (Some(snow), Some(mesh)) = (self.snow.as_ref(), self.snow_mesh.as_ref()) {
            pass.set_bind_group(1, &snow.bind_group, &[]);
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.index_count, 0, 0..1);
        }
    }

    /// The sheet bind-group layout: rebuilt per [`WeatherPass::set_sheets`]
    /// call because the pass keeps no layout handle.
    fn sheet_layout(&self, device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide weather sheet layout"),
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

/// The weather blend: `src_alpha` over `one_minus_src_alpha`, with the
/// target's alpha left as it stands — the translucent layer's own blend.
fn weather_blend() -> wgpu::BlendState {
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
fn vertex_bytes(vertex: &WeatherVertex) -> [u8; VERTEX_BYTES] {
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
    bytes[24..28].copy_from_slice(&vertex.alpha.to_le_bytes());
    bytes
}

/// Uploads one sheet's ribbons, or nothing for an empty vertex list: a
/// zero-sized buffer is not a buffer.
fn upload_sheet(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    vertices: &[WeatherVertex],
    label: &str,
) -> Option<SheetMesh> {
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
    Some(SheetMesh {
        vertex_buffer,
        index_buffer,
        index_count: indices.len() as u32,
    })
}

#[cfg(test)]
mod tests {
    //! The scroll maths, the hash and the ribbon builder, without a GPU.

    use super::*;

    #[test]
    fn the_column_hash_is_deterministic_and_spreads() {
        assert_eq!(column_hash(3, -2), column_hash(3, -2));
        assert_ne!(column_hash(3, -2), column_hash(-2, 3));
        assert_ne!(column_hash(0, 0), column_hash(1, 0));
    }

    #[test]
    fn the_rain_scroll_advances_per_tick_and_phases_per_column() {
        let a = rain_scroll(7, 0);
        let b = rain_scroll(8, 0);
        assert!(
            (b - a - SCROLL_FRAMES_PER_TICK / 32.0 * 3.0).abs() < 1e-6,
            "one tick advances three source frames at the zero hash's speed 3: {a} -> {b}"
        );
        assert_ne!(
            rain_scroll(7, 0),
            rain_scroll(7, 12345),
            "the hash phases the scroll"
        );
    }

    #[test]
    fn the_snow_scroll_wraps_its_period() {
        assert!((snow_scroll(0) - 0.0).abs() < 1e-6);
        assert!(
            (snow_scroll(512) - 0.0).abs() < 1e-4,
            "512 ticks advance 1536 source frames, a whole number of periods: {}",
            snow_scroll(512)
        );
    }

    #[test]
    fn the_light_packing_lands_on_texel_centres() {
        assert_eq!(pack_light(15), 248);
        assert_eq!(pack_light(0), 8);
        assert_eq!(pack_light(99), 248, "levels above 15 clamp");
    }

    #[test]
    fn the_ribbon_spans_the_extent_at_a_quarter_block_each_way() {
        let column = WeatherColumn {
            x: 0,
            z: 0,
            y0: 1,
            y1: 4,
            snow: false,
            alpha: 1.0,
            block_light: 15,
            sky_light: 15,
        };
        let corners = ribbon_vertices(&column, Vec3::X, 0.0, 0.0);
        let xs: Vec<f32> = corners.iter().map(|corner| corner.position[0]).collect();
        assert_eq!(xs, [0.25, 0.75, 0.75, 0.25]);
        assert_eq!(
            corners.map(|corner| corner.position[1]),
            [1.0, 1.0, 4.0, 4.0]
        );
        assert_eq!(corners.map(|corner| corner.uv[0]), [0.0, 1.0, 1.0, 0.0]);
        assert_eq!(corners[0].uv[1], 1.0 * V_PER_BLOCK);
        assert_eq!(corners[2].uv[1], 4.0 * V_PER_BLOCK);
        assert_eq!(corners[0].light, [248, 248]);
        assert_eq!(corners[0].alpha, 1.0);
    }

    #[test]
    fn the_ribbon_axis_falls_back_straight_down() {
        assert_eq!(ribbon_axis(Vec3::NEG_Y), Vec3::X);
        let axis = ribbon_axis(Vec3::Z);
        assert!(
            (axis - Vec3::X).length() < 1e-6 || (axis + Vec3::X).length() < 1e-6,
            "across a +z view: {axis:?}"
        );
    }
}
