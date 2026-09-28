//! Tests of the lightmap and of the frame's fog colour. No GPU is involved.
//!
//! Every expected byte and float here is worked through from MCP-919's own arithmetic and
//! written as a literal: the brightness table from `WorldProvider.generateLightBrightnessTable`
//! (`WorldProvider.java:63-72`), the image from `EntityRenderer.updateLightmap`
//! (`EntityRenderer.java:922-1057`), the fog colour from `WorldProvider.getFogColor`
//! (`:177-188`) with `EntityRenderer.updateFogColor`'s void factor (`:1860-1887`) and the
//! celestial angle from `WorldProvider.calculateCelestialAngle` (`:115-133`).

use oxide_render::fog::{fog_colour, linear_params};
use oxide_render::lightmap::{BrightnessTable, lightmap_image, sample_index};

/// The Overworld's brightness table as literal floats: `(1 - f1) / (f1 * 3 + 1)` with
/// `f1 = 1 - i / 15` and the surface provider's floor of zero (`WorldProvider.java:63-72`),
/// i.e. `i / (60 - 3i)`. Level 0 is the dark end, level 15 the bright one.
const TABLE: [f32; 16] = [
    0.0,
    0.017543858,
    0.037037037,
    0.058823526,
    0.08333333,
    0.11111113,
    0.14285712,
    0.1794872,
    0.22222225,
    0.2727273,
    0.33333334,
    0.40740743,
    0.50000006,
    0.61904764,
    0.77777773,
    1.0,
];

/// The Overworld's fog colour at noon: `getFogColor`'s base `(0.7529412, 0.84705883, 1.0)`
/// with the day-night factor at its full value of 1, which multiplies the first two channels
/// by `1 * 0.94 + 0.06` and the third by `1 * 0.91 + 0.09` (`WorldProvider.java:177-188`).
const NOON: [f32; 3] = [0.7529412, 0.84705883, 1.0];

/// The same at midnight: the factor is clamped to 0, so the base is multiplied by
/// `(0.06, 0.06, 0.09)`.
const MIDNIGHT: [f32; 3] = [0.04517647, 0.05082353, 0.09];

/// The ceilometre's fallback: the light level of a cell as the mesher packs it into a vertex —
/// the level shifted four bits with the sampler's eight added.
fn packed(level: u8) -> u16 {
    u16::from(level) * 16 + 8
}

/// The image's cell at the two levels, addressed the way the shader's coordinate does:
/// through [`sample_index`], which returns `(u, v)` — the block level across, the sky level
/// down.
fn cell(image: &[u8; 16 * 16 * 4], sky: u8, block: u8) -> [u8; 4] {
    let (u, v) = sample_index(sky, block);
    let index = (v as usize) * 16 + (u as usize);
    let texel = &image[index * 4..index * 4 + 4];
    [texel[0], texel[1], texel[2], texel[3]]
}

#[test]
fn the_brightness_table_has_vanillas_sixteen_values() {
    assert_eq!(*BrightnessTable::overworld().levels(), TABLE);
}

#[test]
fn a_cell_is_addressed_by_the_block_level_across_and_the_sky_level_down() {
    // The light pair a vertex carries is `(block, sky)`, and the lightmap's coordinate is that
    // pair over 256: the block field picks the texel column and the sky field the row.
    assert_eq!(sample_index(0, 0), (0, 0));
    assert_eq!(sample_index(15, 15), (15, 15));
    assert_eq!(sample_index(15, 0), (0, 15), "a sky level is a row");
    assert_eq!(sample_index(0, 15), (15, 0), "a block level is a column");
    assert_eq!(sample_index(12, 4), (4, 12));
    // The convention is the one the shader's own coordinate uses: the packed pair over 256
    // lands on the cell's centre, since `(level * 16 + 8) / 256` is `(level + 0.5) / 16`.
    assert_eq!(f32::from(packed(15)) / 256.0, 15.5 / 16.0);
    assert_eq!(f32::from(packed(0)) / 256.0, 0.5 / 16.0);
}

