//! The lewton decode path and the sample cache.
//!
//! Ogg bytes decode to interleaved `f32` PCM through lewton's `inside_ogg`
//! reader; the cache keeps decoded sounds by store key so a replayed event
//! never decodes twice. Streamed sounds (the `music.*` and `records.*`
//! files) decode through the same path into the same mix; the cache holds
//! at most the active stream alongside the buffered entries.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The most buffered sounds the cache holds; a new entry past the cap
/// evicts an arbitrary old one. A robustness bound, not a parity value.
pub const MAX_CACHED_SOUNDS: usize = 64;

/// One decoded sound: interleaved PCM in `[-1, 1]` with its layout.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedSound {
    /// The channel count (1 for mono, 2 for stereo).
    pub channels: usize,
    /// The sample rate in Hz.
    pub sample_rate: u32,
    /// The interleaved samples.
    pub samples: Vec<f32>,
}

impl DecodedSound {
    /// Builds decoded PCM directly (the synthetic path: tests generate
    /// their samples in-test and never touch the store).
    pub fn from_samples(channels: usize, sample_rate: u32, samples: Vec<f32>) -> Self {
        Self {
            channels,
            sample_rate,
            samples,
        }
    }

    /// The loudest absolute sample; silence reads 0.
    #[must_use]
    pub fn peak(&self) -> f32 {
        self.samples
            .iter()
            .fold(0.0f32, |loudest, sample| loudest.max(sample.abs()))
    }

    /// The source frame count.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.samples
            .len()
            .checked_div(self.channels.max(1))
            .unwrap_or(0)
    }
}

/// A decode failure.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// The bytes are not a Vorbis stream lewton accepts.
    #[error("the ogg does not decode: {0}")]
    Vorbis(String),
    /// The stream decodes but carries no audio.
    #[error("the ogg carries no audio packets")]
    Empty,
    /// The stream's channel count is not one the mixer maps.
    #[error("the ogg has {channels} channels; only mono and stereo map")]
    Channels {
        /// The channel count the stream states.
        channels: usize,
    },
}

/// Decodes ogg bytes to interleaved `f32` PCM through lewton's `inside_ogg`
/// reader. Anything lewton refuses is a [`DecodeError`], never a panic.
pub fn decode_ogg(bytes: &[u8]) -> Result<DecodedSound, DecodeError> {
    let mut reader =
        lewton::inside_ogg::OggStreamReader::new(Cursor::new(bytes)).map_err(vorbis_error)?;
    let channels = usize::from(reader.ident_hdr.audio_channels);
    if channels != 1 && channels != 2 {
        return Err(DecodeError::Channels { channels });
    }
    let sample_rate = reader.ident_hdr.audio_sample_rate;
    let mut samples = Vec::new();
    loop {
        match reader.read_dec_packet().map_err(vorbis_error)? {
            None => break,
            Some(packet) => {
                let frames = packet.first().map(Vec::len).unwrap_or(0);
                for frame in 0..frames {
                    for channel in packet.iter().take(channels) {
                        samples.push(f32::from(channel[frame]) / 32768.0);
                    }
                }
            }
        }
    }
    if samples.is_empty() {
        return Err(DecodeError::Empty);
    }
    Ok(DecodedSound {
        channels,
        sample_rate,
        samples,
    })
}

/// Renders a lewton failure as a string; lewton's error type stays out of
/// the public surface.
fn vorbis_error(error: lewton::VorbisError) -> DecodeError {
    DecodeError::Vorbis(format!("{error:?}"))
}

/// A store read failure: the audio side reads ogg bytes from the store
/// under a base directory (see [`read_store_ogg`]).
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The asset index does not read.
    #[error("the asset index does not read: {0}")]
    Index(String),
    /// The asset index is not the object map the store ships.
    #[error("the asset index has no object map")]
    IndexShape,
    /// The index names no object for the sound.
    #[error("the index names no object for {key:?}")]
    MissingObject {
        /// The object key the index was asked for.
        key: String,
    },
    /// The object the index names does not read.
    #[error("the store object does not read: {0}")]
    Object(String),
}

/// Reads ogg bytes for a sound file from the store under `root`: the
/// `1.8` asset index maps `minecraft/sounds/<name>.ogg` to its object
/// hash, and the bytes live at
/// `root/assets/objects/<first two hash chars>/<full hash>`.
///
/// This is the exact recipe the ignored `sound_index` suite pins for the
/// index object; only the ignored audio case calls it, so unit tests never
/// touch the filesystem.
pub fn read_store_ogg(root: &Path, file_name: &str) -> Result<Vec<u8>, StoreError> {
    std::fs::read(store_ogg_path(root, file_name)?)
        .map_err(|error| StoreError::Object(format!("{error}")))
}

