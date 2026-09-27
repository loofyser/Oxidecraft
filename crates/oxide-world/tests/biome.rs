//! Tests for the biome table and the tint path: the pinned source numbers, the
//! JVM-pinned noise and LCG vectors, the colour-map lookup, the three-by-three
//! neighbourhood average and the fixed swamp, mesa and roofed-forest colours.
//!
//! # Provenance of the non-literal expectations
//!
//! The values marked "harness vector" were printed by the uncommitted vector
//! harness under `refs/_src/vectors/` (javac/java 27), which copies the
//! decompiled noise classes and runs them on the local JVM:
//!
//! ```text
//! cd refs/_src/vectors
//! javac -d classes VectorHarness.java net/minecraft/util/*.java \
//!     net/minecraft/world/*.java net/minecraft/world/gen/*.java
//! java -cp classes VectorHarness
//! ```
//!
//! Classes on that compile: `NoiseGenerator`, `NoiseGeneratorOctaves`,
//! `NoiseGeneratorPerlin`, `NoiseGeneratorSimplex`, `NoiseGeneratorImproved`
//! (all from `world/gen/`), `ColorizerGrass` (from `world/`) and a reduced
//! `MathHelper` carrying only `floor_double_long` and `clamp_float` (the full
//! class drags in `Vec3i` and Guava). `NoiseGeneratorPerlin` over
//! `NoiseGeneratorSimplex` is the generator the temperature noise and the swamp
//! override use; no vector here exercises `NoiseGeneratorOctaves`, which is over
//! `NoiseGeneratorImproved` and not on the height-adjustment path. Every number
//! is printed with `Double.toString`/`Float.toString`, which round-trips exactly,
//! and the literals below are those digits.

use oxide_proto_v47::column::{ColumnData, SectionData};
use oxide_world::behaviour::TintKind;
use oxide_world::biome::{
    BIOME_TABLE, ColorMap, ColorMapError, TintMaps, biome, colormap_colour,
    height_adjusted_rainfall, height_adjusted_temperature, tint_at, tint_at_9,
};
use oxide_world::noise::{JavaRandom, perlin_sample};
use oxide_world::world::World;

/// Plains (`BiomeGenBase.java:73`, id 1; the constructor's `0.8F, 0.4F` at
/// `BiomeGenPlains.java:17`).
const PLAINS: u8 = 1;
/// Desert (`BiomeGenBase.java:74`).
const DESERT: u8 = 2;
/// Swampland (`BiomeGenBase.java:78`; the override at `BiomeGenSwamp.java:37-46`).
const SWAMP: u8 = 6;
/// Roofed forest (`BiomeGenBase.java:115`; the transform at `BiomeGenForest.java:167-171`).
const ROOFED_FOREST: u8 = 29;
/// Mesa (`BiomeGenBase.java:123`; the fixed colours at `BiomeGenMesa.java:56-64`).
const MESA: u8 = 37;

/// The dark swamp grass colour (`BiomeGenSwamp.java:40`).
const SWAMP_GRASS_DARK: u32 = 5011004;
/// The light swamp grass colour, also swamp's foliage colour (`BiomeGenSwamp.java:40`, `:45`).
const SWAMP_GRASS_LIGHT: u32 = 6975545;
/// Mesa's fixed grass colour (`BiomeGenMesa.java:63`).
const MESA_GRASS: u32 = 9470285;
/// Mesa's fixed foliage colour (`BiomeGenMesa.java:58`).
const MESA_FOLIAGE: u32 = 10387789;
/// Swamp's water multiplier (`BiomeGenSwamp.java:28`).
const SWAMP_WATER: u32 = 14745518;

// ---------------------------------------------------------------------------
// Harness vectors (see the module comment for the command and the JVM).
// ---------------------------------------------------------------------------

/// `TEMPERATURE_NOISE.func_151601_a(x / 8.0, z / 8.0)`, the temperature noise
/// seeded `new Random(1234L)` with one octave (`BiomeGenBase.java:649`).
const TEMPERATURE_SAMPLES: [(i32, i32, f64); 26] = [
    (0, 0, 0.0),
    (8, 0, 0.3465103646672414),
    (0, 8, -0.4950942124223869),
    (8, 8, -0.8928152969943598),
    (1, 2, -0.8569647822162618),
    (3, 4, -0.23634809370102922),
    (16, 16, -0.4989247905504145),
    (-5, 7, -0.43100454734936255),
    (-100, 200, -0.4670307986669561),
    (1000, -1000, 1.3600232051658168E-14),
    (12, -34, 0.24799802175132551),
    (64, 64, 0.6488656853952397),
    (-64, -64, 0.32443232755205353),
    (7, 13, 0.591015454159227),
    (99, -1, 0.20525428816496544),
    (256, 256, -0.21296103333786495),
    (-256, 256, 1.3600232051658168E-14),
    (33, 77, -0.2924006344557984),
    (2048, 1024, 0.3280407662917396),
    (13, 42, 0.9118899488390928),
    (50, 50, 0.37368971650267174),
    (-8, -8, 0.9296548735244428),
    (17, 19, 0.20159641721985438),
    (123, 456, -0.7318332089972048),
    (-123, -456, -0.16355193334428264),
    (255, -255, 0.42180866534132877),
];

