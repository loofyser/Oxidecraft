//! The 1.8 sound index: event name to its sound files.
//!
//! The store carries exactly one copy of `sounds.json`, as the asset-index
//! object `minecraft/sounds.json`; the extracted overlay holds no sounds.
//! The caller reads those bytes (the extracted path when present, else the
//! index object) and hands them to [`SoundIndex::parse`]: this type never
//! touches the filesystem.
//!
//! Each event states a `category` and a `sounds` list whose entries are
//! either a bare path or an object with a `name` and optional `volume`,
//! `pitch`, `weight`, `stream` and `attenuation_distance` keys. Any other
//! key is ignored: the store's copy carries one `{"type": "event", ...}`
//! entry (under `music.game.creative`), and future keys must not break the
//! read. The `type` value itself is not kept: the audio engine resolves the
//! kept `name` against loaded events.

use std::collections::BTreeMap;

use serde::Deserialize;

/// The most events one document may carry; more is refused.
pub const MAX_SOUND_EVENTS: usize = 4096;

/// The most sound files one event may carry; more is refused.
pub const MAX_SOUND_FILES: usize = 256;

/// The longest event name accepted, in bytes.
pub const MAX_EVENT_NAME_BYTES: usize = 128;

/// The longest sound file path accepted, in bytes.
pub const MAX_SOUND_NAME_BYTES: usize = 256;

/// The longest category name accepted, in bytes.
pub const MAX_CATEGORY_BYTES: usize = 32;

/// The `minecraft:` namespace the file may prefix a path with. Such a path
/// resolves to the store-relative path underneath: the paths the store's
/// copy carries are already store-relative, so the prefix only ever strips.
const MINECRAFT_NAMESPACE: &str = "minecraft:";

/// The parsed sound index: every event the document names.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SoundIndex {
    /// The events by name, in name order.
    pub events: BTreeMap<String, SoundDef>,
}

/// One sound event: its category, its stream flag, and its files.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundDef {
    /// The sound category, as the file states it (`music`, `weather`, ...).
    pub category: String,
    /// The event-level `stream` flag, when the file states one. The store's
    /// copy states none; streamed entries carry the flag per file instead.
    pub stream: Option<bool>,
    /// The files the event plays, in file order.
    pub sounds: Vec<SoundFile>,
}

/// One sound file within an event.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundFile {
    /// The store-relative path of the `.ogg`, without extension or
    /// namespace prefix.
    pub name: String,
    /// The volume multiplier, as the file states it, or 1.0 when it states
    /// none. Playback clamps this to `[0.0, 1.0]`; the parse keeps it.
    pub volume: f32,
    /// The pitch multiplier, as the file states it, or 1.0 when it states
    /// none. Playback clamps this to `[0.5, 2.0]`; the parse keeps it.
    pub pitch: f32,
    /// The selection weight: the file's `weight`, or 1 when the file states
    /// none. A stated weight below 1 clamps to 1, so the total is never
    /// zero and [`SoundIndex::pick`] always resolves.
    pub weight: i32,
    /// The file-level `stream` flag, when the file states one.
    pub stream: Option<bool>,
    /// The attenuation distance override, when the file states one.
    pub attenuation_distance: Option<i32>,
}

/// Errors from parsing a sound index.
#[derive(Debug, thiserror::Error)]
pub enum SoundError {
    /// The bytes are not a sound index document.
    #[error("invalid sound index JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// The document names more events than the cap allows.
    #[error("the sound index names {count} events; at most {MAX_SOUND_EVENTS} are accepted")]
    TooManyEvents {
        /// The events the document names.
        count: usize,
    },
    /// An event names more files than the cap allows.
    #[error("the event {event:?} names {count} sounds; at most {MAX_SOUND_FILES} are accepted")]
    TooManySounds {
        /// The event that names them.
        event: String,
        /// The files the event names.
        count: usize,
    },
    /// An event name runs past the string cap.
    #[error("the event name is {len} bytes; at most {MAX_EVENT_NAME_BYTES} are accepted")]
    EventNameTooLong {
        /// The name's length in bytes.
        len: usize,
    },
    /// A sound file path runs past the string cap.
    #[error("the sound path is {len} bytes; at most {MAX_SOUND_NAME_BYTES} are accepted")]
    SoundNameTooLong {
        /// The path's length in bytes.
        len: usize,
    },
    /// A category name runs past the string cap.
    #[error("the category name is {len} bytes; at most {MAX_CATEGORY_BYTES} are accepted")]
    CategoryTooLong {
        /// The name's length in bytes.
        len: usize,
    },
}

