//! The hud pass: the frame's GUI-space draw list, at the source's scaled resolution.
//!
//! The pass owns the scaled resolution (`ScaledResolution.java`:14-41), the vertex
//! buffers a frame's [`HudDraw`] list builds, and the pipeline that draws them into the
//! overlay's own pass — no depth attachment, alpha blended `src_alpha` over
//! `one_minus_src_alpha`, exactly [`crate::overlay`]'s state, so the frame invokes the
//! pass after the dim and before the debug overlay.
//!
//! The scale rule: the factor grows while one more step would keep `320x240` GUI pixels
//! on both axes (`ScaledResolution.java`:27-30) — the auto scale a zero gui-scale
//! setting means (`:22-25`) — and the scaled size is the ceiling of the display size
//! over the factor (`:37-40`). The unicode font's odd-factor stepdown (`:32-35`) does
//! not apply: the client draws with one font, not the unicode sheet.
//!
//! The primitives are the source's own: a solid rect samples a one-texel white texture,
//! so it runs the same pipeline as a textured rect (`Gui.drawRect` and
//! `drawTexturedModalRect` shapes), and text goes through [`crate::text::TextBuilder`]
//! in the draw's own scaled units with the source's shadow pass
//! (`FontRenderer.java`:341-358).

use glam::Mat4;
use oxide_assets::atlas::Atlas;
use oxide_assets::font::{Font, FontError};
use oxide_assets::texture::Texture;

use crate::atlas_texture::AtlasTexture;
use crate::text::{TextBuilder, TextVertex};

/// The hud shader: map scaled GUI pixels to clip space through the orthographic
/// projection, sample the bound texture and multiply the texel by the vertex colour.
///
/// One pipeline serves every primitive: a solid rect samples the one-texel white
/// texture, so its fragment is exactly its colour, and a glyph's texel alpha drives the
/// blend, so a fully transparent texel leaves the frame alone.
const SHADER: &str = r#"
struct Hud {
    ortho: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> hud: Hud;
@group(1) @binding(0) var hud_texture: texture_2d<f32>;
@group(1) @binding(1) var hud_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = hud.ortho * vec4<f32>(input.position, 1.0);
    output.uv = input.uv;
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(hud_texture, hud_sampler, input.uv) * input.color;
}
"#;

/// The GUI-space minimum the auto scale keeps on both axes (`ScaledResolution.java`:27).
const MIN_WIDTH: u32 = 320;

/// The GUI-space minimum height (`ScaledResolution.java`:27).
const MIN_HEIGHT: u32 = 240;

/// What a zero gui-scale setting means: keep stepping until the minimums stop the loop
/// (`ScaledResolution.java`:22-25).
const AUTO_SCALE: u32 = 1000;

/// The solid rects' uv: the white texture is one texel, so any uv inside it samples the
/// same colour; the centre keeps the lookup exact under any filtering.
const WHITE_UV: [f32; 4] = [0.5, 0.5, 0.5, 0.5];

/// The size of one hud vertex in the byte stream the GPU receives.
const VERTEX_BYTES: usize = std::mem::size_of::<TextVertex>();

/// The size of the projection uniform in bytes: one `mat4x4<f32>`.
const UNIFORM_BYTES: usize = 64;

/// The hud vertex attributes: a position at offset 0, a uv at 12 and an RGBA colour at
/// 20, [`TextVertex`]'s own layout.
static ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4];

/// The scaled resolution: the GUI-space size a frame lays out in, and the factor the
/// display was scaled down by (`ScaledResolution`'s three getters, `:43-66`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScaledResolution {
    /// The GUI-space width: `ceil(display width / factor)`.
    pub width: u32,
    /// The GUI-space height: `ceil(display height / factor)`.
    pub height: u32,
    /// The scale factor the display was divided by.
    pub scale_factor: u32,
}

