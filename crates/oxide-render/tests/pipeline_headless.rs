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
use std::sync::Arc;
use std::sync::mpsc;
use std::task::{Context, Poll, Wake, Waker};

use oxide_assets::atlas::{Atlas, AtlasLevel, AtlasSprite, SpriteRect};
use oxide_assets::font::Font;
use oxide_assets::model::Transform;
use oxide_assets::texture::Texture;
use oxide_render::camera::{
    Camera, CameraPose, DEFAULT_FOV, EYE_HEIGHT, FIRST_PERSON_OFFSET, NEAR_PLANE, NO_VIEW_EFFECT,
};
use oxide_render::entity_models::{Pose, PoseExtra, objects};
use oxide_render::entity_pass::{
    BOSS_STATUS_TIME, BossStatus, DrawExtra, EntityDraw, EntityPass, EquipmentDraw, ModelRef,
    NametagDraw, SkinLookup, TextureRef, TextureRegistry,
};
use oxide_render::fog::{FogMode, FogParams, fog_colour, water_fog};
use oxide_render::gui_item::{ATLAS_TEXTURE, IconShape, ItemIcon, ItemIconMesh, ItemIconSource};
use oxide_render::held_item::{HeldItemFrame, HeldItemPass};
use oxide_render::hud::{HudDraw, HudPass, HudTexture, ScaledResolution, scaled_resolution};
use oxide_render::lightmap::{BrightnessTable, lightmap_image, sample_index};
use oxide_render::overlay::OverlayPass;
use oxide_render::renderer::{SKY_COLOR, boss_status_step};
use oxide_render::sky::{
    CloudPass, HORIZON, MOON_HEIGHT, SkyParams, SkyPass, SkyTextures, celestial_rotation,
    star_field,
};
use oxide_render::terrain::{ChunkMesh, Layer, Vertex};
use oxide_render::terrain_pass::{DEPTH_FORMAT, TerrainPass};
use oxide_render::text::string_width;
use oxide_render::world_overlay::{Crack, FULL_CUBE, Outline, WorldOverlay};

use glam::Vec3;

/// The size of the offscreen target in texels.
const SIZE: u32 = 64;
/// The readback row stride: a row must be a multiple of 256 bytes, and 64 RGBA texels are
/// exactly that.
const BYTES_PER_ROW: u32 = 256;
/// The sky colour as 8-bit unorm bytes: 0.62, 0.76 and 0.98 of 255, rounded.
const SKY: [u8; 3] = [158, 194, 250];
/// The icon case's checkerboard texels: the stand-in sprite's own two colours at level 0.
const CHECKER_A: [u8; 4] = [254, 0, 0, 255];
const CHECKER_B: [u8; 4] = [0, 0, 254, 255];
/// The two colours' average: every texel of the stand-in's reduced level, and the value a
/// minified sample blends in.
const CHECKER_AVERAGE: [u8; 4] = [127, 0, 127, 255];
/// The stone block's top face as 8-bit unorm bytes: 0.50 grey of 255, rounded.
const STONE: [u8; 3] = [128, 128, 128];
/// The buried face's colour, which must never reach the target.
const BURIED: [u8; 3] = [255, 0, 0];
/// The overlay text colour: opaque white, the sheet's ink texel.
const TEXT: [u8; 3] = [255, 255, 255];
/// The overlay shadow colour as 8-bit unorm bytes: the source's `(0xFFFFFFFF & 0x00FCFCFC) >> 2
/// | 0xFF000000` is 0xFF3F3F3F, 63 per channel.
const SHADOW: [u8; 3] = [63, 63, 63];
/// The boss bar's background band colour as 8-bit unorm bytes.
const BAR_BACK: [u8; 3] = [0, 0, 255];
/// The boss bar's fill band colour as 8-bit unorm bytes.
const BAR_FILL: [u8; 3] = [255, 255, 0];
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
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| {
        overlay.draw(pass)
    });
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

/// The overlay pass discards the sheet's below-threshold texels: the source's alpha test
/// is kept as a discard (`GL_GREATER` 0.1, `EntityRenderer.java`:1168), so a fragment
/// whose modulated alpha is at or below the threshold draws nothing — the sheet's faint
/// texels and its transparent margins leave the frame alone, where a blended pass would
/// faintly tint the frame and an unblended pass without the test would paint it.
///
/// The fixture: the `|` cell's first column at full coverage, its second at 20/255
/// (below the 0.1 threshold) and its third at 78/255 (above it). The scale is two and
/// the margin four, so the cell's columns ink physical x 4-5, x 6-7 and x 8-9; the
/// shadow copies sit two pixels down and right, below y 6, so the assertions stay
/// clear of them.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_overlay_pass_discards_the_below_threshold_texels() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);

    let mut overlay = OverlayPass::new(&device, format);
    overlay.set_size(&queue, SIZE as f32, SIZE as f32);
    overlay
        .set_font(&device, &queue, &faint_font_sheet())
        .expect("the synthetic sheet is a 16x16 grid");
    overlay.upload_text(&device, &queue, &["|".to_string()]);

    let depth = create_depth(&device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide pipeline headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| {
        overlay.draw(pass)
    });
    queue.submit(Some(encoder.finish()));

    let pixels = read_pixels(&device, &queue, &target);
    // The full-coverage column writes straight through: exact white.
    expect_pixel_exact(&pixels, 4, 4, TEXT, "the full-coverage column");
    expect_pixel_exact(&pixels, 5, 19, TEXT, "its last row");
    // The below-threshold column draws nothing: the sky stays.
    expect_pixel_exact(&pixels, 6, 4, SKY, "the below-threshold column");
    expect_pixel_exact(&pixels, 7, 5, SKY, "its second row");
    // The above-threshold column draws (its 78/255 is over the 0.1 test): exact white.
    expect_pixel_exact(&pixels, 8, 4, TEXT, "the above-threshold column");
    expect_pixel_exact(&pixels, 9, 5, TEXT, "its second row");
    // The cell's transparent remainder and the frame around it stay sky too.
    expect_pixel_exact(&pixels, 10, 4, SKY, "the transparent remainder");
    expect_pixel_exact(&pixels, 20, 4, SKY, "right of the cell");
    expect_pixel_exact(&pixels, 0, 0, SKY, "the corner above the text");
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

/// The synthetic sheet for the discard case: the `|` cell's first column at full
/// coverage, its second at 20/255 (below the alpha test's 0.1 threshold) and its
/// third at 78/255 (above it), so the threshold is pinned from both sides.
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn faint_font_sheet() -> Texture {
    const SIDE: u32 = 128;
    const CELL: u32 = 8;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    // '|' is code 124: column 12, row 7 of the grid.
    let code = '|' as u32;
    let cell_x = (code % 16) * CELL;
    let cell_y = (code / 16) * CELL;
    for row in 0..CELL {
        let at = (((cell_y + row) * SIDE + cell_x) * 4) as usize;
        rgba[at..at + 4].copy_from_slice(&[255, 255, 255, 255]);
        rgba[at + 4..at + 8].copy_from_slice(&[255, 255, 255, 20]);
        rgba[at + 8..at + 12].copy_from_slice(&[255, 255, 255, 78]);
    }
    Texture {
        width: SIDE,
        height: SIDE,
        rgba,
    }
}

/// The synthetic icon sheet: `gui/icons` in miniature — an opaque green band where the
/// below-150 ms latency's window lands (`GuiPlayerTabOverlay.drawPing`:248-250 sets the
/// level, `:270` draws at `(0, 176 + j * 8)`), a red one where the no-signal window
/// (`GuiPlayerTabOverlay.drawPing`:244-246's level 5) does, and the boss bar's two windows
/// (`GuiIngame`:913-918):
/// blue across the background slice's `(0, 74, 182, 5)` and yellow across the fill's
/// `(0, 79, 183, 5)` — over a transparent sheet.
///
/// Generated here; no asset store is read and no sheet pixel is copied.
fn icon_sheet() -> Texture {
    const SIDE: u32 = 256;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    for (v, rows, width, colour) in [
        (176u32, 8u32, 10u32, [0u8, 255, 0, 255]),
        (216, 8, 10, [255, 0, 0, 255]),
        (74, 5, 182, [0, 0, 255, 255]),
        (79, 5, 183, [255, 255, 0, 255]),
    ] {
        for y in v..v + rows {
            for x in 0..width {
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
        mode: FogMode::Linear,
        density: 0.0,
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
        mode: FogMode::Linear,
        density: 0.0,
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

#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_frames_water_fog_falls_off_exponentially() {
    // The submerged eye's branch (`EntityRenderer.java:1985-1995` with `:1845-1847`):
    // EXP at density 0.1 over (0.02, 0.02, 0.2), so a fragment's factor is
    // `exp(-(0.1 * distance)^2)` with the eye's radial distance — the linear
    // start and end below are degenerate on purpose, pinning that the EXP arm
    // reads neither of them. At four blocks the factor is `exp(-0.16)` and the
    // lit `(252, 252, 252)` surface reads `(215, 215, 222)`; at eight it is
    // `exp(-0.64)` for `(135, 135, 157)`. A linear frame with this degenerate
    // range would draw both unfogged.
    let water = water_fog(0, false);
    assert_eq!(water.mode, FogMode::Exp);
    let fog = FogParams {
        colour: water.colour,
        start: 0.0,
        end: 0.0,
        far_plane: 8.0,
        mode: water.mode,
        density: water.density,
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

    assert_eq!(lit, [252, 252, 252, 255], "the unfogged surface");
    expect_pixel(
        &frame(4.0),
        SIZE / 2,
        SIZE / 2,
        [215, 215, 222],
        "the surface four blocks out under EXP water fog",
    );
    expect_pixel(
        &frame(8.0),
        SIZE / 2,
        SIZE / 2,
        [135, 135, 157],
        "the surface eight blocks out under EXP water fog",
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
    // the grid's origin, lifted to `-(eyeY - HORIZON)` `RenderGlobal.java`:1412. Its straight
    // line from the eye the view is built with is the fog's measure.
    let eye = Vec3::new(0.0, EYE_HEIGHT, 0.0) - FIRST_PERSON_OFFSET * camera.forward();
    let plane_y = -((f64::from(high) + f64::from(EYE_HEIGHT)) - f64::from(HORIZON));
    let distance = (Vec3::new(0.0, plane_y as f32, 0.0) - eye).length();
    let factor = ((lateral.far_plane - distance) / lateral.far_plane).clamp(0.0, 1.0);
    let mixed = std::array::from_fn(|channel| {
        lateral.fog_colour[channel] + (below_colour[channel] - lateral.fog_colour[channel]) * factor
    });
    // The corner sits straight under the eye, so it projects onto the frame's centre column;
    // the pitch puts it 30.25 degrees below the view axis, which is
    // `tan(30.25 deg) / tan(35 deg) = 0.833` of the half-height below the centre, i.e. row 58.
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
            sneak: false,
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
            sneak: false,
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

/// Runs the overlay pass over the target: the colour the terrain pass left, loaded, and the
/// depth buffer cleared and attached, the state the hud's item pipelines state their depth
/// against.
fn with_overlay_pass(
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    depth: &wgpu::TextureView,
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
            sneak: false,
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
            sneak: false,
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
        id: 0,
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
        name: None,
        below_name: None,
        equipment: [None; 5],
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
        below_name: None,
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
            sneak: false,
        },
        fov_degrees: 2.0,
        near: NEAR_PLANE,
        far_chunks: 8.0,
        view_effect: NO_VIEW_EFFECT,
    }
}

/// The below-name case's camera: eight blocks out — inside the line's own squared gate —
/// with the field of view widened so a block lands about a hundred pixels and the
/// source's raise is about twenty-eight. The pose aims at the middle of the two-label
/// stack: the plain box's bottom at 2.0867 to the raised box's top at 2.6027, the raise
/// 0.276 apart at the anchors.
fn below_camera() -> Camera {
    const AIM: f64 = 2.344_7;
    Camera {
        pose: CameraPose {
            position: [0.0, 1.06 - f64::from(EYE_HEIGHT), 8.0],
            yaw: 180.0,
            pitch: -((AIM - 1.06) / 8.0).atan().to_degrees() as f32,
            sneak: false,
        },
        fov_degrees: 4.6,
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

/// The tag's text keeps the font's own handedness: the source's chain flips both font axes
/// under the negated camera yaw (`Render.java`:344-346), so at a camera looking along -z the
/// synthetic `A`'s ink — its cell's left five columns (`nametag_sheet`) — lands left of the
/// box's centre. A mirroring half turn would land it right.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_nametag_ink_lands_left_of_the_box_centre() {
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
    entities.set_camera(tag_camera(8.0), 1.0);
    entities
        .set_font(&device, &queue, &nametag_sheet())
        .expect("the synthetic sheet loads");

    let untagged = render_scene(&device, &queue, &target, &depth, |pass| {
        let draw = player_at_origin(TextureRef::Named("entity/test.png"));
        entities.draw(&device, pass, std::slice::from_ref(&draw), &registry);
    });
    let tagged = render_scene(&device, &queue, &target, &depth, |pass| {
        entities.draw(&device, pass, std::slice::from_ref(&tagged("A")), &registry);
    });

    // The box: the columns the tag changed. The ink: the near-white columns of the solid
    // text pass inside it.
    let (mut box_min, mut box_max) = (SIZE, 0u32);
    let (mut ink_min, mut ink_max) = (SIZE, 0u32);
    for y in 0..SIZE {
        for x in 0..SIZE {
            if pixel(&untagged, x, y) != pixel(&tagged, x, y) {
                box_min = box_min.min(x);
                box_max = box_max.max(x);
            }
            if pixel(&tagged, x, y).iter().all(|channel| *channel >= 250) {
                ink_min = ink_min.min(x);
                ink_max = ink_max.max(x);
            }
        }
    }
    assert!(box_max >= box_min, "the tag drew its box");
    assert!(ink_max >= ink_min, "the tag drew its glyph");
    let box_centre = (box_min + box_max) as f32 / 2.0;
    let ink_centre = (ink_min + ink_max) as f32 / 2.0;
    eprintln!(
        "ink side: box x=[{box_min}..{box_max}] centre {box_centre}, \
         ink x=[{ink_min}..{ink_max}] centre {ink_centre}"
    );
    // Measured at the pinning run: ink x=[14..43], centre 28.5, three pixels left of the
    // box's [8..55], centre 31.5. Mirrored, the ink landed [20..49], centre 34.5.
    assert!(
        box_centre - ink_centre >= 2.0,
        "the glyph's ink sits left of the box's centre: box x=[{box_min}..{box_max}] \
         centre {box_centre}, ink x=[{ink_min}..{ink_max}] centre {ink_centre}"
    );
}

/// The below-name line draws under the tag and raises it: a player draw carrying the
/// composed line shows the line's ink in the band below the nametag, and the tag's ink a
/// raise higher than the same draw without the line — the source's override path
/// (`RenderPlayer.java`:139-155): the line draws first at the plain anchor (`:149`) and
/// the nametag above it takes the source's own product `FONT_HEIGHT * 1.15F * 0.02666667F`
/// raise (`:150`, the f32 0.2760000228881836 the unit pin holds bit for bit).
///
/// The probe: the camera sits eight blocks out — inside the line's own `d0 < 100.0D`
/// gate (`:141`) — with the field of view widened so a block is about a hundred pixels;
/// the raise then lands about twenty-eight pixels, and the frame fits the whole two-label
/// stack (the plain box's bottom to the raised box's top, 0.516 blocks). The probe's
/// texts are stand-ins: the nametag is the synthetic sheet's `A` and the line `AA` — two
/// glyphs the tag's single glyph does not cover, so the line's ink reaches columns the
/// tag leaves clear, which is where the pair's difference is read. The model itself falls
/// below the frame's bottom edge.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_below_name_draws_under_the_tag_and_raises_it() {
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
    entities.set_camera(below_camera(), 1.0);
    entities
        .set_font(&device, &queue, &nametag_sheet())
        .expect("the synthetic sheet loads");

    let control = tagged("A");
    let mut with_below = tagged("A");
    with_below.below_name = Some("AA".to_owned());

    let control_frame = render_scene(&device, &queue, &target, &depth, |pass| {
        entities.draw(&device, pass, std::slice::from_ref(&control), &registry);
    });
    let below_frame = render_scene(&device, &queue, &target, &depth, |pass| {
        entities.draw(&device, pass, std::slice::from_ref(&with_below), &registry);
    });

    // (1) The line's ink sits in the band under the tag — and the control, which carries
    // no line, holds the sky at the same pixel.
    expect_pixel(
        &below_frame,
        18,
        45,
        TEXT,
        "the below line's ink under the tag",
    );
    expect_pixel(&control_frame, 18, 45, SKY, "the control's same pixel");
    // (2) The tag's ink shifts up by the raise: the topmost near-white row of each frame.
    let ink_top = |pixels: &[u8]| -> u32 {
        (0..SIZE)
            .find(|&y| (0..SIZE).any(|x| pixel(pixels, x, y).iter().all(|channel| *channel >= 250)))
            .expect("an inked row")
    };
    let raised_top = ink_top(&below_frame);
    let plain_top = ink_top(&control_frame);
    let shift = plain_top - raised_top;
    let changed = changed_pixels(&control_frame, &below_frame);
    eprintln!(
        "below name: tag ink top {raised_top}, control ink top {plain_top}, shift {shift} px, \
         {changed} changed pixels"
    );
    // Measured at the pinning run: tag ink top 10, control ink top 36, shift 26 px, 1058
    // changed pixels.
    assert!(
        (26..=29).contains(&shift),
        "the tag's ink sits the raise above the control's: {plain_top} -> {raised_top}"
    );
    assert!(
        changed >= 100,
        "the line's box and glyph cover pixels, got {changed}"
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
        id: 0,
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
        name: None,
        below_name: None,
        equipment: [None; 5],
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
                DrawExtra::Creeper { powered: false },
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

// ---------------------------------------------------------------- the equipment layers

/// The item fixtures' atlas as a registry texture: its level 0, under the meshes' own
/// [`ATLAS_TEXTURE`] key, so the entity pass resolves the held item's quad the way the
/// client's registry does.
fn atlas_sheet(atlas: &Atlas) -> Texture {
    let level = &atlas.levels[0];
    Texture {
        width: level.width,
        height: level.height,
        rgba: level.rgba.clone(),
    }
}

/// The equipment cases' registry: the zombie's and the creeper's sheets in the test hue,
/// the leather tier sheet white so the dye tint reads alone, the leather overlay fully
/// transparent (the cutout drops it, as the real overlay's spare texels are), the iron
/// sheet a mid grey, the creeper's aura sheet white, the glint sheet white, and the item
/// fixtures' atlas under its own key.
fn equipment_registry(device: &wgpu::Device, queue: &wgpu::Queue) -> TextureRegistry {
    let mut registry = mob_registry(
        device,
        queue,
        &[
            ("entity/zombie/zombie.png", [200, 90, 40, 255]),
            ("entity/creeper/creeper.png", [200, 90, 40, 255]),
            ("models/armor/leather_layer_1.png", [255, 255, 255, 255]),
            (
                "models/armor/leather_layer_1_overlay.png",
                [255, 255, 255, 0],
            ),
            ("models/armor/iron_layer_1.png", [120, 120, 120, 255]),
            ("entity/creeper/creeper_armor.png", [255, 255, 255, 255]),
            ("misc/enchanted_item_glint.png", [255, 255, 255, 255]),
        ],
    );
    registry.set_named(device, queue, ATLAS_TEXTURE, &atlas_sheet(&item_atlas()));
    registry
}

/// One zombie at the origin wearing `equipment`.
fn zombie_wearing(equipment: [Option<EquipmentDraw>; 5]) -> EntityDraw {
    let mut draw = mob_at_origin(
        ModelRef::Zombie,
        "entity/zombie/zombie.png",
        DrawExtra::None,
    );
    draw.equipment = equipment;
    draw
}

/// The equipment pass: the fixture icon source set, the camera driven.
fn equipment_pass(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    registry: &TextureRegistry,
) -> EntityPass {
    let mut entities = EntityPass::new(
        device,
        queue,
        wgpu::TextureFormat::Rgba8Unorm,
        registry.layout(),
    );
    entities.set_camera(entity_camera(), 1.0);
    entities.set_icon_source(Arc::new(TestIcons {
        atlas: item_atlas(),
    }));
    entities
}

/// The bounding box of the pixels carrying the item fixture's green.
fn green_silhouette(pixels: &[u8]) -> Option<(u32, u32, u32, u32)> {
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let [r, g, b] = pixel(pixels, x, y);
            if g as i32 > r as i32 + 40 && g as i32 > b as i32 + 40 {
                bounds = Some(match bounds {
                    None => (x, y, x, y),
                    Some((min_x, min_y, max_x, max_y)) => {
                        (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
                    }
                });
            }
        }
    }
    bounds
}

/// The count of pixels that read as a grey: the channels within six of each other and
/// clear of the shadow's faint wash.
fn grey_pixels(pixels: &[u8]) -> usize {
    (0..SIZE)
        .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let [r, g, b] = pixel(pixels, x, y);
            let [r, g, b] = [r as i32, g as i32, b as i32];
            (r - g).abs() <= 6 && (g - b).abs() <= 6 && r > 40
        })
        .count()
}

/// The pixel whose channels moved most between the two frames, with both frames' readings
/// there.
fn biggest_change(before: &[u8], after: &[u8]) -> (u32, u32, [u8; 3], [u8; 3]) {
    let mut best = (0, 0, [0; 3], [0; 3], -1i32);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let old = pixel(before, x, y);
            let new = pixel(after, x, y);
            let moved: i32 = old
                .iter()
                .zip(new.iter())
                .map(|(a, b)| (*a as i32 - *b as i32).abs())
                .sum();
            if moved > best.4 {
                best = (x, y, old, new, moved);
            }
        }
    }
    (best.0, best.1, best.2, best.3)
}

/// The held item draws on the zombie's arm: the sword fixture's quad, mounted through the
/// layer's chain at its third-person display transform, stands beside the body — its green
/// sprite's pixels land on the side the sword's own `display.thirdperson` turn puts them
/// (`LayerHeldItem.java`:41-42, :66; the item model's rotation) — and the empty hand draws
/// none of them.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_equipment_held_sword_draws_on_the_zombies_arm() {
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);
    let registry = equipment_registry(&device, &queue);
    let mut entities = equipment_pass(&device, &queue, &registry);

    let bare = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        zombie_wearing([None; 5]),
    );
    let mut held = [None; 5];
    held[0] = Some(EquipmentDraw {
        id: 4,
        damage: 0,
        enchanted: false,
        colour: None,
        cross: false,
    });
    let armed = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        zombie_wearing(held),
    );

    assert_eq!(
        green_pixels(&bare),
        0,
        "the empty hand draws no item pixels"
    );
    let item = green_pixels(&armed);
    assert!(item >= 8, "the sword draws: {item} item pixels");
    // The orientation pin: the quad's silhouette, measured on the fixed frame (x 24..=26,
    // y 4..=28 at 22 green pixels), is a narrow strip beside the body's screen-left edge —
    // the zombie's right arm, mirrored by the `180 - body_yaw` turn — reaching above the
    // head's top (the head's own rows start near 14) down past the chest. The x pin
    // catches a mirrored mount (the strip would land near 37..40); the y pin catches a
    // collapsed or sideways display transform.
    let (min_x, min_y, max_x, max_y) =
        green_silhouette(&armed).expect("the item's pixels stand in the frame");
    assert!(
        min_x >= 20 && max_x <= 30,
        "the sword hangs beside the body, not across it: x span {min_x}..={max_x}"
    );
    assert!(
        min_y <= 8 && (20..=34).contains(&max_y),
        "the blade reaches from above the head down past the chest: y span {min_y}..={max_y}"
    );
}

/// The leather armour takes the stack's dye: the same chestplate with no `display.color`
/// reads the default brown, and the red dye turns its texels red — the tint multiplies the
/// white sheet, so the dyed pixels' green and blue collapse (`ItemArmor.getColor`:135-157
/// through the armour layer's tint term).
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_equipment_leather_armour_takes_the_dye() {
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);
    let registry = equipment_registry(&device, &queue);
    let mut entities = equipment_pass(&device, &queue, &registry);

    let chest = |colour: Option<i32>| {
        let mut equipment = [None; 5];
        equipment[3] = Some(EquipmentDraw {
            id: 299,
            damage: 0,
            enchanted: false,
            colour,
            cross: false,
        });
        zombie_wearing(equipment)
    };
    let plain = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        chest(None),
    );
    let dyed = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        chest(Some(0xFF0000)),
    );

    // The dyed armour's pixels: pure red under the shading — no green or blue survives the
    // tint, which the zombie's own hue (its green high) and the sky never match.
    let red = |pixels: &[u8]| {
        (0..SIZE)
            .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let [r, g, b] = pixel(pixels, x, y);
                r as i32 > 80 && (g as i32) < 24 && (b as i32) < 24
            })
            .count()
    };
    let red_pixels = red(&dyed);
    assert_eq!(
        red(&plain),
        0,
        "the default brown keeps its green: no pure-red pixels"
    );
    assert!(
        red_pixels >= 40,
        "the red dye reads on the armour: {red_pixels} pixels"
    );
    assert!(
        changed_pixels(&plain, &dyed) >= 40,
        "the dye redraws the armour: {} pixels changed",
        changed_pixels(&plain, &dyed)
    );
    let (x, y, before, after) = biggest_change(&plain, &dyed);
    assert!(
        after[0] as i32 > before[0] as i32 && (after[1] as i32) < before[1] as i32,
        "the dye raises red and drops green at ({x}, {y}): {before:?} -> {after:?}"
    );
}

/// The iron tier sheet draws its own grey: the chestplate's texels come from the iron
/// sheet, not the zombie's hue — the frame's grey count climbs by the armour's coverage,
/// and the change's centre pixel reads grey.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_equipment_iron_armour_draws_its_sheet() {
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);
    let registry = equipment_registry(&device, &queue);
    let mut entities = equipment_pass(&device, &queue, &registry);

    let bare = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        zombie_wearing([None; 5]),
    );
    let mut equipment = [None; 5];
    equipment[3] = Some(EquipmentDraw {
        id: 307,
        damage: 0,
        enchanted: false,
        colour: None,
        cross: false,
    });
    let armoured = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        zombie_wearing(equipment),
    );

    let greys = grey_pixels(&armoured) as i64 - grey_pixels(&bare) as i64;
    assert!(
        greys >= 40,
        "the iron sheet's grey covers the torso: {greys} pixels of grey added"
    );
    let (x, y, before, after) = biggest_change(&bare, &armoured);
    let [r, g, b] = after;
    assert!(
        (r as i32 - g as i32).abs() <= 12 && (g as i32 - b as i32).abs() <= 12 && r > 40,
        "the armour's own texel reads at the change's centre ({x}, {y}): {before:?} -> {after:?}"
    );
}

/// The enchanted chestplate glints: the effect flag runs the two glint passes over the
/// armour (`LayerArmorBase.java`:78 through the glint pipeline), and the clock set, the
/// blended overlay shifts the armour's pixels toward the glint colour — a plain chestplate
/// with the same clock does not.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_equipment_enchanted_chestplate_glints() {
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);
    let registry = equipment_registry(&device, &queue);
    let mut entities = equipment_pass(&device, &queue, &registry);
    entities.set_system_time(500);

    let chest = |enchanted: bool| {
        let mut equipment = [None; 5];
        equipment[3] = Some(EquipmentDraw {
            id: 307,
            damage: 0,
            enchanted,
            colour: None,
            cross: false,
        });
        zombie_wearing(equipment)
    };
    let plain = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        chest(false),
    );
    let glinting = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        chest(true),
    );

    let changed = changed_pixels(&plain, &glinting);
    assert!(
        changed >= 40,
        "the glint pass shifts the armour's pixels: {changed} pixels changed"
    );
    let (x, y, before, after) = biggest_change(&plain, &glinting);
    assert!(
        after[2] as i32 > before[2] as i32 + 20,
        "the glint adds its blue at ({x}, {y}): {before:?} -> {after:?}"
    );
}

