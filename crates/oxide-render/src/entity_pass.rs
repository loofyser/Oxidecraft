//! The entity pass: the box models drawn between the terrain's solid and translucent layers.
//!
//! The pass draws the frame's [`EntityDraw`] list through one vertex format and three
//! pipelines. The model pipeline renders a box model's vertices — the per-kind poses turned
//! into world space on the CPU, the texture, the entity's own brightness in the vertex colour
//! and the client's two standard item lights shading each face from its normal
//! (`RenderHelper.enableStandardItemLighting`'s fixed-function pair, expressed per fragment).
//! The hurt pipeline re-draws the same vertices through the source's damage combine: the
//! lightmap stage's interpolate, `0.7 * previous + 0.3 * (1, 0, 0)`
//! (`RendererLivingEntity.setBrightness`), gated by the same condition — `hurtTime > 0 ||
//! deathTime > 0`. The shadow pipeline draws the flat quad the source lays under an entity's
//! feet (`Render.renderShadow`), from the named shadow texture, blending its texture's alpha
//! by the fade the source computes.
//!
//! The draw order inside one entity is the source's: the model (with its hurt pass in place of
//! the plain one when the damage combine is in force), then the cape layer, then the shadow
//! (`RenderManager.renderEntity` draws the shadow after the entity, `RenderManager.java:388`).
//!
//! The transforms compose the transforms `RendererLivingEntity.doRender` and
//! `RenderPlayer.doRender` build: the position (with a sneaking player's own eighth-block
//! drop), `rotateCorpse`'s `180 - bodyYaw` about y with the death tilt about z, the model
//! flip `scale(-1, -1, 1)`, the player's own `0.9375` shrink (`RenderPlayer.preRenderCallback`)
//! and the `-1.5078125` the living renderer drops the model by, all before the model's own
//! 1/16 geometry. Everything is composed into `f32` world-space vertices on the CPU; the
//! shader applies the frame's view-projection and nothing else.
//!
//! The texture registry holds what draws sample: the named textures uploaded once (`Named`
//! keys; a key with nothing behind it resolves to a generated placeholder and logs), the two
//! default skins, and the per-uuid skins and capes the client uploads as fetches land.
//! [`SkinLookup`] is the resolver both the pass and the later tab list share.

use std::collections::BTreeMap;
use std::ops::Range;

use glam::{Mat3, Mat4, Vec3};
use oxide_assets::atlas::missing_pixels;
use oxide_assets::texture::Texture;

use crate::camera::{Camera, render_eye};
use crate::entity_models::{self, PoseExtra, build_vertices, player};
use crate::fog::FogParams;
use crate::terrain_pass::{color_target, depth_state, primitive_state};

/// The model a draw uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelRef {
    /// A player: the slim arms and the model-parts byte (`EnumPlayerModelParts`; this
    /// milestone's draws pin every bit on).
    Player {
        /// Whether the arms draw slim.
        slim: bool,
        /// The model-parts byte: which of the skin's overlay parts the draw wears.
        parts: u8,
    },
}

/// The texture a draw samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextureRef {
    /// A texture the client uploaded once, by its key.
    Named(&'static str),
    /// A player's skin, resolved through the registry by profile.
    Skin {
        /// The profile's hyphenated uuid.
        uuid: String,
        /// Whether the draw wants the slim default when no skin is uploaded.
        slim: bool,
    },
}

/// The extras a draw carries beyond its model — the per-kind state the kind's own path reads;
/// empty at this milestone.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum DrawExtra {
    /// A draw whose model and poses carry everything.
    #[default]
    None,
}

/// One entity's draw for a frame, as the window assembles it.
///
/// The positions and angles are already interpolated for the frame's fraction; `light` is the
/// brightness at the entity's feet (`EntityLivingBase.getBrightness`), the vertex colour the
/// draw bakes; `hurt` and `death` are the damage combines the pass gates on; `health` is the
/// boss kinds' pair, for the status the pass will raise.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityDraw {
    /// The box model the draw renders.
    pub model: ModelRef,
    /// The interpolated position, in blocks.
    pub position: [f64; 3],
    /// The interpolated body yaw in degrees (`renderYawOffset`).
    pub body_yaw: f32,
    /// The interpolated net head yaw in degrees.
    pub head_yaw: f32,
    /// The interpolated head pitch in degrees.
    pub head_pitch: f32,
    /// The model's pose input.
    pub pose: entity_models::Pose,
    /// The texture the draw samples.
    pub texture: TextureRef,
    /// The brightness at the entity's feet, `0.0..1.0`.
    pub light: f32,
    /// The hurt window's fraction: one while it is open, else zero.
    pub hurt: f32,
    /// The death ramp's fraction, one at its end.
    pub death: f32,
    /// The health pair for the kinds whose maximum is known, in hearts.
    pub health: Option<(f32, f32)>,
    /// The kind's own extras.
    pub extra: DrawExtra,
}

/// A registered entity texture: the GPU texture and the bind group the pass samples through.
pub struct RegisteredTexture {
    /// The texture itself, kept so its view stays valid.
    texture: wgpu::Texture,
    /// The view the bind group holds.
    view: wgpu::TextureView,
    /// The bind group the pass sets at group 1.
    bind_group: wgpu::BindGroup,
}

impl RegisteredTexture {
    /// The texture's view, for consumers outside the pipeline binding (the tab list's head
    /// draws).
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// The texture's size in texels.
    pub fn size(&self) -> [u32; 2] {
        [self.texture.width(), self.texture.height()]
    }
}

/// The skin resolver the entity pass and the tab list share: the texture a profile draws
/// with.
pub trait SkinLookup {
    /// The registered texture for a profile's skin: the uploaded skin when one has arrived,
    /// else the default for `slim`.
    fn resolve(&self, uuid: &str, slim: bool) -> &RegisteredTexture;
}