/// `getFloatTemperature` for a plains biome (base `0.8F`) at y = 64, 70, 100 and
/// 150, per `(x, z)` of [`TEMPERATURE_SAMPLES`].
const PLAINS_TEMPERATURES: [(i32, i32, [f32; 4]); 26] = [
    (0, 0, [0.8, 0.79, 0.74, 0.65666664]),
    (8, 0, [0.8, 0.7876899, 0.73769, 0.6543566]),
    (0, 8, [0.8, 0.7933006, 0.7433006, 0.6599673]),
    (8, 8, [0.8, 0.7959521, 0.7459521, 0.66261876]),
    (1, 2, [0.8, 0.7957131, 0.7457131, 0.66237974]),
    (3, 4, [0.8, 0.7915757, 0.74157566, 0.65824234]),
    (16, 16, [0.8, 0.7933262, 0.7433262, 0.6599928]),
    (-5, 7, [0.8, 0.7928734, 0.7428734, 0.65954006]),
    (-100, 200, [0.8, 0.7931135, 0.7431136, 0.6597802]),
    (1000, -1000, [0.8, 0.79, 0.74, 0.65666664]),
    (12, -34, [0.8, 0.7883467, 0.7383467, 0.6550134]),
    (64, 64, [0.8, 0.7856743, 0.73567426, 0.6523409]),
    (-64, -64, [0.8, 0.78783715, 0.73783714, 0.6545038]),
    (7, 13, [0.8, 0.7860599, 0.7360599, 0.6527266]),
    (99, -1, [0.8, 0.7886317, 0.73863167, 0.65529835]),
    (256, 256, [0.8, 0.79141974, 0.74141973, 0.6580864]),
    (-256, 256, [0.8, 0.79, 0.74, 0.65666664]),
    (33, 77, [0.8, 0.79194933, 0.7419493, 0.658616]),
    (2048, 1024, [0.8, 0.78781307, 0.73781306, 0.65447974]),
    (13, 42, [0.8, 0.78392076, 0.73392075, 0.65058744]),
    (50, 50, [0.8, 0.7875087, 0.7375088, 0.6541754]),
    (-8, -8, [0.8, 0.78380233, 0.7338023, 0.65046895]),
    (17, 19, [0.8, 0.78865606, 0.73865604, 0.6553227]),
    (123, 456, [0.8, 0.7948789, 0.7448789, 0.6615456]),
    (-123, -456, [0.8, 0.79109037, 0.74109036, 0.65775704]),
    (255, -255, [0.8, 0.78718793, 0.737188, 0.6538546]),
];

/// A four-octave `NoiseGeneratorPerlin` seeded `new Random(42L)`, sampled at
/// `x / 8.0, z / 8.0`: pins the octave summation, not just one octave.
const FOUR_OCTAVE_SAMPLES: [(i32, i32, f64); 6] = [
    (0, 0, 0.0),
    (8, 8, 0.9234112777719807),
    (-5, 7, -1.015884738827754),
    (1000, -1000, 2.910815938501246),
    (123, 456, 0.35299324786135333),
    (-256, 256, -1.7486012637846216E-14),
];

/// `java.util.Random`, seed 1234: raw `next(31)` draws.
const NEXT_BITS: [u32; 8] = [
    1388524628, 557894633, 2043025133, 509900220, 1841657210, 681066393, 984048529, 1180517449,
];

/// The same, `nextFloat`.
const NEXT_FLOATS: [f32; 8] = [
    0.6465821, 0.25978988, 0.95135766, 0.23744076, 0.8575884, 0.31714624, 0.4582333, 0.54972124,
];

/// The same, `nextInt(256)` (the power-of-two branch).
const NEXT_INT_256: [u32; 8] = [165, 66, 243, 60, 219, 81, 117, 140];

/// The same, `nextInt(37)` (the rejection branch).
const NEXT_INT_37: [u32; 8] = [24, 12, 18, 1, 7, 30, 7, 0];

/// The same, `nextDouble`.
const NEXT_DOUBLES: [f64; 8] = [
    0.6465821602909256,
    0.9513577109193919,
    0.8575884598068334,
    0.45823330506267057,
    0.3359524025416939,
    0.20387478195313158,
    0.34690742873967684,
    0.617314071997303,
];

