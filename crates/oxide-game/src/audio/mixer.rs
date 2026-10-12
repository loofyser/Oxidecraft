//! The pure mixing core: the voice gain law, the resampler and the fade.
//!
//! Everything here is a pure function over samples or a small state machine
//! over frames: no filesystem, no device, no thread. The law pins the
//! source's own rules — `SoundManager.java` (`playSound`, `getNormalizedPitch`,
//! `getNormalizedVolume`, the `f2 == 0.0F` skip) — to exact sampled values,
//! and the engine in the parent module feeds this core decoded PCM.
//!
//! Gain staging, in order: the normalised volume (call volume times the
//! file's volume, clamped to `[0, 1]`, times the category and master
//! volumes) times the attenuation gain (linear falloff to the range, full
//! gain for unpositioned sounds) times the per-voice fade envelope. Only a
//! zero normalised volume skips: a tiny non-zero volume plays at near-zero
//! gain. Pitch is the call pitch times the file's pitch, clamped to
//! `[0.5, 2.0]`.

use std::collections::VecDeque;

/// The lowest pitch factor playback produces.
pub const MIN_PITCH: f32 = 0.5;

/// The highest pitch factor playback produces.
pub const MAX_PITCH: f32 = 2.0;

/// The distance, in blocks, at which a call-volume-1 sound falls silent.
///
/// The source scales this by the call volume when it exceeds 1
/// (`SoundManager.java`, `playSound`: `f1 = 16.0F`, `if (f > 1.0F) f1 *= f`),
/// so a record at call volume 4 carries a 64-block range.
pub const DEFAULT_ATTENUATION_DISTANCE: f32 = 16.0;

/// The most voices the mixer holds; a new voice evicts the oldest.
///
/// A robustness bound, not a parity value: it keeps a burst of plays from
/// growing the mix without bound.
pub const MAX_VOICES: usize = 32;

/// The length of the per-voice attack/release ramp, in output frames.
///
/// An anti-click envelope, not a parity value: a voice opens from silence
/// over this many frames and closes back to silence over its last frames.
pub const FADE_SAMPLES: u64 = 128;

/// The normalised volume: the call volume times the file's volume, clamped
/// to `[0, 1]`, times the category and master volumes
/// (`SoundManager.java`, `getNormalizedVolume`). A zero answer skips the
/// play (`playSound`'s `f2 == 0.0F` rule); any non-zero answer, however
/// tiny, plays.
#[must_use]
pub fn normalised_volume(
    call_volume: f32,
    file_volume: f32,
    category_volume: f32,
    master_volume: f32,
) -> f32 {
    (call_volume * file_volume).clamp(0.0, 1.0) * category_volume * master_volume
}

/// The normalised pitch: the call pitch times the file's pitch, clamped to
/// `[0.5, 2.0]` (`SoundManager.java`, `getNormalizedPitch`).
#[must_use]
pub fn normalised_pitch(call_pitch: f32, file_pitch: f32) -> f32 {
    (call_pitch * file_pitch).clamp(MIN_PITCH, MAX_PITCH)
}

/// The distance, in blocks, at which a sound falls silent: the file's
/// `attenuation_distance` override where it states one (a non-positive
/// override clamps to 1 block), else 16 blocks scaled by the call volume
/// when it exceeds 1 (`SoundManager.java`, `playSound`'s `f1` rule).
#[must_use]
pub fn attenuation_range(call_volume: f32, override_distance: Option<i32>) -> f32 {
    match override_distance {
        Some(distance) => (distance.max(1) as f32).max(1.0),
        None => DEFAULT_ATTENUATION_DISTANCE * call_volume.max(1.0),
    }
}

/// The attenuation gain at a distance: linear falloff to the range, full
/// gain at the listener, silence at and past the range. A non-positive
/// range gains zero.
#[must_use]
pub fn attenuation_gain(distance: f32, range: f32) -> f32 {
    if range <= 0.0 {
        0.0
    } else {
        (1.0 - distance / range).clamp(0.0, 1.0)
    }
}

/// The fade envelope at a voice age: silence at age 0, full gain after the
/// attack ramp, back to silence over the voice's last frames.
#[must_use]
pub fn fade_gain(age: u64, total: u64) -> f32 {
    let attack = (age.min(FADE_SAMPLES) as f32) / (FADE_SAMPLES as f32);
    let release = (total.saturating_sub(age).min(FADE_SAMPLES) as f32) / (FADE_SAMPLES as f32);
    attack.min(release)
}

