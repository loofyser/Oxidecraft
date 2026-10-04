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
