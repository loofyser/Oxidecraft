//! The audio engine suite: the silent-device mix, the clamps, the zero
//! rule, the attenuation curve and the streamed path stub.
//!
//! Every fixture is synthetic: a generated sine under a hand-built index.
//! Only the ignored case reads the user's own store (and tries the real
//! device), so the default run never touches the filesystem.

use std::sync::Arc;

use oxide_assets::sound::SoundIndex;
use oxide_game::audio::decode::DecodedSound;
use oxide_game::audio::sounds::{
    SoundGroup, hurt_pitch, level_volume, pickup_pitch, splash_volume, swim_pitch, swim_volume,
};
use oxide_game::audio::{AudioEngine, ListenerPose, PlaySound, SilentDevice, SoundCategory};

/// A one-second 440 Hz full-scale mono sine at 44100 Hz.
fn sine_440() -> DecodedSound {
    let samples: Vec<f32> = (0..44100)
        .map(|n| ((f64::from(n) * 440.0 * std::f64::consts::TAU) / 44100.0).sin() as f32)
        .collect();
    DecodedSound::from_samples(1, 44100, samples)
}

/// The hand-built index: `test.tone` (blocks) and `music.test` (streamed).
fn index() -> Arc<SoundIndex> {
    let bytes = br#"{
        "test.tone": { "category": "block", "sounds": ["test/tone"] },
        "music.test": {
            "category": "music",
            "sounds": [{ "name": "music/test", "stream": true }]
        }
    }"#;
    Arc::new(SoundIndex::parse(bytes).expect("the fixture parses"))
}

/// An engine over the silent device with the sine injected under both keys.
fn engine() -> AudioEngine {
    let device = Box::new(SilentDevice::open()) as Box<dyn oxide_game::audio::AudioDevice>;
    let mut engine = AudioEngine::new(index(), None, Some(device));
    engine.inject(&AudioEngine::cache_key("test/tone"), sine_440());
    engine.inject(&AudioEngine::cache_key("music/test"), sine_440());
    engine
}

/// The ears at the origin.
fn ears() -> ListenerPose {
    ListenerPose {
        pos: [0.0, 0.0, 0.0],
        yaw: 0.0,
    }
}

/// One unpositioned play of the test tone at full staging.
fn tone(volume: f32, pitch: f32) -> PlaySound {
    PlaySound {
        name: "test.tone".to_string(),
        category_hint: SoundCategory::Blocks,
        pos: None,
        volume,
        pitch,
        listener: [0.0, 0.0, 0.0],
        yaw: 0.0,
    }
}

/// Renders one second of stereo and answers its RMS.
fn render_rms(engine: &mut AudioEngine) -> f32 {
    let mut out = vec![0.0; 44100 * 2];
    engine.render_into(&mut out);
    let sum: f32 = out.iter().map(|sample| sample * sample).sum();
    (sum / out.len() as f32).sqrt()
}

/// Renders one second of stereo and answers its peak absolute sample in
/// the steady window (0.2 s to 0.8 s, past the attack ramp).
fn render_peak(engine: &mut AudioEngine) -> f32 {
    let mut out = vec![0.0; 44100 * 2];
    engine.render_into(&mut out);
    out[17640 * 2..35280 * 2]
        .iter()
        .fold(0.0f32, |loudest, sample| loudest.max(sample.abs()))
}

#[test]
fn the_category_volume_changes_the_mix() {
    let mut engine = engine();
    engine.play(tone(1.0, 1.0));
    let full = render_rms(&mut engine);
    assert!(full > 0.1, "the tone mixes non-silent: {full}");

    engine.set_category_volume(SoundCategory::Blocks, 0.25);
    engine.play(tone(1.0, 1.0));
    let quarter = render_rms(&mut engine);
    let ratio = quarter / full;
    assert!(
        (ratio - 0.25).abs() < 0.02,
        "a 0.25 category volume quarters the RMS: {quarter} vs {full}"
    );
}

#[test]
fn the_master_volume_changes_the_mix() {
    let mut engine = engine();
    engine.set_master_volume(0.5);
    engine.play(tone(1.0, 1.0));
    let half = render_rms(&mut engine);
    assert!(
        half > 0.05 && half < 0.5,
        "master 0.5 stages the mix: {half}"
    );
}

