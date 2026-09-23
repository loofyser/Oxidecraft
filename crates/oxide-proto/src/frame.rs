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
    /// The server sent a threshold. `-1` disables compression.
    Enabled {
        /// Minimum uncompressed `Packet ID + Data` size that gets compressed.
        threshold: i32,
    },
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
    /// The declared uncompressed size was not respected.
    #[error("decompressed frame does not match declared Data Length")]
    BadCompression,
}

/// Frames larger than this are rejected outright as hostile.
pub const MAX_FRAME_LEN: usize = 2 * 1024 * 1024 + 1024;

/// Writes one frame: VarInt frame length, then payload, compressing per `mode`.
pub fn write_frame(
    mut out: impl Write,
    payload: &[u8],
    mode: Compression,
) -> Result<(), FrameError> {
    let compress = match mode {
        Compression::Disabled => false,
        Compression::Enabled { threshold } => threshold >= 0 && payload.len() >= threshold as usize,
    };

    let mut body = Vec::with_capacity(payload.len() + 8);
    if compress {
        write_varint(&mut body, payload.len() as i32)?;
        let mut encoder = ZlibEncoder::new(Vec::new(), ZlibLevel::default());
        encoder.write_all(payload)?;
        body.extend_from_slice(&encoder.finish()?);
    } else if matches!(mode, Compression::Enabled { .. }) {
        write_varint(&mut body, 0)?;
        body.extend_from_slice(payload);
    } else {
        body.extend_from_slice(payload);
    }

    write_varint(&mut out, body.len() as i32)?;
    out.write_all(&body)?;
    Ok(())
}

/// Reads one frame and returns the decompressed `Packet ID + Data` payload.
pub fn read_frame(mut input: impl Read, mode: Compression) -> Result<Vec<u8>, FrameError> {
    let frame_len = read_varint(&mut input)?;
    let frame_len = frame_len.max(0) as usize;
    if frame_len > MAX_FRAME_LEN {
        return Err(FrameError::TooLong(frame_len, MAX_FRAME_LEN));
    }
    let mut body = vec![0u8; frame_len];
    input.read_exact(&mut body)?;

    match mode {
        Compression::Disabled => Ok(body),
        Compression::Enabled { .. } => {
            let mut cursor = &body[..];
            let data_len = read_varint(&mut cursor)?;
            if data_len == 0 {
                return Ok(cursor.to_vec());
            }
            // The peer supplies this length, so check it before sizing any
            // buffer from it; a negative value would cast to a huge size.
            let declared_len = usize::try_from(data_len).map_err(|_| FrameError::BadCompression)?;
            if declared_len > MAX_FRAME_LEN {
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
    }
}