/// The creeper's charge draws its aura: the powered flag runs the inflated second pass in
/// the additive blend (`LayerCreeperCharge.java`:28-31), lifting the creeper's own pixels
/// and adding a rim over the sky — an unpowered creeper draws neither.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_equipment_creeper_charge_overlay_answers_the_flag() {
    let (device, queue) = headless_device();
    let target = create_target(&device, wgpu::TextureFormat::Rgba8Unorm);
    let depth = create_depth(&device);
    let registry = equipment_registry(&device, &queue);
    let mut entities = equipment_pass(&device, &queue, &registry);

    let creeper = |powered: bool| {
        mob_at_origin(
            ModelRef::Creeper,
            "entity/creeper/creeper.png",
            DrawExtra::Creeper { powered },
        )
    };
    let plain = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        creeper(false),
    );
    let charged = render_mob(
        &device,
        &queue,
        &target,
        &depth,
        &mut entities,
        &registry,
        creeper(true),
    );

    let changed = changed_pixels(&plain, &charged);
    assert!(
        changed >= 40,
        "the aura redraws the creeper: {changed} pixels changed"
    );
    assert!(
        model_pixels(&charged) > model_pixels(&plain),
        "the inflated pass adds a rim: {} -> {} non-sky pixels",
        model_pixels(&plain),
        model_pixels(&charged)
    );
    let (x, y, before, after) = biggest_change(&plain, &charged);
    assert!(
        after[0] as i32 >= before[0] as i32
            && after[1] as i32 >= before[1] as i32
            && after[2] as i32 >= before[2] as i32,
        "the additive pass only lifts at ({x}, {y}): {before:?} -> {after:?}"
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
        id: 0,
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
        name: None,
        below_name: None,
        equipment: [None; 5],
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

/// Runs a pass that only clears the target to `colour`, with no depth attachment, so a
/// hud pass can follow and blend over the result.
fn with_clear_pass(
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    colour: wgpu::Color,
) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("oxide pipeline headless clear pass"),
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

/// The blend of a black bar at `bar_alpha` over the sky: the pipeline's `src_alpha` over
/// `one_minus_src_alpha` pair leaves every channel at `channel * (1 - bar_alpha / 255)`.
fn bar_over_sky(bar_alpha: u8) -> [u8; 3] {
    let alpha = f32::from(bar_alpha) / 255.0;
    SKY.map(|channel| (f32::from(channel) * (1.0 - alpha)).round() as u8)
}

/// The blend of the tab list's white entry cell over the grid's panel over the sky —
/// `GuiPlayerTabOverlay.renderPlayerlist`:167's `553648127` fill (0x20FFFFFF) over `:159`'s
/// `Integer.MIN_VALUE` panel: the panel leaves `channel * (1 - 128/255)`, then the cell
/// adds `255 * 32/255` on what remains of `channel * (1 - 32/255)`.
fn cell_over_panel_over_sky() -> [u8; 3] {
    let alpha = 32.0 / 255.0;
    bar_over_sky(128)
        .map(|channel| (f32::from(channel) * (1.0 - alpha) + 255.0 * alpha).round() as u8)
}

/// The blend of the sidebar's band at `band_alpha` over the sky: the assembly's black
/// fill over the cleared frame — the rows at 80/255 and the title band at 96/255
/// (`view.rs:2149`/:2153) — the same rule [`bar_over_sky`] measures for the chat's bar.
fn sidebar_band_over_sky(band_alpha: u8) -> [u8; 3] {
    bar_over_sky(band_alpha)
}

/// The hud draws a chat line's shape over the cleared frame: a black bar at the line's
/// fade alpha and its shadowed text, at the GUI coordinates the chat assembly lays out.
///
/// The line is the box's newest: its bar's top sits at `height - 37` scaled pixels — the
/// `height - 48` block origin (`GuiIngame.java`:343-345) plus the `(2, 20)` translate
/// (`GuiNewChat.java`:49-51) minus the first pitch step (`:81-82`) — its bar 9 tall and 48
/// wide (the assembly's bar is the 320-pixel wrap budget plus four, `view.rs`:891-893;
/// the probe shortens it to fit the 64x64 target), and its text's ink one pixel below the
/// bar's top at the bar's left edge (`:85`). The resolution is 64x64 GUI units onto the 64x64
/// target, so one unit is one pixel and every value lands at its own coordinate.
///
/// The counts: the bar covers 48x9 = 432 pixels, the row above it the sky, pinning the y
/// anchor. The two '|' glyphs ink one column each and their one-font-pixel-down-right
/// shadow copies land two more columns, both flush with the bar's last row; each shadow
/// hangs one row lower than its glyph, so two shadow pixels sit below the bar and the
/// frame's non-sky total is 432 + 2 = 434.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_a_chat_line() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);

    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, SIZE as f32, SIZE as f32);
    hud.set_font(&device, &queue, &overlay_font_sheet())
        .expect("the synthetic sheet is a 16x16 grid");
    let skins = TextureRegistry::new(&device, &queue);
    hud.set_draws(
        &device,
        &queue,
        &[
            HudDraw::Rect {
                x: 2.0,
                y: 27.0,
                width: 48.0,
                height: 9.0,
                colour: [0.0, 0.0, 0.0, 127.0 / 255.0],
            },
            HudDraw::Text {
                text: "||".to_string(),
                x: 2.0,
                y: 28.0,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: true,
                blend: true,
            },
        ],
        &skins,
    );

    let depth = create_depth(&device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide hud headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| hud.draw(pass));
    queue.submit(Some(encoder.finish()));

    let pixels = read_pixels(&device, &queue, &target);
    // The bar's y anchor: the rows on either side of it are still the sky.
    expect_pixel(&pixels, 26, 26, SKY, "the row above the bar");
    expect_pixel(&pixels, 26, 27, bar_over_sky(127), "the bar's top row");
    expect_pixel(
        &pixels,
        2,
        27,
        bar_over_sky(127),
        "the bar's top-left corner",
    );
    expect_pixel(&pixels, 49, 31, bar_over_sky(127), "the bar's right column");
    expect_pixel(&pixels, 26, 35, bar_over_sky(127), "the bar's last row");
    expect_pixel(&pixels, 26, 36, SKY, "the row below the bar");
    expect_pixel(&pixels, 1, 31, SKY, "left of the bar");
    expect_pixel(&pixels, 50, 31, SKY, "right of the bar");
    // The glyphs and their shadows: ink at each pen, the shadow one pixel down and right
    // at the source's 63/255, the cell's transparent columns showing the bar through.
    expect_pixel(&pixels, 2, 28, TEXT, "the first glyph's ink");
    expect_pixel(
        &pixels,
        3,
        29,
        SHADOW,
        "its shadow one pixel down and right",
    );
    expect_pixel(
        &pixels,
        3,
        28,
        bar_over_sky(127),
        "its transparent neighbour",
    );
    expect_pixel(&pixels, 4, 28, TEXT, "the second glyph two advances along");
    expect_pixel(&pixels, 5, 29, SHADOW, "its shadow");
    let lit = non_sky(&pixels);
    assert_eq!(
        lit, 434,
        "the bar's 432 pixels plus the two shadow pixels below its last row"
    );
}

/// The hud draws the fade series at four alphas: the line alphas of four fade ages, each
/// a bar of the chat assembly's shape.
///
/// The fade (`GuiNewChat.drawChat`:59-68): `d0 = 1 - age / 200`, times ten, clamped to
/// one, squared, 255 times, truncated — ages 179, 181, 185 and 195 give line alphas 255,
/// 230, 143 and 15: full, the first tick past it, mid-fade and the tail above the draw
/// gate. The bar draws at the line's integer half (`l1 / 2`, `:82`): 127, 115, 71 and 7.
///
/// The counts: four 32x9 bars, none overlapping, 4 * 288 = 1152 non-sky pixels.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_the_fade_series_at_four_alphas() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);

    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, SIZE as f32, SIZE as f32);

    // (line alpha, bar alpha) at the four ages; the bar alpha is the integer half.
    let series: [(u8, u8); 4] = [(255, 127), (230, 115), (143, 71), (15, 7)];
    let draws: Vec<HudDraw> = series
        .iter()
        .enumerate()
        .map(|(index, &(_, bar))| HudDraw::Rect {
            x: 2.0,
            y: 2.0 + index as f32 * 11.0,
            width: 32.0,
            height: 9.0,
            colour: [0.0, 0.0, 0.0, f32::from(bar) / 255.0],
        })
        .collect();
    let skins = TextureRegistry::new(&device, &queue);
    hud.set_draws(&device, &queue, &draws, &skins);

    let depth = create_depth(&device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide hud headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| hud.draw(pass));
    queue.submit(Some(encoder.finish()));

    let pixels = read_pixels(&device, &queue, &target);
    expect_pixel(&pixels, 18, 1, SKY, "above the first bar");
    expect_pixel(&pixels, 18, 12, SKY, "between the first and second bars");
    for (index, &(_, bar)) in series.iter().enumerate() {
        let y = 2 + index as u32 * 11 + 4;
        expect_pixel(
            &pixels,
            18,
            y,
            bar_over_sky(bar),
            "a bar of the fade series at its own alpha",
        );
    }
    let lit = non_sky(&pixels);
    assert_eq!(lit, 1152, "four bars of 32x9, none overlapping");
}

/// The hud draws the open input line and its caret: the field's frame, its text at the
/// assembly's pen and the end caret bar — the shapes `view.rs`'s input line lays out,
/// mirrored here.
///
/// The frame is `(2, height - 14)` to `(width - 2, height - 2)` at `Integer.MIN_VALUE`
/// (`GuiChat.java`:303) — at this 64x64 probe: `(2, 50, 60, 12)` at half-alpha black.
/// The text sits at the pen `(4, height - 12)` (`GuiChat.java`:58, the textbox's own
/// spot with its background drawing off) in the enabled colour 14737632 = 0xE0E0E0
/// (`GuiTextField.java`:52), and the end caret is the bar straddling `pen - 1`, `i1 - 1`
/// to `i1 + 1 + 9`, at 0xFFD0D0D0 (`GuiTextField.java`:578) — one pixel wide, eleven
/// tall, at the pen the one synthetic glyph (the sheet inks only `|`, advance 2) leaves
/// at 4 + 2 = 6, so 5. The caret draws only in the lit blink phase (`:540`); this case is
/// the lit one.
///
/// The counts: the frame covers 60x12 = 720 pixels, and the glyph's ink rows (52-59), its
/// shadow column and the caret column (5, rows 51-61) all fall inside it — the frame's
/// 720 is the whole lit total.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_the_input_line_and_cursor() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);

    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, SIZE as f32, SIZE as f32);
    hud.set_font(&device, &queue, &overlay_font_sheet())
        .expect("the synthetic sheet is a 16x16 grid");
    let skins = TextureRegistry::new(&device, &queue);
    hud.set_draws(
        &device,
        &queue,
        &[
            HudDraw::Rect {
                x: 2.0,
                y: 50.0,
                width: 60.0,
                height: 12.0,
                colour: [0.0, 0.0, 0.0, 128.0 / 255.0],
            },
            HudDraw::Text {
                text: "|".to_string(),
                x: 4.0,
                y: 52.0,
                scale: 1.0,
                colour: [224.0 / 255.0, 224.0 / 255.0, 224.0 / 255.0, 1.0],
                shadow: true,
                blend: true,
            },
            HudDraw::Rect {
                x: 5.0,
                y: 51.0,
                width: 1.0,
                height: 11.0,
                colour: [208.0 / 255.0, 208.0 / 255.0, 208.0 / 255.0, 1.0],
            },
        ],
        &skins,
    );

    let depth = create_depth(&device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide hud headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| hud.draw(pass));
    queue.submit(Some(encoder.finish()));

    let pixels = read_pixels(&device, &queue, &target);
    // The frame's anchor: the rows on either side of it are still the sky.
    expect_pixel(&pixels, 30, 49, SKY, "the row above the frame");
    expect_pixel(&pixels, 30, 50, bar_over_sky(128), "the frame's top row");
    expect_pixel(
        &pixels,
        2,
        50,
        bar_over_sky(128),
        "the frame's top-left corner",
    );
    expect_pixel(
        &pixels,
        61,
        50,
        bar_over_sky(128),
        "the frame's last column",
    );
    expect_pixel(&pixels, 30, 61, bar_over_sky(128), "the frame's last row");
    expect_pixel(&pixels, 30, 62, SKY, "the row below the frame");
    expect_pixel(&pixels, 1, 50, SKY, "left of the frame");
    expect_pixel(&pixels, 62, 50, SKY, "right of the frame");
    // The text's ink at the pen, in the field's own colour; the caret bar's column at
    // pen - 1, from pen_y - 1 down its eleven rows.
    expect_pixel(
        &pixels,
        4,
        52,
        [224, 224, 224],
        "the field text's ink at the pen",
    );
    expect_pixel(
        &pixels,
        5,
        51,
        [208, 208, 208],
        "the caret's top pixel at pen - 1",
    );
    expect_pixel(&pixels, 5, 61, [208, 208, 208], "the caret's last row");
    expect_pixel(
        &pixels,
        6,
        52,
        bar_over_sky(128),
        "right of the caret, the frame alone",
    );
    let lit = non_sky(&pixels);
    assert_eq!(
        lit, 720,
        "the frame's 60x12 pixels; text, shadow and caret all land inside it"
    );
}

/// The hud draws the open chat's scrolled slice: two lines of the open window at the
/// assembly's own geometry, and the slice moves when the scroll does.
///
/// The lines: an open box draws full alpha from `height - 37` with a nine-pixel pitch
/// (`GuiNewChat.java`:81-82 minus the first step; the assembly's `line_top`), each a
/// black bar and its shadowed text one pixel below the bar's top at the bar's left
/// (`:82`-`:85`). The probe's 48-wide bar is the shortened form of the assembly's 324,
/// as the chat-line case records.
///
/// Slice A (scroll 0) draws `||` at the top slot and `|` beneath; slice B (the scroll one
/// line back) shows the same box further up its history: the top slot carries the line
/// that was the lower one (`|`) and a next older line (`|||`) enters below. The pixels
/// that move prove the slice: (4, 28) is glyph ink in A and bar in B; (4, 19) is bar in
/// A and glyph in B; (5, 36) a shadow in A and sky in B.
///
/// The counts: two 48x9 bars = 864 in both. A adds the top slot's two-glyph shadow
/// column below its last bar row (row 36 at x 3 and 5) = 2 -> 866; B adds the one-glyph
/// line's single shadow pixel (row 36 at x 3) = 1 -> 865. The lower slot's shadow
/// columns land on the top slot's bar rows (row 27) — pixels already non-sky, no add.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_the_open_box_scrolled_slice() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);

    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, SIZE as f32, SIZE as f32);
    hud.set_font(&device, &queue, &overlay_font_sheet())
        .expect("the synthetic sheet is a 16x16 grid");

    let bar = |y: f32| HudDraw::Rect {
        x: 2.0,
        y,
        width: 48.0,
        height: 9.0,
        colour: [0.0, 0.0, 0.0, 1.0],
    };
    let text = |body: &str, y: f32| HudDraw::Text {
        text: body.to_string(),
        x: 2.0,
        y,
        scale: 1.0,
        colour: [1.0, 1.0, 1.0, 1.0],
        shadow: true,
        blend: true,
    };

    let skins = TextureRegistry::new(&device, &queue);
    hud.set_draws(
        &device,
        &queue,
        &[bar(27.0), text("||", 28.0), bar(18.0), text("|", 19.0)],
        &skins,
    );
    let depth = create_depth(&device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide hud headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| hud.draw(pass));
    queue.submit(Some(encoder.finish()));

    let pixels = read_pixels(&device, &queue, &target);
    // The slice's span: sky above the lower bar's top and below the upper bar's last row.
    expect_pixel(&pixels, 26, 17, SKY, "above the slice");
    expect_pixel(
        &pixels,
        26,
        18,
        bar_over_sky(255),
        "the lower slot's top row",
    );
    expect_pixel(
        &pixels,
        26,
        26,
        bar_over_sky(255),
        "the lower slot's last row",
    );
    expect_pixel(&pixels, 26, 27, bar_over_sky(255), "the top slot's top row");
    expect_pixel(
        &pixels,
        26,
        35,
        bar_over_sky(255),
        "the top slot's last row",
    );
    expect_pixel(&pixels, 26, 36, SKY, "below the slice");
    // Slice A's glyphs: `||` up top, `|` below — the second column of the top slot is
    // ink, the second column of the lower slot is bare bar; the top slot's shadows
    // hang one row below its bar.
    expect_pixel(&pixels, 2, 28, TEXT, "the top slot's first glyph");
    expect_pixel(&pixels, 4, 28, TEXT, "the top slot's second glyph");
    expect_pixel(
        &pixels,
        4,
        19,
        bar_over_sky(255),
        "the lower slot's bare column",
    );
    expect_pixel(&pixels, 3, 36, SHADOW, "a shadow below the top slot's bar");
    expect_pixel(&pixels, 5, 36, SHADOW, "the second shadow column");
    let lit = non_sky(&pixels);
    assert_eq!(
        lit, 866,
        "two bars of 48x9, and the top slot's two shadow pixels"
    );

    // The slice scrolled one line: the lower slot's `|` slides up and `|||` enters below.
    hud.set_draws(
        &device,
        &queue,
        &[bar(27.0), text("|", 28.0), bar(18.0), text("|||", 19.0)],
        &skins,
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide hud headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| hud.draw(pass));
    queue.submit(Some(encoder.finish()));

    let pixels = read_pixels(&device, &queue, &target);
    // The moved slice: the top slot now carries the one-glyph line — its second column
    // is bare bar where A had ink — and the lower slot the three-glyph one.
    expect_pixel(&pixels, 2, 28, TEXT, "the top slot's one glyph");
    expect_pixel(
        &pixels,
        4,
        28,
        bar_over_sky(255),
        "no second glyph up top now",
    );
    expect_pixel(&pixels, 4, 19, TEXT, "the lower slot's second glyph");
    expect_pixel(&pixels, 6, 19, TEXT, "the lower slot's third glyph");
    expect_pixel(&pixels, 5, 36, SKY, "the second shadow column is gone");
    let lit = non_sky(&pixels);
    assert_eq!(
        lit, 865,
        "two bars of 48x9, and the one-glyph line's one shadow pixel"
    );
}

/// The hud draws the tab list's shape over the cleared frame: a one-line header and
/// footer over a two-row grid — the first row with a list-objective score and a full
/// latency rect, the second with the no-signal one — in the source's painter order
/// (`GuiPlayerTabOverlay.renderPlayerlist`:145-234).
///
/// The shape follows the source's rules: the centred panels at `width / 2 - l1 / 2 - 1`
/// spanning `l1 + 2` (`:147`, `:159`, `:226`); rows on the nine-pixel pitch with
/// eight-high cells and the five-pixel column gutter (`:163-167`); the head columns at
/// `(8, 8)` and `(40, 8)` of the 64-texel skin space (`:186`, `:192`); the name at the
/// cell's ninth pixel (`:195`); the ten-by-eight latency rect at the cell's right edge
/// (`GuiPlayerTabOverlay.drawPing`:270); and the `§e` score right-aligned at its
/// field's right edge (`GuiPlayerTabOverlay.drawScoreboardValues`:365-366).
///
/// The stand-ins: the probe's own cell width and panel span (the source's cell is
/// name- and score-dependent and narrower at this width, `GuiPlayerTabOverlay.renderPlayerlist`:118, the synthetic
/// font's single `|` glyph for the header, the footer, the names and the score, and the
/// synthetic sheet's bands for the two latencies; the heads resolve the registry's
/// default (the placeholder image), so their cells show its texels where a real skin's
/// face and hat regions would sample. The resolution is 64x64 GUI units onto the 64x64
/// target, so one unit is one pixel and every value lands at its own coordinate.
///
/// The count: the header, grid and footer panels span 42 columns by 39 rows — 1638
/// non-sky pixels — and every cell, head, name, score and bar lands inside them.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_the_tab_list() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);

    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, SIZE as f32, SIZE as f32);
    hud.set_font(&device, &queue, &overlay_font_sheet())
        .expect("the synthetic sheet is a 16x16 grid");
    hud.set_texture(&device, &queue, "gui/icons", &icon_sheet());
    let skins = TextureRegistry::new(&device, &queue);
    let head = skins
        .resolve("00000000-0000-0000-0000-000000000000", false)
        .id();
    let panel = [0.0, 0.0, 0.0, 128.0 / 255.0];
    let cell = [1.0, 1.0, 1.0, 32.0 / 255.0];
    let tint = [1.0, 1.0, 1.0, 1.0];
    let face = [8.0 / 64.0, 8.0 / 64.0, 16.0 / 64.0, 16.0 / 64.0];
    let hat = [40.0 / 64.0, 8.0 / 64.0, 48.0 / 64.0, 16.0 / 64.0];
    let ping_good = [0.0, 176.0 / 256.0, 10.0 / 256.0, 184.0 / 256.0];
    let ping_none = [0.0, 216.0 / 256.0, 10.0 / 256.0, 224.0 / 256.0];
    hud.set_draws(
        &device,
        &queue,
        &[
            // The header block: its panel, then the centred line's string.
            HudDraw::Rect {
                x: 11.0,
                y: 9.0,
                width: 42.0,
                height: 10.0,
                colour: panel,
            },
            HudDraw::Text {
                text: "|".to_string(),
                x: 31.0,
                y: 10.0,
                scale: 1.0,
                colour: tint,
                shadow: true,
                blend: true,
            },
            // The grid's panel.
            HudDraw::Rect {
                x: 11.0,
                y: 19.0,
                width: 42.0,
                height: 19.0,
                colour: panel,
            },
            // Entry one: the cell, the head (face then hat), the name, the score and
            // the latency rect.
            HudDraw::Rect {
                x: 12.0,
                y: 20.0,
                width: 40.0,
                height: 8.0,
                colour: cell,
            },
            HudDraw::SkinRect {
                texture: head,
                x: 12.0,
                y: 20.0,
                width: 8.0,
                height: 8.0,
                uv: face,
                colour: tint,
            },
            HudDraw::SkinRect {
                texture: head,
                x: 12.0,
                y: 20.0,
                width: 8.0,
                height: 8.0,
                uv: hat,
                colour: tint,
            },
            HudDraw::Text {
                text: "|".to_string(),
                x: 21.0,
                y: 20.0,
                scale: 1.0,
                colour: tint,
                shadow: true,
                blend: true,
            },
            HudDraw::Text {
                text: "§e|".to_string(),
                x: 34.0,
                y: 20.0,
                scale: 1.0,
                colour: tint,
                shadow: true,
                blend: true,
            },
            HudDraw::TexturedRect {
                texture: HudTexture::Named("gui/icons"),
                x: 41.0,
                y: 20.0,
                width: 10.0,
                height: 8.0,
                uv: ping_good,
                colour: tint,
            },
            // Entry two: the cell, the head, the name and the no-signal latency rect.
            HudDraw::Rect {
                x: 12.0,
                y: 29.0,
                width: 40.0,
                height: 8.0,
                colour: cell,
            },
            HudDraw::SkinRect {
                texture: head,
                x: 12.0,
                y: 29.0,
                width: 8.0,
                height: 8.0,
                uv: face,
                colour: tint,
            },
            HudDraw::SkinRect {
                texture: head,
                x: 12.0,
                y: 29.0,
                width: 8.0,
                height: 8.0,
                uv: hat,
                colour: tint,
            },
            HudDraw::Text {
                text: "|".to_string(),
                x: 21.0,
                y: 29.0,
                scale: 1.0,
                colour: tint,
                shadow: true,
                blend: true,
            },
            HudDraw::TexturedRect {
                texture: HudTexture::Named("gui/icons"),
                x: 41.0,
                y: 29.0,
                width: 10.0,
                height: 8.0,
                uv: ping_none,
                colour: tint,
            },
            // The footer block: its panel, then the centred line's string.
            HudDraw::Rect {
                x: 11.0,
                y: 38.0,
                width: 42.0,
                height: 10.0,
                colour: panel,
            },
            HudDraw::Text {
                text: "|".to_string(),
                x: 31.0,
                y: 39.0,
                scale: 1.0,
                colour: tint,
                shadow: true,
                blend: true,
            },
        ],
        &skins,
    );

    let depth = create_depth(&device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide hud headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| hud.draw(pass));
    queue.submit(Some(encoder.finish()));

    let pixels = read_pixels(&device, &queue, &target);
    // The panels' extent: sky above the header and below the footer, the panel blend at
    // the corners and rows, and the centred header's margins clear while its centre
    // column is panel.
    expect_pixel(&pixels, 26, 8, SKY, "the row above the header");
    expect_pixel(
        &pixels,
        11,
        9,
        bar_over_sky(128),
        "the header's top-left corner",
    );
    expect_pixel(
        &pixels,
        26,
        9,
        bar_over_sky(128),
        "the header band's top row",
    );
    expect_pixel(
        &pixels,
        26,
        18,
        bar_over_sky(128),
        "the header band's last row",
    );
    expect_pixel(&pixels, 26, 19, bar_over_sky(128), "the grid's top row");
    expect_pixel(
        &pixels,
        32,
        9,
        bar_over_sky(128),
        "the header's centre column",
    );
    expect_pixel(&pixels, 6, 12, SKY, "the margin left of the centred header");
    expect_pixel(&pixels, 57, 12, SKY, "the margin right of it");
    expect_pixel(
        &pixels,
        52,
        47,
        bar_over_sky(128),
        "the footer's bottom-right corner",
    );
    expect_pixel(&pixels, 26, 48, SKY, "the row below the footer");
    // The cells: the white 32/255 fill over the grid's panel at both rows, clear of the
    // head, name, score and bar.
    expect_pixel(
        &pixels,
        30,
        23,
        cell_over_panel_over_sky(),
        "the first cell's bare area",
    );
    expect_pixel(
        &pixels,
        30,
        32,
        cell_over_panel_over_sky(),
        "the second cell's bare area",
    );
    // The heads: the default's texel where the face and hat windows land.
    expect_pixel(&pixels, 14, 22, [0, 0, 0], "the first head's cell");
    expect_pixel(&pixels, 16, 31, [0, 0, 0], "the second head's cell");
    // The names and the score: the ink glyphs at their pens, the score in `§e`'s yellow.
    expect_pixel(&pixels, 21, 20, TEXT, "the first name's ink");
    expect_pixel(&pixels, 34, 20, [255, 255, 85], "the score's ink");
    // The latency rects: the green band under the first cell's level and the no-signal
    // band under the second's.
    expect_pixel(&pixels, 41, 20, [0, 255, 0], "the first latency's top-left");
    expect_pixel(&pixels, 45, 23, [0, 255, 0], "the first latency's band");
    expect_pixel(
        &pixels,
        45,
        32,
        [255, 0, 0],
        "the second latency's no-signal band",
    );
    let lit = non_sky(&pixels);
    assert_eq!(
        lit, 1638,
        "the three panels' 42x39 extent; every cell, head, name, score and bar inside"
    );
}

/// The icon case's stand-in atlas: a 16x16 level-0 checkerboard of [`CHECKER_A`] and
/// [`CHECKER_B`] texels and its 8x8 reduced level, every texel the two colours' average —
/// what the stitcher's own per-sprite reduction of a checkerboard produces — under one
/// sprite covering the whole image.
///
/// The pixels are generated here; no asset store is read and no Mojang pixel is embedded.
fn checkerboard_atlas() -> Atlas {
    const SIDE: u32 = 16;
    let bytes = |texels: &[[u8; 4]]| -> Vec<u8> {
        let mut rgba = Vec::with_capacity(texels.len() * 4);
        for texel in texels {
            rgba.extend_from_slice(texel);
        }
        rgba
    };
    let checker: Vec<[u8; 4]> = (0..SIDE * SIDE)
        .map(|index| {
            let (x, y) = (index % SIDE, index / SIDE);
            if (x + y) % 2 == 0 {
                CHECKER_A
            } else {
                CHECKER_B
            }
        })
        .collect();
    let reduced = vec![CHECKER_AVERAGE; ((SIDE / 2) * (SIDE / 2)) as usize];
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
        levels: vec![
            AtlasLevel {
                width: SIDE,
                height: SIDE,
                rgba: bytes(&checker),
            },
            AtlasLevel {
                width: SIDE / 2,
                height: SIDE / 2,
                rgba: bytes(&reduced),
            },
        ],
        width: SIDE,
        height: SIDE,
        level_count: 2,
        sprites: BTreeMap::from([("test:checker".to_string(), whole)]),
        animated: BTreeMap::new(),
        missing: whole,
    }
}

