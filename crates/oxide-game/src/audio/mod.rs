//! The audio engine: the category mixer, the device shell and the registry
//! hookup.
//!
//! [`AudioEngine`] owns the play channel, the decoded-sample cache, the
//! [`Mixer`][crate::audio::mixer::Mixer] and the output device. Every play
//! call is a channel send: the bounded channel (overflow drops the oldest,
//! never blocks) keeps the tick and read threads free of decode and mixing
//! work, and the client's tick drains the channel through
//! [`AudioEngine::tick`]. A device that cannot open degrades to the silent
//! device and the client keeps running.
//!
//! Sound resolution runs per play: the event name (length-capped before
//! lookup — a missing entry plays nothing and is counted) resolves through
//! the [`SoundIndex`][oxide_assets::sound::SoundIndex] pick (redirects
//! delegate to their target's files), the file decodes from the store
//! through the cache (or from injected samples when no base directory is
//! set), and the voice stages the mixer's law — normalised volume times
//! attenuation — at the clamped pitch.

pub mod decode;
pub mod mixer;
pub mod music;
pub mod sounds;

use std::collections::VecDeque;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender, bounded};
use rand::{RngExt, SeedableRng, rngs::StdRng};
use tracing::warn;

use oxide_assets::sound::{MAX_EVENT_NAME_BYTES, SoundIndex};

use crate::audio::decode::{DecodedSound, SampleCache, decode_ogg, read_store_ogg};
use crate::audio::mixer::{
    Mixer, Voice, attenuation_gain, attenuation_range, normalised_pitch, normalised_volume,
};

/// The sound categories, in the source's order (`SoundCategory.java`:
/// `MASTER` 0 through `AMBIENT` 8).
///
/// The enum constant names and the category name strings differ: `RECORDS`
/// is `"record"`, `BLOCKS` is `"block"`, `MOBS` is `"hostile"`, `ANIMALS`
/// is `"neutral"` and `PLAYERS` is `"player"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SoundCategory {
    /// The master bus.
    Master,
    /// Music.
    Music,
    /// Record (jukebox) sounds.
    Records,
    /// Weather.
    Weather,
    /// Blocks.
    Blocks,
    /// Hostile mobs.
    Mobs,
    /// Neutral animals.
    Animals,
    /// Players.
    Players,
    /// Ambient loops.
    Ambient,
}

impl SoundCategory {
    /// The nine categories in source order.
    pub const ORDER: [SoundCategory; 9] = [
        SoundCategory::Master,
        SoundCategory::Music,
        SoundCategory::Records,
        SoundCategory::Weather,
        SoundCategory::Blocks,
        SoundCategory::Mobs,
        SoundCategory::Animals,
        SoundCategory::Players,
        SoundCategory::Ambient,
    ];

    /// The category name string (`SoundCategory.java`, `getCategoryName`).
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            SoundCategory::Master => "master",
            SoundCategory::Music => "music",
            SoundCategory::Records => "record",
            SoundCategory::Weather => "weather",
            SoundCategory::Blocks => "block",
            SoundCategory::Mobs => "hostile",
            SoundCategory::Animals => "neutral",
            SoundCategory::Players => "player",
            SoundCategory::Ambient => "ambient",
        }
    }

    /// The category id (`SoundCategory.java`, `getCategoryId`).
    #[must_use]
    pub fn as_index(&self) -> usize {
        match self {
            SoundCategory::Master => 0,
            SoundCategory::Music => 1,
            SoundCategory::Records => 2,
            SoundCategory::Weather => 3,
            SoundCategory::Blocks => 4,
            SoundCategory::Mobs => 5,
            SoundCategory::Animals => 6,
            SoundCategory::Players => 7,
            SoundCategory::Ambient => 8,
        }
    }

    /// Maps an index category string to its variant; unknown names fall
    /// back to master rather than refusing the play.
    #[must_use]
    pub fn from_name(name: &str) -> SoundCategory {
        for category in Self::ORDER {
            if category.name() == name {
                return category;
            }
        }
        SoundCategory::Master
    }
}

