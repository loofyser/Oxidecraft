//! The frame's fog: the colour the world fades to and the linear range the terrain fades over.
//!
//! The colour chain is `EntityRenderer.updateFogColor` (`EntityRenderer.java:1763-1928`), of
//! which M2 lands four steps, in the source's order: the per-dimension base with the day-night
//! factor (`WorldProvider.getFogColor`, `WorldProvider.java:177-188`), the sky-colour mix whose
//! strength comes from the render distance (`EntityRenderer.java:1767-1768`, `:1803-1805`), the
//! light-brightness factor the render loop's `fogColor1` converges to (`:362-365`, `:1856-1859`)
//! and the void-fog altitude factor (`:1860-1887`) — plus the default linear range `setupFog`
//! installs for terrain (`:2002-2016`), and the water arm the eye's own block
//! selects when it is water ([`water_fog_for_eye`]: EXP at density 0.1 over
//! (0.02, 0.02, 0.2), `setupFog`'s `:1985-1995` with `updateFogColor`'s
//! `:1845-1847`). The rain and thunder blends, the sunset band, the boss
//! tint, the night-vision term and the remaining per-block overrides are the
//! later scene work's.
//!
//! The colour is also the window's clear colour: the source ends `updateFogColor` by handing it
//! to `glClearColor` with an alpha of zero (`:1927`), which is how the horizon and the fog meet
//! without a seam.

use std::f32::consts::PI;

use crate::lightmap::BrightnessTable;

/// One frame's fog: the colour, the range the terrain fades over and the far plane that range
/// was derived from.
///
/// [`linear_params`] answers the range for a far plane; the colour comes from [`fog_colour`].
/// `far_plane` is the frame's own record of the distance the range was built for — the source
/// keeps the same value in `farPlaneDistance` for its sky pass and its XZ-fog branch
/// (`EntityRenderer.java:2004-2016`) — and takes no part in the shader's mix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FogParams {
    /// The colour the terrain fades to, linear in the same space as every other colour here.
    pub colour: [f32; 3],
    /// The distance the fog starts at, in blocks.
    pub start: f32,
    /// The distance the fog reaches its full strength at, in blocks.
    pub end: f32,
    /// The far plane the frame was drawn with, in blocks.
    pub far_plane: f32,
    /// The fixed-function mode the frame draws with: linear over the range above, or
    /// exponential at [`FogParams::density`].
    pub mode: FogMode,
    /// The EXP density, read only when [`FogParams::mode`] is [`FogMode::Exp`]: zero on
    /// every linear frame, so the uniform word it rides in stays the zero it always was.
    pub density: f32,
}

/// The fixed-function fog mode `setupFog` installs (`EntityRenderer.java:1960-2021`):
/// linear over a start..end range for air, EXP at a density for water, lava and clouds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FogMode {
    /// Linear fog (`GL_LINEAR`): the terrain arm's start..end fade (`:2014-2015`).
    Linear,
    /// Exponential fog (`GL_EXP`, `GlStateManager.setFog(2048)`): the water arm's
    /// density falloff (`:1985-1995`).
    Exp,
}

/// One frame's water fog: the branch `setupFog` installs when the eye block is water
/// (`EntityRenderer.java:1985-1995`), wearing the colour `updateFogColor` computes for
/// it (`:1845-1847`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterFog {
    /// Always [`FogMode::Exp`] on the water branch.
    pub mode: FogMode,
    /// The EXP density: `0.1` less `0.03` per respiration level — `0.01` flat while
    /// water breathing holds.
    pub density: f32,
    /// `(0.02, 0.02, 0.2)` plus the respiration lift on every channel.
    pub colour: [f32; 3],
}

