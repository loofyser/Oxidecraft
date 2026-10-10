//! The sign's front-face text: four centred lines as board-fixed quads.
//!
//! The port reads values and names from the source's in-world renderer
//! (`client/renderer/tileentity/TileEntitySignRenderer.java`), its tile state
//! (`tileentity/TileEntitySign.java`) and the board placement (:28-59); no
//! source text is copied.
//!
//! The draw is board-FIXED, never a billboard: the source's matrix chain
//! (`:75-85`) scales the font pixels by `f3 = 0.015625 * f` with
//! `f = 0.6666667` (`:26`), offsets the text `0.07 * f` off the board face,
//! and draws with depth-mask off — so the pass depth-tests (`LessEqual`,
//! like the terrain) but writes no depth. The nametag billboard must NOT be
//! reused here.
//!
//! Each line is the first 90-pixel split (`splitText(component, 90, ...)`
//! keeping element 0, `:86-111`), drawn through `getFormattedText` — so `§`
//! codes tint the runs — in the literal black `0x000000` (`int i = 0`,
//! declared once, never changed). There is NO light-dimming in the sign
//! code: the board's shading comes from the world lightmap. Rows sit at
//! `y = j * 10 - len * 5` with `len` always 4, so at -20, -10, 0 and 10 font
//! pixels; `x = -width / 2` centres each line. The editing-line wrap
//! (`"> " + s + " <"`) never shows in the world. Lines draw only for the two
//! sign blocks — standing 63, wall 68 (`Block.java`:1321,1326) — and the
//! wall special-cases metadata 2/4/5 only: 180°/90°/−90°, anything else at
//! 0° (a port pin of the source's implicit default).
//!
//! The port rules (recorded): no text for positions outside the map's
//! entries — the pass draws exactly the entries it is uploaded, nothing
//! else; there is NO all-empty clearing rule in the source (S33 always
//! carries four strings, the session map is insert-only), so an all-empty
//! entry uploads no quads simply because empty lines have no glyphs.

use glam::{Mat4, Vec3};

use oxide_assets::font::{Font, FontError};
use oxide_assets::texture::Texture;

use crate::camera::Camera;
use crate::text::{TextBuilder, string_width};

/// The renderer's base scale (`TileEntitySignRenderer.java`:26).
pub const SIGN_TEXT_F: f32 = 0.6666667;
/// The font-pixel-to-world scale: `0.015625 * f` (`:75-85`) ≈ 0.0104167
/// world units per font pixel.
pub const SIGN_TEXT_SCALE: f32 = 0.015625 * SIGN_TEXT_F;
/// The text's offset off the board face: `0.07 * f` (`:75-85`) ≈ 0.0467.
pub const SIGN_FACE_OFFSET: f32 = 0.07 * SIGN_TEXT_F;
/// The rows' pitch in font pixels: `y = j * 10 - len * 5` with `len` always
/// 4, so rows sit at -20, -10, 0 and 10 (`:86-111`).
pub const SIGN_TEXT_PITCH_PX: f32 = 10.0;
/// The per-line width cap in font pixels: `splitText(component, 90, ...)`
/// keeps element 0 only (`:86-111`). Width alone gates — no character cap.
pub const SIGN_TEXT_CAP_PX: i32 = 90;
/// The lines' colour: the literal black `0x000000` (`:86` — `int i = 0`,
/// never changed).
pub const SIGN_TEXT_BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
/// The standing sign's block id (`Block.java`:1321).
pub const SIGN_STANDING_ID: u16 = 63;
/// The wall sign's block id (`Block.java`:1326).
pub const SIGN_WALL_ID: u16 = 68;
/// The board placement's height factor: the board stands at `y + 0.75 * f`
/// (`:28-59`).
pub const SIGN_BOARD_LIFT: f32 = 0.75 * SIGN_TEXT_F;
/// The wall board's drop and set-back: `translate(0, -0.3125, -0.4375)`
/// after the facing rotation (`:28-59`).
pub const SIGN_WALL_DROP: f32 = -0.3125;
/// The wall board's set-back from the wall (see [`SIGN_WALL_DROP`]).
pub const SIGN_WALL_SETBACK: f32 = -0.4375;

