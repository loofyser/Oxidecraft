//! The world clock's sky: the celestial angle, the sun and the stars, the sky's colour and the
//! clouds' tint, from the decompiled 1.8.9 client's own arithmetic.
//!
//! # The clock
//!
//! `S03PacketTimeUpdate` carries two big-endian `i64`s: the world's age and its time of day
//! (`S03PacketTimeUpdate.java:36-49`). The client stores both (`NetHandlerPlayClient.java:952-957`).
//! A server that has stopped the day-night cycle negates the time before sending it (`:17-31`),
//! and the receiving client negates it back before the clock stores it: that is
//! `WorldClient.setWorldTime`'s receive rule (`WorldClient.java:468-483`), so the value every
//! function here reads — the client's `worldTime` — is the positive one the source renders from
//! (the wire's `-6000` is the clock's `6000`, noon). `World.getCelestialAngle` and
//! `World.getMoonPhase` both read `worldTime` (`World.java:1493-1501`), so `time_of_day` is the
//! field that drives the sky; the age is carried for later work and takes no part in anything in
//! this module.
//!
//! M2 has no client tick loop (M3's), so the sky renders from the last received update with
//! `partial_ticks = 0`; every function below takes the partial tick anyway, because that is what
//! the source interpolates with, and a caller that has a tick loop passes its own value.
//!
//! # What is ported
//!
//! * [`celestial_angle`] — `WorldProvider.calculateCelestialAngle` (`WorldProvider.java:115-133`).
//! * [`moon_phase`] — `WorldProvider.getMoonPhase` (`:135-138`).
//! * [`sun_brightness`] — `World.getSunBrightness` (`World.java:1418-1427`).
//! * [`star_brightness`] — `World.getStarBrightness` (`:1596-1603`) times the rain factor
//!   `RenderGlobal` multiplies in at the star draw (`RenderGlobal.java:1325`).
//! * [`sky_colour`] and [`sky_colour_at_temperature`] — `World.getSkyColor` (`:1432-1488`) over
//!   `BiomeGenBase.getSkyColorByTemp` (`BiomeGenBase.java:328-333`) and `MathHelper.hsvToRGB`
//!   (`MathHelper.java:485-543`).
//! * [`cloud_colour`] — `World.getCloudColour` (`:1520-1553`) over the world's white
//!   `cloudColour` base (`:73`).
//!
//! The rain blends are ported where the interface carries a rain strength; the thunder and
//! lightning blends (`World.java:1459-1485`, `:1542-1551`) have no parameter to reach them and are
//! deferred with the weather. M2 sends `rain_strength = 0.0` throughout.
//!
//! The source feeds the brightness and colour factors through `MathHelper.cos`, whose 65536-entry
//! table (`MathHelper.java:30-41`) quantises the angle: the lookup truncates to a table step, so a
//! factor can move by at most about `2e-4`, under one byte of colour. This module uses the plain
//! cosine instead, the same declared divergence Task 11's fog records (`crate::fog`), so the
//! pinned literals and the rendered picture agree with the harness the reference was taken with.
//!
//! # Where the sky is sampled
//!
//! `World.getSkyColor` reads exactly one position: the render-view entity's own block,
//! `floor(posX)`/`floor(posY)`/`floor(posZ)` (`World.java:1437-1443`). There is no view-position
//! grid and no averaging; the biome's height-adjusted temperature at that one block decides the
//! whole sky's colour.

use crate::biome::{biome, biome_id_at, height_adjusted_temperature};
use crate::world::World;

/// The ticks in one day, which is also one moon phase.
const DAY_TICKS: i64 = 24_000;

