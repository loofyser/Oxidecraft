//! The sound index: synthetic fixtures, hostile refusals, and the ignored
//! store-gated read of the real `sounds.json`.
//!
//! The synthetic documents are built in-test; nothing here reads a fixture
//! file. The ignored case reads the user's own store (`OXIDECRAFT_STORE` must
//! name the store root) and fails naming the variable when it is unset, so a
//! run without a store can never pass silently.

use std::collections::BTreeMap;
use std::path::Path;

use oxide_assets::sound::{MAX_SOUND_EVENTS, MAX_SOUND_FILES, SoundIndex};

/// Two events: `dig.grass` with four plain files, `music.game` streamed with
/// two files at weights 1 and 2.
fn two_event_fixture() -> Vec<u8> {
    br#"{
        "dig.grass": {
            "category": "block",
            "sounds": ["dig/grass1", "dig/grass2", "dig/grass3", "dig/grass4"]
        },
        "music.game": {
            "category": "music",
            "sounds": [
                { "name": "music/game/calm1", "stream": true, "weight": 1 },
                { "name": "music/game/calm2", "stream": true, "weight": 2 }
            ]
        }
    }"#
    .to_vec()
}

#[test]
fn sound_two_events_parse_with_categories_and_files() {
    let index = SoundIndex::parse(&two_event_fixture()).expect("the fixture parses");
    assert_eq!(index.events.len(), 2);

    let dig = &index.events["dig.grass"];
    assert_eq!(dig.category, "block");
    assert_eq!(dig.stream, None);
    assert_eq!(dig.sounds.len(), 4);
    assert_eq!(dig.sounds[0].name, "dig/grass1");
    assert_eq!(dig.sounds[0].volume, 1.0);
    assert_eq!(dig.sounds[0].pitch, 1.0);
    assert_eq!(dig.sounds[0].weight, 1);

    let music = &index.events["music.game"];
    assert_eq!(music.category, "music");
    assert_eq!(music.sounds.len(), 2);
    assert!(music.sounds[0].stream == Some(true));
}

#[test]
fn sound_weighted_pick_follows_the_cumulative_rule() {
    let index = SoundIndex::parse(&two_event_fixture()).expect("the fixture parses");
    // Weights 1 and 2 over a total of 3: 0.32 * 3 = 0.96 lands in the first
    // file, 0.34 * 3 = 1.02 lands in the second.
    assert_eq!(
        index
            .pick("music.game", 0.32)
            .map(|sound| sound.name.as_str()),
        Some("music/game/calm1")
    );
    assert_eq!(
        index
            .pick("music.game", 0.34)
            .map(|sound| sound.name.as_str()),
        Some("music/game/calm2")
    );
    assert_eq!(
        index
            .pick("music.game", 0.0)
            .map(|sound| sound.name.as_str()),
        Some("music/game/calm1")
    );
    assert_eq!(index.pick("no.such.event", 0.5), None);
}

#[test]
fn sound_effective_stream_prefers_the_file_flag() {
    let index = SoundIndex::parse(&two_event_fixture()).expect("the fixture parses");
    let music = &index.events["music.game"];
    assert!(music.effective_stream(&music.sounds[0]));
    let dig = &index.events["dig.grass"];
    assert!(!dig.effective_stream(&dig.sounds[0]));
}

#[test]
fn sound_namespaced_path_resolves_to_the_store_relative_path() {
    let bytes = br#"{
        "dig.grass": {
            "category": "block",
            "sounds": ["minecraft:dig/grass1", "dig/grass2"]
        }
    }"#;
    let index = SoundIndex::parse(bytes).expect("the fixture parses");
    let dig = &index.events["dig.grass"];
    assert_eq!(dig.sounds[0].name, "dig/grass1");
    assert_eq!(dig.sounds[1].name, "dig/grass2");
}

#[test]
fn sound_non_positive_weights_clamp_to_one() {
    let bytes = br#"{
        "dig.grass": {
            "category": "block",
            "sounds": [
                { "name": "dig/grass1", "weight": 0 },
                { "name": "dig/grass2", "weight": -3 }
            ]
        }
    }"#;
    let index = SoundIndex::parse(bytes).expect("the fixture parses");
    let dig = &index.events["dig.grass"];
    assert_eq!(dig.sounds[0].weight, 1);
    assert_eq!(dig.sounds[1].weight, 1);
}

#[test]
fn sound_two_hundred_fifty_seven_files_are_refused() {
    let mut sounds = String::from("[");
    for n in 0..=MAX_SOUND_FILES {
        if n > 0 {
            sounds.push(',');
        }
        sounds.push_str(&format!("\"dig/grass{n}\""));
    }
    sounds.push(']');
    let document =
        format!("{{ \"dig.grass\": {{ \"category\": \"block\", \"sounds\": {sounds} }} }}");
    let error = SoundIndex::parse(document.as_bytes()).expect_err("257 files are refused");
    assert!(
        format!("{error}").contains(&MAX_SOUND_FILES.to_string()),
        "the refusal names the cap: {error}"
    );
}

