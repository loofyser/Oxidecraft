//! The client's lightmap: the 16x16 brightness texture `EntityRenderer.updateLightmap` fills
//! each time the lighting changes, and the coordinate convention that ties a vertex's packed
//! light to a texel of it.
//!
//! The texture's two axes are the two light fields of the packed pair. In the source's own
//! loop (`EntityRenderer.java:934-937`) index `i`'s high nibble `i / 16` drives the sky term —
//! the brightness table scaled by the sun's contribution — and its low nibble `i % 16` the
//! block term, the torch-light one. `DynamicTexture` uploads the 256-entry array row-major, so
//! the sky level is the row and the block level the column; and the short pair a terrain
//! vertex carries puts the block field first (`WorldRenderer.putBrightness4`, `:263-271`,
//! writing the packed int whose low half is the block field into the first lightmap short;
//! `ItemRenderer.setLightMapFromPlayer`, `:113-116`, hands the same low half to
//! `glMultiTexCoord2f` first). [`sample_index`] is that convention written down, `(u, v)` =
//! `(block, sky)`.
//!
//! The 0..256 scale a vertex's light is divided by comes from the source's texture matrix:
//! `enableLightmap` scales by `1/256` and translates by 8 on every axis
//! (`EntityRenderer.java:892-909`), so a light value `L` addresses the texture coordinate
//! `(L + 8) / 256`. A vertex here carries the level shifted four bits *with that eight already
//! added* (`oxide-game`'s `light_attribute`), so the shader's coordinate is the pair over 256
//! and lands on the cell's centre: `(level * 16 + 8) / 256 = (level + 0.5) / 16`.
//!
//! M2 builds this image once, at pass construction, from the state a fresh client is in: the
//! Overworld's table, the sun at its noon brightness and the default gamma. `updateLightmap`
//! re-fills it whenever the sun, the gamma, the torch flicker or the player's potion effects
//! change; that per-frame path is M6's, so the flicker sits at its initial zero
//! (`EntityRenderer.java:150`, read at `:937`) and the lightning, boss-colour, End and
//! night-vision branches have no parameters here.

/// The light-to-brightness table of a dimension, one entry per light level 0..=15.
///
/// M2 needed the Overworld's only ([`BrightnessTable::overworld`]). Only the Nether's provider
/// overrides the builder with a different floor (`WorldProviderHell.java:34-43`, `f = 0.1`);
/// the End inherits the Overworld's table, and the Nether's is the dimension work's to add.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrightnessTable([f32; 16]);

impl BrightnessTable {
    /// The Overworld's table, as `WorldProvider.generateLightBrightnessTable` builds it
    /// (`WorldProvider.java:63-72`): for each level, `(1 - f1) / (f1 * 3 + 1) * (1 - f) + f`
    /// with `f1 = 1 - level / 15` and the surface provider's floor `f = 0`.
    ///
    /// With the floor at zero the two `f` terms are exact no-ops, so the arithmetic here is the
    /// source's own, in the same single-precision order: level 0 is zero and level 15 one.
    pub fn overworld() -> Self {
        let mut levels = [0.0f32; 16];
        for (level, value) in levels.iter_mut().enumerate() {
            let f1 = 1.0 - (level as f32) / 15.0;
            *value = (1.0 - f1) / (f1 * 3.0 + 1.0);
        }
        Self(levels)
    }

    /// The sixteen brightnesses, indexed by light level.
    pub fn levels(&self) -> &[f32; 16] {
        &self.0
    }
}

/// The factor the block term's torch-flicker expression carries at rest:
/// `torchFlickerX * 0.1 + 1.5` with `torchFlickerX` at its initial zero
/// (`EntityRenderer.java:150`, `:937`; the flicker's own update is `:914-920`).
const FLICKER: f32 = 1.5;

