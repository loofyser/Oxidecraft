//! The frame's fog: the colour the world fades to and the linear range the terrain fades over.
//!
//! The colour chain is `EntityRenderer.updateFogColor` (`EntityRenderer.java:1763-1928`), of
//! which M2 lands two steps — the per-dimension base with the day-night factor
//! (`WorldProvider.getFogColor`, `WorldProvider.java:177-188`) and the void-fog altitude factor
//! (`EntityRenderer.java:1860-1887`) — plus the default linear range `setupFog` installs for
//! terrain (`:2002-2016`). The rain blend, the sunset band, the sky-colour mix whose strength
//! comes from the render distance (`:1767-1768`, `:1803-1805`), the boss tint, the night-vision
//! term and the per-block overrides are the later scene work's.
//!
//! The colour is also the window's clear colour: the source ends `updateFogColor` by handing it
//! to `glClearColor` with an alpha of zero (`:1927`), which is how the horizon and the fog meet
//! without a seam.

use std::f32::consts::PI;

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

/// The frame's fog colour for a dimension, a world time and an eye height.
///
/// `time_of_day` is the world's own time **in ticks** — the value `WorldInfo.getWorldTime`
/// hands `calculateCelestialAngle` (`World.getFogColor`, `World.java:1559-1563`), where 0 is
/// sunrise, 6000 noon, 12000 sunset and 18000 midnight. The celestial angle is derived here
/// with `WorldProvider.calculateCelestialAngle`'s formula (`WorldProvider.java:115-133`) at
/// zero partial ticks.
///
/// `void_y_factor` is the dimension's own `getVoidFogYFactor` (`WorldProvider.java:231-234`:
/// `0.03125` for every world type but Flat, which answers `1.0`). `eye_y` is the value the
/// source multiplies: the render-view entity's interpolated `posY` (`EntityRenderer.java:1860`),
/// the feet rather than the eye's height. The factor darkens the colour by
/// `(eye_y * void_y_factor)^2` while that product is below one, clamped above zero, and leaves
/// it alone from one upwards (`EntityRenderer.java:1876-1887`).
///
/// Dimension 0 is the Overworld and is the only one the M2 acceptance compares; -1 is the
/// Nether and 1 the End, each its provider's fixed base (the day-night factor is the
/// Overworld's own — neither of the others varies with the time). Any other id, which vanilla's
/// `getProviderForDimension` answers with no provider at all, takes the Overworld's chain.
pub fn fog_colour(dimension: i8, time_of_day: f32, eye_y: f64, void_y_factor: f32) -> [f32; 3] {
    let base = match dimension {
        -1 => NETHER_FOG,
        1 => END_FOG,
        _ => surface_fog(celestial_angle(time_of_day)),
    };
    let mut factor = eye_y * f64::from(void_y_factor);
    if factor < 1.0 {
        if factor < 0.0 {
            factor = 0.0;
        }
        factor *= factor;
        return [
            (f64::from(base[0]) * factor) as f32,
            (f64::from(base[1]) * factor) as f32,
            (f64::from(base[2]) * factor) as f32,
        ];
    }
    base
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
    use super::{FogParams, celestial_angle, fog_colour, linear_params};

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
    fn the_linear_range_reaches_the_far_plane() {
        assert_eq!(linear_params(128.0), (96.0, 128.0));
        let params = FogParams {
            colour: fog_colour(0, 6000.0, 64.0, 0.03125),
            start: linear_params(128.0).0,
            end: linear_params(128.0).1,
            far_plane: 128.0,
        };
        assert_eq!(params.end, params.far_plane);
    }
}
