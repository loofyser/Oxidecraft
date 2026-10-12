//! The session-side local particle taps: the dig hit and destroy sites, the
//! sprint crumbs, the water-entry fan-outs and the crit emitter.
//!
//! Each builder derives its spawn rule from the reference source and returns
//! [`ParticleSpawn`]s the caller lands through [`ParticleEngine::spawn_local`]
//! (the weather's [`splash_spawns`] precedent). The scatter draws come from
//! the engine's own dice ([`ParticleEngine::next_unit`]), so a seeded engine
//! replays the same sites. Air and render-type gates stay with the builders
//! that own them; the block reads stay with the session.
//!
//! [`ParticleEngine::spawn_local`]: crate::particles::ParticleEngine::spawn_local
//! [`ParticleEngine::next_unit`]: crate::particles::ParticleEngine::next_unit
//! [`splash_spawns`]: crate::weather::splash_spawns

use oxide_world::behaviour::TintKind;

use crate::interaction::Face;
use crate::particles::{ParticleEngine, ParticleSpawn};

/// The block-crack row: the digging particle (`EntityDiggingFX.Factory`).
pub const BLOCK_CRACK_ID: u8 = 37;
/// The bubble row (`EntityBubbleFX`).
pub const BUBBLE_ID: u8 = 4;
/// The splash row (`EntitySplashFX`).
pub const SPLASH_ID: u8 = 5;
/// The crit row (`EntityCritFX`).
pub const CRIT_ID: u8 = 9;
/// The magic-crit row (`EntityCrit2FX`).
pub const CRIT_MAGIC_ID: u8 = 10;

/// The emitter's tries per trigger (`EntityParticleEmitter.onUpdate`'s
/// sixteen loop).
pub const CRIT_TRIES: u32 = 16;

/// The hit scatter's inset (`f = 0.1F`, `EffectRenderer.addBlockHitEffects`).
pub const DIG_HIT_INSET: f32 = 0.1;
/// The hit-only scale (`multipleParticleScaleBy(0.6F)`, same function).
pub const DIG_HIT_SCALE: f32 = 0.6;
/// The destroy fan-out's per-axis split (`i = 4`,
/// `EffectRenderer.addBlockDestroyEffects`).
pub const DIG_DESTROY_SUBDIVISIONS: u32 = 4;

/// The sprint crumb's lift above the feet (`minY + 0.1D`,
/// `Entity.createRunningParticles`).
pub const SPRINT_LIFT: f32 = 0.1;
/// The water fan-out's width factor (`1.0F + width * 20.0F`,
/// `Entity.resetHeight`).
pub const WATER_SPAWNS_PER_WIDTH: f32 = 20.0;

/// The digging grey: `particleRed/Green/Blue = 0.6F`
/// (`EntityDiggingFX`'s constructor).
pub const DIG_GREY: [f32; 3] = [0.6, 0.6, 0.6];

/// The grass block's id: the tint exception names `Blocks.grass`
/// (`EntityDiggingFX.setBlockPos`).
pub const GRASS_BLOCK_ID: u16 = 2;

