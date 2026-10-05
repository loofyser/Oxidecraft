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
//! with a synthetic font sheet and checks the glyph pixels, the advances and the one-font-pixel
//! shadow offset (now at the source's 63/255) against the layout `debug_text` produces.
//!
//! The atlas tests come next: the texels of a four-colour atlas are read back through the
//! client's own brightness — the atlas texel times the lightmap's cell — at both the
//! lightmap's brightest cell and its floor (the reference-gradient check for the non-sRGB
//! colour-space decision, and for the lightmap's bytes: the pass's brightest cell is 252, not
//! 255, because the source's closing `* 0.96 + 0.03` chain leaves the full-day cell at 0.99),
//! a magnified sample picks the nearer of two texels, the translucent layer blends its
//! 50%-alpha fragment over the opaque layer's colour exactly as
//! `src_alpha / one_minus_src_alpha` says, a cutout fragment whose texel alpha is zero is
//! discarded so the surface behind it shows through, a translucent fragment whose alpha sits
//! below the client's tenth is discarded the same way, and an atlas with three mip levels is
//! read back at its first and last level, so the level-by-level uploader is pinned beyond
//! level 0.
//!
//! Every read-back expectation of a lit surface is computed from `lightmap_image`'s own
//! output for the pair the fixture's corners carry, so the bytes the pass uploads, the
//! fragment's arithmetic and the texture's colour space are pinned here rather than passing
//! quietly — while a change to `lightmap_image`'s own maths moves the texture and the
//! expectation together and is the CPU suite's to catch. The pair's axis order is the
//! asymmetric case's to pin: its two pairs address different cells, so a transposed link reads
//! the other one and fails, where every symmetric pair here would read the same either way
//! round.

use std::collections::BTreeMap;
use std::sync::mpsc;
use std::task::{Context, Poll, Wake, Waker};

use oxide_assets::atlas::{Atlas, AtlasLevel, AtlasSprite, SpriteRect};
use oxide_assets::texture::Texture;
use oxide_render::camera::{
    Camera, CameraPose, DEFAULT_FOV, EYE_HEIGHT, FIRST_PERSON_OFFSET, NEAR_PLANE, NO_VIEW_EFFECT,
};
use oxide_render::entity_models::{Pose, PoseExtra};
use oxide_render::entity_pass::{
    DrawExtra, EntityDraw, EntityPass, ModelRef, NametagDraw, TextureRef, TextureRegistry,
};
use oxide_render::fog::{FogParams, fog_colour};
use oxide_render::lightmap::{BrightnessTable, lightmap_image, sample_index};
use oxide_render::overlay::OverlayPass;
use oxide_render::renderer::SKY_COLOR;
use oxide_render::sky::{
    CloudPass, HORIZON, MOON_HEIGHT, SkyParams, SkyPass, SkyTextures, celestial_rotation,
    star_field,
};
use oxide_render::terrain::{ChunkMesh, Layer, Vertex};
use oxide_render::terrain_pass::{DEPTH_FORMAT, TerrainPass};
use oxide_render::world_overlay::{Crack, FULL_CUBE, Outline, WorldOverlay};

use glam::Vec3;

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
/// The overlay text colour: opaque white, the sheet's ink texel.
const TEXT: [u8; 3] = [255, 255, 255];
/// The overlay shadow colour as 8-bit unorm bytes: the source's `(0xFFFFFFFF & 0x00FCFCFC) >> 2
/// | 0xFF000000` is 0xFF3F3F3F, 63 per channel.
const SHADOW: [u8; 3] = [63, 63, 63];
/// The stone colour the block's vertices carry: multiplied by the white stand-in atlas's
/// texel, so the block still reads as stone grey.
const STONE_COLOUR: [f32; 3] = [0.5, 0.5, 0.5];
/// The buried face's colour, which is no real texture's at all.
const BURIED_COLOUR: [f32; 3] = [1.0, 0.0, 0.0];
/// The packed light of a corner with both levels full: level 15 shifted four bits with the
/// sampler's eight added, in the block field and again in the sky one.
const FULL_LIGHT: u16 = 248;
/// The packed light of a corner both of whose levels are dark: level 0 shifted four bits with
/// the sampler's eight added, in the block field and again in the sky one.
const LOW_LIGHT: u16 = 8;

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

    let mut terrain = TerrainPass::new(&device, &queue, format);
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

    let mut terrain = TerrainPass::new(&device, &queue, format);
    terrain.set_atlas(&device, &queue, &solid_atlas(16, [255, 255, 255, 255]));
    terrain.set_camera(&queue, camera(), 1.0);
    terrain.upload(&device, &queue, (0, 0, 0), &stone_block_mesh());

    let mut overlay = OverlayPass::new(&device, format);
    overlay.set_size(&queue, SIZE as f32, SIZE as f32);
    overlay
        .set_font(&device, &queue, &overlay_font_sheet())
        .expect("the synthetic sheet is a 16x16 grid");
    // Four vertical bars in one line: each is the first column of its cell, so the glyph
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
    // The text margin is four pixels and the scale two; the '|' cell inks only its first
    // column and advances two font pixels, so the texture's ink lands four pixels apart and
    // the shadow copy sits two pixels down and right.
    expect_pixel(&pixels, 4, 4, TEXT, "the first cell's first textured texel");
    expect_pixel(&pixels, 5, 19, TEXT, "the first cell's last textured row");
    expect_pixel(&pixels, 6, 6, SHADOW, "the first cell's shadow");
    // The gap between the first cell's ink and the second's is transparent sheet, so the
    // frame's sky shows through; the stone face sits at the frame's centre.
    expect_pixel(
        &pixels,
        7,
        4,
        SKY,
        "the transparent gap before the second cell",
    );
    expect_pixel(&pixels, 8, 4, TEXT, "the second cell");
    // The fourth cell starts three advances along: x = 4 + 3 * 2 * 2.
    expect_pixel(&pixels, 16, 4, TEXT, "the fourth cell");
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

/// The synthetic overlay font sheet: a 128x128 grid whose `|` cell inks only its first column,
/// so the glyph's advance is two font pixels and its ink and shadow land on known pixels.
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn overlay_font_sheet() -> Texture {
    const SIDE: u32 = 128;
    const CELL: u32 = 8;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    // '|' is code 124: column 12, row 7 of the grid.
    let code = '|' as u32;
    let cell_x = (code % 16) * CELL;
    let cell_y = (code / 16) * CELL;
    for row in 0..CELL {
        let offset = (((cell_y + row) * SIDE + cell_x) * 4) as usize;
        rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
    }
    Texture {
        width: SIDE,
        height: SIDE,
        rgba,
    }
}

#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_atlas_texels_come_back_lit_by_the_lightmap() {
    // The reference-gradient check for the colour-space policy (the spec's appendix C.1): the
    // fragment's colour is the atlas texel times the vertex colour times the lightmap's cell,
    // with nothing converted on the way in or out.
    let light = light_cell(15, 15);
    assert_eq!(light, [252, 252, 252, 255], "the lightmap's brightest cell");
    let (device, queue) = headless_device();
    let mut mesh = ChunkMesh::default();
    push_quad(
        &mut mesh,
        Layer::Opaque,
        covering_quad(2.0, [[0.0, 0.0], [1.0, 1.0]]),
        WHITE,
    );
    let pixels = render_terrain(&device, &queue, &atlas(2, 2, &FOUR_TEXELS), &mesh);

    // One pixel per texel, each sampling the middle of its quadrant: the read-back is the
    // texel times 252/255 exactly, so a texel of 255 reads back 252 and a black one stays 0.
    expect_texel(
        &pixels,
        16,
        16,
        shaded(FOUR_TEXELS[0], WHITE, light),
        "the lit top-left texel",
    );
    expect_texel(
        &pixels,
        48,
        16,
        shaded(FOUR_TEXELS[1], WHITE, light),
        "the lit top-right texel",
    );
    expect_texel(
        &pixels,
        16,
        48,
        shaded(FOUR_TEXELS[2], WHITE, light),
        "the lit bottom-left texel",
    );
    expect_texel(
        &pixels,
        48,
        48,
        shaded(FOUR_TEXELS[3], WHITE, light),
        "the lit bottom-right texel",
    );

    // The same quads at the lightmap's floor, the darkest dark: every texel comes back times
    // 14/255. A lightmap texture whose bytes were converted to another colour space would read
    // something else here; this pair is symmetric, so a transposed link reads the same cell and
    // the axis order is the asymmetric case's to catch instead.
    let dark = light_cell(0, 0);
    assert_eq!(dark, [14, 14, 14, 255], "the lightmap's floor");
    let mut mesh = ChunkMesh::default();
    push_quad_with_light(
        &mut mesh,
        Layer::Opaque,
        covering_quad(2.0, [[0.0, 0.0], [1.0, 1.0]]),
        WHITE,
        [LOW_LIGHT; 2],
    );
    let pixels = render_terrain(&device, &queue, &atlas(2, 2, &FOUR_TEXELS), &mesh);
    expect_texel(
        &pixels,
        16,
        16,
        shaded(FOUR_TEXELS[0], WHITE, dark),
        "the dim top-left texel",
    );
    expect_texel(
        &pixels,
        48,
        48,
        shaded(FOUR_TEXELS[3], WHITE, dark),
        "the dim bottom-right texel",
    );
}

#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn an_asymmetric_light_pair_reads_its_own_cell() {
    // The one fixture here whose two levels differ, so a transposed link cannot pass: the two
    // pairs address different cells — `(sky 12, block 4)` is [161, 152, 144] and `(sky 4,
    // block 12)` is [210, 194, 164], both pinned by the CPU suite (`tests/lightmap.rs`) — and
    // the expectation is computed from `lightmap_image`'s own output like every other read-back
    // here. The pair's fields are (block, sky): the block level picks the texel column and the
    // sky level the row.
    let (device, queue) = headless_device();
    // Two quads at one depth, one per half of the frame: the left carries (block 4, sky 12) and
    // the right its transposition, each written as the packed pair a vertex carries.
    let half = 2.0 * (DEFAULT_FOV / 2.0).to_radians().tan();
    let quad = |x0: f32, x1: f32| {
        [
            ([x0, -half, -2.0], [0.0, 1.0]),
            ([x1, -half, -2.0], [1.0, 1.0]),
            ([x1, half, -2.0], [1.0, 0.0]),
            ([x0, half, -2.0], [0.0, 0.0]),
        ]
    };
    let mut mesh = ChunkMesh::default();
    // A level `L` of a field is packed as `L * 16 + 8`: 72 is block level 4, 200 sky level 12.
    push_quad_with_light(&mut mesh, Layer::Opaque, quad(-half, 0.0), WHITE, [72, 200]);
    push_quad_with_light(&mut mesh, Layer::Opaque, quad(0.0, half), WHITE, [200, 72]);
    let pixels = render_terrain(
        &device,
        &queue,
        &solid_atlas(4, [255, 255, 255, 255]),
        &mesh,
    );

    // A link that transposed the fields would swap these two: the left half would read
    // [210, 194, 164] instead of [161, 152, 144], and the right the other way round.
    expect_texel(
        &pixels,
        16,
        32,
        shaded(WHITE, WHITE, light_cell(4, 12)),
        "the (block 4, sky 12) half",
    );
    expect_texel(
        &pixels,
        48,
        32,
        shaded(WHITE, WHITE, light_cell(12, 4)),
        "the (block 12, sky 4) half",
    );
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

    // The fragment is lit by the lightmap's brightest cell like every other fixture here, so
    // each texel comes back through that factor.
    let light = light_cell(15, 15);
    expect_texel(
        &pixels,
        16,
        32,
        shaded(FOUR_TEXELS[0], WHITE, light),
        "the pixel nearer the left texel",
    );
    expect_texel(
        &pixels,
        48,
        32,
        shaded(FOUR_TEXELS[1], WHITE, light),
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
    // read-back shows — through the brightness every terrain fragment takes: the lightmap's
    // brightest cell scales both surfaces by 252/255, so 128/255 of red over green is
    // (126, 126, 0) byte-exact, half a byte above the 125.5 the green channel lands on.
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
        [126, 126, 0],
        "the blended centre",
    );
    // The translucent quad covers the whole frame, so a pixel well away from the centre is
    // the same blend: the result does not depend on where in the quad it lands.
    expect_pixel_exact(&pixels, 16, 48, [126, 126, 0], "a blended pixel off centre");
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

    // The surface behind the discarded fragment is the lit white quad: the lightmap's
    // brightest cell scales it to 252, which is the strongest a lit surface can read back.
    expect_pixel(
        &pixels,
        16,
        32,
        [252, 252, 252],
        "the surface behind the discarded fragment",
    );
    expect_texel(
        &pixels,
        48,
        32,
        shaded(CUTOUT_TEXELS[1], WHITE, light_cell(15, 15)),
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

    // The lit white surface behind the discarded fragment, and the kept fragment: an opaque
    // texel blended over the lit surface is that texel times the lightmap's cell.
    expect_pixel(
        &pixels,
        16,
        32,
        [252, 252, 252],
        "the surface behind the discarded translucent fragment",
    );
    expect_texel(
        &pixels,
        48,
        32,
        shaded(TRANSLUCENT_TEXELS[1], WHITE, light_cell(15, 15)),
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
    let light = light_cell(15, 15);
    expect_texel(
        &pixels,
        16,
        32,
        shaded(LEVEL_TEXELS[0], WHITE, light),
        "the magnified level 0",
    );

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
    let light = light_cell(15, 15);
    expect_texel(
        &pixels,
        16,
        32,
        shaded(LEVEL_TEXELS[2], WHITE, light),
        "the clamped deepest level",
    );
    expect_texel(
        &pixels,
        48,
        16,
        shaded(LEVEL_TEXELS[2], WHITE, light),
        "another pixel of the same level",
    );
}

/// One cell of [`lightmap_image`] as RGBA, addressed through the module's own convention: the
/// block level across, the sky level down.
///
/// The noon cell is the client's own starting image — the Overworld's brightness table,
/// `getSunBrightness` at the noon angle and the default gamma (`World.java:1418-1427`,
/// `GameSettings.java:171`). The read-backs below are computed from it rather than from the
/// texels alone, because the fragment stage multiplies the atlas texel by it. Its brightest
/// cell is 252 and not 255: the source's closing `* 0.96 + 0.03` chain leaves the full-day cell
/// at 0.99, so even a fully lit surface reads back a little darker than its texture. Its floor
/// is the darkest dark, 14.
fn light_cell(block: u8, sky: u8) -> [u8; 4] {
    light_cell_with(1.0, block, sky)
}

/// One cell of the lightmap a sky brightness builds, as the fragment would read it: the image
/// `set_lightmap` rewrites the texture with, `lightmap_image` of the same table at
/// `sun_brightness` and the default gamma.
fn light_cell_with(sun_brightness: f32, block: u8, sky: u8) -> [u8; 4] {
    let (u, v) = sample_index(sky, block);
    let image = lightmap_image(&BrightnessTable::overworld(), sun_brightness, 0.0);
    let index = ((v * 16 + u) * 4) as usize;
    [
        image[index],
        image[index + 1],
        image[index + 2],
        image[index + 3],
    ]
}

/// An opaque white vertex colour, which the fixtures whose colour is not the point carry.
const WHITE: [u8; 4] = [255, 255, 255, 255];

/// A texel as the fragment's maths produces it: the atlas texel times the vertex colour times
/// the lightmap's cell. A byte is a 255th, so three of them multiply into a 65025th and the
/// byte comes back rounded to the nearest: `(texel * colour * light + 32512) / 65025`.
///
/// The GPU multiplies the three unorm values in floating point; the product of three integers
/// over 65025 is never exactly a half-integer, so this integer form and the GPU's rounding
/// agree byte for byte.
fn shaded(texel: [u8; 4], colour: [u8; 4], light: [u8; 4]) -> [u8; 4] {
    let channel = |index: usize| {
        ((u32::from(texel[index]) * u32::from(colour[index]) * u32::from(light[index]) + 32512)
            / 65025) as u8
    };
    [channel(0), channel(1), channel(2), channel(3)]
}

/// A colour as the target's unorm bytes: each float times 255, rounded.
fn unorm_bytes(colour: [f32; 3]) -> [u8; 3] {
    let channel = |index: usize| (colour[index] * 255.0).round() as u8;
    [channel(0), channel(1), channel(2)]
}

/// A small quad, a fifth of a block across, centred `depth` blocks ahead of the frame's eye and
/// `x` blocks to its right. The eye the camera's own view uses sits a tenth of a block behind
/// the origin (`crate::camera::FIRST_PERSON_OFFSET`), so the quad's plane is placed at
/// `-(depth - 0.1)`: `depth` is the eye-space depth the source's planar measure would read, and
/// the radial measure reads `sqrt(x^2 + depth^2)` on it. The sampled pixel sits within a couple
/// of hundredths of a block of the quad's centre, so either measure reads the centre's own
/// distance there.
fn flat_quad(depth: f32, x: f32) -> [([f32; 3], [f32; 2]); 4] {
    let z = -(depth - 0.1);
    let [x0, x1] = [x - 0.1, x + 0.1];
    [
        ([x0, -0.1, z], [0.0, 1.0]),
        ([x1, -0.1, z], [1.0, 1.0]),
        ([x1, 0.1, z], [1.0, 0.0]),
        ([x0, 0.1, z], [0.0, 0.0]),
    ]
}

#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_frames_fog_fades_the_terrain_towards_its_colour() {
    // The Overworld's colour at dusk, and a frame whose fade starts two units from the eye and
    // reaches full strength at six. A quad at each distance shows the three states of the mix:
    // at the start the surface keeps its whole colour, at the end it is the fog colour, and
    // between them it is the two mixed — `clamp((end - depth) / (end - start), 0, 1)`. The
    // render distance sits at its thirty-two-chunk maximum so the colour is the dusk base
    // itself, with no sky mix or brightness factor in the way (`EntityRenderer.java:1767-1768`,
    // `:363-364`).
    let colour = fog_colour(0, 14000.0, 64.0, 0.03125, [0.4, 0.6, 0.8], 32, 15);
    let fog = FogParams {
        colour,
        start: 2.0,
        end: 6.0,
        far_plane: 8.0,
    };
    let (device, queue) = headless_device();
    let lit = shaded(WHITE, WHITE, light_cell(15, 15));

    let frame = |depth: f32| -> Vec<u8> {
        let mut mesh = ChunkMesh::default();
        push_quad(&mut mesh, Layer::Opaque, flat_quad(depth, 0.0), WHITE);
        render_terrain_with_fog(
            &device,
            &queue,
            &solid_atlas(4, [255, 255, 255, 255]),
            &mesh,
            Some(fog),
        )
    };

    // At the fade's start the factor is one: the lit surface, unreached by the fog.
    expect_pixel(
        &frame(2.0),
        SIZE / 2,
        SIZE / 2,
        [252, 252, 252],
        "the surface at the fade's start",
    );
    // At its end the factor is zero: the frame's fog colour alone.
    expect_pixel(
        &frame(6.0),
        SIZE / 2,
        SIZE / 2,
        unorm_bytes(colour),
        "the surface at the fade's end",
    );
    // Halfway between them the factor is a half, and the fragment is the two mixed.
    let lit_f = [
        f32::from(lit[0]) / 255.0,
        f32::from(lit[1]) / 255.0,
        f32::from(lit[2]) / 255.0,
    ];
    let mixed = [
        colour[0] * 0.5 + lit_f[0] * 0.5,
        colour[1] * 0.5 + lit_f[1] * 0.5,
        colour[2] * 0.5 + lit_f[2] * 0.5,
    ];
    expect_pixel(
        &frame(4.0),
        SIZE / 2,
        SIZE / 2,
        unorm_bytes(mixed),
        "the surface halfway through the fade",
    );
}