/// The listener pose: ears position and facing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ListenerPose {
    /// The ears position in blocks.
    pub pos: [f32; 3],
    /// The facing yaw in radians (kept for a future pan stage; the current
    /// mix centres positional sounds).
    pub yaw: f32,
}

/// One sound play: the event to resolve and the call-site staging.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaySound {
    /// The sound event name, exactly as sent.
    pub name: String,
    /// The call-site category hint, used when the event's own category
    /// string names nothing known.
    pub category_hint: SoundCategory,
    /// The sound position in blocks; `None` plays unpositioned at full
    /// gain (music, UI).
    pub pos: Option<[f32; 3]>,
    /// The call volume (the wire scale, or the local call site's own; a
    /// record's 4.0 carries its 64-block range).
    pub volume: f32,
    /// The call pitch factor (the wire's 0x29 pitch byte scaled by 63, via
    /// [`oxide_proto_v47::effects_wire::read_sound_pitch`], or the local
    /// call site's own).
    pub pitch: f32,
    /// The listener ears position in blocks at call time.
    pub listener: [f32; 3],
    /// The listener yaw in radians at call time.
    pub yaw: f32,
}

/// The commands the play channel carries; [`AudioEngine::tick`] drains
/// them into the mixer.
#[derive(Debug)]
enum AudioCommand {
    /// Plays one sound.
    Play(PlaySound),
    /// Stops every voice and drops the queued plays.
    StopAll,
}

/// The most commands the play channel holds; a send past the cap drops
/// the oldest, never blocks. A robustness bound, not a parity value.
pub const COMMAND_CAP: usize = 64;

/// The output device description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceSpec {
    /// The sample rate in Hz.
    pub sample_rate: u32,
    /// The channel count (always stereo here).
    pub channels: u16,
}

/// The output sink: the engine renders stereo frames at the spec rate and
/// pushes them here. The cpal device queues them for its stream callback;
/// the silent device counts them and drops them.
pub trait AudioDevice: Send {
    /// The rate and layout the engine must render at.
    fn spec(&self) -> DeviceSpec;
    /// Accepts rendered stereo frames; never blocks (a full queue drops
    /// the incoming frames).
    fn push(&mut self, frames: &[f32]);
}

/// The silent device: records the pushed frame count and drops the audio.
/// The fallback when no device opens, and the device the suites play over.
pub struct SilentDevice {
    /// The rate the engine renders at.
    spec: DeviceSpec,
    /// The stereo frames accepted so far.
    frames_accepted: u64,
}

impl SilentDevice {
    /// Opens the silent device at 44100 Hz stereo.
    pub fn open() -> Self {
        Self {
            spec: DeviceSpec {
                sample_rate: 44100,
                channels: 2,
            },
            frames_accepted: 0,
        }
    }

    /// The stereo frames accepted so far.
    #[must_use]
    pub fn frames_accepted(&self) -> u64 {
        self.frames_accepted
    }
}

impl AudioDevice for SilentDevice {
    fn spec(&self) -> DeviceSpec {
        self.spec
    }

    fn push(&mut self, frames: &[f32]) {
        self.frames_accepted += (frames.len() / 2) as u64;
    }
}

/// The cpal device: the default output stream drains an internal queue on
/// its own callback thread, so device work never touches the engine's
/// threads; the engine's [`AudioDevice::push`] only enqueues.
pub struct CpalDevice {
    /// The negotiated rate and layout.
    spec: DeviceSpec,
    /// The queued stereo frames the callback drains.
    queue: Arc<std::sync::Mutex<VecDeque<f32>>>,
    /// The live stream; held so it keeps running.
    _stream: cpal::Stream,
    /// The device name, for the run log.
    device_name: String,
}

impl CpalDevice {
    /// The most stereo frames the queue holds (~2 s at 44100 Hz); a push
    /// past it drops the incoming frames, never blocks.
    const QUEUE_CAP_FRAMES: usize = 44100 * 2;

