//! The particle engine suite: a wire-shaped spawn drives the engine, ticks run
//! it to expiry, and the cap eviction order is pinned.
//!
//! The wire bodies below are hand-built 0x2A payloads in the
//! `oxide-proto-v47/tests/effects_wire.rs` style: big-endian id, the
//! long-distance byte, seven floats (position, spread, speed), the count,
//! then the varint data run.

use oxide_game::interaction::Face;
use oxide_game::local_particles::{
    BLOCK_CRACK_ID, BUBBLE_ID, CRIT_ID, CRIT_MAGIC_ID, CRIT_TRIES, DIG_DESTROY_SUBDIVISIONS,
    DIG_HIT_INSET, DIG_HIT_SCALE, FULL_CUBE_BOUNDS, SPLASH_ID, crit_burst, crit_id_for_animation,
    dig_destroy, dig_hit, dig_tint, sprint_spawn, water_spawns,
};
use oxide_game::particle_world::{ParticleWorld, StillWorld};
use oxide_game::particles::{ParticleEngine, ParticleSpawn, ParticleTex};
use oxide_proto_v47::effects_wire::decode_particle;
use oxide_world::behaviour::TintKind;

/// A dry, dark, groundless world: particles fall forever.
fn air() -> StillWorld {
    StillWorld {
        water: false,
        light: 0,
        ground: None,
    }
}

/// Feeds one decoded 0x2A body into the engine; answers particles added.
fn spawn_body(
    engine: &mut ParticleEngine,
    body: &[u8],
    viewer: [f32; 3],
    world: &dyn ParticleWorld,
) -> usize {
    let event = decode_particle(body).expect("wire body decodes");
    engine.spawn_wire(
        event.particle_id,
        event.long_distance,
        [event.x, event.y, event.z],
        [event.offset_x, event.offset_y, event.offset_z],
        event.speed,
        event.count,
        &event.data,
        viewer,
        world,
    )
}

/// Particle (0x2A), id 11 `SMOKE_NORMAL`: x = 1.5, y = 64.25, z = -3.5;
/// spread 0.5/0.25/0.125; speed 1.0; count 5; no data run.
const SMOKE_FIVE: &[u8] = &[
    0x00, 0x00, 0x00, 0x0b, // particle id 11
    0x00, // not long-distance
    0x3f, 0xc0, 0x00, 0x00, // x = 1.5
    0x42, 0x80, 0x80, 0x00, // y = 64.25
    0xc0, 0x60, 0x00, 0x00, // z = -3.5
    0x3f, 0x00, 0x00, 0x00, // offset x = 0.5
    0x3e, 0x80, 0x00, 0x00, // offset y = 0.25
    0x3e, 0x00, 0x00, 0x00, // offset z = 0.125
    0x3f, 0x80, 0x00, 0x00, // speed 1.0
    0x00, 0x00, 0x00, 0x05, // count 5
];

/// Particle (0x2A), id 36 `ITEM_CRACK`: count 2; data [267, 0] (the iron
/// sword with metadata 0).
const ITEM_CRACK_TWO: &[u8] = &[
    0x00, 0x00, 0x00, 0x24, // particle id 36
    0x00, // not long-distance
    0x00, 0x00, 0x00, 0x00, // x = 0.0
    0x41, 0x00, 0x00, 0x00, // y = 8.0
    0x00, 0x00, 0x00, 0x00, // z = 0.0
    0x3d, 0xcc, 0xcc, 0xcd, // offset x = 0.1
    0x3d, 0xcc, 0xcc, 0xcd, // offset y = 0.1
    0x3d, 0xcc, 0xcc, 0xcd, // offset z = 0.1
    0x3e, 0x80, 0x00, 0x00, // speed 0.25
    0x00, 0x00, 0x00, 0x02, // count 2
    0x8b, 0x02, // data 267 (varint)
    0x00, // data 0 (varint)
];