#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_frames_fog_measures_the_radial_distance_from_the_eye() {
    // The source asks the driver for the eye-radial fog distance whenever the context carries
    // `GL_NV_fog_distance` (`GL11.glFogi(GL_FOG_DISTANCE_MODE_NV, GL_EYE_RADIAL_NV)`,
    // `EntityRenderer.java:2018-2021`; the rig's NVIDIA driver has it), so a fragment's distance
    // is the straight line from the eye, not the eye-space depth a planar measure would use.
    // The two part company off the view axis: this frame's fog runs from two units to six, and a
    // fragment four units ahead of the eye and two and a half to the side sits at
    // `sqrt(2.5^2 + 4^2) = 4.717` — past the halfway mark radially, while its planar depth of
    // four would read the exact half mix. The expected bytes below are the radial mix of the
    // lit `(252, 252, 252)` surface into the dusk fog colour `(102, 114, 139)`; the planar mix
    // reads `(177, 183, 195)`, twenty to twenty-seven bytes away.
    let colour = fog_colour(0, 14000.0, 64.0, 0.03125, [0.4, 0.6, 0.8], 32, 15);
    let fog = FogParams {
        colour,
        start: 2.0,
        end: 6.0,
        far_plane: 8.0,
    };
    let (device, queue) = headless_device();
    let mut mesh = ChunkMesh::default();
    // The lateral quad, centred two and a half blocks to the right of the axis at four blocks'
    // eye-space depth.
    push_quad(&mut mesh, Layer::Opaque, flat_quad(4.0, 2.5), WHITE);
    let frame = render_terrain_with_fog(
        &device,
        &queue,
        &solid_atlas(4, [255, 255, 255, 255]),
        &mesh,
        Some(fog),
    );
    // The quad's centre projects to pixel x 60.6 at this fov (`x / depth / tan(35°)` with the
    // eye's own offset folded into the plane), and the fragment there reads the radial mix.
    expect_pixel(
        &frame,
        60,
        SIZE / 2,
        [150, 159, 175],
        "the off-axis surface at the eye's radial distance",
    );
}

/// The sky and cloud passes: the band's read-back over a contrasting clear, the star brightness
/// gate, the moon's phase cell, the under-horizon plane's fog and the layer's blend.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_sky_pass_draws_its_band_and_only_draws_stars_when_they_are_bright() {
    // The sky is the pass whose failure mode is a black screen: it draws the frame's background
    // with no depth writes of its own, so a pipeline that lost its state or its geometry shows
    // the clear colour instead of the band. The three textures are synthetic stand-ins built
    // here — no asset store and no Mojang pixel.
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let textures = synthetic_sky_textures();
    let mut sky = SkyPass::new(&device, &queue, format);
    sky.set_textures(&device, &queue, &textures.sun, &textures.moon_phases);
    let mut cloud = CloudPass::new(&device, &queue, format);
    cloud.set_texture(&device, &queue, &textures.clouds);

    // The band at noon, the fog set to the band's own colour so the read-back is the band. The
    // target clears to a colour no band fragment can produce, so a pass that drew nothing —
    // the black screen this case exists for — fails the assertion.
    let noon = [120.0 / 255.0, 167.0 / 255.0, 1.0];
    let day = SkyParams {
        celestial_angle: 0.0,
        sky_colour: noon,
        sun_brightness: 1.0,
        star_brightness: 0.0,
        fog_colour: noon,
        far_plane: 128.0,
        cloud_offset_ticks: 0,
        cloud_colour: [1.0, 1.0, 1.0],
        moon_phase: 0,
    };
    let pixels = render_sky(
        &device,
        &queue,
        &mut sky,
        &mut cloud,
        SkyRequest {
            params: day,
            camera: sky_camera(65.0, 0.0, -20.0, DEFAULT_FOV),
            clear: [1.0, 0.0, 1.0],
            clouds: false,
        },
    );
    expect_pixel(
        &pixels,
        SIZE / 2,
        SIZE / 2,
        unorm_bytes(noon),
        "the sky band at noon over a contrasting clear",
    );

    // The star gate: a narrow camera aimed at the first star of the source's field, with the
    // pass given the boost the source's own brightness would never give at noon — the gate is
    // the pass's, and the star must be drawn when it is above zero and skipped at zero. The
    // camera sits an eye's height above the geometry's frame origin (`EntityRenderer.java:738`).
    let star = star_field()[0];
    let target = celestial_rotation(0.0).transform_point3(Vec3::new(
        star.centre[0] as f32,
        star.centre[1] as f32,
        star.centre[2] as f32,
    ));
    let (yaw, pitch) = aim_at(target);
    let night = SkyParams {
        celestial_angle: 0.0,
        sky_colour: [0.0, 0.0, 0.0],
        sun_brightness: 1.0,
        star_brightness: 0.5,
        fog_colour: [0.0, 0.0, 0.0],
        far_plane: 128.0,
        cloud_offset_ticks: 0,
        cloud_colour: [1.0, 1.0, 1.0],
        moon_phase: 0,
    };
    let camera = sky_camera(65.0, yaw, pitch, 0.5);
    let lit = render_sky(
        &device,
        &queue,
        &mut sky,
        &mut cloud,
        SkyRequest {
            params: night,
            camera,
            clear: [0.0, 0.0, 0.0],
            clouds: false,
        },
    );
    let dark = render_sky(
        &device,
        &queue,
        &mut sky,
        &mut cloud,
        SkyRequest {
            params: SkyParams {
                star_brightness: 0.0,
                ..night
            },
            camera,
            clear: [0.0, 0.0, 0.0],
            clouds: false,
        },
    );
    assert!(
        brightest(&lit) >= 16,
        "the aimed star is drawn when its brightness is above zero: {}",
        brightest(&lit)
    );
    assert!(
        brightest(&dark) <= 2,
        "no star pixel is drawn when the brightness is zero: {}",
        brightest(&dark)
    );

    // The moon's phase cell: a camera aimed at the moon, whose synthetic sheet paints one
    // colour per cell. The sun is underfoot at this angle, so only the moon is in the frame.
    // Each phase must read its own cell, which a pass that pinned phase 0 could not do.
    let moon_target = celestial_rotation(0.375).transform_point3(Vec3::new(0.0, MOON_HEIGHT, 0.0));
    let (moon_yaw, moon_pitch) = aim_at(moon_target);
    let moon = SkyParams {
        celestial_angle: 0.375,
        sky_colour: [0.0, 0.0, 0.0],
        sun_brightness: 1.0,
        star_brightness: 0.0,
        fog_colour: [0.0, 0.0, 0.0],
        far_plane: 128.0,
        cloud_offset_ticks: 0,
        cloud_colour: [1.0, 1.0, 1.0],
        moon_phase: 5,
    };
    let moon_camera = sky_camera(65.0, moon_yaw, moon_pitch, DEFAULT_FOV);
    for phase in [5u8, 0u8] {
        let pixels = render_sky(
            &device,
            &queue,
            &mut sky,
            &mut cloud,
            SkyRequest {
                params: SkyParams {
                    moon_phase: phase,
                    ..moon
                },
                camera: moon_camera,
                clear: [0.0, 0.0, 0.0],
                clouds: false,
            },
        );
        expect_pixel(
            &pixels,
            SIZE / 2,
            SIZE / 2,
            moon_cell_colour(phase),
            "the moon's phase cell",
        );
    }

    // The under-horizon plane is fogged under the sky's own range: looking down at it, the
    // fragment is the plane's darkened colour mixed towards the fog colour, the ray length
    // being the plane's drop below the camera over sin(89 deg). The camera looks almost
    // straight down, so the sample lands on a corner of the sky grid under the eye — a vertex
    // of the grid — where the vertex-distance varying interpolates to the vertex's own value
    // and the mix therefore reads the exact ray length.
    let below_sky = [0.0f32, 0.0, 0.0];
    let below = SkyParams {
        celestial_angle: 0.0,
        sky_colour: below_sky,
        sun_brightness: 1.0,
        star_brightness: 0.0,
        fog_colour: [1.0, 1.0, 1.0],
        far_plane: 128.0,
        cloud_offset_ticks: 0,
        cloud_colour: [1.0, 1.0, 1.0],
        moon_phase: 0,
    };
    let feet = 65.0f32;
    let pitch = 89.0f32;
    let pixels = render_sky(
        &device,
        &queue,
        &mut sky,
        &mut cloud,
        SkyRequest {
            params: below,
            camera: sky_camera(f64::from(feet), 0.0, pitch, DEFAULT_FOV),
            clear: below_sky,
            clouds: false,
        },
    );
    let d0 = (feet + EYE_HEIGHT) - HORIZON;
    let depth = (EYE_HEIGHT + d0) / pitch.to_radians().sin();
    let factor = ((below.far_plane - depth) / below.far_plane).clamp(0.0, 1.0);
    let below_colour = [
        below_sky[0] * 0.2 + 0.04,
        below_sky[1] * 0.2 + 0.04,
        below_sky[2] * 0.6 + 0.1,
    ];
    let mixed = std::array::from_fn(|channel| {
        below.fog_colour[channel] + (below_colour[channel] - below.fog_colour[channel]) * factor
    });
    expect_pixel(
        &pixels,
        SIZE / 2,
        SIZE / 2,
        unorm_bytes(mixed),
        "the fogged below-horizon plane",
    );

    // The off-axis sample: the near-vertical one above sits where the eye-radial measure and a
    // planar one agree, so it cannot tell them apart. This one is 30 degrees off the view axis,
    // where they part: the below-horizon plane's grid corner under the eye, 40.33 blocks away,
    // would read ten bytes darker per channel under an eye-space-depth measure. The camera
    // stands high enough that the plane's drop below the eye is 40 blocks, and the celestial
    // frame is turned a quarter turn so the sun and the moon are on the horizon, clear of a
    // camera looking down.
    let high = 100.0f32;
    let down = 59.66f32;
    let lateral = SkyParams {
        celestial_angle: 0.25,
        sky_colour: below_sky,
        sun_brightness: 1.0,
        star_brightness: 0.0,
        fog_colour: [1.0, 1.0, 1.0],
        far_plane: 128.0,
        cloud_offset_ticks: 0,
        cloud_colour: [1.0, 1.0, 1.0],
        moon_phase: 0,
    };
    let camera = sky_camera(f64::from(high), 0.0, down, DEFAULT_FOV);
    let pixels = render_sky(
        &device,
        &queue,
        &mut sky,
        &mut cloud,
        SkyRequest {
            params: lateral,
            camera,
            clear: below_sky,
            clouds: false,
        },
    );
    // The corner of the plane's own grid that sits under the eye, in the pass's local frame:
    // the grid's origin, lifted to `-(eyeY - HORIZON)` (`RenderGlobal.java:1404`). Its straight
    // line from the eye the view is built with is the fog's measure.
    let eye = Vec3::new(0.0, EYE_HEIGHT, 0.0) - FIRST_PERSON_OFFSET * camera.forward();
    let plane_y = -((f64::from(high) + f64::from(EYE_HEIGHT)) - f64::from(HORIZON));
    let distance = (Vec3::new(0.0, plane_y as f32, 0.0) - eye).length();
    let factor = ((lateral.far_plane - distance) / lateral.far_plane).clamp(0.0, 1.0);
    let mixed = std::array::from_fn(|channel| {
        lateral.fog_colour[channel] + (below_colour[channel] - lateral.fog_colour[channel]) * factor
    });
    // The corner sits straight under the eye, so it projects onto the frame's centre column;
    // the pitch puts it 30.34 degrees below the view axis, which is
    // `tan(30.34 deg) / tan(35 deg) = 0.836` of the half-height below the centre, i.e. row 58.
    expect_pixel(
        &pixels,
        SIZE / 2,
        58,
        unorm_bytes(mixed),
        "the off-axis below-horizon plane at the eye's radial distance",
    );

    // The cloud layer blended over the band: a white texel at the source's 0.8 alpha over the
    // black night band is 204 exactly, and a layer that lost its blend state or its geometry
    // reads the band's black instead. The camera looks 30 degrees up, clear of the noon sun.
    let pixels = render_sky(
        &device,
        &queue,
        &mut sky,
        &mut cloud,
        SkyRequest {
            params: SkyParams {
                star_brightness: 0.0,
                ..night
            },
            camera: sky_camera(65.0, 0.0, -30.0, DEFAULT_FOV),
            clear: below_sky,
            clouds: true,
        },
    );
    expect_pixel(
        &pixels,
        SIZE / 2,
        SIZE / 2,
        [204, 204, 204],
        "the cloud layer over the band",
    );
}

