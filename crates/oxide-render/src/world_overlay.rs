//! The world overlay: the aimed block's selection outline and the destroy-stage crack.
//!
//! One pass draws both, inside the frame's scene pass after the terrain and before the GUI,
//! from the state the client feeds it: the latest aim ([`Outline`]) and the session's
//! destroy-stage map ([`Crack`]). Both are the source's own sections —
//! `EntityRenderer.drawSelectionBox`'s `"outline"` section (`EntityRenderer.java:1412-1416`)
//! and `drawBlockDamageTexture`'s `"destroyProgress"` section (`:1431-1437`) — issued in
//! that order, the outline first.
//!
//! # The outline
//!
//! The source's `RenderGlobal.drawSelectionBox` (`RenderGlobal.java:1875-1901`) draws the
//! aimed block's selection box: `GlStateManager.color(0.0F, 0.0F, 0.0F, 0.4F)` (`:1881`) and
//! `GL11.glLineWidth(2.0F)` (`:1882`) in window pixels, `GlStateManager.depthMask(false)`
//! around the draw (`:1884`, restored at `:1898`), blending `(770, 771, 1, 0)` — `GL_SRC_ALPHA`,
//! `GL_ONE_MINUS_SRC_ALPHA` (`:1880`) — and the box is the block's own selection shape
//! expanded by `0.0020000000949949026` on every axis (`:1895`). `drawSelectionBoundingBox`
//! (`:1904-1931`) walks the box's twelve edges — the bottom and top rings and the four
//! verticals — with no texture.
//!
//! wgpu has no line width: the twelve edges are drawn as screen-space quads two window pixels
//! across, expanded in clip space against the frame's viewport ([`edge_quad`]), so the drawn
//! thickness follows the source's rule — a fixed window-pixel width, whatever the edge's
//! depth — where a triangle-based line renderer would taper it with distance.
//!
//! # The crack
//!
//! The source's `RenderGlobal.drawBlockDamageTexture` (`:1819-1866`) draws every tracked
//! destroy stage through `BlockRendererDispatcher.renderBlockDamage` (`:1858`):
//! `blockModelShapes.getModelForState(state)` with the stage's icon substituted for the
//! model's texture, so the block's own model quads at its position carry the
//! `blocks/destroy_stage_{stage}` sprite (`:212` registers the ten icons). `preRenderDamagedBlocks`
//! (`:1797-1807`) sets the composite: `tryBlendFuncSeparate(774, 768, 1, 0)` (`:1799`) —
//! `(DST_COLOR, SRC_COLOR)`, a ×2 multiply with the sprite that darkens the surface —
//! `color(1, 1, 1, 0.5)` (`:1801`), the polygon offset `(-3, -3)` (`:1802`), the alpha test
//! `alphaFunc(GL_GREATER, 0.1)` (`:1804`); the depth writes are *not* disabled —
//! `postRenderDamagedBlocks` only restores the mask the block layers left on
//! (`depthMask(true)`, `:1815`). The atlas is sampled through the level-0 nearest pair
//! `EntityRenderer` sets around the draw (`setBlurMipmap(false, false)`, `:1434`, restored at
//! `:1436`). An entry is dropped once its squared distance from the view entity exceeds
//! 1024.0 — 32 blocks (`:1845`) — and the four tile-entity blocks are skipped (`:1843`): the
//! 32-block drop is mirrored in [`within_view`], but the tile-entity skip is one the stage map
//! cannot carry, because it names cells and stages and no block state (the same limit fixes
//! the crack to the fallback cube below).
//!
//! The block's own model cannot be resolved on this side of the crate graph — the session
//! publishes no block state with its stage map, and `oxide-render` may not reach the model
//! set — so the crack draws the block's full cube: the shape every full-block class reports
//! and the fallback the mesher draws for a state it cannot resolve. The quad set, its winding
//! and its sprite corners are that fallback cube's own tables (`oxide-game`'s
//! `FALLBACK_CORNERS`/`SPRITE_CORNERS`), so the crack reuses the model-data path the mesher
//! consumes corner for corner. The crack's pipeline culls back faces: the damaged pass sets
//! no cull state of its own and runs with the client's culling on (`RenderGlobal.java:1786`),
//! under the same cube winding the terrain draws with.

use glam::{Mat4, Vec2, Vec3, Vec4};

use oxide_assets::atlas::Atlas;

use crate::atlas_texture::AtlasTexture;
use crate::camera::Camera;
use crate::terrain_pass::DEPTH_FORMAT;

/// The outline's inflation per axis: `drawSelectionBox` expands the block's selection box by
/// `0.0020000000949949026` on every axis (`RenderGlobal.java:1895`; the float literal
/// `0.002F` at `:1885` is the value the double rounds from).
const OUTLINE_INFLATION: f32 = 0.002;

/// The outline's line width in window pixels: `GL11.glLineWidth(2.0F)` (`RenderGlobal.java:1882`).
const OUTLINE_WIDTH_PIXELS: f32 = 2.0;

/// The outline's colour: black at 0.4 alpha (`GlStateManager.color(0.0F, 0.0F, 0.0F, 0.4F)`,
/// `RenderGlobal.java:1881`). Written into the shader source, so the constant the tests pin
/// and the drawn fragment cannot disagree.
const OUTLINE_COLOUR: [f32; 4] = [0.0, 0.0, 0.0, 0.4];

/// The crack's fragment alpha scalar: `GlStateManager.color(1.0F, 1.0F, 1.0F, 0.5F)`
/// (`RenderGlobal.java:1801`) multiplies the sprite texel's alpha by a half before the test.
const CRACK_ALPHA: f32 = 0.5;

/// The alpha at and below which the crack discards its fragment: `alphaFunc(516, 0.1F)`
/// (`RenderGlobal.java:1804`) with `GL_GREATER` (`516`), so the fragment must exceed 0.1.
const CRACK_ALPHA_TEST: f32 = 0.1;

/// The distance beyond which the source drops a damaged-block entry: the squared distance
/// from the view entity, over 1024.0 — 32 blocks (`RenderGlobal.java:1845`).
const CRACK_CULL_DISTANCE: f32 = 32.0;

