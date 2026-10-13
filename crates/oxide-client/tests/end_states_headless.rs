//! Local-only tests of the end-state screens through the real overlay pipeline: they
//! need a real adapter, so they are ignored by default. Run them on a machine with
//! a GPU, one at a time:
//!
//! ```text
//! cargo test -p oxide-client --test end_states_headless death -- --ignored --nocapture
//! ```
//!
//! The frames' draws are the client's own (the screens' `draws` over the dirt and the
//! default-background pair, the order `view::screen_draws` assembles); the hud pass
//! that renders them is the renderer's. That direction — the client driving the
//! renderer — is why these cases live here and not beside the renderer's own
//! pipeline probes: the crate graph gives `oxide-render` no edge to `oxide-client`.
//!
//! Every sheet here is built by hand in the test — flat colours, never a
//! pixel from the asset store — and uploaded through `HudPass::set_texture`,
//! so these are the only tests that run the real sampler over the end-state
//! frames' draws. The death case reads the blended gradient over the clear
//! colour, the title ink and the disabled strip; the disconnect and terrain
//! cases read the tinted dirt, their text ink and the button strip; the
//! confirm case reads the pink warning and the three strips.

use std::sync::mpsc;
use std::task::{Context, Poll, Wake, Waker};

use oxide_assets::font::Font;
use oxide_assets::texture::Texture;
use oxide_client::screens::end_states::{
    ConfirmScreen, DeathScreen, DisconnectedScreen, TerrainScreen, confirm_labels, death_labels,
    disconnect_button_label, disconnect_button_y, disconnect_title, terrain_line,
};
use oxide_client::screens::pause::{DIRT_SHEET, dirt_draws};
use oxide_client::screens::widgets::WIDGETS_SHEET;
use oxide_render::entity_pass::TextureRegistry;
use oxide_render::hud::{HudDraw, HudPass, scaled_resolution};
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
        label: Some("oxide end states headless device"),
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
        label: Some("oxide end states headless target"),
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
        label: Some("oxide end states headless depth"),
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
        label: Some("oxide end states headless readback"),
        size: u64::from(ROW) * u64::from(ROWS_TALL),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide end states headless readback encoder"),
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
        label: Some("oxide end states headless clear pass"),
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
        label: Some("oxide end states headless overlay pass"),
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

/// The probe's synthetic font sheet: 128x128 fully inked white, so every
/// printable cell spans its whole 8x8 and all title and line text draws as
/// solid blocks at known advances.
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn end_font_sheet() -> Texture {
    Texture {
        width: 128,
        height: 128,
        rgba: vec![255u8; 128 * 128 * 4],
    }
}

/// The synthetic dirt sheet: flat mid grey, the tint the frame's assembly
/// reads under the gradients.
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn end_dirt_sheet() -> Texture {
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
fn end_widgets_sheet() -> Texture {
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

/// The probe: the rows-sized target with the font, the dirt and the widgets
/// registered under the keys the end-state draws name.
fn end_scene() -> (wgpu::Device, wgpu::Queue, Target, HudPass, Font) {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = rows_target(&device, format);
    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, ROWS_WIDE as f32, ROWS_TALL as f32);
    let sheet = end_font_sheet();
    hud.set_font(&device, &queue, &sheet)
        .expect("the synthetic sheet is a 16x16 grid");
    hud.set_texture(&device, &queue, DIRT_SHEET, &end_dirt_sheet());
    hud.set_texture(&device, &queue, WIDGETS_SHEET, &end_widgets_sheet());
    let font = Font::load(&sheet, None).expect("the synthetic sheet is a 16x16 grid");
    (device, queue, target, hud, font)
}

/// The default-background pair `view::screen_draws` lays under every screen:
/// 0xC0101010 over the top half, 0xD0101010 under the bottom half.
fn default_pair() -> Vec<HudDraw> {
    vec![
        HudDraw::Rect {
            x: 0.0,
            y: 0.0,
            width: ROWS_WIDE as f32,
            height: ROWS_TALL as f32 / 2.0,
            colour: [16.0 / 255.0, 16.0 / 255.0, 16.0 / 255.0, 192.0 / 255.0],
        },
        HudDraw::Rect {
            x: 0.0,
            y: ROWS_TALL as f32 / 2.0,
            width: ROWS_WIDE as f32,
            height: ROWS_TALL as f32 - ROWS_TALL as f32 / 2.0,
            colour: [16.0 / 255.0, 16.0 / 255.0, 16.0 / 255.0, 208.0 / 255.0],
        },
    ]
}

/// Renders the draws over a cleared rows probe and reads the pixels back.
fn end_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &Target,
    hud: &HudPass,
) -> Vec<u8> {
    let depth = rows_depth(device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide end states headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| hud.draw(pass));
    queue.submit(Some(encoder.finish()));
    read_rows_pixels(device, queue, target)
}

