//! The overlay pass: the debug text, drawn over the finished frame in physical pixels.
//!
//! The overlay's geometry is one textured quad per glyph of the jar's ascii font, positioned
//! in physical pixels with `(0, 0)` at the window's top-left corner, and the pass draws it in
//! a render pass with no depth attachment: the text is never hidden by the terrain, and wgpu
//! rejects a pipeline with a depth state in such a pass, so the two cannot drift apart. The
//! fragment stage multiplies the sampled sheet texel by the vertex colour and writes it
//! straight through — no src-alpha blend — keeping the source's alpha test as a discard (its
//! `GL_GREATER` 0.1 threshold, `EntityRenderer.java`:1168): the debug overlay's text runs
//! unblended, the state the world's HUD leaves in force, and the death view's does the same
//! (its gradient ends `disableBlend()`, `Gui.java`:114). The sheet is sampled nearest and
//! clamp-to-edge, the GUI's own sampler.
//!
//! The pure layout — the quads, their shadow copies and the pen — lives in
//! [`crate::debug_text`]. The pass draws nothing until [`OverlayPass::set_font`] gives it a
//! sheet, and [`OverlayPass::upload_text`] is a no-op when the lines did not change since the
//! last upload (the M1 backlog item 1's fix): the geometry buffers are reused and only
//! recreated when a longer text needs more room.
//!
//! The scale and the margin are M1 stand-ins for the vanilla F3 layout, which M6 owns.

use glam::Mat4;

use oxide_assets::font::{Font, FontError};
use oxide_assets::texture::Texture;

use crate::debug_text::{GlyphVertex, glyph_geometry};

/// The overlay shader: map physical pixels to clip space through the orthographic projection,
/// sample the font sheet and multiply the texel by the vertex colour.
///
/// The fragment writes straight through and discards the fragments at or below the alpha
/// test's 0.1 threshold, so a fully transparent texel leaves the frame alone and no
/// full-cell quad paints.
const SHADER: &str = r#"
struct Overlay {
    ortho: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> overlay: Overlay;
@group(1) @binding(0) var font_texture: texture_2d<f32>;
@group(1) @binding(1) var font_sampler: sampler;

struct VertexInput {
    @location(0) position: vec2<f32>,
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
    output.clip_position = overlay.ortho * vec4<f32>(input.position, 0.0, 1.0);
    output.uv = input.uv;
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(font_texture, font_sampler, input.uv) * input.color;
    if (texel.a <= 0.1) {
        discard;
    }
    return texel;
}
"#;

/// The overlay's text scale in physical pixels per font pixel.
///
/// M1's stand-in for the vanilla layout; M6 owns the real F3 size.
const TEXT_SCALE: f32 = 2.0;
/// The margin between the text and the window's top-left corner, in physical pixels.
const TEXT_MARGIN: f32 = 4.0;
/// The text colour: opaque white.
const TEXT_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// The size of one overlay vertex in the byte stream the GPU receives.
const GLYPH_VERTEX_BYTES: usize = std::mem::size_of::<GlyphVertex>();
/// The size of the projection uniform in bytes: one `mat4x4<f32>`.
const UNIFORM_BYTES: usize = 64;

/// The overlay vertex attributes: a position at offset 0, a uv at 8 and an RGBA colour at 16.
static ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];

/// The font sheet as the pass keeps it: the measured font, the sheet's size and the bind
/// group the pipeline samples through.
struct FontState {
    /// The measured widths the layout advances with.
    font: Font,
    /// The sheet's size in texels, for the quads' uvs.
    sheet_size: (u32, u32),
    /// The sheet's view and sampler, bound at group 1 for the draw.
    bind_group: wgpu::BindGroup,
}