/// Resolves the object path [`read_store_ogg`] reads, without reading it.
pub fn store_ogg_path(root: &Path, file_name: &str) -> Result<PathBuf, StoreError> {
    let key = format!("minecraft/sounds/{file_name}.ogg");
    let index_text = std::fs::read_to_string(root.join("assets/indexes/1.8.json"))
        .map_err(|error| StoreError::Index(format!("{error}")))?;
    let index: serde_json::Value =
        serde_json::from_str(&index_text).map_err(|_| StoreError::IndexShape)?;
    let hash = index
        .get("objects")
        .and_then(|objects| objects.get(&key))
        .and_then(|object| object.get("hash"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            if index.get("objects").is_none() {
                StoreError::IndexShape
            } else {
                StoreError::MissingObject { key: key.clone() }
            }
        })?;
    if hash.len() < 2 {
        return Err(StoreError::MissingObject { key });
    }
    Ok(root.join(format!("assets/objects/{}/{hash}", &hash[..2])))
}

/// The decoded-sample cache: buffered sounds by store key, plus at most
/// the active stream.
pub struct SampleCache {
    /// The buffered entries by store key.
    entries: HashMap<String, Arc<DecodedSound>>,
    /// The active streamed sound, when one plays.
    stream: Option<(String, Arc<DecodedSound>)>,
}

impl SampleCache {
    /// Builds an empty cache.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            stream: None,
        }
    }

    /// Looks up a decoded sound by store key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<Arc<DecodedSound>> {
        if let Some((held, decoded)) = self.stream.as_ref() {
            if held == key {
                return Some(Arc::clone(decoded));
            }
        }
        self.entries.get(key).map(Arc::clone)
    }

    /// Stores a decoded sound: streamed sounds replace the active stream
    /// slot (the cache holds at most the active stream), buffered sounds
    /// join the entries (evicting past the cap).
    pub fn insert(&mut self, key: String, decoded: Arc<DecodedSound>, streamed: bool) {
        if streamed {
            self.stream = Some((key, decoded));
            return;
        }
        if self.entries.len() >= MAX_CACHED_SOUNDS && !self.entries.contains_key(&key) {
            if let Some(evicted) = self.entries.keys().next().cloned() {
                self.entries.remove(&evicted);
            }
        }
        self.entries.insert(key, decoded);
    }

    /// The buffered entry count.
    #[must_use]
    pub fn buffered(&self) -> usize {
        self.entries.len()
    }

    /// Whether a stream is held.
    #[must_use]
    pub fn has_stream(&self) -> bool {
        self.stream.is_some()
    }
}

impl Default for SampleCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 0.1 s mono 440 Hz sine at 44100 Hz, straight into PCM.
    fn sine() -> DecodedSound {
        let samples: Vec<f32> = (0..4410)
            .map(|n| ((f64::from(n) * 440.0 * std::f64::consts::TAU) / 44100.0).sin() as f32)
            .collect();
        DecodedSound::from_samples(1, 44100, samples)
    }

    #[test]
    fn garbage_is_a_decode_error_not_a_panic() {
        assert!(matches!(
            decode_ogg(b"this is not an ogg"),
            Err(DecodeError::Vorbis(_))
        ));
        assert!(matches!(decode_ogg(b""), Err(DecodeError::Vorbis(_))));
    }

    #[test]
    fn the_sine_peaks_near_one() {
        let peak = sine().peak();
        assert!(peak > 0.99, "a full-scale sine peaks near 1, not {peak}");
    }

    #[test]
    fn the_stream_slot_holds_at_most_one() {
        let mut cache = SampleCache::new();
        let first = Arc::new(sine());
        let second = Arc::new(sine());
        cache.insert("minecraft/sounds/a.ogg".to_string(), first, true);
        cache.insert("minecraft/sounds/b.ogg".to_string(), second, true);
        assert!(cache.has_stream());
        assert!(cache.get("minecraft/sounds/b.ogg").is_some());
        assert!(cache.get("minecraft/sounds/a.ogg").is_none());
    }

    #[test]
    fn the_entries_cap_evicts() {
        let mut cache = SampleCache::new();
        for n in 0..=MAX_CACHED_SOUNDS {
            cache.insert(format!("k{n}"), Arc::new(sine()), false);
        }
        assert_eq!(cache.buffered(), MAX_CACHED_SOUNDS);
    }

    #[test]
    fn a_missing_object_names_its_key() {
        let error =
            store_ogg_path(Path::new("/nonexistent"), "dig/grass1").expect_err("no store reads");
        assert!(matches!(error, StoreError::Index(_)));
    }
}