/// The source's scaled resolution for a `display_width x display_height` window and a
/// gui-scale setting (`ScaledResolution.java`:14-41).
///
/// `gui_scale` zero is the auto setting: the factor grows while `factor + 1` still
/// leaves at least `320x240` GUI pixels on both axes, to the source's own thousand-step
/// ceiling (`:22-25`, `:27-30`). Any other value caps the growth there — the size guard
/// still applies, so a display too small for the setting stops at what fits (`:27`).
pub fn scaled_resolution(
    display_width: u32,
    display_height: u32,
    gui_scale: u8,
) -> ScaledResolution {
    let ceiling = if gui_scale == 0 {
        AUTO_SCALE
    } else {
        u32::from(gui_scale)
    };
    let mut factor = 1;
    while factor < ceiling
        && display_width / (factor + 1) >= MIN_WIDTH
        && display_height / (factor + 1) >= MIN_HEIGHT
    {
        factor += 1;
    }
    ScaledResolution {
        width: ceiling_div(display_width, factor),
        height: ceiling_div(display_height, factor),
        scale_factor: factor,
    }
}

/// `MathHelper.ceiling_double_int` of `value / divisor` (`ScaledResolution.java`:37-40).
fn ceiling_div(value: u32, divisor: u32) -> u32 {
    (f64::from(value) / f64::from(divisor)).ceil() as u32
}

/// One texture a [`HudDraw::TexturedRect`] samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudTexture {
    /// A texture registered under a name through [`HudPass::set_texture`].
    Named(&'static str),
    /// The block atlas, through [`HudPass::set_atlas`].
    Atlas,
}

/// One item of a frame's hud draw list, in GUI-space units at the scaled resolution.
///
/// The frame fills the list in painter's order — the source's own `drawRect` and
/// `drawString` call order — and the pass draws it in that order, back to front.
#[derive(Debug, Clone, PartialEq)]
pub enum HudDraw {
    /// A solid rectangle: the top-left corner, the size in GUI pixels and the straight
    /// (non-premultiplied) RGBA colour (`Gui.drawRect`'s shape).
    Rect {
        /// The left edge.
        x: f32,
        /// The top edge.
        y: f32,
        /// The width.
        width: f32,
        /// The height.
        height: f32,
        /// The RGBA colour.
        colour: [f32; 4],
    },
    /// A textured rectangle: the atlas or a registered texture, sampled at `uv` and
    /// tinted by `colour` (`Gui.drawTexturedModalRect`'s shape).
    TexturedRect {
        /// The texture to sample.
        texture: HudTexture,
        /// The left edge.
        x: f32,
        /// The top edge.
        y: f32,
        /// The width.
        width: f32,
        /// The height.
        height: f32,
        /// The uv rectangle `[u0, v0, u1, v1]`, `(0, 0)` the texture's top-left.
        uv: [f32; 4],
        /// The RGBA tint.
        colour: [f32; 4],
    },
    /// One text draw through the shared builder: the legacy `§`-coded string, the
    /// pen's start, GUI pixels per font pixel and the base RGBA
    /// (`FontRenderer.drawString`'s shape).
    Text {
        /// The draw's legacy `§`-coded text.
        text: String,
        /// The pen's start x, in GUI pixels.
        x: f32,
        /// The pen's start y.
        y: f32,
        /// GUI pixels per font pixel.
        scale: f32,
        /// The straight (non-premultiplied) RGBA base colour.
        colour: [f32; 4],
        /// Whether the darkened shadow copy draws under the text.
        shadow: bool,
    },
}

/// The font sheet as the pass keeps it: the measured font, the sheet's texel size and
/// the bind group the glyphs sample through.
struct FontSheet {
    /// The measured widths and glyph cells the layout reads.
    font: Font,
    /// The sheet's size in texels, for the quads' uvs.
    sheet: (u32, u32),
    /// The sheet's view and sampler, bound at group 1 for the draw.
    bind: wgpu::BindGroup,
}

/// The texture key one draw batches under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BatchTexture {
    /// The one-texel white texture solid rects sample.
    White,
    /// The font sheet.
    Font,
    /// A named registered texture.
    Named(&'static str),
    /// The block atlas.
    Atlas,
}

/// One consecutive run of draws that samples the same texture.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Batch {
    /// The texture the run samples.
    texture: BatchTexture,
    /// The run's index range into the shared index buffer.
    indices: std::ops::Range<u32>,
}

