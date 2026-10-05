//! The debug overlay's pure layout: one textured quad per glyph of the jar's ascii font.
//!
//! The overlay lays text out in physical pixels from a top-left origin. The layout arithmetic
//! itself — the pen and line metrics, the full-cell quads and the shadow copies — lives in
//! [`crate::text`]'s [`TextBuilder`]; this module keeps the overlay's own shape (a list of
//! lines at one origin and one scale) and maps the shared builder's vertices into the
//! overlay's physical-pixel [`GlyphVertex`]. The overlay's quads are pinned by its own suite
//! (`tests/overlay_geometry.rs`).
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

use crate::text::TextBuilder;

pub use crate::text::shadow_colour;

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

/// Lays `lines` out as textured glyph quads, the shadow copies first, in physical pixels.
///
/// `origin` is the top-left corner of the first line in physical pixels, `scale` is the text
/// scale in physical pixels per font pixel and `colour` the text's RGBA colour. Every line
/// starts at `origin.x`; lines stack at [`Font::height`] font pixels times `scale`. Within a
/// line the pen advances by [`Font::advance`] per character times `scale`, and every printable
/// character except the space draws one quad covering its whole 8x8-texel cell, positioned at
/// the pen, with the cell's rect over the sheet as its uv. The `§` codes of the shared
/// decoder apply ([`crate::text::decode_legacy`]); the overlay's own lines carry none. The
/// shadow copy is the same quads one font pixel down and right, in [`shadow_colour`]; it
/// comes first, so the text lands over it where the two overlap.
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
    let mut builder = TextBuilder::new();
    for (index, line) in lines.iter().enumerate() {
        let top = origin[1] + index as f32 * font.height() as f32 * scale;
        builder.push(line, [origin[0], top, 0.0], scale, colour, true);
    }
    let (vertices, indices) = builder.geometry(font, sheet);
    let vertices = vertices
        .into_iter()
        .map(|vertex| GlyphVertex {
            position: [vertex.position[0], vertex.position[1]],
            uv: vertex.uv,
            color: vertex.colour,
        })
        .collect();
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    //! The layout's arithmetic, on a synthetic sheet with no GPU and no file.

    use super::{GlyphVertex, glyph_geometry, shadow_colour};
    use oxide_assets::font::Font;
    use oxide_assets::texture::Texture;

    /// The sheet's side in texels: the real `font/ascii.png` shape.
    const SIDE: u32 = 128;
    /// The side of one glyph cell in texels.
    const CELL: u32 = 8;

    /// A 128x128 synthetic sheet whose `'A'` cell inks columns 0..=4, so the scan measures
    /// six font pixels.
    fn a_sheet() -> Texture {
        let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
        let code = 'A' as u32;
        let cell_x = (code % 16) * CELL;
        let cell_y = (code / 16) * CELL;
        for row in 0..CELL {
            for column in 0..=4 {
                let offset = (((cell_y + row) * SIDE + cell_x + column) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        Texture {
            width: SIDE,
            height: SIDE,
            rgba,
        }
    }

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
    fn one_glyph_is_one_quad_of_two_triangles_in_the_overlays_own_vertices() {
        let sheet = a_sheet();
        let font = Font::load(&sheet, None).expect("the synthetic sheet loads");
        let (vertices, indices) = glyph_geometry(
            &font,
            (SIDE, SIDE),
            &["A".to_string()],
            [4.0, 4.0],
            2.0,
            [1.0, 1.0, 1.0, 1.0],
        );
        assert_eq!(vertices.len(), 8, "the shadow quad and the text quad");
        assert_eq!(indices, [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]);
        // The shadow comes first, one font pixel (two physical pixels here) down and right,
        // in the source's darkened colour.
        assert_eq!(vertices[0].position, [6.0, 6.0]);
        assert_eq!(vertices[0].color, shadow_colour([1.0, 1.0, 1.0, 1.0]));
        // The text's cell quad: eight font pixels at scale two, from the origin, its uv the
        // whole `'A'` cell — (8, 32) to (16, 40) over the sheet.
        assert_eq!(
            vertices[4],
            GlyphVertex {
                position: [4.0, 4.0],
                uv: [8.0 / 128.0, 32.0 / 128.0],
                color: [1.0, 1.0, 1.0, 1.0],
            }
        );
        assert_eq!(
            vertices[6].position,
            [20.0, 20.0],
            "the text quad's far corner"
        );
        assert_eq!(vertices[6].uv, [16.0 / 128.0, 40.0 / 128.0]);
    }
}
