//! Local-only tests of the textured terrain and overlay pipelines: they need a real adapter,
//! so they are ignored by default. Run them on a machine with a GPU:
//!
//! ```text
//! cargo test -p oxide-render --test pipeline_headless -- --ignored --nocapture
//! ```
//!
//! Every atlas here is built by hand in the test — a few texels of generated colour, never a
//! pixel from the asset store — and uploaded through `TerrainPass::set_atlas`, so these are
//! the only tests that run the real sampler and the real layer pipelines over a framebuffer.
//!
//! The first test renders one stone block's mesh from a camera looking down at it and reads
//! the pixels back: the centre must be the stone colour and a corner must still be the sky.
//! The mesh carries a buried face as well, wound the same way and indexed after the block's
//! faces, so a pipeline that lost its depth test, its depth write or its depth attachment
//! draws it over the top face and the centre turns the buried colour instead. The atlas is a
//! solid white stand-in, so a texel contributes nothing and the vertex colours come back as
//! they were before the atlas existed.
//!
//! The read-back pins the winding convention too: the faces are counter-clockwise seen from
//! outside and the pipelines cull back faces, so a face wound the other way, or a pipeline
//! that culls front faces instead, drops the top face and shows the far side of the block at
//! the centre — brightness 0.8 where the top face's 1.0 is wanted — and the colour check
//! fails. (Culling nothing at all would still draw the same picture, so `terrain_pass`'s unit
//! test pins the cull mode itself.)
//!
//! The second test renders the same block, then draws a line through the overlay pass over it
//! and checks the glyph pixels and the one-pixel shadow offset against the layout
//! `debug_text` produces.
//!
//! The atlas tests come next: the texels of a four-colour atlas are read back byte-identical
//! (the reference check for the non-sRGB colour-space decision: a stored byte must arrive as
//! the same byte), a magnified sample picks the nearer of two texels, the translucent layer
//! blends its 50%-alpha fragment over the opaque layer's colour exactly as
//! `src_alpha / one_minus_src_alpha` says, a cutout fragment whose texel alpha is zero is
//! discarded so the surface behind it shows through, a translucent fragment whose alpha sits
//! below the client's tenth is discarded the same way, and an atlas with three mip levels is
//! read back at its first and last level, so the level-by-level uploader is pinned beyond
//! level 0.

use std::collections::BTreeMap;
use std::sync::mpsc;
use std::task::{Context, Poll, Wake, Waker};

use oxide_assets::atlas::{Atlas, AtlasLevel, AtlasSprite, SpriteRect};
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
/// The stone colour the block's vertices carry: multiplied by the white stand-in atlas's
/// texel, so the block still reads as stone grey.
const STONE_COLOUR: [f32; 3] = [0.5, 0.5, 0.5];
/// The buried face's colour, which is no real texture's at all.
const BURIED_COLOUR: [f32; 3] = [1.0, 0.0, 0.0];
/// The packed light of a full-sky corner, both channels: level 15 shifted four bits with the
/// sampler's eight added.
const FULL_SKY: u16 = 248;
/// The four texels of the atlas the read-back and magnification tests use, row-major with the
/// first row at the top: red and green on the top row, blue and white below.
const FOUR_TEXELS: [[u8; 4]; 4] = [
    [255, 0, 0, 255],
    [0, 255, 0, 255],
    [0, 0, 255, 255],
    [255, 255, 255, 255],
];
/// The atlas the cutout test uses: a fully transparent red texel, an opaque green one, and
/// two more that only fill the image out.
const CUTOUT_TEXELS: [[u8; 4]; 4] = [
    [255, 0, 0, 0],
    [0, 255, 0, 255],
    [0, 0, 255, 255],
    [255, 255, 255, 255],
];
/// The atlas the translucent-discard test uses: a red texel whose alpha, 24/255, sits below
/// the client's 0.1 threshold, an opaque green one, and two more that only fill the image
/// out.
const TRANSLUCENT_TEXELS: [[u8; 4]; 4] = [
    [255, 0, 0, 24],
    [0, 255, 0, 255],
    [0, 0, 255, 255],
    [255, 255, 255, 255],
];
/// The three levels of the multi-level atlas, one flat colour each: a 16x16 red level 0, an
/// 8x8 green level 1 and a 4x4 blue level 2, so a read-back names the level it sampled.
const LEVEL_TEXELS: [[u8; 4]; 3] = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]];

