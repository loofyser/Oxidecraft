//! The overlay's pure glyph layout, without a GPU: the quad list for a known line, the shadow
//! rule, the pen and line metrics, and the upload cache's change detection.
//!
//! The font sheet is built here in memory from synthetic cells, so no asset store is read and
//! no game pixel is involved.

use oxide_assets::font::Font;
use oxide_assets::texture::Texture;
use oxide_render::debug_text::{GlyphVertex, glyph_geometry, shadow_colour};
use oxide_render::overlay::lines_changed;

/// The sheet's side in texels: the real `font/ascii.png` shape.
const SHEET_SIDE: u32 = 128;
/// The side of one glyph cell in texels.
const CELL: u32 = 8;
/// The layout origin the overlay draws its text from, in physical pixels.
const ORIGIN: [f32; 2] = [4.0, 4.0];
/// The text scale in physical pixels per font pixel.
const SCALE: f32 = 2.0;
/// The colour the overlay's text is drawn in: opaque white.
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

#[test]
fn a_glyph_is_one_full_cell_quad_drawn_twice() {
    let sheet = a_sheet();
    let font = Font::load(&sheet, None).expect("the synthetic sheet loads");
    assert_eq!(font.advance('A'), 6, "the store-verified 'A' pattern");

    let lines = vec!["A".to_string()];
    let (vertices, indices) = glyph_geometry(
        &font,
        (SHEET_SIDE, SHEET_SIDE),
        &lines,
        ORIGIN,
        SCALE,
        WHITE,
    );

    // One glyph, one full-cell quad, drawn twice: the shadow copy first, then the text.
    assert_eq!(vertices.len(), 8);
    assert_eq!(indices.len(), 12);
    assert_eq!(indices, [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]);

    // 'A' is code 65: column 1, row 4, so its cell is (8, 32) to (16, 40) over the sheet.
    let (u0, v0, u1, v1) = (8.0 / 128.0, 32.0 / 128.0, 16.0 / 128.0, 40.0 / 128.0);
    // The shadow is the same quad one font pixel (two physical pixels) down and right, in the
    // source's darkened colour: `(0xFFFFFFFF & 0x00FCFCFC) >> 2 | 0xFF000000` = 0xFF3F3F3F.
    let shadow = shadow_colour(WHITE);
    assert_eq!(shadow, [63.0 / 255.0, 63.0 / 255.0, 63.0 / 255.0, 1.0]);
    let corners = |left: f32, top: f32| {
        [
            GlyphVertex {
                position: [left, top],
                uv: [u0, v0],
                color: WHITE,
            },
            GlyphVertex {
                position: [left, top + CELL as f32 * SCALE],
                uv: [u0, v1],
                color: WHITE,
            },
            GlyphVertex {
                position: [left + CELL as f32 * SCALE, top + CELL as f32 * SCALE],
                uv: [u1, v1],
                color: WHITE,
            },
            GlyphVertex {
                position: [left + CELL as f32 * SCALE, top],
                uv: [u1, v0],
                color: WHITE,
            },
        ]
    };
    let mut expected_shadow = corners(ORIGIN[0] + SCALE, ORIGIN[1] + SCALE);
    for vertex in &mut expected_shadow {
        vertex.color = shadow;
    }
    assert_eq!(&vertices[..4], &expected_shadow[..], "the shadow quad");
    assert_eq!(
        &vertices[4..],
        &corners(ORIGIN[0], ORIGIN[1])[..],
        "the text quad"
    );
}

#[test]
fn the_pen_advances_by_the_glyphs_width_and_the_lines_stack_by_the_height() {
    let sheet = a_sheet();
    let font = Font::load(&sheet, None).expect("the synthetic sheet loads");

    // "AA": the second glyph starts one 6-font-pixel advance along.
    let (vertices, _) = glyph_geometry(
        &font,
        (SHEET_SIDE, SHEET_SIDE),
        &["AA".to_string()],
        ORIGIN,
        SCALE,
        WHITE,
    );
    assert_eq!(vertices.len(), 16, "two glyphs, two copies each");
    // The shadow copies come first (both glyphs), then the text copies.
    assert_eq!(
        vertices[4].position,
        [ORIGIN[0] + SCALE + 6.0 * SCALE, ORIGIN[1] + SCALE]
    );
    assert_eq!(vertices[12].position, [ORIGIN[0] + 6.0 * SCALE, ORIGIN[1]]);

    // Two lines stack at `Font::height()`, 9 font pixels.
    let lines = vec!["A".to_string(), "A".to_string()];
    let (vertices, _) = glyph_geometry(
        &font,
        (SHEET_SIDE, SHEET_SIDE),
        &lines,
        ORIGIN,
        SCALE,
        WHITE,
    );
    assert_eq!(vertices.len(), 16);
    assert_eq!(
        vertices[12].position,
        [ORIGIN[0], ORIGIN[1] + 9.0 * SCALE],
        "the second line's text"
    );
}

#[test]
fn the_space_draws_nothing_and_only_advances() {
    let sheet = a_sheet();
    let font = Font::load(&sheet, None).expect("the synthetic sheet loads");
    let (vertices, indices) = glyph_geometry(
        &font,
        (SHEET_SIDE, SHEET_SIDE),
        &[" ".to_string()],
        ORIGIN,
        SCALE,
        WHITE,
    );
    assert!(
        vertices.is_empty(),
        "the source draws no quad for the space"
    );
    assert!(indices.is_empty());

    // But it still moves the pen: 'A' after a space starts 4 font pixels along.
    let (vertices, _) = glyph_geometry(
        &font,
        (SHEET_SIDE, SHEET_SIDE),
        &[" A".to_string()],
        ORIGIN,
        SCALE,
        WHITE,
    );
    assert_eq!(vertices.len(), 8, "only the 'A' draws");
    assert_eq!(vertices[4].position, [ORIGIN[0] + 4.0 * SCALE, ORIGIN[1]]);
}

#[test]
fn an_unchanged_line_set_is_reported_unchanged() {
    let lines = vec!["Oxidecraft 1.8.9".to_string(), "60 fps".to_string()];
    let same = lines.clone();
    let changed = vec!["Oxidecraft 1.8.9".to_string(), "61 fps".to_string()];
    assert!(!lines_changed(&lines, &same), "the same lines are a no-op");
    assert!(lines_changed(&lines, &changed), "a changed line rebuilds");
    assert!(lines_changed(&[], &lines), "the first upload is a change");
    assert!(!lines_changed(&[], &[]), "and nothing stays nothing");
}

/// A 128x128 synthetic sheet whose `'A'` cell carries the store-verified ink pattern: ink in
/// columns 0..=4, so the scan measures six.
fn a_sheet() -> Texture {
    let mut rgba = vec![0u8; (SHEET_SIDE * SHEET_SIDE * 4) as usize];
    let code = 'A' as u32;
    let cell_x = (code % 16) * CELL;
    let cell_y = (code / 16) * CELL;
    for row in 0..CELL {
        for column in 0..=4 {
            let x = cell_x + column;
            let y = cell_y + row;
            let offset = ((y * SHEET_SIDE + x) * 4) as usize;
            rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    Texture {
        width: SHEET_SIDE,
        height: SHEET_SIDE,
        rgba,
    }
}
