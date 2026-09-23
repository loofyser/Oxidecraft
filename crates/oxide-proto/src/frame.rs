//! Length-prefixed packet framing, with the 1.8 compression rules.

use std::io::{self, Read, Write};

use flate2::Compression as ZlibLevel;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

use crate::varint::{VarIntError, read_varint, write_varint};

/// How the connection compresses frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    /// No compression, as before Set Compression arrives.
    Disabled,
    /// The server sent a threshold. A negative threshold means plain framing
    /// in both directions: no Data Length field is written or read, exactly as
    /// before Set Compression arrived. Prefer [`Compression::from_server_threshold`]
    /// when wiring Set Compression.
    Enabled {
        /// Minimum uncompressed `Packet ID + Data` size that gets compressed.
        threshold: i32,
    },
}

impl Compression {
    /// Normalises a server-supplied threshold: a negative value means plain framing.
    pub fn from_server_threshold(threshold: i32) -> Self {
        if threshold < 0 {
            Compression::Disabled
        } else {
            Compression::Enabled { threshold }
        }
    }
}

/// The threshold in force, or `None` when framing is plain.
///
/// `Compression::Disabled` and a negative threshold both mean plain framing,
/// so both take the same path here.
fn active_threshold(mode: Compression) -> Option<usize> {
    match mode {
        Compression::Disabled => None,
        Compression::Enabled { threshold } if threshold < 0 => None,
        Compression::Enabled { threshold } => Some(threshold as usize),
    }
}

/// Errors from reading or writing frames.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// Underlying IO failure.
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    /// The length prefix was malformed.
    #[error("bad varint: {0}")]
    VarInt(#[from] VarIntError),
    /// The frame exceeded the sanity limit.
    #[error("frame length {0} exceeds the {1} byte limit")]
    TooLong(usize, usize),
    /// A frame length or Data Length below zero was declared.
    #[error("negative length: {0}")]
    NegativeLength(i32),
    /// The declared uncompressed size was not respected.
    #[error("decompressed frame does not match declared Data Length")]
    BadCompression,
    /// A zero-length payload cannot be framed.
    #[error("cannot frame an empty payload")]
    EmptyPayload,
}

/// Frames larger than this are rejected outright as hostile.
pub const MAX_FRAME_LEN: usize = 2 * 1024 * 1024 + 1024;

/// Writes one frame: VarInt frame length, then payload, compressing per `mode`.
///
/// A negative threshold writes a plain frame, as does [`Compression::Disabled`].
/// An empty payload is refused, and so is a frame the reader would reject as
/// larger than [`MAX_FRAME_LEN`].
pub fn write_frame(
    mut out: impl Write,
    payload: &[u8],
    mode: Compression,
) -> Result<(), FrameError> {
    if payload.is_empty() {
        return Err(FrameError::EmptyPayload);
    }
    // The reader caps the uncompressed size it accepts, so never declare more
    // than that; this also bounds every allocation below.
    if payload.len() > MAX_FRAME_LEN {
        return Err(FrameError::TooLong(payload.len(), MAX_FRAME_LEN));
    }

    let mut body = Vec::with_capacity(payload.len() + 8);
    match active_threshold(mode) {
        None => body.extend_from_slice(payload),
        Some(threshold) if payload.len() >= threshold => {
            write_varint(&mut body, payload.len() as i32)?;
            let mut encoder = ZlibEncoder::new(Vec::new(), ZlibLevel::default());
            encoder.write_all(payload)?;
            body.extend_from_slice(&encoder.finish()?);
        }
        Some(_) => {
            write_varint(&mut body, 0)?;
            body.extend_from_slice(payload);
        }
    }

    // The frame length is what the reader caps first, and compression can
    // widen the body slightly, so check the framed size too.
    if body.len() > MAX_FRAME_LEN {
        return Err(FrameError::TooLong(body.len(), MAX_FRAME_LEN));
    }
    write_varint(&mut out, body.len() as i32)?;
    out.write_all(&body)?;
    Ok(())
}

/// Reads one frame and returns the decompressed `Packet ID + Data` payload.
///
/// A negative threshold reads plain frames, matching [`write_frame`].
pub fn read_frame(mut input: impl Read, mode: Compression) -> Result<Vec<u8>, FrameError> {
    let frame_len = read_varint(&mut input)?;
    if frame_len < 0 {
        return Err(FrameError::NegativeLength(frame_len));
    }
    let frame_len = frame_len as usize;
    if frame_len > MAX_FRAME_LEN {
        return Err(FrameError::TooLong(frame_len, MAX_FRAME_LEN));
    }
    let mut body = vec![0u8; frame_len];
    input.read_exact(&mut body)?;

    // Plain framing: nothing follows the frame length but the payload itself,
    // so `Disabled` and a negative threshold return the body untouched.
    let Some(threshold) = active_threshold(mode) else {
        return Ok(body);
    };

    let mut cursor = &body[..];
    let data_len = read_varint(&mut cursor)?;
    if data_len < 0 {
        return Err(FrameError::NegativeLength(data_len));
    }
    if data_len == 0 {
        return Ok(cursor.to_vec());
    }
    // The peer supplies this length, so check it before sizing any buffer from
    // it. One past the frame limit is hostile; one below the active threshold
    // is a frame vanilla's decoder refuses.
    let declared_len = data_len as usize;
    if declared_len > MAX_FRAME_LEN {
        return Err(FrameError::BadCompression);
    }
    if declared_len < threshold {
        return Err(FrameError::BadCompression);
    }
    // Read one byte past the limit so a hostile stream cannot expand
    // without bound. A result larger than the limit necessarily fails
    // the comparison below, since the declaration is within the limit.
    let mut decoder = ZlibDecoder::new(cursor).take(MAX_FRAME_LEN as u64 + 1);
    let mut decoded = Vec::with_capacity(declared_len);
    decoder.read_to_end(&mut decoded)?;
    if decoded.len() != declared_len {
        return Err(FrameError::BadCompression);
    }
    Ok(decoded)
}