#[test]
fn a_zero_volume_skips_while_near_zero_plays() {
    let mut engine = engine();
    engine.play(tone(0.0, 1.0));
    engine.tick(ears());
    assert_eq!(engine.voices(), 0);
    assert_eq!(engine.stats().zero_skips, 1);

    engine.play(tone(0.005, 1.0));
    engine.tick(ears());
    assert_eq!(engine.voices(), 1, "0.005 plays at near-zero gain");
    let peak = render_peak(&mut engine);
    assert!(
        peak > 0.0 && peak < 0.01,
        "the near-zero play stays near zero: {peak}"
    );
}

#[test]
fn an_extreme_pitch_still_spawns() {
    let mut engine = engine();
    engine.play(tone(1.0, 10.0));
    engine.play(tone(1.0, 0.01));
    engine.tick(ears());
    assert_eq!(
        engine.voices(),
        2,
        "out-of-range pitches clamp, never refuse the play"
    );
}

#[test]
fn the_attenuation_curve_samples_in_the_mix() {
    let peak_at = |distance: f32| {
        let mut engine = engine();
        engine.play(PlaySound {
            pos: Some([distance, 0.0, 0.0]),
            ..tone(1.0, 1.0)
        });
        engine.tick(ears());
        render_peak(&mut engine)
    };
    let near = peak_at(1.0);
    let far = peak_at(15.0);
    let past = peak_at(17.0);
    assert!(near > 0.5, "1 block plays near full gain: {near}");
    assert!(far > 0.0, "15 blocks still plays: {far}");
    assert_eq!(past, 0.0, "17 blocks past the 16-block range is silent");
    let ratio = far / near;
    assert!(
        (ratio - 0.0625 / 0.9375).abs() < 0.01,
        "the mix follows the linear falloff: {far} vs {near}"
    );
}

#[test]
fn the_streamed_path_mixes_through_the_same_law() {
    let mut engine = engine();
    engine.play(PlaySound {
        name: "music.test".to_string(),
        category_hint: SoundCategory::Music,
        pos: None,
        volume: 1.0,
        pitch: 1.0,
        listener: [0.0, 0.0, 0.0],
        yaw: 0.0,
    });
    engine.tick(ears());
    assert_eq!(engine.voices(), 1);
    assert_eq!(engine.stats().streamed_plays, 1);
    let rms = render_rms(&mut engine);
    assert!(rms > 0.1, "the streamed voice mixes non-silent: {rms}");
}

#[test]
fn a_missing_name_plays_nothing_and_counts() {
    let mut engine = engine();
    engine.play(PlaySound {
        name: "no.such.sound".to_string(),
        ..tone(1.0, 1.0)
    });
    engine.play(PlaySound {
        name: "x".repeat(200),
        ..tone(1.0, 1.0)
    });
    engine.tick(ears());
    assert_eq!(engine.voices(), 0);
    assert_eq!(engine.stats().missing, 2);
}

#[test]
fn the_bounded_channel_never_blocks_a_stalled_consumer() {
    let mut engine = engine();
    for _ in 0..5000 {
        engine.play(tone(1.0, 1.0));
    }
    assert_eq!(
        engine.pending(),
        oxide_game::audio::COMMAND_CAP,
        "the channel caps and drops the oldest, never grows"
    );
    engine.tick(ears());
    assert_eq!(engine.pending(), 0);
}

#[test]
fn stop_all_silences() {
    let mut engine = engine();
    engine.play(tone(1.0, 1.0));
    engine.tick(ears());
    assert_eq!(engine.voices(), 1);
    engine.stop_all();
    let mut out = vec![0.0; 1024 * 2];
    engine.render_into(&mut out);
    assert!(out.iter().all(|sample| *sample == 0.0));
}