/// Particle (0x2A), id 37 `BLOCK_CRACK`: count 1; data [5].
const BLOCK_CRACK_ONE: &[u8] = &[
    0x00, 0x00, 0x00, 0x25, // particle id 37
    0x00, // not long-distance
    0x00, 0x00, 0x00, 0x00, // x = 0.0
    0x41, 0x00, 0x00, 0x00, // y = 8.0
    0x00, 0x00, 0x00, 0x00, // z = 0.0
    0x00, 0x00, 0x00, 0x00, // offset x = 0.0
    0x00, 0x00, 0x00, 0x00, // offset y = 0.0
    0x00, 0x00, 0x00, 0x00, // offset z = 0.0
    0x00, 0x00, 0x00, 0x00, // speed 0.0
    0x00, 0x00, 0x00, 0x01, // count 1
    0x05, // data [5]
];

/// Particle (0x2A), id 35 `BARRIER`: count 3.
const BARRIER_THREE: &[u8] = &[
    0x00, 0x00, 0x00, 0x23, // particle id 35
    0x00, // not long-distance
    0x00, 0x00, 0x00, 0x00, // x = 0.0
    0x41, 0x00, 0x00, 0x00, // y = 8.0
    0x00, 0x00, 0x00, 0x00, // z = 0.0
    0x00, 0x00, 0x00, 0x00, // offset x = 0.0
    0x00, 0x00, 0x00, 0x00, // offset y = 0.0
    0x00, 0x00, 0x00, 0x00, // offset z = 0.0
    0x00, 0x00, 0x00, 0x00, // speed 0.0
    0x00, 0x00, 0x00, 0x03, // count 3
];

#[test]
fn a_wire_spawn_emits_count_particles_and_ticks_run_them_to_expiry() {
    let world = air();
    let mut engine = ParticleEngine::seeded(11, 4000);
    let viewer = [1.5, 64.25, -3.5];
    let added = spawn_body(&mut engine, SMOKE_FIVE, viewer, &world);
    assert_eq!(added, 5, "count 5 emits five particles");
    assert_eq!(engine.stats().count, 5);
    assert_eq!(engine.snapshot().len(), 5);
    for _ in 0..400 {
        engine.tick(&world);
    }
    assert_eq!(engine.stats().count, 0, "every smoke ages out");
    assert!(engine.snapshot().is_empty());
}

#[test]
fn item_and_block_crack_data_land_as_their_tex_kinds() {
    let world = air();
    let mut engine = ParticleEngine::seeded(3, 4000);
    let viewer = [0.0, 8.0, 0.0];
    assert_eq!(spawn_body(&mut engine, ITEM_CRACK_TWO, viewer, &world), 2);
    assert_eq!(spawn_body(&mut engine, BLOCK_CRACK_ONE, viewer, &world), 1);
    let frames = engine.snapshot();
    assert_eq!(frames.len(), 3);
    assert!(
        frames[0..2]
            .iter()
            .all(|f| f.tex == ParticleTex::ItemAtlas(267, 0)),
        "item crack carries the item id and metadata: {:?}",
        frames.iter().map(|f| f.tex).collect::<Vec<_>>()
    );
    assert_eq!(
        frames[2].tex,
        ParticleTex::Atlas(5),
        "block crack carries the block id"
    );
}

#[test]
fn a_barrier_spawn_draws_nothing_but_counts() {
    let world = air();
    let mut engine = ParticleEngine::seeded(5, 4000);
    let added = spawn_body(&mut engine, BARRIER_THREE, [0.0, 8.0, 0.0], &world);
    assert_eq!(added, 3, "all three barrier spawns land");
    assert_eq!(engine.stats().count, 3, "barrier counts");
    assert_eq!(engine.stats().visible, 0, "barrier draws nothing");
    assert!(engine.snapshot().is_empty());
}

#[test]
fn an_unknown_wire_id_counts_as_barrier() {
    let world = air();
    let mut engine = ParticleEngine::seeded(5, 4000);
    let mut body = BARRIER_THREE.to_vec();
    body[3] = 99;
    let event = decode_particle(&body).expect("unknown id still decodes");
    assert_eq!(
        event.particle_id, 35,
        "the decoder maps unknown ids to barrier"
    );
    let added = engine.spawn_wire(
        event.particle_id,
        event.long_distance,
        [event.x, event.y, event.z],
        [event.offset_x, event.offset_y, event.offset_z],
        event.speed,
        event.count,
        &event.data,
        [0.0, 8.0, 0.0],
        &world,
    );
    assert_eq!(added, 3);
    assert_eq!(engine.stats().count, 3);
    assert!(engine.snapshot().is_empty());
}