#[test]
fn the_lightmap_floor_and_ceiling_are_the_sources_own() {
    let image = lightmap_image(&BrightnessTable::overworld(), 1.0, 0.0);
    // The darkest dark: `updateLightmap`'s own floor terms (`0.03` twice over), which is the
    // minimum of every channel because the image is monotone in both axes.
    assert_eq!(cell(&image, 0, 0), [14, 14, 14, 255], "the (0, 0) cell");
    // The brightest: the input saturates, so the `0.96 / 0.03` pair is applied once.
    assert_eq!(
        cell(&image, 15, 15),
        [252, 252, 252, 255],
        "the (15, 15) cell"
    );
    assert!(
        image.chunks_exact(4).all(|texel| texel[3] == 255),
        "every texel is opaque"
    );
    assert!(
        image.chunks_exact(4).all(|texel| texel[0] >= 14
            && texel[1] >= 14
            && texel[2] >= 14
            && texel[0] <= 252
            && texel[1] <= 252
            && texel[2] <= 252),
        "no texel leaves the floor and the ceiling"
    );
}

#[test]
fn six_cells_carry_the_sources_bytes() {
    let image = lightmap_image(&BrightnessTable::overworld(), 1.0, 0.0);
    // Worked through by hand from `updateLightmap` (`EntityRenderer.java:934-1053`) with the
    // sun at its noon brightness and no torch flicker; the (12, 4) and (4, 12) pair differ, so
    // a transposed axis order fails here.
    assert_eq!(cell(&image, 0, 0), [14, 14, 14, 255]);
    assert_eq!(cell(&image, 15, 15), [252, 252, 252, 255]);
    assert_eq!(
        cell(&image, 0, 15),
        [252, 252, 252, 255],
        "a torch-lit cell with no sky light"
    );
    assert_eq!(
        cell(&image, 15, 0),
        [250, 250, 250, 255],
        "full sky light, no torch light"
    );
    assert_eq!(cell(&image, 12, 4), [161, 152, 144, 255]);
    assert_eq!(cell(&image, 4, 12), [210, 194, 164, 255]);
}

#[test]
fn the_image_is_monotone_in_both_axes() {
    let image = lightmap_image(&BrightnessTable::overworld(), 1.0, 0.0);
    for sky in 0..16 {
        for block in 0..16 {
            let here = cell(&image, sky, block);
            for (channel, &value) in here.iter().enumerate().take(3) {
                if sky < 15 {
                    assert!(
                        cell(&image, sky + 1, block)[channel] >= value,
                        "sky {sky} -> {} at block {block}",
                        sky + 1
                    );
                }
                if block < 15 {
                    assert!(
                        cell(&image, sky, block + 1)[channel] >= value,
                        "block {block} -> {} at sky {sky}",
                        block + 1
                    );
                }
            }
        }
    }
}

#[test]
fn a_dimmer_sun_dims_the_sky_and_a_higher_gamma_lifts_the_floor() {
    // The sun's brightness reaches the image twice: through `f1 = f * 0.95 + 0.05` on the sky
    // term and through the `f * 0.65 + 0.35` pair (`EntityRenderer.java:931-945`). At a fifth
    // of the noon brightness the full-sky, unlit cell is no longer near white.
    let night = lightmap_image(&BrightnessTable::overworld(), 0.2, 0.0);
    assert_eq!(cell(&night, 15, 0), [42, 42, 71, 255]);
    // The gamma setting blends each channel towards `1 - (1 - c)^4` (`:1005-1014`); at its
    // maximum the floor cell lifts from 14 to 35 while the saturated cells do not move.
    let lifted = lightmap_image(&BrightnessTable::overworld(), 1.0, 1.0);
    assert_eq!(cell(&lifted, 0, 0), [35, 35, 35, 255]);
    assert_eq!(cell(&lifted, 15, 15), [252, 252, 252, 255]);
}