/// One draw list laid out as triangles: the vertices and indices, batched per texture
/// in draw order.
#[derive(Debug, Default, PartialEq)]
struct BuiltGeometry {
    /// The frame's vertices.
    vertices: Vec<TextVertex>,
    /// The frame's triangle-list indices into `vertices`.
    indices: Vec<u32>,
    /// The per-texture runs, in draw order.
    batches: Vec<Batch>,
}

/// Lays `draws` out as triangles.
///
/// `font` is the measured font and sheet size the text draws lay out against; a text
/// draw without one contributes nothing, like the overlay before its sheet. Consecutive
/// draws that sample the same texture share one batch, and the batches stay in draw
/// order, so the painter's order survives the batching.
fn build(draws: &[HudDraw], font: Option<(&Font, (u32, u32))>) -> BuiltGeometry {
    let mut built = BuiltGeometry::default();
    for draw in draws {
        let texture = match draw {
            HudDraw::Rect { .. } => BatchTexture::White,
            HudDraw::TexturedRect { texture, .. } => match texture {
                HudTexture::Named(name) => BatchTexture::Named(name),
                HudTexture::Atlas => BatchTexture::Atlas,
            },
            HudDraw::Text { .. } => BatchTexture::Font,
        };
        match draw {
            HudDraw::Rect {
                x,
                y,
                width,
                height,
                colour,
            } => {
                open_batch(&mut built, texture);
                push_quad(&mut built, (*x, *y), (*width, *height), WHITE_UV, *colour);
            }
            HudDraw::TexturedRect {
                x,
                y,
                width,
                height,
                uv,
                colour,
                ..
            } => {
                open_batch(&mut built, texture);
                push_quad(&mut built, (*x, *y), (*width, *height), *uv, *colour);
            }
            HudDraw::Text {
                text,
                x,
                y,
                scale,
                colour,
                shadow,
            } => {
                let Some((font, sheet)) = font else {
                    continue;
                };
                open_batch(&mut built, texture);
                let mut builder = TextBuilder::new();
                builder.push(text, [*x, *y, 0.0], *scale, *colour, *shadow);
                let (vertices, indices) = builder.geometry(font, sheet);
                let base = built.vertices.len() as u32;
                built.vertices.extend(vertices);
                built
                    .indices
                    .extend(indices.iter().map(|index| index + base));
                close_batch(&mut built);
            }
        }
    }
    built
}

/// Opens the batch `texture`'s next draws belong to: the last one when it samples the
/// same texture, a new one otherwise, and returns its position.
fn open_batch(built: &mut BuiltGeometry, texture: BatchTexture) -> usize {
    if built.batches.last().map(|batch| batch.texture) != Some(texture) {
        built.batches.push(Batch {
            texture,
            indices: built.indices.len() as u32..0,
        });
    }
    built.batches.len() - 1
}

/// Marks the open batch's index range up to the indices pushed so far.
fn close_batch(built: &mut BuiltGeometry) {
    if let Some(batch) = built.batches.last_mut() {
        batch.indices.end = built.indices.len() as u32;
    }
}

/// Appends one axis-aligned quad as two triangles, with every vertex at `uv`'s corners
/// and carrying `colour`.
fn push_quad(
    built: &mut BuiltGeometry,
    at: (f32, f32),
    size: (f32, f32),
    uv: [f32; 4],
    colour: [f32; 4],
) {
    let (x, y) = at;
    let (width, height) = size;
    let [u0, v0, u1, v1] = uv;
    let base = built.vertices.len() as u32;
    for (position, uv) in [
        ([x, y, 0.0], [u0, v0]),
        ([x, y + height, 0.0], [u0, v1]),
        ([x + width, y + height, 0.0], [u1, v1]),
        ([x + width, y, 0.0], [u1, v0]),
    ] {
        built.vertices.push(TextVertex {
            position,
            uv,
            colour,
        });
    }
    built
        .indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    close_batch(built);
}