/// One profile's uploaded textures.
struct SkinEntry {
    /// The skin, at the profile's own resolution; `None` until one arrives, so the resolver
    /// falls back to the default.
    skin: Option<RegisteredTexture>,
    /// The cape, when the profile named one and it arrived; absent clears it.
    cape: Option<RegisteredTexture>,
}

/// The entity texture registry: the named textures, the default skins, the per-profile skins
/// and the placeholder a missing key resolves to.
pub struct TextureRegistry {
    /// The layout every texture bind group is built against, shared with the pipeline.
    layout: wgpu::BindGroupLayout,
    /// The sampler every entity texture is sampled through: nearest, clamped — the pixel
    /// sheet's own look.
    sampler: wgpu::Sampler,
    /// The named textures, by key.
    named: BTreeMap<&'static str, RegisteredTexture>,
    /// The wide and slim default skins, in that order.
    defaults: [RegisteredTexture; 2],
    /// The placeholder a named key with nothing behind it resolves to: the generated
    /// magenta-and-black checkerboard the atlas's fallback sprite is built from.
    placeholder: RegisteredTexture,
    /// The per-profile entries, ascending uuid.
    skins: BTreeMap<String, SkinEntry>,
}

impl TextureRegistry {
    /// Builds the registry with its placeholder; no named texture and no default skin is
    /// uploaded yet.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let layout = texture_bind_group_layout(device);
        let sampler = device.create_sampler(&texture_sampler_descriptor());
        let placeholder = upload_texture(
            device,
            queue,
            "oxide entity placeholder",
            &placeholder_image(),
            &layout,
            &sampler,
        );
        let defaults = [
            upload_texture(
                device,
                queue,
                "oxide entity default wide",
                &placeholder_image(),
                &layout,
                &sampler,
            ),
            upload_texture(
                device,
                queue,
                "oxide entity default slim",
                &placeholder_image(),
                &layout,
                &sampler,
            ),
        ];
        Self {
            layout,
            sampler,
            named: BTreeMap::new(),
            defaults,
            placeholder,
            skins: BTreeMap::new(),
        }
    }

    /// The bind group layout the entity pass's second bind group expects; a [`TextureRegistry`]
    /// built over it pairs with the pass.
    pub fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// Uploads a named texture, replacing whatever the key held.
    pub fn set_named(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: &'static str,
        image: &Texture,
    ) {
        let texture = upload_texture(device, queue, key, image, &self.layout, &self.sampler);
        self.named.insert(key, texture);
    }

    /// Uploads the two default skins: the wide fallback and the slim one.
    pub fn set_defaults(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        wide: &Texture,
        slim: &Texture,
    ) {
        self.defaults[0] = upload_texture(
            device,
            queue,
            "oxide entity default wide",
            wide,
            &self.layout,
            &self.sampler,
        );
        self.defaults[1] = upload_texture(
            device,
            queue,
            "oxide entity default slim",
            slim,
            &self.layout,
            &self.sampler,
        );
    }

    /// Uploads one profile's skin and cape, replacing the profile's entry whole: an absent
    /// texture clears that half (a re-upload replaces, a missing cape is no cape).
    ///
    /// A profile whose skin has not arrived keeps the default skin but may still carry a
    /// cape; an entry with neither is dropped.
    pub fn set_skin(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        uuid: &str,
        skin: Option<&Texture>,
        cape: Option<&Texture>,
    ) {
        let skin = skin.map(|skin| {
            upload_texture(
                device,
                queue,
                "oxide entity skin",
                skin,
                &self.layout,
                &self.sampler,
            )
        });
        let cape = cape.map(|cape| {
            upload_texture(
                device,
                queue,
                "oxide entity cape",
                cape,
                &self.layout,
                &self.sampler,
            )
        });
        if skin.is_none() && cape.is_none() {
            self.skins.remove(uuid);
            return;
        }
        self.skins
            .insert(uuid.to_string(), SkinEntry { skin, cape });
    }

    /// The cape a profile uploaded, when one is registered.
    pub fn cape(&self, uuid: &str) -> Option<&RegisteredTexture> {
        self.skins.get(uuid).and_then(|entry| entry.cape.as_ref())
    }

    /// The texture a draw's reference resolves to: a named key, or a profile's skin.
    ///
    /// A named key with nothing behind it resolves to the placeholder and logs at debug; a
    /// profile with no uploaded skin resolves to the default for `slim`.
    pub fn texture_for(&self, texture: &TextureRef) -> &RegisteredTexture {
        match texture {
            TextureRef::Named(key) => match self.named.get(*key) {
                Some(texture) => texture,
                None => {
                    tracing::debug!(
                        "the entity texture {key:?} has nothing behind it; drawing the placeholder"
                    );
                    &self.placeholder
                }
            },
            TextureRef::Skin { uuid, slim } => self.resolve(uuid, *slim),
        }
    }

    /// The default skin for a slim or wide draw.
    fn default_skin(&self, slim: bool) -> &RegisteredTexture {
        &self.defaults[usize::from(slim)]
    }
}

impl SkinLookup for TextureRegistry {
    fn resolve(&self, uuid: &str, slim: bool) -> &RegisteredTexture {
        match self.skins.get(uuid) {
            Some(entry) => match &entry.skin {
                Some(skin) => skin,
                None => self.default_skin(slim),
            },
            None => self.default_skin(slim),
        }
    }
}

