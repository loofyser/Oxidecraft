//! Local-only tests of the multiplayer server list through the real overlay
//! pipeline: they need a real adapter, so they are ignored by default. Run
//! them on a machine with a GPU:
//!
//! ```text
//! cargo test -p oxide-client --test multiplayer_headless -- --ignored --nocapture
//! ```
//!
//! The list's draws are the client's own (`MultiplayerScreen::draws`, one
//! row's draws); the hud pass that renders them is the renderer's. That
//! direction — the client driving the renderer — is why these cases live
//! here and not beside the renderer's own pipeline probes: the crate graph
//! gives `oxide-render` no edge to `oxide-client`.
//!
//! Every sheet here is built by hand in the test — flat colours, never a
//! pixel from the asset store — and uploaded through `HudPass::set_texture`,
//! so these are the only tests that run the real sampler over the list's
//! draws. The icons sheet carries one flat band per latency level at the
//! source's own rows (`176 + l * 8`), so each frame's bar pixel names the
//! level it sampled; the styled-MOTD scan reads the grey and the red runs
//! the `§`-coded MOTD inks.

use std::sync::mpsc;
use std::task::{Context, Poll, Wake, Waker};

use oxide_assets::font::Font;
use oxide_assets::texture::Texture;
use oxide_client::screens::multiplayer::{
    ICONS_SHEET, LIST_TOP, MultiplayerScreen, PingOutcome, PingResult, ServerEntry, ServerList,
    multiplayer_labels,
};
use oxide_client::screens::widgets::{Button, WIDGETS_SHEET};
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
        label: Some("oxide multiplayer headless device"),
        ..Default::default()
    }))
    .expect("a device is created")
}

/// The probe target's width in texels; the readback row stride (448 × 4)
/// stays a multiple of 256 bytes.
const ROWS_WIDE: u32 = 448;
/// The probe target's height in texels: one row plus the title and buttons.
const ROWS_TALL: u32 = 240;

