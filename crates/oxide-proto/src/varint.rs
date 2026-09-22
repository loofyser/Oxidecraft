//! Minecraft VarInt encoding: little-endian 7-bit groups, high bit means continue.

use std::io::{self, Read, Write};

/// Errors produced while decoding a VarInt.
#[derive(Debug, thiserror::Error)]
pub enum VarIntError {
    /// The stream ended in the middle of a VarInt.
    #[error("unexpected end of stream while reading VarInt")]
    UnexpectedEof,
    /// The encoding used more than five bytes.
    #[error("VarInt is longer than five bytes")]
    TooLong,
}

/// Writes `value` as a VarInt.
pub fn write_varint(mut out: impl Write, value: i32) -> io::Result<()> {
    let mut remaining = value as u32;
    loop {
        let byte = (remaining & 0x7f) as u8;
        remaining >>= 7;
        if remaining == 0 {
            return out.write_all(&[byte]);
        }
        out.write_all(&[byte | 0x80])?;
    }
}

/// Reads one VarInt from `input`.
pub fn read_varint(mut input: impl Read) -> Result<i32, VarIntError> {
    let mut result: u32 = 0;
    for index in 0..5 {
        let mut byte = [0u8; 1];
        input
            .read_exact(&mut byte)
            .map_err(|error| match error.kind() {
                io::ErrorKind::UnexpectedEof => VarIntError::UnexpectedEof,
                _ => VarIntError::UnexpectedEof,
            })?;
        result |= u32::from(byte[0] & 0x7f) << (7 * index);
        if byte[0] & 0x80 == 0 {
            return Ok(result as i32);
        }
    }
    Err(VarIntError::TooLong)
}
