//! Local-only tests of the pause menu through the real overlay pipeline: they need
//! a real adapter, so they are ignored by default. Run them on a machine with
//! a GPU:
//!
//! ```text
//! cargo test -p oxide-client --test pause_headless -- --ignored --nocapture
//! ```
//!
//! The pause frame's draws are the client's own (`dirt_draws`, the frame's
//! gradient pair, `PauseScreen::draws`, the button's draws); the hud pass
//! that renders them is the renderer's. That direction — the client driving
//! the renderer — is why this case lives here and not beside the renderer's
//! own pipeline probes: the crate graph gives `oxide-render` no edge to
//! `oxide-client`.
//!
//! Every sheet here is built by hand in the test — flat colours, never a
//! pixel from the asset store — and uploaded through `HudPass::set_texture`,
//! so this is the only test that runs the real sampler over the pause
//! frame's draws. The dirt case reads the tinted tile under the top
//! gradient, the title case counts the inked title pixels, and the button
//! case reads the idle and disabled strips.

use std::sync::mpsc;
use std::task::{Context, Poll, Wake, Waker};

use oxide_assets::font::Font;
use oxide_assets::texture::Texture;
use oxide_client::screens::pause::{DIRT_SHEET, PauseScreen, dirt_draws, pause_labels};
use oxide_client::screens::widgets::WIDGETS_SHEET;
use oxide_render::entity_pass::TextureRegistry;
use oxide_render::hud::{HudDraw, HudPass, HudTexture, scaled_resolution};
use oxide_render::renderer::SKY_COLOR;
use oxide_render::terrain_pass::DEPTH_FORMAT;

/// Runs `future` to completion on the calling thread, parking it while the
/// polled future is pending.
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

/// The headless device and queue: Vulkan on Linux, the platform primary
/// elsewhere, high-performance adapter, no surface.
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
        label: Some("oxide pause headless device"),
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

/// The probe target's width in texels; the readback row stride (448 × 4) stays
/// a multiple of 256 bytes.
const ROWS_WIDE: u32 = 448;
/// The probe target's height in texels.
const ROWS_TALL: u32 = 240;