#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_terrain_pass_draws_the_block_and_keeps_the_buried_face_hidden() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);
    let depth = create_depth(&device);

    let mut terrain = TerrainPass::new(&device, format);
    terrain.set_atlas(&device, &queue, &solid_atlas(16, [255, 255, 255, 255]));
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
    terrain.set_atlas(&device, &queue, &solid_atlas(16, [255, 255, 255, 255]));
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

#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_atlas_texels_come_back_byte_identical() {
    let (device, queue) = headless_device();
    let mut mesh = ChunkMesh::default();
    push_quad(
        &mut mesh,
        Layer::Opaque,
        covering_quad(2.0, [[0.0, 0.0], [1.0, 1.0]]),
        [255, 255, 255, 255],
    );
    let pixels = render_terrain(&device, &queue, &atlas(2, 2, &FOUR_TEXELS), &mesh);

    // One pixel per texel, each sampling the middle of its quadrant. The bytes must match the
    // uploaded texel exactly: this is the reference check for the non-sRGB policy, which is
    // also what makes the shader's colour multiplies match the client's.
    expect_texel(&pixels, 16, 16, FOUR_TEXELS[0], "the top-left texel");
    expect_texel(&pixels, 48, 16, FOUR_TEXELS[1], "the top-right texel");
    expect_texel(&pixels, 16, 48, FOUR_TEXELS[2], "the bottom-left texel");
    expect_texel(&pixels, 48, 48, FOUR_TEXELS[3], "the bottom-right texel");
}

#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn nearest_magnification_samples_the_nearer_texel() {
    let (device, queue) = headless_device();
    // The quad's uv runs from the centre of the atlas's left texel to the centre of its right
    // one on a constant row, so every pixel samples a point halfway between two magnified
    // texels or nearer to one of them. A nearest filter must return that texel's own colour,
    // never a blend of the two.
    let mut mesh = ChunkMesh::default();
    push_quad(
        &mut mesh,
        Layer::Opaque,
        covering_quad(2.0, [[0.25, 0.25], [0.75, 0.25]]),
        [255, 255, 255, 255],
    );
    let pixels = render_terrain(&device, &queue, &atlas(2, 2, &FOUR_TEXELS), &mesh);

    expect_texel(
        &pixels,
        16,
        32,
        FOUR_TEXELS[0],
        "the pixel nearer the left texel",
    );
    expect_texel(
        &pixels,
        48,
        32,
        FOUR_TEXELS[1],
        "the pixel nearer the right texel",
    );
}

#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_translucent_layer_blends_over_the_opaque_one() {
    let (device, queue) = headless_device();
    let mut mesh = ChunkMesh::default();
    // A green opaque quad three units out, and a red half-transparent one in front of it.
    // The white stand-in atlas carries the colours, so the blend's own arithmetic is what the
    // read-back shows: 50% of red over green is (128, 127, 0) byte-exact.
    push_quad(
        &mut mesh,
        Layer::Opaque,
        covering_quad(3.0, [[0.0, 0.0], [1.0, 1.0]]),
        [0, 255, 0, 255],
    );
    push_quad(
        &mut mesh,
        Layer::Translucent,
        covering_quad(2.0, [[0.0, 0.0], [1.0, 1.0]]),
        [255, 0, 0, 128],
    );
    let pixels = render_terrain(
        &device,
        &queue,
        &solid_atlas(4, [255, 255, 255, 255]),
        &mesh,
    );

    expect_pixel_exact(
        &pixels,
        SIZE / 2,
        SIZE / 2,
        [128, 127, 0],
        "the blended centre",
    );
    // The translucent quad covers the whole frame, so a pixel well away from the centre is
    // the same blend: the result does not depend on where in the quad it lands.
    expect_pixel_exact(&pixels, 16, 48, [128, 127, 0], "a blended pixel off centre");
}