/// The size of the crack's frame uniform in bytes: one `mat4x4<f32>`.
const MATRIX_BYTES: usize = 64;

/// The size of one outline vertex in the byte stream the GPU receives: a `Float32x4`
/// clip-space position.
const OUTLINE_VERTEX_BYTES: usize = 16;

/// The full cube: the shape every full-block class's selection bounds report.
pub const FULL_CUBE: [[f32; 3]; 2] = [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]];

/// The aimed block the outline draws.
///
/// `shape` is the block's box in block-local units — the behaviour table's shape for the
/// aimed block's state, which the source's `Block.getSelectedBoundingBox` answers
/// (`RenderGlobal.java:1895`) and `drawSelectionBox` inflates. The wiring fills
/// [`FULL_CUBE`]: the session publishes no block state with the aim, so every block draws the
/// full cube's box, which is the shape every full-block class reports; a block whose table
/// shape is partial (stairs, slabs, fences, panes, walls, doors) is a recorded class for the
/// milestone that carries block state into the frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outline {
    /// The block's world cell.
    pub block: [i32; 3],
    /// The block's box in block-local units.
    pub shape: [[f32; 3]; 2],
}

/// One breaking block the crack draws: its cell and its destroy stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crack {
    /// The block's world cell.
    pub block: [i32; 3],
    /// The destroy stage, 0..=9: the `blocks/destroy_stage_{stage}` sprite textures it.
    pub stage: u8,
}

/// One crack vertex as the GPU receives it: a world-space position and an atlas uv.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CrackVertex {
    /// World-space position.
    position: [f32; 3],
    /// The atlas uv, level-0 coordinates.
    uv: [f32; 2],
}

/// The size of one crack vertex in the byte stream the GPU receives: a `Float32x3`
/// world-space position and a `Float32x2` uv.
const CRACK_VERTEX_BYTES: usize = 20;

/// The outline's vertex attributes: one `Float32x4` clip-space position.
static OUTLINE_ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x4];

/// The crack's vertex attributes: the `Float32x3` position at offset 0 and the `Float32x2` uv
/// at 12.
static CRACK_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];

/// The world-overlay pass: the pipelines, the frame's uniform, the stage lookups and the
/// frame's vertex buffers.
pub struct WorldOverlay {
    /// The outline's pipeline: clip-space quads, a constant colour, no bind groups.
    outline_pipeline: wgpu::RenderPipeline,
    /// The crack's pipeline: world-space quads sampling the atlas's level-0 pair.
    crack_pipeline: wgpu::RenderPipeline,
    /// The buffer the crack's view-projection matrix is written to each frame.
    frame_buffer: wgpu::Buffer,
    /// The bind group the crack reads its frame uniform through.
    frame_bind_group: wgpu::BindGroup,
    /// The atlas layout the crack's bind group is built under.
    atlas_layout: wgpu::BindGroupLayout,
    /// The atlas's level-0 bind group, once an atlas has been set.
    atlas_bind_group: Option<wgpu::BindGroup>,
    /// The ten stage sprites' uv rects, resolved from the atlas ([`stage_uvs`]).
    stage_uvs: [[[f32; 2]; 2]; 10],
    /// The outline the frame draws, when one is set.
    outline: Option<Outline>,
    /// The breaking blocks the crack draws.
    cracks: Vec<Crack>,
    /// The outline's vertex buffer and the vertex count the draw reads.
    outline_vertices: Option<(wgpu::Buffer, u32)>,
    /// The crack's vertex buffer and the vertex count the draw reads.
    crack_vertices: Option<(wgpu::Buffer, u32)>,
}