/// One sky frame the test asks for: the parameters, the camera, the target's clear colour and
/// whether the cloud layer draws after the sky.
struct SkyRequest {
    /// The frame's sky parameters.
    params: SkyParams,
    /// The camera both passes draw with.
    camera: Camera,
    /// The target's clear colour, opaque.
    clear: [f32; 3],
    /// Whether the cloud layer draws after the sky.
    clouds: bool,
}

/// The camera's yaw and pitch for a local target position, from the camera at
/// `(0, EYE_HEIGHT, 0)` — the frame origin the source's modelview translates the geometry by
/// (`EntityRenderer.java:738`).
fn aim_at(target: Vec3) -> (f32, f32) {
    let direction = (target - Vec3::new(0.0, EYE_HEIGHT, 0.0)).normalize();
    let yaw = (-direction.x).atan2(direction.z).to_degrees();
    let pitch = (-direction.y).asin().to_degrees();
    (yaw, pitch)
}

/// The three synthetic environment textures: flat white sun and cloud stand-ins and the painted
/// moon sheet, with the source's own sun, moon sheet and cloud sizes — all generated here, never
/// a pixel from the asset store.
fn synthetic_sky_textures() -> SkyTextures {
    SkyTextures {
        sun: flat_texture(32, 32),
        moon_phases: moon_sheet(),
        clouds: flat_texture(64, 64),
    }
}

/// A `width` x `height` texture of one opaque white texel value.
fn flat_texture(width: u32, height: u32) -> Texture {
    Texture {
        width,
        height,
        rgba: vec![255u8; (width * height * 4) as usize],
    }
}

/// The moon phase sheet: the source's 128x64 4x2 grid, each 32x32 cell one opaque colour, so a
/// read-back names the phase the pass drew.
fn moon_sheet() -> Texture {
    let mut rgba = vec![0u8; 128 * 64 * 4];
    for phase in 0..8u8 {
        let column = usize::from(phase % 4);
        let row = usize::from(phase / 4);
        for y in row * 32..(row + 1) * 32 {
            for x in column * 32..(column + 1) * 32 {
                let offset = (y * 128 + x) * 4;
                let colour = moon_cell_colour(phase);
                rgba[offset..offset + 4].copy_from_slice(&[colour[0], colour[1], colour[2], 255]);
            }
        }
    }
    Texture {
        width: 128,
        height: 64,
        rgba,
    }
}

/// The colour [`moon_sheet`] paints the cell of `phase`.
fn moon_cell_colour(phase: u8) -> [u8; 3] {
    [phase * 30, 255 - phase * 30, 100]
}

/// A camera at `feet_y` looking along `yaw` and `pitch`, with `fov` degrees of view.
fn sky_camera(feet_y: f64, yaw: f32, pitch: f32, fov: f32) -> Camera {
    Camera {
        pose: CameraPose {
            position: [0.5, feet_y, 0.5],
            yaw,
            pitch,
        },
        fov_degrees: fov,
        near: NEAR_PLANE,
        far_chunks: 8.0,
        view_effect: NO_VIEW_EFFECT,
    }
}

/// Draws one sky frame into a fresh target — the sky, then the cloud layer when asked — and
/// reads it back. The target clears to the request's colour.
fn render_sky(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    sky: &mut SkyPass,
    cloud: &mut CloudPass,
    request: SkyRequest,
) -> Vec<u8> {
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(device, format);
    let depth = create_depth(device);
    sky.set_params(queue, request.params);
    sky.set_camera(queue, request.camera, 1.0);
    if request.clouds {
        cloud.set_params(queue, request.params);
        cloud.set_camera(queue, request.camera, 1.0);
    }
    let clear = to_color(request.clear);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide sky headless encoder"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("oxide sky headless pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        sky.draw(&mut pass);
        if request.clouds {
            cloud.draw(&mut pass);
        }
    }
    queue.submit(Some(encoder.finish()));
    read_pixels(device, queue, &target)
}

/// A colour as an opaque clear value.
fn to_color(colour: [f32; 3]) -> wgpu::Color {
    wgpu::Color {
        r: f64::from(colour[0]),
        g: f64::from(colour[1]),
        b: f64::from(colour[2]),
        a: 1.0,
    }
}

/// The largest colour channel in a read-back; the alpha byte is the opaque surface's and takes
/// no part.
fn brightest(pixels: &[u8]) -> u8 {
    pixels
        .chunks_exact(4)
        .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
        .max()
        .unwrap_or(0)
}

/// Renders one mesh through the terrain pass with one atlas and reads the frame back.
///
/// The frame draws with no fog set, which is the pass's own default: the shader's mix leaves
/// every fragment as it is.
fn render_terrain(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    atlas: &Atlas,
    mesh: &ChunkMesh,
) -> Vec<u8> {
    render_terrain_with_fog(device, queue, atlas, mesh, None)
}

/// Renders one mesh through the terrain pass with one atlas and `fog`, reads the frame back.
fn render_terrain_with_fog(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    atlas: &Atlas,
    mesh: &ChunkMesh,
    fog: Option<FogParams>,
) -> Vec<u8> {
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(device, format);
    let depth = create_depth(device);
    let mut terrain = TerrainPass::new(device, queue, format);
    terrain.set_atlas(device, queue, atlas);
    terrain.set_camera(queue, frame_camera(), 1.0);
    if let Some(fog) = fog {
        terrain.set_fog(queue, fog);
    }
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
///
/// The corners carry [`FULL_LIGHT`]; a quad that needs another light uses
/// [`push_quad_with_light`].
fn push_quad(
    mesh: &mut ChunkMesh,
    layer: Layer,
    corners: [([f32; 3], [f32; 2]); 4],
    colour: [u8; 4],
) {
    push_quad_with_light(mesh, layer, corners, colour, [FULL_LIGHT; 2]);
}

/// Appends one textured quad whose corners all carry `light`.
fn push_quad_with_light(
    mesh: &mut ChunkMesh,
    layer: Layer,
    corners: [([f32; 3], [f32; 2]); 4],
    colour: [u8; 4],
    light: [u16; 2],
) {
    let target = &mut mesh.layers[layer.index()];
    let base = target.vertices.len() as u32;
    for (position, uv) in corners {
        target.vertices.push(Vertex {
            position,
            uv,
            light,
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
        view_effect: NO_VIEW_EFFECT,
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
            light: [FULL_LIGHT; 2],
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
        view_effect: NO_VIEW_EFFECT,
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

/// Renders one frame through the terrain-sized pass: the colour cleared to the sky, the depth
/// cleared to the far plane, the caller's draws in the given order, then the target read back.
fn render_scene(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &Target,
    depth: &wgpu::TextureView,
    draw: impl FnOnce(&mut wgpu::RenderPass<'_>),
) -> Vec<u8> {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide pipeline headless encoder"),
    });
    with_terrain_pass(&mut encoder, &target.view, depth, draw);
    queue.submit(Some(encoder.finish()));
    read_pixels(device, queue, target)
}

/// The world overlay's outline drawn over the terrain, pixel evidence for the source's state:
/// the aimed block's twelve edges as two-pixel screen-space quads in black at 0.4 alpha with
/// the depth writes off and no culling (`RenderGlobal.java:1875-1901`). The frame is rendered
/// twice — the terrain alone, then the terrain and the overlay — at a fixed camera.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_world_overlay_draws_the_aimed_blocks_outline() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);
    let depth = create_depth(&device);

    let mut terrain = TerrainPass::new(&device, &queue, format);
    terrain.set_atlas(&device, &queue, &solid_atlas(16, [255, 255, 255, 255]));
    terrain.set_camera(&queue, camera(), 1.0);
    terrain.upload(&device, &queue, (0, 0, 0), &stone_block_mesh());

    let mut overlay = WorldOverlay::new(&device, format);
    overlay.set_outline(Some(Outline {
        block: [0, 0, 0],
        shape: FULL_CUBE,
    }));
    overlay.set_frame(&device, &queue, &camera(), [SIZE as f32, SIZE as f32]);

    let without = render_scene(&device, &queue, &target, &depth, |pass| terrain.draw(pass));
    let with = render_scene(&device, &queue, &target, &depth, |pass| {
        terrain.draw(pass);
        overlay.draw(pass);
    });

    // The outline inked pixels the frame did not have, and every pixel it touched came out
    // darker — black at 0.4 alpha over what was there (the blend's 770/771), never brighter.
    let mut changed = 0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let before = pixel(&without, x, y);
            let after = pixel(&with, x, y);
            if before != after {
                changed += 1;
                assert!(
                    after
                        .iter()
                        .zip(before)
                        .all(|(&after, before)| after <= before),
                    "the outline brightened ({x}, {y}): {before:?} -> {after:?}"
                );
            }
        }
    }
    assert!(
        changed > 20,
        "the outline left only {changed} pixels changed: its edges did not reach the frame"
    );

    // The outline's quads sit on the box's edges, not across its faces: the aimed block's top
    // face is untouched and the corners above it keep the sky.
    expect_pixel(
        &with,
        SIZE / 2,
        SIZE / 2,
        STONE,
        "the aimed block's top face",
    );
    expect_pixel(&with, 0, 0, SKY, "the corner above the block");
}

/// The crack over a breaking block: the stage sprite drawn over the block's own cube with the
/// source's ×2 multiply (`tryBlendFuncSeparate(774, 768, 1, 0)`, `RenderGlobal.java:1799`), so
/// the surface darkens towards the sprite and never brightens. The frame is rendered twice —
/// the terrain alone, then the terrain and the overlay — at a fixed camera.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_crack_darkens_the_block_with_the_sources_multiply() {
    // A mid-grey sprite: 2 * 64/255 * dst is darker than either source, so a wrong blend —
    // src-over would lighten the sprite's grey into the frame, a single src×dst would darken
    // twice as far — fails the prediction below.
    const SPRITE: u8 = 64;
    let grey = [SPRITE, SPRITE, SPRITE, 255];

    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);
    let depth = create_depth(&device);

    let mut terrain = TerrainPass::new(&device, &queue, format);
    terrain.set_atlas(&device, &queue, &solid_atlas(16, grey));
    terrain.set_camera(&queue, camera(), 1.0);
    terrain.upload(&device, &queue, (0, 0, 0), &stone_block_mesh());

    let mut overlay = WorldOverlay::new(&device, format);
    overlay.set_atlas(&device, &queue, &solid_atlas(16, grey));
    overlay.set_cracks(vec![Crack {
        block: [0, 0, 0],
        stage: 5,
    }]);
    overlay.set_frame(&device, &queue, &camera(), [SIZE as f32, SIZE as f32]);

    let without = render_scene(&device, &queue, &target, &depth, |pass| terrain.draw(pass));
    let with = render_scene(&device, &queue, &target, &depth, |pass| {
        terrain.draw(pass);
        overlay.draw(pass);
    });

    // Every pixel the crack touched darkened — the multiply's factor on this sprite is below
    // one — and pixels it did not reach are untouched.
    let mut darkened = 0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let before = pixel(&without, x, y);
            let after = pixel(&with, x, y);
            assert!(
                after
                    .iter()
                    .zip(before)
                    .all(|(&after, before)| after <= before + 1),
                "the crack brightened ({x}, {y}): {before:?} -> {after:?}"
            );
            if after != before {
                darkened += 1;
            }
        }
    }
    assert!(
        darkened > 20,
        "the crack left only {darkened} pixels changed: it did not cover the block"
    );

    // The centre pixel is the multiply's own prediction: 2 * sprite/255 * dst, which the unorm
    // target rounds. It is below both sources — the frame's own pixel and the sprite's grey.
    let before = pixel(&without, SIZE / 2, SIZE / 2);
    let after = pixel(&with, SIZE / 2, SIZE / 2);
    assert_eq!(before, [32, 32, 32], "the lit grey block's top face");
    let predicted = (2.0 * f64::from(SPRITE) / 255.0 * f64::from(before[0])).round() as u8;
    for channel in 0..3 {
        assert!(
            (i16::from(after[channel]) - i16::from(predicted)).abs() <= 1,
            "channel {channel} at the centre: got {}, want {predicted} (2 * {SPRITE}/255 * {})",
            after[channel],
            before[channel]
        );
        assert!(
            after[channel] <= before[channel] && after[channel] <= SPRITE,
            "channel {channel} is not below both sources: {} vs dst {} and src {SPRITE}",
            after[channel],
            before[channel]
        );
    }
}

/// The lightmap's runtime rewrite, folded in from the M2 final review's deferred item 34: a
/// change between two draws through the same pass must take the buffer-rewrite path and reach
/// the frame, not read a stale image. The fixture is a quad lit by the (block 0, sky 15) cell —
/// the cell the sun's brightness scales — so a stale texture reads the noon value twice.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn a_lightmap_change_mid_frame_rewrites_the_lightmap() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);
    let depth = create_depth(&device);

    let mut terrain = TerrainPass::new(&device, &queue, format);
    terrain.set_atlas(&device, &queue, &solid_atlas(16, [255, 255, 255, 255]));
    terrain.set_camera(&queue, frame_camera(), 1.0);
    let mut mesh = ChunkMesh::default();
    // `[8, 248]`: block light 0, sky light 15.
    push_quad_with_light(
        &mut mesh,
        Layer::Opaque,
        covering_quad(2.0, [[0.0, 0.0], [1.0, 1.0]]),
        WHITE,
        [8, 248],
    );
    terrain.upload(&device, &queue, (0, 0, 0), &mesh);

    let noon = light_cell_with(1.0, 0, 15);
    let dusk = light_cell_with(0.2, 0, 15);
    assert_ne!(
        noon, dusk,
        "the cell this test reads must move with the sun"
    );

    let first = render_scene(&device, &queue, &target, &depth, |pass| terrain.draw(pass));
    expect_texel(
        &first,
        SIZE / 2,
        SIZE / 2,
        noon,
        "the lightmap the pass was built with",
    );

    terrain.set_lightmap(&queue, 0.2);
    let second = render_scene(&device, &queue, &target, &depth, |pass| terrain.draw(pass));
    expect_texel(&second, SIZE / 2, SIZE / 2, dusk, "the rewritten lightmap");

    terrain.set_lightmap(&queue, 1.0);
    let third = render_scene(&device, &queue, &target, &depth, |pass| terrain.draw(pass));
    expect_texel(
        &third,
        SIZE / 2,
        SIZE / 2,
        noon,
        "the lightmap rewritten back to noon",
    );
}

/// The entity cases' synthetic skin: a 64x64 sheet in one flat colour, so every face of the
/// model reads that colour scaled only by its own shade.
fn flat_sheet(colour: [u8; 4]) -> Texture {
    Texture {
        width: 64,
        height: 64,
        rgba: colour.repeat(64 * 64),
    }
}

/// The cube case's slime sheet: the gel shell's own cells (the sheet's top half, rows 0..32)
/// one colour and the inner body's (its bottom half, the rows the body's `v + 16` uvs
/// address) another, both at the gel's own half alpha, so the wash the layer draws is
/// neither colour alone.
fn slime_shell_sheet() -> Texture {
    const SHELL: [u8; 4] = [200, 60, 40, 128];
    const BODY: [u8; 4] = [60, 140, 220, 128];
    let mut rgba = Vec::with_capacity(64 * 64 * 4);
    for row in 0..64 {
        let colour = if row < 32 { SHELL } else { BODY };
        for _ in 0..64 {
            rgba.extend_from_slice(&colour);
        }
    }
    Texture {
        width: 64,
        height: 64,
        rgba,
    }
}

/// The entity cases' camera: level with an entity standing at the origin, looking at it from
/// +z. Its view rotation is the identity, so the eye-space item lights are already world ones
/// and the model's shaded faces can be computed directly.
///
/// The pose carries the feet position, so it sits [`EYE_HEIGHT`] below the chest's 1.06: the
/// eye lands level with the chest face at the frame's middle.
fn entity_camera() -> Camera {
    Camera {
        pose: CameraPose {
            position: [0.0, 1.06 - f64::from(EYE_HEIGHT), 2.5],
            yaw: 180.0,
            pitch: 0.0,
        },
        fov_degrees: DEFAULT_FOV,
        near: NEAR_PLANE,
        far_chunks: 8.0,
        view_effect: NO_VIEW_EFFECT,
    }
}