#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_cutout_layer_discards_the_zero_alpha_texel() {
    let (device, queue) = headless_device();
    let mut mesh = ChunkMesh::default();
    // A white opaque quad behind everything, then a cutout quad over it whose left half
    // samples a fully transparent red texel and whose right half samples an opaque green one.
    // The discarded half must leave the white surface behind it visible; a layer without the
    // discard would write the transparent texel's red instead.
    push_quad(
        &mut mesh,
        Layer::Opaque,
        covering_quad(3.0, [[0.75, 0.75], [0.75, 0.75]]),
        [255, 255, 255, 255],
    );
    push_quad(
        &mut mesh,
        Layer::Cutout,
        covering_quad(2.0, [[0.25, 0.25], [0.75, 0.25]]),
        [255, 255, 255, 255],
    );
    let pixels = render_terrain(&device, &queue, &atlas(2, 2, &CUTOUT_TEXELS), &mesh);

    expect_pixel(
        &pixels,
        16,
        32,
        [255, 255, 255],
        "the surface behind the discarded fragment",
    );
    expect_texel(
        &pixels,
        48,
        32,
        CUTOUT_TEXELS[1],
        "the fragment the cutout layer keeps",
    );
}

#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_translucent_layer_discards_the_below_threshold_texel() {
    let (device, queue) = headless_device();
    let mut mesh = ChunkMesh::default();
    // A white opaque quad behind everything, then a translucent quad over it whose left half
    // samples a texel with alpha 24/255 — below the client's 0.1 alpha test — and whose right
    // half samples an opaque green one. The discarded half must leave the white surface
    // behind it visible; a translucent layer without the alpha test would blend the texel
    // anyway and tint that pixel (255, 231, 231).
    push_quad(
        &mut mesh,
        Layer::Opaque,
        covering_quad(3.0, [[0.75, 0.75], [0.75, 0.75]]),
        [255, 255, 255, 255],
    );
    push_quad(
        &mut mesh,
        Layer::Translucent,
        covering_quad(2.0, [[0.25, 0.25], [0.75, 0.25]]),
        [255, 255, 255, 255],
    );
    let pixels = render_terrain(&device, &queue, &atlas(2, 2, &TRANSLUCENT_TEXELS), &mesh);

    expect_pixel(
        &pixels,
        16,
        32,
        [255, 255, 255],
        "the surface behind the discarded translucent fragment",
    );
    expect_texel(
        &pixels,
        48,
        32,
        TRANSLUCENT_TEXELS[1],
        "the translucent fragment above the threshold",
    );
}

#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_multi_level_atlas_reads_back_every_level_it_writes() {
    let (device, queue) = headless_device();
    // A magnified view of the three-level atlas: each fragment's level of detail sits far
    // below the chain, so the sampler reads level 0 and the read-back is its red exactly.
    let mut magnified = ChunkMesh::default();
    push_quad(
        &mut magnified,
        Layer::Opaque,
        covering_quad(2.0, [[0.0, 0.0], [1.0, 1.0]]),
        [255, 255, 255, 255],
    );
    let pixels = render_terrain(&device, &queue, &levelled_atlas(), &magnified);
    expect_texel(&pixels, 16, 32, LEVEL_TEXELS[0], "the magnified level 0");

    // The same atlas sampled 32 times over: every fragment's level of detail passes the
    // chain's deepest level, so the sampler clamps there and the read-back is level 2's blue
    // exactly. A level written with the wrong size, row pitch or order — or skipped and left
    // as wgpu initialised it — reads black or garbage instead.
    let mut minified = ChunkMesh::default();
    push_quad(
        &mut minified,
        Layer::Opaque,
        covering_quad(2.0, [[0.0, 0.0], [32.0, 32.0]]),
        [255, 255, 255, 255],
    );
    let pixels = render_terrain(&device, &queue, &levelled_atlas(), &minified);
    expect_texel(
        &pixels,
        16,
        32,
        LEVEL_TEXELS[2],
        "the clamped deepest level",
    );
    expect_texel(
        &pixels,
        48,
        16,
        LEVEL_TEXELS[2],
        "another pixel of the same level",
    );
}