impl WorldOverlay {
    /// Builds both pipelines and the crack's frame uniform.
    ///
    /// The outline's pipeline has no bind groups — its vertices are already clip space and its
    /// colour is a shader constant — and draws with the depth writes off, culling nothing and
    /// blending `(770, 771, 1, 0)`. The crack's pipeline reads the frame's view-projection at
    /// group 0 and the atlas at group 1 (through [`AtlasTexture::bind_group_layout`]'s layout),
    /// culls back faces and draws with the depth writes on, the source's polygon offset and
    /// the `(774, 768, 1, 0)` blend. Both test [`DEPTH_FORMAT`] with `LessEqual`, so the pass
    /// they draw in needs a depth attachment of that format and a colour attachment in
    /// `format`. Until an atlas is set ([`WorldOverlay::set_atlas`]) the crack issues no draw;
    /// the outline needs no atlas.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oxide world overlay shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source().into()),
        });
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide world overlay frame layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(MATRIX_BYTES as u64),
                },
                count: None,
            }],
        });
        let atlas_layout = AtlasTexture::bind_group_layout(device);
        let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide world overlay frame uniform"),
            size: MATRIX_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide world overlay frame bind group"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buffer.as_entire_binding(),
            }],
        });
        let outline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("oxide outline pipeline layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });
        let crack_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("oxide crack pipeline layout"),
            bind_group_layouts: &[&frame_layout, &atlas_layout],
            push_constant_ranges: &[],
        });
        let outline_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("oxide outline pipeline"),
            layout: Some(&outline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_outline"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[outline_vertex_layout()],
            },
            primitive: primitive_state(None),
            depth_stencil: Some(depth_state(false)),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_outline"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[color_target(format, Some(outline_blend()))],
            }),
            multiview: None,
            cache: None,
        });
        let crack_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("oxide crack pipeline"),
            layout: Some(&crack_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_crack"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[crack_vertex_layout()],
            },
            primitive: primitive_state(Some(wgpu::Face::Back)),
            depth_stencil: Some(crack_depth_state()),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_crack"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[color_target(format, Some(crack_blend()))],
            }),
            multiview: None,
            cache: None,
        });
        Self {
            outline_pipeline,
            crack_pipeline,
            frame_buffer,
            frame_bind_group,
            atlas_layout,
            atlas_bind_group: None,
            // No atlas yet: every stage resolves to nothing, and the crack draws nothing
            // until `set_atlas` fills these in. A zero rect is a valid value here; the
            // absence of the bind group is what gates the draw.
            stage_uvs: [[[0.0, 0.0], [0.0, 0.0]]; 10],
            outline: None,
            cracks: Vec::new(),
            outline_vertices: None,
            crack_vertices: None,
        }
    }

    /// Uploads `atlas`'s level-0 texture state and resolves the ten stage sprites.
    ///
    /// The upload is [`AtlasTexture::upload`] — the atlas's whole mip chain — but the crack
    /// binds only the level-0 pair, because the source draws the damage pass between its
    /// `setBlurMipmap(false, false)` and `restoreLastBlurMipmap()` calls
    /// (`EntityRenderer.java:1434-1436`). The ten `blocks/destroy_stage_{stage}` uv rects are
    /// resolved once, here, through [`Atlas::drawn`], so a stage updates from the atlas itself
    /// and the draw path only looks rectangles up. Dropping the texture handle after building
    /// the bind group is safe: the bind group holds the view.
    pub fn set_atlas(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, atlas: &Atlas) {
        let texture = AtlasTexture::upload(device, queue, atlas);
        self.atlas_bind_group = Some(texture.plain_bind_group(device, &self.atlas_layout));
        self.stage_uvs = stage_uvs(atlas);
    }

    /// Sets the aimed block the outline draws; `None` stops the outline.
    pub fn set_outline(&mut self, outline: Option<Outline>) {
        self.outline = outline;
    }

    /// Sets the breaking blocks the crack draws, replacing any earlier set.
    pub fn set_cracks(&mut self, cracks: Vec<Crack>) {
        self.cracks = cracks;
    }

    /// Uploads the frame's geometry: the outline's screen-space quads and the crack's cube
    /// quads for every entry the 32-block view keeps.
    ///
    /// The renderer calls this once per frame, after the camera is set, with the surface's
    /// current viewport in pixels — the outline's pixel width needs it, and the projection's
    /// aspect comes from it, so a resize cannot leave a stale frame behind. Both vertex
    /// streams are rebuilt every frame, as the source re-tessellates the damage passes every
    /// frame; the buffers are reused while they can hold the new data.
    pub fn set_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &Camera,
        viewport: [f32; 2],
    ) {
        let view_projection = camera.view_projection(viewport[0] / viewport[1]);
        queue.write_buffer(&self.frame_buffer, 0, &matrix_bytes(view_projection));

        let outline_vertices = self
            .outline
            .as_ref()
            .map(|outline| outline_vertices(outline, view_projection, viewport))
            .unwrap_or_default();
        let outline_bytes = outline_bytes(&outline_vertices);
        upload(
            device,
            queue,
            "oxide outline vertices",
            &mut self.outline_vertices,
            &outline_bytes,
            OUTLINE_VERTEX_BYTES,
        );

        // The view entity's own position, not the eye the view is built with: the source
        // measures the drop from the interpolated entity position
        // (`RenderGlobal.java:1821-1823`), the same point `EntityRenderer` hands the camera.
        let view_position = Vec3::new(
            camera.pose.position[0] as f32,
            camera.pose.position[1] as f32,
            camera.pose.position[2] as f32,
        );
        let mut vertices = Vec::new();
        for crack in &self.cracks {
            if within_view(crack, view_position) {
                // The stage is validated where it enters the client (0..=9); a value past the
                // ten sprites is clamped rather than trusted, so a hostile stage cannot index
                // out of the lookup.
                let stage = self.stage_uvs[crack.stage.min(9) as usize];
                vertices.extend(crack_vertices(crack, stage));
            }
        }
        let crack_bytes = crack_bytes(&vertices);
        upload(
            device,
            queue,
            "oxide crack vertices",
            &mut self.crack_vertices,
            &crack_bytes,
            CRACK_VERTEX_BYTES,
        );
    }

    /// Draws the outline, then the crack, into the frame's scene pass.
    ///
    /// The order is the source's section order (`EntityRenderer.java:1412-1416` before
    /// `:1431-1437`) and both draws are gated on the state that built them: no outline
    /// vertices means no outline, and the crack draws only with an atlas bound, because a
    /// draw without one can only be wrong.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some((buffer, count)) = &self.outline_vertices {
            pass.set_pipeline(&self.outline_pipeline);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..*count, 0..1);
        }
        if let (Some((buffer, count)), Some(atlas)) = (&self.crack_vertices, &self.atlas_bind_group)
        {
            pass.set_pipeline(&self.crack_pipeline);
            pass.set_bind_group(0, &self.frame_bind_group, &[]);
            pass.set_bind_group(1, atlas, &[]);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..*count, 0..1);
        }
    }
}

/// The shader source: the outline's pass-through vertex stage with the source's colour, and
/// the crack's atlased stage with the source's alpha scalar and test.
fn shader_source() -> String {
    format!(
        r#"
// The outline's vertex stage: the vertices are clip-space quads the CPU built against the
// frame's viewport, so this stage passes them through.
@vertex
fn vs_outline(@location(0) clip: vec4<f32>) -> @builtin(position) vec4<f32> {{
    return clip;
}}

// The outline's fragment stage: the source's flat colour,
// GlStateManager.color(0.0F, 0.0F, 0.0F, 0.4F) (RenderGlobal.java:1881).
@fragment
fn fs_outline() -> @location(0) vec4<f32> {{
    return vec4<f32>({r}, {g}, {b}, {a});
}}

// The crack's vertex stage: the cube quads' block-local corners plus the block's cell,
// through the frame's view-projection — the source draws the same quads through the world
// renderer's own translation (RenderGlobal.java:1830-1831, :1858).
struct CrackOutput {{
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}}

@group(0) @binding(0) var<uniform> view_projection: mat4x4<f32>;
@group(1) @binding(0) var atlas: texture_2d<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

@vertex
fn vs_crack(@location(0) position: vec3<f32>, @location(1) uv: vec2<f32>) -> CrackOutput {{
    var output: CrackOutput;
    output.clip = view_projection * vec4<f32>(position, 1.0);
    output.uv = uv;
    return output;
}}

// The crack's fragment stage: the stage sprite's texel with the source's alpha scalar
// (GlStateManager.color(1.0F, 1.0F, 1.0F, 0.5F), RenderGlobal.java:1801) and its alpha test
// (alphaFunc(GL_GREATER, 0.1F), :1804). The pipeline's blend does the source's multiply.
@fragment
fn fs_crack(input: CrackOutput) -> @location(0) vec4<f32> {{
    let texel = textureSample(atlas, atlas_sampler, input.uv);
    let alpha = texel.a * {crack_alpha};
    if (alpha <= {crack_test}) {{
        discard;
    }}
    return vec4<f32>(texel.rgb, alpha);
}}
"#,
        r = OUTLINE_COLOUR[0],
        g = OUTLINE_COLOUR[1],
        b = OUTLINE_COLOUR[2],
        a = OUTLINE_COLOUR[3],
        crack_alpha = CRACK_ALPHA,
        crack_test = CRACK_ALPHA_TEST,
    )
}

