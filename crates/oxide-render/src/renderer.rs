//! The GPU device, the window surface and the passes that fill the window.

use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use winit::dpi::PhysicalSize;
use winit::window::Window;

use oxide_assets::atlas::Atlas;
use oxide_assets::font::FontError;
use oxide_assets::texture::Texture;

use crate::camera::Camera;
use crate::camera::{DEFAULT_FOV, FOV_RANGE};
use crate::capture::{CapturedFrame, capture_texture};
use crate::dim_pass::DimPass;
use crate::entity_pass::{BossStatus, EntityDraw, EntityPass, ItemMeshSource, TextureRegistry};
use crate::fog::FogParams;
use crate::gui_item::ItemIconSource;
use crate::held_item::{HeldItemFrame, HeldItemPass};
use crate::hud::{HudDraw, HudPass, ScaledResolution, projection_size, scaled_resolution};
use crate::lightmap::gamma_for_brightness;
use crate::overlay::OverlayPass;
use crate::particle_pass::{ParticleDraw, ParticlePass};
use crate::sign_text::{SignTextEntry, SignTextPass};
use crate::sky::{
    CLOUD_FANCY, CloudPass, SkyParams, SkyPass, SkyTextures, cloud_at_or_above_layer,
    cloud_layer_draws, cloud_under_layer,
};
use crate::terrain::{ChunkMesh, SectionKey};
use crate::terrain_pass::{DEPTH_FORMAT, TerrainPass};
use crate::weather_pass::{WeatherColumn, WeatherPass};
use crate::world_overlay::{Crack, Hitbox, Outline, WorldOverlay};

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
    /// The signs' front-face text, after the entities: the depth test hides
    /// text behind blocks, and the text itself writes no depth.
    SignText,
    /// The particle quads, after the entities' text and before the weather:
    /// the source's `renderParticles` arm between the two
    /// (`EffectRenderer.java:222+`, `EntityRenderer.java:1594+`).
    Particles,
    /// The rain/snow ribbons, after the particles and before the translucent
    /// terrain (`EntityRenderer.java:1594+`).
    Weather,
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