#[test]
fn the_cap_evicts_oldest_first_and_leaves_other_lists_alone() {
    let mut engine = ParticleEngine::seeded(9, 2);
    for x in [0.0, 1.0, 2.0] {
        assert!(engine.spawn_local(ParticleSpawn {
            id: 11,
            pos: [x, 10.0, 0.0],
            vel: [0.0, 0.0, 0.0],
            colour: None,
            atlas_sprite: None,
            item: None,
            scale: 1.0,
        }));
    }
    assert!(engine.spawn_local(ParticleSpawn {
        id: 37,
        pos: [9.0, 10.0, 0.0],
        vel: [0.0, 0.0, 0.0],
        colour: None,
        atlas_sprite: Some(5),
        item: None,
        scale: 1.0,
    }));
    assert_eq!(engine.stats().count, 3, "two smokes plus the layer-1 dig");
    let mut xs: Vec<f32> = engine.snapshot().iter().map(|f| f.pos[0]).collect();
    xs.sort_by(f32::total_cmp);
    assert_eq!(
        xs,
        [1.0, 2.0, 9.0],
        "the oldest smoke evicted; the dig list untouched"
    );
}

/// Whether a value is within an epsilon of another; the tap positions are
/// `f32` arithmetic, so exact equality would pin rounding, not the rule.
fn close_to(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() < 1e-6
}

#[test]
fn a_dig_tick_spawns_one_inset_crack_at_the_struck_face() {
    // `EffectRenderer.addBlockHitEffects:395-442`: one `EntityDiggingFX`
    // (row 37) inside the block inset 0.1, the face axis pushed 0.1
    // outside along the struck face, zero velocity then `* 0.2`, the
    // `* 0.6` scale, the state id as the sprite and the 0.6-grey tint.
    assert_eq!(BLOCK_CRACK_ID, 37, "the hit particle is block crack");
    assert_eq!(DIG_HIT_INSET, 0.1, "the inset");
    assert_eq!(DIG_HIT_SCALE, 0.6, "the hit-only scale");
    assert_eq!(
        FULL_CUBE_BOUNDS,
        ([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]),
        "the bounds fallback: the source's own full-cube default"
    );
    let state = (5u16 << 4) | 3;
    let tint = [0.6, 0.5, 0.4];
    let cell = [3, 65, -7];
    for face in [
        Face::Down,
        Face::Up,
        Face::North,
        Face::South,
        Face::West,
        Face::East,
    ] {
        let mut engine = ParticleEngine::seeded(77, 4000);
        let spawn = dig_hit(&mut engine, cell, face, state, tint, FULL_CUBE_BOUNDS);
        assert_eq!(spawn.id, BLOCK_CRACK_ID, "{face:?}: the crack row");
        assert_eq!(spawn.vel, [0.0, 0.0, 0.0], "{face:?}: zero, then * 0.2");
        assert_eq!(
            spawn.atlas_sprite,
            Some(state),
            "{face:?}: the state id rides the sprite"
        );
        assert_eq!(spawn.colour, Some(tint), "{face:?}: the tint rides colour");
        assert_eq!(spawn.scale, DIG_HIT_SCALE, "{face:?}: the 0.6 scale");
        // The face axis sits 0.1 outside the cell; the other two axes sit
        // inside the 0.1 inset (`0.1 + rand * 0.8` over the full cube).
        let inset = |v: f32, c: i32| v >= c as f32 + 0.1 && v <= c as f32 + 0.9;
        match face {
            Face::Down => {
                assert!(close_to(spawn.pos[1], 65.0 - 0.1), "{face:?}: {spawn:?}");
                assert!(inset(spawn.pos[0], 3), "{face:?}: {spawn:?}");
                assert!(inset(spawn.pos[2], -7), "{face:?}: {spawn:?}");
            }
            Face::Up => {
                assert!(close_to(spawn.pos[1], 66.0 + 0.1), "{face:?}: {spawn:?}");
                assert!(inset(spawn.pos[0], 3), "{face:?}: {spawn:?}");
                assert!(inset(spawn.pos[2], -7), "{face:?}: {spawn:?}");
            }
            Face::North => {
                assert!(close_to(spawn.pos[2], -7.0 - 0.1), "{face:?}: {spawn:?}");
                assert!(inset(spawn.pos[0], 3), "{face:?}: {spawn:?}");
                assert!(inset(spawn.pos[1], 65), "{face:?}: {spawn:?}");
            }
            Face::South => {
                assert!(close_to(spawn.pos[2], -6.0 + 0.1), "{face:?}: {spawn:?}");
                assert!(inset(spawn.pos[0], 3), "{face:?}: {spawn:?}");
                assert!(inset(spawn.pos[1], 65), "{face:?}: {spawn:?}");
            }
            Face::West => {
                assert!(close_to(spawn.pos[0], 3.0 - 0.1), "{face:?}: {spawn:?}");
                assert!(inset(spawn.pos[1], 65), "{face:?}: {spawn:?}");
                assert!(inset(spawn.pos[2], -7), "{face:?}: {spawn:?}");
            }
            Face::East => {
                assert!(close_to(spawn.pos[0], 4.0 + 0.1), "{face:?}: {spawn:?}");
                assert!(inset(spawn.pos[1], 65), "{face:?}: {spawn:?}");
                assert!(inset(spawn.pos[2], -7), "{face:?}: {spawn:?}");
            }
        }
        assert!(engine.spawn_local(spawn), "{face:?}: the hit lands");
        let frames = engine.snapshot();
        assert_eq!(frames.len(), 1, "{face:?}: one particle");
        assert_eq!(
            frames[0].tex,
            ParticleTex::Atlas(state),
            "{face:?}: the block sprite"
        );
    }
}

