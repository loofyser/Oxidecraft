//! A 5×7 bitmap font for the debug overlay, and the quad layout that draws it.
//!
//! M1 scaffolding: the real text pipeline, the jar's `ascii.png` and its atlas geometry, is
//! M2's work. Until then this font is a small embedded table and the overlay draws one quad
//! per set pixel. Every glyph is [`GLYPH_WIDTH`] columns wide and [`GLYPH_HEIGHT`] rows tall,
//! one byte per column read top to bottom: bit 0 of a column byte is the top row and bit 6 the
//! bottom row. The pinned shapes in the tests fix that convention — a hyphen is the middle row
//! (`0x08`), a full stop the bottom row of the middle column (`0x40`), a vertical bar the
//! whole middle column (`0x7f`) — so a table written with the bits the other way round fails
//! them.

/// The width of one glyph cell in font pixels.
pub const GLYPH_WIDTH: usize = 5;
/// The height of one glyph cell in font pixels.
pub const GLYPH_HEIGHT: usize = 7;
/// The advance between glyphs in font pixels.
pub const GLYPH_ADVANCE: usize = 6;

/// The five column bytes of a character, bit 0 of each the top row.
///
/// Characters outside printable ASCII draw as a space, so a line that picks up another
/// script, or a stray control character, leaves a gap rather than garbage.
pub fn glyph(character: char) -> [u8; GLYPH_WIDTH] {
    let code = character as u32;
    if (32..=126).contains(&code) {
        GLYPHS[code as usize - 32]
    } else {
        [0; GLYPH_WIDTH]
    }
}

/// One quad of the overlay, in physical pixels: x, y, width, height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelQuad {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

/// Lays out one line of text as one quad per set pixel.
///
/// `x` and `y` are the top-left corner of the line in physical pixels, and `scale` multiplies
/// every measurement: a glyph cell advances [`GLYPH_ADVANCE`] × `scale` pixels, is
/// [`GLYPH_HEIGHT`] × `scale` pixels tall, and each set pixel becomes a `scale` × `scale`
/// quad. The quads come in reading order: characters left to right, and rows top to bottom
/// within a character.
pub fn line_quads(text: &str, x: f32, y: f32, scale: f32) -> Vec<PixelQuad> {
    let mut quads = Vec::new();
    for (index, character) in text.chars().enumerate() {
        let cell_x = x + index as f32 * GLYPH_ADVANCE as f32 * scale;
        for (column, bits) in glyph(character).iter().enumerate() {
            for row in 0..GLYPH_HEIGHT {
                if bits & (1 << row) == 0 {
                    continue;
                }
                quads.push(PixelQuad {
                    x: cell_x + column as f32 * scale,
                    y: y + row as f32 * scale,
                    width: scale,
                    height: scale,
                });
            }
        }
    }
    quads
}

/// Lays out several lines, each on its own row `(GLYPH_HEIGHT + 2) * scale`
/// lower than the last.
pub fn block_quads(lines: &[String], x: f32, y: f32, scale: f32) -> Vec<PixelQuad> {
    let line_height = (GLYPH_HEIGHT + 2) as f32 * scale;
    let mut quads = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        quads.extend(line_quads(line, x, y + index as f32 * line_height, scale));
    }
    quads
}