#[test]
fn the_overworld_fog_at_noon_is_the_providers_base() {
    // Noon is 6000 ticks: `calculateCelestialAngle(6000)` is 0, whose cosine is 1, so the
    // clamp leaves the factor at its maximum and the base passes through unchanged.
    assert_eq!(fog_colour(0, 6000.0, 64.0, 0.03125), NOON);
}

#[test]
fn the_overworld_fog_at_midnight_is_the_darkened_base() {
    // Midnight is 18000 ticks: the angle is 0.5, whose cosine is -1, so the factor clamps to
    // zero and only the `(0.06, 0.06, 0.09)` terms remain.
    assert_eq!(fog_colour(0, 18000.0, 64.0, 0.03125), MIDNIGHT);
}

#[test]
fn the_dusk_angle_gives_the_day_night_factor_the_cosine_says() {
    // 14000 ticks is the angle 0.25, whose cosine is zero: the factor is the raw 0.5, so the
    // first two channels take `0.5 * 0.94 + 0.06` and the third `0.5 * 0.91 + 0.09`.
    assert_eq!(
        fog_colour(0, 14000.0, 64.0, 0.03125),
        [0.39905876, 0.4489411, 0.54499996]
    );
}

#[test]
fn the_void_factor_darkens_below_the_threshold_and_clamps_at_zero() {
    // `d1 = eye_y * void_y_factor` is squared while it is below one and multiplied into the
    // colour (`EntityRenderer.java:1860-1887`). At eye height 16 with the Overworld's
    // 0.03125, `d1` is 0.5 and the colour takes a quarter of itself; at height zero, and
    // below it, `d1` clamps to zero and the fog goes black.
    assert_eq!(
        fog_colour(0, 6000.0, 16.0, 0.03125),
        [0.1882353, 0.21176471, 0.25]
    );
    assert_eq!(
        fog_colour(0, 18000.0, 16.0, 0.03125),
        [0.011294117, 0.012705882, 0.0225]
    );
    assert_eq!(fog_colour(0, 6000.0, 0.0, 0.03125), [0.0, 0.0, 0.0]);
    assert_eq!(fog_colour(0, 6000.0, -5.0, 0.03125), [0.0, 0.0, 0.0]);
}

#[test]
fn the_void_factor_leaves_the_colour_above_the_threshold() {
    // At eye height 32 the product is exactly 1, which is not below the threshold: the colour
    // is left as the base produced it.
    assert_eq!(fog_colour(0, 6000.0, 32.0, 0.03125), NOON);
    assert_eq!(fog_colour(0, 6000.0, 64.0, 0.03125), NOON);
    // A flat world's factor of 1 takes even a low camera above the threshold.
    assert_eq!(fog_colour(0, 6000.0, 4.0, 1.0), NOON);
}

#[test]
fn the_other_dimensions_keep_their_providers_fixed_bases() {
    // The Nether's provider returns one constant colour whatever the time
    // (`WorldProviderHell.java:26-29`); the End's multiplies its `0xA080A0` by the constant
    // 0.15 because the celestial-angle term carries a zero factor (`WorldProviderEnd.java:50-62`).
    assert_eq!(fog_colour(-1, 6000.0, 64.0, 0.03125), [0.2, 0.03, 0.03]);
    assert_eq!(
        fog_colour(1, 6000.0, 64.0, 0.03125),
        [0.09411766, 0.07529412, 0.09411766]
    );
    // The same fixed bases still take the void factor below the threshold.
    assert_eq!(
        fog_colour(-1, 6000.0, 16.0, 0.03125),
        [0.05, 0.0075, 0.0075]
    );
}

#[test]
fn the_default_linear_fog_starts_three_quarters_of_the_way_out() {
    assert_eq!(linear_params(128.0), (96.0, 128.0));
    assert_eq!(linear_params(0.0), (0.0, 0.0));
}
