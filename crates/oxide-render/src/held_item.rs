//! The first-person held item: the source's `ItemRenderer` chain, its own state
//! machines and the camera-space pass that draws the selected stack.
//!
//! One frame's matrix composes exactly as the source's chain does, in call order
//! (first-called leftmost, the point seeing rightmost first):
//! `doItemUsedTransformations` (`ItemRenderer.java`:261-267), then
//! `transformFirstPersonItem` (`:296-307`), then the wrapper's 3D scale
//! (`ItemRenderer.renderItem`:67) or the generated class's `preTransform` scale
//! (`RenderItem.java`:254-257, run at `:320`), then the display transform
//! (`RenderItem.java`:327), then the render path's tail — `scale(0.5)` (`:145`) and
//! `translate(-0.5,-0.5,-0.5)` (`:157`) — over the 1/16-unit mesh:
//!
//! ```text
//! M = doItemUsed(s) . TFPI(equip, s) . S(2) . display(firstperson) . S(0.5) . T(-0.5) . S(1/16)
//! ```
//!
//! The carried derivation notes' "CORRECTION 1" reversed the `display` and
//! `S(0.5)·T(-0.5)` segment; the source order above says the original chain was
//! right — `preTransform` runs at `:320`, `applyTransform` at `:327` and
//! `renderItem` at `:334`, and GL applies the last-called transform first — and the
//! port's own GUI chain (`gui_item::icon_matrix`:312-342) composes the same way
//! (`display_matrix` outside `S(0.5)·T(-0.5)`), pinned by its own literal tests.
//!
//! The hand's lighting is the source's own: `rotateArroundXAndY` (`:99-106`) turns
//! the standard item lights by the player's pitch and yaw — no geometry — and
//! `enableStandardItemLighting` (`RenderHelper.java`:28-48) lights every face with
//! `min(0.4 + 0.6·Σ max(N·L, 0), 1)`, folded per vertex on the CPU the way the GUI
//! draws fold it (`gui_item::lit_colour`), then multiplied by the frame's
//! brightness (the lightmap's own factor; the port's client has no own-player light
//! channel, so it fills the full-bright equivalent — recorded).

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::{Mat3, Mat4, Vec3};
use oxide_assets::model::Transform;

use crate::camera::Camera;
use crate::entity_pass::ItemMesh;
use crate::gui_item::{ATLAS_TEXTURE, ItemIcon, ItemIconMesh, ItemIconSource, display_matrix};
use crate::terrain_pass::depth_state;
use crate::text::TextVertex;

/// The first-person translate's z (`ItemRenderer.java`:298): the source's own
/// `-0.71999997F` literal, kept as stated.
const FIRST_PERSON_TRANSLATE: [f32; 3] = [0.56, -0.52, -0.719_999_97];

/// The first-person item's 45-degree y turn (`ItemRenderer.java`:300).
const FIRST_PERSON_TURN: f32 = 45.0;

/// The first-person item's own scale (`ItemRenderer.java`:306).
const FIRST_PERSON_SCALE: f32 = 0.4;

/// The swing's own y turn (`ItemRenderer.java`:303's `f * -20.0F`).
const SWING_TURN_Y: f32 = -20.0;

/// The swing's z turn (`ItemRenderer.java`:304's `f1 * -20.0F`).
const SWING_TURN_Z: f32 = -20.0;

/// The swing's x turn (`ItemRenderer.java`:305's `f1 * -80.0F`).
const SWING_TURN_X: f32 = -80.0;

/// The equip drop (`ItemRenderer.java`:299's `equipProgress * -0.6F`).
const EQUIP_DROP: f32 = -0.6;

/// `doItemUsedTransformations`' x amplitude (`ItemRenderer.java`:263's `-0.4F`).
const ITEM_USED_X: f32 = -0.4;

/// `doItemUsedTransformations`' y amplitude (`:264`'s `0.2F`).
const ITEM_USED_Y: f32 = 0.2;

/// `doItemUsedTransformations`' z amplitude (`:265`'s `-0.2F`).
const ITEM_USED_Z: f32 = -0.2;

/// The class scale both branches apply exactly once: the 3D class at
/// `ItemRenderer.renderItem`:67, the generated class at `RenderItem.preTransform`
/// (`:256`, run at `:320`).
const CLASS_SCALE: f32 = 2.0;

/// The render path's own scale (`RenderItem.renderItem`:145).
const RENDER_SCALE: f32 = 0.5;

/// The render path's centring translate (`RenderItem.renderItem`:157).
const RENDER_CENTRE: f32 = -0.5;

/// The model units: a mesh's coordinates are sixteenths of a block
/// (`FaceBakery`'s 0..16 element space divided by 16).
const MESH_SCALE: f32 = 1.0 / 16.0;

/// The swing's length in ticks: `getArmSwingAnimationEnd` (`EntityLivingBase.java`
/// :1334-1337) without the dig-speed potions the port has no effect state for —
/// six (recorded).
pub const ARM_SWING_END: i32 = 6;

/// The equip ease's per-tick step cap (`ItemRenderer.updateEquippedItem`:603's
/// `0.4F`).
const EQUIP_STEP: f32 = 0.4;

