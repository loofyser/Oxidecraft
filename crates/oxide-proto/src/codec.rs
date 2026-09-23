//! Primitive and string codecs: the fixed-width types, and length-prefixed UTF-8.

use std::io::{self, Read, Write};

use crate::varint::{VarIntError, read_varint, write_varint};

/// The protocol-wide ceiling on any string field, in bytes.
pub const MAX_STRING_BYTES: usize = 32767;

/// Errors from decoding a primitive or a string.
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    /// The stream ended in the middle of the value, or another IO failure occurred.
    #[error("io error while decoding: {0}")]
    Io(#[from] io::Error),
    /// A VarInt inside the value was malformed.
    #[error("bad varint: {0}")]
    VarInt(#[from] VarIntError),
    /// A length-prefixed value was longer than its cap allows.
    #[error("value of {len} bytes exceeds the {max} byte cap")]
    TooLong {
        /// The length the stream declared.
        len: usize,
        /// The cap in force.
        max: usize,
    },
    /// A declared length was negative.
    #[error("negative length: {0}")]
    NegativeLength(i32),
    /// The bytes were not valid UTF-8.
    #[error("string is not valid UTF-8")]
    BadUtf8,
}

/// Reads a fixed-width big-endian integer of the requested shape.
macro_rules! read_int {
    ($name:ident, $ty:ty, $width:expr) => {
        /// Reads a big-endian value.
        pub fn $name(input: impl Read) -> Result<$ty, CodecError> {
            let mut bytes = [0u8; $width];
            read_exact(input, &mut bytes)?;
            Ok(<$ty>::from_be_bytes(bytes))
        }
    };
}

read_int!(read_u16, u16, 2);
read_int!(read_i16, i16, 2);
read_int!(read_i32, i32, 4);
read_int!(read_i64, i64, 8);
read_int!(read_f32, f32, 4);
read_int!(read_f64, f64, 8);

/// Reads one byte.
pub fn read_u8(input: impl Read) -> Result<u8, CodecError> {
    let mut byte = [0u8; 1];
    read_exact(input, &mut byte)?;
    Ok(byte[0])
}

/// Reads a boolean: zero is false, anything else is true, as vanilla reads it.
pub fn read_bool(input: impl Read) -> Result<bool, CodecError> {
    Ok(read_u8(input)? != 0)
}

/// Reads a length-prefixed UTF-8 string, refusing anything over `max_bytes`.
///
/// The protocol ceiling [`MAX_STRING_BYTES`] always applies: a caller cap above
/// it is clamped down, so no string can exceed the protocol limit.
pub fn read_string(mut input: impl Read, max_bytes: usize) -> Result<String, CodecError> {
    let max_bytes = max_bytes.min(MAX_STRING_BYTES);
    let len = read_varint(&mut input)?;
    if len < 0 {
        return Err(CodecError::NegativeLength(len));
    }
    let len = len as usize;
    if len > max_bytes {
        return Err(CodecError::TooLong {
            len,
            max: max_bytes,
        });
    }
    let mut bytes = vec![0u8; len];
    read_exact(&mut input, &mut bytes)?;
    String::from_utf8(bytes).map_err(|_| CodecError::BadUtf8)
}

/// Reads a 16-byte UUID in the play-state wire form.
pub fn read_uuid(input: impl Read) -> Result<[u8; 16], CodecError> {
    let mut bytes = [0u8; 16];
    read_exact(input, &mut bytes)?;
    Ok(bytes)
}

/// Writes one byte.
pub fn write_u8(mut out: impl Write, value: u8) -> io::Result<()> {
    out.write_all(&value.to_be_bytes())
}

/// Writes a big-endian value.
pub fn write_u16(mut out: impl Write, value: u16) -> io::Result<()> {
    out.write_all(&value.to_be_bytes())
}

/// Writes a big-endian value.
pub fn write_i16(mut out: impl Write, value: i16) -> io::Result<()> {
    out.write_all(&value.to_be_bytes())
}

/// Writes a big-endian value.
pub fn write_i32(mut out: impl Write, value: i32) -> io::Result<()> {
    out.write_all(&value.to_be_bytes())
}

/// Writes a big-endian value.
pub fn write_i64(mut out: impl Write, value: i64) -> io::Result<()> {
    out.write_all(&value.to_be_bytes())
}

/// Writes a big-endian value.
pub fn write_f32(mut out: impl Write, value: f32) -> io::Result<()> {
    out.write_all(&value.to_be_bytes())
}

/// Writes a big-endian value.
pub fn write_f64(mut out: impl Write, value: f64) -> io::Result<()> {
    out.write_all(&value.to_be_bytes())
}

/// Writes a boolean as a single byte: 1 for true, 0 for false.
pub fn write_bool(mut out: impl Write, value: bool) -> io::Result<()> {
    out.write_all(&[u8::from(value)])
}

/// Writes a length-prefixed UTF-8 string.
///
/// A string longer than the protocol ceiling [`MAX_STRING_BYTES`] is refused
/// with [`io::ErrorKind::InvalidInput`] instead of being framed with a length
/// that would wrap.
pub fn write_string(mut out: impl Write, value: &str) -> io::Result<()> {
    if value.len() > MAX_STRING_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "string exceeds the protocol string ceiling",
        ));
    }
    write_varint(&mut out, value.len() as i32)?;
    out.write_all(value.as_bytes())
}

/// Writes a UUID in the play-state wire form.
pub fn write_uuid(mut out: impl Write, uuid: &[u8; 16]) -> io::Result<()> {
    out.write_all(uuid)
}

/// Reads exactly `bytes.len()` bytes, mapping a short read to `UnexpectedEof`.
fn read_exact(mut input: impl Read, bytes: &mut [u8]) -> Result<(), CodecError> {
    input.read_exact(bytes).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            CodecError::Io(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the stream ended inside a value",
            ))
        } else {
            CodecError::Io(error)
        }
    })
}