impl SoundIndex {
    /// Parses the bytes of a `sounds.json` document.
    ///
    /// Counts and string lengths are capped while the document is read: a
    /// document naming more than [`MAX_SOUND_EVENTS`] events, an event
    /// naming more than [`MAX_SOUND_FILES`] files, or a name past its byte
    /// cap is refused with a [`SoundError`], never truncated. A document
    /// that is not JSON, or not an object of events, is refused the same
    /// way. Non-positive weights clamp to 1; a leading `minecraft:`
    /// namespace strips to the store-relative path underneath.
    pub fn parse(bytes: &[u8]) -> Result<Self, SoundError> {
        let raw: BTreeMap<String, RawEvent> = serde_json::from_slice(bytes)?;
        if raw.len() > MAX_SOUND_EVENTS {
            return Err(SoundError::TooManyEvents { count: raw.len() });
        }
        let mut events = BTreeMap::new();
        for (name, event) in raw {
            if name.len() > MAX_EVENT_NAME_BYTES {
                return Err(SoundError::EventNameTooLong { len: name.len() });
            }
            if event.category.len() > MAX_CATEGORY_BYTES {
                return Err(SoundError::CategoryTooLong {
                    len: event.category.len(),
                });
            }
            if event.sounds.len() > MAX_SOUND_FILES {
                return Err(SoundError::TooManySounds {
                    event: name,
                    count: event.sounds.len(),
                });
            }
            let mut sounds = Vec::with_capacity(event.sounds.len());
            for raw_sound in event.sounds {
                sounds.push(SoundFile::from_raw(&raw_sound)?);
            }
            events.insert(
                name,
                SoundDef {
                    category: event.category,
                    stream: event.stream,
                    sounds,
                },
            );
        }
        Ok(Self { events })
    }

    /// Picks the file a `roll` selects, by cumulative weight.
    ///
    /// The rule is the file's own: the roll scales by the weights' total
    /// and the first file whose running total passes it wins, so weights
    /// 1 and 2 pick the first file at roll 0.32 (0.96 below 1) and the
    /// second at roll 0.34 (1.02 past 1). A roll at or past 1 resolves to
    /// the last file rather than nothing; an unknown event, or one with no
    /// files, resolves to nothing.
    #[must_use]
    pub fn pick(&self, name: &str, roll: f32) -> Option<&SoundFile> {
        let event = self.events.get(name)?;
        if event.sounds.is_empty() {
            return None;
        }
        let total: f64 = event
            .sounds
            .iter()
            .map(|sound| f64::from(sound.weight))
            .sum();
        let scaled = f64::from(roll) * total;
        let mut cumulative = 0.0;
        for sound in &event.sounds {
            cumulative += f64::from(sound.weight);
            if cumulative > scaled {
                return Some(sound);
            }
        }
        event.sounds.last()
    }
}

impl SoundDef {
    /// Whether a file of this event streams: the file's flag where it
    /// states one, else the event's, else false. The store's copy states
    /// `stream` on the file objects of `music.*` and `records.*` only, so
    /// those stream while everything else plays through the buffer.
    #[must_use]
    pub fn effective_stream(&self, sound: &SoundFile) -> bool {
        sound.stream.or(self.stream).unwrap_or(false)
    }
}

impl SoundFile {
    /// Converts one raw entry, enforcing the path cap and the weight clamp.
    fn from_raw(raw: &RawSound) -> Result<Self, SoundError> {
        let (name, volume, pitch, weight, stream, attenuation_distance) = match raw {
            RawSound::Path(path) => (path.clone(), 1.0, 1.0, 1, None, None),
            RawSound::Full {
                name,
                volume,
                pitch,
                weight,
                stream,
                attenuation_distance,
            } => (
                name.clone(),
                volume.unwrap_or(1.0),
                pitch.unwrap_or(1.0),
                weight.unwrap_or(1),
                *stream,
                *attenuation_distance,
            ),
        };
        let name = strip_namespace(&name);
        if name.len() > MAX_SOUND_NAME_BYTES {
            return Err(SoundError::SoundNameTooLong { len: name.len() });
        }
        Ok(Self {
            name,
            volume,
            pitch,
            weight: weight.max(1),
            stream,
            attenuation_distance,
        })
    }
}

