//! Local-only test of the terrain and overlay pipelines: it needs a real adapter, so it is
//! ignored by default. Run it on a machine with a GPU:
//!
//! ```text
//! cargo test -p oxide-render --test pipeline_headless -- --ignored --nocapture
//! ```
//!
//! The first test renders one stone block's mesh from a camera looking down at it and reads
//! the pixels back: the centre must be the stone colour and a corner must still be the sky.
//! The mesh carries a buried face as well, wound the same way and indexed after the block's
//! faces, so a pipeline that lost its depth test, its depth write or its depth attachment
//! draws it over the top face and the centre turns the buried colour instead.
//!
//! The read-back pins the winding convention too: the faces are counter-clockwise seen from
//! outside and the pipeline culls back faces, so a face wound the other way, or a pipeline
//! that culls front faces instead, drops the top face and shows the far side of the block at
//! the centre — brightness 0.8 where the top face's 1.0 is wanted — and the colour check
//! fails. (Culling nothing at all would still draw the same picture, so `terrain_pass`'s unit
//! test pins the cull mode itself.)
//!
//! The second test renders the same block, then draws a line through the overlay pass over it
//! and checks the glyph pixels and the one-pixel shadow offset against the layout
//! `debug_text` produces.

use std::sync::mpsc;
use std::task::{Context, Poll, Wake, Waker};

use oxide_render::camera::{Camera, CameraPose, DEFAULT_FOV, EYE_HEIGHT, NEAR_PLANE};
use oxide_render::overlay::OverlayPass;
use oxide_render::renderer::SKY_COLOR;
use oxide_render::terrain::{ChunkMesh, Layer, Vertex};
use oxide_render::terrain_pass::{DEPTH_FORMAT, TerrainPass};

/// The size of the offscreen target in texels.
const SIZE: u32 = 64;
/// The readback row stride: a row must be a multiple of 256 bytes, and 64 RGBA texels are
/// exactly that.
const BYTES_PER_ROW: u32 = 256;
/// The sky colour as 8-bit unorm bytes: 0.62, 0.76 and 0.98 of 255, rounded.
const SKY: [u8; 3] = [158, 194, 250];
/// The stone block's top face as 8-bit unorm bytes: 0.50 grey of 255, rounded.
const STONE: [u8; 3] = [128, 128, 128];
/// The buried face's colour, which must never reach the target.
const BURIED: [u8; 3] = [255, 0, 0];
/// The overlay text colour: opaque white.
const TEXT: [u8; 3] = [255, 255, 255];
/// The overlay shadow colour as 8-bit unorm bytes: 0.05 of 255, rounded.
const SHADOW: [u8; 3] = [13, 13, 13];
/// The stone colour the block's vertices carry: a stand-in for the atlas's texel colour,
/// which tasks 10 and 11 sample.
const STONE_COLOUR: [f32; 3] = [0.5, 0.5, 0.5];
/// The buried face's colour, which is no real texture's at all.
const BURIED_COLOUR: [f32; 3] = [1.0, 0.0, 0.0];
/// The packed light of a full-sky corner, both channels: level 15 shifted four bits with the
/// sampler's eight added.
const FULL_SKY: u16 = 248;

#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_terrain_pass_draws_the_block_and_keeps_the_buried_face_hidden() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);
    let depth = create_depth(&device);

    let mut terrain = TerrainPass::new(&device, format);
    terrain.set_camera(&queue, camera(), 1.0);
    terrain.upload(&device, &queue, (0, 0, 0), &stone_block_mesh());

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide pipeline headless encoder"),
    });
    with_terrain_pass(&mut encoder, &target.view, &depth, |pass| {
        terrain.draw(pass)
    });
    queue.submit(Some(encoder.finish()));

    let pixels = read_pixels(&device, &queue, &target);
    expect_pixel(&pixels, SIZE / 2, SIZE / 2, STONE, "the top face");
    expect_pixel(&pixels, 0, 0, SKY, "the corner above the block");
    expect_pixel(&pixels, SIZE - 1, SIZE - 1, SKY, "the corner beside it");
    assert!(
        !pixels.chunks_exact(4).any(|pixel| pixel[..3] == BURIED),
        "the buried face is drawn over the top face: the depth test, the depth write or the \
         depth attachment is off"
    );
}

