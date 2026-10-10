//! The world clock's maths: the celestial angle, the sun, the stars, the sky's colour and
//! the clouds' tint, pinned against the source's own numbers.
//!
//! Every literal was derived from the MCP-919 clone before the code existed: the angle and
//! the brightnesses from `WorldProvider.calculateCelestialAngle` (`WorldProvider.java:115-133`),
//! `World.getSunBrightness` (`World.java:1418-1427`) and `World.getStarBrightness`
//! (`:1596-1603`); the colours from `World.getSkyColor` (`:1432-1448`),
//! `BiomeGenBase.getSkyColorByTemp` (`BiomeGenBase.java:328-333`), `MathHelper.hsvToRGB`
//! (`MathHelper.java:485-543`) and `World.getCloudColour` (`:1520-1541`). The figures the
//! float arithmetic decides were taken from the JVM harness `refs/m2-task-12/sky_literals.java`,
//! which copies those expressions out of the source and runs them under the local JVM.

use oxide_proto_v47::column::{ColumnData, SectionData};
use oxide_world::sky::{
    celestial_angle, cloud_colour, moon_phase, sky_colour, sky_colour_at_temperature,
    star_brightness, sun_brightness,
};
use oxide_world::world::World;

/// The angle at the source's key times: 0 sunrise, 6000 noon, 12000 sunset and 18000
/// midnight, and the day wrapping at 24000 (`WorldProvider.java:115-133`).
///
/// The port takes the cosine in `f64` and narrows the result, exactly as
/// `calculateCelestialAngle` widens its `f32` day fraction for `Math.cos`; the four clean
/// points therefore land on the harness's own figures bit for bit, and the fractional cases
/// are pinned within a tolerance that covers the JVM's transcendental rounding.
#[test]
fn the_celestial_angle_wraps_over_a_day() {
    assert_eq!(celestial_angle(6000, 0.0), 0.0, "noon");
    assert_eq!(celestial_angle(18000, 0.0), 0.5, "midnight");
    assert_eq!(celestial_angle(0, 0.0), 0.8535534, "sunrise");
    assert_eq!(celestial_angle(12000, 0.0), 0.14644659, "sunset");
    assert_eq!(
        celestial_angle(24000, 0.0),
        celestial_angle(0, 0.0),
        "a day"
    );
    assert_eq!(celestial_angle(30000, 0.0), celestial_angle(6000, 0.0));
    assert_eq!(celestial_angle(48000, 0.5), celestial_angle(0, 0.5));

    // The partial tick lands inside the tick's own fraction of the day
    // (`:118`, `((float)i + partialTicks) / 24000.0F`).
    let nine = celestial_angle(9000, 0.5);
    assert!(
        (nine - 0.038072765).abs() < 1e-5,
        "9000.5 ticks: got {nine}"
    );
    let twenty_one = celestial_angle(21000, 0.25);
    assert!(
        (twenty_one - 0.6913569).abs() < 1e-5,
        "21000.25 ticks: got {twenty_one}"
    );
}

/// A negative time of day wraps into its day before the cosine reads it, so `-6000` is the
/// same half day as `18000`. A frozen clock's wire value is not what arrives here negative —
/// the receive rule negates it back (`WorldClient.java:468-481`) — but the smallest `i64`,
/// which negation leaves in place, still needs the wrap.
#[test]
fn a_negative_time_of_day_wraps_into_its_day() {
    assert_eq!(celestial_angle(-6000, 0.0), 0.5, "a negative half day");
    assert_eq!(celestial_angle(-6000, 0.0), celestial_angle(18000, 0.0));
    assert_eq!(celestial_angle(-12000, 0.0), celestial_angle(12000, 0.0));
}

