//! The overlay pass: the debug text, drawn over the finished frame in physical pixels.
//!
//! The overlay's geometry is one quad per set pixel of the embedded font, positioned in
//! physical pixels with `(0, 0)` at the window's top-left corner, and the pass draws it in a
//! render pass with no depth attachment: the text is never hidden by the terrain, and wgpu
//! rejects a pipeline with a depth state in such a pass, so the two cannot drift apart.
//!
//! The scale and the margin are M1 stand-ins for the vanilla F3 layout, which M6 owns.

use glam::Mat4;

use crate::debug_text::{PixelQuad, block_quads};

/// The overlay shader: map physical pixels to clip space through the orthographic projection.
///
/// The quads are opaque, so the fragment stage only has to hand the colour on.
const SHADER: &str = r#"
struct Overlay {
    ortho: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> overlay: Overlay;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = overlay.ortho * vec4<f32>(input.position, 0.0, 1.0);
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(input.color, 1.0);
}
"#;

/// The overlay's text scale in physical pixels per font pixel.
///
/// M1's stand-in for the vanilla layout; M6 owns the real F3 size.
const TEXT_SCALE: f32 = 2.0;
/// The margin between the text and the window's top-left corner, in physical pixels.
const TEXT_MARGIN: f32 = 4.0;
/// How far the shadow copy sits below and right of the text, in physical pixels.
const SHADOW_OFFSET: f32 = 1.0;
/// The text colour: opaque white.
const TEXT_COLOR: [f32; 3] = [1.0, 1.0, 1.0];
/// The shadow colour: dark enough to stay legible over a bright sky.
const SHADOW_COLOR: [f32; 3] = [0.05, 0.05, 0.05];
/// The size of one overlay vertex in the byte stream the GPU receives.
const OVERLAY_VERTEX_BYTES: usize = std::mem::size_of::<OverlayVertex>();
/// The size of the projection uniform in bytes: one `mat4x4<f32>`.
const UNIFORM_BYTES: usize = 64;

/// The overlay vertex attributes: a position at offset 0, a colour at offset 8.
static ATTRIBUTES: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x3];

/// One overlay vertex: a physical-pixel position and a colour.
#[derive(Debug, Clone, Copy, PartialEq)]
struct OverlayVertex {
    /// The position in physical pixels, `(0, 0)` at the window's top-left corner.
    position: [f32; 2],
    /// The colour; overlay colours are opaque.
    color: [f32; 3],
}

/// The vertex and index buffers of one uploaded frame of text.
struct Geometry {
    /// The vertex buffer, filled with [`overlay_vertex_bytes`] output.
    vertex_buffer: wgpu::Buffer,
    /// The index buffer, `u32` indices as little-endian bytes.
    index_buffer: wgpu::Buffer,
    /// The number of indices in the index buffer.
    index_count: u32,
}

/// The overlay pipeline and the text it draws.
pub struct OverlayPass {
    /// The pipeline: no depth state, no culling, opaque colours.
    pipeline: wgpu::RenderPipeline,
    /// The uniform buffer holding the orthographic projection.
    ortho_buffer: wgpu::Buffer,
    /// The bind group the pipeline reads the projection through.
    ortho_bind_group: wgpu::BindGroup,
    /// The geometry of the last uploaded text, or nothing while the overlay is hidden.
    geometry: Option<Geometry>,
}

impl OverlayPass {
    /// Builds the pipeline for colour attachments in `format`.
    ///
    /// The pipeline has no depth-stencil state, no culling and no blending: it is meant for a
    /// pass that attaches only the colour target the terrain pass has just drawn into, so
    /// wgpu rejects it in a pass that offers a depth attachment.
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
            bind_group_layouts: &[&ortho_layout],
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
            // No depth state: the overlay is drawn in a pass with no depth attachment.
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
        Self {
            pipeline,
            ortho_buffer,
            ortho_bind_group,
            geometry: None,
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

    /// Replaces the drawn text with `lines`, laid out from the top-left margin.
    ///
    /// Every set pixel becomes one `scale` × `scale` quad drawn twice: a shadow copy one pixel
    /// down and right first, then the text copy over it. An empty line list removes the
    /// geometry, which hides the overlay.
    ///
    /// [`OverlayPass::set_size`] must be called once before the first upload, so the
    /// projection matches the surface the text is drawn on.
    pub fn upload_text(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, lines: &[String]) {
        let quads = block_quads(lines, TEXT_MARGIN, TEXT_MARGIN, TEXT_SCALE);
        let (vertices, indices) = overlay_geometry(&quads);
        if indices.is_empty() {
            self.geometry = None;
            return;
        }
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide overlay vertices"),
            size: (vertices.len() * OVERLAY_VERTEX_BYTES) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&vertex_buffer, 0, &overlay_vertex_bytes(&vertices));
        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide overlay indices"),
            size: (indices.len() * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&index_buffer, 0, &index_bytes(&indices));
        self.geometry = Some(Geometry {
            vertex_buffer,
            index_buffer,
            index_count: indices.len() as u32,
        });
    }