/// The visual-bounds fallback, (min, max): the source builds every block at
/// the full cube (`Block.java:294`'s `setBlockBounds(0, 0, 0, 1, 1, 1)`), and
/// the behaviour table carries no per-block bounds, so the hit tap reads the
/// default for every block.
pub const FULL_CUBE_BOUNDS: ([f32; 3], [f32; 3]) = ([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);

/// One dig-hit particle: `EffectRenderer.addBlockHitEffects:395-442`.
///
/// The position sits inside the block inset [`DIG_HIT_INSET`] along the two
/// off-face axes (`inset + rand * (span - 2 * inset) + min`), and the face
/// axis is pushed the inset outside along the struck face. The velocity is
/// zero (`multiplyVelocity(0.2F)` keeps it there) and the scale is the
/// hit-only [`DIG_HIT_SCALE`]; render-type -1 skips the spawn, so the caller
/// gates it. Gravity is the block's own particle gravity — the one value the
/// source ever sets, the `1.0` default (`Block.java:290`), which row 37
/// already carries — and the tint rides `colour`.
pub fn dig_hit(
    engine: &mut ParticleEngine,
    cell: [i32; 3],
    face: Face,
    state: u16,
    tint: [f32; 3],
    bounds: ([f32; 3], [f32; 3]),
) -> ParticleSpawn {
    let (min, max) = bounds;
    let mut scatter = |axis: usize| {
        cell[axis] as f32
            + engine.next_unit() * (max[axis] - min[axis] - DIG_HIT_INSET * 2.0)
            + DIG_HIT_INSET
            + min[axis]
    };
    let mut pos = [scatter(0), scatter(1), scatter(2)];
    match face {
        Face::Down => pos[1] = cell[1] as f32 + min[1] - DIG_HIT_INSET,
        Face::Up => pos[1] = cell[1] as f32 + max[1] + DIG_HIT_INSET,
        Face::North => pos[2] = cell[2] as f32 + min[2] - DIG_HIT_INSET,
        Face::South => pos[2] = cell[2] as f32 + max[2] + DIG_HIT_INSET,
        Face::West => pos[0] = cell[0] as f32 + min[0] - DIG_HIT_INSET,
        Face::East => pos[0] = cell[0] as f32 + max[0] + DIG_HIT_INSET,
    }
    ParticleSpawn {
        id: BLOCK_CRACK_ID,
        pos,
        vel: [0.0, 0.0, 0.0],
        colour: Some(tint),
        atlas_sprite: Some(state),
        item: None,
        scale: DIG_HIT_SCALE,
    }
}

/// The dig-destroy fan-out: `EffectRenderer.addBlockDestroyEffects:369-393`.
///
/// `4 x 4 x 4` spores at `cell + (n + 0.5)/4` per axis, velocity
/// pos-minus-centre, the state id as the sprite and the tint as the colour.
/// Air fans nothing (the material gate); there is no render-type gate on
/// this path.
pub fn dig_destroy(cell: [i32; 3], value: u16, tint: [f32; 3]) -> Vec<ParticleSpawn> {
    if value >> 4 == 0 {
        return Vec::new();
    }
    let splits = DIG_DESTROY_SUBDIVISIONS as i32;
    let mut spawns = Vec::with_capacity(
        (DIG_DESTROY_SUBDIVISIONS * DIG_DESTROY_SUBDIVISIONS * DIG_DESTROY_SUBDIVISIONS) as usize,
    );
    for j in 0..splits {
        for k in 0..splits {
            for l in 0..splits {
                let pos = [
                    cell[0] as f32 + (j as f32 + 0.5) / splits as f32,
                    cell[1] as f32 + (k as f32 + 0.5) / splits as f32,
                    cell[2] as f32 + (l as f32 + 0.5) / splits as f32,
                ];
                spawns.push(ParticleSpawn {
                    id: BLOCK_CRACK_ID,
                    pos,
                    vel: [
                        pos[0] - (cell[0] as f32 + 0.5),
                        pos[1] - (cell[1] as f32 + 0.5),
                        pos[2] - (cell[2] as f32 + 0.5),
                    ],
                    colour: Some(tint),
                    atlas_sprite: Some(value),
                    item: None,
                    scale: 1.0,
                });
            }
        }
    }
    spawns
}

/// The digging tint: `EntityDiggingFX.setBlockPos`.
///
/// Grass keeps the constructor grey; every other block multiplies it by the
/// block's `colorMultiplier` at the position (white at the base), resolved
/// by the caller into the 0-255 `sample`.
pub fn dig_tint(block_id: u16, kind: TintKind, sample: [u8; 3]) -> [f32; 3] {
    if block_id == GRASS_BLOCK_ID || kind == TintKind::None {
        return DIG_GREY;
    }
    [
        DIG_GREY[0] * f32::from(sample[0]) / 255.0,
        DIG_GREY[1] * f32::from(sample[1]) / 255.0,
        DIG_GREY[2] * f32::from(sample[2]) / 255.0,
    ]
}

/// One sprint crumb: `Entity.createRunningParticles:1172-1188`.
///
/// One `BLOCK_CRACK` per tick while sprinting and dry, at `(posX +-
/// rand*width/2, minY + 0.1, posZ +- rand*width/2)` with velocity
/// `(-motionX*4, 1.5, -motionZ*4)` and the stood-on cell's state id as the
/// data. A stood-on block that draws nothing gates it out
/// (`getRenderType() != -1`).
#[allow(clippy::too_many_arguments)]
pub fn sprint_spawn(
    engine: &mut ParticleEngine,
    feet: [f64; 3],
    motion: [f64; 3],
    width: f64,
    state: u16,
    renderable: bool,
    sprinting: bool,
    in_water: bool,
) -> Option<ParticleSpawn> {
    if !sprinting || in_water || !renderable {
        return None;
    }
    Some(ParticleSpawn {
        id: BLOCK_CRACK_ID,
        pos: [
            (feet[0] + f64::from(engine.next_unit() - 0.5) * width) as f32,
            (feet[1] + f64::from(SPRINT_LIFT)) as f32,
            (feet[2] + f64::from(engine.next_unit() - 0.5) * width) as f32,
        ],
        vel: [(-motion[0] * 4.0) as f32, 1.5, (-motion[2] * 4.0) as f32],
        colour: None,
        atlas_sprite: Some(state),
        item: None,
        scale: 1.0,
    })
}

/// The water-entry fan-outs: `Entity.handleWaterMovement:1111-1129` with
/// `resetHeight:1135-1159`.
///
/// On the dry-to-wet edge, `1 + width*20` bubbles (velocity `motionX,
/// motionY - rand*0.2, motionZ`) followed by as many splashes (velocity the
/// motion), each at `y = floor(minY) + 1` with xz scattered `+-width`. No
/// edge fans nothing.
pub fn water_spawns(
    engine: &mut ParticleEngine,
    feet: [f64; 3],
    motion: [f64; 3],
    width: f64,
    entered: bool,
) -> Vec<ParticleSpawn> {
    if !entered {
        return Vec::new();
    }
    let count = (1.0 + width * WATER_SPAWNS_PER_WIDTH as f64).ceil() as usize;
    let surface = feet[1].floor() + 1.0;
    let scatter = |engine: &mut ParticleEngine| {
        [
            (feet[0] + f64::from(engine.next_unit() * 2.0 - 1.0) * width) as f32,
            surface as f32,
            (feet[2] + f64::from(engine.next_unit() * 2.0 - 1.0) * width) as f32,
        ]
    };
    let mut spawns = Vec::with_capacity(count * 2);
    for _ in 0..count {
        let pos = scatter(engine);
        let lift = f64::from(engine.next_unit() * 0.2);
        spawns.push(ParticleSpawn {
            id: BUBBLE_ID,
            pos,
            vel: [
                motion[0] as f32,
                (motion[1] - lift) as f32,
                motion[2] as f32,
            ],
            colour: None,
            atlas_sprite: None,
            item: None,
            scale: 1.0,
        });
    }
    for _ in 0..count {
        spawns.push(ParticleSpawn {
            id: SPLASH_ID,
            pos: scatter(engine),
            vel: [motion[0] as f32, motion[1] as f32, motion[2] as f32],
            colour: None,
            atlas_sprite: None,
            item: None,
            scale: 1.0,
        });
    }
    spawns
}

/// The crit arm: the S0B animation byte the emitter answers
/// (`NetHandlerPlayClient:888-894` — 4 is `CRIT`, 5 is `CRIT_MAGIC`).
/// Every other animation is no crit.
pub fn crit_id_for_animation(animation: u8) -> Option<u8> {
    match animation {
        4 => Some(CRIT_ID),
        5 => Some(CRIT_MAGIC_ID),
        _ => None,
    }
}

/// One crit-emitter round: `EntityParticleEmitter.onUpdate`.
///
/// Sixteen tries of `rand*2-1` per axis kept when the offsets land inside
/// the unit sphere, at the centre scattered by a quarter of the width and
/// height, with velocity `(d0, d1 + 0.2, d2)`. Answers the tries (always
/// sixteen) with the kept spawns. `centre` is the entity's mid-height
/// position.
pub fn crit_burst(
    engine: &mut ParticleEngine,
    centre: [f64; 3],
    width: f64,
    height: f64,
    id: u8,
) -> (u32, Vec<ParticleSpawn>) {
    let mut spawns = Vec::new();
    for _ in 0..CRIT_TRIES {
        let d = [
            f64::from(engine.next_unit() * 2.0 - 1.0),
            f64::from(engine.next_unit() * 2.0 - 1.0),
            f64::from(engine.next_unit() * 2.0 - 1.0),
        ];
        if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > 1.0 {
            continue;
        }
        spawns.push(ParticleSpawn {
            id,
            pos: [
                (centre[0] + d[0] * width / 4.0) as f32,
                (centre[1] + d[1] * height / 4.0) as f32,
                (centre[2] + d[2] * width / 4.0) as f32,
            ],
            vel: [d[0] as f32, (d[1] + 0.2) as f32, d[2] as f32],
            colour: None,
            atlas_sprite: None,
            item: None,
            scale: 1.0,
        });
    }
    (CRIT_TRIES, spawns)
}
