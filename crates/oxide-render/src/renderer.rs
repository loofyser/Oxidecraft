//! The GPU device, the window surface and the passes that fill the window.

use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use winit::dpi::PhysicalSize;
use winit::window::Window;

use oxide_assets::atlas::Atlas;
use oxide_assets::font::FontError;
use oxide_assets::texture::Texture;

use crate::camera::Camera;
use crate::dim_pass::DimPass;
use crate::entity_pass::{EntityDraw, EntityPass, TextureRegistry};
use crate::fog::FogParams;
use crate::overlay::OverlayPass;
use crate::sky::{
    CloudPass, SkyParams, SkyPass, SkyTextures, cloud_at_or_above_layer, cloud_under_layer,
};
use crate::terrain::{ChunkMesh, SectionKey};
use crate::terrain_pass::{DEPTH_FORMAT, TerrainPass};
use crate::world_overlay::{Crack, Outline, WorldOverlay};

/// One draw of the scene pass, in the order the source issues it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SceneDraw {
    /// The sky pass, the frame's first draw (`EntityRenderer.java:1351-1354`).
    Sky,
    /// The cloud layer through the source's under-layer arm, before the terrain
    /// (`EntityRenderer.java:1364-1367`).
    CloudsUnder,
    /// The terrain's solid layers — the opaque layer and both cutouts — in the source's own
    /// order (`EntityRenderer.java:1386-1390`).
    Terrain,
    /// The entity pass, drawn between the terrain's solid and translucent layers
    /// (`EntityRenderer.java:1402`).
    Entities,
    /// The terrain's translucent layer, after the entities (`EntityRenderer.java:1467`).
    TerrainTranslucent,
    /// The world overlay: the aimed block's outline and the destroy-stage crack, the source's
    /// `"outline"` and `"destroyProgress"` sections after the terrain and before the clouds'
    /// at-or-above arm (`EntityRenderer.java:1412-1416`, `:1431-1437`).
    WorldOverlay,
    /// The cloud layer through the source's at-or-above arm, after the translucent layer
    /// (`EntityRenderer.java:1474-1478`).
    CloudsAtOrAbove,
}

/// The scene pass's draws for a camera, in the order the source issues them.
///
/// The sky draws first (`EntityRenderer.java:1351-1354`), then the terrain's layers
/// (`:1385-1396`), then the world overlay — the outline and the damage texture, the source's
/// `"outline"` and `"destroyProgress"` sections (`:1412-1416`, `:1431-1437`). The cloud layer
/// draws exactly once; the entity eye's height picks the arm and with it the cloud's place in
/// the order — the under-arm before the terrain while the eye is under the layer
/// (`:1364-1367`), the at-or-above arm after the overlay once it is at or above it
/// (`:1474-1478`).
fn scene_draws(camera: &Camera) -> Vec<SceneDraw> {
    let mut draws = vec![SceneDraw::Sky];
    if cloud_under_layer(camera) {
        draws.push(SceneDraw::CloudsUnder);
    }
    draws.push(SceneDraw::Terrain);
    draws.push(SceneDraw::Entities);
    draws.push(SceneDraw::TerrainTranslucent);
    draws.push(SceneDraw::WorldOverlay);
    if cloud_at_or_above_layer(camera) {
        draws.push(SceneDraw::CloudsAtOrAbove);
    }
    draws
}

/// The sky colour the window is cleared to when the frame has no fog, as vanilla 1.8.9 clears
/// it.
///
/// The first frame of a session has no fog to clear to — nothing has computed the world's own
/// colour yet — and a session without an asset store has no world at all, so the clear is this
/// constant until [`Renderer::set_fog`] gives the frame its colour.
pub const SKY_COLOR: wgpu::Color = wgpu::Color {
    r: 0.62,
    g: 0.76,
    b: 0.98,
    a: 1.0,
};