#[test]
fn the_dump_writes_a_stereo_wav() {
    let mut engine = engine();
    let dir = std::env::temp_dir().join("oxidecraft-audio-test");
    std::fs::create_dir_all(&dir).expect("the scratch dir creates");
    let path = dir.join("mix.wav");
    engine.debug_dump_start(&path);
    engine.play(tone(1.0, 1.0));
    engine.tick(ears());
    engine.render_into(&mut vec![0.0; 44100 * 2]);
    engine.debug_dump_stop();
    let bytes = std::fs::read(&path).expect("the dump reads");
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 2);
    assert_eq!(
        u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
        44100
    );
    let data = u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]) as usize;
    assert!(data > 0, "the dump carries frames");
    assert_eq!(
        bytes.len(),
        44 + data,
        "the file is exactly one header plus its frames"
    );
    let loudest = bytes[44..]
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]).abs())
        .max()
        .unwrap_or(0);
    assert!(loudest > 100, "the dumped mix is non-silent: {loudest}");
    std::fs::remove_file(&path).ok();
}

#[test]
fn the_category_order_and_names_pin() {
    let names: Vec<&str> = SoundCategory::ORDER
        .iter()
        .map(SoundCategory::name)
        .collect();
    assert_eq!(
        names,
        [
            "master", "music", "record", "weather", "block", "hostile", "neutral", "player",
            "ambient"
        ]
    );
    for (id, category) in SoundCategory::ORDER.iter().enumerate() {
        assert_eq!(category.as_index(), id);
    }
    assert_eq!(SoundCategory::from_name("hostile"), SoundCategory::Mobs);
    assert_eq!(SoundCategory::from_name("neutral"), SoundCategory::Animals);
    assert_eq!(SoundCategory::from_name("record"), SoundCategory::Records);
    assert_eq!(SoundCategory::from_name("bogus"), SoundCategory::Master);
}

/// The block group map's sample rows: each block's own `stepSound`
/// assignment reduced to its group and events.
#[test]
fn the_block_group_map_samples() {
    // (block id, group, break event, step event, place event)
    let rows = [
        (
            1u16,
            SoundGroup::Piston,
            "dig.stone",
            "step.stone",
            "dig.stone",
        ),
        (2, SoundGroup::Grass, "dig.grass", "step.grass", "dig.grass"),
        (
            3,
            SoundGroup::Gravel,
            "dig.gravel",
            "step.gravel",
            "dig.gravel",
        ),
        (5, SoundGroup::Wood, "dig.wood", "step.wood", "dig.wood"),
        (12, SoundGroup::Sand, "dig.sand", "step.sand", "dig.sand"),
        (
            20,
            SoundGroup::Glass,
            "dig.glass",
            "step.stone",
            "step.stone",
        ),
        (
            35,
            SoundGroup::Cloth,
            "dig.cloth",
            "step.cloth",
            "dig.cloth",
        ),
        (
            65,
            SoundGroup::Ladder,
            "dig.wood",
            "step.ladder",
            "dig.ladder",
        ),
        (78, SoundGroup::Snow, "dig.snow", "step.snow", "dig.snow"),
        (
            145,
            SoundGroup::Anvil,
            "dig.stone",
            "step.anvil",
            "random.anvil_land",
        ),
        // The leftovers the table never names fall back to stone.
        (8, SoundGroup::Stone, "dig.stone", "step.stone", "dig.stone"),
        (
            86,
            SoundGroup::Stone,
            "dig.stone",
            "step.stone",
            "dig.stone",
        ),
        (17, SoundGroup::Wood, "dig.wood", "step.wood", "dig.wood"),
        (
            18,
            SoundGroup::Grass,
            "dig.grass",
            "step.grass",
            "dig.grass",
        ),
        (53, SoundGroup::Wood, "dig.wood", "step.wood", "dig.wood"),
        (
            67,
            SoundGroup::Piston,
            "dig.stone",
            "step.stone",
            "dig.stone",
        ),
        (
            141,
            SoundGroup::Grass,
            "dig.grass",
            "step.grass",
            "dig.grass",
        ),
        (162, SoundGroup::Wood, "dig.wood", "step.wood", "dig.wood"),
    ];
    for (id, group, break_sound, step_sound, place_sound) in rows {
        assert_eq!(SoundGroup::of_block(id), group, "the group of block {id}");
        assert_eq!(group.break_sound(), break_sound, "the break of block {id}");
        assert_eq!(group.step_sound(), step_sound, "the step of block {id}");
        assert_eq!(group.place_sound(), place_sound, "the place of block {id}");
    }
    // The metal group stages the stone events a half-step up.
    assert_eq!(SoundGroup::Metal.break_sound(), "dig.stone");
    assert_eq!(SoundGroup::Metal.step_sound(), "step.stone");
    assert!((SoundGroup::Metal.frequency() - 1.5).abs() < f32::EPSILON);
    // The anvil group is quiet: 0.3 volume into the break staging.
    assert!((SoundGroup::Anvil.volume() - 0.3).abs() < f32::EPSILON);
    assert!((SoundGroup::Anvil.break_volume() - 0.65).abs() < 1e-6);
    // The break staging all other groups share: (volume + 1) / 2 at
    // frequency * 0.8, the step staging volume * 0.15 at frequency.
    let grass = SoundGroup::Grass;
    assert!((grass.break_volume() - 1.0).abs() < f32::EPSILON);
    assert!((grass.break_pitch() - 0.8).abs() < 1e-6);
    assert!((grass.step_volume() - 0.15).abs() < 1e-6);
    assert!((grass.frequency() - 1.0).abs() < f32::EPSILON);
}