/// The hud draws one minified sprite twice — through the standing `Atlas` binding and
/// through the icon binding — and the two readings differ: the standing binding blends the
/// reduced level in, the icon binding samples the sprite's own level-0 texels.
///
/// The source: the block atlas's standing state is blur off with mipmaps on
/// (`Minecraft.java:548-554`, `setBlurMipmapDirect(false, mipmapLevels > 0)`), the pair the
/// terrain's mipped layers read it through; the GUI item draws switch it to
/// `setBlurMipmap(false, false)` before their quads and restore after
/// (`RenderItem.java`:318/:357 — the survey's §1.4), the nearest, level-0-only state
/// [`HudTexture::AtlasIcon`] carries. A minified icon that sampled the standing binding
/// would blend the reduced level in — the rig notes' minified-sprite mip class — where the
/// source's icon draws stay crisp.
///
/// The stand-in atlas is hand-built here: a 16x16 two-colour checkerboard at level 0 and
/// its 8x8 average at level 1. Each draw is a 4x4 GUI-pixel quad sampling the whole
/// 16-texel sprite, so the sampling footprint is four texels per pixel and the level of
/// detail reaches level 1. The resolution is 64x64 GUI units onto the 64x64 target, so one
/// unit is one pixel and each quad lands at its own coordinate.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_atlasicon_binding_stays_crisp_where_the_atlas_binding_blends() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);

    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, SIZE as f32, SIZE as f32);
    hud.set_atlas(&device, &queue, &checkerboard_atlas());
    hud.set_atlas_icon(&device, &queue, &checkerboard_atlas());
    let tint = [1.0, 1.0, 1.0, 1.0];
    let whole = [0.0, 0.0, 1.0, 1.0];
    hud.set_draws(
        &device,
        &queue,
        &[
            // The standing binding: the minified quad through the mipped pair.
            HudDraw::TexturedRect {
                texture: HudTexture::Atlas,
                x: 0.0,
                y: 0.0,
                width: 4.0,
                height: 4.0,
                uv: whole,
                colour: tint,
            },
            // The icon binding: the same quad, crisp.
            HudDraw::TexturedRect {
                texture: HudTexture::AtlasIcon,
                x: 0.0,
                y: 4.0,
                width: 4.0,
                height: 4.0,
                uv: whole,
                colour: tint,
            },
        ],
        &TextureRegistry::new(&device, &queue),
    );

    let depth = create_depth(&device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide hud headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| hud.draw(pass));
    queue.submit(Some(encoder.finish()));

    let pixels = read_pixels(&device, &queue, &target);
    // The standing binding: every pixel the reduced level's own texel — the blend a
    // minified sample reads in through the mipped pair.
    for y in 0..4 {
        for x in 0..4 {
            expect_texel(
                &pixels,
                x,
                y,
                CHECKER_AVERAGE,
                "the standing binding's pixel",
            );
        }
    }
    // The icon binding: every pixel one of the sprite's own two texels — level 0 alone,
    // never the reduced level's blend.
    for y in 4..8 {
        for x in 0..4 {
            let got = pixel_rgba(&pixels, x, y);
            assert!(
                got == CHECKER_A || got == CHECKER_B,
                "the icon binding's pixel at ({x}, {y}) is one of the sprite's own texels: got {got:?}"
            );
        }
    }
}

/// The hud draws the scoreboard sidebar's shape over the cleared frame: the title band,
/// three entry rows and the red number — the assembly's shapes (`view.rs`'s
/// `sidebar_draws`; `GuiIngame.renderScoreboard`:551-607) at the case's own numbers.
///
/// The shape follows the source's rules: the block `9n` tall from the `H/2 + 9n/3`
/// baseline (`:581-585`), each row's band spanning `right - left + 2` at the text column
/// `W - widest - 3` (`:593-595`), the red number right-aligned at its own width off the
/// row's right edge (`:592`), and the title band with its one-pixel separator closing the
/// list, the title centred by integer division (`:599-605`). The alphas are the
/// assembly's constants: the rows' band at 80/255, the title band at 96/255, the text at
/// 32/255 (`view.rs:2149`/:2153/:2157) — black fills under white text, the number's `§c`
/// run the table's (255, 85, 85) (`text.rs:82-85`, `FontRenderer.java`:400).
///
/// The stand-ins: the probe's own widest (20) and the synthetic font's single `|` glyph
/// for the names, the title and the number — the number on the middle row alone, where
/// the assembly draws one per row. The resolution is 64x64 GUI units onto the 64x64
/// target, so one unit is one pixel and every value lands at its own coordinate.
///
/// The count: the three row bands and the title band are 24x9 each and the separator
/// 24x1 — 4 * 216 + 24 = 888 non-sky pixels, the inks inside them.
///
/// The pinning run measured the unblended values byte for byte: the row band
/// [108, 133, 172], the title band [99, 121, 156], the number's ink [255, 85, 85], the
/// name and title inks [255, 255, 255], and the 888 lit pixels.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_the_scoreboard_sidebar() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);

    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, SIZE as f32, SIZE as f32);
    hud.set_font(&device, &queue, &overlay_font_sheet())
        .expect("the synthetic sheet is a 16x16 grid");

    let band = [0.0, 0.0, 0.0, 80.0 / 255.0];
    let title_band = [0.0, 0.0, 0.0, 96.0 / 255.0];
    let text = [1.0, 1.0, 1.0, 32.0 / 255.0];
    let rect = |x: f32, y: f32, width: f32, height: f32, colour: [f32; 4]| HudDraw::Rect {
        x,
        y,
        width,
        height,
        colour,
    };
    let line = |body: &str, x: f32, y: f32| HudDraw::Text {
        text: body.to_string(),
        x,
        y,
        scale: 1.0,
        colour: text,
        shadow: false,
        blend: false,
    };
    let skins = TextureRegistry::new(&device, &queue);
    hud.set_draws(
        &device,
        &queue,
        &[
            // Row one (the bottom): its band and the name at the text column.
            rect(39.0, 32.0, 24.0, 9.0, band),
            line("|", 41.0, 32.0),
            // Row two: the band, the name and the red number right-aligned at the row's
            // right edge (63 - the number's width 2).
            rect(39.0, 23.0, 24.0, 9.0, band),
            line("|", 41.0, 23.0),
            line("§c|", 61.0, 23.0),
            // Row three (the top): its band and name.
            rect(39.0, 14.0, 24.0, 9.0, band),
            line("|", 41.0, 14.0),
            // The title band closing the list: the band, its one-pixel separator and the
            // centred title (41 + 20/2 - 2/2).
            rect(39.0, 4.0, 24.0, 9.0, title_band),
            rect(39.0, 13.0, 24.0, 1.0, band),
            line("|", 50.0, 5.0),
        ],
        &skins,
    );

    let depth = create_depth(&device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide hud headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| hud.draw(pass));
    queue.submit(Some(encoder.finish()));

    let pixels = read_pixels(&device, &queue, &target);
    // The frame around the sidebar: the sky above the title band and below the bottom
    // row, and either side of the bands.
    expect_pixel(&pixels, 26, 3, SKY, "above the title band");
    expect_pixel(&pixels, 26, 41, SKY, "below the bottom row");
    expect_pixel(&pixels, 38, 33, SKY, "left of the bands");
    expect_pixel(&pixels, 63, 33, SKY, "right of the bands");
    // (1) The row band: black at 80/255 over the sky, from the bottom row's first row to
    // its last, and the middle row's last row above it.
    expect_pixel(
        &pixels,
        40,
        32,
        sidebar_band_over_sky(80),
        "the bottom row's band top",
    );
    expect_pixel(
        &pixels,
        40,
        33,
        sidebar_band_over_sky(80),
        "the bottom row's band",
    );
    expect_pixel(
        &pixels,
        40,
        40,
        sidebar_band_over_sky(80),
        "the bottom row's band last row",
    );
    expect_pixel(
        &pixels,
        40,
        31,
        sidebar_band_over_sky(80),
        "the middle row's band last row",
    );
    // (2) The red number's ink: the `§c` run over the band, one glyph column, from its
    // first row to its last.
    expect_pixel_exact(&pixels, 61, 23, [255, 85, 85], "the red number's first row");
    expect_pixel_exact(&pixels, 61, 25, [255, 85, 85], "the red number's ink");
    expect_pixel_exact(&pixels, 61, 30, [255, 85, 85], "the red number's last row");
    // (2b) The names' ink: white at the text column, unblended like the number.
    expect_pixel_exact(&pixels, 41, 32, TEXT, "the bottom name's first row");
    expect_pixel_exact(&pixels, 41, 36, TEXT, "the bottom name's ink");
    expect_pixel_exact(&pixels, 41, 39, TEXT, "the bottom name's last row");
    expect_pixel_exact(&pixels, 41, 23, TEXT, "the middle name's ink");
    expect_pixel_exact(&pixels, 41, 14, TEXT, "the top name's ink");
    // (3) Right-aligned: the ink's column is the row's right edge minus its width
    // (63 - 2 = 61); the band is bare on either side and below the ink.
    expect_pixel(
        &pixels,
        60,
        25,
        sidebar_band_over_sky(80),
        "left of the number",
    );
    expect_pixel(
        &pixels,
        62,
        25,
        sidebar_band_over_sky(80),
        "right of the number",
    );
    expect_pixel(
        &pixels,
        61,
        31,
        sidebar_band_over_sky(80),
        "below the number",
    );
    // (4) The title: its ink at the centred column, the title band bare at the margin
    // and at the separator's own row.
    expect_pixel_exact(&pixels, 50, 8, TEXT, "the title's ink");
    expect_pixel(
        &pixels,
        42,
        8,
        sidebar_band_over_sky(96),
        "the title band at the margin",
    );
    expect_pixel(
        &pixels,
        40,
        4,
        sidebar_band_over_sky(96),
        "the title band's first row",
    );
    expect_pixel(
        &pixels,
        40,
        13,
        sidebar_band_over_sky(80),
        "the separator row",
    );
    let lit = non_sky(&pixels);
    eprintln!(
        "sidebar: band {:?}, title band {:?}, number {:?}, title ink {:?}, {lit} lit",
        pixel(&pixels, 40, 33),
        pixel(&pixels, 40, 8),
        pixel(&pixels, 61, 25),
        pixel(&pixels, 50, 8)
    );
    assert_eq!(
        lit, 888,
        "the three row bands and the title band 24x9 each, the separator 24x1"
    );
}

/// A hud pass ready for the boss bar's cases: the 64x64 target, the synthetic font and
/// icon sheet, and the scaled resolution the bar composes against.
fn boss_bar_scene() -> (wgpu::Device, wgpu::Queue, Target, HudPass, ScaledResolution) {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);
    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, SIZE as f32, SIZE as f32);
    hud.set_font(&device, &queue, &overlay_font_sheet())
        .expect("the synthetic sheet is a 16x16 grid");
    hud.set_texture(&device, &queue, "gui/icons", &icon_sheet());
    (device, queue, target, hud, scaled_resolution(SIZE, SIZE, 0))
}

/// A raised status named `|` at `fraction`, for the bar's cases.
fn boss_status(fraction: f32) -> BossStatus {
    BossStatus {
        name: "|".to_owned(),
        health_fraction: fraction,
        colour_modifier: true,
        time: BOSS_STATUS_TIME,
    }
}

/// Renders the boss bar's list over a cleared frame and reads the pixels back.
fn boss_bar_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &Target,
    hud: &HudPass,
) -> Vec<u8> {
    let depth = create_depth(device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide hud headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| {
        hud.draw_boss_bar(pass)
    });
    queue.submit(Some(encoder.finish()));
    read_pixels(device, queue, target)
}

/// The hud draws the half bar's slices over the cleared frame: the fill's right edge at
/// the boundary pixel and the background past it (`GuiIngame.renderBossHealth`:901-926).
///
/// The shape follows the source's rules: the bar at `x = scaledWidth / 2 - 182 / 2`
/// (`:908-910`), `y = 12`, five tall (`:912`); the background slice `(0, 74, 182, 5)`
/// drawn twice identically (`:913-914`); the fill `(0, 79, l, 5)` with
/// `l = (int)(healthScale * (float)(182 + 1))` — 91 at half (`:911`, `:916-918`). The
/// resolution is 64x64 GUI units onto the 64x64 target, so one unit is one pixel: the
/// bar's left edge lands at `32 - 91 = -59`, off the target's left side, and the fill
/// ends at `-59 + 91 = 32`, so the last visible fill pixel is column 31 and the first
/// background-only pixel column 32. The name draws above: `32 - 2 / 2 = 31` for the
/// synthetic `|` glyph (`:921-922`), white and shadowed.
///
/// The stand-ins: the synthetic sheet's blue background band and yellow fill band (no
/// asset store is read) and the synthetic font's single `|` glyph for the name.
///
/// The count: the bar spans all 64 visible columns of its five rows — 320 pixels — and
/// the name adds its eight-row ink column and its eight-row shadow column, 336 non-sky
/// pixels in all. The pinning run measured the fill [255, 255, 0], the background
/// [0, 0, 255], the ink [255, 255, 255], the shadow [63, 63, 63] and the 336.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_the_half_boss_bar() {
    let (device, queue, target, mut hud, scaled) = boss_bar_scene();
    hud.set_boss_bar(&device, &queue, Some(&boss_status(0.5)), &scaled);
    let pixels = boss_bar_frame(&device, &queue, &target, &hud);

    // The bar's vertical span: sky above and below its five rows.
    expect_pixel(&pixels, 31, 11, SKY, "above the bar");
    expect_pixel(&pixels, 31, 17, SKY, "below the bar");
    // The fill's visible span and its edge: the last fill pixel at column 31, the first
    // background-only pixel at 32 — a fill one pixel narrower or wider lands elsewhere.
    expect_pixel(&pixels, 0, 14, BAR_FILL, "the fill's visible left edge");
    expect_pixel(&pixels, 31, 14, BAR_FILL, "the last fill pixel");
    expect_pixel(&pixels, 32, 14, BAR_BACK, "the first background-only pixel");
    expect_pixel(
        &pixels,
        63,
        14,
        BAR_BACK,
        "the background's visible right edge",
    );
    // The name above the bar: its ink column and its one-pixel-down-right shadow.
    expect_pixel(&pixels, 31, 2, TEXT, "the name's ink top");
    expect_pixel(&pixels, 32, 3, SHADOW, "the name's shadow");
    let lit = non_sky(&pixels);
    assert_eq!(lit, 336, "the bar's 64x5 span and the name's two columns");
}

/// The hud draws the full bar's slices over the cleared frame: the fill across the whole
/// visible span, the background under it (`GuiIngame.renderBossHealth`:901-926).
///
/// The shape rules are the half bar's; `l` is 183 at full (`:911`), so the fill spans
/// `-59 .. 124` and every visible column is fill — the background only shows past column
/// 124, off the target.
///
/// The count: the bar's 64x5 span and the name's two columns — the same 336 non-sky
/// pixels as the half bar. The pinning run measured the fill [255, 255, 0] at both edges
/// and the 336.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_the_full_boss_bar() {
    let (device, queue, target, mut hud, scaled) = boss_bar_scene();
    hud.set_boss_bar(&device, &queue, Some(&boss_status(1.0)), &scaled);
    let pixels = boss_bar_frame(&device, &queue, &target, &hud);

    expect_pixel(&pixels, 31, 11, SKY, "above the bar");
    expect_pixel(&pixels, 31, 17, SKY, "below the bar");
    expect_pixel(&pixels, 0, 14, BAR_FILL, "the fill's visible left edge");
    expect_pixel(
        &pixels,
        31,
        14,
        BAR_FILL,
        "the fill at the half bar's boundary",
    );
    expect_pixel(&pixels, 63, 14, BAR_FILL, "the fill's visible right edge");
    let lit = non_sky(&pixels);
    assert_eq!(lit, 336, "the bar's 64x5 span and the name's two columns");
}

/// The hud stops drawing the boss bar when the countdown's hundred frames are spent: the
/// raise's frame and the ninety-nine after it draw, and the hundredth finds the spent
/// status and draws nothing (`GuiIngame.renderBossHealth`:903-905 — the gate runs before
/// the decrement, so the raise's frame enters at the full hundred).
///
/// The cell follows [`boss_status_step`]: the raise sets it, each further frame spends
/// one of its frames, and the frame that finds zero hides the bar. The resolution is
/// 64x64 GUI units onto the 64x64 target, so the half bar's boundary lands as in its own
/// case.
///
/// The counts: the drawn frame carries the half bar's 336 non-sky pixels; the spent
/// frame's frame is the clear alone, zero.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_hides_the_spent_boss_bar() {
    let (device, queue, target, mut hud, scaled) = boss_bar_scene();

    // The raise's frame draws the bar.
    let mut cell = None;
    let raised =
        boss_status_step(&mut cell, Some(boss_status(0.5))).expect("the raise's frame draws");
    hud.set_boss_bar(&device, &queue, Some(&raised), &scaled);
    let pixels = boss_bar_frame(&device, &queue, &target, &hud);
    expect_pixel(
        &pixels,
        31,
        14,
        BAR_FILL,
        "the raise's frame draws the fill",
    );
    expect_pixel(&pixels, 32, 14, BAR_BACK, "and the background past it");
    assert_eq!(non_sky(&pixels), 336, "the half bar's pixels");

    // The countdown: f+1 through f+99 draw; f+100's gate finds the spent status.
    for frame in 1..BOSS_STATUS_TIME {
        assert!(
            boss_status_step(&mut cell, None).is_some(),
            "frame f+{frame} still draws"
        );
    }
    let spent = boss_status_step(&mut cell, None);
    assert!(spent.is_none(), "f+100's gate finds the spent status");
    hud.set_boss_bar(&device, &queue, None, &scaled);
    let pixels = boss_bar_frame(&device, &queue, &target, &hud);
    assert_eq!(non_sky(&pixels), 0, "the spent frame draws nothing");
}

// ---------------------------------------------------------------------------------------
// The GUI item draws: the fixtures' own atlas and glint sheet, the synthetic icon source
// the four cases resolve through, and the cases themselves.
// ---------------------------------------------------------------------------------------

/// The item fixtures' top face sprite: solid red.
const ITEM_TOP_TEXEL: [u8; 4] = [255, 0, 0, 255];
/// The item fixtures' side sprite: solid blue.
const ITEM_SIDE_TEXEL: [u8; 4] = [0, 0, 255, 255];
/// The generated fixture's checkerboard, light texel.
const ITEM_FLAT_A: [u8; 4] = [255, 255, 255, 255];
/// The generated fixture's checkerboard, dark texel.
const ITEM_FLAT_B: [u8; 4] = [0, 0, 0, 255];
/// The second icon's sprite: solid green.
const ITEM_GREEN_TEXEL: [u8; 4] = [0, 255, 0, 255];
/// The glint fixture's lit texel, and the one the pattern leaves dark.
const GLINT_LIT: [u8; 4] = [255, 255, 255, 255];
const GLINT_DARK: [u8; 4] = [0, 0, 0, 255];

/// The item fixtures' own atlas: a 16x16 level 0 holding four flat 8x8 sprites — the block
/// icon's top (red) and side (blue) faces, the generated icon's one-texel checkerboard, and
/// the second icon's solid green — with the whole image as the missing sprite.
///
/// The texels are generated here; no asset store is read and no Mojang pixel is embedded.
fn item_atlas() -> Atlas {
    const SIDE: u32 = 16;
    let mut texels = vec![[0u8, 0, 0, 255]; (SIDE * SIDE) as usize];
    paint_rect(&mut texels, SIDE, 0, 0, 8, 8, ITEM_TOP_TEXEL);
    paint_rect(&mut texels, SIDE, 8, 0, 8, 8, ITEM_SIDE_TEXEL);
    paint_rect(&mut texels, SIDE, 8, 8, 8, 8, ITEM_GREEN_TEXEL);
    for ty in 0..8 {
        for tx in 0..8 {
            let colour = if (tx + ty) % 2 == 0 {
                ITEM_FLAT_A
            } else {
                ITEM_FLAT_B
            };
            paint_rect(&mut texels, SIDE, tx, 8 + ty, 1, 1, colour);
        }
    }
    let sprite = |x: u32, y: u32| AtlasSprite {
        region: SpriteRect { x, y, w: 8, h: 8 },
        content: SpriteRect { x, y, w: 8, h: 8 },
    };
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
    let mut rgba = Vec::with_capacity(texels.len() * 4);
    for texel in &texels {
        rgba.extend_from_slice(texel);
    }
    Atlas {
        levels: vec![AtlasLevel {
            width: SIDE,
            height: SIDE,
            rgba,
        }],
        width: SIDE,
        height: SIDE,
        level_count: 1,
        sprites: BTreeMap::from([
            ("fixture:top".to_string(), sprite(0, 0)),
            ("fixture:side".to_string(), sprite(8, 0)),
            ("fixture:flat".to_string(), sprite(0, 8)),
            ("fixture:green".to_string(), sprite(8, 8)),
        ]),
        animated: BTreeMap::new(),
        missing: whole,
    }
}

/// Fills one rectangle of a row-major texel image, the fixture builder's own brush.
fn paint_rect(texels: &mut [[u8; 4]], side: u32, x: u32, y: u32, w: u32, h: u32, colour: [u8; 4]) {
    for ty in y..y + h {
        for tx in x..x + w {
            texels[(ty * side + tx) as usize] = colour;
        }
    }
}

/// The glint fixture's sheet: 16x16 at alpha 255, one-texel white and black columns, so the
/// glint's own sampling shows in the pixels it adds over an icon — and its eightfold uv
/// scale wraps through the glint sampler rather than clamping at the sheet's edge.
///
/// The texels are generated here; no asset store is read and no Mojang pixel is embedded.
fn glint_sheet() -> Texture {
    const SIDE: u32 = 16;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    for y in 0..SIDE {
        for x in 0..SIDE {
            let colour = if x % 2 == 0 { GLINT_LIT } else { GLINT_DARK };
            let at = ((y * SIDE + x) * 4) as usize;
            rgba[at..at + 4].copy_from_slice(&colour);
        }
    }
    Texture {
        width: SIDE,
        height: SIDE,
        rgba,
    }
}

/// The synthetic icon source the item cases resolve through: id 1 the block cube (its top
/// face on the fixtures' red sprite, its other faces on the blue one), id 2 the generated
/// quad on the checkerboard sprite and id 3 the generated quad on the green sprite — every
/// mesh built from [`item_atlas`]'s own sprite rects, never from a store.
struct TestIcons {
    /// The fixtures' atlas, whose sprite rects the meshes' uvs map into.
    atlas: Atlas,
}

impl TestIcons {
    /// The block cube: the six faces of the 0..16 box, the top face on `top`'s sprite and
    /// the rest on `side`'s, every vertex carrying its own face's normal.
    fn cube(&self, top: &str, side: &str) -> ItemMesh {
        let mut vertices = Vertices {
            positions: Vec::new(),
            uvs: Vec::new(),
            normals: Vec::new(),
        };
        // One cube face: its four corners, its normal and the sprite its uvs map.
        type CubeFace<'a> = ([[f32; 3]; 4], [f32; 3], &'a str);
        let faces: [CubeFace<'_>; 6] = [
            (
                [
                    [0.0, 16.0, 16.0],
                    [16.0, 16.0, 16.0],
                    [16.0, 16.0, 0.0],
                    [0.0, 16.0, 0.0],
                ],
                [0.0, 1.0, 0.0],
                top,
            ),
            (
                [
                    [0.0, 0.0, 0.0],
                    [16.0, 0.0, 0.0],
                    [16.0, 0.0, 16.0],
                    [0.0, 0.0, 16.0],
                ],
                [0.0, -1.0, 0.0],
                side,
            ),
            (
                [
                    [0.0, 16.0, 0.0],
                    [16.0, 16.0, 0.0],
                    [16.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0],
                ],
                [0.0, 0.0, -1.0],
                side,
            ),
            (
                [
                    [0.0, 0.0, 16.0],
                    [16.0, 0.0, 16.0],
                    [16.0, 16.0, 16.0],
                    [0.0, 16.0, 16.0],
                ],
                [0.0, 0.0, 1.0],
                side,
            ),
            (
                [
                    [0.0, 16.0, 0.0],
                    [0.0, 16.0, 16.0],
                    [0.0, 0.0, 16.0],
                    [0.0, 0.0, 0.0],
                ],
                [-1.0, 0.0, 0.0],
                side,
            ),
            (
                [
                    [16.0, 16.0, 16.0],
                    [16.0, 16.0, 0.0],
                    [16.0, 0.0, 0.0],
                    [16.0, 0.0, 16.0],
                ],
                [1.0, 0.0, 0.0],
                side,
            ),
        ];
        for (corners, normal, sprite) in faces {
            self.push_face(&mut vertices, corners, normal, sprite);
        }
        ItemMesh {
            vertices: Arc::new(vertices),
            texture: ATLAS_TEXTURE,
        }
    }

    /// The generated item's quad: the 0..16 plane at the model's own z 8, the sprite's
    /// rect its four corners.
    fn flat(&self, sprite: &str) -> ItemMesh {
        let mut vertices = Vertices {
            positions: Vec::new(),
            uvs: Vec::new(),
            normals: Vec::new(),
        };
        let corners = [
            [0.0, 0.0, 8.0],
            [16.0, 0.0, 8.0],
            [16.0, 16.0, 8.0],
            [0.0, 16.0, 8.0],
        ];
        self.push_face(&mut vertices, corners, [0.0, 0.0, 1.0], sprite);
        ItemMesh {
            vertices: Arc::new(vertices),
            texture: ATLAS_TEXTURE,
        }
    }

    /// Appends one quad: the corners in order, the normal on every corner, and the sprite's
    /// own four uv corners.
    fn push_face(
        &self,
        vertices: &mut Vertices,
        corners: [[f32; 3]; 4],
        normal: [f32; 3],
        sprite: &str,
    ) {
        let [[u0, v0], [u1, v1]] = self.atlas.uv(self.atlas.drawn(sprite));
        for (corner, uv) in corners
            .into_iter()
            .zip([[u0, v0], [u0, v1], [u1, v1], [u1, v0]])
        {
            vertices.positions.push(corner);
            vertices.uvs.push(uv);
            vertices.normals.push(normal);
        }
    }
}

impl ItemIconSource for TestIcons {
    fn icon(&self, id: i16, _damage: i16) -> Option<ItemIconMesh> {
        match id {
            1 => Some(ItemIconMesh {
                mesh: self.cube("fixture:top", "fixture:side"),
                transform: Transform::DEFAULT,
                first_person: Transform::DEFAULT,
                third_person: Transform::DEFAULT,
                shape: IconShape::Gui3d,
            }),
            2 => Some(ItemIconMesh {
                mesh: self.flat("fixture:flat"),
                transform: Transform::DEFAULT,
                first_person: Transform::DEFAULT,
                third_person: Transform::DEFAULT,
                shape: IconShape::Flat,
            }),
            3 => Some(ItemIconMesh {
                mesh: self.flat("fixture:green"),
                transform: Transform::DEFAULT,
                first_person: Transform::DEFAULT,
                third_person: Transform::DEFAULT,
                shape: IconShape::Flat,
            }),
            // The sword-shaped fixture: the flat quad with the real diamond sword's
            // first-person display transform, for the held item's own cases.
            4 => Some(ItemIconMesh {
                mesh: self.flat("fixture:green"),
                transform: Transform::DEFAULT,
                first_person: sword_transform(),
                third_person: sword_third_transform(),
                shape: IconShape::Flat,
            }),
            // The chest-shaped fixture: the folded trio's own mesh on a synthetic
            // 64x64 sheet under the trio's texture name, for the builtin class's own
            // held case.
            5 => Some(ItemIconMesh {
                mesh: ItemMesh {
                    vertices: Arc::new(objects::chest_item()),
                    texture: "entity/chest/normal",
                },
                transform: Transform::DEFAULT,
                first_person: Transform::DEFAULT,
                third_person: Transform::DEFAULT,
                shape: IconShape::Builtin,
            }),
            _ => None,
        }
    }

    fn missing_icon(&self) -> Option<ItemIconMesh> {
        Some(ItemIconMesh {
            mesh: self.flat("fixture:top"),
            transform: Transform::DEFAULT,
            first_person: Transform::DEFAULT,
            third_person: Transform::DEFAULT,
            shape: IconShape::Flat,
        })
    }
}

/// The diamond sword's first-person display transform, exactly as its model JSON
/// states it (`models/item/diamond_sword.json`'s `display.firstperson`).
fn sword_transform() -> Transform {
    Transform {
        rotation: [0.0, -135.0, 25.0],
        translation: [0.0, 4.0, 2.0],
        scale: [1.7, 1.7, 1.7],
    }
}

/// The diamond sword's third-person display transform, exactly as its model JSON states
/// it (`models/item/diamond_sword.json`'s `display.thirdperson`) — the slot the entity
/// held item's layer applies (`LayerHeldItem.java`:66).
fn sword_third_transform() -> Transform {
    Transform {
        rotation: [0.0, 90.0, -35.0],
        translation: [0.0, 1.25, -3.5],
        scale: [0.85, 0.85, 0.85],
    }
}

/// Renders one item-draw frame over the cleared target: the depth buffer cleared and
/// attached, the hud's list set, and the pixels read back.
fn item_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &Target,
    hud: &HudPass,
) -> Vec<u8> {
    let depth = create_depth(device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide hud headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| hud.draw(pass));
    queue.submit(Some(encoder.finish()));
    read_pixels(device, queue, target)
}

