//! The weather columns and the splash spawns: the session-side half of the
//! rain and snow.
//!
//! Vanilla draws the rain and snow on the render thread
//! (`EntityRenderer.renderRainSnow:1594+`) and samples the splash particles
//! there too (`addRainParticles:1511+`, driven by `rendererUpdateCount`).
//! This port keeps both halves in the session, beside the particle engine:
//! the tick samples the splashes into the engine and computes the columns
//! the weather pass draws, so the client only feeds the pass. The
//! frame-vs-tick deviation is recorded: the source samples once per frame,
//! this port once per tick (20 Hz), so heavy rain spawns a third of the
//! source's splashes per second and the streak scroll steps per tick.
//!
//! The column shape mirrors the source's per-column values: the block range
//! `k2..l2` clamped to the precipitation height (`:1594+`), the rain/snow
//! split on the height-adjusted temperature (`0.15`), the distance-falloff
//! alpha (`f4`/`f5`) and the combined-light pair at `(x, i3, z)`.
//! Thunder has no visual in the rain pass — only the fog dim (landed) and
//! the lightning entity (recorded) — so nothing here reads it.

use rand::RngExt;
use rand::rngs::StdRng;

use oxide_world::behaviour::{LiquidKind, Material, behaviour};
use oxide_world::biome::{biome, height_adjusted_temperature};
use oxide_world::world::World;

use crate::particles::ParticleSpawn;
use crate::settings::ParticleSetting;

/// The rain/snow column radius at normal graphics: the source's `i1 = 5`
/// (`EntityRenderer.java:1594+`).
pub const WEATHER_RADIUS: i32 = 5;
/// The column radius at fancy graphics: the source's `i1 = 10`.
pub const WEATHER_RADIUS_FANCY: i32 = 10;
/// The wire id the splash sampler spawns: `droplet`, the rain fall
/// (`EntityRainFX.Factory`).
pub const WATER_DROP_ID: u8 = 39;
/// The wire id the lava arm spawns: `smoke` (`SMOKE_NORMAL`).
pub const SPLASH_SMOKE_ID: u8 = 11;
/// The rain/snow temperature split: `getTemperatureAtHeight(f2, j2) >= 0.15F`
/// rains, below it snows (`EntityRenderer.java:1594+`).
pub const RAIN_SNOW_SPLIT: f32 = 0.15;
/// The splash sampling window around the viewer's block: the precipitation
/// height must land within ±10 of it (`addRainParticles:1511+`).
pub const SPLASH_HALF_WINDOW: i32 = 10;
/// The splash sampling radius around the viewer's column: ±10 blocks
/// (`addRainParticles:1511+`).
pub const SPLASH_RADIUS: i32 = 10;

/// One rain/snow column for the weather pass, as the session computed it for
/// one tick.
///
/// The renderer turns these into the source's ribbon quads; the per-frame UV
/// scroll reads the tick counter the columns rode in on, so the columns
/// themselves stay stable while the viewer stands still.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeatherColumn {
    /// The column's block x.
    pub x: i32,
    /// The column's block z.
    pub z: i32,
    /// The streak's bottom: `max(viewer_y - radius, precipitation height)`.
    pub y0: i32,
    /// The streak's top: `max(viewer_y + radius, precipitation height)`.
    pub y1: i32,
    /// Whether the column draws the snow sheet (`temp < 0.15`).
    pub snow: bool,
    /// The distance-falloff alpha, rain strength included.
    pub alpha: f32,
    /// The block light at `(x, max(precip, eye), z)`.
    pub block_light: u8,
    /// The sky light at `(x, max(precip, eye), z)`.
    pub sky_light: u8,
}

/// Whether the block at `(x, y, z)` catches precipitation: the top-down scan
/// of [`precipitation_height`] stops here.
///
/// `Chunk.getPrecipitationHeight`'s own rule, as the recon re-read it:
/// leaves and snow layers count as solid. The material predicate alone would
/// let snow through (`Material.snow` neither blocks light nor movement), so
/// the snow material joins the movement-blocking materials and the liquids
/// explicitly.
fn catches_precipitation(id: u16) -> bool {
    behaviour(id).is_some_and(|row| {
        row.material.blocks_movement() || row.liquid.is_some() || row.material == Material::Snow
    })
}

