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
//! key is ignored, except `type`: an object entry with `"type": "event"`
//! is a redirect to another event (the store's copy carries one, as the
//! first sound of `music.game.creative`, naming `music.game`), kept as a
//! [`SoundFile`] with [`SoundFile::redirect`] set. A redirect lends its
//! weight from its target's own total at pick time and a winning redirect
//! delegates to the target's own weighted pick, so the odds match the
//! source's two-stage draw; a missing target lends zero and is never
//! picked. An absent `category` defaults to `master`, as the source's
//! deserialiser does.

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
///
/// A file is either a playable entry (its `name` resolves to the
/// store-relative path of the `.ogg`, without extension or namespace
/// prefix) or a redirect: an entry the file wrote with `"type": "event"`,
/// whose `name` is the target event and whose weight is lent from the
/// target's own total at pick time (see [`SoundIndex::pick`]).
#[derive(Debug, Clone, PartialEq)]
pub struct SoundFile {
    /// The store-relative path of the `.ogg`, without extension or
    /// namespace prefix — or, for a redirect, the target event's name.
    pub name: String,
    /// The volume multiplier, as the file states it, or 1.0 when it states
    /// none. Playback clamps this to `[0.0, 1.0]`; the parse keeps it.
    pub volume: f32,
    /// The pitch multiplier, as the file states it, or 1.0 when it states
    /// none. Playback clamps this to `[0.5, 2.0]`; the parse keeps it.
    pub pitch: f32,
    /// The selection weight: the file's `weight`, or 1 when the file states
    /// none. A stated weight below 1 clamps to 1, so a plain file's total
    /// is never zero. A redirect ignores this field at pick time and lends
    /// its target's total instead (zero when the target is missing).
    pub weight: i32,
    /// The file-level `stream` flag, when the file states one.
    pub stream: Option<bool>,
    /// The attenuation distance override, when the file states one.
    pub attenuation_distance: Option<i32>,
    /// The redirect target: `Some` event name when the file wrote
    /// `"type": "event"`, else `None`. A delegated pick returns the
    /// target's own file with the target's own flags.
    pub redirect: Option<String>,
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
    ///
    /// A redirect entry (see [`SoundFile::redirect`]) lends its weight from
    /// its target's own total — zero when the target is missing, so a
    /// missing target is never picked — and a winning redirect delegates
    /// to the target's own weighted pick, returning the target's own file
    /// with the target's own flags. The second draw reuses the roll's
    /// position inside the redirect's span, so one roll stays
    /// deterministic. Redirect cycles resolve to nothing, never a loop.
    #[must_use]
    pub fn pick(&self, name: &str, roll: f32) -> Option<&SoundFile> {
        let mut current = name.to_string();
        let mut roll = roll;
        let mut visited = vec![current.clone()];
        loop {
            let event = self.events.get(current.as_str())?;
            let total: f64 = event
                .sounds
                .iter()
                .map(|sound| self.effective_weight(&visited, sound))
                .sum();
            if total <= 0.0 {
                return None;
            }
            let scaled = f64::from(roll) * total;
            let mut cursor = 0.0;
            let mut span_start = 0.0;
            let mut span_width = 0.0;
            let mut winner: Option<&SoundFile> = None;
            for sound in &event.sounds {
                let weight = self.effective_weight(&visited, sound);
                span_start = cursor;
                span_width = weight;
                cursor += weight;
                if cursor > scaled {
                    winner = Some(sound);
                    break;
                }
            }
            let won = winner.or(event.sounds.last())?;
            match won.redirect.as_deref() {
                None => return Some(won),
                Some(target) => {
                    if visited.iter().any(|seen| seen == target)
                        || !self.events.contains_key(target)
                    {
                        return None;
                    }
                    let sub = if span_width > 0.0 {
                        ((scaled - span_start) / span_width).clamp(0.0, 0.999_999_9)
                    } else {
                        0.0
                    };
                    visited.push(target.to_string());
                    current = target.to_string();
                    roll = sub as f32;
                }
            }
        }
    }

    /// The weights' total a [`SoundIndex::pick`] scales by: plain files
    /// count their clamped weight, redirects count their target's own
    /// total (zero when the target is missing). Unknown events total zero.
    #[must_use]
    pub fn total_weight(&self, name: &str) -> f64 {
        let mut visited = vec![name.to_string()];
        self.total_weight_in(&mut visited, name)
    }