/// Something failed while setting up or driving the GPU.
#[derive(Debug, thiserror::Error)]
pub enum RendererError {
    /// The window surface could not be created.
    #[error("the window surface could not be created")]
    Surface(#[from] wgpu::CreateSurfaceError),
    /// No adapter could be opened for the requested backends.
    #[error("no usable GPU adapter was found")]
    NoAdapter(#[source] wgpu::RequestAdapterError),
    /// The adapter reports no format the surface can be configured with.
    #[error("the adapter offers no surface format")]
    NoSurfaceFormat,
    /// The logical device and its queue could not be created.
    #[error("the GPU device could not be created")]
    NoDevice(#[source] wgpu::RequestDeviceError),
    /// The next frame could not be acquired or presented.
    #[error("the next frame could not be acquired")]
    Frame(#[from] wgpu::SurfaceError),
}

/// What the render loop must do after the next frame could not be acquired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceAction {
    /// Reconfigure the surface from its stored configuration and retry the frame once.
    Reconfigure,
    /// Drop this frame and continue; the surface stays usable.
    SkipFrame,
    /// Stop: the error leaves the surface unusable.
    Fatal,
}

/// Classifies a [`wgpu::SurfaceError`] into the action the render loop must take.
///
/// `Outdated` and `Lost` mean the surface no longer matches the window, or the driver dropped
/// it; reconfiguring from the stored configuration makes it current again. `Timeout` means the
/// frame was not ready in time, which a hidden window produces, so dropping the frame is enough.
/// `OutOfMemory` and every other error are fatal.
pub fn classify_surface_error(error: &wgpu::SurfaceError) -> SurfaceAction {
    match error {
        wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost => SurfaceAction::Reconfigure,
        wgpu::SurfaceError::Timeout => SurfaceAction::SkipFrame,
        _ => SurfaceAction::Fatal,
    }
}

/// Owns the GPU objects for one window: the surface, the device, the queue, the depth texture
/// and the passes that draw into the frame.
///
/// [`Renderer::render`] clears the window to the frame's fog colour — the sky the terrain fades
/// towards, so the two agree where the terrain ends — and the depth buffer to the far plane,
/// draws the sky through the sky pass, the cloud layer, the section meshes through the terrain
/// pass, the world overlay — the aim's outline and the destroy-stage crack — and the debug
/// overlay over them, then presents the frame. The cloud layer draws exactly
/// once, and the entity eye's height places it: the source's under-arm before the terrain while
/// the eye is under the layer (`EntityRenderer.java:1364-1367`) and its at-or-above arm after
/// the translucent layer once the eye is at or above it (`:1474-1478`); [`scene_draws`] is the
/// order.
/// Until [`Renderer::set_fog`] gives a frame its fog, the clear is [`SKY_COLOR`], and until
/// [`Renderer::set_sky`] gives it its sky only the clear colour stands in for it. A frame
/// with no camera set draws no terrain, sky or clouds — and neither does one with no atlas: the
/// clear is the whole picture, which is what the M0 smoke run shows and what a session that has
/// not loaded an asset store yet draws.
pub struct Renderer {
    /// The presentable surface attached to the window.
    surface: wgpu::Surface<'static>,
    /// The logical device every command is submitted to.
    device: wgpu::Device,
    /// The queue command buffers are submitted on.
    queue: wgpu::Queue,
    /// The surface configuration, kept so a resize can reconfigure the surface.
    config: wgpu::SurfaceConfiguration,
    /// What the driver reports about the chosen adapter.
    adapter_info: wgpu::AdapterInfo,
    /// The depth texture the terrain pass tests and writes.
    depth: DepthTarget,
    /// The terrain pipeline, and the section meshes it draws.
    terrain: TerrainPass,
    /// The world overlay: the aimed block's outline and the destroy-stage crack, drawn in the
    /// scene pass after the terrain (`EntityRenderer.java:1412-1416`, `:1431-1437`).
    world_overlay: WorldOverlay,
    /// The sky pass, drawing the band, the void, the sun, the moon and the stars.
    sky: SkyPass,
    /// The cloud pass, drawing the flat layer under the camera.
    cloud: CloudPass,
    /// The overlay pipeline, and the debug lines it draws.
    overlay: OverlayPass,
    /// The dim quad, drawn over the scene before the overlay text.
    dim: DimPass,
    /// The entity pass: the boxes and the shadows drawn between the terrain's solid and
    /// translucent layers.
    entity_pass: EntityPass,
    /// The entity textures: the named sprites and the skins the pass samples, and the
    /// resolver the later tab list shares.
    entity_textures: TextureRegistry,
    /// The entities the next frame draws, as the window last set them.
    entities: Vec<EntityDraw>,
    /// The camera the next frame is drawn with, until a new one is set.
    camera: Option<Camera>,
    /// The fog the next frames are drawn and cleared with, until a new one is set.
    fog: Option<FogParams>,
}

impl Renderer {
    /// Creates the device for `window` and configures its surface.
    ///
    /// On Linux the instance asks for Vulkan only, because the measurement environment requires
    /// an explicit device choice and two GPUs are present; elsewhere the primary backends are
    /// requested. The chosen adapter, its driver and the surface format are logged.
    pub fn new(window: &Arc<Window>) -> Result<Self, RendererError> {
        let backends = if cfg!(target_os = "linux") {
            wgpu::Backends::VULKAN
        } else {
            wgpu::Backends::PRIMARY
        };
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });
        // The surface holds a handle on the window itself, so it outlives this borrow.
        let surface = instance.create_surface(Arc::clone(window))?;
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }))
        .map_err(RendererError::NoAdapter)?;

        let adapter_info = adapter.get_info();
        tracing::info!(
            adapter = %adapter_info.name,
            backend = ?adapter_info.backend,
            driver = %adapter_info.driver,
            driver_info = %adapter_info.driver_info,
            device_type = ?adapter_info.device_type,
            vendor_id = adapter_info.vendor,
            device_id = adapter_info.device,
            "GPU adapter selected"
        );

        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("oxide-render device"),
            ..Default::default()
        }))
        .map_err(RendererError::NoDevice)?;

        let capabilities = surface.get_capabilities(&adapter);
        // The surface is configured with a format that is *not* sRGB. The client's whole
        // fragment chain — the atlas texels, the vertex colours and the lightmap — is written
        // in the space its bytes describe, with no transfer function anywhere
        // (`docs/DIVERGENCES.md` records the policy and `docs/specs/oxidecraft-v1-design.md`
        // §C.1 the colour space), so a linear-to-sRGB conversion at the swapchain would brighten
        // the whole picture against the reference. `Bgra8Unorm` is preferred where the surface
        // offers it, then the first format that is not sRGB, and a surface that offers nothing
        // else keeps its first format with a warning: there the conversion cannot be avoided.
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| *format == wgpu::TextureFormat::Bgra8Unorm)
            .or_else(|| {
                capabilities
                    .formats
                    .iter()
                    .copied()
                    .find(|format| !format.is_srgb())
            })
            .or_else(|| capabilities.formats.first().copied())
            .ok_or(RendererError::NoSurfaceFormat)?;
        if format.is_srgb() {
            tracing::warn!(
                ?format,
                "the surface offers no format that is not sRGB; the window converts the client's \
                 colour space and the picture will read brighter than the reference"
            );
        }
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: capabilities
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: Vec::new(),
        };
        surface.configure(&device, &config);
        tracing::info!(
            ?format,
            srgb = format.is_srgb(),
            width = config.width,
            height = config.height,
            present_mode = ?config.present_mode,
            "surface configured"
        );

        let terrain = TerrainPass::new(&device, &queue, format);
        let world_overlay = WorldOverlay::new(&device, format);
        let sky = SkyPass::new(&device, &queue, format);
        let cloud = CloudPass::new(&device, &queue, format);
        let mut overlay = OverlayPass::new(&device, format);
        overlay.set_size(&queue, config.width as f32, config.height as f32);
        let dim = DimPass::new(&device, format);
        let depth = DepthTarget::new(&device, config.width, config.height);
        let entity_textures = TextureRegistry::new(&device, &queue);
        let entity_pass = EntityPass::new(&device, &queue, format, entity_textures.layout());

        Ok(Self {
            surface,
            device,
            queue,
            config,
            adapter_info,
            depth,
            terrain,
            world_overlay,
            sky,
            cloud,
            overlay,
            dim,
            entity_pass,
            entity_textures,
            entities: Vec::new(),
            camera: None,
            fog: None,
        })
    }

    /// The name of the chosen adapter, as its driver reports it.
    pub fn adapter_name(&self) -> &str {
        &self.adapter_info.name
    }

    /// Reconfigures the surface after the window changed size.
    ///
    /// A zero size, as a hidden or minimised window reports, is ignored: a surface cannot be
    /// configured with one.
    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.reconfigure();
    }

    /// Reconfigures the surface from the stored configuration.
    ///
    /// Call after the surface reported that it went stale: `Outdated` and `Lost` mean it no
    /// longer matches the window, or the driver dropped it, and reconfiguring makes the next
    /// frame's acquisition succeed. When the size changed, the depth texture and the overlay's
    /// projection are rebuilt too, because both belong to the surface size.
    pub fn reconfigure(&mut self) {
        self.surface.configure(&self.device, &self.config);
        if self.depth.width != self.config.width || self.depth.height != self.config.height {
            self.depth = DepthTarget::new(&self.device, self.config.width, self.config.height);
            self.overlay.set_size(
                &self.queue,
                self.config.width as f32,
                self.config.height as f32,
            );
        }
    }

    /// Replaces the mesh for a section; `None` removes it.
    ///
    /// Each non-empty layer of the mesh is uploaded on its own and a layer the mesh draws
    /// nothing in is removed, so a section whose water vanished frees its translucent buffers
    /// too. An empty mesh removes the section's mesh instead, so a section that stopped
    /// drawing costs nothing. Task 11 calls this once per section of every column the session
    /// rebuilds.
    pub fn set_section_mesh(&mut self, key: SectionKey, mesh: Option<&ChunkMesh>) {
        match mesh {
            Some(mesh) => self.terrain.upload(&self.device, &self.queue, key, mesh),
            None => self.terrain.remove(key),
        }
    }

    /// Uploads the block atlas the terrain draws with.
    ///
    /// The atlas is uploaded once and bound for every frame that follows; calling this again
    /// replaces it, dropping the old texture and its bind group. The world overlay binds the
    /// same atlas through its own level-0 pair, and resolves its ten destroy-stage sprites
    /// from it, so one call serves both passes. Until an atlas is set the terrain pass issues
    /// no draw calls and the overlay draws no crack, so a session that has no asset store yet
    /// — the shipping client sets one from its bootstrap — still renders its clear colour
    /// rather than sampling an unbound texture. M6 re-uploads here when an animated sprite
    /// advances.
    pub fn set_atlas(&mut self, atlas: &Atlas) {
        self.terrain.set_atlas(&self.device, &self.queue, atlas);
        self.world_overlay
            .set_atlas(&self.device, &self.queue, atlas);
    }

    /// Uploads the ascii font sheet the debug overlay draws with.
    ///
    /// The overlay measures the sheet as it uploads it, so the layout's widths and the
    /// sampled texels cannot disagree; until this is called the overlay draws nothing. A
    /// sheet that is not a 16x16 grid is [`FontError`] and the previous font stays.
    pub fn set_font(&mut self, sheet: &Texture) -> Result<(), FontError> {
        self.overlay.set_font(&self.device, &self.queue, sheet)
    }

    /// Rewrites the terrain lightmap for a sky brightness.
    ///
    /// The terrain pass is built with the lightmap at the noon brightness and the default
    /// gamma ([`crate::lightmap::lightmap_image`]); a live client calls this when the
    /// session's clock moves the sun, so the terrain's light follows it. M6 owns the
    /// brightness interpolation between the clock's two samples.
    pub fn set_lightmap(&mut self, sun_brightness: f32) {
        self.terrain.set_lightmap(&self.queue, sun_brightness);
    }

    /// Sets the entities the following frames draw; empty draws none.
    ///
    /// The window lays every draw out against the frame it last saw — interpolated positions
    /// and angles, the poses and the textures — so a frame draws exactly the entities the
    /// caller last set.
    pub fn set_entities(&mut self, entities: Vec<EntityDraw>) {
        self.entities = entities;
    }

    /// Uploads a named entity texture, replacing whatever the key held.
    ///
    /// The client uploads an entity sprite under the key its draws name it by; a key no draw
    /// names costs nothing, and a draw naming a key with nothing behind it samples the
    /// placeholder.
    pub fn set_entity_texture(&mut self, key: &'static str, texture: &Texture) {
        self.entity_textures
            .set_named(&self.device, &self.queue, key, texture);
    }

    /// Uploads the two default skins: the wide fallback and the slim one.
    ///
    /// A profile whose skin the client never resolved draws the default its own arm width
    /// names; until this is called those draws sample the placeholder.
    pub fn set_default_skins(&mut self, wide: &Texture, slim: &Texture) {
        self.entity_textures
            .set_defaults(&self.device, &self.queue, wide, slim);
    }

    /// Uploads one profile's skin and cape, replacing the profile's entry whole.
    ///
    /// A re-upload replaces both textures; a missing cape clears one. A profile with no skin
    /// yet keeps the default and may still carry a cape.
    pub fn set_skin(&mut self, uuid: &str, skin: Option<&Texture>, cape: Option<&Texture>) {
        self.entity_textures
            .set_skin(&self.device, &self.queue, uuid, skin, cape);
    }

    /// The entity textures and their skin resolver.
    ///
    /// The registry is the reader's window into what the entity pass samples; it resolves a
    /// profile to the texture its draws and, later, the tab list's head draws use. Section
    /// `TabList.java`'s reader shares this resolver through [`crate::entity_pass::SkinLookup`].
    pub fn entity_textures(&self) -> &TextureRegistry {
        &self.entity_textures
    }

    /// Sets the camera for the next frame.
    ///
    /// The view-projection matrix is built in [`Renderer::render`] from the camera and the
    /// current surface aspect ratio, so a resize between this call and the frame cannot leave
    /// a stale projection behind.
    pub fn set_camera(&mut self, camera: Camera) {
        self.camera = Some(camera);
    }

    /// Sets the aimed block's outline the following frames draw, or clears it.
    ///
    /// The outline is the source's `"outline"` section (`EntityRenderer.java:1412-1416`),
    /// drawn in the scene pass after the terrain; the caller wraps the aim's block cell and
    /// its shape box. `None` — the state of a frame whose ray meets no block — draws no
    /// outline.
    pub fn set_outline(&mut self, outline: Option<Outline>) {
        self.world_overlay.set_outline(outline);
    }

    /// Sets the breaking blocks the crack draws, replacing any earlier set.
    ///
    /// The crack is the source's `"destroyProgress"` section (`EntityRenderer.java:1431-1437`),
    /// drawn in the scene pass after the terrain; one entry per tracked destroy stage. An
    /// empty set draws no crack, and the overlay itself drops entries beyond the source's
    /// 32-block reach.
    pub fn set_cracks(&mut self, cracks: Vec<Crack>) {
        self.world_overlay.set_cracks(cracks);
    }

    /// Uploads the three environment textures the sky and the clouds sample, replacing any
    /// earlier set.
    ///
    /// The sun and the moon phase sheet go to the sky pass, the cloud texture to the cloud pass.
    /// Until this is called the sky draws its band, its void and its stars, and the clouds draw
    /// nothing: the sun and the moon and the layer itself need their textures. Task 14's
    /// bootstrap calls this once its texture set is loaded.
    pub fn set_sky_textures(&mut self, textures: SkyTextures) {
        self.sky.set_textures(
            &self.device,
            &self.queue,
            &textures.sun,
            &textures.moon_phases,
        );
        self.cloud
            .set_texture(&self.device, &self.queue, &textures.clouds);
    }

    /// Sets the sky the following frames draw and the cloud layer they tint.
    ///
    /// The parameters are the session's clock-derived values plus the client's own fog colour,
    /// far plane and cloud counter; see [`crate::sky::SkyParams`]. The cloud counter changes
    /// every frame in M2, so this is called per frame.
    pub fn set_sky(&mut self, params: SkyParams) {
        self.sky.set_params(&self.queue, params);
        self.cloud.set_params(&self.queue, params);
    }

    /// Sets the fog every following frame is drawn and cleared with.
    ///
    /// The colour and the range go to the terrain pass and the cloud pass, which mix them into
    /// every fragment they draw, and the same colour becomes the clear the sky behind the
    /// terrain is drawn in, so the two agree where the terrain ends. What the colour is for a
    /// given world is the caller's step — [`crate::fog::fog_colour`] produces one for a
    /// dimension, a time and an eye position, and [`crate::fog::linear_params`] the range for a
    /// far plane — and the sky pass's own range comes from [`SkyParams`]. Until this is called
    /// the frames clear to [`SKY_COLOR`] and draw no terrain fog at all.
    pub fn set_fog(&mut self, params: FogParams) {
        self.terrain.set_fog(&self.queue, params);
        self.cloud.set_fog(&self.queue, params);
        self.entity_pass.set_fog(params);
        self.fog = Some(params);
    }

    /// Sets the overlay lines drawn this frame; empty hides the overlay.
    ///
    /// The lines are laid out and uploaded on the call, so a frame draws exactly the lines the
    /// caller last set.
    pub fn set_overlay_lines(&mut self, lines: Vec<String>) {
        self.overlay.upload_text(&self.device, &self.queue, &lines);
    }

    /// Sets the full-frame tint the next frames draw over the scene, or clears it.
    ///
    /// The interim death view's backdrop: a colour here dims the whole frame
    /// — blended so the scene shows through — under the overlay text. `None`,
    /// the state every frame of a live player draws in, issues nothing.
    pub fn set_dim(&mut self, colour: Option<[f32; 4]>) {
        self.dim.set_colour(&self.queue, colour);
    }

    /// Forgets every section mesh the terrain holds, freeing their buffers.
    ///
    /// The world those meshes were built from is gone — a respawn across
    /// dimensions replaces it whole — so the next frame draws none of them
    /// until the session reports fresh columns.
    pub fn clear_section_meshes(&mut self) {
        self.terrain.clear_meshes();
    }

    /// Draws the frame and presents it.
    ///
    /// The colour and depth attachments are cleared in the frame's single scene pass: the colour
    /// to the frame's fog colour, so the sky behind the terrain is the colour the terrain fades
    /// to, and the depth to the far plane. The sky pass draws first over that clear, then every
    /// section mesh the frame's frustum keeps when a camera and an atlas have both been set, and
    /// the cloud layer once — through the source's under-layer arm before the terrain while the
    /// entity eye is under the layer, or through its at-or-above arm after the terrain once the
    /// eye is at or above it (`EntityRenderer.java:1364-1367`, `:1474-1478`; [`scene_draws`]).
    /// The overlay pass then draws the debug lines over the result, in a pass without a depth
    /// attachment, so no terrain can hide the text.
    pub fn render(&mut self) -> Result<(), RendererError> {
        let frame = self.surface.get_current_texture()?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        if let Some(camera) = self.camera {
            let aspect = self.config.width as f32 / self.config.height as f32;
            self.terrain.set_camera(&self.queue, camera, aspect);
            self.sky.set_camera(&self.queue, camera, aspect);
            self.cloud.set_camera(&self.queue, camera, aspect);
            self.entity_pass.set_camera(camera, aspect);
            // The overlay's frame: the outline's pixel width and the crack's projection both
            // follow the surface size, so the frame is built from the same configuration the
            // render pass is.
            self.world_overlay.set_frame(
                &self.device,
                &self.queue,
                &camera,
                [self.config.width as f32, self.config.height as f32],
            );
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("oxide-render encoder"),
            });
        let clear = clear_colour(self.fog);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("oxide-render scene pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        // The depth buffer is not read after the pass.
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if let Some(camera) = self.camera {
                for draw in scene_draws(&camera) {
                    match draw {
                        SceneDraw::Sky => self.sky.draw(&mut pass),
                        SceneDraw::CloudsUnder | SceneDraw::CloudsAtOrAbove => {
                            self.cloud.draw(&mut pass);
                        }
                        SceneDraw::Terrain => self.terrain.draw_solid(&mut pass),
                        SceneDraw::Entities => self.entity_pass.draw(
                            &self.device,
                            &mut pass,
                            &self.entities,
                            &self.entity_textures,
                        ),
                        SceneDraw::TerrainTranslucent => self.terrain.draw_translucent(&mut pass),
                        SceneDraw::WorldOverlay => self.world_overlay.draw(&mut pass),
                    }
                }
            }
        }
        {
            let mut overlay_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("oxide-render overlay pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.dim.draw(&mut overlay_pass);
            self.overlay.draw(&mut overlay_pass);
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }
}