/// One rows-probe pixel's RGB bytes.
fn end_pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let offset = (y * ROWS_WIDE * 4 + x * 4) as usize;
    [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
}

/// Asserts one rows-probe pixel within three levels per channel, naming it in
/// the failure message.
fn expect_end_pixel(pixels: &[u8], x: u32, y: u32, want: [u8; 3], what: &str) {
    let got = end_pixel(pixels, x, y);
    let close = got
        .iter()
        .zip(want)
        .all(|(&got, want)| (i16::from(got) - i16::from(want)).abs() <= 3);
    assert!(close, "{what} at ({x}, {y}): got {got:?}, want {want:?}");
}

/// Counts the rows-probe pixels of exactly `colour` in the box.
fn count_end_ink(pixels: &[u8], x0: u32, x1: u32, y0: u32, y1: u32, colour: [u8; 3]) -> usize {
    (x0..x1)
        .flat_map(|x| (y0..y1).map(move |y| (x, y)))
        .filter(|&(x, y)| end_pixel(pixels, x, y) == colour)
        .count()
}

/// The death frame draws the two-stop gradient over the default pair, the 2x
/// title, the score line and the still-disabled button pair: the top-left
/// pixel blends the top stop over the clear, the title band inks white, the
/// score band inks the yellow value, and the Respawn strip reads disabled.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_death_frame_draws_the_gradient_title_score_and_gated_buttons() {
    let scaled = scaled_resolution(ROWS_WIDE, ROWS_TALL, 1);
    assert_eq!(
        (scaled.width, scaled.height),
        (ROWS_WIDE, ROWS_TALL),
        "the probe maps one to one"
    );
    let (device, queue, target, mut hud, font) = end_scene();
    let screen = DeathScreen::new(&scaled, &death_labels(None), 0);
    let mut draws = default_pair();
    draws.extend(screen.draws(&font, &scaled, None));
    assert!(
        draws
            .iter()
            .any(|draw| matches!(draw, HudDraw::Text { text, .. } if text == "You died!")),
        "the title draws"
    );
    hud.set_draws(
        &device,
        &queue,
        &draws,
        &TextureRegistry::new(&device, &queue),
    );
    let pixels = end_frame(&device, &queue, &target, &hud);
    // The top stop (alpha 96 over red 80) over the default pair over the
    // sky clear: (62, 37, 46).
    expect_end_pixel(&pixels, 5, 5, [62, 37, 46], "the dimmed top gradient");
    let title_ink = count_end_ink(&pixels, 150, 300, 60, 80, [255, 255, 255]);
    assert!(
        title_ink > 100,
        "the 2x title inks white at y 60: {title_ink}"
    );
    let score_ink = count_end_ink(&pixels, 150, 300, 100, 110, [255, 255, 85]);
    assert!(
        score_ink > 10,
        "the score value inks yellow at y 100: {score_ink}"
    );
    expect_end_pixel(
        &pixels,
        125,
        133,
        [100, 100, 100],
        "the disabled Respawn strip",
    );
}

