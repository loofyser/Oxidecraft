//! The particle engine suite: a wire-shaped spawn drives the engine, ticks run
//! it to expiry, and the cap eviction order is pinned.
//!
//! The wire bodies below are hand-built 0x2A payloads in the
//! `oxide-proto-v47/tests/effects_wire.rs` style: big-endian id, the
//! long-distance byte, seven floats (position, spread, speed), the count,
//! then the varint data run.

use oxide_game::particle_world::{ParticleWorld, StillWorld};
use oxide_game::particles::{ParticleEngine, ParticleSpawn, ParticleTex};
use oxide_proto_v47::effects_wire::decode_particle;

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
        }));
    }
    assert!(engine.spawn_local(ParticleSpawn {
        id: 37,
        pos: [9.0, 10.0, 0.0],
        vel: [0.0, 0.0, 0.0],
        colour: None,
        atlas_sprite: Some(5),
        item: None,
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