    /// Draws the uploaded text, or nothing when the overlay is hidden.
    ///
    /// The pass must attach the colour target the terrain pass drew into and no depth
    /// attachment; the text is opaque, so it simply overwrites what is under it.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(geometry) = &self.geometry else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.ortho_bind_group, &[]);
        pass.set_vertex_buffer(0, geometry.vertex_buffer.slice(..));
        pass.set_index_buffer(geometry.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..geometry.index_count, 0, 0..1);
    }
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

/// The vertex buffer layout the pipeline reads, tied to [`OVERLAY_VERTEX_BYTES`] by the tests.
fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: OVERLAY_VERTEX_BYTES as wgpu::BufferAddress,
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

/// The colour target for one attachment in `format`: opaque, every channel written.
fn color_target(format: wgpu::TextureFormat) -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState {
        format,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    })
}

/// Builds the vertices and indices of `quads`: the shadow copy first, then the text copy.
///
/// Both copies are one buffer's worth of geometry, so a single draw covers the line; the
/// shadow comes first so the text always lands on top of it where the two overlap.
fn overlay_geometry(quads: &[PixelQuad]) -> (Vec<OverlayVertex>, Vec<u32>) {
    let mut vertices = Vec::with_capacity(quads.len() * 8);
    let mut indices = Vec::with_capacity(quads.len() * 12);
    for (offset, color) in [(SHADOW_OFFSET, SHADOW_COLOR), (0.0, TEXT_COLOR)] {
        for quad in quads {
            push_quad(quad, offset, color, &mut vertices, &mut indices);
        }
    }
    (vertices, indices)
}

/// Appends one quad to `vertices` and `indices`: four corners, then two triangles.
///
/// The corners run left top, left bottom, right bottom, right top, which is the winding the
/// mesher's faces use; the overlay culls nothing, so it only keeps the two consistent.
fn push_quad(
    quad: &PixelQuad,
    offset: f32,
    color: [f32; 3],
    vertices: &mut Vec<OverlayVertex>,
    indices: &mut Vec<u32>,
) {
    let base = vertices.len() as u32;
    let left = quad.x + offset;
    let top = quad.y + offset;
    let right = left + quad.width;
    let bottom = top + quad.height;
    for position in [[left, top], [left, bottom], [right, bottom], [right, top]] {
        vertices.push(OverlayVertex { position, color });
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
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
fn overlay_vertex_bytes(vertices: &[OverlayVertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * OVERLAY_VERTEX_BYTES);
    for vertex in vertices {
        for component in vertex.position {
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
        OVERLAY_VERTEX_BYTES, SHADOW_COLOR, SHADOW_OFFSET, TEXT_COLOR, color_target, ortho,
        overlay_geometry, primitive_state, vertex_layout,
    };
    use crate::debug_text::PixelQuad;

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
        assert_eq!(OVERLAY_VERTEX_BYTES, 20);
        assert_eq!(layout.array_stride, OVERLAY_VERTEX_BYTES as u64);
        assert_eq!(layout.step_mode, wgpu::VertexStepMode::Vertex);
        let [position, color] = layout.attributes else {
            panic!("two attributes, a position and a colour");
        };
        assert_eq!(position.shader_location, 0);
        assert_eq!(position.format, VertexFormat::Float32x2);
        assert_eq!(position.offset, 0);
        assert_eq!(color.shader_location, 1);
        assert_eq!(color.format, VertexFormat::Float32x3);
        assert_eq!(color.offset, 8);
        assert_eq!(color.offset + 12, layout.array_stride);
    }

    #[test]
    fn the_pipeline_culls_nothing_and_writes_opaque_colours() {
        let primitive = primitive_state();
        assert_eq!(primitive.topology, PrimitiveTopology::TriangleList);
        assert_eq!(primitive.cull_mode, None, "both windings draw");
        let target = color_target(TextureFormat::Rgba8Unorm).expect("a colour target");
        assert_eq!(target.blend, None, "overlay colours are opaque");
    }

    #[test]
    fn every_quad_is_drawn_twice_with_the_shadow_first() {
        let quads = [PixelQuad {
            x: 4.0,
            y: 6.0,
            width: 2.0,
            height: 2.0,
        }];
        let (vertices, indices) = overlay_geometry(&quads);
        assert_eq!(vertices.len(), 8);
        assert_eq!(indices.len(), 12);
        // The shadow copy comes first, one pixel down and right of the text copy.
        let shadow = &vertices[..4];
        assert!(shadow.iter().all(|vertex| vertex.color == SHADOW_COLOR));
        assert_eq!(
            shadow[0].position,
            [4.0 + SHADOW_OFFSET, 6.0 + SHADOW_OFFSET]
        );
        // The text copy follows, and reuses the quad's indices shifted by four vertices.
        let text = &vertices[4..];
        assert!(text.iter().all(|vertex| vertex.color == TEXT_COLOR));
        assert_eq!(text[0].position, [4.0, 6.0]);
        assert_eq!(indices, [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]);
    }
}