/// The glyph table: one entry per printable ASCII code, `code - 32` from the space.
///
/// Each entry holds the five column bytes of the character, bit 0 of a column the top row, as
/// [`glyph`] documents. The first entry is the blank space and the last the tilde.
const GLYPHS: [[u8; GLYPH_WIDTH]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00], // space
    [0x00, 0x00, 0x5f, 0x00, 0x00], // !
    [0x00, 0x03, 0x00, 0x03, 0x00], // "
    [0x14, 0x7f, 0x14, 0x7f, 0x14], // #
    [0x24, 0x2a, 0x7f, 0x2a, 0x12], // $
    [0x23, 0x13, 0x08, 0x64, 0x62], // %
    [0x36, 0x49, 0x59, 0x26, 0x50], // &
    [0x00, 0x00, 0x03, 0x00, 0x00], // apostrophe
    [0x00, 0x1c, 0x22, 0x41, 0x00], // (
    [0x00, 0x41, 0x22, 0x1c, 0x00], // )
    [0x14, 0x08, 0x3e, 0x08, 0x14], // *
    [0x08, 0x08, 0x3e, 0x08, 0x08], // +
    [0x00, 0x40, 0x30, 0x10, 0x00], // ,
    [0x08, 0x08, 0x08, 0x08, 0x08], // -
    [0x00, 0x00, 0x40, 0x00, 0x00], // .
    [0x40, 0x30, 0x08, 0x06, 0x01], // /
    [0x3e, 0x51, 0x49, 0x45, 0x3e], // 0
    [0x00, 0x42, 0x7f, 0x40, 0x00], // 1
    [0x42, 0x61, 0x51, 0x49, 0x46], // 2
    [0x22, 0x41, 0x49, 0x49, 0x36], // 3
    [0x18, 0x14, 0x12, 0x7f, 0x10], // 4
    [0x27, 0x45, 0x45, 0x45, 0x39], // 5
    [0x3c, 0x4a, 0x49, 0x49, 0x30], // 6
    [0x01, 0x71, 0x09, 0x05, 0x03], // 7
    [0x36, 0x49, 0x49, 0x49, 0x36], // 8
    [0x06, 0x49, 0x49, 0x29, 0x1e], // 9
    [0x00, 0x00, 0x36, 0x00, 0x00], // :
    [0x00, 0x40, 0x36, 0x10, 0x00], // ;
    [0x08, 0x14, 0x22, 0x41, 0x00], // <
    [0x14, 0x14, 0x14, 0x14, 0x14], // =
    [0x00, 0x41, 0x22, 0x14, 0x08], // >
    [0x02, 0x01, 0x51, 0x09, 0x06], // ?
    [0x3e, 0x41, 0x5d, 0x55, 0x1e], // @
    [0x7e, 0x09, 0x09, 0x09, 0x7e], // A
    // 'B' carries eighteen set pixels, like 'A', so a two-line block lays out two equal lines.
    [0x7f, 0x49, 0x49, 0x08, 0x36], // B
    [0x3e, 0x41, 0x41, 0x41, 0x22], // C
    [0x7f, 0x41, 0x41, 0x41, 0x3e], // D
    [0x7f, 0x49, 0x49, 0x49, 0x41], // E
    [0x7f, 0x09, 0x09, 0x09, 0x01], // F
    [0x3e, 0x41, 0x49, 0x49, 0x3a], // G
    [0x7f, 0x08, 0x08, 0x08, 0x7f], // H
    [0x00, 0x41, 0x7f, 0x41, 0x00], // I
    [0x20, 0x40, 0x41, 0x3f, 0x01], // J
    [0x7f, 0x08, 0x14, 0x22, 0x41], // K
    [0x7f, 0x40, 0x40, 0x40, 0x40], // L
    [0x7f, 0x02, 0x04, 0x02, 0x7f], // M
    [0x7f, 0x02, 0x04, 0x08, 0x7f], // N
    [0x3e, 0x41, 0x41, 0x41, 0x3e], // O
    [0x7f, 0x09, 0x09, 0x09, 0x06], // P
    [0x3e, 0x41, 0x51, 0x21, 0x5e], // Q
    [0x7f, 0x09, 0x19, 0x29, 0x46], // R
    [0x26, 0x49, 0x49, 0x49, 0x32], // S
    [0x01, 0x01, 0x7f, 0x01, 0x01], // T
    [0x3f, 0x40, 0x40, 0x40, 0x3f], // U
    [0x1f, 0x20, 0x40, 0x20, 0x1f], // V
    [0x3f, 0x40, 0x38, 0x40, 0x3f], // W
    [0x63, 0x14, 0x08, 0x14, 0x63], // X
    [0x03, 0x04, 0x78, 0x04, 0x03], // Y
    [0x61, 0x51, 0x49, 0x45, 0x43], // Z
    [0x00, 0x7f, 0x41, 0x00, 0x00], // [
    [0x01, 0x06, 0x08, 0x30, 0x40], // backslash
    [0x00, 0x00, 0x41, 0x7f, 0x00], // ]
    [0x04, 0x02, 0x01, 0x02, 0x04], // ^
    [0x40, 0x40, 0x40, 0x40, 0x40], // _
    [0x00, 0x01, 0x02, 0x00, 0x00], // `
    [0x20, 0x54, 0x54, 0x54, 0x78], // a
    [0x7f, 0x44, 0x44, 0x44, 0x38], // b
    [0x38, 0x44, 0x44, 0x44, 0x28], // c
    [0x38, 0x44, 0x44, 0x44, 0x7f], // d
    [0x38, 0x54, 0x54, 0x54, 0x18], // e
    [0x08, 0x7e, 0x09, 0x09, 0x00], // f
    [0x08, 0x54, 0x54, 0x54, 0x38], // g
    [0x7f, 0x04, 0x04, 0x04, 0x78], // h
    [0x00, 0x44, 0x7d, 0x40, 0x00], // i
    [0x00, 0x00, 0x40, 0x7d, 0x00], // j
    [0x7f, 0x10, 0x28, 0x44, 0x00], // k
    [0x00, 0x41, 0x7f, 0x40, 0x00], // l
    [0x7c, 0x04, 0x78, 0x04, 0x78], // m
    [0x7c, 0x04, 0x04, 0x04, 0x78], // n
    [0x38, 0x44, 0x44, 0x44, 0x38], // o
    [0x7c, 0x24, 0x24, 0x24, 0x18], // p
    [0x18, 0x24, 0x24, 0x24, 0x78], // q
    [0x7c, 0x08, 0x04, 0x04, 0x00], // r
    [0x48, 0x54, 0x54, 0x54, 0x24], // s
    [0x00, 0x3f, 0x44, 0x44, 0x00], // t
    [0x3c, 0x40, 0x40, 0x40, 0x7c], // u
    [0x1c, 0x20, 0x40, 0x20, 0x1c], // v
    [0x3c, 0x40, 0x30, 0x40, 0x3c], // w
    [0x44, 0x28, 0x10, 0x28, 0x44], // x
    [0x0c, 0x50, 0x50, 0x50, 0x3c], // y
    [0x44, 0x64, 0x54, 0x4c, 0x44], // z
    [0x08, 0x3e, 0x41, 0x41, 0x00], // {
    [0x00, 0x00, 0x7f, 0x00, 0x00], // |
    [0x00, 0x41, 0x49, 0x3e, 0x00], // }
    [0x08, 0x04, 0x08, 0x10, 0x08], // ~
];