/// The sun's brightness at noon and midnight (`World.java:1418-1427`): `1 - (cos * 2 +
/// 0.2)` clamps to zero at noon and to one at midnight, so the value is `1 * 0.8 + 0.2`
/// and `0 * 0.8 + 0.2`.
#[test]
fn the_sun_brightness_reaches_its_noon_and_midnight_values() {
    assert_eq!(sun_brightness(6000, 0.0, 0.0), 1.0, "noon");
    assert_eq!(sun_brightness(18000, 0.0, 0.0), 0.2, "midnight");
    assert_eq!(sun_brightness(0, 0.0, 0.0), 1.0, "sunrise is still full");
}

/// The stars' brightness: `clamp(1 - (cos * 2 + 0.25))`, squared and halved
/// (`World.java:1596-1603`), times the rain factor `1 - rain` `RenderGlobal` multiplies in
/// (`RenderGlobal.java:1325`) — zero through the day, half at midnight, and dimmed by rain.
#[test]
fn the_star_brightness_is_zero_by_day_and_half_at_midnight() {
    assert_eq!(star_brightness(6000, 0.0, 0.0), 0.0, "noon");
    assert_eq!(star_brightness(18000, 0.0, 0.0), 0.5, "midnight");
    assert_eq!(star_brightness(18000, 0.0, 1.0), 0.0, "rained out");
    assert_eq!(star_brightness(18000, 0.0, 0.5), 0.25, "half the rain");
}

/// The biome temperature ramp's two ends and the middle, at the noon angle: the byte
/// triples `BiomeGenBase.getSkyColorByTemp` answers for a cold (0.0), an ocean (0.5), a
/// plains (0.8) and a desert (2.0) temperature, over 255, scaled by the day's factor of
/// one (`World.java:1442-1449`, `BiomeGenBase.java:328-333`).
#[test]
fn the_temperature_ramp_spans_its_two_ends() {
    let at_noon = |temperature: f32| sky_colour_at_temperature(temperature, 0.0);
    assert_eq!(
        at_noon(0.0),
        [127.0 / 255.0, 161.0 / 255.0, 1.0],
        "the cold end: 0x7FA1FF"
    );
    assert_eq!(
        at_noon(0.5),
        [123.0 / 255.0, 164.0 / 255.0, 1.0],
        "the ocean's: 0x7BA4FF"
    );
    assert_eq!(
        at_noon(0.8),
        [120.0 / 255.0, 167.0 / 255.0, 1.0],
        "the plains': 0x78A7FF"
    );
    assert_eq!(
        at_noon(2.0),
        [110.0 / 255.0, 177.0 / 255.0, 1.0],
        "the hot end: 0x6EB1FF"
    );
    // The ramp clamps beyond its ends: four times the plains temperature is still the
    // clamped hot end, and a negative one the cold end.
    assert_eq!(sky_colour_at_temperature(8.0, 0.0), at_noon(3.0));
    assert_eq!(sky_colour_at_temperature(-4.0, 0.0), at_noon(-3.0));
}

/// The sky at the day's key angles for the plains temperature: full at noon, dark at
/// midnight and the dusk literal in between, each the ramp's colour scaled by
/// `clamp(cos(angle * 2pi) * 2 + 0.5)` (`World.java:1434-1449`).
#[test]
fn the_sky_colour_darkens_with_the_angle() {
    assert_eq!(
        sky_colour_at_temperature(0.8, 0.0),
        [120.0 / 255.0, 167.0 / 255.0, 1.0],
        "noon"
    );
    assert_eq!(
        sky_colour_at_temperature(0.8, 0.5),
        [0.0, 0.0, 0.0],
        "midnight: the factor clamps to zero"
    );
    let dusk = sky_colour_at_temperature(0.8, 0.22221488);
    let want = [0.39877048, 0.5549556, 0.84738725];
    for axis in 0..3 {
        assert!(
            (dusk[axis] - want[axis]).abs() < 1e-5,
            "the dusk literal, axis {axis}: got {}, want {}",
            dusk[axis],
            want[axis]
        );
    }
}