/// The disconnect frame stands on dirt under the default pair, with the grey
/// Lost title, the wrapped white body and the Back button on top: the corner
/// reads the tinted dirt, the title band inks grey, and the button strip
/// reads idle.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_disconnect_frame_draws_dirt_title_body_and_back() {
    let scaled = scaled_resolution(ROWS_WIDE, ROWS_TALL, 1);
    let (device, queue, target, mut hud, font) = end_scene();
    let screen = DisconnectedScreen::new(
        &scaled,
        disconnect_title(None),
        String::from("{\"text\":\"End of stream\"}"),
        &font,
        disconnect_button_label(None),
    );
    assert_eq!(screen.title(), "Connection Lost", "the fixed Lost title");
    assert_eq!(screen.lines().len(), 1, "the short body wraps to one line");
    let mut draws = dirt_draws(&scaled);
    draws.extend(default_pair());
    draws.extend(screen.draws(&font, &scaled, None));
    hud.set_draws(
        &device,
        &queue,
        &draws,
        &TextureRegistry::new(&device, &queue),
    );
    let pixels = end_frame(&device, &queue, &target, &hud);
    expect_end_pixel(
        &pixels,
        5,
        5,
        [16, 16, 16],
        "the tinted dirt under the pair",
    );
    let title_ink = count_end_ink(&pixels, 150, 300, 95, 108, [170, 170, 170]);
    assert!(
        title_ink > 50,
        "the grey title inks above the body: {title_ink}"
    );
    let body_ink = count_end_ink(&pixels, 150, 300, 113, 124, [255, 255, 255]);
    assert!(
        body_ink > 10,
        "the white body inks under the title: {body_ink}"
    );
    let button_y = disconnect_button_y(scaled.height, screen.lines().len().max(1), font.height());
    expect_end_pixel(
        &pixels,
        125,
        button_y as u32 + 1,
        [200, 200, 200],
        "the idle Back strip",
    );
}

/// The terrain frame stands on dirt under the default pair, with the centred
/// Downloading line on top: the corner reads the tinted dirt and the line's
/// band inks white at `h/2 - 50`.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_terrain_frame_draws_the_dirt_and_the_centred_line() {
    let scaled = scaled_resolution(ROWS_WIDE, ROWS_TALL, 1);
    let (device, queue, target, mut hud, font) = end_scene();
    let screen = TerrainScreen::new(terrain_line(None));
    assert_eq!(screen.line(), "Downloading terrain", "the standing line");
    let mut draws = dirt_draws(&scaled);
    draws.extend(default_pair());
    draws.extend(screen.draws(&font, &scaled));
    hud.set_draws(
        &device,
        &queue,
        &draws,
        &TextureRegistry::new(&device, &queue),
    );
    let pixels = end_frame(&device, &queue, &target, &hud);
    expect_end_pixel(
        &pixels,
        5,
        5,
        [16, 16, 16],
        "the tinted dirt under the pair",
    );
    let line_ink = count_end_ink(&pixels, 150, 300, 70, 80, [255, 255, 255]);
    assert!(
        line_ink > 50,
        "the centred line inks white at y 70: {line_ink}"
    );
}

/// The confirm frame stands on dirt under the default pair, with the title,
/// the wrapped link, the pink warning and the three-button row on top: the
/// warning band inks pink at y 110 and every strip reads idle.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_confirm_frame_draws_the_warning_and_three_idle_buttons() {
    let scaled = scaled_resolution(ROWS_WIDE, ROWS_TALL, 1);
    let (device, queue, target, mut hud, font) = end_scene();
    let screen = ConfirmScreen::new(
        &scaled,
        String::from("https://a.example"),
        &font,
        &confirm_labels(None),
        false,
    );
    assert_eq!(screen.buttons().len(), 3, "three buttons stand");
    let mut draws = dirt_draws(&scaled);
    draws.extend(default_pair());
    draws.extend(screen.draws(&font, &scaled, None));
    hud.set_draws(
        &device,
        &queue,
        &draws,
        &TextureRegistry::new(&device, &queue),
    );
    let pixels = end_frame(&device, &queue, &target, &hud);
    let warning_ink = count_end_ink(&pixels, 100, 350, 110, 120, [255, 204, 204]);
    assert!(
        warning_ink > 50,
        "the pink warning inks at y 110: {warning_ink}"
    );
    for (x, what) in [(70, "confirm"), (175, "copy"), (280, "cancel")] {
        expect_end_pixel(
            &pixels,
            x,
            137,
            [200, 200, 200],
            &format!("the idle {what} strip"),
        );
    }
}
