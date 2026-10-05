//! The shared text builder: the legacy `§` decoder, the glyph-quad layout and the font's
//! width law — the pieces the debug overlay and the entity pass's nametags both draw with.
//!
//! A [`TextBuilder`] buffers text draws — a string, a position, a scale, a colour and the
//! shadow flag — and lays them out as glyph quads on demand: for each draw the shadow copy
//! is laid out first, one font pixel down and right in the source's darkened colour, and the
//! text lands over it. Every glyph covers its whole 8x8-texel cell; the equivalence with the
//! source's trimmed quad is [`crate::debug_text`]'s module note.
//!
//! The `§` run decoder follows `FontRenderer.renderStringAtPos` (`FontRenderer.java`:392-556):
//! the character after a `§` is lower-cased and looked up in `0123456789abcdefklmnor`
//! (`:400`); the first sixteen entries are colour codes that clear every style and set the
//! run's colour, any other unknown character is *swallowed* and selects white (the source's
//! clamp of the not-found index to 15, `:410-413`), `k` selects obfuscation, `l` bold, `m`
//! strikethrough, `n` underline, `o` italic and `r` resets the styles and returns the colour
//! to the draw's base (`:424-452`). A trailing `§` with no character after it is ordinary
//! text (`:398`). Bold and italic change the glyph geometry — bold draws the glyph twice,
//! the second copy one pixel along, and adds the pixel to the advance; italic shears the
//! quad by one texel from top to bottom (`:479-516`, `:248-267`). The underline and
//! strikethrough bars the source also paints (`:518-545`) decode into the runs but draw
//! nothing here: they are untextured quads and need a colour-only fragment, which the caller
//! that first draws styled text supplies.
//!
//! The width law is `FontRenderer.getStringWidth` (`FontRenderer.java`:607-653), quirks
//! included: a `§` with a following character advances nothing, a trailing or isolated `§`
//! contributes minus one (`:621-638`), and the bold flag — set only by `l`/`L`, cleared only
//! by `r`/`R` — keeps adding one pixel per glyph even across colour codes, while the
//! renderer clears bold on any colour code.
//!
//! The shadow colour is the source's byte rule: `FontRenderer.renderString`:582-590 rewrites
//! the draw colour per channel, `(colour & 0x00FCFCFC) >> 2`, the alpha kept — [`shadow_colour`].

use oxide_assets::font::Font;

/// The side of one glyph cell in texels, the source renderer's own 8.
const CELL_SIDE: f32 = 8.0;

/// The `§` code table (`FontRenderer.renderStringAtPos`, `FontRenderer.java`:400): the
/// sixteen colour codes, then the six style codes, in the source's own order.
const CODES: &str = "0123456789abcdefklmnor";

/// One style and colour run of a decoded legacy text: `text` carries the characters the run
/// drew, every other field the state a `§` code left in force when the run began.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StyledRun {
    /// The run's characters, the `§` codes removed.
    pub text: String,
    /// The run's colour, `0..1` RGB, when a colour code set one; `None` keeps the draw's.
    pub colour: Option<[f32; 3]>,
    /// Whether `§k`'s obfuscation is in force.
    pub obfuscated: bool,
    /// Whether `§l`'s bold is in force.
    pub bold: bool,
    /// Whether `§m`'s strikethrough is in force.
    pub strikethrough: bool,
    /// Whether `§n`'s underline is in force.
    pub underline: bool,
    /// Whether `§o`'s italic is in force.
    pub italic: bool,
}

/// One text vertex: the position in the draw's own units, the sheet uv and the RGBA colour
/// the fragment multiplies the texel by.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextVertex {
    /// The position in the draw's units: font pixels times the draw's scale, offset by the
    /// draw's origin.
    pub position: [f32; 3],
    /// The font sheet's uv, `(0, 0)` at the sheet's top-left corner.
    pub uv: [f32; 2],
    /// The straight (non-premultiplied) RGBA colour the fragment multiplies the texel by.
    pub colour: [f32; 4],
}