/// The scene pass's draws for a camera and a cloud setting, in the order the source
/// issues them.
///
/// The sky draws first (`EntityRenderer.java:1351-1354`), then the terrain's layers
/// (`:1385-1396`), then the world overlay — the outline and the damage texture, the source's
/// `"outline"` and `"destroyProgress"` sections (`:1412-1416`, `:1431-1437`). The particle
/// quads draw after the entities' text and the weather ribbons after the particles, both
/// before the translucent layer (`EffectRenderer.java:222+`, `EntityRenderer.java:1594+`).
/// The cloud layer
/// draws exactly once when the setting keeps it — both of the source's arms gate on
/// `shouldRenderClouds() != 0` (`:1490-1493`) — and the entity eye's height picks the arm
/// and with it the cloud's place in the order — the under-arm before the terrain while
/// the eye is under the layer (`:1364-1367`), the at-or-above arm after the overlay once
/// it is at or above it (`:1474-1478`).
fn scene_draws(camera: &Camera, clouds: u8) -> Vec<SceneDraw> {
    let draw_clouds = cloud_layer_draws(clouds);
    let mut draws = vec![SceneDraw::Sky];
    if draw_clouds && cloud_under_layer(camera) {
        draws.push(SceneDraw::CloudsUnder);
    }
    draws.push(SceneDraw::Terrain);
    draws.push(SceneDraw::Entities);
    draws.push(SceneDraw::SignText);
    draws.push(SceneDraw::Particles);
    draws.push(SceneDraw::Weather);
    draws.push(SceneDraw::TerrainTranslucent);
    draws.push(SceneDraw::WorldOverlay);
    if draw_clouds && cloud_at_or_above_layer(camera) {
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

/// The surface present mode for a vertical-sync switch.
///
/// On means the source's `Display.setVSyncEnabled(true)` (`GameSettings.java:491-492`):
/// `Fifo`, which every surface supports. Off means the no-vsync fallback — the first
/// mode the surface offers of the (`Mailbox`, `Immediate`) pair, `Fifo` when it offers
/// neither.
pub fn present_mode_for_vsync(on: bool, offered: &[wgpu::PresentMode]) -> wgpu::PresentMode {
    if on {
        return wgpu::PresentMode::Fifo;
    }
    for mode in [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate] {
        if offered.contains(&mode) {
            return mode;
        }
    }
    wgpu::PresentMode::Fifo
}

/// The stored field-of-view for a setting in degrees: clamped into
/// [`FOV_RANGE`], the projection base [`Renderer::set_fov`] keeps.
fn clamp_fov(degrees: u8) -> f32 {
    f32::from(degrees.clamp(FOV_RANGE.0, FOV_RANGE.1))
}

/// The hud/screen projection for a surface size and a gui-scale setting.
///
/// The one re-derive the resize path, the reconfigure path and [`Renderer::set_gui_scale`]
/// share: the scaled resolution of [`scaled_resolution`] for the size and setting, and the
/// fractional projection of [`projection_size`] for its factor — the doubles the HUD/screen
/// ortho builds on, not the ceil ints the draw lists lay out in.
fn hud_projection_size(display_width: u32, display_height: u32, gui_scale: u8) -> (f64, f64) {
    let scaled = scaled_resolution(display_width, display_height, gui_scale);
    projection_size(display_width, display_height, scaled.scale_factor)
}

/// Owns the GPU objects for one window: the surface, the device, the queue, the depth texture
/// and the passes that draw into the frame.
///
/// [`Renderer::render`] clears the window to the frame's fog colour — the sky the terrain fades
/// towards, so the two agree where the terrain ends — and the depth buffer to the far plane,
/// draws the sky through the sky pass, the cloud layer, the section meshes through the terrain
/// pass, the world overlay — the aim's outline and the destroy-stage crack — and the GUI over
/// them: the dim quad, the boss bar, the hud's draw list and the debug overlay, then presents
/// the frame. The cloud layer draws exactly
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
    /// The hud pass, drawing the frame's GUI-space draw list between the dim and the
    /// debug overlay text.
    hud: HudPass,
    /// The screen pass, drawing the open screen's GUI-space draw list after the
    /// whole HUD/overlay group: the source draws the HUD overlay first
    /// (`EntityRenderer.java`:1166-1170), then `currentScreen.drawScreen`
    /// after a depth clear (`:1185-1191`). The port's `hud` list sits inside
    /// the overlay group, so the screen needs its own group above it.
    screen: HudPass,
    /// The first-person held item pass, drawing the frame's selected stack in camera
    /// space between the scene pass and the overlay pass — the source's own order: the
    /// hand draws inside the world pass after the terrain and the entities
    /// (`EntityRenderer.java`:864-876), before the GUI's own pass clears depth.
    held_item: HeldItemPass,
    /// The gui-scale setting the hud's scaled resolution reads: zero is the source's
    /// auto scale (`ScaledResolution.java`:22-25). The settings screen that owns the
    /// option is a later milestone's, so the default stands.
    gui_scale: u8,
    /// The brightness setting in percent of the source's gamma slider: zero is Moody,
    /// the field's own default (`GameSettings.java:171`). The terrain lightmap is built
    /// with [`gamma_for_brightness`](crate::lightmap::gamma_for_brightness) of this.
    brightness: u8,
    /// The sun brightness the lightmap was last rewritten for: `World.getSunBrightness`
    /// at the frame's celestial angle, noon's 1.0 until the clock moves it. Kept so a
    /// brightness change rewrites the same sun, and a clock change the same gamma.
    sun_brightness: f32,
    /// The cloud setting: 0 is Off, 1 Fast, 2 Fancy (`CLOUDS_TYPES`,
    /// `GameSettings.java:64`; the field default is 2, `:74`). The scene pass draws no
    /// cloud arm while this is Off.
    clouds: u8,
    /// Whether the graphics setting is Fast (`fancyGraphics`, `GameSettings.java:75`;
    /// the field default is fancy, so this starts false). The mesher owns the flag's
    /// effect — its leaves' layer and occlusion read it — which the client feeds at its
    /// next mesh build; the renderer only carries the value.
    graphics_fast: bool,
    /// The smooth-lighting setting: 0 is Off, 1 Minimum, 2 Maximum
    /// (`AMBIENT_OCCLUSIONS`, `GameSettings.java:59`; the field default is 2, `:78`).
    /// The mesher owns the flag's effect — its ambient-occlusion path — which the client
    /// feeds at its next mesh build; the renderer only carries the value.
    smooth_lighting: u8,
    /// The field-of-view setting in degrees: the frame's projection base
    /// (`FOV`, `GameSettings.java`), default [`DEFAULT_FOV`](crate::camera::DEFAULT_FOV).
    /// The client reads it back for the frame's smoothed FOV, so the stored
    /// setting governs the projection the next frame builds.
    fov_degrees: f32,
    /// Whether vertical sync is on (`enableVsync`, `GameSettings.java:88`; the field
    /// default is on). The surface runs [`wgpu::PresentMode::Fifo`] while this is set.
    vsync: bool,
    /// The present modes the surface offers, kept so [`Renderer::set_vsync`] can pick
    /// the no-vsync fallback without asking the adapter again.
    supported_present_modes: Vec<wgpu::PresentMode>,
    /// The entity pass: the boxes and the shadows drawn between the terrain's solid and
    /// translucent layers.
    entity_pass: EntityPass,
    /// The entity textures: the named sprites and the skins the pass samples, and the
    /// resolver the later tab list shares.
    entity_textures: TextureRegistry,
    /// The entities the next frame draws, as the window last set them.
    entities: Vec<EntityDraw>,
    /// The sign-text pass: the map's entries as board-fixed quads, drawn
    /// after the entities.
    sign_pass: SignTextPass,
    /// The weather pass: the session's rain/snow columns as camera-facing
    /// ribbons, drawn after the particles and before the translucent terrain.
    weather: WeatherPass,
    /// The particle pass: the session's frames as camera-facing quads, drawn
    /// after the entities' text and before the weather.
    particles: ParticlePass,
    /// The boss bar's status cell: the last raised status and the frames left on it,
    /// `None` while the bar is hidden. A frame whose pass raises a wither's or a dragon's
    /// status overwrites it; every other frame's step spends one of its hundred frames
    /// ([`boss_status_step`]).
    boss_status: Option<BossStatus>,
    /// The camera the next frame is drawn with, until a new one is set.
    camera: Option<Camera>,
    /// The fog the next frames are drawn and cleared with, until a new one is set.
    fog: Option<FogParams>,
    /// The finished frame, kept for the screenshot readback: every pass draws
    /// into this texture and the present blits it to the surface, so the
    /// capture reads the presented pixels without rendering twice — the
    /// source's readback shape (`ScreenShotHelper.java:60-83`). Recreated
    /// when the surface size moves; empty until the first frame.
    retained: Option<RetainedFrame>,
}

/// The finished frame the screenshot readback copies from: the texture every
/// pass draws into, at the surface size and format the present blits.
struct RetainedFrame {
    /// The texture itself.
    texture: wgpu::Texture,
    /// Its width in texels.
    width: u32,
    /// Its height in texels.
    height: u32,
}

/// Registers one GUI sheet on the GUI pass pair: on both. The screen pass keeps its
/// own registry beside the hud's, so the bootstrap must fill both — a screen draw
/// naming a sheet only the hud holds is skipped, silently, at the batch's bind
/// (`hud.rs`'s unbound-texture skip). That was the container panel's own bail: the
/// background blits (and the builtin chest icons) named sheets the screen pass never
/// received, with zero texture errors in the log.
pub fn bind_gui_sheet(
    hud: &mut HudPass,
    screen: &mut HudPass,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    name: &'static str,
    texture: &Texture,
) {
    hud.set_texture(device, queue, name, texture);
    screen.set_texture(device, queue, name, texture);
}

/// Uploads the block atlas's level-0 icon binding on the GUI pass pair: on both. The
/// icons' meshes sample it, so until both passes hold it the hud's and the screen's
/// [`HudDraw::Item`] batches are skipped, silently, at the batch's bind. That was the
/// shared item-sprite bail — the container slots' and the hotbar strip's alike.
pub fn bind_gui_atlas(
    hud: &mut HudPass,
    screen: &mut HudPass,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    atlas: &Atlas,
) {
    hud.set_atlas_icon(device, queue, atlas);
    screen.set_atlas_icon(device, queue, atlas);
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
        let supported_present_modes = capabilities.present_modes.clone();
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
            // The present blits the retained frame into the surface texture
            // with a plain copy, so the surface carries `COPY_DST` beside the
            // render attachment bit.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
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
        let mut hud = HudPass::new(&device, &queue, format);
        let (projection_width, projection_height) =
            hud_projection_size(config.width, config.height, 0);
        hud.set_resolution(&queue, projection_width as f32, projection_height as f32);
        let mut screen = HudPass::new(&device, &queue, format);
        screen.set_resolution(&queue, projection_width as f32, projection_height as f32);
        let held_item = HeldItemPass::new(&device, format);
        let depth = DepthTarget::new(&device, config.width, config.height);
        let entity_textures = TextureRegistry::new(&device, &queue);
        let entity_pass = EntityPass::new(&device, &queue, format, entity_textures.layout());
        let sign_pass = SignTextPass::new(&device, format);
        let weather = WeatherPass::new(&device, &queue, format);
        let particles = ParticlePass::new(&device, &queue, format);

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
            hud,
            screen,
            held_item,
            gui_scale: 0,
            brightness: 0,
            sun_brightness: 1.0,
            clouds: CLOUD_FANCY,
            graphics_fast: false,
            smooth_lighting: 2,
            fov_degrees: DEFAULT_FOV,
            vsync: true,
            supported_present_modes,
            entity_pass,
            entity_textures,
            entities: Vec::new(),
            sign_pass,
            weather,
            particles,
            boss_status: None,
            camera: None,
            fog: None,
            retained: None,
        })
    }

    /// The name of the chosen adapter, as its driver reports it.
    pub fn adapter_name(&self) -> &str {
        &self.adapter_info.name
    }

    /// The adapter's driver and driver info, the overlay's `Display:`-tail
    /// equivalent of the source's `GL_VERSION` row.
    pub fn adapter_driver(&self) -> String {
        format!(
            "{} {}",
            self.adapter_info.driver, self.adapter_info.driver_info
        )
    }

    /// The adapter's backend name, the overlay's `Display:`-parenthesis fact
    /// (`Vulkan`, `Dx12`, …): the wgpu backend behind the surface the frame
    /// draws on.
    pub fn adapter_backend(&self) -> String {
        format!("{:?}", self.adapter_info.backend)
    }

    /// The debug overlay's `C:` census: the sections holding a mesh and the
    /// distinct columns they belong to, straight from the terrain pass.
    pub fn mesh_census(&self) -> (u32, u32) {
        self.terrain.mesh_census()
    }

    /// Reconfigures the surface after the window changed size, emitting the recomputed
    /// scaled resolution.
    ///
    /// A zero size, as a hidden or minimised window reports, is ignored: a surface cannot be
    /// configured with one, and the current resolution is emitted unchanged. The caller —
    /// the client's pointer scaling reads [`Renderer::scaled_resolution`] — lays its GUI
    /// draws out in the emitted units.
    pub fn resize(&mut self, size: PhysicalSize<u32>) -> ScaledResolution {
        if size.width == 0 || size.height == 0 {
            return self.scaled_resolution();
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.reconfigure();
        self.scaled_resolution()
    }

    /// Reconfigures the surface from the stored configuration.
    ///
    /// Call after the surface reported that it went stale: `Outdated` and `Lost` mean it no
    /// longer matches the window, or the driver dropped it, and reconfiguring makes the next
    /// frame's acquisition succeed. When the size changed, the depth texture, the overlay's
    /// projection and the hud's scaled-resolution projection are rebuilt too, because all three
    /// belong to the surface size.
    pub fn reconfigure(&mut self) {
        self.surface.configure(&self.device, &self.config);
        if self.depth.width != self.config.width || self.depth.height != self.config.height {
            self.depth = DepthTarget::new(&self.device, self.config.width, self.config.height);
            self.overlay.set_size(
                &self.queue,
                self.config.width as f32,
                self.config.height as f32,
            );
            let (projection_width, projection_height) =
                hud_projection_size(self.config.width, self.config.height, self.gui_scale);
            self.hud.set_resolution(
                &self.queue,
                projection_width as f32,
                projection_height as f32,
            );
            self.screen.set_resolution(
                &self.queue,
                projection_width as f32,
                projection_height as f32,
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
    ///
    /// The same call binds the atlas's level-0 icon view on both GUI passes
    /// ([`bind_gui_atlas`]): the item icons' meshes sample it, so until this call the
    /// hud's and the screen's [`HudDraw::Item`] draws contribute nothing.
    pub fn set_atlas(&mut self, atlas: &Atlas) {
        self.terrain.set_atlas(&self.device, &self.queue, atlas);
        self.particles.set_atlas(&self.device, &self.queue, atlas);
        self.world_overlay
            .set_atlas(&self.device, &self.queue, atlas);
        bind_gui_atlas(
            &mut self.hud,
            &mut self.screen,
            &self.device,
            &self.queue,
            atlas,
        );
    }

    /// Uploads the ascii font sheet the debug overlay, the entity pass's nametags and the hud
    /// draw with.
    ///
    /// Every pass measures the sheet as it uploads it, so the layout's widths and the sampled
    /// texels cannot disagree; until this is called the overlay draws nothing, a nametag has
    /// no glyphs to measure and the hud's text draws contribute nothing. A sheet that is not
    /// a 16x16 grid is [`FontError`] and the previous font stays on every pass.
    pub fn set_font(&mut self, sheet: &Texture) -> Result<(), FontError> {
        self.entity_pass
            .set_font(&self.device, &self.queue, sheet)?;
        self.hud.set_font(&self.device, &self.queue, sheet)?;
        self.screen.set_font(&self.device, &self.queue, sheet)?;
        self.sign_pass.set_font(&self.device, &self.queue, sheet)?;
        self.overlay.set_font(&self.device, &self.queue, sheet)
    }

    /// Sets the gui-scale setting the hud's scaled resolution reads.
    ///
    /// Zero is the source's auto scale (`ScaledResolution.java`:22-25) and any other value
    /// caps the factor there (`:27-30`). The projection is rebuilt from the new resolution,
    /// so the next frame's draw list lays out against it.
    pub fn set_gui_scale(&mut self, gui_scale: u8) {
        self.gui_scale = gui_scale;
        let (projection_width, projection_height) =
            hud_projection_size(self.config.width, self.config.height, self.gui_scale);
        self.hud.set_resolution(
            &self.queue,
            projection_width as f32,
            projection_height as f32,
        );
        self.screen.set_resolution(
            &self.queue,
            projection_width as f32,
            projection_height as f32,
        );
    }

    /// The scaled resolution the hud draws at, for the window's own draw assembly.
    ///
    /// The same call the hud's projection is built from: the window lays its GUI draws out
    /// in these units.
    pub fn scaled_resolution(&self) -> ScaledResolution {
        scaled_resolution(self.config.width, self.config.height, self.gui_scale)
    }

    /// Rewrites the terrain lightmap for a sky brightness.
    ///
    /// The terrain pass is built with the lightmap at the noon brightness and the default
    /// gamma ([`crate::lightmap::lightmap_image`]); a live client calls this when the
    /// session's clock moves the sun, so the terrain's light follows it. The rewrite keeps
    /// the stored brightness setting's gamma, so a clock change never resets the slider.
    pub fn set_lightmap(&mut self, sun_brightness: f32) {
        self.sun_brightness = sun_brightness;
        let gamma = gamma_for_brightness(self.brightness);
        self.terrain
            .set_lightmap(&self.queue, sun_brightness, gamma);
        self.weather
            .set_lightmap(&self.queue, sun_brightness, gamma);
        self.particles
            .set_lightmap(&self.queue, sun_brightness, gamma);
    }

    /// Sets the brightness setting and rewrites the terrain lightmap for it.
    ///
    /// `mode` is percent of the source's gamma slider, 0 Moody to 100 bright
    /// (`GameSettings.java:171`; past-the-end values clamp to the top). The rewrite keeps
    /// the stored sun brightness, so a slider change never resets the clock.
    pub fn set_brightness(&mut self, mode: u8) {
        self.brightness = mode.min(crate::lightmap::BRIGHTNESS_MAX);
        let gamma = gamma_for_brightness(self.brightness);
        self.terrain
            .set_lightmap(&self.queue, self.sun_brightness, gamma);
        self.weather
            .set_lightmap(&self.queue, self.sun_brightness, gamma);
        self.particles
            .set_lightmap(&self.queue, self.sun_brightness, gamma);
    }

    /// Sets the cloud setting: 0 is Off, 1 Fast, 2 Fancy (`CLOUDS_TYPES`,
    /// `GameSettings.java:64`; past-the-end values clamp to Fancy).
    ///
    /// The parameter is the primitive the client maps its `CloudSetting` through
    /// (`as_u8`), so this crate takes no edge on `oxide-game`. Off drops both cloud arms
    /// from the scene pass ([`scene_draws`]); Fast and Fancy share the flat layer until
    /// the fancy geometry lands.
    pub fn set_clouds(&mut self, setting: u8) {
        self.clouds = setting.min(CLOUD_FANCY);
    }

    /// Sets the graphics setting: set means the Fast path.
    ///
    /// The parameter is the primitive the client maps its `GraphicsSetting` through, so
    /// this crate takes no edge on `oxide-game`. The source rebuilds its renderers on the
    /// toggle (`GameSettings.java:412-416`); here the flag is carried for the mesher,
    /// which the client feeds at its next mesh build.
    pub fn set_graphics(&mut self, fast: bool) {
        self.graphics_fast = fast;
    }

    /// Whether the graphics setting is Fast, as [`Renderer::set_graphics`] last set it.
    pub fn graphics_fast(&self) -> bool {
        self.graphics_fast
    }

    /// Sets the smooth-lighting setting: 0 is Off, 1 Minimum, 2 Maximum
    /// (`AMBIENT_OCCLUSIONS`, `GameSettings.java:59`; past-the-end values clamp to
    /// Maximum).
    ///
    /// The parameter is the primitive the client maps its stored value through, so this
    /// crate takes no edge on `oxide-game`. The source rebuilds its renderers on the
    /// toggle (`GameSettings.java:418-422`); here the value is carried for the mesher's
    /// ambient-occlusion path, which the client feeds at its next mesh build.
    pub fn set_smooth_lighting(&mut self, mode: u8) {
        self.smooth_lighting = mode.min(2);
    }

    /// The smooth-lighting setting, as [`Renderer::set_smooth_lighting`] last set it.
    pub fn smooth_lighting(&self) -> u8 {
        self.smooth_lighting
    }

    /// Sets the field-of-view setting in degrees: the frame's projection base.
    ///
    /// The value clamps into [`FOV_RANGE`]. The client reads the stored
    /// setting back for the frame's smoothed FOV, so the next frame's
    /// projection builds from it.
    pub fn set_fov(&mut self, degrees: u8) {
        self.fov_degrees = clamp_fov(degrees);
    }

    /// The field-of-view setting in degrees, as [`Renderer::set_fov`] last
    /// set it.
    pub fn fov_degrees(&self) -> f32 {
        self.fov_degrees
    }

    /// Switches vertical sync: on runs the surface at `Fifo`, off at the no-vsync
    /// fallback ([`present_mode_for_vsync`]).
    ///
    /// The source flips `enableVsync` straight onto the display
    /// (`GameSettings.java:491-492`); here the pick reconfigures the surface from its
    /// stored configuration, so the next frame presents under the new mode.
    pub fn set_vsync(&mut self, on: bool) {
        self.vsync = on;
        self.config.present_mode = present_mode_for_vsync(on, &self.supported_present_modes);
        self.surface.configure(&self.device, &self.config);
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

    /// Sets the source the object draws build their item meshes from.
    ///
    /// The client owns the atlas, the baked block models and the item sheets; the pass asks
    /// this source for the generated item shapes, the block items' baked states, the item
    /// frame's wood and the icon quads. Until this is called, the object draws' item paths
    /// draw nothing.
    pub fn set_item_source(&mut self, source: Arc<dyn ItemMeshSource>) {
        self.entity_pass.set_item_source(source);
    }

    /// Sets the icon source the entity pass's equipment layers resolve their stacks
    /// through: the same resolver the hud's item draws take, so a held item and a GUI
    /// icon read one seam. Until this is called, the held item draws nothing.
    pub fn set_entity_icon_source(&mut self, source: Arc<dyn ItemIconSource>) {
        self.entity_pass.set_icon_source(source);
    }

    /// Hands the entity pass the frame's system time, in milliseconds: the held item's
    /// glint scroll reads it (`Minecraft.getSystemTime`); the armour's own glint scrolls
    /// with the entity's age and reads none of it.
    pub fn set_entity_system_time(&mut self, time_ms: u64) {
        self.entity_pass.set_system_time(time_ms);
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

    /// Sets the hitboxes the line path draws, replacing any earlier set.
    ///
    /// The hitboxes are F3+B's entity boxes (`RenderManager.java:438-468`), drawn in the
    /// scene pass after the terrain; the caller hands over the flag-gated frames, so an
    /// empty set — flag off, or nothing with a box in view — draws no hitbox.
    pub fn set_hitboxes(&mut self, hitboxes: Vec<Hitbox>) {
        self.world_overlay.set_hitboxes(hitboxes);
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
    /// every fragment they draw, and the sky pass takes the same frame's fog for its own
    /// fogged surfaces — the linear range on air frames, the water arm's EXP density when
    /// the eye is submerged — while the same colour becomes the clear the sky behind the
    /// terrain is drawn in, so the two agree where the terrain ends. What the colour is for a
    /// given world is the caller's step — [`crate::fog::fog_colour`] produces one for a
    /// dimension, a time and an eye position, and [`crate::fog::linear_params`] the range for a
    /// far plane — and the sky pass's own range comes from [`SkyParams`]. Until this is called
    /// the frames clear to [`SKY_COLOR`] and draw no terrain fog at all.
    pub fn set_fog(&mut self, params: FogParams) {
        self.terrain.set_fog(&self.queue, params);
        self.cloud.set_fog(&self.queue, params);
        self.sky.set_fog(&self.queue, params);
        self.weather.set_fog(&self.queue, params);
        self.particles.set_fog(&self.queue, params);
        self.entity_pass.set_fog(params);
        self.fog = Some(params);
    }

    /// Sets the sign entries the world draws; empty draws nothing.
    ///
    /// The entries are laid out and uploaded on the call, so a frame draws
    /// exactly the map the caller last set — nothing else, so positions
    /// outside the map draw no text (the port's no-text rule).
    pub fn set_sign_texts(&mut self, entries: Vec<SignTextEntry>) {
        self.sign_pass.upload(&self.device, &self.queue, &entries);
    }

    /// Sets the particle quads the next frames draw; empty draws nothing.
    ///
    /// The quads are built and uploaded on the call, so a frame draws exactly
    /// the draws the caller last set.
    pub fn set_particles(&mut self, draws: &[ParticleDraw]) {
        self.particles.upload(&self.device, &self.queue, draws);
    }

    /// Sets the weather columns the next frames draw, scrolled by `tick` and
    /// oriented around `viewer` — the floored feet block the session boxed
    /// the columns around.
    ///
    /// The ribbons are built and uploaded on the call, so a frame draws
    /// exactly the columns the caller last set.
    pub fn set_weather(&mut self, columns: &[WeatherColumn], tick: u32, viewer: [i32; 2]) {
        self.weather
            .upload(&self.device, &self.queue, columns, tick, viewer);
    }

    /// Uploads the `particles.png` cells sheet the particle pass's cell quads
    /// draw through.
    ///
    /// Until this has been called the cell batches draw nothing; a re-upload
    /// replaces the sheet whole.
    pub fn set_particle_cells(&mut self, cells: &Texture) {
        self.particles.set_cells(&self.device, &self.queue, cells);
    }

    /// Uploads the rain and snow sheets the weather pass's columns draw
    /// through.
    ///
    /// Until both have been set the arms whose sheet is missing draw nothing;
    /// a re-upload replaces the sheets whole.
    pub fn set_weather_sheets(&mut self, rain: &Texture, snow: &Texture) {
        self.weather
            .set_sheets(&self.device, &self.queue, rain, snow);
    }

    /// Sets the overlay columns drawn this frame: the left lines from the
    /// top-left origin and the right lines right-aligned against the
    /// surface's width, each over its own background quad; empty on both
    /// sides hides the overlay.
    ///
    /// The columns are laid out and uploaded on the call, so a frame draws exactly the lines the
    /// caller last set.
    pub fn set_overlay_lines(&mut self, left: Vec<String>, right: Vec<String>) {
        let width = self.config.width as f32;
        self.overlay
            .upload_text(&self.device, &self.queue, &left, &right, width);
    }

    /// Sets the hud draw list drawn this frame; empty draws nothing.
    ///
    /// The list is laid out and uploaded on the call, so a frame draws exactly the list the
    /// caller last set — the window assembles it from the chat and, later milestones, the
    /// rest of the GUI at [`Renderer::scaled_resolution`]'s units — and its skin draws
    /// resolve their ids against the entity texture registry, the same one the entity pass
    /// samples.
    pub fn set_hud(&mut self, draws: Vec<HudDraw>) {
        self.hud
            .set_draws(&self.device, &self.queue, &draws, &self.entity_textures);
    }

    /// Sets the screen draw list drawn this frame; empty draws nothing.
    ///
    /// The list draws after the whole HUD/overlay group — the source's own
    /// order (`EntityRenderer.java`:1166-1170, then `:1185-1191`) — so an
    /// open screen paints over the overlay, the debug lines included.
    pub fn set_screen(&mut self, draws: Vec<HudDraw>) {
        self.screen
            .set_draws(&self.device, &self.queue, &draws, &self.entity_textures);
    }

    /// Registers a hud texture under `name`, for the hud draws that sample a named
    /// texture; a later call under the same name replaces it, and a draw whose name
    /// never landed is skipped.
    pub fn set_hud_texture(&mut self, name: &'static str, texture: &Texture) {
        self.hud
            .set_texture(&self.device, &self.queue, name, texture);
    }

    /// Registers a screen texture under `name`, for the screen draws' sheets;
    /// the screen pass keeps its own registry beside the hud's.
    pub fn set_screen_texture(&mut self, name: &'static str, texture: &Texture) {
        self.screen
            .set_texture(&self.device, &self.queue, name, texture);
    }

    /// Registers one GUI sheet on both GUI passes, through the pair's shared
    /// rule ([`bind_gui_sheet`]): the container family's sheets, the book sheets,
    /// the SGA glyph sheet and the chest trio's icon sheets — every sheet a screen
    /// draw names, under the key its draw carries.
    pub fn set_gui_sheet(&mut self, name: &'static str, texture: &Texture) {
        bind_gui_sheet(
            &mut self.hud,
            &mut self.screen,
            &self.device,
            &self.queue,
            name,
            texture,
        );
    }

    /// Uploads the glint sheet the hud's enchanted icons' glint passes sample.
    ///
    /// The client owns the store's `misc/enchanted_item_glint` sheet; until this is
    /// called, an enchanted icon's glint batches are skipped and only the icon draws.
    pub fn set_hud_glint(&mut self, texture: &Texture) {
        self.hud.set_glint(&self.device, &self.queue, texture);
        self.screen.set_glint(&self.device, &self.queue, texture);
    }

    /// Sets the source the hud's item icons build their meshes and display transforms
    /// from.
    ///
    /// The client owns the bake and the item table; until this is called, the hud's
    /// [`HudDraw::Item`] draws contribute nothing.
    pub fn set_hud_icon_source(&mut self, source: Arc<dyn ItemIconSource>) {
        self.hud
            .set_icon_source(&self.device, &self.queue, source.clone());
        self.screen
            .set_icon_source(&self.device, &self.queue, source);
    }

    /// Hands the hud pass the frame's system time, in milliseconds, which the glint
    /// draws' scroll phases read (`Minecraft.getSystemTime`'s own clock).
    pub fn set_hud_system_time(&mut self, time_ms: u64) {
        self.hud.set_system_time(&self.device, &self.queue, time_ms);
        self.screen
            .set_system_time(&self.device, &self.queue, time_ms);
    }

    /// Sets the first-person held item frame the next draws show, or clears it with a
    /// frame whose stack is `None`.
    ///
    /// The client owns the frame: the stack through the ease's swap rule and the
    /// rendered equip and swing arguments. Until this is called, the hand draws
    /// nothing.
    pub fn set_held_item(&mut self, frame: &HeldItemFrame) {
        self.held_item.set_frame(&self.device, &self.queue, frame);
    }

    /// Uploads the blocks atlas the held item's meshes sample, under the source's own
    /// item sampler (`RenderItem.java`:319).
    pub fn set_held_item_atlas_icon(&mut self, atlas: &Atlas) {
        self.held_item
            .set_atlas_icon(&self.device, &self.queue, atlas);
    }

    /// Registers a named sheet a held item's mesh may name (the folded chest trio).
    pub fn set_held_item_texture(&mut self, name: &'static str, texture: &Texture) {
        self.held_item
            .set_texture(&self.device, &self.queue, name, texture);
    }

    /// Sets the source the held item's stack resolves its mesh and first-person display
    /// transform through — the same source the hud's icons use.
    pub fn set_held_item_icon_source(&mut self, source: Arc<dyn ItemIconSource>) {
        self.held_item.set_icon_source(source);
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
    /// The overlay pass then draws the frame's GUI over the result, with the depth buffer
    /// cleared and offered to the item draws alone — the source clears depth before the GUI
    /// (`EntityRenderer.setupOverlayRendering`:1752) and its icons test it while its 2D draws
    /// pass over it — so no terrain can hide the GUI: the dim quad first, then the boss bar,
    /// then the hud's draw list (its 2D batches and its depth-tested item icons), then the
    /// debug lines text.
    ///
    /// The boss bar's status cell advances once per frame on this call: a wither's or a
    /// dragon's raise from the entity pass overwrites it, an untouched status spends one of
    /// its hundred frames, and a spent one hides the bar ([`boss_status_step`]).
    pub fn render(&mut self) -> Result<(), RendererError> {
        let frame = self.surface.get_current_texture()?;
        self.ensure_retained();
        let Some(kept) = self.retained.as_ref() else {
            // Unreachable: the ensure above fills the texture. The frame is
            // skipped rather than presented without a captured twin.
            return Ok(());
        };
        let view = kept
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut raised: Option<BossStatus> = None;
        if let Some(camera) = self.camera {
            let aspect = self.config.width as f32 / self.config.height as f32;
            self.terrain.set_camera(&self.queue, camera, aspect);
            self.sky.set_camera(&self.queue, camera, aspect);
            self.cloud.set_camera(&self.queue, camera, aspect);
            self.entity_pass.set_camera(camera, aspect);
            self.sign_pass.set_camera(&self.queue, camera, aspect);
            self.weather.set_camera(&self.queue, camera, aspect);
            self.particles.set_camera(&self.queue, camera, aspect);
            // The held item's own camera: the hand's projection (`far * 2`, the view
            // effect alone) and the lights turned by the pose.
            self.held_item
                .set_camera(&self.device, &self.queue, &camera, aspect);
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
                for draw in scene_draws(&camera, self.clouds) {
                    match draw {
                        SceneDraw::Sky => self.sky.draw(&mut pass),
                        SceneDraw::CloudsUnder | SceneDraw::CloudsAtOrAbove => {
                            self.cloud.draw(&mut pass);
                        }
                        SceneDraw::Terrain => self.terrain.draw_solid(&mut pass),
                        SceneDraw::Entities => {
                            raised = self.entity_pass.draw(
                                &self.device,
                                &mut pass,
                                &self.entities,
                                &self.entity_textures,
                            );
                        }
                        SceneDraw::SignText => self.sign_pass.draw(&mut pass),
                        SceneDraw::Particles => self.particles.draw(&mut pass),
                        SceneDraw::Weather => self.weather.draw(&mut pass),
                        SceneDraw::TerrainTranslucent => self.terrain.draw_translucent(&mut pass),
                        SceneDraw::WorldOverlay => self.world_overlay.draw(&mut pass),
                    }
                }
            }
        }
        // The held item draws between the scene and the overlay, in its own pass: the
        // source's hand draws inside the world pass after the terrain and the entities
        // and before the GUI's own pass (`EntityRenderer.java`:864-876), the depth
        // buffer cleared immediately before it (`:1484`'s `clear(256)`). This pass
        // clears its own depth for the same shape, because the scene pass discards its
        // own — nothing stores it — so the hand tests against itself alone and no
        // terrain hides it, matching the source's own clear.
        if self.camera.is_some() {
            let mut held_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("oxide-render held item pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        // The overlay pass clears depth at its own load, so nothing
                        // reads the held item's depth after it.
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.held_item.draw(&mut held_pass);
        }
        // The status step runs after the scene pass: a raise overwrites the cell whole and
        // a frame without one spends a frame of the hold, hiding the bar at zero
        // (`GuiIngame.java`:903-905).
        let bar = boss_status_step(&mut self.boss_status, raised);
        let scaled = self.scaled_resolution();
        self.hud
            .set_boss_bar(&self.device, &self.queue, bar.as_ref(), &scaled);
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
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth.view,
                    depth_ops: Some(wgpu::Operations {
                        // The source clears depth before the GUI's own pass
                        // (`EntityRenderer.setupOverlayRendering`:1752) and nothing
                        // reads it after the frame's GUI is drawn.
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.dim.draw(&mut overlay_pass);
            self.hud.draw_boss_bar(&mut overlay_pass);
            self.hud.draw(&mut overlay_pass);
            self.overlay.draw(&mut overlay_pass);
            self.screen.draw(&mut overlay_pass);
        }
        // The present blits the retained frame to the surface: every pass drew
        // into the retained texture, so the presented pixels and the capture's
        // source are the same frame, rendered once.
        if let Some(kept) = self.retained.as_ref() {
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &kept.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &frame.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: self.config.width,
                    height: self.config.height,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }

    /// Reads the last presented frame back to CPU bytes, top-row-first — the
    /// PNG writer's own layout — or `None` before the first frame.
    ///
    /// The source is the retained texture the present blits from, never a
    /// second render; the presented frame already holds the HUD and any open
    /// screen, so the capture does too.
    pub fn capture_frame(&self) -> Option<CapturedFrame> {
        let kept = self.retained.as_ref()?;
        capture_texture(
            &self.device,
            &self.queue,
            &kept.texture,
            kept.width,
            kept.height,
            self.config.format,
        )
    }

    /// Creates the retained frame texture when it is missing or the surface
    /// size moved under it.
    fn ensure_retained(&mut self) {
        let fresh = match &self.retained {
            Some(kept) => kept.width != self.config.width || kept.height != self.config.height,
            None => true,
        };
        if !fresh {
            return;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("oxide-render retained frame"),
            size: wgpu::Extent3d {
                width: self.config.width,
                height: self.config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        self.retained = Some(RetainedFrame {
            texture,
            width: self.config.width,
            height: self.config.height,
        });
    }
}

/// One frame of the boss bar's status cell, and the status the frame draws with.
///
/// `raised` is the status the entity pass raised this frame, when a wither or a dragon
/// drew; it overwrites the cell whole, exactly as the source's static does. The frame's
/// gate then runs on the cell's value before the decrement (`GuiIngame.java`:903-905): a
/// status with frames left is returned for the frame's bar and spends one frame; a status
/// at zero is hidden and returns nothing, and a spent cell keeps returning nothing until a
/// fresh raise. A frame with neither a raise nor a cell draws no bar.
pub fn boss_status_step(
    cell: &mut Option<BossStatus>,
    raised: Option<BossStatus>,
) -> Option<BossStatus> {
    if let Some(status) = raised {
        *cell = Some(status);
    }
    let status = cell.as_mut()?;
    if status.time == 0 {
        return None;
    }
    let drawn = status.clone();
    status.time -= 1;
    Some(drawn)
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
        CLOUD_FANCY, SKY_COLOR, SceneDraw, SurfaceAction, block_on, boss_status_step, clamp_fov,
        classify_surface_error, clear_colour, hud_projection_size, present_mode_for_vsync,
        scene_draws,
    };
    use crate::camera::{Camera, CameraPose, DEFAULT_FOV, NEAR_PLANE, NO_VIEW_EFFECT};
    use crate::entity_pass::{BOSS_STATUS_TIME, BossStatus};
    use crate::fog::{FogMode, FogParams, fog_colour};
    use crate::sky::{CLOUD_FAST, CLOUD_OFF};

    /// A raised status at `fraction`, for the countdown's own tests.
    fn raised_boss(fraction: f32) -> BossStatus {
        BossStatus {
            name: "Wither".to_owned(),
            health_fraction: fraction,
            colour_modifier: true,
            time: BOSS_STATUS_TIME,
        }
    }

    #[test]
    fn the_boss_status_cell_holds_the_raise_for_a_hundred_frames() {
        // The source's gate runs before the decrement (`GuiIngame.java`:903-905), so the
        // raise's frame draws at the full hundred and the bar draws through f+99; the cell
        // is spent after that frame and f+100 hides. A post-decrement gate would draw one
        // frame short.
        let mut cell = None;
        let drawn =
            boss_status_step(&mut cell, Some(raised_boss(0.5))).expect("the raise's frame draws");
        assert_eq!(
            drawn.time, BOSS_STATUS_TIME,
            "the frame enters at the source's hundred"
        );
        assert_eq!(
            cell.as_ref().expect("the cell keeps the status").time,
            BOSS_STATUS_TIME - 1,
            "one frame spent"
        );
        for frame in 1..BOSS_STATUS_TIME {
            assert!(
                boss_status_step(&mut cell, None).is_some(),
                "frame f+{frame} draws"
            );
        }
        assert!(
            boss_status_step(&mut cell, None).is_none(),
            "f+100 hides the bar"
        );
        assert!(
            boss_status_step(&mut cell, None).is_none(),
            "and a spent cell stays hidden"
        );
    }

    #[test]
    fn the_boss_status_cell_persists_while_re_set() {
        // A boss that draws every frame re-sets the status every frame
        // (`BossStatus.java`:13), so the cell never runs out and every frame enters at
        // the full hundred.
        let mut cell = None;
        for frame in 0..250 {
            let drawn = boss_status_step(&mut cell, Some(raised_boss(0.5)))
                .unwrap_or_else(|| panic!("frame {frame} draws"));
            assert_eq!(drawn.time, BOSS_STATUS_TIME);
        }
    }

    #[test]
    fn the_boss_status_cell_overwrites_whole() {
        // The raise replaces the cell's whole value — name and flag included — exactly as
        // the source's static does (`BossStatus.java`:10-16).
        let mut cell = None;
        assert!(boss_status_step(&mut cell, Some(raised_boss(0.5))).is_some());
        let dragon = BossStatus {
            name: "Ender Dragon".to_owned(),
            health_fraction: 1.0,
            colour_modifier: false,
            time: BOSS_STATUS_TIME,
        };
        let drawn = boss_status_step(&mut cell, Some(dragon)).expect("the dragon's frame draws");
        assert_eq!(drawn.name, "Ender Dragon");
        assert!(!drawn.colour_modifier);
        assert_eq!(drawn.health_fraction, 1.0);
    }

    /// A camera at `feet_y` with no facing, for the scene order's own tests.
    fn scene_camera(feet_y: f64) -> Camera {
        Camera {
            pose: CameraPose {
                position: [0.5, feet_y, 0.5],
                yaw: 0.0,
                pitch: 0.0,
                sneak: false,
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
            scene_draws(&scene_camera(57.0), CLOUD_FAST),
            [
                SceneDraw::Sky,
                SceneDraw::CloudsUnder,
                SceneDraw::Terrain,
                SceneDraw::Entities,
                SceneDraw::SignText,
                SceneDraw::Particles,
                SceneDraw::Weather,
                SceneDraw::TerrainTranslucent,
                SceneDraw::WorldOverlay
            ]
        );
        assert_eq!(
            scene_draws(&scene_camera(150.0), CLOUD_FAST),
            [
                SceneDraw::Sky,
                SceneDraw::Terrain,
                SceneDraw::Entities,
                SceneDraw::SignText,
                SceneDraw::Particles,
                SceneDraw::Weather,
                SceneDraw::TerrainTranslucent,
                SceneDraw::WorldOverlay,
                SceneDraw::CloudsAtOrAbove
            ]
        );
    }

    #[test]
    fn the_scene_pass_draws_no_cloud_arm_when_the_setting_is_off() {
        // Both source arms gate on the setting being non-zero
        // (`EntityRenderer.java:1490-1493`): Off leaves neither arm, whatever the eye
        // height, while Fancy keeps the eye-selected one like Fast.
        for feet_y in [57.0, 150.0] {
            let draws = scene_draws(&scene_camera(feet_y), CLOUD_OFF);
            assert!(
                !draws.contains(&SceneDraw::CloudsUnder),
                "no under-arm at feet {feet_y} while Off"
            );
            assert!(
                !draws.contains(&SceneDraw::CloudsAtOrAbove),
                "no at-or-above arm at feet {feet_y} while Off"
            );
        }
        assert!(
            scene_draws(&scene_camera(57.0), CLOUD_FANCY).contains(&SceneDraw::CloudsUnder),
            "Fancy keeps the under-arm under the layer"
        );
        assert!(
            scene_draws(&scene_camera(150.0), CLOUD_FANCY).contains(&SceneDraw::CloudsAtOrAbove),
            "Fancy keeps the at-or-above arm at the layer"
        );
    }

    #[test]
    fn the_stored_fov_clamps_into_the_slider_range() {
        // `FOV` (`GameSettings.java`: `30.0F, 110.0F`): the ends pin, the
        // out-of-range ends clamp, and the middle passes through.
        assert_eq!(clamp_fov(30), 30.0, "the low end pins");
        assert_eq!(clamp_fov(110), 110.0, "the high end pins");
        assert_eq!(clamp_fov(70), 70.0, "the default passes through");
        assert_eq!(clamp_fov(5), 30.0, "below the range clamps up");
        assert_eq!(clamp_fov(200), 110.0, "above the range clamps down");
    }

    #[test]
    fn the_present_mode_table_pins_the_vsync_switch() {
        // On is Fifo whatever the surface offers; off takes the fallback pair in order.
        let offered = [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate];
        assert_eq!(
            present_mode_for_vsync(true, &offered),
            wgpu::PresentMode::Fifo,
            "vsync on is Fifo"
        );
        assert_eq!(
            present_mode_for_vsync(true, &[]),
            wgpu::PresentMode::Fifo,
            "vsync on is Fifo with no offer"
        );
        assert_eq!(
            present_mode_for_vsync(false, &offered),
            wgpu::PresentMode::Mailbox,
            "vsync off prefers Mailbox"
        );
        assert_eq!(
            present_mode_for_vsync(false, &[wgpu::PresentMode::Immediate]),
            wgpu::PresentMode::Immediate,
            "vsync off falls back to Immediate"
        );
        assert_eq!(
            present_mode_for_vsync(false, &[wgpu::PresentMode::AutoVsync]),
            wgpu::PresentMode::Fifo,
            "vsync off with no fallback stays Fifo"
        );
    }

    #[test]
    fn the_resize_re_derive_matches_the_scaled_resolution() {
        // The one re-derive the resize, reconfigure and gui-scale paths share: the
        // fractional projection of the scaled resolution's own factor.
        let (width, height) = hud_projection_size(1280, 720, 0);
        assert!(
            (width - 1280.0 / 3.0).abs() < 1e-9,
            "the auto factor at 1280x720 is three, so the width is {width}"
        );
        assert!(
            (height - 240.0).abs() < 1e-9,
            "the auto factor at 1280x720 is three, so the height is {height}"
        );
        let (width, height) = hud_projection_size(1920, 1080, 2);
        assert_eq!(
            (width, height),
            (960.0, 540.0),
            "a fixed scale of two halves the display"
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
        let colour = fog_colour(0, 6000.0, 64.0, 0.03125, [0.4, 0.6, 0.8], 32, 15, 0.0, 0.0);
        let clear = clear_colour(Some(FogParams {
            colour,
            start: 24.0,
            end: 32.0,
            far_plane: 32.0,
            mode: FogMode::Linear,
            density: 0.0,
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