/// One player draw at the origin: the wide model, every part on, the rest pose, full
/// brightness, nothing hurting or dying.
fn player_at_origin(texture: TextureRef) -> EntityDraw {
    EntityDraw {
        model: ModelRef::Player {
            slim: false,
            parts: 0x7F,
        },
        position: [0.0; 3],
        body_yaw: 0.0,
        head_yaw: 0.0,
        head_pitch: 0.0,
        pose: Pose::default(),
        texture,
        light: 1.0,
        hurt: 0.0,
        death: 0.0,
        health: Some((20.0, 20.0)),
        nametag: None,
        extra: DrawExtra::None,
    }
}

/// The chest's expected shade: the torso's north face met by the second item light, with the
/// entity camera's identity view rotation.
///
/// The normal is `(0, 0, 1)` — the model's north face after the `180 - body_yaw` half turn —
/// and the light is the eye-space pair rotated into the world, which for this camera is the
/// pair itself: `(0.2, 1.0, -0.7)` and `(-0.2, 1.0, 0.7)`, normalised.
fn chest_shade() -> f32 {
    let light = glam::Vec3::new(-0.2, 1.0, 0.7).normalize();
    let dot = light.z.max(0.0);
    (0.4 + 0.6 * dot).min(1.0)
}

/// The entity texture set the cases upload: a flat skin under the given key and a shadow
/// quad sprite that is white at half alpha, so the shadow's wash is the pass's own alpha.
fn entity_registry(device: &wgpu::Device, queue: &wgpu::Queue, skin: [u8; 4]) -> TextureRegistry {
    let mut registry = TextureRegistry::new(device, queue);
    registry.set_named(device, queue, "entity/test.png", &flat_sheet(skin));
    registry.set_named(
        device,
        queue,
        "misc/shadow.png",
        &flat_sheet([255, 255, 255, 128]),
    );
    registry
}

/// The bounding box of the pixels that are not the sky: `(min x, min y, max x, max y)`.
fn silhouette(pixels: &[u8]) -> (u32, u32, u32, u32) {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (SIZE, SIZE, 0, 0);
    for y in 0..SIZE {
        for x in 0..SIZE {
            if pixel(pixels, x, y) != SKY {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    (min_x, min_y, max_x, max_y)
}

/// The count of pixels two frames disagree on.
fn changed_pixels(before: &[u8], after: &[u8]) -> usize {
    (0..SIZE)
        .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
        .filter(|&(x, y)| pixel(before, x, y) != pixel(after, x, y))
        .count()
}

/// The entity pass draws the player's own boxes: the chest face comes back in the synthetic
/// skin's colour under its shade, the silhouette stands out of the empty sky, and the shadow
/// quad washes the ground below the feet.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_entity_pass_draws_the_player_model() {
    const SKIN: [u8; 4] = [200, 90, 40, 255];
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);

    let registry = entity_registry(&device, &queue, SKIN);
    let mut entities = EntityPass::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        registry.layout(),
    );
    entities.set_camera(entity_camera(), 1.0);
    let draw = player_at_origin(TextureRef::Named("entity/test.png"));

    let empty = render_scene(&device, &queue, &target, &depth, |pass| {
        entities.draw(&device, pass, &[], &registry);
    });
    // Nothing drawn: every pixel is the clear sky.
    for y in 0..SIZE {
        for x in 0..SIZE {
            expect_pixel_exact(&empty, x, y, SKY, "the empty entity frame");
        }
    }

    let filled = render_scene(&device, &queue, &target, &depth, |pass| {
        entities.draw(&device, pass, &[draw], &registry);
    });

    let (min_x, min_y, max_x, max_y) = silhouette(&filled);
    assert!(
        max_x - min_x >= 5 && max_y - min_y >= 20,
        "the silhouette is a standing model, got ({min_x}, {min_y})..({max_x}, {max_y})"
    );

    // The chest: the north face at the frame's middle, the skin's colour through the shade.
    let shade = chest_shade();
    let chest = [
        (200.0 * shade).round() as u8,
        (90.0 * shade).round() as u8,
        (40.0 * shade).round() as u8,
    ];
    expect_pixel(&filled, SIZE / 2, SIZE / 2, chest, "the chest face");

    // The shadow: a way below the middle the ground is washed towards white — the sprite's
    // half alpha times the pass's own fade over the sky.
    let shadow = pixel(&filled, SIZE / 2, 53);
    for (channel, sky) in shadow.iter().zip(SKY) {
        assert!(
            *channel >= sky,
            "the shadow washes the sky, got {shadow:?} against {SKY:?}"
        );
    }
    assert_ne!(shadow, SKY, "the shadow is not the bare sky");
}

/// The hurt overlay re-draws the model's boxes through the source's `0.7 red + 0.3 x` mix:
/// the chest pixel moves towards red between the two frames.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hurt_overlay_mixes_red_into_the_chest() {
    const SKIN: [u8; 4] = [200, 90, 40, 255];
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);

    let registry = entity_registry(&device, &queue, SKIN);
    let mut entities = EntityPass::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        registry.layout(),
    );
    entities.set_camera(entity_camera(), 1.0);
    let draw = player_at_origin(TextureRef::Named("entity/test.png"));

    let calm = render_scene(&device, &queue, &target, &depth, |pass| {
        entities.draw(&device, pass, std::slice::from_ref(&draw), &registry);
    });
    let hurt = render_scene(&device, &queue, &target, &depth, |pass| {
        let hurt = EntityDraw {
            hurt: 1.0,
            ..draw.clone()
        };
        entities.draw(&device, pass, &[hurt], &registry);
    });

    let shade = chest_shade();
    let calm_chest = [
        (200.0 * shade).round() as u8,
        (90.0 * shade).round() as u8,
        (40.0 * shade).round() as u8,
    ];
    expect_pixel(&calm, SIZE / 2, SIZE / 2, calm_chest, "the calm chest face");

    // The mix runs on the shader's floats: 0.7 of the shaded skin plus 0.3 of red.
    let mixed = [
        (0.7 * 200.0 * shade + 0.3 * 255.0).round() as u8,
        (0.7 * 90.0 * shade).round() as u8,
        (0.7 * 40.0 * shade).round() as u8,
    ];
    expect_pixel(&hurt, SIZE / 2, SIZE / 2, mixed, "the hurt chest face");
    let (calm_pixel, hurt_pixel) = (
        pixel(&calm, SIZE / 2, SIZE / 2),
        pixel(&hurt, SIZE / 2, SIZE / 2),
    );
    assert!(
        hurt_pixel[0] > calm_pixel[0] && hurt_pixel[1] < calm_pixel[1],
        "the hurt mix is redder: {calm_pixel:?} became {hurt_pixel:?}"
    );
}

/// The death ramp tips the model a quarter turn about Z: the standing silhouette — tall and
/// narrow — becomes one lying along the ground, wider than it was tall, and the chest pixel's
/// spot in front of the camera is sky again.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_death_tilt_lays_the_model_down() {
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);

    let registry = entity_registry(&device, &queue, [200, 90, 40, 255]);
    let mut entities = EntityPass::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        registry.layout(),
    );
    entities.set_camera(entity_camera(), 1.0);
    let draw = player_at_origin(TextureRef::Named("entity/test.png"));

    let alive = render_scene(&device, &queue, &target, &depth, |pass| {
        entities.draw(&device, pass, std::slice::from_ref(&draw), &registry);
    });
    let dead = render_scene(&device, &queue, &target, &depth, |pass| {
        let fallen = EntityDraw {
            death: 1.0,
            ..draw.clone()
        };
        entities.draw(&device, pass, &[fallen], &registry);
    });

    let (min_x, min_y, max_x, max_y) = silhouette(&alive);
    let (dead_min_x, dead_min_y, dead_max_x, dead_max_y) = silhouette(&dead);
    let (alive_width, alive_height) = (max_x - min_x, max_y - min_y);
    let (dead_width, dead_height) = (dead_max_x - dead_min_x, dead_max_y - dead_min_y);
    // The fall turns the silhouette over: standing the model is taller than it is wide;
    // fallen it lies wider than it is tall and shorter than it stood. The fallen model's
    // head projects past the frame's right edge, so the two widths are compared to each
    // other rather than to a fixed margin.
    assert!(
        alive_height > alive_width,
        "the standing model is taller than wide, got {alive_width} wide by {alive_height} tall"
    );
    assert!(
        dead_width > dead_height,
        "the fallen model lies wider than tall, got {dead_width} wide by {dead_height} tall"
    );
    assert!(
        dead_width > alive_width,
        "the fallen model lies wider, alive {alive_width} wide, fallen {dead_width} wide"
    );
    assert!(
        dead_height < alive_height,
        "the fallen model is shorter, alive {alive_height} tall, fallen {dead_height} tall"
    );
    expect_pixel_exact(
        &dead,
        SIZE / 2,
        SIZE / 2,
        SKY,
        "the chest spot once the model lies down",
    );
    assert!(
        changed_pixels(&alive, &dead) > 100,
        "the two death fractions draw different frames"
    );
}

/// A 128x128 synthetic ascii sheet: the `A` cell inks its left five columns over every
/// row, so the font's scan measures six font pixels and the glyph covers most of its cell.
fn nametag_sheet() -> Texture {
    let mut rgba = vec![0u8; (128 * 128 * 4) as usize];
    let code = 'A' as u32;
    let (cell_x, cell_y) = ((code % 16) * 8, (code / 16) * 8);
    for row in 0..8 {
        for column in 0..=4 {
            let offset = (((cell_y + row) * 128 + cell_x + column) * 4) as usize;
            rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    Texture {
        width: 128,
        height: 128,
        rgba,
    }
}

/// The player at the origin carrying the given nametag text.
fn tagged(text: &str) -> EntityDraw {
    EntityDraw {
        nametag: Some(NametagDraw { text: text.into() }),
        ..player_at_origin(TextureRef::Named("entity/test.png"))
    }
}

/// The height the range cases aim at: the player's tag band, a touch under the anchor's
/// `height + 0.5` = 2.3.
const TAG_AIM_HEIGHT: f64 = 2.2;

/// The range cases' camera: the entity camera's facing pulled back to `distance` blocks
/// from the origin's feet and zoomed in to a two-degree field of view, so a tag at that
/// range still covers whole fragments. The pose aims the view up at the tag's band
/// (positive pitch looks down, [`CameraPose::forward`]).
fn tag_camera(distance: f64) -> Camera {
    Camera {
        pose: CameraPose {
            position: [0.0, 1.06 - f64::from(EYE_HEIGHT), distance],
            yaw: 180.0,
            pitch: -((TAG_AIM_HEIGHT - 1.06) / distance).atan().to_degrees() as f32,
        },
        fov_degrees: 2.0,
        near: NEAR_PLANE,
        far_chunks: 8.0,
        view_effect: NO_VIEW_EFFECT,
    }
}

/// The nametag draws its box and its glyph above the model's head: the silhouette grows
/// upwards, the solid pass lands near-white texels there, and the untagged frame at the
/// same pose holds none of it.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_nametag_draws_its_text_above_the_entity() {
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);
    let registry = entity_registry(&device, &queue, [200, 90, 40, 255]);
    let mut entities = EntityPass::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        registry.layout(),
    );
    entities.set_camera(entity_camera(), 1.0);
    entities
        .set_font(&device, &queue, &nametag_sheet())
        .expect("the synthetic sheet loads");

    let untagged = render_scene(&device, &queue, &target, &depth, |pass| {
        let draw = player_at_origin(TextureRef::Named("entity/test.png"));
        entities.draw(&device, pass, &[draw], &registry);
    });
    let with_tag = render_scene(&device, &queue, &target, &depth, |pass| {
        entities.draw(&device, pass, std::slice::from_ref(&tagged("A")), &registry);
    });

    let (_, body_top, _, _) = silhouette(&untagged);
    let (_, tag_top, _, _) = silhouette(&with_tag);
    assert!(
        tag_top + 3 <= body_top,
        "the tagged silhouette reaches above the body: body top {body_top}, tag top {tag_top}"
    );
    // The solid pass: at least one near-white pixel in the band above the body — the box
    // alone washes black at a quarter alpha, which is darker than the sky, never white.
    let bright = (0..body_top)
        .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
        .filter(|&(x, y)| pixel(&with_tag, x, y).iter().all(|channel| *channel >= 250))
        .count();
    assert!(
        bright >= 1,
        "the solid text pass lands near-white pixels above the body, got {bright}"
    );
    let changed = changed_pixels(&untagged, &with_tag);
    eprintln!(
        "near tag: {changed} changed pixels, body top {body_top}, tag top {tag_top}, \
         {bright} near-white"
    );
    // Measured 16 changed pixels at the pinning run: the box's wash and the glyph.
    assert!(
        changed >= 8,
        "the tag's box and glyph cover whole pixels, got {changed} changed"
    );
}

/// The range rule: an in-range tag adds pixels over the untagged frame at the same pose,
/// and one past the range adds none — 64 blocks standing, 32 sneaking, both the source's
/// own constants (`RendererLivingEntity.java`:499).
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_nametag_keeps_and_drops_at_its_ranges() {
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);
    let registry = entity_registry(&device, &queue, [200, 90, 40, 255]);
    let mut entities = EntityPass::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        registry.layout(),
    );
    entities
        .set_font(&device, &queue, &nametag_sheet())
        .expect("the synthetic sheet loads");

    // The controls carry the same pose as the tagged draws, so the only difference
    // between the pair is the tag itself — the sneak pose alone would crouch the body.
    let plain = player_at_origin(TextureRef::Named("entity/test.png"));
    let mut plain_sneak = plain.clone();
    plain_sneak.pose.sneak = true;
    let named = tagged("A");
    let mut sneaking = named.clone();
    sneaking.pose.sneak = true;

    let contribution = |entities: &mut EntityPass,
                        distance: f64,
                        control: &EntityDraw,
                        draw: &EntityDraw|
     -> usize {
        entities.set_camera(tag_camera(distance), 1.0);
        let untagged = render_scene(&device, &queue, &target, &depth, |pass| {
            entities.draw(&device, pass, std::slice::from_ref(control), &registry);
        });
        let tagged_frame = render_scene(&device, &queue, &target, &depth, |pass| {
            entities.draw(&device, pass, std::slice::from_ref(draw), &registry);
        });
        changed_pixels(&untagged, &tagged_frame)
    };

    let inside = contribution(&mut entities, 63.9, &plain, &named);
    let outside = contribution(&mut entities, 65.0, &plain, &named);
    eprintln!("standing: 63.9 -> {inside} px, 65.0 -> {outside} px");
    // Measured 42 pixels at the pinning run.
    assert!(
        inside >= 20,
        "the standing tag draws just inside 64 blocks, got {inside} changed pixels"
    );
    assert_eq!(
        outside, 0,
        "past 64 blocks the standing tag draws nothing, got {outside} changed pixels"
    );

    let inside = contribution(&mut entities, 31.5, &plain_sneak, &sneaking);
    let outside = contribution(&mut entities, 33.0, &plain_sneak, &sneaking);
    eprintln!("sneaking: 31.5 -> {inside} px, 33.0 -> {outside} px");
    // Measured 168 pixels at the pinning run.
    assert!(
        inside >= 80,
        "the sneaking tag draws just inside 32 blocks, got {inside} changed pixels"
    );
    assert_eq!(
        outside, 0,
        "past 32 blocks the sneaking tag draws nothing, got {outside} changed pixels"
    );
}

