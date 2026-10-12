//! The sound/particle/weather fixture corpus: one hand-built byte vector per
//! struct with catching values, the hostile set, and the index-by-index pin
//! of the particle table against `util/EnumParticleTypes.java` (ids 0-41).
//!
//! Every value is hand-picked to catch unit errors — a negative 1/8-block
//! sound triple with a fractional volume, a particle with moved floats and a
//! 1-arg data run at the count cap, rain begin with value 0, the fade pair,
//! a lightning spawn — and the hostile cases pin the caps: the string cap,
//! the truncated data run, the count cap, per-struct truncations, the kept
//! reason above 10, and the out-of-table particle id mapping to `BARRIER`.

use oxide_proto_v47::PacketError;
use oxide_proto_v47::effects_wire::{
    BARRIER_ID, ChangeGameState, MAX_PARTICLE_COUNT, PARTICLE_NAMES, Particle, SoundEffect,
    SpawnGlobalEntity, decode_change_game_state, decode_particle, decode_sound_effect,
    decode_spawn_global_entity, particle_argument_count, particle_name,
};

/// Sound Effect (0x29): name `mob.creeper.say`; x = -100 / 8 = -12.5,
/// y = 512 / 8 = 64.0, z = -48 / 8 = -6.0; volume 0.75; pitch 63 (full).
const SOUND_EFFECT: &[u8] = &[
    0x0f, // name length 15
    0x6d, 0x6f, 0x62, 0x2e, 0x63, 0x72, 0x65, 0x65, 0x70, 0x65, 0x72, 0x2e, 0x73, 0x61,
    0x79, // "mob.creeper.say"
    0xff, 0xff, 0xff, 0x9c, // x = -100
    0x00, 0x00, 0x02, 0x00, // y = 512
    0xff, 0xff, 0xff, 0xd0, // z = -48
    0x3f, 0x40, 0x00, 0x00, // volume 0.75
    0x3f, // pitch 63
];

/// Particle (0x2A), id 37 `BLOCK_CRACK`: long-distance; x = 1.5, y = 64.25,
/// z = -3.5; spread 0.5/0.25/0.125; speed 1.0; count 512 (the cap); data [5].
const PARTICLE_BLOCK_CRACK: &[u8] = &[
    0x00, 0x00, 0x00, 0x25, // particle id 37
    0x01, // long distance
    0x3f, 0xc0, 0x00, 0x00, // x = 1.5
    0x42, 0x80, 0x80, 0x00, // y = 64.25
    0xc0, 0x60, 0x00, 0x00, // z = -3.5
    0x3f, 0x00, 0x00, 0x00, // offset x = 0.5
    0x3e, 0x80, 0x00, 0x00, // offset y = 0.25
    0x3e, 0x00, 0x00, 0x00, // offset z = 0.125
    0x3f, 0x80, 0x00, 0x00, // speed 1.0
    0x00, 0x00, 0x02, 0x00, // count 512
    0x05, // data [5]
];

/// Change Game State (0x2B): reason 2 (begin raining), value 0.
const GAME_STATE_RAIN_BEGIN: &[u8] = &[0x02, 0x00, 0x00, 0x00, 0x00];

/// Change Game State (0x2B): reason 7 (fade value), value 1.0.
const GAME_STATE_FADE_VALUE: &[u8] = &[0x07, 0x3f, 0x80, 0x00, 0x00];

/// Change Game State (0x2B): reason 8 (fade time), value 60.0.
const GAME_STATE_FADE_TIME: &[u8] = &[0x08, 0x42, 0x70, 0x00, 0x00];

/// Spawn Global Entity (0x2C): lightning (kind 1), entity 42;
/// x = 800 / 32 = 25.0, y = 2048 / 32 = 64.0, z = -160 / 32 = -5.0.
const SPAWN_LIGHTNING: &[u8] = &[
    0x2a, // entity id 42
    0x01, // kind 1 = lightning
    0x00, 0x00, 0x03, 0x20, // x = 800
    0x00, 0x00, 0x08, 0x00, // y = 2048
    0xff, 0xff, 0xff, 0x60, // z = -160
];

#[test]
fn packet_ids_match_the_reference_rows() {
    assert_eq!(SoundEffect::ID, 0x29);
    assert_eq!(Particle::ID, 0x2a);
    assert_eq!(ChangeGameState::ID, 0x2b);
    assert_eq!(SpawnGlobalEntity::ID, 0x2c);
}

#[test]
fn sound_effect_decodes_every_field() {
    let sound = decode_sound_effect(SOUND_EFFECT).expect("sound decodes");
    assert_eq!(sound.name, "mob.creeper.say");
    assert_eq!(sound.x, -100);
    assert_eq!(sound.y, 512);
    assert_eq!(sound.z, -48);
    assert_eq!(sound.volume, 0.75);
    assert_eq!(sound.pitch, 63);
}