/// One stereo frame of source PCM at a fractional frame position, by linear
/// interpolation. A mono source centres on both ears; a stereo source maps
/// its channels; wider sources mix down to their first two channels. A
/// position past the last frame holds the last frame.
#[must_use]
pub fn sample_frame(pcm: &[f32], channels: usize, position: f64) -> [f32; 2] {
    if pcm.is_empty() || channels == 0 {
        return [0.0, 0.0];
    }
    let frames = pcm.len() / channels;
    if frames == 0 {
        return [0.0, 0.0];
    }
    let clamped = position.clamp(0.0, (frames - 1) as f64);
    let first = clamped.floor() as usize;
    let second = (first + 1).min(frames - 1);
    let fraction = (clamped - first as f64) as f32;
    let mut out = [0.0; 2];
    for (ear, slot) in out.iter_mut().enumerate() {
        let channel = if channels == 1 {
            0
        } else {
            ear.min(channels - 1)
        };
        let low = pcm[first * channels + channel];
        let high = pcm[second * channels + channel];
        *slot = low + (high - low) * fraction;
    }
    out
}

/// One playing voice: decoded PCM with its playback cursor, rate step,
/// gain and age.
pub struct Voice {
    /// The decoded PCM, interleaved by channel.
    pcm: Vec<f32>,
    /// The channel count of `pcm`.
    channels: usize,
    /// The fractional frame cursor.
    position: f64,
    /// The source frames advanced per output frame (rate ratio times pitch).
    step: f64,
    /// The staged gain (normalised volume times attenuation).
    gain: f32,
    /// The output frames rendered so far.
    age: u64,
    /// The source frame count.
    total: u64,
    /// Whether this voice streams (kept for the engine's accounting).
    stream: bool,
}

impl Voice {
    /// Builds a voice over decoded PCM at an output rate and pitch factor.
    pub fn new(
        pcm: Vec<f32>,
        channels: usize,
        source_rate: u32,
        output_rate: u32,
        pitch: f32,
        gain: f32,
        stream: bool,
    ) -> Self {
        let channels = channels.max(1);
        let total = (pcm.len() / channels) as u64;
        let step = (source_rate.max(1) as f64) / (output_rate.max(1) as f64) * (pitch as f64);
        Self {
            pcm,
            channels,
            position: 0.0,
            step: step.max(f64::MIN_POSITIVE),
            gain,
            age: 0,
            total,
            stream,
        }
    }

    /// Whether this voice streams.
    #[must_use]
    pub fn is_stream(&self) -> bool {
        self.stream
    }

    /// Mixes one buffer of stereo frames additively into `out` (whose
    /// length must be even); answers true once the voice is spent.
    pub fn render(&mut self, out: &mut [f32]) -> bool {
        debug_assert!(out.len() % 2 == 0, "the mix runs in stereo frames");
        for frame in out.chunks_mut(2) {
            if self.position >= self.total as f64 {
                break;
            }
            let stereo = sample_frame(&self.pcm, self.channels, self.position);
            let envelope = self.gain * fade_gain(self.age, self.total);
            frame[0] += stereo[0] * envelope;
            frame[1] += stereo[1] * envelope;
            self.position += self.step;
            self.age += 1;
        }
        self.position >= self.total as f64
    }
}

/// The mixer: the live voices over one output rate.
pub struct Mixer {
    /// The live voices, oldest first.
    voices: VecDeque<Voice>,
    /// The output sample rate the voices were stepped for.
    output_rate: u32,
}

impl Mixer {
    /// Builds an empty mixer at an output rate.
    pub fn new(output_rate: u32) -> Self {
        Self {
            voices: VecDeque::new(),
            output_rate,
        }
    }

    /// The output sample rate.
    #[must_use]
    pub fn output_rate(&self) -> u32 {
        self.output_rate
    }

    /// Pushes a voice, evicting the oldest when the pool is full.
    pub fn push(&mut self, voice: Voice) {
        if self.voices.len() >= MAX_VOICES {
            self.voices.pop_front();
        }
        self.voices.push_back(voice);
    }

    /// Renders one buffer of stereo frames into `out` (whose length must
    /// be even), mixing every live voice and culling the spent.
    pub fn render(&mut self, out: &mut [f32]) {
        for sample in out.iter_mut() {
            *sample = 0.0;
        }
        let mut spent = Vec::new();
        for (slot, voice) in self.voices.iter_mut().enumerate() {
            if voice.render(out) {
                spent.push(slot);
            }
        }
        for slot in spent.into_iter().rev() {
            self.voices.remove(slot);
        }
    }

    /// The live voice count.
    #[must_use]
    pub fn voices(&self) -> usize {
        self.voices.len()
    }