/// The texture bind group layout: the texture at binding [`TEXTURE_BINDING`] and its sampler
/// next to it.
pub(crate) fn texture_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("oxide entity texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: TEXTURE_BINDING,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: SAMPLER_BINDING,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

/// The sampler every entity texture binds: nearest on both filters, clamped at the edges —
/// the pixel sheet's own look, and the clamp the source's shadow quads read off the sprite's
/// edge.
fn texture_sampler_descriptor() -> wgpu::SamplerDescriptor<'static> {
    wgpu::SamplerDescriptor {
        label: Some("oxide entity sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    }
}

/// The generated placeholder image: the atlas's own fallback checkerboard, so the pass and
/// the terrain show the same missing texture.
fn placeholder_image() -> Texture {
    let side = 16;
    Texture {
        width: side,
        height: side,
        rgba: missing_pixels(),
    }
}

/// Uploads one RGBA image into a registered texture.
fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    image: &Texture,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
) -> RegisteredTexture {
    let size = wgpu::Extent3d {
        width: image.width,
        height: image.height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
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
        &image.rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(image.width * 4),
            rows_per_image: Some(image.height),
        },
        size,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: TEXTURE_BINDING,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: SAMPLER_BINDING,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    RegisteredTexture {
        texture,
        view,
        bind_group,
    }
}

/// The entity pass: its three pipelines, the frame uniforms they read, and the vertex buffer
/// the frame's geometry is uploaded into.
pub struct EntityPass {
    /// The queue the pass uploads through, a handle of the renderer's own.
    queue: wgpu::Queue,
    /// The model pipeline: the lit fragment.
    model_pipeline: wgpu::RenderPipeline,
    /// The hurt pipeline: the damage combine's fragment.
    hurt_pipeline: wgpu::RenderPipeline,
    /// The shadow pipeline: the flat quad, depth writes off.
    shadow_pipeline: wgpu::RenderPipeline,
    /// The uniform buffer holding the frame's matrix, eye, fog and lights.
    frame_buffer: wgpu::Buffer,
    /// The bind group the frame uniforms are read through.
    frame_bind_group: wgpu::BindGroup,
    /// The vertex buffer, grown to hold the largest frame so far.
    vertex_buffer: wgpu::Buffer,
    /// The vertex buffer's capacity in bytes.
    vertex_capacity: usize,
    /// The frame's uniform values, kept so either setter can rewrite them whole.
    frame: FrameUniform,
    /// Whether a camera has been set: without one nothing draws.
    camera_set: bool,
}

impl EntityPass {
    /// Builds the pass's pipelines built for colour attachments in `format`.
    ///
    /// The vertex layout is [`vertex_bytes`]' stream; every pipeline tests the depth buffer
    /// in [`DEPTH_FORMAT`] and culls nothing — the source disables culling for entities.
    /// Group 0 is the frame's uniform; group 1 is the draw's texture, created by the registry
    /// against the `layout` handed in, which is the registry's own.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        texture_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oxide entity shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source().into()),
        });
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide entity frame layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(FRAME_BYTES as u64),
                },
                count: None,
            }],
        });
        let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide entity frame"),
            size: FRAME_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide entity frame bind group"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buffer.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("oxide entity pipeline layout"),
            bind_group_layouts: &[&frame_layout, texture_layout],
            push_constant_ranges: &[],
        });
        let pipeline = |label: &str, fragment: &str, plan: PipelinePlan| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(VS_ENTRY),
                    buffers: &[vertex_layout()],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    targets: &[color_target(format, plan.blend)],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: primitive_state(None),
                depth_stencil: Some(depth_state(plan.depth_write)),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let model_pipeline = pipeline(
            "oxide entity model pipeline",
            FRAGMENT_MODEL,
            PipelinePlan {
                blend: None,
                depth_write: true,
            },
        );
        let hurt_pipeline = pipeline(
            "oxide entity hurt pipeline",
            FRAGMENT_HURT,
            PipelinePlan {
                blend: None,
                depth_write: true,
            },
        );
        let shadow_pipeline = pipeline(
            "oxide entity shadow pipeline",
            FRAGMENT_SHADOW,
            PipelinePlan {
                blend: Some(shadow_blend()),
                depth_write: false,
            },
        );
        let vertex_capacity = INITIAL_VERTEX_BYTES;
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide entity vertices"),
            size: vertex_capacity as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            queue: queue.clone(),
            model_pipeline,
            hurt_pipeline,
            shadow_pipeline,
            frame_buffer,
            frame_bind_group,
            vertex_buffer,
            vertex_capacity,
            frame: FrameUniform::default(),
            camera_set: false,
        }
    }

    /// Writes the frame's camera: its view-projection matrix, the eye and the two standard
    /// item lights in world space.
    ///
    /// The lights are the source's fixed-function pair — positions `(0.2, 1.0, -0.7)` and
    /// `(-0.2, 1.0, 0.7)` in eye space (`RenderHelper.enableStandardItemLighting`) — rotated
    /// into world space by the camera's own rotation, so a face's shading follows the camera
    /// the way the fixed-function pipeline lights it.
    pub fn set_camera(&mut self, camera: Camera, aspect: f32) {
        let eye = render_eye(&camera.pose);
        self.frame.view_projection = camera.view_projection(aspect);
        self.frame.eye = [eye.x, eye.y, eye.z, 0.0];
        let rotation = Mat3::from_mat4(camera.view());
        let lights = entity_lights(rotation);
        self.frame.light0 = [lights[0][0], lights[0][1], lights[0][2], 0.0];
        self.frame.light1 = [lights[1][0], lights[1][1], lights[1][2], 0.0];
        self.write_frame();
        self.camera_set = true;
    }

    /// Writes the frame's fog for the frames that follow.
    pub fn set_fog(&mut self, params: FogParams) {
        self.frame.fog_colour = [params.colour[0], params.colour[1], params.colour[2], 0.0];
        self.frame.fog_params = [params.start, params.end, params.far_plane, 0.0];
        self.write_frame();
    }

    /// Writes the frame uniform back out.
    fn write_frame(&mut self) {
        self.queue
            .write_buffer(&self.frame_buffer, 0, &self.frame.to_bytes());
    }

    /// Draws the frame's entities.
    ///
    /// Every draw composes its model-space vertices into world space, uploads them in one
    /// write, then issues the source's own sequence: the model (or the hurt combine in its
    /// place when the damage window is open), the cape when the parts byte wears it and a cape
    /// texture is registered, and the shadow quad under the feet. Nothing draws until a camera
    /// has been set.
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        pass: &mut wgpu::RenderPass<'_>,
        draws: &[EntityDraw],
        textures: &TextureRegistry,
    ) {
        if !self.camera_set || draws.is_empty() {
            return;
        }
        let mut vertices: Vec<EntityVertex> = Vec::new();
        let mut built = Vec::with_capacity(draws.len());
        for draw in draws {
            built.push(self.build(draw, textures, &mut vertices));
        }
        if vertices.is_empty() {
            return;
        }
        self.upload(device, &vertices);
        for (draw, geometry) in draws.iter().zip(&built) {
            let texture = textures.texture_for(&draw.texture);
            pass.set_pipeline(&self.model_pipeline);
            pass.set_bind_group(0, &self.frame_bind_group, &[]);
            pass.set_bind_group(1, &texture.bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            pass.draw(geometry.body.clone(), 0..1);
            if geometry.hurt {
                pass.set_pipeline(&self.hurt_pipeline);
                pass.draw(geometry.body.clone(), 0..1);
            }
            if let Some(range) = &geometry.cape {
                if let TextureRef::Skin { uuid, .. } = &draw.texture {
                    if let Some(cape_texture) = textures.cape(uuid) {
                        pass.set_pipeline(&self.model_pipeline);
                        pass.set_bind_group(1, &cape_texture.bind_group, &[]);
                        pass.draw(range.clone(), 0..1);
                    }
                }
            }
            // The shadow draws last: `RenderManager.renderEntity` draws it after the entity
            // (`RenderManager.java:388`), through the shared shadow sprite.
            if let Some(range) = &geometry.shadow {
                let shadow = textures.texture_for(&TextureRef::Named(SHADOW_TEXTURE));
                pass.set_pipeline(&self.shadow_pipeline);
                pass.set_bind_group(0, &self.frame_bind_group, &[]);
                pass.set_bind_group(1, &shadow.bind_group, &[]);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                pass.draw(range.clone(), 0..1);
            }
        }
    }

    /// Builds one draw's geometry into `vertices`, returning the ranges to draw.
    fn build(
        &self,
        draw: &EntityDraw,
        textures: &TextureRegistry,
        vertices: &mut Vec<EntityVertex>,
    ) -> BuiltDraw {
        let ModelRef::Player { parts, .. } = draw.model;
        let model = entity_models::model_for(draw.model);
        let colour = [draw.light, draw.light, draw.light, 1.0];

        let mut built = BuiltDraw {
            shadow: None,
            body: 0..0,
            cape: None,
            hurt: draw.hurt > 0.0 || draw.death > 0.0,
        };

        // The shadow first in the buffer (drawn after the model, in the source's order).
        if let Some((corners, uvs, alpha)) = shadow_quad(
            draw,
            entity_models::shadow(draw.model),
            self.frame.eye_world(),
        ) {
            let start = vertices.len() as u32;
            let corners: Vec<EntityVertex> = corners
                .into_iter()
                .zip(uvs)
                .map(|(position, uv)| EntityVertex {
                    position,
                    uv,
                    normal: [0.0, 1.0, 0.0],
                    colour: [1.0, 1.0, 1.0, alpha],
                })
                .collect();
            // Four corners per quad: the quad's two triangles, front-loaded.
            for index in [0, 1, 2, 0, 2, 3] {
                vertices.push(corners[index]);
            }
            built.shadow = Some(start..vertices.len() as u32);
        }

        // The body: the model's parts with the cape's bit cleared — the cape is its own build
        // through its own sheet.
        let mut rots = model.rest();
        player::pose(&draw.pose, parts & !player::PART_CAPE, &mut rots);
        let body = build_vertices(model, &rots, player::PLAYER_TEXTURE_SIZE);
        let chain = body_chain(draw);
        let start = vertices.len() as u32;
        push_vertices(vertices, &body, chain, colour);
        built.body = start..vertices.len() as u32;

        // The cape: the layer's own box, wave and chain.
        if parts & player::PART_CAPE != 0 {
            if let TextureRef::Skin { uuid, .. } = &draw.texture {
                if textures.cape(uuid).is_some() {
                    let rot = player::cape_rot(&draw.pose, parts);
                    let cape = build_vertices(
                        &player::MODEL_PLAYER_CAPE,
                        std::slice::from_ref(&rot),
                        player::CAPE_TEXTURE_SIZE,
                    );
                    let motion = match draw.pose.extra {
                        PoseExtra::Player(cape) => cape.motion,
                        PoseExtra::None => [0.0; 3],
                    };
                    let angles = player::cape_rotation(&draw.pose, motion);
                    let start = vertices.len() as u32;
                    push_vertices(vertices, &cape, cape_chain(draw, angles), colour);
                    built.cape = Some(start..vertices.len() as u32);
                }
            }
        }
        built
    }
}