#[test]
fn particle_decodes_every_field() {
    let particle = decode_particle(PARTICLE_BLOCK_CRACK).expect("particle decodes");
    assert_eq!(particle.particle_id, 37);
    assert!(particle.long_distance);
    assert_eq!(particle.x, 1.5);
    assert_eq!(particle.y, 64.25);
    assert_eq!(particle.z, -3.5);
    assert_eq!(particle.offset_x, 0.5);
    assert_eq!(particle.offset_y, 0.25);
    assert_eq!(particle.offset_z, 0.125);
    assert_eq!(particle.speed, 1.0);
    assert_eq!(particle.count, MAX_PARTICLE_COUNT);
    assert_eq!(particle.data, vec![5]);
}

#[test]
fn game_state_decodes_rain_begin_and_the_fade_pair() {
    let rain = decode_change_game_state(GAME_STATE_RAIN_BEGIN).expect("rain decodes");
    assert_eq!(rain.reason, 2);
    assert_eq!(rain.value, 0.0);
    let fade = decode_change_game_state(GAME_STATE_FADE_VALUE).expect("fade value decodes");
    assert_eq!(fade.reason, 7);
    assert_eq!(fade.value, 1.0);
    let time = decode_change_game_state(GAME_STATE_FADE_TIME).expect("fade time decodes");
    assert_eq!(time.reason, 8);
    assert_eq!(time.value, 60.0);
}

#[test]
fn global_entity_decodes_the_lightning_spawn() {
    let spawn = decode_spawn_global_entity(SPAWN_LIGHTNING).expect("spawn decodes");
    assert_eq!(spawn.entity_id, 42);
    assert_eq!(spawn.kind, 1);
    assert_eq!(spawn.x, 800);
    assert_eq!(spawn.y, 2048);
    assert_eq!(spawn.z, -160);
}

#[test]
fn sound_effect_refuses_a_name_past_the_string_cap() {
    // A declared length of 32768 tops the crate's 32767-byte string cap; the
    // read refuses before touching any body bytes.
    let body = [0x80, 0x80, 0x02];
    assert!(decode_sound_effect(&body).is_err());
}

#[test]
fn particle_refuses_a_data_run_cut_inside_the_argument_count() {
    // Id 36 `ITEM_CRACK` reads 2 VarInts; the second is missing.
    let mut body = vec![
        0x00, 0x00, 0x00, 0x24, // particle id 36
        0x00, // short distance
        0x00, 0x00, 0x00, 0x00, // x
        0x00, 0x00, 0x00, 0x00, // y
        0x00, 0x00, 0x00, 0x00, // z
        0x00, 0x00, 0x00, 0x00, // offset x
        0x00, 0x00, 0x00, 0x00, // offset y
        0x00, 0x00, 0x00, 0x00, // offset z
        0x00, 0x00, 0x00, 0x00, // speed
        0x00, 0x00, 0x00, 0x01, // count 1
        0x05, // first data VarInt; the second never arrives
    ];
    assert!(decode_particle(&body).is_err());
    body.pop();
    body.pop();
    body.pop();
    assert!(decode_particle(&body).is_err());
}

#[test]
fn particle_refuses_a_count_past_the_cap() {
    // Id 0 reads no data VarInts; count 513 tops the 512 cap.
    let body = [
        0x00, 0x00, 0x00, 0x00, // particle id 0
        0x00, // short distance
        0x00, 0x00, 0x00, 0x00, // x
        0x00, 0x00, 0x00, 0x00, // y
        0x00, 0x00, 0x00, 0x00, // z
        0x00, 0x00, 0x00, 0x00, // offset x
        0x00, 0x00, 0x00, 0x00, // offset y
        0x00, 0x00, 0x00, 0x00, // offset z
        0x00, 0x00, 0x00, 0x00, // speed
        0x00, 0x00, 0x02, 0x01, // count 513
    ];
    assert!(decode_particle(&body).is_err());
    // A negative count is refused the same way.
    let mut negative = body.to_vec();
    negative[33] = 0xff;
    negative[34] = 0xff;
    negative[35] = 0xff;
    negative[36] = 0xff;
    assert!(decode_particle(&negative).is_err());
}

