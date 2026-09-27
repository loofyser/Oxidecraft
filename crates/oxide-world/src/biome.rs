//! The biome table and the tint path: what colour grass, foliage and water take
//! at a position, from vanilla's own rules.
//!
//! # What the values are taken from
//!
//! Every constant traces to the decompiled 1.8.9 client under
//! `refs/_src/MCP-919/src/minecraft/net/minecraft/`:
//!
//! * `world/biome/BiomeGenBase.java` — the biome table's static fields (72-125),
//!   the constructor defaults (183-199: temperature 0.5, rainfall 0.5, water
//!   multiplier 16777215), the mutation block (603-652), `getFloatRainfall`
//!   (399-401, not height-adjusted), `getFloatTemperature` (407-418, the height
//!   adjustment), `getGrassColorAtPos`/`getFoliageColorAtPos` (425-437, the
//!   clamp and the colour-map lookup), `getBiome` (584-600, the ocean fallback)
//!   and the two Perlin seeds (649-650).
//! * the biome classes' constructors and overrides — `BiomeGenPlains.java`
//!   (17, and the mutation at 100-108), `BiomeGenForest.java` (36-44, and the
//!   mutation and roofed-forest transform at 167-199), `BiomeGenSwamp.java`
//!   (28, 37-46), `BiomeGenMesa.java` (34, 56-64, 317-334),
//!   `BiomeGenSnow.java` (58-63), `BiomeGenSavanna.java` (32-39, 56-64),
//!   `BiomeGenTaiga.java` (110-113), `BiomeGenHills.java` (88-102),
//!   `BiomeGenMutated.java` (14-38) — every temperature, rainfall, name and
//!   colour override in the table.
//! * `world/ColorizerGrass.java` (16-23) and `world/ColorizerFoliage.java`
//!   (16-22) — the `(1 - temperature, 1 - rainfall * temperature)` index.
//! * `world/biome/BiomeColorHelper.java` (30-60) — the nine-sample average, the
//!   water-multiplier resolver and the channel masks.
//! * `client/renderer/BlockModelRenderer.java` (278-294, the non-smooth path's
//!   tint call and the channels-to-floats conversion) and
//!   `client/renderer/WorldRenderer.java` (368, the floats-to-bytes conversion
//!   the water scaling follows).
//!
//! Only names and values are carried over; no source text is reproduced.

use crate::behaviour::TintKind;
use crate::noise::perlin_sample;
use crate::world::World;

/// The white water multiplier the constructor installs, `16777215`
/// (`BiomeGenBase.java:189`).
const DEFAULT_WATER_COLOUR: [f32; 3] = [1.0, 1.0, 1.0];

/// Swamp's water multiplier, `14745518` (`BiomeGenSwamp.java:28`), whose
/// channels are `(224, 255, 174)`.
const SWAMP_WATER_COLOUR: [f32; 3] = [224.0 / 255.0, 1.0, 174.0 / 255.0];

/// The dark swamp grass colour, `5011004` (`BiomeGenSwamp.java:40`).
const SWAMP_GRASS_DARK: u32 = 5011004;

/// The light swamp grass colour, `6975545` (`BiomeGenSwamp.java:40`; swamp's
/// foliage colour too, `:45`).
const SWAMP_GRASS_LIGHT: u32 = 6975545;

/// Mesa's fixed grass colour, `9470285` (`BiomeGenMesa.java:63`).
const MESA_GRASS: u32 = 9470285;

/// Mesa's fixed foliage colour, `10387789` (`BiomeGenMesa.java:58`).
const MESA_FOLIAGE: u32 = 10387789;

/// The temperature noise's seed (`BiomeGenBase.java:649`, one octave).
const TEMPERATURE_SEED: i64 = 1234;

/// The swamp grass override's noise seed (`BiomeGenBase.java:650`, one octave).
const SWAMP_NOISE_SEED: i64 = 2345;