/// The legacy colour code table (`FontRenderer`'s constructor, `FontRenderer.java`:107-140):
/// the sixteen classic colours and their darkened shadow half, `0..1` RGB.
///
/// `index` 0..16 is the classic palette — entry 6 is the source's own gold patch, red raised
/// by 85 (`:115-118`) — and 16..32 the same channels divided by four (`:130-134`).
pub fn colour_code(index: u8) -> [f32; 3] {
    let index = u32::from(index % 32);
    let base = ((index >> 3) & 1) * 85;
    let mut red = ((index >> 2) & 1) * 170 + base;
    let green = ((index >> 1) & 1) * 170 + base;
    let blue = (index & 1) * 170 + base;
    if index == 6 {
        red += 85;
    }
    let (red, green, blue) = if index >= 16 {
        (red / 4, green / 4, blue / 4)
    } else {
        (red, green, blue)
    };
    [
        red as f32 / 255.0,
        green as f32 / 255.0,
        blue as f32 / 255.0,
    ]
}

/// The source's shadow colour for `colour`: `(colour & 0x00FCFCFC) >> 2 | colour & 0xFF000000`
/// (`FontRenderer.renderString`, `FontRenderer.java`:582-590), the alpha byte kept.
///
/// For opaque white the result is `0xFF3F3F3F`: 63 per channel, alpha 1.
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

/// Decodes a legacy `§`-coded text into styled runs, in draw order.
///
/// Each `§` code closes the run before it and applies to the run that follows; codes at the
/// same spot compose (a style code keeps the attributes already in force). A colour code or
/// an unknown code closes and clears every style (`FontRenderer.java`:404-408, `:410-413`);
/// `§r` returns the colour to `None`, the draw's base (`:444-452`). Empty runs are dropped.
pub fn decode_legacy(text: &str) -> Vec<StyledRun> {
    let mut runs: Vec<StyledRun> = Vec::new();
    let mut current = StyledRun::default();
    let characters: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < characters.len() {
        let character = characters[index];
        if character == '§' && index + 1 < characters.len() {
            if !current.text.is_empty() {
                runs.push(current.clone());
                current.text.clear();
            }
            // The source lower-cases the whole string before indexing; for the ASCII codes
            // the table holds, lower-casing the one character is the same lookup.
            let code = characters[index + 1].to_ascii_lowercase();
            match CODES.find(code) {
                Some(position) if position < 16 => {
                    current.colour = Some(colour_code(position as u8));
                    current.obfuscated = false;
                    current.bold = false;
                    current.strikethrough = false;
                    current.underline = false;
                    current.italic = false;
                }
                None => {
                    // The source clamps the not-found index to 15: white, styles cleared.
                    current.colour = Some(colour_code(15));
                    current.obfuscated = false;
                    current.bold = false;
                    current.strikethrough = false;
                    current.underline = false;
                    current.italic = false;
                }
                Some(16) => current.obfuscated = true,
                Some(17) => current.bold = true,
                Some(18) => current.strikethrough = true,
                Some(19) => current.underline = true,
                Some(20) => current.italic = true,
                Some(21) => {
                    current.colour = None;
                    current.obfuscated = false;
                    current.bold = false;
                    current.strikethrough = false;
                    current.underline = false;
                    current.italic = false;
                }
                Some(_) => unreachable!("the table's twenty-two entries are handled"),
            }
            index += 2;
        } else {
            current.text.push(character);
            index += 1;
        }
    }
    if !current.text.is_empty() {
        runs.push(current);
    }
    runs
}

/// The source's `getStringWidth` (`FontRenderer.getStringWidth`, `FontRenderer.java`:607-653),
/// in font pixels.
///
/// A `§` with a following character consumes it at zero width — the loop's own `++j` skips it
/// — and a trailing `§` contributes minus one; while the bold flag is set every glyph of
/// positive width adds one (`:638-646`). The flag is set by `l`/`L` and cleared only by
/// `r`/`R`, not by colour codes (`:628-638`).
pub fn string_width(font: &Font, text: &str) -> i32 {
    let characters: Vec<char> = text.chars().collect();
    let mut width = 0;
    let mut bold = false;
    let mut index = 0;
    while index < characters.len() {
        let character = characters[index];
        let mut advance = if character == '§' {
            -1
        } else {
            font.advance(character) as i32
        };
        if advance < 0 && index < characters.len() - 1 {
            index += 1;
            let code = characters[index];
            if code != 'l' && code != 'L' {
                if code == 'r' || code == 'R' {
                    bold = false;
                }
            } else {
                bold = true;
            }
            advance = 0;
        }
        width += advance;
        if bold && advance > 0 {
            width += 1;
        }
        index += 1;
    }
    width
}