/// The uploaded geometry of one draw list, with the buffers kept across uploads.
#[derive(Default)]
struct Geometry {
    /// The vertex buffer.
    vertex_buffer: Option<wgpu::Buffer>,
    /// The vertex buffer's capacity in bytes; a longer list recreates it.
    vertex_capacity: u64,
    /// The index buffer.
    index_buffer: Option<wgpu::Buffer>,
    /// The index buffer's capacity in bytes; a longer list recreates it.
    index_capacity: u64,
    /// The number of indices to draw; zero draws nothing.
    index_count: u32,
    /// The per-texture runs, in draw order.
    batches: Vec<Batch>,
}

impl Geometry {
    /// Replaces the stored geometry with `built`, reusing the buffers when they are
    /// already large enough.
    fn store(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, built: BuiltGeometry) {
        let vertex_bytes = vertex_bytes(&built.vertices);
        let index_bytes = index_bytes(&built.indices);
        if self.vertex_capacity < vertex_bytes.len() as u64 {
            self.vertex_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("oxide hud vertices"),
                size: vertex_bytes.len() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.vertex_capacity = vertex_bytes.len() as u64;
        }
        if self.index_capacity < index_bytes.len() as u64 {
            self.index_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("oxide hud indices"),
                size: index_bytes.len() as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.index_capacity = index_bytes.len() as u64;
        }
        if !vertex_bytes.is_empty() {
            if let Some(buffer) = &self.vertex_buffer {
                queue.write_buffer(buffer, 0, &vertex_bytes);
            }
        }
        if !index_bytes.is_empty() {
            if let Some(buffer) = &self.index_buffer {
                queue.write_buffer(buffer, 0, &index_bytes);
            }
        }
        self.index_count = built.indices.len() as u32;
        self.batches = built.batches;
    }
}