/// The hud draws a block item's icon: the block cube's three visible faces at their own
/// lit colours, its own silhouette inside the draw's 16x16 cell and the sky around it.
///
/// The chain is the source's own (`RenderItem.renderItemIntoGUI`:353-397): the 3D branch's
/// 40x scale and its 210-about-x, -135-about-y turns (`:385-387`), the default display
/// transform (a 1.8 block item's own model chain carries no `display.gui` entry — the
/// assets' `block/cube.json` and `item/stone.json` state only a third-person one — so
/// `ItemCameraTransforms.DEFAULT` applies) and the render path's 0.5 scale and -0.5
/// translate (`:145`, `:157`). The z is the ladder's first rung: `100 + 50`
/// (`setupGuiTransform`:378, `renderItemAndEffectIntoGUI`:402).
///
/// The pinning run measured the bytes: the top face [255, 0, 0] (its light 1.320266
/// clamped to 1 by the fixed-function colour clamp), the left face [0, 0, 162] (the east
/// face's 0.636575), the right face [0, 0, 111] (the north face's 0.434702) — the two
/// side faces the source's own lights leave distinct.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_a_block_items_icon() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);

    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, SIZE as f32, SIZE as f32);
    hud.set_atlas_icon(&device, &queue, &item_atlas());
    hud.set_icon_source(
        &device,
        &queue,
        Arc::new(TestIcons {
            atlas: item_atlas(),
        }),
    );
    hud.set_draws(
        &device,
        &queue,
        &[HudDraw::Item {
            stack: Some(ItemIcon {
                id: 1,
                damage: 0,
                enchanted: false,
            }),
            x: 24.0,
            y: 24.0,
            pop: 0.0,
        }],
        &TextureRegistry::new(&device, &queue),
    );

    let pixels = item_frame(&device, &queue, &target, &hud);
    expect_pixel(&pixels, 32, 27, [255, 0, 0], "the icon's top face");
    expect_pixel(&pixels, 28, 34, [0, 0, 162], "the icon's left face");
    expect_pixel(&pixels, 35, 34, [0, 0, 111], "the icon's right face");
    // The silhouette: the cube's own extent stops short of its cell's corners, and the
    // sky above and beside it is untouched.
    expect_pixel(&pixels, 32, 22, SKY, "the sky above the icon");
    expect_pixel(&pixels, 23, 32, SKY, "the sky beside the icon");
    expect_pixel(&pixels, 40, 40, SKY, "the sky past the icon's cell");
    // The pinning run measured the icon's own 176 pixels — the cube's silhouette under
    // the case's rasterization (the 384 of the first draft was the geometric
    // prediction); the floor keeps a margin under the measurement.
    let own = non_sky(&pixels);
    assert!(
        own > 140,
        "the icon's own pixel count: the cube's silhouette, got {own}"
    );
}

/// The held item's cases' projection aspect: the client's own 16:9 window. The probe
/// target is square, so the cases drive the pass with the aspect the client's frames
/// carry — the chain's geometry is the frame's, and the probe only rasterizes it.
const HELD_ASPECT: f32 = 16.0 / 9.0;

/// The camera the held item's cases draw with: at the origin, level (the lights unturned),
/// the default fov, the near plane and one chunk's far plane — the pass doubles the far
/// for the hand (`EntityRenderer.renderHand`:844).
fn held_camera() -> Camera {
    Camera {
        pose: CameraPose {
            position: [0.0, 0.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            sneak: false,
        },
        fov_degrees: DEFAULT_FOV,
        near: NEAR_PLANE,
        far_chunks: 1.0,
        view_effect: NO_VIEW_EFFECT,
    }
}

/// The held item's cases' frame: the sword fixture (`TestIcons` id 4) with the given
/// rendered arguments, at the full-bright equivalent and awake.
fn held_frame(equip: f32, equip_prev: f32, sleeping: bool) -> HeldItemFrame {
    HeldItemFrame {
        stack: Some(ItemIcon {
            id: 4,
            damage: 0,
            enchanted: false,
        }),
        equip,
        equip_prev,
        swing: 0.0,
        swing_prev: 0.0,
        sway_pitch: 0.0,
        sway_yaw: 0.0,
        brightness: 1.0,
        sleeping,
    }
}

/// The held chest case's frame: the chest fixture (`TestIcons` id 5) at rest, full
/// bright and awake.
fn held_chest_frame() -> HeldItemFrame {
    HeldItemFrame {
        stack: Some(ItemIcon {
            id: 5,
            damage: 0,
            enchanted: false,
        }),
        equip: 0.0,
        equip_prev: 0.0,
        swing: 0.0,
        swing_prev: 0.0,
        sway_pitch: 0.0,
        sway_yaw: 0.0,
        brightness: 1.0,
        sleeping: false,
    }
}

/// The held chest case's sheet: the chest model's own two uv cells — the lid and the
/// knob sample rows 0..19 (their `v = 0` cell), the base rows 19..44 (its `v = 19`
/// cell). The lid cell is banded along u (the box's own face columns: west 0..14,
/// down/north 14..28, up/east 28..42, south 42..56), so the builtin class's
/// `rotate(180, Y)` — which swaps the model's north and south faces — changes the
/// pixels; the base cell is one colour, the classifier's own "base".
fn chest_sheet() -> Texture {
    let lid = |u: usize| match u {
        0..=13 => [220, 60, 40, 255],
        14..=27 => [80, 200, 60, 255],
        28..=41 => [240, 240, 240, 255],
        42..=55 => [250, 200, 40, 255],
        _ => [140, 80, 200, 255],
    };
    const BASE: [u8; 4] = [60, 120, 220, 255];
    let mut rgba = Vec::with_capacity(64 * 64 * 4);
    for row in 0..64 {
        for column in 0..64 {
            let colour = if row < 19 { lid(column) } else { BASE };
            rgba.extend_from_slice(&colour);
        }
    }
    Texture {
        width: 64,
        height: 64,
        rgba,
    }
}

/// Builds the held item's cases' pass: the atlas bound under the item sampler and the
/// fixture source set, the camera already driven.
fn held_pass(device: &wgpu::Device, queue: &wgpu::Queue) -> HeldItemPass {
    let mut held = HeldItemPass::new(device, wgpu::TextureFormat::Rgba8Unorm);
    held.set_atlas_icon(device, queue, &item_atlas());
    held.set_icon_source(Arc::new(TestIcons {
        atlas: item_atlas(),
    }));
    held.set_camera(device, queue, &held_camera(), HELD_ASPECT);
    held
}

/// Renders one held item frame over the cleared target: the colour cleared to the sky,
/// the pass's own draw in the overlay-shaped pass (colour loaded, depth cleared) and the
/// pixels read back.
fn held_item_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &Target,
    held: &HeldItemPass,
) -> Vec<u8> {
    let depth = create_depth(device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide held item headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| held.draw(pass));
    queue.submit(Some(encoder.finish()));
    read_pixels(device, queue, target)
}

/// The frame's ink: the pixels that are not the sky, with their bounds and centroid.
struct Ink {
    /// How many pixels the item landed over.
    count: usize,
    /// The ink's bounds.
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    /// The ink's centroid.
    centroid_x: f32,
    centroid_y: f32,
}

/// Measures one frame's ink, or nothing when the frame stayed sky.
fn ink(pixels: &[u8]) -> Option<Ink> {
    let mut ink = Ink {
        count: 0,
        min_x: SIZE as f32,
        min_y: SIZE as f32,
        max_x: 0.0,
        max_y: 0.0,
        centroid_x: 0.0,
        centroid_y: 0.0,
    };
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            if pixel(pixels, x, y) == SKY {
                continue;
            }
            ink.count += 1;
            let (x, y) = (x as f32, y as f32);
            ink.min_x = ink.min_x.min(x);
            ink.min_y = ink.min_y.min(y);
            ink.max_x = ink.max_x.max(x);
            ink.max_y = ink.max_y.max(y);
            sum_x += x;
            sum_y += y;
        }
    }
    if ink.count == 0 {
        return None;
    }
    ink.centroid_x = sum_x / ink.count as f32;
    ink.centroid_y = sum_y / ink.count as f32;
    Some(ink)
}

/// The held item's rest frame: the sword fixture's quad through the first-person chain —
/// `doItemUsed(0) . TFPI(0, 0) . S(2) . display(firstperson) . S(0.5) . T(-0.5) . S(1/16)`
/// with the model's own `[0, -135, 25]` rotation, `[0, 4, 2]` translation and 1.7 scale.
///
/// The predictions come from `refs/m5-task-12/derive.py` (its `derive.log` is the
/// receipt), sampled densely and clipped to the frame at this case's own 16:9 aspect:
/// the ink spans `(46.7, 21.5)..(64, 64)`, its centroid sits at `(54.05, 46.23)` — left
/// of the bounds' own centre-x `55.36` by 1.31 pixels — the mesh's centre projects to
/// `(56.97, 54.52)` and the quad's shade is `0.497014`. The pins below carry a few
/// pixels' slack around those predictions; the run that first pinned them is recorded in
/// the task report.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_held_sword_draws_its_blade_in_the_frames_lower_right() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);
    let mut held = held_pass(&device, &queue);
    held.set_frame(&device, &queue, &held_frame(0.0, 0.0, false));

    let pixels = held_item_frame(&device, &queue, &target, &held);
    let ink = ink(&pixels).expect("the blade's ink");
    // The blade fills the right half from the frame's bottom edge to just above its
    // middle: the bounds' own prediction, with slack.
    assert!(
        (ink.min_x - 46.7).abs() < 4.0,
        "the blade's left bound: {} (predicted 46.7)",
        ink.min_x
    );
    assert!(
        (ink.min_y - 21.5).abs() < 4.0,
        "the blade's top bound: {} (predicted 21.5)",
        ink.min_y
    );
    assert!(
        ink.max_x > 61.0 && ink.max_y > 61.0,
        "the blade reaches the frame's corner: {} x {}",
        ink.max_x,
        ink.max_y
    );
    assert!(
        ink.centroid_x > 40.0 && ink.centroid_y > 40.0,
        "the blade's own mass sits lower-right: ({}, {})",
        ink.centroid_x,
        ink.centroid_y
    );
    // The colour pin: the green fixture sprite's texel times the quad's own shade
    // (255 * 0.497014 = 127), sampled at a pixel well inside the silhouette.
    expect_pixel(&pixels, 57, 55, [0, 127, 0], "the blade's lit texel");
    assert!(
        ink.count > 400,
        "the blade's own pixel count: got {}",
        ink.count
    );
}

/// The held sword's orientation-sensitive assertion: the blade is seen edge-on, and the
/// reversed 45-degree turn sweeps it across the frame.
///
/// The carried notes predicted the ink's centroid to sit left of its bounds' centre at
/// rest and to flip under the mutation. The pinning run measured otherwise: the centroid
/// sits right of the bounds' centre at BOTH turns (margin -1.29 at rest, -1.44 under the
/// reversed turn — the notes' dense sample over-weighted the quad's near part), so this
/// case asserts the signals the run found to move: the silhouette's own width (16 px
/// edge-on at rest, 29 px under the reversed turn), its left edge (47 vs 34, crossing the
/// frame's centre), the recorded side and margin, and the shade the case-1 pin reads
/// ([0, 127, 0] at rest — the edge-on face's own normal — against the reversed turn's
/// [0, 189, 0]). The mutation run in `refs/m5-task-12/mutations.sh` proves every one of
/// them fails under the reversed turn.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_held_sword_blade_is_seen_edge_on() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);
    let mut held = held_pass(&device, &queue);
    held.set_frame(&device, &queue, &held_frame(0.0, 0.0, false));

    let pixels = held_item_frame(&device, &queue, &target, &held);
    let ink = ink(&pixels).expect("the blade's ink");
    let width = ink.max_x - ink.min_x;
    assert!(
        width <= 20.0,
        "the blade's edge-on width: {width} px (the reversed turn shows its face, 29 px)"
    );
    assert!(
        ink.min_x >= 44.0,
        "the blade's own left edge: {} (the reversed turn sweeps it to 34, past the \
         frame's centre)",
        ink.min_x
    );
    // The notes' own signal, recorded as measured: the mass sits right of the bounds'
    // centre by 1.29 px at rest.
    let centre_x = (ink.min_x + ink.max_x) / 2.0;
    let margin = centre_x - ink.centroid_x;
    assert!(
        (-3.0..=-0.5).contains(&margin),
        "the blade's mass sits right of its bounds' centre by {margin:.2} px \
         (centroid_x {:.2}, bounds' centre-x {centre_x:.2})",
        ink.centroid_x
    );
    expect_pixel(
        &pixels,
        57,
        55,
        [0, 127, 0],
        "the blade's lit texel: the edge-on face's own shade (the reversed turn reads 189)",
    );
}

/// The held item at the equip ease's midpoint: `f = 0.5` drops it by `0.5 * -0.6` model
/// units (`ItemRenderer.java`:299's `equipProgress * -0.6F`), so its ink slides down the
/// frame and its top bound sits well below the rest frame's — the item is drawn, lower.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_held_item_draws_dropped_at_the_eases_midpoint() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);
    let mut held = held_pass(&device, &queue);
    held.set_frame(&device, &queue, &held_frame(0.5, 0.0, false));

    let pixels = held_item_frame(&device, &queue, &target, &held);
    let ink = ink(&pixels).expect("the dropped item's ink");
    assert!(
        ink.min_y > 44.0,
        "the item has dropped below the rest position: top bound {} (rest's 22)",
        ink.min_y
    );
    assert!(
        ink.count > 150,
        "the dropped item's own pixel count: got {}",
        ink.count
    );
    // The pin: the item's own green texel times the shade, at a pixel inside the
    // dropped silhouette (the pinning run measured bounds (47, 48)..(63, 63)).
    expect_pixel(&pixels, 57, 57, [0, 127, 0], "the dropped item's lit texel");
}

/// The held item draws nothing while the player sleeps: the source's own skip
/// (`EntityRenderer.java`:861-863) — the hand is skipped while `thePlayer.isPlayerSleeping()`.
/// The frame stays sky, pixel for pixel.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_held_item_draws_nothing_while_the_player_sleeps() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);
    let mut held = held_pass(&device, &queue);
    held.set_frame(&device, &queue, &held_frame(0.0, 0.0, true));

    let pixels = held_item_frame(&device, &queue, &target, &held);
    assert_eq!(non_sky(&pixels), 0, "the sleeping frame draws nothing");
    expect_pixel(&pixels, 57, 55, SKY, "the blade's own pixel stays sky");
}

/// The builtin class's own tail (`RenderItem.renderItem`:147-154's `rotate(180, Y)`
/// and `TileEntityChestRenderer.renderTileEntityAt`:124-125's lift and y/z flip): a
/// held chest carries what the flat class doesn't — the lid's own texels (the
/// sheet's rows 0..19) sit above the base's (rows 19..44), told apart by their
/// dominant channel. The lid cell is banded along u (the box's own face columns:
/// west 0..14, down/north 14..28, up/east 28..42, south 42..56), so the class's own
/// turn is pinned by which band its side face reads: east (u 28..42) with the turn,
/// west (u 0..14) without. Without the tail the lid never appears at all: the whole
/// ink is the base's own colour.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_held_chest_carries_the_builtin_tail() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);
    let mut held = held_pass(&device, &queue);
    held.set_texture(&device, &queue, "entity/chest/normal", &chest_sheet());
    held.set_frame(&device, &queue, &held_chest_frame());
    let pixels = held_item_frame(&device, &queue, &target, &held);
    let (mut lid_n, mut lid_y, mut base_n, mut base_y) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let p = pixel(&pixels, x, y);
            if p == SKY {
                continue;
            }
            if p[0] > p[2] {
                lid_n += 1.0;
                lid_y += y as f32;
            } else if p[2] > p[0] {
                base_n += 1.0;
                base_y += y as f32;
            }
        }
    }
    assert!(
        lid_n >= 100.0,
        "the lid's own texels under the builtin tail: {lid_n} pixels"
    );
    assert!(
        lid_y / lid_n < base_y / base_n.max(1.0),
        "the lid sits above the base: lid {:.2} vs base {:.2}",
        lid_y / lid_n,
        base_y / base_n.max(1.0)
    );
    expect_pixel(
        &pixels,
        50,
        55,
        [80, 200, 60],
        "the lid's own down-face texel, the flip's own shade",
    );
    expect_pixel(
        &pixels,
        45,
        58,
        [170, 170, 170],
        "the lid's own side face, the turn's own band",
    );
    expect_pixel(
        &pixels,
        43,
        63,
        [43, 85, 156],
        "the base's own shaded texel",
    );
}

/// The hud draws a generated item's sprite crisp: the checkerboard sprite's own two texels
/// alone across the icon's 16x16 cell, never a blend of them.
///
/// The draw is the flat branch (`RenderItem.setupGuiTransform`:390-394): the 64x scale and
/// the 180-about-x turn, lighting off, and the sprite filling the cell at the ladder's
/// first rung. The cell magnifies the sprite 2:1 — each texel covers two pixels — and at
/// that ratio the icon binding's level-0 pair reads the same exact texels the standing
/// atlas binding would (both pairs' mag filter is nearest), so the case pins the icon
/// draw's own crispness, not the sampler choice: the pair discrimination belongs to
/// [`the_atlasicon_binding_stays_crisp_where_the_atlas_binding_blends`], the minified
/// case.
///
/// The pinning run measured the bytes: every pixel of the cell exactly one of
/// [255, 255, 255] and [0, 0, 0] — 128 of each — and nothing else inside it.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_a_generated_items_sprite_crisp_edges() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);

    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, SIZE as f32, SIZE as f32);
    hud.set_atlas_icon(&device, &queue, &item_atlas());
    hud.set_icon_source(
        &device,
        &queue,
        Arc::new(TestIcons {
            atlas: item_atlas(),
        }),
    );
    hud.set_draws(
        &device,
        &queue,
        &[HudDraw::Item {
            stack: Some(ItemIcon {
                id: 2,
                damage: 0,
                enchanted: false,
            }),
            x: 24.0,
            y: 24.0,
            pop: 0.0,
        }],
        &TextureRegistry::new(&device, &queue),
    );

    let pixels = item_frame(&device, &queue, &target, &hud);
    let mut light = 0;
    let mut dark = 0;
    for y in 24..40 {
        for x in 24..40 {
            let got = pixel_rgba(&pixels, x, y);
            match got {
                [255, 255, 255, 255] => light += 1,
                [0, 0, 0, 255] => dark += 1,
                other => {
                    panic!("the sprite's own texels at ({x}, {y}), never a blend: got {other:?}")
                }
            }
        }
    }
    assert_eq!(light, 128, "the checkerboard's light texels");
    assert_eq!(dark, 128, "the checkerboard's dark texels");
    expect_pixel(&pixels, 23, 32, SKY, "the sky beside the cell");
    assert_eq!(non_sky(&pixels), 256, "the sprite's own 16x16 pixel count");
}

/// The hud draws the glint over an enchanted icon: the two passes' own additions over the
/// icon's pixels and the icon alone where both passes land on the pattern's dark texels.
///
/// The glint is `RenderItem.renderEffect`: the texture matrix scales the model's uvs
/// eightfold, turns them -50 degrees about z on the first pass and +10 on the second, and
/// scrolls them by `(time % 3000) / 3000 / 8` and `(time % 4873) / 4873 / 8` (`:181-192`);
/// the colour is `0xFF8040CC` (`:185`), the blend `src_alpha`/`one` (`:172-173`) and the
/// depth pair `GL_EQUAL` with writes off (`:171-172`), so both passes land exactly on the
/// icon's own fragments. At time zero both scrolls are zero, so the two passes' difference
/// is their own z turns.
///
/// The glint's own sampler is the source's pair for the sheet: both filters `GL_LINEAR`
/// (`TextureUtil.setTextureBlurMipmap(true, false)`:260-274, from the sheet's
/// `{"texture": {"blur": true}}` metadata read at `SimpleTexture.loadTexture`:44) and
/// `GL_REPEAT` (`setTextureClamped(false)`:250-251). The eightfold uv scale minifies the
/// sheet over the icon, so every covered pixel's sample is a bilinear mix of the
/// fixture's lit and dark columns: the additions form a spread of blends — under the
/// nearest sampler this case pinned three classes (12 pixels at the icon's own colour,
/// 25 with one pass's addition and 37 with both, the top face reading [255, 64, 204] and
/// [255, 128, 255]) — and under the linear one no pixel keeps the icon's own colour, so
/// the pinning run's bytes below pin the spread, one pixel per face and phase, and the
/// count of untouched pixels is the assertion that the blend reached every one of them.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_the_glint_over_an_enchanted_icon() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);

    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, SIZE as f32, SIZE as f32);
    hud.set_atlas_icon(&device, &queue, &item_atlas());
    hud.set_glint(&device, &queue, &glint_sheet());
    hud.set_icon_source(
        &device,
        &queue,
        Arc::new(TestIcons {
            atlas: item_atlas(),
        }),
    );
    hud.set_system_time(&device, &queue, 0);
    hud.set_draws(
        &device,
        &queue,
        &[HudDraw::Item {
            stack: Some(ItemIcon {
                id: 1,
                damage: 0,
                enchanted: true,
            }),
            x: 24.0,
            y: 24.0,
            pop: 0.0,
        }],
        &TextureRegistry::new(&device, &queue),
    );

    let pixels = item_frame(&device, &queue, &target, &hud);
    // The glint draws the icon's own geometry again: the silhouette is the same, so the
    // sky around it is untouched and the icon's own pixel count stands — the pinning
    // run measured 176 under the case's rasterization (the first draft's 384 was the
    // geometric prediction), and the floor keeps a margin under it.
    expect_pixel(&pixels, 32, 22, SKY, "the sky above the icon");
    let own = non_sky(&pixels);
    assert!(
        own > 140,
        "the glint adds over the icon's own pixels alone, got {own}"
    );
    // The two passes over the icon's pixels: the pattern's own texels leave the icon's
    // colour where both are dark and add the glint's colour once or twice where they are
    // lit.
    // Under the linear sampler every covered pixel's bilinear sample mixes the sheet's
    // lit and dark columns: no pixel keeps the icon's own colour (the nearest sampler
    // left 12), the additions form a spread of blends, and the pinning run's bytes below
    // pin one pixel per face and phase.
    let mut untouched = 0;
    for y in 24..40 {
        for x in 24..40 {
            let got = pixel_rgba(&pixels, x, y);
            if got == [255, 0, 0, 255] || got == [0, 0, 111, 255] || got == [0, 0, 162, 255] {
                untouched += 1;
            }
        }
    }
    assert_eq!(
        untouched, 0,
        "every covered pixel carries the linear blend's addition"
    );
    expect_pixel(&pixels, 27, 26, [255, 69, 221], "the top face's blend");
    expect_pixel(
        &pixels,
        25,
        27,
        [255, 115, 255],
        "the top face's edge blend",
    );
    expect_pixel(
        &pixels,
        32,
        27,
        [255, 66, 209],
        "the top face's centre blend",
    );
    expect_pixel(&pixels, 25, 28, [102, 51, 255], "the left face's blend");
    expect_pixel(&pixels, 33, 33, [210, 105, 255], "the right face's blend");
    expect_pixel(&pixels, 27, 37, [48, 24, 239], "the lower face's blend");
}

/// The hud draws the later icon over the earlier one: the list's own order is the source's
/// z ladder (`renderItemAndEffectIntoGUI`:402 — each icon's `zLevel` is the rung
/// `50 * (index + 1)`), so a second draw's geometry sits nearer and wins where the two
/// overlap, while the first icon's own pixels outside it stand.
///
/// The first icon is the block cube at its first rung, the second the generated quad one
/// rung deeper. Without the ladder both would share a z, and the cube's own front faces —
/// half a block, five pixels, in front of its centre — would win the overlap against a
/// later draw.
///
/// The pinning run measured the bytes: the overlap's pixel the second icon's own [0, 255,
/// 0], the first icon's pixel outside it its right face's [0, 0, 111], and the second
/// icon's own [0, 255, 0] where it stands alone.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_the_later_icon_over_the_earlier_one() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);

    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, SIZE as f32, SIZE as f32);
    hud.set_atlas_icon(&device, &queue, &item_atlas());
    hud.set_icon_source(
        &device,
        &queue,
        Arc::new(TestIcons {
            atlas: item_atlas(),
        }),
    );
    hud.set_draws(
        &device,
        &queue,
        &[
            HudDraw::Item {
                stack: Some(ItemIcon {
                    id: 1,
                    damage: 0,
                    enchanted: false,
                }),
                x: 24.0,
                y: 24.0,
                pop: 0.0,
            },
            HudDraw::Item {
                stack: Some(ItemIcon {
                    id: 3,
                    damage: 0,
                    enchanted: false,
                }),
                x: 20.0,
                y: 20.0,
                pop: 0.0,
            },
        ],
        &TextureRegistry::new(&device, &queue),
    );

    let pixels = item_frame(&device, &queue, &target, &hud);
    // The overlap: the later draw's own quad, one rung nearer.
    expect_pixel(
        &pixels,
        32,
        27,
        [0, 255, 0],
        "the later icon over the earlier one's face",
    );
    // The earlier icon's own pixel outside the later draw's cell.
    expect_pixel(
        &pixels,
        38,
        32,
        [0, 0, 111],
        "the earlier icon's right face",
    );
    // The later icon where it stands alone, and the sky past its own cell.
    expect_pixel(&pixels, 21, 21, [0, 255, 0], "the later icon's own cell");
    expect_pixel(&pixels, 19, 19, SKY, "the sky past the later icon's cell");
}

/// The hud draws the count text over the icon: the icon's own draw, then the 2D text's
/// ink and shadow over it — the batch resume path, a 2D primitive after an `Item` in one
/// list.
///
/// The source draws the stack's count after the icon, inside the slot's bottom-right
/// corner: `RenderItem.renderItemOverlayIntoGUI`:455-473 disables depth and blend, then
/// `drawStringWithShadow(s, x + 19 - 2 - width, y + 6 + 3, 16777215)` at :471 draws the
/// count string over the icon (`FontRenderer.drawStringWithShadow`:325, its shadow the
/// own `(colour & 16579836) >> 2`:589). The fixture font's `|` inks one column of its
/// cell, so the case draws the same shape at the icon's lower right and pins the resume:
/// the ink and its shadow column over the icon's own faces, the icon through the glyph's
/// transparent cells, and the icon's own pixels outside the text's cell untouched. The
/// order is what makes the ink visible — the item's geometry sits nearer than the 2D
/// draws, so a text batch ahead of the item's would be overwritten.
///
/// The pinning run measured the bytes: the ink column [255, 255, 255] and the shadow
/// [63, 63, 63] (`0xFF3F3F3F`, the source's own quarter), both over the icon's faces,
/// with the faces' own [0, 0, 162] and [0, 0, 111] standing under the glyph's
/// transparent cells.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_the_count_text_over_the_icon() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);

    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, SIZE as f32, SIZE as f32);
    hud.set_atlas_icon(&device, &queue, &item_atlas());
    hud.set_font(&device, &queue, &overlay_font_sheet())
        .expect("the synthetic sheet is a 16x16 grid");
    hud.set_icon_source(
        &device,
        &queue,
        Arc::new(TestIcons {
            atlas: item_atlas(),
        }),
    );
    hud.set_draws(
        &device,
        &queue,
        &[
            HudDraw::Item {
                stack: Some(ItemIcon {
                    id: 1,
                    damage: 0,
                    enchanted: false,
                }),
                x: 24.0,
                y: 24.0,
                pop: 0.0,
            },
            HudDraw::Text {
                text: "|".to_string(),
                x: 32.0,
                y: 30.0,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: true,
                blend: true,
            },
        ],
        &TextureRegistry::new(&device, &queue),
    );

    let pixels = item_frame(&device, &queue, &target, &hud);
    // The ink column over the icon's own faces: the text's batches after the item's.
    expect_pixel(
        &pixels,
        32,
        30,
        [255, 255, 255],
        "the ink over the icon's top face",
    );
    expect_pixel(
        &pixels,
        32,
        34,
        [255, 255, 255],
        "the ink over the icon's side face",
    );
    // The shadow column one pixel along, the source's own quarter.
    expect_pixel(&pixels, 33, 31, [63, 63, 63], "the shadow over the icon");
    // The glyph's transparent cells leave the icon's own faces standing.
    expect_pixel(
        &pixels,
        36,
        32,
        [0, 0, 111],
        "the icon through the transparent cells",
    );
    // The icon's own pixels outside the text's cell are untouched.
    expect_pixel(
        &pixels,
        32,
        27,
        [255, 0, 0],
        "the icon's top face above the text",
    );
    expect_pixel(
        &pixels,
        28,
        34,
        [0, 0, 162],
        "the icon's left face beside the text",
    );
    expect_pixel(&pixels, 23, 32, SKY, "the sky beside the icon");
    expect_pixel(&pixels, 40, 40, SKY, "the sky past the icon's cell");
}