/// Appends one draw's runs as glyph quads to `vertices`/`indices`, in the draw's own units,
/// and returns the pen's end x.
///
/// `pen` is the run's start — font pixels times `scale`, offset by the draw's origin — and
/// `colour` the draw's base RGBA. A run's own colour keeps `colour`'s alpha; under `shadow`
/// every colour, a run's own included, goes through [`shadow_colour`] (`FontRenderer.java`:
/// the shadow pass runs the whole string at the darkened colour, `:341-353`, and the colour
/// codes select the table's shadow half, `:415-418`). Bold draws the glyph twice, one font
/// pixel apart, and adds one pixel to its advance; italic shears the cell one texel
/// (`:479-516`, `:248-267`).
// One argument per input of the run pass; a bundle would only move the same list one level down.
#[allow(clippy::too_many_arguments)]
pub fn draw_runs(
    font: &Font,
    sheet: (u32, u32),
    runs: &[StyledRun],
    pen: [f32; 3],
    scale: f32,
    colour: [f32; 4],
    shadow: bool,
    vertices: &mut Vec<TextVertex>,
    indices: &mut Vec<u32>,
) -> f32 {
    let mut pen_x = pen[0];
    for run in runs {
        let mut run_colour = match run.colour {
            Some([red, green, blue]) => [red, green, blue, colour[3]],
            None => colour,
        };
        if shadow {
            run_colour = shadow_colour(run_colour);
        }
        let shear = if run.italic { 1.0 } else { 0.0 };
        let bold = if run.bold { 1.0 } else { 0.0 };
        for character in run.text.chars() {
            // The source's `renderChar` returns before drawing anything for the space
            // (`FontRenderer.java`:223-227).
            let glyph = if character == ' ' {
                None
            } else {
                font.glyph_rect(character)
            };
            if let Some((cell_x, cell_y, _, _)) = glyph {
                quad(
                    vertices,
                    indices,
                    sheet,
                    (cell_x, cell_y),
                    [pen_x, pen[1], pen[2]],
                    scale,
                    shear,
                    run_colour,
                );
                if run.bold {
                    // The bold copy one pixel along (`FontRenderer.java`:496-506).
                    quad(
                        vertices,
                        indices,
                        sheet,
                        (cell_x, cell_y),
                        [pen_x + scale, pen[1], pen[2]],
                        scale,
                        shear,
                        run_colour,
                    );
                }
            }
            pen_x += (font.advance(character) as f32 + bold) * scale;
        }
    }
    pen_x
}