/// One sign's text for the world draw: the map entry's position, the block
/// the store held there, and the four lines as sent.
#[derive(Debug, Clone, PartialEq)]
pub struct SignTextEntry {
    /// The sign's world x.
    pub x: i32,
    /// The sign's world y.
    pub y: i32,
    /// The sign's world z.
    pub z: i32,
    /// The block id at the position (63 standing, 68 wall).
    pub block_id: u16,
    /// The metadata nibble: the standing sign's rotation 0..15, the wall
    /// sign's facing.
    pub metadata: u8,
    /// The four lines, as sent.
    pub lines: [String; 4],
}

/// A world-space glyph vertex: the board-fixed position, the sheet uv and
/// the run's colour.
#[derive(Debug, Clone, PartialEq)]
pub struct SignVertex {
    /// The world-space position.
    pub position: [f32; 3],
    /// The font sheet's uv.
    pub uv: [f32; 2],
    /// The run's RGBA: black, or the `§` code's tint.
    pub colour: [f32; 4],
}

/// A standing sign's yaw in degrees: `rotate(-meta * 360 / 16)` (`:28-59`).
pub fn floor_sign_yaw_deg(metadata: u8) -> f32 {
    f32::from(metadata) * 360.0 / 16.0
}

/// A wall sign's facing yaw in degrees: meta 2 → 180°, 4 → 90°, 5 → −90° —
/// anything else draws at 0°, the source's implicit default for unlisted
/// values (`:28-59` special-cases 2/4/5 only).
pub fn wall_sign_yaw_deg(metadata: u8) -> f32 {
    match metadata {
        2 => 180.0,
        4 => 90.0,
        5 => -90.0,
        _ => 0.0,
    }
}

/// The line the world draws: the first 90-pixel split — characters kept
/// while the width test holds (`splitText(component, 90, ...)` keeping
/// element 0, `:86-111`). `§` codes ride at zero width through the shared
/// width law, so a formatted line splits on its visible glyphs.
pub fn split_sign_line(font: &Font, line: &str) -> String {
    let mut kept = String::new();
    for c in line.chars() {
        let mut candidate = kept.clone();
        candidate.push(c);
        if string_width(font, &candidate) > SIGN_TEXT_CAP_PX {
            break;
        }
        kept = candidate;
    }
    kept
}

/// One entry's world-space glyph quads: the four centred lines laid in font
/// pixels (`x = -width / 2`, `y = j * 10 - 20`), scaled by
/// [`SIGN_TEXT_SCALE`], offset [`SIGN_FACE_OFFSET`] off the face, and fixed
/// to the board by the placement matrix — the standing rotation or the wall
/// facing. Entries on any other block id upload no quads: no board stands
/// there to fix them to.
pub fn sign_text_geometry(
    font: &Font,
    sheet: (u32, u32),
    entry: &SignTextEntry,
) -> (Vec<SignVertex>, Vec<u32>) {
    let yaw_deg = match entry.block_id {
        SIGN_STANDING_ID => floor_sign_yaw_deg(entry.metadata),
        SIGN_WALL_ID => wall_sign_yaw_deg(entry.metadata),
        _ => return (Vec::new(), Vec::new()),
    };
    let mut builder = TextBuilder::new();
    for (row, line) in entry.lines.iter().enumerate() {
        let kept = split_sign_line(font, line);
        if kept.is_empty() {
            continue;
        }
        let width = string_width(font, &kept) as f32;
        builder.push(
            &kept,
            [-width / 2.0, row as f32 * SIGN_TEXT_PITCH_PX - 20.0, 0.0],
            1.0,
            SIGN_TEXT_BLACK,
            false,
        );
    }
    let (vertices, indices) = builder.geometry(font, sheet);
    if vertices.is_empty() {
        return (Vec::new(), indices);
    }
    let mut board = Mat4::from_translation(Vec3::new(
        entry.x as f32 + 0.5,
        entry.y as f32 + SIGN_BOARD_LIFT,
        entry.z as f32 + 0.5,
    )) * Mat4::from_rotation_y(-yaw_deg.to_radians());
    if entry.block_id == SIGN_WALL_ID {
        board *= Mat4::from_translation(Vec3::new(0.0, SIGN_WALL_DROP, SIGN_WALL_SETBACK));
    }
    let vertices = vertices
        .into_iter()
        .map(|vertex| {
            let local = Vec3::new(
                vertex.position[0] * SIGN_TEXT_SCALE,
                0.5 * SIGN_TEXT_F - vertex.position[1] * SIGN_TEXT_SCALE,
                SIGN_FACE_OFFSET,
            );
            let world = board.transform_point3(local);
            SignVertex {
                position: [world.x, world.y, world.z],
                uv: vertex.uv,
                colour: vertex.colour,
            }
        })
        .collect();
    (vertices, indices)
}