    /// Opens the default output device; answers `None` when no host,
    /// device or stream configuration is available, so the engine falls
    /// back to silence.
    #[must_use]
    pub fn open() -> Option<Self> {
        let host = cpal::default_host();
        let device = host.default_output_device()?;
        let device_name = device
            .description()
            .map(|description| description.name().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        let supported = device.default_output_config().ok()?;
        let spec = DeviceSpec {
            sample_rate: supported.sample_rate(),
            channels: supported.channels(),
        };
        let stream_config = supported.config();
        let queue = Arc::new(std::sync::Mutex::new(VecDeque::new()));
        let drained = Arc::clone(&queue);
        let channels = usize::from(spec.channels);
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                stream_config,
                move |output: &mut [f32], _| drain_queue(&drained, output, channels),
                |error| warn!("the audio stream errored: {error}"),
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                stream_config,
                move |output: &mut [i16], _| {
                    drain_queue_i16(&drained, output, channels);
                },
                |error| warn!("the audio stream errored: {error}"),
                None,
            ),
            cpal::SampleFormat::U16 => device.build_output_stream(
                stream_config,
                move |output: &mut [u16], _| {
                    drain_queue_u16(&drained, output, channels);
                },
                |error| warn!("the audio stream errored: {error}"),
                None,
            ),
            _ => return None,
        }
        .ok()?;
        stream.play().ok()?;
        Some(Self {
            spec,
            queue,
            _stream: stream,
            device_name,
        })
    }

    /// The output device name, for the run log.
    #[must_use]
    pub fn device_name(&self) -> &str {
        &self.device_name
    }
}

impl AudioDevice for CpalDevice {
    fn spec(&self) -> DeviceSpec {
        self.spec
    }

    fn push(&mut self, frames: &[f32]) {
        if let Ok(mut queue) = self.queue.lock() {
            if queue.len() < Self::QUEUE_CAP_FRAMES * 2 {
                queue.extend(frames.iter());
            }
        }
    }
}

/// Drains queued stereo frames into an `f32` stream buffer, mapping to the
/// device channel count and zero-filling on underrun.
fn drain_queue(queue: &Arc<std::sync::Mutex<VecDeque<f32>>>, output: &mut [f32], channels: usize) {
    let Ok(mut held) = queue.lock() else {
        output.fill(0.0);
        return;
    };
    for frame in output.chunks_mut(channels.max(1)) {
        let left = held.pop_front().unwrap_or(0.0);
        let right = held.pop_front().unwrap_or(0.0);
        for (slot, value) in frame.iter_mut().enumerate() {
            *value = if slot % 2 == 0 { left } else { right };
        }
    }
}

/// Drains queued stereo frames into an `i16` stream buffer.
fn drain_queue_i16(
    queue: &Arc<std::sync::Mutex<VecDeque<f32>>>,
    output: &mut [i16],
    channels: usize,
) {
    let mut floats = vec![0.0f32; output.len()];
    drain_queue(queue, &mut floats, channels);
    for (slot, value) in output.iter_mut().zip(floats) {
        *slot = (value.clamp(-1.0, 1.0) * 32767.0) as i16;
    }
}

/// Drains queued stereo frames into a `u16` stream buffer.
fn drain_queue_u16(
    queue: &Arc<std::sync::Mutex<VecDeque<f32>>>,
    output: &mut [u16],
    channels: usize,
) {
    let mut floats = vec![0.0f32; output.len()];
    drain_queue(queue, &mut floats, channels);
    for (slot, value) in output.iter_mut().zip(floats) {
        *slot = ((value.clamp(-1.0, 1.0) * 0.5 + 0.5) * 65535.0) as u16;
    }
}

/// The engine counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EngineStats {
    /// The voices spawned so far.
    pub plays: u64,
    /// The plays that resolved nothing (unknown or overlong names, empty
    /// events, missing redirect targets, undecodable files).
    pub missing: u64,
    /// The plays skipped on a zero staged gain.
    pub zero_skips: u64,
    /// The streamed voices spawned so far.
    pub streamed_plays: u64,
}