/// The body chain for a draw: the source's `renderLivingAt` composition — the interpolated
/// position, the `180 - body_yaw` turn, the death tilt, the `(-1, -1, 1)` flip, the player
/// renderer's `0.9375` pre-render scale, the `-1.5078125` model drop and the sneak lift,
/// then the model's own 1/16 units (`Render.doRender`, `RenderPlayer`'s pre-render callback,
/// `RendererLivingEntity.doRender`).
fn body_chain(draw: &EntityDraw) -> Mat4 {
    let position = Vec3::new(
        draw.position[0] as f32,
        draw.position[1] as f32 - sneak_drop(&draw.pose),
        draw.position[2] as f32,
    );
    let death = (draw.death * DEATH_MAX_ROTATION).to_radians();
    let lift = if draw.pose.sneak {
        SNEAK_MODEL_LIFT
    } else {
        0.0
    };
    Mat4::from_translation(position)
        * Mat4::from_rotation_y((180.0 - draw.body_yaw).to_radians())
        * Mat4::from_rotation_z(death)
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
        * Mat4::from_scale(Vec3::splat(RENDER_SCALE))
        * Mat4::from_translation(Vec3::new(0.0, MODEL_DROP, 0.0))
        * Mat4::from_translation(Vec3::new(0.0, lift, 0.0))
        * Mat4::from_scale(Vec3::splat(1.0 / 16.0))
}