/// Renders one mesh through the terrain pass with one atlas and reads the frame back.
fn render_terrain(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    atlas: &Atlas,
    mesh: &ChunkMesh,
) -> Vec<u8> {
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(device, format);
    let depth = create_depth(device);
    let mut terrain = TerrainPass::new(device, format);
    terrain.set_atlas(device, queue, atlas);
    terrain.set_camera(queue, frame_camera(), 1.0);
    // The vertices are world-space; the key only decides which draw entry carries them, and
    // it is a section the frustum keeps (chunk -1, -1, section 0).
    terrain.upload(device, queue, (-1, -1, 0), mesh);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide pipeline headless encoder"),
    });
    with_terrain_pass(&mut encoder, &target.view, &depth, |pass| {
        terrain.draw(pass)
    });
    queue.submit(Some(encoder.finish()));
    read_pixels(device, queue, &target)
}

/// A hand-built stand-in atlas: one level of `width` x `height` texels, row-major with the
/// first row at the top, and one sprite covering the whole image.
///
/// The pixels are generated here; no asset store is read and no Mojang pixel is embedded.
fn atlas(width: u32, height: u32, texels: &[[u8; 4]]) -> Atlas {
    assert_eq!(texels.len(), (width * height) as usize, "one texel each");
    let mut rgba = Vec::with_capacity(texels.len() * 4);
    for texel in texels {
        rgba.extend_from_slice(texel);
    }
    let whole = AtlasSprite {
        region: SpriteRect {
            x: 0,
            y: 0,
            w: width,
            h: height,
        },
        content: SpriteRect {
            x: 0,
            y: 0,
            w: width,
            h: height,
        },
    };
    Atlas {
        levels: vec![AtlasLevel {
            width,
            height,
            rgba,
        }],
        width,
        height,
        level_count: 1,
        sprites: BTreeMap::from([("test:whole".to_string(), whole)]),
        animated: BTreeMap::new(),
        missing: whole,
    }
}

/// A one-colour atlas of `side` x `side` texels.
fn solid_atlas(side: u32, colour: [u8; 4]) -> Atlas {
    atlas(side, side, &vec![colour; (side * side) as usize])
}

/// A hand-built stand-in atlas with one level per entry of [`LEVEL_TEXELS`]: a 16x16 level 0,
/// an 8x8 level 1 and a 4x4 level 2, each a flat colour at its own size, and one sprite
/// covering the image.
///
/// The level sizes and the flat colours are generated here; no asset store is read and no
/// Mojang pixel is embedded. A stitcher-built atlas cannot stand in: `build_atlas` needs a
/// `TextureSet`, which only `TextureSet::load` builds, and these tests never touch a store.
fn levelled_atlas() -> Atlas {
    const SIDE: u32 = 16;
    let levels = LEVEL_TEXELS
        .iter()
        .enumerate()
        .map(|(level, texel)| {
            let side = (SIDE >> level).max(1);
            AtlasLevel {
                width: side,
                height: side,
                rgba: texel.repeat((side * side) as usize),
            }
        })
        .collect();
    let whole = AtlasSprite {
        region: SpriteRect {
            x: 0,
            y: 0,
            w: SIDE,
            h: SIDE,
        },
        content: SpriteRect {
            x: 0,
            y: 0,
            w: SIDE,
            h: SIDE,
        },
    };
    Atlas {
        levels,
        width: SIDE,
        height: SIDE,
        level_count: LEVEL_TEXELS.len() as u32,
        sprites: BTreeMap::from([("test:levels".to_string(), whole)]),
        animated: BTreeMap::new(),
        missing: whole,
    }
}