/// The offscreen colour target at the probe's size.
fn rows_target(device: &wgpu::Device, format: wgpu::TextureFormat) -> Target {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("oxide pause headless target"),
        size: wgpu::Extent3d {
            width: ROWS_WIDE,
            height: ROWS_TALL,
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

/// The depth texture at the probe's size.
fn rows_depth(device: &wgpu::Device) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("oxide pause headless depth"),
        size: wgpu::Extent3d {
            width: ROWS_WIDE,
            height: ROWS_TALL,
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

/// Reads the probe's target back: one 1792-byte row per line, 240 lines.
fn read_rows_pixels(device: &wgpu::Device, queue: &wgpu::Queue, target: &Target) -> Vec<u8> {
    const ROW: u32 = ROWS_WIDE * 4;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("oxide pause headless readback"),
        size: u64::from(ROW) * u64::from(ROWS_TALL),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide pause headless readback encoder"),
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
                bytes_per_row: Some(ROW),
                rows_per_image: Some(ROWS_TALL),
            },
        },
        wgpu::Extent3d {
            width: ROWS_WIDE,
            height: ROWS_TALL,
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

/// Runs a pass that only clears the target to `colour`, with no depth attachment, so a
/// hud pass can follow and blend over the result.
fn with_clear_pass(
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    colour: wgpu::Color,
) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("oxide pause headless clear pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(colour),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
}

/// Runs the overlay pass over the target: the colour the clear pass left, loaded, and the
/// depth buffer cleared and attached, the state the hud's pipelines state their depth
/// against.
fn with_overlay_pass(
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    draw: impl FnOnce(&mut wgpu::RenderPass<'_>),
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("oxide pause headless overlay pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
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

/// The pause probe's synthetic font sheet: 128x128 fully inked white, so every
/// printable cell spans its whole 8x8 and the title draws as a solid block at
/// a known advance.
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn pause_font_sheet() -> Texture {
    Texture {
        width: 128,
        height: 128,
        rgba: vec![255u8; 128 * 128 * 4],
    }
}

/// The synthetic dirt sheet: 16x16 flat mid-grey, the store tile's size, so a
/// read-back names the tinted tile under the gradient.
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn pause_dirt_sheet() -> Texture {
    Texture {
        width: 16,
        height: 16,
        rgba: [64u8, 64, 64, 255].repeat(16 * 16),
    }
}

/// The synthetic widgets sheet: the three button strips as flat rows — the
/// disabled strip at `v` 46, the normal strip at 66, the hovered strip at 86
/// (`GuiButton.drawButton`'s `46 + i * 20`).
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn pause_widgets_sheet() -> Texture {
    const SIDE: u32 = 256;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    for (row, colour) in [
        (46, [100u8, 100, 100, 255]),
        (66, [200u8, 200, 200, 255]),
        (86, [235u8, 235, 235, 255]),
    ] {
        for y in row..row + 20 {
            for x in 0..SIDE {
                let at = ((y * SIDE + x) * 4) as usize;
                rgba[at..at + 4].copy_from_slice(&colour);
            }
        }
    }
    Texture {
        width: SIDE,
        height: SIDE,
        rgba,
    }
}

/// The pause probe: the rows-sized target with the font, the dirt tile and
/// the widgets registered under the keys the pause frame's draws name.
fn pause_scene() -> (wgpu::Device, wgpu::Queue, Target, HudPass, Font) {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = rows_target(&device, format);
    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, ROWS_WIDE as f32, ROWS_TALL as f32);
    let sheet = pause_font_sheet();
    hud.set_font(&device, &queue, &sheet)
        .expect("the synthetic sheet is a 16x16 grid");
    hud.set_texture(&device, &queue, DIRT_SHEET, &pause_dirt_sheet());
    hud.set_texture(&device, &queue, WIDGETS_SHEET, &pause_widgets_sheet());
    let font = Font::load(&sheet, None).expect("the synthetic sheet is a 16x16 grid");
    (device, queue, target, hud, font)
}

/// Renders the hud's draws over a cleared rows probe and reads the pixels
/// back.
fn pause_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &Target,
    hud: &HudPass,
) -> Vec<u8> {
    let depth = rows_depth(device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide pause headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| hud.draw(pass));
    queue.submit(Some(encoder.finish()));
    read_rows_pixels(device, queue, target)
}

/// One rows-probe pixel's RGB bytes.
fn pause_pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let offset = (y * ROWS_WIDE * 4 + x * 4) as usize;
    [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
}

/// Asserts one rows-probe pixel within two levels per channel, naming it in
/// the failure message.
fn expect_pause_pixel(pixels: &[u8], x: u32, y: u32, want: [u8; 3], what: &str) {
    let got = pause_pixel(pixels, x, y);
    let close = got
        .iter()
        .zip(want)
        .all(|(&got, want)| (i16::from(got) - i16::from(want)).abs() <= 2);
    assert!(close, "{what} at ({x}, {y}): got {got:?}, want {want:?}");
}

/// The pause frame's own draws at the probe size: the dirt tiles lead, the
/// frame's world-present gradient pair falls over them, and the centred
/// title plus the six buttons ride on top — the assembly `screen_draws`
/// builds for the pause family, with the gradient pair's own literals.
fn pause_frame_draws(font: &Font) -> Vec<HudDraw> {
    let scaled = scaled_resolution(ROWS_WIDE, ROWS_TALL, 1);
    assert_eq!(
        (scaled.width, scaled.height),
        (ROWS_WIDE, ROWS_TALL),
        "the probe maps one to one"
    );
    let screen = PauseScreen::new(&scaled, pause_labels(None));
    let mut draws = dirt_draws(&scaled);
    draws.push(HudDraw::Rect {
        x: 0.0,
        y: 0.0,
        width: ROWS_WIDE as f32,
        height: ROWS_TALL as f32 / 2.0,
        colour: [16.0 / 255.0, 16.0 / 255.0, 16.0 / 255.0, 192.0 / 255.0],
    });
    draws.push(HudDraw::Rect {
        x: 0.0,
        y: ROWS_TALL as f32 / 2.0,
        width: ROWS_WIDE as f32,
        height: ROWS_TALL as f32 - ROWS_TALL as f32 / 2.0,
        colour: [16.0 / 255.0, 16.0 / 255.0, 16.0 / 255.0, 208.0 / 255.0],
    });
    draws.extend(screen.draws(font, &scaled, None));
    draws
}

/// The pause frame at the probe size: the dirt tiles span 32 px under the
/// (64, 64, 64) tint, the gradient falls over them, the "Game menu" title
/// inks white at y 40, and the six buttons draw.
///
/// At 448x240 the layout pins half 224 and quarter 60: Back to Game
/// (124, 68), the Open-to-LAN button disabled at (226, 140). The flat dirt
/// tints to 64*64/255 ≈ 16 per channel, and both gradient halves blend
/// near-black at 16/255 over it — every background pixel reads 16.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_pause_frame_draws_dirt_gradient_title_and_buttons() {
    let scaled = scaled_resolution(ROWS_WIDE, ROWS_TALL, 1);
    let (device, queue, target, mut hud, font) = pause_scene();
    let screen = PauseScreen::new(&scaled, pause_labels(None));
    let draws = pause_frame_draws(&font);
    let dirt = draws
        .iter()
        .filter(|draw| {
            matches!(draw, HudDraw::TexturedRect {
                texture: HudTexture::Named(name),
                ..
            } if *name == DIRT_SHEET)
        })
        .count();
    assert_eq!(
        dirt,
        14 * 8,
        "448x240 tiles ceil(448/32) by ceil(240/32) at the 32 px span"
    );
    assert!(
        draws
            .iter()
            .any(|draw| matches!(draw, HudDraw::Text { text, x, y, .. }
            if text == "Game menu" && *y == 40.0 && (*x - 186.0).abs() < 1.0)),
        "the centred title draws at (w/2, 40)"
    );
    for label in screen.buttons().iter().map(|button| &button.label) {
        assert!(
            draws
                .iter()
                .any(|draw| matches!(draw, HudDraw::Text { text, .. }
                if text == label)),
            "the {label} button's label draws"
        );
    }
    hud.set_draws(
        &device,
        &queue,
        &draws,
        &TextureRegistry::new(&device, &queue),
    );
    let pixels = pause_frame(&device, &queue, &target, &hud);
    expect_pause_pixel(
        &pixels,
        5,
        5,
        [16, 16, 16],
        "the tinted dirt under the top gradient",
    );
    expect_pause_pixel(
        &pixels,
        5,
        200,
        [16, 16, 16],
        "the tinted dirt under the bottom gradient",
    );
    let title_ink = (150u32..300)
        .flat_map(|x| (40u32..48).map(move |y| (x, y)))
        .filter(|&(x, y)| pause_pixel(&pixels, x, y) == [255, 255, 255])
        .count();
    assert!(
        title_ink > 100,
        "the title inks white pixels at y 40: {title_ink}"
    );
    expect_pause_pixel(
        &pixels,
        130,
        72,
        [200, 200, 200],
        "the idle Back to Game strip",
    );
    expect_pause_pixel(
        &pixels,
        230,
        144,
        [100, 100, 100],
        "the disabled Open to LAN strip",
    );
}