/// The geometry of one uploaded text, with the buffers kept across uploads.
#[derive(Default)]
struct Geometry {
    /// The vertex buffer, filled with [`overlay_vertex_bytes`] output.
    vertex_buffer: Option<wgpu::Buffer>,
    /// The index buffer, `u32` indices as little-endian bytes.
    index_buffer: Option<wgpu::Buffer>,
    /// The vertex buffer's capacity in bytes; a longer text recreates it.
    vertex_capacity: u64,
    /// The index buffer's capacity in bytes; a longer text recreates it.
    index_capacity: u64,
    /// The number of indices to draw; zero draws nothing.
    index_count: u32,
}

impl Geometry {
    /// Replaces the stored geometry with `vertices` and `indices`, reusing the buffers when
    /// they are already large enough.
    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vertices: &[GlyphVertex],
        indices: &[u32],
    ) {
        let vertex_bytes = overlay_vertex_bytes(vertices);
        let index_bytes = index_bytes(indices);
        if self.vertex_capacity < vertex_bytes.len() as u64 {
            self.vertex_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("oxide overlay vertices"),
                size: vertex_bytes.len() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.vertex_capacity = vertex_bytes.len() as u64;
        }
        if self.index_capacity < index_bytes.len() as u64 {
            self.index_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("oxide overlay indices"),
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
        self.index_count = indices.len() as u32;
    }

    /// Keeps the buffers but draws nothing until the next upload.
    fn clear(&mut self) {
        self.index_count = 0;
    }
}

/// The overlay pipeline and the text it draws.
pub struct OverlayPass {
    /// The pipeline: no depth state, no culling, the sheet sampled and written straight
    /// through.
    pipeline: wgpu::RenderPipeline,
    /// The uniform buffer holding the orthographic projection.
    ortho_buffer: wgpu::Buffer,
    /// The bind group the pipeline reads the projection through.
    ortho_bind_group: wgpu::BindGroup,
    /// The layout the font sheet is bound through: built once, so every sheet's bind group
    /// and the pipeline agree.
    font_layout: wgpu::BindGroupLayout,
    /// The font and its sheet, once [`OverlayPass::set_font`] has landed.
    font: Option<FontState>,
    /// The geometry of the last uploaded text.
    geometry: Geometry,
    /// The lines the last completed upload carried; equal lines make the next upload a no-op.
    lines: Vec<String>,
}

impl OverlayPass {
    /// Builds the pipeline for colour attachments in `format`.
    ///
    /// The pipeline has no depth-stencil state and no culling: it is meant for a pass that
    /// attaches only the colour target the terrain pass has just drawn into, so wgpu rejects
    /// it in a pass that offers a depth attachment. Its fragment stage writes the sampled
    /// sheet straight through and discards the fragments at or below the alpha test's 0.1
    /// threshold, so a transparent glyph texel draws nothing.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oxide overlay shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let ortho_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide overlay ortho layout"),
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
        let font_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide overlay font layout"),
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
            label: Some("oxide overlay ortho"),
            size: UNIFORM_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ortho_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide overlay ortho bind group"),
            layout: &ortho_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: ortho_buffer.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("oxide overlay pipeline layout"),
            bind_group_layouts: &[&ortho_layout, &font_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("oxide overlay pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_layout()],
            },
            primitive: primitive_state(),
            // The hud pass attaches depth for the item draws, so the overlay states its
            // own: always passing and never writing.
            depth_stencil: Some(crate::terrain_pass::depth_state_off()),
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
        Self {
            pipeline,
            ortho_buffer,
            ortho_bind_group,
            font_layout,
            font: None,
            geometry: Geometry::default(),
            lines: Vec::new(),
        }
    }

    /// Rebuilds the orthographic projection for a surface size and writes it out.
    ///
    /// The projection maps `(0, 0)` to the top-left corner and `(width, height)` to the
    /// bottom-right, in the physical pixels the quads are laid out in. The queue is passed
    /// here rather than left to [`OverlayPass::upload_text`] so that a resize cannot leave a
    /// stale projection behind while the lines stay the same.
    pub fn set_size(&mut self, queue: &wgpu::Queue, width: f32, height: f32) {
        queue.write_buffer(&self.ortho_buffer, 0, &matrix_bytes(ortho(width, height)));
    }