/// The precipitation height of a column: the top catching block plus one.
///
/// `World.java:1565` through the chunk's own rule (see
/// [`catches_precipitation`]). `None` when the column is not loaded — an
/// unloaded column draws and spawns nothing.
pub fn precipitation_height(world: &World, x: i32, z: i32) -> Option<i32> {
    world.chunk(x.div_euclid(16), z.div_euclid(16))?;
    let mut y = 255;
    while y >= 0 {
        if catches_precipitation(world.block(x, y, z)) {
            return Some(y + 1);
        }
        y -= 1;
    }
    Some(0)
}

/// Whether precipitation falls in a biome at all: `BiomeGenBase.canRain()`,
/// which the desert, savanna and mesa rows fail with their `0.0` rainfall.
pub fn can_rain_here(biome_id: u8) -> bool {
    biome(biome_id).rainfall > 0.0
}

/// Whether the precipitation at `(x, y, z)` falls as snow: the
/// height-adjusted temperature below [`RAIN_SNOW_SPLIT`].
pub fn snows_here(biome_id: u8, x: i32, y: i32, z: i32) -> bool {
    height_adjusted_temperature(biome(biome_id).temperature, x, y, z) < RAIN_SNOW_SPLIT
}

/// The biome id of a column: the chunk's array when loaded, ocean (0) over
/// the void — the same fallback the sky's colour samples through.
fn column_biome(world: &World, x: i32, z: i32) -> u8 {
    match world.chunk(x.div_euclid(16), z.div_euclid(16)) {
        Some(chunk) => chunk.biome((x & 15) as usize, (z & 15) as usize),
        None => 0,
    }
}

/// The rain/snow columns around the viewer's feet block, for one tick.
///
/// `renderRainSnow`'s per-column values (`EntityRenderer.java:1594+`): the
/// square `(2·radius+1)²` box around the floored feet, the biome gate
/// (`canRain || getEnableSnow` — the snow biomes all carry rain-bearing
/// rainfall here, so the gate is [`can_rain_here`]), the `k2..l2` extent
/// clamped to the precipitation height (skipped when the clamp empties it),
/// the `f4`/`f5` distance alpha and the combined-light pair at the
/// precipitation/eye maximum. Columns over unloaded cells are skipped.
///
/// An empty vector when the rain strength is zero or below.
#[allow(clippy::too_many_arguments)]
pub fn weather_columns(
    world: &World,
    feet: [f64; 3],
    eye_height: f32,
    rain: f32,
    fancy: bool,
) -> Vec<WeatherColumn> {
    if rain <= 0.0 {
        return Vec::new();
    }
    let radius = if fancy {
        WEATHER_RADIUS_FANCY
    } else {
        WEATHER_RADIUS
    };
    let (cx, cy, cz) = (
        feet[0].floor() as i32,
        feet[1].floor() as i32,
        feet[2].floor() as i32,
    );
    let eye = (feet[1] + f64::from(eye_height)).floor() as i32;
    let mut columns = Vec::new();
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            let (x, z) = (cx + dx, cz + dz);
            let biome_id = column_biome(world, x, z);
            if !can_rain_here(biome_id) {
                continue;
            }
            let Some(precip) = precipitation_height(world, x, z) else {
                continue;
            };
            let y0 = (cy - radius).max(precip);
            let y1 = (cy + radius).max(precip);
            if y0 == y1 {
                continue;
            }
            let distance = ((dx * dx + dz * dz) as f32).sqrt() / radius as f32;
            let snow = snows_here(biome_id, x, precip, z);
            // The source's two falloffs: `((1 - f3²) * 0.5 + 0.5)` for rain,
            // `((1 - f6²) * 0.3 + 0.5)` for snow, each times the rain
            // strength.
            let falloff = if snow { 0.3 } else { 0.5 };
            let alpha = ((1.0 - distance * distance) * falloff + 0.5) * rain;
            let ly = precip.max(eye);
            columns.push(WeatherColumn {
                x,
                z,
                y0,
                y1,
                snow,
                alpha,
                block_light: world.block_light(x, ly, z).min(15),
                sky_light: world.sky_light(x, ly, z).min(15),
            });
        }
    }
    columns
}