/// The hurt randomisation range: `(first - second) * 0.2 + 1.0` over unit
/// draws stays inside `[0.8, 1.2]` and reaches both ends.
#[test]
fn the_hurt_pitch_range() {
    use rand::{RngExt, SeedableRng, rngs::StdRng};

    let mut rng = StdRng::seed_from_u64(9);
    let mut lowest = f32::MAX;
    let mut highest = f32::MIN;
    for _ in 0..1000 {
        let pitch = hurt_pitch(rng.random(), rng.random());
        assert!(
            (0.8..=1.2).contains(&pitch),
            "the hurt pitch stays in range: {pitch}"
        );
        lowest = lowest.min(pitch);
        highest = highest.max(pitch);
    }
    assert!(lowest < 0.9, "the range reaches its floor: {lowest}");
    assert!(highest > 1.1, "the range reaches its ceiling: {highest}");
    assert!((hurt_pitch(1.0, 0.0) - 1.2).abs() < 1e-6);
    assert!((hurt_pitch(0.0, 1.0) - 0.8).abs() < 1e-6);
}

/// The pickup randomisation range: `((first - second) * 0.7 + 1.0) * 2.0`
/// over unit draws stays inside `[0.6, 3.4]`.
#[test]
fn the_pickup_pitch_range() {
    use rand::{RngExt, SeedableRng, rngs::StdRng};

    let mut rng = StdRng::seed_from_u64(13);
    let mut lowest = f32::MAX;
    let mut highest = f32::MIN;
    for _ in 0..1000 {
        let pitch = pickup_pitch(rng.random(), rng.random());
        assert!(
            (0.6..=3.4).contains(&pitch),
            "the pickup pitch stays in range: {pitch}"
        );
        lowest = lowest.min(pitch);
        highest = highest.max(pitch);
    }
    assert!(lowest < 0.8, "the range reaches its floor: {lowest}");
    assert!(highest > 3.2, "the range reaches its ceiling: {highest}");
    assert!((pickup_pitch(1.0, 0.0) - 3.4).abs() < 1e-6);
}

/// The step's own pitch rule: no randomisation — the group's frequency plays
/// as-is — while the swim pitch spreads `1.0 +- 0.4`.
#[test]
fn the_step_pitch_rule() {
    for group in [
        SoundGroup::Stone,
        SoundGroup::Wood,
        SoundGroup::Metal,
        SoundGroup::Glass,
        SoundGroup::Sand,
        SoundGroup::Snow,
        SoundGroup::Ladder,
        SoundGroup::Anvil,
    ] {
        assert_eq!(
            group.break_pitch(),
            group.frequency() * 0.8,
            "the break pitch stages the frequency: {group:?}"
        );
    }
    assert!((swim_pitch(1.0, 0.0) - 1.4).abs() < 1e-6);
    assert!((swim_pitch(0.0, 1.0) - 0.6).abs() < 1e-6);
    // The splash volume caps the motion term at 1.0; still water is silent.
    assert!((splash_volume([0.0, 0.0, 0.0]) - 0.0).abs() < f32::EPSILON);
    assert!(splash_volume([10.0, 10.0, 10.0]) <= 1.0);
    assert!(swim_volume([10.0, 10.0, 10.0]) <= 1.0);
    // The level-up volume climbs to 0.75 at level 30 and holds.
    assert!((level_volume(30) - 0.75).abs() < 1e-6);
    assert!((level_volume(60) - 0.75).abs() < 1e-6);
    assert!((level_volume(15) - 0.375).abs() < 1e-6);
}