    /// Uploads `sheet` and measures it as the font every following text draws with.
    ///
    /// The widths come from the sheet through [`Font::load`], so the geometry the next
    /// upload lays out and the sheet the fragments sample cannot disagree. Until this is
    /// called the pass draws nothing; the lines uploaded before it are laid out again with
    /// the new sheet's metrics. A sheet that is not a 16x16 grid is
    /// [`FontError`], and the pass keeps its previous font.
    pub fn set_font(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sheet: &Texture,
    ) -> Result<(), FontError> {
        let font = Font::load(sheet, None)?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("oxide overlay font sheet"),
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
        let sampler = device.create_sampler(&font_sampler_descriptor());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide overlay font bind group"),
            layout: &self.font_layout,
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
        self.font = Some(FontState {
            font,
            sheet_size: (sheet.width, sheet.height),
            bind_group,
        });
        // The lines the last upload carried were laid out with the previous sheet's metrics;
        // clearing them makes the next upload rebuild whatever the caller draws.
        self.lines.clear();
        self.geometry.clear();
        Ok(())
    }

    /// Replaces the drawn text with `lines`, laid out from the top-left margin.
    ///
    /// The shadow copies are laid out first and the text over them; see
    /// [`crate::debug_text::glyph_geometry`]. An empty line list removes the geometry, which
    /// hides the overlay. Calling this again with the same lines does nothing: the geometry
    /// and the sheet bindings the last upload made stay as they are, and the buffers are
    /// reused rather than recreated.
    ///
    /// [`OverlayPass::set_size`] must be called once before the first upload, so the
    /// projection matches the surface the text is drawn on. Nothing is drawn until
    /// [`OverlayPass::set_font`] has given the pass a sheet.
    pub fn upload_text(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, lines: &[String]) {
        if !lines_changed(&self.lines, lines) {
            return;
        }
        self.lines.clear();
        self.lines.extend_from_slice(lines);
        let Some(font) = &self.font else {
            self.geometry.clear();
            return;
        };
        if lines.is_empty() {
            self.geometry.clear();
            return;
        }
        let (vertices, indices) = glyph_geometry(
            &font.font,
            font.sheet_size,
            lines,
            [TEXT_MARGIN, TEXT_MARGIN],
            TEXT_SCALE,
            TEXT_COLOR,
        );
        self.geometry.upload(device, queue, &vertices, &indices);
    }

    /// Draws the uploaded text, or nothing when the overlay is hidden or has no font.
    ///
    /// The pass must attach the colour target the terrain pass drew into and no depth
    /// attachment; the text blends over what is under it.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(font) = &self.font else {
            return;
        };
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
        pass.set_bind_group(1, &font.bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.geometry.index_count, 0, 0..1);
    }
}

/// Whether `incoming` differs from the lines the last upload carried.
///
/// This is the whole of the upload cache's decision, kept pure so the tests can pin the
/// unchanged/unchanged/changed sequence without a GPU: equal lines mean the pass's geometry
/// already is what the caller wants, so the upload must do nothing.
pub fn lines_changed(uploaded: &[String], incoming: &[String]) -> bool {
    uploaded != incoming
}

/// The orthographic projection from physical pixels to clip space.
///
/// `(0, 0)` maps to the top-left corner of the frame and `(width, height)` to the
/// bottom-right, so pixel coordinates go in and the quads come out where the layout put them.
/// The z axis is unused: the overlay has no depth state, and a vertex on the z = 0 plane stays
/// inside the 0..1 clip range.
fn ortho(width: f32, height: f32) -> Mat4 {
    Mat4::orthographic_rh(0.0, width, height, 0.0, 0.0, 1.0)
}

/// The vertex buffer layout the pipeline reads, tied to [`GLYPH_VERTEX_BYTES`] by the tests.
fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: GLYPH_VERTEX_BYTES as wgpu::BufferAddress,
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

/// The colour target for one attachment in `format`: no blend — the sheet writes straight
/// through, the source's state for the debug overlay's glyph runs.
fn color_target(format: wgpu::TextureFormat) -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState {
        format,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    })
}