    /// Drops every voice.
    pub fn clear(&mut self) {
        self.voices.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_attenuation_curve_pins_sampled_values() {
        assert_eq!(attenuation_gain(0.0, 16.0), 1.0);
        assert_eq!(attenuation_gain(1.0, 16.0), 0.9375);
        assert_eq!(attenuation_gain(15.0, 16.0), 0.0625);
        assert_eq!(attenuation_gain(16.0, 16.0), 0.0);
        assert_eq!(attenuation_gain(17.0, 16.0), 0.0);
        assert_eq!(attenuation_gain(100.0, 16.0), 0.0);
    }

    #[test]
    fn the_range_scales_with_call_volume_and_override() {
        assert_eq!(attenuation_range(1.0, None), 16.0);
        assert_eq!(attenuation_range(0.5, None), 16.0);
        assert_eq!(attenuation_range(4.0, None), 64.0);
        assert_eq!(attenuation_range(1.0, Some(32)), 32.0);
        assert_eq!(attenuation_range(4.0, Some(32)), 32.0);
    }

    #[test]
    fn the_category_volume_halves_the_mix() {
        assert_eq!(normalised_volume(1.0, 1.0, 0.5, 1.0), 0.5);
        assert_eq!(normalised_volume(1.0, 1.0, 1.0, 1.0), 1.0);
        assert_eq!(normalised_volume(1.0, 1.0, 1.0, 0.25), 0.25);
    }

    #[test]
    fn only_a_zero_volume_skips() {
        assert_eq!(normalised_volume(0.0, 1.0, 1.0, 1.0), 0.0);
        assert_eq!(normalised_volume(1.0, 1.0, 0.0, 1.0), 0.0);
        assert_eq!(normalised_volume(1.0, 1.0, 1.0, 0.0), 0.0);
        let tiny = normalised_volume(0.005, 1.0, 1.0, 1.0);
        assert!(tiny > 0.0, "0.005 plays at near-zero gain, not silence");
        assert_eq!(tiny, 0.005);
    }

    #[test]
    fn the_volume_product_clamps_to_one() {
        assert_eq!(normalised_volume(4.0, 1.0, 1.0, 1.0), 1.0);
        assert_eq!(normalised_volume(1.0, 2.0, 1.0, 1.0), 1.0);
    }

    #[test]
    fn the_pitch_clamps() {
        assert_eq!(normalised_pitch(2.5, 1.0), 2.0);
        assert_eq!(normalised_pitch(0.4, 1.0), 0.5);
        assert_eq!(normalised_pitch(1.0, 1.0), 1.0);
        assert_eq!(normalised_pitch(1.0, 1.5), 1.5);
    }

    #[test]
    fn the_resampler_interpolates_a_ramp() {
        let ramp = [0.0, 1.0, 2.0, 3.0];
        assert_eq!(sample_frame(&ramp, 1, 0.0), [0.0, 0.0]);
        assert_eq!(sample_frame(&ramp, 1, 0.5), [0.5, 0.5]);
        assert_eq!(sample_frame(&ramp, 1, 2.0), [2.0, 2.0]);
        assert_eq!(sample_frame(&ramp, 1, 3.0), [3.0, 3.0]);
        assert_eq!(sample_frame(&ramp, 1, 99.0), [3.0, 3.0]);
        let stereo = [0.0, 10.0, 2.0, 12.0];
        assert_eq!(sample_frame(&stereo, 2, 0.5), [1.0, 11.0]);
    }

    #[test]
    fn the_fade_opens_and_closes() {
        assert_eq!(fade_gain(0, 10_000), 0.0);
        assert_eq!(fade_gain(FADE_SAMPLES, 10_000), 1.0);
        assert_eq!(fade_gain(5_000, 10_000), 1.0);
        let closing = fade_gain(9_999, 10_000);
        assert!(
            closing > 0.0 && closing < 1.0,
            "the last frames ramp back down: {closing}"
        );
        assert_eq!(fade_gain(10_000, 10_000), 0.0);
    }

    #[test]
    fn a_voice_plays_out_and_goes_spent() {
        let pcm: Vec<f32> = (0..4410).map(|n| (n as f32) / 4410.0).collect();
        let mut voice = Voice::new(pcm, 1, 44100, 44100, 1.0, 1.0, false);
        let mut out = vec![0.0; 2048];
        let mut energy = 0.0;
        let mut spent = false;
        for _ in 0..8 {
            spent = voice.render(&mut out);
            energy += out.iter().map(|sample| sample.abs()).sum::<f32>();
            if spent {
                break;
            }
        }
        assert!(
            spent,
            "a 0.1 s voice spends within 8 renders of 1024 frames"
        );
        assert!(energy > 0.0, "the voice mixes non-silence");
    }

    #[test]
    fn the_pool_evicts_the_oldest_voice() {
        let mut mixer = Mixer::new(44100);
        for _ in 0..=MAX_VOICES {
            mixer.push(Voice::new(vec![0.5; 64], 1, 44100, 44100, 1.0, 1.0, false));
        }
        assert_eq!(mixer.voices(), MAX_VOICES);
    }
}
