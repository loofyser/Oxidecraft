//! Animation sidecars: the `.png.mcmeta` files that sit beside a texture.
//!
//! Only the sidecar's `animation` object is read, and its values are kept as
//! the file states them. The effective per-frame schedule — a frame's own
//! `time` where it gives one, the `frametime` for the frames that do not —
//! is the atlas's to compute, not this module's.

use serde::Deserialize;

/// The animation values a sidecar states.
///
/// A sidecar with no `animation` object parses to the defaults below rather
/// than failing: an absent animation is a value, not an error. The four
/// `misc/` sidecars in the 1.8 tree, which state only a `texture` section,
/// are the real examples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnimationMeta {
    /// The default frame duration in ticks, as `frametime` states it, or 1
    /// when the file omits it.
    pub frametime: u32,
    /// The frame indices in playback order. Empty when the file omits
    /// `frames`; an empty list is read as the identity list by the atlas.
    pub frames: Vec<u32>,
    /// `frame_times[i]` is the `time` the `frames[i]` entry gives, or `None`
    /// when that entry gives none. Empty alongside `frames`.
    pub frame_times: Vec<Option<u32>>,
    /// True when the file asks for interpolation between frames.
    pub interpolate: bool,
}

/// Errors from parsing an animation sidecar.
#[derive(Debug, thiserror::Error)]
pub enum McmetaError {
    /// The text is not a JSON document of the shape the format allows.
    #[error("invalid animation JSON: {0}")]
    Json(#[from] serde_json::Error),
}

impl AnimationMeta {
    /// Parses a sidecar's text.
    ///
    /// The 1.8 shape: an optional `animation` object holding an optional
    /// `frametime`, an optional `frames` list whose entries are either a bare
    /// index or an `{ "index": .., "time": .. }` object, and an optional
    /// `interpolate`. A file with no `animation` object parses to the
    /// defaults; a file whose sections this milestone does not use (a
    /// `texture` section, say) parses by ignoring them.
    pub fn parse(json: &str) -> Result<Self, McmetaError> {
        let sidecar: Sidecar = serde_json::from_str(json)?;
        let Some(animation) = sidecar.animation else {
            return Ok(Self::still());
        };

        let mut frames = Vec::new();
        let mut frame_times = Vec::new();
        for entry in animation.frames.unwrap_or_default() {
            let (index, time) = match entry {
                Frame::Index(index) => (index, None),
                Frame::Timed { index, time } => (index, time),
            };
            frames.push(index);
            frame_times.push(time);
        }

        Ok(Self {
            frametime: animation.frametime,
            frames,
            frame_times,
            interpolate: animation.interpolate,
        })
    }

    /// The meta a sidecar with no `animation` object parses to.
    fn still() -> Self {
        Self {
            frametime: 1,
            frames: Vec::new(),
            frame_times: Vec::new(),
            interpolate: false,
        }
    }
}

/// The sidecar document: the `animation` object, when the file has one.
#[derive(Deserialize)]
struct Sidecar {
    /// The animation section.
    animation: Option<Animation>,
}

/// The `animation` section as 1.8 writes it.
#[derive(Deserialize)]
struct Animation {
    /// Ticks each frame shows for, when the frame gives no `time` of its own.
    #[serde(default = "default_frametime")]
    frametime: u32,
    /// The frame list, when the file gives one.
    frames: Option<Vec<Frame>>,
    /// Whether frames are interpolated.
    #[serde(default)]
    interpolate: bool,
}

/// One `frames` entry: a bare index, or an object with an optional `time`.
#[derive(Deserialize)]
#[serde(untagged)]
enum Frame {
    /// The bare-number form.
    Index(u32),
    /// The object form.
    Timed {
        /// The frame index.
        index: u32,
        /// The frame's own duration in ticks, when it states one.
        time: Option<u32>,
    },
}

/// The `frametime` a file that omits it states, in ticks.
fn default_frametime() -> u32 {
    1
}