/// One tick's splash spawns around the viewer's block.
///
/// `addRainParticles` (`EntityRenderer.java:1511+`): the try count is
/// [`rain_drops`](crate::particles::rain_drops) of the halved strength —
/// the source halves `f` past fancy graphics before the `100·f²` — and each
/// try samples a column in ±[`SPLASH_RADIUS`], reads its precipitation
/// height, and keeps it inside ±[`SPLASH_HALF_WINDOW`] of the viewer's block
/// under a raining biome at or above the [`RAIN_SNOW_SPLIT`] temperature. A
/// lava floor smokes (`SMOKE_NORMAL`); any other non-air floor rains a
/// [`WATER_DROP_ID`] at `maxY + 0.1`. The rain sound's reservoir counter is
/// the audio milestone's (T8/T9); this sampler only spawns.
pub fn splash_spawns(
    rng: &mut StdRng,
    world: &World,
    viewer: [i32; 3],
    rain: f32,
    fancy: bool,
    setting: ParticleSetting,
) -> Vec<ParticleSpawn> {
    use crate::particles::rain_drops;
    if rain <= 0.0 {
        return Vec::new();
    }
    let intensity = if fancy { rain } else { rain * 0.5 };
    let tries = rain_drops(intensity, setting);
    let mut spawns = Vec::new();
    for _ in 0..tries {
        let x = viewer[0] + rng.random_range(-SPLASH_RADIUS..=SPLASH_RADIUS);
        let z = viewer[2] + rng.random_range(-SPLASH_RADIUS..=SPLASH_RADIUS);
        let Some(precip) = precipitation_height(world, x, z) else {
            continue;
        };
        if precip > viewer[1] + SPLASH_HALF_WINDOW || precip < viewer[1] - SPLASH_HALF_WINDOW {
            continue;
        }
        let biome_id = column_biome(world, x, z);
        if !can_rain_here(biome_id) || snows_here(biome_id, x, precip, z) {
            continue;
        }
        let floor = world.block(x, precip - 1, z);
        let liquid = behaviour(floor).and_then(|row| row.liquid);
        if liquid == Some(LiquidKind::Lava) {
            spawns.push(ParticleSpawn {
                id: SPLASH_SMOKE_ID,
                pos: [
                    x as f32 + rng.random::<f32>(),
                    precip as f32,
                    z as f32 + rng.random::<f32>(),
                ],
                vel: [0.0, 0.0, 0.0],
                colour: None,
                atlas_sprite: None,
                item: None,
                scale: 1.0,
            });
        } else if floor != 0 {
            spawns.push(ParticleSpawn {
                id: WATER_DROP_ID,
                pos: [
                    x as f32 + rng.random::<f32>(),
                    precip as f32 + 0.1,
                    z as f32 + rng.random::<f32>(),
                ],
                vel: [0.0, 0.0, 0.0],
                colour: None,
                atlas_sprite: None,
                item: None,
                scale: 1.0,
            });
        }
    }
    spawns
}

#[cfg(test)]
mod tests {
    //! The precipitation, column and splash pins.
    //!
    //! The world fixtures are hand-built chunk columns — never the asset
    //! store — and the seeded runs pin the probabilistic rate,
    //! pinned, not flaky.

    use rand::SeedableRng;

    use super::*;
    use crate::particle_world::StillWorld;
    use crate::particles::{ParticleEngine, ParticleTex};
    use oxide_proto_v47::column::ColumnData;

    /// A world with the chunk square `cx0..=cx1 × cz0..=cz1` loaded, each
    /// column air over the void; the caller lays its own roof with
    /// [`World::set_block`]. Fresh columns default to the ocean biome, which
    /// rains.
    fn loaded_world(cx0: i32, cx1: i32, cz0: i32, cz1: i32) -> World {
        let mut world = World::new(true);
        for cx in cx0..=cx1 {
            for cz in cz0..=cz1 {
                world.apply_column(cx, cz, &ColumnData::empty(), false);
            }
        }
        world
    }

    /// A world with one loaded column: stone (id 1) at `top`, air above.
    fn column_world(top: i32) -> World {
        let mut world = loaded_world(0, 0, 0, 0);
        world.set_block(0, top, 0, 1);
        world
    }