/// The colour the window is cleared to: the frame's fog colour when one is set, so the sky
/// behind the terrain is the colour the terrain fades towards, and [`SKY_COLOR`] otherwise.
///
/// The fog colour's three components are the shader's, handed to the clear as they are; the
/// alpha is opaque, because the surface is.
fn clear_colour(fog: Option<FogParams>) -> wgpu::Color {
    match fog {
        Some(params) => wgpu::Color {
            r: params.colour[0] as f64,
            g: params.colour[1] as f64,
            b: params.colour[2] as f64,
            a: 1.0,
        },
        None => SKY_COLOR,
    }
}

/// The depth texture the terrain pass tests and writes, with the size it was built for.
///
/// The render pass borrows a view of it, and a `TextureView` keeps its texture alive, so the
/// texture handle itself is not stored.
struct DepthTarget {
    /// A view of the whole texture, as the pass's depth attachment needs.
    view: wgpu::TextureView,
    /// The width in texels the texture was built for.
    width: u32,
    /// The height in texels the texture was built for.
    height: u32,
}

impl DepthTarget {
    /// Creates a depth texture of `width` by `height` texels in [`DEPTH_FORMAT`].
    ///
    /// Every drawable surface is at least one texel across, so a zero size, which a hidden
    /// window reports before the caller filters it, becomes one texel rather than an invalid
    /// texture.
    fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("oxide-render depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            view,
            width,
            height,
        }
    }
}