/// The audio engine: the play channel, the sample cache, the mixer and
/// the device.
pub struct AudioEngine {
    /// The sound index plays resolve against.
    index: Arc<SoundIndex>,
    /// The audio-side base directory; `Some` reads ogg bytes from the
    /// store under it, `None` plays synthetic/injected data only.
    cache_dir: Option<PathBuf>,
    /// The play command sender (every [`AudioEngine::play`] sends here).
    sender: Sender<AudioCommand>,
    /// The play command receiver ([`AudioEngine::tick`] drains it).
    receiver: Receiver<AudioCommand>,
    /// The mixer.
    mixer: Mixer,
    /// The decoded-sample cache.
    cache: SampleCache,
    /// The pick-roll source.
    rng: StdRng,
    /// The per-category volumes, indexed by [`SoundCategory::as_index`].
    category_volumes: [f32; 9],
    /// The master volume.
    master_volume: f32,
    /// The last listener pose [`AudioEngine::tick`] saw.
    listener: ListenerPose,
    /// The output device.
    device: Box<dyn AudioDevice>,
    /// The armed PCM dump, when `--audio-dump` runs.
    dump: Option<WavDump>,
    /// The counters.
    stats: EngineStats,
}

impl AudioEngine {
    /// Builds the engine over an index, an optional store base directory
    /// and a device. A `None` device (or no store directory) is not an
    /// error: the engine falls back to the silent device, records one log
    /// line, and keeps running.
    pub fn new(
        index: Arc<SoundIndex>,
        cache_dir: Option<PathBuf>,
        device: Option<Box<dyn AudioDevice>>,
    ) -> Self {
        let (sender, receiver) = bounded(COMMAND_CAP);
        let device = match device {
            Some(device) => device,
            None => {
                warn!("no audio device opened; the sound engine runs silent");
                Box::new(SilentDevice::open()) as Box<dyn AudioDevice>
            }
        };
        let output_rate = device.spec().sample_rate;
        Self {
            index,
            cache_dir,
            sender,
            receiver,
            mixer: Mixer::new(output_rate),
            cache: SampleCache::new(),
            rng: StdRng::seed_from_u64(engine_seed()),
            category_volumes: [1.0; 9],
            master_volume: 1.0,
            listener: ListenerPose {
                pos: [0.0, 0.0, 0.0],
                yaw: 0.0,
            },
            device,
            dump: None,
            stats: EngineStats::default(),
        }
    }

    /// Queues one play; a full channel drops the oldest command first, so
    /// this never blocks. (Under concurrent senders a racing play may be
    /// dropped instead of displacing; the tick-owned client sends from one
    /// thread.)
    pub fn play(&self, event: PlaySound) {
        if self
            .sender
            .try_send(AudioCommand::Play(event.clone()))
            .is_err()
        {
            let _ = self.receiver.try_recv();
            let _ = self.sender.try_send(AudioCommand::Play(event));
        }
    }

    /// Sets one category's volume, clamped to `[0, 1]`.
    pub fn set_category_volume(&mut self, category: SoundCategory, volume: f32) {
        self.category_volumes[category.as_index()] = volume.clamp(0.0, 1.0);
    }

    /// Sets the master volume, clamped to `[0, 1]`.
    pub fn set_master_volume(&mut self, volume: f32) {
        self.master_volume = volume.clamp(0.0, 1.0);
    }

    /// Runs one client tick: records the listener, drains the play channel
    /// into the mixer, and feeds the device one chunk.
    pub fn tick(&mut self, listener: ListenerPose) {
        self.listener = listener;
        self.drain();
        let mut chunk = vec![0.0; 1024 * 2];
        self.mixer.render(&mut chunk);
        self.device.push(&chunk);
        if let Some(dump) = self.dump.as_mut() {
            dump.write_frames(&chunk);
        }
    }

    /// Renders exactly `out` (interleaved stereo at the device rate) after
    /// draining the play channel; the deterministic path the suites mix
    /// through.
    pub fn render_into(&mut self, out: &mut [f32]) {
        self.drain();
        self.mixer.render(out);
        if let Some(dump) = self.dump.as_mut() {
            dump.write_frames(out);
        }
    }

    /// Stops every voice and drops the queued plays.
    pub fn stop_all(&mut self) {
        self.mixer.clear();
        while self.receiver.try_recv().is_ok() {}
    }

