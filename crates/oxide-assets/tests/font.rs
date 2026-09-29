//! The font sheet's width rules: the client's own ink-column scan, the space's special case,
//! the `glyph_sizes` half-scale rule and the sheet-size check.
//!
//! The sheet is built here in memory — [`Texture`] is plain public fields — so no file and no
//! game pixel is involved. Every cell's ink is synthetic: cells are painted column by column
//! and the expected widths follow the scan rule the tests pin.

use oxide_assets::font::{Font, FontError};
use oxide_assets::texture::Texture;

/// The sheet's side in texels: the real `font/ascii.png` is the 16x16 grid of 8-texel cells.
const SIDE: u32 = 128;
/// The side of one glyph cell in texels.
const CELL: u32 = 8;
/// The grid side: the sheet is 16 cells across and down.
const GRID: u32 = 16;

#[test]
fn the_ink_scan_sets_each_printable_width() {
    let mut sheet = blank_sheet();
    // The scan's rule on a 128x128 sheet: ink in columns `0..=k` gives width `k + 2`, a blank
    // cell gives 1. The cells below are the store-verified widths' patterns.
    paint(&mut sheet, 'A', 4); // 6
    paint(&mut sheet, 'i', 0); // 2
    paint(&mut sheet, 'l', 1); // 3
    paint(&mut sheet, '!', 0); // 2
    paint(&mut sheet, '8', 4); // 6
    paint(&mut sheet, '.', 0); // 2
    // A printable cell with no ink at all still answers a width of one.
    let font = Font::load(&sheet, None).expect("the sheet loads");

    assert_eq!(font.advance('A'), 6);
    assert_eq!(font.advance('i'), 2);
    assert_eq!(font.advance('l'), 3);
    assert_eq!(font.advance('!'), 2);
    assert_eq!(font.advance('8'), 6);
    assert_eq!(font.advance('.'), 2);
    assert_eq!(font.advance('E'), 1, "a blank printable cell is one wide");
    assert_eq!(font.height(), 9, "FontRenderer.FONT_HEIGHT");

    // The glyph rect: the cell's origin and the `charWidth - 1` x 8 box.
    assert_eq!(font.glyph_rect('A'), Some((8, 32, 5, 8)));
    assert_eq!(
        font.glyph_rect('E'),
        Some((40, 32, 0, 8)),
        "a blank cell's box is empty, not absent"
    );
    assert_eq!(font.glyph_rect(' '), Some((0, 16, 3, 8)));
    assert_eq!(
        font.glyph_rect('é'),
        None,
        "non-ASCII is outside the sheet's grid"
    );
    assert_eq!(font.glyph_rect('\u{1F600}'), None);
}

#[test]
fn the_space_is_four_wide_and_the_scan_never_reaches_it() {
    // A sheet whose space cell carries ink: the advance must stay 4, because the source
    // returns before `renderDefaultChar` for the space and `getCharWidth` answers 4.
    let mut sheet = blank_sheet();
    paint(&mut sheet, ' ', 4);
    let font = Font::load(&sheet, None).expect("the sheet loads");
    assert_eq!(font.advance(' '), 4);
    assert_eq!(font.glyph_rect(' '), Some((0, 16, 3, 8)));
}

#[test]
fn a_non_ascii_character_takes_its_glyph_sizes_entry_or_zero() {
    let sheet = blank_sheet();
    let no_table = Font::load(&sheet, None).expect("the sheet loads");
    assert_eq!(
        no_table.advance('é'),
        0,
        "without a glyph_sizes table a non-ASCII character has no width"
    );
    assert_eq!(no_table.advance('\u{7}'), 0, "a control character too");

    let mut glyph_sizes = [0u8; 65536];
    // `0x24` is start 2, end 4: `(4 + 1 - 2) / 2 + 1 = 2`.
    glyph_sizes[0xE9] = 0x24;
    // `0x0A` is start 0 with end above 7: the source's arm makes it end 15, so
    // `(15 + 1 - 0) / 2 + 1 = 9`.
    glyph_sizes[0x100] = 0x0A;
    let font = Font::load(&sheet, Some(&glyph_sizes)).expect("the table loads");
    assert_eq!(font.advance('é'), 2);
    assert_eq!(font.advance('Ā'), 9);
    assert_eq!(font.advance('\u{7}'), 0, "a zero entry has no width");
}

#[test]
fn a_glyph_sizes_table_of_the_wrong_size_is_refused() {
    let sheet = blank_sheet();
    let short = [0u8; 128];
    let error = Font::load(&sheet, Some(&short)).expect_err("a short table is refused");
    assert!(matches!(error, FontError::GlyphSizesLength { got: 128 }));
    assert!(error.to_string().contains("128"), "{error}");
}

#[test]
fn a_sheet_that_does_not_divide_into_the_grid_is_refused() {
    for (width, height) in [(100, 128), (128, 100), (127, 128)] {
        let sheet = Texture {
            width,
            height,
            rgba: vec![0; (width * height * 4) as usize],
        };
        let error = Font::load(&sheet, None).expect_err("a sheet off the grid is refused");
        assert!(matches!(error, FontError::SheetSize { .. }));
        let message = error.to_string();
        assert!(
            message.contains(&format!("{width}x{height}")),
            "the message names the size: {message}"
        );
    }
}

/// A transparent 128x128 sheet.
fn blank_sheet() -> Texture {
    Texture {
        width: SIDE,
        height: SIDE,
        rgba: vec![0; (SIDE * SIDE * 4) as usize],
    }
}

/// Paints `character`'s cell with opaque white ink in columns `0..=last_ink`, every row.
fn paint(sheet: &mut Texture, character: char, last_ink: u32) {
    let code = character as u32;
    assert!(code < GRID * GRID, "the test sheet holds ASCII cells only");
    let cell_x = (code % GRID) * CELL;
    let cell_y = (code / GRID) * CELL;
    for row in 0..CELL {
        for column in 0..=last_ink {
            let x = cell_x + column;
            let y = cell_y + row;
            let offset = ((y * SIDE + x) * 4) as usize;
            sheet.rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
}