/// Blocks the calling thread until `future` resolves.
///
/// The adapter and device requests are the only futures this crate waits on, and both finish
/// after a driver round trip, so a waker that unparks the thread is enough; it keeps the crate
/// free of an async runtime.
fn block_on<F: Future>(future: F) -> F::Output {
    /// Wakes the waiting thread by unparking it.
    struct Unpark(std::thread::Thread);

    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests of the blocking helper and of the surface-error classification; the GPU paths
    //! need a device and a window.

    use wgpu::SurfaceError;

    use super::{
        SKY_COLOR, SceneDraw, SurfaceAction, block_on, classify_surface_error, clear_colour,
        scene_draws,
    };
    use crate::camera::{Camera, CameraPose, DEFAULT_FOV, NEAR_PLANE, NO_VIEW_EFFECT};
    use crate::fog::{FogParams, fog_colour};

    /// A camera at `feet_y` with no facing, for the scene order's own tests.
    fn scene_camera(feet_y: f64) -> Camera {
        Camera {
            pose: CameraPose {
                position: [0.5, feet_y, 0.5],
                yaw: 0.0,
                pitch: 0.0,
            },
            fov_degrees: DEFAULT_FOV,
            near: NEAR_PLANE,
            far_chunks: 8.0,
            view_effect: NO_VIEW_EFFECT,
        }
    }

    #[test]
    fn the_scene_pass_draws_the_cloud_arm_the_entity_eye_selects() {
        // The source draws the cloud layer twice and gates each arm on the entity eye
        // (`entity.posY + entity.getEyeHeight()`): the under-arm before the terrain while the
        // eye is under the layer (`EntityRenderer.java:1364-1367`) and the at-or-above arm
        // after the translucent layer once the eye is at or above it (`:1474-1478`). The
        // acceptance's mark pose stands at feet 150.0 — eye 151.62 — so its frame carries the
        // second arm; the wall pose's feet 57.0 leaves the eye under the layer. The entities
        // draw between the solid and translucent terrain layers (`:1402` vs `:1386-1390` and
        // `:1467`).
        assert_eq!(
            scene_draws(&scene_camera(57.0)),
            [
                SceneDraw::Sky,
                SceneDraw::CloudsUnder,
                SceneDraw::Terrain,
                SceneDraw::Entities,
                SceneDraw::TerrainTranslucent,
                SceneDraw::WorldOverlay
            ]
        );
        assert_eq!(
            scene_draws(&scene_camera(150.0)),
            [
                SceneDraw::Sky,
                SceneDraw::Terrain,
                SceneDraw::Entities,
                SceneDraw::TerrainTranslucent,
                SceneDraw::WorldOverlay,
                SceneDraw::CloudsAtOrAbove
            ]
        );
    }

    #[test]
    fn block_on_returns_the_output_of_a_ready_future() {
        assert_eq!(block_on(async { 7_u32 + 1 }), 8);
    }

    #[test]
    fn a_frame_with_no_fog_clears_to_the_sky_colour() {
        assert_eq!(clear_colour(None), SKY_COLOR);
    }

    #[test]
    fn a_frame_with_a_fog_clears_to_the_fogs_own_colour() {
        // The Overworld's colour at noon, the eye on the ground and the render distance at its
        // thirty-two-chunk maximum, where the sky mix contributes nothing and the full-light
        // brightness factor is one (`fog_colour`'s own tests pin the steps).
        let colour = fog_colour(0, 6000.0, 64.0, 0.03125, [0.4, 0.6, 0.8], 32, 15);
        let clear = clear_colour(Some(FogParams {
            colour,
            start: 24.0,
            end: 32.0,
            far_plane: 32.0,
        }));
        assert!((clear.r - f64::from(colour[0])).abs() < 1e-9);
        assert!((clear.g - f64::from(colour[1])).abs() < 1e-9);
        assert!((clear.b - f64::from(colour[2])).abs() < 1e-9);
        assert_eq!(clear.a, 1.0, "the surface is opaque");
    }

    #[test]
    fn a_stale_surface_is_reconfigured_and_the_frame_retried() {
        assert_eq!(
            classify_surface_error(&SurfaceError::Outdated),
            SurfaceAction::Reconfigure
        );
        assert_eq!(
            classify_surface_error(&SurfaceError::Lost),
            SurfaceAction::Reconfigure
        );
    }

    #[test]
    fn a_timeout_skips_the_frame() {
        assert_eq!(
            classify_surface_error(&SurfaceError::Timeout),
            SurfaceAction::SkipFrame
        );
    }

    #[test]
    fn out_of_memory_and_generic_errors_are_fatal() {
        assert_eq!(
            classify_surface_error(&SurfaceError::OutOfMemory),
            SurfaceAction::Fatal
        );
        assert_eq!(
            classify_surface_error(&SurfaceError::Other),
            SurfaceAction::Fatal
        );
    }
}