/// The see-through rule as the source holds it: the standing path draws its box and its
/// faint pass with the depth test off and re-enables it only for the solid pass
/// (`Render.java`:348-349, `:371-372`), so a standing tag in front of a wall still washes
/// through; the sneaking branch keeps the test on for both its passes
/// (`RendererLivingEntity.java`:518, `:532`), so a sneaking tag behind the same wall
/// leaves the frame untouched.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_nametag_sees_through_walls_standing_but_not_sneaking() {
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);
    let registry = entity_registry(&device, &queue, [200, 90, 40, 255]);
    let mut entities = EntityPass::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        registry.layout(),
    );
    entities.set_camera(entity_camera(), 1.0);
    entities
        .set_font(&device, &queue, &nametag_sheet())
        .expect("the synthetic sheet loads");

    // The wall: a giant's body a block and a half in front of the tagged player, filling
    // the frame's middle.
    let wall = EntityDraw {
        position: [0.0, 0.0, 1.0],
        ..mob_at_origin(ModelRef::Giant, "entity/test.png", DrawExtra::None)
    };
    let plain = player_at_origin(TextureRef::Named("entity/test.png"));
    let mut sneaking = tagged("A");
    sneaking.pose.sneak = true;

    let frame = |entities: &mut EntityPass, draw: &EntityDraw| -> Vec<u8> {
        let draws = [wall.clone(), draw.clone()];
        render_scene(&device, &queue, &target, &depth, |pass| {
            entities.draw(&device, pass, &draws, &registry);
        })
    };

    let bare = frame(&mut entities, &plain);
    let standing = frame(&mut entities, &tagged("A"));
    let sneaking_frame = frame(&mut entities, &sneaking);
    let standing_diff = changed_pixels(&bare, &standing);
    let sneaking_diff = changed_pixels(&bare, &sneaking_frame);
    eprintln!("through a wall: standing {standing_diff} px, sneaking {sneaking_diff} px");
    // Measured 16 pixels at the pinning run; the sneaking pair's exact zero is the
    // discriminating read.
    assert!(
        standing_diff >= 8,
        "the standing tag's box and faint pass wash through the wall, got {standing_diff} \
         changed pixels"
    );
    assert_eq!(
        sneaking_diff, 0,
        "the sneaking tag's passes are depth-tested, got {sneaking_diff} changed pixels"
    );
}

/// The entities sit between the terrain's solid and translucent layers: a translucent quad
/// in front of an entity lets the entity show through the blend when the entity draws first
/// (the source's order) and hides it when the order is inverted — the canary that proves the
/// order matters.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_entities_draw_between_the_terrain_layers() {
    const WATER: [u8; 4] = [0, 255, 0, 128];
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);

    // A translucent quad at z = +1, between the camera (z = 2.5) and the entity (z = 0),
    // covering the frame; its windings face the camera like every terrain quad.
    let mut mesh = ChunkMesh::default();
    push_quad(
        &mut mesh,
        Layer::Translucent,
        [
            ([-2.0, -2.0, 1.0], [0.0, 0.0]),
            ([2.0, -2.0, 1.0], [1.0, 0.0]),
            ([2.0, 2.0, 1.0], [1.0, 1.0]),
            ([-2.0, 2.0, 1.0], [0.0, 1.0]),
        ],
        WATER,
    );

    let mut terrain = TerrainPass::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
    terrain.set_atlas(&device, &queue, &solid_atlas(4, [255, 255, 255, 255]));
    terrain.set_camera(&queue, entity_camera(), 1.0);
    terrain.upload(&device, &queue, (0, 0, 0), &mesh);

    let registry = entity_registry(&device, &queue, [255, 0, 0, 255]);
    let mut entities = EntityPass::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        registry.layout(),
    );
    entities.set_camera(entity_camera(), 1.0);
    let draw = player_at_origin(TextureRef::Named("entity/test.png"));

    // The quad alone: its green through the brightest lightmap cell over the sky.
    let water = render_scene(&device, &queue, &target, &depth, |pass| {
        terrain.draw_solid(pass);
        terrain.draw_translucent(pass);
    });
    let water_over_sky = [
        (0.498 * 158.0) as u8,
        (0.502 * 252.0 + 0.498 * 194.0) as u8,
        (0.498 * 250.0) as u8,
    ];
    expect_pixel(
        &water,
        SIZE / 2,
        SIZE / 2,
        water_over_sky,
        "the quad over the sky",
    );

    // The source's order: solid, entities, translucent. The entity's chest red shows through
    // the quad's half alpha.
    let in_order = render_scene(&device, &queue, &target, &depth, |pass| {
        terrain.draw_solid(pass);
        entities.draw(&device, pass, std::slice::from_ref(&draw), &registry);
        terrain.draw_translucent(pass);
    });
    let chest_red = (255.0 * chest_shade()).round();
    let blend = [(0.498 * chest_red) as u8, (0.502 * 252.0) as u8, 0];
    expect_pixel(
        &in_order,
        SIZE / 2,
        SIZE / 2,
        blend,
        "the chest through the water",
    );

    // The order inverted: the translucent layer writes no depth, so the later entity draws
    // over it — the fixture must differ from the source's order, or it proves nothing.
    let inverted = render_scene(&device, &queue, &target, &depth, |pass| {
        terrain.draw_solid(pass);
        terrain.draw_translucent(pass);
        entities.draw(&device, pass, &[draw], &registry);
    });
    let raw = [(255.0 * chest_shade()).round() as u8, 0, 0];
    expect_pixel(
        &inverted,
        SIZE / 2,
        SIZE / 2,
        raw,
        "the raw chest over the water",
    );
    assert!(
        changed_pixels(&in_order, &inverted) > 20,
        "the order fixture differs from its inversion"
    );
}

/// One mob draw at the origin with the given model, sheet and extras.
fn mob_at_origin(model: ModelRef, sheet: &'static str, extra: DrawExtra) -> EntityDraw {
    EntityDraw {
        model,
        position: [0.0; 3],
        body_yaw: 0.0,
        head_yaw: 0.0,
        head_pitch: 0.0,
        pose: Pose::default(),
        texture: TextureRef::Named(sheet),
        light: 1.0,
        hurt: 0.0,
        death: 0.0,
        health: None,
        nametag: None,
        extra,
    }
}

/// The entity registry with each key's own flat sheet and the shadow sprite.
fn mob_registry(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    sheets: &[(&'static str, [u8; 4])],
) -> TextureRegistry {
    let mut registry = TextureRegistry::new(device, queue);
    for (key, colour) in sheets {
        registry.set_named(device, queue, key, &flat_sheet(*colour));
    }
    registry.set_named(
        device,
        queue,
        "misc/shadow.png",
        &flat_sheet([255, 255, 255, 128]),
    );
    registry
}

/// Renders one draw on the entity camera and returns the frame.
fn render_mob(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &Target,
    depth: &wgpu::TextureView,
    entities: &mut EntityPass,
    registry: &TextureRegistry,
    draw: EntityDraw,
) -> Vec<u8> {
    render_scene(device, queue, target, depth, |pass| {
        entities.draw(device, pass, &[draw], registry);
    })
}

/// The count of pixels that are not the sky.
fn model_pixels(pixels: &[u8]) -> usize {
    (0..SIZE)
        .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
        .filter(|&(x, y)| pixel(pixels, x, y) != SKY)
        .count()
}

/// The count of pixels carrying the test sheet's hue: its red is well above its green, its
/// green above its blue, so any shaded texel of it keeps that ordering while the sky and
/// the grey shadow do not.
fn hued_pixels(pixels: &[u8]) -> usize {
    (0..SIZE)
        .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let [r, g, b] = pixel(pixels, x, y);
            r as i32 > g as i32 + 30 && g as i32 > b as i32 + 10
        })
        .count()
}

/// The count of pixels whose green is well above both other channels: the saddle sheet's
/// hue, which neither the sky (its green sits under its red) nor a white sheet carries.
fn green_pixels(pixels: &[u8]) -> usize {
    (0..SIZE)
        .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let [r, g, b] = pixel(pixels, x, y);
            g as i32 > r as i32 + 40 && g as i32 > b as i32 + 40
        })
        .count()
}

/// Every biped draws its own model: each silhouette stands in the frame, carries its own
/// class sheet's hue, and the classes that differ from the wide biped read differently —
/// the skeleton's thin limbs cover fewer pixels than the zombie's, the giant's sixfold
/// scale fills the frame, and the snow golem stands wider than tall.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_biped_family_draws_its_own_silhouettes() {
    const SKIN: [u8; 4] = [200, 90, 40, 255];
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);

    let registry = mob_registry(
        &device,
        &queue,
        &[
            ("entity/zombie/zombie.png", SKIN),
            ("entity/zombie/zombie_villager.png", SKIN),
            ("entity/skeleton/skeleton.png", SKIN),
            ("entity/villager/farmer.png", SKIN),
            ("entity/witch.png", SKIN),
            ("entity/snowman.png", SKIN),
            ("entity/iron_golem.png", SKIN),
        ],
    );
    let mut entities = EntityPass::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        registry.layout(),
    );
    entities.set_camera(entity_camera(), 1.0);

    let frames = [
        (
            "the zombie",
            mob_at_origin(
                ModelRef::Zombie,
                "entity/zombie/zombie.png",
                DrawExtra::None,
            ),
        ),
        (
            "the zombie villager",
            mob_at_origin(
                ModelRef::ZombieVillager,
                "entity/zombie/zombie_villager.png",
                DrawExtra::ZombieVillager,
            ),
        ),
        (
            "the skeleton",
            mob_at_origin(
                ModelRef::Skeleton,
                "entity/skeleton/skeleton.png",
                DrawExtra::None,
            ),
        ),
        (
            "the villager",
            mob_at_origin(
                ModelRef::Villager {
                    profession: 0,
                    child: false,
                },
                "entity/villager/farmer.png",
                DrawExtra::Villager {
                    profession: 0,
                    child: false,
                },
            ),
        ),
        (
            "the witch",
            mob_at_origin(ModelRef::Witch, "entity/witch.png", DrawExtra::None),
        ),
        (
            "the giant",
            mob_at_origin(ModelRef::Giant, "entity/zombie/zombie.png", DrawExtra::None),
        ),
        (
            "the snow golem",
            mob_at_origin(ModelRef::SnowGolem, "entity/snowman.png", DrawExtra::None),
        ),
        (
            "the iron golem",
            mob_at_origin(
                ModelRef::IronGolem,
                "entity/iron_golem.png",
                DrawExtra::None,
            ),
        ),
    ];

    let mut rendered = Vec::new();
    for (name, draw) in frames {
        rendered.push((
            name,
            render_mob(
                &device,
                &queue,
                &target,
                &depth,
                &mut entities,
                &registry,
                draw,
            ),
        ));
    }

    for (name, frame) in &rendered {
        let (min_x, min_y, max_x, max_y) = silhouette(frame);
        assert!(
            max_x - min_x >= 4 && max_y - min_y >= 16,
            "{name} stands in the frame, got ({min_x}, {min_y})..({max_x}, {max_y})"
        );
        assert!(
            hued_pixels(frame) >= 30,
            "{name} draws its own sheet's hue, {} pixels",
            hued_pixels(frame)
        );
    }

    // The skeleton's two-wide limbs read fewer pixels than the zombie's four-wide ones at
    // the same camera (`ModelSkeleton.java`:31-43).
    assert!(
        model_pixels(&rendered[2].1) < model_pixels(&rendered[0].1),
        "the skeleton's thin limbs cover fewer pixels: {} against {}",
        model_pixels(&rendered[2].1),
        model_pixels(&rendered[0].1)
    );
    // The giant's pre-render callback scales the whole model sixfold
    // (`RenderGiantZombie.preRenderCallback`:44): the frame is all but covered.
    let giant = silhouette(&rendered[5].1);
    assert!(
        giant.3 - giant.1 >= 55 && giant.2 - giant.0 >= 40,
        "the giant fills the frame, got ({}, {})..({}, {})",
        giant.0,
        giant.1,
        giant.2,
        giant.3
    );
    assert!(
        model_pixels(&rendered[5].1) > model_pixels(&rendered[0].1),
        "the giant covers more than the zombie"
    );
    // The snow golem's base is twelve units wide where the zombie is eight, and its head
    // seven (`ModelSnowMan.java`:18-20): the silhouette stands and spreads.
    let snow = silhouette(&rendered[6].1);
    assert!(
        snow.2 - snow.0 >= 14 && snow.3 - snow.1 >= 25,
        "the snow golem stands, got ({}, {})..({}, {})",
        snow.0,
        snow.1,
        snow.2,
        snow.3
    );
}

/// Every quadruped draws its own model: each stands in the frame wider than tall, carries
/// its class sheet's hue, and the mooshroom draws the cow's model off its own sheet.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_quadruped_family_draws_its_own_silhouettes() {
    const COW_SKIN: [u8; 4] = [200, 90, 40, 255];
    const PIG_SKIN: [u8; 4] = [90, 200, 40, 255];
    const SHEEP_SKIN: [u8; 4] = [40, 90, 200, 255];
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);

    let registry = mob_registry(
        &device,
        &queue,
        &[
            ("entity/cow/cow.png", COW_SKIN),
            ("entity/cow/mooshroom.png", COW_SKIN),
            ("entity/pig/pig.png", PIG_SKIN),
            ("entity/sheep/sheep.png", SHEEP_SKIN),
            ("entity/sheep/sheep_fur.png", [255, 255, 255, 255]),
        ],
    );
    let mut entities = EntityPass::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        registry.layout(),
    );
    entities.set_camera(entity_camera(), 1.0);

    let frames = [
        (
            "the cow",
            mob_at_origin(ModelRef::Cow, "entity/cow/cow.png", DrawExtra::None),
        ),
        (
            "the mooshroom",
            mob_at_origin(
                ModelRef::Mooshroom,
                "entity/cow/mooshroom.png",
                DrawExtra::None,
            ),
        ),
        (
            "the pig",
            mob_at_origin(
                ModelRef::Pig { saddle: false },
                "entity/pig/pig.png",
                DrawExtra::Pig { saddle: false },
            ),
        ),
        (
            "the sheep",
            mob_at_origin(
                ModelRef::Sheep {
                    wool: 0,
                    sheared: true,
                },
                "entity/sheep/sheep.png",
                DrawExtra::Sheep {
                    wool: 0,
                    sheared: true,
                },
            ),
        ),
    ];

    let mut rendered = Vec::new();
    for (name, draw) in frames {
        rendered.push((
            name,
            render_mob(
                &device,
                &queue,
                &target,
                &depth,
                &mut entities,
                &registry,
                draw,
            ),
        ));
    }

    for (name, frame) in &rendered {
        let (min_x, min_y, max_x, max_y) = silhouette(frame);
        assert!(
            max_x - min_x >= 8 && max_y - min_y >= 6,
            "{name} stands in the frame, got ({min_x}, {min_y})..({max_x}, {max_y})"
        );
        assert!(
            max_y - min_y >= 18,
            "{name} stands in the frame, got ({min_x}, {min_y})..({max_x}, {max_y})"
        );
        assert!(
            model_pixels(frame) >= 150,
            "{name} covers pixels, {}",
            model_pixels(frame)
        );
    }
    // Six-high legs and a sixteen-long body sit smaller than the cow's twelve-high legs
    // and eighteen-long body (`ModelPig.java`:12, `ModelCow.java`:13-17).
    assert!(
        model_pixels(&rendered[2].1) < model_pixels(&rendered[0].1),
        "the pig covers fewer pixels than the cow: {} against {}",
        model_pixels(&rendered[2].1),
        model_pixels(&rendered[0].1)
    );
    assert!(
        model_pixels(&rendered[3].1) < model_pixels(&rendered[0].1),
        "the sheep covers fewer pixels than the cow: {} against {}",
        model_pixels(&rendered[3].1),
        model_pixels(&rendered[0].1)
    );
    // The cow and the mooshroom share the model; the sheep's own sheet still binds.
    assert_eq!(
        silhouette(&rendered[0].1),
        silhouette(&rendered[1].1),
        "the mooshroom draws the cow's model"
    );
}