/// The water fog for a respiration level and the water-breathing potion.
///
/// `respiration` is `EnchantmentHelper.getRespiration(entity)`'s level and
/// `water_breathing` whether `Potion.waterBreathing` is active on the view entity.
/// The colour lifts `respiration * 0.2` on every channel over the `(0.02, 0.02, 0.2)`
/// base, folded to `0.3x + 0.6` while the potion holds (`EntityRenderer.java:1845-1847`);
/// the density is `0.1` less `0.03` a level, pinned at `0.01` while the potion holds
/// (`:1985-1995`).
pub fn water_fog(respiration: u8, water_breathing: bool) -> WaterFog {
    let level = f32::from(respiration);
    let (density, lift) = if water_breathing {
        (0.01, level * 0.2 * 0.3 + 0.6)
    } else {
        (0.1 - level * 0.03, level * 0.2)
    };
    WaterFog {
        mode: FogMode::Exp,
        density,
        colour: [0.02 + lift, 0.02 + lift, 0.2 + lift],
    }
}

/// The fog branch for an eye: the water arm when the eye block is water, nothing in
/// air. `submerged` is `ActiveRenderInfo.getBlockAtEntityViewpoint`'s material test
/// (`EntityRenderer.java:1828`, `:1985`) read as a bool — the caller resolves the eye
/// block; the remaining arguments are [`water_fog`]'s.
pub fn water_fog_for_eye(
    submerged: bool,
    respiration: u8,
    water_breathing: bool,
) -> Option<WaterFog> {
    if submerged {
        Some(water_fog(respiration, water_breathing))
    } else {
        None
    }
}

/// The Overworld's fog base (`WorldProvider.getFogColor`, `WorldProvider.java:181-183`).
const SURFACE_FOG: [f32; 3] = [0.7529412, 0.84705883, 1.0];

/// The Nether's fog, one constant whatever the time
/// (`WorldProviderHell.getFogColor`, `WorldProviderHell.java:26-29`).
const NETHER_FOG: [f32; 3] = [0.2, 0.03, 0.03];

/// The End's fog: the provider's int `10518688` = `0xA080A0`, each channel over 255 and scaled
/// by its constant `0.15` (`WorldProviderEnd.getFogColor`, `WorldProviderEnd.java:50-62`, where
/// the celestial-angle term is multiplied by zero).
const END_FOG: [f32; 3] = [
    (0xA0 as f32) / 255.0 * 0.15,
    (0x80 as f32) / 255.0 * 0.15,
    (0xA0 as f32) / 255.0 * 0.15,
];

/// The frame's fog colour for a dimension, a world time, an eye height, the view block's sky
/// colour and light level, and the render distance, in `updateFogColor`'s own order.
///
/// `time_of_day` is the world's own time **in ticks** — the value `WorldInfo.getWorldTime`
/// hands `calculateCelestialAngle` (`World.getFogColor`, `World.java:1559-1563`), where 0 is
/// sunrise, 6000 noon, 12000 sunset and 18000 midnight. The celestial angle is derived here
/// with `WorldProvider.calculateCelestialAngle`'s formula (`WorldProvider.java:115-133`) at
/// zero partial ticks.
///
/// `sky_colour` is `World.getSkyColor`'s value at the render-view entity's own block
/// (`EntityRenderer.java:1769-1772`), the triple the fog mixes towards. The mix's strength is
/// `1 - (0.25 + 0.75 * render_distance_chunks / 32)^(1/4)` (`:1767-1768`) and each channel moves
/// that fraction of the way to the sky colour (`:1803-1805`) — the step that makes the horizon
/// meet the sky instead of leaving the provider's pale base standing against a deeper one.
///
/// `light_level` is the light `World.getLightBrightness` reads at the same block, 0..15
/// (`EntityRenderer.java:362`, `World.java:845-848`), and `render_distance_chunks` the client's
/// own distance setting. The provider's brightness table value is scaled by
/// `brightness * (1 - chunks / 32) + chunks / 32` (`:363-364`) — the value the render loop's
/// `fogColor1` converges to (`:362`, `:365`) and `updateFogColor` multiplies the colour by
/// (`:1856-1859`). M2 has no tick loop, so the partial tick is zero and the converged value is
/// the whole factor.
///
/// `void_y_factor` is the dimension's own `getVoidFogYFactor` (`WorldProvider.java:231-234`:
/// `0.03125` for every world type but Flat, which answers `1.0`). `eye_y` is the value the
/// source multiplies: the render-view entity's interpolated `posY` (`EntityRenderer.java:1860`),
/// the feet rather than the eye's height. The factor darkens the colour by
/// `(eye_y * void_y_factor)^2` while that product is below one, clamped above zero, and leaves
/// it alone from one upwards (`EntityRenderer.java:1876-1887`); it closes the chain.
///
/// Dimension 0 is the Overworld and is the only one the M2 acceptance compares; -1 is the
/// Nether and 1 the End, each its provider's fixed base. The day-night factor is the
/// Overworld's own — neither of the others varies with the time — while the sky mix and the
/// brightness factor are the source's for every dimension and read the Overworld's table. Any
/// other id, which vanilla's `getProviderForDimension` answers with no provider at all, takes
/// the Overworld's chain.
pub fn fog_colour(
    dimension: i8,
    time_of_day: f32,
    eye_y: f64,
    void_y_factor: f32,
    sky_colour: [f32; 3],
    render_distance_chunks: u8,
    light_level: u8,
) -> [f32; 3] {
    let base = match dimension {
        -1 => NETHER_FOG,
        1 => END_FOG,
        _ => surface_fog(celestial_angle(time_of_day)),
    };
    // The sky mix, then the light factor, per channel in the source's own order
    // (`EntityRenderer.java:1803-1805`, `:1856-1859`).
    let mix = sky_mix_strength(render_distance_chunks);
    let light = light_factor(light_level, render_distance_chunks);
    let mixed: [f32; 3] =
        std::array::from_fn(|channel| base[channel] + (sky_colour[channel] - base[channel]) * mix);
    let colour: [f32; 3] =
        std::array::from_fn(|channel| (f64::from(mixed[channel]) * f64::from(light)) as f32);
    // The void factor closes the chain, in the source's double arithmetic
    // (`EntityRenderer.java:1860-1887`).
    let mut factor = eye_y * f64::from(void_y_factor);
    if factor < 1.0 {
        if factor < 0.0 {
            factor = 0.0;
        }
        factor *= factor;
        return std::array::from_fn(|channel| (f64::from(colour[channel]) * factor) as f32);
    }
    colour
}