/// The hotbar frame's own probe size: a 256x64 target, so the assembly's true
/// 182-wide background and its true coordinates land one GUI unit per pixel —
/// the frame `view.rs` composes at `ScaledResolution { width: 256, height: 64 }`
/// (`bg = (128 − 91, 64 − 22)`, slot 4's highlight at `128 − 92 + 4 × 20`,
/// slot cells at `128 − 88 + 20j`, the popup at `64 − 59`, the crosshair at
/// `(128 − 7, 32 − 7)`).
const HOTBAR_WIDE: u32 = 256;
/// The probe target's height in texels; the readback row stride (256 × 4) stays
/// a multiple of 256 bytes.
const HOTBAR_TALL: u32 = 64;

/// The offscreen colour target at the hotbar probe's size.
fn wide_target(device: &wgpu::Device, format: wgpu::TextureFormat) -> Target {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("oxide hotbar headless target"),
        size: wgpu::Extent3d {
            width: HOTBAR_WIDE,
            height: HOTBAR_TALL,
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

/// The depth texture at the hotbar probe's size.
fn wide_depth(device: &wgpu::Device) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("oxide hotbar headless depth"),
        size: wgpu::Extent3d {
            width: HOTBAR_WIDE,
            height: HOTBAR_TALL,
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

/// Reads the hotbar probe's target back: one 1024-byte row per line, 64 lines.
fn read_wide_pixels(device: &wgpu::Device, queue: &wgpu::Queue, target: &Target) -> Vec<u8> {
    const ROW: u32 = HOTBAR_WIDE * 4;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("oxide hotbar headless readback"),
        size: u64::from(ROW) * u64::from(HOTBAR_TALL),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide hotbar headless readback encoder"),
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
                rows_per_image: Some(HOTBAR_TALL),
            },
        },
        wgpu::Extent3d {
            width: HOTBAR_WIDE,
            height: HOTBAR_TALL,
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

/// Asserts one probe pixel's RGB bytes exactly, naming it in the failure message.
fn expect_wide(pixels: &[u8], x: u32, y: u32, want: [u8; 3], what: &str) {
    let offset = (y * HOTBAR_WIDE * 4 + x * 4) as usize;
    let got = [pixels[offset], pixels[offset + 1], pixels[offset + 2]];
    assert_eq!(got, want, "{what} at ({x}, {y})");
}

/// The synthetic widgets sheet: `gui/widgets` in miniature — the background
/// window `(0, 0, 182, 22)` split green left and blue right so the slice's span
/// reads in the pixels, the highlight window `(0, 22, 24, 22)` solid yellow —
/// over a transparent sheet.
///
/// Generated here; no asset store is read and no sheet pixel is copied.
fn hotbar_sheet() -> Texture {
    const SIDE: u32 = 256;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    for y in 0..22 {
        for x in 0..182 {
            let colour = if x < 91 {
                [0u8, 255, 0, 255]
            } else {
                [0u8, 0, 255, 255]
            };
            let at = ((y * SIDE + x) * 4) as usize;
            rgba[at..at + 4].copy_from_slice(&colour);
        }
    }
    for y in 22..44 {
        for x in 0..24 {
            let at = ((y * SIDE + x) * 4) as usize;
            rgba[at..at + 4].copy_from_slice(&[255, 255, 0, 255]);
        }
    }
    Texture {
        width: SIDE,
        height: SIDE,
        rgba,
    }
}

/// The synthetic hotbar font sheet: a 128x128 grid whose `1`, `6` and `|`
/// cells ink only their first column, so the count digits and the popup's
/// placeholder glyph land on known pixels at advance two.
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn hotbar_font_sheet() -> Texture {
    const SIDE: u32 = 128;
    const CELL: u32 = 8;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    for code in ['1' as u32, '6' as u32, '|' as u32] {
        let cell_x = (code % 16) * CELL;
        let cell_y = (code / 16) * CELL;
        for row in 0..CELL {
            let offset = (((cell_y + row) * SIDE + cell_x) * 4) as usize;
            rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    Texture {
        width: SIDE,
        height: SIDE,
        rgba,
    }
}

/// The synthetic crosshair sheet: `gui/icons` in miniature — the 16x16
/// crosshair window `(0, 0, 16, 16)` carrying the source's own 17 opaque white
/// texels (the vertical arm `x = 7, y = 3..11` and the horizontal arm `y = 7,
/// x = 3..11`, `GuiIngame.java`:179 over the extracted `icons.png`) over a
/// transparent sheet.
///
/// Generated here; no asset store is read and no sheet pixel is copied.
fn crosshair_sheet() -> Texture {
    const SIDE: u32 = 256;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    for y in 3..12 {
        let at = ((y * SIDE + 7) * 4) as usize;
        rgba[at..at + 4].copy_from_slice(&[255, 255, 255, 255]);
    }
    for x in 3..12 {
        let at = ((7 * SIDE + x) * 4) as usize;
        rgba[at..at + 4].copy_from_slice(&[255, 255, 255, 255]);
    }
    Texture {
        width: SIDE,
        height: SIDE,
        rgba,
    }
}

/// The hud draws one hotbar frame at its true coordinates: the 182x22
/// background slice and the slot-4 highlight from the widgets sheet, slot 0's
/// damaged sword (its icon, its count suppressed at one, its durability ramp),
/// slot 1's 16-stack (its icon, its count, no ramp), the held sword's popup at
/// forty ticks, and the crosshair inverting the sky.
///
/// The geometry is the assembly's own at `ScaledResolution { width: 256,
/// height: 64 }`, mirrored here (`view.rs`'s hotbar draws — the render crate
/// cannot import the client): the background at `(128 − 91, 64 − 22)`, the
/// highlight at `(128 − 92 + 4 × 20, 64 − 23)`, slot cells at `(128 − 88 +
/// 20j, 64 − 19)`, the count at `(x + 17 − width, y + 9)`, the ramp at `(x +
/// 2, y + 13)` for damage 780 of 1561 (`j = 7`, fill `(127, 128, 0)`), the
/// popup at `(64 − 59)` fully opaque, and the crosshair at `(128 − 7, 32 − 7)`.
///
/// The pins: the background's green/blue span and its four sky neighbours, the
/// highlight's yellow over the background's green, the sword icon's cell, the
/// ramp's three colours and the icon beside and above them, the count's white
/// ink and quarter shadow over the dirt cube, the popup's ink and shadow over
/// the sky, and the crosshair's inverted centre and arm against its untouched
/// transparent corner.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_the_hotbar_frame() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = wide_target(&device, format);

    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, HOTBAR_WIDE as f32, HOTBAR_TALL as f32);
    hud.set_texture(&device, &queue, "gui/widgets", &hotbar_sheet());
    hud.set_texture(&device, &queue, "gui/icons", &crosshair_sheet());
    hud.set_font(&device, &queue, &hotbar_font_sheet())
        .expect("the synthetic sheet is a 16x16 grid");
    hud.set_atlas_icon(&device, &queue, &item_atlas());
    hud.set_icon_source(
        &device,
        &queue,
        Arc::new(TestIcons {
            atlas: item_atlas(),
        }),
    );
    let tint = [1.0, 1.0, 1.0, 1.0];
    hud.set_draws(
        &device,
        &queue,
        &[
            HudDraw::TexturedRect {
                texture: HudTexture::Named("gui/widgets"),
                x: 37.0,
                y: 42.0,
                width: 182.0,
                height: 22.0,
                uv: [0.0, 0.0, 182.0 / 256.0, 22.0 / 256.0],
                colour: tint,
            },
            HudDraw::TexturedRect {
                texture: HudTexture::Named("gui/widgets"),
                x: 116.0,
                y: 41.0,
                width: 24.0,
                height: 22.0,
                uv: [0.0, 22.0 / 256.0, 24.0 / 256.0, 44.0 / 256.0],
                colour: tint,
            },
            HudDraw::Item {
                stack: Some(ItemIcon {
                    id: 4,
                    damage: 0,
                    enchanted: false,
                }),
                x: 40.0,
                y: 45.0,
                pop: 0.0,
            },
            HudDraw::Rect {
                x: 42.0,
                y: 58.0,
                width: 13.0,
                height: 2.0,
                colour: [0.0, 0.0, 0.0, 1.0],
            },
            HudDraw::Rect {
                x: 42.0,
                y: 58.0,
                width: 12.0,
                height: 1.0,
                colour: [31.0 / 255.0, 64.0 / 255.0, 0.0, 1.0],
            },
            HudDraw::Rect {
                x: 42.0,
                y: 58.0,
                width: 7.0,
                height: 1.0,
                colour: [127.0 / 255.0, 128.0 / 255.0, 0.0, 1.0],
            },
            HudDraw::Item {
                stack: Some(ItemIcon {
                    id: 1,
                    damage: 0,
                    enchanted: false,
                }),
                x: 60.0,
                y: 45.0,
                pop: 0.0,
            },
            HudDraw::Text {
                text: "16".to_string(),
                x: 73.0,
                y: 54.0,
                scale: 1.0,
                colour: tint,
                shadow: true,
                blend: false,
            },
            HudDraw::Text {
                text: "|".to_string(),
                x: 127.0,
                y: 5.0,
                scale: 1.0,
                colour: tint,
                shadow: true,
                blend: true,
            },
            HudDraw::InvertRect {
                x: 121.0,
                y: 25.0,
                w: 16.0,
                h: 16.0,
            },
        ],
        &TextureRegistry::new(&device, &queue),
    );

    let depth = wide_depth(&device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide hotbar headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| hud.draw(pass));
    queue.submit(Some(encoder.finish()));

    let pixels = read_wide_pixels(&device, &queue, &target);
    // The background slice: its green/blue span and the sky on all four sides.
    expect_wide(
        &pixels,
        37,
        42,
        [0, 255, 0],
        "the background's top-left texel",
    );
    expect_wide(&pixels, 100, 50, [0, 255, 0], "the background's green half");
    expect_wide(&pixels, 200, 50, [0, 0, 255], "the background's blue half");
    expect_wide(
        &pixels,
        218,
        63,
        [0, 0, 255],
        "the background's bottom-right texel",
    );
    expect_wide(&pixels, 36, 42, SKY, "the sky left of the background");
    expect_wide(&pixels, 37, 41, SKY, "the sky above the background");
    expect_wide(&pixels, 219, 42, SKY, "the sky right of the background");
    // The highlight at slot 4's offset: yellow over the background's green.
    expect_wide(&pixels, 116, 41, [255, 255, 0], "the highlight's top-left");
    expect_wide(
        &pixels,
        139,
        62,
        [255, 255, 0],
        "the highlight's bottom-right",
    );
    expect_wide(&pixels, 115, 41, SKY, "the sky left of the highlight");
    expect_wide(&pixels, 116, 40, SKY, "the sky above the highlight");
    expect_wide(
        &pixels,
        120,
        50,
        [255, 255, 0],
        "the highlight over the background",
    );
    // Slot 0's sword icon: the green cell and its edges against the background.
    expect_wide(&pixels, 40, 45, [0, 255, 0], "the icon's top-left");
    expect_wide(&pixels, 55, 57, [0, 255, 0], "the icon's bottom-right");
    expect_wide(&pixels, 56, 50, [0, 255, 0], "the background past the icon");
    // The durability ramp: the fill, the underlay, the black bed's overhang,
    // and the icon standing beside and above the bar.
    expect_wide(&pixels, 43, 58, [127, 128, 0], "the ramp's fill");
    expect_wide(&pixels, 48, 58, [127, 128, 0], "the fill's last column");
    expect_wide(&pixels, 49, 58, [31, 64, 0], "the underlay past the fill");
    expect_wide(&pixels, 53, 58, [31, 64, 0], "the underlay's last column");
    expect_wide(
        &pixels,
        54,
        58,
        [0, 0, 0],
        "the black bed past the underlay",
    );
    expect_wide(&pixels, 42, 59, [0, 0, 0], "the black bed's second row");
    expect_wide(&pixels, 41, 58, [0, 255, 0], "the icon left of the bar");
    expect_wide(&pixels, 42, 57, [0, 255, 0], "the icon above the bar");
    // The count of 16 over slot 1: the digits' white ink and their quarter
    // shadows, drawn unblended.
    expect_wide(&pixels, 73, 54, TEXT, "the one's ink");
    expect_wide(&pixels, 75, 54, TEXT, "the six's ink");
    expect_wide(&pixels, 74, 55, SHADOW, "the one's shadow");
    expect_wide(&pixels, 76, 55, SHADOW, "the six's shadow");
    // The popup at forty ticks: opaque white ink and shadow over the sky.
    expect_wide(&pixels, 127, 5, TEXT, "the popup's ink");
    expect_wide(&pixels, 128, 6, SHADOW, "the popup's shadow");
    expect_wide(&pixels, 126, 5, SKY, "the sky left of the popup");
    // The crosshair: the sky inverted under the sprite's opaque texels, untouched
    // where the sheet is transparent.
    expect_wide(&pixels, 128, 32, [97, 61, 5], "the inverted centre");
    expect_wide(&pixels, 128, 28, [97, 61, 5], "the inverted vertical arm");
    expect_wide(&pixels, 124, 32, [97, 61, 5], "the inverted horizontal arm");
    expect_wide(&pixels, 121, 25, SKY, "the transparent corner stays sky");
    expect_wide(&pixels, 136, 40, SKY, "the opposite corner stays sky");
}

/// The rows probe's own size: a 448x240 target, so the rows' true coordinates at
/// `ScaledResolution { width: 427, height: 240 }` land one GUI unit per pixel —
/// hearts/food at `y = 240 − 39 = 201`, armour/air at `191`, the bar at `211`,
/// the level at `205`, the food row reaching `x = 304` (`view.rs`'s rows draws —
/// the render crate cannot import the client).
const ROWS_WIDE: u32 = 448;
/// The probe target's height in texels; the readback row stride (448 × 4) stays
/// a multiple of 256 bytes.
const ROWS_TALL: u32 = 240;

/// The offscreen colour target at the rows probe's size.
fn rows_target(device: &wgpu::Device, format: wgpu::TextureFormat) -> Target {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("oxide rows headless target"),
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

/// The depth texture at the rows probe's size.
fn rows_depth(device: &wgpu::Device) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("oxide rows headless depth"),
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

/// Reads the rows probe's target back: one 1792-byte row per line, 240 lines.
fn read_rows_pixels(device: &wgpu::Device, queue: &wgpu::Queue, target: &Target) -> Vec<u8> {
    const ROW: u32 = ROWS_WIDE * 4;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("oxide rows headless readback"),
        size: u64::from(ROW) * u64::from(ROWS_TALL),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide rows headless readback encoder"),
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

/// Asserts one rows probe pixel's RGB bytes exactly, naming it in the failure message.
fn expect_rows(pixels: &[u8], x: u32, y: u32, want: [u8; 3], what: &str) {
    let offset = (y * ROWS_WIDE * 4 + x * 4) as usize;
    let got = [pixels[offset], pixels[offset + 1], pixels[offset + 2]];
    assert_eq!(got, want, "{what} at ({x}, {y})");
}

/// Counts the rows probe's non-sky pixels: the RGB triple differs from [`SKY`].
///
/// Ten opaque 9-wide cells at spacing eight share nine columns, so ten fully
/// covered cells count 10 × 81 − 9 × 9 = 729.
fn rows_lit(pixels: &[u8]) -> usize {
    rows_lit_band(pixels, 0, 0, ROWS_WIDE, ROWS_TALL)
}

/// Counts the non-sky pixels inside the `(x, y, w, h)` band.
fn rows_lit_band(pixels: &[u8], x: u32, y: u32, w: u32, h: u32) -> usize {
    let mut lit = 0;
    for row in y..y + h {
        for col in x..x + w {
            let offset = (row * ROWS_WIDE * 4 + col * 4) as usize;
            if [pixels[offset], pixels[offset + 1], pixels[offset + 2]] != SKY {
                lit += 1;
            }
        }
    }
    lit
}

/// The synthetic rows sheet: `gui/icons` in miniature — one opaque band per slice
/// window the rows sample, each a colour no other window uses, over a transparent
/// sheet. The half windows (`61`, `79`, `97` and the armour `25`) ink only their
/// left five columns, mimicking the source sheet's transparent right halves, so a
/// half slice shows the draw beneath on its right.
///
/// Generated here; no asset store is read and no sheet pixel is copied.
fn rows_icon_sheet() -> Texture {
    const SIDE: u32 = 256;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    let rect = |rgba: &mut [u8], x0: u32, x1: u32, y0: u32, y1: u32, colour: [u8; 4]| {
        for y in y0..y1 {
            for x in x0..x1 {
                let at = ((y * SIDE + x) * 4) as usize;
                rgba[at..at + 4].copy_from_slice(&colour);
            }
        }
    };
    // Hearts row windows: the container, the blink container, the full, the half,
    // the flash pair and the poison pair.
    rect(&mut rgba, 16, 25, 0, 9, [128, 128, 128, 255]);
    rect(&mut rgba, 25, 34, 0, 9, [255, 255, 255, 255]);
    rect(&mut rgba, 52, 61, 0, 9, [255, 0, 0, 255]);
    rect(&mut rgba, 61, 66, 0, 9, [255, 0, 0, 255]);
    rect(&mut rgba, 70, 79, 0, 9, [255, 0, 255, 255]);
    rect(&mut rgba, 79, 84, 0, 9, [255, 0, 255, 255]);
    rect(&mut rgba, 88, 97, 0, 9, [0, 255, 0, 255]);
    rect(&mut rgba, 97, 102, 0, 9, [0, 255, 0, 255]);
    // Armour windows: the empty, the half and the full.
    rect(&mut rgba, 16, 25, 9, 18, [64, 64, 64, 255]);
    rect(&mut rgba, 25, 30, 9, 18, [70, 130, 180, 255]);
    rect(&mut rgba, 34, 43, 9, 18, [70, 130, 180, 255]);
    // Air windows: the full bubble and the popping one.
    rect(&mut rgba, 16, 25, 18, 27, [0, 255, 255, 255]);
    rect(&mut rgba, 25, 34, 18, 27, [0, 0, 255, 255]);
    // Food windows: the background, the full haunch and the half.
    rect(&mut rgba, 16, 25, 27, 36, [150, 75, 0, 255]);
    rect(&mut rgba, 52, 61, 27, 36, [255, 165, 0, 255]);
    rect(&mut rgba, 61, 66, 27, 36, [255, 165, 0, 255]);
    // Experience windows: the background slice and the fill.
    rect(&mut rgba, 0, 182, 64, 69, [0, 0, 139, 255]);
    rect(&mut rgba, 0, 183, 69, 74, [255, 255, 0, 255]);
    Texture {
        width: SIDE,
        height: SIDE,
        rgba,
    }
}

/// The synthetic rows font sheet: a 128x128 grid whose `1` and `2` cells ink only
/// their first column, so the level digits land on known pixels at advance two.
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn rows_font_sheet() -> Texture {
    const SIDE: u32 = 128;
    const CELL: u32 = 8;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    for code in ['1' as u32, '2' as u32] {
        let cell_x = (code % 16) * CELL;
        let cell_y = (code / 16) * CELL;
        for row in 0..CELL {
            let offset = (((cell_y + row) * SIDE + cell_x) * 4) as usize;
            rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    Texture {
        width: SIDE,
        height: SIDE,
        rgba,
    }
}

/// A hud pass ready for the rows' cases: the 448x240 target, the synthetic font and
/// rows sheet, and the 448x240 resolution so one GUI unit is one pixel.
fn rows_scene() -> (wgpu::Device, wgpu::Queue, Target, HudPass) {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = rows_target(&device, format);
    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, ROWS_WIDE as f32, ROWS_TALL as f32);
    hud.set_font(&device, &queue, &rows_font_sheet())
        .expect("the synthetic sheet is a 16x16 grid");
    hud.set_texture(&device, &queue, "gui/icons", &rows_icon_sheet());
    (device, queue, target, hud)
}

/// Renders the rows' draws over a cleared frame and reads the pixels back.
fn rows_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &Target,
    hud: &HudPass,
) -> Vec<u8> {
    let depth = rows_depth(device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide rows headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| hud.draw(pass));
    queue.submit(Some(encoder.finish()));
    read_rows_pixels(device, queue, target)
}

/// One 9x9 rows slice draw at `(x, y)` sampling the `(u, v)` icons window.
fn rows_slice(x: f32, y: f32, u: i32, v: i32) -> HudDraw {
    HudDraw::TexturedRect {
        texture: HudTexture::Named("gui/icons"),
        x,
        y,
        width: 9.0,
        height: 9.0,
        uv: [
            u as f32 / 256.0,
            v as f32 / 256.0,
            (u + 9) as f32 / 256.0,
            (v + 9) as f32 / 256.0,
        ],
        colour: [1.0, 1.0, 1.0, 1.0],
    }
}

/// The hud draws the hurt hearts at their true coordinates: 17.5 health against the
/// remembered nineteen with the blink running — the blink containers, the flash pair
/// (fulls over cells 0–8, the half over cell 9) and the current fulls over cells 0–8.
///
/// The geometry mirrors `view.rs`'s rows draws: the containers at `(122 + 8c, 201)`
/// with `u = 25`, the flash fulls at `u = 70`, the flash half at `(194, 201)`, the
/// current fulls at `u = 52`.
///
/// The pins: the current full's red over cell 0, the flash half's magenta on cell 9's
/// left, the blink container's white showing through its transparent right, the sky
/// above the row, and the 729 non-sky pixels of ten overlapping cells.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_the_hurt_hearts() {
    let (device, queue, target, mut hud) = rows_scene();
    let mut draws = Vec::new();
    for cell in 0..10 {
        draws.push(rows_slice(122.0 + cell as f32 * 8.0, 201.0, 25, 0));
    }
    for cell in 0..9 {
        let x = 122.0 + cell as f32 * 8.0;
        draws.push(rows_slice(x, 201.0, 70, 0));
        draws.push(rows_slice(x, 201.0, 52, 0));
    }
    draws.push(rows_slice(194.0, 201.0, 79, 0));
    hud.set_draws(
        &device,
        &queue,
        &draws,
        &TextureRegistry::new(&device, &queue),
    );
    let pixels = rows_frame(&device, &queue, &target, &hud);
    expect_rows(
        &pixels,
        126,
        205,
        [255, 0, 0],
        "the current full over cell 0",
    );
    expect_rows(
        &pixels,
        196,
        205,
        [255, 0, 255],
        "the flash half on cell 9's left",
    );
    expect_rows(
        &pixels,
        201,
        205,
        [255, 255, 255],
        "the blink container through its transparent right",
    );
    expect_rows(&pixels, 126, 200, SKY, "the sky above the row");
    assert_eq!(rows_lit(&pixels), 729, "ten overlapping cells");
}

/// The hud draws the poisoned hearts at their true coordinates: nineteen health under
/// the poison effect — the containers, the green fulls over cells 0–8 and the green
/// half over cell 9, whose transparent right shows the container.
///
/// The pins: the green full over cell 0, the green half on cell 9's left, the grey
/// container through its right, and the 729 non-sky pixels.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_the_poisoned_hearts() {
    let (device, queue, target, mut hud) = rows_scene();
    let mut draws = Vec::new();
    for cell in 0..10 {
        draws.push(rows_slice(122.0 + cell as f32 * 8.0, 201.0, 16, 0));
    }
    for cell in 0..9 {
        draws.push(rows_slice(122.0 + cell as f32 * 8.0, 201.0, 88, 0));
    }
    draws.push(rows_slice(194.0, 201.0, 97, 0));
    hud.set_draws(
        &device,
        &queue,
        &draws,
        &TextureRegistry::new(&device, &queue),
    );
    let pixels = rows_frame(&device, &queue, &target, &hud);
    expect_rows(
        &pixels,
        126,
        205,
        [0, 255, 0],
        "the poisoned full over cell 0",
    );
    expect_rows(
        &pixels,
        196,
        205,
        [0, 255, 0],
        "the poisoned half on cell 9's left",
    );
    expect_rows(
        &pixels,
        201,
        205,
        [128, 128, 128],
        "the container through its transparent right",
    );
    assert_eq!(rows_lit(&pixels), 729, "ten overlapping cells");
}

/// The hud draws the jittered food row at its true coordinates: six food at zero
/// saturation on the qualifying tick — cells 0–1 straight, cell 2 one pixel lower,
/// every cell's background under its haunch.
///
/// The pins: cell 0's orange mid and top row (unjittered), cell 2's orange mid one
/// lower, the sky above the shifted cell, and cell 3's bare saddle background.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_the_jittered_food() {
    let (device, queue, target, mut hud) = rows_scene();
    let mut draws = Vec::new();
    for cell in 0..10 {
        // Cell 2 jitters one lower: its background and its haunch both sit at 202.
        let y = if cell == 2 { 202.0 } else { 201.0 };
        draws.push(rows_slice(295.0 - cell as f32 * 8.0, y, 16, 27));
    }
    draws.push(rows_slice(295.0, 201.0, 52, 27));
    draws.push(rows_slice(287.0, 201.0, 52, 27));
    draws.push(rows_slice(279.0, 202.0, 52, 27));
    hud.set_draws(
        &device,
        &queue,
        &draws,
        &TextureRegistry::new(&device, &queue),
    );
    let pixels = rows_frame(&device, &queue, &target, &hud);
    expect_rows(&pixels, 299, 205, [255, 165, 0], "cell 0's full haunch");
    expect_rows(
        &pixels,
        299,
        201,
        [255, 165, 0],
        "cell 0's unshifted top row",
    );
    expect_rows(&pixels, 283, 206, [255, 165, 0], "cell 2's shifted haunch");
    expect_rows(&pixels, 283, 201, SKY, "the sky above the shifted cell");
    expect_rows(&pixels, 275, 205, [150, 75, 0], "cell 3's bare background");
}

/// The hud draws the mixed armour row at its true coordinates: thirteen points — six
/// full icons, the seventh halved (its transparent right over the sky) and the rest
/// empty.
///
/// The pins: the first full's steel, the halved icon's steel left, the sky through
/// its right, and the next icon's empty slate.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_the_mixed_armour() {
    let (device, queue, target, mut hud) = rows_scene();
    let mut draws = Vec::new();
    for cell in 0..6 {
        draws.push(rows_slice(122.0 + cell as f32 * 8.0, 191.0, 34, 9));
    }
    draws.push(rows_slice(170.0, 191.0, 25, 9));
    for cell in 7..10 {
        draws.push(rows_slice(122.0 + cell as f32 * 8.0, 191.0, 16, 9));
    }
    hud.set_draws(
        &device,
        &queue,
        &draws,
        &TextureRegistry::new(&device, &queue),
    );
    let pixels = rows_frame(&device, &queue, &target, &hud);
    expect_rows(&pixels, 126, 195, [70, 130, 180], "the first full icon");
    expect_rows(&pixels, 174, 195, [70, 130, 180], "the halved icon's left");
    expect_rows(
        &pixels,
        177,
        195,
        SKY,
        "the sky through its transparent right",
    );
    expect_rows(
        &pixels,
        182,
        195,
        [64, 64, 64],
        "the next icon's empty slate",
    );
}

/// The hud draws the draining air row at its true coordinates: ninety-one air
/// submerged — three full bubbles and the fading pair on the fourth, nothing
/// past it.
///
/// The pins: the first bubble's cyan, the fourth's popping blue, the sky where the
/// fifth would sit, and the sky above the row.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_the_draining_air() {
    let (device, queue, target, mut hud) = rows_scene();
    let draws = [
        rows_slice(295.0, 191.0, 16, 18),
        rows_slice(287.0, 191.0, 16, 18),
        rows_slice(279.0, 191.0, 16, 18),
        rows_slice(271.0, 191.0, 25, 18),
    ];
    hud.set_draws(
        &device,
        &queue,
        &draws,
        &TextureRegistry::new(&device, &queue),
    );
    let pixels = rows_frame(&device, &queue, &target, &hud);
    expect_rows(&pixels, 299, 195, [0, 255, 255], "the first full bubble");
    expect_rows(&pixels, 275, 195, [0, 0, 255], "the fading fourth bubble");
    expect_rows(&pixels, 267, 195, SKY, "no fifth bubble");
    expect_rows(&pixels, 295, 190, SKY, "the sky above the row");
}