/// `getGrassColorAtPos` for a plains biome through the synthetic coordinate map
/// (below), per `(x, z)` and y = 64, 70, 100 and 150. Pins the height
/// adjustment, the clamp, the index arithmetic and the map lookup together.
const PLAINS_GRASS_COLOURS: [(i32, i32, [u32; 4]); 6] = [
    (0, 0, [4281511168, 4281708032, 4282561280, 4283939840]),
    (8, 0, [4281511168, 4281773568, 4282561280, 4284005376]),
    (0, 8, [4281511168, 4281642496, 4282495744, 4283874048]),
    (8, 8, [4281511168, 4281642240, 4282429952, 4283874048]),
    (1, 2, [4281511168, 4281642240, 4282429952, 4283874048]),
    (3, 4, [4281511168, 4281708032, 4282495744, 4283939584]),
];

/// The same for a desert biome (temperature 2.0, rainfall 0.0), per y.
const DESERT_GRASS_COLOURS: [u32; 4] = [4278255360, 4278255360, 4278255360, 4278255360];

/// Roofed forest's grass, per `(x, z)` and y = 64, 70: the map colour with
/// `BiomeGenForest.getGrassColorAtPos`'s roofed-forest transform.
const ROOFED_GRASS_COLOURS: [(i32, i32, [u32; 2]); 6] = [
    (0, 0, [3822085, 3887877]),
    (8, 0, [3822085, 3887877]),
    (0, 8, [3822085, 3887621]),
    (8, 8, [3822085, 3822085]),
    (1, 2, [3822085, 3822085]),
    (3, 4, [3822085, 3887621]),
];

/// The roofed-forest transform over flat base colours whose low bits the
/// coordinate map never sets: `(base, ((base & 16711422) + 2634762) >> 1)`,
/// pinning the mask's width bit by bit (`BiomeGenForest.java:170`).
const ROOFED_MASK_SAMPLES: [(u32, u32); 8] = [
    (16711422, 9673092),
    (5200386, 3884806),
    (5177856, 3873541),
    (154112, 1394437),
    (5135114, 3884810),
    (5069378, 3819302),
    (5003906, 3819334),
    (0, 1317381),
];

/// Swamp's grass override: `GRASS_COLOR_NOISE` samples (seed 2345) at
/// `x * 0.0225, z * 0.0225`, and the colour that follows, per `(x, z)`.
const SWAMP_SAMPLES: [(i32, i32, f64, u32); 10] = [
    (-64, -64, 0.017498703061163638, 6975545),
    (-64, -63, 0.07923754268035402, 6975545),
    (-64, -62, 0.1449407579981205, 6975545),
    (-64, -61, 0.21297588478853602, 6975545),
    (-64, -60, 0.2817317995967429, 6975545),
    (-64, -15, -0.11864018719545448, 5011004),
    (-64, -14, -0.20915927447294155, 5011004),
    (-64, -13, -0.29458012679847934, 5011004),
    (-64, -12, -0.3728635911747763, 5011004),
    (-64, -11, -0.4421798488364103, 5011004),
];

// ---------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------

/// The three bytes of a packed `0xRRGGBB` colour; alpha is not part of a tint.
fn rgb(value: u32) -> [u8; 3] {
    [(value >> 16) as u8, (value >> 8) as u8, value as u8]
}

/// One air section, so a column the tests build is applied rather than taken
/// for an unload (`World::apply_column`).
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

/// A column whose biome id is picked per column position, `f(x, z)`.
fn column_with(f: impl Fn(usize, usize) -> u8) -> ColumnData {
    let mut data = ColumnData::empty();
    data.mask = 1;
    data.sections[0] = Some(air_section());
    let mut biomes = [0u8; 256];
    for z in 0..16 {
        for x in 0..16 {
            biomes[(z << 4) | x] = f(x, z);
        }
    }
    data.biomes = Some(biomes);
    data
}

/// A world with `id` at every position, covering chunk coordinates -4..=1 in
/// both axes.
fn world_of(id: u8) -> World {
    let mut world = World::new(true);
    for cx in -4i32..=1 {
        for cz in -4i32..=1 {
            world.apply_column(cx, cz, &column(id), true);
        }
    }
    world
}

/// A world with a north-south seam in every chunk: local x < 8 is plains, the
/// rest desert.
fn seam_world() -> World {
    let mut world = World::new(true);
    for cx in -1i32..=1 {
        for cz in -1i32..=1 {
            world.apply_column(
                cx,
                cz,
                &column_with(|x, _z| if x < 8 { PLAINS } else { DESERT }),
                true,
            );
        }
    }
    world
}