/// `MathHelper.clamp_float(value, 0.0F, 1.0F)`, the clamp the source applies to its factors.
///
/// Its bounds are constants, so the panic `clamp` documents cannot fire; a NaN passes through as
/// the source's comparison chain passes it.
fn clamp_01(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

/// The plain cosine the harness and the ports use, in the source's own order: the angle widens to
/// `f64`, the cosine runs there and the result narrows to `f32`.
///
/// `World.getSunBrightness` and the colour functions call `MathHelper.cos`, whose table this
/// module does not reproduce; see the module comment for that declared divergence.
fn plain_cos(angle: f32) -> f32 {
    (f64::from(angle) * std::f64::consts::PI * 2.0).cos() as f32
}

/// The day-night factor `cos(angle * 2pi) * 2 + 0.5` clamped to `0..=1`: one at noon, zero at
/// midnight, the factor the sun and star brightnesses and both colours start from
/// (`World.java:1434`, `:1598`, `:1522`).
fn day_factor(celestial_angle: f32) -> f32 {
    clamp_01(plain_cos(celestial_angle) * 2.0 + 0.5)
}

/// The angle of the sun and moon in the sky for a world time in ticks and a partial tick:
/// `WorldProvider.calculateCelestialAngle` (`WorldProvider.java:115-133`).
///
/// The time wraps into its day first (`worldTime % 24000`), the wrapped value becomes the day's
/// fraction, and the cosine turns that into the angle: zero at noon, `0.5` at midnight, the
/// sunrise and sunset values in between. A frozen clock's wire value arrives here already
/// negated by the receive rule (the module comment) — the wire's `-6000` is the `+6000` of noon
/// — and the wrap is what keeps the arithmetic defined for values the negation leaves in place,
/// the smallest `i64` included.
///
/// The source's closing `f = f + (f - f) / 3.0F` adds zero for every input and is not written.
pub fn celestial_angle(time_of_day: i64, partial_ticks: f32) -> f32 {
    let mut f = (time_of_day % DAY_TICKS) as f32 + partial_ticks;
    f = f / 24_000.0 - 0.25;
    if f < 0.0 {
        f += 1.0;
    }
    if f > 1.0 {
        f -= 1.0;
    }
    let cosine = (f64::from(f) * std::f64::consts::PI).cos();
    1.0 - ((cosine + 1.0) / 2.0) as f32
}

/// The moon's phase for a world time in ticks: `WorldProvider.getMoonPhase`
/// (`WorldProvider.java:135-138`).
///
/// One phase per day, eight phases in a cycle, wrapped positive so the answer is a phase index
/// whatever `i64` the arithmetic meets: `-24000` is phase 7. A frozen clock's wire value arrives
/// here already negated by the receive rule (the module comment), so it is read as its positive day.
pub fn moon_phase(time_of_day: i64) -> i32 {
    ((time_of_day / DAY_TICKS) % 8 + 8) as i32 % 8
}

/// The sun's brightness for a world time, a partial tick and a rain strength:
/// `World.getSunBrightness` (`World.java:1418-1427`).
///
/// The factor is `1 - (cos(angle * 2pi) * 2 + 0.2)` clamped and flipped, so it is one through the
/// day and zero at midnight, then the rain term `1 - rain * 5 / 16` scales it and the result is
/// `f * 0.8 + 0.2`. The source's thunder term (`:1425`) has no parameter here and is deferred with
/// the weather.
pub fn sun_brightness(time_of_day: i64, partial_ticks: f32, rain_strength: f32) -> f32 {
    let angle = celestial_angle(time_of_day, partial_ticks);
    let mut f = 1.0 - (plain_cos(angle) * 2.0 + 0.2);
    f = clamp_01(f);
    f = 1.0 - f;
    f = (f64::from(f) * (1.0 - f64::from(rain_strength * 5.0) / 16.0)) as f32;
    f * 0.8 + 0.2
}

/// The stars' brightness for a world time, a partial tick and a rain strength:
/// `World.getStarBrightness` (`World.java:1596-1603`) times the rain factor `RenderGlobal`
/// multiplies in at the star draw (`RenderGlobal.java:1325`, `f15 = getStarBrightness * (1 - rain)`).
///
/// The factor is `1 - (cos(angle * 2pi) * 2 + 0.25)` clamped, squared and halved: zero through the
/// day, one half at midnight, then dimmed by rain. The renderer skips the stars when this is not
/// above zero.
pub fn star_brightness(time_of_day: i64, partial_ticks: f32, rain_strength: f32) -> f32 {
    let angle = celestial_angle(time_of_day, partial_ticks);
    let mut f = 1.0 - (plain_cos(angle) * 2.0 + 0.25);
    f = clamp_01(f);
    f * f * 0.5 * (1.0 - rain_strength)
}

/// One channel of `MathHelper.hsvToRGB`'s conversion: the value times 255, truncated towards
/// zero, then clamped to a byte (`MathHelper.java:536-538`).
fn hsv_byte(value: f32) -> u8 {
    ((value * 255.0) as i32).clamp(0, 255) as u8
}

/// `MathHelper.hsvToRGB` (`MathHelper.java:485-543`): hue in `0..1`, saturation and value in
/// `0..1`, an `0xRRGGBB` triple back.
fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> [u8; 3] {
    let scaled = hue * 6.0;
    let sector = (scaled as i32) % 6;
    let fraction = scaled - sector as f32;
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - fraction * saturation);
    let t = value * (1.0 - (1.0 - fraction) * saturation);
    let (r, g, b) = match sector {
        0 => (value, t, p),
        1 => (q, value, p),
        2 => (p, value, t),
        3 => (p, q, value),
        4 => (t, p, value),
        _ => (value, p, q),
    };
    [hsv_byte(r), hsv_byte(g), hsv_byte(b)]
}

