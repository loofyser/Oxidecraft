//! The terrain pipeline and the section mesh table it draws.
//!
//! The pipeline reads the byte stream [`vertex_bytes`] produces — a position and a colour,
//! three `f32` components each — and transforms the positions with the camera's
//! view-projection matrix. Fragment colours are written through unchanged: the mesher has
//! already multiplied the palette entry by the face's brightness, so M1 has no texturing and
//! no lighting to do here.
//!
//! The winding is load-bearing. The mesher emits every face counter-clockwise seen from
//! outside the block, so the pipeline declares counter-clockwise front faces and culls back
//! faces: a face wound the other way, or a pipeline that culls the wrong side, disappears, and
//! the headless pipeline test's read-back fails on it. The same test fails if the depth test,
//! the depth write or the depth attachment stops working.
//!
//! Depth: the camera's projection maps the near plane to 0 and the far plane to 1, which is
//! the convention [`DEPTH_FORMAT`] with [`wgpu::CompareFunction::Less`] expects. The pipeline
//! tests and writes depth, so the nearest surface wins whatever order the meshes draw in.

use std::collections::HashMap;

use glam::Mat4;

use crate::camera::Camera;
use crate::terrain::{ChunkMesh, SectionKey, VERTEX_BYTES, vertex_bytes};

/// The terrain shader: transform a position with the camera, pass the colour through.
///
/// The mesh's colours are already lit — the mesher multiplies the atlas entry by the face's
/// brightness and the biome tint and packs the light with the vertex — so the fragment stage
/// writes its input unchanged.
///
/// The uv and light attributes are declared for the stages that follow (the atlas sampler and
/// the light ramp) and are not read here yet.
const SHADER: &str = r#"
struct Camera {
    view_projection: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) light: vec2<u32>,
    @location(3) colour: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) colour: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = camera.view_projection * vec4<f32>(input.position, 1.0);
    output.colour = input.colour;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.colour;
}
"#;

/// The depth format the pipeline tests and writes against.
///
/// The renderer builds its depth texture with this format, and the depth state of the
/// pipeline declares it; the projection maps the near plane to depth 0 and the far plane to
/// depth 1, as [`wgpu::CompareFunction::Less`] over a 0..1 range expects.
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

/// A section's mesh on the GPU: its two buffers and how many indices to draw.
struct GpuMesh {
    /// The vertex buffer, filled with [`vertex_bytes`] output.
    vertex_buffer: wgpu::Buffer,
    /// The index buffer, `u32` indices as little-endian bytes.
    index_buffer: wgpu::Buffer,
    /// The number of indices in the index buffer.
    index_count: u32,
}

/// The terrain pipeline and the meshes it draws.
pub struct TerrainPass {
    /// The pipeline: counter-clockwise front faces, back faces culled, depth test and write.
    pipeline: wgpu::RenderPipeline,
    /// The uniform buffer holding the frame's view-projection matrix.
    camera_buffer: wgpu::Buffer,
    /// The bind group the pipeline reads the uniform through.
    camera_bind_group: wgpu::BindGroup,
    /// Every section's mesh, keyed by section.
    meshes: HashMap<SectionKey, GpuMesh>,
}

impl TerrainPass {
    /// Builds the pipeline for colour attachments in `format`.
    ///
    /// The vertex layout is the byte stream [`vertex_bytes`] produces: a `Float32x3` position
    /// at offset 0, a `Float32x2` uv at 12, a `Uint16x2` light pair at 20 and a `Unorm8x4`
    /// colour at 24, with a stride of [`VERTEX_BYTES`]. The pipeline culls back faces and
    /// tests and writes the depth buffer in [`DEPTH_FORMAT`], so a pass that draws with it
    /// needs a depth attachment of that format and a colour attachment in `format`.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oxide terrain shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
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
            bind_group_layouts: &[&camera_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("oxide terrain pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_layout()],
            },
            primitive: primitive_state(),
            depth_stencil: Some(depth_state()),
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
            camera_buffer,
            camera_bind_group,
            meshes: HashMap::new(),
        }
    }

    /// Writes the camera's view-projection matrix for an aspect ratio.
    ///
    /// The matrix reaches the shader as it is; nothing is transformed on the CPU. The renderer
    /// calls this once per frame with the surface's current aspect ratio, so a resize cannot
    /// leave a stale projection behind.
    pub fn set_camera(&self, queue: &wgpu::Queue, camera: Camera, aspect: f32) {
        queue.write_buffer(
            &self.camera_buffer,
            0,
            &matrix_bytes(camera.view_projection(aspect)),
        );
    }

    /// Adds or replaces a section's mesh on the GPU.
    ///
    /// An empty mesh is treated as a removal, because a zero-sized buffer is not a buffer; the
    /// mesher already returns `None` for a section that draws nothing. The buffers are sized
    /// exactly to the mesh, so a caller uploads a section only when its mesh changed.
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
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide terrain vertices"),
            size: (mesh.vertices.len() * VERTEX_BYTES) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&vertex_buffer, 0, &vertex_bytes(&mesh.vertices));
        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide terrain indices"),
            size: (mesh.indices.len() * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&index_buffer, 0, &index_bytes(&mesh.indices));
        self.meshes.insert(
            key,
            GpuMesh {
                vertex_buffer,
                index_buffer,
                index_count: mesh.indices.len() as u32,
            },
        );
    }

    /// Removes a section's mesh, freeing its buffers.
    ///
    /// Removing a section that holds no mesh does nothing.
    pub fn remove(&mut self, key: SectionKey) {
        self.meshes.remove(&key);
    }

    /// Draws every mesh in the table, one `draw_indexed` per section.
    ///
    /// The table is a hash map, so the sections draw in no particular order; every surface is
    /// opaque and depth-tested, so the picture does not depend on it.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        for mesh in self.meshes.values() {
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.index_count, 0, 0..1);
        }
    }
}

/// The vertex buffer layout the pipeline reads, tied to [`VERTEX_BYTES`] by the unit tests.
fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: VERTEX_BYTES as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    }
}

/// The primitive state: triangles, counter-clockwise front faces, back faces culled.
///
/// A face is front-facing exactly when the camera sees its outside, because the mesher winds
/// every face counter-clockwise seen from outside; culling back faces then drops the faces
/// the camera cannot see.
fn primitive_state() -> wgpu::PrimitiveState {
    wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        front_face: wgpu::FrontFace::Ccw,
        cull_mode: Some(wgpu::Face::Back),
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

/// The colour target for one attachment in `format`: opaque, every channel written.
fn color_target(format: wgpu::TextureFormat) -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState {
        format,
        blend: None,
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
    use wgpu::{CompareFunction, Face, FrontFace, PrimitiveTopology, TextureFormat, VertexFormat};

    use super::{DEPTH_FORMAT, color_target, depth_state, primitive_state, vertex_layout};
    use crate::terrain::VERTEX_BYTES;

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
    fn the_pipeline_takes_counter_clockwise_faces_and_culls_the_back() {
        let primitive = primitive_state();
        assert_eq!(primitive.topology, PrimitiveTopology::TriangleList);
        assert_eq!(primitive.front_face, FrontFace::Ccw);
        assert_eq!(primitive.cull_mode, Some(Face::Back));
    }

    #[test]
    fn the_pipeline_writes_opaque_colours() {
        let target = color_target(TextureFormat::Rgba8Unorm).expect("a colour target");
        assert_eq!(target.format, TextureFormat::Rgba8Unorm);
        assert_eq!(target.blend, None, "terrain colours are opaque");
        assert_eq!(target.write_mask, wgpu::ColorWrites::ALL);
    }
}
