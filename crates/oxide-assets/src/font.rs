//! The jar's ascii font sheet: the ink-column widths the debug overlay lays text out with.
//!
//! The sheet is `textures/font/ascii.png`, a 16x16 grid of glyph cells. The client's own
//! loader measures each printable character from its cell (`FontRenderer.readFontTexture`,
//! `FontRenderer.java:184-206`): it scans the cell's columns from the right for the last one
//! holding any non-zero alpha, and derives the character's advance width from that column's
//! index. [`Font::load`] performs the same scan on a decoded [`Texture`] and nothing else;
//! the pixel data stays the caller's.
//!
//! The source's two special cases are kept:
//!
//! * The space never reaches the scan: `renderChar` returns before `renderDefaultChar` for
//!   it (`:210-213`) and `getCharWidth` answers 4 (`:664-667`), so [`Font::advance`] does too.
//! * A character outside printable ASCII has no cell in the width table; the source falls
//!   back to the optional `font/glyph_sizes.bin` table, whose byte packs the glyph's start
//!   and end columns, and to zero when that table has no entry (`:676-689`). The overlay
//!   draws printable ASCII only (known limit 6), but [`Font::advance`] keeps the rule.
//!
//! The sheet must divide into the 16x16 grid on both axes; anything else is a
//! [`FontError::SheetSize`] naming the size, never a silent default.

use crate::texture::Texture;

/// The number of cells along each axis of the sheet.
const GRID: u32 = 16;

/// The first printable ASCII code: the space, whose cell is the grid's first.
const FIRST_PRINTABLE: u32 = 32;

/// The last printable ASCII code: the tilde.
const LAST_PRINTABLE: u32 = 126;

/// The number of printable ASCII codes.
const PRINTABLE_COUNT: usize = (LAST_PRINTABLE - FIRST_PRINTABLE + 1) as usize;

/// The advance the source gives the space: `getCharWidth`'s own special case, 4.
const SPACE_ADVANCE: u32 = 4;

/// The height the source gives one line of text: `FontRenderer.FONT_HEIGHT`, 9.
const LINE_HEIGHT: u32 = 9;

/// The glyph cell side in texels the source's renderer reads: `renderDefaultChar`'s own 8.
const CELL_SIDE: u32 = 8;

/// The size of the optional `font/glyph_sizes.bin` table: one byte per UTF-16 code unit.
pub const GLYPH_SIZES_LEN: usize = 1 << 16;

/// Errors from measuring a font sheet.
#[derive(Debug, thiserror::Error)]
pub enum FontError {
    /// A sheet whose axes do not divide into the 16x16 grid.
    #[error(
        "the font sheet is {width}x{height} texels; both axes must divide into the 16x16 grid \
         the sheet is read as"
    )]
    SheetSize {
        /// The sheet's width in texels.
        width: u32,
        /// The sheet's height in texels.
        height: u32,
    },
    /// A `glyph_sizes` table of the wrong length.
    #[error(
        "the glyph_sizes table is {got} bytes; the font declares 65536, one per UTF-16 code unit"
    )]
    GlyphSizesLength {
        /// The length that arrived.
        got: usize,
    },
}

/// A measured ascii font sheet: the printable characters' advances and the optional
/// `glyph_sizes` table for everything else.
#[derive(Clone)]
pub struct Font {
    /// The scanned width in font pixels of each printable ASCII code, `code - 32` from the
    /// space. The space's entry is [`SPACE_ADVANCE`], the source's own special case.
    widths: [u32; PRINTABLE_COUNT],
    /// `font/glyph_sizes.bin`, one packed start/end pair per UTF-16 code unit, when the
    /// caller supplied it.
    glyph_sizes: Option<Box<[u8; GLYPH_SIZES_LEN]>>,
}

impl std::fmt::Debug for Font {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Font")
            .field("widths", &self.widths)
            .field("glyph_sizes", &self.glyph_sizes.is_some())
            .finish()
    }
}