/// The offscreen colour target at the probe's size.
fn rows_target(device: &wgpu::Device, format: wgpu::TextureFormat) -> Target {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("oxide multiplayer headless target"),
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

/// The offscreen colour target: the texture the pixels are read back from
/// and the view the passes render through.
struct Target {
    /// The texture itself.
    texture: wgpu::Texture,
    /// A view of the whole texture.
    view: wgpu::TextureView,
}

/// The depth texture at the probe's size.
fn rows_depth(device: &wgpu::Device) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("oxide multiplayer headless depth"),
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
        label: Some("oxide multiplayer headless readback"),
        size: u64::from(ROW) * u64::from(ROWS_TALL),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide multiplayer headless readback encoder"),
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

/// Runs a pass that only clears the target to `colour`, with no depth
/// attachment, so a hud pass can follow and blend over the result.
fn with_clear_pass(
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    colour: wgpu::Color,
) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("oxide multiplayer headless clear pass"),
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

/// Runs the overlay pass over the target: the colour the clear pass left,
/// loaded, and the depth buffer cleared and attached, the state the hud's
/// pipelines state their depth against.
fn with_overlay_pass(
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    draw: impl FnOnce(&mut wgpu::RenderPass<'_>),
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("oxide multiplayer headless overlay pass"),
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

/// The list probe's synthetic font sheet: 128x128 fully inked white, so
/// every printable cell spans its whole 8x8 and all label and MOTD text
/// draws as solid blocks at known advances.
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn list_font_sheet() -> Texture {
    Texture {
        width: 128,
        height: 128,
        rgba: vec![255u8; 128 * 128 * 4],
    }
}

/// The synthetic widgets sheet: the three button strips as flat rows — the
/// disabled strip at `v` 46, the normal strip at 66, the hovered strip at 86
/// (`GuiButton.drawButton`'s `46 + i * 20`).
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn list_widgets_sheet() -> Texture {
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

/// One latency level's flat band colour, level 0..=5.
const BAND: [[u8; 3]; 6] = [
    [200, 30, 30],
    [30, 200, 30],
    [30, 30, 200],
    [200, 200, 30],
    [200, 30, 200],
    [30, 200, 200],
];

/// The synthetic icons sheet: one flat band per latency level at the
/// source's own rows, `176 + l * 8` (`ServerListEntryNormal.java`,
/// `drawEntry`), so a frame's bar pixel names the level it sampled.
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn list_icons_sheet() -> Texture {
    const SIDE: u32 = 256;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    for (level, colour) in BAND.iter().enumerate() {
        let top = 176 + level as u32 * 8;
        for y in top..top + 8 {
            for x in 0..SIDE {
                let at = ((y * SIDE + x) * 4) as usize;
                rgba[at] = colour[0];
                rgba[at + 1] = colour[1];
                rgba[at + 2] = colour[2];
                rgba[at + 3] = 255;
            }
        }
    }
    Texture {
        width: SIDE,
        height: SIDE,
        rgba,
    }
}

/// The list probe: the rows-sized target with the font, the widgets and the
/// icons registered under the keys the list's draws name.
fn list_scene() -> (wgpu::Device, wgpu::Queue, Target, HudPass, Font) {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = rows_target(&device, format);
    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, ROWS_WIDE as f32, ROWS_TALL as f32);
    let sheet = list_font_sheet();
    hud.set_font(&device, &queue, &sheet)
        .expect("the synthetic sheet is a 16x16 grid");
    hud.set_texture(&device, &queue, WIDGETS_SHEET, &list_widgets_sheet());
    hud.set_texture(&device, &queue, ICONS_SHEET, &list_icons_sheet());
    let font = Font::load(&sheet, None).expect("the synthetic sheet is a 16x16 grid");
    (device, queue, target, hud, font)
}

/// Renders the hud's draws over a cleared rows probe and reads the pixels
/// back.
fn list_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &Target,
    hud: &HudPass,
) -> Vec<u8> {
    let depth = rows_depth(device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide multiplayer headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| hud.draw(pass));
    queue.submit(Some(encoder.finish()));
    read_rows_pixels(device, queue, target)
}

/// One rows-probe pixel's RGB bytes.
fn list_pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let offset = (y * ROWS_WIDE * 4 + x * 4) as usize;
    [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
}

/// Asserts one rows-probe pixel within two levels per channel, naming it in
/// the failure message.
fn expect_list_pixel(pixels: &[u8], x: u32, y: u32, want: [u8; 3], what: &str) {
    let got = list_pixel(pixels, x, y);
    let close = got
        .iter()
        .zip(want)
        .all(|(&got, want)| (i16::from(got) - i16::from(want)).abs() <= 2);
    assert!(close, "{what} at ({x}, {y}): got {got:?}, want {want:?}");
}

/// The canned coloured MOTD behind the styled entry: plain grey words, then
/// red words, through the standing `parse_json → to_legacy` path.
const STYLED_MOTD_RAW: &str = r#"{"text":"Plain ","extra":[{"text":"red words","color":"red"}]}"#;

/// The C01/S01 latencies each answered frame carries, one per bar level 0..=4.
const LEVEL_LATENCY: [i64; 5] = [10, 200, 400, 800, 5000];

/// The server list draws its styled MOTD and every latency bar: one frame
/// per bar level 0..=5. Each frame's bar record samples the icons sheet at
/// the level's own row, and the frame shows that row's flat band; the first
/// frame's MOTD line inks both the unstyled grey run and the `§c` red run.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_server_list_draws_its_styled_motd_and_every_latency_bar() {
    let scaled = scaled_resolution(ROWS_WIDE, ROWS_TALL, 1);
    assert_eq!(
        (scaled.width, scaled.height),
        (ROWS_WIDE, ROWS_TALL),
        "the probe maps one to one"
    );
    let (device, queue, target, mut hud, font) = list_scene();
    let mut list = ServerList::default();
    list.add(ServerEntry::new(
        "Test server".to_string(),
        "127.0.0.1:25565".to_string(),
        None,
    ));
    let path = std::env::temp_dir().join("oxide-multiplayer-headless-servers.toml");
    let mut screen = MultiplayerScreen::open(list, path, multiplayer_labels(None));
    // The list's own buttons draw without a row selected: the idle strip
    // proves the widgets sheet is live under the list too.
    let idle = Button::new(1, 124.0, 200.0, 200.0, 20.0, "Join".to_string());
    let idle_draws = idle.draws(&font, None);
    match &idle_draws[0] {
        HudDraw::TexturedRect { uv, .. } => {
            assert_eq!(uv[1], 66.0 / 256.0, "the idle strip samples v 66");
        }
        other => panic!("the first draw is the strip half, got {other:?}"),
    }

    for level in 0..=5u8 {
        if level < 5 {
            screen.apply_ping(PingResult {
                index: 0,
                outcome: PingOutcome::Answered {
                    motd_raw: Some(STYLED_MOTD_RAW.to_string()),
                    online: 3,
                    max: 20,
                    player_list: None,
                    version_name: String::from("1.8.9"),
                    protocol: oxide_proto_v47::PROTOCOL,
                    icon: None,
                    latency_ms: LEVEL_LATENCY[usize::from(level)],
                },
            });
        } else {
            screen.apply_ping(PingResult {
                index: 0,
                outcome: PingOutcome::Unreachable,
            });
        }
        let draws = screen.draws(&font, &scaled, None, 0);
        // The bar record: the icons sheet at `(0, 176 + l * 8)`, 10x8, at the
        // row's right edge.
        let bar = draws
            .iter()
            .find_map(|draw| match draw {
                HudDraw::TexturedRect {
                    texture: HudTexture::Named(name),
                    x,
                    y,
                    width,
                    height,
                    uv,
                    ..
                } if *name == ICONS_SHEET => Some((*x, *y, *width, *height, *uv)),
                _ => None,
            })
            .expect("the latency bar draws");
        assert_eq!(
            (bar.0, bar.1, bar.2, bar.3),
            (ROWS_WIDE as f32 - 15.0, LIST_TOP as f32, 10.0, 8.0),
            "level {level}: the bar cell stands at the row's right edge"
        );
        assert_eq!(
            bar.4,
            [
                0.0,
                (176.0 + f32::from(level) * 8.0) / 256.0,
                10.0 / 256.0,
                (176.0 + f32::from(level) * 8.0 + 8.0) / 256.0,
            ],
            "level {level}: the bar samples its own sheet row"
        );
        hud.set_draws(
            &device,
            &queue,
            &draws,
            &TextureRegistry::new(&device, &queue),
        );
        let pixels = list_frame(&device, &queue, &target, &hud);
        expect_list_pixel(
            &pixels,
            ROWS_WIDE - 15 + 5,
            LIST_TOP as u32 + 4,
            BAND[usize::from(level)],
            "the level {level} bar",
        );
        if level == 0 {
            // The styled MOTD draws: the record carries the `§c` run the
            // coloured description renders through.
            let motd = draws
                .iter()
                .find_map(|draw| match draw {
                    HudDraw::Text { text, .. } if text.contains("red words") => Some(text.clone()),
                    _ => None,
                })
                .expect("the styled MOTD draws");
            assert!(
                motd.contains("§c"),
                "the coloured run keeps its code: {motd:?}"
            );
            // The frame inks both runs on the MOTD line: the unstyled grey
            // and the `§c` red, over the 9-tall line at `row + 12`.
            let top = LIST_TOP as u32 + 12;
            let mut grey = 0u32;
            let mut red = 0u32;
            for y in top..top + 9 {
                for x in 35..160 {
                    let [r, g, b] = list_pixel(&pixels, x, y);
                    let spread = r.max(g).max(b) - r.min(g).min(b);
                    if (100..=160).contains(&r) && spread <= 12 {
                        grey += 1;
                    }
                    if r >= 200 && g <= 120 && b <= 120 {
                        red += 1;
                    }
                }
            }
            println!("styled MOTD pixels: grey={grey} red={red}");
            assert!(grey > 150, "the unstyled run inks grey pixels: {grey}");
            assert!(red > 250, "the §c run inks red pixels: {red}");
        }
    }
}