/// The outline's blend: `tryBlendFuncSeparate(770, 771, 1, 0)` — `GL_SRC_ALPHA` over
/// `GL_ONE_MINUS_SRC_ALPHA` with the destination's alpha left alone
/// (`RenderGlobal.java:1880`).
fn outline_blend() -> wgpu::BlendState {
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

/// The crack's blend: `tryBlendFuncSeparate(774, 768, 1, 0)` — a ×2 multiply,
/// `src × dst + dst × src`, with the fragment's alpha written as it is
/// (`RenderGlobal.java:1799`).
fn crack_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Dst,
            dst_factor: wgpu::BlendFactor::Src,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::Zero,
            operation: wgpu::BlendOperation::Add,
        },
    }
}

/// The crack's polygon offset: `doPolygonOffset(-3.0F, -3.0F)` (`RenderGlobal.java:1802`) —
/// the constant is the source's `units` term (multiples of the smallest resolvable depth
/// difference) and the slope scale its `factor`; both negative, so the crack's fragments come
/// out nearer and pass the `LessEqual` test over the block's own coincident faces.
fn crack_depth_bias() -> wgpu::DepthBiasState {
    wgpu::DepthBiasState {
        constant: -3,
        slope_scale: -3.0,
        clamp: 0.0,
    }
}

/// The depth state both overlays' pipelines declare: the terrain's format and comparison, and
/// the layer's write flag.
///
/// The comparison is `LessEqual` over the 0..1 range the projection maps the near and far
/// planes to — the client's `GlStateManager.depthFunc(515)` (`Minecraft.java:540`) — so the
/// overlay wins against an equal depth, which is what the outline's coincident edges and the
/// crack's offset fragments need.
fn depth_state(write: bool) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: write,
        depth_compare: wgpu::CompareFunction::LessEqual,
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

/// The crack's depth state: the source's polygon offset with the depth writes left on
/// (`RenderGlobal.java:1802`, and `postRenderDamagedBlocks` restoring `depthMask(true)` at
/// `:1815` rather than disabling the writes).
fn crack_depth_state() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        bias: crack_depth_bias(),
        ..depth_state(true)
    }
}

/// The primitive state for a cull mode: triangles, counter-clockwise front faces.
fn primitive_state(cull: Option<wgpu::Face>) -> wgpu::PrimitiveState {
    wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        front_face: wgpu::FrontFace::Ccw,
        cull_mode: cull,
        ..Default::default()
    }
}

/// The colour target for one attachment in `format`, blended when the pass asks for it.
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

/// The outline's vertex layout: one `Float32x4` position and the stride
/// [`OUTLINE_VERTEX_BYTES`].
fn outline_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: OUTLINE_VERTEX_BYTES as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &OUTLINE_ATTRIBUTES,
    }
}

/// The crack's vertex layout: the position at offset 0 and the uv at 12, the stride
/// [`CRACK_VERTEX_BYTES`].
fn crack_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: CRACK_VERTEX_BYTES as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &CRACK_ATTRIBUTES,
    }
}

/// The world-space box the outline draws: the block's cell offset by its shape box, then
/// inflated by [`OUTLINE_INFLATION`] on every axis.
fn outline_box(outline: &Outline) -> (Vec3, Vec3) {
    let base = Vec3::new(
        outline.block[0] as f32,
        outline.block[1] as f32,
        outline.block[2] as f32,
    );
    let min = base + Vec3::from(outline.shape[0]) - Vec3::splat(OUTLINE_INFLATION);
    let max = base + Vec3::from(outline.shape[1]) + Vec3::splat(OUTLINE_INFLATION);
    (min, max)
}

/// The twelve edges of a box, as pairs of corners.
///
/// The source walks the same twelve edges as the bottom and top rings and the four verticals
/// (`RenderGlobal.java:1907-1931`); here they are the corner pairs that differ in one axis, so
/// the pair list cannot drift from the box.
fn box_edges(min: Vec3, max: Vec3) -> [(Vec3, Vec3); 12] {
    let corner = |index: usize| {
        Vec3::new(
            if index & 1 == 0 { min.x } else { max.x },
            if index & 2 == 0 { min.y } else { max.y },
            if index & 4 == 0 { min.z } else { max.z },
        )
    };
    let mut edges = [(Vec3::ZERO, Vec3::ZERO); 12];
    let mut next = 0;
    for index in 0..8usize {
        for bit in [1usize, 2, 4] {
            let other = index | bit;
            if other != index {
                edges[next] = (corner(index), corner(other));
                next += 1;
            }
        }
    }
    edges
}