/// The store-gated leg: decodes one real store ogg through the
/// index/object path and tries the real device.
///
/// Ignored by default because it needs the user's own store:
/// `OXIDECRAFT_STORE` must name the store root, and the test fails naming
/// the variable when it is unset, so a run without a store can never pass
/// silently. A missing audio device is not a failure: the skip is recorded
/// and the silent fallback covers headless.
#[test]
#[ignore = "reads the real store; run it with OXIDECRAFT_STORE set and --ignored"]
fn the_store_ogg_decodes_and_the_device_opens() {
    use oxide_game::audio::decode::{decode_ogg, read_store_ogg};
    use oxide_game::audio::{AudioDevice, CpalDevice};

    let store = std::env::var("OXIDECRAFT_STORE").expect(
        "OXIDECRAFT_STORE must name the store root that holds assets/indexes/1.8.json (for example \
         ~/.local/share/oxidecraft); this test does not pass without a store",
    );
    let root = std::path::Path::new(&store);

    match CpalDevice::open() {
        Some(device) => println!(
            "audio device: opened {} at {} Hz x{}",
            device.device_name(),
            device.spec().sample_rate,
            device.spec().channels
        ),
        None => println!("audio device: skipped (no output device; the silent fallback covers it)"),
    }

    let bytes = read_store_ogg(root, "dig/grass1").expect("the store ogg reads");
    println!("dig/grass1 ogg bytes: {}", bytes.len());
    let decoded = decode_ogg(&bytes).expect("the store ogg decodes");
    println!(
        "dig/grass1 decoded: channels={} rate={} frames={} peak={}",
        decoded.channels,
        decoded.sample_rate,
        decoded.frames(),
        decoded.peak()
    );
    assert!(
        decoded.channels == 1 || decoded.channels == 2,
        "the store ogg maps to the mixer"
    );
    assert!(decoded.sample_rate >= 8000, "the store ogg carries a rate");
    assert!(decoded.peak() > 0.001, "the store ogg is non-silent");

    let bytes = br#"{
        "dig.grass": { "category": "block", "sounds": ["dig/grass1", "dig/grass2"] }
    }"#;
    let index = Arc::new(SoundIndex::parse(bytes).expect("the fixture parses"));
    let device = Box::new(SilentDevice::open()) as Box<dyn AudioDevice>;
    let mut engine = AudioEngine::new(index, Some(root.to_path_buf()), Some(device));
    engine.play(PlaySound {
        name: "dig.grass".to_string(),
        category_hint: SoundCategory::Blocks,
        pos: None,
        volume: 1.0,
        pitch: 1.0,
        listener: [0.0, 0.0, 0.0],
        yaw: 0.0,
    });
    engine.tick(ListenerPose {
        pos: [0.0, 0.0, 0.0],
        yaw: 0.0,
    });
    assert_eq!(
        engine.voices(),
        1,
        "the store file spawns through the engine"
    );
    let mut out = vec![0.0; 44100 * 2];
    engine.render_into(&mut out);
    let peak = out
        .iter()
        .fold(0.0f32, |loudest, sample| loudest.max(sample.abs()));
    println!("dig.grass engine mix peak: {peak}");
    assert!(
        peak > 0.0,
        "the store file mixes non-silent through the engine"
    );

    if let Ok(path) = std::env::var("OXIDECRAFT_AUDIO_DUMP") {
        engine.debug_dump_start(std::path::Path::new(&path));
        engine.play(PlaySound {
            name: "dig.grass".to_string(),
            category_hint: SoundCategory::Blocks,
            pos: None,
            volume: 1.0,
            pitch: 1.0,
            listener: [0.0, 0.0, 0.0],
            yaw: 0.0,
        });
        engine.tick(ListenerPose {
            pos: [0.0, 0.0, 0.0],
            yaw: 0.0,
        });
        engine.render_into(&mut vec![0.0; 44100 * 2]);
        engine.debug_dump_stop();
        println!("audio dump: {path}");
    }
}