#[test]
fn a_dig_destroy_fans_64_spores_over_a_non_air_cell() {
    // `EffectRenderer.addBlockDestroyEffects:369-393`: 4 x 4 x 4 spores at
    // `cell + (j/k/l + 0.5)/4`, velocity pos-minus-centre, non-air only.
    assert_eq!(DIG_DESTROY_SUBDIVISIONS, 4, "the per-axis split");
    let state = (1u16 << 4) | 2;
    let tint = [0.6, 0.5, 0.4];
    let spawns = dig_destroy([3, 65, -7], state, tint);
    assert_eq!(spawns.len(), 64, "the full 4 x 4 x 4 fan-out");
    let mut cells = Vec::new();
    for spawn in &spawns {
        assert_eq!(spawn.id, BLOCK_CRACK_ID, "the digging row");
        assert_eq!(spawn.atlas_sprite, Some(state), "the state id");
        assert_eq!(spawn.colour, Some(tint), "the tint");
        assert_eq!(spawn.scale, 1.0, "no hit scale on the destroy fan-out");
        let j = ((spawn.pos[0] - 3.0) * 4.0 - 0.5).round() as i32;
        let k = ((spawn.pos[1] - 65.0) * 4.0 - 0.5).round() as i32;
        let l = ((spawn.pos[2] + 7.0) * 4.0 - 0.5).round() as i32;
        assert!(
            (0..4).contains(&j) && (0..4).contains(&k) && (0..4).contains(&l),
            "on the sub-cell grid: {spawn:?}"
        );
        assert!(
            close_to(spawn.pos[0], 3.0 + (j as f32 + 0.5) / 4.0)
                && close_to(spawn.pos[1], 65.0 + (k as f32 + 0.5) / 4.0)
                && close_to(spawn.pos[2], -7.0 + (l as f32 + 0.5) / 4.0),
            "the (n + 0.5)/4 lattice: {spawn:?}"
        );
        assert!(
            close_to(spawn.vel[0], spawn.pos[0] - 3.5)
                && close_to(spawn.vel[1], spawn.pos[1] - 65.5)
                && close_to(spawn.vel[2], spawn.pos[2] + 6.5),
            "velocity is pos-minus-centre: {spawn:?}"
        );
        cells.push((j, k, l));
    }
    cells.sort();
    cells.dedup();
    assert_eq!(cells.len(), 64, "every sub-cell exactly once");
    assert!(
        dig_destroy([3, 65, -7], 0, tint).is_empty(),
        "air fans nothing"
    );
    let mut engine = ParticleEngine::seeded(5, 4000);
    for spawn in spawns {
        assert!(engine.spawn_local(spawn));
    }
    assert_eq!(engine.snapshot().len(), 64, "every spore lands");
}