    /// Sums one event's effective weights under the visited chain, so a
    /// redirect cycle lends zero instead of recursing forever.
    fn total_weight_in(&self, visited: &mut [String], name: &str) -> f64 {
        match self.events.get(name) {
            None => 0.0,
            Some(event) => event
                .sounds
                .iter()
                .map(|sound| self.effective_weight(visited, sound))
                .sum(),
        }
    }

    /// One entry's weight at pick time: the clamped weight for a plain
    /// file, the target's own total for a redirect (zero when the target
    /// is missing or already on the chain).
    fn effective_weight(&self, visited: &[String], sound: &SoundFile) -> f64 {
        match sound.redirect.as_deref() {
            None => f64::from(sound.weight),
            Some(target) => {
                if visited.iter().any(|seen| seen == target) {
                    0.0
                } else {
                    let mut chain = visited.to_vec();
                    chain.push(target.to_string());
                    self.total_weight_in(&mut chain, target)
                }
            }
        }
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
    ///
    /// An object entry with `"type": "event"` becomes a redirect: its name
    /// is the target event and its weight is lent at pick time, so the
    /// stored weight is kept but unused. Any other `type` value reads as a
    /// plain file; the parse never fails on it.
    fn from_raw(raw: &RawSound) -> Result<Self, SoundError> {
        let (name, volume, pitch, weight, stream, attenuation_distance, redirect) = match raw {
            RawSound::Path(path) => (path.clone(), 1.0, 1.0, 1, None, None, None),
            RawSound::Full {
                name,
                volume,
                pitch,
                weight,
                stream,
                attenuation_distance,
                sound_type,
            } => (
                name.clone(),
                volume.unwrap_or(1.0),
                pitch.unwrap_or(1.0),
                weight.unwrap_or(1),
                *stream,
                *attenuation_distance,
                sound_type
                    .as_deref()
                    .and_then(|kind| (kind == "event").then(|| strip_namespace(name))),
            ),
        };
        let name = strip_namespace(&name);
        if name.len() > MAX_SOUND_NAME_BYTES {
            return Err(SoundError::SoundNameTooLong { len: name.len() });
        }
        if let Some(target) = redirect.as_deref() {
            if target.len() > MAX_SOUND_NAME_BYTES {
                return Err(SoundError::SoundNameTooLong { len: target.len() });
            }
        }
        Ok(Self {
            name,
            volume,
            pitch,
            weight: weight.max(1),
            stream,
            attenuation_distance,
            redirect,
        })
    }
}

/// One raw event, as the file writes it: unknown keys fail nothing, so a
/// future key cannot break the read.
///
/// An absent `category` defaults to `master`, as the source's deserialiser
/// does (`SoundListSerializer.java:20` falls back to
/// `SoundCategory.MASTER.getCategoryName()`, which is `"master"`
/// (`SoundCategory.java:8`)).
#[derive(Deserialize)]
struct RawEvent {
    /// The sound category; `master` when the file states none.
    #[serde(default = "default_category")]
    category: String,
    /// The event-level stream flag, when the file states one.
    #[serde(default)]
    stream: Option<bool>,
    /// The files the event plays.
    sounds: Vec<RawSound>,
}

/// The category an event without one plays under.
fn default_category() -> String {
    "master".to_string()
}

/// One raw sound entry: a bare path, or the object form.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawSound {
    /// The bare-path form.
    Path(String),
    /// The object form; any other key is ignored, but `type` is kept: an
    /// entry with `"type": "event"` names another event and becomes a
    /// redirect (see [`SoundFile::redirect`]).
    Full {
        /// The sound path — or the target event, for a redirect.
        name: String,
        /// The entry kind (`"file"` or `"event"`), when the file states one.
        #[serde(rename = "type", default)]
        sound_type: Option<String>,
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
        assert_eq!(
            event.sounds[0].redirect.as_deref(),
            Some("music.game"),
            "the event entry is kept as a redirect"
        );
        assert_eq!(event.sounds[1].redirect, None);
    }