/// The sign-text shader: the camera's view-projection maps the board-fixed
/// positions to clip space, the sheet is sampled and multiplied by the run
/// colour, and fragments at or below the 0.1 alpha test discard — the
/// overlay's own fragment rule, so a transparent glyph texel draws nothing.
const SHADER: &str = r#"
struct Camera {
    view_projection: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var font_texture: texture_2d<f32>;
@group(1) @binding(1) var font_sampler: sampler;

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
    output.clip_position = camera.view_projection * vec4<f32>(input.position, 1.0);
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

/// The size of one sign vertex in the byte stream the GPU receives: three
/// position floats, two uv floats, four colour floats.
const SIGN_VERTEX_BYTES: usize = std::mem::size_of::<[f32; 9]>();
/// The size of the camera uniform in bytes: one `mat4x4<f32>`.
const UNIFORM_BYTES: usize = 64;

/// The sign vertex attributes: a position at offset 0, a uv at 12 and an
/// RGBA colour at 20.
static ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4];

/// The font sheet as the pass keeps it: the measured font, the sheet's size
/// and the bind group the pipeline samples through.
struct FontState {
    /// The measured widths the layout advances with.
    font: Font,
    /// The sheet's size in texels, for the quads' uvs.
    sheet_size: (u32, u32),
    /// The sheet's view and sampler, bound at group 1 for the draw.
    bind_group: wgpu::BindGroup,
}