#[test]
fn effect_decodes_refuse_a_truncated_body() {
    assert!(decode_sound_effect(&[]).is_err());
    assert!(decode_particle(&[]).is_err());
    assert!(decode_change_game_state(&[]).is_err());
    assert!(decode_spawn_global_entity(&[]).is_err());
    assert!(decode_sound_effect(&SOUND_EFFECT[..SOUND_EFFECT.len() - 1]).is_err());
    assert!(decode_particle(&PARTICLE_BLOCK_CRACK[..PARTICLE_BLOCK_CRACK.len() - 1]).is_err());
    assert!(decode_change_game_state(&GAME_STATE_RAIN_BEGIN[..3]).is_err());
    assert!(decode_spawn_global_entity(&SPAWN_LIGHTNING[..SPAWN_LIGHTNING.len() - 1]).is_err());
}

#[test]
fn game_state_keeps_a_reason_above_ten() {
    // The source ignores unknown reasons; the decode keeps the byte.
    let state = decode_change_game_state(&[0x0b, 0x00, 0x00, 0x00, 0x00]).expect("reason kept");
    assert_eq!(state.reason, 11);
    assert_eq!(state.value, 0.0);
}

#[test]
fn particle_maps_an_unknown_id_to_barrier() {
    // Id 99 sits outside 0-41: the decode reports `BARRIER` (35) with its
    // empty data run, exactly as the source's fallback does.
    let body = [
        0x00, 0x00, 0x00, 0x63, // particle id 99
        0x00, // short distance
        0x00, 0x00, 0x00, 0x00, // x
        0x00, 0x00, 0x00, 0x00, // y
        0x00, 0x00, 0x00, 0x00, // z
        0x00, 0x00, 0x00, 0x00, // offset x
        0x00, 0x00, 0x00, 0x00, // offset y
        0x00, 0x00, 0x00, 0x00, // offset z
        0x00, 0x00, 0x00, 0x00, // speed
        0x00, 0x00, 0x00, 0x03, // count 3
    ];
    let particle = decode_particle(&body).expect("unknown id maps");
    assert_eq!(particle.particle_id, BARRIER_ID);
    assert_eq!(particle.particle_id, 35);
    assert_eq!(particle.count, 3);
    assert!(particle.data.is_empty());
}

#[test]
fn sound_effect_refuses_trailing_bytes() {
    let mut body = SOUND_EFFECT.to_vec();
    body.push(0x00);
    assert!(matches!(
        decode_sound_effect(&body),
        Err(PacketError::Trailing(1))
    ));
}

/// The 42 `EnumParticleTypes` rows, transcribed id-by-id from the source —
/// including the trailing underscores on rows 36-38.
const EXPECTED_PARTICLE_NAMES: [&str; 42] = [
    "explode",          // 0
    "largeexplode",     // 1
    "hugeexplosion",    // 2
    "fireworksSpark",   // 3
    "bubble",           // 4
    "splash",           // 5
    "wake",             // 6
    "suspended",        // 7
    "depthsuspend",     // 8
    "crit",             // 9
    "magicCrit",        // 10
    "smoke",            // 11
    "largesmoke",       // 12
    "spell",            // 13
    "instantSpell",     // 14
    "mobSpell",         // 15
    "mobSpellAmbient",  // 16
    "witchMagic",       // 17
    "dripWater",        // 18
    "dripLava",         // 19
    "angryVillager",    // 20
    "happyVillager",    // 21
    "townaura",         // 22
    "note",             // 23
    "portal",           // 24
    "enchantmenttable", // 25
    "flame",            // 26
    "lava",             // 27
    "footstep",         // 28
    "cloud",            // 29
    "reddust",          // 30
    "snowballpoof",     // 31
    "snowshovel",       // 32
    "slime",            // 33
    "heart",            // 34
    "barrier",          // 35
    "iconcrack_",       // 36
    "blockcrack_",      // 37
    "blockdust_",       // 38
    "droplet",          // 39
    "take",             // 40
    "mobappearance",    // 41
];

#[test]
fn particle_table_pins_all_forty_two_rows() {
    assert_eq!(PARTICLE_NAMES.len(), 42);
    for (id, expected) in EXPECTED_PARTICLE_NAMES.iter().enumerate() {
        assert_eq!(PARTICLE_NAMES[id], *expected, "row {id}");
        assert_eq!(particle_name(id as i32), Some(*expected), "lookup {id}");
    }
    assert_eq!(particle_name(42), None);
    assert_eq!(particle_name(-1), None);
}

#[test]
fn particle_argument_counts_match_the_source() {
    assert_eq!(particle_argument_count(36), 2);
    assert_eq!(particle_argument_count(37), 1);
    assert_eq!(particle_argument_count(38), 1);
    for id in 0..42i32 {
        if id != 36 && id != 37 && id != 38 {
            assert_eq!(particle_argument_count(id), 0, "id {id}");
        }
    }
    // Out-of-table ids report `BARRIER`'s (empty) count.
    assert_eq!(particle_argument_count(99), 0);
    assert_eq!(particle_argument_count(-7), 0);
}