/// The progress below which `itemToRender` swaps
/// (`ItemRenderer.updateEquippedItem`:609).
const SWAP_THRESHOLD: f32 = 0.1;

/// The light model's ambient term (`RenderHelper.enableStandardItemLighting`:35-47).
const LIGHT_AMBIENT: f32 = 0.4;

/// Each of the two lights' diffuse term (`RenderHelper.enableStandardItemLighting`
/// :36, :39, :43).
const LIGHT_DIFFUSE: f32 = 0.6;

/// The two standard item lights' raw positions (`RenderHelper.java`:12-13).
const LIGHT_POSITIONS: [[f32; 3]; 2] = [[0.2, 1.0, -0.7], [-0.2, 1.0, 0.7]];

/// The alpha test's threshold: `alphaFunc(516, 0.1F)` (`RenderItem.java`:322,
/// `GL_GREATER`), a fragment at or below it draws nothing — the shader's own
/// literal below.
#[cfg(test)]
const ALPHA_TEST: f32 = 0.1;

/// The own player's swing counters, the source's `EntityLivingBase` fields:
/// `isSwingInProgress` (:66), `swingProgressInt` (:67), `swingProgress` (:86) and
/// `prevSwingProgress` (:85).
///
/// The client owns them: the source's own player is an `EntityPlayerSP`, whose
/// swing is set locally (`swingItem`) and never read off the wire — only other
/// entities' swings arrive as animation packets (`:1351`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Swing {
    /// Whether a swing is in progress (`isSwingInProgress`).
    swinging: bool,
    /// The swing's tick counter (`swingProgressInt`); `-1` on a fresh swing.
    progress_int: i32,
    /// The current swing progress, `0..1` (`swingProgress`).
    progress: f32,
    /// The progress at the previous tick (`prevSwingProgress`).
    prev: f32,
}

impl Default for Swing {
    fn default() -> Self {
        Self::new()
    }
}

impl Swing {
    /// The resting state: no swing, both progress values zero.
    pub fn new() -> Self {
        Self {
            swinging: false,
            progress_int: 0,
            progress: 0.0,
            prev: 0.0,
        }
    }

    /// Starts a swing, the source's own restart rule (`swingItem`:1342-1354): a
    /// fresh swing lands unless one is already past its halfway point, which is
    /// left to finish.
    pub fn swing(&mut self) {
        if !self.swinging || self.progress_int >= ARM_SWING_END / 2 || self.progress_int < 0 {
            self.progress_int = -1;
            self.swinging = true;
        }
    }

    /// One tick: `onEntityUpdate`'s latch (`:266`) then `updateArmSwingProgress`
    /// (`:1402-1422`).
    pub fn tick(&mut self) {
        self.prev = self.progress;
        if self.swinging {
            self.progress_int += 1;
            if self.progress_int >= ARM_SWING_END {
                self.progress_int = 0;
                self.swinging = false;
            }
        } else {
            self.progress_int = 0;
        }
        self.progress = self.progress_int as f32 / ARM_SWING_END as f32;
    }

    /// The rendered swing argument, `getSwingProgress` (`:2188-2198`): the pair
    /// interpolated at `partial`, the wrapped difference so the pair's own
    /// one-to-zero reset slides through one rather than back.
    pub fn render(&self, partial: f32) -> f32 {
        let mut step = self.progress - self.prev;
        if step < 0.0 {
            step += 1.0;
        }
        self.prev + step * partial
    }

    /// The current tick's progress (`swingProgress`), `0..1`.
    pub fn progress(&self) -> f32 {
        self.progress
    }

    /// The previous tick's progress (`prevSwingProgress`), `0..1`.
    pub fn prev(&self) -> f32 {
        self.prev
    }
}

/// The equip ease's counters, the source's `ItemRenderer` fields `equippedProgress`
/// (:43) and `prevEquippedProgress` (:42).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Equip {
    /// The ease's current value (`equippedProgress`): 1 while the item is up.
    progress: f32,
    /// The value at the previous tick (`prevEquippedProgress`).
    prev: f32,
}

impl Default for Equip {
    fn default() -> Self {
        Self::new()
    }
}

impl Equip {
    /// The resting state: both values zero — a fresh `ItemRenderer`'s own
    /// (`resetEquippedProgress`:617-620).
    pub fn new() -> Self {
        Self {
            progress: 0.0,
            prev: 0.0,
        }
    }

    /// One tick of `updateEquippedItem` (`:581-611`): the pair's latch, then the
    /// clamped step — `differ` is the source's own `flag`, whether the slot's stack
    /// differs from `itemToRender` — so the ease runs to 0 while they differ and to
    /// 1 while they match.
    pub fn tick(&mut self, differ: bool) {
        self.prev = self.progress;
        let target = if differ { 0.0 } else { 1.0 };
        self.progress += (target - self.progress).clamp(-EQUIP_STEP, EQUIP_STEP);
    }

    /// The rendered equip argument f (`ItemRenderer.renderItemInFirstPerson`:357):
    /// `1 - (prev + (cur - prev) * partial)`, the drop the item's own translate
    /// consumes.
    pub fn render(&self, partial: f32) -> f32 {
        1.0 - (self.prev + (self.progress - self.prev) * partial)
    }