/// The hud pass's GPU state.
pub struct HudPass {
    /// The pipeline: GUI-space triangles, sampled texel times vertex colour.
    pipeline: wgpu::RenderPipeline,
    /// The uniform buffer holding the scaled-resolution orthographic projection.
    ortho_buffer: wgpu::Buffer,
    /// The bind group the pipeline reads the projection through.
    ortho_bind_group: wgpu::BindGroup,
    /// The layout every drawn texture is bound through: built once, so the pipeline,
    /// the white texel, the font sheet and every registered texture agree.
    texture_layout: wgpu::BindGroupLayout,
    /// The sampler every hud texture is read through: nearest and clamp-to-edge.
    sampler: wgpu::Sampler,
    /// The one-texel white texture solid rects sample.
    white_bind: wgpu::BindGroup,
    /// The font sheet, once [`HudPass::set_font`] has landed.
    font: Option<FontSheet>,
    /// The block atlas' bind group, once [`HudPass::set_atlas`] has landed.
    atlas: Option<wgpu::BindGroup>,
    /// The named textures [`HudPass::set_texture`] registered.
    textures: Vec<(&'static str, wgpu::BindGroup)>,
    /// The last draw list the frame handed over, kept so a late font still lays out.
    draws: Vec<HudDraw>,
    /// The uploaded geometry of that list.
    geometry: Geometry,
}

impl HudPass {
    /// Builds the pipeline for colour attachments in `format`.
    ///
    /// The pipeline has no depth-stencil state and no culling: it is meant for the
    /// pass that attaches only the colour target the terrain pass has just drawn into,
    /// so wgpu rejects it in a pass that offers a depth attachment. Its fragment stage
    /// blends the sampled texel with `src_alpha / one_minus_src_alpha`, the client's
    /// own pair.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oxide hud shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let ortho_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide hud ortho layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES as u64),
                },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide hud texture layout"),
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
        let ortho_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide hud ortho"),
            size: UNIFORM_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ortho_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide hud ortho bind group"),
            layout: &ortho_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: ortho_buffer.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("oxide hud pipeline layout"),
            bind_group_layouts: &[&ortho_layout, &texture_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("oxide hud pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_layout()],
            },
            primitive: primitive_state(),
            // No depth state: the hud draws in a pass with no depth attachment.
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[color_target(format)],
            }),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&texture_sampler_descriptor());
        let white_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide hud white bind group"),
            layout: &texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&white_texture(device, queue)),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        Self {
            pipeline,
            ortho_buffer,
            ortho_bind_group,
            texture_layout,
            sampler,
            white_bind,
            font: None,
            atlas: None,
            textures: Vec::new(),
            draws: Vec::new(),
            geometry: Geometry::default(),
        }
    }

    /// Rebuilds the orthographic projection for a scaled resolution and writes it out.
    ///
    /// The projection maps `(0, 0)` to the top-left corner and `(width, height)` to
    /// the bottom-right, in GUI pixels the draw list is laid out in. The queue is
    /// passed here rather than left to [`HudPass::set_draws`] so that a resize cannot
    /// leave a stale projection behind while the draws stay the same.
    pub fn set_resolution(&mut self, queue: &wgpu::Queue, width: f32, height: f32) {
        queue.write_buffer(&self.ortho_buffer, 0, &matrix_bytes(ortho(width, height)));
    }

    /// Uploads `sheet` and measures it as the font every following text draws with.
    ///
    /// The widths come from the sheet through [`Font::load`], so the geometry the next
    /// upload lays out and the sheet the fragments sample cannot disagree. The draws
    /// from before the sheet are laid out again with the new metrics; until a sheet
    /// lands, text contributes nothing. A sheet that is not a 16x16 grid is
    /// [`FontError`], and the pass keeps its previous font.
    pub fn set_font(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sheet: &Texture,
    ) -> Result<(), FontError> {
        let font = Font::load(sheet, None)?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("oxide hud font sheet"),
            size: wgpu::Extent3d {
                width: sheet.width,
                height: sheet.height,
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
            &sheet.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(sheet.width * 4),
                rows_per_image: Some(sheet.height),
            },
            wgpu::Extent3d {
                width: sheet.width,
                height: sheet.height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide hud font bind group"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.font = Some(FontSheet {
            font,
            sheet: (sheet.width, sheet.height),
            bind,
        });
        // The stored list was laid out with the previous sheet's metrics, or with no
        // sheet at all: lay it out again.
        self.rebuild(device, queue);
        Ok(())
    }

    /// Registers `texture` under `name`, for [`HudDraw::TexturedRect`]s that sample
    /// [`HudTexture::Named`]. A later call under the same name replaces it.
    pub fn set_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        name: &'static str,
        texture: &Texture,
    ) {
        let gpu_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("oxide hud named texture"),
            size: wgpu::Extent3d {
                width: texture.width,
                height: texture.height,
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
                texture: &gpu_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &texture.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(texture.width * 4),
                rows_per_image: Some(texture.height),
            },
            wgpu::Extent3d {
                width: texture.width,
                height: texture.height,
                depth_or_array_layers: 1,
            },
        );
        let view = gpu_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide hud named texture bind group"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        match self.textures.iter_mut().find(|(key, _)| *key == name) {
            Some(entry) => entry.1 = bind,
            None => self.textures.push((name, bind)),
        }
    }

    /// Uploads `atlas` and binds its view for [`HudDraw::TexturedRect`]s that sample
    /// [`HudTexture::Atlas`].
    ///
    /// The upload is [`AtlasTexture`]'s, the same levels-and-mips shape the terrain
    /// pass draws with; the plain sampler is the atlas' own nearest one.
    pub fn set_atlas(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, atlas: &Atlas) {
        let texture = AtlasTexture::upload(device, queue, atlas);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide hud atlas bind group"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(texture.view()),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(texture.plain_sampler()),
                },
            ],
        });
        self.atlas = Some(bind);
    }

    /// Replaces the drawn list with `draws`.
    ///
    /// Calling this again with an equal list does nothing: the geometry and the
    /// bindings the last upload made stay as they are, and the buffers are reused
    /// rather than recreated. Nothing is drawn until [`HudPass::set_resolution`] has
    /// given the pass a projection, and text waits for [`HudPass::set_font`].
    pub fn set_draws(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, draws: &[HudDraw]) {
        if self.draws == draws {
            return;
        }
        self.draws.clear();
        self.draws.extend_from_slice(draws);
        self.rebuild(device, queue);
    }

    /// Lays the stored draw list out again, with the current font.
    fn rebuild(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let font = self.font.as_ref().map(|sheet| (&sheet.font, sheet.sheet));
        let built = build(&self.draws, font);
        self.geometry.store(device, queue, built);
    }

    /// Draws the stored list, or nothing when it is empty.
    ///
    /// The pass must attach the colour target the dim pass has just drawn into and no
    /// depth attachment; the draws blend over what is under them. A batch whose
    /// texture was never set is skipped.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let (Some(vertex_buffer), Some(index_buffer)) = (
            self.geometry.vertex_buffer.as_ref(),
            self.geometry.index_buffer.as_ref(),
        ) else {
            return;
        };
        if self.geometry.index_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.ortho_bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        for batch in &self.geometry.batches {
            let bind = match batch.texture {
                BatchTexture::White => &self.white_bind,
                BatchTexture::Font => match &self.font {
                    Some(sheet) => &sheet.bind,
                    None => continue,
                },
                BatchTexture::Named(name) => {
                    match self.textures.iter().find(|(key, _)| *key == name) {
                        Some((_, bind)) => bind,
                        None => continue,
                    }
                }
                BatchTexture::Atlas => match &self.atlas {
                    Some(bind) => bind,
                    None => continue,
                },
            };
            pass.set_bind_group(1, bind, &[]);
            pass.draw_indexed(batch.indices.clone(), 0, 0..1);
        }
    }
}

