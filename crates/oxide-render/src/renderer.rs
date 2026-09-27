//! The GPU device, the window surface and the passes that fill the window.

use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use winit::dpi::PhysicalSize;
use winit::window::Window;

use crate::camera::Camera;
use crate::overlay::OverlayPass;
use crate::terrain::{ChunkMesh, SectionKey};
use crate::terrain_pass::{DEPTH_FORMAT, TerrainPass};

/// The sky colour the window is cleared to, as vanilla 1.8.9 clears it.
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
/// and the two passes that draw into the frame.
///
/// [`Renderer::render`] clears the window to [`SKY_COLOR`] and the depth buffer to the far
/// plane, draws the section meshes through the terrain pass and the debug overlay over them,
/// then presents the frame. A frame with no camera set draws no terrain: the clear is the
/// whole picture, which is what the M0 smoke run shows.
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
    /// The overlay pipeline, and the debug lines it draws.
    overlay: OverlayPass,
    /// The camera the next frame is drawn with, until a new one is set.
    camera: Option<Camera>,
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
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| capabilities.formats.first().copied())
            .ok_or(RendererError::NoSurfaceFormat)?;
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

        let terrain = TerrainPass::new(&device, format);
        let mut overlay = OverlayPass::new(&device, format);
        overlay.set_size(&queue, config.width as f32, config.height as f32);
        let depth = DepthTarget::new(&device, config.width, config.height);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            adapter_info,
            depth,
            terrain,
            overlay,
            camera: None,
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
    /// The mesh is uploaded to the GPU as it is, and an empty mesh removes the section's mesh
    /// instead, so a section that stopped drawing costs nothing. Task 11 calls this once per
    /// section of every column the session rebuilds.
    pub fn set_section_mesh(&mut self, key: SectionKey, mesh: Option<&ChunkMesh>) {
        match mesh {
            Some(mesh) => self.terrain.upload(&self.device, &self.queue, key, mesh),
            None => self.terrain.remove(key),
        }
    }

    /// Sets the camera for the next frame.
    ///
    /// The view-projection matrix is built in [`Renderer::render`] from the camera and the
    /// current surface aspect ratio, so a resize between this call and the frame cannot leave
    /// a stale projection behind.
    pub fn set_camera(&mut self, camera: Camera) {
        self.camera = Some(camera);
    }

    /// Sets the overlay lines drawn this frame; empty hides the overlay.
    ///
    /// The lines are laid out and uploaded on the call, so a frame draws exactly the lines the
    /// caller last set.
    pub fn set_overlay_lines(&mut self, lines: Vec<String>) {
        self.overlay.upload_text(&self.device, &self.queue, &lines);
    }

    /// Draws the frame and presents it.
    ///
    /// The colour and depth attachments are cleared in the terrain pass, which draws every
    /// section mesh in the table when a camera has been set; the overlay pass then draws the
    /// debug lines over the result, in a pass without a depth attachment, so no terrain can
    /// hide the text.
    pub fn render(&mut self) -> Result<(), RendererError> {
        let frame = self.surface.get_current_texture()?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        if let Some(camera) = self.camera {
            let aspect = self.config.width as f32 / self.config.height as f32;
            self.terrain.set_camera(&self.queue, camera, aspect);
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("oxide-render encoder"),
            });
        {
            let mut terrain_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("oxide-render terrain pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(SKY_COLOR),
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
            if self.camera.is_some() {
                self.terrain.draw(&mut terrain_pass);
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
            self.overlay.draw(&mut overlay_pass);
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
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

    use super::{SurfaceAction, block_on, classify_surface_error};

    #[test]
    fn block_on_returns_the_output_of_a_ready_future() {
        assert_eq!(block_on(async { 7_u32 + 1 }), 8);
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
