//! Local-only tests of the main menu through the real overlay pipeline: they need
//! a real adapter, so they are ignored by default. Run them on a machine with
//! a GPU:
//!
//! ```text
//! cargo test -p oxide-client --test menu_headless -- --ignored --nocapture
//! ```
//!
//! The menu's draws are the client's own (`MainMenuScreen::draws`, the
//! button's draws); the hud pass that renders them is the renderer's. That
//! direction — the client driving the renderer — is why these cases live
//! here and not beside the renderer's own pipeline probes: the crate graph
//! gives `oxide-render` no edge to `oxide-client`.
//!
//! Every sheet here is built by hand in the test — flat colours, never a
//! pixel from the asset store — and uploaded through `HudPass::set_texture`,
//! so these are the only tests that run the real sampler over the menu's
//! draws. The panorama case names the yaw-facing face by its flat colour,
//! the splash case reads the yellow pixels the rotated text inks, and the
//! button case reads the three strips with their label colours.

use std::sync::mpsc;
use std::task::{Context, Poll, Wake, Waker};

use oxide_assets::font::Font;
use oxide_assets::texture::Texture;
use oxide_client::screens::menu::{
    LOGO_SHEET, MainMenuScreen, PANORAMA_SHEETS, SPLASH_ANGLE_DEG, SPLASH_COLOUR, menu_labels,
};
use oxide_client::screens::widgets::{Button, WIDGETS_SHEET};
use oxide_render::entity_pass::TextureRegistry;
use oxide_render::hud::{HudDraw, HudPass, HudTexture, scaled_resolution};
use oxide_render::renderer::SKY_COLOR;
use oxide_render::terrain_pass::DEPTH_FORMAT;
use oxide_render::text::string_width;

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
        label: Some("oxide menu headless device"),
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
        label: Some("oxide menu headless target"),
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
        label: Some("oxide menu headless depth"),
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
        label: Some("oxide menu headless readback"),
        size: u64::from(ROW) * u64::from(ROWS_TALL),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide menu headless readback encoder"),
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
        label: Some("oxide menu headless clear pass"),
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
        label: Some("oxide menu headless overlay pass"),
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

/// The menu probe's synthetic font sheet: 128x128 fully inked white, so every
/// printable cell spans its whole 8x8 and all label and splash text draws as
/// solid blocks at known advances.
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn menu_font_sheet() -> Texture {
    Texture {
        width: 128,
        height: 128,
        rgba: vec![255u8; 128 * 128 * 4],
    }
}

/// One synthetic panorama face: a solid 256x256 sheet in the face's own flat
/// colour, so a read-back names the face it sampled.
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn menu_face_sheet(face: usize) -> Texture {
    const FACE: [[u8; 4]; 6] = [
        [200, 30, 30, 255],
        [30, 200, 30, 255],
        [30, 30, 200, 255],
        [200, 200, 30, 255],
        [200, 30, 200, 255],
        [30, 200, 200, 255],
    ];
    Texture {
        width: 256,
        height: 256,
        rgba: FACE[face].repeat(256 * 256),
    }
}

/// The synthetic widgets sheet: the three button strips as flat rows — the
/// disabled strip at `v` 46, the normal strip at 66, the hovered strip at 86
/// (`GuiButton.drawButton`'s `46 + i * 20`).
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn menu_widgets_sheet() -> Texture {
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

/// The synthetic logo sheet: flat green, so any logo blit reads green
/// whichever branch (normal or rare) draws.
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn menu_logo_sheet() -> Texture {
    Texture {
        width: 256,
        height: 256,
        rgba: [40u8, 160, 40, 255].repeat(256 * 256),
    }
}

/// The menu probe: the rows-sized target with the font, the six panorama
/// faces, the logo and the widgets registered under the keys the menu's
/// draws name.
fn menu_scene() -> (wgpu::Device, wgpu::Queue, Target, HudPass, Font) {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = rows_target(&device, format);
    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, ROWS_WIDE as f32, ROWS_TALL as f32);
    let sheet = menu_font_sheet();
    hud.set_font(&device, &queue, &sheet)
        .expect("the synthetic sheet is a 16x16 grid");
    for (face, key) in PANORAMA_SHEETS.iter().enumerate() {
        hud.set_texture(&device, &queue, key, &menu_face_sheet(face));
    }
    hud.set_texture(&device, &queue, LOGO_SHEET, &menu_logo_sheet());
    hud.set_texture(&device, &queue, WIDGETS_SHEET, &menu_widgets_sheet());
    let font = Font::load(&sheet, None).expect("the synthetic sheet is a 16x16 grid");
    (device, queue, target, hud, font)
}

