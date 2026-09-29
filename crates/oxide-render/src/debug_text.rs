//! The pure layout of the debug overlay: one textured quad per glyph of the jar's ascii font.
//!
//! The overlay lays text out in physical pixels from a top-left origin: every printable
//! character except the space becomes one quad covering its whole 8x8-texel cell, the pen
//! advances by the character's own width, lines stack at the font's 9-font-pixel height, and
//! the whole layout is scaled by the text scale. The same quads are laid out once more one
//! font pixel down and right as the shadow copy, in the source's darkened colour, and come
//! first in the returned buffer so the text lands over them.
//!
//! # The full-cell quad against the source
//!
//! `FontRenderer.renderDefaultChar` (`FontRenderer.java:248-267`) draws a glyph as a
//! `charWidth - 1.01` by 7.99 quad whose left and top edges sit at the pen and whose right
//! edge sits at `pen + charWidth - 1.01`. This module draws the whole 8x8 cell instead. The
//! two are pixel-identical under the GUI's nearest sampling, and the equivalence is worth
//! stating because the geometry here is the cached, width-independent one:
//!
//! * The scan behind `charWidth` found the rightmost inked column and set
//!   `charWidth = rightmost + 2`, so every column from `charWidth - 1` to 7 of the cell is
//!   fully transparent; a fragment the full-cell quad covers there samples a transparent
//!   texel and blends nothing.
//! * The uv is linear in the position over either quad's width, and nearest sampling picks a
//!   texel by the fragment centre's distance from the quad's left edge measured in texels;
//!   for a fragment at `pen + t` both quads map to texel column `t`, so every fragment the
//!   trimmed quad draws reads the same texel the full-cell quad gives it.
//! * The source's quad is 7.99 tall; the 1/100-texel it trims cannot move a fragment centre
//!   across the last texel row, and the sheet's last cell row is blank in any case.
//!
//! The shadow copy's colour is the source's own byte rule: [`shadow_colour`].

use oxide_assets::font::Font;

/// The side of one glyph cell in texels, the source renderer's own 8.
const CELL_SIDE: f32 = 8.0;

/// One overlay vertex: a physical-pixel position, the sheet uv and an RGBA colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphVertex {
    /// The position in physical pixels, `(0, 0)` at the window's top-left corner.
    pub position: [f32; 2],
    /// The font sheet's uv, `(0, 0)` at the sheet's top-left corner.
    pub uv: [f32; 2],
    /// The straight (non-premultiplied) RGBA colour the fragment multiplies the texel by.
    pub color: [f32; 4],
}

/// The source's shadow colour for `colour`: `(colour & 0x00FCFCFC) >> 2 | colour & 0xFF000000`
/// (`FontRenderer.renderString`, `FontRenderer.java:587-590`), the alpha byte kept.
///
/// For the overlay's opaque white the result is `0xFF3F3F3F`: 63 per channel, alpha 1.
pub fn shadow_colour(colour: [f32; 4]) -> [f32; 4] {
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u32;
    let argb = (byte(colour[3]) << 24)
        | (byte(colour[0]) << 16)
        | (byte(colour[1]) << 8)
        | byte(colour[2]);
    let shadow = (argb & 0x00FC_FCFC) >> 2 | (argb & 0xFF00_0000);
    [
        ((shadow >> 16) & 0xFF) as f32 / 255.0,
        ((shadow >> 8) & 0xFF) as f32 / 255.0,
        (shadow & 0xFF) as f32 / 255.0,
        ((shadow >> 24) & 0xFF) as f32 / 255.0,
    ]
}