impl Font {
    /// Measures `sheet`, with the optional `glyph_sizes` table for non-ASCII characters.
    ///
    /// The widths are computed by the source's scan: for each printable ASCII character,
    /// scan the cell's columns from the right for the rightmost one holding any non-zero
    /// alpha; the width is `(int)(0.5 + (rightmost + 1) * 8 / cellWidth) + 1`, so on the
    /// jar's 8-texel cells a cell inked in columns `0..=k` measures `k + 2` and a blank cell
    /// measures 1. The space keeps the source's own 4 rather than its blank cell's 1.
    ///
    /// `glyph_sizes` must be exactly [`GLYPH_SIZES_LEN`] bytes when given; a different length
    /// is [`FontError::GlyphSizesLength`]. A sheet whose axes do not divide into the 16x16
    /// grid, or whose cells would be empty, is [`FontError::SheetSize`].
    pub fn load(sheet: &Texture, glyph_sizes: Option<&[u8]>) -> Result<Font, FontError> {
        if sheet.width % GRID != 0 || sheet.height % GRID != 0 {
            return Err(FontError::SheetSize {
                width: sheet.width,
                height: sheet.height,
            });
        }
        let cell_width = sheet.width / GRID;
        let cell_height = sheet.height / GRID;
        if cell_width == 0 || cell_height == 0 {
            return Err(FontError::SheetSize {
                width: sheet.width,
                height: sheet.height,
            });
        }

        let mut widths = [0u32; PRINTABLE_COUNT];
        for code in FIRST_PRINTABLE..=LAST_PRINTABLE {
            widths[(code - FIRST_PRINTABLE) as usize] =
                scanned_width(sheet, code, cell_width, cell_height);
        }
        widths[0] = SPACE_ADVANCE;

        let glyph_sizes = match glyph_sizes {
            Some(bytes) => Some(Box::new(
                <[u8; GLYPH_SIZES_LEN]>::try_from(bytes)
                    .map_err(|_| FontError::GlyphSizesLength { got: bytes.len() })?,
            )),
            None => None,
        };

        Ok(Font {
            widths,
            glyph_sizes,
        })
    }

    /// The advance of `character` in font pixels, following `getCharWidth` (`:660-698`).
    ///
    /// A printable ASCII character answers its scanned width, the space the source's 4, and
    /// anything else the half-scale width of its `glyph_sizes` entry — start and end columns
    /// packed in one byte, with the source's `end > 7` arm — or 0 when there is no table, no
    /// entry, or no UTF-16 code unit for the character.
    pub fn advance(&self, character: char) -> u32 {
        let code = character as u32;
        if code == FIRST_PRINTABLE {
            return SPACE_ADVANCE;
        }
        if (FIRST_PRINTABLE..=LAST_PRINTABLE).contains(&code) {
            return self.widths[(code - FIRST_PRINTABLE) as usize];
        }
        let Some(table) = &self.glyph_sizes else {
            return 0;
        };
        if code as usize >= GLYPH_SIZES_LEN {
            return 0;
        }
        half_scale_width(table[code as usize])
    }

    /// The cell box of a printable ASCII character: its origin and its `charWidth - 1` by 8
    /// box in sheet texels, or `None` outside printable ASCII.
    ///
    /// The box is the source's own quad size before the trimming constants: `charWidth - 1`
    /// wide (`renderDefaultChar`'s `f = charWidth - 0.01` ends at `charWidth - 1.01`) and one
    /// cell tall. A blank character's box is zero wide.
    pub fn glyph_rect(&self, character: char) -> Option<(u32, u32, u32, u32)> {
        let code = character as u32;
        if !(FIRST_PRINTABLE..=LAST_PRINTABLE).contains(&code) {
            return None;
        }
        let column = code % GRID;
        let row = code / GRID;
        let width = self.widths[(code - FIRST_PRINTABLE) as usize];
        Some((column * CELL_SIDE, row * CELL_SIDE, width - 1, CELL_SIDE))
    }

    /// The height of one line of text in font pixels: `FontRenderer.FONT_HEIGHT`, 9.
    pub fn height(&self) -> u32 {
        LINE_HEIGHT
    }
}

/// The source's scan for one printable code: the rightmost inked column plus one, through
/// `(int)(0.5 + i2 * 8 / cellWidth) + 1`.
fn scanned_width(sheet: &Texture, code: u32, cell_width: u32, cell_height: u32) -> u32 {
    let column = (code % GRID) * cell_width;
    let row = (code / GRID) * cell_height;
    let mut ink_columns = 0u32;
    let mut offset = cell_width;
    while offset > 0 {
        offset -= 1;
        if column_has_ink(sheet, column + offset, row, cell_height) {
            ink_columns = offset + 1;
            break;
        }
    }
    let scale = CELL_SIDE as f32 / cell_width as f32;
    (0.5 + ink_columns as f32 * scale) as u32 + 1
}

/// Whether the texel column `x` holds any non-zero alpha in the cell rows `row..row + height`.
fn column_has_ink(sheet: &Texture, x: u32, row: u32, height: u32) -> bool {
    (row..row + height).any(|y| {
        let alpha = sheet.rgba[((y * sheet.width + x) * 4 + 3) as usize];
        alpha != 0
    })
}

/// The source's half-scale width of a `glyph_sizes` byte (`getCharWidth`, `:676-689`): the
/// high nibble is the glyph's start column and the low nibble its end column, an end above 7
/// means the whole row, and the width is `(end + 1 - start) / 2 + 1`. A zero byte has no
/// entry, so the width is zero.
fn half_scale_width(entry: u8) -> u32 {
    if entry == 0 {
        return 0;
    }
    let mut start = u32::from(entry >> 4);
    let mut end = u32::from(entry & 15);
    if end > 7 {
        end = 15;
        start = 0;
    }
    end += 1;
    (end - start) / 2 + 1
}