    /// Whether the stack swap lands this tick: `equippedProgress < 0.1`
    /// (`updateEquippedItem`:609-613).
    pub fn swap_lands(&self) -> bool {
        self.progress < SWAP_THRESHOLD
    }

    /// The ease's current value.
    pub fn progress(&self) -> f32 {
        self.progress
    }

    /// The ease's previous value.
    pub fn prev(&self) -> f32 {
        self.prev
    }
}

/// `doItemUsedTransformations` (`ItemRenderer.java`:261-267): the swing's own
/// translate, applied before the first-person item chain.
pub fn do_item_used(swing: f32) -> Mat4 {
    let root = swing.sqrt();
    let f = ITEM_USED_X * (root * std::f32::consts::PI).sin();
    let f1 = ITEM_USED_Y * (root * std::f32::consts::PI * 2.0).sin();
    let f2 = ITEM_USED_Z * (swing * std::f32::consts::PI).sin();
    Mat4::from_translation(Vec3::new(f, f1, f2))
}

/// `transformFirstPersonItem` (`ItemRenderer.java`:296-307): the hand's own place,
/// the equip drop, the 45-degree turn and the swing's three turns, then the
/// item's own scale.
pub fn transform_first_person(equip: f32, swing: f32) -> Mat4 {
    let f = (swing * swing * std::f32::consts::PI).sin();
    let f1 = (swing.sqrt() * std::f32::consts::PI).sin();
    let mut matrix = Mat4::from_translation(Vec3::from_array(FIRST_PERSON_TRANSLATE));
    matrix *= Mat4::from_translation(Vec3::new(0.0, equip * EQUIP_DROP, 0.0));
    matrix *= Mat4::from_rotation_y(FIRST_PERSON_TURN.to_radians());
    matrix *= Mat4::from_rotation_y((f * SWING_TURN_Y).to_radians());
    matrix *= Mat4::from_rotation_z((f1 * SWING_TURN_Z).to_radians());
    matrix *= Mat4::from_rotation_x((f1 * SWING_TURN_X).to_radians());
    matrix *= Mat4::from_scale(Vec3::splat(FIRST_PERSON_SCALE));
    matrix
}

/// The full first-person item matrix for a display transform, at `equip` and
/// `swing`:
///
/// ```text
/// doItemUsed(s) . TFPI(equip, s) . S(2) . display(firstperson) . S(0.5) . T(-0.5) . S(1/16)
/// ```
///
/// exactly one `S(2)` applies per class — the 3D class at `ItemRenderer`:67, the
/// generated class at `RenderItem`:256 — so one constant covers both.
pub fn held_matrix(equip: f32, swing: f32, transform: Transform) -> Mat4 {
    let mut matrix = do_item_used(swing);
    matrix *= transform_first_person(equip, swing);
    matrix *= Mat4::from_scale(Vec3::splat(CLASS_SCALE));
    matrix *= display_matrix(transform);
    matrix *= Mat4::from_scale(Vec3::splat(RENDER_SCALE));
    matrix *= Mat4::from_translation(Vec3::splat(RENDER_CENTRE));
    matrix *= Mat4::from_scale(Vec3::splat(MESH_SCALE));
    matrix
}

/// The two standard item lights in the hand's own eye space: the raw positions
/// (`RenderHelper.java`:12-13), normalised and turned by `rotateArroundXAndY`'s
/// pitch-about-x then yaw-about-y (`ItemRenderer.java`:99-106`) — the turn reaches
/// the lights only, never the geometry.
pub fn hand_lights(pitch: f32, yaw: f32) -> [Vec3; 2] {
    let turn = Mat3::from_rotation_x(pitch.to_radians()) * Mat3::from_rotation_y(yaw.to_radians());
    LIGHT_POSITIONS.map(|position| (turn * Vec3::from_array(position).normalize()).normalize())
}

/// The colour one vertex carries: the source's flat lighting for the vertex's
/// normal — `ambient + diffuse * (max(N·L0, 0) + max(N·L1, 0))` over the hand's
/// own lights (`RenderHelper.enableStandardItemLighting`:35-47), clamped to the
/// unit range the fixed-function pipeline clamps a lit fragment to — times the
/// frame's brightness (the lightmap's factor).
fn lit_colour(
    normal: [f32; 3],
    normal_matrix: Mat3,
    lights: &[Vec3; 2],
    brightness: f32,
) -> [f32; 4] {
    let normal = (normal_matrix * Vec3::from_array(normal)).normalize_or_zero();
    let mut shade = LIGHT_AMBIENT;
    for light in lights {
        shade += LIGHT_DIFFUSE * normal.dot(*light).max(0.0);
    }
    let shade = shade.clamp(0.0, 1.0) * brightness;
    [shade, shade, shade, 1.0]
}