/// One edge's screen-space quad: the two endpoints offset ±half the line's width in window
/// pixels, perpendicular to the edge's screen-space direction.
///
/// The offsets are computed in NDC — a pixel is `2 / viewport` there — so the drawn thickness
/// is [`OUTLINE_WIDTH_PIXELS`] window pixels at every depth, which is the source's
/// `glLineWidth(2.0)` rule (`RenderGlobal.java:1882`). `None` for an edge that is behind the
/// camera or projects to a single point, which has no direction to expand along; that keeps a
/// hostile or degenerate box from producing non-finite geometry.
fn edge_quad(a: Vec4, b: Vec4, viewport: [f32; 2], width_pixels: f32) -> Option<[Vec4; 4]> {
    if a.w <= 0.0 || b.w <= 0.0 || viewport[0] <= 0.0 || viewport[1] <= 0.0 {
        return None;
    }
    let a_ndc = Vec2::new(a.x / a.w, a.y / a.w);
    let b_ndc = Vec2::new(b.x / b.w, b.y / b.w);
    let delta = Vec2::new(
        (b_ndc.x - a_ndc.x) * viewport[0] / 2.0,
        (b_ndc.y - a_ndc.y) * viewport[1] / 2.0,
    );
    let length = delta.length();
    if !length.is_finite() || length <= f32::EPSILON {
        return None;
    }
    let perpendicular = Vec2::new(-delta.y, delta.x) / length * (width_pixels / 2.0);
    let offset = Vec2::new(
        perpendicular.x * 2.0 / viewport[0],
        perpendicular.y * 2.0 / viewport[1],
    );
    let widen = |clip: Vec4, ndc: Vec2, sign: f32| {
        Vec4::new(
            (ndc.x + sign * offset.x) * clip.w,
            (ndc.y + sign * offset.y) * clip.w,
            clip.z,
            clip.w,
        )
    };
    Some([
        widen(a, a_ndc, 1.0),
        widen(a, a_ndc, -1.0),
        widen(b, b_ndc, -1.0),
        widen(b, b_ndc, 1.0),
    ])
}

/// The outline's clip-space vertices for a frame: six vertices — two triangles — per visible
/// edge.
fn outline_vertices(outline: &Outline, view_projection: Mat4, viewport: [f32; 2]) -> Vec<[f32; 4]> {
    let (min, max) = outline_box(outline);
    let mut vertices = Vec::with_capacity(12 * 6);
    for (a, b) in box_edges(min, max) {
        let a = view_projection * a.extend(1.0);
        let b = view_projection * b.extend(1.0);
        let Some(quad) = edge_quad(a, b, viewport, OUTLINE_WIDTH_PIXELS) else {
            continue;
        };
        for corner in [quad[0], quad[1], quad[2], quad[0], quad[2], quad[3]] {
            vertices.push(corner.to_array());
        }
    }
    vertices
}