/// Renders the hud's draws over a cleared rows probe and reads the pixels
/// back.
fn menu_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &Target,
    hud: &HudPass,
) -> Vec<u8> {
    let depth = rows_depth(device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide menu headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| hud.draw(pass));
    queue.submit(Some(encoder.finish()));
    read_rows_pixels(device, queue, target)
}

/// One rows-probe pixel's RGB bytes.
fn menu_pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let offset = (y * ROWS_WIDE * 4 + x * 4) as usize;
    [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
}

/// Asserts one rows-probe pixel within two levels per channel, naming it in
/// the failure message.
fn expect_menu_pixel(pixels: &[u8], x: u32, y: u32, want: [u8; 3], what: &str) {
    let got = menu_pixel(pixels, x, y);
    let close = got
        .iter()
        .zip(want)
        .all(|(&got, want)| (i16::from(got) - i16::from(want)).abs() <= 2);
    assert!(close, "{what} at ({x}, {y}): got {got:?}, want {want:?}");
}

/// The menu samples the yaw-facing panorama face: the blit leads the draws
/// under the timer's face key and covers the screen, and the frame shows
/// that face's colour dimmed by the top gradient at two yaws half a
/// revolution apart.
///
/// Faces 0 and 3: timer 0 looks down yaw 0, timer 2100 turns −210 degrees
/// into face 3's sextant. The top gradient (`MENU_GRADIENT_TOP`) blends
/// near-black at 129/255 over the face, so each channel reads
/// `(126 * face + 129) / 255`.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_menu_samples_the_yaw_facing_panorama_face() {
    const FACE_RGB: [[u8; 3]; 6] = [
        [200, 30, 30],
        [30, 200, 30],
        [30, 30, 200],
        [200, 200, 30],
        [200, 30, 200],
        [30, 200, 200],
    ];
    let scaled = scaled_resolution(ROWS_WIDE, ROWS_TALL, 1);
    assert_eq!(
        (scaled.width, scaled.height),
        (ROWS_WIDE, ROWS_TALL),
        "the probe maps one to one"
    );
    let (device, queue, target, mut hud, font) = menu_scene();
    let mut menu =
        MainMenuScreen::new(&scaled, &["Test splash".to_string()], 11, menu_labels(None));
    for (timer, face) in [(0i64, 0usize), (2100, 3)] {
        menu.set_panorama_timer(timer);
        let draws = menu.draws(&font, &scaled, None, 0);
        match draws.first().expect("the panorama blit leads") {
            HudDraw::TexturedRect {
                texture: HudTexture::Named(name),
                x,
                y,
                width,
                height,
                ..
            } => {
                assert_eq!(*name, PANORAMA_SHEETS[face], "yaw {timer} faces {face}");
                assert_eq!(
                    (*x, *y, *width, *height),
                    (0.0, 0.0, ROWS_WIDE as f32, ROWS_TALL as f32),
                    "the blit covers the screen"
                );
            }
            other => panic!("the first draw is the panorama blit, got {other:?}"),
        }
        hud.set_draws(
            &device,
            &queue,
            &draws,
            &TextureRegistry::new(&device, &queue),
        );
        let pixels = menu_frame(&device, &queue, &target, &hud);
        let [r, g, b] = FACE_RGB[face];
        let dim = |channel: u8| ((f32::from(channel) * 126.0 + 129.0) / 255.0).round() as u8;
        expect_menu_pixel(
            &pixels,
            10,
            50,
            [dim(r), dim(g), dim(b)],
            "the dimmed face under the top gradient",
        );
    }
}