/// The matrix a mesh's normals transform by: the draw matrix's own inverse
/// transpose (the fixed-function normal matrix), with a degenerate matrix falling
/// back to its linear part (`gui_item::normal_matrix`'s rule).
fn normal_matrix(matrix: Mat4) -> Mat3 {
    let linear = Mat3::from_mat4(matrix);
    let determinant = linear.determinant();
    if determinant.is_finite() && determinant.abs() > f32::EPSILON {
        linear.inverse().transpose()
    } else {
        linear
    }
}

/// One frame's held-item state: the stack the ease's swap rule shows, the rendered
/// ease and swing arguments, and the draw's own gates.
///
/// The client fills it each frame from the session's window and tick events: the
/// rendered values are resolved at the view seam (the source's own
/// `f = 1 - lerp(prev, cur, partial)` at `:357` and `getSwingProgress(pt)` at
/// `:359`), while `equip_prev`/`swing_prev` carry the source's raw previous-tick
/// latches (`prevEquippedProgress` `:42`, `prevSwingProgress` `:85`) for the record.
/// `brightness` is the lightmap's factor — the source enables the lightmap before
/// the hand (`EntityRenderer.java`:865) — and `sleeping` is the source's own skip
/// (`:861-863`), which the port has no state for yet: the pass honours it and the
/// client always fills false (recorded).
#[derive(Debug, Clone, PartialEq)]
pub struct HeldItemFrame {
    /// The stack `itemToRender` holds, already through the swap rule.
    pub stack: Option<ItemIcon>,
    /// The rendered equip argument f, `0..1`: 0 while the item is up.
    pub equip: f32,
    /// The raw `prevEquippedProgress`.
    pub equip_prev: f32,
    /// The rendered swing argument, `0..1`.
    pub swing: f32,
    /// The raw `prevSwingProgress`.
    pub swing_prev: f32,
    /// The lightmap's brightness factor, `0..1`: 1 is the full-bright equivalent.
    pub brightness: f32,
    /// Whether the player sleeps, the source's own skip.
    pub sleeping: bool,
}

/// The held-item pass: the frame's one item, transformed on the CPU and drawn in
/// camera space with the hand's own projection.
///
/// The pass draws into its own render pass — the colour attachment loaded, the
/// depth attachment cleared — between the scene pass and the overlay pass, which is
/// the source's own order: the hand draws inside the world pass after the terrain
/// and before the overlays (`EntityRenderer.java`:864-876), and
/// `setupOverlayRendering` clears depth before the GUI (`:1752`).
pub struct HeldItemPass {
    /// The pipeline: blended, alpha-tested, depth-tested and depth-writing.
    pipeline: wgpu::RenderPipeline,
    /// The projection-and-view uniform the vertex stage reads.
    uniform: wgpu::Buffer,
    /// The bind group the pipeline reads the uniform through.
    uniform_bind: wgpu::BindGroup,
    /// The texture-and-sampler layout every item texture binds under.
    texture_layout: wgpu::BindGroupLayout,
    /// The blocks atlas's icon binding: the plain sampler the source installs for
    /// the item draw (`RenderItem.java`:319's `setBlurMipmap(false, false)`).
    atlas_icon: Option<wgpu::BindGroup>,
    /// The named sheets (the folded chest trio's own textures).
    textures: BTreeMap<&'static str, wgpu::BindGroup>,
    /// The icon source the frame's stack resolves through.
    icon_source: Option<Arc<dyn ItemIconSource>>,
    /// The frame's draw inputs, stored until the camera rebuilds the vertices.
    frame: Option<HeldItemFrame>,
    /// The composed projection and view effect, when a camera has been set.
    view_projection: Option<Mat4>,
    /// The hand's lights for the stored pose.
    lights: [Vec3; 2],
    /// The frame's vertex buffer and its capacity in vertices.
    vertices: Option<(wgpu::Buffer, usize)>,
    /// What the frame draws: the vertex count and the texture key.
    drawable: Option<(u32, &'static str)>,
}

impl HeldItemPass {
    /// Builds the pass for colour attachments in `format`.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oxide held item shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide held item uniform layout"),
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
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide held item uniform"),
            size: UNIFORM_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide held item uniform bind group"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let texture_layout = crate::entity_pass::texture_bind_group_layout(device);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("oxide held item pipeline layout"),
            bind_group_layouts: &[&uniform_layout, &texture_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("oxide held item pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_layout()],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                // The source culls back faces during the world pass; the port's
                // item quads are single-sided and the chain can flip their
                // winding, so culling stays off, the port's other item
                // pipelines' own choice (recorded).
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(depth_state(true)),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(src_alpha_blend()),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        Self {
            pipeline,
            uniform,
            uniform_bind,
            texture_layout,
            atlas_icon: None,
            textures: BTreeMap::new(),
            icon_source: None,
            frame: None,
            view_projection: None,
            lights: hand_lights(0.0, 0.0),
            vertices: None,
            drawable: None,
        }
    }

    /// Uploads the blocks atlas the item meshes sample, bound under the source's
    /// item sampler (`RenderItem.java`:319).
    pub fn set_atlas_icon(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &oxide_assets::atlas::Atlas,
    ) {
        let texture = crate::atlas_texture::AtlasTexture::upload(device, queue, atlas);
        self.atlas_icon = Some(texture_bind(
            device,
            &self.texture_layout,
            texture.view(),
            texture.plain_sampler(),
            "oxide held item atlas bind group",
        ));
    }

    /// Uploads one named sheet an item mesh may name (the folded chest trio).
    pub fn set_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        name: &'static str,
        sheet: &oxide_assets::texture::Texture,
    ) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("oxide held item sheet"),
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
            label: Some("oxide held item sheet sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        self.textures.insert(
            name,
            texture_bind(
                device,
                &self.texture_layout,
                &view,
                &sampler,
                "oxide held item sheet bind group",
            ),
        );
    }