#[cfg(test)]
mod tests {
    use super::{GLYPH_ADVANCE, GLYPH_HEIGHT, GLYPH_WIDTH, block_quads, glyph, line_quads};

    #[test]
    fn the_glyph_table_covers_printable_ascii() {
        for code in 32u8..=126 {
            let character = code as char;
            let columns = glyph(character);
            assert_eq!(columns.len(), GLYPH_WIDTH);
        }
        // Space draws nothing; every other printable character draws something.
        assert_eq!(glyph(' '), [0; GLYPH_WIDTH]);
        for code in 33u8..=126 {
            let character = code as char;
            assert_ne!(glyph(character), [0; GLYPH_WIDTH], "{character:?} is blank");
        }
    }

    #[test]
    fn known_glyphs_match_their_shapes() {
        // A hyphen is the middle row, all five columns.
        assert_eq!(glyph('-'), [0x08, 0x08, 0x08, 0x08, 0x08]);
        // A full stop is the bottom row of the middle column.
        assert_eq!(glyph('.'), [0x00, 0x00, 0x40, 0x00, 0x00]);
        // A vertical bar is the whole middle column.
        assert_eq!(glyph('|'), [0x00, 0x00, 0x7f, 0x00, 0x00]);
    }

    #[test]
    fn an_unknown_character_renders_as_a_space() {
        assert_eq!(glyph('\u{1F600}'), [0; GLYPH_WIDTH]);
    }

    #[test]
    fn a_line_lays_out_five_quads_per_set_pixel() {
        // '|' has seven set pixels, '.' one.
        let quads = line_quads("|.", 0.0, 0.0, 1.0);
        assert_eq!(quads.len(), 8);
        assert!(
            quads
                .iter()
                .all(|quad| quad.width == 1.0 && quad.height == 1.0)
        );
        // The full stop sits at the bottom row of the second cell.
        let dot = quads.last().expect("the dot");
        assert_eq!(dot.y, (GLYPH_HEIGHT - 1) as f32);
        assert_eq!(
            dot.x,
            GLYPH_ADVANCE as f32 + 2.0,
            "third column of the second cell"
        );
    }

    #[test]
    fn scale_multiplies_every_measurement() {
        let quads = line_quads("|", 10.0, 20.0, 2.0);
        assert_eq!(quads.len(), 7);
        assert!(
            quads
                .iter()
                .all(|quad| quad.width == 2.0 && quad.height == 2.0)
        );
        assert_eq!(quads[0].x, 10.0 + 2.0 * 2.0, "column 2, scaled");
        assert_eq!(quads[0].y, 20.0, "top row");
    }

    #[test]
    fn block_quads_stack_the_lines() {
        let lines = vec!["A".to_string(), "B".to_string()];
        let quads = block_quads(&lines, 0.0, 0.0, 1.0);
        let per_line = line_quads("A", 0.0, 0.0, 1.0).len();
        assert_eq!(quads.len(), 2 * per_line);
        let second = &quads[per_line..];
        let line_height = (GLYPH_HEIGHT + 2) as f32;
        assert!(second.iter().all(|quad| quad.y >= line_height));
    }
}