/// The menu draws the picked splash yellow at −20 degrees about the
/// translate: the record carries the angle, the colour and the pivot, and
/// the frame inks yellow splash pixels.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_menu_draws_the_yellow_splash_at_minus_twenty() {
    let scaled = scaled_resolution(ROWS_WIDE, ROWS_TALL, 1);
    let (device, queue, target, mut hud, font) = menu_scene();
    let menu = MainMenuScreen::new(&scaled, &["Test splash".to_string()], 11, menu_labels(None));
    let splash = menu.splash().to_string();
    assert!(!splash.is_empty(), "the menu picks a splash");
    let draws = menu.draws(&font, &scaled, None, 0);
    let (angle, colour, pivot, text) = draws
        .iter()
        .find_map(|draw| match draw {
            HudDraw::RotatedText {
                angle_deg,
                colour,
                cx,
                cy,
                text,
                ..
            } => Some((angle_deg, colour, (*cx, *cy), text)),
            _ => None,
        })
        .expect("the splash record");
    assert_eq!(*angle, SPLASH_ANGLE_DEG, "minus twenty about Z");
    assert_eq!(*colour, SPLASH_COLOUR, "yellow");
    assert_eq!(
        pivot,
        (ROWS_WIDE as f32 / 2.0 + 90.0, 70.0),
        "the translate"
    );
    assert_eq!(text, &splash, "the picked splash draws");
    hud.set_draws(
        &device,
        &queue,
        &draws,
        &TextureRegistry::new(&device, &queue),
    );
    let pixels = menu_frame(&device, &queue, &target, &hud);
    let yellow = (230u32..400)
        .flat_map(|x| (30u32..115).map(move |y| (x, y)))
        .filter(|&(x, y)| menu_pixel(&pixels, x, y) == [255, 255, 0])
        .count();
    assert!(
        yellow > 50,
        "the splash inks yellow pixels around the translate: {yellow}"
    );
}

/// The buttons draw their three states: the record's strip row reads
/// `46 + i * 20` per state, and the frame shows the normal, hovered and
/// disabled strips with their label colours.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_menu_buttons_draw_the_three_states() {
    let (device, queue, target, mut hud, font) = menu_scene();
    let up = Button::new(1, 124.0, 40.0, 200.0, 20.0, "A".to_string());
    let over = Button::new(1, 124.0, 100.0, 200.0, 20.0, "A".to_string());
    let mut off = Button::new(1, 124.0, 160.0, 200.0, 20.0, "A".to_string());
    off.enabled = false;
    // The pointer hovers the middle button only.
    let mouse = Some((200.0, 110.0));
    let ups = up.draws(&font, mouse);
    let overs = over.draws(&font, mouse);
    let offs = off.draws(&font, mouse);
    for (draws, row, what) in [
        (&ups, 66.0, "the idle strip"),
        (&overs, 86.0, "the hovered strip"),
        (&offs, 46.0, "the disabled strip"),
    ] {
        match &draws[0] {
            HudDraw::TexturedRect { uv, .. } => {
                assert_eq!(uv[1], row / 256.0, "{what} samples v {row}");
            }
            other => panic!("the first draw is the strip half, got {other:?}"),
        }
    }
    let mut draws = Vec::new();
    draws.extend(ups);
    draws.extend(overs);
    draws.extend(offs);
    hud.set_draws(
        &device,
        &queue,
        &draws,
        &TextureRegistry::new(&device, &queue),
    );
    let pixels = menu_frame(&device, &queue, &target, &hud);
    expect_menu_pixel(&pixels, 130, 45, [200, 200, 200], "the idle strip");
    expect_menu_pixel(&pixels, 130, 105, [235, 235, 235], "the hovered strip");
    expect_menu_pixel(&pixels, 130, 165, [100, 100, 100], "the disabled strip");
    // The label starts centred at half-pixel ink: the probe reads one pixel
    // inside the solid block, clear of the shadow's +1 offset.
    let width = string_width(&font, "A");
    let label_x = (124.0 + 100.0 - f64::from(width) as f32 / 2.0).ceil() as u32 + 1;
    for (top, rgb, what) in [
        (40u32, [224u8, 224, 224], "the idle label"),
        (100u32, [255u8, 255, 160], "the hovered label"),
        (160u32, [160u8, 160, 160], "the disabled label"),
    ] {
        expect_menu_pixel(&pixels, label_x, top + 7, rgb, what);
    }
}