/// The cape layer's chain: the body's, without the model's sneak lift, with the layer's own
/// `+0.125` z offset and its three turns (`LayerCape.doRenderLayer`).
fn cape_chain(draw: &EntityDraw, angles: [f32; 3]) -> Mat4 {
    let position = Vec3::new(
        draw.position[0] as f32,
        draw.position[1] as f32 - sneak_drop(&draw.pose),
        draw.position[2] as f32,
    );
    let death = (draw.death * DEATH_MAX_ROTATION).to_radians();
    Mat4::from_translation(position)
        * Mat4::from_rotation_y((180.0 - draw.body_yaw).to_radians())
        * Mat4::from_rotation_z(death)
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
        * Mat4::from_scale(Vec3::splat(RENDER_SCALE))
        * Mat4::from_translation(Vec3::new(0.0, MODEL_DROP, 0.0))
        * Mat4::from_translation(Vec3::new(0.0, 0.0, CAPE_OFFSET))
        * Mat4::from_rotation_x(angles[0].to_radians())
        * Mat4::from_rotation_z(angles[2].to_radians())
        * Mat4::from_rotation_y(angles[1].to_radians())
        * Mat4::from_scale(Vec3::splat(1.0 / 16.0))
}

impl EntityPass {
    /// Grows the vertex buffer if the frame needs it and writes the frame's vertices.
    fn upload(&mut self, device: &wgpu::Device, vertices: &[EntityVertex]) {
        let bytes = vertices.len() * VERTEX_BYTES;
        if bytes > self.vertex_capacity {
            let capacity = bytes.next_power_of_two();
            self.vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("oxide entity vertices"),
                size: capacity as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.vertex_capacity = capacity;
        }
        self.queue
            .write_buffer(&self.vertex_buffer, 0, &vertex_bytes(vertices));
    }
}

/// One draw's built ranges into the frame's vertex buffer.
struct BuiltDraw {
    /// The shadow quad's range, when the fade leaves it visible.
    shadow: Option<Range<u32>>,
    /// The model's range.
    body: Range<u32>,
    /// The cape's range, when both the bit and a texture are present.
    cape: Option<Range<u32>>,
    /// Whether the hurt combine draws over the body.
    hurt: bool,
}

/// The shadow quad's geometry: its four corners, their uvs and the vertex alpha.
type ShadowQuad = ([[f32; 3]; 4], [[f32; 2]; 4], f32);

/// The shadow quad for a draw: its four corners and their uvs, and the vertex alpha.
///
/// The quad is the block the entity stands in, centred on the entity and a whit above the
/// feet, the sprite sampled from corner to corner with the source's own mapping
/// (`Render.renderShadowBlock`: `(x - minX) / 2f + 0.5` reads one at the low corner, so the
/// sprite runs backwards). The alpha is the source's own: the camera's distance fade
/// `1 - d / 256` times the class's opacity (`Render.doRender`), halved for the block under
/// the entity and scaled by the feet's light (`Render.renderShadowBlock`'s `d0` for the
/// block the entity stands in).
fn shadow_quad(draw: &EntityDraw, shadow: [f32; 2], eye: [f32; 3]) -> Option<ShadowQuad> {
    let [size, opacity] = shadow;
    let position = Vec3::new(
        draw.position[0] as f32,
        draw.position[1] as f32,
        draw.position[2] as f32,
    );
    let distance = position.distance(Vec3::from(eye));
    let fade = (1.0 - distance / SHADOW_FADE_DISTANCE) * opacity;
    let alpha = fade * 0.5 * draw.light;
    if alpha <= 0.0 {
        return None;
    }
    let half = size;
    let y = position.y + SHADOW_LIFT;
    let corners = [
        [position.x - half, y, position.z - half],
        [position.x - half, y, position.z + half],
        [position.x + half, y, position.z + half],
        [position.x + half, y, position.z - half],
    ];
    let uvs = [[1.0, 1.0], [1.0, 0.0], [0.0, 0.0], [0.0, 1.0]];
    Some((corners, uvs, alpha))
}

/// The two standard item lights in world space, from the eye-space pair the source sets.
fn entity_lights(rotation: Mat3) -> [[f32; 3]; 2] {
    let eye_to_world = rotation.transpose();
    let l0 = eye_to_world * Vec3::new(0.2, 1.0, -0.7).normalize();
    let l1 = eye_to_world * Vec3::new(-0.2, 1.0, 0.7).normalize();
    [l0.normalize().into(), l1.normalize().into()]
}

/// The sneaking player's own drop, in blocks (`RenderPlayer.doRender`).
fn sneak_drop(pose: &entity_models::Pose) -> f32 {
    if pose.sneak { SNEAK_POSITION_DROP } else { 0.0 }
}

/// Pushes a built vertex set through `chain` with the draw's colour.
///
/// The build emits four corners per quad; the stream carries the quad's two triangles — the
/// pipelines index nothing — so each quad lands front-loaded as `(0, 1, 2)` and `(0, 2, 3)`.
fn push_vertices(
    out: &mut Vec<EntityVertex>,
    vertices: &entity_models::Vertices,
    chain: Mat4,
    colour: [f32; 4],
) {
    let corner = |index: usize| EntityVertex {
        position: chain
            .transform_point3(Vec3::from(vertices.positions[index]))
            .into(),
        uv: vertices.uvs[index],
        normal: chain
            .transform_vector3(Vec3::from(vertices.normals[index]))
            .normalize_or_zero()
            .into(),
        colour,
    };
    let quads = vertices.positions.len() & !3;
    for quad in (0..quads).step_by(4) {
        for index in [quad, quad + 1, quad + 2, quad, quad + 2, quad + 3] {
            out.push(corner(index));
        }
    }
}