/// The hud draws the experience bar and level at their true coordinates: the 182-wide
/// background, the 76-wide fill at 0.42, and the green `12` with its black outline.
///
/// The pins: the fill's yellow mid-row, the background's blue past the fill and at
/// its right edge, the main line's green under both digits, and the outline's black
/// beside and above the pen.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn the_hud_pass_draws_the_experience_bar() {
    let (device, queue, target, mut hud) = rows_scene();
    let tint = [1.0, 1.0, 1.0, 1.0];
    let green = [128.0 / 255.0, 1.0, 32.0 / 255.0, 1.0];
    let black = [0.0, 0.0, 0.0, 1.0];
    // The truncated fill at 0.42: `(int)(0.42 * 183) = 76`.
    let fill = 76.0;
    let mut draws = vec![
        HudDraw::TexturedRect {
            texture: HudTexture::Named("gui/icons"),
            x: 122.0,
            y: 211.0,
            width: 182.0,
            height: 5.0,
            uv: [0.0, 64.0 / 256.0, 182.0 / 256.0, 69.0 / 256.0],
            colour: tint,
        },
        HudDraw::TexturedRect {
            texture: HudTexture::Named("gui/icons"),
            x: 122.0,
            y: 211.0,
            width: fill,
            height: 5.0,
            uv: [0.0, 69.0 / 256.0, fill / 256.0, 74.0 / 256.0],
            colour: tint,
        },
    ];
    for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        draws.push(HudDraw::Text {
            text: "12".to_string(),
            x: 212.0 + dx,
            y: 205.0 + dy,
            scale: 1.0,
            colour: black,
            shadow: false,
            blend: false,
        });
    }
    draws.push(HudDraw::Text {
        text: "12".to_string(),
        x: 212.0,
        y: 205.0,
        scale: 1.0,
        colour: green,
        shadow: false,
        blend: false,
    });
    hud.set_draws(
        &device,
        &queue,
        &draws,
        &TextureRegistry::new(&device, &queue),
    );
    let pixels = rows_frame(&device, &queue, &target, &hud);
    expect_rows(&pixels, 150, 213, [255, 255, 0], "the fill's yellow");
    expect_rows(&pixels, 180, 213, [255, 255, 0], "the fill's mid yellow");
    expect_rows(
        &pixels,
        200,
        213,
        [0, 0, 139],
        "the background past the fill",
    );
    expect_rows(
        &pixels,
        303,
        213,
        [0, 0, 139],
        "the background's right edge",
    );
    expect_rows(&pixels, 212, 208, [128, 255, 32], "the main one's green");
    expect_rows(&pixels, 214, 208, [128, 255, 32], "the main two's green");
    expect_rows(&pixels, 213, 205, [0, 0, 0], "the outline beside the pen");
    expect_rows(&pixels, 212, 204, [0, 0, 0], "the outline above the pen");
}

/// The container sheet in miniature: the `t16/container` fixture for the screen
/// group's own cases — the sampled 176x166 window green left of panel x 88 and
/// blue right of it, a white strip along the bottom (`y >= 160`) and a yellow
/// strip along the right (`x >= 170`, over the white where they meet), so the
/// sheet's pins discriminate the blit's colour halves and both uv scales.
///
/// Generated here; no asset store is read and no sheet pixel is copied.
fn t16_container_sheet() -> Texture {
    const SIDE: u32 = 256;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    for y in 0..SIDE {
        for x in 0..SIDE {
            let at = ((y * SIDE + x) * 4) as usize;
            let texel = if x >= 170 {
                [255, 255, 0, 255]
            } else if y >= 160 {
                [255, 255, 255, 255]
            } else if x < 88 {
                [0, 255, 0, 255]
            } else {
                [0, 0, 255, 255]
            };
            rgba[at..at + 4].copy_from_slice(&texel);
        }
    }
    Texture {
        width: SIDE,
        height: SIDE,
        rgba,
    }
}

/// The screen group paints over the whole HUD group: a 176x166 container frame
/// at the 427x240 origin (125, 37) over an opaque red hud rect, with slot 0's
/// checker item, the hover highlight over empty slot 1 and the carried stack at
/// the (100, 50) pointer.
///
/// The geometry mirrors `view.rs`'s screen draws for the task's local two-slot
/// fixture — slots at panel (8, 18) and (26, 18) — which the render crate
/// cannot import: the sheet blit at the centred origin, slot cells 16x16, the
/// semi-white hover rect over the hovered cell, the carried stack at the
/// pointer minus 8. The background gradient and the title stay out of the
/// mirror — the client-side assembly pins own them — and the clear colour
/// stands in for the gradient.
///
/// The pins: the sheet's green/blue halves, its white bottom and yellow right
/// strips, the sheet's corner over the hud's red (the order proof — red shows
/// nowhere), the checker item's white and black cells over the sheet, the
/// hover's blend over the sheet's green, the carried flat-green stack over the
/// sky with the sky on two sides, and the sky past the frame.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t16_screen_frame_paints_over_the_hud() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = rows_target(&device, format);

    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, ROWS_WIDE as f32, ROWS_TALL as f32);
    hud.set_draws(
        &device,
        &queue,
        &[HudDraw::Rect {
            x: 125.0,
            y: 37.0,
            width: 176.0,
            height: 166.0,
            colour: [1.0, 0.0, 0.0, 1.0],
        }],
        &TextureRegistry::new(&device, &queue),
    );

    let mut screen = HudPass::new(&device, &queue, format);
    screen.set_resolution(&queue, ROWS_WIDE as f32, ROWS_TALL as f32);
    screen.set_texture(&device, &queue, "t16/container", &t16_container_sheet());
    screen
        .set_font(&device, &queue, &hotbar_font_sheet())
        .expect("the synthetic sheet is a 16x16 grid");
    screen.set_atlas_icon(&device, &queue, &item_atlas());
    screen.set_icon_source(
        &device,
        &queue,
        Arc::new(TestIcons {
            atlas: item_atlas(),
        }),
    );
    screen.set_draws(
        &device,
        &queue,
        &[
            HudDraw::TexturedRect {
                texture: HudTexture::Named("t16/container"),
                x: 125.0,
                y: 37.0,
                width: 176.0,
                height: 166.0,
                uv: [0.0, 0.0, 176.0 / 256.0, 166.0 / 256.0],
                colour: [1.0, 1.0, 1.0, 1.0],
            },
            HudDraw::Item {
                stack: Some(ItemIcon {
                    id: 2,
                    damage: 0,
                    enchanted: false,
                }),
                x: 133.0,
                y: 55.0,
                pop: 0.0,
            },
            HudDraw::Rect {
                x: 151.0,
                y: 55.0,
                width: 16.0,
                height: 16.0,
                colour: [1.0, 1.0, 1.0, 128.0 / 255.0],
            },
            HudDraw::Item {
                stack: Some(ItemIcon {
                    id: 3,
                    damage: 0,
                    enchanted: false,
                }),
                x: 92.0,
                y: 42.0,
                pop: 0.0,
            },
        ],
        &TextureRegistry::new(&device, &queue),
    );

    let depth = rows_depth(&device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide t16 screen headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    // The source's own order (`EntityRenderer.java`:1166-1170, then
    // `:1185-1191`): the hud group first, the screen group over it.
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| {
        hud.draw(pass);
        screen.draw(pass);
    });
    queue.submit(Some(encoder.finish()));

    let pixels = read_rows_pixels(&device, &queue, &target);
    // The sheet's halves and strips, through the 1:1 blit.
    expect_rows(&pixels, 135, 137, [0, 255, 0], "the sheet's green half");
    expect_rows(&pixels, 225, 137, [0, 0, 255], "the sheet's blue half");
    expect_rows(
        &pixels,
        135,
        199,
        [255, 255, 255],
        "the sheet's white strip",
    );
    expect_rows(&pixels, 295, 100, [255, 255, 0], "the sheet's yellow strip");
    // The order proof: the sheet's corner over the hud's red, and red shows
    // nowhere the sheet reaches.
    expect_rows(
        &pixels,
        125,
        37,
        [0, 255, 0],
        "the sheet over the hud's red",
    );
    expect_rows(
        &pixels,
        300,
        202,
        [255, 255, 0],
        "the yellow strip, no red past the edge",
    );
    // Slot 0's checker item over the sheet: the mesh's v-flip puts the
    // checker's bottom row on top, so the top-left cell is black, its right
    // neighbour white and the cell below it white.
    expect_rows(&pixels, 133, 55, [0, 0, 0], "the checker's black cell");
    expect_rows(
        &pixels,
        135,
        55,
        [255, 255, 255],
        "the checker's white cell",
    );
    expect_rows(
        &pixels,
        133,
        57,
        [255, 255, 255],
        "the checker's flipped row",
    );
    // The hover over empty slot 1: the semi-white blend over the sheet's
    // green.
    expect_rows(&pixels, 159, 63, [128, 255, 128], "the hover's blend");
    // The carried stack at the pointer minus 8 over the sky.
    expect_rows(&pixels, 92, 42, [0, 255, 0], "the carried stack's top-left");
    expect_rows(
        &pixels,
        107,
        57,
        [0, 255, 0],
        "the carried stack's bottom-right",
    );
    expect_rows(&pixels, 91, 42, SKY, "the sky left of the carried stack");
    expect_rows(&pixels, 92, 41, SKY, "the sky above the carried stack");
    expect_rows(&pixels, 400, 230, SKY, "the sky past the frame");
}

/// A right-drag's view side on the pixels: two covered slots drawing the
/// preview count of 1 with the white rect, and the carried stack drawing the
/// remnant count of 16 (18 carried minus the two placed).
///
/// The geometry mirrors the same fixture mid-drag: covered slots 0 and 1 draw
/// the semi-white rect, the cursor's own item and the preview count, while the
/// cursor draws the remnant. The pins: the preview one's ink and shadow over
/// both covered slots and the remnant sixteen's ink and shadow over the sky.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t16_screen_drag_preview_counts_the_remnant() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = rows_target(&device, format);

    let mut screen = HudPass::new(&device, &queue, format);
    screen.set_resolution(&queue, ROWS_WIDE as f32, ROWS_TALL as f32);
    screen.set_texture(&device, &queue, "t16/container", &t16_container_sheet());
    screen
        .set_font(&device, &queue, &hotbar_font_sheet())
        .expect("the synthetic sheet is a 16x16 grid");
    screen.set_atlas_icon(&device, &queue, &item_atlas());
    screen.set_icon_source(
        &device,
        &queue,
        Arc::new(TestIcons {
            atlas: item_atlas(),
        }),
    );
    let carried = ItemIcon {
        id: 3,
        damage: 0,
        enchanted: false,
    };
    screen.set_draws(
        &device,
        &queue,
        &[
            HudDraw::TexturedRect {
                texture: HudTexture::Named("t16/container"),
                x: 125.0,
                y: 37.0,
                width: 176.0,
                height: 166.0,
                uv: [0.0, 0.0, 176.0 / 256.0, 166.0 / 256.0],
                colour: [1.0, 1.0, 1.0, 1.0],
            },
            HudDraw::Rect {
                x: 133.0,
                y: 55.0,
                width: 16.0,
                height: 16.0,
                colour: [1.0, 1.0, 1.0, 128.0 / 255.0],
            },
            HudDraw::Item {
                stack: Some(carried),
                x: 133.0,
                y: 55.0,
                pop: 0.0,
            },
            HudDraw::Text {
                text: "1".to_string(),
                x: 148.0,
                y: 64.0,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: true,
                blend: false,
            },
            HudDraw::Rect {
                x: 151.0,
                y: 55.0,
                width: 16.0,
                height: 16.0,
                colour: [1.0, 1.0, 1.0, 128.0 / 255.0],
            },
            HudDraw::Item {
                stack: Some(carried),
                x: 151.0,
                y: 55.0,
                pop: 0.0,
            },
            HudDraw::Text {
                text: "1".to_string(),
                x: 166.0,
                y: 64.0,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: true,
                blend: false,
            },
            HudDraw::Item {
                stack: Some(carried),
                x: 92.0,
                y: 42.0,
                pop: 0.0,
            },
            HudDraw::Text {
                text: "16".to_string(),
                x: 105.0,
                y: 51.0,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: true,
                blend: false,
            },
        ],
        &TextureRegistry::new(&device, &queue),
    );

    let depth = rows_depth(&device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide t16 drag headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| {
        screen.draw(pass);
    });
    queue.submit(Some(encoder.finish()));

    let pixels = read_rows_pixels(&device, &queue, &target);
    // The covered slots' preview ones: white ink and quarter shadow over the
    // carried item's green.
    expect_rows(&pixels, 148, 64, TEXT, "slot 0's preview one");
    expect_rows(&pixels, 149, 65, SHADOW, "slot 0's preview shadow");
    expect_rows(&pixels, 166, 64, TEXT, "slot 1's preview one");
    expect_rows(&pixels, 167, 65, SHADOW, "slot 1's preview shadow");
    // The remnant sixteen at the carried stack: both digits' ink and shadow
    // over the sky.
    expect_rows(&pixels, 105, 51, TEXT, "the remnant one's ink");
    expect_rows(&pixels, 107, 51, TEXT, "the remnant six's ink");
    expect_rows(&pixels, 106, 52, SHADOW, "the remnant one's shadow");
    expect_rows(&pixels, 108, 52, SHADOW, "the remnant six's shadow");
    expect_rows(&pixels, 91, 51, SKY, "the sky left of the carried stack");
}

/// The blend of one RGBA rect colour over a backdrop: the pipeline's
/// `src_alpha` over `one_minus_src_alpha` pair, per channel.
fn blend_over(rgb: [f32; 3], alpha: f32, backdrop: [u8; 3]) -> [u8; 3] {
    [
        (rgb[0] * alpha + f32::from(backdrop[0]) * (1.0 - alpha)).round() as u8,
        (rgb[1] * alpha + f32::from(backdrop[1]) * (1.0 - alpha)).round() as u8,
        (rgb[2] * alpha + f32::from(backdrop[2]) * (1.0 - alpha)).round() as u8,
    ]
}

/// The tooltip's two-line box over the sky, then flipped past the right
/// edge: a two-line `||` tooltip at the (10, 20) cursor on the 448x240
/// screen, then the same box at the (440, 20) cursor.
///
/// The draws mirror `oxide-client/src/tooltip.rs`'s assembly draw for draw —
/// the render crate cannot import the client crate — for the lines `||`
/// (white name row) and `||` (grey `§7` second row): the `|` glyph inks its
/// first column only at two font pixels' advance, so each row is four font
/// pixels wide and the box is (22, 8, 4, 20).
///
/// The pins: the `0xF0100010` fill's blend inside the box, the `0x505000FF`
/// top edge's blend and the `0x5028007F` bottom edge's blend, the name row's
/// white ink, the second row's grey ink, the sky right of the box — then the
/// flipped frame's fill blend inside the moved box, its name ink, and the sky
/// where the unflipped box would have run off the screen.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t17_tooltip_paints_fill_border_lines_and_flip() {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = rows_target(&device, format);

    let mut hud = HudPass::new(&device, &queue, format);
    hud.set_resolution(&queue, ROWS_WIDE as f32, ROWS_TALL as f32);
    hud.set_font(&device, &queue, &overlay_font_sheet())
        .expect("the synthetic sheet is a 16x16 grid");

    // The assembly's literals as floats — the source's ARGB ints `0xF0100010`,
    // `0x505000FF`, `0x5028007F`.
    let fill = [16.0 / 255.0, 0.0, 16.0 / 255.0, 240.0 / 255.0];
    let top = [80.0 / 255.0, 0.0, 1.0, 80.0 / 255.0];
    let bottom = [40.0 / 255.0, 0.0, 127.0 / 255.0, 80.0 / 255.0];
    let grey = [170.0 / 255.0, 170.0 / 255.0, 170.0 / 255.0, 1.0];
    let white = [1.0, 1.0, 1.0, 1.0];
    let rect = |x: f32, y: f32, width: f32, height: f32, colour: [f32; 4]| HudDraw::Rect {
        x,
        y,
        width,
        height,
        colour,
    };
    let line = |body: &str, x: f32, y: f32, colour: [f32; 4]| HudDraw::Text {
        text: body.to_string(),
        x,
        y,
        scale: 1.0,
        colour,
        shadow: true,
        blend: false,
    };
    // One two-line box at (box_x, 8): the five fills, the six border steps
    // (the top edge in the top colour, the bottom edge in the bottom colour,
    // each side split halfway), then the white name row and the grey second
    // row — the draws the assembly emits, last over the screen.
    let box_draws = |box_x: f32| {
        vec![
            rect(box_x - 3.0, 4.0, 10.0, 1.0, fill),
            rect(box_x - 3.0, 31.0, 10.0, 1.0, fill),
            rect(box_x - 3.0, 5.0, 10.0, 26.0, fill),
            rect(box_x - 4.0, 5.0, 1.0, 26.0, fill),
            rect(box_x + 7.0, 5.0, 1.0, 26.0, fill),
            rect(box_x - 3.0, 6.0, 1.0, 12.0, top),
            rect(box_x - 3.0, 18.0, 1.0, 12.0, bottom),
            rect(box_x + 6.0, 6.0, 1.0, 12.0, top),
            rect(box_x + 6.0, 18.0, 1.0, 12.0, bottom),
            rect(box_x - 3.0, 5.0, 10.0, 1.0, top),
            rect(box_x - 3.0, 30.0, 10.0, 1.0, bottom),
            line("||", box_x, 8.0, white),
            line("||", box_x, 18.0, grey),
        ]
    };

    let skins = TextureRegistry::new(&device, &queue);
    let depth = rows_depth(&device);

    // Frame one: the cursor at (10, 20) puts the box at x = 22, y = 8.
    hud.set_draws(&device, &queue, &box_draws(22.0), &skins);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide t17 tooltip headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| hud.draw(pass));
    queue.submit(Some(encoder.finish()));

    let pixels = read_rows_pixels(&device, &queue, &target);
    // The border draws over the fill, so its rows blend twice: the fill over
    // the sky, then the edge over the fill.
    let fill_sky = blend_over([16.0, 0.0, 16.0], 240.0 / 255.0, SKY);
    let top_fill = blend_over([80.0, 0.0, 255.0], 80.0 / 255.0, fill_sky);
    let bottom_fill = blend_over([40.0, 0.0, 127.0], 80.0 / 255.0, fill_sky);
    expect_rows(&pixels, 20, 7, fill_sky, "the fill inside the box");
    expect_rows(&pixels, 24, 5, top_fill, "the top edge's row");
    expect_rows(&pixels, 24, 30, bottom_fill, "the bottom edge's row");
    expect_rows(&pixels, 22, 8, TEXT, "the name row's ink");
    expect_rows(
        &pixels,
        22,
        18,
        [170, 170, 170],
        "the second row's grey ink",
    );
    expect_rows(&pixels, 30, 10, SKY, "the sky right of the box");

    // Frame two: the cursor at (440, 20) flips the box to x = 400, y = 8.
    hud.set_draws(&device, &queue, &box_draws(400.0), &skins);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide t17 tooltip flip headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| hud.draw(pass));
    queue.submit(Some(encoder.finish()));

    let pixels = read_rows_pixels(&device, &queue, &target);
    expect_rows(&pixels, 398, 7, fill_sky, "the flipped fill inside the box");
    expect_rows(&pixels, 400, 8, TEXT, "the flipped name row's ink");
    expect_rows(
        &pixels,
        446,
        7,
        SKY,
        "the sky where the unflipped box would run off",
    );
}

/// Task 18's family-A sheet in miniature: the t16-style zoning — green left
/// of sheet x 88, blue right of it — with the caller's rect fills painted
/// over it, so each screen's property-source rects read in their own colour.
///
/// Generated here; no asset store is read and no sheet pixel is copied.
fn t18_family_sheet(fills: &[(u32, u32, u32, u32, [u8; 4])]) -> Texture {
    const SIDE: u32 = 256;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    for y in 0..SIDE {
        for x in 0..SIDE {
            let at = ((y * SIDE + x) * 4) as usize;
            let mut texel = if x < 88 {
                [0, 255, 0, 255]
            } else {
                [0, 0, 255, 255]
            };
            for (rx, ry, rw, rh, colour) in fills {
                if x >= *rx && x < rx + rw && y >= *ry && y < ry + rh {
                    texel = *colour;
                }
            }
            rgba[at..at + 4].copy_from_slice(&texel);
        }
    }
    Texture {
        width: SIDE,
        height: SIDE,
        rgba,
    }
}

/// One sheet-space blit as the screen pass reads it: the panel-dest rect and
/// the sheet-src origin, mirroring `view.rs`'s Task 18 background loop.
/// The six blit terms in order: the dest x/y, the width/height, the sheet
/// x/y. One array (not nine arguments) keeps the helper under the lint's
/// arity cap.
fn t18_blit(name: &'static str, gx: f32, gy: f32, r: [i32; 6]) -> HudDraw {
    let (dx, dy, w, h, sx, sy) = (r[0], r[1], r[2], r[3], r[4], r[5]);
    HudDraw::TexturedRect {
        texture: HudTexture::Named(name),
        x: gx + dx as f32,
        y: gy + dy as f32,
        width: w as f32,
        height: h as f32,
        uv: [
            sx as f32 / 256.0,
            sy as f32 / 256.0,
            (sx + w) as f32 / 256.0,
            (sy + h) as f32 / 256.0,
        ],
        colour: [1.0, 1.0, 1.0, 1.0],
    }
}

/// One functional slot's checker item (id 2, whose top-left cell is black),
/// mirroring the screen pass's per-slot item draw.
fn t18_slot_item(x: f32, y: f32) -> HudDraw {
    HudDraw::Item {
        stack: Some(ItemIcon {
            id: 2,
            damage: 0,
            enchanted: false,
        }),
        x,
        y,
        pop: 0.0,
    }
}

/// Task 18's family-A frame runner: one screen pass over the sky clear with
/// the named sheet and the given draws, through the item icon seam. The
/// background gradient and the title lines stay out of the mirror — the
/// client-side assembly pins own them, as in the t16 frame case.
fn t18_family_pixels(name: &'static str, sheet: &Texture, draws: &[HudDraw]) -> Vec<u8> {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = rows_target(&device, format);
    let mut screen = HudPass::new(&device, &queue, format);
    screen.set_resolution(&queue, ROWS_WIDE as f32, ROWS_TALL as f32);
    screen.set_texture(&device, &queue, name, sheet);
    screen.set_atlas_icon(&device, &queue, &item_atlas());
    screen.set_icon_source(
        &device,
        &queue,
        Arc::new(TestIcons {
            atlas: item_atlas(),
        }),
    );
    screen.set_draws(
        &device,
        &queue,
        draws,
        &TextureRegistry::new(&device, &queue),
    );
    let depth = rows_depth(&device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide t18 family headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| {
        screen.draw(pass);
    });
    queue.submit(Some(encoder.finish()));
    read_rows_pixels(&device, &queue, &target)
}

/// The six-row chest frame: the 176x222 panel at the centred origin
/// (136, 9), the split pair (the 125-tall upper slice, the 96-row bottom
/// blit from sheet row 126, here white), and a checker item in two chest
/// slots and two player slots.
///
/// The geometry mirrors `view.rs`'s screen draws for `CHEST_90`, which the
/// render crate cannot import. The pins: the upper slice's green/blue, the
/// white bottom on both sides of the split line, the four items' black
/// cells, and the sky past the frame.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t18_familya_chest_draws_both_blits_and_items() {
    const NAME: &str = "t18/chest";
    const GX: f32 = 136.0;
    const GY: f32 = 9.0;
    let sheet = t18_family_sheet(&[(0, 126, 256, 130, [255, 255, 255, 255])]);
    let pixels = t18_family_pixels(
        NAME,
        &sheet,
        &[
            t18_blit(NAME, GX, GY, [0, 0, 176, 125, 0, 0]),
            t18_blit(NAME, GX, GY, [0, 125, 176, 96, 0, 126]),
            t18_slot_item(GX + 8.0, GY + 18.0),
            t18_slot_item(GX + 152.0, GY + 108.0),
            t18_slot_item(GX + 8.0, GY + 139.0),
            t18_slot_item(GX + 152.0, GY + 197.0),
        ],
    );
    expect_rows(&pixels, 140, 20, [0, 255, 0], "the upper slice's green");
    expect_rows(&pixels, 230, 20, [0, 0, 255], "the upper slice's blue");
    expect_rows(
        &pixels,
        140,
        133,
        [0, 255, 0],
        "the green just above the split",
    );
    expect_rows(
        &pixels,
        140,
        135,
        [255, 255, 255],
        "the white just below the split",
    );
    expect_rows(&pixels, 140, 150, [255, 255, 255], "the bottom blit");
    expect_rows(&pixels, 144, 27, [0, 0, 0], "chest slot 0's item");
    expect_rows(&pixels, 288, 117, [0, 0, 0], "chest slot 53's item");
    expect_rows(&pixels, 144, 148, [0, 0, 0], "player slot 54's item");
    expect_rows(&pixels, 288, 206, [0, 0, 0], "player slot 89's item");
    expect_rows(&pixels, 320, 100, SKY, "the sky right of the panel");
    expect_rows(&pixels, 400, 230, SKY, "the sky past the frame");
}

/// The hopper frame: the 176x133 panel at the centred origin (136, 53) with
/// a checker item in each of the five hopper slots.
///
/// The pins: the sheet's green/blue, the five items' black cells, and the
/// sky past the frame.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t18_familya_hopper_draws_five_slots() {
    const NAME: &str = "t18/hopper";
    const GX: f32 = 136.0;
    const GY: f32 = 53.0;
    let sheet = t18_family_sheet(&[]);
    let pixels = t18_family_pixels(
        NAME,
        &sheet,
        &[
            t18_blit(NAME, GX, GY, [0, 0, 176, 133, 0, 0]),
            t18_slot_item(GX + 44.0, GY + 20.0),
            t18_slot_item(GX + 62.0, GY + 20.0),
            t18_slot_item(GX + 80.0, GY + 20.0),
            t18_slot_item(GX + 98.0, GY + 20.0),
            t18_slot_item(GX + 116.0, GY + 20.0),
        ],
    );
    expect_rows(&pixels, 140, 60, [0, 255, 0], "the sheet's green");
    expect_rows(&pixels, 260, 60, [0, 0, 255], "the sheet's blue");
    expect_rows(&pixels, 180, 73, [0, 0, 0], "hopper slot 0's item");
    expect_rows(&pixels, 198, 73, [0, 0, 0], "hopper slot 1's item");
    expect_rows(&pixels, 216, 73, [0, 0, 0], "hopper slot 2's item");
    expect_rows(&pixels, 234, 73, [0, 0, 0], "hopper slot 3's item");
    expect_rows(&pixels, 252, 73, [0, 0, 0], "hopper slot 4's item");
    expect_rows(&pixels, 140, 40, SKY, "the sky above the panel");
    expect_rows(&pixels, 400, 230, SKY, "the sky past the frame");
}

/// The dispenser frame: the 176x166 panel at the centred origin (136, 37)
/// with a checker item in each of the nine grid slots.
///
/// The pins: the sheet's green/blue, the grid corners' and centre's black
/// cells, and the sky past the frame.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t18_familya_dispenser_draws_nine_slots() {
    const NAME: &str = "t18/dispenser";
    const GX: f32 = 136.0;
    const GY: f32 = 37.0;
    let sheet = t18_family_sheet(&[]);
    let pixels = t18_family_pixels(
        NAME,
        &sheet,
        &[
            t18_blit(NAME, GX, GY, [0, 0, 176, 166, 0, 0]),
            t18_slot_item(GX + 62.0, GY + 17.0),
            t18_slot_item(GX + 80.0, GY + 17.0),
            t18_slot_item(GX + 98.0, GY + 17.0),
            t18_slot_item(GX + 62.0, GY + 35.0),
            t18_slot_item(GX + 80.0, GY + 35.0),
            t18_slot_item(GX + 98.0, GY + 35.0),
            t18_slot_item(GX + 62.0, GY + 53.0),
            t18_slot_item(GX + 80.0, GY + 53.0),
            t18_slot_item(GX + 98.0, GY + 53.0),
        ],
    );
    expect_rows(&pixels, 140, 45, [0, 255, 0], "the sheet's green");
    expect_rows(&pixels, 260, 45, [0, 0, 255], "the sheet's blue");
    expect_rows(&pixels, 198, 54, [0, 0, 0], "grid slot 0's item");
    expect_rows(&pixels, 234, 54, [0, 0, 0], "grid slot 2's item");
    expect_rows(&pixels, 216, 72, [0, 0, 0], "grid slot 4's item");
    expect_rows(&pixels, 198, 90, [0, 0, 0], "grid slot 6's item");
    expect_rows(&pixels, 234, 90, [0, 0, 0], "grid slot 8's item");
    expect_rows(&pixels, 400, 230, SKY, "the sky past the frame");
}