/// The layers draw over their models: the sheep's wool re-draws the grown fleece in the
/// fleece colour's tint unless the sheep is sheared (`LayerSheepWool.java`:27-28), and the
/// pig's saddle only when the pig carries one (`LayerSaddle.java`:25-29).
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_wool_and_saddle_layers_draw_over_their_models() {
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);

    // The sheep's own sheet and the pig's are white flats; the fur sheet and the saddle
    // sheet carry their own colours, so a layer's pixels are unmistakable.
    let registry = mob_registry(
        &device,
        &queue,
        &[
            ("entity/sheep/sheep.png", [255, 255, 255, 255]),
            ("entity/sheep/sheep_fur.png", [255, 255, 255, 255]),
            ("entity/pig/pig.png", [255, 255, 255, 255]),
            ("entity/pig/pig_saddle.png", [30, 220, 60, 255]),
        ],
    );
    let mut entities = EntityPass::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        registry.layout(),
    );
    entities.set_camera(entity_camera(), 1.0);

    // A white-woolled sheep: the wool layer draws, but its tint is white too, so a sheared
    // and an unsheared sheep differ — the sheared one has no fleece geometry at all.
    let unsheared = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        mob_at_origin(
            ModelRef::Sheep {
                wool: 14,
                sheared: false,
            },
            "entity/sheep/sheep.png",
            DrawExtra::Sheep {
                wool: 14,
                sheared: false,
            },
        ),
    );
    // The wool colour 14 is the red [0.6, 0.2, 0.2] (`EntitySheep.java`:372-387): its
    // pixels are the sheet's white multiplied by that tint, red well above green.
    let red = (0..SIZE)
        .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let [r, g, _] = pixel(&unsheared, x, y);
            r as i32 > g as i32 + 30
        })
        .count();
    assert!(
        red >= 50,
        "the unsheared sheep's wool is tinted in its fleece colour, {red} pixels"
    );
    let sheared = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        mob_at_origin(
            ModelRef::Sheep {
                wool: 14,
                sheared: true,
            },
            "entity/sheep/sheep.png",
            DrawExtra::Sheep {
                wool: 14,
                sheared: true,
            },
        ),
    );
    let sheared_red = (0..SIZE)
        .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let [r, g, _] = pixel(&sheared, x, y);
            r as i32 > g as i32 + 30
        })
        .count();
    assert_eq!(
        sheared_red, 0,
        "a sheared sheep draws no fleece, {sheared_red} tinted pixels"
    );
    assert!(
        model_pixels(&sheared) < model_pixels(&unsheared),
        "the fleece covers pixels the sheared sheep does not: {} against {}",
        model_pixels(&sheared),
        model_pixels(&unsheared)
    );

    // The pig's saddle: the layer draws only while the pig carries one, in the saddle
    // sheet's colour.
    let saddled = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        mob_at_origin(
            ModelRef::Pig { saddle: true },
            "entity/pig/pig.png",
            DrawExtra::Pig { saddle: true },
        ),
    );
    assert!(
        green_pixels(&saddled) >= 6,
        "the saddled pig carries the saddle sheet's colour, {} pixels",
        green_pixels(&saddled)
    );
    let bare = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        mob_at_origin(
            ModelRef::Pig { saddle: false },
            "entity/pig/pig.png",
            DrawExtra::Pig { saddle: false },
        ),
    );
    assert_eq!(
        green_pixels(&bare),
        0,
        "an unsaddled pig carries no saddle pixels"
    );
    assert!(
        changed_pixels(&saddled, &bare) >= 6,
        "the saddle layer changes the frame: {} pixels",
        changed_pixels(&saddled, &bare)
    );
}

/// The crawler family draws its own models — the creeper's stocky stand, the spider's eight
/// legs fanned wider than it stands, the cave spider on the spider's model at its pre-render
/// callback's seventh-tenths scale, the enderman over-topping both on its thirty-unit limbs
/// (`ModelCreeper.java`:23-44, `ModelSpider.java`:43-77, `ModelSpider.render`:86-96,
/// `RenderCaveSpider.java`:23, `ModelEnderman.java`:23-36) — and the spiders' and the
/// enderman's eyes layers draw over their models: the eyes sheet's colours added onto the
/// body at the source's constant full-bright lightmap (`LayerSpiderEyes.java`:24,:35-38,
/// `LayerEndermanEyes.java`:24,:27-30), so the addition follows the eyes sheet's own texels
/// and not the draw's brightness.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_crawler_family_draws_its_own_silhouettes() {
    const SKIN: [u8; 4] = [200, 90, 40, 255];
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);

    let mut registry = mob_registry(
        &device,
        &queue,
        &[
            ("entity/creeper/creeper.png", SKIN),
            ("entity/spider/spider.png", SKIN),
            ("entity/spider/cave_spider.png", SKIN),
            ("entity/spider_eyes.png", [0, 0, 0, 255]),
            ("entity/enderman/enderman.png", SKIN),
            ("entity/enderman/enderman_eyes.png", [0, 0, 0, 255]),
        ],
    );
    let mut entities = EntityPass::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        registry.layout(),
    );
    entities.set_camera(entity_camera(), 1.0);

    let frames = [
        (
            "the creeper",
            mob_at_origin(
                ModelRef::Creeper,
                "entity/creeper/creeper.png",
                DrawExtra::Creeper,
            ),
        ),
        (
            "the spider",
            mob_at_origin(
                ModelRef::Spider,
                "entity/spider/spider.png",
                DrawExtra::None,
            ),
        ),
        (
            "the cave spider",
            mob_at_origin(
                ModelRef::CaveSpider,
                "entity/spider/cave_spider.png",
                DrawExtra::None,
            ),
        ),
        (
            "the enderman",
            mob_at_origin(
                ModelRef::Enderman,
                "entity/enderman/enderman.png",
                DrawExtra::None,
            ),
        ),
    ];
    let mut rendered = Vec::new();
    for (name, draw) in frames {
        let frame = render_mob(
            &device,
            &queue,
            &target,
            &depth,
            &mut entities,
            &registry,
            draw,
        );
        let (min_x, min_y, max_x, max_y) = silhouette(&frame);
        assert!(
            max_x - min_x >= 4 && max_y - min_y >= 8,
            "{name} stands in the frame, got ({min_x}, {min_y})..({max_x}, {max_y})"
        );
        assert!(
            model_pixels(&frame) >= 60 && hued_pixels(&frame) >= 30,
            "{name} draws its own sheet's hue: {} hued of {} pixels",
            hued_pixels(&frame),
            model_pixels(&frame)
        );
        rendered.push((name, frame));
    }

    // The creeper is taller than wide; the spider's eight-leg spread is the other way
    // (`ModelCreeper.java`:30-43, `ModelSpider.render`:86-96).
    let creeper = silhouette(&rendered[0].1);
    let creeper_height = creeper.3 - creeper.1;
    assert!(
        creeper_height > creeper.2 - creeper.0,
        "the creeper stands taller than wide, got {} wide by {creeper_height} tall",
        creeper.2 - creeper.0
    );
    let spider = silhouette(&rendered[1].1);
    assert!(
        spider.2 - spider.0 > spider.3 - spider.1,
        "the spider spreads wider than it stands, got {} wide by {} tall",
        spider.2 - spider.0,
        spider.3 - spider.1
    );
    // The cave spider draws the spider's model at the seventh-tenths scale its own pre-render
    // callback sets (`RenderCaveSpider.java`:23).
    let cave = silhouette(&rendered[2].1);
    assert!(
        cave.2 - cave.0 < spider.2 - spider.0 && cave.3 - cave.1 < spider.3 - spider.1,
        "the cave spider draws smaller than the spider: ({}, {})..({}, {}) against ({}, {})..({}, {})",
        cave.0,
        cave.1,
        cave.2,
        cave.3,
        spider.0,
        spider.1,
        spider.2,
        spider.3
    );
    // The enderman over-tops the creeper on its thirty-unit limbs (`ModelEnderman.java`:23-36).
    let enderman = silhouette(&rendered[3].1);
    assert!(
        enderman.3 - enderman.1 > creeper_height + 6,
        "the enderman over-tops the creeper: {} against {creeper_height} tall",
        enderman.3 - enderman.1
    );

    // The eyes layers: a black eyes sheet adds nothing, so the frames under it are the bodies;
    // the same keys re-uploaded with a bright sheet move those frames by the added texels
    // alone, at either of the draw's own brightnesses — the layers' lightmap is the source's
    // constant (`LayerSpiderEyes.java`:35-38, `LayerEndermanEyes.java`:27-30).
    let draw_spider = || {
        mob_at_origin(
            ModelRef::Spider,
            "entity/spider/spider.png",
            DrawExtra::None,
        )
    };
    let draw_enderman = || {
        mob_at_origin(
            ModelRef::Enderman,
            "entity/enderman/enderman.png",
            DrawExtra::None,
        )
    };
    let spider_bright = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        draw_spider(),
    );
    let spider_dim = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        EntityDraw {
            light: 0.25,
            ..draw_spider()
        },
    );
    let enderman_body = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        draw_enderman(),
    );
    // What the eyes add: green and blue in a two-to-one ratio, nothing to red.
    const EYES: [u8; 4] = [0, 100, 200, 255];
    registry.set_named(&device, &queue, "entity/spider_eyes.png", &flat_sheet(EYES));
    registry.set_named(
        &device,
        &queue,
        "entity/enderman/enderman_eyes.png",
        &flat_sheet(EYES),
    );
    let spider_bright_eyes = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        draw_spider(),
    );
    let spider_dim_eyes = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        EntityDraw {
            light: 0.25,
            ..draw_spider()
        },
    );
    let enderman_eyes = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        draw_enderman(),
    );

    let mut spider_eyes_pixels = 0usize;
    let mut moved_with_the_light = 0usize;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let body = pixel(&spider_bright, x, y);
            let lit = pixel(&spider_bright_eyes, x, y);
            let (dr, dg, db) = (
                i32::from(lit[0]) - i32::from(body[0]),
                i32::from(lit[1]) - i32::from(body[1]),
                i32::from(lit[2]) - i32::from(body[2]),
            );
            if dg < 20 {
                continue;
            }
            spider_eyes_pixels += 1;
            assert!(
                dr.abs() <= 2 && db >= 2 * dg - 8,
                "the eyes add their texels onto the body, got ({dr}, {dg}, {db}) at ({x}, {y})"
            );
            let dim_body = pixel(&spider_dim, x, y);
            let dim_lit = pixel(&spider_dim_eyes, x, y);
            let (dim_green, dim_blue) = (
                i32::from(dim_lit[1]) - i32::from(dim_body[1]),
                i32::from(dim_lit[2]) - i32::from(dim_body[2]),
            );
            if (dim_green - dg).abs() > 2 || (dim_blue - db).abs() > 3 {
                moved_with_the_light += 1;
            }
        }
    }
    assert!(
        spider_eyes_pixels >= 40,
        "the eyes sheet adds over the spider, {spider_eyes_pixels} pixels"
    );
    assert!(
        moved_with_the_light <= (spider_eyes_pixels / 20).max(2),
        "the eyes' lightmap is constant: {moved_with_the_light} of {spider_eyes_pixels} pixels moved with the draw's brightness"
    );
    let mut enderman_eyes_pixels = 0usize;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let body = pixel(&enderman_body, x, y);
            let lit = pixel(&enderman_eyes, x, y);
            if i32::from(lit[1]) - i32::from(body[1]) >= 20 {
                enderman_eyes_pixels += 1;
            }
        }
    }
    assert!(
        enderman_eyes_pixels >= 20,
        "the enderman's eyes sheet adds over it, {enderman_eyes_pixels} pixels"
    );
}

/// The cube family: a slime's whole model scales with its size and its squash pair stretches
/// and pinches the silhouette (`RenderSlime.preRenderCallback`:32-37), and the gel layer
/// washes the body — the outer shell draws the sheet's own cells over the inner body's at
/// the layer's half alpha (`LayerSlimeGel.java`:19-32), so the read-back carries neither
/// colour alone. The magma cube draws its core and eight segments off its own sheet
/// (`ModelMagmaCube.setLivingAnimations`:42-56).
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_cube_family_scales_with_its_size() {
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);

    let mut registry = TextureRegistry::new(&device, &queue);
    registry.set_named(
        &device,
        &queue,
        "entity/slime/slime.png",
        &slime_shell_sheet(),
    );
    registry.set_named(
        &device,
        &queue,
        "entity/slime/magmacube.png",
        &flat_sheet([255, 120, 0, 255]),
    );
    // The shadow sprite's texel alpha is zero, so its fragment is discarded and the frames
    // below carry the models' own silhouettes: the squash's pinch is the model's, not the
    // ground sprite's.
    registry.set_named(
        &device,
        &queue,
        "misc/shadow.png",
        &flat_sheet([255, 255, 255, 0]),
    );
    let mut entities = EntityPass::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        registry.layout(),
    );
    entities.set_camera(entity_camera(), 1.0);

    let slime = |size: u8, squish: f32| {
        mob_at_origin(
            ModelRef::Slime { size },
            "entity/slime/slime.png",
            DrawExtra::Slime { size, squish },
        )
    };
    let one = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        slime(1, 0.0),
    );
    let four = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        slime(4, 0.0),
    );
    let squashed = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        slime(1, 1.0),
    );
    let magma = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        mob_at_origin(
            ModelRef::MagmaCube { size: 2 },
            "entity/slime/magmacube.png",
            DrawExtra::None,
        ),
    );

    // The size scales the whole model: the four-cube stands taller and covers more.
    let one_sil = silhouette(&one);
    let four_sil = silhouette(&four);
    assert!(
        four_sil.3 - four_sil.1 > (one_sil.3 - one_sil.1) + 8,
        "the four-cube stands taller than the one-cube: {} against {}",
        four_sil.3 - four_sil.1,
        one_sil.3 - one_sil.1
    );
    assert!(
        model_pixels(&four) > model_pixels(&one),
        "the four-cube covers more pixels: {} against {}",
        model_pixels(&four),
        model_pixels(&one)
    );
    // The squash pair at full: the one-cube stretches taller and pinches narrower.
    let squash_sil = silhouette(&squashed);
    assert!(
        squash_sil.3 - squash_sil.1 > (one_sil.3 - one_sil.1) + 3,
        "the squashed slime stands taller: {} against {}",
        squash_sil.3 - squash_sil.1,
        one_sil.3 - one_sil.1
    );
    assert!(
        squash_sil.2 - squash_sil.0 < one_sil.2 - one_sil.0,
        "the squashed slime pinches narrower: {} against {}",
        squash_sil.2 - squash_sil.0,
        one_sil.2 - one_sil.0
    );
    // The gel's wash: the shell's colour over the inner body's, neither alone
    // (`LayerSlimeGel.java`:19-32), at the class's own half alpha and each face's own shade.
    let shade = chest_shade();
    let shell = [200.0f32, 60.0, 40.0];
    let body = [60.0f32, 140.0, 220.0];
    let alpha = 128.0 / 255.0;
    let mut inner_only = [0u8; 3];
    let mut washed = [0u8; 3];
    for (channel, (inner_only, washed)) in inner_only.iter_mut().zip(washed.iter_mut()).enumerate()
    {
        let inner = (body[channel] * shade).round();
        *inner_only = inner as u8;
        *washed = (alpha * shell[channel] * shade + (1.0 - alpha) * inner).round() as u8;
    }
    let mut washed_pixels = 0usize;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let got = pixel(&one, x, y);
            if got
                .iter()
                .zip(washed)
                .all(|(&got, want)| (i16::from(got) - i16::from(want)).abs() <= 3)
            {
                washed_pixels += 1;
            }
        }
    }
    assert!(
        washed_pixels >= 8,
        "the gel washes the inner body's face, {washed_pixels} pixels carry the blend"
    );
    // The spot on the inner body's face reads the wash, not the body's colour alone: were
    // the layer skipped, its half of the sheet would not read at all.
    let spot = pixel(&one, SIZE / 2, SIZE / 2 + 18);
    expect_pixel(&one, SIZE / 2, SIZE / 2 + 18, washed, "the gel wash");
    assert!(
        spot.iter()
            .zip(inner_only)
            .any(|(&got, want)| (i16::from(got) - i16::from(want)).abs() > 10),
        "the gel's half of the sheet reads at the shell's face, got {spot:?}"
    );
    // The magma cube draws its core and its eight segments off its own sheet.
    let magma_sil = silhouette(&magma);
    assert!(
        magma_sil.2 - magma_sil.0 >= 10 && model_pixels(&magma) >= 150 && hued_pixels(&magma) >= 80,
        "the magma cube draws its core and segments: {} wide, {} pixels, {} hued",
        magma_sil.2 - magma_sil.0,
        model_pixels(&magma),
        hued_pixels(&magma)
    );
}

