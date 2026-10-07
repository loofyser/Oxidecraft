//! Local-only test of the mesher's barrier output through the terrain pass: it needs a real
//! adapter, so it is ignored by default. Run it on a machine with a GPU:
//!
//! ```text
//! cargo test -p oxide-game --test barrier_headless -- --ignored --nocapture
//! ```
//!
//! The mesh is the mesher's own — a column carrying a stone block beside a barrier cell,
//! meshed by [`build_column_meshes`] over a synthetic model tree — so the case consumes the
//! real output: the barrier emits nothing at all, not the fallback cube. The atlas is built
//! by hand here (a few texels of generated colour, never a pixel from the asset store): the
//! fallback sprite is magenta and the stone's sprite white, so a sample names itself. The
//! frame must carry zero magenta pixels — the acceptance's own mask, equal red and blue with
//! no green — and the pixels where the barrier's cell would have drawn must read the stone
//! or the sky instead.
//!
//! This is the end of the chain the live boss frames showed broken: with the barrier's row
//! absent from the behaviour table, the mesher draws the fallback cube over the missing
//! sprite and the magenta returns — the cage's walls rendered as solid blocks. The terrain
//! pass and the device plumbing mirror `oxide-render`'s own test pattern
//! (`pipeline_headless.rs`); the case lives here because the mesh it must consume is
//! `oxide-game`'s, and the crate graph gives `oxide-render` no edge to reach it.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::mpsc;
use std::task::{Context, Poll, Wake, Waker};

use oxide_assets::atlas::{Atlas, AtlasLevel, AtlasSprite, SpriteRect};
use oxide_assets::model::ModelSource;
use oxide_game::mesher::{
    BlockModelSet, ColumnSnapshot, MeshContext, SmoothLighting, build_column_meshes,
};
use oxide_proto_v47::column::{ColumnData, SectionData};
use oxide_render::camera::{
    Camera, CameraPose, DEFAULT_FOV, EYE_HEIGHT, NEAR_PLANE, NO_VIEW_EFFECT,
};
use oxide_render::renderer::SKY_COLOR;
use oxide_render::terrain_pass::{DEPTH_FORMAT, TerrainPass};
use oxide_world::biome::{ColorMap, TintMaps};
use oxide_world::chunk::SECTION_COUNT;
use oxide_world::world::World;

/// The size of the offscreen target in texels.
const SIZE: u32 = 64;
/// The readback row stride: a row must be a multiple of 256 bytes, and 64 RGBA texels are
/// exactly that.
const BYTES_PER_ROW: u32 = 256;
/// The sky colour as 8-bit unorm bytes: 0.62, 0.76 and 0.98 of 255, rounded.
const SKY: [u8; 3] = [158, 194, 250];
/// The barrier's id.
const BARRIER: u16 = 166;
/// The stone's id.
const STONE: u16 = 1;