/// The strength the fog's sky-colour mix reaches at a render distance:
/// `1 - (0.25 + 0.75 * chunks / 32)^(1/4)` (`EntityRenderer.java:1767-1768`).
///
/// The strength grows with the render distance, because the farther the fogged world reaches
/// the more of the sky's own colour the fog has to agree with: it is 0.1867... at the
/// eight-chunk distance the acceptance captures with, and exactly zero at the 32-chunk maximum,
/// where the summed factor is one and so is its fourth root. The source's power is `Math.pow`,
/// a double operation (`:1768`), so the base widens before the root and the result narrows
/// after it.
fn sky_mix_strength(render_distance_chunks: u8) -> f32 {
    let f = 0.25 + 0.75 * f32::from(render_distance_chunks) / 32.0;
    1.0 - f64::powf(f64::from(f), 0.25) as f32
}

/// The light-brightness factor the fog colour takes at a light level and a render distance:
/// `brightness * (1 - chunks / 32) + chunks / 32` (`EntityRenderer.java:363-364`).
///
/// `brightness` is the provider's light table at the level (`World.getLightBrightness`,
/// `World.java:845-848`); the Overworld's is [`BrightnessTable`]'s, whose level 15 is one and
/// whose level 0 is zero, so a view block in full light leaves the colour alone while an unlit
/// one takes the `chunks / 32` floor — 0.25 at the eight-chunk distance. The other dimensions'
/// tables (the Nether's floor of `0.1`, `WorldProviderHell`) are not ported: M2's acceptance
/// compares the Overworld alone. The source's factor is f32 throughout.
fn light_factor(light_level: u8, render_distance_chunks: u8) -> f32 {
    let brightness = BrightnessTable::overworld().levels()[usize::from(light_level).min(15)];
    let f = f32::from(render_distance_chunks) / 32.0;
    brightness * (1.0 - f) + f
}

/// The default linear fog's start and end for a far plane: three quarters of the way out, to
/// the far plane itself (`EntityRenderer.setupFog`, `EntityRenderer.java:2014-2015`).
///
/// This is the terrain arm; the sky pass installs a range from zero to the far plane instead
/// (`:2009-2010`), which is the sky work's.
pub fn linear_params(far_plane: f32) -> (f32, f32) {
    (far_plane * 0.75, far_plane)
}