/// Appends one full-cell glyph quad at `position` (the cell's top-left corner), sheared by
/// `shear` font pixels from top to bottom for the italic style.
// One argument per input of the quad; a bundle would only move the same list one level down.
#[allow(clippy::too_many_arguments)]
fn quad(
    vertices: &mut Vec<TextVertex>,
    indices: &mut Vec<u32>,
    sheet: (u32, u32),
    cell: (u32, u32),
    position: [f32; 3],
    scale: f32,
    shear: f32,
    colour: [f32; 4],
) {
    let [left, top, z] = position;
    let right = left + CELL_SIDE * scale;
    let bottom = top + CELL_SIDE * scale;
    let k = shear * scale;
    let u0 = cell.0 as f32 / sheet.0 as f32;
    let v0 = cell.1 as f32 / sheet.1 as f32;
    let u1 = (cell.0 as f32 + CELL_SIDE) / sheet.0 as f32;
    let v1 = (cell.1 as f32 + CELL_SIDE) / sheet.1 as f32;
    let base = vertices.len() as u32;
    for (position, uv) in [
        ([left + k, top, z], [u0, v0]),
        ([left - k, bottom, z], [u0, v1]),
        ([right - k, bottom, z], [u1, v1]),
        ([right + k, top, z], [u1, v0]),
    ] {
        vertices.push(TextVertex {
            position,
            uv,
            colour,
        });
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// One buffered text draw.
#[derive(Debug, Clone, PartialEq)]
struct TextDraw {
    /// The draw's legacy `§`-coded text.
    text: String,
    /// The pen's start: font pixels times the draw's scale, offset by the origin.
    at: [f32; 3],
    /// The draw's scale, in the draw's own units per font pixel.
    scale: f32,
    /// The draw's base RGBA colour.
    colour: [f32; 4],
    /// Whether the shadow copy draws under the text.
    shadow: bool,
}

/// The shared text builder: a buffer of text draws laid out as glyph quads on demand.
///
/// The overlay pushes its lines with the physical-pixel scale and reads the quads back in
/// its own vertex format; a nametag pushes one draw with the font-pixel scale of one and
/// maps the quads through its billboard. The shadow copies come first in the returned
/// buffers, all draws' before any text's, the source's own order (`FontRenderer.drawString`
/// runs the shadow pass at `(x + 1, y + 1)` before the text, `FontRenderer.java`:341-358).
#[derive(Debug, Clone, Default)]
pub struct TextBuilder {
    /// The buffered draws, in push order.
    draws: Vec<TextDraw>,
}

impl TextBuilder {
    /// An empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Buffers one text draw.
    ///
    /// `at` is the pen's start — font pixels times `scale`, offset by the draw's origin —
    /// `scale` the draw's units per font pixel, `colour` the base RGBA and `shadow` whether
    /// the darkened copy one font pixel down and right draws under the text.
    pub fn push(&mut self, text: &str, at: [f32; 3], scale: f32, colour: [f32; 4], shadow: bool) {
        self.draws.push(TextDraw {
            text: text.to_string(),
            at,
            scale,
            colour,
            shadow,
        });
    }

    /// Lays the buffered draws out as glyph quads and triangle-list indices.
    ///
    /// The shadow copies first — every shadowed draw's, in push order — then every draw's
    /// text, all in the same order: the source's two passes.
    pub fn geometry(&self, font: &Font, sheet: (u32, u32)) -> (Vec<TextVertex>, Vec<u32>) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let decoded: Vec<Vec<StyledRun>> = self
            .draws
            .iter()
            .map(|draw| decode_legacy(&draw.text))
            .collect();
        for copy_shadow in [true, false] {
            for (draw, runs) in self.draws.iter().zip(&decoded) {
                if copy_shadow && !draw.shadow {
                    continue;
                }
                let offset = if copy_shadow { draw.scale } else { 0.0 };
                let pen = [draw.at[0] + offset, draw.at[1] + offset, draw.at[2]];
                draw_runs(
                    font,
                    sheet,
                    runs,
                    pen,
                    draw.scale,
                    draw.colour,
                    copy_shadow,
                    &mut vertices,
                    &mut indices,
                );
            }
        }
        (vertices, indices)
    }
}

#[cfg(test)]
mod tests {
    //! The decoder's table, the width law and the layout's arithmetic, without a GPU.

    use super::*;
    use oxide_assets::texture::Texture;

    /// The synthetic sheet's side in texels: the real `font/ascii.png` shape.
    const SIDE: u32 = 128;
    /// The side of one glyph cell in texels.
    const CELL: u32 = 8;
    /// The colour the layout tests draw in: opaque white.
    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

    /// A 128x128 synthetic sheet whose `'A'` cell carries the store-verified ink pattern:
    /// ink in columns 0..=4, so the scan measures six font pixels.
    fn sheet() -> Texture {
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

    /// The synthetic sheet's measured font.
    fn font() -> Font {
        Font::load(&sheet(), None).expect("the synthetic sheet loads")
    }

    #[test]
    fn the_colour_table_is_the_sources_palette_and_its_shadow_half() {
        assert_eq!(colour_code(0), [0.0, 0.0, 0.0]);
        // Entry 6 is the source's gold patch: red raised by 85 over the plain ramp.
        assert_eq!(colour_code(6), [1.0, 170.0 / 255.0, 0.0]);
        assert_eq!(colour_code(15), [1.0, 1.0, 1.0]);
        // Entries 16..32 are the base channels divided by four, integer division: 0xAA / 4
        // is 42. Index 20 is 16 + 4: the dark red's shadow; index 31 the white's.
        assert_eq!(colour_code(20), [42.0 / 255.0, 0.0, 0.0]);
        assert_eq!(colour_code(21), [42.0 / 255.0, 0.0, 42.0 / 255.0]);
        assert_eq!(colour_code(31), [63.0 / 255.0, 63.0 / 255.0, 63.0 / 255.0]);
        // The shadow half and the shadow-copy byte rule agree.
        let shadow = shadow_colour([170.0 / 255.0, 0.0, 170.0 / 255.0, 1.0]);
        assert_eq!(colour_code(21), [shadow[0], shadow[1], shadow[2]]);
    }

    #[test]
    fn every_colour_code_selects_its_table_entry() {
        for (position, digit) in "0123456789abcdef".chars().enumerate() {
            let text = format!("§{digit}x");
            let runs = decode_legacy(&text);
            assert_eq!(runs.len(), 1, "one coloured run for {text}");
            assert_eq!(runs[0].text, "x");
            assert_eq!(
                runs[0].colour,
                Some(colour_code(position as u8)),
                "the entry for {text}"
            );
        }
    }

    #[test]
    fn each_style_code_sets_its_own_style() {
        let case = |text: &str| decode_legacy(text).remove(0);
        let run = case("§kA");
        assert!(run.obfuscated && !run.bold && !run.strikethrough && !run.underline && !run.italic);
        let run = case("§lA");
        assert!(run.bold && !run.obfuscated && !run.italic);
        let run = case("§mA");
        assert!(run.strikethrough && !run.underline && !run.bold);
        let run = case("§nA");
        assert!(run.underline && !run.strikethrough && !run.italic);
        let run = case("§oA");
        assert!(run.italic && !run.bold && !run.underline);
        // The code character lower-cases: `§L` is bold.
        assert!(case("§LA").bold);
    }

    #[test]
    fn the_reset_returns_to_the_base_colour_and_clears_every_style() {
        let runs = decode_legacy("§l§4a§rb");
        assert_eq!(runs.len(), 2, "the code positions split the runs");
        assert_eq!(runs[0].text, "a");
        assert_eq!(runs[0].colour, Some(colour_code(4)), "the last colour code");
        assert!(!runs[0].bold, "the colour code cleared the bold");
        assert_eq!(runs[1].text, "b");
        assert_eq!(runs[1].colour, None, "§r returns to the draw's own colour");
        assert!(!runs[1].bold);
    }

    #[test]
    fn a_mid_string_colour_code_splits_the_run() {
        let runs = decode_legacy("ab§4cd");
        assert_eq!(runs.len(), 2);
        assert_eq!((runs[0].text.as_str(), runs[0].colour), ("ab", None));
        assert_eq!(
            (runs[1].text.as_str(), runs[1].colour),
            ("cd", Some(colour_code(4)))
        );
    }

    #[test]
    fn an_unknown_code_selects_white_and_swallows_its_character() {
        // '!' and 'g' are not in the table; both select white, cleared of styles, and both
        // characters are consumed.
        for text in ["a§!b", "a§gb"] {
            let runs = decode_legacy(text);
            assert_eq!(runs.len(), 2, "the runs around {text}");
            assert_eq!(runs[0].text, "a");
            assert_eq!(runs[1].text, "b", "the code character is swallowed");
            assert_eq!(
                runs[1].colour,
                Some(colour_code(15)),
                "white for the unknown code in {text}"
            );
        }
    }

    #[test]
    fn a_trailing_section_sign_is_text_and_weighs_minus_one() {
        let runs = decode_legacy("a§");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "a§", "no following character: ordinary text");
        let font = font();
        assert_eq!(
            string_width(&font, "a§"),
            string_width(&font, "a") - 1,
            "the trailing §'s own quirk"
        );
    }

    #[test]
    fn the_width_law_matches_the_sources_quirks() {
        let font = font();
        assert_eq!(
            string_width(&font, "A"),
            6,
            "the store-verified 'A' pattern"
        );
        assert_eq!(string_width(&font, "A A"), 16, "the space advances four");
        assert_eq!(
            string_width(&font, "§4A"),
            6,
            "a colour code advances nothing"
        );
        assert_eq!(string_width(&font, "§lA"), 7, "bold adds one per glyph");
        assert_eq!(string_width(&font, "§lAA"), 14);
        assert_eq!(
            string_width(&font, "§lA§4A"),
            14,
            "a colour code does not clear the width's bold flag"
        );
        assert_eq!(string_width(&font, "§lA§rA"), 13, "§r clears it");
    }

    #[test]
    fn the_builder_lays_the_shadow_copy_first_one_font_pixel_down_and_right() {
        let font = font();
        let mut builder = TextBuilder::new();
        builder.push("A", [4.0, 4.0, 0.0], 2.0, WHITE, true);
        let (vertices, indices) = builder.geometry(&font, (SIDE, SIDE));
        assert_eq!(vertices.len(), 8);
        assert_eq!(indices, [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]);
        // The shadow first: the same quad two pixels down and right (one font pixel at
        // scale two), in the source's darkened colour.
        assert_eq!(vertices[0].position, [6.0, 6.0, 0.0]);
        assert_eq!(vertices[0].colour, shadow_colour(WHITE));
        assert_eq!(vertices[4].position, [4.0, 4.0, 0.0]);
        // 'A' is code 65: column 1, row 4, so the cell's uv starts at (8, 32).
        assert_eq!(vertices[4].uv, [8.0 / 128.0, 32.0 / 128.0]);
    }

    #[test]
    fn a_bold_run_draws_the_glyph_twice_and_advances_one_pixel_further() {
        let font = font();
        let mut builder = TextBuilder::new();
        builder.push("§lA", [0.0, 0.0, 0.0], 1.0, WHITE, false);
        let (vertices, _) = builder.geometry(&font, (SIDE, SIDE));
        assert_eq!(vertices.len(), 8, "two copies of the one glyph");
        assert_eq!(vertices[0].position[0], 0.0);
        assert_eq!(
            vertices[4].position[0], 1.0,
            "the second copy one font pixel along"
        );
        let mut builder = TextBuilder::new();
        builder.push("§lAA", [0.0, 0.0, 0.0], 1.0, WHITE, false);
        let (vertices, _) = builder.geometry(&font, (SIDE, SIDE));
        assert_eq!(vertices.len(), 16);
        assert_eq!(
            vertices[8].position[0], 7.0,
            "six plus the bold pixel after the first glyph"
        );
    }

    #[test]
    fn an_italic_run_shears_the_cell_one_texel() {
        let font = font();
        let mut builder = TextBuilder::new();
        builder.push("§oA", [0.0, 0.0, 0.0], 1.0, WHITE, false);
        let (vertices, _) = builder.geometry(&font, (SIDE, SIDE));
        assert_eq!(
            vertices[0].position,
            [1.0, 0.0, 0.0],
            "the top edge leans right"
        );
        assert_eq!(
            vertices[1].position,
            [-1.0, 8.0, 0.0],
            "the bottom edge leans left"
        );
        assert_eq!(vertices[2].position, [7.0, 8.0, 0.0]);
        assert_eq!(vertices[3].position, [9.0, 0.0, 0.0]);
    }

    #[test]
    fn a_colour_run_keeps_the_draws_alpha_and_darkens_under_the_shadow_copy() {
        let font = font();
        let mut builder = TextBuilder::new();
        let faint = [1.0, 1.0, 1.0, 32.0 / 255.0];
        builder.push("§4A", [0.0, 0.0, 0.0], 1.0, faint, true);
        let (vertices, _) = builder.geometry(&font, (SIDE, SIDE));
        // The shadow copy of the red run: 0xAA0000's channels quartered, the alpha kept
        // (`FontRenderer.java`:582-590 and the table's shadow half, `:130-134`).
        assert_eq!(vertices[0].colour, [42.0 / 255.0, 0.0, 0.0, 32.0 / 255.0]);
        // The text copy: the table's red with the draw's alpha.
        assert_eq!(vertices[4].colour, [170.0 / 255.0, 0.0, 0.0, 32.0 / 255.0]);
    }
}