/// The sampler the font sheet is read through: nearest and clamp-to-edge, the GUI's own
/// sampler, so a texel is never blended with its neighbour and a uv on the sheet's edge
/// cannot wrap around to the opposite side.
fn font_sampler_descriptor() -> wgpu::SamplerDescriptor<'static> {
    wgpu::SamplerDescriptor {
        label: Some("oxide overlay font sampler"),
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
fn overlay_vertex_bytes(vertices: &[GlyphVertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * GLYPH_VERTEX_BYTES);
    for vertex in vertices {
        for component in vertex.position {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
        for component in vertex.uv {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
        for component in vertex.color {
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
    use glam::Vec4;
    use wgpu::{PrimitiveTopology, TextureFormat, VertexFormat};

    use super::{
        GLYPH_VERTEX_BYTES, color_target, font_sampler_descriptor, ortho, primitive_state,
        vertex_layout,
    };

    #[test]
    fn the_ortho_matrix_maps_pixels_to_the_corners_of_the_ndc() {
        let matrix = ortho(64.0, 32.0);
        let top_left = matrix * Vec4::new(0.0, 0.0, 0.0, 1.0);
        let centre = matrix * Vec4::new(32.0, 16.0, 0.0, 1.0);
        let bottom_right = matrix * Vec4::new(64.0, 32.0, 0.0, 1.0);
        assert_eq!((top_left.x, top_left.y), (-1.0, 1.0));
        assert_eq!((centre.x, centre.y), (0.0, 0.0));
        assert_eq!((bottom_right.x, bottom_right.y), (1.0, -1.0));
        // A vertex on the z = 0 plane stays inside the 0..1 clip range.
        assert_eq!(top_left.z, 0.0);
    }

    #[test]
    fn the_vertex_layout_matches_the_serialised_vertices() {
        let layout = vertex_layout();
        assert_eq!(GLYPH_VERTEX_BYTES, 32);
        assert_eq!(layout.array_stride, GLYPH_VERTEX_BYTES as u64);
        assert_eq!(layout.step_mode, wgpu::VertexStepMode::Vertex);
        let [position, uv, color] = layout.attributes else {
            panic!("three attributes: a position, a uv and a colour");
        };
        assert_eq!(position.shader_location, 0);
        assert_eq!(position.format, VertexFormat::Float32x2);
        assert_eq!(position.offset, 0);
        assert_eq!(uv.shader_location, 1);
        assert_eq!(uv.format, VertexFormat::Float32x2);
        assert_eq!(uv.offset, 8);
        assert_eq!(color.shader_location, 2);
        assert_eq!(color.format, VertexFormat::Float32x4);
        assert_eq!(color.offset, 16);
        assert_eq!(color.offset + 16, layout.array_stride);
    }

    #[test]
    fn the_pipeline_culls_nothing_and_writes_the_sheet_straight_through() {
        let primitive = primitive_state();
        assert_eq!(primitive.topology, PrimitiveTopology::TriangleList);
        assert_eq!(primitive.cull_mode, None, "both windings draw");
        let target = color_target(TextureFormat::Rgba8Unorm).expect("a colour target");
        assert_eq!(
            target.blend, None,
            "the overlay's glyph runs draw unblended: the source's blend state"
        );
    }

    #[test]
    fn the_font_sheet_is_sampled_nearest_and_clamped() {
        let sampler = font_sampler_descriptor();
        assert_eq!(sampler.mag_filter, wgpu::FilterMode::Nearest);
        assert_eq!(sampler.min_filter, wgpu::FilterMode::Nearest);
        assert_eq!(sampler.mipmap_filter, wgpu::FilterMode::Nearest);
        assert_eq!(sampler.address_mode_u, wgpu::AddressMode::ClampToEdge);
        assert_eq!(sampler.address_mode_v, wgpu::AddressMode::ClampToEdge);
        assert_eq!(sampler.address_mode_w, wgpu::AddressMode::ClampToEdge);
    }
}
