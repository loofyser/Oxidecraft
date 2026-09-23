//! Local-only smoke test of the GPU path: it needs a real adapter, so it is ignored by
//! default. Run it on a machine with a GPU:
//!
//! ```text
//! cargo test -p oxide-render --test headless -- --ignored --nocapture
//! ```
//!
//! It clears an offscreen target to the sky colour and reads the pixel back, which proves the
//! instance, adapter, device and clear pass work before a window is involved.

use std::sync::mpsc;
use std::task::{Context, Poll, Wake, Waker};

use oxide_render::renderer::SKY_COLOR;

/// The clear colour as 8-bit unorm bytes: 0.62, 0.76 and 0.98 of 255, rounded.
const EXPECTED: [u8; 3] = [158, 194, 250];

#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn clears_an_offscreen_target_to_the_sky_colour() {
    let backends = if cfg!(target_os = "linux") {
        wgpu::Backends::VULKAN
    } else {
        wgpu::Backends::PRIMARY
    };
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends,
        ..Default::default()
    });
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("an adapter is available");
    let info = adapter.get_info();
    println!(
        "adapter: {} ({:?}, {} {}, device type {:?})",
        info.name, info.backend, info.driver, info.driver_info, info.device_type
    );
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("oxide headless device"),
        ..Default::default()
    }))
    .expect("a device is created");

    const SIZE: u32 = 4;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("oxide headless target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide headless encoder"),
    });
    {
        let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("oxide headless clear pass"),
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

    // A readback row must be a multiple of 256 bytes, so the buffer is padded.
    let bytes_per_row = 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("oxide headless readback"),
        size: u64::from(bytes_per_row) * u64::from(SIZE),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(SIZE),
            },
        },
        wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = buffer.slice(..);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device.poll(wgpu::PollType::Wait).expect("the device polls");
    receiver
        .recv()
        .expect("the map callback runs")
        .expect("the buffer maps");
    let pixel = {
        let mapped = slice.get_mapped_range();
        [mapped[0], mapped[1], mapped[2]]
    };
    buffer.unmap();

    for (channel, (&got, &want)) in pixel.iter().zip(EXPECTED.iter()).enumerate() {
        assert!(
            (i16::from(got) - i16::from(want)).abs() <= 2,
            "channel {channel}: got {got}, want {want}"
        );
    }
}

/// Blocks the calling thread until `future` resolves; the test has no async runtime.
fn block_on<F: Future>(future: F) -> F::Output {
    /// Wakes the waiting thread by unparking it.
    struct Unpark(std::thread::Thread);

    impl Wake for Unpark {
        fn wake(self: std::sync::Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(std::sync::Arc::new(Unpark(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}