    /// Queues a stop: the next [`AudioEngine::tick`] (or
    /// [`AudioEngine::render_into`]) clears every voice and drops the queued
    /// plays, the channel-send form of [`AudioEngine::stop_all`] for owners
    /// of a shared engine — the music ticker stops a finished kind through
    /// it without a mutable borrow.
    pub fn stop_async(&self) {
        if self.sender.try_send(AudioCommand::StopAll).is_err() {
            let _ = self.receiver.try_recv();
            let _ = self.sender.try_send(AudioCommand::StopAll);
        }
    }

    /// Starts the `--audio-dump` capture: every rendered chunk appends to
    /// a 16-bit stereo WAV at `path` until [`AudioEngine::debug_dump_stop`].
    pub fn debug_dump_start(&mut self, path: &Path) {
        let spec = self.device.spec();
        match WavDump::create(path, spec) {
            Ok(dump) => self.dump = Some(dump),
            Err(error) => warn!("the audio dump does not start: {error}"),
        }
    }

    /// Stops the capture and finalises the WAV header.
    pub fn debug_dump_stop(&mut self) {
        if let Some(dump) = self.dump.take() {
            if let Err(error) = dump.finish() {
                warn!("the audio dump does not finalise: {error}");
            }
        }
    }

    /// The queued (undrained) play count.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.receiver.len()
    }

    /// The live voice count.
    #[must_use]
    pub fn voices(&self) -> usize {
        self.mixer.voices()
    }

    /// The counters.
    #[must_use]
    pub fn stats(&self) -> EngineStats {
        self.stats
    }

    /// The device spec the engine renders at.
    #[must_use]
    pub fn device_spec(&self) -> DeviceSpec {
        self.device.spec()
    }

    /// Injects decoded PCM under a store key (see [`AudioEngine::cache_key`]):
    /// the synthetic path tests play through when no base directory is set.
    pub fn inject(&mut self, key: &str, decoded: DecodedSound) {
        self.cache.insert(key.to_string(), Arc::new(decoded), false);
    }

    /// The cache key a file decodes under: `minecraft/sounds/<name>.ogg`.
    #[must_use]
    pub fn cache_key(file_name: &str) -> String {
        format!("minecraft/sounds/{file_name}.ogg")
    }

    /// Drains the play channel into the mixer.
    fn drain(&mut self) {
        while let Ok(command) = self.receiver.try_recv() {
            match command {
                AudioCommand::Play(play) => self.spawn(&play),
                AudioCommand::StopAll => {
                    self.mixer.clear();
                    while self.receiver.try_recv().is_ok() {}
                    return;
                }
            }
        }
    }

    /// Resolves one play into a mixer voice (or a counter).
    fn spawn(&mut self, play: &PlaySound) {
        if play.name.len() > MAX_EVENT_NAME_BYTES {
            self.stats.missing += 1;
            return;
        }
        let Some(event) = self.index.events.get(play.name.as_str()) else {
            self.stats.missing += 1;
            return;
        };
        let category = SoundCategory::from_name(event.category.as_str());
        let category = if category == SoundCategory::Master && event.category != "master" {
            play.category_hint
        } else {
            category
        };
        let roll = self.rng.random::<f32>();
        let Some(file) = self.index.pick(play.name.as_str(), roll) else {
            self.stats.missing += 1;
            return;
        };
        let stream = if let Some(target) = file.redirect.as_deref() {
            self.index
                .events
                .get(target)
                .map(|def| def.effective_stream(file))
                .unwrap_or(false)
        } else {
            event.effective_stream(file)
        };
        let staged = normalised_volume(
            play.volume,
            file.volume,
            self.category_volumes[category.as_index()],
            self.master_volume,
        );
        if staged == 0.0 {
            self.stats.zero_skips += 1;
            return;
        }
        let range = attenuation_range(play.volume, file.attenuation_distance);
        let gain = match play.pos {
            None => staged,
            Some(pos) => {
                let distance = distance_to(play.listener, pos);
                staged * attenuation_gain(distance, range)
            }
        };
        if gain == 0.0 {
            self.stats.zero_skips += 1;
            return;
        }
        let pitch = normalised_pitch(play.pitch, file.pitch);
        let key = Self::cache_key(file.name.as_str());
        let decoded = match self.cache.get(&key) {
            Some(decoded) => decoded,
            None => match self.load(&key, file.name.as_str()) {
                Some(decoded) => {
                    let held = Arc::clone(&decoded);
                    self.cache.insert(key, decoded, stream);
                    held
                }
                None => {
                    self.stats.missing += 1;
                    return;
                }
            },
        };
        let output_rate = self.device.spec().sample_rate;
        self.mixer.push(Voice::new(
            decoded.samples.clone(),
            decoded.channels,
            decoded.sample_rate,
            output_rate,
            pitch,
            gain,
            stream,
        ));
        self.stats.plays += 1;
        if stream {
            self.stats.streamed_plays += 1;
        }
    }

    /// Loads one file's PCM: from the store under the base directory when
    /// one is set, else nothing (synthetic/injected data only).
    fn load(&self, _key: &str, file_name: &str) -> Option<Arc<DecodedSound>> {
        let root = self.cache_dir.as_deref()?;
        let bytes = read_store_ogg(root, file_name).ok()?;
        decode_ogg(&bytes).ok().map(Arc::new)
    }
}