#[test]
fn dig_tint_greys_grass_and_scales_the_multiplier() {
    // `EntityDiggingFX.setBlockPos`: grass keeps the 0.6 grey, every other
    // block multiplies it by `colorMultiplier` (white at the base).
    assert_eq!(
        dig_tint(2, TintKind::Grass, [0, 0, 0]),
        [0.6, 0.6, 0.6],
        "the grass exception"
    );
    assert_eq!(
        dig_tint(1, TintKind::None, [255, 255, 255]),
        [0.6, 0.6, 0.6],
        "an untinted block"
    );
    assert_eq!(
        dig_tint(18, TintKind::Foliage, [255, 0, 0]),
        [0.6, 0.0, 0.0],
        "a tinted block scales the multiplier"
    );
}

#[test]
fn a_sprint_tick_crumbs_one_crack_while_sprinting_and_dry() {
    // `Entity.createRunningParticles:1172-1188`: one `BLOCK_CRACK` per tick
    // while sprinting and dry, at `(posX +- rand*width/2, minY + 0.1,
    // posZ +- rand*width/2)`, velocity `(-motionX*4, 1.5, -motionZ*4)`,
    // data the stood-on cell's state id, render-type -1 gated.
    let feet = [10.0, 64.0, -3.0];
    let motion = [0.05, -0.1, 0.02];
    let state = 1u16 << 4;
    for (sprinting, in_water, renderable) in [
        (false, false, true),
        (true, true, true),
        (true, false, false),
    ] {
        let mut engine = ParticleEngine::seeded(21, 4000);
        assert!(
            sprint_spawn(
                &mut engine,
                feet,
                motion,
                0.6,
                state,
                renderable,
                sprinting,
                in_water
            )
            .is_none(),
            "gated out: sprinting {sprinting}, wet {in_water}, stood draws {renderable}"
        );
    }
    let mut engine = ParticleEngine::seeded(21, 4000);
    let spawn = sprint_spawn(&mut engine, feet, motion, 0.6, state, true, true, false)
        .expect("sprinting and dry crumbs once");
    assert_eq!(spawn.id, BLOCK_CRACK_ID, "the crack row");
    assert_eq!(spawn.atlas_sprite, Some(state), "the stood-on state id");
    assert!(
        (spawn.pos[0] - 10.0).abs() <= 0.3 && (spawn.pos[2] + 3.0).abs() <= 0.3,
        "within half the width: {spawn:?}"
    );
    assert!(
        close_to(spawn.pos[1], 64.0 + 0.1),
        "a tenth above the feet: {spawn:?}"
    );
    let velocity = [(-0.05 * 4.0) as f32, 1.5, (-0.02 * 4.0) as f32];
    assert!(
        close_to(spawn.vel[0], velocity[0])
            && close_to(spawn.vel[1], velocity[1])
            && close_to(spawn.vel[2], velocity[2]),
        "against the motion: {spawn:?}"
    );
    assert!(engine.spawn_local(spawn), "the crumb lands");
    assert_eq!(engine.snapshot().len(), 1, "one crumb per tick");
}