#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_overlay_pass_draws_its_text_over_the_terrain() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);
    let depth = create_depth(&device);

    let mut terrain = TerrainPass::new(&device, format);
    terrain.set_camera(&queue, camera(), 1.0);
    terrain.upload(&device, &queue, (0, 0, 0), &stone_block_mesh());

    let mut overlay = OverlayPass::new(&device, format);
    overlay.set_size(&queue, SIZE as f32, SIZE as f32);
    // Four vertical bars in one line: each is the middle column of its cell, so the glyph
    // pixels sit at known places along the line.
    overlay.upload_text(&device, &queue, &["||||".to_string()]);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide pipeline headless encoder"),
    });
    with_terrain_pass(&mut encoder, &target.view, &depth, |pass| {
        terrain.draw(pass)
    });
    with_overlay_pass(&mut encoder, &target.view, |pass| overlay.draw(pass));
    queue.submit(Some(encoder.finish()));

    let pixels = read_pixels(&device, &queue, &target);
    // The scene is still there behind the overlay.
    expect_pixel(&pixels, SIZE / 2, SIZE / 2, STONE, "the top face");
    // The text margin is four pixels and the scale two, so the first cell's middle column is
    // at x = 8; its top row is at y = 4 and its shadow sits one pixel down and right.
    expect_pixel(&pixels, 8, 4, TEXT, "the first cell's first pixel");
    expect_pixel(&pixels, 9, 5, TEXT, "the text over its own shadow");
    expect_pixel(&pixels, 10, 5, SHADOW, "the shadow one pixel right");
    // The fourth cell starts one advance further along: x = 4 + 3 * 6 * 2 + 2 * 2.
    expect_pixel(&pixels, 44, 4, TEXT, "the fourth cell's first pixel");
    expect_pixel(&pixels, 0, 0, SKY, "the corner above the text");
    let painted = (0..24)
        .flat_map(|y| (0..24).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let pixel = pixel(&pixels, x, y);
            pixel != SKY && pixel != STONE
        })
        .count();
    assert!(painted > 0, "the overlay left no pixels in the top-left");
}

/// Builds the headless device and queue the tests render with.
///
/// Mirrors the backend choice in `Renderer::new` (`renderer.rs`); the test target cannot reach
/// the crate's private code, so keep the two copies in step.
fn headless_device() -> (wgpu::Device, wgpu::Queue) {
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
    block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("oxide pipeline headless device"),
        ..Default::default()
    }))
    .expect("a device is created")
}

/// The offscreen colour target: the texture the pixels are read back from and the view the
/// passes render through.
struct Target {
    /// The texture itself.
    texture: wgpu::Texture,
    /// A view of the whole texture.
    view: wgpu::TextureView,
}

