//! The world's answers to the particle engine: the block material, light and
//! ground queries the simulation reads each tick.
//!
//! The engine takes a [`ParticleWorld`] trait so its stepping stays headless:
//! the [`StillWorld`] stub answers fixed values for the deterministic tests,
//! and [`SessionParticleWorld`] adapts the live chunk store for the session.
//! The adapter lives here — beside the trait — so the session files stay
//! untouched: the session hands the engine a borrow of its store each tick.
//!
//! The three queries mirror the only world reads the reference particle
//! classes perform:
//!
//! * the water-material death of the bubble and suspended classes
//!   (`EntityBubbleFX.java:38`, `EntitySuspendFX.java:34`: the block at the
//!   particle's own position reads other than `Material.water`),
//! * the sampled block light the lit pass draws with
//!   (`EntityFX.getBrightnessForRender`, carried here as one byte),
//! * the ground plane the motion rules rest on (`Entity.onGround`).

use oxide_world::behaviour::{CollisionShape, LiquidKind, behaviour};
use oxide_world::world::World;

/// What the particle simulation reads from the world.
///
/// Positions ride blocks as `f32`; the cell read floors each axis, the
/// reference's `new BlockPos(this)` convention.
pub trait ParticleWorld {
    /// Whether the block at `pos` is water.
    ///
    /// The bubble/suspended death rule reads exactly this
    /// (`EntityBubbleFX.java:38`).
    fn is_water(&self, pos: [f32; 3]) -> bool;

    /// The light level at `pos`, 0-15, for the snapshot's light byte.
    fn light_at(&self, pos: [f32; 3]) -> u8;

    /// The top solid surface under `(x, z)`, or `None` over the void.
    ///
    /// A particle at or below the surface rests on the ground that tick.
    /// The scan starts at the particle's own cell and walks down
    /// [`GROUND_SCAN_DEPTH`] cells; a ceiling above the particle never
    /// answers.
    fn ground_height(&self, pos: [f32; 3]) -> Option<f32>;
}

/// A headless world with fixed answers, for the deterministic tests.
///
/// One value per query: every cell is water or none is, every cell carries
/// the same light, and the ground is one flat plane (or the void).
pub struct StillWorld {
    /// What [`ParticleWorld::is_water`] answers everywhere.
    pub water: bool,
    /// What [`ParticleWorld::light_at`] answers everywhere (0-15).
    pub light: u8,
    /// What [`ParticleWorld::ground_height`] answers everywhere.
    pub ground: Option<f32>,
}

impl StillWorld {
    /// Dry, dark and groundless: particles fall forever unless a test says
    /// otherwise.
    pub fn air() -> Self {
        Self {
            water: false,
            light: 0,
            ground: None,
        }
    }
}

impl ParticleWorld for StillWorld {
    fn is_water(&self, _pos: [f32; 3]) -> bool {
        self.water
    }

    fn light_at(&self, _pos: [f32; 3]) -> u8 {
        self.light.min(15)
    }

    fn ground_height(&self, _pos: [f32; 3]) -> Option<f32> {
        self.ground
    }
}

/// The session-side adapter: the live chunk store as a [`ParticleWorld`].
///
/// The borrow covers one tick's worth of queries; cell reads go through
/// [`World::block`], so an unloaded column answers air (dry, dark, no
/// ground).
pub struct SessionParticleWorld<'a> {
    /// The chunk store the queries read.
    world: &'a World,
}

impl<'a> SessionParticleWorld<'a> {
    /// Borrows the session's store for one tick's worth of particle queries.
    pub fn new(world: &'a World) -> Self {
        Self { world }
    }
}

/// The deepest cell the ground scan walks below the particle before it
/// answers the void.
///
/// The reference sweeps an axis-aligned box against the real collision
/// shapes; this port reads a heightmap, so the scan only needs to cover a
/// particle's plausible fall per tick plus its resting depth.
const GROUND_SCAN_DEPTH: i32 = 32;

impl ParticleWorld for SessionParticleWorld<'_> {
    fn is_water(&self, pos: [f32; 3]) -> bool {
        let id = self.world.block(
            pos[0].floor() as i32,
            pos[1].floor() as i32,
            pos[2].floor() as i32,
        );
        behaviour(id).is_some_and(|row| row.liquid == Some(LiquidKind::Water))
    }

    fn light_at(&self, pos: [f32; 3]) -> u8 {
        let (x, y, z) = (
            pos[0].floor() as i32,
            pos[1].floor() as i32,
            pos[2].floor() as i32,
        );
        self.world
            .block_light(x, y, z)
            .max(self.world.sky_light(x, y, z))
            .min(15)
    }

    fn ground_height(&self, pos: [f32; 3]) -> Option<f32> {
        let (xi, zi) = (pos[0].floor() as i32, pos[2].floor() as i32);
        let mut yi = pos[1].floor() as i32 + 1;
        let floor = yi - GROUND_SCAN_DEPTH;
        while yi >= 0 && yi > floor {
            let id = self.world.block(xi, yi, zi);
            let solid =
                behaviour(id).is_some_and(|row| !matches!(row.collision, CollisionShape::None));
            if solid {
                return Some((yi + 1) as f32);
            }
            yi -= 1;
        }
        None
    }
}