/// The Overworld's fog base with the day-night factor applied
/// (`WorldProvider.getFogColor`, `WorldProvider.java:177-188`).
///
/// The factor is `cos(celestialAngle * 2pi) * 2 + 0.5` clamped to 0..1 — it is 1 at noon and
/// clamped to 0 at midnight — and it scales the base's first two channels by `f * 0.94 + 0.06`
/// and the last by `f * 0.91 + 0.09`.
///
/// The source takes this cosine from `MathHelper.cos`, whose 65536-entry table
/// (`MathHelper.java:30-41`) quantises the angle: the lookup truncates to a table step, so the
/// factor can move by at most about `2e-4` (one step, doubled by the `* 2`) — well under one
/// byte of colour, and exactly nothing at the noon and midnight angles the tests pin, where the
/// clamped factor is 1 and 0 either way.
fn surface_fog(celestial_angle: f32) -> [f32; 3] {
    let factor = ((celestial_angle * PI * 2.0).cos() * 2.0 + 0.5).clamp(0.0, 1.0);
    [
        SURFACE_FOG[0] * (factor * 0.94 + 0.06),
        SURFACE_FOG[1] * (factor * 0.94 + 0.06),
        SURFACE_FOG[2] * (factor * 0.91 + 0.09),
    ]
}

/// The angle of the sun and moon in the sky for a world time in ticks
/// (`WorldProvider.calculateCelestialAngle`, `WorldProvider.java:115-133`), with zero partial
/// ticks and the source's own float order.
///
/// The closing `f = f + (f - f) / 3.0F` of the source's formula (`:131`) adds zero in every
/// case and is not written. The cosine is the plain one, as the source's own `Math.cos` call
/// is; the table that `MathHelper.cos` routes through belongs to the fog colour's factor
/// (`WorldProvider.java:178`).
fn celestial_angle(time_of_day: f32) -> f32 {
    let mut f = (time_of_day % 24000.0) / 24000.0 - 0.25;
    if f < 0.0 {
        f += 1.0;
    }
    if f > 1.0 {
        f -= 1.0;
    }
    1.0 - ((f * PI).cos() + 1.0) / 2.0
}

#[cfg(test)]
mod tests {
    use super::{
        FogMode, FogParams, celestial_angle, fog_colour, light_factor, linear_params,
        sky_mix_strength, surface_fog, water_fog_for_eye,
    };

    /// The sky the chain's tests mix towards: the same flat triple every time, so a failure
    /// points at one step rather than at the sky's own ramp.
    const SKY: [f32; 3] = [0.4, 0.6, 0.8];

    #[test]
    fn the_celestial_angle_wraps_over_a_day() {
        assert_eq!(celestial_angle(6000.0), 0.0, "noon");
        assert_eq!(celestial_angle(18000.0), 0.5, "midnight");
        assert_eq!(celestial_angle(0.0), 0.8535534, "sunrise");
        assert_eq!(celestial_angle(12000.0), 0.14644659, "sunset");
        assert_eq!(celestial_angle(24000.0), celestial_angle(0.0), "a full day");
        assert_eq!(celestial_angle(30000.0), celestial_angle(6000.0));
    }

    #[test]
    fn the_base_is_the_providers_value_darkened_by_the_day_factor() {
        // Noon's factor is one, so the base passes through; midnight's clamps to zero, leaving
        // the `(0.06, 0.06, 0.09)` terms.
        assert_eq!(surface_fog(0.0), [0.7529412, 0.84705883, 1.0]);
        assert_eq!(surface_fog(0.5), [0.04517647, 0.05082353, 0.09]);
    }

    #[test]
    fn the_sky_mix_strength_is_the_sources_fourth_root() {
        // `1 - (0.25 + 0.75 * chunks / 32)^(1/4)` (`EntityRenderer.java:1767-1768`): the
        // eight-chunk distance the acceptance captures with, the four-chunk setting, and the
        // thirty-two-chunk maximum, where the sum is one and nothing mixes.
        let eight = sky_mix_strength(8);
        assert!(
            (eight - 0.18671173).abs() < 1e-6,
            "the eight-chunk strength is 0.186711...; got {eight}"
        );
        let four = sky_mix_strength(4);
        assert!(
            (four - 0.23429644).abs() < 1e-6,
            "the four-chunk strength is 0.234296...; got {four}"
        );
        assert_eq!(sky_mix_strength(32), 0.0);
    }