/// Creates the offscreen colour target in `format`.
fn create_target(device: &wgpu::Device, format: wgpu::TextureFormat) -> Target {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("oxide pipeline headless target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    Target { texture, view }
}

/// Creates the depth texture the terrain pass tests against; the view keeps it alive.
fn create_depth(device: &wgpu::Device) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("oxide pipeline headless depth"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// Runs the terrain pass over the target: colour cleared to the sky, depth cleared to the far
/// plane, then `draw`.
fn with_terrain_pass(
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    draw: impl FnOnce(&mut wgpu::RenderPass<'_>),
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("oxide pipeline headless terrain pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(SKY_COLOR),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    draw(&mut pass);
}

/// Runs the overlay pass over the target: the colour the terrain pass left, loaded, and no
/// depth attachment at all, because the overlay's pipeline has no depth state.
fn with_overlay_pass(
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    draw: impl FnOnce(&mut wgpu::RenderPass<'_>),
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("oxide pipeline headless overlay pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
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
    draw(&mut pass);
}

/// One stone block's mesh at the origin, plus a buried face the depth test must hide.
///
/// The six faces are the mesher's: wound counter-clockwise seen from outside, each coloured
/// stone grey times the face's brightness. This test cannot use the mesher itself — the crate
/// graph gives `oxide-render` no edge to `oxide-game` — so the corner table and the brightness
/// values mirror the mesher's winding and the source's face shade table; both have their own
/// tests in `oxide-game`.
///
/// The buried face sits half a block up inside the block, facing the camera, wound the same
/// way, and is indexed after the block's faces. Nothing else can see it while the depth test
/// works: it is nearer than nothing in the frame and farther than the top face.
fn stone_block_mesh() -> ChunkMesh {
    /// The block's faces as `(corners, brightness)`, in the mesher's `Face::ALL` order.
    const FACES: [([[f32; 3]; 4], f32); 6] = [
        (
            [
                [0.0, 1.0, 0.0],
                [0.0, 1.0, 1.0],
                [1.0, 1.0, 1.0],
                [1.0, 1.0, 0.0],
            ],
            1.0,
        ),
        (
            [
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 0.0, 1.0],
                [0.0, 0.0, 1.0],
            ],
            0.5,
        ),
        (
            [
                [0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
                [1.0, 0.0, 0.0],
            ],
            0.8,
        ),
        (
            [
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 1.0],
                [1.0, 1.0, 1.0],
                [0.0, 1.0, 1.0],
            ],
            0.8,
        ),
        (
            [
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [1.0, 1.0, 1.0],
                [1.0, 0.0, 1.0],
            ],
            0.6,
        ),
        (
            [
                [0.0, 0.0, 1.0],
                [0.0, 1.0, 1.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0],
            ],
            0.6,
        ),
    ];

    let mut mesh = ChunkMesh::default();
    for (corners, brightness) in FACES {
        push_face(&mut mesh, corners, brightness, STONE_COLOUR);
    }
    push_face(
        &mut mesh,
        [
            [0.05, 0.5, 0.05],
            [0.05, 0.5, 0.95],
            [0.95, 0.5, 0.95],
            [0.95, 0.5, 0.05],
        ],
        1.0,
        BURIED_COLOUR,
    );
    mesh
}

/// Appends one face to `mesh`'s opaque layer — the fixture's faces are solid
/// terrain — four corners, then six indices for two triangles.
fn push_face(mesh: &mut ChunkMesh, corners: [[f32; 3]; 4], brightness: f32, colour: [f32; 3]) {
    let layer = &mut mesh.layers[Layer::Opaque.index()];
    let base = layer.vertices.len() as u32;
    for position in corners {
        let shade = |channel: usize| (colour[channel] * brightness * 255.0).round() as u8;
        layer.vertices.push(Vertex {
            position,
            // The shader reads neither the uv nor the light yet (tasks 10 and 11 do): the
            // corners carry the face's own uv corner order zeroed out and a full-sky packed
            // light sample.
            uv: [0.0; 2],
            light: [FULL_SKY; 2],
            colour: [shade(0), shade(1), shade(2), 255],
        });
    }
    layer
        .indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// The camera both tests look at the block with.
///
/// The eye sits three units from the top face's centre along `(0, -0.8, -0.6)`, so the frame's
/// centre ray meets the top face at its middle: the eye is `1.0 + 3.0 * 0.8` above the block
/// and `0.5 + 3.0 * 0.6` in front of it, and the pose's pitch is that direction's.
fn camera() -> Camera {
    Camera {
        pose: CameraPose {
            position: [0.5, 3.4 - f64::from(EYE_HEIGHT), 2.3],
            yaw: 180.0,
            pitch: 0.8_f32.asin().to_degrees(),
        },
        fov_degrees: DEFAULT_FOV,
        near: NEAR_PLANE,
        far_chunks: 8.0,
    }
}

/// Reads the whole target back as RGBA bytes, `BYTES_PER_ROW` per row.
fn read_pixels(device: &wgpu::Device, queue: &wgpu::Queue, target: &Target) -> Vec<u8> {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("oxide pipeline headless readback"),
        size: u64::from(BYTES_PER_ROW) * u64::from(SIZE),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide pipeline headless readback encoder"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target.texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(BYTES_PER_ROW),
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
    let pixels = slice.get_mapped_range().to_vec();
    buffer.unmap();
    pixels
}

/// The RGB bytes of one pixel of a readback.
fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let offset = (y * BYTES_PER_ROW + x * 4) as usize;
    [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
}

/// Asserts one pixel, naming it in the failure message.
fn expect_pixel(pixels: &[u8], x: u32, y: u32, want: [u8; 3], what: &str) {
    let got = pixel(pixels, x, y);
    let close = got
        .iter()
        .zip(want)
        .all(|(&got, want)| (i16::from(got) - i16::from(want)).abs() <= 2);
    assert!(close, "{what} at ({x}, {y}): got {got:?}, want {want:?}");
}

/// Blocks the calling thread until `future` resolves; the test has no async runtime.
///
/// The test target cannot reach the private `block_on` in `renderer.rs`, so this is a copy of
/// it; keep the two in step.
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