/// One vertex the entity pipelines take.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
struct EntityVertex {
    /// World-space position.
    position: [f32; 3],
    /// The texture uv.
    uv: [f32; 2],
    /// The face's normal.
    normal: [f32; 3],
    /// The vertex colour: the entity's brightness in the rgb channels.
    colour: [f32; 4],
}

/// The vertex stream's stride in bytes: a `Float32x3` position, a `Float32x2` uv, a
/// `Float32x3` normal and a `Float32x4` colour.
const VERTEX_BYTES: usize = 12 + 8 + 12 + 16;

/// The vertex attributes [`vertex_bytes`] lays out.
static ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x2,
    2 => Float32x3,
    3 => Float32x4
];

/// The vertex layout the pipelines declare.
fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: VERTEX_BYTES as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    }
}

/// Packs the frame's vertices into little-endian bytes.
fn vertex_bytes(vertices: &[EntityVertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * VERTEX_BYTES);
    for vertex in vertices {
        for value in vertex.position {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in vertex.uv {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in vertex.normal {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in vertex.colour {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

/// The frame's uniform values, laid out as the shader's `Frame` struct.
#[derive(Debug, Clone, Copy)]
struct FrameUniform {
    /// The camera's view-projection matrix.
    view_projection: Mat4,
    /// The eye's world position, the fourth component unused.
    eye: [f32; 4],
    /// The fog colour, the alpha unused.
    fog_colour: [f32; 4],
    /// The fog's start, end and far plane.
    fog_params: [f32; 4],
    /// The first standard item light's world direction.
    light0: [f32; 4],
    /// The second standard item light's world direction.
    light1: [f32; 4],
}

impl Default for FrameUniform {
    fn default() -> Self {
        Self {
            view_projection: Mat4::IDENTITY,
            eye: [0.0; 4],
            fog_colour: [0.0; 4],
            fog_params: [0.0; 4],
            light0: [0.0; 4],
            light1: [0.0; 4],
        }
    }
}

/// The size of the frame uniform in bytes: a matrix and five `vec4`s.
const FRAME_BYTES: usize = 64 + 5 * 16;

impl FrameUniform {
    /// The eye the frame was set with, in world space; zero before a camera is set.
    fn eye_world(&self) -> [f32; 3] {
        [self.eye[0], self.eye[1], self.eye[2]]
    }

    /// Packs the uniform into little-endian bytes.
    fn to_bytes(self) -> [u8; FRAME_BYTES] {
        let mut bytes = [0u8; FRAME_BYTES];
        let mut at = 0;
        push_values(&mut bytes, &mut at, &self.view_projection.to_cols_array());
        push_values(&mut bytes, &mut at, &self.eye);
        push_values(&mut bytes, &mut at, &self.fog_colour);
        push_values(&mut bytes, &mut at, &self.fog_params);
        push_values(&mut bytes, &mut at, &self.light0);
        push_values(&mut bytes, &mut at, &self.light1);
        bytes
    }
}

/// Appends one run of `f32`s to a byte buffer at a cursor.
fn push_values(bytes: &mut [u8], at: &mut usize, values: &[f32]) {
    for value in values {
        bytes[*at..*at + 4].copy_from_slice(&value.to_le_bytes());
        *at += 4;
    }
}

/// One pipeline's state choices, kept as a value so the tests can pin them without a device.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PipelinePlan {
    /// The colour blend; the model and hurt pipelines draw the source's unblended state.
    blend: Option<wgpu::BlendState>,
    /// Whether the pipeline writes the depth buffer.
    depth_write: bool,
}

/// The shadow's blend: source alpha over one-minus-source alpha, the alpha channel kept
/// (`Render.renderShadow`: `blendFunc(770, 771)`).
fn shadow_blend() -> wgpu::BlendState {
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

/// The texture binding in the texture group.
const TEXTURE_BINDING: u32 = 0;

/// The sampler binding in the texture group.
const SAMPLER_BINDING: u32 = 1;

/// The vertex entry point.
const VS_ENTRY: &str = "vs_main";

/// The model fragment: the lit sheet.
const FRAGMENT_MODEL: &str = "fs_model";

/// The hurt fragment: the damage combine.
const FRAGMENT_HURT: &str = "fs_hurt";

/// The shadow fragment.
const FRAGMENT_SHADOW: &str = "fs_shadow";

/// The alpha below which the model and hurt fragments discard: the client's own tenth.
const CUTOUT_ALPHA: f32 = 0.1;

/// The player's render shrink (`RenderPlayer.preRenderCallback`).
const RENDER_SCALE: f32 = 0.9375;

/// The drop the living renderer puts under every model, in blocks
/// (`RendererLivingEntity.doRender`: `translate(0, -1.5078125, 0)`).
const MODEL_DROP: f32 = -1.5078125;

/// The sneaking player's own position drop, in blocks (`RenderPlayer.doRender`).
const SNEAK_POSITION_DROP: f32 = 0.125;

/// The model's own sneak lift, in blocks (`ModelBiped.render`'s `translate(0, 0.2, 0)`); the
/// model frame's y runs downwards after the flip, so the lift lowers the model.
const SNEAK_MODEL_LIFT: f32 = 0.2;

/// The cape layer's own forward offset, in blocks (`LayerCape.doRenderLayer`).
const CAPE_OFFSET: f32 = 0.125;

/// The death tilt's largest angle in degrees (`RendererLivingEntity.getDeathMaxRotation`).
const DEATH_MAX_ROTATION: f32 = 90.0;

/// The shadow sprite's key: the shared sheet every shadow quad samples
/// (`Render.renderShadow`'s `misc/shadow.png`).
const SHADOW_TEXTURE: &str = "misc/shadow.png";

/// The camera distance at which the shadow's fade reaches zero (`Render.doRender`: the alpha
/// `(1 - d / 256) * shadowOpaque`).
const SHADOW_FADE_DISTANCE: f32 = 256.0;

/// How far above the feet the shadow quad lies, in blocks (`Render.renderShadowBlock`:
/// `pos.getY() + blockBounds + dy + 0.015625`).
const SHADOW_LIFT: f32 = 0.015625;

/// The first vertex buffer's capacity in bytes.
const INITIAL_VERTEX_BYTES: usize = 1024 * 48;

/// The shader source: the entity shader.
fn shader_source() -> String {
    format!(
        r#"
struct Frame {{
    view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    fog_colour: vec4<f32>,
    fog_params: vec4<f32>,
    light0: vec4<f32>,
    light1: vec4<f32>,
}};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var entity: texture_2d<f32>;
@group(1) @binding(1) var entity_sampler: sampler;

// The alpha below which the client's alpha test discards a fragment.
const CUTOUT_ALPHA: f32 = {CUTOUT_ALPHA};

struct VertexInput {{
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) colour: vec4<f32>,
}};

struct VertexOutput {{
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) shade: f32,
    @location(2) colour: vec4<f32>,
    @location(3) distance: f32,
}};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {{
    var output: VertexOutput;
    output.clip_position = frame.view_projection * vec4<f32>(input.position, 1.0);
    output.distance = length(input.position - frame.eye.xyz);
    // The two standard item lights: the global ambient plus each light's own diffuse term,
    // clamped once the way the fixed-function pipeline clamps the lit colour.
    let diffuse = max(dot(input.normal, frame.light0.xyz), 0.0)
        + max(dot(input.normal, frame.light1.xyz), 0.0);
    output.shade = min(0.4 + 0.6 * diffuse, 1.0);
    output.uv = input.uv;
    output.colour = input.colour;
    return output;
}}

// The linear fog, the terrain pass's own rule: the factor is one at the fade's start and zero
// at its end; a range that does not run forwards leaves the colour alone.
fn fogged(colour: vec4<f32>, distance: f32) -> vec4<f32> {{
    let span = frame.fog_params.y - frame.fog_params.x;
    if (span <= 0.0) {{
        return colour;
    }}
    let factor = clamp((frame.fog_params.y - distance) / span, 0.0, 1.0);
    return vec4<f32>(mix(frame.fog_colour.rgb, colour.rgb, factor), colour.a);
}}

// The model's fragment: the texel times the entity's brightness and the face's shade, then
// the fog; alpha comes from the texel alone, and the fragment below the client's tenth is
// discarded.
@fragment
fn fs_model(input: VertexOutput) -> @location(0) vec4<f32> {{
    let texel = textureSample(entity, entity_sampler, input.uv);
    let colour = vec4<f32>(texel.rgb * input.colour.rgb * input.shade, texel.a);
    if (colour.a < CUTOUT_ALPHA) {{
        discard;
    }}
    return fogged(colour, input.distance);
}}

// The hurt combine: the lightmap stage's interpolate — 0.7 of the lit texel with 0.3 of the
// damage red added (`RendererLivingEntity.setBrightness`'s (1, 0, 0, 0.3) constant).
@fragment
fn fs_hurt(input: VertexOutput) -> @location(0) vec4<f32> {{
    let texel = textureSample(entity, entity_sampler, input.uv);
    let lit = texel.rgb * input.colour.rgb * input.shade;
    let colour = vec4<f32>(lit * 0.7 + vec3<f32>(0.3, 0.0, 0.0), texel.a);
    if (colour.a < CUTOUT_ALPHA) {{
        discard;
    }}
    return fogged(colour, input.distance);
}}

// The shadow's fragment: the sprite's texel times the vertex colour, no lighting — the
// source's quad carries no normals.
@fragment
fn fs_shadow(input: VertexOutput) -> @location(0) vec4<f32> {{
    let texel = textureSample(entity, entity_sampler, input.uv);
    let colour = vec4<f32>(texel.rgb * input.colour.rgb, texel.a * input.colour.a);
    return fogged(colour, input.distance);
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{CameraPose, DEFAULT_FOV, NEAR_PLANE, NO_VIEW_EFFECT};
    use glam::Vec3;

    /// A camera at the origin facing north (down -z), where the view rotation is the
    /// identity.
    fn north_camera() -> Camera {
        Camera {
            pose: CameraPose {
                position: [0.0, 0.0, 0.0],
                yaw: 180.0,
                pitch: 0.0,
            },
            fov_degrees: DEFAULT_FOV,
            near: NEAR_PLANE,
            far_chunks: 8.0,
            view_effect: NO_VIEW_EFFECT,
        }
    }

    /// A player draw standing at the origin, lit at half brightness.
    fn player_draw() -> EntityDraw {
        EntityDraw {
            model: ModelRef::Player {
                slim: false,
                parts: entity_models::player::PARTS_ALL,
            },
            position: [0.0, 0.0, 0.0],
            body_yaw: 0.0,
            head_yaw: 0.0,
            head_pitch: 0.0,
            pose: entity_models::Pose::default(),
            texture: TextureRef::Named("entity/steve.png"),
            light: 0.5,
            hurt: 0.0,
            death: 0.0,
            health: None,
            extra: DrawExtra::None,
        }
    }

    #[test]
    fn the_shadow_blend_is_the_sources_alpha_over() {
        let blend = shadow_blend();
        assert_eq!(blend.color.src_factor, wgpu::BlendFactor::SrcAlpha);
        assert_eq!(blend.color.dst_factor, wgpu::BlendFactor::OneMinusSrcAlpha);
        assert_eq!(blend.alpha.src_factor, wgpu::BlendFactor::One);
        assert_eq!(blend.alpha.dst_factor, wgpu::BlendFactor::Zero);
    }

    #[test]
    fn a_plug_keeps_the_frame_uniform_144_bytes() {
        assert_eq!(FRAME_BYTES, 144);
        let uniform = FrameUniform::default();
        assert_eq!(uniform.to_bytes().len(), 144);
    }

    #[test]
    fn the_item_lights_stand_in_eye_space_and_rotate_with_the_camera() {
        let lights = entity_lights(Mat3::IDENTITY);
        let expected0 = Vec3::new(0.2, 1.0, -0.7).normalize();
        let expected1 = Vec3::new(-0.2, 1.0, 0.7).normalize();
        assert!((Vec3::from(lights[0]) - expected0).length() < 1.0e-6);
        assert!((Vec3::from(lights[1]) - expected1).length() < 1.0e-6);
        assert!((Vec3::from(lights[0]).length() - 1.0).abs() < 1.0e-6);
        // The north camera's rotation is the identity, so its lights stand as they are;
        // turning the camera a quarter turn swings them with it.
        let camera = north_camera();
        let north = entity_lights(Mat3::from_mat4(camera.view()));
        assert!((Vec3::from(north[0]) - expected0).length() < 1.0e-5);
        let turned = Camera {
            pose: CameraPose {
                yaw: 90.0,
                ..camera.pose
            },
            ..camera
        };
        let swung = entity_lights(Mat3::from_mat4(turned.view()));
        assert!((Vec3::from(swung[0]) - Vec3::from(north[0])).length() > 0.5);
    }

    #[test]
    fn the_shadow_quad_spans_the_block_and_fades_with_distance() {
        let draw = player_draw();
        let shadow = entity_models::shadow(draw.model);
        let (corners, uvs, alpha) = shadow_quad(&draw, shadow, [0.0, 0.0, 8.0]).unwrap();
        // The quad spans twice the class's shadow size around the entity, just above the
        // feet.
        assert_eq!(corners[0], [-0.5, SHADOW_LIFT, -0.5]);
        assert_eq!(corners[2], [0.5, SHADOW_LIFT, 0.5]);
        // The sprite runs backwards: the low corner reads one (`(x - minX) / 2f + 0.5`).
        assert_eq!(uvs[0], [1.0, 1.0]);
        assert_eq!(uvs[2], [0.0, 0.0]);
        // The alpha: the distance fade times the opacity, halved, times the feet's light.
        let expected = (1.0 - 8.0 / 256.0) * shadow[1] * 0.5 * draw.light;
        assert!((alpha - expected).abs() < 1.0e-6);
        // Out past the fade's reach, nothing draws.
        assert!(shadow_quad(&draw, shadow, [0.0, 0.0, 256.0]).is_none());
    }

    #[test]
    fn the_placeholder_is_the_atlases_checkerboard() {
        // The placeholder a missing named key resolves to is the assets crate's own missing
        // sprite bytes, so both passes show the same fallback.
        let placeholder = placeholder_image();
        assert_eq!(placeholder.width, 16);
        assert_eq!(placeholder.height, 16);
        assert_eq!(placeholder.rgba.len(), 16 * 16 * 4);
        assert_eq!(placeholder.rgba, missing_pixels());
    }

    /// Whether two corners agree to within a ten-thousandth of a block.
    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1.0e-4)
    }

    #[test]
    fn the_body_chain_composes_the_sources_turns_and_scales() {
        // The right arm's outer top corner, (-3, -2, -2) in model units: the 1/16 scale and
        // the drop land it at 1.53 blocks up, the (-1, -1, 1) flip turns the model's down
        // axis into the world's up one, and `180 - body_yaw` turns it.
        let draw = player_draw();
        let corner = body_chain(&draw).transform_point3(Vec3::new(-3.0, -2.0, -2.0));
        assert!(close(
            corner.into(),
            [-0.175_781_25, 1.530_761_7, 0.117_187_5]
        ));
        // A quarter turn of the body: the same corner swings a quarter turn about the
        // entity's own axis.
        let turned = EntityDraw {
            body_yaw: 90.0,
            ..player_draw()
        };
        let corner = body_chain(&turned).transform_point3(Vec3::new(-3.0, -2.0, -2.0));
        assert!(close(
            corner.into(),
            [-0.117_187_5, 1.530_761_7, -0.175_781_25]
        ));
    }

    #[test]
    fn the_death_tilt_quarters_the_model_at_a_full_ramp() {
        // `RendererLivingEntity.rotateCorpse` turns three quarters of the ramp's 90 degrees
        // about Z after the yaw turn; at one the model lies on its side.
        let draw = EntityDraw {
            death: 1.0,
            ..player_draw()
        };
        let top = body_chain(&draw).transform_point3(Vec3::new(0.0, -8.0, 0.0));
        assert!(close(top.into(), [1.882_324_2, 0.0, 0.0]));
        // The ramp's quarter point: a quarter of the turn, the head a quarter over.
        let quarter = EntityDraw {
            death: 0.25,
            ..player_draw()
        };
        let top = body_chain(&quarter).transform_point3(Vec3::new(0.0, -8.0, 0.0));
        let angle = (22.5_f32).to_radians();
        assert!(close(
            top.into(),
            [angle.sin() * 1.882_324_2, angle.cos() * 1.882_324_2, 0.0]
        ));
    }

    #[test]
    fn the_sneak_drop_and_lift_move_the_body_down() {
        let sketch = entity_models::Pose {
            sneak: true,
            ..entity_models::Pose::default()
        };
        let draw = EntityDraw {
            pose: sketch,
            ..player_draw()
        };
        // The drop comes off the position and the model's own lift pushes down again — the
        // model's local +y is the world's down after the flip.
        let feet = body_chain(&draw).transform_point3(Vec3::new(0.0, 24.0, 0.0));
        let plain = body_chain(&player_draw()).transform_point3(Vec3::new(0.0, 24.0, 0.0));
        assert!((feet[1] - (plain[1] - 0.125 - 0.1875)).abs() < 1.0e-4);
    }
}