    /// Sets the icon source the frame's stack resolves through.
    pub fn set_icon_source(&mut self, source: Arc<dyn ItemIconSource>) {
        self.icon_source = Some(source);
    }

    /// Sets the camera the pass projects through and lights with.
    ///
    /// The projection is the source's own for the hand (`EntityRenderer.renderHand`
    /// :844): the camera's fov and near plane, the far plane doubled —
    /// `farPlaneDistance * 2` — and the modelview the view effect alone
    /// (`:831-856`'s loadIdentity: no look rotation). The lights turn by the pose's
    /// pitch and yaw (`ItemRenderer.java`:99-106). A stored frame's vertices are
    /// rebuilt, so the frame's colours read the fresh pose.
    pub fn set_camera(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &Camera,
        aspect: f32,
    ) {
        let far = camera.far_chunks * 16.0 * 2.0;
        let projection =
            Mat4::perspective_rh(camera.fov_degrees.to_radians(), aspect, camera.near, far);
        self.view_projection = Some(projection * camera.view_effect);
        self.lights = hand_lights(camera.pose.pitch, camera.pose.yaw);
        queue.write_buffer(
            &self.uniform,
            0,
            &matrix_bytes(self.view_projection.unwrap_or(Mat4::IDENTITY)),
        );
        self.rebuild(device, queue);
    }

    /// Sets the frame the next draws show.
    ///
    /// The frame's stack resolves through the icon source — an unresolved id falls
    /// back to the missing icon — and its mesh is transformed and lit on the CPU.
    /// A sleeping frame and a frame with no stack draw nothing.
    pub fn set_frame(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &HeldItemFrame) {
        self.frame = Some(frame.clone());
        self.rebuild(device, queue);
    }

    /// The frame's vertices: the mesh through [`held_matrix`], each corner's colour
    /// the source's flat lighting times the frame's brightness.
    fn frame_vertices(&self, frame: &HeldItemFrame, icon: &ItemIconMesh) -> Vec<TextVertex> {
        let matrix = held_matrix(frame.equip, frame.swing, icon.first_person);
        let normals = normal_matrix(matrix);
        let mesh: &ItemMesh = &icon.mesh;
        let mut out = Vec::with_capacity(mesh.vertices.positions.len() / 4 * 6);
        let quads = mesh.vertices.positions.len() & !3;
        for quad in (0..quads).step_by(4) {
            for index in [quad, quad + 1, quad + 2, quad, quad + 2, quad + 3] {
                let position = matrix.transform_point3(Vec3::from(mesh.vertices.positions[index]));
                out.push(TextVertex {
                    position: position.into(),
                    uv: mesh.vertices.uvs[index],
                    colour: lit_colour(
                        mesh.vertices.normals[index],
                        normals,
                        &self.lights,
                        frame.brightness,
                    ),
                });
            }
        }
        out
    }

    /// Rebuilds the stored frame's vertex buffer and the draw it issues.
    fn rebuild(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.drawable = None;
        let Some(frame) = self.frame.as_ref() else {
            return;
        };
        if frame.sleeping {
            return;
        }
        let Some(stack) = frame.stack.as_ref() else {
            return;
        };
        let Some(source) = self.icon_source.as_ref() else {
            return;
        };
        let Some(icon) = source
            .icon(stack.id, stack.damage)
            .or_else(|| source.missing_icon())
        else {
            return;
        };
        let vertices = self.frame_vertices(frame, &icon);
        if vertices.is_empty() {
            return;
        }
        let bytes = vertex_bytes(&vertices);
        let needed = bytes.len();
        let buffer = match &self.vertices {
            Some((buffer, capacity)) if *capacity >= needed => buffer.clone(),
            _ => {
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("oxide held item vertices"),
                    size: needed as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.vertices = Some((buffer.clone(), needed));
                buffer
            }
        };
        queue.write_buffer(&buffer, 0, &bytes);
        self.drawable = Some((vertices.len() as u32, icon.mesh.texture));
    }

    /// Draws the frame's item, when one resolves and its texture is bound.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some((count, texture)) = self.drawable else {
            return;
        };
        let Some((buffer, _)) = self.vertices.as_ref() else {
            return;
        };
        let bind = if texture == ATLAS_TEXTURE {
            self.atlas_icon.as_ref()
        } else {
            self.textures.get(texture)
        };
        let Some(bind) = bind else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.uniform_bind, &[]);
        pass.set_bind_group(1, bind, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..count, 0..1);
    }
}