    #[test]
    fn the_light_factor_reads_the_brightness_table_and_the_distance() {
        // Level 15 is the table's one, so the factor is one at every distance; level 8 is
        // 0.2222..., which the eight-chunk distance scales to `0.75 * 0.2222... + 0.25`; level
        // 0 leaves the `chunks / 32` floor (`EntityRenderer.java:363-364`).
        assert_eq!(light_factor(15, 8), 1.0);
        assert_eq!(light_factor(8, 8), 0.416_666_7);
        assert_eq!(light_factor(0, 8), 0.25);
        assert_eq!(light_factor(0, 0), 0.0);
    }

    #[test]
    fn the_linear_range_reaches_the_far_plane() {
        assert_eq!(linear_params(128.0), (96.0, 128.0));
        let params = FogParams {
            colour: fog_colour(0, 6000.0, 64.0, 0.03125, SKY, 8, 15),
            start: linear_params(128.0).0,
            end: linear_params(128.0).1,
            far_plane: 128.0,
            mode: FogMode::Linear,
            density: 0.0,
        };
        assert_eq!(params.end, params.far_plane);
    }

    /// The submerged eye's branch (`EntityRenderer.java:1985-1995` for the mode
    /// and density, `:1845-1847` for the colour): EXP at density 0.1 over
    /// (0.02, 0.02, 0.2) with no respiration and no potion; respiration
    /// thins the density `0.03` a level and lifts every colour channel `0.2`
    /// a level, while water breathing pins the density at `0.01` and folds
    /// the lift to `0.3x + 0.6`. Dry eyes take no branch.
    #[test]
    fn the_submerged_eye_selects_the_exp_water_branch() {
        let water =
            water_fog_for_eye(true, 0, false).expect("a submerged eye takes the water branch");
        assert_eq!(water.mode, FogMode::Exp);
        assert!(
            (water.density - 0.1).abs() < 1e-6,
            "the still-water density is 0.1; got {}",
            water.density
        );
        for (channel, expected) in water.colour.iter().zip([0.02, 0.02, 0.2]) {
            assert!(
                (channel - expected).abs() < 1e-6,
                "the still-water colour is (0.02, 0.02, 0.2); got {:?}",
                water.colour
            );
        }
        // Respiration III, no potion: `0.1 - 3 * 0.03` over the lifted triple.
        let skilled = water_fog_for_eye(true, 3, false).expect("respiration keeps the branch");
        assert!(
            (skilled.density - 0.01).abs() < 1e-6,
            "respiration III thins the density to 0.01; got {}",
            skilled.density
        );
        for (channel, expected) in skilled.colour.iter().zip([0.62, 0.62, 0.8]) {
            assert!(
                (channel - expected).abs() < 1e-6,
                "respiration III lifts the colour to (0.62, 0.62, 0.8); got {:?}",
                skilled.colour
            );
        }
        // Water breathing, no respiration: the density pins at `0.01` and the
        // lift folds to `0.6`.
        let potion = water_fog_for_eye(true, 0, true).expect("the potion keeps the branch");
        assert!(
            (potion.density - 0.01).abs() < 1e-6,
            "water breathing pins the density at 0.01; got {}",
            potion.density
        );
        for (channel, expected) in potion.colour.iter().zip([0.62, 0.62, 0.8]) {
            assert!(
                (channel - expected).abs() < 1e-6,
                "water breathing lifts the colour to (0.62, 0.62, 0.8); got {:?}",
                potion.colour
            );
        }
        assert_eq!(
            water_fog_for_eye(false, 0, false),
            None,
            "a dry eye takes no branch"
        );
        assert_eq!(
            water_fog_for_eye(false, 3, true),
            None,
            "respiration and potion are dry-eyed no-ops"
        );
    }
}