/// Builds the 16x16 lightmap `EntityRenderer.updateLightmap` fills (`EntityRenderer.java:922-1057`)
/// as RGBA bytes, in the source's own index order.
///
/// Cell `i` of the 256 sits at byte `i * 4`, indexed `i = sky * 16 + block`: the row is the sky
/// level and the column the block level (see the module doc). The four bytes are red, green,
/// blue and alpha — the source packs `j << 24 | k << 16 | l << 8 | i1` (its alpha, then red,
/// green and blue) into an int and uploads the array as `GL_BGRA` with
/// `GL_UNSIGNED_INT_8_8_8_8_REV` (`TextureUtil.java:180`), so the memory bytes are blue, green,
/// red, alpha and the texel the GPU ends up with is the source's red, green, blue and its own
/// 255.
///
/// `table` is the dimension's brightness table, `sun_brightness` the value
/// `World.getSunBrightness` answers for the frame's celestial angle (`World.java:1418-1427`;
/// it is 1.0 at noon) and `gamma` the brightness setting
/// (`GameSettings.gammaSetting`, `GameSettings.java:171`; the default is zero). The arithmetic
/// below is the source's order of operations, single-precision throughout, with the terms a
/// fresh client has switched off left out: no lightning override (`:939-942`), no boss colour
/// modifier (`:955-961`), no End override (`:963-968`) and no night-vision term (`:970-988`).
pub fn lightmap_image(
    table: &BrightnessTable,
    sun_brightness: f32,
    gamma: f32,
) -> [u8; 16 * 16 * 4] {
    let sun = sun_brightness;
    let sky_factor = sun * 0.95 + 0.05;
    let sun_mix = sun * 0.65 + 0.35;
    let mut image = [0u8; 16 * 16 * 4];
    for (index, texel) in image.chunks_exact_mut(4).enumerate() {
        let sky_value = table.0[index / 16] * sky_factor;
        let block_value = table.0[index % 16] * FLICKER;
        // f4, f5 and f2's own terms, then f6 and f7, the two shaped block terms.
        let red = sky_value * sun_mix + block_value;
        let green = sky_value * sun_mix + block_value * ((block_value * 0.6 + 0.4) * 0.6 + 0.4);
        let blue = sky_value + block_value * (block_value * block_value * 0.6 + 0.4);
        // The `0.96 / 0.03` pair, the upper clamp, then the gamma blend towards
        // `1 - (1 - c)^4`, then the pair and both clamps again.
        let mut channels = [
            (red * 0.96 + 0.03).min(1.0),
            (green * 0.96 + 0.03).min(1.0),
            (blue * 0.96 + 0.03).min(1.0),
        ];
        for channel in &mut channels {
            let lifted =
                1.0 - (1.0 - *channel) * (1.0 - *channel) * (1.0 - *channel) * (1.0 - *channel);
            *channel = *channel * (1.0 - gamma) + lifted * gamma;
            *channel = (*channel * 0.96 + 0.03).clamp(0.0, 1.0);
        }
        texel[0] = (channels[0] * 255.0) as u8;
        texel[1] = (channels[1] * 255.0) as u8;
        texel[2] = (channels[2] * 255.0) as u8;
        texel[3] = 255;
    }
    image
}

/// The texel `(u, v)` a cell sits at: the block level across, the sky level down.
///
/// The two arguments are the levels of a cell's light pair in the order the image's own index
/// uses — the sky level of `i / 16` first, the block level of `i % 16` second — so the answer is
/// the texel `(u, v) = (block, sky)`. A vertex's [`crate::terrain::Vertex::light`] carries the
/// same two levels as a (block, sky) pair, so [`crate::terrain::Vertex::light`]`[1]` is this
/// function's first argument. The caller samples the image at `((level * 16 + 8) / 256)` on each
/// axis; see the module doc for where the eight comes from.
pub fn sample_index(sky: u8, block: u8) -> (u32, u32) {
    (u32::from(block), u32::from(sky))
}

#[cfg(test)]
mod tests {
    use super::{BrightnessTable, lightmap_image, sample_index};

    #[test]
    fn a_brightness_table_holds_sixteen_levels() {
        let table = BrightnessTable::overworld();
        assert_eq!(table.levels().len(), 16);
        assert_eq!(table.levels()[0], 0.0, "a dark level is dark");
        assert_eq!(table.levels()[15], 1.0, "a full level is full");
    }

    #[test]
    fn an_image_is_a_quarter_of_a_kilobyte_and_matches_the_index_convention() {
        let image = lightmap_image(&BrightnessTable::overworld(), 1.0, 0.0);
        assert_eq!(image.len(), 16 * 16 * 4);
        let at = |sky: u8, block: u8| {
            let (u, v) = sample_index(sky, block);
            let index = (v as usize) * 16 + (u as usize);
            [image[index * 4], image[index * 4 + 1], image[index * 4 + 2]]
        };
        // With the other level held at zero, each axis lifts the cell on its own: the sky
        // through the row the sky level picks and the block through the column the block
        // level picks. A transposed index would read the two cells the other way round.
        assert!(at(15, 0)[0] > at(0, 0)[0], "the sky axis lifts a cell");
        assert!(at(0, 15)[0] > at(0, 0)[0], "the block axis lifts a cell");
    }
}