/// The held item shader: the composed projection and view effect maps the
/// camera-space vertex, the texel is multiplied by the vertex colour, and the
/// source's alpha test (`RenderItem.java`:322) discards what it would.
const SHADER: &str = r#"
struct Held {
    view_projection: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> held: Held;
@group(1) @binding(0) var held_texture: texture_2d<f32>;
@group(1) @binding(1) var held_sampler: sampler;

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
    output.clip_position = held.view_projection * vec4<f32>(input.position, 1.0);
    output.uv = input.uv;
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(held_texture, held_sampler, input.uv) * input.color;
    if (texel.a <= 0.1) {
        discard;
    }
    return texel;
}
"#;

/// The size of the uniform in bytes: one `mat4x4<f32>`.
const UNIFORM_BYTES: usize = 64;

/// The size of one vertex in the byte stream the GPU receives.
const VERTEX_BYTES: usize = std::mem::size_of::<TextVertex>();

/// The vertex attributes: a position at offset 0, a uv at 12 and an RGBA colour at
/// 20, [`TextVertex`]'s own layout.
static ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4];

/// The vertex buffer layout the pipeline reads, tied to [`VERTEX_BYTES`] by the
/// tests.
fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: VERTEX_BYTES as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    }
}

/// The client's own blend pair: `src_alpha` over `one_minus_src_alpha`
/// (`RenderItem.java`:324's `tryBlendFuncSeparate(770, 771, 1, 0)`).
fn src_alpha_blend() -> wgpu::BlendState {
    wgpu::BlendState {
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
    }
}

/// Builds one texture bind group: the view with `sampler` under the crate's shared
/// texture layout.
fn texture_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