/// The uploaded geometry, with the buffers kept across uploads.
#[derive(Default)]
struct Geometry {
    /// The vertex buffer, filled with [`sign_vertex_bytes`] output.
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
    /// Replaces the stored geometry with `vertices` and `indices`, reusing
    /// the buffers when they are already large enough.
    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vertices: &[SignVertex],
        indices: &[u32],
    ) {
        let vertex_bytes = sign_vertex_bytes(vertices);
        let index_bytes = sign_index_bytes(indices);
        if self.vertex_capacity < vertex_bytes.len() as u64 {
            self.vertex_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("oxide sign text vertices"),
                size: vertex_bytes.len() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.vertex_capacity = vertex_bytes.len() as u64;
        }
        if self.index_capacity < index_bytes.len() as u64 {
            self.index_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("oxide sign text indices"),
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

/// The sign-text pass: the world's sign entries as board-fixed quads.
pub struct SignTextPass {
    /// The pipeline: the camera uniform, the sheet sampled straight
    /// through, the depth tested but never written.
    pipeline: wgpu::RenderPipeline,
    /// The uniform buffer holding the view-projection matrix.
    camera_buffer: wgpu::Buffer,
    /// The bind group the pipeline reads the camera through.
    camera_bind_group: wgpu::BindGroup,
    /// The layout the font sheet is bound through: built once, so every
    /// sheet's bind group and the pipeline agree.
    font_layout: wgpu::BindGroupLayout,
    /// The font and its sheet, once [`SignTextPass::set_font`] has landed.
    font: Option<FontState>,
    /// The geometry of the last upload.
    geometry: Geometry,
    /// The entries the last upload carried; equal entries make the next
    /// upload a no-op.
    entries: Vec<SignTextEntry>,
}

impl SignTextPass {
    /// Builds the pipeline for colour attachments in `format`.
    ///
    /// The pipeline depth-tests (`LessEqual`, the terrain's own comparison)
    /// but writes no depth — the source's `depthMask(false)` — and culls
    /// nothing: the source draws the text with face culling off, so a quad
    /// whose winding faces away still draws. What hides text behind solid
    /// geometry is the depth test against the terrain, which draws first
    /// with depth write (`renderer.rs:scene_draws`) — an opaque cube at the
    /// sign's cell buries the text inside it. The mesher emits no such cube
    /// for 63/68 (the fallback is skipped, the barrier's `Invisible`
    /// precedent, until a board-meshing task lands), so the text floats
    /// with no board (recorded). The fragment writes the sampled sheet
    /// straight through and discards at or below the 0.1 alpha test, the
    /// overlay's own rule.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oxide sign text shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide sign text camera layout"),
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
            label: Some("oxide sign text font layout"),
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
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide sign text camera"),
            size: UNIFORM_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide sign text camera bind group"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("oxide sign text pipeline layout"),
            bind_group_layouts: &[&camera_layout, &font_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("oxide sign text pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_layout()],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(crate::terrain_pass::depth_state(false)),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        Self {
            pipeline,
            camera_buffer,
            camera_bind_group,
            font_layout,
            font: None,
            geometry: Geometry::default(),
            entries: Vec::new(),
        }
    }

    /// Writes the camera's view-projection for `aspect` into the uniform.
    pub fn set_camera(&mut self, queue: &wgpu::Queue, camera: Camera, aspect: f32) {
        queue.write_buffer(
            &self.camera_buffer,
            0,
            &matrix_bytes(camera.view_projection(aspect)),
        );
    }

    /// Uploads `sheet` and measures it as the font every following entry
    /// draws with.
    ///
    /// The widths come from the sheet through [`Font::load`], so the layout
    /// the next upload computes and the sheet the fragments sample cannot
    /// disagree. Until this is called the pass draws nothing. A sheet that
    /// is not a 16x16 grid is [`FontError`], and the pass keeps its previous
    /// font.
    pub fn set_font(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sheet: &Texture,
    ) -> Result<(), FontError> {
        let font = Font::load(sheet, None)?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("oxide sign text font sheet"),
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("oxide sign text font sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide sign text font bind group"),
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
        self.entries.clear();
        self.geometry.clear();
        Ok(())
    }

    /// Replaces the drawn entries with `entries`: the map's entries, nothing
    /// else — positions outside the map never reach this call (the port's
    /// no-text rule). Calling this again with equal entries does nothing.
    /// Nothing is drawn until [`SignTextPass::set_font`] has given the pass
    /// a sheet.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        entries: &[SignTextEntry],
    ) {
        if self.entries == entries {
            return;
        }
        self.entries.clear();
        self.entries.extend_from_slice(entries);
        let Some(font) = &self.font else {
            self.geometry.clear();
            return;
        };
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for entry in entries {
            let (mut entry_vertices, entry_indices) =
                sign_text_geometry(&font.font, font.sheet_size, entry);
            let base = vertices.len() as u32;
            indices.extend(entry_indices.iter().map(|index| base + index));
            vertices.append(&mut entry_vertices);
        }
        self.geometry.upload(device, queue, &vertices, &indices);
    }

    /// Draws the uploaded entries, or nothing when the pass has no font or
    /// no geometry.
    ///
    /// The pass runs inside the scene pass — after the terrain, so the depth
    /// test hides text behind blocks — with the scene's depth attachment
    /// loaded, never cleared here.
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
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_bind_group(1, &font.bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.geometry.index_count, 0, 0..1);
    }
}

/// The vertex buffer layout the pipeline reads, tied to
/// [`SIGN_VERTEX_BYTES`] by the tests.
fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: SIGN_VERTEX_BYTES as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    }
}

/// Packs a matrix into the 64 little-endian bytes of a WGSL `mat4x4<f32>`.
fn matrix_bytes(matrix: Mat4) -> [u8; UNIFORM_BYTES] {
    let mut bytes = [0u8; UNIFORM_BYTES];
    for (index, component) in matrix.to_cols_array().iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&component.to_le_bytes());
    }
    bytes
}