#[test]
fn sound_event_name_past_the_cap_is_refused() {
    let name: String = "e".repeat(129);
    let document = format!("{{ {name:?}: {{ \"category\": \"block\", \"sounds\": [] }} }}");
    SoundIndex::parse(document.as_bytes()).expect_err("a 129-byte event name is refused");
}

#[test]
fn sound_malformed_json_is_refused_cleanly() {
    SoundIndex::parse(b"{ this is not json").expect_err("malformed JSON is refused");
    SoundIndex::parse(b"[1, 2, 3]").expect_err("a JSON array is refused");
}

/// The real `sounds.json` from the store: it parses to the surveyed 246
/// events, and the pinned names resolve with their flags.
///
/// The bytes live in the asset-index object `minecraft/sounds.json`; the
/// extracted overlay (`extracted/1.8.9/assets/`) holds no sounds, so the
/// object is the only copy. When the overlay carries a `sounds.json` it wins;
/// otherwise the object bytes are read through the `1.8` index.
///
/// Ignored by default because it needs the user's own store: `OXIDECRAFT_STORE`
/// must name the store root, and the test fails naming the variable when it
/// is unset, so a run without a store can never pass silently.
#[test]
#[ignore = "reads the real store; run it with OXIDECRAFT_STORE set and --ignored"]
fn the_real_sound_index_parses() {
    let store = std::env::var("OXIDECRAFT_STORE").expect(
        "OXIDECRAFT_STORE must name the store root that holds assets/indexes/1.8.json (for example \
         ~/.local/share/oxidecraft); this test does not pass without a store",
    );
    let root = Path::new(&store);

    let bytes = if root
        .join("extracted/1.8.9/assets/minecraft/sounds.json")
        .exists()
    {
        println!("sounds.json source: extracted overlay");
        std::fs::read(root.join("extracted/1.8.9/assets/minecraft/sounds.json"))
            .expect("the overlay sounds.json reads")
    } else {
        println!("sounds.json source: asset-index object minecraft/sounds.json");
        let index_text = std::fs::read_to_string(root.join("assets/indexes/1.8.json"))
            .expect("the 1.8 asset index reads");
        let index: serde_json::Value =
            serde_json::from_str(&index_text).expect("the 1.8 asset index parses");
        let object = &index["objects"]["minecraft/sounds.json"];
        let hash = object["hash"].as_str().expect("the object has a hash");
        let size = object["size"].as_u64().expect("the object has a size");
        assert_eq!(size, 37_731, "the surveyed object size");
        let object_path = root.join(format!("assets/objects/{}/{hash}", &hash[..2]));
        std::fs::read(&object_path).expect("the sounds.json object reads")
    };

    let index = SoundIndex::parse(&bytes).expect("the real sounds.json parses");
    println!("sound events parsed: {}", index.events.len());
    assert_eq!(index.events.len(), 246, "the surveyed event count");

    let mut by_category: BTreeMap<&str, usize> = BTreeMap::new();
    for event in index.events.values() {
        *by_category.entry(event.category.as_str()).or_default() += 1;
    }
    println!("events by category: {by_category:?}");
    assert_eq!(
        by_category,
        BTreeMap::from([
            ("ambient", 8),
            ("block", 31),
            ("hostile", 76),
            ("master", 1),
            ("music", 7),
            ("neutral", 86),
            ("player", 16),
            ("record", 19),
            ("weather", 2),
        ]),
        "the surveyed category counts"
    );

    let music = &index.events["music.game"];
    assert_eq!(music.category, "music");
    assert!(
        music
            .sounds
            .iter()
            .all(|sound| music.effective_stream(sound)),
        "every music.game file streams"
    );

    let rain = &index.events["ambient.weather.rain"];
    assert_eq!(rain.category, "weather");
    assert_eq!(rain.sounds.len(), 4);

    let record = &index.events["records.13"];
    assert_eq!(record.category, "record");
    assert_eq!(record.sounds.len(), 1);
    assert_eq!(record.sounds[0].name, "records/13");
    assert!(
        record.effective_stream(&record.sounds[0]),
        "records.13 streams"
    );

    let creative = &index.events["music.game.creative"];
    assert_eq!(
        creative.sounds.len(),
        7,
        "the real file still carries one redirect ahead of six files"
    );
    assert_eq!(
        creative.sounds[0].redirect.as_deref(),
        Some("music.game"),
        "the first sound of music.game.creative redirects at music.game"
    );
    assert_eq!(
        index.total_weight("music.game.creative"),
        18.0,
        "the redirect lends music.game's 12 over the six plain files"
    );
    let delegated = index
        .pick("music.game.creative", 0.0)
        .expect("a roll inside the lent span delegates");
    assert!(
        delegated.redirect.is_none(),
        "a delegated pick returns the target's own file, never the redirect"
    );
    assert!(
        music
            .sounds
            .iter()
            .any(|sound| sound.name == delegated.name),
        "the delegated file is one of music.game's own files"
    );

    assert!(MAX_SOUND_EVENTS >= index.events.len());
}