#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_meshers_barrier_draws_no_geometry_and_no_magenta() {
    // The mesher's own output: a stone block at (0, 1, 1) in full daylight with a barrier
    // cell beside it at (0, 1, 2), meshed through the real model set. The barrier has no
    // file to resolve — the client builds it in — and its row routes it around the fallback
    // cube too, so the section's geometry is the stone's six faces alone.
    let tree = barrier_tree();
    let models = BlockModelSet::load(&tree.source());
    let atlas = barrier_atlas();
    let maps = white_maps();
    let ctx = MeshContext {
        models: &models,
        atlas: &atlas,
        tint_maps: &maps,
        graphics_fast: true,
        smooth_lighting: SmoothLighting::Off,
    };
    let mut world = World::new(true);
    world.apply_column(
        0,
        0,
        &column(&[(0, 1, 1, state(STONE, 0)), (0, 1, 2, state(BARRIER, 0))]),
        true,
    );
    let snapshot = ColumnSnapshot::from_world(&world, 0, 0);
    let mesh = build_column_meshes(&snapshot, &ctx)
        .into_iter()
        .find_map(|(_, mesh)| mesh)
        .expect("the stone's section meshes");

    // Render the mesh through the terrain pass with the camera looking north at the stone.
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);
    let depth = create_depth(&device);
    let mut terrain = TerrainPass::new(&device, &queue, format);
    terrain.set_atlas(&device, &queue, &atlas);
    terrain.set_camera(&queue, stone_camera(), 1.0);
    // The vertices are world-space; the key names the section they stand in (chunk 0, 0,
    // section 0), which the camera sits inside.
    terrain.upload(&device, &queue, (0, 0, 0), &mesh);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide barrier headless encoder"),
    });
    with_terrain_pass(&mut encoder, &target.view, &depth, |pass| {
        terrain.draw(pass)
    });
    queue.submit(Some(encoder.finish()));
    let pixels = read_pixels(&device, &queue, &target);

    // The acceptance's own magenta mask: equal red and blue, no green, bright. The fallback
    // cube's samples of the missing sprite would light up here — the barrier's five visible
    // faces' worth of them, a solid cage wall in the live frames.
    let magenta = pixels
        .chunks_exact(4)
        .filter(|pixel| pixel[0] == pixel[2] && pixel[1] == 0 && pixel[0] > 100)
        .count();
    assert_eq!(magenta, 0, "the fallback sprite reached the frame");

    // The frame's centre: the stone's south face at z = 2 — the block's own sprite, times
    // the south face's brightness 204, times the lightmap's cell for the mesher's daylight
    // pair, block 0 and sky 15, which reads back 250: 255 * 204/255 * 250/255 = 200 — where
    // the barrier's fallback cube would put the missing sprite instead.
    expect_texel(
        &pixels,
        SIZE / 2,
        SIZE / 2,
        [200, 200, 200, 255],
        "the stone's south face",
    );
    // Above the centre: the sky. The ray clears the stone's top edge — the eye sits at the
    // stone's mid-height, so the top face itself is not in this frame — and the barrier's
    // cube, which would fill this pixel with its own face, is gone.
    expect_pixel(
        &pixels,
        SIZE / 2,
        SIZE / 2 - 12,
        SKY,
        "the sky over the stone",
    );
    // Below the centre: the sky again. The ray passes under the stone's bottom edge, and
    // the barrier's cube, which would reach this pixel too, is gone.
    expect_pixel(
        &pixels,
        SIZE / 2,
        SIZE / 2 + 12,
        SKY,
        "the sky under the stone",
    );

    // And the geometry itself: the stone's six faces alone, nothing inside the barrier's
    // cell (x 0..1, z 2..3, y 1..2), and every vertex at or before the stone's own south
    // plane.
    assert_eq!(mesh.vertex_count(), 24, "the stone's six faces alone");
    for vertex in mesh.layers.iter().flat_map(|layer| layer.vertices.iter()) {
        assert!(
            vertex.position[2] <= 2.0,
            "no vertex in the barrier's cell: {:?}",
            vertex.position
        );
    }
}

// -- the mesh's inputs ------------------------------------------------------

/// One wire block value: the id and its metadata.
fn state(id: u16, meta: u8) -> u16 {
    (id << 4) | u16::from(meta)
}

/// One column carrying `blocks` in full daylight, no block light: sky 15 and block 0 in every
/// cell, one plains biome.
fn column(blocks: &[(usize, usize, usize, u16)]) -> ColumnData {
    let mut sections: [Option<SectionData>; SECTION_COUNT] = std::array::from_fn(|_| None);
    for (index, slot) in sections.iter_mut().enumerate() {
        let mut grid = Box::new([0u16; 4096]);
        for (x, y, z, id) in blocks {
            if y >> 4 == index {
                grid[((y & 15) << 8) | (z << 4) | x] = *id;
            }
        }
        *slot = Some(SectionData {
            blocks: grid,
            block_light: Box::new([0u8; 2048]),
            sky_light: Some(Box::new([0xFFu8; 2048])),
        });
    }
    let biomes = std::array::from_fn(|_| 1u8);
    ColumnData {
        mask: 0xFFFF,
        sections,
        biomes: Some(biomes),
    }
}

/// Neutral colour maps: white everywhere.
fn white_maps() -> TintMaps {
    let neutral = || ColorMap::from_rgba(&[255u8; 256 * 256 * 4]).expect("a valid map");
    TintMaps {
        grass: neutral(),
        foliage: neutral(),
    }
}

/// A synthetic extraction tree, laid out as the project's extractor emits it: the stone's
/// full-cube model and the blockstate arm that names it. The barrier needs no files: the
/// client builds it in.
struct Tree {
    root: tempfile::TempDir,
}

impl Tree {
    fn new() -> Tree {
        let root = tempfile::tempdir().expect("a temporary directory");
        for directory in ["models/block", "blockstates"] {
            std::fs::create_dir_all(root.path().join("assets/minecraft").join(directory))
                .expect("the tree's directories");
        }
        Tree { root }
    }

    fn write(&self, name: &str, json: &str) {
        std::fs::write(
            self.root
                .path()
                .join("assets/minecraft")
                .join(format!("{name}.json")),
            json,
        )
        .expect("a tree file");
    }

    fn source(&self) -> ModelSource {
        ModelSource::open(self.root.path()).expect("the tree opens")
    }
}