/// A 256 x 256 RGBA map whose pixel (x, y) encodes its own coordinates: red is
/// x, green is y, blue is 0, alpha is 255. The lookup position is then directly
/// readable from the colour `colormap_colour` returns.
fn coordinate_map() -> ColorMap {
    let mut bytes = vec![0u8; 256 * 256 * 4];
    for y in 0..256 {
        for x in 0..256 {
            let offset = (y * 256 + x) * 4;
            bytes[offset] = x as u8;
            bytes[offset + 1] = y as u8;
            bytes[offset + 2] = 0;
            bytes[offset + 3] = 255;
        }
    }
    ColorMap::from_rgba(&bytes).expect("a 256 x 256 RGBA map is accepted")
}

/// A map whose every pixel is one colour.
fn flat_map(red: u8, green: u8, blue: u8, alpha: u8) -> ColorMap {
    let pixel = [red, green, blue, alpha];
    let mut bytes = vec![0u8; 256 * 256 * 4];
    for chunk in bytes.chunks_exact_mut(4) {
        chunk.copy_from_slice(&pixel);
    }
    ColorMap::from_rgba(&bytes).expect("a 256 x 256 RGBA map is accepted")
}

/// The two maps the grass and foliage lookups read.
fn maps() -> TintMaps {
    TintMaps {
        grass: coordinate_map(),
        foliage: coordinate_map(),
    }
}

/// The truncated per-channel mean vanilla takes over samples
/// (`BiomeColorHelper.java:44`): each channel summed, divided by the count.
fn mean_of(weighted: &[(u32, [u8; 3])]) -> [u8; 3] {
    let mut sums = [0u32; 3];
    let mut count = 0u32;
    for &(weight, colour) in weighted {
        for (sum, channel) in sums.iter_mut().zip(colour) {
            *sum += u32::from(channel) * weight;
        }
        count += weight;
    }
    std::array::from_fn(|channel| (sums[channel] / count) as u8)
}

/// The colours a height's index in the vector arrays stands for.
const HEIGHTS: [i32; 4] = [64, 70, 100, 150];

// ---------------------------------------------------------------------------
// The table.
// ---------------------------------------------------------------------------

#[test]
fn biome_table_rows_pin_the_source_numbers() {
    let rows: [(u8, &str, f32, f32, [f32; 3]); 18] = [
        (0, "Ocean", 0.5, 0.5, [1.0, 1.0, 1.0]),
        (PLAINS, "Plains", 0.8, 0.4, [1.0, 1.0, 1.0]),
        (DESERT, "Desert", 2.0, 0.0, [1.0, 1.0, 1.0]),
        (4, "Forest", 0.7, 0.8, [1.0, 1.0, 1.0]),
        (
            SWAMP,
            "Swampland",
            0.8,
            0.9,
            [224.0 / 255.0, 1.0, 174.0 / 255.0],
        ),
        (8, "Hell", 2.0, 0.0, [1.0, 1.0, 1.0]),
        (21, "Jungle", 0.95, 0.9, [1.0, 1.0, 1.0]),
        (ROOFED_FOREST, "Roofed Forest", 0.7, 0.8, [1.0, 1.0, 1.0]),
        (MESA, "Mesa", 2.0, 0.0, [1.0, 1.0, 1.0]),
        (38, "Mesa Plateau F", 2.0, 0.0, [1.0, 1.0, 1.0]),
        (39, "Mesa Plateau", 2.0, 0.0, [1.0, 1.0, 1.0]),
        (129, "Sunflower Plains", 0.8, 0.4, [1.0, 1.0, 1.0]),
        (132, "Flower Forest", 0.7, 0.8, [1.0, 1.0, 1.0]),
        (
            134,
            "Swampland M",
            0.8,
            0.9,
            [224.0 / 255.0, 1.0, 174.0 / 255.0],
        ),
        (140, "Ice Plains Spikes", 0.0, 0.5, [1.0, 1.0, 1.0]),
        (160, "Mega Spruce Taiga", 0.25, 0.8, [1.0, 1.0, 1.0]),
        (163, "Savanna M", 1.1, 0.0, [1.0, 1.0, 1.0]),
        (165, "Mesa (Bryce)", 2.0, 0.0, [1.0, 1.0, 1.0]),
    ];
    for (id, name, temperature, rainfall, water_colour) in rows {
        let row = biome(id);
        assert_eq!(row.id, id, "biome({id}) answers for its own id");
        assert_eq!(row.name, name, "biome({id})'s name");
        assert_eq!(row.temperature, temperature, "biome({id})'s temperature");
        assert_eq!(row.rainfall, rainfall, "biome({id})'s rainfall");
        assert_eq!(
            row.water_colour, water_colour,
            "biome({id})'s water multiplier"
        );
    }
    // The water multiplier is a colour, so its bytes are the source's channels:
    // 16777215 is white, swamp's 14745518 is (224, 255, 174).
    assert_eq!(rgb(16777215), [255, 255, 255]);
    assert_eq!(rgb(SWAMP_WATER), [224, 255, 174]);
}

