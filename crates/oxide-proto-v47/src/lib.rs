//! All 74 clientbound and 26 serverbound play packets, plus handshake, status, and login states.

pub mod clientbound;
pub mod handshake;
pub mod serverbound;
pub mod status;

use oxide_proto::codec::CodecError;

/// The protocol version these codecs speak.
pub const PROTOCOL: i32 = 47;

/// The handshake `next_state` that begins a login.
pub const NEXT_STATE_LOGIN: i32 = 2;

/// Errors from decoding a packet payload.
#[derive(Debug, thiserror::Error)]
pub enum PacketError {
    /// A field was malformed or the payload ended early.
    #[error("malformed packet: {0}")]
    Codec(#[from] CodecError),
    /// The payload carried bytes the field list does not account for.
    #[error("{0} trailing byte(s) after the last field")]
    Trailing(usize),
    /// A chunk column's declared size does not match its mask.
    #[error("column data is {got} bytes, the mask and flags imply {expected}")]
    BadColumnSize {
        /// The size the stream declared.
        got: usize,
        /// The size the mask, sky flag and biome flag imply.
        expected: usize,
    },
}