    #[test]
    fn a_missing_category_defaults_to_master() {
        let bytes = br#"{ "lonely": { "sounds": ["random/click"] } }"#;
        let index = SoundIndex::parse(bytes).expect("a category-less event parses");
        assert_eq!(index.events["lonely"].category, "master");
    }

    /// The redirect shape: one redirect at `music.game` (twelve files,
    /// weight 1 each) ahead of six plain files.
    fn redirect_index() -> SoundIndex {
        let mut document = String::from(
            "{\"music.game.creative\": {\"category\": \"music\", \"sounds\": [{\"type\": \"event\", \"name\": \"music.game\"},",
        );
        for n in 1..=6 {
            document.push_str(&format!(
                "{{\"name\": \"music/game/creative/creative{n}\", \"stream\": true}},"
            ));
        }
        document.pop();
        document.push_str("]}, \"music.game\": {\"category\": \"music\", \"sounds\": [");
        for n in 1..=12 {
            document.push_str(&format!("\"music/game/calm{n}\","));
        }
        document.pop();
        document.push_str("]}}");
        SoundIndex::parse(document.as_bytes()).expect("the redirect fixture parses")
    }

    #[test]
    fn the_redirect_lends_the_target_total() {
        let index = redirect_index();
        assert_eq!(index.total_weight("music.game"), 12.0);
        assert_eq!(
            index.total_weight("music.game.creative"),
            18.0,
            "the redirect lends 12 and the six files add 6"
        );
    }

    #[test]
    fn the_redirect_span_boundary_pins() {
        let index = redirect_index();
        let name = |roll: f32| {
            index
                .pick("music.game.creative", roll)
                .map(|sound| sound.name.clone())
        };
        // The redirect spans the leading 12/18: a roll below 12/18 lands
        // inside it and delegates into the target's files, a roll above
        // lands on the plain files.
        let first = name(0.0).expect("roll 0 resolves");
        assert!(
            first.starts_with("music/game/calm"),
            "a redirect win resolves into the target's files, not the redirect: {first}"
        );
        let last = name(0.999).expect("a late roll resolves");
        assert!(
            last.starts_with("music/game/creative/creative"),
            "a roll past the span lands on the plain files: {last}"
        );
        let edge = name(12.0 / 18.0).expect("the boundary resolves");
        assert!(
            edge.starts_with("music/game/creative/creative"),
            "at 12/18 the redirect's span is spent: {edge}"
        );
    }

    #[test]
    fn a_redirect_win_keeps_the_target_flags() {
        let bytes = br#"{
            "target": {
                "category": "music",
                "sounds": [{ "name": "music/game/calm1", "stream": true, "volume": 0.5 }]
            },
            "holder": {
                "category": "master",
                "sounds": [{ "type": "event", "name": "target" }]
            }
        }"#;
        let index = SoundIndex::parse(bytes).expect("the fixture parses");
        assert_eq!(index.total_weight("holder"), 1.0);
        let file = index.pick("holder", 0.0).expect("the lent span resolves");
        assert_eq!(file.name, "music/game/calm1");
        assert_eq!(file.stream, Some(true));
        assert_eq!(file.volume, 0.5);
    }

    #[test]
    fn a_missing_target_lends_zero_and_is_never_picked() {
        let bytes = br#"{
            "holder": {
                "category": "master",
                "sounds": [
                    { "type": "event", "name": "gone" },
                    "random/click"
                ]
            }
        }"#;
        let index = SoundIndex::parse(bytes).expect("the fixture parses");
        assert_eq!(
            index.total_weight("holder"),
            1.0,
            "the missing target lends zero"
        );
        for roll in [0.0, 0.25, 0.5, 0.75, 0.999] {
            let file = index.pick("holder", roll).expect("a roll resolves");
            assert_eq!(
                file.redirect, None,
                "the missing target is never picked (roll {roll})"
            );
            assert_eq!(file.name, "random/click");
        }
    }

    #[test]
    fn a_redirect_cycle_resolves_to_nothing() {
        let bytes = br#"{
            "loop": { "category": "master", "sounds": [{ "type": "event", "name": "loop" }] }
        }"#;
        let index = SoundIndex::parse(bytes).expect("the fixture parses");
        assert_eq!(index.total_weight("loop"), 0.0);
        assert_eq!(index.pick("loop", 0.5), None);
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