#[test]
fn biome_table_covers_the_source_id_space_and_falls_back_to_ocean() {
    let ids: Vec<u8> = BIOME_TABLE.iter().map(|row| row.id).collect();
    assert_eq!(
        ids.len(),
        61,
        "0-39 plus the 21 mutations the source registers"
    );
    assert!(
        ids.windows(2).all(|pair| pair[0] < pair[1]),
        "the table is ascending by id: {ids:?}"
    );
    for id in 0u8..=39 {
        assert_eq!(biome(id).id, id, "id {id} is registered");
    }
    for id in [
        129u8, 130, 131, 132, 133, 134, 140, 149, 151, 155, 156, 157, 158, 160, 161, 162, 163, 164,
        165, 166, 167,
    ] {
        assert_eq!(biome(id).id, id, "mutation {id} is registered");
    }
    // The source's holes inside 128-167 and everything outside it fall back to
    // ocean (`BiomeGenBase.java:584-600` returns the ocean entry out of bounds;
    // the plan pins the same answer for a null slot).
    for id in [
        40u8, 41, 100, 127, 128, 135, 139, 141, 148, 150, 152, 154, 159, 168, 200, 255,
    ] {
        let row = biome(id);
        assert_eq!(row.id, 0, "id {id} is not registered, so it is ocean");
        assert_eq!(row.name, "Ocean");
    }
}

// ---------------------------------------------------------------------------
// Height adjustment and the noise.
// ---------------------------------------------------------------------------

#[test]
fn perlin_sample_matches_the_jvm_temperature_noise() {
    for (x, z, expected) in TEMPERATURE_SAMPLES {
        let got = perlin_sample(1234, 1, f64::from(x) / 8.0, f64::from(z) / 8.0);
        assert_eq!(got, expected, "temperature noise at ({x}, {z})");
    }
}

#[test]
fn perlin_sample_matches_the_jvm_four_octave_generator() {
    for (x, z, expected) in FOUR_OCTAVE_SAMPLES {
        let got = perlin_sample(42, 4, f64::from(x) / 8.0, f64::from(z) / 8.0);
        assert_eq!(got, expected, "four-octave noise at ({x}, {z})");
    }
}

#[test]
fn perlin_sample_of_no_octaves_is_zero() {
    // `NoiseGeneratorPerlin.func_151601_a` sums over its octaves; none sums to 0.
    assert_eq!(perlin_sample(1234, 0, 0.5, 0.5), 0.0);
}

#[test]
fn height_adjusted_temperature_matches_the_jvm_vectors() {
    let base = biome(PLAINS).temperature;
    for (x, z, expected) in PLAINS_TEMPERATURES {
        for (index, y) in HEIGHTS.into_iter().enumerate() {
            let got = height_adjusted_temperature(base, x, y, z);
            assert!(
                (got - expected[index]).abs() < 1e-5,
                "plains temperature at ({x}, {y}, {z}): got {got}, want {}",
                expected[index]
            );
        }
    }
}

#[test]
fn temperature_at_or_below_sea_level_is_the_base_value_exactly() {
    // `BiomeGenBase.getFloatTemperature` guards on `pos.getY() > 64`.
    for y in [0, 1, 63, 64] {
        for (x, z, _) in TEMPERATURE_SAMPLES {
            assert_eq!(
                height_adjusted_temperature(0.8, x, y, z),
                0.8,
                "y = {y} takes the base value"
            );
        }
    }
    assert!(
        height_adjusted_temperature(0.8, 8, 65, 0) < 0.8,
        "y = 65 is above the guard and the noise term lowers the value"
    );
}

#[test]
fn height_adjusted_rainfall_is_the_base_value() {
    // `getFloatRainfall` takes no position (`BiomeGenBase.java:399-401`).
    for y in [0, 64, 65, 100, 150, 255] {
        assert_eq!(height_adjusted_rainfall(0.9, -7, y, 12), 0.9);
    }
    for id in [0u8, PLAINS, DESERT, SWAMP, ROOFED_FOREST, MESA, 39] {
        let row = biome(id);
        assert_eq!(
            height_adjusted_rainfall(row.rainfall, 0, 100, 0),
            row.rainfall,
            "biome({id}) rainfall is not adjusted"
        );
    }
}

// ---------------------------------------------------------------------------
// The colour map.
// ---------------------------------------------------------------------------

#[test]
fn colormap_colour_pins_the_source_index_lookup() {
    let map = coordinate_map();
    // (temperature, rainfall, expected) — the index is
    // (255 * (1 - t), 255 * (1 - rain * t)), truncating, and the synthetic map
    // encodes the index in its own channels.
    let cases: [(f32, f32, [u8; 3]); 7] = [
        (0.0, 0.0, [255, 255, 0]),
        (1.0, 1.0, [0, 0, 0]),
        (0.5, 0.5, [127, 191, 0]),
        (0.8, 0.4, [50, 173, 0]),
        (2.0, 0.0, [0, 255, 0]),
        (-1.0, 2.0, [255, 255, 0]),
        (0.25, 0.75, [191, 207, 0]),
    ];
    for (temperature, rainfall, expected) in cases {
        assert_eq!(
            colormap_colour(&map, temperature, rainfall),
            expected,
            "the {temperature} / {rainfall} lookup"
        );
    }
}