    /// A stone roof of `id` at `top` over the whole loaded square.
    fn roof(world: &mut World, x0: i32, x1: i32, z0: i32, z1: i32, top: i32, id: u16) {
        for x in x0..=x1 {
            for z in z0..=z1 {
                world.set_block(x, top, z, id);
            }
        }
    }

    #[test]
    fn precipitation_height_is_the_top_solid_plus_one() {
        let world = column_world(63);
        assert_eq!(precipitation_height(&world, 0, 0), Some(64));
        assert_eq!(
            precipitation_height(&world, 24, 24),
            None,
            "an unloaded column draws and spawns nothing"
        );
    }

    #[test]
    fn precipitation_height_counts_leaves_and_snow_layers() {
        // Leaves (18) and a snow layer (78) both catch the streaks, while
        // tall grass (31) lets them through to the stone below.
        let mut world = loaded_world(0, 0, 0, 0);
        world.set_block(0, 60, 0, 1);
        world.set_block(4, 70, 0, 18);
        world.set_block(8, 68, 0, 78);
        world.set_block(12, 69, 0, 31);
        world.set_block(12, 60, 0, 1);
        assert_eq!(precipitation_height(&world, 4, 0), Some(71));
        assert_eq!(precipitation_height(&world, 8, 0), Some(69));
        assert_eq!(
            precipitation_height(&world, 12, 0),
            Some(61),
            "the grass is skipped, the stone catches"
        );
    }

    #[test]
    fn the_biome_gate_fails_where_nothing_falls() {
        assert!(!can_rain_here(2), "the desert carries no rain");
        assert!(!can_rain_here(35), "the savanna carries no rain");
        assert!(!can_rain_here(37), "the mesa carries no rain");
        assert!(can_rain_here(1), "the plains do");
        assert!(can_rain_here(12), "the ice plains do — as snow");
    }

    #[test]
    fn the_temperature_split_names_snow() {
        assert!(
            snows_here(12, 0, 70, 0),
            "the ice plains freeze at any height"
        );
        assert!(
            !snows_here(1, 0, 70, 0),
            "the warm plains rain at any height"
        );
    }

    #[test]
    fn clear_weather_builds_no_columns() {
        let world = column_world(63);
        assert!(weather_columns(&world, [0.5, 65.0, 0.5], 1.62, 0.0, false).is_empty());
    }

    #[test]
    fn full_rain_lays_the_square_box() {
        // A flat stone roof at y 63 under plains rain: every column of the
        // 11×11 box survives (biome defaults rain, open sky), each spanning
        // the viewer window clamped to the roof.
        let mut world = loaded_world(-1, 0, -1, 0);
        roof(&mut world, -8, 8, -8, 8, 63, 1);
        let columns = weather_columns(&world, [0.5, 70.0, 0.5], 1.62, 1.0, false);
        assert_eq!(columns.len(), 121, "the (2·5+1)² box, none skipped");
        let centre = columns
            .iter()
            .find(|column| column.x == 0 && column.z == 0)
            .expect("the viewer's own column");
        assert_eq!((centre.y0, centre.y1), (65, 75));
        assert!(!centre.snow, "the plains rain");
        assert!(
            (centre.alpha - 1.0).abs() < 1e-6,
            "the centre column keeps the full strength: {}",
            centre.alpha
        );
        let corner = columns
            .iter()
            .find(|column| column.x == 5 && column.z == 5)
            .expect("the box corner");
        assert!(
            (corner.alpha - 0.0).abs() < 1e-6,
            "the edge falloff reaches zero: {}",
            corner.alpha
        );
    }

    #[test]
    fn a_covered_column_draws_no_streak() {
        // The viewer under a roof at y 70: the clamp empties every column
        // whose precipitation sits above the window's top.
        let mut world = loaded_world(-1, 0, -1, 0);
        roof(&mut world, -8, 8, -8, 8, 70, 1);
        let columns = weather_columns(&world, [0.5, 65.0, 0.5], 1.62, 1.0, false);
        assert!(
            columns.is_empty(),
            "a roofed viewer sees no streaks: {}",
            columns.len()
        );
    }