/// The one-texel white texture solid rects sample: opaque white, written at creation so
/// the bind group's view is never blank.
fn white_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("oxide hud white texel"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
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
        &[255, 255, 255, 255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// The orthographic projection from GUI pixels to clip space.
///
/// `(0, 0)` maps to the top-left corner of the frame and `(width, height)` to the
/// bottom-right, so GUI coordinates go in and the quads come out where the layout put
/// them. The z axis is unused: the hud has no depth state, and a vertex on the z = 0
/// plane stays inside the 0..1 clip range.
fn ortho(width: f32, height: f32) -> Mat4 {
    Mat4::orthographic_rh(0.0, width, height, 0.0, 0.0, 1.0)
}

/// The vertex buffer layout the pipeline reads, tied to [`VERTEX_BYTES`] by the tests.
fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: VERTEX_BYTES as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    }
}

/// The primitive state: triangles of both windings, because the quads are axis-aligned.
fn primitive_state() -> wgpu::PrimitiveState {
    wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        front_face: wgpu::FrontFace::Ccw,
        cull_mode: None,
        ..Default::default()
    }
}

/// The colour target for one attachment in `format`: the fragment's alpha blends over
/// the frame with the client's own `src_alpha / one_minus_src_alpha` pair.
fn color_target(format: wgpu::TextureFormat) -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState {
        format,
        blend: Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        }),
        write_mask: wgpu::ColorWrites::ALL,
    })
}

/// The sampler every hud texture is read through: nearest and clamp-to-edge, the GUI's
/// own sampler, so a texel is never blended with its neighbour and a uv on a texture's
/// edge cannot wrap around to the opposite side.
fn texture_sampler_descriptor() -> wgpu::SamplerDescriptor<'static> {
    wgpu::SamplerDescriptor {
        label: Some("oxide hud sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    }
}

/// Packs a matrix into the 64 little-endian bytes of a WGSL `mat4x4<f32>`.
///
/// WGSL lays a uniform matrix out as four columns of four `f32`, which is the order
/// [`Mat4::to_cols_array`] returns, so the components go out as they are.
fn matrix_bytes(matrix: Mat4) -> [u8; UNIFORM_BYTES] {
    let mut bytes = [0u8; UNIFORM_BYTES];
    for (index, component) in matrix.to_cols_array().iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&component.to_le_bytes());
    }
    bytes
}