/// The clouds' tint over the world's `0xFFFFFF` base (`World.java:1520-1541`, `:73`):
/// white at noon, the clamped night triple at midnight, the dusk literal between them.
#[test]
fn the_cloud_colour_follows_the_same_factor() {
    assert_eq!(cloud_colour(6000, 0.0, 0.0), [1.0, 1.0, 1.0], "noon");
    assert_eq!(
        cloud_colour(18000, 0.0, 0.0),
        [0.1, 0.1, 0.15],
        "midnight: each channel at its own floor"
    );
    let dusk = cloud_colour(13500, 0.0, 0.0);
    let want = [0.86264855, 0.86264855, 0.8702792];
    for axis in 0..3 {
        assert!(
            (dusk[axis] - want[axis]).abs() < 1e-5,
            "the dusk literal, axis {axis}: got {}, want {}",
            dusk[axis],
            want[axis]
        );
    }
}

/// The moon's phase: `worldTime / 24000 % 8`, wrapped positive
/// (`WorldProvider.java:135-138`), so it steps once a day, wraps at eight, and the wrap
/// answers a phase index for a negative time of day as well — the receive rule
/// (`WorldClient.java:468-481`) keeps a frozen clock positive before it is read.
#[test]
fn the_moon_phase_steps_once_a_day() {
    assert_eq!(moon_phase(0), 0, "the first day");
    assert_eq!(moon_phase(23999), 0, "the day is not over yet");
    assert_eq!(moon_phase(24000), 1, "the next day");
    assert_eq!(moon_phase(168000), 7, "the eighth day");
    assert_eq!(moon_phase(192000), 0, "the ninth wraps to the first");
    assert_eq!(moon_phase(-24000), 7, "a negative day wraps positive");
    assert_eq!(moon_phase(-192000), 0);
}

/// The single sample the sky's colour is taken at: the view entity's own block, through
/// the world's biome at that column (`World.java:1437-1443`) — and the fallback an
/// unloaded column answers, which is ocean (temperature 0.5).
#[test]
fn the_sky_colour_samples_the_view_blocks_biome() {
    let empty = World::new(true);
    assert_eq!(
        sky_colour(&empty, 12, 70, -3, 0.0),
        [123.0 / 255.0, 164.0 / 255.0, 1.0],
        "with no column loaded the fallback biome answers"
    );

    // A loaded plains column at sea level is the plains temperature, so the sample is the
    // plains colour — the same answer at any block of the column, and the height
    // adjustment only bites above y = 64.
    let plains = world_of(1);
    assert_eq!(
        sky_colour(&plains, 12, 64, -3, 0.0),
        [120.0 / 255.0, 167.0 / 255.0, 1.0],
        "plains at sea level"
    );
    assert_eq!(
        sky_colour(&plains, -20, 3, 30, 0.0),
        [120.0 / 255.0, 167.0 / 255.0, 1.0],
        "the same column, another block"
    );

    // A desert column answers the hot end.
    let desert = world_of(2);
    assert_eq!(
        sky_colour(&desert, 12, 64, -3, 0.0),
        [110.0 / 255.0, 177.0 / 255.0, 1.0],
        "desert at sea level"
    );
}

/// One air section, so a column the tests build is applied rather than taken for an unload
/// (`World::apply_column`).
fn air_section() -> SectionData {
    SectionData {
        blocks: Box::new([0u16; 4096]),
        block_light: Box::new([0u8; 2048]),
        sky_light: Some(Box::new([0xFFu8; 2048])),
    }
}

/// A column whose biome array is one id at every position.
fn column(id: u8) -> ColumnData {
    let mut data = ColumnData::empty();
    data.mask = 1;
    data.sections[0] = Some(air_section());
    data.biomes = Some([id; 256]);
    data
}

/// A world with `id` at every position, covering chunk coordinates -4..=1 in both axes.
fn world_of(id: u8) -> World {
    let mut world = World::new(true);
    for cx in -4i32..=1 {
        for cz in -4i32..=1 {
            world.apply_column(cx, cz, &column(id), true);
        }
    }
    world
}