/// The furnace frame mid-burn (burn 100/200, cook 100/200): the 176x166
/// panel at (136, 37), the red flame 14x7 at (56, 42) from sheet (176, 6),
/// the magenta arrow 13x16 at (79, 34) from sheet (176, 14), and a checker
/// item in the input, fuel and output slots.
///
/// The pins: the flame's and arrow's pixels at the pinned sizes, the three
/// items' black cells, and the sky past the frame.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t18_familya_furnace_draws_flame_and_arrow() {
    const NAME: &str = "t18/furnace";
    const GX: f32 = 136.0;
    const GY: f32 = 37.0;
    let sheet = t18_family_sheet(&[
        (176, 0, 14, 13, [255, 0, 0, 255]),
        (176, 14, 25, 16, [255, 0, 255, 255]),
    ]);
    let pixels = t18_family_pixels(
        NAME,
        &sheet,
        &[
            t18_blit(NAME, GX, GY, [0, 0, 176, 166, 0, 0]),
            t18_blit(NAME, GX, GY, [56, 42, 14, 7, 176, 6]),
            t18_blit(NAME, GX, GY, [79, 34, 13, 16, 176, 14]),
            t18_slot_item(GX + 56.0, GY + 17.0),
            t18_slot_item(GX + 56.0, GY + 53.0),
            t18_slot_item(GX + 116.0, GY + 35.0),
        ],
    );
    expect_rows(&pixels, 194, 81, [255, 0, 0], "the flame's pixels");
    expect_rows(&pixels, 217, 73, [255, 0, 255], "the arrow's pixels");
    expect_rows(&pixels, 192, 54, [0, 0, 0], "the input's item");
    expect_rows(&pixels, 192, 90, [0, 0, 0], "the fuel's item");
    expect_rows(&pixels, 252, 72, [0, 0, 0], "the output's item");
    expect_rows(&pixels, 300, 45, [0, 0, 255], "the sheet's blue");
    expect_rows(&pixels, 400, 230, SKY, "the sky past the frame");
}

/// The brewing frame mid-brew (brewTime 200): the 176x166 panel at
/// (136, 37), the cyan fill 9x14 at (97, 16) from sheet (176, 0), the orange
/// bubble frame 12x20 at (65, 23) from sheet (185, 9), and a checker item in
/// the three potion slots and the ingredient slot.
///
/// The pins: the fill's and bubbles' pixels at the pinned sizes, the four
/// items' black cells, and the sky past the frame.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t18_familya_brewing_draws_fill_and_bubbles() {
    const NAME: &str = "t18/brewing";
    const GX: f32 = 136.0;
    const GY: f32 = 37.0;
    let sheet = t18_family_sheet(&[
        (176, 0, 9, 28, [0, 255, 255, 255]),
        (185, 0, 12, 29, [255, 128, 0, 255]),
    ]);
    let pixels = t18_family_pixels(
        NAME,
        &sheet,
        &[
            t18_blit(NAME, GX, GY, [0, 0, 176, 166, 0, 0]),
            t18_blit(NAME, GX, GY, [97, 16, 9, 14, 176, 0]),
            t18_blit(NAME, GX, GY, [65, 23, 12, 20, 185, 9]),
            t18_slot_item(GX + 56.0, GY + 46.0),
            t18_slot_item(GX + 79.0, GY + 53.0),
            t18_slot_item(GX + 102.0, GY + 46.0),
            t18_slot_item(GX + 79.0, GY + 17.0),
        ],
    );
    expect_rows(&pixels, 235, 55, [0, 255, 255], "the fill's pixels");
    expect_rows(&pixels, 203, 62, [255, 128, 0], "the bubbles' pixels");
    expect_rows(&pixels, 192, 83, [0, 0, 0], "potion slot 0's item");
    expect_rows(&pixels, 215, 90, [0, 0, 0], "potion slot 1's item");
    expect_rows(&pixels, 238, 83, [0, 0, 0], "potion slot 2's item");
    expect_rows(&pixels, 215, 54, [0, 0, 0], "the ingredient's item");
    expect_rows(&pixels, 400, 230, SKY, "the sky past the frame");
}

/// The crafting frame: the 176x166 panel at (136, 37) with a checker item
/// in the result slot and each of the nine grid slots.
///
/// The pins: the sheet's green/blue, the result's and three grid items'
/// black cells, and the sky past the frame.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t18_familya_crafting_draws_result_and_grid() {
    const NAME: &str = "t18/crafting";
    const GX: f32 = 136.0;
    const GY: f32 = 37.0;
    let sheet = t18_family_sheet(&[]);
    let pixels = t18_family_pixels(
        NAME,
        &sheet,
        &[
            t18_blit(NAME, GX, GY, [0, 0, 176, 166, 0, 0]),
            t18_slot_item(GX + 124.0, GY + 35.0),
            t18_slot_item(GX + 30.0, GY + 17.0),
            t18_slot_item(GX + 48.0, GY + 17.0),
            t18_slot_item(GX + 66.0, GY + 17.0),
            t18_slot_item(GX + 30.0, GY + 35.0),
            t18_slot_item(GX + 48.0, GY + 35.0),
            t18_slot_item(GX + 66.0, GY + 35.0),
            t18_slot_item(GX + 30.0, GY + 53.0),
            t18_slot_item(GX + 48.0, GY + 53.0),
            t18_slot_item(GX + 66.0, GY + 53.0),
        ],
    );
    expect_rows(&pixels, 140, 45, [0, 255, 0], "the sheet's green");
    expect_rows(&pixels, 260, 45, [0, 0, 255], "the sheet's blue");
    expect_rows(&pixels, 260, 72, [0, 0, 0], "the result's item");
    expect_rows(&pixels, 166, 54, [0, 0, 0], "grid slot 1's item");
    expect_rows(&pixels, 184, 72, [0, 0, 0], "grid slot 5's item");
    expect_rows(&pixels, 202, 90, [0, 0, 0], "grid slot 9's item");
    expect_rows(&pixels, 400, 230, SKY, "the sky past the frame");
}

/// Task 19's digit sheet: the `hotbar_font_sheet` construction extended to
/// every digit, so the enchanting costs ("1", "6") and the anvil's cost
/// ("41") and field ("11") ink. Each cell inks its first column only, at two
/// font pixels' advance — the pins read the ink, never the advance math.
fn t19_font_sheet() -> Texture {
    const SIDE: u32 = 128;
    const CELL: u32 = 8;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    for code in '0' as u32..='9' as u32 {
        let cell_x = (code % 16) * CELL;
        let cell_y = (code / 16) * CELL;
        for row in 0..CELL {
            let offset = (((cell_y + row) * SIDE + cell_x) * 4) as usize;
            rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    Texture {
        width: SIDE,
        height: SIDE,
        rgba,
    }
}

/// Task 19's family-B frame runner: `t18_family_pixels` with the digit sheet
/// bound, for the cases whose cost and field texts ink.
fn t19_family_pixels(name: &'static str, sheet: &Texture, draws: &[HudDraw]) -> Vec<u8> {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = rows_target(&device, format);
    let mut screen = HudPass::new(&device, &queue, format);
    screen.set_resolution(&queue, ROWS_WIDE as f32, ROWS_TALL as f32);
    screen.set_texture(&device, &queue, name, sheet);
    screen
        .set_font(&device, &queue, &t19_font_sheet())
        .expect("the digit sheet is a 16x16 grid");
    screen.set_atlas_icon(&device, &queue, &item_atlas());
    screen.set_icon_source(
        &device,
        &queue,
        Arc::new(TestIcons {
            atlas: item_atlas(),
        }),
    );
    screen.set_draws(
        &device,
        &queue,
        draws,
        &TextureRegistry::new(&device, &queue),
    );
    let depth = rows_depth(&device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide t19 family headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| {
        screen.draw(pass);
    });
    queue.submit(Some(encoder.finish()));
    read_rows_pixels(&device, &queue, &target)
}

/// The beacon frame at pyramid level 4 with speed chosen and the payment in:
/// the 230x219 panel at (109, 10), the selected row-0 speed button on the
/// +22 strip, its enabled neighbour and the lower rows on the idle strip,
/// the enabled confirm with its white icon and the cancel with its purple
/// one, and a checker item in the payment slot.
///
/// The pins: the selected strip's cyan against the idle strip's yellow, the
/// confirm's and cancel's icons, the payment item's black cell, the sheet's
/// blue past the panel's middle, and the sky past the frame. The potion icons
/// sample a second sheet the runner does not bind, so they stay out of the
/// mirror — the client's `potion_icon_uv` pins own them.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t19_familyb_beacon_draws_rows_and_confirm() {
    const NAME: &str = "t19/beacon";
    const GX: f32 = 109.0;
    const GY: f32 = 10.0;
    let sheet = t18_family_sheet(&[
        (0, 219, 22, 22, [255, 255, 0, 255]),
        (22, 219, 22, 22, [0, 255, 255, 255]),
        (44, 219, 22, 22, [255, 128, 0, 255]),
        (66, 219, 22, 22, [255, 0, 255, 255]),
        (90, 220, 18, 18, [255, 255, 255, 255]),
        (112, 220, 18, 18, [128, 0, 128, 255]),
    ]);
    let pixels = t19_family_pixels(
        NAME,
        &sheet,
        &[
            t18_blit(NAME, GX, GY, [0, 0, 230, 219, 0, 0]),
            t18_blit(NAME, GX, GY, [53, 22, 22, 22, 22, 219]),
            t18_blit(NAME, GX, GY, [77, 22, 22, 22, 0, 219]),
            t18_blit(NAME, GX, GY, [53, 47, 22, 22, 0, 219]),
            t18_blit(NAME, GX, GY, [65, 72, 22, 22, 0, 219]),
            t18_blit(NAME, GX, GY, [144, 47, 22, 22, 0, 219]),
            t18_blit(NAME, GX, GY, [168, 47, 22, 22, 0, 219]),
            t18_blit(NAME, GX, GY, [164, 107, 22, 22, 0, 219]),
            t18_blit(NAME, GX, GY, [166, 109, 18, 18, 90, 220]),
            t18_blit(NAME, GX, GY, [190, 107, 22, 22, 0, 219]),
            t18_blit(NAME, GX, GY, [192, 109, 18, 18, 112, 220]),
            t18_slot_item(GX + 136.0, GY + 110.0),
        ],
    );
    expect_rows(&pixels, 164, 34, [0, 255, 255], "the chosen row's cyan");
    expect_rows(
        &pixels,
        188,
        34,
        [255, 255, 0],
        "the neighbour row's yellow",
    );
    expect_rows(&pixels, 176, 84, [255, 255, 0], "the single third-tier row");
    expect_rows(&pixels, 255, 59, [255, 255, 0], "the regeneration row");
    expect_rows(&pixels, 273, 117, [255, 255, 0], "the confirm's idle strip");
    expect_rows(&pixels, 276, 120, [255, 255, 255], "the confirm's icon");
    expect_rows(&pixels, 301, 119, [128, 0, 128], "the cancel's icon");
    expect_rows(&pixels, 245, 120, [0, 0, 0], "the payment's item");
    expect_rows(&pixels, 309, 110, [0, 0, 255], "the sheet's blue");
    expect_rows(&pixels, 400, 230, SKY, "the sky past the frame");
}

/// The enchanting frame with costs [1, 6, 0], one lapis and level 6: the
/// 176x166 panel at (136, 37), row 0 on the idle strip with its cyan clasp
/// and green "1", row 1 on the empty strip with its magenta dim clasp and dim
/// "6", row 2 empty, the book's brown cover with both cream pages open, and
/// checker items in the item and lapis slots.
///
/// The pins: the two clasps, the affordable green against the dim cost, the
/// cover against the pages, the two items' black cells, and the sky past the
/// frame. The glyph runs stay out of the mirror — the client's
/// `glyph_word_at` pins own them, as the titles stay out per the t16 case.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t19_familyb_enchanting_draws_book_and_offer() {
    const NAME: &str = "t19/enchanting";
    const GX: f32 = 136.0;
    const GY: f32 = 37.0;
    let sheet = t18_family_sheet(&[
        (0, 166, 108, 19, [255, 128, 0, 255]),
        (0, 185, 108, 19, [255, 0, 0, 255]),
        (0, 223, 16, 16, [0, 255, 255, 255]),
        (16, 239, 16, 16, [255, 0, 255, 255]),
    ]);
    let pixels = t19_family_pixels(
        NAME,
        &sheet,
        &[
            t18_blit(NAME, GX, GY, [0, 0, 176, 166, 0, 0]),
            t18_blit(NAME, GX, GY, [60, 14, 108, 19, 0, 166]),
            t18_blit(NAME, GX, GY, [61, 15, 16, 16, 0, 223]),
            t18_blit(NAME, GX, GY, [60, 33, 108, 19, 0, 185]),
            t18_blit(NAME, GX, GY, [61, 34, 16, 16, 16, 239]),
            t18_blit(NAME, GX, GY, [60, 52, 108, 19, 0, 185]),
            HudDraw::Text {
                text: "1".to_string(),
                x: GX + 164.0,
                y: GY + 23.0,
                scale: 1.0,
                colour: [128.0 / 255.0, 1.0, 32.0 / 255.0, 1.0],
                shadow: true,
                blend: false,
            },
            HudDraw::Text {
                text: "6".to_string(),
                x: GX + 164.0,
                y: GY + 42.0,
                scale: 1.0,
                colour: [64.0 / 255.0, 127.0 / 255.0, 16.0 / 255.0, 1.0],
                shadow: true,
                blend: false,
            },
            HudDraw::Rect {
                x: GX + 62.0,
                y: GY + 2.0,
                width: 52.0,
                height: 10.0,
                colour: [107.0 / 255.0, 74.0 / 255.0, 53.0 / 255.0, 1.0],
            },
            HudDraw::Rect {
                x: GX + 64.0,
                y: GY + 2.0,
                width: 24.0,
                height: 10.0,
                colour: [216.0 / 255.0, 207.0 / 255.0, 168.0 / 255.0, 1.0],
            },
            HudDraw::Rect {
                x: GX + 88.0,
                y: GY + 2.0,
                width: 24.0,
                height: 10.0,
                colour: [216.0 / 255.0, 207.0 / 255.0, 168.0 / 255.0, 1.0],
            },
            t18_slot_item(GX + 15.0, GY + 47.0),
            t18_slot_item(GX + 35.0, GY + 47.0),
        ],
    );
    expect_rows(&pixels, 236, 57, [255, 128, 0], "the idle row's strip");
    expect_rows(&pixels, 198, 53, [0, 255, 255], "the affordable clasp");
    expect_rows(&pixels, 300, 60, [128, 255, 32], "the affordable one's ink");
    expect_rows(&pixels, 236, 77, [255, 0, 0], "the dim row's strip");
    expect_rows(&pixels, 198, 72, [255, 0, 255], "the dim clasp");
    expect_rows(&pixels, 300, 79, [64, 127, 16], "the dim six's ink");
    expect_rows(&pixels, 249, 40, [107, 74, 53], "the book's cover");
    expect_rows(&pixels, 206, 42, [216, 207, 168], "the book's left page");
    expect_rows(&pixels, 236, 42, [216, 207, 168], "the book's right page");
    expect_rows(&pixels, 151, 84, [0, 0, 0], "the item's icon");
    expect_rows(&pixels, 171, 84, [0, 0, 0], "the lapis's icon");
    expect_rows(&pixels, 400, 230, SKY, "the sky past the frame");
}

/// The villager frame on the first of two enabled recipes: the 176x166 panel
/// at (136, 37), the enabled next button cyan and the disabled previous
/// orange, the baked arrow lane magenta, and checker items for the buy and
/// the sell.
///
/// The pins: the two pager states, the arrow lane, both recipe items, the
/// sheet's green past the buttons, and the sky past the frame. The red X
/// stays out — the shown recipe is enabled — and the client's pager tests pin
/// the disabled draw's flag.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t19_familyb_villager_draws_recipe_and_pager() {
    const NAME: &str = "t19/villager";
    const GX: f32 = 136.0;
    const GY: f32 = 37.0;
    let sheet = t18_family_sheet(&[
        (176, 0, 12, 19, [0, 255, 255, 255]),
        (200, 19, 12, 19, [255, 128, 0, 255]),
        (84, 28, 32, 8, [255, 0, 255, 255]),
    ]);
    let pixels = t19_family_pixels(
        NAME,
        &sheet,
        &[
            t18_blit(NAME, GX, GY, [0, 0, 176, 166, 0, 0]),
            t18_blit(NAME, GX, GY, [147, 23, 12, 19, 176, 0]),
            t18_blit(NAME, GX, GY, [17, 23, 12, 19, 200, 19]),
            t18_slot_item(GX + 36.0, GY + 24.0),
            t18_slot_item(GX + 120.0, GY + 24.0),
        ],
    );
    expect_rows(&pixels, 285, 62, [0, 255, 255], "the next button's cyan");
    expect_rows(
        &pixels,
        155,
        62,
        [255, 128, 0],
        "the previous button's orange",
    );
    expect_rows(&pixels, 226, 67, [255, 0, 255], "the arrow lane");
    expect_rows(&pixels, 172, 61, [0, 0, 0], "the buy's item");
    expect_rows(&pixels, 256, 61, [0, 0, 0], "the sell's item");
    expect_rows(&pixels, 140, 45, [0, 255, 0], "the sheet's green");
    expect_rows(&pixels, 400, 230, SKY, "the sky past the frame");
}

/// The chested horse frame: the 176x166 panel at (136, 37), the cyan chest
/// block and the magenta armour frame, a checker item in the saddle slot and
/// one in the first chest slot.
///
/// The pins: the chest block beside its item, the armour frame past the
/// saddle, both items' black cells, and the sky past the frame. The live
/// preview stays out — it is Task 20's `drawEntityOnScreen` — and the port
/// carries only its anchor.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t19_familyb_horse_draws_chested_layout() {
    const NAME: &str = "t19/horse";
    const GX: f32 = 136.0;
    const GY: f32 = 37.0;
    let sheet = t18_family_sheet(&[
        (0, 166, 90, 54, [0, 255, 255, 255]),
        (0, 220, 18, 18, [255, 0, 255, 255]),
    ]);
    let pixels = t19_family_pixels(
        NAME,
        &sheet,
        &[
            t18_blit(NAME, GX, GY, [0, 0, 176, 166, 0, 0]),
            t18_blit(NAME, GX, GY, [79, 17, 90, 54, 0, 166]),
            t18_blit(NAME, GX, GY, [7, 35, 18, 18, 0, 220]),
            t18_slot_item(GX + 8.0, GY + 18.0),
            t18_slot_item(GX + 80.0, GY + 18.0),
        ],
    );
    expect_rows(&pixels, 215, 54, [0, 255, 255], "the chest block's cyan");
    expect_rows(
        &pixels,
        143,
        72,
        [255, 0, 255],
        "the armour frame's magenta",
    );
    expect_rows(&pixels, 144, 55, [0, 0, 0], "the saddle's item");
    expect_rows(&pixels, 216, 55, [0, 0, 0], "the chest slot's item");
    expect_rows(&pixels, 400, 230, SKY, "the sky past the frame");
}

/// The anvil frame with both inputs, no output and cost 41: the 176x166 panel
/// at (136, 37), the cyan field strip, the magenta broken arrow, the red "41"
/// cost, the grey "11" field with its grey cursor, and a checker item in the
/// first input slot.
///
/// The pins: the strip, the arrow, the red cost's ink, the field's ink and
/// cursor, the input's black cell, and the sky past the frame.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t19_familyb_anvil_draws_cost_and_field() {
    const NAME: &str = "t19/anvil";
    const GX: f32 = 136.0;
    const GY: f32 = 37.0;
    let sheet = t18_family_sheet(&[
        (0, 166, 110, 16, [0, 255, 255, 255]),
        (176, 0, 28, 21, [255, 0, 255, 255]),
    ]);
    let pixels = t19_family_pixels(
        NAME,
        &sheet,
        &[
            t18_blit(NAME, GX, GY, [0, 0, 176, 166, 0, 0]),
            t18_blit(NAME, GX, GY, [59, 20, 110, 16, 0, 166]),
            t18_blit(NAME, GX, GY, [99, 45, 28, 21, 176, 0]),
            HudDraw::Text {
                text: "41".to_string(),
                x: GX + 164.0,
                y: GY + 67.0,
                scale: 1.0,
                colour: [1.0, 96.0 / 255.0, 96.0 / 255.0, 1.0],
                shadow: true,
                blend: false,
            },
            HudDraw::Text {
                text: "11".to_string(),
                x: GX + 62.0,
                y: GY + 24.0,
                scale: 1.0,
                colour: [224.0 / 255.0, 224.0 / 255.0, 224.0 / 255.0, 1.0],
                shadow: false,
                blend: false,
            },
            HudDraw::Rect {
                x: GX + 71.0,
                y: GY + 25.0,
                width: 1.0,
                height: 9.0,
                colour: [208.0 / 255.0, 208.0 / 255.0, 208.0 / 255.0, 1.0],
            },
            t18_slot_item(GX + 27.0, GY + 47.0),
        ],
    );
    expect_rows(&pixels, 195, 57, [0, 255, 255], "the field strip's cyan");
    expect_rows(
        &pixels,
        236,
        83,
        [255, 0, 255],
        "the broken arrow's magenta",
    );
    expect_rows(&pixels, 300, 104, [255, 96, 96], "the red cost's ink");
    expect_rows(&pixels, 198, 61, [224, 224, 224], "the field's ink");
    expect_rows(&pixels, 207, 62, [208, 208, 208], "the field's cursor");
    expect_rows(&pixels, 163, 84, [0, 0, 0], "the input's item");
    expect_rows(&pixels, 400, 230, SKY, "the sky past the frame");
}

// ---------------------------------------------------------------------------------------
// Task 20's inventory frame: the sheet, one grid item, the preview silhouette and two
// effect rows.
// ---------------------------------------------------------------------------------------

/// Task 20's font sheet: the digit sheet's construction extended to the case's
/// effect-row glyphs, so the names ("Speed II", "Strength") and the durations
/// ("3:00", "1:00") ink. Each named cell inks its first column only, at two
/// font pixels' advance — the pins read the first glyph's ink, never the
/// advance math.
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn t20_font_sheet() -> Texture {
    const SIDE: u32 = 128;
    const CELL: u32 = 8;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    for code in [
        'S', 'p', 'e', 'd', 'I', 't', 'r', 'n', 'g', 'h', ':', '0', '1', '3',
    ] {
        let code = code as u32;
        let cell_x = (code % 16) * CELL;
        let cell_y = (code / 16) * CELL;
        for row in 0..CELL {
            let offset = (((cell_y + row) * SIDE + cell_x) * 4) as usize;
            rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    Texture {
        width: SIDE,
        height: SIDE,
        rgba,
    }
}

/// Task 20's frame runner: one screen pass over the sky clear with the named
/// sheet, the lettered font and the given draws, through the item icon seam.
/// The runner owns the preview draw — the 16x16 skin-face silhouette centred
/// on the pinned anchor, mirroring `view.rs`'s inventory preview — so the
/// case reads the sheet, the item and the effect rows; the mutation probe
/// shifts the runner's rect.
///
/// The skin uploads flat under `uuid`, so the silhouette's pixels are the
/// skin's own colour: the source draws the preview without world lighting,
/// which the flat sample honours by construction.
fn t20_inventory_pixels(
    name: &'static str,
    sheet: &Texture,
    skin: [u8; 4],
    uuid: &str,
    draws: &[HudDraw],
) -> Vec<u8> {
    const ANCHOR_X: f32 = 247.0;
    const ANCHOR_Y: f32 = 112.0;
    const FACE: f32 = 16.0;
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = rows_target(&device, format);
    let mut screen = HudPass::new(&device, &queue, format);
    screen.set_resolution(&queue, ROWS_WIDE as f32, ROWS_TALL as f32);
    screen.set_texture(&device, &queue, name, sheet);
    screen
        .set_font(&device, &queue, &t20_font_sheet())
        .expect("the lettered sheet is a 16x16 grid");
    screen.set_atlas_icon(&device, &queue, &item_atlas());
    screen.set_icon_source(
        &device,
        &queue,
        Arc::new(TestIcons {
            atlas: item_atlas(),
        }),
    );
    let mut skins = TextureRegistry::new(&device, &queue);
    skins.set_skin(&device, &queue, uuid, Some(&flat_sheet(skin)), None);
    let preview = skins.resolve(uuid, false).id();
    let mut all = Vec::with_capacity(draws.len() + 2);
    all.extend_from_slice(&draws[..1]);
    all.push(HudDraw::SkinRect {
        texture: preview,
        x: ANCHOR_X - FACE / 2.0,
        y: ANCHOR_Y - FACE / 2.0,
        width: FACE,
        height: FACE,
        uv: [8.0 / 64.0, 8.0 / 64.0, 16.0 / 64.0, 16.0 / 64.0],
        colour: [1.0, 1.0, 1.0, 1.0],
    });
    all.push(HudDraw::SkinRect {
        texture: preview,
        x: ANCHOR_X - FACE / 2.0,
        y: ANCHOR_Y - FACE / 2.0,
        width: FACE,
        height: FACE,
        uv: [40.0 / 64.0, 8.0 / 64.0, 48.0 / 64.0, 16.0 / 64.0],
        colour: [1.0, 1.0, 1.0, 1.0],
    });
    all.extend_from_slice(&draws[1..]);
    screen.set_draws(&device, &queue, &all, &skins);
    let depth = rows_depth(&device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide t20 inventory headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| {
        screen.draw(pass);
    });
    queue.submit(Some(encoder.finish()));
    read_rows_pixels(&device, &queue, &target)
}

/// The populated inventory frame: the shifted 176x166 panel at (196, 37) —
/// `160 + (448 - 176 - 200) / 2` while effects are non-empty — a checker
/// item in grid slot 1, the skin-face preview centred on the (51, 75)
/// anchor, and two effect rows (Speed II for 3:00, Strength for 1:00).
///
/// The geometry mirrors `view.rs`'s inventory draws, which the render crate
/// cannot import. The pins: the sheet's green/blue, the grid item's black
/// cell, the anchor's skin colour, each row's cyan, each icon's colour and
/// each name/duration's ink, and the sky past the frame.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t20_inventory_draws_sheet_item_preview_and_effects() {
    const NAME: &str = "t20/inventory";
    const GX: f32 = 196.0;
    const GY: f32 = 37.0;
    const SKIN: [u8; 4] = [200, 90, 40, 255];
    const UUID: &str = "00000000-0000-0000-0000-000000000020";
    let sheet = t18_family_sheet(&[
        (0, 166, 140, 32, [0, 255, 255, 255]),
        (0, 198, 18, 18, [255, 0, 255, 255]),
        (72, 198, 18, 18, [255, 255, 0, 255]),
    ]);
    let pixels = t20_inventory_pixels(
        NAME,
        &sheet,
        SKIN,
        UUID,
        &[
            t18_blit(NAME, GX, GY, [0, 0, 176, 166, 0, 0]),
            t18_slot_item(GX + 88.0, GY + 26.0),
            t18_blit(NAME, GX, GY, [-124, 0, 140, 32, 0, 166]),
            t18_blit(NAME, GX, GY, [-118, 7, 18, 18, 0, 198]),
            HudDraw::Text {
                text: "Speed II".to_string(),
                x: GX - 124.0 + 28.0,
                y: GY + 6.0,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: true,
                blend: false,
            },
            HudDraw::Text {
                text: "3:00".to_string(),
                x: GX - 124.0 + 28.0,
                y: GY + 16.0,
                scale: 1.0,
                colour: [127.0 / 255.0, 127.0 / 255.0, 127.0 / 255.0, 1.0],
                shadow: true,
                blend: false,
            },
            t18_blit(NAME, GX, GY, [-124, 33, 140, 32, 0, 166]),
            t18_blit(NAME, GX, GY, [-118, 40, 18, 18, 72, 198]),
            HudDraw::Text {
                text: "Strength".to_string(),
                x: GX - 124.0 + 28.0,
                y: GY + 39.0,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: true,
                blend: false,
            },
            HudDraw::Text {
                text: "1:00".to_string(),
                x: GX - 124.0 + 28.0,
                y: GY + 49.0,
                scale: 1.0,
                colour: [127.0 / 255.0, 127.0 / 255.0, 127.0 / 255.0, 1.0],
                shadow: true,
                blend: false,
            },
        ],
    );
    expect_rows(&pixels, 250, 150, [0, 255, 0], "the sheet's green");
    expect_rows(&pixels, 300, 150, [0, 0, 255], "the sheet's blue");
    expect_rows(&pixels, 284, 63, [0, 0, 0], "grid slot 1's item");
    expect_rows(&pixels, 247, 112, [200, 90, 40], "the preview's skin");
    expect_rows(&pixels, 200, 40, [0, 255, 255], "the first row's cyan");
    expect_rows(&pixels, 78, 44, [255, 0, 255], "the Speed icon's magenta");
    expect_rows(&pixels, 100, 43, [255, 255, 255], "the Speed name's ink");
    expect_rows(&pixels, 100, 53, [127, 127, 127], "the 3:00 ink");
    expect_rows(&pixels, 78, 77, [255, 255, 0], "the Strength icon's yellow");
    expect_rows(&pixels, 100, 76, [255, 255, 255], "the Strength name's ink");
    expect_rows(&pixels, 100, 86, [127, 127, 127], "the 1:00 ink");
    expect_rows(&pixels, 400, 230, SKY, "the sky past the frame");
}

/// Task 21's frame runner: one screen pass over the sky clear with the panel
/// and strip sheets, the lettered font and the given draws, through the item
/// icon seam. The runner owns both sheet names — the panel and the strip —
/// so the case reads the strip, the panel, an item, the thumb, the search
/// ink and the delete cell; the mutation probe shifts the runner's panel
/// rect.
fn t21_creative_pixels(
    panel_name: &'static str,
    panel: &Texture,
    strip_name: &'static str,
    strip: &Texture,
    draws: &[HudDraw],
) -> Vec<u8> {
    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = rows_target(&device, format);
    let mut screen = HudPass::new(&device, &queue, format);
    screen.set_resolution(&queue, ROWS_WIDE as f32, ROWS_TALL as f32);
    screen.set_texture(&device, &queue, panel_name, panel);
    screen.set_texture(&device, &queue, strip_name, strip);
    screen
        .set_font(&device, &queue, &t20_font_sheet())
        .expect("the lettered sheet is a 16x16 grid");
    screen.set_atlas_icon(&device, &queue, &item_atlas());
    screen.set_icon_source(
        &device,
        &queue,
        Arc::new(TestIcons {
            atlas: item_atlas(),
        }),
    );
    screen.set_draws(
        &device,
        &queue,
        draws,
        &TextureRegistry::new(&device, &queue),
    );
    let depth = rows_depth(&device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide t21 creative headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| {
        screen.draw(pass);
    });
    queue.submit(Some(encoder.finish()));
    read_rows_pixels(&device, &queue, &target)
}

/// The creative frame on the search tab: the eleven unselected strip tabs,
/// the 195x136 panel, the selected tab last, the twelve tab icons, the 45
/// page cells with the hotbar row, the enabled thumb, the search text and
/// the delete cell's panel pixel.
///
/// The geometry mirrors `view.rs`'s creative draws at the centred origin
/// (126, 52) with the search tab selected, which the render crate cannot
/// import. The pins: an unselected tab's red, the selected tab's yellow past
/// its icon, the panel's green/blue, a grid item's black cell, the thumb's
/// white, the search ink, the delete cell's magenta and the sky past the
/// frame.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t21_creative_draws_strip_grid_thumb_search_and_bin() {
    const PANEL: &str = "t21/tab_search";
    const STRIP: &str = "t21/tabs";
    const GX: f32 = 126.0;
    const GY: f32 = 52.0;
    const SEARCH: i32 = 5;
    let panel = t18_family_sheet(&[(173, 112, 16, 16, [255, 0, 255, 255])]);
    let strip = t18_family_sheet(&[
        (0, 0, 168, 32, [255, 0, 0, 255]),
        (0, 32, 168, 32, [255, 255, 0, 255]),
        (232, 0, 12, 15, [255, 255, 255, 255]),
    ]);
    let mut draws = Vec::new();
    // The strip: every unselected tab first — mirroring `tab_sprite` (the
    // col-5 nudge to 167, the +col step, the -28/+132 sprite rows) and
    // `tab_uv` (u = col·28, v = 0/64 by row, +32 when selected).
    for index in 0..12 {
        if index == SEARCH {
            continue;
        }
        let col = index % 6;
        let x = if col == 5 {
            167
        } else if col > 0 {
            28 * col + col
        } else {
            0
        };
        let (dy, v) = if index < 6 { (-28, 0) } else { (132, 64) };
        draws.push(t18_blit(STRIP, GX, GY, [x, dy, 28, 32, 28 * col, v]));
    }
    // The panel second, the selected tab last.
    draws.push(t18_blit(PANEL, GX, GY, [0, 0, 195, 136, 0, 0]));
    draws.push(t18_blit(STRIP, GX, GY, [167, -28, 28, 32, 140, 32]));
    // The twelve tab icons over the strip, then the page cells with the
    // hotbar row — checker items throughout.
    for index in 0..12 {
        let col = index % 6;
        let x = if col == 5 {
            167
        } else if col > 0 {
            28 * col + col
        } else {
            0
        };
        let iy = if index < 6 { -19 } else { 139 };
        draws.push(t18_slot_item(GX + (x + 6) as f32, GY + iy as f32));
    }
    for cell in 0..45 {
        draws.push(t18_slot_item(
            GX + (9 + (cell % 9) * 18) as f32,
            GY + (18 + (cell / 9) * 18) as f32,
        ));
    }
    for hotbar in 0..9 {
        draws.push(t18_slot_item(GX + (9 + hotbar * 18) as f32, GY + 112.0));
    }
    // The enabled thumb and the search text.
    draws.push(t18_blit(STRIP, GX, GY, [175, 18, 12, 15, 232, 0]));
    draws.push(HudDraw::Text {
        text: "Speed".to_string(),
        x: GX + 82.0,
        y: GY + 6.0,
        scale: 1.0,
        colour: [1.0, 1.0, 1.0, 1.0],
        shadow: false,
        blend: false,
    });
    let pixels = t21_creative_pixels(PANEL, &panel, STRIP, &strip, &draws);
    expect_rows(&pixels, 150, 36, [255, 0, 0], "unselected tab 0's red");
    expect_rows(&pixels, 296, 36, [255, 255, 0], "the selected tab's yellow");
    expect_rows(&pixels, 150, 150, [0, 255, 0], "the panel's green");
    expect_rows(&pixels, 300, 150, [0, 0, 255], "the panel's blue");
    expect_rows(&pixels, 135, 70, [0, 0, 0], "grid cell 0's item");
    expect_rows(&pixels, 307, 77, [255, 255, 255], "the thumb's white");
    expect_rows(&pixels, 208, 58, [255, 255, 255], "the search ink");
    expect_rows(
        &pixels,
        307,
        172,
        [255, 0, 255],
        "the delete cell's magenta",
    );
    expect_rows(&pixels, 400, 230, SKY, "the sky past the frame");
}