/// Packs clip-space outline vertices into the byte stream the vertex buffer holds.
fn outline_bytes(vertices: &[[f32; 4]]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * OUTLINE_VERTEX_BYTES);
    for vertex in vertices {
        for component in vertex {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    bytes
}

/// The full cube's corners per face, counter-clockwise seen from outside, in the order the
/// source's `EnumFaceDirection` gives the mesher's fallback cube walks it — top, bottom,
/// north, south, west, east — so the sprite corners pair with them corner for corner.
///
/// The values repeat `oxide-game`'s `FALLBACK_CORNERS` (`crates/oxide-game/src/mesher.rs`),
/// the model-data path the mesher draws for a state it cannot resolve; the crack draws the
/// same cube with the stage sprite in place of the model's texture.
const CUBE_CORNERS: [[[f32; 3]; 4]; 6] = [
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

/// The sprite coordinates of a full-face quad's four corners: the sprite's top-left,
/// bottom-left, bottom-right and top-right — the order `BlockFaceUV` walks a face with no
/// rotation, and the mesher's own `SPRITE_CORNERS`.
const SPRITE_CORNERS: [[f32; 2]; 4] = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];

/// The ten destroy-stage sprites' uv rects, resolved through [`Atlas::drawn`].
///
/// `blocks/destroy_stage_{stage}` is the path the source registers its ten icons under
/// (`"minecraft:blocks/destroy_stage_" + i`, `RenderGlobal.java:212`); [`Atlas::drawn`] falls
/// back to the missing sprite for a path the atlas never stitched, so a lookup cannot panic
/// on a stage without a sprite.
fn stage_uvs(atlas: &Atlas) -> [[[f32; 2]; 2]; 10] {
    std::array::from_fn(|stage| {
        let path = format!("blocks/destroy_stage_{stage}");
        atlas.uv(atlas.drawn(&path))
    })
}

/// Whether the crack's 32-block view keeps `crack`.
///
/// The source drops an entry whose squared distance from the view entity — the block's corner
/// against the entity's interpolated position — exceeds 1024.0
/// (`RenderGlobal.java:1838-1845`).
fn within_view(crack: &Crack, eye: Vec3) -> bool {
    let dx = crack.block[0] as f32 - eye.x;
    let dy = crack.block[1] as f32 - eye.y;
    let dz = crack.block[2] as f32 - eye.z;
    dx * dx + dy * dy + dz * dz <= CRACK_CULL_DISTANCE * CRACK_CULL_DISTANCE
}

/// The crack's vertices for one entry: the block's full cube, each face's four corners as two
/// triangles, every uv mapped into the stage sprite's rect.
///
/// The position and uv arithmetic is the mesher's fallback quad's, corner for corner
/// (`crates/oxide-game/src/mesher.rs`: `position + corner`,
/// `rect[0] + uv * (rect[1] - rect[0])`), so the crack's quads and a model's would map the
/// sprite identically.
fn crack_vertices(crack: &Crack, stage: [[f32; 2]; 2]) -> Vec<CrackVertex> {
    let base = Vec3::new(
        crack.block[0] as f32,
        crack.block[1] as f32,
        crack.block[2] as f32,
    );
    let mut vertices = Vec::with_capacity(6 * 6);
    for face in CUBE_CORNERS {
        for index in [0usize, 1, 2, 0, 2, 3] {
            let corner = Vec3::from(face[index]);
            let sprite = SPRITE_CORNERS[index];
            let uv = [
                stage[0][0] + sprite[0] * (stage[1][0] - stage[0][0]),
                stage[0][1] + sprite[1] * (stage[1][1] - stage[0][1]),
            ];
            vertices.push(CrackVertex {
                position: (base + corner).to_array(),
                uv,
            });
        }
    }
    vertices
}

/// Packs crack vertices into the byte stream the vertex buffer holds.
fn crack_bytes(vertices: &[CrackVertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * CRACK_VERTEX_BYTES);
    for vertex in vertices {
        for component in vertex.position {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
        for component in vertex.uv {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    bytes
}

/// Packs a matrix into the little-endian bytes of the crack's frame uniform.
///
/// WGSL lays a uniform matrix out as four columns of four `f32`, which is the order
/// [`Mat4::to_cols_array`] returns, so the components go out as they are.
fn matrix_bytes(matrix: Mat4) -> [u8; MATRIX_BYTES] {
    let mut bytes = [0u8; MATRIX_BYTES];
    for (index, component) in matrix.to_cols_array().iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&component.to_le_bytes());
    }
    bytes
}

/// Uploads one overlay's frame vertices into its slot, recreating the buffer when the new
/// data does not fit, and records the vertex count the draw reads.
///
/// The buffers are sized exactly to the frame that needed them, so a frame that shrinks —
/// fewer cracks, an outline that went away — reuses the buffer while it can hold the data and
/// a growing one replaces it; an empty frame clears the slot, because a zero-sized buffer is
/// not a buffer.
fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    slot: &mut Option<(wgpu::Buffer, u32)>,
    bytes: &[u8],
    vertex_bytes: usize,
) {
    if bytes.is_empty() || vertex_bytes == 0 {
        *slot = None;
        return;
    }
    let needed = bytes.len() as u64;
    let fits = slot
        .as_ref()
        .is_some_and(|(buffer, _)| buffer.size() >= needed);
    if !fits {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: needed,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        *slot = Some((buffer, 0));
    }
    let (buffer, count) = slot.as_mut().expect("the slot was just filled");
    queue.write_buffer(buffer, 0, bytes);
    *count = (bytes.len() / vertex_bytes) as u32;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use glam::{Vec2, Vec3, Vec4};
    use oxide_assets::atlas::{Atlas, AtlasLevel, AtlasSprite, SpriteRect};
    use wgpu::{BlendFactor, BlendOperation, CompareFunction, PrimitiveTopology, TextureFormat};

    use super::{
        CRACK_ALPHA, CRACK_ALPHA_TEST, CRACK_CULL_DISTANCE, Crack, FULL_CUBE, OUTLINE_COLOUR,
        OUTLINE_INFLATION, OUTLINE_WIDTH_PIXELS, Outline, box_edges, crack_blend, crack_bytes,
        crack_depth_bias, crack_depth_state, crack_vertices, depth_state, edge_quad, outline_blend,
        outline_box, outline_vertices, primitive_state, stage_uvs, within_view,
    };
    use crate::camera::{Camera, CameraPose, DEFAULT_FOV, EYE_HEIGHT, NEAR_PLANE, NO_VIEW_EFFECT};
    use crate::terrain_pass::DEPTH_FORMAT;

    /// The camera the geometry tests project with: the eye at the origin looking down -Z,
    /// the vanilla field of view, an aspect of one.
    fn frame_camera() -> Camera {
        Camera {
            pose: CameraPose {
                position: [0.0, -f64::from(EYE_HEIGHT), 0.0],
                yaw: 180.0,
                pitch: 0.0,
            },
            fov_degrees: DEFAULT_FOV,
            near: NEAR_PLANE,
            far_chunks: 1.0,
            view_effect: NO_VIEW_EFFECT,
        }
    }

    /// A hand-built atlas whose ten stage sprites sit on consecutive 16-texel cells, for the
    /// stage lookup's own test; no asset store is read and no Mojang pixel is embedded. The
    /// eleventh cell is the fallback sprite, so a lookup that resolves the fallback instead
    /// of a stage's own sprite differs.
    fn stage_atlas() -> Atlas {
        const CELLS: u32 = 11;
        let mut rgba = vec![0u8; (CELLS * 16 * 16 * 4) as usize];
        for (index, texel) in rgba.chunks_exact_mut(4).enumerate() {
            texel[0] = (index % 251) as u8;
            texel[3] = 255;
        }
        let sprite = |cell: u32| AtlasSprite {
            region: SpriteRect {
                x: cell * 16,
                y: 0,
                w: 16,
                h: 16,
            },
            content: SpriteRect {
                x: cell * 16,
                y: 0,
                w: 16,
                h: 16,
            },
        };
        let mut sprites = BTreeMap::new();
        for stage in 0..10u32 {
            sprites.insert(format!("blocks/destroy_stage_{stage}"), sprite(stage));
        }
        Atlas {
            levels: vec![AtlasLevel {
                width: CELLS * 16,
                height: 16,
                rgba,
            }],
            width: CELLS * 16,
            height: 16,
            level_count: 1,
            sprites,
            animated: BTreeMap::new(),
            missing: sprite(10),
        }
    }

    #[test]
    fn the_outline_box_inflates_the_aimed_shape_by_the_sources_0002() {
        // `drawSelectionBox` expands `Block.getSelectedBoundingBox`'s box by 0.002 on every
        // axis (`RenderGlobal.java:1895`), over the block's own position: the box's two
        // corners move one inflation outwards on each axis.
        let outline = Outline {
            block: [2, -1, 5],
            shape: [[0.0, 0.0, 0.0], [0.5, 1.0, 1.0]],
        };
        let (min, max) = outline_box(&outline);
        let want_min = Vec3::new(2.0 - 0.002, -1.0 - 0.002, 5.0 - 0.002);
        let want_max = Vec3::new(2.5 + 0.002, 0.0 + 0.002, 6.0 + 0.002);
        assert!((min - want_min).length() < 1e-5, "min {min:?}");
        assert!((max - want_max).length() < 1e-5, "max {max:?}");
        assert_eq!(OUTLINE_INFLATION, 0.002);
    }

    #[test]
    fn a_box_has_twelve_edges_and_each_corner_meets_three() {
        let (min, max) = (
            Vec3::new(-0.002, -0.002, -0.002),
            Vec3::new(1.002, 1.002, 1.002),
        );
        let edges = box_edges(min, max);
        assert_eq!(edges.len(), 12, "a box's edges are twelve");
        // Every edge is axis-aligned: two of its three axes agree, and every corner ends
        // three edges.
        let mut corners: BTreeMap<[i32; 3], u32> = BTreeMap::new();
        for (a, b) in edges {
            let shared = (0..3).filter(|&axis| a[axis] == b[axis]).count();
            assert_eq!(shared, 2, "an edge runs along one axis: {a:?}..{b:?}");
            assert!((a - b).length() > 0.0, "an edge has a length");
            for corner in [a, b] {
                let key = [
                    (corner.x * 1000.0).round() as i32,
                    (corner.y * 1000.0).round() as i32,
                    (corner.z * 1000.0).round() as i32,
                ];
                *corners.entry(key).or_default() += 1;
            }
        }
        assert_eq!(corners.len(), 8, "a box has eight corners");
        assert!(
            corners.values().all(|&meetings| meetings == 3),
            "every corner is the end of three edges: {corners:?}"
        );
    }

    #[test]
    fn the_outline_edges_are_two_window_pixels_wide_at_two_depths() {
        // The source's `glLineWidth(2.0)` (`RenderGlobal.java:1882`) is a window-pixel width:
        // the projected thickness must be two pixels whatever the edge's depth. The
        // expansion is expressed in NDC against the viewport, so a screen-horizontal edge is
        // `2 * 2 / height` of NDC tall and a screen-vertical one `2 * 2 / width` wide.
        let view_projection = frame_camera().view_projection(1.0);
        let viewport = [800.0f32, 600.0];
        let ndc = |clip: Vec4| clip.truncate() / clip.w;
        for depth in [2.0f32, 20.0] {
            // A screen-vertical edge at the frame's centre.
            let a = view_projection * Vec4::new(0.0, 0.0, -depth, 1.0);
            let b = view_projection * Vec4::new(0.0, 1.0, -depth, 1.0);
            let quad = edge_quad(a, b, viewport, OUTLINE_WIDTH_PIXELS).expect("a visible edge");
            let width_ndc = (ndc(quad[1]) - ndc(quad[0])).length();
            assert!(
                (width_ndc - 4.0 / viewport[0]).abs() < 1e-5,
                "depth {depth}: the vertical edge is 4/width of NDC wide, got {width_ndc}"
            );
            let pixels = Vec2::new(
                (ndc(quad[1]).x - ndc(quad[0]).x) * viewport[0] / 2.0,
                (ndc(quad[1]).y - ndc(quad[0]).y) * viewport[1] / 2.0,
            )
            .length();
            assert!(
                (pixels - 2.0).abs() < 1e-4,
                "depth {depth}: the edge is two window pixels wide, got {pixels}"
            );
            // A screen-horizontal edge, measured against the viewport height.
            let a = view_projection * Vec4::new(-0.5, 0.0, -depth, 1.0);
            let b = view_projection * Vec4::new(0.5, 0.0, -depth, 1.0);
            let quad = edge_quad(a, b, viewport, OUTLINE_WIDTH_PIXELS).expect("a visible edge");
            let height_ndc = (ndc(quad[1]).y - ndc(quad[0]).y).abs();
            assert!(
                (height_ndc - 4.0 / viewport[1]).abs() < 1e-5,
                "depth {depth}: the horizontal edge is 4/height of NDC tall, got {height_ndc}"
            );
            let pixels = height_ndc * viewport[1] / 2.0;
            assert!(
                (pixels - 2.0).abs() < 1e-4,
                "depth {depth}: the horizontal edge is two window pixels wide, got {pixels}"
            );
        }
    }

    #[test]
    fn an_edge_behind_the_camera_or_without_direction_has_no_quad() {
        let view_projection = frame_camera().view_projection(1.0);
        let viewport = [800.0f32, 600.0];
        // Behind the camera: w is negative, so there is no NDC point to expand around.
        let a = view_projection * Vec4::new(0.0, 0.0, 2.0, 1.0);
        let b = view_projection * Vec4::new(0.0, 1.0, 2.0, 1.0);
        assert!(edge_quad(a, b, viewport, 2.0).is_none());
        // A zero-length edge projects to one point and has no direction.
        let a = view_projection * Vec4::new(0.0, 0.0, -2.0, 1.0);
        assert!(edge_quad(a, a, viewport, 2.0).is_none());
    }

    #[test]
    fn a_visible_box_draws_all_twelve_edges_as_quads() {
        // The box sits in front of the camera (the frame camera looks down -Z from the
        // origin), so every edge has both endpoints in front and all twelve project.
        let outline = Outline {
            block: [0, 0, -10],
            shape: FULL_CUBE,
        };
        let vertices = outline_vertices(
            &outline,
            frame_camera().view_projection(1.0),
            [800.0, 600.0],
        );
        assert_eq!(vertices.len(), 12 * 6, "twelve edges, two triangles each");
        // Every emitted vertex carries its edge's own clip depth, not zero: the quads are
        // built from the projected corners.
        assert!(
            vertices.iter().all(|vertex| vertex[3] > 0.0),
            "every vertex is in front of the camera"
        );
    }

    #[test]
    fn the_stage_lookup_resolves_each_destroy_sprite_path() {
        // `Atlas::drawn` resolves `blocks/destroy_stage_{stage}` to the stitched sprite
        // (`crates/oxide-assets/src/atlas.rs`); the ten rects the crack draws with come from
        // its uv.
        let atlas = stage_atlas();
        let uvs = stage_uvs(&atlas);
        for (stage, uv) in uvs.iter().enumerate() {
            let path = format!("blocks/destroy_stage_{stage}");
            let sprite = atlas.drawn(&path);
            assert_eq!(*uv, atlas.uv(sprite), "stage {stage}");
            assert_ne!(
                *uv,
                atlas.uv(&atlas.missing),
                "stage {stage} resolves its own sprite, not the fallback"
            );
        }
        // A stage the atlas never stitched falls back to the missing sprite, never panics.
        let mut sparse = stage_atlas();
        sparse.sprites.remove("blocks/destroy_stage_7");
        let uvs = stage_uvs(&sparse);
        assert_eq!(uvs[7], sparse.uv(&sparse.missing));
    }

    #[test]
    fn the_crack_quad_set_is_a_full_cube_with_the_sprite_on_every_face() {
        let crack = Crack {
            block: [3, 64, -2],
            stage: 5,
        };
        // A non-trivial sprite rect, so a swapped corner or a dropped mapping fails.
        let stage = [[0.25, 0.5], [0.5, 0.75]];
        let vertices = crack_vertices(&crack, stage);
        assert_eq!(vertices.len(), 36, "six faces as two triangles each");
        // Every corner is a corner of the block's cell.
        for vertex in &vertices {
            for axis in 0..3 {
                let low = [3.0, 64.0, -2.0][axis];
                let value = vertex.position[axis];
                assert!(
                    value >= low - 1e-6 && value <= low + 1.0 + 1e-6,
                    "vertex {vertex:?} inside the cell"
                );
            }
        }
        // The first face is the top one, its corners counter-clockwise seen from outside
        // (the mesher's `FALLBACK_CORNERS` order), with the sprite's own corner order. The
        // quad is emitted as the triangles (0, 1, 2) and (0, 2, 3).
        assert_eq!(vertices[0].position, [3.0, 65.0, -2.0]);
        assert_eq!(vertices[1].position, [3.0, 65.0, -1.0]);
        assert_eq!(vertices[2].position, [4.0, 65.0, -1.0]);
        assert_eq!(vertices[3].position, [3.0, 65.0, -2.0]);
        assert_eq!(vertices[5].position, [4.0, 65.0, -2.0]);
        assert_eq!(vertices[0].uv, [0.25, 0.5], "the sprite's top-left");
        assert_eq!(vertices[1].uv, [0.25, 0.75], "the sprite's bottom-left");
        assert_eq!(vertices[2].uv, [0.5, 0.75], "the sprite's bottom-right");
        assert_eq!(vertices[5].uv, [0.5, 0.5], "the sprite's top-right");
        // Every face winds outwards: the cross of its first two edges points away from the
        // block's centre, which is what the back-face cull reads. The face's four corners are
        // the first triangle's three plus the second triangle's last.
        let centre = Vec3::new(3.5, 64.5, -1.5);
        for face in vertices.chunks_exact(6) {
            let corner = |index: usize| Vec3::from(face[[0usize, 1, 2, 5][index]].position);
            let normal = (corner(1) - corner(0)).cross(corner(2) - corner(1));
            let middle = (corner(0) + corner(1) + corner(2) + corner(3)) / 4.0;
            assert!(
                normal.dot(middle - centre) > 0.0,
                "face {face:?} winds counter-clockwise seen from outside"
            );
        }
        // The packed stream is 20 bytes a vertex, position then uv.
        let bytes = crack_bytes(&vertices);
        assert_eq!(bytes.len(), 36 * 20);
        assert_eq!(&bytes[0..4], &3.0f32.to_le_bytes());
        assert_eq!(&bytes[12..16], &0.25f32.to_le_bytes());
    }

    #[test]
    fn the_crack_composite_is_the_sources_multiply() {
        // `preRenderDamagedBlocks` (`RenderGlobal.java:1797-1807`): blend
        // `(774, 768, 1, 0)` = `(DST_COLOR, SRC_COLOR)` — src×dst + dst×src — with the
        // fragment's alpha written as it is, the colour alpha at 0.5, the alpha test at
        // 0.1 and the polygon offset at (-3, -3); the depth mask stays on.
        let blend = crack_blend();
        assert_eq!(blend.color.src_factor, BlendFactor::Dst);
        assert_eq!(blend.color.dst_factor, BlendFactor::Src);
        assert_eq!(blend.color.operation, BlendOperation::Add);
        assert_eq!(blend.alpha.src_factor, BlendFactor::One);
        assert_eq!(blend.alpha.dst_factor, BlendFactor::Zero);
        assert_eq!(CRACK_ALPHA, 0.5);
        assert_eq!(CRACK_ALPHA_TEST, 0.1);
        assert_eq!(crack_depth_bias().constant, -3);
        assert_eq!(crack_depth_bias().slope_scale, -3.0);
        assert_eq!(crack_depth_bias().clamp, 0.0);
        let depth = crack_depth_state();
        assert_eq!(depth.format, DEPTH_FORMAT);
        assert_eq!(depth.depth_compare, CompareFunction::LessEqual);
        assert!(
            depth.depth_write_enabled,
            "the damaged pass does not disable the depth writes"
        );
        assert_eq!(depth.bias, crack_depth_bias());
    }

    #[test]
    fn the_outline_draw_state_is_the_sources() {
        // `drawSelectionBox` (`RenderGlobal.java:1879-1884`): blend `(770, 771, 1, 0)`,
        // colour black at 0.4 alpha, a two-pixel line and `depthMask(false)`; the lines are
        // not culled.
        let blend = outline_blend();
        assert_eq!(blend.color.src_factor, BlendFactor::SrcAlpha);
        assert_eq!(blend.color.dst_factor, BlendFactor::OneMinusSrcAlpha);
        assert_eq!(blend.color.operation, BlendOperation::Add);
        assert_eq!(blend.alpha.src_factor, BlendFactor::One);
        assert_eq!(blend.alpha.dst_factor, BlendFactor::Zero);
        assert_eq!(OUTLINE_COLOUR, [0.0, 0.0, 0.0, 0.4]);
        assert_eq!(OUTLINE_WIDTH_PIXELS, 2.0);
        let depth = depth_state(false);
        assert_eq!(depth.format, DEPTH_FORMAT);
        assert_eq!(depth.depth_compare, CompareFunction::LessEqual);
        assert!(
            !depth.depth_write_enabled,
            "the outline draws with the depth mask off"
        );
        let primitive = primitive_state(None);
        assert_eq!(primitive.topology, PrimitiveTopology::TriangleList);
        assert_eq!(primitive.cull_mode, None, "the outline culls nothing");
        assert_eq!(TextureFormat::Depth32Float, DEPTH_FORMAT);
    }

    #[test]
    fn the_crack_drops_entries_beyond_thirty_two_blocks_of_the_eye() {
        // The source removes an entry whose squared distance from the view entity is over
        // 1024.0 (`RenderGlobal.java:1845`): 32 blocks exactly is kept, anything past is not.
        let eye = Vec3::new(0.5, 64.0, -1.5);
        assert!(within_view(
            &Crack {
                block: [32, 64, -1],
                stage: 0
            },
            eye
        ));
        assert!(!within_view(
            &Crack {
                block: [33, 64, -1],
                stage: 0
            },
            eye
        ));
        assert!(within_view(
            &Crack {
                block: [0, 64, 0],
                stage: 9
            },
            eye
        ));
        assert!(!within_view(
            &Crack {
                block: [0, 96, 0],
                stage: 9
            },
            eye
        ));
        assert_eq!(CRACK_CULL_DISTANCE, 32.0);
    }
}