/// One raw event, as the file writes it: unknown keys fail nothing, so a
/// future key cannot break the read.
#[derive(Deserialize)]
struct RawEvent {
    /// The sound category.
    category: String,
    /// The event-level stream flag, when the file states one.
    #[serde(default)]
    stream: Option<bool>,
    /// The files the event plays.
    sounds: Vec<RawSound>,
}

/// One raw sound entry: a bare path, or the object form.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawSound {
    /// The bare-path form.
    Path(String),
    /// The object form; any other key (the store's copy carries one
    /// `"type": "event"` entry) is ignored.
    Full {
        /// The sound path.
        name: String,
        /// The volume multiplier, when the file states one.
        #[serde(default)]
        volume: Option<f32>,
        /// The pitch multiplier, when the file states one.
        #[serde(default)]
        pitch: Option<f32>,
        /// The selection weight, when the file states one.
        #[serde(default)]
        weight: Option<i32>,
        /// The file-level stream flag, when the file states one.
        #[serde(default)]
        stream: Option<bool>,
        /// The attenuation distance override, when the file states one.
        #[serde(default)]
        attenuation_distance: Option<i32>,
    },
}

/// Strips one leading `minecraft:` namespace to the store-relative path
/// underneath; any other path returns untouched.
fn strip_namespace(name: &str) -> String {
    name.strip_prefix(MINECRAFT_NAMESPACE)
        .unwrap_or(name)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two-event shape the integration suite pins: plain files with
    /// defaults, and object files with flags.
    fn two_events() -> SoundIndex {
        let bytes = br#"{
            "dig.grass": {
                "category": "block",
                "sounds": ["dig/grass1", "dig/grass2"]
            },
            "music.game": {
                "category": "music",
                "stream": false,
                "sounds": [
                    { "name": "music/game/calm1", "stream": true, "weight": 2,
                      "volume": 0.5, "pitch": 1.5, "attenuation_distance": 32 }
                ]
            }
        }"#;
        SoundIndex::parse(bytes).expect("the fixture parses")
    }

    #[test]
    fn bare_paths_take_the_defaults() {
        let index = two_events();
        let first = &index.events["dig.grass"].sounds[0];
        assert_eq!(first.name, "dig/grass1");
        assert_eq!(first.volume, 1.0);
        assert_eq!(first.pitch, 1.0);
        assert_eq!(first.weight, 1);
        assert_eq!(first.stream, None);
        assert_eq!(first.attenuation_distance, None);
    }

    #[test]
    fn the_object_form_keeps_its_values() {
        let index = two_events();
        let event = &index.events["music.game"];
        assert_eq!(event.stream, Some(false));
        let file = &event.sounds[0];
        assert_eq!(file.volume, 0.5);
        assert_eq!(file.pitch, 1.5);
        assert_eq!(file.weight, 2);
        assert_eq!(file.stream, Some(true));
        assert_eq!(file.attenuation_distance, Some(32));
    }

    #[test]
    fn unknown_keys_do_not_break_the_read() {
        let bytes = br#"{
            "music.game.creative": {
                "category": "music",
                "sounds": [
                    { "type": "event", "name": "music.game" },
                    { "name": "music/game/creative/creative1", "stream": true }
                ]
            }
        }"#;
        let index = SoundIndex::parse(bytes).expect("the store's entry shape parses");
        let event = &index.events["music.game.creative"];
        assert_eq!(event.sounds.len(), 2);
        assert_eq!(event.sounds[0].name, "music.game");
    }

    #[test]
    fn the_event_flag_is_the_stream_fallback() {
        let bytes = br#"{
            "ui.click": {
                "category": "master",
                "stream": true,
                "sounds": ["random/click"]
            }
        }"#;
        let index = SoundIndex::parse(bytes).expect("the fixture parses");
        let event = &index.events["ui.click"];
        assert!(event.effective_stream(&event.sounds[0]));
    }

    #[test]
    fn pick_pins_the_weight_boundary() {
        let bytes = br#"{
            "music.game": {
                "category": "music",
                "sounds": [
                    { "name": "a", "weight": 1 },
                    { "name": "b", "weight": 2 }
                ]
            }
        }"#;
        let index = SoundIndex::parse(bytes).expect("the fixture parses");
        let name = |roll: f32| {
            index
                .pick("music.game", roll)
                .map(|sound| sound.name.as_str())
        };
        // The boundary sits at roll * 3 = 1: below it the first file wins,
        // on it and above the second does.
        assert_eq!(name(0.0), Some("a"));
        assert_eq!(name(0.32), Some("a"));
        assert_eq!(name(1.0 / 3.0), Some("b"));
        assert_eq!(name(0.34), Some("b"));
        assert_eq!(name(0.999), Some("b"));
        assert_eq!(name(1.0), Some("b"));
        assert_eq!(
            index
                .pick("music.game", 0.5)
                .map(|sound| sound.name.as_str()),
            Some("b")
        );
    }

    #[test]
    fn pick_on_an_empty_event_is_nothing() {
        let bytes = br#"{ "empty": { "category": "master", "sounds": [] } }"#;
        let index = SoundIndex::parse(bytes).expect("the fixture parses");
        assert_eq!(index.pick("empty", 0.5), None);
        assert_eq!(index.pick("missing", 0.5), None);
    }

    #[test]
    fn exactly_256_files_parse_and_257_do_not() {
        let mut sounds = String::from("[");
        for n in 0..MAX_SOUND_FILES {
            if n > 0 {
                sounds.push(',');
            }
            sounds.push_str(&format!("\"s{n}\""));
        }
        sounds.push(']');
        let document =
            format!("{{ \"ok\": {{ \"category\": \"master\", \"sounds\": {sounds} }} }}");
        let index = SoundIndex::parse(document.as_bytes()).expect("256 files parse");
        assert_eq!(index.events["ok"].sounds.len(), MAX_SOUND_FILES);

        let mut sounds = String::from("[");
        for n in 0..=MAX_SOUND_FILES {
            if n > 0 {
                sounds.push(',');
            }
            sounds.push_str(&format!("\"s{n}\""));
        }
        sounds.push(']');
        let document =
            format!("{{ \"full\": {{ \"category\": \"master\", \"sounds\": {sounds} }} }}");
        let error = SoundIndex::parse(document.as_bytes()).expect_err("257 files are refused");
        assert!(matches!(
            error,
            SoundError::TooManySounds { count: 257, .. }
        ));
    }

    #[test]
    fn the_event_cap_is_one_past_4096() {
        let mut document = String::from("{");
        for n in 0..=MAX_SOUND_EVENTS {
            if n > 0 {
                document.push(',');
            }
            document.push_str(&format!(
                "\"e{n}\": {{ \"category\": \"master\", \"sounds\": [] }}"
            ));
        }
        document.push('}');
        let error = SoundIndex::parse(document.as_bytes()).expect_err("4097 events are refused");
        assert!(matches!(error, SoundError::TooManyEvents { count: 4097 }));
    }

    #[test]
    fn overlong_strings_are_refused() {
        let long = "x".repeat(MAX_EVENT_NAME_BYTES + 1);
        let document = format!("{{ {long:?}: {{ \"category\": \"master\", \"sounds\": [] }} }}");
        assert!(matches!(
            SoundIndex::parse(document.as_bytes()),
            Err(SoundError::EventNameTooLong { .. })
        ));

        let long = "x".repeat(MAX_CATEGORY_BYTES + 1);
        let document = format!("{{ \"e\": {{ \"category\": {long:?}, \"sounds\": [] }} }}");
        assert!(matches!(
            SoundIndex::parse(document.as_bytes()),
            Err(SoundError::CategoryTooLong { .. })
        ));

        let long = "x".repeat(MAX_SOUND_NAME_BYTES + 1);
        let document =
            format!("{{ \"e\": {{ \"category\": \"master\", \"sounds\": [{long:?}] }} }}");
        assert!(matches!(
            SoundIndex::parse(document.as_bytes()),
            Err(SoundError::SoundNameTooLong { .. })
        ));
    }
}