/// The height adjustment's noise scale: `x / 8.0` (`BiomeGenBase.java:411`).
const TEMPERATURE_NOISE_SCALE: f64 = 8.0;

/// The swamp override's noise scale: `x * 0.0225` (`BiomeGenSwamp.java:39`).
const SWAMP_NOISE_SCALE: f64 = 0.0225;

/// The swamp override's branch threshold (`BiomeGenSwamp.java:40`).
const SWAMP_NOISE_THRESHOLD: f64 = -0.1;

/// Bytes in one colour map: 256 x 256 pixels of RGBA.
const COLOR_MAP_BYTES: usize = 256 * 256 * 4;

/// One row of the biome table.
///
/// The names, temperatures, rainfalls and water multipliers are the values the
/// decompiled client's constructors and static block settle; see the module
/// comment for the files.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiomeData {
    /// The id the row answers for.
    pub id: u8,
    /// The biome's name, as `setBiomeName` sets it.
    pub name: &'static str,
    /// The temperature `getFloatTemperature` starts from.
    pub temperature: f32,
    /// The rainfall `getFloatRainfall` answers.
    pub rainfall: f32,
    /// The water multiplier as `0.0..=1.0` channels, the water colour before
    /// the client scales it back to bytes.
    pub water_colour: [f32; 3],
}

/// The shape of [`BIOME_TABLE`]: one row per id the client registers.
pub type BiomeTable = [BiomeData; 61];

/// The biome table, in ascending id order: ids 0-39 and the 21 mutations the
/// static block creates (`BiomeGenBase.java:603-625`), each with the numbers the
/// source gives it. A mutation copies its parent's temperature, rainfall and
/// water multiplier (`BiomeGenMutated.java:25-27`) except where its own class
/// sets them, and the mesa mutations are mesa rows.
pub static BIOME_TABLE: BiomeTable = [
    row(0, "Ocean", 0.5, 0.5),
    row(1, "Plains", 0.8, 0.4),
    row(2, "Desert", 2.0, 0.0),
    row(3, "Extreme Hills", 0.2, 0.3),
    row(4, "Forest", 0.7, 0.8),
    row(5, "Taiga", 0.25, 0.8),
    BiomeData {
        id: 6,
        name: "Swampland",
        temperature: 0.8,
        rainfall: 0.9,
        water_colour: SWAMP_WATER_COLOUR,
    },
    row(7, "River", 0.5, 0.5),
    row(8, "Hell", 2.0, 0.0),
    row(9, "The End", 0.5, 0.5),
    row(10, "FrozenOcean", 0.0, 0.5),
    row(11, "FrozenRiver", 0.0, 0.5),
    row(12, "Ice Plains", 0.0, 0.5),
    row(13, "Ice Mountains", 0.0, 0.5),
    row(14, "MushroomIsland", 0.9, 1.0),
    row(15, "MushroomIslandShore", 0.9, 1.0),
    row(16, "Beach", 0.8, 0.4),
    row(17, "DesertHills", 2.0, 0.0),
    row(18, "ForestHills", 0.7, 0.8),
    row(19, "TaigaHills", 0.25, 0.8),
    row(20, "Extreme Hills Edge", 0.2, 0.3),
    row(21, "Jungle", 0.95, 0.9),
    row(22, "JungleHills", 0.95, 0.9),
    row(23, "JungleEdge", 0.95, 0.8),
    row(24, "Deep Ocean", 0.5, 0.5),
    row(25, "Stone Beach", 0.2, 0.3),
    row(26, "Cold Beach", 0.05, 0.3),
    row(27, "Birch Forest", 0.6, 0.6),
    row(28, "Birch Forest Hills", 0.6, 0.6),
    row(29, "Roofed Forest", 0.7, 0.8),
    row(30, "Cold Taiga", -0.5, 0.4),
    row(31, "Cold Taiga Hills", -0.5, 0.4),
    row(32, "Mega Taiga", 0.3, 0.8),
    row(33, "Mega Taiga Hills", 0.3, 0.8),
    row(34, "Extreme Hills+", 0.2, 0.3),
    row(35, "Savanna", 1.2, 0.0),
    row(36, "Savanna Plateau", 1.0, 0.0),
    row(37, "Mesa", 2.0, 0.0),
    row(38, "Mesa Plateau F", 2.0, 0.0),
    row(39, "Mesa Plateau", 2.0, 0.0),
    row(129, "Sunflower Plains", 0.8, 0.4),
    row(130, "Desert M", 2.0, 0.0),
    row(131, "Extreme Hills M", 0.2, 0.3),
    row(132, "Flower Forest", 0.7, 0.8),
    row(133, "Taiga M", 0.25, 0.8),
    BiomeData {
        id: 134,
        name: "Swampland M",
        temperature: 0.8,
        rainfall: 0.9,
        water_colour: SWAMP_WATER_COLOUR,
    },
    row(140, "Ice Plains Spikes", 0.0, 0.5),
    row(149, "Jungle M", 0.95, 0.9),
    row(151, "JungleEdge M", 0.95, 0.8),
    row(155, "Birch Forest M", 0.6, 0.6),
    row(156, "Birch Forest Hills M", 0.6, 0.6),
    row(157, "Roofed Forest M", 0.7, 0.8),
    row(158, "Cold Taiga M", -0.5, 0.4),
    row(160, "Mega Spruce Taiga", 0.25, 0.8),
    row(161, "Redwood Taiga Hills M", 0.25, 0.8),
    row(162, "Extreme Hills+ M", 0.2, 0.3),
    row(163, "Savanna M", 1.1, 0.0),
    row(164, "Savanna Plateau M", 1.0, 0.0),
    row(165, "Mesa (Bryce)", 2.0, 0.0),
    row(166, "Mesa Plateau F M", 2.0, 0.0),
    row(167, "Mesa Plateau M", 2.0, 0.0),
];