#[test]
fn a_water_entry_fans_bubbles_and_splash_by_width() {
    // `Entity.handleWaterMovement:1111-1129` with `resetHeight:1135-1159`:
    // `1 + width*20` each of bubble (vel `motionX, motionY - rand*0.2,
    // motionZ`) and splash (vel motion) at `y = floor(minY) + 1`, xz
    // scattered `+-width`.
    assert_eq!(BUBBLE_ID, 4, "the bubble row");
    assert_eq!(SPLASH_ID, 5, "the splash row");
    let feet = [8.0, 63.7, 8.0];
    let motion = [0.1, -0.4, 0.05];
    let mut engine = ParticleEngine::seeded(31, 4000);
    assert!(
        water_spawns(&mut engine, feet, motion, 0.6, false).is_empty(),
        "no edge, no fan-out"
    );
    let spawns = water_spawns(&mut engine, feet, motion, 0.6, true);
    let bubbles: Vec<_> = spawns
        .iter()
        .filter(|spawn| spawn.id == BUBBLE_ID)
        .collect();
    let splashes: Vec<_> = spawns
        .iter()
        .filter(|spawn| spawn.id == SPLASH_ID)
        .collect();
    assert_eq!(bubbles.len(), 13, "1 + 0.6*20 bubbles");
    assert_eq!(splashes.len(), 13, "1 + 0.6*20 splashes");
    for bubble in &bubbles {
        assert_eq!(bubble.pos[1], 64.0, "a block past the floored feet");
        assert!(
            (bubble.pos[0] - 8.0).abs() <= 0.6 && (bubble.pos[2] - 8.0).abs() <= 0.6,
            "scattered by the width: {bubble:?}"
        );
        assert_eq!(bubble.vel[0], 0.1_f32, "the motion rides x");
        assert_eq!(bubble.vel[2], 0.05_f32, "the motion rides z");
        assert!(
            bubble.vel[1] <= -0.4_f32 && bubble.vel[1] >= -0.6_f32,
            "motionY minus up to 0.2: {bubble:?}"
        );
    }
    for splash in &splashes {
        assert_eq!(splash.pos[1], 64.0, "a block past the floored feet");
        assert!(
            (splash.pos[0] - 8.0).abs() <= 0.6 && (splash.pos[2] - 8.0).abs() <= 0.6,
            "scattered by the width: {splash:?}"
        );
        assert_eq!(
            splash.vel,
            [0.1_f32, -0.4_f32, 0.05_f32],
            "the spawn rule carries the motion"
        );
    }
    let mut engine = ParticleEngine::seeded(31, 4000);
    for spawn in spawns {
        assert!(engine.spawn_local(spawn));
    }
    assert_eq!(engine.snapshot().len(), 26, "both fan-outs land");
}

#[test]
fn a_crit_runs_sixteen_tries_at_mid_height_for_either_animation() {
    // `EntityPlayerSP.onCriticalHit:671-674` and the S0B 4/5 arm
    // (`NetHandlerPlayClient:888-894`) share `EntityParticleEmitter`: 16
    // unit-sphere tries at the entity's mid-height, ids 9/10, velocity
    // `(d0, d1 + 0.2, d2)`.
    assert_eq!(CRIT_ID, 9, "the crit row");
    assert_eq!(CRIT_MAGIC_ID, 10, "the magic crit row");
    assert_eq!(CRIT_TRIES, 16, "the emitter's tries");
    assert_eq!(crit_id_for_animation(4), Some(CRIT_ID), "the crit arm");
    assert_eq!(
        crit_id_for_animation(5),
        Some(CRIT_MAGIC_ID),
        "the magic arm"
    );
    for animation in [0u8, 1, 2, 3, 6] {
        assert_eq!(
            crit_id_for_animation(animation),
            None,
            "animation {animation} is no crit"
        );
    }
    let mut engine = ParticleEngine::seeded(41, 4000);
    let (tries, spawns) = crit_burst(&mut engine, [10.5, 64.9, 2.5], 0.6, 1.8, CRIT_ID);
    assert_eq!(tries, 16, "every trigger runs sixteen tries");
    assert!(
        !spawns.is_empty() && spawns.len() <= 16,
        "the sphere keeps a share of the tries"
    );
    for spawn in &spawns {
        assert_eq!(spawn.id, CRIT_ID, "the crit row");
        assert!(
            (spawn.pos[0] - 10.5).abs() <= 0.15 + 1e-6
                && (spawn.pos[1] - 64.9).abs() <= 0.45 + 1e-6
                && (spawn.pos[2] - 2.5).abs() <= 0.15 + 1e-6,
            "inside the quarter-box: {spawn:?}"
        );
        assert!(
            spawn.vel.iter().all(|v| v.abs() <= 1.2),
            "the unit sphere plus the 0.2 lift: {spawn:?}"
        );
        assert!(
            spawn.colour.is_none() && spawn.atlas_sprite.is_none(),
            "no data run: {spawn:?}"
        );
    }
    let count = spawns.len();
    for spawn in spawns {
        assert!(engine.spawn_local(spawn));
    }
    assert_eq!(engine.snapshot().len(), count, "every kept try lands");
}