#[test]
fn colormap_colour_drops_the_alpha_channel() {
    // Only red, green and blue become a tint; the colour map pixels' alpha is
    // not part of the value the renderer masks (`BlockModelRenderer.java:287-289`).
    let map = flat_map(10, 20, 30, 40);
    assert_eq!(colormap_colour(&map, 0.5, 0.5), [10, 20, 30]);
}

#[test]
fn colormap_with_the_wrong_length_names_the_value() {
    let error = ColorMap::from_rgba(&[0u8; 10]).expect_err("ten bytes is not a colour map");
    assert_eq!(
        error,
        ColorMapError::WrongLength {
            expected: 256 * 256 * 4,
            got: 10
        }
    );
    assert!(error.to_string().contains("262144"), "{error}");
    assert!(error.to_string().contains("10"), "{error}");
}

#[test]
fn colormap_pixel_answers_only_inside_the_map() {
    let map = coordinate_map();
    assert_eq!(map.pixel(3, 7), Some([3, 7, 0, 255]));
    assert_eq!(map.pixel(255, 255), Some([255, 255, 0, 255]));
    assert_eq!(map.pixel(256, 0), None);
    assert_eq!(map.pixel(0, 256), None);
}

// ---------------------------------------------------------------------------
// The tint path.
// ---------------------------------------------------------------------------

#[test]
fn tint_at_grass_matches_the_jvm_vectors() {
    let world = world_of(PLAINS);
    let maps = maps();
    for (x, z, expected) in PLAINS_GRASS_COLOURS {
        for (index, y) in HEIGHTS.into_iter().enumerate() {
            let got = tint_at(&world, &maps, x, y, z, TintKind::Grass);
            assert_eq!(got, rgb(expected[index]), "plains grass at ({x}, {y}, {z})");
        }
    }
    // Desert's flat colour, pinned at one position per height: temperature 2.0
    // clamps to 1, rainfall 0.0 gives the last row, so the map's first column.
    let desert = world_of(DESERT);
    for (index, y) in HEIGHTS.into_iter().enumerate() {
        assert_eq!(
            tint_at(&desert, &maps, 0, y, 0, TintKind::Grass),
            rgb(DESERT_GRASS_COLOURS[index]),
            "desert grass at y = {y}"
        );
    }
    assert_eq!(rgb(DESERT_GRASS_COLOURS[0]), [0, 255, 0]);
}

#[test]
fn tint_at_resolves_the_kind_to_the_right_map() {
    let maps = TintMaps {
        grass: flat_map(1, 2, 3, 255),
        foliage: flat_map(4, 5, 6, 255),
    };
    let world = world_of(PLAINS);
    assert_eq!(tint_at(&world, &maps, 0, 64, 0, TintKind::Grass), [1, 2, 3]);
    assert_eq!(
        tint_at(&world, &maps, 0, 64, 0, TintKind::GrassSideOverlay),
        [1, 2, 3],
        "the side overlay is the grass colour"
    );
    assert_eq!(
        tint_at(&world, &maps, 0, 64, 0, TintKind::Foliage),
        [4, 5, 6]
    );
    assert_eq!(
        tint_at(&world, &maps, 0, 64, 0, TintKind::None),
        [255, 255, 255],
        "an untinted quad's colour is the neutral multiplier"
    );
}

#[test]
fn water_tint_is_the_multiplier_scaled_to_bytes() {
    let world_ocean = world_of(0);
    let world_swamp = world_of(SWAMP);
    let maps = maps();
    assert_eq!(
        tint_at(&world_ocean, &maps, 0, 64, 0, TintKind::Water),
        [255, 255, 255]
    );
    assert_eq!(
        tint_at(&world_swamp, &maps, 0, 64, 0, TintKind::Water),
        rgb(SWAMP_WATER)
    );
    assert_eq!(rgb(SWAMP_WATER), [224, 255, 174]);
    // The multiplier does not depend on y or on the maps.
    assert_eq!(
        tint_at(&world_swamp, &maps, 0, 200, 0, TintKind::Water),
        rgb(SWAMP_WATER)
    );
}