/// A table row with the default white water multiplier.
///
/// Swamp's two ids are rewritten below with their own multiplier; every other
/// biome keeps the constructor's (`BiomeGenBase.java:189`).
const fn row(id: u8, name: &'static str, temperature: f32, rainfall: f32) -> BiomeData {
    BiomeData {
        id,
        name,
        temperature,
        rainfall,
        water_colour: DEFAULT_WATER_COLOUR,
    }
}

/// The row for a biome id: ocean when the id is not registered.
///
/// The table carries exactly the ids the 1.8 client registers — 0-39 and the
/// mutations 129-134, 140, 149, 151, 155-158 and 160-167. Every other id,
/// including the holes the source leaves inside 128-167, answers the ocean
/// entry: out of bounds that is `BiomeGenBase.getBiome`'s own answer
/// (`BiomeGenBase.java:598`), and the plan pins the same answer for a hole.
pub fn biome(id: u8) -> &'static BiomeData {
    match BIOME_TABLE.binary_search_by_key(&id, |row| row.id) {
        Ok(index) => &BIOME_TABLE[index],
        Err(_) => &BIOME_TABLE[0],
    }
}

/// A biome's temperature at a position: `BiomeGenBase.getFloatTemperature`.
///
/// At or below y = 64 it is the base value exactly; above it the height
/// adjustment subtracts `(noise * 4 + y - 64) * 0.05 / 30`, with `noise` the
/// one-octave generator seeded 1234 sampled at `x / 8, z / 8`
/// (`BiomeGenBase.java:407-418`, `:649`). The noise product is narrowed to
/// `f32` before the subtraction, as the source narrows it.
pub fn height_adjusted_temperature(base: f32, x: i32, y: i32, z: i32) -> f32 {
    if y > 64 {
        let noise = perlin_sample(
            TEMPERATURE_SEED,
            1,
            f64::from(x) / TEMPERATURE_NOISE_SCALE,
            f64::from(z) / TEMPERATURE_NOISE_SCALE,
        );
        let term = (noise * 4.0) as f32;
        base - (term + y as f32 - 64.0) * 0.05 / 30.0
    } else {
        base
    }
}

