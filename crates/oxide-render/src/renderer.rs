//! The GPU device, the window surface and the clear pass that fills the window.

use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use winit::dpi::PhysicalSize;
use winit::window::Window;

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

/// Owns the GPU objects for one window: the surface, the device, the queue and the clear pass.
///
/// A clear pass issues no draw calls, so it needs no pipeline; the passes that draw geometry
/// arrive with the terrain work. [`Renderer::render`] clears the window to [`SKY_COLOR`] and
/// presents it.
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

        Ok(Self {
            surface,
            device,
            queue,
            config,
            adapter_info,
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
    /// frame's acquisition succeed.
    pub fn reconfigure(&mut self) {
        self.surface.configure(&self.device, &self.config);
    }

    /// Clears the window to the sky colour and presents the frame.
    pub fn render(&mut self) -> Result<(), RendererError> {
        let frame = self.surface.get_current_texture()?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("oxide-render encoder"),
            });
        {
            let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("oxide-render clear pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(SKY_COLOR),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
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