/// The four corners of a quad on the plane z = -`depth` that exactly covers the frame.
///
/// The frame its eye is the origin looking down -Z with the vanilla field of view, so the
/// plane is `depth * tan(fov / 2)` half as tall and, with a square target, as wide. `uv` is
/// the frame's top-left and bottom-right corner in texture coordinates; the corners go
/// counter-clockwise seen from the camera, the winding the mesher emits.
fn covering_quad(depth: f32, uv: [[f32; 2]; 2]) -> [([f32; 3], [f32; 2]); 4] {
    let half = depth * (DEFAULT_FOV / 2.0).to_radians().tan();
    let [[u0, v0], [u1, v1]] = uv;
    [
        ([-half, -half, -depth], [u0, v1]),
        ([half, -half, -depth], [u1, v1]),
        ([half, half, -depth], [u1, v0]),
        ([-half, half, -depth], [u0, v0]),
    ]
}

/// Appends one textured quad to `mesh`'s layer: four corners, then six indices.
fn push_quad(
    mesh: &mut ChunkMesh,
    layer: Layer,
    corners: [([f32; 3], [f32; 2]); 4],
    colour: [u8; 4],
) {
    let target = &mut mesh.layers[layer.index()];
    let base = target.vertices.len() as u32;
    for (position, uv) in corners {
        target.vertices.push(Vertex {
            position,
            uv,
            light: [FULL_SKY; 2],
            colour,
        });
    }
    target
        .indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// The camera the atlas tests draw with: the eye at the origin looking down -Z, the vanilla
/// field of view and an aspect of one (the target is square).
///
/// The pose's feet position is the eye minus [`EYE_HEIGHT`], so the eye lands exactly on the
/// origin; the render distance is one chunk, which puts the far plane well past the quads.
fn frame_camera() -> Camera {
    Camera {
        pose: CameraPose {
            position: [0.0, -f64::from(EYE_HEIGHT), 0.0],
            yaw: 180.0,
            pitch: 0.0,
        },
        fov_degrees: DEFAULT_FOV,
        near: NEAR_PLANE,
        far_chunks: 1.0,
    }
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

/// Appends one face to `mesh`'s opaque layer — the fixture's faces are solid terrain — four
/// corners, then six indices for two triangles.
///
/// Each corner carries its own uv corner in the mesher's own order, with v zero at the top of
/// the sprite; the stand-in atlas is one colour, so the sample is the same wherever it lands.
fn push_face(mesh: &mut ChunkMesh, corners: [[f32; 3]; 4], brightness: f32, colour: [f32; 3]) {
    let layer = &mut mesh.layers[Layer::Opaque.index()];
    let base = layer.vertices.len() as u32;
    const UV: [[f32; 2]; 4] = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    for (position, uv) in corners.into_iter().zip(UV) {
        let shade = |channel: usize| (colour[channel] * brightness * 255.0).round() as u8;
        layer.vertices.push(Vertex {
            position,
            uv,
            light: [FULL_SKY; 2],
            colour: [shade(0), shade(1), shade(2), 255],
        });
    }
    layer
        .indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// The camera both block tests look at the block with.
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

/// Asserts one pixel's RGB bytes exactly, naming it in the failure message.
fn expect_pixel_exact(pixels: &[u8], x: u32, y: u32, want: [u8; 3], what: &str) {
    let got = pixel(pixels, x, y);
    assert_eq!(got, want, "{what} at ({x}, {y})");
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