/// A biome's rainfall at a position: `BiomeGenBase.getFloatRainfall`.
///
/// 1.8.9 does not adjust rainfall by height: `getFloatRainfall` takes no
/// position and answers the biome's value (`BiomeGenBase.java:399-401`), so
/// this returns the base whatever the coordinates. The parameters are kept so
/// the call site reads like [`height_adjusted_temperature`]'s.
pub fn height_adjusted_rainfall(base: f32, _x: i32, _y: i32, _z: i32) -> f32 {
    base
}

/// Why a colour map could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ColorMapError {
    /// The hand-off carried a byte count that is not 256 x 256 RGBA.
    #[error("a colour map is 256 x 256 RGBA, {expected} bytes; got {got}")]
    WrongLength {
        /// The byte count a colour map needs.
        expected: usize,
        /// The byte count that arrived.
        got: usize,
    },
}

/// A 256 x 256 RGBA colour map, as decoding `textures/colormap/grass.png` or
/// `foliage.png` yields one (`GrassColorReloadListener.java:10-16` names the
/// grass map; the client reads both into flat int arrays).
///
/// The type wraps the raw bytes and does not load them: the store's PNG decode
/// belongs to a later task, and `oxide-world` has no asset dependency.
#[derive(Clone, PartialEq, Eq)]
pub struct ColorMap {
    /// Row-major RGBA bytes: the pixel at (x, y) starts at `(y * 256 + x) * 4`,
    /// the texture's top row first (`TextureUtil.readImageData` reads a
    /// `BufferedImage` with `getRGB(0, 0, width, height, ...)`).
    pixels: Box<[u8]>,
}

impl std::fmt::Debug for ColorMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ColorMap(256x256 RGBA)")
    }
}

impl ColorMap {
    /// A map from raw 256 x 256 RGBA bytes.
    ///
    /// The length must be exactly 256 x 256 x 4 = 262144; anything else is
    /// [`ColorMapError::WrongLength`], which names the count that arrived.
    pub fn from_rgba(bytes: &[u8]) -> Result<Self, ColorMapError> {
        if bytes.len() != COLOR_MAP_BYTES {
            return Err(ColorMapError::WrongLength {
                expected: COLOR_MAP_BYTES,
                got: bytes.len(),
            });
        }
        Ok(Self {
            pixels: bytes.to_vec().into_boxed_slice(),
        })
    }

    /// The RGBA pixel at a column and row; `None` outside 0..256.
    pub fn pixel(&self, x: usize, y: usize) -> Option<[u8; 4]> {
        if x >= 256 || y >= 256 {
            return None;
        }
        let offset = (y * 256 + x) * 4;
        let pixel = &self.pixels[offset..offset + 4];
        Some([pixel[0], pixel[1], pixel[2], pixel[3]])
    }
}

/// The colour maps the grass and foliage lookups read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TintMaps {
    /// The grass colour map (`ColorizerGrass`, `textures/colormap/grass.png`).
    pub grass: ColorMap,
    /// The foliage colour map (`ColorizerFoliage`, `textures/colormap/foliage.png`).
    pub foliage: ColorMap,
}