/// The arthropod family draws its own models: the chicken's head and stocky body with the
/// child drawing the renderer's own folded table (`ModelChicken.java`:20-44,
/// `ModelChicken.render`:54-72), the squid's body above its eight-tentacle fan
/// (`ModelSquid.java`:10-34), the bat — folded flat while it hangs where the flying one beats
/// its wings, with the hanging frame shifted down the tenth of a block the corpse rotation
/// adds (`ModelBat.setRotationAngles`:75, `RenderBat.rotateCorpse`:35-46) — and the
/// silverfish and the endermite as their own small crawling bodies
/// (`ModelSilverfish.setRotationAngles`:73, `ModelEnderMite.setRotationAngles`:49).
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_arthropod_family_draws_its_own_silhouettes() {
    const SKIN: [u8; 4] = [200, 90, 40, 255];
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);

    let registry = mob_registry(
        &device,
        &queue,
        &[
            ("entity/chicken.png", SKIN),
            ("entity/squid.png", SKIN),
            ("entity/bat.png", SKIN),
            ("entity/silverfish.png", SKIN),
            ("entity/endermite.png", SKIN),
        ],
    );
    let mut entities = EntityPass::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        registry.layout(),
    );
    entities.set_camera(entity_camera(), 1.0);

    let bat = |hanging: bool| EntityDraw {
        pose: Pose {
            extra: PoseExtra::Bat { hanging },
            ..Pose::default()
        },
        ..mob_at_origin(
            ModelRef::Bat { hanging },
            "entity/bat.png",
            DrawExtra::Bat { hanging },
        )
    };
    let chicken = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        mob_at_origin(
            ModelRef::Chicken { child: false },
            "entity/chicken.png",
            DrawExtra::Chicken { child: false },
        ),
    );
    let chick = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        mob_at_origin(
            ModelRef::Chicken { child: true },
            "entity/chicken.png",
            DrawExtra::Chicken { child: true },
        ),
    );
    let squid = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        mob_at_origin(ModelRef::Squid, "entity/squid.png", DrawExtra::None),
    );
    let flying = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        bat(false),
    );
    let hanging = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        bat(true),
    );
    let silverfish = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        mob_at_origin(
            ModelRef::Silverfish,
            "entity/silverfish.png",
            DrawExtra::None,
        ),
    );
    let endermite = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        mob_at_origin(ModelRef::EnderMite, "entity/endermite.png", DrawExtra::None),
    );

    for (name, frame, floor) in [
        ("the chicken", &chicken, 40usize),
        ("the chick", &chick, 30),
        ("the squid", &squid, 80),
        ("the flying bat", &flying, 8),
        ("the hanging bat", &hanging, 8),
        ("the silverfish", &silverfish, 12),
        ("the endermite", &endermite, 8),
    ] {
        let (min_x, min_y, max_x, max_y) = silhouette(frame);
        assert!(
            max_x - min_x >= 3 && max_y - min_y >= 3,
            "{name} stands in the frame, got ({min_x}, {min_y})..({max_x}, {max_y})"
        );
        assert!(
            model_pixels(frame) >= floor && hued_pixels(frame) >= floor / 3,
            "{name} draws its own sheet's hue: {} hued of {} pixels",
            hued_pixels(frame),
            model_pixels(frame)
        );
    }
    // The child folds the table: the same head over a halved body, so the frames differ
    // (`ModelChicken.render`:54-72).
    assert!(
        changed_pixels(&chicken, &chick) >= 15,
        "the child folds the table, {} pixels changed",
        changed_pixels(&chicken, &chick)
    );
    // The bat's fold: the hanging one's wings lie flat where the flying one beats them, and
    // the corpse shift rides with the flag (`RenderBat.rotateCorpse`:35-46).
    assert!(
        changed_pixels(&flying, &hanging) >= 20,
        "the hang flag folds the bat, {} pixels changed",
        changed_pixels(&flying, &hanging)
    );
    // Two small crawlers, two bodies.
    assert!(
        changed_pixels(&silverfish, &endermite) >= 4,
        "the silverfish and the endermite are different bodies, {} pixels",
        changed_pixels(&silverfish, &endermite)
    );
}

/// The exotic quadrupeds draw their own models: the horse's long body with the saddle
/// boxes and the marking and armour passes the class stacks after it
/// (`ModelHorse.render`:210-317, `RenderHorse.getEntityTexture`:51-78), the wolf with
/// the collar layer a tamed one draws (`LayerWolfCollar.java`:20-30), the ocelot on its
/// cat coats (`RenderOcelot.getEntityTexture`:23-40) and the rabbit on its fur table
/// (`RenderRabbit.getEntityTexture`:27-62). The layer gates read at pixels: the marking
/// and armour sheets draw only where the extras name them, the saddle boxes only while
/// the saddle does, and the collar's dye tint only on the tamed wolf.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_exotic_quadrupeds_draw_their_own_silhouettes() {
    const SKIN: [u8; 4] = [200, 90, 40, 255];
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);

    // The horse's and the wolf's base sheets are white and the layer sheets carry their
    // own colours, so a layer's pixels are unmistakable.
    let registry = mob_registry(
        &device,
        &queue,
        &[
            ("entity/horse/horse_white.png", [255, 255, 255, 255]),
            ("entity/horse/horse_markings_white.png", [30, 220, 60, 255]),
            (
                "entity/horse/armor/horse_armor_diamond.png",
                [60, 120, 255, 255],
            ),
            ("entity/wolf/wolf_tame.png", [255, 255, 255, 255]),
            ("entity/wolf/wolf.png", [255, 255, 255, 255]),
            ("entity/wolf/wolf_collar.png", [255, 255, 255, 255]),
            ("entity/wolf/wolf_angry.png", [30, 220, 60, 255]),
            ("entity/cat/ocelot.png", SKIN),
            ("entity/cat/red.png", [30, 220, 60, 255]),
            ("entity/rabbit/brown.png", SKIN),
        ],
    );
    let mut entities = EntityPass::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        registry.layout(),
    );
    entities.set_camera(entity_camera(), 1.0);

    let horse = |markings: u8, armour: u8, saddle: bool| EntityDraw {
        pose: Pose {
            extra: PoseExtra::Horse {
                saddle,
                chested: false,
                adult: true,
                variant: 0,
            },
            ..Pose::default()
        },
        ..mob_at_origin(
            ModelRef::Horse {
                variant: 0,
                colour: 0,
                markings,
                saddle,
                armour,
            },
            "entity/horse/horse_white.png",
            DrawExtra::Horse { markings, armour },
        )
    };
    let horse_marked = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        horse(1, 0, true),
    );
    let horse_armoured = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        horse(0, 3, true),
    );
    let horse_saddled = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        horse(0, 0, true),
    );
    let horse_bare = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        horse(0, 0, false),
    );

    // A marked horse: the body stands and the marking sheet reads over it.
    let (min_x, min_y, max_x, max_y) = silhouette(&horse_marked);
    assert!(
        max_x - min_x >= 10 && max_y - min_y >= 10 && model_pixels(&horse_marked) >= 150,
        "the horse stands, ({min_x}, {min_y})..({max_x}, {max_y}), {} pixels",
        model_pixels(&horse_marked)
    );
    assert!(
        green_pixels(&horse_marked) >= 8,
        "the marking sheet reads over the horse, {} pixels",
        green_pixels(&horse_marked)
    );
    // The shadow sprite sits at a quarter white over the sky ([182, 209, 251]) close
    // enough in blue to fool a loose test; the armour's own blue keeps its red low.
    let blue = |pixels: &[u8]| {
        (0..SIZE)
            .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let [r, g, b] = pixel(pixels, x, y);
                r < 150 && b as i32 > r as i32 + 40 && b as i32 > g as i32 + 40
            })
            .count()
    };
    assert!(
        blue(&horse_armoured) >= 6,
        "the armour sheet reads over the horse, {} pixels",
        blue(&horse_armoured)
    );
    // Each layer pass stands alone: the marking-only frame carries no armour colour and
    // the armour-only frame no marking colour.
    assert_eq!(blue(&horse_marked), 0, "no armour colour without its byte");
    assert_eq!(
        green_pixels(&horse_armoured),
        0,
        "no marking colour without its byte"
    );
    // A bare horse: no marking, no armour and no saddle boxes.
    assert_eq!(
        green_pixels(&horse_bare),
        0,
        "no marking layer without its byte"
    );
    assert_eq!(blue(&horse_bare), 0, "no armour layer without its byte");
    assert!(
        changed_pixels(&horse_saddled, &horse_bare) >= 4,
        "the saddle boxes draw: {} pixels",
        changed_pixels(&horse_saddled, &horse_bare)
    );
    assert!(
        changed_pixels(&horse_marked, &horse_bare) >= 12,
        "the marking pass and the saddle change the frame: {} pixels",
        changed_pixels(&horse_marked, &horse_bare)
    );
    assert!(
        changed_pixels(&horse_armoured, &horse_bare) >= 12,
        "the armour pass changes the frame: {} pixels",
        changed_pixels(&horse_armoured, &horse_bare)
    );

    // The wolf's collar: the tamed one wears the dye tint, the wild one wears nothing.
    let wolf = |tamed: bool, angry: bool, collar: u8| EntityDraw {
        pose: Pose {
            extra: PoseExtra::Wolf {
                tamed,
                angry,
                sitting: false,
                health: 20.0,
            },
            ..Pose::default()
        },
        ..mob_at_origin(
            ModelRef::Wolf {
                tamed,
                collar,
                angry,
            },
            if tamed {
                "entity/wolf/wolf_tame.png"
            } else if angry {
                "entity/wolf/wolf_angry.png"
            } else {
                "entity/wolf/wolf.png"
            },
            DrawExtra::Wolf { tamed, collar },
        )
    };
    let wolf_tamed = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        wolf(true, false, 14),
    );
    let wolf_wild = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        wolf(false, false, 14),
    );
    let wolf_angry = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        wolf(false, true, 14),
    );
    let red = |pixels: &[u8]| {
        (0..SIZE)
            .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let [r, g, _] = pixel(pixels, x, y);
                r as i32 > g as i32 + 20
            })
            .count()
    };
    assert!(
        red(&wolf_tamed) >= 6,
        "the tamed wolf's collar carries its dye tint, {} pixels",
        red(&wolf_tamed)
    );
    assert_eq!(red(&wolf_wild), 0, "the wild wolf carries no collar pixels");
    assert!(
        changed_pixels(&wolf_tamed, &wolf_wild) >= 8,
        "the collar layer changes the frame: {} pixels",
        changed_pixels(&wolf_tamed, &wolf_wild)
    );
    assert!(
        green_pixels(&wolf_angry) >= 6,
        "the angry wolf draws its own sheet, {} pixels",
        green_pixels(&wolf_angry)
    );

    // The two small cats: each stands on its own coat.
    let ocelot = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        mob_at_origin(
            ModelRef::Ocelot {
                variant: 2,
                child: false,
                tamed: false,
            },
            "entity/cat/ocelot.png",
            DrawExtra::None,
        ),
    );
    let ocelot_red = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        mob_at_origin(
            ModelRef::Ocelot {
                variant: 2,
                child: false,
                tamed: false,
            },
            "entity/cat/red.png",
            DrawExtra::None,
        ),
    );
    let rabbit = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        mob_at_origin(
            ModelRef::Rabbit {
                variant: 0,
                child: false,
            },
            "entity/rabbit/brown.png",
            DrawExtra::None,
        ),
    );
    for (name, frame, floor) in [
        ("the ocelot", &ocelot, 20usize),
        ("the rabbit", &rabbit, 12),
    ] {
        let (min_x, min_y, max_x, max_y) = silhouette(frame);
        assert!(
            max_x - min_x >= 3 && max_y - min_y >= 3,
            "{name} stands in the frame, got ({min_x}, {min_y})..({max_x}, {max_y})"
        );
        assert!(
            model_pixels(frame) >= floor && hued_pixels(frame) >= floor / 3,
            "{name} draws its own sheet's hue: {} hued of {} pixels",
            hued_pixels(frame),
            model_pixels(frame)
        );
    }
    assert!(
        model_pixels(&rabbit) < model_pixels(&horse_bare),
        "the rabbit is the smaller quadruped: {} against {}",
        model_pixels(&rabbit),
        model_pixels(&horse_bare)
    );
    assert!(
        green_pixels(&ocelot_red) >= 6 && changed_pixels(&ocelot, &ocelot_red) >= 8,
        "the red cat's coat changes the frame: {} green, {} changed",
        green_pixels(&ocelot_red),
        changed_pixels(&ocelot, &ocelot_red)
    );
}

/// The supernatural set draws its own bodies: the ghast's hanging tentacle fan at its
/// renderer's own four-and-a-half scale, the blaze's rod cage, and the guardian with its
/// tail and spikes — the elder drawing its own sheet, as the ghast's shooting state does
/// (`RenderGhast.getEntityTexture`:21-24, `RenderGuardian.getEntityTexture`:177-180).
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_supernatural_family_draws_its_own_silhouettes() {
    const SKIN: [u8; 4] = [200, 90, 40, 255];
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);

    let registry = mob_registry(
        &device,
        &queue,
        &[
            ("entity/ghast/ghast.png", SKIN),
            ("entity/ghast/ghast_shooting.png", [30, 220, 60, 255]),
            ("entity/blaze.png", SKIN),
            ("entity/guardian.png", SKIN),
            ("entity/guardian_elder.png", [30, 220, 60, 255]),
        ],
    );
    let mut entities = EntityPass::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        registry.layout(),
    );
    entities.set_camera(entity_camera(), 1.0);

    let ghast_rest = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        mob_at_origin(
            ModelRef::Ghast { shooting: false },
            "entity/ghast/ghast.png",
            DrawExtra::None,
        ),
    );
    let ghast_shooting = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        mob_at_origin(
            ModelRef::Ghast { shooting: true },
            "entity/ghast/ghast_shooting.png",
            DrawExtra::None,
        ),
    );
    let blaze = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        mob_at_origin(ModelRef::Blaze, "entity/blaze.png", DrawExtra::None),
    );
    let guardian = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        mob_at_origin(
            ModelRef::Guardian { elder: false },
            "entity/guardian.png",
            DrawExtra::None,
        ),
    );
    let guardian_elder = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        mob_at_origin(
            ModelRef::Guardian { elder: true },
            "entity/guardian_elder.png",
            DrawExtra::None,
        ),
    );

    for (name, frame, floor) in [
        ("the ghast", &ghast_rest, 300usize),
        ("the blaze", &blaze, 40),
        ("the guardian", &guardian, 40),
    ] {
        let (min_x, min_y, max_x, max_y) = silhouette(frame);
        assert!(
            max_x - min_x >= 10 && max_y - min_y >= 10,
            "{name} stands in the frame, got ({min_x}, {min_y})..({max_x}, {max_y})"
        );
        assert!(
            model_pixels(frame) >= floor && hued_pixels(frame) >= floor / 3,
            "{name} draws its own sheet's hue: {} hued of {} pixels",
            hued_pixels(frame),
            model_pixels(frame)
        );
    }
    // The ghast's fan spreads far wider than the blaze's cage.
    let ghast_width = {
        let (min_x, _, max_x, _) = silhouette(&ghast_rest);
        max_x - min_x
    };
    let blaze_width = {
        let (min_x, _, max_x, _) = silhouette(&blaze);
        max_x - min_x
    };
    assert!(
        ghast_width >= blaze_width + 8,
        "the ghast spreads wider than the blaze: {ghast_width} against {blaze_width}"
    );
    // The two sheet selections read: the shooting ghast carries its sheet's colour and
    // the elder guardian its own; neither frame equals its base-sheet sibling.
    assert!(
        green_pixels(&ghast_shooting) >= 8 && changed_pixels(&ghast_rest, &ghast_shooting) >= 10,
        "the shooting sheet reads: {} green, {} changed",
        green_pixels(&ghast_shooting),
        changed_pixels(&ghast_rest, &ghast_shooting)
    );
    assert!(
        green_pixels(&guardian_elder) >= 8 && changed_pixels(&guardian, &guardian_elder) >= 10,
        "the elder sheet reads: {} green, {} changed",
        green_pixels(&guardian_elder),
        changed_pixels(&guardian, &guardian_elder)
    );
    assert!(
        changed_pixels(&blaze, &guardian) >= 8,
        "the blaze and the guardian are different bodies: {} pixels",
        changed_pixels(&blaze, &guardian)
    );
}

