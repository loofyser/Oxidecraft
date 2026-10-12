//! The audio engine suite: the silent-device mix, the clamps, the zero
//! rule, the attenuation curve and the streamed path stub.
//!
//! Every fixture is synthetic: a generated sine under a hand-built index.
//! Only the ignored case reads the user's own store (and tries the real
//! device), so the default run never touches the filesystem.

use std::sync::Arc;

use oxide_assets::sound::SoundIndex;
use oxide_game::audio::decode::DecodedSound;
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
