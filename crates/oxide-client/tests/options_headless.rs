//! Local-only tests of the video settings screen through the real overlay pipeline: they need
//! a real adapter, so they are ignored by default. Run them on a machine with
//! a GPU:
//!
//! ```text
//! cargo test -p oxide-client --test options_headless -- --ignored --nocapture
//! ```
//!
//! The video frame's draws are the client's own (`dirt_draws`, the frame's
//! gradient pair, `VideoScreen::draws`); the hud pass that renders them is
//! the renderer's. That direction — the client driving the renderer — is why
//! these cases live here and not beside the renderer's own pipeline probes:
//! the crate graph gives `oxide-render` no edge to `oxide-client`.
//!
//! Every sheet here is built by hand in the test — flat colours, never a
//! pixel from the asset store — and uploaded through `HudPass::set_texture`,
//! so these are the only tests that run the real sampler over the video
//! frame's draws. The default-scale case reads the tinted tile under the top
//! gradient, the inked title, the render-distance knob strip and the idle
//! Done strip; the scrolled case wheels one notch and reads the moved knob
//! strip while the title and dirt still hold.

use std::sync::mpsc;
use std::task::{Context, Poll, Wake, Waker};

use oxide_assets::font::Font;
use oxide_assets::texture::Texture;
use oxide_client::screens::options::{VideoControl, VideoScreen, video_labels};
use oxide_client::screens::pause::{DIRT_SHEET, dirt_draws};
use oxide_client::screens::widgets::WIDGETS_SHEET;
use oxide_game::settings::GameSettings;
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
        label: Some("oxide options headless device"),
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
        label: Some("oxide options headless target"),
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
        label: Some("oxide options headless depth"),
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
        label: Some("oxide options headless readback"),
        size: u64::from(ROW) * u64::from(ROWS_TALL),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide options headless readback encoder"),
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
        label: Some("oxide options headless clear pass"),
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
        label: Some("oxide options headless overlay pass"),
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

/// The options probe's synthetic font sheet: 128x128 fully inked white, so every
/// printable cell spans its whole 8x8 and the title draws as a solid block at
/// a known advance.
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn options_font_sheet() -> Texture {
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
fn options_dirt_sheet() -> Texture {
    Texture {
        width: 16,
        height: 16,
        rgba: [64u8, 64, 64, 255].repeat(16 * 16),
    }
}

/// The synthetic widgets sheet: the three button strips as flat rows — the
/// disabled strip at `v` 46, the normal strip at 66, the hovered strip at 86
/// (`GuiButton.drawButton`'s `46 + i * 20`). The option slider's track and
/// knob read the same strips.
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn options_widgets_sheet() -> Texture {
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

/// The options probe: the rows-sized target with the font, the dirt tile and
/// the widgets registered under the keys the video frame's draws name.
fn options_scene() -> (wgpu::Device, wgpu::Queue, Target, HudPass, Font) {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = rows_target(&device, format);
    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, ROWS_WIDE as f32, ROWS_TALL as f32);
    let sheet = options_font_sheet();
    hud.set_font(&device, &queue, &sheet)
        .expect("the synthetic sheet is a 16x16 grid");
    hud.set_texture(&device, &queue, DIRT_SHEET, &options_dirt_sheet());
    hud.set_texture(&device, &queue, WIDGETS_SHEET, &options_widgets_sheet());
    let font = Font::load(&sheet, None).expect("the synthetic sheet is a 16x16 grid");
    (device, queue, target, hud, font)
}

/// Renders the hud's draws over a cleared rows probe and reads the pixels
/// back.
fn options_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &Target,
    hud: &HudPass,
) -> Vec<u8> {
    let depth = rows_depth(device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide options headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| hud.draw(pass));
    queue.submit(Some(encoder.finish()));
    read_rows_pixels(device, queue, target)
}

/// One rows-probe pixel's RGB bytes.
fn options_pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let offset = (y * ROWS_WIDE * 4 + x * 4) as usize;
    [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
}

/// Asserts one rows-probe pixel within two levels per channel, naming it in
/// the failure message.
fn expect_options_pixel(pixels: &[u8], x: u32, y: u32, want: [u8; 3], what: &str) {
    let got = options_pixel(pixels, x, y);
    let close = got
        .iter()
        .zip(want)
        .all(|(&got, want)| (i16::from(got) - i16::from(want)).abs() <= 2);
    assert!(close, "{what} at ({x}, {y}): got {got:?}, want {want:?}");
}