    #[test]
    fn no_rain_means_no_splashes() {
        let world = column_world(63);
        let mut rng = StdRng::seed_from_u64(11);
        assert!(
            splash_spawns(
                &mut rng,
                &world,
                [0, 65, 0],
                0.0,
                false,
                ParticleSetting::All
            )
            .is_empty()
        );
    }

    #[test]
    fn an_open_column_spawns_drops_at_the_pinned_rate() {
        // A plains roof at y 63, the viewer at y 65: every surviving try
        // rains a WATER_DROP at the roof plus a tenth. The seeded rate pins
        // the count, and every spawn carries the falling id at the right
        // height.
        let mut world = loaded_world(-1, 0, -1, 0);
        roof(&mut world, -12, 12, -12, 12, 63, 1);
        // Chunk (0,0)'s biome array defaults to zero — ocean, which rains.
        let mut rng = StdRng::seed_from_u64(7);
        let spawns = splash_spawns(
            &mut rng,
            &world,
            [0, 65, 0],
            1.0,
            true,
            ParticleSetting::All,
        );
        assert_eq!(
            spawns.len(),
            100,
            "full rain at fancy tries the whole hundred over open ground"
        );
        for spawn in &spawns {
            assert_eq!(spawn.id, WATER_DROP_ID);
            assert!(
                (spawn.pos[1] - 64.1).abs() < 1e-4,
                "a tenth above the roof: {}",
                spawn.pos[1]
            );
        }
        // The drops tick against the roof plane: they fall and die on the
        // ground, feeding the snapshot the pass draws.
        let roof_plane = StillWorld {
            water: false,
            light: 0,
            ground: Some(64.0),
        };
        let mut engine = ParticleEngine::seeded(21, 4000);
        for spawn in &spawns {
            assert!(engine.spawn_local(*spawn));
        }
        assert_eq!(engine.stats().count, 100);
        for _ in 0..4 {
            engine.tick(&roof_plane);
        }
        let frames = engine.snapshot();
        assert!(
            frames.len() < 100,
            "the rain dies on the ground: {} left",
            frames.len()
        );
        assert!(
            frames
                .iter()
                .all(|frame| matches!(frame.tex, ParticleTex::Cell(_))),
            "the drops draw cells"
        );
    }

    #[test]
    fn a_covered_column_spawns_none() {
        // The viewer deep below the surface: every column's precipitation
        // sits outside the ±10 window, roof or none.
        let mut world = loaded_world(-1, 0, -1, 0);
        roof(&mut world, -12, 12, -12, 12, 63, 1);
        let mut rng = StdRng::seed_from_u64(7);
        let spawns = splash_spawns(
            &mut rng,
            &world,
            [0, 40, 0],
            1.0,
            true,
            ParticleSetting::All,
        );
        assert!(
            spawns.is_empty(),
            "a roofed column spawns nothing: {}",
            spawns.len()
        );
    }

    #[test]
    fn a_lava_floor_smokes() {
        // Lava (10/11) under an open column: the arm smokes instead of
        // raining.
        let mut world = loaded_world(-1, 0, -1, 0);
        roof(&mut world, -12, 12, -12, 12, 63, 11);
        let mut rng = StdRng::seed_from_u64(7);
        let spawns = splash_spawns(
            &mut rng,
            &world,
            [0, 65, 0],
            1.0,
            true,
            ParticleSetting::All,
        );
        assert_eq!(spawns.len(), 100);
        assert!(
            spawns.iter().all(|spawn| spawn.id == SPLASH_SMOKE_ID),
            "the lava arm smokes"
        );
    }

    #[test]
    fn minimal_kills_the_splashes_and_decreased_halves_the_tries() {
        let mut world = loaded_world(-1, 0, -1, 0);
        roof(&mut world, -12, 12, -12, 12, 63, 1);
        let mut rng = StdRng::seed_from_u64(7);
        assert!(
            splash_spawns(
                &mut rng,
                &world,
                [0, 65, 0],
                1.0,
                true,
                ParticleSetting::Minimal
            )
            .is_empty()
        );
        let mut rng = StdRng::seed_from_u64(7);
        let halved = splash_spawns(
            &mut rng,
            &world,
            [0, 65, 0],
            1.0,
            true,
            ParticleSetting::Decreased,
        );
        assert_eq!(halved.len(), 50, "the setting halves the hundred tries");
    }
}