/// Task 22's editor case: the editor's content through the real screen pass — the board
/// backing in its brown, the white title, and the editing line with the cursor wrap
/// (`"> || <"`): the `>`/`<` brackets are the cursor, glyph pixels on both sides.
/// Positions are measured with the same width law the pass lays out with, so the pens
/// agree by construction; the asserts pin the pixels.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t22_sign_editor_draws_text_and_cursor() {
    const WIDE: f32 = 448.0;
    const TALL: f32 = 240.0;

    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = rows_target(&device, format);
    let depth = rows_depth(&device);
    let sheet = t22_font_sheet();
    let font = Font::load(&sheet, None).expect("the synthetic sheet is a 16x16 grid");
    let title = "Edit sign message";
    let line = "> || <";
    // The editor's own centring (`draws`): title at y 40, the first line at y 80.
    let title_x = WIDE / 2.0 - f64::from(string_width(&font, title)) as f32 / 2.0;
    let line_x = WIDE / 2.0 - f64::from(string_width(&font, line)) as f32 / 2.0;
    let mut screen = HudPass::new(&device, &queue, format);
    screen.set_resolution(&queue, WIDE, TALL);
    screen
        .set_font(&device, &queue, &sheet)
        .expect("the synthetic sheet is a 16x16 grid");
    screen.set_draws(
        &device,
        &queue,
        &[
            // The board backing (the editor's brown), the title and the editing line.
            HudDraw::Rect {
                x: WIDE / 2.0 - 53.0,
                y: 76.0,
                width: 106.0,
                height: 46.0,
                colour: [0.55, 0.42, 0.28, 1.0],
            },
            HudDraw::Text {
                text: title.to_string(),
                x: title_x,
                y: 40.0,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: true,
                blend: true,
            },
            HudDraw::Text {
                text: line.to_string(),
                x: line_x,
                y: 80.0,
                scale: 1.0,
                colour: [0.0, 0.0, 0.0, 1.0],
                shadow: false,
                blend: true,
            },
        ],
        &TextureRegistry::new(&device, &queue),
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide t22 editor headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| {
        screen.draw(pass);
    });
    queue.submit(Some(encoder.finish()));
    let pixels = read_rows_pixels(&device, &queue, &target);
    // The board backing's bytes: 0.55/0.42/0.28 of 255, rounded.
    let board = [
        (0.55f32 * 255.0).round() as u8,
        (0.42f32 * 255.0).round() as u8,
        (0.28f32 * 255.0).round() as u8,
    ];
    expect_rows(
        &pixels,
        WIDE as u32 / 2,
        100,
        board,
        "the editor's board backing",
    );
    // The white title's first cell inks at the measured pen.
    expect_rows(
        &pixels,
        title_x as u32,
        40,
        [255, 255, 255],
        "the title's first glyph",
    );
    // The editing line: the bars ink, and the cursor wrap brackets both sides.
    expect_rows(&pixels, line_x as u32 + 6, 80, [0, 0, 0], "the line's bars");
    expect_rows(
        &pixels,
        line_x as u32,
        80,
        [0, 0, 0],
        "the cursor's opening bracket",
    );
    expect_rows(
        &pixels,
        line_x as u32 + 14,
        80,
        [0, 0, 0],
        "the cursor's closing bracket",
    );
}

/// Task 22's lettered sheet: the title and cursor cells inked in their first column —
/// `E d i t s g n m a D o > < |` — so every one advances two font pixels and the pens
/// the test measures match the pass's layout. Generated here; no asset pixel is embedded.
fn t22_font_sheet() -> Texture {
    const SIDE: u32 = 128;
    const CELL: u32 = 8;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    for code in [
        'E', 'd', 'i', 't', 's', 'g', 'n', 'm', 'a', 'D', 'o', '>', '<', '|',
    ] {
        let code = code as u32;
        let cell_x = (code % 16) * CELL;
        let cell_y = (code / 16) * CELL;
        for row in 0..CELL {
            let offset = (((cell_y + row) * SIDE + cell_x) * 4) as usize;
            rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    Texture {
        width: SIDE,
        height: SIDE,
        rgba,
    }
}

/// Task 22's close-up camera: the eye at the board's height, looking straight at the
/// sign's face from one block out, so the ~0.4-world-unit text fills the 64x64 target.
fn t22_camera() -> Camera {
    Camera {
        pose: CameraPose {
            position: [0.5, 1.5 - f64::from(EYE_HEIGHT), 1.6],
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

/// One sign-text render: the entries through the sign pass alone over the sky clear,
/// read back for the ink census.
fn t22_sign_pixels(entries: &[oxide_render::sign_text::SignTextEntry]) -> Vec<u8> {
    use oxide_render::sign_text::SignTextPass;

    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = create_target(&device, format);
    let depth = create_depth(&device);
    let mut pass = SignTextPass::new(&device, format);
    pass.set_camera(&queue, t22_camera(), 1.0);
    pass.set_font(&device, &queue, &overlay_font_sheet())
        .expect("the synthetic sheet is a 16x16 grid");
    pass.upload(&device, &queue, entries);
    render_scene(&device, &queue, &target, &depth, |pass_in| {
        pass.draw(pass_in);
    })
}

/// The ink census of a 64x64 read-back: the non-sky pixels' bounding box and count.
/// Pixel counts are reflection-invariant, so the orientation pins below compare box
/// sides and widths across facings — never bare counts.
fn t22_ink_census(pixels: &[u8]) -> Option<(u32, u32, u32, u32, usize)> {
    let mut ink: Vec<(u32, u32)> = Vec::new();
    for y in 0..SIZE {
        for x in 0..SIZE {
            if pixel(pixels, x, y) != SKY {
                ink.push((x, y));
            }
        }
    }
    if ink.is_empty() {
        return None;
    }
    let min_x = ink.iter().map(|&(x, _)| x).min().expect("ink");
    let max_x = ink.iter().map(|&(x, _)| x).max().expect("ink");
    let min_y = ink.iter().map(|&(_, y)| y).min().expect("ink");
    let max_y = ink.iter().map(|&(_, y)| y).max().expect("ink");
    Some((min_x, max_x, min_y, max_y, ink.len()))
}

/// Task 22's floor-sign case: a standing sign's text at rotation 0 lies flat on the
/// board face, spreading along the world's x-axis — while the same text at rotation 4
/// (a quarter turn) spreads along the depth axis and collapses on screen.
///
/// The width comparison is the orientation pin: a billboard, a mirrored quad or a
/// rotation-ignoring draw renders both rotations equally wide and fails it; the side
/// (wide at rotation 0) and the margin are recorded here.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t22_sign_floor_text_turns_with_its_rotation() {
    use oxide_render::sign_text::SignTextEntry;

    let lines = || ["|".repeat(20), String::new(), String::new(), String::new()];
    let flat = t22_sign_pixels(&[SignTextEntry {
        x: 0,
        y: 1,
        z: 0,
        block_id: 63,
        metadata: 0,
        lines: lines(),
    }]);
    let turned = t22_sign_pixels(&[SignTextEntry {
        x: 0,
        y: 1,
        z: 0,
        block_id: 63,
        metadata: 4,
        lines: lines(),
    }]);
    let (flat_min, flat_max, _, _, flat_count) =
        t22_ink_census(&flat).expect("rotation 0 leaves ink");
    // The quarter turn stands the text along the depth axis: edge-on, it
    // leaves a sliver at most — no census at all when the sliver misses.
    let turned_wide = t22_ink_census(&turned)
        .map(|(min, max, _, _, _)| max - min)
        .unwrap_or(0);
    let flat_wide = flat_max - flat_min;
    assert!(flat_count > 0, "rotation 0 inks pixels");
    // The recorded side and margin: rotation 0 spreads wide, rotation 4 stands narrow.
    assert!(
        flat_wide >= turned_wide + 8,
        "rotation 0 (wide {flat_wide}) must outspread rotation 4 (narrow {turned_wide}) by 8px"
    );
}

/// One opaque stone cube spanning `y0..y0 + 1` in the terrain mesh: the
/// integrated client's terrain for one cell — the ground under the sign, or
/// the sign cell itself as the mesher fills it today with the fallback cube
/// (a full opaque cube with depth write; the stand-in atlas is one colour,
/// so the sprite is immaterial). The faces reuse `stone_block_mesh`'s own
/// corner table and brightness values, shifted up by `y0`.
fn t22_push_stone_cube(mesh: &mut ChunkMesh, y0: f32) {
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
    for (corners, brightness) in FACES {
        let lifted = corners.map(|[x, y, z]| [x, y + y0, z]);
        push_face(mesh, lifted, brightness, STONE_COLOUR);
    }
}

/// Task 22's integrated-client case: the standing sign's text over the
/// terrain layer in the client's own layer order (the solid terrain layer
/// with depth write before `SignText`, `renderer.rs:scene_draws`).
///
/// Two terrains go through the same frame: the ground cube alone — what the
/// fixed mesher emits, the sign cell empty — and the ground cube plus a full
/// opaque cube at the sign's own cell, the fallback the mesher used to emit
/// (a stone stand-in: the atlas is one colour, so the sprite is immaterial).
/// The text must ink pixels the bare terrain lacks over the fixed terrain
/// (it survives), while over the cubed cell it inks nothing new (the text
/// quads sit inside the cube and fail the LessEqual test — the burial F1
/// removes). The control arm keeps the survival pin honest: a vacuous pass
/// would ink both frames alike.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t22_sign_text_survives_over_the_terrain_layer() {
    use oxide_render::sign_text::{SignTextEntry, SignTextPass};

    let entries = [SignTextEntry {
        x: 0,
        y: 1,
        z: 0,
        block_id: 63,
        metadata: 0,
        lines: ["|".repeat(20), String::new(), String::new(), String::new()],
    }];
    let frame = |cells: &ChunkMesh, texts: &[SignTextEntry]| {
        let (device, queue) = headless_device();
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let target = create_target(&device, format);
        let depth = create_depth(&device);
        let mut terrain = TerrainPass::new(&device, &queue, format);
        terrain.set_atlas(&device, &queue, &solid_atlas(16, [255, 255, 255, 255]));
        terrain.set_camera(&queue, t22_camera(), 1.0);
        terrain.upload(&device, &queue, (0, 0, 0), cells);
        let mut text = SignTextPass::new(&device, format);
        text.set_camera(&queue, t22_camera(), 1.0);
        text.set_font(&device, &queue, &overlay_font_sheet())
            .expect("the synthetic sheet is a 16x16 grid");
        text.upload(&device, &queue, texts);
        render_scene(&device, &queue, &target, &depth, |pass| {
            terrain.draw_solid(pass);
            text.draw(pass);
        })
    };
    let fresh_bytes = |cells: &ChunkMesh| {
        let bare = frame(cells, &[]);
        let with = frame(cells, &entries);
        with.iter()
            .zip(bare.iter())
            .filter(|(pixel, plain)| pixel != plain)
            .count()
    };
    // The fixed terrain: the ground cube alone, the sign cell empty.
    let mut fixed = ChunkMesh::default();
    t22_push_stone_cube(&mut fixed, 0.0);
    // The old terrain: the fallback cube fills the sign's cell too.
    let mut cubed = ChunkMesh::default();
    t22_push_stone_cube(&mut cubed, 0.0);
    t22_push_stone_cube(&mut cubed, 1.0);
    let buried = fresh_bytes(&cubed);
    assert_eq!(
        buried, 0,
        "inside the fallback cube the text inks nothing new"
    );
    let survived = fresh_bytes(&fixed);
    assert!(
        survived > 40,
        "over the fixed terrain the text survives, {survived} fresh bytes"
    );
}
/// Task 22's wall-sign case: a wall sign with facing 2 shows its face (and its text)
/// to this camera, while facing 4 turns the board edge-on and the ink collapses to a
/// sliver.
///
/// The width comparison is the orientation pin: a facing-ignoring draw shows both
/// equally wide and fails it; the side (wide at facing 2) and the margin are recorded
/// here.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t22_sign_wall_text_turns_with_its_facing() {
    use oxide_render::sign_text::SignTextEntry;

    let lines = || ["|".repeat(20), String::new(), String::new(), String::new()];
    let faced = t22_sign_pixels(&[SignTextEntry {
        x: 0,
        y: 1,
        z: 0,
        block_id: 68,
        metadata: 2,
        lines: lines(),
    }]);
    let edged = t22_sign_pixels(&[SignTextEntry {
        x: 0,
        y: 1,
        z: 0,
        block_id: 68,
        metadata: 4,
        lines: lines(),
    }]);
    let (faced_min, faced_max, _, _, faced_count) =
        t22_ink_census(&faced).expect("facing 2 leaves ink");
    // Facing 4 turns the board edge-on: the face offset leaves a sliver at
    // most — no census at all when the sliver misses.
    let edged_wide = t22_ink_census(&edged)
        .map(|(min, max, _, _, _)| max - min)
        .unwrap_or(0);
    let faced_wide = faced_max - faced_min;
    // The recorded side and margin: facing 2 spreads wide, facing 4 stands narrow.
    assert!(
        faced_count > 40,
        "facing 2 inks the face, got {faced_count}"
    );
    assert!(
        faced_wide >= edged_wide + 8,
        "facing 2 (wide {faced_wide}) must outspread facing 4 (narrow {edged_wide}) by 8px"
    );
}

/// Task 23's reader case: a three-page unsigned book standing open on page 2 through
/// the real screen pass — the sheet's panel pixels, both page-text rows' ink, the
/// indicator's ink, the two arrow sprites and the Done button with its label.
///
/// The draws mirror `oxide-client/src/screens/book.rs`'s `draws` draw for draw — the
/// render crate may not take the client edge (`scripts/check-graph.sh` forbids it) —
/// with every literal cited below; the client's unit suite pins the real `draws`
/// structurally. Positions are measured with the same width law the pass lays out
/// with, so the pens agree by construction; the asserts pin the pixels. Both sheets
/// are generated here; no asset pixel is embedded.
#[test]
#[ignore = "needs a GPU adapter; run locally with -- --ignored"]
fn t23_book_reader_draws_sheet_text_and_indicator() {
    const WIDE: f32 = 448.0;
    const TALL: f32 = 240.0;
    // The reader's own literals (`screens/book.rs`, from `GuiScreenBook.java`):
    // the 192x192 frame at ((448-192)/2, 2), the text pen at (frame+36, 2+32),
    // the indicator row at 2+16, the arrows at (frame+120/38, 2+154) 23x13 on
    // rows 192/205, the Done 200x20 at (centre-100, 4+192) with its label 6
    // below its top. The wrap mirrors `wrap_lines` at 116 with the lettered
    // sheet's 2-pixel advances, so the 70-glyph run cuts mid-word at 58.
    const T23_WRAP: i32 = 116;
    const T23_ADVANCE: i32 = 2;
    const FRAME_X: f32 = 128.0;
    const TEXT_X: f32 = 164.0;
    const TEXT_Y: f32 = 34.0;
    const CUT: usize = (T23_WRAP / T23_ADVANCE) as usize;
    let run = "A".repeat(70);
    let line_one: String = run.chars().take(CUT).collect();
    let line_two: String = run.chars().skip(CUT).collect();
    // `Page 2 of 3` weighs 8 two-pixel glyphs plus 3 four-pixel spaces: 28.
    // Right-aligned at (128 - 28 + 192 - 44, 18).
    const INDICATOR: &str = "Page 2 of 3";
    const INDICATOR_X: f32 = 248.0;
    const INDICATOR_Y: f32 = 18.0;

    let (device, queue) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = rows_target(&device, format);
    let depth = rows_depth(&device);
    let font_sheet = t23_font_sheet();
    let font = Font::load(&font_sheet, None).expect("the lettered sheet is a 16x16 grid");
    let indicator_width = f64::from(string_width(&font, INDICATOR)) as f32;
    assert_eq!(indicator_width, 28.0, "the pen math the test asserts");
    let mut screen = HudPass::new(&device, &queue, format);
    screen.set_resolution(&queue, WIDE, TALL);
    screen.set_texture(&device, &queue, "gui/book", &t23_book_sheet());
    screen.set_texture(&device, &queue, "gui/widgets", &t23_widgets_sheet());
    screen
        .set_font(&device, &queue, &font_sheet)
        .expect("the lettered sheet is a 16x16 grid");
    let black = [0.0, 0.0, 0.0, 1.0];
    screen.set_draws(
        &device,
        &queue,
        &[
            HudDraw::TexturedRect {
                texture: HudTexture::Named("gui/book"),
                x: FRAME_X,
                y: 2.0,
                width: 192.0,
                height: 192.0,
                uv: [0.0, 0.0, 192.0 / 256.0, 192.0 / 256.0],
                colour: [1.0, 1.0, 1.0, 1.0],
            },
            HudDraw::Text {
                text: INDICATOR.to_string(),
                x: INDICATOR_X,
                y: INDICATOR_Y,
                scale: 1.0,
                colour: black,
                shadow: false,
                blend: true,
            },
            HudDraw::Text {
                text: line_one,
                x: TEXT_X,
                y: TEXT_Y,
                scale: 1.0,
                colour: black,
                shadow: false,
                blend: true,
            },
            HudDraw::Text {
                text: line_two,
                x: TEXT_X,
                y: TEXT_Y + 9.0,
                scale: 1.0,
                colour: black,
                shadow: false,
                blend: true,
            },
            HudDraw::TexturedRect {
                texture: HudTexture::Named("gui/book"),
                x: FRAME_X + 120.0,
                y: 156.0,
                width: 23.0,
                height: 13.0,
                uv: [0.0, 192.0 / 256.0, 23.0 / 256.0, 205.0 / 256.0],
                colour: [1.0, 1.0, 1.0, 1.0],
            },
            HudDraw::TexturedRect {
                texture: HudTexture::Named("gui/book"),
                x: FRAME_X + 38.0,
                y: 156.0,
                width: 23.0,
                height: 13.0,
                uv: [0.0, 205.0 / 256.0, 23.0 / 256.0, 218.0 / 256.0],
                colour: [1.0, 1.0, 1.0, 1.0],
            },
            HudDraw::TexturedRect {
                texture: HudTexture::Named("gui/widgets"),
                x: 124.0,
                y: 196.0,
                width: 200.0,
                height: 20.0,
                uv: [0.0, 66.0 / 256.0, 200.0 / 256.0, 86.0 / 256.0],
                colour: [1.0, 1.0, 1.0, 1.0],
            },
            HudDraw::Text {
                text: "Done".to_string(),
                x: 220.0,
                y: 202.0,
                scale: 1.0,
                colour: [224.0 / 255.0, 224.0 / 255.0, 224.0 / 255.0, 1.0],
                shadow: true,
                blend: true,
            },
        ],
        &TextureRegistry::new(&device, &queue),
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("oxide t23 book headless encoder"),
    });
    with_clear_pass(&mut encoder, &target.view, SKY_COLOR);
    with_overlay_pass(&mut encoder, &target.view, &depth, |pass| {
        screen.draw(pass);
    });
    queue.submit(Some(encoder.finish()));
    let pixels = read_rows_pixels(&device, &queue, &target);
    // The sheet's own panel colour: the synthetic book sheet's base.
    const PANEL: [u8; 3] = [150, 110, 70];
    expect_rows(&pixels, 200, 100, PANEL, "the sheet's panel");
    expect_rows(&pixels, 100, 100, SKY, "the sky past the frame");
    // The wrapped text: the run's first glyph inks both rows, the 54th glyph
    // still inks inside the 116 wrap, and the pixel past the cut stays panel.
    expect_rows(&pixels, 164, 34, [0, 0, 0], "page 2's first glyph");
    expect_rows(&pixels, 270, 34, [0, 0, 0], "a glyph inside the 116 wrap");
    expect_rows(&pixels, 281, 34, PANEL, "the panel past the wrap cut");
    expect_rows(&pixels, 164, 43, [0, 0, 0], "the wrapped second row");
    // The indicator's first glyph inks at the right-aligned pen.
    expect_rows(&pixels, 248, 18, [0, 0, 0], "the indicator's first glyph");
    // The arrows sample their idle sprites: red next, blue previous.
    expect_rows(&pixels, 249, 157, [255, 0, 0], "the next arrow's sprite");
    expect_rows(
        &pixels,
        167,
        157,
        [0, 0, 255],
        "the previous arrow's sprite",
    );
    // The Done button's white strip and its grey label.
    expect_rows(&pixels, 200, 200, [255, 255, 255], "the Done strip");
    expect_rows(&pixels, 220, 202, [224, 224, 224], "the Done label");
}

/// Task 23's lettered sheet: the page, indicator and label cells inked in their
/// first column — `A P a g e 2 o f 3 D n` — so every one advances two font pixels
/// and the pens the test measures match the pass's layout. Generated here; no asset
/// pixel is embedded.
fn t23_font_sheet() -> Texture {
    const SIDE: u32 = 128;
    const CELL: u32 = 8;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    for code in ['A', 'P', 'a', 'g', 'e', '2', 'o', 'f', '3', 'D', 'n'] {
        let code = code as u32;
        let cell_x = (code % 16) * CELL;
        let cell_y = (code / 16) * CELL;
        for row in 0..CELL {
            let offset = (((cell_y + row) * SIDE + cell_x) * 4) as usize;
            rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    Texture {
        width: SIDE,
        height: SIDE,
        rgba,
    }
}

/// Task 23's synthetic book sheet: the panel base with the next arrow's idle sprite
/// in red at `(0..23, 192..205)` and the previous arrow's in blue at
/// `(0..23, 205..218)` — the source's own sprite cells. Generated here; no asset
/// pixel is embedded.
fn t23_book_sheet() -> Texture {
    const SIDE: u32 = 256;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    for y in 0..SIDE {
        for x in 0..SIDE {
            let offset = ((y * SIDE + x) * 4) as usize;
            let colour = if x < 23 && (192..205).contains(&y) {
                [255, 0, 0, 255]
            } else if x < 23 && (205..218).contains(&y) {
                [0, 0, 255, 255]
            } else {
                [150, 110, 70, 255]
            };
            rgba[offset..offset + 4].copy_from_slice(&colour);
        }
    }
    Texture {
        width: SIDE,
        height: SIDE,
        rgba,
    }
}

/// Task 23's synthetic widgets strip: the Done button's idle row white at v 66..86 —
/// `GuiButton`'s enabled strip — over transparency. Generated here; no asset pixel is
/// embedded.
fn t23_widgets_sheet() -> Texture {
    const SIDE: u32 = 256;
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    for y in 66..86u32 {
        for x in 0..200u32 {
            let offset = ((y * SIDE + x) * 4) as usize;
            rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    Texture {
        width: SIDE,
        height: SIDE,
        rgba,
    }
}