/// Packs the vertices into the byte stream the vertex buffer holds.
fn sign_vertex_bytes(vertices: &[SignVertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * SIGN_VERTEX_BYTES);
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
fn sign_index_bytes(indices: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(indices));
    for index in indices {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    //! The sign geometry's literals, without a GPU: the board-fixed quads
    //! for a floor sign at rotation 0 and one wall sign, the facing table,
    //! the black literal, the `§` passthrough and the 90-pixel split.

    use super::*;
    use oxide_assets::texture::Texture;

    /// The synthetic sheet: the `A` cell inks columns 0..=4, so `A` is six
    /// font pixels wide; `|` inks its first column only (two wide).
    fn geometry_font() -> Font {
        const SIDE: u32 = 128;
        const CELL: u32 = 8;
        let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
        for (code, columns) in [('A', 0..=4), ('|', 0..=0)] {
            let code = code as u32;
            let cell_x = (code % 16) * CELL;
            let cell_y = (code / 16) * CELL;
            for row in 0..CELL {
                for column in columns.clone() {
                    let offset = (((cell_y + row) * SIDE + cell_x + column) * 4) as usize;
                    rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
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
        .expect("the synthetic sheet is a 16x16 grid")
    }

    /// One standing sign's entry at the origin's block above, rotation 0.
    fn floor_entry(lines: [String; 4]) -> SignTextEntry {
        SignTextEntry {
            x: 0,
            y: 1,
            z: 0,
            block_id: SIGN_STANDING_ID,
            metadata: 0,
            lines,
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn the_floor_sign_at_rotation_zero_lies_flat_on_the_face() {
        let font = geometry_font();
        let (vertices, indices) = sign_text_geometry(
            &font,
            (128, 128),
            &floor_entry(["A".to_string(), String::new(), String::new(), String::new()]),
        );
        // One glyph: one quad, two triangles.
        assert_eq!(vertices.len(), 4, "one glyph is one quad");
        assert_eq!(indices.len(), 6, "two triangles");
        // The centred pen: `pen = -width / 2`, and each glyph quad is the
        // full 8-pixel cell at the pen — so the quad spans `pen..pen + 8`
        // font pixels. Rotation 0 is the identity, so the board centre
        // (0.5, 1 + 0.75f, 0.5) offsets the local quad directly.
        let f3 = SIGN_TEXT_SCALE;
        let pen = -(string_width(&font, "A") as f32) / 2.0;
        let (min_x, max_x) = vertices
            .iter()
            .fold((f32::MAX, f32::MIN), |(min, max), vertex| {
                (min.min(vertex.position[0]), max.max(vertex.position[0]))
            });
        assert!(
            close(min_x, 0.5 + pen * f3) && close(max_x, 0.5 + (pen + 8.0) * f3),
            "the quad is centred on the board: {min_x}..{max_x}"
        );
        for vertex in &vertices {
            assert!(
                close(vertex.position[2], 0.5 + SIGN_FACE_OFFSET),
                "the quad sits the face offset off the board: {}",
                vertex.position[2]
            );
        }
        // The row's top edge: `0.5 * f - (-20) * f3` above the board centre's
        // `1 + 0.75 * f`.
        let f = SIGN_TEXT_F;
        let top = 1.0 + 0.75 * f + 0.5 * f + 20.0 * f3;
        let vertex_top = vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::MIN, f32::max);
        assert!(
            close(vertex_top, top),
            "the first row rides high: {vertex_top} vs {top}"
        );
        let _ = f;
    }

    #[test]
    fn the_wall_sign_faces_its_metadata_and_defaults_unlisted_to_zero() {
        assert_eq!(wall_sign_yaw_deg(2), 180.0, "meta 2 faces south");
        assert_eq!(wall_sign_yaw_deg(4), 90.0, "meta 4 faces west");
        assert_eq!(wall_sign_yaw_deg(5), -90.0, "meta 5 faces east");
        assert_eq!(wall_sign_yaw_deg(3), 0.0, "unlisted metadata draws at 0");
        assert_eq!(wall_sign_yaw_deg(0), 0.0, "unlisted metadata draws at 0");
        assert_eq!(floor_sign_yaw_deg(0), 0.0, "rotation 0 is the identity");
        assert_eq!(
            floor_sign_yaw_deg(4),
            90.0,
            "rotation 4 is the quarter turn"
        );
        assert_eq!(floor_sign_yaw_deg(8), 180.0, "rotation 8 is the half turn");
        // Meta 2 turns the board to face +z: the half turn maps local +x to
        // world -x, so the `pen..pen + 8` quad mirrors about the board
        // centre x 0.5.
        let font = geometry_font();
        let pen = -(string_width(&font, "A") as f32) / 2.0;
        let middle = 1.0 - (2.0 * pen + 8.0) * SIGN_TEXT_SCALE;
        let entry = SignTextEntry {
            x: 0,
            y: 1,
            z: 0,
            block_id: SIGN_WALL_ID,
            metadata: 2,
            lines: ["A".to_string(), String::new(), String::new(), String::new()],
        };
        let (vertices, _) = sign_text_geometry(&font, (128, 128), &entry);
        assert_eq!(vertices.len(), 4, "one glyph is one quad");
        let (min_x, max_x) = vertices
            .iter()
            .fold((f32::MAX, f32::MIN), |(min, max), vertex| {
                (min.min(vertex.position[0]), max.max(vertex.position[0]))
            });
        assert!(
            close(min_x + max_x, middle),
            "the half turn mirrors about the board centre x 0.5: {min_x}..{max_x}"
        );
        assert!(
            vertices.iter().all(|vertex| vertex.position[2] > 0.5),
            "the faced board stands off the wall toward +z"
        );
    }

    #[test]
    fn the_text_is_black_and_format_codes_tint_their_runs() {
        let font = geometry_font();
        let (vertices, _) = sign_text_geometry(
            &font,
            (128, 128),
            &floor_entry(["A".to_string(), String::new(), String::new(), String::new()]),
        );
        assert!(
            vertices
                .iter()
                .all(|vertex| vertex.colour == SIGN_TEXT_BLACK),
            "the literal black, no dimming"
        );
        // `§c` tints its run the classic red; the shared width law measures
        // it at zero width, so the split keeps the visible glyphs.
        let (tinted, _) = sign_text_geometry(
            &font,
            (128, 128),
            &floor_entry([
                "§cA".to_string(),
                String::new(),
                String::new(),
                String::new(),
            ]),
        );
        assert_eq!(tinted.len(), 4, "the code carries no quad of its own");
        let red = crate::text::colour_code(12);
        assert!(
            tinted
                .iter()
                .all(|vertex| vertex.colour[..3] == red && vertex.colour[3] == 1.0),
            "the run wears the code's colour: {:?}",
            tinted.first().map(|vertex| vertex.colour)
        );
    }

    #[test]
    fn the_draw_splits_each_line_at_ninety_pixels() {
        let font = geometry_font();
        // `A` is six wide: fifteen fit in 90, sixteen do not.
        let long = "A".repeat(16);
        assert!(string_width(&font, &long) > SIGN_TEXT_CAP_PX);
        let kept = split_sign_line(&font, &long);
        assert_eq!(kept, "A".repeat(15), "the sixteenth glyph is cut");
        assert!(string_width(&font, &kept) <= SIGN_TEXT_CAP_PX);
        // A short line passes whole, and an empty line stays empty — an
        // all-empty entry uploads no quads at all.
        assert_eq!(split_sign_line(&font, "A"), "A");
        let (vertices, _) = sign_text_geometry(
            &font,
            (128, 128),
            &floor_entry([String::new(), String::new(), String::new(), String::new()]),
        );
        assert!(vertices.is_empty(), "no glyphs, no quads");
    }

    #[test]
    fn entries_off_the_sign_blocks_upload_no_quads() {
        let font = geometry_font();
        for block_id in [0u16, 1, 54, 69] {
            let (vertices, indices) = sign_text_geometry(
                &font,
                (128, 128),
                &SignTextEntry {
                    x: 0,
                    y: 1,
                    z: 0,
                    block_id,
                    metadata: 0,
                    lines: ["A".to_string(), String::new(), String::new(), String::new()],
                },
            );
            assert!(
                vertices.is_empty() && indices.is_empty(),
                "block {block_id} has no board to fix quads to"
            );
        }
    }

    #[test]
    fn the_vertex_layout_matches_the_serialised_vertices() {
        use wgpu::{VertexFormat, VertexStepMode};
        let layout = vertex_layout();
        assert_eq!(layout.array_stride, SIGN_VERTEX_BYTES as u64);
        assert_eq!(layout.step_mode, VertexStepMode::Vertex);
        let formats: Vec<VertexFormat> = layout.attributes.iter().map(|attr| attr.format).collect();
        assert_eq!(
            formats,
            [
                VertexFormat::Float32x3,
                VertexFormat::Float32x2,
                VertexFormat::Float32x4
            ]
        );
    }
}