#[test]
fn swamp_overrides_ignore_the_colour_map() {
    let world = world_of(SWAMP);
    let maps = maps();
    let other = TintMaps {
        grass: flat_map(7, 8, 9, 255),
        foliage: flat_map(7, 8, 9, 255),
    };
    let mut dark = 0;
    let mut light = 0;
    for (x, z, noise, colour) in SWAMP_SAMPLES {
        let sample = perlin_sample(2345, 1, f64::from(x) * 0.0225, f64::from(z) * 0.0225);
        assert_eq!(sample, noise, "swamp noise at ({x}, {z})");
        assert_eq!(
            sample < -0.1,
            colour == SWAMP_GRASS_DARK,
            "the branch the source takes at ({x}, {z})"
        );
        assert_eq!(
            tint_at(&world, &maps, x, 64, z, TintKind::Grass),
            rgb(colour),
            "swamp grass at ({x}, {z})"
        );
        assert_eq!(
            tint_at(&world, &other, x, 64, z, TintKind::Grass),
            rgb(colour),
            "swamp grass at ({x}, {z}) ignores the map"
        );
        assert_eq!(
            tint_at(&world, &maps, x, 90, z, TintKind::Grass),
            rgb(colour),
            "swamp grass at ({x}, {z}) ignores y"
        );
        assert_eq!(
            tint_at(&world, &maps, x, 64, z, TintKind::Foliage),
            rgb(SWAMP_GRASS_LIGHT),
            "swamp foliage is fixed at ({x}, {z})"
        );
        if colour == SWAMP_GRASS_DARK {
            dark += 1;
        } else {
            light += 1;
        }
    }
    assert_eq!(
        (dark, light),
        (5, 5),
        "the vectors cover both sides of the -0.1 branch"
    );
    assert_eq!(rgb(SWAMP_GRASS_DARK), [76, 118, 60]);
    assert_eq!(rgb(SWAMP_GRASS_LIGHT), [106, 112, 57]);
}

#[test]
fn mesa_grass_and_foliage_are_the_fixed_tints() {
    // All six mesa rows answer both fixed colours: 37, 38 and 39 are
    // `BiomeGenMesa` instances (`BiomeGenBase.java:123-125`) and the class
    // overrides both colour methods without a condition
    // (`BiomeGenMesa.java:56-63`); 165, 166 and 167 are their mutations, each
    // built as a `BiomeGenMesa` too (`BiomeGenMesa.java:317-334`).
    for id in [MESA, 38u8, 39, 165, 166, 167] {
        let world = world_of(id);
        let maps = maps();
        let other = TintMaps {
            grass: flat_map(1, 1, 1, 255),
            foliage: flat_map(2, 2, 2, 255),
        };
        for (x, y, z) in [(0, 64, 0), (3, 70, 4), (7, 150, 11)] {
            assert_eq!(
                tint_at(&world, &maps, x, y, z, TintKind::Grass),
                rgb(MESA_GRASS),
                "mesa grass for id {id} at ({x}, {y}, {z})"
            );
            assert_eq!(
                tint_at(&world, &other, x, y, z, TintKind::Grass),
                rgb(MESA_GRASS),
                "mesa grass for id {id} ignores the map"
            );
            assert_eq!(
                tint_at(&world, &maps, x, y, z, TintKind::Foliage),
                rgb(MESA_FOLIAGE),
                "mesa foliage for id {id} at ({x}, {y}, {z})"
            );
        }
    }
    assert_eq!(rgb(MESA_GRASS), [144, 129, 77]);
    assert_eq!(rgb(MESA_FOLIAGE), [158, 129, 77]);
}

#[test]
fn roofed_forest_grass_applies_the_source_transform() {
    // `BiomeGenForest.getGrassColorAtPos`: roofed forest (its type 3) answers
    // `((i & 16711422) + 2634762) >> 1` over the map colour, and its mutation
    // delegates there.
    let maps = maps();
    for id in [ROOFED_FOREST, 157u8] {
        let world = world_of(id);
        for (x, z, expected) in ROOFED_GRASS_COLOURS {
            for (index, y) in [64, 70].into_iter().enumerate() {
                assert_eq!(
                    tint_at(&world, &maps, x, y, z, TintKind::Grass),
                    rgb(expected[index]),
                    "roofed forest grass for id {id} at ({x}, {y}, {z})"
                );
            }
        }
    }
    assert_eq!(rgb(ROOFED_GRASS_COLOURS[0].2[0]), [58, 82, 5]);
}

#[test]
fn roofed_forest_mask_low_bits_are_pinned() {
    // The transform masks the map colour with 16711422 (0x00FEFEFE), which
    // clears bit 0 of each byte and keeps bit 1 of the blue byte; every pixel
    // of the coordinate map has blue 0, so a mask that cleared one more bit
    // (16711420) would answer the same colours there. These flat maps feed the
    // base colours that do set those bits, and the vectors above pin the
    // transform on them.
    let world = world_of(ROOFED_FOREST);
    for (base, expected) in ROOFED_MASK_SAMPLES {
        let map = flat_map((base >> 16) as u8, (base >> 8) as u8, base as u8, 255);
        let maps = TintMaps {
            grass: map,
            foliage: flat_map(0, 0, 0, 255),
        };
        assert_eq!(
            tint_at(&world, &maps, 0, 64, 0, TintKind::Grass),
            rgb(expected),
            "the roofed transform of {base:#08X}"
        );
    }
    assert_eq!(rgb(ROOFED_MASK_SAMPLES[0].1), [147, 153, 132]);
}