/// The stone's own files: a full cube whose six faces each cull against their own side, and
/// the `normal` arm naming it. No barrier file exists, so the barrier's states stay the
/// missing choice — the row's `Invisible` kind, not the fallback cube, answers them.
fn barrier_tree() -> Tree {
    let tree = Tree::new();
    tree.write(
        "models/block/probe_cube",
        r##"{"textures": {"all": "blocks/probe"}, "elements": [
            {"from": [0, 0, 0], "to": [16, 16, 16], "faces": {
                "down": {"texture": "#all", "cullface": "down"},
                "up": {"texture": "#all", "cullface": "up"},
                "north": {"texture": "#all", "cullface": "north"},
                "south": {"texture": "#all", "cullface": "south"},
                "west": {"texture": "#all", "cullface": "west"},
                "east": {"texture": "#all", "cullface": "east"}}}]}"##,
    );
    tree.write(
        "blockstates/stone",
        r#"{"variants": {"normal": [{"model": "minecraft:probe_cube"}]}}"#,
    );
    tree
}

/// The atlas the case draws with: 16 x 16 texels, the left half magenta — the missing
/// sprite, the acceptance's magenta family — and the right half white, the stone's own
/// sprite, so a sample names itself.
///
/// The texels are generated here; no asset store is read and no Mojang pixel is embedded.
fn barrier_atlas() -> Atlas {
    let mut rgba = Vec::with_capacity(16 * 16 * 4);
    for _ in 0..16 {
        for x in 0..16 {
            if x < 8 {
                rgba.extend_from_slice(&[255, 0, 255, 255]);
            } else {
                rgba.extend_from_slice(&[255, 255, 255, 255]);
            }
        }
    }
    let sprite = |x: u32| AtlasSprite {
        region: SpriteRect {
            x,
            y: 0,
            w: 8,
            h: 8,
        },
        content: SpriteRect {
            x,
            y: 0,
            w: 8,
            h: 8,
        },
    };
    let missing = sprite(0);
    Atlas {
        levels: vec![AtlasLevel {
            width: 16,
            height: 16,
            rgba,
        }],
        width: 16,
        height: 16,
        level_count: 1,
        sprites: BTreeMap::from([
            ("missingno".to_string(), missing),
            ("blocks/probe".to_string(), sprite(8)),
        ]),
        animated: BTreeMap::new(),
        missing,
    }
}

/// The camera the case looks at the stone with: the eye at y 1.5, the stone's mid-height,
/// two and a half units south of the stone's south face, looking north (yaw 180, pitch 0),
/// so the frame's centre ray meets the stone's south face at its middle.
fn stone_camera() -> Camera {
    Camera {
        pose: CameraPose {
            position: [0.5, 1.5 - f64::from(EYE_HEIGHT), 4.5],
            yaw: 180.0,
            pitch: 0.0,
            sneak: false,
        },
        fov_degrees: DEFAULT_FOV,
        near: NEAR_PLANE,
        far_chunks: 8.0,
        view_effect: NO_VIEW_EFFECT,
    }
}

// -- the device and the readback --------------------------------------------

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

/// Builds the headless device and queue the case renders with.
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
        label: Some("oxide barrier headless device"),
        ..Default::default()
    }))
    .expect("a device is created")
}

/// The offscreen colour target: the texture the pixels are read back from and the view the
/// pass renders through.
struct Target {
    /// The texture itself.
    texture: wgpu::Texture,
    /// A view of the whole texture.
    view: wgpu::TextureView,
}

/// Creates the offscreen colour target in `format`.
fn create_target(device: &wgpu::Device, format: wgpu::TextureFormat) -> Target {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("oxide barrier headless target"),
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
        label: Some("oxide barrier headless depth"),
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
        label: Some("oxide barrier headless terrain pass"),
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

/// Reads the whole target back as RGBA bytes, `BYTES_PER_ROW` per row.
fn read_pixels(device: &wgpu::Device, queue: &wgpu::Queue, target: &Target) -> Vec<u8> {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("oxide barrier headless readback"),
        size: u64::from(BYTES_PER_ROW) * u64::from(SIZE),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide barrier headless readback encoder"),
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

/// The RGBA bytes of one pixel of a readback.
fn pixel_rgba(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let offset = (y * BYTES_PER_ROW + x * 4) as usize;
    [
        pixels[offset],
        pixels[offset + 1],
        pixels[offset + 2],
        pixels[offset + 3],
    ]
}

/// The RGB bytes of one pixel of a readback.
fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let [r, g, b, _] = pixel_rgba(pixels, x, y);
    [r, g, b]
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

/// Asserts one pixel against a texel byte for byte, alpha included.
fn expect_texel(pixels: &[u8], x: u32, y: u32, want: [u8; 4], what: &str) {
    let got = pixel_rgba(pixels, x, y);
    assert_eq!(got, want, "{what} at ({x}, {y})");
}