/// The colour a colour map holds for a temperature and rainfall:
/// `ColorizerGrass.getGrassColor` or `ColorizerFoliage.getFoliageColor`.
///
/// Both inputs are clamped to `0.0..=1.0` first, as the source's callers clamp
/// them (`BiomeGenBase.java:427-428`, `:434-435`), then the index is
/// `(255 * (1 - temperature), 255 * (1 - rainfall * temperature))`, truncating,
/// and the pixel there answers without its alpha. The humidity product is the
/// source's: both values widened to `f64` and multiplied there
/// (`ColorizerGrass.java:18-20`).
pub fn colormap_colour(map: &ColorMap, temperature: f32, rainfall: f32) -> [u8; 3] {
    let temperature = clamp_01(temperature);
    let rainfall = clamp_01(rainfall);
    let humidity = f64::from(rainfall) * f64::from(temperature);
    let column = ((1.0 - f64::from(temperature)) * 255.0) as usize;
    let row = ((1.0 - humidity) * 255.0) as usize;
    let offset = (row * 256 + column) * 4;
    let pixel = &map.pixels[offset..offset + 4];
    [pixel[0], pixel[1], pixel[2]]
}

/// `MathHelper.clamp_float(value, 0.0F, 1.0F)`, the clamp the source applies
/// before a colour-map lookup.
///
/// Its bounds are constants, so the panic `clamp` documents cannot fire; a NaN
/// passes through as the source's comparison chain passes it, and the cast to
/// an index then answers 0, the same answer the JVM's `(int)` gives.
fn clamp_01(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

/// The tint colour at a position: the single-sample lookup
/// [`tint_at_9`] averages.
///
/// * [`TintKind::None`] answers `[255, 255, 255]`, the neutral multiplier a
///   mesher's colour column multiplies by.
/// * [`TintKind::Grass`] and [`TintKind::GrassSideOverlay`] answer the grass
///   colour, and [`TintKind::Foliage`] the foliage colour: the colour map at
///   the height-adjusted temperature and the biome's rainfall, with the fixed
///   swamp and mesa colours and the roofed-forest transform below.
/// * [`TintKind::Water`] answers the biome's water multiplier scaled back to
///   bytes — each channel over 255 and truncated, the source's own conversion
///   (`WorldRenderer.java:368`, over the channels `BlockModelRenderer.java:287-289`
///   divides) — so ocean is white and swamp is `(224, 255, 174)`.
///
/// The biome is the one the column's biome array holds; where no column is
/// loaded the fallback biome (ocean) answers, so a missing column is not an
/// error on this path.
pub fn tint_at(world: &World, maps: &TintMaps, x: i32, y: i32, z: i32, kind: TintKind) -> [u8; 3] {
    let data = biome(biome_id_at(world, x, z));
    match kind {
        TintKind::None => [255, 255, 255],
        TintKind::Grass | TintKind::GrassSideOverlay => grass_colour(data, maps, x, y, z),
        TintKind::Foliage => foliage_colour(data, maps, x, y, z),
        TintKind::Water => scale_water(data.water_colour),
    }
}

/// The nine-sample neighbourhood average around a position: what the mesher
/// consumes in both graphics modes.
///
/// The samples are [`tint_at`] over `x - 1..=x + 1` and `z - 1..=z + 1` at the
/// same y, summed per channel and divided by nine with the division truncating.
/// That is `BiomeColorHelper.getColorAtPos`'s average
/// (`BiomeColorHelper.java:30-45`; its per-channel `& 255` cannot bite, since a
/// sum of nine channels over nine is at most 255), and the client's non-smooth
/// path reaches it through `BlockModelRenderer.java:280` ->
/// `BlockGrass.java:51-54` and `BlockLeaves.java:47-50`.
pub fn tint_at_9(
    world: &World,
    maps: &TintMaps,
    x: i32,
    y: i32,
    z: i32,
    kind: TintKind,
) -> [u8; 3] {
    let mut sums = [0u32; 3];
    for offset_z in -1..=1 {
        for offset_x in -1..=1 {
            let colour = tint_at(world, maps, x + offset_x, y, z + offset_z, kind);
            for (sum, channel) in sums.iter_mut().zip(colour) {
                *sum += u32::from(channel);
            }
        }
    }
    std::array::from_fn(|channel| (sums[channel] / 9) as u8)
}

/// The biome id at a column position: the column's biome array when the column
/// is loaded, the fallback biome's id (ocean, 0) when it is not.
fn biome_id_at(world: &World, x: i32, z: i32) -> u8 {
    match world.chunk(x >> 4, z >> 4) {
        Some(chunk) => chunk.biome((x & 15) as usize, (z & 15) as usize),
        None => 0,
    }
}

/// The colour-map colour at a biome's height-adjusted temperature and its
/// (not adjusted) rainfall.
fn map_colour(map: &ColorMap, data: &BiomeData, x: i32, y: i32, z: i32) -> [u8; 3] {
    colormap_colour(
        map,
        height_adjusted_temperature(data.temperature, x, y, z),
        height_adjusted_rainfall(data.rainfall, x, y, z),
    )
}

/// The grass colour: the map colour, with the source's overrides.
///
/// * Swamp and its mutation answer one of two fixed colours, chosen by the
///   second Perlin (`BiomeGenSwamp.java:37-41`); the colour map is not read and
///   neither is y.
/// * The mesa rows answer one fixed colour (`BiomeGenMesa.java:61-64`).
/// * Roofed forest and its mutation halve the map colour
///   (`BiomeGenForest.java:167-171`), which is what darkens that biome's grass;
///   the mutation delegates there (`BiomeGenMutated.java:68-71`).
fn grass_colour(data: &BiomeData, maps: &TintMaps, x: i32, y: i32, z: i32) -> [u8; 3] {
    match data.id {
        6 | 134 => {
            let noise = perlin_sample(
                SWAMP_NOISE_SEED,
                1,
                f64::from(x) * SWAMP_NOISE_SCALE,
                f64::from(z) * SWAMP_NOISE_SCALE,
            );
            if noise < SWAMP_NOISE_THRESHOLD {
                rgb(SWAMP_GRASS_DARK)
            } else {
                rgb(SWAMP_GRASS_LIGHT)
            }
        }
        37 | 165 | 166 | 167 => rgb(MESA_GRASS),
        29 | 157 => roofed_forest_grass(map_colour(&maps.grass, data, x, y, z)),
        _ => map_colour(&maps.grass, data, x, y, z),
    }
}

/// The three bytes of a packed `0xRRGGBB` colour, masked the way the colour
/// helpers mask one (`BiomeColorHelper.java:39-41`).
fn rgb(value: u32) -> [u8; 3] {
    [(value >> 16) as u8, (value >> 8) as u8, value as u8]
}

/// `((colour & 16711422) + 2634762) >> 1`: `BiomeGenForest.getGrassColorAtPos`'s
/// roofed-forest branch (`BiomeGenForest.java:170`).
fn roofed_forest_grass(colour: [u8; 3]) -> [u8; 3] {
    let packed = (u32::from(colour[0]) << 16) | (u32::from(colour[1]) << 8) | u32::from(colour[2]);
    rgb(((packed & 16711422) + 2634762) >> 1)
}

/// A water multiplier's channels scaled back to bytes.
///
/// The source divides each channel by 255 to reach the renderer's floats
/// (`BlockModelRenderer.java:287-289`) and truncates `channel * 255.0` on the
/// way to a byte (`WorldRenderer.java:368`); the round trip is exact for the
/// eight-bit channels a multiplier carries.
fn scale_water(colour: [f32; 3]) -> [u8; 3] {
    std::array::from_fn(|channel| (colour[channel] * 255.0) as u8)
}

/// The foliage colour: the map colour, with the fixed mesa and swamp overrides.
fn foliage_colour(data: &BiomeData, maps: &TintMaps, x: i32, y: i32, z: i32) -> [u8; 3] {
    match data.id {
        6 | 134 => rgb(SWAMP_GRASS_LIGHT),
        37 | 165 | 166 | 167 => rgb(MESA_FOLIAGE),
        _ => map_colour(&maps.foliage, data, x, y, z),
    }
}