/// Packs the vertices into the byte stream the vertex buffer holds.
fn vertex_bytes(vertices: &[TextVertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * VERTEX_BYTES);
    for vertex in vertices {
        for component in vertex.position {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
        for component in vertex.uv {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
        for component in vertex.colour {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
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
    //! The scale rule and the geometry's arithmetic, without a GPU.

    use super::{
        Batch, BatchTexture, BuiltGeometry, HudDraw, HudTexture, MIN_HEIGHT, MIN_WIDTH,
        VERTEX_BYTES, WHITE_UV, build, scaled_resolution, vertex_layout,
    };
    use oxide_assets::font::Font;
    use oxide_assets::texture::Texture;

    /// A 128x128 synthetic sheet whose `'A'` cell is inked in columns 0..=4: the same
    /// metric the game's chat suite and the text passes' tests measure with.
    fn font() -> Font {
        const SIDE: u32 = 128;
        const CELL: u32 = 8;
        let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
        let code = 'A' as u32;
        let cell_x = (code % 16) * CELL;
        let cell_y = (code / 16) * CELL;
        for row in 0..CELL {
            for column in 0..=4 {
                let offset = (((cell_y + row) * SIDE + cell_x + column) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        Font::load(
            &Texture {
                width: SIDE,
                height: SIDE,
                rgba,
            },
            None,
        )
        .expect("the synthetic sheet loads")
    }

    #[test]
    fn the_scaled_resolution_keeps_the_sources_minimum() {
        // 1280x720 at the settings default (scale zero): one more step would leave
        // 1280/4 = 320 on the width but only 720/4 = 180 on the height, under the
        // 240 minimum, so the factor is three and the scaled size is the ceiling of
        // the display over it: 427x240 (`ScaledResolution.java`:27-30, `:37-40`).
        let scaled = scaled_resolution(1280, 720, 0);
        assert_eq!(
            (scaled.width, scaled.height, scaled.scale_factor),
            (427, 240, 3)
        );
    }

    #[test]
    fn the_scaled_resolution_steps_up_with_a_larger_window() {
        // 1920x1080: five steps would leave 1080/5 = 216, under the minimum, so the
        // factor is four and the scaled size 480x270 (`ScaledResolution.java`:27-30).
        let scaled = scaled_resolution(1920, 1080, 0);
        assert_eq!(
            (scaled.width, scaled.height, scaled.scale_factor),
            (480, 270, 4)
        );
    }

    #[test]
    fn an_explicit_gui_scale_caps_the_auto_loop() {
        // GuiScale 2 on 1280x720: the auto loop would reach three, the setting stops
        // it at two (`ScaledResolution.java`:22-25, `:27-30`).
        let scaled = scaled_resolution(1280, 720, 2);
        assert_eq!(
            (scaled.width, scaled.height, scaled.scale_factor),
            (640, 360, 2)
        );
        // GuiScale 1 is the ceiling exactly.
        let scaled = scaled_resolution(1280, 720, 1);
        assert_eq!(
            (scaled.width, scaled.height, scaled.scale_factor),
            (1280, 720, 1)
        );
    }

    #[test]
    fn a_display_under_the_minimum_stays_at_factor_one() {
        // The size guard runs whatever the setting says: 320x240 with GuiScale 3 has
        // no room for a second step, so the factor stays one
        // (`ScaledResolution.java`:27).
        let scaled = scaled_resolution(320, 240, 3);
        assert_eq!(
            (scaled.width, scaled.height, scaled.scale_factor),
            (320, 240, 1)
        );
    }

    #[test]
    fn the_scaled_size_is_the_ceiling_of_the_division() {
        // 1000/3 does not divide: the width rounds up (`ScaledResolution.java`:37-40).
        let scaled = scaled_resolution(1000, 720, 0);
        assert_eq!((scaled.width, scaled.height), (334, 240));
    }

    #[test]
    fn one_bar_and_its_text_batch_and_lay_out() {
        let font = font();
        let draws = [
            HudDraw::Rect {
                x: 2.0,
                y: 203.0,
                width: 324.0,
                height: 9.0,
                colour: [0.0, 0.0, 0.0, 127.0 / 255.0],
            },
            HudDraw::Text {
                text: "A§r".to_owned(),
                x: 2.0,
                y: 204.0,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: true,
            },
        ];
        let built = build(&draws, Some((&font, (128, 128))));
        // The bar's quad and the text's two copies of the one glyph, shadow first.
        assert_eq!(built.vertices.len(), 4 + 8);
        assert_eq!(built.indices.len(), 6 + 12);
        // The bar first, then the text, one batch each.
        assert_eq!(
            built.batches,
            vec![
                Batch {
                    texture: BatchTexture::White,
                    indices: 0..6,
                },
                Batch {
                    texture: BatchTexture::Font,
                    indices: 6..18,
                },
            ]
        );
        // The bar's corners and its white-texel uv; the source's drawRect shape.
        let corners: Vec<[f32; 3]> = built.vertices[..4]
            .iter()
            .map(|vertex| vertex.position)
            .collect();
        assert_eq!(
            corners,
            vec![
                [2.0, 203.0, 0.0],
                [2.0, 212.0, 0.0],
                [326.0, 212.0, 0.0],
                [326.0, 203.0, 0.0],
            ]
        );
        for vertex in &built.vertices[..4] {
            assert_eq!(vertex.uv, [WHITE_UV[0], WHITE_UV[1]]);
            assert_eq!(vertex.colour, [0.0, 0.0, 0.0, 127.0 / 255.0]);
        }
        // The text's shadow copy one pixel down and right, at the darkened colour,
        // then the text itself at the draw's own place (`FontRenderer.java`:341-358).
        assert_eq!(built.vertices[4].position, [3.0, 205.0, 0.0]);
        assert_eq!(built.vertices[4].colour[0], 63.0 / 255.0);
        assert_eq!(built.vertices[8].position, [2.0, 204.0, 0.0]);
        assert_eq!(built.vertices[8].colour, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn a_text_draw_without_a_font_contributes_nothing() {
        let draws = [
            HudDraw::Rect {
                x: 0.0,
                y: 0.0,
                width: 4.0,
                height: 4.0,
                colour: [1.0, 1.0, 1.0, 1.0],
            },
            HudDraw::Text {
                text: "A".to_owned(),
                x: 0.0,
                y: 0.0,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: false,
            },
        ];
        let built = build(&draws, None);
        assert_eq!(built.vertices.len(), 4, "the bar keeps its quad");
        assert_eq!(built.batches.len(), 1);
    }

    #[test]
    fn a_textured_rect_batches_under_its_own_texture() {
        let draws = [
            HudDraw::Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
                colour: [1.0; 4],
            },
            HudDraw::TexturedRect {
                texture: HudTexture::Atlas,
                x: 0.0,
                y: 0.0,
                width: 16.0,
                height: 16.0,
                uv: [0.0, 0.0, 0.25, 0.25],
                colour: [1.0; 4],
            },
            HudDraw::TexturedRect {
                texture: HudTexture::Named("gui/icons"),
                x: 0.0,
                y: 0.0,
                width: 16.0,
                height: 16.0,
                uv: [0.0, 0.0, 0.25, 0.25],
                colour: [1.0; 4],
            },
        ];
        let built: BuiltGeometry = build(&draws, None);
        assert_eq!(
            built
                .batches
                .iter()
                .map(|batch| batch.texture)
                .collect::<Vec<BatchTexture>>(),
            vec![
                BatchTexture::White,
                BatchTexture::Atlas,
                BatchTexture::Named("gui/icons"),
            ]
        );
        assert_eq!(built.vertices[4].uv, [0.0, 0.0]);
        assert_eq!(built.vertices[6].uv, [0.25, 0.25]);
    }

    #[test]
    fn the_vertex_layout_matches_the_serialised_vertices() {
        let layout = vertex_layout();
        assert_eq!(VERTEX_BYTES, 36);
        assert_eq!(layout.array_stride, VERTEX_BYTES as u64);
        let [position, uv, colour] = layout.attributes else {
            panic!("three attributes: a position, a uv and a colour");
        };
        assert_eq!(position.shader_location, 0);
        assert_eq!(position.format, wgpu::VertexFormat::Float32x3);
        assert_eq!(position.offset, 0);
        assert_eq!(uv.shader_location, 1);
        assert_eq!(uv.format, wgpu::VertexFormat::Float32x2);
        assert_eq!(uv.offset, 12);
        assert_eq!(colour.shader_location, 2);
        assert_eq!(colour.format, wgpu::VertexFormat::Float32x4);
        assert_eq!(colour.offset, 20);
        assert_eq!(colour.offset + 16, layout.array_stride);
    }

    #[test]
    fn the_scale_minimums_are_the_sources_own() {
        assert_eq!((MIN_WIDTH, MIN_HEIGHT), (320, 240));
    }
}