/// The uniform as the little-endian bytes the GPU copies.
fn matrix_bytes(matrix: Mat4) -> [u8; UNIFORM_BYTES] {
    let mut bytes = [0u8; UNIFORM_BYTES];
    for (slot, value) in bytes.chunks_exact_mut(4).zip(matrix.to_cols_array()) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// The vertex stream's bytes, in the layout [`vertex_layout`] declares.
fn vertex_bytes(vertices: &[TextVertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * VERTEX_BYTES);
    for vertex in vertices {
        for value in vertex.position {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in vertex.uv {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in vertex.colour {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    //! The transform math and the state machines, pinned against the source's own
    //! constants.

    use super::*;

    /// The sword's first-person display transform, exactly as its model JSON states
    /// it (`models/item/diamond_sword.json`'s `display.firstperson`).
    fn sword() -> Transform {
        Transform {
            rotation: [0.0, -135.0, 25.0],
            translation: [0.0, 4.0, 2.0],
            scale: [1.7, 1.7, 1.7],
        }
    }

    /// The swing curve: `getSwingProgress` at zero, mid-swing and one, with the
    /// pair's wrap sliding forward rather than back (`EntityLivingBase`:2188-2198).
    #[test]
    fn the_swing_curve_interpolates_the_counter_pair() {
        let mut swing = Swing::new();
        assert_eq!(swing.render(0.5), 0.0, "at rest the curve is zero");
        swing.swing();
        swing.tick();
        swing.tick();
        // Two ticks in: the counter is at one of six, and the pair interpolates.
        assert_eq!(swing.progress(), 1.0 / 6.0);
        assert_eq!(swing.prev(), 0.0);
        assert!((swing.render(0.0) - 0.0).abs() < 1e-6, "the pair's start");
        assert!(
            (swing.render(0.5) - 1.0 / 12.0).abs() < 1e-6,
            "the pair's midpoint: {}",
            swing.render(0.5)
        );
        assert!(
            (swing.render(1.0) - 1.0 / 6.0).abs() < 1e-6,
            "the pair's end"
        );
        // The counter runs to its end and stops: five of six is its last value, and
        // the next tick resets it.
        for _ in 0..4 {
            swing.tick();
        }
        assert!((swing.progress() - 5.0 / 6.0).abs() < 1e-6);
        swing.tick();
        assert_eq!(swing.progress(), 0.0, "the swing is over");
        // The wrap's own render: the last pair (five of six to zero) slides forward
        // through one rather than back — the sixth tick lands on five of six, and the
        // seventh resets to zero, so the wrapped pair renders one at the seam.
        let mut wrap = Swing::new();
        wrap.swing();
        for _ in 0..6 {
            wrap.tick();
        }
        assert!((wrap.progress() - 5.0 / 6.0).abs() < 1e-6);
        assert!((wrap.prev() - 4.0 / 6.0).abs() < 1e-6);
        assert!(
            (wrap.render(0.5) - 4.5 / 6.0).abs() < 1e-6,
            "the last pair's midpoint: {}",
            wrap.render(0.5)
        );
        wrap.tick();
        assert_eq!(wrap.progress(), 0.0, "the swing is over");
        assert!((wrap.prev() - 5.0 / 6.0).abs() < 1e-6);
        assert!(
            (wrap.render(0.5) - 11.0 / 12.0).abs() < 1e-6,
            "the wrapped pair's midpoint: {}",
            wrap.render(0.5)
        );
    }

    /// The restart rule (`swingItem`:1342-1354): a swing is restarted only when it is
    /// past its halfway point, not while it is short of it.
    #[test]
    fn a_swing_restarts_only_past_its_halfway_point() {
        let mut swing = Swing::new();
        swing.swing();
        swing.tick();
        swing.tick();
        assert_eq!(swing.progress(), 1.0 / 6.0);
        swing.swing();
        // One of six is short of the halfway point (three of six): the call is
        // ignored and the swing runs on.
        swing.tick();
        assert_eq!(
            swing.progress(),
            2.0 / 6.0,
            "short of halfway, the swing runs on"
        );
        swing.tick();
        assert_eq!(swing.progress(), 3.0 / 6.0);
        swing.swing();
        // Three of six is the halfway point: the call restarts the counter, and the
        // next tick lands on zero.
        swing.tick();
        assert_eq!(
            swing.progress(),
            0.0,
            "past halfway, a fresh swing restarts"
        );
    }

    /// The ease's step (`updateEquippedItem`:581-611): start, midpoint and settle,
    /// the clamped delta in both directions and the swap's own threshold.
    #[test]
    fn the_ease_steps_by_the_clamped_delta_and_settles() {
        let mut equip = Equip::new();
        assert_eq!(equip.progress(), 0.0);
        assert!(
            equip.swap_lands(),
            "a fresh ease is below the swap threshold"
        );
        // Start: matching stacks raise the ease by the full step.
        equip.tick(false);
        assert!((equip.progress() - 0.4).abs() < 1e-6);
        assert!(!equip.swap_lands());
        // Midpoint: the step is clamped to what remains.
        equip.tick(false);
        equip.tick(false);
        assert!((equip.progress() - 1.0).abs() < 1e-6);
        // Settle: a matching pair holds at one, and the render is zero.
        equip.tick(false);
        assert_eq!(equip.progress(), 1.0);
        assert_eq!(equip.render(0.5), 0.0);
        // Differing stacks run it down: full step, then the clamped remainder.
        equip.tick(true);
        assert!((equip.progress() - 0.6).abs() < 1e-6);
        equip.tick(true);
        assert!((equip.progress() - 0.2).abs() < 1e-6);
        equip.tick(true);
        assert_eq!(equip.progress(), 0.0);
        assert!(equip.swap_lands());
    }

    /// The rendered ease argument (`ItemRenderer.renderItemInFirstPerson`:357):
    /// `f = 1 - (prev + (cur - prev) * partial)`, pinned at three pairs.
    #[test]
    fn the_ease_renders_the_sources_interpolated_argument() {
        let mut equip = Equip::new();
        equip.tick(false);
        assert!(
            (equip.render(0.5) - 0.8).abs() < 1e-6,
            "prev 0 to cur 0.4 at half: {}",
            equip.render(0.5)
        );
        assert!((equip.render(0.0) - 1.0).abs() < 1e-6, "the previous latch");
        assert!((equip.render(1.0) - 0.6).abs() < 1e-6, "the current value");
        equip.tick(false);
        equip.tick(false);
        equip.tick(true);
        // prev 1.0 to cur 0.6 at a quarter.
        assert!(
            (equip.render(0.25) - 0.1).abs() < 1e-6,
            "prev 1.0 to cur 0.6 at a quarter: {}",
            equip.render(0.25)
        );
    }

    /// The sword's full first-person chain by literal: the source's constants
    /// through `doItemUsedTransformations`, `transformFirstPersonItem`, the class
    /// scale, the display transform and the render path's tail, at rest.
    ///
    /// The matrix is derived in `refs/m5-task-12/derive.py` (its `derive.log` is
    /// the receipt): the linear part is `R_y(45) · R_y(-135) · R_z(25)` times
    /// `0.4·2·1.7·0.5/16 = 0.0425`, which sends the mesh's own normal `(0, 0, 1)`
    /// to `(-1, 0, 0)` — the quad's own plane, seen from the lower right.
    #[test]
    fn the_sword_first_person_chain_composes_to_the_pinned_matrix() {
        let matrix = held_matrix(0.0, 0.0, sword());
        let expected: [f32; 16] = [
            0.0,
            0.017_961_3,
            0.038_518_1,
            0.0,
            0.0,
            0.038_518_1,
            -0.017_961_3,
            0.0,
            -0.042_5,
            0.0,
            0.0,
            0.0,
            0.970_710_7,
            -0.771_834_9,
            -0.813_743_7,
            1.0,
        ];
        let got = matrix.to_cols_array();
        for (index, (got, want)) in got.iter().zip(&expected).enumerate() {
            assert!(
                (got - want).abs() < 1e-5,
                "the composed matrix's element {index}: {got} != {want}"
            );
        }
        // The chain's own order: the display transform sits outside the render
        // path's S(0.5)·T(-0.5) — the port's GUI chain composes the same way
        // (`gui_item::icon_matrix`:328-333). A reversed pair would move the
        // translation by the difference, which this literal pins.
        assert!(
            (matrix.w_axis.z - (-0.813_743_6)).abs() < 1e-5,
            "the display sits outside the render tail: {}",
            matrix.w_axis.z
        );
    }

    /// The swing's own translate (`doItemUsedTransformations`:261-267) and the
    /// first-person item's own turns (`transformFirstPersonItem`:296-307), pinned
    /// at the swing's peak.
    #[test]
    fn the_swing_terms_follow_the_sources_own_constants() {
        // At the peak (s = 1) the doItemUsed translate is its full x term:
        // -0.4 * sin(sqrt(1) * pi) = 0, and the y term 0.2 * sin(2 * pi) = 0.
        let peak = do_item_used(1.0);
        assert!(peak.w_axis.x.abs() < 1e-6, "the peak's x term: {peak:?}");
        assert!(peak.w_axis.y.abs() < 1e-6, "the peak's y term");
        // At s = 1/4: -0.4 * sin(pi/2) = -0.4 and 0.2 * sin(pi) = 0.
        let quarter = do_item_used(0.25);
        assert!(
            (quarter.w_axis.x - (-0.4)).abs() < 1e-6,
            "the quarter's x term: {quarter:?}"
        );
        assert!(quarter.w_axis.y.abs() < 1e-6, "the quarter's y term");
        // The first-person item at rest: the translate, the 45-degree turn and the
        // 0.4 scale, in that order — the mesh's origin lands at the translate.
        let rest = transform_first_person(0.0, 0.0);
        let origin = rest.transform_point3(Vec3::ZERO);
        assert!(
            (origin - Vec3::new(0.56, -0.52, -0.719_999_97)).length() < 1e-5,
            "the item's origin: {origin:?}"
        );
        // The equip drop is the translate's own y, outside the 45-degree turn and
        // the scale (`:299` is called before `:300` and `:306`).
        let dropped = transform_first_person(1.0, 0.0);
        let drop = dropped.transform_point3(Vec3::ZERO) - origin;
        assert!(
            (drop - Vec3::new(0.0, -0.6, 0.0)).length() < 1e-5,
            "the drop is the outer translate: {drop:?}"
        );
        // The swing's own rotations at a quarter of the curve, where both sine
        // factors are full: `f = sin(s^2 * pi)` turns the y axis and
        // `f1 = sin(sqrt(s) * pi)` the z and x ones (`:301-305`). The peak's
        // angles are all zero, so only an interior sample pins them.
        let swung = held_matrix(0.0, 0.25, sword());
        let expected: [f32; 16] = [
            -0.020_088_47,
            0.037_451_26,
            0.000_326_88,
            0.0,
            -0.018_891_06,
            -0.009_811_91,
            -0.036_784_57,
            0.0,
            -0.032_339_32,
            -0.017_532_25,
            0.021_284_71,
            0.0,
            0.646_826_3,
            -0.475_679_93,
            -0.905_328_1,
            1.0,
        ];
        for (index, (value, want)) in swung.to_cols_array().iter().zip(expected).enumerate() {
            assert!(
                (value - want).abs() < 1e-6,
                "the swing sample's element {index}: {value} != {want}"
            );
        }
    }

    /// The shader's alpha test carries the source's own threshold (`RenderItem.java`:322's
    /// `alphaFunc(516, 0.1F)`).
    #[test]
    fn the_shader_carries_the_sources_alpha_test() {
        assert!(
            SHADER.contains(&format!("texel.a <= {ALPHA_TEST}")),
            "the shader's alpha test threshold"
        );
    }

    /// The hand's lights (`rotateArroundXAndY`:99-106): the two standard positions
    /// turned by the pose — the pitch about x, then the yaw about y — and unit
    /// length either way.
    #[test]
    fn the_hand_lights_turn_with_the_pose() {
        let level = hand_lights(0.0, 0.0);
        let want = Vec3::from_array(LIGHT_POSITIONS[0]).normalize();
        assert!((level[0] - want).length() < 1e-6, "the level light");
        for light in level {
            assert!((light.length() - 1.0).abs() < 1e-5, "unit length");
        }
        // A 90-degree pitch turns the light: the y and z terms swap roles.
        let pitched = hand_lights(90.0, 0.0);
        assert!(
            (pitched[0] - Vec3::new(0.2, 0.7, 1.0).normalize()).length() < 1e-5,
            "the pitched light: {:?}",
            pitched[0]
        );
        // The flat quad's own shade at rest: the normal (0, 0, 1) through the
        // chain's normal matrix is (-1, 0, 0), so only the second light's own x
        // term contributes: 0.4 + 0.6 * 0.2/sqrt(0.2^2 + 1^2 + 0.7^2).
        let matrix = held_matrix(0.0, 0.0, sword());
        let colour = lit_colour([0.0, 0.0, 1.0], normal_matrix(matrix), &level, 1.0);
        let expected = 0.4 + 0.6 * (0.2f32 / (0.2f32 * 0.2 + 1.0 + 0.7 * 0.7).sqrt());
        assert!(
            (colour[0] - expected).abs() < 1e-5,
            "the quad's shade: {} != {expected}",
            colour[0]
        );
        // The brightness multiplies the shade.
        let dim = lit_colour([0.0, 0.0, 1.0], normal_matrix(matrix), &level, 0.5);
        assert!(
            (dim[0] - colour[0] * 0.5).abs() < 1e-5,
            "the lightmap factor"
        );
    }
}