/// The video frame's own draws at the probe size: the dirt tiles lead, the
/// frame's world-present gradient pair falls over them, and the centred
/// title plus the sixteen rows and Done ride on top — the assembly
/// `screen_draws` builds for the video family, with the gradient pair's own
/// literals.
fn video_frame_draws(font: &Font, screen: &VideoScreen) -> Vec<HudDraw> {
    let scaled = scaled_resolution(ROWS_WIDE, ROWS_TALL, 1);
    assert_eq!(
        (scaled.width, scaled.height),
        (ROWS_WIDE, ROWS_TALL),
        "the probe maps one to one"
    );
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

/// The video frame at the default scale: the dirt tiles span 32 px under the
/// (64, 64, 64) tint, the gradient falls over them, the "Video Settings"
/// title inks white at y 5, the render-distance knob reads the idle strip,
/// and the idle Done strip draws.
///
/// At 448x240 the layout pins half 224: the row-0 pair sits at x 69 / 229
/// and y 32, Done spans (124, 213). The flat dirt tints to 64*64/255 ≈ 16
/// per channel, and both gradient halves blend near-black at 16/255 over
/// it — every background pixel reads 16.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_video_frame_at_default_scale() {
    let scaled = scaled_resolution(ROWS_WIDE, ROWS_TALL, 1);
    let settings = GameSettings::default();
    let screen = VideoScreen::new(&scaled, &settings, video_labels(None), true);
    assert_eq!(screen.scroll(), 0.0, "the list opens home");
    let (device, queue, target, mut hud, font) = options_scene();
    let draws = video_frame_draws(&font, &screen);
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
            if text == "Video Settings" && *y == 5.0 && (*x - 163.5).abs() < 1.0)),
        "the centred title draws at (w/2, 5)"
    );
    assert!(
        draws
            .iter()
            .any(|draw| matches!(draw, HudDraw::Text { text, .. }
            if text == "Render Distance: 12 chunks")),
        "the row-0 slider's label draws at the default distance"
    );
    let (_, right) = screen.row_pair(0);
    let VideoControl::Slider(_, slider) = right else {
        panic!("row 0 pairs graphics with the render-distance slider");
    };
    assert!(
        draws
            .iter()
            .any(|draw| matches!(draw, HudDraw::TexturedRect {
            width,
            ..
        } if *width == 4.0)),
        "the knob halves draw at 4 px wide"
    );
    let knob = (slider.knob_x() + 2.0, 34.0);
    hud.set_draws(
        &device,
        &queue,
        &draws,
        &TextureRegistry::new(&device, &queue),
    );
    let pixels = options_frame(&device, &queue, &target, &hud);
    expect_options_pixel(
        &pixels,
        5,
        5,
        [16, 16, 16],
        "the tinted dirt under the top gradient",
    );
    expect_options_pixel(
        &pixels,
        5,
        200,
        [16, 16, 16],
        "the tinted dirt under the bottom gradient",
    );
    let title_ink = (150u32..300)
        .flat_map(|x| (5u32..13).map(move |y| (x, y)))
        .filter(|&(x, y)| options_pixel(&pixels, x, y) == [255, 255, 255])
        .count();
    assert!(
        title_ink > 100,
        "the title inks white pixels at y 5: {title_ink}"
    );
    expect_options_pixel(
        &pixels,
        knob.0 as u32,
        knob.1 as u32,
        [200, 200, 200],
        "the render-distance knob reads the idle strip",
    );
    expect_options_pixel(&pixels, 130, 217, [200, 200, 200], "the idle Done strip");
}

/// The video frame one notch down: the wheel scrolls a single slot, the
/// rows re-seat, and the frame still renders — row 0 culls above the list
/// top while row 1's track reads the never-highlighting strip, and the
/// title and dirt hold.
///
/// At 448x240 the eight rows overrun the 176 px window by 24 px, so one
/// notch down lands the ceiling: row 0 re-seats at y 8 (culled), row 1
/// at y 33.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_video_frame_one_notch_down() {
    let scaled = scaled_resolution(ROWS_WIDE, ROWS_TALL, 1);
    let settings = GameSettings::default();
    let mut screen = VideoScreen::new(&scaled, &settings, video_labels(None), true);
    screen.wheel(&scaled, -1.0);
    assert_eq!(screen.scroll(), 24.0, "one notch down lands the ceiling");
    assert_eq!(screen.row_y(0), 8.0, "row 0 re-seats at the scrolled top");
    let (device, queue, target, mut hud, font) = options_scene();
    let draws = video_frame_draws(&font, &screen);
    let (_, right) = screen.row_pair(1);
    let VideoControl::Slider(_, slider) = right else {
        panic!("row 1 pairs smooth lighting with the framerate slider");
    };
    assert_eq!(slider.y(), 33.0, "row 1 re-seats below the list top");
    assert!(
        !draws
            .iter()
            .any(|draw| matches!(draw, HudDraw::Text { text, .. }
            if text == "Render Distance: 12 chunks")),
        "the culled row-0 label leaves the draws one notch down"
    );
    assert!(
        draws
            .iter()
            .any(|draw| matches!(draw, HudDraw::Text { text, .. }
            if text.starts_with("Max Framerate: "))),
        "the row-1 slider's label still draws one notch down"
    );
    hud.set_draws(
        &device,
        &queue,
        &draws,
        &TextureRegistry::new(&device, &queue),
    );
    let pixels = options_frame(&device, &queue, &target, &hud);
    expect_options_pixel(
        &pixels,
        5,
        5,
        [16, 16, 16],
        "the tinted dirt under the top gradient",
    );
    let title_ink = (150u32..300)
        .flat_map(|x| (5u32..13).map(move |y| (x, y)))
        .filter(|&(x, y)| options_pixel(&pixels, x, y) == [255, 255, 255])
        .count();
    assert!(
        title_ink > 100,
        "the title still inks white pixels at y 5: {title_ink}"
    );
    expect_options_pixel(
        &pixels,
        375,
        38,
        [100, 100, 100],
        "the row-1 track reads the never-highlighting strip one notch down",
    );
}