/// `BiomeGenBase.getSkyColorByTemp` (`BiomeGenBase.java:328-333`): the temperature over three,
/// clamped to `-1..=1`, feeds the hue and saturation of the sky's base colour as three bytes.
pub fn sky_colour_bytes(temperature: f32) -> [u8; 3] {
    let t = (temperature / 3.0).clamp(-1.0, 1.0);
    hsv_to_rgb(0.62222224 - t * 0.05, 0.5 + t * 0.1, 1.0)
}

/// The sky's colour for a biome temperature and a celestial angle, the temperature ramp scaled by
/// the day's factor: `World.getSkyColor`'s middle stretch (`World.java:1434-1449`).
///
/// The ramp's bytes are divided by 255 and multiplied by the factor, each channel on its own, in
/// the source's order. A caller that has sampled a world's biome uses [`sky_colour`] instead.
pub fn sky_colour_at_temperature(temperature: f32, celestial_angle: f32) -> [f32; 3] {
    let factor = day_factor(celestial_angle);
    let bytes = sky_colour_bytes(temperature);
    std::array::from_fn(|channel| f32::from(bytes[channel]) / 255.0 * factor)
}

/// The sky's colour at the view block (`World.getSkyColor`, `World.java:1432-1488`).
///
/// One position is sampled: the given block, whose column's biome supplies the height-adjusted
/// temperature for [`sky_colour_at_temperature`]. A block with no column loaded answers the
/// fallback biome, ocean (`BiomeGenBase.getBiome`, `BiomeGenBase.java:584-600`). The source's
/// rain, thunder and lightning blends (`:1450-1485`) are the weather's: M2 has none and the
/// function stops before them.
pub fn sky_colour(world: &World, x: i32, y: i32, z: i32, celestial_angle: f32) -> [f32; 3] {
    let data = biome(biome_id_at(world, x, z));
    let temperature = height_adjusted_temperature(data.temperature, x, y, z);
    sky_colour_at_temperature(temperature, celestial_angle)
}

/// The clouds' tint for a world time, a partial tick and a rain strength: `World.getCloudColour`
/// (`World.java:1520-1553`) over the world's white `cloudColour` base (`:73`).
///
/// Each channel starts at one, the rain blend mixes in the grey `0.6` of the colour's own
/// luminance, the day factor scales the first two channels by `f * 0.9 + 0.1` and the last by
/// `f * 0.85 + 0.15`. The source's thunder blend (`:1542-1551`) has no parameter here and is
/// deferred with the weather, as `sky_colour`'s heavier blends are.
pub fn cloud_colour(time_of_day: i64, partial_ticks: f32, rain_strength: f32) -> [f32; 3] {
    let angle = celestial_angle(time_of_day, partial_ticks);
    let factor = day_factor(angle);
    let mut channels = [1.0f32, 1.0, 1.0];
    if rain_strength > 0.0 {
        let grey = (channels[0] * 0.3 + channels[1] * 0.59 + channels[2] * 0.11) * 0.6;
        let keep = 1.0 - rain_strength * 0.95;
        for channel in &mut channels {
            *channel = *channel * keep + grey * (1.0 - keep);
        }
    }
    [
        channels[0] * (factor * 0.9 + 0.1),
        channels[1] * (factor * 0.9 + 0.1),
        channels[2] * (factor * 0.85 + 0.15),
    ]
}