#[test]
fn tint_at_9_averages_the_neighbourhood_at_a_biome_seam() {
    let world = seam_world();
    let maps = maps();
    let plains = tint_at(&world, &maps, 7, 64, 3, TintKind::Grass);
    let desert = rgb(DESERT_GRASS_COLOURS[0]);
    assert_eq!(plains, rgb(PLAINS_GRASS_COLOURS[0].2[0]));
    // The centre (7, 64, 3) covers x = 6..8: two plains columns and one desert
    // column per row, so six plains samples and three desert ones.
    let expected = mean_of(&[(6, plains), (3, desert)]);
    assert_eq!(
        tint_at_9(&world, &maps, 7, 64, 3, TintKind::Grass),
        expected,
        "the seam average"
    );
    assert_ne!(expected, plains, "the seam really mixes the two colours");
}

#[test]
fn tint_at_9_in_a_biome_interior_is_tint_at() {
    let world = seam_world();
    let maps = maps();
    // At or below the sea level a biome's temperature is its base value, so
    // (3, 64, 3)'s neighbourhood is one colour and the average is that colour.
    let single = tint_at(&world, &maps, 3, 64, 3, TintKind::Grass);
    assert_eq!(tint_at_9(&world, &maps, 3, 64, 3, TintKind::Grass), single);
    // Above the sea level the height adjustment's noise depends on x and z, so
    // a neighbourhood's nine samples differ: the average is over each sample's
    // own colour, not nine times the centre's.
    let centre = tint_at(&world, &maps, 3, 100, 3, TintKind::Grass);
    let mut sums = [0u32; 3];
    let mut differing = 0;
    for offset_z in -1..=1 {
        for offset_x in -1..=1 {
            let colour = tint_at(
                &world,
                &maps,
                3 + offset_x,
                100,
                3 + offset_z,
                TintKind::Grass,
            );
            if colour != centre {
                differing += 1;
            }
            for (sum, channel) in sums.iter_mut().zip(colour) {
                *sum += u32::from(channel);
            }
        }
    }
    let average = tint_at_9(&world, &maps, 3, 100, 3, TintKind::Grass);
    assert_eq!(
        average,
        std::array::from_fn(|channel| (sums[channel] / 9) as u8)
    );
    assert!(
        differing > 0,
        "the adjusted noise varies across the neighbourhood"
    );
    assert_ne!(average, centre, "the average is not the centre's colour");
}

#[test]
fn tint_at_on_an_unloaded_column_takes_the_fallback_biome() {
    // A missing column is not an error on the tint path; the value is what the
    // fallback biome (ocean) answers. Ocean's temperature and rainfall are the
    // constructor defaults (0.5, 0.5): the map index is (127, 191).
    let empty = World::new(true);
    let loaded = world_of(0);
    let maps = maps();
    let ocean_colour = [127, 191, 0];
    assert_eq!(
        tint_at(&loaded, &maps, 0, 64, 0, TintKind::Grass),
        ocean_colour,
        "a loaded ocean column"
    );
    assert_eq!(
        tint_at(&empty, &maps, 1000, 64, 1000, TintKind::Grass),
        ocean_colour,
        "an unloaded column falls back to ocean"
    );
    assert_eq!(
        tint_at(&empty, &maps, -5, 64, 7, TintKind::Water),
        [255, 255, 255],
        "the fallback biome's water multiplier"
    );
    assert_eq!(
        tint_at_9(&empty, &maps, -5, 64, 7, TintKind::Grass),
        ocean_colour,
        "every sample of the average falls back the same way"
    );
}

// ---------------------------------------------------------------------------
// The LCG.
// ---------------------------------------------------------------------------

#[test]
fn java_random_matches_the_jvm_vectors() {
    let mut random = JavaRandom::new(1234);
    for expected in NEXT_BITS {
        assert_eq!(random.next_bits(31), expected);
    }

    let mut random = JavaRandom::new(1234);
    for expected in NEXT_FLOATS {
        assert_eq!(random.next_float(), expected);
    }

    let mut random = JavaRandom::new(1234);
    for expected in NEXT_INT_256 {
        assert_eq!(random.next_int(256), expected);
    }

    let mut random = JavaRandom::new(1234);
    for expected in NEXT_INT_37 {
        assert_eq!(random.next_int(37), expected);
    }

    let mut random = JavaRandom::new(1234);
    for expected in NEXT_DOUBLES {
        assert_eq!(random.next_double(), expected);
    }
}