/// The dragon's wing beat reads the flight clock the pose carries: at rest the wings sit
/// at one beat of the cycle, half a turn later at the other, and the two frames differ
/// over the wings and the striding legs (`ModelDragon.setRotationAngles`'s flight sine).
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_dragon_flaps_its_wings_at_two_phases() {
    const SKIN: [u8; 4] = [200, 90, 40, 255];
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);

    let registry = mob_registry(&device, &queue, &[("entity/enderdragon/dragon.png", SKIN)]);
    let mut entities = EntityPass::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        registry.layout(),
    );
    entities.set_camera(entity_camera(), 1.0);

    let dragon = |anim_time: f32| EntityDraw {
        pose: Pose {
            extra: PoseExtra::Dragon { anim_time },
            ..Pose::default()
        },
        ..mob_at_origin(
            ModelRef::EnderDragon,
            "entity/enderdragon/dragon.png",
            DrawExtra::None,
        )
    };
    let at_rest = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        dragon(0.0),
    );
    let at_half = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        dragon(0.5),
    );

    for (name, frame) in [
        ("the dragon at rest", &at_rest),
        ("the dragon at half", &at_half),
    ] {
        let (min_x, min_y, max_x, max_y) = silhouette(frame);
        assert!(
            max_x - min_x >= 20 && max_y - min_y >= 12,
            "{name} stands in the frame, got ({min_x}, {min_y})..({max_x}, {max_y})"
        );
        assert!(
            model_pixels(frame) >= 250 && hued_pixels(frame) >= 80,
            "{name} draws: {} hued of {} pixels",
            hued_pixels(frame),
            model_pixels(frame)
        );
    }
    assert!(
        changed_pixels(&at_rest, &at_half) >= 30,
        "the wing beat moves the frame: {} pixels",
        changed_pixels(&at_rest, &at_half)
    );
}

/// The wither draws on both its sheets: the base one while the spawn shield is down and
/// the invulnerable one while it runs — a draw-level sheet selection that also rides the
/// renderer's own scale ladder (`RenderWither.getEntityTexture`:33-37,
/// `RenderWither.preRenderCallback`:43-54).
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_wither_draws_its_second_sheet() {
    const SKIN: [u8; 4] = [200, 90, 40, 255];
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);

    let registry = mob_registry(
        &device,
        &queue,
        &[
            ("entity/wither/wither.png", SKIN),
            ("entity/wither/wither_invulnerable.png", [30, 220, 60, 255]),
        ],
    );
    let mut entities = EntityPass::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        registry.layout(),
    );
    entities.set_camera(entity_camera(), 1.0);

    let wither = |invul_time: u16, sheet: &'static str| {
        mob_at_origin(ModelRef::Wither { invul_time }, sheet, DrawExtra::None)
    };
    let shield_down = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        wither(0, "entity/wither/wither.png"),
    );
    let shield_up = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        wither(40, "entity/wither/wither_invulnerable.png"),
    );

    // The wither stands wide on either sheet — the three heads and the ribs — and the
    // invulnerable sheet's colour reads at its pixels.
    for (name, frame) in [
        ("the wither", &shield_down),
        ("the shielded wither", &shield_up),
    ] {
        let (min_x, min_y, max_x, max_y) = silhouette(frame);
        assert!(
            max_x - min_x >= 15 && max_y - min_y >= 10,
            "{name} stands in the frame, got ({min_x}, {min_y})..({max_x}, {max_y})"
        );
        assert!(
            model_pixels(frame) >= 120,
            "{name} draws: {} pixels",
            model_pixels(frame)
        );
    }
    assert!(
        hued_pixels(&shield_down) >= 40,
        "the base sheet's hue reads: {} pixels",
        hued_pixels(&shield_down)
    );
    assert!(
        green_pixels(&shield_up) >= 8,
        "the invulnerable sheet reads: {} pixels",
        green_pixels(&shield_up)
    );
    assert!(
        changed_pixels(&shield_down, &shield_up) >= 10,
        "the two sheets read differently: {} pixels",
        changed_pixels(&shield_down, &shield_up)
    );
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

// ---------------------------------------------------------------- the object set

use oxide_render::entity_models::Vertices;
use oxide_render::entity_pass::{FrameContent, ItemMesh, ItemMeshSource};

/// The object sheets the cases upload: every object draw's sheet is a flat light grey, so
/// the geometry — not the texture — carries the silhouette.
fn object_registry(device: &wgpu::Device, queue: &wgpu::Queue) -> TextureRegistry {
    let mut registry = TextureRegistry::new(device, queue);
    for key in [
        "entity/experience_orb.png",
        "entity/boat.png",
        "entity/minecart.png",
        "painting/paintings_kristoffer_zetterstrand.png",
        "items/test.png",
        "items/block.png",
    ] {
        let colour = if key == "items/block.png" {
            [120, 120, 120, 255]
        } else {
            [210, 210, 210, 255]
        };
        registry.set_named(device, queue, key, &flat_sheet(colour));
    }
    registry.set_named(
        device,
        queue,
        "misc/shadow.png",
        &flat_sheet([255, 255, 255, 128]),
    );
    registry
}

/// One quad over the whole sheet: `(min x, min y)` to `(max x, max y)` in 1/16 units at
/// depth `z`, normal `(0, 0, 1)`. The stub meshes below are all quads — the pass's object
/// paths carry the transform chains the cases measure, not any atlas mapping.
fn stub_quad(min: [f32; 2], max: [f32; 2], z: f32) -> Vertices {
    let mut mesh = Vertices::default();
    for corner in [
        [min[0], min[1]],
        [max[0], min[1]],
        [max[0], max[1]],
        [min[0], max[1]],
    ] {
        mesh.positions.push([corner[0], corner[1], z]);
        mesh.uvs.push([0.0, 0.0]);
        mesh.normals.push([0.0, 0.0, 1.0]);
    }
    mesh.uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    mesh
}

/// The object cases' mesh source: the meshes a client source would build, synthetic —
/// a generated sprite face, a block box face, the frame wood, the icon quad.
struct StubSource;

impl ItemMeshSource for StubSource {
    fn generated(&self, _key: &str) -> Option<ItemMesh> {
        Some(ItemMesh {
            vertices: std::sync::Arc::new(stub_quad([-8.0, -8.0], [8.0, 8.0], 0.0)),
            texture: "items/test.png",
        })
    }

    fn block_item(&self, _block: u16, _meta: u8) -> Option<ItemMesh> {
        Some(ItemMesh {
            vertices: std::sync::Arc::new(stub_quad([0.0, 0.0], [16.0, 16.0], 8.0)),
            texture: "items/block.png",
        })
    }

    fn frame_wood(&self) -> Option<ItemMesh> {
        Some(ItemMesh {
            vertices: std::sync::Arc::new(stub_quad([0.0, 0.0], [16.0, 16.0], 0.0)),
            texture: "items/test.png",
        })
    }

    fn icon_quad(&self, _key: &str) -> Option<ItemMesh> {
        Some(ItemMesh {
            vertices: std::sync::Arc::new(stub_quad([-8.0, -4.0], [8.0, 12.0], 0.0)),
            texture: "items/test.png",
        })
    }
}

/// One object draw at the origin, facing the entity camera, full brightness.
fn object_draw(model: ModelRef, extra: DrawExtra) -> EntityDraw {
    EntityDraw {
        model,
        position: [0.0; 3],
        body_yaw: 0.0,
        head_yaw: 0.0,
        head_pitch: 0.0,
        pose: Pose::default(),
        texture: TextureRef::Named("items/test.png"),
        light: 1.0,
        hurt: 0.0,
        death: 0.0,
        health: None,
        nametag: None,
        extra,
    }
}

/// The frame's non-sky pixel count: the object cases read silhouettes by how many pixels
/// an object lands over the empty sky.
fn non_sky(pixels: &[u8]) -> usize {
    (0..SIZE)
        .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
        .filter(|&(x, y)| pixel(pixels, x, y) != SKY)
        .count()
}

/// Sets an object scene up: the device, the target, the depth, the registry and a pass
/// with the stub source and the entity camera.
fn object_scene() -> (
    wgpu::Device,
    wgpu::Queue,
    Target,
    wgpu::TextureView,
    TextureRegistry,
    EntityPass,
) {
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);
    let registry = object_registry(&device, &queue);
    let mut entities = EntityPass::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        registry.layout(),
    );
    entities.set_camera(entity_camera(), 1.0);
    entities.set_item_source(std::sync::Arc::new(StubSource));
    (device, queue, target, depth, registry, entities)
}

/// Renders one object draw and returns the frame's pixels.
fn object_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &Target,
    depth: &wgpu::TextureView,
    entities: &mut EntityPass,
    registry: &TextureRegistry,
    draw: &EntityDraw,
) -> Vec<u8> {
    render_scene(device, queue, target, depth, |pass| {
        entities.draw(device, pass, std::slice::from_ref(draw), registry);
    })
}

/// The dropped block item draws the block's own box through the cache: the box's face
/// lands pixels above the sky.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_block_item_draws_the_baked_box() {
    let (device, queue, target, depth, registry, mut entities) = object_scene();
    let draw = object_draw(
        ModelRef::BlockItem { block: 1 },
        DrawExtra::Item {
            id: 1,
            count: 1,
            damage: 0,
        },
    );
    let filled = object_frame(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        &draw,
    );
    let count = non_sky(&filled);
    // The drawn box lands 32 px under the case's camera and scale; the floor keeps a
    // margin under the measured count.
    assert!(count > 24, "the block box lands pixels, got {count}");
}

/// The sprite item draws the generated-item shape: the item's face lands pixels.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_sprite_item_draws_the_generated_shape() {
    let (device, queue, target, depth, registry, mut entities) = object_scene();
    let draw = object_draw(
        ModelRef::Sprite {
            key: "items/test.png",
        },
        DrawExtra::Item {
            id: 280,
            count: 1,
            damage: 0,
        },
    );
    let filled = object_frame(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        &draw,
    );
    let count = non_sky(&filled);
    // The corrected flat net (0.5) halves the drawn face: it lands 70 px under the case's
    // camera — 231 px before the correction. The floor keeps a margin under the count.
    assert!(count > 30, "the generated face lands pixels, got {count}");
}

/// The orb draws its icon quad on the camera-facing billboard: pixels above the sky.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_orb_draws_its_icon_quad() {
    let (device, queue, target, depth, registry, mut entities) = object_scene();
    let draw = object_draw(ModelRef::Orb { value: 1 }, DrawExtra::None);
    let filled = object_frame(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        &draw,
    );
    let count = non_sky(&filled);
    assert!(count > 10, "the orb's quad lands pixels, got {count}");
}

/// The arrow draws its shaft: the source's six quads land a small silhouette.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_arrow_draws_its_shaft() {
    let (device, queue, target, depth, registry, mut entities) = object_scene();
    let draw = object_draw(ModelRef::Arrow, DrawExtra::None);
    let filled = object_frame(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        &draw,
    );
    let count = non_sky(&filled);
    assert!(count > 8, "the arrow lands pixels, got {count}");
}

/// The fireball's icon quad draws half the size of the snowball's generated item under
/// their registered scales: the two silhouettes are distinct in the same frame.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_throwable_billboards_draw_their_own_shapes() {
    let (device, queue, target, depth, registry, mut entities) = object_scene();
    let snowball = object_draw(
        ModelRef::Sprite {
            key: "items/test.png",
        },
        DrawExtra::Projectile {
            billboard: oxide_render::entity_models::objects::Billboard::Snowball,
            scale: 0.5,
        },
    );
    let small = object_frame(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        &snowball,
    );
    let fireball = object_draw(
        ModelRef::Sprite {
            key: "items/test.png",
        },
        DrawExtra::Projectile {
            billboard: oxide_render::entity_models::objects::Billboard::Fireball,
            scale: 2.0,
        },
    );
    let large = object_frame(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        &fireball,
    );
    let (small_count, large_count) = (non_sky(&small), non_sky(&large));
    assert!(
        small_count > 0,
        "the snowball lands pixels, got {small_count}"
    );
    assert!(
        large_count > small_count,
        "the large fireball's quad is the bigger one: {large_count} against {small_count}"
    );
}

/// The painting draws its art quad, then the frame and back its source gives the art:
/// the visible art's silhouette is the art's own aspect — a square for the first art.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_painting_draws_its_art_quad() {
    let (device, queue, target, depth, registry, mut entities) = object_scene();
    let draw = object_draw(
        ModelRef::Painting { art: 0 },
        DrawExtra::Painting { facing: 0 },
    );
    let filled = object_frame(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        &draw,
    );
    let (min_x, min_y, max_x, max_y) = silhouette(&filled);
    let (width, height) = (max_x - min_x + 1, max_y - min_y + 1);
    assert!(
        width >= 12 && height >= 12,
        "the first art is 16 by 16 pixels, its quad lands ({min_x}, {min_y})..({max_x}, {max_y})"
    );
    assert!(
        width.abs_diff(height) <= 6,
        "a square art's silhouette, got {width} by {height}"
    );
}

/// The frame draws its wood alone when empty; a block content adds the nested box's
/// pixels inside it.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_frame_draws_its_content_over_the_wood() {
    let (device, queue, target, depth, registry, mut entities) = object_scene();
    let mut empty = object_draw(
        ModelRef::ItemFrame {
            content: FrameContent::Empty,
        },
        DrawExtra::Frame { rotation: 0 },
    );
    // The frame hangs facing the entity camera: `RenderItemFrame` turns the model by
    // `180 - rotationYaw`, so the half turn points the frame's own front at the viewer.
    empty.body_yaw = 180.0;
    let bare = object_frame(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        &empty,
    );
    let bare_count = non_sky(&bare);
    assert!(
        bare_count > 30,
        "the frame's wood lands pixels, got {bare_count}"
    );

    // The nested block's face re-draws the wood at the frame's centre: its own sheet over
    // the wood's, so the centre pixels change between the two frames.
    let mut filled = object_draw(
        ModelRef::ItemFrame {
            content: FrameContent::Block(1),
        },
        DrawExtra::Frame { rotation: 0 },
    );
    filled.body_yaw = 180.0;
    let with_content = object_frame(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        &filled,
    );
    let changed = changed_pixels(&bare, &with_content);
    assert!(
        changed > 0,
        "the nested block's face re-draws the frame's centre, {changed} pixels changed"
    );
}

/// The boat draws its multi-box hull: a wide low silhouette.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_boat_draws_its_hull() {
    let (device, queue, target, depth, registry, mut entities) = object_scene();
    let draw = object_draw(ModelRef::Boat, DrawExtra::None);
    let filled = object_frame(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        &draw,
    );
    let count = non_sky(&filled);
    assert!(count > 100, "the hull's boxes land pixels, got {count}");
}

/// The minecart's bodies: the plain cart draws its hopper box; the chest cart draws its
/// cargo in addition, so it lands the more pixels of the two.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_minecart_draws_its_body_and_cargo() {
    let (device, queue, target, depth, registry, mut entities) = object_scene();
    let plain = object_draw(
        ModelRef::Minecart {
            body: oxide_render::entity_models::objects::MinecartBody::Plain as u8,
        },
        DrawExtra::None,
    );
    let bare = object_frame(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        &plain,
    );
    let bare_count = non_sky(&bare);
    assert!(
        bare_count > 50,
        "the cart's boxes land pixels, got {bare_count}"
    );
    let chest = object_draw(
        ModelRef::Minecart {
            body: oxide_render::entity_models::objects::MinecartBody::Chest as u8,
        },
        DrawExtra::None,
    );
    let cargo = object_frame(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        &chest,
    );
    let cargo_count = non_sky(&cargo);
    assert!(
        cargo_count > bare_count,
        "the chest cargo adds pixels: {cargo_count} against {bare_count}"
    );
}