/// Lays `lines` out as textured glyph quads, the shadow copies first, in physical pixels.
///
/// `origin` is the top-left corner of the first line in physical pixels, `scale` is the text
/// scale in physical pixels per font pixel and `colour` the text's RGBA colour. Every line
/// starts at `origin.x`; lines stack at [`Font::height`] font pixels times `scale`. Within a
/// line the pen advances by [`Font::advance`] per character times `scale`, and every printable
/// character except the space draws one quad covering its whole 8x8-texel cell, positioned at
/// the pen, with the cell's rect over the sheet as its uv. The shadow copy is the same quads
/// one font pixel down and right, in [`shadow_colour`]; it comes first, so the text lands over
/// it where the two overlap.
///
/// The returned index buffer is `u32` triangle-list indices into the returned vertices; an
/// empty line list returns empty buffers.
pub fn glyph_geometry(
    font: &Font,
    sheet: (u32, u32),
    lines: &[String],
    origin: [f32; 2],
    scale: f32,
    colour: [f32; 4],
) -> (Vec<GlyphVertex>, Vec<u32>) {
    let mut layout = Layout {
        sheet,
        scale,
        vertices: Vec::new(),
        indices: Vec::new(),
    };
    // The shadow copy is laid out first, one font pixel down and right of the text, in the
    // source's darkened colour; the source draws it first too (`drawString` runs the shadow
    // pass at `x + 1, y + 1` before the text, `FontRenderer.java:341-353`).
    for (copy, copy_colour) in [(1.0_f32, shadow_colour(colour)), (0.0_f32, colour)] {
        for (line_index, line) in lines.iter().enumerate() {
            let top = origin[1] + line_index as f32 * font.height() as f32 * scale + copy * scale;
            let mut pen = origin[0] + copy * scale;
            for character in line.chars() {
                // The source's `renderChar` returns before drawing anything for the space;
                // every other printable character draws its cell.
                if character != ' ' {
                    if let Some((cell_x, cell_y, _, _)) = font.glyph_rect(character) {
                        layout.quad((cell_x, cell_y), [pen, top], copy_colour);
                    }
                }
                pen += font.advance(character) as f32 * scale;
            }
        }
    }
    (layout.vertices, layout.indices)
}

/// The layout's accumulator while [`glyph_geometry`] walks the lines.
struct Layout {
    /// The sheet's size in texels, for the quads' uvs.
    sheet: (u32, u32),
    /// The text scale in physical pixels per font pixel.
    scale: f32,
    /// The vertices built so far.
    vertices: Vec<GlyphVertex>,
    /// The `u32` triangle-list indices built so far.
    indices: Vec<u32>,
}

impl Layout {
    /// Appends one full-cell quad at `position` in `colour`.
    fn quad(&mut self, cell: (u32, u32), position: [f32; 2], colour: [f32; 4]) {
        let [left, top] = position;
        let right = left + CELL_SIDE * self.scale;
        let bottom = top + CELL_SIDE * self.scale;
        let u0 = cell.0 as f32 / self.sheet.0 as f32;
        let v0 = cell.1 as f32 / self.sheet.1 as f32;
        let u1 = (cell.0 as f32 + CELL_SIDE) / self.sheet.0 as f32;
        let v1 = (cell.1 as f32 + CELL_SIDE) / self.sheet.1 as f32;
        let base = self.vertices.len() as u32;
        for (position, uv) in [
            ([left, top], [u0, v0]),
            ([left, bottom], [u0, v1]),
            ([right, bottom], [u1, v1]),
            ([right, top], [u1, v0]),
        ] {
            self.vertices.push(GlyphVertex {
                position,
                uv,
                color: colour,
            });
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

#[cfg(test)]
mod tests {
    //! The layout's arithmetic, on a synthetic sheet with no GPU and no file.

    use super::{Layout, shadow_colour};

    #[test]
    fn the_shadow_rule_darkens_the_colour_and_keeps_the_alpha() {
        // The source's own byte rule for opaque white: 0xFF3F3F3F, 63 per channel.
        assert_eq!(
            shadow_colour([1.0, 1.0, 1.0, 1.0]),
            [63.0 / 255.0, 63.0 / 255.0, 63.0 / 255.0, 1.0]
        );
        // A colour keeps its own channels through the rule: 0xFF804020 -> 0xFF201008.
        let colour = [
            0x80 as f32 / 255.0,
            0x40 as f32 / 255.0,
            0x20 as f32 / 255.0,
            1.0,
        ];
        assert_eq!(
            shadow_colour(colour),
            [
                0x20 as f32 / 255.0,
                0x10 as f32 / 255.0,
                0x08 as f32 / 255.0,
                1.0
            ]
        );
    }

    #[test]
    fn one_quad_draws_two_triangles() {
        let mut layout = Layout {
            sheet: (128, 128),
            scale: 1.0,
            vertices: Vec::new(),
            indices: Vec::new(),
        };
        layout.quad((0, 0), [0.0, 0.0], [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(layout.vertices.len(), 4);
        assert_eq!(layout.indices, [0, 1, 2, 0, 2, 3]);
        assert_eq!(layout.vertices[2].position, [8.0, 8.0]);
        assert_eq!(layout.vertices[2].uv, [8.0 / 128.0, 8.0 / 128.0]);
    }
}