/// The Euclidean distance in blocks from the listener to a sound position.
fn distance_to(listener: [f32; 3], pos: [f32; 3]) -> f32 {
    let dx = pos[0] - listener[0];
    let dy = pos[1] - listener[1];
    let dz = pos[2] - listener[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Seeds the pick-roll source from the wall clock and process id: unique
/// per run without touching OS entropy (which can fail), and never a
/// panic path.
fn engine_seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|span| span.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15);
    nanos ^ (u64::from(std::process::id()).wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

/// A minimal 16-bit stereo WAV writer for `--audio-dump`.
struct WavDump {
    /// The file under capture.
    file: File,
    /// The stereo frames written so far.
    frames: u32,
    /// The rate the engine renders at.
    sample_rate: u32,
}

impl WavDump {
    /// Creates the capture at `path`, deferring the header sizes to
    /// [`WavDump::finish`].
    fn create(path: &Path, spec: DeviceSpec) -> std::io::Result<Self> {
        let mut file = File::create(path)?;
        let sample_rate = spec.sample_rate;
        file.write_all(&wav_header(sample_rate, 0))?;
        Ok(Self {
            file,
            frames: 0,
            sample_rate,
        })
    }

    /// Appends rendered stereo frames as 16-bit samples.
    fn write_frames(&mut self, frames: &[f32]) {
        let mut bytes = Vec::with_capacity(frames.len() * 2);
        for sample in frames {
            bytes.extend_from_slice(&((sample.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
        if self.file.write_all(&bytes).is_ok() {
            self.frames += (frames.len() / 2) as u32;
        }
    }

    /// Finalises the header sizes by rewriting the header in place.
    fn finish(mut self) -> std::io::Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        self.file
            .write_all(&wav_header(self.sample_rate, self.frames))?;
        Ok(())
    }
}

impl Drop for WavDump {
    fn drop(&mut self) {
        let header = wav_header(self.sample_rate, self.frames);
        if self.file.seek(SeekFrom::Start(0)).is_ok() {
            let _ = self.file.write_all(&header);
        }
    }
}

/// The 44-byte WAV header for 16-bit stereo PCM at a rate and frame count.
fn wav_header(sample_rate: u32, frames: u32) -> [u8; 44] {
    let bytes = frames * 4;
    let mut header = [0u8; 44];
    header[0..4].copy_from_slice(b"RIFF");
    header[4..8].copy_from_slice(&(36 + bytes).to_le_bytes());
    header[8..12].copy_from_slice(b"WAVE");
    header[12..16].copy_from_slice(b"fmt ");
    header[16..20].copy_from_slice(&16u32.to_le_bytes());
    header[20..22].copy_from_slice(&1u16.to_le_bytes());
    header[22..24].copy_from_slice(&2u16.to_le_bytes());
    header[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    header[28..32].copy_from_slice(&(sample_rate * 4).to_le_bytes());
    header[32..34].copy_from_slice(&4u16.to_le_bytes());
    header[34..36].copy_from_slice(&16u16.to_le_bytes());
    header[36..40].copy_from_slice(b"data");
    header[40..44].copy_from_slice(&bytes.to_le_bytes());
    header
}
